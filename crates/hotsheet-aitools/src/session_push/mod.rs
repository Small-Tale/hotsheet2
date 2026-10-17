//! **Session push** (HS2-PEJ962, `docs/13` §13.12): deliver a message into a running
//! *interactive* AI session that a Hot Sheet terminal launched, so Hot Sheet can tell an
//! attached agent about a new note or answered feedback without a human typing it.
//!
//! Each tool reaches its live session differently, so the host keeps one small
//! [`SessionPush`] trait and one adapter per mechanism, selected by the plugin manifest's
//! `[session_push] mechanism` (never by tool id, `docs/05` §5.1). A tool without that
//! section has no adapter and the host reports the capability as unsupported (Antigravity's
//! user-started TUI has no attach point at all).
//!
//! | Mechanism | Adapter | Launch wiring |
//! | --- | --- | --- |
//! | `opencode-http` | [`OpenCodeHttpPush`] — the TUI's own HTTP server | `--port {port}` + a random `OPENCODE_SERVER_PASSWORD` |
//!
//! The Claude inbox-socket (HS2-M15CHN) and `codex queue` (HS2-FKM97G) adapters implement
//! the same trait.

mod http;
mod opencode;

use std::sync::Arc;

pub use http::{HttpPort, HttpRequest, HttpResponse, UreqHttp, basic_auth_header, query_escape};
pub use opencode::{MECHANISM as OPENCODE_HTTP, OpenCodeHttpPush};

/// What to deliver into a live session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushMessage {
    /// Prose the session's agent receives as a new prompt. `banner` additionally shows a
    /// short visible notice in the tool's UI when the mechanism has one.
    Prompt {
        text: String,
        banner: Option<String>,
    },
    /// A command the tool's own UI runs (for example an OpenCode TUI command such as
    /// `agent_cycle`). Only mechanisms that can run UI commands accept it.
    Command { command: String },
}

/// The outcome of a delivered message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushReceipt {
    /// The mechanism that delivered it (`opencode-http`, …).
    pub mechanism: &'static str,
    /// The tool-native session the message went to, when the mechanism addresses one.
    pub session_id: Option<String>,
    /// `false` when the tool accepted the request but cannot confirm it acted on it (the
    /// OpenCode TUI answers `true` to every command, including unknown ones).
    pub confirmed: bool,
}

/// Why a message could not be delivered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PushError {
    /// The mechanism cannot deliver this kind of message.
    #[error("{0}")]
    Unsupported(String),
    /// The message itself is malformed (for example empty text).
    #[error("{0}")]
    Invalid(String),
    /// The session could not be reached (not listening, exited, network error).
    #[error("session unreachable: {0}")]
    Unreachable(String),
    /// The session answered with an error status.
    #[error("session rejected the message (HTTP {status}): {body}")]
    Rejected { status: u16, body: String },
}

/// One live session's push adapter. An instance is bound to exactly one launched session;
/// the host keeps it alongside that session's terminal connection and drops it when the
/// terminal exits.
pub trait SessionPush: Send + Sync {
    /// The manifest mechanism this adapter implements.
    fn mechanism(&self) -> &'static str;
    /// The tool-native session id, once known.
    fn session_id(&self) -> Option<String>;
    /// Deliver one message. Blocking: callers on an async runtime use `spawn_blocking`.
    fn message(&self, message: &PushMessage) -> Result<PushReceipt, PushError>;
}

/// Everything an adapter needs to know about the session the host is launching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushLaunch {
    /// Loopback port allocated for the session (substituted for `{port}`).
    pub port: u16,
    /// Per-session random secret (exported through `password_env`).
    pub password: String,
    /// The session's working directory (the terminal's cwd).
    pub directory: String,
    /// The model the launch selected (`provider/model` for OpenCode), if any.
    pub model: Option<String>,
    /// When the launch happened (Unix ms); a session created before it is not this one.
    pub launched_at_ms: u64,
}

/// Arguments and environment to add to an interactive launch for a push-capable session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchWiring {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Whether this host has an adapter for the manifest's mechanism.
pub fn is_supported(spec: &hotsheet_plugins::SessionPushSpec) -> bool {
    spec.mechanism == OPENCODE_HTTP
}

/// The launch arguments/environment that make the session reachable, or `None` when the
/// mechanism is unknown to this host.
pub fn launch_wiring(
    spec: &hotsheet_plugins::SessionPushSpec,
    launch: &PushLaunch,
) -> Option<LaunchWiring> {
    if !is_supported(spec) {
        return None;
    }
    let port = launch.port.to_string();
    let args = spec
        .port_args
        .iter()
        .map(|arg| arg.replace("{port}", &port))
        .collect();
    let env = spec
        .password_env
        .iter()
        .map(|name| (name.clone(), launch.password.clone()))
        .collect();
    Some(LaunchWiring { args, env })
}

/// Build the adapter for a launched session, or `None` when the mechanism is unknown.
pub fn adapter_for(
    spec: &hotsheet_plugins::SessionPushSpec,
    launch: PushLaunch,
    http: Arc<dyn HttpPort>,
) -> Option<Arc<dyn SessionPush>> {
    if spec.mechanism == OPENCODE_HTTP {
        return Some(Arc::new(OpenCodeHttpPush::new(launch, http)));
    }
    None
}

/// A free loopback TCP port for a session's local API. The listener is released before the
/// tool binds it, so a racing process could take it; the tool then fails to bind and the
/// push reports the session unreachable instead of reaching the wrong process (every
/// request carries the per-session password).
pub fn allocate_loopback_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

#[cfg(test)]
mod tests;
