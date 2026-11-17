//! HS2-WN0MXZ: validation steps run on a real second server over authenticated TLS
//! through the owned CLI process, and through the stdio MCP tool on a live service.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FE4";
const NOW: &str = "2026-10-15T00:00:00Z";
/// SHA-256 of the two bytes `ok`.
const OK_SHA256: &str = "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df";

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

/// The target project's own profile file: each profile is one shell line.
fn profiles(project: &std::path::Path, profiles: &[(&str, &str)]) {
    let directory = project.join(".hotsheet2");
    std::fs::create_dir_all(&directory).unwrap();
    let profiles: Vec<Value> = profiles
        .iter()
        .map(|(name, line)| json!({"name": name, "program": "/bin/sh", "args": ["-c", line]}))
        .collect();
    std::fs::write(
        directory.join("validation-profiles.json"),
        json!({"schema_version": 1, "profiles": profiles}).to_string(),
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_wn0mxz_real_tls_remote_validation_runs_on_the_peer_through_the_cli() {
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
    profiles(
        &peer_project,
        &[
            (
                "smoke",
                "printf ok; printf synthetic-secret >&2; echo x >> smoke-runs",
            ),
            (
                "flaky",
                "if [ -f seen ]; then exit 0; else touch seen; exit 3; fi",
            ),
            (
                "held",
                "while [ ! -f release ]; do sleep 0.05; done; echo x >> held-ends",
            ),
        ],
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
    let orchestration = ticket(&owner_store, "OWN", "Validate the synthetic release");
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
    let validation = |rest: &[&str]| {
        let mut arguments = vec!["validation", rest[0], "--project", &owner.id];
        arguments.extend_from_slice(&rest[1..]);
        arguments
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>()
    };
    let ok = |actor: &[&str], rest: &[&str]| {
        let arguments = validation(rest);
        cli_named(
            &home,
            actor,
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    };
    let fail = |actor: &[&str], rest: &[&str]| {
        let arguments = validation(rest);
        cli_failure(
            &home,
            actor,
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    };
    let step = |name: &'static str| ["--ticket", orchestration.slug.as_str(), "--step", name];
    let define_smoke = [
        "define",
        "--ticket",
        &orchestration.slug,
        "--step",
        "smoke",
        "--target",
        TARGET,
        "--profile",
        "smoke",
        "--expect-stdout-sha256",
        OK_SHA256,
        "--timeout-seconds",
        "30",
    ];
    // HS2-XHQ1EH: defining a step changes only the orchestration project's own
    // record and asks nobody; running one asks unless a rule is remembered.
    // HS2-XHQ1EH: a person remembers the rules the orchestrator needs.
    {
        let (action, tool) = ("validation_run", None::<&str>);
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
    let defined = ok(&worker, &define_smoke);
    assert_eq!(defined["state"], "unverified");
    assert_eq!(defined["reason"], "not_run");
    assert_eq!(defined["step"]["definition"]["check"]["name"], "smoke");
    assert_eq!(defined["step"]["definition"]["timeout_seconds"], 30);
    // The CLI waits for the result; the step ran on the peer, in its checkout.
    let run = [
        &["run"][..],
        &step("smoke"),
        &["--operation-id", "op-val-1"],
    ]
    .concat();
    let passed = ok(&worker, &run);
    assert_eq!(passed["result"]["state"], "passed", "{passed}");
    assert_eq!(passed["result"]["outcome"]["exit_code"], 0);
    assert_eq!(passed["result"]["outcome"]["stdout"]["sha256"], OK_SHA256);
    assert_eq!(passed["result"]["outcome"]["stdout"]["bytes"], 2);
    assert_eq!(passed["evidence"]["state"], "passed");
    assert!(peer_project.join("smoke-runs").exists());
    assert!(!project.join("smoke-runs").exists());
    // Repeating the command is the same result and one execution.
    assert_eq!(ok(&worker, &run)["result"], passed["result"]);
    assert_eq!(
        std::fs::read_to_string(peer_project.join("smoke-runs"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    // Only references are recorded: no output, credential or origin anywhere.
    let record = std::fs::read_to_string(home.join("orchestration-validation.json")).unwrap();
    let notes: String = owner_store
        .read_ticket(&orchestration.id)
        .unwrap()
        .notes
        .iter()
        .map(|note| note.text.clone())
        .collect();
    assert!(notes.contains(passed["result"]["result_id"].as_str().unwrap()));
    for forbidden in [SECRET, peer.origin.as_str(), "synthetic-secret", "printf"] {
        assert!(!record.contains(forbidden));
        assert!(!notes.contains(forbidden));
    }
    assert!(!peer_home.join("orchestration-validation.json").exists());
    // A failing step is retried by the same command while its policy allows.
    ok(
        &worker,
        &[
            "define",
            "--ticket",
            &orchestration.slug,
            "--step",
            "flaky",
            "--target",
            TARGET,
            "--profile",
            "flaky",
            "--max-attempts",
            "2",
            "--requires",
            "smoke",
        ],
    );
    let flaky = ok(
        &worker,
        &[
            &["run"][..],
            &step("flaky"),
            &["--operation-id", "op-val-2"],
        ]
        .concat(),
    );
    assert_eq!(flaky["result"]["state"], "passed", "{flaky}");
    assert_eq!(flaky["result"]["attempt"], 2);
    let shown = ok(&worker, &[&["show"][..], &step("flaky")].concat());
    let history = shown["history"].as_array().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["state"], "failed");
    assert_eq!(history[0]["reason"], "exit_code_mismatch");
    assert_eq!(history[0]["outcome"]["exit_code"], 3);
    // A command chosen by the orchestration project waits for a person.
    ok(
        &worker,
        &[
            "define",
            "--ticket",
            &orchestration.slug,
            "--step",
            "custom",
            "--target",
            TARGET,
            "--program",
            "/bin/sh",
            "--arg",
            "-c",
            "--arg",
            "echo x >> custom-runs; exit 7",
            "--expect-exit",
            "7",
        ],
    );
    let custom = [
        &["run"][..],
        &step("custom"),
        &["--operation-id", "op-val-3"],
    ]
    .concat();
    // A command the orchestration project chose asks a person for each operation,
    // whatever is remembered.
    assert!(fail(&worker, &custom).contains("permission_required"));
    assert!(!peer_project.join("custom-runs").exists());
    let listed = cli_named(
        &home,
        &human,
        &["permission", "list", "--project", &owner.id],
    );
    let pending: Vec<&Value> = listed["requests"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|request| request["state"] == "pending")
        .collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["action"], "validation_run");
    assert_eq!(pending[0]["operation_id"], "op-val-3");
    assert_eq!(pending[0]["each_time"], true);
    cli_named(
        &home,
        &human,
        &[
            "permission",
            "answer",
            pending[0]["request_id"].as_str().unwrap(),
            "--project",
            &owner.id,
            "--answer",
            "allow_once",
        ],
    );
    let approved = ok(&worker, &custom);
    assert_eq!(approved["result"]["state"], "passed", "{approved}");
    assert_eq!(approved["result"]["outcome"]["exit_code"], 7);
    assert!(peer_project.join("custom-runs").exists());
    // A profile the peer does not define is unavailable, not failed.
    ok(
        &worker,
        &[
            "define",
            "--ticket",
            &orchestration.slug,
            "--step",
            "absent",
            "--target",
            TARGET,
            "--profile",
            "not-defined-there",
        ],
    );
    let absent = ok(
        &worker,
        &[
            &["run"][..],
            &step("absent"),
            &["--operation-id", "op-val-4"],
        ]
        .concat(),
    );
    assert_eq!(absent["result"]["state"], "unavailable");
    assert_eq!(absent["result"]["reason"], "profile_not_defined");
    let listed = ok(&worker, &["list", "--ticket", &orchestration.slug]);
    let states: Vec<_> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|evidence| {
            (
                evidence["step"]["definition"]["step_id"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
                evidence["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            ("smoke".to_owned(), "passed".to_owned()),
            ("flaky".into(), "passed".into()),
            ("custom".into(), "passed".into()),
            ("absent".into(), "unavailable".into()),
        ]
    );
    // HS2-YYNW9F: what the peer offers is read through the gate, and this
    // project's own (absent) file is checked locally.
    let offered = ok(&worker, &["profiles", "--target", TARGET]);
    assert_eq!(offered["valid"], true, "{offered}");
    let names: Vec<_> = offered["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|profile| profile["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["smoke", "flaky", "held"]);
    let own = ok(&worker, &["profiles"]);
    assert_eq!(
        (own["present"].clone(), own["valid"].clone()),
        (json!(false), json!(true))
    );
    // HS2-17ABM2: a running step is stopped on the peer by a cancel that asks nobody,
    // and ends `cancelled`, not `failed`.
    ok(
        &worker,
        &[
            "define",
            "--ticket",
            &orchestration.slug,
            "--step",
            "held",
            "--target",
            TARGET,
            "--profile",
            "held",
        ],
    );
    let held = ok(
        &worker,
        &[
            &["run"][..],
            &step("held"),
            &["--operation-id", "op-val-held", "--no-wait"],
        ]
        .concat(),
    );
    assert_eq!(held["result"]["state"], "running", "{held}");
    // HS2-5EZZPH: the CLI repeats its bounded waits until the host confirms the stop.
    // A host too busy to confirm within one wait answers `unverified`, and the same
    // cancel asks again; the contract is "the same cancel again waits further".
    let mut cancelled = ok(&worker, &[&["cancel"][..], &step("held")].concat());
    for _ in 0..10 {
        if cancelled["result"]["state"] != "running" {
            break;
        }
        assert!(cancelled.get("unverified").is_some(), "{cancelled}");
        cancelled = ok(&worker, &[&["cancel"][..], &step("held")].concat());
    }
    assert_eq!(cancelled["result"]["state"], "cancelled", "{cancelled}");
    assert_eq!(cancelled["result"]["outcome"]["end"], "cancelled");
    assert_eq!(
        cancelled["result"]["result_id"],
        held["result"]["result_id"]
    );
    std::fs::write(peer_project.join("release"), "").unwrap();
    assert!(!peer_project.join("held-ends").exists());
    assert_eq!(
        ok(&worker, &[&["cancel"][..], &step("held")].concat())["result"],
        cancelled["result"]
    );
    // The peer narrows the pairing to read-only: it refuses to run anything, and the
    // step is unavailable rather than failed.
    set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadOnly).unwrap();
    let refused = ok(
        &worker,
        &[
            &["run"][..],
            &step("smoke"),
            &["--operation-id", "op-val-5"],
        ]
        .concat(),
    );
    assert_eq!(refused["result"]["state"], "unavailable", "{refused}");
    assert_eq!(refused["result"]["reason"], "target_provider_unauthorized");
    assert_eq!(
        std::fs::read_to_string(peer_project.join("smoke-runs"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    // Removing a step is the worker's with its grant, and gone means gone.
    let removed = ok(&worker, &[&["remove"][..], &step("absent")].concat());
    assert_eq!(removed["step"]["definition"]["step_id"], "absent");
    assert!(
        fail(&worker, &[&["show"][..], &step("absent")].concat())
            .contains("validation_step_not_found")
    );
    let audit = cli_named(
        &home,
        &human,
        &["audit", "--project", &owner.id, "--limit", "500"],
    )
    .to_string();
    for expected in [
        "validation_define",
        "validation_run",
        "validation_remove",
        "denied",
    ] {
        assert!(audit.contains(expected), "{expected}");
    }
    assert!(!audit.contains("synthetic-secret"));
    peer.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_wn0mxz_stdio_mcp_defines_runs_and_reads_validation_on_a_live_service() {
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
    let store = FsStore::open(&source).unwrap();
    let orchestration = ticket(&store, "TST", "Validate through MCP");
    profiles(
        &project,
        &[
            ("smoke", "printf ok; echo x >> smoke-runs"),
            ("gated", "while [ ! -f go ]; do sleep 0.05; done"),
            ("held", "while [ ! -f never ]; do sleep 0.05; done"),
        ],
    );
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
        json!({"actor_id":"scheduler-1","actions":["validation_run"],
            "expires_at_ms":expires,"actor":{"role":"human","id":"maintainer"}}),
    )
    .await;
    // The validation routes need the bearer like everything else.
    let hosted = format!("/checkouts/{}/target-validations", checkout.id);
    for (method, path) in [
        ("POST", format!("{base}/validations/list")),
        ("POST", format!("{base}/validations/run")),
        ("POST", format!("{base}/validations/cancel")),
        ("POST", format!("{base}/validations/profiles")),
        (
            "GET",
            format!("/checkouts/{}/target-validation-profiles", checkout.id),
        ),
        ("POST", hosted.clone()),
        ("DELETE", hosted.clone()),
        ("GET", format!("{hosted}?execution_id={TARGET}")),
    ] {
        assert_eq!(
            api(router.clone(), method, &path, json!({}), false).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    // The hosted half answers in the contract's shape and validates before it starts.
    let (status, unknown) = api(
        router.clone(),
        "GET",
        &format!("{hosted}?execution_id={TARGET}"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unknown, json!({"execution_id":TARGET,"state":"unknown"}));
    let (status, invalid) = api(
        router.clone(),
        "POST",
        &hosted,
        json!({"execution_id":TARGET,"check":{"kind":"profile","name":"smoke"},"timeout_seconds":0}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid["code"], "invalid_request");
    // HS2-17ABM2: the hosted stop answers an id it does not know as unknown.
    let (status, stopped) = api(
        router.clone(),
        "DELETE",
        &hosted,
        json!({"execution_id":TARGET,"wait_ms":0}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stopped, json!({"execution_id":TARGET,"state":"unknown"}));
    let (status, unsupported) = api(
        router.clone(),
        "POST",
        &format!("{base}/validations/explode"),
        json!({}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{unsupported}");
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
    let of = json!({"native_id":orchestration.slug});
    // HS2-WD475E: a call waits a bounded time; the same call again waits further, as
    // the tool says, which a loaded machine may need before a step's end is recorded.
    let settled = |arguments: Value| {
        let mut answer = call(arguments.clone());
        for _ in 0..5 {
            if answer["result"]["state"] != "running" {
                break;
            }
            answer = call(arguments.clone());
        }
        answer
    };
    let defined = call(
        json!({"action":"validation_define","target_id":TARGET,"ticket":of,
        "step":{"step_id":"smoke","check":{"kind":"profile","name":"smoke"},
            // The validated target_id argument wins over this.
            "target_id":"01ARZ3NDEKTSV4RRFFQ69G5FZZ"}}),
    );
    assert_eq!(defined["state"], "unverified");
    assert_eq!(defined["step"]["definition"]["target_id"], TARGET);
    let run = json!({"action":"validation_run","ticket":of,"step_id":"smoke",
        "operation_id":"op-mcp-val","wait_ms":8000});
    let passed = settled(run.clone());
    assert_eq!(passed["result"]["state"], "passed", "{passed}");
    assert_eq!(passed["result"]["outcome"]["stdout"]["sha256"], OK_SHA256);
    assert_eq!(passed["attempts_left"], 0);
    assert_eq!(call(run)["result"], passed["result"]);
    assert_eq!(
        std::fs::read_to_string(project.join("smoke-runs"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let evidence = call(json!({"action":"validation_evidence","ticket":of,"step_id":"smoke"}));
    assert_eq!(evidence["state"], "passed");
    assert_eq!(evidence["latest"], passed["result"]);
    // A step still running answers `running`; the same call again waits on it, and the
    // server that hosts the execution keeps it between calls.
    call(
        json!({"action":"validation_define","target_id":TARGET,"ticket":of,
        "step":{"step_id":"gated","check":{"kind":"profile","name":"gated"},"prerequisites":["smoke"]}}),
    );
    let gated = json!({"action":"validation_run","ticket":of,"step_id":"gated",
        "operation_id":"op-mcp-gated","wait_ms":0});
    let started = call(gated.clone());
    assert_eq!(started["result"]["state"], "running");
    assert_eq!(started["result"]["host_started"], true);
    assert_eq!(started["evidence"]["state"], "running");
    std::fs::write(project.join("go"), "").unwrap();
    let mut waiting = gated.clone();
    waiting["wait_ms"] = json!(8000);
    let finished = settled(waiting);
    assert_eq!(finished["result"]["state"], "passed", "{finished}");
    assert_eq!(
        finished["result"]["result_id"],
        started["result"]["result_id"]
    );
    let listed = call(json!({"action":"validation_list","ticket":of}));
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert_eq!(
        call(json!({"action":"validation_list"}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let removed = call(json!({"action":"validation_remove","ticket":of,"step_id":"gated"}));
    assert_eq!(removed["step"]["definition"]["step_id"], "gated");
    assert_eq!(
        call(json!({"action":"validation_list","ticket":of}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // HS2-17ABM2: cancelling a running step through MCP.
    call(
        json!({"action":"validation_define","target_id":TARGET,"ticket":of,
        "step":{"step_id":"held","check":{"kind":"profile","name":"held"}}}),
    );
    let held = call(
        json!({"action":"validation_run","ticket":of,"step_id":"held",
        "operation_id":"op-mcp-held","wait_ms":0}),
    );
    assert_eq!(held["result"]["state"], "running", "{held}");
    let cancelled = settled(
        json!({"action":"validation_cancel","ticket":of,"step_id":"held",
        "wait_ms":8000}),
    );
    assert_eq!(cancelled["result"]["state"], "cancelled", "{cancelled}");
    assert_eq!(cancelled["evidence"]["state"], "cancelled");
    assert_eq!(
        cancelled["result"]["result_id"],
        held["result"]["result_id"]
    );
    // HS2-YYNW9F: the profiles a target offers, and this checkout's own, checked.
    let offered = call(json!({"action":"validation_profiles","target_id":TARGET}));
    assert_eq!(offered["valid"], true, "{offered}");
    assert_eq!(offered["profiles"].as_array().unwrap().len(), 3);
    assert_eq!(call(json!({"action":"validation_profiles"})), offered);
    for arguments in [
        // No step object, no target, an unknown step, no identified actor, an unknown
        // action.
        json!({"action":"validation_define","checkout":checkout.id,"target_id":TARGET,"ticket":of,
            "actor_role":"ai","actor_id":"scheduler-1"}),
        json!({"action":"validation_define","checkout":checkout.id,"ticket":of,
            "step":{"step_id":"x","check":{"kind":"profile","name":"smoke"}},
            "actor_role":"ai","actor_id":"scheduler-1"}),
        json!({"action":"validation_evidence","checkout":checkout.id,"ticket":of,"step_id":"gated"}),
        json!({"action":"validation_run","checkout":checkout.id,"ticket":of,"step_id":"smoke",
            "operation_id":"op-other"}),
        json!({"action":"validation_explode","checkout":checkout.id}),
        json!({"action":"validation_cancel","checkout":checkout.id,"ticket":of,"step_id":"held",
            "actor_role":"ai","actor_id":"scheduler-1","operation_id":"op-none"}),
    ] {
        mcp_rejected(&home, &backend, "hotsheet_targets", arguments);
    }
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}

/// HS2-ZCR3Y7: a step's small output is kept on the machine that ran it and read back
/// by digest through the real service (bearer required) and the owned CLI; the hosted
/// start answers digests only, never the bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_zcr3y7_kept_output_is_read_by_digest_on_the_hosting_machine() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let router = app(host);
    let (status, mut started) = api(
        router.clone(),
        "POST",
        &format!("/checkouts/{}/target-validations", checkout.id),
        json!({"execution_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "check":{"kind":"command","program":"/bin/sh","args":["-c","printf ok"]},
            "timeout_seconds":20,"wait_ms":8000}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    // HS2-M7EV18: the step runs in a detached runner; a loaded machine may start it
    // after the first bounded wait, so the same execution is waited on again.
    for _ in 0..6 {
        if started["state"] != "running" {
            break;
        }
        started = api(
            router.clone(),
            "GET",
            &format!(
                "/checkouts/{}/target-validations?execution_id=01ARZ3NDEKTSV4RRFFQ69G5FAV&wait_ms=8000",
                checkout.id
            ),
            Value::Null,
            true,
        )
        .await
        .1;
    }
    assert_eq!(
        started["outcome"]["stdout"]["sha256"], OK_SHA256,
        "{started}"
    );
    assert!(!started.to_string().contains("\"ok\""));
    let path = format!(
        "/checkouts/{}/target-validation-output/{OK_SHA256}",
        checkout.id
    );
    assert_eq!(
        api(router.clone(), "GET", &path, Value::Null, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, kept) = api(router.clone(), "GET", &path, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK, "{kept}");
    assert_eq!(kept["bytes"], 2);
    assert_eq!(kept["content_base64"], "b2s=");
    let unknown = format!(
        "/checkouts/{}/target-validation-output/{}",
        checkout.id,
        "0".repeat(64)
    );
    assert_eq!(
        api(router.clone(), "GET", &unknown, Value::Null, true)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // The owned CLI on the same machine reads it without a server.
    let read = cli_named(
        &home,
        &["-C", source.to_str().unwrap(), "orchestration"],
        &["validation", "output", OK_SHA256, "--project", &checkout.id],
    );
    assert_eq!(read["content_base64"], "b2s=");
}
