//! The **app-server** (persistent daemon) drive shape (`docs/13` §13.1, the Codex
//! "app-server" column). A play is a `turn/start` on a **new or resumed thread** against
//! the already-running `codex app-server daemon` — **not** a fresh process per turn
//! (this is what HS1 used, docs/121, and what agy/spawn deliberately isn't).
//!
//! The drive is transport logic over an injected [`AppServerClient`] (in the
//! [`DriveCtx`]), so it's testable against a fake daemon. The real client is
//! [`crate::codex::CodexAppServer`] — it speaks the `initialize` → `thread/*` → `turn/*`
//! JSON-RPC over stdio ([`crate::codex::StdioTransport`]) and over the daemon's control
//! socket ([`crate::codex::UdsWsTransport`], via `live::connect_shared_daemon`). Driving
//! the daemon through the `codex app-server proxy` byte-relay specifically is still open
//! (HS2-115).

use std::path::Path;
use std::time::Duration;

use crate::codex::{CodexAppServer, CodexDaemonService, StdioTransport};
use crate::drive::{
    BackingService, DoneReason, Drive, DriveCtx, DriveError, DriveInfo, Target, Transport,
    TurnHandle,
};
use crate::model_catalog::{RuntimeModelCatalog, RuntimeModelCatalogSource};
use crate::ports::{AppServerError, AppServerOutcome, AppServerTurn};

const MODEL_CATALOG_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

/// Drives Codex via its persistent app-server daemon. Optionally carries the
/// [`CodexDaemonService`] so a generic caller can prestart the daemon through
/// [`Drive::service`] without importing [`crate::codex`] (`docs/13` §13.5).
#[derive(Default)]
pub struct AppServerDrive {
    service: Option<CodexDaemonService>,
    model_catalog: Option<AppServerModelCatalog>,
}

impl AppServerDrive {
    /// A drive with no backing-service handle — [`Drive::service`] returns `None`, so the
    /// daemon must already be warm (the historical unit-struct behavior).
    pub fn new() -> Self {
        Self::default()
    }

    /// A drive that exposes `program`'s app-server daemon through [`Drive::service`].
    pub fn with_daemon(program: impl Into<String>) -> Self {
        let program = program.into();
        Self {
            service: Some(CodexDaemonService::new(program.clone())),
            model_catalog: Some(AppServerModelCatalog::new(program, Vec::new())),
        }
    }

    /// A drive whose backing daemon targets a specific isolated `CODEX_HOME` (HS2-B7C66H).
    pub fn with_daemon_home(
        program: impl Into<String>,
        codex_home: impl Into<std::path::PathBuf>,
    ) -> Self {
        let program = program.into();
        let codex_home = codex_home.into();
        Self {
            service: Some(CodexDaemonService::with_home(
                program.clone(),
                codex_home.clone(),
            )),
            model_catalog: Some(AppServerModelCatalog::new(
                program,
                vec![("CODEX_HOME".into(), codex_home.display().to_string())],
            )),
        }
    }
}

impl Drive for AppServerDrive {
    fn info(&self) -> DriveInfo {
        DriveInfo {
            transport: Transport::AppServer,
        }
    }

    fn supports_interrupt(&self) -> bool {
        true // `turn/interrupt`
    }

    fn service(&self) -> Option<&dyn BackingService> {
        self.service.as_ref().map(|s| s as &dyn BackingService)
    }

    fn model_catalog(&self) -> Option<&dyn RuntimeModelCatalogSource> {
        self.model_catalog
            .as_ref()
            .map(|catalog| catalog as &dyn RuntimeModelCatalogSource)
    }

    fn run(
        &self,
        target: &Target,
        content: &str,
        ctx: &DriveCtx,
    ) -> Result<Box<dyn TurnHandle>, DriveError> {
        let client = ctx
            .app_server
            .ok_or_else(|| DriveError::NotConnected("codex app-server not connected".into()))?;
        // `Target` selects which thread to resume; None starts a fresh thread.
        let thread = client
            .open_thread(target.0.as_deref(), &ctx.cwd)
            .map_err(as_drive_err)?;
        let turn = client
            .start_turn(
                &thread,
                content,
                ctx.model.as_deref(),
                ctx.effort.as_deref(),
            )
            .map_err(as_drive_err)?;
        Ok(Box::new(AppServerTurnHandle { turn, done: None }))
    }
}

#[derive(Debug)]
struct AppServerModelCatalog {
    program: String,
    env: Vec<(String, String)>,
}

impl AppServerModelCatalog {
    fn new(program: String, env: Vec<(String, String)>) -> Self {
        Self { program, env }
    }
}

impl RuntimeModelCatalogSource for AppServerModelCatalog {
    fn version(&self) -> Result<String, String> {
        let mut command = hotsheet_ticketing::git::launch(&self.program);
        command.arg("--version");
        for (key, value) in &self.env {
            command.env(key, value);
        }
        // Bounded: a hung tool is killed and the manifest catalog stands in (HS2-BH3M53).
        let output = crate::probe::run_probe(&mut command)?;
        if !output.status.success() {
            return Err(format!(
                "'{} --version' exited with {}",
                self.program, output.status
            ));
        }
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if version.is_empty() {
            Err(format!("'{} --version' returned no version", self.program))
        } else {
            Ok(version)
        }
    }

