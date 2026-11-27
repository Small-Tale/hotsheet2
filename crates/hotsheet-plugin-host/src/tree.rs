//! The process tree a plugin owns (docs/38 §38.6): its own process group on Unix, a
//! kill-on-close job object on Windows when one can be made (HS2-CYDK9Z's pattern, as the
//! external claim-backend runner uses). Stopping is by id, so it never needs the `Child`
//! lock that the reaping thread holds.

use std::process::{Child, Command};

pub(crate) struct ProcessTree {
    #[cfg(windows)]
    job: Option<SharedJob>,
}

/// A job object shared by the threads that use one plugin connection.
#[cfg(windows)]
struct SharedJob(hotsheet_process_journal::tree_job::TreeJob);

// SAFETY: the wrapped value is a Windows job-object HANDLE, a kernel handle that any thread
// of the process may use. It is only used through `TerminateJobObject` (via `terminate`,
// which takes `&self`) and closed once in `Drop`; both are thread-safe kernel calls, and
// nothing reads or writes memory behind the pointer.
#[cfg(windows)]
unsafe impl Send for SharedJob {}
// SAFETY: as above; shared references only reach the thread-safe `TerminateJobObject`.
#[cfg(windows)]
unsafe impl Sync for SharedJob {}

impl ProcessTree {
    /// Sets `command` up to start as the root of its own tree.
    #[cfg(unix)]
    pub(crate) fn prepare(command: &mut Command) -> Self {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        ProcessTree {}
    }

    /// Sets `command` up to start suspended, so nothing it starts runs before it is in
    /// the job.
    #[cfg(windows)]
    pub(crate) fn prepare(command: &mut Command) -> Self {
        use hotsheet_process_journal::tree_job::TreeJob;
        use std::os::windows::process::CommandExt;
        let job = TreeJob::new();
        if job.is_some() {
            command.creation_flags(TreeJob::creation_flags());
        }
        ProcessTree {
            job: job.map(SharedJob),
        }
    }

    #[cfg(not(any(unix, windows)))]
    pub(crate) fn prepare(_command: &mut Command) -> Self {
        ProcessTree {}
    }

    /// The spawned root. Nothing to do outside Windows.
    #[cfg(not(windows))]
    pub(crate) fn started(self, _child: &mut Child) -> Result<Self, String> {
        Ok(self)
    }

    /// The spawned root: put it in the job and let it run.
    #[cfg(windows)]
    pub(crate) fn started(self, child: &mut Child) -> Result<Self, String> {
        let Some(SharedJob(job)) = self.job else {
            return Ok(ProcessTree { job: None });
        };
        let held = job.assign(child);
        if !hotsheet_process_journal::tree_job::resume(child.id()) {
            job.terminate();
            // Already failing; the kill and reap results add nothing to the report.
            let _ = child.kill();
            let _ = child.wait();
            return Err("the plugin process could not be resumed".to_owned());
        }
        Ok(ProcessTree {
            job: held.then_some(SharedJob(job)),
        })
    }

    /// Asks the tree to stop (SIGTERM to the group). Windows has no polite signal for a
    /// console-less child; [`ProcessTree::kill`] follows after the grace period.
    #[cfg_attr(not(unix), allow(unused_variables))] // `pid` is only used on Unix (HS2-XY0VTN)
    pub(crate) fn terminate(&self, pid: u32) {
        #[cfg(unix)]
        signal_group(pid, libc::SIGTERM);
    }

    /// Ends the whole tree.
    #[cfg_attr(not(unix), allow(unused_variables))] // `pid` is only used on Unix (HS2-XY0VTN)
    pub(crate) fn kill(&self, pid: u32) {
        #[cfg(windows)]
        if let Some(SharedJob(job)) = &self.job {
            job.terminate();
        }
        #[cfg(unix)]
        signal_group(pid, libc::SIGKILL);
    }
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: libc::c_int) {
    if let Ok(group) = libc::pid_t::try_from(pid) {
        // SAFETY: `killpg` takes plain integers and touches no memory. The group is the
        // plugin's own, made by `process_group(0)` at spawn; callers only signal while
        // the root is unreaped, so the id cannot name another group.
        unsafe {
            libc::killpg(group, signal);
        }
    }
}
