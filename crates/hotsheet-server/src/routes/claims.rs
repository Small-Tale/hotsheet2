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
use hotsheet_cli::provisional_claims::{ClaimProvisional, ProvisionalContext, claim_provisional};
use hotsheet_ticketing::{ApiTicket, ops};
use serde::Serialize;

// ---- coordination: claim / release / renew (HS2-86) ------------------------------

const DEFAULT_LEASE_MINUTES: i64 = 30;

/// HS2-XYF4YT: a claim, claim-next or renew answer. `provisional` appears only under an
/// enabled speculation policy with a prerequisite awaiting CI or a red gate not yet
/// reported to this claim; otherwise the answer is the bare ticket, byte for byte.
#[derive(Serialize)]
pub(crate) struct ClaimAnswer<T> {
    #[serde(flatten)]
    answer: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    provisional: Option<ClaimProvisional>,
}

/// The code checkout of a default-store claim: the one registered checkout linking it.
fn store_context(state: &AppState, entry: &multistore::StoreEntry) -> Option<ProvisionalContext> {
    ProvisionalContext::for_store(entry.store.root(), state.machine_home())
}

fn checkout_context(state: &AppState, reference: &str) -> Option<ProvisionalContext> {
    state
        .checkout_registry
        .resolve(reference)
        .ok()
        .map(|checkout| ProvisionalContext::for_checkout(&checkout, state.machine_home()))
}

/// The context is resolved only for a ticket with prerequisites, so a plain claim does
/// no extra registry or Git work.
fn answer<T>(
    answer: T,
    context: impl FnOnce() -> Option<ProvisionalContext>,
    entry: &multistore::StoreEntry,
    ticket: &hotsheet_model::Ticket,
) -> ClaimAnswer<T> {
    let provisional = if ticket.blocked_by.is_empty() {
        None
    } else {
        claim_provisional(context().as_ref(), &entry.store, ticket)
    };
    ClaimAnswer {
        answer,
        provisional,
    }
}

/// `POST /claim-next` — atomically claim the top available ticket for a worker. Returns the
/// claimed ticket, or `null` (200) when nothing is claimable.
pub(crate) async fn claim_next_ticket(
    State(state): State<AppState>,
    Json(req): Json<ClaimReq>,
) -> Result<Json<Option<ClaimAnswer<ApiTicket>>>, ApiError> {
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
    let Some(ticket) = claimed else {
        return Ok(Json(None));
    };
    Ok(Json(Some(answer(
        api_ticket(&entry, &ticket)?,
        || store_context(&state, &entry),
        &entry,
        &ticket,
    ))))
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
) -> Result<Json<Option<ClaimAnswer<ResolvedTicket>>>, ApiError> {
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
    let Json(resolved) = resolved_claim_ticket(entry, &claimed)?;
    let context = || {
        Some(ProvisionalContext::for_checkout(
            &checkout,
            state.machine_home(),
        ))
    };
    Ok(Json(Some(answer(resolved, context, entry, &claimed))))
}

/// `POST /tickets/{id}/claim` — claim one exact open ticket by slug or ULID; a blocked
/// ticket is claimable and keeps its reason (HS2-BAK4BC).
pub(crate) async fn claim_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ClaimReq>,
) -> Result<Json<ClaimAnswer<ApiTicket>>, ApiError> {
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
    Ok(Json(answer(
        api_ticket(&entry, &claimed)?,
        || store_context(&state, &entry),
        &entry,
        &claimed,
    )))
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
) -> Result<Json<ClaimAnswer<ApiTicket>>, ApiError> {
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
    Ok(Json(answer(
        api_ticket(&entry, &renewed)?,
        || store_context(&state, &entry),
        &entry,
        &renewed,
    )))
}

/// Resolve the owner before a claim mutation. An unqualified id must identify one
/// ticket in this checkout; a qualified id selects its linked source directly.
pub(crate) fn checkout_claim_entry(
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
) -> Result<Json<ClaimAnswer<ResolvedTicket>>, ApiError> {
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
    let Json(resolved) = resolved_claim_ticket(&entry, &claimed)?;
    Ok(Json(answer(
        resolved,
        || checkout_context(&state, &reference),
        &entry,
        &claimed,
    )))
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
) -> Result<Json<ClaimAnswer<ResolvedTicket>>, ApiError> {
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
    let Json(resolved) = resolved_claim_ticket(&entry, &renewed)?;
    Ok(Json(answer(
        resolved,
        || checkout_context(&state, &reference),
        &entry,
        &renewed,
    )))
}

#[cfg(test)]
mod tests {
    //! HS2-XYF4YT: with nothing to report, a claim answer serializes to exactly the bytes
    //! of the bare ticket (and of the resolved `{ store, ticket }`).
    use super::*;
    use hotsheet_model::{Status, Ticket, Ulid};

    fn ticket() -> ApiTicket {
        let mut ticket = Ticket {
            id: Ulid::new(),
            slug: "HS-1".into(),
            title: "Synthetic".into(),
            status: Status::Started,
            blocked_by: vec![Ulid::new()],
            ..Ticket::default()
        };
        ticket.claimed_by = Some("agent".into());
        ApiTicket::from(&ticket)
    }

    #[test]
    fn hs2_xyf4yt_answer_without_news_is_byte_identical() {
        let bare = ticket();
        let wrapped = ClaimAnswer {
            answer: bare.clone(),
            provisional: None,
        };
        assert_eq!(
            serde_json::to_string(&wrapped).unwrap(),
            serde_json::to_string(&bare).unwrap()
        );
        let resolved = ResolvedTicket {
            store: "abc".into(),
            ticket: bare,
        };
        let expected = serde_json::to_string(&resolved).unwrap();
        let wrapped = ClaimAnswer {
            answer: resolved,
            provisional: None,
        };
        assert_eq!(serde_json::to_string(&wrapped).unwrap(), expected);
    }

    #[test]
    fn hs2_xyf4yt_answer_with_news_adds_one_member() {
        let bare = ticket();
        let wrapped = ClaimAnswer {
            answer: bare.clone(),
            provisional: Some(ClaimProvisional {
                integration: hotsheet_ticketing::provisional::IntegrationRule::Parked,
                prerequisites: Vec::new(),
                red_gates: Vec::new(),
            }),
        };
        let mut expected = serde_json::to_value(&bare).unwrap();
        expected["provisional"] = serde_json::json!({ "integration": "parked" });
        assert_eq!(serde_json::to_value(&wrapped).unwrap(), expected);
    }
}
