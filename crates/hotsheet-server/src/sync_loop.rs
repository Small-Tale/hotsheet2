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
    let Some(_lifecycle_guard) = state.begin_background_work() else {
        return Vec::new();
    };
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

/// How often the loop sweeps Trash for tickets past retention. The sweep is local and
/// bounded; a day-granularity retention does not need a tighter cadence.
pub const TRASH_PURGE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Purge every hosted store's Trash of tickets deleted beyond its effective project
/// `trash_cleanup_days` setting (30 days by default). Returns how many
/// tickets each store purged; a store that fails to open or purge is skipped and retried
/// on the next sweep.
pub fn purge_all_trash(state: &AppState) -> Vec<(String, usize)> {
    let Some(_lifecycle_guard) = state.begin_background_work() else {
        return Vec::new();
    };
    let now = crate::now();
    state
        .hosted_store_roots()
        .into_iter()
        .filter_map(|(id, root)| {
            let retention_days = state.trash_cleanup_days_for_store(std::path::Path::new(&root));
            let store = hotsheet_ticketing::FsStore::open(root).ok()?;
            let purged =
                hotsheet_ticketing::ops::purge_trash(&store, &now, i64::from(retention_days))
                    .ok()?;
            Some((id, purged.len()))
        })
        .collect()
}

fn run(state: AppState, base: Duration, rx: Receiver<()>) {
    let mut last_trash_purge: Option<std::time::Instant> = None;
    drive(base, &mut ChannelWaker::new(rx), || {
        if last_trash_purge.is_none_or(|at| at.elapsed() >= TRASH_PURGE_INTERVAL) {
            purge_all_trash(&state);
            last_trash_purge = Some(std::time::Instant::now());
        }
        sync_all(&state)
    });
}

/// What ended a wait in [`drive`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Wake {
    Kick,
    Timeout,
    Disconnected,
}

/// The loop's view of time and kicks, so [`drive`] runs against a virtual clock in tests.
trait Waker {
    /// Time elapsed since the loop started.
    fn now(&self) -> Duration;
    /// Block until a kick arrives or `deadline` (on the [`Waker::now`] clock) passes.
    fn wait_until(&mut self, deadline: Duration) -> Wake;
    /// Discard every kick already queued (they coalesce into the pass about to run).
    fn drain(&mut self);
}

struct ChannelWaker {
    rx: Receiver<()>,
    started: std::time::Instant,
}

impl ChannelWaker {
    fn new(rx: Receiver<()>) -> Self {
        Self {
            rx,
            started: std::time::Instant::now(),
        }
    }
}

impl Waker for ChannelWaker {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
    fn wait_until(&mut self, deadline: Duration) -> Wake {
        match self.rx.recv_timeout(deadline.saturating_sub(self.now())) {
            Ok(()) => Wake::Kick,
            Err(RecvTimeoutError::Timeout) => Wake::Timeout,
            Err(RecvTimeoutError::Disconnected) => Wake::Disconnected,
        }
    }
    fn drain(&mut self) {
        while self.rx.try_recv().is_ok() {}
    }
}

