//! The always-on **background sync loop** (`docs/02` §2.12, HS2-19 follow-up) — drives
//! `ticketing::sync_once` for every hosted store on a cadence, so a user effectively never
//! runs git by hand. Interval-based **and** event-driven (a server write "kicks" the loop
//! so local changes push promptly), with **exponential backoff** when the remote is
//! offline so an unreachable remote doesn't spin.
//!
//! The pass also covers every Git store linked to a registered checkout, opened or not
//! (HS2-D9ZGAE): after a host restart only the bootstrap store is hosted, and a CLI write
//! to a project nobody has opened must still publish. A CLI write asks for one store with
//! `POST /sync/kick`; that request runs a prompt pass over that store only and leaves the
//! regular pass's schedule alone.
//!
//! The thread body is thin; the decisions worth testing are pulled out as pure functions:
//! [`sync_all`] (one pass over the hosted stores), [`sync_registered`] (the registered
//! stores no client opened), [`next_delay`] (the backoff schedule), and `drive` (when a
//! pass runs and over which stores).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use hotsheet_ticketing::repository_size::{
    GitObjectSize, SizeLevel, crossing_message, git_object_size,
};
use hotsheet_ticketing::sync::{SyncReport, sync_once};
use hotsheet_ticketing::sync_health::SyncState;

use crate::AppState;

/// The base (idle) interval between sync passes, and the ceiling a backoff climbs to.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// A request to the sync loop: every store (a server write), or one store a CLI write
/// committed to (HS2-D9ZGAE). The path is canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncKick {
    All,
    Store(PathBuf),
}

/// How the host answered a request to sync one store (HS2-D9ZGAE).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreSyncRequest {
    /// The sync loop will sync the store promptly.
    Accepted,
    /// The store is neither hosted nor linked to a registered checkout.
    NotRegistered,
    /// This host runs no background sync loop.
    NotRunning,
}

/// The stores one pass covers: every store (the regular pass), or only the stores kicks
/// named since the previous pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassScope {
    All,
    Stores(BTreeSet<PathBuf>),
}

impl PassScope {
    fn empty() -> Self {
        Self::Stores(BTreeSet::new())
    }

    fn add(&mut self, kick: SyncKick) {
        match (&mut *self, kick) {
            (Self::All, _) => {}
            (_, SyncKick::All) => *self = Self::All,
            (Self::Stores(stores), SyncKick::Store(root)) => {
                stores.insert(root);
            }
        }
    }

    /// Whether this pass covers the store at `root` (compared canonically).
    pub fn covers(&self, root: &Path) -> bool {
        match self {
            Self::All => true,
            Self::Stores(stores) => hotsheet_sync::paths::canonicalize(root)
                .is_ok_and(|canonical| stores.contains(&canonical)),
        }
    }
}

/// Keeps the sync-loop thread alive; dropping it lets the thread wind down after its next
/// wake. Holds nothing the caller needs to touch.
pub struct SyncHandle {
    _kick_marker: (),
}

/// Run one sync pass over every hosted store, returning each store's report. A store with
/// no remote reports `NoRemote` (a local-only project) — harmless.
pub fn sync_all(state: &AppState) -> Vec<(String, SyncReport)> {
    sync_all_with_sizes(state, &mut SizeLevels::default(), git_object_size)
}

/// [`sync_all`], announcing every hosted store whose git object size rose across a GitHub
/// limit since the previous pass that reached its remote (HS2-KKFEY5). `levels` remembers each store's last level between passes;
/// a store seen for the first time is measured before its sync, so the first push after a
/// server start is compared against the size it started from. `measure` is injected so
/// tests can script sizes.
pub fn sync_all_with_sizes(
    state: &AppState,
    levels: &mut SizeLevels,
    measure: impl Fn(&Path) -> Option<GitObjectSize>,
) -> Vec<(String, SyncReport)> {
    sync_hosted(state, levels, measure, &PassScope::All)
}

