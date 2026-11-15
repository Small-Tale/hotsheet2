#![cfg(unix)] // A `cat` PTY fixture.

//! HS2-PPHJMZ: the contract a desktop bridge relies on when it opens another host's terminals
//! through a paired remote session. A read-only session may list terminals but never create,
//! write or attach; a read-write session needs its paired Origin and CSRF proof for both REST
//! mutations and the attach upgrade; revocation ends a live attach stream.

use axum::Extension;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use futures_util::{SinkExt, StreamExt};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;

const SECRET: &str = "synthetic-local-secret";
const ORIGIN: &str = "http://localhost:4175";
const OFF_BOX: &str = "192.0.2.20:12345";

fn fixture() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(dir.path().join("store"), &StoreMetadata::new("SYN")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(dir.path().join("home"))
        .with_remote_auth(dir.path().join("home/remote-auth.json"))
        .unwrap()
        .with_remote_listener();
    (dir, state)
}

fn peer(request: &mut Request<Body>, address: &str) {
    request.extensions_mut().insert(ConnectInfo(
        address.parse::<std::net::SocketAddr>().unwrap(),
    ));
}

fn remote(
    method: &str,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-hotsheet-csrf", csrf);
    }
    let mut request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    peer(&mut request, OFF_BOX);
    request
}

fn owner(method: &str, path: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x-hotsheet-secret", SECRET);
    let mut request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    peer(&mut request, "127.0.0.1:12345");
    request
}

async fn json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

/// Pair an off-box device with the given role and return its cookie header and CSRF token.
async fn paired(router: &axum::Router, role: &str) -> (String, String) {
    let pair = json(
        router
            .clone()
            .oneshot(remote(
                "POST",
                "/remote-auth/pair",
                None,
                None,
                Some(json!({"label": format!("Synthetic {role} desktop")})),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = pair["id"].as_str().unwrap();
    let challenge = pair["challenge"].as_str().unwrap();
    let approved = router
        .clone()
        .oneshot(owner(
            "POST",
            "/remote-auth/owner/approve",
            Some(json!({"id": id, "role": role})),
        ))
        .await
        .unwrap();
    assert_eq!(approved.status(), StatusCode::NO_CONTENT);
    let redeemed = router
        .clone()
        .oneshot(remote(
            "POST",
            "/remote-auth/redeem",
            None,
            None,
            Some(json!({"id": id, "challenge": challenge})),
        ))
        .await
        .unwrap();
    assert_eq!(redeemed.status(), StatusCode::OK);
    let cookies = redeemed
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| {
            value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let csrf = cookies
        .iter()
        .find_map(|value| value.strip_prefix("hs2_remote_csrf="))
        .unwrap()
        .to_owned();
    let session = cookies
        .iter()
        .find(|value| value.starts_with("hs2_remote_session="))
        .unwrap()
        .clone();
    (session, csrf)
}

async fn status(router: &axum::Router, request: Request<Body>) -> StatusCode {
    router.clone().oneshot(request).await.unwrap().status()
}

/// Serve the router as if every connection came from an off-box peer, so the attach upgrade
/// goes through the same accepted-peer guard as a real remote desktop.
async fn serve_off_box(state: AppState) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app(state).layer(Extension(ConnectInfo(
                OFF_BOX.parse::<std::net::SocketAddr>().unwrap(),
            ))),
        )
        .await
        .unwrap();
    });
    addr
}

