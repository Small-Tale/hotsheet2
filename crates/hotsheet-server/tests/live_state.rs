//! HS2-JTTQMY E2E: the per-store live state follows real server writes through the running
//! notification router, is pushed as cursor-replayable `live_state` change events, and is
//! read back through `GET /live-state` and `GET /stores/{store_id}/live-state`. No
//! notification rule is configured: the claim tracker feeds the live state regardless.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::{AppState, ChangeEvent, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "test-secret";

struct Harness {
    _dir: tempfile::TempDir,
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
        _dir: dir,
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

/// The next pushed `live_state` for the store that satisfies `want`.
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
            let live =
                serde_json::to_value(event.live.expect("live_state carries a snapshot")).unwrap();
            if want(&live) {
                return live;
            }
        }
    })
    .await
    .expect("the expected live state was pushed")
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_jttqmy_live_state_follows_claims_release_and_feedback_without_rules() {
    let h = harness();
    let router = app(h.state.clone());
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(h.state.clone()).await;
    let mut events = h.state.subscribe();
    let (_, start) = call(&router, "GET", "/ws/poll?timeout_ms=0", None).await;
    let since = start["cursor"].as_u64().unwrap();

    let (status, created) = call(
        &router,
        "POST",
        "/tickets",
        Some(json!({"title":"Glance work"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap().to_string();
    let slug = created["slug"].as_str().unwrap().to_string();
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w1","lease_minutes":30,"eta":"45m"})),
    )
    .await;
    let active = next_live(&mut events, &h.store_id, |live| live["state"] == "active").await;
    assert_eq!(active["claims"][0]["slug"], slug.as_str());
    assert_eq!(active["claims"][0]["worker"], "w1");
    assert!(
        active["claims"][0]["eta_at_ms"].as_i64().unwrap()
            > active["claims"][0]["claimed_at_ms"].as_i64().unwrap()
    );
    assert_eq!(active["workers"], json!({"live":1,"expected":1}));
    assert_eq!(active["rates"]["tokens"]["window_minutes"], 5);
    assert_eq!(active["rates"]["tickets_updated"]["window_minutes"], 60);
    assert!(
        !active["rates"]["tickets_updated"]["at_ms"]
            .as_array()
            .unwrap()
            .is_empty(),
        "creating the ticket is a progress write"
    );

    // The REST reads return the pushed snapshot and revision.
    let (status, one) = call(
        &router,
        "GET",
        &format!("/stores/{}/live-state", h.store_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["state"], "active");
    assert!(one["revision"].as_u64().unwrap() >= active["revision"].as_u64().unwrap());
    let (_, all) = call(&router, "GET", "/live-state", None).await;
    assert_eq!(all["stores"].as_array().unwrap().len(), 1);
    assert_eq!(all["stores"][0]["store"], h.store_id.as_str());
    assert_eq!(all["epoch_ms"], one["epoch_ms"]);
    let (status, _) = call(&router, "GET", "/stores/no-such-store/live-state", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A feedback request outranks Active: Waiting on you.
    call(
        &router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({"note":"Pick A or B?","note_kind":"feedback_needed"})),
    )
    .await;
    let waiting = next_live(&mut events, &h.store_id, |live| {
        live["state"] == "waiting_on_you"
    })
    .await;
    assert_eq!(waiting["activity"]["word"], "active");
    assert_eq!(waiting["waiting"]["count"], 1);
    assert_eq!(waiting["waiting"]["tickets"][0]["reason"], "feedback");

    // Release: the grace still reads Active underneath, then the tracker stops. The open
    // feedback request still outranks Stopped.
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/release"),
        Some(json!({"worker":"w1"})),
    )
    .await;
    let stopped = next_live(&mut events, &h.store_id, |live| {
        live["activity"]["word"] == "stopped"
    })
    .await;
    assert_eq!(stopped["state"], "waiting_on_you");
    assert!(stopped["claims"].as_array().unwrap().is_empty());
    assert_eq!(stopped["workers"], json!({"live":0,"expected":0}));
    assert!(stopped["activity"]["stopped_at_ms"].is_i64());
    // Closing the ticket ends the wait: Stopped.
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/close"),
        Some(json!({"reason":"not_planned"})),
    )
    .await;
    let stopped = next_live(&mut events, &h.store_id, |live| live["state"] == "stopped").await;
    assert_eq!(stopped["waiting"]["count"], 0);

    // Cursor replay: a client reconnecting from before the claim sees every pushed snapshot.
    let (_, replay) = call(
        &router,
        "GET",
        &format!("/ws/poll?since={since}&timeout_ms=0"),
        None,
    )
    .await;
    let states: Vec<&str> = replay["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "live_state")
        .map(|event| event["live"]["state"].as_str().unwrap())
        .collect();
    assert!(
        states.contains(&"active") && states.contains(&"waiting_on_you"),
        "{states:?}"
    );
    assert_eq!(states.last(), Some(&"stopped"));
    let revisions: Vec<u64> = replay["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "live_state")
        .map(|event| event["live"]["revision"].as_u64().unwrap())
        .collect();
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "{revisions:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_jttqmy_an_unrenewed_lease_reads_stalled_and_an_unchanged_state_pushes_nothing() {
    let h = harness();
    let router = app(h.state.clone());
    let (_, created) = call(&router, "POST", "/tickets", Some(json!({"title":"Stalls"}))).await;
    let id = created["id"].as_str().unwrap().to_string();
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w1","lease_minutes":1})),
    )
    .await;
    let state = h.state.clone();
    let store = h.store_id.clone();
    let (claimed, again, stalled, quiet) = tokio::task::spawn_blocking(move || {
        // Drive the router synchronously, as the bus would.
        let claim = ChangeEvent {
            live: None,
            cursor: None,
            store: store.clone(),
            kind: "claimed".into(),
            id: id.clone(),
            slug: String::new(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        };
        hotsheet_server::notification_router::route_change_event(&state, &claim);
        let now = time::OffsetDateTime::now_utc();
        let claimed = hotsheet_server::live_state::refresh_at(&state, &store, now).unwrap();
        let mut bus = state.subscribe();
        let again = hotsheet_server::live_state::refresh_at(&state, &store, now).unwrap();
        let quiet_after_repeat = bus.try_recv().is_err();
        let later = now + time::Duration::minutes(2);
        hotsheet_server::notification_router::check_project_activity(&state, &store, later);
        let stalled = hotsheet_server::live_state::refresh_at(&state, &store, later).unwrap();
        (claimed, again, stalled, quiet_after_repeat)
    })
    .await
    .unwrap();
    assert_eq!(serde_json::to_value(claimed.state).unwrap(), "active");
    assert_eq!(
        again.revision, claimed.revision,
        "nothing changed, nothing pushed"
    );
    assert!(quiet, "an unchanged rebuild emits no event");
    assert_eq!(serde_json::to_value(stalled.state).unwrap(), "stalled");
    assert!(stalled.revision > claimed.revision);
    assert_eq!(stalled.activity.stalled[0].worker, "w1");
    assert_eq!(stalled.workers.expected, 1);
    assert_eq!(stalled.workers.live, 0);
}
