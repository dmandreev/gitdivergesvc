/// Abstraction over progress reporting for long-running operations.
///
/// Implementations can be no-ops (for tests / headless mode), CLI progress
/// bars, web-socket progress streams, etc. The trait is object-safe and
/// thread-safe so it can be shared across worker threads and rayon tasks.
pub trait ProgressReporter: Send + Sync {
    /// Initialise the progress indicator.
    ///
    /// * `total` — `Some(n)` when the total amount of work is known upfront,
    ///   `None` for indeterminate / streaming work.
    /// * `message` — Human-readable description of the current operation.
    fn start(&self, total: Option<u64>, message: &str);

    /// Advance the progress by `amount` units.
    fn advance(&self, amount: u64);

    /// Mark the operation as complete.
    fn finish(&self);

    /// Update the human-readable message for the current operation without
    /// changing the progress count.
    fn message(&self, _message: &str) {}
}

/// No-op progress reporter for tests and headless usage.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl ProgressReporter for NoProgress {
    fn start(&self, _total: Option<u64>, _message: &str) {}
    fn advance(&self, _amount: u64) {}
    fn finish(&self) {}
}

/// A progress reporter that wraps a callback. Useful for ad-hoc composition.
pub struct CallbackProgress<F> {
    callback: F,
}

impl<F> CallbackProgress<F> {
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> ProgressReporter for CallbackProgress<F>
where
    F: Fn(u64, Option<u64>, &str) + Send + Sync,
{
    fn start(&self, total: Option<u64>, message: &str) {
        (self.callback)(0, total, message);
    }

    fn advance(&self, amount: u64) {
        (self.callback)(amount, None, "");
    }

    fn finish(&self) {
        (self.callback)(0, Some(0), "done");
    }
}

use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// A structured progress event suitable for streaming to web clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ProgressEvent {
    /// Operation started.
    Start {
        /// Total amount of work, if known upfront.
        total: Option<u64>,
        /// Human-readable description of the current operation.
        message: String,
    },
    /// Progress advanced by one or more units.
    Advance {
        /// Absolute current position.
        current: u64,
        /// Total amount of work, if known.
        total: Option<u64>,
        /// Optional message update.
        message: String,
    },
    /// Operation finished.
    Finish {
        /// Final message.
        message: String,
    },
}

/// A [`ProgressReporter`] that streams [`ProgressEvent`]s over a
/// `crossbeam_channel` sender.
///
/// This type bridges the synchronous git-worker world with the asynchronous
/// web world: workers call the `ProgressReporter` methods, the events are sent
/// over the channel, and an async task forwards them to an SSE stream.
#[derive(Debug)]
pub struct SseProgress {
    tx: Sender<ProgressEvent>,
    state: Mutex<SseProgressState>,
}

#[derive(Debug)]
struct SseProgressState {
    current: u64,
    total: Option<u64>,
}

impl SseProgress {
    /// Create a new `SseProgress` backed by `tx`.
    pub fn new(tx: Sender<ProgressEvent>) -> Self {
        Self {
            tx,
            state: Mutex::new(SseProgressState {
                current: 0,
                total: None,
            }),
        }
    }

    fn send(&self, event: ProgressEvent) {
        // Best-effort delivery; if the receiver is gone the client disconnected.
        let _ = self.tx.send(event);
    }
}

impl ProgressReporter for SseProgress {
    fn start(&self, total: Option<u64>, message: &str) {
        let (current, tot) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.current = 0;
            state.total = total;
            (state.current, state.total)
        };
        tracing::debug!(%current, ?tot, %message, "sse progress start");
        self.send(ProgressEvent::Start {
            total,
            message: message.to_string(),
        });
    }

    fn advance(&self, amount: u64) {
        let (current, total) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.current += amount;
            (state.current, state.total)
        };
        tracing::trace!(%current, ?total, "sse progress advance");
        self.send(ProgressEvent::Advance {
            current,
            total,
            message: String::new(),
        });
    }

    fn finish(&self) {
        tracing::debug!("sse progress finish");
        self.send(ProgressEvent::Finish {
            message: "done".to_string(),
        });
    }

    fn message(&self, message: &str) {
        let (current, total) = {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            (state.current, state.total)
        };
        tracing::trace!(%current, ?total, %message, "sse progress message");
        self.send(ProgressEvent::Advance {
            current,
            total,
            message: message.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_progress_does_not_panic() {
        let p = NoProgress;
        p.start(Some(10), "msg");
        p.advance(5);
        p.finish();
    }

    #[test]
    fn callback_progress_triggers_on_start() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let c = calls.clone();
        let progress = CallbackProgress::new(move |amount, total, msg: &str| {
            c.lock().unwrap().push((amount, total, msg.to_string()));
        });
        progress.start(Some(100), "testing");
        let guard = calls.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0], (0, Some(100), "testing".to_string()));
    }

    #[test]
    fn callback_progress_triggers_on_advance() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let c = calls.clone();
        let progress = CallbackProgress::new(move |amount, total, msg: &str| {
            c.lock().unwrap().push((amount, total, msg.to_string()));
        });
        progress.advance(10);
        let guard = calls.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0], (10, None, "".to_string()));
    }

    #[test]
    fn callback_progress_triggers_on_finish() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let c = calls.clone();
        let progress = CallbackProgress::new(move |amount, total, msg: &str| {
            c.lock().unwrap().push((amount, total, msg.to_string()));
        });
        progress.finish();
        let guard = calls.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0], (0, Some(0), "done".to_string()));
    }

    #[test]
    fn sse_progress_emits_start_advance_finish() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let progress = SseProgress::new(tx);

        progress.start(Some(10), "working");
        progress.advance(3);
        progress.advance(2);
        progress.finish();

        let events: Vec<ProgressEvent> = rx.try_iter().collect();
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0],
            ProgressEvent::Start {
                total: Some(10),
                message: "working".to_string(),
            }
        );
        assert_eq!(
            events[1],
            ProgressEvent::Advance {
                current: 3,
                total: Some(10),
                message: String::new(),
            }
        );
        assert_eq!(
            events[2],
            ProgressEvent::Advance {
                current: 5,
                total: Some(10),
                message: String::new(),
            }
        );
        assert_eq!(
            events[3],
            ProgressEvent::Finish {
                message: "done".to_string(),
            }
        );
    }

    #[test]
    fn sse_progress_tracks_absolute_position() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let progress = SseProgress::new(tx);

        progress.start(None, "indeterminate");
        progress.advance(5);
        progress.advance(7);

        let events: Vec<ProgressEvent> = rx.try_iter().collect();
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[1],
            ProgressEvent::Advance {
                current: 5,
                total: None,
                message: String::new(),
            }
        );
        assert_eq!(
            events[2],
            ProgressEvent::Advance {
                current: 12,
                total: None,
                message: String::new(),
            }
        );
    }

    #[test]
    fn sse_progress_resets_on_new_start() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let progress = SseProgress::new(tx);

        progress.start(Some(5), "phase 1");
        progress.advance(5);
        progress.start(Some(3), "phase 2");
        progress.advance(1);

        let events: Vec<ProgressEvent> = rx.try_iter().collect();
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[2],
            ProgressEvent::Start {
                total: Some(3),
                message: "phase 2".to_string(),
            }
        );
        assert_eq!(
            events[3],
            ProgressEvent::Advance {
                current: 1,
                total: Some(3),
                message: String::new(),
            }
        );
    }
}
