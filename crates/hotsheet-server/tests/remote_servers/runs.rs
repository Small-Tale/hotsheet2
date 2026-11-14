//! HS2-EBHDSB: dispatch runs against a real second server over authenticated TLS
//! through the owned CLI process, and through the stdio MCP tool on a live service.
use super::orchestration::api;
use super::*;
use hotsheet_model::Status;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, StoreMetadata, TicketPatch, ops};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FE0";
const NOW: &str = "2026-10-15T00:00:00Z";

fn ticket(store: &FsStore, prefix: &str, title: &str) -> hotsheet_model::Ticket {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_ebhdsb_real_tls_remote_dispatch_supervise_and_cancel_through_the_cli() {
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
    let connection = peer_checkout.sources[0].connection_id.clone();
    let peer_store = FsStore::open(&peer_source).unwrap();
    let first = ticket(&peer_store, "TST", "Synthetic remote work one");
    let second = ticket(&peer_store, "TST", "Synthetic remote work two");
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
    let orchestration = ticket(&owner_store, "OWN", "Coordinate the synthetic release");
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
    let store_arg = source.to_str().unwrap();
    let targets = [
        "-C",
        store_arg,
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
    let human = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        store_arg,
        "orchestration",
    ];
    let worker = [
        "--actor-role",
        "ai",
        "--actor-id",
        "scheduler-1",
        "-C",
        store_arg,
        "orchestration",
    ];
    // HS2-XHQ1EH: a person remembers the rules the orchestrator needs.
    for (action, tool) in [
        ("tickets_write", None::<&str>),
        ("work_start", None::<&str>),
    ] {
        let mut arguments = vec![
            "permission",
            "remember",
            TARGET,
            "--project",
            &owner.id,
            "--action",
            action,
        ];
        if let Some(tool) = tool {
            arguments.extend(["--tool", tool]);
        }
        cli_named(&home, &human, &arguments);
    }
    let first_id = first.id.to_string();
    let dispatch = [
        "run",
        "dispatch",
        "--project",
        &owner.id,
        "--ticket",
        &orchestration.slug,
        "--target",
        TARGET,
        "--connection",
        &connection,
        "--id",
        &first.slug,
        "--operation-id",
        "op-run-1",
        "--worker",
        "remote-worker-1",
        "--worker-label",
        "Remote worker",
        "--lease-minutes",
        "20",
    ];
    let run = cli_named(&home, &worker, &dispatch);
    assert_eq!(run["state"], "dispatched");
    assert_eq!(run["mode"], "direct");
    assert_eq!(run["worker_id"], "remote-worker-1");
    assert_eq!(run["target"]["native_id"], first_id);
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    // The claim lives on the peer; the controller holds only the run record.
    let held = peer_store.read_ticket(&first.id).unwrap();
    assert_eq!(held.claimed_by.as_deref(), Some("remote-worker-1"));
    assert_eq!(held.status, Status::Started);
    assert_eq!(held.claim_count, 1);
    let record = std::fs::read_to_string(home.join("orchestration-runs.json")).unwrap();
    for forbidden in [SECRET, peer.origin.as_str(), "Synthetic remote work"] {
        assert!(!record.contains(forbidden));
    }
    // Repeating the command is the same run and one claim.
    assert_eq!(cli_named(&home, &worker, &dispatch)["run_id"], run_id);
    assert_eq!(peer_store.read_ticket(&first.id).unwrap().claim_count, 1);
    // A second dispatch of the same ticket, and a busy worker, are refused.
    let other = |ticket: &str, operation: &str, worker_id: &str| {
        vec![
            "run".to_owned(),
            "dispatch".into(),
            "--project".into(),
            owner.id.clone(),
            "--ticket".into(),
            orchestration.slug.clone(),
            "--target".into(),
            TARGET.into(),
            "--connection".into(),
            connection.clone(),
            "--id".into(),
            ticket.into(),
            "--operation-id".into(),
            operation.into(),
            "--worker".into(),
            worker_id.into(),
        ]
    };
    let fail = |arguments: &[String]| {
        cli_failure(
            &home,
            &worker,
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    };
    assert!(fail(&other(&first_id, "op-run-2", "remote-worker-2")).contains("run_target_active"));
    assert!(
        fail(&other(
            &second.id.to_string(),
            "op-run-3",
            "remote-worker-1"
        ))
        .contains("run_worker_busy")
    );
    let run_args = |operation: &'static str| ["run", operation, &run_id, "--project", &owner.id];
    assert_eq!(
        cli_named(&home, &worker, &run_args("renew"))["state"],
        "dispatched"
    );
    assert_eq!(
        cli_named(&home, &worker, &run_args("sync"))["state"],
        "dispatched"
    );
    assert_eq!(
        cli_named(
            &home,
            &worker,
            &["run", "list", "--project", &owner.id, "--active"]
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    // HS2-7QHWHJ: status and the event stream through the CLI.
    let status = cli_named(&home, &worker, &run_args("status"));
    assert_eq!(status["reachability"], "reachable");
    assert_eq!(status["observed"]["claimed_by"], "remote-worker-1");
    let stream = cli_named(
        &home,
        &worker,
        &["run", "events", "--project", &owner.id, "--run", &run_id],
    );
    assert_eq!(stream["entries"][0]["kind"], "state");
    assert_eq!(stream["entries"][0]["state"], "dispatched");
    assert_eq!(stream["gap"], false);
    // A person on the peer takes the ticket over; the next sync reports it.
    ops::release(&peer_store, &first.id, NOW.into(), "remote-worker-1", false).unwrap();
    let taken = cli_named(&home, &worker, &run_args("sync"));
    assert_eq!(taken["state"], "intervened");
    assert!(
        fail(
            &run_args("renew")
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>()
        )
        .contains("run_not_active")
    );
    // The ticket is finished on the peer; the run completes and the orchestration
    // ticket records it.
    ops::update(
        &peer_store,
        &first.id,
        NOW.into(),
        TicketPatch {
            status: Some(Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
    let done = cli_named(&home, &worker, &run_args("sync"));
    assert_eq!(done["state"], "completed");
    let states: Vec<_> = done["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["to"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        states,
        ["requested", "dispatched", "intervened", "completed"]
    );
    let source_ticket = owner_store.read_ticket(&orchestration.id).unwrap();
    assert_eq!(source_ticket.orchestration_links.len(), 1);
    assert!(
        source_ticket
            .notes
            .iter()
            .any(|note| note.text.contains(&run_id))
    );
    // Dispatcher mode marks the peer's ticket Up Next and takes no claim; cancelling
    // withdraws the mark.
    let queued = cli_named(
        &home,
        &worker,
        &[
            "run",
            "dispatch",
            "--project",
            &owner.id,
            "--ticket",
            &orchestration.slug,
            "--target",
            TARGET,
            "--connection",
            &connection,
            "--id",
            &second.slug,
            "--operation-id",
            "op-run-4",
            "--via",
            "dispatcher",
        ],
    );
    assert_eq!(queued["state"], "queued");
    let marked = peer_store.read_ticket(&second.id).unwrap();
    assert!(marked.up_next);
    assert!(marked.claimed_by.is_none());
    let queued_id = queued["run_id"].as_str().unwrap().to_owned();
    let cancelled = cli_named(
        &home,
        &worker,
        &["run", "cancel", &queued_id, "--project", &owner.id],
    );
    assert_eq!(cancelled["state"], "cancelled");
    assert!(!peer_store.read_ticket(&second.id).unwrap().up_next);
    // The peer narrows the pairing to read-only: a new dispatch is refused by the
    // target itself and the run records the failure reason.
    set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadOnly).unwrap();
    let refused = fail(&other(
        &second.id.to_string(),
        "op-run-5",
        "remote-worker-1",
    ));
    assert!(
        refused.contains("target_provider_unauthorized"),
        "{refused}"
    );
    assert!(
        peer_store
            .read_ticket(&second.id)
            .unwrap()
            .claimed_by
            .is_none()
    );
    let shown = cli_named(
        &home,
        &worker,
        &["run", "show", &run_id, "--project", &owner.id],
    );
    assert_eq!(shown["state"], "completed");
    peer.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_ebhdsb_stdio_mcp_dispatches_and_supervises_a_run() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let connection = checkout.sources[0].connection_id.clone();
    let store = FsStore::open(&source).unwrap();
    let orchestration = ticket(&store, "TST", "Coordinate through MCP");
    let work = ticket(&store, "TST", "Work to dispatch");
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
    let added = api(
        router.clone(),
        "POST",
        &format!("{base}/targets"),
        json!({"target_id":TARGET,"label":"Self","expected_revision":null}),
        true,
    )
    .await
    .1;
    let candidate = api(
        router.clone(),
        "POST",
        &format!("{base}/inspect"),
        json!({"destination":{"kind":"local","checkout_id":checkout.id}}),
        true,
    )
    .await
    .1;
    api(
        router.clone(),
        "PUT",
        &format!("{base}/targets/{TARGET}/binding"),
        json!({"target":{"owner_checkout_id":checkout.id,"target_id":TARGET},
            "expected_manifest_revision":added["revision"],"expected_binding_revision":0,
            "previous_destination":null,"destination":candidate["destination"]}),
        true,
    )
    .await;
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 3_600_000;
    super::orchestration::remember_rules(
        router.clone(),
        &format!("{base}/targets/{TARGET}"),
        json!({"actor_id":"scheduler-1","actions":["tickets_read","work_start"],
            "expires_at_ms":expires,"actor":{"role":"human","id":"maintainer"}}),
    )
    .await;
    // The run routes need the bearer like everything else.
    for (method, path) in [
        ("GET", format!("{base}/runs")),
        ("POST", format!("{base}/runs")),
        ("POST", format!("/checkouts/{}/target-claims", checkout.id)),
    ] {
        assert_eq!(
            api(router.clone(), method, &path, json!({}), false).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app(host))
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let backend = ["--server", origin.as_str(), "--secret", SECRET];
    let call = |mut arguments: Value| {
        arguments["checkout"] = json!(checkout.id);
        arguments["actor_role"] = json!("ai");
        arguments["actor_id"] = json!("scheduler-1");
        mcp_named(&home, &backend, "hotsheet_targets", arguments)
    };
    let dispatch = json!({"action":"run_dispatch","target_id":TARGET,
        "ticket":{"native_id":orchestration.slug},"connection_id":connection,
        "native_id":work.slug,"operation_id":"op-mcp-run","worker_id":"mcp-worker",
        "lease_minutes":15});
    let run = call(dispatch.clone());
    assert_eq!(run["state"], "dispatched");
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    assert_eq!(call(dispatch)["run_id"], run_id);
    assert_eq!(
        store.read_ticket(&work.id).unwrap().claimed_by.as_deref(),
        Some("mcp-worker")
    );
    assert_eq!(store.read_ticket(&work.id).unwrap().claim_count, 1);
    assert_eq!(
        call(json!({"action":"run_show","run_id":run_id}))["run_id"],
        run_id
    );
    assert_eq!(
        call(json!({"action":"run_list","active":true}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(json!({"action":"run_renew","run_id":run_id,"lease_minutes":30}))["state"],
        "dispatched"
    );
    assert_eq!(
        call(json!({"action":"run_sync","run_id":run_id}))["state"],
        "dispatched"
    );
    // HS2-7QHWHJ: status and the event stream through the tool.
    let status = call(json!({"action":"run_status","run_id":run_id}));
    assert_eq!(status["reachability"], "reachable");
    assert_eq!(status["stale"], false);
    assert_eq!(status["observed"]["claimed_by"], "mcp-worker");
    let stream = call(json!({"action":"run_events","run_id":run_id,"after":0}));
    let kinds: Vec<_> = stream["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["kind"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(kinds[0], "state");
    assert!(kinds.contains(&"observed".to_owned()));
    // HS2-8C9KFP: the server starts a follower for the new run, and the follower's
    // first step reconciles it (HS2-WBREDM), which publishes a `reconciled` event.
    // Under load that step can run after the snapshot above, so the feed is quiet only
    // once it has: follow the feed, woken by its own long poll, until it is there.
    let mut cursor = stream["cursor"].clone();
    let mut reconciled = kinds.contains(&"reconciled".to_owned());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !reconciled {
        assert!(
            std::time::Instant::now() < deadline,
            "the run's follower never reconciled it"
        );
        let page = call(json!({"action":"run_events","run_id":run_id,"after":cursor,
            "wait_ms":5000}));
        reconciled = page["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "reconciled");
        cursor = page["cursor"].clone();
    }
    // With the follower caught up, nothing more is published for an unchanged run.
    assert_eq!(
        call(json!({"action":"run_events","after":cursor,"wait_ms":200}))["entries"],
        json!([])
    );
    let cancelled = call(json!({"action":"run_cancel","run_id":run_id}));
    assert_eq!(cancelled["state"], "cancelled");
    assert!(store.read_ticket(&work.id).unwrap().claimed_by.is_none());
    for arguments in [
        json!({"action":"run_show","checkout":checkout.id,"run_id":"not-a-run"}),
        json!({"action":"run_cancel","checkout":checkout.id,"run_id":run_id,
            "actor_role":"ai","actor_id":"scheduler-1"}),
        json!({"action":"run_dispatch","checkout":checkout.id,"target_id":TARGET,
            "ticket":{"native_id":orchestration.slug},"connection_id":connection,
            "native_id":work.slug,"operation_id":"op-other","worker_id":"mcp-worker"}),
    ] {
        mcp_rejected(&home, &backend, "hotsheet_targets", arguments);
    }
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}

/// HS2-7QHWHJ: a run follows its remote target ticket to completion with nobody
/// calling sync: the controller's follower holds one request open on the peer's
/// change feed and the subscriber waits on the run event stream.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_7qhwhj_a_remote_run_completes_from_the_peers_change_feed_without_polling() {
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
    let connection = peer_checkout.sources[0].connection_id.clone();
    let peer_store = FsStore::open(&peer_source).unwrap();
    let work = ticket(&peer_store, "TST", "Followed remotely");
    let expected = identity(&peer_home);
    // A second router on the same peer state stands in for a worker using the peer's
    // own API; its ticket writes reach the peer's change stream.
    let peer_api = app(peer_state.clone());
    let peer = serve(peer_state, &paths, None).await;
    let home = root.path().join("controller-home");
    std::fs::create_dir(&home).unwrap();
    let source = root.path().join("controller-store");
    let memory = Arc::new(Memory::default());
    let host = state(&source, &home);
    let owner_store = FsStore::open(&source).unwrap();
    let project = root.path().join("owner");
    std::fs::create_dir(&project).unwrap();
    let owner = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let orchestration = ticket(&owner_store, "TST", "Coordinate and follow");
    runtime(&home, &memory)
        .register(hotsheet_ticketing::remote_servers::RegisterRemote {
            id: ID.into(),
            label: "Synthetic peer".into(),
            origin: peer.origin.clone(),
            expected_server_id: expected.server_id,
            credentials: credentials(&device),
        })
        .unwrap();
    let host = host.with_orchestration_runtime(super::orchestration::shared_runtime(
        &source, &home, &memory,
    ));
    let router = app(host);
    let base = format!("/checkouts/{}/orchestration", owner.id);
    let added = api(
        router.clone(),
        "POST",
        &format!("{base}/targets"),
        json!({"target_id":TARGET,"label":"Remote","expected_revision":null}),
        true,
    )
    .await
    .1;
    let candidate = api(
        router.clone(),
        "POST",
        &format!("{base}/inspect"),
        json!({"destination":{"kind":"remote","registration_id":ID,"checkout_id":peer_checkout.id}}),
        true,
    )
    .await
    .1;
    let bound = api(
        router.clone(),
        "PUT",
        &format!("{base}/targets/{TARGET}/binding"),
        json!({"target":{"owner_checkout_id":owner.id,"target_id":TARGET},
            "expected_manifest_revision":added["revision"],"expected_binding_revision":0,
            "previous_destination":null,"destination":candidate["destination"]}),
        true,
    )
    .await
    .1;
    assert_eq!(bound["usable"], true, "{bound}");
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 3_600_000;
    super::orchestration::remember_rules(
        router.clone(),
        &format!("{base}/targets/{TARGET}"),
        json!({"actor_id":"scheduler-1","actions":["tickets_read","work_start"],
            "expires_at_ms":expires,"actor":{"role":"human","id":"maintainer"}}),
    )
    .await;
    // Before any run exists a subscriber's wait ends empty at its deadline.
    let (status, idle) = api(
        router.clone(),
        "GET",
        &format!("{base}/run-events?after=0&wait_ms=300"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(idle["entries"], json!([]));
    let (status, run) = api(
        router.clone(),
        "POST",
        &format!("{base}/runs"),
        json!({"ticket":{"native_id":orchestration.slug},"target_id":TARGET,
            "connection_id":connection,"native_id":work.slug,"operation_id":"op-follow",
            "worker_id":"remote-worker","actor":{"role":"ai","id":"scheduler-1"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{run}");
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    // Read the stream up to now; the dispatch is there.
    let events = |after: u64, wait_ms: u64| {
        let router = router.clone();
        let path = format!("{base}/run-events?after={after}&wait_ms={wait_ms}&run={run_id}");
        async move { api(router, "GET", &path, Value::Null, true).await.1 }
    };
    let first = events(0, 0).await;
    assert_eq!(first["entries"][0]["state"], "dispatched");
    // A worker completes the ticket through the peer's own API. Nobody asks the
    // controller to sync; the subscriber only waits on the stream.
    let (status, _) = api(
        peer_api,
        "PATCH",
        &format!("/tickets/{}", work.id),
        json!({"status":"completed","note":"Synthetic completion"}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut cursor = first["cursor"].as_u64().unwrap();
    let mut seen = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    while std::time::Instant::now() < deadline && !seen.contains(&"completed".to_owned()) {
        // Each request waits for the next event; this loop is cursor replay, not a timer.
        let page = events(cursor, 10_000).await;
        cursor = page["cursor"].as_u64().unwrap();
        for entry in page["entries"].as_array().unwrap() {
            if let Some(state) = entry["state"].as_str() {
                seen.push(state.to_owned());
            }
            if entry["kind"] == "state" {
                assert_eq!(entry["source"], "target");
            }
        }
    }
    assert_eq!(
        seen,
        ["completed"],
        "the run never completed from the peer's feed"
    );
    let (status, view) = api(
        router.clone(),
        "GET",
        &format!("{base}/runs/{run_id}/status"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["state"], "completed");
    assert_eq!(view["reachability"], "reachable");
    assert_eq!(view["stale"], false);
    assert_eq!(view["observed"]["status"], "completed");
    // The peer's hosted feed needs the bearer and answers a handshake at once.
    let hosted = format!("/checkouts/{}/target-events", peer_checkout.id);
    let peer_router = app(state(&peer_source, &peer_home));
    assert_eq!(
        api(peer_router.clone(), "GET", &hosted, Value::Null, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, handshake) = api(peer_router, "GET", &hosted, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(handshake["events"], json!([]));
    peer.stop().await;
}
