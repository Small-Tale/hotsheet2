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
use hotsheet_ticketing::screensaver::{self, Locations, SNAPSHOT_FILE, SNAPSHOT_VALID_MS};
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

/// Removes the snapshot directory the router test opts into, even when it fails.
struct OptIn(std::path::PathBuf);
impl Drop for OptIn {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_mzm988_the_running_router_keeps_the_snapshot_current_from_live_state_events() {
    // This test writes where the server writes in production, so it runs only against the isolated
    // home `.cargo/config.toml` forces, never the developer's real screen saver container.
    let locations = screensaver::locations_from_env().expect("a saver home");
    let dir = locations.snapshot_dirs()[0].clone();
    assert!(
        dir.to_string_lossy().contains("test-screensaver-home"),
        "refusing to write outside the isolated saver home: {}",
        dir.display()
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let _cleanup = OptIn(dir.clone());

    let h = harness();
    let router = app(h.state.clone());
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(h.state.clone()).await;
    start_work(&router).await;

    // A pushed live_state reaches the file without anyone asking for it.
    let snapshot = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(bytes) = std::fs::read(dir.join(SNAPSHOT_FILE))
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                && value["live"]["stores"][0]["state"] == "active"
            {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the router wrote the active state to the snapshot");
    assert_eq!(snapshot["version"], 1);
    assert_eq!(snapshot["live"]["stores"][0]["store"], h.store_id.as_str());
    assert!(
        snapshot["valid_until_ms"].as_i64().unwrap() > snapshot["written_at_ms"].as_i64().unwrap(),
        "the snapshot is vouched for ahead of its own write"
    );
}
