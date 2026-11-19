//! HS2-JJWESD headless E2E: `hotsheet-cli resource backend|namespace|source` through the
//! real binary with an injected `HOTSHEET_HOME`. The native credential store is denied
//! (`HOTSHEET_NATIVE_SECRETS=deny`), so nothing here can reach the keychain. Synthetic
//! names only.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

struct Env {
    root: tempfile::TempDir,
    home: PathBuf,
}

fn env() -> Env {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    Env { root, home }
}

struct Run {
    ok: bool,
    json: Value,
    stdout: String,
    stderr: String,
}

fn run(home: &Path, args: &[&str]) -> Run {
    let output = Command::new(workspace_bin::path("hotsheet-cli"))
        .arg("resource")
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

fn ok(home: &Path, args: &[&str]) -> Value {
    let run = run(home, args);
    assert!(run.ok, "{args:?}: {}{}", run.stdout, run.stderr);
    run.json
}

#[test]
fn hs2_jjwesd_configuration_lifecycle_is_idempotent_and_secret_free() {
    let e = env();
    let add = [
        "backend",
        "add",
        "shared",
        "--kind",
        "git-ref",
        "--setting",
        "remote=origin",
        "--credential",
        "--offline",
        "provisional",
    ];
    let first = ok(&e.home, &add);
    assert_eq!(first["change"], "created");
    assert_eq!(first["configuration"]["offline"], "provisional");
    assert_eq!(
        first["configuration"]["credential"],
        "resource-claims.shared"
    );
    assert!(
        first["next_steps"][0]
            .as_str()
            .unwrap()
            .contains("--generate")
    );
    assert_eq!(ok(&e.home, &add)["change"], "unchanged");

    // A secret pasted as a setting is refused before anything is written.
    let refused = run(
        &e.home,
        &[
            "backend",
            "add",
            "other",
            "--kind",
            "git-ref",
            "--setting",
            "token=synthetic-secret",
        ],
    );
    assert!(!refused.ok);
    assert!(
        refused.stderr.contains("Secrets never go in settings"),
        "{}",
        refused.stderr
    );

    ok(
        &e.home,
        &["source", "bind", "conn-a", "--backend", "shared"],
    );
    let like = ok(&e.home, &["source", "bind", "conn-b", "--like", "conn-a"]);
    assert_eq!(like["configuration"], "shared");
    let shown = ok(&e.home, &["backend", "show", "shared"]);
    assert_eq!(
        shown["bindings"],
        serde_json::json!(["source conn-a", "source conn-b"])
    );

    let refused = run(&e.home, &["backend", "remove", "shared"]);
    assert!(!refused.ok);
    assert!(refused.stderr.contains("in_use"), "{}", refused.stderr);

    assert_eq!(
        ok(&e.home, &["source", "unbind", "conn-a"])["change"],
        "removed"
    );
    assert_eq!(
        ok(&e.home, &["source", "unbind", "conn-a"])["change"],
        "absent"
    );
    let shown = ok(&e.home, &["backend", "show", "shared"]);
    assert_eq!(shown["bindings"], serde_json::json!(["source conn-b"]));

    // The key lives only in the credential store, and a denied store fails cleanly.
    let key = run(&e.home, &["backend", "key", "shared", "--generate"]);
    assert!(!key.ok);
    assert!(key.stderr.contains("credential store"), "{}", key.stderr);

    let text = std::fs::read_to_string(e.home.join("resource-claims/configurations.json")).unwrap();
    assert!(!text.contains("synthetic-secret"));
    ok(&e.home, &["source", "unbind", "conn-b"]);
    assert_eq!(
        ok(&e.home, &["backend", "remove", "shared"])["change"],
        "removed"
    );
    assert_eq!(
        ok(&e.home, &["backend", "remove", "shared"])["change"],
        "absent"
    );
}

#[test]
fn hs2_jjwesd_test_makes_a_real_call_and_names_missing_steps() {
    let e = env();
    ok(&e.home, &["backend", "add", "box", "--kind", "local"]);
    let local = ok(&e.home, &["backend", "test", "box"]);
    assert_eq!(local["reachable"], true);

    let missing = e.root.path().join("no-such-repo");
    let repository = format!("repository={}", missing.display());
    ok(
        &e.home,
        &[
            "backend",
            "add",
            "shared",
            "--kind",
            "git-ref",
            "--setting",
            &repository,
        ],
    );
    let failed = run(&e.home, &["backend", "test", "shared"]);
    assert!(!failed.ok);
    assert_eq!(failed.json["reachable"], false);
    assert!(
        failed.json["error"]
            .as_str()
            .unwrap()
            .starts_with("backend_unreachable"),
        "{}",
        failed.json
    );
}

#[test]
fn hs2_jjwesd_namespace_scopes_stay_isolated_through_the_cli() {
    let e = env();
    let project = e.root.path().join("project");
    let project_arg = project.to_str().unwrap();
    ok(
        &e.home,
        &[
            "backend",
            "add",
            "box",
            "--kind",
            "local",
            "--project",
            project_arg,
        ],
    );
    ok(
        &e.home,
        &[
            "namespace",
            "bind",
            "vm",
            "--backend",
            "box",
            "--project",
            project_arg,
        ],
    );
    assert!(project.join(".hotsheet2/resource-claims.json").is_file());

    let machine = ok(&e.home, &["acquire", "vm/runner", "--duration", "1m"]);
    assert_eq!(machine["acquired"], true);
    let scoped = ok(
        &e.home,
        &[
            "acquire",
            "vm/runner",
            "--duration",
            "1m",
            "--project",
            project_arg,
        ],
    );
    assert_eq!(
        scoped["acquired"], true,
        "the project's vm namespace is separate"
    );
    let listed = ok(&e.home, &["list", "vm", "--project", project_arg]);
    assert_eq!(listed["configuration"], "box");
    assert_eq!(listed["claims"].as_array().unwrap().len(), 1);
    let listed = ok(&e.home, &["list", "vm"]);
    assert_eq!(listed["configuration"], Value::Null);

    let all = ok(&e.home, &["backend", "list", "--project", project_arg]);
    assert_eq!(all["scopes"][1]["namespaces"][0]["namespace"], "vm");
    assert_eq!(
        ok(
            &e.home,
            &["namespace", "unbind", "vm", "--project", project_arg]
        )["change"],
        "removed"
    );
}

#[test]
fn hs2_jjwesd_a_store_binding_names_the_store_identity_and_is_committed() {
    let e = env();
    let store = e.root.path().join("tickets.hs2");
    std::fs::create_dir_all(&store).unwrap();
    let init = Command::new(workspace_bin::path("hotsheet-cli"))
        .arg("-C")
        .arg(&store)
        .args(["init", "--prefix", "ST"])
        .env("HOTSHEET_HOME", &e.home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
        .env_remove("HOTSHEET_STORE")
        .current_dir(&store)
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let store_arg = store.to_str().unwrap();
    ok(
        &e.home,
        &[
            "backend",
            "add",
            "shared",
            "--kind",
            "git-ref",
            "--project",
            store_arg,
        ],
    );
    let bound = ok(
        &e.home,
        &[
            "source",
            "bind",
            "--store",
            store_arg,
            "--backend",
            "shared",
            "--project",
            store_arg,
        ],
    );
    assert!(
        bound["source"].as_str().unwrap().starts_with("store:"),
        "{bound}"
    );
    let status = hotsheet_ticketing::git::command()
        .args(["-C", store_arg, "status", "--porcelain", "--", ".hotsheet2"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(
        String::from_utf8_lossy(&status.stdout).trim(),
        "",
        "committed with the store"
    );
}
