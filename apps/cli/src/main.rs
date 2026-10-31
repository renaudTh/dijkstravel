//! `dijkstravel <commande> runs/<nom>` : toute la chaîne, des lieux à la vidéo publiée.

mod console;
mod editor;
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
        /// Envoie la variante sans carte d'infos (`render --no-hud`).
        #[arg(long)]
        no_hud: bool,
    },
    /// Éditeur du thème du run, sur sa tournée déjà calculée, dans le navigateur.
    Editor { run: PathBuf },
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
        } => render::main(
            &run,
            seconds,
            render::Variant { duration, stop_slowdown, no_slowdown, relief, zoom, no_hud },
            threads,
            frame,
        ),
        Command::Publication { run } => publication::main(&run),
        Command::Source(source) => source::main(source),
        Command::Upload { run, duration, stop_slowdown, no_slowdown, relief, zoom, no_hud } => {
            upload::main(&run, render::Variant { duration, stop_slowdown, no_slowdown, relief, zoom, no_hud })
        }
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
