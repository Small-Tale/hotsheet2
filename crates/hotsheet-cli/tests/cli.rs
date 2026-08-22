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

/// LIVE, gated: a real `hotsheet-cli trigger codex` drives codex in an auto-built,
/// MCP-free isolated CODEX_HOME (HS2-YRDQNX) and completes a turn, leaving the user's
/// `~/.codex` and any HS1 instance untouched. Off by default; set `HOTSHEET_CODEX_LIVE=1`
/// (needs codex + creds; invokes the model).
#[test]
#[ignore = "live: needs a real codex + creds; set HOTSHEET_CODEX_LIVE=1"]
fn trigger_codex_isolates_codex_home_and_completes() {
    if std::env::var("HOTSHEET_CODEX_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set HOTSHEET_CODEX_LIVE=1 to run the live codex trigger");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let t = new_ticket(p, "Create GREETING.txt containing hello");
    hs(p).args(["edit", &t, "--up-next"]).assert().success();

    // Bare `trigger codex` — no --env CODEX_HOME — must auto-isolate and NOT refuse.
    hs(p)
        .args([
            "trigger",
            "codex",
            "--prompt",
            "Create a file named GREETING.txt containing the word hello in this project, \
             then stop. If you use the shell, the CLI is hotsheet-cli (never a bare 'hotsheet').",
        ])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();

    assert!(
        p.join("GREETING.txt").is_file(),
        "codex created GREETING.txt"
    );
    assert!(
        !p.join(".hotsheet").exists(),
        "no HS1 instance was launched (no .hotsheet)"
    );
}

/// LIVE, gated: `hotsheet-cli trigger codex --shared-daemon` drives codex via a **daemon**
/// started for the isolated CODEX_HOME (HS2-B7C66H) — reusing one codex instance while
/// keeping MCP isolation — and completes a turn. Off by default; set `HOTSHEET_CODEX_LIVE=1`
/// (needs codex + creds; invokes the model).
#[test]
#[ignore = "live: needs a real codex + creds; set HOTSHEET_CODEX_LIVE=1"]
fn trigger_codex_shared_daemon_reuses_one_instance() {
    if std::env::var("HOTSHEET_CODEX_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set HOTSHEET_CODEX_LIVE=1 to run the shared-daemon codex trigger");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let t = new_ticket(p, "Create GREETING.txt containing hello");
    hs(p).args(["edit", &t, "--up-next"]).assert().success();

    hs(p)
        .args([
            "trigger",
            "codex",
            "--shared-daemon",
            "--prompt",
            "Create a file named GREETING.txt containing the word hello in this project, \
             then stop. If you use the shell, the CLI is hotsheet-cli (never a bare 'hotsheet').",
        ])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();

    assert!(
        p.join("GREETING.txt").is_file(),
        "codex created GREETING.txt"
    );
    assert!(
        !p.join(".hotsheet").exists(),
        "no HS1 instance was launched"
    );
}

/// LIVE, gated: `hotsheet-cli work codex --shared-daemon` drains a one-ticket queue via a
/// daemon started for the isolated CODEX_HOME (HS2-B7C66H) and, when the loop ends, leaves
/// **no orphaned daemon home** behind (HS2-9M6T68). Off by default; set HOTSHEET_CODEX_LIVE=1.
#[test]
#[ignore = "live: needs a real codex + creds; set HOTSHEET_CODEX_LIVE=1"]
fn work_shared_daemon_completes_and_leaves_no_orphan_home() {
    if std::env::var("HOTSHEET_CODEX_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set HOTSHEET_CODEX_LIVE=1 to run the shared-daemon work loop");
        return;
    }
    let count_homes = || {
        std::fs::read_dir("/tmp")
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with("hs2cx-"))
                    .count()
            })
            .unwrap_or(0)
    };
    let before = count_homes();

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let t = new_ticket(p, "Create GREETING.txt containing hello");
    hs(p).args(["edit", &t, "--up-next"]).assert().success();
    // Seed the file-creation instruction into the ticket the loop will read.
    hs(p)
        .args([
            "edit",
            &t,
            "--details",
            "Create a file named GREETING.txt containing the word hello in this project.",
        ])
        .assert()
        .success();

    hs(p)
        .args(["work", "codex", "--shared-daemon", "--max", "3"])
        .timeout(std::time::Duration::from_secs(240))
        .assert()
        .success();

    assert!(
        !p.join(".hotsheet").exists(),
        "no HS1 instance was launched"
    );
    // The daemon home is torn down on loop exit — no NEW /tmp/hs2cx-* left behind.
    assert!(
        count_homes() <= before,
        "shared-daemon loop leaked an isolated CODEX_HOME under /tmp"
    );
}