/// [`sync_all_with_sizes`] over the hosted stores `scope` covers.
fn sync_hosted(
    state: &AppState,
    levels: &mut SizeLevels,
    measure: impl Fn(&Path) -> Option<GitObjectSize>,
    scope: &PassScope,
) -> Vec<(String, SyncReport)> {
    let Some(_lifecycle_guard) = state.begin_background_work() else {
        return Vec::new();
    };
    // Publish the server's queued background commits, not a half-committed store
    // (HS2-0DH8ZS).
    hotsheet_ticketing::flush_background_commits();
    state
        .hosted_store_roots()
        .into_iter()
        .filter(|(_, root)| scope.covers(Path::new(root)))
        .map(|(id, root)| {
            let path = Path::new(&root);
            if !levels.contains(&id) {
                levels.observe(&id, &root, measure(path));
            }
            // HS2-ADHFKZ: tell open clients when this store's sync health changes state
            // (for example offline -> rejected, or rejected -> synced), not on every pass.
            let record = hotsheet_ticketing::sync_health::read_record(path);
            let before = record.as_ref().and_then(|record| record.outcome);
            let recovered = record.and_then(|record| record.stale_lock_recovery);
            let report = sync_once(path);
            log_stale_lock_recovery(path, recovered.as_ref());
            let after = SyncState::from(report);
            // Offline (and a Git lock, HS2-6ACT5Y) becomes banner-worthy only once it
            // persists, so that crossing is a change too.
            let offline_became_persistent =
                matches!(after, SyncState::Offline | SyncState::Blocked)
                    && hotsheet_ticketing::sync_health::read_record(path).is_some_and(|record| {
                        record.consecutive_failures
                            == hotsheet_ticketing::sync_health::PERSISTENT_OFFLINE_FAILURES
                    });
            if report != SyncReport::NoRemote
                && (before != Some(after) || offline_became_persistent)
            {
                state.announce_ticket_store_sync(&id, path, after);
            }
            // HS2-QR1K8R: a stall sync cannot finish alone asks an AI (once per stall); a
            // pass that publishes again closes the ask.
            if report != SyncReport::NoRemote {
                crate::sync_ai::ask_after_pass(state, &id, path);
                // HS2-1639Q0: a claim this host lost to another machine, found by the sync
                // that just ran, is pushed to the losing session at once.
                tell_losing_sessions(state, path);
            }
            // Measure after every pass that reached the remote. `UpToDate` matters too: the
            // store's best-effort autocommit push can publish growth between passes, leaving
            // this pass nothing to push.
            if matches!(report, SyncReport::Synced { .. } | SyncReport::UpToDate)
                && let Some(crossing) = levels.observe(&id, &root, measure(path))
            {
                tracing::warn!(store = %root, "{}", crossing.message);
                state.announce_repository_size(&id, path, crossing.level, &crossing.message);
            }
            (id, report)
        })
        .collect()
}

/// Log the stale Git locks a pass moved aside (HS2-T0V269): the store's activity line for
/// a recovery, beside the health reason that names them. `before` is the recorded recovery
/// from before the pass, so an older one is not logged again.
fn log_stale_lock_recovery(
    store: &Path,
    before: Option<&hotsheet_ticketing::sync_health::StaleLockRecovery>,
) {
    if let Some(recovery) = hotsheet_ticketing::sync_health::read_record(store)
        .and_then(|record| record.stale_lock_recovery)
        .filter(|recovery| Some(recovery) != before)
    {
        tracing::warn!(
            store = %store.display(),
            "{}",
            hotsheet_ticketing::sync_stale_locks::moved_line(&recovery.moved)
        );
    }
}

