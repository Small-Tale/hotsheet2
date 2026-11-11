//! Continuous usage for native Claude terminals (HS2-W2GNTT, HS2-SSMDQ1).
//!
//! The hook names the session's transcript at prompt and stop. The server follows that
//! transcript from the first prompt it sees and records each main-chain assistant message's
//! numeric usage as soon as the message is final (a later message appeared) or the turn
//! stopped, so a turn that runs for hours still moves its project's tokens per minute. Every
//! message is recorded once under `claude-message:<session>:<message id>`, so the follower,
//! a Stop flush, and a resumed follow cannot count it twice. The follow's resume offset,
//! session, transcript path, and usage checkout persist under the machine home, so a
//! restarted server resumes following terminals the broker kept alive. Only message ids,
//! timestamps, models, and counters are read; no transcript content enters server state.
//!
//! Background subagents write their own transcripts under `<session>/subagents/`
//! (HS2-JW6N67). The follow reads those too, so a session that mostly waits on its agents
//! still moves its project's tokens per minute. Each subagent message is recorded once
//! under `claude-message:<session>:agent-<id>:<message id>` in the same project store.

use super::usage_store::{UsageTarget, terminal_usage_target, usage_target_for_checkout};
use crate::{ApiError, AppState};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_aitools::claude_transcript_usage::{
    MessageUsage, TranscriptFollower, TranscriptWatch, subagent_transcripts, watch_transcript,
};
use hotsheet_model::Timestamp;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::metrics;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path as FilePath, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const TRANSCRIPT_FLUSH_WAIT: Duration = Duration::from_secs(1);
const TRANSCRIPT_FLUSH_RETRY: Duration = Duration::from_millis(50);
/// Machine-home directory of persisted follows, one `<terminal>.json` each.
const FOLLOW_DIR: &str = "claude-usage";

/// One terminal's live follow of its Claude session's transcript. Dropping it stops the
/// watcher; the persisted record stays until the session or terminal ends.
pub struct ClaudeUsageFollow {
    session_id: String,
    transcript: PathBuf,
    inner: Arc<Mutex<FollowInner>>,
    _watch: Option<TranscriptWatch>,
}

struct FollowInner {
    terminal: String,
    session_id: String,
    transcript: PathBuf,
    reader: TranscriptFollower,
    /// One follower per background subagent transcript, by agent id (HS2-JW6N67).
    subagents: BTreeMap<String, TranscriptFollower>,
    usage_checkout: Option<String>,
    /// The current turn's project store and unique claim snapshot (HS2-B4EQ87); `None`
    /// until a prompt sets it or a resumed follow first records.
    target: Option<UsageTarget>,
    /// The resume offset when the current turn's prompt arrived: a repeated prompt before
    /// the transcript grows keeps the original snapshot.
    turn_start: Option<u64>,
    /// Messages recorded since the current prompt, for Stop's wait for a late append.
    turn_messages: usize,
    /// A Stop is waiting for a late append; a concurrent Stop returns at once.
    stop_waiting: bool,
}

/// The persisted part of a follow: no transcript content and no credential.
#[derive(Serialize, Deserialize)]
struct FollowRecord {
    terminal: String,
    session_id: String,
    transcript: PathBuf,
    offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage_checkout: Option<String>,
    /// Resume offsets of the followed subagent transcripts (HS2-JW6N67). `None` in a record
    /// written before subagents were followed: their existing transcripts start at the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subagents: Option<BTreeMap<String, u64>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum TurnEvent {
    Start,
    Stop,
    Failure,
}

#[derive(Deserialize)]
pub(crate) struct ClaudeTurnReq {
    event: TurnEvent,
    session_id: String,
    transcript_path: String,
}

fn transcript_for_session(path: &str, session_id: &str) -> Result<PathBuf, ApiError> {
    if session_id.is_empty()
        || session_id.len() > 128
        || session_id.contains(['/', '\\'])
        || path.len() > 4096
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid Claude session path",
        ));
    }
    let path = FilePath::new(path);
    let filename = format!("{session_id}.jsonl");
    let parent = path.parent();
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || path.file_name().and_then(|name| name.to_str()) != Some(filename.as_str())
        || parent
            .and_then(FilePath::parent)
            .and_then(FilePath::file_name)
            .and_then(|name| name.to_str())
            != Some("projects")
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid Claude transcript path",
        ));
    }
    // A fresh Claude session names its transcript before creating it. Once it exists,
    // still reject a symlink that resolves outside a projects/session path.
    if path.exists() {
        let resolved = path
            .canonicalize()
            .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "Claude transcript unavailable"))?;
        if !resolved.is_file()
            || resolved.file_name().and_then(|name| name.to_str()) != Some(filename.as_str())
            || resolved
                .parent()
                .and_then(FilePath::parent)
                .and_then(FilePath::file_name)
                .and_then(|name| name.to_str())
                != Some("projects")
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid Claude transcript path",
            ));
        }
    }
    Ok(path.to_path_buf())
}

