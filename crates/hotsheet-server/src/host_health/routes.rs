//! `GET /host-health` and `GET`/`PUT /host-health/thresholds` (HS2-DTAVQA).

use super::{Emission, HostHealthSnapshot};
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
use hotsheet_ticketing::host_health_disks::{
    DISKS_SETTING_KEY, DiskChoice, DiskChoices, STORE_VOLUME_ID, stored_choices,
};
use hotsheet_ticketing::{Scope, Settings};
use serde::{Deserialize, Serialize};

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

/// Read the saved disk choices (HS2-NHK8V8).
pub(crate) fn read_disk_choices(state: &AppState) -> Result<DiskChoices, ApiError> {
    let value = machine_settings(state)
        .get(DISKS_SETTING_KEY, Scope::Global)
        .map_err(settings_error)?;
    Ok(stored_choices(value.as_ref()))
}

fn write_disk_choices(state: &AppState, choices: &DiskChoices) -> Result<(), ApiError> {
    let settings = machine_settings(state);
    if choices.is_empty() {
        settings
            .unset(DISKS_SETTING_KEY, Scope::Global)
            .map_err(settings_error)?;
    } else {
        let value = serde_json::to_value(choices).map_err(join_error)?;
        settings
            .set(DISKS_SETTING_KEY, value, Scope::Global)
            .map_err(settings_error)?;
    }
    Ok(())
}

/// Apply saved choices to the snapshot at once and tell clients to re-read it.
fn publish_choices(state: &AppState, choices: DiskChoices) -> HostHealthSnapshot {
    let snapshot = state.host_health.monitor.with_lock(|monitor| {
        monitor.set_disk_choices(choices);
        monitor.snapshot().clone()
    });
    super::emit_event(
        state,
        &Emission {
            id: "disks",
            status: "changed",
            reason: None,
        },
    );
    snapshot
}

#[derive(Debug, Deserialize)]
pub(crate) struct DiskChoiceBody {
    id: String,
    monitor: bool,
}

/// `PUT /host-health/disks`: include or ignore one known volume (HS2-NHK8V8). The choice is
/// a machine-wide setting keyed by the volume id, so it returns with a reconnected drive. The
/// store's volume is always monitored.
pub(crate) async fn put_host_health_disk(
    State(state): State<AppState>,
    Json(body): Json<DiskChoiceBody>,
) -> Result<Json<HostHealthSnapshot>, ApiError> {
    let id = body.id;
    if id == STORE_VOLUME_ID {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "the ticket store's disk is always monitored",
        ));
    }
    let known = state.host_health.monitor.with_lock(|monitor| {
        monitor
            .snapshot()
            .disks
            .iter()
            .find(|disk| disk.id == id)
            .map(|disk| {
                (
                    disk.label.clone(),
                    disk.mount.clone().unwrap_or_default(),
                    disk.manual,
                )
            })
    });
    let Some((label, mount, manual)) = known else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "unknown disk"));
    };
    let choices = tokio::task::spawn_blocking(move || {
        let mut choices = read_disk_choices(&state)?;
        choices.insert(
            id,
            DiskChoice {
                monitor: body.monitor,
                label,
                mount,
                manual,
            },
        );
        write_disk_choices(&state, &choices)?;
        Ok::<_, ApiError>((state, choices))
    })
    .await
    .map_err(join_error)??;
    let (state, choices) = choices;
    Ok(Json(publish_choices(&state, choices)))
}
