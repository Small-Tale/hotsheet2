//! HS2-73Z1R8 (GH-5 parity): deleting a Jira ticket through a checkout adds the
//! `hotsheet-trash` label and leaves the workflow status alone, so it reads back as deleted
//! and Restore returns the exact prior status; Empty Trash and the retention sweep delete
//! the issue only when the connection has Jira's Delete Issues permission, and otherwise
//! report it as retained. Synthetic issues only.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_extsync::{GitHubTransport, HttpResponse, JiraConfig, JiraProvider};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";
const TRASH: &str = "hotsheet-trash";

/// A stateful Jira Cloud fake with REST v3 response shapes: `PUT issue/{key}` applies
/// `fields` and `update.labels` add/remove operations and answers 204; `POST search/jql`
/// honours `labels = "..."` clauses; `DELETE issue/{key}` answers 204 with the Delete Issues
/// permission and Jira's 403 `errorMessages` otherwise; a deleted issue answers 404.
#[derive(Default)]
struct FakeJira {
    issues: Mutex<BTreeMap<String, Value>>,
    can_delete: Mutex<bool>,
    clock: Mutex<u64>,
}

fn reply(status: u16, body: Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: HashMap::new(),
        body: body.to_string(),
    }
}

fn no_content() -> HttpResponse {
    HttpResponse {
        status: 204,
        headers: HashMap::new(),
        body: String::new(),
    }
}

fn missing() -> HttpResponse {
    reply(
        404,
        json!({"errorMessages": ["Issue does not exist or you do not have permission to see it."], "errors": {}}),
    )
}

impl FakeJira {
    fn seed(&self, key: &str, category: &str, labels: &[&str]) {
        let name = match category {
            "done" => "Done",
            "indeterminate" => "In Progress",
            _ => "To Do",
        };
        self.issues.lock().unwrap().insert(
            key.into(),
            json!({
                "id": "10001", "key": key,
                "fields": {
                    "summary": format!("Synthetic issue {key}"),
                    "description": null,
                    "status": {"name": name, "statusCategory": {"key": category, "name": name}},
                    "priority": {"name": "Medium"}, "issuetype": {"name": "Task"},
                    "labels": labels, "assignee": null,
                    "created": "2026-11-01T00:00:00.000+0000",
                    "updated": "2026-11-01T00:01:00.000+0000",
                    "resolutiondate": (category == "done").then_some("2026-11-01T00:01:00.000+0000"),
                    "attachment": []
                }
            }),
        );
    }

    fn issue(&self, key: &str) -> Option<Value> {
        self.issues.lock().unwrap().get(key).cloned()
    }

