use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use fake::faker::internet::en::FreeEmail;
use fake::faker::lorem::en::{Sentences, Words};
use fake::faker::name::en::Name;
use fake::rand::seq::IndexedRandom;
use fake::rand::RngExt;
use fake::Fake;
use gitdiverge_lib::ProcessGitProvider;
use gitdiverge_lib::RepoIndex;
use std::path::{Path, PathBuf};
use std::process::Command;

const BASE_BRANCHES: &[&str] = &["dev", "stage", "prod", "release1.1", "release1.2"];

const FEATURE_PREFIXES: &[&str] = &[
    "feat/PROJ-756",
    "feat/PROJ-123",
    "feat/PROJ-842",
    "feat/PROJ-999",
    "feat/PROJ-101",
    "bugfix/PROJ-321",
    "bugfix/PROJ-555",
    "hotfix/urgent",
    "refactor/cleanup",
    "chore/deps",
];

const FEAT_SUFFIXES: &[&str] = &[
    "impl",
    "add-auth",
    "fix-null",
    "optimize",
    "refactor",
    "update-schema",
    "migration",
    "tests",
    "docs",
    "pipeline",
    "ci",
    "deploy",
];

const COMMIT_VERBS: &[&str] = &[
    "feat:",
    "fix:",
    "refactor:",
    "chore:",
    "docs:",
    "test:",
    "style:",
    "perf:",
];

const PROJECT_TEMPLATES: &[&[(&str, &str)]] = &[
    // Rust
    &[
        ("Cargo.toml", "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\n"),
        ("src/main.rs", "fn main() {\n    println!(\"hello\");\n}\n"),
        (".gitignore", "/target\nCargo.lock\n"),
    ],
    // Node
    &[
        ("package.json", "{\n  \"name\": \"demo-app\",\n  \"version\": \"1.0.0\"\n}\n"),
        ("index.js", "console.log('hello');\n"),
        (".gitignore", "node_modules/\npackage-lock.json\n"),
    ],
    // Python
    &[
        ("pyproject.toml", "[project]\nname = \"demo-app\"\nversion = \"0.1.0\"\n"),
        ("app.py", "def main():\n    print('hello')\n"),
        (".gitignore", "__pycache__/\n*.pyc\n"),
    ],
    // Java
    &[
        ("pom.xml", "<?xml version=\"1.0\"?>\n<project>\n  <artifactId>demo</artifactId>\n</project>\n"),
        ("src/main/java/App.java", "public class App {\n    public static void main(String[] args) {\n        System.out.println(\"hello\");\n    }\n}\n"),
        (".gitignore", "target/\n*.class\n"),
    ],
    // Go
    &[
        ("go.mod", "module demo\n\ngo 1.22\n"),
        ("main.go", "package main\n\nimport \"fmt\"\n\nfunc main() {\n    fmt.Println(\"hello\")\n}\n"),
        (".gitignore", "/bin\n"),
    ],
];

#[derive(Clone)]
struct AuthorInfo {
    name: String,
    email: String,
}

fn random_author<R: RngExt + ?Sized>(rng: &mut R) -> AuthorInfo {
    let name: String = Name().fake_with_rng(rng);
    let email: String = FreeEmail().fake_with_rng(rng);
    AuthorInfo { name, email }
}

fn random_commit_message<R: RngExt + ?Sized>(rng: &mut R) -> (String, Option<String>) {
    let verb = COMMIT_VERBS.choose(rng).unwrap();
    let words: Vec<String> = Words(2..6).fake_with_rng(rng);
    let subject = format!("{} {}", verb, words.join(" "));

    let has_body = rng.random_bool(0.4);
    let body = if has_body {
        let sentences: Vec<String> = Sentences(1..4).fake_with_rng(rng);
        Some(sentences.join(" "))
    } else {
        None
    };
    (subject, body)
}

fn random_date<R: RngExt + ?Sized>(
    rng: &mut R,
    base: DateTime<FixedOffset>,
) -> DateTime<FixedOffset> {
    let offset_secs = rng.random_range(300..7200);
    base + Duration::seconds(offset_secs)
}

