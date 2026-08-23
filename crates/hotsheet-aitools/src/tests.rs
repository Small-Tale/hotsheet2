//! Drive conformance (`docs/13` §13.7), minus the `hs-fake-agent` parts (HS2-64):
//! `run` with an injected spawner reports the exact command; the `TurnHandle` goes
//! busy → done; interrupt acts; absent caps are false. Plus a real-process check of the
//! `SystemSpawner` adapter.

use super::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

// ---- a fake spawner + process ----------------------------------------------------

struct FakeProc {
    alive: bool,
    code: i32,
    killed: Rc<Cell<bool>>,
}
impl SpawnedProcess for FakeProc {
    fn is_running(&mut self) -> bool {
        self.alive
    }
    fn wait(&mut self) -> i32 {
        self.alive = false;
        self.code
    }
    fn kill(&mut self) {
        self.killed.set(true);
        self.alive = false;
    }
}

struct FakeSpawner {
    last: RefCell<Option<SpawnSpec>>,
    code: i32,
    killed: Rc<Cell<bool>>,
}
impl FakeSpawner {
    fn new(code: i32) -> Self {
        Self {
            last: RefCell::new(None),
            code,
            killed: Rc::new(Cell::new(false)),
        }
    }
    fn last(&self) -> SpawnSpec {
        self.last.borrow().clone().expect("run() spawned something")
    }
}
impl ProcessSpawner for FakeSpawner {
    fn spawn(&self, spec: &SpawnSpec) -> std::io::Result<Box<dyn SpawnedProcess>> {
        *self.last.borrow_mut() = Some(spec.clone());
        Ok(Box::new(FakeProc {
            alive: true,
            code: self.code,
            killed: self.killed.clone(),
        }))
    }
}

fn ctx<'a>(spawner: &'a FakeSpawner, cwd: &str) -> DriveCtx<'a> {
    DriveCtx {
        cwd: PathBuf::from(cwd),
        spawner,
        env: Vec::new(),
        app_server: None,
        channel: None,
    }
}

// ---- a fake codex app-server daemon ----------------------------------------------

struct FakeTurn {
    running: bool,
    outcome: AppServerOutcome,
    interrupted: Rc<Cell<bool>>,
}
impl AppServerTurn for FakeTurn {
    fn is_running(&mut self) -> bool {
        self.running
    }
    fn wait(&mut self) -> AppServerOutcome {
        self.running = false;
        self.outcome.clone()
    }
    fn interrupt(&mut self) {
        self.interrupted.set(true);
        self.running = false;
    }
}

#[derive(Default)]
struct FakeAppServer {
    opened: RefCell<Option<(Option<String>, PathBuf)>>, // (resume, cwd)
    last_turn: RefCell<Option<(String, String)>>,       // (thread_id, content)
    interrupted: Rc<Cell<bool>>,
    fail: bool,
}
impl AppServerClient for FakeAppServer {
    fn open_thread(
        &self,
        resume: Option<&str>,
        cwd: &std::path::Path,
    ) -> Result<String, AppServerError> {
        if self.fail {
            return Err(AppServerError::Unavailable("daemon down".into()));
        }
        *self.opened.borrow_mut() = Some((resume.map(str::to_string), cwd.to_path_buf()));
        Ok(resume
            .map(str::to_string)
            .unwrap_or_else(|| "thread-1".to_string()))
    }
    fn start_turn(
        &self,
        thread_id: &str,
        content: &str,
    ) -> Result<Box<dyn AppServerTurn>, AppServerError> {
        *self.last_turn.borrow_mut() = Some((thread_id.to_string(), content.to_string()));
        Ok(Box::new(FakeTurn {
            running: true,
            outcome: AppServerOutcome::Completed,
            interrupted: self.interrupted.clone(),
        }))
    }
}

fn appctx<'a>(spawner: &'a FakeSpawner, app: &'a FakeAppServer, cwd: &str) -> DriveCtx<'a> {
    DriveCtx {
        cwd: PathBuf::from(cwd),
        spawner,
        env: Vec::new(),
        app_server: Some(app),
        channel: None,
    }
}

// ---- the checklist ---------------------------------------------------------------

