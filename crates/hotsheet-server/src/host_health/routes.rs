//! `GET /host-health` and `GET`/`PUT /host-health/thresholds` (HS2-DTAVQA).

use super::HostHealthSnapshot;
use crate::AppState;
use crate::api_error::ApiError;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::host_health::{
    ConfiguredHostHealthThresholds, HostHealthThresholds, THRESHOLDS, default_thresholds,
    effective_thresholds, setting_key, stored_threshold, validate_thresholds,
};
use hotsheet_ticketing::{Scope, Settings};
use serde::Serialize;

/// The machine-wide thresholds: defaults, explicit settings (`null` = default), and effective.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub(crate) struct HostHealthThresholdsView {
    pub(crate) defaults: HostHealthThresholds,
    pub(crate) configured: ConfiguredHostHealthThresholds,
    pub(crate) effective: HostHealthThresholds,
}

fn machine_settings(state: &AppState) -> Settings {
    Settings::with_global_home(state.store.root(), state.machine_home())
}

fn settings_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
}

fn join_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

/// Read the stored thresholds. A stored value outside its range is ignored (default applies).
pub(crate) fn read_thresholds(state: &AppState) -> Result<HostHealthThresholdsView, ApiError> {
    let settings = machine_settings(state);
    let mut configured = ConfiguredHostHealthThresholds::default();
    for spec in &THRESHOLDS {
        let key = setting_key(spec.name);
        let value = settings.get(&key, Scope::Global).map_err(settings_error)?;
        configured.set(spec.name, stored_threshold(&key, value.as_ref()));
    }
    Ok(HostHealthThresholdsView {
        defaults: default_thresholds(),
        configured,
        effective: effective_thresholds(configured),
    })
}

/// `GET /host-health`: the latest evaluated sample of the server's host.
pub(crate) async fn get_host_health(State(state): State<AppState>) -> Json<HostHealthSnapshot> {
    Json(
        state
            .host_health
            .monitor
            .with_lock(|monitor| monitor.snapshot().clone()),
    )
}

pub(crate) async fn get_host_health_thresholds(
    State(state): State<AppState>,
) -> Result<Json<HostHealthThresholdsView>, ApiError> {
    let view = tokio::task::spawn_blocking(move || read_thresholds(&state))
        .await
        .map_err(join_error)??;
    Ok(Json(view))
}

/// Replace every threshold: a `null` or missing field clears it back to its default. The next
/// sample applies the new values; a CLI `settings set` takes effect the same way.
pub(crate) async fn put_host_health_thresholds(
    State(state): State<AppState>,
    Json(value): Json<ConfiguredHostHealthThresholds>,
) -> Result<Json<HostHealthThresholdsView>, ApiError> {
    validate_thresholds(&value)
        .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    let view = tokio::task::spawn_blocking(move || {
        let settings = machine_settings(&state);
        for spec in &THRESHOLDS {
            let key = setting_key(spec.name);
            match value.get(spec.name) {
                Some(limit) => settings
                    .set(&key, serde_json::Value::from(limit), Scope::Global)
                    .map_err(settings_error)?,
                None => {
                    settings
                        .unset(&key, Scope::Global)
                        .map_err(settings_error)?;
                }
            }
        }
        read_thresholds(&state)
    })
    .await
    .map_err(join_error)??;
    Ok(Json(view))
}
