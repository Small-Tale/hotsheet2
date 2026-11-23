//! HS2-7DBJMS remote browser auth endpoints. Device management (`/remote-auth/owner/*`)
//! accepts the machine-local secret from the trusted host, or an admin browser session with
//! its paired Origin and CSRF proof on every change (HS2-DXM0YN). Any manager, an admin
//! session included, may grant or remove host access (`local`, HS2-85W6BE), as the
//! maintainer decided in HS2-KTHQE5.

use crate::AppState;
use crate::api_error::ApiError;
use crate::events::ChangeEvent;
use crate::remote_auth::{
    AuthFailure, Device, DeviceChange, Grants, IssuedSession, PendingDevice, Role,
};
use axum::Extension;
use axum::Json;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

const SESSION_COOKIE: &str = "hs2_remote_session";
const CSRF_COOKIE: &str = "hs2_remote_csrf";
/// How long a browser keeps the session cookies: the longest lifetime browsers honor (400
/// days). The server's 30-day idle and 90-day session policy decides validity (HS2-4B0KSD); the cookie
/// only has to outlive it, and `GET /remote-auth/me` refreshes it while the device is in use.
const COOKIE_MAX_AGE: i64 = 400 * 24 * 60 * 60;

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
    /// Whether this caller itself has host access (the trusted host, or a `local` session).
    /// Granting host access no longer depends on it (HS2-KTHQE5).
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

/// The change event kind open pages re-read their own session grants on (HS2-A3TDNH).
pub(crate) const BROWSER_GRANTS_EVENT: &str = "browser_grants";

/// The change event kind device managers re-read the pending pairing requests on
/// (HS2-BZHMEV). Only subscribers that may manage devices receive it.
pub(crate) const PAIRING_REQUESTS_EVENT: &str = "pairing_requests";

/// Push one anonymous, cursored, machine-wide event of `kind`. It carries no store, id,
/// label, origin or value, so it tells a receiver only that something of that kind changed.
fn emit_anonymous(state: &AppState, kind: &str) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: kind.into(),
        id: String::new(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
        live: None,
    });
}

/// Announce that some browser session's grants or validity may have changed. It names no
/// device and carries no values, so it tells other sessions nothing: each remote-mode page
/// re-reads its own `GET /remote-auth/me`, which stays the one source of truth (HS2-A3TDNH).
fn emit_grants_changed(state: &AppState) {
    emit_anonymous(state, BROWSER_GRANTS_EVENT);
}

/// Announce that the requests waiting for approval changed (HS2-BZHMEV): one arrived, or one
/// was approved or rejected. Device managers then re-read the admin-gated
/// `/remote-auth/owner` list; the event names no request and carries no pairing secret.
fn emit_pairing_requests_changed(state: &AppState) {
    emit_anonymous(state, PAIRING_REQUESTS_EVENT);
}

/// Whether one change-stream subscriber may receive `event` (HS2-BZHMEV). Pairing-request
/// events reach only device managers: the trusted host (`local`: machine secret or client
/// certificate) or a browser session that holds the admin grant now, so a grant removed
/// meanwhile stops them. Every other kind reaches every authorized subscriber.
pub(crate) fn event_visible(
    state: &AppState,
    headers: &HeaderMap,
    local: bool,
    websocket: bool,
    event: &ChangeEvent,
) -> bool {
    event.kind != PAIRING_REQUESTS_EVENT
        || local
        || session(state, headers, false, websocket).is_ok_and(|device| device.admin)
}

/// Drop the events this long-poll subscriber may not receive from one page (HS2-BZHMEV). The
/// session is checked once per page, and only when the page holds a gated event. The page's
/// cursor still advances past dropped events.
pub(crate) fn retain_visible_events(
    state: &AppState,
    headers: &HeaderMap,
    local: bool,
    events: &mut Vec<ChangeEvent>,
) {
    let Some(gated) = events
        .iter()
        .find(|event| event.kind == PAIRING_REQUESTS_EVENT)
    else {
        return;
    };
    if !event_visible(state, headers, local, false, gated) {
        events.retain(|event| event.kind != PAIRING_REQUESTS_EVENT);
    }
}

/// The session catalog's clock (HS2-4B0KSD), so transition tests can drive time; the wall clock
/// when remote authentication is not configured.
fn now(state: &AppState) -> i64 {
    state.remote_auth.as_ref().map_or_else(
        crate::remote_auth::wall_clock,
        crate::remote_auth::RemoteAuth::now,
    )
}

fn catalog(state: &AppState) -> Result<&crate::remote_auth::RemoteAuth, ApiError> {
    state.remote_auth.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "remote authentication is not configured",
        )
    })
}

/// The `code` of a paired session past its idle or absolute limit; renewal restores it.
pub(crate) const SESSION_EXPIRED_CODE: &str = "session_expired";
/// The `code` of a revoked paired session; only a new pairing grants access again.
pub(crate) const SESSION_REVOKED_CODE: &str = "session_revoked";
/// The `code` of an unknown credential or a failed pairing proof.
pub(crate) const SESSION_INVALID_CODE: &str = "session_invalid";

