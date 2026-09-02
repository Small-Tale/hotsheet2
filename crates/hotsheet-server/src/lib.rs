//! Hot Sheet 2 server: a thin HTTP + WebSocket layer over the shared engine
//! (`hotsheet-ticketing::ops`), the single authority every GUI talks to
//! (`docs/04-core-server-cli.md` §4.3). v1 is loopback + shared-secret (Tier 0).
//! Reads go through the SQLite/FTS index (HS2-5); a filesystem watcher (HS2-6) keeps
//! it fresh and broadcasts change events, so a CLI/git edit shows up live. Terminals
//! (HS2-10) and the detached lifecycle (HS2-59) are separate.

pub mod commands;
pub mod dist_work_loop;
pub mod lifecycle;
pub mod multistore;
pub mod notifications;
pub mod source_revision;
pub mod sync_loop;
pub mod terminal_broker;
pub mod tls;
pub mod tts;

use std::path::Path as FsPath;
use std::sync::{Arc, Mutex};

use multistore::{StoreEntry, StoreHost, StoreInfo};

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use hotsheet_index::{Index, IndexError, TicketRow, hash_bytes};
use hotsheet_model::{
    CloseReason, NoteKind, ReviewKind, ReviewRequest, Ticket, Timestamp, Ulid, parse_file,
    to_file_string,
};
use hotsheet_ticketing::wire::ApiAttachment;
use hotsheet_ticketing::{
    FsStore, GitProvider, KeyRegistry, MutationContext, NewTicket, NotWorkingReport, OpError,
    OsKeychain, ProviderConfigRegistry, ProviderConnection, ProviderDraft, ProviderEvidence,
    ProviderPatch, ProviderRegistry, Settings, SortKey, StoreError, StoreRegistry, TicketPatch,
    TicketQuery, TicketRef, auto_context, copy_between, move_between, ops,
};
// Wire DTOs are defined once in the engine crate (wire SSOT); re-export for callers.
pub use hotsheet_ticketing::{ApiNote, ApiTicket};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use time::OffsetDateTime;
use tokio::sync::broadcast;

/// Attachment uploads may contain screenshots, recordings, and other binary evidence.
/// Keep this route-specific so ordinary JSON endpoints retain Axum's conservative default.
pub const MAX_ATTACHMENT_BODY_BYTES: usize = 100 * 1024 * 1024;

/// Shared server state (cheaply cloned into each handler).
#[derive(Clone)]
pub struct AppState {
    store: FsStore,
    secret: String,
    events: broadcast::Sender<ChangeEvent>,
    index: Arc<Mutex<Index>>,
    /// A bounded, sequenced log of recent [`ChangeEvent`]s backing the **long-poll**
    /// fallback (`GET /ws/poll`) for clients that can't hold a WebSocket (HS2-P3P3CC). The
    /// live push over `/ws/sync` is the primary transport; this replays "everything since
    /// cursor N" over plain HTTP.
    event_log: Arc<Mutex<EventLog>>,
    /// Every store this machine server hosts (HS2-87). The primary `store` is registered
    /// here as the default entry; additional stores are added via `POST /stores`.
    host: StoreHost,
    injected_providers: ProviderRegistry,
    /// Keeps the fs-watchers of `POST /stores`-registered stores alive (the default
    /// store's watcher is held by the server binary). Never read — just not dropped.
    watchers: Arc<Mutex<Vec<WatchHandle>>>,
    /// Whether a `POST /stores`-registered store gets a **file-backed** index
    /// (`${HOTSHEET_HOME}/index/<id>.sqlite`, persists + restores) or an in-memory one.
    /// Off by default so tests stay hermetic (they never touch the machine home); the
    /// server binary turns it on for a real run.
    persist_indexes: bool,
    /// The machine server's coordinates, set after bind in a real run (HS2-87, topology A):
    /// when present, every hosted store gets a per-store discovery instance file pointing
    /// here, so `lifecycle::find_instance(storeX)` resolves to this one machine server for
    /// each project it hosts. `None` in tests (they never write under the machine home).
    instance: Arc<Mutex<Option<InstanceMeta>>>,
    /// Keeps the per-store instance-file guards alive; they remove their files on shutdown.
    instance_guards: Arc<Mutex<Vec<lifecycle::InstanceGuard>>>,
    /// Per-hosted-store index-writer locks (HS2-AYCA1W). The primary `store`'s lock is held
    /// by the server binary (main.rs); this holds one for every *additional* hosted store, so
    /// no second machine server double-writes a registered store's index. Real-run only (a
    /// held lock touches the machine home) — acquired in `register_store_instance`.
    writer_locks: Arc<Mutex<Vec<lifecycle::WriterLock>>>,
    /// A "kick" to the background sync loop (HS2-731C2X): a server write signals it so local
    /// changes push promptly rather than waiting for the next interval. `None` until the
    /// loop is spawned (tests don't run it).
    sync_kick: Arc<Mutex<Option<std::sync::mpsc::Sender<()>>>>,
    /// The live permission bridge (HS2-9R9YZW): a driven tool blocks on it; a client answers
    /// over `GET/POST /permissions`. A `permission_asked` nudge rides the event bus so
    /// clients know to fetch + answer. Empty (headless auto-nothing) until seeded.
    permissions: Arc<hotsheet_aitools::SharedPermissionBridge>,
    /// Where `Always` allow-rules persist (a store-local JSON file), so remembered answers
    /// survive a restart. `None` in tests (they don't touch disk for rules).
    permission_rules_path: Option<std::path::PathBuf>,
    /// The **shared** connection registry for the driving loop (HS2-TCV3BF): each ticket the
    /// server is driving registers here for its turn, so `GET /connections` shows what's
    /// live and (once driving is concurrent) the in-flight bound can consult busy state.
    drive_registry: Arc<Mutex<hotsheet_aitools::ConnectionRegistry>>,
    /// The in-process PTY manager (HS2-10) exposed over HTTP (HS2-A6R5QV): open/list/input/
    /// read/kill terminals. Lazy — a terminal spawns on first `POST /terminals`. Used unless
    /// the detached broker is enabled.
    terminals: Arc<hotsheet_terminals::TerminalManager>,
    /// When set (opt-in), terminals live in a **detached broker** process so they survive a
    /// server restart (HS2-ERT00F); the `/terminals` request/response ops route through it.
    terminal_broker: Option<terminal_broker::TerminalBroker>,
    /// Search roots for third-party plugins; embedded first-party plugins are always present.
    plugin_dirs: Arc<Vec<std::path::PathBuf>>,
    /// Public URL injected into manifest-launched terminal tools for permission route-back.
    terminal_server_url: Arc<Mutex<Option<String>>>,
    /// Machine-local checkout discovery. Checkout ids identify working directories and
    /// are intentionally separate from store ids and server authentication tokens.
    checkout_registry: hotsheet_ticketing::checkouts::CheckoutRegistry,
    commands: commands::CommandManager,
    notifications: notifications::NotificationHub,
    tts: tts::TtsProviders,
    source_revision: source_revision::SourceRevisionMonitor,
}

/// The machine server's coordinates, shared by every hosted store's discovery instance file.
#[derive(Clone)]
struct InstanceMeta {
    url: String,
    secret: String,
    started_at: String,
}

impl AppState {
    /// State over a store + a prepared index, guarded by `secret`. The caller decides
    /// whether the index is in-memory or file-backed (`Index::open_reconciled`).
    pub fn with_index(store: FsStore, secret: String, index: Index) -> Self {
        let store = store.with_deferred_push();
        let (events, _) = broadcast::channel(256);
        let index = Arc::new(Mutex::new(index));
        let host = StoreHost::new();
        // The primary store is the default hosted entry (shares the same index Arc, so
        // the unprefixed routes and /stores/{default}/… see one index).
        host.register(StoreEntry {
            store: store.clone(),
            index: index.clone(),
        });
        let event_log = Arc::new(Mutex::new(EventLog::default()));
        // A permission request enqueued by a driven tool pushes a `permission_asked` nudge
        // over the event bus (WS + long-poll ring), so clients fetch + answer it.
        let permissions = Arc::new(hotsheet_aitools::SharedPermissionBridge::default());
        {
            let events = events.clone();
            let log = event_log.clone();
            permissions.set_on_pending(move |req| {
                let ev = ChangeEvent {
                    store: String::new(),
                    kind: "permission_asked".to_string(),
                    id: req.id.to_string(),
                    slug: req.tool.clone(),
                    message: None,
                    activity: None,
                    assignment: None,
                };
                if let Ok(mut l) = log.lock() {
                    l.push(ev.clone());
                }
                let _ = events.send(ev);
            });
        }
        let command_defs =
            hotsheet_ticketing::commands::from_settings(&Settings::new(store.root()))
                .unwrap_or_default();
        let command_event_log = event_log.clone();
        let command_events = events.clone();
        let commands = commands::CommandManager::new(store.root().to_path_buf(), command_defs)
            .with_on_change(Arc::new(move |run| {
                emit_change(
                    &command_event_log,
                    &command_events,
                    ChangeEvent {
                        store: String::new(),
                        kind: "command_updated".into(),
                        id: run.id,
                        slug: run.command_id,
                        message: Some(run.state),
                        activity: None,
                        assignment: None,
                    },
                );
            }));
        Self {
            store,
            secret,
            events,
            index,
            event_log,
            host,
            injected_providers: ProviderRegistry::default(),
            watchers: Arc::new(Mutex::new(Vec::new())),
            persist_indexes: false,
            instance: Arc::new(Mutex::new(None)),
            instance_guards: Arc::new(Mutex::new(Vec::new())),
            writer_locks: Arc::new(Mutex::new(Vec::new())),
            sync_kick: Arc::new(Mutex::new(None)),
            permissions,
            permission_rules_path: None,
            // A generous busy window: a driven turn heartbeats via the local registry, but
            // the shared one tracks "currently driving" by registration, not the window.
            drive_registry: Arc::new(Mutex::new(hotsheet_aitools::ConnectionRegistry::new(
                60_000,
            ))),
            terminals: Arc::new(hotsheet_terminals::TerminalManager::new()),
            terminal_broker: None,
            plugin_dirs: Arc::new(hotsheet_plugins::default_dirs()),
            terminal_server_url: Arc::new(Mutex::new(None)),
            checkout_registry: hotsheet_ticketing::checkouts::CheckoutRegistry::new(
                hotsheet_plugins::hotsheet_home().join("checkouts.json"),
            ),
            commands,
            notifications: Default::default(),
            tts: Default::default(),
            source_revision: source_revision::SourceRevisionMonitor::current_build(),
        }
    }

    /// Override checkout-registry storage (primarily for hermetic tests).
    pub fn with_checkout_registry(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.checkout_registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(path);
        self
    }

    pub fn with_commands(
        mut self,
        definitions: Vec<hotsheet_ticketing::commands::CommandDefinition>,
    ) -> Self {
        self.commands = commands::CommandManager::new(self.store.root().to_path_buf(), definitions);
        self
    }

    pub fn with_tts_providers(mut self, providers: Vec<Arc<dyn tts::TtsProvider>>) -> Self {
        self.tts = tts::TtsProviders::new(providers);
        self
    }

    /// Override local-build source monitoring (primarily for embedders and tests).
    pub fn with_source_revision_monitor(
        mut self,
        monitor: source_revision::SourceRevisionMonitor,
    ) -> Self {
        self.source_revision = monitor;
        self
    }

    /// Register an injected provider (deterministic tests and embedders). Production
    /// connections normally load from `providers.json` and the key registry.
    pub fn with_ticket_provider(
        self,
        provider: Arc<dyn hotsheet_ticketing::TicketProvider>,
    ) -> Self {
        self.injected_providers
            .register(provider)
            .expect("fresh injected provider registry");
        self
    }

    /// The shared driving-loop connection registry — the loop registers each ticket it
    /// drives, and `GET /connections` reads it (HS2-TCV3BF).
    pub fn drive_registry(&self) -> Arc<Mutex<hotsheet_aitools::ConnectionRegistry>> {
        self.drive_registry.clone()
    }

    /// Subscribe to the live change/announce bus (what `/ws/sync` pushes).
    pub fn subscribe(&self) -> broadcast::Receiver<ChangeEvent> {
        self.events.subscribe()
    }

    /// Seed the permission bridge with the durable `Always` allow-rules stored at `path`,
    /// and persist future `Always` answers there (call this in a real run; leave off in
    /// tests so they never touch disk). Builder-style.
    pub fn with_permission_rules(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        let rules = hotsheet_aitools::load_permission_rules(&path);
        // Re-seed the bridge with the loaded rules (keeping the on_pending observer).
        self.permissions.reseed_rules(rules);
        self.permission_rules_path = Some(path);
        self
    }

    /// The shared permission bridge — the drive/tool side blocks on it via
    /// `request_blocking`; the server answers it over `POST /permissions/{id}`.
    pub fn permission_bridge(&self) -> Arc<hotsheet_aitools::SharedPermissionBridge> {
        self.permissions.clone()
    }

    /// The `(url-id, root-path)` of every hosted store — the set the background sync loop
    /// iterates.
    pub fn hosted_store_roots(&self) -> Vec<(String, String)> {
        self.host
            .list()
            .into_iter()
            .map(|s| (s.id, s.root))
            .collect()
    }

    /// Register the background sync loop's kick channel (called by [`sync_loop::spawn_sync_loop`]).
    pub fn set_sync_kicker(&self, tx: std::sync::mpsc::Sender<()>) {
        if let Ok(mut k) = self.sync_kick.lock() {
            *k = Some(tx);
        }
    }

    /// Nudge the sync loop to run now (best-effort; a no-op if the loop isn't running).
    fn kick_sync(&self) {
        if let Ok(k) = self.sync_kick.lock() {
            if let Some(tx) = k.as_ref() {
                let _ = tx.send(());
            }
        }
    }

    /// Persist the indexes of `POST /stores`-registered stores to
    /// `${HOTSHEET_HOME}/index/` (call this in a real server run; leave off in tests so
    /// they never write under the machine home). Builder-style.
    pub fn with_persistent_registered_indexes(mut self) -> Self {
        self.persist_indexes = true;
        self
    }

    /// Enable the **detached terminal broker** (HS2-ERT00F): terminals live in a separate
    /// `hotsheet-terminal-broker` process (spawned/discovered for the primary store), so they
    /// survive a server restart. Opt-in — without it, terminals are in-process.
    pub fn with_terminal_broker(mut self) -> anyhow::Result<Self> {
        self.terminal_broker = Some(terminal_broker::TerminalBroker::ensure(self.store.root())?);
        Ok(self)
    }

    /// Point terminal ops at an explicit already-running broker (tests).
    pub fn with_terminal_broker_at(mut self, broker: terminal_broker::TerminalBroker) -> Self {
        self.terminal_broker = Some(broker);
        self
    }

    /// Override plugin search roots (for hermetic hosts and integration tests).
    pub fn with_plugin_dirs(mut self, dirs: Vec<std::path::PathBuf>) -> Self {
        self.plugin_dirs = Arc::new(dirs);
        self
    }

    /// Set the URL injected into interactively launched tools. The real server calls this
    /// after binding; tests may use it without publishing machine discovery files.
    pub fn set_terminal_server_url(&self, url: String) {
        if let Ok(mut slot) = self.terminal_server_url.lock() {
            *slot = Some(url);
        }
    }

    /// Host a store: build its index (file-backed when persisting, else in-memory),
    /// register it, and spawn its fs-watcher. Idempotent by store id — returns whether it
    /// was newly added. Shared by `POST /stores` and startup discovery.
    fn host_store(&self, store: FsStore) -> Result<bool, ApiError> {
        let store = store.with_deferred_push();
        let id = multistore::store_url_id(&store);
        if self.host.contains(&id) {
            return Ok(false);
        }
        let index = if self.persist_indexes {
            let path = multistore::index_path_for(&store)
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            Index::open_reconciled(&path, &store)?
        } else {
            let ix = Index::open_in_memory(store.root().display().to_string())?;
            ix.rebuild_from_store(&store)?;
            ix
        };
        let entry = StoreEntry {
            store: store.clone(),
            index: Arc::new(Mutex::new(index)),
        };
        self.host.register(entry.clone());
        let store_root = store.root().to_path_buf();
        match spawn_watcher_for(
            WatchTarget {
                entry,
                store_id: id,
                events: self.events.clone(),
                event_log: self.event_log.clone(),
                checkout_registry: self.checkout_registry.clone(),
            },
            registered_watcher_backend(),
        ) {
            Ok(handle) => {
                if let Ok(mut w) = self.watchers.lock() {
                    w.push(handle);
                }
            }
            Err(e) => eprintln!("watcher for {} failed to start: {e}", store_root.display()),
        }
        // Advertise the newly-hosted store for discovery (real run only; a no-op in tests).
        self.register_store_instance(&store_root);
        Ok(true)
    }

