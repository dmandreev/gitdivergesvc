#![allow(dead_code)]

pub mod auth;
pub mod config;
pub mod daemon;
pub mod demo;
pub mod divergence_cache;
pub mod logging;

use anyhow::{bail, Context, Result};
use gitdiverge_lib::{
    analyze_branch_divergence, Commit, GitProvider, GixProvider, LogOptions, NoProgress,
    ProcessGitProvider, ProgressReporter, Runtime, Task,
};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct OutputEntry {
    pub branch: String,
    pub commits: Vec<Commit>,
    pub error: Option<String>,
}

pub struct IndicatifProgress {
    pub bar: ProgressBar,
}

impl IndicatifProgress {
    pub fn new(bar: ProgressBar) -> Self {
        bar.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}",
                )
                .unwrap()
                .progress_chars("#>-"),
        );
        Self { bar }
    }
}

impl ProgressReporter for IndicatifProgress {
    fn start(&self, total: Option<u64>, message: &str) {
        if let Some(t) = total {
            self.bar.set_length(t);
        }
        self.bar.set_message(message.to_string());
        self.bar.enable_steady_tick(Duration::from_millis(100));
    }

    fn advance(&self, amount: u64) {
        self.bar.inc(amount);
    }

    fn finish(&self) {
        self.bar.finish_with_message("done");
    }

    fn message(&self, message: &str) {
        self.bar.set_message(message.to_string());
    }
}

pub fn build_overall_progress(mp: &MultiProgress, len: usize) -> ProgressBar {
    let overall = mp.add(ProgressBar::new(len as u64));
    overall.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] {wide_msg} {pos}/{len}")
            .unwrap(),
    );
    overall.set_message("querying branches");
    overall
}

pub fn run_multi_branch(
    git: &dyn GitProvider,
    repo: &Path,
    branches: &[String],
    options: LogOptions,
    progress: &dyn ProgressReporter,
) -> Vec<OutputEntry> {
    let results = gitdiverge_lib::log_parallel(git, repo, branches, options, progress);
    branches
        .iter()
        .zip(results)
        .map(|(branch, result)| match result {
            Ok(commits) => OutputEntry {
                branch: branch.clone(),
                commits,
                error: None,
            },
            Err(e) => OutputEntry {
                branch: branch.clone(),
                commits: Vec::new(),
                error: Some(e.to_string()),
            },
        })
        .collect()
}

pub fn format_entries(entries: &[OutputEntry], json: bool) -> Result<String> {
    if json {
        Ok(serde_json::to_string_pretty(entries)?)
    } else {
        let mut out = String::new();
        for entry in entries {
            out.push_str(&format!("== branch: {} ==\n", entry.branch));
            if let Some(ref err) = entry.error {
                out.push_str(&format!("ERROR: {}\n", err));
                continue;
            }
            for c in &entry.commits {
                out.push_str(&format!(
                    "  {}  {}  {}\n",
                    &c.hash[..7],
                    c.subject,
                    c.author.name
                ));
            }
        }
        Ok(out)
    }
}

pub fn format_commits(commits: &[Commit], json: bool) -> Result<String> {
    if json {
        Ok(serde_json::to_string_pretty(commits)?)
    } else {
        let mut out = String::new();
        for c in commits {
            out.push_str(&format!("commit {}\n", c.hash));
            out.push_str(&format!("Author: {} <{}>\n", c.author.name, c.author.email));
            out.push_str(&format!(
                "Date:   {}\n",
                c.timestamp.format("%a %b %e %H:%M:%S %Y %z")
            ));
            out.push('\n');
            out.push_str(&format!("    {}\n", c.subject));
            if let Some(ref body) = c.body {
                out.push('\n');
                for line in body.lines() {
                    out.push_str(&format!("    {}\n", line));
                }
            }
            out.push('\n');
        }
        Ok(out)
    }
}

pub fn create_progress_bar(mp: &MultiProgress, count: Option<usize>) -> ProgressBar {
    mp.add(ProgressBar::new(count.unwrap_or(0) as u64))
}

