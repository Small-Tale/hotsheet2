//! Claude subagent hooks as live progress (HS2-X0ZSZ8).
//!
//! HS2-PPZQX9 made each recorded model response progress, but the server follows usage only
//! for sessions in Hot Sheet terminals and for driven turns. A Claude session started in its
//! own shell, IDE or the desktop app reports no transcript, so its background agents were
//! invisible until they wrote a ticket. The installed Claude hooks therefore report a
//! subagent's `SubagentStart`, `SubagentStop` and its tool calls (`PreToolUse` carrying
//! `agent_id`) here, with only identifiers and the session's working directory
//! (`hotsheet_cli::permission_hook::subagent_progress_payload`). The checkout containing that
//! directory decides the store, as for a terminal's usage (HS2-B4EQ87): its usage source. The
//! report moves that store's last progress through the same tracker call recorded responses
//! use, so the live word reads Active while the agent works.

use crate::routes::terminals::usage_store::checkout_containing;
use crate::{ApiError, AppState};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The longest accepted identifier and directory, matching the transcript route's bounds.
const MAX_ID: usize = 128;
const MAX_CWD: usize = 4096;

/// Which subagent hook was reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SubagentEvent {
    Start,
    Stop,
    Tool,
}

/// `POST /ai-hooks/subagent-progress`: one subagent hook of a Claude session.
#[derive(Debug, Deserialize)]
pub(crate) struct SubagentProgressReq {
    event: SubagentEvent,
    session_id: String,
    cwd: String,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    agent_type: Option<String>,
}

/// Where the report counted: the store whose progress moved, or none when the directory is
/// outside every registered checkout or the checkout links no local ticket source.
#[derive(Debug, Serialize)]
pub(crate) struct SubagentProgressReply {
    recorded: bool,
    store: Option<String>,
}

/// Record a Claude subagent hook as progress for the store of the checkout containing the
/// session's directory (HS2-X0ZSZ8). An unknown directory records nothing and still succeeds,
/// so a hook in an unregistered project is quiet.
pub(crate) async fn report_subagent_progress(
    State(state): State<AppState>,
    Json(body): Json<SubagentProgressReq>,
) -> Result<Json<SubagentProgressReply>, ApiError> {
    validate(&body)?;
    let store = tokio::task::spawn_blocking(move || record(&state, Path::new(&body.cwd)))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(SubagentProgressReply {
        recorded: store.is_some(),
        store,
    }))
}

fn validate(body: &SubagentProgressReq) -> Result<(), ApiError> {
    let bad_id = |id: &str| id.is_empty() || id.len() > MAX_ID || id.chars().any(char::is_control);
    let optional_bad = |id: &Option<String>| id.as_deref().is_some_and(bad_id);
    let cwd = body.cwd.trim();
    if bad_id(&body.session_id)
        || optional_bad(&body.agent_id)
        || optional_bad(&body.agent_type)
        || cwd.is_empty()
        || cwd.len() > MAX_CWD
        || !Path::new(cwd).is_absolute()
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "a subagent progress report needs a session id, an absolute directory, and bounded agent ids",
        ));
    }
    tracing::debug!(
        event = ?body.event,
        agent_type = body.agent_type.as_deref().unwrap_or_default(),
        "Claude subagent progress"
    );
    Ok(())
}

/// Move the containing checkout's usage store's progress; returns that store's id. Blocking:
/// reads the checkout registry.
fn record(state: &AppState, cwd: &Path) -> Option<String> {
    let checkout = checkout_containing(state, cwd)?;
    let store = store_id(state, &checkout)?;
    crate::notification_router::note_usage_progress(state, &store);
    crate::live_state::touch(state, &store);
    Some(store)
}

/// The id of `checkout`'s usage store: the hosted entry's id, else the store opened from its
/// path, the same store a terminal in that checkout records usage in (HS2-B4EQ87).
fn store_id(
    state: &AppState,
    checkout: &hotsheet_ticketing::checkouts::Checkout,
) -> Option<String> {
    let source = checkout.usage_source()?;
    if let Some(entry) = state.host.get(&source.connection_id) {
        return Some(crate::multistore::store_url_id(&entry.store));
    }
    hotsheet_ticketing::FsStore::open(&source.locator)
        .ok()
        .map(|store| crate::multistore::store_url_id(&store))
}