#[test]
fn spawn_drive_reports_the_exact_command_and_completes() {
    let spawner = FakeSpawner::new(0);
    let drive = SpawnDrive::codex();
    assert_eq!(drive.info().transport, Transport::Spawn);
    assert!(drive.supports_interrupt());

    let mut turn = drive
        .run(
            &Target::default(),
            "work the top ticket",
            &ctx(&spawner, "/work"),
        )
        .unwrap();

    // reports the exact content it would send (no real tool)
    let spec = spawner.last();
    assert_eq!(spec.program, "codex");
    assert_eq!(spec.args, vec!["exec", "work the top ticket"]);
    assert_eq!(spec.cwd, PathBuf::from("/work"));
    assert_eq!(spec.stdin, None);

    // busy → done
    assert!(turn.is_busy());
    assert_eq!(turn.wait(), DoneReason::Completed);
    assert!(!turn.is_busy());
    assert_eq!(turn.wait(), DoneReason::Completed, "wait is idempotent");
}

#[test]
fn a_nonzero_exit_is_failed() {
    let spawner = FakeSpawner::new(3);
    let mut turn = SpawnDrive::codex()
        .run(&Target::default(), "x", &ctx(&spawner, "/w"))
        .unwrap();
    assert_eq!(turn.wait(), DoneReason::Failed(3));
}

#[test]
fn interrupt_kills_the_turn() {
    let spawner = FakeSpawner::new(0);
    let mut turn = SpawnDrive::codex()
        .run(&Target::default(), "x", &ctx(&spawner, "/w"))
        .unwrap();
    assert!(turn.is_busy());
    assert!(turn.interrupt(), "codex drive supports interrupt");
    assert!(spawner.killed.get(), "the process was killed");
    assert!(!turn.is_busy());
    assert_eq!(turn.wait(), DoneReason::Interrupted);
}

#[test]
fn content_via_stdin_and_absent_interrupt_cap() {
    let drive = SpawnDrive::new(SpawnConfig {
        program: "some-tool".into(),
        args: vec!["-p".into()],
        content: ContentMode::Stdin,
        interrupt: false,
        resume_flag: None,
    });
    // absence is the signal
    assert!(!drive.supports_interrupt());
    let spec = drive.spec("hello", std::path::Path::new("/w"), None);
    assert_eq!(spec.args, vec!["-p"], "content did NOT go into args");
    assert_eq!(spec.stdin.as_deref(), Some("hello"));

    // a turn from a non-interruptible drive won't act on interrupt
    let spawner = FakeSpawner::new(0);
    let mut turn = drive
        .run(&Target::default(), "hello", &ctx(&spawner, "/w"))
        .unwrap();
    assert!(!turn.interrupt());
    assert_eq!(turn.wait(), DoneReason::Completed);
}

#[test]
fn agy_drive_resumes_a_conversation() {
    let drive = SpawnDrive::agy();
    assert_eq!(drive.info().transport, Transport::Spawn);
    // Fresh conversation → no --conversation.
    let fresh = drive.spec("do it", std::path::Path::new("/w"), None);
    assert_eq!(fresh.program, "agy");
    assert_eq!(fresh.args, vec!["--print", "do it"]);
    // Resume → inject `--conversation <id>` before the prompt (continuous thread).
    let resumed = drive.spec("keep going", std::path::Path::new("/w"), Some("conv-7"));
    assert_eq!(
        resumed.args,
        vec!["--print", "--conversation", "conv-7", "keep going"]
    );
}

// ---- plugins -> drive -> registry glue (host) ------------------------------------

#[test]
fn trigger_codex_uses_the_app_server_and_registers_a_connection() {
    let spawner = FakeSpawner::new(0);
    let app = FakeAppServer::default();
    let mut reg = ConnectionRegistry::new(5_000);
    let codex = hotsheet_plugins::find_in("codex", &[]).expect("codex plugin");

    // codex is driven via the persistent app-server (not a spawn).
    assert_eq!(
        drive_for(&codex).unwrap().info().transport,
        Transport::AppServer
    );

    let out = trigger(
        &codex,
        "/proj",
        Role::Main,
        "sess-1".into(),
        "work the top ticket",
        &appctx(&spawner, &app, "/proj"),
        &mut reg,
        1_000,
    )
    .unwrap();

    // a connection registered as an app-server transport + marked busy
    let c = reg.get("sess-1").unwrap();
    assert_eq!(c.tool, "codex");
    assert_eq!(c.transport, Transport::AppServer);
    assert!(reg.is_busy("sess-1", 1_000));

    // it opened a NEW thread (no resume) and sent the turn — no process spawned
    assert_eq!(app.opened.borrow().as_ref().unwrap().0, None);
    assert_eq!(
        app.last_turn.borrow().as_ref().unwrap().1,
        "work the top ticket"
    );
    assert!(
        spawner.last.borrow().is_none(),
        "app-server drive spawns no process"
    );

    let mut turn = out.turn;
    assert_eq!(turn.wait(), DoneReason::Completed);
}