/// Sync every Git store `scope` covers that is linked to a registered checkout but not
/// hosted (HS2-D9ZGAE): the stores of projects no client opened since the host started.
/// The pass reads the checkout registry only; it never opens a project, and announces
/// nothing because no client holds the store. Each store's outcome is still recorded in
/// its durable sync health, which `hotsheet-cli sync --status` and a later project open
/// read. Reports are keyed by the store's canonical path.
pub fn sync_registered(state: &AppState, scope: &PassScope) -> Vec<(String, SyncReport)> {
    let Some(_lifecycle_guard) = state.begin_background_work() else {
        return Vec::new();
    };
    let hosted: Vec<PathBuf> = state
        .hosted_store_roots()
        .into_iter()
        .filter_map(|(_, root)| hotsheet_sync::paths::canonicalize(Path::new(&root)).ok())
        .collect();
    state
        .registered_git_store_roots()
        .into_iter()
        .filter(|root| !hosted.contains(root) && scope.covers(root))
        .map(|root| {
            let record = hotsheet_ticketing::sync_health::read_record(&root);
            let before = record.as_ref().and_then(|record| record.outcome);
            let recovered = record.and_then(|record| record.stale_lock_recovery);
            let report = sync_once(&root);
            log_stale_lock_recovery(&root, recovered.as_ref());
            let after = SyncState::from(report);
            if report != SyncReport::NoRemote && before != Some(after) {
                tracing::info!(store = %root.display(), state = ?after, "registered store sync");
            }
            (root.display().to_string(), report)
        })
        .collect()
}

/// Publish and refresh the group registry of every checkout linked to a hosted store
/// (HS2-9EGWC4): commit registry changes no write committed (Git was busy, or a hook
/// refused), fetch the branch's upstream so groups made in other clones appear, and push
/// unpublished registry commits. Each checkout's status is recorded beside its registry.
pub fn sync_group_registries(
    state: &AppState,
) -> Vec<(
    String,
    hotsheet_ticketing::groups::git_sync::RegistrySyncStatus,
)> {
    let Some(_lifecycle_guard) = state.begin_background_work() else {
        return Vec::new();
    };
    hotsheet_ticketing::groups::git_sync::flush_background();
    state
        .group_registry_checkout_roots()
        .into_iter()
        .map(|root| {
            let status = hotsheet_ticketing::groups::git_sync::sync_checkout(Path::new(&root));
            if status.state.needs_attention() {
                tracing::warn!(checkout = %root, "group registry: {}", status.detail);
            }
            (root, status)
        })
        .collect()
}

/// A store's git object size rising past a GitHub limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeCrossing {
    pub level: SizeLevel,
    pub message: String,
}

/// The last measured [`SizeLevel`] of each hosted store, kept by the sync loop so a store
/// is announced once per rise rather than on every pass (HS2-KKFEY5).
#[derive(Debug, Default)]
pub struct SizeLevels(HashMap<String, SizeLevel>);

impl SizeLevels {
    /// Whether `store` has a recorded level.
    pub fn contains(&self, store: &str) -> bool {
        self.0.contains_key(store)
    }

    /// Record `size` for `store` and return the crossing when the level rose past a limit.
    /// The first measurement only seeds the level; an unmeasurable store keeps its old one.
    /// A drop lowers the level, so a later rise is announced again.
    pub fn observe(
        &mut self,
        store: &str,
        label: &str,
        size: Option<GitObjectSize>,
    ) -> Option<SizeCrossing> {
        let size = size?;
        let level = SizeLevel::for_bytes(size.total_bytes);
        let before = self.0.insert(store.to_string(), level)?;
        crossing_message(label, before, size).map(|message| SizeCrossing { level, message })
    }
}

/// The delay before the next pass: the base interval when all is well, else an
/// exponentially-backed-off delay (capped) while any store is `Offline`, `Rejected`, or
/// `Blocked` (HS2-ADHFKZ: a refused push re-sends the same history, so it must not hammer
/// the remote at the base cadence; HS2-6ACT5Y: a stale Git lock stays until someone removes
/// it). A `Conflict` or stalled rebase is a user-action-needed state
/// whose next pass is a cheap local check, so it does **not** back off.
pub fn next_delay(base: Duration, current: Duration, reports: &[(String, SyncReport)]) -> Duration {
    let any_offline = reports.iter().any(|(_, r)| {
        matches!(
            r,
            SyncReport::Offline | SyncReport::Rejected | SyncReport::Blocked
        )
    });
    if any_offline {
        (current * 2).clamp(base, MAX_BACKOFF)
    } else {
        base
    }
}

