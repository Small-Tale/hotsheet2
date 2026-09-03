//! Multi-store resolution (`docs/02` §2.2.1, §2.13; `docs/03` §3.5, HS2-4). A ULID is
//! globally unique, so a ticket referenced from one store — a `blocked_by`, a
//! `duplicate_of`, an `@mention` — may actually live in **another** store. A
//! [`StoreRegistry`] holds several open [`FsStore`]s and resolves a ULID to the single
//! **live** instance across them, following a `moved` tombstone's `moved_to_store`
//! redirect to wherever the ticket now lives (HS2-60 keeps the same ULID on a move and
//! leaves a tombstone behind, so the id exists in both the source and the destination).
//!
//! This is the store-layer foundation the multi-store server (HS2-87) and cross-store
//! copy/move surfaces (HS2-60 follow-up) build on. It is **pure** over the injected
//! stores — no globals, no discovery magic; the caller decides which stores are in play.

use std::path::Path;

use hotsheet_model::{Status, Ticket, Ulid};

use crate::store::{FsStore, StoreError};

/// A set of open stores a ULID can be resolved across. Deduplicates by canonical root.
#[derive(Default)]
pub struct StoreRegistry {
    stores: Vec<FsStore>,
}

impl StoreRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// The canonical identity of a store: its canonicalized root path. Matches the
    /// index's `store_id` derivation, and what `move_ticket` records in `moved_to_store`.
    pub fn store_id(store: &FsStore) -> String {
        store
            .root()
            .canonicalize()
            .unwrap_or_else(|_| store.root().to_path_buf())
            .display()
            .to_string()
    }

    /// Add an already-open store (idempotent — a store with the same canonical root is
    /// not added twice).
    pub fn add(&mut self, store: FsStore) {
        let id = Self::store_id(&store);
        if !self.stores.iter().any(|s| Self::store_id(s) == id) {
            self.stores.push(store);
        }
    }

    /// Open a store at `root` and add it.
    pub fn open(&mut self, root: impl AsRef<Path>) -> Result<(), StoreError> {
        let store = FsStore::open(root.as_ref())?;
        self.add(store);
        Ok(())
    }

    /// The registered stores.
    pub fn stores(&self) -> &[FsStore] {
        &self.stores
    }

    /// Find the store matching `id_or_path` — either its canonical `store_id` or its raw
    /// (uncanonicalized) root path, since `moved_to_store` may record either.
    fn store_by_ref(&self, id_or_path: &str) -> Option<&FsStore> {
        self.stores.iter().find(|s| {
            Self::store_id(s) == id_or_path || s.root().display().to_string() == id_or_path
        })
    }

    /// Resolve a ULID to its single **live** instance across all registered stores. A
    /// `moved` tombstone is followed to its `moved_to_store` destination; if that store
    /// is registered and holds a live copy, that copy wins. Returns `Ok(None)` when no
    /// registered store has the id (or only unresolvable tombstones remain).
    pub fn resolve(&self, id: &Ulid) -> Result<Option<(&FsStore, Ticket)>, StoreError> {
        for store in &self.stores {
            let Some(t) = read_opt(store, id)? else {
                continue;
            };
            // A live (non-tombstone) hit is the answer.
            if t.status != Status::Moved {
                return Ok(Some((store, t)));
            }
            // A tombstone: follow the redirect to the live instance if we can.
            if let Some(dest) = t.moved_to_store.as_deref() {
                if let Some(dest_store) = self.store_by_ref(dest) {
                    if let Some(live) = read_opt(dest_store, id)? {
                        if live.status != Status::Moved {
                            return Ok(Some((dest_store, live)));
                        }
                    }
                }
            }
            // Tombstone with an unregistered/also-moved destination — keep scanning; the
            // live copy may be in a later store.
        }
        Ok(None)
    }
}

/// Read a ticket, mapping a missing file to `None` (it just isn't in this store).
fn read_opt(store: &FsStore, id: &Ulid) -> Result<Option<Ticket>, StoreError> {
    match store.read_ticket(id) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.is_io_kind(std::io::ErrorKind::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{NewTicket, create};
    use hotsheet_model::Timestamp;

    fn ts(s: &str) -> Timestamp {
        Timestamp::new(format!("2026-08-22T00:00:0{s}Z"))
    }

    fn store(prefix: &str) -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &crate::store::StoreMetadata::new(prefix)).unwrap();
        (dir, store)
    }

    #[test]
    fn resolves_within_and_across_stores_and_reports_none() {
        let (_da, a) = store("AA");
        let (_db, b) = store("BB");

        let in_a = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5AAA").unwrap();
        let in_b = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5BBB").unwrap();
        create(&a, in_a, "AA", ts("0"), NewTicket::default()).unwrap();
        create(&b, in_b, "BB", ts("0"), NewTicket::default()).unwrap();

        let mut reg = StoreRegistry::new();
        reg.add(a);
        reg.add(b);

        let (s, t) = reg.resolve(&in_b).unwrap().expect("id in B resolves");
        assert_eq!(t.id, in_b);
        assert_eq!(
            StoreRegistry::store_id(s),
            StoreRegistry::store_id(&reg.stores()[1])
        );

        assert!(
            reg.resolve(&in_a).unwrap().is_some(),
            "id in A resolves too"
        );
        let unknown = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5ZZZ").unwrap();
        assert!(
            reg.resolve(&unknown).unwrap().is_none(),
            "unknown id → None"
        );
    }

    #[test]
    fn a_move_tombstone_resolves_to_the_live_destination() {
        let (_da, a) = store("AA");
        let (_db, b) = store("BB");
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FMV").unwrap();

        // Live copy in B (same ULID), tombstone in A pointing at B — the shape
        // `move_ticket` leaves behind (HS2-60).
        create(&b, id, "BB", ts("1"), NewTicket::default()).unwrap();
        let mut tomb = create(&a, id, "AA", ts("0"), NewTicket::default()).unwrap();
        tomb.status = Status::Moved;
        tomb.moved_to_store = Some(StoreRegistry::store_id(&b));
        a.write_ticket(&tomb).unwrap();

        // Registered A-first: the tombstone must be followed to B's live instance.
        let mut reg = StoreRegistry::new();
        reg.add(a);
        reg.add(b);

        let (s, t) = reg
            .resolve(&id)
            .unwrap()
            .expect("resolves through the tombstone");
        assert_eq!(
            t.status,
            Status::NotStarted,
            "the live instance, not the tombstone"
        );
        assert_eq!(
            StoreRegistry::store_id(s),
            StoreRegistry::store_id(&reg.stores()[1]),
            "resolved to the destination store"
        );
    }

    #[test]
    fn add_is_idempotent_by_canonical_root() {
        let (dir, a) = store("AA");
        let a2 = FsStore::open(dir.path()).unwrap();
        let mut reg = StoreRegistry::new();
        reg.add(a);
        reg.add(a2);
        assert_eq!(reg.stores().len(), 1, "same root added once");
    }
}
