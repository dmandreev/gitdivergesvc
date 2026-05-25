use crate::error::Error;
use crate::git::{Commit, GitProvider, LogOptions};
use crate::progress::{NoProgress, ProgressReporter};
use rayon::prelude::*;
use std::path::Path;

/// Query the log for multiple branches in parallel using Rayon's thread pool.
///
/// `progress` is advanced by one unit for **each branch completed** (not per
/// commit). If you need per-commit granularity, pass a custom reporter to the
/// individual `git.log()` calls instead.
///
/// # Example
///
/// ```ignore
/// use gitdiverge_lib::{ProcessGitProvider, log_parallel, LogOptions, NoProgress};
/// use std::path::Path;
///
/// let git = ProcessGitProvider::new();
/// let branches = vec!["main".to_string(), "feature".to_string()];
/// let results = log_parallel(&git, Path::new("."), &branches, LogOptions::default(), &NoProgress);
/// ```
pub fn log_parallel(
    git: &dyn GitProvider,
    repo: &Path,
    branches: &[String],
    options: LogOptions,
    progress: &dyn ProgressReporter,
) -> Vec<Result<Vec<Commit>, Error>> {
    progress.start(Some(branches.len() as u64), "querying branches");
    let results: Vec<_> = branches
        .par_iter()
        .map(|branch| {
            let _span = tracing::info_span!("log_parallel", branch = %branch).entered();
            let result = git.log(repo, branch, options.clone(), &NoProgress);
            progress.advance(1);
            result
        })
        .collect();
    progress.finish();
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{Author, Commit, GitProvider, LogOptions};
    use crate::progress::ProgressReporter;
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct MockGitProvider {
        responses: Mutex<HashMap<String, Result<Vec<Commit>, String>>>,
    }

    impl MockGitProvider {
        fn with_response(self, branch: &str, result: Result<Vec<Commit>, String>) -> Self {
            self.responses
                .lock()
                .unwrap()
                .insert(branch.to_string(), result);
            self
        }
    }

    impl GitProvider for MockGitProvider {
        fn log(
            &self,
            _repo: &Path,
            branch: &str,
            _options: LogOptions,
            _progress: &dyn ProgressReporter,
        ) -> Result<Vec<Commit>, Error> {
            match self.responses.lock().unwrap().get(branch) {
                Some(Ok(commits)) => Ok(commits.clone()),
                Some(Err(msg)) => Err(Error::ReferenceNotFound(msg.clone())),
                None => Ok(Vec::new()),
            }
        }

        fn branches(
            &self,
            _repo: &Path,
            _progress: &dyn ProgressReporter,
        ) -> Result<Vec<String>, Error> {
            Ok(Vec::new())
        }
    }

    fn make_commit(hash: &str, subject: &str) -> Commit {
        Commit {
            hash: hash.to_string(),
            author: Author {
                name: "Test".to_string(),
                email: "test@example.com".to_string(),
            },
            timestamp: Utc.timestamp_opt(1700000000, 0).unwrap(),
            subject: subject.to_string(),
            body: None,
            parents: Vec::new(),
        }
    }

    #[test]
    fn log_parallel_queries_all_branches() {
        let git = MockGitProvider::default()
            .with_response("main", Ok(vec![make_commit("abc", "First")]))
            .with_response("dev", Ok(vec![make_commit("def", "Second")]));

        let results = log_parallel(
            &git,
            Path::new("."),
            &["main".to_string(), "dev".to_string()],
            LogOptions::default(),
            &crate::progress::NoProgress,
        );

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].as_ref().unwrap()[0].subject, "First");
        assert_eq!(results[1].as_ref().unwrap()[0].subject, "Second");
    }

    #[test]
    fn log_parallel_propagates_errors() {
        let git = MockGitProvider::default()
            .with_response("main", Ok(vec![make_commit("abc", "First")]))
            .with_response("bad", Err("bad".to_string()));

        let results = log_parallel(
            &git,
            Path::new("."),
            &["main".to_string(), "bad".to_string()],
            LogOptions::default(),
            &crate::progress::NoProgress,
        );

        assert!(results[0].is_ok());
        assert!(results[1].is_err());
    }

    #[test]
    fn log_parallel_with_empty_branches() {
        let results = log_parallel(
            &MockGitProvider::default(),
            Path::new("."),
            &[],
            LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(results.is_empty());
    }
}
