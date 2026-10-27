//! Cas d'usage du studio : mettre un job en file, l'annuler, le traiter (worker), lire l'état pour
//! l'interface, importer un dépôt existant. L'orchestration est ici ; le métier reste dans les libs.

pub mod enqueue;
pub mod import;
pub mod process;
pub mod queries;
pub mod steps;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use crate::domain::ports::{ArtifactStore, Catalog, JobEvents, JobQueue, RoutingDirectory};

/// Réglages des étapes, lus dans l'environnement par la racine de composition.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Cache de tuiles, polices et images, partagé par tous les rendus.
    pub cache_dir: PathBuf,
    /// Dossier temporaire de chaque job (vidéo avant envoi).
    pub scratch_dir: PathBuf,
    pub render_threads: usize,
    pub overpass_url: String,
}

#[derive(Clone)]
pub struct Studio {
    pub catalog: Arc<dyn Catalog>,
    pub jobs: Arc<dyn JobQueue>,
    pub store: Arc<dyn ArtifactStore>,
    pub events: Arc<dyn JobEvents>,
    pub routing: Arc<dyn RoutingDirectory>,
    pub settings: Settings,
}
