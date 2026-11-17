//! Bounded capability probes for installed AI tools (HS2-BH3M53).
//!
//! AI-tool discovery asks every detected drivable tool for its version and, where the
//! plugin declares one, its model catalog. Those are third-party executables: one can hang
//! (an auth prompt, a network stall, a wedged daemon). A probe runs the tool in its own
//! process group with a deadline; on expiry the whole group is killed and the caller falls
//! back to the plugin manifest. Every in-flight probe is also tracked so a stopping server
//! can kill them all and refuse new ones ([`stop_probes`]) instead of orphaning them.

use hotsheet_process_journal::{ExitReason, LaunchMeta, ProcessJournal, Purpose};
use hotsheet_sync::LockExt;
use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

/// Default deadline for one probe (a `--version` or catalog listing).
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

static PROBE_TIMEOUT_MS: AtomicU64 = AtomicU64::new(DEFAULT_PROBE_TIMEOUT.as_millis() as u64);

/// Process ids (each its own process-group leader) of probes still running, each with the
/// handle that owns its leader when another owner can reap it (HS2-9820JG).
///
/// Why process-global (HS2-YEYF6Y): these registries track OS child processes, which belong
/// to the process, not to any one server state. Shutdown (`stop_probes`) must reach every
/// probe started by any caller (detection, catalog listing, tests) from a signal path that
/// has no `AppState`, and the timeout is set once from a process-level CLI flag.
static IN_FLIGHT: Mutex<Option<HashMap<u32, LeaderHandle>>> = Mutex::new(None);

/// The handle that owns a registered probe leader, when one other than this module does.
type LeaderHandle = Option<Arc<dyn ProbeLeader>>;

/// HS2-9820JG: the handle that owns a probe leader whose exit someone else may observe
/// (and so reap) through it, such as a transport's `Child`. A group is signalled only while
/// that handle is held and says its leader is unreaped, so the pid cannot have been handed
/// out again in between; the leader is reaped only through the handle. Lock order: the
/// in-flight registry, then the handle.
pub(crate) trait ProbeLeader: Send + Sync {
    /// Run `signal` while holding the handle, if the leader is still unreaped. Whether it ran.
    fn while_unreaped(&self, signal: &mut dyn FnMut()) -> bool;
    /// Wait for the leader through the handle; its status if it can still be read.
    fn reap(&self) -> Option<ExitStatus>;
}

impl ProbeLeader for Mutex<Child> {
    fn while_unreaped(&self, signal: &mut dyn FnMut()) -> bool {
        let mut child = self.lock_or_recover();
        let unreaped = matches!(child.try_wait(), Ok(None));
        if unreaped {
            signal();
        }
        unreaped
    }

    fn reap(&self) -> Option<ExitStatus> {
        self.lock_or_recover().wait().ok()
    }
}

/// A probe child to register: its pid, and the handle that owns it when the caller keeps
/// one that can reap it (see [`ProbeLeader`]).
pub(crate) struct Leader {
    pub(crate) pid: u32,
    pub(crate) handle: Option<Arc<dyn ProbeLeader>>,
}

impl Leader {
    /// A leader only this module reaps.
    pub(crate) fn pid(pid: u32) -> Self {
        Self { pid, handle: None }
    }
}

/// Signal the group of `pid` only if its leader is still unreaped: through its owning
/// handle when there is one, otherwise because this module is its only reaper.
fn signal_group(pid: u32, handle: Option<&Arc<dyn ProbeLeader>>) -> bool {
    match handle {
        Some(handle) => handle.while_unreaped(&mut || kill_group(pid)),
        None => {
            kill_group(pid);
            true
        }
    }
}
static JOURNAL: OnceLock<ProcessJournal> = OnceLock::new();

/// Configure machine-local probe diagnostics once in a production host. In-process
/// tests and embedded callers that do not opt in never write to a developer's home.
pub fn configure_process_journal(home: impl AsRef<std::path::Path>) {
    let _ = JOURNAL.set(ProcessJournal::at(home));
}

