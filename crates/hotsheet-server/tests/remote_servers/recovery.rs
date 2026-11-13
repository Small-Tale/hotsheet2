//! HS2-WBREDM: reconciling and controlling runs through the owned CLI process, a
//! server that starts with unfinished runs on disk, and the stdio MCP tool on a live
//! service. Synthetic stores and identities only.
use super::orchestration::{api, shared_runtime};
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FE1";
const NOW: &str = "2026-10-15T00:00:00Z";
const LEASE: &str = "2100-01-01T00:00:00Z";
const NOT_WIRED: [&str; 2] = ["deployment", "session"];

fn ticket(store: &FsStore, title: &str) -> hotsheet_model::Ticket {
    ops::create(
        store,
        hotsheet_model::Ulid::new(),
        "TST",
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}
fn kinds(page: &Value) -> Vec<String> {
    page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["kind"].as_str().unwrap().to_owned())
        .collect()
}

/// The bare CLI controls a run, then stops with two runs unfinished. The world moves
/// on while nothing is running, and a server that starts afterwards reconciles both
/// runs from its own start-up, with nobody asking it to.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_wbredm_cli_controls_a_run_and_a_starting_server_reconciles_what_was_left() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let source = root.path().join("store");
    let store = FsStore::init_git(&source, &StoreMetadata::new("TST")).unwrap();
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let owner = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let connection = owner.sources[0].connection_id.clone();
    let orchestration = ticket(&store, "Coordinate the synthetic recovery");
    let direct = ticket(&store, "Synthetic direct work");
    let pooled = ticket(&store, "Synthetic pooled work");
    let store_arg = source.to_str().unwrap();
    let targets = [
        "-C",
        store_arg,
        "orchestration",
        "target",
        "--project",
        &owner.id,
    ];
    let inspected = cli_named(&home, &targets, &["inspect", &owner.id]);
    let added = cli_named(
        &home,
        &targets,
        &["add", "--target-id", TARGET, "--label", "Self"],
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
    cli_named(
        &home,
        &human,
        &[
            "grant",
            "add",
            TARGET,
            "--project",
            &owner.id,
            "--actor",
            "scheduler-1",
            "--action",
            "tickets_read",
            "--action",
            "tickets_write",
            "--action",
            "work_start",
            "--expires-in",
            "1h",
        ],
    );
    let dispatch = |ticket: &str, operation: &str, extra: &[&str]| {
        let mut arguments = vec![
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
            ticket,
            "--operation-id",
            operation,
        ];
        arguments.extend_from_slice(extra);
        cli_named(&home, &worker, &arguments)
    };
    let run = dispatch(&direct.slug, "op-direct", &["--worker", "worker-1"]);
    assert_eq!(run["state"], "dispatched");
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    let on_run = |operation: &'static str, extra: &[&str]| -> Vec<String> {
        ["run", operation, &run_id, "--project", &owner.id]
            .iter()
            .chain(extra)
            .map(|part| (*part).to_owned())
            .collect()
    };
    let ok = |prefix: &[&str], arguments: Vec<String>| {
        cli_named(
            &home,
            prefix,
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    };
    let fail = |prefix: &[&str], arguments: Vec<String>| {
        cli_failure(
            &home,
            prefix,
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    };
    // Reconcile: the claim is verified and exactly the sources nobody reports yet
    // are named as unverified.
    let reconciled = ok(&worker, on_run("reconcile", &[]));
    assert_eq!(reconciled["state"], "dispatched");
    assert_eq!(reconciled["reconciliation"]["disposition"], "continue");
    assert_eq!(reconciled["unverified"], json!(NOT_WIRED));
    // Pause, then resume: the target is not told, and a paused run is not renewed.
    let paused = ok(&worker, on_run("pause", &["--reason", "maintenance"]));
    assert_eq!(paused["paused"]["reason"], "maintenance");
    assert_eq!(paused["paused"]["actor_id"], "scheduler-1");
    assert!(fail(&worker, on_run("renew", &[])).contains("run_paused"));
    assert_eq!(
        store.read_ticket(&direct.id).unwrap().claimed_by.as_deref(),
        Some("worker-1")
    );
    let resumed = ok(&worker, on_run("resume", &[]));
    assert_eq!(resumed["paused"], Value::Null);
    assert_eq!(resumed["reconciliation"]["disposition"], "continue");
    // Handoff moves the claim to the new worker.
    let handed = ok(
        &worker,
        on_run(
            "handoff",
            &["--to-worker", "worker-2", "--lease-minutes", "20"],
        ),
    );
    assert_eq!(handed["worker_id"], "worker-2");
    assert_eq!(
        store.read_ticket(&direct.id).unwrap().claimed_by.as_deref(),
        Some("worker-2")
    );
    // A correction is a person's action.
    let correction = ["--state", "intervened", "--reason", "operator_fix"];
    assert!(fail(&worker, on_run("correct", &correction)).contains("forbidden"));
    let corrected = ok(&human, on_run("correct", &correction));
    assert_eq!(corrected["state"], "intervened");
    // Reconciling everything reads the target again, and what it shows wins: the new
    // worker holds a live claim, so the run is dispatched after all.
    let all = cli_named(
        &home,
        &worker,
        &["run", "reconcile", "--project", &owner.id],
    );
    assert_eq!(all["runs"].as_array().unwrap().len(), 1);
    assert_eq!(all["runs"][0]["state"], "dispatched");
    let history: Vec<_> = all["runs"][0]["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["to"].as_str().unwrap().to_owned(),
                entry["control"].as_str().unwrap_or("-").to_owned(),
            )
        })
        .collect();
    let expected = [
        ("requested", "-"),
        ("dispatched", "-"),
        ("dispatched", "pause"),
        ("dispatched", "resume"),
        ("dispatched", "handoff"),
        ("intervened", "correct"),
        ("dispatched", "-"),
    ]
    .map(|(to, control)| (to.to_owned(), control.to_owned()));
    assert_eq!(history, expected);
    let stream = cli_named(
        &home,
        &worker,
        &["run", "events", "--project", &owner.id, "--run", &run_id],
    );
    for kind in ["paused", "resumed", "handoff", "corrected", "reconciled"] {
        assert!(kinds(&stream).contains(&kind.to_owned()), "{kind}");
    }
    // The records hold identifiers and codes only.
    let record = std::fs::read_to_string(home.join("orchestration-runs.json")).unwrap();
    assert!(!record.contains("Synthetic direct work"));
    // A second run waits for the target's own dispatcher.
    let queued = dispatch(&pooled.slug, "op-pooled", &["--via", "dispatcher"]);
    assert_eq!(queued["state"], "queued");
    let queued_id = queued["run_id"].as_str().unwrap().to_owned();
    let cursor = cli_named(&home, &worker, &["run", "events", "--project", &owner.id])["cursor"]
        .as_u64()
        .unwrap();

    // Nothing is running now. Meanwhile a person releases the first ticket and the
    // target's dispatcher assigns the second.
    ops::release(&store, &direct.id, NOW.into(), "worker-2", false).unwrap();
    ops::claim_with_eta(
        &store,
        &pooled.id,
        &NOW.into(),
        LEASE.into(),
        "pool-1",
        None,
        None,
    )
    .unwrap();

    // A server starts on that home. Its start-up alone follows and reconciles the
    // unfinished runs; no request is made to it.
    let host = state(&source, &home);
    host.spawn_run_recovery();
    let waiter = shared_runtime(&source, &home, &Arc::new(Memory::default()));
    let mut after = cursor;
    let mut outcomes = std::collections::BTreeMap::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline && outcomes.len() < 2 {
        // Each read waits for the next event; this loop is cursor replay, not a timer.
        let (waiter, owner_id) = (waiter.clone(), owner.id.clone());
        let page = tokio::task::spawn_blocking(move || {
            waiter.run_events(&owner_id, after, 100, None, 10_000)
        })
        .await
        .unwrap()
        .unwrap();
        after = page.cursor;
        for entry in page.entries {
            let record = serde_json::to_value(&entry.record).unwrap();
            if record["kind"] == "reconciled" {
                outcomes.insert(
                    record["run_id"].as_str().unwrap().to_owned(),
                    record["reason"].as_str().unwrap().to_owned(),
                );
            }
        }
    }
    assert_eq!(
        outcomes.get(&run_id).map(String::as_str),
        Some("needs_attention"),
        "the starting server never reconciled the released run: {outcomes:?}"
    );
    assert_eq!(
        outcomes.get(&queued_id).map(String::as_str),
        Some("continue"),
        "{outcomes:?}"
    );
    // The status view answers from the record alone.
    let status = |id: &str| {
        cli_named(
            &home,
            &worker,
            &["run", "status", id, "--project", &owner.id],
        )
    };
    let released = status(&run_id);
    assert_eq!(released["state"], "intervened");
    assert_eq!(released["reachability"], "reachable");
    assert_eq!(released["reconciliation"]["disposition"], "needs_attention");
    let assigned = status(&queued_id);
    assert_eq!(assigned["state"], "dispatched");
    assert_eq!(assigned["worker_id"], "pool-1");
    // Recovery claimed nothing and queued nothing a second time.
    assert_eq!(store.read_ticket(&pooled.id).unwrap().claim_count, 1);
    assert!(store.read_ticket(&direct.id).unwrap().claimed_by.is_none());
    host.begin_stopping();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_wbredm_stdio_mcp_reconciles_and_controls_a_run() {
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
    let orchestration = ticket(&store, "Coordinate through MCP");
    let work = ticket(&store, "Synthetic work to control");
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
    api(
        router.clone(),
        "POST",
        &format!("{base}/targets/{TARGET}/grants"),
        json!({"actor_id":"scheduler-1","actions":["tickets_read","work_start"],
            "expires_at_ms":expires,"actor":{"role":"human","id":"maintainer"}}),
        true,
    )
    .await;
    // The recovery routes need the bearer like everything else.
    for path in [
        format!("{base}/run-reconcile"),
        format!("{base}/runs/{TARGET}/reconcile"),
        format!("{base}/runs/{TARGET}/pause"),
        format!("{base}/runs/{TARGET}/correct"),
    ] {
        assert_eq!(
            api(router.clone(), "POST", &path, json!({}), false).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    // With no unfinished run there is nothing to reconcile.
    let (status, empty) = api(
        router.clone(),
        "POST",
        &format!("{base}/run-reconcile"),
        json!({"actor":{"role":"ai","id":"scheduler-1"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty, json!({"runs":[]}));
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
    let as_actor = |role: &str, id: &str, mut arguments: Value| {
        arguments["checkout"] = json!(checkout.id);
        arguments["actor_role"] = json!(role);
        arguments["actor_id"] = json!(id);
        arguments
    };
    let call = |arguments: Value| {
        mcp_named(
            &home,
            &backend,
            "hotsheet_targets",
            as_actor("ai", "scheduler-1", arguments),
        )
    };
    let run = call(json!({"action":"run_dispatch","target_id":TARGET,
        "ticket":{"native_id":orchestration.slug},"connection_id":connection,
        "native_id":work.slug,"operation_id":"op-mcp-recovery","worker_id":"mcp-worker"}));
    assert_eq!(run["state"], "dispatched");
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    let reconciled = call(json!({"action":"run_reconcile","run_id":run_id}));
    assert_eq!(
        reconciled["reconciliation"]["disposition"], "continue",
        "{reconciled}"
    );
    assert_eq!(reconciled["unverified"], json!(NOT_WIRED));
    assert_eq!(
        reconciled["reconciliation"]["findings"][2],
        json!({"source":"target_claim","verification":"verified"})
    );
    let paused = call(json!({"action":"run_pause","run_id":run_id,"reason":"maintenance"}));
    assert_eq!(paused["paused"]["reason"], "maintenance");
    mcp_rejected(
        &home,
        &backend,
        "hotsheet_targets",
        as_actor(
            "ai",
            "scheduler-1",
            json!({"action":"run_renew","run_id":run_id}),
        ),
    );
    // A paused run is read but reported as paused, whatever the target shows.
    assert_eq!(
        call(json!({"action":"run_reconcile"}))["runs"][0]["reconciliation"]["disposition"],
        "paused"
    );
    let resumed = call(json!({"action":"run_resume","run_id":run_id}));
    assert_eq!(resumed["paused"], Value::Null);
    assert_eq!(resumed["reconciliation"]["disposition"], "continue");
    let handed = call(json!({"action":"run_handoff","run_id":run_id,"to_worker":"mcp-worker-2"}));
    assert_eq!(handed["worker_id"], "mcp-worker-2");
    assert_eq!(
        store.read_ticket(&work.id).unwrap().claimed_by.as_deref(),
        Some("mcp-worker-2")
    );
    // An AI cannot correct the run it supervises; a person can.
    let correction = json!({"action":"run_correct","run_id":run_id,"state":"cancelled",
        "reason":"abandoned"});
    mcp_rejected(
        &home,
        &backend,
        "hotsheet_targets",
        as_actor("ai", "scheduler-1", correction.clone()),
    );
    let corrected = mcp_named(
        &home,
        &backend,
        "hotsheet_targets",
        as_actor("human", "maintainer", correction),
    );
    assert_eq!(corrected["state"], "cancelled");
    // Only this machine's record changed: the target still shows the claim.
    assert_eq!(
        store.read_ticket(&work.id).unwrap().claimed_by.as_deref(),
        Some("mcp-worker-2")
    );
    let stream = call(json!({"action":"run_events","run_id":run_id,"after":0}));
    for kind in ["paused", "resumed", "handoff", "corrected", "reconciled"] {
        assert!(kinds(&stream).contains(&kind.to_owned()), "{kind}");
    }
    assert_eq!(call(json!({"action":"run_reconcile"})), json!({"runs":[]}));
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}
