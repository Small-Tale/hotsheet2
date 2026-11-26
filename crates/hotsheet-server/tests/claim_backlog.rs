//! HS2-K6KG1T: over HTTP and through the MCP shim's HTTP backend against a real server
//! socket, a claim starts a Backlog ticket, a phase update keeps the claim, a phase outside
//! Started is refused with 400 and writes nothing, and an update that ends a claim names it
//! in `warnings`. Automatic claim-next skips Backlog (HS2-K6Q8TW). Synthetic tickets only.

use hotsheet_mcp::{Backend, HttpBackend, handle_message};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const SECRET: &str = "test-secret";

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

fn tool(backend: &dyn Backend, name: &str, arguments: Value) -> (bool, Value) {
    let reply = handle_message(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name": name, "arguments": arguments}}),
        backend,
    )
    .unwrap();
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let failed = reply["result"]["isError"] == Value::Bool(true);
    (
        failed,
        serde_json::from_str(text).unwrap_or_else(|_| json!(text)),
    )
}

fn create_backlog(base: &str, title: &str) -> String {
    let (status, created) = request(
        "POST",
        &format!("{base}/tickets"),
        json!({"title": title, "status": "backlog"}),
    );
    assert_eq!(status, 201, "{created}");
    assert_eq!(created["status"], "backlog");
    created["slug"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn hs2_k6kg1t_http_and_mcp_claims_start_backlog_and_keep_the_claim() {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(directory.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(directory.path().join("no-codex-home"));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });

    tokio::task::spawn_blocking(move || {
        // HS2-K6Q8TW: automatic claim-next skips parked Backlog work over HTTP and MCP.
        let parked = create_backlog(&base, "parked");
        let (status, next) = request(
            "POST",
            &format!("{base}/claim-next"),
            json!({"worker": "auto"}),
        );
        assert_eq!(status, 200);
        assert!(next.is_null(), "HTTP claim-next skips Backlog: {next}");
        let (failed, next) = tool(
            &HttpBackend::new(&base, SECRET),
            "hotsheet_claim_next",
            json!({"worker": "auto"}),
        );
        assert!(
            !failed && next.is_null(),
            "MCP claim-next skips Backlog: {next}"
        );
        let (_, still) = request("GET", &format!("{base}/tickets/{parked}"), json!({}));
        assert_eq!(still["status"], "backlog");
        assert!(still["claimed_by"].is_null());

        // HTTP: refused phase, claim, phase, shelve.
        let slug = create_backlog(&base, "http backlog");
        let (status, refused) = request(
            "PATCH",
            &format!("{base}/tickets/{slug}"),
            json!({"started_phase": "working"}),
        );
        assert_eq!(status, 400, "{refused}");
        let (_, unchanged) = request("GET", &format!("{base}/tickets/{slug}"), json!({}));
        assert_eq!(unchanged["status"], "backlog");

        let (status, claimed) = request(
            "POST",
            &format!("{base}/tickets/{slug}/claim"),
            json!({"worker": "person", "lease_minutes": 15}),
        );
        assert_eq!(status, 200, "{claimed}");
        assert_eq!(claimed["status"], "started");
        assert_eq!(claimed["started_phase"], "analyzing");

        let (status, working) = request(
            "PATCH",
            &format!("{base}/tickets/{slug}"),
            json!({"started_phase": "working"}),
        );
        assert_eq!(status, 200, "{working}");
        assert_eq!(working["claimed_by"], "person");
        assert_eq!(working["started_phase"], "working");
        assert!(working.get("warnings").is_none(), "{working}");

        let (status, shelved) = request(
            "PATCH",
            &format!("{base}/tickets/{slug}"),
            json!({"status": "backlog"}),
        );
        assert_eq!(status, 200, "{shelved}");
        assert!(shelved["claimed_by"].is_null());
        assert!(
            shelved["warnings"][0]
                .as_str()
                .unwrap()
                .starts_with(&format!("Released person's claim on {slug}")),
            "{shelved}"
        );

        // MCP through the HTTP backend: the same path.
        let backend = HttpBackend::new(&base, SECRET);
        let mcp_slug = create_backlog(&base, "mcp backlog");
        let (failed, refused) = tool(
            &backend,
            "hotsheet_update",
            json!({"id": mcp_slug, "started_phase": "working"}),
        );
        assert!(failed, "{refused}");
        let (failed, claimed) = tool(
            &backend,
            "hotsheet_claim",
            json!({"id": mcp_slug, "worker": "mcp-agent"}),
        );
        assert!(!failed, "{claimed}");
        assert_eq!(claimed["status"], "started");
        let (failed, working) = tool(
            &backend,
            "hotsheet_update",
            json!({"id": mcp_slug, "started_phase": "working"}),
        );
        assert!(!failed, "{working}");
        assert_eq!(working["claimed_by"], "mcp-agent");
        assert_eq!(working["started_phase"], "working");
    })
    .await
    .unwrap();
}

/// HS2-0R7XCR: the provider-connection ticket route answers a refused phase with 400, as
/// the store and checkout routes do, and names a claim its edit released in `warnings`.
#[tokio::test]
async fn hs2_0r7xcr_provider_connection_patch_refuses_a_phase_and_names_a_release() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let store_root = directory.path().join("AA.hs2");
    let store = FsStore::init_git(&store_root, &StoreMetadata::new("AA")).unwrap();
    let registry = directory.path().join("checkouts.json");
    let registered = hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry)
        .register(&root, Some("project"), None, vec![store_root.clone()])
        .unwrap();
    let connection = registered.sources[0].connection_id.clone();
    let now = hotsheet_model::Timestamp::new("2026-11-26T00:00:00Z");
    let parked = hotsheet_ticketing::ops::create(
        &store,
        hotsheet_model::Ulid::new(),
        "AA",
        now.clone(),
        hotsheet_ticketing::NewTicket {
            title: "parked".into(),
            status: hotsheet_model::Status::Backlog,
            ..Default::default()
        },
    )
    .unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(directory.path().join("no-codex-home"))
        .with_checkout_registry(&registry);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });

    tokio::task::spawn_blocking(move || {
        let uri = format!("{base}/providers/{connection}/tickets/{}", parked.id);
        let (status, refused) = request("PATCH", &uri, json!({"started_phase": "working"}));
        assert_eq!(status, 400, "{refused}");
        assert!(
            refused["error"].as_str().unwrap().contains("not Started"),
            "{refused}"
        );
        let (_, unchanged) = request("GET", &uri, json!({}));
        assert_eq!(unchanged["status"], "backlog");

        let (status, claimed) = request(
            "POST",
            &format!("{base}/tickets/{}/claim", parked.slug),
            json!({"worker": "person"}),
        );
        assert_eq!(status, 200, "{claimed}");
        assert_eq!(claimed["status"], "started");
        let (status, working) = request("PATCH", &uri, json!({"started_phase": "working"}));
        assert_eq!(status, 200, "{working}");
        assert_eq!(working["claimed_by"], "person");
        assert!(working.get("warnings").is_none(), "{working}");

        let (status, shelved) = request("PATCH", &uri, json!({"status": "backlog"}));
        assert_eq!(status, 200, "{shelved}");
        assert!(shelved["claimed_by"].is_null());
        assert!(
            shelved["warnings"][0]
                .as_str()
                .unwrap()
                .starts_with(&format!("Released person's claim on {}", parked.slug)),
            "{shelved}"
        );
        // A later edit does not repeat the notice for the earlier release.
        let (status, titled) = request("PATCH", &uri, json!({"title": "renamed"}));
        assert_eq!(status, 200, "{titled}");
        assert!(titled.get("warnings").is_none(), "{titled}");
    })
    .await
    .unwrap();
}
