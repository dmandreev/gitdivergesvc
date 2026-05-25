use anyhow::{bail, Context};
use axum::http::{header, HeaderValue, Request, StatusCode, Uri};
use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use gitdiverge_lib::{
    analyze_branch_divergence, BranchAnalyticsSummary, BranchComparisonSummary, BranchStatus,
    Commit, GitCredential, GitProvider, GixProvider, ProcessGitProvider, ProgressReporter,
    RepoIndex, RepoSpec,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, instrument, trace, warn};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use crate::auth::{generate_test_token, AuthState, TestTokenResponse};
use crate::divergence_cache::DivergenceCache;

use axum::response::sse::{Event, KeepAlive, Sse};
use gitdiverge_lib::{ProgressEvent, SseProgress};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;

/// Manages per-repository locks to serialize git operations on the same repo.
///
/// Keys can be either a repository URL (for clone operations) or a GUID
/// (for fetch / divergence operations).
#[derive(Debug, Clone)]
pub struct RepoLockManager {
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl Default for RepoLockManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RepoLockManager {
    /// Create a new empty lock manager.
    pub fn new() -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Acquire a lock for the given key (URL or GUID).
    ///
    /// The returned `Arc<Mutex<()>>` can be moved into a blocking task.
    pub fn acquire(&self, key: String) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().unwrap();
        locks
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

/// Whether the `webclient` static files were embedded at compile time.
pub const WEBCLIENT_EMBEDDED: bool = cfg!(webclient_present);

/// Conditionally-compiled accessor for the embedded `webclient` directory.
#[cfg(webclient_present)]
mod webclient {
    use include_dir::{include_dir, Dir};

    pub static DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../webclientsrc/dist");

    pub fn get_file(path: &str) -> Option<&'static [u8]> {
        DIR.get_file(path).map(|f| f.contents())
    }

    pub fn get_index_html() -> Option<&'static [u8]> {
        DIR.get_file("index.html").map(|f| f.contents())
    }
}

/// Stub implementation used when the `webclient` folder is missing or empty.
/// Every request results in a 404.
#[cfg(not(webclient_present))]
mod webclient {
    pub fn get_file(_path: &str) -> Option<&'static [u8]> {
        None
    }

    pub fn get_index_html() -> Option<&'static [u8]> {
        None
    }
}

/// Shared application state for the daemon.
#[derive(Clone)]
pub struct AppState {
    pub clone_dir: PathBuf,
    pub git: ProcessGitProvider,
    pub repo_locks: Arc<RepoLockManager>,
    pub auth: Arc<AuthState>,
    pub webclient: crate::config::WebClientConfig,
    pub divergence_cache: DivergenceCache,
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Error response returned by the API when a request cannot be fulfilled.
///
/// The `error` field contains a short human-readable message, while `details`
/// may hold additional context such as the underlying git stderr output.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ApiError {
    /// Short human-readable error message.
    pub error: String,
    /// Optional additional context (e.g. git stderr, IO path, parse hint).
    pub details: Option<String>,
}

/// Request body for cloning a remote repository.
///
/// The daemon stores cloned repositories under a GUID-derived directory.
/// If the repository has already been cloned, the endpoint returns
/// `already_exists` without re-cloning unless `force` is set to `true`.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CloneRequest {
    /// Git remote URL. Supported formats include HTTPS (`https://host/owner/repo.git`)
    /// and SSH (`git@host:owner/repo.git`).
    pub url: String,
    /// When `true`, remove the existing local repository (if any) and re-clone
    /// it under the same GUID.
    #[serde(default)]
    pub force: bool,
}

/// Result of a clone operation.
///
/// `repo_guid` is the stable directory name derived from the repository URL and
/// is used in all subsequent API calls targeting this repository.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CloneResponse {
    /// Human-readable repository name (derived from the URL unless overridden).
    pub repo_name: String,
    /// Stable GUID assigned to this repository in the local index.
    pub repo_guid: String,
    /// Absolute filesystem path where the repository was (or already is) cloned.
    pub repo_path: String,
    /// Operation status: `"cloned"`, `"already_exists"` or `"recloned"`.
    pub status: String,
    /// Human-readable description of the outcome.
    pub message: String,
}

/// Request body for fetching updates in an existing repository.
///
/// The daemon runs `git fetch origin`, validates every branch in `branches`
/// against the remote, and checks out the valid ones locally.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FetchRequest {
    /// Branches to validate and checkout after fetching.
    /// Non-existent branches are reported in `branch_statuses` with `exists: false`
    /// and are skipped during checkout.
    pub branches: Vec<String>,
}

/// Result of a fetch operation.
///
/// `branch_statuses` contains one entry per requested branch, indicating whether
/// it exists on the remote. At least one valid branch is required for checkout.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FetchResponse {
    /// Human-readable repository name.
    pub repo_name: String,
    /// Stable GUID of the repository.
    pub repo_guid: String,
    /// Absolute filesystem path of the repository.
    pub repo_path: String,
    /// Operation status: always `"fetched"` on success.
    pub status: String,
    /// Per-branch existence status after checking the remote.
    pub branch_statuses: Vec<BranchStatus>,
    /// Human-readable description of how many branches were checked out.
    pub message: String,
}

/// Request body for branch-divergence analysis of a single repository.
///
/// The endpoint first fetches from `origin`, validates the requested branches,
/// checks out the valid ones, and then compares every ordered branch pair to
/// compute missing commits.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DivergenceRequest {
    /// Branches to compare. At least one branch must exist on the remote.
    /// The analysis generates `n*(n-1)` comparisons for `n` valid branches.
    pub branches: Vec<String>,
}

/// Query parameters for branch-divergence analysis of a single repository.
///
/// The endpoint validates the requested branches against local remote-tracking
/// refs and then computes pairwise commit divergence using the `gix` library
/// provider. No network fetch is performed; call `POST /fetch` first if you
/// need fresh remote data.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct DivergenceQuery {
    /// Comma-separated list of branches to compare.
    pub branches: String,
}

/// Branch-divergence analysis result for a single repository.
///
/// `analytics` contains lightweight summaries (counts only) for every branch
/// pair. Use `GET /api/v1/repos/{repo_guid}/divergence/commits` to retrieve
/// the actual commit lists paginated.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DivergenceResponse {
    /// Human-readable repository name.
    pub repo_name: String,
    /// Stable GUID of the repository.
    pub repo_guid: String,
    /// Absolute filesystem path of the repository.
    pub repo_path: String,
    /// Per-branch existence status after remote validation.
    pub branch_statuses: Vec<BranchStatus>,
    /// Lightweight divergence analytics (counts only) for all valid branch pairs.
    pub analytics: BranchAnalyticsSummary,
}

/// Request body for batch branch-divergence analysis across multiple repositories.
///
/// The daemon runs a three-phase pipeline for every repository:
/// 1. Clone repositories that are missing locally (or skip if already present).
/// 2. Fetch updates and validate/checkout the requested branches.
/// 3. Run pairwise branch-divergence analysis.
///
/// Results are returned as an array with one item per requested repository.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BatchDivergenceRequest {
    /// List of repositories to process. Each entry must contain a `url`;
    /// the repository name is derived automatically from the URL.
    pub repos: Vec<BatchRepoRequest>,
    /// Branches to validate, checkout, and analyze in every repository.
    pub branches: Vec<String>,
}

/// Single repository entry inside a batch divergence request.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BatchRepoRequest {
    /// Git remote URL. Supported formats include HTTPS and SSH.
    pub url: String,
}