async fn attach(
    addr: std::net::SocketAddr,
    id: &str,
    session: &str,
    csrf: Option<&str>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    StatusCode,
> {
    let mut request = format!("ws://{addr}/terminals/{id}/attach?replay_protocol=2")
        .into_client_request()
        .unwrap();
    let headers = request.headers_mut();
    headers.insert("origin", ORIGIN.parse().unwrap());
    headers.insert("cookie", session.parse().unwrap());
    if let Some(csrf) = csrf {
        headers.insert("x-hotsheet-csrf", csrf.parse().unwrap());
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => Ok(socket),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => Err(response.status()),
        Err(other) => panic!("unexpected attach failure: {other:?}"),
    }
}

#[tokio::test]
async fn hs2_pphjmz_remote_terminals_need_a_read_write_session_and_end_on_revocation() {
    let (dir, state) = fixture();
    remote_terminal_contract(dir, state).await;
}

/// The same contract when terminals live in the detached broker (HS2-ERT00F).
#[tokio::test]
async fn hs2_pphjmz_remote_broker_terminals_end_on_revocation() {
    use hotsheet_server::terminal_broker::TerminalBroker;
    use std::sync::Arc;

    let broker = tempfile::tempdir().unwrap();
    let socket = broker.path().join("b.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    tokio::spawn(hotsheet_terminals::serve_broker(
        listener,
        "proj".into(),
        Arc::new(hotsheet_terminals::TerminalManager::new()),
    ));
    let (dir, state) = fixture();
    let state = state.with_terminal_broker_at(TerminalBroker::at(&socket, "proj"));
    remote_terminal_contract(dir, state).await;
}

