//! `dijkstravel render runs/<nom>` → `data/out/<nom>/video.mp4`, par le moteur natif (sans GPU), à
//! partir du seul `tour.json`. Tout ce que lit le moteur est téléchargé avant, dans `data/cache/`.

use std::path::Path;

use dijkstravel_render::download::Downloader;
use dijkstravel_render::render::{Image, encode};
use dijkstravel_render::scene::Scene;

use crate::console::Console;
use crate::load::{self, Outcome, RunPaths, fail};

pub const CACHE_DIR: &str = "data/cache";

/// La scène du run, depuis ses fichiers ; les images de lieux introuvables sont signalées.
pub fn scene(arg: &Path) -> Outcome<(Scene, RunPaths)> {
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
    let scene = Scene::new(run, tour, &places, Downloader::new(CACHE_DIR));
    for (id, url) in &scene.missing_images {
        eprintln!("Image introuvable pour {id} : {url}");
    }
    Ok((scene, paths))
}

pub fn console(scene: &Scene) -> Console {
    Console { fps: scene.run.video.fps }
}

/// PNG ou JPEG d'après l'extension de `out`.
pub fn write_image(image: &Image, out: &Path) -> Outcome {
    let bytes = encode(image, &out.display().to_string()).ok_or("encodage impossible")?;
    load::write_file(out, bytes)
}

pub fn main(arg: &Path, seconds: Option<f64>, threads: Option<usize>, frame: Option<u32>) -> Outcome {
    if seconds.is_some_and(|seconds| seconds.is_nan() || seconds <= 0.0) || threads == Some(0) {
        return fail("--seconds attend un nombre > 0, --threads un entier > 0");
    }
    let (scene, paths) = scene(arg)?;
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
    scene.encode_video(&renderer, seconds, threads, &paths.video.display().to_string(), &console)?;
    println!("{}", paths.video.display());
    Ok(())
}
