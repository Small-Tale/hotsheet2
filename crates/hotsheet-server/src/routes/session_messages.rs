//! The **message session** operation (HS2-PEJ962, `docs/13` §13.12): push a prompt or a
//! UI command into a live AI terminal session through that tool's session-push adapter.
//!
//! The per-tool protocol lives behind `hotsheet_aitools::session_push::SessionPush`; this
//! module only keeps the per-terminal adapters, wires a supported tool's launch so its
//! session is reachable, and maps the operation onto HTTP.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_aitools::session_push::{
    self, PushError, PushLaunch, PushMessage, SessionPush, UreqHttp,
};
use hotsheet_sync::LockExt;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api_error::ApiError;

/// Live session-push adapters keyed by terminal (= connection) id.
pub(crate) type SessionPushes = Arc<Mutex<HashMap<String, Arc<dyn SessionPush>>>>;

/// The connection action a push-capable session advertises in `GET /connections`.
pub(crate) const MESSAGE_ACTION: &str = "message";

/// A launch prepared for session push: extra launch args/env plus the adapter to register
/// once the terminal is actually spawned (never on a reattach).
pub(crate) struct PreparedSessionPush {
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) adapter: Arc<dyn SessionPush>,
}

/// Prepare session push for an interactive launch of `plugin`, or `None` when the tool
/// declares no `[session_push]` or this host has no adapter for its mechanism.
pub(crate) fn prepare_session_push(
    plugin: &hotsheet_plugins::Plugin,
    directory: &str,
    model: Option<&str>,
) -> Result<Option<PreparedSessionPush>, ApiError> {
    let Some(spec) = plugin.manifest.session_push.as_ref() else {
        return Ok(None);
    };
    // A reported mechanism (Codex) registers when its SessionStart hook reports the address.
    if session_push::is_reported(spec) {
        return Ok(None);
    }
    if !session_push::is_supported(spec) {
        tracing::warn!(
            tool = plugin.id(),
            mechanism = spec.mechanism,
            "unknown session-push mechanism; the session will not accept messages"
        );
        return Ok(None);
    }
    let port = session_push::allocate_loopback_port().map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("allocating a session port: {error}"),
        )
    })?;
    let directory = std::fs::canonicalize(directory)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| directory.to_string());
    let launch = PushLaunch {
        port,
        // Two ULIDs: 160 random bits that only this server and the launched tool know.
        password: format!(
            "{}{}",
            hotsheet_model::Ulid::new(),
            hotsheet_model::Ulid::new()
        ),
        directory,
        model: model.map(str::to_string),
        launched_at_ms: crate::routes::permissions::now_ms(),
    };
    let wiring = session_push::launch_wiring(spec, &launch).unwrap_or_default();
    let Some(adapter) = session_push::adapter_for(spec, launch, Arc::new(UreqHttp::default()))
    else {
        return Ok(None);
    };
    Ok(Some(PreparedSessionPush {
        args: wiring.args,
        env: wiring.env,
        adapter,
    }))
}

/// Register a spawned terminal's adapter.
pub(crate) fn register(state: &AppState, terminal_id: &str, adapter: Arc<dyn SessionPush>) {
    state
        .session_pushes
        .lock_or_recover()
        .insert(terminal_id.to_string(), adapter);
}

/// Register (or replace) the adapter for a session whose `SessionStart` hook reported its
/// address after launch (HS2-FKM97G): the shared registration path for reported mechanisms.
/// The terminal's earlier reported adapter is dropped first; a launch-wired adapter (OpenCode)
/// is never replaced. A tool without a reported mechanism, or a report without a usable
/// address, registers nothing.
pub(crate) fn register_reported(
    state: &AppState,
    terminal_id: &str,
    agent: Option<&str>,
    reported: &session_push::ReportedSession,
) {
    unregister_reported(state, terminal_id);
    let Some(plugin) = agent.and_then(|agent| hotsheet_plugins::find_in(agent, &state.plugin_dirs))
    else {
        return;
    };
    let Some(spec) = plugin
        .manifest
        .session_push
        .as_ref()
        .filter(|spec| session_push::is_reported(spec))
    else {
        return;
    };
    let program = match state.session_push_programs.get(plugin.id()) {
        Some(program) => program.clone(),
        // A socket-based mechanism (Claude's inbox, HS2-M15CHN) never runs the tool.
        None if !session_push::reported_needs_program(&spec.mechanism) => {
            std::path::PathBuf::from(plugin.id())
        }
        None => {
            let declared = plugin
                .manifest
                .launch
                .as_ref()
                .map(|launch| launch.program.clone())
                .unwrap_or_else(|| plugin.id().to_string());
            match hotsheet_aitools::launch_safety::resolve_program(&declared) {
                Ok(program) => program,
                Err(error) => {
                    tracing::warn!(tool = plugin.id(), %error, "session push program not found");
                    return;
                }
            }
        }
    };
    let Some(adapter) = session_push::adapter_for_reported(spec, reported, &program) else {
        return;
    };
    let mut pushes = state.session_pushes.lock_or_recover();
    // Never displace a launch-wired adapter registered for this terminal.
    if pushes
        .get(terminal_id)
        .is_none_or(|current| session_push::is_reported_mechanism(current.mechanism()))
    {
        pushes.insert(terminal_id.to_string(), adapter);
    }
}

