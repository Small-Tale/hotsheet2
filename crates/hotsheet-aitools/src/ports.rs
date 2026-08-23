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
    /// The most recently opened thread id (for cross-turn resume, HS2-3C1XK3). `None`
    /// until a thread has been opened.
    fn session_id(&self) -> Option<String> {
        None
    }
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
    /// Token usage the turn reported on `turn/completed`, if any (`docs/14`, HS2-0WCRZY).
    /// Default `None` — a fake/non-reporting turn contributes no metrics.
    fn usage(&mut self) -> Option<crate::drive::Usage> {
        None
    }
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

// ---- the JSON-RPC byte transport under the real client ---------------------------

/// A newline-delimited JSON-RPC **duplex** to a codex app-server. This is the lowest
/// seam: the real client (`crate::codex`) speaks the `initialize`/`thread/*`/`turn/*`
/// protocol over it, while tests inject a **scripted daemon** so the entire protocol
/// engine is exercised with no live `codex` (`docs/05` §5.10). The real transport bridges
/// `codex app-server proxy` stdio to the running daemon's control socket.
///
/// It splits into a [`RpcWriter`] the client sends requests on and a [`RpcReader`] a
/// background thread drains for responses + notifications.
pub trait RpcTransport {
    fn split(self: Box<Self>) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>);
}

/// The write half of an [`RpcTransport`] — sends one JSON message at a time.
pub trait RpcWriter: Send {
    /// Send one JSON-RPC message. The transport frames it (appends the newline).
    fn send(&mut self, msg: &str) -> std::io::Result<()>;
}

/// The read half of an [`RpcTransport`] — yields one JSON message at a time.
pub trait RpcReader: Send {
    /// Block for the next message; `Ok(None)` means the peer closed the connection.
    fn recv(&mut self) -> std::io::Result<Option<String>>;
}
