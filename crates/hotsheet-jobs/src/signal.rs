//! Waking on a change under the Job directory instead of rescanning it on a timer.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::error::JobError;
use crate::store::JobStore;
use crate::watch::{BackgroundWatcher, Event, EventKind, RecursiveMode, WatchStatus};

/// A missed file event must not strand a waiter, so every wait re-checks at least this
/// often. The re-check is a local file read, never a network request.
const SAFETY_RECHECK: Duration = Duration::from_secs(1);

/// Whether something changed since the last wait, and the signal that it did.
#[derive(Default)]
struct Changed {
    seen: Mutex<bool>,
    woke: Condvar,
}

impl Changed {
    fn wake(&self) {
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.woke.notify_all();
    }
}

/// Wakes when anything under its directories changes.
pub struct ChangeWaiter {
    changed: Arc<Changed>,
    // Dropping the watcher stops the events, so it lives as long as the waiter. HS2-F6F42W:
    // it is the shared [`BackgroundWatcher`], so its stream starts, is retried after a
    // refusal, and stops on its own thread; until it runs, waits end at the safety re-check.
    watcher: BackgroundWatcher,
}

impl std::fmt::Debug for ChangeWaiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangeWaiter").finish_non_exhaustive()
    }
}

impl ChangeWaiter {
    /// Watch `dir` and everything below it, creating it when missing. If the platform
    /// watcher cannot start, waits still end at the safety re-check.
    pub fn new(dir: &Path) -> io::Result<Self> {
        Self::watching(&[dir])
    }

    /// HS2-ECWG50: one waiter over several directories, for a waiter whose wake can
    /// come from more than one journal. A change in any of them ends the wait. All of
    /// them are registered before the platform watcher starts once (HS2-N1QDZW).
    ///
    /// Returns once the platform watcher is live, or has been refused once (it is then
    /// retried in the background, and waits end at the safety re-check meanwhile), so a
    /// change made after this returns is reported. A caller that reads after each wait
    /// need not wait for the start at all: see [`ChangeWaiter::starting`].
    pub fn watching(dirs: &[&Path]) -> io::Result<Self> {
        let (first, reported) = std::sync::mpsc::channel();
        let waiter = Self::with_first_status(dirs, Some(first))?;
        let _ = reported.recv();
        Ok(waiter)
    }

    /// HS2-WD475E: [`ChangeWaiter::watching`] that returns at once. On a loaded machine a
    /// start took one to five seconds, and longer while many started together; a reader
    /// that waited for it first noticed nothing, not even a change its own re-check would
    /// find. Until the watcher runs, and while the platform refuses it, every wait still
    /// ends at the safety re-check, so a caller that reads again after each wait finds a
    /// change made meanwhile within that interval. HS2-F6F42W: going live ends the next
    /// wait too, so the caller reads once more as soon as events can be trusted, and
    /// dropping the waiter never waits for its stream to stop.
    pub fn starting(dirs: &[&Path]) -> io::Result<Self> {
        Self::with_first_status(dirs, None)
    }

    fn with_first_status(
        dirs: &[&Path],
        first: Option<std::sync::mpsc::Sender<()>>,
    ) -> io::Result<Self> {
        for dir in dirs {
            fs::create_dir_all(dir)?;
        }
        let changed = Arc::new(Changed::default());
        let (signal, status) = (Arc::clone(&changed), Arc::clone(&changed));
        let paths: Vec<(&Path, RecursiveMode)> = dirs
            .iter()
            .map(|dir| (*dir, RecursiveMode::Recursive))
            .collect();
        let mut first = first;
        let watcher = BackgroundWatcher::start(
            &paths,
            move |event| {
                if event.is_ok_and(|event| is_change(&event)) {
                    signal.wake();
                }
            },
            move |_: WatchStatus| {
                status.wake();
                if let Some(first) = first.take() {
                    let _ = first.send(());
                }
            },
        );
        Ok(Self { changed, watcher })
    }

