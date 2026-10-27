//! Traiter un job (worker) : le prendre en file, enchaîner ses étapes, suivre l'avancement et
//! l'annulation, puis enregistrer son issue.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use tokio::sync::{Mutex, mpsc};

use super::Studio;
use super::steps::{Signal, StepContext};
use crate::domain::job::Job;

/// Signe de vie du worker et lecture d'une demande d'annulation.
const HEARTBEAT: Duration = Duration::from_secs(2);
/// L'avancement est enregistré et diffusé au plus à ce rythme.
const PROGRESS_EVERY: Duration = Duration::from_millis(400);
/// Un job `running` sans signe de vie depuis ce délai a perdu son worker.
pub const STALE_AFTER: Duration = Duration::from_secs(120);

impl Studio {
    /// Les jobs orphelins d'un worker arrêté en route, passés à `failed`.
    pub async fn reap_stale_jobs(&self) -> anyhow::Result<u64> {
        let now = Utc::now();
        self.jobs.reap_stale(now - STALE_AFTER, now).await
    }

    /// Traite le plus ancien job en file ; `false` s'il n'y en avait pas.
    pub async fn process_next(&self, worker: &str) -> anyhow::Result<bool> {
        let Some(job) = self.jobs.claim_next(worker, Utc::now()).await? else { return Ok(false) };
        tracing::info!(job = %job.id, run = job.run_name, kind = ?job.kind, "job pris");
        self.events.job_changed(&job).await?;
        let scratch = self.settings.scratch_dir.join(job.id.to_string());
        tokio::fs::create_dir_all(&scratch).await.with_context(|| format!("{}", scratch.display()))?;

        let (signals, mut received) = mpsc::unbounded_channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let context = StepContext { job: job.id, signals, cancel: cancel.clone(), scratch: scratch.clone() };
        let job = Arc::new(Mutex::new(job));
        let follow = tokio::spawn({
            let (studio, job, cancel) = (self.clone(), job.clone(), cancel.clone());
            async move { studio.follow(job, &mut received, cancel).await }
        });

        let outcome = self.run_steps(job.clone(), &context).await;
        drop(context);
        follow.await.context("suivi du job")??;
        let _ = tokio::fs::remove_dir_all(&scratch).await;

        let mut job = job.lock().await;
        let now = Utc::now();
        let transition = match outcome {
            Ok(()) => job.succeed(now),
            Err(_) if cancel.load(Ordering::Relaxed) => job.cancelled(now),
            Err(error) => {
                tracing::warn!(job = %job.id, "échec : {error:#}");
                job.fail(format!("{error:#}"), now)
            }
        };
        // Une annulation déjà enregistrée par l'API (job pris entre-temps) reste l'issue retenue.
        if let Err(transition) = transition {
            tracing::warn!(job = %job.id, "{transition}");
        }
        self.jobs.save(&job).await?;
        self.events.job_changed(&job).await?;
        tracing::info!(job = %job.id, status = job.status.as_str(), "job fini");
        Ok(true)
    }

    async fn run_steps(&self, job: Arc<Mutex<Job>>, context: &StepContext) -> anyhow::Result<()> {
        let (run_id, kind) = {
            let job = job.lock().await;
            (job.run_id, job.kind.clone())
        };
        for step in kind.steps() {
            if context.cancel.load(Ordering::Relaxed) {
                anyhow::bail!(dijkstravel_render::monitor::CANCELLED);
            }
            // Relu à chaque étape : la précédente vient d'y ajouter ses fichiers.
            let production = self.catalog.production_by_id(run_id).await?.context("run disparu")?;
            self.run_step(step, &kind, &production, context).await?;
        }
        Ok(())
    }

    /// Recueille les signaux des étapes jusqu'à la fin du job ; tient le heartbeat et relève
    /// l'annulation demandée par l'API.
    async fn follow(
        &self,
        job: Arc<Mutex<Job>>,
        received: &mut mpsc::UnboundedReceiver<Signal>,
        cancel: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        let mut heartbeat = tokio::time::interval(HEARTBEAT);
        let mut last_save = tokio::time::Instant::now();
        let mut dirty = false;
        loop {
            tokio::select! {
                signal = received.recv() => {
                    let Some(signal) = signal else { break };
                    let mut job = job.lock().await;
                    let finished_step = match signal {
                        Signal::Progress(progress) => {
                            let finished = progress.done == progress.total;
                            job.report(progress);
                            finished
                        }
                        Signal::Note(line) => {
                            job.note(line);
                            true
                        }
                    };
                    dirty = true;
                    if finished_step || last_save.elapsed() >= PROGRESS_EVERY {
                        self.jobs.save(&job).await?;
                        self.events.job_changed(&job).await?;
                        (last_save, dirty) = (tokio::time::Instant::now(), false);
                    }
                }
                _ = heartbeat.tick() => {
                    let id = job.lock().await.id;
                    if self.jobs.heartbeat(id, Utc::now()).await? {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    if dirty {
                        let job = job.lock().await;
                        self.jobs.save(&job).await?;
                        self.events.job_changed(&job).await?;
                        (last_save, dirty) = (tokio::time::Instant::now(), false);
                    }
                }
            }
        }
        Ok(())
    }
}
