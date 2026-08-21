//! Wire a plugin's declared drive to a **real** running tool and drive one turn,
//! streaming its events (HS2-109 — the reachability behind `hotsheet-cli trigger`). The
//! transport/protocol logic is fake-tested in each drive module (`docs/05` §5.10), so
//! this file is only the effectful glue: spawn the real process (with HS2-103 isolation),
//! build the [`DriveCtx`], run [`trigger`], and stream to completion.

use std::path::PathBuf;

use hotsheet_plugins::Plugin;

use crate::claude::{ClaudeChannel, ClaudeStreamTransport};
use crate::codex::{
    CodexAppServer, StdioTransport, UdsWsTransport, codex_control_socket_path,
    ensure_codex_daemon_in,
};
use crate::drive::{DoneReason, DriveCtx, TurnEvent};
use crate::host::{TriggerError, trigger};
use crate::registry::{ConnectionRegistry, Role};
use crate::system::SystemSpawner;

/// What to launch, and how to keep it from reaching HS1 (HS2-103 safety).
pub struct LiveTrigger {
    /// The project directory the tool runs in.
    pub cwd: PathBuf,
    /// The prompt/turn content (e.g. "work the top Up Next ticket").
    pub prompt: String,
    pub role: Role,
    /// Caller-minted connection id (session id).
    pub conn_id: String,
    /// Resume a prior session (channel `--resume`), if any.
    pub resume: Option<String>,
    /// Restrict a channel tool to only this MCP config (`--strict-mcp-config`), so it can
    /// reach the Hot Sheet shim but nothing else (ignored by other transports).
    pub mcp_config: Option<PathBuf>,
    /// Claude permission mode for headless work (e.g. `"acceptEdits"`); the real bridge is
    /// HS2-113.
    pub permission_mode: Option<String>,
    /// Extra env for the launched process (e.g. an isolated `CODEX_HOME`).
    pub env: Vec<(String, String)>,
    /// For an app-server tool: drive the **shared daemon** for this connection's
    /// `CODEX_HOME` (one codex instance reused across turns) instead of spawning a fresh
    /// `app-server` process per connection (HS2-B7C66H). Requires `CODEX_HOME` in `env`.
    pub shared_daemon: bool,
    /// Injected clock.
    pub now_ms: u64,
}

/// A failed live trigger.
#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    #[error("'{0}' is not drivable (no [drive], or its transport isn't built yet)")]
    NotDrivable(String),
    #[error("launching '{program}': {source}")]
    Launch {
        program: String,
        source: std::io::Error,
    },
    #[error("connecting to '{0}': {1}")]
    Connect(String, String),
    #[error(transparent)]
    Drive(#[from] crate::drive::DriveError),
}

