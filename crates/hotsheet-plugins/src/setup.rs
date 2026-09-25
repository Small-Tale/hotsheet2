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
    #[error("unknown hook config format '{format}' for plugin '{id}'")]
    UnknownHookFormat { id: String, format: String },
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

    setup_plugins(store_path, project_dir, plugins, None)
}

/// Refresh every tool that is currently detected or already has a Hot Sheet managed
/// instruction block. This is the idempotent project-open/startup path: it repairs partial
/// setup and updates bundled assets without requiring the client to know tool-specific files.
///
/// Refresh also reconciles **shared** instruction sections (HS2-329EED): a shared section's
/// served-tool list is rebuilt from the tools in this refresh, and a shared section whose
/// every listed tool is now disabled (or no longer installed as a plugin) is removed.
pub fn refresh_setup_in(
    store_path: &Path,
    project_dir: &Path,
    enabled: Option<&HashSet<String>>,
    plugin_dirs: &[std::path::PathBuf],
) -> Result<Vec<SetupReport>, SetupError> {
    let all = all_plugins(plugin_dirs);
    let mut reconcile_targets: Vec<String> = Vec::new();
    for plugin in &all {
        let target = &plugin.manifest.instructions.target;
        if crate::is_safe_rel_path(target) && !reconcile_targets.contains(target) {
            reconcile_targets.push(target.clone());
        }
    }
    // Tools the project explicitly left out of `enabled_plugins` (HS2-FKC8VN). Without an
    // enabled list nothing is excluded: refresh then only ever adds or repairs.
    let (plugins, excluded): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|plugin| enabled.is_none_or(|set| set.contains(plugin.id())));
    let plugins = plugins
        .into_iter()
        .filter(|plugin| is_detected(plugin) || has_managed_setup(project_dir, plugin))
        .collect::<Vec<_>>();
    let reports = if plugins.is_empty() {
        // Nothing to set up, but a shared section whose sharers were all disabled is removed.
        let run_ids = HashSet::new();
        for target in &reconcile_targets {
            write_instruction_target(project_dir, target, &[], Some(&run_ids))?;
        }
        Vec::new()
    } else {
        setup_plugins(store_path, project_dir, plugins, Some(&reconcile_targets))?
    };
    remove_disabled_tool_sections(project_dir, &excluded)?;
    Ok(reports)
}

/// Remove the per-tool `hotsheet:<id>` instruction section of every tool the project
/// disabled, so disabled-tool guidance leaves per-tool and shared layouts alike
/// (HS2-FKC8VN). The same ownership rule as refresh applies: a section written by a newer
/// Hot Sheet, or an equal-version section the project customized, is preserved. The file
/// is never created, and content outside the managed markers is untouched.
fn remove_disabled_tool_sections(project: &Path, disabled: &[Plugin]) -> Result<(), SetupError> {
    for plugin in disabled {
        let rel = &plugin.manifest.instructions.target;
        if !crate::is_safe_rel_path(rel) {
            continue;
        }
        let path = project.join(rel);
        let Ok(existing) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (begin, end) = (begin_marker(plugin.id()), end_marker(plugin.id()));
        let bundled_block = format!("{begin}\n{}\n{end}", plugin.instructions_body().trim_end());
        let bundled_version =
            marked_version(plugin.instructions_body(), INSTRUCTIONS_VERSION_PREFIX).unwrap_or(0);
        let mut out = existing.clone();
        while let Some((start, finish)) = find_block(&out, &begin, &end) {
            if marked_artifact_requires_preservation(
                &out[start..finish],
                &bundled_block,
                bundled_version,
                INSTRUCTIONS_VERSION_PREFIX,
            ) {
                break;
            }
            out = remove_range(&out, (start, finish));
        }
        if out != existing {
            write_file(&path, &out)?;
        }
    }
    Ok(())
}

fn has_managed_setup(project_dir: &Path, plugin: &Plugin) -> bool {
    let marker = format!("<!-- BEGIN hotsheet:{} -->", plugin.id());
    let target = &plugin.manifest.instructions.target;
    std::fs::read_to_string(project_dir.join(target)).is_ok_and(|contents| {
        contents.contains(&marker)
            || parse_shared_section(&contents, &shared_section_key(target))
                .is_some_and(|shared| shared.tools.iter().any(|tool| tool == plugin.id()))
    }) || plugin
        .skill()
        .is_some_and(|(target, _)| project_dir.join(target).is_file())
}

