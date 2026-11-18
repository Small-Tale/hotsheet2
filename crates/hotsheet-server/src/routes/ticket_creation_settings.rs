//! `GET`/`PUT /checkouts/{reference}/ticket-creation-settings` (HS2-FN018Q): the project's
//! shared "group new tickets automatically" setting, which Project Settings › General shows.
//! The headless twin is `hotsheet-cli settings set auto_group_new_tickets <true|false>`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api_error::ApiError;
use crate::routes::project_tools::checkout_settings;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TicketCreationSettings {
    auto_group_new_tickets: bool,
}

pub(crate) async fn get_checkout_ticket_creation_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<TicketCreationSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let auto_group_new_tickets = settings
        .auto_group_new_tickets()
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(TicketCreationSettings {
        auto_group_new_tickets,
    }))
}

pub(crate) async fn put_checkout_ticket_creation_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(value): Json<TicketCreationSettings>,
) -> Result<Json<TicketCreationSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            hotsheet_ticketing::AUTO_GROUP_NEW_TICKETS_SETTING,
            serde_json::json!(value.auto_group_new_tickets),
            hotsheet_ticketing::Scope::Shared,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(value))
}
