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

use hotsheet_terminals::{BrokerClient, BrokerRequest, BrokerResponse, BrokerTermInfo};

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

        match is_live(&socket) {
            Ok(true) => {
                return Ok(Self {
                    socket,
                    project,
                    restart: Arc::new(tokio::sync::Mutex::new(())),
                    legacy_list_only: Arc::default(),
                });
            }
            Ok(false) => spawn_broker(&socket, &project)?,
            Err(error) => return Err(error.into()),
        }
        // Wait (briefly) for the freshly-spawned broker to bind + accept.
        for _ in 0..40 {
            if is_live(&socket).unwrap_or(false) {
                return Ok(Self {
                    socket,
                    project,
                    restart: Arc::new(tokio::sync::Mutex::new(())),
                    legacy_list_only: Arc::default(),
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        anyhow::bail!("terminal broker did not come up on {}", socket.display())
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

    async fn connect_or_restart(
        &self,
        spawn: impl FnOnce() -> std::io::Result<()>,
    ) -> std::io::Result<BrokerClient> {
        if let Ok(client) = BrokerClient::connect(&self.socket).await {
            return Ok(client);
        }
        let _restart = self.restart.lock().await;
        if let Ok(client) = BrokerClient::connect(&self.socket).await {
            return Ok(client);
        }
        spawn()?;
        let mut last_error = None;
        for _ in 0..40 {
            match BrokerClient::connect(&self.socket).await {
                Ok(client) => return Ok(client),
                Err(error) => last_error = Some(error),
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Err(last_error.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "terminal broker did not come up on {}",
                    self.socket.display()
                ),
            )
        }))
    }
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

/// Spawn the broker detached with stdio disconnected from the server. Prefer the dedicated
/// sibling binary; a server-only development build falls back to the server's hidden broker
/// process mode so making detached hosting the default does not add a packaging footgun.
fn spawn_broker(socket: &Path, project: &str) -> std::io::Result<()> {
    hotsheet_terminals::broker_socket::prepare_parent(socket)?;
    let current = std::env::current_exe().ok();
    let (executable, self_hosted) = broker_launch(current);
    let mut command = hotsheet_ticketing::git::launch(executable);
    if self_hosted {
        command
            .arg("--terminal-broker-process")
            .arg(socket)
            .arg("--terminal-broker-project")
            .arg(project);
    } else {
        command.arg(socket).arg(project);
    }
    command.env(
        "HOTSHEET_PROCESS_JOURNAL_HOME",
        hotsheet_plugins::hotsheet_home(),
    );
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(hotsheet_plugins::hotsheet_home()),
        supervisor: "hotsheet-server",
        purpose: hotsheet_process_journal::Purpose::BrokerSupervisor,
        project_id: Some(project.to_owned()),
        session_id: None,
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }
    .detached(&mut command)?; // not waited on → outlives this server (reparented on exit)
    Ok(())
}

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
                .connect_or_restart(|| panic!("the other handle already owns the restarted broker"))
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
}
