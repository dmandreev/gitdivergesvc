# GitDiverge Client — Agent Guide

This document describes the GitDiverge web client so that AI coding agents can work on it effectively without prior knowledge of the project.

---

## Project Overview

GitDiverge Client is a single-page React application that provides a visual interface for branch divergence analytics. Users authenticate via OpenID Connect, clone or fetch Git repositories, and run pairwise branch divergence analysis. Results are streamed from the backend as Server-Sent Events (SSE) and rendered as an interactive matrix.

- **Repository name:** `gitdiverge-client`
- **Type:** Browser SPA (Single Page Application)
- **Language:** TypeScript
- **Current stage:** Active development

## Standalone Development

This folder can be opened **independently** in an editor (e.g. VSCode) without the parent Rust workspace. The backend API is treated as an external service that is assumed to be running separately.

- Open `webclientsrc/` as the workspace root.
- Copy `.env.example` to `.env` and adjust values for your local backend.
- Run `npm install` followed by `npm run dev`.
- All build, test, and lint commands are self-contained inside this directory.

---

## Technology Stack

| Layer | Technology | Version |
|---|---|---|
| Framework | React | ^19.2.6 |
| Router | react-router-dom | ^7.15.1 |
| Build tool | Vite | ^8.0.12 |
| CSS framework | Tailwind CSS | ^4.3.0 |
| Auth library | oidc-client-ts | ^3.5.0 |
| API client generator | @hey-api/openapi-ts | ^0.95.0 |
| API client runtime | @hey-api/client-fetch | ^0.13.1 |
| Virtualisation | @tanstack/react-virtual | ^3.13.24 |
| Test runner | Vitest | ^4.1.6 |
| DOM environment | happy-dom | ^20.9.0 |
| Linting | ESLint 10 + typescript-eslint 8 | — |

Tailwind CSS v4 is used with the `@tailwindcss/vite` plugin. Custom design tokens (colours, fonts) are declared in `src/index.css` via the `@theme` block instead of a traditional `tailwind.config.js`.

Fonts are loaded via `@fontsource/inter` (body) and `@fontsource/jetbrains-mono` (monospace).

---

## Project Structure

```
src/
├── main.tsx                 # Application entry point (StrictMode + BrowserRouter)
├── App.tsx                  # Route definitions and layout shell
├── index.css                # Tailwind v4 theme, base styles, custom colour palette
├── config.ts                # Runtime config reader (window.__GITDIVERGE_CONFIG__)
├── api/
│   └── client.ts            # API client wrapper + SSE stream consumer
├── auth/
│   ├── oidcConfig.ts        # UserManager settings for oidc-client-ts
│   └── AuthProvider.tsx     # React Context for auth state and tokens
├── components/
│   ├── Header.tsx           # Top navigation bar with sign-in / sign-out
│   ├── RepoList.tsx         # List of indexed repos with filtering & selection
│   ├── CloneRepo.tsx        # Form to clone a new repository by URL
│   ├── AnalysisPanel.tsx    # Branch input + orchestrates divergence analysis
│   ├── BranchInput.tsx      # Autocomplete branch selector with keyboard navigation
│   ├── DivergenceMatrix.tsx # N×N matrix visualisation of commit gaps
│   ├── ProgressBar.tsx      # Accessible progress bar for SSE progress events
│   ├── BulkProgressPanel.tsx# Bulk-operation progress (three-phase clone/fetch/analytics)
│   ├── CommitDetailPanel.tsx# Slide-over panel showing missing commits between branches
│   ├── VirtualList.tsx      # Lightweight custom virtual list (ResizeObserver-based)
│   └── *.test.tsx           # Co-located component tests
├── generated/               # Auto-generated OpenAPI client (DO NOT EDIT MANUALLY)
│   ├── sdk.gen.ts
│   ├── types.gen.ts
│   ├── client.gen.ts
│   ├── index.ts
│   ├── client/
│   │   ├── client.gen.ts
│   │   ├── types.gen.ts
│   │   ├── utils.gen.ts
│   │   └── index.ts
│   └── core/
│       ├── auth.gen.ts
│       ├── bodySerializer.gen.ts
│       ├── params.gen.ts
│       ├── pathSerializer.gen.ts
│       ├── queryKeySerializer.gen.ts
│       ├── serverSentEvents.gen.ts
│       ├── types.gen.ts
│       └── utils.gen.ts
├── hooks/
│   ├── useDivergenceStream.ts      # Hook that consumes SSE via async generator
│   ├── useDivergenceStream.test.ts # Hook unit tests
│   └── useFetchStream.ts           # Hook that consumes fetch SSE via async generator
├── pages/
│   └── Callback.tsx         # OIDC callback handler (/auth/callback)
├── silent-renew.ts          # Silent-renew callback entry point
└── test/
    └── setup.ts             # Vitest setup (imports @testing-library/jest-dom)
```

