//! `hotsheet-cli setup <tool>` — prepare a project directory for an AI tool, **headless**
//! (no server, no client; `docs/05-ai-tool-plugins.md` §5.1a). Writes the plugin's
//! one-shot artifacts from the core plugin loader (`hotsheet-plugins`, HS2-97):
//!   1. the managed instruction section (a delimited block, refreshed in place),
//!   2. the worklist skill,
//!   3. the MCP-config entry registering the serverless `hotsheet-mcp --path <store>`.
//!
//! Every write is **merge-safe + idempotent** — re-running refreshes the managed
//! pieces without clobbering the user's own content.

use std::path::Path;

use anyhow::{Context, Result, bail};
use hotsheet_plugins::{Plugin, all_plugins, default_dirs, find};

/// What one tool's setup wrote (project-relative paths), for reporting.
#[derive(Debug)]
pub struct SetupReport {
    pub tool: String,
    pub wrote: Vec<String>,
}

/// Set up one named `tool`, or every **detected** tool when `detect` is set. Writes
/// into `project_dir`; the MCP entry points at `store_path` (which must exist).
pub fn run_setup(
    store_path: &Path,
    project_dir: &Path,
    tool: Option<&str>,
    detect: bool,
) -> Result<Vec<SetupReport>> {
    let plugins: Vec<Plugin> = match (tool, detect) {
        (Some(id), _) => {
            vec![find(id).with_context(|| format!("unknown tool '{id}' (no such plugin)"))?]
        }
        (None, true) => {
            // Honor a project's `enabled_plugins` shared setting if present (else no
            // restriction) — HS2-94 settings driving HS2-92/HS2-98 setup.
            let enabled = enabled_plugin_ids(store_path);
            all_plugins(&default_dirs())
                .into_iter()
                .filter(is_detected)
                .filter(|p| enabled.as_ref().is_none_or(|set| set.contains(p.id())))
                .collect()
        }
        (None, false) => {
            bail!("specify a tool (e.g. `hotsheet-cli setup claude`) or pass --detect")
        }
    };
    if plugins.is_empty() {
        bail!("no supported AI tools detected on this machine");
    }

    // Absolute store path so the MCP `--path` works from anywhere the tool launches.
    let store_abs = store_path.canonicalize().with_context(|| {
        format!(
            "store path does not exist: {} (run `hotsheet-cli init` first)",
            store_path.display()
        )
    })?;

    let mut reports = Vec::new();
    for p in plugins {
        // Security: refuse a plugin whose write targets would escape the project.
        let bad = p.unsafe_targets();
        if !bad.is_empty() {
            bail!(
                "plugin '{}' declares unsafe target path(s): {} (targets must be project-relative)",
                p.id(),
                bad.join(", ")
            );
        }
        let mut wrote = vec![write_instructions(project_dir, &p)?];
        if let Some(skill) = write_skill(project_dir, &p)? {
            wrote.push(skill); // absent for tools with no skills concept (e.g. Codex)
        }
        wrote.push(write_mcp(project_dir, &store_abs, &p)?);
        reports.push(SetupReport {
            tool: p.manifest.product_name.clone(),
            wrote,
        });
    }
    Ok(reports)
}

/// The project's `enabled_plugins` shared setting as a set of ids, or `None` if unset
/// (no restriction). A non-array or empty value is treated as "no restriction".
fn enabled_plugin_ids(store: &Path) -> Option<std::collections::HashSet<String>> {
    use hotsheet_ticketing::{Scope, Settings};
    let value = Settings::new(store)
        .get("enabled_plugins", Scope::Shared)
        .ok()??;
    let set: std::collections::HashSet<String> = value
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    (!set.is_empty()).then_some(set)
}

fn is_detected(p: &Plugin) -> bool {
    p.manifest
        .detection
        .binaries
        .iter()
        .any(|b| binary_on_path(b))
}

/// Whether `name` is an executable file on `PATH`.
fn binary_on_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
}

