//! HS2-HYF56Y with the HS2-CY01TZ null-bootstrap harness: a real `hotsheet-server` whose Mach
//! bootstrap port is dead launches its terminal broker through the desktop-session launch
//! relay, so the broker reports a live namespace and opens a PTY. The broker survives the
//! server's SIGKILL, a restarted server adopts it, and explicit kill-all still ends its
//! terminals. Without a relay the server falls back to its direct spawn and says why.
//!
//! The relay here is a child of the test standing in for the one launchd runs; nothing
//! touches launchd or `~/Library/LaunchAgents`.
#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hotsheet_process_journal::{ProcessJournal, Purpose};
use hotsheet_server::lifecycle::InstanceInfo;
use hotsheet_terminals::bootstrap::{BootstrapHealth, null_bootstrap_for_child};
use hotsheet_terminals::{BrokerClient, BrokerRequest, BrokerResponse, TerminalKind};
use hotsheet_ticketing::{FsStore, StoreMetadata};

const SERVER: &str = env!("CARGO_BIN_EXE_hotsheet-server");

/// Kills its process group on drop, so a failed assertion leaves nothing running.
struct Held {
    child: Child,
    log: PathBuf,
}

impl Held {
    fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn kill(&mut self) {
        // SAFETY: the negative id is this fixture's own process group.
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    home: PathBuf,
    store: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        let store = dir.path().join("store");
        FsStore::init_git(&store, &StoreMetadata::new("HS")).unwrap();
        Self { dir, home, store }
    }

    fn project(&self) -> String {
        hotsheet_tls::project_id(&self.store)
    }

    fn broker_socket(&self) -> PathBuf {
        hotsheet_terminals::broker_socket::select_socket(&self.home, &self.project()).unwrap()
    }

    fn relay_socket(&self) -> PathBuf {
        hotsheet_terminals::broker_socket::relay_socket_path(&self.home).unwrap()
    }

    fn base(&self, command: &mut Command, log: &Path) {
        let output = fs::File::create(log).unwrap();
        command
            .env("HOTSHEET_HOME", &self.home)
            // Every process this test starts, brokers included, ends with the test process.
            .env(
                "HOTSHEET_TEST_SERVER_OWNER_PID",
                std::process::id().to_string(),
            )
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .process_group(0);
    }

