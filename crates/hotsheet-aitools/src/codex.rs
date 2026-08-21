//! The **real** [`AppServerClient`] — a live JSON-RPC client for the Codex app-server
//! (`docs/13` §13.5, HS2-112). It speaks the `codex 0.148` protocol
//! (`codex app-server generate-ts`): `initialize` → `initialized`, `thread/start` /
//! `thread/resume`, `turn/start` with text input, observing `turn/started` →
//! `turn/completed` notifications, and `turn/interrupt`.
//!
//! It drives a **persistent** codex the HS1 way — a turn on a running (or resumed)
//! thread, never a fresh process per play. The bytes go over an injected
//! [`RpcTransport`]: [`StdioTransport`] runs `codex app-server` direct (one persistent
//! process per connection — the live-verified default), [`ProxyTransport`] bridges the
//! shared `codex app-server proxy` daemon socket; tests inject a **scripted daemon**, so
//! the whole protocol engine ([`CodexRpc`], [`CodexAppServer`], [`CodexTurn`]) is
//! exercised with no live `codex` (`docs/05` §5.10, the load-bearing testability rule).

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::ports::{
    AppServerClient, AppServerError, AppServerOutcome, AppServerTurn, RpcReader, RpcTransport,
    RpcWriter,
};
use crate::procio::StreamChild;

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
        // Map each known approval to its permissive decision; unknown → empty result.
        let result = match method {
            "execCommandApproval" | "applyPatchApproval" => json!({ "decision": "approved" }),
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                json!({ "decision": "accept" })
            }
            _ => json!({}),
        };
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
///   control socket so connections could reuse one instance across the machine. Built,
///   but **not usable yet** (HS2-115): probing showed the daemon control socket does
///   **not** serve the app-server JSON-RPC — an `initialize` (JSONL, `Content-Length`,
///   or raw; via the proxy or straight to the socket; with remote-control on or off)
///   draws zero bytes, while `daemon version`/`stop` on the same socket answer. It speaks
///   a separate **experimental, undocumented control protocol** not covered by
///   `generate-ts`/`generate-json-schema`. Reviving this needs that protocol reverse-
///   engineered from codex source (or a documented upstream API); `StdioTransport` is the
///   supported path meanwhile.
///
/// `codex app-server` direct over stdio — the live-default transport.
pub struct StdioTransport(StreamChild);

impl StdioTransport {
    /// Spawn `program app-server` in `cwd`, piping stdio for JSON-RPC.
    pub fn spawn(program: &str, cwd: &Path) -> std::io::Result<Box<Self>> {
        Ok(Box::new(Self(StreamChild::spawn(
            program,
            &["app-server"],
            cwd,
            &[],
        )?)))
    }
}

impl RpcTransport for StdioTransport {
    fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
        self.0.into_halves()
    }
}

/// `codex app-server proxy` — bridges the shared daemon control socket (daemon handshake
/// is blocked, HS2-115; see the transport-family doc above).
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

/// Ensure the shared Codex app-server daemon is running before connecting a proxy. The
/// CLI's `daemon start` is idempotent ("start … if it is not already running").
/// Live-only (BackingService, `docs/13` §13.5).
pub fn ensure_codex_daemon(program: &str) -> std::io::Result<()> {
    let status = Command::new(program)
        .args(["app-server", "daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "`{program} app-server daemon start` exited with {status}"
        )))
    }
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
