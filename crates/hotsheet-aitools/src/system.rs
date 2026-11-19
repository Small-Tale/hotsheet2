//! The real [`ProcessSpawner`] over `std::process` — the host's production adapter.

use hotsheet_process_journal::{ExitReason, LaunchMeta};
use std::io::{Read, Write};
use std::process::{Child, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::ports::{ProcessSpawner, SpawnSpec, SpawnedProcess};
use crate::procio::ProcessContext;

/// Spawns real OS processes.
pub struct SystemSpawner;

/// Production spawner carrying the session identity of a driven child (HS2-GDCMXE).
pub(crate) struct TrackedSystemSpawner(pub Option<ProcessContext>);

impl ProcessSpawner for SystemSpawner {
    fn spawn(&self, spec: &SpawnSpec) -> std::io::Result<Box<dyn SpawnedProcess>> {
        spawn(spec, None)
    }
}

impl ProcessSpawner for TrackedSystemSpawner {
    fn spawn(&self, spec: &SpawnSpec) -> std::io::Result<Box<dyn SpawnedProcess>> {
        spawn(spec, self.0.clone())
    }
}

fn spawn(
    spec: &SpawnSpec,
    context: Option<ProcessContext>,
) -> std::io::Result<Box<dyn SpawnedProcess>> {
    // HS2-E5B0K0: resolved for the child's own PATH and directory (posix_spawn, no fork).
    let search = hotsheet_ticketing::git::child_path(spec.env.iter().map(|(k, v)| (k, v)));
    let mut cmd = hotsheet_ticketing::git::launch_resolved(
        &spec.program,
        search.as_deref(),
        Some(spec.cwd.as_ref()),
    );
    cmd.args(&spec.args).current_dir(&spec.cwd);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if spec.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    if spec.stderr_tail {
        cmd.stderr(Stdio::piped());
    }
    let mut child = cmd.spawn()?;
    let stderr = child.stderr.take().map(StderrTail::start);
    let record = if let Some(context) = context {
        let record = context.journal.start(LaunchMeta {
            pid: child.id(),
            supervisor_pid: std::process::id(),
            supervisor: "hotsheet".into(),
            purpose: context.purpose,
            project_id: Some(context.project_id),
            session_id: Some(context.session_id),
            binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
        });
        match record {
            Ok(record) => Some((context.journal, record.launch_id)),
            Err(error) => {
                eprintln!(
                    "Hot Sheet drive process journal unavailable: {}",
                    hotsheet_process_journal::start_error_summary(&error)
                );
                None
            }
        }
    } else {
        None
    };
    let mut process = SystemProcess {
        child,
        record,
        stderr,
    };
    if let (Some(data), Some(mut si)) = (&spec.stdin, process.child.stdin.take()) {
        if let Err(error) = si.write_all(data.as_bytes()) {
            process.kill();
            return Err(error);
        }
    }
    Ok(Box::new(process))
}

struct SystemProcess {
    child: Child,
    record: Option<(hotsheet_process_journal::ProcessJournal, String)>,
    stderr: Option<StderrTail>,
}

/// The most of a child's stderr a [`StderrTail`] keeps (HS2-86MK1Y): enough for a
/// structured failure line after any amount of earlier output.
const STDERR_TAIL_BYTES: usize = 64 * 1024;

/// How long a finished child's stderr may stay open (a descendant still holding it)
/// before the tail is read as it stands.
const STDERR_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Passes a child's stderr through to the host's stderr and keeps its last
/// [`STDERR_TAIL_BYTES`] for a structured failure line (HS2-86MK1Y).
struct StderrTail {
    kept: Arc<Mutex<Vec<u8>>>,
    closed: Receiver<()>,
}

impl StderrTail {
    fn start(mut pipe: std::process::ChildStderr) -> Self {
        let kept = Arc::new(Mutex::new(Vec::new()));
        let (done, closed) = std::sync::mpsc::channel();
        let sink = Arc::clone(&kept);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8 * 1024];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let _ = std::io::stderr().write_all(&chunk[..read]);
                        let mut kept = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        kept.extend_from_slice(&chunk[..read]);
                        let excess = kept.len().saturating_sub(STDERR_TAIL_BYTES);
                        kept.drain(..excess);
                    }
                }
            }
            let _ = done.send(());
        });
        Self { kept, closed }
    }

    /// The kept tail once the stream closed, or as it stands after the drain grace.
    fn read(&self) -> String {
        match self.closed.recv_timeout(STDERR_DRAIN_GRACE) {
            Ok(()) | Err(RecvTimeoutError::Disconnected | RecvTimeoutError::Timeout) => {}
        }
        let kept = self
            .kept
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        String::from_utf8_lossy(&kept).into_owned()
    }
}

impl SystemProcess {
    fn record_exit(&mut self, reason: ExitReason) {
        if let Some((journal, launch_id)) = self.record.take() {
            let _ = journal.finish(&launch_id, reason);
        }
    }
}

impl SpawnedProcess for SystemProcess {
    fn is_running(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                self.record_exit(if status.success() {
                    ExitReason::Completed
                } else {
                    ExitReason::Failed
                });
                false
            }
            Err(_) => false,
        }
    }

    fn wait(&mut self) -> i32 {
        match self.child.wait() {
            Ok(status) => {
                self.record_exit(if status.success() {
                    ExitReason::Completed
                } else {
                    ExitReason::Failed
                });
                status.code().unwrap_or(-1)
            }
            Err(_) => -1,
        }
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.record_exit(ExitReason::Killed);
    }

    fn stderr_tail(&mut self) -> Option<String> {
        self.stderr.as_ref().map(StderrTail::read)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use hotsheet_process_journal::{ProcessJournal, Purpose};

    fn context(home: &std::path::Path) -> ProcessContext {
        ProcessContext {
            journal: ProcessJournal::at(home),
            project_id: "project-1".into(),
            session_id: "session-1".into(),
            purpose: Purpose::AiSpawn,
        }
    }

    fn spec(script: &str, home: &std::path::Path) -> SpawnSpec {
        SpawnSpec {
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: home.to_path_buf(),
            stdin: None,
            env: Vec::new(),
            stderr_tail: false,
        }
    }

    #[test]
    fn hs2_gdcmxe_spawn_drive_records_normal_and_killed_exits() {
        let home = tempfile::tempdir().unwrap();
        let spawner = TrackedSystemSpawner(Some(context(home.path())));
        let mut completed = spawner.spawn(&spec("exit 0", home.path())).unwrap();
        assert_eq!(completed.wait(), 0);
        let mut killed = spawner.spawn(&spec("sleep 30", home.path())).unwrap();
        killed.kill();
        let records = ProcessJournal::at(home.path()).list().unwrap();
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .all(|record| record.session_id.as_deref() == Some("session-1"))
        );
        assert!(
            records
                .iter()
                .any(|record| record.exit_reason == Some(ExitReason::Completed))
        );
        assert!(
            records
                .iter()
                .any(|record| record.exit_reason == Some(ExitReason::Killed))
        );
    }
}