#[test]
fn app_server_drive_resumes_a_thread_and_interrupts() {
    let spawner = FakeSpawner::new(0);
    let app = FakeAppServer::default();
    let drive = AppServerDrive::new();
    assert_eq!(drive.info().transport, Transport::AppServer);
    assert!(drive.supports_interrupt());

    // Target selects the thread to resume.
    let mut turn = drive
        .run(
            &Target(Some("thread-42".into())),
            "continue",
            &appctx(&spawner, &app, "/w"),
        )
        .unwrap();
    assert_eq!(
        app.opened.borrow().as_ref().unwrap().0.as_deref(),
        Some("thread-42")
    );
    assert!(turn.is_busy());
    assert!(turn.interrupt());
    assert!(app.interrupted.get());
    assert_eq!(turn.wait(), DoneReason::Interrupted);
}

#[test]
fn app_server_drive_errors_without_a_connection() {
    let spawner = FakeSpawner::new(0);
    // No app_server in the ctx. (Box<dyn TurnHandle> isn't Debug, so match.)
    match AppServerDrive::new().run(&Target::default(), "x", &ctx(&spawner, "/w")) {
        Err(DriveError::NotConnected(_)) => {}
        _ => panic!("expected NotConnected"),
    }

    // A daemon-down client surfaces as an app-server error.
    let app = FakeAppServer {
        fail: true,
        ..Default::default()
    };
    match AppServerDrive::new().run(&Target::default(), "x", &appctx(&spawner, &app, "/w")) {
        Err(DriveError::AppServer(_)) => {}
        _ => panic!("expected AppServer error"),
    }
}

#[test]
fn claude_is_drivable_via_the_channel_transport() {
    let claude = hotsheet_plugins::find_in("claude", &[]).expect("claude plugin");
    let drive = drive_for(&claude).expect("claude declares a channel drive");
    assert_eq!(drive.info().transport, Transport::ClaudeChannel);
    assert!(
        !drive.supports_interrupt(),
        "no channel interrupt in phase 1"
    );
}

// ---- the real AppServerClient over a scripted daemon (HS2-112) -------------------

use crate::codex::CodexAppServer;
use crate::codex::scripted::{ScriptedDaemon, TurnMode};

#[test]
fn codex_client_starts_a_thread_and_completes_a_turn() {
    let cx = CodexAppServer::connect(ScriptedDaemon::new(TurnMode::AutoComplete)).unwrap();
    let thread = cx
        .open_thread(None, std::path::Path::new("/work/proj"))
        .unwrap();
    assert_eq!(thread, "thread-1", "new thread id from thread/start");

    let mut turn = cx.start_turn(&thread, "work the top ticket").unwrap();
    assert_eq!(turn.wait(), AppServerOutcome::Completed);
    assert!(!turn.is_running());
    assert_eq!(
        turn.wait(),
        AppServerOutcome::Completed,
        "wait is idempotent"
    );
}

#[test]
fn codex_client_resumes_a_thread_by_id() {
    let cx = CodexAppServer::connect(ScriptedDaemon::new(TurnMode::AutoComplete)).unwrap();
    // Resuming echoes the requested thread id back (thread/resume by threadId).
    let thread = cx
        .open_thread(Some("thread-42"), std::path::Path::new("/w"))
        .unwrap();
    assert_eq!(thread, "thread-42");
    let mut turn = cx.start_turn(&thread, "keep going").unwrap();
    assert_eq!(turn.wait(), AppServerOutcome::Completed);
}

