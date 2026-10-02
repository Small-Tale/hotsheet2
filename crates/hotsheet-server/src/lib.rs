//! Hot Sheet 2 server: a thin HTTP + WebSocket layer over the shared engine
//! (`hotsheet-ticketing::ops`), the single authority every GUI talks to
//! (`docs/04-core-server-cli.md` §4.3). v1 is loopback + shared-secret (Tier 0).
//! Reads go through the SQLite/FTS index (HS2-5); a filesystem watcher (HS2-6) keeps
//! it fresh and broadcasts change events, so a CLI/git edit shows up live. Terminals
//! (HS2-10) and the detached lifecycle (HS2-59) are separate.

mod ai_tool_discovery;
pub mod client_drive;
pub mod code_review;
pub mod commands;
mod custom_views;
pub mod dist_work_loop;
pub mod github_app_config;
mod health_scan;
pub mod lifecycle;
pub mod media;
pub mod multistore;
pub mod notifications;
mod presence;
pub mod repository_browser;
pub mod source_revision;
pub mod sync_loop;
pub mod terminal_broker;
pub mod tls;
pub mod tts;
pub mod turn_stream;

use std::collections::HashSet;
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use multistore::{StoreEntry, StoreHost, StoreInfo};

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use hotsheet_index::{Index, IndexError, TicketRow, TicketSummary, hash_bytes};
use hotsheet_model::{
    CloseReason, Confidence, ConfidenceError, NoteKind, ReviewKind, ReviewRequest, Status, Ticket,
    Timestamp, Ulid, parse_file, to_file_string,
};
use hotsheet_ticketing::checkout_order::{AfterKey, MergeKey};
use hotsheet_ticketing::checkout_page;
use hotsheet_ticketing::wire::ApiAttachment;
use hotsheet_ticketing::{
    FsStore, GitProvider, KeyRegistry, MutationContext, NewTicket, NotWorkingReport, OpError,
    OsKeychain, ProjectTicketRef, ProviderConfigRegistry, ProviderConnection, ProviderDraft,
    ProviderEvidence, ProviderPatch, ProviderRegistry, STORE_METADATA_FILE, Settings, SortKey,
    StoreError, StoreRegistry, TicketPatch, TicketQuery, TicketRef, auto_context, copy_between,
    move_between, ops,
};
// Wire DTOs are defined once in the engine crate (wire SSOT); re-export for callers.
pub use hotsheet_ticketing::{ApiNote, ApiTicket};

#[cfg(test)]
mod request_performance_tests;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::{Notify, broadcast};

/// Attachment uploads may contain screenshots, recordings, and other binary evidence.
/// Keep this route-specific so ordinary JSON endpoints retain Axum's conservative default.
pub const MAX_ATTACHMENT_BODY_BYTES: usize = 100 * 1024 * 1024;
/// Browser-generated JPEG posters are small even for large source videos.
pub const MAX_VIDEO_POSTER_BODY_BYTES: usize = 5 * 1024 * 1024;

/// Shared server state (cheaply cloned into each handler).
#[derive(Clone)]
pub struct AppState {
    store: FsStore,
    secret: String,
    events: broadcast::Sender<ChangeEvent>,
    index: Arc<Mutex<Index>>,
    /// The primary store's stat-validated corrupt-ticket cache (HS2-KYSBT2), shared with
    /// its hosted entry so the unprefixed and checkout routes see one cache.
    corrupt: Arc<Mutex<hotsheet_ticketing::CorruptTicketCache>>,
    /// A bounded, sequenced log of recent [`ChangeEvent`]s backing the **long-poll**
    /// fallback (`GET /ws/poll`) for clients that can't hold a WebSocket (HS2-P3P3CC). The
    /// live push over `/ws/sync` is the primary transport; this replays "everything since
    /// cursor N" over plain HTTP.
    event_log: Arc<Mutex<EventLog>>,
    /// Every store this machine server hosts (HS2-87). The primary `store` is registered
    /// here as the default entry; additional stores are added via `POST /stores`.
    host: StoreHost,
    injected_providers: ProviderRegistry,
    /// Keeps the fs-watchers of `POST /stores`-registered stores alive, by store id (the
    /// default store's watcher is held by the server binary). Removing one stops it.
    watchers: Arc<Mutex<std::collections::HashMap<String, WatchHandle>>>,
    /// Which checkouts some client has open, from live change-stream leases (HS2-ARJ9J1).
    presence: presence::Presence,
    /// Stores hosted because a project opened them: the only ones unhosted again once no open
    /// checkout references them. The default and startup-configured stores stay hosted.
    project_hosted: Arc<Mutex<std::collections::HashSet<String>>>,
    /// When the next unhost sweep is due, and the wake-up for an earlier request.
    unhost_sweep: Arc<UnhostSweep>,
    /// One bounded monitor per open code checkout. It emits only invalidation events;
    /// clients obtain the authoritative status through the existing endpoint.
    repository_watchers: Arc<Mutex<std::collections::HashMap<String, RepositoryWatchHandle>>>,
    /// Server-owned GitHub device flows. Browser clients see only user codes and terminal
    /// states; OAuth device/access/refresh tokens never cross this boundary.
    github_auth_sessions: Arc<Mutex<std::collections::HashMap<String, Arc<GitHubAuthSession>>>>,
    /// Whether a `POST /stores`-registered store gets a **file-backed** index
    /// (`${HOTSHEET_HOME}/index/<id>.v<schema>.sqlite`, persists + restores) or an in-memory one.
    /// Off by default so tests stay hermetic (they never touch the machine home); the
    /// server binary turns it on for a real run.
    persist_indexes: bool,
    /// The machine server's coordinates, set after bind in a real run (HS2-87, topology A):
    /// when present, every hosted store gets a per-store discovery instance file pointing
    /// here, so `lifecycle::find_instance(storeX)` resolves to this one machine server for
    /// each project it hosts. `None` in tests (they never write under the machine home).
    instance: Arc<Mutex<Option<InstanceMeta>>>,
    /// Keeps the per-store instance-file guards alive, by store root; they remove their files
    /// on shutdown or when the store is unhosted.
    instance_guards: Arc<Mutex<std::collections::HashMap<String, lifecycle::InstanceGuard>>>,
    /// Per-hosted-store index-writer locks (HS2-AYCA1W). The primary `store`'s lock is held
    /// by the server binary (main.rs); this holds one for every *additional* hosted store, so
    /// no second machine server double-writes a registered store's index. Real-run only (a
    /// held lock touches the machine home) — acquired in `register_store_instance`.
    writer_locks: Arc<Mutex<std::collections::HashMap<String, lifecycle::WriterLock>>>,
    /// A "kick" to the background sync loop (HS2-731C2X): a server write signals it so local
    /// changes push promptly rather than waiting for the next interval. `None` until the
    /// loop is spawned (tests don't run it).
    sync_kick: Arc<Mutex<Option<std::sync::mpsc::Sender<()>>>>,
    /// Recently indexed server writes, used to distinguish their filesystem echo from
    /// an external edit that happens to match the current index hash.
    local_write_hashes: LocalWriteHashes,
    /// The live permission bridge (HS2-9R9YZW): a driven tool blocks on it; a client answers
    /// over `GET/POST /permissions`. A `permission_asked` nudge rides the event bus so
    /// clients know to fetch + answer. Empty (headless auto-nothing) until seeded.
    permissions: Arc<hotsheet_aitools::SharedPermissionBridge>,
    /// Where `Always` allow-rules persist (a store-local JSON file), so remembered answers
    /// survive a restart. `None` in tests (they don't touch disk for rules).
    permission_rule_paths: Arc<Mutex<std::collections::HashMap<String, std::path::PathBuf>>>,
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
    /// Runtime AI model catalogs keyed by plugin id and installed runtime version, plus a
    /// short-lived memo of the last full discovery (HS2-QV8B7R). The manifest remains the
    /// fallback for unsupported or unavailable capabilities.
    model_catalogs: Arc<Mutex<ai_tool_discovery::AiToolDiscoveryCache>>,
    /// Bumped by anything that changes tool installation or plugin state; a discovery memo
    /// scanned under an older generation is never served (HS2-QV8B7R).
    ai_tool_generation: Arc<AtomicU64>,
    /// Memo lifetime and scan behind AI-tool discovery; injectable for hermetic tests
    /// (HS2-BK350W).
    ai_tool_discovery: Arc<ai_tool_discovery::AiToolDiscoveryConfig>,
    /// Checkout ids with a background setup refresh in flight; repeated opens coalesce.
    setup_refreshes: Arc<Mutex<std::collections::HashSet<String>>>,
    /// Public URL injected into manifest-launched terminal tools for permission route-back.
    terminal_server_url: Arc<Mutex<Option<String>>>,
    /// Machine-local checkout discovery. Checkout ids identify working directories and
    /// are intentionally separate from store ids and server authentication tokens.
    checkout_registry: hotsheet_ticketing::checkouts::CheckoutRegistry,
    /// The machine-local Hot Sheet home (`${HOTSHEET_HOME:-~/.hotsheet2}`), resolved once at
    /// construction. Every machine-local path this state reads or writes (instance files,
    /// index-writer locks, file-backed indexes, `stores.json`, key metadata) derives from it,
    /// so tests inject a directory with [`AppState::with_machine_home`] instead of mutating
    /// the process-global environment (HS2-NYZ3PS).
    machine_home: Arc<std::path::PathBuf>,
    /// Machine-local cache root (video posters), resolved once at construction and
    /// injectable for hermetic tests (HS2-FQEESP).
    cache_dir: Arc<std::path::PathBuf>,
    commands: commands::CommandManager,
    notifications: notifications::NotificationHub,
    tts: tts::TtsProviders,
    source_revision: source_revision::SourceRevisionMonitor,
    /// Candidate windows for the built-in deterministic local adapter. Project-attributed
    /// events are keyed by checkout and re-read that checkout's machine-local settings on
    /// every event; unattributed legacy events retain store-only compatibility behavior.
    activity_distillation:
        Arc<Mutex<std::collections::HashMap<String, hotsheet_ticketing::DistillationPipeline>>>,
    /// Per-session admission state keeps a noisy tool from flooding disk or live clients.
    activity_volume: Arc<Mutex<hotsheet_ticketing::ActivityVolumeGuard>>,
    /// Human/client-owned AI connections, separate from the autonomous work queue.
    client_drives: client_drive::ClientDriveManager,
    /// Coordinates client-requested upgrades. Mutation admission closes atomically before
    /// quiescence is checked, so no new write or process launch can race a safe restart.
    lifecycle: Arc<LifecycleControl>,
    /// Single-flight, bounded primary-store listing behind `GET /health` (HS2-9PPDR1).
    health_scan: Arc<health_scan::HealthScan>,
}

type LocalWriteKey = (String, String, String);
type LocalWriteHashes = Arc<Mutex<std::collections::HashMap<LocalWriteKey, std::time::Instant>>>;

#[derive(Default)]
struct LifecycleControl {
    quiescing: AtomicBool,
    active_mutations: AtomicUsize,
    active_background: AtomicUsize,
    shutdown: Notify,
    /// Set once the process has begun its bounded shutdown drain (HS2-W1KJR4). Long waits
    /// (`/permissions/ask`, `/ws/poll`) end promptly instead of holding the drain open.
    stopping: AtomicBool,
    stopping_changed: Notify,
}

