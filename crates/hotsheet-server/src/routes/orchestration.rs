//! HS2-4J6V46: authenticated adapters over the one shared target runtime.
use crate::{AppState, api_error::ApiError};
use axum::{
    Json,
    extract::{OriginalUri, State},
    http::{Method, StatusCode},
};
use hotsheet_cli::orchestration::{OrchestrationRuntime, error_status, request_cleanup_now};
use hotsheet_ticketing::{ProviderConfigRegistry, orchestration::OrchestrationError};
use serde_json::Value;
use std::sync::Arc;

fn runtime(state: &AppState) -> Result<Arc<OrchestrationRuntime>, OrchestrationError> {
    if let Some(runtime) = &state.orchestration_runtime {
        return Ok(runtime.clone());
    }
    OrchestrationRuntime::live(
        state.machine_home(),
        state.checkout_registry.clone(),
        ProviderConfigRegistry::new(state.store.root().join("providers.json")),
    )
    .map(Arc::new)
}
fn error(error: OrchestrationError) -> ApiError {
    ApiError::new(
        StatusCode::from_u16(error_status(&error)).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        error.to_string(),
    )
    .with_code(error.code())
}
async fn execute(
    state: AppState,
    method: Method,
    path: String,
    body: Value,
) -> Result<Json<Value>, ApiError> {
    tokio::task::spawn_blocking(move || {
        // Must precede runtime/identity/provider construction, including for an orphan
        // owner that the normal checkout router can no longer resolve.
        if let Some(result) =
            request_cleanup_now(state.machine_home(), method.as_str(), &path, &body)
        {
            return result;
        }
        runtime(&state)?.request_now(method.as_str(), &path, &body)
    })
    .await
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "target task failed"))?
    .map(Json)
    .map_err(error)
}
pub(crate) async fn read(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
) -> Result<Json<Value>, ApiError> {
    execute(state, Method::GET, uri.path().into(), Value::Null).await
}
pub(crate) async fn write(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    execute(state, method, uri.path().into(), body).await
}
