//! Terminal routes and the live attach socket (HS2-A6R5QV): settings and the shared
//! terminal info types; the launch, signal, and attach routes live in submodules
//! (HS2-YMG48C).

mod attach;
pub(crate) mod launch;
mod signals;

pub(crate) use attach::attach_terminal;
pub(crate) use launch::{
    INHERIT_GLOBAL_SHELL_HISTORY_SETTING, open_terminal, release_session_claims,
    release_session_claims_blocking,
};
pub use launch::{
    prune_orphaned_terminal_names, release_orphaned_drive_sessions, resume_broker_terminal_sessions,
};
pub(crate) use signals::{
    TerminalAiConnectionReq, answer_terminal_question, ask_terminal_question, clear_terminal_halt,
    connect_terminal_ai, disconnect_terminal_ai, emit_permission_rules_changed, halt_terminal,
    kill_terminal, list_permission_rules, list_terminals, poll_terminal_question_answer,
    read_terminal, rename_terminal, resolve_permission, resolve_terminal_question,
    revoke_permission_rule, write_terminal,
};

use crate::AppState;
use crate::api_error::ApiError;
use crate::routes::project_tools::checkout_settings;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_ticketing::Settings;
use serde::{Deserialize, Serialize};

// ---- terminals (HS2-A6R5QV) ------------------------------------------------------

/// Body for `POST /terminals`: what to run.
#[derive(Deserialize)]
pub(crate) struct OpenTerminalReq {
    /// The program to spawn (e.g. `bash`, `codex`).
    pub(crate) command: Option<String>,
    /// Command text for the user's default shell to execute.
    pub(crate) shell_command: Option<String>,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    /// Working directory (defaults to the served store root).
    pub(crate) cwd: Option<String>,
    /// Client-chosen terminal id; a ULID is minted when omitted.
    pub(crate) id: Option<String>,
    /// When set, register this terminal as a live AI-tool **connection** for that plugin id
    /// (e.g. `claude`), so `GET /connections` shows it and its busy is fed from the terminal's
    /// OSC-133 / spinner inference (HS2-4M67VN). Omit for a plain shell terminal.
    #[serde(default)]
    pub(crate) connect: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) effort: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct TerminalSettings {
    #[serde(default)]
    inherit_global_shell_history: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct TrashSettings {
    trash_cleanup_days: u32,
}

pub(crate) fn read_terminal_settings(state: &AppState) -> Result<TerminalSettings, ApiError> {
    read_terminal_settings_from(&Settings::new(state.store.root()))
}

fn read_terminal_settings_from(settings: &Settings) -> Result<TerminalSettings, ApiError> {
    let inherit_global_shell_history = settings
        .get(
            INHERIT_GLOBAL_SHELL_HISTORY_SETTING,
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    Ok(TerminalSettings {
        inherit_global_shell_history,
    })
}

pub(crate) async fn get_terminal_settings(
    State(state): State<AppState>,
) -> Result<Json<TerminalSettings>, ApiError> {
    Ok(Json(read_terminal_settings(&state)?))
}

pub(crate) async fn put_terminal_settings(
    State(state): State<AppState>,
    Json(value): Json<TerminalSettings>,
) -> Result<Json<TerminalSettings>, ApiError> {
    Settings::new(state.store.root())
        .set(
            INHERIT_GLOBAL_SHELL_HISTORY_SETTING,
            serde_json::Value::Bool(value.inherit_global_shell_history),
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(value))
}

pub(crate) async fn get_checkout_terminal_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<TerminalSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    Ok(Json(read_terminal_settings_from(&settings)?))
}

pub(crate) async fn put_checkout_terminal_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(value): Json<TerminalSettings>,
) -> Result<Json<TerminalSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            INHERIT_GLOBAL_SHELL_HISTORY_SETTING,
            serde_json::Value::Bool(value.inherit_global_shell_history),
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(value))
}

pub(crate) async fn get_checkout_trash_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<TrashSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let trash_cleanup_days = settings
        .trash_cleanup_days()
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(TrashSettings { trash_cleanup_days }))
}

pub(crate) async fn put_checkout_trash_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(value): Json<TrashSettings>,
) -> Result<Json<TrashSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            hotsheet_ticketing::TRASH_CLEANUP_DAYS_SETTING,
            serde_json::json!(value.trash_cleanup_days),
            hotsheet_ticketing::Scope::Shared,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    // "Apply now" (HS2-KPD942): a shorter retention purges what it newly covers immediately.
    crate::routes::checkout_tickets::purge_checkout_trash_past(
        &state,
        &reference,
        value.trash_cleanup_days,
    )?;
    Ok(Json(value))
}

