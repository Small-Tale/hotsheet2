//! HS2-JSF985: provisional work across two targets over authenticated HTTP, the
//! stdio MCP tool on a live service and the owned CLI process, all on one runtime.
//! The prerequisite's watch Job is judged through the Job store the way the monitor
//! does; no provider is contacted and no AI tool is started. Synthetic stores and
//! identities only.
use super::orchestration::api;
use super::*;
use hotsheet_jobs::{Acquire, ActiveStatus, FinalStatus, JobStore, Outcome};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::provisional::SpeculationPolicy;
use hotsheet_ticketing::{NewTicket, Settings, StoreMetadata, ops};

const FIRST: &str = "01ARZ3NDEKTSV4RRFFQ69G5FJ1";
const SECOND: &str = "01ARZ3NDEKTSV4RRFFQ69G5FJ2";
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

/// Fail a watch Job with the verdict a monitor would record.
fn judge_red(home: &std::path::Path, owner: &str, job_id: &str, head: &str) {
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
        ("gate", "red"),
        ("run_id", "9001"),
        ("run_attempt", "1"),
        ("run_head_sha", head),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    store
        .finish(
            owner,
            job_id,
            &token,
            FinalStatus::Failed,
            Outcome {
                summary: None,
                refs,
            },
        )
        .unwrap();
}

fn opt_in(project: &std::path::Path) {
    SpeculationPolicy {
        enabled: true,
        ..SpeculationPolicy::default()
    }
    .write(&Settings::for_project(project))
    .unwrap();
}

