//! Agent Client Protocol drive + standard usage mapper (HS2-DEK94W/HS2-96BZEF).
//!
//! `session/request_permission` is the ACP permission adapter (`docs/05` §5.7,
//! HS2-K6HH96): each ask is routed to the host's [`SharedPermissionBridge`] and answered
//! with the peer's own option id once a human (or a remembered rule, or the unattended
//! timeout) decides. The client never picks an option on its own, and without a bridge it
//! never selects an allow option.
//!
//! [`SharedPermissionBridge`]: crate::permission::SharedPermissionBridge

use hotsheet_sync::{CondvarExt, LockExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::codex::PermissionPolicy;
use crate::drive::{DoneReason, PermReq, TurnEvent};
use crate::drive::{Drive, DriveCtx, DriveError, DriveInfo, Target, Transport, TurnHandle, Usage};
use crate::model_catalog::{CommandModelCatalog, RuntimeModelCatalogSource};
use crate::permission::{Decision, PermissionAsk};
use crate::ports::{AcpClient, RpcReader, RpcTransport, RpcWriter};
use crate::procio::StreamChild;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
// OpenCode has emitted the final session/update immediately after session/prompt's
// response in released builds. Let the reader drain that small race before Done.
const PROMPT_DRAIN_GRACE: Duration = Duration::from_millis(100);
/// How often a prompt wait re-checks whether its session is waiting on a human.
const PROMPT_WAIT_SLICE: Duration = Duration::from_millis(200);

/// How the session routes permission asks (HS2-K6HH96).
#[derive(Clone)]
struct AcpPermission {
    policy: PermissionPolicy,
    /// The asking agent named on each request (the plugin id, e.g. `opencode`).
    agent: String,
}

/// A `session/request_permission` the peer is still waiting on. Removing its entry from
/// [`Inner::asks`] is the single token that entitles the remover to send the one reply.
struct PendingAsk {
    /// The peer's JSON-RPC request id, echoed on the reply.
    rpc_id: Value,
    session: String,
    /// The bridge request id once the ask is queued for a human.
    bridge_id: Option<u64>,
}

struct Inner {
    writer: Mutex<Box<dyn RpcWriter>>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, Sender<Value>>>,
    notes: Mutex<Vec<Value>>,
    cvar: Condvar,
    closed: AtomicBool,
    /// How long a turn may go without progress before it is failed. Time a session spends
    /// waiting on a human permission answer does not count (HS2-K6HH96).
    turn_timeout: Duration,
    permission: Mutex<Option<AcpPermission>>,
    /// Unanswered permission asks, keyed by the serialized peer request id.
    asks: Mutex<HashMap<String, PendingAsk>>,
    /// When the last ask settled, so a turn's stall clock restarts after a human answers.
    ask_settled_at: Mutex<Option<Instant>>,
    /// Set by a Stop request from another thread. A turn blocked waiting on a human answer
    /// emits no events, so its owner acts on this from inside the wait (HS2-K6HH96).
    interrupt_wake: AtomicBool,
    /// Set when the [`AcpSession`] is dropped (HS2-GSW2AA). The peer may outlive it, so an ask
    /// arriving afterwards is answered `cancelled` instead of reaching the bridge with no
    /// session left to withdraw it.
    ended: AtomicBool,
}
impl Inner {
    fn request(&self, method: &str, params: Value) -> Result<Value, DriveError> {
        self.request_for(method, params, None)
    }

    /// Send a request and wait for its response. With `session`, the deadline restarts
    /// while that session has a permission ask waiting on a human, so a slow answer never
    /// fails the turn it pauses.
    fn request_for(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value, DriveError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        self.pending.lock_or_recover().insert(id, tx);
        self.writer
            .lock_or_recover()
            .send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string())
            .map_err(|e| DriveError::NotConnected(e.to_string()))?;
        let timeout = || DriveError::Protocol(format!("timeout waiting for ACP {method}"));
        let mut deadline = Instant::now() + self.turn_timeout;
        let response = loop {
            let now = Instant::now();
            if now >= deadline {
                self.pending.lock_or_recover().remove(&id);
                return Err(timeout());
            }
            match rx.recv_timeout(PROMPT_WAIT_SLICE.min(deadline - now)) {
                Ok(response) => break response,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(DriveError::NotConnected(format!(
                        "ACP connection closed while waiting for {method}"
                    )));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(session) = session {
                        let since = deadline - self.turn_timeout;
                        deadline = self.stall_clock(session, since) + self.turn_timeout;
                    }
                }
            }
        };
        if let Some(e) = response.get("error") {
            return Err(DriveError::Protocol(format!("ACP {method}: {e}")));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }
    fn notify(&self, method: &str, params: Value) -> bool {
        self.writer
            .lock_or_recover()
            .send(&json!({"jsonrpc":"2.0","method":method,"params":params}).to_string())
            .is_ok()
    }
    fn reply(&self, id: &Value, result: Value) {
        let _ = self
            .writer
            .lock_or_recover()
            .send(&json!({"jsonrpc":"2.0","id":id,"result":result}).to_string());
    }
    fn on_message(self: &Arc<Self>, line: &str) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let has_method = v.get("method").is_some();
        let has_id = v.get("id").is_some();
        if has_method && has_id {
            self.answer_request(v);
        } else if has_method {
            self.notes.lock_or_recover().push(v);
            self.cvar.notify_all();
        } else if let Some(id) = v.get("id").and_then(Value::as_i64) {
            if let Some(tx) = self.pending.lock_or_recover().remove(&id) {
                let _ = tx.send(v);
            }
        }
    }
    fn answer_request(self: &Arc<Self>, v: Value) {
        let id = v.get("id").cloned().unwrap_or(Value::Null);
        if v.get("method").and_then(Value::as_str) == Some("session/request_permission") {
            self.ask_permission(id, v.get("params").cloned().unwrap_or(Value::Null));
        } else {
            self.reply(&id, json!({}));
        }
    }

    /// Route one permission ask to the bridge on its own thread, so the reader keeps
    /// serving concurrent asks, updates and responses while a human decides.
    fn ask_permission(self: &Arc<Self>, rpc_id: Value, params: Value) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        if self.ended.load(Ordering::SeqCst) {
            self.reply(&rpc_id, json!({"outcome":{"outcome":"cancelled"}}));
            return;
        }
        let options = params
            .get("options")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let Some(permission) = self.permission.lock_or_recover().clone() else {
            // No human can be asked (a bare headless run): never grant anything.
            self.reply(&rpc_id, permission_outcome(&options, Decision::Deny));
            return;
        };
        let session = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let key = rpc_id.to_string();
        self.asks.lock_or_recover().insert(
            key.clone(),
            PendingAsk {
                rpc_id: rpc_id.clone(),
                session,
                bridge_id: None,
            },
        );
        // HS2-GSW2AA: the session may have ended between the check above and the insert, after
        // its drop withdrew every ask it could see. `Drop` sets `ended` before taking `asks`,
        // so either it saw this entry or this load sees `ended`; whoever removes the entry
        // answers it.
        if self.ended.load(Ordering::SeqCst) {
            if self.asks.lock_or_recover().remove(&key).is_some() {
                self.reply(&rpc_id, json!({"outcome":{"outcome":"cancelled"}}));
            }
            return;
        }
        let (tool, action) = permission_subject(&params);
        let inner = self.clone();
        // Detached on purpose: it ends when the bridge answers, times out, or is cancelled.
        // `cancel_asks` (session drop, peer exit, `session/cancel`) cancels its bridge request,
        // so the ask never outlives its session (HS2-GSW2AA).
        std::thread::spawn(move || {
            let AcpPermission { policy, agent } = permission;
            let bridge = policy.bridge.clone();
            let decision = policy.bridge.request_blocking_timeout_with_pending(
                PermissionAsk {
                    project: policy.project,
                    connection: policy.connection,
                    tool,
                    action,
                    agent: Some(agent),
                },
                policy.timeout,
                policy.default,
                |bridge_id| {
                    let mut asks = inner.asks.lock_or_recover();
                    match asks.get_mut(&key) {
                        Some(ask) => ask.bridge_id = Some(bridge_id),
                        // Cancelled or closed before it was queued: withdraw it now.
                        None => {
                            drop(asks);
                            bridge.cancel(bridge_id);
                        }
                    }
                },
                |_| {},
            );
            let owned = inner.asks.lock_or_recover().remove(&key).is_some();
            inner.mark_ask_settled();
            if owned && !inner.closed.load(Ordering::SeqCst) {
                inner.reply(&rpc_id, permission_outcome(&options, decision));
            }
            inner.cvar.notify_all();
        });
    }

    fn mark_ask_settled(&self) {
        *self.ask_settled_at.lock_or_recover() = Some(Instant::now());
    }

    /// Restart `since` past any time spent waiting on a human answer.
    fn stall_clock(&self, session: &str, since: Instant) -> Instant {
        if self.waiting_on_human(session) {
            return Instant::now();
        }
        self.ask_settled_at
            .lock_or_recover()
            .map_or(since, |settled| settled.max(since))
    }

    fn waiting_on_human(&self, session: &str) -> bool {
        self.asks
            .lock_or_recover()
            .values()
            .any(|ask| ask.session == session)
    }

    /// Withdraw unanswered asks (all of them, or one session's) from the bridge. With
    /// `reply`, each is answered `cancelled`, which ACP requires after `session/cancel`.
    fn cancel_asks(&self, session: Option<&str>, reply: bool) {
        let taken: Vec<PendingAsk> = {
            let mut asks = self.asks.lock_or_recover();
            let keys: Vec<String> = asks
                .iter()
                .filter(|(_, ask)| session.is_none_or(|s| ask.session == s))
                .map(|(key, _)| key.clone())
                .collect();
            keys.iter().filter_map(|key| asks.remove(key)).collect()
        };
        if taken.is_empty() {
            return;
        }
        self.mark_ask_settled();
        let bridge = self
            .permission
            .lock_or_recover()
            .as_ref()
            .map(|p| p.policy.bridge.clone());
        for ask in taken {
            if reply {
                self.reply(&ask.rpc_id, json!({"outcome":{"outcome":"cancelled"}}));
            }
            if let (Some(bridge), Some(id)) = (&bridge, ask.bridge_id) {
                bridge.cancel(id);
            }
        }
        self.cvar.notify_all();
    }
}

