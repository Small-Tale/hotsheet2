//! Real `hotsheet-server` binary: a stop is bounded even with long-lived connections open
//! (HS2-W1KJR4). A held permission hook is cancelled with 503, a stuck request cannot keep
//! the process past its drain deadline, a second stop signal forces exit immediately, and
//! every path removes the discovery instance file and index-writer lock.

use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use hotsheet_process_journal::{ExitReason, ProcessJournal, ProcessRecord, Purpose};
use hotsheet_server::lifecycle::InstanceInfo;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use notify::{RecursiveMode, Watcher};
use serde_json::json;

/// A drivable AI tool on the fixture `PATH` whose probe hangs during startup discovery.
#[derive(Clone, Copy)]
enum SlowTool {
    /// `opencode`: every invocation, including `--version`, hangs (HS2-NPBZJ9).
    Opencode,
    /// The child cannot emit its marker before the parent kills it (HS2-0WN2P5).
    OpencodeBeforeMarker,
    /// `codex`: `--version` answers, then the model-catalog `app-server` hangs (HS2-F6S04C).
    CodexAppServer,
}

impl SlowTool {
    fn name(self) -> &'static str {
        match self {
            Self::Opencode | Self::OpencodeBeforeMarker => "opencode",
            Self::CodexAppServer => "codex",
        }
    }

    /// Each hanging invocation appends its pid to `<tool>.calls` before it hangs.
    fn script(self) -> &'static str {
        match self {
            Self::Opencode => "#!/bin/sh\necho $$ >> \"$0.calls\"\nexec sleep 30\n",
            Self::OpencodeBeforeMarker => {
                "#!/bin/sh\nread token < \"$0.gate\"\necho $$ >> \"$0.calls\"\nexec sleep 30\n"
            }
            Self::CodexAppServer => {
                "#!/bin/sh\ncase \"$1\" in\n--version) echo codex-test 1.0 ;;\n\
                 app-server) echo $$ >> \"$0.calls\"; exec sleep 30 ;;\nesac\n"
            }
        }
    }
}

