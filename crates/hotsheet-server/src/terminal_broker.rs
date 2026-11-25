//! Server ↔ **detached terminal broker** integration (`docs/05` §5.4, HS2-ERT00F). When the
//! terminal broker is enabled, the server routes its `/terminals` request/response ops through
//! a [`BrokerClient`] to a separate `hotsheet-terminal-broker` process, so terminals **survive
//! a server restart**: on restart the server just reconnects to the still-running broker (or
//! spawns one if none is live) and its terminals are still there.
//!
//! This module owns discovery + detached spawn + a per-request call helper. Broker mode serves
//! open/list/read/input/kill and the live WebSocket attach; the server bridges the broker stream
//! so output, input, viewport-size arbitration, and the busy feed survive a server restart.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hotsheet_terminals::{
    BrokerClient, BrokerIdentity, BrokerRequest, BrokerResponse, BrokerTermInfo,
};

#[path = "terminal_broker_startup.rs"]
mod startup;
use startup::{
    BROKER_STARTUP, ContenderState, STARTUP_POLL, StartupContender, StartupWindow, keep_waiting,
};

/// A project's broker coordinates: the Unix socket + the project id the broker serves.
#[derive(Debug, Clone)]
pub struct TerminalBroker {
    pub socket: PathBuf,
    pub project: String,
    restart: Arc<tokio::sync::Mutex<()>>,
    /// Set once the broker rejected `info` as unknown: it predates the op, so status reads
    /// fall back to `list` (HS2-7RRAA8). A restarted broker is the current build again, but
    /// the flag only costs a slightly larger reply, never correctness, so it is not reset.
    legacy_list_only: Arc<AtomicBool>,
}

fn unexpected(response: BrokerResponse) -> std::io::Error {
    std::io::Error::other(format!("unexpected terminal broker response: {response:?}"))
}

impl TerminalBroker {
    /// New namespaces use short private paths. An adopted legacy namespace keeps its
    /// original address across idle exits so old and new server handles cannot split.
    fn socket_for(project: &str) -> std::io::Result<PathBuf> {
        Self::socket_in(&hotsheet_plugins::hotsheet_home(), project)
    }

    /// [`Self::socket_for`] under an explicit machine home, so tests inject a directory
    /// instead of mutating the process-global `HOTSHEET_HOME` (HS2-NYZ3PS).
    fn socket_in(home: &Path, project: &str) -> std::io::Result<PathBuf> {
        hotsheet_terminals::broker_socket::select_socket(home, project)
    }

    /// Ensure a broker is running for `store_path` and return its coordinates. Connects to an
    /// existing live broker if the socket answers; otherwise spawns `hotsheet-terminal-broker`
    /// **detached** (it outlives this server) and waits for it to accept. Blocking — call once
    /// at startup.
    pub fn ensure(store_path: &Path) -> anyhow::Result<Self> {
        let project = hotsheet_tls::project_id(store_path);
        let socket = Self::socket_for(&project)?;

        let spawned = match is_live(&socket) {
            // HS2-H0WFJG: an idle broker from another build is handed over to this server's
            // build; a broker hosting terminals is always adopted, never restarted.
            Ok(true) if !hand_off_other_build(&socket, &own_broker_build()) => {
                warn_on_dead_namespace(&socket, "adopted");
                return Ok(Self {
                    socket,
                    project,
                    restart: Arc::new(tokio::sync::Mutex::new(())),
                    legacy_list_only: Arc::default(),
                });
            }
            Ok(_) => {
                if hotsheet_terminals::bootstrap::probe().is_dead() {
                    tracing::warn!(
                        "this server's macOS bootstrap namespace is dead; a terminal broker it \
                         spawns directly will inherit it (HS2-CY01TZ). The desktop-session launch \
                         relay avoids that when installed (HS2-HYF56Y); otherwise relaunch Hot \
                         Sheet from the Dock or Finder"
                    );
                }
                spawn_broker(&socket, &project)?
            }
            Err(error) => return Err(error.into()),
        };
        let mut spawned = spawned;
        match &mut spawned {
            SpawnedBroker::Direct { child, stderr } => {
                let pid = child.id();
                wait_for_startup(&socket, pid, "spawned", BROKER_STARTUP, || {
                    child_state_with_stderr(child, stderr.path())
                })?;
            }
            SpawnedBroker::Relayed { pid } => wait_for_relayed_startup(&socket, *pid)?,
        }
        warn_on_dead_namespace(&socket, "spawned");
        Ok(Self {
            socket,
            project,
            restart: Arc::new(tokio::sync::Mutex::new(())),
            legacy_list_only: Arc::default(),
        })
    }

    /// Return the already-running broker for `store_path` without spawning one.
    pub fn discover(store_path: &Path) -> Option<Self> {
        let project = hotsheet_tls::project_id(store_path);
        let socket = Self::socket_for(&project).ok()?;
        // An owned but unresponsive socket still identifies this namespace; callers must
        // not silently choose another one while its liveness is uncertain (HS2-YDD60V).
        (!matches!(is_live(&socket), Ok(false))).then_some(Self {
            socket,
            project,
            restart: Arc::new(tokio::sync::Mutex::new(())),
            legacy_list_only: Arc::default(),
        })
    }

