//! `POST /checkouts/{reference}/batch`: one multi-selection update that lands on every
//! listed ticket or on none (HS2-A8TTD7).
//!
//! Every involved store's ticket transaction is held for the whole batch, taken in one
//! stable order, and each ticket file is snapshotted before the first write. When any
//! item fails (a stale token, a validation or completion rule, or an I/O error), the
//! files already written are restored while the locks are still held, nothing is
//! committed or published, and the error is returned. Another writer therefore sees the
//! selection either before or after the whole batch, never part-way.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use hotsheet_model::{Ticket, Ulid};
use hotsheet_ticketing::store_transactions::{TicketFileSnapshot, with_stores_transaction};
use hotsheet_ticketing::{FsStore, Settings};
use serde::Deserialize;

use super::checkout_git_ticket;
use crate::api_error::{ActorReq, ApiError};
use crate::dto::UpdateReq;
use crate::multistore::StoreEntry;
use crate::routes::groups;
use crate::routes::project_tools::checkout_settings;
use crate::routes::store_scoped::ResolvedTicket;
use crate::routes::ticket_writes::do_update_with_ticket;
use crate::routes::tickets::contextualize_api_ticket;
use crate::{AppState, multistore};

#[derive(Deserialize)]
struct CheckoutBatchUpdateReq {
    id: String,
    #[serde(flatten)]
    update: UpdateReq,
}

#[derive(Deserialize)]
pub(crate) struct CheckoutBatchReq {
    updates: Vec<CheckoutBatchUpdateReq>,
    /// One actor for the whole bulk operation (HS2-XF81CJ); an update naming its own wins.
    #[serde(default)]
    actor: Option<ActorReq>,
}

/// One admitted item: the store it lives in (with commits deferred to the batch), its
/// ticket, and the update to apply.
struct BatchItem {
    entry: StoreEntry,
    deferred: StoreEntry,
    ticket: Ulid,
    update: UpdateReq,
}

/// Apply a multi-selection update through one checkout-scoped request, all or nothing.
pub(crate) async fn batch_update_checkout_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<CheckoutBatchReq>,
) -> Result<(HeaderMap, Json<Vec<ResolvedTicket>>), ApiError> {
    let started = Instant::now();
    if req.updates.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bulk update requires at least one ticket",
        ));
    }
    let (_, settings) = checkout_settings(&state, &reference)?;
    let mut items = Vec::with_capacity(req.updates.len());
    let mut seen = BTreeSet::<(PathBuf, Ulid)>::new();
    for mut item in req.updates {
        if item.update.actor.is_none() {
            item.update.actor.clone_from(&req.actor);
        }
        let (entry, ticket) = checkout_git_ticket(&state, &reference, &item.id)?;
        // A ticket listed twice would see its own first write as a concurrent change; one
        // selection names each ticket once (HS2-A8TTD7).
        if !seen.insert((entry.store.root().to_path_buf(), ticket.id)) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("bulk update lists ticket {} more than once", ticket.slug),
            ));
        }
        // A fast refusal before taking any lock; the authoritative check runs again
        // under the store lock inside each update (HS2-W3JD5K).
        if item
            .update
            .expected_token
            .as_deref()
            .is_some_and(|token| token != ticket.updated_at.as_str())
        {
            return Err(ApiError::ticket_changed(format!(
                "ticket {} changed since it was read",
                item.id
            )));
        }
        let mut deferred = entry.clone();
        deferred.store = deferred.store.with_deferred_autocommit();
        items.push(BatchItem {
            entry,
            deferred,
            ticket: ticket.id,
            update: item.update,
        });
    }
    // HS2-PDSHNN, HS2-9KZSD4: resolve each requested group once through the project's
    // registry before the first write, so every ticket of the selection gets one id. An
    // invalid name refuses the batch.
    let mut resolved_groups = BTreeMap::<String, Option<Option<String>>>::new();
    for item in &mut items {
        if let Some(Some(input)) = &item.update.group {
            let value = match resolved_groups.get(input) {
                Some(value) => value.clone(),
                None => {
                    let value = groups::resolve_for_checkout(
                        &state,
                        &reference,
                        Some(Some(input.clone())),
                    )?;
                    resolved_groups.insert(input.clone(), value.clone());
                    value
                }
            };
            item.update.group = value;
            item.update.group_resolved = true;
        }
    }
    // HS2-A820C1: resolve each requested milestone once through the registry, so an
    // unknown title refuses the batch before the first write.
    let mut resolved_milestones = BTreeMap::<String, Option<Option<String>>>::new();
    for item in &mut items {
        if let Some(Some(input)) = &item.update.milestone {
            let value = match resolved_milestones.get(input) {
                Some(value) => value.clone(),
                None => {
                    let value = crate::routes::milestones::resolve_link_for_checkout(
                        &state,
                        &reference,
                        Some(Some(input.clone())),
                    )?;
                    resolved_milestones.insert(input.clone(), value.clone());
                    value
                }
            };
            item.update.milestone = value;
            item.update.milestone_resolved = true;
        }
    }
    let admission_time = started.elapsed();
    let stores = items
        .iter()
        .map(|item| item.deferred.store.clone())
        .collect::<Vec<FsStore>>();
    let store_refs = stores.iter().collect::<Vec<_>>();
    let applied = with_stores_transaction(&store_refs, || apply_all(&state, &settings, items))?;
    let write_time = started.elapsed() - admission_time;
    Ok(publish(
        &state,
        started,
        admission_time,
        write_time,
        applied,
    ))
}

