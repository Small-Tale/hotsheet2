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

use crate::{Plugin, all_plugins, default_dirs};

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
    run_setup_in(
        store_path,
        project_dir,
        tool,
        detect,
        enabled,
        &default_dirs(),
    )
}

/// [`run_setup`] with an explicit plugin search path, used by hosts that keep a stable
/// registry and by hermetic integration tests.
pub fn run_setup_in(
    store_path: &Path,
    project_dir: &Path,
    tool: Option<&str>,
    detect: bool,
    enabled: Option<&HashSet<String>>,
    plugin_dirs: &[std::path::PathBuf],
) -> Result<Vec<SetupReport>, SetupError> {
    let plugins: Vec<Plugin> = match (tool, detect) {
        (Some(id), _) => vec![
            crate::find_in(id, plugin_dirs)
                .ok_or_else(|| SetupError::UnknownTool(id.to_string()))?,
        ],
        (None, true) => all_plugins(plugin_dirs)
            .into_iter()
            .filter(is_detected)
            .filter(|p| enabled.is_none_or(|set| set.contains(p.id())))
            .collect(),
        (None, false) => return Err(SetupError::NoToolGiven),
    };
    if plugins.is_empty() {
        return Err(SetupError::NoneDetected);
    }

    setup_plugins(store_path, project_dir, plugins)
}

/// Refresh every tool that is currently detected or already has a Hot Sheet managed
/// instruction block. This is the idempotent project-open/startup path: it repairs partial
/// setup and updates bundled assets without requiring the client to know tool-specific files.
pub fn refresh_setup_in(
    store_path: &Path,
    project_dir: &Path,
    enabled: Option<&HashSet<String>>,
    plugin_dirs: &[std::path::PathBuf],
) -> Result<Vec<SetupReport>, SetupError> {
    let plugins = all_plugins(plugin_dirs)
        .into_iter()
        .filter(|plugin| enabled.is_none_or(|set| set.contains(plugin.id())))
        .filter(|plugin| is_detected(plugin) || has_managed_setup(project_dir, plugin))
        .collect::<Vec<_>>();
    if plugins.is_empty() {
        return Ok(Vec::new());
    }
    setup_plugins(store_path, project_dir, plugins)
}

fn has_managed_setup(project_dir: &Path, plugin: &Plugin) -> bool {
    let marker = format!("<!-- BEGIN hotsheet:{} -->", plugin.id());
    std::fs::read_to_string(project_dir.join(&plugin.manifest.instructions.target))
        .is_ok_and(|contents| contents.contains(&marker))
        || plugin
            .skill()
            .is_some_and(|(target, _)| project_dir.join(target).is_file())
}

fn setup_plugins(
    store_path: &Path,
    project_dir: &Path,
    plugins: Vec<Plugin>,
) -> Result<Vec<SetupReport>, SetupError> {
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
        if let Some(hook) = write_hooks(project_dir, &p)? {
            wrote.push(hook); // absent for tools with their own approval path (e.g. Codex)
        }
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

pub(crate) fn is_detected(p: &Plugin) -> bool {
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
    let extensions = executable_extensions();
    binary_in_paths(name, std::env::split_paths(&paths), &extensions)
}

fn binary_in_paths(
    name: &str,
    paths: impl IntoIterator<Item = std::path::PathBuf>,
    extensions: &[String],
) -> bool {
    paths.into_iter().any(|dir| {
        dir.join(name).is_file()
            || extensions
                .iter()
                .any(|extension| dir.join(format!("{name}{extension}")).is_file())
    })
}

fn executable_extensions() -> Vec<String> {
    if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    } else {
        Vec::new()
    }
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
    let locally_owned =
        !target.exists() || is_hotsheet_only_mcp_config(&target, &p.manifest.mcp.format, name);

    match p.manifest.mcp.format.as_str() {
        "claude-json" => write_mcp_json(&target, name, &command, &args)?,
        "codex-toml" => write_mcp_toml(&target, name, &command, &args)?,
        "opencode-json" => write_mcp_opencode_json(&target, name, &command, &args)?,
        other => {
            return Err(SetupError::UnknownMcpFormat {
                id: p.id().to_string(),
                format: other.to_string(),
            });
        }
    }
    if locally_owned {
        ensure_local_git_exclude(project, rel)?;
    }
    Ok(rel.clone())
}

