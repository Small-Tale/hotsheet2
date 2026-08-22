//! The always-on **background sync loop** (`docs/02` §2.12, HS2-19 follow-up) — drives
//! `ticketing::sync_once` for every hosted store on a cadence, so a user effectively never
//! runs git by hand. Interval-based **and** event-driven (a server write "kicks" the loop
//! so local changes push promptly), with **exponential backoff** when the remote is
//! offline so an unreachable remote doesn't spin.
//!
//! The thread body is thin; the two decisions worth testing are pulled out as pure
//! functions: [`sync_all`] (one pass over the hosted stores) and [`next_delay`] (the
//! backoff schedule).

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use hotsheet_ticketing::sync::{SyncReport, sync_once};

use crate::AppState;

/// The base (idle) interval between sync passes, and the ceiling a backoff climbs to.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// Keeps the sync-loop thread alive; dropping it lets the thread wind down after its next
/// wake. Holds nothing the caller needs to touch.
pub struct SyncHandle {
    _kick_marker: (),
}

/// Run one sync pass over every hosted store, returning each store's report. A store with
/// no remote reports `NoRemote` (a local-only project) — harmless.
pub fn sync_all(state: &AppState) -> Vec<(String, SyncReport)> {
    state
        .hosted_store_roots()
        .into_iter()
        .map(|(id, root)| (id, sync_once(std::path::Path::new(&root))))
        .collect()
}

/// The delay before the next pass: the base interval when all is well, else an
/// exponentially-backed-off delay (capped) while any store is `Offline`. A `Conflict` is a
/// user-action-needed state, not a transient one, so it does **not** back off (the next
/// pass re-checks cheaply once the user resolves it).
pub fn next_delay(base: Duration, current: Duration, reports: &[(String, SyncReport)]) -> Duration {
    let any_offline = reports.iter().any(|(_, r)| *r == SyncReport::Offline);
    if any_offline {
        (current * 2).clamp(base, MAX_BACKOFF)
    } else {
        base
    }
}

/// Spawn the background sync loop. The returned handle keeps it running; a write on the
/// server sends a "kick" (via [`AppState`]) to wake the loop early for a prompt push.
pub fn spawn_sync_loop(state: AppState, base: Duration) -> SyncHandle {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    state.set_sync_kicker(tx);
    std::thread::spawn(move || run(state, base, rx));
    SyncHandle { _kick_marker: () }
}

fn run(state: AppState, base: Duration, rx: Receiver<()>) {
    let mut delay = base;
    loop {
        let reports = sync_all(&state);
        delay = next_delay(base, delay, &reports);
        // Wait for the next tick OR a kick (a local write) — whichever comes first. A
        // kick resets to the base cadence so a fresh change pushes promptly.
        match rx.recv_timeout(delay) {
            Ok(()) => {
                delay = base;
                // Drain any coalesced kicks so a burst of writes is one pass.
                while rx.try_recv().is_ok() {}
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return, // AppState gone → stop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_delay_backs_off_on_offline_and_resets_otherwise() {
        let base = Duration::from_secs(30);
        let off = vec![("s".into(), SyncReport::Offline)];
        // Offline doubles, from the current delay, capped.
        assert_eq!(next_delay(base, base, &off), base * 2);
        assert_eq!(next_delay(base, base * 2, &off), base * 4);
        assert_eq!(next_delay(base, MAX_BACKOFF, &off), MAX_BACKOFF, "capped");

        // A healthy pass returns to the base cadence.
        let ok = vec![("s".into(), SyncReport::UpToDate)];
        assert_eq!(next_delay(base, MAX_BACKOFF, &ok), base);
        // A conflict is not transient → no backoff.
        let conflict = vec![("s".into(), SyncReport::Conflict)];
        assert_eq!(next_delay(base, base, &conflict), base);
    }
}
