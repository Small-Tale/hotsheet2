//! HS2-SHHGC6: announce orchestration settings a CLI process changed without a server
//! request.
//!
//! The server publishes `orchestration_changed` after its own owner mutations. A
//! `hotsheet-cli` process can write the project type, the target declaration, this
//! machine's bindings or the policy directly, which announced nothing. Once a client
//! reads an owner's orchestration settings, this server watches the files they come
//! from, natively and without recursion: the owner's `.hotsheet2` directory and the
//! machine home. A file event wakes the watcher; after a short quiet period it
//! compares the owner's settings fingerprint with the last one seen and publishes the
//! same `orchestration_changed` event when it differs. HS2-WD475E: when no event
//! arrives for a second it compares too, as every change waiter re-checks
//! (`hotsheet_jobs::ChangeWaiter`), and it compares before it watches a directory that
//! appeared (a restart of the platform's event stream that took up to twenty seconds on
//! a loaded machine). The re-check reads local files only and never makes a request.
use crate::AppState;
use hotsheet_cli::orchestration::OrchestrationRuntime;
use hotsheet_jobs::watch::{Delivery, PathWatcher, RecursiveMode};
use hotsheet_sync::LockExt;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Settings watchers, one per `(server, machine home, owner)`.
static WATCHERS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
/// The fingerprint each watched owner's settings had when last announced or read.
static SEEN: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();

/// HS2-2EKDSM: changes this server is making to an owner's settings right now. The
/// watcher compares only while none is in flight, so the server's own change, which it
/// announces itself, is not announced a second time.
static IN_FLIGHT: OnceLock<Mutex<HashMap<String, Changes>>> = OnceLock::new();

/// HS2-N8VRFC: the server's own changes of one owner: how many are in flight, and how
/// many ever began. The watcher reads the count of begun changes before it reads the
/// settings and records what it read only if no change began since, under the same
/// lock, so a change that starts between its quiet check and its read is left to the
/// server, which records and announces it.
#[derive(Default)]
struct Changes {
    in_flight: usize,
    begun: u64,
}
/// The longest the watcher defers to a change in flight before comparing anyway.
const IN_FLIGHT_LIMIT: Duration = Duration::from_secs(30);

/// Writes land as several events (a temporary file, a rename); wait for them to pause.
const QUIET: Duration = Duration::from_millis(150);
/// HS2-WD475E: how long an idle watcher waits for an event before it compares the
/// settings anyway (local files only), and checks whether the server is stopping.
const SAFETY_RECHECK: Duration = Duration::from_secs(1);

/// The owner whose orchestration settings a request reads, so its files are watched.
pub(super) fn settings_owner(method: &axum::http::Method, path: &str) -> Option<String> {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.as_str(), parts.as_slice()) {
        (
            "GET",
            [
                "checkouts",
                owner,
                "orchestration",
                "project-type" | "targets" | "policy",
            ],
        ) => Some((*owner).to_owned()),
        _ => None,
    }
}

/// Remember the owner's current settings as seen, so a change this server just made
/// and announced itself is not announced again by the watcher. Best effort.
pub(super) fn note_seen(state: &AppState, runtime: &OrchestrationRuntime, owner: &str) {
    let key = super::orchestration::loop_key(state, owner);
    if let Ok(fingerprint) = runtime.orchestration_fingerprint(owner) {
        SEEN.get_or_init(Default::default)
            .lock_or_recover()
            .insert(key, fingerprint);
    }
}

/// Start watching the owner's settings files unless this server already does. Call
/// inside the runtime, off the async executor.
pub(super) fn ensure_settings_watcher(state: &AppState, owner: &str) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let key = super::orchestration::loop_key(state, owner);
    if !WATCHERS
        .get_or_init(Default::default)
        .lock_or_recover()
        .insert(key.clone())
    {
        return;
    }
    let started = super::orchestration::runtime(state, handle).and_then(|runtime| {
        let paths = runtime.orchestration_watch_paths(owner)?;
        Ok((runtime, paths))
    });
    let Ok((runtime, paths)) = started else {
        forget(&key);
        return;
    };
    // The watch is in place before the reader gets its answer, and the settings are
    // read after it, so no change between the two is missed.
    let (tx, rx) = std::sync::mpsc::channel();
    // HS2-654VBY: every directory is registered before the watcher starts once.
    let mut watched = Vec::new();
    let initial = targets(&paths, &watched);
    let initial: Vec<(&std::path::Path, RecursiveMode)> = initial
        .iter()
        .map(|target| (target.as_path(), RecursiveMode::NonRecursive))
        .collect();
    let Ok(watcher) = PathWatcher::start(&initial, move |event| {
        let _ = tx.send(event);
    }) else {
        forget(&key);
        return;
    };
    watched.extend(initial.iter().map(|(target, _)| target.to_path_buf()));
    note_seen(state, &runtime, owner);
    let (state, owner) = (state.clone(), owner.to_owned());
    let spawned = std::thread::Builder::new()
        .name(format!("hotsheet-orchestration-settings-{owner}"))
        .spawn({
            let key = key.clone();
            move || {
                follow(
                    &state,
                    &runtime,
                    &owner,
                    &key,
                    &paths,
                    (watcher, watched),
                    &rx,
                );
                forget(&key);
            }
        });
    if spawned.is_err() {
        forget(&key);
    }
}

