//! Filesystem watcher (HS2-6): store and repository change monitoring.

use crate::events::{
    ChangeEvent, EventLog, INDEX_REPAIR_RETRY_MIN, IndexRepairs, emit_change,
    next_index_repair_delay,
};
use crate::multistore::{StoreEntry, StoreHost};
use crate::{AppState, LocalWriteHashes, multistore};
use hotsheet_index::hash_bytes;
use hotsheet_model::{Ulid, parse_file};
use hotsheet_sync::LockExt;
use std::path::Path as FsPath;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

// ---- filesystem watcher (HS2-6) --------------------------------------------------

/// Keeps the watcher alive; dropping it stops watching.
pub struct WatchHandle {
    _watcher: HeldWatcher,
}

/// Keeps one checkout monitor alive. Initialization and repository inspection happen on
/// its background thread, so opening a project never waits for a recursive traversal.
pub(crate) struct RepositoryWatchHandle {
    stop: std::sync::mpsc::Sender<()>,
}

impl Drop for RepositoryWatchHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

/// Held only for its `Drop`.
enum HeldWatcher {
    Native { _hold: NativeWatcherHold },
    Poll { _watcher: notify::PollWatcher },
}

/// A native watcher started on its own thread; `stopped` tells a still-starting thread to
/// discard the watcher it is creating.
#[derive(Default)]
struct NativeWatcherState {
    watcher: Option<notify::RecommendedWatcher>,
    /// Bridges the native stream's start-up: polls until the stream has been live a moment.
    bridge: Option<notify::PollWatcher>,
    stopped: bool,
}

type NativeWatcherSlot = Arc<Mutex<NativeWatcherState>>;

/// Owns a started (or starting) native watcher; dropping it stops the watcher.
struct NativeWatcherHold(pub(crate) NativeWatcherSlot);

