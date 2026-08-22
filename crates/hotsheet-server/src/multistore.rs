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

use hotsheet_index::{Index, hash_bytes};
use hotsheet_ticketing::FsStore;
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
    let root = store
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.root().to_path_buf());
    hash_bytes(root.to_string_lossy().as_bytes())[..16].to_string()
}

/// A served store as listed by `GET /stores`.
#[derive(Debug, Serialize)]
pub struct StoreInfo {
    pub id: String,
    pub root: String,
    pub prefix: String,
    pub tickets: usize,
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
}
