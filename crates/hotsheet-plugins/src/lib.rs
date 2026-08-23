//! The core `plugins` module: the AI-tool plugin loader + registry
//! (`docs/05-ai-tool-plugins.md` §5.11). Both binaries (server + CLI) drive it, so
//! setup can run headless (`docs/05` §5.1a).
//!
//! Plugins are **external + loadable** (HS2-92). A plugin is a directory with a
//! `manifest.toml` + template files; the same `Plugin` loads from a **bundled**
//! first-party directory (embedded via `include_dir`) or a **real on-disk** directory
//! in the search path — one code path, so a third-party plugin is not special. The
//! behavioral subprocess/WASM boundary + the trust gate are HS2-93.
//!
//! Search path (later entries add to earlier, first-party ids win a collision):
//!   1. bundled built-ins (Claude, Codex) — first-party.
//!   2. `${HOTSHEET_HOME:-~/.hotsheet2}/plugins/<id>/` — machine.
//!
//! (Project-scoped dirs land with the multi-store project model.)

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};
use serde::Deserialize;

pub mod setup;
pub use setup::{SetupError, SetupReport, mcp_command, run_setup};

/// The bundled first-party plugins, embedded from the repo's `plugins/` tree at build
/// time. Adding a first-party tool = adding a directory here.
static CLAUDE: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../plugins/claude");
static CODEX: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../plugins/codex");
static ANTIGRAVITY: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../plugins/antigravity");

/// A failure loading a plugin.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin '{0}' has no manifest.toml")]
    MissingManifest(String),
    #[error("plugin manifest is not valid UTF-8")]
    NotUtf8,
    #[error("parsing plugin manifest: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("plugin '{id}' references missing file '{file}'")]
    MissingFile { id: String, file: String },
    #[error("reading plugin dir {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

/// Where a plugin was loaded from — its provenance (a trust input for HS2-93).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSource {
    /// A bundled first-party plugin.
    BuiltIn,
    /// An on-disk plugin directory.
    Disk(PathBuf),
}

/// A plugin's declarative manifest (`manifest.toml`). The behavioral capabilities
/// (drive/terminals/…) are out of scope for this minimal loader.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub display_name: String,
    pub product_name: String,
    pub tier: String,
    #[serde(default)]
    pub detection: Detection,
    pub instructions: Instructions,
    /// Optional: some tools (e.g. Codex) have no "skills" concept. Absence = the tool
    /// gets no skill artifact (docs/05 §5.3, "absence is the signal").
    #[serde(default)]
    pub skills: Option<Skills>,
    pub mcp: Mcp,
    /// Optional: how the host **drives** the tool (docs/13). Absent = not drivable
    /// (editor tools, or a transport not built yet). Declarative/client-safe — the
    /// behavioral host (`hotsheet-aitools`) maps it to a `Drive`.
    #[serde(default)]
    pub drive: Option<DriveSpec>,
    /// Optional: the tool emits usage/cost **telemetry** the host maps to `UsageEvent`s
    /// (`docs/14`, HS2-8PSAFE). Absent = no metrics captured for this tool (absence is the
    /// signal). Names the tool's native telemetry `source` so the host picks the right mapper.
    #[serde(default)]
    pub metrics: Option<MetricsSpec>,
    /// Optional: a **permission hook** the tool runs before each tool use (`docs/05` §5.7,
    /// HS2-YMR9HE). `setup` registers it in the tool's config so a Hot Sheet-driven run
    /// routes approvals to the bridge. Absent = no hook (the tool has its own approval path,
    /// e.g. codex's app-server ServerRequests).
    #[serde(default)]
    pub hooks: Option<HooksSpec>,
}

/// A tool's permission-hook declaration (`docs/05` §5.7). Declarative — `setup` merges it
/// into the tool's config; the hook `command` (its first token resolved to the sibling
/// binary) routes the prompt to the Hot Sheet server.
#[derive(Debug, Clone, Deserialize)]
pub struct HooksSpec {
    /// The tool config file the hook is written into (e.g. `.claude/settings.json`).
    pub target: String,
    /// The hook event to register on (e.g. `PreToolUse`).
    pub event: String,
    /// The command line to run (e.g. `hotsheet-cli permission-hook`); its first token is
    /// resolved to the absolute sibling binary at setup (no PATH reliance, HS2-103).
    pub command: String,
}

/// A tool's metrics-capability declaration (`docs/14` §14.2). Declarative — the behavioral
/// host maps `source` to the concrete telemetry parser (codex usage / claude OTLP / …).
#[derive(Debug, Clone, Deserialize)]
pub struct MetricsSpec {
    /// Which native telemetry the tool exposes: `codex-usage` | `claude-otlp` | `acp` | ….
    pub source: String,
}

