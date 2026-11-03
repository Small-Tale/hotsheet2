//! Checkout-qualified advisory claims and provisional Jira replay (HS2-BPKM76).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::advisory_claim::{
    AdvisoryClaimEvent, AdvisoryClaimEventKind, AdvisoryClaimProjection, reconcile_claims,
    valid_write_event,
};
use hotsheet_ticketing::advisory_claim_ledger::{ClaimLedgerError, ClaimReplayState, now_epoch};
use hotsheet_ticketing::{ProviderError, TicketRef};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api_error::ApiError;
use crate::routes::checkout_tickets::checkout_source_for_create;
use crate::routes::providers::{provider_for, provider_transfer_error};

#[derive(Deserialize)]
pub struct ClaimRequest {
    worker: String,
    operation_id: String,
    kind: AdvisoryClaimEventKind,
    /// Caller asserts that provider connectivity is unavailable. A provisional
    /// claim is local intent and never authorizes automatic handoff.
    #[serde(default)]
    provisional: bool,
}

#[derive(Serialize)]
pub struct ClaimStatus {
    pub projection: AdvisoryClaimProjection,
    pub provisional: bool,
    pub remote_verified: bool,
    pub automatic_handoff_safe: bool,
    pub pending_operation_ids: Vec<String>,
    pub pending_operations: Vec<PendingOperation>,
}

#[derive(Serialize)]
pub struct PendingOperation {
    pub operation_id: String,
    pub state: ClaimReplayState,
}

fn ledger_error(error: ClaimLedgerError) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, error.to_string())
}

fn source(
    state: &AppState,
    checkout: &str,
    connection_id: &str,
    native_id: &str,
) -> Result<
    (
        String,
        std::sync::Arc<dyn hotsheet_ticketing::TicketProvider>,
    ),
    ApiError,
> {
    let linked = checkout_source_for_create(state, checkout, Some(connection_id))?;
    if linked.connection_id != connection_id || linked.provider != "jira" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "advisory claim source is not a linked Jira connection",
        ));
    }
    let checkout = state
        .checkout_registry
        .resolve(checkout)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    let provider = provider_for(state, connection_id)?;
    if !provider.descriptor().capabilities.advisory_claims {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "provider does not support advisory claims",
        ));
    }
    if native_id.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "native ticket id is empty",
        ));
    }
    Ok((checkout.id, provider))
}

fn local_status(
    state: &AppState,
    project_scope: &str,
    ticket: &TicketRef,
    remote: Result<AdvisoryClaimProjection, ProviderError>,
    now_ms: i64,
) -> Result<ClaimStatus, ApiError> {
    let ledger = state.advisory_claim_ledger.as_ref().ok_or_else(|| {
        ApiError::new(StatusCode::CONFLICT, "advisory claim ledger is unavailable")
    })?;
    let pending = ledger
        .lock_or_recover()
        .for_ticket(project_scope, &ticket.connection_id, &ticket.native_id)
        .map_err(ledger_error)?;
    if pending.is_empty() {
        let projection = remote.map_err(provider_transfer_error)?;
        return Ok(ClaimStatus {
            automatic_handoff_safe: !projection.conflicted,
            projection,
            provisional: false,
            remote_verified: true,
            pending_operation_ids: Vec::new(),
            pending_operations: Vec::new(),
        });
    }
    let mut events = pending
        .iter()
        .map(|entry| entry.event.clone())
        .collect::<Vec<_>>();
    for (i, event) in events.iter_mut().enumerate() {
        event.provider_order = (i + 1) as u64;
    }
    let mut projection = reconcile_claims(ticket, &events, now_ms, 60_000);
    let remote_verified = if let Ok(remote) = remote {
        projection.conflicted |= remote.conflicted;
        for claim in remote.claims {
            if let Some(local) = projection
                .claims
                .iter()
                .find(|local| local.operation_id == claim.operation_id)
            {
                projection.conflicted |= local.worker != claim.worker;
            } else {
                projection.claims.push(claim);
            }
        }
        projection.conflicted |= projection.claims.len() > 1;
        true
    } else {
        false
    };
    Ok(ClaimStatus {
        pending_operation_ids: pending
            .iter()
            .map(|entry| entry.event.operation_id.clone())
            .collect(),
        pending_operations: pending
            .iter()
            .map(|entry| PendingOperation {
                operation_id: entry.event.operation_id.clone(),
                state: entry.state,
            })
            .collect(),
        projection,
        provisional: true,
        remote_verified,
        automatic_handoff_safe: false,
    })
}

