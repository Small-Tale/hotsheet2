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

// ---- HS2-322ZSZ: external claim-backend plugins through the real CLI ----------------

#[cfg(unix)]
const SHELL_PLUGIN: &str = r#"#!/bin/sh
input=$(cat)
op=$(printf '%s' "$input" | sed -n 's/.*"op":"\([a-z_]*\)".*/\1/p')
case "$op" in
  namespaces) echo '{"ok":true,"result":{"namespaces":["vms"]}}' ;;
  get) echo '{"ok":true,"result":{"claim":null}}' ;;
  acquire)
    id=$(printf '%s' "$input" | sed -n 's/.*"claim_id":"\([^"]*\)".*/\1/p')
    echo "{\"ok\":true,\"result\":{\"claim\":{\"claim_id\":\"$id\",\"fencing_token\":1,\"acquired_at_ms\":1000,\"expires_at_ms\":99999999999999,\"metadata\":{}}}}" ;;
  *) echo '{"ok":false,"error":{"code":"unsupported","message":"not in this fixture"}}' ;;
esac
"#;

#[cfg(unix)]
fn install_shell_plugin(home: &Path, secret: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = home.join("claim-backends").join("shellplug");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            r#"id = "shellplug"
display_name = "Shell fixture"
command = ["run.sh"]
timeout_seconds = 10

[capabilities]
guarantee = "exclusive_with_fencing"

[[settings]]
key = "style"
label = "Style"
type = "string"
{}"#,
            // A secret setting makes every resolve read the credential store, which the
            // tests deny; only the settings-validation test needs one.
            if secret {
                "\n[[settings]]\nkey = \"token\"\nlabel = \"Access token\"\ntype = \"secret\"\n"
            } else {
                ""
            }
        ),
    )
    .unwrap();
    let script = dir.join("run.sh");
    std::fs::write(&script, SHELL_PLUGIN).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

#[cfg(unix)]
#[test]
fn hs2_322zsz_an_external_plugin_is_approved_on_add_and_never_retrusted_when_changed() {
    let e = env();
    let dir = install_shell_plugin(&e.home, false);

    let listed = ok(&e.home, &["plugin", "list"]);
    assert_eq!(listed["plugins"][0]["id"], "shellplug");
    assert_eq!(listed["plugins"][0]["approval"]["state"], "unapproved");

    let add = [
        "backend",
        "add",
        "plug",
        "--kind",
        "external",
        "--setting",
        "plugin=shellplug",
    ];
    let first = ok(&e.home, &add);
    assert_eq!(first["change"], "created");
    assert_eq!(first["plugin"]["state"], "approved", "{first}");
    assert_eq!(ok(&e.home, &add)["change"], "unchanged");

    // Code changes after approval: a repeated add keeps it unapproved, and only `trust` re-approves.
    std::fs::write(dir.join("extra.txt"), "changed").unwrap();
    let again = ok(&e.home, &add);
    assert_eq!(again["plugin"]["state"], "changed", "{again}");
    let refused = run(&e.home, &["backend", "test", "plug"]);
    assert!(!refused.ok, "{}", refused.stdout);
    assert!(
        refused.json["error"]
            .as_str()
            .unwrap_or(&refused.stdout)
            .contains("resource plugin trust shellplug")
            || refused.stdout.contains("resource plugin trust shellplug"),
        "{}{}",
        refused.stdout,
        refused.stderr
    );
    let trusted = ok(&e.home, &["plugin", "trust", "shellplug"]);
    assert_eq!(trusted["change"], "updated");
    assert_eq!(trusted["approval"]["state"], "approved");
    assert_eq!(
        ok(&e.home, &["plugin", "trust", "shellplug"])["change"],
        "unchanged"
    );

    let missing = run(&e.home, &["plugin", "trust", "nope"]);
    assert!(!missing.ok);
    assert!(missing.stderr.contains("not_found"), "{}", missing.stderr);
}

#[cfg(unix)]
#[test]
fn hs2_322zsz_a_namespace_bound_to_an_external_plugin_acquires_through_it() {
    let e = env();
    install_shell_plugin(&e.home, false);
    ok(
        &e.home,
        &[
            "backend",
            "add",
            "plug",
            "--kind",
            "external",
            "--setting",
            "plugin=shellplug",
        ],
    );
    let tested = ok(&e.home, &["backend", "test", "plug"]);
    assert_eq!(tested["ok"], true, "{tested}");
    assert_eq!(tested["reachable"], true);
    ok(&e.home, &["namespace", "bind", "vms", "--backend", "plug"]);
    let acquired = ok(&e.home, &["acquire", "vms/one", "--duration", "60s"]);
    assert_eq!(acquired["acquired"], true, "{acquired}");
    assert_eq!(acquired["fencing_token"], 1);
    assert!(
        acquired["renewal_token"]
            .as_str()
            .unwrap()
            .starts_with("hsrc1.")
    );
    // The plugin implements only what the fixture says: the rest is its own error, not Hot Sheet's.
    let renew = run(
        &e.home,
        &[
            "renew",
            "--token",
            acquired["renewal_token"].as_str().unwrap(),
            "--duration",
            "60s",
        ],
    );
    assert!(!renew.ok);
    assert!(renew.stdout.contains("unsupported"), "{}", renew.stdout);
}

#[cfg(unix)]
#[test]
fn hs2_322zsz_plugin_settings_are_checked_and_ticket_claims_refuse_an_external_backend() {
    let e = env();
    install_shell_plugin(&e.home, true);
    for (setting, needle) in [
        (
            "token=pasted",
            "is a secret: never put it in a configuration",
        ),
        ("nope=1", "has no setting 'nope'"),
    ] {
        let refused = run(
            &e.home,
            &[
                "backend",
                "add",
                "plug",
                "--kind",
                "external",
                "--setting",
                "plugin=shellplug",
                "--setting",
                setting,
            ],
        );
        assert!(!refused.ok);
        assert!(
            refused.stderr.contains(needle),
            "{setting}: {}",
            refused.stderr
        );
        assert!(
            !refused.stderr.contains("pasted")
                || setting.starts_with("token") && !refused.stderr.contains("=pasted")
        );
    }
    let no_plugin = run(&e.home, &["backend", "add", "plug", "--kind", "external"]);
    assert!(!no_plugin.ok);
    assert!(
        no_plugin.stderr.contains("plugin=<id>"),
        "{}",
        no_plugin.stderr
    );
    let undeclared = run(
        &e.home,
        &[
            "backend",
            "add",
            "ghost",
            "--kind",
            "external",
            "--setting",
            "plugin=ghost",
        ],
    );
    assert!(
        undeclared
            .stderr
            .contains("no claim-backend plugin 'ghost'"),
        "{}",
        undeclared.stderr
    );

    ok(
        &e.home,
        &[
            "backend",
            "add",
            "plug",
            "--kind",
            "external",
            "--setting",
            "plugin=shellplug",
        ],
    );
    let secret = run(&e.home, &["backend", "secret", "plug", "style"]);
    assert!(!secret.ok);
    assert!(
        secret.stderr.contains("declares no secret setting 'style'"),
        "{}",
        secret.stderr
    );
    let ticket = run(
        &e.home,
        &["source", "bind", "some-connection", "--backend", "plug"],
    );
    assert!(!ticket.ok);
    assert!(ticket.stderr.contains("HS2-3SJ8JX"), "{}", ticket.stderr);
}