    fn labels(&self, key: &str) -> Vec<String> {
        let mut names = self.issue(key).unwrap()["fields"]["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|label| label.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn category(&self, key: &str) -> String {
        self.issue(key).unwrap()["fields"]["status"]["statusCategory"]["key"]
            .as_str()
            .unwrap()
            .into()
    }

    fn age(&self, key: &str, updated: &str) {
        self.issues.lock().unwrap().get_mut(key).unwrap()["fields"]["updated"] = json!(updated);
    }

    fn put(&self, issue: &mut Value, body: &Value) {
        let now = {
            let mut clock = self.clock.lock().unwrap();
            *clock += 1;
            format!("2026-11-24T00:00:{:02}.000+0000", *clock)
        };
        if let Some(fields) = body.get("fields").and_then(Value::as_object) {
            for (name, value) in fields {
                issue["fields"][name] = value.clone();
            }
        }
        for operation in body
            .pointer("/update/labels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let labels = issue["fields"]["labels"].as_array_mut().unwrap();
            if let Some(add) = operation.get("add") {
                if !labels.contains(add) {
                    labels.push(add.clone());
                }
            } else if let Some(remove) = operation.get("remove") {
                labels.retain(|label| label != remove);
            }
        }
        issue["fields"]["updated"] = json!(now);
    }
}

impl GitHubTransport for FakeJira {
    fn request(
        &self,
        method: &str,
        url: &str,
        _: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let rest = url
            .strip_prefix("https://jira.test/rest/api/3/")
            .ok_or_else(|| format!("unexpected Jira request: {method} {url}"))?;
        let path = rest.split('?').next().unwrap_or(rest);
        if path == "search/jql" {
            let jql = body.unwrap()["jql"].as_str().unwrap_or_default().to_owned();
            let required = jql
                .split("labels = \"")
                .skip(1)
                .filter_map(|rest| rest.split('"').next())
                .map(|label| json!(label))
                .collect::<Vec<_>>();
            let issues = self
                .issues
                .lock()
                .unwrap()
                .values()
                .filter(|issue| {
                    let labels = issue["fields"]["labels"].as_array().unwrap();
                    required.iter().all(|label| labels.contains(label))
                })
                .cloned()
                .collect::<Vec<_>>();
            return Ok(reply(200, json!({"issues": issues, "isLast": true})));
        }
        let Some(key) = path.strip_prefix("issue/") else {
            return Err(format!("unexpected Jira request: {method} {url}"));
        };
        if let Some(key) = key.strip_suffix("/comment") {
            return Ok(if self.issue(key).is_some() {
                reply(200, json!({"comments": [], "total": 0}))
            } else {
                missing()
            });
        }
        let mut issues = self.issues.lock().unwrap();
        let Some(issue) = issues.get_mut(key) else {
            return Ok(missing());
        };
        match method {
            "GET" => Ok(reply(200, issue.clone())),
            "PUT" => {
                self.put(issue, body.unwrap());
                Ok(no_content())
            }
            "DELETE" if *self.can_delete.lock().unwrap() => {
                issues.remove(key);
                Ok(no_content())
            }
            "DELETE" => Ok(reply(
                403,
                json!({"errorMessages": ["You do not have permission to delete issues in this project."], "errors": {}}),
            )),
            _ => Err(format!("unexpected Jira request: {method} {url}")),
        }
    }
}

struct Project {
    _dir: tempfile::TempDir,
    router: axum::Router,
    state: AppState,
    base: String,
    jira: Arc<FakeJira>,
}

/// A checkout linking one Git store and one Jira connection.
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
        .push(json!({ "connection_id": "jira-eng", "provider": "jira", "locator": "ENG" }));
    std::fs::write(&registry, serde_json::to_vec(&saved).unwrap()).unwrap();
    let jira = Arc::new(FakeJira::default());
    let config = JiraConfig {
        connection_id: "jira-eng".into(),
        project_key: "ENG".into(),
        base_url: "https://jira.test".into(),
        email: "synthetic@example.com".into(),
        token: "fixture-token".into(),
        default: false,
    };
    let state = AppState::new(FsStore::open(&store_root).unwrap(), SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_checkout_registry(&registry)
        .with_ticket_provider(Arc::new(JiraProvider::new(config, jira.clone())));
    Project {
        _dir: dir,
        router: app(state.clone()),
        state,
        base: format!("/checkouts/{}", registered.id),
        jira,
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
async fn hs2_73z1r8_deleting_a_started_jira_ticket_trashes_it_and_restore_returns_it() {
    let project = project();
    project.jira.seed("ENG-5", "indeterminate", &["customer"]);
    let router = &project.router;
    let ticket = format!("{}/tickets/jira-eng:ENG-5", project.base);

    let deleted = ok(
        router,
        "PATCH",
        &ticket,
        Some(json!({ "status": "deleted" })),
    )
    .await;
    assert_eq!(t(&deleted)["status"], "deleted", "{deleted}");
    assert_eq!(t(&deleted)["tags"], json!(["customer"]), "{deleted}");
    assert_eq!(project.jira.labels("ENG-5"), ["customer", TRASH]);
    assert_eq!(
        project.jira.category("ENG-5"),
        "indeterminate",
        "Delete never transitions the Jira workflow"
    );
    let read = ok(router, "GET", &ticket, None).await;
    assert_eq!(t(&read)["status"], "deleted", "{read}");

    let restored = ok(router, "POST", &format!("{ticket}/restore"), None).await;
    assert_eq!(restored["store"], "jira-eng");
    assert_eq!(t(&restored)["status"], "started", "{restored}");
    assert_eq!(project.jira.labels("ENG-5"), ["customer"]);

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

    // The connection route restores the same way, here to a done (Completed) issue.
    project.jira.seed("ENG-6", "done", &[]);
    ok(
        router,
        "PATCH",
        &format!("{}/tickets/jira-eng:ENG-6", project.base),
        Some(json!({ "status": "deleted" })),
    )
    .await;
    let restored = ok(
        router,
        "POST",
        "/providers/jira-eng/tickets/ENG-6/restore",
        None,
    )
    .await;
    assert_eq!(restored["status"], "completed", "{restored}");
    assert_eq!(restored["close_reason"], "completed");
    assert_eq!(project.jira.category("ENG-6"), "done");
}

#[tokio::test]
async fn hs2_73z1r8_other_status_changes_stay_explicitly_unsupported() {
    let project = project();
    project.jira.seed("ENG-5", "new", &[]);
    let (status, body) = send(
        &project.router,
        "PATCH",
        &format!("{}/tickets/jira-eng:ENG-5", project.base),
        Some(json!({ "status": "completed" })),
    )
    .await;
    assert!(status.is_client_error(), "{status} {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("status transitions"),
        "{body}"
    );
    assert_eq!(project.jira.category("ENG-5"), "new");
    assert_eq!(project.jira.labels("ENG-5"), Vec::<String>::new());
}

#[tokio::test]
async fn hs2_73z1r8_empty_trash_reports_retained_issues_until_the_connection_may_delete_them() {
    let project = project();
    project.jira.seed("ENG-5", "done", &[]);
    project.jira.seed("ENG-6", "new", &[]);
    project.jira.seed("ENG-7", "new", &[]);
    let router = &project.router;
    for key in ["ENG-5", "ENG-6"] {
        ok(
            router,
            "PATCH",
            &format!("{}/tickets/jira-eng:{key}", project.base),
            Some(json!({ "status": "deleted" })),
        )
        .await;
    }
    let empty = format!("{}/trash/empty", project.base);

    let refused = ok(router, "POST", &empty, None).await;
    assert_eq!(refused["purged"], 0, "{refused}");
    let retained = refused["retained"].as_array().unwrap();
    assert_eq!(retained.len(), 2, "{refused}");
    assert_eq!(retained[0]["qualified_id"], "jira-eng:ENG-5");
    assert_eq!(retained[0]["slug"], "ENG-5");
    let message = retained[0]["message"].as_str().unwrap();
    assert!(message.contains("cannot permanently delete"), "{refused}");
    assert!(
        message.contains("You do not have permission to delete issues"),
        "{refused}"
    );
    assert_eq!(refused["unavailable"], json!([]));
    assert!(project.jira.issue("ENG-5").is_some() && project.jira.issue("ENG-6").is_some());

    *project.jira.can_delete.lock().unwrap() = true;
    let emptied = ok(router, "POST", &empty, None).await;
    assert_eq!(emptied["purged"], 2, "{emptied}");
    assert_eq!(emptied["tickets"], json!(["ENG-5", "ENG-6"]));
    assert_eq!(emptied["retained"], json!([]));
    assert!(project.jira.issue("ENG-5").is_none() && project.jira.issue("ENG-6").is_none());
    assert!(
        project.jira.issue("ENG-7").is_some(),
        "a live issue is never purged"
    );
    // Idempotent: nothing left to delete.
    let again = ok(router, "POST", &empty, None).await;
    assert_eq!(again["purged"], 0, "{again}");
}

#[tokio::test]
async fn hs2_73z1r8_the_retention_sweep_deletes_only_expired_jira_trash() {
    let project = project();
    *project.jira.can_delete.lock().unwrap() = true;
    for key in ["ENG-5", "ENG-6"] {
        project.jira.seed(key, "done", &[]);
        ok(
            &project.router,
            "PATCH",
            &format!("{}/tickets/jira-eng:{key}", project.base),
            Some(json!({ "status": "deleted" })),
        )
        .await;
    }
    // Trashed long before the default 30-day retention, in Jira's `+0000` offset form;
    // ENG-6 stays recent.
    project.jira.age("ENG-5", "2020-01-01T00:00:00.000+0000");
    let swept = tokio::task::spawn_blocking({
        let state = project.state.clone();
        move || hotsheet_server::sync_loop::purge_all_provider_trash(&state)
    })
    .await
    .unwrap();
    assert_eq!(swept, vec![("jira-eng".to_string(), 1, 0)]);
    assert!(project.jira.issue("ENG-5").is_none());
    assert!(project.jira.issue("ENG-6").is_some());
}

/// Checkout responses carry the ticket flattened beside `store`, or nested as `ticket`.
fn t(value: &Value) -> &Value {
    value.get("ticket").unwrap_or(value)
}
