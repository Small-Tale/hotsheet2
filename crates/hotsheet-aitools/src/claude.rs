//! The **Claude channel** drive (`docs/13` §13.6, the interface's acceptance shape —
//! "why this isn't Claude-shaped"). Unlike spawn/app-server, a play is **one user message
//! injected into a running, persistent `claude` session**, and its turn is observed as an
//! **async event stream** (assistant output → result), not a single terminal wait.
//!
//! Verified `claude 2.1.238` protocol: `claude -p --input-format stream-json
//! --output-format stream-json [--resume <id>]` — a long-lived process over NDJSON. Input
//! is `{"type":"user","message":{"role":"user","content":"…"}}`; output events are
//! `system`/`init` (carries `session_id`), `assistant` (message blocks = output), and
//! `result` (`subtype:"success"` / `is_error` → the turn is done). Same NDJSON framing as
//! the codex client, so it rides the same injected [`RpcTransport`]: [`ClaudeStreamTransport`]
//! spawns the real `claude`, while tests inject a **scripted claude** so the whole engine
//! is exercised with no live tool (`docs/05` §5.10).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::drive::{
    ClaudeChannelClient, DoneReason, Drive, DriveCtx, DriveError, DriveInfo, Target, Transport,
    TurnEvent, TurnHandle,
};
use crate::ports::{RpcReader, RpcTransport, RpcWriter};
use crate::procio::StreamChild;

/// How long a turn may run before we stop waiting for its `result`.
const TURN_TIMEOUT: Duration = Duration::from_secs(600);

// ---- the channel engine ----------------------------------------------------------

/// Shared state behind the channel: the write half plus the running event log a background
/// reader thread appends to.
struct Inner {
    writer: Mutex<Box<dyn RpcWriter>>,
    /// Every parsed output event, in order; turns scan it from their own cursor.
    events: Mutex<Vec<Value>>,
    cvar: Condvar,
    closed: AtomicBool,
    /// The running session id (from `system`/`init`), for `Target`/resume.
    session_id: Mutex<Option<String>>,
}

impl Inner {
    fn send_user(&self, content: &str) -> Result<(), DriveError> {
        let msg = json!({ "type": "user", "message": { "role": "user", "content": content } });
        self.writer
            .lock()
            .unwrap()
            .send(&msg.to_string())
            .map_err(|e| DriveError::NotConnected(format!("claude channel write failed: {e}")))
    }

