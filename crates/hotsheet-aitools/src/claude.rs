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

use hotsheet_sync::LockExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::drive::{
    ClaudeChannelClient, DoneReason, Drive, DriveCtx, DriveError, DriveInfo, Target, Transport,
    TurnControl, TurnEvent, TurnHandle,
};
use crate::ports::{ProcessSignals, RpcReader, RpcTransport, RpcWriter};
use crate::procio::{StreamChild, stop_process};

/// How long a turn may wait without a new event before its process is stopped.
const TURN_TIMEOUT: Duration = Duration::from_secs(600);

/// How long a stopped `claude` process gets to exit after SIGTERM before SIGKILL.
const TERMINATE_GRACE: Duration = Duration::from_secs(5);

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
    /// Signals the backing `claude` process, when the transport owns one (HS2-V0SWSM).
    signals: Mutex<Option<Box<dyn ProcessSignals>>>,
    /// Set by a [`TurnControl`] wake callback; the blocked turn stops the process.
    interrupt_requested: AtomicBool,
    turn_timeout: Duration,
    terminate_grace: Duration,
}

impl Inner {
    fn send_user(&self, content: &str) -> Result<(), DriveError> {
        let msg = json!({ "type": "user", "message": { "role": "user", "content": content } });
        self.writer
            .lock_or_recover()
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
            *self.session_id.lock_or_recover() = Some(sid.to_string());
        }
        let mut events = self.events.lock_or_recover();
        events.push(v);
        self.cvar.notify_all();
    }

    fn mark_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let _guard = self.events.lock_or_recover();
        self.cvar.notify_all();
    }

    fn events_len(&self) -> usize {
        self.events.lock_or_recover().len()
    }

    /// Deliver EOF on the child's stdin so a per-turn process exits once its queued work ends.
    fn close_input(&self) {
        self.writer.lock_or_recover().close();
    }

    /// Wake a blocked turn so it observes a Stop request (runs on the requesting thread).
    fn request_interrupt(&self) {
        self.interrupt_requested.store(true, Ordering::SeqCst);
        let _guard = self.events.lock_or_recover();
        self.cvar.notify_all();
    }

    /// Stop the backing process: SIGTERM, then SIGKILL after the grace period. Returns
    /// whether a process handle exists (a transport without one cannot be stopped).
    fn stop_process(&self) -> bool {
        let mut signals = self.signals.lock_or_recover();
        match signals.as_mut() {
            Some(signals) => {
                stop_process(signals.as_mut(), self.terminate_grace);
                true
            }
            None => false,
        }
    }

    fn can_stop(&self) -> bool {
        self.signals.lock_or_recover().is_some()
    }
}

