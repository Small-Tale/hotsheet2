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
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};

use crate::manager::TerminalManager;
use crate::sizing::ViewportClaim;
use crate::terminal::TermSpec;

/// A real millis clock for the broker process (the size arbiter is deterministic given it).
/// Distinct from the terminal's injected test clock — the broker runs as its own process.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

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
    /// **Switch this connection into streaming mode** for terminal `id` (HS2-ERT00F item 4):
    /// the broker replays the scrollback then streams live output + size decisions as
    /// [`StreamOut`] frames, and reads [`StreamIn`] frames (input + size claims) until the
    /// connection closes. No further request/response ops happen on this connection.
    Attach { id: String },
}

/// A frame the broker streams to an attached client (one JSON line each), after an
/// [`Request::Attach`]. The stream ends when the connection closes (terminal exited or the
/// viewer went away).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum StreamOut {
    /// The initial scrollback replay (and a re-sync snapshot after a lag).
    Scrollback { data: Vec<u8> },
    /// A live PTY output chunk as it arrives.
    Output { data: Vec<u8> },
    /// The size arbiter's chosen PTY size changed.
    Size {
        cols: u16,
        rows: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        driven_by: Option<String>,
    },
    /// No terminal with that id (attach failed).
    NotFound,
    /// The attach failed.
    Err { message: String },
}

/// A frame an attached client streams to the broker (one JSON line each).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum StreamIn {
    /// Raw bytes to write to the PTY (keystrokes / a command).
    Input { data: Vec<u8> },
    /// This viewport's leased size claim (fed to the arbiter, HS2-BD7Q74).
    Resize {
        viewer_id: String,
        cols: u16,
        rows: u16,
        #[serde(default)]
        focus: bool,
        #[serde(default = "stream_default_true")]
        visible: bool,
    },
}

fn stream_default_true() -> bool {
    true
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
        match serde_json::from_str::<Request>(&line) {
            // Attach switches this connection into streaming mode for the rest of its life.
            Ok(Request::Attach { id }) => {
                return stream_terminal(lines, write, project, manager, id).await;
            }
            Ok(req) => {
                let resp = handle_request(project, manager, req);
                write_line(&mut write, &resp).await?;
            }
            Err(e) => {
                write_line(
                    &mut write,
                    &Response::Err {
                        message: format!("bad request: {e}"),
                    },
                )
                .await?;
            }
        }
    }
    Ok(())
}

/// Serialize `frame` as one JSON line and flush it.
async fn write_line<T: Serialize>(write: &mut OwnedWriteHalf, frame: &T) -> std::io::Result<()> {
    let mut out = serde_json::to_string(frame).map_err(std::io::Error::other)?;
    out.push('\n');
    write.write_all(out.as_bytes()).await?;
    write.flush().await
}

