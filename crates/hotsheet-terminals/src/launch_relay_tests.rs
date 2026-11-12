//! HS2-HYF56Y: the terminal launch relay's protocol, peer authentication, and fallbacks.

use super::*;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

/// A launcher that records requests instead of spawning anything.
struct FakeLauncher {
    broker: PathBuf,
    launched: Mutex<Vec<LaunchRequest>>,
    fail: bool,
}

impl FakeLauncher {
    fn new(broker: &Path) -> Arc<Self> {
        Arc::new(Self {
            broker: broker.to_path_buf(),
            launched: Mutex::new(Vec::new()),
            fail: false,
        })
    }
}

impl BrokerLauncher for FakeLauncher {
    fn broker(&self) -> (PathBuf, bool) {
        (self.broker.clone(), false)
    }

    fn launch(&self, request: &LaunchRequest) -> io::Result<u32> {
        if self.fail {
            return Err(io::Error::other("synthetic spawn failure"));
        }
        self.launched.lock().unwrap().push(request.clone());
        Ok(4242)
    }
}

/// A short private directory: macOS's per-user temp path can exceed the socket limit.
fn private_dir() -> tempfile::TempDir {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

fn request(directory: &Path) -> LaunchRequest {
    LaunchRequest {
        socket: directory.join("broker.sock"),
        project: "project-1".into(),
        journal_home: directory.to_path_buf(),
        cwd: Some(directory.to_path_buf()),
        env: vec![("PATH".into(), "/usr/bin:/bin".into())],
    }
}

async fn start(
    directory: &Path,
    launcher: Arc<dyn BrokerLauncher>,
) -> (PathBuf, tokio::task::JoinHandle<()>) {
    let socket = directory.join("relay.sock");
    let (listener, ownership) = crate::broker_socket::bind(&socket).unwrap();
    let task = tokio::spawn(async move {
        let _ownership = ownership;
        serve_relay(listener, launcher).await;
    });
    (socket, task)
}

/// Run a blocking client call off the runtime's worker.
async fn blocking<T: Send + 'static>(call: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(call).await.unwrap()
}

#[test]
fn hs2_hyf56y_launch_requests_must_be_absolute_and_plain() {
    let directory = Path::new("/tmp/relay-validate");
    assert_eq!(request(directory).validate(), Ok(()));
    let mut relative = request(directory);
    relative.socket = PathBuf::from("broker.sock");
    assert!(relative.validate().is_err());
    let mut journal = request(directory);
    journal.journal_home = PathBuf::from("home");
    assert!(journal.validate().is_err());
    let mut cwd = request(directory);
    cwd.cwd = Some(PathBuf::from("relative"));
    assert!(cwd.validate().is_err());
    for project in ["", "two\nlines", "nul\0"] {
        let mut bad = request(directory);
        bad.project = project.into();
        assert!(bad.validate().is_err(), "{project:?}");
    }
    for (key, value) in [("", "x"), ("A=B", "x"), ("A", "nul\0")] {
        let mut bad = request(directory);
        bad.env = vec![(key.into(), value.into())];
        assert!(bad.validate().is_err(), "{key:?}={value:?}");
    }
}

