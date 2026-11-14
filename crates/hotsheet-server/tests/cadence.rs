//! HS2-0BJ1A3: real checkout Workers response uses the shared cadence policy.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn request(method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", "synthetic-secret");
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(body.map_or(Body::empty(), |value| Body::from(value.to_owned())))
        .unwrap()
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn hs2_0bj1a3_policy_only_goal_change_reaches_real_workers_route() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let tickets = root.path().join("tickets");
    std::fs::create_dir_all(&project).unwrap();
    assert!(
        hotsheet_ticketing::git::launch("git")
            .args(["init", "-q"])
            .arg(&project)
            .status()
            .unwrap()
            .success()
    );
    let store = FsStore::init_git(&tickets, &StoreMetadata::new("SYN")).unwrap();
    let server = app(AppState::new(store, "synthetic-secret".to_owned())
        .unwrap()
        .with_default_codex_home(root.path().join("no-codex-home"))
        .with_checkout_registry(root.path().join("checkouts.json")));
    let opened = server
        .clone()
        .oneshot(request(
            "POST",
            "/projects/open",
            Some(&serde_json::json!({"root": project, "stores": [tickets]}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(opened.status(), StatusCode::CREATED);
    let id = json(opened).await["checkout"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let route = format!("/checkouts/{id}/workers?wait_ms=0");
    let before = json(
        server
            .clone()
            .oneshot(request("GET", &route, None))
            .await
            .unwrap(),
    )
    .await;
    assert!(before.get("cadence").is_none());
    let settings = hotsheet_ticketing::Settings::for_project(&project);
    let mut integration = hotsheet_cli::coordination::StrategyPolicy::read(&settings).unwrap();
    integration
        .allow(
            hotsheet_cli::coordination::IntegrationStrategy::CiStaged,
            "synthetic cadence",
        )
        .unwrap();
    integration.write(&settings).unwrap();
    let policy =
        serde_json::json!({"version": 1, "default": "steady-main", "green_main_goal_minutes": 90});
    let saved = server
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/checkouts/{id}/development-strategy"),
            Some(&policy.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    let after = json(
        server
            .clone()
            .oneshot(request("GET", &route, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(after["cadence"]["goal_minutes"], 90);
    assert_eq!(
        after["cadence"]["last_green_main_at"],
        serde_json::Value::Null
    );
    assert_eq!(after["cadence"]["action"], "no_ready_work");
    assert_ne!(before["cursor"], after["cursor"]);
}

/// HS2-A8HP69: a strategy write publishes a change event for its checkout, so a waiting
/// Workers long poll (which wakes on any change event and reads again) shows the new
/// cadence without waiting for the platform file watcher. A waiting poll answers with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_a8hp69_a_strategy_write_wakes_a_waiting_workers_poll_by_event() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let tickets = root.path().join("tickets");
    std::fs::create_dir_all(&project).unwrap();
    assert!(
        hotsheet_ticketing::git::launch("git")
            .args(["init", "-q"])
            .arg(&project)
            .status()
            .unwrap()
            .success()
    );
    let store = FsStore::init_git(&tickets, &StoreMetadata::new("SYN")).unwrap();
    let state = AppState::new(store, "synthetic-secret".to_owned())
        .unwrap()
        .with_default_codex_home(root.path().join("no-codex-home"))
        .with_checkout_registry(root.path().join("checkouts.json"));
    let mut events = state.subscribe();
    let server = app(state);
    let opened = server
        .clone()
        .oneshot(request(
            "POST",
            "/projects/open",
            Some(&serde_json::json!({"root": project, "stores": [tickets]}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(opened.status(), StatusCode::CREATED);
    let id = json(opened).await["checkout"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = json(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/checkouts/{id}/workers?wait_ms=0"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(before.get("cadence").is_none());
    let strategy_events =
        |events: &mut tokio::sync::broadcast::Receiver<hotsheet_server::ChangeEvent>| {
            let mut seen = 0;
            while let Ok(event) = events.try_recv() {
                if event.kind == "development_strategy_changed" && event.id == id {
                    seen += 1;
                }
            }
            seen
        };
    strategy_events(&mut events);
    // Both strategy writes publish the event.
    let integration = server
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/checkouts/{id}/integration-strategies"),
            Some(&{
                let mut policy = hotsheet_cli::coordination::StrategyPolicy::default();
                policy
                    .allow(
                        hotsheet_cli::coordination::IntegrationStrategy::CiStaged,
                        "synthetic cadence",
                    )
                    .unwrap();
                serde_json::to_string(&policy).unwrap()
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        integration.status(),
        StatusCode::OK,
        "{:?}",
        json(integration).await
    );
    assert_eq!(strategy_events(&mut events), 1);
    // A Workers poll waiting on the current view answers with the cadence the next write
    // adds.
    let current = json(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/checkouts/{id}/workers?wait_ms=0"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    let waiting = tokio::spawn({
        let server = server.clone();
        let route = format!(
            "/checkouts/{id}/workers?wait_ms=30000&after={}",
            current["cursor"].as_str().unwrap()
        );
        async move { json(server.oneshot(request("GET", &route, None)).await.unwrap()).await }
    });
    let saved = server
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/checkouts/{id}/development-strategy"),
            Some(
                &serde_json::json!({"version": 1, "default": "steady-main", "green_main_goal_minutes": 90})
                    .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(strategy_events(&mut events), 1);
    let woken = waiting.await.unwrap();
    assert_eq!(woken["cadence"]["goal_minutes"], 90, "{woken}");
}