/// Write every plugin's artifacts. `refresh_targets` is `Some` on the refresh path: it names
/// every instruction target known to the plugin registry and makes this run's tool set
/// authoritative for shared-section membership. Explicit setup (`None`) only ever adds
/// tools to an existing shared section.
fn setup_plugins(
    store_path: &Path,
    project_dir: &Path,
    plugins: Vec<Plugin>,
    refresh_targets: Option<&[String]>,
) -> Result<Vec<SetupReport>, SetupError> {
    // Absolute store path so the MCP `--path` works from wherever the tool launches.
    let store_abs = store_path
        .canonicalize()
        .map_err(|_| SetupError::NoStore(store_path.display().to_string()))?;

    for p in &plugins {
        let bad = p.unsafe_targets();
        if !bad.is_empty() {
            return Err(SetupError::UnsafeTargets {
                id: p.id().to_string(),
                targets: bad.join(", "),
            });
        }
    }

    let preserved: Vec<bool> = plugins
        .iter()
        .map(|p| installed_workflow_requires_preservation(project_dir, p))
        .collect();
    let run_ids: HashSet<String> = plugins.iter().map(|p| p.id().to_string()).collect();
    let membership = refresh_targets.map(|_| &run_ids);

    // Instruction targets first, grouped so tools sharing one file (e.g. AGENTS.md) can be
    // served by one shared section instead of byte-identical per-tool copies.
    let mut targets: Vec<&str> = Vec::new();
    for (p, preserve) in plugins.iter().zip(&preserved) {
        let target = p.manifest.instructions.target.as_str();
        if !preserve && !targets.contains(&target) {
            targets.push(target);
        }
    }
    for target in &targets {
        let writers: Vec<&Plugin> = plugins
            .iter()
            .zip(&preserved)
            .filter(|(p, preserve)| !**preserve && p.manifest.instructions.target == *target)
            .map(|(p, _)| p)
            .collect();
        write_instruction_target(project_dir, target, &writers, membership)?;
    }
    for target in refresh_targets.unwrap_or_default() {
        if !targets.contains(&target.as_str()) {
            write_instruction_target(project_dir, target, &[], membership)?;
        }
    }

    let mut reports = Vec::new();
    for (p, preserve_installed_workflow) in plugins.iter().zip(preserved) {
        let mut wrote = Vec::new();
        if !preserve_installed_workflow {
            wrote.push(p.manifest.instructions.target.clone());
            if let Some(skill) = write_skill(project_dir, p)? {
                wrote.push(skill); // absent for tools with no skills concept (e.g. Antigravity)
            }
        }
        wrote.push(write_mcp(project_dir, &store_abs, p)?);
        if let Some(hook) = write_hooks(project_dir, p)? {
            wrote.push(hook); // absent when no native interactive adapter is declared
        }
        reports.push(SetupReport {
            tool: p.manifest.product_name.clone(),
            wrote,
        });
    }
    Ok(reports)
}

const INSTRUCTIONS_VERSION_PREFIX: &str = "<!-- hotsheet-instructions-version: ";
const SKILL_VERSION_PREFIX: &str = "<!-- hotsheet-skill-version: ";

fn marked_version(contents: &str, prefix: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        line.trim()
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(" -->"))
            .and_then(|value| value.parse().ok())
    })
}

fn installed_workflow_requires_preservation(project: &Path, plugin: &Plugin) -> bool {
    let bundled_instruction_version =
        marked_version(plugin.instructions_body(), INSTRUCTIONS_VERSION_PREFIX).unwrap_or(0);
    let instructions =
        std::fs::read_to_string(project.join(&plugin.manifest.instructions.target)).ok();
    let begin = format!("<!-- BEGIN hotsheet:{} -->", plugin.id());
    let end = format!("<!-- END hotsheet:{} -->", plugin.id());
    let bundled_instruction_block =
        format!("{begin}\n{}\n{end}", plugin.instructions_body().trim_end());
    let installed_instruction_block = instructions.as_deref().and_then(|contents| {
        let start = contents.find(&begin)?;
        let finish = contents[start..].find(&end)? + start + end.len();
        Some(&contents[start..finish])
    });
    if installed_instruction_block.is_some_and(|installed| {
        marked_artifact_requires_preservation(
            installed,
            &bundled_instruction_block,
            bundled_instruction_version,
            INSTRUCTIONS_VERSION_PREFIX,
        )
    }) {
        return true;
    }
    // A shared section in the same file is this tool's installed instructions too (it
    // serves, or would serve, every sharer of the file), so the same newer/equal-divergent
    // rules apply to its body — the served-tool list line is membership, not content.
    let shared_key = shared_section_key(&plugin.manifest.instructions.target);
    if instructions
        .as_deref()
        .and_then(|contents| parse_shared_section(contents, &shared_key))
        .is_some_and(|shared| {
            marked_artifact_requires_preservation(
                &shared.body,
                plugin.instructions_body().trim_end(),
                bundled_instruction_version,
                INSTRUCTIONS_VERSION_PREFIX,
            )
        })
    {
        return true;
    }
    plugin.skill().is_some_and(|(target, bundled)| {
        let bundled_version = marked_version(bundled, SKILL_VERSION_PREFIX).unwrap_or(0);
        std::fs::read_to_string(project.join(target))
            .ok()
            .is_some_and(|installed| {
                marked_artifact_requires_preservation(
                    &installed,
                    bundled,
                    bundled_version,
                    SKILL_VERSION_PREFIX,
                )
            })
    })
}

