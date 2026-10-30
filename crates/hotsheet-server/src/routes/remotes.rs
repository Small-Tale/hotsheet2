//! HS2-1Y45MZ: authenticated pairing identity, path-free discovery and local registry.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_cli::remote_servers::RemoteRuntime;
use hotsheet_ticketing::remote_servers::{
    MAX_REMOTE_CHECKOUTS, RegisterRemote, RemoteCheckout, RemoteCheckoutState, RemoteDiscovery,
    RemoteError, RemoteIdentity, RemoteSource, RemoteView, ReviewRemote, RevokeRemote,
};

use crate::AppState;
use crate::api_error::ApiError;

fn runtime(state: &AppState) -> Arc<RemoteRuntime> {
    state
        .remote_runtime
        .clone()
        .unwrap_or_else(|| Arc::new(RemoteRuntime::live(state.machine_home())))
}

fn error(error: RemoteError) -> ApiError {
    let status = match error {
        RemoteError::Invalid(_) => StatusCode::BAD_REQUEST,
        RemoteError::Conflict | RemoteError::Revoked => StatusCode::CONFLICT,
        RemoteError::NotFound => StatusCode::NOT_FOUND,
        RemoteError::UnsupportedVersion => StatusCode::UPGRADE_REQUIRED,
        RemoteError::Verification(_) => StatusCode::BAD_GATEWAY,
        RemoteError::Credentials => StatusCode::SERVICE_UNAVAILABLE,
        RemoteError::Storage(_) | RemoteError::Corrupt => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.to_string())
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, RemoteError> + Send + 'static,
) -> Result<Json<T>, ApiError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "remote registry task failed",
            )
        })?
        .map(Json)
        .map_err(error)
}

pub(crate) async fn identity(
    State(state): State<AppState>,
) -> Result<Json<RemoteIdentity>, ApiError> {
    let home = state.machine_home().to_path_buf();
    blocking(move || {
        hotsheet_ticketing::remote_servers::server_identity(&home, || ulid::Ulid::new().to_string())
    })
    .await
}

pub(crate) async fn hosted_checkouts(
    State(state): State<AppState>,
) -> Result<Json<Vec<RemoteCheckout>>, ApiError> {
    let registry = state.checkout_registry.clone();
    tokio::task::spawn_blocking(move || {
        let checkouts = registry.list().map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "checkout registry unavailable",
            )
        })?;
        if checkouts.len() > MAX_REMOTE_CHECKOUTS {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "remote checkout discovery limit exceeded",
            ));
        }
        let values: Vec<_> = checkouts
            .into_iter()
            .map(|checkout| {
                let state = if !std::path::Path::new(&checkout.root).is_dir() {
                    RemoteCheckoutState::Inaccessible
                } else {
                    match registry.resolve(&checkout.id) {
                        Ok(_) => RemoteCheckoutState::Available,
                        Err(hotsheet_ticketing::checkouts::CheckoutError::Invalid(_)) => {
                            RemoteCheckoutState::IdentityChanged
                        }
                        Err(_) => RemoteCheckoutState::Inaccessible,
                    }
                };
                RemoteCheckout {
                    id: checkout.id,
                    name: checkout.alias,
                    sources: checkout
                        .sources
                        .into_iter()
                        .map(|source| RemoteSource {
                            connection_id: source.connection_id,
                            provider: source.provider,
                        })
                        .collect(),
                    state,
                }
            })
            .collect();
        hotsheet_ticketing::remote_servers::validate_remote_checkouts(&values).map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid remote checkout discovery",
            )
        })?;
        Ok(Json(values))
    })
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "checkout discovery task failed",
        )
    })?
}

pub(crate) async fn list(State(state): State<AppState>) -> Result<Json<Vec<RemoteView>>, ApiError> {
    let runtime = runtime(&state);
    blocking(move || runtime.list()).await
}

// Actor data belongs to request attribution, not a credential bundle or registry identity.
fn body<T: serde::de::DeserializeOwned>(mut value: serde_json::Value) -> Result<T, ApiError> {
    if let Some(object) = value.as_object_mut() {
        object.remove("actor");
    }
    serde_json::from_value(value)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid remote registry request"))
}

pub(crate) async fn register(
    State(state): State<AppState>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<RemoteDiscovery>, ApiError> {
    let request: RegisterRemote = body(value)?;
    let runtime = runtime(&state);
    blocking(move || runtime.register(request)).await
}

pub(crate) async fn review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<RemoteDiscovery>, ApiError> {
    let request: ReviewRemote = body(value)?;
    let runtime = runtime(&state);
    blocking(move || runtime.review(&id, request)).await
}

pub(crate) async fn revoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<RemoteView>, ApiError> {
    let request: RevokeRemote = body(value)?;
    let runtime = runtime(&state);
    blocking(move || runtime.revoke(&id, request.expected_revision)).await
}

pub(crate) async fn discover(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RemoteDiscovery>, ApiError> {
    let runtime = runtime(&state);
    blocking(move || runtime.discover(&id)).await
}