    /// Record the machine server's coordinates (URL + start time; a real run, after bind)
    /// and register a discovery instance file for **every** already-hosted store, so a
    /// client asking "who serves project X?" finds this one machine server for each project
    /// it hosts (HS2-87 topology A). Runtime `POST /stores` additions register via
    /// [`Self::host_store`]. No-op'd in tests (they never call this).
    pub fn publish_instances(&self, url: String, started_at: String) {
        if let Ok(mut m) = self.instance.lock() {
            *m = Some(InstanceMeta {
                url,
                secret: self.secret.clone(),
                started_at,
            });
        }
        for info in self.host.list() {
            self.register_store_instance(FsPath::new(&info.root));
        }
    }

    /// Write the discovery instance file for one hosted store (if instance publishing is
    /// on), retaining its guard so the file is removed on shutdown.
    fn register_store_instance(&self, store_path: &FsPath) {
        let Some(meta) = self.instance.lock().ok().and_then(|m| m.clone()) else {
            return; // not a published (real) run — nothing to register
        };
        let index_path = if self.persist_indexes {
            FsStore::open(store_path)
                .ok()
                .and_then(|s| multistore::index_path_for(&s).ok())
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        } else {
            "(in-memory)".into()
        };
        let info = lifecycle::InstanceInfo {
            pid: std::process::id(),
            url: meta.url,
            secret: meta.secret,
            store_path: store_path.display().to_string(),
            index_path,
            started_at: meta.started_at,
        };
        // Hold this store's index-writer lock too, so a second machine server can't
        // double-write its index (HS2-AYCA1W). The primary store's lock is already held by
        // the server binary (main.rs), so skip it here. A lock held by another *live* server
        // is logged, not fatal — the discovery instance file (above) already steers clients
        // to a single server; this is belt-and-suspenders against a stray duplicate.
        let is_primary = same_path(store_path, self.store.root());
        if !is_primary {
            match lifecycle::acquire_writer_lock(store_path) {
                Ok(lock) => {
                    if let Ok(mut w) = self.writer_locks.lock() {
                        w.push(lock);
                    }
                }
                Err(lifecycle::LockError::Held(pid)) => eprintln!(
                    "warning: store {} is also index-write-locked by live server pid {pid} \
                     — index writes may collide",
                    store_path.display()
                ),
                Err(e) => eprintln!("writer lock for {} failed: {e}", store_path.display()),
            }
        }
        match lifecycle::register_instance(&info, store_path) {
            Ok(guard) => {
                if let Ok(mut g) = self.instance_guards.lock() {
                    g.push(guard);
                }
            }
            Err(e) => eprintln!(
                "instance registration failed for {}: {e}",
                store_path.display()
            ),
        }
    }

    /// Auto-host the stores listed in `${HOTSHEET_HOME}/stores.json` (HS2-87 startup
    /// discovery). A path that isn't a store is logged and skipped — one bad entry never
    /// stops the server. Returns how many were newly hosted.
    pub fn host_configured_stores(&self) -> usize {
        let mut hosted = 0;
        for path in multistore::configured_store_paths() {
            match FsStore::open(&path) {
                Ok(store) => match self.host_store(store) {
                    Ok(true) => hosted += 1,
                    Ok(false) => {}
                    Err(e) => eprintln!("could not host {}: {}", path.display(), e.message),
                },
                Err(e) => eprintln!("configured store {} skipped: {e}", path.display()),
            }
        }
        hosted
    }

    /// State over a store with a fresh **in-memory** index rebuilt from it (tests, or
    /// a run that doesn't want to persist the cache).
    pub fn new(store: FsStore, secret: String) -> anyhow::Result<Self> {
        let index = Index::open_in_memory(store.root().display().to_string())?;
        index.rebuild_from_store(&store)?;
        Ok(Self::with_index(store, secret, index))
    }

    fn emit(&self, event: ChangeEvent) {
        emit_change(&self.event_log, &self.events, event);
    }

    /// The current long-poll cursor (the last emitted event's seq; 0 if none).
    fn event_cursor(&self) -> u64 {
        self.event_log.lock().map(|l| l.seq).unwrap_or(0)
    }

    /// The default (primary) served store as a host entry — what the unprefixed routes
    /// operate on.
    fn default_entry(&self) -> StoreEntry {
        StoreEntry {
            store: self.store.clone(),
            index: self.index.clone(),
        }
    }

    /// Reindex a ticket the server just wrote into `entry`'s index, then broadcast a
    /// change tagged with the store it happened in. The index now carries the file's
    /// hash, so the watcher sees "no change" and won't re-emit.
    fn changed_in(&self, entry: &StoreEntry, kind: &str, t: &Ticket) {
        let text = to_file_string(t);
        let path = entry.store.ticket_path(&t.id).display().to_string();
        if let Ok(index) = entry.index.lock() {
            let _ = index.upsert(t, &path, &hash_bytes(text.as_bytes()));
        }
        self.emit(ChangeEvent {
            store: multistore::store_url_id(&entry.store),
            kind: kind.to_string(),
            id: t.id.to_string(),
            slug: t.slug.clone(),
            message: None,
            activity: None,
            assignment: None,
        });
        // A write is worth pushing promptly — wake the background sync loop (HS2-731C2X).
        self.kick_sync();
    }

    /// Broadcast an **ephemeral announcement** to live `/ws/sync` subscribers (HS2-HHDNTH):
    /// a store-level message that is **not** persisted — it rides the WS bus only, so it is
    /// NOT recorded in the long-poll ring and never replayed. A client not connected when it
    /// fires simply misses it. `store` is the target store's URL id (empty = the default).
    pub fn announce(&self, store: String, message: String) {
        // WS-only: intentionally skip the EventLog ring (ephemeral, unlike `emit`).
        let _ = self.events.send(ChangeEvent {
            store,
            kind: "announce".to_string(),
            id: String::new(),
            slug: String::new(),
            message: Some(message),
            activity: None,
            assignment: None,
        });
    }

    /// Persist one activity event and publish that exact event on the shared live bus.
    /// Unlike announcements this is also placed in the long-poll ring; the rolling
    /// activity store remains the authoritative reconnect/digest source.
    pub fn record_activity(
        &self,
        store: &FsStore,
        event: hotsheet_ticketing::ActivityEvent,
    ) -> std::io::Result<()> {
        hotsheet_ticketing::activity::record(store, &event)?;
        self.emit(ChangeEvent {
            store: multistore::store_url_id(store),
            kind: "activity".to_string(),
            id: event.id.clone(),
            slug: String::new(),
            message: None,
            activity: Some(event),
            assignment: None,
        });
        Ok(())
    }
}

/// A live-change event pushed over `/ws/sync`.
#[derive(Clone, Debug, Serialize)]
pub struct ChangeEvent {
    /// The URL id of the store the change happened in (multi-store, HS2-87).
    pub store: String,
    pub kind: String,
    pub id: String,
    pub slug: String,
    /// For `kind == "announce"` (HS2-HHDNTH): the broadcast message text. `None` for
    /// ticket-change events (omitted on the wire).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// For `kind == "activity"`: the complete event persisted to the rolling timeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<hotsheet_ticketing::ActivityEvent>,
    /// For `kind == "assignment"`: newly assigned/requested recipients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment: Option<AssignmentEvent>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssignmentEvent {
    pub newly_assigned: Vec<String>,
    pub review_requested: Vec<String>,
    pub requested_by: Option<String>,
}

/// How many recent events the long-poll ring retains. A poller whose cursor falls behind
/// this many events gets an `overflow` signal and should re-sync via a full list.
const EVENT_LOG_CAP: usize = 512;

/// A bounded, monotonically-sequenced ring of recent [`ChangeEvent`]s, so a long-poll
/// client can ask "everything since cursor N" without holding a socket (HS2-P3P3CC). Each
/// event gets a `seq`; the ring keeps the newest [`EVENT_LOG_CAP`].
#[derive(Default)]
struct EventLog {
    /// The last sequence number assigned (0 = nothing emitted yet).
    seq: u64,
    /// `(seq, event)`, oldest first, capped at [`EVENT_LOG_CAP`].
    ring: std::collections::VecDeque<(u64, ChangeEvent)>,
}

fn emit_change(
    event_log: &Arc<Mutex<EventLog>>,
    events: &broadcast::Sender<ChangeEvent>,
    event: ChangeEvent,
) {
    // Record first so a long poll racing the broadcast can always replay by cursor.
    if let Ok(mut log) = event_log.lock() {
        log.push(event.clone());
    }
    let _ = events.send(event); // Err just means no live subscribers.
}

impl EventLog {
    /// Record an event, assigning it the next seq.
    fn push(&mut self, event: ChangeEvent) {
        self.seq += 1;
        self.ring.push_back((self.seq, event));
        while self.ring.len() > EVENT_LOG_CAP {
            self.ring.pop_front();
        }
    }

    /// Events with `seq > since`, plus whether `since` fell off the back of the ring
    /// (the caller lost events and should re-sync). `since >= seq` (caught up / future) is
    /// not an overflow — it just yields no events.
    fn since(&self, since: u64) -> (Vec<ChangeEvent>, bool) {
        let oldest = self.ring.front().map(|(s, _)| *s);
        // Overflow only when we've dropped events the caller hadn't seen: they ask for
        // `since` strictly before our oldest retained event, and we have emitted past it.
        let overflow = matches!(oldest, Some(o) if since.saturating_add(1) < o);
        let events = self
            .ring
            .iter()
            .filter(|(s, _)| *s > since)
            .map(|(_, e)| e.clone())
            .collect();
        (events, overflow)
    }
}

#[cfg(test)]
mod event_log_tests {
    use super::{ChangeEvent, EVENT_LOG_CAP, EventLog};

    fn ev(n: usize) -> ChangeEvent {
        ChangeEvent {
            store: "s".into(),
            kind: "created".into(),
            id: format!("id-{n}"),
            slug: format!("HS-{n}"),
            message: None,
            activity: None,
            assignment: None,
        }
    }

    #[test]
    fn since_returns_the_tail_and_advances_the_cursor() {
        let mut log = EventLog::default();
        for i in 0..3 {
            log.push(ev(i));
        }
        assert_eq!(log.seq, 3);
        // Everything since 0 = all three; since 2 = just the last; caught up = none.
        let (all, of) = log.since(0);
        assert_eq!(all.len(), 3);
        assert!(!of);
        assert_eq!(log.since(2).0.len(), 1);
        assert!(log.since(3).0.is_empty(), "caught up → none");
        // A future/equal cursor is not an overflow.
        assert!(!log.since(3).1);
        assert!(!log.since(99).1);
        assert!(
            log.since(u64::MAX).0.is_empty(),
            "the largest possible future cursor is safe and caught up"
        );
        assert!(!log.since(u64::MAX).1);
    }

    #[test]
    fn falling_behind_the_ring_signals_overflow() {
        let mut log = EventLog::default();
        // Emit more than the ring holds, so the oldest retained seq > 1.
        for i in 0..(EVENT_LOG_CAP + 10) {
            log.push(ev(i));
        }
        // A poller stuck at cursor 1 lost events that aged out → overflow.
        let (_, overflow) = log.since(1);
        assert!(
            overflow,
            "cursor before the oldest retained event overflows"
        );
        // A poller within the retained window does not overflow.
        let recent = log.seq - 5;
        assert!(!log.since(recent).1);
        assert_eq!(log.since(recent).0.len(), 5);
    }
}

/// Build the router. Ticket routes require the secret; `/health` and `/ws/sync` don't
/// (the WS checks the secret via a query param, since browsers can't set WS headers).
pub fn app(state: AppState) -> Router {
    let protected = Router::new()
        .route("/tickets", get(list_tickets).post(create_ticket))
        .route("/tickets/{id}", get(get_ticket).patch(update_ticket))
        .route("/tickets/{id}/notes/{note_id}", delete(delete_ticket_note))
        .route(
            "/tickets/{id}/attachments",
            post(add_ticket_attachment).layer(DefaultBodyLimit::max(MAX_ATTACHMENT_BODY_BYTES)),
        )
        .route("/tickets/{id}/close", post(close_ticket))
        .route("/tickets/{id}/assign", post(assign_ticket))
        // Coordination: claim the next available ticket, release, renew a lease (HS2-86).
        .route("/claim-next", post(claim_next_ticket))
        .route("/tickets/{id}/release", post(release_ticket))
        .route("/tickets/{id}/renew", post(renew_ticket))
        // Cross-store copy / move (HS2-60 / HS2-S4H2AM): source is the default store,
        // `?to=<store_id>` names a hosted destination store.
        .route("/tickets/{id}/copy", post(copy_ticket_route))
        .route("/tickets/{id}/move", post(move_ticket_route))
        .route("/batch", post(batch_update))
        .route("/setup/{tool}", post(setup_tool))
        // Multi-store (HS2-87): list/register hosted stores + store-scoped ticket routes
        // (path-prefix scheme, maintainer's pick), sharing the default routes' logic.
        .route("/stores", get(list_stores).post(add_store))
        .route("/providers", get(list_providers))
        .route("/provider-transfers/copy", post(provider_copy_route))
        .route("/provider-transfers/move", post(provider_move_route))
        .route("/checkouts", get(list_checkouts).post(register_checkout))
        .route("/projects/open", post(open_project))
        .route("/checkouts/{reference}", get(resolve_checkout))
        .route(
            "/checkouts/{reference}/repository/status",
            get(checkout_repository_status),
        )
        .route(
            "/checkouts/{reference}/tickets",
            get(list_checkout_tickets).post(create_checkout_ticket),
        )
        .route(
            "/checkouts/{reference}/batch",
            post(batch_update_checkout_tickets),
        )
        .route(
            "/checkouts/{reference}/corrupt-tickets",
            get(list_checkout_corrupt_tickets),
        )
        .route(
            "/checkouts/{reference}/corrupt-tickets/repair",
            post(create_corrupt_ticket_repair),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}",
            get(get_checkout_ticket).patch(update_checkout_ticket),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/close",
            post(close_checkout_ticket),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/assign",
            post(assign_checkout_ticket),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/attachments",
            post(add_checkout_ticket_attachment)
                .layer(DefaultBodyLimit::max(MAX_ATTACHMENT_BODY_BYTES)),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/attachments/{attachment_id}",
            get(get_checkout_ticket_attachment).delete(delete_checkout_ticket_attachment),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/notes/{note_id}",
            delete(delete_checkout_ticket_note),
        )
        .route(
            "/stores/{store_id}/tickets",
            get(list_store_tickets).post(create_store_ticket),
        )
        .route(
            "/stores/{store_id}/tickets/{id}",
            get(get_store_ticket).patch(update_store_ticket),
        )
        .route(
            "/stores/{store_id}/tickets/{id}/close",
            post(close_store_ticket),
        )
        .route(
            "/stores/{store_id}/tickets/{id}/assign",
            post(assign_store_ticket),
        )
        // Provider-neutral aliases. Store routes remain compatibility shorthands for the
        // built-in git provider while clients migrate to `/providers/{connection_id}`.
        .route(
            "/providers/{connection_id}/tickets",
            get(list_provider_tickets).post(create_provider_ticket),
        )
        .route(
            "/providers/{connection_id}/tickets/{id}",
            get(get_provider_ticket).patch(update_provider_ticket),
        )
        .route(
            "/providers/{connection_id}/tickets/{id}/notes/{note_id}",
            delete(delete_provider_ticket_note),
        )
        .route(
            "/providers/{connection_id}/tickets/{id}/close",
            post(close_provider_ticket),
        )
        .route(
            "/providers/{connection_id}/tickets/{id}/assign",
            post(assign_provider_ticket),
        )
        .route(
            "/providers/{connection_id}/tickets/{id}/not-working",
            post(report_provider_ticket_not_working)
                .layer(DefaultBodyLimit::max(MAX_ATTACHMENT_BODY_BYTES)),
        )
        .route(
            "/provider-connections",
            get(list_provider_connections).post(create_provider_connection),
        )
        .route(
            "/provider-connections/{connection_id}",
            patch(update_provider_connection).delete(delete_provider_connection),
        )
        .route("/provider-attachments/copy", post(copy_provider_attachment))
        // Authenticated application/protocol negotiation. `/health` intentionally remains
        // a small unauthenticated liveness probe.
        .route("/compatibility", get(compatibility))
        // Cross-store resolve: a global ULID → its live instance in whichever store hosts
        // it (follows moved tombstones). HS2-87 / HS2-S4H2AM.
        .route("/resolve/{id}", get(resolve_ticket))
        // Permission round-trip (HS2-9R9YZW): list what a driven tool is blocked on, and
        // answer one (allow/deny + once/session/always).
        .route("/permissions", get(list_permissions))
        .route("/permissions/{id}", post(resolve_permission))
        // Raise a blocking permission request (the asking side — e.g. a Claude PreToolUse
        // hook), HS2-YMR9HE. Blocks until answered over the route-back, or times out.
        .route("/permissions/ask", post(ask_permission))
        // What the server is currently driving (HS2-TCV3BF).
        .route("/connections", get(list_connections))
        .route("/analytics/tickets", get(ticket_flow_summary))
        .route("/analytics/usage", get(usage_metrics_summary))
        .route("/commands", get(list_commands).put(save_commands))
        .route("/commands/{id}/run", post(run_command))
        .route("/command-runs", get(list_command_runs))
        .route("/command-runs/{id}", get(get_command_run))
        .route("/command-runs/{id}/cancel", post(cancel_command_run))
        .route(
            "/notifications",
            get(list_notifications).post(publish_notification),
        )
        .route("/notifications/{id}/ack", post(acknowledge_notification))
        .route("/tts/synthesize", post(synthesize_speech))
        // Terminals (HS2-A6R5QV): open a PTY, list them, feed input, read the scrollback,
        // kill one — the HTTP attach surface over the in-process TerminalManager.
        .route("/terminals", get(list_terminals).post(open_terminal))
        .route("/terminals/{id}", get(read_terminal).delete(kill_terminal))
        .route("/terminals/{id}/input", post(write_terminal))
        // Activity timeline (HS2-KP31ZE): ingest a tool's activity event, and read the
        // per-ticket/session "what happened" window (docs/15). The Announcer/timeline consumer.
        .route("/activity", get(list_activity).post(ingest_activity))
        // Ephemeral store-level announcement broadcast over the WS bus (HS2-HHDNTH) — not
        // persisted, live subscribers only.
        .route("/announce", post(post_announce))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_secret,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/ws/sync", get(ws_sync))
        // Long-poll fallback for clients that can't hold a WebSocket (HS2-P3P3CC). Like
        // `/ws/sync`, it authenticates via a `secret` query param (not a header) so the
        // same constrained clients can use it.
        .route("/ws/poll", get(poll_events))
        // Live terminal attach (HS2-XTTTMV): a WebSocket that replays the scrollback then
        // streams new PTY output + forwards the viewer's input. Query-param auth (a WS
        // upgrade can't set headers from a browser), like `/ws/sync`.
        .route("/terminals/{id}/attach", get(attach_terminal))
        .merge(protected)
        .with_state(state)
}

