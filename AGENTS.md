# Agent Guidelines

## Build & Test

- Run all tests: `cargo test --workspace`
- Run linting: `cargo clippy --workspace -- -D warnings`
- Format code: `cargo fmt --all`
- Minimum supported Rust version: 1.95 
- Git **2.30.2+** must be available in PATH for integration tests

## Code Coverage

Two recommended tools are pre-configured for the project:

### Option A: cargo-tarpaulin (Recommended for CI)

```bash
# Install (requires nightly or stable with llvm-tools)
cargo install cargo-tarpaulin

# Generate coverage report (HTML)
cargo tarpaulin --out Html --workspace

# Generate LCOV for IDE integration
cargo tarpaulin --out Lcov --workspace
```

Tarpaulin works by compiling your tests with special instrumentation and
running them. It is the most widely used coverage tool in the Rust ecosystem
and produces reports compatible with Codecov / GitHub Actions.

### Option B: cargo-llvm-cov (More accurate, requires LLVM)

```bash
# Install
cargo install cargo-llvm-cov

# Run tests with coverage instrumentation
cargo llvm-cov --workspace

# Generate HTML report
cargo llvm-cov --workspace --html
# Open: target/llvm-cov/html/index.html

# Generate LCOV report
cargo llvm-cov --workspace --lcov --output-path lcov.info
```

`cargo-llvm-cov` uses LLVM's native coverage instrumentation (the same
infrastructure as Clang / Swift). It is more accurate than tarpaulin for
complex control flow and does not require nightly Rust.

### Coverage targets

- Aim for **>90%** line coverage on `gitdiverge-lib`.
- Unit tests should cover all error branches in parsers.
- Integration tests should cover all `GitProvider` methods against real git
  repositories.
- Do not chase 100% coverage at the expense of meaningful assertions.

## Dependencies

### Approved crates

| Crate | Purpose | Where |
|-------|---------|-------|
| `thiserror` | Library error types | `gitdiverge-lib` |
| `anyhow` | Binary error propagation | `gitdiverge` |
| `tracing` / `tracing-subscriber` | Structured logging / spans | both |
| `rayon` | Data parallelism | `gitdiverge-lib` |
| `crossbeam-channel` | MPSC task queues | both |
| `serde` / `serde_json` | Data serialisation | both |
| `clap` | CLI parsing | `gitdiverge` |
| `ctrlc` | Graceful shutdown signals | `gitdiverge` |
| `single-instance` | Single-process enforcement | `gitdiverge` |
| `indicatif` | CLI progress bars | `gitdiverge` |
| `enable-ansi-support` | Windows terminal colours | `gitdiverge` (windows only) |
| `chrono` | Date/time handling | both |
| `tempfile` | Test temp directories | dev-deps |
| `axum` | HTTP web framework | `gitdiverge` |
| `tokio` | Async runtime | `gitdiverge` |
| `utoipa` / `utoipa-swagger-ui` | OpenAPI spec generation & Swagger UI | `gitdiverge` (binary), `gitdiverge-lib` (schemas) |
| `tower-http` | HTTP middleware (CORS, tracing) | `gitdiverge` |
| `jsonwebtoken` | JWT decoding & validation | `gitdiverge` |
| `reqwest` | HTTP client for JWKS fetching | `gitdiverge` |
| `toml` | Configuration file parsing | `gitdiverge` |
| `gix` | Pure-Rust git implementation | `gitdiverge-lib` |
| `uuid` | GUID generation for `RepoIndex` | `gitdiverge-lib` |
| `fake` | Demo data generation (fake repos, commits) | `gitdiverge` |
| `tracing-journald` | systemd journald logging layer | `gitdiverge` |
| `tokio-stream` | SSE streaming | `gitdiverge` |
| `include_dir` | Embedding webclient static files at compile time | `gitdiverge` |
| `sd-notify` | Linux systemd `READY=1` / `STOPPING=1` notifications | `gitdiverge` |
| `tower` | HTTP middleware/testing utilities | `gitdiverge` (dev-deps) |
| `hyper` | HTTP primitives for tests | `gitdiverge` (dev-deps) |

