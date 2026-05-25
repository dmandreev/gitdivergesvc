use crate::error::Error;
use crate::git::ProcessGitProvider;
use crate::progress::ProgressReporter;
use crate::{analyze_branch_divergence, BranchAnalytics, GixProvider, NoProgress, RepoIndex};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Specification of a repository to process.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RepoSpec {
    /// Git remote URL. Supported formats include HTTPS and SSH.
    pub url: String,
    /// Human-readable name used for display and local directory naming.
    pub name: String,
}

/// Result of Phase 1: Clone / initial fetch.
#[derive(Debug)]
pub struct ClonePhaseResult {
    pub repo_name: String,
    pub repo_url: String,
    pub repo_guid: String,
    pub repo_path: PathBuf,
    /// `true` if the repository was freshly cloned, `false` if it already existed.
    pub cloned: bool,
    pub result: Result<(), Error>,
}

/// Status of a requested branch after checking the remote.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, utoipa::ToSchema)]
pub struct BranchStatus {
    /// Name of the branch that was checked.
    pub branch: String,
    /// `true` if `origin/<branch>` exists on the remote, `false` otherwise.
    pub exists: bool,
}

/// Result of Phase 2: Fetch updates and determine branch existence.
#[derive(Debug)]
pub struct FetchPhaseResult {
    pub repo_name: String,
    pub repo_url: String,
    pub repo_guid: String,
    pub repo_path: PathBuf,
    pub fetch_result: Result<(), Error>,
    pub branch_statuses: Vec<BranchStatus>,
    pub valid_branches: Vec<String>,
    pub checkout_result: Result<(), Error>,
}

/// Analytics result for a single repository (Phase 3).
#[derive(Serialize, Deserialize, Debug, Clone, utoipa::ToSchema)]
pub struct RepoAnalytics {
    pub repo_name: String,
    pub repo_url: String,
    pub repo_guid: String,
    pub repo_path: PathBuf,
    pub branch_statuses: Vec<BranchStatus>,
    pub analytics: Result<BranchAnalytics, String>,
}

/// Read a repositories file.
/// Lines starting with `#` or empty lines are ignored.
/// Each line may be either:
/// - `https://host/owner/repo.git` (name derived from URL)
/// - `my-name https://host/owner/repo.git` (explicit clone directory name)
pub fn read_repositories_file(path: &Path) -> Result<Vec<RepoSpec>, Error> {
    let content = std::fs::read_to_string(path).map_err(Error::Io)?;
    let mut repos = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let (name, url) = if parts.len() >= 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            let url = parts[0].to_string();
            let name = repo_name_from_url(&url);
            (name, url)
        };
        repos.push(RepoSpec { url, name });
    }
    Ok(repos)
}

/// Read a branches file.
/// Lines starting with `#` or empty lines are ignored.
pub fn read_branches_file(path: &Path) -> Result<Vec<String>, Error> {
    let content = std::fs::read_to_string(path).map_err(Error::Io)?;
    let mut branches = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        branches.push(line.to_string());
    }
    Ok(branches)
}

fn repo_name_from_url(url: &str) -> String {
    let name = url.rsplit('/').next().unwrap_or("repo");
    if name.is_empty() {
        "repo".to_string()
    } else {
        name.trim_end_matches(".git").to_string()
    }
}

