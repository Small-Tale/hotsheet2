//! Coalesced, off-request-path checkout worklist regeneration (HS2-Z99TJN).
//!
//! A server ticket write used to regenerate every linked checkout's `worklist.md`
//! synchronously inside the request: one index query plus a read of every open Up Next
//! ticket file (about 19 ms per write on the HS2 store in a debug build). Concurrent
//! writes also raced: a regeneration that queried the index earlier could finish later
//! and leave a stale worklist.
//!
//! [`WorklistRefresher`] moves that work to one background thread. A write calls
//! [`WorklistRefresher::request`] with a key (the store root) and the job that refreshes
//! it; a later request for the same key replaces the pending job. The single worker
//! waits a short debounce, takes every pending job, runs them, and repeats until nothing
//! is pending. Because one thread runs every job, two regenerations never overlap, and a
//! request that arrives while a job runs always gets one more pass after it, so the last
//! write's state is what the worklist ends up showing.

use hotsheet_sync::LockExt;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// How long the worker waits after the first request of a burst before running it, so a
/// batch of writes shares one regeneration.
pub(crate) const WORKLIST_REFRESH_DEBOUNCE: Duration = Duration::from_millis(50);

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct RefreshState {
    pending: BTreeMap<PathBuf, Job>,
    worker_active: bool,
    /// Jobs run so far (observability and tests).
    runs: u64,
}

struct Inner {
    state: Mutex<RefreshState>,
    idle: Condvar,
    debounce: Duration,
}

/// The shared, clonable handle to the coalescing worker.
#[derive(Clone)]
pub(crate) struct WorklistRefresher {
    inner: Arc<Inner>,
}

impl Default for WorklistRefresher {
    fn default() -> Self {
        Self::with_debounce(WORKLIST_REFRESH_DEBOUNCE)
    }
}

impl WorklistRefresher {
    pub(crate) fn with_debounce(debounce: Duration) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(RefreshState::default()),
                idle: Condvar::new(),
                debounce,
            }),
        }
    }

    /// Queue `job` as the refresh for `key`, replacing any job still pending for it, and
    /// make sure the worker is running. Never blocks on a running job.
    pub(crate) fn request(&self, key: PathBuf, job: impl FnOnce() + Send + 'static) {
        let start_worker = {
            let mut state = self.inner.state.lock_or_recover();
            state.pending.insert(key, Box::new(job));
            !std::mem::replace(&mut state.worker_active, true)
        };
        if start_worker {
            let inner = self.inner.clone();
            let spawned = std::thread::Builder::new()
                .name("hotsheet-worklist-refresh".into())
                .spawn(move || run_worker(&inner));
            if let Err(error) = spawned {
                tracing::warn!(%error, "could not start the worklist refresh worker");
                let mut state = self.inner.state.lock_or_recover();
                state.worker_active = false;
                state.pending.clear();
                self.inner.idle.notify_all();
            }
        }
    }

    /// Wait until no job is pending or running, up to `timeout`. Returns whether it went
    /// idle. Used by tests and orderly shutdown; the write path never waits.
    pub(crate) fn wait_idle(&self, timeout: Duration) -> bool {
        let state = self.inner.state.lock_or_recover();
        let (state, result) = self
            .inner
            .idle
            .wait_timeout_while(state, timeout, |state| state.worker_active)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(state);
        !result.timed_out()
    }

    /// Jobs run so far.
    #[cfg(test)]
    pub(crate) fn runs(&self) -> u64 {
        self.inner.state.lock_or_recover().runs
    }
}

