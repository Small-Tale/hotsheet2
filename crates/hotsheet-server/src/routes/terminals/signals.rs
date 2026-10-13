//! Terminal list/read/write/kill, questions, halts, AI connections, names, and
//! permission resolution.

use crate::api_error::ApiError;
use crate::events::ChangeEvent;
use crate::routes::terminals::{
    TerminalAiConnection, TerminalHalt, TerminalHookSource, TerminalInfo, TerminalQuestion,
    broker_err, broker_info, term_info, term_key,
};
use crate::{AppState, terminal_names};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::Settings;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// `GET /terminals` — the live terminals (id, alive, busy).
pub(crate) async fn list_terminals(State(state): State<AppState>) -> Json<Vec<TerminalInfo>> {
    Json(with_terminal_names(
        &state,
        live_terminal_infos(&state).await,
    ))
}

/// Every live terminal this server reports, before saved names are applied.
pub(crate) async fn live_terminal_infos(state: &AppState) -> Vec<TerminalInfo> {
    if let Some(tb) = &state.terminal_broker {
        if let Ok(hotsheet_terminals::BrokerResponse::List { terminals }) =
            tb.call(hotsheet_terminals::BrokerRequest::List).await
        {
            return terminals.into_iter().map(broker_info).collect();
        }
        return Vec::new();
    }
    state
        .terminals
        .list()
        .into_iter()
        .filter_map(|key| state.terminals.get(&key).map(|t| term_info(&t, &key.1)))
        .collect()
}

/// Apply the project's saved terminal names (HS2-89FPV1). An unreadable settings file leaves
/// every terminal unnamed rather than failing the terminal list.
fn with_terminal_names(state: &AppState, mut infos: Vec<TerminalInfo>) -> Vec<TerminalInfo> {
    let names = terminal_names::all(&Settings::new(state.store.root())).unwrap_or_default();
    let halts = state.terminal_halts.lock_or_recover().clone();
    let mut questions = state.terminal_questions.lock_or_recover();
    let connections = state.terminal_ai_connections.lock_or_recover().clone();
    let mut last_reports = state.terminal_ai_last_reports.lock_or_recover();
    for info in &mut infos {
        if !info.alive {
            last_reports.remove(&info.id);
        }
        info.name = names.get(&info.id).cloned();
        info.halt = halts.get(&info.id).cloned();
        info.question = if info.alive {
            questions.get(&info.id).cloned()
        } else {
            questions.remove(&info.id);
            None
        };
        info.ai_connection = info
            .alive
            .then(|| connections.get(&info.id).cloned())
            .flatten();
        info.last_hook_report = last_reports.get(&info.id).cloned();
    }
    infos
}

#[derive(Deserialize)]
pub(crate) struct TerminalQuestionReq {
    question: String,
    tool_use_id: String,
    session_id: Option<String>,
    #[serde(default)]
    questions: serde_json::Value,
}