/// The `code` of the 409 for a turn report from a terminal with no reported AI session, as
/// after a server restart: the hook re-sends its `SessionStart` report and retries
/// (HS2-SSMDQ1). A different session keeps the plain conflict and is never adopted.
pub(crate) const CLAUDE_SESSION_NOT_CONNECTED_CODE: &str = "claude_session_not_connected";

/// Whether `session_id` is terminal `terminal`'s reported Claude session. With no report at
/// all the conflict says so, so the hook can reconnect instead of losing every turn until
/// its session restarts (HS2-SSMDQ1).
fn check_claude_session(
    state: &AppState,
    terminal: &str,
    session_id: &str,
) -> Result<(), ApiError> {
    match state
        .terminal_ai_connections
        .lock_or_recover()
        .get(terminal)
    {
        None => Err(ApiError::new(
            StatusCode::CONFLICT,
            "Claude terminal session not connected",
        )
        .with_code(CLAUDE_SESSION_NOT_CONNECTED_CODE)),
        Some(connection)
            if connection.agent.as_deref() == Some("claude")
                && connection.session_id.as_deref() == Some(session_id) =>
        {
            Ok(())
        }
        Some(_) => Err(ApiError::new(
            StatusCode::CONFLICT,
            "Claude terminal session changed",
        )),
    }
}

fn internal(error: std::io::Error) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

/// Where terminal `terminal`'s follow persists, or `None` for an id unsafe as a file name.
fn record_path(state: &AppState, terminal: &str) -> Option<PathBuf> {
    (!terminal.is_empty()
        && terminal.len() <= 128
        && terminal
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'))
    .then(|| {
        state
            .machine_home()
            .join(FOLLOW_DIR)
            .join(format!("{terminal}.json"))
    })
}

/// Best effort: a follow that cannot persist still records while this server runs.
fn persist(state: &AppState, inner: &FollowInner) {
    let Some(path) = record_path(state, &inner.terminal) else {
        return;
    };
    let record = FollowRecord {
        terminal: inner.terminal.clone(),
        session_id: inner.session_id.clone(),
        transcript: inner.transcript.clone(),
        offset: inner.reader.resume_offset(),
        usage_checkout: inner.usage_checkout.clone(),
        subagents: Some(
            inner
                .subagents
                .iter()
                .map(|(agent, reader)| (agent.clone(), reader.resume_offset()))
                .collect(),
        ),
    };
    let write = || -> std::io::Result<()> {
        let dir = path.parent().expect("record paths have a directory");
        std::fs::create_dir_all(dir)?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec(&record)?)?;
        std::fs::rename(temp, &path)
    };
    if let Err(error) = write() {
        tracing::warn!(%error, "persisting a Claude usage follow failed");
    }
}

/// The project store and unique claim of the terminal's Claude or shell worker now.
fn resolve_target(state: &AppState, terminal: &str, usage_checkout: Option<&str>) -> UsageTarget {
    let ai_worker = hotsheet_aitools::session_worker_id("claude", terminal);
    let shell_worker = super::launch::terminal_session_worker_id(terminal);
    usage_target_for_checkout(state, usage_checkout, &[&ai_worker, &shell_worker]).unwrap_or_else(
        |error| {
            tracing::warn!(%error, "looking up Claude terminal ticket failed");
            UsageTarget {
                store: state.store.clone(),
                ticket: None,
            }
        },
    )
}

/// A message's own time when it is a valid past RFC 3339 time, else now. Using the message
/// time keeps a re-read message in the same raw day file, where its identity dedupes it.
fn message_ts(message: &MessageUsage) -> String {
    let now = OffsetDateTime::now_utc();
    let at = OffsetDateTime::parse(&message.timestamp, &Rfc3339)
        .ok()
        .map(|at| at.to_offset(time::UtcOffset::UTC))
        .filter(|at| *at <= now)
        .unwrap_or(now);
    Timestamp::from_datetime(at).as_str().to_owned()
}