fn run_worker(inner: &Inner) {
    loop {
        std::thread::sleep(inner.debounce);
        let jobs = {
            let mut state = inner.state.lock_or_recover();
            if state.pending.is_empty() {
                state.worker_active = false;
                inner.idle.notify_all();
                return;
            }
            std::mem::take(&mut state.pending)
        };
        let count = jobs.len() as u64;
        for (key, job) in jobs {
            // A panicking job must not wedge the worker with `worker_active` stuck true.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                tracing::warn!(key = %key.display(), "worklist refresh job panicked");
            }
        }
        inner.state.lock_or_recover().runs += count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    const LONG: Duration = Duration::from_secs(5);

    fn counter() -> (Arc<AtomicUsize>, impl Fn() -> Box<dyn FnOnce() + Send>) {
        let count = Arc::new(AtomicUsize::new(0));
        let job_count = count.clone();
        (count, move || {
            let count = job_count.clone();
            Box::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            })
        })
    }

    #[test]
    fn a_request_runs_its_job_off_the_calling_thread() {
        let refresher = WorklistRefresher::with_debounce(Duration::from_millis(1));
        let caller = std::thread::current().id();
        let (tx, rx) = mpsc::channel();
        refresher.request("a".into(), move || {
            tx.send(std::thread::current().id()).unwrap();
        });
        let ran_on = rx.recv_timeout(LONG).expect("job ran");
        assert_ne!(ran_on, caller);
        assert!(refresher.wait_idle(LONG));
        assert_eq!(refresher.runs(), 1);
    }

    #[test]
    fn a_burst_for_one_key_coalesces_to_the_latest_job() {
        let refresher = WorklistRefresher::with_debounce(Duration::from_millis(200));
        let seen = Arc::new(Mutex::new(Vec::new()));
        for n in 0..20 {
            let seen = seen.clone();
            refresher.request("store".into(), move || seen.lock_or_recover().push(n));
        }
        assert!(refresher.wait_idle(LONG));
        assert_eq!(*seen.lock_or_recover(), vec![19]);
    }

    #[test]
    fn distinct_keys_each_run_once_per_pass() {
        let refresher = WorklistRefresher::with_debounce(Duration::from_millis(100));
        let (count, job) = counter();
        for key in ["a", "b", "a", "c", "b"] {
            refresher.request(key.into(), job());
        }
        assert!(refresher.wait_idle(LONG));
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    /// Running → request → the worker does one more pass after the running job, and the
    /// two jobs never overlap.
    #[test]
    fn a_request_during_a_running_job_gets_a_later_pass_without_overlap() {
        let refresher = WorklistRefresher::with_debounce(Duration::from_millis(1));
        let running = Arc::new(AtomicUsize::new(0));
        let max_running = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let order = Arc::new(Mutex::new(Vec::new()));
        {
            let (running, max_running, order) =
                (running.clone(), max_running.clone(), order.clone());
            refresher.request("store".into(), move || {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                max_running.fetch_max(now, Ordering::SeqCst);
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(LONG).unwrap();
                order.lock_or_recover().push("first");
                running.fetch_sub(1, Ordering::SeqCst);
            });
        }
        started_rx.recv_timeout(LONG).unwrap();
        {
            let (running, max_running, order) =
                (running.clone(), max_running.clone(), order.clone());
            refresher.request("store".into(), move || {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                max_running.fetch_max(now, Ordering::SeqCst);
                order.lock_or_recover().push("second");
                running.fetch_sub(1, Ordering::SeqCst);
            });
        }
        // The request returned while the first job is still blocked.
        assert!(!refresher.wait_idle(Duration::from_millis(20)));
        release_tx.send(()).unwrap();
        assert!(refresher.wait_idle(LONG));
        assert_eq!(*order.lock_or_recover(), vec!["first", "second"]);
        assert_eq!(max_running.load(Ordering::SeqCst), 1);
    }

    /// Idle → busy → idle → busy again: the worker exits when drained and a later request
    /// starts a fresh one.
    #[test]
    fn the_worker_restarts_after_draining_empty() {
        let refresher = WorklistRefresher::with_debounce(Duration::from_millis(1));
        let (count, job) = counter();
        assert!(refresher.wait_idle(Duration::ZERO), "starts idle");
        for round in 1..=3 {
            refresher.request("store".into(), job());
            assert!(refresher.wait_idle(LONG));
            assert_eq!(count.load(Ordering::SeqCst), round);
        }
        assert_eq!(refresher.runs(), 3);
    }

    #[test]
    fn a_panicking_job_does_not_wedge_the_worker() {
        let refresher = WorklistRefresher::with_debounce(Duration::from_millis(1));
        refresher.request("bad".into(), || panic!("injected job panic"));
        assert!(refresher.wait_idle(LONG));
        let (count, job) = counter();
        refresher.request("good".into(), job());
        assert!(refresher.wait_idle(LONG));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_requesters_never_lose_the_last_job() {
        let refresher = WorklistRefresher::with_debounce(Duration::from_millis(2));
        let last = Arc::new(Mutex::new(BTreeMap::<usize, usize>::new()));
        let threads = (0..8)
            .map(|writer| {
                let (refresher, last) = (refresher.clone(), last.clone());
                std::thread::spawn(move || {
                    for n in 0..50 {
                        let last = last.clone();
                        refresher.request(format!("k{writer}").into(), move || {
                            last.lock_or_recover().insert(writer, n);
                        });
                        if n % 7 == 0 {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap();
        }
        assert!(refresher.wait_idle(LONG));
        let last = last.lock_or_recover();
        assert_eq!(last.len(), 8);
        assert!(last.values().all(|&n| n == 49), "{last:?}");
    }
}
