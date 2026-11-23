//! HS2-4B0KSD: the remote session lifetime through the real router, a real listener and a
//! restarted server, on a mocked clock. Policy: 30 days idle, 90 days from approval or renewal,
//! renewal in place, a distinct failure code for expired and revoked sessions.
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use futures_util::StreamExt;
use hotsheet_server::remote_auth::{IDLE_TTL, ManualClock, SESSION_TTL};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:4175";
const SECRET: &str = "synthetic-local-secret";
const DAY: i64 = 24 * 60 * 60;
const START: i64 = 2_000_000_000;

/// One server process over a fixed machine home: a "restart" opens the same files again.
fn server(dir: &std::path::Path, clock: &ManualClock) -> AppState {
    let store = if dir.join("store").exists() {
        FsStore::open(dir.join("store")).unwrap()
    } else {
        FsStore::init_git(dir.join("store"), &StoreMetadata::new("SYN")).unwrap()
    };
    AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(dir.join("home"))
        .with_remote_auth(dir.join("home/remote-auth.json"))
        .unwrap()
        .with_remote_auth_clock(clock.clock())
}

fn request(
    method: &str,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
    csrf: Option<&str>,
    owner: bool,
) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, ORIGIN);
    if let Some(cookie) = cookie {
        req = req.header(header::COOKIE, cookie);
    }
    if let Some(csrf) = csrf {
        req = req.header("x-hotsheet-csrf", csrf);
    }
    if owner {
        req = req.header("x-hotsheet-secret", SECRET);
    }
    match body {
        Some(value) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    }
}

async fn call(router: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

struct Paired {
    cookie: String,
    csrf: String,
    id: String,
}

async fn pair(router: &axum::Router, label: &str) -> Paired {
    let (status, pairing) = call(
        router,
        request(
            "POST",
            "/remote-auth/pair",
            Some(json!({"label": label})),
            None,
            None,
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call(
        router,
        request(
            "POST",
            "/remote-auth/owner/approve",
            Some(json!({"id": pairing["id"], "role": "read_write"})),
            None,
            None,
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let redeemed = router
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/redeem",
            Some(json!({"id": pairing["id"], "challenge": pairing["challenge"]})),
            None,
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(redeemed.status(), StatusCode::OK);
    let cookies = redeemed
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    // The browser keeps the cookies longer than any session lives; the server decides.
    assert!(
        cookies
            .iter()
            .all(|cookie| cookie.contains("Max-Age=34560000")),
        "{cookies:?}"
    );
    let first = |cookie: &String| cookie.split(';').next().unwrap().to_owned();
    let device: Value =
        serde_json::from_slice(&redeemed.into_body().collect().await.unwrap().to_bytes()).unwrap();
    Paired {
        cookie: first(&cookies[0]),
        csrf: first(&cookies[1]).split_once('=').unwrap().1.to_owned(),
        id: device["id"].as_str().unwrap().to_owned(),
    }
}

async fn me(router: &axum::Router, device: &Paired) -> (StatusCode, Value) {
    call(
        router,
        request(
            "GET",
            "/remote-auth/me",
            None,
            Some(&device.cookie),
            None,
            false,
        ),
    )
    .await
}

type SyncSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn serve(state: AppState) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = app(state);
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    addr
}

async fn sync_socket(addr: std::net::SocketAddr, device: &Paired) -> SyncSocket {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut upgrade = format!("ws://{addr}/ws/sync")
        .into_client_request()
        .unwrap();
    let headers = upgrade.headers_mut();
    headers.insert("origin", ORIGIN.parse().unwrap());
    headers.insert(
        "cookie",
        format!("{}; hs2_remote_csrf={}", device.cookie, device.csrf)
            .parse()
            .unwrap(),
    );
    tokio_tungstenite::connect_async(upgrade).await.unwrap().0
}

/// Wait until the socket's server closes it; any event frames before that are skipped.
async fn closed(socket: &mut SyncSocket) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(message)) = socket.next().await {
            if message.is_close() {
                return;
            }
        }
    })
    .await
    .expect("the server closes the stream");
}

/// Advance the clock and wait for the open stream's once-a-second recheck to persist the use.
async fn advance_while_streaming(state: &AppState, clock: &ManualClock, seconds: i64) {
    let mut writes = state.remote_auth().unwrap().writes();
    let before = *writes.borrow_and_update();
    clock.advance(seconds);
    tokio::time::timeout(
        Duration::from_secs(10),
        writes.wait_for(|count| *count > before),
    )
    .await
    .expect("the open stream records the device's use")
    .unwrap();
}

