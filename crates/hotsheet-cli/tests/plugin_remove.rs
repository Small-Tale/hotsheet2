//! HS2-80JHZD: `hotsheet-cli plugin remove <id>` validates the id before touching the
//! filesystem, so a traversal argument fails loudly and deletes nothing outside the
//! machine plugin root. HS2-YJCPD5: `plugin list` warns about a plugin dir discovery
//! skipped. HS2-TVRJPB: that includes a plugin whose id a built-in already claims.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::Path;
use std::process::{Command, Output};

fn plugin_remove(home: &Path, id: &str) -> Output {
    Command::new(workspace_bin::path("hotsheet-cli"))
        .env("HOTSHEET_HOME", home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .args(["plugin", "remove", id])
        .output()
        .unwrap()
}

#[test]
fn hs2_80jhzd_plugin_remove_refuses_a_traversal_id_without_deleting() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let plugins = home.join("plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    // Siblings a traversal id would reach from the plugin root.
    let victim = home.join("victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("keep.txt"), "keep").unwrap();
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();

    for id in ["../victim", "../../outside", "..", "a/b"] {
        let out = plugin_remove(&home, id);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{id:?} must fail");
        assert!(stderr.contains("invalid plugin id"), "{id:?}: {stderr}");
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("Removed plugin"),
            "{id:?}"
        );
    }
    assert!(victim.join("keep.txt").is_file());
    assert!(outside.is_dir());
    assert!(plugins.is_dir());

    // A valid id still works: absent is a no-op, present is removed.
    let out = plugin_remove(&home, "acme");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("No installed plugin 'acme'"));
    std::fs::create_dir_all(plugins.join("acme")).unwrap();
    let out = plugin_remove(&home, "acme");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Removed plugin 'acme'"));
    assert!(!plugins.join("acme").exists());
}

/// HS2-YJCPD5: `plugin list` names a machine plugin dir that discovery skipped (here an
/// id refused by HS2-80JHZD) on stderr, keeping the stdout table unchanged.
#[test]
fn hs2_yjcpd5_plugin_list_warns_about_a_skipped_plugin_on_stderr() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bad = home.join("plugins").join("acme-upper");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(
        bad.join("manifest.toml"),
        r#"id = "Acme_Upper"
display_name = "Acme"
product_name = "Acme Agent"
tier = "cli-agent"
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
    )
    .unwrap();
    std::fs::write(bad.join("instructions.md"), "## Hot Sheet\n").unwrap();

    let out = Command::new(workspace_bin::path("hotsheet-cli"))
        .env("HOTSHEET_HOME", &home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
        .args(["plugin", "list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stdout.lines().any(|line| line.starts_with("claude ")),
        "{stdout}"
    );
    assert!(!stdout.contains("Acme"), "{stdout}");
    assert!(!stdout.contains("warning"), "{stdout}");
    let warning = stderr
        .lines()
        .find(|line| line.starts_with("warning: skipped plugin "))
        .unwrap_or_else(|| panic!("no skipped-plugin warning: {stderr}"));
    assert!(warning.contains(&bad.display().to_string()), "{warning}");
    assert!(
        warning.contains("invalid plugin id \"Acme_Upper\""),
        "{warning}"
    );
}

#[test]
fn hs2_tvrjpb_plugin_list_warns_about_a_plugin_reusing_a_built_in_id() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let shadow = home.join("plugins").join("shadow-codex");
    std::fs::create_dir_all(&shadow).unwrap();
    std::fs::write(
        shadow.join("manifest.toml"),
        r#"id = "codex"
display_name = "Shadow"
product_name = "Shadow Agent"
tier = "cli-agent"
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
    )
    .unwrap();
    std::fs::write(shadow.join("instructions.md"), "## Hot Sheet\n").unwrap();

    let out = Command::new(workspace_bin::path("hotsheet-cli"))
        .env("HOTSHEET_HOME", &home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
        .args(["plugin", "list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    // The built-in keeps the id; the shadowing plugin is not listed.
    let codex: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("codex "))
        .collect();
    assert_eq!(codex.len(), 1, "{stdout}");
    assert!(!stdout.contains("Shadow"), "{stdout}");
    let warning = stderr
        .lines()
        .find(|line| line.starts_with("warning: skipped plugin "))
        .unwrap_or_else(|| panic!("no skipped-plugin warning: {stderr}"));
    assert_eq!(
        warning,
        format!(
            "warning: skipped plugin {}: duplicate plugin id \"codex\": the built-in plugin wins",
            shadow.display()
        )
    );
}