/// Map a decision onto one of the peer's own options (HS2-K6HH96). Allow selects only an
/// `allow_once` option, so the peer never remembers a grant Hot Sheet did not record;
/// Deny prefers `reject_once`. Without a matching option the ask is `cancelled`, which
/// ACP defines as not granted.
fn permission_outcome(options: &[Value], decision: Decision) -> Value {
    let kinds: &[&str] = match decision {
        Decision::Allow => &["allow_once"],
        Decision::Deny => &["reject_once", "reject_always"],
    };
    kinds
        .iter()
        .find_map(|kind| {
            options
                .iter()
                .find(|o| o.get("kind").and_then(Value::as_str) == Some(kind))
                .and_then(|o| o.get("optionId"))
        })
        .map_or_else(
            || json!({"outcome":{"outcome":"cancelled"}}),
            |id| json!({"outcome":{"outcome":"selected","optionId":id}}),
        )
}

/// The bridge's tool and rule-match action for an ACP `toolCall`: the ACP tool kind is
/// named like the Claude/Codex tools where one corresponds, and the action is the raw
/// command, path, or URL, falling back to the first location and then the title.
fn permission_subject(params: &Value) -> (String, String) {
    let call = params.get("toolCall").unwrap_or(&Value::Null);
    let text = |key: &str| {
        call.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    let tool = match text("kind") {
        Some("execute") => "Bash",
        Some("edit") => "Edit",
        Some("read") => "Read",
        Some("fetch") => "WebFetch",
        Some(kind) if kind != "other" => kind,
        _ => text("title").unwrap_or("tool"),
    }
    .to_owned();
    let raw = call.get("rawInput").unwrap_or(&Value::Null);
    let action = match raw.get("command") {
        Some(Value::Array(parts)) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Some(Value::String(command)) => Some(command.clone()),
        _ => None,
    }
    .or_else(|| {
        ["filePath", "path", "url", "pattern"]
            .iter()
            .find_map(|key| raw.get(*key).and_then(Value::as_str).map(str::to_owned))
    })
    .or_else(|| {
        call.pointer("/locations/0/path")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
    .or_else(|| text("title").map(str::to_owned))
    .unwrap_or_default();
    (tool, action)
}

fn connect(transport: Box<dyn RpcTransport>, turn_timeout: Duration) -> Arc<Inner> {
    let (writer, mut reader) = transport.split();
    let inner = Arc::new(Inner {
        writer: Mutex::new(writer),
        next_id: AtomicI64::new(1),
        pending: Default::default(),
        notes: Default::default(),
        cvar: Condvar::new(),
        closed: AtomicBool::new(false),
        turn_timeout,
        permission: Mutex::new(None),
        asks: Default::default(),
        ask_settled_at: Mutex::new(None),
        interrupt_wake: AtomicBool::new(false),
        ended: AtomicBool::new(false),
    });
    let r = inner.clone();
    // Detached on purpose (HS2-GSW2AA): the reader ends when the peer's stdout closes.
    std::thread::spawn(move || {
        loop {
            match reader.recv() {
                Ok(Some(line)) => r.on_message(&line),
                _ => {
                    r.closed.store(true, Ordering::SeqCst);
                    // The peer is gone: its asks can no longer be answered.
                    r.cancel_asks(None, false);
                    r.pending.lock_or_recover().clear();
                    r.cvar.notify_all();
                    break;
                }
            }
        }
    });
    inner
}

/// A real ACP v1 client over an injected newline-delimited JSON-RPC transport.
pub struct AcpSession {
    inner: Arc<Inner>,
}
impl AcpSession {
    pub fn connect(transport: Box<dyn RpcTransport>) -> Result<Self, DriveError> {
        Self::connect_with_turn_timeout(transport, REQUEST_TIMEOUT)
    }

    fn connect_with_turn_timeout(
        transport: Box<dyn RpcTransport>,
        turn_timeout: Duration,
    ) -> Result<Self, DriveError> {
        let inner = connect(transport, turn_timeout);
        inner.request("initialize",json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"hotsheet","version":env!("CARGO_PKG_VERSION")}}))?;
        Ok(Self { inner })
    }

    /// Route `session/request_permission` asks through `policy`'s bridge, naming `agent`
    /// as the asker (HS2-K6HH96). Set it before running a turn. Without one, asks are
    /// refused, never granted.
    pub fn set_permission_policy(&self, policy: PermissionPolicy, agent: impl Into<String>) {
        *self.inner.permission.lock_or_recover() = Some(AcpPermission {
            policy,
            agent: agent.into(),
        });
    }

    /// Wake a running turn when `control` requests a Stop (HS2-K6HH96). While OpenCode
    /// waits on a permission answer it streams nothing, so the turn pump never gets back
    /// to its own interrupt check; the waker lets the turn's owner send `session/cancel`
    /// and answer the pending asks `cancelled` from inside its wait.
    pub fn wake_on_interrupt(&self, control: &crate::drive::TurnControl) {
        let inner = Arc::downgrade(&self.inner);
        control.on_interrupt(move || {
            if let Some(inner) = inner.upgrade() {
                inner.interrupt_wake.store(true, Ordering::SeqCst);
                inner.cvar.notify_all();
            }
        });
    }
}
impl Drop for AcpSession {
    /// The session is ending: answer and withdraw any ask still on the bridge.
    fn drop(&mut self) {
        self.inner.ended.store(true, Ordering::SeqCst);
        self.inner.cancel_asks(None, true);
    }
}

