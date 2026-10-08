//! HTTP contract for opt-in Jira write-behind admission and provisional reads.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_extsync::{GitHubTransport, HttpResponse, JiraConfig, JiraProvider};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const SECRET: &str = "jira-outbox-test";
const CONNECTION: &str = "jira-eng";
const NATIVE_ID: &str = "ENG-9";

#[derive(Default)]
struct FakeJira {
    requests: Mutex<Vec<(String, String)>>,
}

impl GitHubTransport for FakeJira {
    fn request(
        &self,
        method: &str,
        url: &str,
        _: &[(&str, String)],
        _: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        self.requests
            .lock()
            .unwrap()
            .push((method.into(), url.into()));
        let issue = json!({
            "key":NATIVE_ID,
            "fields":{
                "summary":"Remote title","description":null,
                "status":{"statusCategory":{"key":"new"}},
                "priority":{"name":"Medium"},"issuetype":{"name":"Task"},
                "labels":["remote"],"assignee":null,
                "created":"2026-08-26T00:00:00Z","updated":"2026-08-26T00:01:00Z",
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
