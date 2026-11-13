//! HS2-FMHH9Y: resource-claim routes over the one shared `ClaimService` workflow
//! (`docs/36-resource-claims.md` §36.10).
//!
//! Two authorities reach these handlers. The `/resource-claims/...` routes sit behind the
//! server's ordinary authentication. The `/resource-claim-token/...` routes sit outside
//! it and accept a renewal token as the only credential; they can renew or release that
//! token's own claim and do nothing else.

use std::time::Duration;

use axum::{
    Json,
    body::Bytes,
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
};
use hotsheet_resource_claims::{
    ClaimError, ClaimService, MAX_RENEWAL_TOKEN_LEN, TOKEN_ROUTE_PREFIX, http_status,
};
use serde_json::Value;
use tokio::sync::{Semaphore, oneshot};

use crate::{AppState, api_error::ApiError};

/// The longest one request parks. A client with a later deadline asks again at once, so
/// this is a long poll and never a fixed-interval one.
pub(crate) const MAX_REQUEST_WAIT: Duration = Duration::from_secs(55);
/// Most requests parked on a claim at the same time; each holds one blocking-pool thread.
pub(crate) const MAX_PARKED: usize = 64;

static PARKED: Semaphore = Semaphore::const_new(MAX_PARKED);

fn service(state: &AppState) -> ClaimService {
    ClaimService::new(state.machine_home()).with_max_wait(MAX_REQUEST_WAIT)
}

fn error(error: ClaimError) -> ApiError {
    ApiError::new(
        StatusCode::from_u16(http_status(&error)).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        format!("{}: {error}", error.code()),
    )
    .with_code(error.code())
}

/// `GET /resource-claims/{namespace}` and `GET /resource-claims/{namespace}/{resource}`.
pub(crate) async fn read(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
) -> Result<Json<Value>, ApiError> {
    service(&state)
        .request("GET", uri.path(), &Value::Null)
        .map(Json)
        .map_err(error)
}

/// `POST /resource-claims/{namespace}/{resource}/renew|release`: the renewal token in
/// the body must be for the resource in the path.
pub(crate) async fn write(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    service(&state)
        .request("POST", uri.path(), &body)
        .map(Json)
        .map_err(error)
}

/// `POST /resource-claims/{namespace}/{resource}/acquire|wait`: may park until the
/// backend signals a release or the holder's lease lapses.
pub(crate) async fn park(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let waits = body.get("wait_ms").is_some_and(|wait| !wait.is_null());
    let permit = if waits {
        Some(PARKED.try_acquire().map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("capacity: {MAX_PARKED} requests are already waiting on resource claims"),
            )
            .with_code("capacity")
        })?)
    } else {
        None
    };
    let service = service(&state);
    let path = uri.path().to_owned();
    let (answer, received) = oneshot::channel();
    tokio::task::spawn_blocking(move || {
        let result = service.request("POST", &path, &body);
        if let Err(Ok(unclaimed)) = answer.send(result) {
            // The client went away while this request was parked. A grant nobody
            // received would hold the resource for its whole lease; end it now.
            if let Some(token) = unclaimed.get("renewal_token").and_then(Value::as_str) {
                let _ = service.release(token, None);
            }
        }
    });
    let result = received.await;
    drop(permit);
    result
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "claim task failed"))?
        .map(Json)
        .map_err(error)
}

/// `POST /resource-claim-token/renew|release`, authorized by
/// `Authorization: Bearer <renewal token>` alone. Nothing but the token's own claim is
/// readable or writable here.
pub(crate) async fn token(
    State(state): State<AppState>,
    Path(action): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let unauthorized = || {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "token_invalid: present the renewal token as an Authorization bearer",
        )
        .with_code("token_invalid")
    };
    let presented = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty() && token.len() <= MAX_RENEWAL_TOKEN_LEN)
        .ok_or_else(unauthorized)?;
    // Only the documented field crosses from the body; a body can never name a token.
    let mut request = serde_json::Map::new();
    if !body.is_empty() {
        let parsed: Value = serde_json::from_slice(&body)
            .map_err(|_| error(ClaimError::Invalid("body must be JSON".into())))?;
        if let Some(duration) = parsed.get("duration_ms") {
            request.insert("duration_ms".into(), duration.clone());
        }
    }
    request.insert("renewal_token".into(), presented.into());
    service(&state)
        .request(
            "POST",
            &format!("{TOKEN_ROUTE_PREFIX}/{action}"),
            &Value::Object(request),
        )
        .map(Json)
        .map_err(error)
}
