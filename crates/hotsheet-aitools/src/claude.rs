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
    /// The highest `result.total_cost_usd` seen. Claude reports it cumulatively for the whole
    /// *session*: across turns in one process and across `--resume` processes (live-verified
    /// on 2.1.302, HS2-4JPDJA).
    session_cost_usd: Mutex<Option<f64>>,
    /// The session's cumulative cost before this process started, supplied by the host that
    /// resumes it, so the first turn reports only its own cost.
    cost_baseline_usd: Mutex<Option<f64>>,
}

impl Inner {
    /// The cumulative cost a new turn's own cost is measured from: what this process has
    /// already reported, else the host's baseline for a resumed session.
    fn turn_cost_baseline(&self) -> Option<f64> {
        (*self.session_cost_usd.lock_or_recover()).or(*self.cost_baseline_usd.lock_or_recover())
    }

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
        if v.get("type").and_then(Value::as_str) == Some("result")
            && let Some(cost) = v.get("total_cost_usd").and_then(Value::as_f64)
        {
            let mut seen = self.session_cost_usd.lock_or_recover();
            *seen = Some(seen.map_or(cost, |previous| previous.max(cost)));
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
        session_cost_usd: Mutex::new(None),
        cost_baseline_usd: Mutex::new(None),
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

    /// The session's cumulative cost before this process, for a `--resume` process whose
    /// host recorded it at the end of the previous turn (HS2-4JPDJA). Claude's
    /// `total_cost_usd` restarts from the resumed session's total, so without it the first
    /// turn reports every earlier turn's cost again. Ignored once this process has reported a
    /// cost of its own.
    pub fn set_session_cost_baseline(&self, baseline: Option<f64>) {
        *self.inner.cost_baseline_usd.lock_or_recover() = baseline.filter(|cost| *cost > 0.0);
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

    fn session_cost_usd(&self) -> Option<f64> {
        *self.inner.session_cost_usd.lock_or_recover()
    }

    fn start_turn(&self, content: &str) -> Result<Box<dyn TurnHandle>, DriveError> {
        // Capture the cursor BEFORE sending so a fast `result` can't slip past.
        let cursor = self.inner.events_len();
        let cost_baseline = self.inner.turn_cost_baseline();
        self.inner.send_user(content)?;
        Ok(Box::new(ClaudeTurn {
            inner: self.inner.clone(),
            cursor,
            cost_baseline,
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
    /// The session's cumulative cost when this turn started; the turn's own cost is the
    /// latest cumulative `total_cost_usd` minus it (HS2-4JPDJA).
    cost_baseline: Option<f64>,
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
    /// Tool names by `tool_use` id, so a `tool_result` names the tool it finished (HS2-RGMJSP).
    tool_names: std::collections::HashMap<String, String>,
}

/// Bound for one sub-agent message or background-task summary carried as native activity.
const MAX_NARRATED_CHARS: usize = 280;

/// The text of a narrated sub-agent message or task summary: trimmed, whitespace-collapsed,
/// and bounded so a chatty sub-agent cannot flood the activity timeline.
fn narrated(text: &str) -> Option<String> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let mut chars = collapsed.chars();
    let mut bounded = chars.by_ref().take(MAX_NARRATED_CHARS).collect::<String>();
    if chars.next().is_some() {
        bounded.pop();
        bounded.push('…');
    }
    Some(bounded)
}

/// Map a whole recorded stream-json transcript (one record per element) to the turn events a
/// live channel would emit, for offline protocol-cassette replay (HS2-RGMJSP).
pub fn claude_stream_events(records: &[Value]) -> Vec<TurnEvent> {
    let mut projection = AssistantProjection::default();
    records
        .iter()
        .flat_map(|record| map_events(record, &mut projection))
        .collect()
}

/// The sub-agent a stream record belongs to: Claude Code tags every record a Task/Agent
/// sub-agent produces with the parent call's `parent_tool_use_id`; main-agent records carry
/// `null`.
fn parent_tool_use_id(v: &Value) -> Option<&str> {
    v.get("parent_tool_use_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

fn claude_native(mut payload: Value, v: &Value) -> TurnEvent {
    if let Some(session) = v.get("session_id").and_then(Value::as_str) {
        payload["session_id"] = Value::String(session.to_string());
    }
    if let Some(parent) = parent_tool_use_id(v) {
        payload["parent_tool_use_id"] = Value::String(parent.to_string());
    }
    TurnEvent::NativeActivity {
        source: crate::tool_progress::CLAUDE_HOOKS_SOURCE.into(),
        payload,
    }
}

fn content_blocks(v: &Value) -> &[Value] {
    v.get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Each `tool_use` block of an assistant record as the verified `PreToolUse` hook contract.
fn tool_use_events(v: &Value, projection: &mut AssistantProjection) -> Vec<TurnEvent> {
    content_blocks(v)
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(|block| {
            let name = block.get("name").and_then(Value::as_str).unwrap_or("");
            let mut payload = json!({
                "hook_event_name": "PreToolUse",
                "tool_name": name,
                "tool_input": block.get("input").cloned().unwrap_or(Value::Null),
            });
            // The call id lets the host clear this tool when its result arrives (HS2-XV1VCR).
            if let Some(id) = block.get("id").and_then(Value::as_str) {
                payload["tool_use_id"] = Value::String(id.to_string());
                projection
                    .tool_names
                    .insert(id.to_string(), name.to_string());
            }
            claude_native(payload, v)
        })
        .collect()
}

/// Each `tool_result` block of a user record as a `PostToolUse` (or, for an error result,
/// `PostToolUseFailure`) hook-shaped event naming the call it finished (HS2-RGMJSP).
fn tool_result_events(v: &Value, projection: &AssistantProjection) -> Vec<TurnEvent> {
    content_blocks(v)
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        .map(|block| {
            let failed = block
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut payload = json!({
                "hook_event_name": if failed { "PostToolUseFailure" } else { "PostToolUse" },
            });
            if let Some(id) = block.get("tool_use_id").and_then(Value::as_str) {
                payload["tool_use_id"] = Value::String(id.to_string());
                if let Some(name) = projection.tool_names.get(id) {
                    payload["tool_name"] = Value::String(name.clone());
                }
            }
            claude_native(payload, v)
        })
        .collect()
}

/// A background-task `system` record (`task_started`, `task_progress`, `task_notification`)
/// as a `TaskNotification` event; other system records (init, compaction, status,
/// `background_tasks_changed`) are not activity. Claude 2.1.302 also emits `task_updated`
/// (a bare `patch` such as `{status, end_time}`) immediately before the `task_notification`
/// carrying the same status and a summary, so it is skipped rather than narrated twice
/// (HS2-NTP52W live capture).
fn task_notification_event(v: &Value) -> Option<TurnEvent> {
    let subtype = v.get("subtype").and_then(Value::as_str)?;
    if !subtype.starts_with("task_") || subtype == "task_updated" {
        return None;
    }
    let mut payload = json!({ "hook_event_name": "TaskNotification", "subtype": subtype });
    for key in ["task_id", "status", "task_type", "tool_use_id"] {
        if let Some(value) = v.get(key).and_then(Value::as_str) {
            payload[key] = Value::String(value.to_string());
        }
    }
    if let Some(summary) = ["summary", "description"]
        .iter()
        .find_map(|key| v.get(*key).and_then(Value::as_str).and_then(narrated))
    {
        payload["summary"] = Value::String(summary);
    }
    Some(claude_native(payload, v))
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
/// `total_cost_usd` cumulatively for the session, so tokens add and cost keeps the maximum;
/// the turn subtracts its starting baseline in `usage()` (HS2-4JPDJA).
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
        // A sub-agent's own text is its narration, never the main reply (HS2-RGMJSP).
        Some("assistant") if parent_tool_use_id(v).is_some() => {
            let text = content_blocks(v)
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let mut events = narrated(&text)
                .map(|text| {
                    claude_native(
                        json!({ "hook_event_name": "SubagentMessage", "text": text }),
                        v,
                    )
                })
                .into_iter()
                .collect::<Vec<_>>();
            events.extend(tool_use_events(v, assistant_projection));
            events
        }
        Some("user") => tool_result_events(v, assistant_projection),
        Some("system") => task_notification_event(v).into_iter().collect(),
        Some("assistant") => {
            let text = assistant_text(v, assistant_projection);
            let mut events = Vec::new();
            if !text.is_empty() {
                events.push(TurnEvent::Output(text));
            }
            events.extend(tool_use_events(v, assistant_projection));
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
        _ => Vec::new(), // rate_limit_event, stream_event deltas, …
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
        let mut usage = self.usage.clone()?;
        usage.cost_usd = turn_cost(usage.cost_usd, self.cost_baseline);
        Some(usage)
    }
}

/// This turn's own cost from the session-cumulative `total_cost_usd` (HS2-4JPDJA). A
/// cumulative cost below the baseline means the baseline belonged to another session, so the
/// reported cost stands as is.
fn turn_cost(cumulative: Option<f64>, baseline: Option<f64>) -> Option<f64> {
    match (cumulative, baseline) {
        (Some(total), Some(base)) if total >= base => Some(total - base),
        (total, _) => total,
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
        let args = stream_args(&options);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let env = headless_claude_env(options.env);
        Ok(Box::new(Self(StreamChild::spawn(
            program, &refs, cwd, &env,
        )?)))
    }
}

/// The `claude -p` stream-json argument list for a persistent child.
pub(crate) fn stream_args(options: &ClaudeStreamSpawnOptions<'_>) -> Vec<String> {
    {
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
        // A bypass-mode session holds every cross-session message for an approval dialog a
        // headless child cannot show; accept them so a Hot Sheet inbox message is not parked
        // (HS2-M15CHN, code.claude.com/docs/en/cross-session-messaging).
        if options.permission_mode == Some("bypassPermissions") {
            args.push("--settings".into());
            args.push(r#"{"crossSessionInbound":"accept"}"#.into());
        }
        args
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
        /// Replay the synthetic HS2-RGMJSP sub-agent / background-task stream-json cassette.
        SubagentStream,
        /// Replay a recorded stream-json cassette (a JSON array) on each user message, without
        /// its leading `system`/`init` (the scripted process already sent one).
        Replay(&'static str),
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
                    ClaudeMode::SubagentStream => {
                        let records: Vec<Value> = serde_json::from_str(include_str!(
                            "../tests/fixtures/claude-stream-subagent-synthetic.json"
                        ))
                        .expect("valid cassette");
                        // The channel already emitted its own `system`/`init` at start.
                        for record in records.into_iter().skip(1) {
                            self.push(record);
                        }
                        return Ok(());
                    }
                    ClaudeMode::Replay(cassette) => {
                        let records: Vec<Value> =
                            serde_json::from_str(cassette).expect("valid cassette");
                        let leading_init = records.first().is_some_and(|record| {
                            record.get("subtype").and_then(Value::as_str) == Some("init")
                        });
                        for record in records.into_iter().skip(usize::from(leading_init)) {
                            self.push(record);
                        }
                        return Ok(());
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
                    | ClaudeMode::SubagentStream
                    | ClaudeMode::Replay(_)
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
    fn only_a_bypass_mode_child_accepts_cross_session_messages() {
        // HS2-M15CHN: a bypass-mode session holds inbox messages for a dialog -p cannot show.
        let settings = |mode: Option<&str>| {
            let args = stream_args(&ClaudeStreamSpawnOptions {
                permission_mode: mode,
                ..Default::default()
            });
            args.iter()
                .position(|arg| arg == "--settings")
                .map(|at| args[at + 1].clone())
        };
        assert_eq!(
            settings(Some("bypassPermissions")).as_deref(),
            Some(r#"{"crossSessionInbound":"accept"}"#)
        );
        assert_eq!(settings(Some("acceptEdits")), None);
        assert_eq!(settings(None), None);
        let args = stream_args(&ClaudeStreamSpawnOptions {
            permission_mode: Some("auto"),
            model: Some("haiku"),
            ..Default::default()
        });
        assert_eq!(&args[..2], ["-p", "--input-format"]);
        assert!(args.windows(2).any(|w| w == ["--permission-mode", "auto"]));
        assert!(args.windows(2).any(|w| w == ["--model", "haiku"]));
    }

    #[test]
    fn turn_cost_subtracts_a_matching_session_baseline() {
        // HS2-4JPDJA: Claude's total_cost_usd is session-cumulative across --resume.
        assert_eq!(turn_cost(Some(0.75), Some(0.5)), Some(0.25));
        assert_eq!(turn_cost(Some(0.5), Some(0.5)), Some(0.0));
        assert_eq!(turn_cost(Some(0.25), None), Some(0.25));
        assert_eq!(
            turn_cost(Some(0.25), Some(0.5)),
            Some(0.25),
            "a baseline above the total belongs to another session"
        );
        assert_eq!(turn_cost(None, Some(0.5)), None);
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
