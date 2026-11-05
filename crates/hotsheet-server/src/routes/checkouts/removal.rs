//! `DELETE /checkouts/{reference}` — remove a registered checkout (HS2-X4RZ73).
//!
//! The route and `hotsheet-cli checkout remove` share
//! [`hotsheet_ticketing::checkout_removal::remove_checkout`]. The server adds the one check
//! only it can make: a project a client has open (a live presence lease) is refused, so a
//! visible project never loses its registry entry underneath its window.

use crate::AppState;
use crate::api_error::ApiError;
use crate::routes::providers::provider_transfer_error;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hotsheet_ticketing::ProviderConfigRegistry;
use hotsheet_ticketing::checkout_removal::{CheckoutRemoval, remove_checkout};
use hotsheet_ticketing::checkouts::CheckoutError;
use hotsheet_ticketing::connection_removal::ConnectionRemovalError;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RemoveCheckoutParams {
    #[serde(default)]
    dry_run: bool,
}

pub(crate) async fn remove_checkout_route(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(params): Query<RemoveCheckoutParams>,
) -> Result<Json<CheckoutRemoval>, ApiError> {
    let working = state.clone();
    let report = tokio::task::spawn_blocking(move || {
        let state = working;
        let target = state
            .checkout_registry
            .find_for_removal(&reference)
            .map_err(lookup_error)?;
        if let Some(checkout) = &target
            && state.presence.live().checkouts.contains(&checkout.id)
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!(
                    "project {} ({}) is open in a Hot Sheet window; close it there, then remove it",
                    checkout.alias, checkout.id
                ),
            ));
        }
        let report = remove_checkout(
            &ProviderConfigRegistry::new(state.store.root().join("providers.json")),
            &state.checkout_registry,
            &reference,
            params.dry_run,
        )
        .map_err(|error| match error {
            ConnectionRemovalError::Checkout(error) => lookup_error(error),
            ConnectionRemovalError::Provider(error) => provider_transfer_error(error),
            other => ApiError::new(StatusCode::BAD_REQUEST, other.to_string()),
        })?;
        for connection_id in &report.removed_connections {
            crate::github_webhooks::forget_connection(&state, connection_id);
        }
        Ok::<_, ApiError>(report)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    if report.removed {
        // Its stores and repository monitor stop unless another project or live work needs them.
        state.request_unhost_sweep(Duration::ZERO);
    }
    Ok(Json(report))
}

fn lookup_error(error: CheckoutError) -> ApiError {
    let status = match error {
        CheckoutError::Ambiguous(_) => StatusCode::CONFLICT,
        CheckoutError::Invalid(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.to_string())
}
