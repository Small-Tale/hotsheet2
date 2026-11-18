//! HS2-5JBYPV: CI and deployment gates of a run over authenticated HTTP, the stdio MCP
//! tool on a live service and the owned CLI process, all on one runtime. The watch Job
//! is judged through the Job store the way the monitor does; no provider is contacted.
use super::orchestration::api;
use super::*;
use hotsheet_jobs::{Acquire, ActiveStatus, FinalStatus, JobStore, Outcome};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, ops};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FE7";
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

/// Finish a watch Job with the verdict a monitor would record.
pub(super) fn judge(home: &std::path::Path, owner: &str, job_id: &str, gate: &str, head: &str) {
    let store = JobStore::at(home);
    let (_, token) = store
        .acquire(
            owner,
            job_id,
            Acquire {
                owner: "monitor:test".into(),
                status: ActiveStatus::Running,
                launch_id: None,
                external: None,
            },
        )
        .unwrap();
    let refs = [
        ("gate", gate),
        ("run_id", "9001"),
        ("run_attempt", "1"),
        ("run_head_sha", head),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    let status = if gate == "green" {
        FinalStatus::Succeeded
    } else {
        FinalStatus::Failed
    };
    store
        .finish(
            owner,
            job_id,
            &token,
            status,
            Outcome {
                summary: None,
                refs,
            },
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_5jbypv_http_stdio_mcp_and_cli_link_follow_and_deploy_on_one_runtime() {
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
    let orchestration = ticket(&store, "Coordinate the gated release");
    let work = ticket(&store, "Work whose commit is gated");
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
    let human = json!({"role":"human","id":"maintainer"});
    let ai = json!({"role":"ai","id":"scheduler-1"});
    let (status, _) = super::orchestration::remember_rules(
        router.clone(),
        &format!("{base}/targets/{TARGET}"),
        json!({"actor_id":"scheduler-1",
            "actions":["tickets_read","work_start","gates_observe","deployments_run"],
            "expires_at_ms":expires,"actor":human}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, run) = api(
        router.clone(),
        "POST",
        &format!("{base}/runs"),
        json!({"ticket":{"native_id":orchestration.slug},"target_id":TARGET,
            "connection_id":connection,"native_id":work.slug,"operation_id":"op-gated",
            "worker_id":"gated-worker","actor":ai}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{run}");
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    // Every gate route needs the bearer.
    for (method, path) in [
        ("GET", format!("{base}/gates")),
        ("GET", format!("{base}/gates/{TARGET}")),
        ("POST", format!("{base}/runs/{run_id}/gates")),
        ("POST", format!("{base}/runs/{run_id}/gate-sync")),
        ("POST", format!("{base}/runs/{run_id}/deployments")),
        ("POST", format!("{base}/runs/{run_id}/reviews")),
    ] {
        assert_eq!(
            api(router.clone(), method, &path, json!({}), false).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    // Nothing linked: unknown, and a deployment has nothing to rest on.
    let (status, empty) = api(
        router.clone(),
        "GET",
        &format!("{base}/gates?run={run_id}"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&empty["ci"], &empty["ci_reason"]),
        (&json!("unknown"), &json!("no_gate"))
    );
    let deployment = json!({"environment":"production","script":"scripts/deploy.sh",
        "production":true,"operation_id":"deploy-prod-1","actor":ai});
    let (status, refused) = api(
        router.clone(),
        "POST",
        &format!("{base}/runs/{run_id}/deployments"),
        deployment.clone(),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "gate_not_passed");
    // A link without authority, with a short commit, or for an unknown run is refused.
    for (body, expected, code) in [
        (
            json!({"commit":a,"repository":"example/widgets"}),
            StatusCode::BAD_REQUEST,
            "invalid_request",
        ),
        (
            json!({"commit":"abc1234","repository":"example/widgets","actor":ai}),
            StatusCode::BAD_REQUEST,
            "invalid_request",
        ),
        (
            json!({"commit":a,"actor":ai}),
            StatusCode::BAD_REQUEST,
            "invalid_request",
        ),
        (
            json!({"commit":a,"repository":"example/widgets","provider":"other-ci","actor":ai}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "gate_provider_unsupported",
        ),
    ] {
        let (status, reply) = api(
            router.clone(),
            "POST",
            &format!("{base}/runs/{run_id}/gates"),
            body,
            true,
        )
        .await;
        assert_eq!(
            (status, reply["code"].as_str()),
            (expected, Some(code)),
            "{reply}"
        );
    }
    assert_eq!(
        api(
            router.clone(),
            "POST",
            &format!("{base}/runs/{TARGET}/gates"),
            json!({"commit":a,"repository":"example/widgets","actor":ai}),
            true
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // Link the commit's CI gate: one watch Job, pending, and nobody observing yet.
    let (status, linked) = api(
        router.clone(),
        "POST",
        &format!("{base}/runs/{run_id}/gates"),
        json!({"commit":a,"repository":"example/widgets","actor":ai}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["created"], true);
    assert_eq!(linked["watching"], false);
    assert_eq!(linked["gate"]["state"], "pending");
    assert_eq!(linked["gate"]["provider"], "github-actions");
    assert_eq!(linked["gate"]["ci"]["workflow"], "CI");
    let gate_id = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    let job_id = linked["gate"]["watch_ref"].as_str().unwrap().to_owned();
    assert_eq!(
        JobStore::at(&home).list(&checkout.id).unwrap().jobs.len(),
        1
    );
    // A subscriber waits on the run event stream while the watch Job is judged green.
    // Nobody calls sync: the server's follower wakes on the Job store's change.
    let events = |after: u64, wait_ms: u64| {
        let router = router.clone();
        let path = format!("{base}/run-events?after={after}&wait_ms={wait_ms}&run={run_id}");
        async move { api(router, "GET", &path, Value::Null, true).await.1 }
    };
    let first = events(0, 0).await;
    let gate_states = |page: &Value| -> Vec<String> {
        page["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["kind"] == "gate")
            .map(|entry| {
                assert_eq!(entry["gate_id"], gate_id);
                entry["gate_state"].as_str().unwrap().to_owned()
            })
            .collect()
    };
    assert_eq!(gate_states(&first), ["unknown", "pending"]);
    let mut cursor = first["cursor"].as_u64().unwrap();
    let waiting = tokio::spawn(events(cursor, 15_000));
    judge(&home, &checkout.id, &job_id, "green", &a);
    let mut seen = gate_states(&waiting.await.unwrap());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    while seen.is_empty() && std::time::Instant::now() < deadline {
        // Each request waits for the next event; this is cursor replay, not a timer.
        let page = events(cursor, 10_000).await;
        cursor = page["cursor"].as_u64().unwrap();
        seen = gate_states(&page);
    }
    assert_eq!(
        seen,
        ["passed"],
        "the gate never passed from the Job store's change"
    );
    let (_, summary) = api(
        router.clone(),
        "GET",
        &format!(
            "{base}/gates?target_id={TARGET}&commit={}",
            a.to_uppercase()
        ),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(summary["ci"], "passed");
    assert_eq!(summary["commits"], json!([a]));
    assert_eq!(summary["gates"][0]["evidence"]["reference"], "9001");
    assert_eq!(summary["gates"][0]["evidence"]["coverage"], "exact");
    // The stdio MCP tool on the live service.
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
    let listed = call(json!({"action":"gate_list","run_id":run_id}));
    assert_eq!(
        (&listed["ci"], &listed["deployment"]),
        (&json!("passed"), &Value::Null)
    );
    assert_eq!(
        call(json!({"action":"gate_list","target_id":TARGET,"commit":a}))["ci"],
        "passed"
    );
    let shown = call(json!({"action":"gate_show","gate_id":gate_id}));
    assert_eq!(shown["state"], "passed");
    assert_eq!(shown["history"].as_array().unwrap().len(), 3);
    let again = call(json!({"action":"gate_link","run_id":run_id,"commit":a,
        "repository":"example/widgets"}));
    assert_eq!(
        (&again["created"], &again["gate"]["gate_id"]),
        (&json!(false), &json!(gate_id))
    );
    let synced = call(json!({"action":"gate_sync","run_id":run_id,"wait_ms":100}));
    assert_eq!(
        (&synced["ci"], &synced["changed"]),
        (&json!("passed"), &json!([]))
    );
    // Production: the request waits for a person, and asking again files nothing new.
    let request = json!({"action":"deployment_request","run_id":run_id,
        "environment":"production","script":"scripts/deploy.sh","production":true,
        "operation_id":"deploy-prod-1"});
    let waiting = call(request.clone());
    assert_eq!(waiting["gate"]["state"], "awaiting_approval");
    assert_eq!(waiting["gate"]["commit"], a);
    assert_eq!(waiting["gate"]["deployment"]["requires_gate_id"], gate_id);
    let approval_id = waiting["request_id"].as_str().unwrap().to_owned();
    assert_eq!(call(request.clone())["request_id"], approval_id);
    let listed = call(json!({"action":"permission_list"}));
    let requests = listed["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["action"], "deployments_run");
    assert_eq!(requests[0]["each_time"], true);
    // The governed session cannot answer for itself; a person can.
    let approval_path = format!("{base}/permissions/{approval_id}");
    for (actor, expected) in [(&ai, StatusCode::FORBIDDEN), (&human, StatusCode::OK)] {
        let body = json!({"answer":"allow_once","actor":actor});
        assert_eq!(
            api(router.clone(), "POST", &approval_path, body, true)
                .await
                .0,
            expected
        );
    }
    // Approved and consumed. The target's machine runs only a tracked script
    // (HS2-G3VAK4); this target tracks none, and the gate says so.
    let approved = call(request.clone());
    assert_eq!(approved["gate"]["state"], "unknown");
    assert_eq!(
        approved["gate"]["reason"],
        "gate_deployment_script_untracked"
    );
    assert_eq!(call(request)["gate"]["state"], "unknown");
    assert_eq!(
        call(json!({"action":"permission_list"}))["requests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let listed = call(json!({"action":"gate_list","run_id":run_id}));
    assert_eq!(
        (&listed["ci"], &listed["deployment"]),
        (&json!("passed"), &json!("unknown"))
    );
    for arguments in [
        json!({"action":"gate_show","checkout":checkout.id,"gate_id":"not-a-gate"}),
        json!({"action":"gate_show","checkout":checkout.id,"gate_id":TARGET}),
        json!({"action":"gate_list","checkout":checkout.id,"commit":"main"}),
        json!({"action":"gate_link","checkout":checkout.id,"run_id":run_id,"commit":a,
            "repository":"example/widgets"}),
        json!({"action":"deployment_request","checkout":checkout.id,"run_id":run_id,
            "environment":"beta","script":"../deploy.sh","operation_id":"deploy-beta",
            "actor_role":"ai","actor_id":"scheduler-1"}),
    ] {
        mcp_rejected(&home, &backend, "hotsheet_targets", arguments);
    }
    // HS2-GD3NNK: an AI requests a code review over MCP and a person approves it over
    // HTTP; nobody is asked, and the run's summary carries the verdict.
    let review = call(
        json!({"action":"gate_review","run_id":run_id,"review":"requested",
        "commit":a}),
    );
    assert_eq!(
        (&review["gate"]["kind"], &review["gate"]["state"]),
        (&json!("review"), &json!("awaiting_approval"))
    );
    assert_eq!(review["gate"]["provider"], "review");
    let (status, approved) = api(
        router.clone(),
        "POST",
        &format!("{base}/runs/{run_id}/reviews"),
        json!({"review":"approved","actor":human}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    assert_eq!(approved["gate"]["state"], "passed");
    assert_eq!(approved["gate"]["gate_id"], review["gate"]["gate_id"]);
    assert_eq!(
        call(json!({"action":"gate_list","run_id":run_id}))["review"],
        "passed"
    );
    mcp_rejected(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"gate_review","checkout":checkout.id,"run_id":run_id,
            "review":"maybe","actor_role":"ai","actor_id":"scheduler-1"}),
    );
    assert_eq!(
        call(json!({"action":"permission_list"}))["requests"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "a review asks nobody"
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    // The owned CLI process on the same machine home.
    let store_arg = source.to_str().unwrap();
    let worker = [
        "--actor-role",
        "ai",
        "--actor-id",
        "scheduler-1",
        "-C",
        store_arg,
        "orchestration",
        "gate",
    ];
    let listed = cli_named(
        &home,
        &worker,
        &["list", "--project", &checkout.id, "--run", &run_id],
    );
    assert_eq!(listed["ci"], "passed");
    assert_eq!(listed["review"], "passed");
    assert_eq!(listed["gates"].as_array().unwrap().len(), 3);
    assert_eq!(
        cli_named(
            &home,
            &worker,
            &["show", &gate_id, "--project", &checkout.id]
        )["state"],
        "passed"
    );
    // The run moves to another commit: the CLI links it without starting a monitor,
    // the old link is superseded with the deployment that never ran.
    let moved = cli_named(
        &home,
        &worker,
        &[
            "link",
            &run_id,
            "--project",
            &checkout.id,
            "--commit",
            &b,
            "--repo",
            "example/widgets",
            "--no-monitor",
        ],
    );
    assert_eq!(moved["gate"]["state"], "pending");
    assert_eq!(moved["watching"], false);
    assert_eq!(moved["superseded"].as_array().unwrap().len(), 2);
    let synced = cli_named(
        &home,
        &worker,
        &["sync", &run_id, "--project", &checkout.id],
    );
    assert_eq!(
        (&synced["ci"], &synced["superseded"]),
        (&json!("pending"), &json!(2))
    );
    let refused = cli_failure(
        &home,
        &worker,
        &[
            "deploy",
            &run_id,
            "--project",
            &checkout.id,
            "--environment",
            "beta",
            "--script",
            "scripts/deploy.sh",
            "--operation-id",
            "deploy-beta-1",
        ],
    );
    assert!(refused.contains("gate_not_passed"), "{refused}");
    // The watch of the new commit is judged red; a sync that waits picks it up.
    let job_b = moved["gate"]["watch_ref"].as_str().unwrap();
    judge(&home, &checkout.id, job_b, "red", &b);
    let red = cli_named(
        &home,
        &worker,
        &[
            "sync",
            &run_id,
            "--project",
            &checkout.id,
            "--wait-seconds",
            "5",
        ],
    );
    assert_eq!(red["ci"], "failed");
    assert_eq!(red["changed"].as_array().unwrap().len(), 1);
    // HS2-GD3NNK: the CLI requests a review of the new commit, which supersedes the
    // approved one, and sends it back.
    let requested = cli_named(
        &home,
        &worker,
        &[
            "review",
            &run_id,
            "--project",
            &checkout.id,
            "--review",
            "requested",
            "--commit",
            &b,
        ],
    );
    assert_eq!(requested["gate"]["state"], "awaiting_approval");
    assert_eq!(requested["superseded"].as_array().unwrap().len(), 1);
    let sent_back = cli_named(
        &home,
        &worker,
        &[
            "review",
            &run_id,
            "--project",
            &checkout.id,
            "--review",
            "changes-requested",
        ],
    );
    assert_eq!(sent_back["gate"]["state"], "failed");
    assert_eq!(sent_back["gate"]["reason"], "changes_requested");
    assert_eq!(
        cli_named(
            &home,
            &worker,
            &["list", "--project", &checkout.id, "--run", &run_id],
        )["review"],
        "failed"
    );
    // Records hold identifiers and codes only.
    let record = std::fs::read_to_string(home.join("orchestration-gates.json")).unwrap();
    for forbidden in [SECRET, "Work whose commit is gated", "Coordinate the gated"] {
        assert!(!record.contains(forbidden));
    }
    let audit = api(
        router,
        "GET",
        &format!("{base}/audit?after=0&limit=500"),
        Value::Null,
        true,
    )
    .await
    .1;
    let actions: Vec<_> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["action"].as_str().unwrap(),
                entry["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    for expected in [
        ("gates_observe", "succeeded"),
        ("deployments_run", "denied"),
        ("permission_answer", "approved"),
        ("deployments_run", "succeeded"),
    ] {
        assert!(
            actions.contains(&expected),
            "{expected:?} missing from {actions:?}"
        );
    }
}
