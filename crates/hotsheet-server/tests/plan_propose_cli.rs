#![cfg(unix)] // Process groups and `flock` for the fixture.

//! HS2-YWN8JT: the headless `hotsheet-cli orchestration plan propose` against a real
//! running `hotsheet-server` starts the planner on its first try, even while the
//! server's terminal launch waits on the shared permission vault, as it did behind an
//! OS credential prompt in the HS2-4RVDJC trial. The planner is the workspace fake
//! agent; no AI tool is launched and nothing leaves loopback. Synthetic data only.

use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
use serde_json::{Value, json};

#[path = "../../test-support/first_launch.rs"]
mod first_launch;
#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

const SECRET: &str = "synthetic-plan-propose";
const TOOL: &str = "fake-report";
const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FJ8";
/// How long the fixture holds the permission vault while the launch waits on it.
const VAULT_HELD: Duration = Duration::from_secs(3);

/// Kills the server's process group on drop, so a failed assertion leaves nothing.
struct Server {
    child: Child,
    log: PathBuf,
    url: String,
}

impl Server {
    fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: the negative id is this fixture's own process group.
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

struct World {
    _dir: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    store: PathBuf,
    reports: PathBuf,
    path: String,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir(&home).unwrap();
        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();
        let status = hotsheet_ticketing::git::launch("git")
            .arg("-C")
            .arg(&project)
            .args(["init", "-q"])
            .status()
            .unwrap();
        assert!(status.success());
        let store = dir.path().join("store");
        FsStore::init_git(&store, &StoreMetadata::new("ORC")).unwrap();
        let reports = dir.path().join("reports");
        fs::create_dir(&reports).unwrap();
        // The workspace binaries are siblings in one snapshot: the fake agent is the
        // plugin's detected binary, and the server finds `hotsheet-cli` beside itself.
        // Launched once to completion first, so the planner's launch below does not wait on
        // the macOS first-launch assessment (HS2-04D6DW).
        let fake_agent =
            first_launch::launched(workspace_bin::path("hs-fake-agent"), &["--hold-ms", "0"]);
        let bin = fake_agent.parent().unwrap().to_path_buf();
        let plugin = home.join("plugins").join(TOOL);
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("instructions.md"), "Synthetic agent\n").unwrap();
        fs::write(
            plugin.join("manifest.toml"),
            format!(
                r#"
id = "{TOOL}"
display_name = "{TOOL}"
product_name = "Synthetic agent"
tier = "cli-agent"
[detection]
binaries = ["hs-fake-agent"]
[drive]
transport = "acp"
program = "hs-fake-agent"
args = []
content = "arg"
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
args = ["--report-dir", {reports:?}, "--print", "ready", "--hold-ms", "600000"]
prompt_args = ["--prompt", "{{prompt}}"]
"#
            ),
        )
        .unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.display());
        Self {
            _dir: dir,
            home,
            project,
            store,
            reports,
            path,
        }
    }

    fn server(&self) -> Server {
        let log = self.home.join("server.log");
        let output = fs::File::create(&log).unwrap();
        let child = Command::new(first_launch::launched(
            workspace_bin::path("hotsheet-server"),
            &["--version"],
        ))
        .arg("-C")
        .arg(&self.store)
        .args(["--bind", "127.0.0.1:0", "--secret", SECRET])
        .args(["--no-terminal-broker", "--exit-on-stdin-eof"])
        .current_dir(&self.project)
        .env("HOTSHEET_HOME", &self.home)
        .env("PATH", &self.path)
        // Admission reads a roomy machine, whatever load this test machine is under.
        .env(hotsheet_server::ROOMY_CAPACITY_ENV, "1")
        // Piped and held: the server stops if this test process dies (HS2-VQ8ZWT).
        .stdin(Stdio::piped())
        .stdout(output.try_clone().unwrap())
        .stderr(output)
        .process_group(0)
        .spawn()
        .unwrap();
        // Ready once it has published its instance record, the record the CLI finds.
        let instance = self
            .home
            .join("instances")
            .join(format!("{}.json", hotsheet_tls::project_id(&self.store)));
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut server = Server {
            child,
            log,
            url: String::new(),
        };
        loop {
            if let Some(url) = fs::read_to_string(&instance)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|record| record["url"].as_str().map(str::to_owned))
            {
                server.url = url;
                return server;
            }
            assert!(
                Instant::now() < deadline,
                "the server never published its instance record: {}",
                server.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn cli(&self, arguments: &[&str]) -> Output {
        Command::new(workspace_bin::path("hotsheet-cli"))
            .args(["--actor-role", "human", "--actor-id", "maintainer", "-C"])
            .arg(&self.store)
            .args(arguments)
            .current_dir(&self.project)
            .env("HOTSHEET_HOME", &self.home)
            .env("PATH", &self.path)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn call(server: &Server, method: &str, path: &str, body: Value) -> Value {
    let request = ureq::request(method, &format!("{}{path}", server.url))
        .set("X-Hotsheet-Secret", SECRET)
        .timeout(Duration::from_secs(60));
    let response = if body.is_null() {
        request.call()
    } else {
        request
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
    };
    match response {
        Ok(response) => serde_json::from_reader(response.into_reader()).unwrap_or(Value::Null),
        Err(ureq::Error::Status(status, response)) => {
            panic!(
                "{method} {path}: {status} {}",
                response.into_string().unwrap_or_default()
            )
        }
        Err(error) => panic!("{method} {path}: {error}"),
    }
}

/// Hold the machine's permission vault the way a credential prompt holds it, then let
/// it go after [`VAULT_HELD`].
fn hold_vault(home: &Path) -> std::thread::JoinHandle<()> {
    let directory = home.join("permissions");
    fs::create_dir_all(&directory).unwrap();
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(".permission-master.lock"))
        .unwrap();
    // SAFETY: a valid open descriptor this fixture owns.
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);
    std::thread::spawn(move || {
        std::thread::sleep(VAULT_HELD);
        drop(lock);
    })
}

#[test]
fn hs2_ywn8jt_the_first_headless_propose_starts_the_planner_on_a_running_server() {
    let world = World::new();
    let server = world.server();
    // The orchestration project orchestrates itself as its one bound target.
    let checkout = call(
        &server,
        "POST",
        "/checkouts",
        json!({"root": world.project, "stores": [world.store]}),
    );
    let owner = checkout["id"].as_str().unwrap().to_owned();
    let base = format!("/checkouts/{owner}/orchestration");
    let added = call(
        &server,
        "POST",
        &format!("{base}/targets"),
        json!({"target_id": TARGET, "label": "Self", "expected_revision": null}),
    );
    let candidate = call(
        &server,
        "POST",
        &format!("{base}/inspect"),
        json!({"destination": {"kind": "local", "checkout_id": owner}}),
    );
    call(
        &server,
        "PUT",
        &format!("{base}/targets/{TARGET}/binding"),
        json!({"target": {"owner_checkout_id": owner, "target_id": TARGET},
            "expected_manifest_revision": added["revision"], "expected_binding_revision": 0,
            "previous_destination": null, "destination": candidate["destination"]}),
    );
    let goal = ops::create(
        &FsStore::open(&world.store).unwrap(),
        hotsheet_model::Ulid::new(),
        "ORC",
        "2026-11-18T00:00:00Z".into(),
        NewTicket {
            title: "Ship the launch".into(),
            ..Default::default()
        },
    )
    .unwrap();

    let held = hold_vault(&world.home);
    let started = Instant::now();
    let propose = [
        "orchestration",
        "plan",
        "propose",
        "--project",
        owner.as_str(),
        "--tool",
        TOOL,
        "--operation-id",
        "press-1",
    ];
    let output = world.cli(&propose);
    held.join().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        server.log()
    );
    let proposed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(proposed["outcome"], "started", "{proposed}");
    assert_eq!(proposed["proposal_id"], "press-1");
    assert_eq!(
        proposed["goals"][0]["slug"],
        goal.slug.as_str(),
        "{proposed}"
    );
    assert_eq!(proposed["role_prompt"], "at_launch", "{proposed}");
    // The launch waited on the vault rather than being given up as no host.
    assert!(started.elapsed() >= VAULT_HELD, "{:?}", started.elapsed());
    let terminal = proposed["session"]["session"]["session_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // The running server lists the planner, labelled, and it is the launch recorded.
    let terminals = call(&server, "GET", "/terminals", Value::Null);
    let listed = terminals
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == terminal.as_str())
        .unwrap_or_else(|| panic!("{terminals}"));
    assert_eq!(listed["alive"], true, "{listed}");
    assert!(
        listed["name"]
            .as_str()
            .is_some_and(|name| name.ends_with(": planner")),
        "{listed}"
    );
    // The fake agent reports its launch in the orchestration project.
    let report = world.reports.join(format!("{terminal}.json"));
    let deadline = Instant::now() + Duration::from_secs(120);
    while !report.is_file() {
        assert!(
            Instant::now() < deadline,
            "the planner never reported its launch"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // Both clients now read the same planner, and pressing again launches nothing.
    let shown = world.cli(&["orchestration", "plan", "show", "--project", owner.as_str()]);
    let shown: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(shown["state"], "planning", "{shown}");
    assert_eq!(shown["proposal_id"], "press-1", "{shown}");
    let again = world.cli(&[
        "orchestration",
        "plan",
        "propose",
        "--project",
        owner.as_str(),
        "--tool",
        TOOL,
    ]);
    let again: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(again["outcome"], "existing", "{again}");
    let terminals = call(&server, "GET", "/terminals", Value::Null);
    assert_eq!(terminals.as_array().unwrap().len(), 1, "{terminals}");
}
