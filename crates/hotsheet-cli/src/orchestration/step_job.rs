//! HS2-2R32SD: a Windows job object that holds one validation step and everything it
//! starts, so a timeout or a cancel ends the whole tree, as a process group does on
//! Unix.
//!
//! The step is created suspended, assigned to the job, and only then resumed, so no
//! process it starts can escape the job. The job is created with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: closing its handle when the step ends also ends
//! anything the step left running, and so does the death of the process holding it (a
//! detached validation runner). When a job cannot be created or the step cannot be
//! assigned (a host job that forbids nesting), the step still runs and only its own
//! process is stopped, as before.

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
    };

    /// A job that ends its processes when it is dropped.
    pub(in crate::orchestration) struct StepJob(HANDLE);

    // SAFETY: a job handle may be used and closed from any thread.
    unsafe impl Send for StepJob {}

    impl StepJob {
        /// A new anonymous kill-on-close job, or `None` when one cannot be made.
        pub(in crate::orchestration) fn new() -> Option<Self> {
            // SAFETY: no security attributes and no name; the handle is checked.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return None;
            }
            let job = Self(handle);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: the job is open and `limits` is a complete structure of the
            // class named, of the size passed.
            let set = unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            (set != 0).then_some(job)
        }

        /// The creation flag a step held by a job is started with.
        pub(in crate::orchestration) const fn creation_flags() -> u32 {
            CREATE_SUSPENDED
        }

        /// Put the suspended `child` in the job. Whether it was assigned.
        pub(in crate::orchestration) fn assign(&self, child: &std::process::Child) -> bool {
            // SAFETY: both handles are open; the child's is owned by `child`.
            unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) != 0 }
        }

        /// End every process in the job.
        pub(in crate::orchestration) fn terminate(&self) {
            // SAFETY: the job handle is open.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for StepJob {
        fn drop(&mut self) {
            // SAFETY: the handle is owned here and closed once; with the kill-on-close
            // limit this also ends whatever the step left running.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// Resume the threads of a process started suspended: a new process has one, its
    /// main thread. Whether one was resumed.
    pub(in crate::orchestration) fn resume(pid: u32) -> bool {
        // SAFETY: a thread snapshot takes no pointers; the handle is checked and closed.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return false;
        }
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut resumed = false;
        // SAFETY: the snapshot is open and dwSize names the writable entry.
        let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: OpenThread takes no pointers; a null handle is checked.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !thread.is_null() {
                    // SAFETY: the thread handle is open and closed right after.
                    unsafe {
                        resumed |= ResumeThread(thread) != u32::MAX;
                        CloseHandle(thread);
                    }
                }
            }
            // SAFETY: as above.
            more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        // SAFETY: the snapshot handle is owned here and closed once.
        unsafe {
            CloseHandle(snapshot);
        }
        resumed
    }
}

#[cfg(windows)]
pub(super) use imp::{StepJob, resume};
