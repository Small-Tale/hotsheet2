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

pub mod appserver;
pub mod claude;
pub mod codex;
pub mod drive;
pub mod host;
pub mod live;
pub mod ports;
mod procio;
pub mod registry;
pub mod spawn;
pub mod system;

pub use appserver::AppServerDrive;
pub use claude::{ClaudeChannel, ClaudeChannelDrive, ClaudeStreamTransport};
pub use codex::{
    CodexAppServer, ProxyTransport, StdioTransport, UdsWsTransport, codex_control_socket_path,
    ensure_codex_daemon, ensure_codex_daemon_in, stop_codex_daemon_in,
};
pub use drive::{
    ClaudeChannelClient, DoneReason, Drive, DriveCtx, DriveError, DriveInfo, PermReq, Target,
    Transport, TurnEvent, TurnHandle,
};
pub use host::{TriggerError, Triggered, drive_for, trigger};
pub use live::{LiveError, LiveTrigger, run_trigger};
pub use ports::{
    AppServerClient, AppServerError, AppServerOutcome, AppServerTurn, ProcessSpawner, RpcReader,
    RpcTransport, RpcWriter, SpawnSpec, SpawnedProcess,
};
pub use registry::{Connection, ConnectionRegistry, Role};
pub use spawn::{ContentMode, SpawnConfig, SpawnDrive};
pub use system::SystemSpawner;

#[cfg(test)]
mod tests;
