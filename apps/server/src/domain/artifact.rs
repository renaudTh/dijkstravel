//! Les fichiers qu'un run produit, étape par étape, et où ils se rangent dans le stockage.

use serde::{Deserialize, Serialize};

use super::production::RunId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Places,
    Tour,
    Video,
    /// Le début seul de la vidéo, pour juger vite d'un thème : il ne remplace jamais la vidéo complète.
    Preview,
    Cover,
    Publication,
}

impl ArtifactKind {
    pub const ALL: [ArtifactKind; 6] = [
        ArtifactKind::Places,
        ArtifactKind::Tour,
        ArtifactKind::Video,
        ArtifactKind::Preview,
        ArtifactKind::Cover,
        ArtifactKind::Publication,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Places => "places",
            ArtifactKind::Tour => "tour",
            ArtifactKind::Video => "video",
            ArtifactKind::Preview => "preview",
            ArtifactKind::Cover => "cover",
            ArtifactKind::Publication => "publication",
        }
    }

    pub fn parse(text: &str) -> Option<ArtifactKind> {
        ArtifactKind::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// Le nom du fichier, celui que la CLI écrit aussi.
    pub fn file_name(self) -> &'static str {
        match self {
            ArtifactKind::Places => "places.geojson",
            ArtifactKind::Tour => "tour.json",
            ArtifactKind::Video => "video.mp4",
            ArtifactKind::Preview => "preview.mp4",
            ArtifactKind::Cover => "cover.jpg",
            ArtifactKind::Publication => "publication.txt",
        }
    }

    pub fn from_file_name(name: &str) -> Option<ArtifactKind> {
        ArtifactKind::ALL.into_iter().find(|kind| kind.file_name() == name)
    }

    pub fn content_type(self) -> &'static str {
        match self {
            ArtifactKind::Places => "application/geo+json",
            ArtifactKind::Tour => "application/json",
            ArtifactKind::Video | ArtifactKind::Preview => "video/mp4",
            ArtifactKind::Cover => "image/jpeg",
            ArtifactKind::Publication => "text/plain; charset=utf-8",
        }
    }

    /// La vidéo que rend un job `render` : l'aperçu s'il n'en demande que le début.
    pub fn rendered(seconds: Option<f64>) -> ArtifactKind {
        if seconds.is_some() { ArtifactKind::Preview } else { ArtifactKind::Video }
    }

    /// Les entrées du run (`places`, `tour`) à la racine du run, ce qui s'en déduit sous `out/`.
    pub fn key(self, run: RunId) -> String {
        match self {
            ArtifactKind::Places | ArtifactKind::Tour => format!("runs/{run}/{}", self.file_name()),
            _ => format!("runs/{run}/out/{}", self.file_name()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub key: String,
    pub bytes: u64,
    /// Résumé lisible sans ouvrir le fichier : `{ "places": n }`, `{ "stops": n, "meters": m }`.
    pub meta: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_keeps_its_own_file() {
        assert_eq!(ArtifactKind::rendered(None), ArtifactKind::Video);
        assert_eq!(ArtifactKind::rendered(Some(10.0)), ArtifactKind::Preview);
        let run = RunId(uuid::Uuid::nil());
        assert_ne!(ArtifactKind::Preview.key(run), ArtifactKind::Video.key(run));
        assert_eq!(ArtifactKind::from_file_name("preview.mp4"), Some(ArtifactKind::Preview));
    }
}
