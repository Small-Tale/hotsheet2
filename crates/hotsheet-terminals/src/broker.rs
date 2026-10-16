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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};

pub use crate::broker_socket::SocketCleanup;
use crate::manager::TerminalManager;
use crate::sizing::ViewportClaim;
use crate::terminal::{TermSpec, TerminalKind};
use crate::viewer_output::{VIEWER_QUEUE_FRAMES, ViewerFrame, pump_viewer_output};
use crate::wire_bytes::{BytesEncoding, WireBytes};

/// Detached brokers exit after this long with no terminals and no connected clients.
pub const DEFAULT_IDLE_GRACE: Duration = Duration::from_secs(5 * 60);

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
    /// Protocol-level liveness check (stronger than merely accepting a socket connection).
    Ping,
    /// Open (or reattach to) a terminal `id` running `command`.
    Open {
        id: String,
        #[serde(default)]
        kind: TerminalKind,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        env: Vec<(String, String)>,
        /// The replay buffer for a newly spawned terminal, in bytes (HS2-Y5DYC0). Older servers omit
        /// it and get [`SCROLLBACK_BYTES`](crate::SCROLLBACK_BYTES); a reattach keeps the size the
        /// terminal was spawned with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replay_bytes: Option<u64>,
    },
    /// List all hosted terminals.
    List,
    /// The terminal's state + scrollback snapshot. `bytes` asks for the scrollback's encoding
    /// (HS2-9R751Y); brokers that predate it ignore the field and answer with an array.
    Read {
        id: String,
        #[serde(default, skip_serializing_if = "BytesEncoding::is_array")]
        bytes: BytesEncoding,
    },
    /// The terminal's state alone, answered with [`Response::Terminal`] (HS2-7RRAA8). Status
    /// pollers use it: a `read` carries the whole 256 KiB scrollback as a JSON number array, and
    /// two of those a second per terminal kept the broker and server busy while idle. Brokers
    /// that predate it answer `Err { "bad request: …" }`, which callers treat as "use `list`".
    Info { id: String },
    /// Write input bytes to the terminal's PTY. Input is small keystroke data, so it stays a
    /// number array that every broker parses.
    Input { id: String, data: Vec<u8> },
    /// Kill + forget the terminal.
    Kill { id: String },
    /// **Switch this connection into streaming mode** for terminal `id` (HS2-ERT00F item 4):
    /// the broker replays the scrollback then streams live output + size decisions as
    /// [`StreamOut`] frames, and reads [`StreamIn`] frames (input + size claims) until the
    /// connection closes. No further request/response ops happen on this connection.
    /// `bytes` asks for the streamed payloads' encoding (HS2-9R751Y); brokers that predate it
    /// ignore the field and stream arrays.
    Attach {
        id: String,
        #[serde(default, skip_serializing_if = "BytesEncoding::is_array")]
        bytes: BytesEncoding,
    },
}

/// A frame the broker streams to an attached client (one JSON line each), after an
/// [`Request::Attach`]. The stream ends when the connection closes (terminal exited or the
/// viewer went away).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum StreamOut {
    /// The initial scrollback replay (and a re-sync snapshot after a lag).
    Scrollback { data: WireBytes },
    /// A live PTY output chunk as it arrives.
    Output { data: WireBytes },
    /// The size arbiter's applied PTY size: sent once right after the initial replay
    /// (HS2-7Y1BQ2) and again whenever it changes.
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
    /// Raw bytes to write to the PTY (keystrokes / a command). A client sends base64 only
    /// after the broker streamed base64 on this connection (see [`BrokerStream::send_input`]).
    Input { data: WireBytes },
    /// This viewport's leased size claim (fed to the arbiter, HS2-BD7Q74).
    Resize {
        viewer_id: String,
        cols: u16,
        rows: u16,
        #[serde(default)]
        focus: bool,
        #[serde(default = "stream_default_true")]
        visible: bool,
        /// This claim was driven by a genuine user interaction rather than a heartbeat; only
        /// interacting claims advance the size-arbiter recency (HS2-3ZBQDG).
        #[serde(default)]
        interacting: bool,
    },
}

fn stream_default_true() -> bool {
    true
}

