//! HS2-25TP3N: route a human ticket note to a live AI session in its checkout.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_aitools::session_push::PushMessage;
use hotsheet_model::{AttachmentActorRole, NoteKind, Status, Ticket, Ulid};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{checkouts::Checkout, ops};
use serde::{Deserialize, Serialize};

use crate::api_error::ApiError;
use crate::multistore::StoreEntry;
use crate::routes::checkout_tickets::checkout_ticket_owner;
use crate::routes::project_tools::checkout_settings;
use crate::routes::{permissions, session_messages, terminals};
use crate::{AppState, now};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Channel {
    Terminal,
    Drive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) id: String,
    pub(crate) tool: String,
    pub(crate) worker: String,
    pub(crate) project: String,
    /// A client-owned drive is attached to one ticket source; terminals serve the project.
    pub(crate) source: Option<String>,
    pub(crate) channel: Channel,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Direct(Candidate),
    Choose(Vec<Candidate>),
    Start,
}

/// A short-lived reservation, removed after a durable delivery activity note is written.
/// The activity note itself guards retry after a server restart.
pub(crate) type Reservations = Arc<Mutex<HashSet<String>>>;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct SessionChoice {
    pub(crate) id: String,
    pub(crate) tool: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum Delivery {
    Sent {
        note_id: String,
        connection_id: String,
    },
    Queued {
        note_id: String,
        connection_id: String,
    },
    Choose {
        note_id: String,
        sessions: Vec<SessionChoice>,
    },
    Start {
        note_id: String,
    },
}

fn choice(candidate: &Candidate) -> SessionChoice {
    SessionChoice {
        id: candidate.id.clone(),
        tool: candidate.tool.clone(),
    }
}

fn message(slug: &str, response: bool) -> String {
    if response {
        format!(
            "A human answered FEEDBACK NEEDED on {slug}. Read the whole ticket before continuing."
        )
    } else {
        format!("A human added a note to {slug}. Read the whole ticket before continuing.")
    }
}

/// Only a newly appended human regular note on a Started ticket triggers a notification.
pub(crate) fn note_to_notify(ticket: &Ticket) -> Option<&hotsheet_model::Note> {
    (ticket.status == Status::Started)
        .then(|| ticket.notes.last())
        .flatten()
        .filter(|note| {
            note.kind == NoteKind::Regular
                && note
                    .actor
                    .as_ref()
                    .is_some_and(|actor| actor.role == AttachmentActorRole::Human)
        })
}

async fn candidates(state: &AppState, checkout: &Checkout, source: &str) -> Vec<Candidate> {
    let project = checkout.root.as_str();
    let live_ids = terminals::try_live_terminal_infos(state)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|terminal| terminal.alive)
        .map(|terminal| terminal.id)
        .collect::<HashSet<_>>();
    let registered = state
        .drive_registry
        .lock_or_recover()
        .list()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    for terminal in registered {
        if terminal.project != project
            || !live_ids.contains(&terminal.id)
            || session_messages::adapter(state, &terminal.id).is_none()
        {
            continue;
        }
        result.push(Candidate {
            worker: terminals::terminal_worker(state, &terminal.id)
                .await
                .unwrap_or_default(),
            id: terminal.id,
            tool: terminal.tool,
            project: terminal.project,
            source: None,
            channel: Channel::Terminal,
        });
    }
    result.extend(state.client_drives.list().into_iter().filter_map(|drive| {
        (drive.project == project
            && drive.source == source
            && drive.actions.iter().any(|action| action == "send_turn"))
        .then(|| Candidate {
            worker: hotsheet_aitools::session_worker_id(&drive.tool, &drive.id),
            id: drive.id,
            tool: drive.tool,
            project: drive.project,
            source: Some(drive.source),
            channel: Channel::Drive,
        })
    }));
    result
}

fn delivery_recorded(ticket: &Ticket, note_id: &str) -> bool {
    let marker = format!("HS2-25TP3N notified note {note_id} ");
    ticket
        .notes
        .iter()
        .any(|note| note.kind == NoteKind::Activity && note.text.starts_with(&marker))
}

fn reserve(state: &AppState, entry: &StoreEntry, ticket_id: &Ulid, note_id: &str) -> bool {
    let mut pending = state.feedback_notification_reservations.lock_or_recover();
    if pending.contains(note_id)
        || entry
            .store
            .read_ticket(ticket_id)
            .is_ok_and(|ticket| delivery_recorded(&ticket, note_id))
    {
        return false;
    }
    pending.insert(note_id.to_string())
}