fn record_probe(pid: u32) -> Option<(ProcessJournal, String)> {
    let journal = JOURNAL.get()?.clone();
    match journal.start(LaunchMeta {
        pid,
        supervisor_pid: std::process::id(),
        supervisor: "hotsheet".into(),
        purpose: Purpose::AiProbe,
        project_id: None,
        session_id: None,
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }) {
        Ok(record) => Some((journal, record.launch_id)),
        Err(error) => {
            eprintln!(
                "Hot Sheet probe journal unavailable: {}",
                hotsheet_process_journal::start_error_summary(&error)
            );
            None
        }
    }
}

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
    // Signal under the lock: a probe leaves the set before it is reaped, so every pid seen
    // here is still this process's own unreaped child.
    let guard = IN_FLIGHT.lock_or_recover();
    guard
        .iter()
        .flatten()
        .filter(|(pid, handle)| signal_group(**pid, handle.as_ref()))
        .count()
}

/// Stop probing for the rest of this process: kill every running probe and refuse new
/// ones (they fail at once, so discovery falls back to the manifests). A discovery scan
/// that is still walking its tools after a stop therefore cannot start another child that
/// the exiting process would orphan.
pub fn stop_probes() -> usize {
    {
        let _guard = IN_FLIGHT.lock_or_recover();
        STOPPED.store(true, Ordering::SeqCst);
    }
    kill_in_flight_probes()
}

/// Whether this process has begun stopping its probes ([`stop_probes`]).
pub fn probes_stopped() -> bool {
    STOPPED.load(Ordering::SeqCst)
}

/// A long-running probe child the caller talks to itself (the codex app-server model
/// catalog, HS2-BJ7A59). The child must lead its own process group. The guard registers it
/// with the in-flight probes (so [`stop_probes`] kills it), kills the group once `deadline`
/// passes, and kills and reaps it when dropped — on success, error, or timeout alike.
pub struct ProbeGroup {
    pid: u32,
    finished: Arc<Mutex<bool>>,
    record: Arc<Mutex<Option<(ProcessJournal, String)>>>,
    disarm: Option<mpsc::Sender<()>>,
}

impl ProbeGroup {
    /// Track the probe child `pid` (its own process-group leader) until the guard drops,
    /// killing and reaping it at `deadline` if it is still running then.
    pub fn track(pid: u32, deadline: Duration) -> Self {
        track(pid, true);
        Self::armed(pid, deadline)
    }

    /// Spawn a probe child and register it in the same step as the stop check, then track
    /// it like [`ProbeGroup::track`]. A stop either refuses the spawn or sees the child in
    /// its kill snapshot; it can never finish in the gap between the two (HS2-F6S04C).
    /// `leader` names the child, and the handle that owns it when the caller keeps one
    /// that can reap it (HS2-9820JG).
    pub(crate) fn spawn<T>(
        spawn: impl FnOnce() -> std::io::Result<T>,
        leader: impl Fn(&T) -> Leader,
        deadline: Duration,
    ) -> std::io::Result<(T, Self)> {
        let child = spawn_registered(spawn, &leader)
            .ok_or_else(|| std::io::Error::other("not probed: the process is stopping"))??;
        let guard = Self::armed(leader(&child).pid, deadline);
        Ok((child, guard))
    }

    /// Arm the deadline watchdog and drop guard for an already registered probe `pid`.
    fn armed(pid: u32, deadline: Duration) -> Self {
        let finished = Arc::new(Mutex::new(false));
        let record = Arc::new(Mutex::new(record_probe(pid)));
        let (disarm, disarmed) = mpsc::channel::<()>();
        let watchdog = finished.clone();
        let watchdog_record = record.clone();
        std::thread::spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = disarmed.recv_timeout(deadline) {
                finish(pid, &watchdog, &watchdog_record);
            }
        });
        Self {
            pid,
            finished,
            record,
            disarm: Some(disarm),
        }
    }
}

