//! Claim backend configurations and their bindings (HS2-JJWESD, `docs/36` §36.13).
//!
//! A **configuration** names one claim backend: its kind, its non-secret settings, the
//! OS credential-store account that holds its key (never the key itself), and what to
//! do while it cannot be reached. A **namespace binding** points a resource namespace
//! at a configuration; a **source binding** points a ticket data source at one, so the
//! claims of that source's tickets go through it (HS2-WJF0D8). Several namespaces and
//! sources may share one configuration.
//!
//! Two scopes keep them:
//!
//! - **machine**: `<HOTSHEET_HOME>/resource-claims/configurations.json`, private to
//!   this machine (mode `0600`).
//! - **project**: `<project>/.hotsheet2/resource-claims.json`, checked in. A Git ticket
//!   store is a project too, so a binding written there travels to every clone.
//!
//! A project binding wins over a machine one, and may name a configuration from either
//! scope. Every mutation is idempotent and reports what it did.
//!
//! This lives in `ticketing` rather than `hotsheet-resource-claims` because ticket
//! claims (`ops::claim`) must read a source's binding, and the claims crate already
//! depends on this one.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::FileLock;
use crate::orchestration::OfflinePolicy;

/// The version this build writes and the newest it reads.
pub const REGISTRY_SCHEMA: u32 = 1;
/// The machine registry, relative to `HOTSHEET_HOME`.
pub const MACHINE_REGISTRY_FILE: &str = "resource-claims/configurations.json";
/// The project registry, relative to the project (checkout or ticket store) root.
pub const PROJECT_REGISTRY_FILE: &str = ".hotsheet2/resource-claims.json";
/// The lasting lock every registry writer takes beside the registry file. Machine-local:
/// a ticket store's managed `.gitignore` names it (HS2-JBF90Q).
pub(crate) const REGISTRY_LOCK_FILE: &str = ".resource-claims-config.lock";
/// [`REGISTRY_LOCK_FILE`] beside the project registry, relative to the project root.
pub(crate) const PROJECT_REGISTRY_LOCK_FILE: &str = ".hotsheet2/.resource-claims-config.lock";
/// The source key prefix that names a Git ticket store by its durable identity, shared
/// by every clone of that store.
pub const STORE_SOURCE_PREFIX: &str = "store:";
/// The prefix of the credential-store account that holds a configuration's key.
pub const CREDENTIAL_ACCOUNT_PREFIX: &str = "resource-claims.";
const MAX_ID_LEN: usize = 64;
const MAX_SETTING_LEN: usize = 1024;
const MAX_SOURCE_LEN: usize = 128;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    /// A configuration that is still bound cannot be removed.
    #[error("configuration '{id}' is still bound to {}; unbind them first", bindings.join(", "))]
    InUse { id: String, bindings: Vec<String> },
    #[error("{path} is unreadable or from a newer Hot Sheet; it was not changed")]
    Corrupt { path: String },
    #[error("claim configuration storage failed: {0}")]
    Io(#[from] std::io::Error),
}

impl RegistryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid",
            Self::NotFound(_) => "not_found",
            Self::InUse { .. } => "in_use",
            Self::Corrupt { .. } => "corrupt",
            Self::Io(_) => "storage",
        }
    }
}

/// Which backend implementation a configuration selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// The built-in machine file backend (`docs/36` §36.6). Never offline.
    Local,
    /// The built-in Git-ref compare-and-swap on a shared remote (HS2-WJF0D8).
    GitRef,
    /// An external plugin under `<HOTSHEET_HOME>/claim-backends/<id>/` (HS2-322ZSZ),
    /// named by the `plugin` setting. The plugin's own manifest declares the other
    /// settings, so the registry only checks their shape.
    External,
    /// The built-in Upstash Redis REST backend (HS2-K9J1RK). The REST token lives in the
    /// OS credential store, never in a setting.
    UpstashRedis,
}

