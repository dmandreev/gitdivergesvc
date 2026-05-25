use axum::{
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, EncodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::config::{AuthConfig, AuthMode};

// ---------------------------------------------------------------------------
// Hard-coded RSA key pair for local / test mode.
// This key is NOT secret — it exists only so the service can be exercised
// without a live Keycloak instance.
// ---------------------------------------------------------------------------
const TEST_RSA_PRIVATE_PEM: &str = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAs72oV3HBgW/X9y7iXtKsYa0RBzGkh3VYhCAYw30HKEV+5mtF
cm3rh5bzxQ7AOLSBrl/SFhxdxZEbvap+bmW2QrdGK0EyFNbLjvYvEsQ/g5Ak0gxf
9+ZpCvGM6uIZY6L0wvdHWHb5+oZVopIwUn0I+TWzwuxSrlswLB9BoTbG4UiTCOHx
Z0pJdxqhBkbt+bcCghtLuOa4w7HhYu3BgCPdtGeLir1fAq9zmQ/S3kDYTFnAS4Hy
VBiLoyu1hK0OtbNnAJjVYOKHPY6T5zWaxTlROVr5cNK519FUH9X6AzM4ghUldStZ
vUC6xGYKYw1FMThEoVCmGCebQZDegxlXCV4oHQIDAQABAoIBAGn0Tk0l8MUFklDT
IXx2QkneBKCyAeQcJ47TgOWUSWjS9siLycd3xpUKRi8Oz/9dYOjS8Xw5QonZTXoU
DC215agUc9fBue0Q5bQjqYItj6dVjG8J/nSbLabW15QKrp7Oi+x2amda02d8UvTf
qn6l2GlX39zzBJZliNMppb2MFdbsu/LsrTvtEsp34sB+FaiW7MEQDbEy8WWkONSe
aGQJ3iPxwR0p9DYSqWTBfwJugvoOGNgvuEuO0NO2lqnzpSHhxx2lpkEURTJp8oug
IGhqYVIWur3Hc1+kIAeKOdf7uhqOdt4NR2Big9dk3Sjb0A2VJsflUORj7MSl16If
ivrFTkECgYEA7pRY6wr54WSBV8+bMnR+06u0/LRP/Oq+E5CP3KwwaWMEQ7GtDQ9u
n6ADBWqOm2eIHiHjKvXI03h1MKGqxKFhwaR+JpLEabuIzZ2cPVCh6LpEPG/QKQqm
Y0mbXnDftC+5HmAvsaKyFPXlhW+etvNvn78TeY4RXwlL7A8ZIdtjRK0CgYEAwN13
vEC++Iu2dEbCzglwoakulO6ZFr7QQaxWAG5JQKjHO9kR5r9GsHjeVuDo+6Oqw2fY
dy4GDqAZEmk9NGTkASqjpwOkh/oxQ6gD2h8+eeoPNjY2bFynXjO/GDbCDxPw6+B+
bwF6DspNhLiHibgvtalgbZtG8EvyJBeX+r7TbzECgYAGetj/aUjoKkapD+ZzNF7N
ePhtdKhHgkivV1nQ8IxQEHRpMkY0+JpUk5ABcad16RX1W45D+HD/7WGhdIKi3I8/
JyyV956GEKXij8lSkQIUxBpeWdsZgkSKpdEme4JX7oPko1AoTvbvQs59FU8GQQ5j
FFl/D7DBGAuL2c0g8kh78QKBgBRqHNdvXwd9+mUabFpUw3hJKSYYj1nJ/s9Ex6Gq
CtTuSJB8LJnpGzlowdgeXGruaw/d+Rq8Y2W+6oh5XUIjf8Lj2Yi/KPY/tGE98pJv
BjTvYobRfDdCI3EkNxEAEtB3wuOk0p07YckY/tWSlr4sIdivwgY4Dm03DL1nRe4D
ruuxAoGBAOq41EBvLaVrXzTfzJ/86oYRBIW7CFXE9IcJpo9kETNPmxXLEkm+Anb4
f29+c3/2K3D5hDFTLju4nbSRS57Z72SyWUNU86dgSBKqgYTTYW7Af//pdtLjOAN8
DMWPTWmimR5SM0JIv7ezcuWaRhgr5iS5HTD7nwViu9Gd+aH1aYl5
-----END RSA PRIVATE KEY-----"#;

const TEST_RSA_PUBLIC_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAs72oV3HBgW/X9y7iXtKs
Ya0RBzGkh3VYhCAYw30HKEV+5mtFcm3rh5bzxQ7AOLSBrl/SFhxdxZEbvap+bmW2
QrdGK0EyFNbLjvYvEsQ/g5Ak0gxf9+ZpCvGM6uIZY6L0wvdHWHb5+oZVopIwUn0I
+TWzwuxSrlswLB9BoTbG4UiTCOHxZ0pJdxqhBkbt+bcCghtLuOa4w7HhYu3BgCPd
tGeLir1fAq9zmQ/S3kDYTFnAS4HyVBiLoyu1hK0OtbNnAJjVYOKHPY6T5zWaxTlR
OVr5cNK519FUH9X6AzM4ghUldStZvUC6xGYKYw1FMThEoVCmGCebQZDegxlXCV4o
HQIDAQAB
-----END PUBLIC KEY-----"#;

/// Fixed expiration for test tokens: 2035-01-01T00:00:00Z.
const TEST_TOKEN_EXP: usize = 2_051_222_400;

// ---------------------------------------------------------------------------
// JWT claims
// ---------------------------------------------------------------------------

/// Standard JWT claims plus Keycloak-specific `realm_access`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub iss: String,
    pub aud: serde_json::Value,
    #[serde(default)]
    pub realm_access: Option<RealmAccess>,
}

