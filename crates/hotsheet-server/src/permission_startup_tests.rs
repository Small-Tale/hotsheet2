use super::{AppState, app};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use std::io;
use std::sync::mpsc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

fn state() -> (tempfile::TempDir, AppState) {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init(directory.path(), &StoreMetadata::new("HS")).unwrap();
    let rules = directory.path().join("rules.json");
    let state = AppState::new(store, "synthetic-secret".into())
        .unwrap()
        .with_permission_rules_deferred(rules);
    (directory, state)
}

fn request(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("x-hotsheet-secret", "synthetic-secret")
        .body(Body::empty())
        .unwrap()
}

fn answer(id: u64, scope: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/permissions/{id}"))
        .header("x-hotsheet-secret", "synthetic-secret")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            "{{\"decision\":\"allow\",\"scope\":\"{scope}\"}}"
        )))
        .unwrap()
}

async fn pending_id(state: &AppState) -> u64 {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(request) = state.permissions.pending().first() {
                return request.id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

async fn first_permission(router: &axum::Router) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(request("/permissions"))
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()[0].clone()
}

/// HS2-Q7R2RM: a blocked credential lookup cannot hold the listener or let a
/// permission settings route repeat that lookup on an HTTP worker.
#[tokio::test]
async fn stalled_vault_loader_leaves_http_available_and_rules_locked() {
    let (directory, state) = state();
    let extra = directory.path().join("extra-store");
    FsStore::init(&extra, &StoreMetadata::new("EX")).unwrap();
    std::fs::write(
        directory.path().join("stores.json"),
        serde_json::json!({ "stores": [extra] }).to_string(),
    )
    .unwrap();
    let state = state.with_machine_home(directory.path());
    assert_eq!(state.host_configured_stores_deferred(), 1);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (loaded_tx, loaded_rx) = mpsc::channel();
    let primary_path = state.permission_rule_paths.lock().unwrap()[""].clone();
    let loader = state.spawn_permission_rules_load_with(move |path| {
        loaded_tx.send(path.to_path_buf()).unwrap();
        if path == primary_path {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        }
        Ok(([7; 32], Vec::new()))
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(state.permissions.durable_rules_locked());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = app(state.clone());
    let server_router = router.clone();
    let server = tokio::spawn(async move { axum::serve(listener, server_router).await.unwrap() });
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nX-Hotsheet-Secret: synthetic-secret\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
    server.abort();
    let health = router.clone().oneshot(request("/health")).await.unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    let rules = router.oneshot(request("/permissions/rules")).await.unwrap();
    assert_eq!(rules.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bridge = state.permissions.clone();
    let waiter = std::thread::spawn(move || bridge.request_blocking("worker", "Bash", "ls"));
    let id = pending_id(&state).await;
    assert_eq!(
        first_permission(&app(state.clone())).await["always_allow_supported"],
        false
    );
    let early_always = app(state.clone())
        .oneshot(answer(id, "always"))
        .await
        .unwrap();
    assert_eq!(early_always.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(state.permissions.pending()[0].id, id);
    let once = app(state.clone())
        .oneshot(answer(id, "once"))
        .await
        .unwrap();
    assert_eq!(once.status(), StatusCode::OK);
    assert_eq!(
        waiter.join().unwrap(),
        hotsheet_aitools::PermissionDecision::Allow
    );
    let mut events = state.subscribe();
    release_tx.send(()).unwrap();
    loader.join().unwrap();
    let loaded = loaded_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(
        loaded.len(),
        2,
        "primary and configured store vaults loaded"
    );
    {
        let paths = state.permission_rule_paths.lock().unwrap();
        assert!(loaded.contains(&paths[""]));
        assert!(loaded.contains(&paths[&extra.display().to_string()]));
    }
    assert_eq!(events.try_recv().unwrap().kind, "permission_rules_changed");
    assert!(!state.permissions.durable_rules_locked());
    assert!(
        !state
            .permission_rules_loading
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let bridge = state.permissions.clone();
    let waiter = std::thread::spawn(move || bridge.request_blocking("worker-2", "Bash", "pwd"));
    let id = pending_id(&state).await;
    assert_eq!(
        first_permission(&app(state.clone())).await["always_allow_supported"],
        true
    );
    let once = app(state.clone())
        .oneshot(answer(id, "once"))
        .await
        .unwrap();
    assert_eq!(once.status(), StatusCode::OK);
    assert_eq!(
        waiter.join().unwrap(),
        hotsheet_aitools::PermissionDecision::Allow
    );
}

/// HS2-Q7R2RM: a credential failure keeps durable approvals unavailable.
#[test]
fn failed_vault_loader_stays_closed() {
    let (_directory, state) = state();
    state
        .spawn_permission_rules_load_with(|_| Err(io::Error::other("synthetic credential failure")))
        .join()
        .unwrap();
    assert!(state.permissions.durable_rules_locked());
    assert!(
        !state
            .permission_rules_loading
            .load(std::sync::atomic::Ordering::Acquire)
    );
}