    fn on_message(&self, line: &str) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return; // ignore non-JSON noise
        };
        if v.get("type").and_then(Value::as_str) == Some("system")
            && v.get("subtype").and_then(Value::as_str) == Some("init")
            && let Some(sid) = v.get("session_id").and_then(Value::as_str)
        {
            *self.session_id.lock().unwrap() = Some(sid.to_string());
        }
        let mut events = self.events.lock().unwrap();
        events.push(v);
        self.cvar.notify_all();
    }

    fn mark_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let _guard = self.events.lock().unwrap();
        self.cvar.notify_all();
    }

    fn events_len(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

fn start_reader(transport: Box<dyn RpcTransport>) -> Arc<Inner> {
    let (writer, mut reader) = transport.split();
    let inner = Arc::new(Inner {
        writer: Mutex::new(writer),
        events: Mutex::new(Vec::new()),
        cvar: Condvar::new(),
        closed: AtomicBool::new(false),
        session_id: Mutex::new(None),
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

// ---- the ClaudeChannelClient over the engine -------------------------------------

/// A live, persistent `claude` stream-json session, implementing [`ClaudeChannelClient`]:
/// each `start_turn` injects one user message and returns a streaming [`TurnHandle`].
pub struct ClaudeChannel {
    inner: Arc<Inner>,
}

impl ClaudeChannel {
    /// Attach to a running `claude` session over `transport` (starts the reader thread).
    pub fn connect(transport: Box<dyn RpcTransport>) -> Self {
        Self {
            inner: start_reader(transport),
        }
    }
}

impl ClaudeChannelClient for ClaudeChannel {
    fn session_id(&self) -> Option<String> {
        self.inner.session_id.lock().unwrap().clone()
    }

    fn start_turn(&self, content: &str) -> Result<Box<dyn TurnHandle>, DriveError> {
        // Capture the cursor BEFORE sending so a fast `result` can't slip past.
        let cursor = self.inner.events_len();
        self.inner.send_user(content)?;
        Ok(Box::new(ClaudeTurn {
            inner: self.inner.clone(),
            cursor,
            done: None,
            usage: None,
        }))
    }
}

// ---- one streaming turn ----------------------------------------------------------

/// Streams one turn's events (assistant output …) ending in exactly one
/// [`TurnEvent::Done`] mapped from the `result` event.
struct ClaudeTurn {
    inner: Arc<Inner>,
    cursor: usize,
    done: Option<DoneReason>,
    /// Token usage captured from the final `result` event, if it reported any (HS2-TJ8FGR).
    usage: Option<crate::drive::Usage>,
}

/// Extract token usage from a Claude stream-json `result` event (`docs/14`, the
/// `claude-usage` mapper for HS2-TJ8FGR — reads the in-band stream result rather than a
/// separate OTLP collector). Lenient: `None` when the event reports no usage, so a schema
/// tweak degrades quietly. The exact field names should be confirmed against a live claude.
pub fn claude_result_usage(result: &Value) -> Option<crate::drive::Usage> {
    let usage = result.get("usage")?;
    let u64_at = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| usage.get(*k).and_then(Value::as_u64))
    };
    // Base (uncached) input, plus the cached-prompt tokens Claude reports separately, so the
    // count is the total the model actually processed (HS2-CQ6B96).
    let base_in = u64_at(&["input_tokens", "prompt_tokens"])?;
    let cache_read = u64_at(&["cache_read_input_tokens"]).unwrap_or(0);
    let cache_creation = u64_at(&["cache_creation_input_tokens"]).unwrap_or(0);
    let tokens_in = base_in + cache_read + cache_creation;
    let tokens_out = u64_at(&["output_tokens", "completion_tokens"]).unwrap_or(0);
    // Claude Code's stream-json result reports the model + a total cost at the top level.
    let model = result
        .get("model")
        .or_else(|| usage.get("model"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let cost_usd = result
        .get("total_cost_usd")
        .or_else(|| result.get("cost_usd"))
        .and_then(Value::as_f64);
    Some(crate::drive::Usage {
        model,
        tokens_in,
        tokens_out,
        cost_usd,
    })
}

/// Map one output event to a user-visible [`TurnEvent`], or `None` to skip it.
fn map_event(v: &Value) -> Option<TurnEvent> {
    match v.get("type").and_then(Value::as_str) {
        Some("assistant") => {
            let text = assistant_text(v);
            (!text.is_empty()).then_some(TurnEvent::Output(text))
        }
        Some("result") => {
            let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let ok = !is_error && v.get("subtype").and_then(Value::as_str) == Some("success");
            Some(TurnEvent::Done(if ok {
                DoneReason::Completed
            } else {
                DoneReason::Failed(1)
            }))
        }
        _ => None, // system/init, rate_limit_event, tool-result user echoes, …
    }
}

/// Concatenate the text blocks of an `assistant` message event.
fn assistant_text(v: &Value) -> String {
    let mut out = String::new();
    if let Some(blocks) = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    {
        for b in blocks {
            if b.get("type").and_then(Value::as_str) == Some("text")
                && let Some(t) = b.get("text").and_then(Value::as_str)
            {
                out.push_str(t);
            }
        }
    }
    out
}

impl ClaudeTurn {
    /// Advance to the next mappable event, blocking until one arrives (or the turn ends /
    /// the channel closes / it times out). Setting `done` is terminal.
    fn pull(&mut self) -> Option<TurnEvent> {
        if self.done.is_some() {
            return None;
        }
        let deadline = Instant::now() + TURN_TIMEOUT;
        let mut events = self.inner.events.lock().unwrap();
        loop {
            while self.cursor < events.len() {
                let ev = &events[self.cursor];
                // Capture token usage from the terminal `result` event (HS2-TJ8FGR).
                if ev.get("type").and_then(Value::as_str) == Some("result") {
                    self.usage = claude_result_usage(ev);
                }
                let mapped = map_event(ev);
                self.cursor += 1;
                if let Some(te) = mapped {
                    if let TurnEvent::Done(r) = &te {
                        self.done = Some(*r);
                    }
                    return Some(te);
                }
            }
            if self.inner.closed.load(Ordering::SeqCst) {
                let r = DoneReason::Failed(1);
                self.done = Some(r);
                return Some(TurnEvent::Done(r));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let r = DoneReason::Failed(1);
                self.done = Some(r);
                return Some(TurnEvent::Done(r));
            }
            let (g, _) = self.inner.cvar.wait_timeout(events, remaining).unwrap();
            events = g;
        }
    }
}

impl TurnHandle for ClaudeTurn {
    fn is_busy(&mut self) -> bool {
        if self.done.is_some() || self.inner.closed.load(Ordering::SeqCst) {
            return false;
        }
        // Read-only peek: a `result` at/after the cursor means the turn is finished.
        let events = self.inner.events.lock().unwrap();
        !events[self.cursor..]
            .iter()
            .any(|e| e.get("type").and_then(Value::as_str) == Some("result"))
    }

    fn wait(&mut self) -> DoneReason {
        if let Some(r) = self.done {
            return r;
        }
        loop {
            match self.pull() {
                Some(TurnEvent::Done(r)) => return r,
                Some(_) => continue,
                None => return self.done.unwrap_or(DoneReason::Failed(1)),
            }
        }
    }

    fn next_event(&mut self) -> Option<TurnEvent> {
        self.pull()
    }

    fn usage(&mut self) -> Option<crate::drive::Usage> {
        self.usage.clone()
    }
}

// ---- the drive -------------------------------------------------------------------

/// Drives Claude via a channel injection into its running stream-json session.
pub struct ClaudeChannelDrive;

impl Drive for ClaudeChannelDrive {
    fn info(&self) -> DriveInfo {
        DriveInfo {
            transport: Transport::ClaudeChannel,
        }
    }

    fn run(
        &self,
        _target: &Target,
        content: &str,
        ctx: &DriveCtx,
    ) -> Result<Box<dyn TurnHandle>, DriveError> {
        let channel = ctx
            .channel
            .ok_or_else(|| DriveError::NotConnected("claude channel not connected".into()))?;
        channel.start_turn(content)
    }
}

// ---- the real transport: a `claude` stream-json child ----------------------------

/// A live [`RpcTransport`] backed by a `claude -p --…-format stream-json` child — a
/// persistent process the channel drive injects turns into (no process per play).
/// Live-only; unit tests inject a scripted fake instead.
pub struct ClaudeStreamTransport(StreamChild);

impl ClaudeStreamTransport {
    /// Spawn `program` as a stream-json channel in `cwd`. `resume` continues a prior
    /// session (`--resume <id>`); `mcp_config`, when set, is the only MCP config used
    /// (`--strict-mcp-config --mcp-config <path>`), so nothing else is reachable.
    pub fn spawn(
        program: &str,
        cwd: &Path,
        resume: Option<&str>,
        mcp_config: Option<&Path>,
        permission_mode: Option<&str>,
        env: &[(String, String)],
    ) -> std::io::Result<Box<Self>> {
        let mut args: Vec<String> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if let Some(id) = resume {
            args.push("--resume".into());
            args.push(id.to_string());
        }
        if let Some(cfg) = mcp_config {
            args.push("--strict-mcp-config".into());
            args.push("--mcp-config".into());
            args.push(cfg.display().to_string());
        }
        // Headless work needs a non-blocking permission mode so tools (edit/bash) don't
        // stall waiting for a prompt (`docs/05` §5.7; the real bridge is HS2-113).
        if let Some(mode) = permission_mode {
            args.push("--permission-mode".into());
            args.push(mode.to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        Ok(Box::new(Self(StreamChild::spawn(
            program, &refs, cwd, env,
        )?)))
    }
}

impl RpcTransport for ClaudeStreamTransport {
    fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
        self.0.into_halves()
    }
}

// ---- a scripted claude for tests (no live claude) --------------------------------

/// A loopback [`RpcTransport`] that answers the channel protocol in-process, so
/// [`ClaudeChannel`] is tested end to end (init → user message → assistant → result) with
/// no `claude` binary. Exposed for the crate's tests.
#[cfg(test)]
pub(crate) mod scripted {
    use super::*;
    use std::sync::mpsc::{Receiver, Sender, channel};

    /// How the scripted claude resolves each turn.
    #[derive(Clone, Copy)]
    pub enum ClaudeMode {
        /// Emit an assistant message then a `result` (`success`).
        Success,
        /// Emit an assistant message then a `result` (`error_during_execution`).
        Failure,
    }

    pub struct ScriptedClaude {
        mode: ClaudeMode,
    }

    impl ScriptedClaude {
        pub fn new(mode: ClaudeMode) -> Box<Self> {
            Box::new(Self { mode })
        }
    }

    impl RpcTransport for ScriptedClaude {
        fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
            let (tx, rx) = channel::<String>();
            // `system`/`init` is emitted once at process start (carries the session id).
            let _ = tx.send(
                json!({ "type": "system", "subtype": "init",
                        "session_id": "sess-abc", "tools": [] })
                .to_string(),
            );
            (
                Box::new(ClaudeWriter {
                    out: tx,
                    mode: self.mode,
                }),
                Box::new(ClaudeReader { inbox: rx }),
            )
        }
    }

    struct ClaudeWriter {
        out: Sender<String>,
        mode: ClaudeMode,
    }
    impl ClaudeWriter {
        fn push(&self, v: Value) {
            let _ = self.out.send(v.to_string());
        }
    }
    impl RpcWriter for ClaudeWriter {
        fn send(&mut self, msg: &str) -> std::io::Result<()> {
            let v: Value = serde_json::from_str(msg).expect("client sends valid JSON");
            if v.get("type").and_then(Value::as_str) == Some("user") {
                let content = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                self.push(json!({ "type": "assistant",
                    "message": { "role": "assistant",
                                 "content": [{ "type": "text", "text": format!("done: {content}") }] },
                    "session_id": "sess-abc" }));
                let (subtype, is_error, result) = match self.mode {
                    ClaudeMode::Success => ("success", false, "done"),
                    ClaudeMode::Failure => ("error_during_execution", true, ""),
                };
                self.push(
                    json!({ "type": "result", "subtype": subtype, "is_error": is_error,
                    "result": result, "session_id": "sess-abc", "num_turns": 1,
                    "model": "claude-opus-4-8",
                    "usage": { "input_tokens": 800, "output_tokens": 150 } }),
                );
            }
            Ok(())
        }
    }

    struct ClaudeReader {
        inbox: Receiver<String>,
    }
    impl RpcReader for ClaudeReader {
        fn recv(&mut self) -> std::io::Result<Option<String>> {
            Ok(self.inbox.recv().ok())
        }
    }
}

#[cfg(test)]
mod usage_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_result_usage_sums_cached_input_and_reads_cost_and_model() {
        // The Claude Code stream-json `result` event: base input + cache read + cache
        // creation are distinct fields; all are input the model processed (HS2-CQ6B96).
        let ev = json!({
            "type": "result",
            "model": "claude-opus-4-8",
            "total_cost_usd": 0.0731,
            "usage": {
                "input_tokens": 1000,
                "cache_read_input_tokens": 4000,
                "cache_creation_input_tokens": 200,
                "output_tokens": 350
            }
        });
        let u = claude_result_usage(&ev).unwrap();
        assert_eq!(u.tokens_in, 5200, "1000 + 4000 + 200");
        assert_eq!(u.tokens_out, 350);
        assert_eq!(u.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(u.cost_usd, Some(0.0731));
    }

    #[test]
    fn claude_result_usage_handles_the_minimal_shape() {
        // Only base input/output, no cache, no cost → still parses; cost is None.
        let u = claude_result_usage(&json!({
            "usage": { "input_tokens": 12, "output_tokens": 3 }
        }))
        .unwrap();
        assert_eq!((u.tokens_in, u.tokens_out), (12, 3));
        assert_eq!(u.cost_usd, None);
        assert_eq!(u.model, None);

        // No usage block → nothing to attribute.
        assert!(claude_result_usage(&json!({ "type": "result" })).is_none());
    }
}
