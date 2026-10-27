//! Les fichiers qu'un run produit, étape par étape, et où ils se rangent dans le stockage.

use serde::{Deserialize, Serialize};

use super::production::RunId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Places,
    Tour,
    Video,
    Cover,
    Publication,
}

impl ArtifactKind {
    pub const ALL: [ArtifactKind; 5] =
        [ArtifactKind::Places, ArtifactKind::Tour, ArtifactKind::Video, ArtifactKind::Cover, ArtifactKind::Publication];

    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Places => "places",
            ArtifactKind::Tour => "tour",
            ArtifactKind::Video => "video",
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
            ArtifactKind::Video => "video/mp4",
            ArtifactKind::Cover => "image/jpeg",
            ArtifactKind::Publication => "text/plain; charset=utf-8",
        }
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