## Code Style

- Follow standard Rust conventions and `cargo fmt` formatting
- Use `thiserror` for library error types; use `anyhow` for the binary
- Document all public APIs with `///` doc comments
- Keep trait definitions in `src/git.rs`; implementations go in `src/git/*.rs`
- Prefer `Path` and `PathBuf` for all filesystem paths
- Use `std::process::Command` directly; avoid shell wrappers to stay portable

## Logging & Observability

- **Never** use `println!` / `eprintln!` in library code. Use `tracing::info!`,
  `debug!`, `warn!`, `error!` instead.
- In the binary, `tracing_subscriber::fmt::init()` is the single point of log
  initialisation.
- Use `#[instrument]` on `GitProvider` methods so spans automatically capture
  repo path, branch, worker ID, etc.
- Prefer structured fields (`tracing::info!(branch = %b, "msg")`) over string
  interpolation where the data is useful for filtering.

## Concurrency Rules

1. **Task queue** (`crossbeam-channel`) is for coarse-grained, I/O-bound work
   (git commands, network requests).
2. **Rayon** is for fine-grained, CPU-bound or embarrassingly-parallel work
   (parsing, filtering, aggregating).
3. `GitProvider` is `Send + Sync`; all implementations must be thread-safe.
4. Never block a rayon thread on a channel `recv()` — use `std::thread` for
   long-lived workers and rayon inside individual tasks.
5. Always implement graceful shutdown: signal workers, then `join()`.

## Progress Reporting Rules

1. **Library** — Every long-running `GitProvider` method must accept
   `&dyn ProgressReporter` as its final parameter.
2. **No-ops** — Tests and headless consumers pass `&NoProgress`.
3. **Granularity** — Report `start()` before work begins, `advance(1)` after
   each meaningful unit of work (e.g. each parsed commit, each completed
   branch), and `finish()` when done.
   - **Batch operations** — The three-phase batch pipeline (`repo_batch.rs`)
     counts **2 progress units per repository** (`advance(1)` when work starts,
     another when it finishes). This keeps the progress bar smooth but means
     `total` equals `repo_count * 2`. The bulk-page frontend
     (`BulkProgressPanel`) divides the displayed count by 2 so users see
     repository counts, not raw progress units.
4. **Binary** — Use `indicatif` with `MultiProgress` for parallel operations.
   Use `ProgressBar` with a spinner template when the total is unknown.
5. **Web service** — `SseProgress` implements `ProgressReporter` by sending
   [`ProgressEvent`]s over a `crossbeam_channel`. The daemon exposes
   `POST /api/v1/repos/{repo_guid}/divergence` which returns a
   Server-Sent Events stream: `event: progress` while git operations run,
   followed by either `event: complete` with the final JSON payload or
   `event: error` if the operation fails.

## Authentication Rules

1. **Stateless validation** — The daemon never calls Keycloak (or any IdP)
   during request handling. It validates JWTs locally using cached JWKS or a
   local RSA key pair.
2. **JWKS caching** — Remote JWKS is fetched once and cached for **5 minutes**.
   On expiry it is re-fetched automatically on the next request.
3. **Testability** — Auth can be enabled with `--auth` or switched to
   `Local` mode for development without a live Keycloak instance.
4. **Swagger integration** — The OpenAPI spec exposes a `bearer_auth` security
   scheme. Protected endpoints declare it explicitly; public endpoints declare
   `security()` (empty) so Swagger UI does not show a lock icon for them.
5. **Test tokens** — In `Local` mode, `GET /auth/test-token` returns a JWT
   signed with an embedded key pair. The token expires in **2035** and can be
   pasted into Swagger UI's *Authorize* dialog.

## Web Client Configuration

