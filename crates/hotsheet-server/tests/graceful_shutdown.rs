//! Real `hotsheet-server` binary: a stop is bounded even with long-lived connections open
//! (HS2-W1KJR4). A held permission hook is cancelled with 503, a stuck request cannot keep
//! the process past its drain deadline, a second stop signal forces exit immediately, and
//! every path removes the discovery instance file and index-writer lock.

use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use hotsheet_server::lifecycle::InstanceInfo;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::json;

struct Server {
    child: Child,
    log: PathBuf,
    instance: PathBuf,
    lock: PathBuf,
    _fixture: tempfile::TempDir,
}

impl Server {
    fn start(drain_ms: u64) -> Self {
        Self::start_with(drain_ms, true, Stdio::piped(), None)
    }

    /// `watch_stdin` opts into `--exit-on-stdin-eof`; `stdin` is what the server reads.
    /// `output` replaces the log file for stdout + stderr (to model a dead owner's pipes).
    fn start_with(
        drain_ms: u64,
        watch_stdin: bool,
        stdin: Stdio,
        output: Option<(Stdio, Stdio)>,
    ) -> Self {
        Self::start_inner(drain_ms, watch_stdin, stdin, output, false)
    }

    /// Start with a drivable AI tool on `PATH` whose every invocation hangs, so the
    /// startup AI catalog warmup is still running when the stop arrives (HS2-NPBZJ9).
    fn start_with_slow_discovery(drain_ms: u64) -> Self {
        Self::start_inner(drain_ms, true, Stdio::piped(), None, true)
    }

    fn start_inner(
        drain_ms: u64,
        watch_stdin: bool,
        stdin: Stdio,
        output: Option<(Stdio, Stdio)>,
        slow_discovery: bool,
    ) -> Self {
        let fixture = tempfile::tempdir().unwrap();
        // Keep locally installed AI tools out of startup catalog discovery; a slow-discovery
        // fixture adds only its own hanging `opencode` (a drivable plugin's detection binary).
        let path = if slow_discovery {
            let tools = fixture.path().join("tools");
            fs::create_dir(&tools).unwrap();
            let tool = tools.join("opencode");
            fs::write(
                &tool,
                "#!/bin/sh\necho started >> \"$0.calls\"\nexec sleep 30\n",
            )
            .unwrap();
            fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
            format!("{}:/usr/bin:/bin", tools.display())
        } else {
            "/usr/bin:/bin".to_string()
        };
        let home = fixture.path().join("home");
        fs::create_dir(&home).unwrap();
        let store = fixture.path().join("store");
        FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
        let log = fixture.path().join("server.log");
        let (stdout, stderr) = output.unwrap_or_else(|| {
            let file = fs::File::create(&log).unwrap();
            (file.try_clone().unwrap().into(), file.into())
        });
        let child = Command::new(env!("CARGO_BIN_EXE_hotsheet-server"))
            .arg("-C")
            .arg(&store)
            .args(["--bind", "127.0.0.1:0", "--secret", "drain-test"])
            .args(["--no-terminal-broker", "--shutdown-drain-ms"])
            .arg(drain_ms.to_string())
            .env("HOTSHEET_HOME", &home)
            .env("PATH", path)
            // Piped and held by the fixture: the server stops if this test process dies
            // without running its teardown (HS2-VQ8ZWT).
            .args(watch_stdin.then_some("--exit-on-stdin-eof"))
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .process_group(0)
            .spawn()
            .unwrap();
        let project = hotsheet_tls::project_id(&store);
        Self {
            child,
            log,
            instance: home.join("instances").join(format!("{project}.json")),
            lock: home.join("instances").join(format!("{project}.lock")),
            _fixture: fixture,
        }
    }

    /// The hanging tool's invocation record, written when discovery starts probing it.
    fn slow_tool_calls(&self) -> PathBuf {
        self._fixture.path().join("tools").join("opencode.calls")
    }

    fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn ready(&mut self) -> InstanceInfo {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "server exited: {}",
                self.log()
            );
            if let Ok(text) = fs::read_to_string(&self.instance)
                && let Ok(info) = serde_json::from_str::<InstanceInfo>(&text)
                && info.pid == self.child.id()
            {
                return info;
            }
            assert!(
                Instant::now() < deadline,
                "server did not start: {}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn sigterm(&self) {
        // SAFETY: the positive pid belongs to this fixture's child.
        assert_eq!(
            unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) },
            0
        );
    }

    /// Wait for exit; panic (with the log) if the process outlives `within`.
    fn exits_within(&mut self, within: Duration) -> ExitStatus {
        let deadline = Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "server still running {within:?} after stop: {}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_registration_released(&self) {
        assert!(
            !self.instance.exists(),
            "instance file left behind: {}",
            self.log()
        );
        assert!(
            !self.lock.exists(),
            "writer lock left behind: {}",
            self.log()
        );
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: the negative id is this fixture's own process group.
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

/// Open a request whose body never finishes arriving, so its handler stays in flight and
/// a graceful drain can never complete on its own.
fn stuck_request(info: &InstanceInfo) -> TcpStream {
    let address = info.url.trim_start_matches("http://");
    let mut stream = TcpStream::connect(address).unwrap();
    write!(
        stream,
        "POST /permissions/ask HTTP/1.1\r\nHost: {address}\r\nX-Hotsheet-Secret: {}\r\n\
         Content-Type: application/json\r\nContent-Length: 4096\r\n\r\n{{\"connection\":",
        info.secret
    )
    .unwrap();
    stream.flush().unwrap();
    stream
}

fn wait_for_pending_permission(info: &InstanceInfo) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let pending: serde_json::Value = ureq::get(&format!("{}/permissions", info.url))
            .set("x-hotsheet-secret", &info.secret)
            .call()
            .unwrap()
            .into_string()
            .map(|text| serde_json::from_str(&text).unwrap())
            .unwrap();
        if pending.as_array().is_some_and(|items| !items.is_empty()) {
            return;
        }
        assert!(Instant::now() < deadline, "permission ask never pended");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn stop_cancels_a_held_permission_hook_with_503_and_exits_cleanly() {
    // A long drain proves the hook is cancelled promptly, not waited out.
    let mut server = Server::start(60_000);
    let info = server.ready();
    let hook = {
        let info = info.clone();
        std::thread::spawn(move || {
            ureq::post(&format!("{}/permissions/ask", info.url))
                .set("x-hotsheet-secret", &info.secret)
                .timeout(Duration::from_secs(60))
                .set("content-type", "application/json")
                .send_string(
                    &json!({"connection": "c1", "tool": "Bash", "action": "ls"}).to_string(),
                )
                .map(|response| response.status())
                .map_err(|error| match error {
                    ureq::Error::Status(code, _) => code.to_string(),
                    other => other.to_string(),
                })
        })
    };
    wait_for_pending_permission(&info);

    let stopped = Instant::now();
    server.sigterm();
    match hook.join().unwrap() {
        Err(code) if code == "503" => {}
        Err(other) => panic!("hook got {other}, want HTTP 503"),
        Ok(status) => panic!("hook got HTTP {status}, want 503"),
    }
    let status = server.exits_within(Duration::from_secs(5));
    assert!(status.success(), "unclean exit {status}: {}", server.log());
    assert!(
        stopped.elapsed() < Duration::from_secs(5),
        "graceful stop took {:?}",
        stopped.elapsed()
    );
    assert!(!server.log().contains("forcing exit"), "{}", server.log());
    server.assert_registration_released();
}

#[test]
fn stop_cancels_an_idle_long_poll() {
    let mut server = Server::start(60_000);
    let info = server.ready();
    let cursor = ureq::get(&format!("{}/ws/poll", info.url))
        .set("x-hotsheet-secret", &info.secret)
        .call()
        .unwrap()
        .into_string()
        .map(|text| serde_json::from_str::<serde_json::Value>(&text).unwrap())
        .unwrap()["cursor"]
        .as_u64()
        .unwrap();
    let poll = {
        let info = info.clone();
        std::thread::spawn(move || {
            ureq::get(&format!(
                "{}/ws/poll?since={cursor}&timeout_ms=55000",
                info.url
            ))
            .set("x-hotsheet-secret", &info.secret)
            .timeout(Duration::from_secs(60))
            .call()
            .map_err(|error| error.to_string())
            .and_then(|response| response.into_string().map_err(|error| error.to_string()))
        })
    };
    // Let the poll reach its wait before stopping.
    std::thread::sleep(Duration::from_millis(300));
    let stopped = Instant::now();
    server.sigterm();
    let reply: serde_json::Value = serde_json::from_str(&poll.join().unwrap().unwrap()).unwrap();
    assert_eq!(reply["events"], json!([]));
    assert!(
        stopped.elapsed() < Duration::from_secs(5),
        "long poll held the stop for {:?}",
        stopped.elapsed()
    );
    assert!(server.exits_within(Duration::from_secs(5)).success());
    server.assert_registration_released();
}

#[test]
fn drain_deadline_forces_exit_past_a_stuck_request() {
    let mut server = Server::start(400);
    let info = server.ready();
    let _stuck = stuck_request(&info);
    std::thread::sleep(Duration::from_millis(200));
    server.sigterm();
    let status = server.exits_within(Duration::from_secs(5));
    assert!(status.success(), "unclean exit {status}: {}", server.log());
    assert!(
        server.log().contains("drain deadline passed"),
        "{}",
        server.log()
    );
    server.assert_registration_released();
}

#[test]
fn second_stop_signal_forces_exit_during_a_long_drain() {
    let mut server = Server::start(60_000);
    let info = server.ready();
    let _stuck = stuck_request(&info);
    std::thread::sleep(Duration::from_millis(200));
    server.sigterm();
    // Still draining: the first signal alone waits for the stuck request.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        server.child.try_wait().unwrap().is_none(),
        "exited before the drain deadline without a second signal: {}",
        server.log()
    );
    server.sigterm();
    let status = server.exits_within(Duration::from_secs(3));
    assert!(status.success(), "unclean exit {status}: {}", server.log());
    assert!(
        server.log().contains("second stop signal"),
        "{}",
        server.log()
    );
    server.assert_registration_released();
}

