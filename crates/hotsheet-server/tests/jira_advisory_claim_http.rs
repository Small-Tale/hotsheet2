//! HS2-BPKM76: real authenticated routes with synthetic Jira issue properties.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_extsync::{GitHubTransport, HttpResponse, JiraConfig, JiraProvider};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata, advisory_claim_ledger::now_epoch};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tower::ServiceExt;

struct FakeJira {
    offline: AtomicBool,
    timeout_after_property_write: AtomicBool,
    properties: Mutex<HashMap<String, Value>>,
}

impl Default for FakeJira {
    fn default() -> Self {
        Self {
            offline: AtomicBool::new(false),
            timeout_after_property_write: AtomicBool::new(false),
            properties: Mutex::new(HashMap::new()),
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
        if self.offline.load(Ordering::SeqCst) {
            return Err("synthetic offline".into());
        }
        let prefix = "/issue/ENG-9/properties";
        let response = if let Some(suffix) = url.split(prefix).nth(1) {
            if suffix.is_empty() && method == "GET" {
                json!({"keys":self.properties.lock().unwrap().keys().map(|key|json!({"key":key})).collect::<Vec<_>>()})
            } else if let Some(key) = suffix.strip_prefix('/') {
                if method == "PUT" {
                    self.properties
                        .lock()
                        .unwrap()
                        .insert(key.into(), body.unwrap().clone());
                    if self
                        .timeout_after_property_write
                        .swap(false, Ordering::SeqCst)
                    {
                        return Err("synthetic timeout after applied property".into());
                    }
                    return Ok(HttpResponse {
                        status: 204,
                        headers: HashMap::new(),
                        body: String::new(),
                    });
                }
                json!({"key":key,"value":self.properties.lock().unwrap().get(key).cloned().ok_or("missing property")?})
            } else {
                return Err(format!("unexpected path {url}"));
            }
        } else {
            return Err(format!("unexpected request {method} {url}"));
        };
        Ok(HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: response.to_string(),
        })
    }
}

fn request(method: &str, path: &str, body: Option<Value>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("x-hotsheet-secret", "synthetic-secret")
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap()
}

async fn response_json(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn provider(fake: Arc<FakeJira>) -> JiraProvider {
    JiraProvider::new(
        JiraConfig {
            connection_id: "jira-eng".into(),
            project_key: "ENG".into(),
            base_url: "https://jira.test".into(),
            email: "synthetic@example.test".into(),
            token: "fixture".into(),
            default: false,
        },
        fake,
    )
}

#[tokio::test]
async fn hs2_bpkm76_provisional_claim_replays_after_restart_and_reconciles() {
    let dir = tempfile::tempdir().unwrap();
    let checkout_dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let ledger = dir.path().join("advisory.sqlite");
    let registry = dir.path().join("checkouts.json");
    let fake = Arc::new(FakeJira::default());
    let state = AppState::new(store, "synthetic-secret".into())
        .unwrap()
        .with_checkout_registry(&registry)
        .with_ticket_provider(Arc::new(provider(fake.clone())))
        .with_advisory_claim_ledger(&ledger)
        .unwrap();
    let server = app(state);
    let (status, _) = response_json(server.clone().oneshot(request("POST", "/provider-connections", Some(json!({
        "id":"jira-eng","provider":"jira","locator":"ENG","default":false,
        "settings":{"base_url":"https://jira.test","email":"synthetic@example.test","credential":{"secret":"fixture"}}
    })))).await.unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, registration) = response_json(
        server
            .clone()
            .oneshot(request(
                "POST",
                "/checkouts",
                Some(json!({
                    "root":checkout_dir.path(),"alias":"synthetic-jira",
                    "sources":[{"connection_id":"jira-eng","provider":"jira","locator":"ENG"}],
                    "default_source":"jira-eng"
                })),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{registration}");
    let checkout_id = registration["id"].as_str().unwrap();
    let uri = format!("/checkouts/{checkout_id}/providers/jira-eng/tickets/ENG-9/advisory-claim");
    let expiry = (now_epoch() + 3600) * 1000;
    fake.offline.store(true, Ordering::SeqCst);
    let (status, admitted) = response_json(server.clone().oneshot(request("POST", &uri, Some(json!({
        "worker":"worker-a","operation_id":"op-a","kind":{"type":"claim","expires_unix_ms":expiry},"provisional":true
    })))).await.unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{admitted}");
    assert_eq!(admitted["provisional"], true);
    assert_eq!(admitted["remote_verified"], false);
    assert_eq!(admitted["automatic_handoff_safe"], false);
    drop(server);
    fake.offline.store(false, Ordering::SeqCst);
    fake.timeout_after_property_write
        .store(true, Ordering::SeqCst);
    let restarted = app(AppState::new(
        FsStore::open(dir.path()).unwrap(),
        "synthetic-secret".into(),
    )
    .unwrap()
    .with_checkout_registry(&registry)
    .with_ticket_provider(Arc::new(provider(fake.clone())))
    .with_advisory_claim_ledger(&ledger)
    .unwrap());
    let (status, uncertain) = response_json(
        restarted
            .clone()
            .oneshot(request("GET", &uri, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{uncertain}");
    assert_eq!(uncertain["provisional"], true);
    assert_eq!(uncertain["automatic_handoff_safe"], false);
    let (status, reconciled) =
        response_json(restarted.oneshot(request("GET", &uri, None)).await.unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{reconciled}");
    assert_eq!(reconciled["provisional"], false);
    assert_eq!(reconciled["remote_verified"], true);
    assert_eq!(reconciled["projection"]["claims"][0]["worker"], "worker-a");
    assert_eq!(fake.properties.lock().unwrap().len(), 1);
}
