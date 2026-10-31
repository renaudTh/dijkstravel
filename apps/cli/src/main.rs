//! `dijkstravel <commande> runs/<nom>` : toute la chaîne, des lieux à la vidéo publiée.

mod clock;
mod console;
mod editor;
mod import;
mod load;
mod montage;
mod publication;
mod render;
mod solve;
mod source;
mod upload;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use dijkstravel_core::schedule::{LocalDateTime, Schedule};

use crate::load::Outcome;

#[derive(Parser)]
#[command(name = "dijkstravel", about = "Le plus court chemin entre tous les lieux d'un run, en vidéo")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Valide le run et ses lieux (`places.geojson`).
    Validate {
        /// `runs/<nom>` ou `runs/<nom>/run.json`.
        run: PathBuf,
    },
    /// Crée un run complet depuis une activité enregistrée (GPX) : tracé, départ et arrivée, cols et
    /// sommets franchis, heures de passage.
    Import(import::Import),
    /// Calcule la tournée (`tour.json`) par OSRM et VROOM.
    Solve { run: PathBuf },
    /// Rend la vidéo (`data/out/<nom>/video.mp4`) à partir du seul `tour.json`.
    Render {
        run: PathBuf,
        /// Ne rend que les n premières secondes, pour itérer vite.
        #[arg(long)]
        seconds: Option<f64>,
        /// Durée du trajet en secondes, à la place de `durationSec` ; écrit `video-<n>s.mp4`.
        #[arg(long)]
        duration: Option<f64>,
        /// Ralentissement aux lieux, à la place de `camera.stopSlowdown` du thème ; écrit
        /// `video-ralenti-<n>.mp4` (`video-<d>s-ralenti-<n>.mp4` avec `--duration d`).
        #[arg(long)]
        stop_slowdown: Option<f64>,
        /// Sans aucun ralenti, ni aux lieux ni aux repères (`camera.stopSlowdown` et `markerSlowdown` à 0) ;
        /// écrit `video-sans-ralenti.mp4`.
        #[arg(long, conflicts_with = "stop_slowdown")]
        no_slowdown: bool,
        /// Threads de rendu (tous les cœurs par défaut).
        #[arg(long)]
        threads: Option<usize>,
        /// L'image n seule, dans `data/out/<nom>/frame-<n>.png`.
        #[arg(long)]
        frame: Option<u32>,
        /// Sans la carte d'infos (titre, distance, lieu, profil) ; écrit `video-sans-carte.mp4`. Avec
        /// `--frame`, l'image garde son nom (vignettes, couvertures composées à la main).
        #[arg(long)]
        no_hud: bool,
        /// En relief, caméra inclinée (`camera.pitch` du thème, 55° s'il n'en a pas) et un peu plus serrée ;
        /// écrit `video-3d.mp4`.
        #[arg(long = "3d")]
        relief: bool,
        /// Rapproche la caméra du parcours n fois (1.5 : plus près, 0.8 : plus loin), en plus du resserrement
        /// de `--3d` ; écrit `video-zoom-<n>.mp4`.
        #[arg(long)]
        zoom: Option<f64>,
        /// Avec `--3d`, relief éclairé par le soleil, de la position a à la position b de sa course pendant la
        /// vidéo : 0 à son lever à l'est, 90 à midi, 180 à son coucher à l'ouest, 270 à minuit sous la pleine
        /// lune, 360 au lever suivant (`--sun 20,140` : du matin à la fin d'après-midi ; `--sun 150,178` :
        /// coucher de soleil ; `--sun 170,230` : la nuit qui tombe ; `--sun 300,359` : l'aube) ; écrit
        /// `video-3d-soleil-<a>-<b>.mp4`.
        #[arg(long, value_name = "A,B", value_parser = angles)]
        sun: Option<(f64, f64)>,
        /// Avec `--3d`, heure locale du départ, à la place de `schedule.start` du run, avec `--finish-time`
        /// (`2026-07-24T05:30`) : le relief s'éclaire du vrai soleil et de la vraie lune, l'heure suivant
        /// l'avancée sur le tracé, dans le fuseau du lieu ; écrit `video-3d-horaire-<départ>-<arrivée>.mp4`.
        /// Sans elle, un run s'éclaire en relief des heures de passage de son activité (`import`), sinon de
        /// ses horaires.
        #[arg(long, value_name = "AAAA-MM-JJTHH:MM", value_parser = local_time, requires = "finish_time", conflicts_with = "sun")]
        start_time: Option<LocalDateTime>,
        /// Heure locale de l'arrivée, avec `--start-time`.
        #[arg(long, value_name = "AAAA-MM-JJTHH:MM", value_parser = local_time, requires = "start_time")]
        finish_time: Option<LocalDateTime>,
        /// Largeur de la vidéo en pixels, à la place du format du run, avec `--height` ; écrit
        /// `video-<l>x<h>.mp4`. Tout se met à l'échelle : 2160 × 3840, c'est la vidéo 1080 × 1920 en 4K.
        #[arg(long, requires = "height")]
        width: Option<u32>,
        /// Hauteur de la vidéo en pixels, avec `--width`.
        #[arg(long, requires = "width")]
        height: Option<u32>,
        /// Images par seconde, à la place de `fps` du run (60 : mouvement plus fluide, surtout en 4K) ;
        /// écrit `video-<n>fps.mp4`.
        #[arg(long)]
        fps: Option<u32>,
    },
    /// Légende prête à coller (`publication.txt`) et couverture (`cover.jpg`) du run.
    Publication { run: PathBuf },
    /// Produit les lieux du run (`places.geojson`).
    #[command(subcommand)]
    Source(source::Source),
    /// Envoie la vidéo, la couverture et la légende sur Nextcloud (`.env`).
    Upload {
        run: PathBuf,
        /// Envoie la variante `video-<n>s.mp4` (`render --duration n`) à la place de `video.mp4`.
        #[arg(long)]
        duration: Option<f64>,
        /// Envoie la variante rendue avec `render --stop-slowdown n`.
        #[arg(long)]
        stop_slowdown: Option<f64>,
        /// Envoie la variante sans ralenti (`render --no-slowdown`).
        #[arg(long, conflicts_with = "stop_slowdown")]
        no_slowdown: bool,
        /// Envoie la variante en relief (`render --3d`).
        #[arg(long = "3d")]
        relief: bool,
        /// Envoie la variante rendue avec `render --zoom n`.
        #[arg(long)]
        zoom: Option<f64>,
        /// Envoie la variante éclairée par le soleil (`render --3d --sun a,b`).
        #[arg(long, value_name = "A,B", value_parser = angles)]
        sun: Option<(f64, f64)>,
        /// Envoie la variante rendue avec `render --3d --start-time a --finish-time b`.
        #[arg(long, value_name = "AAAA-MM-JJTHH:MM", value_parser = local_time, requires = "finish_time", conflicts_with = "sun")]
        start_time: Option<LocalDateTime>,
        #[arg(long, value_name = "AAAA-MM-JJTHH:MM", value_parser = local_time, requires = "start_time")]
        finish_time: Option<LocalDateTime>,
        /// Envoie la variante sans carte d'infos (`render --no-hud`).
        #[arg(long)]
        no_hud: bool,
        /// Envoie la variante rendue avec `render --width l --height h`.
        #[arg(long, requires = "height")]
        width: Option<u32>,
        #[arg(long, requires = "width")]
        height: Option<u32>,
        /// Envoie la variante rendue avec `render --fps n`.
        #[arg(long)]
        fps: Option<u32>,
    },
    /// Éditeur du thème du run, sur sa tournée déjà calculée, dans le navigateur.
    Editor { run: PathBuf },
}

