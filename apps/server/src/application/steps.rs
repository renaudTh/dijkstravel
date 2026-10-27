//! Les étapes du pipeline : chacune charge ses entrées depuis les ports, appelle le domaine (libs
//! `sources`, `solve`, `render`, `core`) hors du runtime async, puis range ce qu'elle a produit.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, anyhow, bail};
use bytes::Bytes;
use dijkstravel_core::json::to_js_string;
use dijkstravel_core::places::{Place, parse_places};
use dijkstravel_core::publication::sheet;
use dijkstravel_core::run::Run;
use dijkstravel_core::theme::Theme;
use dijkstravel_core::tour::Tour;
use dijkstravel_render::download::Downloader;
use dijkstravel_render::monitor::{Monitor, Stage};
use dijkstravel_render::render::encode;
use dijkstravel_render::scene::Scene;
use dijkstravel_sources::{finalize, overpass};
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use super::Studio;
use crate::domain::artifact::ArtifactKind;
use crate::domain::job::{JobId, JobKind, Progress, Step};
use crate::domain::ports::artifact;
use crate::domain::production::RunProduction;

/// Ce qu'une étape remonte pendant qu'elle tourne.
#[derive(Debug)]
pub enum Signal {
    Progress(Progress),
    Note(String),
}

/// Le lien d'une étape avec son job : avancement, faits notables, demande d'arrêt.
#[derive(Clone)]
pub struct StepContext {
    pub job: JobId,
    pub signals: UnboundedSender<Signal>,
    pub cancel: Arc<AtomicBool>,
    pub scratch: PathBuf,
}

impl StepContext {
    fn note(&self, line: impl Into<String>) {
        let _ = self.signals.send(Signal::Note(line.into()));
    }

    fn progress(&self, step: Step, stage: Option<Stage>, done: u64, total: u64) {
        let stage = stage.map(|stage| stage.as_str().to_owned());
        let _ = self.signals.send(Signal::Progress(Progress { step, stage, done, total }));
    }

    fn monitor(&self, step: Step) -> JobMonitor {
        JobMonitor { context: self.clone(), step }
    }
}

/// Le port `Monitor` du rendu, branché sur le job.
struct JobMonitor {
    context: StepContext,
    step: Step,
}

impl Monitor for JobMonitor {
    fn progress(&self, stage: Stage, done: u64, total: u64) {
        self.context.progress(self.step, Some(stage), done, total);
    }

    fn cancelled(&self) -> bool {
        self.context.cancel.load(Ordering::Relaxed)
    }
}

/// Le domaine est synchrone (HTTP bloquant, rayon) : il tourne sur un thread à part.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(work).await.context("étape interrompue")?
}

