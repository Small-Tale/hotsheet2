//! HS2-J8PRTV: target capacity and admission against a real second server over
//! authenticated TLS through the owned CLI process, and through the stdio MCP tool and
//! a headless CLI on a live local service. Scripted host readings only.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::StoreMetadata;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FF0";
const MIB: u64 = 1024 * 1024;

/// A machine with 6000 MB free of 16 GB, 20% busy, plenty of disk and codex installed.
struct Fixed;
impl hotsheet_server::CapacitySource for Fixed {
    fn machine(&mut self) -> hotsheet_server::MachineReading {
        hotsheet_server::MachineReading {
            memory_free_bytes: Some(6000 * MIB),
            memory_total_bytes: Some(16_384 * MIB),
            cpu_percent: Some(20.0),
        }
    }
    fn disk(&mut self, _: &Path) -> Option<(u64, u64)> {
        Some((80_000 * MIB, 500_000 * MIB))
    }
    fn resident_bytes(&mut self, _: &[u32]) -> HashMap<u32, u64> {
        HashMap::new()
    }
    fn tools(&mut self, _: &[PathBuf]) -> Option<Vec<(String, bool)>> {
        Some(vec![("codex".into(), true)])
    }
    /// HS2-ZY4Z4G: Windows on Arm with cargo, whatever toolchains its owner declares.
    fn capabilities(
        &mut self,
        _: &[PathBuf],
        toolchains: &[String],
    ) -> Option<hotsheet_ticketing::orchestration::HostCapabilities> {
        use hotsheet_ticketing::orchestration::{HostCapabilities, OsFamily, ToolchainCapability};
        Some(HostCapabilities {
            os_version: Some("10.0.26100".into()),
            toolchains: Some(
                toolchains
                    .iter()
                    .map(|name| ToolchainCapability {
                        name: name.clone(),
                        installed: name == "cargo",
                        version: (name == "cargo").then(|| "1.97.0".into()),
                    })
                    .collect(),
            ),
            ..HostCapabilities::bare(OsFamily::Windows, "arm64")
        })
    }
}

