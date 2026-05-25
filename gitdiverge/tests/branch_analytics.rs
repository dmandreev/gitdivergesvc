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
fn full_workflow_with_run_branch_analytics() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin.git");
    let clone = tmp.path().join("clone");
    let output = tmp.path().join("output.json");

    // 1) Create bare origin
    Command::new("git")
        .args(["init", "--bare"])
        .arg(&origin)
        .status()
        .unwrap();

    // 2) Create working repo and push branches
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
        .args(["checkout", "-b", "develop"])
        .status()
        .unwrap();
    commit(&working, "Dev commit", "d.txt", "d");

    Command::new("git")
        .current_dir(&working)
        .args(["checkout", "master"])
        .status()
        .unwrap();
    commit(&working, "Master commit", "m.txt", "m");

    Command::new("git")
        .current_dir(&working)
        .args(["push", origin.to_str().unwrap(), "master", "develop"])
        .status()
        .unwrap();

    // 3) Run the full workflow
    gitdiverge::run_branch_analytics(
        origin.to_str().unwrap(),
        &clone,
        "",
        &["master".to_string(), "develop".to_string()],
        &output,
    )
    .unwrap();

    assert!(output.exists());
    let json = fs::read_to_string(&output).unwrap();
    assert!(json.contains("master"));
    assert!(json.contains("develop"));
    assert!(json.contains("Dev commit"));
    assert!(json.contains("Master commit"));
}

#[test]
fn workflow_skips_missing_branches_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin.git");
    let clone = tmp.path().join("clone");
    let output = tmp.path().join("output.json");

    // 1) Create bare origin
    Command::new("git")
        .args(["init", "--bare"])
        .arg(&origin)
        .status()
        .unwrap();

    // 2) Create working repo and push only master
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

    // 3) Request master + a non-existent branch "blabla"
    gitdiverge::run_branch_analytics(
        origin.to_str().unwrap(),
        &clone,
        "",
        &["master".to_string(), "blabla".to_string()],
        &output,
    )
    .unwrap();

    assert!(output.exists());
    let json = fs::read_to_string(&output).unwrap();
    assert!(json.contains("master"));
    // Missing branch is recorded in branch_statuses with exists=false
    assert!(json.contains("blabla"));
    assert!(json.contains("\"exists\": false"));
}

#[test]
fn workflow_fails_when_all_branches_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin.git");
    let clone = tmp.path().join("clone");
    let output = tmp.path().join("output.json");

    // 1) Create bare origin with no branches
    Command::new("git")
        .args(["init", "--bare"])
        .arg(&origin)
        .status()
        .unwrap();

    // 2) Request only non-existent branches
    let result = gitdiverge::run_branch_analytics(
        origin.to_str().unwrap(),
        &clone,
        "",
        &["foo".to_string(), "bar".to_string()],
        &output,
    );

    assert!(result.is_err());
}