    fn discover(&self, cwd: &Path) -> Result<RuntimeModelCatalog, String> {
        // A probe child, not a session (HS2-BJ7A59): its own process group, killed at the
        // probe deadline, when the probe is stopped, and when this listing returns (`_guard`
        // drops last), so a wedged app-server can never outlive it.
        let (transport, _guard) = StdioTransport::spawn_probe(
            &self.program,
            cwd,
            &self.env,
            crate::probe::probe_timeout(),
        )
        .map_err(|error| format!("starting '{} app-server': {error}", self.program))?;
        CodexAppServer::connect_with_timeout(transport, MODEL_CATALOG_REQUEST_TIMEOUT)
            .map_err(|error| error.to_string())?
            .list_models()
            .map_err(|error| error.to_string())
    }
}

fn as_drive_err(e: AppServerError) -> DriveError {
    DriveError::AppServer(e.to_string())
}

/// Observes one app-server turn: busy = the turn is in flight; done = its outcome.
struct AppServerTurnHandle {
    turn: Box<dyn AppServerTurn>,
    done: Option<DoneReason>,
}

impl TurnHandle for AppServerTurnHandle {
    fn is_busy(&mut self) -> bool {
        self.done.is_none() && self.turn.is_running()
    }

    fn wait(&mut self) -> DoneReason {
        if let Some(d) = &self.done {
            return d.clone();
        }
        let reason = match self.turn.wait() {
            AppServerOutcome::Completed => DoneReason::Completed,
            AppServerOutcome::Interrupted => DoneReason::Interrupted,
            AppServerOutcome::Failed(message) => DoneReason::failed(1, message),
        };
        self.done = Some(reason.clone());
        reason
    }

    fn interrupt(&mut self) -> bool {
        if self.done.is_none() {
            self.turn.interrupt();
            self.done = Some(DoneReason::Interrupted);
            true
        } else {
            false
        }
    }

    fn next_event(&mut self) -> Option<crate::drive::TurnEvent> {
        if self.done.is_some() {
            return None;
        }
        let event = self.turn.next_event()?;
        if let crate::drive::TurnEvent::Done(reason) = &event {
            self.done = Some(reason.clone());
        }
        Some(event)
    }

    fn usage(&mut self) -> Option<crate::drive::Usage> {
        self.turn.usage()
    }
}

#[cfg(all(test, unix))]
mod catalog_probe_tests {
    //! HS2-BJ7A59: the codex app-server started for a model listing is a probe child. A fake
    //! `codex` that answers `--version` but wedges as `app-server` (never replying, ignoring
    //! stdin EOF) must not outlive the listing, its deadline, or a stopped probe set.

    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use super::*;

    /// A fake `codex` whose `app-server` records its pid and then hangs forever.
    fn wedged_codex(dir: &Path) -> (String, PathBuf) {
        let program = dir.join("codex");
        let pid_file = dir.join("app-server.pid");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\nif [ \"$1\" = --version ]; then echo codex-test 1.0; exit 0; fi\n\
                 echo $$ > '{}'\nexec sleep 30\n",
                pid_file.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        (program.display().to_string(), pid_file)
    }

    fn app_server_pid(pid_file: &Path) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(pid) = std::fs::read_to_string(pid_file)
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "the fake app-server never started"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_gone(pid: i32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        // SAFETY: signal 0 only checks whether the pid still exists.
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(
                Instant::now() < deadline,
                "app-server {pid} outlived its probe"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_wedged_catalog_app_server_is_killed_when_its_listing_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let (program, pid_file) = wedged_codex(dir.path());
        let catalog = AppServerModelCatalog::new(program, Vec::new());
        assert_eq!(catalog.version().unwrap(), "codex-test 1.0");
        let started = Instant::now();
        let error = catalog.discover(dir.path()).unwrap_err();
        assert!(error.contains("initialize"), "{error}");
        assert!(started.elapsed() < MODEL_CATALOG_REQUEST_TIMEOUT + Duration::from_secs(2));
        assert_gone(app_server_pid(&pid_file));
        assert_eq!(crate::probe::in_flight_probes(), 0);
    }

    #[test]
    fn stopping_probes_kills_a_catalog_app_server_mid_listing_and_refuses_new_ones() {
        if crate::probe::isolated_probe_test(
            "appserver::catalog_probe_tests::stopping_probes_kills_a_catalog_app_server_mid_listing_and_refuses_new_ones",
        ) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let (program, pid_file) = wedged_codex(dir.path());
        let listing = {
            let catalog = AppServerModelCatalog::new(program.clone(), Vec::new());
            let cwd = dir.path().to_path_buf();
            std::thread::spawn(move || catalog.discover(&cwd))
        };
        let pid = app_server_pid(&pid_file);
        assert!(crate::probe::stop_probes() >= 1);
        assert_gone(pid);
        assert!(listing.join().unwrap().is_err());
        // A listing that starts after the stop never spawns an app-server.
        std::fs::remove_file(&pid_file).unwrap();
        let catalog = AppServerModelCatalog::new(program, Vec::new());
        let error = catalog.discover(dir.path()).unwrap_err();
        assert!(error.contains("stopping"), "{error}");
        assert!(!pid_file.exists());
    }

    #[test]
    fn the_probe_deadline_kills_a_wedged_app_server_child() {
        // Each JSON-RPC request is bounded, but the probe deadline bounds the child itself.
        let dir = tempfile::tempdir().unwrap();
        let (program, pid_file) = wedged_codex(dir.path());
        let (_transport, guard) =
            StdioTransport::spawn_probe(&program, dir.path(), &[], Duration::from_millis(1_500))
                .unwrap();
        assert_gone(app_server_pid(&pid_file));
        drop(guard);
    }
}
