//! HS2-DBKM89 (GH-5 parity): deleting a GitLab ticket through a checkout moves it to Trash
//! (closed, `status:deleted`) instead of reading back as Completed; Restore returns the exact
//! prior status and state; Empty Trash and the retention sweep delete the issue only when the
//! connection may (project Owner), and otherwise report it as retained. Synthetic issues only.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_extsync::{GitHubTransport, GitLabConfig, GitLabProvider, HttpResponse};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";

/// A stateful GitLab Issues fake with GitLab REST v4 response shapes: `PUT` replaces the
/// comma-separated labels and applies `state_event`; `DELETE` answers 204 for an Owner and
/// `403 Forbidden` otherwise; a deleted issue answers `404 Issue Not Found`.
#[derive(Default)]
struct FakeGitLab {
    issues: Mutex<BTreeMap<u64, Value>>,
    owner: Mutex<bool>,
    clock: Mutex<u64>,
}

fn reply(status: u16, body: Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: HashMap::from([("x-next-page".to_string(), String::new())]),
        body: body.to_string(),
    }
}

impl FakeGitLab {
    fn seed(&self, iid: u64, state: &str, labels: &[&str]) {
        self.issues.lock().unwrap().insert(
            iid,
            json!({
                "id": 9000 + iid, "iid": iid, "project_id": 42,
                "title": format!("Synthetic issue {iid}"), "description": "Synthetic.",
                "state": state,
                "web_url": format!("https://gitlab.test/acme/repo/-/issues/{iid}"),
                "created_at": "2026-11-01T00:00:00.000Z", "updated_at": "2026-11-01T00:01:00.000Z",
                "closed_at": (state == "closed").then_some("2026-11-01T00:01:00.000Z"),
                "labels": labels, "assignees": [], "type": "ISSUE"
            }),
        );
    }

    fn issue(&self, iid: u64) -> Option<Value> {
        self.issues.lock().unwrap().get(&iid).cloned()
    }

