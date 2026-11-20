//! One provider configuration per machine (HS2-7QPJ05, after HS2-1M8E9D).
//!
//! External provider connections (GitHub, GitLab, Jira) hold keychain references, a
//! signed-in account, and endpoint operations that name this machine's checkouts. Every
//! store and project on a machine shares one registry, `${HOTSHEET_HOME}/providers.json`,
//! so the desktop host (which serves the metadata-only anchor store) and a `hotsheet-cli`
//! run inside any project read and write the same file. Connection ids are already
//! machine-scoped in `checkouts.json`.
//!
//! Two older layouts are migrated into it, each once per store identity:
//!
//! - HS2-1M8E9D kept one copy per store under `${HOTSHEET_HOME}/stores/<instance id>/`.
//!   Every such file is merged, in instance-id order, on the first resolve that finds it.
//! - Before that, the file was `<store root>/providers.json` and sync committed it. A store
//!   without a per-store copy is merged from that working file, else from the last version
//!   in its Git history, when that store is first resolved.
//!
//! The ledger `${HOTSHEET_HOME}/providers.migration.json` records which store identities
//! are merged and every warning. Merging is by connection id and never replaces an entry
//! the registry already holds: an identical record is skipped, a different record with the
//! same id is a conflict whose dropped record is kept verbatim in the ledger, and a second
//! default connection loses its default flag. Source files are never changed or removed,
//! so a rollback still finds them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{ProviderConfigRegistry, ProviderConnection, ProviderConnectionsFile, ProviderError};
use crate::{FsStore, StoreError};

/// The file name of the provider configuration: the machine registry in the home, a legacy
/// per-store copy, and the legacy store-root file.
pub const PROVIDER_CONFIG_FILE: &str = "providers.json";

/// The machine-home directory that held HS2-1M8E9D's per-store provider copies.
pub const MACHINE_STORE_CONFIG_DIR: &str = "stores";

/// The machine-home ledger of merged store identities and migration warnings.
pub const PROVIDER_MIGRATION_FILE: &str = "providers.migration.json";

const LEDGER_VERSION: u32 = 1;

/// Where a store's provider configuration came from when it was merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConfigSeed {
    /// HS2-1M8E9D's `${HOTSHEET_HOME}/stores/<instance id>/providers.json`.
    MachineStoreFile,
    /// The store-root `providers.json` working file.
    StoreFile,
    /// The last `providers.json` committed to the store's Git history, for a machine whose
    /// working file an older client lost when it pulled the untrack.
    StoreHistory,
    /// The store never had provider configuration.
    Empty,
}

/// Why a migration warning was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMigrationWarningKind {
    /// The registry already holds a different record with this id. It is kept, and the
    /// store's record is preserved in [`ProviderMigrationWarning::dropped`].
    Conflict,
    /// The registry already has a default connection, so this one was merged without its
    /// default flag.
    DefaultCleared,
    /// The source could not be read or parsed. Nothing from it was merged; the next resolve
    /// tries it again.
    InvalidSource,
}

/// One recorded migration warning. Identifiers and paths only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderMigrationWarning {
    /// The store identity whose configuration was being merged.
    pub store: String,
    pub kind: ProviderMigrationWarningKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    pub message: String,
    /// The record that was not merged, kept so no entry is ever lost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<ProviderConnection>,
}

impl std::fmt::Display for ProviderMigrationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.connection_id {
            Some(id) => write!(f, "store {} connection {id}: {}", self.store, self.message),
            None => write!(f, "store {}: {}", self.store, self.message),
        }
    }
}

/// One merged store identity in the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMigrationEntry {
    pub source: ProviderConfigSeed,
    /// Connections added to the registry from this store.
    pub added: usize,
}

/// The persisted migration ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderMigrationLedger {
    pub version: u32,
    #[serde(default)]
    pub stores: BTreeMap<String, ProviderMigrationEntry>,
    #[serde(default)]
    pub warnings: Vec<ProviderMigrationWarning>,
}

