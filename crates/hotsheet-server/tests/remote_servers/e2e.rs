//! HS2-2EY9DP: end-to-end orchestration fixtures. A synthetic fleet of two real
//! servers: the controller hosts the orchestration project and the local target A,
//! and a peer reached over authenticated mTLS hosts the remote target B. Each target
//! runs a different fake AI tool (the workspace `hs-fake-agent` posing as a
//! Claude-like tool that takes its role by session push and a Codex-like tool that
//! takes it at launch); CI evidence comes from a loopback GitHub Actions provider
//! observed by a real Job monitor process. Nothing leaves loopback and no real AI
//! tool, provider account or GitHub repository is contacted.
//!
//! The scenarios built on it live in `e2e_loop.rs` and `e2e_failures.rs`; each is its
//! own test so that a failure names the seam it crossed.
use super::job_monitor::Provider;
use super::orchestration::{api, shared_runtime};
use super::session_work::{PUSH_AGENT, PUSH_PLUGIN, now_ms};
use super::sessions::{commit_all, git_project};
use super::*;
use hotsheet_cli::orchestration::{DetachedJobMonitor, OwnedSessions};
use hotsheet_server::AppState;
use hotsheet_ticketing::checkouts::{Checkout, CheckoutRegistry};
use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) const TARGET_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FK0";
pub(super) const TARGET_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FK1";
/// A Claude-like fake: its role prompt is pushed once the tool reports it is ready.
pub(super) const TOOL_A: &str = "fake-claude";
/// A Codex-like fake: its role prompt is on its command line at launch.
pub(super) const TOOL_B: &str = "fake-codex";
pub(super) const ACTOR: &str = "scheduler-1";
pub(super) const ACTIONS: [&str; 4] = [
    "tickets_read",
    "gates_observe",
    "sessions_manage",
    "work_start",
];
const MIB: u64 = 1024 * 1024;
/// The synthetic provider token the Job monitor presents to the loopback provider.
pub(super) const TOKEN: &str = "synthetic-token-for-tests";

/// The detached Job monitor a fleet server starts, pointed at the loopback provider
/// for GitHub Actions and (HS2-QV00AP) Buildkite and CircleCI, with synthetic tokens.
fn loopback_monitor(provider_url: &str) -> DetachedJobMonitor {
    DetachedJobMonitor::new("hotsheet-server")
        .flags(
            // HS2-R49D43: the unchanged-poll backoff capped, so a verdict after a slow
            // step is seen at the next poll, not minutes later.
            [
                "--poll-ms",
                "40",
                "--max-poll-ms",
                "500",
                "--idle-exit-ms",
                "3000",
            ]
            .map(String::from),
        )
        .env("HOTSHEET_GITHUB_API_URL", provider_url)
        .env("GH_TOKEN", TOKEN)
        .env("HOTSHEET_BUILDKITE_API_URL", provider_url)
        .env("BUILDKITE_API_TOKEN", TOKEN)
        .env("HOTSHEET_CIRCLECI_API_URL", provider_url)
        .env("CIRCLECI_TOKEN", TOKEN)
}

/// A roomy machine with the fake tools installed. HS2-RCR4A6: with an OS it also
/// reports its capabilities (HS2-ZY4Z4G), as a machine of that family would, so a
/// step's requirements are matched against it without probing the real host.
/// HS2-ZRPZJJ: with a sign-in status program, each tool's sign-in is that program's
/// exit, probed as the real host probes a declared `auth_status_args` command.
struct Roomy(
    Vec<&'static str>,
    Option<hotsheet_ticketing::orchestration::OsFamily>,
    Option<PathBuf>,
);
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
        Some(
            self.0
                .iter()
                .map(|tool| ((*tool).to_owned(), true))
                .collect(),
        )
    }
    fn capabilities(
        &mut self,
        _: &[PathBuf],
        _: &[String],
    ) -> Option<hotsheet_ticketing::orchestration::HostCapabilities> {
        use hotsheet_ticketing::orchestration::{HostCapabilities, OsFamily, ToolVersion};
        let Some(status) = &self.2 else {
            return self.1.map(|os| HostCapabilities::bare(os, "arm64"));
        };
        let args = ["auth".to_owned(), "status".to_owned()];
        Some(HostCapabilities {
            ai_tools: Some(
                self.0
                    .iter()
                    .map(|tool| ToolVersion {
                        tool: (*tool).to_owned(),
                        version: None,
                        signed_in: hotsheet_server::signed_in_at(
                            status,
                            &args,
                            Duration::from_secs(10),
                        ),
                    })
                    .collect(),
            ),
            ..HostCapabilities::bare(self.1.unwrap_or(OsFamily::Linux), "arm64")
        })
    }
}

