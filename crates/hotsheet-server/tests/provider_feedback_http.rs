//! HS2-H9Z9KZ: human notes on an external provider ticket route to the project's AI session
//! best-effort, through the real checkout write route, without persisting a delivery record.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_extsync::{GitHubConfig, GitHubProvider, GitHubTransport, HttpResponse};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";

/// A synthetic GitHub issue that is Started, plus a comment list that grows with each POST.
#[derive(Default)]
struct StartedIssue {
    comments: Mutex<Vec<Value>>,
    writes: Mutex<usize>,
}

impl GitHubTransport for StartedIssue {
    fn request(
        &self,
        method: &str,
        url: &str,
        _: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let path = url.split('?').next().unwrap_or(url);
        let ok = |status: u16, body: Value| HttpResponse {
            status,
            headers: HashMap::new(),
            body: body.to_string(),
        };
        if path.ends_with("/repos/acme/repo/issues/42/comments") {
            if method == "POST" {
                *self.writes.lock().unwrap() += 1;
                let mut comments = self.comments.lock().unwrap();
                let comment = json!({
                    "id": comments.len() + 1,
                    "body": body.and_then(|body| body["body"].as_str()).unwrap_or_default(),
                    "created_at": "2026-11-03T00:02:00Z",
                    "updated_at": "2026-11-03T00:02:00Z",
                });
                comments.push(comment.clone());
                return Ok(ok(201, comment));
            }
            return Ok(ok(200, Value::Array(self.comments.lock().unwrap().clone())));
        }
        if path.ends_with("/repos/acme/repo/issues/42") {
            return Ok(ok(
                200,
                json!({
                    "number": 42, "title": "Synthetic started issue", "body": "body",
                    "state": "open", "state_reason": null,
                    "html_url": "https://github.test/acme/repo/issues/42",
                    "created_at": "2026-11-03T00:00:00Z", "updated_at": "2026-11-03T00:01:00Z",
                    "closed_at": null, "labels": [{"name": "status:started"}], "assignees": [],
                    "pull_request": null
                }),
            ));
        }
        if path.ends_with("/events") || path.ends_with("/timeline") {
            return Ok(ok(200, json!([])));
        }
        Ok(ok(404, json!({"message": "Not Found"})))
    }
}

fn authed(method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET)
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn hs2_h9z9kz_provider_note_routes_best_effort_without_a_persisted_record() {
    let dir = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    let registry = dir.path().join("checkouts.json");
    let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
    let registered = CheckoutRegistry::new(&registry)
        .register_sources(checkout.path(), Some("remote"), None, vec![], None)
        .unwrap();
    // Link the GitHub source the way the provider-aware workflow records it.
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    let entry = saved["checkouts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == registered.id)
        .unwrap();
    entry["sources"] =
        json!([{"connection_id": "github-main", "provider": "github", "locator": "acme/repo"}]);
    entry["default_source"] = json!("github-main");
    std::fs::write(&registry, serde_json::to_vec(&saved).unwrap()).unwrap();
    let transport = Arc::new(StartedIssue::default());
    let provider = GitHubProvider::new(
        GitHubConfig::new("github-main", "acme/repo", "fixture-token"),
        transport.clone(),
    );
    let router = app(AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_checkout_registry(&registry)
        .with_ticket_provider(Arc::new(provider)));
    let uri = format!("/checkouts/{}/tickets/github-main:42", registered.id);

    // A human note with no reachable session: the note is saved and the client is offered Start.
    let (status, human) = json_body(
        router
            .clone()
            .oneshot(authed(
                "PATCH",
                &uri,
                Some(json!({"note": "Synthetic human answer", "actor": {"role": "human"}})),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{human}");
    assert_eq!(human["feedback_delivery"]["status"], "start", "{human}");
    let note_id = human["feedback_delivery"]["note_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Best-effort only: the human note is the one provider write; no delivery record is added.
    assert_eq!(*transport.writes.lock().unwrap(), 1);

    // An explicit choice of a session that is not reachable in this project is refused.
    let (status, _) = json_body(
        router
            .clone()
            .oneshot(authed(
                "POST",
                &format!("{uri}/notes/{note_id}/notify"),
                Some(json!({"connection_id": "foreign-session"})),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // An AI note never notifies, even though GitHub reads comments back without an actor.
    let (status, ai) = json_body(
        router
            .oneshot(authed(
                "PATCH",
                &uri,
                Some(json!({"note": "Synthetic AI progress", "actor": {"role": "ai", "id": "codex-test"}})),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ai}");
    assert!(ai.get("feedback_delivery").is_none(), "{ai}");
    assert_eq!(*transport.writes.lock().unwrap(), 2);
}