    /// The stand-in for the launchd-started relay, in this (healthy) namespace.
    fn start_relay(&self) -> Held {
        let log = self.dir.path().join("relay.log");
        let mut command = Command::new(SERVER);
        command
            .arg("--terminal-launch-relay")
            .arg(self.relay_socket());
        self.base(&mut command, &log);
        let held = Held {
            child: command.spawn().unwrap(),
            log,
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match hotsheet_terminals::launch_relay::relay_hello(&self.relay_socket()) {
                Ok(hello) => {
                    assert_eq!(hello.pid, held.child.id());
                    assert!(!hello.bootstrap.is_dead(), "{hello:?}");
                    return held;
                }
                Err(error) => assert!(
                    Instant::now() < deadline,
                    "relay did not answer: {error}; {}",
                    held.log()
                ),
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// A real server whose bootstrap port is null: the 2026-10-29 dead-namespace state.
    fn start_dead_namespace_server(&self, name: &str) -> (Held, InstanceInfo) {
        let log = self.dir.path().join(format!("{name}.log"));
        let mut command = Command::new(SERVER);
        command.arg("-C").arg(&self.store).args([
            "--bind",
            "127.0.0.1:0",
            "--secret",
            "relay-test",
        ]);
        self.base(&mut command, &log);
        null_bootstrap_for_child(&mut command);
        let mut held = Held {
            child: command.spawn().unwrap(),
            log,
        };
        let instance = self
            .home
            .join("instances")
            .join(format!("{}.json", self.project()));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            assert!(
                held.child.try_wait().unwrap().is_none(),
                "server exited: {}",
                held.log()
            );
            if let Ok(text) = fs::read_to_string(&instance)
                && let Ok(info) = serde_json::from_str::<InstanceInfo>(&text)
                && info.pid == held.child.id()
            {
                return (held, info);
            }
            if Instant::now() >= deadline {
                held.kill();
                panic!("server did not start: {}", held.log());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn relayed_broker_launches(&self) -> Vec<u32> {
        ProcessJournal::at(&self.home)
            .list()
            .unwrap()
            .into_iter()
            .filter(|record| {
                record.supervisor == "hotsheet-terminal-relay"
                    && record.purpose == Purpose::BrokerSupervisor
            })
            .map(|record| record.pid)
            .collect()
    }
}

async fn health(client: &mut BrokerClient) -> BootstrapHealth {
    match client.request(&BrokerRequest::Health).await.unwrap() {
        BrokerResponse::Health { bootstrap } => bootstrap,
        other => panic!("unexpected health reply: {other:?}"),
    }
}

async fn terminal_ids(client: &mut BrokerClient) -> Vec<String> {
    match client.request(&BrokerRequest::List).await.unwrap() {
        BrokerResponse::List { terminals } => terminals.into_iter().map(|t| t.id).collect(),
        other => panic!("unexpected list reply: {other:?}"),
    }
}

fn parent_pid(pid: u32) -> u32 {
    let output = Command::new("/bin/ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_hyf56y_dead_namespace_server_gets_a_live_broker_through_the_relay() {
    let fixture = Fixture::new();
    let relay = fixture.start_relay();
    let (mut server, info) = fixture.start_dead_namespace_server("server-1");

    // The broker was launched by the relay, not by the dead-namespace server.
    let launches = fixture.relayed_broker_launches();
    assert_eq!(launches.len(), 1, "{launches:?}; {}", server.log());
    let broker_pid = launches[0];
    assert_eq!(parent_pid(broker_pid), relay.child.id());
    assert!(
        server
            .log()
            .contains("terminal broker launched by the desktop-session launch relay"),
        "{}",
        server.log()
    );

    let socket = fixture.broker_socket();
    let mut client = BrokerClient::connect(&socket).await.unwrap();
    assert_eq!(
        health(&mut client).await,
        BootstrapHealth::Live,
        "server: {}\nrelay: {}\njournal: {:?}",
        server.log(),
        relay.log(),
        ProcessJournal::at(&fixture.home).list().unwrap()
    );
    let open = BrokerRequest::Open {
        id: "t1".into(),
        kind: TerminalKind::Shell,
        command: "cat".into(),
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        replay_bytes: None,
        push: None,
    };
    match client.request(&open).await.unwrap() {
        BrokerResponse::Terminal { info } => assert!(info.alive, "{info:?}"),
        other => panic!("a relayed broker did not open a PTY: {other:?}"),
    }

    // The server dies without any cleanup; the broker and its PTY survive it.
    server.kill();
    let mut client = BrokerClient::connect(&socket).await.unwrap();
    assert_eq!(terminal_ids(&mut client).await, ["t1"]);
    drop(info);

    // A restarted server, still in a dead namespace, adopts the live broker.
    let (server, info) = fixture.start_dead_namespace_server("server-2");
    let listed = ureq::get(&format!("{}/terminals", info.url))
        .set("X-Hotsheet-Secret", &info.secret)
        .call()
        .unwrap()
        .into_string()
        .unwrap();
    assert!(listed.contains("\"t1\""), "{listed}");
    assert_eq!(fixture.relayed_broker_launches(), [broker_pid]);
    assert!(
        !server.log().contains("did not come up"),
        "{}",
        server.log()
    );

    // Explicit kill-all recovery is unchanged.
    let stop = Command::new(SERVER)
        .arg("-C")
        .arg(&fixture.store)
        .args(["--stop", "--kill-all-terminals"])
        .env("HOTSHEET_HOME", &fixture.home)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&stop.stdout);
    assert!(stop.status.success(), "{stop:?}");
    assert!(stdout.contains("killed 1 retained terminal(s)"), "{stdout}");
    let mut client = BrokerClient::connect(&socket).await.unwrap();
    assert!(terminal_ids(&mut client).await.is_empty());
    drop(server);
    drop(relay);
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_hyf56y_without_a_relay_the_server_spawns_directly_and_says_why() {
    let fixture = Fixture::new();
    let (server, _info) = fixture.start_dead_namespace_server("server");
    assert!(fixture.relayed_broker_launches().is_empty());
    let log = server.log();
    assert!(log.contains("no terminal launch relay is running"), "{log}");
    assert!(log.contains("hotsheet-cli terminal-relay install"), "{log}");
    // The direct spawn inherited the dead namespace, which HS2-CY01TZ keeps reporting.
    let mut client = BrokerClient::connect(&fixture.broker_socket())
        .await
        .unwrap();
    assert!(health(&mut client).await.is_dead());
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_hyf56y_a_stopped_relay_leaves_its_brokers_running() {
    let fixture = Fixture::new();
    let mut relay = fixture.start_relay();
    let (_server, _info) = fixture.start_dead_namespace_server("server");
    let socket = fixture.broker_socket();
    // The relay job going away (bootout, KeepAlive restart, upgrade) must not end the broker:
    // it runs in its own session, outside the relay's process group.
    relay.kill();
    let mut client = BrokerClient::connect(&socket).await.unwrap();
    assert!(matches!(
        client.request(&BrokerRequest::Ping).await.unwrap(),
        BrokerResponse::Pong
    ));
    assert_eq!(health(&mut client).await, BootstrapHealth::Live);
}