async fn remote_terminal_contract(dir: tempfile::TempDir, state: AppState) {
    let router = app(state.clone());
    let workdir = dir.path().join("checkout");
    std::fs::create_dir_all(&workdir).unwrap();
    let workdir = workdir.canonicalize().unwrap().display().to_string();
    let launch = json!({"id": "remote-cat", "command": "cat", "cwd": workdir});

    // A read-only desktop session lists terminals but cannot open one.
    let (viewer, viewer_csrf) = paired(&router, "read_only").await;
    assert_eq!(
        status(
            &router,
            remote("GET", "/terminals", Some(&viewer), None, None)
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &router,
            remote(
                "POST",
                "/terminals",
                Some(&viewer),
                Some(&viewer_csrf),
                Some(launch.clone())
            )
        )
        .await,
        StatusCode::FORBIDDEN
    );

    // A read-write session needs its CSRF proof, then launches in the requested checkout.
    let (desktop, csrf) = paired(&router, "read_write").await;
    assert_eq!(
        status(
            &router,
            remote(
                "POST",
                "/terminals",
                Some(&desktop),
                None,
                Some(launch.clone())
            )
        )
        .await,
        StatusCode::FORBIDDEN
    );
    let opened = router
        .clone()
        .oneshot(remote(
            "POST",
            "/terminals",
            Some(&desktop),
            Some(&csrf),
            Some(launch),
        ))
        .await
        .unwrap();
    assert_eq!(opened.status(), StatusCode::OK);
    let opened = json(opened).await;
    assert_eq!(opened["launch_cwd"], json!(workdir));
    let listed = json(
        router
            .clone()
            .oneshot(remote("GET", "/terminals", Some(&desktop), None, None))
            .await
            .unwrap(),
    )
    .await;
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|terminal| terminal["id"] == "remote-cat"
                && terminal["launch_cwd"] == json!(workdir))
    );

    // The attach upgrade refuses the read-only session and a read-write one without CSRF.
    let addr = serve_off_box(state.clone()).await;
    assert_eq!(
        attach(addr, "remote-cat", &viewer, Some(&viewer_csrf))
            .await
            .err(),
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        attach(addr, "remote-cat", &desktop, None).await.err(),
        Some(StatusCode::UNAUTHORIZED)
    );
    let mut stream = attach(addr, "remote-cat", &desktop, Some(&csrf))
        .await
        .expect("a read-write session with its CSRF proof attaches");
    // The attach is live: input reaches the remote PTY and its echo streams back.
    stream
        .send(WsMessage::Text("remote-echo-marker\n".into()))
        .await
        .unwrap();
    let echoed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut output = Vec::new();
        while let Some(Ok(message)) = stream.next().await {
            if let WsMessage::Binary(bytes) = message {
                output.extend_from_slice(&bytes);
                if String::from_utf8_lossy(&output).contains("remote-echo-marker") {
                    return true;
                }
            }
        }
        false
    })
    .await;
    assert_eq!(echoed, Ok(true), "the remote PTY did not echo input");

    // Revoking every device ends the live stream within the server's recheck interval.
    assert_eq!(
        status(
            &router,
            owner("POST", "/remote-auth/owner/revoke-all", None)
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let ended = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match stream.next().await {
                None | Some(Err(_)) => return,
                Some(Ok(message)) if message.is_close() => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(
        ended.is_ok(),
        "a revoked session's terminal stream stays open"
    );
    assert_eq!(
        attach(addr, "remote-cat", &desktop, Some(&csrf))
            .await
            .err(),
        Some(StatusCode::UNAUTHORIZED)
    );
    assert!(
        status(&router, owner("DELETE", "/terminals/remote-cat", None))
            .await
            .is_success()
    );
}

type Viewer =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Pair a read-write device and return its cookie, CSRF token and device id, so one device can
/// be revoked while another stays attached.
async fn paired_device(router: &axum::Router) -> (String, String, String) {
    let before = device_ids(router).await;
    let (session, csrf) = paired(router, "read_write").await;
    let id = device_ids(router)
        .await
        .into_iter()
        .find(|id| !before.contains(id))
        .expect("pairing added a device");
    (session, csrf, id)
}

async fn device_ids(router: &axum::Router) -> Vec<String> {
    let listed = json(
        router
            .clone()
            .oneshot(owner("GET", "/remote-auth/owner", None))
            .await
            .unwrap(),
    )
    .await;
    listed["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|device| device["id"].as_str().unwrap().to_owned())
        .collect()
}

async fn revoke_device(router: &axum::Router, id: &str) {
    assert_eq!(
        status(
            router,
            owner("DELETE", &format!("/remote-auth/owner/devices/{id}"), None)
        )
        .await,
        StatusCode::NO_CONTENT
    );
}

/// Send a focused, interacting size claim for `viewer_id`.
async fn claim(stream: &mut Viewer, viewer_id: &str, cols: u16, rows: u16) {
    let claim = json!({"resize": {
        "viewer_id": viewer_id, "cols": cols, "rows": rows,
        "focus": true, "visible": true, "interacting": true,
    }});
    stream
        .send(WsMessage::Text(claim.to_string().into()))
        .await
        .unwrap();
}

/// Read until a `{pty_size, driven_by}` frame matches, skipping output and other control
/// frames. Fails when the stream ends or no match arrives in time.
async fn expect_size(
    stream: &mut Viewer,
    cols: u16,
    rows: u16,
    driven_by: Option<&str>,
    why: &str,
) {
    let wanted = (json!({"cols": cols, "rows": rows}), json!(driven_by));
    let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(Ok(message)) = stream.next().await {
            if let WsMessage::Text(text) = message
                && let Ok(frame) = serde_json::from_str::<Value>(&text)
                && frame.get("pty_size").is_some()
                && (frame["pty_size"].clone(), frame["driven_by"].clone()) == wanted
            {
                return true;
            }
        }
        false
    })
    .await;
    assert_eq!(found, Ok(true), "{why}: no {wanted:?} size frame");
}

async fn expect_closed(stream: &mut Viewer, why: &str) {
    let ended = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match stream.next().await {
                None | Some(Err(_)) => return,
                Some(Ok(message)) if message.is_close() => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "{why}: the stream stayed open");
}

/// HS2-3EDFG2: a remote viewer whose session is revoked must release its size claim, so the
/// PTY size recomputes for the viewers that remain. Walks normal exit, revocation, a repeated
/// attach and revoke, and revoking the last viewer.
#[tokio::test]
async fn hs2_3edfg2_revoked_viewer_releases_its_size_claim() {
    let (dir, state) = fixture();
    size_claim_release(dir, state).await;
}

/// The same transitions through the detached broker, which self-heals when its connection
/// drops (HS2-ERT00F).
#[tokio::test]
async fn hs2_3edfg2_revoked_broker_viewer_releases_its_size_claim() {
    use hotsheet_server::terminal_broker::TerminalBroker;
    use std::sync::Arc;

    let broker = tempfile::tempdir().unwrap();
    let socket = broker.path().join("b.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    tokio::spawn(hotsheet_terminals::serve_broker(
        listener,
        "proj".into(),
        Arc::new(hotsheet_terminals::TerminalManager::new()),
    ));
    let (dir, state) = fixture();
    let state = state.with_terminal_broker_at(TerminalBroker::at(&socket, "proj"));
    size_claim_release(dir, state).await;
}