/// A trusted Claude hook reports the question before the terminal waits for its answer.
pub(crate) async fn ask_terminal_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalQuestionReq>,
) -> Result<StatusCode, ApiError> {
    if !live_terminal_infos(&state)
        .await
        .iter()
        .any(|info| info.id == id)
    {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "no such terminal"));
    }
    if body.tool_use_id.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "missing tool_use_id",
        ));
    }
    if state
        .terminal_ai_connections
        .lock_or_recover()
        .get(&id)
        .is_some_and(|connection| {
            connection.session_id.is_some() && connection.session_id != body.session_id
        })
    {
        return Ok(StatusCode::NO_CONTENT);
    }
    if forget_terminal_halt(&state, &id) {
        emit_terminal_halted(&state, &id, None);
    }
    let mut questions = state.terminal_questions.lock_or_recover();
    let same = questions.get(&id).is_some_and(|current| {
        current.tool_use_id == body.tool_use_id && current.session_id == body.session_id
    });
    if !same {
        questions.insert(
            id.clone(),
            TerminalQuestion {
                question: body.question.trim().chars().take(500).collect(),
                tool_use_id: body.tool_use_id,
                questions: body.questions,
                session_id: body.session_id,
                answer: None,
                at: OffsetDateTime::now_utc()
                    .format(&Rfc3339)
                    .unwrap_or_default(),
            },
        );
    }
    drop(questions);
    if !same {
        emit_terminal_question(&state, &id);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(crate) struct TerminalQuestionIdentity {
    tool_use_id: String,
    session_id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct TerminalQuestionAnswerReq {
    tool_use_id: String,
    at: String,
    #[serde(default)]
    answers: serde_json::Value,
    #[serde(default)]
    native: bool,
}

/// The browser may answer only the exact live tool use in the exact Claude session. A stale
/// notice cannot deliver text to a replacement question or a replacement terminal session.
pub(crate) async fn answer_terminal_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalQuestionAnswerReq>,
) -> Result<StatusCode, ApiError> {
    let mut questions = state.terminal_questions.lock_or_recover();
    let current = questions
        .get_mut(&id)
        .filter(|current| current.tool_use_id == body.tool_use_id && current.at == body.at)
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "question is no longer active"))?;
    if current.answer.is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "question already answered",
        ));
    }
    if body.native {
        questions.remove(&id);
        drop(questions);
        emit_terminal_question(&state, &id);
        return Ok(StatusCode::NO_CONTENT);
    }
    let expected = current
        .questions
        .as_array()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "question options unavailable"))?;
    let answers = body
        .answers
        .as_object()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "answers must be an object"))?;
    if expected.is_empty()
        || expected.len() > 4
        || answers.len() != expected.len()
        || expected.iter().any(|question| {
            question
                .get("question")
                .and_then(serde_json::Value::as_str)
                .and_then(|text| answers.get(text))
                .and_then(serde_json::Value::as_str)
                .is_none_or(|answer| answer.trim().is_empty() || answer.len() > 2000)
        })
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "incomplete question answers",
        ));
    }
    current.answer = Some(body.answers);
    Ok(StatusCode::NO_CONTENT)
}

/// A short HTTP poll keeps the PreToolUse hook responsive to server loss without exposing the
/// answer in the terminal snapshot. Repeated polls return the same answer until resolution.
pub(crate) async fn poll_terminal_question_answer(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TerminalQuestionIdentity>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let questions = state.terminal_questions.lock_or_recover();
    let current = questions
        .get(&id)
        .filter(|current| {
            current.tool_use_id == query.tool_use_id && current.session_id == query.session_id
        })
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "question is no longer active"))?;
    if let Some(answer) = &current.answer {
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "answers": answer })),
        ));
    }
    Ok((StatusCode::NO_CONTENT, Json(serde_json::Value::Null)))
}

#[derive(Default, Deserialize)]
pub(crate) struct TerminalQuestionClearQuery {
    tool_use_id: Option<String>,
    session_id: Option<String>,
}

pub(crate) async fn resolve_terminal_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TerminalQuestionClearQuery>,
) -> StatusCode {
    if forget_terminal_question_for_session(
        &state,
        &id,
        query.tool_use_id.as_deref(),
        query.session_id.as_deref(),
    ) {
        emit_terminal_question(&state, &id);
    }
    StatusCode::NO_CONTENT
}

fn forget_terminal_question(state: &AppState, id: &str, tool_use_id: Option<&str>) -> bool {
    forget_terminal_question_for_session(state, id, tool_use_id, None)
}

fn forget_terminal_question_for_session(
    state: &AppState,
    id: &str,
    tool_use_id: Option<&str>,
    session_id: Option<&str>,
) -> bool {
    let mut questions = state.terminal_questions.lock_or_recover();
    if tool_use_id.is_some_and(|expected| {
        questions
            .get(id)
            .is_none_or(|current| current.tool_use_id != expected)
    }) {
        return false;
    }
    if session_id.is_some_and(|expected| {
        questions
            .get(id)
            .is_none_or(|current| current.session_id.as_deref() != Some(expected))
    }) {
        return false;
    }
    questions.remove(id).is_some()
}

