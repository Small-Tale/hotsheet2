//! HS2-25TP3N: route a human ticket note to a live AI session in its checkout.
//! HS2-H9Z9KZ: external provider tickets use the same routing best-effort, without a
//! persisted delivery record; the human note itself is the durable record.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_aitools::session_push::PushMessage;
use hotsheet_model::{AttachmentActorRole, NoteKind, Status, Ticket, Ulid};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{ApiNote, ApiTicket, checkouts::Checkout, ops};
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
    /// When the terminal's connection registered (Unix ms); orders the primary (HS2-DB2FQ2).
    pub(crate) started_at_ms: u64,
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
    /// Best-effort delivery failed; the saved note reaches the AI when it next reads the
    /// ticket (HS2-H9Z9KZ).
    NotShared {
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

/// The provider-ticket form of [`note_to_notify`]: the newly appended regular note on a Started
/// ticket, written by a human. Providers such as GitHub read comments back without an actor, so
/// authorship comes from the write request (`request_human`) or a stored human actor. Not
/// Working reports are a separate operation and never notify (HS2-25TP3N).
pub(crate) fn api_note_to_notify(ticket: &ApiTicket, request_human: bool) -> Option<&ApiNote> {
    (ticket.status == Status::Started)
        .then(|| ticket.notes.last())
        .flatten()
        .filter(|note| {
            note.kind == NoteKind::Regular
                && match note.actor.as_ref() {
                    Some(actor) => actor.role == AttachmentActorRole::Human,
                    None => request_human,
                }
        })
}

/// Live terminals whose reported AI session can receive a pushed message, as registry
/// connections (HS2-25TP3N). `live_ids` are the terminals currently alive; `None` trusts the
/// registry, whose lifecycle monitor removes a terminal soon after it exits.
fn push_capable_terminals(
    state: &AppState,
    live_ids: Option<&HashSet<String>>,
) -> Vec<hotsheet_aitools::Connection> {
    state
        .drive_registry
        .lock_or_recover()
        .list()
        .into_iter()
        .filter(|terminal| {
            live_ids.is_none_or(|live| live.contains(&terminal.id))
                && session_messages::adapter(state, &terminal.id).is_some()
        })
        .cloned()
        .collect()
}

/// HS2-DB2FQ2: the primary AI session of each project, the one proactive feedback delivery
/// goes to when no claim names a session: the oldest push-capable live terminal (registry
/// start time, then id). It is derived, never stored, so closing the primary promotes the
/// next oldest and opening another terminal never takes the role (HS2-DB2FQ2).
pub(crate) fn primary_terminals<'a>(
    terminals: impl IntoIterator<Item = (&'a str, &'a str, u64)>,
) -> HashSet<String> {
    let mut best: std::collections::HashMap<&str, (u64, &str)> = std::collections::HashMap::new();
    for (id, project, started_at_ms) in terminals {
        let entry = best.entry(project).or_insert((started_at_ms, id));
        if (started_at_ms, id) < *entry {
            *entry = (started_at_ms, id);
        }
    }
    best.into_values().map(|(_, id)| id.to_string()).collect()
}

