use super::*;
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
            let _guard = resource.lock().unwrap();
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
    assert_eq!(state.watchers.lock().unwrap().len(), 1);
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
