//! Terminal **launch relay** (HS2-HYF56Y): a small per-user process that launches detached
//! terminal brokers from a healthy macOS bootstrap namespace.
//!
//! A broker inherits the Mach bootstrap port of whatever launched it. When the server's
//! launching context dies, every broker it spawns, and every terminal those brokers spawn,
//! inherit a dead namespace (HS2-CY01TZ). `launchctl asuser` cannot repair that from inside
//! the dead namespace, so a relay that launchd already runs in the per-user GUI (Aqua) domain
//! does the spawning instead. The server asks it over a user-only Unix socket; when the relay
//! is absent or untrusted the server falls back to its own direct spawn and says so.
//!
//! The relay launches only its own broker executable (the [`BrokerLauncher`] decides which),
//! never a program named by the client, so it adds no capability beyond what a same-user
//! process already has through the broker sockets themselves.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

use crate::bootstrap::BootstrapHealth;

/// Relay wire protocol version, reported by `hello`.
pub const RELAY_PROTOCOL: u32 = 1;

/// One request line may carry the server's environment; anything larger is refused.
pub const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

/// A `hello` round-trip on an owned socket answers at once; a slower peer is unhealthy.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(2);

/// A launch forks and execs one process; the broker's own readiness is waited separately.
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);

/// What the server asks the relay to launch: the broker's socket and project, where the
/// process journal lives, and the server's own working directory and environment so a
/// relayed broker (and its terminals) see the same `PATH` and variables a directly spawned
/// one would. Only the bootstrap namespace differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchRequest {
    pub socket: PathBuf,
    pub project: String,
    pub journal_home: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

impl LaunchRequest {
    /// Reject requests the relay must never act on. The relay runs with the user's full
    /// authority, so paths must be absolute and names must be plain.
    pub fn validate(&self) -> Result<(), String> {
        if !self.socket.is_absolute() {
            return Err("broker socket must be an absolute path".into());
        }
        if !self.journal_home.is_absolute() {
            return Err("journal home must be an absolute path".into());
        }
        if self.project.is_empty() || self.project.contains(['\0', '\n']) {
            return Err("project id must be a non-empty single line".into());
        }
        if let Some(cwd) = &self.cwd
            && !cwd.is_absolute()
        {
            return Err("working directory must be an absolute path".into());
        }
        if self
            .env
            .iter()
            .any(|(key, value)| key.is_empty() || key.contains(['=', '\0']) || value.contains('\0'))
        {
            return Err("environment entries must be valid variable assignments".into());
        }
        Ok(())
    }
}

/// A request line on the relay socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RelayRequest {
    Hello,
    Launch { request: LaunchRequest },
}

/// A response line on the relay socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum RelayResponse {
    Hello {
        protocol: u32,
        pid: u32,
        /// The executable this relay launches for every broker request.
        broker: PathBuf,
        /// Whether that executable is a server binary hosting the broker itself.
        self_hosted: bool,
        /// The relay's own namespace, which every relayed broker inherits.
        bootstrap: BootstrapHealth,
    },
    Launched {
        pid: u32,
    },
    Err {
        message: String,
    },
}

/// Launches a broker for the relay. The executable is fixed by the implementation.
pub trait BrokerLauncher: Send + Sync + 'static {
    /// The broker executable and whether it is a self-hosting server binary.
    fn broker(&self) -> (PathBuf, bool);
    /// Spawn one detached broker and return its pid.
    fn launch(&self, request: &LaunchRequest) -> io::Result<u32>;
}

/// Bind `socket` exclusively and serve relay requests until killed (or, in tests, until the
/// owning test process exits).
pub async fn run_relay_process(socket: &Path, launcher: Arc<dyn BrokerLauncher>) -> io::Result<()> {
    let (listener, _ownership) = crate::broker_socket::bind(socket)?;
    let serve = serve_relay(listener, launcher);
    if let Some(owner_pid) = crate::broker::test_server_owner_pid()? {
        tokio::select! {
            () = serve => {},
            () = crate::broker::wait_for_test_server_owner_exit(owner_pid) => {},
        }
    } else {
        serve.await;
    }
    Ok(())
}