fn is_hotsheet_only_mcp_config(target: &Path, format: &str, name: &str) -> bool {
    let Ok(contents) = std::fs::read_to_string(target) else {
        return false;
    };
    match format {
        "claude-json" => serde_json::from_str::<serde_json::Value>(&contents)
            .ok()
            .and_then(|root| {
                let object = root.as_object()?;
                let servers = object.get("mcpServers")?.as_object()?;
                Some(object.len() == 1 && servers.len() == 1 && servers.contains_key(name))
            })
            .unwrap_or(false),
        "opencode-json" => serde_json::from_str::<serde_json::Value>(&contents)
            .ok()
            .and_then(|root| {
                let object = root.as_object()?;
                let servers = object.get("mcp")?.as_object()?;
                Some(
                    object.keys().all(|key| key == "$schema" || key == "mcp")
                        && servers.len() == 1
                        && servers.contains_key(name),
                )
            })
            .unwrap_or(false),
        "codex-toml" => toml::from_str::<toml::Table>(&contents)
            .ok()
            .and_then(|root| {
                let servers = root.get("mcp_servers")?.as_table()?;
                Some(root.len() == 1 && servers.len() == 1 && servers.contains_key(name))
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn project_git_dir(project: &Path) -> Option<std::path::PathBuf> {
    let dot_git = project.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let pointer = std::fs::read_to_string(dot_git).ok()?;
    let path = pointer.trim().strip_prefix("gitdir:")?.trim();
    let path = std::path::PathBuf::from(path);
    Some(if path.is_absolute() {
        path
    } else {
        project.join(path)
    })
}

/// Keep a wholly Hot Sheet-owned machine config out of this checkout without changing its
/// shared `.gitignore`. Existing mixed/user-owned configs remain visible to git.
fn ensure_local_git_exclude(project: &Path, rel: &str) -> Result<(), SetupError> {
    let Some(git_dir) = project_git_dir(project) else {
        return Ok(());
    };
    let target = git_dir.join("info/exclude");
    let entry = format!("/{}", rel.replace('\\', "/"));
    let existing = std::fs::read_to_string(&target).unwrap_or_default();
    if existing.lines().any(|line| line == entry) {
        return Ok(());
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    write_file(&target, &format!("{existing}{separator}{entry}\n"))
}

/// Register the tool's permission hook (`docs/05` §5.7, HS2-YMR9HE) in its config, if it
/// declares one. Claude's `.claude/settings.local.json` shape:
/// `{ "hooks": { "<event>": [ { "matcher": "*", "hooks": [ { "type": "command", "command": … } ] } ] } }`.
/// Merge-safe + idempotent: an existing Hot Sheet hook (same resolved command) is not
/// duplicated. Returns the written path when a hook was registered.
fn write_hooks(project: &Path, p: &Plugin) -> Result<Option<String>, SetupError> {
    let Some(spec) = &p.manifest.hooks else {
        return Ok(None);
    };
    let target = project.join(&spec.target);
    let locally_owned = spec.machine_local || !target.exists();
    let command = resolve_hook_command(&spec.command);

    let mut root: serde_json::Value = std::fs::read_to_string(&target)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let obj = root.as_object_mut().unwrap();

    let hooks = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }
    let event = hooks
        .as_object_mut()
        .unwrap()
        .entry(spec.event.clone())
        .or_insert_with(|| serde_json::json!([]));
    if !event.is_array() {
        *event = serde_json::json!([]);
    }
    let arr = event.as_array_mut().unwrap();

    // Our hook entry (matcher "*" = every tool use).
    let entry = serde_json::json!({
        "matcher": "*",
        "hooks": [ { "type": "command", "command": command } ],
    });
    // Idempotent: drop any prior Hot Sheet permission-hook entry before re-adding, so
    // re-running setup doesn't stack duplicates. We recognize it by our command tail.
    arr.retain(|e| !is_hotsheet_hook(e));
    arr.push(entry);

    write_file(
        &target,
        &(serde_json::to_string_pretty(&root).unwrap() + "\n"),
    )?;
    if locally_owned {
        ensure_local_git_exclude(project, &spec.target)?;
    }
    Ok(Some(spec.target.clone()))
}

/// Whether a hook-array entry is a Hot Sheet permission hook (any of its commands ends with
/// `permission-hook`) — used to de-dupe on re-setup without touching the user's own hooks.
fn is_hotsheet_hook(entry: &serde_json::Value) -> bool {
    entry
        .get("hooks")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|hs| {
            hs.iter().any(|h| {
                h.get("command")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|c| c.trim_end().ends_with("permission-hook"))
            })
        })
}

/// Resolve a hook command's binary (its first token) to the absolute sibling next to the
/// running binary when present (no PATH reliance, HS2-103), keeping the rest of the args.
fn resolve_hook_command(command: &str) -> String {
    let mut parts = command.split_whitespace();
    let Some(bin) = parts.next() else {
        return command.to_string();
    };
    let resolved = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::parent)
        .map(|d| d.join(bin))
        .filter(|pth| pth.is_file())
        .map(|pth| pth.to_string_lossy().into_owned())
        .unwrap_or_else(|| bin.to_string());
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        resolved
    } else {
        format!("{resolved} {}", rest.join(" "))
    }
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

/// OpenCode project config: `{ "mcp": { "<name>": { type: "local", command: [...] } } }`.
fn write_mcp_opencode_json(
    target: &Path,
    name: &str,
    command: &str,
    args: &[String],
) -> Result<(), SetupError> {
    let mut root: serde_json::Value = std::fs::read_to_string(target)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({ "$schema": "https://opencode.ai/config.json" }));
    let mcp = root
        .as_object_mut()
        .unwrap()
        .entry("mcp")
        .or_insert_with(|| serde_json::json!({}));
    if !mcp.is_object() {
        *mcp = serde_json::json!({});
    }
    let mut command_line = vec![command.to_string()];
    command_line.extend_from_slice(args);
    mcp.as_object_mut().unwrap().insert(
        name.to_string(),
        serde_json::json!({ "type": "local", "command": command_line }),
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
    if std::fs::read(path).is_ok_and(|existing| existing == contents.as_bytes()) {
        return Ok(());
    }
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

    fn fixture_plugin(root: &Path, instructions: &str) {
        let plugin = root.join("fixture");
        std::fs::create_dir(&plugin).unwrap();
        std::fs::write(plugin.join("instructions.md"), instructions).unwrap();
        std::fs::write(plugin.join("SKILL.md"), "current skill\n").unwrap();
        std::fs::write(
            plugin.join("manifest.toml"),
            r#"
id = "fixture"
display_name = "Fixture"
product_name = "Fixture Tool"
tier = "cli-agent"
[detection]
binaries = ["definitely-not-installed-hotsheet-fixture"]
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[skills]
target = ".fixture/skills/hotsheet/SKILL.md"
source = "SKILL.md"
[mcp]
target = ".fixture/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
        )
        .unwrap();
    }

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

    #[test]
    fn refresh_repairs_a_stale_managed_tool_and_is_idempotent() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let plugins = tempfile::tempdir().unwrap();
        fixture_plugin(plugins.path(), "current instructions\n");
        let enabled = HashSet::from(["fixture".to_string()]);
        std::fs::write(
            project.path().join("AGENTS.md"),
            "User text.\n\n<!-- BEGIN hotsheet:fixture -->\nstale\n<!-- END hotsheet:fixture -->\n",
        )
        .unwrap();

        refresh_setup_in(
            store.path(),
            project.path(),
            Some(&enabled),
            &[plugins.path().to_path_buf()],
        )
        .unwrap();
        let instructions = std::fs::read(project.path().join("AGENTS.md")).unwrap();
        assert!(String::from_utf8_lossy(&instructions).contains("User text."));
        assert!(String::from_utf8_lossy(&instructions).contains("current instructions"));
        assert_eq!(
            std::fs::read(project.path().join(".fixture/skills/hotsheet/SKILL.md")).unwrap(),
            b"current skill\n"
        );
        assert!(project.path().join(".fixture/mcp.json").is_file());

        refresh_setup_in(
            store.path(),
            project.path(),
            Some(&enabled),
            &[plugins.path().to_path_buf()],
        )
        .unwrap();
        assert_eq!(
            std::fs::read(project.path().join("AGENTS.md")).unwrap(),
            instructions
        );
    }

    #[test]
    fn refresh_leaves_a_clean_unconfigured_project_untouched() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let plugins = tempfile::tempdir().unwrap();
        fixture_plugin(plugins.path(), "current instructions\n");
        let enabled = HashSet::from(["fixture".to_string()]);
        assert!(
            refresh_setup_in(
                store.path(),
                project.path(),
                Some(&enabled),
                &[plugins.path().to_path_buf()]
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(std::fs::read_dir(project.path()).unwrap().count(), 0);
    }

    #[test]
    fn unchanged_managed_files_are_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("managed.txt");
        std::fs::write(&path, "same").unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        write_file(&path, "same").unwrap();
    }

    #[test]
    fn local_excludes_follow_a_worktree_gitdir_pointer() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let git_dir = root.path().join("worktree-git");
        std::fs::create_dir(&project).unwrap();
        std::fs::create_dir_all(git_dir.join("info")).unwrap();
        std::fs::write(project.join(".git"), "gitdir: ../worktree-git\n").unwrap();

        ensure_local_git_exclude(&project, ".codex/config.toml").unwrap();

        assert_eq!(
            std::fs::read_to_string(git_dir.join("info/exclude")).unwrap(),
            "/.codex/config.toml\n"
        );
    }

    #[test]
    fn path_detection_supports_windows_command_wrappers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("codex.cmd"), "@echo off\n").unwrap();
        assert!(binary_in_paths(
            "codex",
            [dir.path().to_path_buf()],
            &[".cmd".to_string()]
        ));
    }

    #[test]
    fn bundled_claude_skill_matches_the_current_adapter_version() {
        fn version(contents: &str) -> &str {
            contents
                .lines()
                .find_map(|line| line.strip_prefix("<!-- hotsheet-skill-version: "))
                .and_then(|line| line.strip_suffix(" -->"))
                .expect("skill version marker")
        }
        assert_eq!(
            version(include_str!("../../../plugins/claude/SKILL.md")),
            version(include_str!("../../../.agents/skills/hotsheet/SKILL.md"))
        );
    }
}
