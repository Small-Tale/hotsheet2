//! The **real** [`AppServerClient`] — a live JSON-RPC client for the Codex app-server
//! (`docs/13` §13.5, HS2-112). It speaks the `codex 0.148` protocol
//! (`codex app-server generate-ts`): `initialize` → `initialized`, `thread/start` /
//! `thread/resume`, `turn/start` with text input, observing `turn/started` →
//! `turn/completed` notifications, and `turn/interrupt`.
//!
//! It drives a **persistent** codex the HS1 way — a turn on a running (or resumed)
//! thread, never a fresh process per play. The bytes go over an injected
//! [`RpcTransport`]: [`StdioTransport`] runs `codex app-server` direct (one persistent
//! process per connection — the live-verified default), while [`UdsWsTransport`] speaks the
//! **shared daemon**'s WebSocket control socket so many connections reuse one codex
//! instance (HS2-115). Tests inject a **scripted daemon** (both a loopback and a scripted
//! WebSocket over a temp socket), so the whole protocol engine ([`CodexRpc`],
//! [`CodexAppServer`], [`CodexTurn`]) is exercised with no live `codex` (`docs/05` §5.10,
//! the load-bearing testability rule).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::permission::{Decision, Outcome, PermissionBridge, Scope};
use crate::ports::{
    AppServerClient, AppServerError, AppServerOutcome, AppServerTurn, RpcReader, RpcTransport,
    RpcWriter,
};
use crate::procio::StreamChild;

/// How a Codex approval ServerRequest is decided (`docs/05` §5.7, HS2-113): the shared
/// [`PermissionBridge`] answers from its allow-rules; anything it can't auto-resolve falls
/// back to `default` (the headless policy — a real human round-trip over the bridge's
/// WebSocket transport is the remaining piece, HS2-0QGW07 follow-on).
pub struct PermissionPolicy {
    pub bridge: Arc<Mutex<PermissionBridge>>,
    pub default: Decision,
}

/// How long to wait for a single request's response before giving up.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a turn may run before we stop waiting for its `turn/completed`.
const TURN_TIMEOUT: Duration = Duration::from_secs(600);

// ---- the JSON-RPC engine ---------------------------------------------------------

/// Shared state behind the client: the write half plus the demultiplexed inbound
/// messages a background reader thread routes into.
struct Inner {
    writer: Mutex<Box<dyn RpcWriter>>,
    next_id: AtomicI64,
    /// In-flight requests awaiting a response, keyed by request id.
    pending: Mutex<HashMap<i64, Sender<Value>>>,
    /// The running notification log; turns scan it from their own cursor.
    notes: Mutex<Vec<Value>>,
    cvar: Condvar,
    closed: AtomicBool,
    /// How approval ServerRequests are decided; `None` = permissively auto-approve (the
    /// back-compat default, and what the isolated headless launch expects).
    permission: Mutex<Option<PermissionPolicy>>,
}

impl Inner {
    fn send_raw(&self, msg: &str) -> Result<(), AppServerError> {
        self.writer
            .lock()
            .unwrap()
            .send(msg)
            .map_err(|e| AppServerError::Unavailable(e.to_string()))
    }

