//! The server's side of the terminal launch relay (HS2-HYF56Y): asking the relay to start a
//! broker, reporting why it was not used, and the relay process's own broker launcher.
//! Protocol, authentication, and the LaunchAgent live in `hotsheet_terminals`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hotsheet_terminals::launch_relay::{
    BrokerLauncher, LaunchRequest, RelayUnavailable, relay_launch,
};

use super::{broker_arguments, broker_launch};

/// The environment a directly spawned broker would inherit: this process's variables minus
/// the repository and build-pin variables `git::launch` removes and the launchd job context
/// (see [`is_launchd_context`]). Non-UTF-8 entries cannot be carried over the relay's JSON
/// protocol and are left out.
pub(super) fn forwarded_environment() -> Vec<(String, String)> {
    let probe = hotsheet_ticketing::git::launch("broker");
    let removed: std::collections::HashSet<std::ffi::OsString> = probe
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(key, _)| key.to_os_string())
        .collect();
    std::env::vars_os()
        .filter(|(key, _)| !removed.contains(key))
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .filter(|(key, _)| !is_launchd_context(key))
        .collect()
}

/// `XPC_*` variables describe the launchd job a process belongs to. libxpc honors
/// `XPC_NULL_BOOTSTRAP` at process start by discarding the inherited bootstrap port, and a
/// process in a dead namespace carries it, so forwarding it would make the relayed broker
/// dead again. The relay keeps its own `XPC_*` values instead (HS2-HYF56Y).
pub(super) fn is_launchd_context(key: &str) -> bool {
    key.starts_with("XPC_")
}

/// Ask this machine home's launch relay to start the broker that `executable` names.
pub(super) fn relay_spawn(
    executable: &Path,
    socket: &Path,
    project: &str,
) -> Result<u32, RelayUnavailable> {
    let home = hotsheet_plugins::hotsheet_home();
    let relay = hotsheet_terminals::broker_socket::relay_socket_path(&home)
        .map_err(|error| RelayUnavailable::Untrusted(error.to_string()))?;
    let journal_home =
        std::path::absolute(&home).map_err(|error| RelayUnavailable::Refused(error.to_string()))?;
    relay_launch(
        &relay,
        executable,
        LaunchRequest {
            socket: socket.to_path_buf(),
            project: project.to_owned(),
            journal_home,
            cwd: std::env::current_dir().ok(),
            env: forwarded_environment(),
        },
    )
}

/// The fallback message, shared by the log line and its test.
pub(super) fn fallback_message(reason: &RelayUnavailable) -> String {
    format!(
        "{reason}; spawning the terminal broker directly, so it inherits this server's macOS \
         bootstrap namespace. Install the launch relay from a desktop session with \
         `hotsheet-cli terminal-relay install` (HS2-HYF56Y)"
    )
}

/// Say why brokers are spawned directly. An absent relay is the normal state on a machine that
/// never installed it, so it is reported once per process unless this server's namespace is
/// dead; a relay that exists but was not used is reported every time.
pub(super) fn report_fallback(reason: &RelayUnavailable) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REPORTED_ABSENT: AtomicBool = AtomicBool::new(false);
    let dead = hotsheet_terminals::bootstrap::probe().is_dead();
    if matches!(reason, RelayUnavailable::Absent)
        && !dead
        && REPORTED_ABSENT.swap(true, Ordering::Relaxed)
    {
        return;
    }
    tracing::warn!(bootstrap_dead = dead, "{}", fallback_message(reason));
}

/// The relay's broker launcher: the same executable choice as `spawn_broker`, started from
/// the relay's own desktop-session bootstrap namespace.
struct RelayBrokerLauncher {
    executable: PathBuf,
    self_hosted: bool,
}

impl BrokerLauncher for RelayBrokerLauncher {
    fn broker(&self) -> (PathBuf, bool) {
        (self.executable.clone(), self.self_hosted)
    }

