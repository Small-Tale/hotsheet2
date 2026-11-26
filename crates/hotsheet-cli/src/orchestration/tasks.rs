//! HS2-H1CSSN: the owner of the orchestration runtime's background threads.
//!
//! A runtime is built per request in the server and per command in the CLI, so the
//! threads one of them starts (a readiness deadline timer, a capacity or claim follow, a
//! detached validation follower) outlive it. They are started through one shared
//! [`RuntimeTasks`] instead: it keeps every handle, carries the stop token they wait on,
//! and at shutdown stops them and joins them within a bound. The server owns one for its
//! lifetime and hands it to every runtime it builds; a runtime built without one gets
//! its own.
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The threads one owner started, and whether it is stopping.
#[derive(Default)]
pub struct RuntimeTasks {
    state: Mutex<TaskState>,
    /// Woken when the owner starts stopping and whenever a thread ends.
    changed: Condvar,
}

#[derive(Default)]
struct TaskState {
    stopping: bool,
    /// Threads started and not yet ended.
    live: usize,
    handles: Vec<JoinHandle<()>>,
}

/// How a [`RuntimeTasks::shutdown`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TasksShutdown {
    /// Threads joined by this shutdown.
    pub joined: usize,
    /// Threads still running when the grace ran out (a request already in flight that
    /// cannot be interrupted). Each does nothing more once it returns.
    pub outstanding: usize,
}

impl std::fmt::Debug for RuntimeTasks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.lock();
        f.debug_struct("RuntimeTasks")
            .field("stopping", &state.stopping)
            .field("live", &state.live)
            .finish_non_exhaustive()
    }
}

/// Ends a thread's record when it returns or unwinds.
struct Ended(Arc<RuntimeTasks>);

impl Drop for Ended {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.live = state.live.saturating_sub(1);
        self.0.changed.notify_all();
    }
}

impl RuntimeTasks {
    fn lock(&self) -> MutexGuard<'_, TaskState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the owner is stopping. A loop checks it between steps.
    pub fn is_stopping(&self) -> bool {
        self.lock().stopping
    }

    /// Tell every thread to stop at its next check, and wake every [`Self::wait_stop`].
    /// Idempotent; joins nothing.
    pub fn stop(&self) {
        self.lock().stopping = true;
        self.changed.notify_all();
    }

    /// Wait up to `timeout`, ending early when the owner starts stopping. Returns
    /// whether it is stopping. The one way a background thread waits for time to pass.
    pub fn wait_stop(&self, timeout: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, timeout, |state| !state.stopping)
            .unwrap_or_else(PoisonError::into_inner);
        state.stopping
    }

    /// Threads started and not yet ended.
    pub fn running(&self) -> usize {
        self.lock().live
    }

    /// Start a named thread this owner keeps. Refused once the owner is stopping, so
    /// nothing new starts during a shutdown. Handles of threads that already ended are
    /// joined here, so the list stays as long as the running set.
    pub fn spawn<F>(self: &Arc<Self>, name: &str, work: F) -> std::io::Result<()>
    where
        F: FnOnce() + Send + 'static,
    {
        let mut state = self.lock();
        if state.stopping {
            return Err(std::io::Error::other("orchestration is stopping"));
        }
        let (ended, running): (Vec<_>, Vec<_>) = std::mem::take(&mut state.handles)
            .into_iter()
            .partition(JoinHandle::is_finished);
        state.handles = running;
        for handle in ended {
            let _ = handle.join();
        }
        let owner = self.clone();
        state.live += 1;
        // The guard is made on the thread, so a closure dropped unrun (a refused spawn)
        // never takes this lock while it is held here.
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let _ended = Ended(owner);
                work();
            });
        match spawned {
            Ok(handle) => {
                state.handles.push(handle);
                Ok(())
            }
            Err(error) => {
                state.live -= 1;
                Err(error)
            }
        }
    }

    /// Stop every thread and join them, waiting at most `grace` for them to end.
    pub fn shutdown(&self, grace: Duration) -> TasksShutdown {
        self.stop();
        let deadline = Instant::now() + grace;
        let mut state = self.lock();
        while state.live > 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        let outstanding = state.live;
        // With none live every thread is past its work and only returning, so all are
        // joined; otherwise only those that finished, and the rest are kept.
        let (ended, running): (Vec<_>, Vec<_>) = std::mem::take(&mut state.handles)
            .into_iter()
            .partition(|handle| outstanding == 0 || handle.is_finished());
        state.handles = running;
        drop(state);
        let joined = ended.len();
        for handle in ended {
            let _ = handle.join();
        }
        TasksShutdown {
            joined,
            outstanding,
        }
    }
}

impl super::OrchestrationRuntime {
    /// Start this runtime's background threads under `tasks`, the owner the server keeps
    /// for its lifetime and hands to every runtime it builds.
    #[must_use]
    pub fn with_tasks(mut self, tasks: Arc<RuntimeTasks>) -> Self {
        self.tasks = tasks;
        self
    }