    /// Send a request and block for its response (or a timeout / closed connection).
    fn request(&self, method: &str, params: Value) -> Result<Value, AppServerError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(AppServerError::Unavailable("connection closed".into()));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);

        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(e) = self.send_raw(&msg.to_string()) {
            self.pending.lock().unwrap().remove(&id);
            return Err(e);
        }

        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(resp) => {
                if let Some(err) = resp.get("error") {
                    return Err(AppServerError::Protocol(format!("{method}: {err}")));
                }
                Ok(resp.get("result").cloned().unwrap_or(Value::Null))
            }
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                if self.closed.load(Ordering::SeqCst) {
                    Err(AppServerError::Unavailable("connection closed".into()))
                } else {
                    Err(AppServerError::Protocol(format!(
                        "timeout waiting for {method} response"
                    )))
                }
            }
        }
    }

    /// Send a notification (no response expected).
    fn notify(&self, method: &str, params: Value) -> Result<(), AppServerError> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.send_raw(&msg.to_string())
    }

    fn notes_len(&self) -> usize {
        self.notes.lock().unwrap().len()
    }

    /// Route one inbound message: a response to a pending request, a server request
    /// (approval — auto-answered headless), or a notification (appended to the log).
    fn on_message(&self, line: &str) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return; // ignore unframed / non-JSON noise
        };
        let has_method = v.get("method").and_then(Value::as_str).is_some();
        let has_id = v.get("id").is_some();

        if has_method && has_id {
            self.auto_answer_server_request(&v);
        } else if has_method {
            let mut notes = self.notes.lock().unwrap();
            notes.push(v);
            self.cvar.notify_all();
        } else if let Some(id) = v.get("id").and_then(Value::as_i64) {
            if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                let _ = tx.send(v);
            }
        }
    }

    /// Headless approval policy is `never`, so approval ServerRequests shouldn't fire;
    /// if one does (e.g. an MCP elicitation), answer permissively so the turn can't hang.
    fn auto_answer_server_request(&self, v: &Value) {
        let Some(id) = v.get("id") else { return };
        let method = v.get("method").and_then(Value::as_str).unwrap_or("");
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let policy = self.permission.lock().unwrap();
        let result = decide_approval(policy.as_ref(), method, &params);
        let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        let _ = self.send_raw(&reply.to_string());
    }

    fn mark_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
        // Wake any turn blocked in `wait_for_completed` so it can observe the close.
        let _guard = self.notes.lock().unwrap();
        self.cvar.notify_all();
    }
}

/// Decide a Codex approval ServerRequest (`docs/05` §5.7). Without a policy, auto-approve
/// (back-compat — what the isolated headless launch expects). With one, the
/// [`PermissionBridge`] answers from its allow-rules and anything it can't auto-resolve
/// falls back to the policy `default`. An unknown request method → empty result (no-op).
fn decide_approval(policy: Option<&PermissionPolicy>, method: &str, params: &Value) -> Value {
    let Some((yes, no)) = approval_tokens(method) else {
        return json!({}); // not an approval we recognize → empty result
    };
    let decision = match policy {
        None => Decision::Allow,
        Some(p) => resolve_decision(p, method, params),
    };
    let tok = if decision == Decision::Allow { yes } else { no };
    json!({ "decision": tok })
}

/// The (approve, deny) reply tokens for each known approval method.
fn approval_tokens(method: &str) -> Option<(&'static str, &'static str)> {
    match method {
        "execCommandApproval" | "applyPatchApproval" => Some(("approved", "denied")),
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            Some(("accept", "reject"))
        }
        _ => None,
    }
}

/// Consult the bridge: an allow-rule auto-resolves; a pending request (no live human
/// transport yet) falls back to the policy default and is resolved so the queue can't leak.
fn resolve_decision(p: &PermissionPolicy, method: &str, params: &Value) -> Decision {
    let action = approval_action(params);
    let Ok(mut bridge) = p.bridge.lock() else {
        return p.default;
    };
    match bridge.request("codex", method, action) {
        Outcome::Auto(d) => d,
        Outcome::Pending(reqid) => {
            let _ = bridge.resolve(reqid, p.default, Scope::Once);
            p.default
        }
    }
}

/// A coarse action string for rule-matching: the command (joined), else a call id / path.
fn approval_action(params: &Value) -> String {
    if let Some(cmd) = params.get("command").and_then(Value::as_array) {
        return cmd
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
    }
    for k in ["call_id", "path", "reason"] {
        if let Some(s) = params.get(k).and_then(Value::as_str) {
            return s.to_string();
        }
    }
    String::new()
}

/// Starts the reader thread over a split transport and returns the shared [`Inner`].
struct CodexRpc;

impl CodexRpc {
    fn start(transport: Box<dyn RpcTransport>) -> Arc<Inner> {
        let (writer, mut reader) = transport.split();
        let inner = Arc::new(Inner {
            writer: Mutex::new(writer),
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            notes: Mutex::new(Vec::new()),
            cvar: Condvar::new(),
            closed: AtomicBool::new(false),
            permission: Mutex::new(None),
        });
        let ri = inner.clone();
        std::thread::spawn(move || {
            loop {
                match reader.recv() {
                    Ok(Some(line)) => ri.on_message(&line),
                    Ok(None) | Err(_) => {
                        ri.mark_closed();
                        break;
                    }
                }
            }
        });
        inner
    }
}