/// End-to-end: `init` registers the semantic merge driver, and a real git merge of two
/// branches that edited the SAME ticket lands cleanly — frontmatter merged field-by-field,
/// tags unioned, both notes kept — with no conflict markers (HS2-18).
#[test]
fn merge_driver_resolves_concurrent_ticket_edits() {
    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    git(p, &["config", "user.email", "t@example.com"]);
    git(p, &["config", "user.name", "Tester"]);

    // Driver registered by init.
    let attrs = std::fs::read_to_string(p.join(".gitattributes")).unwrap();
    assert!(attrs.contains("tickets/**/*.md merge=hotsheet-ticket"));

    // `new`/`edit` auto-commit, so the base state is already committed on the base branch.
    let slug = new_ticket(p, "Concurrent edits");
    hs(p)
        .args(["edit", &slug, "--details", "shared body"])
        .assert()
        .success();
    let base_branch = String::from_utf8(
        std::process::Command::new("git")
            .current_dir(p)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let base_branch = base_branch.trim();

    // Branch A: priority high + tag alpha + a note (each edit auto-commits).
    git(p, &["checkout", "-q", "-b", "branch-a"]);
    hs(p)
        .args([
            "edit",
            &slug,
            "--priority",
            "high",
            "--tag",
            "alpha",
            "--note",
            "from A",
        ])
        .assert()
        .success();

    // Branch B off the base: status started + tag beta + a note.
    git(p, &["checkout", "-q", base_branch]);
    git(p, &["checkout", "-q", "-b", "branch-b"]);
    hs(p)
        .args([
            "edit", &slug, "--status", "started", "--tag", "beta", "--note", "from B",
        ])
        .assert()
        .success();

    // Merge A into B — must succeed with no conflict markers.
    let merged = std::process::Command::new("git")
        .current_dir(p)
        .args(["merge", "branch-a", "-m", "merge"])
        .status()
        .unwrap();
    assert!(
        merged.success(),
        "semantic merge should resolve automatically"
    );

    let show = hs(p).args(["show", &slug]).assert().success();
    let out = String::from_utf8(show.get_output().stdout.clone()).unwrap();
    assert!(
        out.contains("priority: high"),
        "A's priority merged in:\n{out}"
    );
    assert!(out.contains("status: started"), "B's status merged in");
    assert!(
        out.contains("alpha") && out.contains("beta"),
        "tags unioned"
    );
    assert!(
        out.contains("from A") && out.contains("from B"),
        "both notes kept"
    );
    assert!(
        !out.contains("<<<<<<<"),
        "no conflict markers in a clean merge"
    );
}

/// Cross-store copy & move (HS2-60): copy makes a fresh SEC-prefixed ticket (source
/// untouched); move keeps the ULID and leaves a `moved` tombstone; move refuses without
/// --yes (the retention/exposure gate).
#[test]
fn copy_and_move_between_stores() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    hs(src.path())
        .args(["init", "--prefix", "HS"])
        .assert()
        .success();
    hs(dest.path())
        .args(["init", "--prefix", "SEC"])
        .assert()
        .success();

    // Copy: new SEC ticket in dest, original still in src.
    let a = new_ticket(src.path(), "copy me");
    hs(src.path())
        .args(["copy", &a, "--to", dest.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Copied"));
    hs(dest.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("SEC-").and(predicate::str::contains("copy me")));
    // source keeps the original.
    hs(src.path()).args(["show", &a]).assert().success();

    // Move without --yes is refused (retention/exposure gate).
    let b = new_ticket(src.path(), "move me");
    hs(src.path())
        .args(["move", &b, "--to", dest.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));

    // Move with --yes: same ULID in dest (SEC slug), tombstone in source.
    hs(src.path())
        .args(["move", &b, "--to", dest.path().to_str().unwrap(), "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Moved"));
    // The source ticket is now a moved tombstone.
    hs(src.path())
        .args(["show", &b])
        .assert()
        .success()
        .stdout(predicate::str::contains("status: moved"))
        .stdout(predicate::str::contains("moved_to_store:"));
    // The destination has it live under a SEC slug (same ULID → resolvable by the old ULID
    // is not asserted here since the CLI shows by slug; ls proves it landed).
    hs(dest.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("move me"));
}

/// End-to-end sync across two clones of a bare remote (HS2-19): each side pushes its own
/// ticket, and a sync on the other side pulls it in (rebase) and pushes — hands-off.
#[test]
fn sync_pulls_and_pushes_between_clones() {
    fn git(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .unwrap()
            .success()
    }
    let bare = tempfile::tempdir().unwrap();
    assert!(git(bare.path(), &["init", "--bare", "--quiet"]));
    let bare_url = bare.path().to_str().unwrap();

    // Store A: init + remote + first ticket, then publish.
    let a = tempfile::tempdir().unwrap();
    hs(a.path()).arg("init").assert().success();
    git(a.path(), &["config", "user.email", "a@example.com"]);
    git(a.path(), &["config", "user.name", "A"]);
    git(a.path(), &["remote", "add", "origin", bare_url]);
    let tx = new_ticket(a.path(), "ticket X");
    hs(a.path()).arg("sync").assert().success();

    // Store B: clone the bare (inherits X).
    let bwrap = tempfile::tempdir().unwrap();
    let b = bwrap.path().join("clone");
    assert!(git(
        bwrap.path(),
        &["clone", "--quiet", bare_url, b.to_str().unwrap()]
    ));
    git(&b, &["config", "user.email", "b@example.com"]);
    git(&b, &["config", "user.name", "B"]);
    hs(&b).args(["show", &tx]).assert().success(); // B sees X

    // B adds Y and syncs (push).
    let ty = new_ticket(&b, "ticket Y");
    hs(&b).arg("sync").assert().success();

    // A adds Z and syncs — must PULL Y (rebase) and push Z.
    let tz = new_ticket(a.path(), "ticket Z");
    hs(a.path())
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("pulled"));

    // A now holds all three tickets.
    for t in [&tx, &ty, &tz] {
        hs(a.path()).args(["show", t]).assert().success();
    }
}

/// Per-user read tracking via the gitignored local overlay (HS2-21): a fresh ticket lists
/// as unread (●); `read` clears it; the state lives in `local/reads.json`, gitignored.
#[test]
fn read_tracking_marks_and_is_gitignored() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let slug = new_ticket(p, "read me");

    // Unread → the ● marker appears.
    hs(p)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("●"));

    hs(p)
        .args(["read", &slug])
        .assert()
        .success()
        .stdout(predicate::str::contains("Marked"));

    // Read → no ● marker for the single ticket.
    hs(p)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("●").not());

    // Durable on disk + gitignored (never committed).
    assert!(p.join("local/reads.json").is_file());
    let gi = std::fs::read_to_string(p.join(".gitignore")).unwrap();
    assert!(
        gi.lines().any(|l| l.trim() == "local/"),
        "local/ gitignored: {gi}"
    );
}

