//! HS2-75Q254 with HS2-7QNK7Y, HS2-BYEB60 and HS2-G5H9FQ: a server restarts over a
//! home with an unfinished run that has a CI gate waiting on its watch, a validation
//! attempt whose host process is gone, and an orchestrator-owned session whose
//! terminal died with the old server. The new server's own start-up reconciles the
//! run, and each source is reported truthfully. The session is a real PTY running the
//! workspace fake agent; the CI watch is a Job nobody observes; no network.
use super::orchestration::{api, shared_runtime};
use super::sessions::{IDLE_TOOL, Roomy, fake_plugins, git_project};
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, ops};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FH3";
const NOW: &str = "2026-10-15T00:00:00Z";

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_75q254_a_restarted_server_reconciles_ci_validation_and_session_truthfully() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let plugins = fake_plugins(root.path());
    let first = state(&source, &home)
        .with_capacity_source(Box::new(Roomy))
        .with_capacity_cache(std::time::Duration::ZERO)
        .with_plugin_dirs(vec![plugins.clone()]);
    let project = root.path().join("project");
    git_project(&project);
    // The project's own validation profile: a step that takes longer than its host, and
    // ends when the test releases it.
    std::fs::create_dir_all(project.join(".hotsheet2")).unwrap();
    std::fs::write(
        project.join(".hotsheet2/validation-profiles.json"),
        json!({"schema_version": 1, "profiles": [
            {"name": "slow", "program": "/bin/sh", "args": ["-c",
                "while [ ! -f release ]; do sleep 0.05; done"]}]})
        .to_string(),
    )
    .unwrap();
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let connection = checkout.sources[0].connection_id.clone();
    let store = FsStore::open(&source).unwrap();
    let orchestration = ticket(&store, "Coordinate the restart");
    let work = ticket(&store, "Synthetic work across a restart");
    let router = app(first.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
    let person = json!({"role":"human","id":"maintainer"});
    let ai = json!({"role":"ai","id":"scheduler-1"});
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
    .await;
    assert_eq!(bound.0, StatusCode::OK, "{}", bound.1);
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 3_600_000;
    let actions = json!([
        "tickets_read",
        "work_start",
        "gates_observe",
        "validation_run",
        "sessions_manage"
    ]);
    let granted = super::orchestration::remember_rules(
        router.clone(),
        &format!("{base}/targets/{TARGET}"),
        json!({"actor_id":"scheduler-1","actions":actions,
            "session_scope":{"tools":[{"tool":IDLE_TOOL}],"max_concurrency":4},
            "expires_at_ms":expires,"actor":person}),
    )
    .await;
    assert_eq!(granted.0, StatusCode::OK, "{}", granted.1);
    let policy = json!({"machine":{"max_sessions":4},
        "tools":[{"tool":IDLE_TOOL,"max_sessions":4}],
        "targets":[{"target_id":TARGET,"actions":actions,"tools":[{"tool":IDLE_TOOL}],
            "max_sessions":2}]});
    let stored = api(
        router.clone(),
        "PUT",
        &format!("{base}/policy"),
        json!({"expected_revision":null,"policy":policy,"actor":person}),
        true,
    )
    .await;
    assert_eq!(stored.0, StatusCode::OK, "{}", stored.1);
    // The checked-in orchestration files are committed, so the work tree is clean.
    for arguments in [
        &["add", "-A"][..],
        &[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.test",
            "commit",
            "-q",
            "-m",
            "Declare the target, policy and profile",
        ],
    ] {
        let status = hotsheet_ticketing::git::launch("git")
            .arg("-C")
            .arg(&project)
            .args(arguments)
            .status()
            .unwrap();
        assert!(status.success(), "{arguments:?}");
    }
    // The run and its CI gate are recorded through the shared runtime on this machine,
    // so the first server starts no follower that would outlive it in this process.
    let shared = Arc::new(shared_runtime(&source, &home, &Arc::new(Memory::default())));
    let agent =
        hotsheet_ticketing::actor::MutationActor::parse("ai", Some("scheduler-1".into())).unwrap();
    let (run_id, gate_id) = {
        let (shared, owner, agent) = (shared.clone(), checkout.id.clone(), agent.clone());
        let (orchestration, work, connection) = (
            orchestration.slug.clone(),
            work.slug.clone(),
            connection.clone(),
        );
        tokio::task::spawn_blocking(move || {
            let run = shared
                .request_now(
                    "POST",
                    &format!("/checkouts/{owner}/orchestration/runs"),
                    &json!({"ticket":{"native_id":orchestration},"target_id":TARGET,
                        "connection_id":connection,"native_id":work,
                        "operation_id":"op-restart","worker_id":"worker-a",
                        "actor":{"role":"ai","id":agent.id}}),
                )
                .unwrap();
            assert_eq!(run["state"], "dispatched", "{run}");
            let run_id = run["run_id"].as_str().unwrap().to_owned();
            let linked = shared
                .request_now(
                    "POST",
                    &format!("/checkouts/{owner}/orchestration/runs/{run_id}/gates"),
                    &json!({"commit":"c".repeat(40),"repository":"example/widgets",
                        "actor":{"role":"ai","id":"scheduler-1"}}),
                )
                .unwrap();
            assert_eq!(linked["gate"]["state"], "pending", "{linked}");
            (
                run_id,
                linked["gate"]["gate_id"].as_str().unwrap().to_owned(),
            )
        })
        .await
        .unwrap()
    };
    // The first server starts the session for the run: a real terminal in its drawer.
    let (status, session) = api(
        router.clone(),
        "POST",
        &format!("{base}/sessions"),
        json!({"target_id":TARGET,"operation_id":"session-restart","tool":IDLE_TOOL,
            "run_id":run_id,"actor":ai}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["state"], "running");
    let session_id = session["session_id"].as_str().unwrap().to_owned();
    let terminal = session["hosted"]["session_id"].as_str().unwrap().to_owned();
    // A validation step of the run, started by a headless CLI that exits without
    // waiting: that host is gone once the command returns. HS2-M7EV18: the step runs in
    // its own detached runner, so it outlives that host and both servers.
    let store_arg = source.to_str().unwrap();
    let worker = [
        "--actor-role",
        "ai",
        "--actor-id",
        "scheduler-1",
        "-C",
        store_arg,
        "orchestration",
        "validation",
    ];
    cli_named(
        &home,
        &worker,
        &[
            "define",
            "--project",
            &checkout.id,
            "--ticket",
            &orchestration.slug,
            "--step",
            "slow",
            "--target",
            TARGET,
            "--profile",
            "slow",
            "--timeout-seconds",
            "60",
        ],
    );
    let started = cli_named(
        &home,
        &worker,
        &[
            "run",
            "--project",
            &checkout.id,
            "--ticket",
            &orchestration.slug,
            "--step",
            "slow",
            "--operation-id",
            "validate-restart",
            "--run",
            &run_id,
            "--no-wait",
        ],
    );
    assert_eq!(started["result"]["state"], "running", "{started}");
    assert_eq!(started["result"]["host_started"], true, "{started}");
    let result_id = started["result"]["result_id"].as_str().unwrap().to_owned();

    // The first server goes down, and its terminal with it.
    let closed = api(
        router.clone(),
        "DELETE",
        &format!("/terminals/{terminal}"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(closed.0, StatusCode::NO_CONTENT);
    first.begin_stopping();
    drop(router);
    let cursor = {
        let (shared, owner) = (shared.clone(), checkout.id.clone());
        tokio::task::spawn_blocking(move || shared.run_events(&owner, 0, 500, None, 0))
            .await
            .unwrap()
            .unwrap()
            .cursor
    };

    // A new server starts on the same home. Its start-up alone reconciles the run.
    let second = state(&source, &home)
        .with_capacity_source(Box::new(Roomy))
        .with_capacity_cache(std::time::Duration::ZERO)
        .with_plugin_dirs(vec![plugins]);
    second.spawn_run_recovery();
    let mut after = cursor;
    let mut reconciled = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while reconciled.is_none() && std::time::Instant::now() < deadline {
        // Each read waits for the next event; this loop is cursor replay, not a timer.
        let (shared, owner, run) = (shared.clone(), checkout.id.clone(), run_id.clone());
        let page = tokio::task::spawn_blocking(move || {
            shared.run_events(&owner, after, 100, Some(&run), 10_000)
        })
        .await
        .unwrap()
        .unwrap();
        after = page.cursor;
        reconciled = page
            .entries
            .iter()
            .map(|entry| serde_json::to_value(&entry.record).unwrap())
            .find(|record| record["kind"] == "reconciled");
    }
    let reconciled = reconciled.expect("the restarted server never reconciled the run");
    assert_eq!(reconciled["reason"], "continue", "{reconciled}");
    let run = {
        let (shared, owner, run) = (shared.clone(), checkout.id.clone(), run_id.clone());
        tokio::task::spawn_blocking(move || shared.run(&owner, &run))
            .await
            .unwrap()
            .unwrap()
    };
    let findings = serde_json::to_value(&run.reconciliation.as_ref().unwrap().findings).unwrap();
    assert_eq!(
        findings,
        json!([
            {"source":"target_reachability","verification":"verified"},
            {"source":"target_ticket","verification":"verified"},
            {"source":"target_claim","verification":"verified"},
            {"source":"run_lease","verification":"verified"},
            // The watch Job is still pending: the same watch the gate names.
            {"source":"ci_run","verification":"verified"},
            {"source":"deployment","verification":"verified","reason":"no_deployment"},
            // Its host process is gone, but its detached runner still runs it
            // (HS2-M7EV18): still running, never run again.
            {"source":"validation","verification":"verified"},
            // The terminal died with the old server while the run waits on CI: no work
            // was lost with it.
            {"source":"session","verification":"verified","reason":"waiting_on_gate"},
        ]),
        "{findings}"
    );
    assert_eq!(
        run.state,
        hotsheet_ticketing::orchestration::RunState::Dispatched
    );
    // The records say the same thing.
    let (shared_check, owner, records) = (shared.clone(), checkout.id.clone(), home.clone());
    let expected_result = result_id.clone();
    let (session_state, validation, gate) = tokio::task::spawn_blocking(move || {
        let session = shared_check.session(&owner, &session_id).unwrap();
        let validation = hotsheet_ticketing::orchestration::ValidationRegistry::new(&records)
            .steps(&owner, None, 0)
            .unwrap()
            .into_iter()
            .flat_map(|evidence| evidence.history)
            .find(|result| result.result_id == result_id)
            .unwrap();
        let gate = shared_check.gate(&owner, &gate_id).unwrap();
        (session.state, validation, gate)
    })
    .await
    .unwrap();
    assert_eq!(session_state.code(), "lost");
    assert_eq!(validation.state.code(), "running");
    assert!(validation.host_started);
    assert_eq!(gate.state.code(), "pending");
    // The step ends after both servers it outlived; the same request collects its
    // result from the runner's record.
    std::fs::write(project.join("release"), "").unwrap();
    let collected = cli_named(
        &home,
        &worker,
        &[
            "run",
            "--project",
            &checkout.id,
            "--ticket",
            &orchestration.slug,
            "--step",
            "slow",
            "--operation-id",
            "validate-restart",
            "--run",
            &run_id,
        ],
    );
    assert_eq!(collected["result"]["state"], "passed", "{collected}");
    assert_eq!(collected["result"]["result_id"], expected_result.as_str());
    second.begin_stopping();
}