// ---- auth ------------------------------------------------------------------------

async fn require_secret(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let presented = req
        .headers()
        .get("x-hotsheet-secret")
        .and_then(|v| v.to_str().ok());
    if presented == Some(state.secret.as_str()) {
        Ok(next.run(req).await)
    } else {
        Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "missing or invalid secret",
        ))
    }
}

// ---- handlers --------------------------------------------------------------------

async fn health(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    // Resilient enumeration (HS2-PRVPCQ): a single corrupt ticket file must not make the
    // whole store un-openable. Count the healthy tickets and surface any unparseable files
    // separately instead of hard-failing the project.
    let listing = state.store.list_tickets_resilient()?;
    let metadata = state.store.metadata()?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "generation": "hs2",
        "api_version": 1,
        "ticket_prefix": metadata.ticket_prefix,
        "store_schema": metadata.schema_version,
        "tickets": listing.tickets.len(),
        "corrupt": listing.corrupt.iter().map(|c| serde_json::json!({
            "path": c.path.display().to_string(),
            "id": c.id.map(|id| id.to_string()),
            "slug": c.slug,
            "error": c.error,
            "error_code": c.error_code,
        })).collect::<Vec<_>>()
    })))
}

const API_PROTOCOL_MIN: u32 = 1;
const API_PROTOCOL_MAX: u32 = 1;

/// Authenticated build metadata and protocol compatibility. Exact application/revision
/// equality is informational; clients make hard decisions from the inclusive range.
async fn compatibility(State(state): State<AppState>) -> Json<serde_json::Value> {
    let started_at = state
        .instance
        .lock()
        .ok()
        .and_then(|instance| instance.as_ref().map(|value| value.started_at.clone()));
    let source = state.source_revision.status();
    Json(serde_json::json!({
        "generation": "hs2",
        "application_version": env!("CARGO_PKG_VERSION"),
        "build_revision": source.build_revision,
        "source_revision": source.source_revision,
        "source_stale": source.source_stale,
        "protocol": { "min": API_PROTOCOL_MIN, "max": API_PROTOCOL_MAX },
        "store_schema": { "min": 1, "max": hotsheet_ticketing::STORE_SCHEMA_VERSION },
        "capabilities": {
            "lifecycle_restart": false,
            "lifecycle_quiescence": false
        },
        "started_at": started_at
    }))
}

async fn list_tickets(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let compact = params.compact.unwrap_or(true);
    let fields = parse_fields(&params.fields);
    let query = params.into_query(state.store.root())?;
    let mut rows = state
        .index
        .lock()
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned"))?
        .query(&query)?;
    let connection_id = multistore::store_url_id(&state.store);
    for row in &mut rows {
        row.set_connection(&connection_id);
    }
    if compact {
        for row in &mut rows {
            row.make_compact();
        }
    }
    let contexts = auto_context::effective(&Settings::new(state.store.root()))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    for row in &mut rows {
        row.add_auto_context(&contexts);
    }
    Ok(Json(rows_to_json(rows, &fields)))
}

async fn get_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiTicket>, ApiError> {
    let ticket = ops::resolve(&state.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json(api_ticket(&state.default_entry(), &ticket)?))
}

fn api_ticket(entry: &StoreEntry, ticket: &Ticket) -> Result<ApiTicket, ApiError> {
    let contexts = auto_context::effective(&Settings::new(entry.store.root()))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(ApiTicket::with_provider_auto_context(
        ticket,
        &multistore::store_url_id(&entry.store),
        None,
        &contexts,
    ))
}

// ---- activity timeline (HS2-KP31ZE) ----------------------------------------------

/// `POST /activity` body — a tool's activity signal. The server stamps `id`/`ts`; `summary`
/// and `importance` default from the kind unless the caller provides them (docs/15 §15.7).
#[derive(Debug, Deserialize)]
struct ActivityIngest {
    tool: String,
    kind: hotsheet_ticketing::ActivityKind,
    #[serde(default)]
    detail: serde_json::Value,
    #[serde(default)]
    ticket: Option<String>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    importance: Option<hotsheet_ticketing::Importance>,
}

/// `POST /activity` — record one activity event to the store's rolling window.
async fn ingest_activity(
    State(state): State<AppState>,
    Json(body): Json<ActivityIngest>,
) -> Result<Json<hotsheet_ticketing::ActivityEvent>, ApiError> {
    let mut ev = hotsheet_ticketing::ActivityEvent::new(
        Ulid::new().to_string(),
        now().as_str().to_string(),
        body.tool,
        body.kind,
        body.detail,
    );
    ev.ticket = body.ticket;
    ev.session = body.session;
    ev.project = body.project;
    if let Some(s) = body.summary {
        ev.summary = s;
    }
    if let Some(i) = body.importance {
        ev.importance = i;
    }
    state
        .record_activity(&state.store, ev.clone())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ev))
}

/// `GET /activity` query params — the timeline filter (docs/15 §15.6).
#[derive(Debug, Default, Deserialize)]
struct ActivityParams {
    ticket: Option<String>,
    session: Option<String>,
    /// `low` | `normal` | `high` — only events at or above this emphasis.
    min_importance: Option<String>,
    limit: Option<usize>,
}

/// `GET /activity` — the per-ticket/session "what happened" window, most-recent-capped.
async fn list_activity(
    State(state): State<AppState>,
    Query(params): Query<ActivityParams>,
) -> Result<Json<Vec<hotsheet_ticketing::ActivityEvent>>, ApiError> {
    let min_importance = match params.min_importance.as_deref() {
        None => None,
        Some("low") => Some(hotsheet_ticketing::Importance::Low),
        Some("normal") => Some(hotsheet_ticketing::Importance::Normal),
        Some("high") => Some(hotsheet_ticketing::Importance::High),
        Some(other) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("invalid min_importance '{other}' (low|normal|high)"),
            ));
        }
    };
    let filter = hotsheet_ticketing::TimelineFilter {
        ticket: params.ticket,
        session: params.session,
        min_importance,
        limit: params.limit,
    };
    let events = hotsheet_ticketing::activity::timeline(&state.store, &filter)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(events))
}

/// `POST /announce` body: a store-level broadcast message (HS2-HHDNTH).
#[derive(Debug, Deserialize)]
struct AnnounceReq {
    message: String,
    /// The target store's URL id; omitted = the default store.
    #[serde(default)]
    store: Option<String>,
}

/// `POST /announce` — broadcast an ephemeral message to live `/ws/sync` subscribers. Not
/// persisted (no long-poll replay); a client not connected when it fires misses it.
async fn post_announce(
    State(state): State<AppState>,
    Json(body): Json<AnnounceReq>,
) -> Result<StatusCode, ApiError> {
    if body.message.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "empty announcement"));
    }
    let store = body
        .store
        .unwrap_or_else(|| multistore::store_url_id(&state.store));
    state.announce(store, body.message);
    Ok(StatusCode::NO_CONTENT)
}

// ---- multi-store (HS2-87) --------------------------------------------------------

/// `GET /stores` — the stores this machine server hosts.
async fn list_stores(State(state): State<AppState>) -> Json<Vec<StoreInfo>> {
    Json(state.host.list())
}

/// `GET /providers` — capability-bearing ticket-provider connections. The current
/// implementation registers every hosted store as a built-in git provider.
async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<hotsheet_ticketing::ProviderDescriptor>>, ApiError> {
    let default_id = multistore::store_url_id(&state.store);
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    let external_default = connections.iter().any(|connection| connection.default)
        || state
            .injected_providers
            .descriptors()
            .iter()
            .any(|descriptor| descriptor.default);
    let mut descriptors = state
        .host
        .list()
        .into_iter()
        .map(|info| {
            let is_default = info.id == default_id && !external_default;
            info.provider_descriptor(is_default)
        })
        .collect::<Vec<_>>();
    for connection in connections {
        if connection.provider != "git" {
            descriptors
                .push(hotsheet_extsync::descriptor(&connection).map_err(provider_transfer_error)?);
        }
    }
    descriptors.extend(state.injected_providers.descriptors());
    descriptors.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
    Ok(Json(descriptors))
}

/// Project-scoped, non-secret provider configuration. Credential values stay in the
/// server's key registry; this surface only carries a credential reference.
async fn list_provider_connections(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProviderConnection>>, ApiError> {
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map(Json)
        .map_err(provider_transfer_error)
}

fn validate_external_connection(connection: &ProviderConnection) -> Result<(), ApiError> {
    if connection.provider == "git" {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "git connections are managed through the store registry",
        ));
    }
    hotsheet_extsync::descriptor(connection)
        .map(|_| ())
        .map_err(provider_transfer_error)
}

fn save_provider_connections(
    state: &AppState,
    mut connections: Vec<ProviderConnection>,
    connection: ProviderConnection,
    replacing: Option<&str>,
) -> Result<(), ApiError> {
    validate_external_connection(&connection)?;
    if connection.default {
        for existing in &mut connections {
            existing.default = false;
        }
    }
    match replacing {
        Some(id) => {
            let slot = connections
                .iter_mut()
                .find(|candidate| candidate.id == id)
                .ok_or_else(|| ApiError::not_found(id))?;
            *slot = connection;
        }
        None => {
            if connections
                .iter()
                .any(|candidate| candidate.id == connection.id)
            {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "provider connection id already exists",
                ));
            }
            connections.push(connection);
        }
    }
    connections.sort_by(|a, b| a.id.cmp(&b.id));
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .save(&connections)
        .map_err(provider_transfer_error)
}

async fn create_provider_connection(
    State(state): State<AppState>,
    Json(connection): Json<ProviderConnection>,
) -> Result<(StatusCode, Json<ProviderConnection>), ApiError> {
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    save_provider_connections(&state, connections, connection.clone(), None)?;
    Ok((StatusCode::CREATED, Json(connection)))
}

async fn update_provider_connection(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(mut connection): Json<ProviderConnection>,
) -> Result<Json<ProviderConnection>, ApiError> {
    connection.id = connection_id.clone();
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    save_provider_connections(
        &state,
        connections,
        connection.clone(),
        Some(&connection_id),
    )?;
    Ok(Json(connection))
}

async fn delete_provider_connection(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let registry = ProviderConfigRegistry::new(state.store.root().join("providers.json"));
    let mut connections = registry.load().map_err(provider_transfer_error)?;
    let before = connections.len();
    connections.retain(|connection| connection.id != connection_id);
    if connections.len() == before {
        return Err(ApiError::not_found(&connection_id));
    }
    registry
        .save(&connections)
        .map_err(provider_transfer_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct ProviderTransferBody {
    source: TicketRef,
    destination_connection: String,
    operation_id: String,
    #[serde(default)]
    confirm: bool,
}

fn hosted_provider_registry(state: &AppState) -> Result<ProviderRegistry, ApiError> {
    let registry = ProviderRegistry::default();
    let default_id = multistore::store_url_id(&state.store);
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    let external_default = connections.iter().any(|connection| connection.default)
        || state
            .injected_providers
            .descriptors()
            .iter()
            .any(|descriptor| descriptor.default);
    for info in state.host.list() {
        let entry = state
            .host
            .get(&info.id)
            .ok_or_else(|| ApiError::not_found(&info.id))?;
        registry
            .register(Arc::new(
                GitProvider::new(info.id.clone(), entry.store)
                    .with_default(info.id == default_id && !external_default),
            ))
            .map_err(provider_transfer_error)?;
    }
    let keys = KeyRegistry::new(hotsheet_plugins::hotsheet_home(), OsKeychain);
    for connection in connections {
        if connection.provider == "git" {
            continue;
        }
        let credential =
            hotsheet_extsync::credential_reference(&connection).map_err(provider_transfer_error)?;
        let token = keys.get(credential).map_err(provider_transfer_error)?;
        registry
            .register(
                hotsheet_extsync::live_provider(&connection, token)
                    .map_err(provider_transfer_error)?,
            )
            .map_err(provider_transfer_error)?;
    }
    for descriptor in state.injected_providers.descriptors() {
        registry
            .register(
                state
                    .injected_providers
                    .get(&descriptor.connection_id)
                    .map_err(provider_transfer_error)?,
            )
            .map_err(provider_transfer_error)?;
    }
    Ok(registry)
}

fn provider_for(
    state: &AppState,
    connection_id: &str,
) -> Result<Arc<dyn hotsheet_ticketing::TicketProvider>, ApiError> {
    if let Some(entry) = state.host.get(connection_id) {
        return Ok(Arc::new(GitProvider::new(connection_id, entry.store)));
    }
    if let Ok(provider) = state.injected_providers.get(connection_id) {
        return Ok(provider);
    }
    let connection = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?
        .into_iter()
        .find(|connection| connection.id == connection_id)
        .ok_or_else(|| ApiError::not_found(connection_id))?;
    let credential =
        hotsheet_extsync::credential_reference(&connection).map_err(provider_transfer_error)?;
    let token = KeyRegistry::new(hotsheet_plugins::hotsheet_home(), OsKeychain)
        .get(credential)
        .map_err(provider_transfer_error)?;
    hotsheet_extsync::live_provider(&connection, token).map_err(provider_transfer_error)
}

#[derive(Debug, Deserialize)]
struct AttachmentCopyRef {
    connection_id: String,
    native_id: String,
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
struct AttachmentTicketRef {
    connection_id: String,
    native_id: String,
}

#[derive(Debug, Deserialize)]
struct CopyAttachmentReq {
    source: AttachmentCopyRef,
    destination: AttachmentTicketRef,
}

async fn copy_provider_attachment(
    State(state): State<AppState>,
    Json(req): Json<CopyAttachmentReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let source = provider_for(&state, &req.source.connection_id)?;
    let destination = provider_for(&state, &req.destination.connection_id)?;
    let source_ticket = source
        .get(&req.source.native_id)
        .map_err(provider_transfer_error)?;
    let metadata = source_ticket
        .attachments
        .into_iter()
        .find(|item| item.id == req.source.attachment_id)
        .ok_or_else(|| ApiError::not_found(&req.source.attachment_id))?;
    let bytes = source
        .attachment_bytes(&req.source.native_id, &req.source.attachment_id)
        .map_err(provider_transfer_error)?;
    let copied = destination
        .add_attachment(
            &req.destination.native_id,
            ApiAttachment {
                id: Ulid::new().to_string(),
                filename: metadata.filename,
                created_at: now().to_string(),
            },
            bytes,
        )
        .map_err(provider_transfer_error)?;
    Ok((StatusCode::CREATED, Json(copied)))
}

fn provider_transfer_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, error.to_string())
}

async fn list_provider_tickets(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<ApiTicket>>, ApiError> {
    let query = params.into_query(state.store.root())?;
    let provider = provider_for(&state, &connection_id)?;
    provider
        .query(&query)
        .map(Json)
        .map_err(provider_transfer_error)
}

async fn get_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    provider_for(&state, &connection_id)?
        .get(&id)
        .map(Json)
        .map_err(provider_transfer_error)
}

async fn create_provider_ticket(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let priority = opt_parse(req.priority.as_deref())?.unwrap_or_default();
    let provider = provider_for(&state, &connection_id)?;
    let ticket = provider
        .create(
            MutationContext {
                now: now(),
                generated_id: Ulid::new(),
            },
            ProviderDraft {
                title: req.title,
                category: req.category.unwrap_or_else(|| "issue".into()),
                priority,
                details: req.details.unwrap_or_default(),
                tags: req.tags.unwrap_or_default(),
                up_next: req.up_next.unwrap_or(false),
                blocked_by: req.blocked_by.unwrap_or_default(),
                transfer: None,
            },
        )
        .map_err(provider_transfer_error)?;
    Ok((StatusCode::CREATED, Json(ticket)))
}

async fn update_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let provider = provider_for(&state, &connection_id)?;
    if req.note_id.is_some() && !provider.supports_note_edit() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("provider connection '{connection_id}' does not support note editing"),
        ));
    }
    let timestamp = now();
    let note = req.note.clone();
    let note_id = req.note_id.clone();
    let note_kind = req.note_kind.unwrap_or(NoteKind::Regular);
    let mut ticket = provider
        .update(
            &id,
            timestamp.clone(),
            ProviderPatch {
                expected_token: req.expected_token,
                title: req.title,
                details: req.details,
                category: req.category,
                priority: opt_parse(req.priority.as_deref())?,
                status: opt_parse(req.status.as_deref())?,
                tags: req.tags,
                up_next: req.up_next,
                blocked_by: req.blocked_by,
            },
        )
        .map_err(provider_transfer_error)?;
    if let Some(note) = note {
        ticket = match note_id {
            Some(note_id) => provider.edit_note(&id, &note_id, timestamp, note),
            None => provider.add_note(
                &id,
                MutationContext {
                    now: timestamp,
                    generated_id: Ulid::new(),
                },
                note_kind,
                note,
            ),
        }
        .map_err(provider_transfer_error)?;
    }
    Ok(Json(ticket))
}

