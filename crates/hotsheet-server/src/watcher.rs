//! Filesystem watcher (HS2-6): store and repository change monitoring.

use crate::events::{
    ChangeEvent, EventLog, INDEX_REPAIR_RETRY_MIN, IndexRepairs, emit_change,
    next_index_repair_delay,
};
use crate::multistore::{StoreEntry, StoreHost};
use crate::store_watch_scope::TicketsWatchScope;
use crate::{AppState, LocalWriteHashes, multistore};
use hotsheet_index::hash_bytes;
use hotsheet_model::{Ulid, parse_file};
use hotsheet_sync::LockExt;
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::broadcast;

// ---- filesystem watcher (HS2-6) --------------------------------------------------

/// Keeps the watcher alive; dropping it stops watching.
pub struct WatchHandle {
    _watcher: HeldWatcher,
}

#[cfg(test)]
impl WatchHandle {
    /// Whether a native watcher still polls through its bridge (HS2-190GDZ).
    fn native_bridge_held(&self) -> bool {
        match &self._watcher {
            HeldWatcher::Native { _hold: hold } => hold.0.with_lock(|state| state.bridge.is_some()),
            HeldWatcher::Poll { .. } => false,
        }
    }
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
    /// Bridges the native stream's start-up: polls until the stream proves it delivers.
    bridge: Option<notify::PollWatcher>,
    stopped: bool,
    /// Wakes a supervisor waiting for its stream's probe when the handle is dropped.
    proof: Arc<StreamProof>,
}

/// File names of the probes a native registration must deliver before it is trusted
/// (HS2-190GDZ). A dot-file ending in `.tmp` matches the store's staged-write ignore
/// pattern, and the ticket listing skips plain files directly under `tickets/`.
const STREAM_PROBE_PREFIX: &str = ".hotsheet-watch-probe.";
/// How long the first probe waits; each unanswered probe doubles it up to
/// [`STREAM_PROBE_WAIT_MAX`].
const STREAM_PROBE_WAIT: Duration = Duration::from_secs(1);
const STREAM_PROBE_WAIT_MAX: Duration = Duration::from_secs(30);
/// After this many unanswered probes in a row the directory is registered again.
const STREAM_PROBE_ROUNDS_PER_REGISTRATION: u32 = 2;
static STREAM_PROBE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Evidence that a native registration delivers events (HS2-190GDZ).
///
/// macOS FSEvents can return from a stream (re)start while `fseventsd` is busy and then
/// deliver nothing for the watched subtree, with no dropped-event flag. A fixed delay
/// cannot tell that apart from a live stream, so after every registration the supervisor
/// writes a probe file into the watched directory and waits for the stream to report it.
/// FSEvents and inotify deliver in order, so once a probe arrives every later write is
/// reported, and every rearm signal raised before the probe is already queued.
#[derive(Default)]
struct StreamProof {
    state: Mutex<StreamProofState>,
    changed: Condvar,
}

#[derive(Default)]
struct StreamProofState {
    /// Probes written since the current registration; only these prove it.
    pending: Vec<std::ffi::OsString>,
    seen: bool,
    stopped: bool,
    /// Test seam: probes the simulated stream fails to report before one proves it.
    drop_probes: usize,
    dropped: Vec<std::ffi::OsString>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProofOutcome {
    /// The stream reported a probe written after its registration.
    Proven,
    /// No probe could be written (read-only store, or the directory just vanished).
    Unwritable,
    /// Registering the directory again failed: it is no longer watched.
    Unregistered,
    /// The watcher handle was dropped.
    Stopped,
}

impl StreamProof {
    fn is_probe(name: &std::ffi::OsStr) -> bool {
        name.to_str()
            .is_some_and(|name| name.starts_with(STREAM_PROBE_PREFIX))
    }

    /// Called by the native callback. Returns whether `event` names a probe, which is
    /// never a store change; a probe of the current registration proves the stream.
    fn observe(&self, event: &notify::Event) -> bool {
        let mut probes = event
            .paths
            .iter()
            .filter_map(|path| path.file_name())
            .filter(|name| Self::is_probe(name))
            .peekable();
        if probes.peek().is_none() {
            return false;
        }
        let mut state = self.state.lock_or_recover();
        for name in probes {
            if !state.pending.iter().any(|pending| pending == name) {
                continue;
            }
            if state.dropped.iter().any(|dropped| dropped == name) {
                continue;
            }
            if state.drop_probes > 0 {
                state.drop_probes -= 1;
                state.dropped.push(name.to_os_string());
                continue;
            }
            state.seen = true;
            self.changed.notify_all();
        }
        true
    }

    /// Starts a proof for a new registration: earlier probes no longer count.
    fn begin(&self) -> bool {
        let mut state = self.state.lock_or_recover();
        state.pending.clear();
        state.seen = false;
        !state.stopped
    }

    fn stop(&self) {
        self.state.lock_or_recover().stopped = true;
        self.changed.notify_all();
    }

    /// Writes one probe into `dir` and waits up to `wait` for the stream to report any
    /// probe of the current registration. `None` means the wait timed out.
    fn probe(&self, dir: &FsPath, wait: Duration) -> Option<ProofOutcome> {
        let name = format!(
            "{STREAM_PROBE_PREFIX}{}.{}.tmp",
            std::process::id(),
            STREAM_PROBE_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        {
            let mut state = self.state.lock_or_recover();
            if state.stopped {
                return Some(ProofOutcome::Stopped);
            }
            if state.seen {
                return Some(ProofOutcome::Proven);
            }
            state.pending.push(name.clone().into());
        }
        let path = dir.join(&name);
        if std::fs::File::create(&path).is_err() {
            return Some(ProofOutcome::Unwritable);
        }
        let _ = std::fs::remove_file(&path);
        let state = self.state.lock_or_recover();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, wait, |state| !state.seen && !state.stopped)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped {
            Some(ProofOutcome::Stopped)
        } else if state.seen {
            Some(ProofOutcome::Proven)
        } else {
            None
        }
    }
}

/// Waits until the native stream in `slot` reports a probe written into `dir`, its
/// recursively watched directory (HS2-190GDZ). Unanswered probes back off, and every
/// [`STREAM_PROBE_ROUNDS_PER_REGISTRATION`] of them the directory is registered again:
/// a registration that misses its own probe may never report that subtree.
/// `on_unanswered` runs after each unanswered probe with the count so far.
fn prove_native_stream<W: notify::Watcher>(
    proof: &StreamProof,
    watcher: &Mutex<impl WatcherSlot<W>>,
    dir: &FsPath,
    first_wait: Duration,
    mut on_unanswered: impl FnMut(u32),
) -> ProofOutcome {
    if !proof.begin() {
        return ProofOutcome::Stopped;
    }
    let mut wait = first_wait;
    let mut unanswered = 0;
    loop {
        if let Some(outcome) = proof.probe(dir, wait) {
            return outcome;
        }
        unanswered += 1;
        tracing::debug!(dir = %dir.display(), unanswered, "native watcher has not reported its probe");
        on_unanswered(unanswered);
        wait = (wait * 2).min(STREAM_PROBE_WAIT_MAX);
        if unanswered % STREAM_PROBE_ROUNDS_PER_REGISTRATION != 0 {
            continue;
        }
        tracing::warn!(dir = %dir.display(), unanswered, "native watcher missed its probes; registering it again");
        {
            let mut slot = watcher.lock_or_recover();
            let Some(watcher) = slot.live_watcher() else {
                return ProofOutcome::Stopped;
            };
            let _ = watcher.unwatch(dir);
            if watcher
                .watch(dir, notify::RecursiveMode::Recursive)
                .is_err()
            {
                return ProofOutcome::Unregistered;
            }
        }
        if !proof.begin() {
            return ProofOutcome::Stopped;
        }
    }
}

/// The running watcher a proof registers again, unless its handle was dropped.
trait WatcherSlot<W> {
    fn live_watcher(&mut self) -> Option<&mut W>;
}

impl WatcherSlot<notify::RecommendedWatcher> for NativeWatcherState {
    fn live_watcher(&mut self) -> Option<&mut notify::RecommendedWatcher> {
        if self.stopped {
            return None;
        }
        self.watcher.as_mut()
    }
}

/// A parent-directory signal survives deletion of the watched store directory.
#[derive(Clone, Copy)]
struct WatchRootSignal {
    root_removed: bool,
    tickets_removed: bool,
}

/// Identifies the directory object, not just its path, across startup races.
#[cfg(unix)]
fn watched_directory_identity(path: &FsPath) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn watched_directory_identity(path: &FsPath) -> Option<(u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, GetFileInformationByHandle,
    };

