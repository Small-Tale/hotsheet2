//! Injected adapters the drive reaches the outside world through — so every drive is
//! testable against a **fake**, never a real process (`docs/05` §5.10, the load-bearing
//! testability rule). The real implementations live in [`crate::system`]; tests inject
//! fakes.

use std::path::{Path, PathBuf};

/// How to launch one process. Content-agnostic — the drive fills it in from the turn's
/// prompt (`docs/13` §13.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Data piped to the child's stdin (e.g. the prompt), if the tool reads it there.
    pub stdin: Option<String>,
    pub env: Vec<(String, String)>,
}

/// A running child process the drive observes. Injected so a spawn drive can be tested
/// without launching anything real.
pub trait SpawnedProcess {
    /// Whether the process is still running (non-blocking).
    fn is_running(&mut self) -> bool;
    /// Block until it exits; return the exit code (`-1` if it was signalled/unknown).
    fn wait(&mut self) -> i32;
    /// Best-effort terminate (backs the `interrupt` capability).
    fn kill(&mut self);
}

/// Spawns processes. The real impl uses `std::process`; tests inject a fake that records
/// the [`SpawnSpec`] and scripts the child's lifecycle.
pub trait ProcessSpawner {
    fn spawn(&self, spec: &SpawnSpec) -> std::io::Result<Box<dyn SpawnedProcess>>;
}

// ---- codex app-server (persistent daemon) ----------------------------------------

/// A connection to a running **codex app-server** daemon (JSON-RPC over its control
/// socket; `codex 0.148 app-server daemon`). Injected so the [`AppServerDrive`] is
/// testable without a live daemon. `open_thread` maps to `thread/start` (new) or
/// `thread/resume`; `start_turn` maps to `turn/start`.
///
/// [`AppServerDrive`]: crate::AppServerDrive
pub trait AppServerClient {
    /// Start a new thread (`resume = None`) or resume an existing one; returns the
    /// thread id.
    fn open_thread(&self, resume: Option<&str>, cwd: &Path) -> Result<String, AppServerError>;
    /// Send one turn (`turn/start`) with the prompt; returns a handle to observe it.
    fn start_turn(
        &self,
        thread_id: &str,
        content: &str,
    ) -> Result<Box<dyn AppServerTurn>, AppServerError>;
}

/// A turn running on the app-server — observed via its `turn/started` → `turn/completed`
/// notifications.
pub trait AppServerTurn {
    /// Whether the turn is still in flight (no `turn/completed` yet).
    fn is_running(&mut self) -> bool;
    /// Block until `turn/completed`; return the outcome.
    fn wait(&mut self) -> AppServerOutcome;
    /// `turn/interrupt` the running turn.
    fn interrupt(&mut self);
}

/// How an app-server turn finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppServerOutcome {
    Completed,
    Failed(String),
}

/// An app-server connection/protocol failure.
#[derive(Debug, thiserror::Error)]
pub enum AppServerError {
    #[error("codex app-server unavailable: {0}")]
    Unavailable(String),
    #[error("codex app-server protocol error: {0}")]
    Protocol(String),
}