struct Server {
    child: Child,
    log: PathBuf,
    slow_tool: Option<SlowTool>,
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
        Self::start_inner(drain_ms, watch_stdin, stdin, output, None, None)
    }

    /// Start with a drivable AI tool on `PATH` whose every invocation hangs, so the
    /// startup AI catalog warmup is still running when the stop arrives (HS2-NPBZJ9).
    /// `probe_timeout_ms` sets the server's per-probe deadline (HS2-BH3M53).
    fn start_with_slow_discovery(drain_ms: u64, probe_timeout_ms: u64) -> Self {
        Self::start_with_slow_tool(drain_ms, probe_timeout_ms, SlowTool::Opencode)
    }

    fn start_with_slow_tool(drain_ms: u64, probe_timeout_ms: u64, tool: SlowTool) -> Self {
        Self::start_inner(
            drain_ms,
            true,
            Stdio::piped(),
            None,
            Some((probe_timeout_ms, tool)),
            None,
        )
    }

    fn start_inner(
        drain_ms: u64,
        watch_stdin: bool,
        stdin: Stdio,
        output: Option<(Stdio, Stdio)>,
        slow_discovery: Option<(u64, SlowTool)>,
        test_owner_pid: Option<u32>,
    ) -> Self {
        let fixture = tempfile::tempdir().unwrap();
        // Keep locally installed AI tools out of startup catalog discovery; a slow-discovery
        // fixture adds only its own hanging tool (a drivable plugin's detection binary).
        let path = if let Some((_, slow_tool)) = slow_discovery {
            let tools = fixture.path().join("tools");
            fs::create_dir(&tools).unwrap();
            let tool = tools.join(slow_tool.name());
            fs::write(&tool, slow_tool.script()).unwrap();
            fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
            if matches!(slow_tool, SlowTool::OpencodeBeforeMarker) {
                use std::os::unix::ffi::OsStrExt;
                let gate =
                    std::ffi::CString::new(tool.with_extension("gate").as_os_str().as_bytes())
                        .unwrap();
                // SAFETY: gate is a valid NUL-terminated fixture path. No writer is
                // opened, so the shell blocks before its calls marker, without a timer.
                assert_eq!(unsafe { libc::mkfifo(gate.as_ptr(), 0o600) }, 0);
            }
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
        let mut command = Command::new(env!("CARGO_BIN_EXE_hotsheet-server"));
        if slow_discovery.is_some() {
            // HS2-7W3D7V: the test environment disables tool probes; this fixture probes
            // only its own hanging tool, so it opts back in.
            command.env_remove(hotsheet_aitools::AI_TOOL_PROBES_ENV);
        }
        let child = command
            .arg("-C")
            .arg(&store)
            .args(["--bind", "127.0.0.1:0", "--secret", "drain-test"])
            .args(["--no-terminal-broker", "--shutdown-drain-ms"])
            .arg(drain_ms.to_string())
            .env("HOTSHEET_HOME", &home)
            .envs(test_owner_pid.map(|pid| ("HOTSHEET_TEST_SERVER_OWNER_PID", pid.to_string())))
            .env("PATH", path)
            .args(slow_discovery.map(|(ms, _)| format!("--ai-probe-timeout-ms={ms}")))
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
            slow_tool: slow_discovery.map(|(_, tool)| tool),
            instance: home.join("instances").join(format!("{project}.json")),
            lock: home.join("instances").join(format!("{project}.lock")),
            _fixture: fixture,
        }
    }

    /// The hanging tool's invocation record, written when discovery starts probing it.
    fn slow_tool_calls(&self) -> PathBuf {
        let tool = self.slow_tool.expect("a slow-tool fixture").name();
        self._fixture
            .path()
            .join("tools")
            .join(format!("{tool}.calls"))
    }

    /// Wait until discovery has probed the hanging tool; return every probe's pid so far.
    fn slow_tool_pids(&self) -> Vec<i32> {
        // Catalog warmup is scheduled after the server registers itself. Under a busy
        // parallel suite, that background task can start well after `ready()` returns;
        // this wait is setup, not the probe/queue latency assertion below.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let pids: Vec<i32> = fs::read_to_string(self.slow_tool_calls())
                .unwrap_or_default()
                .lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect();
            if !pids.is_empty() {
                return pids;
            }
            assert!(
                Instant::now() < deadline,
                "discovery never probed the slow tool: {}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// HS2-0WN2P5: spawn/kill is a parent fact; a short deadline may expire before
    /// the shell ever writes its first marker. Subscribe before reading so a completed
    /// startup probe or a concurrent journal update is observed without a fixed delay.
    fn killed_probe(&self) -> ProcessRecord {
        let home = self._fixture.path().join("home");
        let journal = ProcessJournal::at(&home);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .unwrap();
        watcher.watch(&home, RecursiveMode::Recursive).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(record) = journal.list().unwrap().into_iter().find(|record| {
                record.supervisor_pid == self.child.id()
                    && record.purpose == Purpose::AiProbe
                    && record.exit_reason == Some(ExitReason::Killed)
            }) {
                return record;
            }
            loop {
                let event: notify::Event = rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .unwrap_or_else(|error| panic!("probe was not killed: {error}: {}", self.log()))
                    .unwrap();
                if event.need_rescan()
                    || matches!(
                        event.kind,
                        notify::EventKind::Create(_)
                            | notify::EventKind::Modify(_)
                            | notify::EventKind::Remove(_)
                    )
                {
                    break;
                }
            }
        }
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

#[test]
fn hs2_bnghkm_interrupted_test_owner_reaps_repeated_detached_servers() {
    for _ in 0..3 {
        let mut owner = Command::new("sleep").arg("30").spawn().unwrap();
        let mut server =
            Server::start_inner(250, false, Stdio::null(), None, None, Some(owner.id()));
        let _ = server.ready();
        owner.kill().unwrap();
        owner.wait().unwrap();
        assert!(server.exits_within(Duration::from_secs(5)).success());
        server.assert_registration_released();
    }
}

#[test]
fn hs2_bnghkm_interrupted_test_owner_reaps_detached_broker() {
    let fixture = tempfile::tempdir_in("/tmp").unwrap();
    fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let socket = fixture.path().join("broker.sock");
    let log = fixture.path().join("broker.log");
    let mut owner = Command::new("sleep").arg("30").spawn().unwrap();
    let mut broker = Command::new(env!("CARGO_BIN_EXE_hotsheet-server"))
        .arg("--terminal-broker-process")
        .arg(&socket)
        .args(["--terminal-broker-project", "test-project"])
        .env("HOTSHEET_TEST_SERVER_OWNER_PID", owner.id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !socket.exists() && Instant::now() < deadline {
        assert!(
            broker.try_wait().unwrap().is_none(),
            "broker exited before binding: {}",
            fs::read_to_string(&log).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        socket.exists(),
        "broker did not bind: {}",
        fs::read_to_string(&log).unwrap_or_default()
    );
    owner.kill().unwrap();
    owner.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = broker.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "test broker outlived its owner");
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(!socket.exists(), "test broker left its socket behind");
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
    // A long probe deadline keeps the probe in flight when the stop arrives.
    let mut server = Server::start_with_slow_discovery(5_000, 60_000);
    server.ready();
    let probes = server.slow_tool_pids();
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
    // HS2-BH3M53: the stop also killed the in-flight probe; nothing it started survives.
    assert_processes_gone(&probes, &server);
}

#[test]
fn hs2_f6s04c_stop_kills_a_startup_catalog_app_server_probe() {
    // The startup catalog warmup lists codex models through a long-running `app-server`
    // probe child. A stop that lands while it hangs must exit promptly and kill it: the
    // child is registered with the probe set atomically with its spawn, so shutdown can
    // never snapshot the set before it and leave the app-server orphaned.
    let mut server = Server::start_with_slow_tool(5_000, 60_000, SlowTool::CodexAppServer);
    server.ready();
    let app_servers = server.slow_tool_pids();
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
        "stop waited {:?} on the hanging app-server: {}",
        stopped.elapsed(),
        server.log()
    );
    server.assert_registration_released();
    assert_processes_gone(&app_servers, &server);
}

/// Wait (bounded) until every pid is gone; a probe left running is an orphan.
fn assert_processes_gone(pids: &[i32], server: &Server) {
    let deadline = Instant::now() + Duration::from_secs(5);
    for pid in pids {
        // SAFETY: signal 0 only checks whether the pid still exists.
        while unsafe { libc::kill(*pid, 0) } == 0 {
            assert!(
                Instant::now() < deadline,
                "tool probe {pid} outlived the server: {}",
                server.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn timed_request(
    info: &InstanceInfo,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> (u16, Duration) {
    let started = Instant::now();
    let request = ureq::request(method, &format!("{}{path}", info.url))
        .set("x-hotsheet-secret", &info.secret)
        .timeout(Duration::from_secs(30));
    let response = match body {
        Some(body) => request
            .set("content-type", "application/json")
            .send_string(&body.to_string()),
        None => request.call(),
    };
    let status = match response {
        Ok(response) => response.status(),
        Err(ureq::Error::Status(status, _)) => status,
        Err(error) => panic!("{method} {path} failed: {error}"),
    };
    (status, started.elapsed())
}

#[test]
fn a_hanging_tool_probe_is_killed_at_its_deadline_so_discovery_never_queues_behind_it() {
    assert_hanging_discovery_is_bounded(SlowTool::Opencode);
}

#[test]
fn hs2_0wn2p5_deadline_observation_does_not_require_the_child_entry_marker() {
    assert_hanging_discovery_is_bounded(SlowTool::OpencodeBeforeMarker);
}

fn assert_hanging_discovery_is_bounded(tool: SlowTool) {
    // HS2-BH3M53: a hung `opencode --version` used to hold the shared catalog lock forever,
    // so every later discovery (`/ai-tools`, `/ai-settings`, terminal-launch validation)
    // queued behind it. Each probe now has a deadline: the hung child is killed, the tool
    // falls back to its manifest catalog, and later requests answer within that bound.
    let mut server = Server::start_with_slow_tool(5_000, 500, tool);
    let info = server.ready();
    let warmup = server.killed_probe();
    assert!(warmup.ended_ms.unwrap() >= warmup.started_ms + 500);
    // The startup warmup's probe is killed at its deadline, not left running.
    assert_processes_gone(&[warmup.pid as i32], &server);
    let bound = Duration::from_secs(4);
    for path in ["/ai-tools?refresh=true", "/ai-tools", "/ai-settings"] {
        let (status, elapsed) = timed_request(&info, "GET", path, None);
        assert_eq!(status, 200, "GET {path}: {}", server.log());
        assert!(
            elapsed < bound,
            "GET {path} queued {elapsed:?} behind the hung probe"
        );
    }
    let tools: serde_json::Value = ureq::get(&format!("{}/ai-tools", info.url))
        .set("x-hotsheet-secret", &info.secret)
        .call()
        .unwrap()
        .into_string()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap();
    assert!(
        tools
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["id"] == "opencode"),
        "the hung tool lost its manifest fallback: {tools}"
    );
    // Terminal-launch validation shares the same discovery; an unsupported effort is
    // rejected promptly instead of waiting on the hung probe.
    let (status, elapsed) = timed_request(
        &info,
        "POST",
        "/terminals",
        Some(
            json!({ "connect": "opencode", "model": "no-such-model", "effort": "no-such-effort" }),
        ),
    );
    assert_eq!(status, 400, "terminal launch validation: {}", server.log());
    assert!(elapsed < bound, "terminal launch queued {elapsed:?}");
    server.sigterm();
    assert!(server.exits_within(Duration::from_secs(5)).success());
    server.assert_registration_released();
    let probes = ProcessJournal::at(server._fixture.path().join("home"))
        .list()
        .unwrap();
    let probes: Vec<_> = probes
        .into_iter()
        .filter(|record| record.purpose == Purpose::AiProbe)
        .collect();
    assert!(!probes.is_empty());
    for record in &probes {
        assert_eq!(record.exit_reason, Some(ExitReason::Killed));
    }
    assert_processes_gone(
        &probes
            .iter()
            .map(|record| record.pid as i32)
            .collect::<Vec<_>>(),
        &server,
    );
    if matches!(tool, SlowTool::OpencodeBeforeMarker) {
        assert!(
            !server.slow_tool_calls().exists(),
            "the gated child must never have emitted its entry marker"
        );
    }
}

/// The web client samples a remote server's clock from the `Date` header of the
/// `/connections` snapshot (HS2-FMZSNY), so the real binary's responses must carry one.
#[test]
fn real_server_responses_carry_a_date_header_for_client_clock_sampling() {
    let mut server = Server::start(1_000);
    let info = server.ready();
    let response = ureq::get(&format!("{}/connections", info.url))
        .set("x-hotsheet-secret", &info.secret)
        .call()
        .unwrap();
    let date = response.header("date").expect("a Date header");
    assert!(date.ends_with(" GMT"), "an HTTP date: {date}");
    server.sigterm();
    assert!(server.exits_within(Duration::from_secs(5)).success());
}
