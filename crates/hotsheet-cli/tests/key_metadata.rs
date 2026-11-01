//! HS2-HWC0FS: actual headless parents, explicit debug synthetic credential port.
#![cfg(any(unix, windows))]
use assert_cmd::Command;
use std::{path::Path, time::Duration};

fn command(root: &Path, home: &Path) -> Command {
    let mut command = Command::cargo_bin("hotsheet-cli").unwrap();
    let system_root = std::env::var_os("SystemRoot");
    command.env_clear();
    if let Some(system_root) = system_root {
        command.env("SystemRoot", system_root);
    }
    command
        .current_dir(root)
        .env("HOME", root.join("user"))
        .env("USERPROFILE", root.join("user"))
        .env("HOTSHEET_HOME", home)
        .env("CODEX_HOME", root.join("no-codex"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("HOTSHEET_CACHE_DIR", root.join("cache"))
        .env("TMPDIR", root.join("tmp"))
        .env("TMP", root.join("tmp"))
        .env("TEMP", root.join("tmp"))
        .env("HOTSHEET_AI_TOOL_PROBES", "off")
        .timeout(Duration::from_secs(20));
    command
}

#[test]
fn hs2_hwc0fs_cli_key_parents_publish_merge_refuse_and_retry_in_private_home() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("private-key-home");
    for dir in ["user", "config", "data", "state", "cache", "tmp"] {
        std::fs::create_dir(root.path().join(dir)).unwrap();
    }
    let capabilities = command(root.path(), &home)
        .arg("--test-fixture-capabilities")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let capabilities: serde_json::Value = serde_json::from_slice(&capabilities).unwrap();
    assert_eq!(
        capabilities["isolated_native_credentials"], true,
        "rebuild owned debug CLI with hotsheet-aitools/test-permission-vault"
    );
    assert!(
        !home.exists(),
        "capability assertion must precede private credential commands"
    );
    let run = || {
        let mut command = command(root.path(), &home);
        command.arg("--test-fixture-credentials");
        command
    };
    let account = "github-app-hwc-cli-synthetic";
    run()
        .args(["key", "set", account])
        .write_stdin("synthetic-first\n")
        .assert()
        .success();
    let first = std::fs::read(home.join("keys.json")).unwrap();
    assert!(!String::from_utf8_lossy(&first).contains("synthetic-first"));
    run()
        .args(["key", "get", account])
        .assert()
        .success()
        .stdout("synthetic-first\n");
    run()
        .args(["key", "set", "hwc-unrelated"])
        .write_stdin("synthetic-unrelated\n")
        .assert()
        .success();
    run()
        .args(["key", "set", account])
        .write_stdin("synthetic-second\n")
        .assert()
        .success();
    let committed = std::fs::read(home.join("keys.json")).unwrap();
    let rows: serde_json::Value = serde_json::from_slice(&committed).unwrap();
    let old: serde_json::Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(rows.as_object().unwrap().len(), 2);
    assert_ne!(
        rows[account]["credential_generation"],
        old[account]["credential_generation"]
    );
    let listed = run()
        .args(["key", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed = String::from_utf8(listed).unwrap();
    assert!(listed.contains(account));
    assert!(listed.contains("hwc-unrelated"));
    assert!(!listed.contains("credential_generation"));
    assert!(!listed.contains("synthetic-second"));
    std::fs::write(home.join("keys.json"), b"{").unwrap();
    run()
        .args(["key", "set", account])
        .write_stdin("must-not-replace\n")
        .assert()
        .failure();
    assert_eq!(std::fs::read(home.join("keys.json")).unwrap(), b"{");
    run()
        .args(["key", "get", account])
        .assert()
        .success()
        .stdout("synthetic-second\n");
    // Deliberate synthetic corruption recovery, not an authorization for raw-map
    // restore in the product. Stronger restore fencing remains HS2-67SP63.
    std::fs::write(home.join("keys.json"), committed).unwrap();
    run()
        .args(["key", "set", account])
        .write_stdin("synthetic-recovered\n")
        .assert()
        .success();
    run().args(["key", "delete", account]).assert().success();
    run().args(["key", "delete", account]).assert().success();
    run().args(["key", "get", account]).assert().failure();
    run()
        .args(["key", "get", "hwc-unrelated"])
        .assert()
        .success()
        .stdout("synthetic-unrelated\n");
    let final_rows: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.join("keys.json")).unwrap()).unwrap();
    assert!(final_rows.get(account).is_none());
    assert!(final_rows.get("hwc-unrelated").is_some());
}
