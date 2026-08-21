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
    let drive = AppServerDrive;
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
    match AppServerDrive.run(&Target::default(), "x", &ctx(&spawner, "/w")) {
        Err(DriveError::NotConnected(_)) => {}
        _ => panic!("expected NotConnected"),
    }

    // A daemon-down client surfaces as an app-server error.
    let app = FakeAppServer {
        fail: true,
        ..Default::default()
    };
    match AppServerDrive.run(&Target::default(), "x", &appctx(&spawner, &app, "/w")) {
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
        app_server: Some(&cx),
        channel: None,
    };
    let mut turn = AppServerDrive
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
    let transport = crate::codex::StdioTransport::spawn(&program, &cwd).expect("app-server spawn");
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

// ---- the Claude channel drive over a scripted claude (HS2-116) -------------------

use crate::claude::ClaudeChannel;
use crate::claude::scripted::{ClaudeMode, ScriptedClaude};

fn chanctx<'a>(spawner: &'a FakeSpawner, ch: &'a ClaudeChannel, cwd: &str) -> DriveCtx<'a> {
    DriveCtx {
        cwd: PathBuf::from(cwd),
        spawner,
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
    let transport =
        crate::claude::ClaudeStreamTransport::spawn(&program, &cwd, None, mcp.as_deref())
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
