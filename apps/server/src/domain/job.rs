//! Agrégat `Job` : une demande de production sur un run, de la file d'attente à son issue. Ses
//! transitions gardent l'historique cohérent : un job fini ne repart pas, ne rapporte plus rien.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::production::RunId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Une étape du pipeline, exécutée par le worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Source,
    Solve,
    Render,
    Publication,
}

/// Ce que l'on demande. `All` : calculer, rendre, puis préparer la publication, à la suite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum JobKind {
    Source,
    Solve,
    /// `seconds` : n'en rendre que le début, pour juger vite d'un thème.
    Render {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seconds: Option<f64>,
    },
    Publication,
    All,
}

impl JobKind {
    pub fn steps(&self) -> Vec<Step> {
        match self {
            JobKind::Source => vec![Step::Source],
            JobKind::Solve => vec![Step::Solve],
            JobKind::Render { .. } => vec![Step::Render],
            JobKind::Publication => vec![Step::Publication],
            JobKind::All => vec![Step::Solve, Step::Render, Step::Publication],
        }
    }

    /// Durée demandée, rejetée si elle n'a pas de sens.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            JobKind::Render { seconds: Some(seconds) } if !(seconds.is_finite() && *seconds > 0.0) => {
                Err("seconds attend un nombre > 0".to_owned())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(text: &str) -> Option<JobStatus> {
        [JobStatus::Queued, JobStatus::Running, JobStatus::Succeeded, JobStatus::Failed, JobStatus::Cancelled]
            .into_iter()
            .find(|status| status.as_str() == text)
    }

    pub fn is_finished(self) -> bool {
        matches!(self, JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled)
    }
}

/// Avancement de l'étape en cours : `stage` vaut `tiles` ou `frames` pendant un rendu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub step: Step,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// Le job n'est pas dans l'état d'où part cette transition.
    NotAllowed { from: JobStatus, to: JobStatus },
}

impl std::fmt::Display for Transition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Transition::NotAllowed { from, to } = self;
        write!(f, "job {} : passage à {} impossible", from.as_str(), to.as_str())
    }
}

impl std::error::Error for Transition {}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: JobId,
    pub run_id: RunId,
    pub run_name: String,
    pub kind: JobKind,
    pub status: JobStatus,
    pub progress: Option<Progress>,
    /// Ce que les étapes ont signalé (lieux exclus, images introuvables…), une ligne par fait.
    pub log: Vec<String>,
    pub error: Option<String>,
    pub cancel_requested: bool,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl Job {
    pub fn new(run_id: RunId, run_name: impl Into<String>, kind: JobKind, now: DateTime<Utc>) -> Job {
        Job {
            id: JobId(Uuid::new_v4()),
            run_id,
            run_name: run_name.into(),
            kind,
            status: JobStatus::Queued,
            progress: None,
            log: Vec::new(),
            error: None,
            cancel_requested: false,
            created_at: now,
            started_at: None,
            finished_at: None,
        }
    }

    fn move_to(&mut self, allowed_from: &[JobStatus], to: JobStatus) -> Result<(), Transition> {
        if !allowed_from.contains(&self.status) {
            return Err(Transition::NotAllowed { from: self.status, to });
        }
        self.status = to;
        Ok(())
    }

    pub fn start(&mut self, now: DateTime<Utc>) -> Result<(), Transition> {
        self.move_to(&[JobStatus::Queued], JobStatus::Running)?;
        self.started_at = Some(now);
        Ok(())
    }

    /// Ignoré une fois le job fini : un dernier rapport peut arriver après l'annulation.
    pub fn report(&mut self, progress: Progress) {
        if self.status == JobStatus::Running {
            self.progress = Some(progress);
        }
    }

    pub fn note(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
    }

    pub fn succeed(&mut self, now: DateTime<Utc>) -> Result<(), Transition> {
        self.move_to(&[JobStatus::Running], JobStatus::Succeeded)?;
        self.finished_at = Some(now);
        Ok(())
    }

    pub fn fail(&mut self, error: impl Into<String>, now: DateTime<Utc>) -> Result<(), Transition> {
        self.move_to(&[JobStatus::Running], JobStatus::Failed)?;
        self.error = Some(error.into());
        self.finished_at = Some(now);
        Ok(())
    }

    /// En file, le job est annulé sur-le-champ ; en cours, l'annulation est demandée au worker, qui
    /// arrête l'étape puis confirme par `cancelled`.
    pub fn request_cancel(&mut self, now: DateTime<Utc>) -> Result<(), Transition> {
        match self.status {
            JobStatus::Queued => self.cancelled(now),
            JobStatus::Running => {
                self.cancel_requested = true;
                Ok(())
            }
            from => Err(Transition::NotAllowed { from, to: JobStatus::Cancelled }),
        }
    }

    pub fn cancelled(&mut self, now: DateTime<Utc>) -> Result<(), Transition> {
        self.move_to(&[JobStatus::Queued, JobStatus::Running], JobStatus::Cancelled)?;
        self.finished_at = Some(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(kind: JobKind) -> Job {
        Job::new(RunId(Uuid::new_v4()), "essai", kind, Utc::now())
    }

    #[test]
    fn runs_from_queue_to_success() {
        let mut job = job(JobKind::Solve);
        job.start(Utc::now()).unwrap();
        job.report(Progress { step: Step::Solve, stage: None, done: 1, total: 1 });
        job.succeed(Utc::now()).unwrap();
        assert_eq!(job.status, JobStatus::Succeeded);
        assert!(job.finished_at.is_some());
    }

    #[test]
    fn a_finished_job_does_not_move_again() {
        let mut job = job(JobKind::Solve);
        job.start(Utc::now()).unwrap();
        job.fail("OSRM injoignable", Utc::now()).unwrap();
        assert_eq!(
            job.succeed(Utc::now()),
            Err(Transition::NotAllowed { from: JobStatus::Failed, to: JobStatus::Succeeded })
        );
        assert!(job.start(Utc::now()).is_err());
        assert!(job.request_cancel(Utc::now()).is_err());
        job.report(Progress { step: Step::Solve, stage: None, done: 1, total: 1 });
        assert_eq!(job.progress, None);
    }

    #[test]
    fn cancels_a_queued_job_at_once_and_a_running_one_on_request() {
        let mut queued = job(JobKind::Render { seconds: None });
        queued.request_cancel(Utc::now()).unwrap();
        assert_eq!(queued.status, JobStatus::Cancelled);
        let mut running = job(JobKind::Render { seconds: None });
        running.start(Utc::now()).unwrap();
        running.request_cancel(Utc::now()).unwrap();
        assert_eq!(running.status, JobStatus::Running);
        assert!(running.cancel_requested);
        running.cancelled(Utc::now()).unwrap();
        assert_eq!(running.status, JobStatus::Cancelled);
    }

    #[test]
    fn all_chains_solve_render_and_publication() {
        assert_eq!(JobKind::All.steps(), [Step::Solve, Step::Render, Step::Publication]);
    }

    #[test]
    fn reads_the_kind_from_the_request_body() {
        let kind: JobKind = serde_json::from_str(r#"{ "kind": "render", "seconds": 10 }"#).unwrap();
        assert_eq!(kind, JobKind::Render { seconds: Some(10.0) });
        assert!(JobKind::Render { seconds: Some(-1.0) }.validate().is_err());
        assert_eq!(serde_json::from_str::<JobKind>(r#"{ "kind": "all" }"#).unwrap(), JobKind::All);
    }
}