impl Default for ProviderMigrationLedger {
    fn default() -> Self {
        Self {
            version: LEDGER_VERSION,
            stores: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }
}

impl ProviderMigrationLedger {
    /// The ledger at `home`; an absent file is an empty ledger. A file that does not
    /// parse is an error: forgetting which stores were merged would bring back
    /// connections that were removed since.
    pub fn load(home: &Path) -> Result<Self, ProviderError> {
        let path = provider_migration_ledger_path(home);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(StoreError::Io(error).into()),
        };
        serde_json::from_str(&text).map_err(|error| ProviderError::Conflict {
            ticket: path.display().to_string(),
            message: format!(
                "invalid provider migration ledger: {error}; repair it, or remove it to merge \
                 every per-store provider configuration again"
            ),
        })
    }
}

/// What one migration pass changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderMigrationReport {
    /// Store identities merged by this pass, in merge order.
    pub merged: Vec<(String, ProviderConfigSeed)>,
    /// Warnings this pass recorded for the first time.
    pub warnings: Vec<ProviderMigrationWarning>,
}

/// The machine's provider configuration: `${home}/providers.json`.
pub fn machine_provider_config_path(home: &Path) -> PathBuf {
    home.join(PROVIDER_CONFIG_FILE)
}

/// HS2-1M8E9D's per-store copy for the store whose identity is `store_key`. Read only by
/// the migration.
pub fn legacy_store_provider_config_path(home: &Path, store_key: &str) -> PathBuf {
    home.join(MACHINE_STORE_CONFIG_DIR)
        .join(store_key)
        .join(PROVIDER_CONFIG_FILE)
}

/// The migration ledger: `${home}/providers.migration.json`.
pub fn provider_migration_ledger_path(home: &Path) -> PathBuf {
    home.join(PROVIDER_MIGRATION_FILE)
}

/// The store's committed identity (`hotsheet-store.json` `instance_id`). A legacy store
/// without one is assigned one through the idempotent [`FsStore::ensure_instance_id`].
pub fn store_config_key(store_root: &Path) -> Result<String, StoreError> {
    let store = FsStore::open_without_maintenance(store_root)?;
    match store.metadata()?.instance_id {
        Some(id) => Ok(id),
        None => store.ensure_instance_id(),
    }
}

/// The store being resolved, whose legacy store-root file or history may seed it.
#[derive(Debug, Clone, Copy)]
pub struct ProviderMigrationStore<'a> {
    pub key: &'a str,
    pub root: &'a Path,
}

impl ProviderConfigRegistry {
    /// The machine's registry under `home`, without migrating anything.
    pub fn for_machine(home: &Path) -> Self {
        Self::new(machine_provider_config_path(home))
    }

    /// The provider configuration for the store at `store_root`, under the process's
    /// `HOTSHEET_HOME`: the machine registry, after any pending migration.
    pub fn for_store(store_root: &Path) -> Result<Self, ProviderError> {
        Self::for_store_in(&crate::machine_home::hotsheet_home(), store_root)
    }

    /// [`Self::for_store`] with an explicit machine home. Migration warnings that are new
    /// are printed to standard error; every warning stays in the ledger.
    ///
    /// A host-configuration directory that is not a store (no `hotsheet-store.json`, for
    /// example a `-C` directory that only holds provider records) keeps its file beside it,
    /// as before: Hot Sheet never syncs such a directory, so it is machine-local already.
    pub fn for_store_in(home: &Path, store_root: &Path) -> Result<Self, ProviderError> {
        let key = match store_config_key(store_root) {
            Ok(key) => key,
            Err(StoreError::NotAStore(_)) => {
                return Ok(Self::new(store_root.join(PROVIDER_CONFIG_FILE)));
            }
            Err(error) => return Err(error.into()),
        };
        let report = migrate_machine_provider_config(
            home,
            Some(ProviderMigrationStore {
                key: &key,
                root: store_root,
            }),
        )?;
        print_warnings(&report);
        Ok(Self::for_machine(home))
    }
}

fn print_warnings(report: &ProviderMigrationReport) {
    for warning in &report.warnings {
        eprintln!("warning: provider configuration migration: {warning}");
    }
}

