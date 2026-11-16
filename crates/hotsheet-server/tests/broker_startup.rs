#![cfg(unix)] // HS2-2TD7M7: Unix socket ownership and process-group fixture.

//! Real detached server/broker startup with a home longer than AF_UNIX permits.

use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hotsheet_server::lifecycle::InstanceInfo;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};

#[path = "../../test-support/first_launch.rs"]
mod first_launch;
#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

struct TestServer {
    child: Child,
    log: PathBuf,
    instance: PathBuf,
}

impl TestServer {
    fn start(binary: &Path, home: &Path, store: &Path, log: PathBuf) -> Self {
        let output = fs::File::create(&log).unwrap();
        let child = Command::new(binary)
            .arg("-C")
            .arg(store)
            .args(["--bind", "127.0.0.1:0", "--secret", "long-home-test"])
            .env("HOTSHEET_HOME", home)
            // Keep unrelated locally installed AI tools out of startup discovery;
            // this regression exercises broker transport, not provider CLIs.
            .env("PATH", "/usr/bin:/bin")
            // Piped and held by the fixture: the server stops if this test process dies
            // without running its teardown (HS2-VQ8ZWT).
            .arg("--exit-on-stdin-eof")
            .stdin(Stdio::piped())
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            // Own the server and its detached child as a test-only process group. Drop
            // terminates that entire group even if the server was already restarted.
            .process_group(0)
            .spawn()
            .unwrap();
        Self {
            child,
            log,
            instance: home
                .join("instances")
                .join(format!("{}.json", hotsheet_tls::project_id(store))),
        }
    }

    fn ready(&mut self) -> InstanceInfo {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "server exited: {}",
                fs::read_to_string(&self.log).unwrap()
            );
            if let Ok(text) = fs::read_to_string(&self.instance)
                && let Ok(info) = serde_json::from_str::<InstanceInfo>(&text)
                && info.pid == self.child.id()
            {
                return info;
            }
            assert!(
                Instant::now() < deadline,
                "server did not start: {}",
                fs::read_to_string(&self.log).unwrap()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn stop_server_only(&mut self) {
        // SAFETY: the positive pid belongs to this fixture's child, not a process group.
        assert_eq!(
            unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "server exited unsuccessfully: {status}");
                break;
            }
            assert!(Instant::now() < deadline, "server did not shut down");
            std::thread::sleep(Duration::from_millis(25));
        }
        // A graceful exit releases its registration explicitly (HS2-W1KJR4), even while
        // the detached broker keeps running.
        assert!(
            !self.instance.exists(),
            "stopped server left its instance file: {}",
            fs::read_to_string(&self.log).unwrap()
        );
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // SAFETY: the negative id is this fixture's process group, created above. It
        // includes the detached broker but never the test runner or unrelated servers.
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

struct SocketFiles(PathBuf);

impl Drop for SocketFiles {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_file(self.0.with_extension("lock"));
        let _ = fs::remove_file(self.0.with_extension("namespace.lock"));
    }
}

fn request(info: &InstanceInfo, method: &str, path: &str, body: Option<Value>) -> Value {
    let request = ureq::request(method, &format!("{}{path}", info.url))
        .timeout(Duration::from_secs(5))
        .set("x-hotsheet-secret", &info.secret)
        .set("content-type", "application/json");
    let response = match body {
        Some(body) => request.send_string(&body.to_string()),
        None => request.call(),
    }
    .unwrap();
    let text = response.into_string().unwrap();
    if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    }
}

