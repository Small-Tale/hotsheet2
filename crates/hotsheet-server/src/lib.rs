//! Hot Sheet 2 server: a thin HTTP + WebSocket layer over the shared engine
//! (`hotsheet-ticketing::ops`), the single authority every GUI talks to
//! (`docs/04-core-server-cli.md` §4.3). v1 is loopback + shared-secret (Tier 0).
//! Reads go through the SQLite/FTS index (HS2-5); a filesystem watcher (HS2-6) keeps
//! it fresh and broadcasts change events, so a CLI/git edit shows up live. Terminals
//! (HS2-10) and the detached lifecycle (HS2-59) are separate.

mod ai_tool_discovery;
mod api_error;
pub mod client_drive;
pub mod code_review;
pub mod commands;
mod credential_cache;
mod custom_views;
pub mod dist_work_loop;
mod dto;
mod events;
pub mod github_app_config;
mod health_scan;
mod image_crop_cache;
pub mod lifecycle;
pub mod media;
pub mod multistore;
pub mod notifications;
mod presence;
mod provider_overlay;
pub mod provider_write_behind;
pub mod repository_browser;
mod routes;
pub mod source_revision;
pub mod sync_loop;
pub mod terminal_broker;
mod terminal_names;
pub mod tls;
pub mod tts;
pub mod turn_stream;
mod watcher;

use hotsheet_sync::LockExt;
use std::collections::HashSet;
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use credential_cache::{CachedSecretStore, ManagedSecretCache};
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
use hotsheet_ticketing::provider_outbox::{OutboxAdmission, OutboxError, ProviderOutbox};
use hotsheet_ticketing::wire::ApiAttachment;
use hotsheet_ticketing::{
    FsStore, GitProvider, KeyRegistry, MutationContext, NewTicket, NotWorkingReport, OpError,
    OsKeychain, ProjectTicketRef, ProviderConfigRegistry, ProviderConnection, ProviderDraft,
    ProviderEvidence, ProviderMutationTiming, ProviderPatch, ProviderRegistry, STORE_METADATA_FILE,
    Settings, SortKey, StoreError, StoreRegistry, TicketPatch, TicketQuery, TicketRef,
    auto_context, copy_between, move_between, ops,
};
// Wire DTOs are defined once in the engine crate (wire SSOT); re-export for callers.
pub use hotsheet_ticketing::{ApiNote, ApiTicket};

pub use api_error::*;
use dto::*;
pub use events::*;
use routes::accounts::*;
use routes::activity::*;
use routes::auth::*;
use routes::checkout_tickets::*;
use routes::checkouts::*;
use routes::claims::*;
use routes::permissions::*;
use routes::project_tools::*;
use routes::providers::*;
use routes::store_scoped::*;
pub use routes::terminals::*;
use routes::ticket_writes::*;
use routes::tickets::*;
use routes::transfer::*;
pub use watcher::*;

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
type LiveProviderCache = Arc<
    Mutex<std::collections::HashMap<String, (String, Arc<dyn hotsheet_ticketing::TicketProvider>)>>,
>;

