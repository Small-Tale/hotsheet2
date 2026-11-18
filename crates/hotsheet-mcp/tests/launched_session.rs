//! HS2-JFE7JJ: the `hotsheet-mcp --project` shim of a Hot Sheet-launched session attaches
//! to the server that launched it. It starts no machine host and leaves the launching
//! server's instance record as it was. Real processes: a `hotsheet-server` anchored on a
//! synthetic store, and the shipped shim with the environment that server gives its
//! terminals. Synthetic names only.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};

const SECRET: &str = "launched-session-secret";

/// A server anchored on `store`, as the trial and a developer's `serve` run it.
struct Server {
    child: Child,
}

impl Server {
    fn start(store: &Path, home: &Path) -> Self {
        let child = Command::new(workspace_bin::path("hotsheet-server"))
            .arg("-C")
            .arg(store)
            .args([
                "--bind",
                "127.0.0.1:0",
                "--secret",
                SECRET,
                "--no-terminal-broker",
            ])
            .env("HOTSHEET_HOME", home)
            .env("HOTSHEET_HOST_HEALTH_SAMPLER", "off")
            .env("HOTSHEET_AI_TOOL_PROBES", "off")
            .env_remove("HOTSHEET_SERVER")
            .env_remove("HOTSHEET_SECRET")
            .env_remove("HOTSHEET_STORE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self { child }
    }

    /// The server's own instance record, once it is published.
    fn record(&mut self, path: &Path) -> Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            assert!(self.child.try_wait().unwrap().is_none(), "server exited");
            if let Ok(text) = std::fs::read_to_string(path)
                && let Ok(record) = serde_json::from_str::<Value>(&text)
                && record["pid"] == self.child.id()
            {
                return record;
            }
            assert!(
                Instant::now() < deadline,
                "server did not publish its record"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Run the shim as a launched terminal's AI tool would, send `requests`, and return its
/// replies and whether it exited successfully.
fn shim(
    home: &Path,
    project: &Path,
    server: &str,
    requests: &[Value],
) -> (Vec<Value>, bool, String) {
    let mut child = Command::new(workspace_bin::path("hotsheet-mcp"))
        .arg("--project")
        .arg(project)
        .current_dir(project)
        .env("HOTSHEET_HOME", home)
        .env("HOTSHEET_SERVER", server)
        .env("HOTSHEET_SECRET", SECRET)
        .env("HOTSHEET_TERMINAL_ID", "01ARZ3NDEKTSV4RRFFQ69G5FAV")
        .env("HOTSHEET_AGENT", "claude")
        .env("HOTSHEET_ACTOR_ROLE", "ai")
        .env_remove("HOTSHEET_STORE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for request in requests {
        writeln!(stdin, "{request}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let replies = BufReader::new(output.stdout.as_slice())
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    (
        replies,
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Every instance record under `home`, by file name.
fn records(home: &Path) -> Vec<(String, Value)> {
    let mut found: Vec<(String, Value)> = std::fs::read_dir(home.join("instances"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .map(|path| {
                    (
                        path.file_name().unwrap().to_string_lossy().into_owned(),
                        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let store = root.path().join("orch-store");
    FsStore::init_git(&store, &StoreMetadata::new("ORC")).unwrap();
    let project = root.path().join("orch");
    std::fs::create_dir_all(&project).unwrap();
    hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![store.clone()])
        .unwrap();
    (root, home, store, project)
}

#[test]
fn hs2_jfe7jj_a_launched_sessions_shim_uses_its_server_and_starts_none() {
    let (_root, home, store, project) = fixture();
    let mut server = Server::start(&store, &home);
    let record_path = hotsheet_cli::external_launch::instance_path(&home, &store);
    let record = server.record(&record_path);
    let url = record["url"].as_str().unwrap().to_owned();
    let before = records(&home);

    let (replies, ok, stderr) = shim(
        &home,
        &project,
        &url,
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
                "params":{"name":"hotsheet_query","arguments":{}}}),
        ],
    );
    assert!(ok, "{stderr}");
    let query = replies.iter().find(|reply| reply["id"] == 2).unwrap();
    assert_ne!(query["result"]["isError"], true, "{query}");

    // No machine host was started for the session: no anchor store, no record of one,
    // and the launching server's record is exactly as it published it.
    let machine = hotsheet_cli::external_launch::machine_host_store(&home);
    assert!(
        !machine.exists(),
        "a machine host was set up at {}",
        machine.display()
    );
    assert_eq!(records(&home), before);
    assert_eq!(
        hotsheet_cli::external_launch::running_servers(&home)
            .into_iter()
            .map(|server| server.pid)
            .collect::<Vec<_>>(),
        [server.child.id()]
    );
}

/// A route inherited from another machine home (this suite itself may run in a Hot Sheet
/// terminal) is never used: the server is live and answers, but it is not recorded under
/// the shim's home.
#[test]
fn hs2_jfe7jj_a_route_from_another_home_is_never_used() {
    let (_root, home, store, project) = fixture();
    let other_home = home.with_file_name("other-home");
    std::fs::create_dir_all(&other_home).unwrap();
    let mut server = Server::start(&store, &other_home);
    let record = server.record(&hotsheet_cli::external_launch::instance_path(
        &other_home,
        &store,
    ));
    let (_, ok, stderr) = shim(
        &home,
        &project,
        record["url"].as_str().unwrap(),
        &[json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})],
    );
    assert!(!ok, "the shim used another home's server");
    assert!(
        stderr.contains("not running under this machine home"),
        "{stderr}"
    );
    assert!(!hotsheet_cli::external_launch::machine_host_store(&home).exists());
    assert!(records(&home).is_empty());
}

#[test]
fn hs2_jfe7jj_a_launched_shim_whose_server_is_gone_starts_none_and_says_so() {
    let (_root, home, _store, project) = fixture();
    // The launching server is gone and no machine host runs: the shim refuses to start a
    // server of its own rather than leave one behind.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let (_, ok, stderr) = shim(
        &home,
        &project,
        &url,
        &[json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})],
    );
    assert!(!ok);
    assert!(
        stderr.contains("start") && stderr.contains("no server of their own"),
        "{stderr}"
    );
    assert!(!hotsheet_cli::external_launch::machine_host_store(&home).exists());
    assert!(records(&home).is_empty());
}
