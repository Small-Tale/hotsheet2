//! Waking on a change under the Job directory instead of rescanning it on a timer.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use crate::error::JobError;
use crate::store::JobStore;

/// A missed file event must not strand a waiter, so every wait re-checks at least this
/// often. The re-check is a local file read, never a network request.
const SAFETY_RECHECK: Duration = Duration::from_secs(1);

/// Wakes when anything under one directory changes.
pub struct ChangeWaiter {
    events: Receiver<()>,
    // Dropping the watcher stops the events, so it lives as long as the waiter.
    _watcher: Option<notify::RecommendedWatcher>,
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
    /// come from more than one journal. A change in any of them ends the wait.
    pub fn watching(dirs: &[&Path]) -> io::Result<Self> {
        for dir in dirs {
            fs::create_dir_all(dir)?;
        }
        let (sender, events) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|event| is_change(&event)) {
                let _ = sender.send(());
            }
        })
        .and_then(|mut watcher| {
            for dir in dirs {
                watcher.watch(dir, RecursiveMode::Recursive)?;
            }
            Ok(watcher)
        })
        .ok();
        Ok(Self {
            events,
            _watcher: watcher,
        })
    }

    /// Block until something changed or `timeout` passed, whichever is first, and never
    /// longer than the safety re-check. Returns whether a change was seen.
    pub fn wait(&self, timeout: Duration) -> bool {
        let changed = match self.events.recv_timeout(timeout.min(SAFETY_RECHECK)) {
            Ok(()) => true,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => false,
        };
        // One write produces several events; fold them into this wake.
        while self.events.try_recv().is_ok() {}
        changed
    }
}

/// A content change, as opposed to a file merely being opened or closed. Lock files are
/// only ever locked, so their events are never a change either.
fn is_change(event: &notify::Event) -> bool {
    use notify::EventKind;

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
        let deadline = Instant::now() + timeout;
        let waiter = ChangeWaiter::new(&dir)?;
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
