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
    pub fn automatic() -> Option<Self> {
        let off = std::env::var(JOB_MONITOR_AUTOSTART_ENV)
            .is_ok_and(|value| value.trim().eq_ignore_ascii_case("off"));
        (!off).then(|| Self::new(current_program()))
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