/// Write every item under the held store locks, restoring every attempted ticket file
/// when one fails.
fn apply_all(
    state: &AppState,
    settings: &Settings,
    items: Vec<BatchItem>,
) -> Result<Vec<(StoreEntry, ResolvedTicket, Ticket)>, ApiError> {
    let snapshots = items
        .iter()
        .map(|item| TicketFileSnapshot::take(&item.deferred.store, &item.ticket))
        .collect::<Result<Vec<_>, _>>()?;
    let mut applied = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let result = do_update_with_ticket(
            state,
            &item.deferred,
            &item.ticket.to_string(),
            item.update,
            false,
        )
        .and_then(|(ticket, changed)| {
            contextualize_api_ticket(ticket, settings).map(|ticket| (ticket, changed))
        });
        match result {
            Ok((ticket, changed)) => {
                let resolved = ResolvedTicket {
                    store: multistore::store_url_id(&item.entry.store),
                    ticket,
                };
                applied.push((item.entry, resolved, changed));
            }
            // The failing item may itself have written part of its update (fields before
            // its note), so it is restored with the items before it.
            Err(error) => return Err(roll_back(&snapshots[..=index], error)),
        }
    }
    Ok(applied)
}

/// Restore the snapshots newest first and return the item's error. A restore that fails
/// is named in the message, since that ticket then keeps the batch's write.
fn roll_back(snapshots: &[TicketFileSnapshot], mut error: ApiError) -> ApiError {
    let failures = snapshots
        .iter()
        .rev()
        .filter_map(|snapshot| snapshot.restore().err())
        .map(|failure| failure.to_string())
        .collect::<Vec<_>>();
    if !failures.is_empty() {
        error.message = format!(
            "{}; rolling back the bulk update also failed: {}",
            error.message,
            failures.join("; ")
        );
    }
    error
}

/// Commit and publish a fully applied batch, one commit and one change set per store.
fn publish(
    state: &AppState,
    started: Instant,
    admission_time: Duration,
    write_time: Duration,
    applied: Vec<(StoreEntry, ResolvedTicket, Ticket)>,
) -> (HeaderMap, Json<Vec<ResolvedTicket>>) {
    let mut updated = Vec::with_capacity(applied.len());
    let mut pending = BTreeMap::<PathBuf, (StoreEntry, Vec<Ticket>)>::new();
    for (entry, resolved, changed) in applied {
        updated.push(resolved);
        pending
            .entry(entry.store.root().to_path_buf())
            .or_insert_with(|| (entry, Vec::new()))
            .1
            .push(changed);
    }
    let mut git_time = Duration::ZERO;
    let mut index_time = Duration::ZERO;
    let mut event_time = Duration::ZERO;
    let mut worklist_time = Duration::ZERO;
    for (_, (entry, tickets)) in pending {
        let paths = tickets
            .iter()
            .map(|ticket| entry.store.ticket_path(&ticket.id))
            .collect::<Vec<_>>();
        let git_started = Instant::now();
        entry
            .store
            .queue_autocommit_paths("Update selected Hot Sheet tickets", &paths);
        git_time += git_started.elapsed();
        let [index, events, worklist] = state.changed_many_in(&entry, "updated", &tickets);
        index_time += index;
        event_time += events;
        worklist_time += worklist;
    }
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
    let timing = format!(
        "admission;dur={:.1}, write;dur={:.1}, git;dur={:.1}, index;dur={:.1}, events;dur={:.1}, worklist;dur={:.1}, total;dur={:.1}",
        ms(admission_time),
        ms(write_time),
        ms(git_time),
        ms(index_time),
        ms(event_time),
        ms(worklist_time),
        ms(started.elapsed()),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "server-timing",
        axum::http::HeaderValue::from_str(&timing).expect("numeric server timing"),
    );
    (headers, Json(updated))
}
