//! `hotsheet setup <tool>` — prepare a project directory for an AI tool, **headless**
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
use hotsheet_plugins::{Plugin, builtin_plugins, find};

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
        (None, true) => builtin_plugins().into_iter().filter(is_detected).collect(),
        (None, false) => bail!("specify a tool (e.g. `hotsheet setup claude`) or pass --detect"),
    };
    if plugins.is_empty() {
        bail!("no supported AI tools detected on this machine");
    }

    // Absolute store path so the MCP `--path` works from anywhere the tool launches.
    let store_abs = store_path.canonicalize().with_context(|| {
        format!(
            "store path does not exist: {} (run `hotsheet init` first)",
            store_path.display()
        )
    })?;

    let mut reports = Vec::new();
    for p in plugins {
        let wrote = vec![
            write_instructions(project_dir, &p)?,
            write_skill(project_dir, &p)?,
            write_mcp(project_dir, &store_abs, &p)?,
        ];
        reports.push(SetupReport {
            tool: p.manifest.product_name.clone(),
            wrote,
        });
    }
    Ok(reports)
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

/// Write the worklist skill (a fully managed file — overwritten).
fn write_skill(project: &Path, p: &Plugin) -> Result<String> {
    let rel = &p.manifest.skills.target;
    write_file(&project.join(rel), p.skill_body())?;
    Ok(rel.clone())
}

/// Register the `hotsheet-mcp` server in the tool's MCP config, merge-safe (other
/// servers + other top-level keys are preserved).
fn write_mcp(project: &Path, store_abs: &Path, p: &Plugin) -> Result<String> {
    let rel = &p.manifest.mcp.target;
    let target = project.join(rel);

    let mut root: serde_json::Value = std::fs::read_to_string(&target)
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
        p.manifest.mcp.server_name.clone(),
        serde_json::json!({
            "command": p.manifest.mcp.command,
            "args": p.mcp_args(&store_abs.to_string_lossy()),
        }),
    );

    write_file(&target, &(serde_json::to_string_pretty(&root)? + "\n"))?;
    Ok(rel.clone())
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
        assert!(claude_md.contains("hotsheet ls --up-next"));

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
