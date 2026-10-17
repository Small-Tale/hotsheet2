//! Best-effort halt detection for an interactive Codex TUI (HS2-MN865E, `docs/05` §5.7).
//!
//! Codex hooks and `notify` carry no failure outcome: a turn that ends on an API error fires
//! `UserPromptSubmit` but no `Stop`. The interactive TUI runs on the shared app-server daemon
//! by default, though, and v2 `thread/resume` on a running thread **rejoins** it, so a second
//! connection receives that thread's notifications. This module rejoins the thread the TUI's
//! `SessionStart` hook reported and turns its notifications into [`ThreadSignal`]s. It never
//! sends `turn/start` and never answers a server request, so it cannot drive or approve
//! anything in the user's session.
//!
//! Observed on codex 0.162.1 with the daemon at 0.163.0 (cassette
//! `tests/fixtures/codex-0.162.1-thread-rejoin-failure.json`): a failed turn emits
//! `thread/status/changed systemError`, `error {willRetry: false}`, and `turn/completed
//! {turn.status: "failed", turn.error}`. A retried transient error carries `willRetry: true`.
//!
//! Limits: the protocol is experimental and its schema is version-specific; a TUI started
//! with `--no-daemon` or `daemon_auto_start = false` is not on the daemon and stays invisible;
//! and while this watcher is subscribed the daemon keeps the thread loaded (it otherwise
//! unloads a thread 30 minutes after its last subscriber).

use crate::codex::{UdsWsTransport, codex_control_socket_path};
use crate::ports::{RpcTransport, RpcWriter};
use serde_json::{Value, json};
use std::path::Path;

/// What a rejoined thread's notification means for the terminal running it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadSignal {
    /// The turn ended on an error Codex will not retry; the session waits for the user.
    Halted { error_type: String, message: String },
    /// A new turn started, so an earlier halt is over.
    Resumed,
}

/// Classify one JSON-RPC message from the daemon for `thread_id`. Responses, server requests,
/// other threads, and unrelated notifications are `None`.
pub fn classify_notification(thread_id: &str, message: &Value) -> Option<ThreadSignal> {
    if message.get("id").is_some() {
        // A response to our own request, or a server request (an approval) that the TUI answers.
        return None;
    }
    let params = message.get("params")?;
    if params.get("threadId").and_then(Value::as_str) != Some(thread_id) {
        return None;
    }
    match message.get("method").and_then(Value::as_str)? {
        "turn/started" => Some(ThreadSignal::Resumed),
        "turn/completed" => {
            let turn = params.get("turn")?;
            (turn.get("status").and_then(Value::as_str) == Some("failed"))
                .then(|| halted(turn.get("error")))
        }
        "error" => (params.get("willRetry").and_then(Value::as_bool) == Some(false))
            .then(|| halted(params.get("error"))),
        _ => None,
    }
}

/// A [`ThreadSignal::Halted`] from a v2 `TurnError` (`{message, codexErrorInfo, …}`).
fn halted(error: Option<&Value>) -> ThreadSignal {
    let error_type = error
        .and_then(|error| error.get("codexErrorInfo"))
        .and_then(|info| match info {
            // Unit variants serialize as a string; data-carrying ones as `{variant: {...}}`.
            Value::String(kind) => Some(kind.clone()),
            Value::Object(map) => map.keys().next().cloned(),
            _ => None,
        })
        .filter(|kind| !kind.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(readable_error_message)
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "The Codex turn failed. You can try again.".to_owned());
    ThreadSignal::Halted {
        error_type,
        message,
    }
}

/// Codex passes a provider's error body through as the message; show its inner `message`.
fn readable_error_message(message: &str) -> String {
    let trimmed = message.trim();
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|body| {
            body.pointer("/error/message")
                .or_else(|| body.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| trimmed.to_owned())
}

/// A live rejoin of one thread. Dropping it closes the connection and ends the watcher.
pub struct ThreadWatch {
    writer: Box<dyn RpcWriter>,
}

impl Drop for ThreadWatch {
    fn drop(&mut self) {
        self.writer.close();
    }
}

/// Rejoin `thread_id` on the shared daemon for `codex_home` and call `on_signal` from a
/// background thread for each [`ThreadSignal`]. `on_end` runs once when the connection closes
/// (the daemon went away, the thread could not be resumed, or the handle was dropped). The
/// daemon is never started here: a missing socket means the TUI is not on a daemon.
pub fn watch_thread(
    codex_home: &Path,
    thread_id: &str,
    on_signal: impl FnMut(ThreadSignal) + Send + 'static,
    on_end: impl FnOnce() + Send + 'static,
) -> std::io::Result<ThreadWatch> {
    let socket = codex_control_socket_path(codex_home);
    if !socket.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no Codex daemon socket at {}", socket.display()),
        ));
    }
    watch_thread_over(
        UdsWsTransport::connect(&socket)?,
        thread_id,
        on_signal,
        on_end,
    )
}