/// Insert/refresh the managed instruction block in the tool's instruction file.
fn write_instructions(project: &Path, p: &Plugin) -> Result<String> {
    let rel = &p.manifest.instructions.target;
    let target = project.join(rel);
    let begin = format!("<!-- BEGIN hotsheet:{} -->", p.id());
    let end = format!("<!-- END hotsheet:{} -->", p.id());
    let block = format!("{begin}\n{}\n{end}", p.instructions_body().trim_end());
    let existing = std::fs::read_to_string(&target).unwrap_or_default();
    write_file(
        &target,
        &replace_or_append_block(&existing, &begin, &end, &block),
    )?;
    Ok(rel.clone())
}

/// Replace the region between the markers (inclusive) with `block`, or append `block`
/// if the markers aren't present. Preserves everything outside the block.
fn replace_or_append_block(existing: &str, begin: &str, end: &str, block: &str) -> String {
    if let (Some(bi), Some(ei)) = (existing.find(begin), existing.find(end)) {
        if ei >= bi {
            let end_full = ei + end.len();
            return format!("{}{block}{}", &existing[..bi], &existing[end_full..]);
        }
    }
    if existing.trim().is_empty() {
        format!("{block}\n")
    } else {
        format!("{}\n\n{block}\n", existing.trim_end())
    }
}

/// Write the worklist skill (a fully managed file), or nothing if the tool has no
/// skills concept (Codex). Returns the written path when present.
fn write_skill(project: &Path, p: &Plugin) -> Result<Option<String>> {
    match p.skill() {
        Some((target, body)) => {
            write_file(&project.join(target), body)?;
            Ok(Some(target.to_string()))
        }
        None => Ok(None),
    }
}

/// Register the `hotsheet-mcp` server in the tool's MCP config, merge-safe. The
/// writer is chosen by the manifest's `format` (a host helper keyed on the format,
/// not the tool id — docs/05 §5.3), so a new tool with a known format needs no code.
fn write_mcp(project: &Path, store_abs: &Path, p: &Plugin) -> Result<String> {
    let rel = &p.manifest.mcp.target;
    let target = project.join(rel);
    let name = &p.manifest.mcp.server_name;
    // Prefer the absolute `hotsheet-mcp` next to this CLI, so the MCP config works even
    // when the tool's PATH doesn't include our install dir (HS2-103). Falls back to the
    // manifest's bare command when there's no sibling (e.g. during tests).
    let command = crate::launch_safety::mcp_command(&p.manifest.mcp.command);
    let args = p.mcp_args(&store_abs.to_string_lossy());

    match p.manifest.mcp.format.as_str() {
        "claude-json" => write_mcp_json(&target, name, &command, &args)?,
        "codex-toml" => write_mcp_toml(&target, name, &command, &args)?,
        other => bail!(
            "unknown MCP config format '{other}' for plugin '{}'",
            p.id()
        ),
    }
    Ok(rel.clone())
}

/// Claude-style `.mcp.json`: `{ "mcpServers": { "<name>": { command, args } } }`.
fn write_mcp_json(target: &Path, name: &str, command: &str, args: &[String]) -> Result<()> {
    let mut root: serde_json::Value = std::fs::read_to_string(target)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));

    let servers = root
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    if !servers.is_object() {
        *servers = serde_json::json!({});
    }
    servers.as_object_mut().unwrap().insert(
        name.to_string(),
        serde_json::json!({ "command": command, "args": args }),
    );
    write_file(target, &(serde_json::to_string_pretty(&root)? + "\n"))
}