impl BackendKind {
    pub fn parse(text: &str) -> Result<Self, RegistryError> {
        match text.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "local" => Ok(Self::Local),
            "git_ref" => Ok(Self::GitRef),
            "external" => Ok(Self::External),
            "upstash" | "upstash_redis" => Ok(Self::UpstashRedis),
            other => Err(RegistryError::Invalid(format!(
                "unknown backend kind '{other}'; use local, git-ref, upstash-redis or external"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::GitRef => "git_ref",
            Self::External => "external",
            Self::UpstashRedis => "upstash_redis",
        }
    }

    /// The settings this kind accepts. Anything else is refused, so a token pasted
    /// into a setting never reaches a checked-in file.
    pub fn setting_keys(self) -> &'static [&'static str] {
        match self {
            Self::Local => &[],
            // `remote`: the Git remote name (default `origin`). `repository`: the Git
            // repository whose remote serializes claims (default: the project or the
            // bound ticket store).
            Self::GitRef => &["remote", "repository"],
            // Only `plugin` is fixed; the plugin's manifest declares the rest.
            Self::External => &["plugin"],
            // `url`: the database's REST URL. `prefix`: the key prefix (default `hs2rc`).
            Self::UpstashRedis => &["url", "prefix"],
        }
    }

    /// Whether this kind can ever be unreachable.
    pub fn is_remote(self) -> bool {
        matches!(self, Self::GitRef)
    }
}

/// One backend configuration. It never holds secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendConfiguration {
    pub id: String,
    pub kind: BackendKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, String>,
    /// The OS credential-store account that holds this configuration's 32-byte key,
    /// when it has one (`resource-claims.<name>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    /// What to do while the backend cannot be reached (`docs/36` §36.7).
    #[serde(default)]
    pub offline: OfflinePolicy,
}

impl BackendConfiguration {
    pub fn new(id: &str, kind: BackendKind) -> Result<Self, RegistryError> {
        Ok(Self {
            id: validate_id(id, "configuration id")?,
            kind,
            settings: BTreeMap::new(),
            credential: None,
            offline: OfflinePolicy::FailClosed,
        })
    }

