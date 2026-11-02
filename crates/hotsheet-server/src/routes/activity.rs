//! Activity timeline and announcement routes (HS2-KP31ZE).

use crate::api_error::ApiError;
use crate::routes::checkouts::{checkout_entries, checkout_lookup_error};
use crate::{AppState, multistore, now};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hotsheet_model::Ulid;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path as FsPath, PathBuf};
use std::process::Stdio;

const MAX_CURRENT_DIFF_BYTES: u64 = 64 * 1024;

// ---- activity timeline (HS2-KP31ZE) ----------------------------------------------

/// `POST /activity` body — a tool's activity signal. The server stamps `id`/`ts`; `summary`
/// and `importance` default from the kind unless the caller provides them (docs/15 §15.7).
#[derive(Debug, Deserialize)]
pub(crate) struct ActivityIngest {
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
pub(crate) async fn ingest_activity(
    State(state): State<AppState>,
    Json(body): Json<ActivityIngest>,
) -> Result<Json<hotsheet_ticketing::ActivityEvent>, ApiError> {
    // The public ingest boundary follows the same edit metadata rule as native adapters.
    // In particular, old/new strings and patch text are never retained or broadcast.
    let detail = if body.kind == hotsheet_ticketing::ActivityKind::Edit {
        let input = &body.detail;
        let mut projected = serde_json::Map::new();
        for key in ["path", "operation"] {
            if let Some(value) = input.get(key).and_then(serde_json::Value::as_str) {
                projected.insert(key.into(), value.into());
            }
        }
        if let Some(count) = input.get("file_count").and_then(serde_json::Value::as_u64) {
            projected.insert("file_count".into(), count.into());
        }
        serde_json::Value::Object(projected)
    } else {
        body.detail
    };
    let mut ev = hotsheet_ticketing::ActivityEvent::new(
        Ulid::new().to_string(),
        now().as_str().to_string(),
        body.tool,
        body.kind,
        detail,
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
    ticket: Option<String>,
    session: Option<String>,
    /// `low` | `normal` | `high` — only events at or above this emphasis.
    min_importance: Option<String>,
    limit: Option<usize>,
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

/// A retained event with its authoritative store identity. One store may be linked to several
/// checkouts, so the identity also lets clients deduplicate a cross-project reel (HS2-5PSQJQ).
#[derive(Serialize)]
pub(crate) struct CheckoutActivityEvent {
    store_id: String,
    event: hotsheet_ticketing::ActivityEvent,
}

/// `GET /checkouts/{reference}/activity` reads all linked Git stores, deduplicating stores that
/// are linked through several sources. The retained activity window is already bounded by the
/// store, and the response is capped after merging stores.
pub(crate) async fn checkout_activity(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(params): Query<ActivityParams>,
) -> Result<Json<Vec<CheckoutActivityEvent>>, ApiError> {
    tokio::task::spawn_blocking(move || {
        state
            .checkout_registry
            .resolve(&reference)
            .map_err(checkout_lookup_error)?;
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
        let mut seen = BTreeSet::new();
        let mut events = Vec::new();
        for (store_id, entry) in checkout_entries(&state, &reference)? {
            if !seen.insert(store_id.clone()) {
                continue;
            }
            let filter = hotsheet_ticketing::TimelineFilter {
                ticket: params.ticket.clone(),
                session: params.session.clone(),
                min_importance,
                limit: params.limit,
            };
            let timeline =
                hotsheet_ticketing::activity::timeline(&entry.store, &filter).map_err(|error| {
                    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
            events.extend(timeline.into_iter().map(|event| CheckoutActivityEvent {
                store_id: store_id.clone(),
                event,
            }));
        }
        events.sort_by(|left, right| {
            left.event
                .ts
                .cmp(&right.event.ts)
                .then_with(|| left.event.id.cmp(&right.event.id))
                .then_with(|| left.store_id.cmp(&right.store_id))
        });
        if let Some(limit) = params.limit {
            if events.len() > limit {
                events.drain(..events.len() - limit);
            }
        }
        Ok(Json(events))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

/// An explicit, non-retained read of the checkout's current working-tree diff (HS2-A93BHS).
/// Native patch text never enters the 14-day activity stream. The path is metadata supplied by
/// a selected edit; it is confined to the resolved checkout before Git sees it.
#[derive(Debug, Deserialize)]
pub(crate) struct CurrentDiffRequest {
    path: String,
}

#[derive(Serialize)]
pub(crate) struct CurrentDiffResponse {
    diff: String,
    truncated: bool,
}

fn confined_diff_path(root: &FsPath, requested: &str) -> Result<PathBuf, ApiError> {
    if requested.is_empty() || requested.len() > 4096 {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid edit path"));
    }
    let registered_root = root;
    let root = root
        .canonicalize()
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "checkout is unavailable"))?;
    let input = FsPath::new(requested);
    let relative = if input.is_absolute() {
        input
            .strip_prefix(registered_root)
            .or_else(|_| input.strip_prefix(&root))
            .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "edit path is outside checkout"))?
    } else {
        input
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || relative
            .components()
            .next()
            .is_some_and(|part| part.as_os_str() == ".git")
    {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid edit path"));
    }
    let target = root.join(relative);
    // An existing symlink (or a deleted file under a symlinked parent) cannot escape the root.
    let resolved = if target.exists() {
        target.canonicalize()
    } else {
        target.parent().unwrap_or(&root).canonicalize()
    }
    .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "edit path is unavailable"))?;
    if !resolved.starts_with(&root) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "edit path is outside checkout",
        ));
    }
    Ok(relative.to_path_buf())
}

fn read_current_diff(root: &FsPath, requested: &str) -> Result<CurrentDiffResponse, ApiError> {
    let relative = confined_diff_path(root, requested)?;
    let mut child = hotsheet_ticketing::git::command_in(root)
        .args([
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "HEAD",
            "--",
        ])
        .arg(relative)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "current diff is unavailable",
            )
        })?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .expect("piped Git stdout")
        .take(MAX_CURRENT_DIFF_BYTES + 1)
        .read_to_end(&mut bytes);
    let truncated = bytes.len() as u64 > MAX_CURRENT_DIFF_BYTES;
    if truncated || read.is_err() {
        let _ = child.kill();
    }
    let status = child.wait();
    if read.is_err() || (!truncated && !status.is_ok_and(|status| status.success())) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "current diff is unavailable",
        ));
    }
    bytes.truncate(MAX_CURRENT_DIFF_BYTES as usize);
    Ok(CurrentDiffResponse {
        diff: String::from_utf8_lossy(&bytes).into_owned(),
        truncated,
    })
}

pub(crate) async fn checkout_current_diff(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(body): Json<CurrentDiffRequest>,
) -> Result<
    (
        [(axum::http::HeaderName, &'static str); 1],
        Json<CurrentDiffResponse>,
    ),
    ApiError,
> {
    tokio::task::spawn_blocking(move || {
        let checkout = state
            .checkout_registry
            .resolve(&reference)
            .map_err(checkout_lookup_error)?;
        let diff = read_current_diff(FsPath::new(&checkout.root), &body.path)?;
        Ok((
            [(axum::http::header::CACHE_CONTROL, "no-store")],
            Json(diff),
        ))
    })
    .await
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "current diff failed"))?
}

/// `POST /announce` body: a store-level broadcast message (HS2-HHDNTH).
#[derive(Debug, Deserialize)]
pub(crate) struct AnnounceReq {
    message: String,
    /// The target store's URL id; omitted = the default store.
    #[serde(default)]
    store: Option<String>,
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