#[test]
fn stopping_server_stops_accepting_connections() {
    let mut server = Server::start(60_000);
    let info = server.ready();
    let _stuck = stuck_request(&info);
    std::thread::sleep(Duration::from_millis(200));
    server.sigterm();
    std::thread::sleep(Duration::from_millis(300));
    let address = info.url.trim_start_matches("http://");
    assert!(
        TcpStream::connect_timeout(&address.parse().unwrap(), Duration::from_secs(1)).is_err(),
        "a draining server must not accept new connections"
    );
}

#[test]
fn closing_stdin_stops_an_opted_in_server() {
    // HS2-VQ8ZWT: the owner's pipe closing is the signal; the bounded drain then runs.
    let mut server = Server::start(60_000);
    server.ready();
    drop(server.child.stdin.take());
    let status = server.exits_within(Duration::from_secs(5));
    assert!(status.success(), "unclean exit {status}: {}", server.log());
    assert!(server.log().contains("stdin closed"), "{}", server.log());
    server.assert_registration_released();
}

#[test]
fn a_sigkilled_owner_cannot_leave_its_server_running() {
    // The owner here is a stand-in process holding the only write end of the server's
    // stdin, like a test runner. SIGKILL skips every teardown path; the kernel still closes
    // the pipe, so the server stops on its own.
    let mut owner = Command::new("sleep")
        .arg("300")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let pipe = owner.stdout.take().unwrap();
    let mut server = Server::start_with(60_000, true, Stdio::from(pipe), None);
    server.ready();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        server.child.try_wait().unwrap().is_none(),
        "{}",
        server.log()
    );
    owner.kill().unwrap();
    owner.wait().unwrap();
    let status = server.exits_within(Duration::from_secs(5));
    assert!(status.success(), "unclean exit {status}: {}", server.log());
    server.assert_registration_released();
}

#[test]
fn stdin_eof_is_ignored_without_the_opt_in() {
    // Detached launches must outlive their client: EOF alone never stops them.
    let mut server = Server::start_with(60_000, false, Stdio::null(), None);
    server.ready();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        server.child.try_wait().unwrap().is_none(),
        "{}",
        server.log()
    );
    assert!(!server.log().contains("stdin closed"));
    server.sigterm();
    assert!(server.exits_within(Duration::from_secs(5)).success());
    server.assert_registration_released();
}

#[test]
fn owner_death_stops_the_server_even_when_its_output_pipes_died_too() {
    // Regression (HS2-VQ8ZWT): a real runner also owns the server's stdout/stderr pipes.
    // Once it dies, writing a shutdown diagnostic must not panic the watchdog before it
    // begins stopping (the first web E2E caught exactly that).
    let mut owner = Command::new("sleep")
        .arg("300")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let pipe = owner.stdout.take().unwrap();
    let mut sink = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let sink_in = sink.stdin.take().unwrap();
    let err_in = std::os::fd::AsFd::as_fd(&sink_in)
        .try_clone_to_owned()
        .unwrap();
    let output = (Stdio::from(sink_in), Stdio::from(err_in));
    let mut server = Server::start_with(60_000, true, Stdio::from(pipe), Some(output));
    server.ready();
    // The owner dies: its stdin write end and the output reader both go away.
    sink.kill().unwrap();
    sink.wait().unwrap();
    owner.kill().unwrap();
    owner.wait().unwrap();
    let status = server.exits_within(Duration::from_secs(5));
    assert!(status.success(), "unclean exit {status}");
    server.assert_registration_released();
}

#[test]
fn stop_is_prompt_while_startup_ai_catalog_discovery_hangs() {
    // HS2-NPBZJ9: the startup catalog warmup runs blocking tool probes (`<tool> --version`)
    // with no deadline of their own. A stop that lands while one hangs must not wait for it:
    // nothing is left to drain, so the process exits promptly and releases its registration
    // instead of sitting out the drain deadline (or, before HS2-W1KJR4, forever).
    let mut server = Server::start_with_slow_discovery(5_000);
    server.ready();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !server.slow_tool_calls().exists() {
        assert!(
            Instant::now() < deadline,
            "discovery never probed the slow tool: {}",
            server.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let stopped = Instant::now();
    server.sigterm();
    let status = server.exits_within(Duration::from_secs(4));
    assert!(
        status.success(),
        "unclean exit {status:?}: {}",
        server.log()
    );
    assert!(
        stopped.elapsed() < Duration::from_secs(2),
        "stop waited {:?} on the hanging discovery: {}",
        stopped.elapsed(),
        server.log()
    );
    server.assert_registration_released();
}
