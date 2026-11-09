//! HS2-7DBJMS remote browser auth endpoints. Owner operations additionally require the
//! machine-local secret even when a browser session is otherwise valid.

use crate::AppState;
use crate::api_error::ApiError;
use crate::remote_auth::{AuthFailure, Device, IssuedSession, PendingDevice, Role};
use axum::Extension;
use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
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
pub(crate) struct ApproveRequest {
    id: String,
    role: Role,
}

#[derive(Serialize)]
pub(crate) struct DeviceList {
    server_id: String,
    devices: Vec<Device>,
    pending: Vec<PendingDevice>,
}

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
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
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
    owner(&state, &headers, peer)?;
    let catalog = catalog(&state)?;
    Ok(Json(DeviceList {
        server_id: catalog.server_id(),
        devices: catalog.list(now()),
        pending: catalog.pending(now()),
    }))
}

pub(crate) async fn approve(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<ApproveRequest>,
) -> Result<StatusCode, ApiError> {
    owner(&state, &headers, peer)?;
    catalog(&state)?
        .approve(&request.id, request.role, now())
        .map_err(|_| internal())?
        .map_err(failed)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn revoke(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    owner(&state, &headers, peer)?;
    catalog(&state)?.revoke(&id).map_err(|_| internal())?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn revoke_all(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    owner(&state, &headers, peer)?;
    catalog(&state)?.revoke_all().map_err(|_| internal())?;
    Ok(StatusCode::NO_CONTENT)
}
