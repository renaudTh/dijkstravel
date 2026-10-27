//! L'API HTTP du studio (`/api/*`) et le build Angular servi à la racine, avec repli sur
//! `index.html` pour les routes de l'application.

use std::convert::Infallible;
use std::path::Path;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use dijkstravel_core::run::Profile;
use futures::Stream;
use serde_json::{Value, json};
use sqlx::postgres::{PgListener, PgPool};
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::application::Studio;
use crate::application::enqueue::{CancelError, EnqueueError};
use crate::application::queries::profile_name;
use crate::domain::artifact::ArtifactKind;
use crate::domain::job::{JobId, JobKind};
use crate::infrastructure::postgres::JOB_EVENTS;

#[derive(Clone)]
struct AppState {
    studio: Studio,
    events: broadcast::Sender<String>,
}

/// Une erreur rendue au client : `{ "error": "..." }`.
struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        tracing::error!("{error:#}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}"))
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn not_found(what: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, format!("{what} introuvable"))
}

pub async fn serve(studio: Studio, pool: PgPool, listen: &str, web_dir: &Path) -> anyhow::Result<()> {
    let (events, _) = broadcast::channel(256);
    tokio::spawn(relay_job_events(studio.clone(), pool, events.clone()));
    let index = web_dir.join("index.html");
    let app = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/routing", get(routing))
        .route("/api/runs", get(runs))
        .route("/api/runs/{name}", get(run))
        .route("/api/runs/{name}/jobs", post(enqueue))
        .route("/api/runs/{name}/files/{file}", get(file))
        .route("/api/jobs/{id}", get(job))
        .route("/api/jobs/{id}/cancel", post(cancel))
        .route("/api/events", get(events_stream))
        .fallback_service(ServeDir::new(web_dir).fallback(ServeFile::new(index)))
        .layer(TraceLayer::new_for_http())
        .with_state(AppState { studio, events });
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!("studio sur http://{listen}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Chaque `NOTIFY job_events` (identifiant d'un job) devient le job à jour, diffusé à tous les clients.
async fn relay_job_events(studio: Studio, pool: PgPool, events: broadcast::Sender<String>) {
    loop {
        let relay = async {
            let mut listener = PgListener::connect_with(&pool).await?;
            listener.listen(JOB_EVENTS).await?;
            loop {
                let notification = listener.recv().await?;
                let Ok(id) = notification.payload().parse::<Uuid>() else { continue };
                if let Some(job) = studio.job(JobId(id)).await? {
                    let _ = events.send(serde_json::to_string(&job)?);
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        };
        if let Err(error) = relay.await {
            tracing::warn!("relais des événements : {error:#}");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn routing(State(state): State<AppState>) -> Json<Value> {
    let mut status = serde_json::Map::new();
    for profile in [Profile::Car, Profile::Foot, Profile::Train] {
        status.insert(profile_name(profile).to_owned(), state.studio.routing.available(profile).await.into());
    }
    Json(Value::Object(status))
}

async fn runs(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    Ok(Json(state.studio.runs().await?))
}

async fn run(State(state): State<AppState>, UrlPath(name): UrlPath<String>) -> ApiResult<impl IntoResponse> {
    Ok(Json(state.studio.run(&name).await?.ok_or_else(|| not_found(format!("run {name}")))?))
}

async fn enqueue(
    State(state): State<AppState>,
    UrlPath(name): UrlPath<String>,
    Json(kind): Json<JobKind>,
) -> ApiResult<impl IntoResponse> {
    match state.studio.enqueue(&name, kind).await {
        Ok(job) => Ok((StatusCode::CREATED, Json(job))),
        Err(EnqueueError::UnknownRun) => Err(not_found(format!("run {name}"))),
        Err(EnqueueError::Invalid(message)) => Err(ApiError(StatusCode::BAD_REQUEST, message)),
        Err(EnqueueError::Refused(refusal)) => Err(ApiError(StatusCode::CONFLICT, refusal.to_string())),
        Err(EnqueueError::Busy(job)) => {
            Err(ApiError(StatusCode::CONFLICT, format!("un job est déjà en cours ({job})")))
        }
        Err(EnqueueError::RoutingDown(profile)) => Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("routage {} injoignable (OSRM ou VROOM arrêté)", profile_name(profile)),
        )),
        Err(EnqueueError::Internal(error)) => Err(error.into()),
    }
}

async fn job(State(state): State<AppState>, UrlPath(id): UrlPath<Uuid>) -> ApiResult<impl IntoResponse> {
    Ok(Json(state.studio.job(JobId(id)).await?.ok_or_else(|| not_found(format!("job {id}")))?))
}

async fn cancel(State(state): State<AppState>, UrlPath(id): UrlPath<Uuid>) -> ApiResult<impl IntoResponse> {
    match state.studio.cancel(JobId(id)).await {
        Ok(job) => Ok(Json(job)),
        Err(CancelError::UnknownJob) => Err(not_found(format!("job {id}"))),
        Err(CancelError::Transition(transition)) => Err(ApiError(StatusCode::CONFLICT, transition.to_string())),
        Err(CancelError::Internal(error)) => Err(error.into()),
    }
}

async fn events_stream(State(state): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Un client trop lent perd des événements (`Lagged`) : le suivant le remet à jour.
    let stream = BroadcastStream::new(state.events.subscribe())
        .filter_map(|message| message.ok())
        .map(|job| Ok(Event::default().event("job").data(job)));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// `bytes=a-b`, `bytes=a-` ou `bytes=-n` sur un fichier de `size` octets ; `None` si illisible ou hors du fichier.
fn byte_range(header: &str, size: u64) -> Option<std::ops::Range<u64>> {
    let spec = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (start, end) = spec.split_once('-')?;
    let range = match (start.trim(), end.trim()) {
        ("", suffix) => size.saturating_sub(suffix.parse().ok()?)..size,
        (start, "") => start.parse().ok()?..size,
        (start, end) => start.parse().ok()?..(end.parse::<u64>().ok()? + 1).min(size),
    };
    (range.start < range.end).then_some(range)
}

/// Le fichier, relayé depuis le stockage ; les requêtes `Range` permettent de parcourir la vidéo.
async fn file(
    State(state): State<AppState>,
    UrlPath((name, file)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let kind = ArtifactKind::from_file_name(&file).ok_or_else(|| not_found(&file))?;
    let artifact = state.studio.file(&name, kind).await?.ok_or_else(|| not_found(format!("{name}/{file}")))?;
    let store = &state.studio.store;
    let size = store.size(&artifact.key).await?;
    let content_type = HeaderValue::from_static(kind.content_type());
    let requested = headers.get(header::RANGE).and_then(|value| value.to_str().ok());
    let mut response = match requested {
        None => {
            let body = store.get(&artifact.key).await?;
            (StatusCode::OK, Body::from(body)).into_response()
        }
        Some(requested) => match byte_range(requested, size) {
            None => {
                let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                let unsatisfied = HeaderValue::from_str(&format!("bytes */{size}")).expect("en-tête ASCII");
                response.headers_mut().insert(header::CONTENT_RANGE, unsatisfied);
                return Ok(response);
            }
            Some(range) => {
                let content_range = format!("bytes {}-{}/{size}", range.start, range.end - 1);
                let body = store.get_range(&artifact.key, range).await?;
                let mut response = (StatusCode::PARTIAL_CONTENT, Body::from(body)).into_response();
                response
                    .headers_mut()
                    .insert(header::CONTENT_RANGE, HeaderValue::from_str(&content_range).expect("en-tête ASCII"));
                response
            }
        },
    };
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::byte_range;

    #[test]
    fn reads_the_three_forms_of_a_byte_range() {
        assert_eq!(byte_range("bytes=0-99", 1000), Some(0..100));
        assert_eq!(byte_range("bytes=900-", 1000), Some(900..1000));
        assert_eq!(byte_range("bytes=-100", 1000), Some(900..1000));
        assert_eq!(byte_range("bytes=990-2000", 1000), Some(990..1000));
    }

    #[test]
    fn rejects_a_range_outside_the_file() {
        assert_eq!(byte_range("bytes=1000-", 1000), None);
        assert_eq!(byte_range("lines=0-1", 1000), None);
        assert_eq!(byte_range("bytes=a-b", 1000), None);
    }
}
