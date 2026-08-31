//! Multi-store serving (`docs/04` §4.3.1, HS2-87) — one machine server hosting **several**
//! local projects. Each served store keeps its **own** file-/memory-backed index; a stable
//! URL id (a short hash of the canonical root, matching the index db-file id) keys them so
//! a slash-laden path never lands in a URL segment.
//!
//! This is the **first increment**: the `StoreHost` registry + a stable id + the served-
//! store listing DTO, plus a store-scoped read path (`GET /stores/{id}/tickets`). The
//! per-store fs-watcher, the scoped write routes, and reconciling the machine-server
//! instance registry (HS2-59) with N hosted projects are the next increments.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use hotsheet_index::Index;
use hotsheet_model::{Ticket, Ulid};
use hotsheet_ticketing::{FsStore, StoreError, StoreRegistry};
use serde::Serialize;

/// One served store: its file store + its own index.
#[derive(Clone)]
pub struct StoreEntry {
    pub store: FsStore,
    pub index: Arc<Mutex<Index>>,
}

/// The stable, URL-safe id for a store: the first 16 hex of the canonical-root hash — the
/// same id the index uses for its db file, so a store's URL id and its index file agree.
pub fn store_url_id(store: &FsStore) -> String {
    hotsheet_ticketing::git_connection_id(store)
}

/// The store paths a machine server should auto-host at startup, read from
/// `${HOTSHEET_HOME}/stores.json` — `{ "stores": ["/path/a", "/path/b"] }` (HS2-87). A
/// missing or malformed file yields an empty list (nothing extra hosted; the primary
/// store is always served regardless).
pub fn configured_store_paths() -> Vec<std::path::PathBuf> {
    #[derive(serde::Deserialize, Default)]
    struct Config {
        #[serde(default)]
        stores: Vec<String>,
    }
    let path = hotsheet_plugins::hotsheet_home().join("stores.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Config>(&text)
        .unwrap_or_default()
        .stores
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect()
}

/// The file-backed index path for a hosted store — `${HOTSHEET_HOME}/index/<id>.sqlite`,
/// the same convention the server binary uses for the primary store, so a store's index
/// file is shared whether it's served as the primary or a registered store.
pub fn index_path_for(store: &FsStore) -> std::io::Result<std::path::PathBuf> {
    let dir = hotsheet_plugins::hotsheet_home().join("index");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{}.sqlite", store_url_id(store))))
}

/// Machine-local durable permission rules for a server's primary store. Keeping these
/// under `HOTSHEET_HOME` avoids committing personal approvals to the ticket repository;
/// keying by store identity also prevents an approval in one project from silently
/// becoming an approval in another independently served project.
pub fn permission_rules_path_for(store: &FsStore) -> std::io::Result<std::path::PathBuf> {
    permission_rules_path_in(&hotsheet_plugins::hotsheet_home(), store)
}

fn permission_rules_path_in(
    home: &std::path::Path,
    store: &FsStore,
) -> std::io::Result<std::path::PathBuf> {
    let dir = home.join("permissions");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{}.json", store_url_id(store))))
}

/// A served store as listed by `GET /stores`.
#[derive(Debug, Serialize)]
pub struct StoreInfo {
    pub id: String,
    pub root: String,
    pub prefix: String,
    pub tickets: usize,
}

impl StoreInfo {
    pub fn provider_descriptor(&self, is_default: bool) -> hotsheet_ticketing::ProviderDescriptor {
        hotsheet_ticketing::ProviderDescriptor {
            connection_id: self.id.clone(),
            provider: "git".into(),
            display_name: if self.prefix.is_empty() {
                "Git tickets".into()
            } else {
                format!("{} git tickets", self.prefix)
            },
            locator: self.root.clone(),
            default: is_default,
            capabilities: hotsheet_ticketing::ProviderCapabilities::git(),
        }
    }
}

/// The registry of stores this machine server hosts, keyed by [`store_url_id`].
#[derive(Clone, Default)]
pub struct StoreHost {
    stores: Arc<Mutex<HashMap<String, StoreEntry>>>,
}

impl StoreHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a served store, returning its URL id.
    pub fn register(&self, entry: StoreEntry) -> String {
        let id = store_url_id(&entry.store);
        if let Ok(mut map) = self.stores.lock() {
            map.insert(id.clone(), entry);
        }
        id
    }

    /// The entry for a URL id, if hosted.
    pub fn get(&self, id: &str) -> Option<StoreEntry> {
        self.stores.lock().ok()?.get(id).cloned()
    }

    /// Whether a store with this canonical root is already hosted.
    pub fn contains(&self, id: &str) -> bool {
        self.stores
            .lock()
            .map(|m| m.contains_key(id))
            .unwrap_or(false)
    }

    /// A listing of the hosted stores (id, root, prefix, ticket count), sorted by id for
    /// a deterministic response.
    pub fn list(&self) -> Vec<StoreInfo> {
        let Ok(map) = self.stores.lock() else {
            return Vec::new();
        };
        let mut out: Vec<StoreInfo> = map
            .iter()
            .map(|(id, e)| StoreInfo {
                id: id.clone(),
                root: e.store.root().display().to_string(),
                prefix: e
                    .store
                    .metadata()
                    .map(|m| m.ticket_prefix)
                    .unwrap_or_default(),
                tickets: e.store.list_tickets().map(|t| t.len()).unwrap_or(0),
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// How many stores are hosted.
    pub fn count(&self) -> usize {
        self.stores.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Resolve a ULID to its single **live** instance across every hosted store,
    /// following `moved_to_store` tombstones (`StoreRegistry`, HS2-79RXD1). Returns the
    /// hosting store's URL id + the ticket, or `None` if no hosted store has it. This is
    /// how a cross-store `blocked_by` / `duplicate_of` / mention resolves (HS2-S4H2AM).
    pub fn resolve(&self, id: &Ulid) -> Result<Option<(String, Ticket)>, StoreError> {
        let mut reg = StoreRegistry::new();
        if let Ok(map) = self.stores.lock() {
            for e in map.values() {
                reg.add(e.store.clone());
            }
        }
        match reg.resolve(id)? {
            Some((store, ticket)) => Ok(Some((store_url_id(store), ticket))),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::StoreMetadata;

    #[test]
    fn permission_rules_are_machine_local_and_store_scoped() {
        let home = tempfile::tempdir().unwrap();
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = FsStore::init(first_dir.path(), &StoreMetadata::new("ONE")).unwrap();
        let second = FsStore::init(second_dir.path(), &StoreMetadata::new("TWO")).unwrap();

        let first_path = permission_rules_path_in(home.path(), &first).unwrap();
        let second_path = permission_rules_path_in(home.path(), &second).unwrap();

        assert_eq!(
            first_path.parent(),
            Some(home.path().join("permissions").as_path())
        );
        assert_ne!(first_path, second_path);
        assert_eq!(
            first_path.extension().and_then(|value| value.to_str()),
            Some("json")
        );
        assert!(home.path().join("permissions").is_dir());
    }
}
