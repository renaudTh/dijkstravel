//! Agrégat `RunProduction` : un run et ce qui en a déjà été produit. Il décide quelles étapes peuvent
//! être lancées : une étape n'est acceptée que si ses entrées existent.

use std::collections::BTreeMap;

use dijkstravel_core::run::{Profile, Run};
use serde::Serialize;
use uuid::Uuid;

use super::artifact::{Artifact, ArtifactKind};
use super::job::JobKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct RunId(pub Uuid);

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub struct RunProduction {
    pub id: RunId,
    pub run: Run,
    /// `run.json` tel qu'enregistré, rendu à l'interface.
    pub spec: serde_json::Value,
    pub artifacts: BTreeMap<ArtifactKind, Artifact>,
}

/// Pourquoi une étape ne peut pas être lancée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Il manque ce fichier, produit par une étape précédente.
    Missing(ArtifactKind),
    /// Le run n'a pas de section `overpass` : ses lieux viennent d'ailleurs.
    NoOverpassSection,
    /// Le run n'a pas de section `publication` (légende, hashtags).
    NoPublicationSection,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Missing(ArtifactKind::Places) => write!(f, "lieux absents : sourcer le run d'abord"),
            Refusal::Missing(ArtifactKind::Tour) => write!(f, "tournée absente : la calculer d'abord"),
            Refusal::Missing(kind) => write!(f, "{} absent", kind.file_name()),
            Refusal::NoOverpassSection => write!(f, "le run n'a pas de section overpass"),
            Refusal::NoPublicationSection => write!(f, "le run n'a pas de section publication"),
        }
    }
}

impl RunProduction {
    pub fn name(&self) -> &str {
        &self.run.name
    }

    pub fn has(&self, kind: ArtifactKind) -> bool {
        self.artifacts.contains_key(&kind)
    }

    /// Le profil de routage, si l'étape en a besoin.
    pub fn routing_needed(&self, kind: &JobKind) -> Option<Profile> {
        matches!(kind, JobKind::Solve | JobKind::All).then_some(self.run.profile)
    }

    pub fn can_enqueue(&self, kind: &JobKind) -> Result<(), Refusal> {
        let require = |artifact| if self.has(artifact) { Ok(()) } else { Err(Refusal::Missing(artifact)) };
        match kind {
            JobKind::Source => self.run.overpass.as_ref().map(|_| ()).ok_or(Refusal::NoOverpassSection),
            JobKind::Solve => require(ArtifactKind::Places),
            JobKind::Render { .. } => require(ArtifactKind::Tour),
            JobKind::Publication => {
                require(ArtifactKind::Tour)?;
                self.run.publication.as_ref().map(|_| ()).ok_or(Refusal::NoPublicationSection)
            }
            JobKind::All => require(ArtifactKind::Places),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use serde_json::json;

    use super::*;

    pub fn production(spec: serde_json::Value, artifacts: &[ArtifactKind]) -> RunProduction {
        let id = RunId(Uuid::new_v4());
        RunProduction {
            id,
            run: Run::parse(&spec).expect("run valide"),
            spec,
            artifacts: artifacts
                .iter()
                .map(|&kind| (kind, Artifact { kind, key: kind.key(id), bytes: 1, meta: json!({}) }))
                .collect(),
        }
    }

    pub fn spec() -> serde_json::Value {
        json!({
            "name": "essai",
            "source": { "kind": "csv", "description": "essai", "date": "2026-10-27", "licence": "CC0" },
            "bbox": { "west": -5.0, "south": 47.0, "east": -1.0, "north": 49.0 },
            "mode": "loop"
        })
    }

    #[test]
    fn each_step_requires_the_output_of_the_previous_one() {
        let bare = production(spec(), &[]);
        assert_eq!(bare.can_enqueue(&JobKind::Solve), Err(Refusal::Missing(ArtifactKind::Places)));
        assert_eq!(bare.can_enqueue(&JobKind::Render { seconds: None }), Err(Refusal::Missing(ArtifactKind::Tour)));
        let sourced = production(spec(), &[ArtifactKind::Places]);
        assert_eq!(sourced.can_enqueue(&JobKind::Solve), Ok(()));
        assert_eq!(sourced.can_enqueue(&JobKind::All), Ok(()));
        let solved = production(spec(), &[ArtifactKind::Places, ArtifactKind::Tour]);
        assert_eq!(solved.can_enqueue(&JobKind::Render { seconds: Some(5.0) }), Ok(()));
    }

    #[test]
    fn sourcing_and_publication_need_their_section() {
        let solved = production(spec(), &[ArtifactKind::Places, ArtifactKind::Tour]);
        assert_eq!(solved.can_enqueue(&JobKind::Source), Err(Refusal::NoOverpassSection));
        assert_eq!(solved.can_enqueue(&JobKind::Publication), Err(Refusal::NoPublicationSection));
        let mut with_sections = spec();
        with_sections["overpass"] = json!({ "areaIso": "FR-BRE", "filter": "[\"historic\"=\"castle\"]" });
        with_sections["publication"] = json!({ "caption": "{lieux} châteaux", "hashtags": ["#bzh"] });
        let ready = production(with_sections, &[ArtifactKind::Tour]);
        assert_eq!(ready.can_enqueue(&JobKind::Source), Ok(()));
        assert_eq!(ready.can_enqueue(&JobKind::Publication), Ok(()));
    }

    #[test]
    fn only_solving_needs_routing() {
        let run = production(spec(), &[]);
        assert_eq!(run.routing_needed(&JobKind::Solve), Some(Profile::Car));
        assert_eq!(run.routing_needed(&JobKind::Render { seconds: None }), None);
    }
}
