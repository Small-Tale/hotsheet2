//! Halt detection and per-response usage for an interactive Codex TUI from its own session
//! rollout (HS2-CYHAWB, `docs/05` §5.7, `docs/14`).
//!
//! Hot Sheet launches its Codex terminals with `--no-daemon` (HS2-4J7WCV), so each TUI hosts
//! its thread in process and nothing else can subscribe to it. The thread's host does append
//! every turn to `CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<session>.jsonl`, and that
//! file is per session by construction: its name and its `session_meta.id` are the session id
//! the TUI's `SessionStart` hook reports. This module follows that file through filesystem
//! change notifications and turns the records appended after the session was
//! reported into [`RolloutSignal`]s.
//!
//! Observed on codex-tui 0.166.0 (record shapes only; no content was copied):
//!
//! - `event_msg` `task_started {turn_id}` opens a turn.
//! - `event_msg` `task_complete {turn_id, error?: {codex_error_info, message}}` closes it. A
//!   turn that ended on an error Codex did not retry carries `error`; a retried transient error
//!   writes nothing. `turn_aborted` (a user interrupt) is not a failure.
//! - `turn_context {turn_id, model}` names the turn's model.
//! - `token_usage_record {response_id, turn_id, usage {input_tokens, output_tokens}}` counts one
//!   model response; `input_tokens` already includes its cached subset.
//!
//! Codex keeps the rollout open and appends to it. FSEvents reports such a file's change only
//! when it is closed, so on macOS the rollout itself is followed per write through a kqueue
//! vnode registration (HS2-TWE913). Windows also reports changes to a file held open only
//! after the writer closes, so its active rollout gets one local read per second (HS2-WAAKXQ).
//! Directory waits and other platforms use `notify`; no wake issues a network request.
//!
//! Only those numeric and identifying fields are parsed. Prompts, tool calls, and agent output
//! in the same file are skipped unread by the typed parser and never logged. The rollout is an
//! observed local format, not a public contract: an unrecognized or unverified record fails
//! closed (no signal).

use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// What an appended rollout record means for the terminal running the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RolloutSignal {
    /// A turn ended on an error Codex will not retry; the session waits for the user.
    Halted { error_type: String, message: String },
    /// A new turn started, so an earlier halt is over.
    Resumed,
    /// One model response's numeric usage.
    Usage(ResponseUsage),
}

/// The numeric usage of one Codex model response, keyed for deduplication by `response_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseUsage {
    pub response_id: String,
    pub model: Option<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    payload: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct EventPayload {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    error: Option<TurnError>,
}

#[derive(Deserialize)]
struct TurnError {
    #[serde(default)]
    codex_error_info: Option<serde_json::Value>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct TurnContext {
    turn_id: String,
    model: String,
}

#[derive(Deserialize)]
struct TokenRecord {
    response_id: String,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    root_turn_id: Option<String>,
    usage: TokenCounts,
}

#[derive(Deserialize)]
struct TokenCounts {
    input_tokens: u64,
    output_tokens: u64,
}

/// An incremental reader of one session's rollout. Records timestamped before `live_after`
/// (the moment the session reported, so earlier turns of a resumed session) only teach it
/// the turns' models; they never halt, resume, or count usage.
pub struct RolloutReader {
    session: String,
    live_after: OffsetDateTime,
    verified: bool,
    models: HashMap<String, String>,
    offset: u64,
    partial: Vec<u8>,
}

impl RolloutReader {
    pub fn new(session: &str, live_after: OffsetDateTime) -> Self {
        Self {
            session: session.to_owned(),
            live_after,
            verified: false,
            models: HashMap::new(),
            offset: 0,
            partial: Vec::new(),
        }
    }

