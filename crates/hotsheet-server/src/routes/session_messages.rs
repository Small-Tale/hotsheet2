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
    self, PushError, PushLaunch, PushMessage, PushReceipt, SessionPush, UreqHttp,
};
use hotsheet_sync::LockExt;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api_error::ApiError;

/// Live session-push adapters keyed by terminal (= connection) id.
pub(crate) type SessionPushes = Arc<Mutex<HashMap<String, Arc<dyn SessionPush>>>>;

/// The connection action a push-capable session advertises in `GET /connections`.
pub(crate) const MESSAGE_ACTION: &str = "message";
const CODEX_BOOTSTRAP: &str = "codex-pty-bootstrap";

/// Only a newly spawned, Hot Sheet-owned Codex TUI gets this one-shot address. The
/// first prompt starts Codex's SessionStart hook, which replaces this adapter with
/// the thread-specific queue adapter. No daemon-wide cwd lookup is involved.
///
/// HS2-4R5MNJ: a paste typed while Codex still shows a first-run dialog (update notice,
/// folder trust, hooks review) can be lost, and Codex fires no hook before its first
/// prompt. So the prompt is pasted only once the TUI's output shows its composer
/// ([`codex_composer_shown`]). Until then the adapter keeps it (`Held`) and pastes it
/// itself, exactly once, when the composer appears; past [`BOOTSTRAP_HOLD`] it gives up
/// and pastes nothing into whatever is on screen.
struct CodexBootstrapPush {
    inner: Mutex<Bootstrap>,
    write: Box<dyn Fn(Vec<u8>) -> Result<(), PushError> + Send + Sync>,
    now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
}

/// How long a held bootstrap prompt waits for the composer: the orchestrator's own role
/// prompt window, after which it reads as failed.
const BOOTSTRAP_HOLD: std::time::Duration =
    std::time::Duration::from_millis(hotsheet_ticketing::orchestration::ROLE_PROMPT_WINDOW_MS);
/// The recent output kept to decide what the TUI shows now.
const COMPOSER_TAIL_CHARS: usize = 16 * 1024;

#[derive(Default)]
struct Bootstrap {
    /// The TUI's recent output, terminal controls removed.
    tail: String,
    /// The prompt waiting for the composer, and when it stops waiting.
    held: Option<(String, u64)>,
    /// The prompt pasted, once.
    pasted: Option<String>,
}

/// Text the Codex TUI shows only in its composer, when it is ready for a prompt.
const COMPOSER_MARKERS: &[&str] = &["Ask Codex to do anything"];
/// Text of screens that take input instead of the composer: the update notice, folder
/// trust, the hooks review, and a composer that is still starting.
const NOT_READY_MARKERS: &[&str] = &[
    "Update available!",
    "Trust this folder?",
    "Hooks need review",
    "Waiting for startup",
];

/// HS2-4R5MNJ: whether the Codex TUI's latest output shows its composer: the composer's
/// placeholder was drawn after the last first-run dialog or start-up notice. Pure, over
/// output with terminal controls already removed.
fn codex_composer_shown(text: &str) -> bool {
    let last = |markers: &[&str]| markers.iter().filter_map(|marker| text.rfind(marker)).max();
    match (last(COMPOSER_MARKERS), last(NOT_READY_MARKERS)) {
        (Some(composer), Some(dialog)) => composer > dialog,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// Printable text of terminal output: escape sequences (CSI, OSC, single-character
/// escapes) and other controls removed, newlines kept.
fn printable(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\x1b' => match chars.next() {
                Some('[') => {
                    // CSI: parameters and intermediates up to a final byte.
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']' | 'P' | '_' | '^') => {
                    // OSC/DCS/APC/PM: up to BEL or ST.
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }
                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\n' => out.push('\n'),
            character if character.is_control() => {}
            character => out.push(character),
        }
    }
    out
}

