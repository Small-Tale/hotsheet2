//! HS2-GDJXGZ: orchestrator-owned sessions against a real second server over
//! authenticated TLS through the owned CLI process, and through the stdio MCP tool and
//! a headless CLI on a live local service. The sessions are real PTYs running the
//! workspace fake agent; no AI tool and no network beyond loopback.
use super::orchestration::api;
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const TARGET_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FG0";
const TARGET_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FG1";
const MIB: u64 = 1024 * 1024;
/// An idle tool, and one that is in the middle of a turn for as long as it lives.
pub(super) const IDLE_TOOL: &str = "fake-idle";
const BUSY_TOOL: &str = "fake-busy";

/// A roomy machine with both fake tools installed.
pub(super) struct Roomy;
impl hotsheet_server::CapacitySource for Roomy {
    fn machine(&mut self) -> hotsheet_server::MachineReading {
        hotsheet_server::MachineReading {
            memory_free_bytes: Some(16_384 * MIB),
            memory_total_bytes: Some(32_768 * MIB),
            cpu_percent: Some(10.0),
        }
    }
    fn disk(&mut self, _: &Path) -> Option<(u64, u64)> {
        Some((80_000 * MIB, 500_000 * MIB))
    }
    fn resident_bytes(&mut self, _: &[u32]) -> HashMap<u32, u64> {
        HashMap::new()
    }
    fn tools(&mut self, _: &[PathBuf]) -> Option<Vec<(String, bool)>> {
        Some(vec![(IDLE_TOOL.into(), true), (BUSY_TOOL.into(), true)])
    }
}

