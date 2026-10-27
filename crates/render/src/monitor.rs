//! Port de suivi d'un rendu : l'app affiche l'avancement à sa façon (console, job) et peut l'interrompre.

/// Étape d'un rendu qui rapporte son avancement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Tuiles du fond de carte téléchargées dans le cache.
    Tiles,
    /// Images de la vidéo écrites dans l'encodeur.
    Frames,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Tiles => "tiles",
            Stage::Frames => "frames",
        }
    }
}

/// Partagé entre les threads du rendu. Chaque étape rapporte `done` à mesure, et `total / total`
/// exactement une fois, à la fin, même s'il n'y avait rien à faire.
pub trait Monitor: Sync {
    fn progress(&self, _stage: Stage, _done: u64, _total: u64) {}

    /// Vérifié entre deux tuiles et entre deux images : l'étape s'arrête alors sur `Err(CANCELLED)`.
    fn cancelled(&self) -> bool {
        false
    }
}

pub const CANCELLED: &str = "Rendu annulé";

/// Ni affichage ni interruption.
pub struct Silent;

impl Monitor for Silent {}
