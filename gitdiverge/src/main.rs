use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "gitdiverge")]
#[command(about = "Git repository branch analytics tool")]
enum Cli {
    /// Run branch analytics on a single repository or a batch.
    Run(RunArgs),
    /// Start the HTTP daemon.
    Daemon {
        /// Address to bind to
        #[arg(short, long, default_value = "127.0.0.1")]
        bind: String,
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Base directory for cloned repositories
        #[arg(short = 'C', long, default_value = "repos")]
        clone_dir: PathBuf,
        /// Enable JWT authentication
        #[arg(long)]
        auth: bool,
        /// Path to a TOML configuration file for the daemon
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Increase logging verbosity (can be used multiple times)
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
        /// Decrease logging verbosity (can be used multiple times)
        #[arg(short, long, action = clap::ArgAction::Count)]
        quiet: u8,
    },
    /// Render the OpenAPI JSON spec to a file.
    Openapi(OpenapiArgs),
    /// Generate an example TOML configuration file.
    ConfigExample(ConfigExampleArgs),
    /// Generate demo repositories for testing.
    Demo(DemoArgs),
}

#[derive(Parser, Debug)]
struct OpenapiArgs {
    /// Output file path for the OpenAPI JSON spec
    #[arg(short, long, default_value = "openapi.json")]
    output: PathBuf,
    /// Increase logging verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Decrease logging verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    quiet: u8,
}

#[derive(Parser, Debug)]
struct ConfigExampleArgs {
    /// Output file path for the example TOML config
    #[arg(short, long, default_value = "gitdiverge.toml.example")]
    output: PathBuf,
    /// Increase logging verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Decrease logging verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    quiet: u8,
}

#[derive(Parser, Debug)]
struct DemoArgs {
    /// Number of repositories to generate
    #[arg(short, long, default_value_t = 10)]
    count: usize,
    /// Output directory for demo repositories
    #[arg(short, long, default_value = "reps")]
    output: PathBuf,
    /// Increase logging verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Decrease logging verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    quiet: u8,
}

#[derive(Parser, Debug)]
struct RunArgs {
    /// Git repository URL (single-repo mode)
    url: Option<String>,

    /// Local path for the repository (single-repo mode)
    #[arg(short, long)]
    path: Option<PathBuf>,

    /// Comma-separated list of branches (single-repo or batch mode)
    #[arg(short, long)]
    branches: Option<String>,

    /// Path to repositories.txt file (batch mode)
    #[arg(short = 'R', long)]
    repositories_file: Option<PathBuf>,

    /// Path to branches.txt file (batch mode)
    #[arg(long)]
    branches_file: Option<PathBuf>,

    /// Directory for cloned repositories (default: repos)
    #[arg(short = 'C', long, default_value = "repos")]
    clone_dir: PathBuf,

    /// Output JSON file path
    #[arg(short, long, default_value = "branch-analytics.json")]
    output: PathBuf,

    /// Increase logging verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Decrease logging verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    quiet: u8,
}

fn repo_name_from_url(url: &str) -> String {
    let name = url.rsplit('/').next().unwrap_or("repo");
    if name.is_empty() {
        "repo".to_string()
    } else {
        name.trim_end_matches(".git").to_string()
    }
}

/// Verify that `git` is available in PATH and meets the minimum version.
fn check_git_requirements() -> Result<()> {
    let output = std::process::Command::new("git")
        .arg("--version")
        .output()
        .with_context(|| "git does not appear to be installed or is not available in PATH")?;

    if !output.status.success() {
        bail!("git --version exited with a non-zero status");
    }

    let version_str = String::from_utf8_lossy(&output.stdout);
    let version_str = version_str.trim();

    let numbers: Vec<u32> = version_str
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(3)
        .map(|s| s.parse().unwrap_or(0))
        .collect();

    if numbers.len() < 3 {
        bail!("unable to parse git version from: {}", version_str);
    }

    let actual = (numbers[0], numbers[1], numbers[2]);
    let required = (2u32, 30u32, 2u32);

    if actual < required {
        tracing::warn!(
            detected = %version_str,
            required = "2.30.2",
            "git version is below the recommended minimum; some features may not work correctly"
        );
    } else {
        tracing::info!(git_version = %version_str, "git detected");
    }

    Ok(())
}

