//! Runs a plugin command once (HS2-322ZSZ): the envelope on stdin, one result line on
//! stdout, killed when it outlives its timeout. No shell is involved.
//!
//! HS2-P234BH: a [`WaitCancel`] ends a run early the same way a timeout does: the process is
//! killed and reaped, and nothing it printed is read.
//!
//! HS2-CYDK9Z: a plugin runs as its own process tree, so a stop ends everything it
//! started too: on Unix in its own process group, ended with `killpg`; on Windows in a
//! kill-on-close job object (the one validation steps use, HS2-2R32SD). A plugin that
//! starts a child of its own (a shell script that does not `exec`) therefore leaves
//! nothing running after a timeout or a cancel, and nothing holding its output pipes.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::cancel::WaitCancel;

/// What a finished plugin process left behind.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    #[error("could not start '{program}': {message}")]
    Spawn { program: String, message: String },
    #[error("'{program}' did not finish within {seconds}s and was stopped")]
    Timeout { program: String, seconds: u64 },
    #[error("waiting for '{program}': {message}")]
    Wait { program: String, message: String },
    /// HS2-P234BH: the caller's cancel ended the run; the process was stopped.
    #[error("'{program}' was stopped because the wait was cancelled")]
    Cancelled { program: String },
}

/// Injected so unit tests never start a process.
pub trait CommandRunner: Send + Sync {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: &[u8],
        timeout: Duration,
    ) -> Result<ProcessOutput, RunError>;

    /// HS2-P234BH: [`Self::run`] that `cancel` ends early with [`RunError::Cancelled`]. The
    /// default honours only a cancel made before the run starts.
    fn run_cancellable(
        &self,
        program: &str,
        args: &[String],
        stdin: &[u8],
        timeout: Duration,
        cancel: &WaitCancel,
    ) -> Result<ProcessOutput, RunError> {
        if cancel.is_cancelled() {
            return Err(RunError::Cancelled {
                program: program.to_owned(),
            });
        }
        self.run(program, args, stdin, timeout)
    }
}

/// Spawns real processes.
#[derive(Debug, Default, Clone, Copy)]
pub struct LiveRunner;

impl CommandRunner for LiveRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: &[u8],
        timeout: Duration,
    ) -> Result<ProcessOutput, RunError> {
        run_process(program, args, stdin, timeout, None)
    }

    fn run_cancellable(
        &self,
        program: &str,
        args: &[String],
        stdin: &[u8],
        timeout: Duration,
        cancel: &WaitCancel,
    ) -> Result<ProcessOutput, RunError> {
        run_process(program, args, stdin, timeout, Some(cancel))
    }
}

/// Run one process; `cancel` stops it like a timeout does (HS2-P234BH).
fn run_process(
    program: &str,
    args: &[String],
    stdin: &[u8],
    timeout: Duration,
    cancel: Option<&WaitCancel>,
) -> Result<ProcessOutput, RunError> {
    let cancelled = || cancel.is_some_and(WaitCancel::is_cancelled);
    if cancelled() {
        return Err(RunError::Cancelled {
            program: program.to_owned(),
        });
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let tree = ProcessTree::prepare(&mut command);
    let mut child = command.spawn().map_err(|e| RunError::Spawn {
        program: program.to_owned(),
        message: e.to_string(),
    })?;
    let tree = tree
        .started(&mut child)
        .map_err(|message| RunError::Spawn {
            program: program.to_owned(),
            message,
        })?;
    if let Some(mut pipe) = child.stdin.take() {
        // A plugin that exits without reading stdin closes the pipe; its exit status
        // and output then explain the failure, so a write error is not reported here.
        let _ = pipe.write_all(stdin);
    }
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            // HS2-P234BH: checked on the same local child-exit loop, so a cancel stops
            // the process within one step and adds no wake-up of its own.
            Ok(None) if cancelled() => {
                tree.stop(&mut child);
                let _ = child.wait();
                return Err(RunError::Cancelled {
                    program: program.to_owned(),
                });
            }
            Ok(None) if started.elapsed() >= timeout => {
                tree.stop(&mut child);
                let _ = child.wait();
                return Err(RunError::Timeout {
                    program: program.to_owned(),
                    seconds: timeout.as_secs(),
                });
            }
            // A local child-process wait; no network request is issued here.
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                return Err(RunError::Wait {
                    program: program.to_owned(),
                    message: e.to_string(),
                });
            }
        }
    };
    Ok(ProcessOutput {
        success: status.success(),
        code: status.code(),
        stdout: out_reader.join().unwrap_or_default(),
        stderr: err_reader.join().unwrap_or_default(),
    })
}

/// HS2-CYDK9Z: the process tree a plugin run owns: its own process group on Unix, a
/// kill-on-close job object on Windows when one can be made.
struct ProcessTree {
    #[cfg(windows)]
    job: Option<hotsheet_process_journal::tree_job::TreeJob>,
}

impl ProcessTree {
    /// Set `command` up to start as the root of its own tree.
    #[cfg(unix)]
    fn prepare(command: &mut Command) -> Self {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        Self {}
    }

    /// Set `command` up to start as the root of its own tree: suspended, so nothing it
    /// starts runs before it is in the job.
    #[cfg(windows)]
    fn prepare(command: &mut Command) -> Self {
        use hotsheet_process_journal::tree_job::TreeJob;
        use std::os::windows::process::CommandExt;
        let job = TreeJob::new();
        if job.is_some() {
            command.creation_flags(TreeJob::creation_flags());
        }
        Self { job }
    }

    #[cfg(not(any(unix, windows)))]
    fn prepare(_command: &mut Command) -> Self {
        Self {}
    }

    /// The spawned root. Nothing to do outside Windows.
    #[cfg(not(windows))]
    fn started(self, _child: &mut std::process::Child) -> Result<Self, String> {
        Ok(self)
    }

    /// The spawned root: put it in the job and let it run. A process that cannot be
    /// resumed is stopped and reported, since it would only reach its timeout.
    #[cfg(windows)]
    fn started(self, child: &mut std::process::Child) -> Result<Self, String> {
        let Some(job) = self.job else {
            return Ok(Self { job: None });
        };
        let held = job.assign(child);
        if !hotsheet_process_journal::tree_job::resume(child.id()) {
            job.terminate();
            let _ = child.kill();
            let _ = child.wait();
            return Err("the process could not be resumed".to_owned());
        }
        Ok(Self {
            job: held.then_some(job),
        })
    }

    /// Stop the root and everything it started. The root is not reaped yet, so its
    /// group id cannot name another group.
    fn stop(&self, child: &mut std::process::Child) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        #[cfg(unix)]
        if let Ok(group) = libc::pid_t::try_from(child.id()) {
            // SAFETY: `killpg` takes plain integers and touches no memory. The group is
            // the child's own, made by `process_group(0)` at spawn, and the child is
            // not reaped yet.
            unsafe {
                libc::killpg(group, libc::SIGKILL);
            }
        }
        let _ = child.kill();
    }
}

/// The last `max` bytes of a process stream as lossy text, for error messages.
pub(crate) fn tail(bytes: &[u8], max: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    if text.len() <= max {
        return text.to_owned();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}
