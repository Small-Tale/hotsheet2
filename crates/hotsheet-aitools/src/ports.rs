//! Injected adapters the drive reaches the outside world through — so every drive is
//! testable against a **fake**, never a real process (`docs/05` §5.10, the load-bearing
//! testability rule). The real implementations live in [`crate::system`]; tests inject
//! fakes.

use std::path::PathBuf;

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
