//! Connections and bindings (HS2-8X2S08, `docs/37` §37.3): a machine-local, secret-free file
//! `<HOTSHEET_HOME>/remote-change/connections.json`. A *connection* names an adapter and its
//! plain settings and is reusable; a *binding* ties one source (a code checkout or a Git
//! ticket store) to a connection through a channel, with the repository identity the source
//! expects and the refs it cares about. Credentials live in the OS credential store.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use hotsheet_ticketing::claim_backends::Change;

use super::model::RepoIdentity;

pub const REGISTRY_DIR: &str = "remote-change";
pub const REGISTRY_FILE: &str = "connections.json";
pub const REGISTRY_SCHEMA: u32 = 1;
/// The credential-store account prefix: `remote-change.<connection id>`.
pub const CREDENTIAL_PREFIX: &str = "remote-change.";
const MAX_SETTING_LEN: usize = 1024;
const MAX_BINDINGS: usize = 256;
const MAX_REFS: usize = 16;
/// The refs a binding follows unless it says otherwise: branches, not tags.
pub const DEFAULT_REF: &str = "refs/heads/*";

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("not_found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error(
        "the remote-change registry is from a newer Hot Sheet or damaged and was left as it is"
    )]
    Corrupt,
    #[error("remote-change storage failed: {0}")]
    Io(#[from] std::io::Error),
}

impl RegistryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Corrupt => "corrupt",
            Self::Io(_) => "storage",
        }
    }
}