#[test]
fn codex_client_maps_a_failed_turn() {
    let cx = CodexAppServer::connect(ScriptedDaemon::new(TurnMode::AutoFail)).unwrap();
    let thread = cx.open_thread(None, std::path::Path::new("/w")).unwrap();
    let mut turn = cx.start_turn(&thread, "x").unwrap();
    match turn.wait() {
        AppServerOutcome::Failed(msg) => assert_eq!(msg, "boom"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn codex_client_interrupts_a_running_turn() {
    let cx = CodexAppServer::connect(ScriptedDaemon::new(TurnMode::UntilInterrupt)).unwrap();
    let thread = cx.open_thread(None, std::path::Path::new("/w")).unwrap();
    let mut turn = cx.start_turn(&thread, "long task").unwrap();
    assert!(turn.is_running(), "no turn/completed until interrupt");
    turn.interrupt();
    assert!(!turn.is_running());
}

#[test]
fn codex_client_drives_the_appserver_drive_end_to_end() {
    // The real client behind the AppServerDrive: a full trigger, no process spawned.
    let cx = CodexAppServer::connect(ScriptedDaemon::new(TurnMode::AutoComplete)).unwrap();
    let spawner = FakeSpawner::new(0);
    let ctx = DriveCtx {
        cwd: PathBuf::from("/proj"),
        spawner: &spawner,
        env: Vec::new(),
        app_server: Some(&cx),
        channel: None,
    };
    let mut turn = AppServerDrive::new()
        .run(&Target::default(), "work the top ticket", &ctx)
        .unwrap();
    assert_eq!(turn.wait(), DoneReason::Completed);
    assert!(
        spawner.last.borrow().is_none(),
        "app-server drive spawns no process"
    );
}

/// LIVE, gated: a real `codex app-server` turn against a persistent codex process. Off by
/// default (and even under `--ignored` it needs `HOTSHEET_CODEX_LIVE=1`), because it needs
/// Codex creds and invokes the model. Run under the HS2-103 launch safety (isolated
/// `CODEX_HOME`, no HS1-reachable MCP). Proves the real `StdioTransport` + `CodexAppServer`
/// drive one persistent instance through a complete turn (no process per play).
#[test]
#[ignore = "live: needs a real codex + creds; set HOTSHEET_CODEX_LIVE=1"]
fn codex_live_turn_against_the_daemon() {
    if std::env::var("HOTSHEET_CODEX_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set HOTSHEET_CODEX_LIVE=1 to run the live codex turn");
        return;
    }
    let program = std::env::var("HOTSHEET_CODEX_BIN").unwrap_or_else(|_| "codex".into());
    let cwd = std::env::var("CODEX_LIVE_CWD")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let transport =
        crate::codex::StdioTransport::spawn(&program, &cwd, &[]).expect("app-server spawn");
    let cx = CodexAppServer::connect(transport).expect("initialize handshake");
    let thread = cx.open_thread(None, &cwd).expect("thread/start");
    eprintln!("live: opened thread {thread}");
    let mut turn = cx
        .start_turn(&thread, "Reply with only the word: pong")
        .expect("turn/start");
    let outcome = turn.wait();
    eprintln!("live: turn outcome = {outcome:?}");
    assert_eq!(
        outcome,
        AppServerOutcome::Completed,
        "live turn should complete"
    );
}

// ---- the shared-daemon WebSocket transport over a scripted WS daemon (HS2-115) ----

/// A scripted WebSocket "daemon": binds a Unix socket, upgrades one connection with
/// `accept_async` (exactly like the real codex control socket), and answers the
/// `initialize`/`thread/start`/`turn/start` JSON-RPC as text frames — so the real
/// [`UdsWsTransport`] and [`CodexAppServer`] are exercised end to end over a WebSocket with
/// no codex. It signals `ready` only after the listener binds so the client can't race it.
fn spawn_scripted_ws_daemon(sock: std::path::PathBuf) -> std::sync::mpsc::Receiver<()> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            use futures_util::{SinkExt, StreamExt};
            use serde_json::{Value, json};
            use tokio_tungstenite::tungstenite::Message;

            let listener = tokio::net::UnixListener::bind(&sock).expect("bind uds");
            ready_tx.send(()).unwrap();

            let (stream, _) = listener.accept().await.expect("accept");
            let ws = tokio_tungstenite::accept_async(stream)
                .await
                .expect("ws upgrade");
            let (mut write, mut read) = ws.split();

            while let Some(Ok(Message::Text(t))) = read.next().await {
                let v: Value = serde_json::from_str(&t).expect("client sends valid JSON");
                let method = v.get("method").and_then(Value::as_str).unwrap_or("");
                let id = v.get("id").cloned();
                let mut out: Vec<Value> = Vec::new();
                match method {
                    "initialize" => {
                        if let Some(id) = &id {
                            out.push(json!({"jsonrpc":"2.0","id":id,
                                "result":{"userAgent":"codex/test"}}));
                        }
                    }
                    "initialized" => {}
                    "thread/start" => {
                        if let Some(id) = &id {
                            out.push(json!({"jsonrpc":"2.0","id":id,
                                "result":{"thread":{"id":"thread-1"}}}));
                        }
                    }
                    "turn/start" => {
                        let thread_id = v
                            .pointer("/params/threadId")
                            .and_then(Value::as_str)
                            .unwrap_or("thread-1")
                            .to_string();
                        if let Some(id) = &id {
                            out.push(json!({"jsonrpc":"2.0","id":id,
                                "result":{"turn":{"id":"turn-1","status":"inProgress"}}}));
                        }
                        out.push(json!({"jsonrpc":"2.0","method":"turn/completed",
                            "params":{"threadId":thread_id,
                                      "turn":{"id":"turn-1","status":"completed"}}}));
                    }
                    _ => {
                        if let Some(id) = &id {
                            out.push(json!({"jsonrpc":"2.0","id":id,"result":{}}));
                        }
                    }
                }
                for msg in out {
                    if write
                        .send(Message::Text(msg.to_string().into()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        });
    });
    ready_rx
}

/// Proves the WebSocket framing: the real [`UdsWsTransport`] connects to a scripted WS
/// daemon over a temp UDS, and the unchanged [`CodexAppServer`] engine handshakes, opens a
/// thread, and completes a turn — the same protocol as [`ScriptedDaemon`], just WS-framed.
#[test]
fn codex_client_drives_a_turn_over_a_websocket_uds() {
    use crate::codex::UdsWsTransport;

    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("s.sock");
    let ready = spawn_scripted_ws_daemon(sock.clone());
    ready.recv().expect("daemon bound its socket");

    let transport = UdsWsTransport::connect(&sock).expect("connect + ws handshake");
    let cx = CodexAppServer::connect(transport).expect("initialize over websocket");
    let thread = cx
        .open_thread(None, std::path::Path::new("/work/proj"))
        .expect("thread/start over websocket");
    assert_eq!(thread, "thread-1");
    let mut turn = cx.start_turn(&thread, "work the top ticket").unwrap();
    assert_eq!(turn.wait(), AppServerOutcome::Completed);
}

/// Connecting to a socket path that no daemon is serving fails at connect time (a clean
/// error), not as a later `initialize` timeout.
#[test]
fn uds_ws_transport_reports_a_missing_socket() {
    use crate::codex::UdsWsTransport;
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.sock");
    assert!(
        UdsWsTransport::connect(&missing).is_err(),
        "no daemon at the path → connect fails"
    );
}

/// LIVE, gated: a real turn against the **shared** codex daemon over the WebSocket control
/// socket ([`UdsWsTransport`]). Off by default; set `HOTSHEET_CODEX_LIVE=1`. Proves the
/// shared-daemon path (multiple host connections can reuse one codex instance) end to end.
#[test]
#[ignore = "live: needs a real codex daemon + creds; set HOTSHEET_CODEX_LIVE=1"]
fn codex_live_turn_over_the_shared_daemon() {
    use crate::codex::{UdsWsTransport, codex_control_socket_path, ensure_codex_daemon};

    if std::env::var("HOTSHEET_CODEX_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set HOTSHEET_CODEX_LIVE=1 to run the shared-daemon turn");
        return;
    }
    let program = std::env::var("HOTSHEET_CODEX_BIN").unwrap_or_else(|_| "codex".into());
    let codex_home = std::env::var("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".codex")
        });
    let cwd = std::env::var("CODEX_LIVE_CWD")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());

    ensure_codex_daemon(&program).expect("daemon start");
    let sock = codex_control_socket_path(&codex_home);
    let transport = UdsWsTransport::connect(&sock).expect("connect to control socket");
    let cx = CodexAppServer::connect(transport).expect("initialize over the shared daemon");
    let thread = cx.open_thread(None, &cwd).expect("thread/start");
    eprintln!("live(shared): opened thread {thread}");
    let mut turn = cx
        .start_turn(&thread, "Reply with only the word: pong")
        .expect("turn/start");
    let outcome = turn.wait();
    eprintln!("live(shared): turn outcome = {outcome:?}");
    assert_eq!(
        outcome,
        AppServerOutcome::Completed,
        "live turn should complete"
    );
}

