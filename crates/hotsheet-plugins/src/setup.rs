//! **Core-owned AI-tool setup** (`docs/05` §5.1a, `docs/04` §4.1, HS2-91). Preparing a
//! project for an AI tool — the managed instruction section, the worklist skill, and the
//! per-tool MCP-config entry — is a **one-shot, host-agnostic** operation, so it lives here
//! in the core plugins crate and is driven identically by the **CLI** (headless: no server,
//! no client) and the **server** (`POST /setup/<tool>` for client-driven flows). One impl,
//! no drift (`docs/04` §4.5).
//!
//! The `enabled` set (which plugins a project has opted into — `docs/05` settings) is passed
//! **in** by the caller. This crate reads only the shared `push_policy` setting itself so the
//! CLI and server render the same policy-specific managed workflow.

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use hotsheet_ticketing::settings::{Scope, Settings, SettingsError};

use crate::{Plugin, all_plugins, default_dirs};

pub const PUSH_POLICY_SETTING: &str = "push_policy";
const SHARED_SETTINGS_PATH: &str = ".hotsheet2/settings.json";
const POLICY_BEGIN: &str = "<!-- hotsheet:push-policy:begin -->";
const POLICY_END: &str = "<!-- hotsheet:push-policy:end -->";
const LEGACY_POLICY: &str = "**Pushing is up to this repository.** Follow whatever push/PR/review conventions this project\nalready uses; this default guidance does not require or forbid pushing on its own.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushPolicy {
    PerTicket,
    PerBatch,
}

impl PushPolicy {
    fn parse(value: &serde_json::Value) -> Option<Self> {
        match value.as_str()? {
            "push-per-ticket" => Some(Self::PerTicket),
            "push-per-batch" => Some(Self::PerBatch),
            _ => None,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::PerTicket => "push-per-ticket",
            Self::PerBatch => "push-per-batch",
        }
    }

    fn wording(self) -> &'static str {
        match self {
            Self::PerTicket => {
                "Push policy: `push-per-ticket`. The maintainer authorizes routine fast-forward\npushes of verified ticket commits to the shipping branch. Push each ticket\ncommit immediately, confirm it succeeded, then begin the next ticket."
            }
            Self::PerBatch => {
                "Push policy: `push-per-batch`. The maintainer authorizes routine fast-forward\npushes of verified ticket commits to the shipping branch. Hold verified commits\nlocally until the queue drain or an earlier CI need, then push and confirm before\nending the session. Record the measured reason beside the project declaration."
            }
        }
    }

    fn skill_wording(self) -> String {
        self.wording().replace('\n', "\n   ")
    }
}

fn shared_settings(
    fs: &PlanFs,
    project: &Path,
) -> Result<serde_json::Map<String, serde_json::Value>, SetupError> {
    let path = project.join(SHARED_SETTINGS_PATH);
    let contents = match fs.read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(serde_json::Map::from_iter([(
                "$hotsheetSchema".into(),
                serde_json::json!(1),
            )]));
        }
        Err(source) => {
            return Err(SetupError::Io {
                path: path.display().to_string(),
                source,
            });
        }
    };
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&contents)
        .map_err(|_| SetupError::InvalidPushPolicy(SHARED_SETTINGS_PATH.into()))
}

/// Missing declarations are written by setup and reported by read-only diagnosis.
fn ensure_push_policy(fs: &mut PlanFs, project: &Path) -> Result<PushPolicy, SetupError> {
    let mut settings = shared_settings(fs, project)?;
    if let Some(value) = settings.get(PUSH_POLICY_SETTING) {
        return PushPolicy::parse(value)
            .ok_or_else(|| SetupError::InvalidPushPolicy(SHARED_SETTINGS_PATH.into()));
    }
    let policy = PushPolicy::PerTicket;
    settings.insert(
        PUSH_POLICY_SETTING.into(),
        serde_json::json!(policy.value()),
    );
    let rendered =
        serde_json::to_string_pretty(&settings).expect("settings object serializes") + "\n";
    fs.write(&project.join(SHARED_SETTINGS_PATH), rendered.as_bytes());
    Ok(policy)
}

fn read_push_policy(fs: &PlanFs, project: &Path) -> Result<PushPolicy, SetupError> {
    let settings = shared_settings(fs, project)?;
    settings
        .get(PUSH_POLICY_SETTING)
        .map(|value| {
            PushPolicy::parse(value)
                .ok_or_else(|| SetupError::InvalidPushPolicy(SHARED_SETTINGS_PATH.into()))
        })
        .unwrap_or(Ok(PushPolicy::PerTicket))
}

/// What one tool's setup wrote (project-relative paths), for reporting.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetupReport {
    pub tool: String,
    pub wrote: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<SetupNotice>,
}

/// A successful setup change that requires the user's attention (HS2-4AR09Z).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetupNotice {
    CodexHooksChanged {
        path: String,
    },
    BundledGuidanceBlocked {
        path: String,
        section: String,
        installed_version: u64,
        bundled_version: u64,
    },
}

impl SetupNotice {
    pub fn message(&self) -> String {
        match self {
            Self::CodexHooksChanged { path } => format!(
                "Codex hooks changed in {path}. Run /hooks in Codex for this checkout to review and trust the updated hooks."
            ),
            Self::BundledGuidanceBlocked {
                path,
                section,
                installed_version,
                bundled_version,
            } => format!(
                "Bundled guidance section {section} v{bundled_version} was preserved in {path} because its local v{installed_version} body differs. Review the section and setup diagnosis before editing it."
            ),
        }
    }
}

/// What a refresh changed (HS2-CAM9J5): the tools it set up or repaired, and the managed
/// artifacts it removed from tools the project disabled.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RefreshReport {
    /// Every enabled tool the refresh set up (detected or previously managed).
    pub set_up: Vec<SetupReport>,
    /// Disabled tools whose managed artifacts the refresh removed; only tools with at
    /// least one change are listed.
    pub removed: Vec<RemovalReport>,
}

/// A read-only preview of the same setup work that refresh will apply.
pub struct SetupPlan {
    pub report: RefreshReport,
    pub changes: Vec<PlannedChange>,
    pub tools: Vec<ToolSetupFindings>,
    operations: Vec<PlannedOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlannedChange {
    /// Project-relative, except a machine-local Git exclude file outside the project.
    pub path: String,
    pub action: PlannedAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedAction {
    Write,
    Remove,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolSetupFindings {
    pub tool: String,
    pub enabled: bool,
    pub detected: bool,
    pub previously_managed: bool,
    pub artifacts: Vec<ArtifactFinding>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ArtifactFinding {
    pub kind: ArtifactKind,
    pub path: String,
    pub status: ArtifactStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Instructions,
    Skill,
    Mcp,
    Hooks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    Current,
    Missing,
    Outdated,
    Preserved,
    Disabled,
}

enum PlannedOperation {
    Write(PathBuf, Vec<u8>),
    Remove(PathBuf, PathBuf),
}

#[derive(Default)]
struct PlanFs {
    original: BTreeMap<PathBuf, Option<Vec<u8>>>,
    current: BTreeMap<PathBuf, Option<Vec<u8>>>,
}

impl PlanFs {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        match self.current.get(path) {
            Some(Some(bytes)) => Ok(bytes.clone()),
            Some(None) => Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            None => std::fs::read(path),
        }
    }

    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        String::from_utf8(self.read(path)?)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    fn is_file(&self, path: &Path) -> bool {
        self.current
            .get(path)
            .map_or_else(|| path.is_file(), Option::is_some)
    }

    fn exists(&self, path: &Path) -> bool {
        self.current
            .get(path)
            .map_or_else(|| path.exists(), Option::is_some)
    }

    fn remember(&mut self, path: &Path) {
        self.original
            .entry(path.to_path_buf())
            .or_insert_with(|| std::fs::read(path).ok());
    }

    fn write(&mut self, path: &Path, contents: &[u8]) {
        self.remember(path);
        if self.read(path).ok().as_deref() == Some(contents) {
            return;
        }
        self.current
            .insert(path.to_path_buf(), Some(contents.to_vec()));
    }

    fn remove(&mut self, _project: &Path, path: &Path) -> bool {
        if !self.is_file(path) {
            return false;
        }
        self.remember(path);
        self.current.insert(path.to_path_buf(), None);
        true
    }

    fn final_operations(&self, project: &Path) -> Vec<PlannedOperation> {
        self.current
            .iter()
            .filter_map(|(path, current)| {
                if self.original.get(path) == Some(current) {
                    return None;
                }
                Some(match current {
                    Some(contents) => PlannedOperation::Write(path.clone(), contents.clone()),
                    None => PlannedOperation::Remove(project.to_path_buf(), path.clone()),
                })
            })
            .collect()
    }

    fn changes(&self, project: &Path) -> Vec<PlannedChange> {
        self.current
            .iter()
            .filter_map(|(path, current)| {
                let original = self.original.get(path)?;
                if original == current {
                    return None;
                }
                let label = path
                    .strip_prefix(project)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned();
                Some(PlannedChange {
                    path: label,
                    action: if current.is_some() {
                        PlannedAction::Write
                    } else {
                        PlannedAction::Remove
                    },
                })
            })
            .collect()
    }
}

impl RefreshReport {
    /// Whether the refresh set nothing up and removed nothing.
    pub fn is_empty(&self) -> bool {
        self.set_up.is_empty() && self.removed.is_empty()
    }
}

/// The project-relative paths one disabled tool's refresh changed (HS2-CAM9J5).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct RemovalReport {
    pub tool: String,
    /// Files deleted outright (a Hot Sheet-owned skill, or a config left holding nothing).
    pub removed: Vec<String>,
    /// Files kept but edited to drop Hot Sheet's section, MCP entry, or hooks.
    pub edited: Vec<String>,
}

impl RemovalReport {
    fn record(&mut self, rel: &str, change: Option<Change>) {
        let list = match change {
            Some(Change::Removed) => &mut self.removed,
            Some(Change::Edited) => &mut self.edited,
            None => return,
        };
        if !list.iter().any(|path| path == rel) {
            list.push(rel.to_string());
        }
    }
}

/// How a removal step changed one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Removed,
    Edited,
}

/// A setup failure.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("{source}\n{review}")]
    Partial {
        source: Box<SetupError>,
        review: String,
    },
    #[error("unknown tool '{0}' (no such plugin)")]
    UnknownTool(String),
    #[error("specify a tool (e.g. `setup claude`) or pass detect=true")]
    NoToolGiven,
    #[error("no supported AI tools detected on this machine")]
    NoneDetected,
    #[error(
        "no detected AI tool is enabled for this project (its `enabled_plugins` setting excludes them all)"
    )]
    NoneEnabled,
    #[error("store path does not exist: {0} (run `init` first)")]
    NoStore(String),
    #[error("plugin '{id}' declares unsafe target path(s): {targets} (must be project-relative)")]
    UnsafeTargets { id: String, targets: String },
    #[error(
        "invalid guidance ownership in {0}: expected schema_version 1 and owner builtin or rulesync"
    )]
    InvalidGuidanceOwner(String),
    #[error("invalid push_policy in {0}: expected push-per-ticket or push-per-batch")]
    InvalidPushPolicy(String),
    #[error("push_policy changed since setup was planned; rerun setup to use the current policy")]
    StalePushPolicy,
    #[error("settings transaction: {0}")]
    Settings(#[from] SettingsError),
    #[error("unknown MCP config format '{format}' for plugin '{id}'")]
    UnknownMcpFormat { id: String, format: String },
    #[error("unknown hook config format '{format}' for plugin '{id}'")]
    UnknownHookFormat { id: String, format: String },
    #[error("guidance sources: {0}")]
    GuidanceSource(#[from] crate::guidance_sources::GuidanceSourceError),
    #[error("writing {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

impl SetupError {
    fn with_notices(self, reports: &[SetupReport]) -> Self {
        let review = reports
            .iter()
            .flat_map(|report| &report.notices)
            .map(SetupNotice::message)
            .collect::<Vec<_>>()
            .join("\n");
        if review.is_empty() {
            self
        } else {
            Self::Partial {
                source: Box::new(self),
                review,
            }
        }
    }
}

pub const GUIDANCE_OWNER_PATH: &str = ".hotsheet2/guidance-owner.json";
const LOCAL_RULE_PATH: &str = ".rulesync/rules/project-local.md";

/// Keep prose outside managed sections, plus project-owned nested specifics, before
/// Rulesync replaces AGENTS.md. Refuse incomplete markers rather than lose their body.
fn local_guidance_from_agents(contents: &str) -> Result<String, SetupError> {
    let mut local = String::new();
    let mut specifics = String::new();
    let mut section_end: Option<String> = None;
    let mut specifics_end: Option<String> = None;
    let mut tool_end: Option<String> = None;
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(end) = &specifics_end {
            if trimmed == end {
                specifics_end = None;
            } else {
                specifics.push_str(line);
                specifics.push('\n');
            }
            continue;
        }
        if let Some(end) = &section_end {
            if trimmed == end {
                section_end = None;
            } else if let Some(name) = trimmed
                .strip_prefix("<!-- hotsheet:begin specifics=")
                .and_then(|rest| rest.split_once(" v="))
                .map(|(name, _)| name)
            {
                specifics_end = Some(format!("<!-- hotsheet:end specifics={name} -->"));
            }
            continue;
        }
        if let Some(end) = &tool_end {
            if trimmed == end {
                tool_end = None;
            }
            continue;
        }
        if let Some(name) = trimmed
            .strip_prefix("<!-- hotsheet:begin section=")
            .and_then(|rest| rest.split_once(" v="))
            .map(|(name, _)| name)
        {
            section_end = Some(format!("<!-- hotsheet:end section={name} -->"));
        } else if let Some(name) = trimmed
            .strip_prefix("<!-- BEGIN hotsheet:")
            .and_then(|rest| rest.strip_suffix(" -->"))
        {
            tool_end = Some(format!("<!-- END hotsheet:{name} -->"));
        } else if !trimmed.starts_with("<!-- hotsheet-instructions-version:") {
            local.push_str(line);
            local.push('\n');
        }
    }
    if section_end.is_some() || specifics_end.is_some() || tool_end.is_some() {
        return Err(SetupError::GuidanceSource(
            crate::guidance_sources::GuidanceSourceError::Invalid(
                "AGENTS.md has an incomplete managed instruction marker; local guidance was not moved".into(),
            ),
        ));
    }
    let local = local.trim();
    let specifics = specifics.trim();
    Ok(match (local.is_empty(), specifics.is_empty()) {
        (true, true) => String::new(),
        (false, true) => local.to_owned(),
        (true, false) => specifics.to_owned(),
        (false, false) => format!("{local}\n\n{specifics}"),
    })
}

fn preserve_local_guidance_for_rulesync(project: &Path) -> Result<(), SetupError> {
    let agents = project.join("AGENTS.md");
    let contents = match std::fs::read_to_string(&agents) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(SetupError::Io {
                path: agents.display().to_string(),
                source,
            });
        }
    };
    let body = local_guidance_from_agents(&contents)?;
    if body.is_empty() {
        return Ok(());
    }
    let path = project.join(LOCAL_RULE_PATH);
    let rule = format!(
        "---\nroot: true\ntargets: [\"*\"]\ndescription: \"Project-local guidance preserved from AGENTS.md\"\n---\n\n{body}\n"
    );
    if path.exists() {
        let existing = std::fs::read_to_string(&path).map_err(|source| SetupError::Io {
            path: path.display().to_string(),
            source,
        })?;
        if existing == rule {
            return Ok(());
        }
        return Err(SetupError::GuidanceSource(
            crate::guidance_sources::GuidanceSourceError::Invalid(format!(
                "{LOCAL_RULE_PATH} already differs; review it before switching instruction ownership"
            )),
        ));
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|source| SetupError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| SetupError::Io {
            path: path.display().to_string(),
            source,
        })?;
    file.write_all(rule.as_bytes())
        .map_err(|source| SetupError::Io {
            path: path.display().to_string(),
            source,
        })
}

/// Names of project-authored Rulesync rule files. The generated `.curated` cache is
/// deliberately excluded; the picker shows these as read-only local rules.
pub fn local_rules_in(project: &Path) -> Result<Vec<String>, SetupError> {
    let directory = project.join(".rulesync/rules");
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(SetupError::Io {
                path: directory.display().to_string(),
                source,
            });
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SetupError::Io {
            path: directory.display().to_string(),
            source,
        })?;
        let kind = entry.file_type().map_err(|source| SetupError::Io {
            path: entry.path().display().to_string(),
            source,
        })?;
        let path = entry.path();
        if kind.is_file()
            && path.extension().is_some_and(|extension| extension == "md")
            && let Some(name) = path.file_stem().and_then(|name| name.to_str())
            && !name.starts_with('.')
        {
            names.push(name.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// Explicit project ownership; absent markers retain the built-in offline writer.
fn rulesync_owns_guidance(fs: &PlanFs, project: &Path) -> Result<bool, SetupError> {
    let target = project.join(GUIDANCE_OWNER_PATH);
    let text = match fs.read_to_string(&target) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(SetupError::Io {
                path: target.display().to_string(),
                source,
            });
        }
    };
    let invalid = || SetupError::InvalidGuidanceOwner(GUIDANCE_OWNER_PATH.into());
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| invalid())?;
    if value["schema_version"] != 1 {
        return Err(invalid());
    }
    match value["owner"].as_str() {
        Some("builtin") => Ok(false),
        Some("rulesync") => Ok(true),
        _ => Err(invalid()),
    }
}

/// The selected project writer, shared by CLI and authenticated Settings routes.
pub fn guidance_owner_in(project: &Path) -> Result<&'static str, SetupError> {
    Ok(if rulesync_owns_guidance(&PlanFs::default(), project)? {
        "rulesync"
    } else {
        "builtin"
    })
}

/// Read-only preview of the exact pinned rule input and the section an explicit
/// selection would change. The built-in path uses the same renderer as apply.
#[derive(Debug, serde::Serialize)]
pub struct GuidanceRulePreview {
    pub owner: &'static str,
    pub current: Option<String>,
    pub pinned: String,
    pub after: Option<String>,
    pub outcome: &'static str,
}

pub fn preview_guidance_rule_in(
    project: &Path,
    source_id: &str,
    rule_name: &str,
) -> Result<GuidanceRulePreview, SetupError> {
    let mut config = crate::guidance_sources::load(project)?;
    let source = config
        .sources
        .iter_mut()
        .find(|source| source.id == source_id)
        .ok_or_else(|| {
            SetupError::GuidanceSource(crate::guidance_sources::GuidanceSourceError::Invalid(
                "unknown guidance source".into(),
            ))
        })?;
    let rule = source
        .rules
        .iter()
        .find(|rule| rule.name == rule_name)
        .ok_or_else(|| {
            SetupError::GuidanceSource(crate::guidance_sources::GuidanceSourceError::Invalid(
                "unknown guidance rule".into(),
            ))
        })?;
    let choice = crate::guidance_sources::chosen_rule(source, rule);
    crate::guidance_sources::require_compatible(&choice)?;
    let pinned = choice.body.to_owned();
    let known = rule.known_bodies.clone();
    if !source.selected_rules.iter().any(|name| name == rule_name) {
        source.selected_rules.push(rule_name.to_owned());
    }
    config.explicit_selection = true;
    let owner = guidance_owner_in(project)?;
    let (current, after) = if owner == "rulesync" {
        let path = project
            .join(".rulesync/rules/.curated")
            .join(format!("{rule_name}.md"));
        let current = match std::fs::read_to_string(path) {
            Ok(text) => Some(
                crate::guidance_sources::split_rule_body(&text)
                    .map_or(text.clone(), |(body, _)| body.to_owned()),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(crate::guidance_sources::GuidanceSourceError::Io(error).into());
            }
        };
        let replaceable = current
            .as_ref()
            .is_none_or(|body| body == &pinned || known.contains(body));
        let after = if replaceable {
            Some(pinned.clone())
        } else {
            current.clone()
        };
        (current, after)
    } else {
        let existing = match std::fs::read_to_string(project.join("AGENTS.md")) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(crate::guidance_sources::GuidanceSourceError::Io(error).into());
            }
        };
        let section = rule_name.strip_prefix("hotsheet-").unwrap_or(rule_name);
        let extract = |text: &str| {
            if rule_name == "hotsheet-workflow" {
                parse_shared_section(text, "agents-md").map(|block| block.body)
            } else {
                guidance_bounds(text, section).map(|(start, end)| text[start..end].to_owned())
            }
        };
        let current = extract(&existing);
        let rendered =
            render_selected_guidance(&render_guidance_sections(&existing, true), &config, true);
        let after = extract(&rendered);
        (current, after)
    };
    let outcome = if current == after {
        if current
            .as_deref()
            .is_some_and(|body| body.trim() == pinned.trim())
        {
            "current"
        } else {
            "protected"
        }
    } else if current.is_none() {
        "add"
    } else {
        "replace"
    };
    Ok(GuidanceRulePreview {
        owner,
        current,
        pinned,
        after,
        outcome,
    })
}

/// Reconcile selected rules and skills without requiring a Git ticket store or an AI tool
/// launch. The normal setup planner calls these same writers on project open.
pub fn apply_selected_guidance_in(project: &Path) -> Result<(), SetupError> {
    let config = crate::guidance_sources::load(project)?;
    for source in &config.sources {
        for rule in source
            .rules
            .iter()
            .filter(|rule| source.selected_rules.contains(&rule.name))
        {
            crate::guidance_sources::require_compatible(&crate::guidance_sources::chosen_rule(
                source, rule,
            ))?;
        }
    }
    let mut fs = PlanFs::default();
    if rulesync_owns_guidance(&fs, project)? {
        return Err(SetupError::GuidanceSource(
            crate::guidance_sources::GuidanceSourceError::Invalid(
                "Rulesync owns this project's instructions; use the Rulesync apply path".into(),
            ),
        ));
    }
    write_guidance_target(&mut fs, project, "AGENTS.md", true)?;
    let skill_plugins = all_plugins(&default_dirs())
        .into_iter()
        .filter(is_detected)
        .collect::<Vec<_>>();
    write_selected_source_skills(&mut fs, project, &skill_plugins)?;
    apply_operations(fs.final_operations(project))
}