// ---- the AppServerClient over the engine -----------------------------------------

/// A live connection to a Codex app-server, implementing [`AppServerClient`] by speaking
/// the `thread/*` + `turn/*` JSON-RPC over an [`RpcTransport`].
pub struct CodexAppServer {
    inner: Arc<Inner>,
}

impl CodexAppServer {
    /// Connect over `transport` and perform the `initialize` → `initialized` handshake.
    pub fn connect(transport: Box<dyn RpcTransport>) -> Result<Self, AppServerError> {
        let inner = CodexRpc::start(transport);
        inner.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "hotsheet",
                    "title": "Hot Sheet",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": null,
            }),
        )?;
        inner.notify("initialized", json!(null))?;
        Ok(Self { inner })
    }

    /// Route Codex approval ServerRequests through a permission policy instead of
    /// auto-approving (HS2-0QGW07 / §5.7). Set it before running a turn; without one,
    /// approvals are auto-approved (the isolated headless default).
    pub fn set_permission_policy(&self, policy: PermissionPolicy) {
        if let Ok(mut p) = self.inner.permission.lock() {
            *p = Some(policy);
        }
    }
}

/// Extract `result.thread.id` from a `thread/start` | `thread/resume` response.
fn thread_id_of(result: &Value) -> Result<String, AppServerError> {
    result
        .get("thread")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| AppServerError::Protocol("thread response missing thread.id".into()))
}

impl AppServerClient for CodexAppServer {
    fn open_thread(&self, resume: Option<&str>, cwd: &Path) -> Result<String, AppServerError> {
        let cwd = cwd.to_string_lossy().to_string();
        // Headless: never prompt for approvals, and let the agent write in its workspace.
        let result = match resume {
            Some(thread_id) => self.inner.request(
                "thread/resume",
                json!({ "threadId": thread_id, "cwd": cwd,
                        "approvalPolicy": "never", "sandbox": "workspace-write" }),
            )?,
            None => self.inner.request(
                "thread/start",
                json!({ "cwd": cwd, "approvalPolicy": "never", "sandbox": "workspace-write" }),
            )?,
        };
        thread_id_of(&result)
    }

    fn start_turn(
        &self,
        thread_id: &str,
        content: &str,
    ) -> Result<Box<dyn AppServerTurn>, AppServerError> {
        // Capture the notification cursor BEFORE sending, so a fast `turn/completed`
        // can't slip past between the response and our first scan.
        let cursor = self.inner.notes_len();
        let result = self.inner.request(
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": [{ "type": "text", "text": content, "text_elements": [] }],
            }),
        )?;
        let turn_id = result
            .get("turn")
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(Box::new(CodexTurn {
            inner: self.inner.clone(),
            thread_id: thread_id.to_string(),
            turn_id,
            cursor,
            done: None,
        }))
    }
}

// ---- one running turn ------------------------------------------------------------

/// Observes one `turn/start` via `turn/completed` notifications; `interrupt` sends
/// `turn/interrupt`.
struct CodexTurn {
    inner: Arc<Inner>,
    thread_id: String,
    turn_id: Option<String>,
    /// Where in the shared notification log this turn has scanned up to.
    cursor: usize,
    done: Option<AppServerOutcome>,
}

/// If `n` is this turn's `turn/completed`, map its status to an outcome.
fn completed_outcome(
    n: &Value,
    thread_id: &str,
    turn_id: Option<&str>,
) -> Option<AppServerOutcome> {
    if n.get("method").and_then(Value::as_str) != Some("turn/completed") {
        return None;
    }
    let p = n.get("params")?;
    if p.get("threadId").and_then(Value::as_str) != Some(thread_id) {
        return None;
    }
    let turn = p.get("turn")?;
    if let Some(want) = turn_id {
        if turn.get("id").and_then(Value::as_str) != Some(want) {
            return None; // a different turn on the same thread
        }
    }
    Some(match turn.get("status").and_then(Value::as_str) {
        Some("completed") => AppServerOutcome::Completed,
        Some("interrupted") => AppServerOutcome::Failed("interrupted".into()),
        Some("failed") => {
            let msg = turn
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("turn failed");
            AppServerOutcome::Failed(msg.to_string())
        }
        other => AppServerOutcome::Failed(format!("unexpected turn status: {other:?}")),
    })
}

