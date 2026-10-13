//! Hot Sheet 2 server: a thin HTTP + WebSocket layer over the shared engine
//! (`hotsheet-ticketing::ops`), the single authority every GUI talks to
//! (`docs/04-core-server-cli.md` §4.3). v1 is loopback + shared-secret (Tier 0).
//! Reads go through the SQLite/FTS index (HS2-5); a filesystem watcher (HS2-6) keeps
//! it fresh and broadcasts change events, so a CLI/git edit shows up live. Terminals
//! (HS2-10) and the detached lifecycle (HS2-59) are separate.

mod activity_capture;
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
mod hosting;
mod image_crop_cache;
pub mod lifecycle;
pub mod mcp_presence;
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
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use credential_cache::{CachedSecretStore, ManagedSecretCache};
use multistore::{StoreEntry, StoreHost};

use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use hotsheet_index::Index;
use hotsheet_model::{Status, Timestamp};
use hotsheet_ticketing::provider_outbox::{OutboxAdmission, OutboxError, ProviderOutbox};
use hotsheet_ticketing::{
    FsStore, KeyRegistry, OsKeychain, ProviderPatch, ProviderRegistry, Settings,
};
// Wire DTOs are defined once in the engine crate (wire SSOT); re-export for callers.
pub use hotsheet_ticketing::{ApiNote, ApiTicket};

pub use api_error::ApiError;
pub use events::{AssignmentEvent, ChangeEvent};
pub(crate) use events::{EventLog, emit_change};
use routes::accounts::{
    GitHubAuthSession, cancel_github_device_auth, identify_github_account,
    list_account_github_repositories, list_accounts_route, list_github_auth_repositories,
    list_stores, remove_unused_account_source, sign_out_account, start_github_device_auth,
    wait_github_device_auth,
};
use routes::activity::{ingest_activity, list_activity, post_announce};
use routes::auth::require_secret;
use routes::checkout_tickets::{
    act_on_checkout_ticket_attachment, act_on_checkout_ticket_attachment_by_name,
    add_checkout_ticket_attachment, assign_checkout_ticket, batch_update_checkout_tickets,
    close_checkout_ticket, create_checkout_ticket, delete_checkout_ticket_attachment,
    delete_checkout_ticket_note, empty_checkout_trash, get_checkout_ticket,
    get_checkout_ticket_attachment, get_checkout_ticket_attachment_by_name,
    get_checkout_ticket_attachment_original, get_checkout_ticket_attachment_thumbnail,
    get_checkout_ticket_duplicate_backlinks, preview_checkout_trash_purge,
    put_checkout_ticket_attachment_thumbnail, rename_checkout_ticket_attachment,
    restore_checkout_ticket, update_checkout_ticket, update_checkout_ticket_attachment_annotations,
    update_checkout_ticket_attachment_markup, update_checkout_ticket_attachment_metadata,
    update_ticket_attachment_annotations,
};
use routes::checkouts::{
    add_checkout_source, checkout_repository_commits, checkout_repository_file_action,
    checkout_repository_files, checkout_repository_status, configure_checkout_repository_remote,
    create_corrupt_ticket_repair, get_checkout_code_review, initialize_checkout_repository,
    list_checkout_corrupt_tickets, list_checkout_tickets, list_checkouts,
    open_checkout_code_review, open_checkout_repository_review, open_project,
    prewarm_corrupt_tickets, register_checkout, relink_checkout_git_source, remove_checkout_source,
    resolve_checkout, set_checkout_default_source, set_checkout_source_color,
};
use routes::claims::{claim_next_ticket, claim_ticket, release_ticket, renew_ticket};
use routes::permissions::{
    ask_permission, create_drive_connection, delete_drive_connection,
    discovered_ai_tools_off_runtime, get_ai_settings, get_checkout_ai_settings,
    interrupt_drive_turn, list_ai_tools, list_connections, list_drive_sessions, list_permissions,
    now_ms, probe_permission_bridge, put_ai_settings, put_checkout_ai_settings, send_drive_turn,
};
use routes::project_tools::{
    acknowledge_notification, cancel_checkout_command_run, cancel_command_run,
    checkout_confidence_report, confidence_report, get_checkout_command_run, get_command_run,
    list_checkout_command_groups, list_checkout_command_runs, list_checkout_commands,
    list_checkout_custom_views, list_command_groups, list_command_runs, list_commands,
    list_custom_views, list_notifications, publish_notification, run_checkout_command, run_command,
    save_checkout_command_groups, save_checkout_commands, save_checkout_custom_views,
    save_command_groups, save_commands, save_custom_views, synthesize_speech, ticket_flow_summary,
    usage_metrics_summary,
};
use routes::providers::{
    assign_provider_ticket, close_provider_ticket, copy_provider_attachment,
    create_checkout_provider_connection, create_provider_connection, create_provider_ticket,
    delete_provider_connection, delete_provider_ticket_note, get_provider_ticket,
    list_checkout_provider_connections, list_checkout_providers, list_provider_ai_feedback,
    list_provider_connections, list_provider_tickets, list_providers, provider_copy_route,
    provider_for, provider_move_route, provider_transfer_error, report_provider_ticket_not_working,
    report_provider_ticket_not_working_json, restore_provider_ticket,
    set_checkout_provider_connection_disabled, set_provider_connection_disabled,
    update_checkout_provider_connection, update_provider_connection, update_provider_ticket,
};
use routes::store_scoped::{
    UnhostSweep, assign_store_ticket, close_checkout_session, close_store_ticket,
    create_store_ticket, get_store_ticket, poll_events, resolve_ticket, setup_tool,
    update_store_ticket, ws_sync,
};
pub use routes::terminals::{
    TerminalAiConnection, TerminalHalt, TerminalHookSource, TerminalQuestion,
    prune_orphaned_terminal_names, release_orphaned_drive_sessions,
    resume_broker_terminal_sessions,
};
pub(crate) use routes::terminals::{
    answer_terminal_question, ask_terminal_question, attach_terminal, clear_terminal_halt,
    connect_terminal_ai, disconnect_terminal_ai, get_checkout_terminal_settings,
    get_checkout_trash_settings, get_terminal_settings, halt_terminal, kill_terminal,
    list_terminals, open_terminal, poll_terminal_question_answer, put_checkout_terminal_settings,
    put_checkout_trash_settings, put_terminal_settings, read_terminal, rename_terminal,
    resolve_permission, resolve_terminal_question, write_terminal,
};
use routes::ticket_writes::{
    add_store, add_ticket_attachment, assign_ticket, batch_update, close_ticket, create_ticket,
    delete_ticket_note, list_store_tickets, update_ticket,
};
use routes::tickets::{
    compatibility, get_ticket, health, lifecycle_quiescence, lifecycle_restart, list_tickets,
};
use routes::transfer::{copy_ticket_route, move_ticket_route};
pub(crate) use watcher::RepositoryWatchHandle;
pub use watcher::{WatchHandle, spawn_polling_watcher_for_test, spawn_watcher};

