//! Hot Sheet 2 server: a thin HTTP + WebSocket layer over the shared engine
//! (`hotsheet-ticketing::ops`), the single authority every GUI talks to
//! (`docs/04-core-server-cli.md` §4.3). v1 is loopback + shared-secret (Tier 0).
//! Reads go through the SQLite/FTS index (HS2-5); a filesystem watcher (HS2-6) keeps
//! it fresh and broadcasts change events, so a CLI/git edit shows up live. Terminals
//! (HS2-10) and the detached lifecycle (HS2-59) are separate.

pub mod lifecycle;
pub mod multistore;

use std::path::Path as FsPath;
use std::sync::{Arc, Mutex};

use multistore::{StoreEntry, StoreHost, StoreInfo};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use hotsheet_index::{Index, IndexError, TicketRow, hash_bytes};
use hotsheet_model::{CloseReason, NoteKind, Ticket, Timestamp, Ulid, parse_file, to_file_string};
use hotsheet_ticketing::{
    FsStore, NewTicket, OpError, SortKey, StoreError, TicketPatch, TicketQuery, ops,
};
// Wire DTOs are defined once in the engine crate (wire SSOT); re-export for callers.
pub use hotsheet_ticketing::{ApiNote, ApiTicket};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::broadcast;

/// Shared server state (cheaply cloned into each handler).
#[derive(Clone)]
pub struct AppState {
    store: FsStore,
    secret: String,
    events: broadcast::Sender<ChangeEvent>,
    index: Arc<Mutex<Index>>,
    /// Every store this machine server hosts (HS2-87). The primary `store` is registered
    /// here as the default entry; additional stores are added via `POST /stores`.
    host: StoreHost,
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
        let (events, _) = broadcast::channel(256);
        let index = Arc::new(Mutex::new(index));
        let host = StoreHost::new();
        // The primary store is the default hosted entry (shares the same index Arc, so
        // the unprefixed routes and /stores/{default}/… see one index).
        host.register(StoreEntry {
            store: store.clone(),
            index: index.clone(),
        });
        Self {
            store,
            secret,
            events,
            index,
            host,
            watchers: Arc::new(Mutex::new(Vec::new())),
            persist_indexes: false,
            instance: Arc::new(Mutex::new(None)),
            instance_guards: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Persist the indexes of `POST /stores`-registered stores to
    /// `${HOTSHEET_HOME}/index/` (call this in a real server run; leave off in tests so
    /// they never write under the machine home). Builder-style.
    pub fn with_persistent_registered_indexes(mut self) -> Self {
        self.persist_indexes = true;
        self
    }

    /// Host a store: build its index (file-backed when persisting, else in-memory),
    /// register it, and spawn its fs-watcher. Idempotent by store id — returns whether it
    /// was newly added. Shared by `POST /stores` and startup discovery.
    fn host_store(&self, store: FsStore) -> Result<bool, ApiError> {
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
        match spawn_watcher_for(WatchTarget {
            entry,
            store_id: id,
            events: self.events.clone(),
        }) {
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
        let _ = self.events.send(event); // Err just means no subscribers
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
        });
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
}

/// Build the router. Ticket routes require the secret; `/health` and `/ws/sync` don't
/// (the WS checks the secret via a query param, since browsers can't set WS headers).
pub fn app(state: AppState) -> Router {
    let protected = Router::new()
        .route("/tickets", get(list_tickets).post(create_ticket))
        .route("/tickets/{id}", get(get_ticket).patch(update_ticket))
        .route("/tickets/{id}/close", post(close_ticket))
        .route("/setup/{tool}", post(setup_tool))
        // Multi-store (HS2-87): list/register hosted stores + store-scoped ticket routes
        // (path-prefix scheme, maintainer's pick), sharing the default routes' logic.
        .route("/stores", get(list_stores).post(add_store))
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
        // Cross-store resolve: a global ULID → its live instance in whichever store hosts
        // it (follows moved tombstones). HS2-87 / HS2-S4H2AM.
        .route("/resolve/{id}", get(resolve_ticket))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_secret,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/ws/sync", get(ws_sync))
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
    let count = state.store.list_tickets()?.len();
    Ok(Json(
        serde_json::json!({ "status": "ok", "tickets": count }),
    ))
}

async fn list_tickets(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<TicketRow>>, ApiError> {
    let compact = params.compact.unwrap_or(true);
    let query = params.into_query()?;
    let mut rows = state
        .index
        .lock()
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned"))?
        .query(&query)?;
    if compact {
        for row in &mut rows {
            row.make_compact();
        }
    }
    Ok(Json(rows))
}

async fn get_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiTicket>, ApiError> {
    let ticket = ops::resolve(&state.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json((&ticket).into()))
}

// ---- multi-store (HS2-87) --------------------------------------------------------

/// `GET /stores` — the stores this machine server hosts.
async fn list_stores(State(state): State<AppState>) -> Json<Vec<StoreInfo>> {
    Json(state.host.list())
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
) -> Result<Json<Vec<TicketRow>>, ApiError> {
    let entry = state
        .host
        .get(&store_id)
        .ok_or_else(|| ApiError::not_found(&store_id))?;
    let compact = params.compact.unwrap_or(true);
    let query = params.into_query()?;
    let mut rows = entry
        .index
        .lock()
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned"))?
        .query(&query)?;
    if compact {
        for row in &mut rows {
            row.make_compact();
        }
    }
    Ok(Json(rows))
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
    Ok((&ticket).into())
}

fn do_update(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
    req: UpdateReq,
) -> Result<ApiTicket, ApiError> {
    let ticket = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
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
    // An optional note append rides the same update call (parity with the CLI + MCP).
    let latest = match req.note.filter(|t| !t.is_empty()) {
        Some(text) => ops::add_note(
            &entry.store,
            &ticket.id,
            Ulid::new(),
            now(),
            NoteKind::Regular,
            text,
        )?,
        None => updated,
    };
    state.changed_in(entry, "updated", &latest);
    Ok((&latest).into())
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
    Ok((&closed).into())
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

async fn close_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    Ok(Json(do_close(&state, &state.default_entry(), &id, req)?))
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

async fn get_store_ticket(
    State(state): State<AppState>,
    Path((store_id, id)): Path<(String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json((&ticket).into()))
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
    Ok(Json(ResolvedTicket {
        store,
        ticket: (&ticket).into(),
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
    let reports = hotsheet_plugins::run_setup(&store, &store, Some(&tool), false, None)
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

// ---- request / response DTOs -----------------------------------------------------

#[derive(Debug, Default, Deserialize)]
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
    sort: Option<String>,
    limit: Option<usize>,
    /// Omit the Markdown body from each row (default true). `compact=false` keeps it.
    compact: Option<bool>,
}

impl ListParams {
    fn into_query(self) -> Result<TicketQuery, ApiError> {
        let sort = match self.sort {
            Some(s) => s
                .parse::<SortKey>()
                .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?,
            None => SortKey::default(),
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
            assignee: self.assignee,
            sort,
            limit: self.limit,
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

#[derive(Debug, Deserialize)]
struct UpdateReq {
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
}

#[derive(Debug, Deserialize)]
struct CloseReq {
    reason: String,
    duplicate_of: Option<String>,
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
            other @ (OpError::WrongWorker { .. } | OpError::NotClaimed(_)) => {
                ApiError::new(StatusCode::CONFLICT, other.to_string())
            }
            other @ (OpError::DuplicateNeedsTarget | OpError::SelfBlock(_)) => {
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
    _watcher: notify::RecommendedWatcher,
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
}

/// Watch the **default** store (back-compat entry point used by the server binary).
pub fn spawn_watcher(state: AppState) -> anyhow::Result<WatchHandle> {
    let target = WatchTarget {
        entry: state.default_entry(),
        store_id: multistore::store_url_id(&state.store),
        events: state.events.clone(),
    };
    spawn_watcher_for(target)
}

/// Watch one store (any hosted store). The returned [`WatchHandle`] must be kept alive
/// for the watcher to run.
fn spawn_watcher_for(target: WatchTarget) -> anyhow::Result<WatchHandle> {
    use notify::{RecursiveMode, Watcher};

    let tickets_dir = target.entry.store.root().join("tickets");
    std::fs::create_dir_all(&tickets_dir)?;

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(&tickets_dir, RecursiveMode::Recursive)?;

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
        // The derived worklist.md is regenerated once per debounced batch (not per file),
        // so it stays in sync with the tickets without churning on every event (docs/03
        // §3.6, HS2-90). worklist.md lives at the store root — outside the watched
        // tickets/ dir — so this write never re-triggers the watcher.
        if let Err(e) = hotsheet_ticketing::worklist::regenerate(&target.entry.store) {
            eprintln!("worklist regenerate failed: {e}");
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
        let _ = target.events.send(ChangeEvent {
            store: target.store_id.clone(),
            kind: kind.to_string(),
            id,
            slug,
        });
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
    use super::expand_ticket_files;

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
