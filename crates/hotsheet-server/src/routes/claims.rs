//! Coordination: claim / release / renew (HS2-86).

use crate::api_error::ApiError;
use crate::dto::{ClaimReq, ReleaseReq, RenewReq};
use crate::multistore;
use crate::routes::checkout_tickets::checkout_ticket_owner;
use crate::routes::store_scoped::ResolvedTicket;
use crate::routes::tickets::api_ticket;
use crate::{AppState, now};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_ticketing::{ApiTicket, ops};

// ---- coordination: claim / release / renew (HS2-86) ------------------------------

const DEFAULT_LEASE_MINUTES: i64 = 30;

/// `POST /claim-next` — atomically claim the top available ticket for a worker. Returns the
/// claimed ticket, or `null` (200) when nothing is claimable.
pub(crate) async fn claim_next_ticket(
    State(state): State<AppState>,
    Json(req): Json<ClaimReq>,
) -> Result<Json<Option<ApiTicket>>, ApiError> {
    let entry = state.default_entry();
    let now = now();
    let lease = now.plus_minutes(req.lease_minutes.unwrap_or(DEFAULT_LEASE_MINUTES));
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let eta = req
        .eta
        .as_deref()
        .map(|raw| ops::parse_claim_eta(&now, raw))
        .transpose()?;
    let claimed = ops::claim_next_with_eta(&entry.store, &now, lease, &worker, req.label, eta)?;
    if let Some(t) = &claimed {
        state.changed_in(&entry, "claimed", t);
    }
    Ok(Json(
        claimed
            .as_ref()
            .map(|ticket| api_ticket(&entry, ticket))
            .transpose()?,
    ))
}

/// `POST /checkouts/{reference}/claim-next` — HS2-AV80C4: claim the next ticket across
/// the project's linked local ticket sources, so a project-launched agent self-serves from
/// its own queue instead of the machine server's default store. The sources form one
/// merged queue (Up Next, priority, creation order) with the default source winning a tie
/// (see `ops::claim_next_across_with_eta`). Provider sources (GitHub, GitLab, Jira) have
/// no claim leases and are skipped; a project with only provider sources gets 409, and a
/// linked local source that cannot be opened is a 409 rather than a silently empty queue.
pub(crate) async fn claim_checkout_next_ticket(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<ClaimReq>,
) -> Result<Json<Option<ResolvedTicket>>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(crate::routes::checkouts::checkout_lookup_error)?;
    let local = checkout
        .claim_next_sources()
        .map_err(|message| ApiError::new(StatusCode::CONFLICT, message))?;
    let entries = local
        .iter()
        .map(|source| {
            state.hosted_source(source).ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    format!(
                        "checkout links an unhosted git source: {}",
                        source.connection_id
                    ),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let stores = entries.iter().map(|entry| &entry.store).collect::<Vec<_>>();
    let now = now();
    let lease = now.plus_minutes(req.lease_minutes.unwrap_or(DEFAULT_LEASE_MINUTES));
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let eta = req
        .eta
        .as_deref()
        .map(|raw| ops::parse_claim_eta(&now, raw))
        .transpose()?;
    let Some((index, claimed)) =
        ops::claim_next_across_with_eta(&stores, &now, lease, &worker, req.label, eta)?
    else {
        return Ok(Json(None));
    };
    let entry = &entries[index];
    state.changed_in(entry, "claimed", &claimed);
    resolved_claim_ticket(entry, &claimed).map(|Json(ticket)| Json(Some(ticket)))
}

/// `POST /tickets/{id}/claim` — claim one exact open, unblocked ticket by slug or ULID.
pub(crate) async fn claim_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ClaimReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = state.default_entry();
    let ticket = entry
        .resolve(&id)?
        .ok_or_else(|| ApiError::not_found(&id))?;
    let now = now();
    let lease = now.plus_minutes(req.lease_minutes.unwrap_or(DEFAULT_LEASE_MINUTES));
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let eta = req
        .eta
        .as_deref()
        .map(|raw| ops::parse_claim_eta(&now, raw))
        .transpose()?;
    let claimed = ops::claim_with_eta(
        &entry.store,
        &ticket.id,
        &now,
        lease,
        &worker,
        req.label,
        eta,
    )?;
    state.changed_in(&entry, "claimed", &claimed);
    Ok(Json(api_ticket(&entry, &claimed)?))
}