impl CodexTurn {
    /// Non-blocking scan of newly-arrived notifications for this turn's completion.
    fn poll(&mut self) {
        if self.done.is_some() {
            return;
        }
        let notes = self.inner.notes.lock().unwrap();
        while self.cursor < notes.len() {
            let n = &notes[self.cursor];
            self.cursor += 1;
            if let Some(o) = completed_outcome(n, &self.thread_id, self.turn_id.as_deref()) {
                self.done = Some(o);
                return;
            }
        }
    }
}

impl AppServerTurn for CodexTurn {
    fn is_running(&mut self) -> bool {
        if self.done.is_some() {
            return false;
        }
        self.poll();
        self.done.is_none() && !self.inner.closed.load(Ordering::SeqCst)
    }

    fn wait(&mut self) -> AppServerOutcome {
        if let Some(o) = &self.done {
            return o.clone();
        }
        let deadline = Instant::now() + TURN_TIMEOUT;
        let mut notes = self.inner.notes.lock().unwrap();
        loop {
            while self.cursor < notes.len() {
                let n = &notes[self.cursor];
                self.cursor += 1;
                if let Some(o) = completed_outcome(n, &self.thread_id, self.turn_id.as_deref()) {
                    self.done = Some(o.clone());
                    return o;
                }
            }
            if self.inner.closed.load(Ordering::SeqCst) {
                let o = AppServerOutcome::Failed("app-server connection closed".into());
                self.done = Some(o.clone());
                return o;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let o = AppServerOutcome::Failed("timeout waiting for turn/completed".into());
                self.done = Some(o.clone());
                return o;
            }
            let (g, _) = self.inner.cvar.wait_timeout(notes, remaining).unwrap();
            notes = g;
        }
    }

    fn interrupt(&mut self) {
        if self.done.is_some() {
            return;
        }
        if let Some(turn_id) = &self.turn_id {
            let _ = self.inner.request(
                "turn/interrupt",
                json!({ "threadId": self.thread_id, "turnId": turn_id }),
            );
        }
        // The drive's TurnHandle records `Interrupted`; keep our own state terminal so a
        // later `wait()` can't block on a `turn/completed` that may never arrive.
        self.done = Some(AppServerOutcome::Failed("interrupted".into()));
    }
}

// ---- the real transport: a `codex app-server` child over stdio -------------------

/// A live [`RpcTransport`] backed by a `codex app-server …` child process speaking
/// newline-delimited JSON-RPC over its stdio (verified framing). Live-only; unit tests
/// inject a scripted fake instead. Two shapes:
///
/// - [`StdioTransport`] — `codex app-server` **direct**: one server per connection,
///   persistent for that connection's many turns (no process per play). This is the
///   shape the live turn uses today.
/// - [`ProxyTransport`] — `codex app-server proxy`: bridges the shared **daemon**
///   control socket so connections could reuse one instance across the machine. The child
///   spawns and its stdio pipes, but our client can't drive the daemon over it yet
///   (HS2-115). The blocker is now understood (codex 0.148 source, not an undocumented
///   protocol): the daemon control socket is a **plain WebSocket** endpoint — its server
///   does tungstenite `accept_async` on the UDS (`app-server-transport/.../unix_socket.rs`),
///   so the *local* socket needs **no** auth token (the `Authorization: Bearer` check is
///   only on the network `ws://IP:PORT` remote-control path), and the very same JSON-RPC
///   (`initialize` → `thread/*` → `turn/*`) rides as WebSocket **text frames** on
///   `ws://localhost/rpc`. The `proxy` subcommand is a *dumb byte relay* (`stdio_to_uds`:
///   `tokio::io::copy` stdin↔UDS), so writing raw newline JSON-RPC through it reaches a
///   server awaiting an HTTP/WebSocket upgrade → zero bytes back (the "no response" we
///   saw). Making this usable needs a **WebSocket framing layer** in the transport
///   (upgrade handshake, then text-frame each message; connect straight to the UDS, or
///   frame over the proxy child's stdio) — the protocol engine above is unchanged.
///   `StdioTransport` is the supported path meanwhile.
///
/// `codex app-server` direct over stdio — the live-default transport.
pub struct StdioTransport(StreamChild);