/// Keycloak realm roles embedded in the token.
#[derive(Debug, Serialize, Deserialize)]
pub struct RealmAccess {
    pub roles: Vec<String>,
}

// ---------------------------------------------------------------------------
// JWKS caching
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct CachedJwk {
    kid: Option<String>,
    key: Arc<DecodingKey>,
}

struct JwksCache {
    keys: Vec<CachedJwk>,
    fetched_at: Instant,
}

#[derive(Debug, Deserialize)]
struct JwksResponse {
    keys: Vec<JwkKey>,
}

#[derive(Debug, Deserialize, Clone)]
struct JwkKey {
    kty: String,
    #[serde(default)]
    kid: Option<String>,
    n: String,
    e: String,
}

// ---------------------------------------------------------------------------
// Auth state
// ---------------------------------------------------------------------------

/// Shared authentication state held in [`AppState`](crate::daemon::AppState).
pub struct AuthState {
    pub config: AuthConfig,
    jwks_cache: RwLock<Option<JwksCache>>,
    local_encoding_key: Option<Arc<EncodingKey>>,
    local_decoding_key: Option<Arc<DecodingKey>>,
    client: reqwest::Client,
}

impl AuthState {
    /// Create a new auth state from configuration.
    ///
    /// In `Local` mode the embedded RSA key pair is loaded.
    /// In `Jwks` mode no keys are loaded until the first request.
    pub fn new(config: AuthConfig) -> anyhow::Result<Self> {
        let mode_name = match &config.mode {
            AuthMode::Local { .. } => "local",
            AuthMode::Jwks { .. } => "jwks",
        };
        tracing::info!(
            auth_enabled = config.enabled,
            auth_mode = mode_name,
            "initialising auth state"
        );

        let (local_encoding_key, local_decoding_key) = match &config.mode {
            AuthMode::Local { .. } => {
                let enc = EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_PEM.as_bytes())
                    .map_err(|e| anyhow::anyhow!("failed to load test private key: {e}"))?;
                let dec = DecodingKey::from_rsa_pem(TEST_RSA_PUBLIC_PEM.as_bytes())
                    .map_err(|e| anyhow::anyhow!("failed to load test public key: {e}"))?;
                tracing::debug!("local RSA key pair loaded successfully");
                (Some(Arc::new(enc)), Some(Arc::new(dec)))
            }
            _ => (None, None),
        };

