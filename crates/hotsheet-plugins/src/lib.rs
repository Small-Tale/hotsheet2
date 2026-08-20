//! The core `plugins` module: the AI-tool plugin loader + registry
//! (`docs/05-ai-tool-plugins.md` §5.11). Both binaries (server + CLI) drive it, so
//! setup can run headless (`docs/05` §5.1a).
//!
//! This is the **minimal** loader (HS2-97): it loads the bundled **first-party**
//! plugins and exposes their one-shot setup artifacts (instruction section, worklist
//! skill, MCP-config template) for a setup writer (HS2-98) to install. The external
//! search-path / third-party / subprocess+WASM / trust machinery is HS2-92/HS2-93.
//!
//! Built-ins are **not special-cased**: Claude is a normal plugin directory
//! (`plugins/claude/`) bundled into the binary via `include_dir` and loaded through
//! the exact code path a third-party plugin will use — the anti-drift discipline of
//! `docs/05` §5.11.

use std::collections::BTreeMap;

use include_dir::{Dir, include_dir};
use serde::Deserialize;

/// The bundled first-party plugins, embedded from the repo's `plugins/` tree at build
/// time. Adding a first-party tool = adding a directory here.
static CLAUDE: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../plugins/claude");

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
    pub skills: Skills,
    pub mcp: Mcp,
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
/// `args` is substituted with the project's store path at setup time.
#[derive(Debug, Clone, Deserialize)]
pub struct Mcp {
    pub target: String,
    pub format: String,
    pub server_name: String,
    pub command: String,
    pub args: Vec<String>,
}

/// A loaded plugin: its manifest plus the (flat) set of files bundled with it, so a
/// setup writer can pull the instruction section + skill body by name.
#[derive(Debug, Clone)]
pub struct Plugin {
    pub manifest: Manifest,
    files: BTreeMap<String, String>,
}

impl Plugin {
    /// Load a plugin from a directory (embedded here; a real on-disk dir in HS2-92).
    /// Reads `manifest.toml` and captures the directory's top-level text files.
    fn from_dir(dir: &Dir) -> Result<Self, PluginError> {
        let raw = dir
            .get_file("manifest.toml")
            .ok_or_else(|| PluginError::MissingManifest(dir.path().display().to_string()))?
            .contents_utf8()
            .ok_or(PluginError::NotUtf8)?;
        let manifest: Manifest = toml::from_str(raw)?;

        let mut files = BTreeMap::new();
        for f in dir.files() {
            if let (Some(name), Some(text)) = (
                f.path().file_name().and_then(|n| n.to_str()),
                f.contents_utf8(),
            ) {
                files.insert(name.to_string(), text.to_string());
            }
        }

        let plugin = Plugin { manifest, files };
        // Fail loudly if the manifest points at files that aren't there.
        plugin.require(&plugin.manifest.instructions.section)?;
        plugin.require(&plugin.manifest.skills.source)?;
        Ok(plugin)
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

    /// A bundled file's contents by name.
    pub fn file(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }

    /// The Hot Sheet section to merge into the tool's instruction file.
    pub fn instructions_body(&self) -> &str {
        self.file(&self.manifest.instructions.section).unwrap_or("")
    }

    /// The worklist skill body written into the project.
    pub fn skill_body(&self) -> &str {
        self.file(&self.manifest.skills.source).unwrap_or("")
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
}

/// Every bundled first-party plugin, loaded through the same path a third-party
/// plugin will use. Panics only on a build-time-embedded malformed plugin (a bug).
pub fn builtin_plugins() -> Vec<Plugin> {
    [&CLAUDE]
        .into_iter()
        .map(|d| Plugin::from_dir(d).expect("bundled first-party plugin must load"))
        .collect()
}

/// Find a plugin by id (built-ins only, for now).
pub fn find(id: &str) -> Option<Plugin> {
    builtin_plugins().into_iter().find(|p| p.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_is_a_loadable_first_party_plugin() {
        let p = find("claude").expect("claude plugin present");
        assert_eq!(p.manifest.display_name, "Claude");
        assert_eq!(p.manifest.product_name, "Claude Code");
        assert_eq!(p.manifest.tier, "cli-agent");
        assert!(p.manifest.detection.binaries.iter().any(|b| b == "claude"));
    }

    #[test]
    fn claude_exposes_nonempty_setup_artifacts() {
        let p = find("claude").unwrap();

        // instruction section
        let instr = p.instructions_body();
        assert!(instr.contains("Hot Sheet"), "instruction section present");
        assert!(instr.contains("hotsheet ls --up-next"));

        // worklist skill with valid frontmatter
        let skill = p.skill_body();
        assert!(skill.starts_with("---"), "skill has frontmatter");
        assert!(skill.contains("name: hotsheet"));

        // targets the manifest declares
        assert_eq!(p.manifest.instructions.target, "CLAUDE.md");
        assert_eq!(p.manifest.skills.target, ".claude/skills/hotsheet/SKILL.md");
        assert_eq!(p.manifest.mcp.target, ".mcp.json");
        assert_eq!(p.manifest.mcp.server_name, "hotsheet");
    }

    #[test]
    fn mcp_args_substitute_the_store_path() {
        let p = find("claude").unwrap();
        assert_eq!(
            p.mcp_args("/work/proj"),
            vec!["--path".to_string(), "/work/proj".to_string()]
        );
        assert_eq!(p.manifest.mcp.command, "hotsheet-mcp");
    }

    #[test]
    fn builtins_all_load() {
        let all = builtin_plugins();
        assert!(!all.is_empty());
        assert!(all.iter().all(|p| !p.id().is_empty()));
    }

    #[test]
    fn unknown_plugin_is_none() {
        assert!(find("nope").is_none());
    }
}
