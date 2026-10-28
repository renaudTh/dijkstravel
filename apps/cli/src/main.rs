//! `dijkstravel <commande> runs/<nom>` : toute la chaîne, des lieux à la vidéo publiée.

mod console;
mod editor;
mod load;
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
        /// Threads de rendu (tous les cœurs par défaut).
        #[arg(long)]
        threads: Option<usize>,
        /// L'image n seule, dans `data/out/<nom>/frame-<n>.png`.
        #[arg(long)]
        frame: Option<u32>,
    },
    /// Légende prête à coller (`publication.txt`) et couverture (`cover.jpg`) du run.
    Publication { run: PathBuf },
    /// Produit les lieux du run (`places.geojson`).
    #[command(subcommand)]
    Source(source::Source),
    /// Envoie la vidéo, la couverture et la légende sur Nextcloud (`.env`).
    Upload { run: PathBuf },
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
        Command::Render { run, seconds, duration, threads, frame } => {
            render::main(&run, seconds, duration, threads, frame)
        }
        Command::Publication { run } => publication::main(&run),
        Command::Source(source) => source::main(source),
        Command::Upload { run } => upload::main(&run),
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