/// Result item for a single repository in a batch divergence analysis.
///
/// If a repository fails at any phase, `analytics` will be `null` and `error`
/// will contain a description of the failure.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BatchDivergenceResponseItem {
    /// Human-readable repository name.
    pub repo_name: String,
    /// Stable GUID assigned to the repository in the local index.
    pub repo_guid: String,
    /// Original remote URL used for cloning.
    pub repo_url: String,
    /// Absolute filesystem path of the repository.
    pub repo_path: String,
    /// Per-branch existence status after remote validation.
    pub branch_statuses: Vec<BranchStatus>,
    /// Lightweight divergence analytics (counts only) when successful.
    pub analytics: Option<BranchAnalyticsSummary>,
    /// Error message when the repository could not be cloned, fetched, or analyzed.
    pub error: Option<String>,
}

/// Query parameters for retrieving paginated commits from a branch comparison.
#[derive(Serialize, Deserialize, utoipa::ToSchema, utoipa::IntoParams, Debug, Clone)]
pub struct PagedCommitsRequest {
    /// Source branch (the branch that may contain missing commits).
    pub source_branch: String,
    /// Target branch (the branch being compared against).
    pub target_branch: String,
    /// Comma-separated list of branches that were included in the divergence analysis.
    pub branches: String,
    /// Zero-based page index.
    pub page: usize,
    /// Number of commits per page.
    pub page_size: usize,
}

/// Response for a paginated commit query.
#[derive(Serialize, utoipa::ToSchema, Debug, Clone)]
pub struct PagedCommitsResponse {
    /// Source branch name.
    pub source_branch: String,
    /// Target branch name.
    pub target_branch: String,
    /// Current page index.
    pub page: usize,
    /// Number of commits per page.
    pub page_size: usize,
    /// Total number of missing commits for this branch pair.
    pub total_commits: usize,
    /// Commits in the requested page.
    pub commits: Vec<Commit>,
}

/// Health-check response.
///
/// Indicates whether the daemon is running and able to serve requests.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct HealthResponse {
    /// Always `"ok"` when the service is healthy.
    pub status: String,
    /// Daemon version string (taken from `CARGO_PKG_VERSION`).
    pub version: String,
}

/// Single repository entry in the local index.
///
/// The index maps repository URLs to stable GUIDs and local paths, allowing
/// the daemon to locate repositories without exposing filesystem details to
/// the caller.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RepoIndexEntry {
    /// Stable GUID (directory name for the repository).
    pub guid: String,
    /// Human-readable repository name.
    pub name: String,
    /// Original remote URL.
    pub url: String,
    /// Absolute filesystem path of the cloned repository.
    pub repo_path: String,
}

/// Response for listing branches in a repository.
///
/// Returns the short names of all remote-tracking branches (e.g. `"main"`,
/// `"feature-x"`) after stripping the remote name prefix.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BranchesResponse {
    /// List of branch names.
    pub branches: Vec<String>,
}

// ---------------------------------------------------------------------------
// SSE stream event types
// ---------------------------------------------------------------------------

/// Wrapper enum used to serialise events on an SSE stream.
///
/// The `type` field discriminates between progress updates, the final
/// successful payload, or an error that occurred during processing.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamEvent<T: Serialize> {
    Progress {
        /// The wrapped progress event.
        event: ProgressEvent,
        /// Elapsed milliseconds since the stream started when the event was emitted.
        ts: u64,
    },
    Complete {
        /// Final result payload.
        data: T,
    },
    Error {
        /// Human-readable error message.
        message: String,
        /// Optional additional context.
        details: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// OpenAPI definition (generated at runtime from typed code)
// ---------------------------------------------------------------------------

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.as_mut().unwrap();
        components.add_security_scheme(
            "bearer_auth",
            utoipa::openapi::security::SecurityScheme::Http(
                utoipa::openapi::security::HttpBuilder::new()
                    .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    modifiers(&SecurityAddon),
    paths(
        health,
        test_token,
        list_repos,
        list_branches,
        clone_repo,
        fetch_repo,
        repo_divergence,
        get_divergence_commits,
        batch_divergence,
    ),
    components(schemas(
        ApiError,
        CloneRequest,
        CloneResponse,
        FetchRequest,
        FetchResponse,
        DivergenceRequest,
        DivergenceQuery,
        DivergenceResponse,
        BatchDivergenceRequest,
        BatchDivergenceResponseItem,
        HealthResponse,
        RepoIndexEntry,
        BranchesResponse,
        TestTokenResponse,
        PagedCommitsRequest,
        PagedCommitsResponse,
        BranchComparisonSummary,
        BranchAnalyticsSummary,
        // Re-export library schemas so the spec is self-contained.
        gitdiverge_lib::Author,
        gitdiverge_lib::Commit,
        gitdiverge_lib::LogOptions,
        gitdiverge_lib::BranchComparison,
        gitdiverge_lib::BranchAnalytics,
        BatchRepoRequest,
        gitdiverge_lib::BranchStatus,
        gitdiverge_lib::ProgressEvent,
    )),
    info(
        title = "GitDiverge Daemon API",
        version = "0.1.0",
        description = r#"HTTP API for cloning, fetching and analysing branch divergence across git repositories.

**Authentication**

All API endpoints except `GET /health` and `GET /auth/test-token` require a valid
Bearer JWT in the `Authorization` header. The token is validated locally using
the configured JWKS endpoint or a local test key. No calls to Keycloak are made
during request handling.

Click the *Authorize* button in Swagger UI and paste a token. When running in
`Local` auth mode, retrieve a test token from `GET /auth/test-token`.


**Server-Sent Events (SSE)**

All mutating endpoints (`clone`, `fetch`, `batch/divergence`) return a
Server-Sent Events stream (`text/event-stream`). While the operation runs the server
emits `event: progress` messages. The stream ends with either:

- `event: complete` — the final JSON payload for the operation.
- `event: error` — an error message with optional details.

The only non-streaming endpoints are `GET /health`, `GET /api/v1/repos`, and `GET /api/v1/repos/{repo_guid}/divergence`.

**Core concepts**

- *Repository index* — Every cloned repository is assigned a stable GUID. The daemon
  maintains an index file (`repos.json`) that maps URLs to GUIDs.
  All mutating endpoints update this index automatically.

- *Clone idempotency* — `POST /api/v1/repos/clone` is safe to retry. If the repository
  already exists locally the endpoint returns `already_exists` without re-cloning.

- *Branch validation* — Fetch and divergence endpoints validate every requested branch
  against `origin/<branch>`. Non-existent branches are reported in `branch_statuses`
  with `exists: false` and are skipped during checkout.

- *Divergence analysis* — For every ordered pair of valid branches `(target, source)`,
  the daemon computes the commits that exist in `source` but are **not** reachable from
  `target` (judged by commit-hash equality). For `n` valid branches this produces
  `n*(n-1)` comparisons.

- *Batch pipeline* — `POST /api/v1/batch/divergence` runs a three-phase workflow
  (clone → fetch/checkout → analytics) in parallel across all requested repositories.

**Concurrency**

The daemon is safe to use from multiple clients simultaneously. Requests targeting
*different* repositories run fully in parallel. Requests targeting the *same*
repository are automatically serialized so that git operations do not collide:

- `clone` locks by repository URL (preventing duplicate GUIDs for the same URL)
  and then by GUID, so fetch/divergence requests for that repo wait until cloning
  finishes.
- `fetch` and `divergence` lock by repository GUID. Two divergence analyses on
  the same repo will run sequentially, not concurrently.
- `batch/divergence` acquires locks for every repository in the batch (in sorted
  order to avoid deadlocks) before starting work. A batch request and a single-repo
  request for an overlapping repo will be serialized correctly.

Because the underlying git working directory is mutated (fetch, checkout, clone),
there is no benefit — and significant risk — in running overlapping git commands
on the same repo concurrently. The locking ensures correctness at the expense of
queueing identical overlapping requests.

**Error handling**

All error responses follow the `ApiError` schema. `400 Bad Request` is returned for
invalid input or missing repositories; `500 Internal Server Error` is returned for
git command failures or unexpected IO errors. The `details` field often contains the
underlying git stderr output."#
    )
)]
pub struct ApiDoc;

/// Generate the OpenAPI JSON specification.
pub fn openapi_json() -> anyhow::Result<String> {
    let spec = ApiDoc::openapi();
    Ok(serde_json::to_string_pretty(&spec)?)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

fn repo_name_from_url(url: &str) -> String {
    let name = url.rsplit('/').next().unwrap_or("repo");
    if name.is_empty() {
        "repo".to_string()
    } else {
        name.trim_end_matches(".git").to_string()
    }
}

fn map_err(e: gitdiverge_lib::Error) -> (StatusCode, Json<ApiError>) {
    let (status, details) = match &e {
        gitdiverge_lib::Error::GitCommand { repo_path, stderr } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(format!("repo={} stderr={}", repo_path.display(), stderr)),
        ),
        gitdiverge_lib::Error::Io(io) => (StatusCode::INTERNAL_SERVER_ERROR, Some(io.to_string())),
        gitdiverge_lib::Error::Utf8(utf8) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Some(utf8.to_string()))
        }
        gitdiverge_lib::Error::TimestampParse(msg) => (StatusCode::BAD_REQUEST, Some(msg.clone())),
        gitdiverge_lib::Error::Parse(msg) => (StatusCode::BAD_REQUEST, Some(msg.clone())),
        gitdiverge_lib::Error::ReferenceNotFound(msg) => {
            (StatusCode::BAD_REQUEST, Some(msg.clone()))
        }
    };
    (
        status,
        Json(ApiError {
            error: e.to_string(),
            details,
        }),
    )
}

