use crate::error::Error;
use crate::progress::ProgressReporter;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Information about the author of a commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Author {
    /// Author display name (e.g. "Alice Liddell").
    pub name: String,
    /// Author email address (e.g. "alice@example.com").
    pub email: String,
}

/// A single git commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Commit {
    /// Full 40-character SHA-1 hash of the commit.
    pub hash: String,
    /// Author metadata (name and email).
    pub author: Author,
    /// Commit timestamp in UTC.
    pub timestamp: DateTime<Utc>,
    /// First line of the commit message (the "subject").
    pub subject: String,
    /// Remaining lines of the commit message, if any.
    pub body: Option<String>,
    /// Hashes of the commit's parent commits. An empty vector indicates a root commit.
    pub parents: Vec<String>,
}

/// Options controlling the behavior of `GitProvider::log`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LogOptions {
    /// Maximum number of commits to return.
    pub max_count: Option<usize>,
    /// Number of commits to skip before starting to return results.
    pub skip: Option<usize>,
    /// Only return commits after this timestamp.
    pub since: Option<DateTime<Utc>>,
    /// Only return commits before this timestamp.
    pub until: Option<DateTime<Utc>>,
    /// Reverse the order of commits (oldest first).
    pub reverse: bool,
}

/// Abstract interface for git operations.
///
/// This trait is designed to be object-safe and thread-safe so that it can be
/// shared across async workers or used behind an `Arc<dyn GitProvider>`.
pub trait GitProvider: Send + Sync {
    /// Retrieve the commit log for the given branch or reference.
    ///
    /// # Arguments
    ///
    /// * `repo_path` — Filesystem path to the git repository.
    /// * `branch`    — Branch name or any valid git revision string.
    /// * `options`   — Filtering and pagination options.
    /// * `progress`  — Progress reporter for the operation.
    fn log(
        &self,
        repo_path: &Path,
        branch: &str,
        options: LogOptions,
        progress: &dyn ProgressReporter,
    ) -> Result<Vec<Commit>, Error>;

    /// List all remote-tracking branches in the repository.
    ///
    /// Returns the short branch names (e.g. `"main"`, `"feature-x"`) after
    /// stripping the remote name (typically `origin/`).
    ///
    /// # Arguments
    ///
    /// * `repo_path` — Filesystem path to the git repository.
    /// * `progress`  — Progress reporter for the operation.
    fn branches(
        &self,
        repo_path: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<Vec<String>, Error>;
}

mod askpass;
mod credential;
mod gix_provider;
mod process;
pub use askpass::AskpassScript;
pub use credential::GitCredential;
pub use gix_provider::GixProvider;
pub use process::ProcessGitProvider;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_options_default() {
        let opts = LogOptions::default();
        assert!(opts.max_count.is_none());
        assert!(opts.skip.is_none());
        assert!(opts.since.is_none());
        assert!(opts.until.is_none());
        assert!(!opts.reverse);
    }

    #[test]
    fn log_options_clone() {
        let since = chrono::Utc::now();
        let until = since + chrono::Duration::days(1);
        let opts = LogOptions {
            max_count: Some(5),
            skip: Some(1),
            since: Some(since),
            until: Some(until),
            reverse: true,
        };
        let cloned = opts.clone();
        assert_eq!(opts.max_count, cloned.max_count);
        assert_eq!(opts.skip, cloned.skip);
        assert_eq!(opts.since, cloned.since);
        assert_eq!(opts.until, cloned.until);
        assert_eq!(opts.reverse, cloned.reverse);
    }

    #[test]
    fn log_options_with_all_fields_set() {
        let since = chrono::Utc::now();
        let until = since + chrono::Duration::days(1);
        let opts = LogOptions {
            max_count: Some(10),
            skip: Some(5),
            since: Some(since),
            until: Some(until),
            reverse: true,
        };
        assert_eq!(opts.max_count, Some(10));
        assert_eq!(opts.skip, Some(5));
        assert_eq!(opts.since, Some(since));
        assert_eq!(opts.until, Some(until));
        assert!(opts.reverse);
    }

    #[test]
    fn author_serializes_to_json() {
        let author = Author {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
        };
        let json = serde_json::to_string(&author).unwrap();
        assert!(json.contains("Alice"));
        assert!(json.contains("alice@example.com"));
    }

    #[test]
    fn commit_serializes_to_json() {
        let commit = Commit {
            hash: "abc123".to_string(),
            author: Author {
                name: "Bob".to_string(),
                email: "bob@example.com".to_string(),
            },
            timestamp: chrono::Utc::now(),
            subject: "Fix bug".to_string(),
            body: Some("Detailed explanation".to_string()),
            parents: vec!["parent1".to_string(), "parent2".to_string()],
        };
        let json = serde_json::to_string(&commit).unwrap();
        assert!(json.contains("abc123"));
        assert!(json.contains("Fix bug"));
        assert!(json.contains("Detailed explanation"));
        assert!(json.contains("parent1"));
        assert!(json.contains("parent2"));
    }

    #[test]
    fn commit_with_none_body_serializes_correctly() {
        let commit = Commit {
            hash: "def456".to_string(),
            author: Author {
                name: "Carol".to_string(),
                email: "carol@example.com".to_string(),
            },
            timestamp: chrono::Utc::now(),
            subject: "Initial commit".to_string(),
            body: None,
            parents: vec![],
        };
        let json = serde_json::to_string(&commit).unwrap();
        assert!(json.contains("def456"));
        assert!(json.contains("null"));
    }

    #[test]
    fn commit_equality() {
        let timestamp = chrono::Utc::now();
        let a = Commit {
            hash: "h1".to_string(),
            author: Author {
                name: "A".to_string(),
                email: "a@x".to_string(),
            },
            timestamp,
            subject: "S".to_string(),
            body: None,
            parents: vec![],
        };
        let b = Commit {
            hash: "h1".to_string(),
            author: Author {
                name: "A".to_string(),
                email: "a@x".to_string(),
            },
            timestamp,
            subject: "S".to_string(),
            body: None,
            parents: vec![],
        };
        assert_eq!(a, b);
    }

    #[test]
    fn author_deserializes_from_json() {
        let original = Author {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Author = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn commit_deserializes_from_json() {
        let original = Commit {
            hash: "abc123".to_string(),
            author: Author {
                name: "Bob".to_string(),
                email: "bob@example.com".to_string(),
            },
            timestamp: chrono::Utc::now(),
            subject: "Fix bug".to_string(),
            body: Some("Detailed explanation".to_string()),
            parents: vec!["parent1".to_string()],
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Commit = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn commit_with_none_body_deserializes_correctly() {
        let original = Commit {
            hash: "def456".to_string(),
            author: Author {
                name: "Carol".to_string(),
                email: "carol@example.com".to_string(),
            },
            timestamp: chrono::Utc::now(),
            subject: "Initial commit".to_string(),
            body: None,
            parents: vec![],
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Commit = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }
}