#[test]
fn long_home_starts_real_server_and_detached_broker_and_preserves_terminals_on_restart() {
    // Use the platform's ordinary temp root, including macOS's long /var/folders path.
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("configured-hotsheet-home-".repeat(6));
    fs::create_dir(&home).unwrap();
    fs::write(
        home.join("retain-state.txt"),
        "configured home remains authoritative",
    )
    .unwrap();
    let store = fixture.path().join("store");
    FsStore::init_git(&store, &StoreMetadata::new("HS")).unwrap();
    let project = hotsheet_tls::project_id(&store);
    let old_socket = home.join("broker").join(format!("{project}.sock"));
    assert!(old_socket.as_os_str().as_bytes().len() > 108);
    assert_eq!(
        std::os::unix::net::SocketAddr::from_pathname(&old_socket)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    let socket = hotsheet_terminals::broker_socket::socket_path(&home, &project).unwrap();
    let _socket_files = SocketFiles(socket.clone());

    // Copy only the server: exercise the shipped self-hosted broker fallback instead
    // of accidentally launching an older sibling binary from a shared target cache.
    let binary = fixture.path().join("hotsheet-server");
    fs::copy(workspace_bin::path("hotsheet-server"), &binary).unwrap();
    // A fresh copy is a new executable to macOS: launch it once before `ready()` times it.
    first_launch::launched(&binary, &["--version"]);
    let mut first = TestServer::start(&binary, &home, &store, fixture.path().join("first.log"));
    let first_info = first.ready();
    assert!(Path::new(&first_info.index_path).starts_with(&home));
    assert!(Path::new(&first_info.index_path).is_file());
    assert!(!home.join("broker").exists());
    let socket_metadata = fs::metadata(&socket).unwrap();
    assert_eq!(socket_metadata.mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(socket.parent().unwrap()).unwrap().mode() & 0o777,
        0o700
    );
    assert_eq!(
        request(
            &first_info,
            "POST",
            "/terminals",
            Some(json!({"id":"retained", "command":"/bin/cat"}))
        )["alive"],
        true
    );
    request(
        &first_info,
        "POST",
        "/terminals/retained/input",
        Some(json!({"data":"before-restart\n"})),
    );

    first.stop_server_only();
    let mut second = TestServer::start(&binary, &home, &store, fixture.path().join("second.log"));
    let second_info = second.ready();
    assert_eq!(
        fs::metadata(&socket).unwrap().ino(),
        socket_metadata.ino(),
        "restart reconnects to the same detached broker"
    );
    let terminals = request(&second_info, "GET", "/terminals", None);
    assert_eq!(terminals.as_array().unwrap().len(), 1);
    assert_eq!(terminals[0]["id"], "retained");
    assert_eq!(terminals[0]["alive"], true);
    request(
        &second_info,
        "POST",
        "/terminals/retained/input",
        Some(json!({"data":"after-restart\n"})),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let terminal = request(&second_info, "GET", "/terminals/retained", None);
        let output = terminal["scrollback"].as_str().unwrap();
        if output.contains("before-restart") && output.contains("after-restart") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "retained terminal output missing: {terminal}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    request(&second_info, "DELETE", "/terminals/retained", None);
    assert!(
        request(&second_info, "GET", "/terminals", None)
            .as_array()
            .unwrap()
            .is_empty()
    );
    second.stop_server_only();
    assert_eq!(
        fs::read_to_string(home.join("retain-state.txt")).unwrap(),
        "configured home remains authoritative"
    );
}

/// A pipe whose write end is open across exec, as a pipe another thread was creating is
/// when a process is spawned in that instant (HS2-WEYR63). The read end does not block.
fn leaked_pipe() -> (std::os::fd::OwnedFd, std::os::fd::OwnedFd) {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let mut fds = [0; 2];
    // SAFETY: pipe fills two new descriptors, which are owned below.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    // SAFETY: both descriptors were just created by pipe and are owned exactly once.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    // SAFETY: plain descriptor flag calls on descriptors this test owns.
    unsafe {
        assert_eq!(libc::fcntl(write.as_raw_fd(), libc::F_SETFD, 0), 0);
        assert_eq!(
            libc::fcntl(read.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC),
            0
        );
        let flags = libc::fcntl(read.as_raw_fd(), libc::F_GETFL);
        assert_eq!(
            libc::fcntl(read.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK),
            0
        );
    }
    (read, write)
}

/// End of file on the read end: no process holds a writer any more.
fn no_writer_left(read: &std::os::fd::OwnedFd) -> Result<(), std::io::Error> {
    use std::os::fd::AsRawFd;
    let mut byte = 0u8;
    // SAFETY: reads at most one byte into a local buffer from a descriptor the test owns.
    match unsafe { libc::read(read.as_raw_fd(), (&raw mut byte).cast(), 1) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// HS2-ETMKCW: the detached broker (and the terminals it hosts) holds no descriptor the
/// server inherited. The server here inherits a pipe that is not close-on-exec; once it
/// has stopped and the broker keeps running, nothing may still hold that pipe's writer.
#[test]
fn the_detached_broker_inherits_no_descriptor_from_the_server() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    fs::create_dir(&home).unwrap();
    let store = fixture.path().join("store");
    FsStore::init_git(&store, &StoreMetadata::new("HS")).unwrap();
    let project = hotsheet_tls::project_id(&store);
    let socket = hotsheet_terminals::broker_socket::socket_path(&home, &project).unwrap();
    let _socket_files = SocketFiles(socket);
    let binary = fixture.path().join("hotsheet-server");
    fs::copy(workspace_bin::path("hotsheet-server"), &binary).unwrap();
    let (read, write) = leaked_pipe();
    let mut server = TestServer::start(&binary, &home, &store, fixture.path().join("server.log"));
    drop(write);
    let info = server.ready();
    assert_eq!(
        request(
            &info,
            "POST",
            "/terminals",
            Some(json!({"id":"held", "command":"/bin/cat"}))
        )["alive"],
        true
    );
    server.stop_server_only();
    no_writer_left(&read).expect("the detached broker holds the server's descriptor");
}

/// The `identity` the live broker on `socket` reports (HS2-H0WFJG).
fn broker_identity(socket: &Path) -> Value {
    use std::io::{BufRead, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
    stream.write_all(b"{\"op\":\"identity\"}\n").unwrap();
    let mut reply = String::new();
    std::io::BufReader::new(stream)
        .read_line(&mut reply)
        .unwrap();
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["result"], "identity", "{reply}");
    reply["identity"].clone()
}

#[test]
fn hs2_h0wfjg_a_pinned_server_keeps_a_dev_broker_with_terminals_and_replaces_it_when_idle() {
    // A development build starts the machine server and its detached broker; the pinned host
    // then restarts the server from its own snapshot. Synthetic store and `cat` terminals only.
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    fs::create_dir(&home).unwrap();
    let store = fixture.path().join("store");
    FsStore::init_git(&store, &StoreMetadata::new("HS")).unwrap();
    let project = hotsheet_tls::project_id(&store);
    let socket = hotsheet_terminals::broker_socket::socket_path(&home, &project).unwrap();
    let _socket_files = SocketFiles(socket.clone());
    // Copy only the server, so each build self-hosts its broker from its own executable.
    let copy = |dir: &str| {
        let binary = fixture.path().join(dir).join("hotsheet-server");
        fs::create_dir(binary.parent().unwrap()).unwrap();
        fs::copy(workspace_bin::path("hotsheet-server"), &binary).unwrap();
        // A fresh copy is a new executable to macOS: launch it once before `ready()` times it.
        first_launch::launched(binary.canonicalize().unwrap(), &["--version"])
    };
    let (dev, pinned) = (copy("dev"), copy("pinned"));
    let executable = |identity: &Value| {
        Path::new(identity["executable"].as_str().unwrap())
            .canonicalize()
            .unwrap()
    };

    let mut dev_server = TestServer::start(&dev, &home, &store, fixture.path().join("dev.log"));
    let dev_info = dev_server.ready();
    let dev_broker = broker_identity(&socket);
    assert_eq!(executable(&dev_broker), dev);
    assert_eq!(
        request(
            &dev_info,
            "POST",
            "/terminals",
            Some(json!({"id":"live", "command":"/bin/cat"}))
        )["alive"],
        true
    );
    request(
        &dev_info,
        "POST",
        "/terminals/live/input",
        Some(json!({"data":"before-adoption\n"})),
    );
    let dev_socket = fs::metadata(&socket).unwrap().ino();
    dev_server.stop_server_only();

    // The pinned server adopts the dev broker as is: it hosts a live terminal.
    let mut pinned_server =
        TestServer::start(&pinned, &home, &store, fixture.path().join("pinned.log"));
    let pinned_info = pinned_server.ready();
    assert_eq!(fs::metadata(&socket).unwrap().ino(), dev_socket);
    let kept = broker_identity(&socket);
    assert_eq!(kept["pid"], dev_broker["pid"], "the dev broker was kept");
    assert_eq!(kept["terminals"], 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let terminal = request(&pinned_info, "GET", "/terminals/live", None);
        assert_eq!(terminal["alive"], true, "{terminal}");
        if terminal["scrollback"]
            .as_str()
            .unwrap()
            .contains("before-adoption")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "terminal output lost: {terminal}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    // The user closes the terminal; the broker is now idle.
    request(&pinned_info, "DELETE", "/terminals/live", None);
    pinned_server.stop_server_only();

    // The next pinned start hands the idle dev broker over to a broker from its own build.
    let mut restarted =
        TestServer::start(&pinned, &home, &store, fixture.path().join("restarted.log"));
    let restarted_info = restarted.ready();
    let replaced = broker_identity(&socket);
    assert_ne!(
        replaced["pid"], dev_broker["pid"],
        "the idle dev broker retired"
    );
    assert_eq!(executable(&replaced), pinned);
    // No inode check here (HS2-9CCPSQ): Linux may reuse the removed socket's inode
    // number for the replacement, so the pid and executable prove the hand-over.
    let retired_pid = dev_broker["pid"].as_u64().unwrap() as i32;
    let deadline = Instant::now() + Duration::from_secs(5);
    // SAFETY: signal 0 only checks whether the retired broker's pid still exists.
    while unsafe { libc::kill(retired_pid, 0) } == 0 {
        assert!(
            Instant::now() < deadline,
            "the retired dev broker kept running"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    // Terminals work on the replacement broker.
    assert_eq!(
        request(
            &restarted_info,
            "POST",
            "/terminals",
            Some(json!({"id":"after", "command":"/bin/cat"}))
        )["alive"],
        true
    );
    request(&restarted_info, "DELETE", "/terminals/after", None);
    restarted.stop_server_only();
}