/// The broker's reply (one JSON line).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    /// Reply to [`Request::Ping`].
    Pong,
    /// One terminal's state (for `open`).
    Terminal { info: BrokerTermInfo },
    /// All terminals (for `list`).
    List { terminals: Vec<BrokerTermInfo> },
    /// State + scrollback (for `read`).
    Read {
        info: BrokerTermInfo,
        scrollback: WireBytes,
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
    /// Immutable creation kind. Older brokers report shell when this field is absent.
    #[serde(default)]
    pub kind: TerminalKind,
    pub alive: bool,
    pub busy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
    /// The session worker id from the terminal's launch environment. Older brokers omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
    /// Whether a command the user started holds the PTY foreground; absent where unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground_command: Option<bool>,
}

fn info_of(term: &crate::terminal::Terminal, id: &str) -> BrokerTermInfo {
    let osc = term.term_state();
    BrokerTermInfo {
        id: id.to_string(),
        kind: term.kind(),
        alive: term.is_alive(),
        busy: term.activity() == crate::busy::Activity::Busy,
        cwd: osc.cwd,
        link: osc.link,
        progress: osc.progress,
        worker: term.worker_id().map(str::to_string),
        foreground_command: term.foreground_command_running(),
    }
}

/// Serve a broker on `listener`, hosting `project`'s terminals in `manager` for the process's
/// lifetime. Each connection is handled concurrently; the shared `manager` (and its PTYs)
/// outlive any one connection — the whole point of the broker.
pub async fn serve_broker(listener: UnixListener, project: String, manager: Arc<TerminalManager>) {
    serve_broker_with_idle(listener, project, manager, None).await;
}

/// Bind and run the detached broker process, removing its socket after idle shutdown.
pub async fn run_broker_process(
    socket: impl AsRef<std::path::Path>,
    project: String,
) -> std::io::Result<()> {
    let socket = socket.as_ref();
    let (listener, _ownership) = crate::broker_socket::bind(socket)?;
    serve_broker_with_idle(
        listener,
        project,
        Arc::new(TerminalManager::new()),
        Some(DEFAULT_IDLE_GRACE),
    )
    .await;
    Ok(())
}

