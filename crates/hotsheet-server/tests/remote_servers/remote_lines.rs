//! HS2-3D8D6V: the orchestration line of a target ticket on a remote server, read on
//! the machine that runs the orchestration project. A real peer over authenticated
//! mTLS hosts the target; the controller binds it through the owned CLI, plans a step
//! on it, and its own service, CLI and MCP answer the remote checkout's lines from the
//! controller's records. Synthetic stores and ids.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::checkouts::{Checkout, CheckoutRegistry};
use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};
use std::path::PathBuf;

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FM0";
const NOW: &str = "2026-10-15T00:00:00Z";

pub(super) struct Bound {
    pub(super) root: tempfile::TempDir,
    pub(super) home: PathBuf,
    pub(super) source: PathBuf,
    pub(super) owner: Checkout,
    pub(super) peer_home: PathBuf,
    pub(super) peer_source: PathBuf,
    pub(super) peer_checkout: Checkout,
    pub(super) peer: Running,
    pub(super) step: String,
    pub(super) work: hotsheet_model::Ticket,
}

fn create(store: &FsStore, prefix: &str, title: &str) -> hotsheet_model::Ticket {
    ops::create(
        store,
        hotsheet_model::Ulid::new(),
        prefix,
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

/// A controller that binds a target on a real mTLS peer and plans one step on it.
pub(super) async fn bound() -> Bound {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-orchestrator").unwrap();
    set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadWrite).unwrap();
    let peer_home = root.path().join("peer-home");
    let peer_source = root.path().join("peer-store");
    let peer_state = state(&peer_source, &peer_home);
    let peer_project = root.path().join("peer-project");
    std::fs::create_dir(&peer_project).unwrap();
    let peer_checkout = CheckoutRegistry::new(peer_home.join("checkouts.json"))
        .register(&peer_project, None, None, vec![peer_source.clone()])
        .unwrap();
    let work = create(
        &FsStore::open(&peer_source).unwrap(),
        "TST",
        "Synthetic remote work",
    );
    let expected = identity(&peer_home);
    let peer = serve(peer_state, &paths, None).await;

    let home = root.path().join("controller-home");
    std::fs::create_dir(&home).unwrap();
    let source = root.path().join("controller-store");
    let owner_store = FsStore::init_git(&source, &StoreMetadata::new("OWN")).unwrap();
    let project = root.path().join("owner");
    std::fs::create_dir(&project).unwrap();
    let owner = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let goal = create(&owner_store, "OWN", "Coordinate the synthetic release");
    let input = home.join("supplied-credentials.json");
    write_credentials(&input, &credentials(&device));
    cli(
        &home,
        &[
            "register",
            "--id",
            ID,
            "--label",
            "Synthetic",
            "--origin",
            &peer.origin,
            "--expected-server-id",
            &expected.server_id,
            "--credentials-file",
            input.to_str().unwrap(),
        ],
    );
    let store_arg = source.to_str().unwrap().to_owned();
    let targets = [
        "-C",
        &store_arg,
        "orchestration",
        "target",
        "--project",
        &owner.id,
    ];
    let inspected = cli_named(
        &home,
        &targets,
        &["inspect", &peer_checkout.id, "--registration-id", ID],
    );
    let added = cli_named(
        &home,
        &targets,
        &["add", "--target-id", TARGET, "--label", "Remote target"],
    );
    cli_named(
        &home,
        &targets,
        &[
            "bind",
            TARGET,
            "--expected-manifest-revision",
            added["revision"].as_str().unwrap(),
            "--expected-binding-revision",
            "0",
            "--destination",
            &inspected["destination"].to_string(),
        ],
    );
    let worker = [
        "--actor-role",
        "ai",
        "--actor-id",
        "worker-1",
        "-C",
        &store_arg,
        "orchestration",
    ];
    let defined = cli_named(
        &home,
        &worker,
        &[
            "readiness",
            "define",
            "--project",
            &owner.id,
            "--ticket",
            &goal.slug,
            "--target",
            TARGET,
            "--connection",
            &peer_checkout.sources[0].connection_id,
            "--id",
            &work.id.to_string(),
        ],
    );
    let step = defined["step"]["step_id"].as_str().unwrap().to_owned();
    Bound {
        root,
        home,
        source,
        owner,
        peer_home,
        peer_source,
        peer_checkout,
        peer,
        step,
        work,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_3d8d6v_the_controller_answers_a_remote_checkouts_lines_over_http_cli_and_mcp() {
    let b = bound().await;
    let controller = state(&b.source, &b.home);
    let router = app(controller.clone());
    let lines = format!(
        "/remotes/{ID}/checkouts/{}/orchestration/target-lines",
        b.peer_checkout.id
    );
    assert_eq!(
        api(router.clone(), "GET", &lines, Value::Null, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, read) = api(router.clone(), "GET", &lines, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["orchestrated"], true);
    let line = &read["lines"][0];
    assert_eq!(line["item"]["step_id"], b.step);
    assert_eq!(line["item"]["target"]["native_id"], b.work.id.to_string());
    assert_eq!(line["item"]["state"], "ready");
    assert_eq!(line["item"]["location"], "remote");
    assert_eq!(line["owner_checkout_id"], b.owner.id);
    assert_eq!(line["proposed"], true);
    // Another registration, or the local route for the remote id, is not this line.
    let other = format!(
        "/remotes/01ARZ3NDEKTSV4RRFFQ69G5FM9/checkouts/{}/orchestration/target-lines",
        b.peer_checkout.id
    );
    assert_eq!(
        api(router.clone(), "GET", &other, Value::Null, true)
            .await
            .1["orchestrated"],
        false
    );
    // The CLI and MCP answer the same version from the controller's records.
    let plain = ["-C", b.source.to_str().unwrap(), "orchestration"];
    let over_cli = cli_named(
        &b.home,
        &plain,
        &[
            "target-lines",
            "--registration-id",
            ID,
            "--project",
            &b.peer_checkout.id,
        ],
    );
    assert_eq!(over_cli["version"], read["version"]);
    assert_eq!(over_cli["lines"][0]["item"]["step_id"], b.step);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app(controller))
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let backend = ["--server", origin.as_str(), "--secret", SECRET];
    let over_mcp = mcp_named(
        &b.home,
        &backend,
        "hotsheet_targets",
        json!({"action":"target_lines","checkout":b.peer_checkout.id,"registration_id":ID}),
    );
    assert_eq!(over_mcp["version"], read["version"]);
    // A waiting read with the current version answers it at the deadline.
    let version = read["version"].as_str().unwrap();
    let waited = api(
        router.clone(),
        "GET",
        &format!("{lines}?after={version}&wait_ms=300"),
        Value::Null,
        true,
    )
    .await
    .1;
    assert_eq!(waited["version"], read["version"]);
    assert_eq!(
        api(
            router,
            "GET",
            &format!("{lines}?after=a%2Fb&wait_ms=10"),
            Value::Null,
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    // Nothing was written on the peer: its store holds only the target ticket.
    assert_eq!(
        FsStore::open(&b.peer_source)
            .unwrap()
            .list_tickets()
            .unwrap()
            .len(),
        1
    );
    let _ = (&b.root, &b.peer_home, &b.peer);
}
