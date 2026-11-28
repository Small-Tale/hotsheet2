//! Milestone registry routes over the real router (HS2-9PYNCN, docs/38 §38.6): list, show,
//! create, edit, close, and reopen for a checkout whose code checkout is a Git clone with an
//! upstream; refusals with their codes; one `milestones_changed` event per write that changed
//! the registry and none for a repeat or a refusal; and the sync loop committing a milestone
//! change a busy Git state left behind. Synthetic data only.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET)
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn ok(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, value) = send(router, method, uri, body).await;
    assert!(status.is_success(), "{method} {uri}: {status} {value}");
    value
}

fn run_git(root: &std::path::Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

struct Project {
    _dir: tempfile::TempDir,
    router: axum::Router,
    state: AppState,
    root: std::path::PathBuf,
    remote: std::path::PathBuf,
    base: String,
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let remote = dir.path().join("code.git");
    run_git(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", "code.git"],
    );
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    run_git(&root, &["init", "-q", "-b", "main"]);
    run_git(&root, &["config", "user.email", "dev@example.invalid"]);
    run_git(&root, &["config", "user.name", "Dev"]);
    std::fs::write(
        root.join(".gitignore"),
        "/.hotsheet2/*\n!/.hotsheet2/groups/\n!/.hotsheet2/milestones/\n",
    )
    .unwrap();
    run_git(&root, &["add", "."]);
    run_git(&root, &["commit", "-q", "-m", "Seed"]);
    run_git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    run_git(&root, &["push", "-q", "-u", "origin", "main"]);
    let store_root = dir.path().join("AA.hs2");
    FsStore::init_git(&store_root, &StoreMetadata::new("AA")).unwrap();
    let registry = dir.path().join("checkouts.json");
    let registered = CheckoutRegistry::new(&registry)
        .register(&root, Some("project"), None, vec![store_root.clone()])
        .unwrap();
    let state = AppState::new(FsStore::open(&store_root).unwrap(), SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_checkout_registry(&registry);
    let router = app(state.clone());
    Project {
        _dir: dir,
        router,
        state,
        root,
        remote,
        base: format!("/checkouts/{}/milestones", registered.id),
    }
}

impl Project {
    /// The `milestones_changed` events after `cursor` (their checkout ids) and the new cursor.
    async fn changed_events(&self, cursor: u64) -> (Vec<String>, u64) {
        let polled = ok(
            &self.router,
            "GET",
            &format!("/ws/poll?since={cursor}&timeout_ms=0"),
            None,
        )
        .await;
        let events = polled["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] == "milestones_changed")
            .map(|event| {
                assert_eq!(event["slug"], "", "{event}");
                event["id"].as_str().unwrap().to_owned()
            })
            .collect();
        (events, polled["cursor"].as_u64().unwrap())
    }

    fn checkout_id(&self) -> String {
        self.base
            .trim_start_matches("/checkouts/")
            .trim_end_matches("/milestones")
            .to_string()
    }

    fn remote_subjects(&self) -> Vec<String> {
        run_git(&self.remote, &["log", "--format=%s", "main"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    async fn refused(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        status: StatusCode,
        code: &str,
    ) {
        let (got, value) = send(&self.router, method, uri, body).await;
        assert_eq!(got, status, "{method} {uri}: {value}");
        assert_eq!(value["code"], code, "{value}");
    }
}

#[tokio::test]
async fn hs2_9pyncn_checkout_milestone_routes_write_commit_and_announce() {
    let p = project();
    let router = &p.router;
    let base = &p.base;
    let (_, cursor) = p.changed_events(0).await;
    assert_eq!(
        ok(router, "GET", base, None).await,
        json!({ "registry": true, "time_zone": "UTC", "milestones": [] })
    );

    let (status, created) = send(
        router,
        "POST",
        base,
        Some(
            json!({ "title": "Toolchain 2.0", "due_on": "2026-12-04", "description": "Ship it." }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["changed"], true);
    let id = created["milestone"]["id"].as_str().unwrap().to_string();
    let (events, cursor) = p.changed_events(cursor).await;
    assert_eq!(events, vec![p.checkout_id()]);
    assert_eq!(p.remote_subjects()[0], "Create milestone Toolchain 2.0");

    // Show by id and by title (encoded).
    let shown = ok(router, "GET", &format!("{base}/{id}"), None).await;
    assert_eq!(shown["due_on"], "2026-12-04");
    let by_title = ok(router, "GET", &format!("{base}/toolchain%202.0"), None).await;
    assert_eq!(by_title["id"], id.as_str());

    // PATCH sets, repeats (no event, no commit), and clears with null.
    let edited = ok(
        router,
        "PATCH",
        &format!("{base}/{id}"),
        Some(json!({ "due_on": "2026-12-11", "actor": { "role": "ai", "id": "test-agent" } })),
    )
    .await;
    assert_eq!(edited["changed"], true);
    let (events, cursor) = p.changed_events(cursor).await;
    assert_eq!(events.len(), 1);
    let repeat = ok(
        router,
        "PATCH",
        &format!("{base}/{id}"),
        Some(json!({ "due_on": "2026-12-11" })),
    )
    .await;
    assert_eq!(repeat["changed"], false);
    let (events, cursor) = p.changed_events(cursor).await;
    assert!(events.is_empty(), "{events:?}");
    let cleared = ok(
        router,
        "PATCH",
        &format!("{base}/{id}"),
        Some(json!({ "due_on": null, "description": null })),
    )
    .await;
    assert!(cleared["milestone"].get("due_on").is_none());

    // Close without a body (completed), close again (unchanged), reopen, close not planned.
    let closed = ok(router, "POST", &format!("{base}/{id}/close"), None).await;
    assert_eq!(closed["milestone"]["closed_reason"], "completed");
    assert!(closed["milestone"]["closed_at"].is_string());
    let again = ok(
        router,
        "POST",
        &format!("{base}/{id}/close"),
        Some(json!({})),
    )
    .await;
    assert_eq!(again["changed"], false);
    ok(router, "POST", &format!("{base}/{id}/reopen"), None).await;
    let dropped = ok(
        router,
        "POST",
        &format!("{base}/{id}/close"),
        Some(json!({ "reason": "not_planned" })),
    )
    .await;
    assert_eq!(dropped["milestone"]["closed_reason"], "not_planned");
    let (events, cursor) = p.changed_events(cursor).await;
    assert_eq!(events.len(), 4, "clear, close, reopen, close: {events:?}");
    assert_eq!(
        p.remote_subjects(),
        vec![
            format!("Close milestone {id} as not planned"),
            format!("Reopen milestone {id}"),
            format!("Close milestone {id}"),
            format!("Edit milestone {id}"),
            format!("Set due date of milestone {id} to 2026-12-11"),
            "Create milestone Toolchain 2.0".to_string(),
            "Seed".to_string(),
        ]
    );
    // Closed milestones need ?all=true.
    assert_eq!(ok(router, "GET", base, None).await["milestones"], json!([]));
    let all = ok(router, "GET", &format!("{base}?all=true"), None).await;
    assert_eq!(all["milestones"][0]["state"], "closed");
    assert_eq!(all["registry_sync"]["state"], "synced");

    // Refusals carry their codes and announce nothing.
    p.refused(
        "POST",
        base,
        Some(json!({ "title": "toolchain 2.0" })),
        StatusCode::CONFLICT,
        "milestone_title_taken",
    )
    .await;
    p.refused(
        "POST",
        base,
        Some(json!({ "title": " " })),
        StatusCode::BAD_REQUEST,
        "invalid_milestone_title",
    )
    .await;
    p.refused(
        "POST",
        base,
        Some(json!({ "title": "X", "due_on": "2026-02-30" })),
        StatusCode::BAD_REQUEST,
        "invalid_due_on",
    )
    .await;
    p.refused(
        "GET",
        &format!("{base}/Missing"),
        None,
        StatusCode::NOT_FOUND,
        "milestone_not_found",
    )
    .await;
    p.refused(
        "POST",
        &format!("{base}/{id}/close"),
        Some(json!({ "reason": "won" })),
        StatusCode::BAD_REQUEST,
        "invalid_milestone_close_reason",
    )
    .await;
    let (status, _) = send(
        router,
        "POST",
        base,
        Some(json!({ "title": "X", "color": "teal" })),
    )
    .await;
    assert!(
        status.is_client_error(),
        "unknown fields are refused: {status}"
    );
    let (status, _) = send(router, "GET", "/checkouts/nope/milestones", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // A conflicted file is listed and refuses writes.
    let file = p.root.join(format!(".hotsheet2/milestones/{id}.json"));
    std::fs::write(&file, "<<<<<<< HEAD\n{}\n=======\n{}\n>>>>>>> other\n").unwrap();
    let listed = ok(router, "GET", base, None).await;
    assert_eq!(listed["milestones"][0]["state"], "conflicted");
    p.refused(
        "POST",
        &format!("{base}/{id}/reopen"),
        None,
        StatusCode::CONFLICT,
        "milestone_registry_conflict",
    )
    .await;
    let (events, _) = p.changed_events(cursor).await;
    assert!(events.is_empty(), "{events:?}");
}

#[tokio::test]
async fn hs2_9pyncn_the_sync_loop_commits_a_milestone_change_a_busy_state_left_behind() {
    let p = project();
    run_git(&p.root, &["checkout", "-q", "--detach"]);
    let created = ok(
        &p.router,
        "POST",
        &p.base,
        Some(json!({ "title": "Waiting" })),
    )
    .await;
    let id = created["milestone"]["id"].as_str().unwrap().to_string();
    let listed = ok(&p.router, "GET", &p.base, None).await;
    assert_eq!(listed["registry_sync"]["state"], "waiting", "{listed}");
    assert_eq!(p.remote_subjects(), vec!["Seed"]);
    run_git(&p.root, &["checkout", "-q", "main"]);
    hotsheet_server::sync_loop::sync_group_registries(&p.state);
    assert_eq!(
        p.remote_subjects(),
        vec!["Update milestone registry", "Seed"]
    );
    assert_eq!(
        run_git(&p.remote, &["show", "--name-only", "--format=", "main"]),
        format!(".hotsheet2/milestones/{id}.json")
    );
    let listed = ok(&p.router, "GET", &p.base, None).await;
    assert_eq!(listed["registry_sync"]["state"], "synced", "{listed}");
}