// ---- the Claude channel drive over a scripted claude (HS2-116) -------------------

use crate::claude::ClaudeChannel;
use crate::claude::scripted::{ClaudeMode, ScriptedClaude};

fn chanctx<'a>(spawner: &'a FakeSpawner, ch: &'a ClaudeChannel, cwd: &str) -> DriveCtx<'a> {
    DriveCtx {
        cwd: PathBuf::from(cwd),
        spawner,
        env: Vec::new(),
        app_server: None,
        channel: Some(ch),
    }
}

#[test]
fn claude_channel_streams_output_then_done() {
    let ch = ClaudeChannel::connect(ScriptedClaude::new(ClaudeMode::Success));
    let mut turn = ch.start_turn("work the top ticket").unwrap();

    // Streaming view: assistant Output, then a terminal Done.
    assert_eq!(
        turn.next_event(),
        Some(TurnEvent::Output("done: work the top ticket".into()))
    );
    assert_eq!(
        turn.next_event(),
        Some(TurnEvent::Done(DoneReason::Completed))
    );
    assert_eq!(turn.next_event(), None, "stream ends after Done");
    assert!(!turn.is_busy());
}

#[test]
fn claude_channel_captures_the_session_id_and_maps_failure() {
    let ch = ClaudeChannel::connect(ScriptedClaude::new(ClaudeMode::Failure));
    let mut turn = ch.start_turn("x").unwrap();
    // wait() drains the stream to the terminal reason.
    assert_eq!(turn.wait(), DoneReason::Failed(1));
    // session id came from the `system`/`init` event.
    assert_eq!(ch.session_id().as_deref(), Some("sess-abc"));
}