/// Production `opencode acp` stdio transport.
pub struct AcpStdio(StreamChild);
impl AcpStdio {
    pub fn spawn(
        program: &str,
        cwd: &Path,
        env: &[(String, String)],
    ) -> std::io::Result<Box<Self>> {
        Self::spawn_with_context(program, cwd, env, None)
    }

    pub(crate) fn spawn_with_context(
        program: &str,
        cwd: &Path,
        env: &[(String, String)],
        context: Option<crate::procio::ProcessContext>,
    ) -> std::io::Result<Box<Self>> {
        Ok(Box::new(Self(StreamChild::spawn_with_context(
            program,
            &["acp"],
            cwd,
            env,
            false,
            context,
        )?)))
    }
}
impl RpcTransport for AcpStdio {
    fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
        self.0.into_halves()
    }
}

struct AcpTurn {
    inner: Arc<Inner>,
    session: String,
    cursor: usize,
    done: Arc<Mutex<Option<DoneReason>>>,
    response: Arc<Mutex<Option<Value>>>,
    emitted_usage: bool,
    emitted_done: bool,
    started: Instant,
    /// `session/cancel` was sent for this turn; a repeated interrupt sends nothing more.
    cancel_sent: bool,
}
/// The first terminal outcome wins even if a prompt RPC resolves after closure or timeout.
fn finish_once(done: &Mutex<Option<DoneReason>>, reason: DoneReason) {
    done.lock_or_recover().get_or_insert(reason);
}

impl AcpTurn {
    fn next(&mut self) -> Option<TurnEvent> {
        loop {
            if self.inner.interrupt_wake.swap(false, Ordering::SeqCst) {
                TurnHandle::interrupt(self);
            }
            let notes = self.inner.notes.lock_or_recover();
            if self.cursor < notes.len() {
                let v = notes[self.cursor].clone();
                self.cursor += 1;
                drop(notes);
                if v.pointer("/params/sessionId").and_then(Value::as_str) != Some(&self.session) {
                    continue;
                }
                if let Some(event) = map_update(&v) {
                    return Some(event);
                }
                continue;
            }
            drop(notes);
            if let Some(done) = self.done.lock_or_recover().clone() {
                if !self.emitted_usage {
                    self.emitted_usage = true;
                    if let Some(u) = self.response.lock_or_recover().as_ref().and_then(usage) {
                        return Some(TurnEvent::Usage(u));
                    }
                }
                if !self.emitted_done {
                    self.emitted_done = true;
                    return Some(TurnEvent::Done(done));
                }
                return None;
            }
            // Time a human spends deciding a permission ask is not a stall.
            self.started = self.inner.stall_clock(&self.session, self.started);
            if self.inner.closed.load(Ordering::SeqCst)
                || self.started.elapsed() > self.inner.turn_timeout
            {
                finish_once(
                    &self.done,
                    DoneReason::failed(
                        1,
                        if self.inner.closed.load(Ordering::SeqCst) {
                            "ACP connection closed"
                        } else {
                            "Timed out waiting for ACP turn result"
                        },
                    ),
                );
                continue;
            }
            let guard = self.inner.notes.lock_or_recover();
            let _ = self
                .inner
                .cvar
                .wait_timeout_or_recover(guard, Duration::from_millis(100));
        }
    }
}
impl TurnHandle for AcpTurn {
    fn is_busy(&mut self) -> bool {
        self.done.lock_or_recover().is_none()
    }
    fn wait(&mut self) -> DoneReason {
        while let Some(e) = self.next() {
            if let TurnEvent::Done(d) = e {
                return d;
            }
        }
        self.done
            .lock_or_recover()
            .clone()
            .unwrap_or(DoneReason::Failed {
                exit_code: 1,
                message: None,
                error_type: None,
            })
    }
    fn interrupt(&mut self) -> bool {
        if self.cancel_sent {
            return true;
        }
        self.cancel_sent = true;
        let sent = self
            .inner
            .notify("session/cancel", json!({"sessionId":self.session}));
        // ACP: after session/cancel the client answers every pending permission ask for
        // the session with `cancelled`; the bridge withdraws the popup (HS2-K6HH96).
        self.inner.cancel_asks(Some(&self.session), true);
        sent
    }
    fn next_event(&mut self) -> Option<TurnEvent> {
        self.next()
    }
    fn usage(&mut self) -> Option<Usage> {
        self.response.lock_or_recover().as_ref().and_then(usage)
    }
}

