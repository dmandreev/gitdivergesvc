use gitdiverge_lib::{
    analyze_branch_divergence, GitProvider, GixProvider, NoProgress, ProcessGitProvider,
};
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
        .args(["commit", "-m", message])
        .status()
        .expect("git commit should succeed");
    assert!(status.success());
}

#[test]
fn process_git_provider_clone_fetch_checkout_workflow() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin.git");
    let clone = tmp.path().join("clone");

    // 1) Create a bare repo acting as "remote"
    let status = Command::new("git")
        .args(["init", "--bare"])
        .arg(&origin)
        .status()
        .unwrap();
    assert!(status.success());

    // 2) Create a normal repo, push master and develop to the bare repo
    let working = tmp.path().join("working");
    init_repo(&working);
    commit(&working, "Initial", "a.txt", "a");

    // Ensure default branch is master
    Command::new("git")
        .current_dir(&working)
        .args(["branch", "-m", "master"])
        .status()
        .unwrap();

    commit(&working, "Master 2", "b.txt", "b");

    Command::new("git")
        .current_dir(&working)
        .args(["checkout", "-b", "develop"])
        .status()
        .unwrap();
    commit(&working, "Develop 1", "d.txt", "d");

    // Push both branches to bare repo
    Command::new("git")
        .current_dir(&working)
        .args(["push", origin.to_str().unwrap(), "master", "develop"])
        .status()
        .unwrap();

    // 3) Clone the bare repo using ProcessGitProvider
    let provider = ProcessGitProvider::new();
    provider
        .clone_repo(origin.to_str().unwrap(), &clone, &NoProgress)
        .unwrap();
    assert!(clone.join(".git").is_dir());

    // 4) Fetch (should succeed even though just cloned)
    provider.fetch(&clone, &NoProgress).unwrap();

    // 5) Checkout develop
    provider
        .checkout_branch(&clone, "develop", &NoProgress)
        .unwrap();

    // Verify develop branch has the develop commit
    let gix = GixProvider::new();
    let commits = gix
        .log(&clone, "develop", Default::default(), &NoProgress)
        .unwrap();
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[0].subject, "Develop 1");
}

#[test]
fn gix_analytics_on_divergent_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo(repo);

    // Rename default branch to master
    Command::new("git")
        .current_dir(repo)
        .args(["branch", "-m", "master"])
        .status()
        .unwrap();

    commit(repo, "M1", "m1.txt", "m1");
    commit(repo, "M2", "m2.txt", "m2");

    Command::new("git")
        .current_dir(repo)
        .args(["checkout", "-b", "develop"])
        .status()
        .unwrap();
    commit(repo, "D1", "d1.txt", "d1");

    Command::new("git")
        .current_dir(repo)
        .args(["checkout", "master"])
        .status()
        .unwrap();

    Command::new("git")
        .current_dir(repo)
        .args(["checkout", "-b", "stage"])
        .status()
        .unwrap();
    commit(repo, "S1", "s1.txt", "s1");

    // Use GixProvider for analytics
    let gix = GixProvider::new();
    let branches = vec![
        "master".to_string(),
        "develop".to_string(),
        "stage".to_string(),
    ];
    let analytics = analyze_branch_divergence(&gix, repo, &branches, &NoProgress).unwrap();

    assert_eq!(analytics.branches, branches);
    assert_eq!(analytics.comparisons.len(), 6);

    // master is missing D1 (from develop) and S1 (from stage)
    let master_missing_dev = analytics
        .comparisons
        .iter()
        .find(|c| c.target_branch == "master" && c.source_branch == "develop")
        .unwrap();
    assert_eq!(master_missing_dev.missing_commits.len(), 1);
    assert_eq!(master_missing_dev.missing_commits[0].subject, "D1");

    let master_missing_stage = analytics
        .comparisons
        .iter()
        .find(|c| c.target_branch == "master" && c.source_branch == "stage")
        .unwrap();
    assert_eq!(master_missing_stage.missing_commits.len(), 1);
    assert_eq!(master_missing_stage.missing_commits[0].subject, "S1");

    // develop is missing S1 (from stage)
    let dev_missing_stage = analytics
        .comparisons
        .iter()
        .find(|c| c.target_branch == "develop" && c.source_branch == "stage")
        .unwrap();
    assert_eq!(dev_missing_stage.missing_commits.len(), 1);
    assert_eq!(dev_missing_stage.missing_commits[0].subject, "S1");

    // stage is missing D1 (from develop)
    let stage_missing_dev = analytics
        .comparisons
        .iter()
        .find(|c| c.target_branch == "stage" && c.source_branch == "develop")
        .unwrap();
    assert_eq!(stage_missing_dev.missing_commits.len(), 1);
    assert_eq!(stage_missing_dev.missing_commits[0].subject, "D1");
}
