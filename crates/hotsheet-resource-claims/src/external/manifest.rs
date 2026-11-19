//! A claim-backend plugin's `manifest.toml` (HS2-322ZSZ, `docs/36` §36.16): a directory
//! `<HOTSHEET_HOME>/claim-backends/<id>/` that declares what the plugin guarantees, how it
//! waits, which settings it takes, and the command that serves each operation.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::{
    Capabilities, Guarantee, MAX_LEASE, MAX_METADATA_BYTES, MAX_METADATA_ENTRIES, WaitMechanism,
};

pub const MANIFEST_FILE: &str = "manifest.toml";
/// Ids a plugin may not take: the built-in kinds win a collision.
pub const RESERVED_IDS: &[&str] = &[
    "local",
    "git-ref",
    "git_ref",
    "upstash",
    "upstash-redis",
    "upstash_redis",
    "external",
];
const MAX_TIMEOUT_SECONDS: u64 = 300;

/// How a plugin's settings are typed. A `secret` never appears in a configuration file:
/// it lives in the OS credential store and travels only on the plugin's stdin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingType {
    String,
    Secret,
    Choice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingSpec {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: SettingType,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// How a plugin waits for a resource to be released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestWait {
    /// The plugin has no `wait` operation: only try-now acquire works.
    #[default]
    None,
    /// The plugin implements `wait`: one invocation blocks, on the plugin's own push
    /// signal, until the resource is free or the timeout passes.
    Blocking,
}

#[derive(Debug, Clone, Deserialize)]
struct RawCapabilities {
    guarantee: Guarantee,
    #[serde(default)]
    wait: ManifestWait,
    max_metadata_entries: Option<usize>,
    max_metadata_bytes: Option<usize>,
    max_lease_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawManifest {
    id: String,
    display_name: String,
    #[serde(default)]
    description: String,
    command: Vec<String>,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
    capabilities: RawCapabilities,
    #[serde(default)]
    settings: Vec<SettingSpec>,
}

fn default_timeout() -> u64 {
    30
}

/// A loaded plugin: the manifest, resolved against its directory.
#[derive(Debug, Clone)]
pub struct PluginManifest {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub dir: PathBuf,
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub guarantee: Guarantee,
    pub wait: ManifestWait,
    pub max_metadata_entries: usize,
    pub max_metadata_bytes: usize,
    pub max_lease: Duration,
    pub settings: Vec<SettingSpec>,
}

/// Plugin ids are short lowercase slugs, safe in paths and credential account names.
pub fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

impl PluginManifest {
    /// Load `<root>/<id>/manifest.toml`, which must declare that same id.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join(MANIFEST_FILE);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let raw: RawManifest =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if !valid_plugin_id(&raw.id) {
            return Err(format!(
                "{}: id '{}' must be lowercase letters, digits, '-' or '_'",
                path.display(),
                raw.id
            ));
        }
        if RESERVED_IDS.contains(&raw.id.as_str()) {
            return Err(format!(
                "{}: id '{}' is a built-in backend and always wins; rename the plugin",
                path.display(),
                raw.id
            ));
        }
        let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if folder != raw.id {
            return Err(format!(
                "{}: id '{}' must match its directory name '{folder}'",
                path.display(),
                raw.id
            ));
        }
        let (program, args) = raw
            .command
            .split_first()
            .ok_or_else(|| format!("{}: command must name a program", path.display()))?;
        for setting in &raw.settings {
            if !hotsheet_ticketing::claim_backends::valid_setting_key(&setting.key)
                || setting.key == "plugin"
            {
                return Err(format!(
                    "{}: setting key '{}' must be lowercase letters, digits or '_' and not 'plugin'",
                    path.display(),
                    setting.key
                ));
            }
            if setting.kind == SettingType::Choice && setting.choices.is_empty() {
                return Err(format!(
                    "{}: choice setting '{}' lists no choices",
                    path.display(),
                    setting.key
                ));
            }
        }
        let mut keys: Vec<&str> = raw.settings.iter().map(|s| s.key.as_str()).collect();
        keys.sort_unstable();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(format!(
                "{}: a setting key is declared twice",
                path.display()
            ));
        }
        let local = dir.join(program);
        let program = if Path::new(program).is_relative() && local.is_file() {
            local.display().to_string()
        } else {
            program.clone()
        };
        let args = args
            .iter()
            .map(|arg| {
                let candidate = dir.join(arg);
                if Path::new(arg).is_relative() && arg.contains('/') && candidate.is_file() {
                    candidate.display().to_string()
                } else {
                    arg.clone()
                }
            })
            .collect();
        let caps = raw.capabilities;
        Ok(Self {
            id: raw.id,
            display_name: raw.display_name,
            description: raw.description,
            dir: dir.to_path_buf(),
            program,
            args,
            timeout: Duration::from_secs(raw.timeout_seconds.clamp(1, MAX_TIMEOUT_SECONDS)),
            guarantee: caps.guarantee,
            wait: caps.wait,
            max_metadata_entries: caps
                .max_metadata_entries
                .map_or(MAX_METADATA_ENTRIES, |n| n.min(MAX_METADATA_ENTRIES)),
            max_metadata_bytes: caps
                .max_metadata_bytes
                .map_or(MAX_METADATA_BYTES, |n| n.min(MAX_METADATA_BYTES)),
            max_lease: caps
                .max_lease_seconds
                .map_or(MAX_LEASE, |s| Duration::from_secs(s).min(MAX_LEASE)),
            settings: raw.settings,
        })
    }

    /// What this plugin advertises. Hot Sheet's own bounds cap what a plugin may claim.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            backend: self.id.clone(),
            guarantee: self.guarantee,
            wait: match self.wait {
                ManifestWait::None => WaitMechanism::None,
                ManifestWait::Blocking => WaitMechanism::LongPoll,
            },
            max_metadata_entries: self.max_metadata_entries,
            max_metadata_bytes: self.max_metadata_bytes,
            max_lease_ms: u64::try_from(self.max_lease.as_millis()).unwrap_or(u64::MAX),
        }
    }

    pub fn setting(&self, key: &str) -> Option<&SettingSpec> {
        self.settings.iter().find(|s| s.key == key)
    }
}

/// Every plugin directory under `root`, plus a message per directory that failed to load.
/// A broken plugin never hides the others.
pub fn load_all(root: &Path) -> (Vec<PluginManifest>, Vec<String>) {
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return (loaded, errors);
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        if !dir.join(MANIFEST_FILE).is_file() {
            errors.push(format!("{}: no {MANIFEST_FILE}", dir.display()));
            continue;
        }
        match PluginManifest::load(&dir) {
            Ok(plugin) => loaded.push(plugin),
            Err(error) => errors.push(error),
        }
    }
    (loaded, errors)
}
