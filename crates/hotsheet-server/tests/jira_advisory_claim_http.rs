//! HS2-BPKM76: real authenticated routes with synthetic Jira issue properties.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_extsync::{GitHubTransport, HttpResponse, JiraConfig, JiraProvider};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{
    FsStore, StoreMetadata, TicketProvider, TicketRef,
    advisory_claim::{AdvisoryClaimEvent, AdvisoryClaimEventKind},
    advisory_claim_ledger::now_epoch,
    advisory_claim_v2::RECLAIM_AFTER_ISSUE_MS,
};
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
    clock_override_ms: Mutex<Option<i64>>,
    properties: Mutex<HashMap<String, Value>>,
}

impl Default for FakeJira {
    fn default() -> Self {
        Self {
            offline: AtomicBool::new(false),
            timeout_after_property_write: AtomicBool::new(false),
            clock_override_ms: Mutex::new(None),
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
                if method == "DELETE" {
                    let removed = self.properties.lock().unwrap().remove(key);
                    return Ok(HttpResponse {
                        status: if removed.is_some() { 204 } else { 404 },
                        headers: HashMap::new(),
                        body: String::new(),
                    });
                }
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
        let clock = self
            .clock_override_ms
            .lock()
            .unwrap()
            .unwrap_or_else(|| time::OffsetDateTime::now_utc().unix_timestamp() * 1000);
        Ok(HttpResponse {
            status: 200,
            headers: HashMap::from([(
                "date".into(),
                time::OffsetDateTime::from_unix_timestamp_nanos(clock as i128 * 1_000_000)
                    .unwrap()
                    .format(&time::format_description::well_known::Rfc2822)
                    .unwrap(),
            )]),
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
    let (_, before) = response_json(
        server
            .clone()
            .oneshot(request("GET", "/ws/poll?timeout_ms=0", None))
            .await
            .unwrap(),
    )
    .await;
    let cursor = before["cursor"].as_u64().unwrap();
    let expiry = (now_epoch() + 3600) * 1000;
    let operation_id = format!("v2:{:013}:0123456789abcdef", now_epoch() * 1000);
    fake.offline.store(true, Ordering::SeqCst);
    let (status, admitted) = response_json(server.clone().oneshot(request("POST", &uri, Some(json!({
        "worker":"worker-a","operation_id":operation_id,"kind":{"type":"claim","expires_unix_ms":expiry},"provisional":true
    })))).await.unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{admitted}");
    assert_eq!(admitted["provisional"], true);
    assert_eq!(admitted["remote_verified"], false);
    assert_eq!(admitted["automatic_handoff_safe"], false);
    assert_eq!(
        admitted["pending_operations"][0]["operation_id"],
        operation_id
    );
    assert_eq!(admitted["pending_operations"][0]["state"], "queued");
    let (_, changed) = response_json(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/ws/poll?since={cursor}&timeout_ms=0"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(changed["events"][0]["kind"], "advisory_claim_changed");
    assert_eq!(changed["events"][0]["store"], checkout_id);
    assert_eq!(changed["events"][0]["id"], "jira-eng");
    assert_eq!(changed["events"][0]["slug"], "ENG-9");
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
    let (_, before_replay) = response_json(
        restarted
            .clone()
            .oneshot(request("GET", "/ws/poll?timeout_ms=0", None))
            .await
            .unwrap(),
    )
    .await;
    let replay_cursor = before_replay["cursor"].as_u64().unwrap();
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
    assert_eq!(
        uncertain["pending_operations"][0]["state"],
        "needs_attention"
    );
    let (status, reconciled) = response_json(
        restarted
            .clone()
            .oneshot(request("GET", &uri, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reconciled}");
    assert_eq!(reconciled["provisional"], false);
    assert_eq!(reconciled["remote_verified"], true);
    assert_eq!(reconciled["projection"]["claims"][0]["worker"], "worker-a");
    assert_eq!(reconciled["pending_operations"], json!([]));
    assert_eq!(fake.properties.lock().unwrap().len(), 1);
    let (_, replay_events) = response_json(
        restarted
            .oneshot(request(
                "GET",
                &format!("/ws/poll?since={replay_cursor}&timeout_ms=0"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(replay_events["events"].as_array().unwrap().len(), 2);
    assert!(
        replay_events["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| {
                event["kind"] == "advisory_claim_changed" && event["store"] == checkout_id
            })
    );
}

#[tokio::test]
async fn hs2_htyzag_http_claim_prunes_fenced_v2_properties_before_remote_write() {
    let dir = tempfile::tempdir().unwrap();
    let checkout_dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let fake = Arc::new(FakeJira::default());
    let old_now = now_epoch() * 1000 - RECLAIM_AFTER_ISSUE_MS - 5_000;
    *fake.clock_override_ms.lock().unwrap() = Some(old_now);
    let old_id = format!("v2:{old_now:013}:0123456789abcdef");
    let old_claim = AdvisoryClaimEvent {
        ticket: TicketRef {
            connection_id: "jira-eng".into(),
            native_id: "ENG-9".into(),
        },
        worker: "old-worker".into(),
        operation_id: old_id.clone(),
        provider_order: 0,
        kind: AdvisoryClaimEventKind::Claim {
            expires_unix_ms: old_now + 60_000,
        },
    };
    let old_release = AdvisoryClaimEvent {
        kind: AdvisoryClaimEventKind::Release,
        ..old_claim.clone()
    };
    let jira = provider(fake.clone());
    jira.append_advisory_claim_event("ENG-9", &old_claim, old_now, 0)
        .unwrap();
    jira.append_advisory_claim_event("ENG-9", &old_release, old_now, 0)
        .unwrap();
    assert_eq!(fake.properties.lock().unwrap().len(), 2);
    *fake.clock_override_ms.lock().unwrap() = None;

    let server = app(AppState::new(store, "synthetic-secret".into())
        .unwrap()
        .with_checkout_registry(dir.path().join("checkouts.json"))
        .with_ticket_provider(Arc::new(jira))
        .with_advisory_claim_ledger(dir.path().join("advisory.sqlite"))
        .unwrap());
    let (status, _) = response_json(
        server
            .clone()
            .oneshot(request(
                "POST",
                "/provider-connections",
                Some(json!({
                    "id":"jira-eng","provider":"jira","locator":"ENG","default":false,
                    "settings":{"base_url":"https://jira.test","email":"synthetic@example.test","credential":{"secret":"fixture"}}
                })),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, registration) = response_json(
        server
            .clone()
            .oneshot(request(
                "POST",
                "/checkouts",
                Some(json!({
                    "root":checkout_dir.path(),"alias":"cleanup-jira",
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
    let new_now = now_epoch() * 1000;
    let new_id = format!("v2:{new_now:013}:fedcba9876543210");
    let (_, before) = response_json(
        server
            .clone()
            .oneshot(request("GET", "/ws/poll?timeout_ms=0", None))
            .await
            .unwrap(),
    )
    .await;
    let cursor = before["cursor"].as_u64().unwrap();
    let (status, result) = response_json(
        server
            .clone()
            .oneshot(request(
                "POST",
                &uri,
                Some(json!({
                    "worker":"new-worker","operation_id":new_id,
                    "kind":{"type":"claim","expires_unix_ms":new_now+60_000},
                    "provisional":false
                })),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["projection"]["claims"][0]["worker"], "new-worker");
    assert_eq!(fake.properties.lock().unwrap().len(), 1);
    let (_, claim_events) = response_json(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/ws/poll?since={cursor}&timeout_ms=0"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(claim_events["events"][0]["kind"], "advisory_claim_changed");
    assert_eq!(claim_events["events"][0]["store"], checkout_id);
    let after_claim = claim_events["cursor"].as_u64().unwrap();
    let other_id = format!("v2:{new_now:013}:0123456789abcdef");
    let (status, _) = response_json(
        server
            .clone()
            .oneshot(request(
                "POST",
                &uri,
                Some(json!({
                    "worker":"other-worker","operation_id":other_id,
                    "kind":{"type":"claim","expires_unix_ms":new_now+60_000}
                })),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, after_conflict) = response_json(
        server
            .clone()
            .oneshot(request(
                "GET",
                &format!("/ws/poll?since={after_claim}&timeout_ms=0"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(after_conflict["events"], json!([]));
    let (status, released) = response_json(
        server
            .clone()
            .oneshot(request(
                "POST",
                &uri,
                Some(json!({
                    "worker":"new-worker","operation_id":new_id,"kind":{"type":"release"}
                })),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{released}");
    assert_eq!(released["projection"]["claims"], json!([]));
    let (_, release_events) = response_json(
        server
            .oneshot(request(
                "GET",
                &format!("/ws/poll?since={after_claim}&timeout_ms=0"),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        release_events["events"][0]["kind"],
        "advisory_claim_changed"
    );
    assert_eq!(release_events["events"][0]["store"], checkout_id);
}
