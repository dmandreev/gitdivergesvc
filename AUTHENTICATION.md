# Authentication

GitDiverge uses **stateless JWT validation** for its REST API and web client. The daemon never calls an external Identity Provider (IdP) during request handling; all validation is performed locally using pure cryptography.

Authentication is **opt-in** and disabled by default. When enabled, protected endpoints require an `Authorization: Bearer <token>` header. Public endpoints (health, Swagger UI, static assets, and the test-token generator) remain accessible without a token.

---

## Table of Contents

- [Enabling Authentication](#enabling-authentication)
- [Validation Modes](#validation-modes)
  - [Local Mode (Development / Testing)](#local-mode-development--testing)
  - [JWKS Mode (Production / Keycloak / OIDC)](#jwks-mode-production--keycloak--oidc)
- [JWT Claims](#jwt-claims)
- [Public vs Protected Endpoints](#public-vs-protected-endpoints)
- [Test Token](#test-token)
- [Swagger UI Integration](#swagger-ui-integration)
- [Configuration Reference](#configuration-reference)

---

## Enabling Authentication

Authentication can be enabled in two ways:

1. **CLI flag** (highest priority):
   ```bash
   gitdiverge daemon --auth
   ```
   This forces authentication on regardless of the configuration file.

2. **Configuration file**:
   ```toml
   [auth]
   enabled = true
   ```

When auth is enabled, the `[auth.mode]` table determines how tokens are validated.

---

## Validation Modes

### Local Mode (Development / Testing)

Local mode uses an **embedded RSA key pair** that ships with the binary. It is intended for local development, integration testing, and demonstrations where a live Keycloak or OIDC server is not available.

```toml
[auth]
enabled = true

[auth.mode]
type = "local"
issuer = "test-issuer"
audience = "account"
```

**Characteristics:**
- Tokens are signed and validated with the embedded key pair.
- The `iss` and `aud` claims are verified against the configured values.
- `GET /auth/test-token` is available and returns a ready-to-use JWT.
- No network requests are made.

### JWKS Mode (Production / Keycloak / OIDC)

JWKS mode downloads the JSON Web Key Set from a remote endpoint (e.g. Keycloak's `openid-connect/certs`) and validates tokens against the cached public keys.

```toml
[auth]
enabled = true

[auth.mode]
type = "jwks"
url = "https://keycloak.example.com/realms/myrealm/protocol/openid-connect/certs"
issuer = "https://keycloak.example.com/realms/myrealm"
audience = "account"
```

**Characteristics:**
- The JWKS is fetched once and cached for **5 minutes**.
- On cache expiry, the key set is re-fetched automatically on the next request.
- Tokens are validated against `iss`, `aud`, and `exp` claims.
- Requires outbound HTTPS access to the JWKS URL.

---

## JWT Claims

The daemon expects the following claims in the JWT payload:

| Claim | Required | Description |
|-------|----------|-------------|
| `sub` | Yes | Subject identifier (user ID). |
| `exp` | Yes | Expiration time (Unix timestamp). |
| `iss` | Yes | Issuer. Must match the configured issuer. |
| `aud` | Yes | Audience. Must match the configured audience. Can be a string or array. |
| `realm_access.roles` | No | Array of role strings (Keycloak convention). Used for future authorisation expansion. |

Example payload:

```json
{
  "sub": "user-123",
  "exp": 2051222400,
  "iss": "test-issuer",
  "aud": "account",
  "realm_access": {
    "roles": ["user"]
  }
}
```

---

## Public vs Protected Endpoints

### Public Endpoints (no token required)

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/health` | Health check and version. |
| `GET` | `/auth/test-token` | Generate a test JWT (local mode only). |
| `GET` | `/swagger-ui` | Swagger UI. |
| `GET` | `/api-docs/openapi.json` | OpenAPI JSON specification. |
| `GET` | `/config.js` | Runtime web-client configuration. |
| `GET` | `/*` | Static assets (SPA fallback). |

### Protected Endpoints (Bearer token required when auth is enabled)

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/v1/repos` | List indexed repositories. |
| `GET` | `/api/v1/repos/{guid}/branches` | List remote-tracking branches. |
| `POST` | `/api/v1/repos/clone` | Clone a repository (SSE). |
| `POST` | `/api/v1/repos/{guid}/fetch` | Fetch and checkout branches (SSE). |
| `GET` | `/api/v1/repos/{guid}/divergence` | Branch divergence analysis (SSE). |
| `GET` | `/api/v1/repos/{guid}/divergence/commits` | Paginated missing commits. |
| `POST` | `/api/v1/batch/divergence` | Batch pipeline (SSE). |

Requests to protected endpoints without a valid token receive **`401 Unauthorized`**.

---

## Test Token

In **Local mode**, the daemon exposes a convenience endpoint that generates a signed JWT for immediate use:

```bash
curl http://localhost:8080/auth/test-token
```

Response:

```json
"eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9..."
```

The returned token:
- Has `sub = "test-user"`
- Expires on **2035-01-01**
- Contains `realm_access.roles = ["user"]`
- Can be pasted directly into Swagger UI's **Authorize** dialog

> **Note:** This endpoint returns `404 Not Found` when the daemon is running in JWKS mode.

---

## Swagger UI Integration

The OpenAPI spec declares a `bearer_auth` security scheme. Protected endpoints include:

```rust
security(("bearer_auth" = []))
```

Public endpoints explicitly declare:

```rust
security()
```

This ensures Swagger UI does **not** display a lock icon on public routes, while requiring a Bearer token for protected routes.

To authorise in Swagger UI:

1. Navigate to `/swagger-ui`.
2. Click **Authorize**.
3. Enter `Bearer <your-jwt>` (including the word `Bearer` and a space) or just paste the raw token if the UI prefixes it automatically.
4. Click **Authorize**, then **Close**.

---

## Configuration Reference

```toml
[auth]
# Master switch for JWT validation.
enabled = true

[auth.mode]
# "local" — embedded key pair (dev/test)
# "jwks"  — remote JWKS endpoint (production)
type = "local"

# Required for both modes:
issuer = "test-issuer"
audience = "account"

# Required only for JWKS mode:
# url = "https://keycloak.example.com/realms/myrealm/protocol/openid-connect/certs"
```

See [`gitdiverge.toml.example`](gitdiverge.toml.example) for a full annotated configuration file including git credentials and web-client settings.
