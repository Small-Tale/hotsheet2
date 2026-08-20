//! E2E for the standalone `hotsheet-migrate` binary. The happy path (spawning the
//! Node exporter) is exercised by the migrator's vitest suite; here we cover the
//! arg surface + the missing-migrator error offline.

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn reports_a_missing_migrator() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("hotsheet-migrate")
        .unwrap()
        .args([
            "/tmp/nonexistent-hotsheet",
            "-C",
            dir.path().to_str().unwrap(),
            "--migrator",
            "/tmp/nope/export.mjs",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migrator not found"));
}

#[test]
fn has_its_own_help() {
    Command::cargo_bin("hotsheet-migrate")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Migrate a Hot Sheet 1 project"));
}
