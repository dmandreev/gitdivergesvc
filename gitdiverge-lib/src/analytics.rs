use crate::error::Error;
use crate::git::{Commit, GitProvider, LogOptions};
use crate::progress::ProgressReporter;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::debug;

/// Result of comparing two branches: commits present in `source_branch` but missing
/// from `target_branch`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, utoipa::ToSchema)]
pub struct BranchComparison {
    /// The branch that may contain commits not present in `target_branch`.
    pub source_branch: String,
    /// The branch against which `source_branch` is compared.
    pub target_branch: String,
    /// Commits that exist in `source_branch` but are not reachable from `target_branch`
    /// (judged by commit-hash equality).
    pub missing_commits: Vec<Commit>,
}

/// Full analytics report for a set of branches.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, utoipa::ToSchema)]
pub struct BranchAnalytics {
    /// Absolute filesystem path of the analysed repository.
    pub repo_path: String,
    /// List of branch names that were included in the analysis.
    pub branches: Vec<String>,
    /// Pairwise comparisons for every ordered branch pair.
    /// For `n` branches there are `n*(n-1)` comparisons.
    pub comparisons: Vec<BranchComparison>,
}

/// Lightweight summary of a single branch comparison.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, utoipa::ToSchema)]
pub struct BranchComparisonSummary {
    /// The branch that may contain commits not present in `target_branch`.
    pub source_branch: String,
    /// The branch against which `source_branch` is compared.
    pub target_branch: String,
    /// Number of commits present in `source_branch` but missing from `target_branch`.
    pub missing_commit_count: usize,
}

/// Lightweight analytics report containing only counts for the N×N matrix.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, utoipa::ToSchema)]
pub struct BranchAnalyticsSummary {
    /// Absolute filesystem path of the analysed repository.
    pub repo_path: String,
    /// List of branch names that were included in the analysis.
    pub branches: Vec<String>,
    /// Pairwise comparison summaries for every ordered branch pair.
    pub comparisons: Vec<BranchComparisonSummary>,
}

impl From<&BranchAnalytics> for BranchAnalyticsSummary {
    fn from(analytics: &BranchAnalytics) -> Self {
        Self {
            repo_path: analytics.repo_path.clone(),
            branches: analytics.branches.clone(),
            comparisons: analytics
                .comparisons
                .iter()
                .map(|c| BranchComparisonSummary {
                    source_branch: c.source_branch.clone(),
                    target_branch: c.target_branch.clone(),
                    missing_commit_count: c.missing_commits.len(),
                })
                .collect(),
        }
    }
}

impl From<BranchAnalytics> for BranchAnalyticsSummary {
    fn from(analytics: BranchAnalytics) -> Self {
        Self::from(&analytics)
    }
}

