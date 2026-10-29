//! Notifier plugin administration (HS2-R5HTZN, `docs/32-notifier-plugins.md` §32.9): the
//! same `hotsheet_notify::admin` workflow `hotsheet-cli notify` runs. Configuration is
//! machine-local; credential values are accepted only by the secret route and are never
//! returned.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hotsheet_notify::admin::{self, AdminError, ChannelRemoval};
use hotsheet_notify::{
    ChannelConfig, DeliveryRecord, NotificationConfig, NotifierDescriptor, Rule, recent_deliveries,
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api_error::ApiError;

fn admin_error(error: AdminError) -> ApiError {
    let status = match error {
        AdminError::NotFound(_) => StatusCode::NOT_FOUND,
        AdminError::Invalid(_) => StatusCode::BAD_REQUEST,
        AdminError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.to_string())
}

/// Run admin work off the async runtime: it reads files and may touch the keychain.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

#[derive(Serialize)]
pub(crate) struct NotifierList {
    notifiers: Vec<NotifierDescriptor>,
    /// Plugin directories that failed to load, with the reason.
    load_errors: Vec<String>,
}

pub(crate) async fn list_notifiers(
    State(state): State<AppState>,
) -> Result<Json<NotifierList>, ApiError> {
    blocking(move || {
        let registry = state.notifier_registry();
        Ok(Json(NotifierList {
            notifiers: registry.descriptors(),
            load_errors: registry.load_errors.clone(),
        }))
    })
    .await
}

pub(crate) async fn get_notification_config(
    State(state): State<AppState>,
) -> Result<Json<NotificationConfig>, ApiError> {
    blocking(move || {
        NotificationConfig::load(state.machine_home())
            .map(Json)
            .map_err(|e| admin_error(e.into()))
    })
    .await
}

/// A channel body; the id comes from the path.
#[derive(Deserialize)]
pub(crate) struct ChannelBody {
    notifier: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default = "enabled")]
    enabled: bool,
    #[serde(default)]
    settings: hotsheet_notify::settings::Settings,
}

fn enabled() -> bool {
    true
}

pub(crate) async fn put_notification_channel(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ChannelBody>,
) -> Result<Json<ChannelConfig>, ApiError> {
    blocking(move || {
        let channel = ChannelConfig {
            id,
            notifier: body.notifier,
            label: body.label,
            enabled: body.enabled,
            settings: body.settings,
            plugin_digest: None,
        };
        admin::put_channel(state.machine_home(), &state.notifier_registry(), channel)
            .map(Json)
            .map_err(admin_error)
    })
    .await
}

pub(crate) async fn delete_notification_channel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ChannelRemoval>, ApiError> {
    blocking(move || {
        admin::remove_channel(
            state.machine_home(),
            &state.notifier_registry(),
            &state.key_registry(),
            &id,
        )
        .map(Json)
        .map_err(admin_error)
    })
    .await
}

#[derive(Deserialize)]
pub(crate) struct SecretBody {
    value: String,
}

pub(crate) async fn put_notification_channel_secret(
    State(state): State<AppState>,
    Path((id, field)): Path<(String, String)>,
    Json(body): Json<SecretBody>,
) -> Result<StatusCode, ApiError> {
    blocking(move || {
        admin::set_channel_secret(
            state.machine_home(),
            &state.notifier_registry(),
            &state.key_registry(),
            &id,
            &field,
            &body.value,
        )
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(admin_error)
    })
    .await
}

pub(crate) async fn test_notification_channel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeliveryRecord>, ApiError> {
    blocking(move || {
        admin::test_channel(state.machine_home(), &state.notification_dispatcher(), &id)
            .map(Json)
            .map_err(admin_error)
    })
    .await
}

/// Approve a channel's external notifier plugin as it is now (HS2-208RB1).
pub(crate) async fn trust_notification_channel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ChannelConfig>, ApiError> {
    blocking(move || {
        admin::trust_channel(state.machine_home(), &state.notifier_registry(), &id)
            .map(Json)
            .map_err(admin_error)
    })
    .await
}

pub(crate) async fn put_notification_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut rule): Json<Rule>,
) -> Result<Json<Rule>, ApiError> {
    rule.id = id;
    blocking(move || {
        admin::put_rule(state.machine_home(), &state.notifier_registry(), rule)
            .map(Json)
            .map_err(admin_error)
    })
    .await
}

pub(crate) async fn delete_notification_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    blocking(move || {
        match admin::remove_rule(state.machine_home(), &state.notifier_registry(), &id) {
            Ok(true) => Ok(StatusCode::NO_CONTENT),
            Ok(false) => Err(ApiError::new(
                StatusCode::NOT_FOUND,
                format!("unknown rule '{id}'"),
            )),
            Err(error) => Err(admin_error(error)),
        }
    })
    .await
}

#[derive(Deserialize)]
pub(crate) struct DeliveryQuery {
    limit: Option<usize>,
}

pub(crate) async fn list_notification_deliveries(
    State(state): State<AppState>,
    Query(q): Query<DeliveryQuery>,
) -> Result<Json<Vec<DeliveryRecord>>, ApiError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    blocking(move || Ok(Json(recent_deliveries(state.machine_home(), limit)))).await
}
