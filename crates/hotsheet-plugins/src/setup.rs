//! **Core-owned AI-tool setup** (`docs/05` §5.1a, `docs/04` §4.1, HS2-91). Preparing a
//! project for an AI tool — the managed instruction section, the worklist skill, and the
//! per-tool MCP-config entry — is a **one-shot, host-agnostic** operation, so it lives here
//! in the core plugins crate and is driven identically by the **CLI** (headless: no server,
//! no client) and the **server** (`POST /setup/<tool>` for client-driven flows). One impl,
//! no drift (`docs/04` §4.5).
//!
//! The `enabled` set (which plugins a project has opted into — `docs/05` settings) is passed
//! **in** by the caller, so this crate needs no ticketing/settings dependency and stays the
//! lean, mostly-declarative plugin core.

use std::collections::HashSet;
use std::path::Path;

use crate::{Plugin, all_plugins, default_dirs, find};

/// What one tool's setup wrote (project-relative paths), for reporting.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetupReport {
    pub tool: String,
    pub wrote: Vec<String>,
}

/// A setup failure.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("unknown tool '{0}' (no such plugin)")]
    UnknownTool(String),
    #[error("specify a tool (e.g. `setup claude`) or pass detect=true")]
    NoToolGiven,
    #[error("no supported AI tools detected on this machine")]
    NoneDetected,
    #[error("store path does not exist: {0} (run `init` first)")]
    NoStore(String),
    #[error("plugin '{id}' declares unsafe target path(s): {targets} (must be project-relative)")]
    UnsafeTargets { id: String, targets: String },
    #[error("unknown MCP config format '{format}' for plugin '{id}'")]
    UnknownMcpFormat { id: String, format: String },
    #[error("writing {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

/// Set up one named `tool`, or every **detected** tool when `detect` is set, writing into
/// `project_dir`; the MCP entry points at `store_path` (which must exist). `enabled`, when
/// `Some`, restricts `detect` to those plugin ids (the project's opted-in set); `None` = no
/// restriction. Discovers plugins across built-ins + the machine search path.
pub fn run_setup(
    store_path: &Path,
    project_dir: &Path,
    tool: Option<&str>,
    detect: bool,
    enabled: Option<&HashSet<String>>,
) -> Result<Vec<SetupReport>, SetupError> {
    let plugins: Vec<Plugin> = match (tool, detect) {
        (Some(id), _) => vec![find(id).ok_or_else(|| SetupError::UnknownTool(id.to_string()))?],
        (None, true) => all_plugins(&default_dirs())
            .into_iter()
            .filter(is_detected)
            .filter(|p| enabled.is_none_or(|set| set.contains(p.id())))
            .collect(),
        (None, false) => return Err(SetupError::NoToolGiven),
    };
    if plugins.is_empty() {
        return Err(SetupError::NoneDetected);
    }

    // Absolute store path so the MCP `--path` works from wherever the tool launches.
    let store_abs = store_path
        .canonicalize()
        .map_err(|_| SetupError::NoStore(store_path.display().to_string()))?;

    let mut reports = Vec::new();
    for p in plugins {
        let bad = p.unsafe_targets();
        if !bad.is_empty() {
            return Err(SetupError::UnsafeTargets {
                id: p.id().to_string(),
                targets: bad.join(", "),
            });
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

/// The `hotsheet-mcp` command string to record in a tool's MCP config: the absolute sibling
/// next to the running binary when it exists (so the config needs no PATH munging — HS2-103),
/// else the bare `fallback` (resolved via PATH at launch). Works from any binary (CLI or
/// server) since `hotsheet-mcp` installs alongside them.
pub fn mcp_command(fallback: &str) -> String {
    mcp_command_for(
        std::env::current_exe()
            .ok()
            .as_deref()
            .and_then(Path::parent),
        fallback,
    )
}

fn mcp_command_for(exe_dir: Option<&Path>, fallback: &str) -> String {
    exe_dir
        .map(|d| d.join("hotsheet-mcp"))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback.to_string())
}

fn is_detected(p: &Plugin) -> bool {
    p.manifest
        .detection
        .binaries
        .iter()
        .any(|b| binary_on_path(b))
}

fn binary_on_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
}

fn write_instructions(project: &Path, p: &Plugin) -> Result<String, SetupError> {
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

/// Replace the region between the markers (inclusive) with `block`, or append it if the
/// markers aren't present. Preserves everything outside the block.
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

/// Write the worklist skill (a fully managed file), or nothing if the tool has no skills
/// concept (Codex). Returns the written path when present.
fn write_skill(project: &Path, p: &Plugin) -> Result<Option<String>, SetupError> {
    match p.skill() {
        Some((target, body)) => {
            write_file(&project.join(target), body)?;
            Ok(Some(target.to_string()))
        }
        None => Ok(None),
    }
}

/// Register the `hotsheet-mcp` server in the tool's MCP config, merge-safe. The writer is
/// chosen by the manifest's `format` (a host helper keyed on the format, not the tool id —
/// docs/05 §5.3), so a new tool with a known format needs no code.
fn write_mcp(project: &Path, store_abs: &Path, p: &Plugin) -> Result<String, SetupError> {
    let rel = &p.manifest.mcp.target;
    let target = project.join(rel);
    let name = &p.manifest.mcp.server_name;
    let command = mcp_command(&p.manifest.mcp.command);
    let args = p.mcp_args(&store_abs.to_string_lossy());

    match p.manifest.mcp.format.as_str() {
        "claude-json" => write_mcp_json(&target, name, &command, &args)?,
        "codex-toml" => write_mcp_toml(&target, name, &command, &args)?,
        other => {
            return Err(SetupError::UnknownMcpFormat {
                id: p.id().to_string(),
                format: other.to_string(),
            });
        }
    }
    Ok(rel.clone())
}

/// Claude-style `.mcp.json`: `{ "mcpServers": { "<name>": { command, args } } }`.
fn write_mcp_json(
    target: &Path,
    name: &str,
    command: &str,
    args: &[String],
) -> Result<(), SetupError> {
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
    write_file(
        target,
        &(serde_json::to_string_pretty(&root).unwrap() + "\n"),
    )
}

/// Codex-style TOML: `[mcp_servers.<name>]` with `command` + `args`.
fn write_mcp_toml(
    target: &Path,
    name: &str,
    command: &str,
    args: &[String],
) -> Result<(), SetupError> {
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

    write_file(target, &toml::to_string_pretty(&root).unwrap())
}

fn write_file(path: &Path, contents: &str) -> Result<(), SetupError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| SetupError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    std::fs::write(path, contents).map_err(|source| SetupError::Io {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_command_prefers_the_absolute_sibling() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            mcp_command_for(Some(dir.path()), "hotsheet-mcp"),
            "hotsheet-mcp"
        );
        assert_eq!(mcp_command_for(None, "hotsheet-mcp"), "hotsheet-mcp");
        let sib = dir.path().join("hotsheet-mcp");
        std::fs::write(&sib, "x").unwrap();
        assert_eq!(
            mcp_command_for(Some(dir.path()), "hotsheet-mcp"),
            sib.to_string_lossy()
        );
    }

    #[test]
    fn detect_with_no_tool_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_setup(dir.path(), dir.path(), None, false, None).unwrap_err();
        assert!(matches!(err, SetupError::NoToolGiven));
    }
}