async fn size_claim_release(dir: tempfile::TempDir, state: AppState) {
    let router = app(state.clone());
    let workdir = dir.path().join("checkout");
    std::fs::create_dir_all(&workdir).unwrap();
    let workdir = workdir.canonicalize().unwrap().display().to_string();
    let launched = router
        .clone()
        .oneshot(owner(
            "POST",
            "/terminals",
            Some(json!({"id": "sized-cat", "command": "cat", "cwd": workdir})),
        ))
        .await
        .unwrap();
    assert_eq!(launched.status(), StatusCode::OK);
    let addr = serve_off_box(state.clone()).await;

    // The remaining viewer drives first.
    let (remaining, remaining_csrf, remaining_device) = paired_device(&router).await;
    let mut stay = attach(addr, "sized-cat", &remaining, Some(&remaining_csrf))
        .await
        .unwrap();
    claim(&mut stay, "viewer-stay", 90, 28).await;
    expect_size(&mut stay, 90, 28, Some("viewer-stay"), "first claim").await;

    // Normal exit: a newer driver takes over, then closes its socket.
    let (driver, driver_csrf, driver_device) = paired_device(&router).await;
    let mut drive = attach(addr, "sized-cat", &driver, Some(&driver_csrf))
        .await
        .unwrap();
    claim(&mut drive, "viewer-drive-1", 120, 40).await;
    expect_size(&mut stay, 120, 40, Some("viewer-drive-1"), "takeover").await;
    drive.close(None).await.unwrap();
    expect_size(&mut stay, 90, 28, Some("viewer-stay"), "normal exit").await;

    // Revocation: the same takeover, but the driver's session is revoked under it.
    let mut drive = attach(addr, "sized-cat", &driver, Some(&driver_csrf))
        .await
        .unwrap();
    claim(&mut drive, "viewer-drive-2", 120, 40).await;
    expect_size(
        &mut stay,
        120,
        40,
        Some("viewer-drive-2"),
        "second takeover",
    )
    .await;
    revoke_device(&router, &driver_device).await;
    expect_closed(&mut drive, "revoked driver").await;
    expect_size(&mut stay, 90, 28, Some("viewer-stay"), "revoked driver").await;

    // Repeated attach and revoke with a newly paired device.
    let (again, again_csrf, again_device) = paired_device(&router).await;
    let mut drive = attach(addr, "sized-cat", &again, Some(&again_csrf))
        .await
        .unwrap();
    claim(&mut drive, "viewer-drive-3", 100, 30).await;
    expect_size(&mut stay, 100, 30, Some("viewer-drive-3"), "third takeover").await;
    revoke_device(&router, &again_device).await;
    expect_closed(&mut drive, "second revoked driver").await;
    expect_size(
        &mut stay,
        90,
        28,
        Some("viewer-stay"),
        "second revoked driver",
    )
    .await;

    // The last viewer revoked: the held size is no longer driven by anyone, so a new viewer
    // attaches to an undriven terminal instead of one held by a revoked claim.
    revoke_device(&router, &remaining_device).await;
    expect_closed(&mut stay, "last viewer").await;
    let (late, late_csrf, _) = paired_device(&router).await;
    let mut late_viewer = attach(addr, "sized-cat", &late, Some(&late_csrf))
        .await
        .unwrap();
    expect_size(&mut late_viewer, 90, 28, None, "last viewer revoked").await;

    assert!(
        status(&router, owner("DELETE", "/terminals/sized-cat", None))
            .await
            .is_success()
    );
}
