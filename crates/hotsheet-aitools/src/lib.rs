//! Hot Sheet 2 AI-tool host — the **behavioral** half of the plugin system
//! (`docs/12` §12.2, `docs/05` §5.3). The declarative loader/registry is
//! `hotsheet-plugins`; this crate holds the host-side capabilities that need process
//! I/O (via injected ports), starting with the **drive/transport** interface
//! (`docs/13`).
//!
//! v1 (HS2-106): the [`Drive`] trait + the [`SpawnDrive`] shape (Codex `exec`), over an
//! injected [`ProcessSpawner`] so it is fully testable against a fake. The
//! persistent-channel (Claude) drive, permission bridge, connection registry, and the
//! async `TurnEvent` stream land next.

pub mod acp;
pub mod appserver;
pub mod claude;
pub mod codex;
pub mod drive;
pub mod host;
pub mod launch_safety;
pub mod live;
pub mod model_catalog;
pub mod permission;
pub mod ports;
pub mod probe;
mod procio;
pub mod registry;
pub mod safe_trigger;
pub mod spawn;
pub mod system;

pub use acp::{AcpDrive, AcpSession, AcpStdio, usage as acp_usage, validate_opencode_transcript};
pub use appserver::AppServerDrive;
pub use claude::{
    ClaudeChannel, ClaudeChannelDrive, ClaudeStreamSpawnOptions, ClaudeStreamTransport,
    claude_result_usage,
};
pub use codex::{
    CodexAppServer, CodexDaemonService, PermissionPolicy, ProxyTransport, StdioTransport,
    UdsWsTransport, codex_control_socket_path, ensure_codex_daemon, ensure_codex_daemon_in,
    notification_usage as codex_notification_usage, stop_codex_daemon_in,
    turn_usage as codex_turn_usage,
};
pub use drive::{
    BackingService, ClaudeChannelClient, DoneReason, Drive, DriveCtx, DriveError, DriveInfo,
    PermReq, Target, Transport, TurnControl, TurnEvent, TurnHandle, Usage,
};
pub use host::{TriggerError, Triggered, drive_for, trigger};
pub use live::{LiveError, LiveTrigger, TurnDone, run_trigger, run_trigger_controlled};
pub use model_catalog::{
    CommandModelCatalog, ModelCatalogCache, RuntimeModelCatalog, RuntimeModelCatalogSource,
    RuntimeModelSpec, discover_ai_tool_descriptors,
};
pub use permission::{
    DEFAULT_PERMISSION_TIMEOUT, Decision as PermissionDecision, Outcome as PermissionOutcome,
    PermissionAsk, PermissionBridge, Request as PermissionRequest, Resolved as PermissionResolved,
    Rule as PermissionRule, Scope as PermissionScope, SharedPermissionBridge,
    StoredRule as StoredPermissionRule, append_rule as append_permission_rule,
    load_rules as load_permission_rules,
};
pub use ports::{
    AcpClient, AppServerClient, AppServerError, AppServerOutcome, AppServerTurn, ProcessSpawner,
    RpcReader, RpcTransport, RpcWriter, SpawnSpec, SpawnedProcess,
};
pub use registry::{Connection, ConnectionRegistry, Role};
pub use safe_trigger::{SafeTrigger, prepare_trigger, prepare_trigger_with_home};
pub use spawn::{ContentMode, SpawnConfig, SpawnDrive};
pub use system::SystemSpawner;

#[cfg(test)]
mod tests;

/// The environment variable carrying a launched AI session's worker id (HS2-1VAW1C).
pub const WORKER_ID_ENV: &str = "HOTSHEET_WORKER_ID";

/// The acting role every Hot Sheet surface reads when a caller does not pass one
/// (HS2-RD4M29). Launchers set it to `ai` for the AI sessions they start, so the CLI and the
/// MCP shim identify those sessions' mutations as AI without per-call flags.
pub const ACTOR_ROLE_ENV: &str = "HOTSHEET_ACTOR_ROLE";

/// The acting id paired with [`ACTOR_ROLE_ENV`]; launchers set it to the session worker id.
pub const ACTOR_ID_ENV: &str = "HOTSHEET_ACTOR_ID";

/// The actor environment for an AI session a launcher starts (HS2-RD4M29).
#[must_use]
pub fn ai_session_actor_env(worker_id: &str) -> [(&'static str, String); 2] {
    [
        (ACTOR_ROLE_ENV, "ai".to_owned()),
        (ACTOR_ID_ENV, worker_id.to_owned()),
    ]
}

/// The worker id a launcher assigns an AI session: `<tool>-<session>`. The bundled
/// instructions tell the AI to claim with it, and the launcher releases every claim it holds
/// when the session ends.
pub fn session_worker_id(tool: &str, session: &str) -> String {
    format!("{tool}-{session}")
}

#[cfg(test)]
mod worker_id_tests {
    #[test]
    fn a_session_worker_id_names_the_tool_and_session() {
        assert_eq!(super::session_worker_id("claude", "01ABC"), "claude-01ABC");
    }
}
