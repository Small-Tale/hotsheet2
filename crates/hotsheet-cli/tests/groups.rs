//! End-to-end ticket group flows through the built `hotsheet-cli` binary (HS2-PDSHNN,
//! HS2-8CDFKR): the headless parity of the group field, filters, listing, rename, and
//! ungroup.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use std::sync::OnceLock;

fn hs(dir: &Path) -> Command {
    static HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
    let home = HOME.get_or_init(|| {
        let path =
            std::env::temp_dir().join(format!("hotsheet-cli-group-tests-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    });
    let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
    cmd.arg("--test-fixture-credentials")
        .env("HOTSHEET_HOME", home)
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .arg("-C")
        .arg(dir);
    cmd
}

fn stdout(dir: &Path, args: &[&str]) -> String {
    let out = hs(dir).args(args).assert().success();
    String::from_utf8(out.get_output().stdout.clone()).unwrap()
}

fn new_ticket(dir: &Path, args: &[&str]) -> String {
    let mut all = vec!["new"];
    all.extend_from_slice(args);
    stdout(dir, &all)
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string()
}

fn groups_json(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&stdout(dir, &["groups", "--json"])).unwrap()
}

#[test]
fn hs2_pdshnn_group_field_filters_and_listing_work_headlessly() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).args(["init", "--prefix", "HS"]).assert().success();
    assert!(stdout(p, &["groups"]).contains("(no groups)"));
    assert_eq!(groups_json(p), serde_json::json!({ "groups": [] }));

    let first = new_ticket(
        p,
        &["--title", "first", "--group", "  Board   Drag & Drop "],
    );
    let second = new_ticket(p, &["--title", "second"]);
    let third = new_ticket(p, &["--title", "third", "--group", "alpha"]);
    assert!(stdout(p, &["show", &first]).contains("group: Board Drag & Drop\n"));
    assert!(!stdout(p, &["show", &second]).contains("group:"));

    // An existing group's spelling is reused; a ticket has at most one group.
    hs(p)
        .args(["edit", &second, "--group", "BOARD drag & drop"])
        .assert()
        .success();
    assert!(stdout(p, &["show", &second]).contains("group: Board Drag & Drop\n"));
    assert_eq!(
        groups_json(p),
        serde_json::json!({ "groups": [
            { "name": "alpha", "count": 1 },
            { "name": "Board Drag & Drop", "count": 2 },
        ] })
    );
    let table = stdout(p, &["groups"]);
    assert!(table.contains("    2  Board Drag & Drop"));
    assert!(table.find("alpha").unwrap() < table.find("Board").unwrap());

    let grouped = stdout(p, &["ls", "--group", "board   DRAG & drop"]);
    assert!(grouped.contains(&first) && grouped.contains(&second) && !grouped.contains(&third));
    assert!(grouped.contains("{Board Drag & Drop}"));
    let any = stdout(p, &["ls", "--has-group"]);
    assert!(any.contains(&first) && any.contains(&third));
    hs(p)
        .args(["edit", &second, "--clear-group"])
        .assert()
        .success();
    let none = stdout(p, &["ls", "--no-group"]);
    assert!(none.contains(&second) && !none.contains(&first) && !none.contains(&third));
    assert!(stdout(p, &["ls", "--group", "no such group"]).contains("(no tickets)"));

    // Conflicting flags and invalid names are refused without a write.
    hs(p)
        .args(["edit", &first, "--group", "x", "--clear-group"])
        .assert()
        .failure();
    hs(p)
        .args(["ls", "--has-group", "--no-group"])
        .assert()
        .failure();
    hs(p)
        .args(["edit", &first, "--group", "line\nbreak"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("control characters"));
    hs(p)
        .args(["new", "--title", "bad", "--group", &"x".repeat(121)])
        .assert()
        .failure()
        .stderr(predicate::str::contains("120 characters"));
    assert!(stdout(p, &["show", &first]).contains("group: Board Drag & Drop\n"));

    // A batch respells a group when its only members are the selected tickets.
    hs(p)
        .args(["edit", &second, "--group", "Board Drag & Drop"])
        .assert()
        .success();
    hs(p)
        .args(["batch", &first, &second, "--group", "board drag & drop"])
        .assert()
        .success();
    assert_eq!(
        groups_json(p)["groups"][1],
        serde_json::json!({ "name": "board drag & drop", "count": 2 })
    );
    // A trashed ticket no longer keeps its group alive.
    hs(p)
        .args(["edit", &third, "--status", "deleted"])
        .assert()
        .success();
    assert_eq!(groups_json(p)["groups"].as_array().unwrap().len(), 1);
}

#[test]
fn hs2_8cdfkr_group_rename_and_ungroup_work_headlessly() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    hs(p).args(["init", "--prefix", "HS"]).assert().success();
    let a = new_ticket(p, &["--title", "a", "--group", "Old"]);
    let b = new_ticket(p, &["--title", "b", "--group", "old"]);
    let other = new_ticket(p, &["--title", "other", "--group", "Taken"]);

    hs(p)
        .args(["group", "rename", "missing", "New"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no group named 'missing'"));
    hs(p)
        .args(["group", "rename", "Old", "taken"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "a group named 'Taken' already exists",
        ));
    hs(p)
        .args(["group", "rename", "Old", "   "])
        .assert()
        .failure();
    assert!(stdout(p, &["show", &a]).contains("group: Old\n"));

    assert!(
        stdout(p, &["group", "rename", "OLD", "New   Name"])
            .contains("Renamed group to 'New Name' on 2 ticket(s).")
    );
    for slug in [&a, &b] {
        assert!(stdout(p, &["show", slug]).contains("group: New Name\n"));
    }
    assert!(stdout(p, &["show", &other]).contains("group: Taken\n"));
    // Case-only rename of the same group is allowed.
    assert!(
        stdout(p, &["group", "rename", "new name", "NEW NAME"])
            .contains("Renamed group to 'NEW NAME' on 2 ticket(s).")
    );

    assert!(stdout(p, &["group", "ungroup", "new name"]).contains("Ungrouped 2 ticket(s)."));
    assert!(!stdout(p, &["show", &a]).contains("group:"));
    hs(p)
        .args(["group", "ungroup", "new name"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no group named"));
    assert_eq!(
        groups_json(p),
        serde_json::json!({ "groups": [{ "name": "Taken", "count": 1 }] })
    );
}
