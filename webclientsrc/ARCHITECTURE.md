# GitDiverge Web Client Architecture

## Overview

The GitDiverge web client is a single-page React application that provides a visual interface for branch divergence analytics. It is designed to be served as static assets by the Rust daemon, but can also be developed independently against a running backend API.

## Technology Stack

| Layer | Technology | Purpose |
|---|---|---|
| Framework | React 19 + TypeScript | UI rendering |
| Router | react-router-dom 7 | Client-side routing |
| Build tool | Vite 8 | Bundling and dev server |
| CSS | Tailwind CSS 4 | Utility-first styling |
| Auth | oidc-client-ts | OpenID Connect flow |
| API client | @hey-api/client-fetch | Typed fetch-based SDK |
| Virtualisation | @tanstack/react-virtual | Large list rendering |
| Test | Vitest + happy-dom | Unit testing |

## Project Structure

```
src/
├── main.tsx                 # Entry point (StrictMode + BrowserRouter)
├── App.tsx                  # Route definitions and layout shell
├── index.css                # Tailwind v4 theme and design tokens
├── config.ts                # Runtime config reader
├── api/
│   └── client.ts            # API client configuration + SSE consumer
├── auth/
│   ├── oidcConfig.ts        # UserManager settings
│   └── AuthProvider.tsx     # React Context for auth state
├── components/
│   ├── Header.tsx           # Navigation bar with sign-in / sign-out
│   ├── RepoList.tsx         # Indexed repos with filtering & selection
│   ├── CloneRepo.tsx        # Clone form
│   ├── AnalysisPanel.tsx    # Branch input + divergence orchestration
│   ├── BranchInput.tsx      # Autocomplete branch selector
│   ├── DivergenceMatrix.tsx # N×N commit-gap matrix
│   ├── ProgressBar.tsx      # SSE progress indicator
│   ├── CommitDetailPanel.tsx# Slide-over commit list
│   └── VirtualList.tsx      # ResizeObserver-based virtual list
├── generated/               # Auto-generated OpenAPI client
├── hooks/
│   ├── useDivergenceStream.ts  # SSE async generator hook
│   └── useFetchStream.ts       # Fetch SSE async generator hook
├── pages/
│   └── Callback.tsx         # OIDC callback handler
└── test/
    └── setup.ts             # Vitest setup
```

## State Management

The application uses **React built-in state** exclusively — no external state library is required.

- **Authentication state** — managed by `AuthProvider` (React Context). Holds the current `User` object from `oidc-client-ts`, provides `login()`, `logout()`, and `getAccessToken()` to the component tree.
- **Local component state** — each major feature owns its own state:
  - `RepoList` manages the repository list, loading state, and selection.
  - `AnalysisPanel` manages the branch list, analysis state, and result data.
  - `DivergenceMatrix` manages hover states and cell selection.
- **Streaming state** — `useDivergenceStream` and `useFetchStream` encapsulate SSE connection lifecycle (`idle | connecting | streaming | complete | error`) and expose `start()` / `cancel()` functions.

## Data Flow

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Generated SDK  │────▶│  api/client.ts  │────▶│  Component /    │
│  (types + fetch)│     │  (base URL,     │     │  Hook           │
│                 │     │   auth header)  │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                                                        │
                                                        ▼
                                               ┌─────────────────┐
                                               │  useDivergence  │
                                               │  Stream.ts      │
                                               │  (SSE parser)   │
                                               └─────────────────┘
```

1. **Static API calls** (`listRepos`, `listBranches`, `cloneRepo`, etc.) go through the generated `@hey-api/client-fetch` SDK. `api/client.ts` injects `CONFIG.API_BASE` and the bearer token before each request.
2. **Streaming API calls** (`repoDivergence`, `fetchRepo`) bypass the generated SDK's standard response handler. Instead, `api/client.ts` provides a custom `divergenceStream()` function that reads the raw `event-stream` response and yields discriminated-union payloads (`{ type: 'progress' } | { type: 'complete' } | { type: 'error' }`).
3. **Hooks** consume the async generator and map it to React state.
4. **Components** render the state and trigger the next `start()` call on user interaction.

## Server-Sent Events (SSE) Architecture

Mutating endpoints (`clone`, `fetch`, `divergence`, `batch/divergence`) return `text/event-stream` instead of JSON. The client handles this as follows:

1. The generated SDK exposes a `stream` property on SSE-capable operations.
2. `api/client.ts` implements an async generator that iterates over `stream`, parsing each event into a typed payload.
3. `useDivergenceStream.ts` wraps the generator in a hook that maintains React-friendly state:
   - `idle` — before `start()` is called.
   - `connecting` — request sent, awaiting first event.
   - `streaming` — progress events arriving.
   - `complete` — final payload received.
   - `error` — stream ended with an error event or network failure.
4. The hook supports cancellation via an `AbortController`, ensuring the fetch is aborted when the component unmounts or the user cancels.

## Authentication Flow

The app uses **OpenID Connect** (Authorization Code flow with PKCE) via `oidc-client-ts`.

```
┌──────────┐   signinRedirect()   ┌──────────┐   redirect   ┌──────────┐
│  Header  │─────────────────────▶│  IdP     │─────────────▶│ Callback │
│ (Sign In)│                      │(Keycloak)│              │  (page)  │
└──────────┘                      └──────────┘              └────┬─────┘
                                                                 │
                                                                 │ signinRedirectCallback()
                                                                 ▼
                                                          ┌──────────┐
                                                          │ AuthProvider
                                                          │ (stores User)
                                                          └──────────┘
