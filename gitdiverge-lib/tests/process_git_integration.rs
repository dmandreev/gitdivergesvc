use gitdiverge_lib::{GitProvider, GixProvider, LogOptions, NoProgress, ProcessGitProvider};
use std::fs;
use std::path::Path;
use std::process::Command;

fn init_repo(path: &Path) {
    let status = Command::new("git")
        .arg("init")
        .arg(path)
        .status()
        .expect("git init should succeed");
    assert!(status.success());

    // Configure git user for commits
    for (key, value) in [
        ("user.name", "Test User"),
        ("user.email", "test@example.com"),
    ] {
        let status = Command::new("git")
            .current_dir(path)
            .args(["config", key, value])
            .status()
            .expect("git config should succeed");
        assert!(status.success());
    }
}

fn commit(repo: &Path, message: &str, file_name: &str, content: &str) {
    let file_path = repo.join(file_name);
    fs::write(&file_path, content).unwrap();

    let status = Command::new("git")
        .current_dir(repo)
        .args(["add", file_name])
        .status()
        .expect("git add should succeed");
    assert!(status.success());

    let status = Command::new("git")
        .current_dir(repo)
        .args([
            "commit",
            "-m",
            message,
            "--author",
            "Test User <test@example.com>",
        ])
        .status()
        .expect("git commit should succeed");
    assert!(status.success());
}

fn assert_log_returns_commits_on_main_branch(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    commit(repo, "First commit", "a.txt", "hello");
    commit(repo, "Second commit", "b.txt", "world");

    let commits = provider
        .log(repo, "HEAD", LogOptions::default(), &NoProgress)
        .unwrap();

    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "Second commit");
    assert_eq!(commits[1].subject, "First commit");
    assert_eq!(commits[0].author.name, "Test User");
    assert_eq!(commits[0].author.email, "test@example.com");
}

#[test]
fn process_log_returns_commits_on_main_branch() {
    assert_log_returns_commits_on_main_branch(&ProcessGitProvider::new());
}

#[test]
fn gix_log_returns_commits_on_main_branch() {
    assert_log_returns_commits_on_main_branch(&GixProvider::new());
}

fn assert_log_respects_max_count(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    commit(repo, "One", "1.txt", "1");
    commit(repo, "Two", "2.txt", "2");
    commit(repo, "Three", "3.txt", "3");

    let commits = provider
        .log(
            repo,
            "HEAD",
            LogOptions {
                max_count: Some(2),
                ..LogOptions::default()
            },
            &NoProgress,
        )
        .unwrap();

    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "Three");
    assert_eq!(commits[1].subject, "Two");
}

#[test]
fn process_log_respects_max_count() {
    assert_log_respects_max_count(&ProcessGitProvider::new());
}

#[test]
fn gix_log_respects_max_count() {
    assert_log_respects_max_count(&GixProvider::new());
}

fn assert_log_returns_commits_for_branch_with_no_extra_commits(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    commit(repo, "Only on main", "a.txt", "x");

    // Create a branch from main with no additional commits
    let status = Command::new("git")
        .current_dir(repo)
        .args(["checkout", "-b", "empty-branch"])
        .status()
        .unwrap();
    assert!(status.success());

    let commits = provider
        .log(repo, "empty-branch", LogOptions::default(), &NoProgress)
        .unwrap();
    // Should contain the same commits as the parent branch
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "Only on main");
}

#[test]
fn process_log_returns_commits_for_branch_with_no_extra_commits() {
    assert_log_returns_commits_for_branch_with_no_extra_commits(&ProcessGitProvider::new());
}

#[test]
fn gix_log_returns_commits_for_branch_with_no_extra_commits() {
    assert_log_returns_commits_for_branch_with_no_extra_commits(&GixProvider::new());
}

fn assert_log_returns_error_for_nonexistent_branch(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    commit(repo, "First", "a.txt", "x");

    let result = provider.log(repo, "does-not-exist", LogOptions::default(), &NoProgress);

    assert!(result.is_err());
}

#[test]
fn process_log_returns_error_for_nonexistent_branch() {
    assert_log_returns_error_for_nonexistent_branch(&ProcessGitProvider::new());
}