/// A tool's drive declaration (`docs/13`). Transport + content are strings here (this
/// crate is the declarative, I/O-free loader); the host maps them to its enums.
#[derive(Debug, Clone, Deserialize)]
pub struct DriveSpec {
    /// `spawn` | `claude-channel` | `app-server` | `acp`.
    pub transport: String,
    /// The tool's own launch program (e.g. `codex`) — NOT `hotsheet-mcp`.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// `arg` (append the prompt) | `stdin` (pipe it).
    #[serde(default = "default_content_mode")]
    pub content: String,
    #[serde(default)]
    pub interrupt: bool,
    /// Spawn transport only: a flag (e.g. `--conversation`) to resume an existing
    /// session when a session id is supplied, instead of starting a fresh one.
    #[serde(default)]
    pub resume_flag: Option<String>,
}

fn default_content_mode() -> String {
    "arg".to_string()
}

/// How the host decides the tool is installed on this machine.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Detection {
    #[serde(default)]
    pub binaries: Vec<String>,
}

/// The managed instruction file + the plugin file holding the section to merge in.
#[derive(Debug, Clone, Deserialize)]
pub struct Instructions {
    /// Where in the project it's written (e.g. `CLAUDE.md`).
    pub target: String,
    /// The plugin-relative file whose contents are the Hot Sheet section.
    pub section: String,
}

/// The worklist skill: where it's written, and its source in the plugin.
#[derive(Debug, Clone, Deserialize)]
pub struct Skills {
    pub target: String,
    pub source: String,
}

/// How the `hotsheet-mcp` server is registered in the tool's MCP config. `{store}` in
/// `args` is substituted with the project's store path at setup time. `format` selects
/// the host's config writer (`claude-json` | `codex-toml`) — keyed on the format, not
/// the tool id (docs/05 §5.3).
#[derive(Debug, Clone, Deserialize)]
pub struct Mcp {
    pub target: String,
    pub format: String,
    pub server_name: String,
    pub command: String,
    pub args: Vec<String>,
}

/// A loaded plugin: its manifest, the (flat) set of files bundled with it, and where
/// it came from.
#[derive(Debug, Clone)]
pub struct Plugin {
    pub manifest: Manifest,
    pub source: PluginSource,
    files: BTreeMap<String, String>,
}

impl Plugin {
    /// Build from a manifest string + the plugin's files, validating referenced paths.
    fn assemble(
        manifest_raw: &str,
        files: BTreeMap<String, String>,
        source: PluginSource,
    ) -> Result<Self, PluginError> {
        let manifest: Manifest = toml::from_str(manifest_raw)?;
        let plugin = Plugin {
            manifest,
            source,
            files,
        };
        plugin.require(&plugin.manifest.instructions.section)?;
        if let Some(skills) = &plugin.manifest.skills {
            plugin.require(&skills.source)?;
        }
        Ok(plugin)
    }

    /// Load a bundled plugin from an embedded directory.
    fn from_dir(dir: &Dir) -> Result<Self, PluginError> {
        let raw = dir
            .get_file("manifest.toml")
            .ok_or_else(|| PluginError::MissingManifest(dir.path().display().to_string()))?
            .contents_utf8()
            .ok_or(PluginError::NotUtf8)?;
        let mut files = BTreeMap::new();
        for f in dir.files() {
            if let (Some(name), Some(text)) = (
                f.path().file_name().and_then(|n| n.to_str()),
                f.contents_utf8(),
            ) {
                files.insert(name.to_string(), text.to_string());
            }
        }
        Self::assemble(raw, files, PluginSource::BuiltIn)
    }

    /// Load a plugin from a real on-disk directory (a third-party plugin).
    pub fn from_fs_dir(dir: &Path) -> Result<Self, PluginError> {
        let io = |source| PluginError::Io {
            path: dir.display().to_string(),
            source,
        };
        let manifest_path = dir.join("manifest.toml");
        if !manifest_path.is_file() {
            return Err(PluginError::MissingManifest(dir.display().to_string()));
        }
        let mut files = BTreeMap::new();
        for entry in fs::read_dir(dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            let path = entry.path();
            if !path.is_file() {
                continue; // top-level files only, like the embedded loader
            }
            if let (Some(name), Ok(text)) = (
                path.file_name().and_then(|n| n.to_str()),
                fs::read_to_string(&path),
            ) {
                files.insert(name.to_string(), text);
            }
        }
        let raw = files
            .get("manifest.toml")
            .cloned()
            .ok_or(PluginError::NotUtf8)?;
        Self::assemble(&raw, files, PluginSource::Disk(dir.to_path_buf()))
    }

    fn require(&self, file: &str) -> Result<(), PluginError> {
        if self.files.contains_key(file) {
            Ok(())
        } else {
            Err(PluginError::MissingFile {
                id: self.manifest.id.clone(),
                file: file.to_string(),
            })
        }
    }

    /// The plugin's id (e.g. `"claude"`).
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    /// Whether this plugin is a bundled first-party one.
    pub fn is_builtin(&self) -> bool {
        self.source == PluginSource::BuiltIn
    }

