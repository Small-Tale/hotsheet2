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
    DiskChoiceError, DiskChoices, VolumeIds, add_disk, detected_volumes, forget_disk, load_choices,
    save_choices, set_monitor, space,
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
    load_choices(&machine_settings(state)).map_err(settings_error)
}

fn write_disk_choices(state: &AppState, choices: &DiskChoices) -> Result<(), ApiError> {
    save_choices(&machine_settings(state), choices).map_err(settings_error)
}

fn choice_error(error: DiskChoiceError) -> ApiError {
    let status = match error {
        DiskChoiceError::NotFound(_) | DiskChoiceError::UnknownDisk(_) => StatusCode::NOT_FOUND,
        DiskChoiceError::StoreVolume => StatusCode::CONFLICT,
    };
    ApiError::new(status, error.to_string())
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

/// `PUT /host-health/disks`: include or ignore one known volume (HS2-NHK8V8). The choice is a
/// machine-wide setting keyed by the volume id, so it returns with a reconnected drive. The store's
/// volume is always monitored (409); an id that is neither present nor saved is 404.
pub(crate) async fn put_host_health_disk(
    State(state): State<AppState>,
    Json(body): Json<DiskChoiceBody>,
) -> Result<Json<HostHealthSnapshot>, ApiError> {
    let present: Vec<(String, String, String)> = state.host_health.monitor.with_lock(|monitor| {
        monitor
            .snapshot()
            .disks
            .iter()
            .filter(|disk| disk.present && !disk.store)
            .map(|disk| {
                (
                    disk.id.clone(),
                    disk.label.clone(),
                    disk.mount.clone().unwrap_or_default(),
                )
            })
            .collect()
    });
    let (state, choices) = tokio::task::spawn_blocking(move || {
        let mut choices = read_disk_choices(&state)?;
        set_monitor(&mut choices, &body.id, body.monitor, &present).map_err(choice_error)?;
        write_disk_choices(&state, &choices)?;
        Ok::<_, ApiError>((state, choices))
    })
    .await
    .map_err(join_error)??;
    Ok(Json(publish_choices(&state, choices)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct AddDiskBody {
    path: String,
}

/// `POST /host-health/disks`: monitor the volume holding a path (HS2-KBVZV1), the same workflow as
/// `hotsheet-cli host-disks add`. It covers mounts detection skips, such as a network share. A path
/// on no volume is 404; the store's volume is 409 because it is always monitored.
pub(crate) async fn post_host_health_disk(
    State(state): State<AppState>,
    Json(body): Json<AddDiskBody>,
) -> Result<Json<HostHealthSnapshot>, ApiError> {
    let (state, choices) = tokio::task::spawn_blocking(move || {
        let store_root = state.store.root().to_path_buf();
        let store = space(&store_root).map(|(_, _, device)| device);
        let detected: Vec<_> = detected_volumes(&mut VolumeIds::default())
            .into_iter()
            .filter(|volume| Some(volume.device) != store)
            .collect();
        let mut choices = read_disk_choices(&state)?;
        add_disk(
            &mut choices,
            std::path::Path::new(&body.path),
            &store_root,
            &detected,
        )
        .map_err(choice_error)?;
        write_disk_choices(&state, &choices)?;
        Ok::<_, ApiError>((state, choices))
    })
    .await
    .map_err(join_error)??;
    Ok(Json(publish_choices(&state, choices)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ForgetDiskParams {
    id: String,
}

/// `DELETE /host-health/disks?id=`: forget a saved choice (HS2-KBVZV1); a still-connected
/// detected disk asks again. Forgetting an unknown id changes nothing.
pub(crate) async fn delete_host_health_disk(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<ForgetDiskParams>,
) -> Result<Json<HostHealthSnapshot>, ApiError> {
    let (state, choices) = tokio::task::spawn_blocking(move || {
        let mut choices = read_disk_choices(&state)?;
        if forget_disk(&mut choices, &params.id) {
            write_disk_choices(&state, &choices)?;
        }
        Ok::<_, ApiError>((state, choices))
    })
    .await
    .map_err(join_error)??;
    Ok(Json(publish_choices(&state, choices)))
}