/// Each failure carries a stable `code`, so a client tells an expired approval from a revoked
/// one, an unknown credential, a network failure or a server error (HS2-4B0KSD).
fn failed(error: AuthFailure) -> ApiError {
    let (status, message, code) = match error {
        AuthFailure::RateLimited => (
            StatusCode::TOO_MANY_REQUESTS,
            "pairing attempts are limited",
            "rate_limited",
        ),
        AuthFailure::Expired => (
            StatusCode::GONE,
            "pairing request expired",
            "pairing_expired",
        ),
        AuthFailure::SessionExpired => (
            StatusCode::UNAUTHORIZED,
            "this device's approval expired; renew it on the host",
            SESSION_EXPIRED_CODE,
        ),
        AuthFailure::Revoked => (
            StatusCode::UNAUTHORIZED,
            "this device's access was revoked",
            SESSION_REVOKED_CODE,
        ),
        AuthFailure::Unapproved => (
            StatusCode::CONFLICT,
            "waiting for host approval",
            "unapproved",
        ),
        AuthFailure::ReadOnly => (StatusCode::FORBIDDEN, "device is read-only", "read_only"),
        AuthFailure::Csrf => (
            StatusCode::FORBIDDEN,
            "CSRF token required",
            "csrf_required",
        ),
        AuthFailure::Invalid => (
            StatusCode::UNAUTHORIZED,
            "invalid pairing or session",
            SESSION_INVALID_CODE,
        ),
    };
    ApiError::new(status, message).with_code(code)
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

/// The paired-session credential of an `Authorization: Bearer` header (HS2-AW2AKF).
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = value(headers, "authorization")?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|token| !token.is_empty())
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
        .authorize(token, csrf, origin, false, now(state))
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

fn device_list(state: &AppState, manager: Manager) -> Result<DeviceList, ApiError> {
    let catalog = catalog(state)?;
    // Read the cursor first: a change racing this read then wakes the next poll at once.
    let revision = catalog.revision();
    Ok(DeviceList {
        server_id: catalog.server_id(),
        revision,
        pair_ttl_seconds: crate::remote_auth::PAIR_TTL,
        manager,
        devices: catalog.list(now(state)),
        pending: catalog.pending(now(state)),
    })
}

/// Browser session validation shared by ordinary REST and the separately routed streams.
pub(crate) fn session(
    state: &AppState,
    headers: &HeaderMap,
    write: bool,
    websocket: bool,
) -> Result<Device, ApiError> {
    // HS2-AW2AKF: a paired machine's server-to-server call presents its credential as a
    // bearer header. A browser never sends that header on its own, so it needs no Origin
    // or CSRF proof; the cookie path below keeps both for browsers. A bearer that names no
    // paired session (another credential on the same header) falls through to the cookie.
    // Without remote authentication there is no paired session to name, and the header
    // keeps its other meanings (a resource-claim renewal token): the cookie path decides.
    if let (Some(token), Some(catalog)) = (bearer(headers), state.remote_auth.as_ref()) {
        match catalog
            .authorize_bearer(token, write, now(state))
            .map_err(|_| internal())?
        {
            Err(AuthFailure::Invalid) if cookie(headers, SESSION_COOKIE).is_some() => {}
            result => return result.map_err(failed),
        }
    }
    let token = cookie(headers, SESSION_COOKIE).ok_or_else(|| failed(AuthFailure::Invalid))?;
    let origin = value(headers, "origin");
    if (write || websocket) && origin.is_none() {
        return Err(failed(AuthFailure::Invalid));
    }
    let csrf = value(headers, "x-hotsheet-csrf")
        .or_else(|| websocket.then(|| cookie(headers, CSRF_COOKIE)).flatten());
    catalog(state)?
        .authorize(token, csrf, origin, write, now(state))
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
        .request(&request.label, origin, now(&state))
        .map_err(|_| internal())?
        .map_err(failed)?;
    emit_pairing_requests_changed(&state);
    Ok((StatusCode::CREATED, Json(pair)))
}

pub(crate) async fn redeem(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RedeemRequest>,
) -> Result<Response, ApiError> {
    let origin = value(&headers, "origin").ok_or_else(|| failed(AuthFailure::Invalid))?;
    let issued = catalog(&state)?
        .redeem(&request.id, &request.challenge, origin, now(&state))
        .map_err(|_| internal())?
        .map_err(failed)?;
    Ok(issued_response(issued, origin))
}

fn issued_response(issued: IssuedSession, origin: &str) -> Response {
    let mut response = Json(issued.device).into_response();
    set_session_cookies(&mut response, &issued.token, Some(&issued.csrf), origin);
    response
}

