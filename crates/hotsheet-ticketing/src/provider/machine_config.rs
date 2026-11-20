//! Per-machine ticket-store provider configuration (HS2-1M8E9D).
//!
//! A store's external provider connections (GitHub, GitLab, Jira) hold keychain
//! references, a signed-in account, and endpoint operations that name this machine's
//! checkouts. They live in the machine home, keyed by the store's committed identity:
//! `${HOTSHEET_HOME}/stores/<instance id>/providers.json`. Each machine therefore keeps
//! its own configuration for a shared store, and one machine's orphan cleanup can never
//! remove another machine's records.
//!
//! Before this change the file was `<store root>/providers.json` and sync committed it.
//! The first resolve on a machine seeds the local copy once (see
//! [`ProviderConfigRegistry::seed_from_store`]); the store-root file then joins the
//! managed ignore list and sync removes it from the index only, so the working file stays
//! as a seed and as the snapshot an older build reads after a rollback.

use std::path::{Path, PathBuf};

use super::{ProviderConfigRegistry, ProviderError};
use crate::{FsStore, StoreError};

/// The file name of a store's provider configuration, both at the legacy store root and
/// in the machine home.
pub const PROVIDER_CONFIG_FILE: &str = "providers.json";

/// The machine-home directory that holds per-store, machine-local configuration.
pub const MACHINE_STORE_CONFIG_DIR: &str = "stores";

const EMPTY_CONFIGURATION: &[u8] = b"{\n  \"connections\": []\n}\n";

/// Where a machine's provider configuration came from when it was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderConfigSeed {
    /// The machine already had its own copy; nothing changed.
    Existing,
    /// Copied from the store-root `providers.json` working file.
    StoreFile,
    /// Restored from the last `providers.json` committed to the store's Git history,
    /// for a machine whose working file an older client lost when it pulled the untrack.
    StoreHistory,
    /// The store never had provider configuration; an empty list was written.
    Empty,
}

/// The machine-local provider configuration path for the store whose identity is
/// `store_key`.
pub fn machine_provider_config_path(home: &Path, store_key: &str) -> PathBuf {
    home.join(MACHINE_STORE_CONFIG_DIR)
        .join(store_key)
        .join(PROVIDER_CONFIG_FILE)
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

impl ProviderConfigRegistry {
    /// This machine's provider configuration for the store at `store_root`, under the
    /// process's `HOTSHEET_HOME`. Seeds the machine copy on first use.
    pub fn for_store(store_root: &Path) -> Result<Self, ProviderError> {
        Self::for_store_in(&crate::machine_home::hotsheet_home(), store_root)
    }

    /// [`Self::for_store`] with an explicit machine home.
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
        let registry = Self::new(machine_provider_config_path(home, &key));
        registry.seed_from_store(store_root)?;
        Ok(registry)
    }

    /// Seed this machine's copy once, under the configuration lock, when it does not
    /// exist yet: from the store-root working file, else from the last committed version
    /// in the store's Git history, else as an empty list. An existing copy is never
    /// replaced, so a repeated call changes nothing. The bytes are copied as they are, so
    /// a malformed legacy file reports the same error it did before.
    pub fn seed_from_store(&self, store_root: &Path) -> Result<ProviderConfigSeed, ProviderError> {
        if self.path().is_file() {
            return Ok(ProviderConfigSeed::Existing);
        }
        let _guard = self.lock_configuration()?;
        if self.path().is_file() {
            return Ok(ProviderConfigSeed::Existing);
        }
        let legacy = store_root.join(PROVIDER_CONFIG_FILE);
        let (bytes, seed) = match std::fs::read(&legacy) {
            Ok(bytes) => (bytes, ProviderConfigSeed::StoreFile),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match committed_configuration(store_root) {
                    Some(bytes) => (bytes, ProviderConfigSeed::StoreHistory),
                    None => (EMPTY_CONFIGURATION.to_vec(), ProviderConfigSeed::Empty),
                }
            }
            Err(error) => return Err(StoreError::Io(error).into()),
        };
        self.publish_bytes_unlocked(&bytes)?;
        Ok(seed)
    }
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

/// Best-effort seed used by sync before it removes the store-root file from the index,
/// so a machine never depends on the working file surviving a later integration. It
/// never assigns a store identity (that writes and commits the store, which sync must not
/// do while it holds the Git transaction); a store without one is seeded on its first
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
    let registry = ProviderConfigRegistry::new(machine_provider_config_path(&home, &key));
    if let Err(error) = registry.seed_from_store(store_root) {
        eprintln!("warning: could not keep this machine's provider configuration: {error}");
    }
}

#[cfg(test)]
mod tests;
