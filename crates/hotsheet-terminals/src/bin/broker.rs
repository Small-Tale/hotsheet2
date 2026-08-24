//! `hotsheet-terminal-broker` (HS2-8HHFHN) — the detached process that owns a project's PTYs
//! so they **survive a server restart**. Usage:
//!
//! ```text
//! hotsheet-terminal-broker <socket-path> <project-id>
//! ```
//!
//! Binds the Unix socket, hosts a `TerminalManager`, and serves the broker protocol until
//! killed. The server spawns/discovers this per project and routes its `/terminals` ops here.

use std::sync::Arc;

use hotsheet_terminals::{
    DEFAULT_IDLE_GRACE, SocketCleanup, TerminalManager, serve_broker_with_idle,
};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let socket = args.next().unwrap_or_else(|| {
        eprintln!("usage: hotsheet-terminal-broker <socket-path> <project-id>");
        std::process::exit(2);
    });
    let project = args.next().unwrap_or_default();

    // A stale socket from a crashed broker would block the bind — remove it first. (The
    // server's discovery only spawns a broker when no live one answers the socket.)
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let _socket_cleanup = SocketCleanup::new(&socket);
    eprintln!("hotsheet-terminal-broker: serving project '{project}' on {socket}");

    serve_broker_with_idle(
        listener,
        project,
        Arc::new(TerminalManager::new()),
        Some(DEFAULT_IDLE_GRACE),
    )
    .await;
    Ok(())
}