#[cfg(test)]
mod request_performance_tests;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use time::OffsetDateTime;
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
    /// Live `hotsheet-mcp` leases (HS2-HNXE38), separate from hook-reported AI connections.
    mcp_presence: Arc<Mutex<mcp_presence::McpPresenceRegistry>>,
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
    /// Tickets whose index write failed (HS2-JD7TK0), keyed by store URL id. Shared with
    /// each store watcher, which retries them on a bounded backoff (HS2-5BAEX9); the
    /// store's next server write also retries them first.
    index_repairs: events::IndexRepairs,
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
            mcp_presence: Arc::default(),
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
            index_repairs: Default::default(),
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
    fn trash_cleanup_days_for_store(&self, root: &FsPath) -> u32 {
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
                    checkout = %checkout.id,
                    %error,
                    "trash retention setting ignored"
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
            "/checkouts/{reference}/trash/purge-preview",
            get(preview_checkout_trash_purge),
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
        // Live hotsheet-mcp presence leases (HS2-HNXE38).
        .route(
            "/mcp-presence",
            get(routes::mcp_presence::list_mcp_presence)
                .post(routes::mcp_presence::put_mcp_presence),
        )
        .route(
            "/mcp-presence/{lease}",
            delete(routes::mcp_presence::delete_mcp_presence),
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
mod terminal_permission_route_tests {
    use super::AppState;
    use crate::routes::terminals::OpenTerminalReq;
    use crate::routes::terminals::launch::{
        terminal_permission_project, terminal_permission_route_env,
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
    use crate::watcher::startup_bridge_config;
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
    use crate::routes::terminals::ai_terminal_tool;
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
