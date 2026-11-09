//! HS2-W4JJJE: the direct listener uses the accepted socket peer, never proxy headers,
//! when deciding whether the powerful machine-local bearer may be used.

use axum::Extension;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::json;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;

const SECRET: &str = "synthetic-local-secret";

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

fn remote_request(
    method: &str,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", "http://localhost:4175");
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-hotsheet-csrf", csrf);
    }
    let mut request = if let Some(body) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    request.extensions_mut().insert(ConnectInfo(
        "192.0.2.20:12345".parse::<std::net::SocketAddr>().unwrap(),
    ));
    request
}

fn request(path: &str, peer: &str, forwarded: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .uri(path)
        .header("x-hotsheet-secret", SECRET)
        .header("origin", "http://localhost:4175");
    if let Some(forwarded) = forwarded {
        builder = builder.header("x-forwarded-for", forwarded);
    }
    if path.starts_with("/ws/sync") || path.contains("/attach") {
        builder = builder
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==");
    }
    let mut request = builder.body(Body::empty()).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(peer.parse::<std::net::SocketAddr>().unwrap()));
    request
}

#[tokio::test]
async fn hs2_w4jjje_remote_peer_cannot_use_local_bearer_on_any_entry_point() {
    let (_dir, state) = fixture();
    let router = app(state);
    let remote = "192.0.2.20:12345";
    for path in [
        "/checkouts",
        "/stores/synthetic/tickets",
        "/remote-auth/owner",
        "/ws/poll?secret=synthetic-local-secret",
        "/ws/sync?secret=synthetic-local-secret",
        "/terminals/synthetic/attach?secret=synthetic-local-secret",
    ] {
        let response = router
            .clone()
            .oneshot(request(path, remote, Some("127.0.0.1")))
            .await
            .unwrap();
        // Router-only requests have no Hyper upgrade handle, so axum rejects WebSocket
        // handshakes before the handler. The same accepted-peer guard is applied in both
        // handlers; the real browser suite exercises upgraded sockets.
        let expected = if path.starts_with("/ws/sync") || path.contains("/attach") {
            StatusCode::UPGRADE_REQUIRED
        } else {
            StatusCode::UNAUTHORIZED
        };
        assert_eq!(response.status(), expected, "{path}");
    }
    let local = "127.0.0.1:12345";
    for path in [
        "/checkouts",
        "/remote-auth/owner",
        "/ws/poll?secret=synthetic-local-secret",
    ] {
        let response = router
            .clone()
            .oneshot(request(path, local, Some("192.0.2.20")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn hs2_w4jjje_paired_remote_session_reads_then_revokes_on_direct_listener() {
    let (_dir, state) = fixture();
    let router = app(state);
    let pair = router
        .clone()
        .oneshot(remote_request(
            "POST",
            "/remote-auth/pair",
            None,
            None,
            Some(json!({"label":"Synthetic device"})),
        ))
        .await
        .unwrap();
    assert_eq!(pair.status(), StatusCode::CREATED);
    let pair: serde_json::Value =
        serde_json::from_slice(&pair.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let id = pair["id"].as_str().unwrap();
    let challenge = pair["challenge"].as_str().unwrap();
    let mut approval = Request::builder()
        .method("POST")
        .uri("/remote-auth/owner/approve")
        .header("x-hotsheet-secret", SECRET)
        .header("content-type", "application/json")
        .body(Body::from(json!({"id":id,"role":"read_only"}).to_string()))
        .unwrap();
    approval.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
    ));
    assert_eq!(
        router.clone().oneshot(approval).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    let redeemed = router
        .clone()
        .oneshot(remote_request(
            "POST",
            "/remote-auth/redeem",
            None,
            None,
            Some(json!({"id":id,"challenge":challenge})),
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
    let cookie = cookies.join("; ");
    let csrf = cookies
        .iter()
        .find_map(|value| value.strip_prefix("hs2_remote_csrf="))
        .unwrap();
    assert_eq!(
        router
            .clone()
            .oneshot(remote_request(
                "GET",
                "/checkouts",
                Some(&cookie),
                None,
                None
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        router
            .clone()
            .oneshot(remote_request(
                "POST",
                "/checkouts",
                Some(&cookie),
                Some(csrf),
                Some(json!({"root":"/tmp/synthetic"}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut revoke = Request::builder()
        .method("POST")
        .uri("/remote-auth/owner/revoke-all")
        .header("x-hotsheet-secret", SECRET)
        .body(Body::empty())
        .unwrap();
    revoke.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
    ));
    assert_eq!(
        router.clone().oneshot(revoke).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        router
            .oneshot(remote_request(
                "GET",
                "/checkouts",
                Some(&cookie),
                None,
                None
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn hs2_w4jjje_real_websocket_upgrade_refuses_off_box_bearer() {
    let (_dir, state) = fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let local = tokio::spawn(async move {
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let url = format!("ws://{addr}/ws/sync?secret={SECRET}");
    let mut request = url.clone().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", "http://localhost:4175".parse().unwrap());
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    drop(socket);
    local.abort();

    // CI may have no routable second interface. A real bound Hyper upgrade carries an
    // explicit off-box accepted-peer extension through the same service boundary.
    let (_dir, state) = fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let remote = tokio::spawn(async move {
        axum::serve(
            listener,
            app(state).layer(Extension(ConnectInfo(
                "192.0.2.20:12345".parse::<std::net::SocketAddr>().unwrap(),
            ))),
        )
        .await
        .unwrap();
    });
    for path in [
        format!("ws://{addr}/ws/sync?secret={SECRET}"),
        format!("ws://{addr}/terminals/synthetic/attach?secret={SECRET}"),
    ] {
        let mut request = path.into_client_request().unwrap();
        request
            .headers_mut()
            .insert("origin", "http://localhost:4175".parse().unwrap());
        let result = tokio_tungstenite::connect_async(request).await;
        match result {
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            }
            other => panic!("off-box bearer must not upgrade: {other:?}"),
        }
    }
    remote.abort();
}