/// `POST /tickets/{id}/release` — release a claim (holder-only unless `force`).
pub(crate) async fn release_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ReleaseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = state.default_entry();
    let ticket = entry
        .resolve(&id)?
        .ok_or_else(|| ApiError::not_found(&id))?;
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let released = ops::release(
        &entry.store,
        &ticket.id,
        now(),
        &worker,
        req.force.unwrap_or(false),
    )?;
    state.changed_in(&entry, "released", &released);
    Ok(Json(api_ticket(&entry, &released)?))
}

/// `POST /tickets/{id}/renew` — extend a claim's lease (holder-only).
pub(crate) async fn renew_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RenewReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = state.default_entry();
    let ticket = entry
        .resolve(&id)?
        .ok_or_else(|| ApiError::not_found(&id))?;
    let now = now();
    let lease = now.plus_minutes(req.lease_minutes.unwrap_or(DEFAULT_LEASE_MINUTES));
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let eta = req
        .eta
        .as_deref()
        .map(|raw| ops::parse_claim_eta(&now, raw))
        .transpose()?;
    let renewed = ops::renew_with_eta(&entry.store, &ticket.id, now, lease, &worker, eta)?;
    state.changed_in(&entry, "renewed", &renewed);
    Ok(Json(api_ticket(&entry, &renewed)?))
}

/// Resolve the owner before a claim mutation. An unqualified id must identify one
/// ticket in this checkout; a qualified id selects its linked source directly.
fn checkout_claim_entry(
    state: &AppState,
    reference: &str,
    id: &str,
) -> Result<(multistore::StoreEntry, String), ApiError> {
    let (source, native_id) = checkout_ticket_owner(state, reference, id)?;
    if source.provider != "git" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "claim operations are unsupported by {} ticket sources",
                source.provider
            ),
        ));
    }
    let entry = state.hosted_source(&source).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "checkout links an unhosted git source",
        )
    })?;
    Ok((entry, native_id))
}

fn resolved_claim_ticket(
    entry: &multistore::StoreEntry,
    ticket: &hotsheet_model::Ticket,
) -> Result<Json<ResolvedTicket>, ApiError> {
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket(entry, ticket)?,
    }))
}

/// HS2-EM6YWP: project-launched MCP claims must mutate the source that supplied
/// the ticket read, rather than the machine server's default store.
pub(crate) async fn claim_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<ClaimReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (entry, native_id) = checkout_claim_entry(&state, &reference, &id)?;
    let ticket = entry
        .resolve(&native_id)?
        .ok_or_else(|| ApiError::not_found(&id))?;
    let now = now();
    let lease = now.plus_minutes(req.lease_minutes.unwrap_or(DEFAULT_LEASE_MINUTES));
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let eta = req
        .eta
        .as_deref()
        .map(|raw| ops::parse_claim_eta(&now, raw))
        .transpose()?;
    let claimed = ops::claim_with_eta(
        &entry.store,
        &ticket.id,
        &now,
        lease,
        &worker,
        req.label,
        eta,
    )?;
    state.changed_in(&entry, "claimed", &claimed);
    resolved_claim_ticket(&entry, &claimed)
}

pub(crate) async fn release_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<ReleaseReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (entry, native_id) = checkout_claim_entry(&state, &reference, &id)?;
    let ticket = entry
        .resolve(&native_id)?
        .ok_or_else(|| ApiError::not_found(&id))?;
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let released = ops::release(
        &entry.store,
        &ticket.id,
        now(),
        &worker,
        req.force.unwrap_or(false),
    )?;
    state.changed_in(&entry, "released", &released);
    resolved_claim_ticket(&entry, &released)
}

pub(crate) async fn renew_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<RenewReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (entry, native_id) = checkout_claim_entry(&state, &reference, &id)?;
    let ticket = entry
        .resolve(&native_id)?
        .ok_or_else(|| ApiError::not_found(&id))?;
    let now = now();
    let lease = now.plus_minutes(req.lease_minutes.unwrap_or(DEFAULT_LEASE_MINUTES));
    let worker = req.worker.unwrap_or_else(|| "worker".into());
    let eta = req
        .eta
        .as_deref()
        .map(|raw| ops::parse_claim_eta(&now, raw))
        .transpose()?;
    let renewed = ops::renew_with_eta(&entry.store, &ticket.id, now, lease, &worker, eta)?;
    state.changed_in(&entry, "renewed", &renewed);
    resolved_claim_ticket(&entry, &renewed)
}