pub fn run_single_branch(
    runtime: &Runtime,
    repo: std::path::PathBuf,
    branch: String,
    options: LogOptions,
    progress: Arc<dyn ProgressReporter>,
    ctrlc_rx: &crossbeam_channel::Receiver<()>,
) -> Result<Vec<Commit>> {
    let handle = runtime.handle();
    let (result_tx, result_rx) = crossbeam_channel::bounded(1);
    handle.submit(Task::Log {
        repo,
        branch,
        options,
        progress,
        respond: result_tx,
    });

    crossbeam_channel::select! {
        recv(result_rx) -> msg => {
            match msg {
                Ok(Ok(commits)) => Ok(commits),
                Ok(Err(e)) => Err(e.into()),
                Err(_) => {
                    tracing::warn!("result channel closed unexpectedly");
                    Ok(Vec::new())
                }
            }
        }
        recv(ctrlc_rx) -> _ => {
            tracing::info!("shutting down before result arrived");
            Ok(Vec::new())
        }
    }
}

/// Orchestrate the full branch-analytics workflow as three explicit phases:
///
/// 1. Clone (if missing).
/// 2. Fetch updates and collect branch existence information.
/// 3. Checkout valid branches and run divergence analytics.
pub fn run_branch_analytics(
    url: &str,
    repo_path: &Path,
    repo_guid: &str,
    branches: &[String],
    output: &Path,
) -> Result<()> {
    let process_git = ProcessGitProvider::new();

    // Phase 1: Clone
    if !repo_path.join(".git").is_dir() {
        if let Some(parent) = repo_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        process_git
            .clone_repo(url, repo_path, &NoProgress)
            .with_context(|| format!("failed to clone {}", url))?;
    }

    // Phase 2: Fetch + branch info
    process_git
        .fetch(repo_path, &NoProgress)
        .with_context(|| "failed to fetch from origin")?;

    let mut branch_statuses = Vec::new();
    let mut valid_branches = Vec::new();
    for branch in branches {
        match process_git.branch_exists_on_remote(repo_path, branch) {
            Ok(true) => {
                valid_branches.push(branch.clone());
                branch_statuses.push(gitdiverge_lib::BranchStatus {
                    branch: branch.clone(),
                    exists: true,
                });
            }
            Ok(false) => {
                tracing::warn!(branch = %branch, "branch does not exist on remote, skipping");
                branch_statuses.push(gitdiverge_lib::BranchStatus {
                    branch: branch.clone(),
                    exists: false,
                });
            }
            Err(e) => {
                tracing::error!(branch = %branch, error = %e, "failed to check branch on remote");
                branch_statuses.push(gitdiverge_lib::BranchStatus {
                    branch: branch.clone(),
                    exists: false,
                });
            }
        }
    }

    if valid_branches.is_empty() {
        bail!("none of the requested branches exist on the remote");
    }

    for branch in &valid_branches {
        process_git
            .checkout_branch(repo_path, branch, &NoProgress)
            .with_context(|| format!("failed to update branch {}", branch))?;
    }

    // Phase 3: Analytics
    let gix = GixProvider::new();
    let analytics = analyze_branch_divergence(&gix, repo_path, &valid_branches, &NoProgress)
        .with_context(|| "branch divergence analysis failed")?;

    let result = gitdiverge_lib::RepoAnalytics {
        repo_name: repo_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        repo_url: url.to_string(),
        repo_guid: repo_guid.to_string(),
        repo_path: repo_path.to_path_buf(),
        branch_statuses,
        analytics: Ok(analytics),
    };

    let json = serde_json::to_string_pretty(&result)?;
    std::fs::write(output, json)
        .with_context(|| format!("failed to write {}", output.display()))?;

    Ok(())
}

