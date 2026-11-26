//! HS2-H1CSSN: the owner of this server's orchestration background work.
//!
//! The server starts orchestration loops (target, gate and scheduler followers) as
//! async tasks and followers (hosted sessions, line push, scheduler wake, settings
//! watches) as threads. A runtime is built per request, so the deadline timers and
//! watches the CLI runtime starts would otherwise belong to no one. One
//! [`OrchestrationTasks`] per server keeps all of them: every runtime the server builds
//! starts its threads under [`OrchestrationTasks::threads`], and the async loops are kept
//! here with the registry that holds one loop per key. `begin_stopping` signals them all;
//! [`OrchestrationTasks::shutdown`] then waits for them, within a bound.
use hotsheet_cli::orchestration::{RuntimeTasks, TasksShutdown};
use hotsheet_sync::LockExt;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// How long a stopping server waits for its orchestration work to end. A step already
/// in flight (one bounded long poll on a target) may take longer; it is then left to
/// end on its own and does nothing more.
pub const ORCHESTRATION_SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// This server's orchestration loops and threads.
#[derive(Default)]
pub struct OrchestrationTasks {
    threads: Arc<RuntimeTasks>,
    loops: Mutex<Vec<JoinHandle<()>>>,
    /// The keys of the loops running now, one loop per key.
    running: Mutex<HashSet<String>>,
}

/// How an [`OrchestrationTasks::shutdown`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrchestrationShutdown {
    /// Async loops still running when the grace ran out.
    pub loops_outstanding: usize,
    /// The threads' shutdown.
    pub threads: TasksShutdown,
}

impl OrchestrationShutdown {
    /// Whether everything ended within the grace.
    pub fn is_complete(&self) -> bool {
        self.loops_outstanding == 0 && self.threads.outstanding == 0
    }
}

impl std::fmt::Debug for OrchestrationTasks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrchestrationTasks")
            .field("threads", &self.threads)
            .field("loops", &self.running_loops())
            .finish_non_exhaustive()
    }
}

impl OrchestrationTasks {
    /// The owner every runtime this server builds starts its threads under.
    pub fn threads(&self) -> &Arc<RuntimeTasks> {
        &self.threads
    }

    /// Take the key of a loop about to start; false when one runs for it already.
    pub(crate) fn begin_loop(&self, key: &str) -> bool {
        self.running.lock_or_recover().insert(key.to_owned())
    }

    /// A loop for `key` ended (or never started).
    pub(crate) fn end_loop(&self, key: &str) {
        self.running.lock_or_recover().remove(key);
    }

    /// Start an async loop this server keeps. Call inside the runtime. Refused (false)
    /// once stopping, so nothing new starts during a shutdown. Handles of loops that
    /// already ended are dropped here, so the list stays as long as the running set.
    pub(crate) fn spawn_loop<F>(&self, work: F) -> bool
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut loops = self.loops.lock_or_recover();
        if self.threads.is_stopping() {
            return false;
        }
        loops.retain(|handle| !handle.is_finished());
        loops.push(tokio::spawn(work));
        true
    }

    /// [`Self::spawn_loop`] for the loop that took `key` with [`Self::begin_loop`]; a
    /// refused start frees the key again.
    pub(crate) fn spawn_keyed_loop<F>(&self, key: &str, work: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if !self.spawn_loop(work) {
            self.end_loop(key);
        }
    }

    /// Async loops not yet ended.
    pub fn running_loops(&self) -> usize {
        self.loops
            .lock_or_recover()
            .iter()
            .filter(|handle| !handle.is_finished())
            .count()
    }

    /// Tell every loop and thread to stop at its next check. Idempotent.
    pub fn stop(&self) {
        self.threads.stop();
    }

    /// Stop everything and wait at most `grace` for it to end: the async loops first,
    /// then the threads, within what is left. Call after `begin_stopping`, which is what
    /// the async loops race their waits against.
    pub async fn shutdown(&self, grace: Duration) -> OrchestrationShutdown {
        self.stop();
        let deadline = Instant::now() + grace;
        let loops: Vec<JoinHandle<()>> = std::mem::take(&mut *self.loops.lock_or_recover());
        let mut loops_outstanding = 0;
        for handle in loops {
            let left = deadline.saturating_duration_since(Instant::now());
            if tokio::time::timeout(left, handle).await.is_err() {
                loops_outstanding += 1;
            }
        }
        let threads = self.threads.clone();
        let left = deadline.saturating_duration_since(Instant::now());
        let threads = tokio::task::spawn_blocking(move || threads.shutdown(left))
            .await
            .unwrap_or(TasksShutdown {
                joined: 0,
                outstanding: self.threads.running(),
            });
        OrchestrationShutdown {
            loops_outstanding,
            threads,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hs2_h1cssn_shutdown_ends_loops_and_threads_together() {
        let tasks = Arc::new(OrchestrationTasks::default());
        let (stop, stopped) = tokio::sync::watch::channel(false);
        for _ in 0..3 {
            let mut stopped = stopped.clone();
            tasks.spawn_loop(async move {
                let _ = stopped.wait_for(|stop| *stop).await;
            });
        }
        let threads = tasks.threads().clone();
        tasks
            .threads()
            .spawn("hs2-h1cssn-server-thread", move || {
                while !threads.wait_stop(Duration::from_secs(3600)) {}
            })
            .unwrap();
        assert_eq!(tasks.running_loops(), 3);
        assert_eq!(tasks.threads().running(), 1);
        // The loops race the server's stopping signal; this stands in for it.
        stop.send(true).unwrap();
        let report = tasks.shutdown(Duration::from_secs(10)).await;
        assert!(report.is_complete(), "{report:?}");
        assert_eq!(report.threads.joined, 1);
        assert_eq!(tasks.running_loops(), 0);
        assert_eq!(tasks.threads().running(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hs2_h1cssn_a_loop_that_ignores_the_stop_is_reported_after_the_grace() {
        let tasks = OrchestrationTasks::default();
        let (_keep, mut never) = tokio::sync::watch::channel(false);
        tasks.spawn_loop(async move {
            let _ = never.wait_for(|stop| *stop).await;
        });
        let report = tasks.shutdown(Duration::from_millis(20)).await;
        assert_eq!(report.loops_outstanding, 1);
        assert!(!report.is_complete());
    }

    #[tokio::test]
    async fn hs2_h1cssn_no_loop_starts_once_stopping() {
        let tasks = OrchestrationTasks::default();
        tasks.stop();
        assert!(!tasks.spawn_loop(async {}));
        assert_eq!(tasks.running_loops(), 0);
        // A refused keyed loop frees its key.
        assert!(tasks.begin_loop("home|owner"));
        tasks.spawn_keyed_loop("home|owner", async {});
        assert!(tasks.begin_loop("home|owner"));
        assert!(tasks.shutdown(Duration::from_secs(1)).await.is_complete());
    }

    #[test]
    fn hs2_h1cssn_one_loop_per_key_and_a_key_is_free_again_once_it_ends() {
        let tasks = OrchestrationTasks::default();
        assert!(tasks.begin_loop("home|owner"));
        assert!(!tasks.begin_loop("home|owner"));
        assert!(tasks.begin_loop("home|other"));
        tasks.end_loop("home|owner");
        tasks.end_loop("home|owner");
        assert!(tasks.begin_loop("home|owner"));
    }
}