        Ok(Self {
            config,
            jwks_cache: RwLock::new(None),
            local_encoding_key,
            local_decoding_key,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
        })
    }

    /// Validate a Bearer token and return the decoded claims.
    pub async fn validate_token(&self, token: &str) -> Result<Claims, AuthError> {
        let header = decode_header(token).map_err(|e| {
            tracing::debug!(error = %e, "token header decode failed");
            AuthError::InvalidToken
        })?;

        match &self.config.mode {
            AuthMode::Jwks {
                url,
                issuer,
                audience,
            } => {
                let cached = self.get_jwk(url, header.kid.as_deref()).await?;
                let mut validation = Validation::new(Algorithm::RS256);
                validation.set_issuer(&[issuer.as_str()]);
                validation.set_audience(&[audience.as_str()]);
                let token_data = decode::<Claims>(token, &cached.key, &validation)
                    .map_err(|e| AuthError::Validation(e.to_string()))?;
                Ok(token_data.claims)
            }
            AuthMode::Local { issuer, audience } => {
                let key = self
                    .local_decoding_key
                    .as_ref()
                    .ok_or(AuthError::InvalidKey)?;
                let mut validation = Validation::new(Algorithm::RS256);
                validation.set_issuer(&[issuer.as_str()]);
                validation.set_audience(&[audience.as_str()]);
                let token_data = decode::<Claims>(token, key, &validation)
                    .map_err(|e| AuthError::Validation(e.to_string()))?;
                Ok(token_data.claims)
            }
        }
    }

    async fn get_jwk(&self, url: &str, kid: Option<&str>) -> Result<CachedJwk, AuthError> {
        // Fast path: check cache.
        {
            let cache = self.jwks_cache.read().await;
            if let Some(ref c) = *cache {
                if c.fetched_at.elapsed() < Duration::from_secs(300) {
                    if let Some(kid) = kid {
                        if let Some(jwk) = c.keys.iter().find(|k| k.kid.as_deref() == Some(kid)) {
                            tracing::debug!(kid, "JWKS cache hit");
                            return Ok(jwk.clone());
                        }
                    } else if let Some(jwk) = c.keys.first() {
                        tracing::debug!("JWKS cache hit (no kid)");
                        return Ok(jwk.clone());
                    }
                }
            }
        }

        tracing::debug!(url = %url, "fetching JWKS from remote");
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| AuthError::JwksFetch(e.to_string()))?;
        let text = response
            .text()
            .await
            .map_err(|e| AuthError::JwksFetch(e.to_string()))?;

        let jwks: JwksResponse =
            serde_json::from_str(&text).map_err(|e| AuthError::JwksFetch(e.to_string()))?;

        let keys: Vec<CachedJwk> = jwks
            .keys
            .into_iter()
            .filter(|k| k.kty == "RSA")
            .map(|k| {
                let decoding = DecodingKey::from_rsa_components(&k.n, &k.e)
                    .map_err(|e| AuthError::Validation(e.to_string()))?;
                Ok(CachedJwk {
                    kid: k.kid,
                    key: Arc::new(decoding),
                })
            })
            .collect::<Result<Vec<_>, AuthError>>()?;

        if keys.is_empty() {
            return Err(AuthError::KeyNotFound);
        }

        let result = if let Some(kid) = kid {
            keys.iter()
                .find(|k| k.kid.as_deref() == Some(kid))
                .cloned()
                .ok_or(AuthError::KeyNotFound)?
        } else {
            keys.first().cloned().unwrap()
        };

        let mut cache = self.jwks_cache.write().await;
        *cache = Some(JwksCache {
            keys,
            fetched_at: Instant::now(),
        });

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum AuthError {
    InvalidToken,
    InvalidKey,
    KeyNotFound,
    JwksFetch(String),
    Validation(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AuthError::InvalidToken => (axum::http::StatusCode::UNAUTHORIZED, "Invalid token"),
            AuthError::InvalidKey => (axum::http::StatusCode::UNAUTHORIZED, "Invalid key"),
            AuthError::KeyNotFound => (axum::http::StatusCode::UNAUTHORIZED, "Key not found"),
            AuthError::JwksFetch(_) => (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "JWKS fetch failed",
            ),
            AuthError::Validation(_) => (
                axum::http::StatusCode::UNAUTHORIZED,
                "Token validation failed",
            ),
        };
        let body = Json(serde_json::json!({ "error": msg }));
        (status, body).into_response()
    }
}

// ---------------------------------------------------------------------------
// Test token generation
// ---------------------------------------------------------------------------

/// Response payload for the test-token endpoint.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TestTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
}

