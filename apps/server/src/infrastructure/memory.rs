//! Adaptateurs en mémoire, pour tester l'application sans Postgres, S3 ni OSRM.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use dijkstravel_core::geo::{LonLat, distance_meters};
use dijkstravel_core::run::{Profile, Run};
use dijkstravel_solve::{DistanceTable, RoutedPath, Routing, SolveError};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::domain::artifact::Artifact;
use crate::domain::job::{Job, JobId, JobStatus};
use crate::domain::ports::{ArtifactStore, Catalog, JobEvents, JobQueue, Result, RoutingDirectory};
use crate::domain::production::{RunId, RunProduction};

#[derive(Default)]
pub struct Memory {
    runs: Mutex<BTreeMap<String, RunProduction>>,
    themes: Mutex<HashMap<String, Value>>,
    jobs: Mutex<Vec<Job>>,
    files: Mutex<HashMap<String, Bytes>>,
    /// Statuts annoncés, dans l'ordre.
    pub events: Mutex<Vec<(JobId, JobStatus)>>,
}

#[async_trait]
impl Catalog for Memory {
    async fn productions(&self) -> Result<Vec<RunProduction>> {
        Ok(self.runs.lock().unwrap().values().cloned().collect())
    }

    async fn production(&self, name: &str) -> Result<Option<RunProduction>> {
        Ok(self.runs.lock().unwrap().get(name).cloned())
    }

    async fn production_by_id(&self, id: RunId) -> Result<Option<RunProduction>> {
        Ok(self.runs.lock().unwrap().values().find(|production| production.id == id).cloned())
    }

    async fn save_run(&self, spec: &Value) -> Result<RunId> {
        let run = Run::parse(spec).map_err(|issues| anyhow!("{issues:?}"))?;
        let mut runs = self.runs.lock().unwrap();
        let id = runs.get(&run.name).map_or_else(|| RunId(Uuid::new_v4()), |known| known.id);
        let artifacts = runs.get(&run.name).map(|known| known.artifacts.clone()).unwrap_or_default();
        runs.insert(run.name.clone(), RunProduction { id, run, spec: spec.clone(), artifacts });
        Ok(id)
    }

    async fn theme(&self, name: &str) -> Result<Option<Value>> {
        Ok(self.themes.lock().unwrap().get(name).cloned())
    }

    async fn save_theme(&self, name: &str, spec: &Value) -> Result<()> {
        self.themes.lock().unwrap().insert(name.to_owned(), spec.clone());
        Ok(())
    }

    async fn record_artifact(&self, run: RunId, artifact: &Artifact, _job: Option<JobId>) -> Result<()> {
        let mut runs = self.runs.lock().unwrap();
        let production = runs.values_mut().find(|production| production.id == run).context("run inconnu")?;
        production.artifacts.insert(artifact.kind, artifact.clone());
        Ok(())
    }
}

#[async_trait]
impl JobQueue for Memory {
    async fn enqueue(&self, job: &Job) -> Result<()> {
        self.jobs.lock().unwrap().push(job.clone());
        Ok(())
    }

    async fn claim_next(&self, _worker: &str, now: DateTime<Utc>) -> Result<Option<Job>> {
        let mut jobs = self.jobs.lock().unwrap();
        let Some(job) = jobs.iter_mut().find(|job| job.status == JobStatus::Queued) else { return Ok(None) };
        job.start(now)?;
        Ok(Some(job.clone()))
    }

    async fn save(&self, job: &Job) -> Result<()> {
        let mut jobs = self.jobs.lock().unwrap();
        let known = jobs.iter_mut().find(|known| known.id == job.id).context("job inconnu")?;
        if !known.status.is_finished() {
            let cancel_requested = known.cancel_requested || job.cancel_requested;
            *known = job.clone();
            known.cancel_requested = cancel_requested;
        }
        Ok(())
    }

    async fn save_cancellation(&self, id: JobId, now: DateTime<Utc>) -> Result<()> {
        let mut jobs = self.jobs.lock().unwrap();
        let job = jobs.iter_mut().find(|job| job.id == id).context("job inconnu")?;
        job.request_cancel(now)?;
        Ok(())
    }