#[derive(Debug, Clone, Serialize)]
struct QuiescenceBlocker {
    kind: &'static str,
    count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct QuiescenceReport {
    quiescent: bool,
    blockers: Vec<QuiescenceBlocker>,
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
        let machine_home = hotsheet_plugins::hotsheet_home();
        let (events, _) = broadcast::channel(256);
        let index = Arc::new(Mutex::new(index));
        let corrupt = Arc::default();
        let host = StoreHost::new();
        // The primary store is the default hosted entry (shares the same index Arc, so
        // the unprefixed routes and /stores/{default}/… see one index).
        let primary = StoreEntry {
            store: store.clone(),
            index: index.clone(),
            corrupt: Arc::clone(&corrupt),
        };
        host.register(primary.clone());
        prewarm_corrupt_tickets(primary);
        let event_log = Arc::new(Mutex::new(EventLog::default()));
        // A permission request enqueued by a driven tool pushes a `permission_asked` nudge
        // over the event bus (WS + long-poll ring), so clients fetch + answer it.
        let permissions = Arc::new(hotsheet_aitools::SharedPermissionBridge::default());
        {
            let events = events.clone();
            let log = event_log.clone();
            permissions.set_on_pending(move |req| {
                let ev = ChangeEvent {
                    cursor: None,
                    store: req.project.clone(),
                    kind: "permission_asked".to_string(),
                    id: req.id.to_string(),
                    slug: req.tool.clone(),
                    message: None,
                    activity: None,
                    assignment: None,
                    turn: None,
                };
                emit_change(&log, &events, ev);
            });
        }
        {
            let events = events.clone();
            let log = event_log.clone();
            permissions.set_on_removed(move |req| {
                emit_change(
                    &log,
                    &events,
                    ChangeEvent {
                        cursor: None,
                        store: req.project.clone(),
                        kind: "permission_resolved".into(),
                        id: req.id.to_string(),
                        slug: req.tool.clone(),
                        message: None,
                        activity: None,
                        assignment: None,
                        turn: None,
                    },
                );
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
                        cursor: None,
                        store: run.project.clone(),
                        kind: "command_updated".into(),
                        id: run.id,
                        slug: run.command_id,
                        message: Some(run.state),
                        activity: None,
                        assignment: None,
                        turn: None,
                    },
                );
            }));
        Self {
            store,
            secret,
            events,
            index,
            corrupt,
            event_log,
            host,
            injected_providers: ProviderRegistry::default(),
            watchers: Arc::default(),
            presence: presence::Presence::default(),
            project_hosted: Arc::default(),
            unhost_sweep: Arc::default(),
            repository_watchers: Arc::new(Mutex::new(std::collections::HashMap::new())),
            github_auth_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            persist_indexes: false,
            instance: Arc::new(Mutex::new(None)),
            instance_guards: Arc::default(),
            writer_locks: Arc::default(),
            sync_kick: Arc::new(Mutex::new(None)),
            local_write_hashes: Default::default(),
            permissions,
            permission_rule_paths: Arc::new(Mutex::new(std::collections::HashMap::new())),
            // A generous busy window: a driven turn heartbeats via the local registry, but
            // the shared one tracks "currently driving" by registration, not the window.
            drive_registry: Arc::new(Mutex::new(hotsheet_aitools::ConnectionRegistry::new(
                60_000,
            ))),
            terminals: Arc::new(hotsheet_terminals::TerminalManager::new()),
            terminal_broker: None,
            plugin_dirs: Arc::new(hotsheet_plugins::default_dirs()),
            model_catalogs: Arc::default(),
            ai_tool_generation: Arc::default(),
            ai_tool_discovery: Arc::default(),
            setup_refreshes: Arc::new(Mutex::new(std::collections::HashSet::new())),
            terminal_server_url: Arc::new(Mutex::new(None)),
            checkout_registry: hotsheet_ticketing::checkouts::CheckoutRegistry::new(
                machine_home.join("checkouts.json"),
            ),
            machine_home: Arc::new(machine_home),
            cache_dir: Arc::new(media::default_cache_root()),
            commands,
            notifications: Default::default(),
            tts: Default::default(),
            source_revision: source_revision::SourceRevisionMonitor::current_build(),
            activity_distillation: Arc::new(Mutex::new(std::collections::HashMap::new())),
            activity_volume: Arc::new(Mutex::new(
                hotsheet_ticketing::ActivityVolumeGuard::default(),
            )),
            client_drives: client_drive::ClientDriveManager::default(),
            lifecycle: Arc::new(LifecycleControl::default()),
            health_scan: Arc::default(),
        }
    }

    /// Resolves when an authenticated client has atomically entered quiescence and asked
    /// this process to restart. The binary races this with SIGTERM/Ctrl-C.
    pub async fn shutdown_requested(&self) {
        self.lifecycle.shutdown.notified().await;
    }

    /// Enter the shutdown drain (HS2-W1KJR4): the listener stops accepting and every
    /// long-lived request waiting in [`Self::stopping`] is told to finish now. Idempotent.
    pub fn begin_stopping(&self) {
        if !self.lifecycle.stopping.swap(true, Ordering::AcqRel) {
            self.lifecycle.stopping_changed.notify_waiters();
        }
    }

    /// Whether [`Self::begin_stopping`] has been called.
    pub fn is_stopping(&self) -> bool {
        self.lifecycle.stopping.load(Ordering::Acquire)
    }

    /// Resolves once the server is stopping (immediately if it already is). Long waits
    /// race this so an open hook or long poll cannot hold graceful shutdown open forever.
    pub async fn stopping(&self) {
        loop {
            let notified = self.lifecycle.stopping_changed.notified();
            let mut notified = std::pin::pin!(notified);
            // Register before checking the flag so a concurrent `begin_stopping` is never
            // missed between the check and the wait.
            notified.as_mut().enable();
            if self.is_stopping() {
                return;
            }
            notified.await;
        }
    }

    /// Remove every discovery instance file and release every hosted store's index-writer
    /// lock now, instead of waiting for the last `AppState` clone to drop (HS2-W1KJR4).
    /// Connection tasks and blocking work can outlive the listener; the files must not.
    pub fn release_instances(&self) {
        let guards: Vec<_> = self
            .instance_guards
            .lock()
            .map(|mut g| g.drain().map(|(_, guard)| guard).collect())
            .unwrap_or_default();
        let locks: Vec<_> = self
            .writer_locks
            .lock()
            .map(|mut w| w.drain().map(|(_, lock)| lock).collect())
            .unwrap_or_default();
        drop((guards, locks));
    }

    fn quiescence_report(&self) -> QuiescenceReport {
        let mut blockers = Vec::new();
        let active_mutations = self.lifecycle.active_mutations.load(Ordering::Acquire);
        if active_mutations > 0 {
            blockers.push(QuiescenceBlocker {
                kind: "mutations",
                count: active_mutations,
            });
        }
        let active_background = self.lifecycle.active_background.load(Ordering::Acquire);
        if active_background > 0 {
            blockers.push(QuiescenceBlocker {
                kind: "background_tasks",
                count: active_background,
            });
        }
        let commands = self
            .commands
            .list_all()
            .into_iter()
            .filter(|run| run.state == "running")
            .count();
        if commands > 0 {
            blockers.push(QuiescenceBlocker {
                kind: "commands",
                count: commands,
            });
        }
        let setup_refreshes = self
            .setup_refreshes
            .lock()
            .map(|set| set.len())
            .unwrap_or(1);
        if setup_refreshes > 0 {
            blockers.push(QuiescenceBlocker {
                kind: "setup_refreshes",
                count: setup_refreshes,
            });
        }
        let driven_turns = self
            .drive_registry
            .lock()
            .map(|reg| reg.count())
            .unwrap_or(1);
        if driven_turns > 0 {
            blockers.push(QuiescenceBlocker {
                kind: "driven_turns",
                count: driven_turns,
            });
        }
        let client_turns = self
            .client_drives
            .list()
            .into_iter()
            .filter(|connection| connection.busy)
            .count();
        if client_turns > 0 {
            blockers.push(QuiescenceBlocker {
                kind: "client_turns",
                count: client_turns,
            });
        }
        let permissions = self.permissions.pending().len();
        if permissions > 0 {
            blockers.push(QuiescenceBlocker {
                kind: "permissions",
                count: permissions,
            });
        }
        let github_auth = self
            .github_auth_sessions
            .lock()
            .map(|sessions| sessions.len())
            .unwrap_or(1);
        if github_auth > 0 {
            blockers.push(QuiescenceBlocker {
                kind: "github_auth",
                count: github_auth,
            });
        }
        if self.terminal_broker.is_none() {
            self.terminals.reap();
            let terminals = self.terminals.count();
            if terminals > 0 {
                blockers.push(QuiescenceBlocker {
                    kind: "terminals",
                    count: terminals,
                });
            }
        }
        QuiescenceReport {
            quiescent: blockers.is_empty(),
            blockers,
        }
    }

    /// Root every machine-local path at `home` instead of `${HOTSHEET_HOME}` (hermetic
    /// tests; HS2-NYZ3PS). Also re-derives the checkout registry and plugin search dirs
    /// from it, so call this **before** [`Self::with_checkout_registry`] /
    /// [`Self::with_plugin_dirs`] when overriding those too.
    pub fn with_machine_home(mut self, home: impl Into<std::path::PathBuf>) -> Self {
        let home = home.into();
        self.checkout_registry =
            hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
        self.plugin_dirs = Arc::new(vec![home.join("plugins")]);
        self.machine_home = Arc::new(home);
        self
    }

    /// Root the machine-local cache (browser/ffmpeg video posters) at `dir` instead of
    /// `${HOTSHEET_CACHE_DIR}` / the platform cache dir, so tests never mutate the
    /// process environment (HS2-FQEESP).
    pub fn with_cache_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.cache_dir = Arc::new(dir.into());
        self
    }

    /// The machine-local cache root this state writes video posters under.
    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache_dir
    }

    /// The machine-local home this state reads and writes under.
    pub fn machine_home(&self) -> &std::path::Path {
        &self.machine_home
    }

    /// The instance-file / index-writer-lock registry under [`Self::machine_home`].
    pub fn instance_registry(&self) -> lifecycle::InstanceRegistry {
        lifecycle::InstanceRegistry::at(self.machine_home.join("instances"))
    }

    fn key_registry(&self) -> KeyRegistry<OsKeychain> {
        KeyRegistry::new(self.machine_home.as_ref().clone(), OsKeychain)
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

    /// Inject the client-drive adapter (hermetic server/API tests).
    pub fn with_client_drive_backend(
        mut self,
        backend: Arc<dyn client_drive::ClientDriveBackend>,
    ) -> Self {
        self.client_drives = client_drive::ClientDriveManager::new(backend);
        self
    }

    pub fn with_client_drive_persistence(
        mut self,
        session_path: std::path::PathBuf,
        home_root: std::path::PathBuf,
    ) -> Result<Self, client_drive::ClientDriveError> {
        self.client_drives = client_drive::ClientDriveManager::with_persistence(
            Arc::new(client_drive::NativeClientDriveBackend),
            session_path,
            home_root,
        )?;
        Ok(self)
    }

    /// Inject a hermetic backend while retaining the real durable session catalog.
    pub fn with_client_drive_backend_persistence(
        mut self,
        backend: Arc<dyn client_drive::ClientDriveBackend>,
        session_path: std::path::PathBuf,
        home_root: std::path::PathBuf,
    ) -> Result<Self, client_drive::ClientDriveError> {
        self.client_drives =
            client_drive::ClientDriveManager::with_persistence(backend, session_path, home_root)?;
        Ok(self)
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
    pub fn with_permission_rules(self, path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        let project = self.store.root().display().to_string();
        let stored_rules = hotsheet_aitools::load_permission_rules(&path);
        let mut rules = stored_rules
            .clone()
            .into_iter()
            .map(|mut rule| {
                rule.project.clear();
                rule
            })
            .collect::<Vec<_>>();
        rules.extend(stored_rules.into_iter().map(|mut rule| {
            rule.project = project.clone();
            rule
        }));
        // Re-seed the bridge with the loaded rules (keeping the on_pending observer).
        self.permissions.reseed_rules(rules);
        let mut paths = self.permission_rule_paths.lock().unwrap();
        paths.insert(String::new(), path.clone());
        paths.insert(project, path);
        drop(paths);
        self
    }

    /// The shared permission bridge — the drive/tool side blocks on it via
    /// `request_blocking`; the server answers it over `POST /permissions/{id}`.
    pub fn permission_bridge(&self) -> Arc<hotsheet_aitools::SharedPermissionBridge> {
        self.permissions.clone()
    }

    /// The `(url-id, root-path)` of every hosted store — the set the background sync loop
    /// iterates (sync, Trash purge, distributed work), sorted by id. Read from the store
    /// registry alone: the loops open each store themselves, so parsing every ticket just
    /// to enumerate roots was wasted work on every pass (HS2-4XXRJP).
    pub fn hosted_store_roots(&self) -> Vec<(String, String)> {
        let mut roots: Vec<(String, String)> = self
            .host
            .locations()
            .into_iter()
            .map(|(id, root)| (id, root.display().to_string()))
            .collect();
        roots.sort_by(|a, b| a.0.cmp(&b.0));
        roots
    }

    /// Effective automatic Trash retention for a hosted store. A store may be shared by
    /// several checkouts; the longest configured retention wins so one project cannot
    /// permanently remove another project's recoverable tickets early.
    pub(crate) fn trash_cleanup_days_for_store(&self, root: &FsPath) -> u32 {
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut configured = None;
        for checkout in self.checkout_registry.list().unwrap_or_default() {
            let linked = checkout.sources.iter().any(|source| {
                source.provider == "git"
                    && FsPath::new(&source.locator)
                        .canonicalize()
                        .unwrap_or_else(|_| source.locator.clone().into())
                        == canonical_root
            });
            if !linked {
                continue;
            }
            match checkout.settings().trash_cleanup_days() {
                Ok(days) => {
                    configured = Some(configured.map_or(days, |current: u32| current.max(days)))
                }
                Err(error) => eprintln!(
                    "Trash retention ignored for checkout {}: {error}",
                    checkout.id
                ),
            }
        }
        configured.unwrap_or(hotsheet_ticketing::DEFAULT_TRASH_CLEANUP_DAYS)
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
        self.model_catalogs = Arc::default();
        self
    }

    /// Replace AI-tool discovery's memo lifetime and scan (hermetic tests; HS2-BK350W).
    /// The scan receives the plugin search dirs, project root, model catalogs, and the
    /// refresh flag, exactly like the production
    /// [`hotsheet_aitools::discover_ai_tool_descriptors`]. Resets the memo.
    pub fn with_ai_tool_discovery<F>(mut self, ttl: std::time::Duration, scanner: F) -> Self
    where
        F: Fn(
                &[std::path::PathBuf],
                &FsPath,
                &mut hotsheet_aitools::ModelCatalogCache,
                bool,
            ) -> Vec<hotsheet_plugins::AiToolDescriptor>
            + Send
            + Sync
            + 'static,
    {
        self.ai_tool_discovery = Arc::new(ai_tool_discovery::AiToolDiscoveryConfig {
            ttl,
            scanner: Arc::new(scanner),
        });
        self.model_catalogs = Arc::default();
        self
    }

    /// Replace only the discovery memo lifetime, keeping the real scan (tests that
    /// exercise real tool discovery but must not see the memo expire mid-test when that
    /// scan is slow; HS2-BK350W). Resets the memo.
    pub fn with_ai_tool_discovery_ttl(mut self, ttl: std::time::Duration) -> Self {
        let scanner = self.ai_tool_discovery.scanner.clone();
        self.ai_tool_discovery =
            Arc::new(ai_tool_discovery::AiToolDiscoveryConfig { ttl, scanner });
        self.model_catalogs = Arc::default();
        self
    }

    /// Warm the AI model catalog in the background at server start so the **first** client
    /// doesn't pay cold discovery on its startup path (HS2-MYDN7C follow-up to HS2-10R4VV).
    /// Discovery is blocking subprocess work, so it runs on a detached thread
    /// (`discovered_ai_tools_off_runtime`) that never delays binding, serving, or a stop
    /// (HS2-NPBZJ9); a failure just
    /// leaves the cache cold for the first on-demand discovery to fill. Call once, after the
    /// runtime is up and this process has decided it is the serving instance.
    pub fn prewarm_ai_catalog(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let _ = discovered_ai_tools_off_runtime(&state, false).await;
        });
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
        // Validate the store boundary before registering an index, watcher, or discovery
        // file. Otherwise an older server can appear to accept a newer store and fail only
        // when its first ticket mutation reaches the writer.
        store
            .metadata()
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
        let id = multistore::store_url_id(&store);
        let initialization = self.host.initialization_lock(&id);
        let _initializing = initialization.lock().unwrap();
        if self.host.contains(&id) {
            return Ok(false);
        }
        let index = if self.persist_indexes {
            let path = multistore::index_path_for(&self.machine_home, &store)
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
            corrupt: Arc::default(),
        };
        self.host.register(entry.clone());
        prewarm_corrupt_tickets(entry.clone());
        let project = store.root().display().to_string();
        if let Ok(mut paths) = self.permission_rule_paths.lock()
            && !paths.is_empty()
            && !paths.contains_key(&project)
            && let Some(directory) = paths.values().next().and_then(|path| path.parent())
        {
            let path = directory.join(format!("{id}.json"));
            let rules = hotsheet_aitools::load_permission_rules(&path)
                .into_iter()
                .map(|mut rule| {
                    rule.project = project.clone();
                    rule
                });
            self.permissions.add_rules(rules);
            paths.insert(project, path);
        }
        let store_root = store.root().to_path_buf();
        let watched_id = id.clone();
        match spawn_watcher_for(
            WatchTarget {
                entry,
                store_id: id,
                host: self.host.clone(),
                local_write_hashes: self.local_write_hashes.clone(),
                events: self.events.clone(),
                event_log: self.event_log.clone(),
                checkout_registry: self.checkout_registry.clone(),
            },
            registered_watcher_backend(),
        ) {
            Ok(handle) => {
                if let Ok(mut w) = self.watchers.lock() {
                    w.insert(watched_id, handle);
                }
            }
            Err(e) => eprintln!("watcher for {} failed to start: {e}", store_root.display()),
        }
        // Advertise the newly-hosted store for discovery (real run only; a no-op in tests).
        self.register_store_instance(&store_root);
        Ok(true)
    }

    fn watch_checkout_repository(&self, checkout: &hotsheet_ticketing::checkouts::Checkout) {
        let Ok(mut watchers) = self.repository_watchers.lock() else {
            return;
        };
        if watchers.contains_key(&checkout.id) {
            return;
        }
        let root = std::path::PathBuf::from(&checkout.root);
        if hotsheet_ticketing::repository_status::snapshot(&root).is_err() {
            return;
        }
        match spawn_repository_watcher(
            root.clone(),
            checkout.id.clone(),
            self.events.clone(),
            self.event_log.clone(),
        ) {
            Ok(handle) => {
                watchers.insert(checkout.id.clone(), handle);
            }
            Err(error) => eprintln!(
                "repository watcher for {} failed to start: {error}",
                root.display()
            ),
        }
    }

    /// The hosted entry for a checkout's git source, hosting it again when an earlier sweep
    /// unhosted it (HS2-ARJ9J1): a client that kept a project open across a long sleep keeps
    /// working without reopening it.
    fn hosted_source(
        &self,
        source: &hotsheet_ticketing::checkouts::TicketSource,
    ) -> Option<StoreEntry> {
        if let Some(entry) = self.host.get(&source.connection_id) {
            return Some(entry);
        }
        if source.provider != "git" {
            return None;
        }
        let store = FsStore::open(&source.locator).ok()?;
        self.host_project_store(store).ok()?;
        self.host.get(&source.connection_id)
    }

    /// Host a store for an open project, making it eligible to be unhosted once no open
    /// checkout references it (HS2-ARJ9J1).
    fn host_project_store(&self, store: FsStore) -> Result<bool, ApiError> {
        let id = multistore::store_url_id(&store);
        let pinned = self.host.contains(&id) && !self.is_project_hosted(&id);
        let added = self.host_store(store)?;
        if !pinned && let Ok(mut hosted) = self.project_hosted.lock() {
            hosted.insert(id);
        }
        self.request_unhost_sweep(presence::POLL_RECONNECT_GAP + presence::UNHOST_GRACE);
        Ok(added)
    }

    fn is_project_hosted(&self, id: &str) -> bool {
        self.project_hosted
            .lock()
            .is_ok_and(|hosted| hosted.contains(id))
    }

    /// Stop hosting a project store: its index, watcher, discovery file and writer lock go,
    /// and the next open or checkout request hosts it again (HS2-ARJ9J1).
    fn unhost_store(&self, id: &str) {
        if let Ok(mut hosted) = self.project_hosted.lock()
            && !hosted.remove(id)
        {
            return;
        }
        let Some(entry) = self.host.unregister(id) else {
            return;
        };
        let root = entry.store.root().display().to_string();
        let watcher = self.watchers.lock().ok().and_then(|mut w| w.remove(id));
        let guard = self
            .instance_guards
            .lock()
            .ok()
            .and_then(|mut g| g.remove(&root));
        let lock = self
            .writer_locks
            .lock()
            .ok()
            .and_then(|mut w| w.remove(&root));
        // Stop the watcher and release the files outside every lock.
        drop((watcher, guard, lock));
        eprintln!("unhosted store {root}: no open project references it");
    }

    /// Ask for an unhost sweep `delay` from now; an earlier pending request wins.
    fn request_unhost_sweep(&self, delay: Duration) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let at = tokio::time::Instant::now() + delay;
        {
            let Ok(mut due) = self.unhost_sweep.due.lock() else {
                return;
            };
            if due.is_some_and(|current| current <= at) {
                return;
            }
            *due = Some(at);
        }
        self.unhost_sweep.wake.notify_one();
        if !self
            .unhost_sweep
            .started
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            let state = self.clone();
            runtime.spawn(async move { run_unhost_sweeper(state).await });
        }
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
        for (_, root) in self.host.locations() {
            self.register_store_instance(&root);
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
                .and_then(|s| multistore::index_path_for(&self.machine_home, &s).ok())
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
        let instances = self.instance_registry();
        if !is_primary {
            match instances.acquire_writer_lock(store_path) {
                Ok(lock) => {
                    if let Ok(mut w) = self.writer_locks.lock() {
                        w.insert(store_path.display().to_string(), lock);
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
        match instances.register_instance(&info, store_path) {
            Ok(guard) => {
                if let Ok(mut g) = self.instance_guards.lock() {
                    g.insert(store_path.display().to_string(), guard);
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
        for path in multistore::configured_store_paths(&self.machine_home) {
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
        store.metadata()?;
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
            corrupt: self.corrupt.clone(),
        }
    }

    /// Reindex a ticket the server just wrote into `entry`'s index, then broadcast a
    /// change tagged with the store it happened in. The index now carries the file's
    /// hash, so the watcher sees "no change" and won't re-emit.
    fn changed_in(&self, entry: &StoreEntry, kind: &str, t: &Ticket) {
        let text = to_file_string(t);
        let path = entry.store.ticket_path(&t.id).display().to_string();
        let store_id = multistore::store_url_id(&entry.store);
        if let Ok(mut writes) = self.local_write_hashes.lock() {
            writes.insert(
                (
                    store_id.clone(),
                    t.id.to_string(),
                    hash_bytes(text.as_bytes()),
                ),
                std::time::Instant::now(),
            );
        }
        if let Ok(index) = entry.index.lock() {
            let _ = index.upsert(t, &path, &hash_bytes(text.as_bytes()));
        }
        self.emit(ChangeEvent {
            cursor: None,
            store: store_id,
            kind: kind.to_string(),
            id: t.id.to_string(),
            slug: t.slug.clone(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
        if let Ok(checkouts) = self.checkout_registry.list() {
            for checkout in checkouts.into_iter().filter(|checkout| {
                checkout
                    .stores
                    .iter()
                    .any(|root| same_path(FsPath::new(root), entry.store.root()))
            }) {
                if let Err(error) = regenerate_checkout_worklist_indexed(&self.host, &checkout) {
                    eprintln!("worklist regenerate failed for {}: {error}", checkout.root);
                }
            }
        }
        // A write is worth pushing promptly — wake the background sync loop (HS2-731C2X).
        self.kick_sync();
    }

    /// Remove a hard-purged ticket from the live index and publish one deletion event.
    /// The empty hash marker suppresses the filesystem watcher's echo of this local write.
    fn removed_in(&self, entry: &StoreEntry, ticket: &Ticket) {
        let store_id = multistore::store_url_id(&entry.store);
        if let Ok(mut writes) = self.local_write_hashes.lock() {
            writes.insert(
                (store_id.clone(), ticket.id.to_string(), String::new()),
                std::time::Instant::now(),
            );
        }
        if let Ok(index) = entry.index.lock() {
            let _ = index.delete(&ticket.id);
        }
        self.emit(ChangeEvent {
            cursor: None,
            store: store_id,
            kind: "deleted".into(),
            id: ticket.id.to_string(),
            slug: ticket.slug.clone(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
        if let Ok(checkouts) = self.checkout_registry.list() {
            for checkout in checkouts.into_iter().filter(|checkout| {
                checkout
                    .stores
                    .iter()
                    .any(|root| same_path(FsPath::new(root), entry.store.root()))
            }) {
                if let Err(error) = regenerate_checkout_worklist_indexed(&self.host, &checkout) {
                    eprintln!("worklist regenerate failed for {}: {error}", checkout.root);
                }
            }
        }
        self.kick_sync();
    }

    /// Broadcast an **ephemeral announcement** to live `/ws/sync` subscribers (HS2-HHDNTH):
    /// a store-level message that is **not** persisted — it rides the WS bus only, so it is
    /// NOT recorded in the long-poll ring and never replayed. A client not connected when it
    /// fires simply misses it. `store` is the target store's URL id (empty = the default).
    pub fn announce(&self, store: String, message: String) {
        // WS-only: intentionally skip the EventLog ring (ephemeral, unlike `emit`).
        let _ = self.events.send(ChangeEvent {
            cursor: None,
            store,
            kind: "announce".to_string(),
            id: String::new(),
            slug: String::new(),
            message: Some(message),
            activity: None,
            assignment: None,
            turn: None,
        });
    }

    fn emit_drive_updated(&self, info: &client_drive::ClientConnectionInfo) {
        self.emit(ChangeEvent {
            cursor: None,
            store: info.source.clone(),
            kind: "drive_updated".into(),
            id: info.id.clone(),
            slug: info.tool.clone(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
    }

    pub fn emit_turn_event(
        &self,
        store: &FsStore,
        connection_id: &str,
        ticket: Option<String>,
        tool: &str,
        event: turn_stream::ClientTurnEvent,
    ) {
        self.emit(ChangeEvent {
            cursor: None,
            store: multistore::store_url_id(store),
            kind: "turn_event".into(),
            id: connection_id.into(),
            slug: tool.into(),
            message: None,
            activity: None,
            assignment: None,
            turn: Some(turn_stream::TurnStreamEnvelope {
                connection_id: connection_id.into(),
                ticket,
                event,
            }),
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
        let admitted = self
            .activity_volume
            .lock()
            .map_err(|_| std::io::Error::other("activity volume guard is unavailable"))?
            .observe(event);
        for event in admitted {
            hotsheet_ticketing::activity::record(store, &event)?;
            self.emit(ChangeEvent {
                cursor: None,
                store: multistore::store_url_id(store),
                kind: "activity".to_string(),
                id: event.id.clone(),
                slug: String::new(),
                message: None,
                activity: Some(event.clone()),
                assignment: None,
                turn: None,
            });
            self.maybe_distill_activity(store, &event);
        }
        Ok(())
    }

    fn maybe_distill_activity(&self, store: &FsStore, event: &hotsheet_ticketing::ActivityEvent) {
        let store_id = multistore::store_url_id(store);
        let (pipeline_id, settings) = match event.project.as_deref() {
            Some(reference) => {
                let checkout = match self.checkout_registry.resolve(reference) {
                    Ok(checkout) => checkout,
                    Err(error) => {
                        eprintln!(
                            "activity distillation skipped for unresolved checkout {reference}: {error}"
                        );
                        return;
                    }
                };
                let canonical_store = store
                    .root()
                    .canonicalize()
                    .unwrap_or_else(|_| store.root().to_path_buf());
                let linked = checkout.sources.iter().any(|source| {
                    source.provider == "git"
                        && std::path::Path::new(&source.locator)
                            .canonicalize()
                            .unwrap_or_else(|_| source.locator.clone().into())
                            == canonical_store
                });
                if !linked {
                    eprintln!(
                        "activity distillation skipped because checkout {} does not own store {store_id}",
                        checkout.id
                    );
                    return;
                }
                (
                    format!("checkout:{}:store:{store_id}", checkout.id),
                    checkout.settings(),
                )
            }
            None => (
                format!("legacy-store:{store_id}"),
                Settings::new(store.root()),
            ),
        };
        let policy = match hotsheet_ticketing::DistillationPolicy::from_local_settings(&settings) {
            Ok(policy) => policy,
            Err(error) => {
                eprintln!("activity distillation policy ignored: {error}");
                return;
            }
        };
        // Named client adapters (including Apple Foundation Models) consume the same
        // normalized stream on-device. They are never loaded as server requirements.
        if !policy.enabled || policy.adapter != "deterministic" {
            if let Ok(mut pipelines) = self.activity_distillation.lock() {
                pipelines.remove(&pipeline_id);
            }
            return;
        }
        let request = self
            .activity_distillation
            .lock()
            .ok()
            .and_then(|mut pipelines| {
                pipelines
                    .entry(pipeline_id)
                    .or_default()
                    .observe(event, &policy)
            });
        let Some(request) = request else {
            return;
        };
        let Some(note) = hotsheet_ticketing::activity_distillation::distill(
            &request,
            &policy,
            &hotsheet_ticketing::DeterministicActivitySummarizer,
        ) else {
            return;
        };
        let provider = GitProvider::new(store_id, store.clone());
        if let Err(error) = hotsheet_ticketing::write_distilled_note(
            &provider,
            &request.provenance.ticket,
            Timestamp::new(event.ts.clone()),
            &note,
        ) {
            // Activity capture is authoritative and must not fail because an optional
            // distillation adapter or ticket mutation is temporarily unavailable.
            eprintln!("activity distillation note failed: {error}");
        }
    }
}

/// A live-change event pushed over `/ws/sync`.
#[derive(Clone, Debug, Serialize)]
pub struct ChangeEvent {
    /// Monotonic replay cursor. Present for durable/replayable events; absent for ephemeral
    /// WS-only announcements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
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
    /// For `kind == "turn_event"`: bounded raw tool output/usage/activity/done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<turn_stream::TurnStreamEnvelope>,
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
    let event = event_log
        .lock()
        .map(|mut log| log.push(event.clone()))
        .unwrap_or(event);
    let _ = events.send(event); // Err just means no live subscribers.
}

impl EventLog {
    /// Record an event, assigning it the next seq.
    fn push(&mut self, mut event: ChangeEvent) -> ChangeEvent {
        self.seq += 1;
        event.cursor = Some(self.seq);
        self.ring.push_back((self.seq, event.clone()));
        while self.ring.len() > EVENT_LOG_CAP {
            self.ring.pop_front();
        }
        event
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
            cursor: None,
            store: "s".into(),
            kind: "created".into(),
            id: format!("id-{n}"),
            slug: format!("HS-{n}"),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
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
        assert_eq!(all[0].cursor, Some(1));
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
        // Coordination: claim next or one exact ticket, release, renew a lease.
        .route("/claim-next", post(claim_next_ticket))
        .route("/tickets/{id}/claim", post(claim_ticket))
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
        .route("/checkouts/{reference}/close", post(close_checkout_session))
        .route("/checkouts/{reference}", get(resolve_checkout))
        .route(
            "/checkouts/{reference}/commands",
            get(list_checkout_commands).put(save_checkout_commands),
        )
        .route(
            "/checkouts/{reference}/command-groups",
            get(list_checkout_command_groups).put(save_checkout_command_groups),
        )
        .route(
            "/checkouts/{reference}/commands/{id}/run",
            post(run_checkout_command),
        )
        .route(
            "/checkouts/{reference}/command-runs",
            get(list_checkout_command_runs),
        )
        .route(
            "/checkouts/{reference}/command-runs/{id}",
            get(get_checkout_command_run),
        )
        .route(
            "/checkouts/{reference}/command-runs/{id}/cancel",
            post(cancel_checkout_command_run),
        )
        .route(
            "/checkouts/{reference}/views",
            get(list_checkout_custom_views).put(save_checkout_custom_views),
        )
        .route(
            "/checkouts/{reference}/ai-settings",
            get(get_checkout_ai_settings).put(put_checkout_ai_settings),
        )
        .route(
            "/checkouts/{reference}/terminal-settings",
            get(get_checkout_terminal_settings).put(put_checkout_terminal_settings),
        )
        .route(
            "/checkouts/{reference}/trash-settings",
            get(get_checkout_trash_settings).put(put_checkout_trash_settings),
        )
        .route(
            "/checkouts/{reference}/sources/{connection_id}",
            put(add_checkout_source).delete(remove_checkout_source),
        )
        .route(
            "/checkouts/{reference}/default-source",
            put(set_checkout_default_source),
        )
        .route(
            "/checkouts/{reference}/providers",
            get(list_checkout_providers),
        )
        .route(
            "/checkouts/{reference}/repository/status",
            get(checkout_repository_status),
        )
        .route(
            "/checkouts/{reference}/repository/init",
            post(initialize_checkout_repository),
        )
        .route(
            "/checkouts/{reference}/repository/remote",
            post(configure_checkout_repository_remote),
        )
        .route(
            "/checkouts/{reference}/repository/files",
            get(checkout_repository_files),
        )
        .route(
            "/checkouts/{reference}/repository/commits",
            get(checkout_repository_commits),
        )
        .route(
            "/checkouts/{reference}/repository/review",
            post(open_checkout_repository_review),
        )
        .route(
            "/checkouts/{reference}/repository/files/action",
            post(checkout_repository_file_action),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/code-review",
            get(get_checkout_code_review).post(open_checkout_code_review),
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
            "/checkouts/{reference}/confidence-report",
            get(checkout_confidence_report),
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
            "/checkouts/{reference}/tickets/{id}/duplicate-backlinks",
            get(get_checkout_ticket_duplicate_backlinks),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/close",
            post(close_checkout_ticket),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/restore",
            post(restore_checkout_ticket),
        )
        .route(
            "/checkouts/{reference}/trash/empty",
            post(empty_checkout_trash),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/assign",
            post(assign_checkout_ticket),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/attachments",
            post(add_checkout_ticket_attachment)
                .layer(DefaultBodyLimit::max(MAX_ATTACHMENT_BODY_BYTES))
                .patch(update_checkout_ticket_attachment_metadata),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/attachments/by-name/{filename}",
            get(get_checkout_ticket_attachment_by_name),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/attachments/by-name/{filename}/action",
            post(act_on_checkout_ticket_attachment_by_name),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/attachments/{attachment_id}",
            get(get_checkout_ticket_attachment)
                .put(update_checkout_ticket_attachment_annotations)
                .patch(rename_checkout_ticket_attachment)
                .delete(delete_checkout_ticket_attachment),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/attachments/{attachment_id}/thumbnail",
            get(get_checkout_ticket_attachment_thumbnail)
                .put(put_checkout_ticket_attachment_thumbnail)
                .layer(DefaultBodyLimit::max(MAX_VIDEO_POSTER_BODY_BYTES)),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/attachments/{attachment_id}/action",
            post(act_on_checkout_ticket_attachment),
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
            "/providers/{connection_id}/tickets/{id}/restore",
            post(restore_provider_ticket),
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
        .route(
            "/provider-connections/{connection_id}/disabled",
            put(set_provider_connection_disabled),
        )
        .route("/github-auth/device", post(start_github_device_auth))
        .route(
            "/github-auth/device/{session_id}",
            get(wait_github_device_auth).delete(cancel_github_device_auth),
        )
        .route(
            "/github-auth/device/{session_id}/repositories",
            get(list_github_auth_repositories),
        )
        .route("/provider-attachments/copy", post(copy_provider_attachment))
        // Authenticated application/protocol negotiation. `/health` intentionally remains
        // a small unauthenticated liveness probe.
        .route("/compatibility", get(compatibility))
        // A client may supervise this process but never owns it: restart is accepted only
        // after mutation admission closes and all server-owned work is proven quiescent.
        .route("/lifecycle/quiescence", get(lifecycle_quiescence))
        .route("/lifecycle/restart", post(lifecycle_restart))
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
        .route("/ai-tools", get(list_ai_tools))
        .route("/ai-settings", get(get_ai_settings).put(put_ai_settings))
        .route("/drive/connections", post(create_drive_connection))
        .route("/drive/sessions", get(list_drive_sessions))
        .route(
            "/checkouts/{reference}/drive/connections/{id}",
            delete(delete_drive_connection),
        )
        .route("/drive/connections/{id}/turns", post(send_drive_turn))
        .route(
            "/drive/connections/{id}/interrupt",
            post(interrupt_drive_turn),
        )
        .route("/analytics/tickets", get(ticket_flow_summary))
        .route("/analytics/usage", get(usage_metrics_summary))
        .route("/confidence-report", get(confidence_report))
        .route("/commands", get(list_commands).put(save_commands))
        .route(
            "/command-groups",
            get(list_command_groups).put(save_command_groups),
        )
        .route("/views", get(list_custom_views).put(save_custom_views))
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
        .route(
            "/terminal-settings",
            get(get_terminal_settings).put(put_terminal_settings),
        )
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
    if presented != Some(state.secret.as_str()) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "missing or invalid secret",
        ));
    }
    let is_mutation = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    let is_restart = req.uri().path() == "/lifecycle/restart";
    let _mutation = if is_mutation && !is_restart {
        Some(state.begin_mutation().ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "server is quiescing for a safe restart; retry after reconnecting",
            )
        })?)
    } else {
        None
    };
    Ok(next.run(req).await)
}

// ---- handlers --------------------------------------------------------------------

async fn health(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let metadata = state.store.metadata()?;
    let (listing, source) = state.health_listing().await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "generation": "hs2",
        "api_version": 1,
        "ticket_prefix": metadata.ticket_prefix,
        "store_schema": metadata.schema_version,
        "tickets": listing.tickets,
        "corrupt": listing.corrupt,
        "listing": source,
    })))
}

impl AppState {
    /// The primary store's ticket count and corrupt files for `GET /health`.
    ///
    /// Resilient enumeration (HS2-PRVPCQ): a single corrupt ticket file must not make the
    /// whole store un-openable, so healthy tickets are counted and unparseable files are
    /// surfaced separately. That walk parses every ticket, so it runs on the blocking pool,
    /// single-flight, and is awaited only for [`health_scan::HEALTH_SCAN_BUDGET`]
    /// (HS2-9PPDR1): a large or blocked store never stalls the liveness probe. Past the
    /// budget the answer is the last completed scan (`"cached"`), or, before any scan has
    /// completed, the index's row count with no corrupt entries (`"index"`).
    async fn health_listing(
        &self,
    ) -> Result<(Arc<health_scan::HealthListing>, &'static str), ApiError> {
        use health_scan::{HealthListing, Resolution, ScanFailure};
        let store = self.store.clone();
        let resolution = self
            .health_scan
            .resolve(move || {
                let listing = store.list_tickets_resilient().map_err(|error| {
                    let error = ApiError::from(error);
                    ScanFailure {
                        status: error.status,
                        message: error.message,
                    }
                })?;
                Ok(Arc::new(HealthListing {
                    tickets: listing.tickets.len(),
                    corrupt: listing
                        .corrupt
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "path": c.path.display().to_string(),
                                "id": c.id.map(|id| id.to_string()),
                                "slug": c.slug,
                                "error": c.error,
                                "error_code": c.error_code,
                            })
                        })
                        .collect(),
                }))
            })
            .await;
        match resolution {
            Resolution::Fresh(Ok(listing)) => Ok((listing, "fresh")),
            Resolution::Fresh(Err(failure)) => Err(ApiError::new(failure.status, failure.message)),
            Resolution::Cached(listing) => Ok((listing, "cached")),
            Resolution::Pending => {
                let tickets = self
                    .index
                    .lock()
                    .map_err(|_| {
                        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned")
                    })?
                    .ticket_count()?;
                Ok((
                    Arc::new(HealthListing {
                        tickets,
                        corrupt: Vec::new(),
                    }),
                    "index",
                ))
            }
        }
    }
}

struct ActiveMutation(Arc<LifecycleControl>);

impl Drop for ActiveMutation {
    fn drop(&mut self) {
        self.0.active_mutations.fetch_sub(1, Ordering::Release);
    }
}

impl AppState {
    fn begin_mutation(&self) -> Option<ActiveMutation> {
        if self.lifecycle.quiescing.load(Ordering::Acquire) {
            return None;
        }
        self.lifecycle
            .active_mutations
            .fetch_add(1, Ordering::AcqRel);
        if self.lifecycle.quiescing.load(Ordering::Acquire) {
            self.lifecycle
                .active_mutations
                .fetch_sub(1, Ordering::Release);
            None
        } else {
            Some(ActiveMutation(self.lifecycle.clone()))
        }
    }

    pub(crate) fn begin_background_work(&self) -> Option<ActiveBackgroundWork> {
        if self.lifecycle.quiescing.load(Ordering::Acquire) {
            return None;
        }
        self.lifecycle
            .active_background
            .fetch_add(1, Ordering::AcqRel);
        if self.lifecycle.quiescing.load(Ordering::Acquire) {
            self.lifecycle
                .active_background
                .fetch_sub(1, Ordering::Release);
            None
        } else {
            Some(ActiveBackgroundWork(self.lifecycle.clone()))
        }
    }
}

pub(crate) struct ActiveBackgroundWork(Arc<LifecycleControl>);

impl Drop for ActiveBackgroundWork {
    fn drop(&mut self) {
        self.0.active_background.fetch_sub(1, Ordering::Release);
    }
}

async fn lifecycle_quiescence(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "quiescing": state.lifecycle.quiescing.load(Ordering::Acquire),
        "report": state.quiescence_report(),
    }))
}

async fn lifecycle_restart(State(state): State<AppState>) -> Response {
    if state
        .lifecycle
        .quiescing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "a safe restart is already being evaluated"
            })),
        )
            .into_response();
    }
    let report = state.quiescence_report();
    if !report.quiescent {
        state.lifecycle.quiescing.store(false, Ordering::Release);
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "server is not quiescent; active work was preserved",
                "quiescence": report,
            })),
        )
            .into_response();
    }
    state.lifecycle.shutdown.notify_one();
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "restarting": true })),
    )
        .into_response()
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
            "lifecycle_restart": true,
            "lifecycle_quiescence": true
        },
        "started_at": started_at
    }))
}

async fn list_tickets(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Response, ApiError> {
    let entry = state.default_entry();
    list_entry_tickets(&entry, &multistore::store_url_id(&state.store), params)
}

impl From<ops::StoreReadBoundError> for ApiError {
    fn from(error: ops::StoreReadBoundError) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
    }
}

/// One store's unpaged list, served from its index under the bounded-response contract
/// (HS2-3JEFQT): at most `STORE_READ_MAX_ROWS` rows, an implicit overflow fails with 400,
/// and an explicit `limit` truncates with `x-hotsheet-truncated: true`. Larger reads page
/// with `limit` + `page_after`.
fn list_entry_tickets(
    entry: &StoreEntry,
    connection_id: &str,
    params: ListParams,
) -> Result<Response, ApiError> {
    let compact = params.compact.unwrap_or(true);
    let fields = parse_fields(&params.fields);
    let mut query = params.into_query(entry.store.root())?;
    let bound = ops::StoreReadBound::new(query.limit)?;
    query.limit = Some(bound.fetch_limit());
    let mut rows = entry
        .index
        .lock()
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned"))?
        .query(&query)?;
    let truncated = bound.finish(&mut rows)?;
    for row in &mut rows {
        row.set_connection(connection_id);
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
    let mut response = Json(rows_to_json(rows, &fields)).into_response();
    if truncated {
        response.headers_mut().insert(
            TRUNCATED_HEADER,
            axum::http::HeaderValue::from_static("true"),
        );
    }
    Ok(response)
}

async fn get_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiTicket>, ApiError> {
    let ticket = ops::resolve(&state.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json(api_ticket(&state.default_entry(), &ticket)?))
}

fn api_ticket(entry: &StoreEntry, ticket: &Ticket) -> Result<ApiTicket, ApiError> {
    api_ticket_with_settings(entry, ticket, &Settings::new(entry.store.root()))
}

fn api_ticket_with_settings(
    entry: &StoreEntry,
    ticket: &Ticket,
    settings: &Settings,
) -> Result<ApiTicket, ApiError> {
    let contexts = auto_context::effective(settings)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(ApiTicket::with_provider_auto_context(
        ticket,
        &multistore::store_url_id(&entry.store),
        None,
        &contexts,
    ))
}

fn contextualize_api_ticket(
    mut ticket: ApiTicket,
    settings: &Settings,
) -> Result<ApiTicket, ApiError> {
    let contexts = auto_context::effective(settings)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    ticket.auto_context = auto_context::resolve_fields(&ticket.category, &ticket.tags, &contexts);
    Ok(ticket)
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

/// `GET /stores` — the stores this machine server hosts, with ticket counts. Counting
/// parses every ticket in every store, so it runs on a blocking thread and never stalls
/// `/health` or other requests (HS2-4XXRJP).
async fn list_stores(State(state): State<AppState>) -> Result<Json<Vec<StoreInfo>>, ApiError> {
    let host = state.host.clone();
    tokio::task::spawn_blocking(move || host.list())
        .await
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum GitHubAuthStatus {
    Pending,
    Authorized { credential_reference: String },
    Denied,
    Expired,
    Cancelled,
    Error { message: String },
}

struct GitHubAuthSession {
    status: tokio::sync::watch::Sender<GitHubAuthStatus>,
    client_id: String,
    web_base: String,
    credential_reference: String,
}

#[derive(Debug, Deserialize)]
struct StartGitHubAuthBody {
    #[serde(default = "default_github_web_base")]
    web_base: String,
}

fn default_github_web_base() -> String {
    "https://github.com".into()
}

#[derive(Debug, Serialize)]
struct StartGitHubAuthResponse {
    session_id: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
}

async fn start_github_device_auth(
    State(state): State<AppState>,
    Json(body): Json<StartGitHubAuthBody>,
) -> Result<(StatusCode, Json<StartGitHubAuthResponse>), ApiError> {
    let web_base = body.web_base.trim_end_matches('/').to_owned();
    let client_id = github_app_config::client_id_for_web_base(&web_base)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    let client = hotsheet_extsync::GitHubDeviceClient::live(client_id.clone(), web_base.clone());
    let authorization = tokio::task::spawn_blocking(move || client.start())
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;
    let session_id = ulid::Ulid::new().to_string();
    let credential_reference = format!("github-app-{}", session_id.to_ascii_lowercase());
    let (status, _) = tokio::sync::watch::channel(GitHubAuthStatus::Pending);
    let session = Arc::new(GitHubAuthSession {
        status,
        client_id: client_id.clone(),
        web_base: web_base.clone(),
        credential_reference: credential_reference.clone(),
    });
    state
        .github_auth_sessions
        .lock()
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "GitHub sign-in state is unavailable",
            )
        })?
        .insert(session_id.clone(), session.clone());
    let device_code = authorization.device_code.clone();
    let mut interval = authorization.interval.max(1);
    let keys = state.key_registry();
    std::thread::spawn(move || {
        let client =
            hotsheet_extsync::GitHubDeviceClient::live(client_id.clone(), web_base.clone());
        loop {
            std::thread::sleep(Duration::from_secs(interval));
            if !matches!(*session.status.borrow(), GitHubAuthStatus::Pending) {
                break;
            }
            match client.poll(&device_code) {
                Ok(hotsheet_extsync::DevicePoll::Pending) => {}
                Ok(hotsheet_extsync::DevicePoll::SlowDown) => interval = interval.saturating_add(5),
                Ok(hotsheet_extsync::DevicePoll::Expired) => {
                    let _ = session.status.send(GitHubAuthStatus::Expired);
                    break;
                }
                Ok(hotsheet_extsync::DevicePoll::Denied) => {
                    let _ = session.status.send(GitHubAuthStatus::Denied);
                    break;
                }
                Ok(hotsheet_extsync::DevicePoll::Authorized(bundle)) => {
                    let result = hotsheet_extsync::store_device_authorization(
                        &keys,
                        &credential_reference,
                        &client_id,
                        &web_base,
                        &bundle,
                        OffsetDateTime::now_utc().unix_timestamp(),
                    );
                    let next = match result {
                        Ok(()) => GitHubAuthStatus::Authorized {
                            credential_reference: credential_reference.clone(),
                        },
                        Err(error) => GitHubAuthStatus::Error {
                            message: error.to_string(),
                        },
                    };
                    let _ = session.status.send(next);
                    break;
                }
                Err(error) => {
                    let _ = session.status.send(GitHubAuthStatus::Error {
                        message: error.to_string(),
                    });
                    break;
                }
            }
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(StartGitHubAuthResponse {
            session_id,
            user_code: authorization.user_code,
            verification_uri: authorization.verification_uri,
            expires_in: authorization.expires_in,
        }),
    ))
}

async fn wait_github_device_auth(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<GitHubAuthStatus>, ApiError> {
    let session = state
        .github_auth_sessions
        .lock()
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "GitHub sign-in state is unavailable",
            )
        })?
        .get(&session_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(&session_id))?;
    let mut status = session.status.subscribe();
    let pending = matches!(*status.borrow(), GitHubAuthStatus::Pending);
    if pending {
        let _ = status.changed().await;
    }
    let result = status.borrow().clone();
    Ok(Json(result))
}

async fn cancel_github_device_auth(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let session = state
        .github_auth_sessions
        .lock()
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "GitHub sign-in state is unavailable",
            )
        })?
        .get(&session_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(&session_id))?;
    let _ = session.status.send(GitHubAuthStatus::Cancelled);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
struct GitHubRepositoriesResponse {
    repositories: Vec<String>,
    /// What each app installation grants and where to change it (HS2-27T5WT).
    installations: Vec<hotsheet_extsync::AppInstallation>,
    install_url: Option<String>,
}

async fn list_github_auth_repositories(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<GitHubRepositoriesResponse>, ApiError> {
    let session = state
        .github_auth_sessions
        .lock()
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "GitHub sign-in state is unavailable",
            )
        })?
        .get(&session_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(&session_id))?;
    if !matches!(
        *session.status.borrow(),
        GitHubAuthStatus::Authorized { .. }
    ) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "GitHub sign-in is not complete",
        ));
    }
    let raw = state
        .key_registry()
        .get(&session.credential_reference)
        .map_err(provider_transfer_error)?;
    let stored: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            format!("stored GitHub authorization is invalid: {error}"),
        )
    })?;
    let token: hotsheet_extsync::GitHubTokenBundle = serde_json::from_value(
        stored.get("token").cloned().unwrap_or_default(),
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            format!("stored GitHub authorization is invalid: {error}"),
        )
    })?;
    let client = hotsheet_extsync::GitHubDeviceClient::live(
        session.client_id.clone(),
        session.web_base.clone(),
    );
    let access = tokio::task::spawn_blocking(move || client.repository_access(&token.access_token))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;
    Ok(Json(GitHubRepositoriesResponse {
        repositories: access.repositories,
        installations: access.installations,
        install_url: access.install_url,
    }))
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
    // Store metadata only — never a ticket parse (HS2-4XXRJP).
    let mut descriptors = state
        .host
        .summaries()
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