    /// A bundled file's contents by name.
    pub fn file(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }

    /// The Hot Sheet section to merge into the tool's instruction file.
    pub fn instructions_body(&self) -> &str {
        self.file(&self.manifest.instructions.section).unwrap_or("")
    }

    /// The worklist skill (target + body), or `None` if this tool has no skills.
    pub fn skill(&self) -> Option<(&str, &str)> {
        let skills = self.manifest.skills.as_ref()?;
        Some((&skills.target, self.file(&skills.source).unwrap_or("")))
    }

    /// The MCP `args` with `{store}` substituted for the given store path.
    pub fn mcp_args(&self, store_path: &str) -> Vec<String> {
        self.manifest
            .mcp
            .args
            .iter()
            .map(|a| a.replace("{store}", store_path))
            .collect()
    }

    /// Every project-relative path this plugin would write during setup.
    pub fn target_paths(&self) -> Vec<&str> {
        let mut v = vec![
            self.manifest.instructions.target.as_str(),
            self.manifest.mcp.target.as_str(),
        ];
        if let Some(s) = &self.manifest.skills {
            v.push(s.target.as_str());
        }
        v
    }

    /// Declared write targets that would **escape the project** (absolute or `..`).
    /// A plugin with any of these is unsafe to set up — the setup writer refuses it.
    pub fn unsafe_targets(&self) -> Vec<String> {
        self.target_paths()
            .into_iter()
            .filter(|t| !is_safe_rel_path(t))
            .map(String::from)
            .collect()
    }
}

/// The MCP config formats the setup writer understands.
pub const KNOWN_MCP_FORMATS: &[&str] = &["claude-json", "codex-toml"];

/// Whether `p` is a project-relative path that stays inside the project — no absolute
/// path, no `..`, no drive prefix. This is the guardrail against a plugin declaring a
/// write target like `/etc/x` or `../../.ssh/authorized_keys`.
pub fn is_safe_rel_path(p: &str) -> bool {
    use std::path::Component;
    !p.is_empty()
        && Path::new(p)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

// ---- registry --------------------------------------------------------------------

/// Every bundled first-party plugin, loaded through the same path a third-party
/// plugin uses. Panics only on a build-time-embedded malformed plugin (a bug).
pub fn builtin_plugins() -> Vec<Plugin> {
    [&CLAUDE, &CODEX, &ANTIGRAVITY]
        .into_iter()
        .map(|d| Plugin::from_dir(d).expect("bundled first-party plugin must load"))
        .collect()
}

/// The full registry: built-ins first, then every plugin dir found under each search
/// dir. A first-party id wins a collision (a third party can't silently shadow it);
/// an unreadable/malformed on-disk plugin is skipped, not fatal.
pub fn all_plugins(search_dirs: &[PathBuf]) -> Vec<Plugin> {
    let mut plugins = builtin_plugins();
    let mut seen: std::collections::HashSet<String> =
        plugins.iter().map(|p| p.id().to_string()).collect();
    for dir in search_dirs {
        for p in load_dir(dir) {
            if seen.insert(p.id().to_string()) {
                plugins.push(p);
            }
        }
    }
    plugins
}

/// Load every plugin (a subdir containing `manifest.toml`) directly under `dir`.
/// Missing dir → empty; a bad plugin dir is skipped.
pub fn load_dir(dir: &Path) -> Vec<Plugin> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.join("manifest.toml").is_file() {
            if let Ok(p) = Plugin::from_fs_dir(&path) {
                out.push(p);
            }
        }
    }
    out.sort_by(|a, b| a.id().cmp(b.id()));
    out
}

/// Find a plugin by id across built-ins + the given search dirs.
pub fn find_in(id: &str, search_dirs: &[PathBuf]) -> Option<Plugin> {
    all_plugins(search_dirs).into_iter().find(|p| p.id() == id)
}

/// Find a plugin by id across built-ins + the default machine search path.
pub fn find(id: &str) -> Option<Plugin> {
    find_in(id, &default_dirs())
}

/// The machine-local plugin search dirs (currently just `<home>/plugins`). Deliberately
/// under a **HS2-specific** home, not `~/.hotsheet` (which a separately installed Hot
/// Sheet 1 owns — see the `hotsheet_home` note).
pub fn default_dirs() -> Vec<PathBuf> {
    vec![machine_plugins_dir()]
}

/// `<hotsheet_home>/plugins`.
pub fn machine_plugins_dir() -> PathBuf {
    hotsheet_home().join("plugins")
}

/// HS2's machine-local state home: `$HOTSHEET_HOME`, else `~/.hotsheet2`. Kept off
/// `~/.hotsheet` on purpose so HS2 never writes into HS1's data dir.
pub fn hotsheet_home() -> PathBuf {
    if let Some(h) = std::env::var_os("HOTSHEET_HOME") {
        return PathBuf::from(h);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".hotsheet2")
}

#[cfg(test)]
mod tests;