/// The loop core (HS2-2YSMCW). Each pass schedules the next by [`next_delay`]. While the
/// cadence is healthy (`delay == base`) a kick (a local write) runs a pass immediately so
/// the change pushes promptly. While backed off for an offline remote, a kick does **not**
/// reset the backoff or cut the wait short: kicks are coalesced and the pass runs at the
/// backoff deadline, so a burst of local writes cannot make an unreachable remote spin.
/// The loop ends when every kicker is gone.
fn drive<W: Waker>(
    base: Duration,
    waker: &mut W,
    mut pass: impl FnMut() -> Vec<(String, SyncReport)>,
) {
    let mut delay = base;
    loop {
        let reports = pass();
        delay = next_delay(base, delay, &reports);
        let deadline = waker.now() + delay;
        loop {
            match waker.wait_until(deadline) {
                Wake::Kick if delay <= base => break,
                // Backed off: remember nothing beyond "a pass is due"; the deadline pass
                // carries the pending local changes.
                Wake::Kick => {}
                Wake::Timeout => break,
                Wake::Disconnected => return,
            }
        }
        // Drain any coalesced kicks so a burst of writes is one pass.
        waker.drain();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    /// A virtual clock with scripted kicks/disconnects at absolute virtual seconds.
    struct FakeWaker {
        now: Rc<Cell<Duration>>,
        events: VecDeque<(Duration, Wake)>,
    }

    impl Waker for FakeWaker {
        fn now(&self) -> Duration {
            self.now.get()
        }
        fn wait_until(&mut self, deadline: Duration) -> Wake {
            if let Some((at, _)) = self.events.front()
                && *at <= deadline
            {
                let (at, wake) = self.events.pop_front().unwrap();
                self.now.set(self.now.get().max(at));
                return wake;
            }
            self.now.set(deadline);
            Wake::Timeout
        }
        fn drain(&mut self) {
            while self
                .events
                .front()
                .is_some_and(|(at, wake)| *at <= self.now.get() && *wake == Wake::Kick)
            {
                self.events.pop_front();
            }
        }
    }

    const BASE: Duration = Duration::from_secs(30);

    /// Drive the loop over a scripted event list; `report(n)` is pass n's result. Returns
    /// the virtual second each pass ran at. Every script must end in a disconnect.
    fn pass_times(events: &[(u64, Wake)], report: impl Fn(usize) -> SyncReport) -> Vec<u64> {
        let now = Rc::new(Cell::new(Duration::ZERO));
        let mut waker = FakeWaker {
            now: now.clone(),
            events: events
                .iter()
                .map(|(at, wake)| (Duration::from_secs(*at), *wake))
                .collect(),
        };
        let mut times = Vec::new();
        drive(BASE, &mut waker, || {
            let n = times.len();
            times.push(now.get().as_secs());
            assert!(n < 100, "loop did not stop");
            vec![("s".into(), report(n))]
        });
        times
    }

    use Wake::{Disconnected as Gone, Kick};

    #[test]
    fn healthy_kick_runs_an_immediate_pass_and_timeouts_keep_the_base_cadence() {
        let times = pass_times(&[(10, Kick), (75, Gone)], |_| SyncReport::UpToDate);
        assert_eq!(times, [0, 10, 40, 70]);
    }

    #[test]
    fn kick_during_backoff_neither_resets_nor_shortens_the_backoff() {
        // Offline: passes at 0, +60, +120, +240 (cap 300). Kicks mid-backoff must not
        // pull a pass forward or restart the doubling from base.
        let times = pass_times(
            &[(5, Kick), (70, Kick), (71, Kick), (200, Kick), (500, Gone)],
            |_| SyncReport::Offline,
        );
        assert_eq!(times, [0, 60, 180, 420]);
    }

    #[test]
    fn a_burst_of_kicks_while_healthy_coalesces_into_one_pass() {
        let times = pass_times(
            &[(10, Kick), (10, Kick), (10, Kick), (10, Kick), (35, Gone)],
            |_| SyncReport::UpToDate,
        );
        assert_eq!(times, [0, 10], "four kicks at once are one pass");
    }

    #[test]
    fn disconnect_stops_the_loop_in_both_healthy_and_backed_off_waits() {
        assert_eq!(pass_times(&[(1, Gone)], |_| SyncReport::UpToDate), [0]);
        assert_eq!(pass_times(&[(100, Gone)], |_| SyncReport::Offline), [0, 60]);
    }

    #[test]
    fn offline_then_online_then_offline_restarts_the_backoff_from_base() {
        // Passes 0-1 offline, 2-3 online, then offline again.
        let report = |n: usize| match n {
            0 | 1 => SyncReport::Offline,
            2 | 3 => SyncReport::UpToDate,
            _ => SyncReport::Offline,
        };
        // 0 off, 60 off, 180 up, a kick at 190 is prompt again, 220 off (backs off from base).
        let times = pass_times(&[(100, Kick), (190, Kick), (260, Gone)], report);
        assert_eq!(times, [0, 60, 180, 190, 220]);
    }

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

    #[test]
    fn shared_project_retention_drives_a_hosted_store_sweep_policy() {
        let store_root = tempfile::tempdir().unwrap();
        let store = hotsheet_ticketing::FsStore::init(
            store_root.path(),
            &hotsheet_ticketing::StoreMetadata::new("HS"),
        )
        .unwrap();
        let checkout_root = tempfile::tempdir().unwrap();
        hotsheet_ticketing::Settings::for_project(checkout_root.path())
            .set(
                hotsheet_ticketing::TRASH_CLEANUP_DAYS_SETTING,
                serde_json::json!(7),
                hotsheet_ticketing::Scope::Shared,
            )
            .unwrap();
        let registry_root = tempfile::tempdir().unwrap();
        let registry_path = registry_root.path().join("checkouts.json");
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry_path)
            .register(
                checkout_root.path(),
                None,
                None,
                vec![store_root.path().to_path_buf()],
            )
            .unwrap();
        let state = AppState::new(store, "secret".into())
            .unwrap()
            .with_checkout_registry(registry_path);

        assert_eq!(state.trash_cleanup_days_for_store(store_root.path()), 7);

        let second_checkout = tempfile::tempdir().unwrap();
        hotsheet_ticketing::Settings::for_project(second_checkout.path())
            .set(
                hotsheet_ticketing::TRASH_CLEANUP_DAYS_SETTING,
                serde_json::json!(45),
                hotsheet_ticketing::Scope::Shared,
            )
            .unwrap();
        state
            .checkout_registry
            .register(
                second_checkout.path(),
                None,
                None,
                vec![store_root.path().to_path_buf()],
            )
            .unwrap();
        assert_eq!(
            state.trash_cleanup_days_for_store(store_root.path()),
            45,
            "a checkout with shorter retention cannot purge a shared store early"
        );
    }
}