#[derive(Clone)]
pub struct AppState {
    store: FsStore,
    /// Identity of the primary store at startup. An unscoped route must never silently
    /// switch to a different store placed at the same path (HS2-34XE8B).
    primary_instance_id: Result<String, String>,
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
    /// Reuse live provider adapters so short-lived read caches span checkout requests.
    live_providers: LiveProviderCache,
    /// Experimental durable Jira field-edit admission. None keeps every existing write synchronous.
    jira_outbox: Option<Arc<Mutex<ProviderOutbox>>>,
    /// One Keychain read per managed sign-in per metadata revision, shared by requests.
    managed_secrets: ManagedSecretCache,
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
    /// Tickets whose server-side index write failed (HS2-JD7TK0), keyed by store URL id.
    /// Each is re-indexed from its file on the next write to that store, so a transient
    /// SQLite failure cannot leave list/search serving a stale row until restart.
    pending_index_repairs: Arc<Mutex<HashSet<(String, Ulid)>>>,
    /// Test-only fault injection: the next N server index writes fail (HS2-JD7TK0).
    #[cfg(test)]
    index_write_faults: Arc<AtomicUsize>,
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
    /// AI sessions in terminals that halted on an API error, by terminal id (HS2-HJ4D1H).
    terminal_halts: Arc<Mutex<std::collections::HashMap<String, TerminalHalt>>>,
    /// Claude questions currently waiting in a terminal (HS2-KP9K85).
    terminal_questions: Arc<Mutex<std::collections::HashMap<String, TerminalQuestion>>>,
    /// AI sessions whose Hot Sheet hooks reported in from a terminal, by terminal id (HS2-EV1XK3).
    terminal_ai_connections: Arc<Mutex<std::collections::HashMap<String, TerminalAiConnection>>>,
    /// Last trusted hook report for each live terminal, retained after SessionEnd for diagnosis.
    terminal_ai_last_reports: Arc<Mutex<std::collections::HashMap<String, TerminalAiConnection>>>,
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
        let primary_instance_id = store
            .metadata()
            .and_then(|metadata| match metadata.instance_id {
                Some(id) => Ok(id),
                None => store.ensure_instance_id(),
            })
            .map_err(|error| error.to_string());
        let machine_home = hotsheet_plugins::hotsheet_home();
        let (events, _) = broadcast::channel(256);
        let index = Arc::new(Mutex::new(index));
        let corrupt = Arc::default();
        let host = StoreHost::new();
        // The primary store is the default hosted entry (shares the same index Arc, so
        // the unprefixed routes and /stores/{default}/… see one index).
        let primary = StoreEntry {
            instance_id: primary_instance_id.as_ref().ok().cloned(),
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
            primary_instance_id,
            secret,
            events,
            index,
            corrupt,
            event_log,
            host,
            injected_providers: ProviderRegistry::default(),
            live_providers: Arc::default(),
            jira_outbox: None,
            managed_secrets: ManagedSecretCache::default(),
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
            pending_index_repairs: Arc::default(),
            #[cfg(test)]
            index_write_faults: Arc::default(),
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
            terminal_halts: Arc::new(Mutex::new(std::collections::HashMap::new())),
            terminal_questions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            terminal_ai_connections: Arc::new(Mutex::new(std::collections::HashMap::new())),
            terminal_ai_last_reports: Arc::new(Mutex::new(std::collections::HashMap::new())),
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
            .with_lock(|g| g.drain().map(|(_, guard)| guard).collect());
        let locks: Vec<_> = self
            .writer_locks
            .with_lock(|w| w.drain().map(|(_, lock)| lock).collect());
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
        let setup_refreshes = self.setup_refreshes.with_lock(|set| set.len());
        if setup_refreshes > 0 {
            blockers.push(QuiescenceBlocker {
                kind: "setup_refreshes",
                count: setup_refreshes,
            });
        }
        let driven_turns = self.drive_registry.with_lock(|reg| reg.count());
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
            .with_lock(|sessions| sessions.len());
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
        self.managed_secrets = ManagedSecretCache::default();
        self
    }

