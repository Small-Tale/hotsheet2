//! Install, inspect, and remove the terminal launch relay's per-user **LaunchAgent**
//! (HS2-HYF56Y). One workflow serves both the desktop app and the headless
//! `hotsheet-cli terminal-relay` command.
//!
//! The agent must be loaded into the GUI (Aqua) domain while a healthy desktop session
//! exists: from a dead bootstrap namespace even `launchctl asuser` fails, which is the reason
//! the relay exists. Every `launchctl` call goes through [`Launchctl`], and the plist
//! directory is explicit, so tests never touch the real `~/Library/LaunchAgents` or the
//! user's real GUI domain. The test environment additionally forces [`LAUNCHCTL_ENV`] and
//! [`LAUNCH_AGENTS_DIR_ENV`] to inert paths (`.cargo/config.toml`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::launch_relay::{RelayHello, RelayUnavailable, relay_hello};

/// Label of the agent for the default machine home.
pub const BASE_LABEL: &str = "com.smalltale.hotsheet2.terminal-relay";

/// Overrides the `launchctl` executable (tests point it at a fake or at nothing).
pub const LAUNCHCTL_ENV: &str = "HOTSHEET_LAUNCHCTL";

/// Overrides the LaunchAgents directory (tests point it into `target/`).
pub const LAUNCH_AGENTS_DIR_ENV: &str = "HOTSHEET_LAUNCH_AGENTS_DIR";

/// The hidden server flag that runs the relay.
pub const RELAY_FLAG: &str = "--terminal-launch-relay";

/// How long install waits for launchd to start the relay and answer `hello`.
pub const READY_TIMEOUT: Duration = Duration::from_secs(5);

/// One launchctl invocation's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchctlOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `launchctl`. Injected so tests never reach the real launchd.
pub trait Launchctl {
    fn run(&self, args: &[String]) -> io::Result<LaunchctlOutput>;
}

/// The real `launchctl`, or the executable named by [`LAUNCHCTL_ENV`].
pub fn launchctl_program() -> PathBuf {
    std::env::var_os(LAUNCHCTL_ENV).map_or_else(|| PathBuf::from("/bin/launchctl"), PathBuf::from)
}

/// Runs [`launchctl_program`]. This leaf crate builds no commands itself: the caller supplies
/// its scrubbed launch constructor (`hotsheet_ticketing::git::launch`, HS2-J79CZF).
pub struct SystemLaunchctl {
    pub program: PathBuf,
    pub command: fn(&Path) -> Command,
}

impl SystemLaunchctl {
    pub fn from_env(command: fn(&Path) -> Command) -> Self {
        Self {
            program: launchctl_program(),
            command,
        }
    }
}

