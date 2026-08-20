//! The real [`ProcessSpawner`] over `std::process` — the host's production adapter.

use std::io::Write;
use std::process::{Child, Command, Stdio};

use crate::ports::{ProcessSpawner, SpawnSpec, SpawnedProcess};

/// Spawns real OS processes.
pub struct SystemSpawner;

impl ProcessSpawner for SystemSpawner {
    fn spawn(&self, spec: &SpawnSpec) -> std::io::Result<Box<dyn SpawnedProcess>> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args).current_dir(&spec.cwd);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        if spec.stdin.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn()?;
        if let (Some(data), Some(mut si)) = (&spec.stdin, child.stdin.take()) {
            si.write_all(data.as_bytes())?; // dropping `si` closes stdin (EOF)
        }
        Ok(Box::new(SystemProcess { child }))
    }
}

struct SystemProcess {
    child: Child,
}

impl SpawnedProcess for SystemProcess {
    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn wait(&mut self) -> i32 {
        self.child.wait().ok().and_then(|s| s.code()).unwrap_or(-1)
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