/// `GET /checkouts/{reference}/providers` (HS2-3SCH1K): only the ticket sources this checkout
/// links, in its source order, marked default by the checkout's own default source rather than
/// the machine-wide registry. A linked connection that no longer exists is omitted.
async fn list_checkout_providers(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<hotsheet_ticketing::ProviderDescriptor>>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    // Hosting on demand keeps a linked git store listable after a sweep unhosted it.
    let hosted = checkout_entries(&state, &reference)?
        .into_iter()
        .map(|(id, _)| id)
        .collect::<std::collections::HashSet<_>>();
    let summaries = state.host.summaries();
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    let injected = state.injected_providers.descriptors();
    let mut descriptors = Vec::new();
    for source in &checkout.sources {
        let is_default = checkout.default_source.as_deref() == Some(source.connection_id.as_str());
        let descriptor = if source.provider == "git" {
            summaries
                .iter()
                .find(|info| info.id == source.connection_id && hosted.contains(&info.id))
                .map(|info| info.provider_descriptor(is_default))
        } else if let Some(connection) = connections
            .iter()
            .find(|connection| connection.id == source.connection_id)
        {
            Some(hotsheet_extsync::descriptor(connection).map_err(provider_transfer_error)?)
        } else {
            injected
                .iter()
                .find(|descriptor| descriptor.connection_id == source.connection_id)
                .cloned()
        };
        if let Some(mut descriptor) = descriptor {
            descriptor.default = is_default;
            descriptors.push(descriptor);
        }
    }
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

fn save_provider_connections(
    state: &AppState,
    connections: Vec<ProviderConnection>,
    connection: ProviderConnection,
    replacing: Option<&str>,
) -> Result<(), ApiError> {
    let connections = hotsheet_extsync::updated_connections(connections, connection, replacing)
        .map_err(|error| match error {
            hotsheet_ticketing::ProviderError::UnknownConnection(id) => ApiError::not_found(&id),
            hotsheet_ticketing::ProviderError::Conflict { message, .. }
                if message == "provider connection id already exists" =>
            {
                ApiError::new(StatusCode::CONFLICT, message)
            }
            hotsheet_ticketing::ProviderError::Conflict { message, .. }
                if message == "git connections are managed through the store registry" =>
            {
                ApiError::new(StatusCode::BAD_REQUEST, message)
            }
            other => provider_transfer_error(other),
        })?;
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .save(&connections)
        .map_err(provider_transfer_error)
}

fn connection_token(state: &AppState, connection: &ProviderConnection) -> Result<String, ApiError> {
    hotsheet_extsync::connection_access_token(
        connection,
        &state.key_registry(),
        OffsetDateTime::now_utc().unix_timestamp(),
    )
    .map_err(|error| match error {
        hotsheet_extsync::GitHubCredentialError::Provider(error) => provider_transfer_error(error),
        hotsheet_extsync::GitHubCredentialError::Secret(error) => provider_transfer_error(error),
        other => ApiError::new(StatusCode::UNAUTHORIZED, other.to_string()),
    })
}

async fn create_provider_connection(
    State(state): State<AppState>,
    Json(mut connection): Json<ProviderConnection>,
) -> Result<(StatusCode, Json<ProviderConnection>), ApiError> {
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    // Clients no longer ask users for an id; an empty one is generated (HS2-48GA17).
    if connection.id.trim().is_empty() {
        connection.id = hotsheet_ticketing::generate_connection_id(
            &connections,
            &connection.provider,
            &connection.locator,
        );
    }
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
    // Only the dedicated toggle changes whether a connection is disabled (HS2-SF6W34); an
    // ordinary settings edit keeps it as it was.
    connection.disabled = connections
        .iter()
        .any(|existing| existing.id == connection_id && existing.disabled);
    save_provider_connections(
        &state,
        connections,
        connection.clone(),
        Some(&connection_id),
    )?;
    // Checkout links copy the locator; an edit made for every project reaches each of them
    // (HS2-RCBKA3).
    state
        .checkout_registry
        .update_source_locator(&connection_id, &connection.locator)
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(connection))
}

#[derive(Debug, Deserialize)]
struct ProviderConnectionDisabledBody {
    disabled: bool,
}

/// Temporarily switch a connection off (or back on). While disabled, Hot Sheet neither reads
/// from nor writes to it: aggregate views skip it and direct operations fail explicitly
/// (HS2-SF6W34).
async fn set_provider_connection_disabled(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(body): Json<ProviderConnectionDisabledBody>,
) -> Result<Json<ProviderConnection>, ApiError> {
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .set_disabled(&connection_id, body.disabled)
        .map_err(provider_transfer_error)?
        .map(Json)
        .ok_or_else(|| ApiError::not_found(&connection_id))
}

/// Whether `connection_id` names a disabled external connection in `providers.json`.
fn connection_disabled(state: &AppState, connection_id: &str) -> Result<bool, ApiError> {
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .disabled(connection_id)
        .map_err(provider_transfer_error)
}

/// Permanently remove a connection and every local reference to it: checkout links and
/// defaults, the `providers.json` entry, and a Hot Sheet–minted credential. Idempotent —
/// removing an already-removed id still cleans dangling checkout links (HS2-724S9N).
async fn delete_provider_connection(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<hotsheet_ticketing::connection_removal::ConnectionRemoval>, ApiError> {
    tokio::task::spawn_blocking(move || {
        hotsheet_ticketing::connection_removal::remove_provider_connection(
            &ProviderConfigRegistry::new(state.store.root().join("providers.json")),
            &state.checkout_registry,
            &state.key_registry(),
            &connection_id,
        )
        .map(Json)
        .map_err(|error| match error {
            hotsheet_ticketing::connection_removal::ConnectionRemovalError::GitSource(_) => {
                ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
            }
            hotsheet_ticketing::connection_removal::ConnectionRemovalError::Provider(error) => {
                provider_transfer_error(error)
            }
            other => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        })
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
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
    for (id, _) in state.host.locations() {
        let entry = state
            .host
            .get(&id)
            .ok_or_else(|| ApiError::not_found(&id))?;
        registry
            .register(Arc::new(
                GitProvider::new(id.clone(), entry.store)
                    .with_default(id == default_id && !external_default),
            ))
            .map_err(provider_transfer_error)?;
    }
    for connection in connections {
        if connection.provider == "git" {
            continue;
        }
        let token = connection_token(state, &connection)?;
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
    // Refuse before building a client, so a disabled source is never contacted (HS2-SF6W34).
    if connection_disabled(state, connection_id)? {
        return Err(provider_transfer_error(
            hotsheet_ticketing::ProviderError::Disabled {
                connection_id: connection_id.into(),
            },
        ));
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
    let token = connection_token(state, &connection)?;
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
                // A copied standalone file is intentionally Uncategorized: retaining its
                // source batch id could falsely merge it with an unrelated destination batch.
                batch_id: None,
                batch_label: None,
                actor: None,
                purpose: None,
                annotations: metadata.annotations,
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

async fn restore_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    let Some(entry) = state.host.get(&connection_id) else {
        // Preserve not-found diagnostics for unknown connections, then report the
        // capability boundary for every configured non-git provider.
        let _ = provider_for(&state, &connection_id)?;
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "provider connection '{connection_id}' does not support git-backed Hot Sheet Trash restore"
            ),
        ));
    };
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let restored = ops::restore(&entry.store, &ticket.id, now())?;
    state.changed_in(&entry, "updated", &restored);
    Ok(Json(api_ticket(&entry, &restored)?))
}

async fn create_provider_ticket(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    Ok((
        StatusCode::CREATED,
        Json(do_provider_create(&state, &connection_id, req)?),
    ))
}

fn do_provider_create(
    state: &AppState,
    connection_id: &str,
    req: CreateReq,
) -> Result<ApiTicket, ApiError> {
    let priority = opt_parse(req.priority.as_deref())?.unwrap_or_default();
    let status = initial_status(req.status.as_deref())?;
    let provider = provider_for(state, connection_id)?;
    let new = ops::normalize_new_ticket_input(NewTicket {
        title: req.title,
        category: req.category.unwrap_or_else(|| "issue".into()),
        priority,
        status,
        details: req.details.unwrap_or_default(),
        tags: req.tags.unwrap_or_default(),
        up_next: req.up_next.unwrap_or(false),
        blocked_by: Vec::new(),
    });
    provider
        .create(
            MutationContext {
                now: now(),
                generated_id: Ulid::new(),
            },
            ProviderDraft {
                title: new.title,
                category: new.category,
                priority: new.priority,
                status: new.status,
                details: new.details,
                tags: new.tags,
                up_next: new.up_next,
                blocked_by: req.blocked_by.unwrap_or_default(),
                transfer: None,
            },
        )
        .map_err(provider_transfer_error)
}

async fn update_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    Ok(Json(do_provider_update(&state, &connection_id, &id, req)?))
}

fn do_provider_update(
    state: &AppState,
    connection_id: &str,
    id: &str,
    req: UpdateReq,
) -> Result<ApiTicket, ApiError> {
    let provider = provider_for(state, connection_id)?;
    if req.note_id.is_some() && !provider.supports_note_edit() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("provider connection '{connection_id}' does not support note editing"),
        ));
    }
    if req.note_id.is_some() && req.note_summary.is_some() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "note_summary is only valid when appending a note",
        ));
    }
    if req.note_summary.is_some() && req.note.as_deref().is_none_or(str::is_empty) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "note_summary requires a non-empty note",
        ));
    }
    let confidence_change = req.note_confidence_change(req.note_id.is_some())?;
    // An append's null means "no score"; only an edit can clear one (HS2-CY4CWC).
    let note_confidence = confidence_change.flatten();
    if (note_confidence.is_some() || (req.note_id.is_some() && confidence_change.is_some()))
        && !provider.supports_note_confidence()
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("provider connection '{connection_id}' does not support note confidence"),
        ));
    }
    let status = opt_parse(req.status.as_deref())?;
    let actor = parse_actor(req.actor.as_ref())?;
    if actor
        .as_ref()
        .is_some_and(hotsheet_ticketing::actor::MutationActor::is_ai)
        && status == Some(Status::Completed)
    {
        let current = provider.get(id).map_err(provider_transfer_error)?;
        let scores_now = match &req.note_id {
            Some(_) => matches!(confidence_change, Some(Some(_))),
            None => req.note.is_some() && note_confidence.is_some(),
        };
        hotsheet_ticketing::actor::check_completion(
            actor.as_ref(),
            &current.slug,
            hotsheet_ticketing::actor::completes(current.status, status),
            scores_now || hotsheet_ticketing::actor::api_scored_in_current_cycle(&current),
        )?;
    }
    let timestamp = now();
    let note = req.note.clone();
    let note_id = req.note_id.clone();
    let note_kind = req.note_kind.unwrap_or(NoteKind::Regular);
    let note_summary = req.note_summary.clone();
    let mut ticket = provider
        .update(
            id,
            timestamp.clone(),
            ProviderPatch {
                actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
                expected_token: req.expected_token,
                title: req.title,
                details: req.details,
                category: req.category,
                priority: opt_parse(req.priority.as_deref())?,
                status,
                tags: req.tags,
                up_next: req.up_next,
                blocked_by: req.blocked_by,
                blocked_reason: req.blocked_reason,
            },
        )
        .map_err(provider_transfer_error)?;
    let edit = ops::NoteEditInput {
        text: note,
        confidence: confidence_change,
    };
    match (note_id, edit) {
        (Some(note_id), edit) if !edit.is_empty() => {
            ticket = provider
                .edit_note_with_metadata(id, &note_id, timestamp, edit)
                .map_err(provider_transfer_error)?;
        }
        (
            None,
            ops::NoteEditInput {
                text: Some(note), ..
            },
        ) => {
            ticket = provider
                .add_note_with_metadata(
                    id,
                    MutationContext {
                        now: timestamp,
                        generated_id: Ulid::new(),
                    },
                    note_kind,
                    ops::NoteMetadataInput {
                        summary: note_summary,
                        confidence: note_confidence,
                        actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
                    },
                    note,
                )
                .map_err(provider_transfer_error)?;
        }
        _ => {}
    }
    reindex_hosted_provider_write(state, connection_id, id);
    Ok(ticket)
}

/// A provider-route write to a hosted git store reindexes and broadcasts the ticket at
/// once, like the legacy route, so an immediate list read sees it (read-your-writes; the
/// derived `latest_confidence` column depends on it, HS2-RD4M29). External providers are
/// authoritative remotely and have no local index row.
fn reindex_hosted_provider_write(state: &AppState, connection_id: &str, native_id: &str) {
    let Some(entry) = state.host.get(connection_id) else {
        return;
    };
    let Ok(id) = Ulid::from_string(native_id) else {
        return;
    };
    if let Ok(ticket) = entry.store.read_ticket(&id) {
        state.changed_in(&entry, "updated", &ticket);
    }
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
    let duplicate_of = match req.duplicate_of {
        Some(DuplicateOfReq::Legacy(reference)) => Some(reference),
        Some(DuplicateOfReq::Qualified(reference)) => {
            let target = resolve_project_ticket_ref(&state, reference)?;
            // This compatibility route is provider-scoped rather than checkout-scoped,
            // so it has no source project identity to compare. Treat the same provider
            // connection/native pair as the same underlying ticket. Checkout routes use
            // all three ProjectTicketRef fields below.
            if target.connection_id == connection_id && target.native_id == id {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "a ticket cannot be a duplicate of itself",
                ));
            }
            Some(target.qualified())
        }
        None => None,
    };
    let provider = provider_for(&state, &connection_id)?;
    let reason: CloseReason = opt_parse(Some(&req.reason))?.expect("required close reason");
    let actor = parse_actor(req.actor.as_ref())?;
    if actor
        .as_ref()
        .is_some_and(hotsheet_ticketing::actor::MutationActor::is_ai)
    {
        let current = provider.get(&id).map_err(provider_transfer_error)?;
        hotsheet_ticketing::actor::check_completion(
            actor.as_ref(),
            &current.slug,
            hotsheet_ticketing::actor::close_completes(current.status, reason),
            hotsheet_ticketing::actor::api_scored_in_current_cycle(&current),
        )?;
    }
    let closed = provider
        .close(&id, now(), reason, duplicate_of)
        .map_err(provider_transfer_error)?;
    reindex_hosted_provider_write(&state, &connection_id, &id);
    Ok(Json(closed))
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
    #[serde(default)]
    sources: Vec<hotsheet_ticketing::checkouts::TicketSource>,
    default_source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenProjectBody {
    root: String,
    alias: Option<String>,
    repository: Option<String>,
    /// Explicit git stores. When omitted, conservative filesystem discovery is used.
    stores: Option<Vec<String>>,
    /// Explicit provider-neutral sources. When supplied, these are authoritative.
    sources: Option<Vec<hotsheet_ticketing::checkouts::TicketSource>>,
    default_source: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenProjectResponse {
    checkout: hotsheet_ticketing::checkouts::Checkout,
    discovered: bool,
}

fn schedule_setup_freshness(state: &AppState, checkout: &hotsheet_ticketing::checkouts::Checkout) {
    let settings = checkout.settings();
    let source = checkout
        .default_source
        .as_deref()
        .and_then(|id| checkout.source(id))
        .filter(|source| source.provider == "git")
        .or_else(|| {
            checkout
                .sources
                .iter()
                .find(|source| source.provider == "git")
        });
    let Some(source) = source else {
        tokio::task::spawn_blocking(move || {
            let _ = settings.migrate_existing();
        });
        return;
    };
    let id = checkout.id.clone();
    if !state
        .setup_refreshes
        .lock()
        .is_ok_and(|mut active| active.insert(id.clone()))
    {
        return;
    }
    let active = state.setup_refreshes.clone();
    let project = std::path::PathBuf::from(&checkout.root);
    let store = std::path::PathBuf::from(&source.locator);
    let plugin_dirs = state.plugin_dirs.as_ref().clone();
    tokio::task::spawn_blocking(move || {
        let _ = settings.migrate_existing();
        // Same resolution as the CLI: an explicit empty list disables every tool (HS2-8B3VJP).
        let enabled = hotsheet_plugins::enabled_plugins_from_setting(
            settings
                .get("enabled_plugins", hotsheet_ticketing::Scope::Shared)
                .ok()
                .flatten()
                .as_ref(),
        );
        let _ =
            hotsheet_plugins::refresh_setup_in(&store, &project, enabled.as_ref(), &plugin_dirs);
        if let Ok(mut active) = active.lock() {
            active.remove(&id);
        }
    });
}

/// Keep the generated checkout projection fresh without making project opening wait for a
/// full scan of every linked ticket store. The short delay also gives the client's initial
/// ticket-index requests priority over this best-effort local projection refresh.
fn regenerate_checkout_worklist_indexed(
    host: &StoreHost,
    checkout: &hotsheet_ticketing::checkouts::Checkout,
) -> anyhow::Result<usize> {
    let query = TicketQuery {
        up_next_only: true,
        open_only: true,
        ..TicketQuery::default()
    };
    let mut tickets = std::collections::BTreeMap::new();
    for source in checkout
        .sources
        .iter()
        .filter(|source| source.provider == "git")
    {
        let entry = host.get(&source.connection_id).ok_or_else(|| {
            anyhow::anyhow!("checkout links an unhosted store: {}", source.locator)
        })?;
        let rows = entry
            .index
            .lock()
            .map_err(|_| anyhow::anyhow!("index lock poisoned"))?
            .query(&query)?;
        for row in rows {
            let id = Ulid::from_string(&row.id)?;
            if let std::collections::btree_map::Entry::Vacant(ticket) = tickets.entry(id) {
                ticket.insert(entry.store.read_ticket(&id)?);
            }
        }
    }
    Ok(
        hotsheet_ticketing::worklist::regenerate_checkout_from_tickets(
            checkout,
            &tickets.into_values().collect::<Vec<_>>(),
        )?,
    )
}

fn schedule_worklist_regeneration(
    state: &AppState,
    checkout: &hotsheet_ticketing::checkouts::Checkout,
) {
    let checkout = checkout.clone();
    let host = state.host.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let root = checkout.root.clone();
        match tokio::task::spawn_blocking(move || {
            regenerate_checkout_worklist_indexed(&host, &checkout)
        })
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => eprintln!("worklist regenerate failed for {root}: {error}"),
            Err(error) => eprintln!("worklist regenerate task failed for {root}: {error}"),
        }
    });
}

/// Open a code checkout for client use: discover or accept its git ticket stores, host
/// them in this machine server, and persist the checkout-to-store links atomically from
/// the client's point of view. An empty result is valid and lets a settings UI ask the
/// user to choose one or more providers explicitly.
///
/// Store discovery (a directory walk) and hosting each git source (which opens and
/// reconciles or rebuilds its index, parsing every ticket file) run on the blocking pool,
/// so opening a project with a large store never occupies an async request thread
/// (HS2-2VBN8Y).
async fn open_project(
    State(state): State<AppState>,
    Json(body): Json<OpenProjectBody>,
) -> Result<(StatusCode, Json<OpenProjectResponse>), ApiError> {
    let discovered = body.stores.is_none() && body.sources.is_none();
    let hosting_state = state.clone();
    let discovery_root = body.root.clone();
    let explicit_stores = body.stores;
    let explicit_sources = body.sources;
    let sources = tokio::task::spawn_blocking(move || {
        let stores = match explicit_stores {
            Some(paths) => paths.into_iter().map(std::path::PathBuf::from).collect(),
            None if explicit_sources.is_none() => {
                hotsheet_ticketing::checkouts::discover_ticket_stores(FsPath::new(&discovery_root))
                    .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?
            }
            None => Vec::new(),
        };
        let mut sources = explicit_sources.unwrap_or_default();
        sources.extend(
            stores
                .iter()
                .cloned()
                .map(hotsheet_ticketing::checkouts::TicketSource::git),
        );
        for source in sources.iter().filter(|source| source.provider == "git") {
            let store = FsStore::open(&source.locator)
                .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
            hosting_state.host_project_store(store)?;
        }
        Ok::<_, ApiError>(sources)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    let root = FsPath::new(&body.root);
    let default_source = body
        .default_source
        .or_else(|| (sources.len() == 1).then(|| sources[0].connection_id.clone()));
    let checkout = state
        .checkout_registry
        .register_sources(
            root,
            body.alias.as_deref(),
            body.repository,
            sources,
            default_source,
        )
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    state.watch_checkout_repository(&checkout);
    schedule_worklist_regeneration(&state, &checkout);
    schedule_setup_freshness(&state, &checkout);
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
    let mut sources = body.sources;
    sources.extend(
        body.stores
            .into_iter()
            .map(hotsheet_ticketing::checkouts::TicketSource::git),
    );
    let default_source = body
        .default_source
        .or_else(|| (sources.len() == 1).then(|| sources[0].connection_id.clone()));
    let entry = state
        .checkout_registry
        .register_sources(
            FsPath::new(&body.root),
            body.alias.as_deref(),
            body.repository,
            sources,
            default_source,
        )
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    hotsheet_ticketing::worklist::regenerate_checkout(&entry)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state.watch_checkout_repository(&entry);
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

#[derive(Deserialize)]
struct CheckoutSourceBody {
    provider: String,
    locator: String,
    #[serde(default)]
    make_default: bool,
}

async fn add_checkout_source(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
    Json(body): Json<CheckoutSourceBody>,
) -> Result<Json<hotsheet_ticketing::checkouts::Checkout>, ApiError> {
    let source = if body.provider == "git" {
        // Hosting a new store builds its index (a full ticket parse): keep it off the
        // async request threads (HS2-2VBN8Y).
        let hosting_state = state.clone();
        let locator = body.locator;
        let store = tokio::task::spawn_blocking(move || {
            let store = FsStore::open(&locator)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
            hosting_state.host_store(store.clone())?;
            Ok::<_, ApiError>(store)
        })
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
        let source = hotsheet_ticketing::checkouts::TicketSource::git(store.root());
        if source.connection_id != connection_id {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("git source id must be {}", source.connection_id),
            ));
        }
        source
    } else {
        hotsheet_ticketing::checkouts::TicketSource {
            connection_id,
            provider: body.provider,
            locator: body.locator,
        }
    };
    state
        .checkout_registry
        .add_source(&reference, source, body.make_default)
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

async fn remove_checkout_source(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
) -> Result<Json<hotsheet_ticketing::checkouts::Checkout>, ApiError> {
    state
        .checkout_registry
        .remove_source(&reference, &connection_id)
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

#[derive(Deserialize)]
struct CheckoutDefaultSourceBody {
    connection_id: Option<String>,
}

async fn set_checkout_default_source(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(body): Json<CheckoutDefaultSourceBody>,
) -> Result<Json<hotsheet_ticketing::checkouts::Checkout>, ApiError> {
    state
        .checkout_registry
        .set_default_source(&reference, body.connection_id.as_deref())
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

async fn checkout_repository_status(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<repository_browser::RepositoryOverview>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || repository_browser::discover(&root))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository discovery task failed: {error}"),
            )
        })?
        .map(Json)
        .map_err(repository_browser_api_error)
}

async fn initialize_checkout_repository(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<repository_browser::RepositoryOverview>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.clone().into();
    let overview = tokio::task::spawn_blocking(move || repository_browser::initialize(&root))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository initialization task failed: {error}"),
            )
        })?
        .map_err(repository_setup_api_error)?;
    state.watch_checkout_repository(&checkout);
    state.emit(repository_change_event(&checkout.id));
    Ok(Json(overview))
}

#[derive(Deserialize)]
struct RepositoryRemoteBody {
    remote: String,
}

async fn configure_checkout_repository_remote(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(body): Json<RepositoryRemoteBody>,
) -> Result<Json<repository_browser::RepositoryOverview>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.clone().into();
    let overview = tokio::task::spawn_blocking(move || {
        repository_browser::configure_origin(&root, &body.remote)
    })
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("repository remote task failed: {error}"),
        )
    })?
    .map_err(repository_setup_api_error)?;
    state.emit(repository_change_event(&checkout.id));
    Ok(Json(overview))
}

async fn checkout_repository_files(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(query): Query<repository_browser::RepositoryFilePageQuery>,
) -> Result<
    Json<repository_browser::RepositoryPage<hotsheet_ticketing::repository_status::RepositoryFile>>,
    ApiError,
> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || repository_browser::files_page(&root, query))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository file discovery task failed: {error}"),
            )
        })?
        .map(Json)
        .map_err(repository_browser_api_error)
}

async fn checkout_repository_commits(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(query): Query<repository_browser::RepositoryPageQuery>,
) -> Result<Json<code_review::CodeReviewPage>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || repository_browser::commits_page(&root, query))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository commit discovery task failed: {error}"),
            )
        })?
        .map(Json)
        .map_err(repository_browser_api_error)
}

async fn open_checkout_repository_review(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(target): Json<code_review::ReviewTarget>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || {
        let status = hotsheet_ticketing::repository_status::snapshot(&root)?;
        code_review::launch_repository(&root, status.ahead as usize, &target)
            .map_err(repository_browser::RepositoryBrowserError::from)
    })
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("repository review launch task failed: {error}"),
        )
    })?
    .map_err(repository_browser_api_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn checkout_repository_file_action(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(request): Json<repository_browser::RepositoryFileActionRequest>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || repository_browser::act_on_file(&root, &request))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository file action task failed: {error}"),
            )
        })?
        .map_err(repository_browser_api_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn repository_browser_api_error(error: repository_browser::RepositoryBrowserError) -> ApiError {
    use repository_browser::RepositoryBrowserError;
    let status = match error {
        RepositoryBrowserError::UnknownFile
        | RepositoryBrowserError::UnsafePath
        | RepositoryBrowserError::MissingFile
        | RepositoryBrowserError::Review(code_review::CodeReviewError::InvalidTarget) => {
            StatusCode::BAD_REQUEST
        }
        RepositoryBrowserError::Review(code_review::CodeReviewError::DifftoolNotConfigured) => {
            StatusCode::CONFLICT
        }
        RepositoryBrowserError::Status(_)
        | RepositoryBrowserError::Review(code_review::CodeReviewError::NotRepository) => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        RepositoryBrowserError::Review(_) | RepositoryBrowserError::Launch(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    ApiError::new(status, error.to_string())
}

fn repository_setup_api_error(error: repository_browser::RepositorySetupError) -> ApiError {
    use repository_browser::RepositorySetupError;
    let status = match error {
        RepositorySetupError::InvalidRemote => StatusCode::BAD_REQUEST,
        RepositorySetupError::NotRepository | RepositorySetupError::OriginAlreadyConfigured(_) => {
            StatusCode::CONFLICT
        }
        RepositorySetupError::Git { .. } | RepositorySetupError::Discovery(_) => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        RepositorySetupError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.to_string())
}

async fn get_checkout_code_review(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<code_review::CodeReview>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let (_, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let slug = ticket.slug;
    let classification = checkout
        .settings()
        .get_effective("code_review_file_classes")
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
        .map(serde_json::from_value::<code_review::CodeReviewClassification>)
        .transpose()
        .map_err(|error| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("invalid code_review_file_classes setting: {error}"),
            )
        })?
        .unwrap_or_default();
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || {
        code_review::discover_with_classification(&root, &slug, &classification)
    })
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("code review discovery task failed: {error}"),
        )
    })?
    .map(Json)
    .map_err(code_review_api_error)
}

async fn open_checkout_code_review(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(target): Json<code_review::ReviewTarget>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let (_, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let root: std::path::PathBuf = checkout.root.into();
    let slug = ticket.slug;
    tokio::task::spawn_blocking(move || {
        let review = code_review::discover(&root, &slug)?;
        code_review::launch(&root, &review, &target)
    })
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("code review launch task failed: {error}"),
        )
    })?
    .map_err(code_review_api_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn code_review_api_error(error: code_review::CodeReviewError) -> ApiError {
    let status = match error {
        code_review::CodeReviewError::NotRepository => StatusCode::UNPROCESSABLE_ENTITY,
        code_review::CodeReviewError::DifftoolNotConfigured => StatusCode::CONFLICT,
        code_review::CodeReviewError::InvalidTarget => StatusCode::BAD_REQUEST,
        code_review::CodeReviewError::Git(_) | code_review::CodeReviewError::Launch(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    ApiError::new(status, error.to_string())
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
    for source in checkout
        .sources
        .into_iter()
        .filter(|source| source.provider == "git")
    {
        let store_path = source.locator;
        let canonical = FsPath::new(&store_path)
            .canonicalize()
            .unwrap_or_else(|_| store_path.clone().into());
        let find =
            || {
                state.host.locations().into_iter().find(|(_, root)| {
                    root.canonicalize().unwrap_or_else(|_| root.clone()) == canonical
                })
            };
        // A sweep may have unhosted it while the project stayed open (HS2-ARJ9J1).
        let found = find().or_else(|| {
            let store = FsStore::open(&store_path).ok()?;
            state.host_project_store(store).ok()?;
            find()
        });
        let Some((store_id, _)) = found else {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!("checkout {reference} links an unhosted store: {store_path}"),
            ));
        };
        if let Some(entry) = state.host.get(&store_id) {
            entries.push((store_id, entry));
        }
    }
    Ok(entries)
}

/// List a checkout's tickets. `page_size` selects the bounded page envelope with a
/// continuation cursor; otherwise the response is a plain array capped by `limit`,
/// the shape MCP `hotsheet_query` and client search/lookup use. Both shapes share one
/// global order across every local and hosted-provider source (HS2-M0YTB6).
/// Response header set when an explicitly `limit`ed unpaged checkout array omitted
/// further matching rows (HS2-CYXS0N).
const TRUNCATED_HEADER: &str = "x-hotsheet-truncated";

/// `GET /checkouts/{reference}/tickets`. With `page_size` it returns the bounded page
/// envelope. Without it, a plain row array in the same global order, bounded by
/// `CHECKOUT_READ_MAX_ROWS` (HS2-CYXS0N): a caller's explicit `limit` truncates, flagged by
/// `x-hotsheet-truncated: true`, while an implicit read that would exceed the bound fails
/// with 400 so a large checkout is never serialized into one response or silently cut.
async fn list_checkout_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<axum::response::Response, ApiError> {
    if params.page_size.is_some() {
        return list_checkout_ticket_page(&state, &reference, params)
            .map(|page| Json(page).into_response());
    }
    if params.cursor.is_some() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "cursor requires page_size",
        ));
    }
    let cap = match params.limit {
        None => CHECKOUT_READ_MAX_ROWS,
        Some(limit) if limit <= CHECKOUT_READ_MAX_ROWS => limit,
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!(
                    "limit must be at most {CHECKOUT_READ_MAX_ROWS}; page larger reads with page_size and cursor"
                ),
            ));
        }
    };
    if cap == 0 {
        // Still validate the checkout and query even though no row is wanted.
        merge_checkout_page(&state, &reference, &params, 1, false)?;
        return Ok(Json(serde_json::Value::Array(Vec::new())).into_response());
    }
    let (items, next_cursor, _) = merge_checkout_page(&state, &reference, &params, cap, false)?;
    let truncated = next_cursor.is_some();
    if truncated && params.limit.is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "more than {CHECKOUT_READ_MAX_ROWS} tickets match; pass limit (at most {CHECKOUT_READ_MAX_ROWS}) or page with page_size and cursor"
            ),
        ));
    }
    let mut response = Json(serde_json::Value::Array(items)).into_response();
    if truncated {
        response.headers_mut().insert(
            TRUNCATED_HEADER,
            axum::http::HeaderValue::from_static("true"),
        );
    }
    Ok(response)
}