    /// Explicitly kill every terminal retained by this project's detached broker.
    pub async fn kill_all(&self) -> anyhow::Result<usize> {
        let terminals = match self.call(BrokerRequest::List).await? {
            BrokerResponse::List { terminals } => terminals,
            BrokerResponse::Err { message } => anyhow::bail!(message),
            response => anyhow::bail!("unexpected terminal broker response: {response:?}"),
        };
        let mut count = 0;
        for terminal in terminals {
            match self.call(BrokerRequest::Kill { id: terminal.id }).await? {
                BrokerResponse::Ok => count += 1,
                BrokerResponse::NotFound => {}
                BrokerResponse::Err { message } => anyhow::bail!(message),
                response => anyhow::bail!("unexpected terminal broker response: {response:?}"),
            }
        }
        Ok(count)
    }

    /// Point at an explicit socket/project (tests, or an already-running broker).
    pub fn at(socket: impl Into<PathBuf>, project: impl Into<String>) -> Self {
        Self {
            socket: socket.into(),
            project: project.into(),
            restart: Arc::new(tokio::sync::Mutex::new(())),
            legacy_list_only: Arc::default(),
        }
    }

    /// One request/response round-trip to the broker (a fresh connection — terminal ops are
    /// infrequent, so a pooled connection isn't worth the complexity yet).
    pub async fn call(&self, req: BrokerRequest) -> std::io::Result<BrokerResponse> {
        let mut client = self
            .connect_or_restart(|| spawn_broker(&self.socket, &self.project))
            .await?;
        client.request(&req).await
    }

    /// A terminal's state without its scrollback, `None` when the broker has no such terminal
    /// (HS2-7RRAA8). Status pollers call this twice a second per terminal, so it uses the small
    /// `info` op. A broker that predates `info` (it outlives server upgrades, so the maintainer's
    /// long-lived broker is exactly that) rejects it as a bad request; this handle then remembers
    /// to use `list`, which every broker answers with each terminal's state and no scrollback.
    pub async fn info(&self, id: &str) -> std::io::Result<Option<BrokerTermInfo>> {
        if !self.legacy_list_only.load(Ordering::Relaxed) {
            match self
                .call(BrokerRequest::Info { id: id.to_string() })
                .await?
            {
                BrokerResponse::Terminal { info } => return Ok(Some(info)),
                BrokerResponse::NotFound => return Ok(None),
                BrokerResponse::Err { message } if message.starts_with("bad request") => {
                    self.legacy_list_only.store(true, Ordering::Relaxed);
                }
                other => return Err(unexpected(other)),
            }
        }
        match self.call(BrokerRequest::List).await? {
            BrokerResponse::List { terminals } => {
                Ok(terminals.into_iter().find(|terminal| terminal.id == id))
            }
            other => Err(unexpected(other)),
        }
    }

    /// Connect, or start a broker and wait for it to accept. The spawned process is kept so
    /// the wait follows it (HS2-KCM23W): a slow first launch that is still running is waited
    /// for up to the startup ceiling, and one that exited fails without waiting it out.
    async fn connect_or_restart<C: StartupContender>(
        &self,
        spawn: impl FnOnce() -> std::io::Result<C>,
    ) -> std::io::Result<BrokerClient> {
        if let Ok(client) = BrokerClient::connect(&self.socket).await {
            return Ok(client);
        }
        let _restart = self.restart.lock().await;
        if let Ok(client) = BrokerClient::connect(&self.socket).await {
            return Ok(client);
        }
        let mut contender = spawn()?;
        let started = std::time::Instant::now();
        let mut attempts = 0;
        loop {
            let error = match BrokerClient::connect(&self.socket).await {
                Ok(client) => return Ok(client),
                Err(error) => error,
            };
            attempts += 1;
            let absent = matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) && matches!(
                hotsheet_terminals::broker_socket::owner_lock_held(&self.socket),
                Ok(false)
            );
            let state = contender.state();
            if !keep_waiting(BROKER_STARTUP, attempts, started.elapsed(), &state, absent) {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!(
                        "terminal broker did not come up on {} after {} ms ({}; last connect: {error})",
                        self.socket.display(),
                        started.elapsed().as_millis(),
                        state.describe()
                    ),
                ));
            }
            tokio::time::sleep(STARTUP_POLL).await;
        }
    }
}

impl StartupContender for SpawnedBroker {
    fn state(&mut self) -> ContenderState {
        match self {
            Self::Direct { child, stderr } => child_state_with_stderr(child, stderr.path()),
            Self::Relayed { pid } => relayed_state(*pid),
        }
    }
}

