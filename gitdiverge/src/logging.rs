use anyhow::Context;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialise the global `tracing` subscriber.
///
/// * `verbosity` — derived from `-v` / `-q` CLI flags:
///   - `-qq` → ERROR
///   - `-q`  → WARN
///   - none  → INFO
///   - `-v`  → DEBUG
///   - `-vv` → TRACE
///
/// When the process is running under systemd (`JOURNAL_STREAM` is present) a
/// `tracing-journald` layer is used so that log levels map directly to
/// journald priorities (`journalctl -p info`, `-p err`, …).  In all other
/// cases a plain coloured fmt layer is used.
pub fn log_level_from_verbosity(verbosity: i32) -> &'static str {
    match verbosity {
        i32::MIN..=-2 => "error",
        -1 => "warn",
        0 => "info",
        1 => "debug",
        2..=i32::MAX => "trace",
    }
}

pub fn init_logging(verbosity: i32) -> anyhow::Result<()> {
    let level = log_level_from_verbosity(verbosity);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("gitdiverge={level},gitdiverge_lib={level}")));

    let under_systemd = std::env::var_os("JOURNAL_STREAM").is_some();

    if under_systemd {
        let journald_layer =
            tracing_journald::layer().context("failed to create journald layer")?;
        tracing_subscriber::registry()
            .with(env_filter)
            .with(journald_layer)
            .init();
        tracing::info!("logging initialised for systemd journald");
    } else {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .with_target(true);
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
        tracing::info!("logging initialised for console");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_from_verbosity_variants() {
        assert_eq!(log_level_from_verbosity(-3), "error");
        assert_eq!(log_level_from_verbosity(-2), "error");
        assert_eq!(log_level_from_verbosity(-1), "warn");
        assert_eq!(log_level_from_verbosity(0), "info");
        assert_eq!(log_level_from_verbosity(1), "debug");
        assert_eq!(log_level_from_verbosity(2), "trace");
        assert_eq!(log_level_from_verbosity(10), "trace");
    }
}
