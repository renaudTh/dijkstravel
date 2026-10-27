//! `dijkstravel <commande> runs/<nom>` : toute la chaîne, des lieux à la vidéo publiée.

mod load;

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
