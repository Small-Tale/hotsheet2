//! The code-bearing plugin manifest (docs/38 §38.3): a `manifest.toml` with an
//! `[executable]` table and the capabilities the plugin implements. A manifest without
//! `[executable]` is a data plugin and is not this module's concern.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use hotsheet_plugin_protocol::PROTOCOL_VERSION;
use serde::Deserialize;

/// The manifest file inside a plugin directory.
pub const MANIFEST_FILE: &str = "manifest.toml";

/// Capabilities a code-bearing plugin may declare.
pub const KNOWN_CAPABILITIES: &[&str] = &["ticket_source", "ai_drive", "notifier", "diagnostics"];

/// A parsed, validated code-bearing manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodePluginManifest {
    pub id: String,
    pub display_name: String,
    pub version: String,
    /// The protocol versions the plugin speaks, inclusive.
    pub protocol: (u32, u32),
    pub capabilities: Vec<String>,
    pub executable: Executable,
    pub settings: Vec<Setting>,
}

/// How to start the plugin.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Executable {
    /// Path relative to the plugin directory. Never looked up on `PATH`.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Per-platform programs keyed by `<os>-<arch>` as Rust names them
    /// (`macos-aarch64`, `linux-x86_64`, `windows-x86_64`).
    #[serde(default)]
    pub targets: BTreeMap<String, String>,
}

/// A user-provided setting, typed as in notifier and claim-backend manifests.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: SettingKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub choices: Vec<String>,
}

/// A setting's type. Secrets live in the OS keychain and are sent only in-protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKind {
    String,
    Secret,
    Choice,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    id: String,
    display_name: String,
    version: String,
    protocol: RawProtocol,
    capabilities: Vec<String>,
    executable: Executable,
    #[serde(default)]
    settings: Vec<Setting>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProtocol {
    min: u32,
    max: u32,
}

/// Whether a manifest's text declares an executable, without fully validating it.
pub fn is_code_bearing(manifest_text: &str) -> bool {
    toml::from_str::<toml::Table>(manifest_text)
        .map(|table| table.contains_key("executable"))
        .unwrap_or(false)
}

/// The same id rule as AI-tool plugins (`hotsheet-plugins::is_valid_plugin_id`):
/// `[a-z0-9][a-z0-9-]{0,63}`.
pub fn is_valid_plugin_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// The current platform's `<os>-<arch>` key.
pub fn platform_key() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

impl CodePluginManifest {
    /// Parses and validates manifest text. `dir_name` must equal the id.
    pub fn parse(text: &str, dir_name: &str) -> Result<Self, String> {
        let raw: Raw = toml::from_str(text).map_err(|e| format!("invalid manifest: {e}"))?;
        if !is_valid_plugin_id(&raw.id) {
            return Err(format!("invalid plugin id {:?}", raw.id));
        }
        if raw.id != dir_name {
            return Err(format!(
                "plugin id {:?} must match its directory name {dir_name:?}",
                raw.id
            ));
        }
        if raw.display_name.trim().is_empty() || raw.version.trim().is_empty() {
            return Err("display_name and version are required".to_string());
        }
        if raw.protocol.min > raw.protocol.max {
            return Err(format!(
                "protocol range {}..={} is empty",
                raw.protocol.min, raw.protocol.max
            ));
        }
        if !(raw.protocol.min..=raw.protocol.max).contains(&PROTOCOL_VERSION) {
            return Err(format!(
                "plugin speaks protocol {}..={}, this Hot Sheet speaks {PROTOCOL_VERSION}",
                raw.protocol.min, raw.protocol.max
            ));
        }
        if raw.capabilities.is_empty() {
            return Err("at least one capability is required".to_string());
        }
        for capability in &raw.capabilities {
            if !KNOWN_CAPABILITIES.contains(&capability.as_str()) {
                return Err(format!(
                    "unknown capability {capability:?}; known: {KNOWN_CAPABILITIES:?}"
                ));
            }
        }
        let mut programs = vec![raw.executable.program.as_str()];
        programs.extend(raw.executable.targets.values().map(String::as_str));
        for program in programs {
            if !is_safe_relative(program) {
                return Err(format!(
                    "executable {program:?} must be a relative path inside the plugin directory"
                ));
            }
        }
        let mut keys = std::collections::BTreeSet::new();
        for setting in &raw.settings {
            if setting.key.trim().is_empty() || !keys.insert(setting.key.as_str()) {
                return Err(format!(
                    "setting key {:?} is empty or repeated",
                    setting.key
                ));
            }
            if (setting.kind == SettingKind::Choice) == setting.choices.is_empty() {
                return Err(format!(
                    "setting {:?}: choices are required for, and only for, type \"choice\"",
                    setting.key
                ));
            }
        }
        Ok(CodePluginManifest {
            id: raw.id,
            display_name: raw.display_name,
            version: raw.version,
            protocol: (raw.protocol.min, raw.protocol.max),
            capabilities: raw.capabilities,
            executable: raw.executable,
            settings: raw.settings,
        })
    }

    /// Reads `dir/manifest.toml`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(dir.join(MANIFEST_FILE))
            .map_err(|e| format!("{}: {e}", dir.join(MANIFEST_FILE).display()))?;
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("{} has no usable directory name", dir.display()))?;
        Self::parse(&text, name)
    }

    /// The program for this platform, resolved inside `dir`. It must exist; on Unix it
    /// must also be executable.
    pub fn resolve_program(&self, dir: &Path) -> Result<PathBuf, String> {
        let relative = self
            .executable
            .targets
            .get(&platform_key())
            .unwrap_or(&self.executable.program);
        let path = dir.join(relative);
        let metadata = std::fs::metadata(&path)
            .map_err(|e| format!("plugin executable {}: {e}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!(
                "plugin executable {} is not a file",
                path.display()
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err(format!(
                    "plugin executable {} is not executable",
                    path.display()
                ));
            }
        }
        Ok(path)
    }
}

fn is_safe_relative(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}