/// Drop the terminal's hook-reported adapter (SessionEnd, kill, shell exit, or a newer
/// report); a launch-wired adapter stays until its terminal exits.
pub(crate) fn unregister_reported(state: &AppState, terminal_id: &str) {
    let mut pushes = state.session_pushes.lock_or_recover();
    if pushes
        .get(terminal_id)
        .is_some_and(|current| session_push::is_reported_mechanism(current.mechanism()))
    {
        pushes.remove(terminal_id);
    }
}

/// Drop an ended terminal's adapter.
pub(crate) fn unregister(pushes: &SessionPushes, terminal_id: &str) {
    pushes.lock_or_recover().remove(terminal_id);
}

/// The adapter for a connection, if it accepts messages.
pub(crate) fn adapter(state: &AppState, connection_id: &str) -> Option<Arc<dyn SessionPush>> {
    state
        .session_pushes
        .lock_or_recover()
        .get(connection_id)
        .cloned()
}

/// `POST /connections/{id}/message` body: exactly one of `text` (a prompt, with an
/// optional visible `banner`) or `command` (a tool UI command).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageConnectionReq {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    banner: Option<String>,
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MessageConnectionResp {
    delivered: bool,
    mechanism: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    /// `false` when the tool accepted the request but cannot confirm it acted on it.
    confirmed: bool,
}

/// `POST /connections/{id}/message` — deliver a prompt or command into a live session.
///
/// - `404 connection_not_found` — no live connection with that id.
/// - `422 session_push_unsupported` — the connection's tool has no session-push mechanism
///   (Antigravity, or a tool whose adapter is not built), or the mechanism cannot carry
///   this kind of message.
/// - `502 session_unreachable` / `502 session_rejected` — the session's API is not
///   listening or answered with an error.
pub(crate) async fn message_connection(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<MessageConnectionReq>,
) -> Result<Json<MessageConnectionResp>, ApiError> {
    let message = match (req.text, req.command) {
        (Some(text), None) => PushMessage::Prompt {
            text,
            banner: req.banner,
        },
        (None, Some(command)) if req.banner.is_none() => PushMessage::Command { command },
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "send exactly one of `text` (with an optional `banner`) or `command`",
            ));
        }
    };
    let Some(adapter) = adapter(&state, &id) else {
        let tool = state
            .drive_registry
            .lock_or_recover()
            .get(&id)
            .map(|connection| connection.tool.clone());
        return Err(match tool {
            Some(tool) => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("{} sessions cannot be messaged", tool_name(&state, &tool)),
            )
            .with_code("session_push_unsupported"),
            None => ApiError::new(StatusCode::NOT_FOUND, format!("no live connection '{id}'"))
                .with_code("connection_not_found"),
        });
    };
    let receipt = tokio::task::spawn_blocking(move || adapter.message(&message))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(push_error)?;
    Ok(Json(MessageConnectionResp {
        delivered: true,
        mechanism: receipt.mechanism,
        session_id: receipt.session_id,
        confirmed: receipt.confirmed,
    }))
}

fn tool_name(state: &AppState, tool: &str) -> String {
    hotsheet_plugins::find_in(tool, &state.plugin_dirs)
        .map(|plugin| plugin.manifest.display_name.clone())
        .unwrap_or_else(|| tool.to_string())
}

fn push_error(error: PushError) -> ApiError {
    let message = error.to_string();
    match error {
        PushError::Invalid(_) => ApiError::new(StatusCode::BAD_REQUEST, message),
        PushError::Unsupported(_) => ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, message)
            .with_code("session_push_unsupported"),
        PushError::Unreachable(_) => {
            ApiError::new(StatusCode::BAD_GATEWAY, message).with_code("session_unreachable")
        }
        PushError::Rejected { .. } => {
            ApiError::new(StatusCode::BAD_GATEWAY, message).with_code("session_rejected")
        }
    }
}
