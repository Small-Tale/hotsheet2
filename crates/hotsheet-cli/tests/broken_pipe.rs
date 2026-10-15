//! HS2-4BT3VF: a reader that closes the pipe early (`hotsheet-cli ls | head -1`) must end
//! output quietly instead of panicking with "failed printing to stdout: Broken pipe".

use assert_cmd::cargo::CommandCargoExt;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

fn cli(home: &Path, store: &Path) -> Command {
    let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
    cmd.env("HOTSHEET_HOME", home)
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .arg("-C")
        .arg(store);
    cmd
}

#[test]
fn listing_into_a_closed_pipe_ends_quietly_without_a_panic() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let store = root.path().join("tickets");
    std::fs::create_dir_all(&home).unwrap();
    hotsheet_ticketing::FsStore::init(&store, &hotsheet_ticketing::StoreMetadata::new("HS"))
        .unwrap();
    for n in 0..3 {
        let status = cli(&home, &store)
            .args(["new", "--title", &format!("pipe ticket {n}")])
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }

    // Sanity: the listing really produces output, so the closed-pipe run below must write.
    let full = cli(&home, &store).arg("ls").output().unwrap();
    assert!(full.status.success());
    assert!(String::from_utf8_lossy(&full.stdout).contains("pipe ticket 2"));

    // Close the read end before the CLI writes anything: every stdout write hits EPIPE,
    // exactly what happens after `head -1` exits.
    let mut child = cli(&home, &store)
        .arg("ls")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let status = child.wait().unwrap();

    assert!(
        !stderr.contains("panicked") && !stderr.contains("Broken pipe"),
        "closed pipe produced panic text on stderr: {stderr}"
    );
    assert_eq!(
        status.code(),
        Some(0),
        "closed pipe should be a quiet, successful end of output (status {status:?}, stderr {stderr})"
    );
}
