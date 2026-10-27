//! Mettre un job en file, l'annuler.

use chrono::Utc;
use dijkstravel_core::run::Profile;

use super::Studio;
use crate::domain::job::{Job, JobId, JobKind, Transition};
use crate::domain::production::Refusal;

#[derive(Debug)]
pub enum EnqueueError {
    UnknownRun,
    Invalid(String),
    Refused(Refusal),
    /// L'OSRM de ce profil (ou VROOM) ne répond pas.
    RoutingDown(Profile),
    /// Un job de ce run est déjà en file ou en cours.
    Busy(JobId),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for EnqueueError {
    fn from(error: anyhow::Error) -> Self {
        EnqueueError::Internal(error)
    }
}

#[derive(Debug)]
pub enum CancelError {
    UnknownJob,
    Transition(Transition),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for CancelError {
    fn from(error: anyhow::Error) -> Self {
        CancelError::Internal(error)
    }
}

impl Studio {
    pub async fn enqueue(&self, run_name: &str, kind: JobKind) -> Result<Job, EnqueueError> {
        kind.validate().map_err(EnqueueError::Invalid)?;
        let production = self.catalog.production(run_name).await?.ok_or(EnqueueError::UnknownRun)?;
        production.can_enqueue(&kind).map_err(EnqueueError::Refused)?;
        if let Some(active) = self.jobs.for_run(production.id, 20).await?.iter().find(|job| !job.status.is_finished()) {
            return Err(EnqueueError::Busy(active.id));
        }
        if let Some(profile) = production.routing_needed(&kind)
            && !self.routing.available(profile).await
        {
            return Err(EnqueueError::RoutingDown(profile));
        }
        let job = Job::new(production.id, production.name(), kind, Utc::now());
        self.jobs.enqueue(&job).await?;
        self.events.job_changed(&job).await?;
        Ok(job)
    }

    pub async fn cancel(&self, id: JobId) -> Result<Job, CancelError> {
        let mut job = self.jobs.get(id).await?.ok_or(CancelError::UnknownJob)?;
        let now = Utc::now();
        job.request_cancel(now).map_err(CancelError::Transition)?;
        self.jobs.save_cancellation(id, now).await?;
        let job = self.jobs.get(id).await?.ok_or(CancelError::UnknownJob)?;
        self.events.job_changed(&job).await?;
        Ok(job)
    }
}
