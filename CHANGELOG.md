# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-25

### Added

- Initial release of GitDiverge.
- **Core library (`gitdiverge-lib`)**:
  - `GitProvider` trait with pluggable backends: `ProcessGitProvider` (git executable) and `GixProvider` (pure-Rust `gix` crate).
  - Branch divergence analytics: pairwise commit comparisons across branch sets.
  - Structured concurrency via `crossbeam-channel` task runtime for I/O-bound git operations.
  - Data parallelism via `rayon` for embarrassingly-parallel work (e.g. querying logs for multiple branches).
  - `ProgressReporter` trait for decoupled progress reporting across CLI, web, and test consumers.
  - JSON-backed `RepoIndex` mapping repository URLs to stable GUID-derived directories.
  - Three-phase batch pipeline (`repo_batch.rs`): clone → fetch/checkout → analytics.
- **CLI (`gitdiverge` binary)**:
  - `run <URL>` — single-repository branch analytics with progress bars.
  - `run --repositories-file <file> --branches-file <file>` — batch mode across many repositories.
  - `daemon` — start the Axum HTTP server.
  - `openapi` — export the OpenAPI JSON specification.
  - `config-example` — print an example TOML configuration file.
  - `demo` — generate fake repositories for testing.
  - Single-instance enforcement to prevent concurrent process collisions.
  - Coloured progress bars via `indicatif` (`MultiProgress` for batch operations).
- **Web service (`daemon`)**:
  - Axum HTTP server with CORS, structured `tracing` middleware, and graceful shutdown.
  - Server-Sent Events (SSE) streaming for long-running operations (`clone`, `fetch`, `divergence`, `batch/divergence`).
  - In-memory divergence cache with 2 GB memory-bound eviction and automatic invalidation on clone/fetch.
  - Per-repository locking (`RepoLockManager`) to serialize git operations on the same working directory.
  - Swagger UI at `/swagger-ui` with OpenAPI spec generated from `utoipa` annotations.
  - Runtime-generated `/config.js` for the React SPA (API base, OIDC authority, auth state).
- **Web client (`webclientsrc`)**:
  - React 19 + TypeScript + Vite 8 + Tailwind CSS 4 single-page application.
  - OIDC authentication via `oidc-client-ts` (Authorization Code + PKCE, silent renew).
  - Auto-generated API client from OpenAPI spec.
  - SSE consumer hooks mapping stream states to React state.
  - Virtual lists for large commit sets.
  - N×N divergence matrix UI with slide-over commit detail panels.
  - JIRA server integration link support (configurable via `jira_server_addr`).
- **Authentication**:
  - Stateless JWT validation with two modes: **Local** (embedded RSA key pair) and **JWKS** (cached remote key set).
  - `GET /auth/test-token` endpoint for local-mode test token generation.
  - Configurable via CLI (`--auth`) or TOML (`[auth]` section).
- **Observability**:
  - Structured logging with `tracing` and `tracing-subscriber`.
  - Linux systemd integration (`READY=1` / `STOPPING=1` via `sd-notify`).
  - Windows ANSI colour support via `enable-ansi-support`.
- **Testing**:
  - Unit tests co-located with source in `#[cfg(test)]` modules.
  - Integration tests for daemon HTTP API, auth flows, batch analytics, and git backends.
- **Packaging**:
  - Multi-stage `Dockerfile` building the web client, running Rust tests, and producing an optimised binary (`profile = extreme`).
  - Embedded static assets via `include_dir` when `webclientsrc/dist` is present at compile time.

[Unreleased]: https://github.com/dmandreev/gitdivergesvc/compare/v0.1.0...HEAD
[0.1.0-alpha]: https://github.com/dmandreev/gitdivergesvc/releases/tag/v0.1.0