impl Drop for ProbeGroup {
    fn drop(&mut self) {
        drop(self.disarm.take());
        finish(self.pid, &self.finished, &self.record);
    }
}

/// Kill a tracked probe group once, stop tracking it, and reap its leader. The leader is
/// reaped only after it leaves the in-flight set (under that set's lock), so a pid that
/// [`kill_in_flight_probes`] can still see is never one the kernel could have reused.
///
/// HS2-9820JG: when the leader's handle belongs to someone else (a transport's `Child`),
/// that owner may already have reaped it. The group is then signalled only under the
/// handle and only while it reports the leader unreaped, and the leader is reaped through
/// the handle, never by a raw `waitpid` on a pid that may no longer be this process's
/// child. Otherwise this module is the only reaper: the `Child` handle (owned by a stream
/// reader) is dropped without waiting, so without the reap the killed child would linger
/// as a zombie.
fn finish(pid: u32, finished: &Mutex<bool>, record: &Mutex<Option<(ProcessJournal, String)>>) {
    let mut finished = finished.lock_or_recover();
    if *finished {
        return;
    }
    *finished = true;
    let (signalled, status) = {
        let mut guard = IN_FLIGHT.lock_or_recover();
        let handle = guard.as_mut().and_then(|set| set.remove(&pid)).flatten();
        match handle {
            Some(handle) => {
                let signalled = signal_group(pid, Some(&handle));
                (signalled, handle.reap())
            }
            None => {
                kill_group(pid);
                reap(pid);
                (true, None)
            }
        }
    };
    if let Some((journal, launch_id)) = record.lock_or_recover().take() {
        let reason = match status {
            _ if signalled => ExitReason::Killed,
            Some(status) if status.success() => ExitReason::Completed,
            Some(_) => ExitReason::Failed,
            None => ExitReason::Unknown,
        };
        let _ = journal.finish(&launch_id, reason);
    }
}

/// Block until child `pid` has exited, without reaping it.
#[cfg(unix)]
fn wait_exited(pid: u32) {
    let id = libc::id_t::from(pid);
    loop {
        // SAFETY: an all-zero siginfo_t is a valid out-parameter for waitid.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: waits (WNOWAIT: without reaping) only for this probe's own child.
        let result =
            unsafe { libc::waitid(libc::P_PID, id, &mut info, libc::WEXITED | libc::WNOWAIT) };
        if result == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
        {
            return;
        }
    }
}

#[cfg(not(unix))]
fn wait_exited(_pid: u32) {}

/// Wait for a SIGKILLed probe leader, so it cannot linger as a zombie.
#[cfg(unix)]
fn reap(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    let mut status = 0;
    // SAFETY: waits only for this probe's own, just-killed child pid.
    unsafe {
        libc::waitpid(pid, &mut status, 0);
    }
}

#[cfg(not(unix))]
fn reap(_pid: u32) {}

/// The number of probes currently running (diagnostics and tests).
pub fn in_flight_probes() -> usize {
    IN_FLIGHT.lock_or_recover().as_ref().map_or(0, HashMap::len)
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
    let mut child = spawn_tracked(command)?;
    let pid = child.id();
    let record = record_probe(pid);
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let (done_tx, done_rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        // Leave the in-flight set before reaping, so a pid `kill_in_flight_probes` can
        // still see is never one the kernel could already have reused.
        wait_exited(pid);
        let status = {
            let mut guard = IN_FLIGHT.lock_or_recover();
            if let Some(set) = guard.as_mut() {
                set.remove(&pid);
            }
            child.wait()
        };
        let _ = done_tx.send(());
        status
    });
    let timed_out = done_rx.recv_timeout(timeout).is_err();
    if timed_out {
        kill_group(pid);
    }
    let status = waiter.join();
    if let Some((journal, launch_id)) = record {
        let reason = if timed_out {
            ExitReason::Killed
        } else {
            match &status {
                Ok(Ok(code)) if code.success() => ExitReason::Completed,
                Ok(Ok(_)) => ExitReason::Failed,
                _ => ExitReason::Unknown,
            }
        };
        let _ = journal.finish(&launch_id, reason);
    }
    let status = status.map_err(|_| format!("'{program}' waiter panicked"))?;
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