fn child_state_with_stderr(
    child: &mut impl hotsheet_process_journal::ObservedChild,
    stderr_path: &Path,
) -> ContenderState {
    let state = tracked_child_state(child);
    match state {
        ContenderState::Exited(reason) => {
            use std::io::Read;
            let stderr = std::fs::File::open(stderr_path)
                .and_then(|file| {
                    let mut bytes = Vec::new();
                    file.take(4_096).read_to_end(&mut bytes)?;
                    Ok(bytes)
                })
                .unwrap_or_default();
            let stderr = String::from_utf8_lossy(&stderr);
            let stderr = stderr
                .chars()
                .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
                .collect::<String>();
            let stderr = stderr.trim();
            if stderr.is_empty() {
                ContenderState::Exited(reason)
            } else {
                ContenderState::Exited(format!("{reason}; broker stderr: {stderr}"))
            }
        }
        state => state,
    }
}

#[cfg(test)]
fn wait_for_broker_startup(
    socket: &Path,
    child: &mut impl hotsheet_process_journal::ObservedChild,
) -> anyhow::Result<()> {
    let pid = child.id();
    wait_for_startup(socket, pid, "spawned", BROKER_STARTUP, || {
        tracked_child_state(child)
    })
}

/// A relayed broker is the relay's child, not this server's (HS2-HYF56Y); its state is read
/// with a signal-0 probe instead of `wait`.
fn wait_for_relayed_startup(socket: &Path, pid: u32) -> anyhow::Result<()> {
    wait_for_startup(socket, pid, "relayed", BROKER_STARTUP, || {
        relayed_state(pid)
    })
}

fn relayed_state(pid: u32) -> ContenderState {
    // SAFETY: signal 0 only checks that the pid exists; nothing is delivered.
    if pid <= i32::MAX as u32 && unsafe { libc::kill(pid as i32, 0) } == 0 {
        ContenderState::Running("relayed broker still running".to_owned())
    } else {
        ContenderState::Exited("relayed broker exited".to_owned())
    }
}

/// Probe until the broker answers Ping, following the started process (HS2-KCM23W). A
/// healthy socket is adopted before the process is consulted, so an exited contender still
/// adopts another healthy broker (HS2-C8J6WS).
fn wait_for_startup(
    socket: &Path,
    pid: u32,
    how: &str,
    window: StartupWindow,
    mut state: impl FnMut() -> ContenderState,
) -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let mut attempts = 0;
    loop {
        let (last_probe, absent) = match is_live(socket) {
            Ok(true) => return Ok(()),
            Ok(false) => ("absent".to_owned(), true),
            Err(error) => (
                format!("{:?} (os error {:?})", error.kind(), error.raw_os_error()),
                false,
            ),
        };
        attempts += 1;
        let contender = state();
        if !keep_waiting(window, attempts, started.elapsed(), &contender, absent) {
            anyhow::bail!(
                "terminal broker did not come up on {} after {} ms ({how} pid {pid}; {}; last probe: {last_probe})",
                socket.display(),
                started.elapsed().as_millis(),
                contender.describe()
            );
        }
        std::thread::sleep(STARTUP_POLL);
    }
}

fn tracked_child_state(child: &mut impl hotsheet_process_journal::ObservedChild) -> ContenderState {
    match child.try_wait() {
        Ok(Some(status)) => ContenderState::Exited(format!("child exited: {status}")),
        Ok(None) => ContenderState::Running("child still running".to_owned()),
        Err(error) => ContenderState::Unknown(format!(
            "child status unavailable: {:?} (os error {:?})",
            error.kind(),
            error.raw_os_error()
        )),
    }
}

/// Log once when a broker's bootstrap namespace is dead (HS2-CY01TZ). The broker itself refuses
/// to spawn new terminals in that state with the same recovery text; this warning makes the
/// state visible at startup instead of at the first terminal launch.
fn warn_on_dead_namespace(socket: &Path, how: &str) {
    match broker_health(socket) {
        Ok(Some(health)) if health.is_dead() => tracing::warn!(
            broker = how,
            socket = %socket.display(),
            "{}",
            hotsheet_terminals::bootstrap::DEAD_BOOTSTRAP_MESSAGE
        ),
        Ok(_) => {}
        Err(error) => tracing::debug!(%error, "terminal broker health probe failed"),
    }
}

/// The broker's bootstrap namespace state, `None` for a broker that predates the `health` op.
fn broker_health(socket: &Path) -> std::io::Result<Option<hotsheet_terminals::BootstrapHealth>> {
    use std::io::{BufRead, Write};
    let mut stream = hotsheet_terminals::broker_socket::connect_probe(socket)?;
    let timeout = Some(std::time::Duration::from_millis(500));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    let mut line = serde_json::to_string(&BrokerRequest::Health)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reply = String::new();
    std::io::BufReader::new(stream).read_line(&mut reply)?;
    match serde_json::from_str::<BrokerResponse>(&reply) {
        Ok(BrokerResponse::Health { bootstrap }) => Ok(Some(bootstrap)),
        Ok(BrokerResponse::Err { message }) if message.starts_with("bad request") => Ok(None),
        Ok(other) => Err(unexpected(other)),
        Err(error) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
    }
}