fn main() -> Result<()> {
    let instance = single_instance::SingleInstance::new("gitdiverge-single-instance")
        .context("failed to create single-instance lock")?;
    if !instance.is_single() {
        bail!("another instance of gitdiverge is already running");
    }

    #[cfg(windows)]
    {
        let _ = enable_ansi_support::enable_ansi_support();
    }

    let cli = Cli::parse();

    let verbosity = match &cli {
        Cli::Run(args) => (args.verbose as i32) - (args.quiet as i32),
        Cli::Daemon { verbose, quiet, .. } => (*verbose as i32) - (*quiet as i32),
        Cli::Openapi(args) => (args.verbose as i32) - (args.quiet as i32),
        Cli::ConfigExample(args) => (args.verbose as i32) - (args.quiet as i32),
        Cli::Demo(args) => (args.verbose as i32) - (args.quiet as i32),
    };
    gitdiverge::logging::init_logging(verbosity).context("failed to initialise logging")?;

    match cli {
        Cli::Run(args) => {
            check_git_requirements().context("git prerequisite check failed")?;
            run_cli(args)
        }
        Cli::Daemon {
            bind,
            port,
            clone_dir,
            auth,
            config,
            verbose: _,
            quiet: _,
        } => {
            check_git_requirements().context("git prerequisite check failed")?;
            let mut svc_config = if let Some(cfg_path) = config {
                gitdiverge::config::ServiceConfig::load(&cfg_path)?
            } else {
                gitdiverge::config::ServiceConfig::default()
            };

            if auth {
                svc_config.auth.enabled = true;
            }

            let clone_dir = std::path::absolute(&clone_dir)
                .with_context(|| format!("failed to resolve clone-dir {}", clone_dir.display()))?;

            tracing::info!(mode = "daemon", port, clone_dir = %clone_dir.display(), "starting gitdiverge");

            let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
            rt.block_on(gitdiverge::daemon::run(&bind, port, clone_dir, svc_config))
                .context("daemon failed")?;
            Ok(())
        }
        Cli::Openapi(args) => {
            let json =
                gitdiverge::daemon::openapi_json().context("failed to generate openapi spec")?;
            std::fs::write(&args.output, json)
                .with_context(|| format!("failed to write {}", args.output.display()))?;
            tracing::info!(path = %args.output.display(), "openapi spec written successfully");
            Ok(())
        }
        Cli::ConfigExample(args) => {
            std::fs::write(
                &args.output,
                gitdiverge::config::ServiceConfig::EXAMPLE_TOML,
            )
            .with_context(|| format!("failed to write {}", args.output.display()))?;
            tracing::info!(path = %args.output.display(), "example config written successfully");
            Ok(())
        }
        Cli::Demo(args) => {
            gitdiverge::demo::generate(args.count, &args.output)
                .with_context(|| "demo generation failed")?;
            Ok(())
        }
    }
}