fn run_git(repo: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .with_context(|| format!("git {:?} failed in {}", args, repo.display()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("git {:?} failed: {}", args, stderr);
    }
    Ok(())
}

fn run_git_with_env(repo: &Path, args: &[&str], env: &[(String, String)]) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo).args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .with_context(|| format!("git {:?} failed in {}", args, repo.display()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("git {:?} failed: {}", args, stderr);
    }
    Ok(())
}

fn init_repo(repo_path: &Path) -> Result<()> {
    std::fs::create_dir_all(repo_path)?;
    run_git(repo_path, &["init"])?;
    run_git(repo_path, &["config", "user.email", "demo@example.com"])?;
    run_git(repo_path, &["config", "user.name", "Demo User"])?;
    run_git(repo_path, &["branch", "-m", "main"])?;
    Ok(())
}

fn create_commit(
    repo: &Path,
    files: &[(String, String)],
    message: &str,
    body: Option<&str>,
    author: &AuthorInfo,
    date: DateTime<FixedOffset>,
) -> Result<()> {
    for (path, content) in files {
        let full_path = repo.join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full_path, content)?;
    }

    run_git(repo, &["add", "."])?;

    let mut msg = message.to_string();
    if let Some(b) = body {
        msg.push('\n');
        msg.push('\n');
        msg.push_str(b);
    }

    let date_str = date.to_rfc2822();
    let env = vec![
        ("GIT_AUTHOR_NAME".to_string(), author.name.clone()),
        ("GIT_AUTHOR_EMAIL".to_string(), author.email.clone()),
        ("GIT_COMMITTER_NAME".to_string(), author.name.clone()),
        ("GIT_COMMITTER_EMAIL".to_string(), author.email.clone()),
        ("GIT_AUTHOR_DATE".to_string(), date_str.clone()),
        ("GIT_COMMITTER_DATE".to_string(), date_str),
    ];

    run_git_with_env(repo, &["commit", "-m", &msg], &env)?;
    Ok(())
}

fn create_initial_project<R: RngExt + ?Sized>(
    repo: &Path,
    rng: &mut R,
    date: &mut DateTime<FixedOffset>,
) -> Result<()> {
    let template = PROJECT_TEMPLATES.choose(rng).unwrap();
    let author = random_author(rng);
    let files: Vec<(String, String)> = template
        .iter()
        .map(|(p, c)| (p.to_string(), c.to_string()))
        .collect();
    create_commit(repo, &files, "initial commit", None, &author, *date)?;
    Ok(())
}

fn collect_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&current) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with('.') && name != ".gitignore" {
                    continue;
                }
                if name == ".git" {
                    continue;
                }
                if let Ok(meta) = entry.metadata() {
                    if meta.is_dir() {
                        stack.push(path);
                    } else if meta.is_file() {
                        files.push(path);
                    }
                }
            }
        }
    }
    files
}

fn comment_prefix(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next() {
        Some("rs") => Some("//"),
        Some("js") | Some("java") | Some("go") | Some("ts") => Some("//"),
        Some("py") | Some("yaml") | Some("yml") | Some("toml") => Some("#"),
        Some("xml") => Some("<!-- -->"),
        Some("md") | Some("txt") => None,
        Some("json") => None,
        _ => Some("#"),
    }
}

fn add_random_commit<R: RngExt + ?Sized>(
    repo: &Path,
    rng: &mut R,
    date: &mut DateTime<FixedOffset>,
) -> Result<()> {
    let author = random_author(rng);
    let (subject, body) = random_commit_message(rng);

    let mut files = Vec::new();
    let action: u8 = rng.random_range(0..3);
    match action {
        0 => {
            let all_files = collect_files(repo);
            if let Some(path) = all_files.choose(rng) {
                let rel = path
                    .strip_prefix(repo)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                let content = std::fs::read_to_string(path).unwrap_or_default();
                if let Some(prefix) = comment_prefix(&rel) {
                    let line = format!("{} {}\n", prefix, subject);
                    files.push((rel, content + &line));
                }
            }
        }
        1 => {
            let file_names = &["docs.md", "notes.txt", "todo.md", "changelog.md"];
            let name = file_names.choose(rng).unwrap();
            let words: Vec<String> = Words(3..8).fake_with_rng(rng);
            let content = format!("{}\n", words.join(" "));
            files.push((name.to_string(), content));
        }
        2 => {
            let name = if rng.random_bool(0.5) {
                "README.md"
            } else {
                "config.yaml"
            };
            let words: Vec<String> = Words(5..10).fake_with_rng(rng);
            let content = format!("{}\n", words.join(" "));
            files.push((name.to_string(), content));
        }
        _ => unreachable!(),
    }

    if files.is_empty() {
        let words: Vec<String> = Words(2..5).fake_with_rng(rng);
        files.push(("changes.txt".to_string(), format!("{}\n", words.join(" "))));
    }

    *date = random_date(rng, *date);
    create_commit(repo, &files, &subject, body.as_deref(), &author, *date)?;
    Ok(())
}

