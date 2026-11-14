//! HS2-42TARA: the plan of prerequisites through the owned CLI process, the
//! authenticated routes of a real server and the stdio MCP tool on a live service.
//! Synthetic stores and identities only.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, StoreMetadata, TicketPatch, ops};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FF1";
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
fn complete(store: &FsStore, ticket: &hotsheet_model::Ticket) {
    ops::update(
        store,
        &ticket.id,
        NOW.into(),
        TicketPatch {
            status: Some(hotsheet_model::Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
}
/// The levels a step entered, from the owner's run event stream.
fn levels(page: &Value, step: &str) -> Vec<String> {
    page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == "readiness" && entry["run_id"] == step)
        .map(|entry| entry["reason"].as_str().unwrap().to_owned())
        .collect()
}

/// The bare CLI plans two steps, is refused a dispatch that is not ready, gets a
/// person's override for one operation, and sees the plan follow the target.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_42tara_cli_plans_gates_overrides_and_follows_the_target() {
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
    let orchestration = ticket(&store, "Coordinate the synthetic plan");
    let first = ticket(&store, "Synthetic prerequisite");
    let second = ticket(&store, "Synthetic dependent work");
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
    // HS2-XHQ1EH: a person remembers the rules the orchestrator needs.
    {
        let (action, tool) = ("work_start", None::<&str>);
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
    let readiness = |arguments: &[&str]| {
        let mut all = vec!["readiness"];
        all.extend_from_slice(arguments);
        all.extend_from_slice(&["--project", &owner.id]);
        cli_named(&home, &worker, &all)
    };
    // Nothing planned yet.
    assert_eq!(readiness(&["plan"])["steps"], json!([]));
    assert_eq!(readiness(&["ready"]), json!([]));
    let define = |target: &str, extra: &[&str]| {
        let mut arguments = vec![
            "define",
            "--ticket",
            &orchestration.slug,
            "--target",
            TARGET,
            "--connection",
            &connection,
            "--id",
            target,
        ];
        arguments.extend_from_slice(extra);
        readiness(&arguments)
    };
    let step_a = define(&first.slug, &[]);
    assert_eq!(step_a["level"], "confirmed");
    assert_eq!(step_a["step"]["target"]["native_id"], first.id.to_string());
    let requirement = format!("{TARGET}/{connection}:{}", first.slug);
    let step_b = define(
        &second.slug,
        &["--requires", &requirement, "--gates", "none"],
    );
    assert_eq!(step_b["level"], "blocked", "{step_b}");
    assert_eq!(step_b["reasons"][0]["code"], "prerequisite_not_finished");
    assert_eq!(
        step_b["step"]["prerequisites"][0]["native_id"],
        first.id.to_string()
    );
    let (a, b) = (
        step_a["step"]["step_id"].as_str().unwrap().to_owned(),
        step_b["step"]["step_id"].as_str().unwrap().to_owned(),
    );
    // The same command again is the same step.
    let again = define(
        &second.slug,
        &["--requires", &requirement, "--gates", "none"],
    );
    assert_eq!(again["step"], step_b["step"]);
    let ready = readiness(&["ready"]);
    assert_eq!(ready.as_array().unwrap().len(), 1);
    assert_eq!(ready[0]["step_id"], a);
    assert_eq!(ready[0]["unblocks"], json!([b]));
    assert_eq!(ready[0]["up_next"], false);
    let plan = readiness(&["plan"]);
    assert_eq!(plan["counts"], json!({"blocked": 1, "confirmed": 1}));
    assert_eq!(readiness(&["show", &b])["level"], "blocked");
    // A malformed requirement never reaches the runtime.
    assert!(
        cli_failure(
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
                &second.slug,
                "--requires",
                "not-qualified",
                "--project",
                &owner.id,
            ],
        )
        .contains("<target-id>/<connection>:<ticket>")
    );

    let dispatch = |operation: &str, extra: &[&str]| {
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
            &second.slug,
            "--operation-id",
            operation,
            "--worker",
            "worker-b",
        ];
        arguments.extend_from_slice(extra);
        arguments.into_iter().map(str::to_owned).collect::<Vec<_>>()
    };
    fn refs(arguments: &[String]) -> Vec<&str> {
        arguments.iter().map(String::as_str).collect()
    }
    // Not ready: a typed refusal, nothing claimed.
    let refused = cli_failure(&home, &worker, &refs(&dispatch("op-b", &[])));
    assert!(refused.contains("readiness_blocked"), "{refused}");
    assert_eq!(store.read_ticket(&second.id).unwrap().claimed_by, None);
    // HS2-XHQ1EH: the orchestrator's own override is a judgment call. It adds a
    // FEEDBACK NEEDED note with CHOICE options to the orchestration ticket and waits.
    let asking = dispatch("op-b", &["--override-readiness"]);
    let waiting = cli_failure(&home, &worker, &refs(&asking));
    assert!(waiting.contains("decision_required"), "{waiting}");
    let decision = store
        .read_ticket(&orchestration.id)
        .unwrap()
        .notes
        .into_iter()
        .find(|note| note.kind == hotsheet_model::NoteKind::FeedbackNeeded)
        .unwrap();
    assert!(decision.text.contains("CHOICE:") && decision.text.contains("- Start anyway"));
    assert!(!decision.text.contains("Synthetic"));
    // Asking again adds no second note, and nothing is claimed meanwhile.
    cli_failure(&home, &worker, &refs(&asking));
    assert_eq!(
        store
            .read_ticket(&orchestration.id)
            .unwrap()
            .notes
            .iter()
            .filter(|note| note.kind == hotsheet_model::NoteKind::FeedbackNeeded)
            .count(),
        1
    );
    assert_eq!(store.read_ticket(&second.id).unwrap().claimed_by, None);
    // A person responds through the existing feedback flow.
    ops::add_note_with_metadata(
        &store,
        &orchestration.id,
        hotsheet_model::Ulid::new(),
        "2026-10-16T00:00:00Z".into(),
        hotsheet_model::NoteKind::Regular,
        ops::NoteMetadataInput {
            response_to: Some(decision.id.to_string()),
            ..ops::NoteMetadataInput::default()
        },
        "- A. Start anyway".into(),
    )
    .unwrap();
    let run = cli_named(&home, &worker, &refs(&asking));
    assert_eq!(run["state"], "dispatched");
    assert_eq!(
        store.read_ticket(&second.id).unwrap().claimed_by.as_deref(),
        Some("worker-b")
    );
    // The override is in the audit journal and noted on the orchestration ticket
    // by identifiers only.
    let audit = cli_named(
        &home,
        &human,
        &["audit", "--project", &owner.id, "--limit", "500"],
    );
    let overrides: Vec<&Value> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["action"] == "readiness_override")
        .collect();
    // Asked twice, then decided and used, under the one operation.
    assert_eq!(overrides.len(), 3, "{audit}");
    assert_eq!(overrides[0]["outcome"], "denied");
    assert_eq!(overrides[0]["reason"], "decision_required");
    assert_eq!(overrides[2]["outcome"], "allowed");
    assert_eq!(overrides[2]["reason"], "blocked");
    for entry in &overrides {
        assert_eq!(entry["operation_id"], "op-b");
    }
    let noted = store.read_ticket(&orchestration.id).unwrap();
    let note = noted
        .notes
        .iter()
        .find(|note| note.text.starts_with("Readiness override"))
        .unwrap();
    assert!(note.text.contains(&b) && note.text.contains(&decision.id.to_string()));
    assert!(!note.text.contains("Synthetic"));

    // The prerequisite is finished in the target; a refresh confirms it and the
    // stream shows each level exactly once.
    complete(&store, &first);
    let refreshed = readiness(&["refresh"]);
    assert_eq!(refreshed.get("unverified"), None);
    assert_eq!(
        refreshed["counts"],
        json!({"finished": 1, "in_progress": 1})
    );
    let events = cli_named(
        &home,
        &worker,
        &["run", "events", "--project", &owner.id, "--after", "0"],
    );
    assert_eq!(levels(&events, &a), ["confirmed", "finished"]);
    assert_eq!(levels(&events, &b), ["blocked", "in_progress"]);
    let one = cli_named(
        &home,
        &worker,
        &["run", "events", "--project", &owner.id, "--run", &b],
    );
    assert_eq!(one["entries"].as_array().unwrap().len(), 2);
    // Removing a step is audited and leaves the other.
    assert_eq!(readiness(&["remove", &a])["step_id"], a);
    assert!(
        cli_failure(
            &home,
            &worker,
            &["readiness", "show", &a, "--project", &owner.id]
        )
        .contains("readiness_step_not_found")
    );
    assert_eq!(readiness(&["plan"])["steps"].as_array().unwrap().len(), 1);
}

