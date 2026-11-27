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
use crate::routes::feedback_journal::{self, Admission, Journal, JournalEntry, JournalState};
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

/// A short-lived in-process reservation held while one attempt delivers a note. Across a
/// restart the delivery journal and the activity note guard retry (HS2-WKRFD5).
pub(crate) type Reservations = Arc<Mutex<HashSet<String>>>;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct SessionChoice {
    pub(crate) id: String,
    pub(crate) tool: String,
    /// The session's claim worker id, so a client can name the ticket it works (HS2-5MCAKY).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) worker: String,
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
        worker: candidate.worker.clone(),
    }
}

/// The wake-up text pushed into a session. HS2-KNRRCF: Claude Code 2.1.326 labels every
/// inbox message as coming from another Claude session and tells the model never to treat
/// it as the user's approval, so the text says it is a notice from Hot Sheet that carries
/// no decision, and that the person's answer is their own note on the named ticket (which
/// the AI reads from the store, attributed to them).
pub(crate) fn message(slug: &str, response: bool) -> String {
    let (what, written) = if response {
        ("answered FEEDBACK NEEDED on", "Their answer")
    } else {
        ("added a note to", "What they wrote")
    };
    format!(
        "Hot Sheet notice: a person {what} {slug}. {written} is their own note on {slug}, \
         attributed to them on the ticket; this notice carries no decision itself. Read the \
         whole ticket before continuing."
    )
}

/// What a newly appended note on a Git ticket asks of delivery (HS2-25TP3N, HS2-A1M4KN).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Notify<'a> {
    /// A human note on a Started ticket: the full routing (live claim, the AI that asked,
    /// the project's primary, a chooser or a start action).
    Route(&'a hotsheet_model::Note),
    /// HS2-A1M4KN: a human note answering an AI's question on an open ticket that is not
    /// Started, such as an orchestration planner's reuse question on a template goal it
    /// never claimed. Only the live session of the AI that asked (`worker`) is told; no
    /// other session, chooser or start action is offered.
    Asker {
        note: &'a hotsheet_model::Note,
        worker: &'a str,
    },
}

/// Only a newly appended human regular note triggers a notification: on a Started ticket
/// always, and on a Not Started or Backlog ticket only when it answers an AI's question
/// (HS2-A1M4KN). A closed ticket never notifies.
pub(crate) fn note_to_notify(ticket: &Ticket) -> Option<Notify<'_>> {
    let note = ticket.notes.last().filter(|note| {
        note.kind == NoteKind::Regular
            && note
                .actor
                .as_ref()
                .is_some_and(|actor| actor.role == AttachmentActorRole::Human)
    })?;
    match ticket.status {
        Status::Started => Some(Notify::Route(note)),
        Status::NotStarted | Status::Backlog => {
            asker(ticket, note).map(|worker| Notify::Asker { note, worker })
        }
        _ => None,
    }
}

