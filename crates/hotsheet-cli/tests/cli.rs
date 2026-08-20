//! End-to-end tests driving the built `hotsheet` binary (double coverage per
//! CLAUDE.md — the store/import logic is unit-tested; this exercises real user flows
//! through the CLI).

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

fn hs(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("hotsheet").unwrap();
    cmd.arg("-C").arg(dir);
    cmd
}

/// Create a ticket and return its slug (parsed from `Created <slug> (<path>)`).
fn new_ticket(dir: &Path, title: &str) -> String {
    let out = hs(dir).args(["new", "--title", title]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    stdout.split_whitespace().nth(1).unwrap().to_string()
}

#[test]
fn init_new_ls_show_edit_close_flow() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();

    hs(p)
        .args(["init", "--prefix", "HS"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));

    let slug = new_ticket(p, "Fix flicker");
    assert!(slug.starts_with("HS-"), "slug was {slug}");

    hs(p)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("Fix flicker"));

    hs(p)
        .args(["show", &slug])
        .assert()
        .success()
        .stdout(predicate::str::contains("title: Fix flicker"))
        .stdout(predicate::str::contains("status: not_started"));

    // Edit: status + title + up_next.
    hs(p)
        .args([
            "edit",
            &slug,
            "--status",
            "started",
            "--title",
            "Fix the flicker",
            "--up-next",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated"));

    hs(p)
        .args(["show", &slug])
        .assert()
        .success()
        .stdout(predicate::str::contains("status: started"))
        .stdout(predicate::str::contains("up_next: true"))
        .stdout(predicate::str::contains("title: Fix the flicker"));

    // Close with a reason (orthogonal to status).
    hs(p)
        .args(["close", &slug, "--reason", "completed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Closed"));

    hs(p)
        .args(["show", &slug])
        .assert()
        .success()
        .stdout(predicate::str::contains("close_reason: completed"))
        .stdout(predicate::str::contains("closed_at:"));
}

#[test]
fn edit_rejects_an_invalid_status() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let slug = new_ticket(p, "t");
    hs(p)
        .args(["edit", &slug, "--status", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid status"));
}

#[test]
fn close_duplicate_requires_a_target() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let slug = new_ticket(p, "t");
    hs(p)
        .args(["close", &slug, "--reason", "duplicate"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--duplicate-of"));
}

#[test]
fn ls_on_a_fresh_store_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    hs(dir.path()).arg("init").assert().success();
    hs(dir.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("(no tickets)"));
}

#[test]
fn show_unknown_ticket_fails() {
    let dir = tempfile::tempdir().unwrap();
    hs(dir.path()).arg("init").assert().success();
    hs(dir.path())
        .args(["show", "HS-NOPE00"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no ticket matching"));
}

#[test]
fn commands_on_a_non_store_report_it() {
    let dir = tempfile::tempdir().unwrap();
    hs(dir.path())
        .arg("ls")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a Hot Sheet store"));
}