/// Compute commit divergence between all provided branch pairs.
///
/// For every ordered pair `(target, source)` where `target != source`, the result
/// contains the list of commits that exist in `source` but are **not** reachable
/// from `target` (judged purely by commit hash equality).
///
/// The actual log queries are executed via the supplied `git` provider so that
/// callers can inject `GixProvider`, `ProcessGitProvider`, or a mock.
pub fn analyze_branch_divergence(
    git: &dyn GitProvider,
    repo_path: &Path,
    branches: &[String],
    progress: &dyn ProgressReporter,
) -> Result<BranchAnalytics, Error> {
    let pair_count = if branches.len() >= 2 {
        branches.len() * (branches.len() - 1)
    } else {
        0
    };
    let total_work = branches.len() as u64 + pair_count as u64;
    progress.start(Some(total_work), "analyzing branches");

    let mut branch_commits: HashMap<String, Vec<Commit>> = HashMap::new();

    let t0 = std::time::Instant::now();
    for branch in branches {
        let t1 = std::time::Instant::now();
        let commits = git.log(
            repo_path,
            branch,
            LogOptions::default(),
            &crate::progress::NoProgress,
        )?;
        debug!(branch = %branch, commits = commits.len(), elapsed_ms = t1.elapsed().as_millis(), "fetched branch commits");
        branch_commits.insert(branch.clone(), commits);
        progress.advance(1);
    }
    debug!(
        branches = branches.len(),
        elapsed_ms = t0.elapsed().as_millis(),
        "all branch commits fetched"
    );

    let t2 = std::time::Instant::now();
    let mut comparisons = Vec::new();
    for target in branches {
        for source in branches {
            if target == source {
                continue;
            }
            let source_commits = branch_commits.get(source).cloned().unwrap_or_default();
            let target_commits = branch_commits.get(target).cloned().unwrap_or_default();

            let target_hashes: HashSet<_> = target_commits.iter().map(|c| &c.hash).collect();
            let missing: Vec<Commit> = source_commits
                .into_iter()
                .filter(|c| !target_hashes.contains(&c.hash))
                .collect();

            comparisons.push(BranchComparison {
                source_branch: source.clone(),
                target_branch: target.clone(),
                missing_commits: missing,
            });
            progress.advance(1);
        }
    }
    debug!(
        comparisons = comparisons.len(),
        elapsed_ms = t2.elapsed().as_millis(),
        "branch comparisons completed"
    );
    progress.finish();

    Ok(BranchAnalytics {
        repo_path: repo_path.to_string_lossy().to_string(),
        branches: branches.to_vec(),
        comparisons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{Author, GitProvider, LogOptions};
    use crate::progress::ProgressReporter;
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct MockGitProvider {
        responses: Mutex<HashMap<String, Vec<Commit>>>,
    }

    impl MockGitProvider {
        fn with_branch_commits(self, branch: &str, commits: Vec<Commit>) -> Self {
            self.responses
                .lock()
                .unwrap()
                .insert(branch.to_string(), commits);
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
            Ok(self
                .responses
                .lock()
                .unwrap()
                .get(branch)
                .cloned()
                .unwrap_or_default())
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
    fn analyze_no_divergence() {
        let commits = vec![make_commit("a", "A"), make_commit("b", "B")];
        let git = MockGitProvider::default()
            .with_branch_commits("main", commits.clone())
            .with_branch_commits("dev", commits.clone());

        let analytics = analyze_branch_divergence(
            &git,
            Path::new("."),
            &["main".to_string(), "dev".to_string()],
            &crate::progress::NoProgress,
        )
        .unwrap();

        assert_eq!(analytics.comparisons.len(), 2);
        for cmp in &analytics.comparisons {
            assert!(
                cmp.missing_commits.is_empty(),
                "expected no missing commits for {} -> {}",
                cmp.source_branch,
                cmp.target_branch
            );
        }
    }

    #[test]
    fn analyze_with_divergence() {
        let main_commits = vec![make_commit("a", "A"), make_commit("b", "B")];
        let dev_commits = vec![
            make_commit("a", "A"),
            make_commit("b", "B"),
            make_commit("c", "C"),
        ];
        let git = MockGitProvider::default()
            .with_branch_commits("main", main_commits)
            .with_branch_commits("dev", dev_commits);

        let analytics = analyze_branch_divergence(
            &git,
            Path::new("."),
            &["main".to_string(), "dev".to_string()],
            &crate::progress::NoProgress,
        )
        .unwrap();

        assert_eq!(analytics.comparisons.len(), 2);

        let main_missing_dev = analytics
            .comparisons
            .iter()
            .find(|c| c.source_branch == "dev" && c.target_branch == "main")
            .unwrap();
        assert_eq!(main_missing_dev.missing_commits.len(), 1);
        assert_eq!(main_missing_dev.missing_commits[0].hash, "c");

        let dev_missing_main = analytics
            .comparisons
            .iter()
            .find(|c| c.source_branch == "main" && c.target_branch == "dev")
            .unwrap();
        assert!(dev_missing_main.missing_commits.is_empty());
    }

    #[test]
    fn analyze_three_branches() {
        let master = vec![make_commit("m1", "M1"), make_commit("m2", "M2")];
        let develop = vec![
            make_commit("m1", "M1"),
            make_commit("m2", "M2"),
            make_commit("d1", "D1"),
        ];
        let stage = vec![
            make_commit("m1", "M1"),
            make_commit("m2", "M2"),
            make_commit("s1", "S1"),
        ];

        let git = MockGitProvider::default()
            .with_branch_commits("master", master)
            .with_branch_commits("develop", develop)
            .with_branch_commits("stage", stage);

        let analytics = analyze_branch_divergence(
            &git,
            Path::new("."),
            &[
                "master".to_string(),
                "develop".to_string(),
                "stage".to_string(),
            ],
            &crate::progress::NoProgress,
        )
        .unwrap();

        assert_eq!(analytics.comparisons.len(), 6);

        let stage_missing_develop = analytics
            .comparisons
            .iter()
            .find(|c| c.source_branch == "develop" && c.target_branch == "stage")
            .unwrap();
        assert_eq!(stage_missing_develop.missing_commits.len(), 1);
        assert_eq!(stage_missing_develop.missing_commits[0].hash, "d1");

        let develop_missing_stage = analytics
            .comparisons
            .iter()
            .find(|c| c.source_branch == "stage" && c.target_branch == "develop")
            .unwrap();
        assert_eq!(develop_missing_stage.missing_commits.len(), 1);
        assert_eq!(develop_missing_stage.missing_commits[0].hash, "s1");
    }

    #[test]
    fn analyze_empty_branches_list() {
        let git = MockGitProvider::default();
        let analytics =
            analyze_branch_divergence(&git, Path::new("."), &[], &crate::progress::NoProgress)
                .unwrap();
        assert!(analytics.comparisons.is_empty());
        assert!(analytics.branches.is_empty());
    }

    #[test]
    fn analyze_single_branch() {
        let git =
            MockGitProvider::default().with_branch_commits("main", vec![make_commit("a", "A")]);
        let analytics = analyze_branch_divergence(
            &git,
            Path::new("."),
            &["main".to_string()],
            &crate::progress::NoProgress,
        )
        .unwrap();
        assert_eq!(analytics.branches.len(), 1);
        assert!(analytics.comparisons.is_empty());
    }

    #[test]
    fn branch_analytics_serializes_to_json() {
        let analytics = BranchAnalytics {
            repo_path: "/tmp/repo".to_string(),
            branches: vec!["main".to_string(), "dev".to_string()],
            comparisons: vec![BranchComparison {
                source_branch: "dev".to_string(),
                target_branch: "main".to_string(),
                missing_commits: vec![make_commit("c", "C")],
            }],
        };
        let json = serde_json::to_string_pretty(&analytics).unwrap();
        assert!(json.contains("/tmp/repo"));
        assert!(json.contains("main"));
        assert!(json.contains("dev"));
        assert!(json.contains("C"));
    }
}