The React SPA receives its runtime settings from `/config.js`.  This request is
**always intercepted** by the daemon; the embedded `webclient/config.js` file is
never served directly.

> **Note:** The `webclientsrc/` directory has its own `AGENTS.md` with
> frontend-specific build commands, testing guidelines, and standalone-development
> instructions. You can open `webclientsrc/` as a separate workspace root.

### Runtime generation

`static_handler` in `daemon.rs` detects requests for `config.js` and generates a
JavaScript payload from the loaded `ServiceConfig`:

- `API_BASE` — comes from `[webclient] api_base` (empty string means same-origin).
- `OIDC_AUTHORITY` — comes from `[webclient] oidc_authority`.
- `OIDC_CLIENT_ID` — comes from `[webclient] oidc_client_id`.
- `USE_AUTH` — **derived from `[auth] enabled`**; it is not a separate web-client
  setting.

### Priority

`--auth` on the command line takes highest priority: it forces
`[auth] enabled = true`, which in turn forces `USE_AUTH = true` in the
generated `config.js`.

### Adding new web-client settings

1. Add the field to `WebClientConfig` in `gitdiverge/src/config.rs`.
2. Emit it inside the `config.js` branch of `static_handler` in `daemon.rs`.
3. Add an example value to `gitdiverge.toml.example` under `[webclient]`.
4. Add a unit test in `config.rs` and an integration test in
   `gitdiverge/tests/daemon_integration.rs`.

## Testing

- **Unit tests**: Co-located with source code inside `#[cfg(test)]` modules.
  Test parsing logic, edge cases, and mock trait implementations.
- **Integration tests**: Live in `gitdiverge-lib/tests/` and `gitdiverge/tests/`.
  Create temporary git repositories using `tempfile::TempDir` and `git init`.
  Do not rely on external network repositories.
- **Auth integration tests**: Test both enabled and disabled auth paths.
  Verify that protected endpoints return `401` without a token, `200` with a
  valid token, and `200` without a token when auth is disabled.
- Ensure tests pass on both Windows and Linux semantics (use `.gitignore` rules
  compatible with both, avoid shell-specific path handling).

## Platform Support

- Target: Windows and Linux
- The `ProcessGitProvider` must work with Git for Windows and standard Linux git
- Do not use Unix-specific paths or shell features in Rust code
- When constructing commands, do not set shell-specific environment variables

## Extending the Library

When adding new git operations:
1. Add the method signature to the `GitProvider` trait with doc comments and
   a `&dyn ProgressReporter` parameter.
2. Implement it in `ProcessGitProvider` with `#[instrument]` and progress
   calls at appropriate granularity.
3. Add a data struct for the result (e.g., `Diff`, `Branch`) in `git.rs` and
   derive `Serialize`/`Deserialize`
4. Add a `Task` variant in `runtime.rs` if the operation should be queueable
5. Add unit tests for parsing logic (pass `&NoProgress`)
6. Add integration tests using a real git repository
7. Update `ARCHITECTURE.md` if the design changes

## Extending the Web Service

When adding new HTTP endpoints:
1. Add the handler function in `daemon.rs` with `#[utoipa::path(...)]` for
   OpenAPI documentation.
2. Derive `utoipa::ToSchema` on any new request/response structs.
3. If the endpoint requires authentication, add `security(("bearer_auth" = []))`
   to the `#[utoipa::path(...)]` attribute.
4. If the endpoint is public, add `security()` (empty) to the attribute so
   Swagger UI does not display a lock icon.
5. Register the route in `build_router()` under the `protected` or `public`
   router as appropriate.
6. Add integration tests in `gitdiverge/tests/daemon_integration.rs`.
7. Update `ARCHITECTURE.md` and `AGENTS.md` if auth behaviour changes.

When adding new web-client configuration values (see *Web Client Configuration*
above), follow the same update pattern for `config.rs`, `daemon.rs`,
`gitdiverge.toml.example`, and the integration tests.