/// Three sessions per machine and codex permitted on the target.
fn policy() -> Value {
    json!({"machine":{"max_sessions":3,"min_free_ram_mb":2048,"max_cpu_percent":90,
            "min_free_disk_mb":5120},
        "tools":[{"tool":"codex","max_sessions":3}],
        "targets":[{"target_id":TARGET,"actions":["gates_observe","sessions_manage"],
            "tools":[{"tool":"codex"}]}]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_j8prtv_real_tls_remote_capacity_and_admission_through_the_cli() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-orchestrator").unwrap();
    set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadWrite).unwrap();
    let peer_home = root.path().join("peer-home");
    let peer_source = root.path().join("peer-store");
    let peer_state = state(&peer_source, &peer_home).with_capacity_source(Box::new(Fixed));
    let samples = peer_state.clone();
    let peer_project = root.path().join("peer-project");
    std::fs::create_dir(&peer_project).unwrap();
    let peer_checkout = CheckoutRegistry::new(peer_home.join("checkouts.json"))
        .register(&peer_project, None, None, vec![peer_source.clone()])
        .unwrap();
    let expected = identity(&peer_home);
    let peer = serve(peer_state, &paths, None).await;
    let home = root.path().join("controller-home");
    std::fs::create_dir(&home).unwrap();
    let source = root.path().join("controller-store");
    FsStore::init_git(&source, &StoreMetadata::new("OWN")).unwrap();
    let project = root.path().join("owner");
    std::fs::create_dir(&project).unwrap();
    let owner = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
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
    let scheduler = [
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
        let (action, tool) = ("session_start", Some("codex"));
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
    let policy_file = home.join("policy.json");
    std::fs::write(&policy_file, policy().to_string()).unwrap();
    cli_named(
        &home,
        &human,
        &[
            "policy",
            "set",
            "--project",
            &owner.id,
            "--file",
            policy_file.to_str().unwrap(),
        ],
    );
    // Nothing so far made the peer measure itself.
    assert_eq!(samples.capacity_samples(), 0);

    let show = [
        "capacity",
        "show",
        "--project",
        &owner.id,
        "--target",
        TARGET,
        "--claims",
        "builds",
    ];
    let snapshot = cli_named(&home, &scheduler, &show);
    // The peer's identity and the peer's own readings.
    assert_eq!(snapshot["server_id"], expected.server_id.as_str());
    assert_eq!(snapshot["checkout_id"], peer_checkout.id.as_str());
    assert_eq!(snapshot["memory"], json!({"free_mb":6000,"total_mb":16384}));
    assert_eq!(snapshot["cpu"]["load_percent"], 20);
    assert_eq!(snapshot["disk"]["free_mb"], 80_000);
    assert_eq!(
        snapshot["tools"],
        json!([{"tool":"codex","installed":true,"configured":true}])
    );
    assert_eq!(snapshot["sessions"], json!([]));
    assert_eq!(snapshot["claims"], json!([]));
    assert_eq!(snapshot["claim_namespaces"], json!(["builds"]));
    assert_eq!(samples.capacity_samples(), 1);
    // HS2-ZY4Z4G: what the peer is travels with its capacity, for the default toolchains.
    let capabilities = &snapshot["capabilities"];
    assert_eq!(
        (
            &capabilities["os"],
            &capabilities["os_version"],
            &capabilities["arch"]
        ),
        (&json!("windows"), &json!("10.0.26100"), &json!("arm64"))
    );
    let declared = capabilities["toolchains"].as_array().unwrap();
    assert_eq!(declared.len(), 9);
    assert_eq!(
        declared[1],
        json!({"name":"cargo","installed":true,"version":"1.97.0"})
    );
    assert_eq!(samples.capability_detections(), 1);
    // No path, origin or credential of the peer travels with its capacity.
    let wire = snapshot.to_string();
    for private in [
        SECRET,
        peer.origin.as_str(),
        peer_project.to_str().unwrap(),
        peer_home.to_str().unwrap(),
    ] {
        assert!(!wire.contains(private), "{private}");
    }

    let check = |tool: &str, sessions: &str| {
        cli_named(
            &home,
            &scheduler,
            &[
                "admission",
                "check",
                "--project",
                &owner.id,
                "--target",
                TARGET,
                "--tool",
                tool,
                "--sessions",
                sessions,
            ],
        )
    };
    // 6000 free - 2048 floor fits three 1024 MB sessions; the policy caps at three.
    let accepted = check("codex", "3");
    assert_eq!(accepted["outcome"], "accepted");
    assert_eq!(accepted["admitted"], 3);
    assert_eq!(accepted["reason"], Value::Null);
    assert_eq!(accepted["limits"]["max_sessions"], 3);
    assert_eq!(accepted["footprint_mb"], 1024);
    let reduced = check("codex", "5");
    assert_eq!(reduced["outcome"], "reduced");
    assert_eq!(reduced["admitted"], 3);
    assert_eq!(reduced["reason"], "session_cap_reached");

    // The peer's owner tightens its own machine: idempotent, a person's act, and it
    // wins over the orchestrator's policy.
    let peer_owner = [
        "--actor-role",
        "human",
        "--actor-id",
        "peer-owner",
        "orchestration",
        "admission",
    ];
    let set = [
        "set",
        "--max-sessions",
        "1",
        "--tool-footprint",
        "codex=3000",
    ];
    let first = cli_named(&peer_home, &peer_owner, &set);
    assert_eq!(first["configured"], true);
    assert_eq!(first["admission"]["max_sessions"], 1);
    assert_eq!(first["admission"]["tool_footprint_mb"]["codex"], 3000);
    let file = peer_home.join("orchestration-admission.json");
    let written = std::fs::read(&file).unwrap();
    assert_eq!(cli_named(&peer_home, &peer_owner, &set), first);
    assert_eq!(std::fs::read(&file).unwrap(), written);
    assert_eq!(cli_named(&peer_home, &peer_owner, &["show"]), first);
    let forbidden = cli_failure(
        &peer_home,
        &[
            "--actor-role",
            "ai",
            "--actor-id",
            "scheduler-1",
            "orchestration",
            "admission",
        ],
        &["set", "--max-sessions", "64"],
    );
    assert!(forbidden.contains("forbidden"), "{forbidden}");
    assert_eq!(std::fs::read(&file).unwrap(), written);
    let stricter = check("codex", "2");
    assert_eq!(stricter["outcome"], "reduced");
    assert_eq!(stricter["admitted"], 1);
    assert_eq!(stricter["limits"]["max_sessions"], 1);
    assert_eq!(stricter["footprint_mb"], 3000);
    assert_eq!(stricter["footprint_source"], "configured");
    cli_named(
        &peer_home,
        &peer_owner,
        &[
            "set",
            "--clear",
            "max_sessions",
            "--clear",
            "tool_footprint:codex",
        ],
    );
    assert_eq!(check("codex", "2")["outcome"], "accepted");

    // HS2-ZY4Z4G: the peer's owner declares its own toolchains, and the next read
    // reports exactly those; clearing them restores the default list.
    let declared = cli_named(
        &peer_home,
        &peer_owner,
        &["set", "--toolchain", "cargo", "--toolchain", "xcode"],
    );
    assert_eq!(
        declared["admission"]["toolchains"],
        json!(["cargo", "xcode"])
    );
    let reported = cli_named(&home, &scheduler, &show);
    assert_eq!(
        reported["capabilities"]["toolchains"],
        json!([{"name":"cargo","installed":true,"version":"1.97.0"},
            {"name":"xcode","installed":false}])
    );
    let cleared = cli_named(&peer_home, &peer_owner, &["set", "--clear", "toolchains"]);
    assert!(cleared["admission"].get("toolchains").is_none());

    // HS2-XHQ1EH: asking starts nothing, so nobody is asked and no rule is needed:
    // an actor no rule names reads the peer's capacity too.
    let stranger = [
        "--actor-role",
        "ai",
        "--actor-id",
        "stranger",
        "-C",
        store_arg,
        "orchestration",
    ];
    assert!(cli_named(&home, &stranger, &show).is_object());

    // Headless on the controller, where no server hosts sessions: every value is
    // unknown and admission fails closed.
    let local = cli_named(
        &home,
        &scheduler,
        &["capacity", "show", "--checkout", &owner.id],
    );
    assert_eq!(local["sampled_at_ms"], Value::Null);
    assert_eq!(local["memory"]["free_mb"], Value::Null);
    assert_eq!(local["sessions"], Value::Null);
    let closed = cli_named(
        &home,
        &scheduler,
        &[
            "admission",
            "check",
            "--checkout",
            &owner.id,
            "--tool",
            "codex",
        ],
    );
    assert_eq!(closed["outcome"], "refused");
    assert_eq!(closed["reason"], "capacity_unknown");

    // A read-only device may still read capacity but cannot ask for admission.
    set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadOnly).unwrap();
    assert_eq!(
        cli_named(&home, &scheduler, &show)["memory"]["free_mb"],
        6000
    );
    let fail = |prefix: &[&str], arguments: &[&str]| cli_failure(&home, prefix, arguments);
    let refused = fail(
        &scheduler,
        &[
            "admission",
            "check",
            "--project",
            &owner.id,
            "--target",
            TARGET,
            "--tool",
            "codex",
        ],
    );
    assert!(refused.contains("target_peer_read_only"), "{refused}");
    // Every answer and refusal is in the controller's journal, by code only.
    let audit = cli_named(&home, &human, &["audit", "--project", &owner.id]);
    let entries = audit["entries"].as_array().unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| { entry["action"] == "gates_observe" && entry["outcome"] == "succeeded" })
    );
    assert!(entries.iter().any(|entry| {
        entry["action"] == "sessions_manage"
            && entry["outcome"] == "failed"
            && entry["reason"] == "target_peer_read_only"
    }));
    peer.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_j8prtv_stdio_mcp_and_headless_cli_read_a_live_local_servers_capacity() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home).with_capacity_source(Box::new(Fixed));
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
    let target = format!("{base}/targets/{TARGET}");
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
        &format!("{target}/binding"),
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
    let person = json!({"role":"human","id":"maintainer"});
    let granted = super::orchestration::remember_rules(
        router.clone(),
        &target.to_string(),
        json!({"actor_id":"scheduler-1","actions":["gates_observe","sessions_manage"],
            "session_scope":{"tools":[{"tool":"codex"}],"max_concurrency":3},
            "expires_at_ms":expires,"actor":person}),
    )
    .await;
    assert_eq!(granted.0, StatusCode::OK, "{}", granted.1);
    let stored = api(
        router.clone(),
        "PUT",
        &format!("{base}/policy"),
        json!({"expected_revision":null,"policy":policy(),"actor":person}),
        true,
    )
    .await;
    assert_eq!(stored.0, StatusCode::OK, "{}", stored.1);
    // The new routes need the bearer like everything else.
    let hosted = format!("/checkouts/{}", checkout.id);
    for (method, path) in [
        ("GET", format!("{hosted}/target-capacity")),
        ("POST", format!("{hosted}/target-admission")),
        ("GET", format!("{target}/capacity")),
        ("POST", format!("{target}/capacity")),
        ("POST", format!("{target}/admission")),
    ] {
        assert_eq!(
            api(router.clone(), method, &path, json!({}), false).await.0,
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
    assert_eq!(host.capacity_samples(), 0);
    // Plain HTTP: the gated read takes its actor in the query.
    let read = api(
        router.clone(),
        "GET",
        &format!("{target}/capacity?actor_id=scheduler-1&actor_role=ai"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(read.0, StatusCode::OK, "{}", read.1);
    assert_eq!(read.1["memory"]["free_mb"], 6000);
    let unidentified = api(
        router.clone(),
        "GET",
        &format!("{target}/capacity"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(unidentified.0, StatusCode::BAD_REQUEST);
    assert_eq!(unidentified.1["code"], "invalid_request");
    let ungranted = api(
        router.clone(),
        "POST",
        &format!("{target}/admission"),
        json!({"tool":"codex","actor":{"role":"ai","id":"stranger"}}),
        true,
    )
    .await;
    // HS2-XHQ1EH: the question starts nothing and asks no one.
    assert_eq!(ungranted.0, StatusCode::OK, "{}", ungranted.1);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    // The machine-local record a headless process finds this live server by.
    let instance = hotsheet_cli::external_launch::instance_path(&home, &source);
    std::fs::create_dir_all(instance.parent().unwrap()).unwrap();
    std::fs::write(
        &instance,
        json!({"pid":std::process::id(),"url":origin,"secret":SECRET,
            "store_path":hotsheet_sync::paths::canonicalize(&source).unwrap()})
        .to_string(),
    )
    .unwrap();
    let served = host.clone();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app(served))
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
    let snapshot = call(json!({"action":"capacity","target_id":TARGET,
        "claim_namespaces":["builds"]}));
    assert_eq!(snapshot["checkout_id"], checkout.id.as_str());
    assert_eq!(snapshot["memory"]["free_mb"], 6000);
    assert_eq!(snapshot["claims"], json!([]));
    // HS2-YDZAFH: a claim held on this machine is listed by every client when no
    // namespace is named: the stdio MCP tool, the HTTP route and the CLI.
    hotsheet_resource_claims::ClaimService::new(&home)
        .acquire(
            &hotsheet_resource_claims::ResourceRef::parse("vm/macos-vm").unwrap(),
            std::time::Duration::from_secs(600),
            None,
            &std::collections::BTreeMap::new(),
        )
        .unwrap();
    let all = call(json!({"action":"capacity","target_id":TARGET}));
    assert_eq!(all["claim_namespaces"], json!(["vm"]));
    assert_eq!(all["claims"][0]["resource"], "macos-vm");
    let (status, read) = api(
        router.clone(),
        "GET",
        &format!("{target}/capacity?actor_id=scheduler-1&actor_role=ai"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["claims"][0]["namespace"], "vm");
    let decision = call(json!({"action":"admission_check","target_id":TARGET,
        "tool":"codex","sessions":4}));
    assert_eq!(decision["outcome"], "reduced");
    assert_eq!(decision["admitted"], 3);
    assert_eq!(decision["reason"], "session_cap_reached");
    for arguments in [
        json!({"action":"capacity","checkout":checkout.id,"target_id":TARGET}),
        json!({"action":"admission_check","checkout":checkout.id,"target_id":TARGET,
            "tool":"codex","sessions":0,"actor_role":"ai","actor_id":"scheduler-1"}),
    ] {
        mcp_rejected(&home, &backend, "hotsheet_targets", arguments);
    }

    // A headless CLI hosts no sessions: it asks this live server for its snapshot,
    // then decides from the same machine's admission file.
    let store_arg = source.to_str().unwrap();
    let headless = [
        "--actor-role",
        "ai",
        "--actor-id",
        "scheduler-1",
        "-C",
        store_arg,
        "orchestration",
    ];
    let local = cli_named(
        &home,
        &headless,
        &["capacity", "show", "--checkout", &checkout.id],
    );
    assert_eq!(local["memory"]["free_mb"], 6000);
    assert_eq!(local["sessions"], json!([]));
    assert!(local["sampled_at_ms"].is_u64());
    let through_target = cli_named(
        &home,
        &headless,
        &[
            "admission",
            "check",
            "--project",
            &checkout.id,
            "--target",
            TARGET,
            "--tool",
            "codex",
            "--sessions",
            "2",
        ],
    );
    assert_eq!(through_target["outcome"], "accepted");
    assert_eq!(through_target["admitted"], 2);
    // With the server gone the same command knows nothing and fails closed.
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    std::fs::remove_file(&instance).unwrap();
    let gone = cli_named(
        &home,
        &headless,
        &[
            "admission",
            "check",
            "--checkout",
            &checkout.id,
            "--tool",
            "codex",
        ],
    );
    assert_eq!(gone["outcome"], "refused");
    assert_eq!(gone["reason"], "capacity_unknown");
}

/// HS2-P5GTSW: the hosted capacity read is a long poll by version on a live service:
/// a current version waits for its bound, a stale one answers at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_p5gtsw_capacity_waits_by_version_on_a_live_service() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let checkout =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"))
            .register(&project, None, None, vec![source.clone()])
            .unwrap();
    let router = app(host);
    let api = super::orchestration::api;
    let route = format!("/checkouts/{}/target-capacity", checkout.id);
    let (status, first) = api(router.clone(), "GET", &route, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let version = first["version"].as_str().unwrap().to_owned();
    let began = std::time::Instant::now();
    let (status, stale) = api(
        router.clone(),
        "GET",
        &format!("{route}?after=0000000000000000&wait_ms=8000"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(began.elapsed() < std::time::Duration::from_secs(4));
    assert!(stale["version"].is_string());
    let (status, bad) = api(
        router.clone(),
        "GET",
        &format!("{route}?after=not%20hex&wait_ms=10"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    assert_eq!(
        api(
            router,
            "GET",
            &format!("{route}?after={version}&wait_ms=10"),
            Value::Null,
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