/// One request/response round trip on a fresh blocking probe connection, bounded so an
/// unresponsive broker cannot stall startup.
fn probe_request(socket: &Path, request: &BrokerRequest) -> std::io::Result<BrokerResponse> {
    use std::io::{BufRead, Write};
    let mut stream = hotsheet_terminals::broker_socket::connect_probe(socket)?;
    let timeout = Some(std::time::Duration::from_millis(500));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reply = String::new();
    std::io::BufReader::new(stream).read_line(&mut reply)?;
    serde_json::from_str(&reply)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// The build a server starts its brokers from (HS2-H0WFJG).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrokerBuild {
    pub(crate) executable: PathBuf,
    pub(crate) revision: Option<String>,
}

fn own_broker_build() -> BrokerBuild {
    BrokerBuild {
        executable: broker_launch(std::env::current_exe().ok()).0,
        revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }
}

/// Whether a running broker comes from another build than `own` (HS2-H0WFJG). An unknown
/// executable (a broker that predates `identity`) counts as the same build: the broker
/// protocol stays compatible across versions, so adopting it is safe.
pub(crate) fn runs_other_build(identity: &BrokerIdentity, own: &BrokerBuild) -> bool {
    let Some(executable) = identity.executable.as_deref().map(Path::new) else {
        return false;
    };
    let same_file = executable == own.executable
        || hotsheet_sync::paths::canonicalize(executable)
            .ok()
            .zip(hotsheet_sync::paths::canonicalize(&own.executable).ok())
            .is_some_and(|(theirs, ours)| theirs == ours);
    !same_file
        || matches!(
            (identity.revision.as_deref(), own.revision.as_deref()),
            (Some(theirs), Some(ours)) if theirs != ours
        )
}

/// HS2-H0WFJG: when the live broker on `socket` runs another build than `own` (for example a
/// development build that started the machine server before the pinned production host
/// adopted it) and hosts no terminals, ask it to retire and wait for its socket to go, so
/// the caller starts a broker from its own build. Returns whether it retired.
///
/// Live terminals are never ended or moved: a broker hosting any keeps running, and the
/// next server request after its ordinary idle exit starts a broker from the current build.
fn hand_off_other_build(socket: &Path, own: &BrokerBuild) -> bool {
    let identity = match probe_request(socket, &BrokerRequest::Identity) {
        Ok(BrokerResponse::Identity { identity }) => identity,
        Ok(BrokerResponse::Err { message }) if message.starts_with("bad request") => {
            return false;
        }
        Ok(other) => {
            tracing::debug!(?other, "unexpected terminal broker identity reply");
            return false;
        }
        Err(error) => {
            tracing::debug!(%error, "terminal broker identity probe failed");
            return false;
        }
    };
    if !runs_other_build(&identity, own) {
        return false;
    }
    let keep = |terminals: usize| {
        tracing::info!(
            broker_pid = identity.pid,
            broker_executable = identity.executable.as_deref().unwrap_or_default(),
            terminals,
            "keeping a terminal broker from another build because it hosts live terminals; \
             it is replaced after its idle exit (HS2-H0WFJG)"
        );
        false
    };
    if identity.terminals > 0 {
        return keep(identity.terminals);
    }
    // The probe connections just made, or a previous server's streams, may still be closing.
    for _ in 0..20 {
        match probe_request(socket, &BrokerRequest::RetireIfIdle) {
            Ok(BrokerResponse::Retired) => {
                for _ in 0..100 {
                    if matches!(is_live(socket), Ok(false)) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                tracing::info!(
                    broker_pid = identity.pid,
                    broker_executable = identity.executable.as_deref().unwrap_or_default(),
                    "replaced an idle terminal broker from another build (HS2-H0WFJG)"
                );
                return true;
            }
            Ok(BrokerResponse::Busy { terminals, .. }) if terminals > 0 => return keep(terminals),
            Ok(BrokerResponse::Busy { .. }) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(_) | Err(_) => return false,
        }
    }
    false
}

/// A missing/refused socket is confirmed absent. Connect timeout, permission errors,
/// and an accepting but unresponsive listener remain uncertain (HS2-YDD60V).
fn is_live(socket: &Path) -> std::io::Result<bool> {
    use std::io::{BufRead, Write};
    let mut stream = match hotsheet_terminals::broker_socket::connect_probe(socket) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            if hotsheet_terminals::broker_socket::owner_lock_held(socket)? {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "broker owns its socket but is not accepting connections",
                ));
            }
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    let timeout = Some(std::time::Duration::from_millis(500));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    let mut line = serde_json::to_string(&BrokerRequest::Ping)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reply = String::new();
    std::io::BufReader::new(stream).read_line(&mut reply)?;
    match serde_json::from_str::<BrokerResponse>(&reply) {
        Ok(BrokerResponse::Pong) => Ok(true),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "broker did not answer Ping with Pong",
        )),
        Err(error) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
    }
}

/// A broker this server started: its own child, or one the launch relay started for it.
enum SpawnedBroker {
    Direct {
        child: hotsheet_process_journal::DetachedChild,
        stderr: tempfile::NamedTempFile,
    },
    Relayed {
        pid: u32,
    },
}