fn map_join_error(e: tokio::task::JoinError) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error: "task panicked".to_string(),
            details: Some(e.to_string()),
        }),
    )
}

/// Build an SSE stream from a crossbeam progress channel and a Tokio join handle.
///
/// Progress events are forwarded as `event: progress`. When the handle resolves,
/// the result is emitted as either `event: complete` with the serialized payload
/// or `event: error` with the error details.
fn build_sse_stream<T: Serialize + Send + 'static>(
    progress_rx: crossbeam_channel::Receiver<ProgressEvent>,
    handle: tokio::task::JoinHandle<Result<T, gitdiverge_lib::Error>>,
) -> impl IntoResponse {
    let (sse_tx, sse_rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);

    let start = std::time::Instant::now();

    tokio::spawn(async move {
        // Send a large padding comment to force browser/proxy buffers to flush
        // immediately, so the client sees SSE events in real-time.
        let padding = Event::default().comment(" ".repeat(4096));
        let _ = sse_tx.send(Ok(padding)).await;

        let mut ping_interval = tokio::time::interval(Duration::from_secs(1));
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = ping_interval.tick() => {
                    // Periodic comment ping keeps TCP buffers flushed and
                    // prevents browser/proxy idle timeouts.
                    let ping = Event::default().comment("ping");
                    if sse_tx.send(Ok(ping)).await.is_err() {
                        break;
                    }
                }
                event = async {
                    // Poll the crossbeam channel without blocking the tokio worker thread.
                    loop {
                        match progress_rx.try_recv() {
                            Ok(event) => return Some(event),
                            Err(crossbeam_channel::TryRecvError::Empty) => {
                                tokio::time::sleep(Duration::from_millis(10)).await;
                            }
                            Err(crossbeam_channel::TryRecvError::Disconnected) => return None,
                        }
                    }
                } => {
                    match event {
                        Some(event) => {
                            let ts = start.elapsed().as_millis() as u64;
                            trace!(?event, ts, "forwarding sse progress event");
                            let payload = match serde_json::to_string(&StreamEvent::<T>::Progress { event, ts }) {
                                Ok(json) => json,
                                Err(_) => r#"{"type":"progress"}"#.to_string(),
                            };
                            let sse_event = Event::default().event("progress").data(payload);
                            if sse_tx.send(Ok(sse_event)).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }

        match handle.await {
            Ok(Ok(response)) => {
                trace!("SSE stream completed successfully");
                let payload =
                    match serde_json::to_string(&StreamEvent::<T>::Complete { data: response }) {
                        Ok(json) => json,
                        Err(_) => r#"{"type":"complete"}"#.to_string(),
                    };
                let _ = sse_tx
                    .send(Ok(Event::default().event("complete").data(payload)))
                    .await;
            }
            Ok(Err(e)) => {
                error!(error = %e, "SSE stream failed with error");
                let (message, details) = match &e {
                    gitdiverge_lib::Error::GitCommand { repo_path, stderr } => (
                        e.to_string(),
                        Some(format!("repo={} stderr={}", repo_path.display(), stderr)),
                    ),
                    gitdiverge_lib::Error::Io(io) => (e.to_string(), Some(io.to_string())),
                    gitdiverge_lib::Error::Utf8(utf8) => (e.to_string(), Some(utf8.to_string())),
                    gitdiverge_lib::Error::TimestampParse(msg) => {
                        (e.to_string(), Some(msg.clone()))
                    }
                    gitdiverge_lib::Error::Parse(msg) => (e.to_string(), Some(msg.clone())),
                    gitdiverge_lib::Error::ReferenceNotFound(msg) => {
                        (e.to_string(), Some(msg.clone()))
                    }
                };
                let payload =
                    match serde_json::to_string(&StreamEvent::<T>::Error { message, details }) {
                        Ok(json) => json,
                        Err(_) => r#"{"type":"error"}"#.to_string(),
                    };
                let _ = sse_tx
                    .send(Ok(Event::default().event("error").data(payload)))
                    .await;
            }
            Err(join_err) => {
                error!(error = %join_err, "SSE stream task panicked");
                let payload = match serde_json::to_string(&StreamEvent::<T>::Error {
                    message: "task panicked".to_string(),
                    details: Some(join_err.to_string()),
                }) {
                    Ok(json) => json,
                    Err(_) => r#"{"type":"error"}"#.to_string(),
                };
                let _ = sse_tx
                    .send(Ok(Event::default().event("error").data(payload)))
                    .await;
            }
        }
    });

    let stream = ReceiverStream::new(sse_rx);
    let sse = Sse::new(stream).keep_alive(KeepAlive::default());
    let headers = [
        (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        (
            header::HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        ),
    ];
    (StatusCode::OK, headers, sse)
}

// ---------------------------------------------------------------------------
// Auth middleware
// ---------------------------------------------------------------------------

/// Axum middleware that validates the `Authorization: Bearer <token>` header.
///
/// When auth is disabled the request passes through unchanged.
/// Otherwise the token is decoded and validated against the current auth
/// configuration (JWKS or local key).
pub async fn require_auth(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next<Body>,
) -> Result<Response, StatusCode> {
    if !state.auth.config.enabled {
        debug!("auth disabled, allowing request");
        return Ok(next.run(request).await);
    }

    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            warn!("missing or malformed Authorization header");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    match state.auth.validate_token(token).await {
        Ok(claims) => {
            debug!(sub = %claims.sub, "request authenticated");
            Ok(next.run(request).await)
        }
        Err(e) => {
            warn!(error = ?e, "token validation failed");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Health check.
///
/// Returns a simple `ok` status and the daemon version. Use this endpoint to
/// verify that the service is running and reachable before issuing other requests.
#[utoipa::path(
    get,
    path = "/health",
    operation_id = "health",
    security(),
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse),
    )
)]
pub async fn health() -> ApiResult<HealthResponse> {
    Ok(Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}

/// Generate a test JWT signed with the embedded local RSA key.
///
/// This endpoint is only available when the service is configured in `Local`
/// auth mode. The returned token expires on **2035-01-01** and can be used to
/// authorise requests via the Swagger UI *Authorize* button or via the
/// `Authorization: Bearer <token>` header.
#[utoipa::path(
    get,
    path = "/auth/test-token",
    operation_id = "test_token",
    security(),
    responses(
        (status = 200, description = "Test token generated", body = TestTokenResponse),
        (status = 403, description = "Not available in current auth mode", body = ApiError),
    )
)]
pub async fn test_token(State(state): State<AppState>) -> ApiResult<TestTokenResponse> {
    match generate_test_token(&state.auth) {
        Ok(resp) => Ok(Json(resp)),
        Err(e) => Err((
            StatusCode::FORBIDDEN,
            Json(ApiError {
                error: e.to_string(),
                details: Some(
                    "test-token endpoint is only available in local auth mode".to_string(),
                ),
            }),
        )),
    }
}

/// List indexed repositories.
///
/// Reads the local repository index (`repos.json`) and returns
/// every tracked repository with its GUID, name, URL, and absolute filesystem path.
#[utoipa::path(
    get,
    path = "/api/v1/repos",
    operation_id = "list_repos",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "List of indexed repositories", body = Vec<RepoIndexEntry>),
        (status = 401, description = "Unauthorized", body = ApiError),
        (status = 500, description = "Index could not be read", body = ApiError),
    )
)]
pub async fn list_repos(State(state): State<AppState>) -> ApiResult<Vec<RepoIndexEntry>> {
    let clone_dir = state.clone_dir;
    let index = match RepoIndex::open(&clone_dir) {
        Ok(i) => i,
        Err(e) => return Err(map_err(e)),
    };

    let entries: Vec<RepoIndexEntry> = index
        .entries()
        .map(|e| RepoIndexEntry {
            guid: e.guid.clone(),
            name: e.name.clone(),
            url: e.url.clone(),
            repo_path: index.entry_path(e).to_string_lossy().to_string(),
        })
        .collect();

    Ok(Json(entries))
}

