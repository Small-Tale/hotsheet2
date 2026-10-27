//! Owned process boundary for a guarded ticket command (HS2-MFYYJ9).

use std::ffi::OsString;
use std::io;
use std::process::{Child, ExitStatus};

#[cfg(windows)]
use anyhow::bail;
use anyhow::{Context, Result};
use hotsheet_ticketing::git;

pub(super) struct GuardedChild {
    child: Child,
    stopped: bool,
    #[cfg(windows)]
    job: Job,
    // Keep the signaled named event alive until the helper has opened it.
    #[cfg(windows)]
    _gate: StartGate,
}

impl GuardedChild {
    pub(super) fn spawn(command: &[OsString]) -> Result<Self> {
        command.first().context("guarded command is required")?;
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let program = &command[0];
            let mut launch = git::launch(program);
            launch.args(&command[1..]).process_group(0);
            let child = launch
                .spawn()
                .with_context(|| format!("could not start guarded command {program:?}"))?;
            Ok(Self {
                child,
                stopped: false,
            })
        }
        #[cfg(windows)]
        {
            // The helper waits on this event before starting the requested command.
            // Assignment to the job therefore precedes any descendant spawn.
            let job = Job::new().context("could not create guarded process job")?;
            let gate = StartGate::new().context("could not create guarded start gate")?;
            let mut launch = git::launch(std::env::current_exe()?);
            let mut child = launch
                .arg("claim-run-child")
                .arg("--")
                .args(command)
                .env(START_GATE_ENV, &gate.name)
                .spawn()
                .context("could not start guarded command helper")?;
            if let Err(error) = job.assign(&child) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("could not assign helper to its process job");
            }
            if let Err(error) = gate.release() {
                let _ = job.terminate();
                let _ = child.wait();
                return Err(error).context("could not release guarded command helper");
            }
            Ok(Self {
                child,
                stopped: false,
                job,
                _gate: gate,
            })
        }
    }

    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Stop the entire owned tree, including descendants that inherited stdio.
    pub(super) fn stop(&mut self) -> io::Result<()> {
        if self.stopped {
            return Ok(());
        }
        #[cfg(unix)]
        {
            let group = i32::try_from(self.child.id()).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "guarded process id exceeded pgid",
                )
            })?;
            let result = unsafe { libc::kill(-group, libc::SIGKILL) };
            if result != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Err(error);
                }
            }
        }
        #[cfg(windows)]
        self.job.terminate()?;
        let _ = self.child.wait();
        self.stopped = true;
        Ok(())
    }
}

impl Drop for GuardedChild {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(windows)]
pub(super) fn run_child(command: &[OsString]) -> Result<()> {
    #[cfg(debug_assertions)]
    if let Ok(marker) = std::env::var("HOTSHEET_TEST_CLAIM_RUN_PARENT_READY_PATH") {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !std::path::Path::new(&marker).exists() {
            if std::time::Instant::now() >= deadline {
                bail!("guarded parent did not return from spawn");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    StartGate::wait_for_release()?;
    let program = command.first().context("guarded command is required")?;
    let status = git::launch(program)
        .args(&command[1..])
        .env_remove(START_GATE_ENV)
        .env_remove("HOTSHEET_TEST_CLAIM_RUN_PARENT_READY_PATH")
        .status()
        .with_context(|| format!("could not start guarded command {program:?}"))?;
    if !status.success() {
        bail!("guarded command failed with {status}");
    }
    Ok(())
}

#[cfg(windows)]
const START_GATE_ENV: &str = "HOTSHEET_CLAIM_RUN_START_GATE";

#[cfg(windows)]
struct StartGate {
    handle: windows_sys::Win32::Foundation::HANDLE,
    name: String,
}

#[cfg(windows)]
impl StartGate {
    fn new() -> io::Result<Self> {
        use windows_sys::Win32::System::Threading::CreateEventW;

        let name = format!("Local\\hotsheet-claim-run-{}", ulid::Ulid::new());
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { handle, name })
    }

    fn release(&self) -> io::Result<()> {
        use windows_sys::Win32::System::Threading::SetEvent;

        if unsafe { SetEvent(self.handle) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn wait_for_release() -> Result<()> {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

        let name = std::env::var(START_GATE_ENV).context("guarded start gate is missing")?;
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error()).context("could not open guarded start gate");
        }
        let gate = Self { handle, name };
        if unsafe { WaitForSingleObject(gate.handle, 10_000) } != WAIT_OBJECT_0 {
            bail!("guarded helper was not released");
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for StartGate {
    fn drop(&mut self) {
        let _ = unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Job {
    fn new() -> io::Result<Self> {
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self(handle);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn assign(&self, child: &Child) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

        let assigned = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) };
        if assigned == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn terminate(&self) -> io::Result<()> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        if unsafe { TerminateJobObject(self.0, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        let _ = unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}
