//! The **spawn-per-run** drive shape (`docs/13` §13.1, the Codex `exec` /
//! Antigravity `--print` column): each turn spawns a one-shot process; done = the
//! process exits. The simplest shape, and the one that proves the interface isn't
//! Claude-shaped.

use std::path::Path;

use crate::drive::{
    DoneReason, Drive, DriveCtx, DriveError, DriveInfo, Target, Transport, TurnHandle,
};
use crate::ports::{SpawnSpec, SpawnedProcess};

/// How a spawn-shape tool turns a prompt into a command. Data-driven — later sourced
/// from a plugin manifest's launch command.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    pub program: String,
    /// Fixed args before the content (e.g. `["--print"]` for `agy --print`).
    pub args: Vec<String>,
    /// How the prompt content reaches the tool.
    pub content: ContentMode,
    /// Whether the tool can be interrupted mid-turn.
    pub interrupt: bool,
    /// If set, and the `Target` carries a session id, inject `<flag> <id>` to **resume
    /// the same conversation** rather than start a fresh one (agy `--conversation`). The
    /// best a spawn tool can do for continuity without a daemon (`docs/13` §13.0).
    pub resume_flag: Option<String>,
}

/// Where the prompt goes in the spawned command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    /// Appended as the final positional argument.
    Arg,
    /// Piped to the child's stdin.
    Stdin,
}

/// A spawn-per-run drive.
pub struct SpawnDrive {
    cfg: SpawnConfig,
}

impl SpawnDrive {
    pub fn new(cfg: SpawnConfig) -> Self {
        Self { cfg }
    }

    /// A `codex exec <prompt>` drive (the spawn *fallback*; Codex's real drive is the
    /// app-server — see `AppServerDrive`).
    pub fn codex() -> Self {
        Self::new(SpawnConfig {
            program: "codex".into(),
            args: vec!["exec".into()],
            content: ContentMode::Arg,
            interrupt: true,
            resume_flag: None,
        })
    }

    /// An `agy --print <prompt>` drive with `--conversation <id>` resume — the best a
    /// no-daemon tool can do (`docs/13` §13.0).
    pub fn agy() -> Self {
        Self::new(SpawnConfig {
            program: "agy".into(),
            args: vec!["--print".into()],
            content: ContentMode::Arg,
            interrupt: true,
            resume_flag: Some("--conversation".into()),
        })
    }

    /// The exact command this drive would run for `content`, resuming `resume` if the
    /// config declares a resume flag (§13.7 asserts this).
    pub fn spec(&self, content: &str, cwd: &Path, resume: Option<&str>) -> SpawnSpec {
        let mut args = self.cfg.args.clone();
        // Resume an existing conversation when both a flag and a session id are present.
        if let (Some(flag), Some(id)) = (&self.cfg.resume_flag, resume) {
            args.push(flag.clone());
            args.push(id.to_string());
        }
        let stdin = match self.cfg.content {
            ContentMode::Arg => {
                args.push(content.to_string());
                None
            }
            ContentMode::Stdin => Some(content.to_string()),
        };
        SpawnSpec {
            program: self.cfg.program.clone(),
            args,
            cwd: cwd.to_path_buf(),
            stdin,
            env: Vec::new(),
        }
    }
}

impl Drive for SpawnDrive {
    fn info(&self) -> DriveInfo {
        DriveInfo {
            transport: Transport::Spawn,
        }
    }

    fn supports_interrupt(&self) -> bool {
        self.cfg.interrupt
    }

    fn run(
        &self,
        target: &Target,
        content: &str,
        ctx: &DriveCtx,
    ) -> Result<Box<dyn TurnHandle>, DriveError> {
        let mut spec = self.spec(content, &ctx.cwd, target.0.as_deref());
        // Thread the host's env (HS2-103 safety PATH shim + any --env pairs) into the
        // spawned process — the stream transports get theirs at spawn time (HS2-0TWTZ4).
        spec.env = ctx.env.clone();
        let proc = ctx
            .spawner
            .spawn(&spec)
            .map_err(|source| DriveError::Spawn {
                program: spec.program.clone(),
                source,
            })?;
        Ok(Box::new(SpawnTurnHandle {
            proc,
            can_interrupt: self.cfg.interrupt,
            done: None,
        }))
    }
}

/// Observes one spawned turn: busy = the process is alive; done = its exit code.
struct SpawnTurnHandle {
    proc: Box<dyn SpawnedProcess>,
    can_interrupt: bool,
    done: Option<DoneReason>,
}

impl TurnHandle for SpawnTurnHandle {
    fn is_busy(&mut self) -> bool {
        self.done.is_none() && self.proc.is_running()
    }

    fn wait(&mut self) -> DoneReason {
        if let Some(d) = self.done {
            return d;
        }
        let reason = match self.proc.wait() {
            0 => DoneReason::Completed,
            code => DoneReason::Failed(code),
        };
        self.done = Some(reason);
        reason
    }

    fn interrupt(&mut self) -> bool {
        if self.can_interrupt && self.done.is_none() {
            self.proc.kill();
            self.done = Some(DoneReason::Interrupted);
            true
        } else {
            false
        }
    }
}