/// Push each pending lost-claim conflict of `store_root` to its losing session. A pass runs on
/// the loop thread and from async routes, so the terminal lookup is spawned on the server's
/// runtime instead of blocking the pass (a `block_on` here panics inside a runtime).
fn tell_losing_sessions(state: &AppState, store_root: &std::path::Path) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let project = crate::sync_ai::project_root(state, store_root);
    let state = state.clone();
    let store_root = store_root.to_path_buf();
    handle.spawn(async move {
        crate::claim_conflict_push::after_pass(&state, &project, &store_root).await;
    });
}

/// Spawn the background sync loop. The returned handle keeps it running; a write on the
/// server sends a "kick" (via [`AppState`]) to wake the loop early for a prompt push.
pub fn spawn_sync_loop(state: AppState, base: Duration) -> SyncHandle {
    let (tx, rx) = std::sync::mpsc::channel::<SyncKick>();
    state.set_sync_kicker(tx);
    // The loop thread enters the server's runtime (when there is one) so it can await the
    // terminal broker for a lost-claim push (HS2-1639Q0).
    let runtime = tokio::runtime::Handle::try_current().ok();
    std::thread::spawn(move || {
        let _entered = runtime.as_ref().map(tokio::runtime::Handle::enter);
        run(state, base, rx);
    });
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

fn run(state: AppState, base: Duration, rx: Receiver<SyncKick>) {
    let mut last_trash_purge: Option<std::time::Instant> = None;
    let mut levels = SizeLevels::default();
    let mut last_registry_sync: Option<std::time::Instant> = None;
    drive(base, &mut ChannelWaker::new(rx), |scope| {
        if *scope != PassScope::All {
            // A CLI write's prompt pass syncs the stores it named and nothing else.
            let mut reports = sync_hosted(&state, &mut levels, git_object_size, scope);
            reports.extend(sync_registered(&state, scope));
            return reports;
        }
        if last_trash_purge.is_none_or(|at| at.elapsed() >= TRASH_PURGE_INTERVAL) {
            purge_all_trash(&state);
            last_trash_purge = Some(std::time::Instant::now());
        }
        let mut reports = sync_all_with_sizes(&state, &mut levels, git_object_size);
        reports.extend(sync_registered(&state, scope));
        // Ticket writes kick the loop often; code checkouts are fetched on the base cadence
        // only. Registry writes publish themselves as they happen (HS2-9EGWC4).
        // A second of slack keeps a timer pass that wakes just early on schedule.
        let registry_interval = base.saturating_sub(Duration::from_secs(1));
        if last_registry_sync.is_none_or(|at| at.elapsed() >= registry_interval) {
            sync_group_registries(&state);
            last_registry_sync = Some(std::time::Instant::now());
        }
        reports
    });
}

/// What ended a wait in [`drive`].
#[derive(Clone, Debug, Eq, PartialEq)]
enum Wake {
    Kick(SyncKick),
    Timeout,
    Disconnected,
}

/// The loop's view of time and kicks, so [`drive`] runs against a virtual clock in tests.
trait Waker {
    /// Time elapsed since the loop started.
    fn now(&self) -> Duration;
    /// Block until a kick arrives or `deadline` (on the [`Waker::now`] clock) passes.
    fn wait_until(&mut self, deadline: Duration) -> Wake;
    /// Take every kick already queued (they coalesce into the pass about to run).
    fn drain(&mut self) -> Vec<SyncKick>;
}

struct ChannelWaker {
    rx: Receiver<SyncKick>,
    started: std::time::Instant,
}

impl ChannelWaker {
    fn new(rx: Receiver<SyncKick>) -> Self {
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
            Ok(kick) => Wake::Kick(kick),
            Err(RecvTimeoutError::Timeout) => Wake::Timeout,
            Err(RecvTimeoutError::Disconnected) => Wake::Disconnected,
        }
    }
    fn drain(&mut self) -> Vec<SyncKick> {
        self.rx.try_iter().collect()
    }
}

