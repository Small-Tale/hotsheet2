//! HTTP contract for opt-in Jira write-behind admission and provisional reads.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_extsync::{GitHubTransport, HttpResponse, JiraConfig, JiraProvider};
use hotsheet_server::{AppState, app, provider_write_behind::dispatch_jira_at};
use hotsheet_ticketing::{FsStore, StoreMetadata, provider_outbox::ProviderOutbox};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const SECRET: &str = "jira-outbox-test";
const CONNECTION: &str = "jira-eng";
const NATIVE_ID: &str = "ENG-9";

struct FakeJira {
    requests: Mutex<Vec<(String, String)>>,
    title: Mutex<String>,
    revision: Mutex<u32>,
    timeout_after_write: AtomicBool,
    rate_limit_once: AtomicBool,
    reject_once: AtomicBool,
    offline_once: AtomicBool,
    offline: AtomicBool,
}

impl Default for FakeJira {
    fn default() -> Self {
        Self {
            requests: Mutex::default(),
            title: Mutex::new("Remote title".into()),
            revision: Mutex::new(1),
            timeout_after_write: AtomicBool::default(),
            rate_limit_once: AtomicBool::default(),
            reject_once: AtomicBool::default(),
            offline_once: AtomicBool::default(),
            offline: AtomicBool::default(),
        }
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
        self.requests
            .lock()
            .unwrap()
            .push((method.into(), url.into()));
        if self.offline.load(Ordering::SeqCst) || self.offline_once.swap(false, Ordering::SeqCst) {
            return Err("Jira network unavailable".into());
        }
        if method == "PUT" && url.ends_with("/issue/ENG-9") {
            if self.reject_once.swap(false, Ordering::SeqCst) {
                return Ok(HttpResponse {
                    status: 403,
                    headers: HashMap::new(),
                    body: "permission denied".into(),
                });
            }
            if self.rate_limit_once.swap(false, Ordering::SeqCst) {
                return Ok(HttpResponse {
                    status: 429,
                    headers: HashMap::from([("retry-after".into(), "1".into())]),
                    body: "rate limited".into(),
                });
            }
            *self.title.lock().unwrap() =
                body.unwrap()["fields"]["summary"].as_str().unwrap().into();
            *self.revision.lock().unwrap() += 1;
            if self.timeout_after_write.swap(false, Ordering::SeqCst) {
                return Err("timed out after Jira applied the edit".into());
            }
            return Ok(HttpResponse {
                status: 204,
                headers: HashMap::new(),
                body: String::new(),
            });
        }
        let issue = json!({
            "key":NATIVE_ID,
            "fields":{
                "summary":self.title.lock().unwrap().clone(),"description":null,
                "status":{"statusCategory":{"key":"new"}},
                "priority":{"name":"Medium"},"issuetype":{"name":"Task"},
                "labels":["remote"],"assignee":null,
                "created":"2026-08-26T00:00:00Z","updated":format!("2026-08-26T00:{:02}:00Z", *self.revision.lock().unwrap()),
                "resolutiondate":null
            }
        });
        let body = if method == "POST" && url.contains("search/jql") {
            json!({"isLast":true,"issues":[issue]})
        } else if method == "GET" && url.contains("/comment?") {
            json!({"comments":[],"total":0})
        } else if method == "GET" && url.ends_with("/issue/ENG-9") {
            issue
        } else {
            return Err(format!("unexpected Jira request: {method} {url}"));
        };
        Ok(HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: body.to_string(),
        })
    }
}

fn provider(fake: Arc<FakeJira>) -> JiraProvider {
    JiraProvider::new(
        JiraConfig {
            connection_id: CONNECTION.into(),
            project_key: "ENG".into(),
            base_url: "https://jira.test".into(),
            email: "dev@example.com".into(),
            token: "fixture".into(),
            default: false,
        },
        fake,
    )
}

