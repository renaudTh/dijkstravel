//! Importer un dépôt existant (`runs/`, `themes/`, et les sorties de `data/out/`) dans le studio. Un
//! run déjà connu du studio n'est pas touché : ce que le studio a produit depuis prime.

use std::path::{Path, PathBuf};

use anyhow::Context;
use bytes::Bytes;
use dijkstravel_core::places::parse_places;
use dijkstravel_core::run::Run;
use dijkstravel_core::theme::{Theme, is_theme_name};
use dijkstravel_core::tour::Tour;
use serde_json::{Value, json};

use super::Studio;
use crate::domain::artifact::ArtifactKind;
use crate::domain::ports::artifact;
use crate::domain::production::RunId;

#[derive(Debug, Default)]
pub struct ImportReport {
    pub runs: usize,
    pub skipped: usize,
    pub themes: usize,
    /// Ce qui n'a pas pu être importé, une ligne par fichier.
    pub problems: Vec<String>,
}

fn read_json(file: &Path) -> anyhow::Result<Value> {
    let text = std::fs::read_to_string(file).with_context(|| file.display().to_string())?;
    serde_json::from_str(&text).with_context(|| file.display().to_string())
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> =
        std::fs::read_dir(dir).into_iter().flatten().filter_map(|entry| entry.ok().map(|entry| entry.path())).collect();
    entries.sort();
    entries
}

impl Studio {
    /// `repo` : la racine d'un dépôt dijkstravel ; `out` : ses sorties (`data/out` par défaut).
    pub async fn import(&self, repo: &Path, out: &Path) -> anyhow::Result<ImportReport> {
        let mut report = ImportReport::default();
        for file in sorted_entries(&repo.join("themes")) {
            let Some(name) = file.file_stem().and_then(|name| name.to_str()).filter(|name| is_theme_name(name)) else {
                continue;
            };
            if self.catalog.theme(name).await?.is_some() {
                continue;
            }
            match read_json(&file)
                .and_then(|raw| Theme::parse(&raw).map(|_| raw).map_err(|issues| anyhow::anyhow!("{issues:?}")))
            {
                Ok(raw) => {
                    self.catalog.save_theme(name, &raw).await?;
                    report.themes += 1;
                }
                Err(error) => report.problems.push(format!("{} : {error:#}", file.display())),
            }
        }
        for dir in sorted_entries(&repo.join("runs")) {
            let file = dir.join("run.json");
            if !file.exists() {
                continue;
            }
            let spec = match read_json(&file) {
                Ok(spec) => spec,
                Err(error) => {
                    report.problems.push(format!("{error:#}"));
                    continue;
                }
            };
            let run = match Run::parse(&spec) {
                Ok(run) => run,
                Err(issues) => {
                    report.problems.push(format!("{} : {issues:?}", file.display()));
                    continue;
                }
            };
            if self.catalog.production(&run.name).await?.is_some() {
                report.skipped += 1;
                continue;
            }
            let id = self.catalog.save_run(&spec).await?;
            report.runs += 1;
            self.import_files(&run, id, &dir, &out.join(&run.name), &mut report).await?;
        }
        Ok(report)
    }

    async fn import_files(
        &self,
        run: &Run,
        id: RunId,
        dir: &Path,
        out: &Path,
        report: &mut ImportReport,
    ) -> anyhow::Result<()> {
        let places_file = dir.join(ArtifactKind::Places.file_name());
        if let Ok(text) = std::fs::read_to_string(&places_file) {
            match parse_places(&text, &run.bbox) {
                Ok(places) => {
                    self.import_text(id, ArtifactKind::Places, text, json!({ "places": places.len() })).await?
                }
                Err(issues) => report.problems.push(format!("{} : {} problèmes", places_file.display(), issues.len())),
            }
        }
        let tour_file = dir.join(ArtifactKind::Tour.file_name());
        if let Ok(text) = std::fs::read_to_string(&tour_file) {
            match Tour::parse(&text) {
                Ok(tour) => {
                    let meta = json!({ "stops": tour.stops.len(), "meters": tour.total_meters() });
                    self.import_text(id, ArtifactKind::Tour, text, meta).await?;
                }
                Err(error) => report.problems.push(format!("{} : {error}", tour_file.display())),
            }
        }
        for kind in [ArtifactKind::Video, ArtifactKind::Cover, ArtifactKind::Publication] {
            let file = out.join(kind.file_name());
            if file.exists() {
                let bytes = self.store.put_file(&kind.key(id), &file).await?;
                self.catalog.record_artifact(id, &artifact(kind, id, bytes, json!({})), None).await?;
            }
        }
        Ok(())
    }

    async fn import_text(&self, id: RunId, kind: ArtifactKind, text: String, meta: Value) -> anyhow::Result<()> {
        let bytes = self.store.put(&kind.key(id), Bytes::from(text)).await?;
        self.catalog.record_artifact(id, &artifact(kind, id, bytes, meta), None).await
    }
}
