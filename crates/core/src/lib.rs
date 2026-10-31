//! Noyau partagé du domaine : run, montage, lieux, thème, tournée, publication et leur géométrie. Aucune
//! entrée-sortie : les apps chargent, ce noyau valide et calcule.

pub mod geo;
pub mod json;
pub mod montage;
pub mod places;
pub mod publication;
pub mod run;
pub mod schedule;
pub mod theme;
pub mod tour;

/// Problème de validation d'un fichier : chemin de la valeur fautive (`video.fps`) et ce qui était attendu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub expected: String,
}

impl Issue {
    pub fn new(path: impl Into<String>, expected: impl Into<String>) -> Self {
        Issue { path: path.into(), expected: expected.into() }
    }
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = if self.path.is_empty() { "racine" } else { &self.path };
        write!(f, "{path} attendu {}", self.expected)
    }
}
