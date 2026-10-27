//! Les ports du contexte Production : ce dont le domaine et l'application ont besoin, sans dire
//! comment c'est servi (Postgres, S3, OSRM en HTTP ou faux en mémoire dans les tests).

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use dijkstravel_core::run::Profile;
use dijkstravel_solve::Routing;
use serde_json::Value;

use super::artifact::{Artifact, ArtifactKind};
use super::job::{Job, JobId};
use super::production::{RunId, RunProduction};

pub type Result<T> = anyhow::Result<T>;

/// Les runs, leurs thèmes et l'index de ce qu'ils ont produit.
#[async_trait]
pub trait Catalog: Send + Sync {
    async fn productions(&self) -> Result<Vec<RunProduction>>;
    async fn production(&self, name: &str) -> Result<Option<RunProduction>>;
    async fn production_by_id(&self, id: RunId) -> Result<Option<RunProduction>>;
    /// Crée ou remplace le run de ce nom ; son identifiant ne change pas.
    async fn save_run(&self, spec: &Value) -> Result<RunId>;
    /// Thème brut, à valider par `Theme::parse` ; `None` : le thème par défaut.
    async fn theme(&self, name: &str) -> Result<Option<Value>>;
    async fn save_theme(&self, name: &str, spec: &Value) -> Result<()>;
    async fn record_artifact(&self, run: RunId, artifact: &Artifact, job: Option<JobId>) -> Result<()>;
}

/// La file des jobs et leur historique.
#[async_trait]
pub trait JobQueue: Send + Sync {
    async fn enqueue(&self, job: &Job) -> Result<()>;
    /// Prend le plus ancien job en file, déjà passé à `running` ; deux workers n'ont jamais le même.
    async fn claim_next(&self, worker: &str, now: DateTime<Utc>) -> Result<Option<Job>>;
    /// Enregistre le job tel que le worker l'a fait avancer ; sans effet sur un job déjà fini.
    async fn save(&self, job: &Job) -> Result<()>;
    /// Enregistre seulement une annulation : un job encore en file passe à `cancelled`, un job en
    /// cours garde son état et reçoit la demande. Le log et l'avancement du worker sont préservés.
    async fn save_cancellation(&self, id: JobId, now: DateTime<Utc>) -> Result<()>;
    /// Signe de vie du worker ; `true` si le job doit s'arrêter (annulation demandée ou déjà faite).
    async fn heartbeat(&self, id: JobId, now: DateTime<Utc>) -> Result<bool>;
    /// Les jobs `running` sans signe de vie depuis `before`, passés à `failed`.
    async fn reap_stale(&self, before: DateTime<Utc>, now: DateTime<Utc>) -> Result<u64>;
    async fn get(&self, id: JobId) -> Result<Option<Job>>;
    async fn for_run(&self, run: RunId, limit: i64) -> Result<Vec<Job>>;
    async fn latest(&self) -> Result<Vec<Job>>;
}

/// Le contenu des fichiers produits.
#[async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn put(&self, key: &str, body: Bytes) -> Result<u64>;
    async fn put_file(&self, key: &str, file: &Path) -> Result<u64>;
    async fn get(&self, key: &str) -> Result<Bytes>;
    async fn size(&self, key: &str) -> Result<u64>;
    async fn get_range(&self, key: &str, range: Range<u64>) -> Result<Bytes>;
}

/// Ce qui se passe sur les jobs, pour l'interface en direct.
#[async_trait]
pub trait JobEvents: Send + Sync {
    async fn job_changed(&self, job: &Job) -> Result<()>;
}

/// Un routage par profil, s'il est configuré.
#[async_trait]
pub trait RoutingDirectory: Send + Sync {
    fn routing(&self, profile: Profile) -> Option<Arc<dyn Routing + Send + Sync>>;
    /// Le service répond-il ? Un OSRM arrêté (profil compose non démarré) ne l'est pas.
    async fn available(&self, profile: Profile) -> bool;
}

/// L'artefact `kind` du run, rangé à sa place.
pub fn artifact(kind: ArtifactKind, run: RunId, bytes: u64, meta: Value) -> Artifact {
    Artifact { kind, key: kind.key(run), bytes, meta }
}
