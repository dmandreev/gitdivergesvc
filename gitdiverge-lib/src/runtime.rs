use crate::error::Error;
use crate::git::{Commit, GitProvider, LogOptions};
use crate::progress::ProgressReporter;
use crossbeam_channel::{unbounded, Sender};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

/// A unit of work submitted to the runtime.
pub enum Task {
    /// Execute `git log` and send the result back.
    Log {
        repo: PathBuf,
        branch: String,
        options: LogOptions,
        progress: Arc<dyn ProgressReporter>,
        respond: Sender<Result<Vec<Commit>, Error>>,
    },
    /// Signal a worker to stop.
    Shutdown,
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Task::Log {
                repo,
                branch,
                options,
                progress: _,
                respond: _,
            } => f
                .debug_struct("Task::Log")
                .field("repo", repo)
                .field("branch", branch)
                .field("options", options)
                .finish(),
            Task::Shutdown => f.debug_struct("Task::Shutdown").finish(),
        }
    }
}

/// Cloneable handle used to submit tasks to the runtime.
#[derive(Debug, Clone)]
pub struct RuntimeHandle {
    sender: Sender<Task>,
}

impl RuntimeHandle {
    /// Submit a task to the worker pool.
    pub fn submit(&self, task: Task) {
        if let Err(e) = self.sender.send(task) {
            tracing::warn!(error = %e, "failed to submit task");
        }
    }
}

/// A multi-worker task runtime backed by `crossbeam-channel` and `std::thread`.
///
/// Workers pull tasks from a shared queue and execute them using the provided
/// [`GitProvider`]. Rayon can be used *inside* tasks (e.g. [`batch::log_parallel`])
/// for data-parallel sub-work.
pub struct Runtime {
    handle: RuntimeHandle,
    threads: Vec<thread::JoinHandle<()>>,
}

impl Runtime {
    /// Spawn `worker_count` threads that consume from a shared task queue.
    pub fn new(worker_count: usize, git: Arc<dyn GitProvider>) -> Self {
        let (sender, receiver) = unbounded::<Task>();

        let mut threads = Vec::with_capacity(worker_count);
        for worker_id in 0..worker_count {
            let rx = receiver.clone();
            let git = Arc::clone(&git);

            threads.push(thread::spawn(move || {
                tracing::debug!(worker_id, "worker started");
                while let Ok(task) = rx.recv() {
                    match task {
                        Task::Log {
                            repo,
                            branch,
                            options,
                            progress,
                            respond,
                        } => {
                            let _span = tracing::info_span!(
                                "log",
                                repo = ?repo,
                                branch = %branch,
                                worker_id
                            )
                            .entered();
                            tracing::debug!("executing git log");
                            let result = git.log(&repo, &branch, options, &*progress);
                            if respond.send(result).is_err() {
                                tracing::warn!("response channel closed by caller");
                            }
                        }
                        Task::Shutdown => {
                            tracing::debug!(worker_id, "worker shutting down");
                            break;
                        }
                    }
                }
                tracing::debug!(worker_id, "worker exited (channel disconnected)");
            }));
        }

        // Drop the original receiver so workers see disconnect when all handles are gone.
        drop(receiver);

        Self {
            handle: RuntimeHandle { sender },
            threads,
        }
    }

    /// Get a cloneable handle for submitting tasks.
    pub fn handle(&self) -> RuntimeHandle {
        self.handle.clone()
    }

