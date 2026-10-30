//! `dijkstravel render runs/<montage>` quand le dossier contient `montage.json` : les runs des
//! segments, déjà calculés et voisins du montage, rendus à la suite dans `data/out/<montage>/video.mp4`.

use std::path::{Path, PathBuf};

use dijkstravel_core::montage::Montage;
use dijkstravel_render::montage::Montage as Frames;
use dijkstravel_render::render::Frames as _;

use crate::console::Console;
use crate::load::{self, Failure, Outcome, RunPaths, fail};
use crate::render::{scene_of, threads_or_all, write_image};

pub fn is_montage(arg: &Path) -> bool {
    arg.join("montage.json").exists()
}

pub fn load(arg: &Path) -> Outcome<Montage> {
    let file = arg.join("montage.json");
    Montage::parse(&load::read_json(&file)?)
        .map_err(|issues| Failure(issues.iter().map(|issue| format!("{} : {issue}", file.display())).collect()))
}

pub fn render(arg: &Path, seconds: Option<f64>, threads: Option<usize>, frame: Option<u32>) -> Outcome {
    let montage = load(arg)?;
    let runs_dir = arg.parent().map_or_else(PathBuf::new, Path::to_owned);
    let mut scenes = Vec::new();
    let mut problems = Vec::new();
    for segment in &montage.segments {
        let dir = runs_dir.join(&segment.run);
        match load::run(&dir).and_then(|(run, paths)| scene_of(montage.segment_run(segment, run), &paths, &dir)) {
            Ok(scene) => scenes.push(scene),
            Err(Failure(messages)) => problems.extend(messages),
        }
    }
    if !problems.is_empty() {
        return Err(Failure(problems));
    }
    // Le thème du montage, lu une fois : chaque scène porte déjà son nom.
    let theme = load::theme(&scenes[0].run)?;
    let parts = scenes.iter().map(|scene| scene.renderer(theme.clone(), None)).collect::<Result<Vec<_>, _>>()?;
    let fade = (montage.fade_sec * f64::from(montage.fps)).round() as u32;
    let frames = Frames::new(parts, fade);
    // Les tuiles vont dans le cache commun : la première scène suffit à les télécharger.
    let scene = &scenes[0];
    let console = Console { fps: montage.fps };
    let out = RunPaths::of(arg, &montage.name).out;
    std::fs::create_dir_all(&out)?;
    if let Some(frame) = frame {
        let file = out.join(format!("frame-{frame}.png"));
        write_image(&scene.still(&frames, frame, true, &console)?, &file)?;
        println!("{}", file.display());
        return Ok(());
    }
    if frames.frame_count() == 0 {
        return fail("montage vide");
    }
    let video = out.join("video.mp4");
    scene.encode_video(&frames, seconds, threads_or_all(threads), &video.display().to_string(), &console)?;
    println!("{}", video.display());
    Ok(())
}
