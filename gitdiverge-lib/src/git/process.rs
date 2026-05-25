use crate::error::Error;
use crate::git::{AskpassScript, Author, Commit, GitCredential, GitProvider, LogOptions};
use crate::progress::ProgressReporter;
use chrono::{TimeZone, Utc};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tracing::{debug, error, instrument};

/// A [`GitProvider`] implementation that shells out to the system `git` executable.
///
/// This provider is platform agnostic — it only requires `git` (2.30.2+) to be
/// available in `PATH`.
///
/// Credentials for private repositories can be supplied via
/// [`ProcessGitProvider::with_credentials`].  Tokens are injected securely
/// using the `GIT_ASKPASS` mechanism so that secrets never appear in URLs or
/// command-line arguments.
#[derive(Debug, Clone)]
pub struct ProcessGitProvider {
    credentials: Vec<GitCredential>,
    askpass: Option<Arc<AskpassScript>>,
}

impl Default for ProcessGitProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessGitProvider {
    /// Create a new `ProcessGitProvider` with no credentials.
    pub fn new() -> Self {
        Self {
            credentials: Vec::new(),
            askpass: None,
        }
    }

    /// Create a provider that authenticates against specific hosts using
    /// personal access tokens read from files.
    ///
    /// An internal `GIT_ASKPASS` helper script is created once and reused for
    /// all subsequent git operations.  The script is cleaned up automatically
    /// when the provider is dropped.
    pub fn with_credentials(credentials: Vec<GitCredential>) -> std::io::Result<Self> {
        let askpass = if credentials.is_empty() {
            None
        } else {
            Some(Arc::new(AskpassScript::new()?))
        };
        Ok(Self {
            credentials,
            askpass,
        })
    }

    /// Return the token that matches `url`, if any.
    fn resolve_token(&self, url: &str) -> Option<&str> {
        for cred in &self.credentials {
            if url.contains(&cred.host) {
                return Some(&cred.token);
            }
        }
        None
    }

    /// Build a base `git` [`Command`], injecting authentication configuration
    /// when a matching credential exists.
    fn git_cmd(&self, url: Option<&str>, repo_path: Option<&Path>) -> Command {
        let mut cmd = Command::new("git");
        if let Some(path) = repo_path {
            cmd.current_dir(path);
        }

        // Prevent git from caching credentials in ~/.git-credentials.
        cmd.arg("-c").arg("credential.helper=");

        if let Some(url) = url {
            if let Some(token) = self.resolve_token(url) {
                if let Some(script) = &self.askpass {
                    cmd.env("GIT_ASKPASS", script.path());
                    cmd.env("GITDIVERGE_TOKEN", token);
                }
            }
        }

        cmd
    }

    /// Clone a remote repository to `dest`.
    #[instrument(skip(self, progress), fields(url = %url, dest = ?dest))]
    pub fn clone_repo(
        &self,
        url: &str,
        dest: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<(), Error> {
        progress.start(None, &format!("cloning {}", url));
        debug!("spawning git clone");
        let output = self
            .git_cmd(Some(url), None)
            .arg("clone")
            .arg(url)
            .arg(dest)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            error!(%stderr, "git clone failed");
            return Err(Error::GitCommand {
                repo_path: dest.to_path_buf(),
                stderr,
            });
        }
        debug!(%url, dest = ?dest, "git clone completed");
        progress.finish();
        Ok(())
    }