    async fn heartbeat(&self, id: JobId, _now: DateTime<Utc>) -> Result<bool> {
        let jobs = self.jobs.lock().unwrap();
        Ok(jobs
            .iter()
            .find(|job| job.id == id)
            .is_none_or(|job| job.cancel_requested || job.status == JobStatus::Cancelled))
    }

    async fn reap_stale(&self, _before: DateTime<Utc>, _now: DateTime<Utc>) -> Result<u64> {
        Ok(0)
    }

    async fn get(&self, id: JobId) -> Result<Option<Job>> {
        Ok(self.jobs.lock().unwrap().iter().find(|job| job.id == id).cloned())
    }

    async fn for_run(&self, run: RunId, limit: i64) -> Result<Vec<Job>> {
        let jobs = self.jobs.lock().unwrap();
        Ok(jobs.iter().rev().filter(|job| job.run_id == run).take(limit as usize).cloned().collect())
    }

    async fn latest(&self) -> Result<Vec<Job>> {
        let mut latest: HashMap<RunId, Job> = HashMap::new();
        for job in self.jobs.lock().unwrap().iter() {
            latest.insert(job.run_id, job.clone());
        }
        Ok(latest.into_values().collect())
    }
}

#[async_trait]
impl ArtifactStore for Memory {
    async fn put(&self, key: &str, body: Bytes) -> Result<u64> {
        let size = body.len() as u64;
        self.files.lock().unwrap().insert(key.to_owned(), body);
        Ok(size)
    }

    async fn put_file(&self, key: &str, file: &Path) -> Result<u64> {
        self.put(key, Bytes::from(std::fs::read(file)?)).await
    }

    async fn get(&self, key: &str) -> Result<Bytes> {
        self.files.lock().unwrap().get(key).cloned().with_context(|| format!("{key} absent"))
    }

    async fn size(&self, key: &str) -> Result<u64> {
        Ok(ArtifactStore::get(self, key).await?.len() as u64)
    }

    async fn get_range(&self, key: &str, range: Range<u64>) -> Result<Bytes> {
        Ok(ArtifactStore::get(self, key).await?.slice(range.start as usize..range.end as usize))
    }
}

#[async_trait]
impl JobEvents for Memory {
    async fn job_changed(&self, job: &Job) -> Result<()> {
        self.events.lock().unwrap().push((job.id, job.status));
        Ok(())
    }
}

/// Distances à vol d'oiseau, ordre de visite tel que donné : la voiture seule est « démarrée ».
pub struct StraightLines;

impl Routing for StraightLines {
    fn table(&self, locations: &[LonLat]) -> std::result::Result<DistanceTable, SolveError> {
        let matrix = locations
            .iter()
            .map(|from| locations.iter().map(|to| Some(distance_meters(*from, *to))).collect())
            .collect();
        Ok(DistanceTable { matrix, snap_meters: vec![3.0; locations.len()] })
    }

    fn route(&self, waypoints: &[LonLat]) -> std::result::Result<RoutedPath, SolveError> {
        let meters = waypoints.windows(2).map(|pair| distance_meters(pair[0], pair[1])).sum();
        Ok(RoutedPath {
            points: waypoints.to_vec(),
            leg_segments: vec![1; waypoints.len() - 1],
            ferry_segments: Vec::new(),
            meters,
        })
    }

    fn optimize(&self, request: &Value) -> std::result::Result<Value, SolveError> {
        let steps: Vec<Value> = request["jobs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|job| json!({ "type": "job", "id": job["id"] }))
            .collect();
        Ok(json!({ "code": 0, "routes": [{ "steps": steps }], "unassigned": [] }))
    }
}

#[async_trait]
impl RoutingDirectory for StraightLines {
    fn routing(&self, profile: Profile) -> Option<Arc<dyn Routing + Send + Sync>> {
        (profile == Profile::Car).then(|| Arc::new(StraightLines) as Arc<dyn Routing + Send + Sync>)
    }

    async fn available(&self, profile: Profile) -> bool {
        profile == Profile::Car
    }
}
