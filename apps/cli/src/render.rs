//! `dijkstravel render runs/<nom>` → `data/out/<nom>/video.mp4`, par le moteur natif (sans GPU), à
//! partir du seul `tour.json`. Tout ce que lit le moteur est téléchargé avant, dans `data/cache/`.
//! La vue est à plat ; `--3d` la rend en relief, caméra inclinée et un peu plus serrée, dans `video-3d.mp4`.
//! `--zoom n` rapproche la caméra du parcours n fois (`video-zoom-<n>.mp4`) ; `--no-hud` la rend sans la
//! carte d'infos (`video-sans-carte.mp4`) ; `--width l --height h` en l × h pixels (`video-<l>x<h>.mp4`) ;
//! `--fps n` à n images par seconde (`video-<n>fps.mp4`) ; `--sun a,b`, avec `--3d`, éclairé par le soleil
//! de la position a à la position b de sa course du jour (`video-3d-soleil-<a>-<b>.mp4`). `--duration n` rend une variante de n secondes de trajet, dans
//! `video-<n>s.mp4` ; `--stop-slowdown n` une variante qui freine autant aux lieux, dans
//! `video-ralenti-<n>.mp4`. Un dossier qui contient
//! `montage.json` se rend comme un montage (`montage.rs`).

use std::path::{Path, PathBuf};

use dijkstravel_core::run::Run;
use dijkstravel_core::schedule::Schedule;
use dijkstravel_core::theme::Theme;
use dijkstravel_render::download::Downloader;
use dijkstravel_render::model::Clock;
use dijkstravel_render::render::{Image, encode};
use dijkstravel_render::scene::Scene;

use crate::clock;
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
    scene_variant(arg, Variant::default())
}

/// La durée du trajet (`durationSec`), la cadence (`fps`) et la taille de la variante à la place de celles du run ; l'intro et
/// la fin ne changent pas.
fn scene_variant(arg: &Path, variant: Variant) -> Outcome<(Scene, RunPaths)> {
    let (mut run, paths) = load::run(arg)?;
    if let Some(duration) = variant.duration {
        run.video.duration_sec = duration;
    }
    if let Some(fps) = variant.fps {
        run.video.fps = fps;
    }
    let mut scene = scene_of(run, &paths, arg)?;
    if let Some((width, height)) = variant.size {
        scene.resize(width, height);
    }
    Ok((scene, paths))
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
    /// Largeur et hauteur en pixels, à la place du format du run.
    pub size: Option<(u32, u32)>,
    /// Images par seconde, à la place de `fps` du run.
    pub fps: Option<u32>,
    /// Relief éclairé par le soleil (`camera.sunlight` à 1), de cette position sur sa course à
    /// celle-ci (`camera.sunFrom`, `camera.sunTo`), avec `--3d`.
    pub sun: Option<(f64, f64)>,
    /// Horaires à la place de `schedule` du run, avec `--3d` : le vrai soleil et la vraie lune.
    pub schedule: Option<Schedule>,
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
        let sun = self.sun.map(|(from, to)| format!(" --sun {from},{to}"));
        let schedule =
            self.schedule.map(|schedule| format!(" --start-time {} --finish-time {}", schedule.start, schedule.finish));
        let hud = if self.no_hud { " --no-hud" } else { "" };
        let size = self.size.map(|(width, height)| format!(" --width {width} --height {height}"));
        let fps = self.fps.map(|fps| format!(" --fps {fps}"));
        duration.unwrap_or_default()
            + &slowdown.unwrap_or_default()
            + none
            + relief
            + &zoom.unwrap_or_default()
            + &sun.unwrap_or_default()
            + &schedule.unwrap_or_default()
            + hud
            + &size.unwrap_or_default()
            + &fps.unwrap_or_default()
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
        if let Some((from, to)) = self.sun {
            theme.camera.sunlight = 1.0;
            theme.camera.sun_from = from;
            theme.camera.sun_to = to;
        }
        theme.camera.zoom *= self.zoom.unwrap_or(1.0) * if self.relief { RELIEF_ZOOM } else { 1.0 };
    }

    /// Suffixe d'une image seule (`--frame`) : relief, zoom, taille et cadence (l'image n n'est pas au même
    /// instant) ; sans carte, elle garde son nom.
    fn frame_suffix(&self) -> String {
        Variant {
            relief: self.relief,
            zoom: self.zoom,
            sun: self.sun,
            schedule: self.schedule,
            size: self.size,
            fps: self.fps,
            ..Variant::default()
        }
        .suffix()
    }

    /// Suffixe des fichiers rendus, `-60s-ralenti-2-3d-zoom-1.5-sans-carte-1080x1080-60fps`.
    fn suffix(&self) -> String {
        let duration = self.duration.map(|duration| format!("-{duration}s"));
        let slowdown = self.stop_slowdown.map(|slowdown| format!("-ralenti-{slowdown}"));
        let none = if self.no_slowdown { "-sans-ralenti" } else { "" };
        let relief = if self.relief { "-3d" } else { "" };
        let zoom = self.zoom.map(|zoom| format!("-zoom-{zoom}"));
        let sun = self.sun.map(|(from, to)| format!("-soleil-{from}-{to}"));
        let schedule = self
            .schedule
            .map(|schedule| format!("-horaire-{}-{}", schedule.start.compact(), schedule.finish.compact()));
        let hud = if self.no_hud { "-sans-carte" } else { "" };
        let size = self.size.map(|(width, height)| format!("-{width}x{height}"));
        let fps = self.fps.map(|fps| format!("-{fps}fps"));
        duration.unwrap_or_default()
            + &slowdown.unwrap_or_default()
            + none
            + relief
            + &zoom.unwrap_or_default()
            + &sun.unwrap_or_default()
            + &schedule.unwrap_or_default()
            + hud
            + &size.unwrap_or_default()
            + &fps.unwrap_or_default()
    }
}