/// Serve relay requests on `listener`. Only peers running as this process's effective user
/// are answered; any other connection is closed without a reply.
pub async fn serve_relay(listener: UnixListener, launcher: Arc<dyn BrokerLauncher>) {
    let me = effective_uid();
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        match stream.peer_cred() {
            Ok(credentials) if credentials.uid() == me => {}
            _ => continue,
        }
        let launcher = Arc::clone(&launcher);
        tokio::spawn(async move {
            let _ = handle_connection(stream, launcher).await;
        });
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    launcher: Arc<dyn BrokerLauncher>,
) -> io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(read);
    loop {
        let mut line = String::new();
        let read = (&mut reader)
            .take(MAX_REQUEST_BYTES)
            .read_line(&mut line)
            .await?;
        if read == 0 {
            return Ok(());
        }
        if !line.ends_with('\n') {
            let reply = RelayResponse::Err {
                message: "request too large or truncated".into(),
            };
            write_reply(&mut write, &reply).await?;
            return Ok(());
        }
        let reply = match serde_json::from_str::<RelayRequest>(&line) {
            Ok(RelayRequest::Hello) => {
                let (broker, self_hosted) = launcher.broker();
                RelayResponse::Hello {
                    protocol: RELAY_PROTOCOL,
                    pid: std::process::id(),
                    broker,
                    self_hosted,
                    bootstrap: crate::bootstrap::probe(),
                }
            }
            Ok(RelayRequest::Launch { request }) => match request.validate() {
                Err(message) => RelayResponse::Err { message },
                Ok(()) => {
                    let launcher = Arc::clone(&launcher);
                    match tokio::task::spawn_blocking(move || launcher.launch(&request)).await {
                        Ok(Ok(pid)) => RelayResponse::Launched { pid },
                        Ok(Err(error)) => RelayResponse::Err {
                            message: format!("broker launch failed: {error}"),
                        },
                        Err(error) => RelayResponse::Err {
                            message: format!("broker launch task failed: {error}"),
                        },
                    }
                }
            },
            Err(error) => RelayResponse::Err {
                message: format!("bad request: {error}"),
            },
        };
        write_reply(&mut write, &reply).await?;
    }
}

async fn write_reply(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    reply: &RelayResponse,
) -> io::Result<()> {
    let mut line = serde_json::to_string(reply)?;
    line.push('\n');
    write.write_all(line.as_bytes()).await
}

/// Why the server did not use the relay. Every variant falls back to the direct spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayUnavailable {
    /// No relay socket, or a stale one nobody accepts on.
    Absent,
    /// The socket, its directory, or the peer is not owned by this user.
    Untrusted(String),
    /// An owned relay that did not answer, or answered garbage.
    Unresponsive(String),
    /// The relay launches a different broker build than this server would.
    Mismatch { relay: PathBuf, server: PathBuf },
    /// The relay answered but could not launch the broker.
    Refused(String),
}

impl std::fmt::Display for RelayUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Absent => write!(f, "no terminal launch relay is running"),
            Self::Untrusted(reason) => write!(f, "terminal launch relay is not trusted: {reason}"),
            Self::Unresponsive(reason) => {
                write!(f, "terminal launch relay did not answer: {reason}")
            }
            Self::Mismatch { relay, server } => write!(
                f,
                "terminal launch relay launches {} but this server uses {}",
                relay.display(),
                server.display()
            ),
            Self::Refused(reason) => write!(f, "terminal launch relay refused: {reason}"),
        }
    }
}

/// The relay's `hello` reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelayHello {
    pub protocol: u32,
    pub pid: u32,
    pub broker: PathBuf,
    pub self_hosted: bool,
    pub bootstrap: BootstrapHealth,
}

/// An authenticated connection to a relay.
pub struct RelayConnection {
    reader: BufReader<std::os::unix::net::UnixStream>,
}

