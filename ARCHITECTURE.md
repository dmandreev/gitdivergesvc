# GitDiverge Architecture

## Overview

GitDiverge is a Rust workspace that provides git repository analysis capabilities.
It is designed to evolve from a CLI tool into a web service that manipulates
GitLab repositories — cloning, comparing branches, indexing code, and providing
quick answers around repository data.

The architecture is built around **five pillars**:
1. **Structured concurrency** — `crossbeam-channel` task queues + `rayon` data parallelism.
2. **Structured observability** — `tracing` for logs, spans, and metrics.
3. **Pluggable git backend** — trait-based `GitProvider` abstraction.
4. **Progress reporting** — trait-based `ProgressReporter` so UIs (CLI bars, web sockets, silent mode) can plug in without touching business logic.
5. **Stateless authentication** — JWT validation using cached JWKS or a local test key, with zero external calls during request handling.

## Workspace Structure

```
gitdiverge/
├── Cargo.toml           # Workspace manifest
├── ARCHITECTURE.md      # This document
├── AGENTS.md            # Agent-specific guidelines
├── gitdiverge.toml.example # Example daemon configuration
├── gitdiverge-lib/         # Core library
│   ├── src/
│   │   ├── lib.rs       # Public exports
│   │   ├── git.rs       # GitProvider trait & data types
│   │   ├── git/
│   │   │   ├── process.rs  # ProcessGitProvider (git executable)
│   │   │   └── gix_provider.rs  # GixProvider (pure-Rust git)
│   │   ├── runtime.rs   # Task-runtime (channel workers)
│   │   ├── batch.rs     # Rayon parallel batch helpers
│   │   ├── analytics.rs # Branch divergence analytics
│   │   ├── repo_batch.rs# Three-phase batch pipeline (clone/fetch/analytics)
│   │   ├── repo_index.rs# JSON-backed repo registry (URL → GUID)
│   │   ├── progress.rs  # ProgressReporter trait
│   │   └── error.rs     # Structured error types
│   └── tests/
│       ├── process_git_integration.rs
│       └── branch_analytics_integration.rs
├── gitdiverge/             # Application entry point
│   └── src/
│       ├── main.rs      # CLI parsing & mode dispatch
│       ├── lib.rs       # Public exports (binary crate)
│       ├── daemon.rs    # Axum HTTP server, routing, SSE streams
│       ├── auth.rs      # JWT validation, JWKS cache, test tokens
│       ├── config.rs    # TOML configuration loading
│       ├── divergence_cache.rs # In-memory cache for divergence results
│       ├── demo.rs      # Demo repository generator
│       └── logging.rs   # tracing subscriber + journald support
└── webclientsrc/           # React SPA (see Web Client below)
    ├── src/
    ├── public/
    ├── dist/            # Static build output
    └── package.json
```

## Web Client (`webclientsrc`)

The frontend is a **React 19 + TypeScript + Vite** single-page application. It is treated as a static asset that gets compiled and embedded directly into the Rust binary so the daemon can serve it without an external web server.

### Build & embedding flow

1. **Development** — `npm run dev` inside `webclientsrc/` starts the Vite dev server with HMR. The backend API is assumed to be running separately (e.g. `cargo run -- daemon`).
2. **Production build** — `npm run build` produces hashed bundles in `webclientsrc/dist/`.
3. **Compile-time embedding** — `gitdiverge/build.rs` probes `../webclientsrc/dist`. If the directory exists and is non-empty, the crate sets `cfg(webclient_present)` and `daemon.rs` uses `include_dir!("$CARGO_MANIFEST_DIR/../webclientsrc/dist")` to embed the entire `dist/` tree as a static blob inside the release binary.
4. **Runtime serving** — `static_handler` in `daemon.rs` serves files from the embedded directory. Requests for `/config.js` are intercepted and generated dynamically from the loaded `ServiceConfig` so that API base URL, OIDC authority, and auth enabled/disabled state can be changed at runtime without recompiling the SPA.

### Key files

| File | Purpose |
|------|---------|
| `webclientsrc/src/main.tsx` | Entry point (StrictMode + BrowserRouter) |
| `webclientsrc/src/config.ts` | Reads `window.__GITDIVERGE_CONFIG__` at runtime |
| `webclientsrc/scripts/vite-runtime-config.ts` | Vite plugin that emits `config.js` from `.env` at build time |
| `gitdiverge/build.rs` | Sets `cfg(webclient_present)` when `dist/` is present |
| `gitdiverge/src/daemon.rs` | `static_handler` serves embedded files and intercepts `/config.js` |

### Standalone frontend work

`webclientsrc/` contains its own `AGENTS.md` with frontend-specific build commands, code-style rules, and testing guidelines. You can open the folder directly in an editor and work on the UI independently of the Rust workspace.

## Concurrency Model

### Task Runtime (`gitdiverge-lib/src/runtime.rs`)

