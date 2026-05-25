use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur when interacting with a git repository.
#[derive(Error, Debug)]
pub enum Error {
    /// The git executable returned a non-zero exit code.
    #[error("git command failed in {repo_path}: {stderr}")]
    GitCommand { repo_path: PathBuf, stderr: String },

    /// Failed to execute the git process itself (e.g., git not found).
    #[error("failed to spawn git process: {0}")]
    Io(#[from] std::io::Error),

    /// Output from git was not valid UTF-8.
    #[error("git output is not valid UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// Failed to parse a timestamp from git output.
    #[error("failed to parse timestamp: {0}")]
    TimestampParse(String),

    /// Failed to parse structured git output.
    #[error("failed to parse git log output: {0}")]
    Parse(String),

    /// The requested branch or reference does not exist.
    #[error("reference not found: {0}")]
    ReferenceNotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_command_error_display() {
        let err = Error::GitCommand {
            repo_path: PathBuf::from("/tmp/repo"),
            stderr: "fatal: bad object".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "git command failed in /tmp/repo: fatal: bad object"
        );
    }

    #[test]
    fn io_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "git not found");
        let err = Error::Io(io_err);
        assert!(err.to_string().contains("git not found"));
    }

    #[test]
    fn utf8_error_display() {
        let bytes = vec![0x80, 0x81];
        let utf8_err = String::from_utf8(bytes).unwrap_err();
        let err = Error::Utf8(utf8_err);
        assert!(err.to_string().contains("UTF-8"));
    }

    #[test]
    fn timestamp_parse_error_display() {
        let err = Error::TimestampParse("invalid date".to_string());
        assert_eq!(err.to_string(), "failed to parse timestamp: invalid date");
    }

    #[test]
    fn parse_error_display() {
        let err = Error::Parse("unexpected format".to_string());
        assert_eq!(
            err.to_string(),
            "failed to parse git log output: unexpected format"
        );
    }

    #[test]
    fn reference_not_found_display() {
        let err = Error::ReferenceNotFound("bad-ref".to_string());
        assert_eq!(err.to_string(), "reference not found: bad-ref");
    }
}