fn record_delivery(
    state: &AppState,
    entry: &StoreEntry,
    ticket_id: &Ulid,
    note_id: &str,
    connection_id: &str,
) {
    let result = ops::add_note_with_metadata(
        &entry.store,
        ticket_id,
        Ulid::new(),
        now(),
        NoteKind::Activity,
        ops::NoteMetadataInput {
            kind_explicit: true,
            summary: Some("Notified the AI session of human ticket feedback".into()),
            actor: Some(hotsheet_model::NoteActor {
                role: AttachmentActorRole::System,
                id: None,
            }),
            ..Default::default()
        },
        format!("HS2-25TP3N notified note {note_id} through session {connection_id}."),
    );
    match result {
        Ok(ticket) => {
            state.changed_in(entry, "updated", &ticket);
            state
                .feedback_notification_reservations
                .lock_or_recover()
                .remove(note_id);
        }
        Err(error) => tracing::warn!(%error, note_id, "recording feedback delivery failed"),
    }
}

async fn deliver(
    state: &AppState,
    entry: &StoreEntry,
    ticket: &Ticket,
    note_id: &str,
    candidate: Candidate,
    text: String,
) -> Result<Delivery, ApiError> {
    if !reserve(state, entry, &ticket.id, note_id) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "feedback notification already delivered or in progress",
        ));
    }
    let connection_id = candidate.id;
    let result = match candidate.channel {
        Channel::Terminal => {
            let adapter = session_messages::adapter(state, &connection_id).ok_or_else(|| {
                ApiError::new(StatusCode::NOT_FOUND, "AI session is no longer reachable")
            });
            match adapter {
                Ok(adapter) => tokio::task::spawn_blocking(move || {
                    adapter.message(&PushMessage::Prompt { text, banner: None })
                })
                .await
                .map_err(|error| {
                    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?
                .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))
                .map(|_| ()),
                Err(error) => Err(error),
            }
        }
        Channel::Drive => match state.client_drives.get(&connection_id) {
            Ok(info) if info.busy => {
                let state = state.clone();
                let entry = entry.clone();
                let ticket_id = ticket.id;
                let queued_note_id = note_id.to_string();
                let id = connection_id.clone();
                tokio::spawn(async move {
                    let note_id = queued_note_id;
                    let mut events = state.subscribe();
                    loop {
                        let Ok(info) = state.client_drives.get(&id) else {
                            state
                                .feedback_notification_reservations
                                .lock_or_recover()
                                .remove(&note_id);
                            break;
                        };
                        if !info.busy {
                            match permissions::send_feedback_turn(
                                state.clone(),
                                id.clone(),
                                text.clone(),
                            )
                            .await
                            {
                                Ok(()) => {
                                    record_delivery(&state, &entry, &ticket_id, &note_id, &id)
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        ?error,
                                        note_id,
                                        "queued feedback delivery failed"
                                    );
                                    state
                                        .feedback_notification_reservations
                                        .lock_or_recover()
                                        .remove(&note_id);
                                }
                            }
                            break;
                        }
                        match tokio::time::timeout(Duration::from_secs(3600), events.recv()).await {
                            Ok(Ok(event)) if event.kind == "drive_updated" && event.id == id => {}
                            Ok(Ok(_))
                            | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
                            _ => {
                                state
                                    .feedback_notification_reservations
                                    .lock_or_recover()
                                    .remove(&note_id);
                                break;
                            }
                        }
                    }
                });
                return Ok(Delivery::Queued {
                    note_id: note_id.into(),
                    connection_id,
                });
            }
            Ok(_) => {
                permissions::send_feedback_turn(state.clone(), connection_id.clone(), text).await
            }
            Err(error) => Err(ApiError::new(StatusCode::NOT_FOUND, error.to_string())),
        },
    };
    if let Err(error) = result {
        state
            .feedback_notification_reservations
            .lock_or_recover()
            .remove(note_id);
        return Err(error);
    }
    record_delivery(state, entry, &ticket.id, note_id, &connection_id);
    Ok(Delivery::Sent {
        note_id: note_id.into(),
        connection_id,
    })
}

/// Called only after the human's note write commits; a delivery failure never rolls back it.
pub(crate) async fn after_human_note(
    state: &AppState,
    checkout: &Checkout,
    source: &str,
    entry: &StoreEntry,
    ticket: &Ticket,
) -> Option<Delivery> {
    let note = note_to_notify(ticket)?;
    let note_id = note.id.to_string();
    let response = note.response_to.is_some();
    let all = candidates(state, checkout, source).await;
    let claim = (!ops::claim_available(ticket, &now()))
        .then_some(ticket.claimed_by.as_deref())
        .flatten();
    match route(&checkout.root, source, claim, all.clone()) {
        Route::Direct(candidate) => {
            let text = message(&ticket.slug, response);
            match deliver(
                state,
                entry,
                ticket,
                &note_id,
                candidate.clone(),
                text.clone(),
            )
            .await
            {
                Ok(delivery) => Some(delivery),
                Err(error) => {
                    tracing::warn!(?error, note_id, "claimed feedback session unreachable");
                    let remaining = all
                        .into_iter()
                        .filter(|other| other.id != candidate.id)
                        .collect::<Vec<_>>();
                    Some(
                        match route(&checkout.root, source, None, remaining.clone()) {
                            Route::Direct(next) => {
                                match deliver(state, entry, ticket, &note_id, next, text).await {
                                    Ok(delivery) => delivery,
                                    Err(error) => {
                                        tracing::warn!(
                                            ?error,
                                            note_id,
                                            "fallback feedback session unreachable"
                                        );
                                        Delivery::Start { note_id }
                                    }
                                }
                            }
                            Route::Choose(candidates) => Delivery::Choose {
                                note_id,
                                sessions: candidates.iter().map(choice).collect(),
                            },
                            Route::Start => Delivery::Start { note_id },
                        },
                    )
                }
            }
        }
        Route::Choose(candidates) => Some(Delivery::Choose {
            note_id,
            sessions: candidates.iter().map(choice).collect(),
        }),
        Route::Start => Some(Delivery::Start { note_id }),
    }
}