    // A directory needs BACKUP_SEMANTICS to open on Windows. Share deletion so
    // this short-lived identity probe does not block a store move (HS2-R6FZ54).
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(directory.as_raw_handle(), &mut info) } == 0 {
        return None;
    }
    Some((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

fn watch_root_signal(
    event: &notify::Event,
    root: &FsPath,
    tickets: &FsPath,
    canonical_root: Option<&FsPath>,
    canonical_tickets: Option<&FsPath>,
) -> Option<WatchRootSignal> {
    let root_event = event
        .paths
        .iter()
        .any(|path| path == root || canonical_root.is_some_and(|canonical| path == canonical));
    let tickets_event = event.paths.iter().any(|path| {
        path == tickets || canonical_tickets.is_some_and(|canonical| path == canonical)
    });
    if !root_event && !tickets_event {
        return None;
    }
    let removed = matches!(
        event.kind,
        notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
    );
    Some(WatchRootSignal {
        root_removed: removed && root_event,
        tickets_removed: removed && tickets_event,
    })
}

type NativeWatcherSlot = Arc<Mutex<NativeWatcherState>>;

/// Owns a started (or starting) native watcher; dropping it stops the watcher.
struct NativeWatcherHold(pub(crate) NativeWatcherSlot);

impl Drop for NativeWatcherHold {
    fn drop(&mut self) {
        let (watchers, proof) = self.0.with_lock(|state| {
            state.stopped = true;
            (
                (state.watcher.take(), state.bridge.take()),
                state.proof.clone(),
            )
        });
        proof.stop();
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
/// A replaced store can have removed tickets as well as new files (HS2-R6FZ54).
const REARM_RESYNC: &str = "hotsheet-rearm-resync";
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
    let canon =
        |p: &FsPath| hotsheet_sync::paths::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// Watch one store (any hosted store). The returned [`WatchHandle`] must be kept alive
/// for the watcher to run.
pub(crate) fn spawn_watcher_for(
    target: WatchTarget,
    backend: WatcherBackend,
) -> anyhow::Result<WatchHandle> {
    let scope = Arc::new(TicketsWatchScope::for_store(target.entry.store.root()));
    spawn_watcher_in_scope(target, backend, scope)
}

/// [`spawn_watcher_for`] with the native stream's directory and event filter chosen by
/// `scope` (HS2-VN96J5), so tests can run the Windows store-root scope on every platform.
pub(crate) fn spawn_watcher_in_scope(
    target: WatchTarget,
    backend: WatcherBackend,
    scope: Arc<TicketsWatchScope>,
) -> anyhow::Result<WatchHandle> {
    spawn_watcher_in_scope_with_startup_hooks(target, backend, scope, StartupHooks::default())
}

type StartupHook = Box<dyn FnOnce() + Send>;

/// Test seams in the native watcher's startup sequence.
#[derive(Default)]
struct StartupHooks {
    /// After the parent is registered, before the store's own directories.
    before_children: Option<StartupHook>,
    /// After the directories are registered, before their identity is sampled.
    after_watch_before_identity: Option<StartupHook>,
    /// After the identity is sampled, before the stream is proven.
    after_children: Option<StartupHook>,
    /// Probes the stream fails to report before one proves it, simulating a busy
    /// `fseventsd` (HS2-190GDZ).
    dropped_probes: usize,
}

fn spawn_watcher_in_scope_with_startup_hooks(
    target: WatchTarget,
    backend: WatcherBackend,
    scope: Arc<TicketsWatchScope>,
    hooks: StartupHooks,
) -> anyhow::Result<WatchHandle> {
    use notify::{RecursiveMode, Watcher};
    let StartupHooks {
        before_children,
        after_watch_before_identity,
        after_children,
        dropped_probes,
    } = hooks;

    // The tickets directory is derived from the store root, so the root needs no `parent()`
    // lookup that could fail at runtime (HS2-FXF07C).
    let store_root = target.entry.store.root().to_path_buf();
    let tickets_dir = store_root.join("tickets");
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
            let proof = slot.with_lock(|state| state.proof.clone());
            proof.state.lock_or_recover().drop_probes = dropped_probes;
            let started = slot.clone();
            std::thread::spawn(move || {
                let resync = tx.clone();
                let request_resync = |dir: &FsPath, info: &str| {
                    resync.send(Ok(resync_event(dir, info)));
                };
                let bridge_tx = tx.clone();
                let new_bridge = || {
                    let bridge_tx = bridge_tx.clone();
                    let bridge_root = store_root.clone();
                    let bridge_tickets = tickets_dir.clone();
                    let away = std::sync::atomic::AtomicBool::new(false);
                    notify::PollWatcher::new(
                        move |res| {
                            // The poller follows the path, so a moved store reads as every
                            // ticket deleted. Drop that, as the native scope does, keep misses
                            // walking, and reconcile the store once it returns (HS2-190GDZ).
                            if !bridge_root.is_dir() {
                                if !away.swap(true, Ordering::SeqCst) {
                                    bridge_tx.freshness.set_live(false);
                                }
                                return;
                            }
                            if away.swap(false, Ordering::SeqCst) {
                                bridge_tx.send(Ok(resync_event(&bridge_tickets, REARM_RESYNC)));
                            }
                            bridge_tx.send(res);
                        },
                        startup_bridge_config(),
                    )
                    .and_then(|mut bridge| {
                        bridge.watch(&tickets_dir, RecursiveMode::Recursive)?;
                        Ok(bridge)
                    })
                };
                let bridge = new_bridge();
                {
                    let mut slot = started.lock_or_recover();
                    if slot.stopped {
                        return;
                    }
                    slot.bridge = bridge.ok();
                }
                request_resync(&tickets_dir, STARTUP_RESYNC);
                let watch_dir = scope.watch_dir().to_path_buf();
                let Some(parent) = store_root.parent() else {
                    tracing::warn!(dir = %store_root.display(), "store watcher has no parent to monitor for recreation");
                    return;
                };
                let parent = parent.to_path_buf();
                let (rearm_tx, rearm_rx) = std::sync::mpsc::channel();
                let signal_root = store_root.clone();
                let signal_tickets = tickets_dir.clone();
                // FSEvents spells `/var` as `/private/var`; use the stable parent's
                // canonical path because the store root may no longer exist.
                let canonical_signal_root = hotsheet_sync::paths::canonicalize(&parent)
                    .ok()
                    .and_then(|canonical| store_root.file_name().map(|name| canonical.join(name)));
                let canonical_signal_tickets = canonical_signal_root
                    .as_ref()
                    .map(|root| root.join("tickets"));
                let removal_freshness = resync.freshness.clone();
                let observed_proof = proof.clone();
                let mut watcher =
                    match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                        if let Ok(event) = &res
                            && let Some(signal) = watch_root_signal(
                                event,
                                &signal_root,
                                &signal_tickets,
                                canonical_signal_root.as_deref(),
                                canonical_signal_tickets.as_deref(),
                            )
                        {
                            // HS2-R6FZ54: a native unwatch may block on FSEvents. Invalidate
                            // before the supervisor handles the signal, including root-only
                            // events that the scoped ticket stream does not admit.
                            if signal.root_removed || signal.tickets_removed {
                                removal_freshness.set_live(false);
                            }
                            let _ = rearm_tx.send(signal);
                        }
                        // A probe proves the stream; it is never a store change (HS2-190GDZ).
                        if let Ok(event) = &res
                            && observed_proof.observe(event)
                        {
                            return;
                        }
                        if is_access_event(&res) {
                            return;
                        }
                        if let Some(res) = scope.admit(res) {
                            tx.send(res);
                        }
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
                // Register the stable parent first. If the store vanishes during
                // startup, a failed child registration must not discard this signal.
                if let Err(error) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
                    tracing::warn!(dir = %parent.display(), %error, "store watcher parent monitor failed");
                    return;
                }
                if let Some(hook) = before_children {
                    hook();
                }
                // Sample before registration too: replacing the directory between
                // watch() and the later sample would otherwise make a stale handle
                // appear to own the current path.
                let identity_before_watch = watched_directory_identity(&watch_dir);
                // On Linux/macOS, a shallow root watch sees `tickets/` return.
                let mut root_watched = watch_dir != store_root
                    && store_root.is_dir()
                    && watcher
                        .watch(&store_root, RecursiveMode::NonRecursive)
                        .is_ok();
                let mut watched_dir = watch_dir.is_dir()
                    && watcher.watch(&watch_dir, RecursiveMode::Recursive).is_ok();
                if watch_dir == store_root {
                    root_watched = watched_dir;
                }
                if let Some(hook) = after_watch_before_identity {
                    hook();
                }
                let watched_identity = watched_directory_identity(&watch_dir);
                if let Some(hook) = after_children {
                    hook();
                }
                {
                    let mut slot = started.lock_or_recover();
                    // The handle was dropped while the stream started: stop it again.
                    if slot.stopped {
                        return;
                    }
                    slot.watcher = Some(watcher);
                }
                // HS2-190GDZ: trust the stream only once it reports a probe written after
                // its registration. Signals for changes made before the probe are queued
                // by then, so the startup check below sees every replacement.
                // While a registration is unproven the bridge polls the store. Once a probe
                // goes unanswered, the bridge-backed index is trusted for misses after a full
                // reconcile, so a lagging `fseventsd` delays native delivery without making
                // the store look stale.
                let bridge_until_proven = |unanswered: u32| {
                    if unanswered != 1 {
                        return;
                    }
                    if started.with_lock(|slot| slot.bridge.is_none()) {
                        let bridge = new_bridge().ok();
                        let mut slot = started.lock_or_recover();
                        if slot.stopped {
                            return;
                        }
                        slot.bridge = bridge;
                    }
                    request_resync(&tickets_dir, REARM_RESYNC);
                };
                let prove_registration = || {
                    prove_native_stream(
                        &proof,
                        &started,
                        &watch_dir,
                        STREAM_PROBE_WAIT,
                        &bridge_until_proven,
                    )
                };
                let prove = |watched_dir: &mut bool| {
                    let outcome = if *watched_dir {
                        prove_registration()
                    } else {
                        ProofOutcome::Unwritable
                    };
                    match outcome {
                        ProofOutcome::Proven => {}
                        // Nothing to probe: give the stream the former fixed start-up delay.
                        ProofOutcome::Unwritable => std::thread::sleep(STARTUP_RESYNC_DELAY),
                        ProofOutcome::Unregistered => *watched_dir = false,
                        ProofOutcome::Stopped => return false,
                    }
                    true
                };
                if !prove(&mut watched_dir) {
                    return;
                }
                // A child may have disappeared and returned while the native stream
                // started. Re-register its current path before trusting the index.
                let changed_during_startup = rearm_rx.try_iter().fold(false, |changed, signal| {
                    changed || signal.root_removed || signal.tickets_removed
                }) || identity_before_watch != watched_identity
                    || watched_identity != watched_directory_identity(&watch_dir);
                let missing_during_startup = !watched_dir;
                if changed_during_startup || missing_during_startup {
                    let mut slot = started.lock_or_recover();
                    if slot.stopped {
                        return;
                    }
                    let Some(watcher) = slot.watcher.as_mut() else {
                        return;
                    };
                    if watched_dir {
                        let _ = watcher.unwatch(&watch_dir);
                        watched_dir = false;
                    }
                    if root_watched && watch_dir != store_root {
                        let _ = watcher.unwatch(&store_root);
                        root_watched = false;
                    }
                    if store_root.is_dir() && watch_dir != store_root {
                        root_watched = watcher
                            .watch(&store_root, RecursiveMode::NonRecursive)
                            .is_ok();
                    }
                    if watch_dir.is_dir() {
                        watched_dir = watcher.watch(&watch_dir, RecursiveMode::Recursive).is_ok();
                        if watch_dir == store_root {
                            root_watched = watched_dir;
                        }
                    }
                    drop(slot);
                    if watched_dir && !prove(&mut watched_dir) {
                        return;
                    }
                }
                // The native stream is proven: retire the bridge and re-check once more.
                let bridge = started.with_lock(|slot| slot.bridge.take());
                drop(bridge);
                if watched_dir {
                    request_resync(&tickets_dir, REARM_RESYNC);
                } else {
                    resync.freshness.set_live(false);
                }
                while let Ok(signal) = rearm_rx.recv() {
                    // The signal can also describe a path that vanished before the callback
                    // observed a Remove event. Do not leave a stale index live while a native
                    // watch or unwatch waits on the platform run loop.
                    if signal.root_removed
                        || signal.tickets_removed
                        || !store_root.is_dir()
                        || !watch_dir.is_dir()
                    {
                        resync.freshness.set_live(false);
                    }
                    let mut slot = started.lock_or_recover();
                    if slot.stopped {
                        return;
                    }
                    let Some(watcher) = slot.watcher.as_mut() else {
                        return;
                    };
                    if signal.root_removed || !store_root.is_dir() {
                        if watched_dir {
                            let _ = watcher.unwatch(&watch_dir);
                            watched_dir = false;
                            if watch_dir == store_root {
                                root_watched = false;
                            }
                        }
                        if root_watched && watch_dir != store_root {
                            let _ = watcher.unwatch(&store_root);
                            root_watched = false;
                        }
                    } else if (signal.tickets_removed || !watch_dir.is_dir()) && watched_dir {
                        let _ = watcher.unwatch(&watch_dir);
                        watched_dir = false;
                    }
                    if store_root.is_dir() && !root_watched && watch_dir != store_root {
                        root_watched = watcher
                            .watch(&store_root, RecursiveMode::NonRecursive)
                            .is_ok();
                    }
                    if watch_dir.is_dir()
                        && !watched_dir
                        && watcher.watch(&watch_dir, RecursiveMode::Recursive).is_ok()
                    {
                        watched_dir = true;
                        if watch_dir == store_root {
                            root_watched = true;
                        }
                        drop(slot);
                        // HS2-190GDZ: the index goes live only through the bridge or once the
                        // new registration reports a probe; the resync covers earlier writes.
                        let outcome = prove_registration();
                        // The native stream now reports this directory: retire any bridge
                        // that covered the wait. The resync covers writes it may have missed.
                        let bridge = started.with_lock(|slot| slot.bridge.take());
                        drop(bridge);
                        match outcome {
                            ProofOutcome::Stopped => return,
                            ProofOutcome::Unregistered => {
                                watched_dir = false;
                                if watch_dir == store_root {
                                    root_watched = false;
                                }
                                resync.freshness.set_live(false);
                                continue;
                            }
                            ProofOutcome::Proven | ProofOutcome::Unwritable => {}
                        }
                        request_resync(&tickets_dir, REARM_RESYNC);
                    }
                }
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
        live: None,
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

/// Tell clients that a checkout's ticket group registry changed on disk (HS2-70JS8B): an entry
/// written by this server, by the CLI, or by a pull (`.hotsheet2/groups/`), or the machine-local
/// overlay of groups another clone added (`.hotsheet2/groups-remote.json`). A rename changes no
/// ticket, so without this other open clients kept the old name. Carries no names or paths.
pub(crate) fn group_registry_change_event(checkout_id: &str) -> ChangeEvent {
    ChangeEvent {
        live: None,
        cursor: None,
        store: checkout_id.to_string(),
        kind: "group_registry_changed".to_string(),
        id: checkout_id.to_string(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    }
}

/// The group registry's on-disk state: each entry's and the overlay's name, length, and
/// modification time. Cheap (one directory listing), so it is compared only after events.
pub(crate) fn group_registry_fingerprint(root: &FsPath) -> Vec<(String, u64, Option<SystemTime>)> {
    let stat = |path: &FsPath| {
        std::fs::metadata(path)
            .ok()
            .map(|meta| (meta.len(), meta.modified().ok()))
    };
    let mut entries: Vec<(String, u64, Option<SystemTime>)> =
        std::fs::read_dir(root.join(hotsheet_ticketing::groups::GROUP_REGISTRY_DIR))
            .map(|dir| {
                dir.filter_map(Result::ok)
                    .filter_map(|entry| {
                        let (len, modified) = stat(&entry.path())?;
                        Some((
                            entry.file_name().to_string_lossy().into_owned(),
                            len,
                            modified,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
    entries.sort();
    if let Some((len, modified)) =
        stat(&root.join(hotsheet_ticketing::groups::git_sync::GROUP_REMOTE_OVERLAY_FILE))
    {
        entries.push(("\0overlay".to_string(), len, modified));
    }
    entries
}

/// Whether a filesystem event touches the group registry or its remote overlay. Checked before the
/// Git-status filter, which skips the ignored overlay.
fn touches_group_registry(root: &FsPath, result: &hotsheet_jobs::watch::Delivery) -> bool {
    let Ok(event) = result else {
        return false;
    };
    let registry = path_spellings(&root.join(hotsheet_ticketing::groups::GROUP_REGISTRY_DIR));
    let overlay =
        path_spellings(&root.join(hotsheet_ticketing::groups::git_sync::GROUP_REMOTE_OVERLAY_FILE));
    event.paths.iter().any(|path| {
        registry.iter().any(|dir| path.starts_with(dir)) || overlay.iter().any(|file| path == file)
    })
}

/// Announce a registry change when its fingerprint moved since `previous`.
fn announce_group_registry_change(
    root: &FsPath,
    previous: &mut Vec<(String, u64, Option<SystemTime>)>,
    checkout_id: &str,
    events: &broadcast::Sender<ChangeEvent>,
    event_log: &Arc<Mutex<EventLog>>,
) {
    let next = group_registry_fingerprint(root);
    if next != *previous {
        *previous = next;
        emit_change(event_log, events, group_registry_change_event(checkout_id));
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
    /// HS2-2HZN2C: the last raw `ls-files` listing, so an unchanged one costs no further Git.
    listing: Vec<u8>,
    /// HS2-2HZN2C: whether `ignored` holds only entries an ignore pattern matches, which
    /// is what makes them safe to exclude from the platform stream.
    pattern_checked: bool,
}

impl RepositoryEventFilter {
    fn load(root: &FsPath) -> Self {
        let mut filter = Self {
            git_dirs: repository_git_dirs(root),
            ..Self::default()
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
        if !output.status.success() || output.stdout == self.listing {
            return;
        }
        self.listing = output.stdout;
        // HS2-2HZN2C: `--directory` also lists an untracked directory whose files all happen
        // to be ignored (`.claude/` holding only `.claude/worktrees/`). No pattern ignores it,
        // so a new file in it changes the status: keep only entries a pattern matches. If Git
        // cannot answer, keep the listing (the filter's old behavior) and exclude nothing.
        let checked = pattern_ignored_entries(root, &self.listing);
        self.pattern_checked = checked.is_some();
        let entries = checked.unwrap_or_else(|| self.listing.clone());
        let bases = path_spellings(root);
        self.ignored = entries
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

    fn is_relevant(&self, result: &hotsheet_jobs::watch::Delivery) -> bool {
        let Ok(event) = result else {
            return true;
        };
        if event.need_rescan() || event.paths.is_empty() {
            return true;
        }
        event.paths.iter().any(|path| self.path_is_relevant(path))
    }

    /// HS2-2HZN2C: the most specific ignored entry `path` lies in, if any (Git lists both
    /// an untracked directory holding only ignored files and the ignored directory in it).
    fn ignored_entry(&self, path: &FsPath) -> Option<&std::path::PathBuf> {
        self.ignored
            .iter()
            .filter(|ignored| path.starts_with(ignored))
            .max_by_key(|ignored| ignored.components().count())
    }

    fn path_is_relevant(&self, path: &FsPath) -> bool {
        if self.ignored_entry(path).is_some() {
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
    if let Ok(canonical) = hotsheet_sync::paths::canonicalize(path)
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

/// HS2-2HZN2C: what a native repository monitor has done, for tests and measurements.
#[derive(Debug, Default)]
pub(crate) struct RepositoryMonitorProbe {
    /// Every delivery the platform handed the monitor.
    delivered: AtomicU64,
    /// Streams started (the first one and each replacement).
    streams: AtomicU64,
    /// The directories the live stream leaves out.
    excluded: Mutex<Vec<std::path::PathBuf>>,
}

impl RepositoryMonitorProbe {
    #[cfg(test)]
    pub(crate) fn delivered(&self) -> u64 {
        self.delivered.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn streams(&self) -> u64 {
        self.streams.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn excluded(&self) -> Vec<std::path::PathBuf> {
        self.excluded.lock_or_recover().clone()
    }
}

/// HS2-2HZN2C: the paths no exclusion may cover: Git directories (watched or inside the
/// working tree) and the group registry with its ignored remote overlay, in every spelling.
fn repository_protected_paths(
    root: &FsPath,
    git_dirs: &[std::path::PathBuf],
) -> Vec<std::path::PathBuf> {
    let mut protected = git_dirs.to_vec();
    protected.extend(path_spellings(
        &root.join(hotsheet_ticketing::groups::GROUP_REGISTRY_DIR),
    ));
    protected.extend(path_spellings(
        &root.join(hotsheet_ticketing::groups::git_sync::GROUP_REMOTE_OVERLAY_FILE),
    ));
    protected
}

/// HS2-2HZN2C: the ignored directories the checkout's stream should leave out, in the
/// spelling the platform reports (macOS `/private/var` for `/var`).
fn desired_repository_exclusions(
    root: &FsPath,
    filter: &RepositoryEventFilter,
    churn: &std::collections::HashMap<std::path::PathBuf, u64>,
) -> Vec<std::path::PathBuf> {
    if !filter.pattern_checked {
        return Vec::new();
    }
    let reported_root = path_spellings(root)
        .pop()
        .unwrap_or_else(|| root.to_path_buf());
    let candidates: Vec<std::path::PathBuf> = filter
        .ignored
        .iter()
        .filter(|entry| entry.starts_with(&reported_root))
        .cloned()
        .collect();
    crate::repository_watch_exclusions::select_exclusions(
        &candidates,
        &repository_protected_paths(root, &filter.git_dirs),
        churn,
        hotsheet_jobs::watch::MAX_EXCLUDED_PATHS,
        FsPath::is_dir,
    )
}

/// HS2-2HZN2C: the entries of a raw `ls-files -z` listing that one of the checkout's ignore
/// patterns matches, in the same NUL-separated form; `None` when Git cannot answer.
fn pattern_ignored_entries(root: &FsPath, listing: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    if listing.is_empty() {
        return Some(Vec::new());
    }
    let mut child = hotsheet_ticketing::git::command()
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "--stdin", "-z"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let input = listing.to_vec();
    // Written from its own thread so a large answer cannot fill the pipe while we write.
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().ok()?;
    let _ = writer.join();
    // Exit status 1 means that nothing matched; anything else but 0 is an error.
    match output.status.code() {
        Some(0 | 1) => Some(output.stdout),
        _ => None,
    }
}

/// Start the checkout's stream over its working tree and any Git directory outside it,
/// leaving out `excluded`. Returns the watcher and the exclusions it actually applies.
fn start_repository_stream(
    watch_root: &FsPath,
    git_dirs: &[std::path::PathBuf],
    excluded: &[std::path::PathBuf],
    tx: &std::sync::mpsc::Sender<hotsheet_jobs::watch::Delivery>,
) -> Result<(hotsheet_jobs::watch::PathWatcher, Vec<std::path::PathBuf>), hotsheet_jobs::watch::Error>
{
    use hotsheet_jobs::watch::{PathWatcher, RecursiveMode};
    // A linked worktree keeps its HEAD, index and refs outside the working tree.
    // HS2-654VBY: every directory is registered before the watcher starts once; a
    // `watch` call per directory restarted the macOS event stream each time.
    let roots = path_spellings(watch_root);
    let mut outside: Vec<&std::path::Path> = Vec::new();
    for git_dir in git_dirs {
        if roots.iter().any(|root| git_dir.starts_with(root))
            || !git_dir.is_dir()
            || outside.contains(&git_dir.as_path())
        {
            continue;
        }
        outside.push(git_dir);
    }
    let mut paths = vec![(watch_root, RecursiveMode::Recursive)];
    paths.extend(outside.iter().map(|dir| (*dir, RecursiveMode::Recursive)));
    let excluded_refs: Vec<&FsPath> = excluded.iter().map(std::path::PathBuf::as_path).collect();
    // HS2-P0EHD4: `PathWatcher` never delivers access events, so a read of a watched
    // file (Linux inotify reports each open) does not re-read the repository.
    let handler = || {
        let tx = tx.clone();
        move |result| {
            let _ = tx.send(result);
        }
    };
    let error = match PathWatcher::start_excluding(&paths, &excluded_refs, handler()) {
        Ok(watcher) => return Ok((watcher, excluded.to_vec())),
        Err(error) => error,
    };
    // One directory that cannot be watched must not cost the others: watch the working
    // tree alone, as the per-directory registration did.
    tracing::warn!(
        checkout_root = %watch_root.display(),
        %error,
        "repository watcher could not watch an outside Git directory"
    );
    match PathWatcher::start_excluding(&paths[..1], &excluded_refs, handler()) {
        Ok(watcher) => Ok((watcher, excluded.to_vec())),
        Err(_) if !excluded.is_empty() => {
            // HS2-2HZN2C: a platform that refuses the exclusions still gets a plain stream;
            // the event filter drops ignored output as before.
            PathWatcher::start(&paths[..1], handler()).map(|watcher| (watcher, Vec::new()))
        }
        Err(error) => Err(error),
    }
}

fn run_native_repository_monitor(
    root: std::path::PathBuf,
    checkout_id: String,
    events: broadcast::Sender<ChangeEvent>,
    event_log: Arc<Mutex<EventLog>>,
    stop: std::sync::mpsc::Receiver<()>,
) {
    run_native_repository_monitor_with(
        root,
        checkout_id,
        events,
        event_log,
        stop,
        true,
        &RepositoryMonitorProbe::default(),
    );
}

/// The native repository monitor; `exclude` turns the HS2-2HZN2C source exclusions on.
pub(crate) fn run_native_repository_monitor_with(
    root: std::path::PathBuf,
    checkout_id: String,
    events: broadcast::Sender<ChangeEvent>,
    event_log: Arc<Mutex<EventLog>>,
    stop: std::sync::mpsc::Receiver<()>,
    exclude: bool,
    probe: &RepositoryMonitorProbe,
) {
    type Started = Result<
        (hotsheet_jobs::watch::PathWatcher, Vec<std::path::PathBuf>),
        hotsheet_jobs::watch::Error,
    >;
    let mut previous = git_repository_fingerprint(&root);
    let mut registry = group_registry_fingerprint(&root);
    let mut filter = RepositoryEventFilter::load(&root);
    // HS2-2HZN2C: events dropped as ignored, by ignored entry, ranking exclusion candidates.
    let mut churn: std::collections::HashMap<std::path::PathBuf, u64> =
        std::collections::HashMap::new();
    let wanted = if exclude {
        desired_repository_exclusions(&root, &filter, &churn)
    } else {
        Vec::new()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    // Starting a native stream can take many seconds on a busy Mac (HS2-XAHR91), so it starts
    // on its own thread while this one keeps the old Git-status poll going as a bridge.
    let stream_git_dirs = filter.git_dirs.clone();
    let spawn_start = |excluded: Vec<std::path::PathBuf>| {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Started>();
        let watch_root = root.clone();
        let git_dirs = stream_git_dirs.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            // If the monitor already stopped, the send fails and the watcher is dropped.
            let _ = ready_tx.send(start_repository_stream(
                &watch_root,
                &git_dirs,
                &excluded,
                &tx,
            ));
        });
        ready_rx
    };
    let ready_rx = spawn_start(wanted);
    let (mut watcher, mut excluded) = loop {
        match ready_rx.recv_timeout(REPOSITORY_FINGERPRINT_INTERVAL) {
            Ok(Ok(started)) => break started,
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
                // The bridge also compares the group registry, which native events report later.
                announce_group_registry_change(
                    &root,
                    &mut registry,
                    &checkout_id,
                    &events,
                    &event_log,
                );
                announce_repository_change(&root, &mut previous, &checkout_id, &events, &event_log);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    };
    probe.streams.fetch_add(1, Ordering::SeqCst);
    *probe.excluded.lock_or_recover() = excluded.clone();
    // HS2-2HZN2C: a replacement stream with new exclusions, starting while the current one
    // keeps reporting.
    let mut replacement: Option<std::sync::mpsc::Receiver<Started>> = None;
    let mut last_replacement: Option<Instant> = None;

    let mut gate = RepositoryRefreshGate::new();
    // Compare once the stream is live, covering the moments since the bridge's last read.
    gate.note_event(Instant::now());
    let mut registry_touched = true;
    loop {
        if stop.try_recv().is_ok() {
            return;
        }
        if let Some(ready) = &replacement {
            match ready.try_recv() {
                Ok(Ok((next, next_excluded))) => {
                    let old = std::mem::replace(&mut watcher, next);
                    // Stopping a stream can take seconds on a busy Mac; never on this thread.
                    std::thread::spawn(move || drop(old));
                    excluded = next_excluded;
                    probe.streams.fetch_add(1, Ordering::SeqCst);
                    *probe.excluded.lock_or_recover() = excluded.clone();
                    replacement = None;
                    last_replacement = Some(Instant::now());
                    // Read once more: a newly watched directory's changes went unreported.
                    gate.note_event(Instant::now());
                }
                Ok(Err(error)) => {
                    tracing::warn!(
                        checkout_root = %root.display(),
                        %error,
                        "repository watcher kept its stream: the replacement did not start"
                    );
                    replacement = None;
                    last_replacement = Some(Instant::now());
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => replacement = None,
            }
        }
        match rx.recv_timeout(gate.wait(Instant::now())) {
            Ok(result) => {
                probe.delivered.fetch_add(1, Ordering::Relaxed);
                if touches_group_registry(&root, &result) {
                    registry_touched = true;
                    gate.note_event(Instant::now());
                } else if filter.is_relevant(&result) {
                    gate.note_event(Instant::now());
                } else if exclude && let Ok(event) = &result {
                    for path in &event.paths {
                        if let Some(entry) = filter.ignored_entry(path) {
                            *churn.entry(entry.clone()).or_default() += 1;
                        }
                    }
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
        if std::mem::take(&mut registry_touched) {
            announce_group_registry_change(&root, &mut registry, &checkout_id, &events, &event_log);
        }
        announce_repository_change(&root, &mut previous, &checkout_id, &events, &event_log);
        // Events without a status change are usually new ignored output; a status change may
        // be a `.gitignore` edit or a forced add that stops Git ignoring a directory. Either
        // way, learn the ignored set again (HS2-2HZN2C; before, only the first case).
        filter.refresh_ignored(&root);
        if exclude && replacement.is_none() {
            let desired = desired_repository_exclusions(&root, &filter, &churn);
            let still_ignored =
                |path: &FsPath| filter.ignored.iter().any(|entry| entry == path) || !path.exists();
            if crate::repository_watch_exclusions::replacement_due(
                &excluded,
                &desired,
                still_ignored,
                last_replacement,
                now,
            ) {
                replacement = Some(spawn_start(desired));
            }
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
    let mut registry = group_registry_fingerprint(&root);
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
        // Without native events, the registry listing is compared on the same cadence (HS2-70JS8B).
        announce_group_registry_change(&root, &mut registry, &checkout_id, &events, &event_log);
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
    use std::sync::mpsc::RecvTimeoutError;

    let freshness = target.entry.freshness.clone();
    let mut retry_delay = INDEX_REPAIR_RETRY_MIN;
    let mut shard_rechecks = ShardRechecks::default();
    loop {
        // Retry queued index repairs on a bounded backoff so a store with no further
        // writes (and a missed watcher echo) still converges (HS2-5BAEX9). This is a
        // local timer over in-memory state; it issues no network requests. A pending
        // one-shot shard re-read (HS2-ZER3MA) may end the wait earlier.
        let repair_at = Instant::now() + retry_delay;
        let received = loop {
            let deadline = shard_rechecks
                .next_due()
                .map_or(repair_at, |due| due.min(repair_at));
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(first) => break Ok(first),
                Err(RecvTimeoutError::Timeout) => {
                    if apply_due_shard_rechecks(&target, &mut shard_rechecks, Instant::now()) {
                        regenerate_target_worklists(&target);
                    }
                    if Instant::now() >= repair_at {
                        break Err(RecvTimeoutError::Timeout);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    break Err(RecvTimeoutError::Disconnected);
                }
            }
        };
        let first = match received {
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
        // Debounce a burst (editor save, git checkout touching many files), but close the
        // batch within a bound: a stream that never pauses must not starve it (HS2-TFBFRP).
        let opened = Instant::now();
        while let Some(wait) = watch_batch_wait(opened, Instant::now()) {
            match rx.recv_timeout(wait) {
                Ok(next) => batch.add(&target, next),
                Err(_) => break,
            }
        }
        let WatchBatch {
            mut paths,
            events,
            goes_live,
            reconcile,
            shard_dirs,
        } = batch;
        paths.sort();
        paths.dedup();
        let mut index_changed = false;
        let mut reconciled = true;
        if reconcile {
            match target
                .entry
                .index
                .lock_or_recover()
                .reconcile_with_invalidations(&target.entry.store)
            {
                Ok((upserted, deleted, corrupt_source_changed)) => {
                    index_changed = upserted + deleted > 0 || corrupt_source_changed;
                    if index_changed {
                        // Files may have appeared before the replacement native watch.
                        // An aggregate invalidation makes every client reread the index.
                        emit_change(
                            &target.event_log,
                            &target.events,
                            ChangeEvent {
                                live: None,
                                cursor: None,
                                store: target.store_id.clone(),
                                kind: "changed".into(),
                                id: String::new(),
                                slug: String::new(),
                                message: None,
                                activity: None,
                                assignment: None,
                                turn: None,
                            },
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(store = %target.store_id, %error, "rearmed store index reconciliation failed");
                    reconciled = false;
                    freshness.set_live(false);
                }
            }
        }
        for path in &paths {
            index_changed |= handle_path_change(&target, path);
        }
        // Every event in the batch is now in the index (HS2-3R0C6G).
        if goes_live && reconciled {
            freshness.set_live(true);
        }
        freshness.events_applied(events);
        let applied = Instant::now();
        for dir in shard_dirs {
            shard_rechecks.schedule(dir, applied);
        }
        // A stream that never pauses must not starve a due re-read either.
        index_changed |= apply_due_shard_rechecks(&target, &mut shard_rechecks, applied);
        if !index_changed {
            continue;
        }
        regenerate_target_worklists(&target);
    }
}

/// How long after applying a batch that named a new shard directory the watch loop lists
/// that directory once more (HS2-ZER3MA).
const SHARD_RECHECK_DELAY: Duration = Duration::from_secs(1);

/// One-shot re-reads of shard directories (`tickets/<XY>`) that applied batches named
/// (HS2-ZER3MA). notify's inotify backend delivers a new directory's create event before
/// it arms the directory's watch, so a ticket file created in between reports no event.
/// Listing the directory when its event is applied covers a file that landed earlier; one
/// more listing after [`SHARD_RECHECK_DELAY`] covers a file that landed after that listing
/// but before the watch armed. Each directory event schedules at most one re-read, and a
/// directory already pending is not rescheduled: this is never a repeating poll.
#[derive(Default)]
struct ShardRechecks {
    pending: Vec<(std::path::PathBuf, Instant)>,
}

impl ShardRechecks {
    fn schedule(&mut self, dir: std::path::PathBuf, applied: Instant) {
        if self.pending.iter().all(|(pending, _)| *pending != dir) {
            self.pending.push((dir, applied + SHARD_RECHECK_DELAY));
        }
    }

    fn next_due(&self) -> Option<Instant> {
        self.pending.iter().map(|(_, due)| *due).min()
    }

    /// Removes and returns every directory whose re-read is due at `now`.
    fn take_due(&mut self, now: Instant) -> Vec<std::path::PathBuf> {
        let mut due = Vec::new();
        self.pending.retain(|(dir, at)| {
            if *at <= now {
                due.push(dir.clone());
                false
            } else {
                true
            }
        });
        due
    }
}

/// Re-reads each due shard directory and applies its ticket files whose bytes differ from
/// the index; returns whether the index changed. Already indexed files are skipped, so a
/// re-read after the normal event path applied a file announces nothing again.
fn apply_due_shard_rechecks(
    target: &WatchTarget,
    rechecks: &mut ShardRechecks,
    now: Instant,
) -> bool {
    let mut changed = false;
    for dir in rechecks.take_due(now) {
        for path in stale_shard_files(target, &dir) {
            changed |= handle_path_change(target, &path);
        }
    }
    changed
}

/// Whether a raw event path is a shard directory directly under the store's `tickets/`
/// directory (HS2-ZER3MA).
fn is_shard_dir(path: &FsPath) -> bool {
    path.parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|name| name == "tickets")
        && path.is_dir()
}

/// How long the watch loop waits for more events after the last one before applying a batch.
const WATCH_BATCH_QUIET: Duration = Duration::from_millis(150);
/// The longest a batch stays open while events keep arriving (HS2-TFBFRP). Without it, a
/// continuous stream (a bulk import, or unrelated churn the stream reports) kept the batch
/// open indefinitely and no ticket write in it was applied.
const WATCH_BATCH_MAX: Duration = Duration::from_secs(1);

/// How long to wait for the next event of a batch opened at `opened`, or `None` once the
/// batch must be applied.
fn watch_batch_wait(opened: Instant, now: Instant) -> Option<Duration> {
    let left = WATCH_BATCH_MAX.checked_sub(now.saturating_duration_since(opened))?;
    (!left.is_zero()).then(|| left.min(WATCH_BATCH_QUIET))
}

/// One debounced burst of watcher events: the ticket files to apply, how many queued
/// events it consumed, and whether applying it makes the watcher live.
#[derive(Default)]
struct WatchBatch {
    paths: Vec<std::path::PathBuf>,
    events: usize,
    goes_live: bool,
    reconcile: bool,
    /// Shard directories the batch's events named, each re-read once after it is applied
    /// (HS2-ZER3MA).
    shard_dirs: Vec<std::path::PathBuf>,
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
        if let Ok(event) = &res
            && event.info() == Some(REARM_RESYNC)
        {
            self.reconcile = true;
            self.goes_live = true;
            return;
        }
        if let Ok(event) = &res
            && event.info().is_none()
        {
            for dir in event.paths.iter().filter(|path| is_shard_dir(path)) {
                if !self.shard_dirs.contains(dir) {
                    self.shard_dirs.push(dir.clone());
                }
            }
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
    stale_ticket_files_with_reader(target, tickets_dir, |path| std::fs::read(path))
}

fn stale_ticket_files_with_reader(
    target: &WatchTarget,
    tickets_dir: &FsPath,
    read: impl Fn(&FsPath) -> std::io::Result<Vec<u8>>,
) -> Vec<std::path::PathBuf> {
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
    stale_among(target, files, read)
}

/// The ticket files directly inside one shard directory whose content hash differs from
/// the index (HS2-ZER3MA). Unlike [`stale_ticket_files`] it does not descend.
fn stale_shard_files(target: &WatchTarget, shard: &FsPath) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(shard) else {
        return Vec::new();
    };
    let files = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    stale_among(target, files, |path| std::fs::read(path))
}

/// The subset of `files` whose content hash differs from the store's index.
fn stale_among(
    target: &WatchTarget,
    files: Vec<std::path::PathBuf>,
    read: impl Fn(&FsPath) -> std::io::Result<Vec<u8>>,
) -> Vec<std::path::PathBuf> {
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
            let bytes = read(&path).ok()?;
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

/// Whether an event only reports a file being opened or closed without a write (HS2-P0EHD4).
/// notify 8's inotify backend subscribes to `IN_OPEN`, so on Linux every read of a watched
/// file reports an access event. Treating one as a change would make a watcher re-read the file
/// it was told about, which reports another access: a read loop. A write still arrives as
/// `Modify` (and `Access(Close(Write))` carries no new content), so access events are dropped.
pub(crate) fn is_access_event(res: &notify::Result<notify::Event>) -> bool {
    matches!(res, Ok(event) if matches!(event.kind, notify::EventKind::Access(_)))
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
                live: None,
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
        spawn_repository_monitor, watch_root_signal,
    };
    use notify::{Event, EventKind};
    use std::sync::{Arc, Mutex};

    #[cfg(windows)]
    #[test]
    fn hs2_r6fz54_windows_directory_identity_changes_after_recreation() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("store");
        std::fs::create_dir(&root).unwrap();
        let original = super::watched_directory_identity(&root).unwrap();
        std::fs::rename(&root, parent.path().join("old-store")).unwrap();
        std::fs::create_dir(&root).unwrap();
        assert_ne!(super::watched_directory_identity(&root), Some(original));
    }

    #[test]
    fn hs2_r6fz54_only_root_and_tickets_events_rearm_a_store() {
        let root = std::path::Path::new("/synthetic/store");
        let tickets = root.join("tickets");
        let sibling = root.with_file_name("other");
        assert!(
            watch_root_signal(
                &Event::new(EventKind::Create(notify::event::CreateKind::Folder)).add_path(sibling),
                root,
                &tickets,
                None,
                None,
            )
            .is_none()
        );
        let removed = watch_root_signal(
            &Event::new(EventKind::Remove(notify::event::RemoveKind::Folder))
                .add_path(root.to_path_buf()),
            root,
            &tickets,
            None,
            None,
        )
        .unwrap();
        assert!(removed.root_removed);
        assert!(!removed.tickets_removed);
        let created = watch_root_signal(
            &Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                .add_path(tickets.clone()),
            root,
            &tickets,
            None,
            None,
        )
        .unwrap();
        assert!(!created.root_removed && !created.tickets_removed);
        let canonical = std::path::Path::new("/private/synthetic/store");
        assert!(
            watch_root_signal(
                &Event::new(EventKind::Create(notify::event::CreateKind::Folder))
                    .add_path(canonical.to_path_buf()),
                root,
                &tickets,
                Some(canonical),
                Some(&canonical.join("tickets")),
            )
            .is_some()
        );
    }
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
    /// needs must stay free while it does (HS2-P867K1). A controlled read of a regular ticket
    /// file holds the walk at that boundary until the test releases it (HS2-TES8M0).
    #[test]
    fn a_resync_reads_ticket_files_without_holding_the_index_lock() {
        use hotsheet_model::Ulid;
        use hotsheet_ticketing::{FsStore, StoreMetadata};
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        let state = super::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let tickets = dir.path().join("tickets");
        let shard = tickets.join("ZZ");
        std::fs::create_dir_all(&shard).unwrap();
        let file = shard.join(format!("{}.md", Ulid::new()));
        std::fs::write(&file, b"synthetic ticket bytes").unwrap();
        let index = target.entry.index.clone();
        let (reading_tx, reading_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let walk = std::thread::spawn({
            let tickets = tickets.clone();
            move || {
                let stale = super::stale_ticket_files_with_reader(&target, &tickets, |path| {
                    let bytes = std::fs::read(path)?;
                    reading_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(bytes)
                })
                .len();
                let _ = result_tx.send(stale);
            }
        });
        reading_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("resync did not reach the ticket read");
        let lock_free = index.try_lock().is_ok();
        let still_blocked = matches!(
            result_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        );
        release_tx.send(()).unwrap();
        // The unindexed "file" is reported stale once the walk finishes.
        let stale = result_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("resync did not finish after the read was released");
        walk.join().unwrap();
        assert!(
            lock_free,
            "the resync held the index lock while reading ticket files"
        );
        assert!(still_blocked, "resync finished during the blocked read");
        assert_eq!(stale, 1);
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
            Ok(
                hotsheet_jobs::watch::Event::new(hotsheet_jobs::watch::EventKind::Any)
                    .add_path(path),
            )
        };
        let git_dir = filter.git_dirs[0].clone();

        assert!(!filter.is_relevant(&event(root.join("target/debug/artifact"))));
        assert!(!filter.is_relevant(&event(git_dir.join("objects/ab/cdef"))));
        assert!(filter.is_relevant(&event(git_dir.join("index.lock"))));
        assert!(filter.is_relevant(&event(git_dir.join("index"))));
        assert!(filter.is_relevant(&event(git_dir.join("refs/heads/main"))));
        assert!(filter.is_relevant(&event(root.join("src/lib.rs"))));
        assert!(filter.is_relevant(&Err(hotsheet_jobs::watch::Error::generic("dropped"))));
        // A canonical spelling of an ignored path (macOS /private/var) is still ignored.
        let canonical = hotsheet_sync::paths::canonicalize(root)
            .unwrap()
            .join("target/debug/artifact");
        assert!(!filter.is_relevant(&event(canonical)));

        // An ignored directory created after startup is learned on refresh.
        std::fs::write(root.join(".gitignore"), "target/\ndist/\n").unwrap();
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/bundle.js"), "x").unwrap();
        assert!(filter.is_relevant(&event(root.join("dist/bundle.js"))));
        filter.refresh_ignored(root);
        assert!(!filter.is_relevant(&event(root.join("dist/bundle.js"))));

        // HS2-2HZN2C: Git lists `.claude/` as ignored while it holds only an ignored
        // directory, but no pattern ignores it: a new file beside the ignored one counts.
        std::fs::write(
            root.join(".gitignore"),
            "target/\ndist/\n.claude/worktrees/\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".claude/worktrees/agent")).unwrap();
        std::fs::write(root.join(".claude/worktrees/agent/file"), "x").unwrap();
        filter.refresh_ignored(root);
        assert!(filter.pattern_checked);
        assert!(!filter.is_relevant(&event(root.join(".claude/worktrees/agent/file"))));
        assert!(filter.is_relevant(&event(root.join(".claude/settings.json"))));
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

    /// HS2-70JS8B: a group registry entry write (a rename by another process) and a write to the
    /// ignored remote-group overlay each announce `group_registry_changed` for the checkout, from
    /// the native and the Git-status monitor alike; an idle registry stays silent.
    #[tokio::test]
    async fn repository_monitors_announce_group_registry_changes() {
        for (name, monitor) in [
            (
                "native",
                run_native_repository_monitor as super::RepositoryMonitor,
            ),
            (
                "git",
                run_git_repository_monitor as super::RepositoryMonitor,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            git(root, &["init", "--quiet"]);
            // The overlay is machine-local and ignored, as in a real checkout.
            std::fs::write(root.join(".gitignore"), ".hotsheet2/groups-remote.json\n").unwrap();
            let registry = root.join(hotsheet_ticketing::groups::GROUP_REGISTRY_DIR);
            std::fs::create_dir_all(&registry).unwrap();
            std::fs::write(
                registry.join("01GROUPAAAAAAAAAAAAAAAAAAA.json"),
                r#"{"name":"Before"}"#,
            )
            .unwrap();
            let (events, _) = tokio::sync::broadcast::channel(32);
            let mut receiver = events.subscribe();
            let handle = spawn_repository_monitor(
                root.to_path_buf(),
                format!("checkout-{name}"),
                events,
                Arc::new(Mutex::new(EventLog::default())),
                monitor,
            )
            .unwrap();
            tokio::time::sleep(Duration::from_millis(1_000)).await;
            while receiver.try_recv().is_ok() {}
            let registry_event =
                async |receiver: &mut tokio::sync::broadcast::Receiver<super::ChangeEvent>,
                       what: &str| {
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
                    loop {
                        let event = tokio::time::timeout_at(deadline, receiver.recv())
                            .await
                            .unwrap_or_else(|_| panic!("{name}: {what} was not announced"))
                            .unwrap();
                        if event.kind == "group_registry_changed" {
                            assert_eq!(event.id, format!("checkout-{name}"));
                            assert!(event.slug.is_empty() && event.message.is_none());
                            return;
                        }
                    }
                };

            std::fs::write(
                registry.join("01GROUPAAAAAAAAAAAAAAAAAAA.json"),
                r#"{"name":"Renamed"}"#,
            )
            .unwrap();
            registry_event(&mut receiver, "a registry rename").await;
            std::fs::write(
                root.join(hotsheet_ticketing::groups::git_sync::GROUP_REMOTE_OVERLAY_FILE),
                r#"{"entries":[]}"#,
            )
            .unwrap();
            registry_event(&mut receiver, "an overlay write").await;
            let quiet: Vec<_> = {
                tokio::time::sleep(Duration::from_millis(2_000)).await;
                std::iter::from_fn(|| receiver.try_recv().ok())
                    .filter(|event| event.kind == "group_registry_changed")
                    .collect()
            };
            assert!(
                quiet.is_empty(),
                "{name}: an idle registry was announced: {quiet:?}"
            );
            drop(handle);
        }
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
    use super::{
        LIVE_RESYNC, QueuedSender, REARM_RESYNC, WatcherBackend, resync_event, watch_loop,
    };
    use crate::store_watch_scope::TicketsWatchScope;
    use hotsheet_index::IndexError;
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
    use std::sync::Arc;
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

    // HS2-R6FZ54: macOS FSEvents may spend tens of seconds in a native watch()
    // registration even when this test has the full Nextest runner. Wait for the
    // observable rearm result instead of mistaking registration latency for a lost watch.
    // HS2-190GDZ: a startup hook runs only after that registration, so it shares the bound.
    fn native_deadline() -> Duration {
        if cfg!(target_os = "macos") {
            Duration::from_secs(120)
        } else {
            Duration::from_secs(30)
        }
    }

    fn wait_for_native_rearm(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + native_deadline();
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn movable_watch_scopes() -> &'static [bool] {
        // HS2-VN96J5: on NTFS a watcher beneath the store prevents its parent
        // directory from moving. Windows production watches the root for that
        // reason; the tickets scope is movable only on Unix.
        if cfg!(windows) {
            &[true]
        } else {
            &[false, true]
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

    /// HS2-ZER3MA: each directory event schedules one re-read; a pending directory is not
    /// rescheduled, a taken re-read is gone, and a later event schedules a fresh one.
    #[test]
    fn hs2_zer3ma_a_shard_recheck_is_one_shot_per_directory_event() {
        use super::{SHARD_RECHECK_DELAY, ShardRechecks};
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let shard = std::path::PathBuf::from("tickets/01");
        let other = std::path::PathBuf::from("tickets/02");
        let mut rechecks = ShardRechecks::default();
        assert_eq!(rechecks.next_due(), None, "nothing pending at first");
        assert!(rechecks.take_due(at(10_000)).is_empty());

        rechecks.schedule(shard.clone(), start);
        // A repeated event for a pending directory neither duplicates nor postpones it.
        rechecks.schedule(shard.clone(), at(500));
        rechecks.schedule(other.clone(), at(300));
        assert_eq!(rechecks.next_due(), Some(start + SHARD_RECHECK_DELAY));
        assert!(
            rechecks.take_due(at(999)).is_empty(),
            "not due before the delay"
        );
        assert_eq!(rechecks.take_due(at(1_000)), vec![shard.clone()]);
        assert_eq!(
            rechecks.next_due(),
            Some(at(300) + SHARD_RECHECK_DELAY),
            "the interleaved directory keeps its own due time"
        );
        assert_eq!(rechecks.take_due(at(5_000)), vec![other]);
        assert_eq!(rechecks.next_due(), None, "a taken re-read never repeats");

        // Empty, then refilled by a later event for the same directory.
        rechecks.schedule(shard.clone(), at(6_000));
        assert_eq!(rechecks.take_due(at(7_000)), vec![shard]);
        assert!(rechecks.take_due(at(60_000)).is_empty());
    }

    /// HS2-ZER3MA: a due re-read indexes a ticket file that arrived with no event, and a
    /// re-read of a file already indexed announces nothing (idempotent).
    #[test]
    fn hs2_zer3ma_a_due_shard_recheck_indexes_only_unindexed_files() {
        use super::{SHARD_RECHECK_DELAY, ShardRechecks, apply_due_shard_rechecks};
        let (_dir, store) = store();
        let state = crate::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        target.entry.freshness.set_live(true);
        let mut changes = target.events.subscribe();
        let ticket = create(&store, "landed without an event");
        let shard = store
            .ticket_path(&ticket.id)
            .parent()
            .unwrap()
            .to_path_buf();
        assert!(target.entry.resolve(&ticket.slug).unwrap().is_none());

        let mut rechecks = ShardRechecks::default();
        let applied = Instant::now();
        rechecks.schedule(shard.clone(), applied);
        assert!(!apply_due_shard_rechecks(&target, &mut rechecks, applied));
        assert!(target.entry.resolve(&ticket.slug).unwrap().is_none());
        assert!(apply_due_shard_rechecks(
            &target,
            &mut rechecks,
            applied + SHARD_RECHECK_DELAY
        ));
        assert_eq!(
            target.entry.resolve(&ticket.slug).unwrap().unwrap().id,
            ticket.id
        );
        let announced = changes.try_recv().unwrap();
        assert_eq!(
            (announced.kind.as_str(), announced.slug.as_str()),
            ("changed", ticket.slug.as_str())
        );

        // Already indexed: a second directory event's re-read is a no-op.
        rechecks.schedule(shard, applied);
        assert!(!apply_due_shard_rechecks(
            &target,
            &mut rechecks,
            applied + SHARD_RECHECK_DELAY
        ));
        assert!(changes.try_recv().is_err(), "nothing announced again");
        // A missing directory (removed since its event) is skipped quietly.
        rechecks.schedule(_dir.path().join("tickets/ZZ"), applied);
        assert!(!apply_due_shard_rechecks(
            &target,
            &mut rechecks,
            applied + SHARD_RECHECK_DELAY
        ));
    }

    /// HS2-ZER3MA: the real watch loop applies a new shard directory's event while the
    /// directory is still empty; the ticket file then lands with no event (the inotify
    /// window before the new directory's watch arms) and must still be indexed.
    #[test]
    fn hs2_zer3ma_a_file_landing_after_its_shard_event_is_indexed() {
        let (dir, store) = store();
        let state = crate::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let entry = target.entry.clone();
        let freshness = entry.freshness.clone();
        let mut changes = target.events.subscribe();
        let (raw, rx) = std::sync::mpsc::channel();
        let tx = QueuedSender {
            tx: raw,
            freshness: freshness.clone(),
        };
        let looping = std::thread::spawn(move || watch_loop(rx, target));
        tx.send(Ok(resync_event(&dir.path().join("tickets"), LIVE_RESYNC)));
        wait_for("the live resync", || freshness.in_sync());

        for round in 0..2 {
            // Capture a real ticket's bytes, then empty its shard directory.
            let ticket = create(&store, &format!("late file {round}"));
            let path = store.ticket_path(&ticket.id);
            let shard = path.parent().unwrap().to_path_buf();
            let bytes = std::fs::read(&path).unwrap();
            std::fs::remove_file(&path).unwrap();

            let folder = notify::EventKind::Create(notify::event::CreateKind::Folder);
            tx.send(Ok(notify::Event::new(folder).add_path(shard)));
            wait_for("the directory event", || freshness.in_sync());
            assert!(
                entry.resolve(&ticket.slug).unwrap().is_none(),
                "round {round}: the directory was empty when its event applied"
            );

            std::fs::write(&path, bytes).unwrap();
            wait_for("the one-shot shard re-read", || {
                entry.resolve(&ticket.slug).unwrap().is_some()
            });
            let announced = loop {
                let change = changes.try_recv().unwrap();
                if change.slug == ticket.slug {
                    break change;
                }
            };
            assert_eq!(announced.kind, "changed");
        }

        drop(tx);
        looping.join().unwrap();
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

    #[test]
    fn hs2_tfbfrp_a_batch_closes_after_a_quiet_gap_or_its_bound() {
        let opened = Instant::now();
        assert_eq!(
            super::watch_batch_wait(opened, opened),
            Some(super::WATCH_BATCH_QUIET)
        );
        let late = opened + super::WATCH_BATCH_MAX - Duration::from_millis(40);
        assert_eq!(
            super::watch_batch_wait(opened, late),
            Some(Duration::from_millis(40))
        );
        assert_eq!(
            super::watch_batch_wait(opened, opened + super::WATCH_BATCH_MAX),
            None
        );
        assert_eq!(
            super::watch_batch_wait(opened, opened + super::WATCH_BATCH_MAX * 3),
            None
        );
    }

    /// A stream that never pauses for the quiet gap kept the batch open, so a ticket
    /// write queued in it was never applied (HS2-TFBFRP). Each write must land while the
    /// stream continues, including repeated writes to the same ticket.
    #[test]
    fn hs2_tfbfrp_a_continuous_event_stream_does_not_starve_a_ticket_write() {
        let (dir, store) = store();
        let state = crate::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let entry = target.entry.clone();
        let mut events = target.events.subscribe();
        let (raw, rx) = std::sync::mpsc::channel();
        let tx = QueuedSender {
            tx: raw,
            freshness: entry.freshness.clone(),
        };
        let looping = std::thread::spawn(move || watch_loop(rx, target));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let noise = {
            let tx = tx.clone();
            let stop = stop.clone();
            let lock = dir.path().join("tickets").join("not-a-ticket.lock");
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                    tx.send(Ok(
                        notify::Event::new(notify::EventKind::Any).add_path(lock.clone())
                    ));
                    std::thread::sleep(Duration::from_millis(20));
                }
            })
        };
        let ticket = create(&store, "written during a stream");
        let path = store.ticket_path(&ticket.id);
        let mut previous = "written during a stream";
        for title in ["first edit", "second edit"] {
            let body = std::fs::read_to_string(&path)
                .unwrap()
                .replace(previous, title);
            std::fs::write(&path, body).unwrap();
            previous = title;
            tx.send(Ok(
                notify::Event::new(notify::EventKind::Any).add_path(path.clone())
            ));
            wait_for("the ticket write to be applied mid-stream", || {
                while let Ok(event) = events.try_recv() {
                    if event.kind == "changed" && event.id == ticket.id.to_string() {
                        return true;
                    }
                }
                false
            });
            assert_eq!(
                entry
                    .index
                    .lock()
                    .unwrap()
                    .content_hash(&ticket.id)
                    .unwrap(),
                Some(hotsheet_index::hash_bytes(&std::fs::read(&path).unwrap()))
            );
        }
        assert!(!noise.is_finished(), "the stream was still running");
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        noise.join().unwrap();
        drop(tx);
        looping.join().unwrap();
    }

    #[test]
    fn hs2_r6fz54_rearm_resync_reconciles_removed_and_new_tickets() {
        let (dir, store) = store();
        let old = create(&store, "old store ticket");
        let state = crate::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let entry = target.entry.clone();
        let mut events = target.events.subscribe();
        let (raw, rx) = std::sync::mpsc::channel();
        let tx = QueuedSender {
            tx: raw,
            freshness: entry.freshness.clone(),
        };
        let looping = std::thread::spawn(move || watch_loop(rx, target));
        let tickets = dir.path().join("tickets");
        tx.send(Ok(resync_event(&tickets, LIVE_RESYNC)));
        wait_for("initial store to become live", || entry.freshness.in_sync());

        let away = dir.path().with_file_name(format!(
            "{}-away",
            dir.path().file_name().unwrap().to_string_lossy()
        ));
        std::fs::rename(dir.path(), &away).unwrap();
        entry.freshness.set_live(false);
        let replacement = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        let new = create(&replacement, "replacement store ticket");
        tx.send(Ok(resync_event(&tickets, REARM_RESYNC)));
        wait_for("replacement store to reconcile", || {
            entry.freshness.in_sync()
        });
        assert!(
            entry
                .index
                .lock()
                .unwrap()
                .slug_candidates(&old.slug)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            entry
                .index
                .lock()
                .unwrap()
                .slug_candidates(&new.slug)
                .unwrap(),
            vec![new.id]
        );
        wait_for("replacement store to announce an index change", || {
            while let Ok(event) = events.try_recv() {
                if event.kind == "changed" && event.id.is_empty() {
                    return true;
                }
            }
            false
        });
        drop(tx);
        looping.join().unwrap();
        std::fs::remove_dir_all(away).unwrap();
    }

    #[test]
    fn hs2_r6fz54_rearm_resync_invalidates_a_corrupt_source_hash() {
        let (_dir, store) = store();
        let ticket = create(&store, "healthy before rearm");
        let state = crate::AppState::new(store.clone(), "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let entry = target.entry.clone();
        let mut events = target.events.subscribe();
        let healthy_hash = entry
            .index
            .lock()
            .unwrap()
            .content_hash(&ticket.id)
            .unwrap();
        let corrupt_bytes = format!(
            "{}unexpected trailing content\n",
            hotsheet_model::to_file_string(&ticket)
        );
        std::fs::write(store.ticket_path(&ticket.id), corrupt_bytes.as_bytes()).unwrap();

        let (raw, rx) = std::sync::mpsc::channel();
        let tx = QueuedSender {
            tx: raw,
            freshness: entry.freshness.clone(),
        };
        let looping = std::thread::spawn(move || watch_loop(rx, target));
        tx.send(Ok(resync_event(
            &store.root().join("tickets"),
            REARM_RESYNC,
        )));
        let corrupt_hash = hotsheet_index::hash_bytes(corrupt_bytes.as_bytes());
        assert_ne!(healthy_hash.as_deref(), Some(corrupt_hash.as_str()));
        wait_for("corrupt source hash to be recorded during rearm", || {
            entry.freshness.in_sync()
                && entry
                    .index
                    .lock()
                    .unwrap()
                    .content_hash(&ticket.id)
                    .unwrap()
                    .as_deref()
                    == Some(corrupt_hash.as_str())
        });
        assert_eq!(
            entry
                .index
                .lock()
                .unwrap()
                .slug_candidates(&ticket.slug)
                .unwrap(),
            vec![ticket.id],
            "the last healthy row stays indexed"
        );
        wait_for("corrupt source hash to invalidate clients", || {
            while let Ok(event) = events.try_recv() {
                if event.kind == "changed" && event.id.is_empty() {
                    return true;
                }
            }
            false
        });
        drop(tx);
        looping.join().unwrap();
    }

    #[test]
    fn hs2_r6fz54_native_watcher_rearms_after_store_recreation() {
        for &watch_root in movable_watch_scopes() {
            let parent = tempfile::tempdir().unwrap();
            let root = parent.path().join("store");
            let away = parent.path().join("away");
            let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
            let old = create(&store, "old store ticket");
            let state = crate::AppState::new(store, "secret".into()).unwrap();
            let target = super::default_watch_target(&state);
            let entry = target.entry.clone();
            let scope = Arc::new(TicketsWatchScope::new(&root, watch_root));
            let handle =
                super::spawn_watcher_in_scope(target, WatcherBackend::Recommended, scope).unwrap();
            wait_for_native_rearm("native watcher to become live", || {
                entry.freshness.in_sync()
            });

            std::fs::rename(&root, &away).unwrap();
            wait_for_native_rearm(
                &format!("deleted store to be marked out of sync (watch_root={watch_root})"),
                || !entry.freshness.in_sync(),
            );
            let replacement = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
            let first = create(&replacement, "replacement ticket");
            wait_for_native_rearm("recreated store to replace its index", || {
                entry.freshness.in_sync()
                    && entry
                        .index
                        .lock()
                        .unwrap()
                        .slug_candidates(&old.slug)
                        .unwrap()
                        .is_empty()
                    && entry
                        .index
                        .lock()
                        .unwrap()
                        .slug_candidates(&first.slug)
                        .unwrap()
                        == vec![first.id]
            });
            let later = create(&replacement, "external write after rearm");
            wait_for_native_rearm("rearmed native watch to index an external write", || {
                entry
                    .index
                    .lock()
                    .unwrap()
                    .slug_candidates(&later.slug)
                    .unwrap()
                    == vec![later.id]
            });

            std::fs::rename(&root, parent.path().join("away-again")).unwrap();
            wait_for_native_rearm(
                &format!("second removal to mark the store out of sync (watch_root={watch_root})"),
                || !entry.freshness.in_sync(),
            );
            let replacement = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
            let second = create(&replacement, "second replacement ticket");
            wait_for_native_rearm("second recreation to replace its index", || {
                entry.freshness.in_sync()
                    && entry
                        .index
                        .lock()
                        .unwrap()
                        .slug_candidates(&later.slug)
                        .unwrap()
                        .is_empty()
                    && entry
                        .index
                        .lock()
                        .unwrap()
                        .slug_candidates(&second.slug)
                        .unwrap()
                        == vec![second.id]
            });
            drop(handle);
        }
    }

    #[test]
    fn hs2_r6fz54_recreation_during_native_startup_replaces_the_watched_directory() {
        for (watch_root, before_identity_sample) in
            [(false, false), (false, true), (true, false), (true, true)]
                .into_iter()
                .filter(|(watch_root, _)| movable_watch_scopes().contains(watch_root))
        {
            let parent = tempfile::tempdir().unwrap();
            let root = parent.path().join("store");
            let away = parent.path().join("old-store");
            let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
            let old = create(&store, "old startup ticket");
            let state = crate::AppState::new(store, "secret".into()).unwrap();
            let target = super::default_watch_target(&state);
            let entry = target.entry.clone();
            let scope = Arc::new(TicketsWatchScope::new(&root, watch_root));
            let (ready, replaced) = std::sync::mpsc::channel();
            let replacement_root = root.clone();
            let hook: super::StartupHook = Box::new(move || {
                std::fs::rename(&replacement_root, &away).unwrap();
                let store = FsStore::init(&replacement_root, &StoreMetadata::new("HS")).unwrap();
                let new = create(&store, "replacement during startup");
                ready.send((new.slug, new.id)).unwrap();
            });
            let (between_watch_and_identity, after_children) = if before_identity_sample {
                (Some(hook), None)
            } else {
                (None, Some(hook))
            };
            let handle = super::spawn_watcher_in_scope_with_startup_hooks(
                target,
                WatcherBackend::Recommended,
                scope,
                super::StartupHooks {
                    after_watch_before_identity: between_watch_and_identity,
                    after_children,
                    ..Default::default()
                },
            )
            .unwrap();
            let (new_slug, new_id) = replaced.recv_timeout(native_deadline()).unwrap();
            wait_for_native_rearm("startup replacement to reconcile", || {
                entry.freshness.in_sync()
                    && entry
                        .index
                        .lock()
                        .unwrap()
                        .slug_candidates(&old.slug)
                        .unwrap()
                        .is_empty()
                    && entry
                        .index
                        .lock()
                        .unwrap()
                        .slug_candidates(&new_slug)
                        .unwrap()
                        == vec![new_id]
            });
            let replacement = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
            let later = create(&replacement, "write after startup rearm");
            wait_for_native_rearm("startup rearm to index a later external write", || {
                entry
                    .index
                    .lock()
                    .unwrap()
                    .slug_candidates(&later.slug)
                    .unwrap()
                    == vec![later.id]
            });
            drop(handle);
        }
    }

    #[test]
    fn hs2_r6fz54_missing_child_during_native_startup_waits_for_recreation() {
        for watch_root in [false, true] {
            let parent = tempfile::tempdir().unwrap();
            let root = parent.path().join("store");
            let away = parent.path().join("old-store");
            let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
            let old = create(&store, "old startup ticket");
            let state = crate::AppState::new(store, "secret".into()).unwrap();
            let target = super::default_watch_target(&state);
            let entry = target.entry.clone();
            let scope = Arc::new(TicketsWatchScope::new(&root, watch_root));
            let removed_root = root.clone();
            let before = Box::new(move || std::fs::rename(&removed_root, &away).unwrap());
            let (ready, replaced) = std::sync::mpsc::channel();
            let replacement_root = root.clone();
            let after = Box::new(move || {
                let store = FsStore::init(&replacement_root, &StoreMetadata::new("HS")).unwrap();
                let new = create(&store, "replacement after missing child");
                ready.send((new.slug, new.id)).unwrap();
            });
            let handle = super::spawn_watcher_in_scope_with_startup_hooks(
                target,
                WatcherBackend::Recommended,
                scope,
                super::StartupHooks {
                    before_children: Some(before),
                    after_children: Some(after),
                    ..Default::default()
                },
            )
            .unwrap();
            let (new_slug, new_id) = replaced.recv_timeout(native_deadline()).unwrap();
            wait_for_native_rearm("missing startup child to reconcile", || {
                entry.freshness.in_sync()
                    && entry
                        .index
                        .lock()
                        .unwrap()
                        .slug_candidates(&old.slug)
                        .unwrap()
                        .is_empty()
                    && entry
                        .index
                        .lock()
                        .unwrap()
                        .slug_candidates(&new_slug)
                        .unwrap()
                        == vec![new_id]
            });
            drop(handle);
        }
    }
}

#[cfg(test)]
mod stream_proof_tests {
    //! HS2-190GDZ: a native registration is trusted only after it reports a probe.
    use super::{
        NativeWatcherState, ProofOutcome, STREAM_PROBE_PREFIX, StreamProof, WatcherBackend,
        WatcherSlot, prove_native_stream,
    };
    use crate::store_watch_scope::TicketsWatchScope;
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_sync::LockExt;
    use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn probe_event(path: PathBuf) -> notify::Event {
        notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::File))
            .add_path(path)
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The name of the `index`th probe written since the current registration began.
    fn pending(proof: &StreamProof, index: usize) -> Option<PathBuf> {
        proof
            .state
            .lock_or_recover()
            .pending
            .get(index)
            .map(PathBuf::from)
    }

    #[test]
    fn hs2_190gdz_a_reported_probe_proves_the_stream_and_is_not_a_store_change() {
        let dir = tempfile::tempdir().unwrap();
        let proof = Arc::new(StreamProof::default());
        assert!(proof.begin());
        let waiting = {
            let proof = proof.clone();
            let dir = dir.path().to_path_buf();
            std::thread::spawn(move || proof.probe(&dir, Duration::from_secs(60)))
        };
        wait_for("the probe to be written", || pending(&proof, 0).is_some());
        let name = pending(&proof, 0).unwrap();
        assert!(name.to_str().unwrap().starts_with(STREAM_PROBE_PREFIX));
        // A store change is not a probe and proves nothing.
        assert!(!proof.observe(&probe_event(
            dir.path().join("01ARZ3NDEKTSV4RRFFQ69G5FAV.md")
        )));
        // A probe another registration wrote is consumed but proves nothing either.
        let foreign = dir
            .path()
            .join(format!("{STREAM_PROBE_PREFIX}1.999999.tmp"));
        assert!(proof.observe(&probe_event(foreign)));
        assert!(!proof.state.lock_or_recover().seen);
        assert!(proof.observe(&probe_event(dir.path().join(&name))));
        assert_eq!(waiting.join().unwrap(), Some(ProofOutcome::Proven));
        // The probe file itself never outlives its write.
        assert!(!dir.path().join(&name).exists());
        // Once proven, a further probe returns at once without writing.
        assert_eq!(
            proof.probe(dir.path(), Duration::from_secs(60)),
            Some(ProofOutcome::Proven)
        );
    }

    #[test]
    fn hs2_190gdz_a_probe_from_an_earlier_registration_does_not_prove_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let proof = Arc::new(StreamProof::default());
        assert!(proof.begin());
        assert_eq!(proof.probe(dir.path(), Duration::from_millis(10)), None);
        let stale = pending(&proof, 0).unwrap();
        // The directory is registered again: the old probe arrives late.
        assert!(proof.begin());
        assert!(proof.observe(&probe_event(dir.path().join(&stale))));
        assert!(!proof.state.lock_or_recover().seen);
        let waiting = {
            let proof = proof.clone();
            let dir = dir.path().to_path_buf();
            std::thread::spawn(move || proof.probe(&dir, Duration::from_secs(60)))
        };
        wait_for("the new probe to be written", || {
            pending(&proof, 0).is_some()
        });
        let fresh = pending(&proof, 0).unwrap();
        assert_ne!(fresh, stale);
        assert!(proof.observe(&probe_event(dir.path().join(fresh))));
        assert_eq!(waiting.join().unwrap(), Some(ProofOutcome::Proven));
    }

    #[test]
    fn hs2_190gdz_dropping_the_handle_ends_a_proof_wait() {
        let dir = tempfile::tempdir().unwrap();
        let proof = Arc::new(StreamProof::default());
        assert!(proof.begin());
        let waiting = {
            let proof = proof.clone();
            let dir = dir.path().to_path_buf();
            std::thread::spawn(move || proof.probe(&dir, Duration::from_secs(600)))
        };
        wait_for("the probe to be written", || pending(&proof, 0).is_some());
        proof.stop();
        assert_eq!(waiting.join().unwrap(), Some(ProofOutcome::Stopped));
        // A stopped proof neither begins again nor writes another probe.
        assert!(!proof.begin());
        assert_eq!(
            proof.probe(dir.path(), Duration::from_secs(600)),
            Some(ProofOutcome::Stopped)
        );
    }

    #[test]
    fn hs2_190gdz_a_directory_that_takes_no_probe_is_unwritable() {
        let dir = tempfile::tempdir().unwrap();
        let proof = StreamProof::default();
        assert!(proof.begin());
        assert_eq!(
            proof.probe(&dir.path().join("missing"), Duration::from_secs(600)),
            Some(ProofOutcome::Unwritable)
        );
    }

    #[test]
    fn hs2_190gdz_the_simulated_busy_stream_drops_only_the_requested_probes() {
        let dir = tempfile::tempdir().unwrap();
        let proof = StreamProof::default();
        proof.state.lock_or_recover().drop_probes = 1;
        assert!(proof.begin());
        assert_eq!(proof.probe(dir.path(), Duration::from_millis(1)), None);
        let first = dir.path().join(pending(&proof, 0).unwrap());
        // Every event of the dropped probe is ignored, not just the first.
        assert!(proof.observe(&probe_event(first.clone())));
        assert!(proof.observe(&probe_event(first)));
        assert!(!proof.state.lock_or_recover().seen);
        assert_eq!(proof.probe(dir.path(), Duration::from_millis(1)), None);
        let second = dir.path().join(pending(&proof, 1).unwrap());
        assert!(proof.observe(&probe_event(second)));
        assert!(proof.state.lock_or_recover().seen);
    }

    /// Records registrations and, while armed, reports each probe as soon as it is written.
    struct FakeWatcher {
        calls: Arc<Mutex<Vec<String>>>,
        fail_watch: bool,
    }

    impl notify::Watcher for FakeWatcher {
        fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
            unreachable!("constructed directly")
        }
        fn watch(&mut self, path: &Path, _: notify::RecursiveMode) -> notify::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("watch {}", path.display()));
            if self.fail_watch {
                return Err(notify::Error::path_not_found());
            }
            Ok(())
        }
        fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("unwatch {}", path.display()));
            Ok(())
        }
        fn kind() -> notify::WatcherKind {
            notify::WatcherKind::NullWatcher
        }
    }

    struct FakeSlot {
        watcher: Option<FakeWatcher>,
        stopped: bool,
    }

    impl WatcherSlot<FakeWatcher> for FakeSlot {
        fn live_watcher(&mut self) -> Option<&mut FakeWatcher> {
            if self.stopped {
                return None;
            }
            self.watcher.as_mut()
        }
    }

    fn fake_slot(fail_watch: bool) -> (Mutex<FakeSlot>, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let slot = Mutex::new(FakeSlot {
            watcher: Some(FakeWatcher {
                calls: calls.clone(),
                fail_watch,
            }),
            stopped: false,
        });
        (slot, calls)
    }

    #[test]
    fn hs2_190gdz_unanswered_probes_register_the_directory_again() {
        let dir = tempfile::tempdir().unwrap();
        let proof = Arc::new(StreamProof::default());
        let (slot, calls) = fake_slot(false);
        let slot = Arc::new(slot);
        // The stream answers only once the directory has been registered again, as a
        // registration that came up dead under a busy fseventsd would.
        let answering = {
            let proof = proof.clone();
            let calls = calls.clone();
            let dir = dir.path().to_path_buf();
            std::thread::spawn(move || {
                wait_for("the directory to be registered again", || {
                    calls.lock().unwrap().len() >= 2
                });
                // Report the newest probe until one lands in the current registration;
                // another re-registration may race a report and void it.
                wait_for("a probe of the new registration to prove it", || {
                    let newest = proof.state.lock_or_recover().pending.last().cloned();
                    if let Some(name) = newest {
                        proof.observe(&probe_event(dir.join(name)));
                    }
                    proof.state.lock_or_recover().seen
                });
            })
        };
        let mut unanswered = Vec::new();
        let outcome = prove_native_stream(
            &proof,
            &*slot,
            dir.path(),
            Duration::from_millis(1),
            |count| unanswered.push(count),
        );
        answering.join().unwrap();
        assert_eq!(outcome, ProofOutcome::Proven);
        // Every unanswered probe was reported in order, and re-registration came in
        // unwatch/watch pairs, one per two unanswered probes.
        assert!(unanswered.len() >= 2);
        assert!(unanswered.iter().copied().eq(1..=unanswered.len() as u32));
        let display = dir.path().display();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), unanswered.len() / 2 * 2);
        for pair in calls.chunks(2) {
            assert_eq!(
                pair,
                [format!("unwatch {display}"), format!("watch {display}")]
            );
        }
    }

    #[test]
    fn hs2_190gdz_a_failed_registration_or_a_dropped_handle_ends_the_proof() {
        let dir = tempfile::tempdir().unwrap();
        let (failing, _) = fake_slot(true);
        assert_eq!(
            prove_native_stream(
                &StreamProof::default(),
                &failing,
                dir.path(),
                Duration::from_millis(1),
                |_| {}
            ),
            ProofOutcome::Unregistered
        );
        let (dropped, calls) = fake_slot(false);
        dropped.lock().unwrap().stopped = true;
        assert_eq!(
            prove_native_stream(
                &StreamProof::default(),
                &dropped,
                dir.path(),
                Duration::from_millis(1),
                |_| {}
            ),
            ProofOutcome::Stopped
        );
        assert!(calls.lock().unwrap().is_empty());
        // The production slot refuses a stopped watcher the same way.
        let mut state = NativeWatcherState {
            stopped: true,
            ..Default::default()
        };
        assert!(state.live_watcher().is_none());
    }

    fn create(store: &FsStore, title: &str) -> hotsheet_model::Ticket {
        ops::create(
            store,
            Ulid::new(),
            "HS",
            Timestamp::new("2026-11-27T00:00:00Z"),
            NewTicket {
                title: title.into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn native_deadline() -> Duration {
        if cfg!(target_os = "macos") {
            Duration::from_secs(120)
        } else {
            Duration::from_secs(30)
        }
    }

    fn wait_for_native(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + native_deadline();
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Walks external writes landing before, during and after the native registration and
    /// after its proof, against a stream that reports at once, one that misses its first
    /// probe, and one that misses two and is registered again (HS2-190GDZ).
    #[test]
    fn hs2_190gdz_writes_around_an_unproven_registration_are_indexed() {
        for dropped_probes in [0, 1, 2] {
            let parent = tempfile::tempdir().unwrap();
            let root = parent.path().join("store");
            let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
            let state = crate::AppState::new(store, "secret".into()).unwrap();
            let target = super::default_watch_target(&state);
            let entry = target.entry.clone();
            let scope = Arc::new(TicketsWatchScope::for_store(&root));
            let written = Arc::new(Mutex::new(Vec::new()));
            let hook = |title: &'static str| -> Option<super::StartupHook> {
                let root = root.clone();
                let written = written.clone();
                Some(Box::new(move || {
                    let store = FsStore::open(&root).unwrap();
                    written.lock().unwrap().push(create(&store, title));
                }))
            };
            let handle = super::spawn_watcher_in_scope_with_startup_hooks(
                target,
                WatcherBackend::Recommended,
                scope,
                super::StartupHooks {
                    before_children: hook("before registration"),
                    after_watch_before_identity: hook("during registration"),
                    after_children: hook("before the proof"),
                    dropped_probes,
                },
            )
            .unwrap();
            let indexed = |ticket: &hotsheet_model::Ticket| {
                entry
                    .index
                    .lock()
                    .unwrap()
                    .slug_candidates(&ticket.slug)
                    .unwrap()
                    == vec![ticket.id]
            };
            wait_for_native(
                &format!("the proven stream to go live (dropped_probes={dropped_probes})"),
                // A proven stream retires the bridge that covered its unanswered probes.
                || {
                    entry.freshness.in_sync()
                        && written.lock().unwrap().len() == 3
                        && !handle.native_bridge_held()
                },
            );
            for ticket in written.lock().unwrap().iter() {
                assert!(indexed(ticket), "{} was lost", ticket.title);
            }
            let after = create(&FsStore::open(&root).unwrap(), "after the proof");
            wait_for_native(
                &format!(
                    "the proven stream to report a later write (dropped_probes={dropped_probes})"
                ),
                || indexed(&after),
            );
            drop(handle);
        }
    }

    /// A registration that never reports a probe (an `fseventsd` backlog) is covered by
    /// the polling bridge: the index goes live through it after a full reconcile, indexes
    /// later writes, and keeps the bridge until a probe arrives (HS2-190GDZ).
    #[test]
    fn hs2_190gdz_an_unproven_stream_goes_live_through_the_bridge() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("store");
        let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
        let state = crate::AppState::new(store, "secret".into()).unwrap();
        let target = super::default_watch_target(&state);
        let entry = target.entry.clone();
        let scope = Arc::new(TicketsWatchScope::for_store(&root));
        let written = Arc::new(Mutex::new(None));
        let hook_root = root.clone();
        let hook_written = written.clone();
        let handle = super::spawn_watcher_in_scope_with_startup_hooks(
            target,
            WatcherBackend::Recommended,
            scope,
            super::StartupHooks {
                after_children: Some(Box::new(move || {
                    let store = FsStore::open(&hook_root).unwrap();
                    *hook_written.lock().unwrap() = Some(create(&store, "before going live"));
                })),
                dropped_probes: usize::MAX,
                ..Default::default()
            },
        )
        .unwrap();
        let indexed = |ticket: &hotsheet_model::Ticket| {
            entry
                .index
                .lock()
                .unwrap()
                .slug_candidates(&ticket.slug)
                .unwrap()
                == vec![ticket.id]
        };
        wait_for_native("the bridged index to go live", || {
            entry.freshness.in_sync() && written.lock().unwrap().as_ref().is_some_and(indexed)
        });
        assert!(
            handle.native_bridge_held(),
            "an unproven stream keeps its bridge"
        );
        let later = create(&FsStore::open(&root).unwrap(), "while unproven");
        wait_for_native("the bridge to index a later write", || indexed(&later));
        assert!(handle.native_bridge_held());

        // The bridge follows the path: a moved store must not read as deleted tickets. The
        // index stops being trusted while the store is away and reconciles on its return.
        let mut changes = state.events.subscribe();
        std::fs::rename(&root, parent.path().join("away")).unwrap();
        wait_for_native("the moved store to be marked out of sync", || {
            !entry.freshness.in_sync()
        });
        std::fs::rename(parent.path().join("away"), &root).unwrap();
        let first = written.lock().unwrap().clone().unwrap();
        wait_for_native("the returned store to reconcile through the bridge", || {
            entry.freshness.in_sync() && indexed(&first) && indexed(&later)
        });
        let mut deleted = Vec::new();
        while let Ok(event) = changes.try_recv() {
            if event.kind == "deleted" {
                deleted.push(event.id);
            }
        }
        assert!(
            deleted.is_empty(),
            "a moved store reported deletions: {deleted:?}"
        );
        drop(handle);
    }
}

#[cfg(test)]
mod access_event_tests {
    use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, ModifyKind};
    use notify::{Event, EventKind};

    /// HS2-P0EHD4: opens and read-closes are not changes; creates, writes and errors still are.
    #[test]
    fn hs2_p0ehd4_access_events_are_not_changes() {
        let access = |kind| Ok(Event::new(EventKind::Access(kind)));
        assert!(super::is_access_event(&access(AccessKind::Open(
            AccessMode::Any
        ))));
        assert!(super::is_access_event(&access(AccessKind::Close(
            AccessMode::Read
        ))));
        assert!(super::is_access_event(&access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(!super::is_access_event(&Ok(Event::new(EventKind::Create(
            CreateKind::File
        )))));
        assert!(!super::is_access_event(&Ok(Event::new(EventKind::Modify(
            ModifyKind::Data(DataChange::Any)
        )))));
        assert!(!super::is_access_event(&Err(notify::Error::generic(
            "lost"
        ))));
    }
}

/// HS2-VN96J5: the store-root scope Windows uses, run with the platform's native watcher.
/// On Windows these prove the store can move while watched; on macOS and Linux they pin
/// the scope's filtering and the move, restore and re-arm sequence on the native backends.
#[cfg(test)]
mod store_root_scope_tests {
    use super::{TicketsWatchScope, WatcherBackend, spawn_watcher_in_scope};
    use crate::events::ChangeEvent;
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::broadcast;

    fn create(store: &FsStore, title: &str) -> hotsheet_model::Ticket {
        ops::create(
            store,
            Ulid::new(),
            "HS",
            Timestamp::new("2026-11-21T00:00:00Z"),
            NewTicket {
                title: title.into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    /// A native stream can take seconds to start on a loaded machine.
    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // HS2-R6FZ54: a macOS FSEvents root registration took over 40 seconds in an
    // otherwise isolated test. Give native startup and rearm their own deadline;
    // ordinary event assertions keep the shorter wait above.
    fn wait_for_native_rearm(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now()
            + if cfg!(target_os = "macos") {
                Duration::from_secs(120)
            } else {
                Duration::from_secs(30)
            };
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Every change event received so far.
    fn drain(rx: &mut broadcast::Receiver<ChangeEvent>) -> Vec<(String, String)> {
        let mut seen = Vec::new();
        while let Ok(event) = rx.try_recv() {
            seen.push((event.kind, event.id));
        }
        seen
    }

    fn wait_for_change(rx: &mut broadcast::Receiver<ChangeEvent>, id: Ulid) {
        let id = id.to_string();
        wait_for("the ticket's change event", || {
            drain(rx)
                .iter()
                .any(|(kind, seen)| kind == "changed" && *seen == id)
        });
    }

    struct Watched {
        state: crate::AppState,
        scope: Arc<TicketsWatchScope>,
        handle: super::WatchHandle,
        events: broadcast::Receiver<ChangeEvent>,
    }

    fn watch_store(root: &Path, watch_root: bool) -> Watched {
        let store = FsStore::open(root).unwrap();
        let state = crate::AppState::new(store, "secret".into()).unwrap();
        let events = state.events.subscribe();
        let scope = Arc::new(TicketsWatchScope::new(root, watch_root));
        let handle = spawn_watcher_in_scope(
            super::default_watch_target(&state),
            WatcherBackend::Recommended,
            scope.clone(),
        )
        .unwrap();
        let entry = state.default_entry();
        wait_for_native_rearm("the watcher to go live", || entry.freshness.in_sync());
        Watched {
            state,
            scope,
            handle,
            events,
        }
    }

    fn watch_root(root: &Path) -> Watched {
        watch_store(root, true)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn hs2_c3req1_linux_inotify_drops_moved_store_events_and_rescans_on_return() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let away = dir.path().join("away");
        let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
        let original = create(&store, "present before the move");
        let mut watched = watch_store(&root, false);
        assert_eq!(watched.scope.watch_dir(), root.join("tickets"));
        let entry = watched.state.default_entry();

        std::fs::rename(&root, &away).unwrap();
        let moved = FsStore::open(&away).unwrap();
        let while_away = create(&moved, "written while away");
        wait_for("the old-path event to be dropped", || {
            watched.scope.missed()
        });
        assert_eq!(
            entry
                .index
                .lock()
                .unwrap()
                .slug_candidates(&original.slug)
                .unwrap(),
            vec![original.id],
            "moving the store must not delete an indexed ticket"
        );
        assert!(
            drain(&mut watched.events)
                .iter()
                .all(|(kind, _)| kind != "deleted"),
            "old-path events must not announce deletion"
        );

        std::fs::rename(&away, &root).unwrap();
        let after = create(&store, "written after return");
        let after_id = after.id.to_string();
        let mut saw_invalidation = false;
        wait_for("the returned store to reconcile and notify clients", || {
            // HS2-R6FZ54: a whole-store rearm may emit one aggregate invalidation
            // instead of the ticket-specific event from an uninterrupted watch.
            saw_invalidation |= drain(&mut watched.events)
                .iter()
                .any(|(kind, id)| kind == "changed" && (id.is_empty() || id == &after_id));
            let index = entry.index.lock().unwrap();
            index.slug_candidates(&after.slug).unwrap() == vec![after.id]
                && index.slug_candidates(&while_away.slug).unwrap() == vec![while_away.id]
                && saw_invalidation
        });
        drop(watched.handle);
        drop(watched.state);
    }

    /// An idle store and Git churn beside `tickets/` deliver nothing to the watch loop;
    /// a ticket write is still seen.
    #[test]
    fn hs2_vn96j5_root_scope_is_quiet_when_idle_and_ignores_git_churn() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let store = FsStore::init_git(&root, &StoreMetadata::new("HS")).unwrap();
        let mut watched = watch_root(&root);
        // The native stream may still report the store's own creation; let it settle.
        std::thread::sleep(Duration::from_millis(500));
        drain(&mut watched.events);
        let before = watched.scope.delivered();

        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(watched.scope.delivered(), before, "an idle store is silent");

        // Synthetic churn inside the Git directory, which Git itself never reads.
        let objects = root.join(".git").join("hs2-vn96j5-churn");
        std::fs::create_dir_all(&objects).unwrap();
        for index in 0..200 {
            std::fs::write(objects.join(format!("object-{index}")), b"synthetic").unwrap();
        }
        std::fs::write(root.join("notes.txt"), b"synthetic").unwrap();
        std::thread::sleep(Duration::from_secs(1));
        assert_eq!(
            watched.scope.delivered(),
            before,
            "Git churn and store metadata never reach the watch loop"
        );
        assert!(drain(&mut watched.events).is_empty());

        let ticket = create(&store, "written by another process");
        wait_for_change(&mut watched.events, ticket.id);
        assert!(watched.scope.delivered() > before);
        drop(watched.handle);
        drop(watched.state);
    }

    /// The watched store moves away and back twice. The move succeeds while watched (the
    /// Windows failure this fixes), no ticket is reported deleted while it is away, and
    /// writes after it returns are indexed. Where the backend follows the moved directory
    /// (Windows, Linux), a write made while it was away is picked up by the rescan.
    #[test]
    fn hs2_vn96j5_a_watched_store_moves_away_and_back() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let away = dir.path().join("away");
        let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
        let mut watched = watch_root(&root);
        let entry = watched.state.default_entry();
        for round in 0..2 {
            std::fs::rename(&root, &away).expect("a watched store can be moved");
            let moved = FsStore::open(&away).unwrap();
            let while_away = create(&moved, &format!("written while away {round}"));
            std::thread::sleep(Duration::from_millis(800));
            let seen = drain(&mut watched.events);
            assert!(
                seen.iter().all(|(kind, _)| kind != "deleted"),
                "no ticket is reported deleted while the store is away: {seen:?}"
            );
            std::fs::rename(&away, &root).expect("the store moves back");
            let after = create(&store, &format!("written after return {round}"));
            let after_id = after.id.to_string();
            let mut seen_after_return = Vec::new();
            wait_for_native_rearm("the returned store to reconcile and notify clients", || {
                seen_after_return.extend(drain(&mut watched.events));
                let indexed = entry
                    .index
                    .lock()
                    .unwrap()
                    .slug_candidates(&after.slug)
                    .unwrap()
                    == vec![after.id];
                indexed
                    && seen_after_return
                        .iter()
                        .any(|(kind, id)| kind == "changed" && (id.is_empty() || *id == after_id))
            });
            if !cfg!(target_os = "macos") {
                wait_for("the rescan to index the write made while away", || {
                    entry
                        .index
                        .lock()
                        .unwrap()
                        .slug_candidates(&while_away.slug)
                        .unwrap()
                        == vec![while_away.id]
                });
            }
        }
        drop(watched.handle);
    }

    /// Re-arming after a relink: the old watcher is dropped and the moved store is watched
    /// again at its new path, which then sees writes; the old one reports nothing.
    #[test]
    fn hs2_vn96j5_a_moved_store_is_watched_again_at_its_new_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let moved_root = dir.path().join("moved");
        FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
        let mut old = watch_root(&root);
        std::fs::rename(&root, &moved_root).expect("a watched store can be moved");
        drop(old.handle);
        let mut new = watch_root(&moved_root);
        let ticket = create(&FsStore::open(&moved_root).unwrap(), "after the relink");
        wait_for_change(&mut new.events, ticket.id);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            drain(&mut old.events).is_empty(),
            "the dropped watcher reports nothing"
        );
        drop(new.handle);
    }
}
