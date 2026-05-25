use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level service configuration loaded from a TOML file.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub webclient: WebClientConfig,
    #[serde(default)]
    pub git_credentials: Vec<GitCredentialConfig>,
}

/// Git credential configuration for a specific host.
///
/// The token is read from an external file so that secrets never live in the
/// main configuration file.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitCredentialConfig {
    /// Host name to match (e.g. `gitlab.example.com`).
    ///
    /// The credential is used when a repository URL contains this value.
    pub host: String,
    /// Path to a file containing the access token.
    ///
    /// The file should contain a single line with the token and nothing else.
    pub token_file: PathBuf,
}

/// Authentication settings.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: AuthMode,
}

fn default_false() -> bool {
    false
}

/// Authentication backend mode.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMode {
    /// Validate tokens against a remote JWKS endpoint (e.g. Keycloak).
    Jwks {
        /// URL of the JWKS endpoint.
        url: String,
        /// Expected token issuer (`iss` claim).
        issuer: String,
        /// Expected token audience (`aud` claim).
        audience: String,
    },
    /// Validate tokens against a locally embedded RSA key pair.
    /// This mode is intended for development and integration testing
    /// without a live Keycloak instance.
    Local {
        /// Expected token issuer.
        issuer: String,
        /// Expected token audience.
        audience: String,
    },
}

impl Default for AuthMode {
    fn default() -> Self {
        Self::Local {
            issuer: "test-issuer".to_string(),
            audience: "account".to_string(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: AuthMode::Local {
                issuer: "test-issuer".to_string(),
                audience: "account".to_string(),
            },
        }
    }
}

/// Web-client runtime configuration.
///
/// Values here are injected into the `config.js` response served to browsers.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebClientConfig {
    /// Base URL for API calls (e.g. `http://localhost:8080`).
    /// Leave empty to use same-origin requests.
    #[serde(default)]
    pub api_base: String,
    /// OIDC authority URL (e.g. `https://keycloak.example.com/realms/master`).
    pub oidc_authority: Option<String>,
    /// OIDC client ID for the SPA.
    #[serde(default = "default_client_id")]
    pub oidc_client_id: String,
    /// JIRA server base URL (e.g. `https://jira.example.com`).
    /// Leave empty to disable JIRA integration.
    #[serde(default)]
    pub jira_server_addr: String,
}

fn default_client_id() -> String {
    "react-spa".to_string()
}

impl Default for WebClientConfig {
    fn default() -> Self {
        Self {
            api_base: String::new(),
            oidc_authority: None,
            oidc_client_id: default_client_id(),
            jira_server_addr: String::new(),
        }
    }
}

impl ServiceConfig {
    /// Example TOML configuration text.
    pub const EXAMPLE_TOML: &str = include_str!("../../gitdiverge.toml.example");

    /// Load configuration from a TOML file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let config: ServiceConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse config file {}", path.display()))?;
        tracing::info!(path = %path.display(), "configuration loaded successfully");
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn service_config_defaults() {
        let cfg = ServiceConfig::default();
        assert!(!cfg.auth.enabled);
        assert!(matches!(cfg.auth.mode, AuthMode::Local { .. }));
    }

    #[test]
    fn auth_config_default() {
        let auth = AuthConfig::default();
        assert!(!auth.enabled);
        assert!(matches!(auth.mode, AuthMode::Local { .. }));
    }

    #[test]
    fn auth_mode_default() {
        let mode = AuthMode::default();
        assert!(
            matches!(mode, AuthMode::Local { issuer, audience } if issuer == "test-issuer" && audience == "account")
        );
    }

    #[test]
    fn load_valid_config() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmp,
            r#"
[auth]
enabled = false

[auth.mode]
type = "local"
issuer = "custom"
audience = "app"
"#
        )
        .unwrap();
        let cfg = ServiceConfig::load(tmp.path()).unwrap();
        assert!(!cfg.auth.enabled);
        assert!(matches!(
            cfg.auth.mode,
            AuthMode::Local {
                issuer,
                audience
            } if issuer == "custom" && audience == "app"
        ));
    }

    #[test]
    fn load_missing_file_errors() {
        let path = Path::new("/nonexistent/config.toml");
        let err = ServiceConfig::load(path).unwrap_err();
        assert!(err.to_string().contains("failed to read config file"));
    }

    #[test]
    fn load_config_uses_default_true_for_enabled() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmp,
            r#"
[auth.mode]
type = "local"
issuer = "x"
audience = "y"
"#
        )
        .unwrap();
        let cfg = ServiceConfig::load(tmp.path()).unwrap();
        assert!(!cfg.auth.enabled); // default_false() exercised here
    }

    #[test]
    fn load_invalid_toml_errors() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "not valid toml {{").unwrap();
        let err = ServiceConfig::load(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("failed to parse config file"));
    }

    #[test]
    fn webclient_config_default() {
        let cfg = WebClientConfig::default();
        assert_eq!(cfg.api_base, "");
        assert_eq!(cfg.oidc_authority, None);
        assert_eq!(cfg.oidc_client_id, "react-spa");
        assert_eq!(cfg.jira_server_addr, "");
    }

    #[test]
    fn load_config_with_webclient_section() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmp,
            r#"
[webclient]
api_base = "http://localhost:9000"
oidc_authority = "https://auth.example.com/realms/master"
oidc_client_id = "my-client"
jira_server_addr = "https://jira.example.com"
"#
        )
        .unwrap();
        let cfg = ServiceConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.webclient.api_base, "http://localhost:9000");
        assert_eq!(
            cfg.webclient.oidc_authority,
            Some("https://auth.example.com/realms/master".to_string())
        );
        assert_eq!(cfg.webclient.oidc_client_id, "my-client");
        assert_eq!(cfg.webclient.jira_server_addr, "https://jira.example.com");
    }

    #[test]
    fn example_toml_is_not_empty() {
        assert!(!ServiceConfig::EXAMPLE_TOML.is_empty());
        assert!(ServiceConfig::EXAMPLE_TOML.contains("[auth]"));
    }

    #[test]
    fn load_config_with_jwks_mode() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmp,
            r#"
[auth.mode]
type = "jwks"
url = "https://example.com/.well-known/jwks.json"
issuer = "https://example.com"
audience = "my-app"
"#
        )
        .unwrap();
        let cfg = ServiceConfig::load(tmp.path()).unwrap();
        assert!(matches!(
            cfg.auth.mode,
            AuthMode::Jwks { url, issuer, audience }
            if url == "https://example.com/.well-known/jwks.json"
            && issuer == "https://example.com"
            && audience == "my-app"
        ));
    }

    #[test]
    fn load_config_with_git_credentials() {
        let mut token_file = tempfile::NamedTempFile::new().unwrap();
        write!(token_file, "glpat-secret-token").unwrap();
        let token_path = token_file.path().to_str().unwrap().replace('\\', "/");

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmp,
            r#"
[[git_credentials]]
host = "gitlab.example.com"
token_file = "{token_path}"
"#
        )
        .unwrap();
        let cfg = ServiceConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.git_credentials.len(), 1);
        assert_eq!(cfg.git_credentials[0].host, "gitlab.example.com");
        assert_eq!(
            cfg.git_credentials[0].token_file,
            std::path::PathBuf::from(token_file.path())
        );
    }

    #[test]
    fn git_credentials_default_is_empty() {
        let cfg = ServiceConfig::default();
        assert!(cfg.git_credentials.is_empty());
    }
}