/// [`watch_thread`] over any transport (tests use an in-memory one).
pub fn watch_thread_over(
    transport: Box<dyn RpcTransport>,
    thread_id: &str,
    mut on_signal: impl FnMut(ThreadSignal) + Send + 'static,
    on_end: impl FnOnce() + Send + 'static,
) -> std::io::Result<ThreadWatch> {
    let (mut writer, mut reader) = transport.split();
    let send = |writer: &mut Box<dyn RpcWriter>, message: Value| writer.send(&message.to_string());
    send(
        &mut writer,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "hotsheet",
                    "title": "Hot Sheet",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": null,
            },
        }),
    )?;
    send(
        &mut writer,
        json!({ "jsonrpc": "2.0", "method": "initialized" }),
    )?;
    send(
        &mut writer,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "thread/resume",
            "params": { "threadId": thread_id },
        }),
    )?;
    let thread_id = thread_id.to_owned();
    std::thread::spawn(move || {
        while let Ok(Some(line)) = reader.recv() {
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            // A failed resume (an unknown or unloaded thread) leaves nothing to watch.
            if message.get("id") == Some(&json!(2)) && message.get("error").is_some() {
                break;
            }
            if let Some(signal) = classify_notification(&thread_id, &message) {
                on_signal(signal);
            }
        }
        on_end();
    });
    Ok(ThreadWatch { writer })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::RpcReader;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    const THREAD: &str = "00000000-0000-7000-8000-000000000001";

    fn note(method: &str, params: Value) -> Value {
        json!({ "method": method, "params": params })
    }

    #[test]
    fn a_failed_turn_or_final_error_halts_and_a_new_turn_resumes() {
        let failed = note(
            "turn/completed",
            json!({ "threadId": THREAD, "turn": { "status": "failed", "error": {
                "message": "{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The model is not supported.\"}}",
                "codexErrorInfo": "other",
            }}}),
        );
        assert_eq!(
            classify_notification(THREAD, &failed),
            Some(ThreadSignal::Halted {
                error_type: "other".into(),
                message: "The model is not supported.".into()
            })
        );
        let final_error = note(
            "error",
            json!({ "threadId": THREAD, "willRetry": false, "error": {
                "message": "You've hit your usage limit.",
                "codexErrorInfo": { "usageLimitExceeded": { "resetsAt": 1 } },
            }}),
        );
        assert_eq!(
            classify_notification(THREAD, &final_error),
            Some(ThreadSignal::Halted {
                error_type: "usageLimitExceeded".into(),
                message: "You've hit your usage limit.".into()
            })
        );
        assert_eq!(
            classify_notification(THREAD, &note("turn/started", json!({ "threadId": THREAD }))),
            Some(ThreadSignal::Resumed)
        );
    }

    #[test]
    fn retries_successes_other_threads_responses_and_requests_are_ignored() {
        let ignored = [
            note(
                "error",
                json!({ "threadId": THREAD, "willRetry": true, "error": { "message": "Reconnecting" } }),
            ),
            note(
                "turn/completed",
                json!({ "threadId": THREAD, "turn": { "status": "completed" } }),
            ),
            note(
                "turn/completed",
                json!({ "threadId": "other", "turn": { "status": "failed" } }),
            ),
            note(
                "thread/status/changed",
                json!({ "threadId": THREAD, "status": { "type": "systemError" } }),
            ),
            json!({ "id": 2, "result": { "thread": { "id": THREAD } } }),
            json!({ "id": 9, "method": "item/commandExecution/requestApproval", "params": { "threadId": THREAD } }),
            json!({ "method": "turn/started" }),
        ];
        for message in ignored {
            assert_eq!(classify_notification(THREAD, &message), None, "{message}");
        }
    }

    #[test]
    fn a_failure_without_details_still_halts_with_a_readable_fallback() {
        let bare = note(
            "turn/completed",
            json!({ "threadId": THREAD, "turn": { "status": "failed", "error": null } }),
        );
        assert_eq!(
            classify_notification(THREAD, &bare),
            Some(ThreadSignal::Halted {
                error_type: "unknown".into(),
                message: "The Codex turn failed. You can try again.".into()
            })
        );
        let plain = note(
            "error",
            json!({ "threadId": THREAD, "willRetry": false, "error": { "message": " stream closed ", "codexErrorInfo": 7 } }),
        );
        assert_eq!(
            classify_notification(THREAD, &plain),
            Some(ThreadSignal::Halted {
                error_type: "unknown".into(),
                message: "stream closed".into()
            })
        );
    }

    /// An in-memory daemon: records what the watcher sends and replays scripted lines.
    struct Loopback {
        sent: Arc<Mutex<Vec<Value>>>,
        closed: Arc<Mutex<bool>>,
        incoming: Receiver<String>,
    }
    struct LoopWriter {
        sent: Arc<Mutex<Vec<Value>>>,
        closed: Arc<Mutex<bool>>,
        hangup: Option<Sender<String>>,
    }
    impl RpcWriter for LoopWriter {
        fn send(&mut self, msg: &str) -> std::io::Result<()> {
            self.sent
                .lock()
                .unwrap()
                .push(serde_json::from_str(msg).unwrap());
            Ok(())
        }
        fn close(&mut self) {
            *self.closed.lock().unwrap() = true;
            // Closing our half ends the connection, as the WebSocket task does.
            self.hangup.take();
        }
    }
    struct LoopReader(Receiver<String>);
    impl RpcReader for LoopReader {
        fn recv(&mut self) -> std::io::Result<Option<String>> {
            Ok(self.0.recv().ok())
        }
    }
    impl RpcTransport for Loopback {
        fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
            (
                Box::new(LoopWriter {
                    sent: self.sent,
                    closed: self.closed,
                    hangup: None,
                }),
                Box::new(LoopReader(self.incoming)),
            )
        }
    }

    type Sent = Arc<Mutex<Vec<Value>>>;
    type Closed = Arc<Mutex<bool>>;

    fn loopback() -> (Box<Loopback>, Sender<String>, Sent, Closed) {
        let (tx, rx) = channel();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(Mutex::new(false));
        (
            Box::new(Loopback {
                sent: sent.clone(),
                closed: closed.clone(),
                incoming: rx,
            }),
            tx,
            sent,
            closed,
        )
    }

    #[test]
    fn the_watcher_rejoins_without_starting_a_turn_and_reports_the_recorded_failure() {
        let (transport, daemon, sent, closed) = loopback();
        let (signals_tx, signals) = channel();
        let (ended_tx, ended) = channel();
        let watch = watch_thread_over(
            transport,
            THREAD,
            move |signal| signals_tx.send(signal).unwrap(),
            move || ended_tx.send(()).unwrap(),
        )
        .unwrap();
        let methods: Vec<String> = sent
            .lock()
            .unwrap()
            .iter()
            .map(|m| m["method"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(methods, ["initialize", "initialized", "thread/resume"]);
        assert_eq!(sent.lock().unwrap()[2]["params"]["threadId"], THREAD);

        // Replay the live capture: turn/started resumes, then error + turn/completed halt.
        let cassette: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/codex-0.162.1-thread-rejoin-failure.json"
        ))
        .unwrap();
        daemon
            .send(cassette["resume_response"].to_string())
            .unwrap();
        for message in cassette["notifications"].as_array().unwrap() {
            daemon.send(message.to_string()).unwrap();
        }
        let wait = || signals.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(wait(), ThreadSignal::Resumed);
        let halted = ThreadSignal::Halted {
            error_type: "other".into(),
            message: "The 'hs2-no-such-model' model is not supported when using Codex with a ChatGPT account.".into(),
        };
        assert_eq!(wait(), halted, "error {{willRetry: false}}");
        assert_eq!(wait(), halted, "turn/completed failed");
        // Nothing beyond the three requests was ever sent: no turn/start, no approvals.
        assert_eq!(sent.lock().unwrap().len(), 3);

        drop(watch);
        assert!(*closed.lock().unwrap());
        drop(daemon);
        ended.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn a_failed_resume_ends_the_watcher() {
        let (transport, daemon, _sent, _closed) = loopback();
        let (ended_tx, ended) = channel();
        let _watch = watch_thread_over(
            transport,
            THREAD,
            |_| panic!("no signal expected"),
            move || ended_tx.send(()).unwrap(),
        )
        .unwrap();
        daemon
            .send(
                json!({ "id": 2, "error": { "code": -32600, "message": "thread not found" } })
                    .to_string(),
            )
            .unwrap();
        ended.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn a_home_without_a_daemon_socket_is_reported_without_starting_one() {
        let home = tempfile::tempdir().unwrap();
        let error = watch_thread(home.path(), THREAD, |_| {}, || {})
            .err()
            .unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(!home.path().join("app-server-control").exists());
    }
}
