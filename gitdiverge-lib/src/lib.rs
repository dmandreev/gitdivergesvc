pub mod analytics;
pub mod batch;
pub mod error;
pub mod git;
pub mod progress;
pub mod repo_batch;
pub mod repo_index;
pub mod runtime;

pub use analytics::{
    analyze_branch_divergence, BranchAnalytics, BranchAnalyticsSummary, BranchComparison,
    BranchComparisonSummary,
};
pub use batch::log_parallel;
pub use error::Error;
pub use git::{
    Author, Commit, GitCredential, GitProvider, GixProvider, LogOptions, ProcessGitProvider,
};
pub use progress::{CallbackProgress, NoProgress, ProgressEvent, ProgressReporter, SseProgress};
pub use repo_batch::{
    read_branches_file, read_repositories_file, run_analytics_phase, run_batch_analytics,
    run_clone_phase, run_fetch_phase, BranchStatus, ClonePhaseResult, FetchPhaseResult,
    RepoAnalytics, RepoSpec,
};
pub use repo_index::{RepoEntry, RepoIndex, ScanAction, ScanResult};
pub use runtime::{Runtime, RuntimeHandle, Task};