### Important conventions

- **Generated code lives in `src/generated/`**. It is produced from `divergeapi.json` by `@hey-api/openapi-ts`. Never edit these files by hand; regenerate them with `generate-api.cmd` (Windows) or `npx @hey-api/openapi-ts -f openapi-ts.config.ts`.
- **Tests are co-located** next to the code they exercise (`Component.tsx` + `Component.test.tsx`).
- **Runtime configuration is loaded from `/config.js`** (generated from `.env` by a Vite plugin directly into `dist/`). This allows changing settings in a deployed build without re-bundling the app.

---

## Build and Test Commands

```bash
# Development server with HMR
npm run dev

# Production build (type-check + bundle)
npm run build

# Preview the production build locally
npm run preview

# Run the linter
npm run lint

# Run all tests once
npm run test

# Run tests with coverage
npm run test:coverage

# Run tests in watch mode (via Vitest CLI)
npx vitest

# Regenerate the OpenAPI client from divergeapi.json
# Windows:
generate-api.cmd
# Cross-platform (requires the Rust binary to generate divergeapi.json first):
cargo run --manifest-path ../Cargo.toml -- openapi -o divergeapi.json
npx @hey-api/openapi-ts -f openapi-ts.config.ts
```

---

## Code Style Guidelines

- **TypeScript strictness:** `tsconfig.app.json` enables `noUnusedLocals`, `noUnusedParameters`, `erasableSyntaxOnly`, and `noFallthroughCasesInSwitch`. The build will fail on unused variables or parameters.
- **Module system:** ESM only (`"type": "module"` in `package.json`).
- **JSX transform:** `react-jsx` (no need to import React for JSX).
- **File extensions:** Use `.tsx` for components and `.ts` for plain modules. Import paths include the `.tsx` extension (e.g., `import App from './App.tsx'`).
- **Tailwind classes:** Prefer semantic custom tokens defined in `src/index.css` (`surface-0`, `text-main`, `accent`, etc.) over raw hex codes or arbitrary values. Keep class lists readable; multi-line formatting is acceptable.
- **Icons:** Use `lucide-react` for all icons.
- **Fonts:** Inter is used for body text and JetBrains Mono for monospace content (`font-mono`).
- **No default exports for components** is not enforced, but current components use named exports (e.g., `export function Header() { ... }`). Follow the existing pattern.

---

## Testing Instructions

- **Runner:** Vitest, configured inside `vite.config.ts`.
- **Environment:** `happy-dom` (faster than jsdom).
- **Globals:** Enabled — `describe`, `it`, `expect`, `vi` are available without importing them.
- **Setup file:** `src/test/setup.ts` imports `@testing-library/jest-dom` for DOM assertions (`toBeInTheDocument`, `toHaveAttribute`, etc.).
- **CSS processing:** Enabled (`css: true`) so Tailwind classes are resolved during tests.

### Current test suites

1. `src/App.test.tsx` — Route-level smoke test; mocks `AuthProvider` to verify the landing page renders for unauthenticated users.
2. `src/components/DivergenceMatrix.test.tsx` — Matrix rendering, colour thresholds, legend.
3. `src/components/ProgressBar.test.tsx` — Null state, start/advance/finish percentages.
   - Note: `ProgressBar` shows raw SSE units. `BulkProgressPanel` shows
     repository counts (it divides by 2 because the backend emits 2 progress
     units per repo: start + finish).
4. `src/components/RepoList.test.tsx` — Repository list loading, selection, and filtering.
5. `src/components/CommitDetailPanel.test.tsx` — Panel open/close, filtering, and commit selection.
6. `src/hooks/useDivergenceStream.test.ts` — State transitions, error handling, cancellation.

### Adding new tests

- Place the test file next to the source file.
- Mock external side effects (API calls, SSE streams) at the module level with `vi.mock`.
- Use `@testing-library/react` for component tests and `@testing-library/user-event` for interactions.

---

## Authentication Flow

