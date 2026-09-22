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
use std::path::PathBuf;
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
    initializing: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl StoreHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Serialize cold initialization of one source without locking unrelated stores.
    pub fn initialization_lock(&self, id: &str) -> Arc<Mutex<()>> {
        self.initializing
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .clone()
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

    /// Lightweight `(id, root)` pairs for every hosted store, read without opening or
    /// parsing any ticket files. Hot request paths that only need to map a source locator
    /// to a store id use this so they neither scan the stores nor hold the `stores` lock
    /// during disk I/O — which otherwise serialized a single-ticket detail read behind an
    /// unrelated all-stores scan (HS2-P6N7FR).
    pub fn locations(&self) -> Vec<(String, PathBuf)> {
        let Ok(map) = self.stores.lock() else {
            return Vec::new();
        };
        map.iter()
            .map(|(id, e)| (id.clone(), e.store.root().to_path_buf()))
            .collect()
    }

    /// A listing of the hosted stores (id, root, prefix, ticket count), sorted by id for
    /// a deterministic response. The per-store ticket parse runs **after** the `stores`
    /// lock is released (stores are cheap to clone) so counting tickets for `GET /stores`
    /// never blocks concurrent detail reads (HS2-P6N7FR).
    pub fn list(&self) -> Vec<StoreInfo> {
        let entries: Vec<(String, FsStore)> = {
            let Ok(map) = self.stores.lock() else {
                return Vec::new();
            };
            map.iter()
                .map(|(id, e)| (id.clone(), e.store.clone()))
                .collect()
        };
        let mut out: Vec<StoreInfo> = entries
            .into_iter()
            .map(|(id, store)| StoreInfo {
                id,
                root: store.root().display().to_string(),
                prefix: store
                    .metadata()
                    .map(|m| m.ticket_prefix)
                    .unwrap_or_default(),
                // Resilient count (HS2-PRVPCQ): a corrupt file used to zero the whole
                // store's ticket count; count the healthy tickets instead.
                tickets: store
                    .list_tickets_resilient()
                    .map(|l| l.tickets.len())
                    .unwrap_or(0),
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

    fn entry_for(store: FsStore) -> StoreEntry {
        let index = Index::open_in_memory(store.root().display().to_string()).unwrap();
        StoreEntry {
            store,
            index: Arc::new(Mutex::new(index)),
        }
    }

    // The cheap `locations()` mapping the hot request path uses must agree exactly with the
    // full `list()` — same store ids and roots — without parsing tickets (HS2-P6N7FR).
    #[test]
    fn locations_matches_list_ids_and_roots_without_parsing_tickets() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = FsStore::init(first_dir.path(), &StoreMetadata::new("ONE")).unwrap();
        let second = FsStore::init(second_dir.path(), &StoreMetadata::new("TWO")).unwrap();
        let first_id = store_url_id(&first);
        let second_id = store_url_id(&second);
        let first_root = first.root().to_path_buf();
        let second_root = second.root().to_path_buf();

        let host = StoreHost::new();
        host.register(entry_for(first));
        host.register(entry_for(second));

        // `locations()` returns every hosted store as an (id, root) pair.
        let mut locations = host.locations();
        locations.sort_by(|a, b| a.0.cmp(&b.0));
        let mut expected = vec![
            (first_id.clone(), first_root),
            (second_id.clone(), second_root),
        ];
        expected.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(locations, expected);

        // The id set (which `checkout_entries` resolves against) equals `list()`'s id set,
        // and each store maps to a real hosted entry.
        let list_ids: std::collections::BTreeSet<String> =
            host.list().into_iter().map(|info| info.id).collect();
        let location_ids: std::collections::BTreeSet<String> =
            host.locations().into_iter().map(|(id, _)| id).collect();
        assert_eq!(list_ids, location_ids);
        assert!(host.get(&first_id).is_some());
        assert!(host.get(&second_id).is_some());
    }
}