impl StdioTransport {
    /// Spawn `program app-server` in `cwd` (with extra `env`, e.g. an isolated
    /// `CODEX_HOME`), piping stdio for JSON-RPC.
    pub fn spawn(
        program: &str,
        cwd: &Path,
        env: &[(String, String)],
    ) -> std::io::Result<Box<Self>> {
        Ok(Box::new(Self(StreamChild::spawn(
            program,
            &["app-server"],
            cwd,
            env,
        )?)))
    }
}

impl RpcTransport for StdioTransport {
    fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
        self.0.into_halves()
    }
}

/// `codex app-server proxy` — relays stdio to the shared daemon control socket. Not yet
/// drivable: the socket speaks a **plain WebSocket** (text-frame JSON-RPC), so this raw
/// byte pipe needs a WebSocket framing layer on top before `CodexAppServer` can use it
/// (HS2-115; see the transport-family doc above).
pub struct ProxyTransport(StreamChild);

impl ProxyTransport {
    /// Spawn `program app-server proxy` in `cwd`, piping stdio for JSON-RPC.
    pub fn spawn(program: &str, cwd: &Path) -> std::io::Result<Box<Self>> {
        Ok(Box::new(Self(StreamChild::spawn(
            program,
            &["app-server", "proxy"],
            cwd,
            &[],
        )?)))
    }
}

impl RpcTransport for ProxyTransport {
    fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
        self.0.into_halves()
    }
}

// ---- the shared-daemon transport: a WebSocket over the control socket ------------

/// A live [`RpcTransport`] to the **shared** Codex app-server daemon (HS2-115). The daemon
/// control socket (`$CODEX_HOME/app-server-control/app-server-control.sock`) is a *plain*
/// WebSocket endpoint — its server does tungstenite `accept_async` on the UDS (no auth
/// token for the local socket) and carries the same `initialize`/`thread/*`/`turn/*`
/// JSON-RPC as WebSocket **text frames** on `ws://localhost/rpc`. So this connects the UDS
/// directly (the way codex's own client does), upgrades to a WebSocket, and adapts it to
/// the line-oriented [`RpcTransport`] the [`CodexAppServer`] engine expects.
///
/// One dedicated thread owns a current-thread Tokio runtime that drives a single `select!`
/// loop over the split sink/stream, bridged to the sync [`RpcWriter`]/[`RpcReader`] halves
/// by channels — so the [`tokio_tungstenite`] `WebSocketStream` (single-owner) never has to
/// be shared across threads. Multiple host connections can each open one of these against
/// the *same* daemon, reusing one codex instance across the machine.
pub struct UdsWsTransport {
    /// Outbound JSON-RPC lines → the ws task (which frames them as text messages).
    out_tx: tokio::sync::mpsc::UnboundedSender<String>,
    /// Inbound JSON-RPC lines the ws task decoded from text frames.
    in_rx: std::sync::mpsc::Receiver<String>,
}

