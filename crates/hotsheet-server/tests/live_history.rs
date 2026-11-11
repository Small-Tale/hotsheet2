//! HS2-9AN4DN E2E: real server writes move a store through live states; each published
//! transition is appended to the store's `metrics/local/live-history.jsonl`, and
//! `GET /live-history` replays it into segments with the window's completion ticks.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::{AppState, ChangeEvent, app};
use hotsheet_ticketing::live_history::{HistoryState, history_path, read_records};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "test-secret";

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

async fn next_live(
    events: &mut tokio::sync::broadcast::Receiver<ChangeEvent>,
    store: &str,
    want: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = events.recv().await.expect("change bus open");
            if event.kind != "live_state" || event.store != store {
                continue;
            }
            let live = serde_json::to_value(event.live.expect("snapshot")).unwrap();
            if want(&live) {
                return live;
            }
        }
    })
    .await
    .expect("the expected live state was pushed")
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_9an4dn_published_transitions_persist_and_replay_with_completion_ticks() {
    hotsheet_aitools::enable_test_permission_vault();
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(dir.path().join("demo.hs2"), &StoreMetadata::new("HS")).unwrap();
    let root = store.root().to_path_buf();
    let store_id = hotsheet_server::multistore::store_url_id(&store);
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_machine_home(dir.path().join("machine-home"))
        .with_project_idle_grace(Duration::from_millis(300));
    let router = app(state.clone());
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(state.clone()).await;
    let mut events = state.subscribe();

    let (_, created) = call(
        &router,
        "POST",
        "/tickets",
        Some(json!({"title":"Night work"})),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    let slug = created["slug"].as_str().unwrap().to_string();
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w1","lease_minutes":30})),
    )
    .await;
    next_live(&mut events, &store_id, |live| live["state"] == "active").await;
    // The worker waits on remote CI: the timeline reads Waiting on CI.
    call(
        &router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({"started_phase":"final_testing"})),
    )
    .await;
    next_live(&mut events, &store_id, |live| {
        live["claims"][0]["phase"] == "final_testing"
    })
    .await;
    // Completing it ends the work; the tracker stops after its grace.
    call(
        &router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({"status":"completed","note":"Done.","note_confidence":90,"actor_role":"ai"})),
    )
    .await;
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/release"),
        Some(json!({"worker":"w1"})),
    )
    .await;
    next_live(&mut events, &store_id, |live| live["state"] == "stopped").await;

    let states: Vec<HistoryState> = read_records(&history_path(&root))
        .unwrap()
        .into_iter()
        .map(|record| record.state)
        .collect();
    assert!(
        states.windows(2).all(|pair| pair[0] != pair[1]),
        "only changes are recorded: {states:?}"
    );
    for wanted in [
        HistoryState::Active,
        HistoryState::WaitingOnCi,
        HistoryState::Stopped,
    ] {
        assert!(
            states.contains(&wanted),
            "{wanted:?} recorded in {states:?}"
        );
    }
    assert_eq!(states.last(), Some(&HistoryState::Stopped));
    let ignored = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(
        ignored.lines().any(|line| line.trim() == "metrics/local/"),
        "the machine-local history stays out of the store's Git history"
    );

    let (status, body) = call(&router, "GET", "/live-history", None).await;
    assert_eq!(status, StatusCode::OK);
    let lane = &body["stores"][0];
    assert_eq!(lane["store"], store_id.as_str());
    let segments = lane["segments"].as_array().unwrap();
    assert_eq!(segments.last().unwrap()["state"], "stopped");
    assert_eq!(
        segments.last().unwrap()["end_ms"],
        body["until_ms"],
        "the current state runs to the end of the window"
    );
    for pair in segments.windows(2) {
        assert_eq!(
            pair[0]["end_ms"], pair[1]["start_ms"],
            "segments are contiguous"
        );
    }
    assert_eq!(lane["completions"][0]["slug"], slug.as_str());

    // A window before anything was recorded is empty, and an inverted one is refused.
    let since = body["since_ms"].as_i64().unwrap();
    let (_, early) = call(
        &router,
        "GET",
        &format!("/live-history?since_ms={}&until_ms={}", since - 1000, since),
        None,
    )
    .await;
    assert!(
        early["stores"][0]["segments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        early["stores"][0]["completions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let (status, _) = call(
        &router,
        "GET",
        &format!("/live-history?since_ms={since}&until_ms={since}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