    /// Read the URL of a remote.
    #[instrument(skip(self), fields(repo = ?repo_path, remote = %remote))]
    pub fn get_remote_url(&self, repo_path: &Path, remote: &str) -> Result<String, Error> {
        debug!("spawning git config --get remote.{}.url", remote);
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["config", "--get", &format!("remote.{}.url", remote)])
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            error!(%stderr, "git remote get-url failed");
            return Err(Error::GitCommand {
                repo_path: repo_path.to_path_buf(),
                stderr,
            });
        }
        let url = String::from_utf8(output.stdout)?.trim().to_string();
        Ok(url)
    }

    /// Fetch all refs from `origin`.
    #[instrument(skip(self, progress), fields(repo = ?repo_path))]
    pub fn fetch(&self, repo_path: &Path, progress: &dyn ProgressReporter) -> Result<(), Error> {
        progress.start(None, "fetching origin");
        debug!("spawning git fetch origin");
        let t0 = std::time::Instant::now();

        // When credentials are configured we need the remote URL to pick the
        // right token.  If reading the URL fails we fall back to a bare fetch
        // so that repos cloned before credential setup still work.
        let remote_url = if self.credentials.is_empty() {
            None
        } else {
            self.get_remote_url(repo_path, "origin").ok()
        };

        let output = self
            .git_cmd(remote_url.as_deref(), Some(repo_path))
            .args(["fetch", "origin"])
            .output()?;
        let elapsed = t0.elapsed().as_millis();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            error!(%stderr, elapsed_ms = elapsed, "git fetch failed");
            return Err(Error::GitCommand {
                repo_path: repo_path.to_path_buf(),
                stderr,
            });
        }
        debug!(repo = ?repo_path, elapsed_ms = elapsed, "git fetch completed");
        progress.finish();
        Ok(())
    }

    /// Check whether `origin/<branch>` exists after a fetch.
    #[instrument(skip(self), fields(repo = ?repo_path, branch = %branch))]
    pub fn branch_exists_on_remote(&self, repo_path: &Path, branch: &str) -> Result<bool, Error> {
        let t0 = std::time::Instant::now();
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["rev-parse", "--verify", &format!("origin/{}", branch)])
            .output()?;
        debug!(repo = ?repo_path, %branch, elapsed_ms = t0.elapsed().as_millis(), "branch_exists_on_remote completed");
        if output.status.success() {
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.contains("unknown revision")
                || stderr.contains("bad revision")
                || stderr.contains("Needed a single revision")
            {
                Ok(false)
            } else {
                Err(Error::GitCommand {
                    repo_path: repo_path.to_path_buf(),
                    stderr,
                })
            }
        }
    }

    /// Checkout (or create/reset) a local branch to match `origin/<branch>`.
    #[instrument(skip(self, progress), fields(repo = ?repo_path, branch = %branch))]
    pub fn checkout_branch(
        &self,
        repo_path: &Path,
        branch: &str,
        progress: &dyn ProgressReporter,
    ) -> Result<(), Error> {
        progress.start(None, &format!("updating {}", branch));
        debug!("spawning git checkout -B branch origin/branch");
        let t0 = std::time::Instant::now();
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["checkout", "-B", branch, &format!("origin/{}", branch)])
            .output()?;
        let elapsed = t0.elapsed().as_millis();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            error!(%stderr, elapsed_ms = elapsed, "git checkout failed");
            return Err(Error::GitCommand {
                repo_path: repo_path.to_path_buf(),
                stderr,
            });
        }
        debug!(repo = ?repo_path, %branch, elapsed_ms = elapsed, "git checkout completed");
        progress.finish();
        Ok(())
    }

    fn run_git(&self, repo_path: &Path, args: &[String]) -> Result<std::process::Output, Error> {
        let output = Command::new("git")
            .current_dir(repo_path)
            .arg("-c")
            .arg("log.showSignature=false")
            .args(args)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            error!(%stderr, "git command failed");
            // Distinguish "bad revision" from other errors.
            if stderr.contains("unknown revision")
                || stderr.contains("bad revision")
                || stderr.contains("Not a valid object name")
            {
                return Err(Error::ReferenceNotFound(stderr));
            }
            return Err(Error::GitCommand {
                repo_path: repo_path.to_path_buf(),
                stderr,
            });
        }

        Ok(output)
    }
}

