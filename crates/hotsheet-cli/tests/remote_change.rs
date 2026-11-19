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

// ---- HS2-BWJWWX: the Ably plugin through the real CLI -------------------------------

#[test]
fn hs2_bwjwwx_the_ably_plugin_is_listed_and_a_connection_is_added_bound_listed_and_removed() {
    let (_root, home) = home();
    let plugins = run(&home, &["plugins"]);
    assert!(plugins.ok, "{}{}", plugins.stdout, plugins.stderr);
    assert_eq!(plugins.json["plugins"][0]["id"], "ably");
    assert_eq!(plugins.json["plugins"][0]["credential"], true);
    assert_eq!(plugins.json["plugins"][0]["settings"][0]["key"], "endpoint");

    let add = [
        "connection",
        "add",
        "shared",
        "--plugin",
        "ably",
        "--setting",
        "endpoint=wss://eu.example.net",
    ];
    let first = run(&home, &add);
    assert!(first.ok, "{}{}", first.stdout, first.stderr);
    assert_eq!(first.json["change"], "created");
    assert_eq!(first.json["connection"]["credential"], "missing");
    assert!(
        first.json["next_steps"][0]
            .as_str()
            .unwrap()
            .contains("connection secret shared"),
        "{}",
        first.stdout
    );
    assert_eq!(run(&home, &add).json["change"], "unchanged");

    let bind = [
        "bind",
        "checkout:code",
        "--connection",
        "shared",
        "--channel",
        "repo-events",
        "--repository",
        "acme/app",
        "--ref",
        "refs/heads/main",
        "--ref",
        "refs/heads/release/*",
    ];
    assert_eq!(run(&home, &bind).json["change"], "created");
    assert_eq!(run(&home, &bind).json["change"], "unchanged");
    let second = run(
        &home,
        &[
            "bind",
            "store:tickets",
            "--connection",
            "shared",
            "--channel",
            "repo-events",
            "--repository-id",
            "42",
            "--repository",
            "acme/tickets",
        ],
    );
    assert_eq!(
        second.json["change"], "created",
        "{}{}",
        second.stdout, second.stderr
    );

    let listed = run(&home, &["list"]);
    assert_eq!(listed.json["bindings"].as_array().unwrap().len(), 2);
    assert_eq!(
        listed.json["connections"][0]["bindings"][1]["refs"][0], "refs/heads/*",
        "the default follows branches, not tags"
    );
    assert_eq!(
        listed.json["connections"][0]["bindings"][0]["refs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let refused = run(&home, &["connection", "remove", "shared"]);
    assert!(!refused.ok);
    assert!(
        refused.stderr.contains("conflict") && refused.stderr.contains("unbind it first"),
        "{}",
        refused.stderr
    );
    assert_eq!(
        run(
            &home,
            &[
                "unbind",
                "checkout:code",
                "--connection",
                "shared",
                "--channel",
                "repo-events"
            ]
        )
        .json["change"],
        "removed"
    );
    assert_eq!(
        run(
            &home,
            &[
                "unbind",
                "store:tickets",
                "--connection",
                "shared",
                "--channel",
                "repo-events"
            ]
        )
        .json["change"],
        "removed"
    );
    assert_eq!(
        run(&home, &["connection", "remove", "shared"]).json["change"],
        "removed"
    );
    assert_eq!(
        run(&home, &["connection", "remove", "shared"]).json["change"],
        "absent"
    );
}

#[test]
fn hs2_bwjwwx_settings_bindings_and_credentials_are_checked_and_no_secret_is_echoed() {
    let (_root, home) = home();
    for (args, needle) in [
        (
            vec![
                "connection",
                "add",
                "c1",
                "--plugin",
                "ably",
                "--setting",
                "api_key=appid.keyid:pasted-secret",
            ],
            "has no setting 'api_key'",
        ),
        (
            vec![
                "connection",
                "add",
                "c1",
                "--plugin",
                "ably",
                "--setting",
                "endpoint",
            ],
            "key=value",
        ),
        (
            vec!["connection", "add", "Bad Id", "--plugin", "ably"],
            "connection id",
        ),
    ] {
        let refused = run(&home, &args);
        assert!(!refused.ok, "{args:?}");
        assert!(
            refused.stderr.contains(needle),
            "{args:?}: {}",
            refused.stderr
        );
        assert!(
            !refused.stderr.contains("pasted-secret"),
            "never echoes a value"
        );
    }
    assert!(
        !home.join("remote-change").exists(),
        "refused adds write nothing"
    );
    run(&home, &["connection", "add", "c1", "--plugin", "ably"]);
    let no_connection = run(
        &home,
        &[
            "bind",
            "checkout:code",
            "--connection",
            "ghost",
            "--channel",
            "ch",
            "--repository",
            "acme/app",
        ],
    );
    assert!(
        no_connection.stderr.contains("no connection 'ghost'"),
        "{}",
        no_connection.stderr
    );
    let no_repository = run(
        &home,
        &[
            "bind",
            "checkout:code",
            "--connection",
            "c1",
            "--channel",
            "ch",
        ],
    );
    assert!(
        no_repository.stderr.contains("expects"),
        "{}",
        no_repository.stderr
    );
    let bad_ref = run(
        &home,
        &[
            "bind",
            "checkout:code",
            "--connection",
            "c1",
            "--channel",
            "ch",
            "--repository",
            "acme/app",
            "--ref",
            "main",
        ],
    );
    assert!(
        bad_ref.stderr.contains("start with refs/"),
        "{}",
        bad_ref.stderr
    );

    // The native credential store is denied here: storing and testing fail closed and say so.
    let secret = run(&home, &["connection", "secret", "c1"]);
    assert!(!secret.ok);
    assert!(
        secret.stderr.contains("OS credential store") || secret.stderr.contains("invalid"),
        "{}",
        secret.stderr
    );
    let tested = run(&home, &["connection", "test", "c1"]);
    assert!(!tested.ok);
    assert!(
        tested.stdout.contains("backend_unreachable")
            || tested.stdout.contains("missing_credential"),
        "{}",
        tested.stdout
    );
}
