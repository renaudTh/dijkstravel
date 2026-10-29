//! `dijkstravel render runs/<nom>` → `data/out/<nom>/video.mp4`, par le moteur natif (sans GPU), à
//! partir du seul `tour.json`. Tout ce que lit le moteur est téléchargé avant, dans `data/cache/`.
//! `--duration n` rend une variante de n secondes de trajet, dans `video-<n>s.mp4`.

use std::path::{Path, PathBuf};

use dijkstravel_render::download::Downloader;
use dijkstravel_render::render::{Image, encode};
use dijkstravel_render::scene::Scene;

use crate::console::Console;
use crate::load::{self, Outcome, RunPaths, fail};

pub const CACHE_DIR: &str = "data/cache";

/// La scène du run, depuis ses fichiers ; les images de lieux introuvables sont signalées.
pub fn scene(arg: &Path) -> Outcome<(Scene, RunPaths)> {
    scene_lasting(arg, None)
}

/// `duration` remplace la durée du trajet (`durationSec`) du run ; l'intro et la fin ne changent pas.
fn scene_lasting(arg: &Path, duration: Option<f64>) -> Outcome<(Scene, RunPaths)> {
    let (mut run, paths) = load::run(arg)?;
    if let Some(duration) = duration {
        run.video.duration_sec = duration;
    }
    if !paths.tour.exists() {
        return fail(format!(
            "{} absent : calculer la tournée d'abord (`dijkstravel solve {}`).",
            paths.tour.display(),
            arg.display()
        ));
    }
    let tour = load::tour(&paths)?;
    let places = load::places(&run, &paths)?;
    let scene = Scene::new(run, tour, &places, Downloader::new(CACHE_DIR));
    for (id, url) in &scene.missing_images {
        eprintln!("Image introuvable pour {id} : {url}");
    }
    Ok((scene, paths))
}

/// `video.mp4`, ou la variante `video-<n>s.mp4` d'une durée de trajet forcée.
pub fn video_file(paths: &RunPaths, duration: Option<f64>) -> PathBuf {
    duration.map_or_else(|| paths.video.clone(), |duration| paths.out.join(format!("video-{duration}s.mp4")))
}

pub fn console(scene: &Scene) -> Console {
    Console { fps: scene.run.video.fps }
}

/// PNG ou JPEG d'après l'extension de `out`.
pub fn write_image(image: &Image, out: &Path) -> Outcome {
    let bytes = encode(image, &out.display().to_string()).ok_or("encodage impossible")?;
    load::write_file(out, bytes)
}

pub fn main(
    arg: &Path,
    seconds: Option<f64>,
    duration: Option<f64>,
    threads: Option<usize>,
    frame: Option<u32>,
) -> Outcome {
    let positive = |value: Option<f64>| value.is_none_or(|value| value.is_finite() && value > 0.0);
    if !positive(seconds) || !positive(duration) || threads == Some(0) {
        return fail("--seconds et --duration attendent un nombre > 0, --threads un entier > 0");
    }
    let (scene, paths) = scene_lasting(arg, duration)?;
    let renderer = scene.renderer(load::theme(&scene.run)?, None)?;
    std::fs::create_dir_all(&paths.out)?;
    let console = console(&scene);
    if let Some(frame) = frame {
        let out = paths.out.join(format!("frame-{frame}.png"));
        write_image(&scene.still(&renderer, frame, true, &console)?, &out)?;
        println!("{}", out.display());
        return Ok(());
    }
    let threads = threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(4, std::num::NonZero::get));
    let video = video_file(&paths, duration);
    scene.encode_video(&renderer, seconds, threads, &video.display().to_string(), &console)?;
    println!("{}", video.display());
    Ok(())
}