    /// Block until something changed or `timeout` passed, whichever is first, and never
    /// longer than the safety re-check. Returns whether a change was seen. A change
    /// that arrived between two waits ends the next one at once, and several events of
    /// one write are one wake.
    pub fn wait(&self, timeout: Duration) -> bool {
        let seen = self
            .changed
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (mut seen, _) = self
            .changed
            .woke
            .wait_timeout_while(seen, timeout.min(SAFETY_RECHECK), |seen| !*seen)
            .unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut *seen)
    }

    /// How many times the platform watcher was started for this waiter; zero when it
    /// could not start at all, or a background start has not finished yet.
    pub fn platform_starts(&self) -> usize {
        self.watcher.starts()
    }

    /// HS2-F6F42W: whether the platform watcher is live yet.
    pub fn is_live(&self) -> bool {
        self.watcher.is_live()
    }
}

/// A content change, as opposed to a file merely being opened or closed. Lock files are
/// only ever locked, so their events are never a change either.
fn is_change(event: &Event) -> bool {
    let changed = matches!(
        event.kind,
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    );
    let only_locks = !event.paths.is_empty()
        && event.paths.iter().all(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".lock"))
        });
    changed && !only_locks
}

/// HS2-N1QDZW: the waiter of one checkout, kept between waits so a follower that waits
/// step after step starts the platform watcher once rather than once per step.
#[derive(Debug, Default)]
pub struct KeptWaiter {
    slot: Mutex<Option<Kept>>,
    started: AtomicUsize,
}

#[derive(Debug)]
struct Kept {
    dir: PathBuf,
    identity: DirIdentity,
    waiter: ChangeWaiter,
}

/// Which directory a path named when its waiter started. A directory that was removed
/// and created again is a different one, and a watch of the old one may be dead.
#[derive(Debug, PartialEq, Eq)]
struct DirIdentity(Option<(u64, u64)>);

impl DirIdentity {
    #[cfg(unix)]
    fn of(dir: &Path) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self(fs::metadata(dir).ok().map(|meta| (meta.dev(), meta.ino())))
    }

    /// Without a stable identity to compare, no waiter is kept: every wait starts its
    /// own, as before.
    #[cfg(not(unix))]
    fn of(_dir: &Path) -> Self {
        Self(None)
    }

    fn is_same(&self, other: &Self) -> bool {
        self.0.is_some() && self == other
    }
}

impl JobStore {
    /// Block until the checkout's revision differs from `since`, or `timeout` passes.
    /// Returns the current revision either way, so `result == since` means a timeout.
    /// The caller needs no polling loop: this wakes on the store's file changes.
    pub fn wait_for_change(
        &self,
        checkout_id: &str,
        since: u64,
        timeout: Duration,
    ) -> Result<u64, JobError> {
        let dir = self.checkout_dir(checkout_id)?;
        // Watch before reading, so a change in between is not missed.
        let waiter = ChangeWaiter::new(&dir)?;
        self.wait_on(&waiter, checkout_id, since, timeout)
    }

    /// [`JobStore::wait_for_change`] for a caller that waits again and again: the
    /// waiter stays in `kept` between calls. A change between two calls is not missed,
    /// because the kept waiter was watching throughout. A second thread that waits
    /// through the same `kept` at the same time uses a waiter of its own.
    pub fn wait_for_change_keeping(
        &self,
        kept: &KeptWaiter,
        checkout_id: &str,
        since: u64,
        timeout: Duration,
    ) -> Result<u64, JobError> {
        let Ok(mut slot) = kept.slot.try_lock() else {
            return self.wait_for_change(checkout_id, since, timeout);
        };
        let dir = self.checkout_dir(checkout_id)?;
        fs::create_dir_all(&dir)?;
        let identity = DirIdentity::of(&dir);
        let reusable = slot
            .as_ref()
            .is_some_and(|held| held.dir == dir && held.identity.is_same(&identity));
        if !reusable {
            // The old waiter stops before the new one starts.
            *slot = None;
        }
        let held = match slot.take() {
            Some(held) => held,
            None => {
                let waiter = ChangeWaiter::new(&dir)?;
                kept.started.fetch_add(1, Ordering::Relaxed);
                Kept {
                    dir,
                    identity,
                    waiter,
                }
            }
        };
        let waited = self.wait_on(&held.waiter, checkout_id, since, timeout);
        *slot = Some(held);
        waited
    }