impl From<checkout_page::CheckoutPageError> for ApiError {
    fn from(error: checkout_page::CheckoutPageError) -> Self {
        use checkout_page::CheckoutPageError;
        let status = match error {
            CheckoutPageError::StaleCursor
            | CheckoutPageError::InvalidCursor
            | CheckoutPageError::InvalidSummaryDays
            | CheckoutPageError::InvalidCounts => StatusCode::BAD_REQUEST,
            CheckoutPageError::NonAdvancing | CheckoutPageError::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        ApiError::new(status, error.to_string())
    }
}

/// Fold an index summary into checkout counts.
fn index_summary(summary: TicketSummary) -> hotsheet_ticketing::ProviderTicketSummary {
    hotsheet_ticketing::ProviderTicketSummary {
        total: summary.total,
        queued: summary.queued,
        backlog: summary.backlog,
        archive: summary.archive,
        trash: summary.trash,
        open: summary.open,
        up_next: summary.up_next,
        active: summary.active,
        started: summary.started,
        verified: summary.verified,
        completed_today: summary.completed_today,
        completion_trend: summary.completion_trend,
    }
}

#[cfg(test)]
mod checkout_pagination_tests {
    use super::ListParams;
    use hotsheet_ticketing::checkout_page::filter_fingerprint;

    fn fingerprint(query: &str) -> String {
        let params: ListParams = serde_urlencoded_params(query);
        let has_commit = params.has_commit;
        let query = params.into_query(std::path::Path::new(".")).unwrap();
        filter_fingerprint(&query, has_commit)
    }

    fn serde_urlencoded_params(query: &str) -> ListParams {
        let uri: axum::http::Uri = format!("/?{query}").parse().unwrap();
        axum::extract::Query::<ListParams>::try_from_uri(&uri)
            .unwrap()
            .0
    }

    #[test]
    fn filter_fingerprint_ignores_order_and_paging_but_binds_every_filter() {
        let base = fingerprint("tags=a,b&status=started&text=x");
        assert_eq!(base, fingerprint("text=x&tags=b,a,a&status=started"));
        assert_eq!(
            base,
            fingerprint(
                "tags=a,b&status=started&text=x&sort=title&direction=descending&limit=3&page_size=9&cursor=v1.00"
            ),
            "ordering is bound separately; paging parameters are not filters"
        );
        assert_eq!(fingerprint(""), fingerprint("tags=&attachment="));
        for changed in [
            "tags=a&status=started&text=x",
            "tags=a,b&status=completed&text=x",
            "tags=a,b&status=started&text=y",
            "tags=a,b&status=started&text=x&up_next=true",
            "tags=a,b&status=started&text=x&has_commit=true",
            "tags=a,b&status=started&text=x&has_commit=false",
            "tags=a,b&status=started&text=x&updated_after=2026-01-01",
            "tags=a,b&status=started&text=x&collection=queue",
        ] {
            assert_ne!(base, fingerprint(changed), "{changed}");
        }
    }
}

use hotsheet_ticketing::checkout_order::CHECKOUT_READ_MAX_ROWS;

fn list_checkout_ticket_page(
    state: &AppState,
    reference: &str,
    params: ListParams,
) -> Result<serde_json::Value, ApiError> {
    let page_size = params.page_size.unwrap_or(checkout_page::DEFAULT_PAGE_SIZE);
    if page_size == 0 || page_size > CHECKOUT_READ_MAX_ROWS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("page_size must be between 1 and {CHECKOUT_READ_MAX_ROWS}"),
        ));
    }
    let with_counts = params.counts.unwrap_or(true);
    let (items, next_cursor, counts) =
        merge_checkout_page(state, reference, &params, page_size, with_counts)?;
    serde_json::to_value(checkout_page::CheckoutTicketPage {
        items,
        next_cursor,
        counts: with_counts.then_some(counts),
    })
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

/// One bounded page of the checkout-wide merge: at most `page_size` rows in the global
/// order, the continuation cursor when a row remains, and (when requested) exact counts.
/// Both the paged envelope and the capped unpaged array read through here; the merge and
/// cursor codec are shared with the serverless MCP backend (`checkout_page`, HS2-JVF20F).
fn merge_checkout_page(
    state: &AppState,
    reference: &str,
    params: &ListParams,
    page_size: usize,
    with_counts: bool,
) -> Result<
    (
        Vec<serde_json::Value>,
        Option<String>,
        checkout_page::CheckoutTicketCounts,
    ),
    ApiError,
> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let contexts = auto_context::effective(&checkout.settings())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let entries = checkout_entries(state, reference)?;
    // A disabled source is left out of the merged view rather than failing it (HS2-SF6W34).
    let mut external_sources = Vec::new();
    for source in checkout
        .sources
        .iter()
        .filter(|source| source.provider != "git")
    {
        if !connection_disabled(state, &source.connection_id)? {
            external_sources.push(source);
        }
    }
    let source_keys = entries
        .iter()
        .map(|(store_id, _)| checkout_page::git_source_key(store_id))
        .chain(
            external_sources
                .iter()
                .map(|source| checkout_page::provider_source_key(&source.connection_id)),
        )
        .collect::<Vec<_>>();
    let merge_query = params.clone().into_query(state.store.root())?;
    let sort = merge_query.sort;
    let descending = merge_query.descending;
    let filters = checkout_page::filter_fingerprint(&merge_query, params.has_commit);
    // Validate the cursor before paying for counts.
    checkout_page::decode_cursor(
        params.cursor.as_deref(),
        &source_keys,
        sort,
        descending,
        &filters,
    )?;

    let now = OffsetDateTime::now_utc();
    let now_text = now
        .format(&Rfc3339)
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let day_starts = checkout_page::completion_day_starts(params.summary_days.as_deref(), now)?;
    let mut counts = checkout_page::CheckoutTicketCounts::for_days(&day_starts);
    for (_, entry) in entries.iter().filter(|_| with_counts) {
        let summary = entry
            .index
            .lock()
            .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned"))?
            .summary(&now_text, &day_starts)?;
        counts.add(index_summary(summary));
    }
    for source in external_sources.iter().filter(|_| with_counts) {
        counts.add(
            provider_for(state, &source.connection_id)?
                .summary(&now_text, &day_starts)
                .map_err(provider_transfer_error)?,
        );
    }

    let compact = params.compact.unwrap_or(true);
    let fields = parse_fields(&params.fields);
    // Batched `has_commit` evaluation: one repository scan per fetched batch, not per row.
    let commit_filter = |slugs: Vec<String>| -> Result<Option<HashSet<String>>, ApiError> {
        if params.has_commit.is_none() || slugs.is_empty() {
            return Ok(None);
        }
        match code_review::slugs_with_commits(FsPath::new(&checkout.root), &slugs) {
            Ok(matches) => Ok(Some(matches)),
            Err(code_review::CodeReviewError::NotRepository) => Ok(Some(HashSet::new())),
            Err(error) => Err(code_review_api_error(error)),
        }
    };
    let keeps = |matches: &Option<HashSet<String>>, slug: &str| {
        params
            .has_commit
            .is_none_or(|want| matches.as_ref().is_some_and(|set| set.contains(slug)) == want)
    };
    // One bounded batch of rows strictly after the last row a source fetched (a value
    // keyset, HS2-74H84S). Rows removed by post-filters still advance the source, and a
    // batch is one index query or one provider keyset read (HS2-BGZ0NY).
    let fetch =
        |request: checkout_page::SourceFetch<'_>| -> Result<checkout_page::SourceBatch, ApiError> {
            if request.source < entries.len() {
                let (store_id, entry) = &entries[request.source];
                let mut query = params.clone().into_query(entry.store.root())?;
                query.limit = Some(request.want);
                query.page_after = None;
                query.after_key = request.after.cloned().map(|key| AfterKey {
                    key,
                    connection_id: store_id.clone(),
                });
                let rows = entry
                    .index
                    .lock()
                    .map_err(|_| {
                        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned")
                    })?
                    .query(&query)?;
                let exhausted = rows.len() < request.want;
                let matches = commit_filter(rows.iter().map(|row| row.slug.clone()).collect())?;
                let mut batch = Vec::with_capacity(rows.len());
                for mut row in rows {
                    row.set_connection(store_id);
                    let key = MergeKey::from_row(&row);
                    if !keeps(&matches, &row.slug) {
                        batch.push(checkout_page::SourceRow {
                            key,
                            resume: None,
                            value: None,
                        });
                        continue;
                    }
                    if compact {
                        row.make_compact();
                    }
                    row.add_auto_context(&contexts);
                    let mut value = serde_json::to_value(row).map_err(|error| {
                        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    // Project first: every checkout row keeps its source `store`.
                    hotsheet_ticketing::wire::project_fields(
                        std::slice::from_mut(&mut value),
                        &fields,
                    );
                    if let Some(object) = value.as_object_mut() {
                        object.insert("store".into(), store_id.clone().into());
                    }
                    batch.push(checkout_page::SourceRow {
                        key,
                        resume: None,
                        value: Some(value),
                    });
                }
                return Ok(checkout_page::SourceBatch {
                    rows: batch,
                    exhausted,
                });
            }
            let source = external_sources[request.source - entries.len()];
            let provider = provider_for(state, &source.connection_id)?;
            let query = params.clone().into_query(state.store.root())?;
            let page = provider
                .query_after(
                    &hotsheet_ticketing::unbounded_query(&query),
                    request.after,
                    request.resume,
                    request.want,
                )
                .map_err(provider_transfer_error)?;
            let matches = commit_filter(
                page.items
                    .iter()
                    .map(|item| item.ticket.slug.clone())
                    .collect(),
            )?;
            let mut batch = Vec::with_capacity(page.items.len());
            for item in page.items {
                let mut ticket = item.ticket;
                let key = MergeKey::from_ticket(&ticket);
                if !keeps(&matches, &ticket.slug) {
                    batch.push(checkout_page::SourceRow {
                        key,
                        resume: item.resume,
                        value: None,
                    });
                    continue;
                }
                ticket.auto_context =
                    auto_context::resolve_fields(&ticket.category, &ticket.tags, &contexts);
                if compact {
                    ticket.details.clear();
                }
                let mut value = serde_json::to_value(ticket).map_err(|error| {
                    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
                hotsheet_ticketing::wire::project_fields(std::slice::from_mut(&mut value), &fields);
                if let Some(object) = value.as_object_mut() {
                    object.insert("store".into(), source.connection_id.clone().into());
                }
                batch.push(checkout_page::SourceRow {
                    key,
                    resume: item.resume,
                    value: Some(value),
                });
            }
            Ok(checkout_page::SourceBatch {
                rows: batch,
                exhausted: page.exhausted,
            })
        };
    let page = checkout_page::merge_page(
        &source_keys,
        params.cursor.as_deref(),
        sort,
        descending,
        &filters,
        page_size,
        fetch,
    )?;
    Ok((page.items, page.next_cursor, counts))
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

/// `GET /checkouts/{reference}/corrupt-tickets` — every unparseable ticket file across the
/// checkout's hosted git sources.
///
/// Each hosted store keeps a stat-validated [`hotsheet_ticketing::CorruptTicketCache`]
/// (HS2-KYSBT2): a request lists and `stat`s the ticket tree and re-parses only files
/// whose fingerprint changed, so the answer always matches the disk without a full parse
/// on every project activation. The cache is prewarmed when a store is hosted. The walk
/// still runs on the blocking pool so a cold cache or a slow disk never occupies an async
/// request thread (HS2-QV8B7R). It is not shared with the `/health` single-flight scan:
/// that scan covers only the primary store and may answer from a stale listing, whereas
/// the ticket list filters rows by this response and needs the current state of every
/// linked source.
async fn list_checkout_corrupt_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<CheckoutCorruptTicket>>, ApiError> {
    let result = tokio::task::spawn_blocking(move || {
        let mut result = Vec::new();
        for (store_id, entry) in checkout_entries(&state, &reference)? {
            let store_path = entry.store.root().display().to_string();
            let listed = entry
                .corrupt
                .lock()
                .map_err(|_| {
                    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "corrupt cache poisoned")
                })?
                .corrupt_tickets(&entry.store)?;
            for corrupt in listed {
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
        Ok::<_, ApiError>(result)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    Ok(Json(result))
}

/// Fill a newly hosted store's corrupt-ticket cache in the background, so the first
/// project activation's `corrupt-tickets` request finds it warm (or coalesces onto the
/// scan in progress under the cache lock) instead of starting a full parse (HS2-KYSBT2).
fn prewarm_corrupt_tickets(entry: StoreEntry) {
    let _ = std::thread::Builder::new()
        .name("hs-corrupt-prewarm".into())
        .spawn(move || {
            if let Ok(mut cache) = entry.corrupt.lock() {
                let _ = cache.corrupt_tickets(&entry.store);
            }
        });
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
    let (_, settings) = checkout_settings(&state, &reference)?;
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
            return Ok((
                StatusCode::OK,
                Json(api_ticket_with_settings(&entry, existing, &settings)?),
            )
                .into_response());
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
                status: None,
                details: Some(details),
                tags: Some(vec!["corrupt-ticket".into(), "ai-repair".into()]),
                up_next: Some(true),
                blocked_by: None,
            },
        )?;
        return Ok((
            StatusCode::CREATED,
            Json(contextualize_api_ticket(created, &settings)?),
        )
            .into_response());
    }
    Err(ApiError::new(
        StatusCode::NOT_FOUND,
        "The corrupt ticket is no longer present in this checkout.",
    ))
}

#[derive(Deserialize)]
struct CheckoutStoreQuery {
    store: Option<String>,
    source: Option<String>,
}
fn checkout_source_for_create(
    state: &AppState,
    reference: &str,
    requested: Option<&str>,
) -> Result<hotsheet_ticketing::checkouts::TicketSource, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let requested = requested.or(checkout.default_source.as_deref());
    if let Some(id) = requested {
        return checkout
            .sources
            .into_iter()
            .find(|source| source.connection_id == id || source.locator == id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "requested source is not linked to checkout",
                )
            });
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "checkout has no default ticket source; specify ?source=<connection-id>",
    ))
}

fn checkout_ticket_owner(
    state: &AppState,
    reference: &str,
    id: &str,
) -> Result<(hotsheet_ticketing::checkouts::TicketSource, String), ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    if let Some((connection_id, native_id)) = id.split_once(':') {
        match state
            .checkout_registry
            .resolve_source(reference, connection_id)
        {
            Ok((_, source)) => return Ok((source, native_id.to_string())),
            Err(hotsheet_ticketing::checkouts::CheckoutError::NotFound(_)) => {}
            Err(error) => {
                return Err(ApiError::new(StatusCode::CONFLICT, error.to_string()));
            }
        }
    }
    let mut found = Vec::new();
    let mut unavailable = None;
    for source in checkout.sources {
        let exists = if source.provider == "git" {
            state
                .host
                .get(&source.connection_id)
                .map(|entry| ops::resolve(&entry.store, id))
                .transpose()?
                .flatten()
                .is_some()
        } else {
            match probe_provider_source(state, &source.connection_id, id) {
                Ok(ticket) => ticket.is_some(),
                // A source that could not answer only matters when no other source owns the id.
                Err(error) => {
                    unavailable.get_or_insert(error);
                    false
                }
            }
        };
        if exists {
            found.push(source);
        }
    }
    match found.as_slice() {
        [source] => Ok((source.clone(), id.to_string())),
        [] => Err(unavailable.unwrap_or_else(|| ApiError::not_found(id))),
        _ => Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("ticket {id} is ambiguous across checkout sources; use its qualified id"),
        )),
    }
}

fn resolve_project_ticket_ref(
    state: &AppState,
    reference: ProjectTicketRef,
) -> Result<ProjectTicketRef, ApiError> {
    let (checkout, source) = state
        .checkout_registry
        .resolve_source(&reference.project_id, &reference.connection_id)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    let native_id = if source.provider == "git" {
        let entry = state.hosted_source(&source).ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                format!("checkout links an unhosted git source: {}", source.locator),
            )
        })?;
        ops::resolve(&entry.store, &reference.native_id)?
            .ok_or_else(|| ApiError::not_found(&reference.qualified()))?
            .id
            .to_string()
    } else {
        provider_for(state, &source.connection_id)?
            .get(&reference.native_id)
            .map_err(provider_transfer_error)?
            .native_id
    };
    Ok(ProjectTicketRef {
        project_id: checkout.id,
        connection_id: source.connection_id,
        native_id,
    })
}

async fn create_checkout_ticket(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(q): Query<CheckoutStoreQuery>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let source = checkout_source_for_create(
        &state,
        &reference,
        q.source.as_deref().or(q.store.as_deref()),
    )?;
    let ticket = if source.provider == "git" {
        let entry = state.hosted_source(&source).ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                format!("checkout links an unhosted git source: {}", source.locator),
            )
        })?;
        do_create(&state, &entry, req)?
    } else {
        do_provider_create(&state, &source.connection_id, req)?
    };
    Ok((
        StatusCode::CREATED,
        Json(contextualize_api_ticket(ticket, &settings)?),
    ))
}
/// Resolve ownership and retain the fetched payload. A successful qualified reference
/// reads its checkout/source once; an unqualified reference reads each candidate once
/// while preserving ambiguity detection (including remote providers).
fn resolve_checkout_ticket(
    state: &AppState,
    reference: &str,
    id: &str,
) -> Result<
    (
        hotsheet_ticketing::checkouts::Checkout,
        hotsheet_ticketing::checkouts::TicketSource,
        ResolvedTicket,
    ),
    ApiError,
> {
    if let Some((connection_id, native_id)) = id.split_once(':') {
        match state
            .checkout_registry
            .resolve_source(reference, connection_id)
        {
            Ok((checkout, source)) => {
                let ticket = read_checkout_ticket_source(state, &source, native_id, true)?
                    .ok_or_else(|| ApiError::not_found(id))?;
                return Ok((checkout, source, ticket));
            }
            Err(hotsheet_ticketing::checkouts::CheckoutError::NotFound(_)) => {}
            Err(error) => return Err(ApiError::new(StatusCode::CONFLICT, error.to_string())),
        }
    }
    let (checkout, _) = checkout_settings(state, reference)?;
    let mut found = None;
    let mut unavailable = None;
    for source in &checkout.sources {
        // A source that could not answer only matters when no other source owns the id.
        let ticket = match read_checkout_ticket_source(state, source, id, false) {
            Ok(ticket) => ticket,
            Err(error) => {
                unavailable.get_or_insert(error);
                None
            }
        };
        if let Some(ticket) = ticket {
            if found.is_some() {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    format!(
                        "ticket {id} is ambiguous across checkout sources; use its qualified id"
                    ),
                ));
            }
            found = Some((source.clone(), ticket));
        }
    }
    let (source, ticket) =
        found.ok_or_else(|| unavailable.unwrap_or_else(|| ApiError::not_found(id)))?;
    Ok((checkout, source, ticket))
}

fn read_checkout_ticket_source(
    state: &AppState,
    source: &hotsheet_ticketing::checkouts::TicketSource,
    id: &str,
    required: bool,
) -> Result<Option<ResolvedTicket>, ApiError> {
    if source.provider == "git" {
        let Some(entry) = state.hosted_source(source) else {
            return if required {
                Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "checkout links an unhosted git source",
                ))
            } else {
                Ok(None)
            };
        };
        let Some(ticket) = ops::resolve(&entry.store, id)? else {
            return Ok(None);
        };
        let store = multistore::store_url_id(&entry.store);
        return Ok(Some(ResolvedTicket {
            ticket: ApiTicket::from_provider(&ticket, &store, None),
            store,
        }));
    }
    // A qualified reference names this source explicitly, so say why it cannot be read.
    if required && connection_disabled(state, &source.connection_id)? {
        return Err(provider_transfer_error(
            hotsheet_ticketing::ProviderError::Disabled {
                connection_id: source.connection_id.clone(),
            },
        ));
    }
    Ok(
        probe_provider_source(state, &source.connection_id, id)?.map(|ticket| ResolvedTicket {
            store: source.connection_id.clone(),
            ticket,
        }),
    )
}

/// Read `id` from an external provider source. `None` when the source does not hold it,
/// including an id the provider cannot represent: a git ULID is never a GitHub issue number,
/// so probing every linked source for an unqualified id must not fail on that (HS2-GKERTK).
fn probe_provider_source(
    state: &AppState,
    connection_id: &str,
    id: &str,
) -> Result<Option<ApiTicket>, ApiError> {
    // A disabled source is not read, so for an unqualified probe it holds nothing (HS2-SF6W34).
    if connection_disabled(state, connection_id)? {
        return Ok(None);
    }
    match provider_for(state, connection_id)?.get(id) {
        Ok(ticket) => Ok(Some(ticket)),
        Err(
            hotsheet_ticketing::ProviderError::NotFound { .. }
            | hotsheet_ticketing::ProviderError::InvalidNativeId { .. },
        ) => Ok(None),
        Err(error) => Err(provider_transfer_error(error)),
    }
}

async fn get_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let (checkout, _, mut resolved) = resolve_checkout_ticket(&state, &reference, &id)?;
        resolved.ticket = contextualize_api_ticket(resolved.ticket, &checkout.settings())?;
        Ok(Json(resolved))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

#[derive(Debug, Serialize)]
struct DuplicateBacklink {
    reference: String,
    project_id: String,
    project_name: String,
    connection_id: String,
    native_id: String,
    qualified_id: String,
    slug: String,
    title: String,
}

#[derive(Debug, Serialize)]
struct DuplicateBacklinkProject {
    project_id: String,
    project_name: String,
}

#[derive(Debug, Serialize)]
struct DuplicateBacklinkResponse {
    backlinks: Vec<DuplicateBacklink>,
    inaccessible_projects: Vec<DuplicateBacklinkProject>,
}

fn duplicate_reference_matches(reference: &str, target: &ProjectTicketRef) -> bool {
    if let Some(exact) = ProjectTicketRef::from_qualified(reference) {
        return exact == *target;
    }
    reference == target.native_id
}

async fn get_checkout_ticket_duplicate_backlinks(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<DuplicateBacklinkResponse>, ApiError> {
    let (backlinks, inaccessible_projects) =
        tokio::task::spawn_blocking(move || {
            let (checkout, source, resolved) = resolve_checkout_ticket(&state, &reference, &id)?;
            let target = ProjectTicketRef {
                project_id: checkout.id,
                connection_id: source.connection_id,
                native_id: resolved.ticket.native_id,
            };
            let checkouts = state.checkout_registry.list().map_err(|error| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            })?;
            // Cold sources build an index once on this blocking worker; warm sources query
            // maintained reverse indexes. No host-registry lock spans indexing or file I/O.
            let mut backlinks = Vec::new();
            let mut inaccessible_projects = Vec::new();
            for checkout in checkouts {
                // A remembered checkout may outlive a temporary or deleted working directory.
                // It cannot contain a usable backlink while absent, and presenting it as a
                // transient source failure makes every ticket show a permanent warning.
                if !FsPath::new(&checkout.root).is_dir() {
                    continue;
                }
                let mut inaccessible = false;
                for source in &checkout.sources {
                    let tickets =
                        if source.provider == "git" {
                            // A directory can be recreated after a remembered temporary checkout is
                            // deleted (for example by an old setup tool) without recreating its HS2
                            // ticket store. A locator without HS2 metadata is no longer a searchable
                            // source, not a transient lookup failure that should warn on every ticket.
                            if !FsPath::new(&source.locator)
                                .join(STORE_METADATA_FILE)
                                .is_file()
                            {
                                continue;
                            }
                            let indexed =
                        (|| -> Result<Vec<hotsheet_index::DuplicateBacklinkRow>, ApiError> {
                            let store = FsStore::open(&source.locator)?;
                            state.host_store(store.clone())?;
                            let entry = state
                                .host
                                .get(&multistore::store_url_id(&store))
                                .ok_or_else(|| ApiError::not_found(&source.connection_id))?;
                            let rows = entry.index.lock().map_err(|error| {
                                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                            })?.duplicate_backlinks(&target.qualified(), &target.native_id)?;
                            // A watcher retains the last healthy index row for corrupt files.
                            // Validate only indexed matches, outside the index lock, to retain
                            // the old resilient scan's omission of corrupt or removed sources.
                            Ok(rows.into_iter().filter_map(|mut row| {
                                let id = Ulid::from_string(&row.id).ok()?;
                                let ticket = entry.store.read_ticket(&id).ok()?;
                                if ticket.close_reason != Some(CloseReason::Duplicate)
                                    || !ticket.duplicate_of.as_deref().is_some_and(|reference|
                                        duplicate_reference_matches(reference, &target)) {
                                    return None;
                                }
                                row.slug = ticket.slug;
                                row.title = ticket.title;
                                Some(row)
                            }).collect())
                        })();
                            match indexed {
                                Ok(rows) => {
                                    for row in rows {
                                        let source_reference = ProjectTicketRef {
                                            project_id: checkout.id.clone(),
                                            connection_id: source.connection_id.clone(),
                                            native_id: row.id.clone(),
                                        };
                                        backlinks.push(DuplicateBacklink {
                                            reference: source_reference.qualified(),
                                            project_id: checkout.id.clone(),
                                            project_name: checkout.alias.clone(),
                                            connection_id: source.connection_id.clone(),
                                            qualified_id: format!(
                                                "{}:{}",
                                                source.connection_id, row.id
                                            ),
                                            native_id: row.id,
                                            slug: row.slug,
                                            title: row.title,
                                        });
                                    }
                                    continue;
                                }
                                Err(_) => {
                                    inaccessible = true;
                                    continue;
                                }
                            }
                        } else if connection_disabled(&state, &source.connection_id)
                            .unwrap_or(false)
                        {
                            // Disabled sources are not read; they are not "inaccessible" (HS2-SF6W34).
                            continue;
                        } else {
                            match provider_for(&state, &source.connection_id).and_then(|provider| {
                                provider
                                    .query(&TicketQuery::default())
                                    .map_err(provider_transfer_error)
                            }) {
                                Ok(tickets) => tickets,
                                Err(_) => {
                                    inaccessible = true;
                                    continue;
                                }
                            }
                        };
                    for ticket in tickets {
                        let Some(duplicate_of) = ticket.duplicate_of.as_deref() else {
                            continue;
                        };
                        if ticket.close_reason != Some(CloseReason::Duplicate)
                            || !duplicate_reference_matches(duplicate_of, &target)
                        {
                            continue;
                        }
                        let source_reference = ProjectTicketRef {
                            project_id: checkout.id.clone(),
                            connection_id: ticket.connection_id.clone(),
                            native_id: ticket.native_id.clone(),
                        };
                        backlinks.push(DuplicateBacklink {
                            reference: source_reference.qualified(),
                            project_id: checkout.id.clone(),
                            project_name: checkout.alias.clone(),
                            connection_id: ticket.connection_id,
                            native_id: ticket.native_id,
                            qualified_id: ticket.qualified_id,
                            slug: ticket.slug,
                            title: ticket.title,
                        });
                    }
                }
                if inaccessible {
                    inaccessible_projects.push(DuplicateBacklinkProject {
                        project_id: checkout.id,
                        project_name: checkout.alias,
                    });
                }
            }
            backlinks.sort_by(|left, right| {
                left.project_name
                    .cmp(&right.project_name)
                    .then(left.slug.cmp(&right.slug))
                    .then(left.reference.cmp(&right.reference))
            });
            backlinks.dedup_by(|left, right| left.reference == right.reference);
            inaccessible_projects.sort_by(|left, right| {
                left.project_name
                    .cmp(&right.project_name)
                    .then(left.project_id.cmp(&right.project_id))
            });
            Ok::<_, ApiError>((backlinks, inaccessible_projects))
        })
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    Ok(Json(DuplicateBacklinkResponse {
        backlinks,
        inaccessible_projects,
    }))
}
async fn update_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        return Ok(Json(ResolvedTicket {
            store: source.connection_id.clone(),
            ticket: contextualize_api_ticket(
                do_provider_update(&state, &source.connection_id, &native_id, req)?,
                &settings,
            )?,
        }));
    }
    let entry = state.hosted_source(&source).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "checkout links an unhosted git source",
        )
    })?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: contextualize_api_ticket(do_update(&state, &entry, &native_id, req)?, &settings)?,
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
    /// One actor for the whole bulk operation (HS2-XF81CJ); an update naming its own wins.
    #[serde(default)]
    actor: Option<ActorReq>,
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
    let (_, settings) = checkout_settings(&state, &reference)?;
    let mut resolved = Vec::with_capacity(req.updates.len());
    for mut item in req.updates {
        if item.update.actor.is_none() {
            item.update.actor.clone_from(&req.actor);
        }
        let (entry, ticket) = checkout_git_ticket(&state, &reference, &item.id)?;
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
        resolved.push((entry, ticket.id.to_string(), item));
    }
    let mut updated = Vec::with_capacity(resolved.len());
    for (entry, native_id, item) in resolved {
        updated.push(ResolvedTicket {
            store: multistore::store_url_id(&entry.store),
            ticket: contextualize_api_ticket(
                do_update(&state, &entry, &native_id, item.update)?,
                &settings,
            )?,
        });
    }
    Ok(Json(updated))
}
async fn delete_checkout_ticket_note(
    State(state): State<AppState>,
    Path((reference, id, note_id)): Path<(String, String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let note_id = Ulid::from_string(&note_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid note ULID"))?;
    let updated = ops::delete_note(&entry.store, &ticket.id, &note_id, now())?;
    state.changed_in(&entry, "updated", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}
async fn close_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    let source_project = checkout.id.clone();
    let settings = checkout.settings();
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        let duplicate_of = match req.duplicate_of {
            Some(DuplicateOfReq::Legacy(reference)) => Some(reference),
            Some(DuplicateOfReq::Qualified(reference)) => {
                let target = resolve_project_ticket_ref(&state, reference)?;
                if target.project_id == source_project
                    && target.connection_id == source.connection_id
                    && target.native_id == native_id
                {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "a ticket cannot be a duplicate of itself",
                    ));
                }
                Some(target.qualified())
            }
            None => None,
        };
        let ticket = provider_for(&state, &source.connection_id)?
            .close(
                &native_id,
                now(),
                opt_parse(Some(&req.reason))?.expect("required close reason"),
                duplicate_of,
            )
            .map_err(provider_transfer_error)?;
        return Ok(Json(ResolvedTicket {
            store: source.connection_id,
            ticket: contextualize_api_ticket(ticket, &settings)?,
        }));
    }
    let entry = state.hosted_source(&source).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "checkout links an unhosted git source",
        )
    })?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: contextualize_api_ticket(
            do_close(&state, &entry, &native_id, Some(&source_project), req)?,
            &settings,
        )?,
    }))
}
/// Restore a Trash ticket to its pre-deletion status (HS2-MWDR19). Trash is the git
/// provider's soft-delete lifecycle; other providers own deletion natively.
async fn restore_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (source, _) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "the {} provider has no Hot Sheet Trash to restore from",
                source.provider
            ),
        ));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: contextualize_api_ticket(
            do_restore(&state, &entry, &ticket.id.to_string())?,
            &settings,
        )?,
    }))
}

fn do_restore(state: &AppState, entry: &StoreEntry, id: &str) -> Result<ApiTicket, ApiError> {
    let ticket = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    let restored = ops::restore(&entry.store, &ticket.id, now())?;
    state.changed_in(entry, "updated", &restored);
    api_ticket(entry, &restored)
}

async fn empty_checkout_trash(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let entries = checkout_entries(&state, &reference)?;
    if entries.is_empty() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "this checkout has no git-backed Hot Sheet Trash capability",
        ));
    }
    let mut purged = Vec::new();
    for (_, entry) in entries {
        for ticket in ops::purge_trash(&entry.store, &now(), 0)? {
            state.removed_in(&entry, &ticket);
            purged.push(ticket.slug);
        }
    }
    Ok(Json(serde_json::json!({
        "purged": purged.len(),
        "tickets": purged,
    })))
}