impl Launchctl for SystemLaunchctl {
    fn run(&self, args: &[String]) -> io::Result<LaunchctlOutput> {
        let output = (self.command)(&self.program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()?;
        Ok(LaunchctlOutput {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// The LaunchAgents directory: [`LAUNCH_AGENTS_DIR_ENV`], else `~/Library/LaunchAgents`.
pub fn default_agents_dir() -> io::Result<PathBuf> {
    if let Some(directory) = std::env::var_os(LAUNCH_AGENTS_DIR_ENV) {
        return Ok(PathBuf::from(directory));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "HOME is not set, so ~/Library/LaunchAgents is unknown",
        )
    })?;
    Ok(PathBuf::from(home).join("Library").join("LaunchAgents"))
}

/// Everything that identifies one relay agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelayAgent {
    pub label: String,
    pub plist: PathBuf,
    /// The `hotsheet-server` binary launchd runs with [`RELAY_FLAG`].
    pub program: PathBuf,
    pub socket: PathBuf,
    pub home: PathBuf,
    pub log: PathBuf,
}

impl RelayAgent {
    /// The agent for machine home `home`. The default home (`~/.hotsheet2`) keeps the plain
    /// label; any other home gets a label suffixed with its key, so a development or test
    /// home never replaces the user's real agent.
    pub fn for_home(
        home: &Path,
        program: &Path,
        agents_dir: &Path,
        default_home: Option<&Path>,
    ) -> io::Result<Self> {
        let socket = crate::broker_socket::relay_socket_path(home)?;
        let is_default = default_home.is_some_and(|default| same_path(default, home));
        let label = if is_default {
            BASE_LABEL.to_owned()
        } else {
            format!(
                "{BASE_LABEL}.{}",
                crate::broker_socket::home_key(home, "terminal-launch-relay")
            )
        };
        Ok(Self {
            plist: agents_dir.join(format!("{label}.plist")),
            label,
            program: program.to_path_buf(),
            socket,
            home: home.to_path_buf(),
            log: home.join("logs").join("terminal-relay.log"),
        })
    }

    /// The launchd job definition. `LimitLoadToSessionType=Aqua` keeps it in the GUI domain,
    /// `KeepAlive` restarts it, and `AbandonProcessGroup` (with each broker in its own
    /// session) keeps running brokers alive when the relay itself stops or is replaced.
    pub fn render_plist(&self) -> String {
        let string = |value: &str| format!("<string>{}</string>", xml_escape(value));
        let path = |value: &Path| string(&value.to_string_lossy());
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	{label}
	<key>ProgramArguments</key>
	<array>
		{program}
		{flag}
		{socket}
	</array>
	<key>EnvironmentVariables</key>
	<dict>
		<key>HOTSHEET_HOME</key>
		{home}
	</dict>
	<key>LimitLoadToSessionType</key>
	<string>Aqua</string>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<true/>
	<key>AbandonProcessGroup</key>
	<true/>
	<key>ProcessType</key>
	<string>Interactive</string>
	<key>StandardErrorPath</key>
	{log}
</dict>
</plist>
"#,
            label = string(&self.label),
            program = path(&self.program),
            flag = string(RELAY_FLAG),
            socket = path(&self.socket),
            home = path(&self.home),
            log = path(&self.log),
        )
    }

    fn service(&self, uid: u32) -> String {
        format!("gui/{uid}/{}", self.label)
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// What install did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallOutcome {
    /// The agent was not loaded; its plist was written and loaded.
    Installed,
    /// The agent's definition changed (for example a moved binary) and was reloaded.
    Updated,
    /// The current definition was already loaded; nothing changed.
    AlreadyCurrent,
    /// The current definition was loaded but its relay did not answer, so it was restarted.
    Restarted,
}

/// A successful install.
#[derive(Debug, Clone, Serialize)]
pub struct InstallReport {
    pub outcome: InstallOutcome,
    pub agent: RelayAgent,
    pub relay: RelayHello,
}

/// Why install could not finish.
#[derive(Debug)]
pub enum InstallError {
    /// No healthy desktop session: the agent cannot be loaded from here.
    NoGuiSession {
        reason: String,
    },
    /// A launchctl step failed.
    Launchctl {
        step: &'static str,
        output: LaunchctlOutput,
    },
    /// The agent loaded but its relay never answered.
    NotReady {
        agent: Box<RelayAgent>,
        reason: RelayUnavailable,
    },
    Io(io::Error),
}

impl From<io::Error> for InstallError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoGuiSession { reason } => write!(
                f,
                "cannot install the terminal launch relay: {reason}. Log in to the Mac's desktop, \
                 then run `hotsheet-cli terminal-relay install` from Terminal (or open the Hot \
                 Sheet desktop app, which runs the same install)"
            ),
            Self::Launchctl { step, output } => write!(
                f,
                "launchctl {step} failed (exit {}): {}",
                output
                    .code
                    .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
                output.stderr.trim()
            ),
            Self::NotReady { agent, reason } => write!(
                f,
                "the terminal launch relay agent {} is loaded but not answering ({reason}); see \
                 {} and `launchctl print gui/$(id -u)/{}`",
                agent.label,
                agent.log.display(),
                agent.label
            ),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Whether this process can load agents into the GUI domain right now.
pub fn gui_session(launchctl: &dyn Launchctl) -> Result<(), String> {
    if crate::bootstrap::probe().is_dead() {
        return Err("this process's macOS bootstrap namespace is dead".into());
    }
    let output = launchctl
        .run(&["managername".to_owned()])
        .map_err(|error| format!("launchctl is unavailable: {error}"))?;
    let manager = output.stdout.trim();
    if output.success && manager == "Aqua" {
        Ok(())
    } else if output.success {
        Err(format!(
            "this session's launchd manager is {manager:?}, not the desktop (Aqua) session"
        ))
    } else {
        Err(format!(
            "launchctl managername failed: {}",
            output.stderr.trim()
        ))
    }
}

fn is_loaded(agent: &RelayAgent, launchctl: &dyn Launchctl, uid: u32) -> io::Result<bool> {
    Ok(launchctl
        .run(&["print".to_owned(), agent.service(uid)])?
        .success)
}

fn run_step(
    launchctl: &dyn Launchctl,
    step: &'static str,
    args: Vec<String>,
) -> Result<(), InstallError> {
    let output = launchctl.run(&args)?;
    if output.success {
        Ok(())
    } else {
        Err(InstallError::Launchctl { step, output })
    }
}

fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    let temporary = path.with_extension("plist.tmp");
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, path)
}