fn workspace_binary(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current test executable");
    exe.parent()
        .and_then(Path::parent)
        .expect("target profile directory")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// One plugin directory holding one fake tool that reports what it saw at launch to
/// `<reports>/<terminal id>.json`, records every prompt it receives, and then holds.
fn plugin(root: &Path, tool: &str, reports: &Path) -> PathBuf {
    let fake_agent = workspace_binary("hs-fake-agent");
    assert!(fake_agent.is_file(), "build the workspace hs-fake-agent");
    std::fs::create_dir_all(reports).unwrap();
    let (agent, extra) = if tool == TOOL_A {
        (PUSH_AGENT, PUSH_PLUGIN.to_owned())
    } else {
        ("", r#"prompt_args = ["--prompt", "{prompt}"]"#.to_owned())
    };
    let plugins = root.join(format!("plugins-{tool}"));
    let directory = plugins.join(tool);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("instructions.md"), "Synthetic agent\n").unwrap();
    std::fs::write(
        directory.join("manifest.toml"),
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
args = ["--report-dir", {reports:?}, {agent} "--print", "ready", "--hold-ms", "600000"]
{extra}
"#
        ),
    )
    .unwrap();
    plugins
}

/// HS2-ZRPZJJ: a synthetic AI tool's sign-in status program. Like a real tool's
/// `auth status` it prints an account line whatever the answer, and only its exit says
/// whether the tool is signed in: 1 while a `signed-out` file is beside it, else 0. It
/// starts signed out.
fn sign_in_status(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let directory = root.join("sign-in");
    std::fs::create_dir_all(&directory).unwrap();
    let program = directory.join("auth-status");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\necho synthetic-account@example.invalid\n[ \"$1 $2\" = 'auth status' ] || exit 2\n[ -f {:?} ] && exit 1\nexit 0\n",
            directory.join("signed-out")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(directory.join("signed-out"), "").unwrap();
    program
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.test",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// HS2-RCR4A6: one bare ticket store and one bare code repository, each cloned once
/// for each machine. The store is bound to a `git_ref` claim backend on its shared
/// remote (HS2-WJF0D8) in its checked-in project registry, so both clones claim
/// through one compare-and-swap.
fn shared_clones(root: &Path, a_store: &Path, b_store: &Path, a_project: &Path, b_project: &Path) {
    use hotsheet_ticketing::claim_backends::{BackendConfiguration, BackendKind, Scope};
    let bare = |name: &str| {
        let path = root.join(name);
        git(
            root,
            &["init", "-q", "--bare", "-b", "main", path.to_str().unwrap()],
        );
        path
    };
    let publish = |clone: &Path, remote: &Path| {
        commit_all(clone);
        git(clone, &["branch", "-M", "main"]);
        git(
            clone,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(clone, &["push", "-q", "-u", "origin", "main"]);
    };
    let store_remote = bare("store.git");
    let store = FsStore::init_git(a_store, &StoreMetadata::new("TST")).unwrap();
    let sources = hotsheet_ticketing::claim_backends::git_store_sources(&store);
    Scope::Project {
        root: a_store.to_path_buf(),
    }
    .update(|registry| {
        registry.upsert_configuration(BackendConfiguration::new("shared", BackendKind::GitRef)?)?;
        registry.bind_source(&sources[0], "shared");
        Ok(())
    })
    .unwrap();
    publish(a_store, &store_remote);
    git(
        root,
        &[
            "clone",
            "-q",
            store_remote.to_str().unwrap(),
            b_store.to_str().unwrap(),
        ],
    );
    let code_remote = bare("code.git");
    git_project(a_project);
    std::fs::write(a_project.join("README.md"), "Synthetic project\n").unwrap();
    publish(a_project, &code_remote);
    git(
        root,
        &[
            "clone",
            "-q",
            code_remote.to_str().unwrap(),
            b_project.to_str().unwrap(),
        ],
    );
}

fn create(store: &Path, prefix: &str, title: &str) -> hotsheet_model::Ticket {
    ops::create(
        &FsStore::open(store).unwrap(),
        hotsheet_model::Ulid::new(),
        prefix,
        "2026-10-15T00:00:00Z".into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

/// How the fleet's two targets relate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Layout {
    /// Two projects: each server hosts its own store and code repository.
    Separate,
    /// HS2-RCR4A6: one project on two machines. Each server holds its own clone of one
    /// bare ticket store (bound to a `git_ref` claim backend, HS2-WJF0D8) and of one
    /// bare code repository; target B is declared a site of target A, the home
    /// (HS2-SHH7WW). The controller reports itself a Linux machine with both fake
    /// tools installed, the peer a Windows machine with only the push-driven one.
    Sites,
}

/// The controller runtime's shortened waits; `None` keeps each default.
#[derive(Clone, Copy)]
struct Timing {
    /// HS2-F7ZG0F: the first delay before an ended CI watch is linked again.
    relink_retry: Option<Duration>,
    /// HS2-EX594V: the delay before a home outage is announced.
    home_notify_after: Option<Duration>,
    /// HS2-1CC1TQ: the first wait before a home on record as out is read again.
    home_recheck_after: Option<Duration>,
}

/// Which server hosts a target's checkout and its sessions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Side {
    /// The controller: the orchestration project's own server, hosting target A.
    Controller,
    /// The peer, reached over mTLS, hosting target B.
    Peer,
}

pub(super) struct Fleet {
    pub(super) root: tempfile::TempDir,
    pub(super) paths: Paths,
    pub(super) memory: Arc<Memory>,
    pub(super) provider: Provider,
    pub(super) provider_url: String,
    pub(super) reports: PathBuf,
    // Controller: the orchestration project, and target A in the store it serves.
    pub(super) home: PathBuf,
    pub(super) owner_store: PathBuf,
    pub(super) owner: Checkout,
    pub(super) a_store: PathBuf,
    pub(super) a_project: PathBuf,
    pub(super) a_checkout: Checkout,
    a_plugins: Vec<PathBuf>,
    pub(super) layout: Layout,
    /// HS2-RCR4A6: each server's store watcher in [`Layout::Sites`], so a change pulled
    /// from the shared remote reaches that server's change feed as on a real host.
    watchers: Vec<hotsheet_server::WatchHandle>,
    pub(super) controller: AppState,
    pub(super) router: axum::Router,
    pub(super) base: String,
    // Peer: target B in the store it serves.
    pub(super) peer_home: PathBuf,
    pub(super) b_store: PathBuf,
    pub(super) b_project: PathBuf,
    pub(super) b_checkout: Checkout,
    pub(super) peer_state: AppState,
    pub(super) peer_api: axum::Router,
    pub(super) peer: Option<Running>,
    pub(super) peer_address: std::net::SocketAddr,
    /// HS2-XHQ1EH: the remembered rules a person set up on each target, by id.
    pub(super) rules: HashMap<&'static str, Vec<String>>,
    /// The policy revision last stored.
    pub(super) revision: Value,
    /// HS2-F7ZG0F: the controller's first delay before it links an ended CI watch
    /// again; `None` keeps the server's default.
    relink_retry: Option<Duration>,
    /// HS2-EQ7K35: the peer's target B is the project's home and the controller's
    /// target A a site of it, so the orchestrator runs away from the home.
    peer_is_home: bool,
    /// HS2-ZRPZJJ: the controller's sign-in status program, when its tools report
    /// sign-in: it exits 1 (signed out) while a `signed-out` file is beside it.
    auth_status: Option<PathBuf>,
    /// HS2-EX594V: the controller's delay before a home outage is announced; `None`
    /// keeps the runtime's default.
    home_notify_after: Option<Duration>,
    /// HS2-1CC1TQ: the controller's first wait before it reads a home on record as out
    /// again; `None` keeps the runtime's default.
    home_recheck_after: Option<Duration>,
}

impl Fleet {
    /// Two servers, both targets declared, bound, with remembered rules and allowed by a policy
    /// whose placement is not automatic yet.
    pub(super) async fn new() -> Self {
        Self::build(Layout::Separate, false, None, None, false, false).await
    }

    /// HS2-ZRPZJJ: [`Fleet::new`] whose controller reports its tool's sign-in from a
    /// sign-in status program, signed out until [`Fleet::sign_in`].
    pub(super) async fn signed_out() -> Self {
        Self::build(Layout::Separate, false, None, None, false, true).await
    }

    /// HS2-QV00AP: [`Fleet::new`] whose peer also starts a real Job monitor process
    /// for its hosted watches and pipeline reads, pointed at the loopback provider as
    /// the controller's is.
    pub(super) async fn with_peer_monitor() -> Self {
        Self::build(Layout::Separate, false, None, None, true, false).await
    }

    /// HS2-RCR4A6: the same two servers holding one project as two sites ([`Layout::Sites`]).
    pub(super) async fn sites() -> Self {
        Self::build(Layout::Sites, false, None, None, false, false).await
    }

    /// HS2-EQ7K35: the same two sites with the peer's target B as the project's home and
    /// the controller's target A a site of it. The controller announces a home outage
    /// after `notify_after`.
    pub(super) async fn sites_peer_home(notify_after: Duration) -> Self {
        Self::build(Layout::Sites, true, Some(notify_after), None, false, false).await
    }

    /// HS2-1CC1TQ: [`Self::sites_peer_home`] whose controller reads a home on record as
    /// out again after `recheck_after` at first (doubling with the outage's age up to
    /// thirty times that), so its loop sees the home back by itself.
    pub(super) async fn sites_peer_home_rechecked(
        notify_after: Duration,
        recheck_after: Duration,
    ) -> Self {
        Self::build(
            Layout::Sites,
            true,
            Some(notify_after),
            Some(recheck_after),
            false,
            false,
        )
        .await
    }

    async fn build(
        layout: Layout,
        peer_is_home: bool,
        home_notify_after: Option<Duration>,
        home_recheck_after: Option<Duration>,
        peer_monitor: bool,
        signed_out: bool,
    ) -> Self {
        assert!(hotsheet_aitools::test_permission_vault_available());
        hotsheet_aitools::enable_test_permission_vault();
        let root = tempfile::tempdir().unwrap();
        let reports = root.path().join("reports");
        // HS2-ZRPZJJ: written before anything starts a process, so no child holds it open.
        let auth_status = signed_out.then(|| sign_in_status(root.path()));
        let provider = Provider::default();
        let provider_url = provider.start();

        let a_store = root.path().join("a-store");
        let b_store = root.path().join("b-store");
        let a_project = root.path().join("a-project");
        let b_project = root.path().join("b-project");
        let (peer_tools, controller_tools, peer_os, controller_os) = match layout {
            Layout::Separate => {
                git_project(&b_project);
                git_project(&a_project);
                (vec![TOOL_B, TOOL_A], vec![TOOL_A], None, None)
            }
            Layout::Sites => {
                // Each machine merges the shared store's tickets with the semantic merge
                // driver its own CLI registers, as a server and a CLI do at startup.
                hotsheet_ticketing::merge_driver::set_program(super::support::cli_binary());
                shared_clones(root.path(), &a_store, &b_store, &a_project, &b_project);
                use hotsheet_ticketing::orchestration::OsFamily;
                (
                    vec![TOOL_A],
                    vec![TOOL_A, TOOL_B],
                    Some(OsFamily::Windows),
                    Some(OsFamily::Linux),
                )
            }
        };

        // The peer: a server over mTLS hosting target B and its fake tool.
        let paths = Paths::at(root.path().join("tls"));
        init_ca(&paths, &[]).unwrap();
        let device = issue_device(&paths, "synthetic-orchestrator").unwrap();
        set_device_role(&paths, "synthetic-orchestrator", DeviceRole::ReadWrite).unwrap();
        let peer_home = root.path().join("peer-home");
        // Both fake tools are installed on the peer; the policy lets target B run only
        // its own until a scenario says otherwise.
        let peer_state = state(&b_store, &peer_home)
            .with_capacity_source(Box::new(Roomy(peer_tools, peer_os, None)))
            .with_capacity_cache(Duration::ZERO)
            .with_plugin_dirs(vec![
                plugin(root.path(), TOOL_B, &reports),
                plugin(root.path(), TOOL_A, &reports),
            ]);
        let peer_state = if peer_monitor {
            peer_state.with_orchestration_runtime(
                shared_runtime(&b_store, &peer_home, &Arc::new(Memory::default()))
                    .with_job_monitor(Arc::new(loopback_monitor(&provider_url))),
            )
        } else {
            peer_state
        };
        let b_checkout = CheckoutRegistry::new(peer_home.join("checkouts.json"))
            .register(&b_project, None, None, vec![b_store.clone()])
            .unwrap();
        let expected = identity(&peer_home);
        let peer_api = app(peer_state.clone());
        let peer = serve(peer_state.clone(), &paths, None).await;
        let peer_address = peer.address;

        // The controller: the orchestration project with its own store, and target A
        // in the store the controller serves, so ticket writes reach its change feed.
        let home = root.path().join("controller-home");
        std::fs::create_dir_all(&home).unwrap();
        let a_plugins: Vec<PathBuf> = controller_tools
            .iter()
            .map(|tool| plugin(root.path(), tool, &reports))
            .collect();
        let owner_store = root.path().join("owner-store");
        FsStore::init_git(&owner_store, &StoreMetadata::new("OWN")).unwrap();
        let owner_project = root.path().join("release-train");
        std::fs::create_dir_all(&owner_project).unwrap();
        let registry = CheckoutRegistry::new(home.join("checkouts.json"));
        let owner = registry
            .register(&owner_project, None, None, vec![owner_store.clone()])
            .unwrap();
        let memory = Arc::new(Memory::default());
        super::support::runtime(&home, &memory)
            .register(hotsheet_ticketing::remote_servers::RegisterRemote {
                id: ID.into(),
                label: "Synthetic peer".into(),
                origin: peer.origin.clone(),
                expected_server_id: expected.server_id,
                credentials: credentials(&device),
            })
            .unwrap();
        let controller = Self::controller_state(
            &a_store,
            &home,
            &a_plugins,
            &memory,
            &provider_url,
            Roomy(controller_tools, controller_os, auth_status.clone()),
            Timing {
                relink_retry: None,
                home_notify_after,
                home_recheck_after,
            },
        );
        let a_checkout = registry
            .register(&a_project, None, None, vec![a_store.clone()])
            .unwrap();
        let router = app(controller.clone());
        let base = format!("/checkouts/{}/orchestration", owner.id);
        let watchers = match layout {
            Layout::Separate => Vec::new(),
            Layout::Sites => vec![
                hotsheet_server::spawn_watcher(controller.clone()).unwrap(),
                hotsheet_server::spawn_watcher(peer_state.clone()).unwrap(),
            ],
        };
        let mut fleet = Self {
            root,
            paths,
            memory,
            provider,
            provider_url,
            reports,
            home,
            owner_store,
            owner,
            a_store,
            a_project,
            a_checkout,
            a_plugins,
            layout,
            watchers,
            controller,
            router,
            base,
            peer_home,
            b_store,
            b_project,
            b_checkout,
            peer_state,
            peer_api,
            peer: Some(peer),
            peer_address,
            rules: HashMap::new(),
            revision: Value::Null,
            relink_retry: None,
            peer_is_home,
            auth_status,
            home_notify_after,
            home_recheck_after,
        };
        fleet.declare().await;
        fleet.policy(json!({})).await;
        fleet
    }

    /// The controller's state over `home`: its own capacity and tool, and the
    /// orchestration runtime a server composes (`OrchestrationRuntime::live` with the
    /// in-memory credentials of the synthetic remote registration), with the Job
    /// monitor it starts pointed at the loopback provider.
    fn controller_state(
        a_store: &Path,
        home: &Path,
        plugins: &[PathBuf],
        memory: &Arc<Memory>,
        provider_url: &str,
        capacity: Roomy,
        timing: Timing,
    ) -> AppState {
        let Timing {
            relink_retry,
            home_notify_after,
            home_recheck_after,
        } = timing;
        // The runtime a server composes with `OrchestrationRuntime::live`, over the
        // in-memory credentials of the synthetic remote registration.
        let runtime = shared_runtime(a_store, home, memory)
            .with_session_executor(Arc::new(OwnedSessions))
            .with_job_monitor(Arc::new(loopback_monitor(provider_url)));
        let runtime = match relink_retry {
            Some(first) => runtime.with_gate_relink_retry(first),
            None => runtime,
        };
        let runtime = match home_notify_after {
            Some(after) => runtime.with_home_notify_after(after),
            None => runtime,
        };
        let runtime = match home_recheck_after {
            Some(base) => runtime.with_home_recheck_after(base),
            None => runtime,
        };
        // HS2-ZRPZJJ: a sign-in change is seen at the next reading, not ten minutes on.
        let capabilities_ttl = if capacity.2.is_some() {
            Duration::ZERO
        } else {
            Duration::from_secs(600)
        };
        state(a_store, home)
            .with_capacity_source(Box::new(capacity))
            .with_capacity_cache(Duration::ZERO)
            .with_capabilities_ttl(capabilities_ttl)
            .with_plugin_dirs(plugins.to_vec())
            .with_orchestration_runtime(runtime)
    }

    /// A controller route of the owning checkout's orchestration surface.
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

    /// The same, asserting success.
    pub(super) async fn ok(&self, method: &str, path: &str, body: Value) -> Value {
        let (status, value) = self.call(method, path, body).await;
        assert_eq!(status, StatusCode::OK, "{method} {path}: {value}");
        value
    }

    pub(super) fn ai() -> Value {
        json!({"role":"ai","id":ACTOR})
    }

    pub(super) fn person() -> Value {
        json!({"role":"human","id":"maintainer"})
    }

    async fn declare(&mut self) {
        let mut revision = Value::Null;
        let mut targets = [
            (
                TARGET_A,
                json!({"kind":"local","checkout_id":self.a_checkout.id}),
            ),
            (
                TARGET_B,
                json!({"kind":"remote","registration_id":ID,"checkout_id":self.b_checkout.id}),
            ),
        ];
        // HS2-EQ7K35: the home is declared first, so its site can name it.
        if self.peer_is_home {
            targets.reverse();
        }
        for (target, destination) in targets {
            let mut body = json!({"target_id":target,"label":format!("Target {target}"),
                "expected_revision":revision});
            // HS2-RCR4A6: target B is another site of target A's project; HS2-EQ7K35:
            // or target A is another site of target B's.
            let (site, home) = if self.peer_is_home {
                (TARGET_A, TARGET_B)
            } else {
                (TARGET_B, TARGET_A)
            };
            if self.layout == Layout::Sites && target == site {
                body["site_of"] = json!(home);
            }
            let added = self.ok("POST", "/targets", body).await;
            revision = added["revision"].clone();
            let candidate = self
                .ok("POST", "/inspect", json!({"destination":destination}))
                .await;
            let bound = self
                .ok(
                    "PUT",
                    &format!("/targets/{target}/binding"),
                    json!({"target":{"owner_checkout_id":self.owner.id,"target_id":target},
                        "expected_manifest_revision":revision,"expected_binding_revision":0,
                        "previous_destination":null,"destination":candidate["destination"]}),
                )
                .await;
            assert_eq!(bound["usable"], true, "{bound}");
            let rules = self.remember(target).await;
            self.rules.insert(target, rules);
        }
    }

    /// The tool each target's sessions run: its own in [`Layout::Separate`], the
    /// push-driven one on both sites in [`Layout::Sites`], so a site's session can be
    /// told its assignment after launch.
    pub(super) fn tool(&self, target: &str) -> &'static str {
        if target == TARGET_A || self.layout == Layout::Sites {
            TOOL_A
        } else {
            TOOL_B
        }
    }

    /// HS2-XHQ1EH: adding and binding the target consented to reading and following
    /// it; a person remembers a rule for every act the loop needs there (dispatch and
    /// a session start of the target's tool), so the loop asks nobody. Returns the
    /// rule ids.
    pub(super) async fn remember(&self, target: &str) -> Vec<String> {
        self.remember_tools(target, &[self.tool(target)]).await
    }

    /// The same rules for the given tools.
    pub(super) async fn remember_tools(&self, target: &str, tools: &[&str]) -> Vec<String> {
        let mut bodies = vec![json!({"action":"work_start"})];
        bodies.extend(
            tools
                .iter()
                .map(|tool| json!({"action":"session_start","tool":tool})),
        );
        let mut ids = Vec::new();
        for mut body in bodies {
            body["target_id"] = json!(target);
            body["actor"] = Self::person();
            let remembered = self.ok("POST", "/permission-rules", body).await;
            ids.push(remembered["rule_id"].as_str().unwrap().to_owned());
        }
        ids
    }

    /// A person stores the policy: each target runs its own fake tool, one session
    /// each. `extra` adds or replaces top-level members such as `placement`.
    pub(super) async fn policy(&mut self, extra: Value) -> Value {
        let mut policy = json!({"machine":{"max_sessions":4},
            "tools":[{"tool":TOOL_A,"max_sessions":4},{"tool":TOOL_B,"max_sessions":4}],
            "targets":[
                {"target_id":TARGET_A,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1},
                {"target_id":TARGET_B,"actions":ACTIONS,"tools":[{"tool":self.tool(TARGET_B)}],
                    "max_sessions":1}],
            "wind_down":{"idle_minutes":1,"keep_warm":0}});
        policy
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let current = self.ok("GET", "/policy", Value::Null).await;
        let stored = self
            .ok(
                "PUT",
                "/policy",
                json!({"expected_revision":current["revision"],"policy":policy,
                    "actor":Self::person()}),
            )
            .await;
        self.revision = stored["revision"].clone();
        stored
    }

    /// A person turns on automatic placement for the scheduler's actor.
    pub(super) async fn automatic(&mut self) -> Value {
        self.policy(json!({"placement":{"automatic":true,"actor":ACTOR,"cooldown_minutes":0}}))
            .await
    }

    /// A synthetic orchestration ticket in the owner's store.
    pub(super) fn orchestration_ticket(&self, title: &str) -> hotsheet_model::Ticket {
        create(&self.owner_store, "OWN", title)
    }

    /// A synthetic target ticket in the store of `side`.
    pub(super) fn target_ticket(&self, side: Side, title: &str) -> hotsheet_model::Ticket {
        match side {
            Side::Controller => create(&self.a_store, "TST", title),
            Side::Peer => create(&self.b_store, "TST", title),
        }
    }

    pub(super) fn connection(&self, side: Side) -> String {
        match side {
            Side::Controller => self.a_checkout.sources[0].connection_id.clone(),
            Side::Peer => self.b_checkout.sources[0].connection_id.clone(),
        }
    }

    pub(super) fn target(side: Side) -> &'static str {
        match side {
            Side::Controller => TARGET_A,
            Side::Peer => TARGET_B,
        }
    }

    /// Plan one step: the orchestration ticket's work on a target ticket, after the
    /// given prerequisites.
    pub(super) async fn plan(
        &self,
        orchestration: &hotsheet_model::Ticket,
        side: Side,
        work: &hotsheet_model::Ticket,
        prerequisites: Value,
    ) -> Value {
        self.ok(
            "POST",
            "/readiness/define",
            json!({"actor":Self::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":Self::target(side),"connection_id":self.connection(side),
                "native_id":work.slug,"prerequisites":prerequisites}),
        )
        .await
    }

    /// A prerequisite: that target ticket finished with its CI gates green.
    pub(super) fn after(&self, side: Side, ticket: &hotsheet_model::Ticket) -> Value {
        json!({"kind":"ticket","target_id":Self::target(side),
            "connection_id":self.connection(side),"native_id":ticket.slug,"gates":"ci"})
    }

    pub(super) async fn readiness(&self, step: &str) -> Value {
        self.ok("POST", "/readiness/show", json!({"step_id":step}))
            .await
    }

    // ----- Waiting: woken by the stream or the files, never by a timer. -----

    /// Every decision record of the owner's scheduler, in order.
    pub(super) async fn decisions(&self) -> Vec<Value> {
        let mut entries = Vec::new();
        let mut after = 0;
        loop {
            let page = self
                .ok(
                    "POST",
                    "/scheduler/decisions",
                    json!({"after": after, "limit": 200}),
                )
                .await;
            entries.extend(page["entries"].as_array().unwrap().iter().cloned());
            after = page["cursor"].as_u64().unwrap();
            if page["more"] != true {
                return entries;
            }
        }
    }

    /// Waits, woken by the decision log changing, for the first decision `wanted`.
    pub(super) async fn decided(
        &self,
        what: &str,
        wanted: impl Fn(&Value) -> bool,
        within: Duration,
    ) -> Value {
        let directory = self.home.join("orchestration-scheduler-decisions");
        std::fs::create_dir_all(&directory).unwrap();
        // HS2-R49D43: read first. A synchronous stream start took over a minute on a
        // loaded machine, long enough for a refusal backoff to run out unobserved.
        let waiter = hotsheet_jobs::ChangeWaiter::starting(&[directory.as_path()]).unwrap();
        let deadline = std::time::Instant::now() + within;
        loop {
            let log = self.decisions().await;
            if let Some(found) = log.iter().find(|entry| wanted(entry)) {
                return found.clone();
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!remaining.is_zero(), "no {what}: {log:#?}");
            tokio::task::block_in_place(|| waiter.wait(remaining.min(Duration::from_secs(5))));
        }
    }

    /// One long poll on the owner's run event stream, filtered to one run or plan
    /// step: returns when something new is recorded, or after five seconds.
    pub(super) async fn next_event(&self, id: Option<&str>, cursor: u64) -> u64 {
        let filter = id.map_or(String::new(), |id| format!("&run={id}"));
        let page = self
            .ok(
                "GET",
                &format!("/run-events?after={cursor}{filter}&wait_ms=5000"),
                Value::Null,
            )
            .await;
        page["cursor"].as_u64().unwrap_or(cursor)
    }

    /// The run's status once `wanted` holds, woken by the run's event stream.
    pub(super) async fn run_until(
        &self,
        run_id: &str,
        what: &str,
        wanted: impl Fn(&Value) -> bool,
    ) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        let mut cursor = 0;
        loop {
            let status = self
                .ok("GET", &format!("/runs/{run_id}/status"), Value::Null)
                .await;
            if wanted(&status) {
                return status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "run never {what}: {status:#}"
            );
            cursor = self.next_event(Some(run_id), cursor).await;
        }
    }

    /// The step's readiness once it reaches `level`, woken by its readiness events.
    pub(super) async fn step_until(&self, step: &str, level: &str) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        let mut cursor = 0;
        loop {
            let readiness = self.readiness(step).await;
            if readiness["level"] == level {
                return readiness;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "step never {level}: {readiness:#}"
            );
            cursor = self.next_event(Some(step), cursor).await;
        }
    }

    /// The gate once it reaches `state`, woken by the owner's gate events.
    pub(super) async fn gate_until(&self, gate_id: &str, state: &str) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        let mut cursor = 0;
        loop {
            let gate = self
                .ok("GET", &format!("/gates/{gate_id}"), Value::Null)
                .await;
            if gate["state"] == state || gate["gate"]["state"] == state {
                return gate;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "gate never {state}: {gate:#}"
            );
            cursor = self.next_event(None, cursor).await;
        }
    }

    /// One orchestrator session record.
    pub(super) async fn session(&self, session_id: &str) -> Value {
        self.ok("GET", &format!("/sessions/{session_id}"), Value::Null)
            .await
    }

    /// Every orchestrator session record.
    pub(super) async fn sessions(&self) -> Vec<Value> {
        self.ok("GET", "/sessions", Value::Null)
            .await
            .as_array()
            .unwrap()
            .clone()
    }

    /// Every run record.
    pub(super) async fn runs(&self) -> Vec<Value> {
        self.ok("GET", "/runs", Value::Null)
            .await
            .as_array()
            .unwrap()
            .clone()
    }

    /// The terminals of `side`'s drawer.
    pub(super) async fn drawer(&self, side: Side) -> Vec<Value> {
        let router = match side {
            Side::Controller => self.router.clone(),
            Side::Peer => self.peer_api.clone(),
        };
        super::sessions::drawer(&router).await
    }

    // ----- Playing the agent: exactly what the worker skill does, through the
    // ----- hosting server's own routes, so its change feed carries each write.

    pub(super) fn side_router(&self, side: Side) -> axum::Router {
        match side {
            Side::Controller => self.router.clone(),
            Side::Peer => self.peer_api.clone(),
        }
    }

    /// The session registers in its target's coordination pool under its worker id.
    pub(super) fn join_pool(&self, side: Side, worker_id: &str) {
        let (project, store, home) = match side {
            Side::Controller => (&self.a_project, &self.a_store, &self.home),
            Side::Peer => (&self.b_project, &self.b_store, &self.peer_home),
        };
        let coordinator = hotsheet_cli::coordination::Coordinator::open_with_registry_home(
            project,
            store,
            home.clone(),
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

    pub(super) async fn claim(&self, side: Side, ticket: &hotsheet_model::Ticket, worker: &str) {
        let (status, body) = api(
            self.side_router(side),
            "POST",
            &format!("/tickets/{}/claim", ticket.id),
            json!({"worker": worker, "lease_minutes": 30}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    /// Commit the session's work, complete the ticket and release the claim.
    pub(super) async fn finish(&self, side: Side, ticket: &hotsheet_model::Ticket, worker: &str) {
        commit_all(self.project(side));
        let router = self.side_router(side);
        let (status, body) = api(
            router.clone(),
            "PATCH",
            &format!("/tickets/{}", ticket.id),
            json!({"status": "completed"}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, body) = api(
            router,
            "POST",
            &format!("/tickets/{}/release", ticket.id),
            json!({"worker": worker}),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    pub(super) fn project(&self, side: Side) -> &Path {
        match side {
            Side::Controller => &self.a_project,
            Side::Peer => &self.b_project,
        }
    }

    /// The decision that started a session for `step`, once the loop made it.
    pub(super) async fn started_for(&self, step: &str, within: Duration) -> Value {
        self.decided(
            &format!("a started session for {step}"),
            |entry| {
                entry["kind"] == "start"
                    && entry["outcome"] == "started"
                    && entry["step_id"] == step
            },
            within,
        )
        .await
    }

    /// The session a start decision began, and its hosted terminal and worker ids.
    pub(super) async fn started_session(&self, decision: &Value) -> (Value, String, String) {
        let session = self
            .session(
                decision["session_id"]
                    .as_str()
                    .expect("the start names its session"),
            )
            .await;
        let terminal = session["hosted"]["session_id"].as_str().unwrap().to_owned();
        let worker = session["hosted"]["worker_id"].as_str().unwrap().to_owned();
        (session, terminal, worker)
    }

    /// HS2-ZRPZJJ: the controller's tool signs in (or out again), as a person does in
    /// a terminal there. Only a [`Fleet::signed_out`] fleet reports sign-in.
    pub(super) fn sign_in(&self, signed_in: bool) {
        let status = self
            .auth_status
            .as_ref()
            .expect("a fleet that reports sign-in");
        let flag = status.with_file_name("signed-out");
        if signed_in {
            std::fs::remove_file(flag).unwrap();
        } else {
            std::fs::write(flag, "").unwrap();
        }
    }

    // ----- Failures -----

    /// The peer's listener goes away; its sessions and store stay as they are, as on
    /// a machine that dropped off the network.
    pub(super) async fn peer_offline(&mut self) {
        self.peer.take().expect("the peer is online").stop().await;
    }

    /// The peer is reachable again at the same address.
    pub(super) async fn peer_online(&mut self) {
        assert!(self.peer.is_none(), "the peer is online");
        self.peer = Some(
            serve(
                self.peer_state.clone(),
                &self.paths,
                Some(self.peer_address),
            )
            .await,
        );
    }

    /// The controller process stops and a new one starts over the same home. Its
    /// start-up alone follows unfinished runs and resumes the scheduling loop.
    pub(super) async fn restart_controller(&mut self) {
        self.controller.begin_stopping();
        // The terminals the controller hosts die with its process.
        for tab in self.drawer(Side::Controller).await {
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
        let (tools, os) = match self.layout {
            Layout::Separate => (vec![TOOL_A], None),
            Layout::Sites => (
                vec![TOOL_A, TOOL_B],
                Some(hotsheet_ticketing::orchestration::OsFamily::Linux),
            ),
        };
        let next = Self::controller_state(
            &self.a_store,
            &self.home,
            &self.a_plugins,
            &self.memory,
            &self.provider_url,
            Roomy(tools, os, self.auth_status.clone()),
            Timing {
                relink_retry: self.relink_retry,
                home_notify_after: self.home_notify_after,
                home_recheck_after: self.home_recheck_after,
            },
        );
        next.spawn_run_recovery();
        self.router = app(next.clone());
        self.controller = next;
    }

    /// HS2-F7ZG0F: a controller whose gate follower links an ended CI watch again
    /// after `first`, doubling per attempt, instead of the server's default. Started
    /// over the same home, as [`Self::restart_controller`] does.
    pub(super) async fn relink_after(&mut self, first: Duration) {
        self.relink_retry = Some(first);
        self.restart_controller().await;
    }

    /// The controller's API on a plain loopback listener, for the stdio MCP tool's
    /// `--server` backend. Returns the origin and the listener's stop handle.
    pub(super) async fn serve_plain(&self) -> (String, PlainListener) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let serving = self.controller.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app(serving))
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        (origin, PlainListener { stop, task })
    }

    /// One `hotsheet_targets` call through the stdio MCP tool against `origin`, as the
    /// scheduler's actor on the owning checkout.
    pub(super) fn mcp(&self, origin: &str, mut arguments: Value) -> Value {
        arguments["checkout"] = json!(self.owner.id);
        arguments["actor_role"] = json!("ai");
        arguments["actor_id"] = json!(ACTOR);
        mcp_named(
            &self.home,
            &["--server", origin, "--secret", SECRET],
            "hotsheet_targets",
            arguments,
        )
    }

    /// Stop everything this fleet started outside the test process.
    pub(super) async fn shut_down(mut self) {
        self.controller.begin_stopping();
        self.peer_state.begin_stopping();
        if let Some(peer) = self.peer.take() {
            peer.stop().await;
        }
        cli_named(&self.home, &["job", "watch"], &["stop"]);
        drop(std::mem::take(&mut self.watchers));
        drop(self.root);
    }
}

pub(super) struct PlainListener {
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl PlainListener {
    pub(super) async fn stop(self) {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap();
    }
}

/// Rewrite a project's coordination pool as the role commands would leave it.
///
/// HS2-M2KQPA: this holds the pool's own mutation lock across the read and the write and
/// replaces the file atomically, as `Coordinator` does. The loop writes the same pool
/// concurrently (renewing a member, recording a site handoff), so an unlocked
/// read-modify-write here could be overwritten by a loop write that read the pool before
/// it, losing the edit, or let the loop read a half-written file.
pub(super) fn edit_pool_state(
    project: &Path,
    change: impl FnOnce(&mut hotsheet_cli::coordination::State),
) {
    let directory = project.join(".git").join("hotsheet-coordination");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("state.lock"))
        .unwrap();
    let _lock = hotsheet_ticketing::FileLock::acquire(lock).unwrap();
    let path = directory.join("state.json");
    let mut state: hotsheet_cli::coordination::State =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    change(&mut state);
    state.revision += 1;
    let mut temporary = tempfile::NamedTempFile::new_in(&directory).unwrap();
    std::io::Write::write_all(&mut temporary, &serde_json::to_vec(&state).unwrap()).unwrap();
    hotsheet_sync::files::persist(temporary, &path).unwrap();
}
