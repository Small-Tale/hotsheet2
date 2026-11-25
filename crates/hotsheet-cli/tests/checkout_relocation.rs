//! HS2-86ES0B headless parity: after `hotsheet-cli checkout relocate`, `checkout resolve`
//! of the remembered old root names the checkout at its new root, as a restored client tab
//! does. A folder moved without telling the registry stays unresolved.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

fn hs(home: &Path, store: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(workspace_bin::path("hotsheet-cli"));
    cmd.env("HOTSHEET_HOME", home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .arg("-C")
        .arg(store)
        .args(args);
    cmd
}

fn json(cmd: &mut Command) -> Value {
    serde_json::from_slice(&cmd.assert().success().get_output().stdout).unwrap()
}

#[test]
fn hs2_86es0b_resolve_follows_a_relocated_checkout_from_its_old_root() {
    let temp = tempfile::tempdir().unwrap();
    let base = hotsheet_sync::paths::canonicalize(temp.path()).unwrap();
    let (home, store) = (base.join("home"), base.join("app.hs2"));
    let (original, moved) = (base.join("app"), base.join("moved"));
    std::fs::create_dir(&original).unwrap();
    hotsheet_ticketing::FsStore::init_git(&store, &hotsheet_ticketing::StoreMetadata::new("AP"))
        .unwrap();
    let old = original.to_str().unwrap();
    let registered = json(&mut hs(
        &home,
        &store,
        &[
            "checkout",
            "register",
            old,
            "--store",
            store.to_str().unwrap(),
        ],
    ));
    let id = registered["id"].as_str().unwrap();

    std::fs::rename(&original, &moved).unwrap();
    hs(&home, &store, &["checkout", "resolve", old])
        .assert()
        .failure();

    let relocated = json(&mut hs(
        &home,
        &store,
        &["checkout", "relocate", id, moved.to_str().unwrap()],
    ));
    assert_eq!(relocated["root"], moved.to_str().unwrap());
    let resolved = json(&mut hs(&home, &store, &["checkout", "resolve", old]));
    assert_eq!(resolved["id"], id);
    assert_eq!(resolved["root"], moved.to_str().unwrap());
}