async fn assign_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<AssignReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: contextualize_api_ticket(
            do_assign(&state, &entry, &ticket.id.to_string(), req)?,
            &settings,
        )?,
    }))
}

async fn add_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<ResolvedTicket>), ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        // An external provider stores the file natively when its capability allows it
        // (GitHub through its assets repository, HS2-HSA64D); otherwise it refuses explicitly.
        let filename = attachment_filename(&headers)?;
        let metadata = attachment_metadata(&headers)?;
        let ticket = provider_for(&state, &source.connection_id)?
            .add_attachment(
                &native_id,
                ApiAttachment {
                    id: Ulid::new().to_string(),
                    filename,
                    created_at: now().as_str().to_string(),
                    batch_id: metadata.batch_id,
                    batch_label: metadata.batch_label,
                    actor: metadata.actor,
                    purpose: metadata.purpose,
                    annotations: vec![],
                },
                body.to_vec(),
            )
            .map_err(provider_transfer_error)?;
        return Ok((
            StatusCode::CREATED,
            Json(ResolvedTicket {
                store: source.connection_id,
                ticket: contextualize_api_ticket(ticket, &settings)?,
            }),
        ));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let filename = attachment_filename(&headers)?;
    let metadata = attachment_metadata(&headers)?;
    let (updated, _) = entry.store.write_attachment_with_metadata(
        &ticket.id,
        Ulid::new(),
        now(),
        &filename,
        &body,
        metadata,
    )?;
    state.changed_in(&entry, "attachment_added", &updated);
    Ok((
        StatusCode::CREATED,
        Json(ResolvedTicket {
            store: multistore::store_url_id(&entry.store),
            ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
        }),
    ))
}

/// Resolve a ticket reference — a qualified `{connection}:{native}` id, a bare ULID, or a slug —
/// to the hosted git store entry that owns it plus the ticket itself. Every checkout-scoped
/// handler that needs the store goes through here, so a qualified id the client routes by
/// (HS2-HX0VM9) resolves exactly like a bare one (HS2-QS9EQD).
fn checkout_git_ticket(
    state: &AppState,
    reference: &str,
    id: &str,
) -> Result<(StoreEntry, Ticket), ApiError> {
    let (source, native_id) = checkout_ticket_owner(state, reference, id)?;
    if source.provider != "git" {
        // Store-only operations (attachment edits, thumbnails, local file actions, note
        // deletion) name the provider instead of reporting a missing git store (HS2-HSA64D).
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "provider connection '{}' ({}) does not support this operation",
                source.connection_id, source.provider
            ),
        ));
    }
    let entry = state.hosted_source(&source).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "checkout links an unhosted git source",
        )
    })?;
    let ticket = ops::resolve(&entry.store, &native_id)?.ok_or_else(|| ApiError::not_found(id))?;
    Ok((entry, ticket))
}

/// Serve an external provider's attachment through the provider (HS2-HSA64D), so the
/// browser never needs the provider's credentials. Returns `None` for a git-owned ticket.
fn provider_attachment_response(
    state: &AppState,
    reference: &str,
    id: &str,
    matches: impl Fn(&ApiAttachment) -> bool,
    range: Option<&str>,
) -> Result<Option<Response>, ApiError> {
    let (source, native_id) = checkout_ticket_owner(state, reference, id)?;
    if source.provider == "git" {
        return Ok(None);
    }
    let provider = provider_for(state, &source.connection_id)?;
    let ticket = provider.get(&native_id).map_err(provider_transfer_error)?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| matches(attachment))
        .ok_or_else(|| ApiError::not_found(id))?;
    let bytes = provider
        .attachment_bytes(&native_id, &attachment.id)
        .map_err(provider_transfer_error)?;
    Ok(Some(media::attachment_response(
        &attachment.filename,
        bytes,
        range,
    )))
}

async fn get_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(response) = provider_attachment_response(
        &state,
        &reference,
        &id,
        |attachment| attachment.id == attachment_id,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )? {
        return Ok(response);
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| ApiError::not_found(&attachment_id.to_string()))?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    media::attachment_file_response(
        &attachment.filename,
        &path,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )
    .await
    .map_err(|error| {
        if matches!(&error, media::MediaError::Io(error) if error.kind() == std::io::ErrorKind::NotFound) {
            ApiError::not_found(&attachment_id.to_string())
        } else {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    })
}

async fn get_checkout_ticket_attachment_by_name(
    State(state): State<AppState>,
    Path((reference, id, filename)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(response) = provider_attachment_response(
        &state,
        &reference,
        &id,
        |attachment| attachment.filename == filename,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )? {
        return Ok(response);
    }
    let (entry, ticket, attachment_id) =
        checkout_attachment_by_name(&state, &reference, &id, &filename)?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| ApiError::not_found(&attachment_id.to_string()))?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    media::attachment_file_response(
        &attachment.filename,
        &path,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )
    .await
    .map_err(|error| {
        if matches!(&error, media::MediaError::Io(error) if error.kind() == std::io::ErrorKind::NotFound) {
            ApiError::not_found(&attachment_id.to_string())
        } else {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    })
}

async fn get_checkout_ticket_attachment_thumbnail(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, bytes) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    let filename = attachment.filename;
    let thumbnail = {
        let cache_dir = Arc::clone(&state.cache_dir);
        tokio::task::spawn_blocking(move || {
            media::optional_video_poster(&cache_dir, &filename, &bytes)
        })
    }
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("video thumbnail task failed: {error}"),
        )
    })?
    .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "video poster has not been generated"))?;
    let mut response = media::attachment_response("thumbnail.jpg", thumbnail, None);
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(response)
}

async fn put_checkout_ticket_attachment_thumbnail(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    if headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or_default().trim())
        != Some("image/jpeg")
    {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "video posters must be JPEG images",
        ));
    }
    if body.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "video poster cannot be empty",
        ));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, bytes) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    if !media::is_video(&attachment.filename) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "attachment is not a supported video",
        ));
    }
    let cache_dir = Arc::clone(&state.cache_dir);
    tokio::task::spawn_blocking(move || media::cache_video_poster(&cache_dir, &bytes, &body))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("video poster task failed: {error}"),
            )
        })?
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct AttachmentHostActionRequest {
    action: AttachmentHostAction,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AttachmentHostAction {
    Open,
    Reveal,
    Path,
}

#[derive(Debug, Serialize)]
struct AttachmentHostActionResponse {
    path: String,
}

fn checkout_attachment_by_name(
    state: &AppState,
    reference: &str,
    id: &str,
    filename: &str,
) -> Result<(StoreEntry, Ticket, Ulid), ApiError> {
    let (entry, ticket) = checkout_git_ticket(state, reference, id)?;
    let attachment_id = ticket
        .attachments
        .iter()
        .rev()
        .filter(|attachment| filename.starts_with(&attachment.filename))
        .fold(
            None::<&hotsheet_model::Attachment>,
            |best, attachment| match best {
                Some(current) if current.filename.len() >= attachment.filename.len() => {
                    Some(current)
                }
                _ => Some(attachment),
            },
        )
        .map(|attachment| attachment.id)
        .ok_or_else(|| ApiError::not_found(filename))?;
    Ok((entry, ticket, attachment_id))
}

fn attachment_disk_path(
    entry: &StoreEntry,
    ticket_id: &Ulid,
    attachment_id: &Ulid,
    filename: &str,
) -> std::path::PathBuf {
    let nested = entry
        .store
        .attachment_dir(ticket_id)
        .join(attachment_id.to_string())
        .join(filename);
    if nested.is_file() {
        nested
    } else {
        entry.store.attachment_dir(ticket_id).join(filename)
    }
}

async fn act_on_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(request): Json<AttachmentHostActionRequest>,
) -> Result<Json<AttachmentHostActionResponse>, ApiError> {
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, _) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    if request.action != AttachmentHostAction::Path {
        repository_browser::act_on_host_path(
            &path,
            match request.action {
                AttachmentHostAction::Open => repository_browser::RepositoryFileAction::Open,
                AttachmentHostAction::Reveal => repository_browser::RepositoryFileAction::Reveal,
                AttachmentHostAction::Path => unreachable!(),
            },
        )
        .map_err(repository_browser_api_error)?;
    }
    Ok(Json(AttachmentHostActionResponse {
        path: path.display().to_string(),
    }))
}

async fn act_on_checkout_ticket_attachment_by_name(
    State(state): State<AppState>,
    Path((reference, id, filename)): Path<(String, String, String)>,
    Json(request): Json<AttachmentHostActionRequest>,
) -> Result<Json<AttachmentHostActionResponse>, ApiError> {
    let (entry, ticket, attachment_id) =
        checkout_attachment_by_name(&state, &reference, &id, &filename)?;
    let (attachment, _) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    if request.action != AttachmentHostAction::Path {
        repository_browser::act_on_host_path(
            &path,
            match request.action {
                AttachmentHostAction::Open => repository_browser::RepositoryFileAction::Open,
                AttachmentHostAction::Reveal => repository_browser::RepositoryFileAction::Reveal,
                AttachmentHostAction::Path => unreachable!(),
            },
        )
        .map_err(repository_browser_api_error)?;
    }
    Ok(Json(AttachmentHostActionResponse {
        path: path.display().to_string(),
    }))
}

async fn delete_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let updated = entry
        .store
        .remove_attachment(&ticket.id, &attachment_id, now())
        .map_err(|error| {
            if error.is_io_kind(std::io::ErrorKind::NotFound) {
                ApiError::not_found(&attachment_id.to_string())
            } else {
                error.into()
            }
        })?;
    state.changed_in(&entry, "attachment_removed", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

#[derive(Debug, Deserialize)]
struct UpdateAttachmentMetadataBody {
    attachment_ids: Vec<String>,
    #[serde(flatten)]
    metadata: hotsheet_model::AttachmentMetadata,
}

async fn update_checkout_ticket_attachment_metadata(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(body): Json<UpdateAttachmentMetadataBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    if body.attachment_ids.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "attachment_ids must not be empty",
        ));
    }
    validate_attachment_metadata(&body.metadata)?;
    let mut seen = std::collections::HashSet::new();
    let attachment_ids = body
        .attachment_ids
        .iter()
        .map(|id| {
            let parsed = Ulid::from_string(id)
                .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid attachment ULID"))?;
            if !seen.insert(parsed) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "attachment_ids must be unique",
                ));
            }
            Ok(parsed)
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let updated =
        entry
            .store
            .set_attachment_metadata(&ticket.id, &attachment_ids, body.metadata, now())?;
    state.changed_in(&entry, "attachment_metadata_updated", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

#[derive(Debug, Deserialize)]
struct RenameAttachmentBody {
    filename: String,
}

async fn rename_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<RenameAttachmentBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    if body.filename.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "filename is required",
        ));
    }
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id = Ulid::from_string(&attachment_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid attachment ULID"))?;
    let updated =
        entry
            .store
            .rename_attachment(&ticket.id, &attachment_id, now(), &body.filename)?;
    state.changed_in(&entry, "attachment_renamed", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

#[derive(Debug, Deserialize)]
struct UpdateAttachmentAnnotationsBody {
    annotations: Vec<hotsheet_model::MediaAnnotation>,
}

async fn update_checkout_ticket_attachment_annotations(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<UpdateAttachmentAnnotationsBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let mut seen = std::collections::HashSet::new();
    for annotation in &body.annotations {
        let valid_rectangle = annotation.width > 0
            && annotation.height > 0
            && annotation.x.saturating_add(annotation.width) <= 10_000
            && annotation.y.saturating_add(annotation.height) <= 10_000;
        let valid_time = match (annotation.start_ms, annotation.end_ms) {
            (Some(start), Some(end)) => start <= end,
            (None, None) => true,
            _ => false,
        };
        if !seen.insert(annotation.id.clone()) || !valid_rectangle || !valid_time {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "annotations require unique ids, bounded non-empty rectangles, and complete ordered time ranges",
            ));
        }
    }
    let updated = entry
        .store
        .set_attachment_annotations_with_activity(
            &ticket.id,
            &attachment_id,
            body.annotations,
            Ulid::new(),
            now(),
        )
        .map_err(ApiError::from)?;
    state.changed_in(&entry, "attachment_annotations_updated", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

async fn ticket_flow_summary(
    State(state): State<AppState>,
) -> Result<Json<hotsheet_ticketing::analytics::TicketFlowSummary>, ApiError> {
    let tickets = ops::query(&state.store, &TicketQuery::default())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(hotsheet_ticketing::analytics::ticket_flow(&tickets)))
}

/// `GET /confidence-report`: completion-confidence calibration for the default store
/// (HS2-Q1WCCY), the same report as `hotsheet-cli confidence-report --json`.
async fn confidence_report(
    State(state): State<AppState>,
) -> Result<Json<hotsheet_ticketing::calibration::CalibrationReport>, ApiError> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || {
        let tickets = store.list_tickets_resilient()?.tickets;
        Ok(Json(hotsheet_ticketing::calibration::calibration(&tickets)))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

/// `GET /checkouts/{reference}/confidence-report`: calibration across every git store the
/// checkout links (HS2-Q1WCCY). External trackers are not included; their reopen history
/// lives in native events.
async fn checkout_confidence_report(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<hotsheet_ticketing::calibration::CalibrationReport>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let mut tickets = Vec::new();
        for (_, entry) in checkout_entries(&state, &reference)? {
            tickets.extend(entry.store.list_tickets_resilient()?.tickets);
        }
        Ok(Json(hotsheet_ticketing::calibration::calibration(&tickets)))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
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

fn validate_command_definitions(
    definitions: &[hotsheet_ticketing::commands::CommandDefinition],
) -> Result<(), ApiError> {
    use hotsheet_ticketing::commands::CommandKind;
    use std::collections::HashSet;
    let mut ids = HashSet::new();
    if definitions.iter().any(|definition| {
        definition.id.trim().is_empty()
            || definition.title.trim().is_empty()
            || match definition.kind {
                CommandKind::Program => definition.program.trim().is_empty(),
                CommandKind::Shell => definition
                    .command
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty()),
                CommandKind::Ai => definition
                    .prompt
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty()),
            }
            || !ids.insert(definition.id.as_str())
    }) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "commands require unique non-empty ids and titles plus their kind-specific executable content",
        ));
    }
    Ok(())
}

async fn save_commands(
    State(state): State<AppState>,
    Json(definitions): Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>,
) -> Result<Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>, ApiError> {
    validate_command_definitions(&definitions)?;
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

fn checkout_settings(
    state: &AppState,
    reference: &str,
) -> Result<(hotsheet_ticketing::checkouts::Checkout, Settings), ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(|error| {
            let status = match error {
                hotsheet_ticketing::checkouts::CheckoutError::NotFound(_) => StatusCode::NOT_FOUND,
                hotsheet_ticketing::checkouts::CheckoutError::Ambiguous(_) => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            ApiError::new(status, error.to_string())
        })?;
    let settings = checkout.settings();
    Ok((checkout, settings))
}

async fn list_checkout_commands(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>, ApiError> {
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    let definitions = hotsheet_ticketing::commands::from_settings(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state
        .commands
        .replace_project(checkout.id, checkout.root.into(), definitions.clone());
    Ok(Json(definitions))
}

fn read_command_groups(settings: &Settings) -> Result<Json<Vec<String>>, ApiError> {
    hotsheet_ticketing::commands::groups_from_settings(settings)
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

/// Persist the kept (possibly empty) command groups beside `commands`, in the same
/// machine-local scope, so an "Add group" survives a reload (HS2-EZ5KMC).
fn write_command_groups(
    settings: &Settings,
    groups: Vec<String>,
) -> Result<Json<Vec<String>>, ApiError> {
    let groups = hotsheet_ticketing::commands::normalize_groups(groups);
    settings
        .set(
            hotsheet_ticketing::commands::COMMAND_GROUPS_KEY,
            serde_json::to_value(&groups)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?,
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(groups))
}

async fn list_command_groups(State(state): State<AppState>) -> Result<Json<Vec<String>>, ApiError> {
    read_command_groups(&Settings::new(state.store.root()))
}

async fn save_command_groups(
    State(state): State<AppState>,
    Json(groups): Json<Vec<String>>,
) -> Result<Json<Vec<String>>, ApiError> {
    write_command_groups(&Settings::new(state.store.root()), groups)
}

async fn list_checkout_command_groups(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<String>>, ApiError> {
    let (_checkout, settings) = checkout_settings(&state, &reference)?;
    read_command_groups(&settings)
}

async fn save_checkout_command_groups(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(groups): Json<Vec<String>>,
) -> Result<Json<Vec<String>>, ApiError> {
    let (_checkout, settings) = checkout_settings(&state, &reference)?;
    write_command_groups(&settings, groups)
}

async fn save_checkout_commands(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(definitions): Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>,
) -> Result<Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>, ApiError> {
    validate_command_definitions(&definitions)?;
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            "commands",
            serde_json::to_value(&definitions)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?,
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state
        .commands
        .replace_project(checkout.id, checkout.root.into(), definitions.clone());
    Ok(Json(definitions))
}

async fn list_custom_views(
    State(state): State<AppState>,
) -> Result<Json<Vec<custom_views::CustomView>>, ApiError> {
    custom_views::from_settings(&Settings::new(state.store.root()))
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

async fn save_custom_views(
    State(state): State<AppState>,
    Json(views): Json<Vec<custom_views::CustomView>>,
) -> Result<Json<Vec<custom_views::CustomView>>, ApiError> {
    custom_views::validate(&views)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    custom_views::replace(&Settings::new(state.store.root()), &views)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "views_updated".into(),
        id: String::new(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
    Ok(Json(views))
}

async fn list_checkout_custom_views(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<custom_views::CustomView>>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    custom_views::from_settings(&settings)
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

async fn save_checkout_custom_views(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(views): Json<Vec<custom_views::CustomView>>,
) -> Result<Json<Vec<custom_views::CustomView>>, ApiError> {
    custom_views::validate(&views)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    custom_views::replace(&settings, &views)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state.emit(ChangeEvent {
        cursor: None,
        store: checkout.id,
        kind: "views_updated".into(),
        id: String::new(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
    Ok(Json(views))
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

async fn run_checkout_command(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<commands::CommandRun>), ApiError> {
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    let definitions = hotsheet_ticketing::commands::from_settings(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state
        .commands
        .replace_project(checkout.id.clone(), checkout.root.into(), definitions);
    state
        .commands
        .start_for(&checkout.id, &id)
        .map(|run| (StatusCode::ACCEPTED, Json(run)))
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))
}

async fn list_command_runs(State(state): State<AppState>) -> Json<Vec<commands::CommandRun>> {
    Json(state.commands.list())
}

async fn list_checkout_command_runs(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<commands::CommandRun>>, ApiError> {
    let (checkout, _) = checkout_settings(&state, &reference)?;
    Ok(Json(state.commands.list_for(&checkout.id)))
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

async fn get_checkout_command_run(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Query(query): Query<RunOutputQuery>,
) -> Result<Json<commands::CommandRun>, ApiError> {
    let (checkout, _) = checkout_settings(&state, &reference)?;
    state
        .commands
        .get_for(&checkout.id, &id, query.after)
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

async fn cancel_checkout_command_run(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<commands::CommandRun>, ApiError> {
    let (checkout, _) = checkout_settings(&state, &reference)?;
    state
        .commands
        .cancel_for(&checkout.id, &id)
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error))
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
        cursor: None,
        store: n.store.clone().unwrap_or_default(),
        kind: "notification".into(),
        id: n.id.clone(),
        slug: n.ticket.clone().unwrap_or_default(),
        message: Some(n.message.clone()),
        activity: None,
        assignment: None,
        turn: None,
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
///
/// Hosting a new store opens and reconciles (or rebuilds) its index, which parses every
/// ticket file, and the response counts the added store's tickets. Both run on the blocking
/// pool, so registering a large store never occupies an async request thread (HS2-4XXRJP,
/// HS2-GM4FR2).
async fn add_store(
    State(state): State<AppState>,
    Json(body): Json<AddStoreBody>,
) -> Result<(StatusCode, Json<StoreInfo>), ApiError> {
    let (newly, info) = tokio::task::spawn_blocking(move || {
        let store = FsStore::open(&body.path)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
        let id = multistore::store_url_id(&store);
        let newly = state.host_store(store)?;
        let info = state
            .host
            .info(&id)
            .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "store vanished"))?;
        Ok::<_, ApiError>((newly, info))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    let code = if newly {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((code, Json(info)))
}

/// `GET /stores/{store_id}/tickets` — the store-scoped list, served from that store's own
/// index under the same bound as `GET /tickets`. Unknown id → 404.
async fn list_store_tickets(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Response, ApiError> {
    let entry = state
        .host
        .get(&store_id)
        .ok_or_else(|| ApiError::not_found(&store_id))?;
    list_entry_tickets(&entry, &store_id, params)
}

// The write logic is store-generic: it operates on a `StoreEntry` so the unprefixed
// (default store) routes and the `/stores/{id}/…` scoped routes share one implementation.

fn do_create(state: &AppState, entry: &StoreEntry, req: CreateReq) -> Result<ApiTicket, ApiError> {
    let prefix = entry.store.metadata()?.ticket_prefix;
    let status = initial_status(req.status.as_deref())?;
    let blocked_by =
        ops::resolve_blockers(&entry.store, None, &req.blocked_by.unwrap_or_default())?;
    let new = NewTicket {
        title: req.title,
        category: req.category.unwrap_or_else(|| "issue".to_string()),
        priority: opt_parse(req.priority.as_deref())?.unwrap_or_default(),
        status,
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
    let note_text = req.note.clone();
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
    if edit_note_id.is_some() && req.note_summary.is_some() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "note_summary is only valid when appending a note",
        ));
    }
    if req.note_summary.is_some() && req.note.as_deref().is_none_or(str::is_empty) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "note_summary requires a non-empty note",
        ));
    }
    let confidence_change = req.note_confidence_change(edit_note_id.is_some())?;
    // A present `blocked_by` (even []) replaces the set; absent leaves it unchanged.
    let blocked_by = match req.blocked_by {
        Some(needles) => Some(ops::resolve_blockers(
            &entry.store,
            Some(&ticket.id),
            &needles,
        )?),
        None => None,
    };
    let actor = parse_actor(req.actor.as_ref())?;
    let patch = TicketPatch {
        actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
        title: req.title,
        details: req.details,
        category: req.category,
        priority: opt_parse(req.priority.as_deref())?,
        status: opt_parse(req.status.as_deref())?,
        tags: req.tags,
        up_next: req.up_next,
        blocked_by,
        blocked_reason: req.blocked_reason,
    };
    // Role-specific rules run before any write (HS2-RD4M29).
    let scores_now = match edit_note_id {
        Some(_) => matches!(confidence_change, Some(Some(_))),
        None => {
            note_text.as_deref().is_some_and(|text| !text.is_empty())
                && confidence_change.flatten().is_some()
        }
    };
    hotsheet_ticketing::actor::check_completion(
        actor.as_ref(),
        &ticket.slug,
        hotsheet_ticketing::actor::completes(ticket.status, patch.status),
        scores_now || hotsheet_ticketing::actor::scored_in_current_cycle(&ticket),
    )?;
    let updated = ops::update(&entry.store, &ticket.id, now(), patch)?;
    // An optional note append/edit rides the same update call (parity with CLI + MCP).
    // An edit may change only the confidence (HS2-CY4CWC); empty text is never written.
    let note_text_edit = req.note.filter(|t| !t.is_empty());
    let latest = match edit_note_id {
        Some(note_id) => {
            let edit = ops::NoteEditInput {
                text: note_text_edit,
                confidence: confidence_change,
            };
            if edit.is_empty() {
                updated
            } else {
                ops::edit_note_with_metadata(&entry.store, &ticket.id, &note_id, now(), edit)?
            }
        }
        None => match note_text_edit {
            Some(text) => ops::add_note_with_metadata(
                &entry.store,
                &ticket.id,
                Ulid::new(),
                now(),
                req.note_kind.unwrap_or(NoteKind::Regular),
                ops::NoteMetadataInput {
                    summary: req.note_summary,
                    confidence: confidence_change.flatten(),
                    actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
                },
                text,
            )?,
            None => updated,
        },
    };
    state.changed_in(entry, "updated", &latest);
    let mut response = api_ticket(entry, &latest)?;
    if let Some(text) = note_text.filter(|text| !text.is_empty()) {
        response.warnings = ops::attachment_reference_warnings(&entry.store, &latest, &text);
    }
    Ok(response)
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
        cursor: None,
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
        turn: None,
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
    source_project_id: Option<&str>,
    req: CloseReq,
) -> Result<ApiTicket, ApiError> {
    let ticket = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    let reason: CloseReason = opt_parse(Some(req.reason.as_str()))?.expect("reason present");
    let actor = parse_actor(req.actor.as_ref())?;
    hotsheet_ticketing::actor::check_completion(
        actor.as_ref(),
        &ticket.slug,
        hotsheet_ticketing::actor::close_completes(ticket.status, reason),
        hotsheet_ticketing::actor::scored_in_current_cycle(&ticket),
    )?;
    let dup = match req.duplicate_of {
        Some(DuplicateOfReq::Legacy(reference)) => {
            let target = ops::resolve(&entry.store, &reference)?
                .ok_or_else(|| ApiError::not_found(&reference))?;
            if target.id == ticket.id {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "a ticket cannot be a duplicate of itself",
                ));
            }
            Some(target.id.to_string())
        }
        Some(DuplicateOfReq::Qualified(reference)) => {
            let target = resolve_project_ticket_ref(state, reference)?;
            // Checkout routes compare the full project/connection/native identity. The
            // legacy default/store routes have no project identity, so they intentionally
            // fall back to same-underlying-store semantics.
            if source_project_id.is_none_or(|project_id| target.project_id == project_id)
                && target.connection_id == multistore::store_url_id(&entry.store)
                && target.native_id == ticket.id.to_string()
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "a ticket cannot be a duplicate of itself",
                ));
            }
            Some(target.qualified())
        }
        None => None,
    };
    let closed = ops::close_as(
        &entry.store,
        &ticket.id,
        now(),
        reason,
        dup,
        hotsheet_ticketing::actor::note_actor(actor.as_ref()).as_ref(),
    )?;
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
    let metadata = attachment_metadata(&headers)?;
    let (updated, _) = entry.store.write_attachment_with_metadata(
        &ticket.id,
        Ulid::new(),
        now(),
        &filename,
        &body,
        metadata,
    )?;
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
    decode_attachment_header(raw, "filename")
}

fn attachment_metadata(
    headers: &HeaderMap,
) -> Result<hotsheet_model::AttachmentMetadata, ApiError> {
    use hotsheet_model::{
        AttachmentActor, AttachmentActorRole, AttachmentMetadata, AttachmentPurpose,
    };
    let encoded = headers
        .get("x-hotsheet-metadata-encoding")
        .and_then(|value| value.to_str().ok())
        == Some("percent");
    let text = |name: &'static str| -> Result<Option<String>, ApiError> {
        let Some(raw) = headers.get(name).and_then(|value| value.to_str().ok()) else {
            return Ok(None);
        };
        let value = if encoded {
            decode_attachment_header(raw, name)?
        } else {
            raw.to_owned()
        };
        Ok((!value.trim().is_empty()).then_some(value))
    };
    let role = match text("x-hotsheet-actor-role")?.as_deref() {
        None => None,
        Some("human") => Some(AttachmentActorRole::Human),
        Some("ai") => Some(AttachmentActorRole::Ai),
        Some("system") => Some(AttachmentActorRole::System),
        Some("unknown") => Some(AttachmentActorRole::Unknown),
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "attachment actor role must be human, ai, system, or unknown",
            ));
        }
    };
    let identity = text("x-hotsheet-actor-identity")?;
    let display_name = text("x-hotsheet-actor-name")?;
    if role.is_none() && (identity.is_some() || display_name.is_some()) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "attachment actor identity/name requires actor role",
        ));
    }
    let purpose = match text("x-hotsheet-attachment-purpose")?.as_deref() {
        None => None,
        Some("problem_evidence") => Some(AttachmentPurpose::ProblemEvidence),
        Some("correctness_evidence") => Some(AttachmentPurpose::CorrectnessEvidence),
        Some("reference") => Some(AttachmentPurpose::Reference),
        Some("other") => Some(AttachmentPurpose::Other),
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "attachment purpose is invalid",
            ));
        }
    };
    let metadata = AttachmentMetadata {
        batch_id: text("x-hotsheet-attachment-batch")?,
        batch_label: text("x-hotsheet-attachment-batch-label")?,
        actor: role.map(|role| AttachmentActor {
            identity,
            display_name,
            role,
        }),
        purpose,
    };
    validate_attachment_metadata(&metadata)?;
    Ok(metadata)
}

fn validate_attachment_metadata(
    metadata: &hotsheet_model::AttachmentMetadata,
) -> Result<(), ApiError> {
    if metadata.batch_label.is_some() && metadata.batch_id.is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "attachment batch_label requires batch_id",
        ));
    }
    for (label, value, limit) in [
        ("batch_id", metadata.batch_id.as_deref(), 200usize),
        ("batch_label", metadata.batch_label.as_deref(), 200usize),
        (
            "actor.identity",
            metadata
                .actor
                .as_ref()
                .and_then(|actor| actor.identity.as_deref()),
            200usize,
        ),
        (
            "actor.display_name",
            metadata
                .actor
                .as_ref()
                .and_then(|actor| actor.display_name.as_deref()),
            200usize,
        ),
    ] {
        if value.is_some_and(|value| value.trim().is_empty() || value.len() > limit) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("attachment {label} must contain 1–{limit} bytes"),
            ));
        }
    }
    Ok(())
}

fn decode_attachment_header(raw: &str, field: &str) -> Result<String, ApiError> {
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
                    format!("invalid encoded attachment {field}"),
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
            format!("invalid encoded attachment {field}"),
        )
    })
}

async fn close_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    Ok(Json(do_close(
        &state,
        &state.default_entry(),
        &id,
        None,
        req,
    )?))
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
    let eta = req
        .eta
        .as_deref()
        .map(|raw| ops::parse_claim_eta(&now, raw))
        .transpose()?;
    let claimed = ops::claim_next_with_eta(&entry.store, &now, lease, &worker, req.label, eta)?;
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

/// `POST /tickets/{id}/claim` — claim one exact open, unblocked ticket by slug or ULID.
async fn claim_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ClaimReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = state.default_entry();
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let now = now();
    let lease = now.plus_minutes(req.lease_minutes.unwrap_or(DEFAULT_LEASE_MINUTES));
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let eta = req
        .eta
        .as_deref()
        .map(|raw| ops::parse_claim_eta(&now, raw))
        .transpose()?;
    let claimed = ops::claim_with_eta(
        &entry.store,
        &ticket.id,
        &now,
        lease,
        &worker,
        req.label,
        eta,
    )?;
    state.changed_in(&entry, "claimed", &claimed);
    Ok(Json(api_ticket(&entry, &claimed)?))
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
    let eta = req
        .eta
        .as_deref()
        .map(|raw| ops::parse_claim_eta(&now, raw))
        .transpose()?;
    let renewed = ops::renew_with_eta(&entry.store, &ticket.id, now, lease, &worker, eta)?;
    state.changed_in(&entry, "renewed", &renewed);
    Ok(Json(api_ticket(&entry, &renewed)?))
}

// ---- permission round-trip (HS2-9R9YZW) ------------------------------------------