impl CodexBootstrapPush {
    fn new(
        write: Box<dyn Fn(Vec<u8>) -> Result<(), PushError> + Send + Sync>,
        now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            inner: Mutex::new(Bootstrap::default()),
            write,
            now_ms,
        }
    }

    fn paste(&self, text: &str) -> Result<(), PushError> {
        // Bracketed paste keeps embedded newlines inside one prompt; CR submits it.
        (self.write)(format!("\x1b[200~{text}\x1b[201~\r").into_bytes())
    }

    /// Feed the TUI's output. Returns `true` when this output let a held prompt be pasted,
    /// so the host records the delivery.
    fn observe(&self, bytes: &[u8]) -> bool {
        let mut inner = self.inner.lock_or_recover();
        inner.tail.push_str(&printable(bytes));
        if inner.tail.len() > COMPOSER_TAIL_CHARS {
            let mut cut = inner.tail.len() - COMPOSER_TAIL_CHARS;
            while !inner.tail.is_char_boundary(cut) {
                cut += 1;
            }
            inner.tail.drain(..cut);
        }
        let Some((text, until)) = inner.held.clone() else {
            return false;
        };
        if (self.now_ms)() > until {
            // Never ready within the bound: nothing is pasted into what the TUI shows.
            inner.held = None;
            return false;
        }
        if !codex_composer_shown(&inner.tail) || self.paste(&text).is_err() {
            return false;
        }
        inner.held = None;
        inner.pasted = Some(text);
        true
    }
}

impl SessionPush for CodexBootstrapPush {
    fn mechanism(&self) -> &'static str {
        CODEX_BOOTSTRAP
    }
    fn session_id(&self) -> Option<String> {
        None
    }
    fn delivered_prompt(&self) -> Option<String> {
        self.inner.lock_or_recover().pasted.clone()
    }
    fn message(&self, message: &PushMessage) -> Result<PushReceipt, PushError> {
        let PushMessage::Prompt { text, .. } = message else {
            return Err(PushError::Unsupported(
                "Codex cannot run UI commands".into(),
            ));
        };
        if text.trim().is_empty()
            || text
                .chars()
                .any(|character| character.is_control() && character != '\n' && character != '\t')
        {
            return Err(PushError::Invalid(
                "Codex bootstrap prompt is empty or contains terminal control characters".into(),
            ));
        }
        let receipt = PushReceipt {
            mechanism: CODEX_BOOTSTRAP,
            session_id: None,
            confirmed: false,
        };
        let mut inner = self.inner.lock_or_recover();
        if let Some(pasted) = &inner.pasted {
            // The held prompt was pasted when the composer appeared: that is its delivery.
            return if pasted == text {
                Ok(receipt)
            } else {
                Err(PushError::Unreachable(
                    "waiting for Codex SessionStart after the first prompt".into(),
                ))
            };
        }
        let now = (self.now_ms)();
        if let Some((held, until)) = &inner.held {
            if now > *until {
                inner.held = None;
                return Err(PushError::Unreachable(
                    "Codex did not show its composer in time; nothing was pasted".into(),
                ));
            }
            if held != text {
                return Err(PushError::Unreachable(
                    "waiting for Codex to show its composer for an earlier prompt".into(),
                ));
            }
            return Err(PushError::Held(
                "Codex is not showing its composer yet (a first-run dialog may be open)".into(),
            ));
        }
        if codex_composer_shown(&inner.tail) {
            self.paste(text)?;
            inner.pasted = Some(text.clone());
            return Ok(receipt);
        }
        let until =
            now.saturating_add(u64::try_from(BOOTSTRAP_HOLD.as_millis()).unwrap_or(u64::MAX));
        inner.held = Some((text.clone(), until));
        Err(PushError::Held(
            "Codex is not showing its composer yet (a first-run dialog may be open)".into(),
        ))
    }
}

/// A replacement adapter that answers the prompt its predecessor already delivered as
/// delivered, once, instead of sending it again (HS2-4R5MNJ).
struct AlreadyDelivered {
    inner: Arc<dyn SessionPush>,
    text: Mutex<Option<String>>,
}

impl SessionPush for AlreadyDelivered {
    fn mechanism(&self) -> &'static str {
        self.inner.mechanism()
    }
    fn session_id(&self) -> Option<String> {
        self.inner.session_id()
    }
    fn message(&self, message: &PushMessage) -> Result<PushReceipt, PushError> {
        if let PushMessage::Prompt { text, .. } = message {
            let mut delivered = self.text.lock_or_recover();
            if delivered.as_deref() == Some(text.as_str()) {
                *delivered = None;
                return Ok(PushReceipt {
                    mechanism: CODEX_BOOTSTRAP,
                    session_id: self.inner.session_id(),
                    confirmed: false,
                });
            }
        }
        self.inner.message(message)
    }
}

