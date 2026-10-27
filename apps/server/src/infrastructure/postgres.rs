//! Adaptateurs Postgres : catalogue, file des jobs (`FOR UPDATE SKIP LOCKED`) et événements
//! (`NOTIFY`). Requêtes vérifiées à l'exécution : la compilation ne demande pas de base.

use std::collections::BTreeMap;

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dijkstravel_core::run::Run;
use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::types::Json;
use sqlx::{Row, query};
use uuid::Uuid;

use crate::domain::artifact::{Artifact, ArtifactKind};
use crate::domain::job::{Job, JobId, JobKind, JobStatus, Progress};
use crate::domain::ports::{Catalog, JobEvents, JobQueue, Result};
use crate::domain::production::{RunId, RunProduction};

/// Canal réveillant les workers à chaque job mis en file.
pub const JOBS_QUEUED: &str = "jobs_queued";
/// Canal portant l'identifiant de chaque job changé, pour le direct de l'interface.
pub const JOB_EVENTS: &str = "job_events";

pub async fn connect(url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new().max_connections(10).connect(url).await.context("connexion à Postgres")?;
    sqlx::migrate!("./migrations").run(&pool).await.context("migrations")?;
    Ok(pool)
}

#[derive(Clone)]
pub struct Postgres {
    pub pool: PgPool,
}

/// `select … from jobs j join runs r` suivi de `$rest` : des requêtes littérales, sans SQL assemblé à l'exécution.
macro_rules! jobs_select {
    ($distinct:literal, $rest:literal) => {
        concat!(
            "select ",
            $distinct,
            " j.id, j.run_id, r.name as run_name, j.kind, j.status, j.progress, j.log, j.error, ",
            "j.cancel_requested, j.created_at, j.started_at, j.finished_at ",
            "from jobs j join runs r on r.id = j.run_id ",
            $rest
        )
    };
}