#[tokio::test]
async fn hs2_4b0ksd_an_open_stream_keeps_a_device_past_thirty_days_through_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(START);
    let state = server(dir.path(), &clock);
    let router = app(state.clone());
    let addr = serve(state.clone()).await;
    let device = pair(&router, "Synthetic laptop").await;
    let mut socket = sync_socket(addr, &device).await;

    // 36 days with the stream as the only activity, three days at a time.
    let mut writes = state.remote_auth().unwrap().writes();
    let before = *writes.borrow_and_update();
    for _ in 0..12 {
        advance_while_streaming(&state, &clock, 3 * DAY).await;
    }
    assert!(clock.now() - START > IDLE_TTL);
    // Bounded persistence: one write per step, however many rechecks ran.
    assert_eq!(*writes.borrow(), before + 12);

    // The server restarts (a rebuild or a reboot): the device reconnects without approval.
    drop(socket);
    let restarted = server(dir.path(), &clock);
    let router = app(restarted.clone());
    let addr = serve(restarted.clone()).await;
    let (status, current) = me(&router, &device).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert_eq!(current["id"], json!(device.id));
    assert_eq!(current["reminder_day"], Value::Null);
    socket = sync_socket(addr, &device).await;

    // Still in use on day 60 and day 89: the reminder days pass, the session holds.
    advance_while_streaming(&restarted, &clock, 24 * DAY).await;
    let (_, current) = me(&router, &device).await;
    assert_eq!(current["reminder_day"], json!(60));
    advance_while_streaming(&restarted, &clock, 29 * DAY).await;
    let (_, current) = me(&router, &device).await;
    assert_eq!(current["reminder_day"], json!(89));

    // Day 90: the absolute limit ends even a device in constant use, and the stream closes.
    clock.set(START + SESSION_TTL);
    closed(&mut socket).await;
    let (status, failure) = me(&router, &device).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(failure["code"], json!("session_expired"));

    // One renewal on the host restores the same device and credential; nothing is set up again.
    let (status, renewed) = call(
        &router,
        request(
            "POST",
            &format!("/remote-auth/owner/devices/{}/renew", device.id),
            None,
            None,
            None,
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renewed}");
    assert_eq!(renewed["renewed_at"], json!(START + SESSION_TTL));
    let (status, current) = me(&router, &device).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current["id"], json!(device.id));
    assert_eq!(current["role"], json!("read_write"));
    let _socket = sync_socket(addr, &device).await;
}

#[tokio::test]
async fn hs2_4b0ksd_an_idle_device_expires_asks_for_renewal_and_is_renewed_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(START);
    let state = server(dir.path(), &clock);
    let router = app(state);
    let device = pair(&router, "Synthetic phone").await;

    // 30 days without use, no stream open.
    clock.advance(IDLE_TTL - 1);
    assert_eq!(me(&router, &device).await.0, StatusCode::OK);
    clock.advance(IDLE_TTL);
    let (status, failure) = me(&router, &device).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(failure["code"], json!("session_expired"));
    // Other routes report the same code, never a bare failure.
    let (status, failure) = call(
        &router,
        request("GET", "/checkouts", None, Some(&device.cookie), None, false),
    )
    .await;
    assert_eq!(
        (status, failure["code"].clone()),
        (StatusCode::UNAUTHORIZED, json!("session_expired"))
    );

    // The expired device asks for renewal with its own proof; the host sees the request.
    let (status, _) = call(
        &router,
        request(
            "POST",
            "/remote-auth/renewal",
            None,
            Some(&device.cookie),
            None,
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, asked) = call(
        &router,
        request(
            "POST",
            "/remote-auth/renewal",
            None,
            Some(&device.cookie),
            Some(&device.csrf),
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{asked}");
    let (_, list) = call(
        &router,
        request("GET", "/remote-auth/owner", None, None, None, true),
    )
    .await;
    let listed = &list["devices"][0];
    assert_eq!(listed["id"], json!(device.id));
    assert_eq!(listed["expired"], json!(true));
    assert_eq!(listed["renewal_requested_at"], json!(clock.now()));

    let (status, _) = call(
        &router,
        request(
            "POST",
            &format!("/remote-auth/owner/devices/{}/renew", device.id),
            None,
            None,
            None,
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, current) = me(&router, &device).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current["renewal_requested_at"], Value::Null);
    // Renewing needs the host or an admin session.
    let (status, _) = call(
        &router,
        request(
            "POST",
            &format!("/remote-auth/owner/devices/{}/renew", device.id),
            None,
            Some(&device.cookie),
            Some(&device.csrf),
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &router,
        request(
            "POST",
            "/remote-auth/owner/devices/UNKNOWN/renew",
            None,
            None,
            None,
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn hs2_4b0ksd_revoking_an_active_device_closes_its_stream_and_reports_revoked() {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(START);
    let state = server(dir.path(), &clock);
    let router = app(state.clone());
    let addr = serve(state).await;
    let device = pair(&router, "Synthetic tablet").await;
    let mut socket = sync_socket(addr, &device).await;
    let (status, _) = call(
        &router,
        request(
            "DELETE",
            &format!("/remote-auth/owner/devices/{}", device.id),
            None,
            None,
            None,
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    closed(&mut socket).await;
    let (status, failure) = me(&router, &device).await;
    assert_eq!(
        (status, failure["code"].clone()),
        (StatusCode::UNAUTHORIZED, json!("session_revoked"))
    );
    // A revoked device cannot ask for renewal, and the host cannot renew it.
    let (status, failure) = call(
        &router,
        request(
            "POST",
            "/remote-auth/renewal",
            None,
            Some(&device.cookie),
            Some(&device.csrf),
            false,
        ),
    )
    .await;
    assert_eq!(
        (status, failure["code"].clone()),
        (StatusCode::UNAUTHORIZED, json!("session_revoked"))
    );
    let (status, _) = call(
        &router,
        request(
            "POST",
            &format!("/remote-auth/owner/devices/{}/renew", device.id),
            None,
            None,
            None,
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // An unknown credential stays distinct from both.
    let (status, failure) = call(
        &router,
        request(
            "GET",
            "/remote-auth/me",
            None,
            Some("hs2_remote_session=unknown"),
            None,
            false,
        ),
    )
    .await;
    assert_eq!(
        (status, failure["code"].clone()),
        (StatusCode::UNAUTHORIZED, json!("session_invalid"))
    );
}
