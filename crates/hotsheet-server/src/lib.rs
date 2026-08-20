//! Hot Sheet 2 server: a thin HTTP + WebSocket layer over the shared engine
//! (`hotsheet-ticketing::ops`), the single authority every GUI talks to
//! (`docs/04-core-server-cli.md` §4.3). v1 is loopback + shared-secret (Tier 0);
//! it scans the store in-memory (the SQLite/FTS index is HS2-5) and holds no watcher
//! or terminals yet (those need HS2-6 / HS2-10).

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use hotsheet_model::{CloseReason, NoteKind, Priority, Status, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::{
    FsStore, NewTicket, OpError, SortKey, StoreError, TicketPatch, TicketQuery, ops,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::broadcast;

/// Shared server state (cheaply cloned into each handler).
#[derive(Clone)]
pub struct AppState {
    store: FsStore,
    secret: String,
    events: broadcast::Sender<ChangeEvent>,
}

impl AppState {
    /// New state over a store, guarded by `secret`.
    pub fn new(store: FsStore, secret: String) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            store,
            secret,
            events,
        }
    }

    fn emit(&self, kind: &str, t: &Ticket) {
        // Errors mean no subscribers; that's fine.
        let _ = self.events.send(ChangeEvent {
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
) -> Result<Json<Vec<ApiTicket>>, ApiError> {
    let query = params.into_query()?;
    let tickets = ops::query(&state.store, &query)?;
    Ok(Json(tickets.iter().map(ApiTicket::from).collect()))
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
    state.emit("created", &ticket);
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
    state.emit("updated", &updated);
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
    state.emit("closed", &closed);
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

/// The JSON shape of a ticket on the wire — the full ticket, including the Markdown
/// body and notes (unlike the frontmatter-only serde on `Ticket`).
#[derive(Debug, Serialize)]
pub struct ApiTicket {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub details: String,
    pub category: String,
    pub priority: Priority,
    pub status: Status,
    pub up_next: bool,
    pub tags: Vec<String>,
    pub blocked_by: Vec<String>,
    pub blocked_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub verified_at: Option<String>,
    pub closed_at: Option<String>,
    pub close_reason: Option<CloseReason>,
    pub duplicate_of: Option<String>,
    pub claimed_by: Option<String>,
    pub worker_label: Option<String>,
    pub claim_count: u32,
    pub assignees: Vec<String>,
    pub legacy_number: Option<String>,
    pub schema: u32,
    pub notes: Vec<ApiNote>,
}

#[derive(Debug, Serialize)]
pub struct ApiNote {
    pub id: String,
    pub kind: NoteKind,
    pub at: String,
    pub text: String,
}

impl From<&Ticket> for ApiTicket {
    fn from(t: &Ticket) -> Self {
        let ts = |o: &Option<Timestamp>| o.as_ref().map(|x| x.as_str().to_string());
        ApiTicket {
            id: t.id.to_string(),
            slug: t.slug.clone(),
            title: t.title.clone(),
            details: t.details.clone(),
            category: t.category.clone(),
            priority: t.priority,
            status: t.status,
            up_next: t.up_next,
            tags: t.tags.clone(),
            blocked_by: t.blocked_by.iter().map(|u| u.to_string()).collect(),
            blocked_reason: t.blocked_reason.clone(),
            created_at: t.created_at.as_str().to_string(),
            updated_at: t.updated_at.as_str().to_string(),
            completed_at: ts(&t.completed_at),
            verified_at: ts(&t.verified_at),
            closed_at: ts(&t.closed_at),
            close_reason: t.close_reason,
            duplicate_of: t.duplicate_of.map(|u| u.to_string()),
            claimed_by: t.claimed_by.clone(),
            worker_label: t.worker_label.clone(),
            claim_count: t.claim_count,
            assignees: t.assignees.clone(),
            legacy_number: t.legacy_number.clone(),
            schema: t.schema,
            notes: t
                .notes
                .iter()
                .map(|n| ApiNote {
                    id: n.id.to_string(),
                    kind: n.kind,
                    at: n.at.as_str().to_string(),
                    text: n.text.clone(),
                })
                .collect(),
        }
    }
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