/// `GET /permissions` — the requests a driven tool is currently blocked on, for a client
/// to render + answer. Each carries the raising connection + the `(tool, action)` asked.
async fn list_permissions(State(state): State<AppState>) -> Json<Vec<PermissionInfo>> {
    let rule_projects = state.permission_rule_paths.lock().unwrap().clone();
    Json(
        state
            .permissions
            .pending()
            .into_iter()
            .map(|request| PermissionInfo {
                id: request.id,
                project: request.project.clone(),
                connection: request.connection,
                tool: request.tool,
                action: request.action,
                agent: request.agent,
                always_allow_supported: rule_projects.contains_key(&request.project),
            })
            .collect(),
    )
}

/// Client-facing pending permission request. The capability flag is explicit so a client
/// never offers a durable response when this server has no rule store configured.
#[derive(Serialize)]
struct PermissionInfo {
    id: u64,
    project: String,
    connection: String,
    tool: String,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    always_allow_supported: bool,
}

/// Body for `POST /permissions/ask`: who's asking + what.
#[derive(Deserialize)]
struct AskBody {
    #[serde(default)]
    project: String,
    /// The live connection id (so a client attributes the prompt to the right tool).
    connection: String,
    /// The tool/action asking (e.g. `"Bash"`, `"Edit"`) — the rule-match key.
    tool: String,
    action: String,
    #[serde(default)]
    agent: Option<String>,
}

/// A disconnected hook is no longer waiting for a decision. Remove its pending prompt
/// immediately instead of leaving it visible until the 24-hour safety timeout.
struct PermissionAskGuard {
    state: Arc<PermissionAskState>,
}

struct PermissionAskState {
    server: AppState,
    project: String,
    pending_id: AtomicU64,
    cancelled: AtomicBool,
}

impl PermissionAskState {
    fn mark_pending(&self, id: u64) {
        self.pending_id.store(id, Ordering::Release);
        if self.cancelled.load(Ordering::Acquire) {
            self.cancel_pending(id);
        }
    }

    fn cancel_pending(&self, id: u64) {
        if self
            .server
            .permissions
            .resolve(
                id,
                hotsheet_aitools::PermissionDecision::Deny,
                hotsheet_aitools::PermissionScope::Once,
            )
            .is_some()
        {
            self.emit_removed(id);
        }
    }

    fn emit_removed(&self, id: u64) {
        self.server.emit(ChangeEvent {
            cursor: None,
            store: self.project.clone(),
            kind: "permission_resolved".into(),
            id: id.to_string(),
            slug: String::new(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
    }
}

impl Drop for PermissionAskGuard {
    fn drop(&mut self) {
        self.state.cancelled.store(true, Ordering::Release);
        let id = self.state.pending_id.load(Ordering::Acquire);
        if id != 0 {
            self.state.cancel_pending(id);
        }
    }
}

/// `POST /permissions/ask` `{connection, tool, action}` — raise a permission request and
/// **block** until a human answers over the route-back (`POST /permissions/{id}`), up to a
/// timeout then a safe `deny`. This is the *asking* side, for an external tool transport
/// like the Claude PreToolUse hook (HS2-YMR9HE). An allow-rule answers immediately.
async fn ask_permission(State(state): State<AppState>, Json(body): Json<AskBody>) -> Response {
    if state.is_stopping() {
        return permission_ask_stopping();
    }
    let stopping = state.clone();
    let bridge = state.permissions.clone();
    let project = if body.project.is_empty() {
        state.store.root().display().to_string()
    } else {
        body.project
    };
    let cancellation = Arc::new(PermissionAskState {
        server: state,
        project: project.clone(),
        pending_id: AtomicU64::new(0),
        cancelled: AtomicBool::new(false),
    });
    let guard = PermissionAskGuard {
        state: cancellation.clone(),
    };
    // request_blocking_timeout blocks (Condvar); run it off the async runtime.
    let blocking = tokio::task::spawn_blocking(move || {
        bridge.request_blocking_timeout_with_pending(
            hotsheet_aitools::PermissionAsk {
                project,
                connection: body.connection,
                tool: body.tool,
                action: body.action,
                agent: body.agent,
            },
            hotsheet_aitools::DEFAULT_PERMISSION_TIMEOUT,
            hotsheet_aitools::PermissionDecision::Deny,
            |id| cancellation.mark_pending(id),
            |_| {},
        )
    });
    // A stopping server must not hold the hook open (HS2-W1KJR4): answer 503 so the hook
    // falls back to the tool's native prompt. Dropping the guard denies + removes the
    // pending prompt, which also wakes the parked blocking thread.
    let decision = tokio::select! {
        joined = blocking => joined.unwrap_or(hotsheet_aitools::PermissionDecision::Deny),
        () = stopping.stopping() => {
            drop(guard);
            return permission_ask_stopping();
        }
    };
    drop(guard);
    let allow = decision == hotsheet_aitools::PermissionDecision::Allow;
    Json(serde_json::json!({ "decision": if allow { "allow" } else { "deny" } })).into_response()
}

/// `503` for a permission ask on a stopping server: the hook treats it as a transport
/// failure and falls back to the tool's native prompt (HS2-W1KJR4).
fn permission_ask_stopping() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "server is shutting down" })),
    )
        .into_response()
}

/// One driven connection as reported by `GET /connections`.
#[derive(Serialize)]
struct ConnectionInfo {
    id: String,
    tool: String,
    project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    /// `main` | `worker`.
    role: String,
    /// Whether the connection is busy (a turn is actively streaming) right now.
    busy: bool,
    /// Available semantic actions. Capability is represented by presence, never an inert
    /// `can_interrupt` boolean.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
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
            source: None,
            role: format!("{:?}", c.role).to_lowercase(),
            busy: reg.is_busy(&c.id, now),
            actions: Vec::new(),
            session_id: None,
            last_error: None,
            model: None,
            effort: None,
        })
        .collect::<Vec<_>>();
    drop(reg);
    let mut infos = infos;
    for client in state.client_drives.list() {
        if infos.iter().any(|existing| existing.id == client.id) {
            continue;
        }
        infos.push(ConnectionInfo {
            id: client.id,
            tool: client.tool,
            project: client.project,
            source: Some(client.source),
            role: client.role,
            busy: client.busy,
            actions: client.actions,
            session_id: client.session_id,
            last_error: client.last_error,
            model: client.model,
            effort: client.effort,
        });
    }
    infos.sort_by(|left, right| left.id.cmp(&right.id));
    Json(infos)
}

#[derive(Deserialize)]
struct CreateDriveConnectionReq {
    tool: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    checkout: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    connection_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

fn discovered_ai_tools(state: &AppState, refresh: bool) -> Vec<hotsheet_plugins::AiToolDescriptor> {
    discover_ai_tools_memoized(
        &state.model_catalogs,
        &state.ai_tool_generation,
        &state.ai_tool_discovery,
        &state.plugin_dirs,
        state.store.root(),
        refresh,
    )
}

/// Blocking AI-tool discovery through the short-lived memo (HS2-QV8B7R). The generation is
/// read under the catalog lock, so callers that queued behind a scan reuse its result
/// unless an invalidation landed after it started.
fn discover_ai_tools_memoized(
    catalogs: &Mutex<ai_tool_discovery::AiToolDiscoveryCache>,
    generation: &AtomicU64,
    config: &ai_tool_discovery::AiToolDiscoveryConfig,
    plugin_dirs: &[std::path::PathBuf],
    root: &FsPath,
    refresh: bool,
) -> Vec<hotsheet_plugins::AiToolDescriptor> {
    let mut cache = catalogs.lock().unwrap();
    let generation = generation.load(Ordering::Acquire);
    cache.discover(
        std::time::Instant::now(),
        generation,
        config.ttl,
        refresh,
        |catalogs, refresh| (config.scanner)(plugin_dirs, root, catalogs, refresh),
    )
}

impl AppState {
    /// Drop the memoized AI-tool discovery after tool installation or plugin state changed,
    /// without waiting for a discovery in progress (HS2-QV8B7R).
    fn invalidate_ai_tool_discovery(&self) {
        self.ai_tool_generation.fetch_add(1, Ordering::AcqRel);
    }
}

/// Discover AI tools **off the async runtime**.
///
/// AI-tool discovery launches blocking `--version`/`models` subprocesses for every
/// installed drivable tool (even a warm cache re-checks each tool's runtime version),
/// and it takes the single shared `model_catalogs` mutex for the whole scan. Running it
/// inline in an async handler both starves a tokio worker for the duration and serializes
/// unrelated concurrent requests behind it — the mechanism behind a second web client
/// loading far slower than the first (HS2-10R4VV). Move the whole scan (and its lock) to
/// the blocking pool so the async workers stay free. See HS2-S66BZZ.
///
/// The result is memoized for the configured TTL (default
/// [`ai_tool_discovery::AI_TOOL_DISCOVERY_TTL`]) under the same
/// lock, so project activation's `/ai-tools` and `/ai-settings` share one scan and
/// concurrent callers coalesce onto it; `refresh` bypasses and repopulates the memo
/// (HS2-QV8B7R).
async fn discovered_ai_tools_off_runtime(
    state: &AppState,
    refresh: bool,
) -> Vec<hotsheet_plugins::AiToolDescriptor> {
    let catalogs = state.model_catalogs.clone();
    let generation = state.ai_tool_generation.clone();
    let config = state.ai_tool_discovery.clone();
    let plugin_dirs = state.plugin_dirs.clone();
    let root = state.store.root().to_path_buf();
    // A detached thread, not the blocking pool (HS2-NPBZJ9): the scan runs installed tools'
    // `--version`/catalog subprocesses with no deadline of their own, and the runtime waits
    // for every `spawn_blocking` task when it drops. A probe that hangs past a stop (or past
    // its abandoned request) must not hold the process; its result is only a cache fill.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("ai-tool-discovery".into())
        .spawn(move || {
            let _ = sender.send(discover_ai_tools_memoized(
                &catalogs,
                &generation,
                &config,
                &plugin_dirs,
                &root,
                refresh,
            ));
        });
    if spawned.is_err() {
        return Vec::new();
    }
    receiver.await.unwrap_or_default()
}

#[derive(Default, Deserialize)]
struct AiToolsQuery {
    #[serde(default)]
    refresh: bool,
}

async fn list_ai_tools(
    State(state): State<AppState>,
    Query(query): Query<AiToolsQuery>,
) -> Json<Vec<hotsheet_plugins::AiToolDescriptor>> {
    Json(discovered_ai_tools_off_runtime(&state, query.refresh).await)
}

/// The settings key holding the default Drive tool, model, and effort.
const AI_DEFAULTS_SETTING: &str = "ai.defaults";

/// A saved AI default from one settings scope, kept only while it still names an installed tool.
fn saved_ai_defaults(
    settings: &Settings,
    scope: hotsheet_ticketing::Scope,
    tools: &[hotsheet_plugins::AiToolDescriptor],
) -> Result<Option<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(stored_ai_defaults(settings, scope)?
        .filter(|defaults| hotsheet_plugins::validate_ai_defaults(tools, defaults).is_ok())
        .map(|defaults| hotsheet_plugins::sanitize_ai_defaults(tools, defaults)))
}

/// The AI defaults stored in one scope, unvalidated.
fn stored_ai_defaults(
    settings: &Settings,
    scope: hotsheet_ticketing::Scope,
) -> Result<Option<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(settings
        .get(AI_DEFAULTS_SETTING, scope)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
        .and_then(|value| serde_json::from_value(value).ok()))
}

/// The effective AI defaults: the project's own choice (machine-local project settings, HS2-SW5S13),
/// else the legacy machine-wide value, else the discovered default tool.
async fn effective_ai_settings(
    state: &AppState,
    project: Option<&Settings>,
) -> Result<hotsheet_plugins::AiToolDefaults, ApiError> {
    let tools = discovered_ai_tools_off_runtime(state, false).await;
    let project_defaults = match project {
        Some(settings) => saved_ai_defaults(settings, hotsheet_ticketing::Scope::Local, &tools)?,
        None => None,
    };
    let saved = match project_defaults {
        Some(defaults) => Some(defaults),
        None => saved_ai_defaults(
            &Settings::new(state.store.root()),
            hotsheet_ticketing::Scope::Global,
            &tools,
        )?,
    };
    saved
        .or_else(|| hotsheet_plugins::default_ai_settings(&tools))
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no drivable AI tools are installed"))
}

/// Validate and store AI defaults in one settings scope, keeping the per-provider choices for
/// tools that are not installed right now (HS2-EK24KF). Returns the value as served back.
async fn save_ai_settings(
    state: &AppState,
    settings: &Settings,
    scope: hotsheet_ticketing::Scope,
    defaults: &hotsheet_plugins::AiToolDefaults,
) -> Result<hotsheet_plugins::AiToolDefaults, ApiError> {
    let tools = discovered_ai_tools_off_runtime(state, false).await;
    let previous = stored_ai_defaults(settings, scope)?;
    let prepared =
        hotsheet_plugins::prepare_ai_defaults_for_save(&tools, defaults, previous.as_ref())
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    settings
        .set(
            AI_DEFAULTS_SETTING,
            serde_json::to_value(&prepared)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?,
            scope,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(hotsheet_plugins::sanitize_ai_defaults(&tools, prepared))
}

/// A project's AI defaults (HS2-SW5S13): stored in the checkout's machine-local settings because the
/// installed tools and models differ per machine; a project without a choice inherits the
/// machine-wide value.
async fn get_checkout_ai_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<hotsheet_plugins::AiToolDefaults>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    Ok(Json(effective_ai_settings(&state, Some(&settings)).await?))
}

async fn put_checkout_ai_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(defaults): Json<hotsheet_plugins::AiToolDefaults>,
) -> Result<Json<hotsheet_plugins::AiToolDefaults>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    Ok(Json(
        save_ai_settings(
            &state,
            &settings,
            hotsheet_ticketing::Scope::Local,
            &defaults,
        )
        .await?,
    ))
}

/// The machine-wide AI defaults: the fallback for projects without their own choice.
async fn get_ai_settings(
    State(state): State<AppState>,
) -> Result<Json<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(Json(effective_ai_settings(&state, None).await?))
}

async fn put_ai_settings(
    State(state): State<AppState>,
    Json(defaults): Json<hotsheet_plugins::AiToolDefaults>,
) -> Result<Json<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(Json(
        save_ai_settings(
            &state,
            &Settings::new(state.store.root()),
            hotsheet_ticketing::Scope::Global,
            &defaults,
        )
        .await?,
    ))
}

async fn create_drive_connection(
    State(state): State<AppState>,
    Json(request): Json<CreateDriveConnectionReq>,
) -> Result<(StatusCode, Json<client_drive::ClientConnectionInfo>), ApiError> {
    if request.tool.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "tool is required"));
    }
    if request.model.is_some() || request.effort.is_some() {
        let tools = discovered_ai_tools_off_runtime(&state, false).await;
        if tools.iter().any(|tool| tool.id == request.tool) {
            hotsheet_plugins::validate_ai_defaults(
                &tools,
                &hotsheet_plugins::AiToolDefaults {
                    tool: request.tool.clone(),
                    model: request.model.clone(),
                    effort: request.effort.clone(),
                    ..Default::default()
                },
            )
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
        }
    }
    let (project_path, source_id, drive_store) =
        if let Some(reference) = request.checkout.as_deref() {
            let checkout = state
                .checkout_registry
                .resolve(reference)
                .map_err(|error| {
                    let status = match error {
                        hotsheet_ticketing::checkouts::CheckoutError::NotFound(_) => {
                            StatusCode::NOT_FOUND
                        }
                        hotsheet_ticketing::checkouts::CheckoutError::Ambiguous(_) => {
                            StatusCode::CONFLICT
                        }
                        _ => StatusCode::BAD_REQUEST,
                    };
                    ApiError::new(status, error.to_string())
                })?;
            let project_path = std::path::PathBuf::from(&checkout.root);
            let source = checkout_source_for_create(&state, reference, request.source.as_deref())?;
            if source.provider != "git" {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format!(
                        "ticket source '{}' does not support local client-drive activity",
                        source.connection_id
                    ),
                ));
            }
            let entry = state.host.get(&source.connection_id).ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    format!(
                        "ticket source '{}' is not hosted by this server",
                        source.connection_id
                    ),
                )
            })?;
            (project_path, source.connection_id, entry.store)
        } else {
            (
                state.store.root().to_path_buf(),
                multistore::store_url_id(&state.store),
                state.store.clone(),
            )
        };
    let mut env = vec![format!("HOTSHEET_PROJECT={}", project_path.display())];
    if let Some(url) = state
        .terminal_server_url
        .lock()
        .ok()
        .and_then(|value| value.clone())
    {
        env.push(format!("HOTSHEET_SERVER={url}"));
        env.push(format!("HOTSHEET_SECRET={}", state.secret));
    }
    let info = state
        .client_drives
        .create_or_attach(
            client_drive::PrepareDrive {
                store_path: drive_store.root().to_path_buf(),
                source_id,
                project_path,
                tool: request.tool,
                env,
                permission_bridge: state.permission_bridge(),
                persistent_home: None,
                model: request.model,
                effort: request.effort,
            },
            request.connection_id,
            request.session_id,
        )
        .map_err(client_drive_api_error)?;
    state.emit_drive_updated(&info);
    Ok((StatusCode::CREATED, Json(info)))
}

async fn delete_drive_connection(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|error| {
            let status = match error {
                hotsheet_ticketing::checkouts::CheckoutError::NotFound(_) => StatusCode::NOT_FOUND,
                hotsheet_ticketing::checkouts::CheckoutError::Ambiguous(_) => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            ApiError::new(status, error.to_string())
        })?;
    let project = std::path::PathBuf::from(checkout.root)
        .display()
        .to_string();
    let closed = state
        .client_drives
        .close(&project, &id)
        .map_err(client_drive_api_error)?;
    // An idle drive's session ends now; a busy one's when its interrupted turn finishes.
    if let client_drive::DriveClosed::Ended { worker_id } = &closed {
        release_session_claims(state.host.clone(), worker_id.clone()).await;
    }
    if closed != client_drive::DriveClosed::NotFound {
        let permissions = state.permission_bridge();
        for request in permissions
            .pending()
            .into_iter()
            .filter(|request| request.project == project && request.connection == id)
        {
            permissions.resolve(
                request.id,
                hotsheet_aitools::PermissionDecision::Deny,
                hotsheet_aitools::PermissionScope::Once,
            );
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_drive_sessions(
    State(state): State<AppState>,
) -> Json<Vec<client_drive::ClientSessionInfo>> {
    Json(
        state
            .client_drives
            .sessions(&state.store.root().display().to_string()),
    )
}

#[derive(Deserialize)]
struct SendDriveTurnReq {
    content: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
}

async fn send_drive_turn(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<SendDriveTurnReq>,
) -> Result<(StatusCode, Json<client_drive::ClientConnectionInfo>), ApiError> {
    if request.content.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "content is required",
        ));
    }
    let connection = state
        .client_drives
        .get(&id)
        .map_err(client_drive_api_error)?;
    if request.model.is_some() || request.effort.is_some() {
        let tools = discovered_ai_tools_off_runtime(&state, false).await;
        if let Some(descriptor) = tools.iter().find(|tool| tool.id == connection.tool) {
            if request.model.is_some()
                && !descriptor
                    .actions
                    .iter()
                    .any(|action| action == "change_model")
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "this AI tool cannot change model within a session",
                ));
            }
            if request.effort.is_some()
                && !descriptor
                    .actions
                    .iter()
                    .any(|action| action == "change_effort")
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "this AI tool cannot change effort within a session",
                ));
            }
            hotsheet_plugins::validate_ai_defaults(
                &tools,
                &hotsheet_plugins::AiToolDefaults {
                    tool: connection.tool,
                    model: request.model.clone(),
                    effort: request.effort.clone(),
                    ..Default::default()
                },
            )
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
        }
    }
    let job = state
        .client_drives
        .begin_turn(&id, request.session_id, request.model, request.effort)
        .map_err(client_drive_api_error)?;
    let info = state
        .client_drives
        .get(&id)
        .map_err(client_drive_api_error)?;
    state.emit_drive_updated(&info);

    let manager = state.client_drives.clone();
    let thread_state = state.clone();
    let thread_store = FsStore::open(job.store_path()).map_err(ApiError::from)?;
    let tool = info.tool.clone();
    let activity_project = info.project.clone();
    let prompt = request.content;
    let thread_id = id.clone();
    std::thread::spawn(move || {
        let mut guard = turn_stream::TurnStreamGuard::default();
        let result = job.run(&prompt, &mut |event| {
            let projected = match event {
                hotsheet_aitools::TurnEvent::Usage(usage) => {
                    let priced = hotsheet_ticketing::metrics::price_event(
                        &thread_store,
                        hotsheet_ticketing::metrics::UsageEvent {
                            ts: now().as_str().to_string(),
                            tool: tool.clone(),
                            model: usage.model.clone(),
                            tokens_in: usage.tokens_in,
                            tokens_out: usage.tokens_out,
                            cost_usd: usage.cost_usd,
                            ticket: None,
                            session: Some(thread_id.clone()),
                        },
                    );
                    let _ = hotsheet_ticketing::metrics::record(&thread_store, &priced);
                    hotsheet_aitools::TurnEvent::Usage(hotsheet_aitools::Usage {
                        model: priced.model,
                        tokens_in: priced.tokens_in,
                        tokens_out: priced.tokens_out,
                        cost_usd: priced.cost_usd,
                    })
                }
                hotsheet_aitools::TurnEvent::NativeActivity { source, payload } => {
                    let id = Ulid::new().to_string();
                    let ts = now().as_str().to_string();
                    let mapped = match source.as_str() {
                        "codex-transcript" => {
                            hotsheet_ticketing::activity::codex_activity(payload, &id, &ts)
                        }
                        "claude-hooks" => {
                            hotsheet_ticketing::activity::claude_activity(payload, &id, &ts)
                        }
                        _ => None,
                    };
                    if let Some(mut activity) = mapped {
                        activity.session = Some(thread_id.clone());
                        activity.project = Some(activity_project.clone());
                        let _ = thread_state.record_activity(&thread_store, activity);
                    }
                    event.clone()
                }
                _ => event.clone(),
            };
            for event in guard.observe(&projected) {
                thread_state.emit_turn_event(&thread_store, &thread_id, None, &tool, event);
            }
        });
        if result.is_err() {
            for event in guard.transport_failed() {
                thread_state.emit_turn_event(&thread_store, &thread_id, None, &tool, event);
            }
        }
        if let Some(worker) = manager.finish_turn(&job, &result) {
            release_session_claims_blocking(&thread_state.host, &worker);
        }
        if let Ok(info) = manager.get(&thread_id) {
            thread_state.emit_drive_updated(&info);
        }
    });
    Ok((StatusCode::ACCEPTED, Json(info)))
}

async fn interrupt_drive_turn(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<client_drive::ClientConnectionInfo>), ApiError> {
    state
        .client_drives
        .interrupt(&id)
        .map_err(client_drive_api_error)?;
    let info = state
        .client_drives
        .get(&id)
        .map_err(client_drive_api_error)?;
    state.emit_drive_updated(&info);
    Ok((StatusCode::ACCEPTED, Json(info)))
}

fn client_drive_api_error(error: client_drive::ClientDriveError) -> ApiError {
    use client_drive::ClientDriveError as Error;
    let status = match error {
        Error::NotFound(_) => StatusCode::NOT_FOUND,
        Error::Conflict(_) => StatusCode::CONFLICT,
        Error::Unsupported(_) => StatusCode::METHOD_NOT_ALLOWED,
        Error::Prepare(_) => StatusCode::BAD_REQUEST,
        Error::Unavailable | Error::Persistence(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.to_string())
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
    /// Command text for the user's default shell to execute.
    shell_command: Option<String>,
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
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
struct TerminalSettings {
    #[serde(default)]
    inherit_global_shell_history: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
struct TrashSettings {
    trash_cleanup_days: u32,
}

fn read_terminal_settings(state: &AppState) -> Result<TerminalSettings, ApiError> {
    read_terminal_settings_from(&Settings::new(state.store.root()))
}

fn read_terminal_settings_from(settings: &Settings) -> Result<TerminalSettings, ApiError> {
    let inherit_global_shell_history = settings
        .get(
            INHERIT_GLOBAL_SHELL_HISTORY_SETTING,
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    Ok(TerminalSettings {
        inherit_global_shell_history,
    })
}

async fn get_terminal_settings(
    State(state): State<AppState>,
) -> Result<Json<TerminalSettings>, ApiError> {
    Ok(Json(read_terminal_settings(&state)?))
}

async fn put_terminal_settings(
    State(state): State<AppState>,
    Json(value): Json<TerminalSettings>,
) -> Result<Json<TerminalSettings>, ApiError> {
    Settings::new(state.store.root())
        .set(
            INHERIT_GLOBAL_SHELL_HISTORY_SETTING,
            serde_json::Value::Bool(value.inherit_global_shell_history),
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(value))
}

async fn get_checkout_terminal_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<TerminalSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    Ok(Json(read_terminal_settings_from(&settings)?))
}

async fn put_checkout_terminal_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(value): Json<TerminalSettings>,
) -> Result<Json<TerminalSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            INHERIT_GLOBAL_SHELL_HISTORY_SETTING,
            serde_json::Value::Bool(value.inherit_global_shell_history),
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(value))
}

async fn get_checkout_trash_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<TrashSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let trash_cleanup_days = settings
        .trash_cleanup_days()
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(TrashSettings { trash_cleanup_days }))
}

async fn put_checkout_trash_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(value): Json<TrashSettings>,
) -> Result<Json<TrashSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            hotsheet_ticketing::TRASH_CLEANUP_DAYS_SETTING,
            serde_json::json!(value.trash_cleanup_days),
            hotsheet_ticketing::Scope::Shared,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(value))
}

/// One terminal as reported by `GET /terminals`.
#[derive(Serialize)]
struct TerminalInfo {
    id: String,
    /// Immutable creation kind; never inferred from terminal output or command names.
    kind: hotsheet_terminals::TerminalKind,
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
    /// The AI tool an `ai` terminal launched (for example `claude`), from its immutable
    /// `<tool>-<id>` worker id; clients name AI tabs after it (HS2-HZK0NK).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
}

/// The AI tool of an `ai` terminal, recovered from its `<tool>-<id>` worker id. Shell terminals,
/// workers that do not follow that shape, and an explicit-command terminal that also connects a
/// tool (whose session worker is in the reserved `terminal` namespace) have none.
fn ai_terminal_tool(
    kind: hotsheet_terminals::TerminalKind,
    worker: Option<&str>,
    id: &str,
) -> Option<String> {
    if kind != hotsheet_terminals::TerminalKind::Ai {
        return None;
    }
    worker?
        .strip_suffix(&format!("-{id}"))
        .filter(|tool| !tool.is_empty() && *tool != "terminal")
        .map(str::to_string)
}

/// The terminal-manager key for a terminal id — the served store root is the project.
fn term_key(state: &AppState, id: &str) -> hotsheet_terminals::TermKey {
    (state.store.root().display().to_string(), id.to_string())
}

fn term_info(term: &hotsheet_terminals::Terminal, id: &str) -> TerminalInfo {
    let osc = term.term_state();
    TerminalInfo {
        id: id.to_string(),
        kind: term.kind(),
        alive: term.is_alive(),
        busy: term.activity() == hotsheet_terminals::Activity::Busy,
        cwd: osc.cwd,
        link: osc.link,
        progress: osc.progress,
        tool: ai_terminal_tool(term.kind(), term.worker_id(), id),
    }
}

/// Map a broker terminal-info onto the HTTP `TerminalInfo`.
fn broker_info(bi: hotsheet_terminals::BrokerTermInfo) -> TerminalInfo {
    let tool = ai_terminal_tool(bi.kind, bi.worker.as_deref(), &bi.id);
    TerminalInfo {
        id: bi.id,
        kind: bi.kind,
        alive: bi.alive,
        busy: bi.busy,
        cwd: bi.cwd,
        link: bi.link,
        progress: bi.progress,
        tool,
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

/// `POST /terminals` `{command?, shell_command?, args?, cwd?, id?, connect?}` — open (or
/// reattach to) a PTY. `shell_command` runs text through the user's default shell and then keeps the
/// terminal open interactively so its output stays visible (HS2-2BKPGK); with no launch field, that
/// shell is opened interactively.
async fn open_terminal(
    State(state): State<AppState>,
    Json(req): Json<OpenTerminalReq>,
) -> Result<Json<TerminalInfo>, ApiError> {
    let id = req.id.clone().unwrap_or_else(|| Ulid::new().to_string());
    let launch_state = state.clone();
    let launch_id = id.clone();
    // Setup and model discovery may launch subprocesses or wait for a shared catalog
    // lock. Keep the entire synchronous preparation off Tokio workers (HS2-Y7W3Z4).
    let (launch, req) = tokio::task::spawn_blocking(move || {
        terminal_launch(&launch_state, &req, &launch_id).map(|launch| (launch, req))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    let kind = if req.connect.is_some() {
        hotsheet_terminals::TerminalKind::Ai
    } else {
        hotsheet_terminals::TerminalKind::Shell
    };
    // Record a launch directory for every terminal, even one whose shell never reports OSC 7:
    // it is what keeps a checkout's stores hosted while the terminal lives (HS2-R5KV1Q).
    let cwd = req
        .cwd
        .clone()
        .unwrap_or_else(|| state.store.root().to_string_lossy().into_owned());

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
                kind,
                command: launch.command,
                args: launch.args,
                cwd: Some(cwd),
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
        if newly_spawned {
            match &req.connect {
                Some(tool) => register_broker_terminal_connection(&state, &id, tool, tb.clone()),
                None => watch_broker_terminal_session_exit(
                    &state,
                    &id,
                    terminal_session_worker_id(&id),
                    tb.clone(),
                ),
            }
        }
        return Ok(Json(broker_info(info)));
    }

    let newly_spawned = state.terminals.get(&term_key(&state, &id)).is_none();
    let spec = hotsheet_terminals::TermSpec {
        kind,
        command: launch.command,
        args: launch.args,
        cwd: Some(std::path::PathBuf::from(cwd)),
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
    if newly_spawned {
        match &req.connect {
            Some(tool) => register_terminal_connection(&state, &id, tool, term.clone()),
            None => {
                watch_terminal_session_exit(&state, term.clone(), terminal_session_worker_id(&id))
            }
        }
    }
    Ok(Json(term_info(&term, &id)))
}

/// Compose the caller's explicit command, a plugin-declared interactive launch, or the user's
/// default shell. Connect-only launch performs setup first so MCP/instructions/hooks exist before
/// the tool starts, resolves the program without a shell, and injects this server's permission
/// route.
struct PreparedTerminalLaunch {
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

fn terminal_launch(
    state: &AppState,
    req: &OpenTerminalReq,
    terminal_id: &str,
) -> Result<PreparedTerminalLaunch, ApiError> {
    if let Some(shell_command) = &req.shell_command {
        if req.command.is_some() || req.connect.is_some() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "shell_command cannot be combined with command or connect",
            ));
        }
        let command = user_default_shell();
        return Ok(PreparedTerminalLaunch {
            args: shell_command_args(shell_command, &command),
            env: terminal_session_env(state, req, terminal_id, &command)?,
            command,
        });
    }
    if let Some(command) = &req.command {
        return Ok(PreparedTerminalLaunch {
            command: command.clone(),
            args: req.args.clone(),
            env: terminal_session_env(state, req, terminal_id, command)?,
        });
    }
    let Some(tool) = req.connect.as_deref() else {
        let command = user_default_shell();
        return Ok(PreparedTerminalLaunch {
            env: terminal_session_env(state, req, terminal_id, &command)?,
            command,
            args: req.args.clone(),
        });
    };
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
    if req.model.is_some() || req.effort.is_some() {
        hotsheet_plugins::validate_ai_defaults(
            &discovered_ai_tools(state, false),
            &hotsheet_plugins::AiToolDefaults {
                tool: tool.to_string(),
                model: req.model.clone(),
                effort: req.effort.clone(),
                ..Default::default()
            },
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    }
    let program = hotsheet_aitools::launch_safety::resolve_program(&launch.program)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    let mut env = terminal_permission_route_env(state, req);
    env.push(("HOTSHEET_AGENT".to_string(), tool.to_string()));
    // The session's worker id: its claims are released when the terminal exits (HS2-1VAW1C).
    let worker_id = hotsheet_aitools::session_worker_id(tool, terminal_id);
    // An AI terminal session's CLI/MCP mutations act as `ai` (HS2-RD4M29).
    env.extend(
        hotsheet_aitools::ai_session_actor_env(&worker_id)
            .map(|(key, value)| (key.to_string(), value)),
    );
    env.push((hotsheet_aitools::WORKER_ID_ENV.to_string(), worker_id));
    let args = plugin
        .launch_args(req.model.as_deref(), req.effort.as_deref())
        .unwrap_or_default();
    Ok(PreparedTerminalLaunch {
        command: program.to_string_lossy().into_owned(),
        args,
        env,
    })
}

fn user_default_shell() -> String {
    #[cfg(windows)]
    let (variable, fallback) = ("COMSPEC", "cmd.exe");
    #[cfg(not(windows))]
    let (variable, fallback) = ("SHELL", "/bin/sh");

    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback.to_string())
}

/// Build the shell args that run a custom command button. The terminal must stay open with the
/// output visible instead of exiting the moment the command finishes (HS2-2BKPGK): on Windows `/K`
/// runs the command and keeps the prompt (vs `/C`, which exits); on POSIX the command runs and then
/// execs an interactive login shell.
fn shell_command_args(command: &str, shell: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let _ = shell;
        return vec![
            "/D".to_string(),
            "/S".to_string(),
            "/K".to_string(),
            command.to_string(),
        ];
    }
    #[cfg(not(windows))]
    return vec![
        "-lc".to_string(),
        format!("{command}\nexec {} -il", shell_single_quote(shell)),
    ];
}

/// Single-quote a shell word so a shell path with spaces or quotes stays one argument.
#[cfg(not(windows))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

const INHERIT_GLOBAL_SHELL_HISTORY_SETTING: &str = "terminal.inherit_global_shell_history";

/// Per-terminal shell history lives under the state's injected machine home, never a
/// direct `HOTSHEET_HOME` read (HS2-FQEESP).
fn terminal_history_home(state: &AppState) -> std::path::PathBuf {
    state.machine_home().join("terminal-history")
}

fn history_key(value: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value.as_bytes()))[..16].to_string()
}