fn emit_terminal_question(state: &AppState, id: &str) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_question".into(),
        id: id.to_owned(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// Body for `POST /terminals/{id}/halt`, sent by the AI tool's hook adapter in that terminal.
#[derive(Deserialize)]
pub(crate) struct TerminalHaltReq {
    #[serde(default)]
    error_type: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    agent: Option<String>,
}

/// `POST /terminals/{id}/halt` — record that the terminal's AI session stopped on an API error
/// (Claude Code's `StopFailure` hook, HS2-HJ4D1H) and announce it with a `terminal_halted` change
/// event (`message` = the error message) so every client marks the tab. Repeating the same report
/// is not re-announced.
pub(crate) async fn halt_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalHaltReq>,
) -> Result<StatusCode, ApiError> {
    if !live_terminal_infos(&state)
        .await
        .iter()
        .any(|info| info.id == id)
    {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "no such terminal"));
    }
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    let clean = |value: Option<String>, fallback: &str| {
        let value = value.unwrap_or_default();
        let value = value.trim();
        let value = if value.is_empty() { fallback } else { value };
        value.chars().take(500).collect::<String>()
    };
    let halt = TerminalHalt {
        error_type: clean(body.error_type, "unknown"),
        message: clean(body.message, "The AI session stopped on an error."),
        agent: body
            .agent
            .map(|agent| agent.trim().to_owned())
            .filter(|agent| !agent.is_empty()),
        at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default(),
    };
    let changed = {
        let mut halts = state.terminal_halts.lock_or_recover();
        let same = halts.get(&id).is_some_and(|current| {
            current.error_type == halt.error_type && current.message == halt.message
        });
        // Identical reports belong to the same active episode. Clients dedupe prompts by `at`.
        if !same {
            halts.insert(id.clone(), halt.clone());
        }
        !same
    };
    if changed {
        emit_terminal_halted(&state, &id, Some(halt.message));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Default, Deserialize)]
pub(crate) struct TerminalHaltClearQuery {
    /// A manual clear applies only to the episode the user saw; hooks omit this to clear on resume.
    at: Option<String>,
}

/// `DELETE /terminals/{id}/halt` — the session resumed or the user cleared its stopped state.
/// An optional `at` makes a manual clear safe if a newer failure arrived meanwhile.
pub(crate) async fn clear_terminal_halt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TerminalHaltClearQuery>,
) -> StatusCode {
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    if forget_terminal_halt_if(&state, &id, query.at.as_deref()) {
        emit_terminal_halted(&state, &id, None);
    }
    StatusCode::NO_CONTENT
}

fn forget_terminal_halt(state: &AppState, id: &str) -> bool {
    forget_terminal_halt_if(state, id, None)
}

fn forget_terminal_halt_if(state: &AppState, id: &str, at: Option<&str>) -> bool {
    let mut halts = state.terminal_halts.lock_or_recover();
    if at.is_some_and(|expected| halts.get(id).is_none_or(|halt| halt.at != expected)) {
        return false;
    }
    halts.remove(id).is_some()
}

/// Body for `POST /terminals/{id}/ai-connection`, sent by the AI tool's `SessionStart` hook and
/// reused by an authenticated interactive permission ask.
#[derive(Deserialize)]
pub(crate) struct TerminalAiConnectionReq {
    #[serde(default)]
    pub(crate) agent: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) source: TerminalHookSource,
}

