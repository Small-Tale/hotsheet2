//! The real [`ProcessSpawner`] over `std::process` — the host's production adapter.

use hotsheet_process_journal::{ExitReason, LaunchMeta};
use std::io::Write;
use std::process::{Child, Stdio};

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
    let mut cmd = hotsheet_ticketing::git::launch(&spec.program);
    cmd.args(&spec.args).current_dir(&spec.cwd);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if spec.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let child = cmd.spawn()?;
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
                    error.kind()
                );
                None
            }
        }
    } else {
        None
    };
    let mut process = SystemProcess { child, record };
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