/// Install selected project skills for detected tools without touching instruction ownership.
pub fn apply_selected_skills_in(project: &Path) -> Result<(), SetupError> {
    let mut fs = PlanFs::default();
    let skill_plugins = all_plugins(&default_dirs())
        .into_iter()
        .filter(is_detected)
        .collect::<Vec<_>>();
    write_selected_source_skills(&mut fs, project, &skill_plugins)?;
    apply_operations(fs.final_operations(project))
}

/// Select the instruction writer, preserving unmarked local prose before Rulesync owns it.
pub fn set_guidance_owner_in(project: &Path, owner: &str) -> Result<(), SetupError> {
    if !matches!(owner, "builtin" | "rulesync") {
        return Err(SetupError::InvalidGuidanceOwner(GUIDANCE_OWNER_PATH.into()));
    }
    if owner == "rulesync" && !rulesync_owns_guidance(&PlanFs::default(), project)? {
        preserve_local_guidance_for_rulesync(project)?;
    }
    let path = project.join(GUIDANCE_OWNER_PATH);
    let text =
        serde_json::to_string_pretty(&serde_json::json!({"schema_version": 1, "owner": owner}))
            .unwrap()
            + "\n";
    let mut fs = PlanFs::default();
    fs.write(&path, text.as_bytes());
    apply_operations(fs.final_operations(project))
}

/// Resolve the project's `enabled_plugins` shared setting (HS2-94) into the `enabled` set
/// every setup entry point takes, so the CLI and the server interpret it identically
/// (HS2-8B3VJP):
///
/// * unset (`None`) or JSON `null` → `None`: no restriction, refresh never removes anything;
/// * an array → `Some` of its string entries. An **explicit empty list means no tool is
///   enabled**, so refresh removes every tool's managed artifacts; non-string entries are
///   ignored;
/// * any other (malformed) value → `None`, so a corrupt setting never deletes setup.
pub fn enabled_plugins_from_setting(value: Option<&serde_json::Value>) -> Option<HashSet<String>> {
    let values = value?.as_array()?;
    Some(
        values
            .iter()
            .filter_map(|value| value.as_str().map(String::from))
            .collect(),
    )
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
        (None, true) => {
            let detected: Vec<Plugin> = all_plugins(plugin_dirs)
                .into_iter()
                .filter(is_detected)
                .collect();
            if detected.is_empty() {
                return Err(SetupError::NoneDetected);
            }
            let enabled: Vec<Plugin> = detected
                .into_iter()
                .filter(|p| enabled.is_none_or(|set| set.contains(p.id())))
                .collect();
            if enabled.is_empty() {
                return Err(SetupError::NoneEnabled);
            }
            enabled
        }
        (None, false) => return Err(SetupError::NoToolGiven),
    };

    setup_plugins(store_path, project_dir, plugins, None)
}

/// Refresh every tool that is currently detected or already has a Hot Sheet managed
/// instruction block. This is the idempotent project-open/startup path: it repairs partial
/// setup and updates bundled assets without requiring the client to know tool-specific files.
///
/// Refresh also reconciles **shared** instruction sections (HS2-329EED): a shared section's
/// served-tool list is rebuilt from the tools in this refresh, and a shared section whose
/// every listed tool is now disabled (or no longer installed as a plugin) is removed.
///
/// The returned [`RefreshReport`] lists the tools set up and, per disabled tool, the
/// project-relative files deleted or edited to drop Hot Sheet's entries (HS2-CAM9J5).
pub fn refresh_setup_in(
    store_path: &Path,
    project_dir: &Path,
    enabled: Option<&HashSet<String>>,
    plugin_dirs: &[std::path::PathBuf],
) -> Result<RefreshReport, SetupError> {
    apply_setup_plan(plan_refresh_setup_in(
        store_path,
        project_dir,
        enabled,
        plugin_dirs,
    )?)
}

/// Compute refresh's exact file changes without modifying the checkout or Git metadata.
pub fn plan_refresh_setup_in(
    store_path: &Path,
    project_dir: &Path,
    enabled: Option<&HashSet<String>>,
    plugin_dirs: &[PathBuf],
) -> Result<SetupPlan, SetupError> {
    let mut fs = PlanFs::default();
    let policy = read_push_policy(&fs, project_dir)?;
    let external_guidance = rulesync_owns_guidance(&fs, project_dir)?;
    let all = all_plugins(plugin_dirs)
        .into_iter()
        .map(|plugin| plugin.with_push_policy(&policy.skill_wording(), policy.wording()))
        .collect::<Vec<_>>();
    let mut reconcile_targets: Vec<String> = Vec::new();
    for plugin in &all {
        for target in std::iter::once(&plugin.manifest.instructions.target).chain(
            plugin
                .manifest
                .instructions
                .canonical
                .as_ref()
                .map(|canonical| &canonical.target),
        ) {
            if crate::is_safe_rel_path(target) && !reconcile_targets.contains(target) {
                reconcile_targets.push(target.clone());
            }
        }
    }
    // Tools the project explicitly left out of `enabled_plugins` (HS2-FKC8VN). Without an
    // enabled list nothing is excluded: refresh then only ever adds or repairs. An explicit
    // empty list excludes every tool (HS2-8B3VJP).
    let (plugins, excluded): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|plugin| enabled.is_none_or(|set| set.contains(plugin.id())));
    // Every enabled tool keeps each target it declares, even when this run skips it, so a
    // disabled tool never removes an artifact an enabled tool shares (HS2-ZTGX6P).
    let claimed: HashSet<String> = plugins
        .iter()
        .flat_map(|plugin| plugin.target_paths().into_iter().map(String::from))
        .collect();
    let plugins = plugins
        .into_iter()
        .filter(|plugin| is_detected(plugin) || has_managed_setup(&fs, project_dir, plugin))
        .collect::<Vec<_>>();
    // Shared-section membership before this run, so a disabled sharer dropped from (or
    // taken out with) a shared section is reported as an edit of that file (HS2-CAM9J5).
    let shared_members = |fs: &PlanFs, target: &str| {
        fs.read_to_string(&project_dir.join(target))
            .ok()
            .and_then(|contents| parse_shared_section(&contents, &shared_section_key(target)))
            .map(|shared| shared.tools)
            .unwrap_or_default()
    };
    let members_before: Vec<(String, Vec<String>)> = reconcile_targets
        .iter()
        .map(|target| (target.clone(), shared_members(&fs, target)))
        .collect();
    let set_up = if plugins.is_empty() {
        // Nothing to set up, but a shared section whose sharers were all disabled is removed.
        let run_ids = HashSet::new();
        for target in reconcile_targets.iter().filter(|_| !external_guidance) {
            write_instruction_target(&mut fs, project_dir, target, &[], Some(&run_ids))?;
            write_guidance_target(&mut fs, project_dir, target, false)?;
        }
        Vec::new()
    } else {
        setup_plugins_with_fs(
            &mut fs,
            store_path,
            project_dir,
            plugins,
            Some(&reconcile_targets),
        )?
    };
    let mut removed = Vec::new();
    for plugin in &excluded {
        let mut report = remove_disabled_tool_artifacts(&mut fs, project_dir, plugin, &claimed)
            .map_err(|error| error.with_notices(&set_up))?;
        let target = &plugin.manifest.instructions.target;
        let was_member = members_before
            .iter()
            .any(|(t, tools)| t == target && tools.iter().any(|tool| tool == plugin.id()));
        if was_member
            && !shared_members(&fs, target)
                .iter()
                .any(|tool| tool == plugin.id())
        {
            let change = if fs.exists(&project_dir.join(target)) {
                Change::Edited
            } else {
                Change::Removed
            };
            report.record(target, Some(change));
        }
        if !report.removed.is_empty() || !report.edited.is_empty() {
            removed.push(report);
        }
    }
    let report = RefreshReport { set_up, removed };
    let changes = fs.changes(project_dir);
    let tools = collect_findings(store_path, project_dir, enabled, plugin_dirs, &fs, policy);
    let operations = fs.final_operations(project_dir);
    Ok(SetupPlan {
        report,
        changes,
        tools,
        operations,
    })
}

/// Apply exactly the operations produced by [`plan_refresh_setup_in`].
pub fn apply_setup_plan(plan: SetupPlan) -> Result<RefreshReport, SetupError> {
    apply_operations(plan.operations).map_err(|error| error.with_notices(&plan.report.set_up))?;
    Ok(plan.report)
}

fn apply_operations(operations: Vec<PlannedOperation>) -> Result<(), SetupError> {
    let shared_path = operations.iter().find_map(|operation| match operation {
        PlannedOperation::Write(path, _) if path.ends_with(SHARED_SETTINGS_PATH) => Some(path),
        _ => None,
    });
    let Some(shared_path) = shared_path else {
        return apply_operations_unlocked(operations);
    };
    let project = shared_path
        .parent()
        .and_then(Path::parent)
        .expect("shared settings have a project root")
        .to_path_buf();
    let settings = Settings::for_project(&project);
    settings.with_scope_lock(Scope::Shared, || {
        let mut operations = operations;
        for operation in &mut operations {
            let PlannedOperation::Write(path, contents) = operation else {
                continue;
            };
            if !path.ends_with(SHARED_SETTINGS_PATH) {
                continue;
            }
            let planned: serde_json::Map<String, serde_json::Value> =
                serde_json::from_slice(contents)
                    .map_err(|_| SetupError::InvalidPushPolicy(SHARED_SETTINGS_PATH.into()))?;
            let planned_policy = planned
                .get(PUSH_POLICY_SETTING)
                .ok_or_else(|| SetupError::InvalidPushPolicy(SHARED_SETTINGS_PATH.into()))?;
            let mut current = shared_settings(&PlanFs::default(), &project)?;
            match current.get(PUSH_POLICY_SETTING) {
                Some(value) if value != planned_policy => return Err(SetupError::StalePushPolicy),
                Some(_) => {}
                None => {
                    current.insert(PUSH_POLICY_SETTING.into(), planned_policy.clone());
                }
            }
            *contents = (serde_json::to_string_pretty(&current).expect("settings serialize")
                + "\n")
                .into_bytes();
        }
        apply_operations_unlocked(operations)
    })??;
    Ok(())
}

fn apply_operations_unlocked(operations: Vec<PlannedOperation>) -> Result<(), SetupError> {
    for operation in operations {
        match operation {
            PlannedOperation::Write(path, contents) => {
                write_file(&path, &String::from_utf8_lossy(&contents))?
            }
            PlannedOperation::Remove(project, path) => {
                if path.is_file() {
                    std::fs::remove_file(&path).map_err(|source| SetupError::Io {
                        path: path.display().to_string(),
                        source,
                    })?;
                    prune_empty_parent_dirs(&project, &path);
                }
            }
        }
    }
    Ok(())
}