impl AcpClient for AcpSession {
    fn start_turn(
        &self,
        resume: Option<&str>,
        cwd: &Path,
        content: &str,
    ) -> Result<Box<dyn TurnHandle>, DriveError> {
        self.start_turn_with_options(resume, cwd, content, None, None)
    }

    fn start_turn_with_options(
        &self,
        resume: Option<&str>,
        cwd: &Path,
        content: &str,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<Box<dyn TurnHandle>, DriveError> {
        let session = match resume {
            Some(id) => {
                self.inner.request(
                    "session/load",
                    json!({"sessionId":id,"cwd":cwd,"mcpServers":[]}),
                )?;
                id.to_owned()
            }
            None => self
                .inner
                .request("session/new", json!({"cwd":cwd,"mcpServers":[]}))?
                .get("sessionId")
                .and_then(Value::as_str)
                .ok_or_else(|| DriveError::Protocol("ACP session/new omitted sessionId".into()))?
                .to_owned(),
        };
        for (config_id, value) in [("model", model), ("effort", effort)] {
            if let Some(value) = value {
                self.inner.request(
                    "session/set_config_option",
                    json!({"sessionId":session,"configId":config_id,"value":value}),
                )?;
            }
        }
        let cursor = self.inner.notes.lock_or_recover().len();
        // A Stop meant for an earlier turn never cancels this one.
        self.inner.interrupt_wake.store(false, Ordering::SeqCst);
        let done = Arc::new(Mutex::new(None));
        let response = Arc::new(Mutex::new(None));
        let i = self.inner.clone();
        let s = session.clone();
        let d = done.clone();
        let r = response.clone();
        let prompt = content.to_owned();
        // Detached on purpose (HS2-GSW2AA): `request_for` returns on the peer's response, its
        // deadline, or the peer closing, and the turn reads the result from `done`.
        std::thread::spawn(move || {
            let result = i.request_for(
                "session/prompt",
                json!({"sessionId":s,"prompt":[{"type":"text","text":prompt}]}),
                Some(&s),
            );
            match result {
                Ok(v) => {
                    *r.lock_or_recover() = Some(v.clone());
                    let reason = match v.get("stopReason").and_then(Value::as_str) {
                        Some("cancelled") => DoneReason::Interrupted,
                        Some("end_turn" | "stop_sequence" | "max_tokens") | None => {
                            DoneReason::Completed
                        }
                        other => {
                            DoneReason::failed(1, format!("Unexpected ACP stop reason: {other:?}"))
                        }
                    };
                    std::thread::sleep(PROMPT_DRAIN_GRACE);
                    finish_once(&d, reason)
                }
                Err(error) => finish_once(&d, DoneReason::failed(1, error.to_string())),
            };
            i.cvar.notify_all();
        });
        Ok(Box::new(AcpTurn {
            inner: self.inner.clone(),
            session,
            cursor,
            done,
            response,
            emitted_usage: false,
            emitted_done: false,
            started: Instant::now(),
            cancel_sent: false,
        }))
    }
}

fn map_update(v: &Value) -> Option<TurnEvent> {
    let update = v.pointer("/params/update")?;
    match update.get("sessionUpdate").and_then(Value::as_str) {
        Some("agent_message_chunk" | "agent_thought_chunk") => update
            .pointer("/content/text")
            .or_else(|| update.get("text"))
            .and_then(Value::as_str)
            .map(|v| TurnEvent::Output(v.to_owned())),
        Some("tool_call") => Some(TurnEvent::PermissionAsked(PermReq {
            tool: update
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .into(),
            summary: update
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("tool call")
                .into(),
        })),
        Some("usage_update") => usage(update).map(TurnEvent::Usage),
        _ => None,
    }
}

#[derive(Default)]
pub struct AcpDrive {
    model_catalog: Option<CommandModelCatalog>,
}

impl AcpDrive {
    pub fn with_model_catalog(model_catalog: CommandModelCatalog) -> Self {
        Self {
            model_catalog: Some(model_catalog),
        }
    }
}

impl Drive for AcpDrive {
    fn info(&self) -> DriveInfo {
        DriveInfo {
            transport: Transport::Acp,
        }
    }

    fn supports_interrupt(&self) -> bool {
        true
    }

    fn model_catalog(&self) -> Option<&dyn RuntimeModelCatalogSource> {
        self.model_catalog
            .as_ref()
            .map(|catalog| catalog as &dyn RuntimeModelCatalogSource)
    }