/// Target 1 (the owning project itself) holds prerequisite A, target 2 holds B. With
/// no opt-in a dispatch of B is refused as it always was. With the orchestration
/// policy and both projects opted in it is dispatched ahead of A's pending gate, the
/// red result puts it at risk on every surface, and a person's reopen is kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_jsf985_http_stdio_mcp_and_cli_share_provisional_work_across_two_targets() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let registry = CheckoutRegistry::new(home.join("checkouts.json"));
    let checkout = registry
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let connection = checkout.sources[0].connection_id.clone();
    let store = FsStore::open(&source).unwrap();
    let second_source = root.path().join("second-store");
    let second_store = FsStore::init_git(&second_source, &StoreMetadata::new("SEC")).unwrap();
    let second_project = root.path().join("second");
    std::fs::create_dir(&second_project).unwrap();
    let second = registry
        .register(&second_project, None, None, vec![second_source])
        .unwrap();
    let second_connection = second.sources[0].connection_id.clone();
    let orchestration = ticket(&store, "TST", "Coordinate across two targets");
    let a = ticket(&store, "TST", "Synthetic prerequisite");
    let b = ticket(&second_store, "SEC", "Synthetic dependent work");

    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
    let human = json!({"role":"human","id":"maintainer"});
    let ai = json!({"role":"ai","id":"scheduler-1"});
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 3_600_000;
    let mut revision = Value::Null;
    for (target, label, destination) in [
        (FIRST, "First", checkout.id.as_str()),
        (SECOND, "Second", second.id.as_str()),
    ] {
        let (status, added) = api(
            router.clone(),
            "POST",
            &format!("{base}/targets"),
            json!({"target_id":target,"label":label,"expected_revision":revision}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{added}");
        revision = added["revision"].clone();
        let candidate = api(
            router.clone(),
            "POST",
            &format!("{base}/inspect"),
            json!({"destination":{"kind":"local","checkout_id":destination}}),
            true,
        )
        .await
        .1;
        let (status, bound) = api(
            router.clone(),
            "PUT",
            &format!("{base}/targets/{target}/binding"),
            json!({"target":{"owner_checkout_id":checkout.id,"target_id":target},
                "expected_manifest_revision":revision,"expected_binding_revision":0,
                "previous_destination":null,"destination":candidate["destination"]}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{bound}");
        let (status, granted) = super::orchestration::remember_rules(
            router.clone(),
            &format!("{base}/targets/{target}"),
            json!({"actor_id":"scheduler-1",
                "actions":["tickets_read","work_start","gates_observe"],
                "expires_at_ms":expires,"actor":human}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{granted}");
        // HS2-XHQ1EH: building ahead is its own remembered decision.
        let (status, remembered) = api(
            router.clone(),
            "POST",
            &format!("{base}/permission-rules"),
            json!({"target_id":target,"action":"provisional_dispatch","actor":human}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{remembered}");
    }
    let post = |path: String, mut body: Value| {
        let router = router.clone();
        let (base, ai) = (base.clone(), ai.clone());
        async move {
            body["actor"] = ai;
            api(router, "POST", &format!("{base}/{path}"), body, true).await
        }
    };
    let ticket_ref = json!({"native_id": orchestration.slug});

    // A is dispatched and integrated at commit `a`; its CI gate pends.
    let (status, run_a) = post(
        "runs".into(),
        json!({"ticket":ticket_ref,"target_id":FIRST,"connection_id":connection,
            "native_id":a.slug,"operation_id":"op-a","worker_id":"worker-a"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{run_a}");
    let run_a = run_a["run_id"].as_str().unwrap().to_owned();
    let commit = "a".repeat(40);
    let (status, linked) = post(
        format!("runs/{run_a}/gates"),
        json!({"commit":commit,"repository":"example/widgets"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["gate"]["state"], "pending");
    let gate_id = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    let job_id = linked["gate"]["watch_ref"].as_str().unwrap().to_owned();
    // B, in the other target, requires A with CI: provisional.
    let (status, step) = post(
        "readiness/define".into(),
        json!({"ticket":ticket_ref,"target_id":SECOND,"connection_id":second_connection,
            "native_id":b.slug,
            "prerequisites":[{"kind":"ticket","target_id":FIRST,"connection_id":connection,
                "native_id":a.slug,"gates":"ci"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{step}");
    assert_eq!(step["level"], "provisional");
    assert_eq!(step["base_commits"], json!([commit]));
    assert!(step.get("speculation").is_none());
    let step_id = step["step"]["step_id"].as_str().unwrap().to_owned();
    let dispatch_b = json!({"ticket":ticket_ref,"target_id":SECOND,
        "connection_id":second_connection,"native_id":b.slug,"operation_id":"op-b",
        "worker_id":"worker-b"});

    // The two new operations need the bearer like the rest of the plan.
    for operation in ["speculation", "reopen"] {
        assert_eq!(
            api(
                router.clone(),
                "POST",
                &format!("{base}/readiness/{operation}"),
                json!({}),
                false
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED,
            "{operation}"
        );
    }
    // No policy: the refusal and the views are what they were before the opt-in.
    let (status, refused) = post("runs".into(), dispatch_b.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "readiness_provisional", "{refused}");
    let (status, listed) = post("readiness/speculation".into(), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listed,
        json!({"enabled":false,"work":[],"repair_first":[],"reevaluate":[]})
    );
    assert!(!home.join("orchestration-provisional.json").exists());
    // The orchestration policy opts in; neither project has yet.
    let actions = json!(["tickets_read", "work_start", "gates_observe"]);
    let (status, stored) = api(
        router.clone(),
        "PUT",
        &format!("{base}/policy"),
        json!({"expected_revision":null,"actor":human,"policy":{
            "targets":[{"target_id":FIRST,"actions":actions},
                {"target_id":SECOND,"actions":actions}],
            "speculation":{"allowed":true}}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    let (status, refused) = post("runs".into(), dispatch_b.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        refused["code"], "readiness_provisional_refused",
        "{refused}"
    );
    let (_, shown) = post("readiness/show".into(), json!({"step_id":step_id})).await;
    assert_eq!(
        shown["speculation"],
        json!({"allowed":false,"refusal":"target_speculation_disabled","depth":0,
            "max_depth":1})
    );
    assert_eq!(second_store.read_ticket(&b.id).unwrap().claimed_by, None);
    // Both projects opt in: B is dispatched ahead of A's pending gate.
    opt_in(&project);
    opt_in(&second_project);
    let (status, run_b) = post("runs".into(), dispatch_b.clone()).await;
    assert_eq!(status, StatusCode::OK, "{run_b}");
    assert_eq!(run_b["state"], "dispatched");
    let run_b = run_b["run_id"].as_str().unwrap().to_owned();
    assert_eq!(
        second_store
            .read_ticket(&b.id)
            .unwrap()
            .claimed_by
            .as_deref(),
        Some("worker-b")
    );
    let (_, listed) = post("readiness/speculation".into(), json!({})).await;
    assert_eq!(listed["enabled"], true);
    let work = &listed["work"][0];
    assert_eq!(
        (&work["step_id"], &work["state"], &work["depth"]),
        (&json!(step_id), &json!("provisional"), &json!(1))
    );
    assert_eq!(
        work["gates"],
        json!([{"target_id":FIRST,"connection_id":connection,"native_id":a.id.to_string(),
            "commit":commit,"gate_ids":[gate_id],"result":"pending","run":gate_id}])
    );
    let (status, status_b) = api(
        router.clone(),
        "GET",
        &format!("{base}/runs/{run_b}/status"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{status_b}");
    assert_eq!(&status_b["provisional"], work);

    // A's CI fails. A sync reads the watch Job; the work is at risk on every surface.
    judge_red(&home, &checkout.id, &job_id, &commit);
    let (status, synced) = post(format!("runs/{run_a}/gate-sync"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{synced}");
    assert_eq!(synced["ci"], "failed", "{synced}");
    // The server's own gate follower may have read the Job first, so the consequence
    // is awaited on the event stream by cursor, never assumed and never polled.
    let events = |after: u64, wait_ms: u64| {
        let router = router.clone();
        let path = format!("{base}/run-events?after={after}&wait_ms={wait_ms}&run={step_id}");
        async move { api(router, "GET", &path, Value::Null, true).await.1 }
    };
    let mut provisional: Vec<(String, Value)> = Vec::new();
    let mut cursor = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    while !provisional.iter().any(|(reason, _)| reason == "at_risk")
        && std::time::Instant::now() < deadline
    {
        let page = events(cursor, if cursor == 0 { 0 } else { 10_000 }).await;
        cursor = page["cursor"].as_u64().unwrap();
        provisional.extend(
            page["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["kind"] == "provisional")
                .map(|entry| {
                    (
                        entry["reason"].as_str().unwrap().to_owned(),
                        entry["gate_id"].clone(),
                    )
                }),
        );
    }
    assert_eq!(
        provisional,
        [
            ("provisional".to_owned(), json!(gate_id)),
            ("at_risk".to_owned(), json!(gate_id))
        ]
    );
    // Reconciling writes the note the red gate owes, unless the follower already did.
    let (status, reconciled) = post("run-reconcile".into(), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{reconciled}");
    let notes: Vec<_> = store
        .read_ticket(&orchestration.id)
        .unwrap()
        .notes
        .into_iter()
        .filter(|note| note.text.starts_with("Provisional work at risk"))
        .collect();
    assert_eq!(notes.len(), 1, "one note, whoever wrote it");
    let note = store
        .read_ticket(&orchestration.id)
        .unwrap()
        .notes
        .into_iter()
        .find(|note| note.text.starts_with("Provisional work at risk"))
        .expect("the at-risk note");
    assert!(note.text.contains("9001.1") && note.text.contains(&step_id));
    assert!(!note.text.contains("Synthetic"));

    // The stdio MCP tool on the live service reads the same ledger.
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
    let as_worker = |mut arguments: Value| {
        arguments["checkout"] = json!(checkout.id);
        arguments["actor_role"] = json!("ai");
        arguments["actor_id"] = json!("scheduler-1");
        arguments
    };
    let call =
        |arguments: Value| mcp_named(&home, &backend, "hotsheet_targets", as_worker(arguments));
    let listed = call(json!({"action":"readiness_speculation"}));
    assert_eq!(listed["work"][0]["state"], "at_risk");
    assert_eq!(listed["work"][0]["recovery"], "rebase_onto_fix");
    assert_eq!(listed["work"][0]["gates"][0]["run"], "9001.1");
    assert_eq!(
        listed["repair_first"],
        json!([{"target_id":FIRST,"connection_id":connection,"native_id":a.id.to_string(),
            "commit":commit,"stacked":[step_id]}])
    );
    assert_eq!(
        call(json!({"action":"run_status","run_id":run_b}))["provisional"]["state"],
        "at_risk"
    );
    let explained = call(json!({"action":"scheduler_explain","step_id":step_id}));
    assert_eq!(explained["standing"], "running");
    assert_eq!(
        explained["provisional"]["gates"][0]["gate_ids"],
        json!([gate_id])
    );
    // A reopen needs its reason, and an unknown step is refused.
    for arguments in [
        json!({"action":"readiness_reopen","step_id":step_id}),
        json!({"action":"readiness_reopen","reason":"fix_changed_contract"}),
        json!({"action":"readiness_reopen","step_id":FIRST,"reason":"fix_changed_contract"}),
    ] {
        mcp_rejected(&home, &backend, "hotsheet_targets", as_worker(arguments));
    }
    let reopened = call(json!({"action":"readiness_reopen","step_id":step_id,
        "reason":"fix_changed_contract"}));
    assert_eq!(reopened["state"], "reopened");
    assert_eq!(reopened["reopen_reason"], "fix_changed_contract");

    // The owned CLI process reads and repeats it on the same machine state.
    let store_arg = source.to_str().unwrap();
    let worker = [
        "--actor-role",
        "ai",
        "--actor-id",
        "scheduler-1",
        "-C",
        store_arg,
        "orchestration",
    ];
    let listed = cli_named(
        &home,
        &worker,
        &["readiness", "speculation", "--project", &checkout.id],
    );
    assert_eq!(listed["work"][0]["state"], "reopened");
    assert_eq!(listed["repair_first"], json!([]));
    let again = cli_named(
        &home,
        &worker,
        &[
            "readiness",
            "reopen",
            &step_id,
            "--reason",
            "fix_changed_contract",
            "--project",
            &checkout.id,
        ],
    );
    assert_eq!(again, reopened);
    assert!(
        cli_failure(
            &home,
            &worker,
            &[
                "readiness",
                "reopen",
                FIRST,
                "--reason",
                "fix_changed_contract",
                "--project",
                &checkout.id,
            ],
        )
        .contains("readiness_step_not_found")
    );
    let shown = cli_named(
        &home,
        &worker,
        &["readiness", "show", &step_id, "--project", &checkout.id],
    );
    assert_eq!(shown["provisional"]["state"], "reopened");
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}