pub(crate) fn register_codex_bootstrap(state: &AppState, id: &str) {
    let write: Box<dyn Fn(Vec<u8>) -> Result<(), PushError> + Send + Sync> =
        if let Some(broker) = &state.terminal_broker {
            let broker = broker.clone();
            let id = id.to_string();
            let runtime = tokio::runtime::Handle::current();
            Box::new(move |data| {
                let request = hotsheet_terminals::BrokerRequest::Input {
                    id: id.clone(),
                    data,
                };
                // Called only from blocking threads (the output watcher hands each chunk to one).
                match runtime.block_on(broker.call(request)) {
                    Ok(hotsheet_terminals::BrokerResponse::Ok) => Ok(()),
                    Ok(other) => Err(PushError::Unreachable(format!(
                        "terminal broker: {other:?}"
                    ))),
                    Err(error) => Err(PushError::Unreachable(error.to_string())),
                }
            })
        } else {
            let Some(term) = state
                .terminals
                .get(&crate::routes::terminals::term_key(state, id))
            else {
                return;
            };
            Box::new(move |data| {
                term.write(&data)
                    .map_err(|error| PushError::Unreachable(error.to_string()))
            })
        };
    let adapter = Arc::new(CodexBootstrapPush::new(
        write,
        Box::new(crate::routes::permissions::now_ms),
    ));
    register(state, id, adapter.clone());
    watch_codex_output(state, id, adapter);
}