impl UdsWsTransport {
    /// Connect to the daemon control socket at `socket_path`, performing the WebSocket
    /// upgrade before returning so a bad socket / failed handshake surfaces here (not as a
    /// later `initialize` timeout). Live-only; unit tests point it at a scripted WebSocket
    /// daemon over a temp UDS.
    pub fn connect(socket_path: &Path) -> std::io::Result<Box<Self>> {
        use std::sync::mpsc::channel as std_channel;

        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (in_tx, in_rx) = std_channel::<String>();
        // Reports the handshake result back to this call before the loop takes over.
        let (ready_tx, ready_rx) = std_channel::<std::io::Result<()>>();
        let path = socket_path.to_path_buf();

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            rt.block_on(async move {
                use futures_util::{SinkExt, StreamExt};
                use tokio_tungstenite::tungstenite::Message;

                let stream = match tokio::net::UnixStream::connect(&path).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                // Plain WS upgrade; the local UDS server ignores host/path/origin.
                let ws = match tokio_tungstenite::client_async("ws://localhost/rpc", stream).await {
                    Ok((ws, _resp)) => ws,
                    Err(e) => {
                        let _ = ready_tx.send(Err(std::io::Error::other(e)));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));

                let (mut write, mut read) = ws.split();
                loop {
                    tokio::select! {
                        // Prefer draining outbound so a request is on the wire before we
                        // block again on the read.
                        biased;
                        outbound = out_rx.recv() => match outbound {
                            Some(line) => {
                                if write.send(Message::Text(line.into())).await.is_err() {
                                    break;
                                }
                            }
                            None => break, // all RpcWriters dropped → connection done
                        },
                        frame = read.next() => match frame {
                            Some(Ok(Message::Text(t))) => {
                                if in_tx.send(t.to_string()).is_err() {
                                    break; // reader gone
                                }
                            }
                            // Keep the connection healthy; ignore server data frames.
                            Some(Ok(Message::Ping(p))) => {
                                let _ = write.send(Message::Pong(p)).await;
                            }
                            Some(Ok(Message::Close(_))) | None => break,
                            Some(Ok(_)) => {}
                            Some(Err(_)) => break,
                        },
                    }
                }
            });
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(Self { out_tx, in_rx })),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(std::io::Error::other(
                "websocket thread exited before the handshake completed",
            )),
        }
    }
}

impl RpcTransport for UdsWsTransport {
    fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
        let Self { out_tx, in_rx } = *self;
        (Box::new(WsWriter { out_tx }), Box::new(WsReader { in_rx }))
    }
}

/// Write half: hand each JSON-RPC line to the ws task to be framed as a text message.
struct WsWriter {
    out_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl RpcWriter for WsWriter {
    fn send(&mut self, msg: &str) -> std::io::Result<()> {
        self.out_tx
            .send(msg.to_string())
            .map_err(|_| std::io::Error::other("websocket connection closed"))
    }
}

/// Read half: the ws task pushes each decoded text frame here; a closed channel is EOF.
struct WsReader {
    in_rx: std::sync::mpsc::Receiver<String>,
}

impl RpcReader for WsReader {
    fn recv(&mut self) -> std::io::Result<Option<String>> {
        Ok(self.in_rx.recv().ok())
    }
}

/// The shared daemon's control-socket path for a given `CODEX_HOME`, mirroring codex's own
/// `app_server_control_socket_path`: `<codex_home>/app-server-control/app-server-control.sock`.
/// (Note: codex binds it directly there, so a very long `CODEX_HOME` can exceed the
/// platform's `sun_path` limit — keep the home short for the daemon.)
pub fn codex_control_socket_path(codex_home: &Path) -> PathBuf {
    codex_home
        .join("app-server-control")
        .join("app-server-control.sock")
}

/// Ensure the shared Codex app-server daemon is running before connecting to it. The CLI's
/// `daemon start` is idempotent ("start … if it is not already running"). Uses the ambient
/// `CODEX_HOME`; see [`ensure_codex_daemon_in`] to target a specific home. Live-only
/// (BackingService, `docs/13` §13.5).
pub fn ensure_codex_daemon(program: &str) -> std::io::Result<()> {
    run_daemon_start(program, None)
}

/// Like [`ensure_codex_daemon`], but starts the daemon for a specific `CODEX_HOME` — so a
/// per-project **isolated** home gets its own shared daemon (HS2-B7C66H), keeping MCP
/// isolation while still reusing one codex instance across a run's turns.
pub fn ensure_codex_daemon_in(program: &str, codex_home: &Path) -> std::io::Result<()> {
    run_daemon_start(program, Some(codex_home))
}

fn run_daemon_start(program: &str, codex_home: Option<&Path>) -> std::io::Result<()> {
    let mut cmd = Command::new(program);
    cmd.args(["app-server", "daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(home) = codex_home {
        cmd.env("CODEX_HOME", home);
    }
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "`{program} app-server daemon start` exited with {status}"
        )))
    }
}

/// The Codex `app-server daemon` as a [`BackingService`] (`docs/13` §13.5): the concrete
/// [`ensure_codex_daemon_in`] prestart behind the tool-id-free [`Drive::service`] accessor,
/// so a generic caller warms the daemon without importing this module. Carries the `codex`
/// program name and an optional per-project isolated `CODEX_HOME` (HS2-B7C66H).
///
/// [`BackingService`]: crate::drive::BackingService
/// [`Drive::service`]: crate::drive::Drive::service
#[derive(Debug, Clone)]
pub struct CodexDaemonService {
    program: String,
    codex_home: Option<std::path::PathBuf>,
}

