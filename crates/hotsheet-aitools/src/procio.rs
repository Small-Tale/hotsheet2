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
            // A process that ignored SIGTERM may leave tool subprocesses behind; take its whole
            // descendant tree down with it (HS2-M08WMC). The child keeps sharing our process
            // group, so a foreground CLI's terminal Ctrl-C still reaches it.
            #[cfg(unix)]
            kill_descendants(child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn has_exited(&mut self) -> bool {
        !matches!(self.child.lock_or_recover().try_wait(), Ok(None))
    }
}

/// Every live `(pid, parent pid)` pair, from `ps` (macOS and Linux alike).
#[cfg(unix)]
fn process_table() -> Vec<(u32, u32)> {
    let Ok(output) = hotsheet_ticketing::git::launch("ps")
        .args(["-A", "-o", "pid=", "-o", "ppid="])
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect()
}

/// The descendants of `root` in `table`, parents before children.
#[cfg_attr(not(unix), allow(dead_code))]
fn descendants_of(root: u32, table: &[(u32, u32)]) -> Vec<u32> {
    let mut found = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for &(pid, ppid) in table {
            if ppid == parent && pid != root && !found.contains(&pid) {
                found.push(pid);
                frontier.push(pid);
            }
        }
    }
    found
}

/// Force-stop every descendant of the still-running, unreaped `root` (HS2-M08WMC). Each
/// discovered process is frozen with SIGSTOP so it cannot start more, and the tree is walked
/// again until no new process appears; then every frozen process gets SIGKILL. While `root`
/// is alive the tree stays parented to it, so no pid here has been reparented or recycled.
#[cfg(unix)]
fn kill_descendants(root: u32) {
    let mut frozen: Vec<u32> = Vec::new();
    for _ in 0..8 {
        let fresh: Vec<u32> = descendants_of(root, &process_table())
            .into_iter()
            .filter(|pid| !frozen.contains(pid))
            .collect();
        if fresh.is_empty() {
            break;
        }
        for &pid in &fresh {
            // SAFETY: `pid` was a live descendant of our unreaped child a moment ago.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGSTOP);
            }
        }
        frozen.extend(fresh);
    }
    for &pid in frozen.iter().rev() {
        // SAFETY: as above; SIGKILL also ends a stopped process.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
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
    fn descendants_of_walks_the_whole_tree_and_ignores_other_processes() {
        let table = [
            (10, 1),
            (11, 10),
            (12, 11),
            (13, 10),
            (20, 1),
            (21, 20),
            (14, 13),
        ];
        let mut found = descendants_of(10, &table);
        found.sort_unstable();
        assert_eq!(found, vec![11, 12, 13, 14]);
        assert!(descendants_of(12, &table).is_empty());
        assert!(descendants_of(99, &table).is_empty());
    }

    #[cfg(unix)]
    fn alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks that the pid exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    /// HS2-M08WMC: a stand-in `claude` that ignores SIGTERM and has started a tool process
    /// (itself with a child) loses its whole tree when it is force-killed, not just itself.
    #[cfg(unix)]
    #[test]
    fn a_forced_stop_also_kills_the_tool_processes_the_child_started() {
        let child = shell(
            "trap '' TERM; sh -c 'sleep 300 & echo $!; trap \"\" TERM; while :; do sleep 0.05; done' & \
             echo $!; while :; do sleep 0.05; done",
        );
        let mut signals = child.signals();
        let (_writer, mut reader) = child.into_halves();
        let mut pids: Vec<i32> = (0..2)
            .map(|_| reader.recv().unwrap().unwrap().trim().parse().unwrap())
            .collect();
        pids.sort_unstable();
        assert!(
            pids.iter().all(|pid| alive(*pid)),
            "the tool tree is running"
        );
        assert_eq!(
            stop_process(signals.as_mut(), Duration::from_millis(200)),
            StopOutcome::Killed
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while pids.iter().any(|pid| alive(*pid)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            pids.iter().all(|pid| !alive(*pid)),
            "orphaned tool processes survived the forced stop: {pids:?}"
        );
    }

    /// A cooperative child that exits on SIGTERM is never escalated, so its tree is left to it.
    #[cfg(unix)]
    #[test]
    fn a_cooperative_stop_leaves_cleanup_to_the_child() {
        // The PID line is the readiness signal: install the trap first or the parent can send
        // SIGTERM in the gap and leave the sleeper behind (HS2-A179GH).
        let child = shell(
            "sleep 300 & trap 'kill $!; wait $!; exit 0' TERM; echo $!; while :; do sleep 0.05; done",
        );
        let mut signals = child.signals();
        let (_writer, mut reader) = child.into_halves();
        let sleeper: i32 = reader.recv().unwrap().unwrap().trim().parse().unwrap();
        assert_eq!(
            stop_process(signals.as_mut(), Duration::from_secs(5)),
            StopOutcome::Terminated
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(sleeper) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(sleeper), "the child cleaned up its own tool process");
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
