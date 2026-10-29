//! HS2-R5HTZN E2E: notifier administration over HTTP, and real change-bus events (ticket
//! transitions, permission requests, published notifications) delivered through the
//! running notification router to a faithful fake ntfy transport.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_notify::builtin::ntfy::Ntfy;
use hotsheet_notify::http::{HttpRequest, HttpResponse, HttpTransport};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "test-secret";

/// Records each request and wakes the waiting test, replying like ntfy.sh does.
struct SignallingHttp {
    sent: Mutex<tokio::sync::mpsc::UnboundedSender<HttpRequest>>,
}

impl HttpTransport for SignallingHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let _ = self.sent.lock().unwrap().send(request.clone());
        Ok(HttpResponse {
            status: 200,
            body: r#"{"id":"ntfy-1","event":"message"}"#.into(),
        })
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    home: std::path::PathBuf,
    store_id: String,
    state: AppState,
    received: tokio::sync::mpsc::UnboundedReceiver<HttpRequest>,
}

fn harness() -> Harness {
    hotsheet_aitools::enable_test_permission_vault();
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path().join("demo.hs2"), &StoreMetadata::new("HS")).unwrap();
    let home = dir.path().join("machine-home");
    let store_id = hotsheet_server::multistore::store_url_id(&store);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let http = Arc::new(SignallingHttp {
        sent: Mutex::new(tx),
    });
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_machine_home(&home)
        .with_notifiers(vec![Arc::new(Ntfy::with_transport(http))])
        // A short grace keeps the project-stopped E2E fast (HS2-2MM07G).
        .with_project_idle_grace(Duration::from_millis(300));
    Harness {
        _dir: dir,
        home,
        store_id,
        state,
        received: rx,
    }
}

fn authed(method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET);
    if body.is_some() {
        b = b.header(header::CONTENT_TYPE, "application/json");
    }
    b.body(body.map_or(Body::empty(), |v| Body::from(v.to_string())))
        .unwrap()
}

async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(authed(method, uri, body))
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// Wait without occupying a Tokio worker; a one-worker runtime must still route the event.
async fn next(h: &mut Harness) -> HttpRequest {
    tokio::time::timeout(Duration::from_secs(20), h.received.recv())
        .await
        .expect("the router delivered a notification")
        .expect("the notification transport stayed open")
}

