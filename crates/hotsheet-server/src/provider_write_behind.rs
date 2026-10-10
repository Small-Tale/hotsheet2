//! Explicit, durable admission for experimental Jira field edits.

use super::*;
use hotsheet_ticketing::ProviderError;
use hotsheet_ticketing::provider_outbox::{DispatchState, OutboxOperation, project_pending_ticket};
use serde_json::{Map, Value};

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
    state: DispatchState,
    ticket: ApiTicket,
}

#[derive(Serialize)]
pub(super) struct OutboxStatus {
    operation_id: String,
    connection_id: String,
    native_id: String,
    state: DispatchState,
    attempts: i64,
    next_attempt_at: i64,
    last_error: Option<String>,
    conflict: Option<Value>,
}

impl From<OutboxOperation> for OutboxStatus {
    fn from(operation: OutboxOperation) -> Self {
        Self {
            operation_id: operation.operation_id,
            connection_id: operation.connection_id,
            native_id: operation.native_id,
            state: operation.dispatch_state,
            attempts: operation.attempts,
            next_attempt_at: operation.next_attempt_at,
            last_error: operation.last_error,
            conflict: operation.conflict,
        }
    }
}

pub(super) async fn list_jira_operations(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<Vec<OutboxStatus>>, ApiError> {
    let provider = provider_for(&state, &connection_id)?;
    if !provider.descriptor().capabilities.write_behind {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "write-behind is unavailable for this connection",
        ));
    }
    let outbox = state
        .jira_outbox
        .as_ref()
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "write-behind is disabled"))?;
    let operations = outbox
        .lock()
        .map_err(lock_error)?
        .recent_for_connection(&connection_id, 1_000)
        .map_err(outbox_error)?;
    Ok(Json(
        operations.into_iter().map(OutboxStatus::from).collect(),
    ))
}

pub(super) async fn retry_jira_operation(
    State(state): State<AppState>,
    Path((connection_id, operation_id)): Path<(String, String)>,
) -> Result<Json<OutboxStatus>, ApiError> {
    let _provider = provider_for(&state, &connection_id)?;
    let outbox = state
        .jira_outbox
        .as_ref()
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "write-behind is disabled"))?;
    let mut guard = outbox.lock().map_err(lock_error)?;
    let current = guard
        .get(&operation_id)
        .map_err(outbox_error)?
        .filter(|operation| operation.connection_id == connection_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no such operation"))?;
    guard.retry(&operation_id).map_err(outbox_error)?;
    let result = OutboxStatus::from(
        guard
            .get(&operation_id)
            .map_err(outbox_error)?
            .expect("retried row exists"),
    );
    drop(guard);
    emit_operation_event(&state, &connection_id, &current.native_id);
    Ok(Json(result))
}

pub(super) async fn discard_jira_operation(
    State(state): State<AppState>,
    Path((connection_id, operation_id)): Path<(String, String)>,
) -> Result<Json<OutboxStatus>, ApiError> {
    let provider = provider_for(&state, &connection_id)?;
    let outbox = state
        .jira_outbox
        .as_ref()
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "write-behind is disabled"))?;
    let current = outbox
        .lock()
        .map_err(lock_error)?
        .get(&operation_id)
        .map_err(outbox_error)?
        .filter(|operation| operation.connection_id == connection_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no such operation"))?;
    if current.dispatch_state == DispatchState::Sending {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "sending operations cannot be discarded",
        ));
    }
    // An operation never claimed by the dispatcher cannot have reached Jira. Once
    // a send was attempted, read back before discarding because a timeout may
    // have happened after Jira applied the write.
    let already_applied = if current.attempts == 0 {
        false
    } else {
        let remote = provider
            .get(&current.native_id)
            .map_err(provider_transfer_error)?;
        let remote = serde_json::to_value(remote).map_err(internal_json_error)?;
        let desired = patch_fields(&current.patch)?;
        desired
            .iter()
            .all(|(field, value)| remote.get(field) == Some(value))
    };
    let mut guard = outbox.lock().map_err(lock_error)?;
    if already_applied {
        guard.settle_applied(&operation_id).map_err(outbox_error)?;
    } else {
        guard.discard(&operation_id).map_err(outbox_error)?;
    }
    let result = OutboxStatus::from(
        guard
            .get(&operation_id)
            .map_err(outbox_error)?
            .expect("settled row exists"),
    );
    drop(guard);
    emit_operation_event(&state, &connection_id, &current.native_id);
    Ok(Json(result))
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
    drop(guard);
    let mut results = Vec::with_capacity(admitted.len());
    for operation in admitted {
        let pending = outbox
            .lock()
            .map_err(lock_error)?
            .pending_for_ticket(&operation.connection_id, &operation.native_id)
            .map_err(outbox_error)?;
        let base = pending.first().map_or_else(
            || operation.base_ticket.clone(),
            |first| first.base_ticket.clone(),
        );
        let projected = if pending.is_empty() {
            serde_json::to_value(
                provider
                    .get(&operation.native_id)
                    .map_err(provider_transfer_error)?,
            )
            .map_err(internal_json_error)?
        } else {
            project_pending_ticket(&base, &pending).map_err(outbox_error)?
        };
        results.push(QueuedProviderResult {
            operation_id: operation.operation_id,
            state: operation.dispatch_state,
            ticket: serde_json::from_value(projected).map_err(internal_json_error)?,
        });
    }
    for admission in &admissions {
        emit_operation_event(&state, &admission.connection_id, &admission.native_id);
    }
    Ok((StatusCode::ACCEPTED, Json(results)))
}

