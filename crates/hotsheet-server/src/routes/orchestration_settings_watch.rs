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
//! same `orchestration_changed` event when it differs. Nothing here reads on a timer:
//! the wait only checks whether the server is stopping.
use crate::AppState;
use hotsheet_cli::orchestration::OrchestrationRuntime;
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
static IN_FLIGHT: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
/// The longest the watcher defers to a change in flight before comparing anyway.
const IN_FLIGHT_LIMIT: Duration = Duration::from_secs(30);

/// Writes land as several events (a temporary file, a rename); wait for them to pause.
const QUIET: Duration = Duration::from_millis(150);
/// How often an idle watcher checks whether the server is stopping. It reads nothing.
const STOP_CHECK: Duration = Duration::from_secs(1);

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
    let Ok(mut watcher) = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    }) else {
        forget(&key);
        return;
    };
    let mut watched = Vec::new();
    add_watches(&mut watcher, &mut watched, &paths);
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
    *IN_FLIGHT
        .get_or_init(Default::default)
        .lock_or_recover()
        .entry(key)
        .or_default() += 1;
}

fn end_change(key: &str) {
    let mut in_flight = IN_FLIGHT.get_or_init(Default::default).lock_or_recover();
    if let Some(count) = in_flight.get_mut(key) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            in_flight.remove(key);
        }
    }
}

fn in_flight(key: &str) -> bool {
    IN_FLIGHT
        .get_or_init(Default::default)
        .lock_or_recover()
        .contains_key(key)
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

/// Watch each directory that exists; a missing `.hotsheet2` is watched through its
/// parent until it appears.
fn add_watches(
    watcher: &mut notify::RecommendedWatcher,
    watched: &mut Vec<PathBuf>,
    paths: &[PathBuf],
) {
    use notify::{RecursiveMode, Watcher};
    for path in paths {
        if watched.contains(path) {
            continue;
        }
        let target = if path.is_dir() {
            path.clone()
        } else {
            match path.parent() {
                Some(parent) if parent.is_dir() => parent.to_path_buf(),
                _ => continue,
            }
        };
        if watcher.watch(&target, RecursiveMode::NonRecursive).is_ok() && target == *path {
            watched.push(path.clone());
        }
    }
}

/// Announce each change of the owner's settings fingerprint the watched files show.
fn follow(
    state: &AppState,
    runtime: &Arc<OrchestrationRuntime>,
    owner: &str,
    key: &str,
    paths: &[PathBuf],
    (mut watcher, mut watched): (notify::RecommendedWatcher, Vec<PathBuf>),
    rx: &std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
) {
    while !state.is_stopping() {
        match rx.recv_timeout(STOP_CHECK) {
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
        // Let the rest of this write arrive, then read once. HS2-2EKDSM: while the
        // server is making a change itself, wait for it to record what it announces.
        let deferred = std::time::Instant::now();
        while rx.recv_timeout(QUIET).is_ok()
            || (in_flight(key) && deferred.elapsed() < IN_FLIGHT_LIMIT)
        {}
        add_watches(&mut watcher, &mut watched, paths);
        let Ok(fingerprint) = runtime.orchestration_fingerprint(owner) else {
            continue;
        };
        let changed = SEEN
            .get_or_init(Default::default)
            .lock_or_recover()
            .insert(key.to_owned(), fingerprint)
            .is_none_or(|before| before != fingerprint);
        if changed {
            super::orchestration::note_orchestration_change(state, owner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HS2-2EKDSM: changes in flight are counted per owner key, so two overlapping
    /// changes keep the watcher deferring until both are recorded, an end without a
    /// begin changes nothing, and other owners are unaffected.
    #[test]
    fn hs2_2ekdsm_in_flight_changes_are_counted_per_owner() {
        let key = |n: u8| format!("hs2-2ekdsm-test|{n}");
        let begin = |key: &str| {
            *IN_FLIGHT
                .get_or_init(Default::default)
                .lock_or_recover()
                .entry(key.to_owned())
                .or_default() += 1;
        };
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
