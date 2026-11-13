//! HS2-7DBJMS remote browser auth endpoints. Device management (`/remote-auth/owner/*`)
//! accepts the machine-local secret from the trusted host, or an admin browser session with
//! its paired Origin and CSRF proof on every change (HS2-DXM0YN). Only a host-trusted caller
//! (the secret or a `local` session) may grant or remove host access (HS2-85W6BE).

use crate::AppState;
use crate::api_error::ApiError;
use crate::remote_auth::{
    AuthFailure, Device, DeviceChange, Grants, IssuedSession, PendingDevice, Role,
};
use axum::Extension;
use axum::Json;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const SESSION_COOKIE: &str = "hs2_remote_session";
const CSRF_COOKIE: &str = "hs2_remote_csrf";

#[derive(Deserialize)]
pub(crate) struct PairRequest {
    label: String,
}

#[derive(Deserialize)]
pub(crate) struct RedeemRequest {
    id: String,
    challenge: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApproveRequest {
    id: String,
    role: Role,
    #[serde(default)]
    admin: bool,
    #[serde(default)]
    local: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RejectRequest {
    id: String,
}

#[derive(Deserialize)]
pub(crate) struct ChangesQuery {
    after: u64,
}

/// Who is managing devices: the trusted host, or one admin browser session.
#[derive(Serialize)]
pub(crate) struct Manager {
    /// The managing browser's own device id; absent for the trusted host.
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    /// Whether this caller may grant or remove host access.
    local: bool,
}

#[derive(Serialize)]
pub(crate) struct DeviceList {
    server_id: String,
    /// Change cursor for `/remote-auth/owner/changes?after=`.
    revision: u64,
    pair_ttl_seconds: i64,
    manager: Manager,
    devices: Vec<Device>,
    pending: Vec<PendingDevice>,
}

/// How long one device-list long poll waits before answering unchanged.
const CHANGES_WAIT: std::time::Duration = std::time::Duration::from_secs(25);

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn catalog(state: &AppState) -> Result<&crate::remote_auth::RemoteAuth, ApiError> {
    state.remote_auth.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "remote authentication is not configured",
        )
    })
}

fn failed(error: AuthFailure) -> ApiError {
    let (status, message) = match error {
        AuthFailure::RateLimited => (
            StatusCode::TOO_MANY_REQUESTS,
            "pairing attempts are limited",
        ),
        AuthFailure::Expired => (StatusCode::GONE, "pairing or session expired"),
        AuthFailure::Unapproved => (StatusCode::CONFLICT, "waiting for host approval"),
        AuthFailure::ReadOnly => (StatusCode::FORBIDDEN, "device is read-only"),
        AuthFailure::Csrf => (StatusCode::FORBIDDEN, "CSRF token required"),
        AuthFailure::Invalid => (StatusCode::UNAUTHORIZED, "invalid pairing or session"),
    };
    ApiError::new(status, message)
}

fn internal() -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "remote auth storage failed",
    )
}

fn value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    value(headers, "cookie")?.split(';').find_map(|piece| {
        piece
            .trim()
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('='))
    })
}

fn owner(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<&Extension<ConnectInfo<std::net::SocketAddr>>>,
) -> Result<(), ApiError> {
    if value(headers, "x-hotsheet-secret") == Some(state.secret.as_str())
        && state.local_bearer_allowed(peer.map(|info| info.0.0))
    {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "trusted host approval required",
        ))
    }
}

/// The trusted host (machine secret), or an admin browser session. A change additionally
/// needs the session's paired Origin and CSRF token; the device's read role does not limit
/// device management, which is the admin grant's whole meaning (HS2-DXM0YN).
fn manager(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<&Extension<ConnectInfo<std::net::SocketAddr>>>,
    change: bool,
) -> Result<Manager, ApiError> {
    if owner(state, headers, peer).is_ok() {
        return Ok(Manager {
            device_id: None,
            local: true,
        });
    }
    let token = cookie(headers, SESSION_COOKIE).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "trusted host or administrator session required",
        )
    })?;
    let origin = value(headers, "origin");
    let csrf = value(headers, "x-hotsheet-csrf");
    if change && origin.is_none() {
        return Err(failed(AuthFailure::Invalid));
    }
    if change && csrf.is_none() {
        return Err(failed(AuthFailure::Csrf));
    }
    let device = catalog(state)?
        .authorize(token, csrf, origin, false, now())
        .map_err(|_| internal())?
        .map_err(failed)?;
    if !device.admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "administrator access required",
        ));
    }
    Ok(Manager {
        device_id: Some(device.id),
        local: device.local,
    })
}

fn host_access_required() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "only the host or a host-trusted device can change host access",
    )
}

fn device_list(state: &AppState, manager: Manager) -> Result<DeviceList, ApiError> {
    let catalog = catalog(state)?;
    // Read the cursor first: a change racing this read then wakes the next poll at once.
    let revision = catalog.revision();
    Ok(DeviceList {
        server_id: catalog.server_id(),
        revision,
        pair_ttl_seconds: crate::remote_auth::PAIR_TTL,
        manager,
        devices: catalog.list(now()),
        pending: catalog.pending(now()),
    })
}

/// Browser session validation shared by ordinary REST and the separately routed streams.
pub(crate) fn session(
    state: &AppState,
    headers: &HeaderMap,
    write: bool,
    websocket: bool,
) -> Result<Device, ApiError> {
    let token = cookie(headers, SESSION_COOKIE).ok_or_else(|| failed(AuthFailure::Invalid))?;
    let origin = value(headers, "origin");
    if (write || websocket) && origin.is_none() {
        return Err(failed(AuthFailure::Invalid));
    }
    let csrf = value(headers, "x-hotsheet-csrf")
        .or_else(|| websocket.then(|| cookie(headers, CSRF_COOKIE)).flatten());
    catalog(state)?
        .authorize(token, csrf, origin, write, now())
        .map_err(|_| internal())?
        .map_err(failed)
}

