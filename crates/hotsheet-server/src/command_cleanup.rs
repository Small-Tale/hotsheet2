//! Applies AI command auto-cleanup (HS2-27C161).
//!
//! A command ticket (tag `hs2-command:<id>`) whose command sets `cleanup` is archived, or moved
//! to Trash, once it reaches Completed or Verified. Three event-driven paths apply it, and none
//! polls:
//!
//! - a hosted Markdown store's change bus: server writes, MCP, and the headless CLI edits the
//!   store watcher reports;
//! - a write to an external provider (GitHub, GitLab, Jira) through the server;
//! - a bounded sweep of the command's completed tickets each time the host runs the command,
//!   which also catches a provider ticket completed outside Hot Sheet.
//!
//! Every path re-reads the ticket and applies the decision in
//! [`hotsheet_ticketing::command_cleanup`] through the provider update path, so Delete is the
//! provider's restorable Trash, never a permanent delete.

use crate::AppState;
use crate::events::ChangeEvent;
use crate::routes::providers::{do_provider_update, provider_for};
use hotsheet_model::Ulid;
use hotsheet_ticketing::command_cleanup::{self, CleanupPolicy};
use hotsheet_ticketing::commands::{self, CommandDefinition, CommandKind};
use hotsheet_ticketing::{ApiTicket, TicketQuery, command_schedules};
use tokio::sync::broadcast::error::RecvError;

/// Change kinds after which a hosted ticket may have just completed.
const TICKET_KINDS: [&str; 4] = ["created", "updated", "changed", "closed"];

/// The most completed tickets one command run tidies, so a large backlog spreads over runs.
pub(crate) const SWEEP_LIMIT: usize = 25;

/// The cleanup configured for `command_id` by the first checkout linking `connection_id`.
pub(crate) fn policy(state: &AppState, connection_id: &str, command_id: &str) -> CleanupPolicy {
    let hosted_root = state
        .host
        .get(connection_id)
        .map(|entry| entry.store.root().to_path_buf());
    let Ok(checkouts) = state.checkout_registry.list() else {
        return CleanupPolicy::default();
    };
    for checkout in checkouts {
        let linked = checkout
            .sources
            .iter()
            .any(|source| source.connection_id == connection_id)
            || hosted_root.as_deref().is_some_and(|root| {
                checkout
                    .stores
                    .iter()
                    .any(|store| crate::watcher::same_path(std::path::Path::new(store), root))
            });
        if !linked {
            continue;
        }
        let Ok(definitions) = commands::from_settings(&checkout.settings()) else {
            continue;
        };
        if let Some(command) = definitions
            .iter()
            .find(|command| command.id == command_id && command.kind == CommandKind::Ai)
        {
            return command_policy(command);
        }
    }
    CleanupPolicy::default()
}

/// A command's cleanup and when it was turned on.
pub(crate) fn command_policy(command: &CommandDefinition) -> CleanupPolicy {
    CleanupPolicy {
        cleanup: command.cleanup,
        since: command.cleanup_since.clone(),
    }
}

fn already_cleaned(ticket: &ApiTicket) -> bool {
    ticket.notes.iter().any(|note| {
        command_cleanup::is_cleanup_note(
            note.actor.as_ref().and_then(|actor| actor.id.as_deref()),
            note.summary.as_deref(),
            &note.text,
        )
    })
}

fn due(
    ticket: &ApiTicket,
    policy: impl FnOnce(&str) -> CleanupPolicy,
) -> Option<(String, command_cleanup::CommandCleanup)> {
    // Only a completion at or after the command's `cleanup_since` counts (HS2-27C161).
    let done_at = command_cleanup::reached_done_at(
        ticket.completed_at.as_deref(),
        ticket.verified_at.as_deref(),
    );
    command_cleanup::cleanup_due(
        ticket.status,
        &ticket.tags,
        done_at,
        already_cleaned(ticket),
        policy,
    )
}