fn request(method: &str, path: &str, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x-hotsheet-secret", SECRET);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

fn edit(id: &str, title: &str) -> Value {
    json!({"operation_id":id,"native_id":NATIVE_ID,"patch":{"title":title}})
}

#[tokio::test]
async fn queued_jira_batch_reads_provisionally_and_survives_server_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let outbox = dir.path().join("jira-outbox.sqlite");
    let checkout = tempfile::tempdir().unwrap();
    let registry = dir.path().join("checkouts.json");
    let fake = Arc::new(FakeJira::default());
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_checkout_registry(&registry)
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_jira_outbox(&outbox, 2)
        .unwrap();
    let server = app(state);
    let uri = format!("/providers/{CONNECTION}/tickets/queued");
    let connection = server
        .clone()
        .oneshot(request(
            "POST",
            "/provider-connections",
            Some(json!({
                "id": CONNECTION, "provider":"jira", "locator":"ENG", "default":false,
                "settings":{"base_url":"https://jira.test","email":"dev@example.com",
                    "credential":{"secret":"fixture"}}
            })),
        ))
        .await
        .unwrap();
    assert_eq!(connection.status(), StatusCode::CREATED);
    let registration = server
        .clone()
        .oneshot(request(
            "POST",
            "/checkouts",
            Some(json!({
                "root": checkout.path(), "alias":"jira-queue",
                "sources":[{"connection_id":CONNECTION,"provider":"jira","locator":"ENG"}],
                "default_source":CONNECTION
            })),
        ))
        .await
        .unwrap();
    let registration_status = registration.status();
    let registration_body = json_body(registration).await;
    assert_eq!(
        registration_status,
        StatusCode::CREATED,
        "{registration_body}"
    );

    let gated = app(
        AppState::new(FsStore::open(dir.path()).unwrap(), SECRET.into())
            .unwrap()
            .with_ticket_provider(Arc::new(provider(fake.clone()))),
    );
    assert_eq!(
        gated
            .oneshot(request(
                "POST",
                &uri,
                Some(json!({"operations":[edit("op-1", "Draft title")]}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );

    let admitted = server
        .clone()
        .oneshot(request(
            "POST",
            &uri,
            Some(json!({
                "operations":[edit("op-1", "Draft title"), edit("op-2", "Final pending title")]
            })),
        ))
        .await
        .unwrap();
    let admission_status = admitted.status();
    let results = json_body(admitted).await;
    assert_eq!(admission_status, StatusCode::ACCEPTED, "{results}");
    assert_eq!(results.as_array().unwrap().len(), 2);
    assert_eq!(results[1]["ticket"]["title"], "Final pending title");
    assert_eq!(results[1]["state"], "queued");

    let retry = server
        .clone()
        .oneshot(request(
            "POST",
            &uri,
            Some(json!({
                "operations":[edit("op-1", "Draft title")]
            })),
        ))
        .await
        .unwrap();
    let retry_status = retry.status();
    let retry_body = json_body(retry).await;
    assert_eq!(retry_status, StatusCode::ACCEPTED, "{retry_body}");
    assert_eq!(retry_body[0]["ticket"]["title"], "Final pending title");
    let changed = server
        .clone()
        .oneshot(request(
            "POST",
            &uri,
            Some(json!({
                "operations":[edit("op-1", "Changed retry")]
            })),
        ))
        .await
        .unwrap();
    assert_eq!(changed.status(), StatusCode::CONFLICT);

    let full_uri = format!("/providers/{CONNECTION}/tickets/{NATIVE_ID}");
    let full = json_body(
        server
            .clone()
            .oneshot(request("GET", &full_uri, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(full["title"], "Final pending title");
    let list = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/tickets"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["title"], "Final pending title");
    let checkout_full = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/checkouts/jira-queue/tickets/{CONNECTION}:{NATIVE_ID}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(checkout_full["title"], "Final pending title");
    let checkout_page = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                "/checkouts/jira-queue/tickets?page_size=10&sort=title",
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(checkout_page["items"][0]["title"], "Final pending title");
    let checkout_search = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                "/checkouts/jira-queue/tickets?page_size=10&text=Final%20pending%20title",
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(checkout_search["items"][0]["title"], "Final pending title");
    assert_eq!(
        fake.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method != "GET" && method != "POST")
            .count(),
        0
    );

    drop(server);
    let restarted = app(
        AppState::new(FsStore::open(dir.path()).unwrap(), SECRET.into())
            .unwrap()
            .with_checkout_registry(&registry)
            .with_ticket_provider(Arc::new(provider(fake)))
            .with_jira_outbox(&outbox, 2)
            .unwrap(),
    );
    let after_restart = json_body(
        restarted
            .oneshot(request("GET", &full_uri, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(after_restart["title"], "Final pending title");
}

#[tokio::test]
async fn queued_jira_batch_rejects_over_capacity_without_partial_overlay() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_ticket_provider(Arc::new(provider(Arc::new(FakeJira::default()))))
        .with_jira_outbox(dir.path().join("outbox.sqlite"), 1)
        .unwrap();
    let server = app(state);
    let uri = format!("/providers/{CONNECTION}/tickets/queued");
    let denied = server
        .clone()
        .oneshot(request(
            "POST",
            &uri,
            Some(json!({
                "operations":[edit("op-1", "First"), edit("op-2", "Second")]
            })),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
    let full = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/tickets/{NATIVE_ID}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(full["title"], "Remote title");
    assert_eq!(
        server
            .oneshot(request(
                "POST",
                &uri,
                Some(json!({"operations":[edit("op-1", "Now pending")]}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
}

#[tokio::test]
async fn dispatch_rebases_remote_edit_and_confirms_in_ticket_order_for_two_clients() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_jira_outbox(dir.path().join("dispatch.sqlite"), 3)
        .unwrap();
    let first_client = app(state.clone());
    let second_client = app(state.clone());
    let queued = first_client
        .clone()
        .oneshot(request(
            "POST",
            &format!("/providers/{CONNECTION}/tickets/queued"),
            Some(
                json!({"operations":[edit("first", "First local"), edit("later", "Latest local")]}),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(queued.status(), StatusCode::ACCEPTED);
    *fake.title.lock().unwrap() = "Concurrent remote title".into();
    *fake.revision.lock().unwrap() = 2;
    let provisional = json_body(
        second_client
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/tickets/{NATIVE_ID}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(provisional["title"], "Latest local");
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 1);
    let statuses = json_body(
        second_client
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/outbox"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(statuses[1]["state"], "confirmed");
    assert_eq!(statuses[1]["conflict"]["title"], "Concurrent remote title");
    assert_eq!(statuses[0]["state"], "queued");
    fake.reject_once.store(true, Ordering::SeqCst);
    assert_eq!(dispatch_jira_at(&state, 101).await.unwrap(), 1);
    let partial = json_body(
        first_client
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/outbox"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(partial[1]["state"], "confirmed");
    assert_eq!(partial[0]["state"], "needs_attention");
    assert_eq!(*fake.title.lock().unwrap(), "First local");
    assert_eq!(
        first_client
            .oneshot(request(
                "POST",
                &format!("/providers/{CONNECTION}/outbox/later"),
                None,
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(dispatch_jira_at(&state, 102).await.unwrap(), 1);
    assert_eq!(*fake.title.lock().unwrap(), "Latest local");
    let authoritative = json_body(
        second_client
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/tickets/{NATIVE_ID}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(authoritative["title"], "Latest local");
    assert!(authoritative.get("pending_operation_ids").is_none());
}

#[tokio::test]
async fn compacted_jira_operation_rejects_a_late_http_retry_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("outbox.sqlite");
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_jira_outbox(&path, 2)
        .unwrap();
    let server = app(state.clone());
    let uri = format!("/providers/{CONNECTION}/tickets/queued");
    let operation = json!({"operations":[edit("settled-http", "Once only")]});
    assert_eq!(
        server
            .oneshot(request("POST", &uri, Some(operation.clone())))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 1);
    drop(state);
    let mut outbox = ProviderOutbox::open(&path, 2).unwrap();
    assert_eq!(outbox.compact_terminal_before(i64::MAX).unwrap(), 1);
    drop(outbox);
    let restarted = app(
        AppState::new(FsStore::open(dir.path()).unwrap(), SECRET.into())
            .unwrap()
            .with_ticket_provider(Arc::new(provider(fake.clone())))
            .with_jira_outbox(&path, 2)
            .unwrap(),
    );
    let writes_before = fake
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(method, _)| method == "PUT")
        .count();
    let retry = restarted
        .oneshot(request("POST", &uri, Some(operation)))
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::CONFLICT);
    assert_eq!(
        fake.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method == "PUT")
            .count(),
        writes_before
    );
}

#[tokio::test]
async fn pruned_v2_jira_operation_rejects_a_late_http_retry_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("outbox.sqlite");
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_jira_outbox(&path, 2)
        .unwrap();
    let server = app(state.clone());
    let uri = format!("/providers/{CONNECTION}/tickets/queued");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let id = format!("v2:{}:aaaaaaaaaaaaaaaa", (now - 20 * 24 * 60 * 60) * 1000);
    let operation = json!({"operations":[edit(&id, "Once only")]});
    assert_eq!(
        server
            .oneshot(request("POST", &uri, Some(operation.clone())))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 1);
    drop(state);
    let mut outbox = ProviderOutbox::open(&path, 2).unwrap();
    assert_eq!(outbox.compact_terminal_before(i64::MAX).unwrap(), 1);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE provider_outbox SET settled_at = ?1 WHERE operation_id = ?2",
        rusqlite::params![now - 31 * 24 * 60 * 60, id],
    )
    .unwrap();
    db.execute(
        "UPDATE provider_outbox_retention SET floor_ms = ?1",
        [(now - 19 * 24 * 60 * 60) * 1000],
    )
    .unwrap();
    drop(db);
    assert_eq!(outbox.prune_terminal_rows().unwrap(), 1);
    drop(outbox);
    let restarted = app(
        AppState::new(FsStore::open(dir.path()).unwrap(), SECRET.into())
            .unwrap()
            .with_ticket_provider(Arc::new(provider(fake.clone())))
            .with_jira_outbox(&path, 2)
            .unwrap(),
    );
    let writes_before = fake
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(method, _)| method == "PUT")
        .count();
    let retry = restarted
        .oneshot(request("POST", &uri, Some(operation)))
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::CONFLICT);
    assert_eq!(
        fake.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method == "PUT")
            .count(),
        writes_before
    );
}

#[tokio::test]
async fn timeout_after_success_and_rate_limit_reconcile_without_duplicate_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_jira_outbox(dir.path().join("dispatch.sqlite"), 3)
        .unwrap();
    let server = app(state.clone());
    let uri = format!("/providers/{CONNECTION}/tickets/queued");
    assert_eq!(
        server
            .clone()
            .oneshot(request(
                "POST",
                &uri,
                Some(json!({
                    "operations":[edit("rate", "Local intent")]
                }))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    fake.rate_limit_once.store(true, Ordering::SeqCst);
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 1);
    let status_uri = format!("/providers/{CONNECTION}/outbox");
    let limited = json_body(
        server
            .clone()
            .oneshot(request("GET", &status_uri, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(limited[0]["state"], "rate_limited");
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 0);
    fake.timeout_after_write.store(true, Ordering::SeqCst);
    assert_eq!(dispatch_jira_at(&state, 101).await.unwrap(), 1);
    let deferred = json_body(
        server
            .clone()
            .oneshot(request("GET", &status_uri, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(deferred[0]["state"], "queued");
    assert_eq!(*fake.title.lock().unwrap(), "Local intent");
    assert_eq!(dispatch_jira_at(&state, 110).await.unwrap(), 1);
    let confirmed = json_body(
        server
            .oneshot(request("GET", &status_uri, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(confirmed[0]["state"], "confirmed");
    assert_eq!(
        fake.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method == "PUT")
            .count(),
        2
    );
}

#[tokio::test]
async fn attention_retry_and_discard_keep_the_remote_and_projection_honest() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_jira_outbox(dir.path().join("dispatch.sqlite"), 3)
        .unwrap();
    let server = app(state.clone());
    let queue_uri = format!("/providers/{CONNECTION}/tickets/queued");
    assert_eq!(
        server
            .clone()
            .oneshot(request(
                "POST",
                &queue_uri,
                Some(json!({"operations":[edit("attention", "Retry me")]}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    fake.reject_once.store(true, Ordering::SeqCst);
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 1);
    let operation_uri = format!("/providers/{CONNECTION}/outbox/attention");
    let attention = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/outbox"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(attention[0]["state"], "needs_attention");
    assert_eq!(*fake.title.lock().unwrap(), "Remote title");
    let retry = json_body(
        server
            .clone()
            .oneshot(request("POST", &operation_uri, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(retry["state"], "queued");
    assert_eq!(dispatch_jira_at(&state, 101).await.unwrap(), 1);
    assert_eq!(*fake.title.lock().unwrap(), "Retry me");
    assert_eq!(
        server
            .clone()
            .oneshot(request(
                "POST",
                &queue_uri,
                Some(json!({"operations":[edit("discard", "Never send")]}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    fake.offline_once.store(true, Ordering::SeqCst);
    let discard = json_body(
        server
            .clone()
            .oneshot(request(
                "DELETE",
                &format!("/providers/{CONNECTION}/outbox/discard"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(discard["state"], "discarded");
    // Discarding a never-sent operation does not require an online Jira read.
    assert!(fake.offline_once.swap(false, Ordering::SeqCst));
    assert_eq!(dispatch_jira_at(&state, 102).await.unwrap(), 0);
    assert_eq!(*fake.title.lock().unwrap(), "Retry me");
    let full = json_body(
        server
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/tickets/{NATIVE_ID}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(full["title"], "Retry me");
    assert!(full.get("pending_operation_ids").is_none());
}

#[tokio::test]
async fn offline_dispatch_keeps_provisional_intent_until_reconnect() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_jira_outbox(dir.path().join("dispatch.sqlite"), 2)
        .unwrap();
    let server = app(state.clone());
    let queue_uri = format!("/providers/{CONNECTION}/tickets/queued");
    assert_eq!(
        server
            .clone()
            .oneshot(request(
                "POST",
                &queue_uri,
                Some(json!({"operations":[edit("offline", "Local offline edit")]}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    fake.offline_once.store(true, Ordering::SeqCst);
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 1);
    let status = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/outbox"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status[0]["state"], "queued");
    let provisional = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/tickets/{NATIVE_ID}"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(provisional["title"], "Local offline edit");
    assert_eq!(dispatch_jira_at(&state, 110).await.unwrap(), 1);
    assert_eq!(*fake.title.lock().unwrap(), "Local offline edit");
}

#[tokio::test]
async fn manual_retry_gets_a_new_bounded_attempt_window_after_attention() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_jira_outbox(dir.path().join("dispatch.sqlite"), 2)
        .unwrap();
    let server = app(state.clone());
    assert_eq!(
        server
            .clone()
            .oneshot(request(
                "POST",
                &format!("/providers/{CONNECTION}/tickets/queued"),
                Some(json!({"operations":[edit("many-failures", "Retry later")]}))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    fake.offline.store(true, Ordering::SeqCst);
    for attempt in 1..=8 {
        assert_eq!(dispatch_jira_at(&state, attempt * 100).await.unwrap(), 1);
    }
    let operation_uri = format!("/providers/{CONNECTION}/outbox/many-failures");
    let status = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/outbox"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status[0]["state"], "needs_attention");
    assert_eq!(status[0]["attempts"], 8);
    assert_eq!(
        server
            .clone()
            .oneshot(request("POST", &operation_uri, None))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(dispatch_jira_at(&state, 900).await.unwrap(), 1);
    let status = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/outbox"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status[0]["state"], "queued");
    assert_eq!(status[0]["attempts"], 9);
    fake.offline.store(false, Ordering::SeqCst);
    assert_eq!(dispatch_jira_at(&state, 1_000).await.unwrap(), 1);
    assert_eq!(*fake.title.lock().unwrap(), "Retry later");
}

// ---- HS2-699TB6: outbox transition-matrix sequences ----

fn put_count(fake: &FakeJira) -> usize {
    fake.requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(method, _)| method == "PUT")
        .count()
}

fn outbox_state(dir: &std::path::Path, fake: &Arc<FakeJira>) -> AppState {
    let store = FsStore::open(dir).unwrap();
    AppState::new(store, SECRET.into())
        .unwrap()
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_jira_outbox(dir.join("dispatch.sqlite"), 4)
        .unwrap()
}

async fn queue(server: &axum::Router, operations: Value) -> (StatusCode, Value) {
    let response = server
        .clone()
        .oneshot(request(
            "POST",
            &format!("/providers/{CONNECTION}/tickets/queued"),
            Some(json!({ "operations": operations })),
        ))
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn outbox(server: &axum::Router) -> Vec<(String, String, i64)> {
    let rows = json_body(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/providers/{CONNECTION}/outbox"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    let mut rows: Vec<_> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["operation_id"].as_str().unwrap().to_owned(),
                row["state"].as_str().unwrap().to_owned(),
                row["attempts"].as_i64().unwrap(),
            )
        })
        .collect();
    rows.sort();
    rows
}

#[tokio::test]
async fn same_operation_id_retried_across_every_dispatch_state_is_one_remote_write() {
    let dir = tempfile::tempdir().unwrap();
    FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = outbox_state(dir.path(), &fake);
    let server = app(state.clone());
    let op = || json!([edit("same", "Retried intent")]);

    // queued -> retry -> still one row
    assert_eq!(queue(&server, op()).await.0, StatusCode::ACCEPTED);
    assert_eq!(queue(&server, op()).await.0, StatusCode::ACCEPTED);
    assert_eq!(outbox(&server).await, [("same".into(), "queued".into(), 0)]);

    // failed dispatch (deferred) -> retry must not reset attempts or add a duplicate
    fake.offline_once.store(true, Ordering::SeqCst);
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 1);
    assert_eq!(queue(&server, op()).await.0, StatusCode::ACCEPTED);
    assert_eq!(outbox(&server).await, [("same".into(), "queued".into(), 1)]);

    // confirmed -> retry is idempotent and dispatch has nothing to send
    assert_eq!(dispatch_jira_at(&state, 1_000).await.unwrap(), 1);
    let (status, body) = queue(&server, op()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body[0]["state"], "confirmed");
    assert_eq!(dispatch_jira_at(&state, 2_000).await.unwrap(), 0);
    assert_eq!(put_count(&fake), 1, "one remote write for one operation id");

    // the same id with a changed payload is rejected, not silently reapplied
    let (changed, _) = queue(&server, json!([edit("same", "Different")])).await;
    assert!(changed.is_client_error(), "{changed}");
    assert_eq!(*fake.title.lock().unwrap(), "Retried intent");
}

#[tokio::test]
async fn failed_dispatch_requeues_with_backoff_and_later_edits_stay_behind_it() {
    let dir = tempfile::tempdir().unwrap();
    FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = outbox_state(dir.path(), &fake);
    let server = app(state.clone());
    assert_eq!(
        queue(&server, json!([edit("a", "First")])).await.0,
        StatusCode::ACCEPTED
    );
    fake.offline_once.store(true, Ordering::SeqCst);
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 1);
    // An edit to the same ticket arrives while "a" is backed off.
    assert_eq!(
        queue(&server, json!([edit("b", "Second")])).await.0,
        StatusCode::ACCEPTED
    );
    // Before the backoff deadline nothing is claimable: "b" must not overtake "a".
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 0);
    assert_eq!(put_count(&fake), 0);
    // After it, "a" sends first, then "b"; the remote ends at the latest intent.
    assert_eq!(dispatch_jira_at(&state, 1_000).await.unwrap(), 1);
    assert_eq!(*fake.title.lock().unwrap(), "First");
    assert_eq!(dispatch_jira_at(&state, 1_001).await.unwrap(), 1);
    assert_eq!(*fake.title.lock().unwrap(), "Second");
    assert_eq!(
        outbox(&server).await,
        [
            ("a".into(), "confirmed".into(), 2),
            ("b".into(), "confirmed".into(), 1)
        ]
    );
}

#[tokio::test]
async fn interleaved_edits_to_one_native_id_project_and_dispatch_in_admission_order() {
    let dir = tempfile::tempdir().unwrap();
    FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let state = outbox_state(dir.path(), &fake);
    let server = app(state.clone());
    for (id, title) in [("x1", "One"), ("x2", "Two")] {
        assert_eq!(
            queue(&server, json!([edit(id, title)])).await.0,
            StatusCode::ACCEPTED
        );
    }
    // A dispatch confirms x1 while x3 is admitted between passes.
    assert_eq!(dispatch_jira_at(&state, 100).await.unwrap(), 1);
    assert_eq!(*fake.title.lock().unwrap(), "One");
    let (_, body) = queue(&server, json!([edit("x3", "Three")])).await;
    assert_eq!(
        body[0]["ticket"]["title"], "Three",
        "projection shows the latest"
    );
    assert_eq!(dispatch_jira_at(&state, 101).await.unwrap(), 1);
    assert_eq!(*fake.title.lock().unwrap(), "Two");
    assert_eq!(dispatch_jira_at(&state, 102).await.unwrap(), 1);
    assert_eq!(dispatch_jira_at(&state, 103).await.unwrap(), 0);
    assert_eq!(*fake.title.lock().unwrap(), "Three");
    assert_eq!(
        outbox(&server).await,
        [
            ("x1".into(), "confirmed".into(), 1),
            ("x2".into(), "confirmed".into(), 1),
            ("x3".into(), "confirmed".into(), 1)
        ]
    );
    assert_eq!(put_count(&fake), 3);
}

#[tokio::test]
async fn restart_with_pending_and_mid_send_entries_resumes_in_order_without_loss() {
    let dir = tempfile::tempdir().unwrap();
    FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    {
        let state = outbox_state(dir.path(), &fake);
        let server = app(state.clone());
        assert_eq!(
            queue(
                &server,
                json!([edit("p1", "Pending one"), edit("p2", "Pending two")])
            )
            .await
            .0,
            StatusCode::ACCEPTED
        );
    }
    // Simulate a crash mid-send: claim p1 (dispatch_state = sending) with no server alive,
    // so neither a provider call nor a settlement follows.
    let claimed = ProviderOutbox::open(dir.path().join("dispatch.sqlite"), 4)
        .unwrap()
        .claim_ready(100, 4)
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].operation_id, "p1");

    let state = outbox_state(dir.path(), &fake);
    let server = app(state.clone());
    assert_eq!(
        outbox(&server).await,
        [
            ("p1".into(), "queued".into(), 1),
            ("p2".into(), "queued".into(), 0)
        ],
        "restart recovers sending -> queued and keeps later intent"
    );
    assert_eq!(dispatch_jira_at(&state, 200).await.unwrap(), 1);
    assert_eq!(*fake.title.lock().unwrap(), "Pending one");
    assert_eq!(dispatch_jira_at(&state, 201).await.unwrap(), 1);
    assert_eq!(dispatch_jira_at(&state, 202).await.unwrap(), 0);
    assert_eq!(*fake.title.lock().unwrap(), "Pending two");
    assert_eq!(put_count(&fake), 2);
}
