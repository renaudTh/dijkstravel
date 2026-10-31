//! `dijkstravel render runs/<nom>` → `data/out/<nom>/video.mp4`, par le moteur natif (sans GPU), à
//! partir du seul `tour.json`. Tout ce que lit le moteur est téléchargé avant, dans `data/cache/`.
//! La vue est à plat ; `--3d` la rend en relief, caméra inclinée et un peu plus serrée, dans `video-3d.mp4`.
//! `--zoom n` rapproche la caméra du parcours n fois (`video-zoom-<n>.mp4`) ; `--no-hud` la rend sans la
//! carte d'infos (`video-sans-carte.mp4`). `--duration n` rend une variante de n secondes de trajet, dans
//! `video-<n>s.mp4` ; `--stop-slowdown n` une variante qui freine autant aux lieux, dans
//! `video-ralenti-<n>.mp4`. Un dossier qui contient
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
/// `--3d` resserre la caméra du parcours d'autant : l'inclinaison éloigne le sol au fond de l'écran.
const RELIEF_ZOOM: f64 = 1.15;

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
#[derive(Clone, Copy, Default)]
pub struct Variant {
    /// À la place de `durationSec` du run.
    pub duration: Option<f64>,
    /// À la place de `camera.stopSlowdown` du thème.
    pub stop_slowdown: Option<f64>,
    /// Ni ralenti aux lieux ni aux repères.
    pub no_slowdown: bool,
    /// En relief : sinon la caméra reste à plat, quel que soit `camera.pitch` du thème.
    pub relief: bool,
    /// Sans la carte d'infos (titre, distance, lieu, profil).
    pub no_hud: bool,
    /// Multiplie `camera.zoom` du thème : la caméra du parcours se rapproche d'autant, son lissage ne change pas.
    pub zoom: Option<f64>,
}

impl Variant {
    /// Les options de `render` qui la produisent, ` --duration 60 --stop-slowdown 2 --3d --zoom 1.5`.
    pub fn options(&self) -> String {
        let duration = self.duration.map(|duration| format!(" --duration {duration}"));
        let slowdown = self.stop_slowdown.map(|slowdown| format!(" --stop-slowdown {slowdown}"));
        let none = if self.no_slowdown { " --no-slowdown" } else { "" };
        let relief = if self.relief { " --3d" } else { "" };
        let zoom = self.zoom.map(|zoom| format!(" --zoom {zoom}"));
        let hud = if self.no_hud { " --no-hud" } else { "" };
        duration.unwrap_or_default() + &slowdown.unwrap_or_default() + none + relief + &zoom.unwrap_or_default() + hud
    }

    /// Le thème avec les réglages forcés.
    pub fn apply(&self, theme: &mut Theme) {
        if let Some(slowdown) = self.stop_slowdown {
            theme.camera.stop_slowdown = slowdown;
        }
        if self.no_slowdown {
            theme.camera.stop_slowdown = 0.0;
            theme.camera.marker_slowdown = 0.0;
        }
        theme.camera.pitch = match (self.relief, theme.camera.pitch) {
            (false, _) => 0.0,
            (true, pitch) if pitch > 0.0 => pitch,
            (true, _) => RELIEF_PITCH,
        };
        theme.camera.zoom *= self.zoom.unwrap_or(1.0) * if self.relief { RELIEF_ZOOM } else { 1.0 };
    }

    /// Suffixe d'une image seule (`--frame`) : relief et zoom ; sans carte, elle garde son nom.
    fn frame_suffix(&self) -> String {
        Variant { relief: self.relief, zoom: self.zoom, ..Variant::default() }.suffix()
    }

    /// Suffixe des fichiers rendus, `-60s-ralenti-2-3d-zoom-1.5-sans-carte`.
    fn suffix(&self) -> String {
        let duration = self.duration.map(|duration| format!("-{duration}s"));
        let slowdown = self.stop_slowdown.map(|slowdown| format!("-ralenti-{slowdown}"));
        let none = if self.no_slowdown { "-sans-ralenti" } else { "" };
        let relief = if self.relief { "-3d" } else { "" };
        let zoom = self.zoom.map(|zoom| format!("-zoom-{zoom}"));
        let hud = if self.no_hud { "-sans-carte" } else { "" };
        duration.unwrap_or_default() + &slowdown.unwrap_or_default() + none + relief + &zoom.unwrap_or_default() + hud
    }
}

/// `video.mp4`, ou la variante `video-<n>s-ralenti-<m>-3d-zoom-<z>-sans-carte.mp4` d'une durée, d'un
/// ralentissement, d'un relief, d'un zoom forcés ou sans carte d'infos.
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

/// `frame` : l'image seule.
pub fn main(arg: &Path, seconds: Option<f64>, variant: Variant, threads: Option<usize>, frame: Option<u32>) -> Outcome {
    let positive = |value: Option<f64>| value.is_none_or(|value| value.is_finite() && value > 0.0);
    if !positive(seconds) || !positive(variant.duration) || threads == Some(0) {
        return fail("--seconds et --duration attendent un nombre > 0, --threads un entier > 0");
    }
    if variant.stop_slowdown.is_some_and(|slowdown| !(0.0..=5.0).contains(&slowdown)) {
        return fail("--stop-slowdown attend un nombre de 0 à 5");
    }
    if variant.zoom.is_some_and(|zoom| !(0.25..=4.0).contains(&zoom)) {
        return fail("--zoom attend un nombre de 0.25 à 4");
    }
    if montage::is_montage(arg) {
        if variant.duration.is_some()
            || variant.stop_slowdown.is_some()
            || variant.no_slowdown
            || variant.relief
            || variant.zoom.is_some()
        {
            return fail(
                "--duration, --stop-slowdown, --no-slowdown, --3d et --zoom ne s'appliquent pas à un montage : régler chaque segment et son thème",
            );
        }
        return montage::render(arg, seconds, threads, frame, !variant.no_hud);
    }
    let (scene, paths) = scene_lasting(arg, variant.duration)?;
    let mut theme = load::theme(&scene.run)?;
    variant.apply(&mut theme);
    let renderer = scene.renderer(theme, None)?;
    std::fs::create_dir_all(&paths.out)?;
    let console = console(&scene);
    if let Some(frame) = frame {
        let out = paths.out.join(format!("frame-{frame}{}.png", variant.frame_suffix()));
        write_image(&scene.still(&renderer, frame, !variant.no_hud, &console)?, &out)?;
        println!("{}", out.display());
        return Ok(());
    }
    let threads = threads_or_all(threads);
    let video = video_file(&paths, variant);
    scene.encode_video(&renderer, seconds, !variant.no_hud, threads, &video.display().to_string(), &console)?;
    println!("{}", video.display());
    Ok(())
}

pub fn threads_or_all(threads: Option<usize>) -> usize {
    threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(4, std::num::NonZero::get))
}
