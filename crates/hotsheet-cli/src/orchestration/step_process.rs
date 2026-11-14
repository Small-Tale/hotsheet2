//! HS2-HTS89S: a validation step whose hosting process died is stopped, safely.
//!
//! A step runs in its own process group (its pid is the group id). When the process
//! that launched it dies mid-step, the group keeps running with nobody to collect it,
//! and its attempt is unverified (`execution_lost`). The hosting machine records each
//! running step's group with the pid *and start time* of the step and of its hosting
//! process. A later host process, when it starts serving or when the attempt is
//! cancelled, stops a group whose hosting process is gone:
//!
//! - the step's leader is alive with the recorded start time: the group is the step's;
//! - the leader's pid now names another process: it is not the step's, nothing is done;
//! - no process has that pid: a group of that id can only be the step's remaining
//!   members, because a pid is not reused while a group of that id exists.
//!
//! The record keeps no outcome for such a step: what it would have reported is not
//! known, so the attempt stays unverified. Only Unix runs steps as process groups;
//! elsewhere nothing is recorded and nothing is stopped.
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use hotsheet_ticketing::orchestration::{HostedValidationRegistry, StepProcess};

use super::*;

/// When a process started, as the operating system reports it, in a unit fixed per
/// platform. `None` when there is no such process or it cannot be read.
pub(super) fn process_started(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        // Field 22 of /proc/<pid>/stat, after the parenthesized command name.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = &stat[stat.rfind(')')? + 1..];
        rest.split_whitespace().nth(19)?.parse().ok()
    }
    #[cfg(target_os = "macos")]
    {
        let pid = libc::c_int::try_from(pid).ok()?;
        // SAFETY: `info` is a plain C struct that `proc_pidinfo` fills; the size passed
        // is exactly its size, and an error leaves it unread.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
        let read = unsafe {
            libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
        };
        (read == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// The start time of a process this one just spawned. A loaded machine can answer
/// nothing for a child that is still being set up, so it is asked a few more times
/// over a fraction of a second (a local read of its own child, no request).
pub(super) fn spawned_started(pid: u32) -> Option<u64> {
    for _ in 0..50 {
        if let Some(started) = process_started(pid) {
            return Some(started);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

/// The identity of a step just launched by this process.
pub(super) fn step_process(pid: u32) -> Option<StepProcess> {
    if !cfg!(unix) {
        return None;
    }
    let host_pid = std::process::id();
    Some(StepProcess {
        pid,
        started: spawned_started(pid)?,
        host_pid,
        host_started: process_started(host_pid)?,
    })
}

/// Whether the process that launched the step still runs.
fn host_alive(process: &StepProcess) -> bool {
    process_started(process.host_pid) == Some(process.host_started)
}

/// Stop the step's process group when it is provably the step's. Returns whether a
/// stop was sent.
pub(super) fn stop_orphan(process: &StepProcess) -> bool {
    #[cfg(unix)]
    {
        let Ok(group) = libc::pid_t::try_from(process.pid) else {
            return false;
        };
        let ours = match process_started(process.pid) {
            Some(started) => started == process.started,
            None => {
                // SAFETY: `kill` with signal 0 only checks that the pid exists.
                let exists = unsafe { libc::kill(group, 0) } == 0
                    || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
                // A pid that exists but cannot be read is never assumed to be ours.
                !exists
            }
        };
        if !ours {
            return false;
        }
        // SAFETY: `killpg` takes plain integers and touches no memory; the group is
        // the step's, as established above.
        unsafe { libc::killpg(group, libc::SIGKILL) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = process;
        false
    }
}

/// Machine homes this process already reaped, so it happens once per home.
static REAPED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

impl OrchestrationRuntime {
    /// HS2-HTS89S: stop every recorded step group whose hosting process is gone, and
    /// forget the group. The attempts stay unknown here (`execution_lost` on the
    /// orchestrating side). Returns how many groups were stopped. A step of a hosting
    /// process that still runs is left to it.
    pub fn reap_orphaned_validations(&self) -> usize {
        let registry = HostedValidationRegistry::new(&self.home);
        let mut stopped = 0;
        for record in registry.with_process().unwrap_or_default() {
            let Some(process) = record.process else {
                continue;
            };
            if host_alive(&process) {
                continue;
            }
            if stop_orphan(&process) {
                stopped += 1;
            }
            let _ = registry.process_gone(&record.checkout_id, &record.execution_id);
        }
        stopped
    }

    /// HS2-HTS89S: a cancel of an execution no live process holds stops its group
    /// when its hosting process is gone.
    pub(super) fn stop_orphaned_step(&self, checkout_id: &str, execution_id: &str) {
        let registry = HostedValidationRegistry::new(&self.home);
        let Ok(Some(record)) = registry.record(checkout_id, execution_id) else {
            return;
        };
        let Some(process) = record.process else {
            return;
        };
        if !host_alive(&process) {
            stop_orphan(&process);
            let _ = registry.process_gone(checkout_id, execution_id);
        }
    }

    /// The first hosted validation request a process serves for a home reaps first, so
    /// a CLI host that never ran the server's start-up does it too.
    pub(super) fn reap_once(&self) {
        let reaped = REAPED.get_or_init(Default::default);
        let first = reaped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(self.home.clone());
        if first {
            self.reap_orphaned_validations();
        }
    }
}
