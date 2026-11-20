//! One process liveness probe for the CLI and the server (HS2-W7E2MJ).
//!
//! The CLI once carried three copies of this probe with different non-Unix answers: one
//! said every process was dead, so on Windows `checkout remove` skipped the running server
//! and its open-project refusal. Every caller now shares this native probe.
//!
//! The answer matches `kill -0`: only a process this user may signal or query counts as
//! alive. A direct system call, not a spawned `kill`, because MCP presence sweeps check a
//! pid on every heartbeat (HS2-7RRAA8).

/// Whether process `pid` is running.
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // A pid past `pid_t` would wrap negative, and `kill(-1, 0)` targets every process
        // and succeeds. Pid 0 targets our own process group.
        if pid == 0 || pid > i32::MAX as u32 {
            return false;
        }
        // SAFETY: signal 0 performs only the existence and permission check; it delivers nothing.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(windows)]
    {
        windows::alive(pid)
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Without a probe, assume alive: a safety refusal must never be skipped because
        // the platform cannot answer.
        let _ = pid;
        true
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    /// A process that can be opened and has not yet signalled exit. `WaitForSingleObject`
    /// is used rather than `GetExitCodeProcess`, whose `STILL_ACTIVE` (259) is also a
    /// valid exit code. A process this user may not open does not count as alive.
    pub(super) fn alive(pid: u32) -> bool {
        // Pid 0 is the System Idle Process, never a Hot Sheet process.
        if pid == 0 {
            return false;
        }
        // SAFETY: OpenProcess takes plain values and returns null on failure.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return false;
        }
        // SAFETY: a zero timeout only polls the live handle's signalled state.
        let running = unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT;
        // SAFETY: the handle came from a successful OpenProcess and is closed once.
        let _ = unsafe { CloseHandle(handle) };
        running
    }
}

#[cfg(test)]
mod tests {
    use super::pid_alive;

    #[test]
    fn hs2_w7e2mj_this_process_is_alive_and_an_exited_child_is_not() {
        assert!(
            pid_alive(std::process::id()),
            "the test process itself is alive"
        );

        let mut command = if cfg!(windows) {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", "exit 0"]);
            command
        } else {
            std::process::Command::new("true")
        };
        let mut child = command.spawn().expect("spawn a short-lived child");
        let pid = child.id();
        child.wait().expect("wait for the child");
        // On Windows the still-open `Child` handle keeps the pid from being reused.
        assert!(!pid_alive(pid), "an exited, reaped child is dead");
    }

    #[test]
    fn hs2_w7e2mj_out_of_range_pids_never_read_as_alive() {
        assert!(!pid_alive(0), "pid 0 is not a single live process");
        assert!(!pid_alive(u32::MAX), "u32::MAX is not a live process");
    }

    #[test]
    fn hs2_w7e2mj_a_running_child_is_alive_until_it_is_stopped() {
        let mut command = if cfg!(windows) {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1 >NUL"]);
            command
        } else {
            let mut command = std::process::Command::new("sleep");
            command.arg("30");
            command
        };
        let mut child = command
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn a long-lived child");
        assert!(pid_alive(child.id()), "a running child is alive");
        child.kill().expect("stop the child");
        child.wait().expect("reap the child");
        assert!(!pid_alive(child.id()), "a stopped child is dead");
    }
}