fn prune_empty_parent_dirs(project: &Path, path: &Path) {
    let mut dir = path.parent();
    while let Some(current) = dir {
        if current == project || !current.starts_with(project) {
            break;
        }
        if std::fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

fn collect_findings(
    store_path: &Path,
    project: &Path,
    enabled: Option<&HashSet<String>>,
    plugin_dirs: &[PathBuf],
    fs: &PlanFs,
    policy: PushPolicy,
) -> Vec<ToolSetupFindings> {
    let original = PlanFs::default();
    let external_guidance = rulesync_owns_guidance(&original, project).is_ok_and(|owner| owner);
    all_plugins(plugin_dirs).into_iter().map(|plugin| {
        let plugin = plugin.with_push_policy(&policy.skill_wording(), policy.wording());
        let enabled_tool = enabled.is_none_or(|set| set.contains(plugin.id()));
        let previously_managed = has_managed_setup(&original, project, &plugin);
        let detected = is_detected(&plugin);
        let installed_mcp = installed_mcp_command_args(
            &original, &project.join(&plugin.manifest.mcp.target), &plugin,
        );
        let preserved = if external_guidance { skill_requires_preservation(&original, project, &plugin) }
            else { installed_workflow_requires_preservation(&original, project, &plugin) };
        let mut artifacts = Vec::new();
        {
        let mut add = |kind, rel: &str, preserved: bool| {
            let path = project.join(rel);
            let before = original.read(&path).ok();
            let after = fs.read(&path).ok();
            let installed = match kind {
                ArtifactKind::Instructions => before.as_ref().is_some_and(|bytes| {
                    let contents = String::from_utf8_lossy(bytes);
                    contents.contains(&begin_marker(plugin.id()))
                        || parse_shared_section(&contents, &shared_section_key(rel))
                            .is_some_and(|shared| shared.tools.iter().any(|tool| tool == plugin.id()))
                        || (rel == plugin.manifest.instructions.target && plugin.manifest.instructions.canonical.as_ref()
                            .is_some_and(|canonical| contents.lines().any(|line| line.trim() == canonical.import.trim())))
                }),
                ArtifactKind::Hooks => has_hotsheet_hooks(&original, &path),
                _ => before.is_some(),
            };
            let externally_owned = external_guidance && kind == ArtifactKind::Instructions;
            let status = if externally_owned { ArtifactStatus::Preserved }
                else if !enabled_tool { ArtifactStatus::Disabled }
                else if !installed { ArtifactStatus::Missing }
                else if preserved { ArtifactStatus::Preserved }
                else if before == after { ArtifactStatus::Current }
                else { ArtifactStatus::Outdated };
            let detail = if externally_owned { "managed by Rulesync; Hot Sheet leaves this instruction file untouched".to_string() } else { match status {
                ArtifactStatus::Current => "matches the installed setup".to_string(),
                ArtifactStatus::Missing => "artifact is missing".to_string(),
                ArtifactStatus::Outdated => "artifact differs from the setup refresh output".to_string(),
                ArtifactStatus::Preserved => "newer or user-modified workflow is preserved".to_string(),
                ArtifactStatus::Disabled => "tool is excluded by enabled_plugins".to_string(),
            }};
            artifacts.push(ArtifactFinding { kind, path: rel.to_string(), status, detail });
        };
        add(ArtifactKind::Instructions, &plugin.manifest.instructions.target, preserved);
        if let Some(canonical) = &plugin.manifest.instructions.canonical {
            add(ArtifactKind::Instructions, &canonical.target, preserved);
        }
        if let Some((path, _)) = plugin.skill() { add(ArtifactKind::Skill, path, preserved); }
        add(ArtifactKind::Mcp, &plugin.manifest.mcp.target, false);
        if let Some(hooks) = &plugin.manifest.hooks { add(ArtifactKind::Hooks, &hooks.target, false); }
        }
        if enabled_tool {
            if let Some(finding) = artifacts.iter_mut().find(|finding| finding.kind == ArtifactKind::Mcp) {
                let store_abs = store_path.canonicalize().unwrap_or_else(|_| store_path.to_path_buf());
                let (expected_command, expected_args, _) = mcp_entry(
                    &original,
                    project,
                    &store_abs,
                    &plugin,
                );
                match installed_mcp.as_ref() {
                    None if original.is_file(&project.join(&plugin.manifest.mcp.target)) => {
                        finding.status = ArtifactStatus::Missing;
                        finding.detail = "Hot Sheet MCP entry is missing from the config".into();
                    }
                    Some((command, args)) if command != &expected_command || args != &expected_args => {
                        finding.status = ArtifactStatus::Outdated;
                        finding.detail = if command != &expected_command {
                            format!("MCP command differs: installed `{command}`, expected `{expected_command}`")
                        } else {
                            format!("MCP args differ: installed `{}`, expected `{}`", args.join(" "), expected_args.join(" "))
                        };
                    }
                    _ => {}
                }
            }
        }
        ToolSetupFindings {
            tool: plugin.id().to_string(), enabled: enabled_tool,
            detected,
            previously_managed,
            artifacts,
        }
    }).collect()
}

fn installed_mcp_command_args(
    fs: &PlanFs,
    path: &Path,
    plugin: &Plugin,
) -> Option<(String, Vec<String>)> {
    let contents = fs.read_to_string(path).ok()?;
    let spec = &plugin.manifest.mcp;
    match spec.format.as_str() {
        "claude-json" => {
            let root: serde_json::Value = serde_json::from_str(&contents).ok()?;
            let entry = &root["mcpServers"][&spec.server_name];
            Some((
                entry["command"].as_str()?.to_string(),
                entry["args"]
                    .as_array()?
                    .iter()
                    .map(|arg| arg.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()?,
            ))
        }
        "opencode-json" => {
            let root: serde_json::Value = serde_json::from_str(&contents).ok()?;
            let command = root["mcp"][&spec.server_name]["command"].as_array()?;
            let (first, rest) = command.split_first()?;
            Some((
                first.as_str()?.to_string(),
                rest.iter()
                    .map(|arg| arg.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()?,
            ))
        }
        "codex-toml" => {
            let root: toml::Table = toml::from_str(&contents).ok()?;
            let entry = root.get("mcp_servers")?.get(&spec.server_name)?;
            Some((
                entry.get("command")?.as_str()?.to_string(),
                entry
                    .get("args")?
                    .as_array()?
                    .iter()
                    .map(|arg| arg.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()?,
            ))
        }
        _ => None,
    }
}

/// Remove the wholly Hot Sheet-owned setup artifacts of a tool the project disabled
/// (`enabled_plugins` excludes it), so a disabled tool leaves no guidance, skill, MCP
/// server, or permission hook behind and no longer counts as previously managed
/// (HS2-FKC8VN, HS2-ZTGX6P). Ownership follows setup's rules:
///
/// * The instruction section and the worklist skill form one workflow bundle. When either
///   installed half was written by a newer Hot Sheet or is an equal-version customization,
///   both are preserved, exactly as setup would preserve them.
/// * Only the `hotsheet` MCP server entry whose command is Hot Sheet's MCP binary, and only
///   Hot Sheet permission hooks, are removed; every other entry in a shared config stays.
///   A config left holding nothing is deleted with its local git exclude.
/// * A path an enabled tool also declares is never touched.
fn remove_disabled_tool_artifacts(
    fs: &mut PlanFs,
    project: &Path,
    plugin: &Plugin,
    claimed: &HashSet<String>,
) -> Result<RemovalReport, SetupError> {
    let mut report = RemovalReport {
        tool: plugin.manifest.product_name.clone(),
        ..RemovalReport::default()
    };
    let rel = &plugin.manifest.instructions.target;
    let instructions = crate::is_safe_rel_path(rel)
        .then(|| fs.read_to_string(&project.join(rel)).ok())
        .flatten();
    let mut canonical_plugin = plugin.clone();
    if let Some(canonical) = &plugin.manifest.instructions.canonical {
        canonical_plugin.manifest.instructions.target = canonical.target.clone();
        canonical_plugin.manifest.instructions.canonical = None;
    }
    let native_is_reference = instructions.as_ref().is_some_and(|contents| {
        instruction_reference_block(plugin).is_some_and(|block| contents.contains(&block))
    });
    let external_guidance = rulesync_owns_guidance(fs, project)?;
    let workflow_preserved = skill_requires_preservation(fs, project, plugin)
        || (!external_guidance
            && ((!native_is_reference
                && per_tool_section_requires_preservation(plugin, instructions.as_deref()))
                || (plugin.manifest.instructions.canonical.is_some()
                    && installed_workflow_requires_preservation(fs, project, &canonical_plugin))));
    if !workflow_preserved {
        if !external_guidance {
            if !native_is_reference {
                report.record(rel, remove_disabled_tool_section(fs, project, plugin)?);
            }
            if plugin.manifest.instructions.canonical.is_some() {
                report.record(
                    &canonical_plugin.manifest.instructions.target,
                    remove_disabled_tool_section(fs, project, &canonical_plugin)?,
                );
                report.record(
                    rel,
                    write_instruction_reference(fs, project, plugin, false)?,
                );
            }
        }
        if let Some((rel, _)) = plugin.skill() {
            if crate::is_safe_rel_path(rel) && !claimed.contains(rel) {
                report.record(rel, remove_managed_file(fs, project, rel, false)?);
            }
        }
    }
    let mcp = &plugin.manifest.mcp.target;
    if crate::is_safe_rel_path(mcp) && !claimed.contains(mcp) {
        report.record(mcp, remove_mcp_entry(fs, project, plugin)?);
    }
    if let Some(spec) = &plugin.manifest.hooks {
        if crate::is_safe_rel_path(&spec.target) && !claimed.contains(&spec.target) {
            report.record(
                &spec.target,
                remove_hotsheet_hooks(fs, project, &spec.target)?,
            );
        }
    }
    Ok(report)
}

/// Remove the per-tool `hotsheet:<id>` instruction section of a disabled tool, so
/// disabled-tool guidance leaves per-tool and shared layouts alike (HS2-FKC8VN). A section
/// written by a newer Hot Sheet, or an equal-version section the project customized, is
/// preserved. The file is never created, and content outside the managed markers is
/// untouched. A file left holding nothing but whitespace (setup created it for that section)
/// is deleted, like a config that held only Hot Sheet's entry (HS2-G9CD1W).
fn remove_disabled_tool_section(
    fs: &mut PlanFs,
    project: &Path,
    plugin: &Plugin,
) -> Result<Option<Change>, SetupError> {
    let rel = &plugin.manifest.instructions.target;
    if !crate::is_safe_rel_path(rel) {
        return Ok(None);
    }
    let path = project.join(rel);
    let Ok(existing) = fs.read_to_string(&path) else {
        return Ok(None);
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
    if out == existing {
        return Ok(None);
    }
    if out.trim().is_empty() {
        return remove_managed_file(fs, project, rel, false);
    }
    fs.write(&path, out.as_bytes());
    Ok(Some(Change::Edited))
}

/// Delete a Hot Sheet-owned file, then prune the directories it leaves empty (never the
/// project itself). `excluded` also drops the file's local git exclude, which setup added
/// only for a wholly Hot Sheet-owned config.
fn remove_managed_file(
    fs: &mut PlanFs,
    project: &Path,
    rel: &str,
    excluded: bool,
) -> Result<Option<Change>, SetupError> {
    let path = project.join(rel);
    if !fs.is_file(&path) {
        return Ok(None);
    }
    fs.remove(project, &path);
    if excluded {
        remove_local_git_exclude(fs, project, rel)?;
    }
    Ok(Some(Change::Removed))
}

/// Whether an installed MCP `command` launches Hot Sheet's MCP binary (`bundled` is the
/// manifest command; the installed one may be an absolute sibling path or a `.exe`).
fn is_hotsheet_mcp_command(installed: &str, bundled: &str) -> bool {
    let stem = |command: &str| {
        Path::new(command)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    };
    stem(installed).is_some_and(|installed| Some(installed) == stem(bundled))
}

/// Remove a disabled tool's Hot Sheet MCP server entry (HS2-ZTGX6P): the entry named by
/// the manifest whose command is Hot Sheet's MCP binary. Unrelated servers and settings in
/// a shared config are kept; a config that held only Hot Sheet's entry is deleted.
fn remove_mcp_entry(
    fs: &mut PlanFs,
    project: &Path,
    plugin: &Plugin,
) -> Result<Option<Change>, SetupError> {
    let spec = &plugin.manifest.mcp;
    let target = project.join(&spec.target);
    let Ok(contents) = fs.read_to_string(&target) else {
        return Ok(None);
    };
    let name = spec.server_name.as_str();
    let owned = |command: Option<&str>| {
        command.is_some_and(|command| is_hotsheet_mcp_command(command, &spec.command))
    };
    let rendered = match spec.format.as_str() {
        format @ ("claude-json" | "opencode-json") => {
            let key = if format == "claude-json" {
                "mcpServers"
            } else {
                "mcp"
            };
            let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&contents) else {
                return Ok(None);
            };
            let Some(object) = root.as_object_mut() else {
                return Ok(None);
            };
            let Some(servers) = object
                .get_mut(key)
                .and_then(serde_json::Value::as_object_mut)
            else {
                return Ok(None);
            };
            let command = servers.get(name).and_then(|entry| {
                let command = entry.get("command")?;
                command
                    .as_str()
                    .or_else(|| command.as_array()?.first()?.as_str())
            });
            if !owned(command) {
                return Ok(None);
            }
            servers.remove(name);
            if servers.is_empty() {
                object.remove(key);
            }
            let hotsheet_only = object
                .keys()
                .all(|key| format == "opencode-json" && key == "$schema");
            if hotsheet_only {
                None
            } else {
                Some(serde_json::to_string_pretty(&root).unwrap() + "\n")
            }
        }
        "codex-toml" => {
            let Ok(mut root) = toml::from_str::<toml::Table>(&contents) else {
                return Ok(None);
            };
            let Some(servers) = root
                .get_mut("mcp_servers")
                .and_then(toml::Value::as_table_mut)
            else {
                return Ok(None);
            };
            let command = servers
                .get(name)
                .and_then(|entry| entry.get("command")?.as_str());
            if !owned(command) {
                return Ok(None);
            }
            servers.remove(name);
            if servers.is_empty() {
                root.remove("mcp_servers");
            }
            if root.is_empty() {
                None
            } else {
                Some(toml::to_string_pretty(&root).unwrap())
            }
        }
        _ => return Ok(None),
    };
    match rendered {
        Some(rendered) => {
            fs.write(&target, rendered.as_bytes());
            Ok(Some(Change::Edited))
        }
        None => remove_managed_file(fs, project, &spec.target, true),
    }
}

/// Remove every Hot Sheet permission hook from a disabled tool's hook config
/// (HS2-ZTGX6P). The user's own hooks and other settings stay; an event list emptied by
/// the removal is dropped, and a config left empty is deleted with its local git exclude.
fn remove_hotsheet_hooks(
    fs: &mut PlanFs,
    project: &Path,
    rel: &str,
) -> Result<Option<Change>, SetupError> {
    let target = project.join(rel);
    let Some(mut root) = fs
        .read_to_string(&target)
        .ok()
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
    else {
        return Ok(None);
    };
    let Some(object) = root.as_object_mut() else {
        return Ok(None);
    };
    let Some(hooks) = object
        .get_mut("hooks")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(None);
    };
    let mut changed = false;
    hooks.retain(|_, entries| {
        let Some(entries) = entries.as_array_mut() else {
            return true;
        };
        let before = entries.len();
        entries.retain(|entry| !is_hotsheet_hook(entry));
        if entries.len() == before {
            return true;
        }
        changed = true;
        !entries.is_empty()
    });
    if !changed {
        return Ok(None);
    }
    if hooks.is_empty() {
        object.remove("hooks");
    }
    if object.is_empty() {
        remove_managed_file(fs, project, rel, true)
    } else {
        fs.write(
            &target,
            (serde_json::to_string_pretty(&root).unwrap() + "\n").as_bytes(),
        );
        Ok(Some(Change::Edited))
    }
}

fn has_managed_setup(fs: &PlanFs, project_dir: &Path, plugin: &Plugin) -> bool {
    has_tool_instructions(fs, project_dir, plugin)
        || plugin
            .skill()
            .is_some_and(|(target, _)| fs.is_file(&project_dir.join(target)))
        || plugin
            .manifest
            .hooks
            .as_ref()
            .is_some_and(|spec| has_hotsheet_hooks(fs, &project_dir.join(&spec.target)))
}

fn has_tool_instructions(fs: &PlanFs, project_dir: &Path, plugin: &Plugin) -> bool {
    let marker = format!("<!-- BEGIN hotsheet:{} -->", plugin.id());
    std::iter::once(&plugin.manifest.instructions.target)
        .chain(
            plugin
                .manifest
                .instructions
                .canonical
                .as_ref()
                .map(|canonical| &canonical.target),
        )
        .any(|target| {
            fs.read_to_string(&project_dir.join(target))
                .is_ok_and(|contents| {
                    contents.contains(&marker)
                        || (target == &plugin.manifest.instructions.target
                            && contents
                                .contains(&begin_marker(&format!("{}-reference", plugin.id()))))
                        || parse_shared_section(&contents, &shared_section_key(target)).is_some_and(
                            |shared| shared.tools.iter().any(|tool| tool == plugin.id()),
                        )
                })
        })
}

/// Move owned legacy instruction blocks to a shared source before normal planning.
/// Ambiguous/customized layouts retain their existing target and writer protections.
fn canonical_instruction_plugin(
    fs: &mut PlanFs,
    project: &Path,
    plugin: &Plugin,
) -> Result<Plugin, SetupError> {
    let mut effective = plugin.clone();
    let Some(canonical) = &plugin.manifest.instructions.canonical else {
        return Ok(effective);
    };
    let native_path = project.join(&plugin.manifest.instructions.target);
    let native = fs.read_to_string(&native_path).unwrap_or_default();
    let reference = instruction_reference_block(plugin);
    let native_is_reference = reference
        .as_ref()
        .is_some_and(|block| native.contains(block));
    if (!native_is_reference && per_tool_section_requires_preservation(plugin, Some(&native)))
        || skill_requires_preservation(fs, project, plugin)
        || parse_shared_section(
            &native,
            &shared_section_key(&plugin.manifest.instructions.target),
        )
        .is_some()
    {
        return Ok(effective);
    }
    let canonical_path = project.join(&canonical.target);
    let mut incoming = fs.read_to_string(&canonical_path).unwrap_or_default();
    let mut source_plugin = plugin.clone();
    source_plugin.manifest.instructions.target = canonical.target.clone();
    source_plugin.manifest.instructions.canonical = None;
    if !native_is_reference
        && native.contains(&begin_marker(plugin.id()))
        && installed_workflow_requires_preservation(fs, project, &source_plugin)
    {
        return Ok(effective);
    }
    let mut outgoing = native.clone();
    let (begin, end) = (begin_marker(plugin.id()), end_marker(plugin.id()));
    while let Some((start, finish)) = find_block(&outgoing, &begin, &end) {
        let block = &outgoing[start..finish];
        if reference.as_deref() == Some(block) {
            // The native reference stays in place; only older full blocks move.
            break;
        } else if let Some((a, b)) = find_block(&incoming, &begin, &end) {
            if &incoming[a..b] != block {
                return Ok(effective);
            }
        } else {
            incoming = append_block(&incoming, block);
        }
        outgoing = remove_range(&outgoing, (start, finish));
    }
    if !native_is_reference && outgoing.contains(&begin) {
        return Ok(effective);
    }
    for section in GUIDANCE_SECTIONS {
        let Some((start, finish)) = guidance_bounds(&outgoing, section.name) else {
            if outgoing.contains(&format!("<!-- hotsheet:begin section={}", section.name)) {
                return Ok(effective);
            }
            continue;
        };
        let Some((generic, specifics)) = guidance_parts(&outgoing[start..finish], section.name)
        else {
            return Ok(effective);
        };
        let known = |body: &str| {
            body == section.current.trim()
                || section.legacy.iter().any(|legacy| body == legacy.trim())
        };
        if !known(&generic) {
            return Ok(effective);
        }
        if let Some((a, b)) = guidance_bounds(&incoming, section.name) {
            let Some((other_generic, other_specifics)) =
                guidance_parts(&incoming[a..b], section.name)
            else {
                return Ok(effective);
            };
            if !known(&other_generic)
                || (specifics.is_some()
                    && other_specifics.is_some()
                    && specifics != other_specifics)
            {
                return Ok(effective);
            }
            let merged = guidance_with_specifics(
                section.current,
                section.name,
                specifics.or(other_specifics),
            );
            incoming.replace_range(a..b, &merged);
        } else {
            incoming = append_block(
                &incoming,
                &guidance_with_specifics(section.current, section.name, specifics),
            );
        }
        outgoing = remove_range(&outgoing, (start, finish));
    }
    if outgoing != native {
        if outgoing.trim().is_empty() {
            remove_managed_file(fs, project, &plugin.manifest.instructions.target, false)?;
        } else {
            fs.write(&native_path, outgoing.as_bytes());
        }
        fs.write(&canonical_path, incoming.as_bytes());
    }
    effective.manifest.instructions.target = canonical.target.clone();
    Ok(effective)
}

fn instruction_reference_block(plugin: &Plugin) -> Option<String> {
    let canonical = plugin.manifest.instructions.canonical.as_ref()?;
    let version = marked_version(plugin.instructions_body(), INSTRUCTIONS_VERSION_PREFIX)?;
    Some(format!(
        "{}\n{INSTRUCTIONS_VERSION_PREFIX}{version} -->\n\n{}\n{}",
        begin_marker(plugin.id()),
        canonical.import.trim(),
        end_marker(plugin.id())
    ))
}

fn write_instruction_reference(
    fs: &mut PlanFs,
    project: &Path,
    plugin: &Plugin,
    active: bool,
) -> Result<Option<Change>, SetupError> {
    let Some(canonical) = &plugin.manifest.instructions.canonical else {
        return Ok(None);
    };
    let rel = &plugin.manifest.instructions.target;
    let target = project.join(rel);
    let existing = fs.read_to_string(&target).unwrap_or_default();
    let (begin, end) = (begin_marker(plugin.id()), end_marker(plugin.id()));
    let Some(block) = instruction_reference_block(plugin) else {
        return Ok(None);
    };
    let rendered = if active {
        if existing
            .lines()
            .any(|line| line.trim() == canonical.import.trim())
        {
            return Ok(None);
        }
        // An edited reference belongs to the project, just like an edited instruction block.
        if existing.contains(&begin) {
            return Ok(None);
        }
        append_block(&existing, &block)
    } else if let Some((start, finish)) = find_block(&existing, &begin, &end) {
        if existing[start..finish] != block {
            return Ok(None);
        }
        remove_range(&existing, (start, finish))
    } else {
        return Ok(None);
    };
    if rendered.trim().is_empty() {
        return remove_managed_file(fs, project, rel, false);
    }
    fs.write(&target, rendered.as_bytes());
    Ok(Some(Change::Edited))
}

fn has_hotsheet_hooks(fs: &PlanFs, path: &Path) -> bool {
    fs.read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
        .and_then(|root| root.get("hooks")?.as_object().cloned())
        .is_some_and(|hooks| {
            hooks.values().any(|entries| {
                entries
                    .as_array()
                    .is_some_and(|entries| entries.iter().any(is_hotsheet_hook))
            })
        })
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
    let mut fs = PlanFs::default();
    let result = setup_plugins_with_fs(&mut fs, store_path, project_dir, plugins, refresh_targets);
    // Explicit setup historically leaves earlier tools' successful writes in place if a
    // later tool fails. Refresh/diagnose use the pure plan, but retain this direct path.
    apply_operations(fs.final_operations(project_dir))?;
    result
}

fn setup_plugins_with_fs(
    fs: &mut PlanFs,
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

    let policy = ensure_push_policy(fs, project_dir)?;
    let plugins = plugins
        .into_iter()
        .map(|plugin| plugin.with_push_policy(&policy.skill_wording(), policy.wording()))
        .collect::<Vec<_>>();

    let declared_plugins = plugins;
    let external_guidance = rulesync_owns_guidance(fs, project_dir)?;
    let plugins = declared_plugins
        .iter()
        .map(|plugin| {
            if external_guidance {
                Ok(plugin.clone())
            } else {
                canonical_instruction_plugin(fs, project_dir, plugin)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let preserved: Vec<bool> = plugins
        .iter()
        .map(|p| {
            if external_guidance {
                skill_requires_preservation(fs, project_dir, p)
            } else {
                installed_workflow_requires_preservation(fs, project_dir, p)
            }
        })
        .collect();
    let mut blocked_guidance = BTreeMap::<String, Vec<SetupNotice>>::new();
    if !external_guidance {
        for plugin in &plugins {
            if !plugin.manifest.instructions.shared_guidance {
                continue;
            }
            let target = &plugin.manifest.instructions.target;
            blocked_guidance.entry(target.clone()).or_insert_with(|| {
                fs.read_to_string(&project_dir.join(target))
                    .ok()
                    .map(|contents| blocked_bundled_guidance(&contents, target))
                    .unwrap_or_default()
            });
        }
    }
    let run_ids: HashSet<String> = plugins.iter().map(|p| p.id().to_string()).collect();
    let membership = refresh_targets.map(|_| &run_ids);

    // Instruction targets first, grouped so tools sharing one file (e.g. AGENTS.md) can be
    // served by one shared section instead of byte-identical per-tool copies.
    if !external_guidance {
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
            write_instruction_target(fs, project_dir, target, &writers, membership)?;
        }
        // These independently versioned, project-neutral sections are shared by opted-in
        // tools that read an instruction target. They are not copies of this
        // repository's AGENTS.md, whose nested specifics remain project-owned.
        let guidance_targets: HashSet<&str> = plugins
            .iter()
            .filter(|plugin| plugin.manifest.instructions.shared_guidance)
            .map(|plugin| plugin.manifest.instructions.target.as_str())
            .collect();
        for target in &guidance_targets {
            write_guidance_target(fs, project_dir, target, true)?;
        }
        for target in refresh_targets.unwrap_or_default() {
            if !targets.contains(&target.as_str()) {
                write_instruction_target(fs, project_dir, target, &[], membership)?;
            }
            if !guidance_targets.contains(target.as_str()) {
                write_guidance_target(fs, project_dir, target, false)?;
            }
        }
        for (declared, effective) in declared_plugins.iter().zip(&plugins) {
            if declared.manifest.instructions.target != effective.manifest.instructions.target {
                write_instruction_reference(fs, project_dir, declared, true)?;
            }
        }
    }

    let mut reports = Vec::new();
    let result = (|| -> Result<(), SetupError> {
        for ((p, declared), preserve_installed_workflow) in
            plugins.iter().zip(&declared_plugins).zip(preserved)
        {
            let mut wrote = Vec::new();
            if !preserve_installed_workflow {
                if !external_guidance {
                    wrote.push(p.manifest.instructions.target.clone());
                }
                if !external_guidance
                    && p.manifest.instructions.target != declared.manifest.instructions.target
                {
                    wrote.push(declared.manifest.instructions.target.clone());
                }
                if let Some(skill) = write_skill(fs, project_dir, p)? {
                    wrote.push(skill); // absent for tools with no skills concept (e.g. Antigravity)
                }
            }
            wrote.push(write_mcp(fs, project_dir, &store_abs, p)?);
            let mut notices = Vec::new();
            if let Some(blocked) = blocked_guidance.remove(&p.manifest.instructions.target) {
                notices.extend(blocked);
            }
            if let Some((hook, changed)) = write_hooks(fs, project_dir, p)? {
                if changed
                    && matches!(
                        p.manifest
                            .hooks
                            .as_ref()
                            .and_then(|spec| spec.change_notice.as_ref()),
                        Some(crate::HookChangeNotice::CodexHooksChanged)
                    )
                {
                    notices.push(SetupNotice::CodexHooksChanged { path: hook.clone() });
                }
                wrote.push(hook); // absent when no native interactive adapter is declared
            }
            reports.push(SetupReport {
                tool: p.manifest.product_name.clone(),
                wrote,
                notices,
            });
        }
        write_selected_source_skills(fs, project_dir, &plugins)?;
        Ok(())
    })();
    result.map_err(|error| error.with_notices(&reports))?;
    Ok(reports)
}

fn write_selected_source_skills(
    fs: &mut PlanFs,
    project: &Path,
    plugins: &[Plugin],
) -> Result<(), SetupError> {
    use base64::Engine;
    let config = crate::guidance_sources::load(project)?;
    if !config.explicit_selection {
        return Ok(());
    }
    for plugin in plugins {
        let Some((target, _)) = plugin.skill() else {
            continue;
        };
        let Some(parent) = Path::new(target).parent().and_then(Path::parent) else {
            continue;
        };
        for source in &config.sources {
            for skill in &source.skills {
                let selected = source.selected_skills.contains(&skill.name);
                for file in &skill.files {
                    let path = project.join(parent).join(&skill.name).join(&file.path);
                    let body = base64::engine::general_purpose::STANDARD
                        .decode(&file.base64)
                        .map_err(|_| {
                            SetupError::GuidanceSource(
                                crate::guidance_sources::GuidanceSourceError::Invalid(
                                    "invalid cached skill".into(),
                                ),
                            )
                        })?;
                    let prior = file
                        .known_base64
                        .iter()
                        .filter_map(|encoded| {
                            base64::engine::general_purpose::STANDARD
                                .decode(encoded)
                                .ok()
                        })
                        .collect::<Vec<_>>();
                    match fs.read(&path) {
                        Ok(existing)
                            if !selected && (existing == body || prior.contains(&existing)) =>
                        {
                            fs.remove(project, &path);
                        }
                        Ok(existing) if selected && prior.contains(&existing) => {
                            fs.write(&path, &body)
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound && selected => {
                            fs.write(&path, &body);
                        }
                        _ => {} // A local edit or same-name skill belongs to the project.
                    }
                }
            }
        }
    }
    Ok(())
}

const INSTRUCTIONS_VERSION_PREFIX: &str = "<!-- hotsheet-instructions-version: ";
const SKILL_VERSION_PREFIX: &str = "<!-- hotsheet-skill-version: ";

struct GuidanceSection {
    name: &'static str,
    current: &'static str,
    legacy: &'static [&'static str],
}

const GUIDANCE_SECTIONS: &[GuidanceSection] = &[
    GuidanceSection {
        name: "ticket-driven-work",
        current: include_str!("../../../plugins/shared/ticket-driven-work.md"),
        legacy: &[
            include_str!("../../../plugins/shared/legacy-ticket-driven-work.md"),
            include_str!("../../../plugins/shared/legacy-ticket-driven-work-v4.md"),
            include_str!("../../../plugins/shared/legacy-ticket-driven-work-v5.md"),
        ],
    },
    GuidanceSection {
        name: "testing-philosophy",
        current: include_str!("../../../plugins/shared/testing-philosophy.md"),
        legacy: &[
            include_str!("../../../plugins/shared/legacy-testing-philosophy.md"),
            include_str!("../../../plugins/shared/legacy-testing-philosophy-v3.md"),
            include_str!("../../../plugins/shared/legacy-testing-philosophy-v4.md"),
            include_str!("../../../plugins/shared/legacy-testing-philosophy-v5.md"),
        ],
    },
    GuidanceSection {
        name: "requirements-documentation",
        current: include_str!("../../../plugins/shared/requirements-documentation.md"),
        legacy: &[
            include_str!("../../../plugins/shared/legacy-requirements-documentation.md"),
            include_str!("../../../plugins/shared/legacy-requirements-documentation-v2.md"),
            include_str!("../../../plugins/shared/legacy-requirements-documentation-v3.md"),
        ],
    },
    GuidanceSection {
        name: "visual-qa",
        current: include_str!("../../../plugins/shared/visual-qa.md"),
        legacy: &[include_str!(
            "../../../plugins/shared/legacy-visual-qa-v1.md"
        )],
    },
    GuidanceSection {
        name: "sandbox-recovery",
        current: include_str!("../../../plugins/shared/sandbox-recovery.md"),
        legacy: &[],
    },
];

fn guidance_bounds(contents: &str, name: &str) -> Option<(usize, usize)> {
    let begin = format!("<!-- hotsheet:begin section={name} v=");
    let end = format!("<!-- hotsheet:end section={name} -->");
    let start = contents.find(&begin)?;
    let finish = contents[start..].find(&end)? + start + end.len();
    Some((start, finish))
}

/// Separate the versioned generic text from project-owned nested specifics. A malformed
/// specifics block is left untouched rather than risking its loss during refresh.
fn guidance_parts<'a>(block: &'a str, name: &str) -> Option<(String, Option<&'a str>)> {
    let specifics_begin = format!("<!-- hotsheet:begin specifics={name} v=");
    let Some(start) = block.find(&specifics_begin) else {
        return Some((block.trim().to_string(), None));
    };
    let specifics_end = format!("<!-- hotsheet:end specifics={name} -->");
    let finish = block[start..].find(&specifics_end)? + start + specifics_end.len();
    let outer_end = format!("<!-- hotsheet:end section={name} -->");
    let generic = format!("{}\n\n{outer_end}", block[..start].trim_end());
    Some((generic, Some(&block[start..finish])))
}

fn guidance_with_specifics(template: &str, name: &str, specifics: Option<&str>) -> String {
    let Some(specifics) = specifics else {
        return template.trim().to_string();
    };
    let end = format!("<!-- hotsheet:end section={name} -->");
    format!(
        "{}\n\n{}\n\n{end}",
        template.trim().strip_suffix(&end).unwrap().trim_end(),
        specifics.trim()
    )
}

/// Only a newer bundled section that the normal exact-predecessor migration will
/// preserve is a blocked update. Equal-version edits and newer local sections are
/// local choices, while known legacy bodies are upgraded automatically.
fn blocked_bundled_guidance(existing: &str, path: &str) -> Vec<SetupNotice> {
    GUIDANCE_SECTIONS
        .iter()
        .filter_map(|section| {
            let (start, finish) = guidance_bounds(existing, section.name)?;
            let installed = &existing[start..finish];
            let bundled = section.current.trim();
            let installed_version = guidance_section_version(installed, section.name)?;
            let bundled_version = guidance_section_version(bundled, section.name)?;
            if installed_version >= bundled_version {
                return None;
            }
            let generic = guidance_parts(installed, section.name).map(|(generic, _)| generic);
            if generic.as_deref() == Some(bundled)
                || generic.as_ref().is_some_and(|generic| {
                    section.legacy.iter().any(|legacy| generic == legacy.trim())
                })
            {
                return None;
            }
            Some(SetupNotice::BundledGuidanceBlocked {
                path: path.to_owned(),
                section: section.name.to_owned(),
                installed_version,
                bundled_version,
            })
        })
        .collect()
}

fn guidance_section_version(block: &str, name: &str) -> Option<u64> {
    block
        .lines()
        .next()?
        .trim()
        .strip_prefix(&format!("<!-- hotsheet:begin section={name} v="))?
        .strip_suffix(" -->")?
        .parse()
        .ok()
}

/// Install fresh sections and upgrade only an exact known predecessor. A newer version,
/// an equal-version edit, or an unknown older body is treated as project customization.
fn render_guidance_sections(existing: &str, active: bool) -> String {
    let mut rendered = existing.to_string();
    for section in GUIDANCE_SECTIONS {
        let Some((start, finish)) = guidance_bounds(&rendered, section.name) else {
            let marker = format!("<!-- hotsheet:begin section={}", section.name);
            if active && !rendered.contains(&marker) {
                rendered = append_block(&rendered, section.current.trim());
            }
            continue;
        };
        let installed = &rendered[start..finish];
        let Some((generic, specifics)) = guidance_parts(installed, section.name) else {
            continue;
        };
        let current = section.current.trim();
        if generic == current {
            if !active && specifics.is_none() {
                rendered = remove_range(&rendered, (start, finish));
            }
            continue;
        }
        if !section.legacy.iter().any(|legacy| generic == legacy.trim()) {
            continue;
        }
        if active {
            let replacement = guidance_with_specifics(current, section.name, specifics);
            rendered.replace_range(start..finish, &replacement);
        } else if specifics.is_none() {
            rendered = remove_range(&rendered, (start, finish));
        }
    }
    rendered
}

fn render_selected_guidance(
    existing: &str,
    config: &crate::guidance_sources::GuidanceConfig,
    active: bool,
) -> String {
    if !config.explicit_selection {
        return existing.to_owned();
    }
    if config.sources.iter().any(|source| {
        source
            .rules
            .iter()
            .filter(|rule| source.selected_rules.contains(&rule.name))
            .any(|rule| {
                crate::guidance_sources::require_compatible(&crate::guidance_sources::chosen_rule(
                    source, rule,
                ))
                .is_err()
            })
    }) {
        return existing.to_owned();
    }
    let mut rendered = existing.to_owned();
    if active {
        if let Some((source, workflow)) = config.sources.iter().find_map(|source| {
            source
                .rules
                .iter()
                .find(|rule| {
                    rule.name == "hotsheet-workflow" && source.selected_rules.contains(&rule.name)
                })
                .map(|rule| (source, rule))
        }) {
            let chosen = crate::guidance_sources::chosen_rule(source, workflow);
            let plugins = all_plugins(&default_dirs());
            if let Some(shared) = parse_shared_section(&rendered, "agents-md") {
                let bundled = plugins.iter().any(|plugin| {
                    (plugin.manifest.instructions.target == "AGENTS.md"
                        || plugin
                            .manifest
                            .instructions
                            .canonical
                            .as_ref()
                            .is_some_and(|canonical| canonical.target == "AGENTS.md"))
                        && shared.body.trim() == plugin.instructions_body().trim()
                });
                let known = bundled
                    || shared.body.trim() == chosen.body.trim()
                    || workflow
                        .known_bodies
                        .iter()
                        .any(|body| shared.body.trim() == body.trim());
                let installed =
                    marked_version(&shared.body, INSTRUCTIONS_VERSION_PREFIX).unwrap_or(0);
                if known
                    && (installed <= chosen.version || workflow.selected_sha256.is_some())
                    && shared.body.trim() != chosen.body.trim()
                {
                    let members = shared.tools.into_iter().collect();
                    rendered.replace_range(
                        shared.start..shared.end,
                        &shared_block("agents-md", &members, chosen.body),
                    );
                }
            } else {
                let mut served = false;
                for plugin in &plugins {
                    if plugin.manifest.instructions.target != "AGENTS.md"
                        && !plugin
                            .manifest
                            .instructions
                            .canonical
                            .as_ref()
                            .is_some_and(|canonical| canonical.target == "AGENTS.md")
                    {
                        continue;
                    }
                    let (begin, end) = (begin_marker(plugin.id()), end_marker(plugin.id()));
                    let Some((start, finish)) = find_block(&rendered, &begin, &end) else {
                        continue;
                    };
                    served = true;
                    let body = rendered[start + begin.len()..finish - end.len()].trim();
                    let known = body == plugin.instructions_body().trim()
                        || body == chosen.body.trim()
                        || workflow.known_bodies.iter().any(|old| body == old.trim());
                    let installed = marked_version(body, INSTRUCTIONS_VERSION_PREFIX).unwrap_or(0);
                    if known
                        && (installed <= chosen.version || workflow.selected_sha256.is_some())
                        && body != chosen.body.trim()
                    {
                        rendered.replace_range(
                            start..finish,
                            &format!("{begin}\n{}\n{end}", chosen.body.trim_end()),
                        );
                    }
                }
                if !served {
                    rendered = append_block(
                        &rendered,
                        &shared_block("agents-md", &std::collections::BTreeSet::new(), chosen.body),
                    );
                }
            }
        }
    }
    let selected = config
        .sources
        .iter()
        .flat_map(|source| {
            source.rules.iter().filter_map(move |rule| {
                source
                    .selected_rules
                    .contains(&rule.name)
                    .then_some((source, rule))
            })
        })
        .filter(|(_, rule)| rule.name != "hotsheet-workflow")
        .map(|(source, rule)| (rule.name.as_str(), (source, rule)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut candidates: std::collections::BTreeMap<
        &str,
        Vec<&crate::guidance_sources::SourceRule>,
    > = std::collections::BTreeMap::new();
    for rule in config.sources.iter().flat_map(|source| &source.rules) {
        if rule.name != "hotsheet-workflow" {
            let section = rule.name.strip_prefix("hotsheet-").unwrap_or(&rule.name);
            candidates.entry(section).or_default().push(rule);
        }
    }
    for (name, offered) in candidates {
        let selected_entry = active
            .then(|| {
                offered
                    .iter()
                    .find_map(|rule| selected.get(rule.name.as_str()).copied())
            })
            .flatten();
        let chosen =
            selected_entry.map(|(source, rule)| crate::guidance_sources::chosen_rule(source, rule));
        let desired = chosen.as_ref().map(|choice| choice.body.trim());
        let Some((start, finish)) = guidance_bounds(&rendered, name) else {
            if let Some(body) = desired {
                let marker = format!("<!-- hotsheet:begin section={name} v=");
                if !rendered.contains(&marker) {
                    rendered = append_block(&rendered, body);
                }
            }
            continue;
        };
        let installed = &rendered[start..finish];
        let Some((generic, specifics)) = guidance_parts(installed, name) else {
            continue;
        };
        let known = offered.iter().any(|rule| {
            generic == rule.body.trim()
                || rule.known_bodies.iter().any(|body| generic == body.trim())
        }) || GUIDANCE_SECTIONS.iter().any(|section| {
            section.name == name
                && (generic == section.current.trim()
                    || section.legacy.iter().any(|old| generic == old.trim()))
        });
        if !known {
            continue;
        }
        match desired {
            Some(body) if generic != body => {
                let selected_version = chosen.as_ref().unwrap().version;
                if marked_version(installed, &format!("<!-- hotsheet:begin section={name} v="))
                    .is_some_and(|version| {
                        version > selected_version
                            && selected_entry.is_none_or(|(_, rule)| rule.selected_sha256.is_none())
                    })
                {
                    continue;
                }
                rendered.replace_range(
                    start..finish,
                    &guidance_with_specifics(body, name, specifics),
                );
            }
            None if specifics.is_none() => rendered = remove_range(&rendered, (start, finish)),
            _ => {}
        }
    }
    // Once a project opts into explicit selection, historical bundled sections not offered
    // by any currently registered source remain protected rather than guessed away.
    rendered
}

fn write_guidance_target(
    fs: &mut PlanFs,
    project: &Path,
    rel: &str,
    active: bool,
) -> Result<(), SetupError> {
    let path = project.join(rel);
    let existing = fs.read_to_string(&path).unwrap_or_default();
    let mut rendered = render_guidance_sections(&existing, active);
    if rel == "AGENTS.md" {
        let config = crate::guidance_sources::load(project)?;
        rendered = render_selected_guidance(&rendered, &config, true);
    }
    if rendered == existing {
        return Ok(());
    }
    if rendered.trim().is_empty() {
        return remove_managed_file(fs, project, rel, false).map(|_| ());
    }
    fs.write(&path, rendered.as_bytes());
    Ok(())
}

fn marked_version(contents: &str, prefix: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        line.trim()
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(" -->"))
            .and_then(|value| value.parse().ok())
    })
}

/// Whether the installed per-tool `hotsheet:<id>` section (the first one in `instructions`)
/// is newer than, or an equal-version variant of, the bundled section.
fn per_tool_section_requires_preservation(plugin: &Plugin, instructions: Option<&str>) -> bool {
    let bundled_instruction_version =
        marked_version(plugin.instructions_body(), INSTRUCTIONS_VERSION_PREFIX).unwrap_or(0);
    let (begin, end) = (begin_marker(plugin.id()), end_marker(plugin.id()));
    let bundled_instruction_block =
        format!("{begin}\n{}\n{end}", plugin.instructions_body().trim_end());
    instructions
        .and_then(|contents| {
            let (start, finish) = find_block(contents, &begin, &end)?;
            Some(&contents[start..finish])
        })
        .is_some_and(|installed| {
            marked_artifact_requires_preservation(
                installed,
                &bundled_instruction_block,
                bundled_instruction_version,
                INSTRUCTIONS_VERSION_PREFIX,
            )
        })
}

/// Whether the installed worklist skill is newer than, or an equal-version variant of,
/// the bundled skill.
fn skill_requires_preservation(fs: &PlanFs, project: &Path, plugin: &Plugin) -> bool {
    plugin.skill().is_some_and(|(target, bundled)| {
        let bundled_version = marked_version(bundled, SKILL_VERSION_PREFIX).unwrap_or(0);
        fs.read_to_string(&project.join(target))
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

fn installed_workflow_requires_preservation(fs: &PlanFs, project: &Path, plugin: &Plugin) -> bool {
    if let Some(canonical) = &plugin.manifest.instructions.canonical {
        if canonical.target != plugin.manifest.instructions.target {
            let mut source_plugin = plugin.clone();
            source_plugin.manifest.instructions.target = canonical.target.clone();
            source_plugin.manifest.instructions.canonical = None;
            if installed_workflow_requires_preservation(fs, project, &source_plugin) {
                return true;
            }
        }
    }
    let bundled_instruction_version =
        marked_version(plugin.instructions_body(), INSTRUCTIONS_VERSION_PREFIX).unwrap_or(0);
    let instructions = fs
        .read_to_string(&project.join(&plugin.manifest.instructions.target))
        .ok();
    let is_reference = instructions.as_ref().is_some_and(|contents| {
        instruction_reference_block(plugin).is_some_and(|block| contents.contains(&block))
    });
    if !is_reference && per_tool_section_requires_preservation(plugin, instructions.as_deref()) {
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
    skill_requires_preservation(fs, project, plugin)
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
        (version_prefix == INSTRUCTIONS_VERSION_PREFIX
            && bundled_version == 65
            && installed_version.checked_add(1) == Some(bundled_version)
            && !push_policy_only_diff(installed, bundled))
            || installed_version > bundled_version
            || (installed_version == bundled_version
                && installed != bundled
                && !push_policy_only_diff(installed, bundled))
    })
}

/// A changed declared policy owns only this generated span. An edit anywhere else
/// at the same artifact version is still a customization and must be preserved.
fn push_policy_only_diff(installed: &str, bundled: &str) -> bool {
    fn without_policy(contents: &str) -> Option<String> {
        let mut normalized = contents.to_string();
        if let Some(begin) = normalized.find(POLICY_BEGIN) {
            let end = normalized[begin..].find(POLICY_END)? + begin + POLICY_END.len();
            normalized.replace_range(begin..end, "{{HOTSHEET_PUSH_POLICY}}");
        } else {
            normalized = normalized.replace(LEGACY_POLICY, "{{HOTSHEET_PUSH_POLICY}}");
        }
        normalized = normalized.replace(
            "<!-- hotsheet-instructions-version: 64 -->",
            "<!-- hotsheet-instructions-version: 65 -->",
        );
        Some(normalized)
    }
    without_policy(installed)
        .zip(without_policy(bundled))
        .is_some_and(|(a, b)| a == b)
}

/// The `hotsheet-mcp` command string to record in a tool's MCP config. An absolute, existing
/// `HOTSHEET_MCP_BIN` wins (a host that runs the server from a private snapshot names the
/// stable shim it built, HS2-HNXE38); else the absolute sibling next to the running binary
/// when it exists (so the config needs no PATH munging — HS2-103); else the bare `fallback`
/// (resolved via PATH at launch, which can pick up a stale install).
pub fn mcp_command(fallback: &str) -> String {
    mcp_command_for(
        std::env::var_os("HOTSHEET_MCP_BIN")
            .as_deref()
            .map(Path::new),
        std::env::current_exe()
            .ok()
            .as_deref()
            .and_then(Path::parent),
        fallback,
    )
}

fn mcp_command_for(explicit: Option<&Path>, exe_dir: Option<&Path>, fallback: &str) -> String {
    explicit
        .filter(|p| p.is_absolute() && p.is_file())
        .map(Path::to_path_buf)
        .or_else(|| {
            exe_dir
                .map(|d| d.join("hotsheet-mcp"))
                .filter(|p| p.is_file())
        })
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
    fs: &mut PlanFs,
    project: &Path,
    rel: &str,
    writers: &[&Plugin],
    refresh_members: Option<&HashSet<String>>,
) -> Result<(), SetupError> {
    let target = project.join(rel);
    let existing = fs.read_to_string(&target).ok();
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
    // Retiring the last managed section of a file setup created leaves nothing of the
    // user's, so the file goes rather than lingering empty (HS2-G9CD1W).
    if rendered.trim().is_empty() {
        return remove_managed_file(fs, project, rel, false).map(|_| ());
    }
    fs.write(&target, rendered.as_bytes());
    Ok(())
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
fn write_skill(fs: &mut PlanFs, project: &Path, p: &Plugin) -> Result<Option<String>, SetupError> {
    match p.skill() {
        Some((target, body)) => {
            fs.write(&project.join(target), body.as_bytes());
            Ok(Some(target.to_string()))
        }
        None => Ok(None),
    }
}

/// Register the `hotsheet-mcp` server in the tool's MCP config, merge-safe. The writer is
/// chosen by the manifest's `format` (a host helper keyed on the format, not the tool id —
/// docs/05 §5.3), so a new tool with a known format needs no code.
fn write_mcp(
    fs: &mut PlanFs,
    project: &Path,
    store_abs: &Path,
    p: &Plugin,
) -> Result<String, SetupError> {
    let rel = &p.manifest.mcp.target;
    let target = project.join(rel);
    let name = &p.manifest.mcp.server_name;
    let (command, args, locally_owned) = mcp_entry(fs, project, store_abs, p);

    match p.manifest.mcp.format.as_str() {
        "claude-json" => write_mcp_json(fs, &target, name, &command, &args)?,
        "codex-toml" => write_mcp_toml(fs, &target, name, &command, &args)?,
        "opencode-json" => write_mcp_opencode_json(fs, &target, name, &command, &args)?,
        other => {
            return Err(SetupError::UnknownMcpFormat {
                id: p.id().to_string(),
                format: other.to_string(),
            });
        }
    }
    if locally_owned {
        ensure_local_git_exclude(fs, project, rel)?;
    }
    Ok(rel.clone())
}

/// A shared project config may be committed, so its Hot Sheet entry must contain no
/// host-specific binary or ticket-store path. The ignored, Hot Sheet-only file keeps
/// absolute paths so GUI launches do not depend on a shell PATH or working directory.
fn mcp_entry(
    fs: &PlanFs,
    project: &Path,
    store_abs: &Path,
    p: &Plugin,
) -> (String, Vec<String>, bool) {
    let target = project.join(&p.manifest.mcp.target);
    let locally_owned = !fs.exists(&target)
        || is_hotsheet_only_mcp_config(
            fs,
            &target,
            &p.manifest.mcp.format,
            &p.manifest.mcp.server_name,
        );
    if locally_owned {
        let project_abs = project
            .canonicalize()
            .unwrap_or_else(|_| project.to_path_buf());
        let path = if p.is_builtin() {
            &project_abs
        } else {
            store_abs
        };
        (
            mcp_command(&p.manifest.mcp.command),
            p.mcp_args(&path.to_string_lossy()),
            true,
        )
    } else {
        (p.manifest.mcp.command.clone(), p.mcp_args("."), false)
    }
}

fn is_hotsheet_only_mcp_config(fs: &PlanFs, target: &Path, format: &str, name: &str) -> bool {
    let Ok(contents) = fs.read_to_string(target) else {
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

/// The `info/exclude` git actually reads for this checkout. A linked worktree's gitdir
/// (`.git/worktrees/<name>`) carries a `commondir` pointer, and git reads `info/exclude`
/// only from that common directory, so an entry in the per-worktree gitdir is ignored and
/// the machine-local config would show up as untracked (HS2-8XTDZP).
fn project_exclude_file(project: &Path) -> Option<std::path::PathBuf> {
    let git_dir = project_git_dir(project)?;
    let common = std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|pointer| git_dir.join(pointer.trim()))
        .unwrap_or(git_dir);
    Some(common.join("info/exclude"))
}

/// Keep a wholly Hot Sheet-owned machine config out of this checkout without changing its
/// shared `.gitignore`. Existing mixed/user-owned configs remain visible to git.
fn ensure_local_git_exclude(fs: &mut PlanFs, project: &Path, rel: &str) -> Result<(), SetupError> {
    let Some(target) = project_exclude_file(project) else {
        return Ok(());
    };
    let entry = format!("/{}", rel.replace('\\', "/"));
    let existing = fs.read_to_string(&target).unwrap_or_default();
    if existing.lines().any(|line| line == entry) {
        return Ok(());
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    fs.write(
        &target,
        format!("{existing}{separator}{entry}\n").as_bytes(),
    );
    Ok(())
}

/// Drop the `/<rel>` line setup added to this checkout's `.git/info/exclude` for a wholly
/// Hot Sheet-owned config that has now been deleted.
fn remove_local_git_exclude(fs: &mut PlanFs, project: &Path, rel: &str) -> Result<(), SetupError> {
    let Some(target) = project_exclude_file(project) else {
        return Ok(());
    };
    let Ok(existing) = fs.read_to_string(&target) else {
        return Ok(());
    };
    let entry = format!("/{}", rel.replace('\\', "/"));
    if !existing.lines().any(|line| line == entry) {
        return Ok(());
    }
    let kept: String = existing
        .lines()
        .filter(|line| *line != entry)
        .map(|line| format!("{line}\n"))
        .collect();
    fs.write(&target, kept.as_bytes());
    Ok(())
}

/// Register the tool's permission hook (`docs/05` §5.7) in its config, if it declares one.
/// Claude and Codex both currently document this lifecycle JSON shape:
/// `{ "hooks": { "<event>": [ { "matcher": "*", "hooks": [ { "type": "command", "command": … } ] } ] } }`.
/// Merge-safe + idempotent: an existing Hot Sheet hook (same resolved command) is not
/// duplicated. Returns the written path when a hook was registered.
fn write_hooks(
    fs: &mut PlanFs,
    project: &Path,
    p: &Plugin,
) -> Result<Option<(String, bool)>, SetupError> {
    let Some(spec) = &p.manifest.hooks else {
        return Ok(None);
    };
    let target = project.join(&spec.target);
    let locally_owned = spec.machine_local || !fs.exists(&target);
    let command = resolve_hook_command(&spec.command);

    if spec.format != "lifecycle-json" {
        return Err(SetupError::UnknownHookFormat {
            id: p.id().to_string(),
            format: spec.format.clone(),
        });
    }

    let mut root: serde_json::Value = fs
        .read_to_string(&target)
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
    let mut events = vec![spec.event.as_str()];
    for event in &spec.additional_events {
        if !events.contains(&event.as_str()) {
            events.push(event);
        }
    }
    for event_name in events {
        let mut handler = serde_json::json!({ "type": "command", "command": command });
        if let Some(timeout) = spec
            .event_timeout_seconds
            .get(event_name)
            .copied()
            .or(spec.timeout_seconds)
        {
            handler["timeout"] = serde_json::json!(timeout);
        }
        let matcher = spec.event_matchers.get(event_name).unwrap_or(&spec.matcher);
        let mut entry = serde_json::json!({ "hooks": [handler] });
        if !matcher.is_empty() {
            entry["matcher"] = serde_json::json!(matcher);
        }
        let event = hooks
            .entry(event_name.to_string())
            .or_insert_with(|| serde_json::json!([]));
        if !event.is_array() {
            *event = serde_json::json!([]);
        }
        event.as_array_mut().unwrap().push(entry);
    }

    let rendered = serde_json::to_string_pretty(&root).unwrap() + "\n";
    let changed = !fs
        .read(&target)
        .is_ok_and(|existing| existing == rendered.as_bytes());
    if locally_owned {
        ensure_local_git_exclude(fs, project, &spec.target)?;
    }
    fs.write(&target, rendered.as_bytes());
    Ok(Some((spec.target.clone(), changed)))
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
                    .is_some_and(|c| c.split_whitespace().any(|part| part == "permission-hook"))
            })
        })
}

/// Resolve a hook command's binary (its first token) to a stable CLI path when the local host
/// runs a temporary snapshot. Otherwise use the absolute sibling next to the running binary
/// (no PATH reliance, HS2-103), keeping the rest of the args.
fn resolve_hook_command(command: &str) -> String {
    let configured = std::env::var_os("HOTSHEET_HOOK_CLI_BIN").map(std::path::PathBuf::from);
    let home = std::env::var_os("HOTSHEET_HOME").map(std::path::PathBuf::from);
    let snapshot = std::env::var_os("HOTSHEET_LOCAL_HOST_SNAPSHOT").as_deref()
        == Some(std::ffi::OsStr::new("1"))
        || std::env::current_exe()
            .ok()
            .is_some_and(|path| is_local_host_snapshot_binary(&path));
    let stable = stable_hook_cli(configured.as_deref(), snapshot, home.as_deref()).or_else(|| {
        snapshot
            .then(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| snapshot_hook_cli(&exe))
            })
            .flatten()
    });
    resolve_hook_command_with(command, stable.as_deref())
}

fn stable_hook_cli(
    configured: Option<&Path>,
    snapshot: bool,
    home: Option<&Path>,
) -> Option<std::path::PathBuf> {
    snapshot
        .then(|| home.map(|home| home.join("bin/hotsheet-cli")))
        .flatten()
        .filter(|path| path.is_file())
        .or_else(|| {
            configured
                .filter(|path| {
                    path.is_absolute()
                        && path.is_file()
                        && !(snapshot && is_local_host_snapshot_binary(path))
                })
                .map(Path::to_path_buf)
        })
}

fn is_local_host_snapshot_binary(path: &Path) -> bool {
    path.parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .is_some_and(|part| {
            let name = part.to_string_lossy();
            name.starts_with("hotsheet-local-host-")
                && !name.starts_with("hotsheet-local-host-home-")
        })
}

fn snapshot_hook_cli(exe: &Path) -> Option<std::path::PathBuf> {
    let target = std::fs::read_to_string(exe.parent()?.join(".stable-hook-cli")).ok()?;
    let target = Path::new(target.trim());
    (target.is_absolute() && target.is_file()).then(|| target.to_path_buf())
}

fn resolve_hook_command_with(command: &str, stable: Option<&Path>) -> String {
    let mut parts = command.split_whitespace();
    let Some(bin) = parts.next() else {
        return command.to_string();
    };
    let stable = (bin == "hotsheet-cli")
        .then_some(stable)
        .flatten()
        .filter(|path| path.is_absolute() && path.is_file());
    let resolved = stable
        .map(Path::to_path_buf)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .as_deref()
                .and_then(Path::parent)
                .map(|d| d.join(bin))
        })
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

/// Environment every generated MCP server entry declares. MCP configs are read only by AI
/// clients, so the shim identifies each mutation as the `ai` actor even when Hot Sheet did
/// not launch the client (HS2-28DM8B). An explicit `actor_role` argument still wins.
const MCP_ENV: &[(&str, &str)] = &[("HOTSHEET_ACTOR_ROLE", "ai")];

fn mcp_env_json() -> serde_json::Value {
    MCP_ENV
        .iter()
        .map(|(key, value)| ((*key).to_string(), serde_json::Value::from(*value)))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Claude-style `.mcp.json`: `{ "mcpServers": { "<name>": { command, args, env } } }`.
fn write_mcp_json(
    fs: &mut PlanFs,
    target: &Path,
    name: &str,
    command: &str,
    args: &[String],
) -> Result<(), SetupError> {
    let mut root: serde_json::Value = fs
        .read_to_string(target)
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
        serde_json::json!({ "command": command, "args": args, "env": mcp_env_json() }),
    );
    fs.write(
        target,
        (serde_json::to_string_pretty(&root).unwrap() + "\n").as_bytes(),
    );
    Ok(())
}

/// OpenCode project config:
/// `{ "mcp": { "<name>": { type: "local", command: [...], environment } } }`.
fn write_mcp_opencode_json(
    fs: &mut PlanFs,
    target: &Path,
    name: &str,
    command: &str,
    args: &[String],
) -> Result<(), SetupError> {
    let mut root: serde_json::Value = fs
        .read_to_string(target)
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
        serde_json::json!({ "type": "local", "command": command_line, "environment": mcp_env_json() }),
    );
    fs.write(
        target,
        (serde_json::to_string_pretty(&root).unwrap() + "\n").as_bytes(),
    );
    Ok(())
}

/// Codex-style TOML: `[mcp_servers.<name>]` with `command`, `args`, and `env`.
fn write_mcp_toml(
    fs: &mut PlanFs,
    target: &Path,
    name: &str,
    command: &str,
    args: &[String],
) -> Result<(), SetupError> {
    let mut root: toml::Table = fs
        .read_to_string(target)
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
    entry.insert(
        "env".into(),
        toml::Value::Table(
            MCP_ENV
                .iter()
                .map(|(key, value)| ((*key).to_string(), toml::Value::from(*value)))
                .collect(),
        ),
    );
    servers
        .as_table_mut()
        .unwrap()
        .insert(name.to_string(), toml::Value::Table(entry));

    fs.write(target, toml::to_string_pretty(&root).unwrap().as_bytes());
    Ok(())
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
    // HS2-QPEDJR: setup can refresh shared settings while the server reads them.
    // Replace a complete sibling instead of exposing the truncation from fs::write.
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = path
        .file_name()
        .expect("setup output has a file name")
        .to_string_lossy();
    let staged = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let result = (|| {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        file.write_all(contents.as_bytes())?;
        drop(file);
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.permissions().readonly() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "setup output is read-only",
                ));
            }
            std::fs::set_permissions(&staged, metadata.permissions())?;
        }
        std::fs::rename(&staged, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result.map_err(|source| SetupError::Io {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_019vva_preview_reuses_apply_protection_without_writing() {
        use crate::guidance_sources::{GuidanceConfig, GuidanceSource, SourceRule};
        use sha2::{Digest, Sha256};

        let project = tempfile::tempdir().unwrap();
        let old = "<!-- hotsheet:begin section=example-rule v=1 -->\nOld rule\n<!-- hotsheet:end section=example-rule -->";
        let pinned = "<!-- hotsheet:begin section=example-rule v=2 -->\nNew rule\n<!-- hotsheet:end section=example-rule -->";
        let rule = SourceRule {
            name: "example-rule".into(),
            version: 2,
            sha256: format!("{:x}", Sha256::digest(pinned.as_bytes())),
            description: "Synthetic rule".into(),
            owner: Some("example".into()),
            tags: vec!["testing".into()],
            body: pinned.into(),
            known_bodies: vec![old.into()],
            minimum_hotsheet_version: None,
            historical_versions: Vec::new(),
            selected_sha256: None,
        };
        crate::guidance_sources::save(
            project.path(),
            &GuidanceConfig {
                schema_version: 1,
                explicit_selection: true,
                sources: vec![GuidanceSource {
                    id: "fixture".into(),
                    group_id: None,
                    repository: "https://example.test/rules".into(),
                    requested_ref: "main".into(),
                    commit: "a".repeat(40),
                    rules: vec![rule],
                    skills: vec![],
                    selected_rules: vec!["example-rule".into()],
                    selected_skills: vec![],
                }],
            },
        )
        .unwrap();
        let agents = project.path().join("AGENTS.md");
        let added = preview_guidance_rule_in(project.path(), "fixture", "example-rule").unwrap();
        assert_eq!(added.outcome, "add");
        assert!(added.current.is_none());
        assert_eq!(added.after.as_deref(), Some(pinned));
        assert!(!agents.exists());

        std::fs::write(&agents, old).unwrap();
        let replacement =
            preview_guidance_rule_in(project.path(), "fixture", "example-rule").unwrap();
        assert_eq!(replacement.outcome, "replace");
        assert_eq!(replacement.current.as_deref(), Some(old));
        assert_eq!(replacement.after.as_deref(), Some(pinned));
        assert_eq!(std::fs::read_to_string(&agents).unwrap(), old);

        let edited = old.replace("Old rule", "Local edit");
        std::fs::write(&agents, &edited).unwrap();
        let protected =
            preview_guidance_rule_in(project.path(), "fixture", "example-rule").unwrap();
        assert_eq!(protected.outcome, "protected");
        assert_eq!(protected.after.as_deref(), Some(edited.as_str()));

        std::fs::write(&agents, pinned).unwrap();
        assert_eq!(
            preview_guidance_rule_in(project.path(), "fixture", "example-rule")
                .unwrap()
                .outcome,
            "current"
        );

        std::fs::write(
            project.path().join(GUIDANCE_OWNER_PATH),
            r#"{"schema_version":1,"owner":"rulesync"}"#,
        )
        .unwrap();
        let curated = project
            .path()
            .join(".rulesync/rules/.curated/example-rule.md");
        assert_eq!(
            preview_guidance_rule_in(project.path(), "fixture", "example-rule")
                .unwrap()
                .outcome,
            "add"
        );
        std::fs::create_dir_all(curated.parent().unwrap()).unwrap();
        std::fs::write(
            &curated,
            format!("---\nroot: true\ndescription: \"Synthetic rule\"\n---\n\n{old}"),
        )
        .unwrap();
        let rulesync = preview_guidance_rule_in(project.path(), "fixture", "example-rule").unwrap();
        assert_eq!(rulesync.outcome, "replace");
        assert_eq!(rulesync.after.as_deref(), Some(pinned));
        std::fs::write(&curated, "locally changed without frontmatter").unwrap();
        let protected =
            preview_guidance_rule_in(project.path(), "fixture", "example-rule").unwrap();
        assert_eq!(protected.outcome, "protected");
        assert_eq!(
            protected.current.as_deref(),
            Some("locally changed without frontmatter")
        );
    }

    #[test]
    fn push_policy_setup_defaults_and_diagnosis_plan_stays_read_only() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let settings = project.path().join(SHARED_SETTINGS_PATH);
        plan_refresh_setup_in(store.path(), project.path(), None, &[]).unwrap();
        assert!(!settings.exists());

        run_setup_in(
            store.path(),
            project.path(),
            Some("codex"),
            false,
            None,
            &[],
        )
        .unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
        assert_eq!(saved[PUSH_POLICY_SETTING], "push-per-ticket");
        let skill =
            std::fs::read_to_string(project.path().join(".agents/skills/hotsheet/SKILL.md"))
                .unwrap();
        assert!(skill.contains("Push policy: `push-per-ticket`"));
        assert!(!skill.contains("{{HOTSHEET_PUSH_POLICY}}"));
        let instructions = std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
        assert!(instructions.contains("Push policy: `push-per-ticket`"));
    }

    #[test]
    fn hs2_0f2qc2_setup_preserves_shared_settings_written_after_planning() {
        let project = tempfile::tempdir().unwrap();
        let mut plan = PlanFs::default();
        ensure_push_policy(&mut plan, project.path()).unwrap();
        let operations = plan.final_operations(project.path());

        let settings = Settings::for_project(project.path());
        settings
            .set(
                "concurrent_key",
                serde_json::json!("new value"),
                Scope::Shared,
            )
            .unwrap();
        apply_operations(operations).unwrap();

        let saved = shared_settings(&PlanFs::default(), project.path()).unwrap();
        assert_eq!(saved[PUSH_POLICY_SETTING], "push-per-ticket");
        assert_eq!(saved["concurrent_key"], "new value");
    }

    #[test]
    fn hs2_0f2qc2_changed_push_policy_rejects_stale_plan_before_any_writes() {
        let project = tempfile::tempdir().unwrap();
        let mut plan = PlanFs::default();
        ensure_push_policy(&mut plan, project.path()).unwrap();
        let guidance = project.path().join("AGENTS.md");
        plan.write(&guidance, b"planned guidance");
        let operations = plan.final_operations(project.path());

        let settings = Settings::for_project(project.path());
        settings
            .set(
                PUSH_POLICY_SETTING,
                serde_json::json!("push-per-batch"),
                Scope::Shared,
            )
            .unwrap();
        assert!(matches!(
            apply_operations(operations),
            Err(SetupError::StalePushPolicy)
        ));
        assert!(!guidance.exists());
        let saved = shared_settings(&PlanFs::default(), project.path()).unwrap();
        assert_eq!(saved[PUSH_POLICY_SETTING], "push-per-batch");
    }

    #[test]
    fn push_policy_changes_update_only_the_managed_policy_span() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join(SHARED_SETTINGS_PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{\"$hotsheetSchema\":1,\"push_policy\":\"push-per-batch\",\"push_policy_reason\":\"Measured seven-minute pre-push gate\"}\n").unwrap();
        run_setup_in(
            store.path(),
            project.path(),
            Some("codex"),
            false,
            None,
            &[],
        )
        .unwrap();
        let skill_path = project.path().join(".agents/skills/hotsheet/SKILL.md");
        let agents_path = project.path().join("AGENTS.md");
        assert!(
            std::fs::read_to_string(&skill_path)
                .unwrap()
                .contains("Push policy: `push-per-batch`")
        );
        let agents = std::fs::read_to_string(&agents_path).unwrap();
        std::fs::write(&agents_path, format!("{agents}\nProject-specific note.\n")).unwrap();

        std::fs::write(
            &path,
            "{\"$hotsheetSchema\":1,\"push_policy\":\"push-per-ticket\"}\n",
        )
        .unwrap();
        run_setup_in(
            store.path(),
            project.path(),
            Some("codex"),
            false,
            None,
            &[],
        )
        .unwrap();
        let skill = std::fs::read_to_string(skill_path).unwrap();
        let agents = std::fs::read_to_string(agents_path).unwrap();
        assert!(skill.contains("Push policy: `push-per-ticket`"));
        assert!(!skill.contains("Push policy: `push-per-batch`"));
        assert!(agents.contains("Push policy: `push-per-ticket`"));
        assert!(agents.contains("Project-specific note."));
    }

    #[test]
    fn push_policy_transition_updates_a_shared_instruction_section_and_both_skills() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let settings = project.path().join(SHARED_SETTINGS_PATH);
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "{\"push_policy\":\"push-per-batch\"}\n").unwrap();
        for tool in ["claude", "codex"] {
            run_setup_in(store.path(), project.path(), Some(tool), false, None, &[]).unwrap();
        }
        refresh_setup_in(store.path(), project.path(), None, &[]).unwrap();
        let agents = std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("hotsheet-shared-section"));
        assert!(agents.contains("Push policy: `push-per-batch`"));

        std::fs::write(&settings, "{\"push_policy\":\"push-per-ticket\"}\n").unwrap();
        refresh_setup_in(store.path(), project.path(), None, &[]).unwrap();
        let agents = std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
        assert_eq!(agents.matches("Push policy: `push-per-ticket`").count(), 1);
        assert!(!agents.contains("Push policy: `push-per-batch`"));
        for path in [
            ".claude/skills/hotsheet/SKILL.md",
            ".agents/skills/hotsheet/SKILL.md",
        ] {
            let skill = std::fs::read_to_string(project.path().join(path)).unwrap();
            assert!(skill.contains("Push policy: `push-per-ticket`"));
        }
    }

    #[test]
    fn invalid_push_policy_fails_setup_explicitly() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join(SHARED_SETTINGS_PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{\"push_policy\":\"auto\"}\n").unwrap();
        assert!(matches!(
            plan_refresh_setup_in(store.path(), project.path(), None, &[]),
            Err(SetupError::InvalidPushPolicy(_))
        ));
    }

    #[test]
    fn bundled_guidance_notice_only_reports_preserved_older_custom_sections() {
        let section = &GUIDANCE_SECTIONS[0];
        let current = section.current.trim();
        let old = format!(
            "<!-- hotsheet:begin section={} v=1 -->\n\nLocal wording\n\n<!-- hotsheet:end section={} -->",
            section.name, section.name
        );
        let notices = blocked_bundled_guidance(&old, "AGENTS.md");
        assert_eq!(notices.len(), 1);
        assert!(matches!(
            &notices[0],
            SetupNotice::BundledGuidanceBlocked {
                path,
                section,
                installed_version: 1,
                bundled_version: 6,
            } if path == "AGENTS.md" && section == "ticket-driven-work"
        ));
        assert!(render_guidance_sections(&old, true).contains(&old));
        assert!(blocked_bundled_guidance(current, "AGENTS.md").is_empty());
        assert!(blocked_bundled_guidance(section.legacy[0], "AGENTS.md").is_empty());
        let equal_version_edit = current.replace("Ticket-Driven Work", "Local title");
        assert!(blocked_bundled_guidance(&equal_version_edit, "AGENTS.md").is_empty());
        let newer = current.replace(
            "section=ticket-driven-work v=6",
            "section=ticket-driven-work v=7",
        );
        assert!(blocked_bundled_guidance(&newer, "AGENTS.md").is_empty());
    }

    #[test]
    fn codex_hook_command_uses_cli_that_outlives_a_host_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let stable = root.path().join("stable/hotsheet-cli");
        let snapshot = root.path().join("snapshot/hotsheet-cli");
        std::fs::create_dir_all(stable.parent().unwrap()).unwrap();
        std::fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
        std::fs::write(&stable, b"stable").unwrap();
        std::fs::write(&snapshot, b"snapshot").unwrap();

        let command =
            resolve_hook_command_with("hotsheet-cli permission-hook --agent codex", Some(&stable));
        std::fs::remove_dir_all(snapshot.parent().unwrap()).unwrap();
        assert_eq!(
            command,
            format!("{} permission-hook --agent codex", stable.display())
        );
        assert!(stable.is_file());
        assert_eq!(
            resolve_hook_command_with("other-hook --flag", Some(&stable)),
            "other-hook --flag"
        );
    }

    #[test]
    fn production_snapshot_finds_the_durable_hook_cli_when_the_override_is_missing() {
        let home = tempfile::tempdir().unwrap();
        let durable = home.path().join("bin/hotsheet-cli");
        std::fs::create_dir_all(durable.parent().unwrap()).unwrap();
        std::fs::write(&durable, b"durable").unwrap();
        assert_eq!(
            stable_hook_cli(None, true, Some(home.path())),
            Some(durable.clone())
        );
        assert_eq!(stable_hook_cli(None, false, Some(home.path())), None);
        assert_eq!(
            resolve_hook_command_with(
                "hotsheet-cli permission-hook --agent codex",
                stable_hook_cli(None, true, Some(home.path())).as_deref(),
            ),
            format!("{} permission-hook --agent codex", durable.display())
        );
        let snapshot = home.path().join("hotsheet-local-host-abc/bin/hotsheet-cli");
        std::fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
        std::fs::write(&snapshot, b"snapshot").unwrap();
        assert!(is_local_host_snapshot_binary(&snapshot));
        assert!(!is_local_host_snapshot_binary(&durable));
        std::fs::write(
            snapshot.parent().unwrap().join(".stable-hook-cli"),
            durable.to_string_lossy().as_bytes(),
        )
        .unwrap();
        assert_eq!(snapshot_hook_cli(&snapshot), Some(durable.clone()));
        assert_eq!(stable_hook_cli(Some(&snapshot), true, None), None);
        assert_eq!(
            stable_hook_cli(Some(&snapshot), true, Some(home.path())),
            Some(durable)
        );
    }

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
        sharing_tool_detected_by(
            root,
            id,
            instructions,
            version,
            &format!("definitely-not-installed-hotsheet-{id}"),
        );
    }

    /// A shell every test machine has on `PATH`, so a fixture detected by it is always
    /// detected (refresh then sets it up whenever it is enabled).
    const PRESENT_BINARY: &str = if cfg!(windows) { "cmd" } else { "sh" };

    fn sharing_tool_detected_by(
        root: &Path,
        id: &str,
        instructions: &str,
        version: u64,
        binary: &str,
    ) {
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
binaries = ["{binary}"]
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

        fn refresh(&self, enabled: &[&str]) -> RefreshReport {
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
    fn shared_guidance_preserves_local_text_and_version_boundaries() {
        for section in GUIDANCE_SECTIONS {
            let current = section.current.trim();
            let specifics = format!(
                "<!-- hotsheet:begin specifics={} v=7 -->\nLocal policy.\n<!-- hotsheet:end specifics={} -->",
                section.name, section.name
            );
            for legacy in section.legacy {
                let legacy = legacy.trim();
                let older_with_specifics =
                    guidance_with_specifics(legacy, section.name, Some(&specifics));
                let upgraded = render_guidance_sections(
                    &format!("User preface.\n\n{older_with_specifics}\n\nUser footer.\n"),
                    true,
                );
                assert!(upgraded.contains(&guidance_with_specifics(
                    current,
                    section.name,
                    Some(&specifics)
                )));
                assert!(upgraded.starts_with("User preface.\n"));
                assert!(upgraded.contains("User footer.\n"));
                assert_eq!(render_guidance_sections(&upgraded, true), upgraded);
                let customized_older = legacy.replace("## ", "## Customized ");
                let input = format!("Before.\n\n{customized_older}\n\nAfter.\n");
                assert!(render_guidance_sections(&input, true).contains(&customized_older));
            }

            for protected in [
                current.replace("## ", "## Customized "),
                current.replace(" v=", " v=99"),
                guidance_with_specifics(current, section.name, Some(&specifics)).replace(
                    &format!("<!-- hotsheet:end specifics={} -->", section.name),
                    "",
                ),
            ] {
                let input = format!("Before.\n\n{protected}\n\nAfter.\n");
                let rendered = render_guidance_sections(&input, true);
                assert!(
                    rendered.contains(&protected),
                    "protected section was replaced"
                );
            }
            let retired =
                render_guidance_sections(&format!("Before.\n\n{current}\n\nAfter.\n"), false);
            assert_eq!(retired, "Before.\n\nAfter.\n");
        }
    }

    #[test]
    fn refresh_reports_only_a_blocked_newer_bundled_guidance_section() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        run_setup_in(store.path(), root, Some("codex"), false, None, &[]).unwrap();
        let agents_path = root.join("AGENTS.md");
        let agents = std::fs::read_to_string(&agents_path).unwrap();
        let section = &GUIDANCE_SECTIONS[0];
        let old = format!(
            "<!-- hotsheet:begin section={} v=1 -->\n\nLocal wording\n\n<!-- hotsheet:end section={} -->",
            section.name, section.name
        );
        std::fs::write(&agents_path, agents.replace(section.current.trim(), &old)).unwrap();
        let enabled = HashSet::from(["codex".to_string()]);
        let report = refresh_setup_in(store.path(), root, Some(&enabled), &[]).unwrap();
        assert!(report.set_up.iter().flat_map(|tool| &tool.notices).any(|notice| {
            matches!(notice, SetupNotice::BundledGuidanceBlocked { section, .. } if section == "ticket-driven-work")
        }));
        assert!(
            std::fs::read_to_string(&agents_path)
                .unwrap()
                .contains(&old)
        );
        let restored = std::fs::read_to_string(&agents_path)
            .unwrap()
            .replace(&old, section.current.trim());
        std::fs::write(&agents_path, restored).unwrap();
        let report = refresh_setup_in(store.path(), root, Some(&enabled), &[]).unwrap();
        assert!(
            !report
                .set_up
                .iter()
                .flat_map(|tool| &tool.notices)
                .any(|notice| { matches!(notice, SetupNotice::BundledGuidanceBlocked { .. }) })
        );
    }

    #[test]
    fn rulesync_ownership_preserves_instructions_across_setup_disable_and_return() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        for tool in ["claude", "codex"] {
            run_setup_in(store.path(), root, Some(tool), false, None, &[]).unwrap();
        }
        let agents = format!(
            "Rulesync project policy.\n{}",
            std::fs::read_to_string(root.join("AGENTS.md")).unwrap()
        );
        std::fs::write(root.join("AGENTS.md"), &agents).unwrap();
        let native = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        set_guidance_owner_in(root, "rulesync").unwrap();
        let enabled = HashSet::from(["claude".to_string(), "codex".to_string()]);
        for _ in 0..2 {
            run_setup_in(store.path(), root, Some("claude"), false, None, &[]).unwrap();
            refresh_setup_in(store.path(), root, Some(&enabled), &[]).unwrap();
            assert_eq!(
                std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
                agents
            );
            assert_eq!(
                std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
                native
            );
        }
        let plan = plan_refresh_setup_in(store.path(), root, Some(&enabled), &[]).unwrap();
        assert!(
            plan.tools
                .iter()
                .flat_map(|tool| &tool.artifacts)
                .filter(|artifact| artifact.kind == ArtifactKind::Instructions)
                .all(|artifact| artifact.status == ArtifactStatus::Preserved
                    && artifact.detail.contains("Rulesync"))
        );
        refresh_setup_in(store.path(), root, Some(&HashSet::new()), &[]).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
            agents
        );
        assert_eq!(
            std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
            native
        );
        assert!(!root.join(".claude/skills/hotsheet/SKILL.md").exists());
        set_guidance_owner_in(root, "builtin").unwrap();
        for tool in ["claude", "codex"] {
            run_setup_in(store.path(), root, Some(tool), false, None, &[]).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
            agents
        );
        assert_eq!(
            std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
            native
        );
        assert!(root.join(".claude/skills/hotsheet/SKILL.md").exists());
    }

    #[test]
    fn hs2_h99rrw_switch_to_rulesync_saves_unmarked_and_nested_local_guidance() {
        let project = tempfile::tempdir().unwrap();
        let agents = "# Local policy\n\nKeep this project rule.\n\n<!-- hotsheet:begin section=testing-philosophy v=6 -->\nGenerated policy.\n<!-- hotsheet:begin specifics=testing-philosophy v=1 -->\n## Local test setup\nUse isolated fixtures.\n<!-- hotsheet:end specifics=testing-philosophy -->\n<!-- hotsheet:end section=testing-philosophy -->\n\n<!-- BEGIN hotsheet:codex -->\nGenerated tool section.\n<!-- END hotsheet:codex -->\n\nLast local line.\n";
        std::fs::write(project.path().join("AGENTS.md"), agents).unwrap();
        set_guidance_owner_in(project.path(), "rulesync").unwrap();
        let rule = std::fs::read_to_string(project.path().join(LOCAL_RULE_PATH)).unwrap();
        assert_eq!(
            local_rules_in(project.path()).unwrap(),
            vec!["project-local"]
        );
        assert!(rule.contains("# Local policy\n\nKeep this project rule."));
        assert!(rule.contains("Last local line."));
        assert!(rule.contains("## Local test setup\nUse isolated fixtures."));
        assert!(!rule.contains("Generated policy."));
        assert!(!rule.contains("Generated tool section."));
        let original = std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
        set_guidance_owner_in(project.path(), "rulesync").unwrap();
        assert_eq!(
            std::fs::read_to_string(project.path().join(LOCAL_RULE_PATH)).unwrap(),
            rule
        );
        assert_eq!(
            std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap(),
            original
        );
    }

    #[test]
    fn hs2_h99rrw_switch_to_rulesync_fails_closed_on_conflict_or_bad_markers() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("AGENTS.md"), "Local policy.\n").unwrap();
        std::fs::create_dir_all(project.path().join(".rulesync/rules")).unwrap();
        std::fs::write(
            project.path().join(LOCAL_RULE_PATH),
            "Different local rule.\n",
        )
        .unwrap();
        assert!(set_guidance_owner_in(project.path(), "rulesync").is_err());
        assert!(!project.path().join(GUIDANCE_OWNER_PATH).exists());
        std::fs::remove_file(project.path().join(LOCAL_RULE_PATH)).unwrap();
        std::fs::write(
            project.path().join("AGENTS.md"),
            "<!-- hotsheet:begin section=unknown v=1 -->\nLocal text.\n",
        )
        .unwrap();
        assert!(set_guidance_owner_in(project.path(), "rulesync").is_err());
        assert!(!project.path().join(LOCAL_RULE_PATH).exists());
        assert!(!project.path().join(GUIDANCE_OWNER_PATH).exists());
    }

    #[test]
    fn rulesync_ownership_does_not_create_missing_instruction_files_and_invalid_modes_fail() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        set_guidance_owner_in(project.path(), "rulesync").unwrap();
        run_setup_in(
            store.path(),
            project.path(),
            Some("claude"),
            false,
            None,
            &[],
        )
        .unwrap();
        assert!(!project.path().join("AGENTS.md").exists());
        assert!(!project.path().join("CLAUDE.md").exists());
        for invalid in [
            "{",
            r#"{"schema_version":2,"owner":"rulesync"}"#,
            r#"{"schema_version":1,"owner":"unknown"}"#,
        ] {
            std::fs::write(project.path().join(GUIDANCE_OWNER_PATH), invalid).unwrap();
            assert!(matches!(
                plan_refresh_setup_in(store.path(), project.path(), None, &[]),
                Err(SetupError::InvalidGuidanceOwner(_))
            ));
            assert!(!project.path().join("AGENTS.md").exists());
        }
    }

    #[test]
    fn claude_import_migrates_legacy_guidance_and_survives_disable_and_reenable() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        run_setup_in(store.path(), root, Some("codex"), false, None, &[]).unwrap();
        let claude = crate::builtin_plugins()
            .into_iter()
            .find(|plugin| plugin.id() == "claude")
            .unwrap();
        let specifics = "<!-- hotsheet:begin specifics=testing-philosophy v=1 -->\nLocal runner.\n<!-- hotsheet:end specifics=testing-philosophy -->";
        let mut legacy = format!(
            "Claude-local preface.\n\n{}\n",
            per_tool("claude", claude.instructions_body().trim_end())
        );
        for section in GUIDANCE_SECTIONS {
            let body = section.legacy.first().copied().unwrap_or(section.current);
            legacy = append_block(
                &legacy,
                &guidance_with_specifics(
                    body,
                    section.name,
                    (section.name == "testing-philosophy").then_some(specifics),
                ),
            );
        }
        std::fs::write(root.join("CLAUDE.md"), legacy).unwrap();
        run_setup_in(store.path(), root, Some("claude"), false, None, &[]).unwrap();
        let enabled = HashSet::from(["claude".to_string(), "codex".to_string()]);
        refresh_setup_in(store.path(), root, Some(&enabled), &[]).unwrap();
        let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        let adapter = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert_eq!(agents.matches("## Hot Sheet — ticket workflow").count(), 1);
        assert_eq!(agents.matches(specifics).count(), 1);
        assert!(agents.contains("hotsheet-shared-section: claude, codex"));
        assert!(adapter.starts_with("Claude-local preface.\n"));
        assert_eq!(adapter.matches("@AGENTS.md").count(), 1);
        assert!(!adapter.contains("section=testing-philosophy"));
        refresh_setup_in(store.path(), root, Some(&enabled), &[]).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
            agents
        );
        assert_eq!(
            std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
            adapter
        );
        refresh_setup_in(
            store.path(),
            root,
            Some(&HashSet::from(["codex".to_string()])),
            &[],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
            "Claude-local preface.\n"
        );
        assert!(
            std::fs::read_to_string(root.join("AGENTS.md"))
                .unwrap()
                .contains(specifics)
        );
        run_setup_in(store.path(), root, Some("claude"), false, None, &[]).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("CLAUDE.md"))
                .unwrap()
                .matches("@AGENTS.md")
                .count(),
            1
        );
    }

    #[test]
    fn claude_import_preserves_protected_legacy_workflow_and_conflicting_specifics() {
        let store = tempfile::tempdir().unwrap();
        let claude = crate::builtin_plugins()
            .into_iter()
            .find(|plugin| plugin.id() == "claude")
            .unwrap();
        for body in [
            claude
                .instructions_body()
                .replace("## Hot Sheet", "## Customized Hot Sheet"),
            claude.instructions_body().replace(
                &format!(
                    "version: {}",
                    marked_version(claude.instructions_body(), INSTRUCTIONS_VERSION_PREFIX)
                        .unwrap()
                ),
                "version: 999",
            ),
        ] {
            let project = tempfile::tempdir().unwrap();
            let legacy = per_tool("claude", body.trim_end());
            std::fs::write(project.path().join("CLAUDE.md"), &legacy).unwrap();
            run_setup_in(
                store.path(),
                project.path(),
                Some("claude"),
                false,
                None,
                &[],
            )
            .unwrap();
            let after = std::fs::read_to_string(project.path().join("CLAUDE.md")).unwrap();
            assert!(after.starts_with(&legacy));
            assert!(!after.contains("@AGENTS.md"));
            assert!(!project.path().join("AGENTS.md").exists());
        }
        let project = tempfile::tempdir().unwrap();
        let section = &GUIDANCE_SECTIONS[1];
        let local = |text| {
            format!(
                "<!-- hotsheet:begin specifics={} v=1 -->\n{text}\n<!-- hotsheet:end specifics={} -->",
                section.name, section.name
            )
        };
        let native =
            guidance_with_specifics(section.current, section.name, Some(&local("Claude runner")));
        let canonical =
            guidance_with_specifics(section.current, section.name, Some(&local("Shared runner")));
        std::fs::write(project.path().join("CLAUDE.md"), &native).unwrap();
        std::fs::write(project.path().join("AGENTS.md"), &canonical).unwrap();
        run_setup_in(
            store.path(),
            project.path(),
            Some("claude"),
            false,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap(),
            canonical
        );
        let after = std::fs::read_to_string(project.path().join("CLAUDE.md")).unwrap();
        assert!(after.contains("Claude runner"));
        assert!(!after.contains("@AGENTS.md"));
        let project = tempfile::tempdir().unwrap();
        let native = per_tool("claude", claude.instructions_body().trim_end());
        let newer = shared_block(
            "agents-md",
            &std::collections::BTreeSet::from(["codex".to_string()]),
            &claude.instructions_body().replace(
                &format!(
                    "version: {}",
                    marked_version(claude.instructions_body(), INSTRUCTIONS_VERSION_PREFIX)
                        .unwrap()
                ),
                "version: 999",
            ),
        );
        std::fs::write(project.path().join("CLAUDE.md"), &native).unwrap();
        std::fs::write(project.path().join("AGENTS.md"), &newer).unwrap();
        run_setup_in(
            store.path(),
            project.path(),
            Some("claude"),
            false,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap(),
            newer
        );
        assert!(
            !std::fs::read_to_string(project.path().join("CLAUDE.md"))
                .unwrap()
                .contains("@AGENTS.md")
        );
    }

    #[test]
    fn claude_import_accepts_an_existing_unmanaged_native_import() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("CLAUDE.md"), "@AGENTS.md\n").unwrap();
        run_setup_in(
            store.path(),
            project.path(),
            Some("claude"),
            false,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(project.path().join("CLAUDE.md")).unwrap(),
            "@AGENTS.md\n"
        );
        assert!(
            std::fs::read_to_string(project.path().join("AGENTS.md"))
                .unwrap()
                .contains("## Hot Sheet")
        );
    }

    #[test]
    fn fresh_second_project_installs_all_four_tools_and_refreshes_guidance() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let dirs = [];
        std::fs::write(project.path().join("AGENTS.md"), "Local agent text.\n").unwrap();
        std::fs::write(project.path().join("CLAUDE.md"), "Local Claude text.\n").unwrap();
        for id in ["claude", "codex", "antigravity", "opencode"] {
            run_setup_in(store.path(), project.path(), Some(id), false, None, &dirs).unwrap();
        }
        let agents_path = project.path().join("AGENTS.md");
        let claude_path = project.path().join("CLAUDE.md");
        let agents = std::fs::read_to_string(&agents_path).unwrap();
        let claude = std::fs::read_to_string(&claude_path).unwrap();
        assert!(agents.starts_with("Local agent text.\n"));
        assert!(claude.starts_with("Local Claude text.\n"));
        for section in GUIDANCE_SECTIONS {
            assert_eq!(agents.matches(section.current.trim()).count(), 1);
            assert_eq!(claude.matches(section.current.trim()).count(), 0);
        }
        assert_eq!(claude.matches("@AGENTS.md").count(), 1);
        for rule in [
            "meaningful subtask starts and finishes",
            "separate history entries",
            "`regular` Markdown notes",
            "`note_summary`",
            "`--note-file <path>`",
        ] {
            assert!(agents.contains(rule), "AGENTS.md omitted {rule}");
        }
        let enabled = ["claude", "codex", "antigravity", "opencode"]
            .into_iter()
            .map(str::to_string)
            .collect();
        refresh_setup_in(store.path(), project.path(), Some(&enabled), &dirs).unwrap();
        let refreshed_agents = std::fs::read_to_string(&agents_path).unwrap();
        let refreshed_claude = std::fs::read_to_string(&claude_path).unwrap();
        for section in GUIDANCE_SECTIONS {
            assert_eq!(refreshed_agents.matches(section.current.trim()).count(), 1);
            assert_eq!(refreshed_claude.matches(section.current.trim()).count(), 0);
        }
        refresh_setup_in(store.path(), project.path(), Some(&enabled), &dirs).unwrap();
        assert_eq!(
            std::fs::read_to_string(&agents_path).unwrap(),
            refreshed_agents
        );
        assert_eq!(
            std::fs::read_to_string(&claude_path).unwrap(),
            refreshed_claude
        );
    }

    #[test]
    fn codex_only_project_receives_visual_qa_without_claude_file() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let dirs = [];
        run_setup_in(
            store.path(),
            project.path(),
            Some("codex"),
            false,
            None,
            &dirs,
        )
        .unwrap();
        let agents_path = project.path().join("AGENTS.md");
        let first = std::fs::read_to_string(&agents_path).unwrap();
        assert!(first.contains(include_str!("../../../plugins/shared/visual-qa.md").trim()));
        assert!(
            first.contains(include_str!("../../../plugins/shared/ticket-driven-work.md").trim())
        );
        assert!(!project.path().join("CLAUDE.md").exists());
        let enabled = HashSet::from(["codex".to_string()]);
        refresh_setup_in(store.path(), project.path(), Some(&enabled), &dirs).unwrap();
        assert_eq!(std::fs::read_to_string(&agents_path).unwrap(), first);
    }

    #[test]
    fn shared_client_testing_rules_reach_claude_and_codex_without_erasing_local_setup() {
        let testing = GUIDANCE_SECTIONS
            .iter()
            .find(|section| section.name == "testing-philosophy")
            .unwrap();
        let rules = testing.current;
        for required in [
            "real server shapes, optional fields, and status codes",
            "each newly composed API surface against a real server",
            "controls in both directions",
            "Verify resets and a further edit",
            "assert live properties",
            "every part of a selected presentation",
            "through each shipped parent composition",
            "disable unsupported controls explicitly",
            // Testing effort scales with a declared risk tier (HS2-GC3D2X).
            "Scale testing to risk",
            "**T0 trivial**",
            "**T1 normal**",
            "**T2 high-risk**",
            "merge and sync logic, claims and leases, indexes, file formats and parsers",
        ] {
            assert!(rules.contains(required), "missing shared rule: {required}");
        }
        assert!(!rules.contains("docs/TEST-COVERAGE.md"));

        let store = tempfile::tempdir().unwrap();
        let claude_project = tempfile::tempdir().unwrap();
        let specifics = "<!-- hotsheet:begin specifics=testing-philosophy v=1 -->\nLocal test runner and fixtures.\n<!-- hotsheet:end specifics=testing-philosophy -->";
        let legacy = guidance_with_specifics(testing.legacy[0], testing.name, Some(specifics));
        std::fs::write(
            claude_project.path().join("CLAUDE.md"),
            format!("Local preface.\n\n{legacy}\n"),
        )
        .unwrap();
        run_setup_in(
            store.path(),
            claude_project.path(),
            Some("claude"),
            false,
            None,
            &[],
        )
        .unwrap();
        let claude = std::fs::read_to_string(claude_project.path().join("CLAUDE.md")).unwrap();
        let claude_source =
            std::fs::read_to_string(claude_project.path().join("AGENTS.md")).unwrap();
        assert!(claude_source.contains(&guidance_with_specifics(
            rules,
            testing.name,
            Some(specifics)
        )));
        assert!(claude.starts_with("Local preface.\n"));
        assert!(claude.contains("@AGENTS.md"));

        let codex_project = tempfile::tempdir().unwrap();
        run_setup_in(
            store.path(),
            codex_project.path(),
            Some("codex"),
            false,
            None,
            &[],
        )
        .unwrap();
        let agents_path = codex_project.path().join("AGENTS.md");
        let fresh = std::fs::read_to_string(&agents_path).unwrap();
        assert_eq!(fresh.matches(rules.trim()).count(), 1);
        let with_specifics = fresh.replace(
            rules.trim(),
            &guidance_with_specifics(rules, testing.name, Some(specifics)),
        );
        std::fs::write(&agents_path, &with_specifics).unwrap();
        let enabled = HashSet::from(["codex".to_string()]);
        refresh_setup_in(store.path(), codex_project.path(), Some(&enabled), &[]).unwrap();
        assert_eq!(
            std::fs::read_to_string(agents_path).unwrap(),
            with_specifics
        );
        assert!(!codex_project.path().join("CLAUDE.md").exists());
    }

    #[test]
    fn latest_bundled_guidance_snapshots_upgrade_without_losing_local_edits() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        run_setup_in(
            store.path(),
            project.path(),
            Some("codex"),
            false,
            None,
            &[],
        )
        .unwrap();
        let path = project.path().join("AGENTS.md");
        let mut contents = std::fs::read_to_string(&path).unwrap();
        for section in GUIDANCE_SECTIONS {
            if let Some(previous) = section.legacy.last() {
                contents = contents.replace(section.current.trim(), previous.trim());
            }
        }
        contents.push_str("\nLocal policy remains.\n");
        std::fs::write(&path, contents).unwrap();

        let enabled = HashSet::from(["codex".to_string()]);
        refresh_setup_in(store.path(), project.path(), Some(&enabled), &[]).unwrap();
        let refreshed = std::fs::read_to_string(&path).unwrap();
        for section in GUIDANCE_SECTIONS {
            assert!(
                refreshed.contains(section.current.trim()),
                "{} did not upgrade",
                section.name
            );
            if let Some(previous) = section.legacy.last() {
                assert!(!refreshed.contains(previous.trim()));
            }
        }
        assert!(refreshed.contains("Local policy remains."));
    }

    #[test]
    fn synthesis_guidance_reaches_fresh_projects_and_preserves_custom_paths_on_refresh() {
        let section = GUIDANCE_SECTIONS
            .iter()
            .find(|section| section.name == "requirements-documentation")
            .unwrap();
        for required in [
            "codebase map",
            "requirements summary",
            "repository-relative locations",
            "If either document is absent",
            "do not invent a map or product status",
        ] {
            assert!(
                section.current.contains(required),
                "missing shared rule: {required}"
            );
        }
        let specifics = "<!-- hotsheet:begin specifics=requirements-documentation v=1 -->\nCode map: notes/map.md. Status summary: notes/status.md.\n<!-- hotsheet:end specifics=requirements-documentation -->";
        let store = tempfile::tempdir().unwrap();
        for tool in ["claude", "codex"] {
            let project = tempfile::tempdir().unwrap();
            run_setup_in(store.path(), project.path(), Some(tool), false, None, &[]).unwrap();
            let path = project.path().join("AGENTS.md");
            let fresh = std::fs::read_to_string(&path).unwrap();
            assert_eq!(fresh.matches(section.current.trim()).count(), 1);
            assert!(!project.path().join("docs").exists());
            assert_eq!(project.path().join("CLAUDE.md").exists(), tool == "claude");

            let previous =
                guidance_with_specifics(section.legacy[1], section.name, Some(specifics));
            std::fs::write(&path, fresh.replace(section.current.trim(), &previous)).unwrap();
            let enabled = HashSet::from([tool.to_string()]);
            refresh_setup_in(store.path(), project.path(), Some(&enabled), &[]).unwrap();
            let refreshed = std::fs::read_to_string(&path).unwrap();
            assert!(refreshed.contains(&guidance_with_specifics(
                section.current,
                section.name,
                Some(specifics)
            )));
            assert!(refreshed.contains("notes/map.md"));
            assert!(refreshed.contains("notes/status.md"));
        }
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
        assert!(
            reports
                .set_up
                .iter()
                .all(|report| report.wrote[0] == "AGENTS.md")
        );
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

        assert!(
            fixture
                .refresh(&["alpha", "beta", "gamma"])
                .removed
                .is_empty()
        );
        assert_eq!(fixture.agents(), expect("alpha, beta, gamma"));
        // Disable one sharer: the section stays for the others. The refresh reports the
        // disabled sharer leaving the shared file and its deleted artifacts (HS2-CAM9J5).
        let removed = |id: &str| RemovalReport {
            tool: format!("{id} tool"),
            removed: vec![format!(".{id}/SKILL.md"), format!(".{id}/mcp.json")],
            edited: vec!["AGENTS.md".into()],
        };
        assert_eq!(
            fixture.refresh(&["alpha", "beta"]).removed,
            [removed("gamma")]
        );
        assert_eq!(fixture.agents(), expect("alpha, beta"));
        // Down to one sharer: still one shared section, not a per-tool re-split.
        assert_eq!(fixture.refresh(&["alpha"]).removed, [removed("beta")]);
        assert_eq!(fixture.agents(), expect("alpha"));
        assert!(fixture.refresh(&["alpha"]).removed.is_empty());
        assert_eq!(fixture.agents(), expect("alpha"));
        // No sharer remains: the section is removed and user content keeps its shape.
        let last = fixture.refresh(&["unrelated"]);
        assert!(last.set_up.is_empty());
        assert_eq!(last.removed, [removed("alpha")]);
        assert_eq!(fixture.agents(), "User text.\n\nFooter.\n");
        assert!(fixture.refresh(&["unrelated"]).is_empty());
        assert_eq!(fixture.agents(), "User text.\n\nFooter.\n");
        // Disabling also removed their skills (HS2-ZTGX6P), so these undetected tools are no
        // longer previously managed: re-enabling alone restores nothing, never a partial
        // layout, and explicit setup brings them back.
        fixture.refresh(&["gamma", "beta"]);
        assert_eq!(fixture.agents(), "User text.\n\nFooter.\n");
        fixture.setup("gamma");
        fixture.setup("beta");
        fixture.refresh(&["gamma", "beta"]);
        assert_eq!(
            fixture.agents(),
            format!("User text.\n\nFooter.\n\n{}\n", shared("beta, gamma"))
        );
        fixture.setup("alpha");
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

        // Their skills left too (HS2-ZTGX6P), so re-enabling these undetected tools needs
        // an explicit setup; the next refresh keeps what it restored.
        fixture.refresh(&["delta"]);
        assert_eq!(fixture.agents(), "User text.\n");
        fixture.setup("delta");
        fixture.refresh(&["delta"]);
        assert_eq!(fixture.agents(), format!("User text.\n\n{delta}\n"));
        fixture.setup("alpha");
        fixture.setup("beta");
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
    fn disabling_the_only_writer_deletes_an_instruction_file_left_empty() {
        // HS2-G9CD1W: setup created AGENTS.md for a single tool's per-tool section; disabling
        // that tool deletes the file instead of leaving it empty, and reports the deletion.
        let fixture = Sharing::new();
        sharing_tool(fixture.plugins.path(), "delta", "delta instructions\n", 8);
        fixture.setup("delta");
        assert!(fixture.agents().contains("hotsheet:delta"));
        let report = fixture.refresh(&["alpha"]);
        let agents = fixture.project.path().join("AGENTS.md");
        assert!(!agents.exists(), "{:?}", std::fs::read_to_string(&agents));
        let delta = report
            .removed
            .iter()
            .find(|item| item.tool == "delta tool")
            .unwrap();
        assert!(
            delta.removed.iter().any(|rel| rel == "AGENTS.md"),
            "{delta:?}"
        );
        assert!(
            !delta.edited.iter().any(|rel| rel == "AGENTS.md"),
            "{delta:?}"
        );
        // A repeated refresh has nothing left to remove and never recreates the file.
        fixture.refresh(&["alpha"]);
        assert!(!agents.exists());

        // A whitespace-only remainder counts as empty too.
        fixture.write_agents(&format!(
            "\n  \n{}\n\n\t\n",
            per_tool(
                "delta",
                "<!-- hotsheet-instructions-version: 8 -->\ndelta instructions"
            )
        ));
        fixture.refresh(&["alpha"]);
        assert!(!agents.exists());
    }

    #[test]
    fn retiring_a_shared_section_deletes_only_a_file_it_alone_filled() {
        let fixture = Sharing::new();
        fixture.setup("alpha");
        fixture.setup("beta");
        fixture.refresh(&["alpha", "beta"]);
        assert_eq!(fixture.agents(), format!("{}\n", shared("alpha, beta")));
        let report = fixture.refresh(&[]);
        assert!(!fixture.project.path().join("AGENTS.md").exists());
        assert!(
            report
                .removed
                .iter()
                .any(|item| item.removed.iter().any(|rel| rel == "AGENTS.md")),
            "{report:?}"
        );

        // Any user content keeps the file, with only the managed section removed.
        fixture.write_agents(&format!("{}\n\nUser footer.\n", shared("alpha, beta")));
        fixture.refresh(&[]);
        assert_eq!(fixture.agents(), "User footer.\n");
        fixture.write_agents(&format!("# Notes\n\n{}\n", per_tool("alpha", SHARED_BODY)));
        fixture.refresh(&[]);
        assert_eq!(fixture.agents(), "# Notes\n");
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
    fn codex_hook_notices_follow_changed_bytes_and_remain_tool_specific() {
        let fixture = Sharing::new();
        for id in ["codex", "claude"] {
            let notice = if id == "codex" {
                "change_notice = \"codex_hooks_changed\""
            } else {
                ""
            };
            custom_tool(
                fixture.plugins.path(),
                id,
                true,
                &format!(
                    r#"
[mcp]
target = ".{id}/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{{{{store}}}}"]
[hooks]
target = ".{id}/hooks.json"
format = "lifecycle-json"
event = "PermissionRequest"
command = "hotsheet-cli permission-hook"
timeout_seconds = 30
{notice}
"#
                ),
            );
        }
        let created = fixture.setup("codex");
        assert_eq!(
            created[0].notices,
            vec![SetupNotice::CodexHooksChanged {
                path: ".codex/hooks.json".into()
            }]
        );
        assert!(
            created[0].notices[0]
                .message()
                .contains("Run /hooks in Codex for this checkout")
        );
        let unchanged = fixture.setup("codex");
        assert!(unchanged[0].notices.is_empty());
        assert!(
            serde_json::to_value(&unchanged[0])
                .unwrap()
                .get("notices")
                .is_none()
        );

        let path = fixture.path(".codex/hooks.json");
        let mut hooks: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        hooks["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"] = serde_json::json!(99);
        hooks["hooks"]["SessionStart"] =
            serde_json::json!([{ "hooks": [{ "command": "old-binary permission-hook" }] }]);
        std::fs::write(&path, serde_json::to_vec(&hooks).unwrap()).unwrap();
        assert_eq!(fixture.setup("codex")[0].notices, created[0].notices);
        assert!(fixture.setup("codex")[0].notices.is_empty());
        assert!(fixture.setup("claude")[0].notices.is_empty());
        assert!(fixture.setup("claude")[0].notices.is_empty());

        // Refresh carries the same notices; changes in unrelated setup files do not warn.
        std::fs::write(&path, "{}").unwrap();
        let refresh = fixture.refresh(&["codex"]);
        assert_eq!(refresh.set_up[0].notices, created[0].notices);
        fixture.write("AGENTS.md", "user instructions\n");
        assert!(fixture.refresh(&["codex"]).set_up[0].notices.is_empty());
    }

    #[test]
    fn refresh_repairs_an_undetected_tool_with_only_a_legacy_hook() {
        let fixture = Sharing::new();
        custom_tool(
            fixture.plugins.path(),
            "orphan",
            false,
            r#"
[skills]
target = ".orphan/skills/hotsheet/SKILL.md"
source = "SKILL.md"
[mcp]
target = ".orphan/config.toml"
format = "codex-toml"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = []
[hooks]
target = ".orphan/hooks.json"
format = "lifecycle-json"
event = "PermissionRequest"
command = "hotsheet-cli permission-hook --agent codex"
"#,
        );
        fixture.write(
            ".orphan/hooks.json",
            r#"{"hooks":{"PermissionRequest":[{"hooks":[{"command":"/tmp/hotsheet-local-host-old/bin/hotsheet-cli permission-hook --agent codex"}]}]}}"#,
        );

        let refreshed = fixture.refresh(&["orphan"]);
        assert_eq!(refreshed.set_up.len(), 1);
        assert_eq!(refreshed.set_up[0].tool, "orphan tool");
        let hooks = fixture.read(".orphan/hooks.json").unwrap();
        assert!(!hooks.contains("hotsheet-local-host-old"), "{hooks}");
        assert!(hooks.contains("permission-hook --agent codex"), "{hooks}");
        assert!(
            fixture
                .read("AGENTS.md")
                .unwrap()
                .contains("orphan instructions")
        );
        assert!(fixture.path(".orphan/skills/hotsheet/SKILL.md").is_file());
        assert!(fixture.refresh(&["orphan"]).set_up[0].notices.is_empty());
    }

    #[test]
    fn changed_hook_notice_survives_later_setup_and_removal_failures() {
        let fixture = Sharing::new();
        custom_tool(
            fixture.plugins.path(),
            "codex",
            true,
            r#"
[mcp]
target = ".codex/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = []
[hooks]
target = ".codex/hooks.json"
format = "lifecycle-json"
event = "SessionStart"
command = "hotsheet-cli permission-hook"
change_notice = "codex_hooks_changed"
"#,
        );
        custom_tool(
            fixture.plugins.path(),
            "broken",
            true,
            r#"
[mcp]
target = ".broken/mcp.json"
format = "unknown-format"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = []
"#,
        );
        let error = setup_plugins(
            fixture.store.path(),
            fixture.project.path(),
            vec![fixture.plugin("codex"), fixture.plugin("broken")],
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown MCP config format"));
        assert!(
            error
                .to_string()
                .contains("Run /hooks in Codex for this checkout")
        );
        assert!(fixture.path(".codex/hooks.json").is_file());
        // Once the instruction has been reported, a genuine no-op has no review notice.
        assert!(fixture.setup("codex")[0].notices.is_empty());

        // A disabled tool's unwritable config can fail refresh after Codex was updated.
        custom_tool(
            fixture.plugins.path(),
            "disabled",
            false,
            r#"
[mcp]
target = ".disabled/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = []
"#,
        );
        fixture.write(
            ".disabled/mcp.json",
            r#"{"keep": true, "mcpServers": {"hotsheet": {"command": "hotsheet-mcp"}}}"#,
        );
        let disabled_path = fixture.path(".disabled/mcp.json");
        let original_permissions = std::fs::metadata(&disabled_path).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&disabled_path, readonly).unwrap();
        fixture.write(".codex/hooks.json", "{}");
        let enabled = HashSet::from(["codex".to_string()]);
        let refreshed = refresh_setup_in(
            fixture.store.path(),
            fixture.project.path(),
            Some(&enabled),
            &[fixture.plugins.path().to_path_buf()],
        );
        std::fs::set_permissions(&disabled_path, original_permissions).unwrap();
        let error = refreshed.unwrap_err();
        assert!(error.to_string().contains(".disabled/mcp.json"));
        assert!(
            error
                .to_string()
                .contains("Run /hooks in Codex for this checkout")
        );
    }

    /// A fixture tool with an explicit manifest tail (`[skills]`, `[mcp]`, `[hooks]`), its
    /// instruction section in `AGENTS.md` and a versioned skill source.
    fn custom_tool(root: &Path, id: &str, detected: bool, tail: &str) {
        let plugin = root.join(id);
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join("instructions.md"),
            format!("<!-- hotsheet-instructions-version: 8 -->\n{id} instructions\n"),
        )
        .unwrap();
        std::fs::write(
            plugin.join("SKILL.md"),
            format!("<!-- hotsheet-skill-version: 8 -->\n{id} skill\n"),
        )
        .unwrap();
        let binary = if detected {
            PRESENT_BINARY.to_string()
        } else {
            format!("definitely-not-installed-hotsheet-{id}")
        };
        std::fs::write(
            plugin.join("manifest.toml"),
            format!(
                r#"
id = "{id}"
display_name = "{id}"
product_name = "{id} tool"
tier = "cli-agent"
[detection]
binaries = ["{binary}"]
[instructions]
target = "AGENTS.md"
section = "instructions.md"
{tail}
"#
            ),
        )
        .unwrap();
    }

    impl Sharing {
        fn path(&self, rel: &str) -> std::path::PathBuf {
            self.project.path().join(rel)
        }

        fn read(&self, rel: &str) -> Option<String> {
            std::fs::read_to_string(self.path(rel)).ok()
        }

        fn write(&self, rel: &str, contents: &str) {
            let path = self.path(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }

        fn exclude(&self) -> String {
            self.read(".git/info/exclude").unwrap_or_default()
        }

        fn plugin(&self, id: &str) -> Plugin {
            crate::find_in(id, &[self.plugins.path().to_path_buf()]).unwrap()
        }
    }

    fn snapshot_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
        fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    visit(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                        std::fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        visit(root, root, &mut files);
        files
    }

    #[test]
    fn refresh_plan_is_read_only_and_predicts_exact_changed_paths() {
        let fixture = Sharing::new();
        std::fs::create_dir_all(fixture.path(".git/info")).unwrap();
        custom_tool(
            fixture.plugins.path(),
            "echo",
            true,
            r#"[skills]
target = ".echo/skills/hotsheet/SKILL.md"
source = "SKILL.md"
[mcp]
target = ".echo/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
        );
        fixture.write("AGENTS.md", "Local preface.\n");
        let enabled = HashSet::from(["echo".to_string(), "alpha".to_string()]);
        let dirs = [fixture.plugins.path().to_path_buf()];
        let before = snapshot_files(fixture.project.path());
        let plan = plan_refresh_setup_in(
            fixture.store.path(),
            fixture.project.path(),
            Some(&enabled),
            &dirs,
        )
        .unwrap();
        assert_eq!(snapshot_files(fixture.project.path()), before);
        let echo = plan.tools.iter().find(|tool| tool.tool == "echo").unwrap();
        assert!(echo.detected && echo.enabled && !echo.previously_managed);
        assert!(
            echo.artifacts
                .iter()
                .all(|artifact| artifact.status == ArtifactStatus::Missing)
        );
        let predicted: std::collections::BTreeSet<_> = plan
            .changes
            .iter()
            .map(|change| change.path.clone())
            .collect();
        refresh_setup_in(
            fixture.store.path(),
            fixture.project.path(),
            Some(&enabled),
            &dirs,
        )
        .unwrap();
        let after = snapshot_files(fixture.project.path());
        let actual: std::collections::BTreeSet<_> = before
            .keys()
            .chain(after.keys())
            .filter(|path| before.get(*path) != after.get(*path))
            .cloned()
            .collect();
        assert_eq!(predicted, actual);
        assert!(
            after
                .get("AGENTS.md")
                .unwrap()
                .starts_with(b"Local preface.")
        );
        let clean = plan_refresh_setup_in(
            fixture.store.path(),
            fixture.project.path(),
            Some(&enabled),
            &dirs,
        )
        .unwrap();
        assert!(clean.changes.is_empty());
        assert!(
            clean
                .tools
                .iter()
                .find(|tool| tool.tool == "echo")
                .unwrap()
                .artifacts
                .iter()
                .all(|artifact| artifact.status == ArtifactStatus::Current)
        );
        let alpha = clean
            .tools
            .iter()
            .find(|tool| tool.tool == "alpha")
            .unwrap();
        assert!(!alpha.detected && !alpha.previously_managed);
        assert!(
            alpha
                .artifacts
                .iter()
                .find(|artifact| artifact.kind == ArtifactKind::Instructions)
                .is_some_and(|artifact| artifact.status == ArtifactStatus::Missing)
        );
    }

    #[test]
    fn refresh_plan_reports_mcp_drift_and_disabled_removal_without_writing() {
        let fixture = Sharing::new();
        custom_tool(
            fixture.plugins.path(),
            "echo",
            true,
            r#"[skills]
target = ".echo/skills/hotsheet/SKILL.md"
source = "SKILL.md"
[mcp]
target = ".echo/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
        );
        let enabled = HashSet::from(["echo".to_string()]);
        let dirs = [fixture.plugins.path().to_path_buf()];
        fixture.refresh(&["echo"]);
        let mcp_path = fixture.path(".echo/mcp.json");
        let mut mcp: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&mcp_path).unwrap()).unwrap();
        mcp["mcpServers"]["hotsheet"]["args"] = serde_json::json!(["--path", "/old/store"]);
        std::fs::write(&mcp_path, serde_json::to_vec_pretty(&mcp).unwrap()).unwrap();
        let before = snapshot_files(fixture.project.path());
        let drift = plan_refresh_setup_in(
            fixture.store.path(),
            fixture.project.path(),
            Some(&enabled),
            &dirs,
        )
        .unwrap();
        let echo = drift.tools.iter().find(|tool| tool.tool == "echo").unwrap();
        let mcp = echo
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == ArtifactKind::Mcp)
            .unwrap();
        assert_eq!(mcp.status, ArtifactStatus::Outdated);
        assert!(mcp.detail.contains("args differ"));
        assert_eq!(snapshot_files(fixture.project.path()), before);

        let disabled = HashSet::new();
        let removal = plan_refresh_setup_in(
            fixture.store.path(),
            fixture.project.path(),
            Some(&disabled),
            &dirs,
        )
        .unwrap();
        assert!(
            removal
                .tools
                .iter()
                .find(|tool| tool.tool == "echo")
                .unwrap()
                .artifacts
                .iter()
                .all(|artifact| artifact.status == ArtifactStatus::Disabled)
        );
        assert!(removal.changes.iter().any(
            |change| change.path == ".echo/mcp.json" && change.action == PlannedAction::Remove
        ));
        assert_eq!(snapshot_files(fixture.project.path()), before);

        fixture.write(
            ".echo/skills/hotsheet/SKILL.md",
            "<!-- hotsheet-skill-version: 8 -->\nproject-customized skill\n",
        );
        let preserved = plan_refresh_setup_in(
            fixture.store.path(),
            fixture.project.path(),
            Some(&enabled),
            &dirs,
        )
        .unwrap();
        let echo = preserved
            .tools
            .iter()
            .find(|tool| tool.tool == "echo")
            .unwrap();
        assert!(
            echo.artifacts
                .iter()
                .any(|artifact| artifact.kind == ArtifactKind::Skill
                    && artifact.status == ArtifactStatus::Preserved)
        );
        assert!(
            !preserved
                .changes
                .iter()
                .any(|change| change.path == ".echo/skills/hotsheet/SKILL.md")
        );
    }

    /// HS2-ZTGX6P: the full disable → refresh → refresh → re-enable → refresh walk for a
    /// detected tool. Every wholly Hot Sheet-owned artifact leaves, the tool stops counting
    /// as previously managed, and re-enabling restores the identical layout.
    #[test]
    fn disabling_a_tool_removes_its_owned_artifacts_and_re_enabling_restores_them() {
        let fixture = Sharing::new();
        std::fs::create_dir_all(fixture.path(".git/info")).unwrap();
        custom_tool(
            fixture.plugins.path(),
            "echo",
            true,
            r#"[skills]
target = ".echo/skills/hotsheet/SKILL.md"
source = "SKILL.md"
[mcp]
target = ".echo/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
[hooks]
target = ".echo/settings.local.json"
machine_local = true
event = "PreToolUse"
command = "hotsheet-cli permission-hook"
"#,
        );
        fixture.write_agents("User text.\n");
        fixture.write(".echo/notes.md", "user file beside the tool's config\n");
        let artifacts = [
            "AGENTS.md",
            ".echo/skills/hotsheet/SKILL.md",
            ".echo/mcp.json",
            ".echo/settings.local.json",
        ];
        let snapshot = |fixture: &Sharing| artifacts.map(|rel| fixture.read(rel));

        fixture.refresh(&["echo"]);
        let enabled = snapshot(&fixture);
        assert!(enabled.iter().all(Option::is_some), "{enabled:?}");
        assert!(fixture.exclude().contains("/.echo/mcp.json\n"));
        assert!(fixture.exclude().contains("/.echo/settings.local.json\n"));
        let excludes = fixture.exclude();

        for pass in 0..2 {
            let report = fixture.refresh(&["alpha"]);
            let expected = RemovalReport {
                tool: "echo tool".into(),
                removed: artifacts[1..].iter().map(|rel| rel.to_string()).collect(),
                edited: vec!["AGENTS.md".into()],
            };
            // HS2-CAM9J5: the first pass reports every removal; the next reports nothing.
            if pass == 0 {
                assert_eq!(report.removed, [expected]);
            } else {
                assert!(report.removed.is_empty(), "{report:?}");
            }
            assert_eq!(fixture.agents(), "User text.\n");
            for rel in &artifacts[1..] {
                assert!(!fixture.path(rel).exists(), "{rel} was left behind");
            }
            assert!(
                !fixture.path(".echo/skills").exists(),
                "emptied dirs are pruned"
            );
            assert_eq!(
                fixture.read(".echo/notes.md").as_deref(),
                Some("user file beside the tool's config\n"),
                "a directory holding user files stays"
            );
            assert!(
                !fixture.exclude().contains("/.echo/"),
                "{}",
                fixture.exclude()
            );
            assert!(
                !has_managed_setup(
                    &PlanFs::default(),
                    fixture.project.path(),
                    &fixture.plugin("echo")
                ),
                "a disabled tool is no longer previously managed"
            );
        }

        for _ in 0..2 {
            fixture.refresh(&["echo"]);
            assert_eq!(
                snapshot(&fixture),
                enabled,
                "re-enabling restores the layout"
            );
            assert_eq!(fixture.exclude(), excludes);
        }
    }

    /// HS2-8B3VJP: the setting resolver keeps "unset" and "explicitly empty" apart.
    #[test]
    fn enabled_plugins_setting_distinguishes_unset_empty_and_malformed() {
        use serde_json::json;
        assert_eq!(enabled_plugins_from_setting(None), None);
        assert_eq!(enabled_plugins_from_setting(Some(&json!(null))), None);
        assert_eq!(
            enabled_plugins_from_setting(Some(&json!([]))),
            Some(HashSet::new())
        );
        assert_eq!(
            enabled_plugins_from_setting(Some(&json!(["claude", 7, "codex"]))),
            Some(HashSet::from(["claude".to_string(), "codex".to_string()]))
        );
        for malformed in [json!("claude"), json!({"claude": true}), json!(true)] {
            assert_eq!(enabled_plugins_from_setting(Some(&malformed)), None);
        }
    }

    /// HS2-8B3VJP: an explicit empty `enabled_plugins` list disables every tool. Walk
    /// unset → empty (every managed artifact leaves, twice, byte-stable) → unset (detected
    /// tools return) → empty → a restored list (only the listed detected tool returns).
    #[test]
    fn an_empty_enabled_list_disables_every_tool_and_a_restored_list_re_enables() {
        let fixture = Sharing::new();
        std::fs::create_dir_all(fixture.path(".git/info")).unwrap();
        custom_tool(
            fixture.plugins.path(),
            "echo",
            true,
            r#"[skills]
target = ".echo/skills/hotsheet/SKILL.md"
source = "SKILL.md"
[mcp]
target = ".echo/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
        );
        fixture.write_agents("User text.\n");
        let dirs = [fixture.plugins.path().to_path_buf()];
        let refresh = |enabled: Option<&HashSet<String>>| {
            refresh_setup_in(fixture.store.path(), fixture.project.path(), enabled, &dirs).unwrap()
        };
        let none = HashSet::new();
        let echo_artifacts = [".echo/skills/hotsheet/SKILL.md", ".echo/mcp.json"];

        // Unset: the detected tool is set up; explicitly set-up sharers are managed too.
        fixture.setup("alpha");
        fixture.setup("beta");
        refresh(None);
        for rel in echo_artifacts
            .iter()
            .chain(&[".alpha/SKILL.md", ".beta/mcp.json"])
        {
            assert!(fixture.path(rel).is_file(), "{rel} missing while unset");
        }
        assert!(fixture.agents().contains("hotsheet:echo"));

        // Explicit empty list: nothing is enabled, so every managed artifact leaves.
        for _ in 0..2 {
            assert!(refresh(Some(&none)).set_up.is_empty(), "no tool is set up");
            assert_eq!(fixture.agents(), "User text.\n");
            for rel in echo_artifacts.iter().chain(&[
                ".alpha/SKILL.md",
                ".alpha/mcp.json",
                ".beta/SKILL.md",
            ]) {
                assert!(!fixture.path(rel).exists(), "{rel} was left behind");
            }
            assert!(
                !fixture.exclude().contains("/.echo/"),
                "{}",
                fixture.exclude()
            );
            for id in ["echo", "alpha", "beta"] {
                assert!(!has_managed_setup(
                    &PlanFs::default(),
                    fixture.project.path(),
                    &fixture.plugin(id)
                ));
            }
        }
        // Detect-setup honors the empty list with its own error, not "none detected".
        let err = run_setup_in(
            fixture.store.path(),
            fixture.project.path(),
            None,
            true,
            Some(&none),
            &dirs,
        )
        .unwrap_err();
        assert!(matches!(err, SetupError::NoneEnabled), "{err}");

        // Back to unset: detected tools return; the undetected sharers stay gone.
        refresh(None);
        assert!(echo_artifacts.iter().all(|rel| fixture.path(rel).is_file()));
        assert!(!fixture.path(".alpha/SKILL.md").exists());
        let unset_echo = echo_artifacts.map(|rel| fixture.read(rel));

        // Empty again, then restore a list naming the detected tool.
        refresh(Some(&none));
        assert!(echo_artifacts.iter().all(|rel| !fixture.path(rel).exists()));
        let restored = HashSet::from(["echo".to_string()]);
        for _ in 0..2 {
            let reports = refresh(Some(&restored));
            assert_eq!(reports.set_up.len(), 1, "{reports:?}");
            assert_eq!(echo_artifacts.map(|rel| fixture.read(rel)), unset_echo);
            assert!(fixture.agents().contains("hotsheet:echo"));
        }
    }

    #[test]
    fn a_disabled_tools_customized_or_newer_workflow_is_preserved_as_a_bundle() {
        let fixture = Sharing::new();
        sharing_tool(fixture.plugins.path(), "delta", "delta instructions\n", 8);
        let section = per_tool(
            "delta",
            "<!-- hotsheet-instructions-version: 8 -->\ndelta instructions",
        );
        for installed in [
            "<!-- hotsheet-skill-version: 9 -->\nnewer skill\n",
            "<!-- hotsheet-skill-version: 8 -->\nproject-customized skill\n",
        ] {
            fixture.setup("delta");
            fixture.write(".delta/SKILL.md", installed);
            for _ in 0..2 {
                fixture.refresh(&["alpha"]);
                assert_eq!(fixture.read(".delta/SKILL.md").as_deref(), Some(installed));
                assert!(fixture.agents().contains(&section), "{}", fixture.agents());
                assert!(
                    !fixture.path(".delta/mcp.json").exists(),
                    "MCP maintenance continues for a preserved workflow"
                );
                assert!(has_managed_setup(
                    &PlanFs::default(),
                    fixture.project.path(),
                    &fixture.plugin("delta")
                ));
            }
        }

        // An older or unversioned skill is Hot Sheet's to remove, section and all.
        for installed in [
            "<!-- hotsheet-skill-version: 3 -->\nolder\n",
            "unversioned\n",
        ] {
            fixture.setup("delta");
            fixture.write(".delta/SKILL.md", installed);
            fixture.refresh(&["alpha"]);
            assert!(!fixture.path(".delta").exists());
            assert!(!fixture.agents().contains("hotsheet:delta"));
        }
    }

    #[test]
    fn disabling_a_tool_keeps_unrelated_entries_in_shared_mcp_configs() {
        let fixture = Sharing::new();
        std::fs::create_dir_all(fixture.path(".git/info")).unwrap();
        for (id, format, target) in [
            ("jay", "claude-json", "shared.json"),
            ("oscar", "opencode-json", "opencode.json"),
            ("toby", "codex-toml", ".toby/config.toml"),
        ] {
            custom_tool(
                fixture.plugins.path(),
                id,
                false,
                &format!(
                    r#"[mcp]
target = "{target}"
format = "{format}"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{{store}}"]
"#
                ),
            );
        }
        let json_user = "{\n  \"mcpServers\": {\n    \"other\": {\n      \"command\": \"other-mcp\"\n    }\n  },\n  \"theme\": \"dark\"\n}\n";
        let opencode_user = "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"mcp\": {\n    \"other\": {\n      \"command\": [\n        \"other-mcp\"\n      ],\n      \"type\": \"local\"\n    }\n  }\n}\n";
        let toml_user = "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"other-mcp\"\n";
        fixture.write("shared.json", json_user);
        fixture.write("opencode.json", opencode_user);
        fixture.write(".toby/config.toml", toml_user);
        for id in ["jay", "oscar", "toby"] {
            fixture.setup(id);
        }
        // Every MCP format declares the ai actor for external clients (HS2-28DM8B).
        let json: serde_json::Value =
            serde_json::from_str(&fixture.read("shared.json").unwrap()).unwrap();
        assert_eq!(
            json["mcpServers"]["hotsheet"]["env"]["HOTSHEET_ACTOR_ROLE"],
            "ai"
        );
        let opencode: serde_json::Value =
            serde_json::from_str(&fixture.read("opencode.json").unwrap()).unwrap();
        assert_eq!(
            opencode["mcp"]["hotsheet"]["environment"]["HOTSHEET_ACTOR_ROLE"],
            "ai"
        );
        let codex: toml::Table =
            toml::from_str(&fixture.read(".toby/config.toml").unwrap()).unwrap();
        assert_eq!(
            codex["mcp_servers"]["hotsheet"]["env"]["HOTSHEET_ACTOR_ROLE"].as_str(),
            Some("ai")
        );
        // Shared project files can be committed. Their Hot Sheet entry must remain
        // portable even when setup ran from a machine-specific binary and store.
        assert_eq!(json["mcpServers"]["hotsheet"]["command"], "hotsheet-mcp");
        assert_eq!(
            json["mcpServers"]["hotsheet"]["args"],
            serde_json::json!(["--path", "."])
        );
        assert_eq!(
            opencode["mcp"]["hotsheet"]["command"],
            serde_json::json!(["hotsheet-mcp", "--path", "."])
        );
        assert_eq!(
            codex["mcp_servers"]["hotsheet"]["command"].as_str(),
            Some("hotsheet-mcp")
        );
        assert_eq!(
            codex["mcp_servers"]["hotsheet"]["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["--path", "."]
        );
        for contents in [
            fixture.read("shared.json").unwrap(),
            fixture.read("opencode.json").unwrap(),
            fixture.read(".toby/config.toml").unwrap(),
        ] {
            assert!(!contents.contains(&fixture.store.path().display().to_string()));
            assert!(!contents.contains(&fixture.project.path().display().to_string()));
        }
        assert!(
            fixture
                .read("shared.json")
                .unwrap()
                .contains("hotsheet-mcp")
        );
        assert!(
            fixture
                .read("opencode.json")
                .unwrap()
                .contains("hotsheet-mcp")
        );
        assert!(
            fixture
                .read(".toby/config.toml")
                .unwrap()
                .contains("hotsheet-mcp")
        );

        for pass in 0..2 {
            let report = fixture.refresh(&["alpha"]);
            // HS2-CAM9J5: the first pass reports each shared config as edited, never removed.
            // Each fixture's AGENTS.md section leaves too, deleting the file setup created for
            // it (HS2-G9CD1W), and the built-in OpenCode also targets opencode.json, so
            // compare the set of edited configs.
            let edited: std::collections::BTreeSet<_> = report
                .removed
                .iter()
                .flat_map(|tool| {
                    assert!(
                        tool.removed.iter().all(|rel| rel == "AGENTS.md"),
                        "{tool:?}"
                    );
                    tool.edited
                        .iter()
                        .filter(|rel| *rel != "AGENTS.md")
                        .cloned()
                })
                .collect();
            if pass == 0 {
                assert_eq!(
                    edited,
                    ["shared.json", "opencode.json", ".toby/config.toml"]
                        .map(String::from)
                        .into()
                );
            } else {
                assert!(report.removed.is_empty(), "{report:?}");
            }
            let json: serde_json::Value =
                serde_json::from_str(&fixture.read("shared.json").unwrap()).unwrap();
            assert_eq!(
                json,
                serde_json::from_str::<serde_json::Value>(json_user).unwrap()
            );
            let opencode: serde_json::Value =
                serde_json::from_str(&fixture.read("opencode.json").unwrap()).unwrap();
            assert_eq!(
                opencode,
                serde_json::from_str::<serde_json::Value>(opencode_user).unwrap()
            );
            let toml: toml::Table =
                toml::from_str(&fixture.read(".toby/config.toml").unwrap()).unwrap();
            assert_eq!(toml, toml::from_str::<toml::Table>(toml_user).unwrap());
            assert_eq!(
                fixture.exclude(),
                "",
                "user-owned configs were never excluded"
            );
        }

        // A user's own `hotsheet` server (not Hot Sheet's MCP binary) is not ours to remove.
        let own = "{\n  \"mcpServers\": {\n    \"hotsheet\": {\n      \"command\": \"/opt/custom/bridge\"\n    }\n  }\n}\n";
        fixture.write("shared.json", own);
        assert!(fixture.refresh(&["alpha"]).removed.is_empty());
        assert_eq!(fixture.read("shared.json").as_deref(), Some(own));

        // Configs that held only Hot Sheet's entry are deleted outright.
        std::fs::remove_file(fixture.path("shared.json")).unwrap();
        std::fs::remove_file(fixture.path("opencode.json")).unwrap();
        std::fs::remove_dir_all(fixture.path(".toby")).unwrap();
        for id in ["jay", "oscar", "toby"] {
            fixture.setup(id);
        }
        assert_eq!(
            fixture.exclude(),
            "/shared.json\n/opencode.json\n/.toby/config.toml\n"
        );
        let report = fixture.refresh(&["alpha"]);
        let deleted: std::collections::BTreeSet<_> = report
            .removed
            .iter()
            .flat_map(|tool| tool.removed.clone())
            .collect();
        // AGENTS.md held only the disabled tools' sections, so it goes too (HS2-G9CD1W).
        assert_eq!(
            deleted,
            [
                "AGENTS.md",
                "shared.json",
                "opencode.json",
                ".toby/config.toml"
            ]
            .map(String::from)
            .into()
        );
        for rel in ["AGENTS.md", "shared.json", "opencode.json", ".toby"] {
            assert!(!fixture.path(rel).exists(), "{rel}");
        }
        assert_eq!(fixture.exclude(), "");
    }

    #[test]
    fn disabling_a_tool_removes_only_hot_sheet_hooks() {
        let fixture = Sharing::new();
        custom_tool(
            fixture.plugins.path(),
            "hank",
            false,
            r#"[mcp]
target = ".hank/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = []
[hooks]
target = ".hank/settings.json"
event = "PreToolUse"
additional_events = ["PermissionRequest"]
command = "hotsheet-cli permission-hook"
"#,
        );
        let user = serde_json::json!({
            "permissions": { "allow": ["Bash(ls)"] },
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "hooks": [{ "type": "command", "command": "audit" }] }
                ],
                "Stop": []
            }
        });
        fixture.write(
            ".hank/settings.json",
            &serde_json::to_string_pretty(&user).unwrap(),
        );
        fixture.setup("hank");
        let installed = fixture.read(".hank/settings.json").unwrap();
        assert!(installed.contains("permission-hook"), "{installed}");
        assert!(installed.contains("PermissionRequest"), "{installed}");
        for pass in 0..2 {
            let report = fixture.refresh(&["alpha"]);
            if pass == 0 {
                assert_eq!(
                    report.removed,
                    [RemovalReport {
                        tool: "hank tool".into(),
                        // Setup created AGENTS.md for this section alone, so it leaves
                        // with it (HS2-G9CD1W).
                        removed: vec!["AGENTS.md".into(), ".hank/mcp.json".into()],
                        edited: vec![".hank/settings.json".into()],
                    }]
                );
            } else {
                assert!(report.is_empty(), "{report:?}");
            }
            let settings: serde_json::Value =
                serde_json::from_str(&fixture.read(".hank/settings.json").unwrap()).unwrap();
            assert_eq!(settings, user, "only Hot Sheet's hooks leave");
        }
    }

    #[test]
    fn a_disabled_tool_never_removes_a_target_an_enabled_tool_declares() {
        let fixture = Sharing::new();
        for id in ["one", "two"] {
            custom_tool(
                fixture.plugins.path(),
                id,
                false,
                r#"[skills]
target = ".shared/SKILL.md"
source = "SKILL.md"
[mcp]
target = ".shared/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = []
"#,
            );
        }
        fixture.setup("one");
        let skill = fixture.read(".shared/SKILL.md");
        let mcp = fixture.read(".shared/mcp.json");
        assert!(skill.is_some() && mcp.is_some());
        // `two` stays enabled (and previously managed through the shared skill), so `one`
        // being disabled leaves the shared skill and MCP entry in place.
        fixture.refresh(&["two"]);
        assert!(fixture.read(".shared/SKILL.md").is_some());
        assert_eq!(fixture.read(".shared/mcp.json"), mcp);
        assert!(!fixture.agents().contains("hotsheet:one"));
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
        for report in &reports.set_up {
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
            mcp_command_for(None, Some(dir.path()), "hotsheet-mcp"),
            "hotsheet-mcp"
        );
        assert_eq!(mcp_command_for(None, None, "hotsheet-mcp"), "hotsheet-mcp");
        let sib = dir.path().join("hotsheet-mcp");
        std::fs::write(&sib, "x").unwrap();
        assert_eq!(
            mcp_command_for(None, Some(dir.path()), "hotsheet-mcp"),
            sib.to_string_lossy()
        );
    }

    #[test]
    fn mcp_command_prefers_an_existing_absolute_override() {
        // HS2-HNXE38: a snapshot-run server has no sibling shim; the host names a stable one.
        let snapshot = tempfile::tempdir().unwrap();
        let stable = tempfile::tempdir().unwrap();
        let shim = stable.path().join("hotsheet-mcp");
        std::fs::write(&shim, "x").unwrap();
        let shim_str = shim.to_string_lossy().into_owned();
        assert_eq!(
            mcp_command_for(Some(&shim), Some(snapshot.path()), "hotsheet-mcp"),
            shim_str
        );
        // The override beats an existing sibling too.
        std::fs::write(snapshot.path().join("hotsheet-mcp"), "x").unwrap();
        assert_eq!(
            mcp_command_for(Some(&shim), Some(snapshot.path()), "hotsheet-mcp"),
            shim_str
        );
        // A relative or missing override is ignored rather than persisted.
        let sibling = snapshot
            .path()
            .join("hotsheet-mcp")
            .to_string_lossy()
            .into_owned();
        for ignored in [Path::new("hotsheet-mcp"), &stable.path().join("missing")] {
            assert_eq!(
                mcp_command_for(Some(ignored), Some(snapshot.path()), "hotsheet-mcp"),
                sibling
            );
        }
        assert_eq!(
            mcp_command_for(Some(Path::new("rel/mcp")), None, "hotsheet-mcp"),
            "hotsheet-mcp"
        );
    }

    #[test]
    fn detect_with_no_tool_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_setup_in(dir.path(), dir.path(), None, false, None, &[]).unwrap_err();
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
            assert_eq!(report.set_up[0].wrote, [".fixture/mcp.json"]);
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
    fn hs2_qpedjr_shared_settings_readers_see_complete_replacements() {
        use std::io::Read as _;

        let project = tempfile::tempdir().unwrap();
        let path = project.path().join(SHARED_SETTINGS_PATH);
        let versions = [
            "{\"$hotsheetSchema\":1,\"push_policy\":\"push-per-ticket\"}\n",
            "{\"$hotsheetSchema\":1,\"push_policy\":\"push-per-batch\"}\n",
        ];

        // Empty -> populated, then repeatedly replace while a reader holds the old file.
        write_file(&path, versions[0]).unwrap();
        for index in 0..12 {
            let old = versions[index % 2];
            let new = versions[(index + 1) % 2];
            let mut open_reader = std::fs::File::open(&path).unwrap();
            write_file(&path, new).unwrap();
            let mut snapshot = String::new();
            open_reader.read_to_string(&mut snapshot).unwrap();
            assert_eq!(snapshot, old);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), new);
            let _: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
            let _: serde_json::Value = serde_json::from_str(new).unwrap();
        }
    }

    #[test]
    fn local_excludes_follow_a_worktree_gitdir_pointer() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let git_dir = root.path().join("worktree-git");
        std::fs::create_dir(&project).unwrap();
        std::fs::create_dir_all(git_dir.join("info")).unwrap();
        std::fs::write(project.join(".git"), "gitdir: ../worktree-git\n").unwrap();

        let mut fs = PlanFs::default();
        ensure_local_git_exclude(&mut fs, &project, ".codex/config.toml").unwrap();
        apply_operations(fs.final_operations(&project)).unwrap();

        assert_eq!(
            std::fs::read_to_string(git_dir.join("info/exclude")).unwrap(),
            "/.codex/config.toml\n"
        );
    }

    #[test]
    fn local_excludes_land_in_the_common_dir_git_reads_for_a_linked_worktree() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("main");
        let linked = root.path().join("linked");
        let git = |dir: &Path, args: &[&str]| {
            let out = hotsheet_ticketing::git::command_in(dir)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(
            &main,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "i",
            ],
        );
        git(&main, &["worktree", "add", "-q", linked.to_str().unwrap()]);
        std::fs::write(linked.join("opencode.json"), "{}").unwrap();

        let mut fs = PlanFs::default();
        ensure_local_git_exclude(&mut fs, &linked, "opencode.json").unwrap();
        apply_operations(fs.final_operations(&linked)).unwrap();

        assert_eq!(git(&linked, &["status", "--porcelain"]), "");
        let mut fs = PlanFs::default();
        remove_local_git_exclude(&mut fs, &linked, "opencode.json").unwrap();
        apply_operations(fs.final_operations(&linked)).unwrap();
        assert_eq!(
            git(&linked, &["status", "--porcelain"]),
            "?? opencode.json\n"
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

        let rendered = |template: &str| {
            template.replace(
                "{{HOTSHEET_PUSH_POLICY}}",
                &PushPolicy::PerTicket.skill_wording(),
            )
        };
        assert_eq!(rendered(bundled_claude), canonical_claude);
        assert_eq!(rendered(bundled_codex), canonical_codex);
        assert_eq!(
            without_claude_tool_metadata(bundled_claude),
            without_claude_tool_metadata(bundled_codex)
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