/// A real server serves the plan routes behind its bearer, refuses unready work with
/// a typed 409, and the stdio MCP tool reads the same plan and waits on its events.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_42tara_server_routes_and_stdio_mcp_share_the_plan() {
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
    let first = ticket(&store, "Synthetic prerequisite");
    let second = ticket(&store, "Synthetic dependent work");
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
    // Every plan route needs the bearer.
    for operation in ["define", "show", "plan", "ready", "refresh", "remove"] {
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
    let actor = json!({"role":"ai","id":"scheduler-1"});
    let ticket_ref = json!({"native_id": orchestration.slug});
    let (status, step_a) = api(
        router.clone(),
        "POST",
        &format!("{base}/readiness/define"),
        json!({"actor":actor,"ticket":ticket_ref,"target_id":TARGET,
            "connection_id":connection,"native_id":first.slug}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{step_a}");
    assert_eq!(step_a["level"], "confirmed");
    let (status, step_b) = api(
        router.clone(),
        "POST",
        &format!("{base}/readiness/define"),
        json!({"actor":actor,"ticket":ticket_ref,"target_id":TARGET,
            "connection_id":connection,"native_id":second.slug,
            "prerequisites":[{"kind":"ticket","target_id":TARGET,"connection_id":connection,
                "native_id":first.slug,"gates":"none"}]}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{step_b}");
    assert_eq!(step_b["level"], "blocked");
    let (a, b) = (
        step_a["step"]["step_id"].as_str().unwrap().to_owned(),
        step_b["step"]["step_id"].as_str().unwrap().to_owned(),
    );
    // Unready work is a typed conflict; an unknown step is absent; a malformed
    // request is refused; an unidentified caller cannot plan.
    let dispatch = json!({"actor":actor,"ticket":ticket_ref,"target_id":TARGET,
        "connection_id":connection,"native_id":second.slug,"operation_id":"op-b",
        "worker_id":"worker-b"});
    let (status, refused) = api(
        router.clone(),
        "POST",
        &format!("{base}/runs"),
        dispatch.clone(),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "readiness_blocked", "{refused}");
    for (operation, body, expected) in [
        ("show", json!({"step_id": TARGET}), StatusCode::NOT_FOUND),
        ("show", json!({"step": b}), StatusCode::BAD_REQUEST),
        ("explain", json!({}), StatusCode::BAD_REQUEST),
        (
            "define",
            json!({"ticket":ticket_ref,"target_id":TARGET,"connection_id":connection,
                "native_id":first.slug}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        assert_eq!(
            api(
                router.clone(),
                "POST",
                &format!("{base}/readiness/{operation}"),
                body,
                true
            )
            .await
            .0,
            expected,
            "{operation}"
        );
    }
    let (status, ready) = api(
        router.clone(),
        "POST",
        &format!("{base}/readiness/ready"),
        json!({}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ready.as_array().unwrap().len(), 1);
    assert_eq!(ready[0]["step_id"], a);
    assert_eq!(ready[0]["priority"], "default");

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
    let plan = call(json!({"action":"readiness_plan"}));
    assert_eq!(plan["counts"], json!({"blocked": 1, "confirmed": 1}));
    assert_eq!(
        call(json!({"action":"readiness_show","step_id":b}))["reasons"][0]["code"],
        "prerequisite_not_finished"
    );
    // The tool is refused unready work exactly as the route is.
    let unready = json!({"action":"run_dispatch","target_id":TARGET,
        "ticket":ticket_ref,"connection_id":connection,"native_id":second.slug,
        "operation_id":"op-b","worker_id":"worker-b"});
    mcp_rejected(
        &home,
        &backend,
        "hotsheet_targets",
        as_worker(unready.clone()),
    );
    assert_eq!(store.read_ticket(&second.id).unwrap().claimed_by, None);
    // It dispatches the ready step, whose run then drives the plan: when the target
    // finishes the ticket and the run is synced, the dependent step becomes ready.
    let run = call(json!({"action":"run_dispatch","target_id":TARGET,
        "ticket":ticket_ref,"connection_id":connection,"native_id":first.slug,
        "operation_id":"op-a","worker_id":"worker-a"}));
    assert_eq!(run["state"], "dispatched");
    complete(&store, &first);
    let synced = call(json!({"action":"run_sync","run_id":run["run_id"]}));
    assert_eq!(synced["state"], "completed");
    let stream = call(json!({"action":"run_events","after":0}));
    assert_eq!(
        levels(&stream, &a),
        ["confirmed", "in_progress", "finished"]
    );
    assert_eq!(levels(&stream, &b), ["blocked", "confirmed"]);
    let ready = call(json!({"action":"readiness_ready"}));
    assert_eq!(ready.as_array().unwrap().len(), 1);
    assert_eq!(ready[0]["step_id"], b);
    assert_eq!(ready[0]["level"], "confirmed");
    assert_eq!(call(unready)["state"], "dispatched");
    assert_eq!(
        call(json!({"action":"readiness_refresh","step_id":b}))["steps"][0]["level"],
        "in_progress"
    );
    assert_eq!(
        call(json!({"action":"readiness_remove","step_id":a}))["step_id"],
        a
    );
    mcp_rejected(
        &home,
        &backend,
        "hotsheet_targets",
        as_worker(json!({"action":"readiness_show","step_id":a})),
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}