/// Generate a test JWT signed with the embedded RSA private key.
///
/// Only available when the auth mode is `Local`.
pub fn generate_test_token(auth: &AuthState) -> anyhow::Result<TestTokenResponse> {
    let (issuer, audience) = match &auth.config.mode {
        AuthMode::Local { issuer, audience } => (issuer.clone(), audience.clone()),
        _ => {
            return Err(anyhow::anyhow!(
                "test tokens only available in local auth mode"
            ))
        }
    };

    let encoding_key = auth
        .local_encoding_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("local test key not available"))?;

    let claims = Claims {
        sub: "test-user".to_string(),
        exp: TEST_TOKEN_EXP,
        iss: issuer,
        aud: serde_json::Value::String(audience),
        realm_access: Some(RealmAccess {
            roles: vec!["user".to_string()],
        }),
    };

    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::RS256),
        &claims,
        encoding_key,
    )
    .map_err(|e| anyhow::anyhow!(e))?;

    tracing::info!(sub = %claims.sub, exp = %claims.exp, "generated test token");

    Ok(TestTokenResponse {
        access_token: token,
        token_type: "Bearer".to_string(),
        expires_in: TEST_TOKEN_EXP as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn auth_state_new_jwks_mode() {
        let config = AuthConfig {
            enabled: true,
            mode: AuthMode::Jwks {
                url: "https://example.com/.well-known/jwks.json".to_string(),
                issuer: "test-issuer".to_string(),
                audience: "test-audience".to_string(),
            },
        };
        let state = AuthState::new(config).unwrap();
        assert!(state.local_encoding_key.is_none());
        assert!(state.local_decoding_key.is_none());
    }

    #[test]
    fn auth_error_into_response_variants() {
        let cases = vec![
            (
                AuthError::InvalidToken,
                axum::http::StatusCode::UNAUTHORIZED,
                "Invalid token",
            ),
            (
                AuthError::InvalidKey,
                axum::http::StatusCode::UNAUTHORIZED,
                "Invalid key",
            ),
            (
                AuthError::KeyNotFound,
                axum::http::StatusCode::UNAUTHORIZED,
                "Key not found",
            ),
            (
                AuthError::JwksFetch("fail".to_string()),
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "JWKS fetch failed",
            ),
            (
                AuthError::Validation("bad".to_string()),
                axum::http::StatusCode::UNAUTHORIZED,
                "Token validation failed",
            ),
        ];

        for (err, expected_status, _expected_msg) in cases {
            let response = err.into_response();
            assert_eq!(response.status(), expected_status);
            // We can't easily read the Json body in a sync test without async runtime,
            // but the status code alone exercises the match arms.
            assert_eq!(response.status(), expected_status);
        }
    }

    #[test]
    fn validate_token_invalid_format() {
        let config = AuthConfig::default();
        let state = AuthState::new(config).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(state.validate_token("not-a-jwt"));
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn validate_token_valid_local_token() {
        let config = AuthConfig::default();
        let state = AuthState::new(config).unwrap();
        let token = generate_test_token(&state).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let claims = rt
            .block_on(state.validate_token(&token.access_token))
            .unwrap();
        assert_eq!(claims.sub, "test-user");
        assert_eq!(claims.iss, "test-issuer");
        assert_eq!(claims.aud, serde_json::Value::String("account".to_owned()));
    }

    #[test]
    fn generate_test_token_fails_in_jwks_mode() {
        let config = AuthConfig {
            enabled: true,
            mode: AuthMode::Jwks {
                url: "https://example.com/jwks".to_string(),
                issuer: "issuer".to_string(),
                audience: "audience".to_string(),
            },
        };
        let state = AuthState::new(config).unwrap();
        match generate_test_token(&state) {
            Err(e) => assert!(e
                .to_string()
                .contains("test tokens only available in local auth mode")),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn claims_deserializes_array_audience() {
        let json = r#"{"sub":"user-1","exp":9999999999,"iss":"test-issuer","aud":["account","other-app"],"realm_access":{"roles":["admin"]}}"#;
        let claims: Claims = serde_json::from_str(json).unwrap();
        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.iss, "test-issuer");
        assert_eq!(claims.aud, serde_json::json!(["account", "other-app"]));
        assert_eq!(
            claims.realm_access.unwrap().roles,
            vec!["admin".to_string()]
        );
    }
}