/// Register a spawned child before shutdown can finish signalling the probe set.
/// Startup discovery runs on a detached thread: after spawn, even diagnostic journal
/// writes can lose a race with process exit (HS2-3EJNYY).
fn spawn_tracked(command: &mut Command) -> Result<Child, String> {
    let program = command.get_program().to_string_lossy().into_owned();
    spawn_registered(|| command.spawn(), |child: &Child| Leader::pid(child.id()))
        .ok_or_else(|| format!("'{program}' not probed: the process is stopping"))?
        .map_err(|error| format!("starting '{program}': {error}"))
}

/// Run `spawn` and register the child's `pid` while holding the in-flight registry, so
/// [`stop_probes`] (which sets the stop flag and snapshots under the same lock) cannot
/// finish between them. `None` means the process is already stopping and nothing spawned.
fn spawn_registered<T>(
    spawn: impl FnOnce() -> std::io::Result<T>,
    leader: impl FnOnce(&T) -> Leader,
) -> Option<std::io::Result<T>> {
    let mut guard = IN_FLIGHT.lock_or_recover();
    if STOPPED.load(Ordering::SeqCst) {
        return None;
    }
    Some(spawn().inspect(|child| {
        let Leader { pid, handle } = leader(child);
        guard.get_or_insert_with(HashMap::new).insert(pid, handle);
    }))
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
    {
        let mut guard = IN_FLIGHT.lock_or_recover();
        let set = guard.get_or_insert_with(HashMap::new);
        if running {
            set.insert(pid, None);
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
    #[cfg(test)]
    tests::record_signal(pid);
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    // SAFETY: `pid` leads the process group this module created for one probe; signalling
    // the negative id reaches that group only.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

/// Destructive probe tests need their own process: the stop flag is permanent, and
/// killing the global in-flight set can otherwise reach another parallel test's child.
#[cfg(all(test, unix))]
pub(crate) fn isolated_probe_test(test_name: &str) -> bool {
    const ISOLATED: &str = "HOTSHEET_ISOLATED_PROBE_TEST";
    if std::env::var(ISOLATED).ok().as_deref() == Some(test_name) {
        return false;
    }
    let output = hotsheet_ticketing::git::launch(std::env::current_exe().expect("test executable"))
        .args(["--exact", test_name, "--nocapture"])
        .env(ISOLATED, test_name)
        .output()
        .expect("run isolated probe test");
    assert!(
        output.status.success(),
        "isolated {test_name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
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

    /// Every group this module signalled, for the HS2-9820JG tests (each checks only its
    /// own pid, so parallel tests do not disturb one another).
    static SIGNALLED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

    pub(super) fn record_signal(pid: u32) {
        SIGNALLED.lock_or_recover().push(pid);
    }

    fn signalled(pid: u32) -> bool {
        SIGNALLED.lock_or_recover().contains(&pid)
    }

    /// A probe leader in its own group whose `Child` a stand-in transport owns, as the
    /// codex app-server catalog probe's `StreamChild` does.
    fn transport_owned(script: &str) -> (Arc<Mutex<Child>>, ProbeGroup) {
        use std::os::unix::process::CommandExt;
        let (child, guard) = ProbeGroup::spawn(
            || {
                sh(script)
                    .process_group(0)
                    .spawn()
                    .map(|child| Arc::new(Mutex::new(child)))
            },
            |child: &Arc<Mutex<Child>>| Leader {
                pid: child.lock_or_recover().id(),
                handle: Some(child.clone() as Arc<dyn ProbeLeader>),
            },
            Duration::from_secs(60),
        )
        .unwrap();
        (child, guard)
    }

    /// The stand-in transport observes the leader's exit through its own handle, which
    /// reaps it (as `ChildSignals::has_exited` does with `try_wait`).
    fn transport_reaps(child: &Mutex<Child>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.lock_or_recover().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "the probe leader never exited");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// HS2-9820JG: the transport reaps the leader before the guard drops. The pid may since
    /// have been handed out again, so the guard sends nothing to its group and does not wait
    /// for a pid that is no longer this process's child; the probe still leaves the set.
    #[test]
    fn hs2_9820jg_a_leader_its_transport_reaped_is_never_signalled_by_the_guard() {
        let (child, guard) = transport_owned("exit 0");
        let pid = guard.pid;
        transport_reaps(&child);
        drop(guard);
        assert!(
            !signalled(pid),
            "the guard signalled a reaped leader's group"
        );
        assert!(
            IN_FLIGHT
                .lock_or_recover()
                .as_ref()
                .is_none_or(|set| !set.contains_key(&pid)),
            "the reaped probe stayed in flight"
        );
    }

    /// HS2-9820JG: the same order at the probe deadline: the watchdog's `finish` finds the
    /// leader reaped and leaves its group alone.
    #[test]
    fn hs2_9820jg_the_deadline_never_signals_a_reaped_leader() {
        use std::os::unix::process::CommandExt;
        let child = Arc::new(Mutex::new(sh("exit 0").process_group(0).spawn().unwrap()));
        let pid = child.lock_or_recover().id();
        transport_reaps(&child);
        // Registered as the codex probe registers its handle, then left to its deadline.
        IN_FLIGHT
            .lock_or_recover()
            .get_or_insert_with(HashMap::new)
            .insert(pid, Some(child.clone() as Arc<dyn ProbeLeader>));
        let guard = ProbeGroup::armed(pid, Duration::from_millis(20));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !*guard.finished.lock_or_recover() {
            assert!(Instant::now() < deadline, "the deadline never fired");
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(guard);
        assert!(
            !signalled(pid),
            "the deadline signalled a reaped leader's group"
        );
    }

    /// HS2-9820JG: a stop finds the transport already reaped the leader and leaves its group
    /// alone, while a live leader in the same snapshot is still killed.
    #[test]
    fn hs2_9820jg_a_stop_never_signals_a_reaped_leader() {
        if isolated_probe_test("probe::tests::hs2_9820jg_a_stop_never_signals_a_reaped_leader") {
            return;
        }
        let (reaped, reaped_guard) = transport_owned("exit 0");
        let (live, live_guard) = transport_owned("exec sleep 30");
        transport_reaps(&reaped);
        assert_eq!(kill_in_flight_probes(), 1);
        assert!(
            !signalled(reaped_guard.pid),
            "the stop signalled a reaped leader's group"
        );
        assert!(signalled(live_guard.pid), "the stop missed a live probe");
        drop(reaped_guard);
        drop(live_guard);
        assert!(
            !live
                .lock_or_recover()
                .try_wait()
                .unwrap()
                .unwrap()
                .success()
        );
        assert_eq!(in_flight_probes(), 0);
    }

    /// HS2-9820JG: a live, transport-owned leader is still killed with its group and reaped
    /// through its handle when the guard drops.
    #[test]
    fn hs2_9820jg_a_live_transport_owned_leader_is_killed_and_reaped() {
        let (child, guard) = transport_owned("exec sleep 30");
        let pid = guard.pid;
        drop(guard);
        assert!(signalled(pid));
        let status = child.lock_or_recover().try_wait().unwrap();
        assert!(status.is_some_and(|status| !status.success()), "{status:?}");
    }

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
        if isolated_probe_test("probe::tests::stopping_kills_every_in_flight_probe") {
            return;
        }
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

    #[test]
    fn hs2_3ejnyy_shutdown_cannot_miss_a_child_still_spawning() {
        if isolated_probe_test(
            "probe::tests::hs2_3ejnyy_shutdown_cannot_miss_a_child_still_spawning",
        ) {
            return;
        }
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;
        use std::os::unix::{net::UnixStream, process::CommandExt};

        let (mut parent, child_gate) = UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let fd = child_gate.as_raw_fd();
        let spawning = std::thread::spawn(move || {
            let mut command = sh("exec sleep 30");
            command.process_group(0);
            // SAFETY: this callback only uses async-signal-safe libc I/O on the inherited
            // socket, keeping spawn parked until the test releases it.
            unsafe {
                command.pre_exec(move || {
                    let byte = [1_u8];
                    if libc::write(fd, byte.as_ptr().cast(), 1) != 1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    let mut released = [0_u8];
                    if libc::read(fd, released.as_mut_ptr().cast(), 1) != 1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let result = spawn_tracked(&mut command);
            drop(child_gate);
            result.unwrap()
        });
        parent.read_exact(&mut [0]).unwrap();
        // The child exists but spawn has not returned. Stop must share this lock, so
        // it cannot finish its snapshot before the child is registered.
        let spawn_holds_registry = matches!(
            IN_FLIGHT.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        );
        let stopping = std::thread::spawn(stop_probes);
        parent.write_all(&[1]).unwrap();
        let mut child = spawning.join().unwrap();
        let stopped = stopping.join().unwrap();
        // Release and reap the fixture even when the registration invariant regresses.
        kill_group(child.id());
        assert!(!child.wait().unwrap().success());
        track(child.id(), false);
        assert!(
            spawn_holds_registry,
            "spawn did not hold the shutdown registry"
        );
        assert_eq!(stopped, 1);
        assert_eq!(in_flight_probes(), 0);
        assert!(spawn_tracked(&mut sh("exit 0")).is_err());
    }

    #[test]
    fn hs2_f6s04c_shutdown_cannot_miss_a_probe_group_still_registering() {
        if isolated_probe_test(
            "probe::tests::hs2_f6s04c_shutdown_cannot_miss_a_probe_group_still_registering",
        ) {
            return;
        }
        use std::os::unix::process::CommandExt;

        // The spawn closure parks after its child exists but before `ProbeGroup::spawn`
        // registers it: the gap a long-running app-server probe used to leave open.
        let (spawned_tx, spawned_rx) = mpsc::channel::<u32>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let registering = std::thread::spawn(move || {
            ProbeGroup::spawn(
                || {
                    let child = sh("exec sleep 30").process_group(0).spawn()?;
                    spawned_tx.send(child.id()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(child)
                },
                |child: &Child| Leader::pid(child.id()),
                Duration::from_secs(60),
            )
        });
        let pid = spawned_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let spawn_holds_registry = matches!(
            IN_FLIGHT.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        );
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let stopping = std::thread::spawn(move || stopped_tx.send(stop_probes()).unwrap());
        // Stop must wait for the registration instead of snapshotting an empty set.
        let stop_waited = stopped_rx.recv_timeout(Duration::from_millis(200)).is_err();
        release_tx.send(()).unwrap();
        let (child, guard) = registering.join().unwrap().unwrap();
        stopping.join().unwrap();
        let stopped = stopped_rx.recv().unwrap();
        // The stop already killed the group; the guard removes and reaps its leader.
        drop(guard);
        drop(child);
        assert!(
            spawn_holds_registry,
            "spawn did not hold the shutdown registry"
        );
        assert!(stop_waited, "stop finished before the probe registered");
        assert_eq!(stopped, 1, "stop missed probe {pid}");
        assert_eq!(in_flight_probes(), 0);
        let refused = ProbeGroup::spawn(
            || sh("exit 0").spawn(),
            |child: &Child| Leader::pid(child.id()),
            Duration::from_secs(5),
        );
        assert!(refused.is_err_and(|error| error.to_string().contains("stopping")));
    }
}
