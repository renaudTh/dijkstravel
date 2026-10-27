//! L'application de bout en bout sur des adaptateurs en mémoire : file, étapes, issues.

use std::sync::Arc;

use bytes::Bytes;
use dijkstravel_core::json::to_js_string;
use dijkstravel_core::places::{Place, to_places_file};
use serde_json::json;

use super::enqueue::EnqueueError;
use super::{Settings, Studio};
use crate::domain::artifact::ArtifactKind;
use crate::domain::job::{JobKind, JobStatus};
use crate::domain::ports::{ArtifactStore, Catalog, artifact};
use crate::domain::production::Refusal;
use crate::domain::production::tests::spec;
use crate::infrastructure::memory::{Memory, StraightLines};

fn studio(memory: Arc<Memory>, scratch: &std::path::Path) -> Studio {
    Studio {
        catalog: memory.clone(),
        jobs: memory.clone(),
        store: memory.clone(),
        events: memory,
        routing: Arc::new(StraightLines),
        settings: Settings {
            cache_dir: scratch.join("cache"),
            scratch_dir: scratch.join("work"),
            render_threads: 1,
            overpass_url: "http://overpass.invalid".to_owned(),
        },
    }
}

/// Un run de trois lieux bretons, sourcé (places.geojson présent).
async fn sourced(memory: &Memory) {
    let id = memory.save_run(&spec()).await.unwrap();
    let places = [
        Place::new("rennes", "Rennes", [-1.68, 48.11]),
        Place::new("brest", "Brest", [-4.49, 48.39]),
        Place::new("vannes", "Vannes", [-2.76, 47.66]),
    ];
    let text = to_js_string(&to_places_file(&places));
    let bytes = memory.put(&ArtifactKind::Places.key(id), Bytes::from(text)).await.unwrap();
    memory.record_artifact(id, &artifact(ArtifactKind::Places, id, bytes, json!({ "places": 3 })), None).await.unwrap();
}

#[tokio::test]
async fn solves_a_sourced_run_and_records_its_tour() {
    let scratch = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::default());
    sourced(&memory).await;
    let studio = studio(memory.clone(), scratch.path());

    let job = studio.enqueue("essai", JobKind::Solve).await.unwrap();
    assert!(matches!(studio.enqueue("essai", JobKind::Solve).await, Err(EnqueueError::Busy(busy)) if busy == job.id));
    assert!(studio.process_next("test").await.unwrap());
    assert!(!studio.process_next("test").await.unwrap());

    let done = studio.job(job.id).await.unwrap().unwrap();
    assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
    assert!(done.log.iter().any(|line| line.starts_with("3 lieux")), "{:?}", done.log);
    let detail = studio.run("essai").await.unwrap().unwrap();
    assert_eq!(detail.summary.stops, Some(3));
    let production = memory.production("essai").await.unwrap().unwrap();
    let tour = memory.get(&production.artifacts[&ArtifactKind::Tour].key).await.unwrap();
    assert!(String::from_utf8(tour.to_vec()).unwrap().contains("\"stops\""));
    let statuses: Vec<JobStatus> = memory.events.lock().unwrap().iter().map(|(_, status)| *status).collect();
    assert_eq!(statuses.first(), Some(&JobStatus::Queued));
    assert_eq!(statuses.last(), Some(&JobStatus::Succeeded));
}

#[tokio::test]
async fn refuses_a_step_whose_inputs_are_missing() {
    let scratch = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::default());
    memory.save_run(&spec()).await.unwrap();
    let studio = studio(memory, scratch.path());
    assert!(matches!(
        studio.enqueue("essai", JobKind::Render { seconds: None }).await,
        Err(EnqueueError::Refused(Refusal::Missing(ArtifactKind::Tour)))
    ));
    assert!(matches!(studio.enqueue("inconnu", JobKind::Solve).await, Err(EnqueueError::UnknownRun)));
}

#[tokio::test]
async fn refuses_to_solve_when_the_profile_has_no_routing() {
    let scratch = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::default());
    let mut train = spec();
    train["profile"] = json!("train");
    let id = memory.save_run(&train).await.unwrap();
    memory.record_artifact(id, &artifact(ArtifactKind::Places, id, 1, json!({})), None).await.unwrap();
    let studio = studio(memory, scratch.path());
    assert!(matches!(studio.enqueue("essai", JobKind::Solve).await, Err(EnqueueError::RoutingDown(_))));
}

#[tokio::test]
async fn a_cancelled_queued_job_is_never_processed() {
    let scratch = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::default());
    sourced(&memory).await;
    let studio = studio(memory, scratch.path());
    let job = studio.enqueue("essai", JobKind::Solve).await.unwrap();
    let cancelled = studio.cancel(job.id).await.unwrap();
    assert_eq!(cancelled.status, JobStatus::Cancelled);
    assert!(!studio.process_next("test").await.unwrap());
    assert!(studio.cancel(job.id).await.is_err());
}

#[tokio::test]
async fn a_failing_step_fails_the_job_with_its_reason() {
    let scratch = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::default());
    let mut with_overpass = spec();
    with_overpass["overpass"] = json!({ "areaIso": "FR-BRE", "filter": "[\"historic\"=\"castle\"]" });
    memory.save_run(&with_overpass).await.unwrap();
    let studio = studio(memory, scratch.path());
    let job = studio.enqueue("essai", JobKind::Source).await.unwrap();
    assert!(studio.process_next("test").await.unwrap());
    let failed = studio.job(job.id).await.unwrap().unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert!(failed.error.unwrap().contains("Overpass"));
}