/// Stream a terminal's live output + size decisions to an attached client, and apply the
/// input + size claims it sends back, until the connection closes (HS2-ERT00F item 4). On
/// disconnect the viewport's size claim is dropped so the PTY size self-heals for the rest —
/// broker-side, so it holds even across a server restart.
async fn stream_terminal(
    mut lines: tokio::io::Lines<BufReader<OwnedReadHalf>>,
    mut write: OwnedWriteHalf,
    project: &str,
    manager: &Arc<TerminalManager>,
    id: String,
) -> std::io::Result<()> {
    use tokio::sync::broadcast::error::RecvError;

    let Some(term) = manager.get(&(project.to_string(), id.clone())) else {
        return write_line(&mut write, &StreamOut::NotFound).await;
    };

    // Subscribe BEFORE snapshotting so no chunk is lost between the snapshot and the stream.
    let mut out_rx = term.subscribe();
    let mut size_rx = term.subscribe_size();
    let mut my_viewer: Option<String> = None;

    write_line(
        &mut write,
        &StreamOut::Scrollback {
            data: term.scrollback(),
        },
    )
    .await?;

    loop {
        tokio::select! {
            output = out_rx.recv() => match output {
                Ok(data) => write_line(&mut write, &StreamOut::Output { data }).await?,
                // Fell behind the fan-out — re-sync from a fresh snapshot.
                Err(RecvError::Lagged(_)) => {
                    write_line(&mut write, &StreamOut::Scrollback { data: term.scrollback() }).await?;
                }
                Err(RecvError::Closed) => break, // terminal ended
            },
            size = size_rx.recv() => match size {
                Ok(d) => {
                    write_line(&mut write, &StreamOut::Size {
                        cols: d.cols, rows: d.rows, driven_by: d.driven_by,
                    }).await?;
                }
                Err(RecvError::Lagged(_)) => {} // a missed size update self-corrects on the next claim
                Err(RecvError::Closed) => break,
            },
            line = lines.next_line() => match line {
                Ok(Some(l)) if !l.trim().is_empty() => {
                    if let Ok(inbound) = serde_json::from_str::<StreamIn>(&l) {
                        match inbound {
                            StreamIn::Input { data } => { let _ = term.write(&data); }
                            StreamIn::Resize { viewer_id, cols, rows, focus, visible } => {
                                my_viewer = Some(viewer_id.clone());
                                let now = now_ms();
                                term.claim_size(
                                    ViewportClaim { viewer_id, cols, rows, focus, visible, activity_at_ms: now },
                                    now,
                                );
                            }
                        }
                    }
                }
                Ok(Some(_)) => {} // empty keep-alive line
                Ok(None) | Err(_) => break, // client (server viewer) closed
            },
        }
    }

    // Self-heal: drop this viewport's claim so the PTY size recomputes for the rest.
    if let Some(v) = my_viewer {
        term.drop_viewer(&v, now_ms());
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
        // Attach is intercepted in `handle_connection` (it switches the connection into
        // streaming mode); it never reaches the request/response dispatch.
        Request::Attach { .. } => Response::Err {
            message: "attach is a streaming op, not a request/response op".into(),
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

/// A **streaming** attach connection to a running broker (HS2-ERT00F item 4): after
/// [`open`](Self::open) sends the attach request, the broker streams [`StreamOut`] frames
/// (scrollback replay, then live output + size decisions), and [`send`](Self::send) forwards
/// [`StreamIn`] frames (input + size claims). The server bridges this to a WebSocket viewer.
pub struct BrokerStream {
    reader: tokio::io::Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
}

impl BrokerStream {
    /// Connect to the broker at `socket_path` and attach to terminal `id`. The first
    /// [`next`](Self::next) frame is the scrollback replay (or [`StreamOut::NotFound`]).
    pub async fn open(socket_path: &Path, id: &str) -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket_path).await?;
        let (read, mut writer) = stream.into_split();
        let mut line = serde_json::to_string(&Request::Attach { id: id.to_string() })
            .map_err(std::io::Error::other)?;
        line.push('\n');
        writer.write_all(line.as_bytes()).await?;
        writer.flush().await?;
        Ok(Self {
            reader: BufReader::new(read).lines(),
            writer,
        })
    }

    /// The next streamed frame, or `None` when the broker closed the stream (terminal ended /
    /// broker gone).
    pub async fn next(&mut self) -> std::io::Result<Option<StreamOut>> {
        loop {
            match self.reader.next_line().await? {
                Some(l) if l.trim().is_empty() => continue,
                Some(l) => {
                    return serde_json::from_str(&l)
                        .map(Some)
                        .map_err(std::io::Error::other);
                }
                None => return Ok(None),
            }
        }
    }

    /// Forward one input / size-claim frame to the broker.
    pub async fn send(&mut self, frame: &StreamIn) -> std::io::Result<()> {
        let mut line = serde_json::to_string(frame).map_err(std::io::Error::other)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.flush().await
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
    async fn attach_streams_scrollback_then_live_output_and_forwards_input() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let manager = Arc::new(TerminalManager::new());
        tokio::spawn(serve_broker(listener, "proj".into(), manager));

        // Open a `cat` terminal via the request/response client, and seed some scrollback.
        let mut client = BrokerClient::connect(&sock).await.unwrap();
        client
            .request(&Request::Open {
                id: "s1".into(),
                command: "cat".into(),
                args: vec![],
                cwd: None,
            })
            .await
            .unwrap();
        client
            .request(&Request::Input {
                id: "s1".into(),
                data: b"seed-line\n".to_vec(),
            })
            .await
            .unwrap();

        // Attach a streaming client. The first frame(s) replay the scrollback.
        let mut stream = BrokerStream::open(&sock, "s1").await.unwrap();
        let mut seen = Vec::new();
        // Drain frames for up to a few seconds, collecting output, until we've seen the seed.
        let saw_seed = wait_until_stream(&mut stream, &mut seen, "seed-line", 5).await;
        assert!(saw_seed, "the attach replays the scrollback (seed-line)");

        // Now stream NEW input through the attach connection; `cat` echoes it back as live output.
        stream
            .send(&StreamIn::Input {
                data: b"live-echo\n".to_vec(),
            })
            .await
            .unwrap();
        let saw_live = wait_until_stream(&mut stream, &mut seen, "live-echo", 5).await;
        assert!(
            saw_live,
            "input sent over the attach reaches the PTY and streams back as live output"
        );
    }

    #[tokio::test]
    async fn attach_to_a_missing_terminal_yields_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        tokio::spawn(serve_broker(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
        ));

        let mut stream = BrokerStream::open(&sock, "ghost").await.unwrap();
        let first = stream.next().await.unwrap();
        assert!(
            matches!(first, Some(StreamOut::NotFound)),
            "attaching to a missing terminal streams NotFound, got {first:?}"
        );
    }

    /// Poll a stream for frames, accumulating output text, until `needle` appears or `secs`
    /// elapse. Reads with a short timeout so it doesn't block forever on a quiet stream.
    async fn wait_until_stream(
        stream: &mut BrokerStream,
        acc: &mut Vec<u8>,
        needle: &str,
        secs: u64,
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if String::from_utf8_lossy(acc).contains(needle) {
                return true;
            }
            match tokio::time::timeout(Duration::from_millis(200), stream.next()).await {
                Ok(Ok(Some(StreamOut::Scrollback { data })))
                | Ok(Ok(Some(StreamOut::Output { data }))) => acc.extend_from_slice(&data),
                Ok(Ok(Some(_))) => {} // size/other frames — ignore
                Ok(Ok(None)) | Ok(Err(_)) => return false, // stream closed
                Err(_) => {}          // read timeout — loop and re-check
            }
        }
        String::from_utf8_lossy(acc).contains(needle)
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
