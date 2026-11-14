//! HS2-AZ15GA: target tickets against a real second server over authenticated TLS,
//! driven by the owned CLI process, plus the hosted routes of a live service.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};
use std::path::Path;

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FD0";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_az15ga_real_tls_remote_target_tickets_through_the_owned_cli() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-orchestrator").unwrap();
    set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadWrite).unwrap();
    // The peer machine hosts the target checkout and its git ticket store.
    let peer_home = root.path().join("peer-home");
    let peer_source = root.path().join("peer-store");
    let peer_state = state(&peer_source, &peer_home);
    let peer_project = root.path().join("peer-project");
    std::fs::create_dir(&peer_project).unwrap();
    let peer_checkout = CheckoutRegistry::new(peer_home.join("checkouts.json"))
        .register(&peer_project, None, None, vec![peer_source.clone()])
        .unwrap();
    let peer_connection = peer_checkout.sources[0].connection_id.clone();
    let expected = identity(&peer_home);
    let peer = serve(peer_state, &paths, None).await;
    // The controller machine owns the orchestration project and its ticket store.
    let home = root.path().join("controller-home");
    std::fs::create_dir(&home).unwrap();
    let source = root.path().join("controller-store");
    let owner_store = FsStore::init_git(&source, &StoreMetadata::new("OWN")).unwrap();
    let project = root.path().join("owner");
    std::fs::create_dir(&project).unwrap();
    let owner = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let orchestration = ops::create(
        &owner_store,
        hotsheet_model::Ulid::new(),
        "OWN",
        "2026-10-15T00:00:00Z".into(),
        NewTicket {
            title: "Coordinate the synthetic release".into(),
            ..Default::default()
        },
    )
    .unwrap();
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
    assert_eq!(
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
                &inspected["destination"].to_string()
            ]
        )["usable"],
        true
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
        "worker-1",
        "-C",
        store_arg,
        "orchestration",
    ];
    let create = [
        "ticket",
        "create",
        "--project",
        &owner.id,
        "--ticket",
        &orchestration.slug,
        "--target",
        TARGET,
        "--connection",
        &peer_connection,
        "--operation-id",
        "op-remote-1",
        "--title",
        "Synthetic remote work",
        "--details",
        "Created through the reviewed transport",
        "--tag",
        "orchestrated",
        "--priority",
        "high",
    ];
    // HS2-XHQ1EH: bound is consented to, but an AI's write asks a person first; the
    // peer's store is untouched meanwhile.
    assert!(cli_failure(&home, &worker, &create).contains("permission_required"));
    let peer_store = FsStore::open(&peer_source).unwrap();
    assert!(peer_store.list_tickets().unwrap().is_empty());
    // HS2-XHQ1EH: a person remembers the rules the orchestrator needs.
    {
        let (action, tool) = ("tickets_write", None::<&str>);
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
    // An unidentified caller is not an actor the grant can cover.
    assert!(
        cli_failure(&home, &["-C", store_arg, "orchestration"], &create)
            .contains("identified actor")
    );
    let created = cli_named(&home, &worker, &create);
    assert_eq!(created["created"], true);
    assert_eq!(created["link"], json!({"recorded":true}));
    assert_eq!(created["ticket"]["title"], "Synthetic remote work");
    assert_eq!(created["ticket"]["priority"], "high");
    let native_id = created["ticket"]["native_id"].as_str().unwrap().to_owned();
    let on_peer = peer_store.list_tickets().unwrap();
    assert_eq!(on_peer.len(), 1);
    assert_eq!(on_peer[0].id.to_string(), native_id);
    assert_eq!(on_peer[0].tags, ["orchestrated"]);
    assert_eq!(
        on_peer[0].transfer_operation_id.as_deref(),
        Some("op-remote-1")
    );
    // The controller's store holds only its own ticket, now carrying the link.
    assert_eq!(owner_store.list_tickets().unwrap().len(), 1);
    let linked = owner_store.read_ticket(&orchestration.id).unwrap();
    assert_eq!(linked.orchestration_links.len(), 1);
    assert_eq!(
        linked.orchestration_links[0].native_id.as_deref(),
        Some(native_id.as_str())
    );
    let file = std::fs::read_to_string(owner_store.ticket_path(&orchestration.id)).unwrap();
    for forbidden in [SECRET, peer.origin.as_str(), peer_checkout.id.as_str()] {
        assert!(!file.contains(forbidden));
    }
    // Repeating the command never creates a second ticket.
    let repeated = cli_named(&home, &worker, &create);
    assert_eq!(repeated["created"], false);
    assert_eq!(repeated["ticket"]["native_id"], native_id);
    assert_eq!(peer_store.list_tickets().unwrap().len(), 1);
    let target_args = [
        "--project",
        &owner.id,
        "--target",
        TARGET,
        "--connection",
        &peer_connection,
    ];
    let with = |command: &'static str, extra: &[&str]| {
        let mut arguments = vec!["ticket", command];
        arguments.extend_from_slice(&target_args);
        arguments.extend_from_slice(extra);
        arguments.into_iter().map(str::to_owned).collect::<Vec<_>>()
    };
    let run = |prefix: &[&str], arguments: &[String]| {
        cli_named(
            &home,
            prefix,
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    };
    let fail = |prefix: &[&str], arguments: &[String]| {
        cli_failure(
            &home,
            prefix,
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    };
    let shown = run(&worker, &with("show", &["--id", &native_id]));
    assert_eq!(shown["details"], "Created through the reviewed transport");
    let updated = run(
        &worker,
        &with(
            "update",
            &[
                "--id",
                &native_id,
                "--status",
                "started",
                "--up-next",
                "true",
            ],
        ),
    );
    assert_eq!(updated["status"], "started");
    assert!(peer_store.read_ticket(&on_peer[0].id).unwrap().up_next);
    assert!(
        fail(
            &worker,
            &with("show", &["--id", "01ARZ3NDEKTSV4RRFFQ69G5FZZ"])
        )
        .contains("target_ticket_not_found")
    );
    let links = cli_named(
        &home,
        &worker,
        &[
            "ticket",
            "links",
            "--project",
            &owner.id,
            "--ticket",
            &orchestration.slug,
            "--resolve",
        ],
    );
    assert_eq!(links[0]["state"], "resolved");
    assert_eq!(links[0]["relation"], "created");
    assert_eq!(links[0]["backlink"], true);
    assert_eq!(links[0]["summary"]["status"], "started");
    // The peer narrows the pairing to read-only: reads still work, writes are refused
    // by the target server itself whatever the grant says.
    set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadOnly).unwrap();
    assert_eq!(
        run(&worker, &with("show", &["--id", &native_id]))["status"],
        "started"
    );
    assert!(
        fail(
            &worker,
            &with("update", &["--id", &native_id, "--title", "Refused"])
        )
        .contains("target_provider_unauthorized")
    );
    assert_eq!(
        peer_store.read_ticket(&on_peer[0].id).unwrap().title,
        "Synthetic remote work"
    );
    // The journal records each authorized action and how it ended, by code only.
    let journal = cli_named(
        &home,
        &["-C", store_arg, "orchestration"],
        &["audit", "--project", &owner.id, "--limit", "100"],
    );
    let text = journal.to_string();
    assert!(text.contains("\"operation_id\":\"op-remote-1\""));
    assert!(text.contains("target_provider_unauthorized"));
    assert!(!text.contains("Synthetic remote work"));
    // Unlinking is the controller's own write; the peer's ticket is not touched.
    let unlinked = cli_named(
        &home,
        &worker,
        &[
            "ticket",
            "unlink",
            "--project",
            &owner.id,
            "--ticket",
            &orchestration.slug,
            "--target",
            TARGET,
            "--connection",
            &peer_connection,
            "--id",
            &native_id,
        ],
    );
    assert_eq!(unlinked, json!([]));
    assert_eq!(peer_store.list_tickets().unwrap().len(), 1);
    peer.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_az15ga_hosted_and_orchestrating_routes_on_a_live_service() {
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
    let router = app(host);
    let hosted = format!("/checkouts/{}/target-tickets", checkout.id);
    let create = json!({"connection_id":connection,"operation_id":"op-1",
        "source":{"connection_id":"owner-source","native_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"},
        "title":"Hosted over HTTP"});
    for (method, body) in [
        ("GET", Value::Null),
        ("POST", create.clone()),
        ("PATCH", json!({})),
    ] {
        assert_eq!(
            api(router.clone(), method, &hosted, body, false).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, created) = api(router.clone(), "POST", &hosted, create.clone(), true).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["created"], true);
    let native_id = created["ticket"]["native_id"].as_str().unwrap().to_owned();
    let (status, again) = api(router.clone(), "POST", &hosted, create, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["created"], false);
    assert_eq!(again["ticket"]["native_id"], native_id);
    let (status, read) = api(
        router.clone(),
        "GET",
        &format!("{hosted}?connection_id={connection}&native_id={native_id}"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["ticket"]["title"], "Hosted over HTTP");
    // Errors carry the stable code a remote orchestrator classifies by.
    let (status, missing) = api(
        router.clone(),
        "GET",
        &format!("{hosted}?connection_id={connection}&native_id=01ARZ3NDEKTSV4RRFFQ69G5FZZ"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["code"], "target_ticket_not_found");
    let (status, unknown) = api(
        router.clone(),
        "GET",
        &format!("{hosted}?connection_id=not-a-source&native_id={native_id}"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(unknown["code"], "target_ticket_not_found");
    let (status, conflict) = api(
        router.clone(),
        "POST",
        &hosted,
        json!({"connection_id":connection,"operation_id":"op-1",
            "source":{"connection_id":"owner-source","native_id":"01ARZ3NDEKTSV4RRFFQ69G5FAW"},
            "title":"Another origin"}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["code"], "target_conflict");
    let (status, patched) = api(
        router.clone(),
        "PATCH",
        &hosted,
        json!({"connection_id":connection,"native_id":native_id,"title":"Renamed"}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(patched["ticket"]["title"], "Renamed");
    assert_eq!(
        api(
            router.clone(),
            "PATCH",
            &hosted,
            json!({"connection_id":connection}),
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // The orchestrating half is served on the same service and refuses without a grant.
    let (status, refused) = api(
        router,
        "POST",
        &format!("/checkouts/{}/orchestration/tickets/read", checkout.id),
        json!({"target_id":TARGET,"connection_id":connection,"native_id":native_id,
            "actor":{"role":"ai","id":"worker-1"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "target_unavailable");
    assert_eq!(
        FsStore::open(Path::new(&source))
            .unwrap()
            .list_tickets()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_az15ga_stdio_mcp_creates_and_resolves_target_tickets() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    // One checkout is both the orchestration project and its own local target, which
    // is a valid destination and keeps the fixture to one store.
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let connection = checkout.sources[0].connection_id.clone();
    let store = FsStore::open(&source).unwrap();
    let orchestration = ops::create(
        &store,
        hotsheet_model::Ulid::new(),
        "TST",
        "2026-10-15T00:00:00Z".into(),
        NewTicket {
            title: "Coordinate through MCP".into(),
            ..Default::default()
        },
    )
    .unwrap();
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
    let bound = api(
        router.clone(),
        "PUT",
        &format!("{base}/targets/{TARGET}/binding"),
        json!({"target":{"owner_checkout_id":checkout.id,"target_id":TARGET},
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
    let (status, _) = super::orchestration::remember_rules(
        router,
        &format!("{base}/targets/{TARGET}"),
        json!({"actor_id":"worker-1","actions":["tickets_read","tickets_write"],
            "expires_at_ms":expires,"actor":{"role":"human","id":"maintainer"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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
    let create = json!({"action":"ticket_create","checkout":checkout.id,"target_id":TARGET,
        "ticket":{"native_id":orchestration.slug},"connection_id":connection,
        "operation_id":"op-mcp-1","fields":{"title":"From the MCP tool","tags":["mcp"]},
        "actor_role":"ai","actor_id":"worker-1"});
    let created = mcp_named(&home, &backend, "hotsheet_targets", create.clone());
    assert_eq!(created["created"], true);
    assert_eq!(created["link"]["recorded"], true);
    let native_id = created["ticket"]["native_id"].as_str().unwrap().to_owned();
    assert_eq!(
        mcp_named(&home, &backend, "hotsheet_targets", create)["created"],
        false
    );
    assert_eq!(store.list_tickets().unwrap().len(), 2);
    let read = mcp_named(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"ticket_read","checkout":checkout.id,"target_id":TARGET,
            "connection_id":connection,"native_id":native_id,
            "actor_role":"ai","actor_id":"worker-1"}),
    );
    assert_eq!(read["title"], "From the MCP tool");
    let updated = mcp_named(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"ticket_update","checkout":checkout.id,"target_id":TARGET,
            "connection_id":connection,"native_id":native_id,"fields":{"priority":"low"},
            "actor_role":"ai","actor_id":"worker-1"}),
    );
    assert_eq!(updated["priority"], "low");
    let links = mcp_named(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"ticket_links","checkout":checkout.id,
            "ticket":{"native_id":orchestration.slug},"resolve":true,
            "actor_role":"ai","actor_id":"worker-1"}),
    );
    assert_eq!(links[0]["state"], "resolved");
    assert_eq!(links[0]["operation_id"], "op-mcp-1");
    assert_eq!(links[0]["backlink"], true);
    // A call without an identity is malformed.
    for arguments in [
        json!({"action":"ticket_read","checkout":checkout.id,"target_id":TARGET,
            "connection_id":connection,"native_id":native_id}),
        json!({"action":"ticket_delete","checkout":checkout.id,"target_id":TARGET}),
    ] {
        mcp_rejected(&home, &backend, "hotsheet_targets", arguments);
    }
    let unlinked = mcp_named(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"ticket_unlink","checkout":checkout.id,"target_id":TARGET,
            "ticket":{"native_id":orchestration.slug},"connection_id":connection,
            "operation_id":"op-mcp-1"}),
    );
    assert_eq!(unlinked, json!([]));
    assert_eq!(store.list_tickets().unwrap().len(), 2);
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}
