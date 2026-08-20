//! Hot Sheet 2 server: a thin HTTP + WebSocket layer over the shared engine
//! (`hotsheet-ticketing::ops`), the single authority every GUI talks to
//! (`docs/04-core-server-cli.md` §4.3). v1 is loopback + shared-secret (Tier 0).
//! Reads go through the SQLite/FTS index (HS2-5); a filesystem watcher (HS2-6) keeps
//! it fresh and broadcasts change events, so a CLI/git edit shows up live. Terminals
//! (HS2-10) and the detached lifecycle (HS2-59) are separate.

use std::path::Path as FsPath;
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use hotsheet_index::{Index, IndexError, TicketRow, hash_bytes};
use hotsheet_model::{CloseReason, Ticket, Timestamp, Ulid, parse_file, to_file_string};
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
}

impl AppState {
    /// State over a store + a prepared index, guarded by `secret`. The caller decides
    /// whether the index is in-memory or file-backed (`Index::open_reconciled`).
    pub fn with_index(store: FsStore, secret: String, index: Index) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            store,
            secret,
            events,
            index: Arc::new(Mutex::new(index)),
        }
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

    /// Reindex a ticket the server just wrote, then broadcast. The index now carries
    /// the file's hash, so the watcher will see "no change" and not re-emit.
    fn changed(&self, kind: &str, t: &Ticket) {
        let text = to_file_string(t);
        let path = self.store.ticket_path(&t.id).display().to_string();
        if let Ok(index) = self.index.lock() {
            let _ = index.upsert(t, &path, &hash_bytes(text.as_bytes()));
        }
        self.emit(ChangeEvent {
            kind: kind.to_string(),
            id: t.id.to_string(),
            slug: t.slug.clone(),
        });
    }
}

/// A live-change event pushed over `/ws/sync`.
#[derive(Clone, Debug, Serialize)]
pub struct ChangeEvent {
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
    let query = params.into_query()?;
    let rows = state
        .index
        .lock()
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "index lock poisoned"))?
        .query(&query)?;
    Ok(Json(rows))
}

async fn get_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiTicket>, ApiError> {
    let ticket = ops::resolve(&state.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json((&ticket).into()))
}

async fn create_ticket(
    State(state): State<AppState>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let prefix = state.store.metadata()?.ticket_prefix;
    let new = NewTicket {
        title: req.title,
        category: req.category.unwrap_or_else(|| "issue".to_string()),
        priority: opt_parse(req.priority.as_deref())?.unwrap_or_default(),
        details: req.details.unwrap_or_default(),
        tags: req.tags.unwrap_or_default(),
        up_next: req.up_next.unwrap_or(false),
    };
    let ticket = ops::create(&state.store, Ulid::new(), &prefix, now(), new)?;
    state.changed("created", &ticket);
    Ok((StatusCode::CREATED, Json((&ticket).into())))
}

async fn update_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let ticket = ops::resolve(&state.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let patch = TicketPatch {
        title: req.title,
        details: req.details,
        category: req.category,
        priority: opt_parse(req.priority.as_deref())?,
        status: opt_parse(req.status.as_deref())?,
        tags: req.tags,
        up_next: req.up_next,
    };
    let updated = ops::update(&state.store, &ticket.id, now(), patch)?;
    state.changed("updated", &updated);
    Ok(Json((&updated).into()))
}

async fn close_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let ticket = ops::resolve(&state.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let reason: CloseReason = opt_parse(Some(req.reason.as_str()))?.expect("reason present");
    let dup = match req.duplicate_of {
        Some(d) => Some(
            ops::resolve(&state.store, &d)?
                .ok_or_else(|| ApiError::not_found(&d))?
                .id,
        ),
        None => None,
    };
    let closed = ops::close(&state.store, &ticket.id, now(), reason, dup)?;
    state.changed("closed", &closed);
    Ok(Json((&closed).into()))
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
    sort: Option<String>,
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
            sort,
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
            other @ OpError::DuplicateNeedsTarget => {
                ApiError::new(StatusCode::BAD_REQUEST, other.to_string())
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
pub fn spawn_watcher(state: AppState) -> anyhow::Result<WatchHandle> {
    use notify::{RecursiveMode, Watcher};

    let tickets_dir = state.store.root().join("tickets");
    std::fs::create_dir_all(&tickets_dir)?;

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(&tickets_dir, RecursiveMode::Recursive)?;

    std::thread::spawn(move || watch_loop(rx, state));
    Ok(WatchHandle { _watcher: watcher })
}

fn watch_loop(rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>, state: AppState) {
    use std::time::Duration;

    while let Ok(first) = rx.recv() {
        let mut paths = event_paths(first);
        // Debounce a burst (editor save, git checkout touching many files).
        while let Ok(next) = rx.recv_timeout(Duration::from_millis(150)) {
            paths.extend(event_paths(next));
        }
        paths.sort();
        paths.dedup();
        for path in paths {
            handle_path_change(&state, &path);
        }
    }
}

fn event_paths(res: notify::Result<notify::Event>) -> Vec<std::path::PathBuf> {
    let Ok(event) = res else {
        return Vec::new();
    };
    event
        .paths
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect()
}

fn handle_path_change(state: &AppState, path: &FsPath) {
    // The filename stem is the ticket ULID.
    let Some(id) = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| Ulid::from_string(s).ok())
    else {
        return;
    };

    if !path.exists() {
        if let Ok(index) = state.index.lock() {
            let _ = index.delete(&id);
        }
        state.emit(ChangeEvent {
            kind: "deleted".into(),
            id: id.to_string(),
            slug: String::new(),
        });
        return;
    }

    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let hash = hash_bytes(&bytes);

    // Unchanged since we last indexed it (incl. the server's own write) → skip.
    let already = state
        .index
        .lock()
        .ok()
        .and_then(|index| index.content_hash(&id).ok().flatten());
    if already.as_deref() == Some(hash.as_str()) {
        return;
    }

    let Ok(ticket) = parse_file(&String::from_utf8_lossy(&bytes)) else {
        return;
    };
    if let Ok(index) = state.index.lock() {
        let _ = index.upsert(&ticket, &path.display().to_string(), &hash);
    }
    state.emit(ChangeEvent {
        kind: "changed".into(),
        id: ticket.id.to_string(),
        slug: ticket.slug.clone(),
    });
}
