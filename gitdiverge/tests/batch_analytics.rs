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
fn batch_workflow_with_two_repos() {
    let tmp = tempfile::tempdir().unwrap();
    let origin1 = tmp.path().join("origin1.git");
    let origin2 = tmp.path().join("origin2.git");
    let clone_dir = tmp.path().join("repos");
    let output = tmp.path().join("output.json");

    // Create bare origins
    for origin in [&origin1, &origin2] {
        Command::new("git")
            .args(["init", "--bare"])
            .arg(origin)
            .status()
            .unwrap();
    }

    // Working repo 1: master + develop (diverged)
    let working1 = tmp.path().join("working1");
    init_repo(&working1);
    commit(&working1, "Initial", "a.txt", "a");
    Command::new("git")
        .current_dir(&working1)
        .args(["branch", "-m", "master"])
        .status()
        .unwrap();
    commit(&working1, "Master commit", "m.txt", "m");

    Command::new("git")
        .current_dir(&working1)
        .args(["checkout", "-b", "develop"])
        .status()
        .unwrap();
    commit(&working1, "Dev commit", "d.txt", "d");

    Command::new("git")
        .current_dir(&working1)
        .args(["push", origin1.to_str().unwrap(), "master", "develop"])
        .status()
        .unwrap();

    // Working repo 2: master + develop (diverged differently)
    let working2 = tmp.path().join("working2");
    init_repo(&working2);
    commit(&working2, "Initial", "a.txt", "a");
    Command::new("git")
        .current_dir(&working2)
        .args(["branch", "-m", "master"])
        .status()
        .unwrap();

    Command::new("git")
        .current_dir(&working2)
        .args(["checkout", "-b", "develop"])
        .status()
        .unwrap();
    commit(&working2, "Dev2 commit", "d2.txt", "d2");

    Command::new("git")
        .current_dir(&working2)
        .args(["push", origin2.to_str().unwrap(), "master", "develop"])
        .status()
        .unwrap();

    // Run batch analytics
    let repos = vec![
        gitdiverge_lib::RepoSpec {
            url: origin1.to_str().unwrap().to_string(),
            name: "repo1".to_string(),
        },
        gitdiverge_lib::RepoSpec {
            url: origin2.to_str().unwrap().to_string(),
            name: "repo2".to_string(),
        },
    ];
    let branches = vec!["master".to_string(), "develop".to_string()];

    gitdiverge::run_batch_branch_analytics(&repos, &branches, &clone_dir, &output).unwrap();

    assert!(output.exists());
    let json = fs::read_to_string(&output).unwrap();
    assert!(json.contains("repo1"));
    assert!(json.contains("repo2"));
    assert!(json.contains("master"));
    assert!(json.contains("develop"));

    // Both repos should have successful analytics
    assert!(json.contains("\"Ok\""));
}

#[test]
fn batch_workflow_skips_missing_branches_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin.git");
    let clone_dir = tmp.path().join("repos");
    let output = tmp.path().join("output.json");

    Command::new("git")
        .args(["init", "--bare"])
        .arg(&origin)
        .status()
        .unwrap();

    let working = tmp.path().join("working");
    init_repo(&working);
    commit(&working, "Initial", "a.txt", "a");
    Command::new("git")
        .current_dir(&working)
        .args(["branch", "-m", "master"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(&working)
        .args(["push", origin.to_str().unwrap(), "master"])
        .status()
        .unwrap();

    let repos = vec![gitdiverge_lib::RepoSpec {
        url: origin.to_str().unwrap().to_string(),
        name: "repo".to_string(),
    }];
    let branches = vec!["master".to_string(), "blabla".to_string()];

    gitdiverge::run_batch_branch_analytics(&repos, &branches, &clone_dir, &output).unwrap();

    assert!(output.exists());
    let json = fs::read_to_string(&output).unwrap();
    assert!(json.contains("master"));
    // Missing branch is recorded in branch_statuses with exists=false
    assert!(json.contains("blabla"));
    assert!(json.contains("\"exists\": false"));
}
