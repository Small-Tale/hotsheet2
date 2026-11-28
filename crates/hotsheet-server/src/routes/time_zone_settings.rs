//! `GET`/`PUT /checkouts/{reference}/time-zone-settings` (HS2-SZF152): the project's shared
//! `time_zone`, which Project Settings › General shows as Time Zone and every due-date
//! deadline uses (docs/38-milestones.md §38.3). The headless twin is
//! `hotsheet-cli settings set time_zone <zone>`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_ticketing::project_time_zone::{self, ProjectTimeZone};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api_error::ApiError;
use crate::routes::project_tools::checkout_settings;

/// The zone in effect; `invalid_value` names a hand-edited value that reads as `UTC`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct TimeZoneSettingsView {
    time_zone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    invalid_value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic: Option<String>,
}

impl From<ProjectTimeZone> for TimeZoneSettingsView {
    fn from(zone: ProjectTimeZone) -> Self {
        Self {
            diagnostic: zone.diagnostic(),
            time_zone: zone.name,
            invalid_value: zone.invalid_value,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TimeZoneSettingsWrite {
    time_zone: serde_json::Value,
}

fn settings_error(error: hotsheet_ticketing::SettingsError) -> ApiError {
    let code = error.code();
    let api = ApiError::new(StatusCode::BAD_REQUEST, error.to_string());
    match code {
        Some(code) => api.with_code(code),
        None => api,
    }
}

pub(crate) async fn get_checkout_time_zone_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<TimeZoneSettingsView>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let zone = project_time_zone::project_time_zone(&settings).map_err(settings_error)?;
    Ok(Json(zone.into()))
}

pub(crate) async fn put_checkout_time_zone_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(value): Json<TimeZoneSettingsWrite>,
) -> Result<Json<TimeZoneSettingsView>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            project_time_zone::TIME_ZONE_SETTING,
            value.time_zone,
            hotsheet_ticketing::Scope::Shared,
        )
        .map_err(settings_error)?;
    let zone = project_time_zone::project_time_zone(&settings).map_err(settings_error)?;
    Ok(Json(zone.into()))
}