async fn configure(router: &axum::Router, events: Value) {
    let (status, body) = call(
        router,
        "PUT",
        "/notification-channels/phone",
        Some(json!({"notifier":"ntfy","label":"Phone","settings":{"topic":"hs-e2e"}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = call(
        router,
        "PUT",
        "/notification-rules/everything",
        Some(json!({"events": events, "channels":["phone"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_routes_validate_store_secrets_by_reference_and_test_channels() {
    let mut h = harness();
    let router = app(h.state.clone());

    let (status, list) = call(&router, "GET", "/notifiers", None).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<_> = list["notifiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["ntfy"]);
    assert_eq!(
        list["notifiers"][0]["capabilities"]["formats"],
        json!(["markdown", "plain"])
    );

    let (status, body) = call(
        &router,
        "PUT",
        "/notification-channels/phone",
        Some(json!({"notifier":"ntfy","settings":{"topic":"t","token":"raw-token"}})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("credential reference"),
        "{body}"
    );

    configure(&router, json!(["ticket.blocked"])).await;
    let channel = format!("phone-{}", ulid::Ulid::new().to_string().to_lowercase());
    let (status, _) = call(
        &router,
        "PUT",
        &format!("/notification-channels/{channel}"),
        Some(json!({"notifier":"ntfy","settings":{"topic":"hs-e2e"}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &router,
        "PUT",
        &format!("/notification-channels/{channel}/secrets/token"),
        Some(json!({"value":"tk-e2e"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, config) = call(&router, "GET", "/notification-config", None).await;
    assert!(!config.to_string().contains("tk-e2e"), "{config}");
    let stored = &config["channels"][1]["settings"]["token"]["secret"];
    assert_eq!(stored, &json!(format!("notify-{channel}-token")));

    let (status, record) = call(
        &router,
        "POST",
        &format!("/notification-channels/{channel}/test"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{record}");
    assert_eq!(record["outcome"], "delivered");
    assert_eq!(record["event"], "test");
    let sent = next(&mut h).await;
    assert_eq!(header(&sent, "Authorization"), Some("Bearer tk-e2e"));
    assert_eq!(header(&sent, "Title"), Some("Hot Sheet test notification"));

    let (status, deliveries) = call(&router, "GET", "/notification-deliveries?limit=5", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deliveries[0]["channel"], json!(channel));
    assert!(
        !deliveries.to_string().contains("test notification"),
        "the log holds no message text"
    );

    let (status, removal) = call(
        &router,
        "DELETE",
        &format!("/notification-channels/{channel}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        removal["deleted_secrets"],
        json!([format!("notify-{channel}-token")])
    );
    // HS2-208RB1: a built-in channel needs no approval; trusting it records none.
    let (status, trusted) = call(&router, "POST", "/notification-channels/phone/trust", None).await;
    assert_eq!(status, StatusCode::OK, "{trusted}");
    assert!(trusted.get("plugin_digest").is_none(), "{trusted}");
    let (status, _) = call(
        &router,
        "POST",
        "/notification-channels/missing/trust",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&router, "POST", "/notification-channels/missing/test", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&router, "DELETE", "/notification-rules/missing", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&router, "DELETE", "/notification-rules/everything", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(h.home.join("notifications.json").is_file());
}

#[tokio::test(flavor = "multi_thread")]
async fn ticket_transitions_permission_requests_and_published_notifications_reach_channels() {
    let mut h = harness();
    let router = app(h.state.clone());
    configure(
        &router,
        json!([
            "ticket.status_changed",
            "ticket.blocked",
            "ticket.feedback_needed",
            "permission.requested",
            "notification"
        ]),
    )
    .await;
    let (_, created) = call(
        &router,
        "POST",
        "/tickets",
        Some(json!({"title":"Ship the *notifier*"})),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    let slug = created["slug"].as_str().unwrap().to_string();
    // The router has seeded its snapshots when this returns, so the status write below is
    // a transition from the seeded state.
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(h.state.clone()).await;

    // HS2-04J8WS: a configured client origin turns into a click-through deep link.
    let (status, _) = call(
        &router,
        "PUT",
        "/notification-link-base",
        Some(json!({"url":"http://hotsheet.lan:4174/"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &router,
        "PUT",
        "/notification-link-base",
        Some(json!({"url":"ftp://nope"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({"status":"started"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let sent = next(&mut h).await;
    let click = header(&sent, "Click").unwrap();
    assert!(
        click.starts_with("http://hotsheet.lan:4174/?project="),
        "{click}"
    );
    assert!(click.contains("demo.hs2&ticket="), "{click}");
    assert!(click.ends_with(&format!("&ticket={slug}")), "{click}");
    assert_eq!(sent.url, "https://ntfy.sh/hs-e2e");
    assert_eq!(
        header(&sent, "Title"),
        Some(format!("{slug} is now Started").as_str())
    );
    let body = String::from_utf8(sent.body.clone()).unwrap();
    assert!(body.contains("Ship the \\*notifier\\*"), "{body}");
    assert!(body.contains("Not Started → Started"), "{body}");

    // A write that changes neither status, feedback, nor blockers notifies nobody, so the
    // next delivery must be the blocked transition below.
    call(
        &router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({"title":"Renamed"})),
    )
    .await;
    call(
        &router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({"blocked_reason":"waiting on review"})),
    )
    .await;
    let sent = next(&mut h).await;
    assert_eq!(
        header(&sent, "Title"),
        Some(format!("{slug} is blocked").as_str())
    );
    assert_eq!(header(&sent, "Priority"), Some("4"));
    assert!(
        String::from_utf8(sent.body)
            .unwrap()
            .contains("waiting on review")
    );

    call(
        &router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({"note":"Pick A or B?","note_kind":"feedback_needed"})),
    )
    .await;
    let sent = next(&mut h).await;
    assert_eq!(
        header(&sent, "Title"),
        Some(format!("{slug} needs your feedback").as_str())
    );

    let (status, _) = call(
        &router,
        "POST",
        "/notifications",
        Some(json!({"message":"Build finished\nAll **green**","severity":"success","dedupe_key":"build-1"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let sent = next(&mut h).await;
    assert_eq!(header(&sent, "Title"), Some("Build finished"));
    assert_eq!(header(&sent, "Tags"), Some("white_check_mark"));
    assert_eq!(String::from_utf8(sent.body).unwrap(), "All **green**");

    // Records are appended after each send returns, so the synchronous admin test covers
    // the log; here each delivery was observed at the transport, in order.
    assert!(h.received.try_recv().is_err(), "no extra deliveries");
}

#[test]
fn permission_requests_and_unconfigured_events_route_synchronously() {
    let mut h = harness();
    let event = |kind: &str| hotsheet_server::ChangeEvent {
        cursor: None,
        store: "/work/demo.hs2".into(),
        kind: kind.into(),
        id: "perm-1".into(),
        slug: "claude".into(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    };
    // No configuration: nothing is routed and nothing is written.
    assert!(
        hotsheet_server::notification_router::route_change_event(
            &h.state,
            &event("permission_asked")
        )
        .is_empty()
    );
    let registry = hotsheet_notify::Registry::builtin_only();
    hotsheet_notify::admin::put_channel(
        &h.home,
        &registry,
        hotsheet_notify::ChannelConfig {
            id: "phone".into(),
            notifier: "ntfy".into(),
            label: None,
            enabled: true,
            settings: serde_json::from_value(json!({"topic":"hs-e2e"})).unwrap(),
            plugin_digest: None,
            quiet_hours: None,
            max_per_hour: None,
        },
    )
    .unwrap();
    hotsheet_notify::admin::put_rule(
        &h.home,
        &registry,
        hotsheet_notify::Rule {
            id: "perm".into(),
            events: vec![hotsheet_notify::EventKind::PermissionRequested],
            channels: vec!["phone".into()],
            enabled: true,
            filter: Default::default(),
            cooldown_seconds: 600,
        },
    )
    .unwrap();
    let records = hotsheet_server::notification_router::route_change_event(
        &h.state,
        &event("permission_asked"),
    );
    assert_eq!(records.len(), 1);
    let sent = h.received.try_recv().expect("synchronous route delivered");
    assert_eq!(
        header(&sent, "Title"),
        Some("claude needs permission in demo")
    );
    // The same request again is inside the rule's cooldown.
    let records = hotsheet_server::notification_router::route_change_event(
        &h.state,
        &event("permission_asked"),
    );
    assert_eq!(records[0].outcome, hotsheet_notify::Outcome::Suppressed);
    assert!(
        hotsheet_server::notification_router::route_change_event(&h.state, &event("turn_event"))
            .is_empty()
    );
}

/// HS2-2MM07G: a real claim, completion, and release through the HTTP routes reach the
/// running router, which reports the stop after the idle grace with the completed ticket.
#[tokio::test(flavor = "multi_thread")]
async fn releasing_the_last_claim_reports_the_project_stopped_after_the_grace() {
    let mut h = harness();
    let router = app(h.state.clone());
    configure(&router, json!(["project.stopped"])).await;
    let (_, created) = call(
        &router,
        "POST",
        "/tickets",
        Some(json!({"title":"Wire it"})),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    let slug = created["slug"].as_str().unwrap().to_string();
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(h.state.clone()).await;

    let (status, _) = call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w1"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    call(
        &router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({"status":"completed"})),
    )
    .await;
    let (status, _) = call(
        &router,
        "POST",
        &format!("/tickets/{id}/release"),
        Some(json!({"worker":"w1"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let sent = next(&mut h).await;
    assert_eq!(
        header(&sent, "Title"),
        Some("Hot Sheet project demo has stopped working on tickets")
    );
    assert_eq!(header(&sent, "Priority"), Some("4"));
    let body = String::from_utf8(sent.body).unwrap();
    assert!(body.contains(&format!("Completed: {slug}.")), "{body}");
    // One report per active period: nothing else arrives.
    assert!(
        tokio::time::timeout(Duration::from_secs(1), h.received.recv())
            .await
            .is_err()
    );
}

/// HS2-2MM07G: a lease that expires without renewal reports the project stalled, naming
/// the ticket and worker; the clock is driven directly so the test needs no real expiry.
#[tokio::test(flavor = "multi_thread")]
async fn an_unrenewed_claim_reports_the_project_stalled() {
    let mut h = harness();
    let router = app(h.state.clone());
    configure(&router, json!(["project.stopped"])).await;
    let (_, created) = call(&router, "POST", "/tickets", Some(json!({"title":"Hang"}))).await;
    let id = created["id"].as_str().unwrap().to_string();
    let slug = created["slug"].as_str().unwrap().to_string();
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w-stuck","lease_minutes":1})),
    )
    .await;
    let event = hotsheet_server::ChangeEvent {
        cursor: None,
        store: h.store_id.clone(),
        kind: "claimed".into(),
        id: id.clone(),
        slug: slug.clone(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    };
    let state = h.state.clone();
    let store = h.store_id.clone();
    let records = tokio::task::spawn_blocking(move || {
        assert!(
            hotsheet_server::notification_router::route_change_event(&state, &event).is_empty()
        );
        let later = time::OffsetDateTime::now_utc() + time::Duration::minutes(2);
        hotsheet_server::notification_router::check_project_activity(&state, &store, later)
    })
    .await
    .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].event, hotsheet_notify::EventKind::ProjectStopped);
    let sent = next(&mut h).await;
    assert_eq!(
        header(&sent, "Title"),
        Some("Hot Sheet project demo has stalled")
    );
    assert_eq!(header(&sent, "Priority"), Some("5"));
    let body = String::from_utf8(sent.body).unwrap();
    assert!(
        body.contains(&format!("{slug}** (worker w-stuck)")),
        "{body}"
    );
}

/// HS2-4JG95G: a store-scoped event matches a rule that names a linked checkout, and the
/// checkout's alias is the project name in the message.
#[tokio::test(flavor = "multi_thread")]
async fn rules_match_linked_checkouts_and_use_their_alias() {
    let mut h = harness();
    let project = h._dir.path().join("rocket-src");
    std::fs::create_dir_all(&project).unwrap();
    let registry_path = h._dir.path().join("checkouts.json");
    let checkout = hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry_path)
        .register(
            &project,
            Some("Rocket"),
            None,
            vec![h._dir.path().join("demo.hs2")],
        )
        .unwrap();
    let state = h.state.clone().with_checkout_registry(&registry_path);
    let router = app(state.clone());
    let registry = hotsheet_notify::Registry::builtin_only();
    hotsheet_notify::admin::put_channel(
        &h.home,
        &registry,
        hotsheet_notify::ChannelConfig {
            id: "phone".into(),
            notifier: "ntfy".into(),
            label: None,
            enabled: true,
            settings: serde_json::from_value(json!({"topic":"hs-e2e"})).unwrap(),
            plugin_digest: None,
            quiet_hours: None,
            max_per_hour: None,
        },
    )
    .unwrap();
    let rule = |projects: Vec<String>| hotsheet_notify::Rule {
        id: "rocket".into(),
        events: vec![hotsheet_notify::EventKind::ProjectStopped],
        channels: vec!["phone".into()],
        enabled: true,
        filter: hotsheet_notify::RuleFilter {
            projects,
            ..Default::default()
        },
        cooldown_seconds: 0,
    };
    let (_, created) = call(&router, "POST", "/tickets", Some(json!({"title":"Launch"}))).await;
    let id = created["id"].as_str().unwrap().to_string();
    let stall = |projects: Vec<String>| {
        hotsheet_notify::admin::put_rule(&h.home, &registry, rule(projects)).unwrap();
        let state = state.clone();
        let store = h.store_id.clone();
        let event = hotsheet_server::ChangeEvent {
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
        move || {
            hotsheet_server::notification_router::route_change_event(&state, &event);
            let later = time::OffsetDateTime::now_utc() + time::Duration::minutes(2);
            hotsheet_server::notification_router::check_project_activity(&state, &store, later)
        }
    };
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w1","lease_minutes":1})),
    )
    .await;
    let other = tokio::task::spawn_blocking(stall(vec!["another-checkout".into()]))
        .await
        .unwrap();
    assert!(
        other.is_empty(),
        "a rule for another checkout does not match"
    );

    call(
        &router,
        "POST",
        &format!("/tickets/{id}/renew"),
        Some(json!({"worker":"w1","lease_minutes":1})),
    )
    .await;
    let records = tokio::task::spawn_blocking(stall(vec![checkout.id.clone()]))
        .await
        .unwrap();
    assert_eq!(
        records.len(),
        1,
        "the linked checkout id matches the store's event"
    );
    let sent = next(&mut h).await;
    assert_eq!(
        header(&sent, "Title"),
        Some("Hot Sheet project Rocket has stalled")
    );
}

fn claim_event(h: &Harness, kind: &str, id: &str) -> hotsheet_server::ChangeEvent {
    hotsheet_server::ChangeEvent {
        cursor: None,
        store: h.store_id.clone(),
        kind: kind.into(),
        id: id.into(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    }
}

/// Route `events`, then run the activity check `minutes` from now, off the runtime.
async fn route_then_check(
    h: &Harness,
    events: Vec<hotsheet_server::ChangeEvent>,
    minutes: i64,
) -> Vec<hotsheet_notify::DeliveryRecord> {
    let state = h.state.clone();
    let store = h.store_id.clone();
    tokio::task::spawn_blocking(move || {
        for event in &events {
            hotsheet_server::notification_router::route_change_event(&state, event);
        }
        let later = time::OffsetDateTime::now_utc() + time::Duration::minutes(minutes);
        hotsheet_server::notification_router::check_project_activity(&state, &store, later)
    })
    .await
    .unwrap()
}

/// HS2-P57XHV regression: a server-side renewal moves the lease, so a renewed claim is
/// not reported stalled at its original expiry.
#[tokio::test(flavor = "multi_thread")]
async fn a_renewed_claim_is_not_reported_stalled() {
    let h = harness();
    let router = app(h.state.clone());
    configure(&router, json!(["project.stopped"])).await;
    let registry = hotsheet_notify::Registry::builtin_only();
    hotsheet_notify::admin::set_project_timing(&h.home, &registry, None, Some(0)).unwrap();
    let (_, created) = call(&router, "POST", "/tickets", Some(json!({"title":"Long"}))).await;
    let id = created["id"].as_str().unwrap().to_string();
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w1","lease_minutes":1})),
    )
    .await;
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/renew"),
        Some(json!({"worker":"w1","lease_minutes":60})),
    )
    .await;
    let records = route_then_check(
        &h,
        vec![
            claim_event(&h, "claimed", &id),
            claim_event(&h, "renewed", &id),
        ],
        2,
    )
    .await;
    assert!(records.is_empty(), "{records:?}");
}

/// HS2-P57XHV: claims renewed without progress report "may be stuck" once.
#[tokio::test(flavor = "multi_thread")]
async fn held_claims_without_progress_report_the_project_may_be_stuck() {
    let mut h = harness();
    let router = app(h.state.clone());
    configure(&router, json!(["project.stopped"])).await;
    let (status, timing) = call(
        &router,
        "PUT",
        "/notification-project-timing",
        Some(json!({"idle_minutes":1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{timing}");
    assert_eq!(timing["idle_minutes"], 1);
    let (_, created) = call(&router, "POST", "/tickets", Some(json!({"title":"Spin"}))).await;
    let id = created["id"].as_str().unwrap().to_string();
    let slug = created["slug"].as_str().unwrap().to_string();
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w-spin","lease_minutes":60})),
    )
    .await;
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/renew"),
        Some(json!({"worker":"w-spin","lease_minutes":60})),
    )
    .await;
    let records = route_then_check(
        &h,
        vec![
            claim_event(&h, "claimed", &id),
            claim_event(&h, "renewed", &id),
        ],
        2,
    )
    .await;
    assert_eq!(records.len(), 1);
    let sent = next(&mut h).await;
    assert_eq!(
        header(&sent, "Title"),
        Some("Hot Sheet project demo may be stuck")
    );
    let body = String::from_utf8(sent.body).unwrap();
    assert!(
        body.contains(&format!("{slug}** (worker w-spin) is claimed")),
        "{body}"
    );
    // Reported once per idle episode.
    assert!(route_then_check(&h, Vec::new(), 3).await.is_empty());
}

/// HS2-P57XHV: a halt report from the terminal whose worker holds a claim reaches the
/// running router and names the claim and the halt reason.
// HS2-QXG1JQ: the router and the delivery wait must work with only one Tokio worker.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn a_halted_session_with_a_claim_reports_its_project() {
    let mut h = harness();
    let router = app(h.state.clone());
    configure(&router, json!(["project.stopped"])).await;
    let (_, created) = call(
        &router,
        "POST",
        "/tickets",
        Some(json!({"title":"Halting"})),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    let slug = created["slug"].as_str().unwrap().to_string();
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(h.state.clone()).await;
    let (status, _) = call(
        &router,
        "POST",
        "/terminals",
        Some(json!({"command":"cat","id":"halt-me"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"terminal-halt-me","lease_minutes":60})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &router,
        "POST",
        "/terminals/halt-me/halt",
        Some(json!({"error_type":"overloaded","message":"Model at capacity","agent":"claude"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let sent = next(&mut h).await;
    assert_eq!(
        header(&sent, "Title"),
        Some("Hot Sheet project demo: an AI session halted")
    );
    assert_eq!(header(&sent, "Priority"), Some("5"));
    let body = String::from_utf8(sent.body).unwrap();
    assert!(
        body.contains(&format!(
            "{slug}** (worker terminal-halt-me) halted: Model at capacity"
        )),
        "{body}"
    );
    call(&router, "DELETE", "/terminals/halt-me", None).await;
}