fn branch_from(repo: &Path, branch: &str, commit: &str) -> Result<()> {
    run_git(repo, &["branch", branch, commit])?;
    Ok(())
}

fn checkout_branch(repo: &Path, branch: &str) -> Result<()> {
    run_git(repo, &["checkout", branch])?;
    Ok(())
}

fn generate_repo<R: RngExt + ?Sized>(repo_path: &Path, rng: &mut R) -> Result<()> {
    init_repo(repo_path)?;

    let mut date =
        Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap()) - Duration::days(180);

    create_initial_project(repo_path, rng, &mut date)?;

    let main_commits = rng.random_range(40..=60);
    for _ in 0..main_commits {
        add_random_commit(repo_path, rng, &mut date)?;
    }

    for branch in BASE_BRANCHES {
        let discrepancy = rng.random_range(1..=30);
        let commit = format!("main~{}", discrepancy);
        branch_from(repo_path, branch, &commit)?;
        checkout_branch(repo_path, branch)?;

        let extra = rng.random_range(3..=12);
        for _ in 0..extra {
            add_random_commit(repo_path, rng, &mut date)?;
        }
    }

    let num_features = rng.random_range(3..=6);
    for i in 0..num_features {
        let prefix = FEATURE_PREFIXES.choose(rng).unwrap();
        let suffix = FEAT_SUFFIXES.choose(rng).unwrap();
        let branch = format!("{}/{}-{}", prefix, suffix, i);

        let from_branch = if rng.random_bool(0.6) { "dev" } else { "main" };
        let discrepancy = rng.random_range(1..=15);
        let commit = format!("{}~{}", from_branch, discrepancy);
        branch_from(repo_path, &branch, &commit)?;
        checkout_branch(repo_path, &branch)?;

        let extra = rng.random_range(2..=10);
        for _ in 0..extra {
            add_random_commit(repo_path, rng, &mut date)?;
        }
    }

    checkout_branch(repo_path, "main")?;
    Ok(())
}

pub fn generate(count: usize, output_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;
    let mut rng = fake::rand::rng();

    for i in 1..=count {
        let repo_name = format!("demo-repo-{:02}", i);
        let guid = uuid::Uuid::new_v4().to_string();
        let repo_path = output_dir.join(&guid).join(&repo_name);
        let url = format!("https://demo.gitdiverge.local/{}.git", repo_name);

        tracing::info!(repo = %repo_name, guid = %guid, "generating demo repository");
        generate_repo(&repo_path, &mut rng)?;

        // Add origin remote so the repo can be registered in the index.
        // The url is fake (no real host), so add a rewrite rule that makes
        // git fetch from the local .git directory instead.
        run_git(&repo_path, &["remote", "add", "origin", &url])?;
        run_git(&repo_path, &["config", "url..git.insteadOf", &url])?;
    }

    // Build index from generated repos
    let git = ProcessGitProvider::new();
    let mut index = RepoIndex::open(output_dir)
        .with_context(|| format!("failed to open repo index in {}", output_dir.display()))?;
    index
        .scan(&git)
        .with_context(|| "failed to scan generated demo repositories")?;
    index.save().with_context(|| "failed to save repo index")?;

    tracing::info!(count, dir = %output_dir.display(), "demo repositories generated");
    Ok(())
}