    /// Check settings, credential account and offline policy against the kind.
    pub fn validate(&self) -> Result<(), RegistryError> {
        validate_id(&self.id, "configuration id")?;
        let allowed = self.kind.setting_keys();
        for (key, value) in &self.settings {
            // An external plugin declares its own settings; here only their shape is
            // checked. Secret-typed ones are refused where the manifest is known.
            let declared_by_plugin = self.kind == BackendKind::External && valid_setting_key(key);
            if !declared_by_plugin && !allowed.contains(&key.as_str()) {
                return Err(RegistryError::Invalid(if allowed.is_empty() {
                    format!(
                        "a {} backend takes no settings ('{key}')",
                        self.kind.as_str()
                    )
                } else {
                    format!(
                        "unknown setting '{key}' for a {} backend; use {}. Secrets never go in settings",
                        self.kind.as_str(),
                        allowed.join(", ")
                    )
                }));
            }
            if value.is_empty()
                || value.len() > MAX_SETTING_LEN
                || value.chars().any(char::is_control)
            {
                return Err(RegistryError::Invalid(format!(
                    "setting '{key}' must be 1-{MAX_SETTING_LEN} printable characters"
                )));
            }
        }
        if let Some(account) = &self.credential {
            validate_credential(account)?;
        }
        if !self.kind.is_remote() && self.offline == OfflinePolicy::Provisional {
            return Err(RegistryError::Invalid(
                if matches!(self.kind, BackendKind::External | BackendKind::UpstashRedis) {
                    format!(
                        "a {} backend fails closed while it is unreachable; a provisional policy is not supported for it",
                        self.kind.as_str()
                    )
                } else {
                    format!(
                        "a {} backend is never offline, so it takes no provisional policy",
                        self.kind.as_str()
                    )
                },
            ));
        }
        if self.kind == BackendKind::UpstashRedis && self.setting("url").is_none() {
            return Err(RegistryError::Invalid(
                "an upstash-redis backend needs --setting url=<the database's REST URL>".into(),
            ));
        }
        if self.kind == BackendKind::External && self.setting("plugin").is_none() {
            return Err(RegistryError::Invalid(
                "an external backend needs --setting plugin=<id> naming a directory under claim-backends/".into(),
            ));
        }
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Option<&str> {
        self.settings.get(key).map(String::as_str)
    }

    /// The default credential account for a configuration id.
    pub fn default_credential(id: &str) -> String {
        format!("{CREDENTIAL_ACCOUNT_PREFIX}{id}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamespaceBinding {
    pub namespace: String,
    pub configuration: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceBinding {
    /// A ticket connection id, or `store:<instanceId>` for every clone of a Git store.
    pub source: String,
    pub configuration: String,
}

/// One scope's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRegistry {
    #[serde(rename = "$hotsheetSchema")]
    pub schema: u32,
    #[serde(default)]
    pub configurations: Vec<BackendConfiguration>,
    #[serde(default)]
    pub namespaces: Vec<NamespaceBinding>,
    #[serde(default)]
    pub sources: Vec<SourceBinding>,
}

impl Default for ClaimRegistry {
    fn default() -> Self {
        Self {
            schema: REGISTRY_SCHEMA,
            configurations: Vec::new(),
            namespaces: Vec::new(),
            sources: Vec::new(),
        }
    }
}

/// What an idempotent mutation did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Created,
    Updated,
    Unchanged,
    Removed,
    Absent,
}

impl ClaimRegistry {
    pub fn configuration(&self, id: &str) -> Option<&BackendConfiguration> {
        self.configurations.iter().find(|c| c.id == id)
    }

    pub fn namespace_binding(&self, namespace: &str) -> Option<&str> {
        self.namespaces
            .iter()
            .find(|b| b.namespace == namespace)
            .map(|b| b.configuration.as_str())
    }

    pub fn source_binding(&self, source: &str) -> Option<&str> {
        self.sources
            .iter()
            .find(|b| b.source == source)
            .map(|b| b.configuration.as_str())
    }

    /// Everything bound to a configuration, as `namespace <n>` / `source <s>`.
    pub fn bindings_of(&self, id: &str) -> Vec<String> {
        let namespaces = self
            .namespaces
            .iter()
            .filter(|b| b.configuration == id)
            .map(|b| format!("namespace {}", b.namespace));
        let sources = self
            .sources
            .iter()
            .filter(|b| b.configuration == id)
            .map(|b| format!("source {}", b.source));
        namespaces.chain(sources).collect()
    }

    pub fn upsert_configuration(
        &mut self,
        configuration: BackendConfiguration,
    ) -> Result<Change, RegistryError> {
        configuration.validate()?;
        match self
            .configurations
            .iter_mut()
            .find(|c| c.id == configuration.id)
        {
            Some(existing) if *existing == configuration => Ok(Change::Unchanged),
            Some(existing) => {
                *existing = configuration;
                Ok(Change::Updated)
            }
            None => {
                self.configurations.push(configuration);
                self.configurations.sort_by(|a, b| a.id.cmp(&b.id));
                Ok(Change::Created)
            }
        }
    }

    /// Remove a configuration. Refused while anything in this scope is bound to it.
    pub fn remove_configuration(&mut self, id: &str) -> Result<Change, RegistryError> {
        let bindings = self.bindings_of(id);
        if !bindings.is_empty() {
            return Err(RegistryError::InUse {
                id: id.to_owned(),
                bindings,
            });
        }
        let before = self.configurations.len();
        self.configurations.retain(|c| c.id != id);
        Ok(if self.configurations.len() < before {
            Change::Removed
        } else {
            Change::Absent
        })
    }

    pub fn bind_namespace(&mut self, namespace: &str, configuration: &str) -> Change {
        bind(
            &mut self.namespaces,
            |b| b.namespace == namespace,
            |b| &mut b.configuration,
            configuration,
            || NamespaceBinding {
                namespace: namespace.to_owned(),
                configuration: configuration.to_owned(),
            },
        )
    }

    pub fn unbind_namespace(&mut self, namespace: &str) -> Change {
        unbind(&mut self.namespaces, |b| b.namespace == namespace)
    }

    pub fn bind_source(&mut self, source: &str, configuration: &str) -> Change {
        bind(
            &mut self.sources,
            |b| b.source == source,
            |b| &mut b.configuration,
            configuration,
            || SourceBinding {
                source: source.to_owned(),
                configuration: configuration.to_owned(),
            },
        )
    }

    pub fn unbind_source(&mut self, source: &str) -> Change {
        unbind(&mut self.sources, |b| b.source == source)
    }
}

fn bind<T>(
    list: &mut Vec<T>,
    matches: impl Fn(&T) -> bool,
    field: impl Fn(&mut T) -> &mut String,
    configuration: &str,
    create: impl FnOnce() -> T,
) -> Change {
    match list.iter_mut().find(|b| matches(b)) {
        Some(existing) => {
            let current = field(existing);
            if current == configuration {
                return Change::Unchanged;
            }
            *current = configuration.to_owned();
            Change::Updated
        }
        None => {
            list.push(create());
            Change::Created
        }
    }
}

fn unbind<T>(list: &mut Vec<T>, matches: impl Fn(&T) -> bool) -> Change {
    let before = list.len();
    list.retain(|b| !matches(b));
    if list.len() < before {
        Change::Removed
    } else {
        Change::Absent
    }
}

/// Where a registry lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Machine { home: PathBuf },
    Project { root: PathBuf },
}

impl Scope {
    pub fn path(&self) -> PathBuf {
        match self {
            Self::Machine { home } => home.join(MACHINE_REGISTRY_FILE),
            Self::Project { root } => root.join(PROJECT_REGISTRY_FILE),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Machine { .. } => "machine",
            Self::Project { .. } => "project",
        }
    }