    /// Send shutdown signals and join all worker threads.
    pub fn shutdown(self) {
        for _ in &self.threads {
            let _ = self.handle.sender.send(Task::Shutdown);
        }
        for t in self.threads {
            if let Err(e) = t.join() {
                tracing::error!("worker panicked: {:?}", e);
            }
        }
        tracing::debug!("runtime fully shut down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{Author, Commit, GitProvider, LogOptions};
    use crate::progress::ProgressReporter;
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct MockGitProvider {
        responses: Mutex<HashMap<String, Result<Vec<Commit>, String>>>,
    }

    impl MockGitProvider {
        fn with_response(self, branch: &str, result: Result<Vec<Commit>, String>) -> Self {
            self.responses
                .lock()
                .unwrap()
                .insert(branch.to_string(), result);
            self
        }
    }

    impl GitProvider for MockGitProvider {
        fn log(
            &self,
            _repo: &Path,
            branch: &str,
            _options: LogOptions,
            _progress: &dyn ProgressReporter,
        ) -> Result<Vec<Commit>, Error> {
            match self.responses.lock().unwrap().get(branch) {
                Some(Ok(commits)) => Ok(commits.clone()),
                Some(Err(msg)) => Err(Error::ReferenceNotFound(msg.clone())),
                None => Ok(Vec::new()),
            }
        }

        fn branches(
            &self,
            _repo: &Path,
            _progress: &dyn ProgressReporter,
        ) -> Result<Vec<String>, Error> {
            Ok(Vec::new())
        }
    }

    fn make_commit(hash: &str, subject: &str) -> Commit {
        Commit {
            hash: hash.to_string(),
            author: Author {
                name: "Test".to_string(),
                email: "test@example.com".to_string(),
            },
            timestamp: Utc.timestamp_opt(1700000000, 0).unwrap(),
            subject: subject.to_string(),
            body: None,
            parents: Vec::new(),
        }
    }

    #[test]
    fn runtime_executes_task_and_returns_result() {
        let git = Arc::new(
            MockGitProvider::default().with_response("main", Ok(vec![make_commit("abc", "First")])),
        );
        let runtime = Runtime::new(2, git);
        let handle = runtime.handle();

        let (tx, rx) = unbounded();
        handle.submit(Task::Log {
            repo: PathBuf::from("."),
            branch: "main".to_string(),
            options: LogOptions::default(),
            progress: Arc::new(crate::progress::NoProgress),
            respond: tx,
        });

        let result = rx.recv().unwrap();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
        runtime.shutdown();
    }

    #[test]
    fn runtime_returns_error_from_provider() {
        let git = Arc::new(MockGitProvider::default().with_response("bad", Err("bad".to_string())));
        let runtime = Runtime::new(1, git);
        let handle = runtime.handle();

        let (tx, rx) = unbounded();
        handle.submit(Task::Log {
            repo: PathBuf::from("."),
            branch: "bad".to_string(),
            options: LogOptions::default(),
            progress: Arc::new(crate::progress::NoProgress),
            respond: tx,
        });

        let result = rx.recv().unwrap();
        assert!(result.is_err());
        runtime.shutdown();
    }

    #[test]
    fn runtime_handle_cloneable() {
        let git: Arc<dyn GitProvider> = Arc::new(MockGitProvider::default());
        let runtime = Runtime::new(1, git);
        let handle1 = runtime.handle();
        let handle2 = handle1.clone();

        let (tx1, rx1) = unbounded();
        let (tx2, rx2) = unbounded();

        handle1.submit(Task::Log {
            repo: PathBuf::from("."),
            branch: "main".to_string(),
            options: LogOptions::default(),
            progress: Arc::new(crate::progress::NoProgress),
            respond: tx1,
        });

        handle2.submit(Task::Log {
            repo: PathBuf::from("."),
            branch: "main".to_string(),
            options: LogOptions::default(),
            progress: Arc::new(crate::progress::NoProgress),
            respond: tx2,
        });

        let _ = rx1.recv().unwrap();
        let _ = rx2.recv().unwrap();
        runtime.shutdown();
    }

    #[test]
    fn runtime_task_debug() {
        let task = Task::Log {
            repo: PathBuf::from("."),
            branch: "main".to_string(),
            options: LogOptions::default(),
            progress: Arc::new(crate::progress::NoProgress),
            respond: unbounded().0,
        };
        let debug = format!("{:?}", task);
        assert!(debug.contains("Task::Log"));
        assert!(debug.contains("main"));

        let shutdown = Task::Shutdown;
        assert_eq!(format!("{:?}", shutdown), "Task::Shutdown");
    }

    #[test]
    fn runtime_handle_submit_to_disconnected_channel() {
        let git: Arc<dyn GitProvider> = Arc::new(MockGitProvider::default());
        let runtime = Runtime::new(1, git);
        let handle = runtime.handle();

        // Drop the runtime, which drops the sender inside, then try to submit
        // Actually, we need to shutdown first to disconnect the channel
        runtime.shutdown();

        // After shutdown, submitting should not panic (it logs a warning)
        handle.submit(Task::Shutdown);
    }

    #[test]
    fn runtime_warns_on_closed_channel() {
        let git = Arc::new(
            MockGitProvider::default().with_response("main", Ok(vec![make_commit("abc", "First")])),
        );
        let runtime = Runtime::new(1, git);
        let handle = runtime.handle();

        let (tx, rx) = unbounded();
        handle.submit(Task::Log {
            repo: PathBuf::from("."),
            branch: "main".to_string(),
            options: LogOptions::default(),
            progress: Arc::new(crate::progress::NoProgress),
            respond: tx,
        });

        // Drop receiver immediately so the worker sees a closed channel (line 99).
        drop(rx);

        // Give the worker time to process and try to send.
        std::thread::sleep(std::time::Duration::from_millis(100));

        runtime.shutdown();
    }

    #[derive(Debug)]
    struct PanicGitProvider;

    impl GitProvider for PanicGitProvider {
        fn log(
            &self,
            _repo: &Path,
            _branch: &str,
            _options: LogOptions,
            _progress: &dyn ProgressReporter,
        ) -> Result<Vec<Commit>, Error> {
            panic!("intentional panic");
        }

        fn branches(
            &self,
            _repo: &Path,
            _progress: &dyn ProgressReporter,
        ) -> Result<Vec<String>, Error> {
            panic!("intentional panic");
        }
    }

    #[test]
    fn runtime_handles_worker_panic() {
        let git: Arc<dyn GitProvider> = Arc::new(PanicGitProvider);
        let runtime = Runtime::new(1, git);
        let handle = runtime.handle();

        let (tx, rx) = unbounded();
        handle.submit(Task::Log {
            repo: PathBuf::from("."),
            branch: "main".to_string(),
            options: LogOptions::default(),
            progress: Arc::new(crate::progress::NoProgress),
            respond: tx,
        });

        // The worker panics, so we won't get a response (channel closed).
        let result = rx.recv();
        assert!(result.is_err());

        // shutdown() should handle the panicked thread gracefully (line 133).
        runtime.shutdown();
    }
}
