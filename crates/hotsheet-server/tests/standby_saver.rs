//! HS2-MZM988 E2E: the server mirrors the live state clients receive into the credential-free
//! snapshot file the macOS screen saver watches. It writes only into a snapshot directory that
//! exists, never rebuilds or pushes live state to do it, and the running notification router keeps
//! it current from the `live_state` events it already sees.

use std::path::Path;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::standby_saver::write_to;
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::screensaver::{Locations, SNAPSHOT_FILE, SNAPSHOT_VALID_MS};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "test-secret";

struct Harness {
    dir: tempfile::TempDir,
    store_id: String,
    state: AppState,
}

fn harness() -> Harness {
    hotsheet_aitools::enable_test_permission_vault();
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(dir.path().join("demo.hs2"), &StoreMetadata::new("HS")).unwrap();
    let store_id = hotsheet_server::multistore::store_url_id(&store);
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_machine_home(dir.path().join("machine-home"))
        .with_project_idle_grace(Duration::from_millis(300));
    Harness {
        dir,
        store_id,
        state,
    }
}

async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("X-Hotsheet-Secret", SECRET);
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = router
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn read_snapshot(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(SNAPSHOT_FILE)).unwrap()).unwrap()
}

/// Create a ticket and claim it, so the store has a live, active state.
async fn start_work(router: &axum::Router) {
    let (status, created) = call(
        router,
        "POST",
        "/tickets",
        Some(json!({"title": "Saver work"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();
    call(
        router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker": "w1", "lease_minutes": 30})),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_mzm988_the_snapshot_mirrors_the_live_state_clients_read_and_pushes_nothing() {
    let h = harness();
    let router = app(h.state.clone());
    let locations = Locations::for_home(h.dir.path().join("home"));
    let dir = locations.snapshot_dirs()[0].clone();

    // Nothing is written, and nothing is created, until a snapshot directory exists.
    start_work(&router).await;
    let (_, live) = call(&router, "GET", "/live-state", None).await;
    assert_eq!(write_to(&h.state, &locations, 1_000).written, 0);
    assert!(!h.dir.path().join("home").exists());

    std::fs::create_dir_all(&dir).unwrap();
    let cursor = h.state.subscribe();
    let before = (cursor.len(), live.clone());
    assert_eq!(write_to(&h.state, &locations, 2_000).written, 1);
    let snapshot = read_snapshot(&dir);
    assert_eq!(snapshot["version"], 1);
    assert_eq!(snapshot["written_at_ms"], 2_000);
    assert_eq!(snapshot["valid_until_ms"], 2_000 + SNAPSHOT_VALID_MS);
    // It is the list `GET /live-state` returns: same epoch, same store, same revision, same state.
    assert_eq!(snapshot["live"]["epoch_ms"], live["epoch_ms"]);
    assert_eq!(snapshot["live"]["stores"], live["stores"]);
    assert_eq!(snapshot["live"]["stores"][0]["store"], h.store_id.as_str());
    assert_eq!(snapshot["live"]["stores"][0]["state"], "active");
    // Mirroring is read-only: no live_state event came of it and the read did not move a revision.
    assert_eq!(
        cursor.len(),
        before.0,
        "writing the snapshot pushed an event"
    );
    let (_, again) = call(&router, "GET", "/live-state", None).await;
    assert_eq!(again["stores"], before.1["stores"]);

    // A later write moves the vouching window forward and replaces the file whole.
    assert_eq!(write_to(&h.state, &locations, 62_000).written, 1);
    let later = read_snapshot(&dir);
    assert_eq!(later["written_at_ms"], 62_000);
    assert_eq!(later["valid_until_ms"], 62_000 + SNAPSHOT_VALID_MS);
    assert_eq!(later["live"]["stores"], snapshot["live"]["stores"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_mzm988_an_empty_server_still_leaves_a_valid_snapshot() {
    let h = harness();
    let locations = Locations::for_home(h.dir.path().join("home"));
    let dir = locations.snapshot_dirs()[1].clone();
    std::fs::create_dir_all(&dir).unwrap();
    // No store has been built yet, so none is reported rather than an invented one.
    assert_eq!(write_to(&h.state, &locations, 5).written, 1);
    let snapshot = read_snapshot(&dir);
    assert_eq!(snapshot["version"], 1);
    assert_eq!(snapshot["live"]["stores"], json!([]));
}

/// Read the snapshot in `dir` once it reports `state` for its first store, waking on file
/// events in `dir` rather than on a timer.
fn wait_for_snapshot_state(dir: &Path, state: &str) -> Value {
    use notify::Watcher as _;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    })
    .unwrap();
    watcher
        .watch(dir, notify::RecursiveMode::NonRecursive)
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut last = None;
    loop {
        // Checked after the watch starts, so a write that landed before it is still seen.
        if let Ok(bytes) = std::fs::read(dir.join(SNAPSHOT_FILE))
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        {
            if value["live"]["stores"][0]["state"] == state {
                return value;
            }
            last = Some(value);
        }
        if let Err(error) =
            rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        {
            panic!(
                "the router never wrote state {state:?} to the snapshot ({error}); last snapshot: {last:?}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_mzm988_the_running_router_keeps_the_snapshot_current_from_live_state_events() {
    // HS2-MC2PSC: this server's saver locations are its own. `.cargo/config.toml` forces one saver
    // home for every test process, so under suite load another test's server overwrote this
    // snapshot after the live_state write, and the next write was a minute away.
    let h = harness();
    let locations = Locations::for_home(h.dir.path().join("saver-home"));
    let dir = locations.snapshot_dirs()[0].clone();
    std::fs::create_dir_all(&dir).unwrap();
    let state = h
        .state
        .clone()
        .with_screensaver_home(h.dir.path().join("saver-home"));
    let router = app(state.clone());
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(state.clone()).await;

    // A pushed live_state reaches the file without anyone asking for it. Without the server's own
    // saver home this directory does not exist for it, so nothing would arrive.
    let waiter = {
        let dir = dir.clone();
        tokio::task::spawn_blocking(move || wait_for_snapshot_state(&dir, "active"))
    };
    start_work(&router).await;
    let snapshot = waiter.await.unwrap();
    assert_eq!(snapshot["version"], 1);
    assert_eq!(snapshot["live"]["stores"][0]["store"], h.store_id.as_str());
    assert!(
        snapshot["valid_until_ms"].as_i64().unwrap() > snapshot["written_at_ms"].as_i64().unwrap(),
        "the snapshot is vouched for ahead of its own write"
    );
}