    /// Classify one complete JSONL record.
    pub fn feed_line(&mut self, line: &str) -> Option<RolloutSignal> {
        let line: Line = serde_json::from_str(line).ok()?;
        let payload = line.payload?;
        if line.kind == "session_meta" {
            // A rollout names its own session first; anything else is not this session's file.
            self.verified =
                payload.get("id").and_then(serde_json::Value::as_str) == Some(&self.session);
            return None;
        }
        if !self.verified {
            return None;
        }
        if line.kind == "turn_context" {
            let context: TurnContext = serde_json::from_value(payload).ok()?;
            if valid_token(&context.model) && context.turn_id.len() <= 128 {
                self.models.insert(context.turn_id, context.model);
            }
            return None;
        }
        let live = line
            .timestamp
            .as_deref()
            .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
            .is_some_and(|at| at >= self.live_after);
        if !live {
            return None;
        }
        match line.kind.as_str() {
            "event_msg" => {
                let event: EventPayload = serde_json::from_value(payload).ok()?;
                match event.kind.as_str() {
                    "task_started" => Some(RolloutSignal::Resumed),
                    "task_complete" => event.error.map(halted),
                    _ => None,
                }
            }
            "token_usage_record" => {
                let record: TokenRecord = serde_json::from_value(payload).ok()?;
                if !valid_token(&record.response_id)
                    || (record.usage.input_tokens == 0 && record.usage.output_tokens == 0)
                {
                    return None;
                }
                let model = [record.turn_id.as_ref(), record.root_turn_id.as_ref()]
                    .into_iter()
                    .flatten()
                    .find_map(|turn| self.models.get(turn).cloned());
                Some(RolloutSignal::Usage(ResponseUsage {
                    response_id: record.response_id,
                    model,
                    tokens_in: record.usage.input_tokens,
                    tokens_out: record.usage.output_tokens,
                }))
            }
            _ => None,
        }
    }

    /// Read what was appended to `path` since the last call. A trailing record without its
    /// newline waits for the next call; a file that shrank (replaced) is read again from the
    /// start.
    pub fn read_appended(&mut self, path: &Path) -> io::Result<Vec<RolloutSignal>> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
            self.verified = false;
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        file.take(len - self.offset).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        self.partial.extend_from_slice(&bytes);
        let mut signals = Vec::new();
        let complete = match self.partial.iter().rposition(|byte| *byte == b'\n') {
            Some(end) => self.partial.drain(..=end).collect::<Vec<_>>(),
            None => return Ok(signals),
        };
        for line in complete.split(|byte| *byte == b'\n') {
            if let Ok(line) = std::str::from_utf8(line)
                && !line.trim().is_empty()
                && let Some(signal) = self.feed_line(line)
            {
                signals.push(signal);
            }
        }
        Ok(signals)
    }
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._/".contains(&byte))
}

/// A [`RolloutSignal::Halted`] from a rollout `task_complete.error`.
fn halted(error: TurnError) -> RolloutSignal {
    let error_type = error
        .codex_error_info
        .as_ref()
        .and_then(|info| match info {
            // Unit variants serialize as a string; data-carrying ones as `{variant: {...}}`.
            serde_json::Value::String(kind) => Some(kind.clone()),
            serde_json::Value::Object(map) => map.keys().next().cloned(),
            _ => None,
        })
        .filter(|kind| !kind.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    let message = error
        .message
        .as_deref()
        .map(readable_error_message)
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "The Codex turn failed. You can try again.".to_owned());
    RolloutSignal::Halted {
        error_type,
        message,
    }
}

