//! The **detached terminal broker** (`docs/05` §5.4, HS2-8HHFHN) — a separate process that
//! owns the PTYs so **terminals survive a server restart**. The server (or CLI) talks to it
//! over a per-project **Unix-domain socket** with a line-delimited JSON request/response
//! protocol; because the broker is its own process, restarting the server just reconnects to
//! the still-running broker and its live terminals.
//!
//! This module is the protocol + the [`serve_broker`] loop (host a [`TerminalManager`] over a
//! socket) + the [`BrokerClient`] the server uses. Wiring the server's `/terminals` routes to
//! route through a client (spawn/discover/reconnect) is layered on top.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::manager::TerminalManager;
use crate::terminal::TermSpec;

/// A request the server/CLI sends the broker (one JSON line).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Open (or reattach to) a terminal `id` running `command`.
    Open {
        id: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
    },
    /// List all hosted terminals.
    List,
    /// The terminal's state + scrollback snapshot.
    Read { id: String },
    /// Write input bytes to the terminal's PTY.
    Input { id: String, data: Vec<u8> },
    /// Kill + forget the terminal.
    Kill { id: String },
}

/// The broker's reply (one JSON line).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    /// One terminal's state (for `open`).
    Terminal { info: BrokerTermInfo },
    /// All terminals (for `list`).
    List { terminals: Vec<BrokerTermInfo> },
    /// State + scrollback (for `read`).
    Read {
        info: BrokerTermInfo,
        scrollback: Vec<u8>,
    },
    /// A successful side-effecting op (`input` / `kill`).
    Ok,
    /// No terminal with that id.
    NotFound,
    /// The op failed.
    Err { message: String },
}

/// A terminal's state on the broker wire (mirrors the server's `TerminalInfo`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerTermInfo {
    pub id: String,
    pub alive: bool,
    pub busy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
}

fn info_of(term: &crate::terminal::Terminal, id: &str) -> BrokerTermInfo {
    let osc = term.term_state();
    BrokerTermInfo {
        id: id.to_string(),
        alive: term.is_alive(),
        busy: term.activity() == crate::busy::Activity::Busy,
        cwd: osc.cwd,
        link: osc.link,
        progress: osc.progress,
    }
}

/// Serve a broker on `listener`, hosting `project`'s terminals in `manager` for the process's
/// lifetime. Each connection is handled concurrently; the shared `manager` (and its PTYs)
/// outlive any one connection — the whole point of the broker.
pub async fn serve_broker(listener: UnixListener, project: String, manager: Arc<TerminalManager>) {
    loop {
        let Ok((stream, _addr)) = listener.accept().await else {
            continue;
        };
        let (manager, project) = (manager.clone(), project.clone());
        tokio::spawn(async move {
            let _ = handle_connection(stream, &project, &manager).await;
        });
    }
}

async fn handle_connection(
    stream: UnixStream,
    project: &str,
    manager: &Arc<TerminalManager>,
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(req) => handle_request(project, manager, req),
            Err(e) => Response::Err {
                message: format!("bad request: {e}"),
            },
        };
        let mut out = serde_json::to_string(&resp).unwrap_or_else(|_| {
            "{\"result\":\"err\",\"message\":\"serialize failed\"}".to_string()
        });
        out.push('\n');
        write.write_all(out.as_bytes()).await?;
        write.flush().await?;
    }
    Ok(())
}

/// Run one request against the manager (synchronous — PTY ops are quick, non-blocking).
fn handle_request(project: &str, manager: &Arc<TerminalManager>, req: Request) -> Response {
    let key = |id: &str| (project.to_string(), id.to_string());
    match req {
        Request::Open {
            id,
            command,
            args,
            cwd,
        } => {
            let spec = TermSpec {
                command,
                args,
                cwd: cwd.map(std::path::PathBuf::from),
                env: Vec::new(),
                rows: 24,
                cols: 80,
            };
            match manager.get_or_spawn(key(&id), spec) {
                Ok(term) => Response::Terminal {
                    info: info_of(&term, &id),
                },
                Err(e) => Response::Err {
                    message: e.to_string(),
                },
            }
        }
        Request::List => {
            let terminals = manager
                .list()
                .into_iter()
                .filter_map(|k| manager.get(&k).map(|t| info_of(&t, &k.1)))
                .collect();
            Response::List { terminals }
        }
        Request::Read { id } => match manager.get(&key(&id)) {
            Some(term) => Response::Read {
                info: info_of(&term, &id),
                scrollback: term.scrollback(),
            },
            None => Response::NotFound,
        },
        Request::Input { id, data } => match manager.get(&key(&id)) {
            Some(term) => match term.write(&data) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Err {
                    message: e.to_string(),
                },
            },
            None => Response::NotFound,
        },
        Request::Kill { id } => match manager.kill(&key(&id)) {
            Ok(true) => Response::Ok,
            Ok(false) => Response::NotFound,
            Err(e) => Response::Err {
                message: e.to_string(),
            },
        },
    }
}