/// The subagent followers for a follow that starts now: transcripts already present start
/// at their end, like the main transcript, because their earlier rows belong to turns this
/// server did not attribute (HS2-JW6N67).
fn subagents_at_end(transcript: &FilePath) -> BTreeMap<String, TranscriptFollower> {
    subagent_transcripts(transcript)
        .into_iter()
        .map(|(agent, path)| {
            let end = path.metadata().map(|meta| meta.len()).unwrap_or(0);
            (agent, TranscriptFollower::subagent(end))
        })
        .collect()
}

/// Read what the transcript and its subagent transcripts appended and record every final
/// message (all of the main transcript's when the turn ended; a subagent can outlive the
/// turn, so its newest message waits for its stop reason or a later message). Messages
/// stay pending through a failed write and are retried by the next wake, prompt, or stop
/// under the same identities. Blocking.
fn record_ready(
    state: &AppState,
    inner: &mut FollowInner,
    turn_ended: bool,
) -> std::io::Result<usize> {
    let target = match &inner.target {
        Some(target) => target.clone(),
        None => {
            let target = resolve_target(state, &inner.terminal, inner.usage_checkout.as_deref());
            inner.target = Some(target.clone());
            target
        }
    };
    let prices = hotsheet_ticketing::pricing::load_prices(&target.store);
    inner
        .reader
        .read_appended(&inner.transcript, Some(&prices))?;
    let mut subagent_ready = Vec::new();
    for (agent, path) in subagent_transcripts(&inner.transcript) {
        // A transcript not followed yet appeared after the follow began: read it from the
        // start (HS2-JW6N67).
        let reader = inner
            .subagents
            .entry(agent.clone())
            .or_insert_with(|| TranscriptFollower::subagent(0));
        reader.read_appended(&path, Some(&prices))?;
        let ready = reader.ready(false);
        if !ready.is_empty() {
            subagent_ready.push((agent, ready));
        }
    }
    let ready = inner.reader.ready(turn_ended);
    if ready.is_empty() && subagent_ready.is_empty() {
        persist(state, inner);
        return Ok(0);
    }
    let event = |identity: String, message: &MessageUsage| {
        (
            identity,
            metrics::UsageEvent {
                ts: message_ts(message),
                tool: "claude".to_owned(),
                model: message.usage.model.clone(),
                tokens_in: message.usage.tokens_in,
                tokens_out: message.usage.tokens_out,
                cost_usd: message.usage.cost_usd,
                ticket: target.ticket.clone(),
                session: Some(inner.session_id.clone()),
            },
            message.categories,
        )
    };
    let mut events: Vec<_> = ready
        .iter()
        .map(|message| {
            event(
                format!("claude-message:{}:{}", inner.session_id, message.message_id),
                message,
            )
        })
        .collect();
    for (agent, messages) in &subagent_ready {
        events.extend(messages.iter().map(|message| {
            event(
                format!(
                    "claude-message:{}:agent-{agent}:{}",
                    inner.session_id, message.message_id
                ),
                message,
            )
        }));
    }
    let added = metrics::record_many_once_with_claude_tokens(&target.store, &events)?;
    inner.reader.recorded(ready.len());
    inner.turn_messages += ready.len();
    for (agent, messages) in &subagent_ready {
        if let Some(reader) = inner.subagents.get_mut(agent) {
            reader.recorded(messages.len());
        }
    }
    persist(state, inner);
    if added > 0 {
        // Tokens per minute moved in the project store (HS2-JTTQMY, HS2-B4EQ87).
        crate::live_state::touch(state, &crate::multistore::store_url_id(&target.store));
    }
    Ok(added)
}

/// Start following with `reader`; the watcher records each final message as it appears.
fn start_follow(state: &AppState, inner: FollowInner) -> ClaudeUsageFollow {
    let (session_id, transcript) = (inner.session_id.clone(), inner.transcript.clone());
    persist(state, &inner);
    let inner = Arc::new(Mutex::new(inner));
    let (watch_state, watch_inner) = (state.clone(), inner.clone());
    let watch = watch_transcript(&transcript, move || {
        let mut inner = watch_inner.lock_or_recover();
        if let Err(error) = record_ready(&watch_state, &mut inner, false) {
            tracing::warn!(%error, "recording Claude terminal usage failed");
        }
    })
    .inspect_err(|error| tracing::warn!(%error, "watching a Claude transcript failed"))
    .ok();
    ClaudeUsageFollow {
        session_id,
        transcript,
        inner,
        _watch: watch,
    }
}