pub(crate) async fn pair(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PairRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let origin = value(&headers, "origin").ok_or_else(|| failed(AuthFailure::Invalid))?;
    let pair = catalog(&state)?
        .request(&request.label, origin, now())
        .map_err(|_| internal())?
        .map_err(failed)?;
    Ok((StatusCode::CREATED, Json(pair)))
}

pub(crate) async fn redeem(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RedeemRequest>,
) -> Result<Response, ApiError> {
    let origin = value(&headers, "origin").ok_or_else(|| failed(AuthFailure::Invalid))?;
    let issued = catalog(&state)?
        .redeem(&request.id, &request.challenge, origin, now())
        .map_err(|_| internal())?
        .map_err(failed)?;
    Ok(issued_response(issued, origin))
}

fn issued_response(issued: IssuedSession, origin: &str) -> Response {
    let secure = if origin.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    let mut response = Json(issued.device).into_response();
    for cookie in [
        format!(
            "{SESSION_COOKIE}={}; Path=/; Max-Age=2592000; HttpOnly; SameSite=Strict{secure}",
            issued.token
        ),
        format!(
            "{CSRF_COOKIE}={}; Path=/; Max-Age=2592000; SameSite=Strict{secure}",
            issued.csrf
        ),
    ] {
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie).expect("random token is header safe"),
        );
    }
    response
}

pub(crate) async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Device>, ApiError> {
    Ok(Json(session(&state, &headers, false, false)?))
}

pub(crate) async fn rotate(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let token = cookie(&headers, SESSION_COOKIE).ok_or_else(|| failed(AuthFailure::Invalid))?;
    let csrf = value(&headers, "x-hotsheet-csrf").ok_or_else(|| failed(AuthFailure::Csrf))?;
    let origin = value(&headers, "origin").ok_or_else(|| failed(AuthFailure::Invalid))?;
    let issued = catalog(&state)?
        .rotate(token, csrf, origin, now())
        .map_err(|_| internal())?
        .map_err(failed)?;
    Ok(issued_response(issued, origin))
}

pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let token = cookie(&headers, SESSION_COOKIE).ok_or_else(|| failed(AuthFailure::Invalid))?;
    let csrf = value(&headers, "x-hotsheet-csrf").ok_or_else(|| failed(AuthFailure::Csrf))?;
    let origin = value(&headers, "origin").ok_or_else(|| failed(AuthFailure::Invalid))?;
    let device = catalog(&state)?
        .authorize(token, Some(csrf), Some(origin), false, now())
        .map_err(|_| internal())?
        .map_err(failed)?;
    catalog(&state)?
        .revoke(&device.id)
        .map_err(|_| internal())?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    let secure = if origin.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    for (name, http_only) in [(SESSION_COOKIE, "; HttpOnly"), (CSRF_COOKIE, "")] {
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "{name}=; Path=/; Max-Age=0; SameSite=Strict{http_only}{secure}"
            ))
            .expect("cookie name safe"),
        );
    }
    Ok(response)
}

pub(crate) async fn owner_list(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
) -> Result<Json<DeviceList>, ApiError> {
    let manager = manager(&state, &headers, peer.as_ref(), false)?;
    Ok(Json(device_list(&state, manager)?))
}

/// Long poll for the device list (HS2-DXM0YN): answers when the list changes from `after`,
/// or unchanged after [`CHANGES_WAIT`]. Authorization is checked again after the wait, so a
/// session revoked meanwhile receives no list.
pub(crate) async fn owner_changes(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    Query(query): Query<ChangesQuery>,
) -> Result<Json<DeviceList>, ApiError> {
    manager(&state, &headers, peer.as_ref(), false)?;
    catalog(&state)?
        .changed_since(query.after, CHANGES_WAIT)
        .await;
    let manager = manager(&state, &headers, peer.as_ref(), false)?;
    Ok(Json(device_list(&state, manager)?))
}

pub(crate) async fn approve(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<ApproveRequest>,
) -> Result<StatusCode, ApiError> {
    let manager = manager(&state, &headers, peer.as_ref(), true)?;
    if request.local && !manager.local {
        return Err(host_access_required());
    }
    catalog(&state)?
        .approve(
            &request.id,
            request.role,
            Grants {
                admin: request.admin,
                local: request.local,
            },
            now(),
        )
        .map_err(|_| internal())?
        .map_err(failed)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn reject(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<RejectRequest>,
) -> Result<StatusCode, ApiError> {
    manager(&state, &headers, peer.as_ref(), true)?;
    if catalog(&state)?
        .reject(&request.id)
        .map_err(|_| internal())?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "pairing request not found or expired",
        ))
    }
}

pub(crate) async fn change(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<DeviceChange>,
) -> Result<Json<Device>, ApiError> {
    let manager = manager(&state, &headers, peer.as_ref(), true)?;
    if request.local.is_some() && !manager.local {
        return Err(host_access_required());
    }
    catalog(&state)?
        .change(&id, request)
        .map_err(|_| internal())?
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "device not found"))
}

pub(crate) async fn revoke(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    manager(&state, &headers, peer.as_ref(), true)?;
    catalog(&state)?.revoke(&id).map_err(|_| internal())?;
    Ok(StatusCode::NO_CONTENT)
}

/// Revoking every session, the caller's own included, stays with the trusted host.
pub(crate) async fn revoke_all(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    owner(&state, &headers, peer.as_ref())?;
    catalog(&state)?.revoke_all().map_err(|_| internal())?;
    Ok(StatusCode::NO_CONTENT)
}
