//! GH-5: deleting a GitHub Issues ticket through a checkout moves it to Trash (closed as
//! not planned, `status:deleted`) instead of reading back as Completed; Restore returns the
//! exact prior status; Empty Trash and the retention sweep delete the issue only when the
//! connection may, and otherwise report it as retained. Synthetic issues only.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_extsync::{GitHubConfig, GitHubProvider, GitHubTransport, HttpResponse};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";

/// A stateful GitHub Issues fake with GitHub's response shapes: PATCH replaces labels and
/// moves `state`/`state_reason`/`closed_at`; GraphQL `deleteIssue` succeeds for an admin
/// and answers 200 with a FORBIDDEN error otherwise; a deleted issue answers 410.
#[derive(Default)]
struct FakeGitHub {
    issues: Mutex<BTreeMap<u64, Value>>,
    deleted: Mutex<Vec<u64>>,
    admin: Mutex<bool>,
    clock: Mutex<u64>,
}

fn reply(status: u16, body: Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: HashMap::new(),
        body: body.to_string(),
    }
}

impl FakeGitHub {
    fn seed(&self, number: u64, state: &str, reason: Option<&str>, labels: &[&str]) {
        self.issues.lock().unwrap().insert(
            number,
            json!({
                "number": number, "node_id": format!("I_fake{number}"),
                "title": format!("Synthetic issue {number}"), "body": "Synthetic.",
                "state": state, "state_reason": reason,
                "html_url": format!("https://github.com/acme/repo/issues/{number}"),
                "created_at": "2026-11-01T00:00:00Z", "updated_at": "2026-11-01T00:01:00Z",
                "closed_at": (state == "closed").then_some("2026-11-01T00:01:00Z"),
                "labels": labels.iter().map(|name| json!({ "name": name })).collect::<Vec<_>>(),
                "assignees": [], "pull_request": null
            }),
        );
    }

    fn issue(&self, number: u64) -> Option<Value> {
        self.issues.lock().unwrap().get(&number).cloned()
    }

