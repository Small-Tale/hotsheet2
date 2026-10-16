use super::*;
use hotsheet_model::{CloseReason, Ticket, Ulid, to_file_string};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{NewTicket, TicketPatch, ops};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

/// Hold a real shared resource outside the async runtime, releasing even on failure.
struct HeldResource {
    release: Option<mpsc::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
    active: Arc<AtomicBool>,
}

impl HeldResource {
    fn new<T: Send + 'static>(resource: Arc<Mutex<T>>) -> Self {
        let (release, released) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        let held = active.clone();
        let worker = std::thread::spawn(move || {
            let _guard = resource.lock_or_recover();
            ready.send(()).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(10));
            held.store(false, Ordering::SeqCst);
        });
        started.recv().unwrap();
        Self {
            release: Some(release),
            worker: Some(worker),
            active,
        }
    }
}

impl Drop for HeldResource {
    fn drop(&mut self) {
        let _ = self.release.take().unwrap().send(());
        self.worker.take().unwrap().join().unwrap();
    }
}

fn project_fixture() -> (tempfile::TempDir, AppState, String, Ticket) {
    let root = tempfile::tempdir().unwrap();
    let store = FsStore::init(
        root.path().join("tickets.hs2"),
        &hotsheet_ticketing::StoreMetadata::new("HS"),
    )
    .unwrap();
    let ticket = ops::create(
        &store,
        Ulid::new(),
        "HS",
        now(),
        NewTicket {
            title: "Fast detail".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let state = AppState::new(store.clone(), "secret".into())
        .unwrap()
        .with_checkout_registry(root.path().join("checkouts.json"));
    let checkout = state
        .checkout_registry
        .register(
            root.path(),
            Some("project"),
            None,
            vec![store.root().to_path_buf()],
        )
        .unwrap();
    (root, state, checkout.id, ticket)
}

#[tokio::test]
async fn waiting_for_a_backlink_index_does_not_block_a_detail_read() {
    let (_root, state, project, ticket) = project_fixture();
    let held = HeldResource::new(state.index.clone());
    let slow_state = state.clone();
    let slow_project = project.clone();
    let id = ticket.id.to_string();
    let slow_id = id.clone();
    let slow = tokio::spawn(async move {
        get_checkout_ticket_duplicate_backlinks(State(slow_state), Path((slow_project, slow_id)))
            .await
    });
    tokio::task::yield_now().await;
    let detail = tokio::time::timeout(
        Duration::from_secs(2),
        get_checkout_ticket(State(state), Path((project, id))),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(detail.0.ticket.title, "Fast detail");
    assert!(
        held.active.load(Ordering::SeqCst),
        "detail waited for the backlink/index scan"
    );
    assert!(!slow.is_finished());
    drop(held);
    assert!(slow.await.unwrap().unwrap().0.backlinks.is_empty());
}

#[tokio::test]
async fn terminal_model_discovery_waits_off_runtime_while_detail_reads_progress() {
    let (root, mut state, project, ticket) = project_fixture();
    let plugin = root.path().join("plugins/fake");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("instructions.md"), "Test setup\n").unwrap();
    std::fs::write(
        plugin.join("manifest.toml"),
        r#"
id = "fake"
display_name = "Fake"
product_name = "Fake"
tier = "cli-agent"
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
[launch]
program = "/bin/cat"
args = []
"#,
    )
    .unwrap();
    state = state.with_plugin_dirs(vec![root.path().join("plugins")]);
    let held = HeldResource::new(state.model_catalogs.clone());
    let launch_state = state.clone();
    let req = serde_json::from_value(
        serde_json::json!({"connect":"fake", "model":"unavailable", "id":"slow"}),
    )
    .unwrap();
    let launch = tokio::spawn(async move { open_terminal(State(launch_state), Json(req)).await });
    tokio::task::yield_now().await;
    let detail = tokio::time::timeout(
        Duration::from_secs(2),
        get_checkout_ticket(State(state), Path((project, ticket.id.to_string()))),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(detail.0.ticket.title, "Fast detail");
    assert!(
        held.active.load(Ordering::SeqCst),
        "model discovery blocked the async worker"
    );
    assert!(!launch.is_finished());
    drop(held);
    assert!(
        launch.await.unwrap().is_err(),
        "unavailable model must still be validated"
    );
}

#[tokio::test]
async fn concurrent_cold_backlink_sources_initialize_once_and_keep_updates() {
    let (root, state, project, target) = project_fixture();
    let source_root = root.path().join("other");
    std::fs::create_dir(&source_root).unwrap();
    let source = FsStore::init(
        source_root.join("source.hs2"),
        &hotsheet_ticketing::StoreMetadata::new("EX"),
    )
    .unwrap();
    state
        .checkout_registry
        .register(
            &source_root,
            Some("other"),
            None,
            vec![source.root().to_path_buf()],
        )
        .unwrap();
    let duplicate = ops::create(
        &source,
        Ulid::new(),
        "EX",
        now(),
        NewTicket {
            title: "Duplicate".into(),
            ..Default::default()
        },
    )
    .unwrap();
    ops::close(
        &source,
        &duplicate.id,
        now(),
        CloseReason::Duplicate,
        Some(target.id.to_string()),
    )
    .unwrap();
    let mut requests = Vec::new();
    for _ in 0..4 {
        let (state, project, id) = (state.clone(), project.clone(), target.id.to_string());
        requests.push(tokio::spawn(async move {
            get_checkout_ticket_duplicate_backlinks(State(state), Path((project, id))).await
        }));
    }
    for request in requests {
        assert_eq!(request.await.unwrap().unwrap().0.backlinks.len(), 1);
    }
    assert_eq!(state.host.count(), 2);
    assert_eq!(state.watchers.lock_or_recover().len(), 1);
    let healthy = source.read_ticket(&duplicate.id).unwrap();
    std::fs::write(source.ticket_path(&duplicate.id), "corrupt ticket bytes").unwrap();
    assert!(
        get_checkout_ticket_duplicate_backlinks(
            State(state.clone()),
            Path((project.clone(), target.id.to_string()))
        )
        .await
        .unwrap()
        .0
        .backlinks
        .is_empty(),
        "a retained healthy index row must not expose a corrupt source"
    );
    std::fs::write(source.ticket_path(&duplicate.id), to_file_string(&healthy)).unwrap();
    assert_eq!(
        get_checkout_ticket_duplicate_backlinks(
            State(state.clone()),
            Path((project.clone(), target.id.to_string()))
        )
        .await
        .unwrap()
        .0
        .backlinks
        .len(),
        1,
        "repair must restore the link without reinitializing its source"
    );
    let entry = state.host.get(&multistore::store_url_id(&source)).unwrap();
    let reopened = ops::update(
        &source,
        &duplicate.id,
        now(),
        TicketPatch {
            status: Some(Status::Started),
            ..Default::default()
        },
    )
    .unwrap();
    state.changed_in(&entry, "updated", &reopened);
    assert!(
        get_checkout_ticket_duplicate_backlinks(
            State(state),
            Path((project, target.id.to_string()))
        )
        .await
        .unwrap()
        .0
        .backlinks
        .is_empty()
    );
}

fn routed(uri: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .uri(uri)
        .header("x-hotsheet-secret", "secret")
        .body(axum::body::Body::empty())
        .unwrap()
}

/// Through the real router on a single async worker: a list request blocked synchronously on
/// the held index lock must not starve an unrelated ticket detail or health check
/// (HS2-9Z0Q8N). Without the blocking-offload layer the list handler parks the only worker
/// and both of these time out.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn a_blocked_store_request_does_not_starve_unrelated_routes() {
    use tower::ServiceExt;
    let (_root, state, project, ticket) = project_fixture();
    let held = HeldResource::new(state.index.clone());
    let router = app(state);
    let list = tokio::spawn(router.clone().oneshot(routed("/tickets")));
    tokio::time::sleep(Duration::from_millis(50)).await;
    for uri in [
        "/health".to_string(),
        format!("/checkouts/{project}/tickets/{}", ticket.id),
    ] {
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            tokio::spawn(router.clone().oneshot(routed(&uri))),
        )
        .await
        .unwrap_or_else(|_| panic!("{uri} was starved by a blocked store request"))
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
    }
    assert!(held.active.load(Ordering::SeqCst));
    assert!(!list.is_finished(), "the list request waits for the index");
    drop(held);
    let list = tokio::time::timeout(Duration::from_secs(5), list)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    // Its Server-Timing names the mutex wait separately from the handler total (HS2-P867K1).
    let timing: Vec<_> = list
        .headers()
        .get_all("server-timing")
        .iter()
        .map(|value| value.to_str().unwrap().to_string())
        .collect();
    let wait_ms: f64 = timing
        .iter()
        .find_map(|entry| entry.split("index-wait;dur=").nth(1))
        .and_then(|rest| rest.split(',').next())
        .and_then(|ms| ms.trim().parse().ok())
        .unwrap_or_else(|| panic!("no index-wait entry in {timing:?}"));
    assert!(
        wait_ms >= 40.0,
        "the held lock shows as index wait: {timing:?}"
    );
    for phase in [
        "index-query",
        "index-phases",
        "settings",
        "encode",
        "handler",
    ] {
        assert!(
            timing
                .iter()
                .any(|entry| entry.contains(&format!("{phase};dur="))),
            "{phase} missing from {timing:?}"
        );
    }
}

/// Through the real router: more pending permission asks than there are API offload slots
/// must not delay an unrelated request, and `/health` reports them as long waits rather than
/// slots in use (HS2-V3S89Q). Before the fix each parked ask held a slot for up to 24 h, so
/// the 257th request queued silently behind them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_permission_asks_do_not_queue_unrelated_requests() {
    use tower::ServiceExt;
    let (_root, state, project, ticket) = project_fixture();
    let bridge = state.permissions.clone();
    let router = app(state);
    let asks = crate::blocking_offload::MAX_OFFLOADED + 8;
    let pending: Vec<_> = (0..asks)
        .map(|index| {
            let request = axum::http::Request::builder()
                .method("POST")
                .uri("/permissions/ask")
                .header("x-hotsheet-secret", "secret")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({
                        "connection": format!("agent-{index}"),
                        "tool": "Bash",
                        "action": format!("run {index}"),
                    })
                    .to_string(),
                ))
                .unwrap();
            tokio::spawn(router.clone().oneshot(request))
        })
        .collect();
    tokio::time::timeout(Duration::from_secs(20), async {
        while bridge.pending().len() < asks {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("every ask should be pending");
    for uri in [
        "/health".to_string(),
        format!("/checkouts/{project}/tickets/{}", ticket.id),
    ] {
        let response =
            tokio::time::timeout(Duration::from_secs(2), router.clone().oneshot(routed(&uri)))
                .await
                .unwrap_or_else(|_| panic!("{uri} queued behind pending permission asks"))
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let timing: Vec<_> = response
            .headers()
            .get_all("server-timing")
            .iter()
            .map(|value| value.to_str().unwrap().to_string())
            .collect();
        assert!(
            timing.iter().all(|entry| !entry.starts_with("queue")),
            "{uri} waited for an offload slot: {timing:?}"
        );
        if uri == "/health" {
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let health: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let offload = &health["api_offload"];
            assert_eq!(offload["long_waits"], asks, "{offload}");
            assert_eq!(offload["waiting"], 0, "{offload}");
            assert_eq!(offload["capacity"], crate::blocking_offload::MAX_OFFLOADED);
            // Only the health request itself holds a slot.
            assert!(offload["in_use"].as_u64().unwrap() <= 1, "{offload}");
        }
    }
    // A disconnected hook withdraws its prompt (the non-offloaded path still cancels).
    for ask in &pending {
        ask.abort();
    }
    for ask in pending {
        let _ = ask.await;
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        while !bridge.pending().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("aborted asks withdraw their prompts");
}

/// More pending permission asks than Tokio's default blocking pool (512 threads) must not
/// starve offloaded API work (HS2-43YT58). Before the async wait each pending ask parked one
/// blocking-pool thread on the bridge Condvar, so the 513th `spawn_blocking` (every offloaded
/// request) queued inside Tokio, invisible to the offload-slot saturation warning.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_permission_asks_beyond_the_blocking_pool_leave_offloaded_requests_prompt() {
    use tower::ServiceExt;
    const TOKIO_DEFAULT_MAX_BLOCKING_THREADS: usize = 512;
    let (_root, state, project, ticket) = project_fixture();
    let bridge = state.permissions.clone();
    let router = app(state);
    let asks = TOKIO_DEFAULT_MAX_BLOCKING_THREADS + 64;
    let pending: Vec<_> = (0..asks)
        .map(|index| {
            let request = axum::http::Request::builder()
                .method("POST")
                .uri("/permissions/ask")
                .header("x-hotsheet-secret", "secret")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({
                        "connection": format!("agent-{index}"),
                        "tool": "Bash",
                        "action": format!("run {index}"),
                    })
                    .to_string(),
                ))
                .unwrap();
            tokio::spawn(router.clone().oneshot(request))
        })
        .collect();
    tokio::time::timeout(Duration::from_secs(20), async {
        while bridge.pending().len() < asks {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("every ask should be pending");
    // Offloaded store reads still run promptly, and a direct spawn_blocking is served.
    let uri = format!("/checkouts/{project}/tickets/{}", ticket.id);
    for _ in 0..4 {
        let response =
            tokio::time::timeout(Duration::from_secs(2), router.clone().oneshot(routed(&uri)))
                .await
                .expect("an offloaded request queued behind pending permission asks")
                .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    tokio::time::timeout(Duration::from_secs(2), tokio::task::spawn_blocking(|| ()))
        .await
        .expect("the blocking pool is exhausted by pending permission asks")
        .unwrap();
    // Answering one ask still wakes exactly that hook with the human decision.
    let first = bridge.pending()[0].id;
    bridge
        .resolve(
            first,
            hotsheet_aitools::PermissionDecision::Allow,
            hotsheet_aitools::PermissionScope::Once,
        )
        .unwrap();
    assert_eq!(bridge.pending().len(), asks - 1);
    for ask in &pending {
        ask.abort();
    }
    let mut answered = 0;
    for ask in pending {
        if let Ok(Ok(response)) = ask.await {
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let decision: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(decision["decision"], "allow");
            answered += 1;
        }
    }
    assert!(answered <= 1, "only the answered ask completes");
    tokio::time::timeout(Duration::from_secs(10), async {
        while !bridge.pending().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("aborted asks withdraw their prompts");
}