#[test]
fn gix_log_returns_error_for_nonexistent_branch() {
    assert_log_returns_error_for_nonexistent_branch(&GixProvider::new());
}

fn assert_log_can_use_reverse_order(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    commit(repo, "Alpha", "a.txt", "a");
    commit(repo, "Beta", "b.txt", "b");

    let commits = provider
        .log(
            repo,
            "HEAD",
            LogOptions {
                reverse: true,
                ..LogOptions::default()
            },
            &NoProgress,
        )
        .unwrap();

    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "Alpha");
    assert_eq!(commits[1].subject, "Beta");
}

#[test]
fn process_log_can_use_reverse_order() {
    assert_log_can_use_reverse_order(&ProcessGitProvider::new());
}

#[test]
fn gix_log_can_use_reverse_order() {
    assert_log_can_use_reverse_order(&GixProvider::new());
}

fn assert_log_parses_body_correctly(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    // Multi-line commit message: git inserts blank lines between -m paragraphs
    let status = Command::new("git")
        .current_dir(repo)
        .args([
            "commit",
            "--allow-empty",
            "-m",
            "Subject line",
            "-m",
            "Body line one",
            "-m",
            "Body line two",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let commits = provider
        .log(repo, "HEAD", LogOptions::default(), &NoProgress)
        .unwrap();

    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "Subject line");
    let body = commits[0].body.as_ref().unwrap();
    assert!(body.contains("Body line one"));
    assert!(body.contains("Body line two"));
}

#[test]
fn process_log_parses_body_correctly() {
    assert_log_parses_body_correctly(&ProcessGitProvider::new());
}

#[test]
fn gix_log_parses_body_correctly() {
    assert_log_parses_body_correctly(&GixProvider::new());
}

fn assert_log_parses_parents_for_merge_commit(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    // Determine default branch name (master on older git, main on newer)
    commit(repo, "Base", "base.txt", "base");
    let default_branch = String::from_utf8(
        Command::new("git")
            .current_dir(repo)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    // Create a branch and commit
    let status = Command::new("git")
        .current_dir(repo)
        .args(["checkout", "-b", "feature"])
        .status()
        .unwrap();
    assert!(status.success());

    commit(repo, "Feature work", "feat.txt", "feat");

    // Merge feature back into default branch
    let status = Command::new("git")
        .current_dir(repo)
        .args(["checkout", &default_branch])
        .status()
        .unwrap();
    assert!(status.success());

    let status = Command::new("git")
        .current_dir(repo)
        .args(["merge", "--no-ff", "feature", "-m", "Merge feature"])
        .status()
        .unwrap();
    assert!(status.success());

    let commits = provider
        .log(repo, "HEAD", LogOptions::default(), &NoProgress)
        .unwrap();

    // HEAD should be the merge commit with 2 parents
    assert_eq!(commits[0].subject, "Merge feature");
    assert_eq!(commits[0].parents.len(), 2);
}

#[test]
fn process_log_parses_parents_for_merge_commit() {
    assert_log_parses_parents_for_merge_commit(&ProcessGitProvider::new());
}

#[test]
fn gix_log_parses_parents_for_merge_commit() {
    assert_log_parses_parents_for_merge_commit(&GixProvider::new());
}

fn assert_log_respects_skip(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    commit(repo, "One", "1.txt", "1");
    commit(repo, "Two", "2.txt", "2");
    commit(repo, "Three", "3.txt", "3");

    let commits = provider
        .log(
            repo,
            "HEAD",
            LogOptions {
                skip: Some(1),
                ..LogOptions::default()
            },
            &NoProgress,
        )
        .unwrap();

    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "Two");
    assert_eq!(commits[1].subject, "One");
}

#[test]
fn process_log_respects_skip() {
    assert_log_respects_skip(&ProcessGitProvider::new());
}

#[test]
fn gix_log_respects_skip() {
    assert_log_respects_skip(&GixProvider::new());
}

fn assert_log_git_command_error_outside_repo(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let not_a_repo = tmp.path();

    let result = provider.log(not_a_repo, "HEAD", LogOptions::default(), &NoProgress);

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("git command failed")
            || err.contains("does not appear to be a git repository")
    );
}