/// Set the session cookie and, when known, the CSRF cookie, for `origin`'s scheme.
fn set_session_cookies(response: &mut Response, token: &str, csrf: Option<&str>, origin: &str) {
    let secure = if origin.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    let session = Some(format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age={COOKIE_MAX_AGE}; HttpOnly; SameSite=Strict{secure}"
    ));
    let csrf = csrf.map(|csrf| {
        format!("{CSRF_COOKIE}={csrf}; Path=/; Max-Age={COOKIE_MAX_AGE}; SameSite=Strict{secure}")
    });
    for cookie in [session, csrf].into_iter().flatten() {
        // A value that is not header safe was never issued by this server: skip it.
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
}

/// Whether a cookie value is one this server could have issued (43 URL-safe base64 bytes).
fn issued_shape(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// This browser's own session and grants. A valid session also gets its cookies' browser
/// lifetime renewed, so a device in use never loses them to `Max-Age` (HS2-4B0KSD). The CSRF
/// cookie is renewed only when it is the session's own.
pub(crate) async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let device = session(&state, &headers, false, false)?;
    let token = cookie(&headers, SESSION_COOKIE).unwrap_or_default();
    let csrf = cookie(&headers, CSRF_COOKIE).filter(|value| {
        issued_shape(value)
            && catalog(&state).is_ok_and(|catalog| {
                catalog
                    .authorize(token, Some(value), None, false, now(&state))
                    .is_ok_and(|result| result.is_ok())
            })
    });
    let origin = device.origin.clone();
    let mut response = Json(device).into_response();
    if issued_shape(token) {
        set_session_cookies(&mut response, token, csrf, &origin);
    }
    Ok(response)
}

/// The device asks the host to renew its approval in place (HS2-4B0KSD). An expired session
/// may ask; it needs its paired Origin and CSRF proof. Device managers see the request in the
/// device list and renew with one action.
pub(crate) async fn request_renewal(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Device>), ApiError> {
    let token = cookie(&headers, SESSION_COOKIE).ok_or_else(|| failed(AuthFailure::Invalid))?;
    let csrf = value(&headers, "x-hotsheet-csrf").ok_or_else(|| failed(AuthFailure::Csrf))?;
    let origin = value(&headers, "origin").ok_or_else(|| failed(AuthFailure::Invalid))?;
    let device = catalog(&state)?
        .request_renewal(token, csrf, origin, now(&state))
        .map_err(|_| internal())?
        .map_err(failed)?;
    emit_pairing_requests_changed(&state);
    Ok((StatusCode::ACCEPTED, Json(device)))
}

/// Renew one device in place (HS2-4B0KSD): its 90-day lifetime restarts and it keeps its id,
/// credential, role and grants. The trusted host or an admin session may renew any device,
/// expired ones included.
pub(crate) async fn renew(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Device>, ApiError> {
    manager(&state, &headers, peer.as_ref(), true)?;
    let renewed = catalog(&state)?
        .renew(&id, now(&state))
        .map_err(|_| internal())?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "device not found"))?;
    // The renewed device's open pages re-read their session; managers re-read the list.
    emit_grants_changed(&state);
    emit_pairing_requests_changed(&state);
    Ok(Json(renewed))
}

pub(crate) async fn rotate(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let token = cookie(&headers, SESSION_COOKIE).ok_or_else(|| failed(AuthFailure::Invalid))?;
    let csrf = value(&headers, "x-hotsheet-csrf").ok_or_else(|| failed(AuthFailure::Csrf))?;
    let origin = value(&headers, "origin").ok_or_else(|| failed(AuthFailure::Invalid))?;
    let issued = catalog(&state)?
        .rotate(token, csrf, origin, now(&state))
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
        .authorize(token, Some(csrf), Some(origin), false, now(&state))
        .map_err(|_| internal())?
        .map_err(failed)?;
    catalog(&state)?
        .revoke(&device.id)
        .map_err(|_| internal())?;
    // Other tabs of this browser share the session and lose it too.
    emit_grants_changed(&state);
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
    manager(&state, &headers, peer.as_ref(), true)?;
    catalog(&state)?
        .approve(
            &request.id,
            request.role,
            Grants {
                admin: request.admin,
                local: request.local,
            },
            now(&state),
        )
        .map_err(|_| internal())?
        .map_err(failed)?;
    emit_pairing_requests_changed(&state);
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
        emit_pairing_requests_changed(&state);
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
    manager(&state, &headers, peer.as_ref(), true)?;
    let changed = catalog(&state)?
        .change(&id, request)
        .map_err(|_| internal())?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "device not found"))?;
    emit_grants_changed(&state);
    Ok(Json(changed))
}

pub(crate) async fn revoke(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    manager(&state, &headers, peer.as_ref(), true)?;
    if catalog(&state)?.revoke(&id).map_err(|_| internal())? {
        emit_grants_changed(&state);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Revoking every session, the caller's own included, stays with the trusted host.
pub(crate) async fn revoke_all(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    owner(&state, &headers, peer.as_ref())?;
    if catalog(&state)?.revoke_all().map_err(|_| internal())? > 0 {
        emit_grants_changed(&state);
    }
    Ok(StatusCode::NO_CONTENT)
}