/// Human assignment + the people.json roster (HS2-20): add a person, assign a ticket + a
/// review request, filter by assignee, and clear.
#[test]
fn assign_and_people_roster() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).arg("init").assert().success();
    let slug = new_ticket(p, "needs an owner");

    // Roster: add + list (committed people.json).
    hs(p)
        .args(["people", "add", "dana@example.com", "--name", "Dana"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added"));
    hs(p)
        .args(["people", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dana@example.com").and(predicate::str::contains("Dana")));
    assert!(p.join("people.json").is_file());

    // Assign a doer + a soft review request.
    hs(p)
        .args([
            "assign",
            &slug,
            "--to",
            "dana@example.com",
            "--review",
            "sam@example.com:review",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Assigned"));
    hs(p)
        .args(["show", &slug])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("assignees").and(predicate::str::contains("dana@example.com")),
        )
        .stdout(predicate::str::contains("review_requests"));

    // Filter by assignee.
    hs(p)
        .args(["ls", "--assignee", "dana@example.com"])
        .assert()
        .success()
        .stdout(predicate::str::contains("needs an owner"));
    hs(p)
        .args(["ls", "--assignee", "nobody@example.com"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no tickets)"));

    // Clear assignees.
    hs(p).args(["assign", &slug, "--clear"]).assert().success();
    hs(p)
        .args(["ls", "--assignee", "dana@example.com"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no tickets)"));
}
