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
pub mod permission;
pub mod ports;
mod procio;
pub mod registry;
pub mod safe_trigger;
pub mod spawn;
pub mod system;

pub use acp::{AcpDrive, usage as acp_usage};
pub use appserver::AppServerDrive;
pub use claude::{ClaudeChannel, ClaudeChannelDrive, ClaudeStreamTransport, claude_result_usage};
pub use codex::{
    CodexAppServer, CodexDaemonService, PermissionPolicy, ProxyTransport, StdioTransport,
    UdsWsTransport, codex_control_socket_path, ensure_codex_daemon, ensure_codex_daemon_in,
    notification_usage as codex_notification_usage, stop_codex_daemon_in,
    turn_usage as codex_turn_usage,
};
pub use drive::{
    BackingService, ClaudeChannelClient, DoneReason, Drive, DriveCtx, DriveError, DriveInfo,
    PermReq, Target, Transport, TurnEvent, TurnHandle, Usage,
};
pub use host::{TriggerError, Triggered, drive_for, trigger};
pub use live::{LiveError, LiveTrigger, TurnDone, run_trigger};
pub use permission::{
    Decision as PermissionDecision, Outcome as PermissionOutcome, PermissionBridge,
    Request as PermissionRequest, Resolved as PermissionResolved, Rule as PermissionRule,
    Scope as PermissionScope, SharedPermissionBridge, StoredRule as StoredPermissionRule,
    append_rule as append_permission_rule, load_rules as load_permission_rules,
};
pub use ports::{
    AcpClient, AppServerClient, AppServerError, AppServerOutcome, AppServerTurn, ProcessSpawner,
    RpcReader, RpcTransport, RpcWriter, SpawnSpec, SpawnedProcess,
};
pub use registry::{Connection, ConnectionRegistry, Role};
pub use safe_trigger::{SafeTrigger, prepare_trigger};
pub use spawn::{ContentMode, SpawnConfig, SpawnDrive};
pub use system::SystemSpawner;

#[cfg(test)]
mod tests;