    /// Read the registry. A missing file is an empty registry; an unreadable or newer
    /// one is refused and never overwritten.
    pub fn load(&self) -> Result<ClaimRegistry, RegistryError> {
        let path = self.path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ClaimRegistry::default());
            }
            Err(error) => return Err(error.into()),
        };
        let corrupt = || RegistryError::Corrupt {
            path: path.display().to_string(),
        };
        let registry: ClaimRegistry = serde_json::from_str(&text).map_err(|_| corrupt())?;
        if registry.schema == 0 || registry.schema > REGISTRY_SCHEMA {
            return Err(corrupt());
        }
        Ok(registry)
    }

    /// Change the registry under its file lock and publish it atomically. `change`
    /// runs against the latest state; nothing is written when it fails or changes
    /// nothing.
    pub fn update<T>(
        &self,
        change: impl FnOnce(&mut ClaimRegistry) -> Result<T, RegistryError>,
    ) -> Result<T, RegistryError> {
        let path = self.path();
        let parent = path
            .parent()
            .ok_or_else(|| RegistryError::Invalid("registry path has no parent".into()))?;
        fs::create_dir_all(parent)?;
        restrict_dir(parent, self);
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(parent.join(REGISTRY_LOCK_FILE))?;
        let _lock = FileLock::acquire(lock_file)?;
        let mut registry = self.load()?;
        let before = registry.clone();
        let result = change(&mut registry)?;
        if registry != before {
            let mut json = serde_json::to_string_pretty(&registry)
                .map_err(|error| RegistryError::Invalid(error.to_string()))?;
            json.push('\n');
            crate::store::write_file_atomically(&path, json.as_bytes())?;
            restrict_file(&path, self);
        }
        Ok(result)
    }
}