/// List branches for an existing repository.
///
/// Looks up the repository by `repo_guid` and returns all remote-tracking
/// branch names (with the `origin/` prefix removed). The gix library provider
/// is used for the enumeration.
#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo_guid}/branches",
    operation_id = "list_branches",
    security(("bearer_auth" = [])),
    params(
        ("repo_guid" = String, Path, description = "Repository GUID")
    ),
    responses(
        (status = 200, description = "List of branch names", body = BranchesResponse),
        (status = 400, description = "Repository not found or invalid", body = ApiError),
        (status = 401, description = "Unauthorized", body = ApiError),
        (status = 500, description = "Git operation failed", body = ApiError),
    )
)]
pub async fn list_branches(
    State(state): State<AppState>,
    Path(repo_guid): Path<String>,
) -> ApiResult<BranchesResponse> {
    let clone_dir = state.clone_dir;

    let repo_path = {
        let index = match RepoIndex::open(&clone_dir) {
            Ok(i) => i,
            Err(e) => return Err(map_err(e)),
        };
        let entry = match index.resolve_by_guid(&repo_guid) {
            Some(e) => e,
            None => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        error: format!("repository with guid {} not found in index", repo_guid),
                        details: None,
                    }),
                ));
            }
        };
        index.entry_path(entry)
    };

    if !repo_path.join(".git").is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: format!(
                    "repository {} does not exist at {}",
                    repo_guid,
                    repo_path.display()
                ),
                details: None,
            }),
        ));
    }

    let branches = tokio::task::spawn_blocking(move || {
        let gix = GixProvider::new();
        gix.branches(&repo_path, &gitdiverge_lib::progress::NoProgress)
    })
    .await
    .map_err(map_join_error)?
    .map_err(map_err)?;

    Ok(Json(BranchesResponse { branches }))
}

/// Clone a remote repository.
///
/// Clones `url` into a GUID-derived directory. The repository is registered in
/// the local index so that subsequent calls can refer to it by GUID. If the
/// repository already exists locally, it is not re-cloned and the response
/// status is `already_exists`.
///
/// This endpoint returns a Server-Sent Events stream. Progress events are
/// emitted while git operations run in a blocking thread pool. The stream ends
/// with either a `complete` event containing the [`CloneResponse`] or an
/// `error` event.
#[utoipa::path(
    post,
    path = "/api/v1/repos/clone",
    operation_id = "clone_repo",
    security(("bearer_auth" = [])),
    request_body = CloneRequest,
    responses(
        (
            status = 200,
            description = "SSE stream of progress events followed by the final result or an error",
            content(("text/event-stream" = String))
        ),
        (status = 401, description = "Unauthorized"),
    )
)]
#[instrument(skip(state, req))]
pub async fn clone_repo(
    State(state): State<AppState>,
    Json(req): Json<CloneRequest>,
) -> impl IntoResponse {
    let clone_dir = state.clone_dir;
    let url = req.url.clone();
    let force = req.force;
    let git = state.git.clone();
    let repo_locks = state.repo_locks.clone();
    let divergence_cache = state.divergence_cache.clone();

    info!(%url, force, "clone request received");

    let (progress_tx, progress_rx) = crossbeam_channel::unbounded::<ProgressEvent>();
    let progress = Arc::new(SseProgress::new(progress_tx));

    let handle = tokio::task::spawn_blocking(move || {
        // Serialize all clone operations for the same URL.
        let url_lock = repo_locks.acquire(url.clone());
        let _url_guard = url_lock.lock().unwrap();

        let repo_name = repo_name_from_url(&url);
        let mut index = RepoIndex::open(&clone_dir)?;
        let (guid, name) = {
            let entry = index.get_or_insert(url.clone(), repo_name.clone());
            (entry.guid.clone(), entry.name.clone())
        };
        let path = index.clone_dir().join(&guid).join(&name);
        let already_exists = path.join(".git").is_dir();

        // Also lock by GUID so fetch/divergence requests for the same repo wait.
        let guid_lock = repo_locks.acquire(guid.clone());
        let _guid_guard = guid_lock.lock().unwrap();

        let (status, message) = if already_exists && force {
            std::fs::remove_dir_all(&path).map_err(gitdiverge_lib::Error::Io)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(gitdiverge_lib::Error::Io)?;
            }
            git.clone_repo(&url, &path, &*progress)?;
            (
                "recloned".to_string(),
                "repository removed and re-cloned successfully".to_string(),
            )
        } else if !already_exists {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(gitdiverge_lib::Error::Io)?;
            }
            git.clone_repo(&url, &path, &*progress)?;
            (
                "cloned".to_string(),
                "repository cloned successfully".to_string(),
            )
        } else {
            (
                "already_exists".to_string(),
                "repository already cloned".to_string(),
            )
        };

        index.save()?;
        divergence_cache.invalidate_repo(&guid);

        Ok(CloneResponse {
            repo_name,
            repo_guid: guid.clone(),
            repo_path: path.to_string_lossy().to_string(),
            status,
            message,
        })
    });

    build_sse_stream(progress_rx, handle)
}