/// The follow of `terminal`'s Claude session, created at the transcript's current end when
/// none exists (or a different session or transcript had it). `true` when it is new.
fn ensure_follow(
    state: &AppState,
    terminal: &str,
    session_id: &str,
    transcript: &FilePath,
) -> (Arc<Mutex<FollowInner>>, bool) {
    if let Some(follow) = state.claude_usage_follows.lock_or_recover().get(terminal)
        && follow.session_id == session_id
        && follow.transcript == transcript
    {
        return (follow.inner.clone(), false);
    }
    // A new session starts at its transcript's end: older rows belong to earlier turns
    // this server may already have recorded under other identities. A file not created
    // yet starts at byte zero.
    let offset = transcript.metadata().map(|meta| meta.len()).unwrap_or(0);
    let follow = start_follow(
        state,
        FollowInner {
            terminal: terminal.to_owned(),
            session_id: session_id.to_owned(),
            transcript: transcript.to_path_buf(),
            reader: TranscriptFollower::new(offset),
            subagents: subagents_at_end(transcript),
            usage_checkout: super::usage_store::terminal_usage_checkout(state, terminal),
            target: None,
            turn_start: None,
            turn_messages: 0,
            stop_waiting: false,
        },
    );
    let inner = follow.inner.clone();
    state
        .claude_usage_follows
        .lock_or_recover()
        .insert(terminal.to_owned(), follow);
    (inner, true)
}