async fn report_provider_ticket_not_working(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<Json<ApiTicket>, ApiError> {
    let provider = provider_for(&state, &connection_id)?;
    if !provider.descriptor().capabilities.not_working_report {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "provider connection '{connection_id}' does not support an atomic Not Working report"
            ),
        ));
    }
    let timestamp = now();
    let mut note = None;
    let mut expected_token = None;
    let mut evidence = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
    {
        match field.name() {
            Some("note") => {
                note =
                    Some(field.text().await.map_err(|error| {
                        ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
                    })?);
            }
            Some("expected_token") => {
                expected_token =
                    Some(field.text().await.map_err(|error| {
                        ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
                    })?);
            }
            Some("evidence") => {
                let filename = field.file_name().unwrap_or("attachment").to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
                evidence.push(ProviderEvidence {
                    id: Ulid::new(),
                    filename,
                    created_at: timestamp.clone(),
                    bytes: bytes.to_vec(),
                });
            }
            _ => {}
        }
    }
    let note = note.and_then(|text| (!text.trim().is_empty()).then(|| (Ulid::new(), text)));
    if note.is_none() && evidence.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "a Not Working report requires a note or at least one evidence attachment",
        ));
    }
    provider
        .report_not_working(
            &id,
            timestamp,
            NotWorkingReport {
                expected_token,
                note,
                evidence,
            },
        )
        .map(Json)
        .map_err(provider_transfer_error)
}

async fn delete_provider_ticket_note(
    State(state): State<AppState>,
    Path((connection_id, id, note_id)): Path<(String, String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    let provider = provider_for(&state, &connection_id)?;
    if !provider.supports_note_delete() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("provider connection '{connection_id}' does not support note deletion"),
        ));
    }
    provider
        .delete_note(&id, &note_id, now())
        .map(Json)
        .map_err(provider_transfer_error)
}

async fn close_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    provider_for(&state, &connection_id)?
        .close(
            &id,
            now(),
            opt_parse(Some(&req.reason))?.expect("required close reason"),
            req.duplicate_of,
        )
        .map(Json)
        .map_err(provider_transfer_error)
}

async fn assign_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    Json(req): Json<AssignReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let at = now();
    let reviews = req
        .reviews
        .into_iter()
        .map(|review| ReviewRequest {
            who: review.who,
            kind: review.kind,
            by: Ulid::new(),
            at: at.clone(),
            requested_by: None,
        })
        .collect();
    provider_for(&state, &connection_id)?
        .assign(&id, at, req.assignees, reviews)
        .map(Json)
        .map_err(provider_transfer_error)
}

async fn provider_copy_route(
    State(state): State<AppState>,
    Json(body): Json<ProviderTransferBody>,
) -> Result<(StatusCode, Json<hotsheet_ticketing::TransferOutcome>), ApiError> {
    let outcome = copy_between(
        &hosted_provider_registry(&state)?,
        body.source,
        &body.destination_connection,
        &body.operation_id,
        now(),
    )
    .map_err(provider_transfer_error)?;
    Ok((StatusCode::CREATED, Json(outcome)))
}

async fn provider_move_route(
    State(state): State<AppState>,
    Json(body): Json<ProviderTransferBody>,
) -> Result<Json<hotsheet_ticketing::TransferOutcome>, ApiError> {
    if !body.confirm {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "provider move requires confirm=true",
        ));
    }
    let outcome = move_between(
        &hosted_provider_registry(&state)?,
        body.source,
        &body.destination_connection,
        &body.operation_id,
        now(),
    )
    .map_err(provider_transfer_error)?;
    Ok(Json(outcome))
}

#[derive(Debug, Deserialize)]
struct RegisterCheckoutBody {
    root: String,
    alias: Option<String>,
    repository: Option<String>,
    #[serde(default)]
    stores: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OpenProjectBody {
    root: String,
    alias: Option<String>,
    repository: Option<String>,
    /// Explicit git stores. When omitted, conservative filesystem discovery is used.
    stores: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct OpenProjectResponse {
    checkout: hotsheet_ticketing::checkouts::Checkout,
    discovered: bool,
}

/// Open a code checkout for client use: discover or accept its git ticket stores, host
/// them in this machine server, and persist the checkout-to-store links atomically from
/// the client's point of view. An empty result is valid and lets a settings UI ask the
/// user to choose one or more providers explicitly.
async fn open_project(
    State(state): State<AppState>,
    Json(body): Json<OpenProjectBody>,
) -> Result<(StatusCode, Json<OpenProjectResponse>), ApiError> {
    let root = FsPath::new(&body.root);
    let discovered = body.stores.is_none();
    let stores = match body.stores {
        Some(paths) => paths.into_iter().map(std::path::PathBuf::from).collect(),
        None => hotsheet_ticketing::checkouts::discover_ticket_stores(root)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?,
    };
    for path in &stores {
        let store = FsStore::open(path)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
        state.host_store(store)?;
    }
    let checkout = state
        .checkout_registry
        .register(root, body.alias.as_deref(), body.repository, stores)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    hotsheet_ticketing::worklist::regenerate_checkout(&checkout)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(OpenProjectResponse {
            checkout,
            discovered,
        }),
    ))
}

async fn list_checkouts(
    State(state): State<AppState>,
) -> Result<Json<Vec<hotsheet_ticketing::checkouts::Checkout>>, ApiError> {
    state
        .checkout_registry
        .list()
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn register_checkout(
    State(state): State<AppState>,
    Json(body): Json<RegisterCheckoutBody>,
) -> Result<(StatusCode, Json<hotsheet_ticketing::checkouts::Checkout>), ApiError> {
    let entry = state
        .checkout_registry
        .register(
            FsPath::new(&body.root),
            body.alias.as_deref(),
            body.repository,
            body.stores
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect(),
        )
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    hotsheet_ticketing::worklist::regenerate_checkout(&entry)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(entry)))
}

async fn resolve_checkout(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<hotsheet_ticketing::checkouts::Checkout>, ApiError> {
    state
        .checkout_registry
        .resolve(&reference)
        .map(Json)
        .map_err(|e| {
            let status = match e {
                hotsheet_ticketing::checkouts::CheckoutError::NotFound(_) => StatusCode::NOT_FOUND,
                hotsheet_ticketing::checkouts::CheckoutError::Ambiguous(_) => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            ApiError::new(status, e.to_string())
        })
}

async fn checkout_repository_status(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<hotsheet_ticketing::repository_status::RepositoryStatus>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    hotsheet_ticketing::repository_status::snapshot(FsPath::new(&checkout.root))
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))
}

fn checkout_entries(
    state: &AppState,
    reference: &str,
) -> Result<Vec<(String, StoreEntry)>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let mut entries = Vec::new();
    for store_path in checkout.stores {
        let canonical = FsPath::new(&store_path)
            .canonicalize()
            .unwrap_or_else(|_| store_path.clone().into());
        let Some(info) = state.host.list().into_iter().find(|info| {
            FsPath::new(&info.root)
                .canonicalize()
                .unwrap_or_else(|_| info.root.clone().into())
                == canonical
        }) else {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!("checkout {reference} links an unhosted store: {store_path}"),
            ));
        };
        if let Some(entry) = state.host.get(&info.id) {
            entries.push((info.id, entry));
        }
    }
    if entries.is_empty() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("checkout {reference} has no linked ticket stores"),
        ));
    }
    Ok(entries)
}

async fn list_checkout_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let mut result = Vec::new();
    for (store_id, entry) in checkout_entries(&state, &reference)? {
        let compact = params.compact.unwrap_or(true);
        let fields = parse_fields(&params.fields);
        let query = params.clone().into_query(entry.store.root())?;
        let mut rows = entry
            .index
            .lock()
            .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned"))?
            .query(&query)?;
        for row in &mut rows {
            row.set_connection(&store_id);
        }
        if compact {
            for row in &mut rows {
                row.make_compact();
            }
        }
        let contexts = auto_context::effective(&Settings::new(entry.store.root()))
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        for row in &mut rows {
            row.add_auto_context(&contexts);
        }
        let values = rows_to_json(rows, &fields)
            .as_array()
            .cloned()
            .unwrap_or_default();
        for mut value in values {
            if let Some(obj) = value.as_object_mut() {
                obj.insert("store".into(), serde_json::Value::String(store_id.clone()));
            }
            result.push(value);
        }
    }
    Ok(Json(result))
}

#[derive(Serialize)]
struct CheckoutCorruptTicket {
    store: String,
    store_path: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slug: Option<String>,
    error: String,
    error_code: &'static str,
}

async fn list_checkout_corrupt_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<CheckoutCorruptTicket>>, ApiError> {
    let mut result = Vec::new();
    for (store_id, entry) in checkout_entries(&state, &reference)? {
        let store_path = entry.store.root().display().to_string();
        for corrupt in entry.store.list_tickets_resilient()?.corrupt {
            result.push(CheckoutCorruptTicket {
                store: store_id.clone(),
                store_path: store_path.clone(),
                path: corrupt.path.display().to_string(),
                id: corrupt.id.map(|id| id.to_string()),
                slug: corrupt.slug,
                error: corrupt.error,
                error_code: corrupt.error_code,
            });
        }
    }
    Ok(Json(result))
}

#[derive(Deserialize)]
struct CorruptTicketRepairReq {
    path: String,
}

/// Create (or return) an Up Next work item that directs an AI worker to repair one
/// currently-corrupt git ticket. The corrupt file itself is never modified by this API.
async fn create_corrupt_ticket_repair(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<CorruptTicketRepairReq>,
) -> Result<Response, ApiError> {
    let requested = std::path::PathBuf::from(&req.path);
    for (_, entry) in checkout_entries(&state, &reference)? {
        let listing = entry.store.list_tickets_resilient()?;
        let Some(corrupt) = listing
            .corrupt
            .into_iter()
            .find(|item| item.path == requested)
        else {
            continue;
        };
        if corrupt.error_code == "upgrade_required" {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "This ticket requires a newer Hot Sheet 2 version and cannot be safely auto-repaired.",
            ));
        }

        let marker = format!(
            "<!-- hotsheet:corrupt-repair {} -->",
            serde_json::to_string(&req.path).unwrap_or_else(|_| "null".into())
        );
        if let Some(existing) = listing
            .tickets
            .iter()
            .find(|ticket| ticket.details.contains(&marker) && ops::is_open(ticket))
        {
            return Ok((StatusCode::OK, Json(api_ticket(&entry, existing)?)).into_response());
        }

        let identity = corrupt
            .slug
            .clone()
            .or_else(|| corrupt.id.map(|id| id.to_string()))
            .or_else(|| {
                corrupt
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "unreadable ticket".into());
        let details = format!(
            "Repair the corrupt Hot Sheet ticket file at `{}`.\n\nThe parser reported:\n\n```text\n{}\n```\n\nPreserve all recoverable ticket content, rewrite it with the current canonical ticket format, and verify that Hot Sheet can parse and list it. Do not delete the file or discard content merely to make parsing succeed.\n\n{}",
            req.path, corrupt.error, marker
        );
        let created = do_create(
            &state,
            &entry,
            CreateReq {
                title: format!("Repair corrupt ticket {identity}"),
                category: Some("bug".into()),
                priority: Some("high".into()),
                details: Some(details),
                tags: Some(vec!["corrupt-ticket".into(), "ai-repair".into()]),
                up_next: Some(true),
                blocked_by: None,
            },
        )?;
        return Ok((StatusCode::CREATED, Json(created)).into_response());
    }
    Err(ApiError::new(
        StatusCode::NOT_FOUND,
        "The corrupt ticket is no longer present in this checkout.",
    ))
}

#[derive(Deserialize)]
struct CheckoutStoreQuery {
    store: Option<String>,
}
fn checkout_entry_for_create(
    state: &AppState,
    reference: &str,
    requested: Option<&str>,
) -> Result<StoreEntry, ApiError> {
    let entries = checkout_entries(state, reference)?;
    if let Some(id) = requested {
        return entries
            .into_iter()
            .find(|(store_id, entry)| store_id == id || entry.store.root().to_string_lossy() == id)
            .map(|(_, e)| e)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "requested store is not linked to checkout",
                )
            });
    }
    match entries.as_slice() {
        [(_, entry)] => Ok(entry.clone()),
        _ => Err(ApiError::new(
            StatusCode::CONFLICT,
            "checkout has multiple stores; specify ?store=<store-id>",
        )),
    }
}
fn checkout_entry_for_ticket(
    state: &AppState,
    reference: &str,
    id: &str,
) -> Result<StoreEntry, ApiError> {
    let mut found = Vec::new();
    for (_, entry) in checkout_entries(state, reference)? {
        if ops::resolve(&entry.store, id)?.is_some() {
            found.push(entry);
        }
    }
    match found.as_slice() {
        [entry] => Ok(entry.clone()),
        [] => Err(ApiError::not_found(id)),
        _ => Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("ticket {id} is ambiguous across checkout stores"),
        )),
    }
}
async fn create_checkout_ticket(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(q): Query<CheckoutStoreQuery>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let entry = checkout_entry_for_create(&state, &reference, q.store.as_deref())?;
    Ok((StatusCode::CREATED, Json(do_create(&state, &entry, req)?)))
}
async fn get_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let entry = checkout_entry_for_ticket(&state, &reference, &id)?;
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket(&entry, &ticket)?,
    }))
}
async fn update_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let entry = checkout_entry_for_ticket(&state, &reference, &id)?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: do_update(&state, &entry, &id, req)?,
    }))
}

#[derive(Deserialize)]
struct CheckoutBatchUpdateReq {
    id: String,
    #[serde(flatten)]
    update: UpdateReq,
}

#[derive(Deserialize)]
struct CheckoutBatchReq {
    updates: Vec<CheckoutBatchUpdateReq>,
}

/// Apply a multi-selection update through one checkout-scoped request. All optimistic
/// concurrency tokens are validated before the first write, so a stale selection cannot
/// partially apply while the client still sees one logical bulk operation.
async fn batch_update_checkout_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<CheckoutBatchReq>,
) -> Result<Json<Vec<ResolvedTicket>>, ApiError> {
    if req.updates.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bulk update requires at least one ticket",
        ));
    }
    let mut resolved = Vec::with_capacity(req.updates.len());
    for item in req.updates {
        let entry = checkout_entry_for_ticket(&state, &reference, &item.id)?;
        let ticket =
            ops::resolve(&entry.store, &item.id)?.ok_or_else(|| ApiError::not_found(&item.id))?;
        if item
            .update
            .expected_token
            .as_deref()
            .is_some_and(|token| token != ticket.updated_at.as_str())
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!("ticket {} changed since it was read", item.id),
            ));
        }
        resolved.push((entry, item));
    }
    let mut updated = Vec::with_capacity(resolved.len());
    for (entry, item) in resolved {
        updated.push(ResolvedTicket {
            store: multistore::store_url_id(&entry.store),
            ticket: do_update(&state, &entry, &item.id, item.update)?,
        });
    }
    Ok(Json(updated))
}
async fn delete_checkout_ticket_note(
    State(state): State<AppState>,
    Path((reference, id, note_id)): Path<(String, String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let entry = checkout_entry_for_ticket(&state, &reference, &id)?;
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let note_id = Ulid::from_string(&note_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid note ULID"))?;
    let updated = ops::delete_note(&entry.store, &ticket.id, &note_id, now())?;
    state.changed_in(&entry, "updated", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket(&entry, &updated)?,
    }))
}
async fn close_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let entry = checkout_entry_for_ticket(&state, &reference, &id)?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: do_close(&state, &entry, &id, req)?,
    }))
}
async fn assign_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<AssignReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let entry = checkout_entry_for_ticket(&state, &reference, &id)?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: do_assign(&state, &entry, &id, req)?,
    }))
}