```

1. `AuthProvider` creates a `UserManager` on mount and attempts silent session restoration.
2. Unauthenticated users see a **Sign in** button that triggers `signinRedirect()`.
3. The IdP redirects back to `/auth/callback`, which calls `signinRedirectCallback()` and navigates home.
4. All authenticated API requests include `Authorization: Bearer <access_token>`.
5. **Silent renew** is enabled. When the access token nears expiry, `oidc-client-ts` opens a hidden iframe to `/silent-renew.html` (Vite entry point → `src/silent-renew.ts`). If the SSO session is still valid, the token refreshes without a full redirect.
6. When auth is disabled (`USE_AUTH = false`), the OIDC flow is skipped entirely: sign-in buttons are hidden, the callback page is a no-op, and API requests are sent without bearer tokens.

## Build & Embedding

The client is treated as a static asset that gets embedded into the Rust binary:

1. **Development** — `npm run dev` starts the Vite dev server. The backend API is assumed to be running separately.
2. **Production build** — `npm run build` produces hashed bundles in `dist/`.
3. **Runtime config** — A Vite plugin (`scripts/vite-runtime-config.ts`) reads `.env` and emits `dist/config.js`. The app reads `window.__GITDIVERGE_CONFIG__` at runtime via `src/config.ts`.
4. **Compile-time embedding** — `gitdiverge/build.rs` probes `../webclientsrc/dist`. If present and non-empty, it sets `cfg(webclient_present)`, and `daemon.rs` uses `include_dir!` to embed the `dist/` tree into the binary.
5. **Runtime serving** — `static_handler` in `daemon.rs` serves embedded files. Requests for `/config.js` are intercepted and generated dynamically from `ServiceConfig` so that API base URL, OIDC authority, and auth enabled state can be changed at runtime without recompiling.

## Component Hierarchy

```
App
├── Header (auth state, sign-in/out)
└── Routes
    ├── / (landing page)
    │   └── RepoList
    │       └── CloneRepo
    └── /repo/:guid
        ├── AnalysisPanel
        │   └── BranchInput
        └── DivergenceMatrix
            └── CommitDetailPanel
```

- **`RepoList`** — Displays indexed repositories. Selecting a repo navigates to `/repo/:guid`.
- **`AnalysisPanel`** — Accepts a comma-separated branch list, triggers divergence analysis via `useDivergenceStream`, and displays progress.
- **`DivergenceMatrix`** — Renders the N×N matrix of commit gaps. Clicking a cell opens `CommitDetailPanel` with paginated missing commits.
- **`CommitDetailPanel`** — Slide-over panel that fetches paginated commits via `GET /api/v1/repos/{repo_guid}/divergence/commits`.

## Key Design Decisions

1. **No global state library** — The app is small enough that React Context + local state is sufficient. This avoids the bundle size and complexity of Redux/Zustand.
2. **Generated API client** — `@hey-api/openapi-ts` keeps the TypeScript types in sync with the Rust backend automatically. The generated code lives in `src/generated/` and is never edited by hand.
3. **SSE over WebSockets** — The backend uses SSE because the communication is strictly server-to-client (progress + final result). SSE is simpler than WebSockets for this unidirectional flow and works well over HTTP/1.1.
4. **Virtual list for commits** — `@tanstack/react-virtual` is used in `CommitDetailPanel` so that repositories with thousands of missing commits remain performant.
5. **Tailwind v4 with CSS config** — Design tokens (colours, fonts) are declared in `src/index.css` via the `@theme` block, eliminating the need for a separate `tailwind.config.js`.
