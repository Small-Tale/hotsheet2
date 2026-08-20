//! `hotsheet-cli plugin` — manage external AI-tool plugins (HS2-92). Install copies a
//! plugin directory into the machine plugin dir so the loader picks it up; remove
//! deletes it. Listing lives in the binary (it just renders `all_plugins`).
//!
//! There is **no trust gate yet** — install validates the manifest and reports what a
//! plugin declares, but does not sandbox or verify it. That is HS2-93.

use std::path::Path;

use anyhow::{Context, Result, bail};
use hotsheet_plugins::{Plugin, builtin_plugins};

/// Install the plugin directory `src` into `dest_root/<id>` (copying its top-level
/// files — the manifest + templates). Returns the plugin id. Refuses an id that
/// collides with a bundled first-party plugin (which would win anyway).
pub fn install(src: &Path, dest_root: &Path) -> Result<String> {
    let plugin = Plugin::from_fs_dir(src)
        .with_context(|| format!("not a valid plugin directory: {}", src.display()))?;
    let id = plugin.manifest.id.clone();
    if builtin_plugins().iter().any(|p| p.id() == id) {
        bail!(
            "'{id}' is a built-in first-party plugin; a same-id external plugin would be ignored"
        );
    }

    let dest = dest_root.join(&id);
    std::fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;
    for entry in std::fs::read_dir(src)? {
        let path = entry?.path();
        if path.is_file() {
            if let Some(name) = path.file_name() {
                std::fs::copy(&path, dest.join(name))
                    .with_context(|| format!("copying {}", path.display()))?;
            }
        }
    }
    // Re-validate from the installed location so a bad copy fails loudly.
    Plugin::from_fs_dir(&dest).context("installed plugin failed to re-load")?;
    Ok(id)
}

/// Remove an installed external plugin by id from `dest_root`. Returns whether it was
/// present.
pub fn remove(id: &str, dest_root: &Path) -> Result<bool> {
    let dir = dest_root.join(id);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_plugin(parent: &Path, id: &str) {
        let dir = parent.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                r#"id = "{id}"
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
args = ["--path", "{{store}}"]
"#
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("instructions.md"),
            "## Hot Sheet\nuse the tools.\n",
        )
        .unwrap();
    }

    #[test]
    fn install_then_remove_roundtrips() {
        let srcroot = tempfile::tempdir().unwrap();
        let destroot = tempfile::tempdir().unwrap();
        write_plugin(srcroot.path(), "acme");

        let id = install(&srcroot.path().join("acme"), destroot.path()).unwrap();
        assert_eq!(id, "acme");
        assert!(destroot.path().join("acme/manifest.toml").is_file());
        // The loader now finds it under the dest root.
        let found = hotsheet_plugins::find_in("acme", &[destroot.path().to_path_buf()]).unwrap();
        assert_eq!(found.manifest.product_name, "Acme Agent");

        assert!(remove("acme", destroot.path()).unwrap());
        assert!(!destroot.path().join("acme").exists());
        assert!(
            !remove("acme", destroot.path()).unwrap(),
            "second remove is a no-op"
        );
    }

    #[test]
    fn install_rejects_a_builtin_id() {
        let srcroot = tempfile::tempdir().unwrap();
        let destroot = tempfile::tempdir().unwrap();
        write_plugin(srcroot.path(), "claude"); // collides with the built-in
        let err = install(&srcroot.path().join("claude"), destroot.path()).unwrap_err();
        assert!(err.to_string().contains("built-in first-party plugin"));
    }

    #[test]
    fn install_rejects_a_non_plugin_dir() {
        let srcroot = tempfile::tempdir().unwrap();
        let destroot = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(srcroot.path().join("empty")).unwrap();
        assert!(install(&srcroot.path().join("empty"), destroot.path()).is_err());
    }
}
