//! HS2-WEFJV8, HS2-2DPGDY, HS2-YC2P7K: an orchestrator-started session on a real
//! server finds its tool configuration in the checkout it starts in, takes up its
//! role, and gets the exact ticket of the step it was started for. Sessions are real
//! PTYs running the workspace fake agent; no AI tool is launched and nothing leaves
//! loopback.
use super::orchestration::api;
use super::sessions::{commit_all, drawer, git_project};
use super::*;
use hotsheet_server::AppState;
use hotsheet_ticketing::checkouts::{Checkout, CheckoutRegistry};
use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};
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
/// `<reports>/<terminal id>.json` and then holds. `agent` adds script flags before the
/// hold, as TOML array items; `extra` continues the `[launch]` table and may add
/// sections, such as a `[session_push]` declaration.
pub(super) fn report_plugin(root: &Path, reports: &Path, agent: &str, extra: &str) -> PathBuf {
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
args = ["--probe", "AGENTS.md", "--probe", ".mcp.json", "--report-dir", {reports:?}, {agent} "--print", "ready", "--hold-ms", "600000"]
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
    let waiter = hotsheet_jobs::ChangeWaiter::starting(&[reports]).unwrap();
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
    pub(super) home: PathBuf,
    pub(super) server_store: PathBuf,
    pub(super) project: PathBuf,
    pub(super) project_store: PathBuf,
    pub(super) reports: PathBuf,
    pub(super) checkout: Checkout,
    pub(super) router: axum::Router,
    pub(super) base: String,
    /// HS2-3H51NM: the serving state and its plugins, to restart it over the same home.
    pub(super) host: AppState,
    plugins: PathBuf,
    /// HS2-3H51NM: the detached terminal broker's socket, when terminals outlive a
    /// restart of the server, and the in-process broker serving it.
    broker: Option<(PathBuf, tokio::task::JoinHandle<()>)>,
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some((_, broker)) = &self.broker {
            broker.abort();
        }
    }
}

impl World {
    pub(super) async fn new(actions: &[&str], agent: &str, plugin_extra: &str) -> Self {
        Self::with_store(actions, agent, plugin_extra, false).await
    }

    /// `served`: the project's linked store is the one this server serves, so ticket
    /// writes through its ticket routes reach its change feed, as they do for a
    /// project open in it.
    pub(super) async fn with_store(
        actions: &[&str],
        agent: &str,
        plugin_extra: &str,
        served: bool,
    ) -> Self {
        Self::build(actions, agent, plugin_extra, served, false).await
    }

    /// HS2-3H51NM: terminals live in a detached broker (served in process over a Unix
    /// socket), so they outlive a [`Self::restart`] as the planner's would.
    pub(super) async fn with_broker(actions: &[&str], agent: &str, plugin_extra: &str) -> Self {
        Self::build(actions, agent, plugin_extra, false, true).await
    }

