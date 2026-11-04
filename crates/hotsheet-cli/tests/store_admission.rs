//! HS2-726EXJ: the headless source-selection paths (`link`, `checkout add-source`,
//! `setup`) admit a ticket store only when it is an owned or designated Git store and is
//! not the machine bootstrap anchor. A refusal exits non-zero with the ownership
//! diagnostic; the printed recovery admits the store on the next selection.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use hotsheet_ticketing::checkouts::TicketSource;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use predicates::prelude::*;

fn git(dir: &Path, args: &[&str]) {
    let status = hotsheet_ticketing::git::command_in(dir)
        .args([
            "-c",
            "user.name=Hot Sheet Test",
            "-c",
            "user.email=test@localhost",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn cli(home: &Path, cwd: &Path) -> Command {
    let mut command = Command::cargo_bin("hotsheet-cli").unwrap();
    command
        .current_dir(cwd)
        .env("HOTSHEET_HOME", home)
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID");
    command
}

struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    Fixture {
        home,
        project,
        _dir: dir,
    }
}

fn registry(home: &Path) -> serde_json::Value {
    std::fs::read(home.join("checkouts.json"))
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
        .unwrap_or(serde_json::Value::Null)
}

#[test]
fn hs2_726exj_link_refuses_a_repository_less_store_until_it_is_one() {
    let fixture = fixture();
    let store = fixture._dir.path().join("loose.hs2");
    FsStore::init(&store, &StoreMetadata::new("LO")).unwrap();
    cli(&fixture.home, &fixture.project)
        .arg("link")
        .arg(&store)
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not a Git repository"))
        .stderr(predicate::str::contains("git -C"));
    assert!(!fixture.project.join(".hotsheet2/store").exists());

    git(&store, &["init", "-q", "-b", "main"]);
    cli(&fixture.home, &fixture.project)
        .arg("link")
        .arg(&store)
        .assert()
        .success();
    assert!(fixture.project.join(".hotsheet2/store").is_file());
}

#[test]
fn hs2_726exj_link_refuses_a_store_its_enclosing_repository_does_not_track() {
    let fixture = fixture();
    std::fs::write(fixture.project.join(".gitignore"), "target/\n").unwrap();
    let store = fixture.project.join("target/store");
    FsStore::init(&store, &StoreMetadata::new("UN")).unwrap();
    cli(&fixture.home, &fixture.project)
        .arg("link")
        .arg(&store)
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not track it"));
}

#[test]
fn hs2_726exj_link_admits_a_designated_root() {
    let fixture = fixture();
    let store = fixture.project.join("tickets");
    FsStore::init(&store, &StoreMetadata::new("DE")).unwrap();
    git(
        &fixture.project,
        &["add", "--", "tickets/hotsheet-store.json"],
    );
    cli(&fixture.home, &fixture.project)
        .arg("link")
        .arg(&store)
        .assert()
        .success();
}

#[test]
fn hs2_726exj_the_machine_anchor_is_never_linked_as_a_ticket_source() {
    let fixture = fixture();
    let anchor = fixture.home.join("server-bootstrap.hs2");
    FsStore::init(&anchor, &StoreMetadata::new("HS")).unwrap();
    // Even a repository-owned anchor stays metadata-only by contract.
    git(&anchor, &["init", "-q", "-b", "main"]);
    cli(&fixture.home, &fixture.project)
        .arg("link")
        .arg(&anchor)
        .assert()
        .failure()
        .stderr(predicate::str::contains("machine bootstrap store"));
}

#[test]
fn hs2_726exj_checkout_add_source_refuses_an_unowned_store() {
    let fixture = fixture();
    cli(&fixture.home, &fixture.project)
        .args(["checkout", "register"])
        .arg(&fixture.project)
        .args(["--alias", "app"])
        .assert()
        .success();
    let store = fixture._dir.path().join("loose.hs2");
    FsStore::init(&store, &StoreMetadata::new("LO")).unwrap();
    let source = TicketSource::git(&store);
    let add = |fixture: &Fixture| {
        let mut command = cli(&fixture.home, &fixture.project);
        command
            .args([
                "checkout",
                "add-source",
                "app",
                &source.connection_id,
                "git",
            ])
            .arg(&store);
        command
    };
    add(&fixture)
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not a Git repository"));
    assert_eq!(
        registry(&fixture.home)["checkouts"][0]["sources"],
        serde_json::json!([])
    );

    git(&store, &["init", "-q", "-b", "main"]);
    add(&fixture).assert().success();
    assert_eq!(
        registry(&fixture.home)["checkouts"][0]["sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn hs2_726exj_setup_keeps_a_project_source_free_rather_than_linking_an_unowned_store() {
    let fixture = fixture();
    let store = fixture._dir.path().join("loose.hs2");
    FsStore::init(&store, &StoreMetadata::new("LO")).unwrap();
    cli(&fixture.home, &fixture.project)
        .arg("-C")
        .arg(&store)
        .args(["setup", "claude", "--project"])
        .arg(&fixture.project)
        .assert()
        .success()
        .stderr(predicate::str::contains("not linking"));
    let registry = registry(&fixture.home);
    let checkout = registry["checkouts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|checkout| {
            Path::new(checkout["root"].as_str().unwrap()) == fixture.project.canonicalize().unwrap()
        })
        .expect("setup registers the code project");
    assert_eq!(checkout["sources"], serde_json::json!([]));
}