/// Drive one real turn of `plugin` for the project, invoking `on_event` for each streamed
/// event (including the terminal `Done`), and return why it finished. The connection is
/// registered busy in `registry` and set idle when the turn ends.
pub fn run_trigger(
    plugin: &Plugin,
    t: &LiveTrigger,
    registry: &mut ConnectionRegistry,
    on_event: &mut dyn FnMut(&TurnEvent),
) -> Result<DoneReason, LiveError> {
    let spec = plugin
        .manifest
        .drive
        .as_ref()
        .ok_or_else(|| LiveError::NotDrivable(plugin.id().to_string()))?;
    let program = spec.program.clone();
    let spawner = SystemSpawner;

    // Build the tool-specific live connection, then a DriveCtx, then drive within scope so
    // the connection outlives the turn.
    match spec.transport.as_str() {
        "claude-channel" => {
            let transport = ClaudeStreamTransport::spawn(
                &program,
                &t.cwd,
                t.resume.as_deref(),
                t.mcp_config.as_deref(),
                t.permission_mode.as_deref(),
                &t.env,
            )
            .map_err(|source| LiveError::Launch {
                program: program.clone(),
                source,
            })?;
            let channel = ClaudeChannel::connect(transport);
            let ctx = DriveCtx {
                cwd: t.cwd.clone(),
                spawner: &spawner,
                env: t.env.clone(),
                app_server: None,
                channel: Some(&channel),
            };
            drive_and_stream(plugin, t, &ctx, registry, on_event)
        }
        "app-server" => {
            // Two shapes of the persistent app-server (docs/13 §13.5):
            //  - default: spawn `codex app-server` direct — one process for this connection;
            //  - shared_daemon: attach to the daemon for this connection's CODEX_HOME over a
            //    WebSocket, so many connections/turns reuse ONE codex instance (HS2-B7C66H).
            let app = if t.shared_daemon {
                connect_shared_daemon(&program, &t.env)?
            } else {
                let transport =
                    StdioTransport::spawn(&program, &t.cwd, &t.env).map_err(|source| {
                        LiveError::Launch {
                            program: program.clone(),
                            source,
                        }
                    })?;
                CodexAppServer::connect(transport)
                    .map_err(|e| LiveError::Connect(program.clone(), e.to_string()))?
            };
            let ctx = DriveCtx {
                cwd: t.cwd.clone(),
                spawner: &spawner,
                env: t.env.clone(),
                app_server: Some(&app),
                channel: None,
            };
            drive_and_stream(plugin, t, &ctx, registry, on_event)
        }
        "spawn" => {
            let ctx = DriveCtx {
                cwd: t.cwd.clone(),
                spawner: &spawner,
                env: t.env.clone(),
                app_server: None,
                channel: None,
            };
            drive_and_stream(plugin, t, &ctx, registry, on_event)
        }
        _ => Err(LiveError::NotDrivable(plugin.id().to_string())),
    }
}

/// Attach to the shared codex daemon for the `CODEX_HOME` in `env`: start it (idempotent)
/// and connect its WebSocket control socket. Requires `CODEX_HOME` to be set — the caller
/// (`hotsheet-cli`) points it at a **daemon-ready isolated home** so MCP isolation holds
/// while one instance is reused (HS2-B7C66H).
fn connect_shared_daemon(
    program: &str,
    env: &[(String, String)],
) -> Result<CodexAppServer, LiveError> {
    let codex_home = env
        .iter()
        .find(|(k, _)| k == "CODEX_HOME")
        .map(|(_, v)| PathBuf::from(v))
        .ok_or_else(|| {
            LiveError::Connect(
                program.to_string(),
                "--shared-daemon needs a CODEX_HOME (none in env)".into(),
            )
        })?;
    ensure_codex_daemon_in(program, &codex_home).map_err(|source| LiveError::Launch {
        program: program.to_string(),
        source,
    })?;
    let socket = codex_control_socket_path(&codex_home);
    let transport = UdsWsTransport::connect(&socket).map_err(|source| LiveError::Launch {
        program: program.to_string(),
        source,
    })?;
    CodexAppServer::connect(transport)
        .map_err(|e| LiveError::Connect(program.to_string(), e.to_string()))
}

fn drive_and_stream(
    plugin: &Plugin,
    t: &LiveTrigger,
    ctx: &DriveCtx,
    registry: &mut ConnectionRegistry,
    on_event: &mut dyn FnMut(&TurnEvent),
) -> Result<DoneReason, LiveError> {
    let project = t.cwd.display().to_string();
    let out = trigger(
        plugin,
        &project,
        t.role,
        t.conn_id.clone(),
        &t.prompt,
        ctx,
        registry,
        t.now_ms,
    )
    .map_err(|e| match e {
        TriggerError::NotDrivable(id) => LiveError::NotDrivable(id),
        TriggerError::Drive(d) => LiveError::Drive(d),
    })?;

    let conn_id = out.connection_id.clone();
    let mut turn = out.turn;
    let reason = loop {
        match turn.next_event() {
            Some(TurnEvent::Done(r)) => break r,
            Some(ev) => {
                on_event(&ev);
                registry.note_activity(&conn_id, t.now_ms);
            }
            // Non-streaming drives (spawn / app-server): no events, just the terminal wait.
            None => break turn.wait(),
        }
    };
    on_event(&TurnEvent::Done(reason));
    registry.set_idle(&conn_id);
    Ok(reason)
}