/// HS2-A1M4KN: the session of the AI that asked, among those that see this checkout and
/// source; `None` when it has no live session, so nobody else is told.
pub(crate) fn asker_candidate(
    project: &str,
    source: &str,
    worker: &str,
    candidates: impl IntoIterator<Item = Candidate>,
) -> Option<Candidate> {
    let own = candidates
        .into_iter()
        .filter(|candidate| candidate.worker == worker);
    match route(project, source, Some(worker), None, own) {
        Route::Direct(candidate) => Some(candidate),
        Route::Choose(_) | Route::Start => None,
    }
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
pub(crate) fn push_capable_terminals(
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

/// HS2-H3S7YQ: the terminal a person chose as each project's feedback-primary session, keyed by
/// the checkout root a terminal's registry connection names. The choice lives in memory on this
/// server instance while the chosen terminal lives: it is forgotten when that terminal exits or
/// is killed, and is never persisted, so a restart returns every project to the derived rule.
pub(crate) type PrimaryChoices = Arc<Mutex<std::collections::HashMap<String, String>>>;

/// HS2-DB2FQ2: the primary AI session of each project, the one proactive feedback delivery
/// goes to when no claim names a session: the oldest push-capable live terminal (registry
/// start time, then id). Closing the primary promotes the next oldest and opening another
/// terminal never takes the role (HS2-DB2FQ2). HS2-H3S7YQ: a person's `chosen` terminal for a
/// project takes the role while it is among that project's eligible terminals; once it is not,
/// the derived rule applies again.
pub(crate) fn primary_terminals<'a>(
    terminals: impl IntoIterator<Item = (&'a str, &'a str, u64)>,
    chosen: &std::collections::HashMap<String, String>,
) -> HashSet<String> {
    let mut best: std::collections::HashMap<&str, (u64, &str)> = std::collections::HashMap::new();
    let mut picked: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for (id, project, started_at_ms) in terminals {
        let entry = best.entry(project).or_insert((started_at_ms, id));
        if (started_at_ms, id) < *entry {
            *entry = (started_at_ms, id);
        }
        if chosen.get(project).is_some_and(|choice| choice == id) {
            picked.insert(project, id);
        }
    }
    best.into_iter()
        .map(|(project, (_, id))| picked.get(project).copied().unwrap_or(id).to_string())
        .collect()
}

/// Every live terminal's feedback role (HS2-DB2FQ2, HS2-H3S7YQ).
#[derive(Debug, Default)]
pub(crate) struct FeedbackRoles {
    /// Push-capable live terminals: the only ones that can hold or be given the primary role.
    pub(crate) eligible: HashSet<String>,
    /// Each project's primary terminal.
    pub(crate) primaries: HashSet<String>,
    /// Primaries that hold the role because a person chose them.
    pub(crate) chosen: HashSet<String>,
}

/// The feedback roles of every terminal among `live_ids` (HS2-DB2FQ2); `None` trusts the
/// registry's own lifecycle for liveness.
pub(crate) fn feedback_roles(
    state: &AppState,
    live_ids: Option<&HashSet<String>>,
) -> FeedbackRoles {
    let terminals = push_capable_terminals(state, live_ids);
    let choices = state.feedback_primary_choices.lock_or_recover().clone();
    let primaries = primary_terminals(
        terminals
            .iter()
            .map(|t| (t.id.as_str(), t.project.as_str(), t.started_at_ms)),
        &choices,
    );
    let chosen = terminals
        .iter()
        .filter(|t| {
            primaries.contains(&t.id) && choices.get(&t.project).is_some_and(|id| *id == t.id)
        })
        .map(|t| t.id.clone())
        .collect();
    FeedbackRoles {
        eligible: terminals.into_iter().map(|t| t.id).collect(),
        primaries,
        chosen,
    }
}

/// The terminal a person chose as `project`'s feedback-primary session, if any (HS2-H3S7YQ).
pub(crate) fn chosen_primary(state: &AppState, project: &str) -> Option<String> {
    state
        .feedback_primary_choices
        .lock_or_recover()
        .get(project)
        .cloned()
}

/// Forget a terminal's feedback-primary choice once it exits or is killed, since its id may be
/// reused by a later terminal (HS2-H3S7YQ). Returns whether a choice was removed.
pub(crate) fn forget_primary_choice(state: &AppState, id: &str) -> bool {
    let mut choices = state.feedback_primary_choices.lock_or_recover();
    let before = choices.len();
    choices.retain(|_, chosen| chosen != id);
    choices.len() != before
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

/// The `code` of the 409 refusing to resend a note whose earlier delivery may have happened
/// (HS2-WKRFD5).
pub(crate) const DELIVERY_UNCERTAIN_CODE: &str = "feedback_delivery_uncertain";

/// The per-note delivery journal under the machine home (HS2-WKRFD5).
fn journal(state: &AppState) -> Journal {
    Journal::at(state.machine_home().join("feedback-deliveries"))
}

fn delivery_recorded(ticket: &Ticket, note_id: &str) -> bool {
    ticket.notes.iter().any(|note| {
        note.kind == NoteKind::Activity && feedback_journal::records_delivery(&note.text, note_id)
    })
}

fn release(state: &AppState, note_id: &str) {
    state
        .feedback_notification_reservations
        .lock_or_recover()
        .remove(note_id);
}

fn write_activity(
    state: &AppState,
    entry: &StoreEntry,
    ticket_id: &Ulid,
    text: String,
) -> Result<(), hotsheet_ticketing::StoreError> {
    let ticket = ops::add_note_with_metadata(
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
        text,
    )?;
    state.changed_in(entry, "updated", &ticket);
    Ok(())
}

/// Write the activity note a journal entry stands for (HS2-WKRFD5). The caller holds the
/// note's reservation, so no concurrent attempt writes it twice.
fn reconcile_entry(state: &AppState, entry: &StoreEntry, pending: &JournalEntry) {
    let Ok(ticket_id) = Ulid::from_string(&pending.ticket_id) else {
        return;
    };
    let recorded = entry
        .store
        .read_ticket(&ticket_id)
        .is_ok_and(|ticket| delivery_recorded(&ticket, &pending.note_id));
    if let Err(error) = feedback_journal::reconcile(&journal(state), pending, recorded, |text| {
        write_activity(state, entry, &ticket_id, text)
    }) {
        tracing::warn!(%error, note_id = pending.note_id, "reconciling feedback delivery failed");
    }
}

/// Record every journaled delivery of this store that no attempt in this process holds,
/// once a store write has just succeeded (HS2-WKRFD5).
fn reconcile_store(state: &AppState, entry: &StoreEntry) {
    for pending in journal(state).for_store(entry.store.root()) {
        if !state
            .feedback_notification_reservations
            .lock_or_recover()
            .insert(pending.note_id.clone())
        {
            continue;
        }
        reconcile_entry(state, entry, &pending);
        release(state, &pending.note_id);
    }
}

/// Record every journaled delivery of every hosted store (HS2-5TE2TG). A sent entry whose
/// activity note failed was otherwise recorded only on a retry for that note or the next
/// successful delivery to its store, so its ticket could lack the record indefinitely. Run once
/// the server has hosted its stores at startup, and for a store hosted later. Returns how many
/// stores had journaled deliveries.
pub(crate) fn reconcile_hosted_deliveries(state: &AppState) -> usize {
    let journal = journal(state);
    let mut stores = 0;
    for (id, _) in state.host.locations() {
        let Some(entry) = state.host.get(&id) else {
            continue;
        };
        if journal.for_store(entry.store.root()).is_empty() {
            continue;
        }
        stores += 1;
        reconcile_store(state, &entry);
    }
    stores
}

/// Record the journaled deliveries of one store that was just hosted (HS2-5TE2TG).
pub(crate) fn reconcile_hosted_store(state: &AppState, entry: &StoreEntry) {
    if !journal(state).for_store(entry.store.root()).is_empty() {
        reconcile_store(state, entry);
    }
}

/// Admit one delivery of `note_id`, or explain why it was refused. A journal entry left by an
/// earlier attempt (a failed activity write, or a restart mid-send) is reconciled here and
/// never resent (HS2-WKRFD5).
fn reserve(
    state: &AppState,
    entry: &StoreEntry,
    ticket_id: &Ulid,
    note_id: &str,
) -> Result<(), ApiError> {
    let refused = |message: &str| Err(ApiError::new(StatusCode::CONFLICT, message.to_string()));
    let admission = {
        let mut pending = state.feedback_notification_reservations.lock_or_recover();
        if pending.contains(note_id) {
            return refused("feedback notification already delivered or in progress");
        }
        let admission = feedback_journal::admit(&journal(state), note_id, || {
            entry
                .store
                .read_ticket(ticket_id)
                .is_ok_and(|ticket| delivery_recorded(&ticket, note_id))
        });
        if matches!(admission, Admission::Send | Admission::Reconcile(_)) {
            pending.insert(note_id.to_string());
        }
        admission
    };
    match admission {
        Admission::Send => Ok(()),
        Admission::Recorded => refused("feedback notification already delivered or in progress"),
        Admission::Reconcile(pending) => {
            reconcile_entry(state, entry, &pending);
            release(state, note_id);
            match pending.state {
                JournalState::Sent => refused("feedback notification already delivered"),
                JournalState::Sending => Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "feedback notification outcome is uncertain after an interrupted delivery; \
                     it was not resent",
                )
                .with_code(DELIVERY_UNCERTAIN_CODE)),
            }
        }
        Admission::Unreadable => Err(ApiError::new(
            StatusCode::CONFLICT,
            "feedback delivery state is unreadable; the notification was not resent",
        )
        .with_code(DELIVERY_UNCERTAIN_CODE)),
    }
}