#[test]
fn claude_channel_runs_two_sequential_turns() {
    // Transition test: a second turn on the same channel starts fresh from its cursor.
    let ch = ClaudeChannel::connect(ScriptedClaude::new(ClaudeMode::Success));
    let mut t1 = ch.start_turn("first").unwrap();
    assert_eq!(t1.wait(), DoneReason::Completed);

    let mut t2 = ch.start_turn("second").unwrap();
    assert_eq!(
        t2.next_event(),
        Some(TurnEvent::Output("done: second".into())),
        "second turn sees its own output, not the first's"
    );
    assert_eq!(t2.wait(), DoneReason::Completed);
}

#[test]
fn claude_channel_drive_runs_a_turn_end_to_end() {
    let ch = ClaudeChannel::connect(ScriptedClaude::new(ClaudeMode::Success));
    let spawner = FakeSpawner::new(0);
    let mut turn = ClaudeChannelDrive
        .run(
            &Target::default(),
            "work the top ticket",
            &chanctx(&spawner, &ch, "/proj"),
        )
        .unwrap();
    assert_eq!(turn.wait(), DoneReason::Completed);
    assert!(
        spawner.last.borrow().is_none(),
        "channel drive spawns no process"
    );
}

#[test]
fn claude_channel_drive_errors_without_a_connection() {
    let spawner = FakeSpawner::new(0);
    // No channel in the ctx. (Box<dyn TurnHandle> isn't Debug, so match.)
    match ClaudeChannelDrive.run(&Target::default(), "x", &ctx(&spawner, "/w")) {
        Err(DriveError::NotConnected(_)) => {}
        _ => panic!("expected NotConnected"),
    }
}

#[test]
fn trigger_claude_uses_the_channel_and_registers_a_connection() {
    let spawner = FakeSpawner::new(0);
    let ch = ClaudeChannel::connect(ScriptedClaude::new(ClaudeMode::Success));
    let mut reg = ConnectionRegistry::new(5_000);
    let claude = hotsheet_plugins::find_in("claude", &[]).expect("claude plugin");

    let out = trigger(
        &claude,
        "/proj",
        Role::Main,
        "sess-1".into(),
        "work the top ticket",
        &chanctx(&spawner, &ch, "/proj"),
        &mut reg,
        1_000,
    )
    .unwrap();

    let c = reg.get("sess-1").unwrap();
    assert_eq!(c.tool, "claude");
    assert_eq!(c.transport, Transport::ClaudeChannel);
    assert!(reg.is_busy("sess-1", 1_000));

    let mut turn = out.turn;
    assert_eq!(turn.wait(), DoneReason::Completed);
}

