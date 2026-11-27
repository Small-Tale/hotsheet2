//! A Windows job object that holds one process and everything it starts, so a timeout or
//! a cancel ends the whole tree, as a process group does on Unix.
//!
//! HS2-2R32SD built it for validation steps; HS2-CYDK9Z moved it here so the external
//! claim-backend plugin runner stops a plugin's own children the same way.
//!
//! The process is created suspended ([`TreeJob::creation_flags`]), assigned to the job,
//! and only then resumed ([`resume`]), so nothing it starts can escape the job. The job
//! is created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: closing its handle when the
//! process ends also ends anything it left running, and so does the death of the process
//! holding the job. When a job cannot be created or the process cannot be assigned (a
//! host job that forbids nesting), the process still runs and only it is stopped.

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
    pub struct TreeJob(HANDLE);

    // SAFETY: a job handle may be used and closed from any thread.
    unsafe impl Send for TreeJob {}

    impl TreeJob {
        /// A new anonymous kill-on-close job, or `None` when one cannot be made.
        pub fn new() -> Option<Self> {
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
        pub const fn creation_flags() -> u32 {
            CREATE_SUSPENDED
        }

        /// Put the suspended `child` in the job. Whether it was assigned.
        pub fn assign(&self, child: &std::process::Child) -> bool {
            // SAFETY: both handles are open; the child's is owned by `child`.
            unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) != 0 }
        }

        /// End every process in the job.
        pub fn terminate(&self) {
            // SAFETY: the job handle is open.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for TreeJob {
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
    pub fn resume(pid: u32) -> bool {
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
pub use imp::{TreeJob, resume};