impl CodexDaemonService {
    /// A service for the ambient `CODEX_HOME`.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            codex_home: None,
        }
    }

    /// A service targeting a specific (isolated) `CODEX_HOME`.
    pub fn with_home(
        program: impl Into<String>,
        codex_home: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            program: program.into(),
            codex_home: Some(codex_home.into()),
        }
    }
}

impl crate::drive::BackingService for CodexDaemonService {
    fn name(&self) -> &str {
        "codex-app-server"
    }

    fn prestart(&self) -> std::io::Result<()> {
        match &self.codex_home {
            Some(home) => ensure_codex_daemon_in(&self.program, home),
            None => ensure_codex_daemon(&self.program),
        }
    }
}

/// Stop the shared Codex daemon for a specific `CODEX_HOME` (`daemon stop` is a no-op if
/// none is running). Used to tear down a per-run isolated-home daemon so it isn't orphaned
/// when the home goes away (HS2-9M6T68). Best-effort: returns the child's status.
pub fn stop_codex_daemon_in(program: &str, codex_home: &Path) -> std::io::Result<()> {
    Command::new(program)
        .args(["app-server", "daemon", "stop"])
        .env("CODEX_HOME", codex_home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|_| ())
}

// ---- a scripted daemon for tests (no live codex) ---------------------------------

/// A loopback [`RpcTransport`] that answers the real client's protocol in-process, so
/// [`CodexAppServer`] is tested end to end (handshake → thread → turn → interrupt) with
/// no `codex` binary. Exposed for the crate's tests.
#[cfg(test)]
pub(crate) mod scripted {
    use super::*;

    /// How the scripted daemon resolves a `turn/start`.
    #[derive(Clone, Copy)]
    pub enum TurnMode {
        /// Emit `turn/completed` (status `completed`) immediately after the response.
        AutoComplete,
        /// Emit `turn/completed` (status `failed`) immediately after the response.
        AutoFail,
        /// Stay running; only emit `turn/completed` (status `interrupted`) on
        /// `turn/interrupt`.
        UntilInterrupt,
    }

    pub struct ScriptedDaemon {
        mode: TurnMode,
    }

    impl ScriptedDaemon {
        pub fn new(mode: TurnMode) -> Box<Self> {
            Box::new(Self { mode })
        }
    }