/// A hook report from one known Claude session. A prompt makes the previous turn's
/// messages final and snapshots the new turn's claim; a stop or failure records every
/// message so far, waiting briefly for a late append.
pub(crate) async fn report_claude_turn(
    State(state): State<AppState>,
    Path(terminal): Path<String>,
    Json(body): Json<ClaudeTurnReq>,
) -> Result<StatusCode, ApiError> {
    check_claude_session(&state, &terminal, &body.session_id)?;
    let transcript = transcript_for_session(&body.transcript_path, &body.session_id)?;
    if let Err(error) = transcript.metadata()
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "Claude transcript unavailable",
        ));
    }
    let blocking = state.clone();
    tokio::task::spawn_blocking(move || {
        let state = blocking;
        let (inner, created) = ensure_follow(&state, &terminal, &body.session_id, &transcript);
        match body.event {
            TurnEvent::Start => {
                let ai_worker = hotsheet_aitools::session_worker_id("claude", &terminal);
                let shell_worker = super::launch::terminal_session_worker_id(&terminal);
                // Lists the project's tickets for the claim snapshot.
                let snapshot =
                    terminal_usage_target(&state, &terminal, &[&ai_worker, &shell_worker])?;
                let mut inner = inner.lock_or_recover();
                // The previous turn ended at this prompt; its last message is final.
                if let Err(error) = record_ready(&state, &mut inner, true) {
                    tracing::warn!(%error, "recording Claude terminal usage failed");
                }
                let offset = inner.reader.resume_offset();
                // A duplicate prompt hook before the transcript grows keeps its claim.
                let repeated = !created
                    && inner.turn_start == Some(offset)
                    && inner.turn_messages == 0
                    && !inner.reader.has_pending();
                if !repeated {
                    inner.target = Some(snapshot);
                    inner.turn_start = Some(offset);
                    inner.turn_messages = 0;
                }
                persist(&state, &inner);
            }
            TurnEvent::Stop | TurnEvent::Failure => {
                // A follow first seen at a stop knows nothing of the turn that ended.
                if created {
                    return Ok(());
                }
                // Claude can invoke Stop before its last transcript row is visible. Wait
                // briefly for that local append; never poll the server or infer older rows.
                {
                    let mut waiting = inner.lock_or_recover();
                    if waiting.stop_waiting {
                        // The Stop already waiting records this turn.
                        return Ok(());
                    }
                    waiting.stop_waiting = true;
                }
                let deadline = Instant::now() + TRANSCRIPT_FLUSH_WAIT;
                let result = loop {
                    let mut inner = inner.lock_or_recover();
                    if let Err(error) = record_ready(&state, &mut inner, false) {
                        break Err(error);
                    }
                    if inner.turn_messages > 0
                        || inner.reader.has_pending()
                        || Instant::now() >= deadline
                    {
                        break record_ready(&state, &mut inner, true);
                    }
                    drop(inner);
                    std::thread::sleep(TRANSCRIPT_FLUSH_RETRY);
                };
                inner.lock_or_recover().stop_waiting = false;
                result.map_err(internal)?;
            }
        }
        Ok::<(), ApiError>(())
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// The session or terminal ended: stop following and forget the persisted follow.
pub(crate) fn forget_claude_turn(state: &AppState, terminal: &str) {
    let removed = state
        .claude_usage_follows
        .lock_or_recover()
        .remove(terminal);
    drop(removed);
    if let Some(path) = record_path(state, terminal) {
        let _ = std::fs::remove_file(path);
    }
}

/// At startup, resume the persisted follows of terminals the broker kept alive (HS2-SSMDQ1)
/// from their resume offsets; already-recorded messages dedupe by identity. Records of
/// terminals that did not survive are removed.
pub fn resume_claude_usage_follows(state: &AppState, alive: &[String]) {
    let Ok(entries) = std::fs::read_dir(state.machine_home().join(FOLLOW_DIR)) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let record = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<FollowRecord>(&bytes).ok())
            .filter(|record| {
                record_path(state, &record.terminal).as_deref() == Some(path.as_path())
                    && transcript_for_session(
                        &record.transcript.to_string_lossy(),
                        &record.session_id,
                    )
                    .is_ok()
            });
        let Some(record) = record.filter(|record| alive.contains(&record.terminal)) else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        let terminal = record.terminal.clone();
        // Subagent transcripts missing from the record appeared after it was last written,
        // so the first read starts them at byte zero; a record from before HS2-JW6N67 starts
        // the existing ones at their end.
        let subagents = match &record.subagents {
            Some(offsets) => offsets
                .iter()
                .map(|(agent, offset)| (agent.clone(), TranscriptFollower::subagent(*offset)))
                .collect(),
            None => subagents_at_end(&record.transcript),
        };
        let follow = start_follow(
            state,
            FollowInner {
                terminal: record.terminal,
                session_id: record.session_id,
                transcript: record.transcript,
                reader: TranscriptFollower::new(record.offset),
                subagents,
                usage_checkout: record.usage_checkout,
                target: None,
                turn_start: None,
                turn_messages: 0,
                stop_waiting: false,
            },
        );
        state
            .claude_usage_follows
            .lock_or_recover()
            .insert(terminal, follow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_path_must_name_a_session_inside_a_projects_directory() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("projects").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let transcript = project.join("session-1.jsonl");
        std::fs::write(&transcript, "").unwrap();
        assert_eq!(
            transcript_for_session(transcript.to_str().unwrap(), "session-1").unwrap(),
            transcript
        );
        let pending = project.join("pending.jsonl");
        assert_eq!(
            transcript_for_session(pending.to_str().unwrap(), "pending").unwrap(),
            pending
        );
        assert!(transcript_for_session(transcript.to_str().unwrap(), "session-2").is_err());
        assert!(transcript_for_session(transcript.to_str().unwrap(), "../session-1").is_err());
        assert!(transcript_for_session("projects/project/pending.jsonl", "pending").is_err());
    }

    #[test]
    fn hs2_ssmdq1_message_time_is_used_only_when_it_is_a_valid_past_time() {
        let message = |timestamp: &str| MessageUsage {
            message_id: "m".into(),
            timestamp: timestamp.into(),
            usage: Default::default(),
            categories: None,
        };
        assert_eq!(
            message_ts(&message("2026-10-23T01:02:03.5+01:00")),
            "2026-10-23T00:02:03.5Z"
        );
        let today = &crate::now().as_str()[..10].to_owned();
        assert!(message_ts(&message("")).starts_with(today.as_str()));
        assert!(message_ts(&message("2999-01-01T00:00:00Z")).starts_with(today.as_str()));
        assert!(record_path_is_safe("01M6JTZXXEBCC3KN9EHWQZD3P2"));
        assert!(!record_path_is_safe("../x"));
        assert!(!record_path_is_safe(""));
    }

    fn record_path_is_safe(terminal: &str) -> bool {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::new(
            hotsheet_ticketing::FsStore::init(
                root.path().join("s.hs2"),
                &hotsheet_ticketing::StoreMetadata::new("HS"),
            )
            .unwrap(),
            "secret".into(),
        )
        .unwrap()
        .with_machine_home(root.path().join("home"));
        record_path(&state, terminal).is_some()
    }
}