impl Drop for NativeWatcherHold {
    fn drop(&mut self) {
        let watchers = self.0.with_lock(|state| {
            state.stopped = true;
            (state.watcher.take(), state.bridge.take())
        });
        // Stop the stream outside the lock: it waits for the stream's run-loop thread.
        drop(watchers);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WatcherBackend {
    Recommended,
    Poll,
}

/// Watch the store's `tickets/` dir and keep the index + WS bus in sync with changes
/// made outside the server (CLI, `git pull`, another writer). A change whose content
/// hash already matches the index (e.g. the server's own write) is a no-op, so
/// server-driven writes don't double-emit (`docs/03` §3.4).
/// What a watcher thread keeps fresh: one store's entry, its URL id (for tagging change
/// events), and the shared broadcast bus. Store-scoped so the default store and any
/// `POST /stores`-registered store are watched by the same code (HS2-87).
#[derive(Clone)]
pub(crate) struct WatchTarget {
    pub(crate) entry: StoreEntry,
    pub(crate) store_id: String,
    pub(crate) host: StoreHost,
    pub(crate) local_write_hashes: LocalWriteHashes,
    pub(crate) events: broadcast::Sender<ChangeEvent>,
    pub(crate) event_log: Arc<Mutex<EventLog>>,
    pub(crate) checkout_registry: hotsheet_ticketing::checkouts::CheckoutRegistry,
    pub(crate) index_repairs: IndexRepairs,
    pub(crate) worklist_refresher: crate::worklist_refresh::WorklistRefresher,
}

/// Watch the **default** store (back-compat entry point used by the server binary).
pub fn spawn_watcher(state: AppState) -> anyhow::Result<WatchHandle> {
    spawn_watcher_for(default_watch_target(&state), WatcherBackend::Recommended)
}

pub(crate) fn default_watch_target(state: &AppState) -> WatchTarget {
    WatchTarget {
        entry: state.default_entry(),
        store_id: multistore::store_url_id(&state.store),
        host: state.host.clone(),
        local_write_hashes: state.local_write_hashes.clone(),
        events: state.events.clone(),
        event_log: state.event_log.clone(),
        checkout_registry: state.checkout_registry.clone(),
        index_repairs: state.index_repairs.clone(),
        worklist_refresher: state.worklist_refresher.clone(),
    }
}

/// Start the default-store watcher with notify's deterministic polling backend.
///
/// This is a test seam for integration coverage that must coexist with other live macOS
/// FSEvents streams. Production startup continues to use [`spawn_watcher`].
#[doc(hidden)]
pub fn spawn_polling_watcher_for_test(state: AppState) -> anyhow::Result<WatchHandle> {
    spawn_watcher_for(default_watch_target(&state), WatcherBackend::Poll)
}

/// Dynamically registered stores use the native watcher on every platform (HS2-P3SSGR).
/// A native stream drops events from roughly its first 250ms, which is what made the
/// macOS registered-store test fail (HS2-SG1BKJ); the startup resync in
/// [`spawn_watcher_for`] covers that window. The 250ms polling fallback it replaces
/// re-walked every registered store four times a second and kept an idle server busy.
pub(crate) fn registered_watcher_backend() -> WatcherBackend {
    WatcherBackend::Recommended
}

/// Marks the synthetic event that asks a store's watch loop to re-check its tickets once
/// the native stream is live, covering writes made while the stream was starting.
const STARTUP_RESYNC: &str = "hotsheet-startup-resync";
/// Marks the resync after which the watcher is live: once it is applied, every write made
/// before the stream started is in the index, so a slug miss can trust it (HS2-3R0C6G).
const LIVE_RESYNC: &str = "hotsheet-live-resync";
/// When the startup resync runs; a native stream is live well within this (HS2-P3SSGR).
const STARTUP_RESYNC_DELAY: Duration = Duration::from_millis(1500);

/// The startup bridge's polling config. notify's poller compares mtimes in whole seconds, so an
/// edit landing in the same second as the previous scan looks unchanged; comparing contents
/// catches it at the next scan instead of waiting for the native stream, whose start can take
/// several seconds on a busy Mac (HS2-XAHR91). The bridge lives only until that stream is up.
pub(crate) fn startup_bridge_config() -> notify::Config {
    notify::Config::default()
        .with_poll_interval(Duration::from_millis(250))
        .with_compare_contents(true)
}

/// Whether two paths point at the same store root (canonicalized; lexical fallback).
pub(crate) fn same_path(a: &FsPath, b: &FsPath) -> bool {
    let canon = |p: &FsPath| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// Watch one store (any hosted store). The returned [`WatchHandle`] must be kept alive
/// for the watcher to run.
pub(crate) fn spawn_watcher_for(
    target: WatchTarget,
    backend: WatcherBackend,
) -> anyhow::Result<WatchHandle> {
    use notify::{RecursiveMode, Watcher};

    let tickets_dir = target.entry.store.root().join("tickets");
    std::fs::create_dir_all(&tickets_dir)?;

    let freshness = target.entry.freshness.clone();
    let (raw_tx, rx) = std::sync::mpsc::channel();
    // Every queued event counts as a pending reconcile until the loop applies it.
    let tx = QueuedSender {
        tx: raw_tx,
        freshness,
    };
    std::thread::spawn(move || watch_loop(rx, target));
    let watcher = match backend {
        WatcherBackend::Recommended => {
            // Starting a native stream (FSEventStreamStart on macOS) can take seconds under
            // load, and this runs on the project-open path, so the stream starts on its own
            // thread (HS2-P3SSGR). Until it has been live a moment, a short-lived poller bridges
            // the gap so a change made right after the store opens is still seen promptly; the
            // store is re-checked when the bridge starts and again when it is dropped, covering
            // writes made before either watcher took its first look.
            let slot: NativeWatcherSlot = Arc::default();
            let started = slot.clone();
            std::thread::spawn(move || {
                let resync = tx.clone();
                let request_resync = |dir: &FsPath, info: &str| {
                    resync.send(Ok(resync_event(dir, info)));
                };
                let bridge_tx = tx.clone();
                let bridge = notify::PollWatcher::new(
                    move |res| {
                        bridge_tx.send(res);
                    },
                    startup_bridge_config(),
                )
                .and_then(|mut bridge| {
                    bridge.watch(&tickets_dir, RecursiveMode::Recursive)?;
                    Ok(bridge)
                });
                {
                    let mut slot = started.lock_or_recover();
                    if slot.stopped {
                        return;
                    }
                    slot.bridge = bridge.ok();
                }
                request_resync(&tickets_dir, STARTUP_RESYNC);
                let mut watcher = match notify::recommended_watcher(move |res| {
                    tx.send(res);
                }) {
                    Ok(watcher) => watcher,
                    Err(error) => {
                        tracing::warn!(
                            dir = %tickets_dir.display(),
                            %error,
                            "store watcher failed to start"
                        );
                        return;
                    }
                };
                if let Err(error) = watcher.watch(&tickets_dir, RecursiveMode::Recursive) {
                    tracing::warn!(
                        dir = %tickets_dir.display(),
                        %error,
                        "store watcher failed to start"
                    );
                    return;
                }
                {
                    let mut slot = started.lock_or_recover();
                    // The handle was dropped while the stream started: stop it again.
                    if slot.stopped {
                        return;
                    }
                    slot.watcher = Some(watcher);
                }
                std::thread::sleep(STARTUP_RESYNC_DELAY);
                // The native stream is live: retire the bridge and re-check once more.
                let bridge = started.with_lock(|slot| slot.bridge.take());
                drop(bridge);
                request_resync(&tickets_dir, LIVE_RESYNC);
            });
            HeldWatcher::Native {
                _hold: NativeWatcherHold(slot),
            }
        }
        WatcherBackend::Poll => {
            let resync = tx.clone();
            let mut watcher = notify::PollWatcher::new(
                move |res| {
                    tx.send(res);
                },
                notify::Config::default().with_poll_interval(Duration::from_millis(250)),
            )?;
            watcher.watch(&tickets_dir, RecursiveMode::Recursive)?;
            // The poller is scanning: re-check writes made before it started, then go live.
            resync.send(Ok(resync_event(&tickets_dir, LIVE_RESYNC)));
            HeldWatcher::Poll { _watcher: watcher }
        }
    };
    Ok(WatchHandle { _watcher: watcher })
}

/// A watcher-event sender that counts each queued event as a pending reconcile on the
/// store's [`hotsheet_index::IndexFreshness`] until the watch loop applies it.
#[derive(Clone)]
pub(crate) struct QueuedSender {
    pub(crate) tx: std::sync::mpsc::Sender<notify::Result<notify::Event>>,
    pub(crate) freshness: Arc<hotsheet_index::IndexFreshness>,
}

impl QueuedSender {
    pub(crate) fn send(&self, event: notify::Result<notify::Event>) {
        self.freshness.event_queued();
        if self.tx.send(event).is_err() {
            // The loop is gone; nothing will apply it.
            self.freshness.events_applied(1);
        }
    }
}

/// The synthetic event asking the watch loop to re-check every ticket file under `dir`.
fn resync_event(dir: &FsPath, info: &str) -> notify::Event {
    notify::Event::new(notify::EventKind::Other)
        .add_path(dir.to_path_buf())
        .set_info(info)
}

pub(crate) fn repository_change_event(checkout_id: &str) -> ChangeEvent {
    ChangeEvent {
        cursor: None,
        store: checkout_id.to_string(),
        kind: "repository_changed".to_string(),
        id: checkout_id.to_string(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    }
}

/// Monitor a checkout for working-tree, index, commit, and upstream changes. Setup always
/// happens off the request path. Native filesystem events trigger a Git status fingerprint,
/// and only a changed fingerprint is announced, so ignored build output never reaches
/// clients. Clients receive invalidations through WebSocket/long-poll and never simple-poll.
pub(crate) fn spawn_repository_watcher(
    root: std::path::PathBuf,
    checkout_id: String,
    events: broadcast::Sender<ChangeEvent>,
    event_log: Arc<Mutex<EventLog>>,
) -> anyhow::Result<RepositoryWatchHandle> {
    // Native events on every platform (HS2-4S9ENS): a second FSEvents stream works in-process
    // (HS2-P3SSGR), so macOS no longer runs `git status` on a timer. The Git-status poller is
    // the fallback when a native watcher cannot start.
    spawn_repository_monitor(
        root,
        checkout_id,
        events,
        event_log,
        run_native_repository_monitor,
    )
}

/// One checkout monitor's loop: runs on its own thread until the stop channel fires.
type RepositoryMonitor = fn(
    std::path::PathBuf,
    String,
    broadcast::Sender<ChangeEvent>,
    Arc<Mutex<EventLog>>,
    std::sync::mpsc::Receiver<()>,
);

fn spawn_repository_monitor(
    root: std::path::PathBuf,
    checkout_id: String,
    events: broadcast::Sender<ChangeEvent>,
    event_log: Arc<Mutex<EventLog>>,
    monitor: RepositoryMonitor,
) -> anyhow::Result<RepositoryWatchHandle> {
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(format!("hotsheet-repository-{checkout_id}"))
        .spawn(move || monitor(root, checkout_id, events, event_log, stop_rx))?;
    Ok(RepositoryWatchHandle { stop: stop_tx })
}

/// How long filesystem events must pause before the checkout's Git status is re-read.
const REPOSITORY_EVENT_QUIET: Duration = Duration::from_millis(175);
/// The most often a checkout's Git status is re-read while events keep arriving (a build or
/// checkout in progress); matches the old poller's fastest rate, so a busy checkout costs no
/// more than before and an idle one costs nothing.
const REPOSITORY_FINGERPRINT_INTERVAL: Duration = Duration::from_millis(750);
/// A continuous event stream still gets its status re-read this often.
const REPOSITORY_MAX_EVENT_LATENCY: Duration = Duration::from_secs(1);

/// Decides when filesystem events warrant re-reading a checkout's Git status: after a pause in
/// the events, or periodically during a continuous stream, never more often than
/// [`REPOSITORY_FINGERPRINT_INTERVAL`].
#[derive(Debug)]
struct RepositoryRefreshGate {
    first_event: Option<Instant>,
    last_event: Option<Instant>,
    last_fingerprint: Option<Instant>,
}

impl RepositoryRefreshGate {
    fn new() -> Self {
        Self {
            first_event: None,
            last_event: None,
            last_fingerprint: None,
        }
    }

    fn note_event(&mut self, now: Instant) {
        self.first_event.get_or_insert(now);
        self.last_event = Some(now);
    }

    fn due(&self, now: Instant) -> bool {
        let (Some(first), Some(last)) = (self.first_event, self.last_event) else {
            return false;
        };
        let settled = now.duration_since(last) >= REPOSITORY_EVENT_QUIET
            || now.duration_since(first) >= REPOSITORY_MAX_EVENT_LATENCY;
        let rested = self
            .last_fingerprint
            .is_none_or(|at| now.duration_since(at) >= REPOSITORY_FINGERPRINT_INTERVAL);
        settled && rested
    }

    fn fingerprinted(&mut self, now: Instant) {
        self.first_event = None;
        self.last_event = None;
        self.last_fingerprint = Some(now);
    }

    /// How long the monitor may block waiting for the next event.
    fn wait(&self, now: Instant) -> Duration {
        let Some(first) = self.first_event else {
            return Duration::from_millis(250);
        };
        let last = self.last_event.unwrap_or(first);
        let quiet = REPOSITORY_EVENT_QUIET.saturating_sub(now.duration_since(last));
        let latency = REPOSITORY_MAX_EVENT_LATENCY.saturating_sub(now.duration_since(first));
        let rest = self.last_fingerprint.map_or(Duration::ZERO, |at| {
            REPOSITORY_FINGERPRINT_INTERVAL.saturating_sub(now.duration_since(at))
        });
        quiet.min(latency).max(rest).max(Duration::from_millis(10))
    }
}

/// Which filesystem events can change a checkout's Git status. Paths Git ignores (as of the
/// last refresh) and Git's object and log stores are skipped; everything else, including
/// watcher errors and rescan requests, may matter. Lock files are kept: Git updates its index
/// and refs by renaming `*.lock` into place, and FSEvents may report only the lock's path.
#[derive(Debug, Default)]
struct RepositoryEventFilter {
    ignored: Vec<std::path::PathBuf>,
    git_dirs: Vec<std::path::PathBuf>,
}

impl RepositoryEventFilter {
    fn load(root: &FsPath) -> Self {
        let mut filter = Self {
            ignored: Vec::new(),
            git_dirs: repository_git_dirs(root),
        };
        filter.refresh_ignored(root);
        filter
    }

    /// Re-read the ignored paths, so a build directory created after startup stops
    /// triggering status reads once Git has confirmed it ignores it.
    fn refresh_ignored(&mut self, root: &FsPath) {
        let Ok(output) = hotsheet_ticketing::git::command()
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(root)
            .args([
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
                "-z",
            ])
            .output()
        else {
            return;
        };
        if !output.status.success() {
            return;
        }
        let bases = path_spellings(root);
        self.ignored = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                String::from_utf8_lossy(entry)
                    .trim_end_matches('/')
                    .to_string()
            })
            .flat_map(|relative| {
                bases
                    .iter()
                    .map(move |base| base.join(&relative))
                    .collect::<Vec<_>>()
            })
            .collect();
    }

    fn is_relevant(&self, result: &notify::Result<notify::Event>) -> bool {
        let Ok(event) = result else {
            return true;
        };
        if event.need_rescan() || event.paths.is_empty() {
            return true;
        }
        event.paths.iter().any(|path| self.path_is_relevant(path))
    }

    fn path_is_relevant(&self, path: &FsPath) -> bool {
        if self.ignored.iter().any(|ignored| path.starts_with(ignored)) {
            return false;
        }
        for git_dir in &self.git_dirs {
            if let Ok(inside) = path.strip_prefix(git_dir) {
                return !(inside.starts_with("objects") || inside.starts_with("logs"));
            }
        }
        true
    }
}

/// A path as given and canonicalized (macOS reports `/private/var/...` for `/var/...`).
fn path_spellings(path: &FsPath) -> Vec<std::path::PathBuf> {
    let mut spellings = vec![path.to_path_buf()];
    if let Ok(canonical) = path.canonicalize()
        && canonical != path
    {
        spellings.push(canonical);
    }
    spellings
}

/// The checkout's Git directory and common directory (distinct for a linked worktree, whose
/// HEAD, index and refs live outside the working tree), in every spelling.
fn repository_git_dirs(root: &FsPath) -> Vec<std::path::PathBuf> {
    let Ok(output) = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(root)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut dirs: Vec<std::path::PathBuf> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .flat_map(|line| path_spellings(FsPath::new(line)))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Re-read a checkout's Git status and announce it when it differs from the last read.
/// Returns whether it changed.
fn announce_repository_change(
    root: &FsPath,
    previous: &mut Option<Vec<u8>>,
    checkout_id: &str,
    events: &broadcast::Sender<ChangeEvent>,
    event_log: &Arc<Mutex<EventLog>>,
) -> Option<bool> {
    let next = git_repository_fingerprint(root)?;
    let changed = previous.as_ref().is_some_and(|prior| prior != &next);
    *previous = Some(next);
    if changed {
        emit_change(event_log, events, repository_change_event(checkout_id));
    }
    Some(changed)
}

fn run_native_repository_monitor(
    root: std::path::PathBuf,
    checkout_id: String,
    events: broadcast::Sender<ChangeEvent>,
    event_log: Arc<Mutex<EventLog>>,
    stop: std::sync::mpsc::Receiver<()>,
) {
    use notify::{RecursiveMode, Watcher};

    let mut previous = git_repository_fingerprint(&root);
    let mut filter = RepositoryEventFilter::load(&root);
    let (tx, rx) = std::sync::mpsc::channel();
    // Starting a native stream can take many seconds on a busy Mac (HS2-XAHR91), so it starts
    // on its own thread while this one keeps the old Git-status poll going as a bridge.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let watch_root = root.clone();
    let git_dirs = filter.git_dirs.clone();
    std::thread::spawn(move || {
        let started = notify::recommended_watcher(move |result| {
            let _ = tx.send(result);
        })
        .and_then(|mut watcher| {
            watcher.watch(&watch_root, RecursiveMode::Recursive)?;
            // A linked worktree keeps its HEAD, index and refs outside the working tree.
            let roots = path_spellings(&watch_root);
            for git_dir in &git_dirs {
                if roots.iter().any(|root| git_dir.starts_with(root)) || !git_dir.is_dir() {
                    continue;
                }
                if let Err(error) = watcher.watch(git_dir, RecursiveMode::Recursive) {
                    tracing::warn!(
                        checkout_root = %watch_root.display(),
                        dir = %git_dir.display(),
                        %error,
                        "repository watcher could not watch directory"
                    );
                }
            }
            Ok(watcher)
        });
        // If the monitor already stopped, the send fails and the watcher is dropped.
        let _ = ready_tx.send(started);
    });
    let _watcher = loop {
        match ready_rx.recv_timeout(REPOSITORY_FINGERPRINT_INTERVAL) {
            Ok(Ok(watcher)) => break watcher,
            Ok(Err(error)) => {
                tracing::warn!(
                    "native repository watcher for {} failed to start ({error}); falling back to Git status",
                    root.display()
                );
                run_git_repository_monitor(root, checkout_id, events, event_log, stop);
                return;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if stop.try_recv().is_ok() {
                    return;
                }
                announce_repository_change(&root, &mut previous, &checkout_id, &events, &event_log);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    };

    let mut gate = RepositoryRefreshGate::new();
    // Compare once the stream is live, covering the moments since the bridge's last read.
    gate.note_event(Instant::now());
    loop {
        if stop.try_recv().is_ok() {
            return;
        }
        match rx.recv_timeout(gate.wait(Instant::now())) {
            Ok(result) => {
                if filter.is_relevant(&result) {
                    gate.note_event(Instant::now());
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        if !gate.due(now) {
            continue;
        }
        gate.fingerprinted(now);
        let changed =
            announce_repository_change(&root, &mut previous, &checkout_id, &events, &event_log);
        if changed == Some(false) {
            // Events without a status change are usually new ignored output: learn it.
            filter.refresh_ignored(&root);
        }
    }
}

fn git_repository_fingerprint(root: &FsPath) -> Option<Vec<u8>> {
    let output = hotsheet_ticketing::git::command()
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(root)
        .args(["-c", "core.quotepath=false"])
        .args([
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=normal",
            "-z",
        ])
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn run_git_repository_monitor(
    root: std::path::PathBuf,
    checkout_id: String,
    events: broadcast::Sender<ChangeEvent>,
    event_log: Arc<Mutex<EventLog>>,
    stop: std::sync::mpsc::Receiver<()>,
) {
    // Git applies ignore rules before enumerating untracked paths, so target/, node_modules/,
    // and similar ignored trees do not participate in either startup or steady-state work.
    let mut previous = git_repository_fingerprint(&root);
    let mut unchanged_rounds = 0_u8;
    loop {
        let interval = if unchanged_rounds < 12 {
            Duration::from_millis(750)
        } else {
            Duration::from_secs(2)
        };
        if stop.recv_timeout(interval).is_ok() {
            return;
        }
        let Some(next) = git_repository_fingerprint(&root) else {
            continue;
        };
        let changed = previous.as_ref().is_some_and(|prior| prior != &next);
        previous = Some(next);
        if changed {
            unchanged_rounds = 0;
            emit_change(&event_log, &events, repository_change_event(&checkout_id));
        } else {
            unchanged_rounds = unchanged_rounds.saturating_add(1);
        }
    }
}

pub(crate) fn watch_loop(
    rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
    target: WatchTarget,
) {
    use std::time::Duration;

    use std::sync::mpsc::RecvTimeoutError;

    let freshness = target.entry.freshness.clone();
    let mut retry_delay = INDEX_REPAIR_RETRY_MIN;
    loop {
        // Retry queued index repairs on a bounded backoff so a store with no further
        // writes (and a missed watcher echo) still converges (HS2-5BAEX9). This is a
        // local timer over in-memory state; it issues no network requests.
        let first = match rx.recv_timeout(retry_delay) {
            Ok(first) => first,
            Err(RecvTimeoutError::Timeout) => {
                if target.index_repairs.has_pending(&target.store_id) {
                    let still = target.index_repairs.repair(&target.entry, &target.store_id);
                    retry_delay = next_index_repair_delay(retry_delay, still);
                    if !still {
                        regenerate_target_worklists(&target);
                    }
                } else {
                    retry_delay = INDEX_REPAIR_RETRY_MIN;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                // No stream feeds the index any more: a miss must walk again.
                freshness.set_live(false);
                return;
            }
        };
        let mut batch = WatchBatch::default();
        batch.add(&target, first);
        // Debounce a burst (editor save, git checkout touching many files).
        while let Ok(next) = rx.recv_timeout(Duration::from_millis(150)) {
            batch.add(&target, next);
        }
        let WatchBatch {
            mut paths,
            events,
            goes_live,
        } = batch;
        paths.sort();
        paths.dedup();
        let mut index_changed = false;
        for path in &paths {
            index_changed |= handle_path_change(&target, path);
        }
        // Every event in the batch is now in the index (HS2-3R0C6G).
        if goes_live {
            freshness.set_live(true);
        }
        freshness.events_applied(events);
        if !index_changed {
            continue;
        }
        regenerate_target_worklists(&target);
    }
}

/// One debounced burst of watcher events: the ticket files to apply, how many queued
/// events it consumed, and whether applying it makes the watcher live.
#[derive(Default)]
struct WatchBatch {
    paths: Vec<std::path::PathBuf>,
    events: usize,
    goes_live: bool,
}

impl WatchBatch {
    fn add(&mut self, target: &WatchTarget, res: notify::Result<notify::Event>) {
        self.events += 1;
        let lost_events = match &res {
            Ok(event) => event.need_rescan(),
            Err(_) => true,
        };
        if lost_events {
            // The stream may have dropped events: stop trusting misses, re-check every
            // ticket file against the index, and trust them again once that is applied.
            target.entry.freshness.set_live(false);
            self.paths.extend(stale_ticket_files(
                target,
                &target.entry.store.root().join("tickets"),
            ));
            self.goes_live = true;
            return;
        }
        if let Ok(event) = &res
            && event.info() == Some(LIVE_RESYNC)
        {
            self.goes_live = true;
        }
        self.paths.extend(watch_event_paths(target, res));
    }
}

/// Refresh each checkout-local projection that consumes this store. The store is
/// syncable authority; worklists remain local per checkout and may aggregate stores. The
/// refresh runs on the shared refresher so it never overlaps a write-triggered one
/// (HS2-Z99TJN).
fn regenerate_target_worklists(target: &WatchTarget) {
    let registry = target.checkout_registry.clone();
    let host = target.host.clone();
    let store_root = target.entry.store.root().to_path_buf();
    target
        .worklist_refresher
        .request(store_root.clone(), move || {
            crate::events::regenerate_store_worklists(&registry, &host, &store_root);
        });
}

/// The ticket files one watcher event concerns; a startup resync yields the store's ticket
/// files whose bytes differ from the index.
fn watch_event_paths(
    target: &WatchTarget,
    res: notify::Result<notify::Event>,
) -> Vec<std::path::PathBuf> {
    if let Ok(event) = &res
        && matches!(event.info(), Some(STARTUP_RESYNC | LIVE_RESYNC))
    {
        return event
            .paths
            .iter()
            .flat_map(|dir| stale_ticket_files(target, dir))
            .collect();
    }
    event_paths(res)
}

/// Ticket files under `tickets_dir` (`<shard>/<ULID>.md`) whose content hash differs from
/// the store's index: the writes a starting native stream may have missed. Unchanged
/// tickets are skipped so a resync does not re-announce the whole store.
fn stale_ticket_files(target: &WatchTarget, tickets_dir: &FsPath) -> Vec<std::path::PathBuf> {
    let is_md = |path: &FsPath| path.extension().and_then(|e| e.to_str()) == Some("md");
    let mut files = Vec::new();
    let mut dirs = vec![tickets_dir.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                dirs.push(path);
            } else if is_md(&path) {
                files.push(path);
            }
        }
    }
    // Read and hash every file before taking the index mutex (HS2-P867K1). Holding it across
    // the whole walk blocked every list, detail, and note save for the full read of the store
    // (about 600 ms for 2,000 tickets on a debug build), on each startup and every lost-event
    // rescan. A write that lands between the hash and the lookup only makes its file look
    // stale, and applying a stale path re-reads it, so the split never misses a change.
    let hashed: Vec<_> = files
        .into_iter()
        .filter_map(|path| {
            let id = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| Ulid::from_string(stem).ok())?;
            let bytes = std::fs::read(&path).ok()?;
            Some((path, id, hash_bytes(&bytes)))
        })
        .collect();
    let indexed = target
        .entry
        .index
        .lock_or_recover()
        .content_hashes()
        .unwrap_or_default();
    hashed
        .into_iter()
        .filter(|(_, id, hash)| indexed.get(id) != Some(hash))
        .map(|(path, _, _)| path)
        .collect()
}

fn event_paths(res: notify::Result<notify::Event>) -> Vec<std::path::PathBuf> {
    match res {
        Ok(event) => expand_ticket_files(event.paths),
        Err(_) => Vec::new(),
    }
}

/// The ticket `.md` files a set of raw event paths touches. A ticket lands in a **new
/// shard directory** (`tickets/01/<ULID>.md`), and recursive-watch backends (Linux
/// inotify especially) reliably deliver the *directory*-create event but can miss the
/// file event created inside a brand-new subdir — so a bare `.md` filter drops the only
/// event we get and the reindex never fires. We therefore also expand any directory path
/// to the `.md` files it now contains, so a new shard dir's ticket is still picked up.
fn expand_ticket_files(paths: Vec<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    let is_md = |p: &std::path::Path| p.extension().and_then(|e| e.to_str()) == Some("md");
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&p) {
                out.extend(entries.flatten().map(|e| e.path()).filter(|p| is_md(p)));
            }
        } else if is_md(&p) {
            out.push(p);
        }
    }
    out
}

pub(crate) fn handle_path_change(target: &WatchTarget, path: &FsPath) -> bool {
    // The filename stem is the ticket ULID.
    let Some(id) = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| Ulid::from_string(s).ok())
    else {
        return false;
    };
    let index = &target.entry.index;
    let repairs = &target.index_repairs;
    let emit = |kind: &str, id: String, slug: String| {
        emit_change(
            &target.event_log,
            &target.events,
            ChangeEvent {
                cursor: None,
                store: target.store_id.clone(),
                kind: kind.to_string(),
                id,
                slug,
                message: None,
                activity: None,
                assignment: None,
                turn: None,
            },
        );
    };

    if !path.exists() {
        let local_echo = target.local_write_hashes.with_lock(|writes| {
            writes.retain(|_, at| at.elapsed() <= Duration::from_secs(5));
            writes
                .remove(&(target.store_id.clone(), id.to_string(), String::new()))
                .is_some()
        });
        repairs.write_or_schedule(&target.entry, &target.store_id, id, |index| {
            index.delete(&id)
        });
        if local_echo {
            return false;
        }
        emit("deleted", id.to_string(), String::new());
        return true;
    }

    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let hash = hash_bytes(&bytes);
    let local_echo = target.local_write_hashes.with_lock(|writes| {
        writes.retain(|_, at| at.elapsed() <= Duration::from_secs(5));
        writes.contains_key(&(target.store_id.clone(), id.to_string(), hash.clone()))
    });
    if local_echo {
        return false;
    }

    // An unmarked event is external. Preserve its worklist invalidation even when its
    // bytes happen to match the current index row.
    let already = index.with_lock(|index| index.content_hash(&id).ok().flatten());
    if already.as_deref() == Some(hash.as_str()) {
        return true;
    }

    let Ok(ticket) = parse_file(&String::from_utf8_lossy(&bytes)) else {
        // Preserve the last healthy row, but remember the corrupt bytes. Otherwise a
        // manual repair that restores the previous healthy content compares equal to
        // the stale hash and is incorrectly treated as a no-op.
        repairs.write_or_schedule(&target.entry, &target.store_id, id, |index| {
            index.record_source_hash(&id, &hash)
        });
        let (_, slug) = hotsheet_ticketing::recover_ticket_identity(path);
        // Keep the last healthy indexed row available, but wake every client so its
        // resilient refresh can replace that stale projection with recovery UI.
        emit("changed", id.to_string(), slug.unwrap_or_default());
        return true;
    };
    let file = path.display().to_string();
    repairs.write_or_schedule(&target.entry, &target.store_id, ticket.id, |index| {
        index.upsert(&ticket, &file, &hash)
    });
    emit("changed", ticket.id.to_string(), ticket.slug.clone());
    true
}

#[cfg(test)]
mod watcher_tests {
    use super::{
        EventLog, RepositoryEventFilter, RepositoryRefreshGate, WatcherBackend,
        expand_ticket_files, git_repository_fingerprint, registered_watcher_backend,
        repository_change_event, run_git_repository_monitor, run_native_repository_monitor,
        spawn_repository_monitor,
    };
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use std::time::Instant;

    #[test]
    fn startup_resync_finds_only_ticket_files_the_index_has_not_seen() {
        use hotsheet_model::{Timestamp, Ulid};
        use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        let create = |title: &str| {
            ops::create(
                &store,
                Ulid::new(),
                "HS",
                Timestamp::new("2026-09-28T00:00:00Z"),
                NewTicket {
                    title: title.into(),
                    category: "task".into(),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let indexed = create("indexed before the watcher started");
        let state = super::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let tickets = dir.path().join("tickets");
        // Everything on disk is indexed: a resync touches nothing.
        assert!(super::stale_ticket_files(&target, &tickets).is_empty());
        // A write the starting stream missed (new ticket) and an external edit are found.
        let missed = create("written while the stream was starting");
        let mut edited = store.read_ticket(&indexed.id).unwrap();
        edited.title = "edited while the stream was starting".into();
        store.write_ticket(&edited).unwrap();
        let mut stale: Vec<_> = super::stale_ticket_files(&target, &tickets)
            .into_iter()
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        stale.sort();
        let mut expected = vec![indexed.id.to_string(), missed.id.to_string()];
        expected.sort();
        assert_eq!(stale, expected);
    }

    /// A full resync reads every ticket file; the index mutex every list, detail, and note save
    /// needs must stay free while it does (HS2-P867K1). A FIFO named like a ticket blocks the
    /// walk's read deterministically until this test writes to it.
    #[cfg(unix)]
    #[test]
    fn a_resync_reads_ticket_files_without_holding_the_index_lock() {
        use hotsheet_model::Ulid;
        use hotsheet_ticketing::{FsStore, StoreMetadata};
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        let state = super::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let tickets = dir.path().join("tickets");
        let shard = tickets.join("ZZ");
        std::fs::create_dir_all(&shard).unwrap();
        let fifo = shard.join(format!("{}.md", Ulid::new()));
        let path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: a valid NUL-terminated path; mkfifo only creates the special file.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let index = target.entry.index.clone();
        let walk = std::thread::spawn({
            let tickets = tickets.clone();
            move || super::stale_ticket_files(&target, &tickets).len()
        });
        // The walk is now blocked opening the FIFO. The index must be free meanwhile.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut checks = 0;
        while checks < 20 && Instant::now() < deadline {
            assert!(
                index.try_lock().is_ok(),
                "the resync held the index lock while reading ticket files"
            );
            checks += 1;
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::OpenOptions::new()
            .write(true)
            .open(&fifo)
            .unwrap()
            .write_all(b"not a ticket")
            .unwrap();
        // The unindexed "file" is reported stale once the walk finishes.
        assert_eq!(walk.join().unwrap(), 1);
    }

    #[test]
    fn registered_stores_use_the_native_watcher_instead_of_polling() {
        // Polling re-walked every registered store four times a second (HS2-P3SSGR).
        assert_eq!(registered_watcher_backend(), WatcherBackend::Recommended);
    }

    #[test]
    fn the_refresh_gate_waits_for_quiet_bounds_latency_and_rate_limits() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut gate = RepositoryRefreshGate::new();
        // Idle: nothing is due, however long it waits.
        assert!(!gate.due(at(10_000)));

        // One event is due once the events pause.
        gate.note_event(at(0));
        assert!(!gate.due(at(100)));
        assert!(gate.due(at(175)));
        gate.fingerprinted(at(175));
        assert!(!gate.due(at(5_000)), "a read clears the pending events");

        // An event soon after a read waits out the minimum interval.
        gate.note_event(at(200));
        assert!(!gate.due(at(400)), "quiet, but read too recently");
        assert!(gate.due(at(925)));
        gate.fingerprinted(at(925));

        // A continuous stream (a build) is still read within the latency bound.
        for ms in (2_000..3_200).step_by(50) {
            gate.note_event(at(ms));
        }
        assert!(
            gate.due(at(3_000)),
            "continuous events must not starve the read"
        );

        // Empty, refill, repeat: the gate returns to idle after each read.
        gate.fingerprinted(at(3_000));
        assert!(!gate.due(at(4_000)));
        gate.note_event(at(4_000));
        assert!(gate.due(at(4_175)));
    }

    #[test]
    fn the_event_filter_skips_ignored_output_and_git_internals() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "--quiet"]);
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/artifact"), "build output").unwrap();
        let mut filter = RepositoryEventFilter::load(root);
        let event = |path: std::path::PathBuf| {
            Ok(notify::Event::new(notify::EventKind::Any).add_path(path))
        };
        let git_dir = filter.git_dirs[0].clone();

        assert!(!filter.is_relevant(&event(root.join("target/debug/artifact"))));
        assert!(!filter.is_relevant(&event(git_dir.join("objects/ab/cdef"))));
        assert!(filter.is_relevant(&event(git_dir.join("index.lock"))));
        assert!(filter.is_relevant(&event(git_dir.join("index"))));
        assert!(filter.is_relevant(&event(git_dir.join("refs/heads/main"))));
        assert!(filter.is_relevant(&event(root.join("src/lib.rs"))));
        assert!(filter.is_relevant(&Err(notify::Error::generic("dropped"))));
        // A canonical spelling of an ignored path (macOS /private/var) is still ignored.
        let canonical = root.canonicalize().unwrap().join("target/debug/artifact");
        assert!(!filter.is_relevant(&event(canonical)));

        // An ignored directory created after startup is learned on refresh.
        std::fs::write(root.join(".gitignore"), "target/\ndist/\n").unwrap();
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/bundle.js"), "x").unwrap();
        assert!(filter.is_relevant(&event(root.join("dist/bundle.js"))));
        filter.refresh_ignored(root);
        assert!(!filter.is_relevant(&event(root.join("dist/bundle.js"))));
    }

    fn git(root: &std::path::Path, args: &[&str]) {
        let status = hotsheet_ticketing::git::command()
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn git_repository_fingerprint_excludes_ignored_trees() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"]);
        std::fs::write(dir.path().join(".gitignore"), "target/\nnode_modules/\n").unwrap();
        let before = git_repository_fingerprint(dir.path()).unwrap();

        std::fs::create_dir_all(dir.path().join("target/deep/build")).unwrap();
        std::fs::write(
            dir.path().join("target/deep/build/artifact"),
            "large output",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/package")).unwrap();
        std::fs::write(dir.path().join("node_modules/package/index.js"), "ignored").unwrap();
        assert_eq!(git_repository_fingerprint(dir.path()).unwrap(), before);

        std::fs::write(dir.path().join("visible.txt"), "working tree").unwrap();
        assert_ne!(git_repository_fingerprint(dir.path()).unwrap(), before);
    }

    #[tokio::test]
    async fn git_repository_monitor_emits_once_for_a_stable_change() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"]);
        let (events, _) = tokio::sync::broadcast::channel(8);
        let mut receiver = events.subscribe();
        let handle = spawn_repository_monitor(
            dir.path().to_path_buf(),
            "checkout-42".into(),
            events,
            Arc::new(Mutex::new(EventLog::default())),
            run_git_repository_monitor,
        )
        .unwrap();

        // Let the monitor establish its baseline before changing the working tree.
        tokio::time::sleep(Duration::from_millis(850)).await;
        std::fs::write(dir.path().join("changed.txt"), "changed").unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("repository invalidation arrived promptly")
            .unwrap();
        assert_eq!(event.kind, "repository_changed");
        assert_eq!(event.store, "checkout-42");
        assert!(
            tokio::time::timeout(Duration::from_millis(900), receiver.recv())
                .await
                .is_err(),
            "an idle repository must not emit repeated invalidations"
        );
        drop(handle);
    }

    /// The native monitor announces working-tree and commit changes, but not ignored build
    /// output, and stays silent while idle (HS2-4S9ENS).
    #[tokio::test]
    async fn native_repository_monitor_announces_status_changes_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "--quiet"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        let (events, _) = tokio::sync::broadcast::channel(16);
        let mut receiver = events.subscribe();
        let handle = spawn_repository_monitor(
            root.to_path_buf(),
            "checkout-7".into(),
            events,
            Arc::new(Mutex::new(EventLog::default())),
            run_native_repository_monitor,
        )
        .unwrap();
        // Until the native stream is live (seconds on a busy Mac) the Git-status bridge covers
        // changes, so the assertions below hold either way; let the baseline read settle.
        tokio::time::sleep(Duration::from_millis(1_000)).await;
        while receiver.try_recv().is_ok() {}

        let expect_change = async |receiver: &mut tokio::sync::broadcast::Receiver<_>, what| {
            let event: super::ChangeEvent =
                tokio::time::timeout(Duration::from_secs(10), receiver.recv())
                    .await
                    .unwrap_or_else(|_| panic!("{what} was not announced"))
                    .unwrap();
            assert_eq!(event.kind, "repository_changed");
            assert_eq!(event.store, "checkout-7");
        };
        let expect_quiet = async |receiver: &mut tokio::sync::broadcast::Receiver<_>, what| {
            let quiet: Result<Result<super::ChangeEvent, _>, _> =
                tokio::time::timeout(Duration::from_millis(1_500), receiver.recv()).await;
            assert!(quiet.is_err(), "{what} must not be announced");
        };

        std::fs::write(root.join("visible.txt"), "working tree").unwrap();
        expect_change(&mut receiver, "an untracked file").await;

        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/artifact"), "build output").unwrap();
        expect_quiet(&mut receiver, "ignored build output").await;

        git(root, &["add", "."]);
        expect_change(&mut receiver, "staging").await;
        git(root, &["commit", "--quiet", "-m", "first"]);
        expect_change(&mut receiver, "a commit").await;
        expect_quiet(&mut receiver, "an idle checkout").await;
        drop(handle);
    }

    #[test]
    fn expands_a_new_shard_dir_to_its_ticket_file() {
        // Reproduces the inotify new-subdir race deterministically (no real FS events):
        // only the directory event survives, and it must still yield the ticket file.
        let dir = tempfile::tempdir().unwrap();
        let shard = dir.path().join("tickets/01");
        std::fs::create_dir_all(&shard).unwrap();
        let ticket = shard.join("01ARZ3NDEKTSV4RRFFQ69G5FAV.md");
        std::fs::write(&ticket, "x").unwrap();
        std::fs::write(shard.join("README.txt"), "ignore").unwrap();

        // A directory-only event expands to just the .md file inside it.
        assert_eq!(
            expand_ticket_files(vec![shard.clone()]),
            vec![ticket.clone()]
        );
        // A direct .md file event passes through unchanged.
        assert_eq!(
            expand_ticket_files(vec![ticket.clone()]),
            vec![ticket.clone()]
        );
        // Non-.md paths are ignored.
        assert!(expand_ticket_files(vec![shard.join("README.txt")]).is_empty());
    }

    #[test]
    fn repository_invalidation_is_checkout_scoped_and_not_a_ticket_change() {
        let event = repository_change_event("checkout-42");
        assert_eq!(event.kind, "repository_changed");
        assert_eq!(event.store, "checkout-42");
        assert_eq!(event.id, "checkout-42");
        assert!(event.slug.is_empty());
    }
}

/// HS2-3R0C6G: the watcher keeps the store entry's index freshness current, so a slug
/// miss is final only while every queued event and repair has been applied.
#[cfg(test)]
mod freshness_tests {
    use super::{LIVE_RESYNC, QueuedSender, WatcherBackend, resync_event, watch_loop};
    use hotsheet_index::IndexError;
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
    use std::time::{Duration, Instant};

    fn store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn create(store: &FsStore, title: &str) -> hotsheet_model::Ticket {
        ops::create(
            store,
            Ulid::new(),
            "HS",
            Timestamp::new("2026-10-16T00:00:00Z"),
            NewTicket {
                title: title.into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Drives the real watch loop with hand-fed events, walking the freshness states:
    /// starting → live → event queued → applied → events lost → rescanned → stopped.
    #[test]
    fn the_watch_loop_walks_every_freshness_transition() {
        let (dir, store) = store();
        let state = crate::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let entry = target.entry.clone();
        let freshness = entry.freshness.clone();
        let (raw, rx) = std::sync::mpsc::channel();
        let tx = QueuedSender {
            tx: raw,
            freshness: freshness.clone(),
        };
        let looping = std::thread::spawn(move || watch_loop(rx, target));
        let tickets = dir.path().join("tickets");

        // Starting: a write made before the stream is live still resolves (walk).
        let early = create(&store, "written while the watcher started");
        assert!(!freshness.in_sync());
        assert_eq!(entry.resolve(&early.slug).unwrap().unwrap().id, early.id);

        // Live once the live resync is applied; it indexed the early write.
        tx.send(Ok(resync_event(&tickets, LIVE_RESYNC)));
        assert!(
            !freshness.in_sync(),
            "a queued resync is a pending reconcile"
        );
        wait_for("the live resync", || freshness.in_sync());
        assert_eq!(entry.resolve(&early.slug).unwrap().unwrap().id, early.id);
        assert!(entry.resolve("HS-NOSUCH").unwrap().is_none());

        // An external write whose event is queued: out of sync until it is applied, and
        // the miss walks in the meantime.
        let external = create(&store, "external write");
        let path = store.ticket_path(&external.id);
        tx.send(Ok(notify::Event::new(notify::EventKind::Any).add_path(path)));
        assert!(!freshness.in_sync());
        assert_eq!(
            entry.resolve(&external.slug).unwrap().unwrap().id,
            external.id
        );
        wait_for("the external write", || freshness.in_sync());
        assert_eq!(
            entry.resolve(&external.slug).unwrap().unwrap().id,
            external.id
        );

        // A write whose event never arrives is the documented blind spot: in sync, its
        // slug misses. A watcher error (possible lost events) rescans and finds it.
        let unseen = create(&store, "event never delivered");
        assert!(entry.resolve(&unseen.slug).unwrap().is_none());
        tx.send(Err(notify::Error::generic("simulated overflow")));
        assert!(!freshness.in_sync());
        wait_for("the rescan", || freshness.in_sync());
        assert_eq!(entry.resolve(&unseen.slug).unwrap().unwrap().id, unseen.id);

        // A rescan request behaves the same way.
        let rescanned = create(&store, "found by a rescan request");
        tx.send(Ok(
            notify::Event::new(notify::EventKind::Other).set_flag(notify::event::Flag::Rescan)
        ));
        wait_for("the rescan request", || freshness.in_sync());
        assert_eq!(
            entry.resolve(&rescanned.slug).unwrap().unwrap().id,
            rescanned.id
        );

        // Stopping the stream ends the loop and drops back to walking misses.
        drop(tx);
        looping.join().unwrap();
        assert!(!freshness.in_sync());
        let after_stop = create(&store, "written after the watcher stopped");
        assert_eq!(
            entry.resolve(&after_stop.slug).unwrap().unwrap().id,
            after_stop.id
        );
    }

    #[test]
    fn a_failed_index_write_keeps_misses_walking_until_its_repair_lands() {
        let (_dir, store) = store();
        let state = crate::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let entry = target.entry.clone();
        entry.freshness.set_live(true);
        assert!(entry.freshness.in_sync());

        let ticket = create(&store, "index write fails");
        assert!(!target.index_repairs.write_or_schedule(
            &entry,
            &target.store_id,
            ticket.id,
            |_| Err(IndexError::Store("injected".into())),
        ));
        assert!(!entry.freshness.in_sync());
        assert_eq!(entry.resolve(&ticket.slug).unwrap().unwrap().id, ticket.id);

        assert!(!target.index_repairs.repair(&entry, &target.store_id));
        assert!(entry.freshness.in_sync());
        assert_eq!(entry.resolve(&ticket.slug).unwrap().unwrap().id, ticket.id);
    }

    /// The deterministic polling backend goes live on its own, and a genuine miss then
    /// skips the walk while an external write still lands through the stream.
    #[test]
    fn a_running_watcher_goes_live_and_still_sees_external_writes() {
        let (_dir, store) = store();
        let state = crate::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let entry = target.entry.clone();
        let handle = super::spawn_watcher_for(target, WatcherBackend::Poll).unwrap();
        wait_for("the watcher to go live", || entry.freshness.in_sync());
        assert!(entry.resolve("HS-NOSUCH").unwrap().is_none());

        let external = create(&store, "written by another process");
        wait_for("the external write to be indexed", || {
            entry
                .index
                .lock()
                .unwrap()
                .slug_candidates(&external.slug)
                .unwrap()
                == vec![external.id]
        });
        wait_for("the watcher to settle", || entry.freshness.in_sync());
        assert_eq!(
            entry.resolve(&external.slug).unwrap().unwrap().id,
            external.id
        );
        drop(handle);
        wait_for("the watcher to stop", || !entry.freshness.in_sync());
    }
}