    impl RpcTransport for ScriptedDaemon {
        fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
            let (tx, rx) = channel::<String>();
            (
                Box::new(ScriptWriter {
                    out: tx,
                    mode: self.mode,
                }),
                Box::new(ScriptReader { inbox: rx }),
            )
        }
    }

    /// Parses each outgoing request and queues the daemon's replies onto the loopback.
    struct ScriptWriter {
        out: Sender<String>,
        mode: TurnMode,
    }

    impl ScriptWriter {
        fn push(&self, v: Value) {
            let _ = self.out.send(v.to_string());
        }
    }

    impl RpcWriter for ScriptWriter {
        fn send(&mut self, msg: &str) -> std::io::Result<()> {
            let v: Value = serde_json::from_str(msg).expect("client sends valid JSON");
            let method = v.get("method").and_then(Value::as_str).unwrap_or("");
            let id = v.get("id").cloned();
            let params = v.get("params").cloned().unwrap_or(Value::Null);
            let respond = |w: &ScriptWriter, result: Value| {
                if let Some(id) = &id {
                    w.push(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                }
            };
            match method {
                "initialize" => respond(
                    self,
                    json!({ "userAgent": "codex/test", "codexHome": "/tmp/codex",
                            "platformFamily": "unix", "platformOs": "macos" }),
                ),
                "initialized" => {} // client notification; nothing to answer
                "thread/start" => respond(self, json!({ "thread": { "id": "thread-1" } })),
                "thread/resume" => {
                    let tid = params
                        .get("threadId")
                        .and_then(Value::as_str)
                        .unwrap_or("thread-1")
                        .to_string();
                    respond(self, json!({ "thread": { "id": tid } }))
                }
                "turn/start" => {
                    let thread_id = params
                        .get("threadId")
                        .and_then(Value::as_str)
                        .unwrap_or("thread-1")
                        .to_string();
                    respond(
                        self,
                        json!({ "turn": { "id": "turn-1", "status": "inProgress" } }),
                    );
                    self.push(json!({ "jsonrpc": "2.0", "method": "turn/started",
                        "params": { "threadId": thread_id, "turn": { "id": "turn-1", "status": "inProgress" } } }));
                    let status = match self.mode {
                        TurnMode::AutoComplete => Some("completed"),
                        TurnMode::AutoFail => Some("failed"),
                        TurnMode::UntilInterrupt => None,
                    };
                    if let Some(status) = status {
                        let turn = if status == "failed" {
                            json!({ "id": "turn-1", "status": "failed",
                                    "error": { "message": "boom" } })
                        } else {
                            json!({ "id": "turn-1", "status": status })
                        };
                        self.push(json!({ "jsonrpc": "2.0", "method": "turn/completed",
                            "params": { "threadId": thread_id, "turn": turn } }));
                    }
                }
                "turn/interrupt" => {
                    let thread_id = params
                        .get("threadId")
                        .and_then(Value::as_str)
                        .unwrap_or("thread-1")
                        .to_string();
                    respond(self, json!({}));
                    self.push(json!({ "jsonrpc": "2.0", "method": "turn/completed",
                        "params": { "threadId": thread_id,
                                    "turn": { "id": "turn-1", "status": "interrupted" } } }));
                }
                _ => respond(self, json!({})),
            }
            Ok(())
        }
    }

    struct ScriptReader {
        inbox: std::sync::mpsc::Receiver<String>,
    }
    impl RpcReader for ScriptReader {
        fn recv(&mut self) -> std::io::Result<Option<String>> {
            // Block until the writer queues a reply, or all writers drop (connection end).
            Ok(self.inbox.recv().ok())
        }
    }
}

#[cfg(test)]
mod approval_tests {
    use super::*;
    use crate::permission::{Decision, PermissionBridge, Rule};

    fn policy(rules: Vec<Rule>, default: Decision) -> PermissionPolicy {
        PermissionPolicy {
            bridge: Arc::new(Mutex::new(PermissionBridge::with_rules(rules))),
            default,
        }
    }

    #[test]
    fn no_policy_auto_approves_and_maps_reply_tokens() {
        // Back-compat: without a policy, approvals are granted with the method's token.
        let exec = decide_approval(None, "execCommandApproval", &json!({"command": ["ls"]}));
        assert_eq!(exec["decision"], "approved");
        let item = decide_approval(None, "item/fileChange/requestApproval", &json!({}));
        assert_eq!(item["decision"], "accept");
        // An unrecognized request → empty result.
        assert_eq!(
            decide_approval(None, "somethingElse", &json!({})),
            json!({})
        );
    }

    #[test]
    fn an_allow_rule_approves_and_a_deny_rule_denies() {
        let p = policy(
            vec![
                Rule {
                    tool: "execCommandApproval".into(),
                    action: "git status".into(),
                    decision: Decision::Allow,
                    persist: true,
                },
                Rule {
                    tool: "execCommandApproval".into(),
                    action: "rm -rf /".into(),
                    decision: Decision::Deny,
                    persist: true,
                },
            ],
            Decision::Deny,
        );
        // A matching allow-rule → approved.
        let ok = decide_approval(
            Some(&p),
            "execCommandApproval",
            &json!({"command": ["git", "status"]}),
        );
        assert_eq!(ok["decision"], "approved");
        // A matching deny-rule → denied (mapped token).
        let no = decide_approval(
            Some(&p),
            "execCommandApproval",
            &json!({"command": ["rm", "-rf", "/"]}),
        );
        assert_eq!(no["decision"], "denied");
        // Unmatched → the policy default (Deny here), and the pending request is cleared.
        let def = decide_approval(
            Some(&p),
            "execCommandApproval",
            &json!({"command": ["curl", "x"]}),
        );
        assert_eq!(def["decision"], "denied");
        assert_eq!(
            p.bridge.lock().unwrap().pending_count(),
            0,
            "no leaked pending requests"
        );
    }
}