fn workspace_binary(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current test executable");
    let profile = exe
        .parent()
        .and_then(Path::parent)
        .expect("target profile directory");
    profile.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// Two plugins that launch the workspace fake agent: one sits idle, one stays busy.
pub(super) fn fake_plugins(root: &Path) -> PathBuf {
    let fake_agent = workspace_binary("hs-fake-agent");
    assert!(
        fake_agent.is_file(),
        "build the workspace hs-fake-agent binary at {} (cargo build -p hotsheet-terminals --bins into the same target dir)",
        fake_agent.display()
    );
    let plugins = root.join("plugins");
    for (tool, script) in [
        (IDLE_TOOL, r#"["--print", "ready", "--hold-ms", "600000"]"#),
        (
            BUSY_TOOL,
            r#"["--busy", "--print", "working", "--hold-ms", "600000"]"#,
        ),
    ] {
        let plugin = plugins.join(tool);
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("instructions.md"), "Synthetic agent\n").unwrap();
        std::fs::write(
            plugin.join("manifest.toml"),
            format!(
                r#"
id = "{tool}"
display_name = "{tool}"
product_name = "Synthetic agent"
tier = "cli-agent"
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{{store}}"]
[launch]
program = {fake_agent:?}
args = {script}
"#
            ),
        )
        .unwrap();
    }
    plugins
}

pub(super) fn git_project(path: &Path) {
    std::fs::create_dir(path).unwrap();
    let status = hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(path)
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(status.success());
}

fn policy() -> Value {
    json!({"machine":{"max_sessions":4},
        "tools":[{"tool":IDLE_TOOL,"max_sessions":4},{"tool":BUSY_TOOL,"max_sessions":4}],
        "targets":[
            {"target_id":TARGET_A,"actions":["sessions_manage"],"tools":[{"tool":IDLE_TOOL}],
                "max_sessions":2},
            {"target_id":TARGET_B,"actions":["sessions_manage"],"tools":[{"tool":BUSY_TOOL}],
                "max_sessions":2}]})
}

/// The server's terminal list, which is what its bottom drawer renders.
pub(super) async fn drawer(router: &axum::Router) -> Vec<Value> {
    let (status, terminals) = api(router.clone(), "GET", "/terminals", Value::Null, true).await;
    assert_eq!(status, StatusCode::OK);
    terminals.as_array().unwrap().clone()
}

fn tab<'a>(terminals: &'a [Value], id: &Value) -> &'a Value {
    terminals
        .iter()
        .find(|terminal| &terminal["id"] == id)
        .unwrap_or_else(|| panic!("terminal {id} is not in the drawer list"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_gdjxgz_real_tls_sessions_on_two_targets_with_different_tools_through_the_cli() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-orchestrator").unwrap();
    set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadWrite).unwrap();
    let peer_home = root.path().join("peer-home");
    let peer_source = root.path().join("peer-store");
    let peer_state = state(&peer_source, &peer_home)
        .with_capacity_source(Box::new(Roomy))
        .with_capacity_cache(std::time::Duration::ZERO)
        .with_plugin_dirs(vec![fake_plugins(root.path())]);
    let peer_router = app(peer_state.clone());
    let peer_store = FsStore::open(&peer_source).unwrap();
    let registry = CheckoutRegistry::new(peer_home.join("checkouts.json"));
    let mut peer_checkouts = Vec::new();
    for name in ["peer-project-a", "peer-project-b"] {
        let project = root.path().join(name);
        git_project(&project);
        peer_checkouts.push(
            registry
                .register(&project, None, None, vec![peer_source.clone()])
                .unwrap(),
        );
    }
    let expected = identity(&peer_home);
    let peer = serve(peer_state, &paths, None).await;
    let home = root.path().join("controller-home");
    std::fs::create_dir(&home).unwrap();
    let source = root.path().join("controller-store");
    FsStore::init_git(&source, &StoreMetadata::new("OWN")).unwrap();
    let project = root.path().join("release-train");
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
    let mut revision: Option<String> = None;
    for (target, checkout, tool) in [
        (TARGET_A, &peer_checkouts[0], IDLE_TOOL),
        (TARGET_B, &peer_checkouts[1], BUSY_TOOL),
    ] {
        let inspected = cli_named(
            &home,
            &targets,
            &["inspect", &checkout.id, "--registration-id", ID],
        );
        let mut add = vec!["add", "--target-id", target, "--label", "Remote target"];
        if let Some(revision) = &revision {
            add.extend(["--expected-revision", revision.as_str()]);
        }
        let added = cli_named(&home, &targets, &add);
        revision = added["revision"].as_str().map(str::to_owned);
        cli_named(
            &home,
            &targets,
            &[
                "bind",
                target,
                "--expected-manifest-revision",
                revision.as_deref().unwrap(),
                "--expected-binding-revision",
                "0",
                "--destination",
                &inspected["destination"].to_string(),
            ],
        );
        // HS2-XHQ1EH: a person remembers session starts of the target's tool.
        cli_named(
            &home,
            &human,
            &[
                "permission",
                "remember",
                target,
                "--project",
                &owner.id,
                "--action",
                "session_start",
                "--tool",
                tool,
            ],
        );
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
    // Nothing so far started anything on the peer.
    assert!(drawer(&peer_router).await.is_empty());

    let session = |prefix: &[&str], arguments: &[&str]| {
        let mut all = vec!["session"];
        all.extend_from_slice(arguments);
        all.extend(["--project", owner.id.as_str()]);
        cli_named(&home, prefix, &all)
    };
    let refused = |prefix: &[&str], arguments: &[&str]| {
        let mut all = vec!["session"];
        all.extend_from_slice(arguments);
        all.extend(["--project", owner.id.as_str()]);
        cli_failure(&home, prefix, &all)
    };
    let start = |operation: &str, target: &str, tool: &str, extra: &[&str]| {
        let mut arguments = vec![
            "start",
            "--target",
            target,
            "--operation-id",
            operation,
            "--tool",
            tool,
        ];
        arguments.extend_from_slice(extra);
        session(&scheduler, &arguments)
    };

    // Target A gets the idle tool. It runs on the peer, in the peer's drawer, labelled
    // with the orchestration project.
    let a = start("op-a", TARGET_A, IDLE_TOOL, &[]);
    assert_eq!(a["state"], "running");
    assert_eq!(a["owner"], "orchestrator");
    assert_eq!(a["role"], "worker");
    assert_eq!(a["hosted"]["server_id"], expected.server_id.as_str());
    assert_eq!(a["label"], format!("{}: worker", owner.alias).as_str());
    // The fake tool has no way to receive a message from its host.
    assert_eq!(a["role_prompt"], "unsupported");
    let a_terminal = a["hosted"]["session_id"].clone();
    let listed = drawer(&peer_router).await;
    assert_eq!(listed.len(), 1);
    let shown = tab(&listed, &a_terminal);
    assert_eq!(shown["kind"], "ai");
    assert_eq!(shown["alive"], true);
    assert_eq!(shown["tool"], IDLE_TOOL);
    assert_eq!(shown["name"], a["label"]);
    assert_eq!(
        a["hosted"]["worker_id"],
        format!("{IDLE_TOOL}-{}", a_terminal.as_str().unwrap()).as_str()
    );
    // The same operation id again is the same session and no second terminal.
    for _ in 0..2 {
        let again = start("op-a", TARGET_A, IDLE_TOOL, &[]);
        assert_eq!(again["session_id"], a["session_id"]);
        assert_eq!(again["hosted"], a["hosted"]);
    }
    assert_eq!(drawer(&peer_router).await.len(), 1);
    let conflict = refused(
        &scheduler,
        &[
            "start",
            "--target",
            TARGET_A,
            "--operation-id",
            "op-a",
            "--tool",
            IDLE_TOOL,
            "--effort",
            "high",
        ],
    );
    assert!(conflict.contains("conflict"), "{conflict}");

    // Target B gets the other tool. Each target only runs what its policy lists.
    let b = start("op-b", TARGET_B, BUSY_TOOL, &[]);
    assert_eq!(b["state"], "running");
    let b_terminal = b["hosted"]["session_id"].clone();
    let listed = drawer(&peer_router).await;
    assert_eq!(listed.len(), 2);
    assert_eq!(tab(&listed, &b_terminal)["tool"], BUSY_TOOL);
    let wrong_tool = refused(
        &scheduler,
        &[
            "start",
            "--target",
            TARGET_A,
            "--operation-id",
            "op-wrong",
            "--tool",
            BUSY_TOOL,
        ],
    );
    // HS2-XHQ1EH: the policy's limits name the tools of a target.
    assert!(wrong_tool.contains("policy_denied"), "{wrong_tool}");
    let stranger = [
        "--actor-role",
        "ai",
        "--actor-id",
        "stranger",
        "-C",
        store_arg,
        "orchestration",
    ];
    let ungranted = refused(
        &stranger,
        &[
            "start",
            "--target",
            TARGET_A,
            "--operation-id",
            "op-s",
            "--tool",
            BUSY_TOOL,
        ],
    );
    assert!(ungranted.contains("policy_denied"), "{ungranted}");
    assert_eq!(drawer(&peer_router).await.len(), 2);
    assert_eq!(
        session(&scheduler, &["list", "--active"])
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // HS2-WEFJV8: the first start set the checkout up for its tool, which wrote the
    // instruction file as a project file. A person commits it.
    for checkout in &peer_checkouts {
        super::session_work::commit_all(Path::new(&checkout.root));
    }
    // The idle session is at a safe point; the busy one is mid-turn and stays.
    let a_id = a["session_id"].as_str().unwrap();
    let b_id = b["session_id"].as_str().unwrap();
    let point = session(&scheduler, &["safe-point", a_id])["safe_point"].clone();
    assert_eq!(point["blockers"], json!([]), "{point}");
    assert_eq!(point["safe"], true);
    // The terminal reports busy once it has read the tool's first output.
    let mut busy = Value::Null;
    for _ in 0..500 {
        busy = session(&scheduler, &["safe-point", b_id])["safe_point"].clone();
        if busy["blockers"] == json!(["mid_turn"]) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(busy["blockers"], json!(["mid_turn"]), "{busy}");
    let mid_turn = refused(&scheduler, &["stop", b_id]);
    assert!(mid_turn.contains("session_not_safe"), "{mid_turn}");
    let forced = refused(&scheduler, &["stop", b_id, "--force"]);
    assert!(forced.contains("session_force_not_permitted"), "{forced}");
    assert_eq!(tab(&drawer(&peer_router).await, &b_terminal)["alive"], true);

    // While the idle session's worker holds a claim on the peer, it is not wound down.
    let work = ops::create(
        &peer_store,
        hotsheet_model::Ulid::new(),
        "TST",
        "2026-10-15T00:00:00Z".into(),
        NewTicket {
            title: "Synthetic work".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let connection = peer_checkouts[0].sources[0].connection_id.clone();
    let claim = json!({"connection_id":connection,"native_id":work.id.to_string(),
        "worker":a["hosted"]["worker_id"],"lease_minutes":30});
    let claims = format!("/checkouts/{}/target-claims", peer_checkouts[0].id);
    let claimed = api(peer_router.clone(), "POST", &claims, claim.clone(), true).await;
    assert_eq!(claimed.0, StatusCode::OK, "{}", claimed.1);
    let held = session(&scheduler, &["safe-point", a_id])["safe_point"].clone();
    assert_eq!(held["safe"], false);
    assert_eq!(held["blockers"], json!(["held_claim"]));
    assert_eq!(held["facts"]["held_claims"], 1);
    let not_safe = refused(&scheduler, &["stop", a_id]);
    assert!(not_safe.contains("session_not_safe"), "{not_safe}");
    assert_eq!(tab(&drawer(&peer_router).await, &a_terminal)["alive"], true);
    assert_eq!(session(&scheduler, &["show", a_id])["state"], "running");
    // Uncommitted work in the checkout is a second reason.
    let draft = Path::new(&peer_checkouts[0].root).join("draft.txt");
    std::fs::write(&draft, "synthetic").unwrap();
    assert_eq!(
        session(&scheduler, &["safe-point", a_id])["safe_point"]["blockers"],
        json!(["held_claim", "uncommitted_changes"])
    );
    std::fs::remove_file(&draft).unwrap();
    let released = api(peer_router.clone(), "DELETE", &claims, claim, true).await;
    assert_eq!(released.0, StatusCode::OK, "{}", released.1);
    // At the safe point the wind-down goes through and the terminal is gone.
    let stopped = session(&scheduler, &["stop", a_id]);
    assert_eq!(stopped["state"], "stopped");
    assert_eq!(
        stopped["history"].as_array().unwrap().last().unwrap()["control"],
        "wind_down"
    );
    let listed = drawer(&peer_router).await;
    assert!(listed.iter().all(|terminal| terminal["id"] != a_terminal));
    let again = refused(&scheduler, &["stop", a_id]);
    assert!(again.contains("session_not_active"), "{again}");

    // The busy session's terminal is closed at the peer: a crash as far as anyone
    // else can tell. The orchestrator finds it lost and replaces it.
    let closed = api(
        peer_router.clone(),
        "DELETE",
        &format!("/terminals/{}", b_terminal.as_str().unwrap()),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(closed.0, StatusCode::NO_CONTENT);
    let synced = session(&scheduler, &["sync"]);
    assert_eq!(synced["sessions"][0]["state"], "lost");
    let lost = session(&scheduler, &["show", b_id]);
    assert_eq!(
        lost["history"].as_array().unwrap().last().unwrap()["reason"],
        "host_lost"
    );
    let replacement = start("op-b2", TARGET_B, BUSY_TOOL, &["--replaces", b_id]);
    assert_eq!(replacement["state"], "running");
    assert_eq!(replacement["replaces"], b_id);
    assert_ne!(replacement["hosted"]["session_id"], b_terminal);
    let r_id = replacement["session_id"].as_str().unwrap();
    let r_terminal = replacement["hosted"]["session_id"].clone();
    assert_eq!(tab(&drawer(&peer_router).await, &r_terminal)["alive"], true);

    // A person takes the replacement over. The orchestrator then leaves it alone,
    // and so does the peer when asked directly.
    let ai_takeover = refused(&scheduler, &["takeover", r_id]);
    assert!(ai_takeover.contains("forbidden"), "{ai_takeover}");
    let taken = session(&human, &["takeover", r_id]);
    assert_eq!(taken["owner"], "human");
    assert_eq!(taken["state"], "running");
    assert!(taken.get("unverified").is_none(), "{taken}");
    let human_owned = refused(&scheduler, &["stop", r_id]);
    assert!(human_owned.contains("session_human_owned"), "{human_owned}");
    let hosted = format!("/checkouts/{}/target-sessions", peer_checkouts[1].id);
    let direct = api(
        peer_router.clone(),
        "DELETE",
        &hosted,
        json!({"operation_id":"op-b2","actor":{"role":"ai","id":"scheduler-1"}}),
        true,
    )
    .await;
    assert_eq!(direct.0, StatusCode::OK, "{}", direct.1);
    assert_eq!(direct.1["stopped"], false);
    assert_eq!(direct.1["reason"], "session_human_owned");
    assert_eq!(tab(&drawer(&peer_router).await, &r_terminal)["alive"], true);
    // The peer's own listing shows whose it is and that it is mid-turn.
    let hosted_list = api(
        peer_router.clone(),
        "GET",
        &format!("{hosted}?safe_point=true"),
        Value::Null,
        true,
    )
    .await
    .1;
    let live: Vec<&Value> = hosted_list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|hosted| hosted["alive"] == true)
        .collect();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0]["session"]["owner"], "human");
    assert_eq!(
        live[0]["session"]["origin"]["owner_checkout_id"],
        owner.id.as_str()
    );
    assert_eq!(live[0]["label"], replacement["label"]);
    // A person's plain stop still respects the safe point; their explicit one ends it.
    let polite = refused(&human, &["stop", r_id]);
    assert!(polite.contains("session_not_safe"), "{polite}");
    let ended = session(&human, &["stop", r_id, "--force"]);
    assert_eq!(ended["state"], "stopped");
    assert_eq!(
        ended["history"].as_array().unwrap().last().unwrap()["control"],
        "stop"
    );
    assert!(drawer(&peer_router).await.is_empty());
    assert!(
        session(&scheduler, &["list", "--active"])
            .as_array()
            .unwrap()
            .is_empty()
    );

    // No path, origin or credential of the peer travelled with any session answer.
    let wire = session(&scheduler, &["list"]).to_string();
    for private in [
        SECRET,
        peer.origin.as_str(),
        peer_checkouts[0].root.as_str(),
        peer_home.to_str().unwrap(),
    ] {
        assert!(!wire.contains(private), "{private}");
    }
    // A read-only device may read the peer's sessions and can start nothing.
    set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadOnly).unwrap();
    let read_only = refused(
        &scheduler,
        &[
            "start",
            "--target",
            TARGET_A,
            "--operation-id",
            "op-ro",
            "--tool",
            IDLE_TOOL,
        ],
    );
    assert!(
        read_only.contains("target_provider_unauthorized"),
        "{read_only}"
    );
    assert!(drawer(&peer_router).await.is_empty());
    // Every start, stop and refusal is in the controller's journal, by code only.
    let audit = cli_named(&home, &human, &["audit", "--project", &owner.id]);
    let entries = audit["entries"].as_array().unwrap();
    let has = |action: &str, outcome: &str, reason: Value| {
        entries.iter().any(|entry| {
            entry["action"] == action && entry["outcome"] == outcome && entry["reason"] == reason
        })
    };
    assert!(has("sessions_manage", "succeeded", Value::Null));
    assert!(has("sessions_manage", "failed", json!("session_not_safe")));
    assert!(has("session_takeover", "succeeded", Value::Null));
    assert!(has("session_stop", "failed", json!("session_not_safe")));
    assert!(has("session_stop", "succeeded", Value::Null));
    peer.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_gdjxgz_stdio_mcp_and_headless_cli_manage_sessions_on_a_live_local_server() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home)
        .with_capacity_source(Box::new(Roomy))
        .with_capacity_cache(std::time::Duration::ZERO)
        // HS2-WEFJV8: the machine's plugin directory, which the headless CLI's own
        // setup of the target checkout reads like the server does.
        .with_plugin_dirs(vec![fake_plugins(&home)]);
    let project = root.path().join("project");
    git_project(&project);
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", checkout.id);
    let person = json!({"role":"human","id":"maintainer"});
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 3_600_000;
    let mut revision = Value::Null;
    for (target_id, tool) in [(TARGET_A, IDLE_TOOL), (TARGET_B, BUSY_TOOL)] {
        let target = format!("{base}/targets/{target_id}");
        let added = api(
            router.clone(),
            "POST",
            &format!("{base}/targets"),
            json!({"target_id":target_id,"label":"Self","expected_revision":revision}),
            true,
        )
        .await;
        assert_eq!(added.0, StatusCode::OK, "{}", added.1);
        revision = added.1["revision"].clone();
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
            &format!("{target}/binding"),
            json!({"target":{"owner_checkout_id":checkout.id,"target_id":target_id},
                "expected_manifest_revision":revision,"expected_binding_revision":0,
                "previous_destination":null,"destination":candidate["destination"]}),
            true,
        )
        .await;
        assert_eq!(bound.0, StatusCode::OK, "{}", bound.1);
        let granted = super::orchestration::remember_rules(
            router.clone(),
            &target.to_string(),
            json!({"actor_id":"scheduler-1","actions":["sessions_manage"],
                "session_scope":{"tools":[{"tool":tool}],"max_concurrency":4},
                "expires_at_ms":expires,"actor":person}),
        )
        .await;
        assert_eq!(granted.0, StatusCode::OK, "{}", granted.1);
    }
    let stored = api(
        router.clone(),
        "PUT",
        &format!("{base}/policy"),
        json!({"expected_revision":null,"policy":policy(),"actor":person}),
        true,
    )
    .await;
    assert_eq!(stored.0, StatusCode::OK, "{}", stored.1);
    // The project targets itself here, so its checked-in orchestration files are
    // committed like any other project file: uncommitted work would be a blocker.
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
            "Declare targets and policy",
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
    // The new routes need the bearer like everything else.
    let hosted = format!("/checkouts/{}/target-sessions", checkout.id);
    for (method, path) in [
        ("GET", hosted.clone()),
        ("POST", hosted.clone()),
        ("PATCH", hosted.clone()),
        ("DELETE", hosted.clone()),
        ("GET", format!("{base}/sessions")),
        ("POST", format!("{base}/sessions")),
        ("GET", format!("{base}/sessions/{TARGET_A}")),
        ("POST", format!("{base}/sessions/{TARGET_A}/stop")),
        ("POST", format!("{base}/session-sync")),
    ] {
        assert_eq!(
            api(router.clone(), method, &path, json!({}), false).await.0,
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
    // Plain HTTP errors are typed: an unknown session, an unidentified actor, a start
    // nobody granted.
    let missing = api(
        router.clone(),
        "GET",
        &format!("{base}/sessions/{TARGET_A}"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(missing.0, StatusCode::NOT_FOUND);
    assert_eq!(missing.1["code"], "session_not_found");
    let ungranted = api(
        router.clone(),
        "POST",
        &format!("{base}/sessions"),
        json!({"target_id":TARGET_A,"operation_id":"op-x","tool":BUSY_TOOL,
            "actor":{"role":"ai","id":"stranger"}}),
        true,
    )
    .await;
    assert_eq!(ungranted.0, StatusCode::FORBIDDEN);
    assert_eq!(ungranted.1["code"], "policy_denied");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    // The machine-local record a headless process finds this live server by.
    let instance = hotsheet_cli::external_launch::instance_path(&home, &source);
    std::fs::create_dir_all(instance.parent().unwrap()).unwrap();
    std::fs::write(
        &instance,
        json!({"pid":std::process::id(),"url":origin,"secret":SECRET,
            "store_path":source.canonicalize().unwrap()})
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
    // MCP: start, list, show, safe point, stop.
    let started = call(json!({"action":"session_start","target_id":TARGET_A,
        "operation_id":"mcp-op","tool":IDLE_TOOL}));
    assert_eq!(started["state"], "running");
    assert_eq!(
        started["label"],
        format!("{}: worker", checkout.alias).as_str()
    );
    let again = call(json!({"action":"session_start","target_id":TARGET_A,
        "operation_id":"mcp-op","tool":IDLE_TOOL}));
    assert_eq!(again["session_id"], started["session_id"]);
    let terminal = started["hosted"]["session_id"].clone();
    let listed = drawer(&router).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(tab(&listed, &terminal)["name"], started["label"]);
    let session_id = started["session_id"].as_str().unwrap();
    let records = call(json!({"action":"session_list","active":true}));
    assert_eq!(records.as_array().unwrap().len(), 1);
    assert_eq!(
        call(json!({"action":"session_show","session_id":session_id}))["operation_id"],
        "mcp-op"
    );
    // HS2-WEFJV8: the start wrote the tool's instruction file; a person commits it.
    super::session_work::commit_all(&project);
    let point = call(json!({"action":"session_safe_point","session_id":session_id}));
    assert_eq!(point["safe_point"]["safe"], true, "{point}");
    assert_eq!(
        call(json!({"action":"session_sync"}))["sessions"][0]["state"],
        "running"
    );
    for arguments in [
        // No identified actor, a tool the target's policy does not list, and acts this
        // tool does not offer at all.
        json!({"action":"session_start","checkout":checkout.id,"target_id":TARGET_A,
            "operation_id":"x","tool":IDLE_TOOL}),
        json!({"action":"session_start","checkout":checkout.id,"target_id":TARGET_A,
            "operation_id":"y","tool":BUSY_TOOL,"actor_role":"ai","actor_id":"scheduler-1"}),
        json!({"action":"session_takeover","checkout":checkout.id,"session_id":session_id,
            "actor_role":"human","actor_id":"maintainer"}),
    ] {
        mcp_rejected(&home, &backend, "hotsheet_targets", arguments);
    }
    let stopped_session = call(json!({"action":"session_stop","session_id":session_id}));
    assert_eq!(stopped_session["state"], "stopped");
    assert!(drawer(&router).await.is_empty());
    mcp_rejected(
        &home,
        &backend,
        "hotsheet_targets",
        json!({"action":"session_stop","checkout":checkout.id,"session_id":session_id,
            "actor_role":"ai","actor_id":"scheduler-1"}),
    );

    // A headless CLI owns no terminals: its hosted half uses this live server's
    // ordinary terminal routes, so the session still lands in this server's drawer.
    let store_arg = source.to_str().unwrap();
    let headless = [
        "--actor-role",
        "ai",
        "--actor-id",
        "scheduler-1",
        "-C",
        store_arg,
        "orchestration",
        "session",
    ];
    let cli_started = cli_named(
        &home,
        &headless,
        &[
            "start",
            "--project",
            &checkout.id,
            "--target",
            TARGET_A,
            "--operation-id",
            "cli-op",
            "--tool",
            IDLE_TOOL,
        ],
    );
    assert_eq!(cli_started["state"], "running");
    let cli_terminal = cli_started["hosted"]["session_id"].clone();
    let listed = drawer(&router).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(tab(&listed, &cli_terminal)["name"], cli_started["label"]);
    assert_eq!(tab(&listed, &cli_terminal)["tool"], IDLE_TOOL);
    // This machine's own view of what orchestrators started on the checkout.
    let hosted_view = cli_named(
        &home,
        &headless,
        &["list", "--checkout", &checkout.id, "--safe-point"],
    );
    let live: Vec<&Value> = hosted_view["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|hosted| hosted["alive"] == true)
        .collect();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0]["session"]["operation_id"], "cli-op");
    assert_eq!(live[0]["safe_point"]["safe"], true);
    assert_eq!(hosted_view["roles"]["integrator_live"], false);
    let cli_id = cli_started["session_id"].as_str().unwrap();
    let cli_stopped = cli_named(
        &home,
        &headless,
        &["stop", cli_id, "--project", &checkout.id],
    );
    assert_eq!(cli_stopped["state"], "stopped");
    assert!(drawer(&router).await.is_empty());

    // With the server gone no terminal host answers: a start fails closed and the
    // failed session is recorded, never a running one.
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    std::fs::remove_file(&instance).unwrap();
    let gone = cli_failure(
        &home,
        &headless,
        &[
            "start",
            "--project",
            &checkout.id,
            "--target",
            TARGET_A,
            "--operation-id",
            "gone-op",
            "--tool",
            IDLE_TOOL,
        ],
    );
    assert!(gone.contains("session_admission_refused"), "{gone}");
    let records = cli_named(&home, &headless, &["list", "--project", &checkout.id]);
    let failed = records
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["operation_id"] == "gone-op")
        .unwrap();
    assert_eq!(failed["state"], "failed");
    assert_eq!(
        failed["history"].as_array().unwrap().last().unwrap()["reason"],
        "capacity_unknown"
    );
}

/// HS2-WHR4GS: a stop that finds no record fences the operation on a live service, so
/// the start request that arrives after it launches nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_whr4gs_a_late_start_after_its_stop_is_refused_on_a_live_service() {
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
    let hosted = format!("/checkouts/{}/target-sessions", checkout.id);
    let actor = json!({"role":"ai","id":"scheduler-1"});
    let (status, stopped) = api(
        router.clone(),
        "DELETE",
        &hosted,
        json!({"operation_id":"op-late","actor":actor}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{stopped}");
    assert_eq!(stopped["stopped"], false);
    assert_eq!(stopped["reason"], "session_not_found");
    assert_eq!(stopped["fenced"], true);
    let start = json!({
        "session": {
            "operation_id": "op-late",
            "tool": "codex",
            "origin": {"owner_checkout_id": "owner-0123456789ab", "project": "Synthetic"}
        },
        "actor": actor,
    });
    for _ in 0..2 {
        let (status, late) = api(router.clone(), "POST", &hosted, start.clone(), true).await;
        assert_eq!(status, StatusCode::OK, "{late}");
        assert_eq!(late["outcome"], "refused");
        assert_eq!(late["reason"], "session_withdrawn");
        assert!(late.get("session").is_none());
    }
    let (_, listed) = api(router.clone(), "GET", &hosted, Value::Null, true).await;
    assert_eq!(listed["sessions"], json!([]));
    assert!(home.join("orchestration-withdrawn-sessions.json").is_file());
}
