//! HS2-80JHZD: `hotsheet-cli plugin remove <id>` validates the id before touching the
//! filesystem, so a traversal argument fails loudly and deletes nothing outside the
//! machine plugin root.

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
