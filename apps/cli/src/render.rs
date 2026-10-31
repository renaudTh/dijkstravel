//! `dijkstravel render runs/<nom>` → `data/out/<nom>/video.mp4`, par le moteur natif (sans GPU), à
//! partir du seul `tour.json`. Tout ce que lit le moteur est téléchargé avant, dans `data/cache/`.
//! La vue est à plat ; `--3d` la rend en relief, caméra inclinée, dans `video-3d.mp4`. `--duration n` rend une variante de n secondes de trajet, dans `video-<n>s.mp4` ; `--stop-slowdown n`
//! une variante qui freine autant aux lieux, dans `video-ralenti-<n>.mp4`. Un dossier qui contient
//! `montage.json` se rend comme un montage (`montage.rs`).

use std::path::{Path, PathBuf};

use dijkstravel_core::run::Run;
use dijkstravel_core::theme::Theme;
use dijkstravel_render::download::Downloader;
use dijkstravel_render::render::{Image, encode};
use dijkstravel_render::scene::Scene;

use crate::console::Console;
use crate::load::{self, Outcome, RunPaths, fail};
use crate::montage;

pub const CACHE_DIR: &str = "data/cache";
/// Inclinaison de `--3d` quand le thème n'en donne pas.
const RELIEF_PITCH: f64 = 55.0;

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
    Ok((scene_of(run, &paths, arg)?, paths))
}

/// La scène d'un run déjà lu, et peut-être retouché (durée, montage…).
pub fn scene_of(run: Run, paths: &RunPaths, arg: &Path) -> Outcome<Scene> {
    if !paths.tour.exists() {
        return fail(format!(
            "{} absent : calculer la tournée d'abord (`dijkstravel solve {}`).",
            paths.tour.display(),
            arg.display()
        ));
    }
    let tour = load::tour(paths)?;
    let places = load::places(&run, paths)?;
    let scene = Scene::new(run, tour, &places, Downloader::new(CACHE_DIR));
    for (id, url) in &scene.missing_images {
        eprintln!("Image introuvable pour {id} : {url}");
    }
    Ok(scene)
}

/// Réglages du run ou du thème forcés pour un rendu, sans toucher leurs fichiers.
#[derive(Clone, Copy)]
pub struct Variant {
    /// À la place de `durationSec` du run.
    pub duration: Option<f64>,
    /// À la place de `camera.stopSlowdown` du thème.
    pub stop_slowdown: Option<f64>,
    /// En relief : sinon la caméra reste à plat, quel que soit `camera.pitch` du thème.
    pub relief: bool,
}

impl Variant {
    /// Les options de `render` qui la produisent, ` --duration 60 --stop-slowdown 2 --3d`.
    pub fn options(&self) -> String {
        let duration = self.duration.map(|duration| format!(" --duration {duration}"));
        let slowdown = self.stop_slowdown.map(|slowdown| format!(" --stop-slowdown {slowdown}"));
        let relief = if self.relief { " --3d" } else { "" };
        duration.unwrap_or_default() + &slowdown.unwrap_or_default() + relief
    }

    /// Le thème avec les réglages forcés.
    pub fn apply(&self, theme: &mut Theme) {
        if let Some(slowdown) = self.stop_slowdown {
            theme.camera.stop_slowdown = slowdown;
        }
        theme.camera.pitch = match (self.relief, theme.camera.pitch) {
            (false, _) => 0.0,
            (true, pitch) if pitch > 0.0 => pitch,
            (true, _) => RELIEF_PITCH,
        };
    }

    /// Suffixe des fichiers rendus, `-60s-ralenti-2-3d`.
    fn suffix(&self) -> String {
        let duration = self.duration.map(|duration| format!("-{duration}s"));
        let slowdown = self.stop_slowdown.map(|slowdown| format!("-ralenti-{slowdown}"));
        let relief = if self.relief { "-3d" } else { "" };
        duration.unwrap_or_default() + &slowdown.unwrap_or_default() + relief
    }
}

/// `video.mp4`, ou la variante `video-<n>s-ralenti-<m>-3d.mp4` d'une durée, d'un ralentissement ou d'un
/// relief forcés.
pub fn video_file(paths: &RunPaths, variant: Variant) -> PathBuf {
    match variant.suffix() {
        suffix if suffix.is_empty() => paths.video.clone(),
        suffix => paths.out.join(format!("video{suffix}.mp4")),
    }
}

pub fn console(scene: &Scene) -> Console {
    Console { fps: scene.run.video.fps }
}

/// PNG ou JPEG d'après l'extension de `out`.
pub fn write_image(image: &Image, out: &Path) -> Outcome {
    let bytes = encode(image, &out.display().to_string()).ok_or("encodage impossible")?;
    load::write_file(out, bytes)
}

/// `frame` : l'image seule, et si elle porte la carte d'infos.
pub fn main(
    arg: &Path,
    seconds: Option<f64>,
    variant: Variant,
    threads: Option<usize>,
    frame: Option<(u32, bool)>,
) -> Outcome {
    let positive = |value: Option<f64>| value.is_none_or(|value| value.is_finite() && value > 0.0);
    if !positive(seconds) || !positive(variant.duration) || threads == Some(0) {
        return fail("--seconds et --duration attendent un nombre > 0, --threads un entier > 0");
    }
    if variant.stop_slowdown.is_some_and(|slowdown| !(0.0..=5.0).contains(&slowdown)) {
        return fail("--stop-slowdown attend un nombre de 0 à 5");
    }
    if montage::is_montage(arg) {
        if variant.duration.is_some() || variant.stop_slowdown.is_some() || variant.relief {
            return fail(
                "--duration, --stop-slowdown et --3d ne s'appliquent pas à un montage : régler chaque segment et son thème",
            );
        }
        return montage::render(arg, seconds, threads, frame.map(|(frame, _)| frame));
    }
    let (scene, paths) = scene_lasting(arg, variant.duration)?;
    let mut theme = load::theme(&scene.run)?;
    variant.apply(&mut theme);
    let renderer = scene.renderer(theme, None)?;
    std::fs::create_dir_all(&paths.out)?;
    let console = console(&scene);
    if let Some((frame, with_hud)) = frame {
        let out = paths.out.join(format!("frame-{frame}{}.png", if variant.relief { "-3d" } else { "" }));
        write_image(&scene.still(&renderer, frame, with_hud, &console)?, &out)?;
        println!("{}", out.display());
        return Ok(());
    }
    let threads = threads_or_all(threads);
    let video = video_file(&paths, variant);
    scene.encode_video(&renderer, seconds, threads, &video.display().to_string(), &console)?;
    println!("{}", video.display());
    Ok(())
}

pub fn threads_or_all(threads: Option<usize>) -> usize {
    threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(4, std::num::NonZero::get))
}