    fn run(
        &self,
        target: &Target,
        content: &str,
        ctx: &DriveCtx,
    ) -> Result<Box<dyn TurnHandle>, DriveError> {
        let client = ctx
            .acp
            .ok_or_else(|| DriveError::NotConnected("ACP agent is not connected".into()))?;
        client.start_turn_with_options(
            target.0.as_deref(),
            &ctx.cwd,
            content,
            ctx.model.as_deref(),
            ctx.effort.as_deref(),
        )
    }
}

/// Map only ACP's standard PromptResponse usage counters. Implementations are observed
/// with both protocol-style snake_case and SDK camelCase; model/cache/cost are deliberately
/// absent because the standard counters do not guarantee them.
pub fn usage(value: &Value) -> Option<Usage> {
    let u = value
        .pointer("/result/usage")
        .or_else(|| value.get("usage"))?;
    let number = |snake: &str, camel: &str| {
        u.get(snake)
            .or_else(|| u.get(camel))
            .and_then(Value::as_u64)
    };
    let tokens_in = number("input_tokens", "inputTokens")?;
    let tokens_out = number("output_tokens", "outputTokens")?;
    Some(Usage {
        model: None,
        tokens_in,
        tokens_out,
        cost_usd: None,
    })
}

/// Validate the version-pinned OpenCode ACP wire landmarks used by the live client.
pub fn validate_opencode_transcript(messages: &[Value]) -> Result<(), String> {
    let has = |f: &dyn Fn(&Value) -> bool| messages.iter().any(f);
    if !has(&|v| v.pointer("/result/protocolVersion").and_then(Value::as_u64) == Some(1)) {
        return Err("missing ACP v1 initialize response".into());
    }
    if !has(&|v| {
        v.pointer("/result/sessionId")
            .and_then(Value::as_str)
            .is_some()
    }) {
        return Err("missing session/new sessionId".into());
    }
    if !has(&|v| {
        v.get("method").and_then(Value::as_str) == Some("session/update")
            && v.pointer("/params/update/sessionUpdate").is_some()
    }) {
        return Err("missing session/update discriminator".into());
    }
    if !has(&|v| {
        v.pointer("/result/stopReason")
            .and_then(Value::as_str)
            .is_some()
    }) {
        return Err("missing session/prompt stopReason".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::{DoneReason, DriveCtx};
    use crate::ports::{AcpClient, ProcessSpawner, SpawnSpec, SpawnedProcess};
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Receiver, Sender, channel};

    struct NoSpawn;
    impl ProcessSpawner for NoSpawn {
        fn spawn(&self, _: &SpawnSpec) -> std::io::Result<Box<dyn SpawnedProcess>> {
            panic!("ACP must use its protocol client")
        }
    }
    struct Done;
    impl TurnHandle for Done {
        fn is_busy(&mut self) -> bool {
            false
        }
        fn wait(&mut self) -> DoneReason {
            DoneReason::Completed
        }
    }
    struct FakeAcp;
    impl AcpClient for FakeAcp {
        fn start_turn(
            &self,
            _: Option<&str>,
            _: &Path,
            content: &str,
        ) -> Result<Box<dyn TurnHandle>, DriveError> {
            assert_eq!(content, "work");
            Ok(Box::new(Done))
        }
    }

    #[test]
    fn drive_delegates_to_the_injected_acp_session() {
        let ctx = DriveCtx {
            cwd: PathBuf::from("/project"),
            model: None,
            effort: None,
            spawner: &NoSpawn,
            env: vec![],
            app_server: None,
            channel: None,
            acp: Some(&FakeAcp),
        };
        let mut turn = AcpDrive::default()
            .run(&Target::default(), "work", &ctx)
            .unwrap();
        assert_eq!(turn.wait(), DoneReason::Completed);
    }

    #[test]
    fn maps_only_standard_prompt_response_counters() {
        assert_eq!(
            usage(
                &json!({"result":{"usage":{"input_tokens":41,"output_tokens":7,"total_tokens":48}}})
            ),
            Some(Usage {
                model: None,
                tokens_in: 41,
                tokens_out: 7,
                cost_usd: None
            })
        );
        assert!(usage(&json!({"result":{"usage":{"total_tokens":48}}})).is_none());
    }

    #[test]
    fn accepts_sdk_camel_case_without_inventing_attribution() {
        let got = usage(
            &json!({"usage":{"inputTokens":2,"outputTokens":3,"cost":9.99,"model":"ignored"}}),
        )
        .unwrap();
        assert_eq!(got.model, None);
        assert_eq!(got.cost_usd, None);
        assert_eq!((got.tokens_in, got.tokens_out), (2, 3));
    }

    struct ScriptedAcp;
    impl RpcTransport for ScriptedAcp {
        fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
            let (tx, rx) = channel();
            (Box::new(AcpWriter(tx)), Box::new(AcpReader(rx)))
        }
    }
    struct AcpWriter(Sender<String>);
    impl RpcWriter for AcpWriter {
        fn send(&mut self, msg: &str) -> std::io::Result<()> {
            let v: Value = serde_json::from_str(msg).unwrap();
            let Some(id) = v.get("id").cloned() else {
                return Ok(());
            };
            match v.get("method").and_then(Value::as_str) {
                Some("initialize") => {
                    let _=self.0.send(json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1,"agentCapabilities":{}}}).to_string());
                }
                Some("session/new") => {
                    let _ = self.0.send(
                        json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"sess-1"}})
                            .to_string(),
                    );
                }
                Some("session/set_config_option") => {
                    assert_eq!(
                        v.pointer("/params/sessionId").and_then(Value::as_str),
                        Some("sess-1")
                    );
                    let config = v
                        .pointer("/params/configId")
                        .and_then(Value::as_str)
                        .unwrap();
                    let value = v.pointer("/params/value").and_then(Value::as_str).unwrap();
                    assert!(matches!(
                        (config, value),
                        ("model", "provider/model") | ("effort", "high")
                    ));
                    let _ = self.0.send(
                        json!({"jsonrpc":"2.0","id":id,"result":{"configOptions":[]}}).to_string(),
                    );
                }
                Some("session/prompt") => {
                    let _=self.0.send(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello"}}}}).to_string());
                    let _=self.0.send(json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"end_turn","usage":{"inputTokens":4,"outputTokens":2}}}).to_string());
                }
                _ => {}
            }
            Ok(())
        }
    }
    struct AcpReader(Receiver<String>);
    impl RpcReader for AcpReader {
        fn recv(&mut self) -> std::io::Result<Option<String>> {
            Ok(self.0.recv().ok())
        }
    }

    #[test]
    fn live_client_negotiates_creates_session_streams_and_finishes() {
        let client = AcpSession::connect(Box::new(ScriptedAcp)).unwrap();
        let mut turn = client
            .start_turn_with_options(
                None,
                Path::new("/tmp"),
                "work",
                Some("provider/model"),
                Some("high"),
            )
            .unwrap();
        assert_eq!(turn.next_event(), Some(TurnEvent::Output("hello".into())));
        assert_eq!(
            turn.next_event(),
            Some(TurnEvent::Usage(Usage {
                model: None,
                tokens_in: 4,
                tokens_out: 2,
                cost_usd: None
            }))
        );
        assert_eq!(
            turn.next_event(),
            Some(TurnEvent::Done(DoneReason::Completed))
        );
    }

    #[test]
    fn late_prompt_results_cannot_overwrite_an_emitted_terminal_failure() {
        for late in [
            DoneReason::Completed,
            DoneReason::failed(1, "Late RPC error"),
            DoneReason::Interrupted,
        ] {
            let done = Mutex::new(None);
            let first = DoneReason::failed(1, "ACP connection closed");
            finish_once(&done, first.clone());
            finish_once(&done, late);
            assert_eq!(*done.lock_or_recover(), Some(first));
        }
    }

    struct FailureTransport(Value);
    impl RpcTransport for FailureTransport {
        fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
            let (tx, rx) = channel();
            (
                Box::new(FailureWriter {
                    tx,
                    response: self.0,
                }),
                Box::new(AcpReader(rx)),
            )
        }
    }
    struct FailureWriter {
        tx: Sender<String>,
        response: Value,
    }
    impl RpcWriter for FailureWriter {
        fn send(&mut self, message: &str) -> std::io::Result<()> {
            let value: Value = serde_json::from_str(message).unwrap();
            if value.get("method").and_then(Value::as_str) == Some("session/prompt") {
                let mut response = self.response.clone();
                response["jsonrpc"] = json!("2.0");
                response["id"] = value["id"].clone();
                self.tx.send(response.to_string()).unwrap();
                Ok(())
            } else {
                AcpWriter(self.tx.clone()).send(message)
            }
        }
    }
    #[test]
    fn failed_prompt_retains_protocol_diagnostic_and_unexpected_stop_reason_once() {
        for (response, expected) in [
            (
                json!({"error":{"code":-32000,"message":"Provider rate limited"}}),
                "Provider rate limited",
            ),
            (json!({"result":{"stopReason":"refused"}}), "refused"),
        ] {
            let client = AcpSession::connect(Box::new(FailureTransport(response))).unwrap();
            let mut turn = client.start_turn(None, Path::new("/tmp"), "work").unwrap();
            let Some(TurnEvent::Done(reason)) = turn.next_event() else {
                panic!("expected failed Done");
            };
            assert!(reason.failure_message().unwrap().contains(expected));
            assert_eq!(turn.next_event(), None);
            assert_eq!(turn.wait(), reason);
        }
    }

    #[test]
    #[ignore = "live: needs a real opencode + configured provider; set HOTSHEET_OPENCODE_LIVE=1"]
    fn opencode_live_acp_turn() {
        if std::env::var("HOTSHEET_OPENCODE_LIVE").as_deref() != Ok("1") {
            return;
        }
        let cwd = tempfile::tempdir().unwrap();
        let transport = AcpStdio::spawn("opencode", cwd.path(), &[]).unwrap();
        let client = AcpSession::connect(transport).unwrap();
        let mut turn = client
            .start_turn(None, cwd.path(), "Reply with only pong")
            .unwrap();
        assert_eq!(turn.wait(), DoneReason::Completed);
    }
}