    async fn build(
        actions: &[&str],
        agent: &str,
        plugin_extra: &str,
        served: bool,
        broker: bool,
    ) -> Self {
        assert!(hotsheet_aitools::test_permission_vault_available());
        hotsheet_aitools::enable_test_permission_vault();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let server_store = root.path().join("server-store");
        let reports = root.path().join("reports");
        let plugins = report_plugin(root.path(), &reports, agent, plugin_extra);
        let broker = broker.then(|| {
            let socket = root.path().join("broker.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            let serving = tokio::spawn(hotsheet_terminals::serve_broker(
                listener,
                "planning-restart".into(),
                Arc::new(hotsheet_terminals::TerminalManager::new()),
            ));
            (socket, serving)
        });
        let host = Self::serving(
            &server_store,
            &home,
            &plugins,
            broker.as_ref().map(|(socket, _)| socket.as_path()),
        );
        let project = root.path().join("project");
        git_project(&project);
        let project_store = if served {
            server_store.clone()
        } else {
            let store = root.path().join("project-store");
            FsStore::init_git(&store, &StoreMetadata::new("TST")).unwrap();
            store
        };
        let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
            .register(&project, None, None, vec![project_store.clone()])
            .unwrap();
        let router = app(host.clone());
        let base = format!("/checkouts/{}/orchestration", checkout.id);
        let world = Self {
            _root: root,
            home,
            server_store,
            project,
            project_store,
            reports,
            checkout,
            router,
            base,
            host,
            plugins,
            broker,
        };
        world.bind_and_allow(actions).await;
        world
    }

    fn serving(store: &Path, home: &Path, plugins: &Path, broker: Option<&Path>) -> AppState {
        let host = state(store, home)
            .with_capacity_source(Box::new(Roomy))
            .with_capacity_cache(std::time::Duration::ZERO)
            .with_plugin_dirs(vec![plugins.to_path_buf()]);
        match broker {
            Some(socket) => host.with_terminal_broker_at(
                hotsheet_server::terminal_broker::TerminalBroker::at(socket, "planning-restart"),
            ),
            None => host,
        }
    }

    /// HS2-3H51NM: the server stops and a new one starts over the same home and store.
    /// Without a broker the terminals this server hosts die with it; with one they live
    /// on and the new server reaches them.
    pub(super) async fn restart(&mut self) {
        if self.broker.is_none() {
            for tab in drawer(&self.router).await {
                let id = tab["id"].as_str().unwrap();
                let (status, _) = api(
                    self.router.clone(),
                    "DELETE",
                    &format!("/terminals/{id}"),
                    Value::Null,
                    true,
                )
                .await;
                assert_eq!(status, StatusCode::NO_CONTENT);
            }
        }
        self.host.begin_stopping();
        let next = Self::serving(
            &self.server_store,
            &self.home,
            &self.plugins,
            self.broker.as_ref().map(|(socket, _)| socket.as_path()),
        );
        next.spawn_run_recovery();
        self.router = app(next.clone());
        self.host = next;
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

    /// Store a policy that lets the fake tool run on the target. `extra` adds
    /// top-level members such as `placement` or `wind_down`.
    pub(super) async fn allow(&self, actions: &[&str], extra: Value) -> Value {
        let mut policy = json!({"machine":{"max_sessions":4},
            "tools":[{"tool":REPORT_TOOL,"max_sessions":4}],
            "targets":[{"target_id":TARGET,"actions":actions,
                "tools":[{"tool":REPORT_TOOL}],"max_sessions":2}]});
        policy
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let current = self.call("GET", "/policy", Value::Null).await.1;
        let stored = self
            .call(
                "PUT",
                "/policy",
                json!({"expected_revision":current["revision"],"policy":policy,
                    "actor":{"role":"human","id":"maintainer"}}),
            )
            .await;
        assert_eq!(stored.0, StatusCode::OK, "{}", stored.1);
        stored.1
    }

    /// Start a session of the fake tool as the actor a person remembered rules for.
    pub(super) async fn start(&self, operation: &str) -> (StatusCode, Value) {
        self.call(
            "POST",
            "/sessions",
            json!({"target_id":TARGET,"operation_id":operation,"tool":REPORT_TOOL,
                "actor":{"role":"ai","id":"scheduler-1"}}),
        )
        .await
    }

    /// Bring this machine's record of a session in line with its host.
    pub(super) async fn sync(&self, session: &Value) -> Value {
        let (status, view) = self
            .call(
                "POST",
                &format!("/sessions/{}/sync", session["session_id"].as_str().unwrap()),
                json!({"actor":{"role":"ai","id":"scheduler-1"}}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{view}");
        view
    }

    /// A synthetic ticket in the project's own store.
    pub(super) fn ticket(&self, title: &str) -> hotsheet_model::Ticket {
        ops::create(
            &FsStore::open(&self.project_store).unwrap(),
            hotsheet_model::Ulid::new(),
            "TST",
            "2026-10-15T00:00:00Z".into(),
            NewTicket {
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    async fn bind_and_allow(&self, actions: &[&str]) {
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
        // HS2-XHQ1EH: a person remembers a rule for each act the loop needs.
        for action in actions {
            let body = match *action {
                "tickets_read" | "gates_observe" => continue,
                "sessions_manage" => json!({"action":"session_start","tool":REPORT_TOOL}),
                other => json!({"action":other}),
            };
            let mut body = body;
            body["target_id"] = json!(TARGET);
            body["actor"] = person.clone();
            let remembered = self.call("POST", "/permission-rules", body).await;
            assert_eq!(remembered.0, StatusCode::OK, "{}", remembered.1);
        }
    }
}

/// Waits, woken by the directory changing, for the prompts the session with this
/// terminal id has received to include one containing `wanted`.
pub(super) fn received_prompt(reports: &Path, terminal_id: &str, wanted: &str) -> String {
    let waiter = hotsheet_jobs::ChangeWaiter::starting(&[reports]).unwrap();
    let path = reports.join(format!("{terminal_id}.prompts"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let found = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|line| line["text"].as_str().map(str::to_owned))
            .find(|text| text.contains(wanted));
        if let Some(text) = found {
            return text;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "no prompt with {wanted} at {}; received: {:?}",
            path.display(),
            std::fs::read_to_string(&path).unwrap_or_default()
        );
        tokio::task::block_in_place(|| waiter.wait(remaining));
    }
}

/// HS2-2DPGDY: an OpenCode-style session API that comes up after the launch.
pub(super) const PUSH_AGENT: &str = r#""--busy", "--serve-opencode", "1500","#;
pub(super) const PUSH_PLUGIN: &str = r#"
[session_push]
mechanism = "opencode-http"
port_args = ["--port", "{port}"]
password_env = "OPENCODE_SERVER_PASSWORD"
"#;

/// Register the session's worker id in the target's coordination pool, as the
/// `hotsheet-worker` skill does when it starts (`coordination ... next --role worker`).
pub(super) fn join_pool(world: &World, worker_id: &str) {
    let coordinator = hotsheet_cli::coordination::Coordinator::open_with_registry_home(
        &world.project,
        &world.project_store,
        world.home.clone(),
    )
    .unwrap();
    let now = i64::try_from(now_ms() / 1000).unwrap();
    coordinator
        .register(
            hotsheet_cli::coordination::Role::Worker,
            worker_id,
            "synthetic-generation",
            now,
            3600,
            hotsheet_cli::coordination::ReturnRoute::HumanRelay,
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_2dpgdy_a_role_prompt_waits_for_the_tool_and_joining_is_confirmed_by_the_pool() {
    let world = World::new(&["sessions_manage"], PUSH_AGENT, PUSH_PLUGIN).await;
    world.allow(&["sessions_manage"], json!({})).await;
    let (status, started) = world.start("op-role").await;
    assert_eq!(status, StatusCode::OK, "{started}");
    // The tool's session API is not up yet: the prompt waits for it.
    assert_eq!(started["role_prompt"], "pending", "{started}");
    let terminal = started["hosted"]["session_id"].as_str().unwrap().to_owned();
    let worker = started["hosted"]["worker_id"].as_str().unwrap().to_owned();
    // Its environment names the role whatever happens to the prompt.
    let report = launch_report(&world.reports, &terminal);
    assert_eq!(report["env"]["HOTSHEET_SESSION_ROLE"], "worker", "{report}");
    assert_eq!(
        report["env"]["HOTSHEET_ROLE_SKILL"], "hotsheet-worker",
        "{report}"
    );

    // The tool comes up and ends a turn. That event, not a timer, delivers the prompt.
    let prompt = received_prompt(&world.reports, &terminal, "`hotsheet-worker`");
    assert!(prompt.contains(&worker), "{prompt}");
    let synced = world.sync(&started).await;
    assert_eq!(synced["role_prompt"], "delivered", "{synced}");
    assert_eq!(synced.get("joined_at_ms"), None, "{synced}");
    // Delivered once: later turn ends push nothing more.
    let prompts =
        std::fs::read_to_string(world.reports.join(format!("{terminal}.prompts"))).unwrap();
    assert_eq!(prompts.lines().count(), 1, "{prompts}");

    // The session registers in the target's pool under its worker id, as its skill
    // does. The next read confirms it joined.
    join_pool(&world, &worker);
    let joined = world.sync(&started).await;
    assert!(joined["joined_at_ms"].as_u64().is_some(), "{joined}");
    let again = world.sync(&started).await;
    assert_eq!(again["joined_at_ms"], joined["joined_at_ms"]);
    assert_eq!(drawer(&world.router).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_2dpgdy_a_tool_without_session_push_gets_its_role_at_launch() {
    let world = World::new(
        &["sessions_manage"],
        "",
        r#"prompt_args = ["--prompt", "{prompt}"]"#,
    )
    .await;
    world.allow(&["sessions_manage"], json!({})).await;
    let (status, started) = world.start("op-launch").await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["role_prompt"], "at_launch", "{started}");
    let terminal = started["hosted"]["session_id"].as_str().unwrap();
    let prompt = received_prompt(&world.reports, terminal, "`hotsheet-worker`");
    assert!(
        prompt.contains(started["hosted"]["worker_id"].as_str().unwrap()),
        "{prompt}"
    );
}

/// HS2-KK3CCE: Antigravity's own `[launch] prompt_args` (`agy -i <prompt>`, verified on agy
/// 1.5.10) drives a stand-in that reads `-i` the way agy does: the role prompt is on its
/// command line and the start reports `at_launch`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_kk3cce_antigravity_takes_its_role_prompt_with_dash_i_at_launch() {
    let antigravity = hotsheet_plugins::find_in("antigravity", &[]).expect("antigravity plugin");
    let declared = &antigravity.manifest.launch.as_ref().unwrap().prompt_args;
    assert_eq!(declared, &["-i", "{prompt}"]);
    assert!(antigravity.manifest.session_push.is_none());
    let world = World::new(
        &["sessions_manage"],
        "",
        &format!("prompt_args = {declared:?}"),
    )
    .await;
    world.allow(&["sessions_manage"], json!({})).await;
    let (status, started) = world.start("op-agy").await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["role_prompt"], "at_launch", "{started}");
    let terminal = started["hosted"]["session_id"].as_str().unwrap();
    let prompt = received_prompt(&world.reports, terminal, "`hotsheet-worker`");
    assert!(
        prompt.contains(started["hosted"]["worker_id"].as_str().unwrap()),
        "{prompt}"
    );
    let report = launch_report(&world.reports, terminal);
    let argv: Vec<&str> = report["args"]
        .as_array()
        .expect("the stand-in reports its argv")
        .iter()
        .map(|arg| arg.as_str().unwrap())
        .collect();
    let at = argv
        .iter()
        .position(|arg| *arg == "-i")
        .expect("-i on the command line");
    assert_eq!(argv[at + 1], prompt, "{argv:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_wefjv8_a_session_in_another_checkout_starts_with_that_checkouts_configuration() {
    let world = World::new(&["sessions_manage"], "", "").await;
    world.allow(&["sessions_manage"], json!({})).await;
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
        hotsheet_sync::paths::canonicalize(&world.project).unwrap(),
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
        hotsheet_sync::paths::canonicalize(&world.project).unwrap(),
        "{report}"
    );

    // The configuration is the checkout's: its MCP entry points at the project's own
    // linked store, never at the serving store.
    let mcp = std::fs::read_to_string(world.project.join(".mcp.json")).unwrap();
    let project_store = hotsheet_sync::paths::canonicalize(&world.project_store).unwrap();
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

/// Every decision record of the owner's scheduler, in order.
async fn decisions(world: &World) -> Vec<Value> {
    let mut entries = Vec::new();
    let mut after = 0;
    loop {
        let page = world
            .call(
                "POST",
                "/scheduler/decisions",
                json!({"after": after, "limit": 200}),
            )
            .await
            .1;
        entries.extend(page["entries"].as_array().unwrap().iter().cloned());
        after = page["cursor"].as_u64().unwrap();
        if page["more"] != true {
            return entries;
        }
    }
}

/// Waits, woken by the decision log changing, for the first decision wanted.
async fn decided(
    world: &World,
    what: &str,
    wanted: impl Fn(&Value) -> bool,
    within: std::time::Duration,
) -> (Value, Vec<Value>) {
    let directory = world.home.join("orchestration-scheduler-decisions");
    std::fs::create_dir_all(&directory).unwrap();
    let waiter = hotsheet_jobs::ChangeWaiter::starting(&[directory.as_path()]).unwrap();
    let deadline = std::time::Instant::now() + within;
    loop {
        let log = decisions(world).await;
        if let Some(found) = log.iter().find(|entry| wanted(entry)) {
            return (found.clone(), log);
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "no {what}: {log:?}");
        tokio::task::block_in_place(|| waiter.wait(remaining));
    }
}

/// Waits on the run's own event stream (the server's long poll, never a timer) until
/// its status satisfies `wanted`.
async fn run_until(
    world: &World,
    run_id: &str,
    what: &str,
    wanted: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut cursor = 0;
    loop {
        let status = world
            .call("GET", &format!("/runs/{run_id}/status"), Value::Null)
            .await
            .1;
        if wanted(&status) {
            return status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "run never {what}: {status}"
        );
        let page = world
            .call(
                "GET",
                &format!("/run-events?after={cursor}&run={run_id}&wait_ms=5000"),
                Value::Null,
            )
            .await
            .1;
        cursor = page["cursor"].as_u64().unwrap_or(cursor);
    }
}

/// HS2-YC2P7K with HS2-WEFJV8 and HS2-2DPGDY: the whole chain an orchestrator-started
/// session walks, on one real server with the push-capable fake agent. An automatic
/// policy and a confirmed ready step; the loop dispatches the step's run and starts a
/// session linked to it in the target checkout, set up for its tool; the role prompt
/// reaches the tool when it is ready and asks for the exact ticket; the session joins
/// the target's pool, claims that ticket under its own worker id and finishes it; the
/// run follows; the session is wound down at a safe point. Where the test plays the
/// agent's part (registering, claiming, finishing), it does exactly what the worker
/// skill does through the same stores.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_yc2p7k_an_orchestrator_started_session_works_its_step_end_to_end() {
    let actions = [
        "tickets_read",
        "gates_observe",
        "sessions_manage",
        "work_start",
    ];
    let world = World::with_store(&actions, PUSH_AGENT, PUSH_PLUGIN, true).await;
    let orchestration = world.ticket("Coordinate the synthetic chain");
    let work = world.ticket("Synthetic chained work");
    let actor = json!({"role":"ai","id":"scheduler-1"});
    let defined = world
        .call(
            "POST",
            "/readiness/define",
            json!({"actor":actor,"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET,"connection_id":world.checkout.sources[0].connection_id,
                "native_id":work.slug}),
        )
        .await;
    assert_eq!(defined.0, StatusCode::OK, "{}", defined.1);
    assert_eq!(defined.1["level"], "confirmed", "{}", defined.1);
    let step = defined.1["step"]["step_id"].as_str().unwrap().to_owned();

    // A person enables automatic placement; the project's orchestration files are
    // committed like any other project file. HS2-R2N33M: the person chooses no role
    // sessions, so the fake agent is the one session and works as the worker.
    world
        .allow(
            &actions,
            json!({"wind_down":{"idle_minutes":1,"keep_warm":0},
                "placement":{"automatic":true,"actor":"scheduler-1","cooldown_minutes":0,
                    "role_sessions":[{"target_id":TARGET,"roles":[]}]}}),
        )
        .await;
    commit_all(&world.project);

    // 1. The loop dispatches the step's run and starts a session linked to it.
    let minute = std::time::Duration::from_secs(60);
    let started = |entry: &Value| entry["kind"] == "start" && entry["outcome"] == "started";
    let (start, _) = decided(&world, "started session", started, minute).await;
    assert_eq!(start["mode"], "applied");
    assert_eq!(start["step_id"], step.as_str());
    let run_id = start["run_id"]
        .as_str()
        .expect("the start names its run")
        .to_owned();
    let runs = world.call("GET", "/runs", Value::Null).await.1;
    let runs = runs.as_array().unwrap();
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0]["run_id"], run_id.as_str());
    assert_eq!(
        runs[0]["operation_id"],
        format!("sched-run-{step}").as_str()
    );
    assert_eq!(runs[0]["mode"], "dispatcher");
    let session = world
        .call(
            "GET",
            &format!("/sessions/{}", start["session_id"].as_str().unwrap()),
            Value::Null,
        )
        .await
        .1;
    assert_eq!(session["state"], "running", "{session}");
    assert_eq!(session["run_id"], run_id.as_str(), "{session}");
    // The step is in progress; the target ticket is queued for the target's dispatcher.
    let readiness = world
        .call("POST", "/readiness/show", json!({"step_id": step}))
        .await
        .1;
    assert_eq!(readiness["level"], "in_progress", "{readiness}");
    let store = FsStore::open(&world.project_store).unwrap();
    assert!(ops::resolve(&store, &work.slug).unwrap().unwrap().up_next);

    // 2. The session started in the target checkout with its configuration.
    let terminal = session["hosted"]["session_id"].as_str().unwrap().to_owned();
    let worker = session["hosted"]["worker_id"].as_str().unwrap().to_owned();
    let report = launch_report(&world.reports, &terminal);
    assert_eq!(
        Path::new(report["cwd"].as_str().unwrap()),
        hotsheet_sync::paths::canonicalize(&world.project).unwrap()
    );
    assert_eq!(report["files"]["AGENTS.md"], true, "{report}");
    assert_eq!(report["files"][".mcp.json"], true, "{report}");
    assert_eq!(
        report["env"]["HOTSHEET_WORKER_ID"],
        worker.as_str(),
        "{report}"
    );
    assert_eq!(report["env"]["HOTSHEET_SESSION_ROLE"], "worker", "{report}");

    // 3. The tool comes up; the role prompt reaches it and asks for the exact ticket.
    let prompt = received_prompt(&world.reports, &terminal, "`hotsheet-worker`");
    assert!(prompt.contains(&worker), "{prompt}");
    assert!(prompt.contains(&format!("`{}`", work.slug)), "{prompt}");
    let synced = world.sync(&session).await;
    assert_eq!(synced["role_prompt"], "delivered", "{synced}");

    // 4. The session joins the target's pool as its skill does; the next read
    //    confirms it.
    join_pool(&world, &worker);
    let joined = world.sync(&session).await;
    assert!(joined["joined_at_ms"].as_u64().is_some(), "{joined}");

    // 5. The target's dispatcher assigns the requested ticket and the session claims
    //    it under its own worker id. The run follows the target's change feed.
    // It acts through this server, as its Hot Sheet tools do, so the server's change
    // feed carries the claim.
    let ticket_path = format!("/tickets/{}", work.id);
    let claimed = api(
        world.router.clone(),
        "POST",
        &format!("{ticket_path}/claim"),
        json!({"worker": worker, "lease_minutes": 30}),
        true,
    )
    .await;
    assert_eq!(claimed.0, StatusCode::OK, "{}", claimed.1);
    let dispatched = run_until(&world, &run_id, "dispatched to the session", |status| {
        status["state"] == "dispatched"
    })
    .await;
    assert_eq!(dispatched["worker_id"], worker.as_str(), "{dispatched}");

    // Repeated passes in all this time started nothing more and dispatched nothing more.
    let log = decisions(&world).await;
    assert_eq!(
        log.iter().filter(|entry| started(entry)).count(),
        1,
        "{log:?}"
    );
    assert_eq!(
        world
            .call("GET", "/runs", Value::Null)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        world
            .call("GET", "/sessions", Value::Null)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(drawer(&world.router).await.len(), 1);

    // 6. The work is finished: committed, completed and the claim released.
    commit_all(&world.project);
    let finished = api(
        world.router.clone(),
        "PATCH",
        &ticket_path,
        json!({"status": "completed"}),
        true,
    )
    .await;
    assert_eq!(finished.0, StatusCode::OK, "{}", finished.1);
    let released = api(
        world.router.clone(),
        "POST",
        &format!("{ticket_path}/release"),
        json!({"worker": worker}),
        true,
    )
    .await;
    assert_eq!(released.0, StatusCode::OK, "{}", released.1);
    run_until(&world, &run_id, "completed", |status| {
        status["state"] == "completed"
    })
    .await;
    let readiness = world
        .call("POST", "/readiness/show", json!({"step_id": step}))
        .await
        .1;
    assert_eq!(readiness["level"], "finished", "{readiness}");

    // 7. The session idles out and is wound down at its safe point.
    let (stop, _) = decided(
        &world,
        "wind-down",
        |entry| entry["kind"] == "wind_down" && entry["outcome"] == "wind_down_requested",
        3 * minute,
    )
    .await;
    assert_eq!(stop["session_id"], session["session_id"]);
    assert_eq!(stop["reason"], "idle");
    let after = world
        .call(
            "GET",
            &format!("/sessions/{}", session["session_id"].as_str().unwrap()),
            Value::Null,
        )
        .await
        .1;
    assert_eq!(after["state"], "stopped", "{after}");
    assert!(drawer(&world.router).await.is_empty());
    // Still one run and one session: the finished step was never dispatched again.
    assert_eq!(
        world
            .call("GET", "/runs", Value::Null)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let log = decisions(&world).await;
    assert_eq!(
        log.iter().filter(|entry| started(entry)).count(),
        1,
        "{log:?}"
    );
}

/// HS2-MBDKWQ: an AI terminal a person opens from the drawer in another registered
/// checkout starts with that checkout's own configuration, set up against its linked
/// store, as an orchestrator-started session does; the serving store is not set up for
/// it, and the HS1 launch-safety check looks at the checkout.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_mbdkwq_a_drawer_terminal_in_another_checkout_starts_with_its_configuration() {
    let world = World::new(&[], "", "").await;
    assert!(!world.project.join("AGENTS.md").exists());
    assert!(!world.project.join(".mcp.json").exists());
    let open = |id: &str, cwd: &Path| {
        super::orchestration::api(
            world.router.clone(),
            "POST",
            "/terminals",
            json!({"id": id, "connect": REPORT_TOOL, "cwd": cwd}),
            true,
        )
    };
    let (status, opened) = open("drawer-mbdkwq", &world.project).await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    let report = launch_report(&world.reports, "drawer-mbdkwq");
    assert_eq!(
        Path::new(report["cwd"].as_str().unwrap()),
        hotsheet_sync::paths::canonicalize(&world.project).unwrap(),
        "{report}"
    );
    assert_eq!(report["files"]["AGENTS.md"], true, "{report}");
    assert_eq!(report["files"][".mcp.json"], true, "{report}");
    // The checkout's MCP entry points at its own linked store, never the serving store,
    // and the serving store's directory got no project configuration for this launch.
    let mcp = std::fs::read_to_string(world.project.join(".mcp.json")).unwrap();
    assert!(
        mcp.contains(
            hotsheet_sync::paths::canonicalize(&world.project_store)
                .unwrap()
                .to_str()
                .unwrap()
        ),
        "{mcp}"
    );
    assert!(!mcp.contains(world.server_store.to_str().unwrap()), "{mcp}");
    assert!(!world.server_store.join(".mcp.json").exists());

    // A checkout that holds an HS1 store is refused before anything launches there, even
    // though the serving store is clean.
    std::fs::create_dir_all(world.project.join(".hotsheet/db")).unwrap();
    std::fs::write(world.project.join(".hotsheet/db/PG_VERSION"), "16").unwrap();
    let (status, refused) = open("drawer-mbdkwq-hs1", &world.project).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    let names: Vec<Value> = drawer(&world.router)
        .await
        .into_iter()
        .map(|tab| tab["id"].clone())
        .collect();
    assert!(!names.contains(&json!("drawer-mbdkwq-hs1")), "{names:?}");
}