    fn labels(&self, iid: u64) -> Vec<String> {
        let mut names = self.issue(iid).unwrap()["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|label| label.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn age(&self, iid: u64, updated_at: &str) {
        self.issues.lock().unwrap().get_mut(&iid).unwrap()["updated_at"] = json!(updated_at);
    }

    fn put(&self, issue: &mut Value, body: &Value) {
        let now = {
            let mut clock = self.clock.lock().unwrap();
            *clock += 1;
            format!("2026-11-24T00:00:{:02}.000Z", *clock)
        };
        if let Some(labels) = body.get("labels").and_then(Value::as_str) {
            issue["labels"] = json!(
                labels
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>()
            );
        }
        match body.get("state_event").and_then(Value::as_str) {
            Some("close") if issue["state"] != "closed" => {
                issue["state"] = json!("closed");
                issue["closed_at"] = json!(now);
            }
            Some("reopen") if issue["state"] == "closed" => {
                issue["state"] = json!("opened");
                issue["closed_at"] = Value::Null;
            }
            _ => {}
        }
        issue["updated_at"] = json!(now);
    }
}

impl GitHubTransport for FakeGitLab {
    fn request(
        &self,
        method: &str,
        url: &str,
        _: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let path = url.split('?').next().unwrap_or(url);
        let rest = path
            .split("/projects/acme%2Frepo/issues")
            .nth(1)
            .unwrap_or("-")
            .trim_start_matches('/');
        if rest.is_empty() {
            let issues = self.issues.lock().unwrap();
            return Ok(reply(200, json!(issues.values().collect::<Vec<_>>())));
        }
        if rest.ends_with("/notes") || rest.ends_with("/resource_state_events") {
            return Ok(reply(200, json!([])));
        }
        let Ok(iid) = rest.parse::<u64>() else {
            return Ok(reply(404, json!({ "message": "404 Not found" })));
        };
        let mut issues = self.issues.lock().unwrap();
        let Some(issue) = issues.get_mut(&iid) else {
            return Ok(reply(404, json!({ "message": "404 Issue Not Found" })));
        };
        match method {
            "GET" => Ok(reply(200, issue.clone())),
            "PUT" => {
                self.put(issue, body.unwrap());
                Ok(reply(200, issue.clone()))
            }
            "DELETE" if *self.owner.lock().unwrap() => {
                issues.remove(&iid);
                Ok(HttpResponse {
                    status: 204,
                    headers: HashMap::new(),
                    body: String::new(),
                })
            }
            "DELETE" => Ok(reply(403, json!({ "message": "403 Forbidden" }))),
            _ => Ok(reply(405, json!({ "message": "405 Method Not Allowed" }))),
        }
    }
}

struct Project {
    _dir: tempfile::TempDir,
    router: axum::Router,
    state: AppState,
    base: String,
    gitlab: Arc<FakeGitLab>,
}

/// A checkout linking one Git store and one GitLab connection.
fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let store_root = dir.path().join("AA.hs2");
    FsStore::init_git(&store_root, &StoreMetadata::new("AA")).unwrap();
    let registry = dir.path().join("checkouts.json");
    let registered = CheckoutRegistry::new(&registry)
        .register(&root, Some("project"), None, vec![store_root.clone()])
        .unwrap();
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    saved["checkouts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == registered.id)
        .unwrap()["sources"]
        .as_array_mut()
        .unwrap()
        .push(
            json!({ "connection_id": "gitlab-main", "provider": "gitlab", "locator": "acme/repo" }),
        );
    std::fs::write(&registry, serde_json::to_vec(&saved).unwrap()).unwrap();
    let gitlab = Arc::new(FakeGitLab::default());
    let config = GitLabConfig {
        connection_id: "gitlab-main".into(),
        project: "acme/repo".into(),
        api_base: "https://gitlab.test/api/v4".into(),
        token: "fixture-token".into(),
        default: false,
    };
    let state = AppState::new(FsStore::open(&store_root).unwrap(), SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_checkout_registry(&registry)
        .with_ticket_provider(Arc::new(GitLabProvider::new(config, gitlab.clone())));
    Project {
        _dir: dir,
        router: app(state.clone()),
        state,
        base: format!("/checkouts/{}", registered.id),
        gitlab,
    }
}

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

#[tokio::test]
async fn hs2_dbkm89_deleting_a_started_gitlab_ticket_trashes_it_and_restore_reopens_it() {
    let project = project();
    project
        .gitlab
        .seed(5, "opened", &["category:bug", "status:started", "customer"]);
    let router = &project.router;
    let ticket = format!("{}/tickets/gitlab-main:5", project.base);

    let deleted = ok(
        router,
        "PATCH",
        &ticket,
        Some(json!({ "status": "deleted" })),
    )
    .await;
    assert_eq!(t(&deleted)["status"], "deleted", "{deleted}");
    assert_eq!(project.gitlab.issue(5).unwrap()["state"], "closed");
    assert_eq!(
        project.gitlab.labels(5),
        [
            "category:bug",
            "customer",
            "deleted-from:started",
            "priority:default",
            "status:deleted"
        ]
    );
    let read = ok(router, "GET", &ticket, None).await;
    assert_eq!(t(&read)["status"], "deleted", "never Completed: {read}");

    let restored = ok(router, "POST", &format!("{ticket}/restore"), None).await;
    assert_eq!(restored["store"], "gitlab-main");
    assert_eq!(t(&restored)["status"], "started", "{restored}");
    assert_eq!(project.gitlab.issue(5).unwrap()["state"], "opened");
    assert_eq!(
        project.gitlab.labels(5),
        [
            "category:bug",
            "customer",
            "priority:default",
            "status:started"
        ]
    );

    // A second restore is a conflict that names the reason, not a silent success.
    let (status, body) = send(router, "POST", &format!("{ticket}/restore"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("only a ticket in Trash can be restored"),
        "{body}"
    );

    // The connection route restores the same way, here to a closed Completed ticket.
    project.gitlab.seed(6, "closed", &["category:bug"]);
    ok(
        router,
        "PATCH",
        &format!("{}/tickets/gitlab-main:6", project.base),
        Some(json!({ "status": "deleted" })),
    )
    .await;
    let restored = ok(
        router,
        "POST",
        "/providers/gitlab-main/tickets/6/restore",
        None,
    )
    .await;
    assert_eq!(restored["status"], "completed", "{restored}");
    assert_eq!(restored["close_reason"], "completed");
    assert_eq!(project.gitlab.issue(6).unwrap()["state"], "closed");
}

#[tokio::test]
async fn hs2_dbkm89_empty_trash_reports_retained_issues_until_the_connection_may_delete_them() {
    let project = project();
    project.gitlab.seed(5, "closed", &["category:bug"]);
    project.gitlab.seed(6, "opened", &["category:bug"]);
    let router = &project.router;
    for iid in [5, 6] {
        ok(
            router,
            "PATCH",
            &format!("{}/tickets/gitlab-main:{iid}", project.base),
            Some(json!({ "status": "deleted" })),
        )
        .await;
    }
    let empty = format!("{}/trash/empty", project.base);

    let refused = ok(router, "POST", &empty, None).await;
    assert_eq!(refused["purged"], 0, "{refused}");
    let retained = refused["retained"].as_array().unwrap();
    assert_eq!(retained.len(), 2, "{refused}");
    assert_eq!(retained[0]["qualified_id"], "gitlab-main:5");
    assert_eq!(retained[0]["slug"], "acme/repo#5");
    let message = retained[0]["message"].as_str().unwrap();
    assert!(message.contains("cannot permanently delete"), "{refused}");
    assert!(message.contains("403 Forbidden"), "{refused}");
    assert_eq!(refused["unavailable"], json!([]));
    assert!(project.gitlab.issue(5).is_some() && project.gitlab.issue(6).is_some());

    *project.gitlab.owner.lock().unwrap() = true;
    let emptied = ok(router, "POST", &empty, None).await;
    assert_eq!(emptied["purged"], 2, "{emptied}");
    assert_eq!(emptied["tickets"], json!(["acme/repo#5", "acme/repo#6"]));
    assert_eq!(emptied["retained"], json!([]));
    assert!(project.gitlab.issue(5).is_none() && project.gitlab.issue(6).is_none());
    // Idempotent: nothing left to delete.
    let again = ok(router, "POST", &empty, None).await;
    assert_eq!(again["purged"], 0, "{again}");
}

#[tokio::test]
async fn hs2_dbkm89_the_retention_sweep_deletes_only_expired_gitlab_trash() {
    let project = project();
    *project.gitlab.owner.lock().unwrap() = true;
    for iid in [5, 6] {
        project.gitlab.seed(iid, "closed", &["category:bug"]);
        ok(
            &project.router,
            "PATCH",
            &format!("{}/tickets/gitlab-main:{iid}", project.base),
            Some(json!({ "status": "deleted" })),
        )
        .await;
    }
    // Trashed long before the default 30-day retention; 6 stays recent.
    project.gitlab.age(5, "2020-01-01T00:00:00.000Z");
    let swept = tokio::task::spawn_blocking({
        let state = project.state.clone();
        move || hotsheet_server::sync_loop::purge_all_provider_trash(&state)
    })
    .await
    .unwrap();
    assert_eq!(swept, vec![("gitlab-main".to_string(), 1, 0)]);
    assert!(project.gitlab.issue(5).is_none());
    assert!(project.gitlab.issue(6).is_some());
}

/// Checkout responses carry the ticket flattened beside `store`, or nested as `ticket`.
/// HS2-K9H38S: GitLab has no Up Next, so queueing a GitLab ticket through a checkout fails
/// as unsupported and leaves the issue untouched; clearing stays a successful no-op.
#[tokio::test]
async fn hs2_k9h38s_queueing_a_gitlab_ticket_is_refused_not_dropped() {
    let project = project();
    project
        .gitlab
        .seed(6, "opened", &["category:task", "customer"]);
    let router = &project.router;
    let ticket = format!("{}/tickets/gitlab-main:6", project.base);
    let before = project.gitlab.issue(6).unwrap();

    let (status, body) = send(router, "PATCH", &ticket, Some(json!({ "up_next": true }))).await;
    assert!(status.is_client_error(), "{status} {body}");
    assert!(body.to_string().contains("up_next"), "{body}");
    assert_eq!(project.gitlab.issue(6).unwrap(), before);

    let cleared = ok(router, "PATCH", &ticket, Some(json!({ "up_next": false }))).await;
    assert_eq!(t(&cleared)["up_next"], false, "{cleared}");
}

fn t(value: &Value) -> &Value {
    value.get("ticket").unwrap_or(value)
}