/// HS2-K6HH96: `session/request_permission` reaches the permission bridge, and every
/// outcome (allow, deny, timeout, cancel, concurrent asks, session end) answers the peer
/// once with one of its own options, against a fake peer speaking OpenCode's wire shape.
#[cfg(test)]
mod permission_tests {
    use super::*;
    use crate::permission::{Request, Scope, SharedPermissionBridge};
    use std::sync::mpsc::Receiver;

    const WAIT: Duration = Duration::from_secs(10);
    /// The reader returns end-of-stream when it reads this line (the peer exits).
    const EOF: &str = "<eof>";

    /// A fake ACP agent: answers `initialize`/`session/new` itself and hands every other
    /// client message to the test, which injects peer messages through `to_client`.
    struct FakePeer {
        to_client: Sender<String>,
        inbound: Receiver<String>,
        from_client: Sender<Value>,
    }
    struct PeerWriter {
        to_client: Sender<String>,
        from_client: Sender<Value>,
    }
    impl RpcWriter for PeerWriter {
        fn send(&mut self, message: &str) -> std::io::Result<()> {
            let value: Value = serde_json::from_str(message).unwrap();
            let id = value.get("id").cloned();
            let result = match value.get("method").and_then(Value::as_str) {
                Some("initialize") => json!({"protocolVersion":1,"agentCapabilities":{}}),
                Some("session/new") => json!({"sessionId":"sess-1"}),
                _ => {
                    let _ = self.from_client.send(value);
                    return Ok(());
                }
            };
            let _ = self
                .to_client
                .send(json!({"jsonrpc":"2.0","id":id,"result":result}).to_string());
            Ok(())
        }
    }
    struct PeerReader(Receiver<String>);
    impl RpcReader for PeerReader {
        fn recv(&mut self) -> std::io::Result<Option<String>> {
            Ok(self.0.recv().ok().filter(|line| line != EOF))
        }
    }
    impl RpcTransport for FakePeer {
        fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
            (
                Box::new(PeerWriter {
                    to_client: self.to_client,
                    from_client: self.from_client,
                }),
                Box::new(PeerReader(self.inbound)),
            )
        }
    }

    struct Harness {
        session: AcpSession,
        to_client: Sender<String>,
        from_client: Receiver<Value>,
        bridge: Arc<SharedPermissionBridge>,
        asked: Receiver<Request>,
        removed: Receiver<Request>,
    }

    fn harness(policy_timeout: Option<Duration>, turn_timeout: Duration) -> Harness {
        let (to_client, inbound) = channel();
        let (from_client_tx, from_client) = channel();
        let session = AcpSession::connect_with_turn_timeout(
            Box::new(FakePeer {
                to_client: to_client.clone(),
                inbound,
                from_client: from_client_tx,
            }),
            turn_timeout,
        )
        .unwrap();
        let bridge = Arc::new(SharedPermissionBridge::default());
        let (asked_tx, asked) = channel();
        let asked_tx = Mutex::new(asked_tx);
        bridge.set_on_pending(move |r| {
            let _ = asked_tx.lock_or_recover().send(r.clone());
        });
        let (removed_tx, removed) = channel();
        let removed_tx = Mutex::new(removed_tx);
        bridge.set_on_removed(move |r| {
            let _ = removed_tx.lock_or_recover().send(r.clone());
        });
        if let Some(timeout) = policy_timeout {
            session.set_permission_policy(
                PermissionPolicy {
                    bridge: bridge.clone(),
                    project: "project-a".into(),
                    connection: "conn-7".into(),
                    default: Decision::Deny,
                    timeout,
                },
                "opencode",
            );
        }
        Harness {
            session,
            to_client,
            from_client,
            bridge,
            asked,
            removed,
        }
    }

    /// OpenCode's `session/request_permission` shape (a bash permission).
    fn ask(id: u64, command: &str) -> String {
        json!({"jsonrpc":"2.0","id":id,"method":"session/request_permission","params":{
        "sessionId":"sess-1",
        "toolCall":{"toolCallId":format!("call_{id}"),"status":"pending","title":command,
            "kind":"execute","rawInput":{"command":command},"locations":[]},
        "options":[
            {"optionId":"once","kind":"allow_once","name":"Allow once"},
            {"optionId":"always","kind":"allow_always","name":"Always allow"},
            {"optionId":"reject","kind":"reject_once","name":"Reject"}
        ]}})
        .to_string()
    }

    impl Harness {
        fn start(&self) -> (Box<dyn TurnHandle>, Value) {
            let turn = self
                .session
                .start_turn(None, Path::new("/project"), "work")
                .unwrap();
            let prompt = self.next_from_client();
            assert_eq!(prompt["method"], "session/prompt");
            (turn, prompt["id"].clone())
        }
        fn next_from_client(&self) -> Value {
            self.from_client.recv_timeout(WAIT).expect("client message")
        }
        fn inject(&self, line: String) {
            self.to_client.send(line).unwrap();
        }
        fn finish(&self, prompt_id: &Value) {
            self.inject(
                json!({"jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"end_turn"}})
                    .to_string(),
            );
        }
        fn asked(&self) -> Request {
            self.asked.recv_timeout(WAIT).expect("bridge request")
        }
        fn reply_to(&self, id: u64) -> Value {
            let reply = self.next_from_client();
            assert_eq!(reply["id"], id, "reply {reply}");
            assert!(reply.get("method").is_none());
            reply["result"]["outcome"].clone()
        }
        fn quiet(&self) -> bool {
            self.from_client
                .recv_timeout(Duration::from_millis(150))
                .is_err()
        }
    }

    fn selected(option: &str) -> Value {
        json!({"outcome":"selected","optionId":option})
    }

    #[test]
    fn allow_and_deny_route_through_the_bridge_and_answer_with_the_peer_option() {
        for (decision, option) in [(Decision::Allow, "once"), (Decision::Deny, "reject")] {
            let h = harness(Some(WAIT), REQUEST_TIMEOUT);
            let (mut turn, prompt) = h.start();
            h.inject(ask(0, "git status"));
            let request = h.asked();
            assert_eq!(
                (
                    request.project.as_str(),
                    request.connection.as_str(),
                    request.tool.as_str(),
                    request.action.as_str(),
                    request.agent.as_deref()
                ),
                (
                    "project-a",
                    "conn-7",
                    "Bash",
                    "git status",
                    Some("opencode")
                )
            );
            assert!(h.quiet(), "nothing answers the ask before the human does");
            assert!(
                h.bridge
                    .resolve(request.id, decision, Scope::Once)
                    .is_some()
            );
            assert_eq!(h.reply_to(0), selected(option));
            assert!(h.bridge.pending().is_empty());
            h.finish(&prompt);
            assert_eq!(turn.wait(), DoneReason::Completed);
        }
    }

    #[test]
    fn an_unanswered_ask_times_out_to_a_one_time_rejection() {
        let h = harness(Some(Duration::from_millis(50)), REQUEST_TIMEOUT);
        let (mut turn, prompt) = h.start();
        h.inject(ask(3, "rm -rf build"));
        let request = h.asked();
        assert_eq!(h.reply_to(3), selected("reject"));
        assert_eq!(h.removed.recv_timeout(WAIT).unwrap().id, request.id);
        assert!(h.bridge.pending().is_empty());
        assert!(h.bridge.rules().is_empty(), "a timeout remembers nothing");
        h.finish(&prompt);
        assert_eq!(turn.wait(), DoneReason::Completed);
    }

    #[test]
    fn interrupt_cancels_pending_asks_and_withdraws_them_from_the_bridge() {
        let h = harness(Some(WAIT), REQUEST_TIMEOUT);
        let (mut turn, prompt) = h.start();
        h.inject(ask(4, "npm test"));
        let request = h.asked();
        assert!(turn.interrupt());
        let cancel = h.next_from_client();
        assert_eq!(cancel["method"], "session/cancel");
        assert_eq!(cancel["params"]["sessionId"], "sess-1");
        assert_eq!(h.reply_to(4), json!({"outcome":"cancelled"}));
        assert_eq!(h.removed.recv_timeout(WAIT).unwrap().id, request.id);
        assert!(h.bridge.pending().is_empty());
        // A late answer to the withdrawn request is refused and sends nothing more.
        assert!(
            h.bridge
                .resolve(request.id, Decision::Allow, Scope::Once)
                .is_none()
        );
        h.inject(
            json!({"jsonrpc":"2.0","id":prompt,"result":{"stopReason":"cancelled"}}).to_string(),
        );
        assert_eq!(turn.wait(), DoneReason::Interrupted);
        assert!(h.quiet());
    }

    /// HS2-K6HH96 live check: Stop pressed while OpenCode waits on a permission answer.
    /// The peer streams nothing then, so the turn's owner is parked inside `next_event`;
    /// the control's waker must still send `session/cancel` and withdraw the popup.
    #[test]
    fn stop_requested_while_blocked_on_an_ask_cancels_from_inside_the_wait() {
        let h = harness(Some(WAIT), REQUEST_TIMEOUT);
        let control = crate::drive::TurnControl::default();
        h.session.wake_on_interrupt(&control);
        let (mut turn, prompt) = h.start();
        h.inject(ask(5, "echo hs2-k6hh96"));
        let request = h.asked();
        assert!(h.quiet(), "nothing is sent before Stop");
        let Harness {
            session: _session,
            to_client,
            from_client,
            bridge,
            removed,
            ..
        } = h;
        // The Stop and the peer run on another thread while this one is parked in
        // `next_event`; the peer answers the cancel the way OpenCode does.
        let peer = std::thread::spawn({
            let control = control.clone();
            move || {
                control.request_interrupt();
                let cancel = from_client.recv_timeout(WAIT).expect("session/cancel");
                let reply = from_client.recv_timeout(WAIT).expect("ask reply");
                to_client
                    .send(
                        json!({"jsonrpc":"2.0","id":prompt,"result":{"stopReason":"cancelled"}})
                            .to_string(),
                    )
                    .unwrap();
                (cancel, reply, from_client)
            }
        });
        let mut events = Vec::new();
        while let Some(event) = turn.next_event() {
            let done = matches!(event, TurnEvent::Done(_));
            events.push(event);
            if done {
                break;
            }
        }
        let (cancel, reply, from_client) = peer.join().unwrap();
        assert_eq!(cancel["method"], "session/cancel");
        assert_eq!(cancel["params"]["sessionId"], "sess-1");
        assert_eq!(reply["id"], 5);
        assert_eq!(reply["result"]["outcome"], json!({"outcome":"cancelled"}));
        assert_eq!(removed.recv_timeout(WAIT).unwrap().id, request.id);
        assert!(bridge.pending().is_empty());
        assert!(matches!(
            events.last(),
            Some(TurnEvent::Done(DoneReason::Interrupted))
        ));
        // The pump's own interrupt after the wake is idempotent: no second cancel.
        assert!(turn.interrupt());
        assert!(
            from_client
                .recv_timeout(Duration::from_millis(150))
                .is_err()
        );
    }

    #[test]
    fn concurrent_asks_are_both_queued_and_answered_out_of_order_by_id() {
        let h = harness(Some(WAIT), REQUEST_TIMEOUT);
        let (mut turn, prompt) = h.start();
        h.inject(ask(10, "git status"));
        h.inject(ask(11, "git diff"));
        let mut asked = [h.asked(), h.asked()];
        asked.sort_by_key(|r| r.action.clone());
        let [diff, status] = asked;
        assert_eq!(
            (diff.action.as_str(), status.action.as_str()),
            ("git diff", "git status")
        );
        assert_eq!(h.bridge.pending().len(), 2);
        h.bridge
            .resolve(diff.id, Decision::Allow, Scope::Once)
            .unwrap();
        assert_eq!(h.reply_to(11), selected("once"));
        assert_eq!(h.bridge.pending().len(), 1, "the other ask still waits");
        h.bridge
            .resolve(status.id, Decision::Deny, Scope::Once)
            .unwrap();
        assert_eq!(h.reply_to(10), selected("reject"));
        h.finish(&prompt);
        assert_eq!(turn.wait(), DoneReason::Completed);
    }

    #[test]
    fn peer_exit_while_an_ask_is_pending_withdraws_it_and_fails_the_turn() {
        let h = harness(Some(WAIT), REQUEST_TIMEOUT);
        let (mut turn, _prompt) = h.start();
        h.inject(ask(5, "git push"));
        let request = h.asked();
        h.inject(EOF.into());
        assert_eq!(h.removed.recv_timeout(WAIT).unwrap().id, request.id);
        assert!(h.bridge.pending().is_empty());
        let reason = turn.wait();
        assert!(reason.failure_message().is_some(), "{reason:?}");
        assert!(h.quiet(), "no reply is written to a closed peer");
    }

    #[test]
    fn dropping_the_session_answers_cancelled_and_withdraws_the_ask() {
        let h = harness(Some(WAIT), REQUEST_TIMEOUT);
        let (turn, _prompt) = h.start();
        h.inject(ask(6, "git push"));
        let request = h.asked();
        drop(turn);
        let Harness {
            session,
            from_client,
            bridge,
            removed,
            ..
        } = h;
        drop(session);
        let reply = from_client.recv_timeout(WAIT).unwrap();
        assert_eq!(reply["id"], 6);
        assert_eq!(reply["result"]["outcome"], json!({"outcome":"cancelled"}));
        assert_eq!(removed.recv_timeout(WAIT).unwrap().id, request.id);
        assert!(bridge.pending().is_empty());
    }

    #[test]
    fn hs2_gsw2aa_an_ask_after_the_session_ends_is_cancelled_and_never_queued() {
        let h = harness(Some(WAIT), REQUEST_TIMEOUT);
        let Harness {
            session,
            to_client,
            from_client,
            bridge,
            asked,
            ..
        } = h;
        drop(session);
        // The peer outlives the session and asks again: nothing can withdraw that ask later,
        // so it is answered at once and never reaches the bridge.
        to_client.send(ask(9, "git push")).unwrap();
        let reply = from_client.recv_timeout(WAIT).unwrap();
        assert_eq!(reply["id"], 9);
        assert_eq!(reply["result"]["outcome"], json!({"outcome":"cancelled"}));
        assert!(bridge.pending().is_empty());
        assert!(asked.try_recv().is_err(), "the ask never reached a human");
    }

    #[test]
    fn a_remembered_rule_answers_without_queueing_for_a_human() {
        let h = harness(Some(WAIT), REQUEST_TIMEOUT);
        let (mut turn, prompt) = h.start();
        h.inject(ask(7, "git status"));
        let first = h.asked();
        h.bridge
            .resolve(first.id, Decision::Allow, Scope::Session)
            .unwrap();
        assert_eq!(h.reply_to(7), selected("once"));
        h.inject(ask(8, "git status"));
        assert_eq!(h.reply_to(8), selected("once"));
        assert!(h.asked.try_recv().is_err(), "the rule answered, no popup");
        h.finish(&prompt);
        assert_eq!(turn.wait(), DoneReason::Completed);
    }

    #[test]
    fn without_a_bridge_asks_are_refused_and_never_granted() {
        let h = harness(None, REQUEST_TIMEOUT);
        let (mut turn, prompt) = h.start();
        h.inject(ask(1, "git status"));
        assert_eq!(h.reply_to(1), selected("reject"));
        // Only allow options offered (the old code picked the last one): cancelled.
        h.inject(
            json!({"jsonrpc":"2.0","id":2,"method":"session/request_permission","params":{
                "sessionId":"sess-1","toolCall":{"toolCallId":"c","title":"edit"},
                "options":[{"optionId":"once","kind":"allow_once","name":"Allow"},
                    {"optionId":"always","kind":"allow_always","name":"Always"}]}})
            .to_string(),
        );
        assert_eq!(h.reply_to(2), json!({"outcome":"cancelled"}));
        h.finish(&prompt);
        assert_eq!(turn.wait(), DoneReason::Completed);
    }

    #[test]
    fn waiting_on_a_human_does_not_count_toward_the_turn_timeout() {
        let h = harness(Some(WAIT), Duration::from_millis(300));
        let (mut turn, prompt) = h.start();
        h.inject(ask(9, "cargo build"));
        let request = h.asked();
        // Three turn timeouts pass while the human decides; the turn stays alive.
        assert!(h.quiet() && h.quiet() && h.quiet() && h.quiet() && h.quiet() && h.quiet());
        assert!(turn.is_busy(), "a pending ask keeps the turn alive");
        h.bridge
            .resolve(request.id, Decision::Allow, Scope::Once)
            .unwrap();
        assert_eq!(h.reply_to(9), selected("once"));
        h.finish(&prompt);
        assert_eq!(turn.wait(), DoneReason::Completed);
    }

    #[test]
    fn outcome_maps_decisions_onto_peer_options_only() {
        let both = [
            json!({"optionId":"a1","kind":"allow_always"}),
            json!({"optionId":"a0","kind":"allow_once"}),
            json!({"optionId":"r1","kind":"reject_always"}),
            json!({"optionId":"r0","kind":"reject_once"}),
        ];
        assert_eq!(
            permission_outcome(&both, Decision::Allow),
            json!({"outcome":{"outcome":"selected","optionId":"a0"}})
        );
        assert_eq!(
            permission_outcome(&both, Decision::Deny),
            json!({"outcome":{"outcome":"selected","optionId":"r0"}})
        );
        assert_eq!(
            permission_outcome(&both[2..3], Decision::Deny),
            json!({"outcome":{"outcome":"selected","optionId":"r1"}})
        );
        let cancelled = json!({"outcome":{"outcome":"cancelled"}});
        assert_eq!(permission_outcome(&both[..1], Decision::Allow), cancelled);
        assert_eq!(permission_outcome(&both[..2], Decision::Deny), cancelled);
        assert_eq!(permission_outcome(&[], Decision::Allow), cancelled);
    }

    #[test]
    fn subject_names_the_tool_kind_and_the_raw_action() {
        let subject = |call: Value| permission_subject(&json!({ "toolCall": call }));
        assert_eq!(
            subject(json!({"kind":"execute","rawInput":{"command":["git","status"]}})),
            ("Bash".into(), "git status".into())
        );
        assert_eq!(
            subject(json!({"kind":"edit","rawInput":{"filePath":"src/a.rs"}})),
            ("Edit".into(), "src/a.rs".into())
        );
        assert_eq!(
            subject(json!({"kind":"fetch","rawInput":{"url":"https://example.test"}})),
            ("WebFetch".into(), "https://example.test".into())
        );
        assert_eq!(
            subject(json!({"kind":"read","locations":[{"path":"README.md"}]})),
            ("Read".into(), "README.md".into())
        );
        assert_eq!(
            subject(json!({"kind":"search","title":"grep TODO"})),
            ("search".into(), "grep TODO".into())
        );
        assert_eq!(
            subject(json!({"kind":"other","title":"external_directory"})),
            ("external_directory".into(), "external_directory".into())
        );
        assert_eq!(subject(Value::Null), ("tool".into(), String::new()));
    }
}