/// Reconcile remote state on each read, then replay a bounded provisional batch.
/// Unknown outcomes stop at needs_attention; they never mint a replacement op id.
pub fn replay_pending_once(
    state: &AppState,
    project_scope: &str,
    connection_id: &str,
    native_id: &str,
) -> Result<usize, ApiError> {
    let Some(ledger) = &state.advisory_claim_ledger else {
        return Ok(0);
    };
    let pending = ledger
        .lock_or_recover()
        .for_ticket(project_scope, connection_id, native_id)
        .map_err(ledger_error)?
        .into_iter()
        .take(16)
        .collect::<Vec<_>>();
    let mut settled = 0;
    for entry in pending {
        let linked = checkout_source_for_create(
            state,
            &entry.project_scope,
            Some(&entry.event.ticket.connection_id),
        );
        if !matches!(linked, Ok(ref source) if source.connection_id == entry.event.ticket.connection_id && source.provider == "jira")
        {
            if entry.state == ClaimReplayState::Queued {
                ledger
                    .lock_or_recover()
                    .settle(&entry, ClaimReplayState::NeedsAttention)
                    .map_err(ledger_error)?;
                settled += 1;
            }
            continue;
        }
        if entry.created_at.saturating_add(7 * 24 * 60 * 60) < now_epoch()
            || match entry.event.kind {
                AdvisoryClaimEventKind::Claim { expires_unix_ms } => {
                    expires_unix_ms <= now_epoch().saturating_mul(1000)
                }
                _ => false,
            }
        {
            ledger
                .lock_or_recover()
                .settle(&entry, ClaimReplayState::Expired)
                .map_err(ledger_error)?;
            settled += 1;
            continue;
        }
        let provider = match provider_for(state, &entry.event.ticket.connection_id) {
            Ok(provider) => provider,
            Err(_) => continue,
        };
        let now_ms = now_epoch().saturating_mul(1000);
        let status = match provider.advisory_claims(&entry.event.ticket.native_id, now_ms, 60_000) {
            Ok(status) => status,
            Err(ProviderError::Authentication { .. } | ProviderError::RateLimited { .. }) => {
                continue;
            }
            Err(_) => continue,
        };
        let already_there = status.claims.iter().any(|claim| {
            claim.worker == entry.event.worker && claim.operation_id == entry.event.operation_id
        });
        if entry.state == ClaimReplayState::NeedsAttention {
            if already_there && !status.conflicted {
                ledger
                    .lock_or_recover()
                    .settle(&entry, ClaimReplayState::Confirmed)
                    .map_err(ledger_error)?;
                settled += 1;
            }
            continue;
        }
        let outcome = if status.conflicted || (!status.claims.is_empty() && !already_there) {
            ClaimReplayState::NeedsAttention
        } else if already_there {
            ClaimReplayState::Confirmed
        } else {
            match provider.append_advisory_claim_event(
                &entry.event.ticket.native_id,
                &entry.event,
                now_ms,
                60_000,
            ) {
                Ok(after)
                    if !after.conflicted
                        && after.claims.iter().any(|claim| {
                            claim.worker == entry.event.worker
                                && claim.operation_id == entry.event.operation_id
                        }) =>
                {
                    ClaimReplayState::Confirmed
                }
                Ok(_) | Err(ProviderError::Conflict { .. }) => ClaimReplayState::NeedsAttention,
                Err(_) => continue,
            }
        };
        ledger
            .lock_or_recover()
            .settle(&entry, outcome)
            .map_err(ledger_error)?;
        settled += 1;
    }
    Ok(settled)
}