/// The ids of every project's primary terminal among `live_ids` (HS2-DB2FQ2); `None` trusts
/// the registry's own lifecycle for liveness.
pub(crate) fn feedback_primaries(
    state: &AppState,
    live_ids: Option<&HashSet<String>>,
) -> HashSet<String> {
    let terminals = push_capable_terminals(state, live_ids);
    primary_terminals(
        terminals
            .iter()
            .map(|t| (t.id.as_str(), t.project.as_str(), t.started_at_ms)),
    )
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
    let registered = push_capable_terminals(state, Some(&live_ids));
    let mut result = Vec::new();
    for terminal in registered {
        if terminal.project != project {
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
            started_at_ms: terminal.started_at_ms,
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
            started_at_ms: 0,
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

/// Send one notification. `record` names the Git store and ticket that receive a durable
/// delivery activity note and duplicate guard; provider tickets pass `None` and are
/// best-effort only (HS2-H9Z9KZ).
async fn deliver(
    state: &AppState,
    record: Option<(&StoreEntry, Ulid)>,
    note_id: &str,
    candidate: Candidate,
    text: String,
) -> Result<Delivery, ApiError> {
    if let Some((entry, ticket_id)) = record
        && !reserve(state, entry, &ticket_id, note_id)
    {
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
                let record = record.map(|(entry, ticket_id)| (entry.clone(), ticket_id));
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
                                    if let Some((entry, ticket_id)) = &record {
                                        record_delivery(&state, entry, ticket_id, &note_id, &id);
                                    }
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
    if let Some((entry, ticket_id)) = record {
        record_delivery(state, entry, &ticket_id, note_id, &connection_id);
    }
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
                Some((entry, ticket.id)),
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
                                match deliver(state, Some((entry, ticket.id)), &note_id, next, text)
                                    .await
                                {
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

/// HS2-H9Z9KZ: route a human note (including a FEEDBACK NEEDED answer) on a provider ticket. The claimed
/// session comes from a safe advisory claim (Jira); otherwise one reachable session, a human
/// choice, or Start, exactly as for Git. Nothing is persisted, and a failed send reports
/// [`Delivery::NotShared`] so the client can say the AI was not told immediately.
pub(crate) async fn after_provider_note(
    state: &AppState,
    checkout: &Checkout,
    source: &str,
    ticket: &ApiTicket,
    request_human: bool,
) -> Option<Delivery> {
    let note = api_note_to_notify(ticket, request_human)?;
    let note_id = note.id.clone();
    let text = message(&ticket.slug, note.response_to.is_some());
    let all = candidates(state, checkout, source).await;
    let claim = {
        let (state, checkout_id, source, native_id) = (
            state.clone(),
            checkout.id.clone(),
            source.to_string(),
            ticket.native_id.clone(),
        );
        tokio::task::spawn_blocking(move || {
            crate::provider_advisory_claims::safe_claim_worker(
                &state,
                &checkout_id,
                &source,
                &native_id,
            )
        })
        .await
        .ok()
        .flatten()
    };
    Some(
        match route(&checkout.root, source, claim.as_deref(), all.clone()) {
            Route::Direct(candidate) => {
                match deliver(state, None, &note_id, candidate.clone(), text.clone()).await {
                    Ok(delivery) => delivery,
                    Err(error) => {
                        tracing::warn!(?error, note_id, "provider feedback session unreachable");
                        let remaining = all
                            .into_iter()
                            .filter(|other| other.id != candidate.id)
                            .collect::<Vec<_>>();
                        match route(&checkout.root, source, None, remaining) {
                            Route::Direct(next) => deliver(state, None, &note_id, next, text)
                                .await
                                .unwrap_or(Delivery::NotShared { note_id }),
                            _ => Delivery::NotShared { note_id },
                        }
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
    // A human regular note on a Started ticket, read from the ticket's own source.
    let human_note = |kind: NoteKind, role: Option<AttachmentActorRole>| {
        kind == NoteKind::Regular && role == Some(AttachmentActorRole::Human)
    };
    let (slug, response, record) = if source.provider == "git" {
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
            .filter(|note| human_note(note.kind, note.actor.as_ref().map(|actor| actor.role)))
            .ok_or_else(|| ApiError::not_found(&note_id))?;
        if ticket.status != Status::Started {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "ticket is no longer Started",
            ));
        }
        let response = note.response_to.is_some();
        (ticket.slug.clone(), response, Some((entry, ticket.id)))
    } else {
        // HS2-H9Z9KZ: provider tickets are delivered best-effort, without a durable record.
        let provider = crate::routes::providers::provider_for(&state, &source.connection_id)?;
        let lookup = native_id.clone();
        let ticket = tokio::task::spawn_blocking(move || provider.get(&lookup))
            .await
            .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .map_err(crate::routes::providers::provider_transfer_error)?;
        let note = ticket
            .notes
            .iter()
            .find(|note| note.id == note_id)
            // Comments read back without an actor are accepted; the client only offers the
            // chooser for a note its human user just saved.
            .filter(|note| {
                note.kind == NoteKind::Regular
                    && note
                        .actor
                        .as_ref()
                        .is_none_or(|actor| actor.role == AttachmentActorRole::Human)
            })
            .ok_or_else(|| ApiError::not_found(&note_id))?;
        if ticket.status != Status::Started {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "ticket is no longer Started",
            ));
        }
        (ticket.slug.clone(), note.response_to.is_some(), None)
    };
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
    let text = message(&slug, response);
    let record = record
        .as_ref()
        .map(|(entry, ticket_id)| (entry, *ticket_id));
    Ok(Json(
        deliver(&state, record, &note_id, candidate, text).await?,
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
        // HS2-DB2FQ2: no claim names a session, so the project's primary (the oldest
        // push-capable terminal, see [`primary_terminals`]) receives it. Only client-owned drives
        // with no terminal still ask the human to choose.
        _ => match scoped
            .iter()
            .filter(|candidate| candidate.channel == Channel::Terminal)
            .min_by(|left, right| {
                (left.started_at_ms, &left.id).cmp(&(right.started_at_ms, &right.id))
            }) {
            Some(primary) => Route::Direct(primary.clone()),
            None => Route::Choose(scoped),
        },
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
            started_at_ms: 0,
        }
    }

    #[test]
    fn hs2_db2fq2_primary_is_the_oldest_terminal_per_project_across_open_close_reopen() {
        // One terminal: it is the primary.
        assert_eq!(
            primary_terminals([("b", "/p", 20)]),
            HashSet::from(["b".to_string()])
        );
        // A newer terminal opens: the older one stays primary.
        assert_eq!(
            primary_terminals([("b", "/p", 20), ("c", "/p", 30)]),
            HashSet::from(["b".to_string()])
        );
        // The primary closes: the remaining terminal is promoted.
        assert_eq!(
            primary_terminals([("c", "/p", 30)]),
            HashSet::from(["c".to_string()])
        );
        // A reopened terminal is newer, so the promoted primary keeps the role.
        assert_eq!(
            primary_terminals([("c", "/p", 30), ("b2", "/p", 40)]),
            HashSet::from(["c".to_string()])
        );
        // Equal start times break by id, and projects never share a primary.
        assert_eq!(
            primary_terminals([("z", "/p", 5), ("a", "/p", 5), ("q", "/other", 99)]),
            HashSet::from(["a".to_string(), "q".to_string()])
        );
        // Empty, then refilled.
        assert!(primary_terminals([]).is_empty());
        assert_eq!(
            primary_terminals([("n", "/p", 1)]),
            HashSet::from(["n".to_string()])
        );
    }

    fn provider_ticket(
        status: Status,
        kind: NoteKind,
        role: Option<AttachmentActorRole>,
    ) -> ApiTicket {
        let at = hotsheet_model::Timestamp::new("2026-11-03T00:00:00Z");
        let mut model = Ticket::new(
            Ulid::new(),
            "ENG-9",
            "Synthetic",
            "task",
            at.clone(),
            at.clone(),
        );
        model.status = status;
        model.notes.push(hotsheet_model::Note {
            id: Ulid::new(),
            kind,
            kind_explicit: true,
            created_at: at.clone(),
            edited_at: at,
            summary: None,
            confidence: None,
            feedback_for: None,
            feedback_request: false,
            response_to: None,
            ai_feedback: None,
            human_edited: false,
            actor: role.map(|role| hotsheet_model::NoteActor { role, id: None }),
            text: "Synthetic note".into(),
        });
        ApiTicket::from(&model)
    }

    #[test]
    fn hs2_h9z9kz_provider_trigger_matches_the_git_human_note_rule() {
        use AttachmentActorRole::{Ai, Human};
        // (status, kind, stored actor, request was human, notifies)
        let cases = [
            (Status::Started, NoteKind::Regular, Some(Human), false, true),
            (Status::Started, NoteKind::Regular, Some(Ai), true, false),
            // GitHub-style read-back: no stored actor, so the write request decides.
            (Status::Started, NoteKind::Regular, None, true, true),
            (Status::Started, NoteKind::Regular, None, false, false),
            (
                Status::Started,
                NoteKind::Activity,
                Some(Human),
                true,
                false,
            ),
            (
                Status::Completed,
                NoteKind::Regular,
                Some(Human),
                true,
                false,
            ),
            (Status::NotStarted, NoteKind::Regular, None, true, false),
        ];
        for (status, kind, role, request_human, expected) in cases {
            let ticket = provider_ticket(status, kind, role);
            assert_eq!(
                api_note_to_notify(&ticket, request_human).is_some(),
                expected,
                "{status:?} {kind:?} {role:?} {request_human}"
            );
        }
    }

    #[test]
    fn hs2_h9z9kz_not_shared_is_a_distinct_wire_status() {
        assert_eq!(
            serde_json::to_value(Delivery::NotShared {
                note_id: "n".into()
            })
            .unwrap(),
            serde_json::json!({"status": "not_shared", "note_id": "n"})
        );
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
        // HS2-DB2FQ2: without a live claim the deterministic primary receives it.
        assert_eq!(
            route("/project", "source", None, sessions.clone()),
            Route::Direct(sessions[0].clone())
        );
        assert_eq!(
            route("/project", "source", Some("stale-worker"), sessions.clone()),
            Route::Direct(sessions[0].clone())
        );
    }

    #[test]
    fn hs2_db2fq2_routing_follows_the_primary_across_open_close_reopen() {
        let at = |id: &str, started_at_ms: u64| Candidate {
            started_at_ms,
            ..candidate(id, "/project", None)
        };
        let (first, second, reopened) = (at("z-first", 10), at("a-second", 20), at("m-reopen", 30));
        // Two terminals: the older one is primary even though its id sorts later.
        assert_eq!(
            route("/project", "source", None, [second.clone(), first.clone()]),
            Route::Direct(first.clone())
        );
        // A claim still names its own waiting session over the primary.
        assert_eq!(
            route(
                "/project",
                "source",
                Some("codex-a-second"),
                [first.clone(), second.clone()]
            ),
            Route::Direct(second.clone())
        );
        // The primary closes (or its delivery failed and it was dropped): the next is promoted.
        assert_eq!(
            route("/project", "source", None, [second.clone()]),
            Route::Direct(second.clone())
        );
        // A reopened terminal is newer, so the promoted primary keeps the role.
        assert_eq!(
            route(
                "/project",
                "source",
                None,
                [reopened.clone(), second.clone()]
            ),
            Route::Direct(second.clone())
        );
        // Every terminal closed: start a session.
        assert_eq!(route("/project", "source", None, []), Route::Start);
        // Several client-owned drives with no terminal still ask the human.
        let drives = [
            candidate("drive-1", "/project", Some("source")),
            candidate("drive-2", "/project", Some("source")),
        ];
        assert_eq!(
            route("/project", "source", None, drives.clone()),
            Route::Choose(drives.to_vec())
        );
        // A terminal beside a drive is the primary.
        assert_eq!(
            route(
                "/project",
                "source",
                None,
                [drives[0].clone(), reopened.clone()]
            ),
            Route::Direct(reopened)
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