/// Spawn the broker detached with stdio disconnected from the server. Prefer the dedicated
/// sibling binary; a server-only development build falls back to the server's hidden broker
/// process mode so making detached hosting the default does not add a packaging footgun.
///
/// On macOS the per-user launch relay is asked first, so the broker (and every terminal it
/// hosts) holds the desktop session's bootstrap namespace rather than this server's, which
/// may be dead (HS2-HYF56Y). Without a usable relay the server spawns directly, as before.
fn spawn_broker(socket: &Path, project: &str) -> std::io::Result<SpawnedBroker> {
    hotsheet_terminals::broker_socket::prepare_parent(socket)?;
    let current = std::env::current_exe().ok();
    let (executable, self_hosted) = broker_launch(current);
    // Only macOS has bootstrap namespaces; elsewhere the direct spawn is already right.
    if cfg!(target_os = "macos") {
        match launch_relay::relay_spawn(&executable, socket, project) {
            Ok(pid) => {
                tracing::info!(
                    pid,
                    socket = %socket.display(),
                    "terminal broker launched by the desktop-session launch relay (HS2-HYF56Y)"
                );
                return Ok(SpawnedBroker::Relayed { pid });
            }
            Err(reason) => launch_relay::report_fallback(&reason),
        }
    }
    let mut command = hotsheet_ticketing::git::launch(executable);
    broker_arguments(&mut command, self_hosted, socket, project);
    command.env(
        "HOTSHEET_PROCESS_JOURNAL_HOME",
        hotsheet_plugins::hotsheet_home(),
    );
    command.env(hotsheet_terminals::STARTUP_STDERR_ENV, "1");
    let stderr = tempfile::Builder::new()
        .prefix("broker-startup-")
        .tempfile_in(socket.parent().expect("prepared broker socket parent"))?;
    hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(hotsheet_plugins::hotsheet_home()),
        supervisor: "hotsheet-server",
        purpose: hotsheet_process_journal::Purpose::BrokerSupervisor,
        project_id: Some(project.to_owned()),
        session_id: None,
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }
    // Retaining the handle permits startup diagnostics. Dropping it does not wait or
    // kill: the broker keeps its existing detached lifetime on successful startup, so it
    // inherits no stray descriptor of the server's (HS2-ETMKCW).
    .spawn_detached_with_stderr_file(&mut command, stderr.path())
    .map(|child| SpawnedBroker::Direct { child, stderr })
}

/// The broker's command-line arguments for a sibling broker binary or a self-hosting server.
fn broker_arguments(
    command: &mut std::process::Command,
    self_hosted: bool,
    socket: &Path,
    project: &str,
) {
    if self_hosted {
        command
            .arg("--terminal-broker-process")
            .arg(socket)
            .arg("--terminal-broker-project")
            .arg(project);
    } else {
        command.arg(socket).arg(project);
    }
}

#[cfg(unix)]
#[path = "terminal_launch_relay.rs"]
mod launch_relay;
#[cfg(unix)]
pub use launch_relay::run_launch_relay;

fn broker_launch(current: Option<PathBuf>) -> (PathBuf, bool) {
    let sibling = current
        .as_ref()
        .and_then(|path| {
            path.parent()
                .map(|dir| dir.join("hotsheet-terminal-broker"))
        })
        .filter(|path| path.exists());
    if let Some(executable) = sibling {
        (executable, false)
    } else if let Some(exe) = current {
        (exe, true)
    } else {
        (PathBuf::from("hotsheet-terminal-broker"), false)
    }
}