/// Orchestrate batch branch-analytics workflow for multiple repositories.
pub fn run_batch_branch_analytics(
    repos: &[gitdiverge_lib::RepoSpec],
    branches: &[String],
    clone_dir: &Path,
    output: &Path,
) -> Result<()> {
    let mut index = gitdiverge_lib::RepoIndex::open(clone_dir)?;

    let mp = MultiProgress::new();
    let overall = mp.add(ProgressBar::new(0));
    let progress = Arc::new(IndicatifProgress::new(overall));

    let results =
        gitdiverge_lib::run_batch_analytics(repos, branches, &mut index, output, &*progress)
            .with_context(|| "batch analytics workflow failed")?;

    index.save()?;

    let success = results.iter().filter(|r| r.analytics.is_ok()).count();
    let failed = results.iter().filter(|r| r.analytics.is_err()).count();
    tracing::debug!(
        success,
        failed,
        output = %output.display(),
        "batch analytics complete"
    );

    mp.clear()?;
    Ok(())
}

pub fn run_multi(
    git: &dyn GitProvider,
    repo: &Path,
    branches: &[String],
    count: Option<usize>,
    json: bool,
    no_progress: bool,
) -> Result<String> {
    let mp = MultiProgress::new();
    let options = LogOptions {
        max_count: count,
        ..LogOptions::default()
    };
    let overall = build_overall_progress(&mp, branches.len());
    let progress: Arc<dyn ProgressReporter> = if no_progress {
        Arc::new(NoProgress)
    } else {
        Arc::new(IndicatifProgress::new(overall))
    };
    let entries = run_multi_branch(git, repo, branches, options, &*progress);
    let output = format_entries(&entries, json)?;
    mp.clear()?;
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
pub fn run_single(
    git: Arc<dyn GitProvider>,
    repo: std::path::PathBuf,
    branch: String,
    count: Option<usize>,
    json: bool,
    no_progress: bool,
    workers: usize,
    ctrlc_rx: &crossbeam_channel::Receiver<()>,
) -> Result<String> {
    let mp = MultiProgress::new();
    let runtime = Runtime::new(workers, git);
    let bar = create_progress_bar(&mp, count);
    let progress: Arc<dyn ProgressReporter> = if no_progress {
        Arc::new(NoProgress)
    } else {
        Arc::new(IndicatifProgress::new(bar))
    };
    let result = run_single_branch(
        &runtime,
        repo,
        branch,
        LogOptions {
            max_count: count,
            ..LogOptions::default()
        },
        progress,
        ctrlc_rx,
    );
    let output = match result {
        Ok(commits) => format_commits(&commits, json)?,
        Err(e) => {
            tracing::error!(error = %e, "git operation failed");
            return Err(e);
        }
    };
    runtime.shutdown();
    mp.clear()?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use gitdiverge_lib::{Author, Commit, Error, GitProvider, LogOptions};
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
    fn indicatif_progress_methods() {
        let mp = MultiProgress::new();
        let bar = mp.add(ProgressBar::new(10));
        let progress = IndicatifProgress::new(bar);
        progress.start(Some(100), "testing");
        progress.advance(5);
        progress.finish();
    }

    #[test]
    fn run_multi_branch_success() {
        let git = MockGitProvider::default()
            .with_response("main", Ok(vec![make_commit("abc", "First")]))
            .with_response("dev", Ok(vec![make_commit("def", "Second")]));
        let entries = run_multi_branch(
            &git,
            Path::new("."),
            &["main".to_string(), "dev".to_string()],
            LogOptions::default(),
            &gitdiverge_lib::NoProgress,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].branch, "main");
        assert_eq!(entries[0].commits.len(), 1);
        assert_eq!(entries[1].branch, "dev");
    }

    #[test]
    fn run_multi_branch_error() {
        let git = MockGitProvider::default()
            .with_response("main", Ok(vec![make_commit("abc", "First")]))
            .with_response("bad", Err("not found".to_string()));
        let entries = run_multi_branch(
            &git,
            Path::new("."),
            &["main".to_string(), "bad".to_string()],
            LogOptions::default(),
            &gitdiverge_lib::NoProgress,
        );
        assert_eq!(
            entries[1].error,
            Some("reference not found: not found".to_string())
        );
    }

    #[test]
    fn format_entries_json() {
        let entries = vec![OutputEntry {
            branch: "main".to_string(),
            commits: vec![make_commit("abc", "First")],
            error: None,
        }];
        let json = format_entries(&entries, true).unwrap();
        assert!(json.contains("main"));
        assert!(json.contains("First"));
    }

    #[test]
    fn format_entries_text() {
        let entries = vec![OutputEntry {
            branch: "main".to_string(),
            commits: vec![make_commit("abc1234", "First")],
            error: None,
        }];
        let text = format_entries(&entries, false).unwrap();
        assert!(text.contains("== branch: main =="));
        assert!(text.contains("abc1234"));
        assert!(text.contains("First"));
    }

    #[test]
    fn format_entries_text_with_error() {
        let entries = vec![OutputEntry {
            branch: "bad".to_string(),
            commits: vec![],
            error: Some("oops".to_string()),
        }];
        let text = format_entries(&entries, false).unwrap();
        assert!(text.contains("ERROR: oops"));
    }

    #[test]
    fn format_commits_json() {
        let commits = vec![make_commit("abc", "First")];
        let json = format_commits(&commits, true).unwrap();
        assert!(json.contains("abc"));
        assert!(json.contains("First"));
    }

    #[test]
    fn format_commits_text() {
        let commits = vec![make_commit("abc1234", "First")];
        let text = format_commits(&commits, false).unwrap();
        assert!(text.contains("commit abc1234"));
        assert!(text.contains("Author: Test <test@example.com>"));
        assert!(text.contains("First"));
    }

    #[test]
    fn format_commits_text_with_body() {
        let mut commit = make_commit("abc", "Subject");
        commit.body = Some("Line one\nLine two".to_string());
        let text = format_commits(&[commit], false).unwrap();
        assert!(text.contains("Subject"));
        assert!(text.contains("Line one"));
        assert!(text.contains("Line two"));
    }

    #[test]
    fn build_overall_progress_creates_bar() {
        let mp = MultiProgress::new();
        let bar = build_overall_progress(&mp, 5);
        assert_eq!(bar.length(), Some(5));
    }

    #[test]
    fn create_progress_bar_with_count() {
        let mp = MultiProgress::new();
        let bar = create_progress_bar(&mp, Some(10));
        assert_eq!(bar.length(), Some(10));
    }

    #[test]
    fn create_progress_bar_without_count() {
        let mp = MultiProgress::new();
        let bar = create_progress_bar(&mp, None);
        assert_eq!(bar.length(), Some(0));
    }

    #[test]
    fn run_single_branch_success() {
        let git = Arc::new(
            MockGitProvider::default().with_response("main", Ok(vec![make_commit("abc", "First")])),
        );
        let runtime = Runtime::new(1, git);
        let (_ctrlc_tx, ctrlc_rx) = crossbeam_channel::bounded(1);
        let progress: Arc<dyn ProgressReporter> = Arc::new(gitdiverge_lib::NoProgress);
        let result = run_single_branch(
            &runtime,
            std::path::PathBuf::from("."),
            "main".to_string(),
            LogOptions::default(),
            progress,
            &ctrlc_rx,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
        runtime.shutdown();
    }

    #[test]
    fn run_single_branch_error_from_git() {
        let git =
            Arc::new(MockGitProvider::default().with_response("main", Err("boom".to_string())));
        let runtime = Runtime::new(1, git);
        let (_ctrlc_tx, ctrlc_rx) = crossbeam_channel::bounded(1);
        let progress: Arc<dyn ProgressReporter> = Arc::new(gitdiverge_lib::NoProgress);
        let result = run_single_branch(
            &runtime,
            std::path::PathBuf::from("."),
            "main".to_string(),
            LogOptions::default(),
            progress,
            &ctrlc_rx,
        );
        assert!(result.is_err());
        runtime.shutdown();
    }

    #[test]
    fn run_single_branch_ctrlc() {
        let git = Arc::new(
            MockGitProvider::default().with_response("main", Ok(vec![make_commit("abc", "First")])),
        );
        let runtime = Runtime::new(1, git);
        let (ctrlc_tx, ctrlc_rx) = crossbeam_channel::bounded(1);
        let progress: Arc<dyn ProgressReporter> = Arc::new(gitdiverge_lib::NoProgress);
        ctrlc_tx.send(()).unwrap();
        let result = run_single_branch(
            &runtime,
            std::path::PathBuf::from("."),
            "main".to_string(),
            LogOptions::default(),
            progress,
            &ctrlc_rx,
        );
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
        runtime.shutdown();
    }

    #[test]
    fn run_multi_success() {
        let git = MockGitProvider::default()
            .with_response("main", Ok(vec![make_commit("abc1234", "First")]));
        let output = run_multi(
            &git,
            Path::new("."),
            &["main".to_string()],
            None,
            false,
            true,
        )
        .unwrap();
        assert!(output.contains("First"));
        assert!(output.contains("abc1234"));
    }

    #[test]
    fn run_multi_with_progress_bar() {
        let git = MockGitProvider::default()
            .with_response("main", Ok(vec![make_commit("abc1234", "First")]));
        let output = run_multi(
            &git,
            Path::new("."),
            &["main".to_string()],
            None,
            false,
            false,
        )
        .unwrap();
        assert!(output.contains("First"));
    }

    #[test]
    fn run_multi_json() {
        let git =
            MockGitProvider::default().with_response("main", Ok(vec![make_commit("abc", "First")]));
        let output = run_multi(
            &git,
            Path::new("."),
            &["main".to_string()],
            None,
            true,
            true,
        )
        .unwrap();
        assert!(output.contains("First"));
    }

    #[test]
    fn run_single_success() {
        let git = Arc::new(
            MockGitProvider::default()
                .with_response("main", Ok(vec![make_commit("abc1234", "First")])),
        );
        let (_ctrlc_tx, ctrlc_rx) = crossbeam_channel::bounded(1);
        let output = run_single(
            git,
            std::path::PathBuf::from("."),
            "main".to_string(),
            None,
            false,
            true,
            1,
            &ctrlc_rx,
        )
        .unwrap();
        assert!(output.contains("First"));
        assert!(output.contains("abc1234"));
    }

    #[test]
    fn run_single_with_progress_bar() {
        let git = Arc::new(
            MockGitProvider::default().with_response("main", Ok(vec![make_commit("abc", "First")])),
        );
        let (_ctrlc_tx, ctrlc_rx) = crossbeam_channel::bounded(1);
        let output = run_single(
            git,
            std::path::PathBuf::from("."),
            "main".to_string(),
            None,
            false,
            false,
            1,
            &ctrlc_rx,
        )
        .unwrap();
        assert!(output.contains("First"));
    }

    #[test]
    fn run_single_json() {
        let git = Arc::new(
            MockGitProvider::default().with_response("main", Ok(vec![make_commit("abc", "First")])),
        );
        let (_ctrlc_tx, ctrlc_rx) = crossbeam_channel::bounded(1);
        let output = run_single(
            git,
            std::path::PathBuf::from("."),
            "main".to_string(),
            None,
            true,
            true,
            1,
            &ctrlc_rx,
        )
        .unwrap();
        assert!(output.contains("First"));
    }

    #[test]
    fn run_single_branch_channel_closed() {
        let git = Arc::new(
            MockGitProvider::default().with_response("main", Ok(vec![make_commit("abc", "First")])),
        );
        // 0 workers means the runtime channel is disconnected immediately,
        // so the submitted task (and its result_tx) is dropped.
        let runtime = Runtime::new(0, git);
        let (_ctrlc_tx, ctrlc_rx) = crossbeam_channel::bounded(1);
        let progress: Arc<dyn ProgressReporter> = Arc::new(gitdiverge_lib::NoProgress);
        let result = run_single_branch(
            &runtime,
            std::path::PathBuf::from("."),
            "main".to_string(),
            LogOptions::default(),
            progress,
            &ctrlc_rx,
        );
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
        runtime.shutdown();
    }

    #[test]
    fn run_single_error_from_git() {
        let git =
            Arc::new(MockGitProvider::default().with_response("main", Err("boom".to_string())));
        let (_ctrlc_tx, ctrlc_rx) = crossbeam_channel::bounded(1);
        let result = run_single(
            git,
            std::path::PathBuf::from("."),
            "main".to_string(),
            None,
            false,
            true,
            1,
            &ctrlc_rx,
        );
        assert!(result.is_err());
    }
}
