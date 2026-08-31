//! HTTP E2E for the server, driven through the router in-process (no real socket).

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use hotsheet_server::{AppState, MAX_ATTACHMENT_BODY_BYTES, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

use hotsheet_extsync::{
    GitHubConfig, GitHubProvider, GitHubTransport, GitLabConfig, GitLabProvider, HttpResponse,
    JiraConfig, JiraProvider,
};

const SECRET: &str = "test-secret";

fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, AppState::new(store, SECRET.into()).unwrap())
}

fn authed(method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET);
    if body.is_some() {
        b = b.header(header::CONTENT_TYPE, "application/json");
    }
    b.body(body.map_or(Body::empty(), |s| Body::from(s.to_string())))
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

struct FakeGitHub {
    responses: Mutex<VecDeque<HttpResponse>>,
}

impl GitHubTransport for FakeGitHub {
    fn request(
        &self,
        _: &str,
        _: &str,
        _: &[(&str, String)],
        _: Option<&serde_json::Value>,
    ) -> Result<HttpResponse, String> {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| "unexpected GitHub request".into())
    }
}

fn github_issue(number: u64, title: &str) -> serde_json::Value {
    serde_json::json!({
        "number":number,"title":title,"body":"body","state":"open","state_reason":null,
        "html_url":format!("https://github.test/acme/repo/issues/{number}"),
        "created_at":"2026-08-26T00:00:00Z","updated_at":"2026-08-26T00:01:00Z",
        "closed_at":null,"labels":[],"assignees":[],"pull_request":null
    })
}

fn github_response(status: u16, body: serde_json::Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: HashMap::new(),
        body: body.to_string(),
    }
}

#[tokio::test]
async fn health_needs_no_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let health = body_json(resp).await;
    assert_eq!(health["status"], "ok");
    assert_eq!(health["generation"], "hs2");
    assert_eq!(health["api_version"], 1);
    assert_eq!(health["ticket_prefix"], "HS");
    assert_eq!(health["store_schema"], 1);
}