A fixed pool of `std::thread` workers pull tasks from a `crossbeam-channel`
queue. This pattern is ideal for I/O-bound work (spawning git processes) and
gives us explicit back-pressure and graceful shutdown.

- **Enqueue** → `RuntimeHandle::submit(Task::Log { … })`
- **Worker loop** → `while let Ok(task) = rx.recv()`
- **Shutdown** → send `Task::Shutdown` to every worker, then `join()`

The runtime owns an `Arc<dyn GitProvider>` so all workers share the same
backend instance. Each task also carries its own `Arc<dyn ProgressReporter>`
so progress can be reported back from worker threads safely.

### Rayon Data Parallelism (`gitdiverge-lib/src/batch.rs`)

For embarrassingly-parallel work (e.g. querying logs for 10 branches at once),
`rayon::prelude::*` turns a sequential iterator into a parallel one:

```rust
branches
    .par_iter()
    .map(|b| git.log(repo, b, opts.clone(), &NoProgress))
    .collect()
```

Rayon operates *inside* a task (or directly in the main thread) rather than
replacing the channel workers. This composes well: the runtime distributes
coarse tasks, and rayon parallelises fine-grained sub-work.

## Progress Reporting (`gitdiverge-lib/src/progress.rs`)

All long-running operations accept a `&dyn ProgressReporter` so the caller
decides how progress is surfaced.

```rust
pub trait ProgressReporter: Send + Sync {
    fn start(&self, total: Option<u64>, message: &str);
    fn advance(&self, amount: u64);
    fn finish(&self);
}
```

**Built-in implementations:**
- `NoProgress` — no-op; used in tests and headless mode.
- `CallbackProgress<F>` — wraps a closure for ad-hoc composition.
- `IndicatifProgress` (binary) — bridges to `indicatif::ProgressBar` / `MultiProgress`.
- `SseProgress` (web service) — streams `ProgressEvent`s over SSE.

**Usage patterns:**
- **Single branch** → one progress bar, advanced per parsed commit.
- **Multi-branch** → `MultiProgress` with an overall bar advanced once per branch completed.
- **Web service** → SSE stream of progress events via `SseProgress`.
- **Batch pipeline** (`repo_batch.rs`) → Each of the three phases (clone,
  fetch, analytics) reports **2 progress units per repository** (one when work
  starts, one when it finishes). The `total` passed to `start()` is
  `ready.len() * 2`. The React `BulkProgressPanel` divides both `current` and
  `total` by 2 for display so the user sees repository counts rather than raw
  progress units.

## Library Design (`gitdiverge-lib`)

### Core Trait

```rust
pub trait GitProvider: Send + Sync {
    fn log(
        &self,
        repo_path: &Path,
        branch: &str,
        options: LogOptions,
        progress: &dyn ProgressReporter,
    ) -> Result<Vec<Commit>, Error>;

    fn branches(
        &self,
        repo_path: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<Vec<String>, Error>;
}
```

`ProcessGitProvider` also implements non-trait helper methods for repo management:
- `clone_repo(url, dest, progress) -> Result<(), Error>`
- `get_remote_url(repo_path) -> Result<String, Error>`
- `fetch(repo_path, progress) -> Result<(), Error>`
- `branch_exists_on_remote(repo_path, branch) -> Result<bool, Error>`
- `checkout_branch(repo_path, branch, progress) -> Result<(), Error>`

### Data Model

- `Commit` — Full commit metadata (hash, author, timestamp, subject, body, parents)
- `Author` — Name and email
- `LogOptions` — Pagination and filtering for log queries
- `BranchAnalytics` / `BranchComparison` — Pairwise divergence metrics
- `RepoSpec` / `RepoEntry` / `RepoIndex` — Batch registry and indexing types

All data structs derive `Serialize`/`Deserialize` for JSON interchange.

### Implementations

- `ProcessGitProvider` — Invokes `git` via `std::process::Command`.
  Every method is instrumented with `#[instrument]` so spans automatically
  capture repo path, branch, and worker ID.
- `GixProvider` — Pure-Rust implementation using the `gix` crate.
  Provides the same `GitProvider` trait without shelling out.

## Web Service (`gitdiverge/src/daemon.rs`)

The daemon is an Axum HTTP server that exposes a REST API and streams
progress via Server-Sent Events (SSE).

### Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/health` | Public | Health check |
| `GET` | `/auth/test-token` | Public | Generate a test JWT (local mode only) |
| `GET` | `/api/v1/repos` | Bearer | List indexed repositories |
| `GET` | `/api/v1/repos/{repo_guid}/branches` | Bearer | List remote-tracking branches |
| `POST` | `/api/v1/repos/clone` | Bearer | Clone a repository (SSE) |
| `POST` | `/api/v1/repos/{repo_guid}/fetch` | Bearer | Fetch and checkout branches (SSE) |
| `GET`  | `/api/v1/repos/{repo_guid}/divergence` | Bearer | Branch divergence analysis (SSE) |
| `GET`  | `/api/v1/repos/{repo_guid}/divergence/commits` | Bearer | Paginated missing commits for a branch pair |
| `POST` | `/api/v1/batch/divergence` | Bearer | Batch divergence across repos (SSE) |
| `GET` | `/swagger-ui` | Public | Swagger UI (OpenAPI explorer) |
| `GET` | `/api-docs/openapi.json` | Public | OpenAPI JSON spec |

