//! HS2-67N8SJ: the headless `coordination repair-identity` recovery through the real CLI.
//! A disposable checkout and store move; an isolated machine home holds the registry.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use hotsheet_ticketing::checkouts::{CheckoutRegistry, TicketSource};
use hotsheet_ticketing::{FsStore, StoreMetadata};

#[path = "../../test-support/first_launch.rs"]
mod first_launch;
#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

fn cli(home: &Path, store: &Path, project: &Path, args: &[&str]) -> Output {
    std::process::Command::new(first_launch::launched(
        workspace_bin::path("hotsheet-cli"),
        &["--version"],
    ))
    .arg("-C")
    .arg(store)
    .arg("coordination")
    .arg("--project")
    .arg(project)
    .args(args)
    .env("HOTSHEET_HOME", home)
    .env("HOTSHEET_NATIVE_SECRETS", "deny")
    .env("HOTSHEET_ACTOR_ROLE", "system")
    .env_remove("HOTSHEET_ACTOR_ID")
    .env_remove("HOTSHEET_WORKER_ID")
    .output()
    .unwrap()
}

fn ok(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn git_init(path: &Path) {
    fs::create_dir(path).unwrap();
    assert!(
        hotsheet_ticketing::git::launch("git")
            .args(["init", "-q"])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

struct Moved {
    _root: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    store: PathBuf,
}

/// Create a pool at the old paths, move both, and record the move through the registry.
fn moved(record: bool) -> Moved {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let (old_project, old_store) = (
        root.path().join("old-project"),
        root.path().join("old-store"),
    );
    git_init(&old_project);
    FsStore::init_git(&old_store, &StoreMetadata::new("HS")).unwrap();
    ok(cli(
        &home,
        &old_store,
        &old_project,
        &[
            "register",
            "--role",
            "worker",
            "--id",
            "w",
            "--session",
            "w-1",
        ],
    ));
    let registry = CheckoutRegistry::new(home.join("checkouts.json"));
    let old_connection = TicketSource::git(&old_store).connection_id;
    let id = if record {
        registry
            .register_sources(
                &old_project,
                None,
                None,
                vec![TicketSource::git(&old_store)],
                None,
            )
            .unwrap()
            .id
    } else {
        String::new()
    };
    let (project, store) = (root.path().join("project"), root.path().join("store"));
    fs::rename(&old_project, &project).unwrap();
    fs::rename(&old_store, &store).unwrap();
    if record {
        registry.relocate(&id, &project, None).unwrap();
        registry
            .relink_git_source(&id, &old_connection, &store)
            .unwrap();
    }
    Moved {
        _root: root,
        home,
        project,
        store,
    }
}

#[test]
fn hs2_67n8sj_repair_identity_dry_run_then_repair_then_current() {
    let m = moved(true);
    let pool = m.project.join(".git/hotsheet-coordination/state.json");
    let bytes = fs::read(&pool).unwrap();
    let dry = ok(cli(
        &m.home,
        &m.store,
        &m.project,
        &["repair-identity", "--dry-run"],
    ));
    assert_eq!(dry["status"], "equivalent");
    assert_eq!(dry["migrated"], false);
    assert_ne!(dry["stored_project_id"], dry["current_project_id"]);
    assert_eq!(fs::read(&pool).unwrap(), bytes, "dry run never writes");
    let repaired = ok(cli(&m.home, &m.store, &m.project, &["repair-identity"]));
    assert_eq!(repaired["migrated"], true);
    let again = ok(cli(&m.home, &m.store, &m.project, &["repair-identity"]));
    assert_eq!(again["status"], "current");
    // The ordinary pool keeps its worker.
    let status = ok(cli(&m.home, &m.store, &m.project, &["status"]));
    assert!(status["state"]["workers"]["w"].is_object(), "{status}");
}

#[test]
fn hs2_67n8sj_an_unproven_pool_is_refused_and_named_in_the_error() {
    let m = moved(false);
    let pool = m.project.join(".git/hotsheet-coordination/state.json");
    let bytes = fs::read(&pool).unwrap();
    let status = cli(&m.home, &m.store, &m.project, &["status"]);
    assert!(!status.status.success());
    let stderr = String::from_utf8_lossy(&status.stderr);
    assert!(
        stderr.contains("incompatible coordination state"),
        "{stderr}"
    );
    assert!(
        stderr.contains("coordination --project") && stderr.contains("repair-identity --dry-run"),
        "{stderr}"
    );
    let repair = cli(&m.home, &m.store, &m.project, &["repair-identity"]);
    assert!(!repair.status.success());
    let report: serde_json::Value = serde_json::from_slice(&repair.stdout).unwrap();
    assert_eq!(report["status"], "refused");
    assert!(String::from_utf8_lossy(&repair.stderr).contains("left unchanged"));
    assert_eq!(fs::read(&pool).unwrap(), bytes);
}
