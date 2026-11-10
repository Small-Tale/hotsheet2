//! CLI/REST parity for the shared project policy (HS2-BF79YX).

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";

fn request(method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET);
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
async fn hs2_bf79yx_authenticated_rest_and_headless_policy_share_one_setting() {
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
    let server = app(AppState::new(store, SECRET.to_owned())
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
    let route = format!("/checkouts/{id}/development-strategy");
    let default = server
        .clone()
        .oneshot(request("GET", &route, None))
        .await
        .unwrap();
    assert_eq!(default.status(), StatusCode::OK);
    let default = json(default).await;
    assert_eq!(default["policy"]["version"], 1);
    assert_eq!(default["effective"]["preset"], "ticket-assured");
    assert_eq!(default["effective"]["integration"], "serial");
    assert_eq!(default["effective"]["source"], "unconfigured-project");
    assert_eq!(default["effective"]["local_gate"], "risk-selected");
    let rejected = server
        .clone()
        .oneshot(request(
            "PUT",
            &route,
            Some(r#"{"version":1,"default":"steady-main","green_main_goal_minutes":360}"#),
        ))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

    let settings = hotsheet_ticketing::Settings::for_project(&project);
    let mut integration = hotsheet_cli::coordination::StrategyPolicy::read(&settings).unwrap();
    integration
        .allow(
            hotsheet_cli::coordination::IntegrationStrategy::Batch,
            "reviewed existing batch policy",
        )
        .unwrap();
    integration
        .set_default(hotsheet_cli::coordination::IntegrationStrategy::Batch)
        .unwrap();
    integration.write(&settings).unwrap();
    let migrated = server
        .clone()
        .oneshot(request("GET", &route, None))
        .await
        .unwrap();
    assert_eq!(migrated.status(), StatusCode::OK);
    let migrated = json(migrated).await;
    assert_eq!(migrated["effective"]["source"], "unconfigured-project");
    assert_eq!(migrated["effective"]["integration"], "batch");
    assert_eq!(migrated["effective"]["local_gate"], "risk-selected");
    integration
        .allow(
            hotsheet_cli::coordination::IntegrationStrategy::CiStaged,
            "synthetic exact-SHA staging",
        )
        .unwrap();
    integration.write(&settings).unwrap();
    let saved = server
        .clone()
        .oneshot(request(
            "PUT",
            &route,
            Some(r#"{"version":1,"default":"steady-main","green_main_goal_minutes":420}"#),
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    let saved = json(saved).await;
    assert_eq!(saved["effective"]["integration"], "ci-staged");
    assert_eq!(saved["effective"]["source"], "project");
    assert_eq!(saved["effective"]["green_main_goal_minutes"], 420);
    let headless = hotsheet_cli::coordination::DevelopmentPolicy::read(&settings).unwrap();
    assert_eq!(headless.green_main_goal_minutes, 420);
    assert_eq!(
        headless.default,
        hotsheet_cli::coordination::DevelopmentPreset::SteadyMain
    );
    let custom = serde_json::json!({
        "version": 2,
        "default": "ticket-assured",
        "default_custom": "strict-review",
        "custom": {
            "strict-review": {
                "integration": "serial", "local_gate": "full-local", "minimum_tier": "T2",
                "browser": true, "test_plan_timing": "before-work", "ci_gate": "CI",
                "red_ci_recovery": "attribute-and-retry"
            }
        }
    });
    let no_ci = server
        .clone()
        .oneshot(request("PUT", &route, Some(&custom.to_string())))
        .await
        .unwrap();
    assert_eq!(no_ci.status(), StatusCode::BAD_REQUEST);
    std::fs::create_dir_all(project.join(".github/workflows")).unwrap();
    std::fs::write(
        project.join(".github/workflows/ci.yml"),
        "name: CI\non: push\n",
    )
    .unwrap();
    assert!(
        hotsheet_ticketing::git::launch("git")
            .current_dir(&project)
            .args([
                "remote",
                "add",
                "origin",
                "https://example.invalid/synthetic.git"
            ])
            .status()
            .unwrap()
            .success()
    );
    let custom_saved = server
        .clone()
        .oneshot(request("PUT", &route, Some(&custom.to_string())))
        .await
        .unwrap();
    assert_eq!(custom_saved.status(), StatusCode::OK);
    let custom_saved = json(custom_saved).await;
    assert_eq!(custom_saved["effective"]["preset"], "strict-review");
    assert_eq!(custom_saved["effective"]["source"], "project");
    assert_eq!(custom_saved["effective"]["custom"]["minimum_tier"], "T2");
    let session = server
        .clone()
        .oneshot(request(
            "GET",
            &format!("{route}?session=risk-guided"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(session.status(), StatusCode::OK);
    let session = json(session).await;
    assert_eq!(session["effective"]["source"], "session");
    assert_eq!(session["effective"]["preset"], "risk-guided");
    assert_eq!(session["policy"]["default_custom"], "strict-review");
    let ticket = server
        .clone()
        .oneshot(request(
            "GET",
            &format!("{route}?session=risk-guided&ticket=strict-review"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(ticket.status(), StatusCode::OK);
    let ticket = json(ticket).await;
    assert_eq!(ticket["effective"]["source"], "ticket");
    assert_eq!(ticket["effective"]["preset"], "strict-review");
    let unknown = server
        .clone()
        .oneshot(request("GET", &format!("{route}?ticket=missing"), None))
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
    let reread = hotsheet_cli::coordination::DevelopmentPolicy::read(&settings).unwrap();
    assert_eq!(reread.default_custom.as_deref(), Some("strict-review"));
    let unsupported = serde_json::json!({
        "version": 2, "custom": {
            "bad-ci": { "integration": "serial", "local_gate": "targeted", "minimum_tier": "T1",
                "test_plan_timing": "before-handoff", "ci_gate": "Missing CI", "red_ci_recovery": "attribute-and-retry" }
        }
    });
    let rejected = server
        .clone()
        .oneshot(request("PUT", &route, Some(&unsupported.to_string())))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    std::fs::remove_file(project.join(".github/workflows/ci.yml")).unwrap();
    let mut rollback = custom;
    rollback.as_object_mut().unwrap().remove("default_custom");
    let restored = server
        .clone()
        .oneshot(request("PUT", &route, Some(&rollback.to_string())))
        .await
        .unwrap();
    assert_eq!(
        restored.status(),
        StatusCode::OK,
        "a built-in rollback must work after CI disappears"
    );
    assert_eq!(
        json(restored).await["effective"]["preset"],
        "ticket-assured"
    );
    let unauthenticated = server
        .oneshot(Request::builder().uri(&route).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
}
