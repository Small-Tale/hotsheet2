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
fn claude_is_not_drivable_yet_and_leaves_no_connection() {
    let claude = hotsheet_plugins::find_in("claude", &[]).expect("claude plugin");
    assert!(drive_for(&claude).is_none(), "channel drive not built yet");

    let spawner = FakeSpawner::new(0);
    let mut reg = ConnectionRegistry::new(5_000);
    // `Triggered` holds a trait object (not Debug), so match rather than unwrap_err.
    match trigger(
        &claude,
        "/p",
        Role::Main,
        "s".into(),
        "x",
        &ctx(&spawner, "/p"),
        &mut reg,
        0,
    ) {
        Err(TriggerError::NotDrivable(id)) => assert_eq!(id, "claude"),
        other => panic!("expected NotDrivable, got {:?}", other.is_ok()),
    }
    assert_eq!(
        reg.count(),
        0,
        "no connection registered on a failed trigger"
    );
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