async fn add_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<ResolvedTicket>), ApiError> {
    let entry = checkout_entry_for_ticket(&state, &reference, &id)?;
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let filename = attachment_filename(&headers)?;
    let (updated, _) =
        entry
            .store
            .write_attachment(&ticket.id, Ulid::new(), now(), &filename, &body)?;
    state.changed_in(&entry, "attachment_added", &updated);
    Ok((
        StatusCode::CREATED,
        Json(ResolvedTicket {
            store: multistore::store_url_id(&entry.store),
            ticket: api_ticket(&entry, &updated)?,
        }),
    ))
}

async fn get_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let entry = checkout_entry_for_ticket(&state, &reference, &id)?;
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, bytes) = entry
        .store
        .read_attachment(&ticket.id, &attachment_id)
        .map_err(|error| match &error {
            StoreError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                ApiError::not_found(&attachment_id.to_string())
            }
            _ => error.into(),
        })?;
    let mut headers = HeaderMap::new();
    let content_type = match FsPath::new(&attachment.filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };
    headers.insert(
        "content-type",
        content_type.parse().expect("static content type"),
    );
    if let Ok(filename) = attachment.filename.parse() {
        headers.insert("x-hotsheet-filename", filename);
    }
    Ok((headers, Bytes::from(bytes)).into_response())
}

async fn delete_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let entry = checkout_entry_for_ticket(&state, &reference, &id)?;
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let updated = entry
        .store
        .remove_attachment(&ticket.id, &attachment_id, now())
        .map_err(|error| match &error {
            StoreError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                ApiError::not_found(&attachment_id.to_string())
            }
            _ => error.into(),
        })?;
    state.changed_in(&entry, "attachment_removed", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket(&entry, &updated)?,
    }))
}

async fn ticket_flow_summary(
    State(state): State<AppState>,
) -> Result<Json<hotsheet_ticketing::analytics::TicketFlowSummary>, ApiError> {
    let tickets = ops::query(&state.store, &TicketQuery::default())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(hotsheet_ticketing::analytics::ticket_flow(&tickets)))
}

async fn usage_metrics_summary(
    State(state): State<AppState>,
) -> Result<Json<hotsheet_ticketing::metrics::Rollup>, ApiError> {
    hotsheet_ticketing::metrics::summary_settled(&state.store)
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn list_commands(
    State(state): State<AppState>,
) -> Json<Vec<hotsheet_ticketing::commands::CommandDefinition>> {
    Json(state.commands.definitions())
}

async fn save_commands(
    State(state): State<AppState>,
    Json(definitions): Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>,
) -> Result<Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>, ApiError> {
    use std::collections::HashSet;
    let mut ids = HashSet::new();
    if definitions.iter().any(|definition| {
        definition.id.trim().is_empty()
            || definition.title.trim().is_empty()
            || definition.program.trim().is_empty()
            || !ids.insert(definition.id.as_str())
    }) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "commands require unique non-empty id, title, and program values",
        ));
    }
    Settings::new(state.store.root())
        .set(
            "commands",
            serde_json::to_value(&definitions)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?,
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state.commands.replace_definitions(definitions.clone());
    Ok(Json(definitions))
}

async fn run_command(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<commands::CommandRun>), ApiError> {
    state
        .commands
        .start(&id)
        .map(|r| (StatusCode::ACCEPTED, Json(r)))
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))
}

async fn list_command_runs(State(state): State<AppState>) -> Json<Vec<commands::CommandRun>> {
    Json(state.commands.list())
}

#[derive(Deserialize)]
struct RunOutputQuery {
    #[serde(default)]
    after: u64,
}

async fn get_command_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RunOutputQuery>,
) -> Result<Json<commands::CommandRun>, ApiError> {
    state
        .commands
        .get(&id, query.after)
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown command run"))
}

async fn cancel_command_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<commands::CommandRun>, ApiError> {
    state
        .commands
        .cancel(&id)
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::CONFLICT, e))
}

#[derive(Deserialize)]
struct NotificationQuery {
    checkout: Option<String>,
    store: Option<String>,
    ticket: Option<String>,
    recipient: Option<String>,
}
async fn list_notifications(
    State(state): State<AppState>,
    Query(q): Query<NotificationQuery>,
) -> Json<Vec<notifications::Notification>> {
    Json(state.notifications.list(
        q.checkout.as_deref(),
        q.store.as_deref(),
        q.ticket.as_deref(),
        q.recipient.as_deref(),
    ))
}
async fn publish_notification(
    State(state): State<AppState>,
    Json(body): Json<notifications::NewNotification>,
) -> (StatusCode, Json<notifications::Notification>) {
    let n = state.notifications.publish(body);
    state.emit(ChangeEvent {
        store: n.store.clone().unwrap_or_default(),
        kind: "notification".into(),
        id: n.id.clone(),
        slug: n.ticket.clone().unwrap_or_default(),
        message: Some(n.message.clone()),
        activity: None,
        assignment: None,
    });
    (StatusCode::CREATED, Json(n))
}
async fn acknowledge_notification(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<notifications::Notification>, ApiError> {
    state
        .notifications
        .acknowledge(&id)
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown notification"))
}
async fn synthesize_speech(
    State(state): State<AppState>,
    Json(body): Json<tts::TtsRequest>,
) -> Result<Response, ApiError> {
    let audio = state
        .tts
        .synthesize(&body)
        .map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, e))?;
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", audio.content_type)
        .body(axum::body::Body::from(audio.bytes))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Body for `POST /stores`: register another local store by its path.
#[derive(Deserialize)]
struct AddStoreBody {
    path: String,
}

/// `POST /stores` — open a store at `path` (building its own in-memory index) and host it.
/// Idempotent: registering an already-hosted store just returns it.
async fn add_store(
    State(state): State<AppState>,
    Json(body): Json<AddStoreBody>,
) -> Result<(StatusCode, Json<StoreInfo>), ApiError> {
    let store = FsStore::open(&body.path)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    let id = multistore::store_url_id(&store);
    let newly = state.host_store(store)?;
    let info = state
        .host
        .list()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "store vanished"))?;
    let code = if newly {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((code, Json(info)))
}

/// `GET /stores/{store_id}/tickets` — the store-scoped list, served from that store's own
/// index. Unknown id → 404.
async fn list_store_tickets(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let entry = state
        .host
        .get(&store_id)
        .ok_or_else(|| ApiError::not_found(&store_id))?;
    let compact = params.compact.unwrap_or(true);
    let fields = parse_fields(&params.fields);
    let query = params.into_query(entry.store.root())?;
    let mut rows = entry
        .index
        .lock()
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned"))?
        .query(&query)?;
    for row in &mut rows {
        row.set_connection(&store_id);
    }
    if compact {
        for row in &mut rows {
            row.make_compact();
        }
    }
    let contexts = auto_context::effective(&Settings::new(entry.store.root()))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    for row in &mut rows {
        row.add_auto_context(&contexts);
    }
    Ok(Json(rows_to_json(rows, &fields)))
}

// The write logic is store-generic: it operates on a `StoreEntry` so the unprefixed
// (default store) routes and the `/stores/{id}/…` scoped routes share one implementation.

fn do_create(state: &AppState, entry: &StoreEntry, req: CreateReq) -> Result<ApiTicket, ApiError> {
    let prefix = entry.store.metadata()?.ticket_prefix;
    let blocked_by =
        ops::resolve_blockers(&entry.store, None, &req.blocked_by.unwrap_or_default())?;
    let new = NewTicket {
        title: req.title,
        category: req.category.unwrap_or_else(|| "issue".to_string()),
        priority: opt_parse(req.priority.as_deref())?.unwrap_or_default(),
        details: req.details.unwrap_or_default(),
        tags: req.tags.unwrap_or_default(),
        up_next: req.up_next.unwrap_or(false),
        blocked_by,
    };
    let ticket = ops::create(&entry.store, Ulid::new(), &prefix, now(), new)?;
    state.changed_in(entry, "created", &ticket);
    api_ticket(entry, &ticket)
}

fn do_update(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
    req: UpdateReq,
) -> Result<ApiTicket, ApiError> {
    let ticket = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    if req
        .expected_token
        .as_deref()
        .is_some_and(|token| token != ticket.updated_at.as_str())
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("ticket '{}' changed since it was read", ticket.slug),
        ));
    }
    let edit_note_id = req
        .note_id
        .as_deref()
        .map(|note_id| {
            Ulid::from_string(note_id).map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid note ULID '{note_id}'"),
                )
            })
        })
        .transpose()?;
    if let Some(note_id) = edit_note_id
        && !ticket.notes.iter().any(|note| note.id == note_id)
    {
        return Err(ApiError::not_found(&format!("note {note_id}")));
    }
    // A present `blocked_by` (even []) replaces the set; absent leaves it unchanged.
    let blocked_by = match req.blocked_by {
        Some(needles) => Some(ops::resolve_blockers(
            &entry.store,
            Some(&ticket.id),
            &needles,
        )?),
        None => None,
    };
    let patch = TicketPatch {
        title: req.title,
        details: req.details,
        category: req.category,
        priority: opt_parse(req.priority.as_deref())?,
        status: opt_parse(req.status.as_deref())?,
        tags: req.tags,
        up_next: req.up_next,
        blocked_by,
    };
    let updated = ops::update(&entry.store, &ticket.id, now(), patch)?;
    // An optional note append/edit rides the same update call (parity with CLI + MCP).
    let latest = match req.note.filter(|t| !t.is_empty()) {
        Some(text) => match edit_note_id {
            Some(note_id) => ops::edit_note(&entry.store, &ticket.id, &note_id, now(), text)?,
            None => ops::add_note(
                &entry.store,
                &ticket.id,
                Ulid::new(),
                now(),
                req.note_kind.unwrap_or(NoteKind::Regular),
                text,
            )?,
        },
        None => updated,
    };
    state.changed_in(entry, "updated", &latest);
    api_ticket(entry, &latest)
}

#[derive(Debug, Deserialize)]
struct AssignReq {
    /// Present replaces the assignee set; absent leaves it unchanged.
    assignees: Option<Vec<String>>,
    #[serde(default)]
    reviews: Vec<ReviewInput>,
}
#[derive(Debug, Deserialize)]
struct ReviewInput {
    who: String,
    kind: ReviewKind,
}

fn do_assign(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
    req: AssignReq,
) -> Result<ApiTicket, ApiError> {
    let before = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    let at = now();
    let reviews = req
        .reviews
        .into_iter()
        .map(|r| ReviewRequest {
            who: r.who,
            kind: r.kind,
            by: Ulid::new(),
            at: at.clone(),
            requested_by: None,
        })
        .collect();
    let ticket = ops::assign(&entry.store, &before.id, at, req.assignees, reviews)?;
    let newly_assigned = ticket
        .assignees
        .iter()
        .filter(|who| !before.assignees.contains(who))
        .cloned()
        .collect::<Vec<_>>();
    let review_requested = ticket
        .review_requests
        .iter()
        .filter(|r| !before.review_requests.iter().any(|old| old.by == r.by))
        .map(|r| r.who.clone())
        .collect::<Vec<_>>();
    state.changed_in(entry, "assigned", &ticket);
    let requested_by = hotsheet_ticketing::current_user_email(entry.store.root());
    state.emit(ChangeEvent {
        store: multistore::store_url_id(&entry.store),
        kind: "assignment".into(),
        id: ticket.id.to_string(),
        slug: ticket.slug.clone(),
        message: None,
        activity: None,
        assignment: Some(AssignmentEvent {
            newly_assigned: newly_assigned.clone(),
            review_requested: review_requested.clone(),
            requested_by: requested_by.clone(),
        }),
    });
    for (recipient, action) in newly_assigned
        .iter()
        .map(|v| (v, "assigned"))
        .chain(review_requested.iter().map(|v| (v, "review-requested")))
    {
        state.notifications.publish(notifications::NewNotification {
            message: format!("{action}: {} — {}", ticket.slug, ticket.title),
            severity: "info".into(),
            checkout: None,
            store: Some(multistore::store_url_id(&entry.store)),
            ticket: Some(ticket.slug.clone()),
            recipient: Some(recipient.clone()),
            dedupe_key: Some(format!("{action}:{}:{recipient}", ticket.id)),
        });
    }
    api_ticket(entry, &ticket)
}

async fn assign_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AssignReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    Ok(Json(do_assign(&state, &state.default_entry(), &id, req)?))
}

fn do_close(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
    req: CloseReq,
) -> Result<ApiTicket, ApiError> {
    let ticket = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    let reason: CloseReason = opt_parse(Some(req.reason.as_str()))?.expect("reason present");
    let dup = match req.duplicate_of {
        Some(d) => Some(
            ops::resolve(&entry.store, &d)?
                .ok_or_else(|| ApiError::not_found(&d))?
                .id,
        ),
        None => None,
    };
    let closed = ops::close(&entry.store, &ticket.id, now(), reason, dup)?;
    state.changed_in(entry, "closed", &closed);
    api_ticket(entry, &closed)
}

async fn create_ticket(
    State(state): State<AppState>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let ticket = do_create(&state, &state.default_entry(), req)?;
    Ok((StatusCode::CREATED, Json(ticket)))
}

async fn update_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    Ok(Json(do_update(&state, &state.default_entry(), &id, req)?))
}

async fn delete_ticket_note(
    State(state): State<AppState>,
    Path((id, note_id)): Path<(String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = state.default_entry();
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let note_id = Ulid::from_string(&note_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid note ULID"))?;
    let updated = ops::delete_note(&entry.store, &ticket.id, &note_id, now())?;
    state.changed_in(&entry, "updated", &updated);
    Ok(Json(api_ticket(&entry, &updated)?))
}

async fn add_ticket_attachment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let entry = state.default_entry();
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let filename = attachment_filename(&headers)?;
    let (updated, _) =
        entry
            .store
            .write_attachment(&ticket.id, Ulid::new(), now(), &filename, &body)?;
    state.changed_in(&entry, "attachment_added", &updated);
    Ok((StatusCode::CREATED, Json(api_ticket(&entry, &updated)?)))
}

fn attachment_filename(headers: &HeaderMap) -> Result<String, ApiError> {
    let raw = headers
        .get("x-hotsheet-filename")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "missing x-hotsheet-filename"))?;
    if headers
        .get("x-hotsheet-filename-encoding")
        .and_then(|value| value.to_str().ok())
        != Some("percent")
    {
        return Ok(raw.to_owned());
    }
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes
                .get(index + 1)
                .and_then(|value| (*value as char).to_digit(16));
            let low = bytes
                .get(index + 2)
                .and_then(|value| (*value as char).to_digit(16));
            let (Some(high), Some(low)) = (high, low) else {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid encoded attachment filename",
                ));
            };
            decoded.push(((high << 4) | low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid encoded attachment filename",
        )
    })
}

async fn close_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    Ok(Json(do_close(&state, &state.default_entry(), &id, req)?))
}

/// `POST /batch` — apply the same field update to many tickets (HS2-86). One bad id doesn't
/// abort the rest: each ticket's outcome is reported. Default-store.
async fn batch_update(
    State(state): State<AppState>,
    Json(req): Json<BatchReq>,
) -> Json<BatchResult> {
    let entry = state.default_entry();
    let mut updated = Vec::new();
    let mut errors = Vec::new();
    for id in &req.ids {
        match do_update(&state, &entry, id, req.update.clone()) {
            Ok(t) => updated.push(t.slug),
            Err(e) => errors.push(BatchError {
                id: id.clone(),
                message: e.message,
            }),
        }
    }
    Json(BatchResult { updated, errors })
}

// ---- coordination: claim / release / renew (HS2-86) ------------------------------

const DEFAULT_LEASE_MINUTES: i64 = 30;

/// `POST /claim-next` — atomically claim the top available ticket for a worker. Returns the
/// claimed ticket, or `null` (200) when nothing is claimable.
async fn claim_next_ticket(
    State(state): State<AppState>,
    Json(req): Json<ClaimReq>,
) -> Result<Json<Option<ApiTicket>>, ApiError> {
    let entry = state.default_entry();
    let now = now();
    let lease = now.plus_minutes(req.lease_minutes.unwrap_or(DEFAULT_LEASE_MINUTES));
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let claimed = ops::claim_next(&entry.store, &now, lease, &worker, req.label)?;
    if let Some(t) = &claimed {
        state.changed_in(&entry, "claimed", t);
    }
    Ok(Json(
        claimed
            .as_ref()
            .map(|ticket| api_ticket(&entry, ticket))
            .transpose()?,
    ))
}

