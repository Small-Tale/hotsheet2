//! Coordination: claim / release / renew (HS2-86).

use crate::*;

// ---- coordination: claim / release / renew (HS2-86) ------------------------------

pub(crate) const DEFAULT_LEASE_MINUTES: i64 = 30;

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

/// `POST /tickets/{id}/claim` — claim one exact open, unblocked ticket by slug or ULID.
pub(crate) async fn claim_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ClaimReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = state.default_entry();
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
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
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
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
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
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
