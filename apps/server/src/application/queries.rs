//! Lectures pour l'interface : des vues à plat, construites depuis le catalogue et la file.

use std::collections::HashMap;

use dijkstravel_core::run::{Profile, TourMode, VideoFormat};
use serde::Serialize;
use serde_json::Value;

use super::Studio;
use crate::domain::artifact::{Artifact, ArtifactKind};
use crate::domain::job::{Job, JobId};
use crate::domain::production::RunProduction;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub name: String,
    pub title: Option<String>,
    pub profile: &'static str,
    pub mode: &'static str,
    pub format: &'static str,
    pub theme: String,
    pub duration_sec: f64,
    pub places: Option<u64>,
    pub stops: Option<u64>,
    pub meters: Option<f64>,
    pub artifacts: Vec<ArtifactKind>,
    pub latest_job: Option<Job>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetail {
    #[serde(flatten)]
    pub summary: RunSummary,
    pub spec: Value,
    pub files: Vec<Artifact>,
    pub jobs: Vec<Job>,
}

pub fn profile_name(profile: Profile) -> &'static str {
    match profile {
        Profile::Car => "car",
        Profile::Foot => "foot",
        Profile::Train => "train",
    }
}

fn summary(production: &RunProduction, latest_job: Option<Job>) -> RunSummary {
    let run = &production.run;
    let meta = |kind: ArtifactKind, field: &str| {
        production.artifacts.get(&kind).and_then(|artifact| artifact.meta.get(field).cloned())
    };
    RunSummary {
        name: run.name.clone(),
        title: run.title.clone(),
        profile: profile_name(run.profile),
        mode: match run.mode {
            TourMode::Loop => "loop",
            TourMode::Open => "open",
        },
        format: match run.video.format {
            VideoFormat::Landscape => "16:9",
            VideoFormat::Portrait => "9:16",
        },
        theme: run.theme.clone(),
        duration_sec: run.video.duration_sec,
        places: meta(ArtifactKind::Places, "places").and_then(|value| value.as_u64()),
        stops: meta(ArtifactKind::Tour, "stops").and_then(|value| value.as_u64()),
        meters: meta(ArtifactKind::Tour, "meters").and_then(|value| value.as_f64()),
        artifacts: production.artifacts.keys().copied().collect(),
        latest_job,
    }
}

impl Studio {
    pub async fn runs(&self) -> anyhow::Result<Vec<RunSummary>> {
        let mut latest: HashMap<_, Job> = self.jobs.latest().await?.into_iter().map(|job| (job.run_id, job)).collect();
        let mut runs: Vec<RunSummary> = self
            .catalog
            .productions()
            .await?
            .iter()
            .map(|production| summary(production, latest.remove(&production.id)))
            .collect();
        runs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(runs)
    }

    pub async fn run(&self, name: &str) -> anyhow::Result<Option<RunDetail>> {
        let Some(production) = self.catalog.production(name).await? else { return Ok(None) };
        let jobs = self.jobs.for_run(production.id, 20).await?;
        Ok(Some(RunDetail {
            summary: summary(&production, jobs.first().cloned()),
            spec: production.spec.clone(),
            files: production.artifacts.values().cloned().collect(),
            jobs,
        }))
    }

    pub async fn job(&self, id: JobId) -> anyhow::Result<Option<Job>> {
        self.jobs.get(id).await
    }

    /// Le fichier `kind` du run, s'il a été produit.
    pub async fn file(&self, name: &str, kind: ArtifactKind) -> anyhow::Result<Option<Artifact>> {
        Ok(self.catalog.production(name).await?.and_then(|production| production.artifacts.get(&kind).cloned()))
    }
}
