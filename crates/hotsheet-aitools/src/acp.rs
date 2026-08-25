//! Agent Client Protocol drive + standard usage mapper (HS2-DEK94W/HS2-96BZEF).

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::drive::{DoneReason, PermReq, TurnEvent};
use crate::drive::{Drive, DriveCtx, DriveError, DriveInfo, Target, Transport, TurnHandle, Usage};
use crate::ports::{AcpClient, RpcReader, RpcTransport, RpcWriter};
use crate::procio::StreamChild;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
// OpenCode has emitted the final session/update immediately after session/prompt's
// response in released builds. Let the reader drain that small race before Done.
const PROMPT_DRAIN_GRACE: Duration = Duration::from_millis(100);

struct Inner {
    writer: Mutex<Box<dyn RpcWriter>>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, Sender<Value>>>,
    notes: Mutex<Vec<Value>>,
    cvar: Condvar,
    closed: AtomicBool,
}
impl Inner {
    fn request(&self, method: &str, params: Value) -> Result<Value, DriveError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        self.writer
            .lock()
            .unwrap()
            .send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string())
            .map_err(|e| DriveError::NotConnected(e.to_string()))?;
        let response = rx
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|_| DriveError::Protocol(format!("timeout waiting for ACP {method}")))?;
        if let Some(e) = response.get("error") {
            return Err(DriveError::Protocol(format!("ACP {method}: {e}")));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }
    fn notify(&self, method: &str, params: Value) -> bool {
        self.writer
            .lock()
            .unwrap()
            .send(&json!({"jsonrpc":"2.0","method":method,"params":params}).to_string())
            .is_ok()
    }
    fn on_message(&self, line: &str) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let has_method = v.get("method").is_some();
        let has_id = v.get("id").is_some();
        if has_method && has_id {
            self.answer_request(&v);
        } else if has_method {
            self.notes.lock().unwrap().push(v);
            self.cvar.notify_all();
        } else if let Some(id) = v.get("id").and_then(Value::as_i64) {
            if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                let _ = tx.send(v);
            }
        }
    }
    fn answer_request(&self, v: &Value) {
        let id = v.get("id").cloned().unwrap_or(Value::Null);
        let method = v.get("method").and_then(Value::as_str).unwrap_or("");
        let result = if method == "session/request_permission" {
            let option = v
                .pointer("/params/options")
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter()
                        .find(|o| {
                            o.get("kind")
                                .and_then(Value::as_str)
                                .is_some_and(|k| k.starts_with("reject"))
                        })
                        .or_else(|| a.last())
                })
                .and_then(|o| o.get("optionId"))
                .cloned()
                .unwrap_or(Value::Null);
            json!({"outcome":{"outcome":"selected","optionId":option}})
        } else {
            json!({})
        };
        let _ = self
            .writer
            .lock()
            .unwrap()
            .send(&json!({"jsonrpc":"2.0","id":id,"result":result}).to_string());
    }
}

fn connect(transport: Box<dyn RpcTransport>) -> Arc<Inner> {
    let (writer, mut reader) = transport.split();
    let inner = Arc::new(Inner {
        writer: Mutex::new(writer),
        next_id: AtomicI64::new(1),
        pending: Default::default(),
        notes: Default::default(),
        cvar: Condvar::new(),
        closed: AtomicBool::new(false),
    });
    let r = inner.clone();
    std::thread::spawn(move || {
        loop {
            match reader.recv() {
                Ok(Some(line)) => r.on_message(&line),
                _ => {
                    r.closed.store(true, Ordering::SeqCst);
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
        let inner = connect(transport);
        inner.request("initialize",json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"hotsheet","version":env!("CARGO_PKG_VERSION")}}))?;
        Ok(Self { inner })
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
        Ok(Box::new(Self(StreamChild::spawn(
            program,
            &["acp"],
            cwd,
            env,
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
}
impl AcpTurn {
    fn next(&mut self) -> Option<TurnEvent> {
        loop {
            let notes = self.inner.notes.lock().unwrap();
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
            if let Some(done) = *self.done.lock().unwrap() {
                if !self.emitted_usage {
                    self.emitted_usage = true;
                    if let Some(u) = self.response.lock().unwrap().as_ref().and_then(usage) {
                        return Some(TurnEvent::Usage(u));
                    }
                }
                if !self.emitted_done {
                    self.emitted_done = true;
                    return Some(TurnEvent::Done(done));
                }
                return None;
            }
            if self.inner.closed.load(Ordering::SeqCst) || self.started.elapsed() > REQUEST_TIMEOUT
            {
                return Some(TurnEvent::Done(DoneReason::Failed(1)));
            }
            let guard = self.inner.notes.lock().unwrap();
            let _ = self
                .inner
                .cvar
                .wait_timeout(guard, Duration::from_millis(100))
                .unwrap();
        }
    }
}
impl TurnHandle for AcpTurn {
    fn is_busy(&mut self) -> bool {
        self.done.lock().unwrap().is_none()
    }
    fn wait(&mut self) -> DoneReason {
        while let Some(e) = self.next() {
            if let TurnEvent::Done(d) = e {
                return d;
            }
        }
        self.done.lock().unwrap().unwrap_or(DoneReason::Failed(1))
    }
    fn interrupt(&mut self) -> bool {
        self.inner
            .notify("session/cancel", json!({"sessionId":self.session}))
    }
    fn next_event(&mut self) -> Option<TurnEvent> {
        self.next()
    }
    fn usage(&mut self) -> Option<Usage> {
        self.response.lock().unwrap().as_ref().and_then(usage)
    }
}

impl AcpClient for AcpSession {
    fn start_turn(
        &self,
        resume: Option<&str>,
        cwd: &Path,
        content: &str,
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
        let cursor = self.inner.notes.lock().unwrap().len();
        let done = Arc::new(Mutex::new(None));
        let response = Arc::new(Mutex::new(None));
        let i = self.inner.clone();
        let s = session.clone();
        let d = done.clone();
        let r = response.clone();
        let prompt = content.to_owned();
        std::thread::spawn(move || {
            let result = i.request(
                "session/prompt",
                json!({"sessionId":s,"prompt":[{"type":"text","text":prompt}]}),
            );
            match result {
                Ok(v) => {
                    *r.lock().unwrap() = Some(v.clone());
                    let reason = match v.get("stopReason").and_then(Value::as_str) {
                        Some("cancelled") => DoneReason::Interrupted,
                        Some("end_turn" | "stop_sequence" | "max_tokens") | None => {
                            DoneReason::Completed
                        }
                        _ => DoneReason::Failed(1),
                    };
                    std::thread::sleep(PROMPT_DRAIN_GRACE);
                    *d.lock().unwrap() = Some(reason)
                }
                Err(_) => *d.lock().unwrap() = Some(DoneReason::Failed(1)),
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

pub struct AcpDrive;

impl Drive for AcpDrive {
    fn info(&self) -> DriveInfo {
        DriveInfo {
            transport: Transport::Acp,
        }
    }

    fn supports_interrupt(&self) -> bool {
        true
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
        client.start_turn(target.0.as_deref(), &ctx.cwd, content)
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
            spawner: &NoSpawn,
            env: vec![],
            app_server: None,
            channel: None,
            acp: Some(&FakeAcp),
        };
        let mut turn = AcpDrive.run(&Target::default(), "work", &ctx).unwrap();
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
        let mut turn = client.start_turn(None, Path::new("/tmp"), "work").unwrap();
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