/// A strictly newer marker belongs to a newer writer. An equal marker with different
/// bytes belongs to a same-generation variant or an intentional project customization.
/// In either case this writer cannot prove ownership of the installed representation and
/// must leave it alone. Older and unversioned artifacts remain eligible for migration.
fn marked_artifact_requires_preservation(
    installed: &str,
    bundled: &str,
    bundled_version: u64,
    version_prefix: &str,
) -> bool {
    marked_version(installed, version_prefix).is_some_and(|installed_version| {
        installed_version > bundled_version
            || (installed_version == bundled_version && installed != bundled)
    })
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

/// Write one instruction file's Hot Sheet sections for `writers` (the non-preserved tools
/// in this run that target it), merge-safe. See [`render_instruction_target`].
fn write_instruction_target(
    project: &Path,
    rel: &str,
    writers: &[&Plugin],
    refresh_members: Option<&HashSet<String>>,
) -> Result<(), SetupError> {
    let target = project.join(rel);
    let existing = std::fs::read_to_string(&target).ok();
    if writers.is_empty() && existing.is_none() {
        return Ok(()); // never create a file just to reconcile it
    }
    let existing = existing.unwrap_or_default();
    let tools: Vec<(&str, &str)> = writers
        .iter()
        .map(|p| (p.id(), p.instructions_body()))
        .collect();
    let rendered = render_instruction_target(&existing, rel, &tools, refresh_members);
    if rendered == existing {
        return Ok(());
    }
    write_file(&target, &rendered)
}

/// The Hot Sheet managed sections of one instruction file (HS2-329EED), as a pure function
/// of its current contents.
///
/// * `tools` — `(id, bundled body)` for each tool writing this file in this run.
/// * `refresh_members` — `Some(run ids)` on the refresh path: a shared section's served list
///   keeps only tools in the run (dropping disabled/unknown ones). `None` (explicit setup)
///   never drops an existing member.
///
/// When every writer carries a byte-identical body and there are at least two writers (or
/// the file already has a shared section), the writers are served by **one shared section**
/// `hotsheet:<target-key>` that lists them; their per-tool sections are migrated into it (at
/// the first one's position) and removed. Writers whose bodies differ keep per-tool sections
/// and leave any shared section's served list. A shared section with no remaining members is
/// removed. Everything outside the managed markers is preserved.
fn render_instruction_target(
    existing: &str,
    target: &str,
    tools: &[(&str, &str)],
    refresh_members: Option<&HashSet<String>>,
) -> String {
    let key = shared_section_key(target);
    let shared = parse_shared_section(existing, &key);
    let mut members: std::collections::BTreeSet<String> = shared
        .as_ref()
        .map(|s| s.tools.iter().cloned().collect())
        .unwrap_or_default();
    if let Some(run) = refresh_members {
        members.retain(|id| run.contains(id));
    }
    let body = tools.first().map(|(_, body)| body.trim_end());
    let identical = tools
        .iter()
        .all(|(_, candidate)| Some(candidate.trim_end()) == body);
    let use_shared = identical && (tools.len() >= 2 || (!tools.is_empty() && shared.is_some()));
    let mut out = existing.to_string();

    if use_shared {
        let body = body.unwrap_or_default();
        members.extend(tools.iter().map(|(id, _)| (*id).to_string()));
        let block = shared_block(&key, &members, body);
        if let Some(shared) = &shared {
            out.replace_range(shared.start..shared.end, &block);
        } else if let Some((start, end)) = tools
            .iter()
            .filter_map(|(id, _)| find_block(&out, &begin_marker(id), &end_marker(id)))
            .min()
        {
            out.replace_range(start..end, &block);
        } else {
            out = append_block(&out, &block);
        }
        for (id, _) in tools {
            while let Some(range) = find_block(&out, &begin_marker(id), &end_marker(id)) {
                out = remove_range(&out, range);
            }
        }
        return out;
    }

    for (id, body) in tools {
        let (begin, end) = (begin_marker(id), end_marker(id));
        let block = format!("{begin}\n{}\n{end}", body.trim_end());
        out = match find_block(&out, &begin, &end) {
            Some((start, finish)) => {
                format!("{}{block}{}", &out[..start], &out[finish..])
            }
            None => append_block(&out, &block),
        };
        members.remove(*id);
    }
    if let Some(shared) = parse_shared_section(&out, &key) {
        if members.is_empty() {
            out = remove_range(&out, (shared.start, shared.end));
        } else if !tools.is_empty() {
            // Only rewrite membership when this run touched the file; a no-writer refresh
            // leaves a still-served (possibly newer, preserved) section byte-for-byte alone.
            let block = shared_block(&key, &members, &shared.body);
            out.replace_range(shared.start..shared.end, &block);
        }
    }
    out
}

fn begin_marker(id: &str) -> String {
    format!("<!-- BEGIN hotsheet:{id} -->")
}

fn end_marker(id: &str) -> String {
    format!("<!-- END hotsheet:{id} -->")
}

const SHARED_TOOLS_PREFIX: &str = "<!-- hotsheet-shared-section: ";

/// The shared-section key for an instruction target: its path lowercased with every run of
/// non-alphanumerics folded to `-` (`AGENTS.md` → `agents-md`).
fn shared_section_key(target: &str) -> String {
    let mut key = String::new();
    for ch in target.chars() {
        if ch.is_ascii_alphanumeric() {
            key.push(ch.to_ascii_lowercase());
        } else if !key.ends_with('-') && !key.is_empty() {
            key.push('-');
        }
    }
    key.trim_end_matches('-').to_string()
}

fn shared_block(key: &str, members: &std::collections::BTreeSet<String>, body: &str) -> String {
    let tools = members.iter().cloned().collect::<Vec<_>>().join(", ");
    format!(
        "{}\n{SHARED_TOOLS_PREFIX}{tools} -->\n{}\n{}",
        begin_marker(key),
        body.trim_end(),
        end_marker(key)
    )
}

/// An installed shared section: its byte range, the tools it lists, and its body (the
/// content between the served-tools line and the END marker).
#[derive(Debug, PartialEq)]
struct SharedSection {
    start: usize,
    end: usize,
    tools: Vec<String>,
    body: String,
}

fn parse_shared_section(contents: &str, key: &str) -> Option<SharedSection> {
    let (begin, end) = (begin_marker(key), end_marker(key));
    let (start, finish) = find_block(contents, &begin, &end)?;
    let inner = &contents[start + begin.len()..finish - end.len()];
    let inner = inner.strip_prefix('\n').unwrap_or(inner);
    let (tools, body) = match inner
        .strip_prefix(SHARED_TOOLS_PREFIX)
        .map(|rest| rest.split_once('\n').unwrap_or((rest, "")))
    {
        Some((line, body)) => (
            line.trim_end()
                .strip_suffix("-->")
                .unwrap_or(line)
                .split(',')
                .map(str::trim)
                .filter(|tool| !tool.is_empty())
                .map(String::from)
                .collect(),
            body,
        ),
        None => (Vec::new(), inner),
    };
    Some(SharedSection {
        start,
        end: finish,
        tools,
        body: body.strip_suffix('\n').unwrap_or(body).to_string(),
    })
}

/// The byte range of the first `begin`..`end` block (inclusive of both markers).
fn find_block(contents: &str, begin: &str, end: &str) -> Option<(usize, usize)> {
    let start = contents.find(begin)?;
    let finish = contents[start..].find(end)? + start + end.len();
    Some((start, finish))
}

fn append_block(existing: &str, block: &str) -> String {
    if existing.trim().is_empty() {
        format!("{block}\n")
    } else {
        format!("{}\n\n{block}\n", existing.trim_end())
    }
}

/// Remove a managed block, collapsing only the blank lines that separated it from its
/// neighbours so the surrounding user content keeps its shape.
fn remove_range(contents: &str, (start, end): (usize, usize)) -> String {
    let head = contents[..start].trim_end_matches(['\n', '\r']);
    let tail = contents[end..].trim_start_matches(['\n', '\r']);
    match (head.is_empty(), tail.is_empty()) {
        (true, true) => String::new(),
        (true, false) => tail.to_string(),
        (false, true) => format!("{head}\n"),
        (false, false) => format!("{head}\n\n{tail}"),
    }
}

/// Write the worklist skill (a fully managed file), or nothing if the tool has no skills
/// concept. Returns the written path when present.
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

/// Register the tool's permission hook (`docs/05` §5.7) in its config, if it declares one.
/// Claude and Codex both currently document this lifecycle JSON shape:
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

    if spec.format != "lifecycle-json" {
        return Err(SetupError::UnknownHookFormat {
            id: p.id().to_string(),
            format: spec.format.clone(),
        });
    }

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
    let hooks = hooks.as_object_mut().unwrap();
    // Remove every prior Hot Sheet permission hook before rebuilding the declared event
    // set. This also migrates projects when a plugin changes its primary hook event.
    for entries in hooks.values_mut() {
        if let Some(entries) = entries.as_array_mut() {
            entries.retain(|entry| !is_hotsheet_hook(entry));
        }
    }

    // Our hook entry uses the provider-native matcher declared by the plugin.
    let mut handler = serde_json::json!({ "type": "command", "command": command });
    if let Some(timeout) = spec.timeout_seconds {
        handler["timeout"] = serde_json::json!(timeout);
    }
    let entry = serde_json::json!({
        "matcher": spec.matcher,
        "hooks": [handler],
    });
    let mut events = vec![spec.event.as_str()];
    for event in &spec.additional_events {
        if !events.contains(&event.as_str()) {
            events.push(event);
        }
    }
    for event_name in events {
        let event = hooks
            .entry(event_name.to_string())
            .or_insert_with(|| serde_json::json!([]));
        if !event.is_array() {
            *event = serde_json::json!([]);
        }
        event.as_array_mut().unwrap().push(entry.clone());
    }

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
        fixture_plugin_version(root, instructions, 1);
    }

    fn fixture_plugin_version(root: &Path, instructions: &str, version: u64) {
        let plugin = root.join("fixture");
        std::fs::create_dir(&plugin).unwrap();
        std::fs::write(
            plugin.join("instructions.md"),
            format!("<!-- hotsheet-instructions-version: {version} -->\n{instructions}"),
        )
        .unwrap();
        std::fs::write(
            plugin.join("SKILL.md"),
            format!("<!-- hotsheet-skill-version: {version} -->\ncurrent skill\n"),
        )
        .unwrap();
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

    /// A fixture tool `id` targeting `AGENTS.md` with the given versioned instruction body.
    fn sharing_tool(root: &Path, id: &str, instructions: &str, version: u64) {
        let plugin = root.join(id);
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join("instructions.md"),
            format!("<!-- hotsheet-instructions-version: {version} -->\n{instructions}"),
        )
        .unwrap();
        std::fs::write(
            plugin.join("SKILL.md"),
            format!("<!-- hotsheet-skill-version: {version} -->\n{id} skill\n"),
        )
        .unwrap();
        std::fs::write(
            plugin.join("manifest.toml"),
            format!(
                r#"
id = "{id}"
display_name = "{id}"
product_name = "{id} tool"
tier = "cli-agent"
[detection]
binaries = ["definitely-not-installed-hotsheet-{id}"]
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[skills]
target = ".{id}/SKILL.md"
source = "SKILL.md"
[mcp]
target = ".{id}/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{{store}}"]
"#
            ),
        )
        .unwrap();
    }

    struct Sharing {
        store: tempfile::TempDir,
        project: tempfile::TempDir,
        plugins: tempfile::TempDir,
    }

    impl Sharing {
        /// Three fixture tools (`alpha`, `beta`, `gamma`) sharing one identical body.
        fn new() -> Self {
            let fixture = Self {
                store: tempfile::tempdir().unwrap(),
                project: tempfile::tempdir().unwrap(),
                plugins: tempfile::tempdir().unwrap(),
            };
            for id in ["alpha", "beta", "gamma"] {
                sharing_tool(fixture.plugins.path(), id, "shared instructions\n", 8);
            }
            fixture
        }

        fn refresh(&self, enabled: &[&str]) -> Vec<SetupReport> {
            let enabled: HashSet<String> = enabled.iter().map(|id| id.to_string()).collect();
            refresh_setup_in(
                self.store.path(),
                self.project.path(),
                Some(&enabled),
                &[self.plugins.path().to_path_buf()],
            )
            .unwrap()
        }

        fn setup(&self, tool: &str) -> Vec<SetupReport> {
            run_setup_in(
                self.store.path(),
                self.project.path(),
                Some(tool),
                false,
                None,
                &[self.plugins.path().to_path_buf()],
            )
            .unwrap()
        }

        fn agents(&self) -> String {
            std::fs::read_to_string(self.project.path().join("AGENTS.md")).unwrap_or_default()
        }

        fn write_agents(&self, contents: &str) {
            std::fs::write(self.project.path().join("AGENTS.md"), contents).unwrap();
        }
    }

    const SHARED_BODY: &str = "<!-- hotsheet-instructions-version: 8 -->\nshared instructions";

    fn shared(tools: &str) -> String {
        format!(
            "<!-- BEGIN hotsheet:agents-md -->\n<!-- hotsheet-shared-section: {tools} -->\n{SHARED_BODY}\n<!-- END hotsheet:agents-md -->"
        )
    }

    fn per_tool(id: &str, body: &str) -> String {
        format!("<!-- BEGIN hotsheet:{id} -->\n{body}\n<!-- END hotsheet:{id} -->")
    }

    #[test]
    fn shared_section_keys_fold_the_target_path() {
        assert_eq!(shared_section_key("AGENTS.md"), "agents-md");
        assert_eq!(
            shared_section_key(".github/copilot-instructions.md"),
            "github-copilot-instructions-md"
        );
        assert_eq!(shared_section_key("docs//AI--RULES.md"), "docs-ai-rules-md");
    }

    #[test]
    fn shared_sections_round_trip_through_the_parser() {
        let members = ["beta".to_string(), "alpha".to_string()]
            .into_iter()
            .collect();
        let block = shared_block("agents-md", &members, "line one\nline two\n");
        let contents = format!("Intro\n\n{block}\n\nOutro\n");
        let parsed = parse_shared_section(&contents, "agents-md").unwrap();
        assert_eq!(parsed.tools, ["alpha", "beta"]);
        assert_eq!(parsed.body, "line one\nline two");
        assert_eq!(&contents[parsed.start..parsed.end], block);

        // A hand-written section without a served-tools line parses as unowned content.
        let bare = "<!-- BEGIN hotsheet:agents-md -->\nbody\n<!-- END hotsheet:agents-md -->";
        let parsed = parse_shared_section(bare, "agents-md").unwrap();
        assert!(parsed.tools.is_empty());
        assert_eq!(parsed.body, "body");
    }

    #[test]
    fn identical_bodies_migrate_per_tool_copies_into_one_shared_section() {
        let existing = format!(
            "# Project\n\nUser rules.\n\n{}\n\nMiddle user text.\n\n{}\n\n{}\n\nTrailing user text.\n",
            per_tool("alpha", "stale alpha"),
            per_tool("beta", "stale beta"),
            per_tool("gamma", "stale gamma"),
        );
        let tools = [
            ("alpha", "shared body\n"),
            ("beta", "shared body\n"),
            ("gamma", "shared body\n"),
        ];
        let rendered = render_instruction_target(&existing, "AGENTS.md", &tools, None);
        assert_eq!(
            rendered,
            "# Project\n\nUser rules.\n\n<!-- BEGIN hotsheet:agents-md -->\n<!-- hotsheet-shared-section: alpha, beta, gamma -->\nshared body\n<!-- END hotsheet:agents-md -->\n\nMiddle user text.\n\nTrailing user text.\n"
        );
        assert_eq!(
            render_instruction_target(&rendered, "AGENTS.md", &tools, None),
            rendered,
            "a second render is a byte-level no-op"
        );
    }

    #[test]
    fn differing_bodies_keep_per_tool_sections_and_retire_the_shared_one() {
        let existing = format!("User text.\n\n{}\n", shared("alpha, beta"));
        let rendered = render_instruction_target(
            &existing,
            "AGENTS.md",
            &[("alpha", "alpha body\n"), ("beta", "beta body\n")],
            None,
        );
        assert_eq!(
            rendered,
            format!(
                "User text.\n\n{}\n\n{}\n",
                per_tool("alpha", "alpha body"),
                per_tool("beta", "beta body")
            )
        );

        // A member not rewritten in this run keeps the shared section alive (membership only).
        let existing = format!("User text.\n\n{}\n", shared("alpha, beta, gamma"));
        let rendered = render_instruction_target(
            &existing,
            "AGENTS.md",
            &[("alpha", "alpha body\n"), ("beta", "beta body\n")],
            None,
        );
        assert!(rendered.contains(&shared("gamma")));
        assert!(rendered.contains(&per_tool("alpha", "alpha body")));
    }

    #[test]
    fn a_single_tool_in_a_clean_file_keeps_its_per_tool_section() {
        let rendered = render_instruction_target("", "CLAUDE.md", &[("claude", "body\n")], None);
        assert_eq!(rendered, format!("{}\n", per_tool("claude", "body")));
    }

    #[test]
    fn refresh_migrates_sharing_tools_and_is_byte_idempotent() {
        let fixture = Sharing::new();
        fixture.write_agents(&format!(
            "User text.\n\n{}\n\n{}\n\n{}\n",
            per_tool("alpha", "stale"),
            per_tool("beta", "stale"),
            per_tool("gamma", "stale")
        ));
        let reports = fixture.refresh(&["alpha", "beta", "gamma"]);
        assert_eq!(
            fixture.agents(),
            format!("User text.\n\n{}\n", shared("alpha, beta, gamma"))
        );
        assert!(reports.iter().all(|report| report.wrote[0] == "AGENTS.md"));
        for id in ["alpha", "beta", "gamma"] {
            assert!(
                fixture
                    .project
                    .path()
                    .join(format!(".{id}/SKILL.md"))
                    .is_file()
            );
        }

        let first = std::fs::read(fixture.project.path().join("AGENTS.md")).unwrap();
        fixture.refresh(&["alpha", "beta", "gamma"]);
        assert_eq!(
            std::fs::read(fixture.project.path().join("AGENTS.md")).unwrap(),
            first
        );
    }

    #[test]
    fn disabling_and_re_enabling_sharers_walks_the_membership_transitions() {
        let fixture = Sharing::new();
        fixture.write_agents(&format!(
            "User text.\n\n{}\n\n{}\n\n{}\n\nFooter.\n",
            per_tool("alpha", "stale"),
            per_tool("beta", "stale"),
            per_tool("gamma", "stale")
        ));
        let expect = |tools: &str| format!("User text.\n\n{}\n\nFooter.\n", shared(tools));

        fixture.refresh(&["alpha", "beta", "gamma"]);
        assert_eq!(fixture.agents(), expect("alpha, beta, gamma"));
        // Disable one sharer: the section stays for the others.
        fixture.refresh(&["alpha", "beta"]);
        assert_eq!(fixture.agents(), expect("alpha, beta"));
        // Down to one sharer: still one shared section, not a per-tool re-split.
        fixture.refresh(&["alpha"]);
        assert_eq!(fixture.agents(), expect("alpha"));
        fixture.refresh(&["alpha"]);
        assert_eq!(fixture.agents(), expect("alpha"));
        // No sharer remains: the section is removed and user content keeps its shape.
        assert!(fixture.refresh(&["unrelated"]).is_empty());
        assert_eq!(fixture.agents(), "User text.\n\nFooter.\n");
        assert!(fixture.refresh(&["unrelated"]).is_empty());
        assert_eq!(fixture.agents(), "User text.\n\nFooter.\n");
        // Re-enable two (their skills mark them as previously managed): recreated once.
        fixture.refresh(&["gamma", "beta"]);
        assert_eq!(
            fixture.agents(),
            format!("User text.\n\nFooter.\n\n{}\n", shared("beta, gamma"))
        );
        fixture.refresh(&["alpha", "beta", "gamma"]);
        let settled = format!(
            "User text.\n\nFooter.\n\n{}\n",
            shared("alpha, beta, gamma")
        );
        assert_eq!(fixture.agents(), settled);
        fixture.refresh(&["alpha", "beta", "gamma"]);
        assert_eq!(fixture.agents(), settled);
    }

    /// HS2-FKC8VN: a disabled tool's per-tool section leaves on refresh, like a shared
    /// section's member does, and returns when the tool is re-enabled.
    #[test]
    fn disabling_a_per_tool_writer_removes_its_section_and_re_enabling_restores_it() {
        let fixture = Sharing::new();
        // `delta` writes AGENTS.md with a different body, so nothing shares while it is
        // enabled and every tool keeps a per-tool section.
        sharing_tool(fixture.plugins.path(), "delta", "delta instructions\n", 8);
        let delta = per_tool(
            "delta",
            "<!-- hotsheet-instructions-version: 8 -->\ndelta instructions",
        );
        fixture.write_agents("User text.\n");
        for tool in ["alpha", "beta", "delta"] {
            fixture.setup(tool);
        }
        fixture.refresh(&["alpha", "beta", "delta"]);
        let enabled = fixture.agents();
        assert_eq!(
            enabled,
            format!(
                "User text.\n\n{}\n\n{}\n\n{delta}\n",
                per_tool("alpha", SHARED_BODY),
                per_tool("beta", SHARED_BODY)
            )
        );

        // Disable delta: its per-tool section leaves, and the identical sharers now share.
        fixture.refresh(&["alpha", "beta"]);
        let without_delta = format!("User text.\n\n{}\n", shared("alpha, beta"));
        assert_eq!(fixture.agents(), without_delta);
        fixture.refresh(&["alpha", "beta"]);
        assert_eq!(
            fixture.agents(),
            without_delta,
            "a repeated refresh is a byte-level no-op"
        );

        // Disabling everything leaves only the user's content.
        fixture.refresh(&[]);
        assert_eq!(fixture.agents(), "User text.\n");
        fixture.refresh(&[]);
        assert_eq!(fixture.agents(), "User text.\n");

        // Re-enabling (their skills mark them as previously managed) restores guidance.
        fixture.refresh(&["delta"]);
        assert_eq!(fixture.agents(), format!("User text.\n\n{delta}\n"));
        fixture.refresh(&["alpha", "beta", "delta"]);
        let restored = fixture.agents();
        assert!(restored.contains(&delta), "{restored}");
        assert!(
            restored.contains(&per_tool("alpha", SHARED_BODY)),
            "{restored}"
        );
        assert!(
            restored.contains(&per_tool("beta", SHARED_BODY)),
            "{restored}"
        );
        fixture.refresh(&["alpha", "beta", "delta"]);
        assert_eq!(fixture.agents(), restored);
    }

    #[test]
    fn disabled_tool_sections_written_by_a_newer_writer_or_customized_are_preserved() {
        let fixture = Sharing::new();
        sharing_tool(fixture.plugins.path(), "delta", "delta instructions\n", 8);
        let newer = per_tool("delta", "<!-- hotsheet-instructions-version: 9 -->\nnewer");
        let customized = per_tool(
            "delta",
            "<!-- hotsheet-instructions-version: 8 -->\nproject-edited",
        );
        for installed in [&newer, &customized] {
            let contents = format!("User text.\n\n{installed}\n");
            fixture.write_agents(&contents);
            fixture.refresh(&["alpha"]);
            assert_eq!(fixture.agents(), contents, "{installed}");
        }
        // An older section of a disabled tool is Hot Sheet's to remove.
        fixture.write_agents(&format!(
            "User text.\n\n{}\n\nFooter.\n",
            per_tool("delta", "<!-- hotsheet-instructions-version: 3 -->\nolder")
        ));
        fixture.refresh(&["alpha"]);
        assert_eq!(fixture.agents(), "User text.\n\nFooter.\n");
    }

    #[test]
    fn without_an_enabled_list_refresh_never_removes_a_per_tool_section() {
        let fixture = Sharing::new();
        let contents = format!("User text.\n\n{}\n", per_tool("alpha", "stale"));
        fixture.write_agents(&contents);
        refresh_setup_in(
            fixture.store.path(),
            fixture.project.path(),
            None,
            &[fixture.plugins.path().to_path_buf()],
        )
        .unwrap();
        let refreshed = fixture.agents();
        assert!(
            refreshed.contains(&per_tool("alpha", SHARED_BODY)),
            "{refreshed}"
        );
    }

    #[test]
    fn mixed_shared_and_per_tool_sections_converge_to_one_shared_section() {
        let fixture = Sharing::new();
        fixture.write_agents(&format!(
            "{}\n\nUser text.\n\n{}\n\n{}\n\n{}\n",
            per_tool("beta", "stale beta"),
            shared("alpha"),
            per_tool("gamma", "stale gamma"),
            // A duplicated per-tool copy (e.g. from a hand merge) is migrated too.
            per_tool("gamma", "older gamma"),
        ));
        fixture.refresh(&["alpha", "beta", "gamma"]);
        assert_eq!(
            fixture.agents(),
            format!("User text.\n\n{}\n", shared("alpha, beta, gamma"))
        );
    }

    #[test]
    fn explicit_setup_joins_an_existing_shared_section_without_dropping_members() {
        let fixture = Sharing::new();
        fixture.setup("alpha");
        assert_eq!(
            fixture.agents(),
            format!("{}\n", per_tool("alpha", SHARED_BODY))
        );

        fixture.write_agents(&format!("User text.\n\n{}\n", shared("beta, gamma")));
        fixture.setup("alpha");
        assert_eq!(
            fixture.agents(),
            format!("User text.\n\n{}\n", shared("alpha, beta, gamma"))
        );
        // Explicit setup of a member again is a no-op; it never removes the others.
        fixture.setup("beta");
        assert_eq!(
            fixture.agents(),
            format!("User text.\n\n{}\n", shared("alpha, beta, gamma"))
        );
    }

    #[test]
    fn a_newer_installed_shared_section_is_preserved_for_every_sharer() {
        let fixture = Sharing::new();
        let installed = "User text.\n\n<!-- BEGIN hotsheet:agents-md -->\n<!-- hotsheet-shared-section: alpha, beta -->\n<!-- hotsheet-instructions-version: 9 -->\nnewer shared instructions\n<!-- END hotsheet:agents-md -->\n";
        fixture.write_agents(installed);
        // Stale per-tool copy for a sharer does not unfreeze the newer shared section.
        let with_stale = format!("{installed}\n{}\n", per_tool("gamma", "stale"));
        fixture.write_agents(&with_stale);

        let reports = fixture.refresh(&["alpha", "beta", "gamma"]);
        assert_eq!(fixture.agents(), with_stale);
        for report in &reports {
            assert_eq!(report.wrote.len(), 1, "only MCP maintenance: {report:?}");
        }
        for id in ["alpha", "beta", "gamma"] {
            assert!(
                !fixture
                    .project
                    .path()
                    .join(format!(".{id}/SKILL.md"))
                    .exists()
            );
        }
    }

    #[test]
    fn equal_version_shared_sections_preserve_customized_bodies_but_not_membership_changes() {
        let fixture = Sharing::new();
        let customized = "<!-- BEGIN hotsheet:agents-md -->\n<!-- hotsheet-shared-section: alpha, beta, gamma -->\n<!-- hotsheet-instructions-version: 8 -->\nproject-customized instructions\n<!-- END hotsheet:agents-md -->\n";
        fixture.write_agents(customized);
        fixture.refresh(&["alpha", "beta", "gamma"]);
        assert_eq!(fixture.agents(), customized);

        // Same body, only the served-list line differs (unsorted, stale spacing): the list is
        // membership, not content, so the section is ours to normalize.
        fixture.write_agents(&format!("{}\n", shared("gamma,alpha")));
        fixture.refresh(&["alpha", "beta", "gamma"]);
        assert_eq!(fixture.agents(), format!("{}\n", shared("alpha, gamma")));
    }

    #[test]
    fn an_older_shared_section_upgrades_in_place() {
        let fixture = Sharing::new();
        fixture.write_agents("Top.\n\n<!-- BEGIN hotsheet:agents-md -->\n<!-- hotsheet-shared-section: alpha, beta -->\n<!-- hotsheet-instructions-version: 7 -->\nolder\n<!-- END hotsheet:agents-md -->\n\nBottom.\n");
        fixture.refresh(&["alpha", "beta"]);
        assert_eq!(
            fixture.agents(),
            format!("Top.\n\n{}\n\nBottom.\n", shared("alpha, beta"))
        );
    }

    #[test]
    fn reconciling_without_sharers_never_creates_or_rewrites_unrelated_files() {
        let fixture = Sharing::new();
        assert!(fixture.refresh(&["unrelated"]).is_empty());
        assert!(!fixture.project.path().join("AGENTS.md").exists());
        fixture.write_agents("Only user text.\n");
        assert!(fixture.refresh(&["unrelated"]).is_empty());
        assert_eq!(fixture.agents(), "Only user text.\n");
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
            std::fs::read_to_string(project.path().join(".fixture/skills/hotsheet/SKILL.md"))
                .unwrap(),
            "<!-- hotsheet-skill-version: 1 -->\ncurrent skill\n"
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
    fn stale_setup_preserves_a_newer_installed_workflow_as_one_bundle() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let newer_plugins = tempfile::tempdir().unwrap();
        let stale_plugins = tempfile::tempdir().unwrap();
        fixture_plugin_version(newer_plugins.path(), "newer instructions\n", 9);
        fixture_plugin_version(stale_plugins.path(), "stale instructions\n", 8);

        run_setup_in(
            store.path(),
            project.path(),
            Some("fixture"),
            false,
            None,
            &[newer_plugins.path().to_path_buf()],
        )
        .unwrap();
        let instructions = std::fs::read(project.path().join("AGENTS.md")).unwrap();
        let skill =
            std::fs::read(project.path().join(".fixture/skills/hotsheet/SKILL.md")).unwrap();

        let report = run_setup_in(
            store.path(),
            project.path(),
            Some("fixture"),
            false,
            None,
            &[stale_plugins.path().to_path_buf()],
        )
        .unwrap();

        assert_eq!(
            std::fs::read(project.path().join("AGENTS.md")).unwrap(),
            instructions
        );
        assert_eq!(
            std::fs::read(project.path().join(".fixture/skills/hotsheet/SKILL.md")).unwrap(),
            skill
        );
        assert_eq!(report[0].wrote, [".fixture/mcp.json"]);
    }

    #[test]
    fn either_newer_installed_artifact_protects_mixed_workflow_versions() {
        let store = tempfile::tempdir().unwrap();
        let plugins = tempfile::tempdir().unwrap();
        fixture_plugin_version(plugins.path(), "bundled instructions\n", 8);

        for newer_artifact in ["instructions", "skill"] {
            let project = tempfile::tempdir().unwrap();
            let instruction_version = if newer_artifact == "instructions" {
                9
            } else {
                7
            };
            let skill_version = if newer_artifact == "skill" { 9 } else { 7 };
            let instructions = format!(
                "<!-- BEGIN hotsheet:fixture -->\n<!-- hotsheet-instructions-version: {instruction_version} -->\ninstalled instructions\n<!-- END hotsheet:fixture -->\n"
            );
            let skill =
                format!("<!-- hotsheet-skill-version: {skill_version} -->\ninstalled skill\n");
            std::fs::write(project.path().join("AGENTS.md"), &instructions).unwrap();
            let skill_path = project.path().join(".fixture/skills/hotsheet/SKILL.md");
            std::fs::create_dir_all(skill_path.parent().unwrap()).unwrap();
            std::fs::write(&skill_path, &skill).unwrap();

            run_setup_in(
                store.path(),
                project.path(),
                Some("fixture"),
                false,
                None,
                &[plugins.path().to_path_buf()],
            )
            .unwrap();

            assert_eq!(
                std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap(),
                instructions
            );
            assert_eq!(std::fs::read_to_string(skill_path).unwrap(), skill);
        }
    }

    #[test]
    fn either_divergent_equal_version_artifact_protects_the_installed_workflow() {
        let store = tempfile::tempdir().unwrap();
        let plugins = tempfile::tempdir().unwrap();
        fixture_plugin_version(plugins.path(), "bundled instructions\n", 8);

        for customized_artifact in ["instructions", "skill"] {
            let project = tempfile::tempdir().unwrap();
            let instructions = format!(
                "<!-- BEGIN hotsheet:fixture -->\n<!-- hotsheet-instructions-version: 8 -->\n{}\n<!-- END hotsheet:fixture -->\n",
                if customized_artifact == "instructions" {
                    "project-specific instructions"
                } else {
                    "bundled instructions"
                }
            );
            let skill = format!(
                "<!-- hotsheet-skill-version: 8 -->\n{}\n",
                if customized_artifact == "skill" {
                    "project-specific skill"
                } else {
                    "current skill"
                }
            );
            std::fs::write(project.path().join("AGENTS.md"), &instructions).unwrap();
            let skill_path = project.path().join(".fixture/skills/hotsheet/SKILL.md");
            std::fs::create_dir_all(skill_path.parent().unwrap()).unwrap();
            std::fs::write(&skill_path, &skill).unwrap();

            let report = refresh_setup_in(
                store.path(),
                project.path(),
                Some(&HashSet::from(["fixture".to_string()])),
                &[plugins.path().to_path_buf()],
            )
            .unwrap();

            assert_eq!(
                std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap(),
                instructions
            );
            assert_eq!(std::fs::read_to_string(skill_path).unwrap(), skill);
            assert_eq!(report[0].wrote, [".fixture/mcp.json"]);
        }
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
    fn bundled_tool_skills_match_their_canonical_shared_workflows() {
        fn version(contents: &str) -> &str {
            contents
                .lines()
                .find_map(|line| line.strip_prefix("<!-- hotsheet-skill-version: "))
                .and_then(|line| line.strip_suffix(" -->"))
                .expect("skill version marker")
        }
        fn without_claude_tool_metadata(contents: &str) -> String {
            contents
                .lines()
                .filter(|line| !line.starts_with("allowed-tools:"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        let bundled_claude = include_str!("../../../plugins/claude/SKILL.md");
        let bundled_codex = include_str!("../../../plugins/codex/SKILL.md");
        let canonical_claude = include_str!("../../../.claude/skills/hotsheet/SKILL.md");
        let canonical_codex = include_str!("../../../.agents/skills/hotsheet/SKILL.md");

        assert_eq!(bundled_claude, canonical_claude);
        assert_eq!(bundled_codex, canonical_codex);
        assert_eq!(
            without_claude_tool_metadata(bundled_claude),
            without_claude_tool_metadata(canonical_codex)
        );
        assert_eq!(version(bundled_claude), version(bundled_codex));
    }

    #[test]
    fn every_plugin_shares_one_instruction_body() {
        // The Hot Sheet instruction block is a single shared source: plugins/claude/instructions.md
        // is canonical and every other tool's instructions.md must match it byte-for-byte, so the
        // four cannot drift again (HS2-829W59; re-converged in HS2-3JMMAZ). Mirrors the SKILL.md
        // sync test above. If a tool ever needs a genuine per-tool variation, strip that line here
        // as the SKILL.md test does for `allowed-tools:` — never fork the whole body.
        let canonical = include_str!("../../../plugins/claude/instructions.md");
        for (id, body) in [
            (
                "codex",
                include_str!("../../../plugins/codex/instructions.md"),
            ),
            (
                "antigravity",
                include_str!("../../../plugins/antigravity/instructions.md"),
            ),
            (
                "opencode",
                include_str!("../../../plugins/opencode/instructions.md"),
            ),
        ] {
            assert_eq!(
                canonical, body,
                "[{id}] instructions.md has drifted from the canonical plugins/claude/instructions.md; edit the canonical and re-copy so the shared instruction body stays identical"
            );
        }
    }
}
