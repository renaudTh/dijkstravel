//! `dijkstravel render runs/<nom>` → `data/out/<nom>/video.mp4`, par le moteur natif (sans GPU), à
//! partir du seul `tour.json`. Tout ce que lit le moteur est téléchargé avant, dans `data/cache/`.

use std::path::Path;
use std::sync::Arc;

use dijkstravel_core::run::{Run, RunPaths};
use dijkstravel_core::theme::Theme;
use dijkstravel_render::download::{self, Downloader};
use dijkstravel_render::model::{Input, Video};
use dijkstravel_render::render::{Renderer, encode};
use dijkstravel_render::video::encode_video;

use crate::load::{self, Outcome, fail};

pub const CACHE_DIR: &str = "data/cache";

/// Ce que le rendu d'un thème lit hors du thème : lu une fois, l'éditeur en rend plusieurs thèmes.
pub struct Scene {
    pub run: Run,
    pub paths: RunPaths,
    pub tour: Arc<dijkstravel_core::tour::Tour>,
    title: String,
    images: std::collections::HashMap<String, String>,
    pub downloader: Downloader,
}

impl Scene {
    pub fn load(arg: &Path) -> Outcome<Scene> {
        let (run, paths) = load::run(arg)?;
        if !paths.tour.exists() {
            return fail(format!(
                "{} absent : calculer la tournée d'abord (`dijkstravel solve {}`).",
                paths.tour.display(),
                arg.display()
            ));
        }
        let tour = load::tour(&paths)?;
        let places = load::places(&run, &paths)?;
        let downloader = Downloader::new(CACHE_DIR);
        let images = downloader.place_images(&places);
        let title = run.title.clone().unwrap_or_else(|| format!("{} lieux · {}", tour.stops.len(), run.name));
        Ok(Scene { run, paths, tour: Arc::new(tour), title, images, downloader })
    }

    pub fn input(&self, theme: Theme) -> Outcome<Input> {
        let video = &self.run.video;
        let (width, height) = video.format.size();
        Ok(Input {
            video: Video {
                width,
                height,
                duration_sec: video.duration_sec,
                intro_sec: video.intro_sec,
                outro_sec: video.outro_sec,
                fps: video.fps,
            },
            tour_file: self.paths.tour.display().to_string(),
            title: self.title.clone(),
            images: self.images.clone(),
            imagery: download::imagery_source(&theme.basemap.imagery),
            terrain: download::terrain_source(),
            vector: download::vector_source(),
            tile_dir: self.downloader.tile_dir().display().to_string(),
            hud_fonts: self.downloader.hud_fonts(&theme.hud.font)?,
            theme,
        })
    }

    /// Le rendu du thème du run.
    pub fn renderer(&self) -> Outcome<Renderer> {
        Ok(Renderer::new(self.input(load::theme(&self.run)?)?, self.tour.clone(), None))
    }

    /// Télécharge les tuiles que liront les images `frames`.
    pub fn prefetch(&self, renderer: &Renderer, frames: std::ops::Range<u32>, progress: bool) -> Outcome {
        self.downloader.fetch_tiles(&renderer.needed_tiles(frames), progress)?;
        Ok(())
    }
}

/// Une image en PNG ou JPEG, d'après l'extension de `out` ; `with_hud` : avec la carte d'infos.
pub fn write_frame(renderer: &Renderer, frame: u32, out: &Path, with_hud: bool) -> Outcome {
    let image = renderer.render(frame.min(renderer.frame_count().saturating_sub(1)), with_hud);
    let bytes = encode(&image, &out.display().to_string()).ok_or("encodage impossible")?;
    load::write_file(out, bytes)
}

pub fn main(arg: &Path, seconds: Option<f64>, threads: Option<usize>, frame: Option<u32>) -> Outcome {
    if seconds.is_some_and(|seconds| seconds.is_nan() || seconds <= 0.0) || threads == Some(0) {
        return fail("--seconds attend un nombre > 0, --threads un entier > 0");
    }
    let scene = Scene::load(arg)?;
    let renderer = scene.renderer()?;
    std::fs::create_dir_all(&scene.paths.out)?;
    if let Some(frame) = frame {
        scene.prefetch(&renderer, frame..frame + 1, true)?;
        let out = scene.paths.out.join(format!("frame-{frame}.png"));
        write_frame(&renderer, frame, &out, true)?;
        println!("{}", out.display());
        return Ok(());
    }
    let all = renderer.frame_count();
    let frames = seconds.map_or(all, |seconds| ((seconds * f64::from(scene.run.video.fps)).round() as u32).min(all));
    scene.prefetch(&renderer, 0..frames, true)?;
    let threads = threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(4, std::num::NonZero::get));
    encode_video(&renderer, frames, threads, &scene.paths.video.display().to_string())?;
    println!("{}", scene.paths.video.display());
    Ok(())
}