    /// Opt in to durable Jira field-edit admission. The database belongs in the machine
    /// home, outside both the Git ticket store and its disposable search index.
    pub fn with_jira_outbox(
        mut self,
        path: impl AsRef<FsPath>,
        max_pending: usize,
    ) -> Result<Self, OutboxError> {
        self.jira_outbox = Some(Arc::new(Mutex::new(ProviderOutbox::open(
            path,
            max_pending,
        )?)));
        Ok(self)
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

    fn key_registry(&self) -> KeyRegistry<CachedSecretStore<OsKeychain>> {
        KeyRegistry::new(
            self.machine_home.as_ref().clone(),
            self.managed_secrets
                .store(self.machine_home.as_ref().clone(), OsKeychain),
        )
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
        let mut paths = self.permission_rule_paths.lock_or_recover();
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
                Err(error) => tracing::warn!(
                    "Trash retention ignored for checkout {}: {error}",
                    checkout.id
                ),
            }
        }
        configured.unwrap_or(hotsheet_ticketing::DEFAULT_TRASH_CLEANUP_DAYS)
    }

    /// Register the background sync loop's kick channel (called by [`sync_loop::spawn_sync_loop`]).
    pub fn set_sync_kicker(&self, tx: std::sync::mpsc::Sender<()>) {
        {
            let mut k = self.sync_kick.lock_or_recover();
            *k = Some(tx);
        }
    }

    /// Nudge the sync loop to run now (best-effort; a no-op if the loop isn't running).
    fn kick_sync(&self) {
        {
            let k = self.sync_kick.lock_or_recover();
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
            // Best-effort warm-up: a failure leaves the cache cold for on-demand discovery.
            let _ = discovered_ai_tools_off_runtime(&state, false).await;
        });
    }

    /// Set the URL injected into interactively launched tools. The real server calls this
    /// after binding; tests may use it without publishing machine discovery files.
    pub fn set_terminal_server_url(&self, url: String) {
        {
            let mut slot = self.terminal_server_url.lock_or_recover();
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
        let _initializing = initialization.lock_or_recover();
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
            instance_id: store
                .metadata()
                .ok()
                .and_then(|metadata| metadata.instance_id),
            store: store.clone(),
            index: Arc::new(Mutex::new(index)),
            corrupt: Arc::default(),
        };
        self.host.register(entry.clone());
        prewarm_corrupt_tickets(entry.clone());
        let project = store.root().display().to_string();
        let mut paths = self.permission_rule_paths.lock_or_recover();
        if !paths.is_empty()
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
        drop(paths);
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
                let mut w = self.watchers.lock_or_recover();
                w.insert(watched_id, handle);
            }
            Err(e) => tracing::warn!("watcher for {} failed to start: {e}", store_root.display()),
        }
        // Advertise the newly-hosted store for discovery (real run only; a no-op in tests).
        self.register_store_instance(&store_root);
        Ok(true)
    }

    fn watch_checkout_repository(&self, checkout: &hotsheet_ticketing::checkouts::Checkout) {
        let mut watchers = self.repository_watchers.lock_or_recover();
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
            Err(error) => tracing::warn!(
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
        if !pinned {
            self.project_hosted.lock_or_recover().insert(id);
        }
        self.request_unhost_sweep(presence::POLL_RECONNECT_GAP + presence::UNHOST_GRACE);
        Ok(added)
    }

    fn is_project_hosted(&self, id: &str) -> bool {
        self.project_hosted.with_lock(|hosted| hosted.contains(id))
    }

    /// Stop hosting a project store: its index, watcher, discovery file and writer lock go,
    /// and the next open or checkout request hosts it again (HS2-ARJ9J1).
    fn unhost_store(&self, id: &str) {
        if !self.project_hosted.lock_or_recover().remove(id) {
            return;
        }
        let Some(entry) = self.host.unregister(id) else {
            return;
        };
        let root = entry.store.root().display().to_string();
        let watcher = self.watchers.with_lock(|w| w.remove(id));
        let guard = self.instance_guards.with_lock(|g| g.remove(&root));
        let lock = self.writer_locks.with_lock(|w| w.remove(&root));
        // Stop the watcher and release the files outside every lock.
        drop((watcher, guard, lock));
        tracing::info!("unhosted store {root}: no open project references it");
    }

    /// Ask for an unhost sweep `delay` from now; an earlier pending request wins.
    fn request_unhost_sweep(&self, delay: Duration) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let at = tokio::time::Instant::now() + delay;
        {
            let mut due = self.unhost_sweep.due.lock_or_recover();
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
        {
            let mut m = self.instance.lock_or_recover();
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
        let Some(meta) = self.instance.with_lock(|m| m.clone()) else {
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
                    let mut w = self.writer_locks.lock_or_recover();
                    w.insert(store_path.display().to_string(), lock);
                }
                Err(lifecycle::LockError::Held(pid)) => tracing::warn!(
                    "warning: store {} is also index-write-locked by live server pid {pid} \
                     — index writes may collide",
                    store_path.display()
                ),
                Err(e) => tracing::warn!("writer lock for {} failed: {e}", store_path.display()),
            }
        }
        match instances.register_instance(&info, store_path) {
            Ok(guard) => {
                let mut g = self.instance_guards.lock_or_recover();
                g.insert(store_path.display().to_string(), guard);
            }
            Err(e) => tracing::warn!(
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
                    Err(e) => tracing::warn!("could not host {}: {}", path.display(), e.message),
                },
                Err(e) => tracing::warn!("configured store {} skipped: {e}", path.display()),
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

    /// The default (primary) served store as a host entry — what the unprefixed routes
    /// operate on.
    fn default_entry(&self) -> StoreEntry {
        StoreEntry {
            instance_id: self.primary_instance_id.as_ref().ok().cloned(),
            store: self.store.clone(),
            index: self.index.clone(),
            corrupt: self.corrupt.clone(),
        }
    }

    fn require_primary_store_identity(&self) -> Result<(), ApiError> {
        let expected = self.primary_instance_id.as_ref().map_err(|error| {
            ApiError::new(
                StatusCode::CONFLICT,
                format!("primary ticket store identity was unavailable at startup: {error}"),
            )
        })?;
        let current = self.store.metadata().map_err(|error| {
            ApiError::new(
                StatusCode::CONFLICT,
                format!("primary ticket store cannot be verified: {error}"),
            )
        })?;
        if current.instance_id.as_ref() != Some(expected) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "primary ticket store has a different identity at its startup path; restore the original store or restart the server to select the replacement",
            ));
        }
        Ok(())
    }

    /// Persist one activity event and publish that exact event on the shared live bus.
    /// Unlike announcements this is also placed in the long-poll ring; the rolling
    /// activity store remains the authoritative reconnect/digest source.
    pub fn record_activity(
        &self,
        store: &FsStore,
        event: hotsheet_ticketing::ActivityEvent,
    ) -> std::io::Result<()> {
        let admitted = self.activity_volume.lock_or_recover().observe(event);
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
                        tracing::warn!(
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
                    tracing::warn!(
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
                tracing::warn!("activity distillation policy ignored: {error}");
                return;
            }
        };
        // Named client adapters (including Apple Foundation Models) consume the same
        // normalized stream on-device. They are never loaded as server requirements.
        if !policy.enabled || policy.adapter != "deterministic" {
            {
                let mut pipelines = self.activity_distillation.lock_or_recover();
                pipelines.remove(&pipeline_id);
            }
            return;
        }
        let request = self.activity_distillation.with_lock(|pipelines| {
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
            tracing::warn!("activity distillation note failed: {error}");
        }
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
        .route(
            "/tickets/{id}/attachments/{attachment_id}",
            axum::routing::put(update_ticket_attachment_annotations),
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
            "/checkouts/{reference}/sources/{connection_id}/color",
            patch(set_checkout_source_color),
        )
        .route(
            "/checkouts/{reference}/sources/{connection_id}/relink",
            patch(relink_checkout_git_source),
        )
        .route(
            "/checkouts/{reference}/default-source",
            put(set_checkout_default_source),
        )
        // Project-owned ticket sources (HS2-SM9PM8): a project lists, creates, and edits
        // only the connections its checkout links.
        .route(
            "/checkouts/{reference}/provider-connections",
            get(list_checkout_provider_connections).post(create_checkout_provider_connection),
        )
        .route(
            "/checkouts/{reference}/provider-connections/{connection_id}",
            patch(update_checkout_provider_connection),
        )
        .route(
            "/checkouts/{reference}/provider-connections/{connection_id}/disabled",
            put(set_checkout_provider_connection_disabled),
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
            "/checkouts/{reference}/tickets/{id}/attachments/{attachment_id}/markup",
            axum::routing::put(update_checkout_ticket_attachment_markup),
        )
        .route(
            "/checkouts/{reference}/tickets/{id}/attachments/{attachment_id}/original",
            get(get_checkout_ticket_attachment_original),
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
            "/providers/{connection_id}/tickets/queued",
            post(provider_write_behind::queue_jira_updates),
        )
        .route(
            "/providers/{connection_id}/outbox",
            get(provider_write_behind::list_jira_operations),
        )
        .route(
            "/providers/{connection_id}/outbox/{operation_id}",
            post(provider_write_behind::retry_jira_operation)
                .delete(provider_write_behind::discard_jira_operation),
        )
        .route(
            "/providers/{connection_id}/ai-feedback",
            get(list_provider_ai_feedback),
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
            "/providers/{connection_id}/tickets/{id}/not-working-json",
            post(report_provider_ticket_not_working_json)
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
        // Machine-wide provider sign-ins and the projects using each (HS2-SM9PM8).
        .route("/accounts", get(list_accounts_route))
        .route("/accounts/{account}", delete(sign_out_account))
        .route(
            "/accounts/{account}/identity",
            post(identify_github_account),
        )
        .route(
            "/accounts/{account}/sources/{connection_id}",
            delete(remove_unused_account_source),
        )
        .route(
            "/accounts/{account}/github-repositories",
            get(list_account_github_repositories),
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
        .route("/permissions/bridge-probe", get(probe_permission_bridge))
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
        .route("/terminals/{id}/name", put(rename_terminal))
        .route(
            "/terminals/{id}/halt",
            post(halt_terminal).delete(clear_terminal_halt),
        )
        .route(
            "/terminals/{id}/question",
            post(ask_terminal_question)
                .get(poll_terminal_question_answer)
                .delete(resolve_terminal_question),
        )
        .route(
            "/terminals/{id}/question/answer",
            post(answer_terminal_question),
        )
        .route(
            "/terminals/{id}/ai-connection",
            post(connect_terminal_ai).delete(disconnect_terminal_ai),
        )
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
        let machine_home = root.path().join("isolated-machine-home");
        let state = AppState::new(store.clone(), "secret".into())
            .unwrap()
            .with_machine_home(&machine_home);
        let value = |env: &[(String, String)], key: &str| {
            env.iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        };

        // Before the listener URL is known the tool can't reach the server, so no route.
        let env = terminal_permission_route_env(&state, &request(None), "term-1");
        assert_eq!(value(&env, "HOTSHEET_SECRET").as_deref(), Some("secret"));
        assert_eq!(
            value(&env, "HOTSHEET_TERMINAL_ID").as_deref(),
            Some("term-1")
        );
        assert_eq!(
            value(&env, "HOTSHEET_PROJECT"),
            Some(store.root().display().to_string())
        );
        assert_eq!(
            value(&env, "HOTSHEET_HOME"),
            Some(machine_home.display().to_string())
        );
        assert_eq!(value(&env, "HOTSHEET_SERVER"), None);

        state.set_terminal_server_url("http://127.0.0.1:4175".into());
        let env = terminal_permission_route_env(&state, &request(None), "term-1");
        assert_eq!(
            value(&env, "HOTSHEET_HOME"),
            Some(machine_home.display().to_string())
        );
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
            terminal_permission_route_env(&state, &req, "term-1")
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