/// Journal the intent, run `send`, and record the outcome (HS2-WKRFD5). Without a Git record
/// the send is best-effort (HS2-H9Z9KZ). A failed intent write means nothing is sent; a
/// failed activity write leaves a `sent` entry that refuses resends and is reconciled later.
async fn send_recorded(
    state: &AppState,
    record: Option<(&StoreEntry, Ulid)>,
    note_id: &str,
    connection_id: &str,
    send: impl std::future::Future<Output = Result<(), ApiError>>,
) -> Result<(), ApiError> {
    let Some((entry, ticket_id)) = record else {
        return send.await;
    };
    let journal = journal(state);
    let mut pending = JournalEntry {
        note_id: note_id.to_string(),
        ticket_id: ticket_id.to_string(),
        store_root: entry.store.root().to_path_buf(),
        connection_id: connection_id.to_string(),
        state: JournalState::Sending,
    };
    if let Err(error) = journal.write(&pending) {
        tracing::warn!(%error, note_id, "recording feedback delivery intent failed");
        release(state, note_id);
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "could not record the feedback delivery; the AI session was not notified",
        ));
    }
    if let Err(error) = send.await {
        // A leftover entry is later reported as uncertain, never resent.
        if let Err(error) = journal.remove(note_id) {
            tracing::warn!(%error, note_id, "clearing a failed feedback delivery intent failed");
        }
        release(state, note_id);
        return Err(error);
    }
    pending.state = JournalState::Sent;
    if let Err(error) = journal.write(&pending) {
        // The `sending` entry stays, which still refuses a resend.
        tracing::warn!(%error, note_id, "confirming feedback delivery in the journal failed");
    }
    match write_activity(
        state,
        entry,
        &ticket_id,
        feedback_journal::sent_text(note_id, connection_id),
    ) {
        Ok(()) => {
            if let Err(error) = journal.remove(note_id) {
                tracing::warn!(%error, note_id, "clearing a recorded feedback delivery failed");
            }
            release(state, note_id);
            reconcile_store(state, entry);
        }
        Err(error) => {
            // The journal keeps the delivery; a retry or the next successful delivery to this
            // store reconciles it.
            tracing::warn!(%error, note_id, "recording feedback delivery failed");
            release(state, note_id);
        }
    }
    Ok(())
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
    if let Some((entry, ticket_id)) = record {
        reserve(state, entry, &ticket_id, note_id)?;
    }
    let connection_id = candidate.id;
    let result = match candidate.channel {
        Channel::Terminal => {
            let adapter = session_messages::adapter(state, &connection_id);
            send_recorded(state, record, note_id, &connection_id, async move {
                let adapter = adapter.ok_or_else(|| {
                    ApiError::new(StatusCode::NOT_FOUND, "AI session is no longer reachable")
                })?;
                tokio::task::spawn_blocking(move || {
                    adapter.message(&PushMessage::Prompt { text, banner: None })
                })
                .await
                .map_err(|error| {
                    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?
                .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))
                .map(|_| ())
            })
            .await
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
                            release(&state, &note_id);
                            break;
                        };
                        if !info.busy {
                            // Nothing is journaled while queued: a restart before this point
                            // never sent the turn, so a later retry may send it (HS2-WKRFD5).
                            let send = permissions::send_feedback_turn(
                                state.clone(),
                                id.clone(),
                                text.clone(),
                            );
                            let record = record
                                .as_ref()
                                .map(|(entry, ticket_id)| (entry, *ticket_id));
                            if let Err(error) =
                                send_recorded(&state, record, &note_id, &id, send).await
                            {
                                tracing::warn!(?error, note_id, "queued feedback delivery failed");
                                release(&state, &note_id);
                            }
                            break;
                        }
                        match tokio::time::timeout(Duration::from_secs(3600), events.recv()).await {
                            Ok(Ok(event)) if event.kind == "drive_updated" && event.id == id => {}
                            Ok(Ok(_))
                            | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
                            _ => {
                                release(&state, &note_id);
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
                let send =
                    permissions::send_feedback_turn(state.clone(), connection_id.clone(), text);
                send_recorded(state, record, note_id, &connection_id, send).await
            }
            Err(error) => {
                release(state, note_id);
                Err(ApiError::new(StatusCode::NOT_FOUND, error.to_string()))
            }
        },
    };
    result.map(|()| Delivery::Sent {
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
    let note = match note_to_notify(ticket)? {
        Notify::Route(note) => note,
        Notify::Asker { note, worker } => {
            let note_id = note.id.to_string();
            let all = candidates(state, checkout, source).await;
            let candidate = asker_candidate(&checkout.root, source, worker, all)?;
            let text = message(&ticket.slug, note.response_to.is_some());
            return match deliver(state, Some((entry, ticket.id)), &note_id, candidate, text).await {
                Ok(delivery) => Some(delivery),
                Err(error) => {
                    // The note stays; the AI reads it when it next reads the ticket.
                    tracing::warn!(?error, note_id, "asking AI session unreachable");
                    None
                }
            };
        }
    };
    let note_id = note.id.to_string();
    let response = note.response_to.is_some();
    let all = candidates(state, checkout, source).await;
    let chosen = chosen_primary(state, &checkout.root);
    let claim = (!ops::claim_available(ticket, &now()))
        .then_some(ticket.claimed_by.as_deref())
        .flatten()
        // HS2-N5BDE4: without a live claim, the answer goes back to the AI that asked,
        // such as an orchestration planner waiting on a goal ticket's question.
        .or_else(|| asker(ticket, note));
    match route(
        &checkout.root,
        source,
        claim,
        chosen.as_deref(),
        all.clone(),
    ) {
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
                        match route(
                            &checkout.root,
                            source,
                            None,
                            chosen.as_deref(),
                            remaining.clone(),
                        ) {
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
    let chosen = chosen_primary(state, &checkout.root);
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
        match route(
            &checkout.root,
            source,
            claim.as_deref(),
            chosen.as_deref(),
            all.clone(),
        ) {
            Route::Direct(candidate) => {
                match deliver(state, None, &note_id, candidate.clone(), text.clone()).await {
                    Ok(delivery) => delivery,
                    Err(error) => {
                        tracing::warn!(?error, note_id, "provider feedback session unreachable");
                        let remaining = all
                            .into_iter()
                            .filter(|other| other.id != candidate.id)
                            .collect::<Vec<_>>();
                        match route(&checkout.root, source, None, chosen.as_deref(), remaining) {
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

/// HS2-N5BDE4: the worker id of the AI that asked the question a human note answers:
/// the feedback request the note replies to, else the newest unanswered one. Only an AI
/// author with an id counts; a person's own request names nobody.
pub(crate) fn asker<'a>(ticket: &'a Ticket, note: &hotsheet_model::Note) -> Option<&'a str> {
    let answered = |request: &hotsheet_model::Note| {
        ticket
            .notes
            .iter()
            .any(|reply| reply.id != note.id && reply.response_to == Some(request.id))
    };
    let request = match note.response_to {
        Some(id) => ticket.notes.iter().find(|request| request.id == id),
        None => ticket
            .notes
            .iter()
            .filter(|request| {
                request.id != note.id
                    && request.kind == NoteKind::FeedbackNeeded
                    && !answered(request)
            })
            .max_by_key(|request| request.id),
    }?;
    request
        .actor
        .as_ref()
        .filter(|actor| actor.role == AttachmentActorRole::Ai)
        .and_then(|actor| actor.id.as_deref())
}

/// Select only connections that can see this exact checkout and source. A live claim wins;
/// without it, one reachable session is safe to use and ambiguity stays with the human.
pub(crate) fn route(
    project: &str,
    source: &str,
    live_claim_worker: Option<&str>,
    chosen_primary: Option<&str>,
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
        // HS2-DB2FQ2: no claim names a session, so the project's primary (the terminal a
        // person chose, HS2-H3S7YQ, else the oldest push-capable terminal, see
        // [`primary_terminals`]) receives it. Only client-owned drives with no terminal still
        // ask the human to choose.
        _ => match scoped
            .iter()
            .filter(|candidate| candidate.channel == Channel::Terminal)
            .find(|candidate| chosen_primary.is_some_and(|chosen| candidate.id == chosen))
            .or_else(|| {
                scoped
                    .iter()
                    .filter(|candidate| candidate.channel == Channel::Terminal)
                    .min_by(|left, right| {
                        (left.started_at_ms, &left.id).cmp(&(right.started_at_ms, &right.id))
                    })
            }) {
            Some(primary) => Route::Direct(primary.clone()),
            None => Route::Choose(scoped),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HS2-KNRRCF: the pushed wake-up says it comes from Hot Sheet, carries no decision,
    /// and names the ticket whose note is the person's answer, for an answer and for a
    /// plain note alike.
    #[test]
    fn hs2_knrrcf_the_wake_up_names_hot_sheet_and_the_persons_note() {
        let answer = message("HS-7", true);
        assert_eq!(
            answer,
            "Hot Sheet notice: a person answered FEEDBACK NEEDED on HS-7. Their answer is \
             their own note on HS-7, attributed to them on the ticket; this notice carries no \
             decision itself. Read the whole ticket before continuing."
        );
        let note = message("HS-7", false);
        assert_eq!(
            note,
            "Hot Sheet notice: a person added a note to HS-7. What they wrote is their own \
             note on HS-7, attributed to them on the ticket; this notice carries no decision \
             itself. Read the whole ticket before continuing."
        );
        for text in [&answer, &note] {
            assert!(text.starts_with("Hot Sheet notice: a person "));
            assert_eq!(text.matches("HS-7").count(), 2, "{text}");
            // It never speaks for the person or as the user.
            assert!(
                !text.contains("approve") && !text.contains("A human"),
                "{text}"
            );
            assert!(text.chars().count() < 300);
        }
        assert_ne!(message("HS-7", true), message("HS-8", true));
    }

    /// HS2-5MCAKY: a chooser entry carries the session's claim worker, so the client can name
    /// the ticket it works; a candidate with no worker omits the field.
    #[test]
    fn hs2_5mcaky_a_session_choice_carries_its_worker() {
        let mut claimed = candidate("drive-1", "/work/demo", Some("git"));
        claimed.worker = "codex-worker".to_owned();
        let mut idle = candidate("drive-2", "/work/demo", Some("git"));
        idle.worker.clear();
        let delivery = Delivery::Choose {
            note_id: "NOTE1".to_owned(),
            sessions: vec![choice(&claimed), choice(&idle)],
        };
        let json = serde_json::to_value(&delivery).expect("serializes");
        assert_eq!(json["status"], "choose");
        assert_eq!(json["sessions"][0]["id"], "drive-1");
        assert_eq!(json["sessions"][0]["worker"], "codex-worker");
        assert!(json["sessions"][1].get("worker").is_none());
    }

    /// HS2-N5BDE4: an answer names the AI that asked: the request it replies to, else
    /// the newest unanswered one; a person's request or an answered one names nobody.
    #[test]
    fn hs2_n5bde4_an_answer_goes_back_to_the_ai_that_asked() {
        use hotsheet_model::{Note, NoteActor};
        let note = |id: u128, kind: NoteKind, actor: Option<(AttachmentActorRole, &str)>| Note {
            id: Ulid::from(id),
            kind,
            actor: actor.map(|(role, id)| NoteActor {
                role,
                id: Some(id.into()),
            }),
            kind_explicit: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            edited_at: "2026-01-01T00:00:00Z".into(),
            summary: None,
            confidence: None,
            feedback_for: None,
            feedback_request: false,
            response_to: None,
            ai_feedback: None,
            human_edited: false,
            text: String::new(),
        };
        let asked = note(
            1,
            NoteKind::FeedbackNeeded,
            Some((AttachmentActorRole::Ai, "claude-planner")),
        );
        let mut ticket = Ticket {
            notes: vec![asked.clone()],
            ..Ticket::default()
        };
        let mut answer = note(
            2,
            NoteKind::Regular,
            Some((AttachmentActorRole::Human, "maintainer")),
        );
        ticket.notes.push(answer.clone());
        assert_eq!(asker(&ticket, &answer), Some("claude-planner"));
        answer.response_to = Some(asked.id);
        ticket.notes[1] = answer.clone();
        assert_eq!(asker(&ticket, &answer), Some("claude-planner"));
        // Already answered by an earlier reply: a later plain note names nobody.
        let later = note(
            3,
            NoteKind::Regular,
            Some((AttachmentActorRole::Human, "maintainer")),
        );
        ticket.notes.push(later.clone());
        assert_eq!(asker(&ticket, &later), None);
        // A person's own question names nobody.
        let own = Ticket {
            notes: vec![
                note(
                    1,
                    NoteKind::FeedbackNeeded,
                    Some((AttachmentActorRole::Human, "maintainer")),
                ),
                note(2, NoteKind::Regular, None),
            ],
            ..Ticket::default()
        };
        assert_eq!(asker(&own, &own.notes[1]), None);
        // With no live claim, the asker's terminal is chosen over the primary.
        let planner = Candidate {
            worker: "claude-planner".into(),
            started_at_ms: 20,
            ..candidate("planner", "/project", None)
        };
        let primary = Candidate {
            started_at_ms: 10,
            ..candidate("primary", "/project", None)
        };
        assert_eq!(
            route(
                "/project",
                "source",
                asker(&ticket, &answer),
                Some("primary"),
                [primary, planner.clone()]
            ),
            Route::Direct(planner)
        );
    }

    /// HS2-A1M4KN: a person's answer to an AI's question notifies on a Not Started or
    /// Backlog ticket too, but only the AI that asked; a Started ticket keeps the full
    /// routing for any human note; a closed ticket, an AI's own note, a person's own
    /// question and an already answered one notify nobody.
    #[test]
    fn hs2_a1m4kn_an_answer_on_an_unstarted_ticket_goes_only_to_the_asker() {
        use hotsheet_model::{Note, NoteActor};
        let note = |id: u128, kind: NoteKind, actor: Option<(AttachmentActorRole, &str)>| Note {
            id: Ulid::from(id),
            kind,
            actor: actor.map(|(role, id)| NoteActor {
                role,
                id: Some(id.into()),
            }),
            kind_explicit: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            edited_at: "2026-01-01T00:00:00Z".into(),
            summary: None,
            confidence: None,
            feedback_for: None,
            feedback_request: false,
            response_to: None,
            ai_feedback: None,
            human_edited: false,
            text: String::new(),
        };
        let person = Some((AttachmentActorRole::Human, "maintainer"));
        let planner = Some((AttachmentActorRole::Ai, "claude-planner"));
        let asked = note(1, NoteKind::FeedbackNeeded, planner);
        let mut reply = note(2, NoteKind::Regular, person);
        reply.response_to = Some(asked.id);
        let plain = note(2, NoteKind::Regular, person);
        let ticket = |status: Status, notes: Vec<Note>| Ticket {
            status,
            notes,
            ..Ticket::default()
        };
        // Who is told, per status, for an answer replying to the planner's question.
        for (status, expected) in [
            (Status::NotStarted, Some("asker")),
            (Status::Backlog, Some("asker")),
            (Status::Started, Some("route")),
            (Status::Completed, None),
            (Status::Verified, None),
            (Status::Archive, None),
            (Status::Deleted, None),
            (Status::Moved, None),
        ] {
            for answer in [&reply, &plain] {
                let ticket = ticket(status, vec![asked.clone(), answer.clone()]);
                let got = note_to_notify(&ticket);
                match expected {
                    Some("asker") => assert_eq!(
                        got,
                        Some(Notify::Asker {
                            note: &ticket.notes[1],
                            worker: "claude-planner"
                        }),
                        "{status:?}"
                    ),
                    Some(_) => assert_eq!(got, Some(Notify::Route(&ticket.notes[1])), "{status:?}"),
                    None => assert_eq!(got, None, "{status:?}"),
                }
            }
        }
        // Not Started without an AI's open question: nothing, as before HS2-A1M4KN.
        for notes in [
            vec![plain.clone()],
            vec![note(1, NoteKind::FeedbackNeeded, person), plain.clone()],
            vec![
                asked.clone(),
                reply.clone(),
                note(3, NoteKind::Regular, person),
            ],
            vec![asked.clone(), note(2, NoteKind::Regular, planner)],
            vec![asked.clone(), note(2, NoteKind::Regular, None)],
            vec![asked.clone(), note(2, NoteKind::Activity, person)],
            vec![],
        ] {
            let unstarted = ticket(Status::NotStarted, notes);
            assert_eq!(note_to_notify(&unstarted), None, "{:?}", unstarted.notes);
        }
        // Repeated answers: the first reply to a question reaches the asker; once it is
        // answered, a later plain note does not, but a reply naming it still does.
        let mut answered = ticket(Status::NotStarted, vec![asked.clone(), reply.clone()]);
        let mut later = note(3, NoteKind::Regular, person);
        answered.notes.push(later.clone());
        assert_eq!(note_to_notify(&answered), None);
        later.response_to = Some(asked.id);
        answered.notes[2] = later;
        assert!(matches!(
            note_to_notify(&answered),
            Some(Notify::Asker {
                worker: "claude-planner",
                ..
            })
        ));
        // A Started ticket with a person's plain note and no question routes as before.
        let started = ticket(Status::Started, vec![plain.clone()]);
        assert_eq!(
            note_to_notify(&started),
            Some(Notify::Route(&started.notes[0]))
        );

        // Only the asker's own session in this checkout and source receives it.
        let own = Candidate {
            worker: "claude-planner".into(),
            started_at_ms: 20,
            ..candidate("planner", "/project", None)
        };
        let primary = Candidate {
            started_at_ms: 10,
            ..candidate("primary", "/project", None)
        };
        let elsewhere = Candidate {
            worker: "claude-planner".into(),
            ..candidate("elsewhere", "/other", None)
        };
        assert_eq!(
            asker_candidate(
                "/project",
                "source",
                "claude-planner",
                [primary.clone(), elsewhere.clone(), own.clone()]
            ),
            Some(own)
        );
        // Its session gone: neither the primary nor a chooser nor a start action.
        assert_eq!(
            asker_candidate(
                "/project",
                "source",
                "claude-planner",
                [primary.clone(), elsewhere]
            ),
            None
        );
        assert_eq!(
            asker_candidate("/project", "source", "claude-planner", []),
            None
        );
        // A drive of the asker on another source of the project does not see the ticket.
        let drive = Candidate {
            worker: "claude-planner".into(),
            ..candidate("drive", "/project", Some("other-source"))
        };
        assert_eq!(
            asker_candidate("/project", "source", "claude-planner", [primary, drive]),
            None
        );
    }

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
        let no_choice = std::collections::HashMap::new();
        // One terminal: it is the primary.
        assert_eq!(
            primary_terminals([("b", "/p", 20)], &no_choice),
            HashSet::from(["b".to_string()])
        );
        // A newer terminal opens: the older one stays primary.
        assert_eq!(
            primary_terminals([("b", "/p", 20), ("c", "/p", 30)], &no_choice),
            HashSet::from(["b".to_string()])
        );
        // The primary closes: the remaining terminal is promoted.
        assert_eq!(
            primary_terminals([("c", "/p", 30)], &no_choice),
            HashSet::from(["c".to_string()])
        );
        // A reopened terminal is newer, so the promoted primary keeps the role.
        assert_eq!(
            primary_terminals([("c", "/p", 30), ("b2", "/p", 40)], &no_choice),
            HashSet::from(["c".to_string()])
        );
        // Equal start times break by id, and projects never share a primary.
        assert_eq!(
            primary_terminals(
                [("z", "/p", 5), ("a", "/p", 5), ("q", "/other", 99)],
                &no_choice
            ),
            HashSet::from(["a".to_string(), "q".to_string()])
        );
        // Empty, then refilled.
        assert!(primary_terminals([], &no_choice).is_empty());
        assert_eq!(
            primary_terminals([("n", "/p", 1)], &no_choice),
            HashSet::from(["n".to_string()])
        );
    }

    /// HS2-H3S7YQ: the selection rule across choose, the chosen terminal exiting, choosing again,
    /// clearing, and two projects with their own choices.
    #[test]
    fn hs2_h3s7yq_a_chosen_terminal_holds_the_primary_until_it_leaves_or_is_cleared() {
        let set = |ids: &[&str]| {
            ids.iter()
                .map(|id| (*id).to_string())
                .collect::<HashSet<_>>()
        };
        let mut choices = std::collections::HashMap::new();
        let two = [("old", "/p", 10), ("new", "/p", 20)];
        // Nothing chosen: the oldest terminal is primary.
        assert_eq!(primary_terminals(two, &choices), set(&["old"]));
        // Choose the newer terminal: it takes the role.
        choices.insert("/p".to_string(), "new".to_string());
        assert_eq!(primary_terminals(two, &choices), set(&["new"]));
        // A third, newer terminal opens: the choice keeps the role.
        assert_eq!(
            primary_terminals(
                [("old", "/p", 10), ("new", "/p", 20), ("third", "/p", 30)],
                &choices
            ),
            set(&["new"])
        );
        // The chosen terminal exits (it is no longer eligible): the derived rule takes over even
        // before its choice is forgotten.
        assert_eq!(
            primary_terminals([("old", "/p", 10), ("third", "/p", 30)], &choices),
            set(&["old"])
        );
        // Choose again: a different terminal.
        choices.insert("/p".to_string(), "third".to_string());
        assert_eq!(
            primary_terminals([("old", "/p", 10), ("third", "/p", 30)], &choices),
            set(&["third"])
        );
        // Clear: back to the oldest.
        choices.remove("/p");
        assert_eq!(
            primary_terminals([("old", "/p", 10), ("third", "/p", 30)], &choices),
            set(&["old"])
        );
        // Two projects: each choice applies only to its own project, and a choice naming another
        // project's terminal never moves the role across projects.
        choices.insert("/p".to_string(), "p-new".to_string());
        choices.insert("/q".to_string(), "p-old".to_string());
        assert_eq!(
            primary_terminals(
                [
                    ("p-old", "/p", 1),
                    ("p-new", "/p", 2),
                    ("q-old", "/q", 1),
                    ("q-new", "/q", 2)
                ],
                &choices
            ),
            set(&["p-new", "q-old"])
        );
        // Empty, then refilled with the chosen id: the choice applies again.
        assert!(primary_terminals([], &choices).is_empty());
        assert_eq!(
            primary_terminals([("p-new", "/p", 5)], &choices),
            set(&["p-new"])
        );
    }

    /// HS2-H3S7YQ: routing sends feedback to the chosen terminal, a live claim still wins, a
    /// failed chosen terminal falls back to the oldest, and a stale choice is ignored.
    #[test]
    fn hs2_h3s7yq_routing_prefers_the_chosen_terminal_after_the_claim() {
        let at = |id: &str, started_at_ms: u64| Candidate {
            started_at_ms,
            ..candidate(id, "/project", None)
        };
        let (old, new) = (at("old", 10), at("new", 20));
        let both = [old.clone(), new.clone()];
        assert_eq!(
            route("/project", "source", None, Some("new"), both.clone()),
            Route::Direct(new.clone())
        );
        // A live claim names its own session over the chosen primary.
        assert_eq!(
            route(
                "/project",
                "source",
                Some("codex-old"),
                Some("new"),
                both.clone()
            ),
            Route::Direct(old.clone())
        );
        // The chosen terminal was dropped after a failed delivery: the oldest remains.
        assert_eq!(
            route("/project", "source", None, Some("new"), [old.clone()]),
            Route::Direct(old.clone())
        );
        // A choice naming no candidate (exited, or another project's terminal) is ignored.
        assert_eq!(
            route("/project", "source", None, Some("gone"), both),
            Route::Direct(old)
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

    /// A synthetic Started ticket in a fresh Git store, with a state rooted at `home`.
    fn journaled_state(dir: &std::path::Path) -> (AppState, StoreEntry, Ulid) {
        let store = hotsheet_ticketing::FsStore::init_git(
            dir.join("t.hs2"),
            &hotsheet_ticketing::StoreMetadata::new("HS"),
        )
        .unwrap();
        let at = hotsheet_model::Timestamp::new("2026-11-15T00:00:00Z");
        let mut ticket = Ticket::new(Ulid::new(), "HS-1", "Synthetic", "task", at.clone(), at);
        ticket.status = Status::Started;
        store.write_ticket(&ticket).unwrap();
        let state = AppState::new(store, "test-secret".into())
            .unwrap()
            .with_machine_home(dir.join("home"));
        let entry = state.default_entry();
        (state, entry, ticket.id)
    }

    /// HS2-5TE2TG: a restart records every journaled delivery of a hosted store without any
    /// retry or later delivery: a sent entry as notified, a mid-send entry as uncertain. An
    /// entry of a store this server does not host waits for that store, and a second pass
    /// changes nothing. Synthetic tickets only.
    #[test]
    fn hs2_5te2tg_startup_records_journaled_deliveries_of_hosted_stores_once() {
        let dir = tempfile::tempdir().unwrap();
        let (state, entry, ticket_id) = journaled_state(dir.path());
        let journal = journal(&state);
        let (sent, sending, elsewhere) = (
            Ulid::new().to_string(),
            Ulid::new().to_string(),
            Ulid::new().to_string(),
        );
        for (note, state_now, root) in [
            (&sent, JournalState::Sent, entry.store.root().to_path_buf()),
            (
                &sending,
                JournalState::Sending,
                entry.store.root().to_path_buf(),
            ),
            (
                &elsewhere,
                JournalState::Sent,
                dir.path().join("not-hosted.hs2"),
            ),
        ] {
            journal
                .write(&JournalEntry {
                    note_id: note.clone(),
                    ticket_id: ticket_id.to_string(),
                    store_root: root,
                    connection_id: "drive-1".into(),
                    state: state_now,
                })
                .unwrap();
        }
        // The server restarts: nothing retries these notes and nothing else is delivered.
        let restarted = state.clone().with_machine_home(dir.path().join("home"));
        restarted
            .feedback_notification_reservations
            .lock_or_recover()
            .clear();
        assert_eq!(restarted.reconcile_feedback_deliveries(), 1);
        let ticket = entry.store.read_ticket(&ticket_id).unwrap();
        assert!(delivery_recorded(&ticket, &sent));
        assert!(delivery_recorded(&ticket, &sending));
        assert!(journal.read(&sent).unwrap().is_none());
        assert!(journal.read(&sending).unwrap().is_none());
        // The unhosted store's entry stays for when that store is hosted.
        assert!(journal.read(&elsewhere).unwrap().is_some());
        // A second pass has nothing left to do and writes no duplicate note.
        let notes = ticket.notes.len();
        assert_eq!(restarted.reconcile_feedback_deliveries(), 0);
        assert_eq!(
            entry.store.read_ticket(&ticket_id).unwrap().notes.len(),
            notes
        );
    }

    #[tokio::test]
    async fn hs2_wkrfd5_queued_turns_journal_only_when_sent_and_a_restart_while_queued_resends() {
        let dir = tempfile::tempdir().unwrap();
        let (state, entry, ticket_id) = journaled_state(dir.path());
        let note = Ulid::new().to_string();
        let journal_file = dir
            .path()
            .join("home/feedback-deliveries")
            .join(format!("{note}.json"));
        // Queued behind a busy drive: only the in-memory reservation exists.
        reserve(&state, &entry, &ticket_id, &note).unwrap();
        assert!(!journal_file.exists(), "nothing is journaled while queued");
        assert_eq!(
            reserve(&state, &entry, &ticket_id, &note)
                .unwrap_err()
                .status,
            StatusCode::CONFLICT
        );
        // The server restarts while the turn is queued: it was never sent, so it may be.
        let restarted = state.clone().with_machine_home(dir.path().join("home"));
        restarted
            .feedback_notification_reservations
            .lock_or_recover()
            .clear();
        reserve(&restarted, &entry, &ticket_id, &note).unwrap();
        // The drive becomes idle but the turn fails: no journal, no reservation, retry allowed.
        let failed = send_recorded(
            &restarted,
            Some((&entry, ticket_id)),
            &note,
            "drive-1",
            async {
                assert!(
                    journal_file.exists(),
                    "the intent is journaled before the send"
                );
                Err(ApiError::new(StatusCode::BAD_GATEWAY, "drive went away"))
            },
        )
        .await;
        assert!(failed.is_err());
        assert!(!journal_file.exists());
        reserve(&restarted, &entry, &ticket_id, &note).unwrap();
        // The turn is sent and recorded: the journal is cleared and resends are refused.
        send_recorded(
            &restarted,
            Some((&entry, ticket_id)),
            &note,
            "drive-1",
            async { Ok(()) },
        )
        .await
        .unwrap();
        assert!(!journal_file.exists());
        let ticket = entry.store.read_ticket(&ticket_id).unwrap();
        assert!(delivery_recorded(&ticket, &note));
        assert_eq!(
            reserve(&restarted, &entry, &ticket_id, &note)
                .unwrap_err()
                .status,
            StatusCode::CONFLICT
        );
    }

    #[cfg(unix)] // The write failure is injected through Unix directory permissions.
    #[tokio::test]
    async fn hs2_wkrfd5_an_unwritable_journal_sends_nothing() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let (state, entry, ticket_id) = journaled_state(dir.path());
        // A readable but read-only journal directory: admission works, every intent write fails.
        let journal_dir = dir.path().join("home/feedback-deliveries");
        std::fs::create_dir_all(&journal_dir).unwrap();
        std::fs::set_permissions(&journal_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let note = Ulid::new().to_string();
        reserve(&state, &entry, &ticket_id, &note).unwrap();
        let sent = std::sync::atomic::AtomicBool::new(false);
        let result = send_recorded(
            &state,
            Some((&entry, ticket_id)),
            &note,
            "session-1",
            async {
                sent.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        assert_eq!(result.unwrap_err().status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!sent.load(std::sync::atomic::Ordering::SeqCst));
        // The reservation is released, so a later attempt (once the journal works) may send.
        std::fs::set_permissions(&journal_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        reserve(&state, &entry, &ticket_id, &note).unwrap();
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
            route(
                "/project",
                "source",
                Some("codex-two"),
                None,
                sessions.clone()
            ),
            Route::Direct(sessions[1].clone())
        );
        // HS2-DB2FQ2: without a live claim the deterministic primary receives it.
        assert_eq!(
            route("/project", "source", None, None, sessions.clone()),
            Route::Direct(sessions[0].clone())
        );
        assert_eq!(
            route(
                "/project",
                "source",
                Some("stale-worker"),
                None,
                sessions.clone()
            ),
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
            route(
                "/project",
                "source",
                None,
                None,
                [second.clone(), first.clone()]
            ),
            Route::Direct(first.clone())
        );
        // A claim still names its own waiting session over the primary.
        assert_eq!(
            route(
                "/project",
                "source",
                Some("codex-a-second"),
                None,
                [first.clone(), second.clone()]
            ),
            Route::Direct(second.clone())
        );
        // The primary closes (or its delivery failed and it was dropped): the next is promoted.
        assert_eq!(
            route("/project", "source", None, None, [second.clone()]),
            Route::Direct(second.clone())
        );
        // A reopened terminal is newer, so the promoted primary keeps the role.
        assert_eq!(
            route(
                "/project",
                "source",
                None,
                None,
                [reopened.clone(), second.clone()]
            ),
            Route::Direct(second.clone())
        );
        // Every terminal closed: start a session.
        assert_eq!(route("/project", "source", None, None, []), Route::Start);
        // Several client-owned drives with no terminal still ask the human.
        let drives = [
            candidate("drive-1", "/project", Some("source")),
            candidate("drive-2", "/project", Some("source")),
        ];
        assert_eq!(
            route("/project", "source", None, None, drives.clone()),
            Route::Choose(drives.to_vec())
        );
        // A terminal beside a drive is the primary.
        assert_eq!(
            route(
                "/project",
                "source",
                None,
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
                None,
                [foreign, wrong_source]
            ),
            Route::Start
        );
        let ours = candidate("ours", "/project", Some("source"));
        assert_eq!(
            route("/project", "source", None, None, [ours.clone()]),
            Route::Direct(ours)
        );
    }
}