    fn labels(&self, number: u64) -> Vec<String> {
        let mut names = self.issue(number).unwrap()["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|label| label["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn age(&self, number: u64, updated_at: &str) {
        self.issues.lock().unwrap().get_mut(&number).unwrap()["updated_at"] = json!(updated_at);
    }
}

impl GitHubTransport for FakeGitHub {
    fn request(
        &self,
        method: &str,
        url: &str,
        _: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let path = url.split('?').next().unwrap_or(url);
        if method == "POST" && path.ends_with("/graphql") {
            if !*self.admin.lock().unwrap() {
                return Ok(reply(
                    200,
                    json!({ "data": { "deleteIssue": null }, "errors": [{
                        "type": "FORBIDDEN", "path": ["deleteIssue"],
                        "message": "octocat does not have the correct permissions to execute `DeleteIssue`"
                    }] }),
                ));
            }
            let node = body.unwrap()["variables"]["id"].clone();
            let mut issues = self.issues.lock().unwrap();
            let number = issues
                .iter()
                .find(|(_, issue)| issue["node_id"] == node)
                .map(|(number, _)| *number)
                .unwrap();
            issues.remove(&number);
            self.deleted.lock().unwrap().push(number);
            return Ok(reply(
                200,
                json!({ "data": { "deleteIssue": { "clientMutationId": null } } }),
            ));
        }
        let rest = path
            .split("/repos/acme/repo/issues")
            .nth(1)
            .unwrap_or("-")
            .trim_start_matches('/');
        if rest.is_empty() {
            let issues = self.issues.lock().unwrap();
            return Ok(reply(200, json!(issues.values().collect::<Vec<_>>())));
        }
        if rest.ends_with("/comments") || rest.ends_with("/events") || rest.ends_with("/timeline") {
            return Ok(reply(200, json!([])));
        }
        let Ok(number) = rest.parse::<u64>() else {
            return Ok(reply(404, json!({ "message": "Not Found" })));
        };
        if self.deleted.lock().unwrap().contains(&number) {
            return Ok(reply(410, json!({ "message": "This issue was deleted" })));
        }
        if method == "GET" {
            return Ok(match self.issue(number) {
                Some(issue) => reply(200, issue),
                None => reply(404, json!({ "message": "Not Found" })),
            });
        }
        let now = {
            let mut clock = self.clock.lock().unwrap();
            *clock += 1;
            format!("2026-11-24T00:00:{:02}Z", *clock)
        };
        let body = body.unwrap();
        let mut issues = self.issues.lock().unwrap();
        let issue = issues.get_mut(&number).unwrap();
        if let Some(labels) = body.get("labels").and_then(Value::as_array) {
            issue["labels"] = json!(
                labels
                    .iter()
                    .map(|name| json!({ "name": name }))
                    .collect::<Vec<_>>()
            );
        }
        match body.get("state").and_then(Value::as_str) {
            Some("closed") => {
                if issue["state"] != "closed" {
                    issue["closed_at"] = json!(now);
                }
                issue["state"] = json!("closed");
                issue["state_reason"] = body
                    .get("state_reason")
                    .cloned()
                    .unwrap_or(json!("completed"));
            }
            Some("open") => {
                issue["state"] = json!("open");
                issue["state_reason"] = json!("reopened");
                issue["closed_at"] = Value::Null;
            }
            _ => {}
        }
        issue["updated_at"] = json!(now);
        Ok(reply(200, issue.clone()))
    }
}

struct Project {
    _dir: tempfile::TempDir,
    router: axum::Router,
    state: AppState,
    base: String,
    github: Arc<FakeGitHub>,
}

/// A checkout linking one Git store and one GitHub connection.
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
            json!({ "connection_id": "github-main", "provider": "github", "locator": "acme/repo" }),
        );
    std::fs::write(&registry, serde_json::to_vec(&saved).unwrap()).unwrap();
    let github = Arc::new(FakeGitHub::default());
    let mut config = GitHubConfig::new("github-main", "acme/repo", "fixture-token");
    config.api_base = "https://api.test".into();
    let state = AppState::new(FsStore::open(&store_root).unwrap(), SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_checkout_registry(&registry)
        .with_ticket_provider(Arc::new(GitHubProvider::new(config, github.clone())));
    Project {
        _dir: dir,
        router: app(state.clone()),
        state,
        base: format!("/checkouts/{}", registered.id),
        github,
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
async fn gh_5_deleting_a_verified_github_ticket_trashes_it_and_restore_returns_it_verified() {
    let project = project();
    project.github.seed(
        5,
        "closed",
        Some("completed"),
        &["category:bug", "status:verified"],
    );
    let router = &project.router;
    let ticket = format!("{}/tickets/github-main:5", project.base);

    let deleted = ok(
        router,
        "PATCH",
        &ticket,
        Some(json!({ "status": "deleted" })),
    )
    .await;
    assert_eq!(t(&deleted)["status"], "deleted", "{deleted}");
    let issue = project.github.issue(5).unwrap();
    assert_eq!(issue["state"], "closed");
    assert_eq!(issue["state_reason"], "not_planned");
    assert_eq!(
        project.github.labels(5),
        [
            "category:bug",
            "closed:completed",
            "deleted-from:verified",
            "priority:default",
            "status:deleted"
        ]
    );
    assert_eq!(
        t(&ok(router, "GET", &ticket, None).await)["status"],
        "deleted"
    );

    let restored = ok(router, "POST", &format!("{ticket}/restore"), None).await;
    assert_eq!(restored["store"], "github-main");
    assert_eq!(t(&restored)["status"], "verified", "{restored}");
    assert_eq!(t(&restored)["close_reason"], "completed");
    assert_eq!(
        project.github.issue(5).unwrap()["state_reason"],
        "completed"
    );
    assert_eq!(
        project.github.labels(5),
        ["category:bug", "priority:default", "status:verified"]
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

    // The connection route restores the same way.
    ok(
        router,
        "PATCH",
        &ticket,
        Some(json!({ "status": "deleted" })),
    )
    .await;
    let restored = ok(
        router,
        "POST",
        "/providers/github-main/tickets/5/restore",
        None,
    )
    .await;
    assert_eq!(restored["status"], "verified", "{restored}");
}

#[tokio::test]
async fn gh_5_empty_trash_reports_retained_issues_until_the_connection_may_delete_them() {
    let project = project();
    project.github.seed(
        5,
        "closed",
        Some("completed"),
        &["category:bug", "status:verified"],
    );
    project.github.seed(6, "open", None, &["category:bug"]);
    let router = &project.router;
    for number in [5, 6] {
        ok(
            router,
            "PATCH",
            &format!("{}/tickets/github-main:{number}", project.base),
            Some(json!({ "status": "deleted" })),
        )
        .await;
    }
    let empty = format!("{}/trash/empty", project.base);

    let refused = ok(router, "POST", &empty, None).await;
    assert_eq!(refused["purged"], 0, "{refused}");
    let retained = refused["retained"].as_array().unwrap();
    assert_eq!(retained.len(), 2, "{refused}");
    assert_eq!(retained[0]["qualified_id"], "github-main:5");
    assert!(
        retained[0]["message"]
            .as_str()
            .unwrap()
            .contains("cannot permanently delete"),
        "{refused}"
    );
    assert!(project.github.issue(5).is_some() && project.github.issue(6).is_some());

    *project.github.admin.lock().unwrap() = true;
    let emptied = ok(router, "POST", &empty, None).await;
    assert_eq!(emptied["purged"], 2, "{emptied}");
    assert_eq!(emptied["tickets"], json!(["acme/repo#5", "acme/repo#6"]));
    assert_eq!(emptied["retained"], json!([]));
    assert!(project.github.issue(5).is_none() && project.github.issue(6).is_none());
    // Idempotent: nothing left to delete.
    let again = ok(router, "POST", &empty, None).await;
    assert_eq!(again["purged"], 0, "{again}");
}

#[tokio::test]
async fn gh_5_the_retention_sweep_deletes_only_expired_provider_trash() {
    let project = project();
    *project.github.admin.lock().unwrap() = true;
    for number in [5, 6] {
        project
            .github
            .seed(number, "closed", Some("completed"), &["category:bug"]);
        ok(
            &project.router,
            "PATCH",
            &format!("{}/tickets/github-main:{number}", project.base),
            Some(json!({ "status": "deleted" })),
        )
        .await;
    }
    // Trashed long before the default 30-day retention; 6 stays recent.
    project.github.age(5, "2020-01-01T00:00:00Z");
    let swept = tokio::task::spawn_blocking({
        let state = project.state.clone();
        move || hotsheet_server::sync_loop::purge_all_provider_trash(&state)
    })
    .await
    .unwrap();
    assert_eq!(swept, vec![("github-main".to_string(), 1, 0)]);
    assert!(project.github.issue(5).is_none());
    assert!(project.github.issue(6).is_some());
}

/// Checkout responses carry the ticket flattened beside `store`, or nested as `ticket`.
fn t(value: &Value) -> &Value {
    value.get("ticket").unwrap_or(value)
}
