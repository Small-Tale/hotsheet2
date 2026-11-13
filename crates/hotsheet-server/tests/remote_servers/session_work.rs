//! HS2-WEFJV8, HS2-2DPGDY, HS2-YC2P7K: an orchestrator-started session on a real
//! server finds its tool configuration in the checkout it starts in, takes up its
//! role, and gets the exact ticket of the step it was started for. Sessions are real
//! PTYs running the workspace fake agent; no AI tool is launched and nothing leaves
//! loopback.
use super::orchestration::api;
use super::sessions::{drawer, git_project};
use super::*;
use hotsheet_ticketing::StoreMetadata;
use hotsheet_ticketing::checkouts::{Checkout, CheckoutRegistry};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(super) const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FJ0";
/// A fake tool that reports what it saw at launch, then waits.
pub(super) const REPORT_TOOL: &str = "fake-report";

const MIB: u64 = 1024 * 1024;

/// A roomy machine with the reporting fake tool installed.
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
        Some(vec![(REPORT_TOOL.into(), true)])
    }
}

fn workspace_binary(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current test executable");
    exe.parent()
        .and_then(Path::parent)
        .expect("target profile directory")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// One plugin that launches the fake agent, which writes what it saw at launch to
/// `<reports>/<terminal id>.json` and then holds. `extra` adds plugin manifest
/// sections, such as a `[session_push]` declaration.
pub(super) fn report_plugin(root: &Path, reports: &Path, extra: &str) -> PathBuf {
    let fake_agent = workspace_binary("hs-fake-agent");
    assert!(fake_agent.is_file(), "build the workspace hs-fake-agent");
    std::fs::create_dir_all(reports).unwrap();
    let plugins = root.join("report-plugins");
    let plugin = plugins.join(REPORT_TOOL);
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("instructions.md"), "Synthetic agent\n").unwrap();
    std::fs::write(
        plugin.join("manifest.toml"),
        format!(
            r#"
id = "{REPORT_TOOL}"
display_name = "{REPORT_TOOL}"
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
args = ["--probe", "AGENTS.md", "--probe", ".mcp.json", "--report-dir", {reports:?}, "--print", "ready", "--hold-ms", "600000"]
{extra}
"#
        ),
    )
    .unwrap();
    plugins
}

