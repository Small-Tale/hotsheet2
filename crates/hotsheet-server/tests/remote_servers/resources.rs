//! HS2-2FG91X: declaring required resources and reading contention through the owned
//! CLI process, the authenticated routes of a real server whose loop is woken by a
//! release, and the stdio MCP tool on the live service. The claims are taken and
//! released through the claim service's own surfaces, as a session would; the
//! scheduler never claims. Synthetic stores, tools, resources and identities only.
use super::*;

const VM: &str = "vm/macos-vm";

/// A policy that requires the VM of every piece of work on the target.
fn requiring(automatic: bool) -> Value {
    let mut body = policy(automatic, true);
    body["placement"]["resources"] = json!([{"target_id": TARGET,
        "resources": [{"namespace": "vm", "resource": "macos-vm"}]}]);
    body
}

/// The bare CLI with no server: a step and a policy declare resources, the claim
/// CLI takes and releases one, and plan and explain say what waits on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_2fg91x_cli_declares_resources_and_explains_contention() {
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
    let first = ticket(&store, "Synthetic work in the VM");
    let second = ticket(&store, "Synthetic work on the target");
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
    let define = |slug: &str, resources: &[&str], valid: bool| {
        let mut all = vec![
            "readiness",
            "define",
            "--ticket",
            &orchestration.slug,
            "--target",
            TARGET,
            "--connection",
            &connection,
            "--id",
            slug,
            "--project",
            &owner.id,
        ];
        for resource in resources {
            all.extend_from_slice(&["--resource", resource]);
        }
        if valid {
            cli_named(&home, &worker, &all)
        } else {
            cli_failure(&home, &worker, &all);
            Value::Null
        }
    };

    // A step declares what its session must hold; the plan stores it sorted.
    let step = define(&first.slug, &[VM, "ports/8080"], true);
    assert_eq!(step["level"], "confirmed", "{step}");
    assert_eq!(
        step["step"]["resources"],
        json!([{"namespace": "ports", "resource": "8080"},
            {"namespace": "vm", "resource": "macos-vm"}])
    );
    let step_id = step["step"]["step_id"].as_str().unwrap().to_owned();
    // The same declaration again changes nothing; without the flag it is withdrawn.
    let again = define(&first.slug, &["ports/8080", VM], true);
    assert_eq!(again["step"]["revision"], step["step"]["revision"]);
    let plain = define(&second.slug, &[], true);
    assert!(plain["step"].get("resources").is_none(), "{plain}");
    let plain_id = plain["step"]["step_id"].as_str().unwrap().to_owned();
    let ready = cli_named(
        &home,
        &worker,
        &["readiness", "ready", "--project", &owner.id],
    );
    assert_eq!(
        ready
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["step_id"] == step_id)
            .unwrap()["resources"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // A malformed requirement is refused, by the CLI or by the shared runtime.
    for bad in ["macos-vm", "vm/", "VM/macos-vm", "vm/has space"] {
        define(&first.slug, &[bad], false);
    }
    define(&first.slug, &["a/1", "a/2", "a/3", "a/4", "a/5"], false);

    // The policy requires the VM of everything on the target.
    let file = root.path().join("policy.json");
    let set_policy = |body: Value| {
        std::fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
        let arguments = [
            "policy",
            "set",
            "--project",
            &owner.id,
            "--file",
            file.to_str().unwrap(),
        ];
        (body, arguments)
    };
    let (_, arguments) = set_policy(requiring(false));
    let stored = cli_named(&home, &human, &arguments);
    assert_eq!(
        stored["policy"]["placement"]["resources"][0]["resources"],
        json!([{"namespace": "vm", "resource": "macos-vm"}])
    );
    let mut invalid = requiring(false);
    invalid["placement"]["resources"][0]["resources"][0]["namespace"] = json!("V M");
    let (_, arguments) = set_policy(invalid);
    cli_failure(&home, &human, &arguments);

    // Someone holds the VM, taken with the claim CLI as a session would.
    let claims = ["resource"];
    let grant = cli_named(&home, &claims, &["acquire", VM, "--duration", "30m"]);
    assert_eq!(grant["acquired"], true, "{grant}");
    let expires = grant["expires_at_ms"].as_u64().unwrap();
    let token = grant["renewal_token"].as_str().unwrap().to_owned();

    let plan = scheduler(&["plan"]);
    assert_eq!(plan["mode"], "dry_run");
    let reasons: Vec<_> = plan["unplaced"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (item["reason"].clone(), item["detail"].clone()))
        .collect();
    assert_eq!(
        reasons,
        vec![(json!("resource_held"), json!(VM)); 2],
        "{plan}"
    );
    assert_eq!(plan["retry_at_ms"], json!(expires));
    assert_eq!(plan["inputs"]["held_resources"], 1);
    let contention = plan["contention"].as_array().unwrap();
    assert_eq!(contention.len(), 1, "{plan}");
    assert_eq!(
        (
            &contention[0]["namespace"],
            &contention[0]["resource"],
            &contention[0]["state"],
            &contention[0]["expires_at_ms"]
        ),
        (
            &json!("vm"),
            &json!("macos-vm"),
            &json!("held"),
            &json!(expires)
        )
    );
    assert_eq!(contention[0]["waiting"].as_array().unwrap().len(), 2);
    // Only identifiers and a time: never the holder, its token or its metadata.
    let mut keys: Vec<_> = contention[0].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "expires_at_ms",
            "namespace",
            "resource",
            "server_id",
            "state",
            "waiting"
        ]
    );
    assert!(!plan.to_string().contains(&token));

    // Explain says which requirement holds the work back and who declared each.
    let why = scheduler(&["explain", &step_id]);
    assert_eq!(
        (&why["standing"], &why["reason"], &why["detail"]),
        (&json!("not_placed"), &json!("resource_held"), &json!(VM))
    );
    assert_eq!(why["retry_at_ms"], json!(expires));
    let listed: Vec<_> = why["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["resource"].as_str().unwrap(),
                entry["state"].as_str().unwrap(),
                entry["declared_by"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [("8080", "free", "step"), ("macos-vm", "held", "step")]
    );
    let why_plain = scheduler(&["explain", &plain_id]);
    assert_eq!(why_plain["resources"][0]["declared_by"], "target");
    assert_eq!(why_plain["resources"][0]["state"], "held");
    // A logged dry pass records the deferral with the resource and its expiry.
    assert_eq!(scheduler(&["tick"])["logged"], 3);
    let log = scheduler(&["decisions"]);
    assert_eq!(log["entries"][0]["tick"]["held_resources"], 1);
    assert_eq!(log["entries"][0]["tick"]["unreadable_namespaces"], 0);
    assert_eq!(
        (
            &log["entries"][1]["reason"],
            &log["entries"][1]["detail"],
            &log["entries"][1]["retry_at_ms"]
        ),
        (&json!("resource_held"), &json!(VM), &json!(expires))
    );

    // Released with its own token: the resource no longer holds anything back. With
    // no server on this machine the next reason is the unread machine.
    let released = cli_named(&home, &claims, &["release", "--token", &token]);
    assert_eq!(released["released"], true);
    let plan = scheduler(&["plan"]);
    assert!(plan.get("contention").is_none(), "{plan}");
    assert_eq!(plan["unplaced"][0]["reason"], "machine_refused", "{plan}");
    assert_eq!(plan["unplaced"][0]["detail"], "capacity_unknown");
    assert_eq!(
        scheduler(&["explain", &step_id])["resources"][1]["state"],
        "free"
    );
}