/// HS2-2EKDSM: before this server changes the owner's settings. Pair with
/// [`seen_after_change`] or [`change_failed`].
pub(super) fn begin_change(state: &AppState, owner: &str) {
    let key = super::orchestration::loop_key(state, owner);
    begin(&key);
}

fn begin(key: &str) {
    let mut changes = IN_FLIGHT.get_or_init(Default::default).lock_or_recover();
    let changes = changes.entry(key.to_owned()).or_default();
    changes.in_flight += 1;
    changes.begun += 1;
}

fn end_change(key: &str) {
    let mut changes = IN_FLIGHT.get_or_init(Default::default).lock_or_recover();
    if let Some(changes) = changes.get_mut(key) {
        changes.in_flight = changes.in_flight.saturating_sub(1);
    }
}

fn in_flight(key: &str) -> bool {
    IN_FLIGHT
        .get_or_init(Default::default)
        .lock_or_recover()
        .get(key)
        .is_some_and(|changes| changes.in_flight > 0)
}

/// How many of the server's own changes of this owner ever began.
fn begun(key: &str) -> u64 {
    IN_FLIGHT
        .get_or_init(Default::default)
        .lock_or_recover()
        .get(key)
        .map_or(0, |changes| changes.begun)
}

/// Record `fingerprint` as seen unless a change of the server's own began after
/// `since` (or is in flight); whether it differs from the last one seen. One lock
/// orders this against [`begin_change`], so the server's change is not announced here
/// too (HS2-N8VRFC).
fn record_unless_changing(key: &str, since: u64, fingerprint: u64) -> bool {
    let changes = IN_FLIGHT.get_or_init(Default::default).lock_or_recover();
    if changes
        .get(key)
        .is_some_and(|changes| changes.in_flight > 0 || changes.begun != since)
    {
        return false;
    }
    SEEN.get_or_init(Default::default)
        .lock_or_recover()
        .insert(key.to_owned(), fingerprint)
        .is_none_or(|before| before != fingerprint)
}

/// HS2-2EKDSM: the change [`begin_change`] announced failed; nothing to record.
pub(super) fn change_failed(state: &AppState, owner: &str) {
    end_change(&super::orchestration::loop_key(state, owner));
}

/// After this server changed the owner's settings and before it announces that: mark
/// the change as seen when a watcher follows the owner, so it is announced once, and
/// end the change [`begin_change`] started.
pub(super) fn seen_after_change(state: &AppState, owner: &str) {
    let key = super::orchestration::loop_key(state, owner);
    let watched = WATCHERS
        .get_or_init(Default::default)
        .lock_or_recover()
        .contains(&key);
    if watched
        && let Ok(handle) = tokio::runtime::Handle::try_current()
        && let Ok(runtime) = super::orchestration::runtime(state, handle)
    {
        note_seen(state, &runtime, owner);
    }
    end_change(&key);
}

fn forget(key: &str) {
    WATCHERS
        .get_or_init(Default::default)
        .lock_or_recover()
        .remove(key);
}

