//! End-to-end tests driving the built `hotsheet` binary (double coverage per
//! CLAUDE.md — the store/import logic is unit-tested; this exercises real user flows
//! through the CLI).

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

fn hs(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
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
        .stderr(predicate::str::contains("duplicate target is required"));
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

#[test]
fn migrate_is_not_a_subcommand_of_the_main_cli() {
    // Migration lives in the separate `hotsheet-migrate` binary, not here.
    let dir = tempfile::tempdir().unwrap();
    hs(dir.path())
        .args(["migrate", "whatever"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn ls_filters_and_sort() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let a = new_ticket(p, "alpha bug");
    let b = new_ticket(p, "beta feature");
    hs(p)
        .args([
            "edit",
            &a,
            "--status",
            "started",
            "--tag",
            "urgent",
            "--up-next",
        ])
        .assert()
        .success();
    hs(p)
        .args(["edit", &b, "--priority", "high"])
        .assert()
        .success();

    // text filter
    hs(p)
        .args(["ls", "--text", "BETA"])
        .assert()
        .success()
        .stdout(predicate::str::contains("beta feature"))
        .stdout(predicate::str::contains("alpha").not());
    // status filter
    hs(p)
        .args(["ls", "--status", "started"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha bug"))
        .stdout(predicate::str::contains("beta").not());
    // tag filter
    hs(p)
        .args(["ls", "--tag", "urgent"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha bug"));
    // up_next only
    hs(p)
        .args(["ls", "--up-next"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha bug"))
        .stdout(predicate::str::contains("beta").not());
    // invalid sort errors
    hs(p)
        .args(["ls", "--sort", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid sort"));
}

#[test]
fn work_drains_empty_queue_and_requires_setup_when_there_is_work() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();

    // 1) Empty Up Next → exits cleanly without needing setup or launching anything.
    hs(p)
        .args(["work", "claude"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Nothing Up Next"));

    // 2) With work queued but the tool not set up, the shared HS2-103 preflight refuses
    //    (proving `work` inherits `trigger`'s launch safety) before any turn runs.
    let t = new_ticket(p, "do a thing");
    hs(p).args(["edit", &t, "--up-next"]).assert().success();
    hs(p)
        .args(["work", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("isn't set up"));
}

#[test]
fn trigger_preflight_blocks_hs1_and_requires_setup() {
    // Uses the built-in `claude` plugin (always embedded), so `trigger` gets past
    // `find(tool)` and reaches the HS2-103 preflight gates, which both bail before any
    // real tool is launched.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();

    // 1) An HS1 store under the project is refused before anything launches.
    std::fs::create_dir(p.join(".hotsheet")).unwrap();
    hs(p)
        .args(["trigger", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("HS1 store"));
    std::fs::remove_dir(p.join(".hotsheet")).unwrap();

    // 2) Without the tool set up (no .mcp.json), trigger refuses (MCP isolation gate).
    hs(p)
        .args(["trigger", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("isn't set up"));
}

#[test]
fn blocked_by_set_clear_and_reject() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let a = new_ticket(p, "blocker");
    let b = new_ticket(p, "blocked");

    // new --blocked-by resolves a slug at create time
    hs(p)
        .args(["new", "--title", "child", "--blocked-by", &a])
        .assert()
        .success();

    // edit --blocked-by sets it; show renders the frontmatter key
    hs(p)
        .args(["edit", &b, "--blocked-by", &a])
        .assert()
        .success();
    hs(p)
        .args(["show", &b])
        .assert()
        .success()
        .stdout(predicate::str::contains("blocked_by"));

    // self-reference and unknown ticket are errors
    hs(p)
        .args(["edit", &b, "--blocked-by", &b])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot block itself"));
    hs(p)
        .args(["edit", &b, "--blocked-by", "HS-NOPE00"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no ticket matching"));

    // --clear-blocked-by empties it (key drops from the file)
    hs(p)
        .args(["edit", &b, "--clear-blocked-by"])
        .assert()
        .success();
    hs(p)
        .args(["show", &b])
        .assert()
        .success()
        .stdout(predicate::str::contains("blocked_by").not());
}

#[test]
fn ls_limit_caps_rows_after_sort() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    new_ticket(p, "alpha");
    new_ticket(p, "beta");
    new_ticket(p, "gamma");

    // --limit caps to the first N by the sort key (title): alpha, beta — not gamma.
    hs(p)
        .args(["ls", "--sort", "title", "--limit", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"))
        .stdout(predicate::str::contains("gamma").not());
}

#[test]
fn doctor_reports_ok_on_a_healthy_store() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    new_ticket(p, "t");
    hs(p)
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("No issues found"));
}

#[test]
fn claim_next_release_and_renew() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let slug = new_ticket(p, "claim me");

    hs(p)
        .args(["claim-next", "--worker", "w1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Claimed"))
        .stdout(predicate::str::contains(&slug));
    hs(p)
        .args(["show", &slug])
        .assert()
        .success()
        .stdout(predicate::str::contains("claimed_by: w1"))
        .stdout(predicate::str::contains("claim_count: 1"));

    // The only ticket is now claimed → nothing left to claim.
    hs(p)
        .args(["claim-next", "--worker", "w2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No claimable tickets"));

    // Renew: wrong worker rejected, holder accepted.
    hs(p)
        .args(["renew", &slug, "--worker", "w2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("claimed by 'w1'"));
    hs(p)
        .args(["renew", &slug, "--worker", "w1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Renewed"));

    // Release: wrong worker is rejected (needs --force).
    hs(p)
        .args(["release", &slug, "--worker", "w2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("claimed by 'w1'"));
    hs(p)
        .args(["release", &slug, "--worker", "w2", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Released"));
    hs(p)
        .args(["show", &slug])
        .assert()
        .success()
        .stdout(predicate::str::contains("claimed_by").not());
}

#[test]
fn new_accepts_positional_title_up_next_and_tags() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).args(["init"]).assert().success();

    // Positional title (no --title) + --up-next + repeated --tag.
    let out = hs(p)
        .args([
            "new",
            "Fix dashboard flicker",
            "--category",
            "bug",
            "--priority",
            "high",
            "--up-next",
            "--tag",
            "ui",
            "--tag",
            "urgent",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let slug = stdout.split_whitespace().nth(1).unwrap().to_string();

    hs(p)
        .args(["show", &slug])
        .assert()
        .success()
        .stdout(predicate::str::contains("title: Fix dashboard flicker"))
        .stdout(predicate::str::contains("priority: high"))
        .stdout(predicate::str::contains("up_next: true"))
        .stdout(predicate::str::contains("ui"))
        .stdout(predicate::str::contains("urgent"));

    // It shows up under the Up Next filter.
    hs(p)
        .args(["ls", "--up-next"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fix dashboard flicker"));
}

#[test]
fn new_without_a_title_errors() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).args(["init"]).assert().success();
    hs(p)
        .args(["new", "--category", "bug"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("a title is required"));
}

#[test]
fn edit_can_append_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).args(["init"]).assert().success();
    let slug = new_ticket(p, "Fix flicker");

    hs(p)
        .args([
            "edit",
            &slug,
            "--status",
            "started",
            "--note",
            "began investigating",
        ])
        .assert()
        .success();

    hs(p)
        .args(["show", &slug])
        .assert()
        .success()
        .stdout(predicate::str::contains("## Notes"))
        .stdout(predicate::str::contains("began investigating"));
}

#[test]
fn settings_shared_and_local_scopes() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).args(["init"]).assert().success();

    hs(p)
        .args(["settings", "set", "categories", r#"["bug","task"]"#])
        .assert()
        .success()
        .stdout(predicate::str::contains("shared (committed)"));
    hs(p)
        .args([
            "settings",
            "set",
            "index_path",
            "/tmp/idx",
            "--scope",
            "local",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("local (gitignored)"));

    // effective get + list
    hs(p)
        .args(["settings", "get", "categories"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"["bug","task"]"#));
    hs(p)
        .args(["settings", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("categories ="))
        .stdout(predicate::str::contains("index_path = /tmp/idx"));

    // local file is gitignored; shared file is committed (present, not ignored)
    let gi = std::fs::read_to_string(p.join(".gitignore")).unwrap();
    assert!(gi.lines().any(|l| l == "hotsheet-settings.local.json"));
    assert!(p.join("hotsheet-settings.json").is_file());

    // an unknown key errors
    hs(p)
        .args(["settings", "get", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no setting 'nope'"));
}
