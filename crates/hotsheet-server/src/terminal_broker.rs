//! Server ↔ **detached terminal broker** integration (`docs/05` §5.4, HS2-ERT00F). When the
//! terminal broker is enabled, the server routes its `/terminals` request/response ops through
//! a [`BrokerClient`] to a separate `hotsheet-terminal-broker` process, so terminals **survive
//! a server restart**: on restart the server just reconnects to the still-running broker (or
//! spawns one if none is live) and its terminals are still there.
//!
//! This module owns discovery + detached spawn + a per-request call helper. Broker mode serves
//! open/list/read/input/kill and the live WebSocket attach; the server bridges the broker stream
//! so output, input, viewport-size arbitration, and the busy feed survive a server restart.

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

    /// Return the already-running broker for `store_path` without spawning one.
    pub fn discover(store_path: &Path) -> Option<Self> {
        let project = hotsheet_tls::project_id(store_path);
        let socket = Self::socket_for(&project);
        is_live(&socket).then_some(Self { socket, project })
    }

    /// Explicitly kill every terminal retained by this project's detached broker.
    pub async fn kill_all(&self) -> anyhow::Result<usize> {
        let terminals = match self.call(BrokerRequest::List).await? {
            BrokerResponse::List { terminals } => terminals,
            BrokerResponse::Err { message } => anyhow::bail!(message),
            response => anyhow::bail!("unexpected terminal broker response: {response:?}"),
        };
        let mut count = 0;
        for terminal in terminals {
            match self.call(BrokerRequest::Kill { id: terminal.id }).await? {
                BrokerResponse::Ok => count += 1,
                BrokerResponse::NotFound => {}
                BrokerResponse::Err { message } => anyhow::bail!(message),
                response => anyhow::bail!("unexpected terminal broker response: {response:?}"),
            }
        }
        Ok(count)
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

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_terminals::{
        BrokerClient, BrokerRequest, BrokerResponse, TerminalManager, serve_broker,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn kill_all_uses_the_existing_broker_protocol_and_removes_every_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("broker.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        tokio::spawn(serve_broker(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
        ));
        let mut client = BrokerClient::connect(&socket).await.unwrap();
        for id in ["one", "two"] {
            client
                .request(&BrokerRequest::Open {
                    id: id.into(),
                    command: "cat".into(),
                    args: vec![],
                    cwd: None,
                    env: vec![],
                })
                .await
                .unwrap();
        }
        let broker = TerminalBroker::at(&socket, "proj");
        assert_eq!(broker.kill_all().await.unwrap(), 2);
        assert!(
            matches!(broker.call(BrokerRequest::List).await.unwrap(),BrokerResponse::List { terminals } if terminals.is_empty())
        );
    }

    #[test]
    fn server_only_build_uses_the_server_as_its_broker_process_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let server = dir.path().join("hotsheet-server");
        std::fs::write(&server, "").unwrap();
        assert_eq!(broker_launch(Some(server.clone())), (server.clone(), true));
        let sibling = dir.path().join("hotsheet-terminal-broker");
        std::fs::write(&sibling, "").unwrap();
        assert_eq!(broker_launch(Some(server)), (sibling, false));
    }
}

/// Whether a broker is accepting on `socket` right now (a blocking connect probe).
fn is_live(socket: &Path) -> bool {
    use std::io::{BufRead, Write};
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket) else {
        return false;
    };
    let timeout = Some(std::time::Duration::from_millis(500));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
    let Ok(mut line) = serde_json::to_string(&BrokerRequest::Ping) else {
        return false;
    };
    line.push('\n');
    if stream.write_all(line.as_bytes()).is_err() {
        return false;
    }
    let mut reply = String::new();
    if std::io::BufReader::new(stream)
        .read_line(&mut reply)
        .is_err()
    {
        return false;
    }
    matches!(
        serde_json::from_str::<BrokerResponse>(&reply),
        Ok(BrokerResponse::Pong)
    )
}

/// Spawn the broker detached with stdio disconnected from the server. Prefer the dedicated
/// sibling binary; a server-only development build falls back to the server's hidden broker
/// process mode so making detached hosting the default does not add a packaging footgun.
fn spawn_broker(socket: &Path, project: &str) -> std::io::Result<()> {
    let current = std::env::current_exe().ok();
    let (executable, self_hosted) = broker_launch(current);
    let mut command = std::process::Command::new(executable);
    if self_hosted {
        command
            .arg("--terminal-broker-process")
            .arg(socket)
            .arg("--terminal-broker-project")
            .arg(project);
    } else {
        command.arg(socket).arg(project);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?; // not waited on → outlives this server (reparented on exit)
    Ok(())
}

fn broker_launch(current: Option<PathBuf>) -> (PathBuf, bool) {
    let sibling = current
        .as_ref()
        .and_then(|path| {
            path.parent()
                .map(|dir| dir.join("hotsheet-terminal-broker"))
        })
        .filter(|path| path.exists());
    if let Some(executable) = sibling {
        (executable, false)
    } else if let Some(exe) = current {
        (exe, true)
    } else {
        (PathBuf::from("hotsheet-terminal-broker"), false)
    }
}
