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
//!
//! HS2-BR14FM: the same holds after a plugin exits normally. Its output is drained for at
//! most [`DRAIN_GRACE`] after the exit, then whatever it left in its tree is ended and the
//! run returns with what the plugin printed. On Unix the exit is observed without reaping
//! the plugin (`waitid` with `WNOWAIT`), so its group id stays its own until `killpg`.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::cancel::WaitCancel;

/// What a finished plugin process left behind.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// HS2-BR14FM: the plugin exited but something it started still held its output
    /// open after [`DRAIN_GRACE`]; that process tree was ended.
    pub left_running: bool,
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
    let (closed, drained) = std::sync::mpsc::channel();
    let stdout = read_stream(child.stdout.take(), closed.clone());
    let stderr = read_stream(child.stderr.take(), closed);
    let started = Instant::now();
    loop {
        match tree.exited(&mut child) {
            Ok(true) => break,
            // HS2-P234BH: checked on the same local child-exit loop, so a cancel stops
            // the process within one step and adds no wake-up of its own.
            Ok(false) if cancelled() => {
                tree.stop(&mut child);
                let _ = child.wait();
                return Err(RunError::Cancelled {
                    program: program.to_owned(),
                });
            }
            Ok(false) if started.elapsed() >= timeout => {
                tree.stop(&mut child);
                let _ = child.wait();
                return Err(RunError::Timeout {
                    program: program.to_owned(),
                    seconds: timeout.as_secs(),
                });
            }
            // A local child-process wait; no network request is issued here.
            Ok(false) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                tree.stop(&mut child);
                let _ = child.wait();
                return Err(RunError::Wait {
                    program: program.to_owned(),
                    message: e.to_string(),
                });
            }
        }
    }
    // HS2-BR14FM: the plugin has exited (on Unix it is not reaped yet). Its output closes
    // with it unless something it started still holds a pipe; that is waited for only
    // `DRAIN_GRACE`, then the rest of its tree is ended the way a stop ends it.
    let open = await_closed(&drained, 2, DRAIN_GRACE);
    tree.end_leftovers(&child);
    let status = child.wait().map_err(|e| RunError::Wait {
        program: program.to_owned(),
        message: e.to_string(),
    })?;
    if open > 0 {
        // Ending the tree closes the pipes; a process that left the tree is not waited
        // for beyond this, and what was read so far is the output.
        await_closed(&drained, open, DRAIN_GRACE);
    }
    Ok(ProcessOutput {
        success: status.success(),
        code: status.code(),
        stdout: take(&stdout),
        stderr: take(&stderr),
        left_running: open > 0,
    })
}

/// HS2-BR14FM: how long a finished plugin's output may stay open before what holds it
/// is taken for a process the plugin left running.
const DRAIN_GRACE: Duration = Duration::from_secs(1);

/// Read `pipe` to its end on a thread of its own, into a buffer the caller can take at
/// any time; `closed` is told when the stream ends.
fn read_stream<R: Read + Send + 'static>(
    pipe: Option<R>,
    closed: std::sync::mpsc::Sender<()>,
) -> Arc<Mutex<Vec<u8>>> {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let filled = buffer.clone();
    std::thread::spawn(move || {
        if let Some(mut pipe) = pipe {
            let mut chunk = [0_u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => filled
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .extend_from_slice(&chunk[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        let _ = closed.send(());
    });
    buffer
}

/// Wait up to `grace` for `open` streams to end; how many are still open.
fn await_closed(drained: &Receiver<()>, mut open: usize, grace: Duration) -> usize {
    let deadline = Instant::now() + grace;
    while open > 0 {
        match drained.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(()) => open -= 1,
            Err(_) => break,
        }
    }
    open
}

fn take(buffer: &Mutex<Vec<u8>>) -> Vec<u8> {
    std::mem::take(&mut *buffer.lock().unwrap_or_else(PoisonError::into_inner))
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

    /// Whether the root has exited. On Unix it is left unreaped, so its process group id
    /// still names only its own group when [`Self::end_leftovers`] runs.
    #[cfg(unix)]
    fn exited(&self, child: &mut std::process::Child) -> std::io::Result<bool> {
        let id = libc::id_t::from(child.id());
        loop {
            // SAFETY: an all-zero `siginfo_t` is a valid value of this plain C struct.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: `info` is a live, writable `siginfo_t`; `WNOWAIT` leaves the child
            // waitable, so `Child::wait` still reaps it.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    id,
                    &raw mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                // With `WNOHANG` and nothing to report, the zeroed record stays zeroed.
                return Ok(info.si_signo == libc::SIGCHLD);
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    #[cfg(not(unix))]
    fn exited(&self, child: &mut std::process::Child) -> std::io::Result<bool> {
        child.try_wait().map(|status| status.is_some())
    }

    /// HS2-BR14FM: end what the exited root left in its tree. On Unix the root is not
    /// reaped yet, so the group id is still its own.
    fn end_leftovers(&self, child: &std::process::Child) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        #[cfg(unix)]
        kill_group(child);
        #[cfg(not(unix))]
        let _ = child;
    }

    /// Stop the root and everything it started. The root is not reaped yet, so its
    /// group id cannot name another group.
    fn stop(&self, child: &mut std::process::Child) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        #[cfg(unix)]
        kill_group(child);
        let _ = child.kill();
    }
}

/// Kill the process group `child` leads, made by `process_group(0)` at spawn. The caller
/// has not reaped `child`, so the group id cannot name another group.
#[cfg(unix)]
fn kill_group(child: &std::process::Child) {
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        // SAFETY: `killpg` takes plain integers and touches no memory.
        unsafe {
            libc::killpg(group, libc::SIGKILL);
        }
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
