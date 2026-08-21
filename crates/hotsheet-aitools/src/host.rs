//! The glue that turns the declarative plugin registry + the drive interface into a
//! usable capability: build a [`Drive`] from a plugin's `[drive]` declaration, and
//! `trigger` a tool for a project — registering a [`Connection`] and running one turn.
//!
//! Tool-id-free (`docs/05` §5.1): the drive is chosen by the manifest's declared
//! transport, never by branching on which tool it is.

use hotsheet_plugins::Plugin;

use crate::appserver::AppServerDrive;
use crate::claude::ClaudeChannelDrive;
use crate::drive::{Drive, DriveCtx, DriveError, Target, TurnHandle};
use crate::registry::{Connection, ConnectionRegistry, Role};
use crate::spawn::{ContentMode, SpawnConfig, SpawnDrive};

/// Build a [`Drive`] from a plugin's `[drive]` declaration, or `None` if the plugin
/// isn't drivable (no `[drive]`) or declares a transport not built yet.
pub fn drive_for(plugin: &Plugin) -> Option<Box<dyn Drive>> {
    let spec = plugin.manifest.drive.as_ref()?;
    match spec.transport.as_str() {
        // Persistent daemon — a turn on a resumed thread (Codex). The drive uses the
        // injected `AppServerClient`; `program`/`args` aren't its launch line.
        "app-server" => Some(Box::new(AppServerDrive)),
        // Persistent channel — a turn injected into a running `claude` session. Uses the
        // injected `ClaudeChannelClient`; `program`/`args` aren't its launch line.
        "claude-channel" => Some(Box::new(ClaudeChannelDrive)),
        // Spawn-per-run (agy, `codex exec` fallback): a fresh process per turn.
        "spawn" => Some(Box::new(SpawnDrive::new(SpawnConfig {
            program: spec.program.clone(),
            args: spec.args.clone(),
            content: match spec.content.as_str() {
                "stdin" => ContentMode::Stdin,
                _ => ContentMode::Arg,
            },
            interrupt: spec.interrupt,
            resume_flag: spec.resume_flag.clone(),
        }))),
        // acp (OpenCode/Goose) lands with its drive later.
        _ => None,
    }
}

/// The result of a trigger: the registered connection's id + the running turn.
pub struct Triggered {
    pub connection_id: String,
    pub turn: Box<dyn TurnHandle>,
}

/// A failed trigger.
#[derive(Debug, thiserror::Error)]
pub enum TriggerError {
    #[error("plugin '{0}' is not drivable (no [drive], or its transport isn't built yet)")]
    NotDrivable(String),
    #[error(transparent)]
    Drive(#[from] DriveError),
}

/// Trigger `plugin` for `project`: register a connection, run one turn with `content`,
/// and mark the connection busy. `conn_id` is caller-minted (a session id); `now_ms` is
/// the injected clock. The connection's transport is taken from the plugin's drive.
#[allow(clippy::too_many_arguments)]
pub fn trigger(
    plugin: &Plugin,
    project: &str,
    role: Role,
    conn_id: String,
    content: &str,
    ctx: &DriveCtx,
    registry: &mut ConnectionRegistry,
    now_ms: u64,
) -> Result<Triggered, TriggerError> {
    let drive =
        drive_for(plugin).ok_or_else(|| TriggerError::NotDrivable(plugin.id().to_string()))?;
    let connection_id = registry.register(Connection {
        id: conn_id,
        project: project.to_string(),
        tool: plugin.id().to_string(),
        role,
        transport: drive.info().transport,
        pid: None,
        started_at_ms: now_ms,
    });
    let turn = drive.run(&Target::default(), content, ctx)?;
    registry.note_activity(&connection_id, now_ms);
    Ok(Triggered {
        connection_id,
        turn,
    })
}