fn emit_operation_event(state: &AppState, connection_id: &str, native_id: &str) {
    state.emit(ChangeEvent {
        cursor: None,
        store: connection_id.into(),
        kind: "updated".into(),
        id: native_id.into(),
        slug: format!("{connection_id}:{native_id}"),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// One bounded dispatch pass. The server loop calls it repeatedly; tests can drive it
/// deterministically against fake Jira transports without sleeping for the interval.
pub async fn dispatch_jira_once(state: &AppState) -> Result<usize, ApiError> {
    dispatch_jira_at(state, OffsetDateTime::now_utc().unix_timestamp()).await
}

pub async fn dispatch_jira_at(state: &AppState, now: i64) -> Result<usize, ApiError> {
    let Some(outbox) = &state.jira_outbox else {
        return Ok(0);
    };
    let claimed = outbox
        .lock()
        .map_err(lock_error)?
        .claim_ready(now, 4)
        .map_err(outbox_error)?;
    for operation in &claimed {
        emit_operation_event(state, &operation.connection_id, &operation.native_id);
    }
    let count = claimed.len();
    let jobs = claimed.into_iter().map(|operation| {
        let state = state.clone();
        tokio::task::spawn_blocking(move || dispatch_one(&state, operation, now))
    });
    for result in futures_util::future::join_all(jobs).await {
        result.map_err(|error| {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        })??;
    }
    Ok(count)
}

pub fn spawn_jira_dispatch_loop(state: AppState) -> Option<tokio::task::JoinHandle<()>> {
    state.jira_outbox.as_ref()?;
    Some(tokio::spawn(async move {
        loop {
            if let Err(error) = dispatch_jira_once(&state).await {
                eprintln!("Jira outbox dispatch failed: {}", error.message);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }))
}

fn patch_fields(patch: &ProviderPatch) -> Result<Map<String, Value>, ApiError> {
    let value = serde_json::to_value(patch).map_err(internal_json_error)?;
    let mut fields = Map::new();
    for key in ["title", "details", "category", "priority", "tags"] {
        if let Some(value) = value.get(key).filter(|value| !value.is_null()) {
            fields.insert(key.into(), value.clone());
        }
    }
    Ok(fields)
}

fn dispatch_one(
    state: &AppState,
    operation: OutboxOperation,
    now_epoch: i64,
) -> Result<(), ApiError> {
    let outbox = state
        .jira_outbox
        .as_ref()
        .expect("claimed outbox operation");
    let outcome = (|| -> Result<(), ProviderError> {
        let provider = provider_for(state, &operation.connection_id).map_err(|error| {
            ProviderError::Conflict {
                ticket: operation.native_id.clone(),
                message: error.message,
            }
        })?;
        let remote = provider.get(&operation.native_id)?;
        let desired = patch_fields(&operation.patch).map_err(|error| ProviderError::Conflict {
            ticket: operation.native_id.clone(),
            message: error.message,
        })?;
        let remote_value = serde_json::to_value(&remote).expect("ApiTicket serializes");
        if desired
            .iter()
            .all(|(field, value)| remote_value.get(field) == Some(value))
        {
            outbox
                .lock()
                .map_err(|_| ProviderError::Conflict {
                    ticket: operation.native_id.clone(),
                    message: "outbox lock poisoned".into(),
                })?
                .confirm(&operation.operation_id)
                .map_err(|error| ProviderError::Conflict {
                    ticket: operation.native_id.clone(),
                    message: error.to_string(),
                })?;
            return Ok(());
        }
        let mut conflict = Map::new();
        for (field, desired_value) in &desired {
            let current = remote_value.get(field).cloned().unwrap_or(Value::Null);
            if operation.base_ticket.get(field) != Some(&current) && &current != desired_value {
                conflict.insert(field.clone(), current);
            }
        }
        if !conflict.is_empty() {
            outbox
                .lock()
                .map_err(|_| ProviderError::Conflict {
                    ticket: operation.native_id.clone(),
                    message: "outbox lock poisoned".into(),
                })?
                .record_conflict(&operation.operation_id, &Value::Object(conflict))
                .map_err(|error| ProviderError::Conflict {
                    ticket: operation.native_id.clone(),
                    message: error.to_string(),
                })?;
        }
        let mut patch = operation.patch.clone();
        patch.expected_token = remote.concurrency_token.clone();
        let written = provider.update(&operation.native_id, now(), patch)?;
        let written_value = serde_json::to_value(&written).expect("ApiTicket serializes");
        if !desired
            .iter()
            .all(|(field, value)| written_value.get(field) == Some(value))
        {
            outbox
                .lock()
                .map_err(|_| ProviderError::Conflict {
                    ticket: operation.native_id.clone(),
                    message: "outbox lock poisoned".into(),
                })?
                .needs_attention(
                    &operation.operation_id,
                    "provider acknowledgement did not contain local intent",
                    Some(&written_value),
                )
                .map_err(|error| ProviderError::Conflict {
                    ticket: operation.native_id.clone(),
                    message: error.to_string(),
                })?;
            return Ok(());
        }
        outbox
            .lock()
            .map_err(|_| ProviderError::Conflict {
                ticket: operation.native_id.clone(),
                message: "outbox lock poisoned".into(),
            })?
            .confirm(&operation.operation_id)
            .map_err(|error| ProviderError::Conflict {
                ticket: operation.native_id.clone(),
                message: error.to_string(),
            })?;
        Ok(())
    })();
    if let Err(error) = outcome {
        let epoch = now_epoch;
        let mut guard = outbox.lock().map_err(lock_error)?;
        match error {
            ProviderError::RateLimited {
                retry_after_seconds,
                ..
            } => {
                let until = epoch.saturating_add(retry_after_seconds.unwrap_or(30).max(1) as i64);
                guard
                    .defer(
                        &operation.operation_id,
                        until,
                        "provider rate limited",
                        true,
                    )
                    .map_err(outbox_error)?;
                guard
                    .defer_connection(&operation.connection_id, until)
                    .map_err(outbox_error)?;
            }
            ProviderError::Authentication { .. }
            | ProviderError::Disabled { .. }
            | ProviderError::NotFound { .. }
            | ProviderError::Unsupported { .. } => {
                guard
                    .needs_attention(&operation.operation_id, &error.to_string(), None)
                    .map_err(outbox_error)?;
            }
            // A manual retry starts another eight-attempt window without erasing
            // the lifetime send count used for uncertain-write readback.
            _ if operation.attempts % 8 == 0 => {
                guard
                    .needs_attention(&operation.operation_id, &error.to_string(), None)
                    .map_err(outbox_error)?;
            }
            _ => {
                let delay = (1_i64 << operation.attempts.min(6))
                    + i64::from(operation.operation_id.as_bytes()[0] % 3);
                guard
                    .defer(
                        &operation.operation_id,
                        epoch.saturating_add(delay),
                        &error.to_string(),
                        false,
                    )
                    .map_err(outbox_error)?;
            }
        }
    }
    emit_operation_event(state, &operation.connection_id, &operation.native_id);
    Ok(())
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
        OutboxError::ChangedPayload(_)
        | OutboxError::ExpiredOperation(_)
        | OutboxError::MismatchedBase => StatusCode::CONFLICT,
        OutboxError::EmptyIdentity
        | OutboxError::InvalidOperationId(_)
        | OutboxError::InvalidProjectionOrder(_)
        | OutboxError::UnsupportedProjectionField { .. }
        | OutboxError::InvalidProjectionTicket
        | OutboxError::InvalidCapacity => StatusCode::BAD_REQUEST,
        OutboxError::InvalidTransition(_) => StatusCode::CONFLICT,
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