The app uses OpenID Connect (Authorization Code flow with PKCE) via `oidc-client-ts`.

1. `AuthProvider` creates a `UserManager` singleton and restores the session on mount.
2. Unauthenticated users see a landing hero with a **Sign in** button that triggers `signinRedirect()`.
3. The IdP redirects back to `/auth/callback`, handled by `Callback.tsx`, which calls `signinRedirectCallback()` and navigates home.
4. Authenticated requests to the backend include `Authorization: Bearer <access_token>`.
5. Tokens are stored in `window.localStorage` via `WebStorageStateStore`.
6. **Silent renew** is enabled (`automaticSilentRenew: true`). When the access token is about to expire, `oidc-client-ts` opens a hidden iframe to `/silent-renew.html` (a minimal Vite entry point that loads `src/silent-renew.ts` and calls `signinSilentCallback()`). If the user still has a valid SSO session, the token is refreshed without a full redirect. If silent renew fails or the token expires, the user state is cleared and the app falls back to the sign-in screen.

Configuration defaults (overridable in `.env`):

- Authority: `http://localhost:8080/realms/myrealm`
- Client ID: `react-spa`
- Scopes: `openid profile email`

---

## API Client Architecture

- **Generated layer:** `@hey-api/openapi-ts` reads `divergeapi.json` and produces typed fetch-based SDK methods (`listRepos`, `cloneRepo`, `fetchRepo`, `testToken`, `health`, `repoDivergence`, `listBranches`, etc.) in `src/generated/`.
- **Wrapper layer:** `src/api/client.ts` configures the generated client with `CONFIG.API_BASE` (read from `dist/config.js`) and re-exports typed functions and types.
- **SSE stream:** The divergence endpoint (`/api/v1/repos/{repo_guid}/divergence`) returns a Server-Sent Event stream. The generated client exposes SSE support via `@hey-api/client-fetch` (see `src/generated/core/serverSentEvents.gen.ts`). `src/api/client.ts` implements a custom `divergenceStream()` function that calls the generated `repoDivergence` method and iterates over its `stream` property, yielding discriminated-union payloads (`progress`, `complete`, `error`).
- **Hook layer:** `useDivergenceStream.ts` consumes the async generator and exposes React-friendly state (`idle | connecting | streaming | complete | error`) plus `start()` and `cancel()` functions.

---

## Environment Configuration

Settings are maintained in `.env` as `VITE_*` variables. A small Vite plugin (`scripts/vite-runtime-config.ts`) reads `.env` and emits `config.js` at the root of the output directory during build. In dev mode the plugin serves `/config.js` directly from memory. The app reads `window.__GITDIVERGE_CONFIG__` via `src/config.ts` at runtime.

Example `.env`:

```bash
# Backend API base URL
VITE_API_BASE=http://localhost:8080

# OIDC / Keycloak settings
VITE_OIDC_AUTHORITY=http://localhost:8080/realms/myrealm
VITE_OIDC_CLIENT_ID=react-spa

# Enable/disable authentication
USE_AUTH=true
```

Setting `USE_AUTH=false` disables the OIDC flow entirely: sign-in buttons are hidden, the callback page becomes a no-op, and API requests are sent without bearer tokens.

Because the final config is emitted as a standalone JS file, you can modify `dist/config.js` in a deployed build without re-bundling the application.

---

## Security Considerations

- **Do not commit `.env` files.** They are ignored by `.gitignore`.
- **Sensitive files are blocked** from automatic reads by tooling (e.g., `.env`).
- **OIDC tokens are stored in `localStorage`.** This is acceptable for a local-development SPA against Keycloak, but evaluate `sessionStorage` or a Service-Worker-based token vault if stronger security is required.
- **No Content Security Policy (CSP)** is currently configured. Consider adding CSP meta tags or headers before production deployment.
- **No CI/CD pipeline** is present yet. Any future pipeline can template `.env` before build, or overwrite `dist/config.js` at deploy time to inject environment-specific values without rebuilding the bundle.

---

## Deployment Notes

- The build output is a static site in `dist/`.
- `dist/index.html` is generated from `index.html` and includes hashed script and stylesheet references.
- The app expects to be served from the domain root (or a path that matches the `BrowserRouter` base). If deploying under a sub-path, configure `vite.config.ts` with `base: '/sub-path/'`.
- The backend API and OIDC provider must be reachable from the browser at runtime (CORS must be configured on the backend).