    /// The owner of this runtime's background threads.
    pub fn tasks(&self) -> &Arc<RuntimeTasks> {
        &self.tasks
    }

    /// Stop this runtime's background threads and join them within `grace`.
    pub fn shutdown(&self, grace: Duration) -> TasksShutdown {
        self.tasks.shutdown(grace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PROMPT: Duration = Duration::from_secs(5);

    #[test]
    fn hs2_h1cssn_shutdown_wakes_and_joins_every_waiting_thread() {
        let tasks = Arc::new(RuntimeTasks::default());
        let woke = Arc::new(AtomicUsize::new(0));
        for _ in 0..4 {
            let (owner, woke) = (tasks.clone(), woke.clone());
            tasks
                .spawn("hs2-h1cssn-wait", move || {
                    // Far longer than the test: only the stop token ends it.
                    while !owner.wait_stop(Duration::from_secs(3600)) {}
                    woke.fetch_add(1, Ordering::SeqCst);
                })
                .unwrap();
        }
        assert_eq!(tasks.running(), 4);
        let began = Instant::now();
        let report = tasks.shutdown(PROMPT);
        assert!(began.elapsed() < PROMPT, "{:?}", began.elapsed());
        assert_eq!(
            report,
            TasksShutdown {
                joined: 4,
                outstanding: 0
            }
        );
        assert_eq!(woke.load(Ordering::SeqCst), 4);
        assert_eq!(tasks.running(), 0);
        assert!(tasks.lock().handles.is_empty());
    }

    #[test]
    fn hs2_h1cssn_nothing_starts_once_stopping_and_stop_is_idempotent() {
        let tasks = Arc::new(RuntimeTasks::default());
        assert!(!tasks.is_stopping());
        assert!(!tasks.wait_stop(Duration::from_millis(1)));
        tasks.stop();
        tasks.stop();
        assert!(tasks.is_stopping());
        assert!(tasks.wait_stop(Duration::from_secs(3600)));
        let ran = Arc::new(AtomicUsize::new(0));
        let counted = ran.clone();
        assert!(
            tasks
                .spawn("hs2-h1cssn-late", move || {
                    counted.fetch_add(1, Ordering::SeqCst);
                })
                .is_err()
        );
        assert_eq!(tasks.running(), 0);
        assert_eq!(
            tasks.shutdown(PROMPT),
            TasksShutdown {
                joined: 0,
                outstanding: 0
            }
        );
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn hs2_h1cssn_ended_threads_are_joined_at_the_next_spawn() {
        let tasks = Arc::new(RuntimeTasks::default());
        for _ in 0..3 {
            tasks.spawn("hs2-h1cssn-short", || {}).unwrap();
        }
        // Wait for all three to end (the condvar is notified as each does).
        let mut state = tasks.lock();
        while state.live > 0 {
            state = tasks.changed.wait(state).unwrap();
        }
        drop(state);
        // Past their work, each thread still has to return before its handle says so.
        while !tasks.lock().handles.iter().all(JoinHandle::is_finished) {
            std::thread::yield_now();
        }
        let (owner, gate) = (tasks.clone(), Arc::new(()));
        let held = gate.clone();
        tasks
            .spawn("hs2-h1cssn-long", move || {
                let _held = held;
                while !owner.wait_stop(Duration::from_secs(3600)) {}
            })
            .unwrap();
        // The three ended handles were joined; only the running one is kept.
        assert_eq!(tasks.lock().handles.len(), 1);
        assert_eq!(tasks.shutdown(PROMPT).outstanding, 0);
        assert_eq!(Arc::strong_count(&gate), 1);
    }

    #[test]
    fn hs2_h1cssn_a_panicking_thread_still_counts_as_ended() {
        let tasks = Arc::new(RuntimeTasks::default());
        tasks
            .spawn("hs2-h1cssn-panic", || panic!("hs2-h1cssn deliberate"))
            .unwrap();
        let report = tasks.shutdown(PROMPT);
        assert_eq!(report.outstanding, 0);
        assert_eq!(report.joined, 1);
        assert_eq!(tasks.running(), 0);
    }

    #[test]
    fn hs2_h1cssn_a_thread_that_outlives_the_grace_is_reported_and_kept() {
        let tasks = Arc::new(RuntimeTasks::default());
        let (release, released) = std::sync::mpsc::channel::<()>();
        // Stands in for a request already in flight: it ignores the stop token.
        tasks
            .spawn("hs2-h1cssn-stuck", move || {
                let _ = released.recv();
            })
            .unwrap();
        let report = tasks.shutdown(Duration::from_millis(20));
        assert_eq!(
            report,
            TasksShutdown {
                joined: 0,
                outstanding: 1
            }
        );
        assert_eq!(tasks.lock().handles.len(), 1);
        // Once it returns, a later shutdown joins it.
        release.send(()).unwrap();
        assert_eq!(
            tasks.shutdown(PROMPT),
            TasksShutdown {
                joined: 1,
                outstanding: 0
            }
        );
    }
}
