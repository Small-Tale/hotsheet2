//! HS2-ECWG50: the scheduler through the owned CLI process, the authenticated routes
//! of a real server with its event-driven loop, and the stdio MCP tool on a live
//! service. Synthetic stores, tools and identities only.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FF2";
const NOW: &str = "2026-10-15T00:00:00Z";
/// A tool no machine has: a start for it is refused the same way everywhere, so the
/// answers do not depend on the machine the test runs on.
const TOOL: &str = "synthetic-tool";

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
fn policy(automatic: bool, enabled: bool) -> Value {
    json!({
        "tools": [{"tool": TOOL, "max_sessions": 2}],
        "targets": [{"target_id": TARGET, "enabled": enabled,
            "actions": ["tickets_read", "gates_observe", "sessions_manage"],
            "tools": [{"tool": TOOL}], "max_sessions": 2}],
        "placement": {"automatic": automatic, "actor": "scheduler-1"}
    })
}
fn kinds(page: &Value) -> Vec<(String, String)> {
    page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["trigger"].as_str().unwrap().to_owned(),
                entry["kind"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// The bare CLI, with no server running: plan, explain, tick and the decision log,
/// dry by default and applying only under an automatic policy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_ecwg50_cli_plans_explains_ticks_and_reads_the_decision_log() {
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
    let orchestration = ticket(&store, "Coordinate the synthetic schedule");
    let work = ticket(&store, "Synthetic ready work");
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
    // HS2-XHQ1EH: reading and observing need no rule.
    let scheduler = |arguments: &[&str]| {
        let mut all = vec!["scheduler"];
        all.extend_from_slice(arguments);
        all.extend_from_slice(&["--project", &owner.id]);
        cli_named(&home, &worker, &all)
    };
    let set_policy = |body: Value| {
        let file = root.path().join("policy.json");
        std::fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
        cli_named(
            &home,
            &human,
            &[
                "policy",
                "set",
                "--project",
                &owner.id,
                "--file",
                file.to_str().unwrap(),
            ],
        )
    };
    let step = cli_named(
        &home,
        &worker,
        &[
            "readiness",
            "define",
            "--ticket",
            &orchestration.slug,
            "--target",
            TARGET,
            "--connection",
            &connection,
            "--id",
            &work.slug,
            "--project",
            &owner.id,
        ],
    );
    assert_eq!(step["level"], "confirmed", "{step}");
    let step_id = step["step"]["step_id"].as_str().unwrap().to_owned();

    // No policy: nothing is scheduled, and nothing is logged.
    let plan = scheduler(&["plan"]);
    assert_eq!(plan["enabled"], false);
    assert_eq!(plan["placements"], json!([]));
    assert_eq!(scheduler(&["tick"])["logged"], 0);
    assert_eq!(scheduler(&["decisions"])["entries"], json!([]));

    // A policy that does not enable automatic placement: the plan ranks the work
    // and says why it is not placed. With no server on this machine its capacity is
    // unknown, and nothing is assumed in its place.
    let stored = set_policy(policy(false, true));
    assert_eq!(stored["policy"]["placement"]["automatic"], false);
    assert_eq!(stored["policy"]["placement"]["aging_minutes"], 60);
    let plan = scheduler(&["plan"]);
    assert_eq!(
        (&plan["enabled"], &plan["mode"]),
        (&json!(true), &json!("dry_run"))
    );
    assert_eq!(plan["ranking"][0]["step_id"], step_id);
    assert_eq!(plan["ranking"][0]["score"]["priority"], 2000);
    assert_eq!(plan["placements"], json!([]));
    assert_eq!(plan["unplaced"][0]["reason"], "machine_refused", "{plan}");
    assert_eq!(plan["unplaced"][0]["detail"], "capacity_unknown");
    // HS2-95SWQ4: a headless process has the executor too; it has nothing to list.
    assert_eq!(plan["inputs"]["executor"], "available");
    assert_eq!(plan["inputs"]["sessions"], 0);
    assert_eq!(
        scheduler(&["decisions"])["entries"],
        json!([]),
        "a plan logs nothing"
    );

    // Explain, by step and by the exact ticket.
    let by_step = scheduler(&["explain", &step_id]);
    assert_eq!(
        (
            &by_step["running"],
            &by_step["standing"],
            &by_step["reason"]
        ),
        (
            &json!(false),
            &json!("not_placed"),
            &json!("machine_refused")
        )
    );
    let canonical = work.id.to_string();
    let by_ticket = scheduler(&[
        "explain",
        "--target",
        TARGET,
        "--connection",
        &connection,
        "--id",
        &canonical,
    ]);
    assert_eq!(by_ticket["step_id"], step_id);
    assert_eq!(by_ticket["detail"], "capacity_unknown");
    let unplanned = scheduler(&[
        "explain",
        "--target",
        TARGET,
        "--connection",
        &connection,
        "--id",
        "no-such-ticket",
    ]);
    assert_eq!(unplanned["standing"], "not_planned");
    for bad in [
        vec!["scheduler", "explain", "--project", &owner.id],
        vec![
            "scheduler",
            "explain",
            &step_id,
            "--target",
            TARGET,
            "--project",
            &owner.id,
        ],
        vec![
            "scheduler",
            "decisions",
            "--limit",
            "0",
            "--project",
            &owner.id,
        ],
    ] {
        cli_failure(&home, &worker, &bad);
    }

    // A tick is a dry run that logs: one record for the pass, one per decision.
    let tick = scheduler(&["tick"]);
    assert_eq!(
        (&tick["mode"], &tick["logged"]),
        (&json!("dry_run"), &json!(2))
    );
    assert_eq!(tick["outcomes"], json!([]));
    let log = scheduler(&["decisions"]);
    assert_eq!(
        kinds(&log),
        [
            ("manual".into(), "tick".into()),
            ("manual".into(), "unplaced".into())
        ]
    );
    assert_eq!(log["entries"][0]["tick"]["automatic"], false);
    assert_eq!(log["entries"][0]["actor_id"], "scheduler-1");
    assert_eq!(log["entries"][1]["step_id"], step_id);
    assert_eq!(log["entries"][1]["reason"], "machine_refused");
    assert_eq!(log["entries"][1]["mode"], "dry_run");
    let cursor = log["cursor"].to_string();
    assert_eq!(
        scheduler(&["decisions", "--after", &cursor])["entries"],
        json!([])
    );
    assert_eq!(
        scheduler(&["decisions", "--limit", "1"])["more"],
        json!(true)
    );
    assert!(!home.join("orchestration-scheduler.json").exists());

    // Automatic placement: the pass is applied. The machine could not be read, so
    // nothing is started, and the target is left alone for a bounded time.
    set_policy(policy(true, true));
    let applied = scheduler(&["tick"]);
    assert_eq!(applied["mode"], "applied");
    assert_eq!(applied["outcomes"], json!([]));
    let backed_off = scheduler(&["plan"]);
    assert_eq!(backed_off["unplaced"][0]["reason"], "refusal_backoff");
    assert_eq!(backed_off["unplaced"][0]["detail"], "capacity_unknown");
    assert!(backed_off["retry_at_ms"].as_u64().unwrap() > 0);
    // An explicit dry run applies nothing whatever the policy says.
    assert_eq!(scheduler(&["tick", "--dry-run"])["mode"], "dry_run");

    // The policy disables the target: the reason changes with it.
    set_policy(policy(true, false));
    let denied = scheduler(&["explain", &step_id]);
    assert_eq!(
        (&denied["reason"], &denied["detail"]),
        (&json!("policy_denied"), &json!("policy_target_disabled"))
    );
}

/// One authenticated request with an owned path, so a closure can hand back its future.
async fn call(
    router: axum::Router,
    method: &'static str,
    path: String,
    body: Value,
) -> (StatusCode, Value) {
    api(router, method, &path, body, true).await
}

/// Waits, woken by the decision log itself changing, for a complete pass of one
/// trigger that read `work` pieces of work, and returns its records, the pass first.
async fn logged(
    router: axum::Router,
    base: &str,
    home: &std::path::Path,
    trigger: &str,
    work: u64,
) -> Vec<Value> {
    let directory = home.join("orchestration-scheduler-decisions");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let waiter = hotsheet_jobs::ChangeWaiter::new(&directory).unwrap();
    loop {
        let page = api(
            router.clone(),
            "POST",
            &format!("{base}/scheduler/decisions"),
            json!({"limit": 200}),
            true,
        )
        .await
        .1;
        let entries = page["entries"].as_array().unwrap();
        let pass = entries.iter().find(|entry| {
            entry["trigger"] == trigger && entry["kind"] == "tick" && entry["tick"]["work"] == work
        });
        if let Some(pass) = pass {
            let records: Vec<Value> = entries
                .iter()
                .filter(|entry| entry["tick_id"] == pass["tick_id"])
                .cloned()
                .collect();
            // A pass writes one record for itself and one per decision.
            let decided: u64 = ["starts", "keeps", "wind_downs", "unplaced"]
                .iter()
                .map(|key| pass["tick"][key].as_u64().unwrap())
                .sum();
            if records.len() as u64 == decided + 1 {
                return records;
            }
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "no complete {trigger} pass: {page}");
        tokio::task::block_in_place(|| waiter.wait(remaining));
    }
}

/// The authenticated routes, the server's own loop for an automatic policy, and the
/// stdio MCP tool against the live service.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_ecwg50_server_routes_event_loop_and_stdio_mcp_share_the_scheduler() {
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
    let orchestration = ticket(&store, "Coordinate through the server");
    let first = ticket(&store, "Synthetic ready work");
    let second = ticket(&store, "Synthetic later work");
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
        json!({"actor_id":"scheduler-1","actions":["tickets_read","gates_observe"],
            "expires_at_ms":expires,"actor":{"role":"human","id":"maintainer"}}),
    )
    .await;
    // Every scheduler route needs the bearer.
    for operation in ["plan", "explain", "decisions", "tick"] {
        assert_eq!(
            api(
                router.clone(),
                "POST",
                &format!("{base}/scheduler/{operation}"),
                json!({}),
                false
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED,
            "{operation}"
        );
    }
    let actor = json!({"role":"ai","id":"scheduler-1"});
    let ticket_ref = json!({"native_id": orchestration.slug});
    let define = |native: String| {
        call(
            router.clone(),
            "POST",
            format!("{base}/readiness/define"),
            json!({"actor":actor,"ticket":ticket_ref,"target_id":TARGET,
                "connection_id":connection,"native_id":native}),
        )
    };
    let (status, step) = define(first.slug.clone()).await;
    assert_eq!(status, StatusCode::OK, "{step}");
    let step_id = step["step"]["step_id"].as_str().unwrap().to_owned();
    let send = |operation: &'static str, body: Value| {
        call(
            router.clone(),
            "POST",
            format!("{base}/scheduler/{operation}"),
            body,
        )
    };
    let put_policy = |body: Value, revision: Value| {
        call(
            router.clone(),
            "PUT",
            format!("{base}/policy"),
            json!({"expected_revision":revision,"policy":body,
                "actor":{"role":"human","id":"maintainer"}}),
        )
    };

    // No policy: the scheduler is off and says so.
    let (status, plan) = send("plan", json!({"actor":actor})).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["enabled"], false);

    // A policy without automatic placement: reads and a dry tick, and no loop.
    let (status, stored) = put_policy(policy(false, true), Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    let (status, plan) = send("plan", json!({"actor":actor})).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["mode"], "dry_run");
    assert_eq!(plan["ranking"][0]["step_id"], step_id);
    // The server measured its own machine; no machine has the synthetic tool.
    assert_eq!(plan["inputs"]["targets_unavailable"], 0, "{plan}");
    assert_eq!(plan["unplaced"][0]["reason"], "no_tool_available", "{plan}");
    assert_eq!(plan["unplaced"][0]["detail"], "tool_not_installed");
    let (_, explained) = send("explain", json!({"actor":actor,"step_id":step_id})).await;
    assert_eq!(explained["standing"], "not_placed");
    assert_eq!(explained["reason"], "no_tool_available");
    let (status, tick) = send("tick", json!({"actor":actor})).await;
    assert_eq!(status, StatusCode::OK, "{tick}");
    assert_eq!(
        (&tick["mode"], &tick["logged"]),
        (&json!("dry_run"), &json!(2))
    );
    // Malformed bodies and an unknown operation are refused.
    for (operation, body, expected) in [
        ("plan", json!({"extra": true}), StatusCode::BAD_REQUEST),
        ("explain", json!({}), StatusCode::BAD_REQUEST),
        ("explain", json!({"step": step_id}), StatusCode::BAD_REQUEST),
        ("decisions", json!({"limit": 0}), StatusCode::BAD_REQUEST),
        ("tick", json!({"dry": true}), StatusCode::BAD_REQUEST),
        ("apply", json!({}), StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(send(operation, body).await.0, expected, "{operation}");
    }
    let (_, log) = send("decisions", json!({})).await;
    assert_eq!(
        kinds(&log),
        [
            ("manual".into(), "tick".into()),
            ("manual".into(), "unplaced".into())
        ],
        "no loop ran: {log}"
    );

    // The policy enables automatic placement: the server starts the owner's loop,
    // whose first pass acts as the policy's actor.
    let (status, enabled) = put_policy(policy(true, true), stored["revision"].clone()).await;
    assert_eq!(status, StatusCode::OK, "{enabled}");
    let started = logged(router.clone(), &base, &home, "start", 1).await;
    assert_eq!(started[0]["mode"], "applied");
    assert_eq!(started[0]["actor_id"], "scheduler-1");
    assert_eq!(started[0]["tick"]["automatic"], true);
    assert_eq!(started[1]["kind"], "unplaced");
    assert_eq!(started[1]["step_id"], step_id);
    // More work becomes ready: the readiness event is what makes the loop plan again.
    let (status, later) = define(second.slug.clone()).await;
    assert_eq!(status, StatusCode::OK, "{later}");
    let event = logged(router.clone(), &base, &home, "event", 2).await;
    assert_eq!(
        event
            .iter()
            .filter(|entry| entry["kind"] == "unplaced")
            .count(),
        2,
        "{event:?}"
    );

    // The stdio MCP tool reads the same schedule and log from the live service.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let serving = host.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, app(serving))
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
    let plan = call(json!({"action":"scheduler_plan"}));
    assert_eq!(plan["mode"], "applied");
    // HS2-95SWQ4: the service's scheduler has the executor over its own sessions.
    assert_eq!(plan["inputs"]["executor"], "available");
    assert_eq!(plan["ranking"].as_array().unwrap().len(), 2);
    let explained = call(json!({"action":"scheduler_explain","target_id":TARGET,
        "connection_id":connection,"native_id":first.id.to_string()}));
    assert_eq!(explained["step_id"], step_id);
    assert_eq!(explained["reason"], "no_tool_available");
    let dry = call(json!({"action":"scheduler_tick","dry_run":true}));
    assert_eq!(dry["mode"], "dry_run");
    let page = call(json!({"action":"scheduler_decisions","limit":2}));
    assert_eq!(page["entries"].as_array().unwrap().len(), 2);
    assert_eq!(page["more"], true);
    let mut refused = json!({"action":"scheduler_explain","checkout":checkout.id});
    refused["actor_role"] = json!("ai");
    refused["actor_id"] = json!("scheduler-1");
    mcp_rejected(&home, &backend, "hotsheet_targets", refused);

    // Automatic placement is turned off: the change wakes the loop, and it ends.
    let (status, _) = put_policy(policy(false, true), enabled["revision"].clone()).await;
    assert_eq!(status, StatusCode::OK);
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}

/// HS2-2FG91X: scheduling around claimable resources, on the same fixtures.
#[path = "resources.rs"]
mod resources;