#[cfg(all(test, unix))]
#[path = "terminal_broker_startup_tests.rs"]
mod startup_diagnostic_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_terminals::{
        BrokerClient, BrokerRequest, BrokerResponse, TerminalManager, serve_broker,
    };
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A hermetic machine home for the legacy-namespace tests. The legacy socket lives at
    /// `<home>/broker/project.sock`, so the home must be short enough for a Unix socket
    /// path (`sun_path` is 104 bytes on macOS); a long `$TMPDIR` falls back to `/tmp`.
    fn legacy_home() -> tempfile::TempDir {
        let base = std::env::temp_dir();
        let base = if base.as_os_str().len() <= 60 {
            base
        } else {
            std::path::PathBuf::from("/tmp")
        };
        tempfile::Builder::new()
            .prefix("hs-legacy-")
            .tempdir_in(base)
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn legacy_namespace_stays_pinned_across_idle_exit_and_interleaved_restarts() {
        let home = legacy_home();
        let parent = home.path().join("broker");
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let legacy = parent.join("project.sock");
        let listener = tokio::net::UnixListener::bind(&legacy).unwrap();
        let task = tokio::spawn(serve_broker(
            listener,
            "project".into(),
            Arc::new(TerminalManager::new()),
        ));
        assert_eq!(
            TerminalBroker::socket_in(home.path(), "project").unwrap(),
            legacy
        );
        let retained = TerminalBroker::at(&legacy, "project");
        assert_eq!(
            std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
            0o700
        );
        task.abort();
        let _ = task.await;
        std::fs::remove_file(&legacy).unwrap();
        // Idle cleanup removed the socket, but the ownership marker pins the address.
        let fresh = TerminalBroker::at(
            TerminalBroker::socket_in(home.path(), "project").unwrap(),
            "project",
        );
        assert_eq!(fresh.socket, retained.socket);
        for (first, second) in [(&fresh, &retained), (&retained, &fresh)] {
            let listener = tokio::net::UnixListener::bind(&first.socket).unwrap();
            let task = tokio::spawn(serve_broker(
                listener,
                "project".into(),
                Arc::new(TerminalManager::new()),
            ));
            let mut client = second
                .connect_or_restart(|| -> std::io::Result<()> {
                    panic!("the other handle already owns the restarted broker")
                })
                .await
                .unwrap();
            assert!(matches!(
                client.request(&BrokerRequest::Ping).await.unwrap(),
                BrokerResponse::Pong
            ));
            assert_eq!(
                TerminalBroker::socket_in(home.path(), "project").unwrap(),
                legacy
            );
            drop(client);
            task.abort();
            let _ = task.await;
            std::fs::remove_file(&legacy).unwrap();
        }
    }

    #[test]
    fn accepting_legacy_without_pong_does_not_choose_a_new_namespace() {
        let home = legacy_home();
        let parent = home.path().join("broker");
        std::fs::create_dir(&parent).unwrap();
        let legacy = parent.join("project.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&legacy).unwrap();
        // No accept/Pong loop exists: selection must depend on ownership, not health.
        assert_eq!(
            TerminalBroker::socket_in(home.path(), "project").unwrap(),
            legacy
        );
        assert!(legacy.with_extension("lock").is_file());
    }

    #[test]
    fn hs2_ydd60v_ping_distinguishes_refusal_from_an_unresponsive_owner() {
        let home = legacy_home();
        let socket = home.path().join("broker.sock");
        assert!(!is_live(&socket).unwrap());
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let started = std::time::Instant::now();
        let error = is_live(&socket).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
            "{error}"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        drop(listener);
        assert!(!is_live(&socket).unwrap());
    }

    #[test]
    fn hs2_ydd60v_full_backlog_refusal_does_not_mark_owned_broker_absent() {
        let home = legacy_home();
        let socket = home.path().join("broker.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        // SAFETY: listener owns a listening descriptor; reducing its backlog is valid.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 1) }, 0);
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(socket.with_extension("lock"))
            .unwrap();
        // SAFETY: lock owns a valid descriptor until this test drops it.
        assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);
        let mut queued = Vec::new();
        for _ in 0..16 {
            match hotsheet_terminals::broker_socket::connect_probe(&socket) {
                Ok(stream) => queued.push(stream),
                Err(_) => break,
            }
        }
        assert!(!queued.is_empty());
        let started = std::time::Instant::now();
        let error = is_live(&socket).unwrap_err();
        // Linux can report ENOTCONN for a saturated listener; it remains uncertain.
        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::NotConnected
            ),
            "{error}"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        drop(queued);
        drop(listener);
        drop(lock);
        assert!(!is_live(&socket).unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_call_connection_restarts_an_idle_broker_with_a_missing_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("broker.sock");
        let broker = TerminalBroker::at(&socket, "proj");
        let launches = Arc::new(AtomicUsize::new(0));
        let launched = launches.clone();
        let launch_socket = socket.clone();

        let mut client = broker
            .connect_or_restart(move || {
                launched.fetch_add(1, Ordering::SeqCst);
                let listener = tokio::net::UnixListener::bind(&launch_socket)?;
                tokio::spawn(serve_broker(
                    listener,
                    "proj".into(),
                    Arc::new(TerminalManager::new()),
                ));
                Ok(())
            })
            .await
            .unwrap();

        assert!(matches!(
            client.request(&BrokerRequest::Ping).await.unwrap(),
            BrokerResponse::Pong
        ));
        assert_eq!(launches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn kill_all_uses_the_existing_broker_protocol_and_removes_every_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("broker.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        tokio::spawn(serve_broker(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
        ));
        let mut client = BrokerClient::connect(&socket).await.unwrap();
        for id in ["one", "two"] {
            client
                .request(&BrokerRequest::Open {
                    kind: hotsheet_terminals::TerminalKind::Shell,
                    id: id.into(),
                    command: "cat".into(),
                    args: vec![],
                    cwd: None,
                    env: vec![],
                    replay_bytes: None,
                    push: None,
                })
                .await
                .unwrap();
        }
        let broker = TerminalBroker::at(&socket, "proj");
        assert_eq!(broker.kill_all().await.unwrap(), 2);
        assert!(
            matches!(broker.call(BrokerRequest::List).await.unwrap(),BrokerResponse::List { terminals } if terminals.is_empty())
        );
    }

    /// A scripted broker that records each request's `op` and answers it like a broker of the
    /// given age: a legacy broker rejects `info` exactly as the real dispatcher rejects an
    /// unknown op.
    fn scripted_broker(socket: &Path, legacy: bool) -> Arc<std::sync::Mutex<Vec<String>>> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let ops = Arc::new(std::sync::Mutex::new(Vec::new()));
        let listener = tokio::net::UnixListener::bind(socket).unwrap();
        let recorded = ops.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let recorded = recorded.clone();
                tokio::spawn(async move {
                    let (read, mut write) = stream.into_split();
                    let mut lines = tokio::io::BufReader::new(read).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                        let op = request["op"].as_str().unwrap().to_string();
                        recorded.lock().unwrap().push(op.clone());
                        let info = serde_json::json!({ "id": "t1", "alive": true, "busy": true });
                        let reply = match (op.as_str(), legacy) {
                            ("info", false) => {
                                serde_json::json!({ "result": "terminal", "info": info })
                            }
                            ("info", true) => serde_json::json!({
                                "result": "err",
                                "message": "bad request: unknown variant `info`"
                            }),
                            ("read", _) => serde_json::json!({
                                "result": "read",
                                "info": info,
                                "scrollback": vec![b'x'; 4096],
                            }),
                            ("list", _) => {
                                serde_json::json!({ "result": "list", "terminals": [info] })
                            }
                            _ => serde_json::json!({ "result": "not_found" }),
                        };
                        let mut out = reply.to_string();
                        out.push('\n');
                        if write.write_all(out.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        ops
    }

    /// The status pollers' broker call never ships the scrollback on a current broker, and
    /// keeps working against a broker that predates `info` (HS2-7RRAA8). A broker outlives
    /// server upgrades, so both directions of that version skew are live.
    #[tokio::test]
    async fn info_skips_the_scrollback_and_falls_back_once_for_a_legacy_broker() {
        let dir = tempfile::tempdir().unwrap();
        let current_socket = dir.path().join("current.sock");
        let current_ops = scripted_broker(&current_socket, false);
        let current = TerminalBroker::at(&current_socket, "proj");
        for _ in 0..3 {
            let info = current.info("t1").await.unwrap().unwrap();
            assert!(info.alive && info.busy);
        }
        assert_eq!(*current_ops.lock().unwrap(), ["info", "info", "info"]);

        let legacy_socket = dir.path().join("legacy.sock");
        let legacy_ops = scripted_broker(&legacy_socket, true);
        let legacy = TerminalBroker::at(&legacy_socket, "proj");
        // A clone shares the learned fallback, as the per-terminal pollers' clones do.
        let poller = legacy.clone();
        assert!(legacy.info("t1").await.unwrap().unwrap().busy);
        assert!(poller.info("t1").await.unwrap().unwrap().busy);
        assert!(legacy.info("missing").await.unwrap().is_none());
        // Never `read`: an old broker's scrollback stays out of the poll path too.
        assert_eq!(
            *legacy_ops.lock().unwrap(),
            ["info", "list", "list", "list"]
        );
    }

    #[tokio::test]
    async fn info_reports_a_missing_terminal_as_none_on_a_real_broker() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("broker.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        tokio::spawn(serve_broker(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
        ));
        let broker = TerminalBroker::at(&socket, "proj");
        assert!(broker.info("nope").await.unwrap().is_none());
        let mut client = BrokerClient::connect(&socket).await.unwrap();
        client
            .request(&BrokerRequest::Open {
                kind: hotsheet_terminals::TerminalKind::Shell,
                id: "t1".into(),
                command: "cat".into(),
                args: vec![],
                cwd: None,
                env: vec![],
                replay_bytes: None,
                push: None,
            })
            .await
            .unwrap();
        let info = broker.info("t1").await.unwrap().unwrap();
        assert!(info.alive);
        assert!(!broker.legacy_list_only.load(Ordering::Relaxed));
    }

    /// A one-shot broker that answers the first request line with `reply`.
    fn one_reply_broker(socket: &Path, reply: &'static str) -> std::thread::JoinHandle<String> {
        use std::io::{BufRead, Write};
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            std::io::BufReader::new(&stream)
                .read_line(&mut request)
                .unwrap();
            (&stream).write_all(reply.as_bytes()).unwrap();
            (&stream).write_all(b"\n").unwrap();
            request
        })
    }

    #[test]
    fn hs2_cy01tz_broker_health_reads_dead_and_tolerates_a_legacy_broker() {
        let home = legacy_home();
        let dead = home.path().join("dead.sock");
        let served = one_reply_broker(
            &dead,
            r#"{"result":"health","bootstrap":{"state":"dead","code":268435459}}"#,
        );
        let health = broker_health(&dead).unwrap().unwrap();
        assert!(health.is_dead(), "{health:?}");
        assert_eq!(served.join().unwrap().trim(), r#"{"op":"health"}"#);

        // A broker that predates the op rejects it as a bad request: unknown, not an error.
        let legacy = home.path().join("legacy.sock");
        let served = one_reply_broker(
            &legacy,
            r#"{"result":"err","message":"bad request: unknown variant `health`"}"#,
        );
        assert_eq!(broker_health(&legacy).unwrap(), None);
        served.join().unwrap();

        // The adoption warning path never fails startup, whatever the broker answers.
        let missing = home.path().join("missing.sock");
        assert!(broker_health(&missing).is_err());
        warn_on_dead_namespace(&missing, "adopted");
    }

    #[test]
    fn server_only_build_uses_the_server_as_its_broker_process_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let server = dir.path().join("hotsheet-server");
        std::fs::write(&server, "").unwrap();
        assert_eq!(broker_launch(Some(server.clone())), (server.clone(), true));
        let sibling = dir.path().join("hotsheet-terminal-broker");
        std::fs::write(&sibling, "").unwrap();
        assert_eq!(broker_launch(Some(server)), (sibling, false));
    }

    #[test]
    fn hs2_h0wfjg_only_a_known_other_executable_or_revision_is_another_build() {
        let dir = tempfile::tempdir().unwrap();
        let pinned = dir.path().join("pinned/hotsheet-server");
        let dev = dir.path().join("dev/hotsheet-server");
        for path in [&pinned, &dev] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let link = dir.path().join("link-to-pinned");
        std::os::unix::fs::symlink(&pinned, &link).unwrap();
        let own = BrokerBuild {
            executable: pinned.clone(),
            revision: Some("r2".into()),
        };
        let identity = |executable: Option<&Path>, revision: Option<&str>| BrokerIdentity {
            pid: 1,
            executable: executable.map(|path| path.display().to_string()),
            revision: revision.map(str::to_owned),
            terminals: 0,
        };
        // (broker executable, broker revision, another build)
        let cases: [(Option<&Path>, Option<&str>, bool); 7] = [
            // A broker that predates `identity` or cannot name its executable: adopt.
            (None, None, false),
            (None, Some("r1"), false),
            (Some(&pinned), Some("r2"), false),
            (Some(&pinned), None, false),
            // The same file through a symlink is the same build.
            (Some(&link), Some("r2"), false),
            (Some(&dev), Some("r2"), true),
            // Rebuilt in place: same path, another stamped revision.
            (Some(&pinned), Some("r1"), true),
        ];
        for (executable, revision, expected) in cases {
            assert_eq!(
                runs_other_build(&identity(executable, revision), &own),
                expected,
                "{executable:?} {revision:?}"
            );
        }
        // An unstamped server compares executables only.
        let unstamped = BrokerBuild {
            revision: None,
            ..own
        };
        assert!(!runs_other_build(
            &identity(Some(&pinned), Some("r1")),
            &unstamped
        ));
    }

    /// HS2-H0WFJG: the server-side hand-off against a real detached-lifetime broker. A broker
    /// of this build is adopted; one of another build is kept while it hosts a terminal and
    /// retired once it is idle.
    #[tokio::test(flavor = "multi_thread")]
    async fn hs2_h0wfjg_hand_off_keeps_live_terminals_and_retires_only_an_idle_other_build() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("broker.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let task = tokio::spawn(hotsheet_terminals::serve_broker_with_idle(
            listener,
            "proj".into(),
            Arc::new(TerminalManager::new()),
            Some(std::time::Duration::from_secs(3600)),
        ));
        let this_build = BrokerBuild {
            executable: std::env::current_exe().unwrap(),
            revision: None,
        };
        let other_build = BrokerBuild {
            executable: dir.path().join("pinned/hotsheet-server"),
            revision: None,
        };
        let blocking = |socket: PathBuf, build: BrokerBuild| {
            tokio::task::spawn_blocking(move || hand_off_other_build(&socket, &build))
        };
        // Same build: adopted, never retired.
        assert!(!blocking(socket.clone(), this_build).await.unwrap());
        // Another build hosting a live terminal: kept, and the terminal keeps running.
        let mut client = BrokerClient::connect(&socket).await.unwrap();
        client
            .request(&BrokerRequest::Open {
                id: "live".into(),
                kind: Default::default(),
                command: "cat".into(),
                args: vec![],
                cwd: None,
                env: vec![],
                replay_bytes: None,
                push: None,
            })
            .await
            .unwrap();
        drop(client);
        assert!(!blocking(socket.clone(), other_build.clone()).await.unwrap());
        let mut client = BrokerClient::connect(&socket).await.unwrap();
        match client.request(&BrokerRequest::List).await.unwrap() {
            BrokerResponse::List { terminals } => {
                assert_eq!(terminals.len(), 1);
                assert!(terminals[0].alive);
            }
            other => panic!("unexpected {other:?}"),
        }
        // The terminal ends: the idle broker of another build is retired and its socket goes.
        client
            .request(&BrokerRequest::Kill { id: "live".into() })
            .await
            .unwrap();
        drop(client);
        assert!(blocking(socket.clone(), other_build).await.unwrap());
        tokio::time::timeout(std::time::Duration::from_secs(3), task)
            .await
            .expect("the retired broker stopped serving")
            .unwrap();
        assert!(matches!(is_live(&socket), Ok(false)));
    }
}