/// `a,b` : deux nombres séparés par une virgule.
fn local_time(value: &str) -> Result<LocalDateTime, String> {
    LocalDateTime::parse(value)
        .ok_or_else(|| format!("date et heure locales attendues, comme 2026-07-24T05:30 (reçu {value})"))
}

fn angles(value: &str) -> Result<(f64, f64), String> {
    let parse = |text: &str| text.trim().parse::<f64>().map_err(|_| format!("{text} n'est pas un nombre"));
    match value.split_once(',') {
        Some((a, b)) => Ok((parse(a)?, parse(b)?)),
        None => Err(format!("deux angles séparés par une virgule attendus, comme 20,140 (reçu {value})")),
    }
}

fn validate(arg: &std::path::Path) -> Outcome {
    let (run, paths) = load::run(arg)?;
    let places = load::places(&run, &paths)?;
    println!("{} : {} lieux conformes.", paths.places.display(), places.len());
    Ok(())
}

fn main() -> ExitCode {
    let outcome = match Cli::parse().command {
        Command::Validate { run } => validate(&run),
        Command::Solve { run } => solve::main(&run),
        Command::Import(import) => import::main(import),
        Command::Render {
            run,
            seconds,
            duration,
            stop_slowdown,
            no_slowdown,
            threads,
            frame,
            no_hud,
            relief,
            zoom,
            sun,
            start_time,
            finish_time,
            width,
            height,
            fps,
        } => render::main(
            &run,
            seconds,
            render::Variant {
                duration,
                stop_slowdown,
                no_slowdown,
                relief,
                zoom,
                sun,
                schedule: start_time.zip(finish_time).map(|(start, finish)| Schedule { start, finish }),
                no_hud,
                size: width.zip(height),
                fps,
            },
            threads,
            frame,
        ),
        Command::Publication { run } => publication::main(&run),
        Command::Source(source) => source::main(source),
        Command::Upload {
            run,
            duration,
            stop_slowdown,
            no_slowdown,
            relief,
            zoom,
            sun,
            start_time,
            finish_time,
            no_hud,
            width,
            height,
            fps,
        } => upload::main(
            &run,
            render::Variant {
                duration,
                stop_slowdown,
                no_slowdown,
                relief,
                zoom,
                sun,
                schedule: start_time.zip(finish_time).map(|(start, finish)| Schedule { start, finish }),
                no_hud,
                size: width.zip(height),
                fps,
            },
        ),
        Command::Editor { run } => editor::main(&run),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(load::Failure(messages)) => {
            for message in messages {
                eprintln!("{message}");
            }
            ExitCode::FAILURE
        }
    }
}