/// LIVE, gated: a real turn through a persistent `claude` stream-json session. Off by
/// default (needs `HOTSHEET_CLAUDE_LIVE=1`); needs Claude creds and invokes the model. Run
/// under HS2-103 safety in an isolated, MCP-free cwd. Proves the real `ClaudeStreamTransport`
/// + `ClaudeChannel` drive one persistent session through a complete turn.
#[test]
#[ignore = "live: needs a real claude + creds; set HOTSHEET_CLAUDE_LIVE=1"]
fn claude_live_turn_over_the_channel() {
    if std::env::var("HOTSHEET_CLAUDE_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set HOTSHEET_CLAUDE_LIVE=1 to run the live claude turn");
        return;
    }
    let program = std::env::var("HOTSHEET_CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
    let cwd = std::env::var("CLAUDE_LIVE_CWD")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    // Only the given (empty) MCP config, so nothing else is reachable.
    let mcp = std::env::var("CLAUDE_LIVE_MCP")
        .ok()
        .map(std::path::PathBuf::from);
    let transport = crate::claude::ClaudeStreamTransport::spawn(
        &program,
        &cwd,
        None,
        mcp.as_deref(),
        None,
        &[],
    )
    .expect("claude spawn");
    let ch = ClaudeChannel::connect(transport);
    let mut turn = ch
        .start_turn("Reply with only the word: pong")
        .expect("start_turn");
    // Stream the events, then assert the terminal reason.
    let mut saw_output = false;
    let reason = loop {
        match turn.next_event() {
            Some(TurnEvent::Output(t)) => {
                eprintln!("live: output {t:?}");
                saw_output = true;
            }
            Some(TurnEvent::PermissionAsked(p)) => eprintln!("live: permission {p:?}"),
            Some(TurnEvent::Done(r)) => break r,
            None => break turn.wait(),
        }
    };
    eprintln!("live: done {reason:?}, session={:?}", ch.session_id());
    assert!(saw_output, "streamed at least one assistant output");
    assert_eq!(reason, DoneReason::Completed, "live turn should complete");
}

// ---- the live trigger over a real (harmless) spawn tool (HS2-109) ----------------

use crate::live::{LiveError, LiveTrigger, run_trigger};

/// Write a minimal on-disk plugin whose `[drive]` is a real spawn of `/bin/sh -c <prompt>`
/// — a genuine live trigger (SystemSpawner + host::trigger + registry) with no AI tool.
fn write_sh_plugin(dir: &std::path::Path) -> hotsheet_plugins::Plugin {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        r#"id = "shtool"
display_name = "Sh Tool"
product_name = "Sh Tool"
tier = "cli-agent"
[detection]
binaries = ["sh"]
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
[drive]
transport = "spawn"
program = "/bin/sh"
args = ["-c"]
content = "arg"
interrupt = false
"#,
    )
    .unwrap();
    std::fs::write(dir.join("instructions.md"), "## Hot Sheet\n").unwrap();
    hotsheet_plugins::Plugin::from_fs_dir(dir).unwrap()
}

fn live(prompt: &str, cwd: &std::path::Path) -> LiveTrigger {
    LiveTrigger {
        cwd: cwd.to_path_buf(),
        prompt: prompt.to_string(),
        role: Role::Main,
        conn_id: "conn-1".into(),
        resume: None,
        mcp_config: None,
        permission_mode: None,
        env: Vec::new(),
        shared_daemon: false,
        permission_bridge: None,
        now_ms: 1_000,
    }
}

#[test]
fn run_trigger_drives_a_real_spawn_tool_and_tracks_the_connection() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin = write_sh_plugin(&tmp.path().join("shtool"));
    let mut reg = ConnectionRegistry::new(5_000);

    let mut events = Vec::new();
    let reason = run_trigger(&plugin, &live("exit 0", tmp.path()), &mut reg, &mut |ev| {
        events.push(format!("{ev:?}"))
    })
    .unwrap();

    assert_eq!(reason, DoneReason::Completed);
    // The connection was registered (spawn transport) and then set idle at Done.
    let c = reg.get("conn-1").expect("connection registered");
    assert_eq!(c.tool, "shtool");
    assert_eq!(c.transport, Transport::Spawn);
    assert!(!reg.is_busy("conn-1", 1_000), "set idle after the turn");
    // A spawn tool streams no output events, only the terminal Done.
    assert_eq!(events, vec!["Done(Completed)"]);
}