/// `POST /terminals/{id}/ai-connection` — an AI session in the terminal started with Hot Sheet's
/// hooks active (HS2-EV1XK3), announced with a `terminal_ai_connection` change event (`message` =
/// the agent) so every client shows the tab as connected. Repeating the same agent is not
/// re-announced.
pub(crate) async fn connect_terminal_ai(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalAiConnectionReq>,
) -> Result<StatusCode, ApiError> {
    if !live_terminal_infos(&state)
        .await
        .iter()
        .any(|info| info.id == id)
    {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "no such terminal"));
    }
    // A new or resumed AI session in this terminal cannot inherit an earlier session's halt.
    if forget_terminal_halt(&state, &id) {
        emit_terminal_halted(&state, &id, None);
    }
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    let agent = body
        .agent
        .map(|agent| agent.trim().chars().take(64).collect::<String>())
        .filter(|agent| !agent.is_empty());
    let connection = TerminalAiConnection {
        agent: agent.clone(),
        at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default(),
        session_id: body.session_id.filter(|id| !id.is_empty()),
        source: body.source,
    };
    let started_from_shell = terminal_foreground_command(&state, &id).await == Some(true);
    let reported_at = connection.at.clone();
    state
        .terminal_ai_last_reports
        .lock_or_recover()
        .insert(id.clone(), connection.clone());
    let changed = state
        .terminal_ai_connections
        .lock_or_recover()
        .insert(id.clone(), connection)
        .is_none_or(|previous| previous.agent != agent);
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_hook_report".into(),
        id: id.clone(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
    if changed {
        emit_terminal_ai_connection(&state, &id, Some(agent.unwrap_or_default()));
    }
    if started_from_shell {
        watch_shell_ai_session_exit(&state, id, reported_at);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Whether a command the user started from the terminal's shell holds its foreground; `None`
/// when the terminal is gone or the platform cannot tell.
async fn terminal_foreground_command(state: &AppState, id: &str) -> Option<bool> {
    if let Some(tb) = &state.terminal_broker {
        return match tb
            .call(hotsheet_terminals::BrokerRequest::Read { id: id.to_owned() })
            .await
        {
            Ok(hotsheet_terminals::BrokerResponse::Read { info, .. }) if info.alive => {
                info.foreground_command
            }
            _ => None,
        };
    }
    let term = state.terminals.get(&term_key(state, id))?;
    term.is_alive()
        .then(|| term.foreground_command_running())
        .flatten()
}

/// An AI session started from the terminal's shell has ended once the shell takes its
/// foreground back, even when its SessionEnd hook never ran (a crash, a killed process, or a
/// failed hook). Clear that connection then, so the tab stops claiming a live session
/// (HS2-Z1W068). The watch ends as soon as a newer report replaces this one or it is cleared.
fn watch_shell_ai_session_exit(state: &AppState, id: String, reported_at: String) {
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let current = || {
                state
                    .terminal_ai_connections
                    .lock_or_recover()
                    .get(&id)
                    .is_some_and(|connection| connection.at == reported_at)
            };
            if !current() {
                return;
            }
            match terminal_foreground_command(&state, &id).await {
                Some(true) => continue,
                None => return,
                Some(false) => {}
            }
            let removed = {
                let mut connections = state.terminal_ai_connections.lock_or_recover();
                let matches = connections
                    .get(&id)
                    .is_some_and(|connection| connection.at == reported_at);
                matches && connections.remove(&id).is_some()
            };
            if removed {
                if forget_terminal_halt(&state, &id) {
                    emit_terminal_halted(&state, &id, None);
                }
                if forget_terminal_question(&state, &id, None) {
                    emit_terminal_question(&state, &id);
                }
                emit_terminal_ai_connection(&state, &id, None);
            }
            return;
        }
    });
}

/// `DELETE /terminals/{id}/ai-connection` — the terminal's AI session ended (`SessionEnd`). A
/// terminal that was not connected is a no-op without an event.
pub(crate) async fn disconnect_terminal_ai(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TerminalAiConnectionEndQuery>,
) -> StatusCode {
    let Some(removed) = forget_terminal_ai_connection_if(&state, &id, query.session_id.as_deref())
    else {
        return StatusCode::NO_CONTENT;
    };
    if forget_terminal_halt(&state, &id) {
        emit_terminal_halted(&state, &id, None);
    }
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    if removed {
        emit_terminal_ai_connection(&state, &id, None);
    }
    StatusCode::NO_CONTENT
}

#[derive(Default, Deserialize)]
pub(crate) struct TerminalAiConnectionEndQuery {
    session_id: Option<String>,
}

fn forget_terminal_ai_connection(state: &AppState, id: &str) -> bool {
    forget_terminal_ai_connection_if(state, id, None).unwrap_or(false)
}

/// `None` means this end belongs to a different session; `Some(false)` means no report exists.
fn forget_terminal_ai_connection_if(
    state: &AppState,
    id: &str,
    session_id: Option<&str>,
) -> Option<bool> {
    let mut connections = state.terminal_ai_connections.lock_or_recover();
    if session_id.is_some_and(|expected| {
        connections
            .get(id)
            .and_then(|connection| connection.session_id.as_deref())
            != Some(expected)
    }) {
        return None;
    }
    Some(connections.remove(id).is_some())
}