/// Waits, woken by the directory changing, for the report the session with this
/// terminal id writes at its launch.
pub(super) fn launch_report(reports: &Path, terminal_id: &str) -> Value {
    let waiter = hotsheet_jobs::ChangeWaiter::new(reports).unwrap();
    let path = reports.join(format!("{terminal_id}.json"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(report) = serde_json::from_str::<Value>(&text)
        {
            return report;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "no launch report at {}",
            path.display()
        );
        tokio::task::block_in_place(|| waiter.wait(remaining));
    }
}

/// Commit everything in the project, as a person does with the project files the
/// first tool setup wrote (HS2-WEFJV8): uncommitted work holds a session's safe point.
pub(super) fn commit_all(project: &Path) {
    for arguments in [
        &["add", "-A"][..],
        &[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.test",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Synthetic project files",
        ],
    ] {
        let status = hotsheet_ticketing::git::launch("git")
            .arg("-C")
            .arg(project)
            .args(arguments)
            .status()
            .unwrap();
        assert!(status.success(), "{arguments:?}");
    }
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// A server whose own store is not the checkout's: the project has its own linked
/// store, and it orchestrates itself as one target.
pub(super) struct World {
    pub(super) _root: tempfile::TempDir,
    pub(super) server_store: PathBuf,
    pub(super) project: PathBuf,
    pub(super) project_store: PathBuf,
    pub(super) reports: PathBuf,
    pub(super) checkout: Checkout,
    pub(super) router: axum::Router,
    pub(super) base: String,
}

impl World {
    pub(super) async fn new(actions: &[&str], plugin_extra: &str) -> Self {
        assert!(hotsheet_aitools::test_permission_vault_available());
        hotsheet_aitools::enable_test_permission_vault();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let server_store = root.path().join("server-store");
        let reports = root.path().join("reports");
        let host = state(&server_store, &home)
            .with_capacity_source(Box::new(Roomy))
            .with_capacity_cache(std::time::Duration::ZERO)
            .with_plugin_dirs(vec![report_plugin(root.path(), &reports, plugin_extra)]);
        let project = root.path().join("project");
        git_project(&project);
        let project_store = root.path().join("project-store");
        FsStore::init_git(&project_store, &StoreMetadata::new("TST")).unwrap();
        let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
            .register(&project, None, None, vec![project_store.clone()])
            .unwrap();
        let router = app(host);
        let base = format!("/checkouts/{}/orchestration", checkout.id);
        let world = Self {
            _root: root,
            server_store,
            project,
            project_store,
            reports,
            checkout,
            router,
            base,
        };
        world.bind_and_grant(actions).await;
        world
    }

    pub(super) async fn call(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        api(
            self.router.clone(),
            method,
            &format!("{}{path}", self.base),
            body,
            true,
        )
        .await
    }

    async fn bind_and_grant(&self, actions: &[&str]) {
        let person = json!({"role":"human","id":"maintainer"});
        let added = self
            .call(
                "POST",
                "/targets",
                json!({"target_id":TARGET,"label":"Self","expected_revision":null}),
            )
            .await;
        assert_eq!(added.0, StatusCode::OK, "{}", added.1);
        let candidate = self
            .call(
                "POST",
                "/inspect",
                json!({"destination":{"kind":"local","checkout_id":self.checkout.id}}),
            )
            .await
            .1;
        let bound = self
            .call(
                "PUT",
                &format!("/targets/{TARGET}/binding"),
                json!({"target":{"owner_checkout_id":self.checkout.id,"target_id":TARGET},
                    "expected_manifest_revision":added.1["revision"],
                    "expected_binding_revision":0,"previous_destination":null,
                    "destination":candidate["destination"]}),
            )
            .await;
        assert_eq!(bound.0, StatusCode::OK, "{}", bound.1);
        let granted = self
            .call(
                "POST",
                &format!("/targets/{TARGET}/grants"),
                json!({"actor_id":"scheduler-1","actions":actions,
                    "session_scope":{"tools":[{"tool":REPORT_TOOL}],"max_concurrency":2},
                    "expires_at_ms":now_ms() + 3_600_000,"actor":person}),
            )
            .await;
        assert_eq!(granted.0, StatusCode::OK, "{}", granted.1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_wefjv8_a_session_in_another_checkout_starts_with_that_checkouts_configuration() {
    let world = World::new(&["sessions_manage"], "").await;
    let policy = json!({"machine":{"max_sessions":4},
        "tools":[{"tool":REPORT_TOOL,"max_sessions":4}],
        "targets":[{"target_id":TARGET,"actions":["sessions_manage"],
            "tools":[{"tool":REPORT_TOOL}],"max_sessions":2}]});
    let stored = world
        .call(
            "PUT",
            "/policy",
            json!({"expected_revision":null,"policy":policy,
                "actor":{"role":"human","id":"maintainer"}}),
        )
        .await;
    assert_eq!(stored.0, StatusCode::OK, "{}", stored.1);
    // Nothing is configured in the checkout before the start.
    assert!(!world.project.join("AGENTS.md").exists());
    assert!(!world.project.join(".mcp.json").exists());

    let started = world
        .call(
            "POST",
            "/sessions",
            json!({"target_id":TARGET,"operation_id":"op-setup","tool":REPORT_TOOL,
                "actor":{"role":"ai","id":"scheduler-1"}}),
        )
        .await;
    assert_eq!(started.0, StatusCode::OK, "{}", started.1);
    assert_eq!(started.1["state"], "running", "{}", started.1);
    assert!(
        started.1["tool_setup"]["wrote"].as_u64().unwrap() >= 2,
        "{}",
        started.1
    );
    let terminal = started.1["hosted"]["session_id"].as_str().unwrap();
    let worker = started.1["hosted"]["worker_id"].as_str().unwrap();

    // What the tool saw when it started: the checkout as its directory, with the
    // project's instructions and MCP configuration already there.
    let report = launch_report(&world.reports, terminal);
    assert_eq!(
        Path::new(report["cwd"].as_str().unwrap()),
        world.project.canonicalize().unwrap(),
        "{report}"
    );
    assert_eq!(report["files"]["AGENTS.md"], true, "{report}");
    assert_eq!(report["files"][".mcp.json"], true, "{report}");
    let env = &report["env"];
    assert_eq!(env["HOTSHEET_WORKER_ID"], worker, "{report}");
    assert_eq!(env["HOTSHEET_TERMINAL_ID"], terminal, "{report}");
    assert_eq!(env["HOTSHEET_ACTOR_ROLE"], "ai", "{report}");
    assert_eq!(env["HOTSHEET_SECRET"], "<set>", "{report}");
    assert_eq!(
        Path::new(env["HOTSHEET_PROJECT"].as_str().unwrap()),
        world.project.canonicalize().unwrap(),
        "{report}"
    );

    // The configuration is the checkout's: its MCP entry points at the project's own
    // linked store, never at the serving store.
    let mcp = std::fs::read_to_string(world.project.join(".mcp.json")).unwrap();
    let project_store = world.project_store.canonicalize().unwrap();
    assert!(mcp.contains(project_store.to_str().unwrap()), "{mcp}");
    assert!(!mcp.contains(world.server_store.to_str().unwrap()), "{mcp}");
    let instructions = std::fs::read_to_string(world.project.join("AGENTS.md")).unwrap();
    assert!(instructions.contains("Synthetic agent"), "{instructions}");
    // The MCP file is machine-local and excluded from Git; the instructions are
    // project files that someone commits.
    let excluded =
        std::fs::read_to_string(world.project.join(".git/info/exclude")).unwrap_or_default();
    assert!(excluded.contains(".mcp.json"), "{excluded}");

    // A repeated start is the same session: no second setup, no second launch.
    let again = world
        .call(
            "POST",
            "/sessions",
            json!({"target_id":TARGET,"operation_id":"op-setup","tool":REPORT_TOOL,
                "actor":{"role":"ai","id":"scheduler-1"}}),
        )
        .await;
    assert_eq!(again.1["session_id"], started.1["session_id"]);
    assert_eq!(drawer(&world.router).await.len(), 1);

    // A checkout that holds an HS1 store is refused before anything launches there.
    std::fs::create_dir_all(world.project.join(".hotsheet/db")).unwrap();
    std::fs::write(world.project.join(".hotsheet/db/PG_VERSION"), "16").unwrap();
    let refused = world
        .call(
            "POST",
            "/sessions",
            json!({"target_id":TARGET,"operation_id":"op-hs1","tool":REPORT_TOOL,
                "actor":{"role":"ai","id":"scheduler-1"}}),
        )
        .await;
    assert_ne!(refused.0, StatusCode::OK, "{}", refused.1);
    assert_eq!(refused.1["code"], "session_start_failed", "{}", refused.1);
    let failed = world.call("GET", "/sessions", Value::Null).await.1;
    let failed = failed
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["operation_id"] == "op-hs1")
        .unwrap()
        .clone();
    assert_eq!(failed["state"], "failed");
    assert_eq!(
        failed["history"].as_array().unwrap().last().unwrap()["reason"],
        "tool_setup_failed"
    );
    assert_eq!(drawer(&world.router).await.len(), 1);
}