/// `video.mp4`, ou la variante `video-<n>s-ralenti-<m>-3d-zoom-<z>-sans-carte-<l>x<h>.mp4` d'une durée,
/// d'un ralentissement, d'un relief, d'un zoom ou d'une taille forcés, ou sans carte d'infos.
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
    let side = |side: u32| (16..=7680).contains(&side) && side.is_multiple_of(2);
    if variant.size.is_some_and(|(width, height)| !side(width) || !side(height)) {
        return fail("--width et --height attendent des entiers pairs de 16 à 7680 (H.264 en 4:2:0)");
    }
    if variant.fps.is_some_and(|fps| !(1..=120).contains(&fps)) {
        return fail("--fps attend un entier de 1 à 120");
    }
    if variant.sun.is_some() && !variant.relief {
        return fail("--sun éclaire le relief : il va avec --3d");
    }
    if variant.schedule.is_some() && !variant.relief {
        return fail("--start-time et --finish-time éclairent le relief : ils vont avec --3d");
    }
    if variant.schedule.is_some_and(|schedule| schedule.finish <= schedule.start) {
        return fail("--finish-time attend une heure après --start-time");
    }
    if variant.sun.is_some_and(|(from, to)| ![from, to].iter().all(|angle| (1.0..=359.0).contains(angle))) {
        return fail(
            "--sun attend deux angles de 1 à 359 (0 : lever, 90 : midi, 180 : coucher, 270 : minuit), --sun 20,140",
        );
    }
    if montage::is_montage(arg) {
        if variant.duration.is_some()
            || variant.stop_slowdown.is_some()
            || variant.no_slowdown
            || variant.relief
            || variant.zoom.is_some()
            || variant.sun.is_some()
            || variant.schedule.is_some()
            || variant.size.is_some()
            || variant.fps.is_some()
        {
            return fail(
                "--duration, --stop-slowdown, --no-slowdown, --3d, --zoom, --sun, --start-time, --finish-time, --width, --height et --fps ne s'appliquent pas à un montage : régler chaque segment et son thème",
            );
        }
        return montage::render(arg, seconds, threads, frame, !variant.no_hud);
    }
    let (mut scene, paths) = scene_variant(arg, variant)?;
    let mut theme = load::theme(&scene.run)?;
    variant.apply(&mut theme);
    // En relief, sauf course stylisée du soleil (`--sun`) : les horaires de la variante, sinon les heures de
    // passage d'une activité enregistrée, sinon les horaires du run.
    if variant.relief && variant.sun.is_none() {
        let path = &scene.tour.path;
        let (start, finish) = (path.points[0], path.points[path.points.len() - 1]);
        scene.clock = match variant.schedule {
            Some(schedule) => Some(clock::resolve(schedule, start, finish, path.total_meters())?),
            None => match Clock::along(scene.tour.timeline.clone()) {
                Some(recorded) => Some(recorded),
                None => scene
                    .run
                    .schedule
                    .map(|schedule| clock::resolve(schedule, start, finish, path.total_meters()))
                    .transpose()?,
            },
        };
        if scene.clock.is_some() && theme.camera.sunlight <= 0.0 {
            theme.camera.sunlight = 1.0;
        }
    }
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