/// A short lowercase id, safe in paths and credential account names.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// A source: `store:<instance id>` for a Git ticket store (every clone answers to it) or
/// `checkout:<checkout id>` for a project's code checkout.
pub fn valid_source(source: &str) -> bool {
    let Some((kind, id)) = source.split_once(':') else {
        return false;
    };
    matches!(kind, "store" | "checkout")
        && !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// A channel name as the service spells it. Printable, bounded, no whitespace.
pub fn valid_channel(channel: &str) -> bool {
    !channel.is_empty()
        && channel.len() <= 128
        && channel
            .chars()
            .all(|c| !c.is_control() && !c.is_whitespace())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    /// The adapter id (`ably`).
    pub plugin: String,
    /// Plain settings only; the adapter's descriptor names the keys it takes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, String>,
    /// The OS credential-store account that holds the credential, if the adapter needs one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

impl Connection {
    pub fn new(id: &str, plugin: &str) -> Result<Self, RegistryError> {
        if !valid_id(id) {
            return Err(RegistryError::Invalid(
                "a connection id is 1-48 characters of a-z, 0-9, '-' or '_'".into(),
            ));
        }
        if !valid_id(plugin) {
            return Err(RegistryError::Invalid(
                "a plugin id is a short lowercase slug".into(),
            ));
        }
        Ok(Self {
            id: id.to_owned(),
            plugin: plugin.to_owned(),
            settings: BTreeMap::new(),
            credential: None,
        })
    }

    pub fn default_credential(id: &str) -> String {
        format!("{CREDENTIAL_PREFIX}{id}")
    }

    fn validate(&self) -> Result<(), RegistryError> {
        for (key, value) in &self.settings {
            let key_ok = !key.is_empty()
                && key.len() <= 48
                && key
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !key_ok
                || value.is_empty()
                || value.len() > MAX_SETTING_LEN
                || value.chars().any(char::is_control)
            {
                return Err(RegistryError::Invalid(format!(
                    "setting '{key}' must have a lowercase key and a printable value of at most {MAX_SETTING_LEN} characters"
                )));
            }
        }
        Ok(())
    }
}

/// Source + connection + channel is a binding's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub source: String,
    pub connection: String,
    pub channel: String,
    /// The repository this source expects events about.
    pub repository: RepoIdentity,
    /// Refs of interest: an exact name, or a prefix ending in `*`.
    pub refs: Vec<String>,
}

impl Binding {
    pub fn key(&self) -> (&str, &str, &str) {
        (&self.source, &self.connection, &self.channel)
    }

    /// Whether `ref_name` is one this binding follows.
    pub fn follows(&self, ref_name: &str) -> bool {
        self.refs
            .iter()
            .any(|pattern| match pattern.strip_suffix('*') {
                Some(prefix) => ref_name.starts_with(prefix),
                None => ref_name == pattern,
            })
    }

    fn validate(&self) -> Result<(), RegistryError> {
        if !valid_source(&self.source) {
            return Err(RegistryError::Invalid(
                "a source is store:<instance id> or checkout:<checkout id>".into(),
            ));
        }
        if !valid_id(&self.connection) {
            return Err(RegistryError::Invalid(
                "the connection id is not valid".into(),
            ));
        }
        if !valid_channel(&self.channel) {
            return Err(RegistryError::Invalid(
                "a channel is 1-128 printable characters without whitespace".into(),
            ));
        }
        if self.repository.is_empty() {
            return Err(RegistryError::Invalid(
                "a binding needs the repository it expects: --repository owner/name and/or --repository-id".into(),
            ));
        }
        if let Some(name) = &self.repository.full_name {
            let parts: Vec<&str> = name.split('/').collect();
            let ok = parts.len() == 2
                && parts.iter().all(|p| {
                    !p.is_empty()
                        && p.len() <= 100
                        && p.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                });
            if !ok {
                return Err(RegistryError::Invalid(
                    "a repository is written owner/name".into(),
                ));
            }
        }
        if self.refs.is_empty() || self.refs.len() > MAX_REFS {
            return Err(RegistryError::Invalid(format!(
                "a binding follows 1-{MAX_REFS} refs"
            )));
        }
        for pattern in &self.refs {
            let body = pattern.strip_suffix('*').unwrap_or(pattern);
            if !pattern.starts_with("refs/")
                || body.contains('*')
                || pattern.chars().any(char::is_whitespace)
            {
                return Err(RegistryError::Invalid(format!(
                    "ref '{pattern}' must start with refs/ and may end in one '*'"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(rename = "$hotsheetSchema")]
    pub schema: u32,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            schema: REGISTRY_SCHEMA,
            connections: Vec::new(),
            bindings: Vec::new(),
        }
    }
}

pub fn registry_path(home: &Path) -> PathBuf {
    home.join(REGISTRY_DIR).join(REGISTRY_FILE)
}

impl Registry {
    /// Read the registry; a missing file is an empty one. A damaged file or one from a newer
    /// schema is an error and is never overwritten.
    pub fn load(home: &Path) -> Result<Self, RegistryError> {
        match fs::read_to_string(registry_path(home)) {
            Ok(text) => {
                let registry: Registry =
                    serde_json::from_str(&text).map_err(|_| RegistryError::Corrupt)?;
                if registry.schema > REGISTRY_SCHEMA {
                    return Err(RegistryError::Corrupt);
                }
                Ok(registry)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Load, change and save under one exclusive lock, so two processes never lose each
    /// other's change. The file is private to its owner and replaced atomically.
    pub fn update<T>(
        home: &Path,
        change: impl FnOnce(&mut Registry) -> Result<T, RegistryError>,
    ) -> Result<T, RegistryError> {
        let dir = home.join(REGISTRY_DIR);
        fs::create_dir_all(&dir)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(".lock"))?;
        let _guard = hotsheet_ticketing::FileLock::acquire(lock)?;
        let mut registry = Self::load(home)?;
        let before = registry.clone();
        let value = change(&mut registry)?;
        if registry != before {
            registry.connections.sort_by(|a, b| a.id.cmp(&b.id));
            registry.bindings.sort_by(|a, b| a.key().cmp(&b.key()));
            let text = serde_json::to_string_pretty(&registry)
                .map_err(|e| RegistryError::Invalid(e.to_string()))?;
            let mut staged = tempfile::Builder::new()
                .prefix(".connections-")
                .tempfile_in(&dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                staged
                    .as_file()
                    .set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            std::io::Write::write_all(&mut staged, text.as_bytes())?;
            staged.as_file().sync_all()?;
            staged
                .persist(registry_path(home))
                .map_err(|e| RegistryError::Io(e.error))?;
        }
        Ok(value)
    }

    pub fn connection(&self, id: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.id == id)
    }

    pub fn bindings_of(&self, connection: &str) -> Vec<&Binding> {
        self.bindings
            .iter()
            .filter(|b| b.connection == connection)
            .collect()
    }

    /// Add or change a connection. The same definition again is `Unchanged`.
    pub fn upsert_connection(&mut self, connection: Connection) -> Result<Change, RegistryError> {
        connection.validate()?;
        match self.connections.iter_mut().find(|c| c.id == connection.id) {
            Some(existing) if *existing == connection => Ok(Change::Unchanged),
            Some(existing) => {
                *existing = connection;
                Ok(Change::Updated)
            }
            None => {
                self.connections.push(connection);
                Ok(Change::Created)
            }
        }
    }

    /// Remove a connection, refused while a binding uses it.
    pub fn remove_connection(&mut self, id: &str) -> Result<Change, RegistryError> {
        let bound = self.bindings_of(id);
        if !bound.is_empty() {
            let names: Vec<String> = bound
                .iter()
                .map(|b| format!("{} on {}", b.source, b.channel))
                .collect();
            return Err(RegistryError::Conflict(format!(
                "connection '{id}' is still bound ({}); unbind it first",
                names.join(", ")
            )));
        }
        let before = self.connections.len();
        self.connections.retain(|c| c.id != id);
        Ok(if self.connections.len() < before {
            Change::Removed
        } else {
            Change::Absent
        })
    }

    /// Bind a source to a connection's channel. The connection must exist.
    pub fn bind(&mut self, binding: Binding) -> Result<Change, RegistryError> {
        binding.validate()?;
        if self.connection(&binding.connection).is_none() {
            return Err(RegistryError::NotFound(format!(
                "no connection '{}'",
                binding.connection
            )));
        }
        match self.bindings.iter_mut().find(|b| b.key() == binding.key()) {
            Some(existing) if *existing == binding => Ok(Change::Unchanged),
            Some(existing) => {
                *existing = binding;
                Ok(Change::Updated)
            }
            None => {
                if self.bindings.len() >= MAX_BINDINGS {
                    return Err(RegistryError::Invalid(format!(
                        "at most {MAX_BINDINGS} bindings"
                    )));
                }
                self.bindings.push(binding);
                Ok(Change::Created)
            }
        }
    }

    /// Remove one binding; absent is not an error.
    pub fn unbind(&mut self, source: &str, connection: &str, channel: &str) -> Change {
        let before = self.bindings.len();
        self.bindings
            .retain(|b| b.key() != (source, connection, channel));
        if self.bindings.len() < before {
            Change::Removed
        } else {
            Change::Absent
        }
    }

    /// The channels each connection must be attached to: one subscription per distinct
    /// (connection, channel), however many sources share it.
    pub fn subscriptions(&self) -> BTreeMap<String, Vec<String>> {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for binding in &self.bindings {
            let channels = map.entry(binding.connection.clone()).or_default();
            if !channels.contains(&binding.channel) {
                channels.push(binding.channel.clone());
            }
        }
        for channels in map.values_mut() {
            channels.sort();
        }
        map
    }
}
