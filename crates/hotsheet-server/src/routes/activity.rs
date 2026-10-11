//! Activity timeline and announcement routes (HS2-KP31ZE).

use crate::*;

// ---- activity timeline (HS2-KP31ZE) ----------------------------------------------

/// `POST /activity` body — a tool's activity signal. The server stamps `id`/`ts`; `summary`
/// and `importance` default from the kind unless the caller provides them (docs/15 §15.7).
#[derive(Debug, Deserialize)]
pub(crate) struct ActivityIngest {
    pub(crate) tool: String,
    pub(crate) kind: hotsheet_ticketing::ActivityKind,
    #[serde(default)]
    pub(crate) detail: serde_json::Value,
    #[serde(default)]
    pub(crate) ticket: Option<String>,
    #[serde(default)]
    pub(crate) session: Option<String>,
    #[serde(default)]
    pub(crate) project: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) importance: Option<hotsheet_ticketing::Importance>,
}

/// `POST /activity` — record one activity event to the store's rolling window.
pub(crate) async fn ingest_activity(
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
pub(crate) struct ActivityParams {
    pub(crate) ticket: Option<String>,
    pub(crate) session: Option<String>,
    /// `low` | `normal` | `high` — only events at or above this emphasis.
    pub(crate) min_importance: Option<String>,
    pub(crate) limit: Option<usize>,
}

/// `GET /activity` — the per-ticket/session "what happened" window, most-recent-capped.
pub(crate) async fn list_activity(
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
pub(crate) struct AnnounceReq {
    pub(crate) message: String,
    /// The target store's URL id; omitted = the default store.
    #[serde(default)]
    pub(crate) store: Option<String>,
}

/// `POST /announce` — broadcast an ephemeral message to live `/ws/sync` subscribers. Not
/// persisted (no long-poll replay); a client not connected when it fires misses it.
pub(crate) async fn post_announce(
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