fn run_cli(mut cli: RunArgs) -> Result<()> {
    cli.clone_dir = std::path::absolute(&cli.clone_dir)
        .with_context(|| format!("failed to resolve clone-dir {}", cli.clone_dir.display()))?;

    if let Some(repos_file) = &cli.repositories_file {
        // Batch mode
        tracing::debug!(
            repos_file = %repos_file.display(),
            clone_dir = %cli.clone_dir.display(),
            "starting gitdiverge in batch mode"
        );

        let repos = gitdiverge_lib::read_repositories_file(repos_file)
            .with_context(|| format!("failed to read {}", repos_file.display()))?;

        if repos.is_empty() {
            bail!("no repositories found in {}", repos_file.display());
        }

        let branches = if let Some(branches_file) = &cli.branches_file {
            gitdiverge_lib::read_branches_file(branches_file)
                .with_context(|| format!("failed to read {}", branches_file.display()))?
        } else if let Some(b) = &cli.branches {
            b.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            bail!("--branches or --branches-file is required in batch mode");
        };

        if branches.is_empty() {
            bail!("no branches specified");
        }

        gitdiverge::run_batch_branch_analytics(&repos, &branches, &cli.clone_dir, &cli.output)
            .with_context(|| "batch branch analytics workflow failed")?;
    } else if let Some(url) = &cli.url {
        // Single-repo mode
        tracing::debug!(url = %url, "starting gitdiverge in single-repo mode");

        let branches: Vec<String> = match cli.branches {
            Some(b) => b
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            None => bail!("--branches is required in single-repo mode"),
        };

        if branches.is_empty() {
            bail!("no branches specified");
        }

        let mut index = None;
        let (repo_path, repo_guid) = match cli.path {
            Some(p) => (p, "".to_string()),
            None => {
                let mut idx = gitdiverge_lib::RepoIndex::open(&cli.clone_dir)?;
                let name = repo_name_from_url(url);
                let (guid, path) = {
                    let entry = idx.get_or_insert(url.clone(), name.clone());
                    let guid = entry.guid.clone();
                    let name = entry.name.clone();
                    let path = idx.clone_dir().join(&guid).join(&name);
                    (guid, path)
                };
                index = Some(idx);
                (path, guid)
            }
        };

        gitdiverge::run_branch_analytics(url, &repo_path, &repo_guid, &branches, &cli.output)
            .with_context(|| "branch analytics workflow failed")?;

        if let Some(idx) = index {
            idx.save()?;
        }
    } else {
        bail!("either provide a <URL> or use --repositories-file");
    }

    tracing::info!(output = %cli.output.display(), "analytics written successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn repo_name_from_ssh_url() {
        assert_eq!(repo_name_from_url("git@github.com:user/repo.git"), "repo");
    }

    #[test]
    fn repo_name_from_https_url() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/repo.git"),
            "repo"
        );
    }

    #[test]
    fn repo_name_without_git_suffix() {
        assert_eq!(repo_name_from_url("https://github.com/user/repo"), "repo");
    }

    #[test]
    fn repo_name_fallback() {
        assert_eq!(repo_name_from_url(""), "repo");
    }

    #[test]
    fn cli_parse_run_with_url_and_branches() {
        let cli = Cli::try_parse_from(&[
            "gitdiverge",
            "run",
            "https://github.com/user/repo.git",
            "-b",
            "main,dev",
        ])
        .unwrap();
        match cli {
            Cli::Run(args) => {
                assert_eq!(
                    args.url,
                    Some("https://github.com/user/repo.git".to_string())
                );
                assert_eq!(args.branches, Some("main,dev".to_string()));
            }
            _ => panic!("expected Run variant"),
        }
    }

    #[test]
    fn cli_parse_daemon_with_port_and_clone_dir() {
        let cli = Cli::try_parse_from(&[
            "gitdiverge",
            "daemon",
            "-p",
            "3000",
            "-C",
            "/tmp/repos",
            "--auth",
        ])
        .unwrap();
        match cli {
            Cli::Daemon {
                port,
                clone_dir,
                auth,
                ..
            } => {
                assert_eq!(port, 3000);
                assert_eq!(clone_dir, PathBuf::from("/tmp/repos"));
                assert!(auth);
            }
            _ => panic!("expected Daemon variant"),
        }
    }

    #[test]
    fn cli_parse_config_example_default_output() {
        let cli = Cli::try_parse_from(&["gitdiverge", "config-example"]).unwrap();
        match cli {
            Cli::ConfigExample(args) => {
                assert_eq!(args.output, PathBuf::from("gitdiverge.toml.example"));
            }
            _ => panic!("expected ConfigExample variant"),
        }
    }

    #[test]
    fn cli_parse_config_example_custom_output() {
        let cli =
            Cli::try_parse_from(&["gitdiverge", "config-example", "-o", "my-config.toml"]).unwrap();
        match cli {
            Cli::ConfigExample(args) => {
                assert_eq!(args.output, PathBuf::from("my-config.toml"));
            }
            _ => panic!("expected ConfigExample variant"),
        }
    }

    #[test]
    fn cli_parse_run_batch_with_branches_file() {
        let cli = Cli::try_parse_from(&[
            "gitdiverge",
            "run",
            "-R",
            "repos.txt",
            "--branches-file",
            "branches.txt",
        ])
        .unwrap();
        match cli {
            Cli::Run(args) => {
                assert_eq!(args.repositories_file, Some(PathBuf::from("repos.txt")));
                assert_eq!(args.branches_file, Some(PathBuf::from("branches.txt")));
            }
            _ => panic!("expected Run variant"),
        }
    }

    #[test]
    fn run_cli_empty_repositories_file_errors() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "\n# comment\n\n").unwrap();
        let args = RunArgs {
            url: None,
            path: None,
            branches: None,
            repositories_file: Some(tmp.path().to_path_buf()),
            branches_file: None,
            clone_dir: PathBuf::from("repos"),
            output: PathBuf::from("out.json"),
            verbose: 0,
            quiet: 0,
        };
        let err = run_cli(args).unwrap_err();
        assert!(err.to_string().contains("no repositories found"));
    }

    #[test]
    fn run_cli_batch_without_branches_errors() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "https://github.com/user/repo.git").unwrap();
        let args = RunArgs {
            url: None,
            path: None,
            branches: None,
            repositories_file: Some(tmp.path().to_path_buf()),
            branches_file: None,
            clone_dir: PathBuf::from("repos"),
            output: PathBuf::from("out.json"),
            verbose: 0,
            quiet: 0,
        };
        let err = run_cli(args).unwrap_err();
        assert!(err
            .to_string()
            .contains("--branches or --branches-file is required"));
    }

    #[test]
    fn run_cli_single_repo_without_branches_errors() {
        let args = RunArgs {
            url: Some("https://github.com/user/repo.git".to_string()),
            path: None,
            branches: None,
            repositories_file: None,
            branches_file: None,
            clone_dir: PathBuf::from("repos"),
            output: PathBuf::from("out.json"),
            verbose: 0,
            quiet: 0,
        };
        let err = run_cli(args).unwrap_err();
        assert!(err
            .to_string()
            .contains("--branches is required in single-repo mode"));
    }

    #[test]
    fn run_cli_single_repo_with_empty_branches_errors() {
        let args = RunArgs {
            url: Some("https://github.com/user/repo.git".to_string()),
            path: None,
            branches: Some("".to_string()),
            repositories_file: None,
            branches_file: None,
            clone_dir: PathBuf::from("repos"),
            output: PathBuf::from("out.json"),
            verbose: 0,
            quiet: 0,
        };
        let err = run_cli(args).unwrap_err();
        assert!(err.to_string().contains("no branches specified"));
    }

    #[test]
    fn run_cli_batch_with_empty_branches_file_errors() {
        let mut repos_tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(repos_tmp, "https://github.com/user/repo.git").unwrap();
        let branches_tmp = tempfile::NamedTempFile::new().unwrap();
        // leave branches file empty
        let args = RunArgs {
            url: None,
            path: None,
            branches: None,
            repositories_file: Some(repos_tmp.path().to_path_buf()),
            branches_file: Some(branches_tmp.path().to_path_buf()),
            clone_dir: PathBuf::from("repos"),
            output: PathBuf::from("out.json"),
            verbose: 0,
            quiet: 0,
        };
        let err = run_cli(args).unwrap_err();
        assert!(err.to_string().contains("no branches specified"));
    }

    #[test]
    fn check_git_requirements_succeeds_when_git_present() {
        // This test assumes git is available in PATH (required by AGENTS.md).
        assert!(check_git_requirements().is_ok());
    }

    #[test]
    fn cli_parse_demo_default_count_and_output() {
        let cli = Cli::try_parse_from(&["gitdiverge", "demo"]).unwrap();
        match cli {
            Cli::Demo(args) => {
                assert_eq!(args.count, 10);
                assert_eq!(args.output, PathBuf::from("reps"));
            }
            _ => panic!("expected Demo variant"),
        }
    }

    #[test]
    fn cli_parse_demo_custom_count_and_output() {
        let cli = Cli::try_parse_from(&["gitdiverge", "demo", "-c", "5", "-o", "demos"]).unwrap();
        match cli {
            Cli::Demo(args) => {
                assert_eq!(args.count, 5);
                assert_eq!(args.output, PathBuf::from("demos"));
            }
            _ => panic!("expected Demo variant"),
        }
    }

    #[test]
    fn run_cli_with_path_uses_provided_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir_all(repo_path.join(".git")).unwrap();
        let args = RunArgs {
            url: Some("https://github.com/user/repo.git".to_string()),
            path: Some(repo_path),
            branches: Some("main".to_string()),
            repositories_file: None,
            branches_file: None,
            clone_dir: PathBuf::from("repos"),
            output: PathBuf::from("out.json"),
            verbose: 0,
            quiet: 0,
        };
        // The call fails because .git is empty and fetch/clone fail,
        // but the `Some(p)` branch in `run_cli` is exercised.
        let result = run_cli(args);
        assert!(result.is_err());
    }
}
