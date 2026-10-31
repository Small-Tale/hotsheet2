//! HS2-HBA9Z8: trusted hook reporting and terminal-scoped MCP presence are independent.
//! Synthetic identifiers/actions only; no installed AI tool or user server is exercised.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "synthetic-attribution-secret";
const TARGET: &str = "codex-synthetic-target";
const OTHER: &str = "codex-synthetic-other";

async fn send(router: &Router, method: &str, path: &str, body: Option<Value>) -> Response {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("x-hotsheet-secret", SECRET)
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .unwrap();
    router.clone().oneshot(request).await.unwrap()
}

async fn value(response: Response) -> Value {
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

async fn fixture() -> (tempfile::TempDir, Router) {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init(directory.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(directory.path().join("no-codex-home"))
        .with_machine_home(directory.path().join("machine-home"));
    let router = app(state);
    for id in [TARGET, OTHER] {
        assert_eq!(
            send(
                &router,
                "POST",
                "/terminals",
                Some(json!({"id": id, "command": "cat", "connect": "codex"})),
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    (directory, router)
}

async fn terminals(router: &Router) -> Value {
    value(send(router, "GET", "/terminals", None).await).await
}

fn terminal<'a>(snapshot: &'a Value, id: &str) -> &'a Value {
    let rows = snapshot.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    rows.iter().find(|row| row["id"] == id).unwrap()
}

async fn cursor(router: &Router) -> u64 {
    value(send(router, "GET", "/ws/poll?timeout_ms=0", None).await).await["cursor"]
        .as_u64()
        .unwrap()
}

async fn events(router: &Router, since: u64) -> Vec<Value> {
    value(
        send(
            router,
            "GET",
            &format!("/ws/poll?since={since}&timeout_ms=0"),
            None,
        )
        .await,
    )
    .await["events"]
        .as_array()
        .unwrap()
        .clone()
}

async fn pending_id(router: &Router) -> u64 {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let pending = value(send(router, "GET", "/permissions", None).await).await;
            if let Some(request) = pending.as_array().unwrap().first() {
                return request["id"].as_u64().unwrap();
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("synthetic permission was not parked")
}

async fn resolve(router: &Router, id: u64) {
    assert_eq!(
        send(
            router,
            "POST",
            &format!("/permissions/{id}"),
            Some(json!({"decision": "deny", "scope": "once"})),
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn hs2_hba9z8_pending_hook_and_absent_wrong_correct_mcp_attribution_are_independent() {
    let (_directory, router) = fixture().await;
    let before = cursor(&router).await;
    let asking = router.clone();
    let ask = tokio::spawn(async move {
        send(
            &asking,
            "POST",
            "/permissions/ask",
            Some(json!({"connection": "synthetic-codex", "agent": "codex", "tool": "Bash",
                "action": "echo synthetic-attribution", "terminal_id": TARGET, "session_id": "synthetic-session"})),
        )
        .await
    });
    let id = pending_id(&router).await;
    let snapshot = terminals(&router).await;
    assert_eq!(
        terminal(&snapshot, TARGET)["ai_connection"]["agent"],
        "codex"
    );
    assert_eq!(
        terminal(&snapshot, TARGET)["last_hook_report"]["source"],
        "permission_request"
    );
    assert_eq!(terminal(&snapshot, TARGET)["mcp_connected"], false);
    assert!(terminal(&snapshot, OTHER).get("ai_connection").is_none());
    let emitted = events(&router, before).await;
    let kinds: Vec<_> = emitted
        .iter()
        .filter_map(|event| match event["kind"].as_str() {
            Some("terminal_hook_report" | "terminal_ai_connection" | "permission_asked") => {
                Some(event["kind"].as_str().unwrap())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "terminal_hook_report",
            "terminal_ai_connection",
            "permission_asked"
        ]
    );
    for event in emitted.iter().filter(|event| {
        event["kind"] == "terminal_hook_report" || event["kind"] == "terminal_ai_connection"
    }) {
        assert_eq!(event["id"], TARGET);
    }

    // The same live MCP process can report no terminal, the other terminal, then the target.
    // Project connectivity never guesses a terminal from cwd, agent name or recency.
    for (attribution, announced) in [
        (None, vec![":connected".to_owned()]),
        (Some(OTHER), vec![format!("{OTHER}:connected")]),
        (
            Some(TARGET),
            vec![
                format!("{OTHER}:disconnected"),
                format!("{TARGET}:connected"),
            ],
        ),
        (Some(TARGET), Vec::new()),
    ] {
        let before = cursor(&router).await;
        let mut lease = json!({"lease_id": "synthetic-mcp", "pid": std::process::id(), "tool": "codex", "cwd": "/synthetic/shared"});
        if let Some(id) = attribution {
            lease["terminal_id"] = json!(id);
        }
        assert_eq!(
            send(&router, "POST", "/mcp-presence", Some(lease))
                .await
                .status(),
            StatusCode::OK
        );
        let announced_actual: Vec<_> = events(&router, before)
            .await
            .iter()
            .filter(|event| event["kind"] == "mcp_presence")
            .map(|event| {
                format!(
                    "{}:{}",
                    event["id"].as_str().unwrap(),
                    event["message"].as_str().unwrap()
                )
            })
            .collect();
        assert_eq!(announced_actual, announced);
        let presence = value(send(&router, "GET", "/mcp-presence", None).await).await;
        assert_eq!(presence["connected"], true);
        let snapshot = terminals(&router).await;
        assert_eq!(
            terminal(&snapshot, TARGET)["mcp_connected"],
            attribution == Some(TARGET)
        );
        assert_eq!(
            terminal(&snapshot, OTHER)["mcp_connected"],
            attribution == Some(OTHER)
        );
        assert_eq!(
            terminal(&snapshot, TARGET)["ai_connection"]["source"],
            "permission_request"
        );
        assert!(terminal(&snapshot, OTHER).get("ai_connection").is_none());
        assert_eq!(
            value(send(&router, "GET", "/permissions", None).await)
                .await
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    assert_eq!(
        send(&router, "DELETE", "/mcp-presence/synthetic-mcp", None)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        terminal(&terminals(&router).await, TARGET)["mcp_connected"],
        false
    );
    resolve(&router, id).await;
    assert_eq!(value(ask.await.unwrap()).await["decision"], "deny");
    assert_eq!(
        send(
            &router,
            "DELETE",
            &format!("/terminals/{TARGET}/ai-connection"),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let snapshot = terminals(&router).await;
    assert!(terminal(&snapshot, TARGET).get("ai_connection").is_none());
    assert_eq!(
        terminal(&snapshot, TARGET)["last_hook_report"]["source"],
        "permission_request"
    );
}

#[tokio::test]
async fn hs2_hba9z8_absent_or_stale_permission_attribution_does_not_guess_a_terminal() {
    let (_directory, router) = fixture().await;
    for attribution in [None, Some("synthetic-expired-terminal")] {
        let before = cursor(&router).await;
        let asking = router.clone();
        let ask = tokio::spawn(async move {
            let mut body = json!({"connection": "synthetic-codex", "agent": "codex", "tool": "Bash", "action": "echo synthetic-unattributed"});
            if let Some(id) = attribution {
                body["terminal_id"] = json!(id);
            }
            send(&asking, "POST", "/permissions/ask", Some(body)).await
        });
        let id = pending_id(&router).await;
        let snapshot = terminals(&router).await;
        for id in [TARGET, OTHER] {
            assert!(terminal(&snapshot, id).get("ai_connection").is_none());
            assert!(terminal(&snapshot, id).get("last_hook_report").is_none());
        }
        let emitted = events(&router, before).await;
        assert!(
            emitted
                .iter()
                .any(|event| event["kind"] == "permission_asked")
        );
        assert!(
            !emitted
                .iter()
                .any(|event| event["kind"] == "terminal_hook_report"
                    || event["kind"] == "terminal_ai_connection")
        );
        resolve(&router, id).await;
        assert_eq!(value(ask.await.unwrap()).await["decision"], "deny");
    }
}