/// HS2-4R5MNJ: feed the new Codex TUI's output to its bootstrap adapter until the
/// prompt is pasted, the hold ends, or the terminal ends. Driven by the output itself:
/// nothing is read on a timer. A paste is recorded through the role-prompt retry, as any
/// other readiness event.
fn watch_codex_output(state: &AppState, id: &str, adapter: Arc<CodexBootstrapPush>) {
    let (state, id) = (state.clone(), id.to_string());
    let deadline = tokio::time::Instant::now() + BOOTSTRAP_HOLD + std::time::Duration::from_secs(5);
    let pasted = move |state: &AppState, id: &str| {
        crate::routes::orchestration_sessions::retry_role(state, id);
    };
    if let Some(broker) = state.terminal_broker.clone() {
        tokio::spawn(async move {
            let Ok(stream) = hotsheet_terminals::BrokerStream::open(&broker.socket, &id).await
            else {
                return;
            };
            let (mut output, _input) = stream.into_split();
            loop {
                let frame = tokio::select! {
                    frame = output.next() => frame,
                    () = tokio::time::sleep_until(deadline) => return,
                };
                let bytes = match frame {
                    Ok(Some(hotsheet_terminals::StreamOut::Scrollback { data, .. }))
                    | Ok(Some(hotsheet_terminals::StreamOut::Output { data })) => data.into_vec(),
                    Ok(Some(hotsheet_terminals::StreamOut::Size { .. })) => continue,
                    _ => return,
                };
                let observer = adapter.clone();
                let Ok(done) = tokio::task::spawn_blocking(move || observer.observe(&bytes)).await
                else {
                    return;
                };
                if done {
                    pasted(&state, &id);
                    return;
                }
                if adapter.delivered_prompt().is_some() {
                    return;
                }
            }
        });
        return;
    }
    let Some(term) = state
        .terminals
        .get(&crate::routes::terminals::term_key(&state, &id))
    else {
        return;
    };
    let (scrollback, mut output) = term.subscribe_with_scrollback();
    tokio::spawn(async move {
        let observe = |bytes: Vec<u8>| {
            let observer = adapter.clone();
            async move {
                tokio::task::spawn_blocking(move || observer.observe(&bytes))
                    .await
                    .unwrap_or(false)
            }
        };
        if observe(scrollback).await {
            pasted(&state, &id);
            return;
        }
        loop {
            let bytes = tokio::select! {
                bytes = output.recv() => bytes,
                () = tokio::time::sleep_until(deadline) => return,
            };
            match bytes {
                Ok(bytes) => {
                    if observe(bytes).await {
                        pasted(&state, &id);
                        return;
                    }
                    if adapter.delivered_prompt().is_some() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

/// A launch prepared for session push: extra launch args/env plus the adapter to register
/// once the terminal is actually spawned (never on a reattach).
pub(crate) struct PreparedSessionPush {
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) adapter: Arc<dyn SessionPush>,
    pub(crate) broker_state: hotsheet_terminals::BrokerPushState,
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
    let directory = hotsheet_sync::paths::canonicalize(directory)
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
    let broker_state = hotsheet_terminals::BrokerPushState {
        mechanism: spec.mechanism.clone(),
        port: launch.port,
        password: launch.password.clone(),
        directory: launch.directory.clone(),
        launched_at_ms: launch.launched_at_ms,
        model: launch.model.clone(),
        session_id: None,
    };
    let Some(adapter) = session_push::adapter_for(spec, launch, Arc::new(UreqHttp::default()))
    else {
        return Ok(None);
    };
    Ok(Some(PreparedSessionPush {
        args: wiring.args,
        env: wiring.env,
        adapter,
        broker_state,
    }))
}

/// Restore a launch-wired adapter from state retained beside its live broker PTY.
pub(crate) fn restore_broker_push(
    state: &AppState,
    id: &str,
    push: hotsheet_terminals::BrokerPushState,
) {
    if push.mechanism != session_push::OPENCODE_HTTP {
        return;
    }
    let launch = PushLaunch {
        port: push.port,
        password: push.password,
        directory: push.directory,
        model: push.model,
        launched_at_ms: push.launched_at_ms,
    };
    let adapter = hotsheet_aitools::session_push::OpenCodeHttpPush::restored(
        launch,
        push.session_id,
        Arc::new(UreqHttp::default()),
    );
    register(state, id, Arc::new(adapter));
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
    // HS2-4R5MNJ: a prompt the bootstrap adapter pasted on its own is answered as delivered
    // by the adapter that replaces it, never sent a second time.
    let delivered = adapter(state, terminal_id).and_then(|current| current.delivered_prompt());
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
    let adapter: Arc<dyn SessionPush> = match delivered {
        Some(text) => Arc::new(AlreadyDelivered {
            inner: adapter,
            text: Mutex::new(Some(text)),
        }),
        None => adapter,
    };
    let mut pushes = state.session_pushes.lock_or_recover();
    // Never displace a launch-wired adapter registered for this terminal.
    if pushes
        .get(terminal_id)
        .is_none_or(|current| is_replaceable_reported(current.mechanism()))
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
        .is_some_and(|current| is_replaceable_reported(current.mechanism()))
    {
        pushes.remove(terminal_id);
    }
}

fn is_replaceable_reported(mechanism: &str) -> bool {
    mechanism == CODEX_BOOTSTRAP || session_push::is_reported_mechanism(mechanism)
}

/// Drop an ended terminal's adapter.
/// Forget a terminal's adapter; returns whether it had one (it could have been its project's
/// primary for proactive feedback delivery, HS2-DB2FQ2).
pub(crate) fn unregister(pushes: &SessionPushes, terminal_id: &str) -> bool {
    pushes.lock_or_recover().remove(terminal_id).is_some()
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
/// - `409 session_prompt_held` — the session cannot take a prompt yet (a new Codex TUI
///   behind its first-run dialogs); the host keeps it and sends it once the session is
///   ready (HS2-4R5MNJ). Nothing was delivered yet.
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
    let launch_wired = adapter.mechanism() == session_push::OPENCODE_HTTP;
    let receipt = tokio::task::spawn_blocking(move || adapter.message(&message))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(push_error)?;
    if let (true, Some(broker), Some(session_id)) =
        (launch_wired, &state.terminal_broker, &receipt.session_id)
    {
        if let Err(error) = broker
            .call(hotsheet_terminals::BrokerRequest::SetPushSession {
                id: id.clone(),
                session_id: session_id.clone(),
            })
            .await
        {
            tracing::warn!(%error, terminal = %id, "could not retain resolved push session in broker");
        }
    }
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
        // HS2-4R5MNJ: kept, and sent by the host once the session is ready; not delivered.
        PushError::Held(_) => {
            ApiError::new(StatusCode::CONFLICT, message).with_code("session_prompt_held")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    /// A stand-in Codex TUI: records what the adapter types and lets the test move its
    /// clock. Its screens are fed through `observe` as real TUI output would be.
    type Typed = Arc<Mutex<Vec<Vec<u8>>>>;

    fn bootstrap() -> (CodexBootstrapPush, Typed, Arc<AtomicU64>) {
        let writes = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let recorded = writes.clone();
        let clock = Arc::new(AtomicU64::new(1_000));
        let now = clock.clone();
        let push = CodexBootstrapPush::new(
            Box::new(move |bytes| {
                recorded.lock().unwrap().push(bytes);
                Ok(())
            }),
            Box::new(move || now.load(Ordering::SeqCst)),
        );
        (push, writes, clock)
    }

    fn prompt(text: &str) -> PushMessage {
        PushMessage::Prompt {
            text: text.into(),
            banner: None,
        }
    }

    /// A ratatui-like screen: cursor moves and styles around the visible text.
    fn screen(text: &str) -> Vec<u8> {
        format!("\x1b[2J\x1b[1;1H\x1b[2m{text}\x1b[0m\x1b]0;codex\x07\x1b[24;80H").into_bytes()
    }

    const PASTE: &[u8] = b"\x1b[200~one\ntwo\x1b[201~\r";

    #[test]
    fn codex_bootstrap_validates_then_sends_exactly_one_prompt() {
        let (push, writes, _) = bootstrap();
        assert!(!push.observe(&screen("Ask Codex to do anything")));
        assert!(matches!(
            push.message(&PushMessage::Command {
                command: "test".into()
            }),
            Err(PushError::Unsupported(_))
        ));
        assert!(matches!(
            push.message(&prompt("bad\x03")),
            Err(PushError::Invalid(_))
        ));
        let first = push.message(&prompt("one\ntwo")).unwrap();
        assert_eq!(first.mechanism, CODEX_BOOTSTRAP);
        assert!(!first.confirmed);
        assert!(matches!(
            push.message(&prompt("repeat")),
            Err(PushError::Unreachable(_))
        ));
        assert_eq!(*writes.lock().unwrap(), vec![PASTE.to_vec()]);
    }

    /// HS2-4R5MNJ transition matrix with a stand-in TUI that shows its first-run dialogs
    /// first and its composer last: no paste while any dialog is up, exactly one paste
    /// when the composer appears, and the delivery read as delivered only after it.
    #[test]
    fn hs2_4r5mnj_a_prompt_is_held_behind_first_run_dialogs_and_pasted_once_at_the_composer() {
        let (push, writes, _) = bootstrap();
        // Launched: nothing drawn yet. The orchestrator's prompt is held, not delivered.
        assert!(matches!(
            push.message(&prompt("one\ntwo")),
            Err(PushError::Held(_))
        ));
        for dialog in [
            "Update available! 0.167.1 -> 0.168.0  1. Update now  2. Skip",
            "Trust this folder? Codex can read, edit and run files here.",
            "1 hook is new or changed. Hooks need review",
            "Ask Codex to do anything  Waiting for startup",
        ] {
            assert!(!push.observe(&screen(dialog)), "{dialog}");
            // Asked again while a dialog is up: still held, still not typed.
            assert!(matches!(
                push.message(&prompt("one\ntwo")),
                Err(PushError::Held(_))
            ));
            assert!(writes.lock().unwrap().is_empty(), "pasted into: {dialog}");
        }
        // The composer appears (split across reads): exactly one paste.
        assert!(!push.observe(b"\x1b[2J\x1b[20;3H\x1b[2mAsk Codex"));
        assert!(push.observe(b" to do anything\x1b[0m"));
        assert_eq!(*writes.lock().unwrap(), vec![PASTE.to_vec()]);
        // More output, and the retry that records it: delivered, nothing typed again.
        assert!(!push.observe(&screen("Ask Codex to do anything")));
        let receipt = push.message(&prompt("one\ntwo")).unwrap();
        assert!(!receipt.confirmed);
        assert_eq!(push.delivered_prompt().as_deref(), Some("one\ntwo"));
        assert!(matches!(
            push.message(&prompt("other")),
            Err(PushError::Unreachable(_))
        ));
        assert_eq!(writes.lock().unwrap().len(), 1);
    }

    /// HS2-4R5MNJ: a composer already on screen takes the prompt at once; a dialog drawn
    /// after the composer holds it again.
    #[test]
    fn hs2_4r5mnj_the_latest_screen_decides() {
        let (push, writes, _) = bootstrap();
        push.observe(&screen("Ask Codex to do anything"));
        push.observe(&screen("Trust this folder?"));
        assert!(matches!(
            push.message(&prompt("one\ntwo")),
            Err(PushError::Held(_))
        ));
        assert!(writes.lock().unwrap().is_empty());
        assert!(push.observe(&screen("Ask Codex to do anything")));
        assert_eq!(*writes.lock().unwrap(), vec![PASTE.to_vec()]);

        let (ready, writes, _) = bootstrap();
        ready.observe(&screen("Ask Codex to do anything"));
        assert!(ready.message(&prompt("one\ntwo")).is_ok());
        assert_eq!(*writes.lock().unwrap(), vec![PASTE.to_vec()]);
    }

    /// HS2-4R5MNJ: a TUI that never shows its composer within the hold is reported as not
    /// reached, and nothing is pasted into the dialog it shows.
    #[test]
    fn hs2_4r5mnj_a_composer_that_never_appears_gets_no_paste() {
        let (push, writes, clock) = bootstrap();
        assert!(matches!(
            push.message(&prompt("one\ntwo")),
            Err(PushError::Held(_))
        ));
        push.observe(&screen("Trust this folder?"));
        clock.fetch_add(
            u64::try_from(BOOTSTRAP_HOLD.as_millis()).unwrap() + 1,
            Ordering::SeqCst,
        );
        // The composer shows after the hold ended: too late, nothing is typed.
        assert!(!push.observe(&screen("Ask Codex to do anything")));
        assert!(writes.lock().unwrap().is_empty());
        assert_eq!(push.delivered_prompt(), None);
        // Asked once more after the hold: the session can take it now, so it is pasted.
        assert!(push.message(&prompt("one\ntwo")).is_ok());
        assert_eq!(writes.lock().unwrap().len(), 1);

        let (expired, writes, clock) = bootstrap();
        assert!(matches!(
            expired.message(&prompt("x")),
            Err(PushError::Held(_))
        ));
        clock.fetch_add(
            u64::try_from(BOOTSTRAP_HOLD.as_millis()).unwrap() + 1,
            Ordering::SeqCst,
        );
        assert!(matches!(
            expired.message(&prompt("x")),
            Err(PushError::Unreachable(_))
        ));
        assert!(writes.lock().unwrap().is_empty());
    }

    /// HS2-4R5MNJ: the adapter that replaces the bootstrap after SessionStart answers the
    /// prompt the bootstrap pasted as delivered, once, and passes everything else on.
    #[test]
    fn hs2_4r5mnj_the_replacement_never_sends_the_pasted_prompt_again() {
        struct Counting(Mutex<Vec<String>>);
        impl SessionPush for Counting {
            fn mechanism(&self) -> &'static str {
                "codex-thread-queue"
            }
            fn session_id(&self) -> Option<String> {
                Some("thread-1".into())
            }
            fn message(&self, message: &PushMessage) -> Result<PushReceipt, PushError> {
                if let PushMessage::Prompt { text, .. } = message {
                    self.0.lock().unwrap().push(text.clone());
                }
                Ok(PushReceipt {
                    mechanism: "codex-thread-queue",
                    session_id: Some("thread-1".into()),
                    confirmed: true,
                })
            }
        }
        let inner = Arc::new(Counting(Mutex::new(Vec::new())));
        let replacement = AlreadyDelivered {
            inner: inner.clone(),
            text: Mutex::new(Some("role".into())),
        };
        assert!(!replacement.message(&prompt("role")).unwrap().confirmed);
        assert!(inner.0.lock().unwrap().is_empty());
        assert!(replacement.message(&prompt("role")).unwrap().confirmed);
        assert!(replacement.message(&prompt("next")).unwrap().confirmed);
        assert_eq!(*inner.0.lock().unwrap(), ["role", "next"]);
    }

    #[test]
    fn hs2_4r5mnj_screens_are_read_without_terminal_controls() {
        assert_eq!(
            printable(b"\x1b[1;1H\x1b[2mAsk\x1b[0m Codex\x1b]0;title\x07!\x1b]8;;u\x1b\\x\r\n"),
            "Ask Codex!x\n"
        );
        assert!(codex_composer_shown(
            "Trust this folder? ... Ask Codex to do anything"
        ));
        assert!(!codex_composer_shown(
            "Ask Codex to do anything ... Update available!"
        ));
        assert!(!codex_composer_shown("Hooks need review"));
        assert!(!codex_composer_shown(""));
    }
}
