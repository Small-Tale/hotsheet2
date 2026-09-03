//! `hotsheet-cli setup <tool>` — a thin wrapper over the **core** setup impl
//! (`hotsheet_plugins::run_setup`, HS2-91). The one-shot writers (managed instruction
//! section, worklist skill, MCP-config entry) live in the core plugins crate so the CLI
//! (headless) and the server (`POST /setup/<tool>`) share one implementation. The CLI's
//! only extra job is to supply the project's **enabled-plugin** set (a `Settings` value the
//! server also reads) so `--detect` honors it.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

pub use hotsheet_plugins::SetupReport;

/// Set up one named `tool`, or every detected (and enabled) tool when `detect` is set.
pub fn run_setup(
    store_path: &Path,
    project_dir: &Path,
    tool: Option<&str>,
    detect: bool,
) -> Result<Vec<SetupReport>> {
    let enabled = enabled_plugin_ids(store_path);
    Ok(hotsheet_plugins::run_setup(
        store_path,
        project_dir,
        tool,
        detect,
        enabled.as_ref(),
    )?)
}

/// The project's `enabled_plugins` shared setting as a set of ids, or `None` if unset (no
/// restriction). A non-array or empty value is treated as "no restriction". (HS2-94 settings
/// driving HS2-92/HS2-98 setup.)
fn enabled_plugin_ids(store: &Path) -> Option<HashSet<String>> {
    use hotsheet_ticketing::{Scope, Settings};
    let value = Settings::new(store)
        .get("enabled_plugins", Scope::Shared)
        .ok()??;
    let set: HashSet<String> = value
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    (!set.is_empty()).then_some(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::{FsStore, StoreMetadata};

    /// A temp dir that is both the store and the project.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        dir
    }

    fn read(dir: &Path, rel: &str) -> String {
        std::fs::read_to_string(dir.join(rel)).unwrap()
    }

    #[test]
    fn setup_claude_writes_all_three_artifacts() {
        let d = project();
        let reports = run_setup(d.path(), d.path(), Some("claude"), false).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].tool, "Claude Code");

        let claude_md = read(d.path(), "CLAUDE.md");
        assert!(claude_md.contains("<!-- BEGIN hotsheet:claude -->"));
        assert!(claude_md.contains("<!-- END hotsheet:claude -->"));
        assert!(claude_md.contains("hotsheet-cli ls --up-next"));
        assert!(claude_md.contains("Create every follow-up immediately, without asking"));
        assert!(claude_md.contains("`FEEDBACK NEEDED` is only"));

        let skill = read(d.path(), ".claude/skills/hotsheet/SKILL.md");
        assert!(skill.contains("name: hotsheet"));
        assert!(skill.contains("not a stopping condition"));
        assert!(skill.contains("Priority is an important guidance signal"));
        assert!(skill.contains("Completion checklist"));
        assert!(skill.contains("FEEDBACK NEEDED is not deferred-work tracking"));
        assert!(skill.contains("--note-file -"));

        let mcp: serde_json::Value = serde_json::from_str(&read(d.path(), ".mcp.json")).unwrap();
        let hs = &mcp["mcpServers"]["hotsheet"];
        assert_eq!(hs["command"], "hotsheet-mcp");
        let args: Vec<&str> = hs["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        assert_eq!(args[0], "--path");
        assert_eq!(args[1], d.path().canonicalize().unwrap().to_string_lossy());
    }

    #[test]
    fn setup_claude_registers_the_permission_hook_idempotently() {
        let d = project();
        // A user's own PreToolUse hook + settings should be preserved.
        std::fs::create_dir_all(d.path().join(".claude")).unwrap();
        std::fs::write(
            d.path().join(".claude/settings.json"),
            r#"{"model":"opus","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"my-own-hook"}]}]}}"#,
        )
        .unwrap();

        run_setup(d.path(), d.path(), Some("claude"), false).unwrap();
        run_setup(d.path(), d.path(), Some("claude"), false).unwrap(); // twice → no dupes

        let s: serde_json::Value =
            serde_json::from_str(&read(d.path(), ".claude/settings.json")).unwrap();
        assert_eq!(s["model"], "opus", "user settings kept");
        let pre = s["hooks"]["PreToolUse"].as_array().unwrap();
        // The user's own hook survives; exactly one Hot Sheet hook is registered.
        assert!(
            pre.iter()
                .any(|e| e["hooks"][0]["command"] == "my-own-hook"),
            "user's hook kept"
        );
        let ours: Vec<_> = pre
            .iter()
            .filter(|e| {
                e["hooks"][0]["command"]
                    .as_str()
                    .is_some_and(|c| c.ends_with("permission-hook"))
            })
            .collect();
        assert_eq!(ours.len(), 1, "exactly one Hot Sheet hook, no duplicates");
        assert_eq!(ours[0]["matcher"], "*");
        // Codex declares no hook → its setup writes none.
        let d2 = project();
        let reports = run_setup(d2.path(), d2.path(), Some("codex"), false).unwrap();
        assert!(
            reports[0]
                .wrote
                .iter()
                .all(|w| w != ".claude/settings.json")
        );
    }

    #[test]
    fn setup_codex_uses_agents_md_and_toml_and_no_skill() {
        let d = project();
        let reports = run_setup(d.path(), d.path(), Some("codex"), false).unwrap();
        assert_eq!(reports[0].tool, "Codex CLI");

        let agents = read(d.path(), "AGENTS.md");
        assert!(agents.contains("<!-- BEGIN hotsheet:codex -->"));
        assert!(agents.contains("hotsheet-cli ls --up-next"));
        assert!(agents.contains("Create every follow-up immediately, without asking"));
        assert!(agents.contains("`FEEDBACK NEEDED` is only"));
        assert!(!d.path().join(".claude").exists());
        assert!(reports[0].wrote.iter().all(|w| !w.contains("SKILL")));

        let cfg: toml::Table = toml::from_str(&read(d.path(), ".codex/config.toml")).unwrap();
        let hs = cfg["mcp_servers"]["hotsheet"].as_table().unwrap();
        assert_eq!(hs["command"].as_str().unwrap(), "hotsheet-mcp");
    }

    #[test]
    fn setup_is_idempotent_and_preserves_user_content() {
        let d = project();
        std::fs::write(
            d.path().join("CLAUDE.md"),
            "# My project\n\nHand-written notes.\n",
        )
        .unwrap();
        std::fs::write(
            d.path().join(".mcp.json"),
            r#"{"mcpServers":{"other":{"command":"x"}}}"#,
        )
        .unwrap();

        run_setup(d.path(), d.path(), Some("claude"), false).unwrap();
        run_setup(d.path(), d.path(), Some("claude"), false).unwrap(); // twice

        let claude_md = read(d.path(), "CLAUDE.md");
        assert!(claude_md.contains("# My project"), "user content kept");
        assert_eq!(
            claude_md.matches("<!-- BEGIN hotsheet:claude -->").count(),
            1
        );

        let mcp: serde_json::Value = serde_json::from_str(&read(d.path(), ".mcp.json")).unwrap();
        assert_eq!(
            mcp["mcpServers"]["other"]["command"], "x",
            "other server kept"
        );
        assert_eq!(mcp["mcpServers"]["hotsheet"]["command"], "hotsheet-mcp");
    }

    #[test]
    fn enabled_plugins_setting_gates_detect() {
        use hotsheet_ticketing::{Scope, Settings};
        let d = project();
        assert!(enabled_plugin_ids(d.path()).is_none());
        Settings::new(d.path())
            .set(
                "enabled_plugins",
                serde_json::json!(["claude"]),
                Scope::Shared,
            )
            .unwrap();
        let set = enabled_plugin_ids(d.path()).unwrap();
        assert!(set.contains("claude") && !set.contains("codex"));
        Settings::new(d.path())
            .set("enabled_plugins", serde_json::json!([]), Scope::Shared)
            .unwrap();
        assert!(enabled_plugin_ids(d.path()).is_none());
    }

    #[test]
    fn unknown_tool_errors() {
        let d = project();
        let err = run_setup(d.path(), d.path(), Some("nope"), false).unwrap_err();
        assert!(err.to_string().contains("unknown tool 'nope'"));
    }

    #[test]
    fn no_tool_and_no_detect_errors() {
        let d = project();
        let err = run_setup(d.path(), d.path(), None, false).unwrap_err();
        assert!(err.to_string().contains("detect"));
    }
}
