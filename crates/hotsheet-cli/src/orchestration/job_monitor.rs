//! HS2-VVHVF0: making sure the machine's Job monitor runs, as one workflow shared by
//! `hotsheet-cli`, the server and the MCP core backend.
//!
//! A watch Job only advances while the monitor of HS2-5TC1GQ observes its provider.
//! The monitor is a detached `hotsheet-cli job monitor` process that holds the machine's
//! monitor slot, so starting it is safe to repeat and to race: every caller ends up
//! with the one monitor that holds the slot.
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use hotsheet_jobs::{MonitorInfo, MonitorSlot, ensure_monitor_running};

/// Set to `off` to keep [`OrchestrationRuntime::live`](super::OrchestrationRuntime::live)
/// from starting a monitor on its own. The repository's Cargo configuration forces it
/// for tests, so no test process starts a monitor that would contact a provider; a test
/// that wants one injects it.
pub const JOB_MONITOR_AUTOSTART_ENV: &str = "HOTSHEET_JOB_MONITOR_AUTOSTART";
/// How long a start waits for the monitor to publish itself.
const MONITOR_ANSWER: Duration = Duration::from_secs(15);

/// Something that can make sure the machine's Job monitor runs.
pub trait JobMonitor: Send + Sync {
    /// Report the running monitor or start one. `Ok(true)` when this call started it.
    /// Never contacts a provider.
    fn ensure(&self, home: &Path) -> io::Result<bool>;
}

/// The shipped [`JobMonitor`]: a detached `hotsheet-cli job monitor` process.
#[derive(Clone, Debug)]
pub struct DetachedJobMonitor {
    supervisor: &'static str,
    flags: Vec<String>,
    env: Vec<(String, String)>,
}

impl DetachedJobMonitor {
    /// `supervisor` names the launching program in the process journal.
    pub fn new(supervisor: &'static str) -> Self {
        Self {
            supervisor,
            flags: Vec::new(),
            env: Vec::new(),
        }
    }

    /// The monitor for a runtime that starts one on its own, or `None` when
    /// [`JOB_MONITOR_AUTOSTART_ENV`] is `off`.
    ///
    /// A test build (the isolated `test-permission-vault` artifact that browser and
    /// script fixtures launch outside Cargo) starts none unless the variable is `on`:
    /// a monitor asks `gh` for the developer's real token, and those fixtures inherit
    /// the developer's environment rather than the Cargo configuration.
    pub fn automatic() -> Option<Self> {
        let setting = std::env::var(JOB_MONITOR_AUTOSTART_ENV).ok();
        autostart(
            setting.as_deref(),
            hotsheet_aitools::test_permission_vault_available(),
        )
        .then(|| Self::new(current_program()))
    }

    /// Extra `job monitor` flags: the timing overrides of `job watch start`.
    #[must_use]
    pub fn flags(mut self, flags: impl IntoIterator<Item = String>) -> Self {
        self.flags.extend(flags);
        self
    }

    /// An environment variable for the monitor process only, such as the provider's
    /// API root. This process's own environment is not changed.
    #[must_use]
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }

    /// Start the detached monitor unless one already runs, and report the one that
    /// does. Two starts can race: both spawn, one monitor claims the slot and the other
    /// exits.
    pub fn ensure_info(&self, home: &Path) -> io::Result<(MonitorInfo, bool)> {
        let spawn = || {
            let cli = hotsheet_aitools::launch_safety::hotsheet_cli()
                .map_err(|error| io::Error::new(io::ErrorKind::NotFound, error.to_string()))?;
            // HS2-J79CZF: launched without inherited repository or build-pin variables.
            let mut command = hotsheet_ticketing::git::launch(cli);
            command
                .args(["job", "monitor"])
                .args(&self.flags)
                .env("HOTSHEET_HOME", home)
                .envs(self.env.iter().map(|(name, value)| (name, value)))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            detach(&mut command);
            hotsheet_process_journal::LaunchContext {
                journal: hotsheet_process_journal::ProcessJournal::at(home),
                supervisor: self.supervisor,
                purpose: hotsheet_process_journal::Purpose::JobMonitor,
                project_id: None,
                session_id: None,
                binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
            }
            .detached(&mut command)
        };
        ensure_monitor_running(&MonitorSlot::at(home), spawn, MONITOR_ANSWER)
    }
}

impl JobMonitor for DetachedJobMonitor {
    fn ensure(&self, home: &Path) -> io::Result<bool> {
        self.ensure_info(home).map(|(_, started)| started)
    }
}

/// The running program's name for the process journal.
fn current_program() -> &'static str {
    let name = std::env::current_exe().ok().and_then(|path| {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    });
    match name.as_deref() {
        Some("hotsheet-server") => "hotsheet-server",
        Some("hotsheet-mcp") => "hotsheet-mcp",
        _ => "hotsheet-cli",
    }
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // Its own process group, so a Ctrl-C in the starting terminal does not reach it.
    command.process_group(0);
    // HS2-WEYR63: the monitor outlives its launcher by up to its idle exit, so it must hold
    // nothing of the launcher's but what it is given. On macOS the standard library sets
    // close-on-exec on a new pipe in a second step, so a process spawned on another thread
    // in that instant inherits the pipe; the launcher passed such a descriptor on, and a
    // caller waiting for its output's end waited until the monitor exited. Every descriptor
    // above stdio that is not close-on-exec is closed in the child before exec.
    let limit = open_descriptor_limit();
    // SAFETY: runs in the forked child before exec and makes only fcntl and close calls,
    // which are async-signal-safe. Close-on-exec descriptors are left alone, so the
    // standard library's own exec status pipe stays open.
    unsafe {
        command.pre_exec(move || {
            for fd in 3..limit {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                    libc::close(fd);
                }
            }
            Ok(())
        });
    }
}

/// The highest descriptor number worth checking, read before the fork.
#[cfg(unix)]
fn open_descriptor_limit() -> i32 {
    // SAFETY: sysconf only reads a system limit.
    let limit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
    i32::try_from(limit.clamp(256, 65_536)).unwrap_or(65_536)
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn detach(_command: &mut Command) {}

/// Whether a runtime starts the monitor on its own: `off` never does, `on` always does,
/// and an unset or other value does except in a test build.
fn autostart(setting: Option<&str>, test_build: bool) -> bool {
    match setting
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("off") => false,
        Some("on") => true,
        _ => !test_build,
    }
}

#[cfg(test)]
mod autostart_tests {
    use super::autostart;

    /// A browser fixture's server inherits the developer's shell, not the Cargo
    /// configuration, so a test build must not start a monitor that reaches `gh`.
    #[test]
    fn hs2_vvhvf0_a_test_build_starts_no_monitor_unless_asked() {
        for (setting, test_build, expected) in [
            (None, false, true),
            (None, true, false),
            (Some("off"), false, false),
            (Some(" OFF "), true, false),
            (Some("on"), true, true),
            (Some("on"), false, true),
            (Some("yes"), true, false),
            (Some("yes"), false, true),
        ] {
            assert_eq!(
                autostart(setting, test_build),
                expected,
                "{setting:?} {test_build}"
            );
        }
    }
}