#[tokio::test]
async fn compatibility_is_authenticated_and_reports_ranges_without_promising_restart() {
    let (_d, st) = state();
    let router = app(st);
    let denied = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/compatibility")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    let response = router
        .oneshot(authed("GET", "/compatibility", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value = body_json(response).await;
    assert_eq!(value["generation"], "hs2");
    assert_eq!(value["application_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["protocol"], serde_json::json!({"min": 1, "max": 1}));
    assert_eq!(
        value["store_schema"],
        serde_json::json!({"min": 1, "max": 1})
    );
    assert_eq!(value["capabilities"]["lifecycle_restart"], false);
    assert_eq!(value["capabilities"]["lifecycle_quiescence"], false);
    assert!(value.get("build_revision").is_some());
    assert!(value.get("started_at").is_some());
}

#[tokio::test]
async fn tickets_require_the_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(
            Request::builder()
                .uri("/tickets")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[cfg(unix)]
#[tokio::test]
async fn ticket_mutation_does_not_wait_for_remote_publication() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    fn git(path: &std::path::Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(path)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    let dir = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "-A"]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=Hot Sheet Test",
            "-c",
            "user.email=test@localhost",
            "commit",
            "-q",
            "-m",
            "initial",
        ],
    );
    git(remote.path(), &["init", "--bare", "-q"]);
    git(
        dir.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(dir.path(), &["push", "-q", "-u", "origin", "HEAD"]);
    let hook = remote.path().join("hooks/pre-receive");
    std::fs::write(&hook, "#!/bin/sh\nsleep 2\n").unwrap();
    let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(hook, permissions).unwrap();

    let started = Instant::now();
    let response = app(AppState::new(store, SECRET.into()).unwrap())
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"fast local write"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "server mutation waited for the deliberately slow remote"
    );
}

#[tokio::test]
async fn providers_expose_capabilities_and_route_the_default_git_provider() {
    let (_d, st) = state();
    let app = app(st);
    let providers = body_json(
        app.clone()
            .oneshot(authed("GET", "/providers", None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(providers.as_array().unwrap().len(), 1);
    assert_eq!(providers[0]["provider"], "git");
    assert_eq!(providers[0]["default"], true);
    assert_eq!(providers[0]["capabilities"]["claims"], true);
    let connection = providers[0]["connection_id"].as_str().unwrap();

    let created = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                &format!("/providers/{connection}/tickets"),
                Some(r#"{"title":"provider route"}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(created["connection_id"], connection);
    assert_eq!(created["native_id"], created["id"]);
    assert_eq!(
        created["qualified_id"],
        format!("{connection}:{}", created["id"].as_str().unwrap())
    );

    let listed = body_json(
        app.oneshot(authed(
            "GET",
            &format!("/providers/{connection}/tickets"),
            None,
        ))
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(listed[0]["connection_id"], connection);
    assert_eq!(listed[0]["qualified_id"], created["qualified_id"]);
}

#[tokio::test]
async fn checkout_registry_is_authenticated_and_resolvable() {
    let (_d, st) = state();
    let checkout = tempfile::tempdir().unwrap();
    let registry_home = tempfile::tempdir().unwrap();
    let app = app(st.with_checkout_registry(registry_home.path().join("checkouts.json")));
    let body = serde_json::json!({
        "root": checkout.path(), "alias": "worktree-a", "repository": "github.com/acme/app",
        "stores": []
    })
    .to_string();
    let resp = app
        .clone()
        .oneshot(authed("POST", "/checkouts", Some(&body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let checkout_id = created["id"].as_str().unwrap();
    assert_eq!(checkout_id.rsplit('-').next().unwrap().len(), 12);

    let resp = app
        .clone()
        .oneshot(authed("GET", "/checkouts/worktree-a", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        body_json(resp).await["root"],
        checkout
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/checkouts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn opening_project_discovers_hosts_and_links_parallel_hs2_store() {
    let (_primary, st) = state();
    let workspace = tempfile::tempdir().unwrap();
    let checkout = workspace.path().join("app");
    let ticket_store = workspace.path().join("app.hs2");
    std::fs::create_dir(&checkout).unwrap();
    FsStore::init(&ticket_store, &StoreMetadata::new("APP")).unwrap();
    let registry = tempfile::tempdir().unwrap();
    let app = app(st.with_checkout_registry(registry.path().join("checkouts.json")));

    let opened = app
        .clone()
        .oneshot(authed(
            "POST",
            "/projects/open",
            Some(&serde_json::json!({"root": checkout}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(opened.status(), StatusCode::CREATED);
    let opened = body_json(opened).await;
    assert_eq!(opened["discovered"], true);
    assert_eq!(opened["checkout"]["stores"].as_array().unwrap().len(), 1);
    let checkout_id = opened["checkout"]["id"].as_str().unwrap();

    let created = app
        .oneshot(authed(
            "POST",
            &format!("/checkouts/{checkout_id}/tickets"),
            Some(r#"{"title":"Opened through project onboarding"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    assert!(
        body_json(created).await["slug"]
            .as_str()
            .unwrap()
            .starts_with("APP-")
    );
}

#[tokio::test]
async fn checkout_scoped_ticket_routes_aggregate_and_resolve_linked_stores() {
    let (primary, st) = state();
    let extra = tempfile::tempdir().unwrap();
    FsStore::init(extra.path(), &StoreMetadata::new("EX")).unwrap();
    let checkout = tempfile::tempdir().unwrap();
    let registry = tempfile::tempdir().unwrap();
    let app = app(st.with_checkout_registry(registry.path().join("checkouts.json")));
    let added = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                "/stores",
                Some(&serde_json::json!({"path":extra.path()}).to_string()),
            ))
            .await
            .unwrap(),
    )
    .await;
    let store_id = added["id"].as_str().unwrap();
    let registration=serde_json::json!({"root":checkout.path(),"alias":"combo","stores":[primary.path(),extra.path()] }).to_string();
    app.clone()
        .oneshot(authed("POST", "/checkouts", Some(&registration)))
        .await
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(authed(
                "POST",
                "/checkouts/combo/tickets",
                Some(r#"{"title":"Scoped"}"#)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let created = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                &format!("/checkouts/combo/tickets?store={store_id}"),
                Some(r#"{"title":"Scoped"}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    let slug = created["slug"].as_str().unwrap();
    let listed = body_json(
        app.clone()
            .oneshot(authed("GET", "/checkouts/combo/tickets", None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed[0]["store"], store_id);
    let got = body_json(
        app.clone()
            .oneshot(authed(
                "GET",
                &format!("/checkouts/combo/tickets/{slug}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(got["title"], "Scoped");
    // Axum's default buffered-body limit is 2 MiB. Real screen recordings commonly
    // exceed that, so exercise the checkout-scoped route with a representative video.
    let video_bytes = vec![0x5a; 3 * 1024 * 1024];
    let video_request = Request::builder()
        .method("POST")
        .uri(format!("/checkouts/combo/tickets/{slug}/attachments"))
        .header("x-hotsheet-secret", SECRET)
        .header("x-hotsheet-filename", "choppy.mov")
        .body(Body::from(video_bytes))
        .unwrap();
    let video_response = app.clone().oneshot(video_request).await.unwrap();
    assert_eq!(video_response.status(), StatusCode::CREATED);
    let video_attached = body_json(video_response).await;
    assert_eq!(video_attached["attachments"][0]["filename"], "choppy.mov");
    let video_attachment_id = video_attached["attachments"][0]["id"].as_str().unwrap();
    let video_removed = body_json(
        app.clone()
            .oneshot(authed(
                "DELETE",
                &format!("/checkouts/combo/tickets/{slug}/attachments/{video_attachment_id}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(video_removed["attachments"].as_array().unwrap().len(), 0);
    let attachment_request = Request::builder()
        .method("POST")
        .uri(format!("/checkouts/combo/tickets/{slug}/attachments"))
        .header("x-hotsheet-secret", SECRET)
        .header("x-hotsheet-filename", "proof.txt")
        .body(Body::from("checkout evidence"))
        .unwrap();
    let attached = body_json(app.clone().oneshot(attachment_request).await.unwrap()).await;
    assert_eq!(attached["attachments"][0]["filename"], "proof.txt");
    let unicode_request = Request::builder()
        .method("POST")
        .uri(format!("/checkouts/combo/tickets/{slug}/attachments"))
        .header("x-hotsheet-secret", SECRET)
        .header("x-hotsheet-filename", "Screenshot%20%E2%80%AFAM.png")
        .header("x-hotsheet-filename-encoding", "percent")
        .body(Body::from("unicode evidence"))
        .unwrap();
    let unicode_attached = body_json(app.clone().oneshot(unicode_request).await.unwrap()).await;
    assert!(
        unicode_attached["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["filename"] == "Screenshot  AM.png")
    );
    let attachment_id = attached["attachments"][0]["id"].as_str().unwrap();
    let downloaded = app
        .clone()
        .oneshot(authed(
            "GET",
            &format!("/checkouts/combo/tickets/{slug}/attachments/{attachment_id}"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(downloaded.status(), StatusCode::OK);
    assert_eq!(downloaded.headers()["x-hotsheet-filename"], "proof.txt");
    assert_eq!(
        downloaded.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        downloaded.into_body().collect().await.unwrap().to_bytes(),
        "checkout evidence"
    );
    let removed = body_json(
        app.clone()
            .oneshot(authed(
                "DELETE",
                &format!("/checkouts/combo/tickets/{slug}/attachments/{attachment_id}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(removed["attachments"].as_array().unwrap().len(), 1);
    let unicode_attachment_id = removed["attachments"][0]["id"].as_str().unwrap();
    let removed = body_json(
        app.clone()
            .oneshot(authed(
                "DELETE",
                &format!("/checkouts/combo/tickets/{slug}/attachments/{unicode_attachment_id}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(removed["attachments"].as_array().unwrap().len(), 0);
    let updated = body_json(
        app.oneshot(authed(
            "PATCH",
            &format!("/checkouts/combo/tickets/{slug}"),
            Some(r#"{"title":"Updated"}"#),
        ))
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(updated["title"], "Updated");
}

#[tokio::test]
async fn repository_status_endpoint_reports_real_git_state() {
    let (_d, st) = state();
    let checkout = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(checkout.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    };
    run(&["init", "-q"]);
    std::fs::write(checkout.path().join("tracked.txt"), "one\n").unwrap();
    run(&["add", "tracked.txt"]);
    run(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-qm",
        "initial",
    ]);
    std::fs::write(checkout.path().join("tracked.txt"), "two\n").unwrap();
    std::fs::write(checkout.path().join("new.txt"), "new\n").unwrap();
    let registry_home = tempfile::tempdir().unwrap();
    let registry = registry_home.path().join("checkouts.json");
    let app = app(st.with_checkout_registry(&registry));
    let body =
        serde_json::json!({"root": checkout.path(), "alias": "repo", "stores": []}).to_string();
    app.clone()
        .oneshot(authed("POST", "/checkouts", Some(&body)))
        .await
        .unwrap();
    let resp = app
        .oneshot(authed("GET", "/checkouts/repo/repository/status", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let status = body_json(resp).await;
    assert_eq!(status["unstaged"], 1);
    assert_eq!(status["untracked"], 1);
    assert_eq!(status["clean"], false);
}

#[tokio::test]
async fn analytics_endpoints_return_ticket_flow_and_usage_contracts() {
    let (_d, st) = state();
    let app = app(st);
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"Measure me","category":"task"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let flow = app
        .clone()
        .oneshot(authed("GET", "/analytics/tickets", None))
        .await
        .unwrap();
    assert_eq!(flow.status(), StatusCode::OK);
    let flow = body_json(flow).await;
    assert_eq!(flow["total"], 1);
    assert_eq!(flow["open"], 1);
    assert_eq!(flow["historical_cumulative_flow_available"], false);
    let usage = app
        .oneshot(authed("GET", "/analytics/usage", None))
        .await
        .unwrap();
    assert_eq!(usage.status(), StatusCode::OK);
    assert_eq!(body_json(usage).await["events"], 0);
}

#[tokio::test]
async fn configured_commands_stream_output_keep_history_and_cancel() {
    use hotsheet_ticketing::commands::CommandDefinition;
    let (_d, st) = state();
    let defs = vec![
        CommandDefinition {
            id: "echo".into(),
            title: "Echo".into(),
            program: "/bin/echo".into(),
            args: vec!["hello".into()],
            group: None,
            confirmation: None,
        },
        CommandDefinition {
            id: "sleep".into(),
            title: "Sleep".into(),
            program: "/bin/sleep".into(),
            args: vec!["10".into()],
            group: None,
            confirmation: None,
        },
    ];
    let app = app(st.with_commands(defs));
    let run = app
        .clone()
        .oneshot(authed("POST", "/commands/echo/run", None))
        .await
        .unwrap();
    assert_eq!(run.status(), StatusCode::ACCEPTED);
    let id = body_json(run).await["id"].as_str().unwrap().to_owned();
    let mut completed = None;
    for _ in 0..50 {
        let resp = app
            .clone()
            .oneshot(authed("GET", &format!("/command-runs/{id}?after=0"), None))
            .await
            .unwrap();
        let v = body_json(resp).await;
        if v["state"] == "completed" {
            completed = Some(v);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let completed = completed.expect("command completed");
    assert_eq!(completed["output"][0]["text"], "hello");
    let run = app
        .clone()
        .oneshot(authed("POST", "/commands/sleep/run", None))
        .await
        .unwrap();
    let sleep_id = body_json(run).await["id"].as_str().unwrap().to_owned();
    let cancelled = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/command-runs/{sleep_id}/cancel"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(body_json(cancelled).await["state"], "cancelled");
    assert_eq!(
        app.oneshot(authed("POST", "/commands/not-configured/run", None))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn notifications_route_dedupe_ack_and_emit_live_events() {
    let (_d, st) = state();
    let mut events = st.subscribe();
    let app = app(st);
    let body = r#"{"message":"Build done","checkout":"web","dedupe_key":"build-1"}"#;
    let first = body_json(
        app.clone()
            .oneshot(authed("POST", "/notifications", Some(body)))
            .await
            .unwrap(),
    )
    .await;
    let second = body_json(
        app.clone()
            .oneshot(authed("POST", "/notifications", Some(body)))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(first["id"], second["id"]);
    let event = events.recv().await.unwrap();
    assert_eq!(event.kind, "notification");
    let listed = body_json(
        app.clone()
            .oneshot(authed("GET", "/notifications?checkout=web", None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let id = first["id"].as_str().unwrap();
    let ack = body_json(
        app.oneshot(authed("POST", &format!("/notifications/{id}/ack"), None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(ack["acknowledged"], true);
}

#[tokio::test]
async fn assign_endpoint_emits_recipient_payloads_and_targeted_notifications() {
    let (_d, st) = state();
    let mut events = st.subscribe();
    let app = app(st);
    let created = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                "/tickets",
                Some(r#"{"title":"Review this"}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    let slug = created["slug"].as_str().unwrap();
    let body = r#"{"assignees":["dev@example.com"],"reviews":[{"who":"reviewer@example.com","kind":"review"}]}"#;
    let assigned = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                &format!("/tickets/{slug}/assign"),
                Some(body),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(assigned["assignees"][0], "dev@example.com");
    let event = loop {
        let event = events.recv().await.unwrap();
        if event.kind == "assignment" {
            break event;
        }
    };
    assert_eq!(event.kind, "assignment");
    let payload = event.assignment.unwrap();
    assert_eq!(payload.newly_assigned, ["dev@example.com"]);
    assert_eq!(payload.review_requested, ["reviewer@example.com"]);
    let notices = body_json(
        app.oneshot(authed(
            "GET",
            "/notifications?recipient=reviewer%40example.com",
            None,
        ))
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(notices.as_array().unwrap().len(), 1);
    assert_eq!(notices[0]["ticket"], slug);
}

struct FakeTts;
impl hotsheet_server::tts::TtsProvider for FakeTts {
    fn id(&self) -> &str {
        "fake"
    }
    fn synthesize(
        &self,
        text: &str,
        _: Option<&str>,
    ) -> Result<hotsheet_server::tts::TtsAudio, String> {
        Ok(hotsheet_server::tts::TtsAudio {
            content_type: "audio/test".into(),
            bytes: text.as_bytes().to_vec(),
        })
    }
}
#[tokio::test]
async fn tts_runs_behind_server_provider_boundary_without_client_secrets() {
    let (_d, st) = state();
    let app = app(st.with_tts_providers(vec![std::sync::Arc::new(FakeTts)]));
    let resp = app
        .oneshot(authed(
            "POST",
            "/tts/synthesize",
            Some(r#"{"text":"hello","provider":"fake"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "audio/test");
    assert_eq!(
        resp.into_body().collect().await.unwrap().to_bytes(),
        "hello"
    );
}

#[tokio::test]
async fn create_get_update_close_and_query() {
    let (_d, st) = state();
    let app = app(st);

    // create
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"Fix flicker","category":"bug","priority":"high","up_next":true}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let slug = created["slug"].as_str().unwrap().to_string();
    assert!(slug.starts_with("HS-"));
    assert_eq!(created["title"], "Fix flicker");
    assert_eq!(created["priority"], "high");
    assert_eq!(created["up_next"], true);
    assert_eq!(created["auto_context"][0]["source"], "category");
    assert_eq!(created["auto_context"][0]["key"], "bug");

    // get (by slug)
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/tickets/{slug}"), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let fetched = body_json(resp).await;
    assert_eq!(fetched["auto_context"][0]["key"], "bug");
    assert_eq!(fetched["slug"], slug);

    // update status -> stamps completed_at
    let resp = app
        .clone()
        .oneshot(authed(
            "PATCH",
            &format!("/tickets/{slug}"),
            Some(r#"{"status":"completed","details":"root-caused"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = body_json(resp).await;
    assert_eq!(updated["status"], "completed");
    assert_eq!(updated["details"], "root-caused");
    assert!(updated["completed_at"].is_string());

    // close
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/close"),
            Some(r#"{"reason":"completed"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["close_reason"], "completed");

    // query with filters
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?status=completed&text=flick", None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 1);

    // unknown ticket -> 404
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets/HS-NOPE00", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn blocked_by_set_clear_and_reject() {
    let (_d, st) = state();
    let app = app(st);

    let mk = |title: &str| format!(r#"{{"title":"{title}"}}"#);
    let a = body_json(
        app.clone()
            .oneshot(authed("POST", "/tickets", Some(&mk("blocker"))))
            .await
            .unwrap(),
    )
    .await;
    let a_slug = a["slug"].as_str().unwrap().to_string();
    let a_id = a["id"].as_str().unwrap().to_string();

    // create with a blocker (by slug) resolves to the ULID on the wire
    let b = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                "/tickets",
                Some(&format!(
                    r#"{{"title":"blocked","blocked_by":["{a_slug}"]}}"#
                )),
            ))
            .await
            .unwrap(),
    )
    .await;
    let b_slug = b["slug"].as_str().unwrap().to_string();
    assert_eq!(b["blocked_by"], serde_json::json!([a_id]));

    // PATCH with [] clears
    let cleared = body_json(
        app.clone()
            .oneshot(authed(
                "PATCH",
                &format!("/tickets/{b_slug}"),
                Some(r#"{"blocked_by":[]}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(cleared["blocked_by"], serde_json::json!([]));

    // self-reference → 400
    let resp = app
        .clone()
        .oneshot(authed(
            "PATCH",
            &format!("/tickets/{b_slug}"),
            Some(&format!(r#"{{"blocked_by":["{b_slug}"]}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // unknown blocker → 404
    let resp = app
        .clone()
        .oneshot(authed(
            "PATCH",
            &format!("/tickets/{b_slug}"),
            Some(r#"{"blocked_by":["HS-NOPE00"]}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_is_compact_by_default_and_supports_limit() {
    let (_d, st) = state();
    let app = app(st);

    // Two tickets, one with a Markdown body.
    for body in [
        r#"{"title":"has body","details":"a long markdown body"}"#,
        r#"{"title":"no body"}"#,
    ] {
        let resp = app
            .clone()
            .oneshot(authed("POST", "/tickets", Some(body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // Default list omits the body (the key is absent on every row).
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets", None))
        .await
        .unwrap();
    let rows = body_json(resp).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.get("details").is_none()));

    // compact=false keeps the body.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?compact=false&text=markdown", None))
        .await
        .unwrap();
    let full = body_json(resp).await;
    assert_eq!(
        full.as_array().unwrap()[0]["details"],
        "a long markdown body"
    );

    // limit caps the rows.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?limit=1", None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn announce_broadcasts_live_but_is_not_persisted_in_the_poll_ring() {
    let (_d, st) = state();
    // Subscribe to the live bus before announcing.
    let mut rx = st.subscribe();
    let app = app(st);

    // An empty announcement is rejected.
    let resp = app
        .clone()
        .oneshot(authed("POST", "/announce", Some(r#"{"message":"  "}"#)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // A real announcement broadcasts to live subscribers.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/announce",
            Some(r#"{"message":"deploying now"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let ev = rx
        .try_recv()
        .expect("a live subscriber receives the announcement");
    assert_eq!(ev.kind, "announce");
    assert_eq!(ev.message.as_deref(), Some("deploying now"));

    // Ephemeral: it is NOT in the long-poll ring. Create a ticket (which IS logged), then a
    // poll since=0 sees the ticket event but never the announcement.
    let resp = app
        .clone()
        .oneshot(authed("POST", "/tickets", Some(r#"{"title":"logged"}"#)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = app
        .clone()
        .oneshot(authed(
            "GET",
            &format!("/ws/poll?secret={SECRET}&since=0&timeout_ms=0"),
            None,
        ))
        .await
        .unwrap();
    let polled = body_json(resp).await;
    let kinds: Vec<&str> = polled["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"created"),
        "the ticket change is in the ring"
    );
    assert!(
        !kinds.contains(&"announce"),
        "the announcement must not be persisted in the poll ring: {kinds:?}"
    );
}

#[tokio::test]
async fn activity_ingest_then_timeline_filters_by_ticket_and_importance() {
    let (_d, st) = state();
    let mut live = st.subscribe();
    let app = app(st);

    // Ingest three events: two on ticket T1 (an edit + a turn_end), one on T2.
    for body in [
        r#"{"tool":"claude","kind":"edit","detail":{"path":"src/a.rs"},"ticket":"T1","session":"s1"}"#,
        r#"{"tool":"claude","kind":"turn_end","ticket":"T1","session":"s1"}"#,
        r#"{"tool":"codex","kind":"command","detail":{"command":"ls"},"ticket":"T2"}"#,
    ] {
        let resp = app
            .clone()
            .oneshot(authed("POST", "/activity", Some(body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // The server composes a default summary from the kind.
        let ev = body_json(resp).await;
        assert!(ev["summary"].as_str().unwrap().len() > 3);
        let pushed = live
            .try_recv()
            .expect("recorded activity is pushed on the live bus");
        assert_eq!(pushed.kind, "activity");
        assert_eq!(pushed.activity.as_ref().unwrap().id, ev["id"]);
    }

    // Activity also participates in the long-poll fallback rather than becoming a
    // WebSocket-only parallel protocol.
    let resp = app
        .clone()
        .oneshot(authed(
            "GET",
            &format!("/ws/poll?secret={SECRET}&since=0&timeout_ms=0"),
            None,
        ))
        .await
        .unwrap();
    let poll = body_json(resp).await;
    assert_eq!(poll["events"][0]["kind"], "activity");
    assert!(poll["events"][0]["activity"]["summary"].is_string());

    // Timeline for T1 → the two T1 events, chronological.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/activity?ticket=T1", None))
        .await
        .unwrap();
    let rows = body_json(resp).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["kind"], "edit");
    assert_eq!(rows[1]["kind"], "turn_end");

    // High-importance digest of T1 → only the turn_end (edit is normal).
    let resp = app
        .clone()
        .oneshot(authed(
            "GET",
            "/activity?ticket=T1&min_importance=high",
            None,
        ))
        .await
        .unwrap();
    let rows = body_json(resp).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "turn_end");

    // A bad importance is a 400.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/activity?min_importance=urgent", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn fields_projection_returns_a_leaner_row() {
    let (_d, st) = state();
    let app = app(st);

    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"lean me","category":"bug"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // fields=status,up_next keeps only those (+ slug, always). category/title are dropped.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?fields=status,up_next", None))
        .await
        .unwrap();
    let rows = body_json(resp).await;
    let row = &rows.as_array().unwrap()[0];
    let keys: std::collections::HashSet<&str> = row
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["slug", "status", "up_next"].into_iter().collect(),
        "only the requested fields (+ slug) are present: {row}"
    );
}

#[tokio::test]
async fn keyset_paging_walks_the_store_and_rejects_a_bad_cursor() {
    let (_d, st) = state();
    let app = app(st);

    // Three tickets.
    for i in 0..3 {
        let body = format!(r#"{{"title":"t{i}"}}"#);
        let resp = app
            .clone()
            .oneshot(authed("POST", "/tickets", Some(&body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // Page one row at a time via page_after=<last id>, and reassemble the walk.
    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let uri = match &cursor {
            Some(c) => format!("/tickets?limit=1&page_after={c}"),
            None => "/tickets?limit=1".to_string(),
        };
        let resp = app
            .clone()
            .oneshot(authed("GET", &uri, None))
            .await
            .unwrap();
        let rows = body_json(resp).await;
        let rows = rows.as_array().unwrap();
        if rows.is_empty() {
            break;
        }
        let id = rows[0]["id"].as_str().unwrap().to_string();
        cursor = Some(id.clone());
        seen.push(id);
    }
    assert_eq!(seen.len(), 3, "keyset walk should visit every ticket once");
    let unique: std::collections::HashSet<_> = seen.iter().collect();
    assert_eq!(unique.len(), 3, "no ticket should appear twice");

    // A non-ULID cursor is a 400, not a silent full list.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?page_after=not-a-ulid", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn me_views_resolve_assigned_to_me_and_needs_my_review() {
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{NewTicket, ops};

    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    // `me` resolves via `git config user.email`, so the store dir must be a repo with a
    // pinned local identity (deterministic regardless of global git config).
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .arg("init")
        .output()
        .unwrap();
    for args in [["user.email", "me@hs.test"], ["user.name", "Me"]] {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .arg("config")
            .args(args)
            .output()
            .unwrap();
    }
    let now = Timestamp::new("2026-08-19T00:00:00Z");
    let mine = ops::create(
        &store,
        Ulid::new(),
        "HS",
        now.clone(),
        NewTicket {
            title: "mine".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let theirs = ops::create(
        &store,
        Ulid::new(),
        "HS",
        now.clone(),
        NewTicket {
            title: "theirs".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();
    ops::assign(
        &store,
        &mine.id,
        now.clone(),
        Some(vec!["me@hs.test".into()]),
        vec![],
    )
    .unwrap();
    // `theirs` is assigned to someone else but has a review request for ME — so the two
    // `me` views (assigned-to-me vs needs-my-review) must return *different* tickets.
    ops::assign(
        &store,
        &theirs.id,
        now.clone(),
        Some(vec!["other@hs.test".into()]),
        vec![hotsheet_model::ReviewRequest {
            who: "me@hs.test".into(),
            kind: hotsheet_model::ReviewKind::Feedback,
            by: Ulid::new(),
            at: now,
            requested_by: Some("me@hs.test".into()),
        }],
    )
    .unwrap();

    // Index reflects the assignments/reviews (built at construction, over the seeded store).
    let app = app(AppState::new(store, SECRET.into()).unwrap());

    // "Assigned to me": assignee=me resolves to me@hs.test and returns only my ticket.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?assignee=me", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let rows = body_json(resp).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], "mine");

    // "Needs my review": review_requested=me returns the *other* ticket, not the assigned one.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?review_requested=me", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let rows = body_json(resp).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], "theirs");

    // "I requested": review_by=me resolves through the same git identity.
    let resp = app
        .oneshot(authed("GET", "/tickets?review_by=me", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let rows = body_json(resp).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], "theirs");
}

#[tokio::test]
async fn watcher_reindexes_an_external_write() {
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{NewTicket, ops};

    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store.clone(), SECRET.into()).unwrap();
    let _watch = hotsheet_server::spawn_watcher(state.clone()).unwrap();
    let app = app(state);

    // Write a ticket to disk WITHOUT going through the server (a CLI/git-style edit).
    ops::create(
        &store,
        Ulid::new(),
        "HS",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "external ticket".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();

    // The watcher should reindex it; poll the HTTP query until it appears.
    for _ in 0..40 {
        let resp = app
            .clone()
            .oneshot(authed("GET", "/tickets?text=external", None))
            .await
            .unwrap();
        if body_json(resp).await.as_array().unwrap().len() == 1 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("watcher did not reindex the external write within 4s");
}

/// The watcher also regenerates each consuming checkout's local worklist on change.
#[tokio::test]
async fn watcher_regenerates_the_worklist() {
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{NewTicket, ops};

    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let registry_path = dir.path().join("checkouts.json");
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry_path);
    registry
        .register(dir.path(), None, None, vec![dir.path().to_path_buf()])
        .unwrap();
    let state = AppState::new(store.clone(), SECRET.into())
        .unwrap()
        .with_checkout_registry(&registry_path);
    let _watch = hotsheet_server::spawn_watcher(state).unwrap();

    ops::create(
        &store,
        Ulid::new(),
        "HS",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "worklist me".into(),
            category: "task".into(),
            up_next: true,
            ..Default::default()
        },
    )
    .unwrap();

    let worklist = dir
        .path()
        .join(hotsheet_ticketing::worklist::CHECKOUT_WORKLIST);
    for _ in 0..40 {
        if let Ok(body) = std::fs::read_to_string(&worklist) {
            if body.contains("worklist me") && body.contains("## Workflow") {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("watcher did not regenerate worklist.md within 4s");
}

#[tokio::test]
async fn close_duplicate_without_target_is_a_400() {
    let (_d, st) = state();
    let app = app(st);
    let created = body_json(
        app.clone()
            .oneshot(authed("POST", "/tickets", Some(r#"{"title":"t"}"#)))
            .await
            .unwrap(),
    )
    .await;
    let slug = created["slug"].as_str().unwrap();

    let resp = app
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/close"),
            Some(r#"{"reason":"duplicate"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_can_append_edit_and_preserve_repeated_activity() {
    let (_d, st) = state();
    let app = app(st);

    let created = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                "/tickets",
                Some(r#"{"title":"Fix flicker"}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();

    // PATCH with a status change AND a note in the same call.
    let updated = body_json(
        app.clone()
            .oneshot(authed(
                "PATCH",
                &format!("/tickets/{id}"),
                Some(r#"{"status":"started","note":"completed","note_kind":"activity"}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(updated["status"], "started");
    assert_eq!(updated["notes"][0]["kind"], "activity");
    assert_eq!(updated["notes"][0]["text"], "completed");
    let note_id = updated["notes"][0]["id"].as_str().unwrap();
    let created_at = updated["notes"][0]["created_at"].as_str().unwrap();
    assert_eq!(updated["notes"][0]["edited_at"], created_at);

    let edit = serde_json::json!({"note":"completed initial investigation","note_id":note_id});
    let edited = body_json(
        app.clone()
            .oneshot(authed(
                "PATCH",
                &format!("/tickets/{id}"),
                Some(&edit.to_string()),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(edited["notes"][0]["created_at"], created_at);
    assert_eq!(
        edited["notes"][0]["text"],
        "completed initial investigation"
    );

    for (status, note) in [
        ("started", "marked not working"),
        ("completed", "completed again"),
    ] {
        let body = serde_json::json!({"status":status,"note":note,"note_kind":"activity"});
        body_json(
            app.clone()
                .oneshot(authed(
                    "PATCH",
                    &format!("/tickets/{id}"),
                    Some(&body.to_string()),
                ))
                .await
                .unwrap(),
        )
        .await;
    }

    // The note persisted (a fresh GET sees it).
    let deleted = body_json(
        app.clone()
            .oneshot(authed(
                "DELETE",
                &format!("/tickets/{id}/notes/{note_id}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(deleted["notes"].as_array().unwrap().len(), 2);

    let got = body_json(
        app.oneshot(authed("GET", &format!("/tickets/{id}"), None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(got["notes"].as_array().unwrap().len(), 2);
    assert_eq!(got["notes"][0]["text"], "marked not working");
    assert_eq!(got["notes"][1]["text"], "completed again");
}

#[tokio::test]
async fn attachment_upload_returns_and_persists_durable_metadata() {
    let (_dir, state) = state();
    let app = app(state);
    let created = body_json(
        app.clone()
            .oneshot(authed("POST", "/tickets", Some(r#"{"title":"evidence"}"#)))
            .await
            .unwrap(),
    )
    .await;
    let id = created["id"].as_str().unwrap();
    let video_bytes = vec![0x5a; 3 * 1024 * 1024];
    let request = Request::builder()
        .method("POST")
        .uri(format!("/tickets/{id}/attachments"))
        .header("x-hotsheet-secret", SECRET)
        .header("x-hotsheet-filename", "../choppy.mov")
        .body(Body::from(video_bytes))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let attached = body_json(response).await;
    assert_eq!(attached["attachments"][0]["filename"], "choppy.mov");
    assert!(attached["attachments"][0]["id"].is_string());
    assert!(attached["attachments"][0]["created_at"].is_string());

    let reread = body_json(
        app.clone()
            .oneshot(authed("GET", &format!("/tickets/{id}"), None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(reread["attachments"], attached["attachments"]);

    // Stream repeated shared chunks so the boundary is exercised without allocating a
    // second 100 MiB buffer in the test process.
    let chunk = Bytes::from(vec![0x5a; 1024 * 1024]);
    let oversized_body = Body::from_stream(futures_util::stream::iter(
        (0..101).map(move |_| Ok::<_, Infallible>(chunk.clone())),
    ));
    let oversized = Request::builder()
        .method("POST")
        .uri(format!("/tickets/{id}/attachments"))
        .header("x-hotsheet-secret", SECRET)
        .header("x-hotsheet-filename", "too-large.mov")
        .header(
            header::CONTENT_LENGTH,
            MAX_ATTACHMENT_BODY_BYTES + 1024 * 1024,
        )
        .body(oversized_body)
        .unwrap();
    assert_eq!(
        app.oneshot(oversized).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn provider_attachment_copy_preserves_bytes_across_hosted_stores() {
    let (_dir, state) = state();
    let app = app(state);
    let source = body_json(
        app.clone()
            .oneshot(authed("POST", "/tickets", Some(r#"{"title":"source"}"#)))
            .await
            .unwrap(),
    )
    .await;
    let source_id = source["id"].as_str().unwrap();
    let upload = Request::builder()
        .method("POST")
        .uri(format!("/tickets/{source_id}/attachments"))
        .header("x-hotsheet-secret", SECRET)
        .header("x-hotsheet-filename", "evidence.txt")
        .body(Body::from("preserved bytes"))
        .unwrap();
    let attached = body_json(app.clone().oneshot(upload).await.unwrap()).await;
    let attachment_id = attached["attachments"][0]["id"].as_str().unwrap();
    let source_connection = body_json(
        app.clone()
            .oneshot(authed("GET", "/providers", None))
            .await
            .unwrap(),
    )
    .await[0]["connection_id"]
        .as_str()
        .unwrap()
        .to_string();

    let destination_dir = tempfile::tempdir().unwrap();
    FsStore::init(destination_dir.path(), &StoreMetadata::new("DS")).unwrap();
    let register_body = format!(r#"{{"path":"{}"}}"#, destination_dir.path().display());
    let destination_connection = body_json(
        app.clone()
            .oneshot(authed("POST", "/stores", Some(&register_body)))
            .await
            .unwrap(),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let destination = body_json(
        app.clone()
            .oneshot(authed(
                "POST",
                &format!("/providers/{destination_connection}/tickets"),
                Some(r#"{"title":"destination"}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    let destination_id = destination["native_id"].as_str().unwrap();
    let copy_body = serde_json::json!({
        "source": {"connection_id": source_connection, "native_id": source_id, "attachment_id": attachment_id},
        "destination": {"connection_id": destination_connection, "native_id": destination_id}
    });
    let response = app
        .clone()
        .oneshot(authed(
            "POST",
            "/provider-attachments/copy",
            Some(&copy_body.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let copied = body_json(response).await;
    assert_eq!(copied["attachments"][0]["filename"], "evidence.txt");
    let copied_attachment_id = copied["attachments"][0]["id"].as_str().unwrap();
    let stored = std::fs::read(
        destination_dir
            .path()
            .join("attachments")
            .join(destination_id)
            .join(copied_attachment_id)
            .join("evidence.txt"),
    )
    .unwrap();
    assert_eq!(stored, b"preserved bytes");
}

#[tokio::test]
async fn setup_endpoint_prepares_the_project_like_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let st = AppState::new(store, SECRET.into()).unwrap();

    let resp = app(st)
        .oneshot(authed("POST", "/setup/codex", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let reports = body_json(resp).await;
    assert_eq!(reports[0]["tool"], "Codex CLI");

    // Same artifacts the CLI's `setup codex` writes, into the served store.
    let agents = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(agents.contains("<!-- BEGIN hotsheet:codex -->"));
    let cfg = std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap();
    assert!(cfg.contains("mcp_servers") && cfg.contains("hotsheet"));
}

#[tokio::test]
async fn setup_endpoint_rejects_an_unknown_tool() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(authed("POST", "/setup/nope", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(
        body_json(resp).await["error"]
            .as_str()
            .unwrap()
            .contains("unknown tool 'nope'")
    );
}

#[tokio::test]
async fn setup_endpoint_needs_the_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/setup/codex")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// The `close_reason` / `closed` list filters are served through the **index**
/// (the server lists via `index.query`, not `ops::query`), so this pins that the
/// structured close tag actually round-trips to SQLite and back (HS2-61 / HS2-20).
#[tokio::test]
async fn list_filters_by_close_reason_and_closed_through_the_index() {
    let (_d, st) = state();
    let app = app(st);

    // One open ticket, one closed as `duplicate`, one closed as `completed`.
    let mut slugs = vec![];
    for title in ["still open", "a dup", "done deal"] {
        let resp = app
            .clone()
            .oneshot(authed(
                "POST",
                "/tickets",
                Some(&format!(r#"{{"title":"{title}"}}"#)),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        slugs.push(body_json(resp).await["slug"].as_str().unwrap().to_string());
    }
    // Need a real duplicate target for reason=duplicate.
    let dup_target = &slugs[0];
    for (slug, body) in [
        (
            &slugs[1],
            format!(r#"{{"reason":"duplicate","duplicate_of":"{dup_target}"}}"#),
        ),
        (&slugs[2], r#"{"reason":"completed"}"#.to_string()),
    ] {
        let resp = app
            .clone()
            .oneshot(authed(
                "POST",
                &format!("/tickets/{slug}/close"),
                Some(&body),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // closed=true → the two closed ones; closed=false → only the open one.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?closed=true", None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 2);

    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?closed=false", None))
        .await
        .unwrap();
    let open = body_json(resp).await;
    let open = open.as_array().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["title"], "still open");

    // close_reason=completed → just the one.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets?close_reason=completed", None))
        .await
        .unwrap();
    let done = body_json(resp).await;
    let done = done.as_array().unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["title"], "done deal");
}

// ---- multi-store (HS2-87) --------------------------------------------------------

#[tokio::test]
async fn hosts_multiple_stores_and_serves_a_scoped_list() {
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{NewTicket, ops};

    let (_d, st) = state();
    let app = app(st);

    // The primary store is the single hosted entry to start.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let stores = body_json(resp).await;
    assert_eq!(stores.as_array().unwrap().len(), 1, "primary store hosted");

    // A second store on disk with one ticket.
    let dir2 = tempfile::tempdir().unwrap();
    let store2 = FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    ops::create(
        &store2,
        Ulid::new(),
        "BB",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "second-store ticket".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();

    // Register it.
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let info = body_json(resp).await;
    let id2 = info["id"].as_str().unwrap().to_string();
    assert_eq!(info["prefix"], "BB");
    assert_eq!(info["tickets"], 1);

    // Now two stores are hosted.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores", None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 2);

    // The store-scoped list serves the second store's ticket from its own index.
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/stores/{id2}/tickets"), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let rows = body_json(resp).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], "second-store ticket");

    // Re-registering the same store is idempotent (200, not a duplicate).
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores", None))
        .await
        .unwrap();
    assert_eq!(
        body_json(resp).await.as_array().unwrap().len(),
        2,
        "no duplicate"
    );

    // Unknown store id → 404; a non-store path → 400.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores/deadbeefdeadbeef/tickets", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/stores",
            Some(r#"{"path":"/nonexistent/nope"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn store_scoped_writes_are_isolated_to_their_store() {
    let (_d, st) = state();
    let app = app(st);

    // Register a second store, empty to start.
    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    let id2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // Create a ticket in the SCOPED store.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/stores/{id2}/tickets"),
            Some(r#"{"title":"scoped work","priority":"high"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let slug = created["slug"].as_str().unwrap().to_string();
    assert!(
        slug.starts_with("BB-"),
        "minted with the scoped store's prefix"
    );

    // It shows in the scoped list but NOT in the default store's list (isolation).
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/stores/{id2}/tickets"), None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 1);
    let resp = app
        .clone()
        .oneshot(authed("GET", "/tickets", None))
        .await
        .unwrap();
    assert_eq!(
        body_json(resp).await.as_array().unwrap().len(),
        0,
        "default store untouched"
    );

    // PATCH via the scoped route.
    let resp = app
        .clone()
        .oneshot(authed(
            "PATCH",
            &format!("/stores/{id2}/tickets/{slug}"),
            Some(r#"{"status":"started"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["status"], "started");

    // Close via the scoped route.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/stores/{id2}/tickets/{slug}/close"),
            Some(r#"{"reason":"completed"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["close_reason"], "completed");

    // GET one scoped ticket.
    let resp = app
        .clone()
        .oneshot(authed(
            "GET",
            &format!("/stores/{id2}/tickets/{slug}"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["slug"], slug);

    // Writing to an unknown store id → 404.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/stores/deadbeefdeadbeef/tickets",
            Some(r#"{"title":"x"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_registered_store_is_index_write_locked_and_the_primary_is_skipped() {
    use hotsheet_server::lifecycle;

    // Isolate the machine home so the lock files land in a temp dir (nextest runs each test
    // in its own process, so this env write is safe).
    let home = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("HOTSHEET_HOME", home.path()) };

    let dir1 = tempfile::tempdir().unwrap();
    let store1 = FsStore::init(dir1.path(), &StoreMetadata::new("AA")).unwrap();
    let st = AppState::new(store1, SECRET.into()).unwrap();
    let app = app(st.clone());

    // Register a second store.
    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Before publishing (not a "real" run), no writer locks are taken — tests stay hermetic.
    assert!(!lifecycle::is_writer_locked(dir2.path()));

    // Publishing marks this as a real machine server; every *additional* hosted store then
    // gets its own index-writer lock, but the primary is skipped (the binary locks that).
    st.publish_instances("http://127.0.0.1:0".into(), "2026-08-23T00:00:00Z".into());
    assert!(
        lifecycle::is_writer_locked(dir2.path()),
        "the registered store should be index-write-locked"
    );
    assert!(
        !lifecycle::is_writer_locked(dir1.path()),
        "the primary store is locked by the binary, not the server state"
    );

    unsafe { std::env::remove_var("HOTSHEET_HOME") };
}

#[tokio::test]
async fn a_registered_store_gets_its_own_watcher() {
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::{NewTicket, ops};

    let (_d, st) = state();
    let app = app(st);

    // Register an empty second store.
    let dir2 = tempfile::tempdir().unwrap();
    let store2 = FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    let id2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // Write a ticket to the registered store's disk WITHOUT the API (a CLI/git edit).
    ops::create(
        &store2,
        Ulid::new(),
        "BB",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "external to store2".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();

    // The per-store watcher should reindex it; poll the scoped list until it appears. Use a
    // non-blocking async sleep (a blocking sleep starves the runtime the watcher shares).
    for _ in 0..80 {
        let resp = app
            .clone()
            .oneshot(authed(
                "GET",
                &format!("/stores/{id2}/tickets?text=external"),
                None,
            ))
            .await
            .unwrap();
        if body_json(resp).await.as_array().unwrap().len() == 1 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the registered store's watcher did not reindex the external write within 4s");
}

#[tokio::test]
async fn resolve_finds_a_ulid_in_whichever_hosted_store_holds_it() {
    let (_d, st) = state();
    let app = app(st);

    // A ticket in the default store.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"in default"}"#),
        ))
        .await
        .unwrap();
    let default_id = body_json(resp).await["id"].as_str().unwrap().to_string();

    // A second store with its own ticket.
    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    let store2 = body_json(resp).await["id"].as_str().unwrap().to_string();
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/stores/{store2}/tickets"),
            Some(r#"{"title":"in store2"}"#),
        ))
        .await
        .unwrap();
    let ulid2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // Resolving each ULID reports the correct hosting store + the ticket.
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/resolve/{default_id}"), None))
        .await
        .unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["title"], "in default");
    assert!(got["store"].is_string());

    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/resolve/{ulid2}"), None))
        .await
        .unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["title"], "in store2");
    assert_eq!(got["store"], store2, "resolved to the store that hosts it");

    // Unknown ULID → 404; a non-ULID → 400.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/resolve/01ARZ3NDEKTSV4RRFFQ69G5FAV", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = app
        .clone()
        .oneshot(authed("GET", "/resolve/not-a-ulid", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn broker_mode_routes_terminals_and_survives_a_server_restart() {
    use hotsheet_server::terminal_broker::TerminalBroker;
    use std::sync::Arc;

    // A broker (stand-in for the separate `hotsheet-terminal-broker` process) on a temp socket.
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("b.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(hotsheet_terminals::serve_broker(
        listener,
        "proj".into(),
        Arc::new(hotsheet_terminals::TerminalManager::new()),
    ));

    // "Server 1": open a terminal — it lives in the broker, not this server.
    let app1 = app(state()
        .1
        .with_terminal_broker_at(TerminalBroker::at(&sock, "proj")));
    let resp = app1
        .clone()
        .oneshot(authed(
            "POST",
            "/terminals",
            Some(r#"{"command":"cat","id":"tb"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["id"], "tb");

    // (The live WS attach now streams through the broker too — covered by the real-socket E2E
    // `broker_mode_attach_streams_through_the_broker` in tests/terminal_ws.rs, which needs a
    // bound server + WS client rather than a `oneshot` GET.)

    // "Server 2": a FRESH server (the restart) pointing at the SAME broker — the terminal is
    // still there and usable. This is the survive-a-restart property.
    let app2 = app(state()
        .1
        .with_terminal_broker_at(TerminalBroker::at(&sock, "proj")));
    let list = body_json(
        app2.clone()
            .oneshot(authed("GET", "/terminals", None))
            .await
            .unwrap(),
    )
    .await;
    assert!(
        list.as_array().unwrap().iter().any(|t| t["id"] == "tb"),
        "the terminal survived the server restart: {list}"
    );
    let read = body_json(
        app2.oneshot(authed("GET", "/terminals/tb", None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        read["alive"], true,
        "the reattached terminal is still alive"
    );
}

#[tokio::test]
async fn terminal_surfaces_osc7_cwd_in_its_state() {
    let (_d, st) = state();
    let app = app(st);

    // A terminal whose child emits an OSC 7 (cwd) sequence, then some text. The drain thread
    // parses it and the state shows up on GET /terminals/{id}. The ESC/BEL bytes are real
    // (JSON \u escapes); printf prints its arg literally.
    let body =
        "{\"command\":\"printf\",\"args\":[\"\\u001b]7;file://host/tmp/osc-e2e\\u0007ready\\n\"]}";
    let resp = app
        .clone()
        .oneshot(authed("POST", "/terminals", Some(body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let id = body_json(resp).await["id"].as_str().unwrap().to_string();

    // Poll the state until the parsed cwd appears.
    let mut cwd = None;
    for _ in 0..60 {
        let resp = app
            .clone()
            .oneshot(authed("GET", &format!("/terminals/{id}"), None))
            .await
            .unwrap();
        let v = body_json(resp).await;
        if let Some(c) = v.get("cwd").and_then(|c| c.as_str()) {
            cwd = Some(c.to_string());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(
        cwd.as_deref(),
        Some("/tmp/osc-e2e"),
        "OSC 7 cwd should surface"
    );
}

#[tokio::test]
async fn terminals_open_read_input_and_kill() {
    let (_d, st) = state();
    let app = app(st);

    // Open a `cat` terminal (echoes stdin) so we can test both output + input.
    let resp = app
        .clone()
        .oneshot(authed("POST", "/terminals", Some(r#"{"command":"cat"}"#)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let opened = body_json(resp).await;
    let id = opened["id"].as_str().unwrap().to_string();
    assert_eq!(opened["alive"], true);

    // It appears in the list.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/terminals", None))
        .await
        .unwrap();
    let list = body_json(resp).await;
    assert!(
        list.as_array().unwrap().iter().any(|t| t["id"] == id),
        "the terminal is listed"
    );

    // Write to the PTY; `cat` echoes it back into the scrollback.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/terminals/{id}/input"),
            Some(r#"{"data":"hotsheet-echo\n"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Poll the scrollback until the echo shows up (PTY output is async).
    let mut saw = false;
    for _ in 0..40 {
        let resp = app
            .clone()
            .oneshot(authed("GET", &format!("/terminals/{id}"), None))
            .await
            .unwrap();
        let read = body_json(resp).await;
        if read["scrollback"]
            .as_str()
            .unwrap_or("")
            .contains("hotsheet-echo")
        {
            saw = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(saw, "cat echoed the input into the scrollback");

    // Kill it, then a read is 404.
    let resp = app
        .clone()
        .oneshot(authed("DELETE", &format!("/terminals/{id}"), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // A write/read to an unknown terminal is 404; auth is required.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/terminals/nope", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = hotsheet_server::app(state().1)
        .oneshot(
            Request::builder()
                .uri("/terminals")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_connected_terminal_feeds_its_busy_into_the_connection_registry() {
    let (_d, st) = state();
    let app = app(st);

    // A `cat` terminal registered as a claude connection. `cat` echoes its input, so writing
    // OSC-133 markers drives the terminal's BusyDetector, which the feed task mirrors into the
    // connection registry (GET /connections).
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/terminals",
            Some(r#"{"command":"cat","connect":"claude","id":"tc1"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // It appears as a connection: tool=claude, role=main.
    let conns = body_json(
        app.clone()
            .oneshot(authed("GET", "/connections", None))
            .await
            .unwrap(),
    )
    .await;
    let c = conns
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "tc1")
        .expect("the connected terminal is listed");
    assert_eq!(c["tool"], "claude");
    assert_eq!(c["role"], "main");

    // Helper: is connection tc1 currently busy per GET /connections?
    let is_busy = |app: axum::Router| async move {
        let conns = body_json(
            app.oneshot(authed("GET", "/connections", None))
                .await
                .unwrap(),
        )
        .await;
        conns
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "tc1")
            .and_then(|c| c["busy"].as_bool())
            .unwrap_or(false)
    };

    // Write OSC-133 C (command output begins → busy). The trailing newline flushes the tty
    // line so `cat` echoes the raw sequence; the feed task polls every 500ms.
    app.clone()
        .oneshot(authed(
            "POST",
            "/terminals/tc1/input",
            Some("{\"data\":\"\\u001b]133;C\\u0007\\n\"}"),
        ))
        .await
        .unwrap();
    let mut saw_busy = false;
    for _ in 0..40 {
        if is_busy(app.clone()).await {
            saw_busy = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        saw_busy,
        "an OSC-133 command-start should mark the connection busy"
    );

    // Write OSC-133 D (command finished → idle) — the feed set_idles the connection.
    app.clone()
        .oneshot(authed(
            "POST",
            "/terminals/tc1/input",
            Some("{\"data\":\"\\u001b]133;D\\u0007\\n\"}"),
        ))
        .await
        .unwrap();
    let mut saw_idle = false;
    for _ in 0..40 {
        if !is_busy(app.clone()).await {
            saw_idle = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        saw_idle,
        "an OSC-133 command-finish should mark the connection idle"
    );
}

#[tokio::test]
async fn terminal_requires_a_command_or_a_connect_only_plugin_launch() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(authed("POST", "/terminals", Some(r#"{"id":"empty"}"#)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = String::from_utf8(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("command is required"), "{body}");
}

#[tokio::test]
async fn connect_only_terminal_runs_setup_and_the_interactive_manifest_launch() {
    let (store_dir, st) = state();
    let plugins = tempfile::tempdir().unwrap();
    let fake = plugins.path().join("fake-interactive");
    std::fs::create_dir(&fake).unwrap();
    std::fs::write(fake.join("instructions.md"), "Fake setup marker\n").unwrap();
    std::fs::write(
        fake.join("manifest.toml"),
        r#"
id = "fake-interactive"
display_name = "Fake"
product_name = "Fake Interactive"
tier = "cli-agent"
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
[launch]
program = "/bin/sh"
args = ["-c", "printf 'auto-launched:%s' \"$HOTSHEET_SECRET\""]
"#,
    )
    .unwrap();
    let app = app(st.with_plugin_dirs(vec![plugins.path().to_path_buf()]));
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/terminals",
            Some(r#"{"connect":"fake-interactive","id":"auto"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(store_dir.path().join("AGENTS.md").is_file());
    assert!(store_dir.path().join(".mcp.json").is_file());

    let mut output = String::new();
    for _ in 0..30 {
        let terminal = body_json(
            app.clone()
                .oneshot(authed("GET", "/terminals/auto", None))
                .await
                .unwrap(),
        )
        .await;
        output = terminal["scrollback"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if output.contains("auto-launched:test-secret") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(output.contains("auto-launched:test-secret"), "{output}");
}

/// The `connect` busy feed must also work when the terminal lives in the **detached broker**
/// (HS2-ERT00F item 5): the server can't read an in-process `Terminal`, so it polls the broker
/// over the socket for busy/idle and mirrors it into the connection registry.
#[tokio::test]
async fn a_broker_connected_terminal_feeds_its_busy_through_the_broker() {
    use hotsheet_server::terminal_broker::TerminalBroker;
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("b.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(hotsheet_terminals::serve_broker(
        listener,
        "proj".into(),
        Arc::new(hotsheet_terminals::TerminalManager::new()),
    ));

    let app = app(state()
        .1
        .with_terminal_broker_at(TerminalBroker::at(&sock, "proj")));

    // A `cat` terminal in the broker, registered as a claude connection.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/terminals",
            Some(r#"{"command":"cat","connect":"claude","id":"btc"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let conns = body_json(
        app.clone()
            .oneshot(authed("GET", "/connections", None))
            .await
            .unwrap(),
    )
    .await;
    assert!(
        conns
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "btc" && c["tool"] == "claude"),
        "the broker-connected terminal is registered: {conns}"
    );

    let is_busy = |app: axum::Router| async move {
        let conns = body_json(
            app.oneshot(authed("GET", "/connections", None))
                .await
                .unwrap(),
        )
        .await;
        conns
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "btc")
            .and_then(|c| c["busy"].as_bool())
            .unwrap_or(false)
    };

    // OSC-133 C (command output begins → busy); the feed polls the broker every 500ms.
    app.clone()
        .oneshot(authed(
            "POST",
            "/terminals/btc/input",
            Some("{\"data\":\"\\u001b]133;C\\u0007\\n\"}"),
        ))
        .await
        .unwrap();
    let mut saw_busy = false;
    for _ in 0..40 {
        if is_busy(app.clone()).await {
            saw_busy = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        saw_busy,
        "OSC-133 command-start marks the broker-connected terminal busy"
    );

    // OSC-133 D (command finished → idle).
    app.clone()
        .oneshot(authed(
            "POST",
            "/terminals/btc/input",
            Some("{\"data\":\"\\u001b]133;D\\u0007\\n\"}"),
        ))
        .await
        .unwrap();
    let mut saw_idle = false;
    for _ in 0..40 {
        if !is_busy(app.clone()).await {
            saw_idle = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        saw_idle,
        "OSC-133 command-finish marks the broker-connected terminal idle"
    );
}

#[tokio::test]
async fn connections_lists_what_the_driving_loop_is_running() {
    let (_d, st) = state();
    let reg = st.drive_registry();
    let app = app(st);

    // Nothing driving yet → empty.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/connections", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_json(resp).await.as_array().unwrap().is_empty());

    // Simulate a driven ticket registering (what live_drive does per turn).
    {
        let mut r = reg.lock().unwrap();
        r.register(hotsheet_aitools::Connection {
            id: "srv-01ABC".into(),
            project: "/proj".into(),
            tool: "codex".into(),
            role: hotsheet_aitools::Role::Worker,
            transport: hotsheet_aitools::Transport::AppServer,
            pid: None,
            started_at_ms: 1000,
        });
        r.note_activity("srv-01ABC", 1000);
    }
    let resp = app
        .clone()
        .oneshot(authed("GET", "/connections", None))
        .await
        .unwrap();
    let list = body_json(resp).await;
    let arr = list.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], "srv-01ABC");
    assert_eq!(arr[0]["tool"], "codex");
    assert_eq!(arr[0]["role"], "worker");

    // Auth is required.
    let resp = hotsheet_server::app(state().1)
        .oneshot(
            Request::builder()
                .uri("/connections")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn permission_round_trip_lists_answers_and_unblocks_the_tool() {
    let (_d, st) = state();
    let bridge = st.permission_bridge();
    let app = app(st);

    // A driven "tool" thread blocks on an unruled request (real blocking round-trip).
    let bt = bridge.clone();
    let waiter = std::thread::spawn(move || bt.request_blocking("conn-1", "Bash", "rm -rf build"));

    // It shows up on GET /permissions once queued.
    let id = loop {
        let resp = app
            .clone()
            .oneshot(authed("GET", "/permissions", None))
            .await
            .unwrap();
        let list = body_json(resp).await;
        if let Some(first) = list.as_array().unwrap().first() {
            assert_eq!(first["connection"], "conn-1");
            assert_eq!(first["tool"], "Bash");
            assert_eq!(first["action"], "rm -rf build");
            assert_eq!(first["always_allow_supported"], false);
            break first["id"].as_u64().unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };

    // Answer it — allow, once (no rule persisted).
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/permissions/{id}"),
            Some(r#"{"decision":"allow","scope":"once"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ack = body_json(resp).await;
    assert_eq!(ack["connection"], "conn-1");
    assert_eq!(ack["decision"], "allow");
    assert_eq!(ack["persisted"], false);

    // The blocked tool received the human's answer.
    assert_eq!(format!("{:?}", waiter.join().unwrap()), "Allow");

    // The queue is now empty, and answering an unknown id is a 404.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/permissions/999",
            Some(r#"{"decision":"deny"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn permissions_ask_blocks_then_returns_the_human_answer() {
    let (_d, st) = state();
    let bridge = st.permission_bridge();
    let app = app(st);

    // The asking side (a Claude hook) raises a blocking request.
    let ask_app = app.clone();
    let ask = tokio::spawn(async move {
        ask_app
            .oneshot(authed(
                "POST",
                "/permissions/ask",
                Some(r#"{"connection":"claude-1","tool":"Bash","action":"rm x"}"#),
            ))
            .await
            .unwrap()
    });

    // It shows up on GET /permissions; a human answers allow.
    let id = loop {
        let resp = app
            .clone()
            .oneshot(authed("GET", "/permissions", None))
            .await
            .unwrap();
        if let Some(first) = body_json(resp).await.as_array().unwrap().first() {
            assert_eq!(first["connection"], "claude-1");
            break first["id"].as_u64().unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };
    bridge
        .resolve(
            id,
            hotsheet_aitools::PermissionDecision::Allow,
            hotsheet_aitools::PermissionScope::Once,
        )
        .unwrap();

    // The blocked ask returns the decision.
    let resp = ask.await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["decision"], "allow");
}

#[tokio::test]
async fn permissions_require_the_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(
            Request::builder()
                .uri("/permissions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_always_answer_persists_a_rule_that_auto_resolves_next_time() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let rules = dir.path().join("permissions.json");
    let st = AppState::new(store, SECRET.into())
        .unwrap()
        .with_permission_rules(&rules);
    let bridge = st.permission_bridge();
    let app = app(st);

    // Block on a request, then answer it with scope=always.
    let bt = bridge.clone();
    let waiter = std::thread::spawn(move || bt.request_blocking("c", "Bash", "ls"));
    let id = loop {
        let resp = app
            .clone()
            .oneshot(authed("GET", "/permissions", None))
            .await
            .unwrap();
        if let Some(first) = body_json(resp).await.as_array().unwrap().first() {
            assert_eq!(first["always_allow_supported"], true);
            break first["id"].as_u64().unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/permissions/{id}"),
            Some(r#"{"decision":"allow","scope":"always"}"#),
        ))
        .await
        .unwrap();
    let ack = body_json(resp).await;
    assert_eq!(ack["persisted"], true, "an always rule was written to disk");
    assert_eq!(format!("{:?}", waiter.join().unwrap()), "Allow");
    assert!(rules.exists(), "the rule file was created");

    // The same (tool, action) now auto-resolves — no human, nothing pending.
    let bt = bridge.clone();
    let auto = std::thread::spawn(move || bt.request_blocking("c", "Bash", "ls"));
    assert_eq!(format!("{:?}", auto.join().unwrap()), "Allow");
    let resp = app
        .clone()
        .oneshot(authed("GET", "/permissions", None))
        .await
        .unwrap();
    assert!(
        body_json(resp).await.as_array().unwrap().is_empty(),
        "auto-resolved: nothing queued"
    );
}

#[tokio::test]
async fn long_poll_hands_back_a_cursor_then_replays_events_since_it() {
    let (_d, st) = state();
    let app = app(st);

    // Handshake: no `since` → the current cursor (0) + no backlog.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/ws/poll?secret=test-secret", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let hs = body_json(resp).await;
    assert_eq!(hs["cursor"], 0);
    assert_eq!(hs["events"].as_array().unwrap().len(), 0);
    assert_eq!(hs["overflow"], false);

    // Two writes emit two change events.
    for t in [r#"{"title":"one"}"#, r#"{"title":"two"}"#] {
        app.clone()
            .oneshot(authed("POST", "/tickets", Some(t)))
            .await
            .unwrap();
    }

    // Poll since the handshake cursor → both events, cursor advanced to 2.
    let resp = app
        .clone()
        .oneshot(authed("GET", "/ws/poll?secret=test-secret&since=0", None))
        .await
        .unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["cursor"], 2);
    let evs = got["events"].as_array().unwrap();
    assert_eq!(evs.len(), 2);
    assert_eq!(evs[0]["kind"], "created");
    assert!(evs[0]["slug"].as_str().unwrap().starts_with("HS-"));

    // Caught up: a poll at the current cursor with a tiny timeout returns empty, same cursor.
    let resp = app
        .clone()
        .oneshot(authed(
            "GET",
            "/ws/poll?secret=test-secret&since=2&timeout_ms=1",
            None,
        ))
        .await
        .unwrap();
    let caught_up = body_json(resp).await;
    assert_eq!(caught_up["cursor"], 2);
    assert_eq!(caught_up["events"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn long_poll_requires_the_secret() {
    let (_d, st) = state();
    let resp = app(st)
        .oneshot(authed("GET", "/ws/poll?secret=wrong&since=0", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn long_poll_blocks_then_wakes_on_the_next_event() {
    let (_d, st) = state();
    let app = app(st);

    // Start a poll that's caught up (since=0, cursor=0) with a generous timeout — it blocks.
    let poll_app = app.clone();
    let poll = tokio::spawn(async move {
        poll_app
            .oneshot(authed(
                "GET",
                "/ws/poll?secret=test-secret&since=0&timeout_ms=5000",
                None,
            ))
            .await
            .unwrap()
    });

    // Give the poll a moment to subscribe, then write — which should wake it.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    app.clone()
        .oneshot(authed("POST", "/tickets", Some(r#"{"title":"live"}"#)))
        .await
        .unwrap();

    let resp = poll.await.unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["cursor"], 1, "the write advanced the cursor");
    let evs = got["events"].as_array().unwrap();
    assert_eq!(evs.len(), 1, "the blocked poll woke on the new event");
    assert_eq!(evs[0]["kind"], "created");
}

#[tokio::test]
async fn cross_store_copy_and_move_between_hosted_stores() {
    let (_d, st) = state();
    let app = app(st);

    // A ticket in the default store, and a second hosted store to copy/move into.
    let resp = app
        .clone()
        .oneshot(authed("POST", "/tickets", Some(r#"{"title":"portable"}"#)))
        .await
        .unwrap();
    let id = body_json(resp).await["id"].as_str().unwrap().to_string();

    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("DS")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    let store2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // copy → 201, NEW ULID in DS with copied_from provenance; source still resolves.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{id}/copy"),
            Some(&format!(r#"{{"to":"{store2}"}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let copied = body_json(resp).await;
    assert_ne!(copied["id"].as_str().unwrap(), id, "copy mints a new ULID");
    assert!(copied["slug"].as_str().unwrap().starts_with("DS-"));
    assert_eq!(copied["copied_from"], id);
    assert_eq!(copied["store"], store2);

    // move without confirm → 400 naming the caveat.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{id}/move"),
            Some(&format!(r#"{{"to":"{store2}"}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // move with confirm → same ULID now hosted by store2 (resolve follows the tombstone).
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{id}/move"),
            Some(&format!(r#"{{"to":"{store2}","confirm":true}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let moved = body_json(resp).await;
    assert_eq!(moved["id"], id, "move keeps the ULID");
    assert_eq!(moved["store"], store2);
    assert!(moved["tombstone"].as_str().unwrap().starts_with("HS-"));

    // The global resolve now lands in store2 (the live instance, past the tombstone).
    let resp = app
        .clone()
        .oneshot(authed("GET", &format!("/resolve/{id}"), None))
        .await
        .unwrap();
    let got = body_json(resp).await;
    assert_eq!(got["store"], store2, "resolves to the destination store");
    assert_ne!(
        got["status"], "moved",
        "the live instance, not the tombstone"
    );
}

#[tokio::test]
async fn provider_transfer_is_idempotent_and_move_closes_source_after_copy() {
    let (_d, st) = state();
    let app = app(st);
    let created = app
        .clone()
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"portable provider ticket"}"#),
        ))
        .await
        .unwrap();
    let source_id = body_json(created).await["id"].as_str().unwrap().to_string();
    let providers = app
        .clone()
        .oneshot(authed("GET", "/providers", None))
        .await
        .unwrap();
    let source_connection = body_json(providers).await[0]["connection_id"]
        .as_str()
        .unwrap()
        .to_string();

    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("DS")).unwrap();
    let registered = app
        .clone()
        .oneshot(authed(
            "POST",
            "/stores",
            Some(&format!(r#"{{"path":"{}"}}"#, dir2.path().display())),
        ))
        .await
        .unwrap();
    let destination = body_json(registered).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let transfer = format!(
        r#"{{"source":{{"connection_id":"{source_connection}","native_id":"{source_id}"}},"destination_connection":"{destination}","operation_id":"server-op-1"}}"#
    );
    let first = app
        .clone()
        .oneshot(authed("POST", "/provider-transfers/copy", Some(&transfer)))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);
    let first_destination = body_json(first).await["destination"].clone();
    let retry = app
        .clone()
        .oneshot(authed("POST", "/provider-transfers/copy", Some(&transfer)))
        .await
        .unwrap();
    assert_eq!(body_json(retry).await["destination"], first_destination);
    assert_eq!(
        FsStore::open(dir2.path())
            .unwrap()
            .list_tickets()
            .unwrap()
            .len(),
        1
    );

    let moving = transfer.trim_end_matches('}').to_string() + r#", "confirm":true}"#;
    let moved = app
        .clone()
        .oneshot(authed("POST", "/provider-transfers/move", Some(&moving)))
        .await
        .unwrap();
    assert_eq!(moved.status(), StatusCode::OK);
    let source = app
        .oneshot(authed("GET", &format!("/tickets/{source_id}"), None))
        .await
        .unwrap();
    assert_eq!(body_json(source).await["close_reason"], "obsolete");
}

#[tokio::test]
async fn direct_github_provider_runs_through_provider_routes_without_mirroring() {
    let (dir, st) = state();
    let transport = Arc::new(FakeGitHub {
        responses: Mutex::new(
            vec![
                github_response(200, serde_json::json!([github_issue(11, "remote issue")])),
                github_response(201, github_issue(12, "created remotely")),
            ]
            .into(),
        ),
    });
    let provider = GitHubProvider::new(
        GitHubConfig::new("github-main", "acme/repo", "fixture-token"),
        transport,
    );
    let app = app(st.with_ticket_provider(Arc::new(provider)));

    let providers = app
        .clone()
        .oneshot(authed("GET", "/providers", None))
        .await
        .unwrap();
    let providers = body_json(providers).await;
    assert!(
        providers.as_array().unwrap().iter().any(|value| {
            value["connection_id"] == "github-main" && value["provider"] == "github"
        })
    );
    let listed = app
        .clone()
        .oneshot(authed("GET", "/providers/github-main/tickets", None))
        .await
        .unwrap();
    let listed = body_json(listed).await;
    assert_eq!(listed[0]["qualified_id"], "github-main:11");

    let created = app
        .oneshot(authed(
            "POST",
            "/providers/github-main/tickets",
            Some(r#"{"title":"created remotely"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(body_json(created).await["native_id"], "12");
    assert!(
        FsStore::open(dir.path())
            .unwrap()
            .list_tickets()
            .unwrap()
            .is_empty(),
        "direct provider operations must not mirror into the git store"
    );
}

#[tokio::test]
async fn direct_gitlab_and_jira_providers_run_through_the_same_server_contract() {
    let (dir, st) = state();
    let gitlab = GitLabProvider::new(
        GitLabConfig {
            connection_id: "gitlab-main".into(),
            project: "acme/repo".into(),
            api_base: "https://gitlab.test/api/v4".into(),
            token: "fixture".into(),
            default: false,
        },
        Arc::new(FakeGitHub {
            responses: Mutex::new(
                vec![github_response(
                    200,
                    serde_json::json!([{
                        "iid":4,"title":"gitlab remote","description":"body","state":"opened",
                        "web_url":"https://gitlab.test/acme/repo/-/issues/4",
                        "created_at":"2026-08-26T00:00:00Z","updated_at":"2026-08-26T00:01:00Z",
                        "closed_at":null,"labels":[],"assignees":[]
                    }]),
                )]
                .into(),
            ),
        }),
    );
    let jira = JiraProvider::new(
        JiraConfig {
            connection_id: "jira-eng".into(),
            project_key: "ENG".into(),
            base_url: "https://jira.test".into(),
            email: "dev@example.com".into(),
            token: "fixture".into(),
            default: false,
        },
        Arc::new(FakeGitHub {
            responses: Mutex::new(
                vec![github_response(
                    200,
                    serde_json::json!({
                        "isLast":true,"issues":[{
                            "key":"ENG-9","fields":{"summary":"jira remote","description":null,
                            "status":{"statusCategory":{"key":"new"}},"priority":{"name":"Medium"},
                            "issuetype":{"name":"Task"},"labels":[],"assignee":null,
                            "created":"2026-08-26T00:00:00Z","updated":"2026-08-26T00:01:00Z",
                            "resolutiondate":null}
                        }]
                    }),
                )]
                .into(),
            ),
        }),
    );
    let app = app(st
        .with_ticket_provider(Arc::new(gitlab))
        .with_ticket_provider(Arc::new(jira)));
    for (connection, qualified) in [
        ("gitlab-main", "gitlab-main:4"),
        ("jira-eng", "jira-eng:ENG-9"),
    ] {
        let response = app
            .clone()
            .oneshot(authed(
                "GET",
                &format!("/providers/{connection}/tickets"),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await[0]["qualified_id"], qualified);
    }
    assert!(
        FsStore::open(dir.path())
            .unwrap()
            .list_tickets()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn provider_connections_crud_keeps_only_references_and_one_default() {
    let (_dir, st) = state();
    let app = app(st);
    let github = serde_json::json!({
        "id":"github-main","provider":"github","locator":"acme/repo",
        "name":"Public bugs","default":true,
        "settings":{"credential":{"secret":"github-work"}}
    });
    let response = app
        .clone()
        .oneshot(authed(
            "POST",
            "/provider-connections",
            Some(&github.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(!body_json(response).await.to_string().contains("token"));

    let gitlab = serde_json::json!({
        "id":"gitlab-team","provider":"gitlab","locator":"team/project",
        "name":null,"default":true,
        "settings":{"credential":{"secret":"gitlab-work"}}
    });
    assert_eq!(
        app.clone()
            .oneshot(authed(
                "POST",
                "/provider-connections",
                Some(&gitlab.to_string())
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );
    let listed = body_json(
        app.clone()
            .oneshot(authed("GET", "/provider-connections", None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert_eq!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .filter(|connection| connection["default"] == true)
            .count(),
        1
    );
    assert_eq!(listed[1]["id"], "gitlab-team");

    let update = serde_json::json!({
        "id":"ignored-by-path","provider":"github","locator":"acme/renamed",
        "name":"Renamed","default":true,
        "settings":{"credential":{"secret":"github-work"}}
    });
    let updated = body_json(
        app.clone()
            .oneshot(authed(
                "PATCH",
                "/provider-connections/github-main",
                Some(&update.to_string()),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(updated["id"], "github-main");
    assert_eq!(updated["locator"], "acme/renamed");

    assert_eq!(
        app.clone()
            .oneshot(authed("DELETE", "/provider-connections/gitlab-team", None,))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let listed = body_json(
        app.oneshot(authed("GET", "/provider-connections", None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn persistent_mode_writes_a_file_backed_index_for_registered_stores() {
    // Hermetic: point HOTSHEET_HOME at a tempdir (nextest isolates each test process).
    let home = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOTSHEET_HOME", home.path());
    }

    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let st = AppState::new(store, SECRET.into())
        .unwrap()
        .with_persistent_registered_indexes();
    let app = app(st);

    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let id2 = body_json(resp).await["id"].as_str().unwrap().to_string();

    // The registered store's index was written under ${HOTSHEET_HOME}/index/<id>.sqlite.
    let index_file = home.path().join("index").join(format!("{id2}.sqlite"));
    assert!(
        index_file.is_file(),
        "file-backed index persisted: {}",
        index_file.display()
    );

    unsafe {
        std::env::remove_var("HOTSHEET_HOME");
    }
}

#[tokio::test]
async fn startup_discovery_hosts_stores_from_stores_json() {
    let home = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOTSHEET_HOME", home.path());
    }

    // Two stores on disk; one listed in stores.json, one bogus path (must be skipped).
    let good = tempfile::tempdir().unwrap();
    FsStore::init(good.path(), &StoreMetadata::new("BB")).unwrap();
    let stores_json = serde_json::json!({
        "stores": [good.path().to_string_lossy(), "/nonexistent/nope"]
    });
    std::fs::write(home.path().join("stores.json"), stores_json.to_string()).unwrap();

    // Primary store + discovery.
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let st = AppState::new(store, SECRET.into()).unwrap();
    let hosted = st.host_configured_stores();
    assert_eq!(hosted, 1, "one good store hosted, the bogus path skipped");

    // /stores now lists the primary + the discovered one.
    let app = app(st);
    let resp = app
        .clone()
        .oneshot(authed("GET", "/stores", None))
        .await
        .unwrap();
    let stores = body_json(resp).await;
    let arr = stores.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(
        arr.iter().any(|s| s["prefix"] == "BB"),
        "the discovered store is served"
    );

    unsafe {
        std::env::remove_var("HOTSHEET_HOME");
    }
}

#[tokio::test]
async fn one_machine_server_is_discoverable_for_every_hosted_store() {
    use hotsheet_server::lifecycle;

    let home = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOTSHEET_HOME", home.path());
    }

    // Primary store + machine server.
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let st = AppState::new(store, SECRET.into()).unwrap();

    // Publish the machine server's coordinates → the primary gets a discovery file.
    st.publish_instances(
        "http://127.0.0.1:9999".into(),
        "2026-08-22T00:00:00Z".into(),
    );
    let app = app(st);

    // Register a second store at runtime; it advertises the same machine server.
    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    let resp = app
        .clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // "Who serves project X?" resolves to the ONE machine server for BOTH projects.
    let primary = lifecycle::find_instance(dir.path()).expect("primary discoverable");
    assert_eq!(primary.url, "http://127.0.0.1:9999");
    let second = lifecycle::find_instance(dir2.path()).expect("registered store discoverable");
    assert_eq!(second.url, "http://127.0.0.1:9999");
    assert_eq!(
        second.pid, primary.pid,
        "same machine server process hosts both"
    );

    unsafe {
        std::env::remove_var("HOTSHEET_HOME");
    }
}

#[tokio::test]
async fn background_sync_covers_every_hosted_store() {
    use hotsheet_server::sync_loop::sync_all;
    use hotsheet_ticketing::sync::SyncReport;

    let (_d, st) = state();
    let app = app(st.clone());

    // Register a second (local-only, no-remote) store.
    let dir2 = tempfile::tempdir().unwrap();
    FsStore::init(dir2.path(), &StoreMetadata::new("BB")).unwrap();
    let body = format!(r#"{{"path":"{}"}}"#, dir2.path().display());
    app.clone()
        .oneshot(authed("POST", "/stores", Some(&body)))
        .await
        .unwrap();

    // One sync pass reports on BOTH hosted stores; a local-only store is NoRemote.
    let reports = sync_all(&st);
    assert_eq!(reports.len(), 2, "both hosted stores synced");
    assert!(reports.iter().all(|(_, r)| *r == SyncReport::NoRemote));
}

#[tokio::test]
async fn claim_next_release_renew_over_http() {
    let (_d, st) = state();
    let app = app(st);

    // A claimable Up Next ticket.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/tickets",
            Some(r#"{"title":"drain","up_next":true}"#),
        ))
        .await
        .unwrap();
    let slug = body_json(resp).await["slug"].as_str().unwrap().to_string();

    // Claim it.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            "/claim-next",
            Some(r#"{"worker":"w1","lease_minutes":15}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let claimed = body_json(resp).await;
    assert_eq!(claimed["slug"], slug);
    assert_eq!(claimed["claimed_by"], "w1");

    // Renew (holder).
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/renew"),
            Some(r#"{"worker":"w1"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // A non-holder release without force → 4xx (wrong worker).
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/release"),
            Some(r#"{"worker":"w2"}"#),
        ))
        .await
        .unwrap();
    assert!(resp.status().is_client_error());

    // Holder releases → claimed_by cleared.
    let resp = app
        .clone()
        .oneshot(authed(
            "POST",
            &format!("/tickets/{slug}/release"),
            Some(r#"{"worker":"w1"}"#),
        ))
        .await
        .unwrap();
    assert!(body_json(resp).await["claimed_by"].is_null());

    // Nothing claimable now (it's claimable again but let's assert null when queue drained):
    // claim it again then complete, then claim-next returns null.
    app.clone()
        .oneshot(authed("POST", "/claim-next", Some(r#"{"worker":"w1"}"#)))
        .await
        .unwrap();
    app.clone()
        .oneshot(authed(
            "PATCH",
            &format!("/tickets/{slug}"),
            Some(r#"{"status":"completed"}"#),
        ))
        .await
        .unwrap();
    let resp = app
        .clone()
        .oneshot(authed("POST", "/claim-next", Some(r#"{}"#)))
        .await
        .unwrap();
    assert!(
        body_json(resp).await.is_null(),
        "no claimable tickets → null"
    );
}
