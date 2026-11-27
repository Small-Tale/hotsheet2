//! HS2-1Y45MZ: authenticated pairing identity, path-free discovery and local registry.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_cli::remote_servers::machine::{MachineError, MachineQuery};
use hotsheet_cli::remote_servers::{PeerSession, RemoteRuntime};
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
        let identity = hotsheet_ticketing::remote_servers::server_identity(&home, || {
            ulid::Ulid::new().to_string()
        })?;
        // HS2-HGG426: name this machine so a paired client can label its project tabs.
        Ok(RemoteIdentity {
            host_name: crate::host_health::host_name().filter(|name| {
                name.len() <= hotsheet_ticketing::remote_servers::MAX_REMOTE_HOST_NAME
                    && !name.chars().any(char::is_control)
            }),
            ..identity
        })
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
                    resolution_metadata_version: Some(
                        hotsheet_ticketing::orchestration::ORCHESTRATION_VERSION,
                    ),
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

/// HS2-SW03MK: a machine read's refusal. Registry errors keep their statuses; a binding
/// that changed or a remote with no project to read capacity through conflicts, and a
/// failed or refused peer read is a bad gateway that keeps only the peer's status and code.
fn machine_error(error: MachineError) -> ApiError {
    match error {
        MachineError::Remote(error) => self::error(error),
        MachineError::Invalid(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, message).with_code("invalid_request")
        }
        MachineError::Changed => {
            ApiError::new(StatusCode::CONFLICT, MachineError::Changed.to_string())
                .with_code("remote_changed")
        }
        MachineError::NoCheckout => {
            ApiError::new(StatusCode::CONFLICT, MachineError::NoCheckout.to_string())
                .with_code("no_available_checkout")
        }
        other @ (MachineError::Transport(_) | MachineError::Refused { .. }) => {
            ApiError::new(StatusCode::BAD_GATEWAY, other.to_string())
        }
    }
}

/// HS2-SW03MK: `POST /remotes/{id}/machine`, the machine of paired remote `id` with no open
/// project, as `hotsheet-cli remote machine` reads it (HS2-VQ92XZ). The body names the
/// readings ([`MachineQuery`]); `{}` reads all three. Loopback-only, as every `/remotes`
/// route is, and it answers readings only, never a session or credential.
pub(crate) async fn machine(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let query: MachineQuery = body(value)?;
    let (reads, capacity_optional) = query.reads().map_err(machine_error)?;
    let runtime = runtime(&state);
    tokio::task::spawn_blocking(move || runtime.machine(&id, &reads, capacity_optional))
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "remote machine read task failed",
            )
        })?
        .map(Json)
        .map_err(machine_error)
}

/// HS2-SW03MK: `GET /remotes/{id}/session`, this machine's paired session on remote `id`,
/// as `hotsheet-cli remote session` reads it (HS2-4B0KSD).
pub(crate) async fn session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PeerSession>, ApiError> {
    let runtime = runtime(&state);
    blocking(move || runtime.peer_session(&id, false)).await
}

/// HS2-SW03MK: `POST /remotes/{id}/renew` asks remote `id`'s host to renew this machine's
/// paired session in place, as `hotsheet-cli remote renew` does (HS2-4B0KSD).
pub(crate) async fn renew(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PeerSession>, ApiError> {
    let runtime = runtime(&state);
    blocking(move || runtime.peer_session(&id, true)).await
}