impl GitProvider for ProcessGitProvider {
    #[instrument(skip(self, options, progress), fields(repo = ?repo_path, branch = %branch))]
    fn log(
        &self,
        repo_path: &Path,
        branch: &str,
        options: LogOptions,
        progress: &dyn ProgressReporter,
    ) -> Result<Vec<Commit>, Error> {
        debug!("building git log command");
        let t0 = std::time::Instant::now();
        progress.start(
            options.max_count.map(|n| n as u64),
            &format!("git log {}", branch),
        );
        let mut args = vec!["log".to_string()];

        // Machine-readable format using ASCII Unit Separator (\x1F) between fields
        // and NUL (\x00) after each commit record:
        // hash\x1Fauthor_name\x1Fauthor_email\x1Fauthor_timestamp\x1Fparents\x1Fsubject\x1Fbody\x00
        args.push("--format=format:%H%x1F%an%x1F%ae%x1F%at%x1F%P%x1F%s%x1F%b%x00".to_string());

        if let Some(max_count) = options.max_count {
            args.push("-n".to_string());
            args.push(max_count.to_string());
        }
        if let Some(skip) = options.skip {
            args.push("--skip".to_string());
            args.push(skip.to_string());
        }
        if let Some(since) = options.since {
            args.push("--since".to_string());
            args.push(since.to_rfc3339());
        }
        if let Some(until) = options.until {
            args.push("--until".to_string());
            args.push(until.to_rfc3339());
        }
        if options.reverse {
            args.push("--reverse".to_string());
        }

        args.push(branch.to_string());

        let output = self.run_git(repo_path, &args)?;
        let stdout = String::from_utf8(output.stdout)?;
        debug!(
            bytes = stdout.len(),
            elapsed_ms = t0.elapsed().as_millis(),
            "received git output"
        );

        let commits = parse_log_output(&stdout, progress)?;
        debug!(
            commits = commits.len(),
            elapsed_ms = t0.elapsed().as_millis(),
            "git log parsed"
        );
        progress.finish();
        Ok(commits)
    }

    #[instrument(skip(self, progress), fields(repo = ?repo_path))]
    fn branches(
        &self,
        repo_path: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<Vec<String>, Error> {
        progress.start(None, "listing branches");
        debug!("spawning git for-each-ref refs/heads refs/remotes");
        let output = Command::new("git")
            .current_dir(repo_path)
            .args([
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/heads",
                "refs/remotes",
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            error!(%stderr, "git for-each-ref failed");
            return Err(Error::GitCommand {
                repo_path: repo_path.to_path_buf(),
                stderr,
            });
        }

        let stdout = String::from_utf8(output.stdout)?;
        let mut branches = Vec::new();
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || line == "HEAD" || line.ends_with("/HEAD") {
                continue;
            }
            // Strip the remote name prefix (typically "origin/").
            if let Some(branch) = line.split_once('/').map(|(_, b)| b.to_string()) {
                branches.push(branch);
            } else {
                branches.push(line.to_string());
            }
        }

        branches.sort();
        branches.dedup();
        debug!(count = branches.len(), "listed branches");
        progress.finish();
        Ok(branches)
    }
}

/// Parse the structured output produced by our custom `--format` string.
///
/// Fields are separated by ASCII Unit Separator (`\x1F`) and records are
/// terminated by NUL (`\x00`). This avoids ambiguity when fields (e.g.
/// `parents`, `body`) are empty.
fn parse_log_output(stdout: &str, progress: &dyn ProgressReporter) -> Result<Vec<Commit>, Error> {
    let mut commits = Vec::new();

    for chunk in stdout.split('\0') {
        if chunk.is_empty() || !chunk.contains('\x1F') {
            continue;
        }

        let fields: Vec<&str> = chunk.split('\x1F').collect();
        if fields.len() < 6 {
            return Err(Error::Parse(format!(
                "expected at least 6 fields, got {} in chunk: {:?}",
                fields.len(),
                chunk
            )));
        }

        let hash = fields[0].trim().to_string();
        let author_name = fields[1].trim().to_string();
        let author_email = fields[2].trim().to_string();
        let timestamp_secs: i64 = fields[3]
            .trim()
            .parse()
            .map_err(|e| Error::TimestampParse(format!("{}: {}", fields[3].trim(), e)))?;
        let timestamp = Utc
            .timestamp_opt(timestamp_secs, 0)
            .single()
            .ok_or_else(|| {
                Error::TimestampParse(format!("invalid timestamp: {}", timestamp_secs))
            })?;
        let parents: Vec<String> = if fields[4].trim().is_empty() {
            Vec::new()
        } else {
            fields[4].trim().split(' ').map(|s| s.to_string()).collect()
        };
        let subject = fields[5].trim().to_string();

        // Body is the last field. If the body itself contained \x1F characters,
        // they were split; join them back together.
        let body = if fields.len() > 6 {
            let body_parts = &fields[6..];
            let joined = body_parts.join("\x1F").trim().to_string();
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        } else {
            None
        };

        commits.push(Commit {
            hash,
            author: Author {
                name: author_name,
                email: author_email,
            },
            timestamp,
            subject,
            body,
            parents,
        });
        progress.advance(1);
    }

    Ok(commits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_commit() {
        let input =
            "abc123\x1FAlice\x1Falice@example.com\x1F1700000000\x1F\x1FInitial commit\x1F\x00";
        let commits = parse_log_output(input, &crate::progress::NoProgress).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "abc123");
        assert_eq!(commits[0].author.name, "Alice");
        assert_eq!(commits[0].author.email, "alice@example.com");
        assert_eq!(commits[0].subject, "Initial commit");
        assert_eq!(commits[0].body, None);
        assert!(commits[0].parents.is_empty());
    }

    #[test]
    fn parse_commit_with_body() {
        let input = "def456\x1FBob\x1Fbob@example.com\x1F1700000001\x1Fabc123\x1FAdd feature\x1FDetailed description\nwith multiple lines.\x00";
        let commits = parse_log_output(input, &crate::progress::NoProgress).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "def456");
        assert_eq!(commits[0].parents, vec!["abc123"]);
        assert_eq!(commits[0].subject, "Add feature");
        assert_eq!(
            commits[0].body,
            Some("Detailed description\nwith multiple lines.".to_string())
        );
    }