/// One terminal as reported by `GET /terminals`.
#[derive(Serialize)]
pub(crate) struct TerminalInfo {
    id: String,
    /// Immutable creation kind; never inferred from terminal output or command names.
    kind: hotsheet_terminals::TerminalKind,
    /// The PTY is still running.
    pub(crate) alive: bool,
    /// Inferred busy (a tool is actively working) vs idle.
    busy: bool,
    /// The shell's reported working directory (OSC 7), if any (HS2-RCKEJ9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cwd: Option<String>,
    /// A currently-open hyperlink URI (OSC 8), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<String>,
    /// Tool progress percent 0-100 (OSC 9;4), if reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<u8>,
    /// The AI tool an `ai` terminal launched (for example `claude`), from its immutable
    /// `<tool>-<id>` worker id; clients name AI tabs after it (HS2-HZK0NK).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    /// The user's saved tab name (HS2-89FPV1), absent when the terminal was never renamed;
    /// clients then derive a default name.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// The terminal's AI session halted on an API error and is waiting for the user
    /// (HS2-HJ4D1H); absent while it is running normally.
    #[serde(skip_serializing_if = "Option::is_none")]
    halt: Option<TerminalHalt>,
    /// An interactive question from the AI session awaiting an answer in this terminal.
    #[serde(skip_serializing_if = "Option::is_none")]
    question: Option<TerminalQuestion>,
    /// An AI session in this terminal started with Hot Sheet's hooks active, so its permission
    /// prompts come to Hot Sheet (HS2-EV1XK3); absent when no session has reported in.
    #[serde(skip_serializing_if = "Option::is_none")]
    ai_connection: Option<TerminalAiConnection>,
    /// Most recent trusted hook report, even if that session later ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_hook_report: Option<TerminalAiConnection>,
    /// A live `hotsheet-mcp` lease names this terminal (HS2-HNXE38). Separate from
    /// `ai_connection`, which means permission prompts are routed to Hot Sheet. Always
    /// serialized, so a client can tell `false` from a server that predates the field
    /// (HS2-670ADP).
    pub(crate) mcp_connected: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalHookSource {
    #[default]
    SessionStart,
    PermissionRequest,
}

/// An AI session whose `SessionStart` or interactive permission hook reported in from a terminal
/// (HS2-EV1XK3, HS2-XYSXVT). A tool runs a project hook only once it is installed and trusted,
/// so this proves the hook is live.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalAiConnection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    /// RFC 3339 time the session reported in.
    #[serde(default)]
    at: String,
    #[serde(default)]
    source: TerminalHookSource,
    /// Internal lifecycle identity; the terminal API need not expose Codex/Claude session ids.
    #[serde(skip)]
    session_id: Option<String>,
}

/// Why a terminal's AI session stopped: the tool's error category and message (for example
/// Claude Code's `StopFailure` `overloaded` / "Selected model is at capacity").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalHalt {
    error_type: String,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    /// RFC 3339 time the halt was reported.
    #[serde(default)]
    at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalQuestion {
    question: String,
    tool_use_id: String,
    #[serde(default)]
    questions: serde_json::Value,
    #[serde(skip)]
    session_id: Option<String>,
    #[serde(skip)]
    answer: Option<serde_json::Value>,
    at: String,
}

/// The AI tool of an `ai` terminal, recovered from its `<tool>-<id>` worker id. Shell terminals,
/// workers that do not follow that shape, and an explicit-command terminal that also connects a
/// tool (whose session worker is in the reserved `terminal` namespace) have none.
pub(crate) fn ai_terminal_tool(
    kind: hotsheet_terminals::TerminalKind,
    worker: Option<&str>,
    id: &str,
) -> Option<String> {
    if kind != hotsheet_terminals::TerminalKind::Ai {
        return None;
    }
    worker?
        .strip_suffix(&format!("-{id}"))
        .filter(|tool| !tool.is_empty() && *tool != "terminal")
        .map(str::to_string)
}

/// The terminal-manager key for a terminal id — the served store root is the project.
pub(crate) fn term_key(state: &AppState, id: &str) -> hotsheet_terminals::TermKey {
    (state.store.root().display().to_string(), id.to_string())
}

pub(crate) fn term_info(term: &hotsheet_terminals::Terminal, id: &str) -> TerminalInfo {
    let osc = term.term_state();
    TerminalInfo {
        id: id.to_string(),
        kind: term.kind(),
        alive: term.is_alive(),
        busy: term.activity() == hotsheet_terminals::Activity::Busy,
        cwd: osc.cwd,
        link: osc.link,
        progress: osc.progress,
        tool: ai_terminal_tool(term.kind(), term.worker_id(), id),
        name: None,
        halt: None,
        question: None,
        ai_connection: None,
        last_hook_report: None,
        mcp_connected: false,
    }
}

/// Map a broker terminal-info onto the HTTP `TerminalInfo`.
pub(crate) fn broker_info(bi: hotsheet_terminals::BrokerTermInfo) -> TerminalInfo {
    let tool = ai_terminal_tool(bi.kind, bi.worker.as_deref(), &bi.id);
    TerminalInfo {
        id: bi.id,
        kind: bi.kind,
        alive: bi.alive,
        busy: bi.busy,
        cwd: bi.cwd,
        link: bi.link,
        progress: bi.progress,
        tool,
        name: None,
        halt: None,
        question: None,
        ai_connection: None,
        last_hook_report: None,
        mcp_connected: false,
    }
}

/// Map a non-success broker response to an `ApiError`.
pub(crate) fn broker_err(resp: hotsheet_terminals::BrokerResponse) -> ApiError {
    use hotsheet_terminals::BrokerResponse as R;
    match resp {
        R::Err { message } => ApiError::new(StatusCode::BAD_REQUEST, message),
        R::NotFound => ApiError::new(StatusCode::NOT_FOUND, "no such terminal"),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unexpected broker response: {other:?}"),
        ),
    }
}