fn emit_terminal_ai_connection(state: &AppState, id: &str, message: Option<String>) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_ai_connection".into(),
        id: id.to_owned(),
        slug: String::new(),
        message,
        activity: None,
        assignment: None,
        turn: None,
    });
}

fn emit_terminal_halted(state: &AppState, id: &str, message: Option<String>) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_halted".into(),
        id: id.to_owned(),
        slug: String::new(),
        message,
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// Body for `PUT /terminals/{id}/name`; an absent, null, or blank name clears the rename.
#[derive(Deserialize)]
pub(crate) struct TerminalNameReq {
    #[serde(default)]
    name: Option<String>,
}

/// Response for `PUT /terminals/{id}/name`.
#[derive(Serialize)]
pub(crate) struct TerminalNameResp {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

/// `PUT /terminals/{id}/name` `{name}` — save (or clear) a live terminal's tab name in the
/// project's machine-local settings and announce it with a `terminal_renamed` change event
/// (`id` = terminal id, `message` = the new name, absent when cleared) so every connected
/// client retitles the tab without a reload (HS2-89FPV1).
pub(crate) async fn rename_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalNameReq>,
) -> Result<Json<TerminalNameResp>, ApiError> {
    let name = terminal_names::normalize(body.name.as_deref())
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, message))?;
    if !live_terminal_infos(&state)
        .await
        .iter()
        .any(|info| info.id == id)
    {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "no such terminal"));
    }
    let changed = terminal_names::set(&Settings::new(state.store.root()), &id, name.as_deref())
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if changed {
        emit_terminal_renamed(&state, &id, name.clone());
    }
    Ok(Json(TerminalNameResp { id, name }))
}

fn emit_terminal_renamed(state: &AppState, id: &str, name: Option<String>) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_renamed".into(),
        id: id.to_owned(),
        slug: String::new(),
        message: name,
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// A killed terminal's id can be reused by a later terminal, which must not inherit the old
/// tab name.
fn forget_terminal_name(state: &AppState, id: &str) {
    if let Err(error) = terminal_names::set(&Settings::new(state.store.root()), id, None) {
        tracing::warn!(terminal = id, %error, "forgetting terminal name failed");
    }
}

/// A terminal's current scrollback + state (`GET /terminals/{id}`). The scrollback is what a
/// re-attaching viewer replays; it's returned as lossy UTF-8 text.
#[derive(Serialize)]
pub(crate) struct TerminalRead {
    #[serde(flatten)]
    pub(crate) info: TerminalInfo,
    scrollback: String,
}

pub(crate) fn named_terminal_info(state: &AppState, info: TerminalInfo) -> TerminalInfo {
    with_terminal_names(state, vec![info])
        .pop()
        .expect("one terminal in, one out")
}

pub(crate) async fn read_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TerminalRead>, ApiError> {
    if let Some(tb) = &state.terminal_broker {
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        return match resp {
            hotsheet_terminals::BrokerResponse::Read { info, scrollback } => {
                Ok(Json(TerminalRead {
                    info: named_terminal_info(&state, broker_info(info)),
                    scrollback: String::from_utf8_lossy(&scrollback).into_owned(),
                }))
            }
            other => Err(broker_err(other)),
        };
    }
    let term = state
        .terminals
        .get(&term_key(&state, &id))
        .ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json(TerminalRead {
        info: named_terminal_info(&state, term_info(&term, &id)),
        scrollback: String::from_utf8_lossy(&term.scrollback()).into_owned(),
    }))
}

/// Body for `POST /terminals/{id}/input`.
#[derive(Deserialize)]
pub(crate) struct TerminalInput {
    /// Bytes to write to the PTY (as text — includes any control chars like `\n`).
    data: String,
}

