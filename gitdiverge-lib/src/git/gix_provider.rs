use crate::error::Error;
use crate::git::{Author, Commit, GitProvider, LogOptions};
use crate::progress::ProgressReporter;
use chrono::{TimeZone, Utc};
use std::path::Path;
use tracing::debug;

/// A [`GitProvider`] implementation backed by the pure-Rust [`gix`](gitoxide) library.
///
/// This provider does not shell out to the `git` executable; instead it reads
/// the object database and reference files directly.
#[derive(Debug, Clone, Default)]
pub struct GixProvider;

impl GixProvider {
    /// Create a new `GixProvider`.
    pub fn new() -> Self {
        Self
    }
}

impl GitProvider for GixProvider {
    fn log(
        &self,
        repo_path: &Path,
        branch: &str,
        options: LogOptions,
        progress: &dyn ProgressReporter,
    ) -> Result<Vec<Commit>, Error> {
        let t0 = std::time::Instant::now();
        let repo = gix::open(repo_path).map_err(|e| Error::GitCommand {
            repo_path: repo_path.to_path_buf(),
            stderr: e.to_string(),
        })?;

        let tip = repo.rev_parse_single(branch).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("could not be found")
                || msg.contains("couldn't parse revision")
                || msg.contains("ambiguous")
            {
                Error::ReferenceNotFound(msg)
            } else {
                Error::GitCommand {
                    repo_path: repo_path.to_path_buf(),
                    stderr: msg,
                }
            }
        })?;

        let mut walk = repo
            .rev_walk(Some(tip))
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .all()
            .map_err(|e| Error::GitCommand {
                repo_path: repo_path.to_path_buf(),
                stderr: e.to_string(),
            })?;

        let mut commits = Vec::new();
        progress.start(
            options.max_count.map(|n| n as u64),
            &format!("git log {}", branch),
        );

        for info in &mut walk {
            let info = info.map_err(|e| Error::GitCommand {
                repo_path: repo_path.to_path_buf(),
                stderr: e.to_string(),
            })?;

            let commit = repo
                .find_commit(info.id)
                .expect("commit object should exist after walk validation");

            let decoded = commit
                .decode()
                .expect("commit object should be decodable after walk validation");

            let committer = decoded
                .committer()
                .map_err(|e| Error::Parse(format!("invalid committer: {}", e)))?;
            let timestamp_secs = committer
                .time()
                .map_err(|e| Error::Parse(format!("invalid committer time: {}", e)))?
                .seconds;
            let timestamp = Utc
                .timestamp_opt(timestamp_secs, 0)
                .single()
                .ok_or_else(|| {
                    Error::TimestampParse(format!("invalid timestamp: {}", timestamp_secs))
                })?;

            // Apply since / until filters (using committer time, matching `git log`).
            if let Some(since) = options.since {
                if timestamp_secs < since.timestamp() {
                    continue;
                }
            }
            if let Some(until) = options.until {
                if timestamp_secs > until.timestamp() {
                    continue;
                }
            }

            let author = decoded
                .author()
                .map_err(|e| Error::Parse(format!("invalid author: {}", e)))?;
            let author_name = std::str::from_utf8(author.name)
                .map_err(|e| Error::Parse(format!("invalid UTF-8 in author name: {}", e)))?
                .to_string();
            let author_email = std::str::from_utf8(author.email)
                .map_err(|e| Error::Parse(format!("invalid UTF-8 in author email: {}", e)))?
                .to_string();

            let full_msg = std::str::from_utf8(decoded.message)
                .map_err(|e| Error::Parse(format!("invalid UTF-8 in commit message: {}", e)))?
                .trim_end();
            let mut lines = full_msg.lines();
            let subject = lines.next().unwrap_or("").to_string();
            let remaining: Vec<_> = lines.collect();
            let body = remaining
                .iter()
                .position(|l| l.is_empty())
                .map(|pos| remaining[pos + 1..].join("\n"));

            let parents: Vec<String> = decoded.parents().map(|id| id.to_string()).collect();
            let hash = commit.id.to_string();

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

        // `git log --reverse` collects the full list first, then reverses it.
        if options.reverse {
            commits.reverse();
        }

        let skip = options.skip.unwrap_or(0);
        let mut commits: Vec<_> = commits.into_iter().skip(skip).collect();

        if let Some(max_count) = options.max_count {
            commits.truncate(max_count);
        }

        debug!(
            commits = commits.len(),
            elapsed_ms = t0.elapsed().as_millis(),
            "gix log completed"
        );
        progress.finish();
        Ok(commits)
    }

    fn branches(
        &self,
        repo_path: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<Vec<String>, Error> {
        let repo = gix::open(repo_path).map_err(|e| Error::GitCommand {
            repo_path: repo_path.to_path_buf(),
            stderr: e.to_string(),
        })?;

        progress.start(None, "listing branches");

        let references = repo.references().map_err(|e| Error::GitCommand {
            repo_path: repo_path.to_path_buf(),
            stderr: e.to_string(),
        })?;

        let mut branches = Vec::new();
        for reference in references.all().map_err(|e| Error::GitCommand {
            repo_path: repo_path.to_path_buf(),
            stderr: e.to_string(),
        })? {
            let reference = reference.map_err(|e| Error::GitCommand {
                repo_path: repo_path.to_path_buf(),
                stderr: e.to_string(),
            })?;

            let name = reference.name().as_bstr();
            if let Some(local_branch) = name.strip_prefix(b"refs/heads/") {
                if let Ok(s) = std::str::from_utf8(local_branch) {
                    branches.push(s.to_string());
                }
            } else if let Some(remote_branch) = name.strip_prefix(b"refs/remotes/") {
                if let Ok(s) = std::str::from_utf8(remote_branch) {
                    if s == "HEAD" || s.contains("/HEAD") {
                        continue;
                    }
                    if let Some((_, branch)) = s.split_once('/') {
                        branches.push(branch.to_string());
                    } else {
                        branches.push(s.to_string());
                    }
                }
            }
        }

        branches.sort();
        branches.dedup();
        progress.finish();
        Ok(branches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn gix_provider_new_and_default() {
        let _ = GixProvider::new();
        let _ = GixProvider::default();
    }

    #[test]
    fn log_fails_for_non_git_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = GixProvider::new();
        let result = provider.log(
            tmp.path(),
            "main",
            LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
    }

    #[test]
    fn log_reference_not_found_for_missing_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();

        let provider = GixProvider::new();
        let result = provider.log(
            repo,
            "nonexistent",
            LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("could not be found")
                || err_str.contains("couldn't parse revision")
                || err_str.contains("not found")
        );
    }

    #[test]
    fn log_filters_since_until_and_reverse_skip_max_count() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "first"])
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "b").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "second"])
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "c").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "third"])
            .status()
            .unwrap();

        let provider = GixProvider::new();

        // Test reverse + skip + max_count
        let opts = LogOptions {
            reverse: true,
            skip: Some(1),
            max_count: Some(1),
            ..Default::default()
        };
        let commits = provider
            .log(repo, "HEAD", opts, &crate::progress::NoProgress)
            .unwrap();
        assert_eq!(commits.len(), 1);
        // In reverse order after skip(1) and truncate(1), should be "second"
        assert_eq!(commits[0].subject, "second");

        // Test since filtering (exclude very old commits)
        let future = Utc.with_ymd_and_hms(3000, 1, 1, 0, 0, 0).unwrap();
        let opts_since = LogOptions {
            since: Some(future),
            ..Default::default()
        };
        let commits_since = provider
            .log(repo, "HEAD", opts_since, &crate::progress::NoProgress)
            .unwrap();
        assert!(commits_since.is_empty());

        // Test until filtering (exclude commits after a very old date)
        let past = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
        let opts_until = LogOptions {
            until: Some(past),
            ..Default::default()
        };
        let commits_until = provider
            .log(repo, "HEAD", opts_until, &crate::progress::NoProgress)
            .unwrap();
        assert!(commits_until.is_empty());
    }

    #[test]
    fn log_body_empty_after_blank_line() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();

        // Craft a commit object with message "subject\n\n\n" directly so git
        // does not normalise trailing blank lines away.
        let tree_output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["write-tree"])
            .output()
            .unwrap();
        let tree_hash = String::from_utf8(tree_output.stdout)
            .unwrap()
            .trim()
            .to_string();
        let commit_data = format!(
            "tree {}\nauthor Test <test@example.com> 1700000000 +0000\ncommitter Test <test@example.com> 1700000000 +0000\n\nsubject\n\n\n",
            tree_hash
        );
        std::fs::write(repo.join("commit.txt"), &commit_data).unwrap();
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["hash-object", "-t", "commit", "-w", "commit.txt"])
            .output()
            .unwrap();
        let commit_hash = String::from_utf8(output.stdout).unwrap().trim().to_string();
        std::fs::write(repo.join(".git/refs/heads/main"), &commit_hash).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();

        let provider = GixProvider::new();
        let commits = provider
            .log(
                repo,
                "HEAD",
                LogOptions::default(),
                &crate::progress::NoProgress,
            )
            .unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "subject");
        assert_eq!(commits[0].body, None);
    }

    #[test]
    fn log_rev_parse_corrupt_head() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        // Corrupt HEAD so rev_parse_single fails.
        std::fs::write(repo.join(".git/HEAD"), "garbage\n").unwrap();

        let provider = GixProvider::new();
        let result = provider.log(
            repo,
            "HEAD",
            LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
    }

    #[test]
    fn log_rev_walk_blob_tip() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("blob.txt"), "hello").unwrap();
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["hash-object", "-w", "blob.txt"])
            .output()
            .unwrap();
        let blob_hash = String::from_utf8(output.stdout).unwrap().trim().to_string();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["tag", "blobtag", &blob_hash])
            .status()
            .unwrap();

        let provider = GixProvider::new();
        let result = provider.log(
            repo,
            "blobtag",
            LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
    }

    #[test]
    fn log_timestamp_out_of_range() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();

        // Get tree hash.
        let tree_output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["write-tree"])
            .output()
            .unwrap();
        let tree_hash = String::from_utf8(tree_output.stdout)
            .unwrap()
            .trim()
            .to_string();

        // Create a commit object with an out-of-range timestamp.
        let commit_data = format!(
            "tree {}\nauthor Test <test@example.com> 999999999999999 +0000\ncommitter Test <test@example.com> 999999999999999 +0000\n\nsubject\n",
            tree_hash
        );
        std::fs::write(repo.join("commit.txt"), &commit_data).unwrap();
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["hash-object", "-t", "commit", "-w", "commit.txt"])
            .output()
            .unwrap();
        let commit_hash = String::from_utf8(output.stdout).unwrap().trim().to_string();

        // Point HEAD to the bad commit.
        std::fs::write(repo.join(".git/refs/heads/main"), &commit_hash).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();

        let provider = GixProvider::new();
        let result = provider.log(
            repo,
            "main",
            LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("timestamp") || err_str.contains("Timestamp"));
    }

    #[test]
    fn log_missing_commit_object() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();

        // Get the commit hash and delete the object file.
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let commit_hash = String::from_utf8(output.stdout).unwrap().trim().to_string();
        let obj_dir = &commit_hash[..2];
        let obj_file = &commit_hash[2..];
        let obj_path = repo.join(".git/objects").join(obj_dir).join(obj_file);
        std::fs::remove_file(&obj_path).unwrap();

        let provider = GixProvider::new();
        let result = provider.log(
            repo,
            "HEAD",
            LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
    }

    #[test]
    fn log_missing_parent_object() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();

        // Commit A
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "first"])
            .status()
            .unwrap();

        // Commit B (child of A)
        std::fs::write(repo.join("a.txt"), "b").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "second"])
            .status()
            .unwrap();

        // Delete the FIRST commit's object file (the parent).
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["rev-parse", "HEAD~1"])
            .output()
            .unwrap();
        let parent_hash = String::from_utf8(output.stdout).unwrap().trim().to_string();
        let obj_dir = &parent_hash[..2];
        let obj_file = &parent_hash[2..];
        let obj_path = repo.join(".git/objects").join(obj_dir).join(obj_file);
        std::fs::remove_file(&obj_path).unwrap();

        let provider = GixProvider::new();
        let result = provider.log(
            repo,
            "HEAD",
            LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
    }

    #[test]
    fn log_decode_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .arg("init")
            .arg(repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(repo)
            .args(["add", "."])
            .status()
            .unwrap();

        let tree_output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["write-tree"])
            .output()
            .unwrap();
        let tree_hash = String::from_utf8(tree_output.stdout)
            .unwrap()
            .trim()
            .to_string();

        // Create a commit object with malformed content that hash-object will
        // store but gix may fail to decode.
        let commit_data = format!(
            "tree {}\nauthor Test <test@example.com> not-a-number +0000\ncommitter Test <test@example.com> not-a-number +0000\n\nsubject\n",
            tree_hash
        );
        std::fs::write(repo.join("commit.txt"), &commit_data).unwrap();
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["hash-object", "-t", "commit", "-w", "commit.txt"])
            .output()
            .unwrap();
        let commit_hash = String::from_utf8(output.stdout).unwrap().trim().to_string();
        std::fs::write(repo.join(".git/refs/heads/main"), &commit_hash).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();

        let provider = GixProvider::new();
        let result = provider.log(
            repo,
            "HEAD",
            LogOptions::default(),
            &crate::progress::NoProgress,
        );
        assert!(result.is_err());
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

        let provider = GixProvider::new();
        let branches = provider
            .branches(&clone, &crate::progress::NoProgress)
            .unwrap();
        assert!(branches.contains(&"master".to_string()));
        assert!(branches.contains(&"feature-x".to_string()));
    }

    #[test]
    fn branches_fails_for_non_git_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = GixProvider::new();
        let result = provider.branches(tmp.path(), &crate::progress::NoProgress);
        assert!(result.is_err());
    }
}