/// Serve with an optional shutdown-when-empty grace period. `None` keeps the historical
/// run-until-killed behavior used by embedded callers; the detached binary supplies a grace.
pub async fn serve_broker_with_idle(
    listener: UnixListener,
    project: String,
    manager: Arc<TerminalManager>,
    idle_grace: Option<Duration>,
) {
    let clients = Arc::new(AtomicUsize::new(0));
    let mut empty_since = tokio::time::Instant::now();
    let tick = idle_grace
        .map(|grace| grace.min(Duration::from_millis(250)))
        .unwrap_or(Duration::from_millis(250));
    let mut housekeeping = tokio::time::interval(tick);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _addr)) = accepted else { continue };
                clients.fetch_add(1, Ordering::SeqCst);
                let (manager, project, clients) =
                    (manager.clone(), project.clone(), clients.clone());
                tokio::spawn(async move {
                    let _ = handle_connection(stream, &project, &manager).await;
                    clients.fetch_sub(1, Ordering::SeqCst);
                });
            }
            _ = housekeeping.tick(), if idle_grace.is_some() => {
                manager.reap();
                if manager.count() == 0 && clients.load(Ordering::SeqCst) == 0 {
                    if empty_since.elapsed() >= idle_grace.unwrap() {
                        break;
                    }
                } else {
                    empty_since = tokio::time::Instant::now();
                }
            }
        }
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
            Ok(Request::Attach { id, bytes }) => {
                return stream_terminal(lines, write, project, manager, id, bytes).await;
            }
            Ok(req) => {
                let resp = handle_request(project, manager, req).await;
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

/// Write an attached client's queued frames as [`StreamOut`] lines until the queue closes
/// (the terminal ended) or the client stops accepting writes.
async fn write_stream_frames(
    mut write: OwnedWriteHalf,
    mut frames: tokio::sync::mpsc::Receiver<ViewerFrame>,
    encoding: BytesEncoding,
) {
    while let Some(frame) = frames.recv().await {
        let line = match frame {
            ViewerFrame::Output(data) => StreamOut::Output {
                data: WireBytes::new(data, encoding),
            },
            ViewerFrame::Replace(data) => StreamOut::Scrollback {
                data: WireBytes::new(data, encoding),
            },
            ViewerFrame::Size(d) => StreamOut::Size {
                cols: d.cols,
                rows: d.rows,
                driven_by: d.driven_by,
            },
        };
        if write_line(&mut write, &line).await.is_err() {
            return;
        }
    }
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
    encoding: BytesEncoding,
) -> std::io::Result<()> {
    let wire = |data: Vec<u8>| WireBytes::new(data, encoding);

    let Some(term) = manager.get(&(project.to_string(), id.clone())) else {
        return write_line(&mut write, &StreamOut::NotFound).await;
    };

    // Snapshot and subscribe under one output boundary so concurrent PTY output lands exactly
    // once in either the replay or the live stream (HS2-5W0V9M).
    let (snapshot, out_rx) = term.subscribe_with_scrollback();
    let size_rx = term.subscribe_size();
    let mut my_viewer: Option<String> = None;

    write_line(
        &mut write,
        &StreamOut::Scrollback {
            data: wire(snapshot),
        },
    )
    .await?;
    // Size frames only report changes, so tell a new viewer the applied size right after the
    // replay; otherwise a stable-size terminal never reports its grid (HS2-7Y1BQ2).
    if let Some(current) = term.current_size() {
        write_line(
            &mut write,
            &StreamOut::Size {
                cols: current.cols,
                rows: current.rows,
                driven_by: current.driven_by,
            },
        )
        .await?;
    }

    // Output and input run on separate tasks joined by a bounded queue, so a server viewer
    // that is slow to read output never delays its input reaching the PTY (HS2-J52ZJ7).
    let (frames_tx, frames_rx) = tokio::sync::mpsc::channel(VIEWER_QUEUE_FRAMES);
    let pump = tokio::spawn(pump_viewer_output(term.clone(), out_rx, size_rx, frames_tx));
    let mut writer = tokio::spawn(write_stream_frames(write, frames_rx, encoding));

    loop {
        tokio::select! {
            // The writer ends when the terminal ended (its queue closed) or the client is gone.
            _ = &mut writer => break,
            line = lines.next_line() => match line {
                Ok(Some(l)) if !l.trim().is_empty() => {
                    if let Ok(inbound) = serde_json::from_str::<StreamIn>(&l) {
                        match inbound {
                            StreamIn::Input { data } => { let _ = term.queue_input(data.into_vec()).await; }
                            StreamIn::Resize { viewer_id, cols, rows, focus, visible, interacting } => {
                                my_viewer = Some(viewer_id.clone());
                                let now = now_ms();
                                term.claim_size(
                                    ViewportClaim { viewer_id, cols, rows, focus, visible, interacting, activity_at_ms: now },
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
    pump.abort();
    writer.abort();

    // Self-heal: drop this viewport's claim so the PTY size recomputes for the rest.
    if let Some(v) = my_viewer {
        term.drop_viewer(&v, now_ms());
    }
    Ok(())
}

/// Run one request against the manager (synchronous — PTY ops are quick, non-blocking).
/// Answer one request. Only `Input` awaits (the PTY write runs on the terminal's writer
/// thread, HS2-FERD7N); the other requests are immediate.
async fn handle_request(project: &str, manager: &Arc<TerminalManager>, req: Request) -> Response {
    let key = |id: &str| (project.to_string(), id.to_string());
    match req {
        Request::Ping => Response::Pong,
        Request::Open {
            id,
            kind,
            command,
            args,
            cwd,
            env,
            replay_bytes,
        } => {
            let spec = TermSpec {
                kind,
                command,
                args,
                cwd: cwd.map(std::path::PathBuf::from),
                env,
                rows: 24,
                cols: 80,
                replay_bytes: replay_bytes.map_or(crate::SCROLLBACK_BYTES, |bytes| {
                    usize::try_from(bytes).unwrap_or(usize::MAX)
                }),
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
        Request::Info { id } => match manager.get(&key(&id)) {
            Some(term) => Response::Terminal {
                info: info_of(&term, &id),
            },
            None => Response::NotFound,
        },
        Request::Read { id, bytes } => match manager.get(&key(&id)) {
            Some(term) => Response::Read {
                info: info_of(&term, &id),
                scrollback: WireBytes::new(term.scrollback(), bytes),
            },
            None => Response::NotFound,
        },
        Request::Input { id, data } => match manager.get(&key(&id)) {
            Some(term) => match term.write_async(data).await {
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
    reader: BrokerStreamReader,
    writer: BrokerStreamWriter,
}

/// The receiving half of a [`BrokerStream`] (HS2-J52ZJ7): read it on one task while another
/// forwards input through the [`BrokerStreamWriter`], so input never waits behind output.
pub struct BrokerStreamReader {
    reader: tokio::io::Lines<BufReader<OwnedReadHalf>>,
    /// Set once the broker streamed a base64 payload on this connection, proving it
    /// understands the encoding (HS2-9R751Y). Shared with the writer half.
    base64_proven: Arc<AtomicBool>,
}

/// The sending half of a [`BrokerStream`] (HS2-J52ZJ7).
pub struct BrokerStreamWriter {
    writer: OwnedWriteHalf,
    base64_proven: Arc<AtomicBool>,
}

impl BrokerStream {
    /// Connect to the broker at `socket_path` and attach to terminal `id`. The first
    /// [`next`](Self::next) frame is the scrollback replay (or [`StreamOut::NotFound`]).
    /// Payloads are requested as base64; a broker that predates the encoding streams arrays,
    /// which decode the same way.
    pub async fn open(socket_path: &Path, id: &str) -> std::io::Result<Self> {
        Self::open_with(socket_path, id, BytesEncoding::Base64).await
    }

    /// [`open`](Self::open) asking for an explicit payload encoding.
    pub async fn open_with(
        socket_path: &Path,
        id: &str,
        bytes: BytesEncoding,
    ) -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket_path).await?;
        let (read, mut writer) = stream.into_split();
        let mut line = serde_json::to_string(&Request::Attach {
            id: id.to_string(),
            bytes,
        })
        .map_err(std::io::Error::other)?;
        line.push('\n');
        writer.write_all(line.as_bytes()).await?;
        writer.flush().await?;
        let base64_proven = Arc::new(AtomicBool::new(false));
        Ok(Self {
            reader: BrokerStreamReader {
                reader: BufReader::new(read).lines(),
                base64_proven: base64_proven.clone(),
            },
            writer: BrokerStreamWriter {
                writer,
                base64_proven,
            },
        })
    }

    /// Split into independently usable receiving and sending halves (HS2-J52ZJ7).
    pub fn into_split(self) -> (BrokerStreamReader, BrokerStreamWriter) {
        (self.reader, self.writer)
    }

    /// The next streamed frame, or `None` when the broker closed the stream (terminal ended /
    /// broker gone).
    pub async fn next(&mut self) -> std::io::Result<Option<StreamOut>> {
        self.reader.next().await
    }

    /// The encoding [`send_input`](Self::send_input) uses right now.
    pub fn input_encoding(&self) -> BytesEncoding {
        self.writer.input_encoding()
    }

    /// Forward PTY input, as base64 once the broker proved it speaks it and as a number
    /// array before that (so a broker older than the encoding never sees a string).
    pub async fn send_input(&mut self, data: Vec<u8>) -> std::io::Result<()> {
        self.writer.send_input(data).await
    }

    /// Forward one input / size-claim frame to the broker.
    pub async fn send(&mut self, frame: &StreamIn) -> std::io::Result<()> {
        self.writer.send(frame).await
    }
}

impl BrokerStreamReader {
    /// The next streamed frame, or `None` when the broker closed the stream. Cancel-safe, so
    /// it can sit in a `select!`.
    pub async fn next(&mut self) -> std::io::Result<Option<StreamOut>> {
        loop {
            match self.reader.next_line().await? {
                Some(l) if l.trim().is_empty() => continue,
                Some(l) => {
                    let frame: StreamOut =
                        serde_json::from_str(&l).map_err(std::io::Error::other)?;
                    if let StreamOut::Scrollback { data } | StreamOut::Output { data } = &frame
                        && data.encoding() == BytesEncoding::Base64
                    {
                        self.base64_proven.store(true, Ordering::Release);
                    }
                    return Ok(Some(frame));
                }
                None => return Ok(None),
            }
        }
    }
}

impl BrokerStreamWriter {
    /// The encoding [`send_input`](Self::send_input) uses right now.
    pub fn input_encoding(&self) -> BytesEncoding {
        if self.base64_proven.load(Ordering::Acquire) {
            BytesEncoding::Base64
        } else {
            BytesEncoding::Array
        }
    }

    /// Forward PTY input, as base64 once the broker proved it speaks it and as a number
    /// array before that (so a broker older than the encoding never sees a string).
    pub async fn send_input(&mut self, data: Vec<u8>) -> std::io::Result<()> {
        let frame = StreamIn::Input {
            data: WireBytes::new(data, self.input_encoding()),
        };
        self.send(&frame).await
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

    #[test]
    fn replay_size_is_optional_on_the_wire_and_reaches_the_spawned_terminal() {
        // HS2-Y5DYC0: an older server omits the size (the broker keeps 256 KiB); a newer one
        // omits it only when unset, so an older broker never sees an unknown field.
        let legacy: Request =
            serde_json::from_str(r#"{"op":"open","id":"t","command":"cat"}"#).unwrap();
        assert!(matches!(
            legacy,
            Request::Open {
                replay_bytes: None,
                ..
            }
        ));
        let open = |replay_bytes| Request::Open {
            id: "sized".into(),
            kind: TerminalKind::Shell,
            command: "cat".into(),
            args: vec![],
            cwd: None,
            env: vec![],
            replay_bytes,
        };
        assert!(
            serde_json::to_value(open(None))
                .unwrap()
                .get("replay_bytes")
                .is_none()
        );
        let sized = serde_json::to_value(open(Some(512 * 1024))).unwrap();
        assert_eq!(sized["replay_bytes"], 512 * 1024);
        let manager = Arc::new(TerminalManager::new());
        // `handle_request` is async since input awaits the terminal's writer thread (HS2-FERD7N).
        let response = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(handle_request(
                "proj",
                &manager,
                serde_json::from_value(sized).unwrap(),
            ));
        assert!(matches!(response, Response::Terminal { .. }));
        let term = manager.get(&("proj".into(), "sized".into())).unwrap();
        assert_eq!(term.replay_capacity(), 512 * 1024);
        term.kill().unwrap();
    }

    #[test]
    fn creation_kind_round_trips_and_legacy_broker_messages_default_to_shell() {
        let legacy: Request =
            serde_json::from_str(r#"{"op":"open","id":"legacy","command":"claude"}"#).unwrap();
        assert!(matches!(
            legacy,
            Request::Open {
                kind: TerminalKind::Shell,
                ..
            }
        ));
        let legacy: BrokerTermInfo = serde_json::from_str(
            r#"{"id":"legacy","alive":true,"busy":false,"link":"https://ai.example"}"#,
        )
        .unwrap();
        assert_eq!(legacy.kind, TerminalKind::Shell);
        assert_eq!(serde_json::to_value(legacy).unwrap()["kind"], "shell");
        let ai: Request =
            serde_json::from_str(r#"{"op":"open","id":"ai","command":"cat","kind":"ai"}"#).unwrap();
        assert!(matches!(
            ai,
            Request::Open {
                kind: TerminalKind::Ai,
                ..
            }
        ));
        assert_eq!(serde_json::to_value(ai).unwrap()["kind"], "ai");
        assert!(
            serde_json::from_str::<Request>(
                r#"{"op":"open","id":"bad","command":"cat","kind":"link"}"#,
            )
            .is_err()
        );
    }

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
                kind: TerminalKind::Shell,
                command: "cat".into(),
                args: vec![],
                cwd: None,
                env: vec![],
                replay_bytes: None,
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
            .request(&Request::Read {
                id: "t1".into(),
                bytes: BytesEncoding::Base64,
            })
            .await
            .unwrap();
        match read {
            Response::Read { scrollback, .. } => {
                assert_eq!(scrollback.encoding(), BytesEncoding::Base64);
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
                .request(&Request::Read {
                    id: "t1".into(),
                    bytes: BytesEncoding::Array,
                })
                .await
                .unwrap(),
            Response::NotFound
        ));
        assert!(matches!(
            client
                .request(&Request::Info { id: "t1".into() })
                .await
                .unwrap(),
            Response::NotFound
        ));
    }

    /// The status op a poller sends twice a second stays a few hundred bytes however full the
    /// scrollback is, unlike `read`, which re-encodes the whole ring each time (HS2-7RRAA8).
    #[tokio::test]
    async fn info_reports_state_without_shipping_the_scrollback() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let manager = Arc::new(TerminalManager::new());
        tokio::spawn(serve_broker(listener, "proj".into(), manager.clone()));
        let mut client = BrokerClient::connect(&sock).await.unwrap();
        client
            .request(&Request::Open {
                id: "big".into(),
                kind: TerminalKind::Shell,
                command: "cat".into(),
                args: vec![],
                cwd: None,
                env: vec![],
                replay_bytes: None,
            })
            .await
            .unwrap();
        // Short lines: a canonical-mode PTY caps one input line (1 KiB on macOS).
        for _ in 0..200 {
            let mut line = vec![b'x'; 200];
            line.push(b'\n');
            client
                .request(&Request::Input {
                    id: "big".into(),
                    data: line,
                })
                .await
                .unwrap();
        }
        assert!(
            wait_until(
                || {
                    manager
                        .get(&("proj".into(), "big".into()))
                        .is_some_and(|t| t.scrollback().len() > 32 * 1024)
                },
                10,
            )
            .await,
            "the echo fills the scrollback"
        );
        // Measure the raw reply lines: the cost is in their encoded size.
        let reply = |op: &'static str| {
            let sock = sock.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
                let (read, mut write) = stream.into_split();
                let request = serde_json::json!({ "op": op, "id": "big" }).to_string() + "\n";
                write.write_all(request.as_bytes()).await.unwrap();
                let mut line = String::new();
                tokio::io::BufReader::new(read)
                    .read_line(&mut line)
                    .await
                    .unwrap();
                line
            }
        };
        let info = reply("info").await;
        let read = reply("read").await;
        assert!(info.len() < 1024, "info stays small: {} bytes", info.len());
        assert!(read.len() > 32 * 1024, "read carries the scrollback");
        match serde_json::from_str::<Response>(&info).unwrap() {
            Response::Terminal { info } => assert!(info.alive && info.id == "big"),
            other => panic!("expected Terminal, got {other:?}"),
        }
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
                kind: TerminalKind::Shell,
                command: "cat".into(),
                args: vec![],
                cwd: None,
                env: vec![],
                replay_bytes: None,
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
                data: b"live-echo\n".to_vec().into(),
            })
            .await
            .unwrap();
        let saw_live = wait_until_stream(&mut stream, &mut seen, "live-echo", 5).await;
        assert!(
            saw_live,
            "input sent over the attach reaches the PTY and streams back as live output"
        );
    }

    /// A viewer that attaches to a terminal whose size is stable still learns the applied PTY
    /// size: the stream sends it right after the replay, and a second viewer sees the size the
    /// first viewer's claim drove without making a claim of its own (HS2-7Y1BQ2).
    #[tokio::test]
    async fn attach_reports_the_applied_size_right_after_the_replay() {
        async fn next(stream: &mut BrokerStream) -> StreamOut {
            tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .expect("frame in time")
                .expect("stream read")
                .expect("stream open")
        }
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        tokio::spawn(serve_broker(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
        ));
        let mut client = BrokerClient::connect(&sock).await.unwrap();
        client
            .request(&Request::Open {
                id: "sized".into(),
                kind: TerminalKind::Shell,
                command: "cat".into(),
                args: vec![],
                cwd: None,
                env: vec![],
                replay_bytes: None,
            })
            .await
            .unwrap();

        // First viewer: the replay, then the spawn size with no driver.
        let mut first = BrokerStream::open(&sock, "sized").await.unwrap();
        assert!(matches!(
            next(&mut first).await,
            StreamOut::Scrollback { .. }
        ));
        match next(&mut first).await {
            StreamOut::Size {
                cols,
                rows,
                driven_by,
            } => {
                assert_eq!((cols, rows, driven_by), (80, 24, None))
            }
            other => panic!("expected the applied size after the replay, got {other:?}"),
        }

        // The first viewer drives the PTY to 132x40 and sees the change.
        first
            .send(&StreamIn::Resize {
                viewer_id: "wide".into(),
                cols: 132,
                rows: 40,
                focus: true,
                visible: true,
                interacting: true,
            })
            .await
            .unwrap();
        loop {
            if let StreamOut::Size { cols, .. } = next(&mut first).await {
                assert_eq!(cols, 132);
                break;
            }
        }

        // A second, non-claiming viewer gets the driven size and its driver on attach.
        let mut second = BrokerStream::open(&sock, "sized").await.unwrap();
        assert!(matches!(
            next(&mut second).await,
            StreamOut::Scrollback { .. }
        ));
        match next(&mut second).await {
            StreamOut::Size {
                cols,
                rows,
                driven_by,
            } => {
                assert_eq!((cols, rows, driven_by.as_deref()), (132, 40, Some("wide")))
            }
            other => panic!("expected the applied size after the replay, got {other:?}"),
        }
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
                    kind: TerminalKind::Ai,
                    command: "cat".into(),
                    args: vec![],
                    cwd: None,
                    env: vec![],
                    replay_bytes: None,
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
                assert_eq!(terminals[0].kind, TerminalKind::Ai);
                assert!(terminals[0].alive);
            }
            other => panic!("expected List, got {other:?}"),
        }
        // A restarted server's ordinary reattach cannot turn the AI terminal into a shell.
        let reopened = b
            .request(&Request::Open {
                id: "shared".into(),
                kind: TerminalKind::Shell,
                command: "cat".into(),
                args: vec![],
                cwd: None,
                env: vec![],
                replay_bytes: None,
            })
            .await
            .unwrap();
        assert!(matches!(reopened, Response::Terminal { info } if info.kind == TerminalKind::Ai));
        b.request(&Request::Kill {
            id: "shared".into(),
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn ping_pong_is_a_real_protocol_health_check() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        tokio::spawn(serve_broker(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
        ));
        let mut client = BrokerClient::connect(&sock).await.unwrap();
        assert!(matches!(
            client.request(&Request::Ping).await.unwrap(),
            Response::Pong
        ));
    }

    #[tokio::test]
    async fn idle_gc_waits_for_clients_then_exits_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let task = tokio::spawn(serve_broker_with_idle(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
            Some(Duration::from_millis(80)),
        ));
        let client = BrokerClient::connect(&sock).await.unwrap();
        tokio::time::sleep(Duration::from_millis(180)).await;
        assert!(
            !task.is_finished(),
            "a connected client keeps the broker alive"
        );
        drop(client);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("empty broker exits after the grace")
            .unwrap();
    }

    #[tokio::test]
    async fn idle_gc_resets_while_a_terminal_exists_then_exits_after_kill() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let task = tokio::spawn(serve_broker_with_idle(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
            Some(Duration::from_millis(80)),
        ));
        let mut client = BrokerClient::connect(&sock).await.unwrap();
        client
            .request(&Request::Open {
                id: "keeps-alive".into(),
                kind: TerminalKind::Shell,
                command: "cat".into(),
                args: vec![],
                cwd: None,
                env: vec![],
                replay_bytes: None,
            })
            .await
            .unwrap();
        drop(client);
        tokio::time::sleep(Duration::from_millis(180)).await;
        assert!(!task.is_finished(), "a hosted terminal resets idle GC");

        let mut client = BrokerClient::connect(&sock).await.unwrap();
        assert!(matches!(
            client
                .request(&Request::Kill {
                    id: "keeps-alive".into()
                })
                .await
                .unwrap(),
            Response::Ok
        ));
        drop(client);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("broker exits after its final terminal is killed")
            .unwrap();
    }

    #[test]
    fn socket_cleanup_removes_the_socket_path_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("broker.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        drop(SocketCleanup::new(&socket));
        assert!(!socket.exists());
    }

    // ---- HS2-9R751Y: byte-encoding version skew ---------------------------------------

    async fn broker_with_cat(id: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        tokio::spawn(serve_broker(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
        ));
        let mut client = BrokerClient::connect(&sock).await.unwrap();
        client
            .request(&Request::Open {
                id: id.into(),
                kind: TerminalKind::Shell,
                command: "cat".into(),
                args: vec![],
                cwd: None,
                env: vec![],
                replay_bytes: None,
            })
            .await
            .unwrap();
        client
            .request(&Request::Input {
                id: id.into(),
                data: b"seed\n".to_vec(),
            })
            .await
            .unwrap();
        (dir, sock)
    }

    /// A raw JSON-line peer: what a server older than the encoding sends and reads.
    struct RawPeer {
        lines: tokio::io::Lines<BufReader<OwnedReadHalf>>,
        writer: OwnedWriteHalf,
    }

    impl RawPeer {
        async fn connect(sock: &Path) -> Self {
            let (read, writer) = UnixStream::connect(sock).await.unwrap().into_split();
            Self {
                lines: BufReader::new(read).lines(),
                writer,
            }
        }

        async fn send(&mut self, line: &str) {
            self.writer.write_all(line.as_bytes()).await.unwrap();
            self.writer.write_all(b"\n").await.unwrap();
        }

        async fn next(&mut self) -> serde_json::Value {
            let line = tokio::time::timeout(Duration::from_secs(5), self.lines.next_line())
                .await
                .expect("line in time")
                .unwrap()
                .expect("open");
            serde_json::from_str(&line).unwrap()
        }

        /// Collect array-encoded output until `needle` appears.
        async fn read_array_output_until(&mut self, needle: &str) -> Vec<u8> {
            let mut acc = Vec::new();
            while !String::from_utf8_lossy(&acc).contains(needle) {
                let frame = self.next().await;
                if let Some(data) = frame.get("data") {
                    let bytes = data.as_array().expect("an old peer only ever sees arrays");
                    acc.extend(bytes.iter().map(|b| b.as_u64().unwrap() as u8));
                }
            }
            acc
        }
    }

    #[tokio::test]
    async fn an_old_server_without_an_encoding_gets_arrays_and_its_array_input_works() {
        let (_dir, sock) = broker_with_cat("legacy").await;
        let mut old = RawPeer::connect(&sock).await;
        old.send(r#"{"op":"read","id":"legacy"}"#).await;
        let read = old.next().await;
        assert!(read["scrollback"].is_array(), "{read}");

        let mut old = RawPeer::connect(&sock).await;
        old.send(r#"{"op":"attach","id":"legacy"}"#).await;
        old.read_array_output_until("seed").await;
        let input: Vec<u8> = b"old-input\n".to_vec();
        old.send(&format!(
            r#"{{"frame":"input","data":{}}}"#,
            serde_json::to_string(&input).unwrap()
        ))
        .await;
        old.read_array_output_until("old-input").await;
    }

    #[tokio::test]
    async fn a_new_server_streams_base64_and_switches_input_after_the_first_base64_frame() {
        let (_dir, sock) = broker_with_cat("compact").await;
        let mut stream = BrokerStream::open(&sock, "compact").await.unwrap();
        assert_eq!(stream.input_encoding(), BytesEncoding::Array);
        match stream.next().await.unwrap() {
            Some(StreamOut::Scrollback { data }) => {
                assert_eq!(data.encoding(), BytesEncoding::Base64)
            }
            other => panic!("expected the replay first, got {other:?}"),
        }
        assert_eq!(stream.input_encoding(), BytesEncoding::Base64);
        stream.send_input(b"new-input\n".to_vec()).await.unwrap();
        let mut seen = Vec::new();
        assert!(wait_until_stream(&mut stream, &mut seen, "new-input", 5).await);

        // A new broker still decodes array input mid-stream (a peer that has not switched).
        stream
            .send(&StreamIn::Input {
                data: b"array-input\n".to_vec().into(),
            })
            .await
            .unwrap();
        assert!(wait_until_stream(&mut stream, &mut seen, "array-input", 5).await);
    }

    /// An old broker ignores the requested encoding and streams arrays. The new client
    /// decodes them and never sends it a base64 string it could not parse.
    #[tokio::test]
    async fn a_new_server_against_an_old_array_only_broker_keeps_input_as_arrays() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("old-broker.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let old_broker = tokio::spawn(async move {
            let (conn, _) = listener.accept().await.unwrap();
            let (read, mut write) = conn.into_split();
            let mut lines = BufReader::new(read).lines();
            let attach = lines.next_line().await.unwrap().unwrap();
            // The old broker's Request::Attach has only `id`; serde ignores the new field.
            #[derive(Deserialize)]
            #[serde(tag = "op", rename_all = "snake_case")]
            enum OldRequest {
                Attach { id: String },
            }
            let OldRequest::Attach { id } = serde_json::from_str(&attach).unwrap();
            assert_eq!(id, "t");
            write
                .write_all(b"{\"frame\":\"scrollback\",\"data\":[104,105]}\n")
                .await
                .unwrap();
            lines.next_line().await.unwrap().unwrap()
        });

        let mut stream = BrokerStream::open(&sock, "t").await.unwrap();
        match stream.next().await.unwrap() {
            Some(StreamOut::Scrollback { data }) => {
                assert_eq!(
                    (&*data, data.encoding()),
                    (&b"hi"[..], BytesEncoding::Array)
                );
            }
            other => panic!("expected the replay, got {other:?}"),
        }
        assert_eq!(stream.input_encoding(), BytesEncoding::Array);
        stream.send_input(b"x".to_vec()).await.unwrap();
        let input: serde_json::Value = serde_json::from_str(&old_broker.await.unwrap()).unwrap();
        assert_eq!(input, serde_json::json!({"frame": "input", "data": [120]}));
    }
}