fn places_of(run: &Run, text: &str) -> anyhow::Result<Vec<Place>> {
    parse_places(text, &run.bbox).map_err(|issues| {
        anyhow!("places.geojson : {}", issues.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ; "))
    })
}

impl Studio {
    /// `kind` : le job dont l'étape fait partie (durée d'un rendu partiel).
    pub async fn run_step(
        &self,
        step: Step,
        kind: &JobKind,
        production: &RunProduction,
        context: &StepContext,
    ) -> anyhow::Result<()> {
        context.progress(step, None, 0, 1);
        match step {
            Step::Source => self.source(production, context).await?,
            Step::Solve => self.solve(production, context).await?,
            Step::Render => {
                let seconds = if let JobKind::Render { seconds } = kind { *seconds } else { None };
                self.render(production, context, seconds).await?;
            }
            Step::Publication => self.publication(production, context).await?,
        }
        context.progress(step, None, 1, 1);
        Ok(())
    }

    async fn text(&self, production: &RunProduction, kind: ArtifactKind) -> anyhow::Result<String> {
        let artifact = production.artifacts.get(&kind).with_context(|| format!("{} absent", kind.file_name()))?;
        let bytes = self.store.get(&artifact.key).await?;
        String::from_utf8(bytes.to_vec()).with_context(|| format!("{} : pas de l'UTF-8", kind.file_name()))
    }

    async fn keep(
        &self,
        production: &RunProduction,
        context: &StepContext,
        kind: ArtifactKind,
        body: Bytes,
        meta: Value,
    ) -> anyhow::Result<()> {
        let bytes = self.store.put(&kind.key(production.id), body).await?;
        self.catalog
            .record_artifact(production.id, &artifact(kind, production.id, bytes, meta), Some(context.job))
            .await
    }

    /// Le thème du run, validé ; absent du catalogue, le thème par défaut.
    async fn theme(&self, run: &Run) -> anyhow::Result<Theme> {
        let raw = self.catalog.theme(&run.theme).await?.unwrap_or_else(|| json!({}));
        Theme::parse(&raw).map_err(|issues| {
            anyhow!("thème {} : {}", run.theme, issues.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ; "))
        })
    }

    async fn scene(&self, production: &RunProduction, context: &StepContext) -> anyhow::Result<Scene> {
        let (tour, places) =
            (self.text(production, ArtifactKind::Tour).await?, self.text(production, ArtifactKind::Places).await?);
        let (run, cache_dir) = (production.run.clone(), self.settings.cache_dir.clone());
        let scene = blocking(move || {
            let places = places_of(&run, &places)?;
            let tour = Tour::parse(&tour).map_err(|e| anyhow!("tour.json : {e}"))?;
            Ok(Scene::new(run, tour, &places, Downloader::new(cache_dir)))
        })
        .await?;
        for (id, url) in &scene.missing_images {
            context.note(format!("Image introuvable pour {id} : {url}"));
        }
        Ok(scene)
    }

    async fn source(&self, production: &RunProduction, context: &StepContext) -> anyhow::Result<()> {
        let (run, url) = (production.run.clone(), self.settings.overpass_url.clone());
        let (text, count, elements, unnamed) = blocking(move || {
            let source = run.overpass.as_ref().context("aucune section overpass")?;
            let elements = overpass::parse_response(
                &overpass::fetch(&url, &overpass::build_query(source, &run.bbox)).map_err(|e| anyhow!(e))?,
            )
            .map_err(|e| anyhow!(e))?;
            let found = overpass::places_from_elements(&elements).within(&run.bbox);
            let (text, places) = finalize(&found.places, &run.bbox)
                .map_err(|issues| anyhow!(issues.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ; ")))?;
            Ok((text, places.len(), elements.len(), found.unnamed))
        })
        .await?;
        if !unnamed.is_empty() {
            context.note(format!("{} lieux sans nom écartés : {}", unnamed.len(), unnamed.join(", ")));
        }
        context.note(format!("{count} lieux ({elements} éléments OSM)"));
        self.keep(production, context, ArtifactKind::Places, Bytes::from(text), json!({ "places": count })).await
    }

    async fn solve(&self, production: &RunProduction, context: &StepContext) -> anyhow::Result<()> {
        let places = self.text(production, ArtifactKind::Places).await?;
        let profile = production.run.profile;
        let routing =
            self.routing.routing(profile).with_context(|| format!("aucun routage configuré pour {profile:?}"))?;
        let run = production.run.clone();
        let tour = blocking(move || {
            let places = places_of(&run, &places)?;
            dijkstravel_solve::solve_tour(&run, &places, routing.as_ref()).map_err(|e| anyhow!(e.to_string()))
        })
        .await?;
        for place in &tour.excluded {
            context.note(format!("Inaccessible par la route, exclu : {} ({})", place.id, place.name));
        }
        let meta = json!({ "stops": tour.stops.len(), "meters": tour.total_meters() });
        context.note(format!("{} lieux, {:.1} km", tour.stops.len(), tour.total_meters() / 1000.0));
        self.keep(production, context, ArtifactKind::Tour, Bytes::from(to_js_string(&tour)), meta).await
    }

    async fn render(
        &self,
        production: &RunProduction,
        context: &StepContext,
        seconds: Option<f64>,
    ) -> anyhow::Result<()> {
        let scene = self.scene(production, context).await?;
        let theme = self.theme(&production.run).await?;
        let kind = ArtifactKind::rendered(seconds);
        let out = context.scratch.join(kind.file_name());
        let (threads, monitor, file) = (self.settings.render_threads, context.monitor(Step::Render), out.clone());
        blocking(move || {
            let renderer = scene.renderer(theme, None).map_err(|e| anyhow!(e))?;
            scene
                .encode_video(&renderer, seconds, threads, &file.display().to_string(), &monitor)
                .map_err(|e| anyhow!(e))
        })
        .await?;
        if context.cancel.load(Ordering::Relaxed) {
            bail!(dijkstravel_render::monitor::CANCELLED);
        }
        let bytes = self.store.put_file(&kind.key(production.id), &out).await?;
        let meta = json!({ "seconds": seconds });
        self.catalog
            .record_artifact(
                production.id,
                &artifact(kind, production.id, bytes, meta),
                Some(context.job),
            )
            .await
    }

    async fn publication(&self, production: &RunProduction, context: &StepContext) -> anyhow::Result<()> {
        let publication =
            production.run.publication.clone().context("aucune section publication (caption, hashtags)")?;
        let scene = self.scene(production, context).await?;
        let theme = self.theme(&production.run).await?;
        let monitor = context.monitor(Step::Publication);
        let (cover, text) = blocking(move || {
            let renderer = scene.renderer(theme, None).map_err(|e| anyhow!(e))?;
            let image = scene.cover(&renderer, &monitor).map_err(|e| anyhow!(e))?;
            let cover = encode(&image, "cover.jpg").context("encodage de la couverture impossible")?;
            Ok((cover, publication))
        })
        .await?;
        let video = if production.has(ArtifactKind::Video) {
            "Vidéo : video.mp4, à télécharger depuis le studio".to_owned()
        } else {
            "Vidéo : pas encore rendue → « Rendre » dans le studio".to_owned()
        };
        let files = [
            video,
            "Couverture : cover.jpg (tracé complet dézoomé, sans la carte de titre, à choisir comme miniature)"
                .to_owned(),
        ];
        let tour =
            Tour::parse(&self.text(production, ArtifactKind::Tour).await?).map_err(|e| anyhow!("tour.json : {e}"))?;
        let sheet = sheet(&text, &tour, &files);
        self.keep(production, context, ArtifactKind::Cover, Bytes::from(cover), json!({})).await?;
        self.keep(production, context, ArtifactKind::Publication, Bytes::from(sheet), json!({})).await
    }
}