#[derive(Deserialize)]
pub(crate) struct ChooseSessionReq {
    pub(crate) connection_id: String,
}

/// Explicit human fallback after a note was saved and no unique session could be selected.
pub(crate) async fn choose_feedback_session(
    State(state): State<AppState>,
    Path((reference, id, note_id)): Path<(String, String, String)>,
    Json(request): Json<ChooseSessionReq>,
) -> Result<Json<Delivery>, ApiError> {
    let (checkout, _) = checkout_settings(&state, &reference)?;
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        return Err(ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "feedback session delivery is not supported for this source",
        ));
    }
    let entry = state
        .hosted_source(&source)
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "ticket source is not hosted"))?;
    let ticket = entry
        .resolve(&native_id)?
        .ok_or_else(|| ApiError::not_found(&native_id))?;
    let note = ticket
        .notes
        .iter()
        .find(|note| note.id.to_string() == note_id)
        .filter(|note| {
            note.kind == NoteKind::Regular
                && note
                    .actor
                    .as_ref()
                    .is_some_and(|actor| actor.role == AttachmentActorRole::Human)
        })
        .ok_or_else(|| ApiError::not_found(&note_id))?;
    if ticket.status != Status::Started {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ticket is no longer Started",
        ));
    }
    let candidate = candidates(&state, &checkout, &source.connection_id)
        .await
        .into_iter()
        .find(|candidate| candidate.id == request.connection_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "session is not reachable in this project and source",
            )
        })?;
    let text = message(&ticket.slug, note.response_to.is_some());
    Ok(Json(
        deliver(&state, &entry, &ticket, &note_id, candidate, text).await?,
    ))
}

/// Select only connections that can see this exact checkout and source. A live claim wins;
/// without it, one reachable session is safe to use and ambiguity stays with the human.
pub(crate) fn route(
    project: &str,
    source: &str,
    live_claim_worker: Option<&str>,
    candidates: impl IntoIterator<Item = Candidate>,
) -> Route {
    let mut scoped = candidates
        .into_iter()
        .filter(|candidate| {
            candidate.project == project
                && candidate
                    .source
                    .as_deref()
                    .is_none_or(|candidate_source| candidate_source == source)
        })
        .collect::<Vec<_>>();
    scoped.sort_by(|left, right| left.id.cmp(&right.id));
    if let Some(worker) = live_claim_worker
        && let Some(candidate) = scoped.iter().find(|candidate| candidate.worker == worker)
    {
        return Route::Direct(candidate.clone());
    }
    match scoped.len() {
        0 => Route::Start,
        1 => Route::Direct(scoped.remove(0)),
        _ => Route::Choose(scoped),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, project: &str, source: Option<&str>) -> Candidate {
        Candidate {
            id: id.into(),
            tool: "codex".into(),
            worker: format!("codex-{id}"),
            project: project.into(),
            source: source.map(str::to_string),
            channel: if source.is_some() {
                Channel::Drive
            } else {
                Channel::Terminal
            },
        }
    }

    #[test]
    fn hs2_25tp3n_claim_precedes_fallback_and_is_not_inferred_from_claim_count() {
        let sessions = [
            candidate("one", "/project", None),
            candidate("two", "/project", None),
        ];
        assert_eq!(
            route("/project", "source", Some("codex-two"), sessions.clone()),
            Route::Direct(sessions[1].clone())
        );
        assert_eq!(
            route("/project", "source", None, sessions.clone()),
            Route::Choose(sessions.to_vec())
        );
        assert_eq!(
            route("/project", "source", Some("stale-worker"), sessions.clone()),
            Route::Choose(sessions.to_vec())
        );
    }

    #[test]
    fn hs2_25tp3n_scope_and_empty_refill_never_cross_a_project_or_source() {
        let foreign = candidate("foreign", "/other", None);
        let wrong_source = candidate("wrong-source", "/project", Some("another-source"));
        assert_eq!(
            route(
                "/project",
                "source",
                Some("codex-foreign"),
                [foreign, wrong_source]
            ),
            Route::Start
        );
        let ours = candidate("ours", "/project", Some("source"));
        assert_eq!(
            route("/project", "source", None, [ours.clone()]),
            Route::Direct(ours)
        );
    }
}