/// A real server: the routes take the declaration, the loop defers the work while
/// the VM is held, and a release wakes the loop through the claim service's wait.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_2fg91x_server_loop_defers_held_work_and_wakes_on_release() {
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
    let first = ticket(&store, "Synthetic work in the VM");
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
    let actor = json!({"role":"ai","id":"scheduler-1"});
    let ticket_ref = json!({"native_id": orchestration.slug});
    let define = |native: String, resources: Value| {
        call(
            router.clone(),
            "POST",
            format!("{base}/readiness/define"),
            json!({"actor":actor,"ticket":ticket_ref,"target_id":TARGET,
                "connection_id":connection,"native_id":native,"resources":resources}),
        )
    };
    let send = |operation: &'static str, body: Value| {
        call(
            router.clone(),
            "POST",
            format!("{base}/scheduler/{operation}"),
            body,
        )
    };
    let claim = |operation: &'static str, body: Value| {
        call(
            router.clone(),
            "POST",
            format!("/resource-claims/{VM}/{operation}"),
            body,
        )
    };

    // The route validates the declaration and stores it.
    for invalid in [
        json!([{"namespace": "VM", "resource": "macos-vm"}]),
        json!([{"namespace": "vm", "resource": "macos-vm", "holder": "x"}]),
        json!(["vm/macos-vm"]),
        json!([{"namespace": "vm", "resource": "a"}, {"namespace": "vm", "resource": "a"}]),
    ] {
        let (status, body) = define(first.slug.clone(), invalid.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid}: {body}");
    }
    let need = json!([{"namespace": "vm", "resource": "macos-vm"}]);
    let (status, step) = define(first.slug.clone(), need.clone()).await;
    assert_eq!(status, StatusCode::OK, "{step}");
    assert_eq!(step["step"]["resources"], need);
    let step_id = step["step"]["step_id"].as_str().unwrap().to_owned();

    // A session holds the VM, taken through the server's own claim route.
    let (status, grant) = claim("acquire", json!({"duration_ms": 1_800_000})).await;
    assert_eq!(status, StatusCode::OK, "{grant}");
    assert_eq!(grant["acquired"], true);
    let lease_ends = grant["expires_at_ms"].as_u64().unwrap();
    let token = grant["renewal_token"].as_str().unwrap().to_owned();

    let put_policy = |body: Value, revision: Value| {
        call(
            router.clone(),
            "PUT",
            format!("{base}/policy"),
            json!({"expected_revision":revision,"policy":body,
                "actor":{"role":"human","id":"maintainer"}}),
        )
    };
    let (status, stored) = put_policy(policy(false, true), Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    let (status, plan) = send("plan", json!({"actor":actor})).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["unplaced"][0]["reason"], "resource_held", "{plan}");
    assert_eq!(plan["unplaced"][0]["detail"], VM);
    assert_eq!(plan["unplaced"][0]["retry_at_ms"], json!(lease_ends));
    assert_eq!(plan["contention"][0]["state"], "held");
    assert_eq!(plan["contention"][0]["waiting"], json!([step_id]));
    // The claim is the server's own machine's.
    let (_, capacity) = api(
        router.clone(),
        "GET",
        &format!(
            "/checkouts/{}/target-capacity?claim_namespaces=vm",
            checkout.id
        ),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(plan["contention"][0]["server_id"], capacity["server_id"]);
    assert!(capacity.get("claims_offline").is_none(), "{capacity}");
    let (_, why) = send("explain", json!({"actor":actor,"step_id":step_id})).await;
    assert_eq!(why["reason"], "resource_held");
    assert_eq!(
        why["resources"],
        json!([{"namespace":"vm","resource":"macos-vm","state":"held",
            "server_id":capacity["server_id"],"expires_at_ms":lease_ends,"declared_by":"step"}])
    );

    // Automatic placement: the loop's first pass defers the work for the resource.
    let (status, enabled) = put_policy(policy(true, true), stored["revision"].clone()).await;
    assert_eq!(status, StatusCode::OK, "{enabled}");
    let started = logged(router.clone(), &base, &home, "start", 1).await;
    assert_eq!(started[0]["tick"]["held_resources"], 1);
    assert_eq!(
        (
            &started[1]["kind"],
            &started[1]["reason"],
            &started[1]["detail"]
        ),
        (&json!("unplaced"), &json!("resource_held"), &json!(VM))
    );
    assert_eq!(started[1]["retry_at_ms"], json!(lease_ends));

    // The session releases with its own token. Nothing else happens: no run event,
    // no policy change, and the lease had half an hour to go. The pass that follows
    // was woken by the claim service's release signal.
    let (status, released) = claim("release", json!({"renewal_token": token})).await;
    assert_eq!(status, StatusCode::OK, "{released}");
    let woken = logged(router.clone(), &base, &home, "signal", 1).await;
    assert_eq!(woken[0]["tick"]["held_resources"], 0);
    assert_eq!(woken[1]["step_id"], step_id);
    // No machine has the synthetic tool, which is now the only thing in the way.
    assert_eq!(woken[1]["reason"], "no_tool_available", "{woken:?}");

    // The stdio MCP tool declares a requirement and reads contention on the live
    // service, and the claim tool takes the resource as a session would.
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
    let tool = |name: &str, mut arguments: Value| {
        arguments["actor_role"] = json!("ai");
        arguments["actor_id"] = json!("scheduler-1");
        mcp_named(&home, &backend, name, arguments)
    };
    let targets = |mut arguments: Value| {
        arguments["checkout"] = json!(checkout.id);
        tool("hotsheet_targets", arguments)
    };
    let later = targets(json!({"action":"readiness_define","target_id":TARGET,
        "ticket":ticket_ref,"connection_id":connection,"native_id":second.slug,
        "resources":need}));
    assert_eq!(later["step"]["resources"], need, "{later}");
    let later_id = later["step"]["step_id"].as_str().unwrap().to_owned();
    // Nobody holds it, and no machine has the tool: nothing waits on the resource.
    let plan = targets(json!({"action":"scheduler_plan"}));
    assert!(plan.get("contention").is_none(), "{plan}");
    let free = targets(json!({"action":"scheduler_explain","step_id":later_id}));
    assert_eq!(free["resources"][0]["state"], "free", "{free}");
    let held = tool(
        "hotsheet_resource_claim",
        json!({"action":"acquire","target":VM,"duration":"10m"}),
    );
    assert_eq!(held["acquired"], true, "{held}");
    let plan = targets(json!({"action":"scheduler_plan"}));
    assert_eq!(plan["contention"][0]["state"], "held", "{plan}");
    assert_eq!(plan["contention"][0]["waiting"], json!([step_id, later_id]));
    let why = targets(json!({"action":"scheduler_explain","step_id":later_id}));
    assert_eq!(why["reason"], "resource_held");
    assert_eq!(why["resources"][0]["state"], "held");
    assert_eq!(why["resources"][0]["expires_at_ms"], held["expires_at_ms"]);
    let mut refused = json!({"action":"readiness_define","target_id":TARGET,
        "ticket":ticket_ref,"connection_id":connection,"native_id":second.slug,
        "resources":[{"namespace":"vm","resource":"has space"}],"checkout":checkout.id});
    refused["actor_role"] = json!("ai");
    refused["actor_id"] = json!("scheduler-1");
    mcp_rejected(&home, &backend, "hotsheet_targets", refused);
    let released = tool(
        "hotsheet_resource_claim",
        json!({"action":"release","renewal_token":held["renewal_token"]}),
    );
    assert_eq!(released["released"], true);

    // Automatic placement is turned off: the change wakes the loop, and it ends.
    let (status, _) = put_policy(policy(false, true), enabled["revision"].clone()).await;
    assert_eq!(status, StatusCode::OK);
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}