/// Fetch updates for an existing repository.
///
/// Looks up the repository by `repo_guid` in the local index, runs `git fetch`,
/// validates every branch in the request body against `origin/<branch>`, and
/// checks out the valid branches locally.
///
/// This endpoint returns a Server-Sent Events stream. Progress events are
/// emitted while git operations run in a blocking thread pool. The stream ends
/// with either a `complete` event containing the [`FetchResponse`] or an
/// `error` event.
#[utoipa::path(
    post,
    path = "/api/v1/repos/{repo_guid}/fetch",
    operation_id = "fetch_repo",
    security(("bearer_auth" = [])),
    params(
        ("repo_guid" = String, Path, description = "Repository GUID")
    ),
    request_body = FetchRequest,
    responses(
        (
            status = 200,
            description = "SSE stream of progress events followed by the final result or an error",
            content(("text/event-stream" = String))
        ),
        (status = 401, description = "Unauthorized"),
    )
)]
#[instrument(skip(state, req))]
pub async fn fetch_repo(
    State(state): State<AppState>,
    Path(repo_guid): Path<String>,
    Json(req): Json<FetchRequest>,
) -> impl IntoResponse {
    let clone_dir = state.clone_dir;
    let git = state.git.clone();
    let branches = req.branches;
    let repo_locks = state.repo_locks.clone();
    let divergence_cache = state.divergence_cache.clone();

    info!(%repo_guid, branches = ?branches, "fetch request received");

    let (progress_tx, progress_rx) = crossbeam_channel::unbounded::<ProgressEvent>();
    let progress = Arc::new(SseProgress::new(progress_tx));

    let handle = tokio::task::spawn_blocking(move || {
        let lock = repo_locks.acquire(repo_guid.clone());
        let _guard = lock.lock().unwrap();

        let index = RepoIndex::open(&clone_dir)?;
        let entry = index.resolve_by_guid(&repo_guid).ok_or_else(|| {
            gitdiverge_lib::Error::Parse(format!(
                "repository with guid {repo_guid} not found in index"
            ))
        })?;
        let repo_path = index.entry_path(entry);
        let repo_name = entry.name.clone();

        if !repo_path.join(".git").is_dir() {
            return Err(gitdiverge_lib::Error::Parse(format!(
                "repository {} does not exist at {}",
                repo_guid,
                repo_path.display()
            )));
        }

        git.fetch(&repo_path, &*progress)?;
        divergence_cache.invalidate_repo(&repo_guid);

        let mut branch_statuses = Vec::new();
        let mut valid_branches = Vec::new();
        for branch in &branches {
            match git.branch_exists_on_remote(&repo_path, branch) {
                Ok(true) => {
                    valid_branches.push(branch.clone());
                    branch_statuses.push(BranchStatus {
                        branch: branch.clone(),
                        exists: true,
                    });
                }
                Ok(false) => {
                    branch_statuses.push(BranchStatus {
                        branch: branch.clone(),
                        exists: false,
                    });
                }
                Err(e) => {
                    branch_statuses.push(BranchStatus {
                        branch: branch.clone(),
                        exists: false,
                    });
                    return Err(e);
                }
            }
        }

        if valid_branches.is_empty() {
            return Ok(FetchResponse {
                repo_name,
                repo_guid,
                repo_path: repo_path.to_string_lossy().to_string(),
                status: "fetched".to_string(),
                branch_statuses,
                message: "no valid branches to checkout".to_string(),
            });
        }

        for branch in &valid_branches {
            git.checkout_branch(&repo_path, branch, &*progress)?;
        }

        Ok(FetchResponse {
            repo_name,
            repo_guid,
            repo_path: repo_path.to_string_lossy().to_string(),
            status: "fetched".to_string(),
            branch_statuses,
            message: format!("checked out {} branch(es)", valid_branches.len()),
        })
    });

    build_sse_stream(progress_rx, handle)
}

/// Analyse branch divergence for a single repository.
///
/// Looks up the repository by `repo_guid`, validates the requested branches
/// against local remote-tracking refs, then computes pairwise commit divergence
/// using the `gix` library provider.
///
/// This endpoint is **read-only** — it does not fetch from the remote or
/// mutate the working directory. Call `POST /api/v1/repos/{repo_guid}/fetch`
/// first if you need fresh remote data.
///
/// This endpoint returns a Server-Sent Events stream. Progress events are
/// emitted while git operations run in a blocking thread pool. The stream ends
/// with either a `complete` event containing the [`DivergenceResponse`] or an
/// `error` event.
#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo_guid}/divergence",
    operation_id = "repo_divergence",
    security(("bearer_auth" = [])),
    params(
        ("repo_guid" = String, Path, description = "Repository GUID"),
        DivergenceQuery,
    ),
    responses(
        (
            status = 200,
            description = "SSE stream of progress events followed by the final result or an error",
            content(("text/event-stream" = String))
        ),
        (status = 401, description = "Unauthorized"),
    )
)]
#[instrument(skip(state, params))]
pub async fn repo_divergence(
    State(state): State<AppState>,
    Path(repo_guid): Path<String>,
    Query(params): Query<DivergenceQuery>,
) -> impl IntoResponse {
    let clone_dir = state.clone_dir;
    let git = state.git.clone();
    let branches: Vec<String> = params
        .branches
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let repo_locks = state.repo_locks.clone();
    let divergence_cache = state.divergence_cache.clone();

    info!(%repo_guid, branches = ?branches, "divergence request received");

    let (progress_tx, progress_rx) = crossbeam_channel::unbounded::<ProgressEvent>();
    let progress = Arc::new(SseProgress::new(progress_tx));

    let handle = tokio::task::spawn_blocking(move || {
        let lock = repo_locks.acquire(repo_guid.clone());
        let _guard = lock.lock().unwrap();

        let repo_path = {
            let index = RepoIndex::open(&clone_dir)?;
            let entry = index.resolve_by_guid(&repo_guid).ok_or_else(|| {
                gitdiverge_lib::Error::Parse(format!(
                    "repository with guid {repo_guid} not found in index"
                ))
            })?;
            index.entry_path(entry)
        };

        if !repo_path.join(".git").is_dir() {
            return Err(gitdiverge_lib::Error::Parse(format!(
                "repository {} does not exist at {}",
                repo_guid,
                repo_path.display()
            )));
        }

        let mut branch_statuses = Vec::new();
        let mut valid_branches = Vec::new();
        let t1 = std::time::Instant::now();
        for branch in &branches {
            match git.branch_exists_on_remote(&repo_path, branch) {
                Ok(true) => {
                    valid_branches.push(branch.clone());
                    branch_statuses.push(BranchStatus {
                        branch: branch.clone(),
                        exists: true,
                    });
                }
                Ok(false) => {
                    branch_statuses.push(BranchStatus {
                        branch: branch.clone(),
                        exists: false,
                    });
                }
                Err(e) => {
                    branch_statuses.push(BranchStatus {
                        branch: branch.clone(),
                        exists: false,
                    });
                    return Err(e);
                }
            }
        }
        debug!(
            elapsed_ms = t1.elapsed().as_millis(),
            branches = branches.len(),
            "branch validation completed"
        );

        if valid_branches.is_empty() {
            return Err(gitdiverge_lib::Error::Parse(
                "none of the requested branches exist on the remote".to_string(),
            ));
        }

        // Check cache first
        if let Some(cached) = divergence_cache.get_with_branches(&repo_guid, &valid_branches) {
            progress.finish();
            let repo_name = {
                let index = RepoIndex::open(&clone_dir)?;
                index
                    .resolve_by_guid(&repo_guid)
                    .map(|e| e.name.clone())
                    .unwrap_or_else(|| repo_guid.clone())
            };
            return Ok(DivergenceResponse {
                repo_name,
                repo_guid,
                repo_path: cached.analytics.repo_path.clone(),
                branch_statuses,
                analytics: cached.analytics.into(),
            });
        }

        // Divergence analysis reads commit history directly from the object
        // database via GixProvider, so we do not need to check out local
        // branches. We simply reference the remote tracking refs.
        let remote_branches: Vec<String> = valid_branches
            .iter()
            .map(|b| format!("origin/{}", b))
            .collect();

        let gix = GixProvider::new();
        let t3 = std::time::Instant::now();
        let mut analytics =
            analyze_branch_divergence(&gix, &repo_path, &remote_branches, &*progress)?;
        debug!(
            elapsed_ms = t3.elapsed().as_millis(),
            "analyze_branch_divergence completed"
        );

        // Strip the origin/ prefix so the UI sees clean branch names.
        for branch in &mut analytics.branches {
            if let Some(stripped) = branch.strip_prefix("origin/") {
                *branch = stripped.to_string();
            }
        }
        for cmp in &mut analytics.comparisons {
            if let Some(stripped) = cmp.source_branch.strip_prefix("origin/") {
                cmp.source_branch = stripped.to_string();
            }
            if let Some(stripped) = cmp.target_branch.strip_prefix("origin/") {
                cmp.target_branch = stripped.to_string();
            }
        }

        // Store full analytics in cache keyed by both the valid branches and the
        // original requested branch list. This ensures that callers who query
        // commits using the same original branch list (which may include
        // non-existent branches) can still find the cached result.
        divergence_cache.insert_with_branches(
            repo_guid.clone(),
            &valid_branches,
            analytics.clone(),
        );
        divergence_cache.insert_with_branches(repo_guid.clone(), &branches, analytics.clone());

        let repo_name = {
            let index = RepoIndex::open(&clone_dir)?;
            index
                .resolve_by_guid(&repo_guid)
                .map(|e| e.name.clone())
                .unwrap_or_else(|| repo_guid.clone())
        };

        Ok(DivergenceResponse {
            repo_name,
            repo_guid,
            repo_path: repo_path.to_string_lossy().to_string(),
            branch_statuses,
            analytics: analytics.into(),
        })
    });

    build_sse_stream(progress_rx, handle)
}