impl RelayConnection {
    /// Connect to `socket` only if it is a socket owned by this user in a directory no one
    /// else can write, and the process accepting on it runs as this user (HS2-HYF56Y).
    pub fn connect(socket: &Path) -> Result<Self, RelayUnavailable> {
        authenticate_path(socket)?;
        let stream = match crate::broker_socket::connect_probe(socket) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                return Err(RelayUnavailable::Absent);
            }
            Err(error) => return Err(RelayUnavailable::Unresponsive(error.to_string())),
        };
        match peer_uid(&stream) {
            Ok(uid) if uid == effective_uid() => {}
            Ok(uid) => {
                return Err(RelayUnavailable::Untrusted(format!(
                    "the relay process runs as uid {uid}"
                )));
            }
            Err(error) => {
                return Err(RelayUnavailable::Untrusted(format!(
                    "could not read the relay's credentials: {error}"
                )));
            }
        }
        Ok(Self {
            reader: BufReader::new(stream),
        })
    }

    fn round_trip(
        &mut self,
        request: &RelayRequest,
        timeout: Duration,
    ) -> Result<RelayResponse, RelayUnavailable> {
        let unresponsive = |error: io::Error| RelayUnavailable::Unresponsive(error.to_string());
        let stream = self.reader.get_mut();
        stream
            .set_read_timeout(Some(timeout))
            .map_err(unresponsive)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(unresponsive)?;
        let mut line = serde_json::to_string(request)
            .map_err(|error| RelayUnavailable::Unresponsive(error.to_string()))?;
        line.push('\n');
        stream.write_all(line.as_bytes()).map_err(unresponsive)?;
        let mut reply = String::new();
        let read = self.reader.read_line(&mut reply).map_err(unresponsive)?;
        if read == 0 {
            return Err(RelayUnavailable::Unresponsive(
                "the relay closed the connection".into(),
            ));
        }
        serde_json::from_str(&reply).map_err(|error| {
            RelayUnavailable::Unresponsive(format!("unreadable relay reply: {error}"))
        })
    }

    pub fn hello(&mut self) -> Result<RelayHello, RelayUnavailable> {
        match self.round_trip(&RelayRequest::Hello, HELLO_TIMEOUT)? {
            RelayResponse::Hello {
                protocol,
                pid,
                broker,
                self_hosted,
                bootstrap,
            } => Ok(RelayHello {
                protocol,
                pid,
                broker,
                self_hosted,
                bootstrap,
            }),
            RelayResponse::Err { message } => Err(RelayUnavailable::Refused(message)),
            other => Err(RelayUnavailable::Unresponsive(format!(
                "unexpected relay reply: {other:?}"
            ))),
        }
    }

    pub fn launch(&mut self, request: LaunchRequest) -> Result<u32, RelayUnavailable> {
        match self.round_trip(&RelayRequest::Launch { request }, LAUNCH_TIMEOUT)? {
            RelayResponse::Launched { pid } => Ok(pid),
            RelayResponse::Err { message } => Err(RelayUnavailable::Refused(message)),
            other => Err(RelayUnavailable::Unresponsive(format!(
                "unexpected relay reply: {other:?}"
            ))),
        }
    }
}

/// Read the relay's `hello` without launching anything (status and install verification).
pub fn relay_hello(socket: &Path) -> Result<RelayHello, RelayUnavailable> {
    RelayConnection::connect(socket)?.hello()
}

/// Ask the relay at `socket` to launch a broker, but only when it launches `expected_broker`,
/// the executable this caller would have spawned itself. Returns the broker's pid.
pub fn relay_launch(
    socket: &Path,
    expected_broker: &Path,
    request: LaunchRequest,
) -> Result<u32, RelayUnavailable> {
    let mut connection = RelayConnection::connect(socket)?;
    let hello = connection.hello()?;
    if hello.protocol != RELAY_PROTOCOL {
        return Err(RelayUnavailable::Unresponsive(format!(
            "relay speaks protocol {}, expected {RELAY_PROTOCOL}",
            hello.protocol
        )));
    }
    if !same_executable(&hello.broker, expected_broker) {
        return Err(RelayUnavailable::Mismatch {
            relay: hello.broker,
            server: expected_broker.to_path_buf(),
        });
    }
    connection.launch(request)
}

fn same_executable(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn authenticate_path(socket: &Path) -> Result<(), RelayUnavailable> {
    let me = effective_uid();
    let parent = socket
        .parent()
        .ok_or_else(|| RelayUnavailable::Untrusted("the relay socket has no directory".into()))?;
    match fs::symlink_metadata(parent) {
        Ok(directory)
            if directory.is_dir() && directory.uid() == me && directory.mode() & 0o077 == 0 => {}
        Ok(_) => {
            return Err(RelayUnavailable::Untrusted(format!(
                "{} must be a directory owned by this user with mode 0700",
                parent.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(RelayUnavailable::Absent);
        }
        Err(error) => return Err(RelayUnavailable::Untrusted(error.to_string())),
    }
    match fs::symlink_metadata(socket) {
        Ok(metadata) if metadata.file_type().is_socket() && metadata.uid() == me => Ok(()),
        Ok(_) => Err(RelayUnavailable::Untrusted(format!(
            "{} is not a socket owned by this user",
            socket.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(RelayUnavailable::Absent),
        Err(error) => Err(RelayUnavailable::Untrusted(error.to_string())),
    }
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid takes no pointers and has no preconditions.
    unsafe { libc::geteuid() }
}

/// The effective uid of the process on the other end of a connected Unix socket.
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
pub fn peer_uid(stream: &std::os::unix::net::UnixStream) -> io::Result<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: the descriptor is a connected socket owned by `stream`; both out-pointers are
    // valid for the call.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

/// The effective uid of the process on the other end of a connected Unix socket.
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
pub fn peer_uid(stream: &std::os::unix::net::UnixStream) -> io::Result<u32> {
    // SAFETY: a zeroed ucred is a valid out-buffer.
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the descriptor is a connected socket owned by `stream`; the buffer and its
    // length describe `credentials`.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(credentials.uid)
}

#[cfg(test)]
#[path = "launch_relay_tests.rs"]
mod tests;