#[test]
fn hs2_hyf56y_protocol_lines_are_tagged_json() {
    assert_eq!(
        serde_json::to_string(&RelayRequest::Hello).unwrap(),
        r#"{"op":"hello"}"#
    );
    assert_eq!(
        serde_json::to_string(&RelayResponse::Launched { pid: 7 }).unwrap(),
        r#"{"result":"launched","pid":7}"#
    );
    let hello = RelayResponse::Hello {
        protocol: RELAY_PROTOCOL,
        pid: 1,
        broker: "/opt/hotsheet-server".into(),
        self_hosted: true,
        bootstrap: BootstrapHealth::Live,
    };
    let json = serde_json::to_string(&hello).unwrap();
    assert_eq!(
        json,
        r#"{"result":"hello","protocol":1,"pid":1,"broker":"/opt/hotsheet-server","self_hosted":true,"bootstrap":{"state":"live"}}"#
    );
    assert_eq!(serde_json::from_str::<RelayResponse>(&json).unwrap(), hello);
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_hyf56y_relay_answers_hello_and_launches_its_own_broker() {
    let directory = private_dir();
    let broker = directory.path().join("hotsheet-server");
    fs::write(&broker, "").unwrap();
    let launcher = FakeLauncher::new(&broker);
    let (socket, task) = start(directory.path(), launcher.clone()).await;

    let hello = blocking({
        let socket = socket.clone();
        move || relay_hello(&socket)
    })
    .await
    .unwrap();
    assert_eq!(hello.protocol, RELAY_PROTOCOL);
    assert_eq!(hello.pid, std::process::id());
    assert_eq!(hello.broker, broker);
    assert!(!hello.self_hosted);

    let sent = request(directory.path());
    let pid = blocking({
        let socket = socket.clone();
        let broker = broker.clone();
        let sent = sent.clone();
        move || relay_launch(&socket, &broker, sent)
    })
    .await
    .unwrap();
    assert_eq!(pid, 4242);
    assert_eq!(*launcher.launched.lock().unwrap(), vec![sent]);
    task.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_hyf56y_a_relay_for_another_broker_build_is_not_used() {
    let directory = private_dir();
    let relay_broker = directory.path().join("installed-server");
    let server_broker = directory.path().join("dev-server");
    fs::write(&relay_broker, "").unwrap();
    fs::write(&server_broker, "").unwrap();
    let launcher = FakeLauncher::new(&relay_broker);
    let (socket, task) = start(directory.path(), launcher.clone()).await;
    let sent = request(directory.path());
    let result = blocking(move || relay_launch(&socket, &server_broker, sent)).await;
    assert!(
        matches!(result, Err(RelayUnavailable::Mismatch { .. })),
        "{result:?}"
    );
    assert!(launcher.launched.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_hyf56y_invalid_and_failed_launches_are_refused_not_retried() {
    let directory = private_dir();
    let broker = directory.path().join("hotsheet-server");
    fs::write(&broker, "").unwrap();
    let failing = Arc::new(FakeLauncher {
        broker: broker.clone(),
        launched: Mutex::new(Vec::new()),
        fail: true,
    });
    let (socket, task) = start(directory.path(), failing).await;

    let mut invalid = request(directory.path());
    invalid.socket = PathBuf::from("relative.sock");
    let result = blocking({
        let socket = socket.clone();
        let broker = broker.clone();
        move || relay_launch(&socket, &broker, invalid)
    })
    .await;
    assert!(
        matches!(&result, Err(RelayUnavailable::Refused(message)) if message.contains("absolute")),
        "{result:?}"
    );

    let sent = request(directory.path());
    let result = blocking(move || relay_launch(&socket, &broker, sent)).await;
    assert!(
        matches!(&result, Err(RelayUnavailable::Refused(message)) if message.contains("synthetic spawn failure")),
        "{result:?}"
    );
    task.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_hyf56y_relay_rejects_bad_and_oversized_request_lines() {
    use std::io::Read;
    let directory = private_dir();
    let broker = directory.path().join("hotsheet-server");
    let (socket, task) = start(directory.path(), FakeLauncher::new(&broker)).await;
    let replies = blocking(move || {
        let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        stream.write_all(b"{\"op\":\"nope\"}\n").unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut bad = String::new();
        reader.read_line(&mut bad).unwrap();
        let oversized = vec![b'x'; MAX_REQUEST_BYTES as usize + 16];
        // The relay may close mid-write once it has read its limit.
        let _ = stream.write_all(&oversized);
        let mut rest = String::new();
        let _ = reader.read_to_string(&mut rest);
        (bad, rest)
    })
    .await;
    assert!(replies.0.contains("bad request"), "{}", replies.0);
    assert!(replies.1.contains("too large"), "{}", replies.1);
    task.abort();
}

#[test]
fn hs2_hyf56y_missing_and_stale_relay_sockets_are_absent() {
    let directory = private_dir();
    let socket = directory.path().join("relay.sock");
    assert_eq!(
        RelayConnection::connect(&socket).err(),
        Some(RelayUnavailable::Absent)
    );
    assert_eq!(
        RelayConnection::connect(&directory.path().join("gone").join("relay.sock")).err(),
        Some(RelayUnavailable::Absent)
    );
    // A crashed relay leaves its socket inode with nobody accepting.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());
    assert_eq!(
        RelayConnection::connect(&socket).err(),
        Some(RelayUnavailable::Absent)
    );
}

#[test]
fn hs2_hyf56y_shared_directories_and_non_sockets_are_untrusted() {
    let directory = private_dir();
    let socket = directory.path().join("relay.sock");
    fs::write(&socket, "").unwrap();
    assert!(matches!(
        RelayConnection::connect(&socket),
        Err(RelayUnavailable::Untrusted(_))
    ));
    fs::remove_file(&socket).unwrap();
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let result = RelayConnection::connect(&socket);
    assert!(
        matches!(&result, Err(RelayUnavailable::Untrusted(reason)) if reason.contains("0700")),
        "{:?}",
        result.err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_hyf56y_a_second_relay_cannot_take_over_a_live_socket() {
    let directory = private_dir();
    let broker = directory.path().join("hotsheet-server");
    let (socket, task) = start(directory.path(), FakeLauncher::new(&broker)).await;
    let error = run_relay_process(&socket, FakeLauncher::new(&broker))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::AddrInUse
        ),
        "{error:?}"
    );
    task.abort();
}

#[test]
fn hs2_hyf56y_peer_uid_is_this_user_on_a_socket_pair() {
    let (left, _right) = std::os::unix::net::UnixStream::pair().unwrap();
    assert_eq!(peer_uid(&left).unwrap(), effective_uid());
}
