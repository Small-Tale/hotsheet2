//! `hotsheet-cli plugin` — manage external AI-tool plugins (HS2-92). Install copies a
//! plugin directory into the machine plugin dir so the loader picks it up; remove
//! deletes it. Listing lives in the binary (it just renders `all_plugins`).
//!
//! **Trust gate (HS2-93):** `verify` checks a plugin structurally (known MCP format,
//! **write targets stay inside the project** — no `..`/absolute escapes), `describe`
//! discloses what it writes + launches, and `install` runs `verify` + shows the
//! disclosure + requires confirmation before copying. What is **not** built yet: the
//! behavioral subprocess/WASM sandbox (there are no code-bearing plugins yet) and the
//! `hs-fake-agent` conformance run (HS2-64).

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
    let issues = verify(&plugin);
    if !issues.is_empty() {
        bail!(
            "plugin '{id}' failed verification:\n  - {}",
            issues.join("\n  - ")
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

/// Structural verification of a plugin (loads, known MCP format, safe write targets).
/// Returns the list of issues; empty = passes. Behavioral conformance against the
/// `hs-fake-agent` suite is HS2-64.
pub fn verify(plugin: &Plugin) -> Vec<String> {
    let mut issues = Vec::new();
    let fmt = &plugin.manifest.mcp.format;
    if !hotsheet_plugins::KNOWN_MCP_FORMATS.contains(&fmt.as_str()) {
        issues.push(format!(
            "unknown MCP config format '{fmt}' (known: {})",
            hotsheet_plugins::KNOWN_MCP_FORMATS.join(", ")
        ));
    }
    for t in plugin.unsafe_targets() {
        issues.push(format!(
            "write target '{t}' escapes the project (must be project-relative)"
        ));
    }
    issues
}

/// A human-readable disclosure of what a plugin declares it will **write** and
/// **launch** — shown before installing (the trust gate) and by `plugin info`.
pub fn describe(plugin: &Plugin) -> String {
    use std::fmt::Write;
    let m = &plugin.manifest;
    let provenance = if plugin.is_builtin() {
        "first-party (built-in)"
    } else {
        "third-party (unsigned)"
    };
    let detects = if m.detection.binaries.is_empty() {
        "(nothing)".to_string()
    } else {
        m.detection.binaries.join(", ")
    };
    let mut s = String::new();
    let _ = writeln!(s, "  id:         {}", m.id);
    let _ = writeln!(s, "  product:    {} ({})", m.product_name, m.tier);
    let _ = writeln!(s, "  provenance: {provenance}");
    let _ = writeln!(s, "  detects:    {detects}");
    let _ = writeln!(s, "  writes:     {}", plugin.target_paths().join(", "));
    let _ = write!(
        s,
        "  launches:   {} {}",
        m.mcp.command,
        m.mcp.args.join(" ")
    );
    s
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

    #[test]
    fn verify_flags_unknown_format_and_unsafe_target_and_install_refuses() {
        let srcroot = tempfile::tempdir().unwrap();
        let destroot = tempfile::tempdir().unwrap();
        let dir = srcroot.path().join("bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            r#"id = "bad"
display_name = "Bad"
product_name = "Bad"
tier = "cli-agent"
[instructions]
target = "../escape.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "mystery-format"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
        )
        .unwrap();
        std::fs::write(dir.join("instructions.md"), "## Hot Sheet\n").unwrap();

        let p = hotsheet_plugins::Plugin::from_fs_dir(&dir).unwrap();
        let issues = verify(&p);
        assert_eq!(issues.len(), 2, "unknown format + escaping target");
        assert!(issues.iter().any(|i| i.contains("mystery-format")));
        assert!(issues.iter().any(|i| i.contains("escapes the project")));

        // A valid built-in verifies clean; describe discloses its writes/launch.
        let claude = hotsheet_plugins::find_in("claude", &[]).unwrap();
        assert!(verify(&claude).is_empty());
        assert!(describe(&claude).contains("first-party"));
        assert!(describe(&claude).contains("hotsheet-mcp"));

        // install refuses the bad plugin.
        assert!(install(&dir, destroot.path()).is_err());
        assert!(!destroot.path().join("bad").exists());
    }
}
