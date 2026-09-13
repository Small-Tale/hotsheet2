//! `hotsheet-terminal-broker` (HS2-8HHFHN) — the detached process that owns a project's PTYs
//! so they **survive a server restart**. Usage:
//!
//! ```text
//! hotsheet-terminal-broker <socket-path> <project-id>
//! ```
//!
//! Binds the Unix socket, hosts a `TerminalManager`, and serves the broker protocol until its
//! empty idle grace expires. The server spawns/discovers this per project and routes its
//! `/terminals` ops here.

use hotsheet_terminals::run_broker_process;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let socket = args.next().unwrap_or_else(|| {
        eprintln!("usage: hotsheet-terminal-broker <socket-path> <project-id>");
        std::process::exit(2);
    });
    let project = args.next().unwrap_or_default();

    eprintln!("hotsheet-terminal-broker: serving project '{project}' on {socket}");
    run_broker_process(socket, project).await
}
