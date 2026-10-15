//! Shared child-process stdio plumbing for the **stream transports** (codex app-server,
//! claude channel): spawn a process with piped stdio and split it into newline-delimited
//! [`RpcWriter`]/[`RpcReader`] halves. The reader shares the [`Child`] so the process stays
//! alive for the connection's lifetime, and a [`ProcessSignals`] handle can stop it — the
//! Stop action and turn timeout of a per-turn Claude process (HS2-V0SWSM).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hotsheet_sync::LockExt;

use crate::ports::{ProcessSignals, RpcReader, RpcWriter};

/// How a [`stop_process`] call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// The process had already exited; nothing was signalled.
    AlreadyExited,
    /// It exited within the grace period after the polite request (SIGTERM).
    Terminated,
    /// It outlived the grace period and was force-killed (SIGKILL).
    Killed,
}

/// How often [`stop_process`] re-checks a signalled process (a local wait, no I/O).
const STOP_POLL: Duration = Duration::from_millis(20);

/// Stop a process: ask politely (SIGTERM), wait up to `grace` for it to exit, then force it
/// (SIGKILL). Blocks the calling thread for at most about `grace` (HS2-V0SWSM).
pub fn stop_process(signals: &mut dyn ProcessSignals, grace: Duration) -> StopOutcome {
    if signals.has_exited() {
        return StopOutcome::AlreadyExited;
    }
    signals.terminate();
    let deadline = Instant::now() + grace;
    loop {
        if signals.has_exited() {
            return StopOutcome::Terminated;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            signals.kill();
            return StopOutcome::Killed;
        }
        std::thread::sleep(remaining.min(STOP_POLL));
    }
}

/// A spawned tool process whose stdio carries newline-delimited JSON.
pub(crate) struct StreamChild {
    /// Shared with the reader half and any [`ProcessSignals`] handle, so the process can be
    /// signalled after the split (HS2-V0SWSM).
    child: Arc<Mutex<Child>>,
    pid: u32,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl StreamChild {
    /// Spawn `program args…` in `cwd` (with extra env), piping stdin/stdout for JSON.
    pub(crate) fn spawn(
        program: &str,
        args: &[&str],
        cwd: &Path,
        envs: &[(String, String)],
    ) -> std::io::Result<Self> {
        Self::spawn_with(program, args, cwd, envs, false)
    }

    /// [`StreamChild::spawn`], optionally leading its own process group so a short-lived
    /// probe (and anything it starts) can be killed as one (HS2-BJ7A59).
    pub(crate) fn spawn_with(
        program: &str,
        args: &[&str],
        cwd: &Path,
        envs: &[(String, String)],
        own_group: bool,
    ) -> std::io::Result<Self> {
        let mut cmd = hotsheet_ticketing::git::launch(program);
        #[cfg(unix)]
        if own_group {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[cfg(not(unix))]
        let _ = own_group;
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        Ok(Self {
            pid: child.id(),
            child: Arc::new(Mutex::new(child)),
            stdin,
            stdout,
        })
    }

    /// The child's process id.
    pub(crate) fn id(&self) -> u32 {
        self.pid
    }

    /// A handle that can signal this process; it stays valid after [`Self::into_halves`].
    pub(crate) fn signals(&self) -> Box<dyn ProcessSignals> {
        Box::new(ChildSignals {
            child: self.child.clone(),
        })
    }

    /// Split into the write half and the read half (the latter keeps the [`Child`] alive).
    pub(crate) fn into_halves(self) -> (Box<dyn RpcWriter>, Box<dyn RpcReader>) {
        (
            Box::new(PipeWriter {
                stdin: Some(self.stdin),
            }),
            Box::new(PipeReader {
                lines: BufReader::new(self.stdout),
                _child: self.child,
            }),
        )
    }
}

/// [`ProcessSignals`] over a spawned [`Child`]. Exit is observed with `try_wait`, which
/// reaps the child, so a later signal can never reach a recycled pid.
struct ChildSignals {
    child: Arc<Mutex<Child>>,
}

impl ProcessSignals for ChildSignals {
    fn terminate(&mut self) {
        let mut child = self.child.lock_or_recover();
        if matches!(child.try_wait(), Ok(None)) {
            #[cfg(unix)]
            // SAFETY: the pid is our own child, still unreaped (checked under the lock).
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
        }
    }

    fn kill(&mut self) {
        let mut child = self.child.lock_or_recover();
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn has_exited(&mut self) -> bool {
        !matches!(self.child.lock_or_recover().try_wait(), Ok(None))
    }
}

struct PipeWriter {
    stdin: Option<ChildStdin>,
}
impl RpcWriter for PipeWriter {
    fn send(&mut self, msg: &str) -> std::io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "RPC closed"))?;
        stdin.write_all(msg.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()
    }

    fn close(&mut self) {
        self.stdin.take();
    }
}

struct PipeReader {
    lines: BufReader<ChildStdout>,
    _child: Arc<Mutex<Child>>,
}
impl RpcReader for PipeReader {
    fn recv(&mut self) -> std::io::Result<Option<String>> {
        let mut line = String::new();
        match self.lines.read_line(&mut line)? {
            0 => Ok(None), // EOF
            _ => Ok(Some(line.trim_end().to_string())),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn shell(script: &str) -> StreamChild {
        StreamChild::spawn("sh", &["-c", script], Path::new("."), &[]).expect("spawn sh")
    }

    #[test]
    fn stop_process_terminates_a_cooperative_child_and_closes_its_stream() {
        let child = shell("echo ready; while :; do sleep 0.05; done");
        let mut signals = child.signals();
        let (_writer, mut reader) = child.into_halves();
        assert_eq!(reader.recv().unwrap().as_deref(), Some("ready"));
        assert!(!signals.has_exited());
        assert_eq!(
            stop_process(signals.as_mut(), Duration::from_secs(5)),
            StopOutcome::Terminated
        );
        assert!(signals.has_exited());
        assert_eq!(
            stop_process(signals.as_mut(), Duration::from_secs(5)),
            StopOutcome::AlreadyExited,
            "a repeated stop signals nothing"
        );
    }

    #[test]
    fn stop_process_kills_a_child_that_ignores_sigterm_after_the_grace_period() {
        let child = shell("trap '' TERM; echo ready; while :; do sleep 0.05; done");
        let mut signals = child.signals();
        let (_writer, mut reader) = child.into_halves();
        assert_eq!(reader.recv().unwrap().as_deref(), Some("ready"));
        let started = Instant::now();
        assert_eq!(
            stop_process(signals.as_mut(), Duration::from_millis(300)),
            StopOutcome::Killed
        );
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert!(signals.has_exited());
    }

    #[test]
    fn stop_process_reports_an_already_exited_child() {
        let child = shell("exit 0");
        let mut signals = child.signals();
        let (_writer, mut reader) = child.into_halves();
        assert_eq!(reader.recv().unwrap(), None);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !signals.has_exited() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            stop_process(signals.as_mut(), Duration::from_secs(1)),
            StopOutcome::AlreadyExited
        );
        // Signalling an exited (reaped) child is a no-op.
        signals.terminate();
        signals.kill();
    }
}