    fn wait_on(
        &self,
        waiter: &ChangeWaiter,
        checkout_id: &str,
        since: u64,
        timeout: Duration,
    ) -> Result<u64, JobError> {
        let deadline = Instant::now() + timeout;
        loop {
            let revision = self.revision(checkout_id)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if revision != since || remaining.is_zero() {
                return Ok(revision);
            }
            waiter.wait(remaining);
        }
    }
}

impl KeptWaiter {
    /// How many waiters, and so platform watchers, have been started through this one
    /// since it was created. A follower over one unchanged checkout directory stays at
    /// one however many steps it waits.
    pub fn waiters_started(&self) -> usize {
        self.started.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A waiter whose platform watcher could not start.
    fn unwatched() -> ChangeWaiter {
        ChangeWaiter {
            changed: Arc::new(Changed::default()),
            watcher: BackgroundWatcher::start(&[], |_| {}, |_| {}),
        }
    }

    fn signal(waiter: &ChangeWaiter) {
        *waiter.changed.seen.lock().unwrap() = true;
        waiter.changed.woke.notify_all();
    }

    #[test]
    fn hs2_n1qdzw_a_waiter_without_a_watcher_waits_out_its_timeout() {
        let waiter = unwatched();
        assert_eq!(waiter.platform_starts(), 0);
        let started = Instant::now();
        assert!(!waiter.wait(Duration::from_millis(60)));
        assert!(!waiter.wait(Duration::from_millis(60)));
        assert!(
            started.elapsed() >= Duration::from_millis(120),
            "a waiter that cannot watch must not spin"
        );
        // And never longer than the safety re-check.
        let started = Instant::now();
        assert!(!waiter.wait(Duration::from_secs(3600)));
        assert!(started.elapsed() < Duration::from_secs(30));
    }

    #[test]
    fn hs2_n1qdzw_a_change_between_waits_ends_the_next_one_and_only_that_one() {
        let waiter = unwatched();
        // Several events of one write are one wake.
        signal(&waiter);
        signal(&waiter);
        signal(&waiter);
        let started = Instant::now();
        assert!(waiter.wait(Duration::from_secs(3600)));
        assert!(started.elapsed() < Duration::from_secs(30));
        assert!(!waiter.wait(Duration::from_millis(20)));
        // Empty, then refilled.
        signal(&waiter);
        assert!(waiter.wait(Duration::ZERO), "a zero wait still reports");
        assert!(!waiter.wait(Duration::ZERO));
    }

    #[test]
    fn hs2_n1qdzw_a_change_during_a_wait_ends_it() {
        let waiter = unwatched();
        for _ in 0..3 {
            let entered = std::sync::Barrier::new(2);
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    entered.wait();
                    signal(&waiter);
                });
                entered.wait();
                // The safety re-check ends a wait that saw nothing, so ask again.
                assert!((0..30).any(|_| waiter.wait(Duration::from_secs(1))));
            });
        }
    }

    #[test]
    fn hs2_n1qdzw_lock_files_and_reads_are_not_changes() {
        use crate::watch::EventKind;
        let event = |kind: EventKind, name: &str| Event::new(kind).add_path(name.into());
        let any = EventKind::Any;
        assert!(is_change(&event(any, "/jobs/a/record.json")));
        assert!(!is_change(&event(any, "/jobs/a/store.lock")));
        assert!(is_change(&Event::new(any)), "an event without a path");
        assert!(is_change(
            &event(any, "/jobs/a/store.lock").add_path("/jobs/a/record.json".into())
        ));
        assert!(!is_change(&event(EventKind::Other, "/jobs/a/record.json")));
    }
}