### Authentication (`gitdiverge/src/auth.rs`)

Every protected endpoint requires an `Authorization: Bearer <token>` header.
The service validates tokens **locally** using pure cryptography — no calls to
Keycloak (or any other IdP) happen during request handling.

**Two validation modes:**

1. **JWKS mode** — The service downloads the JSON Web Key Set once from a
   configurable URL (e.g. Keycloak's `openid-connect/certs` endpoint) and
   caches it for **5 minutes**. Tokens are validated against the cached public
   keys. On cache expiry the JWKS is re-fetched automatically.

2. **Local mode** — An embedded RSA key pair is used for validation. This mode
   is intended for development and integration testing without a live Keycloak
   instance. A `GET /auth/test-token` endpoint generates a signed JWT that
   expires in **2035**, which can be pasted directly into Swagger UI's
   *Authorize* dialog.

Both modes verify `iss` (issuer), `aud` (audience), and `exp` (expiration).

### Divergence Cache (`gitdiverge/src/divergence_cache.rs`)

The daemon keeps an **in-memory cache** of computed `BranchAnalytics` results so
that repeated requests for the same repository and branch set are served
instantly.

- **Key design** — `(repo_guid, sorted branches)`.  Branch lists are normalised
  so that `["dev", "main"]` and `["main", "dev"]` map to the same entry.
- **Memory bound** — The cache tracks the approximate heap size of every entry.
  When total estimated memory exceeds **2 GB**, the oldest entries (by
  `computed_at`) are evicted until the limit is respected.
- **Invalidation** — Entries for a repository are dropped automatically after
  `clone` or `fetch` operations so that stale data is never served.
- **Thread safety** — `Arc<RwLock<CacheState>>` allows concurrent reads;
  eviction happens during write locks.

### Configuration (`gitdiverge/src/config.rs`)

Auth behaviour is controlled via CLI flags or an optional TOML file:

- `--auth` — enables JWT validation
- `--config gitdiverge.toml` — loads settings from a TOML file
- Default — auth disabled

See `gitdiverge.toml.example` for a full configuration reference.

## Observability (`tracing`)

- Use `tracing::info!`, `debug!`, `warn!`, `error!` instead of `println!`.
- The binary initialises `tracing_subscriber::fmt::init()` which respects
  `RUST_LOG` (e.g. `RUST_LOG=info`, `RUST_LOG=gitdiverge_lib=debug`).
- On Windows, `enable-ansi-support` is loaded so colours work in modern
  terminals.

## Binary Lifecycle (`gitdiverge/src/main.rs`)

1. **Single-instance guard** — `single_instance::SingleInstance` prevents
   multiple processes from running simultaneously.
2. **Tracing init** — structured subscriber with optional `tracing-journald`
   on Linux; ANSI support on Windows. Controlled via `-v` / `-q` flags.
3. **Ctrl-C handler** — `ctrlc::set_handler` signals a `crossbeam-channel`
   so the main thread can shut down the runtime gracefully.
4. **Mode selection**:
   - *Single branch* → submit to the task runtime with a per-branch progress bar.
   - *Multiple branches* → use `batch::log_parallel` (rayon) with a `MultiProgress` overall bar.
   - *Daemon* → start the Axum HTTP server with the configured auth backend.
   - *OpenAPI* → emit the OpenAPI JSON spec to stdout and exit.
   - *Config-example* → write the example TOML to stdout and exit.
5. **Systemd integration** (Linux) — `sd-notify` sends `READY=1` when the
   daemon is listening and `STOPPING=1` during graceful shutdown.
6. **Shutdown** → `Runtime::shutdown()` joins all workers cleanly.

## Error Handling

- **Library** — `thiserror` defines typed errors (`GitCommand`, `Io`, `Parse`,
  `ReferenceNotFound`, etc.).
- **Binary** — `anyhow` wraps library errors with context and backtraces.
- All error paths are logged via `tracing::error!` before being returned.

## Platform Support

- **Windows & Linux** — `ProcessGitProvider` uses `std::process::Command`
  directly; no shell wrappers.
- **Git 2.30.2+** required in `PATH`.
- **Rust 1.95+** 

## Future Roadmap

1. **Diff Parsing** — Structured diff output; each file processed as a rayon
   parallel item with progress ticks.
2. **Code Indexing** — Index repository contents for fast querying.
3. **Webhooks / Async Events** — Push results to consumers instead of relying
   solely on SSE polling.