fn start_reader(transport: Box<dyn RpcTransport>, timing: ChannelTiming) -> Arc<Inner> {
    let signals = transport.process_signals();
    let (writer, mut reader) = transport.split();
    let inner = Arc::new(Inner {
        writer: Mutex::new(writer),
        events: Mutex::new(Vec::new()),
        cvar: Condvar::new(),
        closed: AtomicBool::new(false),
        session_id: Mutex::new(None),
        signals: Mutex::new(signals),
        interrupt_requested: AtomicBool::new(false),
        turn_timeout: timing.turn_timeout,
        terminate_grace: timing.terminate_grace,
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

/// How long a channel waits for turn events and for a stopped process to exit.
#[derive(Debug, Clone, Copy)]
pub struct ChannelTiming {
    /// The longest wait for the next event before the turn times out and its process stops.
    pub turn_timeout: Duration,
    /// How long a stopped process gets after SIGTERM before SIGKILL.
    pub terminate_grace: Duration,
}

impl Default for ChannelTiming {
    fn default() -> Self {
        Self {
            turn_timeout: TURN_TIMEOUT,
            terminate_grace: TERMINATE_GRACE,
        }
    }
}

// ---- the ClaudeChannelClient over the engine -------------------------------------

/// A live, persistent `claude` stream-json session, implementing [`ClaudeChannelClient`]:
/// each `start_turn` injects one user message and returns a streaming [`TurnHandle`].
pub struct ClaudeChannel {
    inner: Arc<Inner>,
    /// The process exists for exactly one turn (HS2-M5GN19); see [`Self::connect_single_turn`].
    single_turn: bool,
}

impl ClaudeChannel {
    /// Attach to a running `claude` session over `transport` (starts the reader thread).
    pub fn connect(transport: Box<dyn RpcTransport>) -> Self {
        Self {
            inner: start_reader(transport, ChannelTiming::default()),
            single_turn: false,
        }
    }

    /// Attach to a `claude` process spawned for exactly one turn (the live drive spawns one
    /// `--resume` process per turn). A `result` does not end such a turn: the channel closes
    /// stdin and keeps streaming until the process exits, so output, tool use, and usage
    /// produced after an early or extra `result` reach the turn instead of an unobserved
    /// orphaned process (HS2-M5GN19). The last `result` decides the outcome.
    pub fn connect_single_turn(transport: Box<dyn RpcTransport>) -> Self {
        Self::connect_single_turn_with(transport, ChannelTiming::default())
    }

    /// [`Self::connect_single_turn`] with explicit timing (tests shorten the waits).
    pub fn connect_single_turn_with(
        transport: Box<dyn RpcTransport>,
        timing: ChannelTiming,
    ) -> Self {
        Self {
            inner: start_reader(transport, timing),
            single_turn: true,
        }
    }

    /// Wake this channel's running turn the moment `control` requests an interrupt, so a
    /// Stop reaches a turn blocked waiting for Claude's next event (HS2-V0SWSM). The turn
    /// then stops the process on its owner thread and reports [`DoneReason::Interrupted`].
    pub fn wake_on_interrupt(&self, control: &TurnControl) {
        let inner = Arc::downgrade(&self.inner);
        control.on_interrupt(move || {
            if let Some(inner) = inner.upgrade() {
                inner.request_interrupt();
            }
        });
    }
}

impl ClaudeChannelClient for ClaudeChannel {
    fn session_id(&self) -> Option<String> {
        self.inner.session_id.lock_or_recover().clone()
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
            pending: std::collections::VecDeque::new(),
            assistant_projection: AssistantProjection::default(),
            single_turn: self.single_turn,
            settled: None,
            emitted_output: false,
            result_text: None,
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
    /// Token usage aggregated from this turn's `result` events (HS2-TJ8FGR). A result that
    /// reports no model work (zero tokens and no cost) contributes nothing, so a turn never
    /// reports `0 tokens` while it consumed some (HS2-M5GN19).
    usage: Option<crate::drive::Usage>,
    pending: std::collections::VecDeque<TurnEvent>,
    /// Claude may repeat a complete `assistant` stream record while changing or omitting
    /// its provider message id. Track both identified repeats and the immediately preceding
    /// text projection so transport retries cannot duplicate prose in one host message.
    assistant_projection: AssistantProjection,
    /// Single-turn process: a `result` closes stdin and the turn ends at EOF.
    single_turn: bool,
    /// The latest `result` outcome of a single-turn process, applied at EOF.
    settled: Option<DoneReason>,
    /// Whether this turn has emitted any assistant text.
    emitted_output: bool,
    /// The latest successful `result` text, shown when the turn emitted no assistant text.
    result_text: Option<String>,
}

#[derive(Default)]
struct AssistantProjection {
    seen_by_id: std::collections::HashSet<(String, String)>,
    last_text: Option<String>,
}

/// Extract token usage from a Claude stream-json `result` event (`docs/14`, the
/// `claude-usage` mapper for HS2-TJ8FGR — reads the in-band stream result rather than a
/// separate OTLP collector). Lenient: `None` when the event reports no usage, so a schema
/// tweak degrades quietly. Field names and cost were live-verified in HS2-CQ6B96; the
/// result may omit `model`, so model attribution remains optional.
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
    // Claude Code's stream-json result may report a model and reports total cost at the top level.
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

/// Fold one `result` usage into a turn's total. Claude reports `usage` per result but
/// `total_cost_usd` cumulatively for the process, so tokens add and cost keeps the maximum.
/// A result without model work (no tokens, no cost) is ignored.
fn absorb_usage(total: &mut Option<crate::drive::Usage>, usage: Option<crate::drive::Usage>) {
    let Some(usage) = usage else {
        return;
    };
    if usage.tokens_in == 0 && usage.tokens_out == 0 && usage.cost_usd.unwrap_or(0.0) <= 0.0 {
        return;
    }
    let merged = match total.take() {
        None => usage,
        Some(previous) => crate::drive::Usage {
            model: usage.model.or(previous.model),
            tokens_in: previous.tokens_in + usage.tokens_in,
            tokens_out: previous.tokens_out + usage.tokens_out,
            cost_usd: match (previous.cost_usd, usage.cost_usd) {
                (Some(left), Some(right)) => Some(left.max(right)),
                (left, right) => left.or(right),
            },
        },
    };
    *total = Some(merged);
}

/// Map one output event to zero or more user-visible/native [`TurnEvent`] values.
fn map_events(v: &Value, assistant_projection: &mut AssistantProjection) -> Vec<TurnEvent> {
    match v.get("type").and_then(Value::as_str) {
        Some("assistant") => {
            let text = assistant_text(v, assistant_projection);
            let mut events = Vec::new();
            if !text.is_empty() {
                events.push(TurnEvent::Output(text));
            }
            let session = v.get("session_id").and_then(Value::as_str);
            if let Some(blocks) = v
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array)
            {
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                        continue;
                    }
                    let mut payload = json!({
                        "hook_event_name": "PreToolUse",
                        "tool_name": block.get("name").and_then(Value::as_str).unwrap_or(""),
                        "tool_input": block.get("input").cloned().unwrap_or(Value::Null),
                    });
                    // The call id lets the host clear this tool when its result arrives (HS2-XV1VCR).
                    if let Some(id) = block.get("id").and_then(Value::as_str) {
                        payload["tool_use_id"] = Value::String(id.to_string());
                    }
                    if let Some(session) = session {
                        payload["session_id"] = Value::String(session.to_string());
                    }
                    events.push(TurnEvent::NativeActivity {
                        source: "claude-hooks".into(),
                        payload,
                    });
                }
            }
            events
        }
        Some("result") => {
            let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let ok = !is_error && v.get("subtype").and_then(Value::as_str) == Some("success");
            vec![TurnEvent::Done(if ok {
                DoneReason::Completed
            } else {
                let message = v
                    .get("result")
                    .and_then(Value::as_str)
                    .filter(|message| !message.trim().is_empty())
                    .or_else(|| {
                        v.get("errors")
                            .and_then(Value::as_array)
                            .and_then(|errors| {
                                errors
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .find(|message| !message.trim().is_empty())
                            })
                    })
                    .or_else(|| {
                        v.get("subtype")
                            .and_then(Value::as_str)
                            .filter(|message| !message.trim().is_empty())
                    })
                    .unwrap_or("Claude turn failed");
                DoneReason::failed(1, message)
            })]
        }
        _ => Vec::new(), // system/init, rate_limit_event, tool-result user echoes, …
    }
}

/// Concatenate the text blocks of an `assistant` message event. Suppress identified repeats
/// anywhere in the turn and consecutive exact repeats even when Claude changes or omits the
/// provider id. A different intervening projection remains a boundary, so later intentional
/// repetition is preserved.
fn assistant_text(v: &Value, projection: &mut AssistantProjection) -> String {
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
    if out.is_empty() {
        return out;
    }
    let repeated_consecutively = projection.last_text.as_deref() == Some(out.as_str());
    projection.last_text = Some(out.clone());
    let repeated_by_id = v
        .get("message")
        .and_then(|message| message.get("id"))
        .and_then(Value::as_str)
        .is_some_and(|message_id| {
            !projection
                .seen_by_id
                .insert((message_id.to_string(), out.clone()))
        });
    if repeated_consecutively || repeated_by_id {
        String::new()
    } else {
        out
    }
}

impl ClaudeTurn {
    /// Advance to the next mappable event, blocking until one arrives (or the turn ends /
    /// the channel closes / it times out). Returning `Done` is terminal.
    fn pull(&mut self) -> Option<TurnEvent> {
        if let Some(event) = self.pending.pop_front() {
            return Some(self.emit(event));
        }
        if self.done.is_some() {
            return None;
        }
        let inner = self.inner.clone();
        let deadline = Instant::now() + inner.turn_timeout;
        let mut events = inner.events.lock_or_recover();
        loop {
            if inner.interrupt_requested.load(Ordering::SeqCst) && inner.can_stop() {
                // Stop requested while the turn waits: end the process on this thread
                // (the reader needs the event lock to observe its exit).
                drop(events);
                self.interrupt();
                return self.pending.pop_front().map(|event| self.emit(event));
            }
            while self.cursor < events.len() {
                let ev = &events[self.cursor];
                if ev.get("type").and_then(Value::as_str) == Some("result") {
                    // Capture token usage from every `result` event (HS2-TJ8FGR, HS2-M5GN19).
                    absorb_usage(&mut self.usage, claude_result_usage(ev));
                    if ev.get("subtype").and_then(Value::as_str) == Some("success")
                        && let Some(text) = ev
                            .get("result")
                            .and_then(Value::as_str)
                            .filter(|text| !text.trim().is_empty())
                    {
                        self.result_text = Some(text.to_string());
                    }
                }
                let mapped = map_events(ev, &mut self.assistant_projection);
                self.cursor += 1;
                for event in mapped {
                    match event {
                        TurnEvent::Done(reason) if self.single_turn => {
                            if self.settled.is_none() {
                                inner.close_input();
                            }
                            self.settled = Some(reason);
                        }
                        TurnEvent::Done(reason) => self.finish(reason),
                        TurnEvent::Output(text) => {
                            self.emitted_output = true;
                            self.pending.push_back(TurnEvent::Output(text));
                        }
                        other => self.pending.push_back(other),
                    }
                }
                if let Some(event) = self.pending.pop_front() {
                    return Some(self.emit(event));
                }
            }
            if inner.closed.load(Ordering::SeqCst) {
                let reason = self
                    .settled
                    .take()
                    .unwrap_or_else(|| DoneReason::failed(1, "Claude channel closed"));
                self.finish(reason);
                return self.pending.pop_front().map(|event| self.emit(event));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // Never leave a timed-out process working unobserved (HS2-V0SWSM).
                drop(events);
                let stopped = inner.stop_process();
                let reason = self.settled.take().unwrap_or_else(|| {
                    DoneReason::failed(
                        1,
                        if stopped {
                            "Timed out waiting for Claude turn result; the Claude process was stopped"
                        } else {
                            "Timed out waiting for Claude turn result"
                        },
                    )
                });
                self.finish(reason);
                return self.pending.pop_front().map(|event| self.emit(event));
            }
            let (g, _) = inner.cvar.wait_timeout(events, remaining).unwrap();
            events = g;
        }
    }

    /// Queue the terminal outcome. A successful turn that emitted no assistant text shows
    /// its final `result` text instead, so a tool-only reply is not presented as empty.
    fn finish(&mut self, reason: DoneReason) {
        if !self.emitted_output
            && reason == DoneReason::Completed
            && let Some(text) = self.result_text.take()
        {
            self.emitted_output = true;
            self.pending.push_back(TurnEvent::Output(text));
        }
        self.pending.push_back(TurnEvent::Done(reason));
    }

    /// Record a terminal event as it leaves the turn.
    fn emit(&mut self, event: TurnEvent) -> TurnEvent {
        if let TurnEvent::Done(reason) = &event {
            self.done = Some(reason.clone());
        }
        event
    }
}

impl TurnHandle for ClaudeTurn {
    fn is_busy(&mut self) -> bool {
        if self.done.is_some() || self.inner.closed.load(Ordering::SeqCst) {
            return false;
        }
        if self.single_turn {
            // A per-turn process stays busy until it exits, even after a `result`.
            return true;
        }
        // Read-only peek: a `result` at/after the cursor means the turn is finished.
        let events = self.inner.events.lock_or_recover();
        !events[self.cursor..]
            .iter()
            .any(|e| e.get("type").and_then(Value::as_str) == Some("result"))
    }

    fn interrupt(&mut self) -> bool {
        if self.done.is_some() || self.pending.iter().any(|e| matches!(e, TurnEvent::Done(_))) {
            return false;
        }
        if !self.inner.stop_process() {
            return false;
        }
        // Output already queued stays ahead of the terminal event.
        self.pending
            .push_back(TurnEvent::Done(DoneReason::Interrupted));
        true
    }

    fn wait(&mut self) -> DoneReason {
        if let Some(r) = &self.done {
            return r.clone();
        }
        loop {
            match self.pull() {
                Some(TurnEvent::Done(r)) => return r,
                Some(_) => continue,
                None => {
                    return self.done.clone().unwrap_or(DoneReason::Failed {
                        exit_code: 1,
                        message: None,
                    });
                }
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

    /// A turn is stopped by ending its `claude` process (HS2-V0SWSM).
    fn supports_interrupt(&self) -> bool {
        true
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

/// Optional arguments for a persistent Claude stream-json child.
#[derive(Debug, Default)]
pub struct ClaudeStreamSpawnOptions<'a> {
    pub resume: Option<&'a str>,
    pub mcp_config: Option<&'a Path>,
    pub permission_mode: Option<&'a str>,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub env: &'a [(String, String)],
}

fn headless_claude_env(input: &[(String, String)]) -> Vec<(String, String)> {
    let mut env = input.to_vec();
    env.retain(|(key, _)| key != "HOTSHEET_CLAUDE_PRETOOLUSE");
    env.push(("HOTSHEET_CLAUDE_PRETOOLUSE".into(), "1".into()));
    env
}

impl ClaudeStreamTransport {
    /// Spawn `program` as a stream-json channel in `cwd`. `resume` continues a prior
    /// session (`--resume <id>`); `mcp_config`, when set, is the only MCP config used
    /// (`--strict-mcp-config --mcp-config <path>`), so nothing else is reachable.
    pub fn spawn(
        program: &str,
        cwd: &Path,
        options: ClaudeStreamSpawnOptions<'_>,
    ) -> std::io::Result<Box<Self>> {
        let mut args: Vec<String> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-hook-events",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if let Some(id) = options.resume {
            args.push("--resume".into());
            args.push(id.to_string());
        }
        if let Some(cfg) = options.mcp_config {
            args.push("--strict-mcp-config".into());
            args.push("--mcp-config".into());
            args.push(cfg.display().to_string());
        }
        // Headless work needs a non-blocking permission mode so tools (edit/bash) don't
        // stall waiting for a prompt (`docs/05` §5.7; the real bridge is HS2-113).
        if let Some(mode) = options.permission_mode {
            args.push("--permission-mode".into());
            args.push(mode.to_string());
        }
        if let Some(model) = options.model {
            args.push("--model".into());
            args.push(model.to_string());
        }
        if let Some(effort) = options.effort {
            args.push("--effort".into());
            args.push(effort.to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let env = headless_claude_env(options.env);
        Ok(Box::new(Self(StreamChild::spawn(
            program, &refs, cwd, &env,
        )?)))
    }
}

impl RpcTransport for ClaudeStreamTransport {
    fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
        self.0.into_halves()
    }

    fn process_signals(&self) -> Option<Box<dyn ProcessSignals>> {
        Some(self.0.signals())
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
        /// Repeat one provider-identified assistant record before a successful result.
        DuplicateOutput,
        /// Emit an assistant message then a `result` (`error_during_execution`).
        Failure,
        /// Emit text plus a tool-use block, then a successful result.
        RichSuccess,
        /// The captured HS2-M5GN19 shape: a successful zero-usage `result` arrives before any
        /// model work, then the real tool use, text, and usage-bearing result follow.
        PrematureResult,
        /// Only a tool-use block, then a successful result whose `result` text is the reply.
        ToolOnly,
        /// The process exits on the user message without emitting a `result`.
        Crash,
        /// Emit some output, then keep working (no `result`, ignores stdin EOF) until a
        /// signal stops it — a long turn the user stops (HS2-V0SWSM).
        Hang,
        /// Like [`ClaudeMode::Hang`], but SIGTERM is ignored; only SIGKILL ends it.
        HangIgnoringTerm,
    }

    /// The scripted process's output pipe: `None` once it has exited.
    type Pipe = Arc<Mutex<Option<Sender<String>>>>;

    pub struct ScriptedClaude {
        mode: ClaudeMode,
        pipe: Pipe,
        rx: Receiver<String>,
        signal_log: Arc<Mutex<Vec<&'static str>>>,
    }

    impl ScriptedClaude {
        pub fn new(mode: ClaudeMode) -> Box<Self> {
            let (tx, rx) = channel::<String>();
            // `system`/`init` is emitted once at process start (carries the session id).
            let _ = tx.send(
                json!({ "type": "system", "subtype": "init",
                        "session_id": "sess-abc", "tools": [] })
                .to_string(),
            );
            Box::new(Self {
                mode,
                pipe: Arc::new(Mutex::new(Some(tx))),
                rx,
                signal_log: Arc::default(),
            })
        }

        /// The signals the scripted process received, in order (`"TERM"`, `"KILL"`).
        pub fn signal_log(&self) -> Arc<Mutex<Vec<&'static str>>> {
            self.signal_log.clone()
        }
    }

    impl RpcTransport for ScriptedClaude {
        fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
            (
                Box::new(ClaudeWriter {
                    out: self.pipe,
                    mode: self.mode,
                }),
                Box::new(ClaudeReader { inbox: self.rx }),
            )
        }

        fn process_signals(&self) -> Option<Box<dyn ProcessSignals>> {
            Some(Box::new(ScriptedSignals {
                pipe: self.pipe.clone(),
                ignore_term: matches!(self.mode, ClaudeMode::HangIgnoringTerm),
                log: self.signal_log.clone(),
            }))
        }
    }

    /// Signals for the scripted process: exiting drops its output pipe (reader EOF).
    struct ScriptedSignals {
        pipe: Pipe,
        ignore_term: bool,
        log: Arc<Mutex<Vec<&'static str>>>,
    }
    impl ProcessSignals for ScriptedSignals {
        fn terminate(&mut self) {
            self.log.lock_or_recover().push("TERM");
            if !self.ignore_term {
                self.pipe.lock_or_recover().take();
            }
        }
        fn kill(&mut self) {
            self.log.lock_or_recover().push("KILL");
            self.pipe.lock_or_recover().take();
        }
        fn has_exited(&mut self) -> bool {
            self.pipe.lock_or_recover().is_none()
        }
    }

    struct ClaudeWriter {
        /// Dropped on `close`, which ends the reader like a process exiting after stdin EOF.
        out: Pipe,
        mode: ClaudeMode,
    }
    impl ClaudeWriter {
        fn push(&self, v: Value) {
            if let Some(out) = self.out.lock_or_recover().as_ref() {
                let _ = out.send(v.to_string());
            }
        }

        fn exit(&self) {
            self.out.lock_or_recover().take();
        }

        fn tool_use(&self) -> Value {
            json!({ "type": "assistant",
                "message": { "role": "assistant", "id": "msg-tool",
                             "content": [{ "type": "tool_use", "id": "tool-redacted",
                                           "name": "mcp__hotsheet__hotsheet_claim",
                                           "input": { "ticket": "HS2-XMJGFJ" } }] },
                "session_id": "sess-abc" })
        }
    }
    impl RpcWriter for ClaudeWriter {
        fn close(&mut self) {
            // A hanging process keeps working after stdin EOF.
            if !matches!(self.mode, ClaudeMode::Hang | ClaudeMode::HangIgnoringTerm) {
                self.exit();
            }
        }

        fn send(&mut self, msg: &str) -> std::io::Result<()> {
            let v: Value = serde_json::from_str(msg).expect("client sends valid JSON");
            if v.get("type").and_then(Value::as_str) == Some("user") {
                let content = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                match self.mode {
                    ClaudeMode::Crash => {
                        self.exit();
                        return Ok(());
                    }
                    ClaudeMode::Hang | ClaudeMode::HangIgnoringTerm => {
                        self.push(json!({ "type": "assistant",
                            "message": { "role": "assistant", "id": "msg-working",
                                         "content": [{ "type": "text",
                                                       "text": format!("working: {content}") }] },
                            "session_id": "sess-abc" }));
                        return Ok(());
                    }
                    ClaudeMode::PrematureResult => {
                        self.push(json!({ "type": "result", "subtype": "success",
                            "is_error": false, "result": "", "session_id": "sess-abc",
                            "num_turns": 0, "total_cost_usd": 0.0,
                            "usage": { "input_tokens": 0, "output_tokens": 0 } }));
                        self.push(self.tool_use());
                    }
                    ClaudeMode::ToolOnly => {
                        self.push(self.tool_use());
                        self.push(json!({ "type": "result", "subtype": "success",
                            "is_error": false, "result": "Claimed HS2-XMJGFJ and started work.",
                            "session_id": "sess-abc", "num_turns": 2, "total_cost_usd": 0.04,
                            "usage": { "input_tokens": 900, "output_tokens": 60 } }));
                        return Ok(());
                    }
                    _ => {}
                }
                let assistant = json!({ "type": "assistant",
                    "message": { "role": "assistant",
                                 "id": "msg-scripted",
                                 "content": if matches!(self.mode, ClaudeMode::RichSuccess) {
                                     json!([{ "type": "text", "text": format!("done: {content}") },
                                         { "type": "tool_use", "id": "tool-redacted",
                                           "name": "Bash", "input": { "command": "cargo test" } }])
                                 } else {
                                     json!([{ "type": "text", "text": format!("done: {content}") }])
                                 } },
                    "session_id": "sess-abc" });
                self.push(assistant.clone());
                if matches!(self.mode, ClaudeMode::DuplicateOutput) {
                    let mut changed_id = assistant.clone();
                    changed_id["message"]["id"] = Value::String("msg-scripted-retry".into());
                    self.push(changed_id);
                    let mut missing_id = assistant;
                    missing_id["message"].as_object_mut().unwrap().remove("id");
                    self.push(missing_id);
                }
                let (subtype, is_error, result) = match self.mode {
                    ClaudeMode::Success
                    | ClaudeMode::DuplicateOutput
                    | ClaudeMode::RichSuccess
                    | ClaudeMode::PrematureResult
                    | ClaudeMode::ToolOnly
                    | ClaudeMode::Crash
                    | ClaudeMode::Hang
                    | ClaudeMode::HangIgnoringTerm => ("success", false, "done"),
                    ClaudeMode::Failure => (
                        "error_during_execution",
                        true,
                        "Selected model is at capacity",
                    ),
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
    fn headless_stream_marks_pre_tool_permission_bridging() {
        let env = headless_claude_env(&[
            ("KEEP".into(), "yes".into()),
            ("HOTSHEET_CLAUDE_PRETOOLUSE".into(), "stale".into()),
        ]);
        assert_eq!(
            env.iter()
                .filter(|(key, _)| key == "HOTSHEET_CLAUDE_PRETOOLUSE")
                .count(),
            1
        );
        assert!(env.contains(&("HOTSHEET_CLAUDE_PRETOOLUSE".into(), "1".into())));
        assert!(env.contains(&("KEEP".into(), "yes".into())));
    }

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
    fn failed_result_uses_first_nonblank_diagnostic_and_success_clears_it() {
        for (value, expected) in [
            (
                json!({"type":"result","is_error":true,"result":" capacity ","errors":["other"]}),
                Some("capacity"),
            ),
            (
                json!({"type":"result","is_error":true,"result":" ","errors":["", "retry later"]}),
                Some("retry later"),
            ),
            (
                json!({"type":"result","is_error":true,"subtype":"error_during_execution"}),
                Some("error_during_execution"),
            ),
            (
                json!({"type":"result","is_error":true,"subtype":" "}),
                Some("Claude turn failed"),
            ),
            (
                json!({"type":"result","subtype":"success","result":"old diagnostic"}),
                None,
            ),
        ] {
            let events = map_events(&value, &mut AssistantProjection::default());
            let TurnEvent::Done(reason) = &events[0] else {
                panic!("expected terminal result");
            };
            assert_eq!(reason.failure_message().as_deref(), expected);
        }
    }

    #[test]
    fn turn_usage_skips_empty_results_adds_tokens_and_keeps_cumulative_cost() {
        let usage = |tokens_in, tokens_out, cost_usd, model: Option<&str>| {
            Some(crate::drive::Usage {
                model: model.map(str::to_string),
                tokens_in,
                tokens_out,
                cost_usd,
            })
        };
        let mut total = None;
        absorb_usage(&mut total, None);
        absorb_usage(&mut total, usage(0, 0, Some(0.0), None));
        absorb_usage(&mut total, usage(0, 0, None, Some("ignored")));
        assert_eq!(
            total, None,
            "results without model work are not usage (HS2-M5GN19)"
        );
        absorb_usage(
            &mut total,
            usage(800, 150, Some(0.03), Some("claude-opus-4-8")),
        );
        absorb_usage(&mut total, usage(0, 0, Some(0.0), None));
        absorb_usage(&mut total, usage(200, 50, Some(0.05), None));
        assert_eq!(
            total,
            usage(1000, 200, Some(0.05), Some("claude-opus-4-8")),
            "tokens add across results, process-cumulative cost keeps its maximum"
        );
        let mut unpriced = None;
        absorb_usage(&mut unpriced, usage(10, 1, None, None));
        absorb_usage(&mut unpriced, usage(5, 1, Some(0.01), None));
        assert_eq!(unpriced, usage(15, 2, Some(0.01), None));
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

    #[test]
    fn assistant_text_suppresses_transport_retries_without_global_text_deduplication() {
        let event = |id: Option<&str>, text: &str| {
            let mut message = json!({
                "role": "assistant",
                "content": [{ "type": "text", "text": text }]
            });
            if let Some(id) = id {
                message["id"] = Value::String(id.to_string());
            }
            json!({ "type": "assistant", "message": message })
        };
        let mut projection = AssistantProjection::default();

        assert_eq!(
            assistant_text(&event(Some("msg-a"), "same"), &mut projection),
            "same"
        );
        assert_eq!(
            assistant_text(&event(Some("msg-b"), "same"), &mut projection),
            ""
        );
        assert_eq!(assistant_text(&event(None, "same"), &mut projection), "");
        assert_eq!(
            assistant_text(&event(Some("msg-a"), " next"), &mut projection),
            " next"
        );
        assert_eq!(
            assistant_text(&event(Some("msg-c"), "same"), &mut projection),
            "same"
        );
        assert_eq!(
            assistant_text(&event(Some("msg-a"), "same"), &mut projection),
            ""
        );
    }
}
