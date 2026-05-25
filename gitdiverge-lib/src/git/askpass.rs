use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A cross-platform GIT_ASKPASS helper script.
///
/// The script reads the actual token from the `GITDIVERGE_TOKEN` environment
/// variable at invocation time, so a single script can be reused for many
/// repositories and tokens.  The prompt text passed by git is inspected to
/// distinguish username requests (answered with `oauth2`) from password
/// requests (answered with the token).
#[derive(Debug)]
pub struct AskpassScript {
    path: PathBuf,
}

impl AskpassScript {
    /// Create a new askpass script in the system temp directory.
    pub fn new() -> std::io::Result<Self> {
        let path = Self::script_path();
        let mut file = std::fs::File::create(&path)?;

        if cfg!(windows) {
            // Windows CMD: inspect argument for "Username" and respond with
            // "oauth2", otherwise echo the token from the env var.
            writeln!(file, "@echo off")?;
            writeln!(
                file,
                r#"echo %~1 | findstr /C:"Username" >nul && echo oauth2 && exit /b 0"#
            )?;
            writeln!(file, "echo %GITDIVERGE_TOKEN%")?;
        } else {
            // Unix shell: same logic using a case statement.
            writeln!(file, "#!/bin/sh")?;
            writeln!(
                file,
                r#"case "$1" in *Username*) echo "oauth2" ;; *) echo "$GITDIVERGE_TOKEN" ;; esac"#
            )?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&path)?.permissions();
                perms.set_mode(0o700);
                std::fs::set_permissions(&path, perms)?;
            }
        }

        Ok(Self { path })
    }

    /// Path to the askpass script.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn script_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut dir = std::env::temp_dir();
        let pid = std::process::id();
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        if cfg!(windows) {
            dir.push(format!("gitdiverge-askpass-{pid}-{count}.cmd"));
        } else {
            dir.push(format!("gitdiverge-askpass-{pid}-{count}.sh"));
        }
        dir
    }
}

impl Drop for AskpassScript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn askpass_script_is_created() {
        let script = AskpassScript::new().unwrap();
        assert!(script.path().exists());
        let content = std::fs::read_to_string(script.path()).unwrap();
        if cfg!(windows) {
            assert!(content.contains("@echo off"));
            assert!(content.contains("GITDIVERGE_TOKEN"));
        } else {
            assert!(content.contains("#!/bin/sh"));
            assert!(content.contains("GITDIVERGE_TOKEN"));
        }
    }

    #[test]
    fn askpass_script_is_removed_on_drop() {
        let path = {
            let script = AskpassScript::new().unwrap();
            let path = script.path().to_path_buf();
            assert!(path.exists());
            path
        };
        assert!(!path.exists());
    }
}