/// Codex passes a provider's error body through as the message; show its inner `message`.
fn readable_error_message(message: &str) -> String {
    let trimmed = message.trim();
    serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .and_then(|body| {
            body.pointer("/error/message")
                .or_else(|| body.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| trimmed.to_owned())
}

/// A live follow of one session's rollout. Dropping it stops the watcher thread.
pub struct RolloutWatch {
    wake: Sender<Wake>,
    stopped: Arc<AtomicBool>,
}

enum Wake {
    Changed,
    Stop,
}

impl Drop for RolloutWatch {
    fn drop(&mut self) {
        // The flag stops a read already in progress from reporting after the drop.
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.wake.send(Wake::Stop);
    }
}

/// How long after a new subscription the watcher reads once more, in case its notification
/// stream started after a write it should have reported. One read per subscription change,
/// never a repeating timer.
const STREAM_SETTLE: Duration = Duration::from_secs(3);

/// Windows can delay a directory change event until the writer closes. Bound the fallback to
/// one local read of the active rollout per second; the watch thread and timer stop together.
#[cfg(windows)]
const WINDOWS_APPEND_CHECK: Duration = Duration::from_secs(1);

/// What the watcher currently subscribes to while it waits for, then follows, the rollout.
#[derive(PartialEq)]
enum Target {
    Home,
    Sessions,
    Rollout(PathBuf),
}

/// Follow `session`'s rollout under `codex_home` and call `on_signals` from a background
/// thread with each batch of signals, in file order. The file may not exist yet: Codex
/// creates it with the session's first turn, so the watcher waits on `sessions/` (or on the
/// home until `sessions/` appears) and then watches the file itself. Wakes come from
/// filesystem notifications, plus one settle read per subscription change. Windows checks
/// only the active rollout locally while Codex may still hold it open.
pub fn watch_rollout(
    codex_home: &Path,
    session: &str,
    live_after: OffsetDateTime,
    mut on_signals: impl FnMut(Vec<RolloutSignal>) + Send + 'static,
) -> io::Result<RolloutWatch> {
    use notify::{RecursiveMode, Watcher};
    if !codex_home.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no Codex home at {}", codex_home.display()),
        ));
    }
    let (wake, woken) = channel();
    let notify_wake = wake.clone();
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        // Our own reads must not wake us again.
        if event.is_ok_and(|event| !matches!(event.kind, notify::EventKind::Access(_))) {
            let _ = notify_wake.send(Wake::Changed);
        }
    })
    .map_err(io::Error::other)?;
    // FSEvents reports resolved paths (`/private/var/...`); a symlinked home would otherwise
    // have its notifications filtered out as foreign.
    let home = std::fs::canonicalize(codex_home)?;
    let sessions = home.join("sessions");
    let mut reader = RolloutReader::new(session, live_after);
    let session = session.to_owned();
    // Subscribe to the narrowest existing target, so a change that happens between this
    // subscription and the first read is still delivered.
    let subscribe = move |watcher: &mut notify::RecommendedWatcher,
                          current: &mut Option<Target>| {
        let (next, path, mode) =
            match hotsheet_ticketing::metrics::codex_work::find_rollout(&home, &session)? {
                Some(path) => (
                    Target::Rollout(path.clone()),
                    path,
                    RecursiveMode::NonRecursive,
                ),
                None if sessions.is_dir() => {
                    (Target::Sessions, sessions.clone(), RecursiveMode::Recursive)
                }
                None => (Target::Home, home.clone(), RecursiveMode::NonRecursive),
            };
        if current.as_ref() != Some(&next) {
            if let Some(previous) = current.take() {
                let _ = watcher.unwatch(match &previous {
                    Target::Home => &home,
                    Target::Sessions => &sessions,
                    Target::Rollout(path) => path,
                });
            }
            watcher.watch(&path, mode).map_err(io::Error::other)?;
            *current = Some(next);
            return io::Result::Ok(true);
        }
        io::Result::Ok(false)
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_stopped = stopped.clone();
    // FSEvents reports a change to a file Codex keeps open only when it closes, so on macOS the
    // rollout itself is followed per write through kqueue (HS2-TWE913).
    #[cfg(target_os = "macos")]
    let append_wake = wake.clone();
    // Starting a native stream (FSEventStreamStart) can take seconds under load, so even the
    // first subscription happens on the watcher's own thread, off the hook's request path.
    std::thread::spawn(move || {
        let mut watcher = watcher;
        let mut current = None;
        let mut settle_at: Option<Instant> = None;
        #[cfg(target_os = "macos")]
        let mut _appends: Option<crate::vnode_watch::VnodeWatch> = None;
        loop {
            // A home that vanished or cannot be watched ends the watch; the terminal keeps
            // its last known state. A new subscription looks again at once, and once more
            // when its stream has had time to start, because a change made before the
            // stream was live is never reported to it.
            if !matches!(current, Some(Target::Rollout(_))) {
                match subscribe(&mut watcher, &mut current) {
                    Err(_) => return,
                    Ok(true) => {
                        #[cfg(target_os = "macos")]
                        if let Some(Target::Rollout(path)) = &current {
                            let wake = append_wake.clone();
                            // Without it the FSEvents subscription still reports closes.
                            _appends = crate::vnode_watch::VnodeWatch::start(path, move || {
                                let _ = wake.send(Wake::Changed);
                            })
                            .ok();
                        }
                        settle_at = Some(Instant::now() + STREAM_SETTLE);
                        continue;
                    }
                    Ok(false) => {}
                }
            }
            if let Some(Target::Rollout(path)) = &current {
                match reader.read_appended(path) {
                    Ok(_) if thread_stopped.load(Ordering::SeqCst) => return,
                    Ok(signals) if !signals.is_empty() => on_signals(signals),
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
            let next = match settle_at {
                Some(deadline) => {
                    match woken.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Err(RecvTimeoutError::Timeout) => {
                            settle_at = None;
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                        Ok(next) => Ok(next),
                    }
                }
                None => {
                    #[cfg(windows)]
                    {
                        if matches!(current, Some(Target::Rollout(_))) {
                            match woken.recv_timeout(WINDOWS_APPEND_CHECK) {
                                Err(RecvTimeoutError::Timeout) => continue,
                                Err(RecvTimeoutError::Disconnected) => return,
                                Ok(next) => Ok(next),
                            }
                        } else {
                            woken.recv()
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        woken.recv()
                    }
                }
            };
            match next {
                Ok(Wake::Changed) => {
                    // One read covers a burst of notifications.
                    while let Ok(next) = woken.try_recv() {
                        if matches!(next, Wake::Stop) {
                            return;
                        }
                    }
                }
                Ok(Wake::Stop) | Err(_) => return,
            }
        }
    });
    Ok(RolloutWatch { wake, stopped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::sync::mpsc::Receiver;

    const SESSION: &str = "00000000-0000-7000-8000-0000000000c1";

    fn at(value: &str) -> OffsetDateTime {
        OffsetDateTime::parse(value, &Rfc3339).unwrap()
    }

    fn meta(session: &str) -> String {
        json!({"timestamp":"2026-11-05T00:00:00.000Z","type":"session_meta","payload":{"id":session,"session_id":session,"originator":"codex-tui","source":"cli"}}).to_string()
    }

    fn started(ts: &str, turn: &str) -> String {
        json!({"timestamp":ts,"type":"event_msg","payload":{"type":"task_started","turn_id":turn,"root_turn_id":turn}}).to_string()
    }

    fn context(ts: &str, turn: &str, model: &str) -> String {
        json!({"timestamp":ts,"type":"turn_context","payload":{"turn_id":turn,"root_turn_id":turn,"model":model,"effort":"high"}}).to_string()
    }

    fn usage(ts: &str, turn: &str, response: &str, input: u64, output: u64) -> String {
        json!({"timestamp":ts,"type":"token_usage_record","payload":{"response_id":response,"turn_id":turn,"root_turn_id":turn,"session_id":SESSION,"thread_id":SESSION,"usage":{"input_tokens":input,"cached_input_tokens":0,"output_tokens":output,"reasoning_output_tokens":0,"total_tokens":input+output}}}).to_string()
    }

    fn complete(ts: &str, turn: &str, error: Option<serde_json::Value>) -> String {
        let mut payload =
            json!({"type":"task_complete","turn_id":turn,"last_agent_message":"synthetic"});
        if let Some(error) = error {
            payload["error"] = error;
        }
        json!({"timestamp":ts,"type":"event_msg","payload":payload}).to_string()
    }

    #[test]
    fn hs2_cyhawb_live_turns_resume_count_usage_and_halt_on_a_failed_completion() {
        let mut reader = RolloutReader::new(SESSION, at("2026-11-05T01:00:00Z"));
        let lines = [
            meta(SESSION),
            context("2026-11-05T01:00:01.000Z", "t1", "gpt-synthetic"),
            started("2026-11-05T01:00:01.100Z", "t1"),
            json!({"timestamp":"2026-11-05T01:00:02.000Z","type":"response_item","payload":{"type":"message","content":"private prompt"}}).to_string(),
            usage("2026-11-05T01:00:03.000Z", "t1", "resp_1", 1200, 40),
            usage("2026-11-05T01:00:03.500Z", "t1", "resp_0", 0, 0),
            complete("2026-11-05T01:00:04.000Z", "t1", None),
            started("2026-11-05T01:01:00.000Z", "t2"),
            complete(
                "2026-11-05T01:01:05.000Z",
                "t2",
                Some(json!({"codex_error_info":"server_overloaded","message":"{\"error\":{\"message\":\"Synthetic overload.\"}}"})),
            ),
        ];
        let signals: Vec<_> = lines
            .iter()
            .filter_map(|line| reader.feed_line(line))
            .collect();
        assert_eq!(
            signals,
            [
                RolloutSignal::Resumed,
                RolloutSignal::Usage(ResponseUsage {
                    response_id: "resp_1".into(),
                    model: Some("gpt-synthetic".into()),
                    tokens_in: 1200,
                    tokens_out: 40,
                }),
                RolloutSignal::Resumed,
                RolloutSignal::Halted {
                    error_type: "server_overloaded".into(),
                    message: "Synthetic overload.".into(),
                },
            ]
        );
    }

    #[test]
    fn hs2_cyhawb_records_before_the_report_and_unverified_files_signal_nothing() {
        // A resumed session's earlier turns predate the report: they teach the model only.
        let mut reader = RolloutReader::new(SESSION, at("2026-11-05T02:00:00Z"));
        assert_eq!(reader.feed_line(&meta(SESSION)), None);
        assert_eq!(
            reader.feed_line(&context("2026-11-05T01:00:00Z", "old", "gpt-old")),
            None
        );
        assert_eq!(
            reader.feed_line(&started("2026-11-05T01:00:01Z", "old")),
            None
        );
        assert_eq!(
            reader.feed_line(&complete(
                "2026-11-05T01:00:02Z",
                "old",
                Some(json!({"codex_error_info":"other","message":"stale"}))
            )),
            None
        );
        assert_eq!(
            reader.feed_line(&usage("2026-11-05T01:00:02Z", "old", "resp_old", 5, 5)),
            None
        );
        // A live response of the old turn still knows its model.
        assert!(matches!(
            reader.feed_line(&usage("2026-11-05T02:00:01Z", "old", "resp_new", 5, 5)),
            Some(RolloutSignal::Usage(ResponseUsage { model: Some(ref model), .. })) if model == "gpt-old"
        ));
        // Missing or malformed timestamps fail closed.
        let undated = json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"x"}});
        assert_eq!(reader.feed_line(&undated.to_string()), None);

        let mut other = RolloutReader::new(SESSION, at("2026-11-05T00:00:00Z"));
        assert_eq!(
            other.feed_line(&meta("00000000-0000-7000-8000-0000000000ff")),
            None
        );
        assert_eq!(other.feed_line(&started("2026-11-05T01:00:00Z", "t")), None);
        assert_eq!(
            other.feed_line(&usage("2026-11-05T01:00:00Z", "t", "r", 1, 1)),
            None
        );
        let mut headless = RolloutReader::new(SESSION, at("2026-11-05T00:00:00Z"));
        assert_eq!(
            headless.feed_line(&started("2026-11-05T01:00:00Z", "t")),
            None
        );
    }

    #[test]
    fn hs2_cyhawb_interrupts_retries_and_untyped_errors_are_classified_safely() {
        let mut reader = RolloutReader::new(SESSION, at("2026-11-05T00:00:00Z"));
        reader.feed_line(&meta(SESSION));
        let aborted = json!({"timestamp":"2026-11-05T01:00:00Z","type":"event_msg","payload":{"type":"turn_aborted","reason":"interrupted","turn_id":"t"}});
        assert_eq!(reader.feed_line(&aborted.to_string()), None);
        let null_error = json!({"timestamp":"2026-11-05T01:00:00Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"t","error":null}});
        assert_eq!(reader.feed_line(&null_error.to_string()), None);
        assert_eq!(
            reader.feed_line(&complete(
                "2026-11-05T01:00:01Z",
                "t",
                Some(json!({"codex_error_info":{"usage_limit_exceeded":{"resets_at":1}},"message":"Limit."}))
            )),
            Some(RolloutSignal::Halted {
                error_type: "usage_limit_exceeded".into(),
                message: "Limit.".into()
            })
        );
        assert_eq!(
            reader.feed_line(&complete("2026-11-05T01:00:02Z", "t", Some(json!({})))),
            Some(RolloutSignal::Halted {
                error_type: "unknown".into(),
                message: "The Codex turn failed. You can try again.".into()
            })
        );
        assert_eq!(reader.feed_line("not json"), None);
        let bad_response = usage("2026-11-05T01:00:03Z", "t", "bad id with spaces", 1, 1);
        assert_eq!(reader.feed_line(&bad_response), None);
    }

    #[test]
    fn hs2_cyhawb_appended_reads_wait_for_a_whole_line_and_restart_on_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let mut reader = RolloutReader::new(SESSION, at("2026-11-05T00:00:00Z"));
        let mut file = File::create(&path).unwrap();
        writeln!(file, "{}", meta(SESSION)).unwrap();
        let line = started("2026-11-05T01:00:00Z", "t1");
        let (head, tail) = line.split_at(20);
        write!(file, "{head}").unwrap();
        file.flush().unwrap();
        assert!(reader.read_appended(&path).unwrap().is_empty());
        writeln!(file, "{tail}").unwrap();
        file.flush().unwrap();
        assert_eq!(
            reader.read_appended(&path).unwrap(),
            [RolloutSignal::Resumed]
        );
        assert!(reader.read_appended(&path).unwrap().is_empty());
        // A shorter replacement file is read from its start (and must verify again).
        std::fs::write(
            &path,
            format!("{}\n", started("2026-11-05T01:00:01Z", "t2")),
        )
        .unwrap();
        assert!(reader.read_appended(&path).unwrap().is_empty());
    }

    fn next(signals: &Receiver<Vec<RolloutSignal>>) -> Vec<RolloutSignal> {
        // HS2-AY33TG: the Home -> Sessions -> Rollout path can cross three
        // three-second settle windows plus delayed macOS stream startup under
        // full-suite load. The channel still waits for the event, not a fixed delay.
        signals
            .recv_timeout(Duration::from_secs(30))
            .expect("the rollout watch reports appended records")
    }

    #[test]
    fn hs2_cyhawb_the_watch_waits_for_the_rollout_then_follows_appends_until_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex-home");
        std::fs::create_dir_all(&home).unwrap();
        let (tx, signals) = channel();
        let watch = watch_rollout(&home, SESSION, at("2026-11-05T00:00:00Z"), move |batch| {
            let _ = tx.send(batch);
        })
        .unwrap();
        // Codex creates `sessions/` and the dated rollout only with the first turn.
        let day = home.join("sessions/2026/11/05");
        std::fs::create_dir_all(&day).unwrap();
        let rollout = day.join(format!("rollout-2026-11-05T00-00-00-{SESSION}.jsonl"));
        let mut file = File::create(&rollout).unwrap();
        writeln!(file, "{}", meta(SESSION)).unwrap();
        writeln!(file, "{}", started("2026-11-05T01:00:00Z", "t1")).unwrap();
        file.flush().unwrap();
        assert_eq!(next(&signals), [RolloutSignal::Resumed]);
        writeln!(
            file,
            "{}",
            complete(
                "2026-11-05T01:00:05Z",
                "t1",
                Some(json!({"codex_error_info":"other","message":"Synthetic failure."}))
            )
        )
        .unwrap();
        file.flush().unwrap();
        assert_eq!(
            next(&signals),
            [RolloutSignal::Halted {
                error_type: "other".into(),
                message: "Synthetic failure.".into()
            }]
        );
        drop(watch);
        writeln!(file, "{}", started("2026-11-05T01:01:00Z", "t2")).unwrap();
        file.flush().unwrap();
        // The dropped watch's thread ends and releases its callback without another report.
        assert_eq!(
            signals.recv_timeout(Duration::from_secs(30)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        );
    }

    /// HS2-TWE913: a rollout that already exists when the watch starts (a resumed session, or
    /// a watch resumed after a server restart) keeps reporting appends long after the one
    /// settle read, from filesystem notifications alone.
    #[test]
    fn hs2_twe913_an_existing_rollout_reports_appends_after_the_settle_read() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex-home");
        let day = home.join("sessions/2026/11/05");
        std::fs::create_dir_all(&day).unwrap();
        let rollout = day.join(format!("rollout-2026-11-05T00-00-00-{SESSION}.jsonl"));
        let mut file = File::create(&rollout).unwrap();
        writeln!(file, "{}", meta(SESSION)).unwrap();
        writeln!(file, "{}", started("2026-11-05T01:00:00Z", "t1")).unwrap();
        file.flush().unwrap();
        let (tx, signals) = channel();
        let watch = watch_rollout(&home, SESSION, at("2026-11-05T00:00:00Z"), move |batch| {
            let _ = tx.send(batch);
        })
        .unwrap();
        assert_eq!(next(&signals), [RolloutSignal::Resumed]);
        std::thread::sleep(STREAM_SETTLE + Duration::from_secs(1));
        for turn in ["t2", "t3"] {
            writeln!(file, "{}", started("2026-11-05T01:01:00Z", turn)).unwrap();
            file.flush().unwrap();
            assert_eq!(next(&signals), [RolloutSignal::Resumed], "turn {turn}");
        }
        // HS2-WAAKXQ: closing and reopening the writer must keep the same rollout live.
        drop(file);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&rollout)
            .unwrap();
        writeln!(file, "{}", started("2026-11-05T01:01:10Z", "t4")).unwrap();
        file.flush().unwrap();
        assert_eq!(next(&signals), [RolloutSignal::Resumed]);

        // A restarted watcher reads the old records for context but reports only new turns.
        drop(watch);
        let (tx, resumed_signals) = channel();
        let _resumed = watch_rollout(&home, SESSION, at("2026-11-05T01:01:30Z"), move |batch| {
            let _ = tx.send(batch);
        })
        .unwrap();
        assert_eq!(
            resumed_signals.recv_timeout(Duration::from_millis(200)),
            Err(RecvTimeoutError::Timeout)
        );
        writeln!(file, "{}", started("2026-11-05T01:02:00Z", "t5")).unwrap();
        file.flush().unwrap();
        assert_eq!(next(&resumed_signals), [RolloutSignal::Resumed]);
    }

    #[test]
    fn hs2_cyhawb_a_missing_home_is_reported_without_creating_it() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("absent");
        assert!(watch_rollout(&home, SESSION, OffsetDateTime::now_utc(), |_| {}).is_err());
        assert!(!home.exists());
    }
}