/// The loop core (HS2-2YSMCW). Each regular pass covers every store and schedules the next
/// by [`next_delay`]. While the cadence is healthy (`delay == base`) a kick (a local write)
/// runs a pass immediately so the change pushes promptly: a server write's kick runs a
/// regular pass, and a CLI write's kick for one store (HS2-D9ZGAE) runs a pass over only
/// the stores kicked since the previous pass, which leaves the regular pass's deadline
/// where it was. A kicked pass that finds its remote unreachable backs off like a regular
/// one. While backed off, a kick does **not** reset the backoff or cut the wait short:
/// kicks are coalesced and the regular pass runs at the backoff deadline, so a burst of
/// local writes cannot make an unreachable remote spin. Kicks taken when the regular pass
/// is due fold into it. The loop ends when every kicker is gone.
fn drive<W: Waker>(
    base: Duration,
    waker: &mut W,
    mut pass: impl FnMut(&PassScope) -> Vec<(String, SyncReport)>,
) {
    let mut delay = base;
    let mut scope = PassScope::All;
    let mut deadline = Duration::ZERO;
    loop {
        let reports = pass(&scope);
        if scope == PassScope::All {
            delay = next_delay(base, delay, &reports);
            deadline = waker.now() + delay;
        } else {
            // A kicked pass keeps the regular deadline; only an unreachable remote moves
            // the loop into backoff, which ignores kicks until that deadline.
            delay = next_delay(base, delay, &reports).max(delay);
        }
        let mut next = PassScope::empty();
        loop {
            match waker.wait_until(deadline) {
                Wake::Kick(kick) if delay <= base => {
                    next.add(kick);
                    break;
                }
                // Backed off: remember nothing beyond "a pass is due"; the deadline pass
                // carries the pending local changes.
                Wake::Kick(_) => {}
                Wake::Timeout => {
                    next = PassScope::All;
                    break;
                }
                Wake::Disconnected => return,
            }
        }
        // Drain any coalesced kicks so a burst of writes is one pass.
        for kick in waker.drain() {
            next.add(kick);
        }
        if waker.now() >= deadline {
            next = PassScope::All;
        }
        scope = next;
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
        fn drain(&mut self) -> Vec<SyncKick> {
            let mut kicks = Vec::new();
            while let Some((at, Wake::Kick(_))) = self.events.front()
                && *at <= self.now.get()
            {
                if let Some((_, Wake::Kick(kick))) = self.events.pop_front() {
                    kicks.push(kick);
                }
            }
            kicks
        }
    }

    const BASE: Duration = Duration::from_secs(30);

    /// Drive the loop over a scripted event list; `report(n)` is pass n's result. Returns
    /// the virtual second each pass ran at. Every script must end in a disconnect.
    fn pass_times(events: &[(u64, Wake)], report: impl Fn(usize) -> SyncReport) -> Vec<u64> {
        scoped_passes(events, report)
            .into_iter()
            .map(|(at, _)| at)
            .collect()
    }

    /// [`pass_times`] with each pass's scope.
    fn scoped_passes(
        events: &[(u64, Wake)],
        report: impl Fn(usize) -> SyncReport,
    ) -> Vec<(u64, PassScope)> {
        let now = Rc::new(Cell::new(Duration::ZERO));
        let mut waker = FakeWaker {
            now: now.clone(),
            events: events
                .iter()
                .map(|(at, wake)| (Duration::from_secs(*at), wake.clone()))
                .collect(),
        };
        let mut passes = Vec::new();
        drive(BASE, &mut waker, |scope| {
            let n = passes.len();
            passes.push((now.get().as_secs(), scope.clone()));
            assert!(n < 100, "loop did not stop");
            vec![("s".into(), report(n))]
        });
        passes
    }

    use Wake::Disconnected as Gone;
    /// A server write's kick: every store.
    const KICK: Wake = Wake::Kick(SyncKick::All);

    /// A CLI write's kick for one store (HS2-D9ZGAE).
    fn store_kick(name: &str) -> Wake {
        Wake::Kick(SyncKick::Store(PathBuf::from(name)))
    }

    fn stores(names: &[&str]) -> PassScope {
        PassScope::Stores(names.iter().map(PathBuf::from).collect())
    }

    /// HS2-D9ZGAE: a CLI kick runs a prompt pass over its store only and keeps the regular
    /// cadence; kicks for several stores in one burst are one pass over those stores; a
    /// repeated kick for one store is one pass; a server write's kick in the same burst
    /// widens it to every store; and a kick taken when the regular pass is due folds into
    /// it.
    #[test]
    fn hs2_d9zgae_store_kicks_coalesce_and_keep_the_regular_cadence() {
        let passes = scoped_passes(
            &[
                (10, store_kick("/a")),
                (10, store_kick("/a")),
                (12, store_kick("/a")),
                (12, store_kick("/b")),
                (12, store_kick("/a")),
                (15, store_kick("/c")),
                (15, KICK),
                (75, store_kick("/d")),
                (95, Gone),
            ],
            |_| SyncReport::UpToDate,
        );
        assert_eq!(
            passes,
            [
                (0, PassScope::All),
                (10, stores(&["/a"])),
                (12, stores(&["/a", "/b"])),
                (15, PassScope::All),
                // The regular pass is 30 s after the last regular pass, not the last kick.
                (45, PassScope::All),
                (75, PassScope::All),
            ],
            "the kick at 75 s arrives exactly when the regular pass is due"
        );
        let times = pass_times(
            &[(10, store_kick("/a")), (20, store_kick("/a")), (35, Gone)],
            |_| SyncReport::UpToDate,
        );
        assert_eq!(
            times,
            [0, 10, 20, 30],
            "store kicks never move the 30 s pass"
        );
    }

    /// HS2-D9ZGAE: a kicked pass that finds its remote offline backs the loop off, so more
    /// CLI writes to that store cannot re-run Git at the write rate; the backoff deadline
    /// pass covers every store and returns to the base cadence once it is healthy.
    #[test]
    fn hs2_d9zgae_an_offline_store_kick_backs_off_until_the_regular_pass() {
        let report = |n: usize| match n {
            1 => SyncReport::Offline,
            _ => SyncReport::UpToDate,
        };
        let passes = scoped_passes(
            &[
                (10, store_kick("/a")),
                (11, store_kick("/a")),
                (20, store_kick("/a")),
                (40, store_kick("/a")),
                (65, Gone),
            ],
            report,
        );
        assert_eq!(
            passes,
            [
                (0, PassScope::All),
                (10, stores(&["/a"])),
                (30, PassScope::All),
                (40, stores(&["/a"])),
                (60, PassScope::All),
            ]
        );
    }

    #[test]
    fn healthy_kick_runs_an_immediate_pass_and_timeouts_keep_the_base_cadence() {
        let times = pass_times(&[(10, KICK), (75, Gone)], |_| SyncReport::UpToDate);
        assert_eq!(times, [0, 10, 40, 70]);
    }

    #[test]
    fn kick_during_backoff_neither_resets_nor_shortens_the_backoff() {
        // Offline: passes at 0, +60, +120, +240 (cap 300). Kicks mid-backoff must not
        // pull a pass forward or restart the doubling from base.
        let times = pass_times(
            &[(5, KICK), (70, KICK), (71, KICK), (200, KICK), (500, Gone)],
            |_| SyncReport::Offline,
        );
        assert_eq!(times, [0, 60, 180, 420]);
    }

    #[test]
    fn a_burst_of_kicks_while_healthy_coalesces_into_one_pass() {
        let times = pass_times(
            &[(10, KICK), (10, KICK), (10, KICK), (10, KICK), (35, Gone)],
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
        let times = pass_times(&[(100, KICK), (190, KICK), (260, Gone)], report);
        assert_eq!(times, [0, 60, 180, 190, 220]);
    }

    /// HS2-6ACT5Y: repeated passes over a stale Git lock back off like an outage instead of
    /// re-running Git at the base cadence, and the pass after the lock is removed restores it.
    #[test]
    fn hs2_6act5y_blocked_passes_back_off_until_the_lock_is_removed() {
        let report = |n: usize| match n {
            0..=2 => SyncReport::Blocked,
            _ => SyncReport::UpToDate,
        };
        let times = pass_times(&[(500, Gone)], report);
        assert_eq!(times, [0, 60, 180, 420, 450, 480]);
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
        // HS2-ADHFKZ: a refused push backs off like offline; a stalled rebase does not.
        let rejected = vec![("s".into(), SyncReport::Rejected)];
        assert_eq!(next_delay(base, base * 2, &rejected), base * 4);
        let stalled = vec![("s".into(), SyncReport::RebaseInProgress)];
        assert_eq!(next_delay(base, base * 2, &stalled), base);
        // HS2-6ACT5Y: a Git lock backs off pass after pass, capped, and a pass that syncs
        // after the lock is removed returns to the base cadence.
        let blocked = vec![("s".into(), SyncReport::Blocked)];
        assert_eq!(next_delay(base, base, &blocked), base * 2);
        assert_eq!(next_delay(base, base * 2, &blocked), base * 4);
        assert_eq!(next_delay(base, MAX_BACKOFF, &blocked), MAX_BACKOFF);
        let mixed = vec![
            ("a".into(), SyncReport::UpToDate),
            ("b".into(), SyncReport::Blocked),
        ];
        assert_eq!(next_delay(base, base, &mixed), base * 2);
        let recovered = vec![(
            "s".into(),
            SyncReport::Synced {
                pulled: true,
                pushed: false,
            },
        )];
        assert_eq!(next_delay(base, MAX_BACKOFF, &recovered), base);
    }

    /// HS2-KKFEY5: the loop announces each rise across a limit exactly once, re-announces
    /// a rise after a drop, and never announces a first (seeding) measurement.
    #[test]
    fn size_levels_announce_each_rise_once_across_the_transition_matrix() {
        use hotsheet_ticketing::repository_size::{CRITICAL_BYTES, WARNING_BYTES};
        let size = |total_bytes| {
            Some(GitObjectSize {
                total_bytes,
                ..GitObjectSize::default()
            })
        };
        let mut levels = SizeLevels::default();
        assert!(!levels.contains("s"));
        // Seeding never announces, even above a limit.
        assert_eq!(levels.observe("s", "store", size(WARNING_BYTES)), None);
        assert!(levels.contains("s"));
        let mut fresh = SizeLevels::default();
        assert_eq!(fresh.observe("t", "other", size(1)), None);

        let steps = [
            (WARNING_BYTES + 1, None),                       // warning → warning
            (CRITICAL_BYTES, Some(SizeLevel::Critical)),     // warning → critical
            (CRITICAL_BYTES + 1, None),                      // critical → critical
            (1, None),                                       // drop to ok
            (WARNING_BYTES, Some(SizeLevel::Warning)),       // ok → warning again
            (CRITICAL_BYTES * 2, Some(SizeLevel::Critical)), // warning → critical again
        ];
        for (bytes, expected) in steps {
            let crossing = levels.observe("s", "store", size(bytes));
            assert_eq!(
                crossing.as_ref().map(|c| c.level),
                expected,
                "{bytes} bytes"
            );
            if let Some(crossing) = crossing {
                assert!(crossing.message.contains("store"), "{}", crossing.message);
            }
        }
        // An unmeasurable pass keeps the level, so it neither announces nor resets.
        assert_eq!(levels.observe("s", "store", None), None);
        assert_eq!(levels.observe("s", "store", size(CRITICAL_BYTES * 3)), None);
        // From ok, a jump straight to critical is one announcement.
        assert_eq!(
            fresh
                .observe("t", "other", size(CRITICAL_BYTES))
                .map(|c| c.level),
            Some(SizeLevel::Critical)
        );
    }

    #[test]
    fn shared_project_retention_drives_a_hosted_store_sweep_policy() {
        let store_root = tempfile::tempdir().unwrap();
        let store = hotsheet_ticketing::FsStore::init_git(
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