/// Run batch branch-divergence analysis across multiple repositories.
///
/// Executes the full three-phase pipeline (clone → fetch/checkout → analytics)
/// in parallel for every repository in the request body.
///
/// This endpoint returns a Server-Sent Events stream. Progress events are
/// emitted while git operations run in a blocking thread pool. The stream ends
/// with either a `complete` event containing the [`Vec<BatchDivergenceResponseItem>`]
/// or an `error` event.
#[utoipa::path(
    post,
    path = "/api/v1/batch/divergence",
    operation_id = "batch_divergence",
    security(("bearer_auth" = [])),
    request_body = BatchDivergenceRequest,
    responses(
        (
            status = 200,
            description = "SSE stream of progress events followed by the final result or an error",
            content(("text/event-stream" = String))
        ),
        (status = 401, description = "Unauthorized"),
    )
)]
#[instrument(skip(state, req))]
pub async fn batch_divergence(
    State(state): State<AppState>,
    Json(req): Json<BatchDivergenceRequest>,
) -> impl IntoResponse {
    let clone_dir = state.clone_dir;
    let git = state.git.clone();
    let repos: Vec<RepoSpec> = req
        .repos
        .into_iter()
        .map(|r| RepoSpec {
            url: r.url.clone(),
            name: repo_name_from_url(&r.url),
        })
        .collect();
    let branches = req.branches;
    let repo_locks = state.repo_locks.clone();
    let divergence_cache = state.divergence_cache.clone();

    info!(repo_count = repos.len(), branches = ?branches, "batch divergence request received");

    let (progress_tx, progress_rx) = crossbeam_channel::unbounded::<ProgressEvent>();
    let progress = Arc::new(SseProgress::new(progress_tx));

    let handle = tokio::task::spawn_blocking(move || {
        let mut index = RepoIndex::open(&clone_dir)?;

        // Pre-resolve URLs to GUIDs so we know which locks to acquire.
        let mut lock_keys: Vec<String> = repos
            .iter()
            .map(|spec| {
                let entry = index.get_or_insert(spec.url.clone(), spec.name.clone());
                entry.guid.clone()
            })
            .collect();
        lock_keys.sort();
        lock_keys.dedup();

        // Acquire all repo locks in sorted order to prevent deadlocks.
        let locks: Vec<_> = lock_keys
            .iter()
            .map(|key| repo_locks.acquire(key.clone()))
            .collect();
        let _guards: Vec<_> = locks.iter().map(|l| l.lock().unwrap()).collect();

        let clone_results =
            gitdiverge_lib::run_clone_phase(&git, &repos, &mut index, 4, &*progress);
        let fetch_results =
            gitdiverge_lib::run_fetch_phase(&git, &clone_results, &branches, &*progress);

        // Analytics phase with cache awareness
        let ready: Vec<&gitdiverge_lib::FetchPhaseResult> = fetch_results
            .iter()
            .filter(|r| !r.valid_branches.is_empty())
            .collect();

        let mut analytics_results = Vec::new();
        if !ready.is_empty() {
            progress.start(
                Some(ready.len() as u64 * 2),
                "phase 3/3: analyzing branch divergence",
            );

            let gix = GixProvider::new();
            for fetch_result in ready {
                let guid = fetch_result.repo_guid.clone();
                progress.advance(1);
                let analytics = if let Some(cached) =
                    divergence_cache.get_with_branches(&guid, &fetch_result.valid_branches)
                {
                    Ok(cached.analytics)
                } else {
                    let res = analyze_branch_divergence(
                        &gix,
                        &fetch_result.repo_path,
                        &fetch_result.valid_branches,
                        &gitdiverge_lib::NoProgress,
                    );
                    match res {
                        Ok(a) => {
                            divergence_cache.insert_with_branches(
                                guid.clone(),
                                &fetch_result.valid_branches,
                                a.clone(),
                            );
                            divergence_cache.insert_with_branches(
                                guid.clone(),
                                &branches,
                                a.clone(),
                            );
                            Ok(a)
                        }
                        Err(e) => Err(e.to_string()),
                    }
                };

                progress.message(&fetch_result.repo_name);
                progress.advance(1);
                analytics_results.push(gitdiverge_lib::RepoAnalytics {
                    repo_name: fetch_result.repo_name.clone(),
                    repo_url: fetch_result.repo_url.clone(),
                    repo_guid: guid,
                    repo_path: fetch_result.repo_path.clone(),
                    branch_statuses: fetch_result.branch_statuses.clone(),
                    analytics,
                });
            }

            progress.finish();
        }

        index.save()?;

        let response: Vec<BatchDivergenceResponseItem> = analytics_results
            .into_iter()
            .map(|r| {
                let (analytics_summary, error) = match r.analytics {
                    Ok(ref a) => (Some(BranchAnalyticsSummary::from(a)), None),
                    Err(ref e) => (None, Some(e.clone())),
                };
                BatchDivergenceResponseItem {
                    repo_name: r.repo_name,
                    repo_guid: r.repo_guid,
                    repo_url: r.repo_url,
                    repo_path: r.repo_path.to_string_lossy().to_string(),
                    branch_statuses: r.branch_statuses,
                    analytics: analytics_summary,
                    error,
                }
            })
            .collect();
        Ok(response)
    });

    build_sse_stream(progress_rx, handle)
}

/// Retrieve paginated missing commits for a specific branch pair.
///
/// Requires that divergence analysis has already been run for this repository
/// (via `GET /api/v1/repos/{repo_guid}/divergence`) so the results are cached.
#[utoipa::path(
    get,
    path = "/api/v1/repos/{repo_guid}/divergence/commits",
    operation_id = "get_divergence_commits",
    security(("bearer_auth" = [])),
    params(
        ("repo_guid" = String, Path, description = "Repository GUID"),
        PagedCommitsRequest,
    ),
    responses(
        (status = 200, description = "Paged commits for a branch pair", body = PagedCommitsResponse),
        (status = 404, description = "No cached divergence found for this repo", body = ApiError),
        (status = 400, description = "Invalid branch pair or page parameters", body = ApiError),
    ),
)]
#[instrument(skip(state, params))]
pub async fn get_divergence_commits(
    State(state): State<AppState>,
    Path(repo_guid): Path<String>,
    Query(params): Query<PagedCommitsRequest>,
) -> ApiResult<PagedCommitsResponse> {
    let branches: Vec<String> = params
        .branches
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let cached = state
        .divergence_cache
        .get_with_branches(&repo_guid, &branches)
        .ok_or_else(|| {
            (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error:
                    "Divergence not computed for this repository. Run divergence analysis first."
                        .to_string(),
                details: None,
            }),
        )
        })?;

    let comparison = cached
        .analytics
        .comparisons
        .iter()
        .find(|c| {
            c.source_branch == params.source_branch && c.target_branch == params.target_branch
        })
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "Invalid branch pair.".to_string(),
                    details: None,
                }),
            )
        })?;

    let total = comparison.missing_commits.len();
    let start = params.page * params.page_size;
    let end = (start + params.page_size).min(total);

    let commits = if start >= total {
        Vec::new()
    } else {
        comparison.missing_commits[start..end].to_vec()
    };

    Ok(Json(PagedCommitsResponse {
        source_branch: params.source_branch,
        target_branch: params.target_branch,
        page: params.page,
        page_size: params.page_size,
        total_commits: total,
        commits,
    }))
}

