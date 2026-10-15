//! Real `hotsheet-server` binaries: an explicit `--stop` leaves a machine server owned by
//! the pinned production host (`--launcher local-host-snapshot`) running unless `--force`,
//! while servers launched by dev hosts or the CLI stop as before (HS2-9AWEX5).

use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use hotsheet_server::lifecycle::InstanceInfo;
use hotsheet_ticketing::{FsStore, StoreMetadata};

struct Server {
    child: Child,
    home: PathBuf,
    store: PathBuf,
    instance: PathBuf,
    _fixture: tempfile::TempDir,
}

impl Server {
    fn start(launcher: &str) -> Self {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        fs::create_dir(&home).unwrap();
        let store = fixture.path().join("store");
        FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
        let log = fs::File::create(fixture.path().join("server.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_hotsheet-server"))
            .arg("-C")
            .arg(&store)
            .args(["--bind", "127.0.0.1:0", "--secret", "pinned-stop"])
            .args(["--no-terminal-broker", "--launcher", launcher])
            .arg("--exit-on-stdin-eof")
            .env("HOTSHEET_HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .process_group(0)
            .spawn()
            .unwrap();
        let project = hotsheet_tls::project_id(&store);
        let mut server = Self {
            child,
            instance: home.join("instances").join(format!("{project}.json")),
            home,
            store,
            _fixture: fixture,
        };
        server.ready();
        server
    }

    fn ready(&mut self) -> InstanceInfo {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(self.child.try_wait().unwrap().is_none(), "server exited");
            if let Ok(text) = fs::read_to_string(&self.instance)
                && let Ok(info) = serde_json::from_str::<InstanceInfo>(&text)
                && info.pid == self.child.id()
            {
                return info;
            }
            assert!(Instant::now() < deadline, "server did not start");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn stop(&self, extra: &[&str]) -> Output {
        stop_command(&self.home, &self.store, extra)
    }

    fn exited_within(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.child.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn stop_command(home: &Path, store: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hotsheet-server"))
        .arg("-C")
        .arg(store)
        .arg("--stop")
        .args(extra)
        .env("HOTSHEET_HOME", home)
        .output()
        .unwrap()
}

#[test]
fn a_pinned_server_survives_an_ordinary_stop_and_stops_when_forced() {
    let mut server = Server::start("local-host-snapshot");
    let registered = server.ready();
    for attempt in 0..2 {
        let refused = server.stop(&[]);
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(!refused.status.success(), "attempt {attempt}: {stderr}");
        assert!(
            stderr.contains("refusing to stop") && stderr.contains("launcher=local-host-snapshot"),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!("pid={}", registered.pid)),
            "{stderr}"
        );
        assert!(stderr.contains("--force"), "{stderr}");
        assert!(
            !server.exited_within(Duration::from_millis(300)),
            "a refused stop never signals"
        );
        assert_eq!(server.ready(), registered, "the registration is untouched");
    }
    // Terminal cleanup is refused with the stop: the owner keeps its retained terminals.
    let refused = server.stop(&["--kill-all-terminals"]);
    assert!(!refused.status.success());
    assert!(!String::from_utf8_lossy(&refused.stdout).contains("killed"));

    let forced = server.stop(&["--force"]);
    let stdout = String::from_utf8_lossy(&forced.stdout);
    assert!(forced.status.success(), "{stdout}");
    assert!(stdout.contains("launcher=local-host-snapshot"), "{stdout}");
    assert!(server.exited_within(Duration::from_secs(15)));
    let after = server.stop(&[]);
    assert!(after.status.success());
    assert!(String::from_utf8_lossy(&after.stdout).contains("no running server"));
}

#[test]
fn dev_and_cli_launched_servers_stop_without_force() {
    for launcher in ["dev-bridge", "stable-dev", "cli"] {
        let mut server = Server::start(launcher);
        let stopped = server.stop(&[]);
        let stdout = String::from_utf8_lossy(&stopped.stdout);
        assert!(stopped.status.success(), "{launcher}: {stdout}");
        assert!(stdout.contains(&format!("launcher={launcher}")), "{stdout}");
        assert!(server.exited_within(Duration::from_secs(15)), "{launcher}");
    }
}

#[test]
fn force_without_stop_is_rejected() {
    let output = Command::new(env!("CARGO_BIN_EXE_hotsheet-server"))
        .arg("--force")
        .output()
        .unwrap();
    assert!(!output.status.success());
}
