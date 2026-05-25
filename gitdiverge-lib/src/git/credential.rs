/// A loaded git credential (host + token) used by [`ProcessGitProvider`].
///
/// Tokens are read from external files once at provider construction time
/// so that missing or unreadable files produce an immediate error.
#[derive(Debug, Clone)]
pub struct GitCredential {
    /// Host name to match (e.g. `gitlab.example.com`).
    ///
    /// A credential applies when the repository URL contains this string.
    pub host: String,
    /// The secret token (e.g. a GitLab personal access token).
    pub token: String,
}

impl GitCredential {
    /// Load a credential from a file on disk.
    pub fn from_file(host: String, path: &std::path::Path) -> std::io::Result<Self> {
        let token = std::fs::read_to_string(path)?.trim().to_string();
        if token.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("token file {} is empty", path.display()),
            ));
        }
        Ok(Self { host, token })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn from_file_reads_token() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "my-secret-token").unwrap();
        let cred = GitCredential::from_file("gitlab.example.com".into(), tmp.path()).unwrap();
        assert_eq!(cred.host, "gitlab.example.com");
        assert_eq!(cred.token, "my-secret-token");
    }

    #[test]
    fn from_file_trims_whitespace() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "  token-with-spaces  \n").unwrap();
        let cred = GitCredential::from_file("github.com".into(), tmp.path()).unwrap();
        assert_eq!(cred.token, "token-with-spaces");
    }

    #[test]
    fn from_file_rejects_empty_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let result = GitCredential::from_file("gitlab.example.com".into(), tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("is empty"));
    }

    #[test]
    fn from_file_rejects_missing_file() {
        let result = GitCredential::from_file(
            "gitlab.example.com".into(),
            std::path::Path::new("/nonexistent/token"),
        );
        assert!(result.is_err());
    }
}