/// `POST /tickets/{id}/release` — release a claim (holder-only unless `force`).
async fn release_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ReleaseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = state.default_entry();
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let released = ops::release(
        &entry.store,
        &ticket.id,
        now(),
        &worker,
        req.force.unwrap_or(false),
    )?;
    state.changed_in(&entry, "released", &released);
    Ok(Json(api_ticket(&entry, &released)?))
}

/// `POST /tickets/{id}/renew` — extend a claim's lease (holder-only).
async fn renew_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RenewReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = state.default_entry();
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let now = now();
    let lease = now.plus_minutes(req.lease_minutes.unwrap_or(DEFAULT_LEASE_MINUTES));
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let renewed = ops::renew(&entry.store, &ticket.id, now, lease, &worker)?;
    state.changed_in(&entry, "renewed", &renewed);
    Ok(Json(api_ticket(&entry, &renewed)?))
}

// ---- permission round-trip (HS2-9R9YZW) ------------------------------------------

/// `GET /permissions` — the requests a driven tool is currently blocked on, for a client
/// to render + answer. Each carries the raising connection + the `(tool, action)` asked.
async fn list_permissions(State(state): State<AppState>) -> Json<Vec<PermissionInfo>> {
    let always_allow_supported = state.permission_rules_path.is_some();
    Json(
        state
            .permissions
            .pending()
            .into_iter()
            .map(|request| PermissionInfo {
                id: request.id,
                connection: request.connection,
                tool: request.tool,
                action: request.action,
                always_allow_supported,
            })
            .collect(),
    )
}

/// Client-facing pending permission request. The capability flag is explicit so a client
/// never offers a durable response when this server has no rule store configured.
#[derive(Serialize)]
struct PermissionInfo {
    id: u64,
    connection: String,
    tool: String,
    action: String,
    always_allow_supported: bool,
}

/// Body for `POST /permissions/ask`: who's asking + what.
#[derive(Deserialize)]
struct AskBody {
    /// The live connection id (so a client attributes the prompt to the right tool).
    connection: String,
    /// The tool/action asking (e.g. `"Bash"`, `"Edit"`) — the rule-match key.
    tool: String,
    action: String,
}

/// `POST /permissions/ask` `{connection, tool, action}` — raise a permission request and
/// **block** until a human answers over the route-back (`POST /permissions/{id}`), up to a
/// timeout then a safe `deny`. This is the *asking* side, for an external tool transport
/// like the Claude PreToolUse hook (HS2-YMR9HE). An allow-rule answers immediately.
async fn ask_permission(
    State(state): State<AppState>,
    Json(body): Json<AskBody>,
) -> Json<serde_json::Value> {
    let bridge = state.permissions.clone();
    // request_blocking_timeout blocks (Condvar); run it off the async runtime.
    let decision = tokio::task::spawn_blocking(move || {
        bridge.request_blocking_timeout(
            body.connection,
            body.tool,
            body.action,
            hotsheet_aitools::DEFAULT_PERMISSION_TIMEOUT,
            hotsheet_aitools::PermissionDecision::Deny,
        )
    })
    .await
    .unwrap_or(hotsheet_aitools::PermissionDecision::Deny);
    let allow = decision == hotsheet_aitools::PermissionDecision::Allow;
    Json(serde_json::json!({ "decision": if allow { "allow" } else { "deny" } }))
}

/// One driven connection as reported by `GET /connections`.
#[derive(Serialize)]
struct ConnectionInfo {
    id: String,
    tool: String,
    project: String,
    /// `main` | `worker`.
    role: String,
    /// Whether the connection is busy (a turn is actively streaming) right now.
    busy: bool,
}

/// `GET /connections` — what the server's driving loop is currently running (HS2-TCV3BF):
/// one entry per in-flight driven ticket. Empty when nothing is being driven.
async fn list_connections(State(state): State<AppState>) -> Json<Vec<ConnectionInfo>> {
    let now = now_ms();
    let reg = match state.drive_registry.lock() {
        Ok(r) => r,
        Err(_) => return Json(Vec::new()),
    };
    let infos = reg
        .list()
        .into_iter()
        .map(|c| ConnectionInfo {
            id: c.id.clone(),
            tool: c.tool.clone(),
            project: c.project.clone(),
            role: format!("{:?}", c.role).to_lowercase(),
            busy: reg.is_busy(&c.id, now),
        })
        .collect();
    Json(infos)
}

/// Wall-clock epoch milliseconds (the driving loop's busy-tracking time base).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---- terminals (HS2-A6R5QV) ------------------------------------------------------

/// Body for `POST /terminals`: what to run.
#[derive(Deserialize)]
struct OpenTerminalReq {
    /// The program to spawn (e.g. `bash`, `codex`).
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    /// Working directory (defaults to the served store root).
    cwd: Option<String>,
    /// Client-chosen terminal id; a ULID is minted when omitted.
    id: Option<String>,
    /// When set, register this terminal as a live AI-tool **connection** for that plugin id
    /// (e.g. `claude`), so `GET /connections` shows it and its busy is fed from the terminal's
    /// OSC-133 / spinner inference (HS2-4M67VN). Omit for a plain shell terminal.
    #[serde(default)]
    connect: Option<String>,
}

/// One terminal as reported by `GET /terminals`.
#[derive(Serialize)]
struct TerminalInfo {
    id: String,
    /// The PTY is still running.
    alive: bool,
    /// Inferred busy (a tool is actively working) vs idle.
    busy: bool,
    /// The shell's reported working directory (OSC 7), if any (HS2-RCKEJ9).
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    /// A currently-open hyperlink URI (OSC 8), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<String>,
    /// Tool progress percent 0-100 (OSC 9;4), if reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<u8>,
}

/// The terminal-manager key for a terminal id — the served store root is the project.
fn term_key(state: &AppState, id: &str) -> hotsheet_terminals::TermKey {
    (state.store.root().display().to_string(), id.to_string())
}

fn term_info(term: &hotsheet_terminals::Terminal, id: &str) -> TerminalInfo {
    let osc = term.term_state();
    TerminalInfo {
        id: id.to_string(),
        alive: term.is_alive(),
        busy: term.activity() == hotsheet_terminals::Activity::Busy,
        cwd: osc.cwd,
        link: osc.link,
        progress: osc.progress,
    }
}

/// Map a broker terminal-info onto the HTTP `TerminalInfo`.
fn broker_info(bi: hotsheet_terminals::BrokerTermInfo) -> TerminalInfo {
    TerminalInfo {
        id: bi.id,
        alive: bi.alive,
        busy: bi.busy,
        cwd: bi.cwd,
        link: bi.link,
        progress: bi.progress,
    }
}

/// Map a non-success broker response to an `ApiError`.
fn broker_err(resp: hotsheet_terminals::BrokerResponse) -> ApiError {
    use hotsheet_terminals::BrokerResponse as R;
    match resp {
        R::Err { message } => ApiError::new(StatusCode::BAD_REQUEST, message),
        R::NotFound => ApiError::new(StatusCode::NOT_FOUND, "no such terminal"),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unexpected broker response: {other:?}"),
        ),
    }
}

/// `POST /terminals` `{command, args?, cwd?, id?, connect?}` — open (or reattach to) a PTY.
async fn open_terminal(
    State(state): State<AppState>,
    Json(req): Json<OpenTerminalReq>,
) -> Result<Json<TerminalInfo>, ApiError> {
    let launch = terminal_launch(&state, &req)?;
    let id = req.id.unwrap_or_else(|| Ulid::new().to_string());

    // Broker mode: the PTY lives in the detached broker (survives a server restart).
    if let Some(tb) = &state.terminal_broker {
        // Newly-spawned vs reattach: a pre-check keeps the `connect` busy feed one-per-terminal
        // (no duplicate poll task on a reattach), mirroring the in-process path.
        let newly_spawned = !matches!(
            tb.call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
                .await,
            Ok(hotsheet_terminals::BrokerResponse::Read { .. })
        );
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Open {
                id: id.clone(),
                command: launch.command,
                args: launch.args,
                cwd: req.cwd,
                env: launch.env,
            })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        let info = match resp {
            hotsheet_terminals::BrokerResponse::Terminal { info } => info,
            other => return Err(broker_err(other)),
        };
        // Tool-in-terminal (HS2-4M67VN): register a live connection + feed its busy from the
        // broker-hosted terminal's inference (polled over the socket), once per fresh terminal.
        if let (Some(tool), true) = (&req.connect, newly_spawned) {
            register_broker_terminal_connection(&state, &id, tool, tb.clone());
        }
        return Ok(Json(broker_info(info)));
    }

    let newly_spawned = state.terminals.get(&term_key(&state, &id)).is_none();
    let spec = hotsheet_terminals::TermSpec {
        command: launch.command,
        args: launch.args,
        cwd: Some(
            req.cwd
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| state.store.root().to_path_buf()),
        ),
        env: launch.env,
        rows: 24,
        cols: 80,
    };
    let term = state
        .terminals
        .get_or_spawn(term_key(&state, &id), spec)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;

    // Tool-in-terminal (HS2-4M67VN): register a live connection + feed its busy from the
    // terminal's inference, once per fresh terminal (not on a reattach).
    if let (Some(tool), true) = (&req.connect, newly_spawned) {
        register_terminal_connection(&state, &id, tool, term.clone());
    }
    Ok(Json(term_info(&term, &id)))
}

/// Compose either the caller's explicit command or a plugin-declared interactive launch.
/// Connect-only launch performs setup first so MCP/instructions/hooks exist before the tool
/// starts, resolves the program without a shell, and injects this server's permission route.
struct PreparedTerminalLaunch {
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

fn terminal_launch(
    state: &AppState,
    req: &OpenTerminalReq,
) -> Result<PreparedTerminalLaunch, ApiError> {
    if let Some(command) = &req.command {
        return Ok(PreparedTerminalLaunch {
            command: command.clone(),
            args: req.args.clone(),
            env: Vec::new(),
        });
    }
    let tool = req.connect.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "command is required unless connect names a plugin with an interactive launch",
        )
    })?;
    let root = state.store.root();
    hotsheet_aitools::launch_safety::assert_no_hs1(root)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    hotsheet_plugins::run_setup_in(root, root, Some(tool), false, None, &state.plugin_dirs)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    let plugin = hotsheet_plugins::find_in(tool, &state.plugin_dirs)
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, format!("unknown tool '{tool}'")))?;
    let launch = plugin.manifest.launch.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("plugin '{tool}' does not declare an interactive launch"),
        )
    })?;
    let program = hotsheet_aitools::launch_safety::resolve_program(&launch.program)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    let mut env = vec![("HOTSHEET_SECRET".to_string(), state.secret.clone())];
    if let Ok(url) = state.terminal_server_url.lock()
        && let Some(url) = url.as_ref()
    {
        env.push(("HOTSHEET_SERVER".to_string(), url.clone()));
    }
    Ok(PreparedTerminalLaunch {
        command: program.to_string_lossy().into_owned(),
        args: launch.args.clone(),
        env,
    })
}

/// Register a launched-in-terminal tool as a `Pty` connection on the shared registry and
/// spawn a background task that feeds the terminal's busy/idle into it until the child exits
/// (HS2-4M67VN). The connection id is the terminal id, so `GET /connections` and the terminal
/// surfaces line up.
fn register_terminal_connection(
    state: &AppState,
    id: &str,
    tool: &str,
    term: std::sync::Arc<hotsheet_terminals::Terminal>,
) {
    let registry = state.drive_registry();
    if let Ok(mut r) = registry.lock() {
        r.register(hotsheet_aitools::Connection {
            id: id.to_string(),
            project: state.store.root().display().to_string(),
            tool: tool.to_string(),
            role: hotsheet_aitools::Role::Main,
            transport: hotsheet_aitools::Transport::Pty,
            pid: None,
            started_at_ms: now_ms(),
        });
    }
    let conn_id = id.to_string();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let alive = term.is_alive();
            if let Ok(mut r) = registry.lock() {
                if !alive {
                    r.unregister(&conn_id);
                    break;
                }
                match term.activity() {
                    hotsheet_terminals::Activity::Busy => r.note_activity(&conn_id, now_ms()),
                    hotsheet_terminals::Activity::Idle => r.set_idle(&conn_id),
                }
            } else {
                break;
            }
        }
    });
}

/// The broker-mode counterpart of [`register_terminal_connection`] (HS2-ERT00F item 5): the
/// PTY lives in the broker, so the busy feed **polls the broker over the socket** (a `Read`
/// every 500ms) for the terminal's busy/idle instead of reading an in-process `Terminal`. Ends
/// (and unregisters) when the terminal is gone (NotFound / not alive) or the broker is
/// unreachable.
fn register_broker_terminal_connection(
    state: &AppState,
    id: &str,
    tool: &str,
    broker: terminal_broker::TerminalBroker,
) {
    let registry = state.drive_registry();
    if let Ok(mut r) = registry.lock() {
        r.register(hotsheet_aitools::Connection {
            id: id.to_string(),
            project: state.store.root().display().to_string(),
            tool: tool.to_string(),
            role: hotsheet_aitools::Role::Main,
            transport: hotsheet_aitools::Transport::Pty,
            pid: None,
            started_at_ms: now_ms(),
        });
    }
    let conn_id = id.to_string();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            match broker
                .call(hotsheet_terminals::BrokerRequest::Read {
                    id: conn_id.clone(),
                })
                .await
            {
                Ok(hotsheet_terminals::BrokerResponse::Read { info, .. }) if info.alive => {
                    if let Ok(mut r) = registry.lock() {
                        if info.busy {
                            r.note_activity(&conn_id, now_ms());
                        } else {
                            r.set_idle(&conn_id);
                        }
                    } else {
                        break;
                    }
                }
                // Terminal gone (NotFound / exited) or broker unreachable → stop + unregister.
                _ => {
                    if let Ok(mut r) = registry.lock() {
                        r.unregister(&conn_id);
                    }
                    break;
                }
            }
        }
    });
}

/// `GET /terminals` — the live terminals (id, alive, busy).
async fn list_terminals(State(state): State<AppState>) -> Json<Vec<TerminalInfo>> {
    if let Some(tb) = &state.terminal_broker {
        if let Ok(hotsheet_terminals::BrokerResponse::List { terminals }) =
            tb.call(hotsheet_terminals::BrokerRequest::List).await
        {
            return Json(terminals.into_iter().map(broker_info).collect());
        }
        return Json(Vec::new());
    }
    let infos = state
        .terminals
        .list()
        .into_iter()
        .filter_map(|key| state.terminals.get(&key).map(|t| term_info(&t, &key.1)))
        .collect();
    Json(infos)
}

/// A terminal's current scrollback + state (`GET /terminals/{id}`). The scrollback is what a
/// re-attaching viewer replays; it's returned as lossy UTF-8 text.
#[derive(Serialize)]
struct TerminalRead {
    #[serde(flatten)]
    info: TerminalInfo,
    scrollback: String,
}

async fn read_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TerminalRead>, ApiError> {
    if let Some(tb) = &state.terminal_broker {
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        return match resp {
            hotsheet_terminals::BrokerResponse::Read { info, scrollback } => {
                Ok(Json(TerminalRead {
                    info: broker_info(info),
                    scrollback: String::from_utf8_lossy(&scrollback).into_owned(),
                }))
            }
            other => Err(broker_err(other)),
        };
    }
    let term = state
        .terminals
        .get(&term_key(&state, &id))
        .ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json(TerminalRead {
        info: term_info(&term, &id),
        scrollback: String::from_utf8_lossy(&term.scrollback()).into_owned(),
    }))
}

/// Body for `POST /terminals/{id}/input`.
#[derive(Deserialize)]
struct TerminalInput {
    /// Bytes to write to the PTY (as text — includes any control chars like `\n`).
    data: String,
}

async fn write_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalInput>,
) -> Result<StatusCode, ApiError> {
    if let Some(tb) = &state.terminal_broker {
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Input {
                id: id.clone(),
                data: body.data.into_bytes(),
            })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        return match resp {
            hotsheet_terminals::BrokerResponse::Ok => Ok(StatusCode::NO_CONTENT),
            other => Err(broker_err(other)),
        };
    }
    let term = state
        .terminals
        .get(&term_key(&state, &id))
        .ok_or_else(|| ApiError::not_found(&id))?;
    term.write(body.data.as_bytes())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn kill_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if let Some(tb) = &state.terminal_broker {
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Kill { id: id.clone() })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        return match resp {
            hotsheet_terminals::BrokerResponse::Ok => Ok(StatusCode::NO_CONTENT),
            other => Err(broker_err(other)),
        };
    }
    let killed = state
        .terminals
        .kill(&term_key(&state, &id))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if killed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found(&id))
    }
}