/// Phase 1: Clone repositories that do not exist locally.
///
/// Repositories that already have a `.git` directory are left untouched in this
/// phase (Phase 2 will fetch them).
pub fn run_clone_phase(
    git: &ProcessGitProvider,
    repos: &[RepoSpec],
    index: &mut RepoIndex,
    max_workers: usize,
    progress: &dyn ProgressReporter,
) -> Vec<ClonePhaseResult> {
    if let Err(e) = std::fs::create_dir_all(index.clone_dir()) {
        tracing::error!("failed to create clone directory: {e}");
    }

    // Pre-resolve all URLs to paths using the index (single-threaded).
    let resolved: Vec<(RepoSpec, String, PathBuf)> = repos
        .iter()
        .map(|spec| {
            let (guid, name) = {
                let entry = index.get_or_insert(spec.url.clone(), spec.name.clone());
                (entry.guid.clone(), entry.name.clone())
            };
            let path = index.clone_dir().join(&guid).join(&name);
            (spec.clone(), guid, path)
        })
        .collect();

    // Persist index so that GUIDs are recorded even if cloning is interrupted.
    if let Err(e) = index.save() {
        tracing::error!("failed to save repo index: {e}");
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(max_workers)
        .build()
        .expect("failed to build clone thread pool");

    progress.start(
        Some(repos.len() as u64 * 2),
        "phase 1/3: cloning repositories",
    );

    let results: Vec<ClonePhaseResult> = pool.install(|| {
        resolved
            .into_par_iter()
            .map(|(spec, guid, repo_path)| {
                let (cloned, result) = if !repo_path.join(".git").is_dir() {
                    progress.advance(1);
                    let res = git.clone_repo(&spec.url, &repo_path, &NoProgress);
                    progress.message(&spec.name);
                    progress.advance(1);
                    (true, res)
                } else {
                    progress.advance(2);
                    (false, Ok(()))
                };
                ClonePhaseResult {
                    repo_name: spec.name,
                    repo_url: spec.url,
                    repo_guid: guid,
                    repo_path,
                    cloned,
                    result,
                }
            })
            .collect()
    });

    progress.finish();
    results
}

/// Phase 2: Fetch updates and collect information about existing / non-existing branches.
///
/// For each repository that passed Phase 1, this function:
/// 1. Fetches from `origin`.
/// 2. Checks every requested branch against `origin/<branch>`.
/// 3. Records which branches exist and which do not.
/// 4. Checks out all valid branches so local refs match the remote.
pub fn run_fetch_phase(
    git: &ProcessGitProvider,
    clone_results: &[ClonePhaseResult],
    branches: &[String],
    progress: &dyn ProgressReporter,
) -> Vec<FetchPhaseResult> {
    let ready: Vec<&ClonePhaseResult> = clone_results.iter().filter(|r| r.result.is_ok()).collect();

    if ready.is_empty() {
        return Vec::new();
    }

    progress.start(
        Some(ready.len() as u64 * 2),
        "phase 2/3: fetching updates and checking branches",
    );

    let results: Vec<FetchPhaseResult> = ready
        .into_par_iter()
        .map(|clone_result| {
            let name = clone_result.repo_name.clone();
            let url = clone_result.repo_url.clone();
            let guid = clone_result.repo_guid.clone();
            let path = clone_result.repo_path.clone();

            progress.advance(1);
            let fetch_result = git.fetch(&path, &NoProgress);

            let mut branch_statuses = Vec::new();
            let mut valid_branches = Vec::new();

            for branch in branches {
                let exists = match git.branch_exists_on_remote(&path, branch) {
                    Ok(true) => {
                        valid_branches.push(branch.clone());
                        true
                    }
                    Ok(false) => {
                        tracing::warn!(
                            repo = %name,
                            branch = %branch,
                            "branch does not exist on remote"
                        );
                        false
                    }
                    Err(e) => {
                        tracing::error!(
                            repo = %name,
                            branch = %branch,
                            error = %e,
                            "branch check failed"
                        );
                        false
                    }
                };
                branch_statuses.push(BranchStatus {
                    branch: branch.clone(),
                    exists,
                });
            }

            let checkout_result = if valid_branches.is_empty() {
                tracing::warn!(repo = %name, "no valid branches to checkout");
                Ok(())
            } else {
                let mut last_err = None;
                for branch in &valid_branches {
                    if let Err(e) = git.checkout_branch(&path, branch, &NoProgress) {
                        tracing::error!(
                            repo = %name,
                            branch = %branch,
                            error = %e,
                            "checkout failed"
                        );
                        last_err = Some(e);
                    }
                }
                last_err.map_or(Ok(()), Err)
            };

            progress.message(&name);
            progress.advance(1);
            FetchPhaseResult {
                repo_name: name,
                repo_url: url,
                repo_guid: guid,
                repo_path: path,
                fetch_result,
                branch_statuses,
                valid_branches,
                checkout_result,
            }
        })
        .collect();

    progress.finish();
    results
}

/// Phase 3: Run branch-divergence analytics on repositories with valid branches.
pub fn run_analytics_phase(
    fetch_results: &[FetchPhaseResult],
    progress: &dyn ProgressReporter,
) -> Vec<RepoAnalytics> {
    let ready: Vec<&FetchPhaseResult> = fetch_results
        .iter()
        .filter(|r| !r.valid_branches.is_empty())
        .collect();

    if ready.is_empty() {
        return Vec::new();
    }

    progress.start(
        Some(ready.len() as u64 * 2),
        "phase 3/3: analyzing branch divergence",
    );

    let gix = GixProvider::new();
    let results: Vec<RepoAnalytics> = ready
        .into_par_iter()
        .map(|fetch_result| {
            let name = fetch_result.repo_name.clone();
            let url = fetch_result.repo_url.clone();
            let guid = fetch_result.repo_guid.clone();
            let path = fetch_result.repo_path.clone();
            let branch_statuses = fetch_result.branch_statuses.clone();

            progress.advance(1);
            let analytics =
                analyze_branch_divergence(&gix, &path, &fetch_result.valid_branches, &NoProgress)
                    .map_err(|e| e.to_string());

            progress.message(&name);
            progress.advance(1);
            RepoAnalytics {
                repo_name: name,
                repo_url: url,
                repo_guid: guid,
                repo_path: path,
                branch_statuses,
                analytics,
            }
        })
        .collect();

    progress.finish();
    results
}

/// Run the full 3-phase batch analytics workflow.
///
/// 1. Clone repositories that are missing locally.
/// 2. Fetch updates and determine which requested branches exist on the remote.
/// 3. Run branch-divergence analysis for every repo with at least one valid branch.
/// 4. Write a single JSON file containing every repo's results.
pub fn run_batch_analytics(
    repos: &[RepoSpec],
    branches: &[String],
    index: &mut RepoIndex,
    output: &Path,
    progress: &dyn ProgressReporter,
) -> Result<Vec<RepoAnalytics>, Error> {
    if branches.is_empty() {
        return Err(Error::Parse("no branches specified".to_string()));
    }

    let git = ProcessGitProvider::new();

    // --- Phase 1: clone ---
    let clone_results = run_clone_phase(&git, repos, index, 4, progress);

    let success_count = clone_results.iter().filter(|r| r.result.is_ok()).count();
    if success_count == 0 {
        return Err(Error::Parse(
            "no repositories were successfully cloned/fetched".to_string(),
        ));
    }

    // --- Phase 2: fetch updates + branch info ---
    let fetch_results = run_fetch_phase(&git, &clone_results, branches, progress);

    let with_branches = fetch_results
        .iter()
        .filter(|r| !r.valid_branches.is_empty())
        .count();
    if with_branches == 0 {
        return Err(Error::Parse(
            "no repositories have valid branches".to_string(),
        ));
    }

    // --- Phase 3: divergence analysis ---
    let results = run_analytics_phase(&fetch_results, progress);

    // Write output
    let json =
        serde_json::to_string_pretty(&results).expect("RepoAnalytics Vec should always serialize");
    std::fs::write(output, json).map_err(Error::Io)?;

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn read_repositories_file_parses_urls() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "# comment").unwrap();
        writeln!(tmp, "").unwrap();
        writeln!(tmp, "https://github.com/owner/repo.git").unwrap();
        writeln!(tmp, "custom-name https://github.com/owner/other.git").unwrap();
        tmp.flush().unwrap();

        let repos = read_repositories_file(tmp.path()).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "repo");
        assert_eq!(repos[0].url, "https://github.com/owner/repo.git");
        assert_eq!(repos[1].name, "custom-name");
        assert_eq!(repos[1].url, "https://github.com/owner/other.git");
    }

    #[test]
    fn read_branches_file_parses_branches() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "# ignore this").unwrap();
        writeln!(tmp, "main").unwrap();
        writeln!(tmp, "").unwrap();
        writeln!(tmp, "develop").unwrap();
        tmp.flush().unwrap();

        let branches = read_branches_file(tmp.path()).unwrap();
        assert_eq!(branches, vec!["main", "develop"]);
    }

    #[test]
    fn repo_name_from_various_urls() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/repo.git"),
            "repo"
        );
        assert_eq!(repo_name_from_url("git@github.com:user/repo.git"), "repo");
        assert_eq!(repo_name_from_url("https://github.com/user/repo"), "repo");
        assert_eq!(repo_name_from_url(""), "repo");
    }

    #[test]
    fn branch_status_serializes_correctly() {
        let status = BranchStatus {
            branch: "main".to_string(),
            exists: true,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("main"));
        assert!(json.contains("true"));
    }

    #[test]
    fn run_batch_analytics_empty_branches_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let repos = vec![RepoSpec {
            url: "https://example.com/r.git".to_string(),
            name: "r".to_string(),
        }];
        let result = run_batch_analytics(
            &repos,
            &[],
            &mut index,
            tmp.path().join("out.json").as_path(),
            &NoProgress,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no branches specified"));
    }

    #[test]
    fn run_batch_analytics_no_success_clones_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let repos = vec![RepoSpec {
            url: "file:///nonexistent/path/to/repo".to_string(),
            name: "r".to_string(),
        }];
        let branches = vec!["main".to_string()];
        let result = run_batch_analytics(
            &repos,
            &branches,
            &mut index,
            tmp.path().join("out.json").as_path(),
            &NoProgress,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no repositories were successfully cloned"));
    }

    #[test]
    fn run_clone_phase_empty_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let git = ProcessGitProvider::new();
        let results = run_clone_phase(&git, &[], &mut index, 1, &NoProgress);
        assert!(results.is_empty());
    }

    #[test]
    fn run_fetch_phase_empty_ready() {
        let git = ProcessGitProvider::new();
        let results = run_fetch_phase(&git, &[], &[], &NoProgress);
        assert!(results.is_empty());
    }

    #[test]
    fn run_analytics_phase_empty_ready() {
        let results = run_analytics_phase(&[], &NoProgress);
        assert!(results.is_empty());
    }

    #[test]
    fn run_clone_phase_existing_repo_not_cloned() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let spec = RepoSpec {
            url: "https://example.com/r.git".to_string(),
            name: "r".to_string(),
        };
        // Pre-create the destination with a .git directory so cloning is skipped.
        let entry = index.get_or_insert(spec.url.clone(), spec.name.clone());
        let guid = entry.guid.clone();
        let repo_path = tmp.path().join(&guid).join(&spec.name);
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();

        let git = ProcessGitProvider::new();
        let results = run_clone_phase(&git, &[spec], &mut index, 1, &NoProgress);
        assert_eq!(results.len(), 1);
        assert!(!results[0].cloned);
        assert!(results[0].result.is_ok());
    }

    #[test]
    fn run_clone_phase_create_dir_error() {
        let tmp = tempfile::tempdir().unwrap();
        // Make clone_dir a file so create_dir_all fails.
        let file_path = tmp.path().join("afile");
        std::fs::write(&file_path, "x").unwrap();
        let mut index = RepoIndex::open(&file_path).unwrap();
        let git = ProcessGitProvider::new();
        let results = run_clone_phase(&git, &[], &mut index, 1, &NoProgress);
        assert!(results.is_empty());
    }

    #[test]
    fn run_clone_phase_save_index_error() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        // Make .gitdiverge a file so save() fails.
        std::fs::write(tmp.path().join(".gitdiverge"), "x").unwrap();
        let spec = RepoSpec {
            url: "https://example.com/r.git".to_string(),
            name: "r".to_string(),
        };
        let git = ProcessGitProvider::new();
        let results = run_clone_phase(&git, &[spec], &mut index, 1, &NoProgress);
        // Should still return results even if save failed.
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn run_clone_phase_thread_pool_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let git = ProcessGitProvider::new();
        // max_workers = 0 causes thread pool build to fall back to default.
        let results = run_clone_phase(&git, &[], &mut index, 0, &NoProgress);
        assert!(results.is_empty());
    }

    #[test]
    fn run_fetch_phase_branch_check_error() {
        let git = ProcessGitProvider::new();
        let clone_result = ClonePhaseResult {
            repo_name: "r".to_string(),
            repo_url: "https://example.com/r.git".to_string(),
            repo_guid: "guid".to_string(),
            repo_path: std::path::PathBuf::from("/nonexistent/path/that/causes/error"),
            cloned: true,
            result: Ok(()),
        };
        let results = run_fetch_phase(&git, &[clone_result], &["main".to_string()], &NoProgress);
        assert_eq!(results.len(), 1);
        assert!(!results[0].branch_statuses[0].exists);
    }

    #[test]
    fn run_fetch_phase_no_valid_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::process::Command::new("git")
            .arg("init")
            .arg(&repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(&repo)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();

        let git = ProcessGitProvider::new();
        let clone_result = ClonePhaseResult {
            repo_name: "r".to_string(),
            repo_url: "https://example.com/r.git".to_string(),
            repo_guid: "guid".to_string(),
            repo_path: repo,
            cloned: true,
            result: Ok(()),
        };
        // Request a branch that does not exist on remote.
        let results = run_fetch_phase(
            &git,
            &[clone_result],
            &["nonexistent".to_string()],
            &NoProgress,
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].valid_branches.is_empty());
    }

    #[test]
    fn run_fetch_phase_checkout_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::process::Command::new("git")
            .arg("init")
            .arg(&repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(&repo)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo)
            .args(["remote", "add", "origin", repo.to_str().unwrap()])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&repo)
            .args(["fetch", "origin", "master:refs/remotes/origin/master"])
            .status()
            .unwrap();

        // Lock the index so checkout fails.
        std::fs::write(repo.join(".git/index.lock"), "").unwrap();

        let git = ProcessGitProvider::new();
        let clone_result = ClonePhaseResult {
            repo_name: "r".to_string(),
            repo_url: "https://example.com/r.git".to_string(),
            repo_guid: "guid".to_string(),
            repo_path: repo,
            cloned: true,
            result: Ok(()),
        };
        let results = run_fetch_phase(&git, &[clone_result], &["master".to_string()], &NoProgress);
        assert_eq!(results.len(), 1);
        assert!(!results[0].checkout_result.is_ok());
    }

    #[test]
    fn run_batch_analytics_no_valid_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::process::Command::new("git")
            .arg("init")
            .arg(&source)
            .status()
            .unwrap();
        std::fs::write(source.join("a.txt"), "a").unwrap();
        std::process::Command::new("git")
            .current_dir(&source)
            .args(["add", "."])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(&source)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap();

        let mut index = RepoIndex::open(tmp.path()).unwrap();
        let repos = vec![RepoSpec {
            url: source.to_str().unwrap().to_string(),
            name: "r".to_string(),
        }];
        let branches = vec!["nonexistent".to_string()];
        let result = run_batch_analytics(
            &repos,
            &branches,
            &mut index,
            tmp.path().join("out.json").as_path(),
            &NoProgress,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no repositories have valid branches"));
    }
}
