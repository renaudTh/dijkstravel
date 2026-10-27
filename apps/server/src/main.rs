//! `dijkstravel-server api | worker | import` : le studio auto-hébergé. Racine de composition : la
//! configuration choisit les adaptateurs, qui servent les ports des cas d'usage.

mod application;
mod config;
mod domain;
mod infrastructure;
mod interfaces;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use sqlx::postgres::PgPool;

use crate::application::Studio;
use crate::config::Config;
use crate::infrastructure::garage;
use crate::infrastructure::postgres::{self, Postgres};
use crate::infrastructure::routing::HttpRouting;
use crate::infrastructure::s3::S3;

#[derive(Parser)]
#[command(name = "dijkstravel-server", about = "Studio dijkstravel : API, worker et import")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Sert l'API et l'interface ; initialise Garage et importe `IMPORT_DIR` au démarrage.
    Api,
    /// Traite les jobs mis en file.
    Worker,
    /// Importe un dépôt dijkstravel (runs, thèmes, sorties).
    Import {
        repo: PathBuf,
        /// Les sorties du dépôt, `<repo>/data/out` par défaut.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

async fn studio(config: &Config) -> anyhow::Result<(Studio, PgPool)> {
    let pool = postgres::connect(&config.database_url).await?;
    let postgres = Arc::new(Postgres { pool: pool.clone() });
    let studio = Studio {
        catalog: postgres.clone(),
        jobs: postgres.clone(),
        store: Arc::new(S3::new(&config.s3)?),
        events: postgres,
        routing: Arc::new(HttpRouting::new(config.routing.clone())),
        settings: config.settings.clone(),
    };
    Ok((studio, pool))
}

async fn import(studio: &Studio, repo: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
    let report = studio.import(repo, out).await?;
    tracing::info!(
        runs = report.runs,
        skipped = report.skipped,
        themes = report.themes,
        "import de {}",
        repo.display()
    );
    for problem in &report.problems {
        tracing::warn!("import : {problem}");
    }
    Ok(())
}

async fn main_with(command: Command) -> anyhow::Result<()> {
    let config = Config::from_env()?;
    if let (Command::Api, Some(garage)) = (&command, config.garage.clone()) {
        tokio::task::spawn_blocking(move || garage::bootstrap(&garage)).await.context("Garage")??;
    }
    let (studio, pool) = studio(&config).await?;
    match command {
        Command::Api => {
            if let Some((repo, out)) = &config.import {
                import(&studio, repo, out).await?;
            }
            interfaces::http::serve(studio, pool, &config.listen, &config.web_dir).await
        }
        Command::Worker => {
            tokio::fs::create_dir_all(&config.settings.scratch_dir).await?;
            interfaces::worker::run(studio, pool, config.worker_name, config.worker_concurrency).await
        }
        Command::Import { repo, out } => {
            let out = out.unwrap_or_else(|| repo.join("data/out"));
            import(&studio, &repo, &out).await
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn,tower_http=info".into()),
        )
        .init();
    match main_with(Cli::parse().command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