/// `GET /terminals/{id}/attach?secret=…` — the **live** terminal attach (HS2-XTTTMV): a
/// WebSocket that first replays the scrollback (one binary frame), then streams each new PTY
/// output chunk as a binary frame and forwards any binary/text the viewer sends as PTY input.
/// The socket closes when the terminal's child exits or the viewer disconnects.
async fn attach_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> Response {
    if params.secret.as_deref() != Some(state.secret.as_str()) {
        return (StatusCode::UNAUTHORIZED, "missing or invalid secret").into_response();
    }
    // Broker mode: the PTY lives in the detached broker. Confirm the terminal exists (clean
    // 404) before upgrading, then bridge the WebSocket to a streaming broker connection
    // (HS2-ERT00F item 4). The size self-heal on disconnect happens broker-side.
    if let Some(tb) = &state.terminal_broker {
        match tb
            .call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
            .await
        {
            Ok(hotsheet_terminals::BrokerResponse::Read { .. }) => {}
            Ok(hotsheet_terminals::BrokerResponse::NotFound) => {
                return (StatusCode::NOT_FOUND, "no such terminal").into_response();
            }
            Ok(other) => return broker_err(other).into_response(),
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}")).into_response();
            }
        }
        let socket_path = tb.socket.clone();
        return ws.on_upgrade(move |socket| broker_attach_loop(socket, socket_path, id));
    }
    let Some(term) = state.terminals.get(&term_key(&state, &id)) else {
        return (StatusCode::NOT_FOUND, "no such terminal").into_response();
    };
    ws.on_upgrade(move |socket| terminal_attach_loop(socket, term))
}

fn default_true() -> bool {
    true
}

/// An inbound control message on the terminal WS (a Text frame). Today: a **size claim**
/// (HS2-BD7Q74). A Text frame that doesn't parse as control is treated as raw PTY input, so a
/// simple client can type over the same socket (Binary is always input).
#[derive(Deserialize)]
struct TermControl {
    #[serde(default)]
    resize: Option<ResizeClaim>,
}

/// A viewport's leased size claim (HS2-BD7Q74).
#[derive(Deserialize)]
struct ResizeClaim {
    viewer_id: String,
    cols: u16,
    rows: u16,
    #[serde(default)]
    focus: bool,
    #[serde(default = "default_true")]
    visible: bool,
}

/// The size the server chose, pushed to every viewer when it changes.
#[derive(Serialize)]
struct SizeMsg<'a> {
    pty_size: PtySizeMsg,
    driven_by: Option<&'a str>,
}
#[derive(Serialize)]
struct PtySizeMsg {
    cols: u16,
    rows: u16,
}