/// Codex-style TOML: `[mcp_servers.<name>]` with `command` + `args`, in the file
/// Codex reads at `$CODEX_HOME/config.toml`.
fn write_mcp_toml(target: &Path, name: &str, command: &str, args: &[String]) -> Result<()> {
    let mut root: toml::Table = std::fs::read_to_string(target)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();

    let servers = root
        .entry("mcp_servers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !servers.is_table() {
        *servers = toml::Value::Table(toml::Table::new());
    }

    let mut entry = toml::Table::new();
    entry.insert("command".into(), toml::Value::String(command.to_string()));
    entry.insert(
        "args".into(),
        toml::Value::Array(
            args.iter()
                .map(|a| toml::Value::String(a.clone()))
                .collect(),
        ),
    );
    servers
        .as_table_mut()
        .unwrap()
        .insert(name.to_string(), toml::Value::Table(entry));

    write_file(target, &toml::to_string_pretty(&root)?)
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
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

        // instruction block
        let claude_md = read(d.path(), "CLAUDE.md");
        assert!(claude_md.contains("<!-- BEGIN hotsheet:claude -->"));
        assert!(claude_md.contains("<!-- END hotsheet:claude -->"));
        assert!(claude_md.contains("hotsheet-cli ls --up-next"));

        // skill
        let skill = read(d.path(), ".claude/skills/hotsheet/SKILL.md");
        assert!(skill.contains("name: hotsheet"));

        // mcp config points at the serverless shim with this store's absolute path
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
        let store_abs = d.path().canonicalize().unwrap();
        assert_eq!(args[1], store_abs.to_string_lossy());
    }

    #[test]
    fn setup_codex_uses_agents_md_and_toml_and_no_skill() {
        let d = project();
        let reports = run_setup(d.path(), d.path(), Some("codex"), false).unwrap();
        assert_eq!(reports[0].tool, "Codex CLI");

        // AGENTS.md, not CLAUDE.md; managed block present.
        let agents = read(d.path(), "AGENTS.md");
        assert!(agents.contains("<!-- BEGIN hotsheet:codex -->"));
        assert!(agents.contains("hotsheet-cli ls --up-next"));

        // No skill file written (Codex has no skills concept).
        assert!(!d.path().join(".claude").exists());
        assert!(reports[0].wrote.iter().all(|w| !w.contains("SKILL")));

        // MCP config is TOML at .codex/config.toml with [mcp_servers.hotsheet].
        let cfg: toml::Table = toml::from_str(&read(d.path(), ".codex/config.toml")).unwrap();
        let hs = cfg["mcp_servers"]["hotsheet"].as_table().unwrap();
        assert_eq!(hs["command"].as_str().unwrap(), "hotsheet-mcp");
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
    fn setup_codex_toml_is_idempotent_and_preserves_other_servers() {
        let d = project();
        // A pre-existing Codex config with an unrelated server + a top-level key.
        std::fs::create_dir_all(d.path().join(".codex")).unwrap();
        std::fs::write(
            d.path().join(".codex/config.toml"),
            "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
        )
        .unwrap();

        run_setup(d.path(), d.path(), Some("codex"), false).unwrap();
        run_setup(d.path(), d.path(), Some("codex"), false).unwrap(); // twice

        let cfg: toml::Table = toml::from_str(&read(d.path(), ".codex/config.toml")).unwrap();
        assert_eq!(cfg["model"].as_str().unwrap(), "o3", "top-level key kept");
        assert_eq!(
            cfg["mcp_servers"]["other"]["command"].as_str().unwrap(),
            "x",
            "other server kept"
        );
        assert_eq!(
            cfg["mcp_servers"]["hotsheet"]["command"].as_str().unwrap(),
            "hotsheet-mcp"
        );
    }

    #[test]
    fn setup_is_idempotent_and_preserves_user_content() {
        let d = project();

        // Pre-existing CLAUDE.md with the user's own content.
        std::fs::write(
            d.path().join("CLAUDE.md"),
            "# My project\n\nHand-written notes.\n",
        )
        .unwrap();
        // Pre-existing .mcp.json with an unrelated server.
        std::fs::write(
            d.path().join(".mcp.json"),
            r#"{"mcpServers":{"other":{"command":"x"}}}"#,
        )
        .unwrap();

        run_setup(d.path(), d.path(), Some("claude"), false).unwrap();
        run_setup(d.path(), d.path(), Some("claude"), false).unwrap(); // twice

        let claude_md = read(d.path(), "CLAUDE.md");
        assert!(claude_md.contains("# My project"), "user content kept");
        assert!(claude_md.contains("Hand-written notes."));
        assert_eq!(
            claude_md.matches("<!-- BEGIN hotsheet:claude -->").count(),
            1,
            "exactly one managed block after re-running"
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
        // Unset → no restriction.
        assert!(enabled_plugin_ids(d.path()).is_none());
        // A non-empty list → that set.
        Settings::new(d.path())
            .set(
                "enabled_plugins",
                serde_json::json!(["claude"]),
                Scope::Shared,
            )
            .unwrap();
        let set = enabled_plugin_ids(d.path()).unwrap();
        assert!(set.contains("claude") && !set.contains("codex"));
        // An empty list → no restriction (treated as unset).
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
        assert!(err.to_string().contains("--detect"));
    }
}
