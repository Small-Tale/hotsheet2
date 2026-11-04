//! HS2-BAK4BC: an explicit claim of a blocked ticket succeeds over HTTP and through the MCP
//! shim's HTTP backend against a real server socket, keeps the blocked reason, and the MCP
//! result names it in `blocked_notice`. Automatic claim-next still skips the ticket. The
//! serverless MCP backend gives the same answer. Synthetic tickets only.

use hotsheet_mcp::{Backend, CoreBackend, HttpBackend, handle_message};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const SECRET: &str = "test-secret";
const REASON: &str = "Waiting on a synthetic dependency";

fn request(method: &str, url: &str, body: Value) -> (u16, Value) {
    let response = ureq::request(method, url)
        .set("x-hotsheet-secret", SECRET)
        .set("content-type", "application/json")
        .send_string(&body.to_string());
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => panic!("{error}"),
    };
    let status = response.status();
    (
        status,
        serde_json::from_str(&response.into_string().unwrap()).unwrap(),
    )
}

fn tool(backend: &dyn Backend, name: &str, arguments: Value) -> Value {
    let reply = handle_message(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name": name, "arguments": arguments}}),
        backend,
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

/// Claim a blocked ticket through MCP, renew it, and check claim-next skips the other.
fn mcp_claims_blocked(backend: &dyn Backend, slug: &str) {
    let claimed = tool(
        backend,
        "hotsheet_claim",
        json!({"id": slug, "worker": "mcp-agent", "eta": "1h"}),
    );
    assert_eq!(claimed["claimed_by"], "mcp-agent");
    assert_eq!(claimed["status"], "started");
    assert_eq!(claimed["blocked_reason"], REASON);
    let notice = claimed["blocked_notice"].as_str().unwrap();
    assert!(
        notice.starts_with(&format!("Blocked: {REASON}.")),
        "{notice}"
    );
    assert!(notice.contains("hotsheet_update"), "{notice}");

    let renewed = tool(
        backend,
        "hotsheet_renew",
        json!({"id": slug, "worker": "mcp-agent"}),
    );
    assert_eq!(renewed["claimed_by"], "mcp-agent");
    assert_eq!(renewed["eta_expired"], false);
    assert!(renewed["blocked_notice"].is_string(), "{renewed}");

    let next = tool(backend, "hotsheet_claim_next", json!({"worker": "auto"}));
    assert!(next.is_null(), "claim-next skips blocked tickets: {next}");
}

fn create_blocked(base_or_backend: Result<&str, &dyn Backend>, title: &str) -> String {
    match base_or_backend {
        Ok(base) => {
            let (status, created) = request(
                "POST",
                &format!("{base}/tickets"),
                json!({"title": title, "up_next": true}),
            );
            assert_eq!(status, 201, "{created}");
            let id = created["id"].as_str().unwrap();
            let (status, blocked) = request(
                "PATCH",
                &format!("{base}/tickets/{id}"),
                json!({"blocked_reason": REASON}),
            );
            assert_eq!(status, 200, "{blocked}");
            created["slug"].as_str().unwrap().to_owned()
        }
        Err(backend) => {
            let created = tool(
                backend,
                "hotsheet_create",
                json!({"title": title, "up_next": true}),
            );
            let id = created["id"].as_str().unwrap();
            tool(
                backend,
                "hotsheet_update",
                json!({"id": id, "blocked_reason": REASON}),
            );
            created["slug"].as_str().unwrap().to_owned()
        }
    }
}

#[tokio::test]
async fn hs2_bak4bc_http_and_mcp_claims_take_a_blocked_ticket_on_a_real_server() {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(directory.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(directory.path().join("no-codex-home"));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });

    tokio::task::spawn_blocking(move || {
        let http_slug = create_blocked(Ok(&base), "http blocked");
        let mcp_slug = create_blocked(Ok(&base), "mcp blocked");

        let (status, next) = request(
            "POST",
            &format!("{base}/claim-next"),
            json!({"worker": "auto"}),
        );
        assert_eq!(status, 200);
        assert!(
            next.is_null(),
            "HTTP claim-next skips blocked tickets: {next}"
        );

        let (status, claimed) = request(
            "POST",
            &format!("{base}/tickets/{http_slug}/claim"),
            json!({"worker": "person", "lease_minutes": 15}),
        );
        assert_eq!(
            status, 200,
            "explicit HTTP claim of a blocked ticket: {claimed}"
        );
        assert_eq!(claimed["claimed_by"], "person");
        assert_eq!(claimed["status"], "started");
        assert_eq!(claimed["blocked_reason"], REASON);
        let (status, renewed) = request(
            "POST",
            &format!("{base}/tickets/{http_slug}/renew"),
            json!({"worker": "person"}),
        );
        assert_eq!(status, 200, "{renewed}");
        assert_eq!(renewed["blocked_reason"], REASON);
        let (status, contested) = request(
            "POST",
            &format!("{base}/tickets/{http_slug}/claim"),
            json!({"worker": "other"}),
        );
        assert_eq!(
            status, 409,
            "the live claim, not the block, refuses: {contested}"
        );

        mcp_claims_blocked(&HttpBackend::new(&base, SECRET), &mcp_slug);
    })
    .await
    .unwrap();
}

#[test]
fn hs2_bak4bc_serverless_mcp_claim_takes_a_blocked_ticket() {
    let directory = tempfile::tempdir().unwrap();
    FsStore::init_git(directory.path(), &StoreMetadata::new("HS")).unwrap();
    let backend = CoreBackend::open(directory.path()).unwrap();
    let slug = create_blocked(Err(&backend), "serverless blocked");
    mcp_claims_blocked(&backend, &slug);
}