/// Apply the cleanup `ticket` is due under `policy`, after re-reading it from its provider so
/// a reopened or already cleaned ticket is left alone. Returns the cleaned-up ticket.
pub(crate) fn cleanup_with(
    state: &AppState,
    connection_id: &str,
    ticket: &ApiTicket,
    policy: impl Fn(&str) -> CleanupPolicy,
) -> Option<ApiTicket> {
    due(ticket, &policy)?;
    let current = provider_for(state, connection_id)
        .ok()?
        .get(&ticket.native_id)
        .ok()?;
    let (command_id, cleanup) = due(&current, &policy)?;
    let status = serde_json::to_value(cleanup.target_status()?).ok()?;
    let request = serde_json::from_value(serde_json::json!({
        "status": status,
        "note": cleanup.note(),
        "note_kind": "regular",
        "note_summary": command_cleanup::CLEANUP_NOTE_SUMMARY,
        "actor": {"role": "system", "id": command_cleanup::CLEANUP_ACTOR_ID},
    }))
    .ok()?;
    match do_provider_update(state, connection_id, &current.native_id, request) {
        Ok((updated, _)) => {
            tracing::info!(
                connection = connection_id,
                ticket = %updated.slug,
                command = command_id,
                cleanup = ?cleanup,
                "cleaned up a completed command ticket"
            );
            Some(updated)
        }
        Err(error) => {
            // A provider without that status (or a transient failure) keeps the ticket as it
            // is; the next run of the command tries again.
            tracing::warn!(
                connection = connection_id,
                ticket = %current.slug,
                command = command_id,
                error = %error.message,
                "command ticket cleanup failed"
            );
            None
        }
    }
}

/// After a hosted store reports a ticket change: clean the ticket up if it just completed.
pub(crate) fn on_change_event(state: &AppState, event: &ChangeEvent) -> Option<ApiTicket> {
    if !TICKET_KINDS.contains(&event.kind.as_str()) {
        return None;
    }
    let entry = state.host.get(&event.store)?;
    let id = Ulid::from_string(&event.id).ok()?;
    // A cheap check of the file before any settings or provider read.
    let ticket = entry.store.read_ticket(&id).ok()?;
    if !command_cleanup::is_cleanup_trigger(ticket.status)
        || command_cleanup::command_id_from_tags(&ticket.tags).is_none()
    {
        return None;
    }
    let api = provider_for(state, &event.store)
        .ok()?
        .get(&event.id)
        .ok()?;
    cleanup_with(state, &event.store, &api, |command| {
        policy(state, &event.store, command)
    })
}

/// After a write to an external provider through the server: the cleaned-up ticket when the
/// write completed a command ticket, else the written ticket. A hosted store's own change
/// event covers it instead.
pub(crate) fn after_provider_write(
    state: &AppState,
    connection_id: &str,
    ticket: ApiTicket,
) -> ApiTicket {
    if state.host.get(connection_id).is_some()
        || !command_cleanup::is_cleanup_trigger(ticket.status)
    {
        return ticket;
    }
    cleanup_with(state, connection_id, &ticket, |command| {
        policy(state, connection_id, command)
    })
    .unwrap_or(ticket)
}

/// Tidy up to [`SWEEP_LIMIT`] of `command`'s completed tickets in `connection_id`, for a
/// ticket completed where no event reached the server. Returns how many it cleaned up.
pub(crate) fn sweep(state: &AppState, connection_id: &str, command: &CommandDefinition) -> usize {
    if command.cleanup.is_none() || command.kind != CommandKind::Ai {
        return 0;
    }
    let Ok(provider) = provider_for(state, connection_id) else {
        return 0;
    };
    let tag = command_schedules::command_tag(&command.id);
    let Ok(tickets) = provider.query(&TicketQuery {
        tags: vec![tag.clone()],
        ..TicketQuery::default()
    }) else {
        return 0;
    };
    tickets
        .iter()
        .filter(|ticket| {
            command_cleanup::is_cleanup_trigger(ticket.status)
                && command_cleanup::command_id_from_tags(&ticket.tags) == Some(command.id.as_str())
        })
        .take(SWEEP_LIMIT)
        .filter(|ticket| {
            cleanup_with(state, connection_id, ticket, |_| command_policy(command)).is_some()
        })
        .count()
}

/// Subscribe to the change bus and clean up hosted command tickets as they complete. Events
/// are applied in order on one worker thread, which blocks on its channel while nothing
/// happens; the bus task ends when the server stops, and the worker with it.
pub fn spawn_command_cleanup(state: AppState) -> tokio::task::JoinHandle<()> {
    let mut events = state.subscribe();
    let (queued, pending) = std::sync::mpsc::channel::<ChangeEvent>();
    let worker = state.clone();
    std::thread::Builder::new()
        .name("hotsheet-command-cleanup".into())
        .spawn(move || {
            while let Ok(event) = pending.recv() {
                on_change_event(&worker, &event);
            }
        })
        .expect("spawn the command cleanup worker");
    tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                event = events.recv() => event,
                () = state.stopping() => break,
            };
            match event {
                Ok(event) => {
                    if TICKET_KINDS.contains(&event.kind.as_str()) && queued.send(event).is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    // Missed completions are tidied by the command's next run.
                    tracing::warn!(skipped, "command cleanup fell behind the change bus");
                }
                Err(RecvError::Closed) => break,
            }
        }
    })
}

#[cfg(test)]
#[path = "command_cleanup_tests.rs"]
mod tests;
