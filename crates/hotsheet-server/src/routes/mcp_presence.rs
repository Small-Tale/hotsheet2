//! `hotsheet-mcp` presence routes (HS2-HNXE38); the lease model lives in
//! [`crate::mcp_presence`].

use std::time::Instant;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use serde::{Deserialize, Serialize};

use crate::mcp_presence::{HEARTBEAT, LEASE_TTL, McpPresenceInfo, Scope, Transition};
use crate::{ApiError, AppState, ChangeEvent};

/// Body for `POST /mcp-presence`: register or renew one shim's lease.
#[derive(Deserialize)]
pub(crate) struct McpPresenceReq {
    lease_id: String,
    #[serde(flatten)]
    info: McpPresenceInfo,
}

#[derive(Serialize)]
pub(crate) struct McpPresenceLease {
    lease_id: String,
    ttl_ms: u64,
    heartbeat_ms: u64,
}

#[derive(Serialize)]
pub(crate) struct McpPresenceEntry {
    lease_id: String,
    #[serde(flatten)]
    info: McpPresenceInfo,
}

#[derive(Serialize)]
pub(crate) struct McpPresenceList {
    /// Some `hotsheet-mcp` holds a live lease for this project.
    connected: bool,
    leases: Vec<McpPresenceEntry>,
}

fn bounded(value: Option<String>, max: usize) -> Option<String> {
    value
        .map(|v| v.trim().chars().take(max).collect::<String>())
        .filter(|v| !v.is_empty())
}

/// `POST /mcp-presence` — register a lease, or renew it (idempotent by `lease_id`). A renewal
/// the server no longer knows (it restarted, or the lease lapsed) registers it again.
pub(crate) async fn put_mcp_presence(
    State(state): State<AppState>,
    Json(body): Json<McpPresenceReq>,
) -> Result<Json<McpPresenceLease>, ApiError> {
    let lease_id = body.lease_id.trim().to_owned();
    if lease_id.is_empty() || lease_id.len() > 128 || body.info.pid == 0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "lease_id (1-128 chars) and a nonzero pid are required",
        ));
    }
    let info = McpPresenceInfo {
        pid: body.info.pid,
        tool: bounded(body.info.tool, 64),
        cwd: bounded(body.info.cwd, 4096),
        terminal_id: bounded(body.info.terminal_id, 128),
    };
    let (fresh, transitions) = {
        let mut registry = state.mcp_presence.lock_or_recover();
        let fresh = !registry.contains(&lease_id);
        (fresh, registry.upsert(&lease_id, info, Instant::now()))
    };
    emit_transitions(&state, &transitions);
    if fresh {
        watch_lease(&state, lease_id.clone());
    }
    Ok(Json(McpPresenceLease {
        lease_id,
        ttl_ms: u64::try_from(LEASE_TTL.as_millis()).unwrap_or(u64::MAX),
        heartbeat_ms: u64::try_from(HEARTBEAT.as_millis()).unwrap_or(u64::MAX),
    }))
}

/// `DELETE /mcp-presence/{lease}` — the shim's stdin closed. Unknown leases are a no-op.
pub(crate) async fn delete_mcp_presence(
    State(state): State<AppState>,
    Path(lease): Path<String>,
) -> StatusCode {
    let transitions = state.mcp_presence.lock_or_recover().remove(&lease);
    emit_transitions(&state, &transitions);
    StatusCode::NO_CONTENT
}

/// `GET /mcp-presence` — the project's live leases.
pub(crate) async fn list_mcp_presence(State(state): State<AppState>) -> Json<McpPresenceList> {
    let leases: Vec<McpPresenceEntry> = state
        .mcp_presence
        .lock_or_recover()
        .list()
        .into_iter()
        .map(|(lease_id, info)| McpPresenceEntry { lease_id, info })
        .collect();
    Json(McpPresenceList {
        connected: !leases.is_empty(),
        leases,
    })
}

/// Announce each connected-state change as an `mcp_presence` change event: `id` is the
/// terminal id (empty for the project scope), `message` is `connected` or `disconnected`.
pub(crate) fn emit_transitions(state: &AppState, transitions: &[Transition]) {
    for transition in transitions {
        let id = match &transition.scope {
            Scope::Project => String::new(),
            Scope::Terminal(id) => id.clone(),
        };
        state.emit(ChangeEvent {
            cursor: None,
            store: String::new(),
            kind: "mcp_presence".into(),
            id,
            slug: String::new(),
            message: Some(
                if transition.connected {
                    "connected"
                } else {
                    "disconnected"
                }
                .into(),
            ),
            activity: None,
            assignment: None,
            turn: None,
        });
    }
}

/// Server-local expiry for one lease: wake at its expiry (and at each heartbeat to notice a
/// dead process early), drop it once it lapsed or its process is gone, and stop when it is
/// removed. No network traffic is involved.
fn watch_lease(state: &AppState, lease: String) {
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            let Some(expires) = state.mcp_presence.lock_or_recover().expires(&lease) else {
                return;
            };
            let wake = expires.min(Instant::now() + HEARTBEAT);
            tokio::time::sleep_until(tokio::time::Instant::from_std(wake)).await;
            let (gone, transitions) = {
                let mut registry = state.mcp_presence.lock_or_recover();
                let transitions = registry.sweep(Instant::now(), crate::lifecycle::pid_alive);
                (!registry.contains(&lease), transitions)
            };
            emit_transitions(&state, &transitions);
            if gone {
                return;
            }
        }
    });
}