/// A client to a running broker over its Unix socket.
pub struct BrokerClient {
    reader: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl BrokerClient {
    /// Connect to the broker at `socket_path`.
    pub async fn connect(socket_path: &Path) -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket_path).await?;
        let (read, writer) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(read).lines(),
            writer,
        })
    }

    /// Send one request and read the reply.
    pub async fn request(&mut self, req: &Request) -> std::io::Result<Response> {
        let mut line = serde_json::to_string(req).map_err(std::io::Error::other)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.flush().await?;
        match self.reader.next_line().await? {
            Some(l) => serde_json::from_str(&l).map_err(std::io::Error::other),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "broker closed the connection",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    async fn wait_until(mut cond: impl FnMut() -> bool, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        cond()
    }

    #[tokio::test]
    async fn broker_hosts_a_pty_reachable_over_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let manager = Arc::new(TerminalManager::new());
        // The broker (a stand-in for the separate process) owns the manager + PTYs.
        tokio::spawn(serve_broker(listener, "proj".into(), manager.clone()));

        // A client opens a `cat` terminal, writes to it, and reads the echo back — all over
        // the socket, with the PTY living on the broker side.
        let mut client = BrokerClient::connect(&sock).await.unwrap();
        let opened = client
            .request(&Request::Open {
                id: "t1".into(),
                command: "cat".into(),
                args: vec![],
                cwd: None,
            })
            .await
            .unwrap();
        assert!(matches!(opened, Response::Terminal { info } if info.alive && info.id == "t1"));

        // list sees it.
        let list = client.request(&Request::List).await.unwrap();
        assert!(matches!(list, Response::List { terminals } if terminals.len() == 1));

        // Write input; `cat` echoes it into the scrollback (readable over the socket).
        let ok = client
            .request(&Request::Input {
                id: "t1".into(),
                data: b"broker-echo\n".to_vec(),
            })
            .await
            .unwrap();
        assert!(matches!(ok, Response::Ok));

        // The PTY + its scrollback live in the broker's manager, independent of this client.
        assert!(
            wait_until(
                || {
                    manager
                        .get(&("proj".into(), "t1".into()))
                        .map(|t| String::from_utf8_lossy(&t.scrollback()).contains("broker-echo"))
                        .unwrap_or(false)
                },
                5,
            )
            .await,
            "cat's echo reaches the broker-hosted scrollback"
        );
        let read = client
            .request(&Request::Read { id: "t1".into() })
            .await
            .unwrap();
        match read {
            Response::Read { scrollback, .. } => {
                assert!(String::from_utf8_lossy(&scrollback).contains("broker-echo"));
            }
            other => panic!("expected Read, got {other:?}"),
        }

        // Kill over the socket; a later read is NotFound.
        assert!(matches!(
            client
                .request(&Request::Kill { id: "t1".into() })
                .await
                .unwrap(),
            Response::Ok
        ));
        assert!(matches!(
            client
                .request(&Request::Read { id: "t1".into() })
                .await
                .unwrap(),
            Response::NotFound
        ));
    }

    #[tokio::test]
    async fn a_second_client_shares_the_brokers_terminals_across_reconnects() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let manager = Arc::new(TerminalManager::new());
        tokio::spawn(serve_broker(listener, "proj".into(), manager));

        // Client A opens a long-lived terminal, then disconnects (drops).
        {
            let mut a = BrokerClient::connect(&sock).await.unwrap();
            let _ = a
                .request(&Request::Open {
                    id: "shared".into(),
                    command: "cat".into(),
                    args: vec![],
                    cwd: None,
                })
                .await
                .unwrap();
        } // A's connection drops here — simulating the server going away.

        // Client B (a fresh "server") reconnects and finds the terminal still alive — the
        // broker outlived the first connection. This is the survive-a-restart property.
        let mut b = BrokerClient::connect(&sock).await.unwrap();
        let list = b.request(&Request::List).await.unwrap();
        match list {
            Response::List { terminals } => {
                assert_eq!(terminals.len(), 1);
                assert_eq!(terminals[0].id, "shared");
                assert!(terminals[0].alive);
            }
            other => panic!("expected List, got {other:?}"),
        }
    }
}
