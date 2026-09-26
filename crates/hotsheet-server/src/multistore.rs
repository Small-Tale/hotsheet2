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
    /// Stat-validated memo of the store's corrupt ticket files (HS2-KYSBT2).
    pub corrupt: Arc<Mutex<hotsheet_ticketing::CorruptTicketCache>>,
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
    /// Count a store's healthy tickets. This parses **every** ticket file, so a caller on
    /// an async request path must run it off the runtime (`spawn_blocking`).
    fn counted(id: String, store: &FsStore) -> Self {
        let HostedStore { id, root, prefix } = HostedStore::of(id, store);
        Self {
            id,
            root,
            prefix,
            // Resilient count (HS2-PRVPCQ): a corrupt file used to zero the whole
            // store's ticket count; count the healthy tickets instead.
            tickets: store
                .list_tickets_resilient()
                .map(|l| l.tickets.len())
                .unwrap_or(0),
        }
    }
}

/// A hosted store's identity and metadata, read **without parsing any ticket file**. This
/// is everything `GET /providers` needs: one small metadata read per store instead of a
/// parse of every ticket (HS2-4XXRJP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedStore {
    pub id: String,
    pub root: String,
    pub prefix: String,
}

impl HostedStore {
    fn of(id: String, store: &FsStore) -> Self {
        Self {
            id,
            root: store.root().display().to_string(),
            prefix: store
                .metadata()
                .map(|m| m.ticket_prefix)
                .unwrap_or_default(),
        }
    }

    /// The built-in git provider descriptor for this store.
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

    /// `(id, store)` clones sorted by id. Stores are cheap to clone, so later disk I/O
    /// never holds the `stores` lock (HS2-P6N7FR).
    fn snapshot(&self) -> Vec<(String, FsStore)> {
        let Ok(map) = self.stores.lock() else {
            return Vec::new();
        };
        let mut entries: Vec<(String, FsStore)> = map
            .iter()
            .map(|(id, e)| (id.clone(), e.store.clone()))
            .collect();
        drop(map);
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    /// Every hosted store's id, root, and prefix, sorted by id, with **no ticket parsing**.
    /// `GET /providers` builds its descriptors from this, so its cost scales with the
    /// number of stores rather than tickets and it cannot stall the request threads
    /// (HS2-4XXRJP).
    pub fn summaries(&self) -> Vec<HostedStore> {
        self.snapshot()
            .into_iter()
            .map(|(id, store)| HostedStore::of(id, &store))
            .collect()
    }

    /// A listing of the hosted stores (id, root, prefix, ticket count), sorted by id for
    /// a deterministic response. This parses every ticket in every store, so it exists
    /// only for `GET /stores`, which runs it on a blocking thread (HS2-4XXRJP). The parse
    /// runs **after** the `stores` lock is released so it never blocks concurrent detail
    /// reads (HS2-P6N7FR).
    pub fn list(&self) -> Vec<StoreInfo> {
        self.snapshot()
            .into_iter()
            .map(|(id, store)| StoreInfo::counted(id, &store))
            .collect()
    }

    /// One hosted store's listing entry with its ticket count, parsing only that store.
    pub fn info(&self, id: &str) -> Option<StoreInfo> {
        let store = self.get(id)?.store;
        Some(StoreInfo::counted(id.to_string(), &store))
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
            corrupt: Arc::default(),
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

    /// Plant a named pipe where a ticket file belongs. Opening it for reading blocks until
    /// a writer appears, so any code path that tries to parse it hangs — a deterministic
    /// "was a ticket file touched?" probe that no timing threshold can flake on.
    #[cfg(unix)]
    fn plant_ticket_fifo(store: &FsStore) -> PathBuf {
        use std::os::unix::ffi::OsStrExt;
        let path = store.ticket_path(&Ulid::new());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c_path` is a valid NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        path
    }

    fn create_ticket(store: &FsStore, prefix: &str) {
        hotsheet_ticketing::ops::create(
            store,
            Ulid::new(),
            prefix,
            hotsheet_model::Timestamp::new("2026-09-26T00:00:00Z"),
            hotsheet_ticketing::NewTicket {
                title: "counted".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
    }

    // `summaries()` (what `GET /providers` uses) must never open a ticket file: with a
    // pipe planted in the ticket tree it still answers, while the counting `list()` would
    // block on it (HS2-4XXRJP).
    #[cfg(unix)]
    #[test]
    fn summaries_never_touch_ticket_files() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = FsStore::init(first_dir.path(), &StoreMetadata::new("ONE")).unwrap();
        let second = FsStore::init(second_dir.path(), &StoreMetadata::new("TWO")).unwrap();
        create_ticket(&first, "ONE");
        plant_ticket_fifo(&second);
        let host = StoreHost::new();
        let first_id = host.register(entry_for(first.clone()));
        let second_id = host.register(entry_for(second.clone()));

        let (tx, rx) = std::sync::mpsc::channel();
        let probe = host.clone();
        std::thread::spawn(move || tx.send(probe.summaries()).unwrap());
        let summaries = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("summaries() opened a ticket file and blocked on the planted pipe");

        let mut expected = vec![
            HostedStore {
                id: first_id.clone(),
                root: first.root().display().to_string(),
                prefix: "ONE".into(),
            },
            HostedStore {
                id: second_id,
                root: second.root().display().to_string(),
                prefix: "TWO".into(),
            },
        ];
        expected.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(summaries, expected);
        let descriptor = summaries
            .iter()
            .find(|s| s.id == first_id)
            .unwrap()
            .provider_descriptor(true);
        assert_eq!(descriptor.display_name, "ONE git tickets");
        assert_eq!(descriptor.locator, first.root().display().to_string());
        assert!(descriptor.default);
    }

    // The counting paths still count: `list()` covers every store, `info()` only one.
    #[test]
    fn list_and_info_count_healthy_tickets() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = FsStore::init(first_dir.path(), &StoreMetadata::new("ONE")).unwrap();
        let second = FsStore::init(second_dir.path(), &StoreMetadata::new("")).unwrap();
        create_ticket(&first, "ONE");
        create_ticket(&first, "ONE");
        let host = StoreHost::new();
        let first_id = host.register(entry_for(first));
        let second_id = host.register(entry_for(second));

        let listed = host.list();
        assert_eq!(listed.len(), 2);
        assert!(listed.windows(2).all(|pair| pair[0].id < pair[1].id));
        let first_info = host.info(&first_id).unwrap();
        assert_eq!((first_info.prefix.as_str(), first_info.tickets), ("ONE", 2));
        assert_eq!(host.info(&second_id).unwrap().tickets, 0);
        assert!(host.info("missing").is_none());
        let unnamed = host
            .summaries()
            .into_iter()
            .find(|s| s.id == second_id)
            .unwrap();
        assert_eq!(
            unnamed.provider_descriptor(false).display_name,
            "Git tickets"
        );
    }
}