// ---------------------------------------------------------------------------
// Server entry point
// ---------------------------------------------------------------------------

fn guess_mime_type(path: &str) -> &'static str {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| match ext {
            "html" => "text/html",
            "js" => "application/javascript",
            "css" => "text/css",
            "svg" => "image/svg+xml",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "json" => "application/json",
            _ => "application/octet-stream",
        })
        .unwrap_or("application/octet-stream")
}

/// Serve embedded static files from the `webclient` folder.
///
/// If the requested path matches an embedded file exactly, it is returned with
/// an appropriate `Content-Type`.  Requests for `config.js` are intercepted
/// and a dynamically-generated response is returned using values from the
/// loaded `ServiceConfig`.  Otherwise `index.html` is served so the React SPA
/// can handle client-side routing (standard nginx `try_files` behaviour).
/// Path-traversal attempts are rejected with `404`.
pub async fn static_handler(State(state): State<AppState>, uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // Reject path traversal.
    if path.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }

    // config.js is always intercepted and generated from runtime configuration.
    if path == "config.js" {
        let cfg = &state.webclient;
        let use_auth = state.auth.config.enabled;
        let body = format!(
            r#"window.__GITDIVERGE_CONFIG__ = {{
  API_BASE: {:?},
  OIDC_AUTHORITY: {:?},
  OIDC_CLIENT_ID: {:?},
  USE_AUTH: {},
  JIRA_SERVER_ADDR: {:?},
}}
"#,
            cfg.api_base,
            cfg.oidc_authority.as_deref().unwrap_or(""),
            cfg.oidc_client_id,
            use_auth,
            cfg.jira_server_addr,
        );
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/javascript")],
            body,
        )
            .into_response();
    }

    let (contents, mime_path) = if path.is_empty() {
        (webclient::get_index_html(), "index.html")
    } else {
        (webclient::get_file(path), path)
    };

    match contents {
        Some(bytes) => {
            let mime = guess_mime_type(mime_path);
            (StatusCode::OK, [(header::CONTENT_TYPE, mime)], bytes).into_response()
        }
        None => {
            // SPA fallback – let the React router display its own 404 page.
            match webclient::get_index_html() {
                Some(bytes) => {
                    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html")], bytes).into_response()
                }
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}

/// Extract the real client IP from a request, respecting reverse-proxy headers.
///
/// Priority:
/// 1. `X-Forwarded-For` (first entry)
/// 2. `X-Real-IP`
/// 3. `ConnectInfo` socket address
/// 4. `"unknown"`
///
/// Note: this trusts the forwarded headers unconditionally; deploy the service
/// behind a trusted reverse proxy.
fn client_ip_from_request<B>(request: &Request<B>) -> String {
    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            request
                .headers()
                .get("x-real-ip")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.trim().to_string())
        })
        .or_else(|| {
            request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0.ip().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Build the axum router for the daemon (useful for testing).
pub fn build_router(state: AppState) -> Router {
    let swagger: Router<AppState> = SwaggerUi::new("/swagger-ui")
        .url("/api-docs/openapi.json", ApiDoc::openapi())
        .into();

    // Public routes — no authentication required.
    let public = Router::new()
        .route("/health", get(health))
        .route("/auth/test-token", get(test_token));

    // Protected routes — require a valid Bearer JWT.
    let protected = Router::new()
        .route("/api/v1/repos", get(list_repos))
        .route("/api/v1/repos/:repo_guid/branches", get(list_branches))
        .route("/api/v1/repos/clone", post(clone_repo))
        .route("/api/v1/repos/:repo_guid/fetch", post(fetch_repo))
        .route("/api/v1/repos/:repo_guid/divergence", get(repo_divergence))
        .route(
            "/api/v1/repos/:repo_guid/divergence/commits",
            get(get_divergence_commits),
        )
        .route("/api/v1/batch/divergence", post(batch_divergence))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .merge(public)
        .merge(protected)
        .merge(swagger)
        .fallback(static_handler)
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
                let client_ip = client_ip_from_request(request);
                tracing::span!(
                    tracing::Level::INFO,
                    "request",
                    method = %request.method(),
                    uri = %request.uri(),
                    version = ?request.version(),
                    client_ip = %client_ip,
                )
            }),
        )
        .with_state(state)
}

