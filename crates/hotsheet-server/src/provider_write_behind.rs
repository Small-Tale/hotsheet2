//! Explicit, durable admission for experimental Jira field edits.

use super::*;
use hotsheet_ticketing::provider_outbox::project_pending_ticket;

#[derive(Deserialize)]
pub(super) struct QueuedProviderBatch {
    operations: Vec<QueuedProviderEdit>,
}

#[derive(Deserialize)]
struct QueuedProviderEdit {
    operation_id: String,
    native_id: String,
    patch: ProviderPatch,
}

#[derive(Serialize)]
pub(super) struct QueuedProviderResult {
    operation_id: String,
    state: &'static str,
    ticket: ApiTicket,
}

pub(super) async fn queue_jira_updates(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(request): Json<QueuedProviderBatch>,
) -> Result<(StatusCode, Json<Vec<QueuedProviderResult>>), ApiError> {
    let provider = provider_for(&state, &connection_id)?;
    if !provider.descriptor().capabilities.write_behind || state.jira_outbox.is_none() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "durable queued edits are unavailable for this provider connection",
        ));
    }
    if request.operations.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "operations must not be empty",
        ));
    }
    let outbox = state.jira_outbox.as_ref().expect("feature gate checked");
    let mut admissions = Vec::with_capacity(request.operations.len());
    for edit in request.operations {
        validate_edit(&edit)?;
        let existing = outbox
            .lock()
            .map_err(lock_error)?
            .get(&edit.operation_id)
            .map_err(outbox_error)?;
        let base = if let Some(existing) = existing {
            if existing.connection_id != connection_id || existing.native_id != edit.native_id {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "operation id belongs to another ticket",
                ));
            }
            serde_json::from_value::<ApiTicket>(existing.base_ticket)
                .map_err(internal_json_error)?
        } else {
            provider
                .get(&edit.native_id)
                .map_err(provider_transfer_error)?
        };
        if base.connection_id != connection_id || base.native_id != edit.native_id {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "provider returned a different ticket",
            ));
        }
        admissions.push(
            OutboxAdmission::new(edit.operation_id, &base, edit.patch).map_err(outbox_error)?,
        );
    }
    let mut guard = outbox.lock().map_err(lock_error)?;
    let admitted = guard.admit_batch(&admissions).map_err(outbox_error)?;
    let mut results = Vec::with_capacity(admitted.len());
    for operation in admitted {
        let pending = guard
            .pending_for_ticket(&operation.connection_id, &operation.native_id)
            .map_err(outbox_error)?;
        let base = pending.first().map_or_else(
            || operation.base_ticket.clone(),
            |first| first.base_ticket.clone(),
        );
        let projected = project_pending_ticket(&base, &pending).map_err(outbox_error)?;
        results.push(QueuedProviderResult {
            operation_id: operation.operation_id,
            state: "queued",
            ticket: serde_json::from_value(projected).map_err(internal_json_error)?,
        });
    }
    Ok((StatusCode::ACCEPTED, Json(results)))
}

fn validate_edit(edit: &QueuedProviderEdit) -> Result<(), ApiError> {
    if edit.operation_id.trim().is_empty() || edit.native_id.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "operation_id and native_id are required",
        ));
    }
    let patch = &edit.patch;
    if patch.status.is_some()
        || patch.started_phase.is_some()
        || patch.up_next.is_some()
        || patch.blocked_by.is_some()
        || patch.blocked_reason.is_some()
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "queued edits support only Jira title, details, category, priority, and tags",
        ));
    }
    if patch.title.is_none()
        && patch.details.is_none()
        && patch.category.is_none()
        && patch.priority.is_none()
        && patch.tags.is_none()
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "queued edit has no supported field change",
        ));
    }
    Ok(())
}

fn outbox_error(error: OutboxError) -> ApiError {
    let status = match error {
        OutboxError::Backpressure { .. } => StatusCode::TOO_MANY_REQUESTS,
        OutboxError::ChangedPayload(_) | OutboxError::MismatchedBase => StatusCode::CONFLICT,
        OutboxError::EmptyIdentity
        | OutboxError::InvalidProjectionOrder(_)
        | OutboxError::UnsupportedProjectionField { .. }
        | OutboxError::InvalidProjectionTicket
        | OutboxError::InvalidCapacity => StatusCode::BAD_REQUEST,
        OutboxError::Io(_) | OutboxError::Sqlite(_) | OutboxError::Json(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    ApiError::new(status, error.to_string())
}

fn lock_error<T>(_error: std::sync::PoisonError<T>) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "provider outbox lock poisoned",
    )
}

fn internal_json_error(error: serde_json::Error) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