/// The single live advisory claim's worker, only when routing to it is safe (HS2-H9Z9KZ):
/// remote-verified, not provisional, not conflicted, and exactly one claim. A non-Jira source
/// or any read failure means there is no claimed session to prefer.
pub(crate) fn safe_claim_worker(
    state: &AppState,
    checkout: &str,
    connection_id: &str,
    native_id: &str,
) -> Option<String> {
    let (project_scope, provider) = source(state, checkout, connection_id, native_id).ok()?;
    let ticket = TicketRef {
        connection_id: connection_id.to_string(),
        native_id: native_id.to_string(),
    };
    let now_ms = now_epoch().saturating_mul(1000);
    let status = local_status(
        state,
        &project_scope,
        &ticket,
        provider.advisory_claims(native_id, now_ms, 60_000),
        now_ms,
    )
    .ok()?;
    match status.projection.claims.as_slice() {
        [claim]
            if status.remote_verified
                && !status.provisional
                && status.automatic_handoff_safe
                && !status.projection.conflicted =>
        {
            Some(claim.worker.clone())
        }
        _ => None,
    }
}

pub async fn get_advisory_claim(
    State(state): State<AppState>,
    Path((checkout, connection_id, native_id)): Path<(String, String, String)>,
) -> Result<Json<ClaimStatus>, ApiError> {
    let (project_scope, provider) = source(&state, &checkout, &connection_id, &native_id)?;
    let _ = replay_pending_once(&state, &project_scope, &connection_id, &native_id)?;
    let ticket = TicketRef {
        connection_id,
        native_id,
    };
    let now_ms = now_epoch().saturating_mul(1000);
    Ok(Json(local_status(
        &state,
        &project_scope,
        &ticket,
        provider.advisory_claims(&ticket.native_id, now_ms, 60_000),
        now_ms,
    )?))
}

pub async fn post_advisory_claim(
    State(state): State<AppState>,
    Path((checkout, connection_id, native_id)): Path<(String, String, String)>,
    Json(request): Json<ClaimRequest>,
) -> Result<Json<ClaimStatus>, ApiError> {
    let (project_scope, provider) = source(&state, &checkout, &connection_id, &native_id)?;
    let ticket = TicketRef {
        connection_id,
        native_id,
    };
    let now_ms = now_epoch().saturating_mul(1000);
    let event = AdvisoryClaimEvent {
        ticket: ticket.clone(),
        worker: request.worker,
        operation_id: request.operation_id,
        provider_order: 0,
        kind: request.kind,
    };
    if !valid_write_event(&event, &ticket, now_ms) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid advisory claim event",
        ));
    }
    if request.provisional {
        let ledger = state.advisory_claim_ledger.as_ref().ok_or_else(|| {
            ApiError::new(StatusCode::CONFLICT, "advisory claim ledger is unavailable")
        })?;
        ledger
            .lock_or_recover()
            .admit(&project_scope, &event, now_epoch())
            .map_err(ledger_error)?;
    } else {
        let ledger = state.advisory_claim_ledger.as_ref().ok_or_else(|| {
            ApiError::new(StatusCode::CONFLICT, "advisory claim ledger is unavailable")
        })?;
        if !ledger
            .lock_or_recover()
            .for_ticket(&project_scope, &ticket.connection_id, &ticket.native_id)
            .map_err(ledger_error)?
            .is_empty()
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "unresolved provisional advisory claim; reconcile before writing",
            ));
        }
        provider
            .append_advisory_claim_event(&ticket.native_id, &event, now_ms, 60_000)
            .map_err(provider_transfer_error)?;
    }
    Ok(Json(local_status(
        &state,
        &project_scope,
        &ticket,
        provider.advisory_claims(&ticket.native_id, now_ms, 60_000),
        now_ms,
    )?))
}