/// Merge every pending legacy provider configuration into the machine registry under
/// `home`: each `stores/<id>/providers.json` not yet in the ledger, in id order, then
/// `current`'s store-root file or history when that store has no per-store copy and is
/// not merged yet. Idempotent; the lock-free fast path returns when nothing is pending.
pub fn migrate_machine_provider_config(
    home: &Path,
    current: Option<ProviderMigrationStore<'_>>,
) -> Result<ProviderMigrationReport, ProviderError> {
    if pending_sources(home, &ProviderMigrationLedger::load(home)?, current).is_empty() {
        return Ok(ProviderMigrationReport::default());
    }
    let registry = ProviderConfigRegistry::for_machine(home);
    let _guard = registry.lock_configuration()?;
    let mut ledger = ProviderMigrationLedger::load(home)?;
    let pending = pending_sources(home, &ledger, current);
    if pending.is_empty() {
        return Ok(ProviderMigrationReport::default());
    }
    // A registry that does not parse is reported, never overwritten.
    let mut connections = registry.load()?;
    let before = connections.clone();
    let mut report = ProviderMigrationReport::default();
    let mut warnings = Vec::new();
    for source in pending {
        match source.read() {
            Ok(Some((seed, incoming))) => {
                let added = merge_into(&mut connections, incoming, &source.key, &mut warnings);
                ledger.stores.insert(
                    source.key.clone(),
                    ProviderMigrationEntry {
                        source: seed,
                        added,
                    },
                );
                report.merged.push((source.key.clone(), seed));
            }
            Ok(None) => {
                ledger.stores.insert(
                    source.key.clone(),
                    ProviderMigrationEntry {
                        source: ProviderConfigSeed::Empty,
                        added: 0,
                    },
                );
                report
                    .merged
                    .push((source.key.clone(), ProviderConfigSeed::Empty));
            }
            Err(message) => warnings.push(ProviderMigrationWarning {
                store: source.key.clone(),
                kind: ProviderMigrationWarningKind::InvalidSource,
                connection_id: None,
                message,
                dropped: None,
            }),
        }
    }
    for warning in warnings {
        if !ledger.warnings.contains(&warning) {
            ledger.warnings.push(warning.clone());
            report.warnings.push(warning);
        }
    }
    // Registry first, ledger second: a crash between them merges the same records again,
    // which changes nothing because identical records are skipped.
    if connections != before {
        registry.publish_unlocked(&connections)?;
    }
    if !report.merged.is_empty() || !report.warnings.is_empty() {
        let text = serde_json::to_string_pretty(&ledger).expect("ledger is serializable");
        ProviderConfigRegistry::new(provider_migration_ledger_path(home))
            .publish_bytes_unlocked(format!("{text}\n").as_bytes())?;
    }
    Ok(report)
}

enum SourceKind {
    MachineStoreFile(PathBuf),
    Store(PathBuf),
}

struct PendingSource {
    key: String,
    kind: SourceKind,
}

impl PendingSource {
    /// The source's connections, `None` when the store never had any, or why it could not
    /// be used.
    fn read(&self) -> Result<Option<(ProviderConfigSeed, Vec<ProviderConnection>)>, String> {
        let (seed, path, bytes) = match &self.kind {
            SourceKind::MachineStoreFile(path) => (
                ProviderConfigSeed::MachineStoreFile,
                path.clone(),
                std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?,
            ),
            SourceKind::Store(root) => {
                let path = root.join(PROVIDER_CONFIG_FILE);
                match std::fs::read(&path) {
                    Ok(bytes) => (ProviderConfigSeed::StoreFile, path, bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        match committed_configuration(root) {
                            Some(bytes) => (ProviderConfigSeed::StoreHistory, path, bytes),
                            None => return Ok(None),
                        }
                    }
                    Err(error) => return Err(format!("{}: {error}", path.display())),
                }
            }
        };
        let file: ProviderConnectionsFile = serde_json::from_slice(&bytes).map_err(|error| {
            format!(
                "{}: invalid provider configuration: {error}",
                path.display()
            )
        })?;
        super::validate_connections(&file.connections)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(Some((seed, file.connections)))
    }
}

