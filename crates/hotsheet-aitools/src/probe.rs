//! Bounded capability probes for installed AI tools (HS2-BH3M53).
//!
//! AI-tool discovery asks every detected drivable tool for its version and, where the
//! plugin declares one, its model catalog. Those are third-party executables: one can hang
//! (an auth prompt, a network stall, a wedged daemon). A probe runs the tool in its own
//! process group with a deadline; on expiry the whole group is killed and the caller falls
//! back to the plugin manifest. Every in-flight probe is also tracked so a stopping server
//! can kill them all and refuse new ones ([`stop_probes`]) instead of orphaning them.

use std::collections::HashSet;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

/// Default deadline for one probe (a `--version` or catalog listing).
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

static PROBE_TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_PROBE_TIMEOUT.as_millis() as u64);

/// Process ids (each its own process-group leader) of probes still running.
static IN_FLIGHT: Mutex<Option<HashSet<u32>>> = Mutex::new(None);

/// Set once the process is stopping: no new probe starts, and one that raced the stop is
/// killed as soon as it registers.
static STOPPED: AtomicBool = AtomicBool::new(false);

/// The current per-probe deadline.
pub fn probe_timeout() -> Duration {
    Duration::from_millis(PROBE_TIMEOUT_MS.load(Ordering::Relaxed))
}

/// Replace the per-probe deadline for this process (the server's hidden test flag).
pub fn set_probe_timeout(timeout: Duration) {
    PROBE_TIMEOUT_MS.store(
        u64::try_from(timeout.as_millis())
            .unwrap_or(u64::MAX)
            .max(1),
        Ordering::Relaxed,
    );
}

/// Kill every probe still running, with its process group. A stopping server calls this so
/// no tool probe outlives it; the probes' callers then see a failed probe.
pub fn kill_in_flight_probes() -> usize {
    let pids: Vec<u32> = IN_FLIGHT
        .lock()
        .map(|guard| guard.iter().flatten().copied().collect())
        .unwrap_or_default();
    for pid in &pids {
        kill_group(*pid);
    }
    pids.len()
}

/// Stop probing for the rest of this process: kill every running probe and refuse new
/// ones (they fail at once, so discovery falls back to the manifests). A discovery scan
/// that is still walking its tools after a stop therefore cannot start another child that
/// the exiting process would orphan.
pub fn stop_probes() -> usize {
    {
        let _guard = IN_FLIGHT.lock();
        STOPPED.store(true, Ordering::SeqCst);
    }
    kill_in_flight_probes()
}

/// The number of probes currently running (diagnostics and tests).
pub fn in_flight_probes() -> usize {
    IN_FLIGHT
        .lock()
        .map(|guard| guard.as_ref().map_or(0, HashSet::len))
        .unwrap_or(0)
}

/// Run `command` to completion within [`probe_timeout`], capturing stdout and stderr.
/// stdin is closed. On timeout the child's whole process group is killed and an error
/// names the deadline.
pub fn run_probe(command: &mut Command) -> Result<Output, String> {
    run_probe_within(command, probe_timeout())
}

/// [`run_probe`] with an explicit deadline.
pub fn run_probe_within(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own group, so a timeout or a stop also reaches anything the tool spawned.
        command.process_group(0);
    }
    let program = command.get_program().to_string_lossy().into_owned();
    if STOPPED.load(Ordering::SeqCst) {
        return Err(format!("'{program}' not probed: the process is stopping"));
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("starting '{program}': {error}"))?;
    let pid = child.id();
    track(pid, true);
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let (done_tx, done_rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let status = child.wait();
        let _ = done_tx.send(());
        status
    });
    let timed_out = done_rx.recv_timeout(timeout).is_err();
    if timed_out {
        kill_group(pid);
    }
    let status = waiter
        .join()
        .map_err(|_| format!("'{program}' waiter panicked"))?;
    track(pid, false);
    let status = status.map_err(|error| format!("waiting for '{program}': {error}"))?;
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if timed_out {
        return Err(format!(
            "'{program}' did not answer within {} ms and was stopped",
            timeout.as_millis()
        ));
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buffer);
        }
        buffer
    })
}

fn track(pid: u32, running: bool) {
    if let Ok(mut guard) = IN_FLIGHT.lock() {
        let set = guard.get_or_insert_with(HashSet::new);
        if running {
            set.insert(pid);
            // Spawned after a stop began (the check above raced it): kill it now.
            if STOPPED.load(Ordering::SeqCst) {
                kill_group(pid);
            }
        } else {
            set.remove(&pid);
        }
    }
}

#[cfg(unix)]
fn kill_group(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    // SAFETY: `pid` leads the process group this module created for one probe; signalling
    // the negative id reaches that group only.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_group(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Instant;

    use super::*;

    fn sh(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn a_prompt_probe_returns_its_output() {
        let output = run_probe_within(&mut sh("echo 1.2.3"), Duration::from_secs(10)).unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1.2.3");
    }

    #[test]
    fn a_hanging_probe_is_killed_with_its_children_at_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");
        // The probe forks a grandchild that also hangs; the group kill must reach both.
        let script = format!("sleep 30 & echo $! > '{}'; wait", pid_file.display());
        let started = Instant::now();
        let error = run_probe_within(&mut sh(&script), Duration::from_millis(300)).unwrap_err();
        assert!(error.contains("did not answer within 300 ms"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let grandchild: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        // SAFETY: signal 0 only checks whether the pid still exists.
        while unsafe { libc::kill(grandchild, 0) } == 0 {
            assert!(
                Instant::now() < deadline,
                "grandchild survived the probe kill"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn stopping_kills_every_in_flight_probe() {
        let handle = std::thread::spawn(|| {
            run_probe_within(&mut sh("exec sleep 30"), Duration::from_secs(60))
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while in_flight_probes() == 0 {
            assert!(Instant::now() < deadline, "the probe never started");
            std::thread::sleep(Duration::from_millis(10));
        }
        let started = Instant::now();
        assert!(kill_in_flight_probes() >= 1);
        let result = handle.join().unwrap();
        assert!(result.is_ok_and(|output| !output.status.success()));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
