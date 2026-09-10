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
use std::process::{Command, Stdio};
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
        let mut command = Command::new(&self.program);
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let output = command
            .output()
            .map_err(|error| format!("starting '{} --version': {error}", self.program))?;
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
        let transport = StdioTransport::spawn(&self.program, cwd, &self.env)
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
        if let Some(d) = self.done {
            return d;
        }
        let reason = match self.turn.wait() {
            AppServerOutcome::Completed => DoneReason::Completed,
            AppServerOutcome::Failed(_) => DoneReason::Failed(1),
        };
        self.done = Some(reason);
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
            self.done = Some(*reason);
        }
        Some(event)
    }

    fn usage(&mut self) -> Option<crate::drive::Usage> {
        self.turn.usage()
    }
}