fn shell_history_environment(
    history_home: &FsPath,
    project: &FsPath,
    terminal_id: &str,
    command: &str,
    inherit: bool,
) -> std::io::Result<Vec<(String, String)>> {
    if inherit {
        return Ok(Vec::new());
    }
    let shell = FsPath::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
        .trim_start_matches('-');
    let project_key = history_key(&project.to_string_lossy());
    let terminal_key = history_key(terminal_id);
    match shell {
        "bash" => {
            let directory = history_home.join(&project_key).join(&terminal_key);
            std::fs::create_dir_all(&directory)?;
            Ok(vec![(
                "HISTFILE".into(),
                directory
                    .join(format!("{shell}_history"))
                    .to_string_lossy()
                    .into_owned(),
            )])
        }
        "zsh" => {
            let directory = history_home.join(&project_key).join(&terminal_key);
            let zdotdir = directory.join("zdotdir");
            let history = directory.join("zsh_history");
            std::fs::create_dir_all(&zdotdir)?;
            let original = std::env::var_os("ZDOTDIR")
                .map(std::path::PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from));
            let quoted = |path: &FsPath| path.to_string_lossy().replace('\'', "'\\''");
            for name in [".zshenv", ".zprofile", ".zshrc", ".zlogin", ".zlogout"] {
                let source = original
                    .as_ref()
                    .map(|root| root.join(name))
                    .filter(|path| path != &zdotdir.join(name));
                let source_line = source.map_or_else(String::new, |path| {
                    format!(
                        "[[ -r '{}' ]] && source '{}'\n",
                        quoted(&path),
                        quoted(&path)
                    )
                });
                std::fs::write(
                    zdotdir.join(name),
                    format!(
                        "{source_line}export ZDOTDIR='{}'\nexport HISTFILE='{}'\nexport SHELL_SESSIONS_DISABLE=1\n",
                        quoted(&zdotdir),
                        quoted(&history)
                    ),
                )?;
            }
            Ok(vec![
                ("HISTFILE".into(), history.to_string_lossy().into_owned()),
                ("ZDOTDIR".into(), zdotdir.to_string_lossy().into_owned()),
                ("SHELL_SESSIONS_DISABLE".into(), "1".into()),
            ])
        }
        // Fish selects a durable history file by session name. Keeping XDG_DATA_HOME intact
        // preserves the user's functions and universal variables while isolating recall.
        "fish" => Ok(vec![(
            "fish_history".into(),
            format!("hotsheet_{project_key}_{terminal_key}"),
        )]),
        _ => Ok(Vec::new()),
    }
}

/// The permission route-back every Hot Sheet terminal carries, so a hook-capable tool
/// (Claude, Codex) started by hand in a shell raises its permission prompts in the app just
/// like a Connect-launched one (HS2-HE4AVD). Without a known server URL, only the project is
/// set and the tool's native prompt stays in charge.
fn terminal_permission_route_env(state: &AppState, req: &OpenTerminalReq) -> Vec<(String, String)> {
    let project = terminal_permission_project(state, req);
    let mut env = vec![
        ("HOTSHEET_SECRET".to_string(), state.secret.clone()),
        ("HOTSHEET_PROJECT".to_string(), project),
    ];
    if let Ok(url) = state.terminal_server_url.lock()
        && let Some(url) = url.as_ref()
    {
        env.push(("HOTSHEET_SERVER".to_string(), url.clone()));
    }
    env
}

fn terminal_permission_project(state: &AppState, req: &OpenTerminalReq) -> String {
    let Some(cwd) = req.cwd.as_deref() else {
        return state.store.root().display().to_string();
    };
    let cwd = FsPath::new(cwd)
        .canonicalize()
        .unwrap_or_else(|_| cwd.into());
    let checkout = state
        .checkout_registry
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|checkout| cwd.starts_with(&checkout.root))
        .max_by_key(|checkout| checkout.root.len());
    if let Some(checkout) = checkout {
        let git_source = checkout
            .default_source
            .as_deref()
            .and_then(|id| checkout.source(id))
            .filter(|source| source.provider == "git")
            .or_else(|| {
                checkout
                    .sources
                    .iter()
                    .find(|source| source.provider == "git")
            });
        return git_source
            .map(|source| source.locator.clone())
            .unwrap_or(checkout.root);
    }
    cwd.display().to_string()
}

/// Shell and command terminals: [`terminal_shell_history_env`] plus the session worker id.
fn terminal_session_env(
    state: &AppState,
    req: &OpenTerminalReq,
    terminal_id: &str,
    command: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let mut env = terminal_shell_history_env(state, req, terminal_id, command)?;
    env.push((
        hotsheet_aitools::WORKER_ID_ENV.to_string(),
        terminal_session_worker_id(terminal_id),
    ));
    Ok(env)
}

/// Shell and command terminals: per-terminal shell history plus the permission route-back.
fn terminal_shell_history_env(
    state: &AppState,
    req: &OpenTerminalReq,
    terminal_id: &str,
    command: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let mut env = terminal_permission_route_env(state, req);
    env.extend(terminal_shell_history_only_env(
        state,
        req,
        terminal_id,
        command,
    )?);
    Ok(env)
}

fn terminal_shell_history_only_env(
    state: &AppState,
    req: &OpenTerminalReq,
    terminal_id: &str,
    command: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let inherit = read_terminal_settings(state)?.inherit_global_shell_history;
    let project = req
        .cwd
        .as_deref()
        .map(FsPath::new)
        .unwrap_or_else(|| state.store.root());
    shell_history_environment(
        &terminal_history_home(state),
        project,
        terminal_id,
        command,
        inherit,
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("preparing terminal history: {error}"),
        )
    })
}

#[cfg(test)]
mod foreground_command_tests {
    use super::ForegroundCommand;

    #[test]
    fn a_finish_is_a_running_to_idle_transition_only() {
        let mut fg = ForegroundCommand::default();
        assert!(!fg.finished(Some(false)), "idle at the prompt");
        assert!(!fg.finished(None), "unknown changes nothing");
        assert!(!fg.finished(Some(true)), "a command starts");
        assert!(!fg.finished(Some(true)), "still running");
        assert!(!fg.finished(None), "a missed observation keeps it running");
        assert!(fg.finished(Some(false)), "back at the prompt: finished");
        assert!(!fg.finished(Some(false)), "reported once");
        assert!(!fg.finished(Some(true)));
        assert!(fg.finished(Some(false)), "a later command finishes too");
    }
}

#[cfg(test)]
mod terminal_history_tests {
    use super::{shell_command_args, shell_history_environment};

    /// HS2-FQEESP: terminal history is rooted at the state's injected machine home, so two
    /// states in one process never share (or touch the real) `~/.hotsheet2` history.
    #[test]
    fn terminal_history_lives_under_the_injected_machine_home() {
        let store_dir = tempfile::tempdir().unwrap();
        let store = hotsheet_ticketing::FsStore::init(
            store_dir.path(),
            &hotsheet_ticketing::StoreMetadata::new("HS"),
        )
        .unwrap();
        let home_a = tempfile::tempdir().unwrap();
        let home_b = tempfile::tempdir().unwrap();
        let state = super::AppState::new(store, "secret".into()).unwrap();
        let state_a = state.clone().with_machine_home(home_a.path());
        let state_b = state.with_machine_home(home_b.path());
        let req: super::OpenTerminalReq = serde_json::from_str("{}").unwrap();
        for (state, home) in [(&state_a, home_a.path()), (&state_b, home_b.path())] {
            let env =
                super::terminal_shell_history_only_env(state, &req, "term-1", "/bin/bash").unwrap();
            let histfile = &env.iter().find(|(key, _)| key == "HISTFILE").unwrap().1;
            assert!(
                std::path::Path::new(histfile).starts_with(home.join("terminal-history")),
                "{histfile} must live under {}",
                home.display()
            );
        }
    }

    #[test]
    fn shell_command_uses_the_platform_shell_command_boundary() {
        let args = shell_command_args("npm run lint", "/bin/zsh");
        // The terminal keeps running after the command so its output stays visible (HS2-2BKPGK):
        // Windows uses /K (not /C), POSIX runs the command then execs an interactive login shell.
        #[cfg(windows)]
        assert_eq!(args, ["/D", "/S", "/K", "npm run lint"]);
        #[cfg(not(windows))]
        assert_eq!(args, ["-lc", "npm run lint\nexec '/bin/zsh' -il"]);
    }

    #[test]
    fn isolates_bash_and_zsh_by_project_and_terminal_with_restart_stable_paths() {
        let home = tempfile::tempdir().unwrap();
        let project_a = home.path().join("project-a");
        let project_b = home.path().join("project-b");
        let bash_a =
            shell_history_environment(home.path(), &project_a, "one", "/bin/bash", false).unwrap();
        let bash_again =
            shell_history_environment(home.path(), &project_a, "one", "/bin/bash", false).unwrap();
        let bash_other_terminal =
            shell_history_environment(home.path(), &project_a, "two", "/bin/bash", false).unwrap();
        let bash_other_project =
            shell_history_environment(home.path(), &project_b, "one", "/bin/bash", false).unwrap();
        let zsh =
            shell_history_environment(home.path(), &project_a, "one", "/bin/zsh", false).unwrap();
        assert_eq!(bash_a, bash_again);
        assert_ne!(bash_a, bash_other_terminal);
        assert_ne!(bash_a, bash_other_project);
        assert!(bash_a[0].1.ends_with("bash_history"));
        assert!(
            zsh.iter()
                .any(|(key, value)| key == "HISTFILE" && value.ends_with("zsh_history"))
        );
        assert!(
            zsh.iter()
                .any(|(key, value)| key == "SHELL_SESSIONS_DISABLE" && value == "1")
        );
        let zdotdir = zsh.iter().find(|(key, _)| key == "ZDOTDIR").unwrap();
        assert!(std::path::Path::new(&zdotdir.1).join(".zshrc").is_file());
        if std::path::Path::new("/bin/zsh").is_file() {
            let output = std::process::Command::new("/bin/zsh")
                .args(["-ic", "print -r -- $HISTFILE"])
                .envs(zsh.iter().cloned())
                .output()
                .unwrap();
            let printed = String::from_utf8_lossy(&output.stdout);
            assert_eq!(
                printed.trim(),
                zsh.iter().find(|(key, _)| key == "HISTFILE").unwrap().1
            )
        }
        assert!(
            std::path::Path::new(&bash_a[0].1)
                .parent()
                .unwrap()
                .is_dir()
        )
    }
    #[test]
    fn gives_fish_a_stable_isolated_session_and_supports_global_opt_out() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("project");
        let fish = shell_history_environment(
            home.path(),
            &project,
            "terminal-1",
            "/opt/homebrew/bin/fish",
            false,
        )
        .unwrap();
        assert_eq!(fish[0].0, "fish_history");
        assert!(fish[0].1.starts_with("hotsheet_"));
        assert!(
            shell_history_environment(home.path(), &project, "terminal-1", "/bin/zsh", true)
                .unwrap()
                .is_empty()
        );
        assert!(
            shell_history_environment(home.path(), &project, "terminal-1", "python", false)
                .unwrap()
                .is_empty()
        )
    }
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
    let host = state.host.clone();
    let worker = hotsheet_aitools::session_worker_id(tool, id);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let alive = term.is_alive();
            if !alive {
                if let Ok(mut r) = registry.lock() {
                    r.unregister(&conn_id);
                }
                release_session_claims(host, worker).await;
                break;
            }
            if let Ok(mut r) = registry.lock() {
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
    let host = state.host.clone();
    let worker = hotsheet_aitools::session_worker_id(tool, id);
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
                    release_session_claims(host, worker).await;
                    break;
                }
            }
        }
    });
}

/// Release every claim an ended AI session's worker still holds in any hosted store: the
/// safety net for a session that stopped without releasing (HS2-1VAW1C). The store watcher
/// reindexes and announces the released tickets like any other write.
async fn release_session_claims(host: multistore::StoreHost, worker: String) {
    let _ =
        tokio::task::spawn_blocking(move || release_session_claims_blocking(&host, &worker)).await;
}

/// The blocking body of [`release_session_claims`], for callers already off the async runtime
/// (a chat drive's turn thread). Returns the released ticket slugs.
fn release_session_claims_blocking(host: &multistore::StoreHost, worker: &str) -> Vec<String> {
    let mut released = Vec::new();
    for (id, _) in host.locations() {
        let Some(entry) = host.get(&id) else {
            continue;
        };
        match ops::release_worker(&entry.store, now(), worker) {
            Ok(tickets) => released.extend(tickets.into_iter().map(|ticket| ticket.slug)),
            Err(error) => eprintln!("releasing {worker}'s claims in {id} failed: {error}"),
        }
    }
    if !released.is_empty() {
        eprintln!("released claims left by {worker}: {}", released.join(", "));
    }
    released
}

/// The worker id of a shell or command terminal's session. A user may start an AI tool in it
/// by hand; the tool claims with this id and the server releases it when the terminal ends
/// (HS2-RXWXQ8).
fn terminal_session_worker_id(terminal_id: &str) -> String {
    hotsheet_aitools::session_worker_id("terminal", terminal_id)
}

/// Tracks a shell terminal's foreground command so its session claims are released when a
/// program the user started there (such as an AI tool) exits back to the prompt, while the shell
/// stays open (HS2-WQQYT1).
#[derive(Debug, Default)]
struct ForegroundCommand {
    running: bool,
}

impl ForegroundCommand {
    /// Record the latest observation; `true` when a running command just finished. An unknown
    /// observation (`None`) changes nothing.
    fn finished(&mut self, observed: Option<bool>) -> bool {
        let Some(running) = observed else {
            return false;
        };
        let finished = self.running && !running;
        self.running = running;
        finished
    }
}

/// Release a shell or command terminal's session claims when its foreground command finishes
/// and once its in-process PTY exits.
fn watch_terminal_session_exit(
    state: &AppState,
    term: std::sync::Arc<hotsheet_terminals::Terminal>,
    worker: String,
) {
    let host = state.host.clone();
    tokio::spawn(async move {
        let mut foreground = ForegroundCommand::default();
        while term.is_alive() {
            if foreground.finished(term.foreground_command_running()) {
                release_session_claims(host.clone(), worker.clone()).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        release_session_claims(host, worker).await;
    });
}

/// The broker-mode counterpart of [`watch_terminal_session_exit`]: poll the broker until the
/// terminal is gone, not alive, or the broker is unreachable, then release the claims.
fn watch_broker_terminal_session_exit(
    state: &AppState,
    id: &str,
    worker: String,
    broker: terminal_broker::TerminalBroker,
) {
    let host = state.host.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        let mut foreground = ForegroundCommand::default();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            match broker
                .call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
                .await
            {
                Ok(hotsheet_terminals::BrokerResponse::Read { info, .. }) if info.alive => {
                    if foreground.finished(info.foreground_command) {
                        release_session_claims(host.clone(), worker.clone()).await;
                    }
                }
                _ => break,
            }
        }
        release_session_claims(host, worker).await;
    });
}

/// Release the claims of chat drives the previous server run left open: drives live only in
/// server memory, so their tool sessions ended with it (HS2-VFXEF4). Each drive's store is
/// opened directly because a project's store may not be hosted yet. Returns the released slugs.
pub async fn release_orphaned_drive_sessions(state: &AppState) -> Vec<String> {
    let orphaned = state.client_drives.take_orphaned_drives();
    if orphaned.is_empty() {
        return Vec::new();
    }
    tokio::task::spawn_blocking(move || {
        let mut released = Vec::new();
        for drive in orphaned {
            match FsStore::open(&drive.store_path)
                .map_err(|error| error.to_string())
                .and_then(|store| {
                    ops::release_worker(&store, now(), &drive.worker_id)
                        .map_err(|error| error.to_string())
                }) {
                Ok(tickets) => released.extend(tickets.into_iter().map(|ticket| ticket.slug)),
                Err(error) => eprintln!(
                    "releasing {}'s claims in {} failed: {error}",
                    drive.worker_id,
                    drive.store_path.display()
                ),
            }
        }
        if !released.is_empty() {
            eprintln!(
                "released claims left by ended chat drives: {}",
                released.join(", ")
            );
        }
        released
    })
    .await
    .unwrap_or_default()
}

/// After a restart, resume the session monitors of terminals that survived in the broker: an
/// AI terminal gets its busy feed and exit release back, a shell terminal its exit release.
/// Terminals from a broker too old to report a worker id keep relying on lease expiry.
pub async fn resume_broker_terminal_sessions(state: &AppState) {
    let Some(broker) = state.terminal_broker.clone() else {
        return;
    };
    let Ok(hotsheet_terminals::BrokerResponse::List { terminals }) =
        broker.call(hotsheet_terminals::BrokerRequest::List).await
    else {
        return;
    };
    for info in terminals.into_iter().filter(|info| info.alive) {
        let Some(worker) = info.worker else {
            continue;
        };
        let tool = worker
            .strip_suffix(&format!("-{}", info.id))
            .filter(|tool| !tool.is_empty());
        match (info.kind, tool) {
            (hotsheet_terminals::TerminalKind::Ai, Some(tool)) => {
                register_broker_terminal_connection(state, &info.id, tool, broker.clone());
            }
            _ => watch_broker_terminal_session_exit(state, &info.id, worker, broker.clone()),
        }
    }
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
            hotsheet_terminals::BrokerResponse::Ok
            | hotsheet_terminals::BrokerResponse::NotFound => Ok(StatusCode::NO_CONTENT),
            other => Err(broker_err(other)),
        };
    }
    state
        .terminals
        .kill(&term_key(&state, &id))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
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
/// (HS2-BD7Q74). Ordinary Text is raw PTY input so a simple client can type over the same
/// socket; malformed JSON carrying the reserved `resize` member is dropped (Binary is input).
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
    /// This claim was driven by a genuine user interaction (tap/click/focus/keystroke) rather
    /// than a heartbeat; only interacting claims advance the size-arbiter recency (HS2-3ZBQDG).
    #[serde(default)]
    interacting: bool,
}

enum TerminalText<'a> {
    Resize(ResizeClaim),
    InvalidControl,
    Input(&'a str),
}

fn classify_terminal_text(value: &str) -> TerminalText<'_> {
    if let Ok(TermControl {
        resize: Some(resize),
    }) = serde_json::from_str(value)
    {
        return TerminalText::Resize(resize);
    }
    if serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|json| json.get("resize").cloned())
        .is_some()
    {
        return TerminalText::InvalidControl;
    }
    TerminalText::Input(value)
}

#[cfg(test)]
mod terminal_text_tests {
    use super::{TerminalText, classify_terminal_text, terminal_replay_control};
    use axum::extract::ws::Message;

