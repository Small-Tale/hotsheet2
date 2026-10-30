//! HS2-JHHXJM headless E2E: `hotsheet-cli checkout views|set-view|remove-view` manage a
//! checkout's saved views, the same shared setting the web client's `/views` routes edit.

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::{Value, json};

#[test]
fn hs2_jhhxjm_saved_views_are_listed_set_idempotently_and_removed() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let project = root.path().join("app");
    let store = root.path().join("app.hs2");
    std::fs::create_dir_all(&project).unwrap();
    hotsheet_ticketing::FsStore::init(&store, &hotsheet_ticketing::StoreMetadata::new("HS"))
        .unwrap();
    let hs = || {
        let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
        cmd.env("HOTSHEET_HOME", &home)
            .env_remove("HOTSHEET_STORE")
            .arg("-C")
            .arg(&store)
            .arg("checkout");
        cmd
    };
    hs().arg("register")
        .arg(&project)
        .arg("--store")
        .arg(&store)
        .assert()
        .success();
    let views = || -> Value {
        serde_json::from_slice(
            &hs()
                .args(["views", "app"])
                .assert()
                .success()
                .get_output()
                .stdout,
        )
        .unwrap()
    };
    assert_eq!(views(), json!([]));

    let set = |args: &[&str]| {
        let mut cmd = hs();
        cmd.args(["set-view", "app"]).args(args);
        cmd
    };
    set(&["docs", "--name", "Docs", "--query", "tag:docs AND is:open"])
        .assert()
        .success()
        .stdout(contains("Saved view docs"));
    set(&["DOCS", "--query", "tag:docs AND is:open"])
        .assert()
        .success()
        .stdout(contains("unchanged"));
    set(&["release", "--query", " is:up-next "])
        .assert()
        .success();
    assert_eq!(
        views(),
        json!([
            {"id": "docs", "name": "Docs", "query": "tag:docs AND is:open"},
            {"id": "release", "name": "release", "query": "is:up-next"},
        ])
    );
    // An update keeps the stored id spelling and name unless a new name is given.
    set(&["Docs", "--query", "tag:docs"]).assert().success();
    assert_eq!(
        views()[0],
        json!({"id": "docs", "name": "Docs", "query": "tag:docs"})
    );

    // Validation is the server's: bad ids, duplicate names, and empty queries are refused.
    set(&["no spaces", "--query", "x"])
        .assert()
        .failure()
        .stderr(contains("letters, numbers, or hyphens"));
    set(&["other", "--name", "docs", "--query", "x"])
        .assert()
        .failure()
        .stderr(contains("names must be unique"));
    set(&["empty", "--query", "  "])
        .assert()
        .failure()
        .stderr(contains("queries"));
    assert_eq!(
        views().as_array().unwrap().len(),
        2,
        "a refusal writes nothing"
    );

    hs().args(["remove-view", "app", "RELEASE"])
        .assert()
        .success()
        .stdout(contains("Removed saved view"));
    hs().args(["remove-view", "app", "release"])
        .assert()
        .success()
        .stdout(contains("No saved view"));
    assert_eq!(
        views(),
        json!([{"id": "docs", "name": "Docs", "query": "tag:docs"}])
    );
    hs().args(["views", "nowhere"]).assert().failure();
}
