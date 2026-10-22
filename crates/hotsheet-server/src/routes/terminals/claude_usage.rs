//! Per-turn usage for native Claude terminals (HS2-W2GNTT).
//!
//! The hook supplies a transcript path at prompt and stop. We keep only the starting byte
//! offset and the ticket claim snapshot; no transcript content enters server state.

use crate::{ApiError, AppState, now};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::metrics;
use serde::Deserialize;
use std::path::{Component, Path as FilePath, PathBuf};
use std::time::{Duration, Instant};

const TRANSCRIPT_FLUSH_WAIT: Duration = Duration::from_secs(1);
const TRANSCRIPT_FLUSH_RETRY: Duration = Duration::from_millis(50);

#[derive(Clone)]
pub struct ClaudeTurnStart {
    session_id: String,
    transcript: PathBuf,
    offset: u64,
    ticket: Option<String>,
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

fn current_claude_session(state: &AppState, terminal: &str, session_id: &str) -> bool {
    state
        .terminal_ai_connections
        .lock_or_recover()
        .get(terminal)
        .is_some_and(|connection| {
            connection.agent.as_deref() == Some("claude")
                && connection.session_id.as_deref() == Some(session_id)
        })
}

/// A hook report from one known Claude session. Unmatched and historical turns cannot
/// create metrics events; a repeated stop has no start state left to count twice.
pub(crate) async fn report_claude_turn(
    State(state): State<AppState>,
    Path(terminal): Path<String>,
    Json(body): Json<ClaudeTurnReq>,
) -> Result<StatusCode, ApiError> {
    if !current_claude_session(&state, &terminal, &body.session_id) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "Claude terminal session changed",
        ));
    }
    let transcript = transcript_for_session(&body.transcript_path, &body.session_id)?;
    match body.event {
        TurnEvent::Start => {
            let offset = match transcript.metadata() {
                Ok(metadata) => metadata.len(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                Err(_) => {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "Claude transcript unavailable",
                    ));
                }
            };
            let ai_worker = hotsheet_aitools::session_worker_id("claude", &terminal);
            let shell_worker = super::launch::terminal_session_worker_id(&terminal);
            let ticket = metrics::active_ticket_for_workers(
                &state.store,
                &[&ai_worker, &shell_worker],
                &now(),
            )
            .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            let mut starts = state.claude_turn_starts.lock_or_recover();
            // A duplicate prompt hook before the transcript grows keeps its original claim.
            if starts.get(&terminal).is_some_and(|previous| {
                previous.session_id == body.session_id
                    && previous.transcript == transcript
                    && previous.offset == offset
            }) {
                return Ok(StatusCode::NO_CONTENT);
            }
            starts.insert(
                terminal,
                ClaudeTurnStart {
                    session_id: body.session_id,
                    transcript,
                    offset,
                    ticket,
                },
            );
        }
        TurnEvent::Stop | TurnEvent::Failure => {
            let start = {
                let mut starts = state.claude_turn_starts.lock_or_recover();
                if starts.get(&terminal).is_none_or(|start| {
                    start.session_id != body.session_id || start.transcript != transcript
                }) {
                    return Ok(StatusCode::NO_CONTENT);
                }
                starts.remove(&terminal).expect("checked above")
            };
            let store = state.store.clone();
            let session_id = body.session_id;
            tokio::task::spawn_blocking(move || {
                let prices = hotsheet_ticketing::pricing::load_prices(&store);
                // Claude can invoke Stop before its last transcript row is visible. Wait
                // briefly for that local append; never poll the server or infer older rows.
                let deadline = Instant::now() + TRANSCRIPT_FLUSH_WAIT;
                let usage = loop {
                    let usage = match hotsheet_aitools::claude_transcript_usage::priced_usage_since(
                        &start.transcript,
                        start.offset,
                        &prices,
                    ) {
                        Ok(usage) => usage,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                        Err(error) => return Err(error),
                    };
                    if usage.is_some() || Instant::now() >= deadline {
                        break usage;
                    }
                    std::thread::sleep(TRANSCRIPT_FLUSH_RETRY);
                };
                if let Some(usage) = usage {
                    metrics::record(
                        &store,
                        &metrics::UsageEvent {
                            ts: now().as_str().to_owned(),
                            tool: "claude".to_owned(),
                            model: usage.model,
                            tokens_in: usage.tokens_in,
                            tokens_out: usage.tokens_out,
                            cost_usd: usage.cost_usd,
                            ticket: start.ticket,
                            session: Some(session_id),
                        },
                    )?;
                }
                Ok::<(), std::io::Error>(())
            })
            .await
            .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn forget_claude_turn(state: &AppState, terminal: &str) {
    state.claude_turn_starts.lock_or_recover().remove(terminal);
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
}