/// Start the HTTP daemon.
pub async fn run(
    bind: &str,
    port: u16,
    clone_dir: PathBuf,
    config: crate::config::ServiceConfig,
) -> anyhow::Result<()> {
    let bind_addr: IpAddr = bind.parse().context("invalid bind address")?;
    let addr = SocketAddr::from((bind_addr, port));

    // Fast-fail if something is already accepting connections on this port.
    // On Windows bind() to a specific interface can succeed even when 0.0.0.0
    // is already bound, so we actively probe with connect() first.
    if std::net::TcpStream::connect(addr).is_ok() {
        bail!("port {} on {} is already in use", port, bind_addr);
    }

    let listener = std::net::TcpListener::bind(addr)
        .with_context(|| format!("failed to bind to port {} on {}", port, bind_addr))?;

    let auth = AuthState::new(config.auth)?;
    let auth_enabled = auth.config.enabled;
    let auth_mode = match &auth.config.mode {
        crate::config::AuthMode::Local { .. } => "local",
        crate::config::AuthMode::Jwks { .. } => "jwks",
    };

    let git_credentials: Vec<GitCredential> = config
        .git_credentials
        .into_iter()
        .map(|c| {
            let host = c.host.clone();
            GitCredential::from_file(c.host, &c.token_file).with_context(|| {
                format!(
                    "failed to load git credential for host '{}' from {}",
                    host,
                    c.token_file.display()
                )
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let git = if git_credentials.is_empty() {
        ProcessGitProvider::new()
    } else {
        ProcessGitProvider::with_credentials(git_credentials)
            .context("failed to initialise git credential helper")?
    };

    let state = AppState {
        clone_dir: clone_dir.clone(),
        git,
        repo_locks: Arc::new(RepoLockManager::new()),
        auth: Arc::new(auth),
        webclient: config.webclient,
        divergence_cache: DivergenceCache::new(),
    };

    let app = build_router(state);
    info!(%addr, clone_dir = %clone_dir.display(), auth_enabled, auth_mode, "daemon starting");

    let server = axum::Server::from_tcp(listener)
        .context("failed to create HTTP server from TCP listener")?
        .serve(app.into_make_service_with_connect_info::<SocketAddr>());
    info!("daemon listening on http://{}", addr);

    notify_systemd_ready();

    let result = server.await;

    notify_systemd_stopping();

    result?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn notify_systemd_ready() {
    let _ = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]);
    info!("sent systemd READY=1 notification");
}

#[cfg(not(target_os = "linux"))]
fn notify_systemd_ready() {
    debug!("systemd notify skipped (non-Linux platform)");
}

#[cfg(target_os = "linux")]
fn notify_systemd_stopping() {
    let _ = sd_notify::notify(true, &[sd_notify::NotifyState::Stopping]);
    info!("sent systemd STOPPING=1 notification");
}

#[cfg(not(target_os = "linux"))]
fn notify_systemd_stopping() {
    debug!("systemd stopping notify skipped (non-Linux platform)");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use utoipa::OpenApi;

    #[test]
    fn repo_lock_manager_new_and_acquire() {
        let manager = RepoLockManager::new();
        let lock1 = manager.acquire("repo-a".to_string());
        let lock2 = manager.acquire("repo-a".to_string());
        let lock3 = manager.acquire("repo-b".to_string());

        // Same key should return the same lock instance (Arc cloned)
        assert!(Arc::ptr_eq(&lock1, &lock2));
        // Different key should be a different lock
        assert!(!Arc::ptr_eq(&lock1, &lock3));
    }

    #[test]
    fn repo_lock_manager_default() {
        let manager = RepoLockManager::default();
        let lock = manager.acquire("key".to_string());
        assert!(lock.try_lock().is_ok());
    }

    #[test]
    fn repo_name_from_url_variants() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/repo.git"),
            "repo"
        );
        assert_eq!(repo_name_from_url("https://github.com/user/repo"), "repo");
        assert_eq!(repo_name_from_url(""), "repo");
        assert_eq!(repo_name_from_url("git@github.com:user/repo.git"), "repo");
    }

    #[test]
    fn map_err_variants() {
        let cases = vec![
            (
                gitdiverge_lib::Error::GitCommand {
                    repo_path: std::path::PathBuf::from("/r"),
                    stderr: "err".to_string(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                gitdiverge_lib::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "io")),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                gitdiverge_lib::Error::Utf8(String::from_utf8(vec![0x80]).unwrap_err()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                gitdiverge_lib::Error::TimestampParse("bad".to_string()),
                StatusCode::BAD_REQUEST,
            ),
            (
                gitdiverge_lib::Error::Parse("bad".to_string()),
                StatusCode::BAD_REQUEST,
            ),
            (
                gitdiverge_lib::Error::ReferenceNotFound("ref".to_string()),
                StatusCode::BAD_REQUEST,
            ),
        ];

        for (err, expected_status) in cases {
            let (status, json) = map_err(err);
            assert_eq!(status, expected_status);
            // Ensure details is populated for every variant
            assert!(json.details.is_some(), "expected details for error variant");
        }
    }

    #[test]
    fn map_join_error_returns_internal_server_error() {
        // We can't easily create a real JoinError without a panicked task,
        // but we can at least verify the function compiles and returns the right status.
        // Creating a JoinError from a panicked task:
        let rt = tokio::runtime::Runtime::new().unwrap();
        let handle = rt.spawn(async { panic!("test panic") });
        let join_err = rt.block_on(handle).unwrap_err();
        let (status, _) = map_join_error(join_err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn openapi_spec_contains_expected_paths() {
        let spec = ApiDoc::openapi();
        let paths = spec.paths.paths;
        assert!(paths.contains_key("/health"));
        assert!(paths.contains_key("/auth/test-token"));
        assert!(paths.contains_key("/api/v1/repos"));
        assert!(paths.contains_key("/api/v1/repos/{repo_guid}/branches"));
        assert!(paths.contains_key("/api/v1/repos/clone"));
        assert!(paths.contains_key("/api/v1/repos/{repo_guid}/fetch"));
        assert!(paths.contains_key("/api/v1/repos/{repo_guid}/divergence"));
        assert!(paths.contains_key("/api/v1/repos/{repo_guid}/divergence/commits"));
        assert!(paths.contains_key("/api/v1/batch/divergence"));
    }

    #[test]
    fn openapi_spec_contains_expected_schemas() {
        let spec = ApiDoc::openapi();
        let schemas = spec.components.expect("components should exist").schemas;
        assert!(schemas.contains_key("CloneRequest"));
        assert!(schemas.contains_key("CloneResponse"));
        assert!(schemas.contains_key("FetchRequest"));
        assert!(schemas.contains_key("FetchResponse"));
        assert!(schemas.contains_key("DivergenceRequest"));
        assert!(schemas.contains_key("DivergenceQuery"));
        assert!(schemas.contains_key("DivergenceResponse"));
        assert!(schemas.contains_key("BatchDivergenceRequest"));
        assert!(schemas.contains_key("HealthResponse"));
        assert!(schemas.contains_key("BranchAnalytics"));
        assert!(schemas.contains_key("BatchDivergenceResponseItem"));
        assert!(schemas.contains_key("RepoIndexEntry"));
        assert!(schemas.contains_key("BranchesResponse"));
        assert!(schemas.contains_key("TestTokenResponse"));
        assert!(schemas.contains_key("ProgressEvent"));
        assert!(schemas.contains_key("PagedCommitsRequest"));
        assert!(schemas.contains_key("PagedCommitsResponse"));
        assert!(schemas.contains_key("BranchComparisonSummary"));
        assert!(schemas.contains_key("BranchAnalyticsSummary"));
    }

    #[test]
    fn guess_mime_type_variants() {
        assert_eq!(guess_mime_type("index.html"), "text/html");
        assert_eq!(guess_mime_type("app.js"), "application/javascript");
        assert_eq!(guess_mime_type("style.css"), "text/css");
        assert_eq!(guess_mime_type("icon.svg"), "image/svg+xml");
        assert_eq!(guess_mime_type("font.woff"), "font/woff");
        assert_eq!(guess_mime_type("font.woff2"), "font/woff2");
        assert_eq!(guess_mime_type("data.json"), "application/json");
        assert_eq!(guess_mime_type("unknown.bin"), "application/octet-stream");
        assert_eq!(guess_mime_type("no-extension"), "application/octet-stream");
    }

    #[test]
    fn api_error_roundtrip() {
        let original = ApiError {
            error: "something went wrong".to_string(),
            details: Some("extra context".to_string()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.error, "something went wrong");
        assert_eq!(deserialized.details, Some("extra context".to_string()));

        let none_details = ApiError {
            error: "plain".to_string(),
            details: None,
        };
        let json2 = serde_json::to_string(&none_details).unwrap();
        let deserialized2: ApiError = serde_json::from_str(&json2).unwrap();
        assert_eq!(deserialized2.details, None);
    }

    #[test]
    fn stream_event_serializes_correctly() {
        let progress = StreamEvent::<serde_json::Value>::Progress {
            event: ProgressEvent::Advance {
                current: 1,
                total: Some(10),
                message: "cloning".to_string(),
            },
            ts: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&progress).unwrap();
        assert!(json.contains("\"type\":\"progress\""));
        assert!(json.contains("cloning"));
        assert!(json.contains("\"ts\":"));

        let complete = StreamEvent::<serde_json::Value>::Complete {
            data: serde_json::json!({"ok": true}),
        };
        let json = serde_json::to_string(&complete).unwrap();
        assert!(json.contains("\"type\":\"complete\""));

        let error = StreamEvent::<serde_json::Value>::Error {
            message: "fail".to_string(),
            details: Some("trace".to_string()),
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"type\":\"error\""));
    }

    #[test]
    fn batch_divergence_response_item_roundtrip() {
        use gitdiverge_lib::BranchStatus;
        let original = BatchDivergenceResponseItem {
            repo_name: "my-repo".to_string(),
            repo_guid: "guid-1".to_string(),
            repo_url: "https://example.com/repo.git".to_string(),
            repo_path: "/repos/guid-1/repo".to_string(),
            branch_statuses: vec![BranchStatus {
                branch: "main".to_string(),
                exists: true,
            }],
            analytics: Some(BranchAnalyticsSummary {
                repo_path: "/repos/guid-1/repo".to_string(),
                branches: vec!["main".to_string(), "dev".to_string()],
                comparisons: vec![BranchComparisonSummary {
                    source_branch: "dev".to_string(),
                    target_branch: "main".to_string(),
                    missing_commit_count: 0,
                }],
            }),
            error: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: BatchDivergenceResponseItem = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.repo_name, "my-repo");
        assert_eq!(deserialized.repo_guid, "guid-1");
        assert!(deserialized.analytics.is_some());
        assert_eq!(deserialized.analytics.as_ref().unwrap().branches.len(), 2);
    }
}
