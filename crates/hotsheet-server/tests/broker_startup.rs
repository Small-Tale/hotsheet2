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
            .stdin(Stdio::null())
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
        // HS2-NPBZJ9 tracks stale instance cleanup after a successful graceful exit.
        // Startup already ignores a dead pid; ready() asserts the replacement owns it.
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
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
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
    fs::copy(env!("CARGO_BIN_EXE_hotsheet-server"), &binary).unwrap();
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
