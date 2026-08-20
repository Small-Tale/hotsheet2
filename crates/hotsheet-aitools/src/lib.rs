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

pub mod drive;
pub mod host;
pub mod ports;
pub mod registry;
pub mod spawn;
pub mod system;

pub use drive::{
    DoneReason, Drive, DriveCtx, DriveError, DriveInfo, Target, Transport, TurnHandle,
};
pub use host::{TriggerError, Triggered, drive_for, trigger};
pub use ports::{ProcessSpawner, SpawnSpec, SpawnedProcess};
pub use registry::{Connection, ConnectionRegistry, Role};
pub use spawn::{ContentMode, SpawnConfig, SpawnDrive};
pub use system::SystemSpawner;

#[cfg(test)]
mod tests;