/// Sources not yet in the ledger, in merge order.
fn pending_sources(
    home: &Path,
    ledger: &ProviderMigrationLedger,
    current: Option<ProviderMigrationStore<'_>>,
) -> Vec<PendingSource> {
    let mut keys = std::fs::read_dir(home.join(MACHINE_STORE_CONFIG_DIR))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    keys.sort();
    let mut pending = keys
        .into_iter()
        .filter(|key| !ledger.stores.contains_key(key))
        .filter_map(|key| {
            let path = legacy_store_provider_config_path(home, &key);
            path.is_file().then_some(PendingSource {
                key,
                kind: SourceKind::MachineStoreFile(path),
            })
        })
        .collect::<Vec<_>>();
    if let Some(store) = current
        && !ledger.stores.contains_key(store.key)
        && !pending.iter().any(|source| source.key == store.key)
    {
        pending.push(PendingSource {
            key: store.key.to_string(),
            kind: SourceKind::Store(store.root.to_path_buf()),
        });
    }
    pending
}

/// Merge `incoming` into `connections` without replacing an existing entry; returns how
/// many records were added.
fn merge_into(
    connections: &mut Vec<ProviderConnection>,
    incoming: Vec<ProviderConnection>,
    store: &str,
    warnings: &mut Vec<ProviderMigrationWarning>,
) -> usize {
    let mut added = 0;
    for mut connection in incoming {
        if let Some(existing) = connections.iter().find(|item| item.id == connection.id) {
            let mut comparable = connection.clone();
            comparable.default = existing.default;
            if comparable != *existing {
                warnings.push(ProviderMigrationWarning {
                    store: store.to_string(),
                    kind: ProviderMigrationWarningKind::Conflict,
                    connection_id: Some(connection.id.clone()),
                    message: "the registry already has a different connection with this id; \
                              it was kept and this store's record is preserved in the ledger"
                        .into(),
                    dropped: Some(connection),
                });
            }
            continue;
        }
        if connection.default && connections.iter().any(|item| item.default) {
            connection.default = false;
            warnings.push(ProviderMigrationWarning {
                store: store.to_string(),
                kind: ProviderMigrationWarningKind::DefaultCleared,
                connection_id: Some(connection.id.clone()),
                message: "the machine already has a default connection; this one was merged \
                          without its default flag"
                    .into(),
                dropped: None,
            });
        }
        connections.push(connection);
        added += 1;
    }
    added
}

/// The last `providers.json` committed at `store_root`, when the store is in a Git
/// repository whose history has one. The newest commit that touched the path is either a
/// change (the file is in it) or the untrack (the file is in its parent).
fn committed_configuration(store_root: &Path) -> Option<Vec<u8>> {
    let pathspec = format!("./{PROVIDER_CONFIG_FILE}");
    let last = git_output(
        store_root,
        &["log", "-1", "--format=%H", "--", pathspec.as_str()],
    )?;
    let last = String::from_utf8_lossy(&last).trim().to_string();
    if last.is_empty() {
        return None;
    }
    [format!("{last}:{pathspec}"), format!("{last}^:{pathspec}")]
        .iter()
        .find_map(|object| git_output(store_root, &["show", object.as_str()]))
}

fn git_output(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// Best-effort merge used by sync before it removes the store-root file from the index,
/// so a machine never depends on the working file surviving a later integration. It
/// never assigns a store identity (that writes and commits the store, which sync must not
/// do while it holds the Git transaction); a store without one is merged on its first
/// ordinary resolve instead.
pub(crate) fn seed_before_untrack(store_root: &Path) {
    if !store_root.join(PROVIDER_CONFIG_FILE).is_file() {
        return;
    }
    let Ok(Some(key)) = FsStore::open_without_maintenance(store_root)
        .and_then(|store| store.metadata())
        .map(|metadata| metadata.instance_id)
    else {
        return;
    };
    let home = crate::machine_home::hotsheet_home();
    match migrate_machine_provider_config(
        &home,
        Some(ProviderMigrationStore {
            key: &key,
            root: store_root,
        }),
    ) {
        Ok(report) => print_warnings(&report),
        Err(error) => {
            eprintln!("warning: could not keep this machine's provider configuration: {error}");
        }
    }
}

#[cfg(test)]
mod tests;