#[test]
fn run_trigger_threads_env_into_a_spawn_tool() {
    // The sh drive runs `/bin/sh -c <prompt>`; the prompt exits 0 only if FOO reached the
    // process env — proving LiveTrigger.env is threaded through SpawnDrive (HS2-0TWTZ4,
    // the HS2-103 safety PATH shim rides the same path).
    let tmp = tempfile::tempdir().unwrap();
    let plugin = write_sh_plugin(&tmp.path().join("shtool"));
    let mut reg = ConnectionRegistry::new(5_000);

    let mut t = live(r#"[ "$FOO" = bar ]"#, tmp.path());
    t.env = vec![("FOO".to_string(), "bar".to_string())];
    let reason = run_trigger(&plugin, &t, &mut reg, &mut |_| {}).unwrap();
    assert_eq!(
        reason,
        DoneReason::Completed,
        "env FOO=bar reached the spawned process"
    );

    // Sanity: without the env, the same check fails (FOO unset) — so the pass above is the
    // env being threaded, not the prompt always succeeding.
    let reason = run_trigger(
        &plugin,
        &live(r#"[ "$FOO" = bar ]"#, tmp.path()),
        &mut reg,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(reason, DoneReason::Failed(1), "unset FOO → the check fails");
}

#[test]
fn run_trigger_reports_a_nonzero_exit_as_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin = write_sh_plugin(&tmp.path().join("shtool"));
    let mut reg = ConnectionRegistry::new(5_000);
    let reason = run_trigger(&plugin, &live("exit 5", tmp.path()), &mut reg, &mut |_| {}).unwrap();
    assert_eq!(reason, DoneReason::Failed(5));
}

#[test]
fn run_trigger_rejects_a_non_drivable_plugin() {
    // The built-in Claude plugin IS drivable; a plugin with no [drive] is not. Use a
    // minimal on-disk plugin without a [drive] block.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("nodrive");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        r#"id = "nodrive"
display_name = "No Drive"
product_name = "No Drive"
tier = "cli-agent"
[detection]
binaries = ["nodrive"]
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
    )
    .unwrap();
    std::fs::write(dir.join("instructions.md"), "## Hot Sheet\n").unwrap();
    let plugin = hotsheet_plugins::Plugin::from_fs_dir(&dir).unwrap();
    let mut reg = ConnectionRegistry::new(5_000);
    match run_trigger(&plugin, &live("x", tmp.path()), &mut reg, &mut |_| {}) {
        Err(LiveError::NotDrivable(id)) => assert_eq!(id, "nodrive"),
        other => panic!(
            "expected NotDrivable, got {:?}",
            other.map(|r| format!("{r:?}"))
        ),
    }
    assert_eq!(reg.count(), 0, "no connection on a non-drivable trigger");
}

// ---- the real adapter ------------------------------------------------------------

#[test]
fn system_spawner_runs_a_real_process() {
    let s = SystemSpawner;
    let spec = |script: &str| SpawnSpec {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: std::env::temp_dir(),
        stdin: None,
        env: Vec::new(),
    };
    assert_eq!(s.spawn(&spec("exit 0")).unwrap().wait(), 0);
    assert_eq!(s.spawn(&spec("exit 7")).unwrap().wait(), 7);
}

// ---- BackingService accessor (HS2-V5Z2EY) ----------------------------------------

#[test]
fn spawn_and_channel_drives_have_no_backing_service() {
    // A generic caller sees `None` and warms nothing — the process is owned per turn.
    assert!(SpawnDrive::codex().service().is_none());
    assert!(ClaudeChannelDrive.service().is_none());
    assert!(AppServerDrive::new().service().is_none());
}

#[test]
fn app_server_drive_exposes_its_daemon_as_a_backing_service() {
    // `with_daemon` surfaces the Codex daemon through the tool-id-free accessor, so a
    // caller prestarts it without importing `codex`. `true`/`false` stand in for the
    // real `codex` binary: they ignore the `app-server daemon start` args and exit 0/1.
    let drive = AppServerDrive::with_daemon("true");
    let svc = drive
        .service()
        .expect("app-server drive has a backing service");
    assert_eq!(svc.name(), "codex-app-server");
    assert!(svc.prestart().is_ok(), "`true` prestart exits 0 → Ok");

    let failing = AppServerDrive::with_daemon("false");
    assert!(
        failing.service().unwrap().prestart().is_err(),
        "`false` prestart exits 1 → Err"
    );
}

#[test]
fn drive_for_app_server_plugin_carries_the_daemon_service() {
    // The host builds the daemon accessor from the plugin's `program` (`true` stands in
    // for a real `codex`: it ignores the daemon-start args and exits 0).
    let dir = std::env::temp_dir().join(format!("hs2-appsvc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        r#"id = "appsvc"
display_name = "App Svc"
product_name = "App Svc"
tier = "cli-agent"
[detection]
binaries = ["true"]
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
[drive]
transport = "app-server"
program = "true"
interrupt = true
"#,
    )
    .unwrap();
    std::fs::write(dir.join("instructions.md"), "## Hot Sheet\n").unwrap();
    let plugin = hotsheet_plugins::Plugin::from_fs_dir(&dir).unwrap();

    let drive = drive_for(&plugin).expect("app-server is drivable");
    let svc = drive.service().expect("drive_for wired the daemon service");
    assert_eq!(svc.name(), "codex-app-server");
    assert!(svc.prestart().is_ok());
    std::fs::remove_dir_all(&dir).ok();
}