    #[test]
    fn parse_multiple_commits() {
        let input = "a\x1FA\x1Fa@x\x1F1700000000\x1F\x1FFirst\x1F\x00b\x1FB\x1Fb@x\x1F1700000001\x1Fa\x1FSecond\x1F\x00";
        let commits = parse_log_output(input, &crate::progress::NoProgress).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "a");
        assert_eq!(commits[1].hash, "b");
    }

    #[test]
    fn parse_merge_commit_with_multiple_parents() {
        let input = "m\x1FM\x1Fm@x\x1F1700000002\x1Fa b c\x1FMerge\x1F\x00";
        let commits = parse_log_output(input, &crate::progress::NoProgress).unwrap();
        assert_eq!(commits[0].parents, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_empty_output() {
        let commits = parse_log_output("", &crate::progress::NoProgress).unwrap();
        assert!(commits.is_empty());
    }

    #[test]
    fn parse_whitespace_only_output() {
        let commits = parse_log_output("   \n\n  ", &crate::progress::NoProgress).unwrap();
        assert!(commits.is_empty());
    }

    #[test]
    fn parse_body_with_unit_separator() {
        // Body contains the unit separator character (extremely rare in practice)
        let input = "h\x1FN\x1Fn@x\x1F1700000000\x1F\x1FSubject\x1FBody with \x1F separator\x00";
        let commits = parse_log_output(input, &crate::progress::NoProgress).unwrap();
        assert_eq!(
            commits[0].body,
            Some("Body with \x1F separator".to_string())
        );
    }

    #[test]
    fn parse_too_few_fields_errors() {
        let input = "abc\x1FAlice\x1Falice@example.com\x1F1700000000\x1Fparents\x00";
        let result = parse_log_output(input, &crate::progress::NoProgress);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("expected at least 6 fields"));
    }

    #[test]
    fn parse_invalid_timestamp_format_errors() {
        let input = "abc\x1FAlice\x1Falice@example.com\x1Fnot_a_number\x1F\x1FSubject\x1F\x00";
        let result = parse_log_output(input, &crate::progress::NoProgress);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("failed to parse timestamp"));
    }

    #[test]
    fn parse_out_of_range_timestamp_errors() {
        let input =
            "abc\x1FAlice\x1Falice@example.com\x1F9223372036854775807\x1F\x1FSubject\x1F\x00";
        let result = parse_log_output(input, &crate::progress::NoProgress);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid timestamp"));
    }

    #[test]
    fn parse_no_body_field_gives_none() {
        // Exactly 6 fields — no body at all
        let input = "abc\x1FAlice\x1Falice@example.com\x1F1700000000\x1F\x1FSubject\x00";
        let commits = parse_log_output(input, &crate::progress::NoProgress).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].body, None);
    }

    #[test]
    fn process_git_provider_new_and_default() {
        let _ = ProcessGitProvider::new();
        let _ = ProcessGitProvider::default();
    }

    #[test]
    fn branch_exists_on_remote_finds_existing_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        // Init repo and create a branch
        let status = std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        assert!(status.success());

        // Create initial commit so master exists
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        let status = std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();
        assert!(status.success());
        let status = std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();
        assert!(status.success());

        let provider = ProcessGitProvider::new();
        // origin/master won't exist, but master will exist locally.
        // Simulate remote by adding a remote pointing to itself and fetching.
        let status = std::process::Command::new("git")
            .current_dir(repo)
            .args(["remote", "add", "origin", repo.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .current_dir(repo)
            .args(["fetch", "origin", "master:refs/remotes/origin/master"])
            .status()
            .unwrap();
        assert!(status.success());

        assert!(provider.branch_exists_on_remote(repo, "master").unwrap());
    }

    #[test]
    fn branch_exists_on_remote_returns_false_for_missing_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let status = std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        assert!(status.success());

        let provider = ProcessGitProvider::new();
        assert!(!provider
            .branch_exists_on_remote(repo, "nonexistent")
            .unwrap());
    }

    #[test]
    fn clone_repo_fails_for_invalid_url() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("dest");
        let provider = ProcessGitProvider::new();
        let result = provider.clone_repo(
            "not-a-valid-url:///foo",
            &dest,
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
    }

    #[test]
    fn get_remote_url_fails_for_missing_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let status = std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        assert!(status.success());

        let provider = ProcessGitProvider::new();
        let result = provider.get_remote_url(repo, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn fetch_fails_for_non_git_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = ProcessGitProvider::new();
        let result = provider.fetch(tmp.path(), &crate::progress::NoProgress);
        assert!(result.is_err());
    }

    #[test]
    fn checkout_branch_fails_for_missing_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let status = std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        assert!(status.success());

        // Need an initial commit so we have a HEAD
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        let status = std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();
        assert!(status.success());
        let status = std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();
        assert!(status.success());

        let provider = ProcessGitProvider::new();
        let result =
            provider.checkout_branch(repo, "nonexistent-branch", &crate::progress::NoProgress);
        assert!(result.is_err());
    }

    #[test]
    fn run_git_generic_error() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = ProcessGitProvider::new();
        // Run a git command in a non-repo that fails with something other than "bad revision"
        let result = provider.log(
            tmp.path(),
            "HEAD",
            crate::git::LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        // Should be a GitCommand error, not ReferenceNotFound
        assert!(!err_str.contains("Reference not found"));
    }

    #[test]
    fn branch_exists_on_remote_unexpected_error() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = ProcessGitProvider::new();
        // A non-git directory causes "not a git repository" which is not in the
        // known-error list, so we should get a GitCommand error (lines 107-109).
        let result = provider.branch_exists_on_remote(tmp.path(), "main");
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("not a git repository") || err_str.contains("git repository"));
    }

    #[test]
    fn branches_lists_remote_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        // Init bare repo to act as remote
        let status = std::process::Command::new("git")
            .arg("init")
            .arg("--bare")
            .arg(repo.join("remote.git"))
            .status()
            .unwrap();
        assert!(status.success());

        // Create local repo, add remote, push two branches
        let local = repo.join("local");
        let status = std::process::Command::new("git")
            .args(["init", local.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());

        std::fs::write(local.join("a.txt"), "a").unwrap();
        let status = std::process::Command::new("git")
            .current_dir(&local)
            .args(["add", "."])
            .status()
            .unwrap();
        assert!(status.success());
        let status = std::process::Command::new("git")
            .current_dir(&local)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .current_dir(&local)
            .args(["checkout", "-b", "feature-x"])
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .current_dir(&local)
            .args([
                "remote",
                "add",
                "origin",
                repo.join("remote.git").to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success());

        let status = std::process::Command::new("git")
            .current_dir(&local)
            .args(["push", "-u", "origin", "master", "feature-x"])
            .status()
            .unwrap();
        assert!(status.success());

        // Clone to a second local repo so we have remotes
        let clone = repo.join("clone");
        let status = std::process::Command::new("git")
            .args([
                "clone",
                repo.join("remote.git").to_str().unwrap(),
                clone.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success());

        let provider = ProcessGitProvider::new();
        let branches = provider
            .branches(&clone, &crate::progress::NoProgress)
            .unwrap();
        assert!(branches.contains(&"master".to_string()));
        assert!(branches.contains(&"feature-x".to_string()));
    }

    #[test]
    fn branches_fails_for_non_git_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = ProcessGitProvider::new();
        let result = provider.branches(tmp.path(), &crate::progress::NoProgress);
        assert!(result.is_err());
    }

    #[test]
    fn with_credentials_creates_provider() {
        let provider = ProcessGitProvider::with_credentials(vec![GitCredential {
            host: "gitlab.example.com".into(),
            token: "glpat-test".into(),
        }])
        .unwrap();
        assert!(provider.askpass.is_some());
    }

    #[test]
    fn with_credentials_empty_vec_skips_askpass() {
        let provider = ProcessGitProvider::with_credentials(vec![]).unwrap();
        assert!(provider.askpass.is_none());
    }

    #[test]
    fn resolve_token_matches_host() {
        let provider = ProcessGitProvider::with_credentials(vec![
            GitCredential {
                host: "gitlab.example.com".into(),
                token: "gitlab-token".into(),
            },
            GitCredential {
                host: "github.com".into(),
                token: "github-token".into(),
            },
        ])
        .unwrap();

        assert_eq!(
            provider.resolve_token("https://gitlab.example.com/group/repo"),
            Some("gitlab-token")
        );
        assert_eq!(
            provider.resolve_token("https://github.com/user/repo"),
            Some("github-token")
        );
        assert_eq!(
            provider.resolve_token("https://bitbucket.org/user/repo"),
            None
        );
    }

    #[test]
    fn resolve_token_matches_ssh_urls() {
        let provider = ProcessGitProvider::with_credentials(vec![GitCredential {
            host: "gitlab.example.com".into(),
            token: "gitlab-token".into(),
        }])
        .unwrap();

        assert_eq!(
            provider.resolve_token("git@gitlab.example.com:group/repo.git"),
            Some("gitlab-token")
        );
    }

    #[test]
    fn clone_repo_without_credentials_still_works() {
        // Local file:// clones don't need auth.
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin.git");
        let dest = tmp.path().join("dest");

        std::process::Command::new("git")
            .args(["init", "--bare", origin.to_str().unwrap()])
            .status()
            .unwrap();

        let provider = ProcessGitProvider::with_credentials(vec![GitCredential {
            host: "never-matches".into(),
            token: "unused".into(),
        }])
        .unwrap();

        provider
            .clone_repo(
                &format!("file://{}", origin.to_str().unwrap().replace('\\', "/")),
                &dest,
                &crate::progress::NoProgress,
            )
            .unwrap();

        assert!(dest.join(".git").is_dir() || dest.join("HEAD").exists());
    }
}
