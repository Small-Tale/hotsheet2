//! Server ↔ **detached terminal broker** integration (`docs/05` §5.4, HS2-ERT00F). When the
//! terminal broker is enabled, the server routes its `/terminals` request/response ops through
//! a [`BrokerClient`] to a separate `hotsheet-terminal-broker` process, so terminals **survive
//! a server restart**: on restart the server just reconnects to the still-running broker (or
//! spawns one if none is live) and its terminals are still there.
//!
//! This module owns discovery + detached spawn + a per-request call helper. The live WS attach
//! streaming through the broker is a follow-up; today broker mode serves open/list/read/input/
//! kill (a client polls `GET /terminals/{id}` for output).

use std::path::{Path, PathBuf};

use hotsheet_terminals::{BrokerClient, BrokerRequest, BrokerResponse};

/// A project's broker coordinates: the Unix socket + the project id the broker serves.
#[derive(Debug, Clone)]
pub struct TerminalBroker {
    pub socket: PathBuf,
    pub project: String,
}

impl TerminalBroker {
    /// The socket path for a store: `${HOTSHEET_HOME}/broker/<project-id>.sock`.
    fn socket_for(project: &str) -> PathBuf {
        hotsheet_plugins::hotsheet_home()
            .join("broker")
            .join(format!("{project}.sock"))
    }

    /// Ensure a broker is running for `store_path` and return its coordinates. Connects to an
    /// existing live broker if the socket answers; otherwise spawns `hotsheet-terminal-broker`
    /// **detached** (it outlives this server) and waits for it to accept. Blocking — call once
    /// at startup.
    pub fn ensure(store_path: &Path) -> anyhow::Result<Self> {
        let project = hotsheet_tls::project_id(store_path);
        let socket = Self::socket_for(&project);

        if is_live(&socket) {
            return Ok(Self { socket, project });
        }
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent)?;
        }
        spawn_broker(&socket, &project)?;

        // Wait (briefly) for the freshly-spawned broker to bind + accept.
        for _ in 0..40 {
            if is_live(&socket) {
                return Ok(Self { socket, project });
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        anyhow::bail!("terminal broker did not come up on {}", socket.display())
    }

    /// Point at an explicit socket/project (tests, or an already-running broker).
    pub fn at(socket: impl Into<PathBuf>, project: impl Into<String>) -> Self {
        Self {
            socket: socket.into(),
            project: project.into(),
        }
    }

    /// One request/response round-trip to the broker (a fresh connection — terminal ops are
    /// infrequent, so a pooled connection isn't worth the complexity yet).
    pub async fn call(&self, req: BrokerRequest) -> std::io::Result<BrokerResponse> {
        let mut client = BrokerClient::connect(&self.socket).await?;
        client.request(&req).await
    }
}

/// Whether a broker is accepting on `socket` right now (a blocking connect probe).
fn is_live(socket: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(socket).is_ok()
}

/// Spawn `hotsheet-terminal-broker <socket> <project>` detached, with its stdio to null so it
/// doesn't hold the server's. The binary resolves as a sibling of the current server exe.
fn spawn_broker(socket: &Path, project: &str) -> std::io::Result<()> {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("hotsheet-terminal-broker")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("hotsheet-terminal-broker"));
    std::process::Command::new(&exe)
        .arg(socket)
        .arg(project)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?; // not waited on → outlives this server (reparented on exit)
    Ok(())
}