pub(crate) async fn write_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalInput>,
) -> Result<StatusCode, ApiError> {
    if let Some(tb) = &state.terminal_broker {
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Input {
                id: id.clone(),
                data: body.data.into_bytes(),
            })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        return match resp {
            hotsheet_terminals::BrokerResponse::Ok => Ok(StatusCode::NO_CONTENT),
            other => Err(broker_err(other)),
        };
    }
    let term = state
        .terminals
        .get(&term_key(&state, &id))
        .ok_or_else(|| ApiError::not_found(&id))?;
    term.write(body.data.as_bytes())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn kill_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if let Some(tb) = &state.terminal_broker {
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Kill { id: id.clone() })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        return match resp {
            hotsheet_terminals::BrokerResponse::Ok
            | hotsheet_terminals::BrokerResponse::NotFound => {
                forget_terminal_name(&state, &id);
                if forget_terminal_halt(&state, &id) {
                    emit_terminal_halted(&state, &id, None);
                }
                if forget_terminal_question(&state, &id, None) {
                    emit_terminal_question(&state, &id);
                }
                forget_terminal_ai_connection(&state, &id);
                state.terminal_ai_last_reports.lock_or_recover().remove(&id);
                Ok(StatusCode::NO_CONTENT)
            }
            other => Err(broker_err(other)),
        };
    }
    state
        .terminals
        .kill(&term_key(&state, &id))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    forget_terminal_name(&state, &id);
    if forget_terminal_halt(&state, &id) {
        emit_terminal_halted(&state, &id, None);
    }
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    forget_terminal_ai_connection(&state, &id);
    state.terminal_ai_last_reports.lock_or_recover().remove(&id);
    Ok(StatusCode::NO_CONTENT)
}

/// Body for `POST /permissions/{id}`: the human's answer.
#[derive(Deserialize)]
pub(crate) struct PermissionAnswer {
    decision: hotsheet_aitools::PermissionDecision,
    /// `once` | `session` | `always` (default `once`).
    #[serde(default = "default_scope")]
    scope: hotsheet_aitools::PermissionScope,
}

fn default_scope() -> hotsheet_aitools::PermissionScope {
    hotsheet_aitools::PermissionScope::Once
}

/// The ack a resolve returns: who it was routed back to + the decision applied.
#[derive(Serialize)]
pub(crate) struct PermissionResolved {
    connection: String,
    decision: hotsheet_aitools::PermissionDecision,
    /// Whether an `Always` rule was persisted durably.
    persisted: bool,
}

/// `POST /permissions/{id}` `{decision, scope}` — answer a pending request. Wakes the
/// blocked tool, and on `always` persists the rule so the answer survives a restart. 404
/// if the id isn't pending (already answered / never existed).
pub(crate) async fn resolve_permission(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(body): Json<PermissionAnswer>,
) -> Result<Json<PermissionResolved>, ApiError> {
    let event_decision = match body.decision {
        hotsheet_aitools::PermissionDecision::Allow => "allow",
        hotsheet_aitools::PermissionDecision::Deny => "deny",
    };
    let event_scope = match body.scope {
        hotsheet_aitools::PermissionScope::Once => "once",
        hotsheet_aitools::PermissionScope::Session => "session",
        hotsheet_aitools::PermissionScope::Always => "always",
    };
    let resolved = state
        .permissions
        .resolve(id, body.decision, body.scope)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no pending request {id}")))?;
    // Persist an `Always` rule so the remembered answer survives a restart.
    let mut persisted = false;
    let rules_path = state
        .permission_rule_paths
        .lock_or_recover()
        .get(&resolved.project)
        .cloned();
    if let (Some(rule), Some(path)) = (&resolved.persisted_rule, rules_path.as_ref()) {
        match hotsheet_aitools::append_permission_rule(path, rule) {
            Ok(()) => persisted = true,
            Err(e) => tracing::warn!(error = %e, "failed to persist permission rule"),
        }
    }
    // Resolution can happen through another client or transport. Publish a replayable
    // nudge so every notification inbox reconciles the now-absent request into history.
    state.emit(ChangeEvent {
        cursor: None,
        store: resolved.project.clone(),
        kind: "permission_resolved".to_string(),
        id: id.to_string(),
        slug: String::new(),
        message: Some(format!("{event_decision}:{event_scope}")),
        activity: None,
        assignment: None,
        turn: None,
    });
    Ok(Json(PermissionResolved {
        connection: resolved.connection,
        decision: resolved.decision,
        persisted,
    }))
}