fn job_of(row: &PgRow) -> Result<Job> {
    let status: String = row.try_get("status")?;
    Ok(Job {
        id: JobId(row.try_get("id")?),
        run_id: RunId(row.try_get("run_id")?),
        run_name: row.try_get("run_name")?,
        kind: row.try_get::<Json<JobKind>, _>("kind")?.0,
        status: JobStatus::parse(&status).ok_or_else(|| anyhow!("statut inconnu : {status}"))?,
        progress: row.try_get::<Option<Json<Progress>>, _>("progress")?.map(|json| json.0),
        log: row.try_get::<Json<Vec<String>>, _>("log")?.0,
        error: row.try_get("error")?,
        cancel_requested: row.try_get("cancel_requested")?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

impl Postgres {
    async fn productions_where(&self, filter: Filter) -> Result<Vec<RunProduction>> {
        let runs = match filter {
            Filter::All => query("select id, spec from runs order by name"),
            Filter::Name(name) => query("select id, spec from runs where name = $1").bind(name),
            Filter::Id(id) => query("select id, spec from runs where id = $1").bind(id.0),
        };
        let rows = runs.fetch_all(&self.pool).await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.try_get("id")).collect::<std::result::Result<_, _>>()?;
        let artifacts = query("select run_id, kind, key, bytes, meta from artifacts where run_id = any($1)")
            .bind(&ids)
            .fetch_all(&self.pool)
            .await?;
        let mut by_run: BTreeMap<Uuid, BTreeMap<ArtifactKind, Artifact>> = BTreeMap::new();
        for row in &artifacts {
            let kind: String = row.try_get("kind")?;
            let kind = ArtifactKind::parse(&kind).ok_or_else(|| anyhow!("artefact inconnu : {kind}"))?;
            let artifact = Artifact {
                kind,
                key: row.try_get("key")?,
                bytes: row.try_get::<i64, _>("bytes")? as u64,
                meta: row.try_get::<Json<Value>, _>("meta")?.0,
            };
            by_run.entry(row.try_get("run_id")?).or_default().insert(kind, artifact);
        }
        rows.iter()
            .map(|row| {
                let id: Uuid = row.try_get("id")?;
                let spec = row.try_get::<Json<Value>, _>("spec")?.0;
                let run = Run::parse(&spec).map_err(|issues| anyhow!("run {id} invalide : {issues:?}"))?;
                Ok(RunProduction { id: RunId(id), run, spec, artifacts: by_run.remove(&id).unwrap_or_default() })
            })
            .collect()
    }

    async fn notify(&self, channel: &str, payload: &str) -> Result<()> {
        query("select pg_notify($1, $2)").bind(channel).bind(payload).execute(&self.pool).await?;
        Ok(())
    }
}

enum Filter {
    All,
    Name(String),
    Id(RunId),
}

#[async_trait]
impl Catalog for Postgres {
    async fn productions(&self) -> Result<Vec<RunProduction>> {
        self.productions_where(Filter::All).await
    }

    async fn production(&self, name: &str) -> Result<Option<RunProduction>> {
        Ok(self.productions_where(Filter::Name(name.to_owned())).await?.pop())
    }

    async fn production_by_id(&self, id: RunId) -> Result<Option<RunProduction>> {
        Ok(self.productions_where(Filter::Id(id)).await?.pop())
    }

    async fn save_run(&self, spec: &Value) -> Result<RunId> {
        let name = spec["name"].as_str().context("run sans nom")?;
        let row = query(
            "insert into runs (id, name, spec) values ($1, $2, $3) \
             on conflict (name) do update set spec = excluded.spec, updated_at = now() returning id",
        )
        .bind(Uuid::new_v4())
        .bind(name)
        .bind(Json(spec))
        .fetch_one(&self.pool)
        .await?;
        Ok(RunId(row.try_get("id")?))
    }

    async fn theme(&self, name: &str) -> Result<Option<Value>> {
        let row = query("select spec from themes where name = $1").bind(name).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.try_get::<Json<Value>, _>("spec")).transpose()?.map(|json| json.0))
    }

    async fn save_theme(&self, name: &str, spec: &Value) -> Result<()> {
        query(
            "insert into themes (name, spec) values ($1, $2) \
             on conflict (name) do update set spec = excluded.spec, updated_at = now()",
        )
        .bind(name)
        .bind(Json(spec))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_artifact(&self, run: RunId, artifact: &Artifact, job: Option<JobId>) -> Result<()> {
        query(
            "insert into artifacts (run_id, kind, key, bytes, meta, job_id) values ($1, $2, $3, $4, $5, $6) \
             on conflict (run_id, kind) do update set key = excluded.key, bytes = excluded.bytes, \
             meta = excluded.meta, job_id = excluded.job_id, created_at = now()",
        )
        .bind(run.0)
        .bind(artifact.kind.as_str())
        .bind(&artifact.key)
        .bind(artifact.bytes as i64)
        .bind(Json(&artifact.meta))
        .bind(job.map(|job| job.0))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl JobQueue for Postgres {
    async fn enqueue(&self, job: &Job) -> Result<()> {
        query("insert into jobs (id, run_id, kind, status, log, created_at) values ($1, $2, $3, $4, $5, $6)")
            .bind(job.id.0)
            .bind(job.run_id.0)
            .bind(Json(&job.kind))
            .bind(job.status.as_str())
            .bind(Json(&job.log))
            .bind(job.created_at)
            .execute(&self.pool)
            .await?;
        self.notify(JOBS_QUEUED, &job.id.to_string()).await
    }

    /// Dans une transaction : le job en file est verrouillé (les autres workers passent au suivant),
    /// démarré par le domaine, puis enregistré.
    async fn claim_next(&self, worker: &str, now: DateTime<Utc>) -> Result<Option<Job>> {
        let mut transaction = self.pool.begin().await?;
        let row = query(jobs_select!(
            "",
            "where j.id = (select id from jobs where status = 'queued' order by created_at \
             for update skip locked limit 1) for update of j"
        ))
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else { return Ok(None) };
        let mut job = job_of(&row)?;
        job.start(now)?;
        query("update jobs set status = $2, started_at = $3, heartbeat_at = $3, worker = $4 where id = $1")
            .bind(job.id.0)
            .bind(job.status.as_str())
            .bind(job.started_at)
            .bind(worker)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(Some(job))
    }

    async fn save(&self, job: &Job) -> Result<()> {
        query(
            "update jobs set status = $2, progress = $3, log = $4, error = $5, \
             cancel_requested = cancel_requested or $6, started_at = $7, finished_at = $8 \
             where id = $1 and status in ('queued', 'running')",
        )
        .bind(job.id.0)
        .bind(job.status.as_str())
        .bind(job.progress.as_ref().map(Json))
        .bind(Json(&job.log))
        .bind(&job.error)
        .bind(job.cancel_requested)
        .bind(job.started_at)
        .bind(job.finished_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn save_cancellation(&self, id: JobId, now: DateTime<Utc>) -> Result<()> {
        query(
            "update jobs set cancel_requested = true, \
             finished_at = case when status = 'queued' then $2 else finished_at end, \
             status = case when status = 'queued' then 'cancelled' else status end \
             where id = $1 and status in ('queued', 'running')",
        )
        .bind(id.0)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn heartbeat(&self, id: JobId, now: DateTime<Utc>) -> Result<bool> {
        let row = query(
            "update jobs set heartbeat_at = $2 where id = $1 \
             returning cancel_requested or status = 'cancelled' as stop",
        )
        .bind(id.0)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.try_get("stop")).transpose()?.unwrap_or(true))
    }

    async fn reap_stale(&self, before: DateTime<Utc>, now: DateTime<Utc>) -> Result<u64> {
        let done = query(
            "update jobs set status = 'failed', error = 'worker arrêté en cours de job', finished_at = $2 \
             where status = 'running' and (heartbeat_at is null or heartbeat_at < $1)",
        )
        .bind(before)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    async fn get(&self, id: JobId) -> Result<Option<Job>> {
        query(jobs_select!("", "where j.id = $1"))
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(job_of)
            .transpose()
    }

    async fn for_run(&self, run: RunId, limit: i64) -> Result<Vec<Job>> {
        query(jobs_select!("", "where j.run_id = $1 order by j.created_at desc limit $2"))
            .bind(run.0)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(job_of)
            .collect()
    }

    async fn latest(&self) -> Result<Vec<Job>> {
        query(jobs_select!("distinct on (j.run_id)", "order by j.run_id, j.created_at desc"))
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(job_of)
            .collect()
    }
}

#[async_trait]
impl JobEvents for Postgres {
    async fn job_changed(&self, job: &Job) -> Result<()> {
        self.notify(JOB_EVENTS, &job.id.to_string()).await
    }
}
