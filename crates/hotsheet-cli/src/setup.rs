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
    let enabled = enabled_plugin_ids(project_dir);
    Ok(hotsheet_plugins::run_setup(
        store_path,
        project_dir,
        tool,
        detect,
        enabled.as_ref(),
    )?)
}

/// Migrate existing settings and refresh every detected or previously managed tool using
/// the same core writers as explicit setup and bootstrap.
pub fn refresh_setup(store_path: &Path, project_dir: &Path) -> Result<Vec<SetupReport>> {
    let settings = hotsheet_ticketing::Settings::with_legacy_stores(project_dir, [store_path]);
    settings.migrate_existing()?;
    let enabled = enabled_plugin_ids(project_dir);
    Ok(hotsheet_plugins::refresh_setup_in(
        store_path,
        project_dir,
        enabled.as_ref(),
        &hotsheet_plugins::default_dirs(),
    )?)
}

/// The project's `enabled_plugins` shared setting as a set of ids: `None` when unset (no
/// restriction), `Some(empty)` for an explicit empty list (no tool enabled). Resolution is
/// shared with the server's project-open refresh through
/// [`hotsheet_plugins::enabled_plugins_from_setting`] (HS2-94, HS2-8B3VJP).
fn enabled_plugin_ids(project: &Path) -> Option<HashSet<String>> {
    use hotsheet_ticketing::{Scope, Settings};
    let value = Settings::for_project(project)
        .get("enabled_plugins", Scope::Shared)
        .ok()
        .flatten();
    hotsheet_plugins::enabled_plugins_from_setting(value.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::{FsStore, StoreMetadata};

    /// A temp dir that is both the store and the project.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        std::fs::create_dir_all(dir.path().join(".git/info")).unwrap();
        std::fs::write(dir.path().join(".git/info/exclude"), "").unwrap();
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
        assert!(claude_md.contains("Write portable durable references"));
        assert!(claude_md.contains("`FEEDBACK NEEDED` is only"));
        // The default guidance must ticket direct-terminal work and carry testing, docs, and
        // commit-hygiene sections while leaving the push decision to each repo (HS2-3JMMAZ).
        assert!(claude_md.contains("Create tickets by default for real work"));
        assert!(claude_md.contains("Double coverage"));
        assert!(claude_md.contains("update them **in the same change as the code**"));
        assert!(claude_md.contains("commit per ticket"));
        assert!(claude_md.contains("Pushing is up to this repository"));

        let skill = read(d.path(), ".claude/skills/hotsheet/SKILL.md");
        assert!(skill.contains("name: hotsheet"));
        assert!(skill.contains("not a stopping condition"));
        assert!(skill.contains("Priority is an important guidance signal"));
        assert!(skill.contains("Completion checklist"));
        assert!(skill.contains("attachment:filename"));
        assert!(skill.contains("FEEDBACK NEEDED is not deferred-work tracking"));
        assert!(skill.contains("Never copy a developer-specific"));
        assert!(skill.contains("--note-file -"));
        assert!(skill.contains("Activity is timeline history"));
        assert!(skill.contains("CLI rejects likely escaped line breaks"));
        assert!(skill.contains("--allow-literal-backslash-n"));

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
        // A user's shared settings stay untouched; local hooks are merge-preserved.
        std::fs::create_dir_all(d.path().join(".claude")).unwrap();
        std::fs::write(
            d.path().join(".claude/settings.json"),
            r#"{"model":"opus"}"#,
        )
        .unwrap();
        std::fs::write(
            d.path().join(".claude/settings.local.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"my-own-hook"}]},{"matcher":"*","hooks":[{"type":"command","command":"old/hotsheet-cli permission-hook"}]}],"PermissionRequest":[{"matcher":"Bash","hooks":[{"type":"command","command":"my-own-request-hook"}]}]}}"#,
        )
        .unwrap();

        run_setup(d.path(), d.path(), Some("claude"), false).unwrap();
        run_setup(d.path(), d.path(), Some("claude"), false).unwrap(); // twice → no dupes

        assert_eq!(
            read(d.path(), ".claude/settings.json"),
            r#"{"model":"opus"}"#
        );
        let s: serde_json::Value =
            serde_json::from_str(&read(d.path(), ".claude/settings.local.json")).unwrap();
        let pre = s["hooks"]["PreToolUse"].as_array().unwrap();
        // The user's own hook survives; the headless fallback is registered once.
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
        let permission_requests = s["hooks"]["PermissionRequest"].as_array().unwrap();
        assert!(
            permission_requests
                .iter()
                .any(|e| e["hooks"][0]["command"] == "my-own-request-hook"),
            "user PermissionRequest hooks must survive setup"
        );
        let ours: Vec<_> = permission_requests
            .iter()
            .filter(|e| {
                e["hooks"][0]["command"]
                    .as_str()
                    .is_some_and(|c| c.ends_with("permission-hook"))
            })
            .collect();
        assert_eq!(ours.len(), 1, "one interactive permission hook");
        assert_eq!(ours[0]["matcher"], "*");
        assert_eq!(ours[0]["hooks"][0]["timeout"], 86_430);
        // Codex declares its own native PermissionRequest hook in its own config.
        let d2 = project();
        let reports = run_setup(d2.path(), d2.path(), Some("codex"), false).unwrap();
        assert!(reports[0].wrote.iter().any(|w| w == ".codex/hooks.json"));
        let codex: serde_json::Value =
            serde_json::from_str(&read(d2.path(), ".codex/hooks.json")).unwrap();
        let requests = codex["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["matcher"], ".*");
        assert!(
            requests[0]["hooks"][0]["command"]
                .as_str()
                .is_some_and(|command| command.ends_with("permission-hook"))
        );
        assert_eq!(requests[0]["hooks"][0]["timeout"], 86_430);
    }

    #[test]
    fn setup_codex_writes_agents_skill_and_toml_idempotently() {
        let d = project();
        std::fs::create_dir_all(d.path().join(".codex")).unwrap();
        std::fs::write(
            d.path().join(".codex/hooks.json"),
            r#"{"description":"user hooks","hooks":{"PermissionRequest":[{"matcher":"Bash","hooks":[{"type":"command","command":"my-policy"}]}]}}"#,
        )
        .unwrap();
        let custom_skill = d.path().join(".agents/skills/custom/SKILL.md");
        std::fs::create_dir_all(custom_skill.parent().unwrap()).unwrap();
        std::fs::write(&custom_skill, "user-authored custom skill\n").unwrap();
        let reports = run_setup(d.path(), d.path(), Some("codex"), false).unwrap();
        assert_eq!(reports[0].tool, "Codex CLI");

        let agents = read(d.path(), "AGENTS.md");
        assert!(agents.contains("<!-- BEGIN hotsheet:codex -->"));
        assert!(agents.contains("hotsheet-cli ls --up-next"));
        assert!(agents.contains("Create every follow-up immediately, without asking"));
        assert!(agents.contains("Write portable durable references"));
        assert!(agents.contains("`FEEDBACK NEEDED` is only"));
        // Same expanded default guidance reaches Codex's AGENTS.md block (HS2-3JMMAZ).
        assert!(agents.contains("Create tickets by default for real work"));
        assert!(agents.contains("Double coverage"));
        assert!(agents.contains("Pushing is up to this repository"));
        assert!(!d.path().join(".claude").exists());
        assert!(
            reports[0]
                .wrote
                .iter()
                .any(|w| w == ".agents/skills/hotsheet/SKILL.md")
        );

        let skill = read(d.path(), ".agents/skills/hotsheet/SKILL.md");
        assert_eq!(skill, include_str!("../../../plugins/codex/SKILL.md"));
        assert!(skill.contains("name: hotsheet"));
        assert!(skill.contains("not a stopping condition"));
        assert_eq!(
            read(d.path(), ".agents/skills/custom/SKILL.md"),
            "user-authored custom skill\n"
        );

        let cfg: toml::Table = toml::from_str(&read(d.path(), ".codex/config.toml")).unwrap();
        let hs = cfg["mcp_servers"]["hotsheet"].as_table().unwrap();
        assert_eq!(hs["command"].as_str().unwrap(), "hotsheet-mcp");
        assert!(
            read(d.path(), ".git/info/exclude")
                .lines()
                .any(|line| line == "/.codex/config.toml")
        );
        assert!(
            read(d.path(), ".git/info/exclude")
                .lines()
                .any(|line| line == "/.codex/hooks.json")
        );
        let hooks: serde_json::Value =
            serde_json::from_str(&read(d.path(), ".codex/hooks.json")).unwrap();
        assert_eq!(hooks["description"], "user hooks");
        let requests = hooks["hooks"]["PermissionRequest"].as_array().unwrap();
        assert!(
            requests
                .iter()
                .any(|entry| entry["hooks"][0]["command"] == "my-policy")
        );
        assert_eq!(
            requests
                .iter()
                .filter(|entry| {
                    entry["hooks"][0]["command"]
                        .as_str()
                        .is_some_and(|command| command.ends_with("permission-hook"))
                })
                .count(),
            1
        );

        run_setup(d.path(), d.path(), Some("codex"), false).unwrap();
        assert_eq!(read(d.path(), "AGENTS.md"), agents);
        assert_eq!(read(d.path(), ".agents/skills/hotsheet/SKILL.md"), skill);
        assert_eq!(
            read(d.path(), ".agents/skills/custom/SKILL.md"),
            "user-authored custom skill\n"
        );
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
        assert!(
            !read(d.path(), ".git/info/exclude")
                .lines()
                .any(|line| line == "/.mcp.json"),
            "a mixed user-owned config stays visible to git"
        );
    }

    #[test]
    fn enabled_plugins_setting_gates_detect() {
        use hotsheet_ticketing::{Scope, Settings};
        let d = project();
        assert!(enabled_plugin_ids(d.path()).is_none());
        Settings::for_project(d.path())
            .set(
                "enabled_plugins",
                serde_json::json!(["claude"]),
                Scope::Shared,
            )
            .unwrap();
        let set = enabled_plugin_ids(d.path()).unwrap();
        assert!(set.contains("claude") && !set.contains("codex"));
        // An explicit empty list is authoritative: no tool is enabled (HS2-8B3VJP).
        Settings::for_project(d.path())
            .set("enabled_plugins", serde_json::json!([]), Scope::Shared)
            .unwrap();
        assert_eq!(enabled_plugin_ids(d.path()), Some(HashSet::new()));
        // A malformed value never restricts (and so never removes) anything.
        Settings::for_project(d.path())
            .set(
                "enabled_plugins",
                serde_json::json!("claude"),
                Scope::Shared,
            )
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
