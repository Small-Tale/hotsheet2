//! HS2-HYF56Y: the headless `hotsheet-cli terminal-relay` workflow end to end. A fake
//! `launchctl` stands in for launchd: `bootstrap` starts the real `hotsheet-server` relay
//! mode on the agent's socket and `bootout` stops it. The agents directory and machine home
//! are temporary, so nothing reaches the real launchd or `~/Library/LaunchAgents`.
#![cfg(target_os = "macos")]

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const FAKE_LAUNCHCTL: &str = r#"#!/bin/sh
S="$FAKE_LAUNCHCTL_STATE"
echo "$*" >> "$S/calls"
case "$1" in
  managername) echo Aqua ;;
  print)
    [ -f "$S/pid" ] || { echo "Could not find service" >&2; exit 113; }
    echo "state = running" ;;
  bootstrap)
    "$FAKE_RELAY_PROGRAM" --terminal-launch-relay "$FAKE_RELAY_SOCKET" </dev/null >/dev/null 2>&1 &
    echo $! > "$S/pid" ;;
  bootout)
    kill "$(cat "$S/pid")"
    rm -f "$S/pid" ;;
  kickstart) ;;
  *) exit 64 ;;
esac
"#;

struct Fixture {
    dir: tempfile::TempDir,
    home: PathBuf,
    agents: PathBuf,
    state: PathBuf,
    launchctl: PathBuf,
    server: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Whatever the test's outcome, stop a relay the fake launchctl started.
        if let Ok(pid) = fs::read_to_string(self.state.join("pid"))
            && let Ok(pid) = pid.trim().parse::<i32>()
        {
            // SAFETY: the pid was written by this fixture's own fake launchctl.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let home = dir.path().join("home");
        let agents = dir.path().join("LaunchAgents");
        let state = dir.path().join("launchctl-state");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&state).unwrap();
        let launchctl = dir.path().join("launchctl");
        fs::write(&launchctl, FAKE_LAUNCHCTL).unwrap();
        fs::set_permissions(&launchctl, fs::Permissions::from_mode(0o755)).unwrap();
        let server = assert_cmd::cargo::cargo_bin("hotsheet-cli")
            .with_file_name("hotsheet-server")
            .canonicalize()
            .unwrap();
        Self {
            dir,
            home,
            agents,
            state,
            launchctl,
            server,
        }
    }

    fn relay_socket(&self) -> PathBuf {
        hotsheet_terminals::broker_socket::relay_socket_path(&self.home).unwrap()
    }

    fn cli(&self, args: &[&str]) -> Command {
        let mut command = Command::cargo_bin("hotsheet-cli").unwrap();
        command
            .current_dir(self.dir.path())
            .args(["terminal-relay"])
            .args(args)
            .env("HOTSHEET_HOME", &self.home)
            .env("HOTSHEET_LAUNCHCTL", &self.launchctl)
            .env("HOTSHEET_LAUNCH_AGENTS_DIR", &self.agents)
            .env("FAKE_LAUNCHCTL_STATE", &self.state)
            .env("FAKE_RELAY_PROGRAM", &self.server)
            .env("FAKE_RELAY_SOCKET", self.relay_socket());
        command
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.state.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(|line| line.split_whitespace().next().unwrap_or("").to_owned())
            .collect()
    }

    fn plists(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.agents)
            .map(|entries| entries.map(|entry| entry.unwrap().path()).collect())
            .unwrap_or_default()
    }
}

fn single(paths: &[PathBuf]) -> &Path {
    assert_eq!(paths.len(), 1, "{paths:?}");
    &paths[0]
}

#[test]
fn hs2_hyf56y_headless_install_status_and_uninstall_drive_a_real_relay() {
    let fixture = Fixture::new();
    let server = fixture.server.to_string_lossy().into_owned();

    fixture
        .cli(&["install", "--server", &server])
        .assert()
        .success()
        .stdout(predicate::str::contains("Terminal launch relay installed"));
    assert_eq!(fixture.calls(), ["managername", "print", "bootstrap"]);
    let plist = single(&fixture.plists()).to_path_buf();
    let name = plist.file_name().unwrap().to_string_lossy().into_owned();
    // A non-default home never takes the user's real agent label.
    assert!(
        name.starts_with("com.smalltale.hotsheet2.terminal-relay."),
        "{name}"
    );
    let definition = fs::read_to_string(&plist).unwrap();
    assert!(definition.contains(&server), "{definition}");
    assert!(definition.contains("--terminal-launch-relay"));
    let lint = std::process::Command::new("/usr/bin/plutil")
        .arg("-lint")
        .arg(&plist)
        .output()
        .unwrap();
    assert!(lint.status.success(), "{lint:?}");
    let relay = hotsheet_terminals::launch_relay::relay_hello(&fixture.relay_socket()).unwrap();
    assert!(!relay.bootstrap.is_dead(), "{relay:?}");

    // Idempotent: a second install leaves the running relay alone.
    fixture
        .cli(&["install", "--server", &server])
        .assert()
        .success()
        .stdout(predicate::str::contains("already installed"));
    assert_eq!(
        fixture.calls(),
        ["managername", "print", "bootstrap", "managername", "print"]
    );
    assert_eq!(
        hotsheet_terminals::launch_relay::relay_hello(&fixture.relay_socket())
            .unwrap()
            .pid,
        relay.pid
    );

    let status = fixture
        .cli(&["status", "--server", &server, "--json"])
        .output()
        .unwrap();
    assert!(status.status.success(), "{status:?}");
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["healthy"], true, "{status}");
    assert_eq!(status["relay"]["pid"], relay.pid);

    fixture
        .cli(&["uninstall"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed terminal launch relay"));
    assert!(fixture.plists().is_empty());
    fixture
        .cli(&["uninstall"])
        .assert()
        .success()
        .stdout(predicate::str::contains("was not installed"));
    fixture
        .cli(&["status", "--server", &server])
        .assert()
        .success()
        .stdout(predicate::str::contains("not in use"))
        .stdout(predicate::str::contains("terminal-relay install"));
}

#[test]
fn hs2_hyf56y_install_without_a_desktop_session_prints_next_steps_and_writes_nothing() {
    let fixture = Fixture::new();
    let server = fixture.server.to_string_lossy().into_owned();
    // The test environment's launchctl does not exist, as from a session with no launchd GUI
    // domain to talk to.
    let mut command = fixture.cli(&["install", "--server", &server]);
    command.env_remove("HOTSHEET_LAUNCHCTL").env(
        "HOTSHEET_LAUNCHCTL",
        fixture.dir.path().join("no-launchctl"),
    );
    command
        .assert()
        .failure()
        .stderr(predicate::str::contains("Log in to the Mac's desktop"))
        .stderr(predicate::str::contains(
            "hotsheet-cli terminal-relay install",
        ));
    assert!(fixture.plists().is_empty());
}