#[cfg(unix)]
fn restrict_dir(dir: &Path, scope: &Scope) {
    use std::os::unix::fs::PermissionsExt;
    if matches!(scope, Scope::Machine { .. }) {
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
}

#[cfg(not(unix))]
fn restrict_dir(_dir: &Path, _scope: &Scope) {}

#[cfg(unix)]
fn restrict_file(path: &Path, scope: &Scope) {
    use std::os::unix::fs::PermissionsExt;
    if matches!(scope, Scope::Machine { .. }) {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path, _scope: &Scope) {}

/// A configuration found for a namespace or source, with the scope it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub configuration: BackendConfiguration,
    /// The scope of the binding (`machine` or `project`).
    pub binding_scope: &'static str,
    /// The project root when the binding is project-scoped.
    pub project: Option<PathBuf>,
}

/// The two registries a lookup consults.
#[derive(Debug, Clone)]
pub struct Registries {
    pub machine: ClaimRegistry,
    pub project: Option<(PathBuf, ClaimRegistry)>,
}

impl Registries {
    pub fn load(home: &Path, project: Option<&Path>) -> Result<Self, RegistryError> {
        Ok(Self {
            machine: Scope::Machine {
                home: home.to_path_buf(),
            }
            .load()?,
            project: match project {
                Some(root) => Some((
                    root.to_path_buf(),
                    Scope::Project {
                        root: root.to_path_buf(),
                    }
                    .load()?,
                )),
                None => None,
            },
        })
    }

    /// The configuration bound to a namespace, project first. `None` means unbound.
    pub fn namespace(&self, namespace: &str) -> Result<Option<Resolved>, RegistryError> {
        self.resolve(|registry| registry.namespace_binding(namespace))
    }

    /// The configuration bound to the first of `sources` that has a binding, project
    /// first.
    pub fn source(&self, sources: &[String]) -> Result<Option<Resolved>, RegistryError> {
        self.resolve(|registry| sources.iter().find_map(|s| registry.source_binding(s)))
    }

    fn resolve<'a>(
        &'a self,
        binding: impl Fn(&'a ClaimRegistry) -> Option<&'a str>,
    ) -> Result<Option<Resolved>, RegistryError> {
        if let Some((root, project)) = &self.project
            && let Some(id) = binding(project)
        {
            let configuration = project
                .configuration(id)
                .or_else(|| self.machine.configuration(id))
                .ok_or_else(|| missing(id))?;
            return Ok(Some(Resolved {
                configuration: configuration.clone(),
                binding_scope: "project",
                project: Some(root.clone()),
            }));
        }
        if let Some(id) = binding(&self.machine) {
            let configuration = self.machine.configuration(id).ok_or_else(|| missing(id))?;
            return Ok(Some(Resolved {
                configuration: configuration.clone(),
                binding_scope: "machine",
                project: None,
            }));
        }
        Ok(None)
    }

    /// A configuration by id, project first.
    pub fn configuration(&self, id: &str) -> Option<(&'static str, &BackendConfiguration)> {
        self.project
            .as_ref()
            .and_then(|(_, p)| p.configuration(id))
            .map(|c| ("project", c))
            .or_else(|| self.machine.configuration(id).map(|c| ("machine", c)))
    }
}

fn missing(id: &str) -> RegistryError {
    RegistryError::NotFound(format!(
        "configuration '{id}' is bound but not defined; add it with `hotsheet-cli resource backend add {id}`"
    ))
}

/// The source keys a Git ticket store answers to, most shared first:
/// `store:<instanceId>` (every clone) and its local connection id.
pub fn git_store_sources(store: &crate::FsStore) -> Vec<String> {
    let mut sources = Vec::new();
    if let Ok(Some(instance)) = store.metadata().map(|m| m.instance_id) {
        sources.push(format!("{STORE_SOURCE_PREFIX}{instance}"));
    }
    sources.push(crate::provider::git_connection_id(store));
    sources
}

/// Check an id: 1-64 characters of `a-z`, `0-9`, `.`, `_`, `-`, starting with a letter
/// or digit.
/// A plugin-declared setting key: a short lowercase identifier.
pub fn valid_setting_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 48
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub fn validate_id(value: &str, what: &str) -> Result<String, RegistryError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        && !value.contains("..");
    if valid {
        Ok(value.to_owned())
    } else {
        Err(RegistryError::Invalid(format!(
            "{what} must be 1-{MAX_ID_LEN} characters of a-z, 0-9, '.', '_' or '-', starting with a letter or digit"
        )))
    }
}

/// Check a source key: a connection id or `store:<instanceId>`.
pub fn validate_source(value: &str) -> Result<String, RegistryError> {
    let body = value.strip_prefix(STORE_SOURCE_PREFIX).unwrap_or(value);
    let valid = !body.is_empty()
        && value.len() <= MAX_SOURCE_LEN
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if valid {
        Ok(value.to_owned())
    } else {
        Err(RegistryError::Invalid(
            "a source is a connection id or store:<instanceId> (letters, digits, '.', '_' or '-')"
                .into(),
        ))
    }
}

fn validate_credential(account: &str) -> Result<(), RegistryError> {
    let ok = account
        .strip_prefix(CREDENTIAL_ACCOUNT_PREFIX)
        .is_some_and(|rest| validate_id(rest, "credential").is_ok());
    if ok {
        Ok(())
    } else {
        Err(RegistryError::Invalid(format!(
            "a credential names an OS credential-store account '{CREDENTIAL_ACCOUNT_PREFIX}<name>', never a secret value"
        )))
    }
}

#[cfg(test)]
#[path = "claim_backends_tests.rs"]
mod tests;