/// The directories to watch that are not watched yet: each path that exists, or for a
/// missing `.hotsheet2` its parent until it appears.
fn targets(paths: &[PathBuf], watched: &[PathBuf]) -> Vec<PathBuf> {
    let mut targets: Vec<PathBuf> = Vec::new();
    for path in paths {
        let target = if path.is_dir() {
            path.clone()
        } else {
            match path.parent() {
                Some(parent) if parent.is_dir() => parent.to_path_buf(),
                _ => continue,
            }
        };
        if !watched.contains(&target) && !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

/// Watch the directories that appeared since the last look, in one change. A directory
/// already watched is not registered again: on macOS each registration restarted the
/// event stream (HS2-654VBY).
fn add_watches(watcher: &mut PathWatcher, watched: &mut Vec<PathBuf>, paths: &[PathBuf]) {
    let added = targets(paths, watched);
    if added.is_empty() {
        return;
    }
    let pairs: Vec<(&std::path::Path, RecursiveMode)> = added
        .iter()
        .map(|target| (target.as_path(), RecursiveMode::NonRecursive))
        .collect();
    if watcher.change(&[], &pairs).is_ok() {
        watched.extend(added);
    }
}

/// Announce each change of the owner's settings fingerprint the watched files show.
fn follow(
    state: &AppState,
    runtime: &Arc<OrchestrationRuntime>,
    owner: &str,
    key: &str,
    paths: &[PathBuf],
    watching: (PathWatcher, Vec<PathBuf>),
    rx: &std::sync::mpsc::Receiver<Delivery>,
) {
    let (rescan, asks) = std::sync::mpsc::channel();
    let owned = paths.to_vec();
    if std::thread::Builder::new()
        .name(format!("hotsheet-orchestration-settings-watch-{owner}"))
        .spawn(move || keep_watching(watching, owned, asks))
        .is_err()
    {
        return;
    }
    while !state.is_stopping() {
        let evented = match rx.recv_timeout(SAFETY_RECHECK) {
            Ok(_) => true,
            // HS2-WD475E: no event for a while: compare anyway, unless the server is
            // making a change itself, which it records and announces.
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => false,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if evented {
            // Let the rest of this write arrive, then read once. HS2-2EKDSM: while the
            // server is making a change itself, wait for it to record what it announces.
            let deferred = std::time::Instant::now();
            while rx.recv_timeout(QUIET).is_ok()
                || (in_flight(key) && deferred.elapsed() < IN_FLIGHT_LIMIT)
            {}
        } else if in_flight(key) {
            continue;
        }
        // HS2-WD475E: compare and announce here, and watch any directory that appeared
        // on the watcher's own thread. Watching one more restarts the platform's event
        // stream, which took up to twenty seconds on a loaded machine; done here, it held
        // back the announcement of the change that created `.hotsheet2` and of every
        // change made while it restarted. Those are found at the next safety re-check.
        let since = begun(key);
        if let Ok(fingerprint) = runtime.orchestration_fingerprint(owner)
            && record_unless_changing(key, since, fingerprint)
        {
            super::orchestration::note_orchestration_change(state, owner);
        }
        let _ = rescan.send(());
    }
}

/// HS2-WD475E: keep the platform watcher on a thread of its own that adds each
/// directory that appeared when asked, so a slow event-stream restart never holds back
/// the follower's comparisons. It ends, dropping the watcher, when the follower ends.
fn keep_watching(
    (mut watcher, mut watched): (PathWatcher, Vec<PathBuf>),
    paths: Vec<PathBuf>,
    asks: std::sync::mpsc::Receiver<()>,
) {
    while asks.recv().is_ok() {
        // Several asks while a restart ran are one look.
        while asks.try_recv().is_ok() {}
        add_watches(&mut watcher, &mut watched, &paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HS2-N8VRFC: a change of the server's own that begins between the watcher's quiet
    /// check and its read keeps the watcher from recording or announcing what it read;
    /// with no change since, it records once and reports a difference only once.
    #[test]
    fn hs2_n8vrfc_a_change_begun_after_the_check_is_left_to_the_server() {
        let key = "hs2-n8vrfc-test|1";
        let since = begun(key);
        begin(key);
        assert!(
            !record_unless_changing(key, since, 7),
            "a change is in flight"
        );
        end_change(key);
        assert!(
            !record_unless_changing(key, since, 7),
            "a change began since the check"
        );
        assert!(
            !SEEN
                .get_or_init(Default::default)
                .lock_or_recover()
                .contains_key(key),
            "nothing was recorded"
        );
        let since = begun(key);
        assert!(record_unless_changing(key, since, 7), "a new fingerprint");
        assert!(!record_unless_changing(key, since, 7), "the same one again");
        assert!(record_unless_changing(key, since, 8));
    }

    /// HS2-2EKDSM: changes in flight are counted per owner key, so two overlapping
    /// changes keep the watcher deferring until both are recorded, an end without a
    /// begin changes nothing, and other owners are unaffected.
    #[test]
    fn hs2_2ekdsm_in_flight_changes_are_counted_per_owner() {
        let key = |n: u8| format!("hs2-2ekdsm-test|{n}");
        assert!(!in_flight(&key(1)));
        begin(&key(1));
        begin(&key(1));
        assert!(in_flight(&key(1)) && !in_flight(&key(2)));
        end_change(&key(1));
        assert!(in_flight(&key(1)), "one change is still in flight");
        end_change(&key(1));
        assert!(!in_flight(&key(1)));
        // Repeated and out-of-order ends are harmless.
        end_change(&key(1));
        end_change(&key(2));
        assert!(!in_flight(&key(1)) && !in_flight(&key(2)));
        // Empty, then refilled.
        begin(&key(1));
        assert!(in_flight(&key(1)));
        end_change(&key(1));
        assert!(!in_flight(&key(1)));
    }
}