    #[test]
    fn malformed_resize_controls_are_dropped_instead_of_becoming_pty_input() {
        assert!(matches!(
            classify_terminal_text(r#"{"resize":{"viewer_id":"viewer","cols":null,"rows":null}}"#),
            TerminalText::InvalidControl
        ));
        assert!(matches!(
            classify_terminal_text(r#"{"resize":{"viewer_id":"viewer","cols":80,"rows":24}}"#),
            TerminalText::Resize(_)
        ));
        assert!(matches!(
            classify_terminal_text("echo hello\n"),
            TerminalText::Input("echo hello\n")
        ));
    }

    #[test]
    fn lag_resync_has_an_explicit_replacement_control() {
        assert!(matches!(
            terminal_replay_control(),
            Message::Text(value) if value.as_str() == r#"{"terminal_replay":"replace"}"#
        ));
    }
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

/// A replay after initial attach replaces the viewer's emulator state rather than appending.
fn terminal_replay_control() -> Message {
    Message::Text(r#"{"terminal_replay":"replace"}"#.into())
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

    // Snapshot and subscribe under one output boundary so concurrent PTY output lands exactly
    // once in either the replay or the live stream (HS2-5W0V9M).
    let (snapshot, mut rx) = term.subscribe_with_scrollback();
    let mut size_rx = term.subscribe_size();
    let mut my_viewer: Option<String> = None;

    // An empty binary replay is still the attach boundary. Reconnecting browsers use it to
    // atomically clear stale emulator state without blanking while waiting for a payload.
    if socket.send(Message::Binary(snapshot.into())).await.is_err() {
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
                    let (snap, replacement) = term.subscribe_with_scrollback();
                    rx = replacement;
                    if socket.send(terminal_replay_control()).await.is_err() {
                        break;
                    }
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
                    match classify_terminal_text(&t) {
                        // A size claim: feed the arbiter (it resizes + broadcasts if the size changed).
                        TerminalText::Resize(r) => {
                            my_viewer = Some(r.viewer_id.clone());
                            let now = term_now_ms();
                            term.claim_size(
                                hotsheet_terminals::ViewportClaim {
                                    viewer_id: r.viewer_id,
                                    cols: r.cols,
                                    rows: r.rows,
                                    focus: r.focus,
                                    visible: r.visible,
                                    interacting: r.interacting,
                                    activity_at_ms: now,
                                },
                                now,
                            );
                        }
                        // A malformed control frame is never terminal input: dropping it avoids
                        // echoing protocol JSON into the user's shell during transient layout.
                        TerminalText::InvalidControl => {}
                        TerminalText::Input(input) => { let _ = term.write(input.as_bytes()); }
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
    let mut received_initial_replay = false;

    loop {
        tokio::select! {
            frame = stream.next() => match frame {
                Ok(Some(f)) => {
                    use hotsheet_terminals::StreamOut as S;
                    match f {
                        S::Scrollback { data } => {
                            if received_initial_replay
                                && socket.send(terminal_replay_control()).await.is_err()
                            {
                                break;
                            }
                            received_initial_replay = true;
                            if socket.send(Message::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        S::Output { data } => {
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
                    let sent = match classify_terminal_text(&t) {
                        // A size claim forwards as a Resize frame the broker feeds to its arbiter.
                        TerminalText::Resize(r) => {
                            stream
                                .send(&hotsheet_terminals::StreamIn::Resize {
                                    viewer_id: r.viewer_id,
                                    cols: r.cols,
                                    rows: r.rows,
                                    focus: r.focus,
                                    visible: r.visible,
                                    interacting: r.interacting,
                                })
                                .await
                        }
                        TerminalText::InvalidControl => continue,
                        TerminalText::Input(input) => {
                            stream
                                .send(&hotsheet_terminals::StreamIn::Input {
                                    data: input.as_bytes().to_vec(),
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
    let rules_path = state
        .permission_rule_paths
        .lock()
        .unwrap()
        .get(&resolved.project)
        .cloned();
    if let (Some(rule), Some(path)) = (&resolved.persisted_rule, rules_path.as_ref()) {
        match hotsheet_aitools::append_permission_rule(path, rule) {
            Ok(()) => persisted = true,
            Err(e) => eprintln!("failed to persist permission rule: {e}"),
        }
    }
    // Resolution can happen through another client or transport. Publish a replayable
    // nudge so every notification inbox reconciles the now-absent request into history.
    state.emit(ChangeEvent {
        cursor: None,
        store: resolved.project.clone(),
        kind: "permission_resolved".to_string(),
        id: id.to_string(),
        slug: String::new(),
        message: Some(format!("{event_decision}:{event_scope}")),
        activity: None,
        assignment: None,
        turn: None,
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
    Ok(Json(do_close(&state, &entry, &id, None, req)?))
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
    // Setup writes instruction/MCP/permission files and may probe the tool, so it runs on
    // the blocking pool instead of occupying an async request thread (HS2-9TV33W).
    let store = state.store.root().to_path_buf();
    let plugin_dirs = state.plugin_dirs.clone();
    let reports = tokio::task::spawn_blocking(move || {
        hotsheet_plugins::run_setup_in(&store, &store, Some(&tool), false, None, &plugin_dirs)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    // Setting a tool up changes its plugin state; the next discovery must rescan.
    state.invalidate_ai_tool_discovery();
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
    let lease = begin_presence(
        &state,
        params.checkout.as_deref(),
        params.client.as_deref(),
        presence::Channel::Socket,
    );
    ws.on_upgrade(move |socket| async move {
        ws_loop(socket, rx).await;
        drop(lease);
    })
}

/// Start a change-stream lease. A lease on a checkout also brings its stores and repository
/// monitor back if an earlier sweep stopped them, off the request path.
fn begin_presence(
    state: &AppState,
    checkout: Option<&str>,
    client: Option<&str>,
    channel: presence::Channel,
) -> presence::PresenceGuard {
    let lease = state.presence.begin(checkout, client, channel);
    state.request_unhost_sweep(presence::POLL_RECONNECT_GAP + presence::UNHOST_GRACE);
    if let Some(reference) = checkout.filter(|reference| !reference.is_empty()) {
        let state = state.clone();
        let reference = reference.to_string();
        tokio::task::spawn_blocking(move || {
            let Ok(checkout) = state.checkout_registry.resolve(&reference) else {
                return;
            };
            for source in checkout.sources.iter().filter(|s| s.provider == "git") {
                state.hosted_source(source);
            }
            state.watch_checkout_repository(&checkout);
        });
    }
    lease
}

/// `POST /checkouts/{reference}/close?client=<id>` — a client closed this project, so its
/// lease ends now and the project's stores are unhosted unless something still needs them
/// (HS2-ARJ9J1).
async fn close_checkout_session(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(params): Query<CloseSessionParams>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    state.presence.close(&checkout.id, params.client.as_deref());
    state.request_unhost_sweep(Duration::ZERO);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct CloseSessionParams {
    client: Option<String>,
}

/// When the next unhost sweep runs; a request for an earlier time wakes the sweeper.
#[derive(Default)]
struct UnhostSweep {
    due: Mutex<Option<tokio::time::Instant>>,
    wake: tokio::sync::Notify,
    started: std::sync::atomic::AtomicBool,
}

/// Run unhost sweeps as they come due (HS2-ARJ9J1). A local timer only: it issues no network
/// requests, and stays idle when no project store is hosted.
async fn run_unhost_sweeper(state: AppState) {
    loop {
        let due = state.unhost_sweep.due.lock().ok().and_then(|due| *due);
        let Some(at) = due else {
            state.unhost_sweep.wake.notified().await;
            continue;
        };
        tokio::select! {
            () = tokio::time::sleep_until(at) => {
                if let Ok(mut due) = state.unhost_sweep.due.lock()
                    && due.is_some_and(|due| due <= tokio::time::Instant::now())
                {
                    *due = None;
                }
                sweep_unhosted(&state).await;
            }
            () = state.unhost_sweep.wake.notified() => {}
        }
    }
}

/// Unhost every project store that no open checkout references and no live work needs: a
/// drive on the store, a terminal inside a checkout that uses it, or a live claim in it.
/// Repository monitors of checkouts nobody has open stop too. While project stores remain
/// hosted, another sweep is scheduled for when their leases could have lapsed.
async fn sweep_unhosted(state: &AppState) {
    let live = state.presence.live();
    let terminal_dirs = live_terminal_dirs(state).await;
    let drive_sources: std::collections::HashSet<String> = state
        .client_drives
        .list()
        .into_iter()
        .map(|drive| drive.source)
        .collect();
    let sweeping = state.clone();
    let remaining = tokio::task::spawn_blocking(move || {
        let state = sweeping;
        let checkouts = state.checkout_registry.list().unwrap_or_default();
        // Checkout roots are canonical; compare terminal directories in the same form (for
        // example `/var` vs `/private/var` on macOS) or a terminal inside a checkout never matches.
        let terminal_dirs: Vec<std::path::PathBuf> = terminal_dirs
            .into_iter()
            .map(|dir| dir.canonicalize().unwrap_or(dir))
            .collect();
        let in_terminal = |checkout: &hotsheet_ticketing::checkouts::Checkout| {
            terminal_dirs
                .iter()
                .any(|dir| dir.starts_with(&checkout.root))
        };
        let hosted: Vec<String> = state
            .project_hosted
            .lock()
            .map(|hosted| hosted.iter().cloned().collect())
            .unwrap_or_default();
        let mut eligible = Vec::new();
        let mut busy = std::collections::HashSet::new();
        for id in hosted {
            let referencing: Vec<&hotsheet_ticketing::checkouts::Checkout> = checkouts
                .iter()
                .filter(|checkout| {
                    checkout
                        .sources
                        .iter()
                        .any(|source| source.provider == "git" && source.connection_id == id)
                })
                .collect();
            let referenced_live = live.untagged
                || referencing
                    .iter()
                    .any(|checkout| live.checkouts.contains(&checkout.id));
            if !referenced_live
                && (drive_sources.contains(&id)
                    || referencing.iter().any(|checkout| in_terminal(checkout))
                    || store_has_live_claim(&state, &id))
            {
                busy.insert(id.clone());
            }
            eligible.push((id, referencing.iter().map(|c| c.id.clone()).collect()));
        }
        let candidates = presence::unhost_candidates(&eligible, &live, &busy);
        for id in &candidates {
            state.unhost_store(id);
        }
        if !live.untagged {
            let stopped: Vec<RepositoryWatchHandle> = state
                .repository_watchers
                .lock()
                .map(|mut watchers| {
                    checkouts
                        .iter()
                        .filter(|checkout| {
                            !live.checkouts.contains(&checkout.id) && !in_terminal(checkout)
                        })
                        .filter_map(|checkout| watchers.remove(&checkout.id))
                        .collect()
                })
                .unwrap_or_default();
            drop(stopped);
        }
        eligible.len() - candidates.len()
    })
    .await
    .unwrap_or(0);
    if remaining > 0 {
        state.request_unhost_sweep(presence::POLL_RECONNECT_GAP + presence::UNHOST_GRACE);
    }
}

/// The working directories of live terminals, which keep their checkout's stores hosted.
async fn live_terminal_dirs(state: &AppState) -> Vec<std::path::PathBuf> {
    let infos: Vec<TerminalInfo> = if let Some(broker) = &state.terminal_broker {
        match broker.call(hotsheet_terminals::BrokerRequest::List).await {
            Ok(hotsheet_terminals::BrokerResponse::List { terminals }) => {
                terminals.into_iter().map(broker_info).collect()
            }
            _ => Vec::new(),
        }
    } else {
        state
            .terminals
            .list()
            .into_iter()
            .filter_map(|key| state.terminals.get(&key).map(|t| term_info(&t, &key.1)))
            .collect()
    };
    infos
        .into_iter()
        .filter(|info| info.alive)
        .filter_map(|info| info.cwd.map(std::path::PathBuf::from))
        .collect()
}

/// Whether any ticket in a hosted store is claimed under a lease that has not expired.
fn store_has_live_claim(state: &AppState, id: &str) -> bool {
    let Some(entry) = state.host.get(id) else {
        return false;
    };
    let now = now();
    entry.store.list_tickets().is_ok_and(|tickets| {
        tickets
            .iter()
            .any(|ticket| ticket.claimed_by.is_some() && !ops::claim_available(ticket, &now))
    })
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
    /// The checkout this subscription serves and the client holding it (HS2-ARJ9J1).
    checkout: Option<String>,
    client: Option<String>,
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
    let _lease = begin_presence(
        &state,
        params.checkout.as_deref(),
        params.client.as_deref(),
        presence::Channel::Poll,
    );
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
    let (backlog, overflow, snapshot_cursor) = match state.event_log.lock() {
        Ok(log) => {
            let (events, overflow) = log.since(since);
            (events, overflow, log.seq)
        }
        Err(_) => (Vec::new(), false, since),
    };
    if overflow || !backlog.is_empty() {
        return Json(PollResponse {
            cursor: snapshot_cursor,
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
    // A stopping server ends the wait early with the ordinary empty "timeout" reply, so a
    // long poll never holds the shutdown drain open (HS2-W1KJR4).
    let next = tokio::select! {
        received = tokio::time::timeout(wait, rx.recv()) => received.map_err(|_| ()),
        () = state.stopping() => Err(()),
    };
    let (events, cursor, overflow) = match next {
        // Re-read the ring rather than returning only the wake-up event. This atomically
        // captures every event + the exact cursor through that span, so a burst racing the
        // response cannot advance the cursor past an event the client never received.
        Ok(Ok(_)) => match state.event_log.lock() {
            Ok(log) => {
                let (events, overflow) = log.since(since);
                (events, log.seq, overflow)
            }
            Err(_) => (Vec::new(), since, false),
        },
        // Lagged (fell behind the broadcast buffer) → signal overflow so the client re-syncs.
        Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
            return Json(PollResponse {
                cursor: state.event_cursor(),
                events: Vec::new(),
                overflow: true,
            })
            .into_response();
        }
        Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => {
            (Vec::new(), state.event_cursor(), false)
        }
    };
    Json(PollResponse {
        cursor,
        events,
        overflow,
    })
    .into_response()
}

// ---- request / response DTOs -----------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
struct ListParams {
    status: Option<String>,
    /// Built-in multi-status client collection (`queue`, `archive`, or `trash`).
    collection: Option<String>,
    priority: Option<String>,
    category: Option<String>,
    /// Comma-separated; a ticket must carry all of them.
    tags: Option<String>,
    text: Option<String>,
    up_next: Option<bool>,
    open: Option<bool>,
    /// Filter by close reason (completed|not_planned|duplicate|obsolete|works_as_designed).
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
    completed_after: Option<String>,
    completed_before: Option<String>,
    verified_after: Option<String>,
    verified_before: Option<String>,
    /// Inclusive bounds on the derived `latest_confidence` (0-100, HS2-RD4M29).
    min_confidence: Option<u8>,
    max_confidence: Option<u8>,
    has_attachment: Option<bool>,
    has_media_annotation: Option<bool>,
    /// Checkout-only filter: whether repository commits reference the ticket slug.
    has_commit: Option<bool>,
    /// Comma-separated attachment filename patterns; `*` is a wildcard.
    attachment: Option<String>,
    sort: Option<String>,
    /// Sort direction for bounded checkout pages (`ascending` or `descending`).
    direction: Option<String>,
    limit: Option<usize>,
    /// Opt into the bounded checkout page envelope. Capped to protect server and browser.
    page_size: Option<usize>,
    /// Opaque checkout-level cursor returned by a prior paged response.
    cursor: Option<String>,
    /// Eight comma-separated RFC 3339 local-day boundaries for exact seven-day summaries.
    summary_days: Option<String>,
    /// Checkout pages only: `counts=false` skips the navigation counts (returned as `null`),
    /// so whole-checkout walkers avoid a provider summary walk per page (HS2-VPEAM4).
    counts: Option<bool>,
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
        let descending = match self.direction.as_deref() {
            None | Some("ascending") => false,
            Some("descending") => true,
            Some(other) => {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid direction '{other}'"),
                ));
            }
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
        for (name, bound) in [
            ("min_confidence", self.min_confidence),
            ("max_confidence", self.max_confidence),
        ] {
            if bound.is_some_and(|value| value > Confidence::MAX) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("{name} must be an integer from 0 to 100"),
                ));
            }
        }
        Ok(TicketQuery {
            status: opt_parse(self.status.as_deref())?,
            collection: opt_parse(self.collection.as_deref())?,
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
            completed_after: self.completed_after,
            completed_before: self.completed_before,
            verified_after: self.verified_after,
            verified_before: self.verified_before,
            min_confidence: self.min_confidence,
            max_confidence: self.max_confidence,
            has_attachment: self.has_attachment,
            has_media_annotation: self.has_media_annotation,
            attachment_patterns: self
                .attachment
                .map(|values| {
                    values
                        .split(',')
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            sort,
            descending,
            limit: self.limit,
            page_after,
            after_key: None,
        })
    }
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    title: String,
    category: Option<String>,
    priority: Option<String>,
    status: Option<String>,
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
    /// Set, clear with JSON null, or leave unchanged when absent.
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    blocked_reason: Option<Option<String>>,
    /// Optional note to append alongside the field update.
    note: Option<String>,
    /// Existing note ULID to edit; absent appends a new note.
    note_id: Option<String>,
    /// Kind of the appended note; defaults to regular for older clients.
    note_kind: Option<NoteKind>,
    /// Optional concise plain-text headline used by timeline presentations.
    note_summary: Option<String>,
    /// Optional AI completion confidence (integer 0-100) on the appended note
    /// (HS2-DWTJ43), or on the note named by `note_id`, where JSON null clears it
    /// (HS2-CY4CWC). Kept as raw JSON so a malformed value gets an explicit 400.
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    note_confidence: Option<Option<serde_json::Value>>,
    /// Who is acting (HS2-RD4M29): `{"role":"human|ai|system","id":"..."}`. Absent is
    /// unspecified, never assumed AI.
    #[serde(default)]
    actor: Option<ActorReq>,
}

impl UpdateReq {
    /// Validate the optional note confidence as a JSON integer from 0 to 100.
    ///
    /// Appending: a score needs a non-empty note, and null means "no score".
    /// Editing (`note_id`): absent leaves the score unchanged, an integer replaces it,
    /// and null clears it; the note text may be omitted.
    fn note_confidence_change(
        &self,
        editing_note: bool,
    ) -> Result<Option<Option<Confidence>>, ApiError> {
        let Some(value) = self.note_confidence.as_ref() else {
            return Ok(None);
        };
        let Some(value) = value else {
            return Ok(editing_note.then_some(None));
        };
        if !editing_note && self.note.as_deref().is_none_or(str::is_empty) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "note_confidence requires a non-empty note",
            ));
        }
        value
            .as_u64()
            .ok_or_else(|| ConfidenceError(value.to_string()))
            .and_then(Confidence::new)
            .map(|confidence| Some(Some(confidence)))
            .map_err(|error| {
                ApiError::new(StatusCode::BAD_REQUEST, format!("note_confidence: {error}"))
            })
    }
}

fn deserialize_nullable_patch<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
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
    duplicate_of: Option<DuplicateOfReq>,
    /// Who is closing (HS2-RD4M29); an `ai` completed-close needs a scored cycle.
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DuplicateOfReq {
    Legacy(String),
    Qualified(ProjectTicketRef),
}

#[derive(Debug, Deserialize)]
struct ClaimReq {
    worker: Option<String>,
    label: Option<String>,
    lease_minutes: Option<i64>,
    /// Estimated completion time: a duration such as `45m` or an RFC 3339 timestamp (HS2-DQQ0AX).
    eta: Option<String>,
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
    /// A new estimated completion time; omitted keeps the current one (HS2-DQQ0AX).
    eta: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsParams {
    secret: Option<String>,
    /// The checkout this subscription serves and the client (browser tab) holding it, so the
    /// server knows which projects are open (HS2-ARJ9J1).
    checkout: Option<String>,
    client: Option<String>,
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

fn initial_status(value: Option<&str>) -> Result<Status, ApiError> {
    let status = opt_parse(value)?.unwrap_or_default();
    match status {
        Status::NotStarted | Status::Started | Status::Backlog => Ok(status),
        _ => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "status '{}' cannot be used when creating a ticket",
                value.unwrap_or_default()
            ),
        )),
    }
}

// ---- errors ----------------------------------------------------------------------

/// An API error rendered as `{ "error": "…" }` with a status code.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
    /// Stable machine-readable code for errors an automated caller should branch on.
    code: Option<&'static str>,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            code: None,
        }
    }
    fn not_found(id: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, format!("no ticket matching '{id}'"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = serde_json::json!({ "error": self.message });
        if let Some(code) = self.code {
            body["code"] = serde_json::Value::from(code);
        }
        (self.status, Json(body)).into_response()
    }
}

/// A role-specific rule refused the mutation before any write (HS2-RD4M29): 422 with the
/// rule's stable `code` and its actor-tailored message.
impl From<hotsheet_ticketing::actor::RuleViolation> for ApiError {
    fn from(violation: hotsheet_ticketing::actor::RuleViolation) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: violation.message,
            code: Some(violation.code),
        }
    }
}

/// The optional `actor` object every mutating request body may carry (HS2-RD4M29).
#[derive(Debug, Clone, Deserialize)]
struct ActorReq {
    /// Defaulted so a missing role reads as an explicit 400, like an unknown one.
    #[serde(default)]
    role: String,
    #[serde(default)]
    id: Option<String>,
}

fn parse_actor(
    actor: Option<&ActorReq>,
) -> Result<Option<hotsheet_ticketing::actor::MutationActor>, ApiError> {
    actor
        .map(|actor| {
            hotsheet_ticketing::actor::MutationActor::parse(&actor.role, actor.id.clone())
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, format!("actor: {error}")))
        })
        .transpose()
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        let status = match &e {
            error if error.is_io_kind(std::io::ErrorKind::NotFound) => StatusCode::NOT_FOUND,
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
            | OpError::ClaimUnavailable { .. }
            | OpError::NotWorkingRequiresCompleted(_)
            | OpError::NotInTrash(_)) => ApiError::new(StatusCode::CONFLICT, other.to_string()),
            other @ (OpError::DuplicateNeedsTarget
            | OpError::SelfBlock(_)
            | OpError::EmptyNotWorkingReport
            | OpError::InvalidEta(_)) => ApiError::new(StatusCode::BAD_REQUEST, other.to_string()),
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

/// Keeps one checkout monitor alive. Initialization and repository inspection happen on
/// its background thread, so opening a project never waits for a recursive traversal.
struct RepositoryWatchHandle {
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
struct NativeWatcherHold(NativeWatcherSlot);

impl Drop for NativeWatcherHold {
    fn drop(&mut self) {
        let watchers = self.0.lock().ok().map(|mut state| {
            state.stopped = true;
            (state.watcher.take(), state.bridge.take())
        });
        // Stop the stream outside the lock: it waits for the stream's run-loop thread.
        drop(watchers);
    }
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
    host: StoreHost,
    local_write_hashes: LocalWriteHashes,
    events: broadcast::Sender<ChangeEvent>,
    event_log: Arc<Mutex<EventLog>>,
    checkout_registry: hotsheet_ticketing::checkouts::CheckoutRegistry,
}

/// Watch the **default** store (back-compat entry point used by the server binary).
pub fn spawn_watcher(state: AppState) -> anyhow::Result<WatchHandle> {
    spawn_watcher_for(default_watch_target(&state), WatcherBackend::Recommended)
}

fn default_watch_target(state: &AppState) -> WatchTarget {
    WatchTarget {
        entry: state.default_entry(),
        store_id: multistore::store_url_id(&state.store),
        host: state.host.clone(),
        local_write_hashes: state.local_write_hashes.clone(),
        events: state.events.clone(),
        event_log: state.event_log.clone(),
        checkout_registry: state.checkout_registry.clone(),
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
fn registered_watcher_backend() -> WatcherBackend {
    WatcherBackend::Recommended
}

/// Marks the synthetic event that asks a store's watch loop to re-check its tickets once
/// the native stream is live, covering writes made while the stream was starting.
const STARTUP_RESYNC: &str = "hotsheet-startup-resync";
/// When the startup resync runs; a native stream is live well within this (HS2-P3SSGR).
const STARTUP_RESYNC_DELAY: Duration = Duration::from_millis(1500);

/// The startup bridge's polling config. notify's poller compares mtimes in whole seconds, so an
/// edit landing in the same second as the previous scan looks unchanged; comparing contents
/// catches it at the next scan instead of waiting for the native stream, whose start can take
/// several seconds on a busy Mac (HS2-XAHR91). The bridge lives only until that stream is up.
fn startup_bridge_config() -> notify::Config {
    notify::Config::default()
        .with_poll_interval(Duration::from_millis(250))
        .with_compare_contents(true)
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
                let request_resync = |dir: &FsPath| {
                    let _ = resync.send(Ok(notify::Event::new(notify::EventKind::Other)
                        .add_path(dir.to_path_buf())
                        .set_info(STARTUP_RESYNC)));
                };
                let bridge_tx = tx.clone();
                let bridge = notify::PollWatcher::new(
                    move |res| {
                        let _ = bridge_tx.send(res);
                    },
                    startup_bridge_config(),
                )
                .and_then(|mut bridge| {
                    bridge.watch(&tickets_dir, RecursiveMode::Recursive)?;
                    Ok(bridge)
                });
                {
                    let Ok(mut slot) = started.lock() else {
                        return;
                    };
                    if slot.stopped {
                        return;
                    }
                    slot.bridge = bridge.ok();
                }
                request_resync(&tickets_dir);
                let mut watcher = match notify::recommended_watcher(move |res| {
                    let _ = tx.send(res);
                }) {
                    Ok(watcher) => watcher,
                    Err(error) => {
                        eprintln!(
                            "watcher for {} failed to start: {error}",
                            tickets_dir.display()
                        );
                        return;
                    }
                };
                if let Err(error) = watcher.watch(&tickets_dir, RecursiveMode::Recursive) {
                    eprintln!(
                        "watcher for {} failed to start: {error}",
                        tickets_dir.display()
                    );
                    return;
                }
                {
                    let Ok(mut slot) = started.lock() else {
                        return;
                    };
                    // The handle was dropped while the stream started: stop it again.
                    if slot.stopped {
                        return;
                    }
                    slot.watcher = Some(watcher);
                }
                std::thread::sleep(STARTUP_RESYNC_DELAY);
                // The native stream is live: retire the bridge and re-check once more.
                let bridge = started.lock().ok().and_then(|mut slot| slot.bridge.take());
                drop(bridge);
                request_resync(&tickets_dir);
            });
            HeldWatcher::Native {
                _hold: NativeWatcherHold(slot),
            }
        }
        WatcherBackend::Poll => {
            let mut watcher = notify::PollWatcher::new(
                move |res| {
                    let _ = tx.send(res);
                },
                notify::Config::default().with_poll_interval(Duration::from_millis(250)),
            )?;
            watcher.watch(&tickets_dir, RecursiveMode::Recursive)?;
            HeldWatcher::Poll { _watcher: watcher }
        }
    };
    Ok(WatchHandle { _watcher: watcher })
}

fn repository_change_event(checkout_id: &str) -> ChangeEvent {
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
fn spawn_repository_watcher(
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
        let Ok(output) = std::process::Command::new("git")
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
    let Ok(output) = std::process::Command::new("git")
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
                    eprintln!(
                        "repository watcher for {} could not watch {}: {error}",
                        watch_root.display(),
                        git_dir.display()
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
                eprintln!(
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
    let output = std::process::Command::new("git")
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

fn watch_loop(rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>, target: WatchTarget) {
    use std::time::Duration;

    while let Ok(first) = rx.recv() {
        let mut paths = watch_event_paths(&target, first);
        // Debounce a burst (editor save, git checkout touching many files).
        while let Ok(next) = rx.recv_timeout(Duration::from_millis(150)) {
            paths.extend(watch_event_paths(&target, next));
        }
        paths.sort();
        paths.dedup();
        if paths.is_empty() {
            continue;
        }
        let mut index_changed = false;
        for path in &paths {
            index_changed |= handle_path_change(&target, path);
        }
        if !index_changed {
            continue;
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
                    if let Err(e) = regenerate_checkout_worklist_indexed(&target.host, &checkout) {
                        eprintln!("worklist regenerate failed for {}: {e}", checkout.root);
                    }
                }
            }
        }
    }
}

/// The ticket files one watcher event concerns; a startup resync yields the store's ticket
/// files whose bytes differ from the index.
fn watch_event_paths(
    target: &WatchTarget,
    res: notify::Result<notify::Event>,
) -> Vec<std::path::PathBuf> {
    if let Ok(event) = &res
        && event.info() == Some(STARTUP_RESYNC)
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
    let Ok(index) = target.entry.index.lock() else {
        return files;
    };
    files
        .into_iter()
        .filter(|path| {
            let Some(id) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| Ulid::from_string(stem).ok())
            else {
                return false;
            };
            let Ok(bytes) = std::fs::read(path) else {
                return false;
            };
            index.content_hash(&id).ok().flatten().as_deref() != Some(hash_bytes(&bytes).as_str())
        })
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

fn handle_path_change(target: &WatchTarget, path: &FsPath) -> bool {
    // The filename stem is the ticket ULID.
    let Some(id) = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| Ulid::from_string(s).ok())
    else {
        return false;
    };
    let index = &target.entry.index;
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
        let local_echo = target.local_write_hashes.lock().is_ok_and(|mut writes| {
            writes.retain(|_, at| at.elapsed() <= Duration::from_secs(5));
            writes
                .remove(&(target.store_id.clone(), id.to_string(), String::new()))
                .is_some()
        });
        if let Ok(index) = index.lock() {
            let _ = index.delete(&id);
        }
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
    let local_echo = target.local_write_hashes.lock().is_ok_and(|mut writes| {
        writes.retain(|_, at| at.elapsed() <= Duration::from_secs(5));
        writes.contains_key(&(target.store_id.clone(), id.to_string(), hash.clone()))
    });
    if local_echo {
        return false;
    }

    // An unmarked event is external. Preserve its worklist invalidation even when its
    // bytes happen to match the current index row.
    let already = index
        .lock()
        .ok()
        .and_then(|index| index.content_hash(&id).ok().flatten());
    if already.as_deref() == Some(hash.as_str()) {
        return true;
    }

    let Ok(ticket) = parse_file(&String::from_utf8_lossy(&bytes)) else {
        // Preserve the last healthy row, but remember the corrupt bytes. Otherwise a
        // manual repair that restores the previous healthy content compares equal to
        // the stale hash and is incorrectly treated as a no-op.
        if let Ok(index) = index.lock() {
            let _ = index.record_source_hash(&id, &hash);
        }
        let (_, slug) = hotsheet_ticketing::recover_ticket_identity(path);
        // Keep the last healthy indexed row available, but wake every client so its
        // resilient refresh can replace that stale projection with recovery UI.
        emit("changed", id.to_string(), slug.unwrap_or_default());
        return true;
    };
    if let Ok(index) = index.lock() {
        let _ = index.upsert(&ticket, &path.display().to_string(), &hash);
    }
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
        let status = std::process::Command::new("git")
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

#[cfg(test)]
mod activity_distillation_tests {
    use super::AppState;
    use hotsheet_model::{NoteKind, Timestamp, Ulid};
    use hotsheet_ticketing::{
        ActivityEvent, ActivityKind, FsStore, NewTicket, Scope, Settings, StoreMetadata, ops,
    };
    use serde_json::json;

    #[test]
    fn enabled_deterministic_adapter_distills_live_activity_once() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let ticket_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        ops::create(
            &store,
            ticket_id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        Settings::new(store.root())
            .set(
                "activity_distillation",
                json!({"enabled":true,"adapter":"deterministic"}),
                Scope::Local,
            )
            .unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let mut event = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2026-09-02T01:00:00Z",
            "codex",
            ActivityKind::Decision,
            json!({"text":"private decision text"}),
        );
        event.ticket = Some(ticket_id.to_string());
        event.session = Some("session-1".into());

        state.record_activity(&store, event.clone()).unwrap();
        state.record_activity(&store, event).unwrap();

        let ticket = store.read_ticket(&ticket_id).unwrap();
        assert_eq!(ticket.notes.len(), 1);
        assert_eq!(ticket.notes[0].kind, NoteKind::Activity);
        assert!(ticket.notes[0].text.starts_with("Recorded a decision"));
        assert!(!ticket.notes[0].text.contains("private decision text"));
    }

    #[test]
    fn project_activity_distillation_uses_explicit_checkout_settings_for_a_shared_store() {
        let root = tempfile::tempdir().unwrap();
        let store_root = root.path().join("shared.hs2");
        let store = FsStore::init(&store_root, &StoreMetadata::new("HS")).unwrap();
        let ticket_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        ops::create(
            &store,
            ticket_id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        let first_root = root.path().join("first");
        let second_root = root.path().join("second");
        std::fs::create_dir(&first_root).unwrap();
        std::fs::create_dir(&second_root).unwrap();
        let registry_path = root.path().join("home/checkouts.json");
        let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry_path);
        let first = registry
            .register(&first_root, Some("first"), None, vec![store_root.clone()])
            .unwrap();
        let second = registry
            .register(&second_root, Some("second"), None, vec![store_root])
            .unwrap();
        first
            .settings()
            .set(
                "activity_distillation",
                json!({"enabled":true,"adapter":"deterministic"}),
                Scope::Local,
            )
            .unwrap();
        second
            .settings()
            .set(
                "activity_distillation",
                json!({"enabled":false,"adapter":"deterministic"}),
                Scope::Local,
            )
            .unwrap();
        let state = AppState::new(store.clone(), "secret".into())
            .unwrap()
            .with_checkout_registry(registry_path);

        let mut first_event = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2026-09-02T01:00:00Z",
            "codex",
            ActivityKind::Decision,
            json!({"text":"first private decision"}),
        );
        first_event.ticket = Some(ticket_id.to_string());
        first_event.session = Some("first-session".into());
        first_event.project = Some(first.id);
        state.record_activity(&store, first_event).unwrap();

        let mut second_event = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAW",
            "2026-09-02T01:01:00Z",
            "codex",
            ActivityKind::Decision,
            json!({"text":"second private decision"}),
        );
        second_event.ticket = Some(ticket_id.to_string());
        second_event.session = Some("second-session".into());
        second_event.project = Some(second.id);
        state.record_activity(&store, second_event).unwrap();

        let ticket = store.read_ticket(&ticket_id).unwrap();
        assert_eq!(ticket.notes.len(), 1);
        assert!(ticket.notes[0].text.starts_with("Recorded a decision"));
    }

    #[test]
    fn distillation_is_disabled_by_default() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let ticket_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        ops::create(
            &store,
            ticket_id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let mut event = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2026-09-02T01:00:00Z",
            "codex",
            ActivityKind::Decision,
            json!({}),
        );
        event.ticket = Some(ticket_id.to_string());
        event.session = Some("session-1".into());
        state.record_activity(&store, event).unwrap();
        assert!(store.read_ticket(&ticket_id).unwrap().notes.is_empty());
    }

    #[test]
    fn noisy_activity_is_bounded_before_persistence_and_broadcast() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let mut live = state.subscribe();

        for index in 0..140 {
            let mut event = ActivityEvent::new(
                format!("01ARZ3NDEKTSV4RRFFQ{index:06}"),
                "2026-09-02T01:00:00Z",
                "codex",
                ActivityKind::Command,
                json!({"command": "chatty"}),
            );
            event.session = Some("noisy-session".into());
            state.record_activity(&store, event).unwrap();
        }
        let mut done = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ999999",
            "2026-09-02T01:01:00Z",
            "codex",
            ActivityKind::TurnEnd,
            json!({}),
        );
        done.session = Some("noisy-session".into());
        state.record_activity(&store, done).unwrap();

        let recorded = hotsheet_ticketing::activity::read_recent(&store).unwrap();
        assert_eq!(recorded.len(), 130);
        assert_eq!(recorded[128].kind, ActivityKind::Summary);
        assert_eq!(recorded[128].detail["coalesced"], 12);
        assert_eq!(recorded[129].kind, ActivityKind::TurnEnd);

        let broadcast = std::iter::from_fn(|| live.try_recv().ok()).collect::<Vec<_>>();
        assert_eq!(broadcast.len(), recorded.len());
        assert_eq!(broadcast[128].activity.as_ref(), Some(&recorded[128]));
    }
}

#[cfg(test)]
mod terminal_permission_route_tests {
    use super::{
        AppState, OpenTerminalReq, terminal_permission_project, terminal_permission_route_env,
    };
    use hotsheet_ticketing::{FsStore, StoreMetadata, checkouts::CheckoutRegistry};

    fn request(cwd: Option<String>) -> OpenTerminalReq {
        OpenTerminalReq {
            command: None,
            shell_command: None,
            args: Vec::new(),
            cwd,
            id: None,
            connect: None,
            model: None,
            effort: None,
        }
    }

    #[test]
    fn every_terminal_route_carries_the_secret_project_and_server_once_known() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let value = |env: &[(String, String)], key: &str| {
            env.iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        };

        // Before the listener URL is known the tool can't reach the server, so no route.
        let env = terminal_permission_route_env(&state, &request(None));
        assert_eq!(value(&env, "HOTSHEET_SECRET").as_deref(), Some("secret"));
        assert_eq!(
            value(&env, "HOTSHEET_PROJECT"),
            Some(store.root().display().to_string())
        );
        assert_eq!(value(&env, "HOTSHEET_SERVER"), None);

        state.set_terminal_server_url("http://127.0.0.1:4175".into());
        let env = terminal_permission_route_env(&state, &request(None));
        assert_eq!(
            value(&env, "HOTSHEET_SERVER").as_deref(),
            Some("http://127.0.0.1:4175")
        );
    }

    #[test]
    fn terminal_uses_its_checkout_store_instead_of_the_primary_server_store() {
        let root = tempfile::tempdir().unwrap();
        let primary =
            FsStore::init(root.path().join("primary.hs2"), &StoreMetadata::new("HS")).unwrap();
        let project = root.path().join("domotion");
        std::fs::create_dir_all(project.join("nested")).unwrap();
        let project_store =
            FsStore::init(root.path().join("domotion.hs2"), &StoreMetadata::new("DM")).unwrap();
        let registry_path = root.path().join("checkouts.json");
        CheckoutRegistry::new(&registry_path)
            .register(
                &project,
                None,
                None,
                vec![project_store.root().to_path_buf()],
            )
            .unwrap();
        let state = AppState::new(primary, "secret".into())
            .unwrap()
            .with_checkout_registry(registry_path);
        let req = request(Some(project.join("nested").display().to_string()));
        let expected = project_store
            .root()
            .canonicalize()
            .unwrap()
            .display()
            .to_string();
        assert_eq!(terminal_permission_project(&state, &req), expected);
        assert!(
            terminal_permission_route_env(&state, &req)
                .contains(&("HOTSHEET_PROJECT".into(), expected))
        );
    }
}

#[cfg(test)]
mod startup_bridge_tests {
    use super::startup_bridge_config;
    use notify::{RecursiveMode, Watcher};
    use std::time::{Duration, SystemTime};

    /// The bridge must report an edit whose mtime lands in the same second as the previous
    /// scan (HS2-XAHR91); an mtime-only poller reports nothing until the next second's write.
    #[test]
    fn the_startup_bridge_sees_an_edit_within_the_same_mtime_second() {
        let dir = tempfile::tempdir().unwrap();
        let ticket = dir.path().join("ticket.md");
        let stamp = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let write = |text: &str| {
            std::fs::write(&ticket, text).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&ticket)
                .unwrap()
                .set_modified(stamp)
                .unwrap();
        };
        write("first");
        let (tx, rx) = std::sync::mpsc::channel();
        let mut bridge = notify::PollWatcher::new(
            move |res: notify::Result<notify::Event>| {
                let _ = tx.send(res);
            },
            startup_bridge_config(),
        )
        .unwrap();
        bridge.watch(dir.path(), RecursiveMode::Recursive).unwrap();
        // Let the initial scan record the file, then drain anything it reported.
        std::thread::sleep(Duration::from_millis(400));
        while rx.try_recv().is_ok() {}

        write("second, same mtime second");
        let seen = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("the bridge should report a same-second edit")
            .unwrap();
        assert!(
            seen.paths.iter().any(|path| path.ends_with("ticket.md")),
            "{seen:?}"
        );
    }
}

#[cfg(test)]
mod ai_terminal_tool_tests {
    use super::ai_terminal_tool;
    use hotsheet_terminals::TerminalKind;

    /// The tool comes only from an `ai` terminal's `<tool>-<id>` worker id (HS2-HZK0NK).
    #[test]
    fn recovers_the_tool_from_an_ai_terminal_worker_id() {
        assert_eq!(
            ai_terminal_tool(TerminalKind::Ai, Some("claude-01ABC"), "01ABC").as_deref(),
            Some("claude")
        );
        // Tool names may themselves contain dashes.
        assert_eq!(
            ai_terminal_tool(TerminalKind::Ai, Some("kind-agent-ai"), "ai").as_deref(),
            Some("kind-agent")
        );
        assert_eq!(
            ai_terminal_tool(TerminalKind::Shell, Some("claude-01ABC"), "01ABC"),
            None
        );
        assert_eq!(ai_terminal_tool(TerminalKind::Ai, None, "01ABC"), None);
        assert_eq!(
            ai_terminal_tool(TerminalKind::Ai, Some("claude-other"), "01ABC"),
            None
        );
        assert_eq!(
            ai_terminal_tool(TerminalKind::Ai, Some("-01ABC"), "01ABC"),
            None
        );
    }
}
