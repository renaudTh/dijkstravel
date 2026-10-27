//! Le worker : traite les jobs dès qu'ils sont mis en file (`LISTEN jobs_queued`), avec une relève
//! périodique au cas où une notification se perdrait.

use std::time::Duration;

use sqlx::postgres::{PgListener, PgPool};

use crate::application::Studio;
use crate::infrastructure::postgres::JOBS_QUEUED;

const FALLBACK_POLL: Duration = Duration::from_secs(30);

pub async fn run(studio: Studio, pool: PgPool, name: String, concurrency: usize) -> anyhow::Result<()> {
    let reaped = studio.reap_stale_jobs().await?;
    if reaped > 0 {
        tracing::warn!("{reaped} jobs orphelins passés à failed");
    }
    let lanes: Vec<_> = (0..concurrency)
        .map(|lane| {
            let (studio, pool, name) = (studio.clone(), pool.clone(), format!("{name}/{lane}"));
            tokio::spawn(async move { lane_loop(studio, pool, name).await })
        })
        .collect();
    for lane in futures::future::join_all(lanes).await {
        lane??;
    }
    Ok(())
}

async fn lane_loop(studio: Studio, pool: PgPool, name: String) -> anyhow::Result<()> {
    let mut listener = PgListener::connect_with(&pool).await?;
    listener.listen(JOBS_QUEUED).await?;
    tracing::info!(worker = name, "en attente de jobs");
    loop {
        match studio.process_next(&name).await {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => {
                tracing::error!(worker = name, "{error:#}");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
        // Une notification arrivée pendant un job est déjà dans la file du listener.
        let _ = tokio::time::timeout(FALLBACK_POLL, listener.recv()).await;
    }
}