#[test]
fn process_log_git_command_error_outside_repo() {
    assert_log_git_command_error_outside_repo(&ProcessGitProvider::new());
}

#[test]
fn gix_log_git_command_error_outside_repo() {
    assert_log_git_command_error_outside_repo(&GixProvider::new());
}

fn assert_log_respects_since(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    // Commit with an old date
    let file_path = repo.join("old.txt");
    fs::write(&file_path, "old").unwrap();
    Command::new("git")
        .current_dir(repo)
        .args(["add", "old.txt"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", "2020-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2020-01-01T00:00:00+00:00")
        .args(["commit", "-m", "Old commit"])
        .status()
        .unwrap();

    // Commit with a new date
    let file_path = repo.join("new.txt");
    fs::write(&file_path, "new").unwrap();
    Command::new("git")
        .current_dir(repo)
        .args(["add", "new.txt"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", "2024-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2024-01-01T00:00:00+00:00")
        .args(["commit", "-m", "New commit"])
        .status()
        .unwrap();

    let since = chrono::DateTime::parse_from_rfc3339("2022-01-01T00:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let commits = provider
        .log(
            repo,
            "HEAD",
            LogOptions {
                since: Some(since),
                ..LogOptions::default()
            },
            &NoProgress,
        )
        .unwrap();

    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "New commit");
}

#[test]
fn process_log_respects_since() {
    assert_log_respects_since(&ProcessGitProvider::new());
}

#[test]
fn gix_log_respects_since() {
    assert_log_respects_since(&GixProvider::new());
}

fn assert_log_respects_until(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    // Commit with an old date
    let file_path = repo.join("old.txt");
    fs::write(&file_path, "old").unwrap();
    Command::new("git")
        .current_dir(repo)
        .args(["add", "old.txt"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", "2020-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2020-01-01T00:00:00+00:00")
        .args(["commit", "-m", "Old commit"])
        .status()
        .unwrap();

    // Commit with a new date
    let file_path = repo.join("new.txt");
    fs::write(&file_path, "new").unwrap();
    Command::new("git")
        .current_dir(repo)
        .args(["add", "new.txt"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", "2024-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2024-01-01T00:00:00+00:00")
        .args(["commit", "-m", "New commit"])
        .status()
        .unwrap();

    let until = chrono::DateTime::parse_from_rfc3339("2022-01-01T00:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let commits = provider
        .log(
            repo,
            "HEAD",
            LogOptions {
                until: Some(until),
                ..LogOptions::default()
            },
            &NoProgress,
        )
        .unwrap();

    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "Old commit");
}

#[test]
fn process_log_respects_until() {
    assert_log_respects_until(&ProcessGitProvider::new());
}

#[test]
fn gix_log_respects_until() {
    assert_log_respects_until(&GixProvider::new());
}

// ------------------------------------------------------------------
// Similarity check: both providers must return identical results.
// ------------------------------------------------------------------

#[test]
fn providers_return_similar_results_on_linear_history() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    commit(repo, "One", "1.txt", "1");
    commit(repo, "Two", "2.txt", "2");
    commit(repo, "Three", "3.txt", "3");

    let process = ProcessGitProvider::new();
    let gix = GixProvider::new();

    let process_commits = process
        .log(repo, "HEAD", LogOptions::default(), &NoProgress)
        .unwrap();
    let gix_commits = gix
        .log(repo, "HEAD", LogOptions::default(), &NoProgress)
        .unwrap();

    assert_eq!(
        process_commits.len(),
        gix_commits.len(),
        "commit count mismatch"
    );
    for (i, (p, g)) in process_commits.iter().zip(gix_commits.iter()).enumerate() {
        assert_eq!(p.hash, g.hash, "hash mismatch at index {}", i);
        assert_eq!(p.subject, g.subject, "subject mismatch at index {}", i);
        assert_eq!(
            p.author.name, g.author.name,
            "author name mismatch at index {}",
            i
        );
        assert_eq!(
            p.author.email, g.author.email,
            "author email mismatch at index {}",
            i
        );
        assert_eq!(p.parents, g.parents, "parents mismatch at index {}", i);
        assert_eq!(p.body, g.body, "body mismatch at index {}", i);
    }
}

#[test]
fn providers_return_similar_results_with_reverse() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    commit(repo, "Alpha", "a.txt", "a");
    commit(repo, "Beta", "b.txt", "b");

    let process = ProcessGitProvider::new();
    let gix = GixProvider::new();

    let opts = LogOptions {
        reverse: true,
        ..LogOptions::default()
    };

    let process_commits = process
        .log(repo, "HEAD", opts.clone(), &NoProgress)
        .unwrap();
    let gix_commits = gix.log(repo, "HEAD", opts, &NoProgress).unwrap();

    assert_eq!(process_commits.len(), gix_commits.len());
    for (i, (p, g)) in process_commits.iter().zip(gix_commits.iter()).enumerate() {
        assert_eq!(p.hash, g.hash, "hash mismatch at index {}", i);
        assert_eq!(p.subject, g.subject, "subject mismatch at index {}", i);
    }
}

#[test]
fn providers_return_similar_results_with_skip_and_max_count() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    commit(repo, "One", "1.txt", "1");
    commit(repo, "Two", "2.txt", "2");
    commit(repo, "Three", "3.txt", "3");

    let process = ProcessGitProvider::new();
    let gix = GixProvider::new();

    let opts = LogOptions {
        skip: Some(1),
        max_count: Some(1),
        ..LogOptions::default()
    };

    let process_commits = process
        .log(repo, "HEAD", opts.clone(), &NoProgress)
        .unwrap();
    let gix_commits = gix.log(repo, "HEAD", opts, &NoProgress).unwrap();

    assert_eq!(process_commits.len(), gix_commits.len());
    for (i, (p, g)) in process_commits.iter().zip(gix_commits.iter()).enumerate() {
        assert_eq!(p.hash, g.hash, "hash mismatch at index {}", i);
        assert_eq!(p.subject, g.subject, "subject mismatch at index {}", i);
    }
}

// ------------------------------------------------------------------
// Branch divergence scenario: master -> develop -> stage with
// independent commits.
// ------------------------------------------------------------------

fn assert_branch_divergence_scenario(provider: &dyn GitProvider) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    // 1) Ensure the default branch is named master
    let status = Command::new("git")
        .current_dir(repo)
        .args(["branch", "-m", "master"])
        .status()
        .unwrap();
    assert!(status.success());

    // 2) Insert 10 commits to master branch
    for i in 1..=10 {
        commit(
            repo,
            &format!("Master commit {}", i),
            &format!("m{}.txt", i),
            &format!("content {}", i),
        );
    }

    let master_commits = provider
        .log(repo, "master", LogOptions::default(), &NoProgress)
        .unwrap();
    assert_eq!(master_commits.len(), 10, "master should have 10 commits");

    // 3) Create develop branch from master
    let status = Command::new("git")
        .current_dir(repo)
        .args(["checkout", "-b", "develop"])
        .status()
        .unwrap();
    assert!(status.success());

    // 4) Create stage branch from develop
    let status = Command::new("git")
        .current_dir(repo)
        .args(["checkout", "-b", "stage"])
        .status()
        .unwrap();
    assert!(status.success());

    // All three branches should share the same 10 commits
    let develop_commits = provider
        .log(repo, "develop", LogOptions::default(), &NoProgress)
        .unwrap();
    let stage_commits = provider
        .log(repo, "stage", LogOptions::default(), &NoProgress)
        .unwrap();
    assert_eq!(develop_commits.len(), 10);
    assert_eq!(stage_commits.len(), 10);

    // 5) Switch to develop branch
    let status = Command::new("git")
        .current_dir(repo)
        .args(["checkout", "develop"])
        .status()
        .unwrap();
    assert!(status.success());

    // 6) Add two commits to develop branch
    commit(repo, "Develop commit 1", "d1.txt", "dev1");
    commit(repo, "Develop commit 2", "d2.txt", "dev2");

    // Develop should now have 2 additional commits compared to stage
    let develop_commits = provider
        .log(repo, "develop", LogOptions::default(), &NoProgress)
        .unwrap();
    let stage_commits = provider
        .log(repo, "stage", LogOptions::default(), &NoProgress)
        .unwrap();

    assert_eq!(develop_commits.len(), 12, "develop should have 12 commits");
    assert_eq!(stage_commits.len(), 10, "stage should have 10 commits");

    let stage_hashes: std::collections::HashSet<_> =
        stage_commits.iter().map(|c| &c.hash).collect();
    let develop_only: Vec<_> = develop_commits
        .iter()
        .filter(|c| !stage_hashes.contains(&c.hash))
        .collect();
    assert_eq!(
        develop_only.len(),
        2,
        "develop should have 2 commits not present in stage"
    );
    assert_eq!(develop_only[0].subject, "Develop commit 2");
    assert_eq!(develop_only[1].subject, "Develop commit 1");

    // 7) Switch to stage branch
    let status = Command::new("git")
        .current_dir(repo)
        .args(["checkout", "stage"])
        .status()
        .unwrap();
    assert!(status.success());

    // 8) Add one commit to stage branch
    commit(repo, "Stage commit 1", "s1.txt", "stage1");

    // Develop should NOT contain the commit from stage
    let develop_commits = provider
        .log(repo, "develop", LogOptions::default(), &NoProgress)
        .unwrap();
    let stage_commits = provider
        .log(repo, "stage", LogOptions::default(), &NoProgress)
        .unwrap();

    assert_eq!(
        develop_commits.len(),
        12,
        "develop should still have 12 commits"
    );
    assert_eq!(stage_commits.len(), 11, "stage should have 11 commits");

    let develop_hashes: std::collections::HashSet<_> =
        develop_commits.iter().map(|c| &c.hash).collect();
    let stage_only: Vec<_> = stage_commits
        .iter()
        .filter(|c| !develop_hashes.contains(&c.hash))
        .collect();
    assert_eq!(
        stage_only.len(),
        1,
        "stage should have 1 commit not present in develop"
    );
    assert_eq!(stage_only[0].subject, "Stage commit 1");
}

#[test]
fn process_branch_divergence_scenario() {
    assert_branch_divergence_scenario(&ProcessGitProvider::new());
}

#[test]
fn gix_branch_divergence_scenario() {
    assert_branch_divergence_scenario(&GixProvider::new());
}

#[test]
fn providers_return_similar_results_on_divergent_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    let status = Command::new("git")
        .current_dir(repo)
        .args(["branch", "-m", "master"])
        .status()
        .unwrap();
    assert!(status.success());

    for i in 1..=5 {
        commit(
            repo,
            &format!("Master commit {}", i),
            &format!("m{}.txt", i),
            &format!("content {}", i),
        );
    }

    Command::new("git")
        .current_dir(repo)
        .args(["checkout", "-b", "develop"])
        .status()
        .unwrap();

    commit(repo, "Develop commit", "dev.txt", "dev");

    Command::new("git")
        .current_dir(repo)
        .args(["checkout", "master"])
        .status()
        .unwrap();

    commit(repo, "Master commit 6", "m6.txt", "content 6");

    let process = ProcessGitProvider::new();
    let gix = GixProvider::new();

    for branch in ["master", "develop"] {
        let process_commits = process
            .log(repo, branch, LogOptions::default(), &NoProgress)
            .unwrap();
        let gix_commits = gix
            .log(repo, branch, LogOptions::default(), &NoProgress)
            .unwrap();

        assert_eq!(
            process_commits.len(),
            gix_commits.len(),
            "commit count mismatch on {}",
            branch
        );
        for (i, (p, g)) in process_commits.iter().zip(gix_commits.iter()).enumerate() {
            assert_eq!(p.hash, g.hash, "hash mismatch on {} at index {}", branch, i);
            assert_eq!(
                p.subject, g.subject,
                "subject mismatch on {} at index {}",
                branch, i
            );
            assert_eq!(
                p.parents, g.parents,
                "parents mismatch on {} at index {}",
                branch, i
            );
            assert_eq!(
                p.author.name, g.author.name,
                "author name mismatch on {} at index {}",
                branch, i
            );
            assert_eq!(
                p.author.email, g.author.email,
                "author email mismatch on {} at index {}",
                branch, i
            );
        }
    }
}