fn wait_ready(agent: &RelayAgent, timeout: Duration) -> Result<RelayHello, RelayUnavailable> {
    let deadline = Instant::now() + timeout;
    loop {
        match relay_hello(&agent.socket) {
            Ok(hello) => return Ok(hello),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Install or refresh the agent, idempotently: an unchanged, loaded, answering agent is left
/// alone; a changed definition is booted out and loaded again; an absent one is loaded.
pub fn install(
    agent: &RelayAgent,
    launchctl: &dyn Launchctl,
    uid: u32,
    ready_timeout: Duration,
) -> Result<InstallReport, InstallError> {
    gui_session(launchctl).map_err(|reason| InstallError::NoGuiSession { reason })?;
    let desired = agent.render_plist();
    let current = fs::read_to_string(&agent.plist).ok();
    let loaded = is_loaded(agent, launchctl, uid)?;
    let outcome = if loaded && current.as_deref() == Some(desired.as_str()) {
        match wait_ready(agent, ready_timeout) {
            Ok(relay) => {
                return Ok(InstallReport {
                    outcome: InstallOutcome::AlreadyCurrent,
                    agent: agent.clone(),
                    relay,
                });
            }
            Err(_) => {
                run_step(
                    launchctl,
                    "kickstart",
                    vec!["kickstart".into(), "-k".into(), agent.service(uid)],
                )?;
                InstallOutcome::Restarted
            }
        }
    } else {
        if let Some(parent) = agent.plist.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = agent.log.parent() {
            fs::create_dir_all(parent)?;
        }
        if loaded {
            run_step(
                launchctl,
                "bootout",
                vec!["bootout".into(), agent.service(uid)],
            )?;
        }
        write_atomically(&agent.plist, &desired)?;
        run_step(
            launchctl,
            "bootstrap",
            vec![
                "bootstrap".into(),
                format!("gui/{uid}"),
                agent.plist.to_string_lossy().into_owned(),
            ],
        )?;
        if loaded {
            InstallOutcome::Updated
        } else {
            InstallOutcome::Installed
        }
    };
    match wait_ready(agent, ready_timeout) {
        Ok(relay) => Ok(InstallReport {
            outcome,
            agent: agent.clone(),
            relay,
        }),
        Err(reason) => Err(InstallError::NotReady {
            agent: Box::new(agent.clone()),
            reason,
        }),
    }
}

/// What uninstall did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UninstallOutcome {
    Removed,
    NotInstalled,
}

/// Unload and remove the agent. Running brokers keep running: each is in its own session and
/// the job abandons its process group.
pub fn uninstall(
    agent: &RelayAgent,
    launchctl: &dyn Launchctl,
    uid: u32,
) -> Result<UninstallOutcome, InstallError> {
    let loaded = is_loaded(agent, launchctl, uid)?;
    if loaded {
        run_step(
            launchctl,
            "bootout",
            vec!["bootout".into(), agent.service(uid)],
        )?;
    }
    let removed = match fs::remove_file(&agent.plist) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    Ok(if loaded || removed {
        UninstallOutcome::Removed
    } else {
        UninstallOutcome::NotInstalled
    })
}

/// The agent's observable state.
#[derive(Debug, Clone, Serialize)]
pub struct RelayStatus {
    pub agent: RelayAgent,
    pub plist_present: bool,
    pub plist_current: bool,
    pub loaded: bool,
    pub relay: Option<RelayHello>,
    pub relay_error: Option<String>,
}

impl RelayStatus {
    /// Whether a server here would launch brokers through the relay.
    pub fn healthy(&self) -> bool {
        self.plist_current
            && self.loaded
            && self
                .relay
                .as_ref()
                .is_some_and(|relay| !relay.bootstrap.is_dead())
    }
}

/// Inspect the agent without changing anything.
pub fn status(agent: &RelayAgent, launchctl: &dyn Launchctl, uid: u32) -> RelayStatus {
    let current = fs::read_to_string(&agent.plist).ok();
    let loaded = is_loaded(agent, launchctl, uid).unwrap_or(false);
    let (relay, relay_error) = match relay_hello(&agent.socket) {
        Ok(hello) => (Some(hello), None),
        Err(error) => (None, Some(error.to_string())),
    };
    RelayStatus {
        plist_present: current.is_some(),
        plist_current: current.as_deref() == Some(agent.render_plist().as_str()),
        agent: agent.clone(),
        loaded,
        relay,
        relay_error,
    }
}

#[cfg(test)]
#[path = "launch_agent_tests.rs"]
mod tests;