    fn launch(&self, request: &LaunchRequest) -> std::io::Result<u32> {
        use std::os::unix::process::CommandExt;
        hotsheet_terminals::broker_socket::prepare_parent(&request.socket)?;
        // Scrubbed of repository variables first; env_clear below then replaces the rest.
        let mut command = hotsheet_ticketing::git::launch(&self.executable);
        broker_arguments(
            &mut command,
            self.self_hosted,
            &request.socket,
            &request.project,
        );
        command
            .env_clear()
            .envs(
                request
                    .env
                    .iter()
                    .filter(|(key, _)| !is_launchd_context(key))
                    .map(|(key, value)| (key, value)),
            )
            // The relay's own launchd context, never the requester's (see is_launchd_context).
            .envs(
                std::env::vars_os().filter(|(key, _)| key.to_str().is_some_and(is_launchd_context)),
            )
            .env("HOTSHEET_PROCESS_JOURNAL_HOME", &request.journal_home)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(cwd) = request.cwd.as_ref().filter(|cwd| cwd.is_dir()) {
            command.current_dir(cwd);
        }
        // SAFETY: setsid is async-signal-safe and only moves the forked child into its own
        // session, so stopping or replacing the relay job never signals a running broker.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = hotsheet_process_journal::LaunchContext {
            journal: hotsheet_process_journal::ProcessJournal::at(&request.journal_home),
            supervisor: "hotsheet-terminal-relay",
            purpose: hotsheet_process_journal::Purpose::BrokerSupervisor,
            project_id: Some(request.project.clone()),
            session_id: None,
            binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
        }
        .spawn(&mut command)?;
        let pid = child.id();
        // The relay is the broker's parent: reap it so an exited broker never stays a zombie,
        // and record the observed exit in the journal.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(pid)
    }
}

/// Run the per-user terminal launch relay on `socket` until killed. launchd starts this from
/// the relay LaunchAgent in the desktop (Aqua) session.
pub async fn run_launch_relay(socket: &Path) -> std::io::Result<()> {
    let (executable, self_hosted) = broker_launch(std::env::current_exe().ok());
    hotsheet_terminals::launch_relay::run_relay_process(
        socket,
        Arc::new(RelayBrokerLauncher {
            executable,
            self_hosted,
        }),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_hyf56y_forwarded_environment_drops_what_git_launch_removes() {
        let forwarded = forwarded_environment();
        let probe = hotsheet_ticketing::git::launch("broker");
        for (key, value) in probe.get_envs() {
            if value.is_none() {
                let key = key.to_string_lossy();
                assert!(
                    forwarded.iter().all(|(name, _)| *name != key),
                    "{key} must not reach a relayed broker"
                );
            }
        }
        // The variables terminals rely on are carried.
        if let Ok(path) = std::env::var("PATH") {
            assert!(forwarded.contains(&("PATH".to_owned(), path)));
        }
        assert!(forwarded.iter().all(|(key, _)| !key.starts_with("XPC_")));
    }

    #[test]
    fn hs2_hyf56y_launchd_job_context_is_never_forwarded() {
        // A dead-namespace process carries XPC_NULL_BOOTSTRAP; a relayed broker given it would
        // discard the relay's healthy bootstrap port at startup.
        for key in ["XPC_NULL_BOOTSTRAP", "XPC_SERVICE_NAME", "XPC_FLAGS"] {
            assert!(is_launchd_context(key), "{key}");
        }
        for key in ["PATH", "HOME", "HOTSHEET_HOME", "XPCX", "MY_XPC_FLAG"] {
            assert!(!is_launchd_context(key), "{key}");
        }
    }

    #[test]
    fn hs2_hyf56y_fallback_names_the_reason_and_the_install_command() {
        let message = fallback_message(&RelayUnavailable::Mismatch {
            relay: "/Applications/Hot Sheet 2.app/Contents/MacOS/hotsheet-server".into(),
            server: "/work/target/debug/hotsheet-terminal-broker".into(),
        });
        assert!(
            message.contains("/Applications/Hot Sheet 2.app"),
            "{message}"
        );
        assert!(message.contains("target/debug"), "{message}");
        assert!(
            message.contains("spawning the terminal broker directly"),
            "{message}"
        );
        assert!(
            message.contains("hotsheet-cli terminal-relay install"),
            "{message}"
        );
    }

    #[test]
    fn hs2_hyf56y_without_a_relay_the_spawn_request_reports_absent() {
        // The test home has no relay; the server must fall back, never hang or error out.
        let socket = std::path::Path::new("/tmp/hotsheet2-hyf56y-unused.sock");
        let result = relay_spawn(Path::new("/nonexistent/hotsheet-server"), socket, "p");
        assert_eq!(result, Err(RelayUnavailable::Absent));
    }
}