/// Monotonic-ish wall clock in ms for the size arbiter (real millis; the arbiter is
/// deterministic given it).
fn term_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Drive one attached viewer: replay scrollback, then interleave live output → socket, the
/// arbiter's size decisions → socket, and socket input/claims → PTY, on one task via
/// `select!`. On disconnect the viewport's size claim is dropped so the size self-heals.
async fn terminal_attach_loop(
    mut socket: WebSocket,
    term: std::sync::Arc<hotsheet_terminals::Terminal>,
) {
    use tokio::sync::broadcast::error::RecvError;

    // Subscribe BEFORE snapshotting so no chunk is lost between the snapshot and the stream
    // (a small overlap of already-seen bytes is harmless — the terminal renders it fine).
    let mut rx = term.subscribe();
    let mut size_rx = term.subscribe_size();
    let mut my_viewer: Option<String> = None;

    let snapshot = term.scrollback();
    if !snapshot.is_empty() && socket.send(Message::Binary(snapshot.into())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            output = rx.recv() => match output {
                Ok(chunk) => {
                    if socket.send(Message::Binary(chunk.into())).await.is_err() {
                        break; // viewer went away
                    }
                }
                // Fell behind the fan-out buffer — re-sync from a fresh snapshot.
                Err(RecvError::Lagged(_)) => {
                    let snap = term.scrollback();
                    if socket.send(Message::Binary(snap.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Closed) => break, // terminal ended
            },
            size = size_rx.recv() => match size {
                Ok(d) => {
                    let msg = SizeMsg {
                        pty_size: PtySizeMsg { cols: d.cols, rows: d.rows },
                        driven_by: d.driven_by.as_deref(),
                    };
                    if let Ok(txt) = serde_json::to_string(&msg) {
                        if socket.send(Message::Text(txt.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Err(RecvError::Lagged(_)) => {} // a missed size update self-corrects on the next claim
                Err(RecvError::Closed) => break,
            },
            inbound = socket.recv() => match inbound {
                Some(Ok(Message::Binary(b))) => { let _ = term.write(&b); }
                Some(Ok(Message::Text(t))) => {
                    match serde_json::from_str::<TermControl>(&t) {
                        // A size claim: feed the arbiter (it resizes + broadcasts if the size changed).
                        Ok(TermControl { resize: Some(r) }) => {
                            my_viewer = Some(r.viewer_id.clone());
                            let now = term_now_ms();
                            term.claim_size(
                                hotsheet_terminals::ViewportClaim {
                                    viewer_id: r.viewer_id,
                                    cols: r.cols,
                                    rows: r.rows,
                                    focus: r.focus,
                                    visible: r.visible,
                                    activity_at_ms: now,
                                },
                                now,
                            );
                        }
                        // Not a control message → raw PTY input.
                        _ => { let _ = term.write(t.as_bytes()); }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong handled by axum
                Some(Err(_)) => break,
            },
        }
    }

    // Self-heal: drop this viewport's claim so the PTY size recomputes for the rest.
    if let Some(v) = my_viewer {
        term.drop_viewer(&v, term_now_ms());
    }
}

/// Bridge one attached viewer to a terminal that lives in the **detached broker** (HS2-ERT00F
/// item 4): open a streaming broker connection, then forward broker output/size frames → the
/// WebSocket and the viewer's input/size claims → the broker, on one task via `select!`. The
/// broker replays the scrollback as its first frame(s) and drops the size claim when this
/// connection closes, so the size self-heal holds even across a server restart.
async fn broker_attach_loop(mut socket: WebSocket, broker_socket: std::path::PathBuf, id: String) {
    let mut stream = match hotsheet_terminals::BrokerStream::open(&broker_socket, &id).await {
        Ok(s) => s,
        Err(_) => return, // dropping the socket closes it
    };

    loop {
        tokio::select! {
            frame = stream.next() => match frame {
                Ok(Some(f)) => {
                    use hotsheet_terminals::StreamOut as S;
                    match f {
                        // Scrollback replay + live output both render as binary output frames.
                        S::Scrollback { data } | S::Output { data } => {
                            if socket.send(Message::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        S::Size { cols, rows, driven_by } => {
                            let msg = SizeMsg {
                                pty_size: PtySizeMsg { cols, rows },
                                driven_by: driven_by.as_deref(),
                            };
                            if let Ok(txt) = serde_json::to_string(&msg) {
                                if socket.send(Message::Text(txt.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        // The terminal's gone (or the attach failed) — end the viewer session.
                        S::NotFound | S::Err { .. } => break,
                    }
                }
                Ok(None) | Err(_) => break, // broker closed (terminal ended / broker gone)
            },
            inbound = socket.recv() => match inbound {
                Some(Ok(Message::Binary(b))) => {
                    if stream
                        .send(&hotsheet_terminals::StreamIn::Input { data: b.to_vec() })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Some(Ok(Message::Text(t))) => {
                    let sent = match serde_json::from_str::<TermControl>(&t) {
                        // A size claim forwards as a Resize frame the broker feeds to its arbiter.
                        Ok(TermControl { resize: Some(r) }) => {
                            stream
                                .send(&hotsheet_terminals::StreamIn::Resize {
                                    viewer_id: r.viewer_id,
                                    cols: r.cols,
                                    rows: r.rows,
                                    focus: r.focus,
                                    visible: r.visible,
                                })
                                .await
                        }
                        // Not a control message → raw PTY input.
                        _ => {
                            stream
                                .send(&hotsheet_terminals::StreamIn::Input {
                                    data: t.as_bytes().to_vec(),
                                })
                                .await
                        }
                    };
                    if sent.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong handled by axum
                Some(Err(_)) => break,
            },
        }
    }

    // Dropping `stream` closes the broker connection, so the broker self-heals the size claim;
    // dropping `socket` closes the WebSocket to the viewer.
}

/// Body for `POST /permissions/{id}`: the human's answer.
#[derive(Deserialize)]
struct PermissionAnswer {
    decision: hotsheet_aitools::PermissionDecision,
    /// `once` | `session` | `always` (default `once`).
    #[serde(default = "default_scope")]
    scope: hotsheet_aitools::PermissionScope,
}

fn default_scope() -> hotsheet_aitools::PermissionScope {
    hotsheet_aitools::PermissionScope::Once
}

/// The ack a resolve returns: who it was routed back to + the decision applied.
#[derive(Serialize)]
struct PermissionResolved {
    connection: String,
    decision: hotsheet_aitools::PermissionDecision,
    /// Whether an `Always` rule was persisted durably.
    persisted: bool,
}

/// `POST /permissions/{id}` `{decision, scope}` — answer a pending request. Wakes the
/// blocked tool, and on `always` persists the rule so the answer survives a restart. 404
/// if the id isn't pending (already answered / never existed).
async fn resolve_permission(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(body): Json<PermissionAnswer>,
) -> Result<Json<PermissionResolved>, ApiError> {
    let event_decision = match body.decision {
        hotsheet_aitools::PermissionDecision::Allow => "allow",
        hotsheet_aitools::PermissionDecision::Deny => "deny",
    };
    let event_scope = match body.scope {
        hotsheet_aitools::PermissionScope::Once => "once",
        hotsheet_aitools::PermissionScope::Session => "session",
        hotsheet_aitools::PermissionScope::Always => "always",
    };
    let resolved = state
        .permissions
        .resolve(id, body.decision, body.scope)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no pending request {id}")))?;
    // Persist an `Always` rule so the remembered answer survives a restart.
    let mut persisted = false;
    if let (Some(rule), Some(path)) = (&resolved.persisted_rule, &state.permission_rules_path) {
        match hotsheet_aitools::append_permission_rule(path, rule) {
            Ok(()) => persisted = true,
            Err(e) => eprintln!("failed to persist permission rule: {e}"),
        }
    }
    // Resolution can happen through another client or transport. Publish a replayable
    // nudge so every notification inbox reconciles the now-absent request into history.
    state.emit(ChangeEvent {
        store: String::new(),
        kind: "permission_resolved".to_string(),
        id: id.to_string(),
        slug: String::new(),
        message: Some(format!("{event_decision}:{event_scope}")),
        activity: None,
        assignment: None,
    });
    Ok(Json(PermissionResolved {
        connection: resolved.connection,
        decision: resolved.decision,
        persisted,
    }))
}

// ---- cross-store copy / move (HS2-60 / HS2-S4H2AM) -------------------------------

/// Body for copy: `to` names the hosted destination store (its URL id).
#[derive(Deserialize)]
struct CopyBody {
    to: String,
}

/// Body for move: destination store + the explicit `confirm` acknowledging that git
/// history in the source never forgets (the retention/exposure caveat, `docs/02` §2.13).
#[derive(Deserialize)]
struct MoveBody {
    to: String,
    #[serde(default)]
    confirm: bool,
}

/// A copy result: the new ticket + the destination store it now lives in.
#[derive(Serialize)]
struct CopyResult {
    /// URL id of the destination store.
    store: String,
    #[serde(flatten)]
    ticket: ApiTicket,
}

/// A move result: the live ticket (now in `store`), the source store it left, and the
/// tombstone slug left behind in the source.
#[derive(Serialize)]
struct MoveResult {
    /// URL id of the destination store (where the live ticket now is).
    store: String,
    /// URL id of the source store (which keeps a `moved` tombstone).
    source_store: String,
    /// Slug of the tombstone left in the source store.
    tombstone: String,
    #[serde(flatten)]
    ticket: ApiTicket,
}

/// `POST /tickets/{id}/copy` `{to:<store_id>}` — copy a default-store ticket into another
/// hosted store as a **new** ticket (new ULID, `copied_from` provenance). Source untouched.
async fn copy_ticket_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CopyBody>,
) -> Result<(StatusCode, Json<CopyResult>), ApiError> {
    let src = state.default_entry();
    let dest = scoped_entry(&state, &body.to)?;
    let ticket = ops::resolve(&src.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let new = ops::copy_ticket(&src.store, &dest.store, &ticket.id, Ulid::new(), now())?;
    state.changed_in(&dest, "created", &new);
    Ok((
        StatusCode::CREATED,
        Json(CopyResult {
            store: multistore::store_url_id(&dest.store),
            ticket: api_ticket(&dest, &new)?,
        }),
    ))
}

/// `POST /tickets/{id}/move` `{to:<store_id>, confirm:true}` — move a default-store ticket
/// to another hosted store, keeping the same ULID and leaving a `moved` tombstone behind.
/// Requires `confirm:true` (the git-retention caveat); without it, 400.
async fn move_ticket_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<MoveResult>, ApiError> {
    if !body.confirm {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "move requires confirm=true: the source store's git history keeps the ticket \
             (and any attachments) even after the move — see docs/02 §2.13",
        ));
    }
    let src = state.default_entry();
    let dest = scoped_entry(&state, &body.to)?;
    let ticket = ops::resolve(&src.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    // Record the destination's canonical root as `moved_to_store` — the identity the
    // `StoreRegistry` follows when resolving the ULID to its live instance.
    let dest_id = StoreRegistry::store_id(&dest.store);
    let outcome = ops::move_ticket(&src.store, &dest.store, &ticket.id, &dest_id, now())?;
    state.changed_in(&dest, "created", &outcome.moved);
    state.changed_in(&src, "moved", &outcome.tombstone);
    Ok(Json(MoveResult {
        store: multistore::store_url_id(&dest.store),
        source_store: multistore::store_url_id(&src.store),
        tombstone: outcome.tombstone.slug.clone(),
        ticket: api_ticket(&dest, &outcome.moved)?,
    }))
}

// ---- store-scoped write routes (multi-store, HS2-87) -----------------------------

/// Look up a hosted store by URL id, 404 if not hosted.
fn scoped_entry(state: &AppState, store_id: &str) -> Result<StoreEntry, ApiError> {
    state
        .host
        .get(store_id)
        .ok_or_else(|| ApiError::not_found(store_id))
}

async fn create_store_ticket(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    let ticket = do_create(&state, &entry, req)?;
    Ok((StatusCode::CREATED, Json(ticket)))
}

async fn update_store_ticket(
    State(state): State<AppState>,
    Path((store_id, id)): Path<(String, String)>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    Ok(Json(do_update(&state, &entry, &id, req)?))
}

async fn close_store_ticket(
    State(state): State<AppState>,
    Path((store_id, id)): Path<(String, String)>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    Ok(Json(do_close(&state, &entry, &id, req)?))
}

async fn assign_store_ticket(
    State(state): State<AppState>,
    Path((store_id, id)): Path<(String, String)>,
    Json(req): Json<AssignReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    Ok(Json(do_assign(&state, &entry, &id, req)?))
}

async fn get_store_ticket(
    State(state): State<AppState>,
    Path((store_id, id)): Path<(String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json(api_ticket(&entry, &ticket)?))
}

/// A cross-store resolve result: the ticket + which hosted store it lives in.
#[derive(Serialize)]
struct ResolvedTicket {
    /// URL id of the store the live instance lives in.
    store: String,
    #[serde(flatten)]
    ticket: ApiTicket,
}

/// `GET /resolve/{ulid}` — resolve a **global ULID** to its single live instance across
/// every hosted store, following `moved_to_store` tombstones (HS2-87 / HS2-S4H2AM). By
/// ULID (not slug): slugs are per-store-prefix, but a ULID is global. 404 if unhosted.
async fn resolve_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let ulid = Ulid::from_string(&id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, format!("not a ULID: {id}")))?;
    let (store, ticket) = state
        .host
        .resolve(&ulid)?
        .ok_or_else(|| ApiError::not_found(&id))?;
    let entry = scoped_entry(&state, &store)?;
    Ok(Json(ResolvedTicket {
        store,
        ticket: api_ticket(&entry, &ticket)?,
    }))
}

/// Prepare the served project for an AI tool — the same core setup the CLI runs headless
/// (`POST /setup/<tool>`, HS2-91). The server serves one store, so the project dir is the
/// store root; a single named tool doesn't need the enabled-plugin filter.
async fn setup_tool(
    State(state): State<AppState>,
    Path(tool): Path<String>,
) -> Result<Json<Vec<hotsheet_plugins::SetupReport>>, ApiError> {
    let store = state.store.root().to_path_buf();
    let reports = hotsheet_plugins::run_setup_in(
        &store,
        &store,
        Some(&tool),
        false,
        None,
        &state.plugin_dirs,
    )
    .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(reports))
}

async fn ws_sync(
    State(state): State<AppState>,
    Query(params): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> Response {
    if params.secret.as_deref() != Some(state.secret.as_str()) {
        return (StatusCode::UNAUTHORIZED, "missing or invalid secret").into_response();
    }
    let rx = state.events.subscribe();
    ws.on_upgrade(move |socket| ws_loop(socket, rx))
}

async fn ws_loop(mut socket: WebSocket, mut rx: broadcast::Receiver<ChangeEvent>) {
    while let Ok(event) = rx.recv().await {
        let text = match serde_json::to_string(&event) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if socket.send(Message::Text(text.into())).await.is_err() {
            break;
        }
    }
}

/// The default and max time a long-poll request will wait for a new event.
const POLL_DEFAULT_MS: u64 = 25_000;
const POLL_MAX_MS: u64 = 55_000;

#[derive(Debug, Deserialize)]
struct PollParams {
    secret: Option<String>,
    /// Return events with `seq > since`. Omit to just fetch the current cursor (no backlog),
    /// the way a fresh WebSocket only sees future events.
    since: Option<u64>,
    /// How long to block for the next event when none are newer than `since` (ms, capped).
    timeout_ms: Option<u64>,
}

/// One long-poll response: the new cursor, any events since the requested one, and whether
/// the caller fell so far behind the ring that events were lost (→ re-sync via a full list).
#[derive(Debug, Serialize)]
struct PollResponse {
    cursor: u64,
    events: Vec<ChangeEvent>,
    overflow: bool,
}

/// `GET /ws/poll?secret=…&since=<seq>&timeout_ms=<n>` — the long-poll fallback to `/ws/sync`
/// (HS2-P3P3CC). Authentication accepts either the legacy query secret or the standard
/// `X-Hotsheet-Secret` header. Returns immediately with any events after `since`; otherwise
/// subscribes and waits up to `timeout_ms` for the next one, returning an empty list (with
/// the current cursor) on timeout. The client re-polls with the returned `cursor`.
async fn poll_events(
    State(state): State<AppState>,
    Query(params): Query<PollParams>,
    headers: HeaderMap,
) -> Response {
    let header_secret = headers
        .get("x-hotsheet-secret")
        .and_then(|value| value.to_str().ok());
    if params.secret.as_deref() != Some(state.secret.as_str())
        && header_secret != Some(state.secret.as_str())
    {
        return (StatusCode::UNAUTHORIZED, "missing or invalid secret").into_response();
    }
    // No `since` → hand back the current cursor with no backlog (initial handshake).
    let Some(since) = params.since else {
        return Json(PollResponse {
            cursor: state.event_cursor(),
            events: Vec::new(),
            overflow: false,
        })
        .into_response();
    };

    // Subscribe BEFORE reading the log, so an event emitted in the gap isn't missed: it
    // either lands in the log we read, or wakes the receiver below.
    let mut rx = state.events.subscribe();
    let (backlog, overflow) = match state.event_log.lock() {
        Ok(log) => log.since(since),
        Err(_) => (Vec::new(), false),
    };
    if overflow || !backlog.is_empty() {
        return Json(PollResponse {
            cursor: state.event_cursor(),
            events: backlog,
            overflow,
        })
        .into_response();
    }

    // Caught up — wait for the next event (or time out with an empty list).
    let wait = Duration::from_millis(
        params
            .timeout_ms
            .unwrap_or(POLL_DEFAULT_MS)
            .min(POLL_MAX_MS),
    );
    let events = match tokio::time::timeout(wait, rx.recv()).await {
        Ok(Ok(ev)) => vec![ev],
        // Lagged (fell behind the broadcast buffer) → signal overflow so the client re-syncs.
        Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
            return Json(PollResponse {
                cursor: state.event_cursor(),
                events: Vec::new(),
                overflow: true,
            })
            .into_response();
        }
        Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => Vec::new(),
    };
    Json(PollResponse {
        cursor: state.event_cursor(),
        events,
        overflow: false,
    })
    .into_response()
}

// ---- request / response DTOs -----------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
struct ListParams {
    status: Option<String>,
    priority: Option<String>,
    category: Option<String>,
    /// Comma-separated; a ticket must carry all of them.
    tags: Option<String>,
    text: Option<String>,
    up_next: Option<bool>,
    open: Option<bool>,
    /// Filter by close reason (completed|not_planned|duplicate|obsolete).
    close_reason: Option<String>,
    /// `true` = only closed tickets; `false` = only tickets with no close reason.
    closed: Option<bool>,
    /// Only tickets assigned to this person (git email).
    assignee: Option<String>,
    /// Only tickets with a review request for this person (git email).
    review_requested: Option<String>,
    /// Only tickets whose review was requested by this person (git email).
    review_by: Option<String>,
    /// `true` = only claimed tickets; `false` = only unclaimed.
    claimed: Option<bool>,
    /// `true` = only blocked tickets; `false` = only unblocked (HS2-T84F9F).
    blocked: Option<bool>,
    /// ISO-8601 `created_at` / `updated_at` range bounds (inclusive).
    created_after: Option<String>,
    created_before: Option<String>,
    updated_after: Option<String>,
    updated_before: Option<String>,
    sort: Option<String>,
    limit: Option<usize>,
    /// Keyset cursor (a ULID): return rows strictly after this one in `sort` order (HS2-TCDTCH).
    page_after: Option<String>,
    /// Omit the Markdown body from each row (default true). `compact=false` keeps it.
    compact: Option<bool>,
    /// Comma-separated field allow-list for a leaner-than-compact projection (HS2-GY3GWT):
    /// each row keeps only these keys (plus `slug`). Empty/absent = the full compact row.
    fields: Option<String>,
}

/// Parse the `fields=` allow-list (comma-separated, empties dropped).
fn parse_fields(fields: &Option<String>) -> Vec<String> {
    fields
        .as_deref()
        .map(|f| {
            f.split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Serialize compact rows to a JSON array, applying the `fields` projection if any.
fn rows_to_json(rows: Vec<TicketRow>, fields: &[String]) -> serde_json::Value {
    let mut vals: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
        .collect();
    hotsheet_ticketing::wire::project_fields(&mut vals, fields);
    serde_json::Value::Array(vals)
}

impl ListParams {
    /// Build the `TicketQuery`, resolving the `me` sentinel in person filters against the
    /// store's git identity (HS2-TCDTCH). `store_root` is the store the query runs against.
    fn into_query(self, store_root: &FsPath) -> Result<TicketQuery, ApiError> {
        let sort = match self.sort {
            Some(s) => s
                .parse::<SortKey>()
                .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?,
            None => SortKey::default(),
        };
        // `me` → the store's git user.email; a `me` that can't be resolved is an error, not a
        // silent match-everyone (docs/10 §10.3).
        let resolve_person = |v: Option<String>| -> Result<Option<String>, ApiError> {
            match v {
                None => Ok(None),
                Some(raw) if raw.eq_ignore_ascii_case(hotsheet_ticketing::ME) => {
                    hotsheet_ticketing::current_user_email(store_root)
                        .map(Some)
                        .ok_or_else(|| {
                            ApiError::new(
                                StatusCode::BAD_REQUEST,
                                "cannot resolve 'me': no git user.email configured",
                            )
                        })
                }
                Some(raw) => Ok(Some(raw)),
            }
        };
        let page_after = match self.page_after {
            Some(s) => Some(Ulid::from_string(&s).map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid page_after cursor (not a ULID)",
                )
            })?),
            None => None,
        };
        Ok(TicketQuery {
            status: opt_parse(self.status.as_deref())?,
            priority: opt_parse(self.priority.as_deref())?,
            category: self.category,
            tags: self
                .tags
                .map(|t| {
                    t.split(',')
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            text: self.text,
            up_next_only: self.up_next.unwrap_or(false),
            open_only: self.open.unwrap_or(false),
            close_reason: opt_parse(self.close_reason.as_deref())?,
            closed: self.closed,
            assignee: resolve_person(self.assignee)?,
            review_requested: resolve_person(self.review_requested)?,
            review_by: resolve_person(self.review_by)?,
            claimed: self.claimed,
            blocked: self.blocked,
            created_after: self.created_after,
            created_before: self.created_before,
            updated_after: self.updated_after,
            updated_before: self.updated_before,
            sort,
            limit: self.limit,
            page_after,
        })
    }
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    title: String,
    category: Option<String>,
    priority: Option<String>,
    details: Option<String>,
    tags: Option<Vec<String>>,
    up_next: Option<bool>,
    /// Blocker tickets (slug or ULID), resolved to ULIDs on create.
    blocked_by: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct UpdateReq {
    expected_token: Option<String>,
    title: Option<String>,
    details: Option<String>,
    category: Option<String>,
    priority: Option<String>,
    status: Option<String>,
    tags: Option<Vec<String>>,
    up_next: Option<bool>,
    /// Replace the blocker set (slug or ULID); `[]` clears it, absent leaves it.
    blocked_by: Option<Vec<String>>,
    /// Optional note to append alongside the field update.
    note: Option<String>,
    /// Existing note ULID to edit; absent appends a new note.
    note_id: Option<String>,
    /// Kind of the appended note; defaults to regular for older clients.
    note_kind: Option<NoteKind>,
}

/// `POST /batch` body: apply the same field update to every listed ticket (HS2-86).
#[derive(Debug, Deserialize)]
struct BatchReq {
    /// Tickets to update (slug or ULID).
    ids: Vec<String>,
    /// The same update applied to each — the `UpdateReq` fields, flattened in.
    #[serde(flatten)]
    update: UpdateReq,
}

/// The per-ticket outcome of a batch update.
#[derive(Debug, Serialize)]
struct BatchResult {
    /// Slugs of the tickets updated.
    updated: Vec<String>,
    /// Tickets that failed, with why (a bad batch never aborts the rest).
    errors: Vec<BatchError>,
}

#[derive(Debug, Serialize)]
struct BatchError {
    id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct CloseReq {
    reason: String,
    duplicate_of: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaimReq {
    worker: Option<String>,
    label: Option<String>,
    lease_minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ReleaseReq {
    worker: Option<String>,
    force: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RenewReq {
    worker: Option<String>,
    lease_minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct WsParams {
    secret: Option<String>,
}

// The full-ticket + note wire DTOs (`ApiTicket`/`ApiNote`) and their `From<&Ticket>`
// mapping live in `hotsheet_ticketing::wire` and are re-exported at the top of this
// module — one definition, shared with the MCP shim (wire SSOT, `docs/04` §4.2).

// ---- helpers ---------------------------------------------------------------------

fn now() -> Timestamp {
    Timestamp::from_datetime(OffsetDateTime::now_utc())
}

/// Parse an enum value from its wire string via serde (so it matches serialization).
fn opt_parse<T: serde::de::DeserializeOwned>(s: Option<&str>) -> Result<Option<T>, ApiError> {
    match s {
        None => Ok(None),
        Some(s) => serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map(Some)
            .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, format!("invalid value '{s}'"))),
    }
}

// ---- errors ----------------------------------------------------------------------

/// An API error rendered as `{ "error": "…" }` with a status code.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    fn not_found(id: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, format!("no ticket matching '{id}'"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        let status = match &e {
            StoreError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                StatusCode::NOT_FOUND
            }
            StoreError::NotAStore(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError::new(status, e.to_string())
    }
}

impl From<IndexError> for ApiError {
    fn from(e: IndexError) -> Self {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

impl From<OpError> for ApiError {
    fn from(e: OpError) -> Self {
        match e {
            OpError::Store(s) => ApiError::from(s),
            other @ (OpError::WrongWorker { .. }
            | OpError::NotClaimed(_)
            | OpError::NotWorkingRequiresCompleted(_)) => {
                ApiError::new(StatusCode::CONFLICT, other.to_string())
            }
            other @ (OpError::DuplicateNeedsTarget
            | OpError::SelfBlock(_)
            | OpError::EmptyNotWorkingReport) => {
                ApiError::new(StatusCode::BAD_REQUEST, other.to_string())
            }
            other @ OpError::UnknownTicket(_) => {
                ApiError::new(StatusCode::NOT_FOUND, other.to_string())
            }
        }
    }
}

// ---- filesystem watcher (HS2-6) --------------------------------------------------

/// Keeps the watcher alive; dropping it stops watching.
pub struct WatchHandle {
    _watcher: HeldWatcher,
}

enum HeldWatcher {
    Recommended(notify::RecommendedWatcher),
    Poll(notify::PollWatcher),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatcherBackend {
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
struct WatchTarget {
    entry: StoreEntry,
    store_id: String,
    events: broadcast::Sender<ChangeEvent>,
    event_log: Arc<Mutex<EventLog>>,
    checkout_registry: hotsheet_ticketing::checkouts::CheckoutRegistry,
}

/// Watch the **default** store (back-compat entry point used by the server binary).
pub fn spawn_watcher(state: AppState) -> anyhow::Result<WatchHandle> {
    let target = WatchTarget {
        entry: state.default_entry(),
        store_id: multistore::store_url_id(&state.store),
        events: state.events.clone(),
        event_log: state.event_log.clone(),
        checkout_registry: state.checkout_registry.clone(),
    };
    spawn_watcher_for(target, WatcherBackend::Recommended)
}

/// FSEvents can stop delivering changes when a process creates a second watcher for a
/// sibling temporary/store directory. Dynamically registered stores therefore use the
/// deterministic polling backend on macOS; other platforms retain their native backend.
fn registered_watcher_backend() -> WatcherBackend {
    if cfg!(target_os = "macos") {
        WatcherBackend::Poll
    } else {
        WatcherBackend::Recommended
    }
}

/// Whether two paths point at the same store root (canonicalized; lexical fallback).
fn same_path(a: &FsPath, b: &FsPath) -> bool {
    let canon = |p: &FsPath| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// Watch one store (any hosted store). The returned [`WatchHandle`] must be kept alive
/// for the watcher to run.
fn spawn_watcher_for(target: WatchTarget, backend: WatcherBackend) -> anyhow::Result<WatchHandle> {
    use notify::{RecursiveMode, Watcher};

    let tickets_dir = target.entry.store.root().join("tickets");
    std::fs::create_dir_all(&tickets_dir)?;

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match backend {
        WatcherBackend::Recommended => {
            HeldWatcher::Recommended(notify::recommended_watcher(move |res| {
                let _ = tx.send(res);
            })?)
        }
        WatcherBackend::Poll => HeldWatcher::Poll(notify::PollWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default().with_poll_interval(Duration::from_millis(250)),
        )?),
    };
    match &mut watcher {
        HeldWatcher::Recommended(watcher) => {
            watcher.watch(&tickets_dir, RecursiveMode::Recursive)?
        }
        HeldWatcher::Poll(watcher) => watcher.watch(&tickets_dir, RecursiveMode::Recursive)?,
    }

    std::thread::spawn(move || watch_loop(rx, target));
    Ok(WatchHandle { _watcher: watcher })
}

fn watch_loop(rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>, target: WatchTarget) {
    use std::time::Duration;

    while let Ok(first) = rx.recv() {
        let mut paths = event_paths(first);
        // Debounce a burst (editor save, git checkout touching many files).
        while let Ok(next) = rx.recv_timeout(Duration::from_millis(150)) {
            paths.extend(event_paths(next));
        }
        paths.sort();
        paths.dedup();
        if paths.is_empty() {
            continue;
        }
        for path in &paths {
            handle_path_change(&target, path);
        }
        // Refresh each checkout-local projection that consumes this store. The store is
        // syncable authority; worklists remain local per checkout and may aggregate stores.
        if let Ok(checkouts) = target.checkout_registry.list() {
            for checkout in checkouts {
                if checkout
                    .stores
                    .iter()
                    .any(|root| same_path(FsPath::new(root), target.entry.store.root()))
                {
                    if let Err(e) = hotsheet_ticketing::worklist::regenerate_checkout(&checkout) {
                        eprintln!("worklist regenerate failed for {}: {e}", checkout.root);
                    }
                }
            }
        }
    }
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

fn handle_path_change(target: &WatchTarget, path: &FsPath) {
    // The filename stem is the ticket ULID.
    let Some(id) = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| Ulid::from_string(s).ok())
    else {
        return;
    };
    let index = &target.entry.index;
    let emit = |kind: &str, id: String, slug: String| {
        emit_change(
            &target.event_log,
            &target.events,
            ChangeEvent {
                store: target.store_id.clone(),
                kind: kind.to_string(),
                id,
                slug,
                message: None,
                activity: None,
                assignment: None,
            },
        );
    };

    if !path.exists() {
        if let Ok(index) = index.lock() {
            let _ = index.delete(&id);
        }
        emit("deleted", id.to_string(), String::new());
        return;
    }

    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let hash = hash_bytes(&bytes);

    // Unchanged since we last indexed it (incl. the server's own write) → skip.
    let already = index
        .lock()
        .ok()
        .and_then(|index| index.content_hash(&id).ok().flatten());
    if already.as_deref() == Some(hash.as_str()) {
        return;
    }

    let Ok(ticket) = parse_file(&String::from_utf8_lossy(&bytes)) else {
        return;
    };
    if let Ok(index) = index.lock() {
        let _ = index.upsert(&ticket, &path.display().to_string(), &hash);
    }
    emit("changed", ticket.id.to_string(), ticket.slug.clone());
}

#[cfg(test)]
mod watcher_tests {
    use super::{WatcherBackend, expand_ticket_files, registered_watcher_backend};

    #[test]
    fn registered_store_watcher_avoids_a_second_fsevents_stream() {
        let expected = if cfg!(target_os = "macos") {
            WatcherBackend::Poll
        } else {
            WatcherBackend::Recommended
        };
        assert_eq!(registered_watcher_backend(), expected);
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
}
