//! HS2-8X2S08 headless E2E: `hotsheet-cli remote-change` through the real binary with an
//! injected `HOTSHEET_HOME`. The native credential store is denied, so nothing here can reach
//! the keychain. Synthetic names only.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

struct Run {
    ok: bool,
    json: Value,
    stdout: String,
    stderr: String,
}

fn run(home: &Path, args: &[&str]) -> Run {
    let output = Command::new(workspace_bin::path("hotsheet-cli"))
        .arg("remote-change")
        .args(args)
        .env("HOTSHEET_HOME", home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
        .env_remove("HOTSHEET_STORE")
        .current_dir(home.parent().unwrap())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    Run {
        ok: output.status.success(),
        json: serde_json::from_str(&stdout).unwrap_or(Value::Null),
        stdout,
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn home() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    (root, home)
}

#[test]
fn hs2_8x2s08_an_empty_machine_lists_nothing_and_unknown_plugins_are_refused_explicitly() {
    let (_root, home) = home();
    let listed = run(&home, &["list"]);
    assert!(listed.ok, "{}{}", listed.stdout, listed.stderr);
    assert_eq!(listed.json["connections"].as_array().unwrap().len(), 0);
    assert_eq!(listed.json["bindings"].as_array().unwrap().len(), 0);
    assert!(
        !home.join("remote-change").exists(),
        "listing creates nothing"
    );

    let unsupported = run(
        &home,
        &["connection", "add", "c1", "--plugin", "carrier-pigeon"],
    );
    assert!(!unsupported.ok);
    assert!(
        unsupported.stderr.contains("unsupported_plugin"),
        "{}",
        unsupported.stderr
    );
    assert!(
        !home.join("remote-change").exists(),
        "a refused add writes nothing"
    );

    let missing = run(&home, &["connection", "show", "ghost"]);
    assert!(!missing.ok);
    assert!(missing.stderr.contains("not_found"), "{}", missing.stderr);
}

#[test]
fn hs2_8x2s08_a_bad_source_and_a_repeated_unbind_are_handled_without_a_stack_trace() {
    let (_root, home) = home();
    let bad = run(
        &home,
        &[
            "bind",
            "elsewhere",
            "--connection",
            "c1",
            "--channel",
            "ch",
            "--repository",
            "acme/app",
        ],
    );
    assert!(!bad.ok);
    assert!(
        bad.stderr
            .contains("checkout:<checkout id> or store:<instance id>")
            || bad.stderr.contains("invalid"),
        "{}",
        bad.stderr
    );
    let unbound = run(
        &home,
        &[
            "unbind",
            "checkout:code",
            "--connection",
            "c1",
            "--channel",
            "ch",
        ],
    );
    assert!(unbound.ok, "{}{}", unbound.stdout, unbound.stderr);
    assert_eq!(unbound.json["change"], "absent");
}
