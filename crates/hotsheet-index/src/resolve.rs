//! Index-first ticket resolution (HS2-ZXFNE6, `docs/03-indexing-and-query.md` §3.4).
//!
//! A slug lookup asks the index for the candidate ULIDs, reads only those files, and
//! verifies that each still carries the slug. The files stay the source of truth: a miss,
//! a stale entry (the ticket was renamed, moved, or deleted after it was indexed), or an
//! index that cannot answer falls back to [`ops::resolve`]'s store walk, so the result is
//! always what that walk would return.

use hotsheet_model::{Ticket, Ulid};
use hotsheet_ticketing::{FsStore, StoreError, ops};

use crate::IndexError;

/// Resolve `needle` (an exact ULID or an ASCII case-insensitive slug) in `store`.
///
/// `candidates` returns the indexed ULIDs whose slug matches, lowest first; it is a
/// closure so the caller can hold its index lock only for that query and never across
/// file reads. ULIDs go straight to their file, as in [`ops::resolve`].
pub fn resolve_ticket(
    store: &FsStore,
    needle: &str,
    candidates: impl FnOnce(&str) -> Result<Vec<Ulid>, IndexError>,
) -> Result<Option<Ticket>, StoreError> {
    if Ulid::from_string(needle).is_ok() {
        return ops::resolve(store, needle);
    }
    // The lowest indexed candidate is the walk's answer whenever its file still carries
    // the slug. Otherwise it was renamed, moved, deleted, or became unreadable after it
    // was indexed, and the walk decides exactly as it would without an index.
    if let Some(id) = candidates(needle).ok().and_then(|ids| ids.first().copied())
        && let Ok(ticket) = store.read_ticket(&id)
        && ticket.slug.eq_ignore_ascii_case(needle)
    {
        return Ok(Some(ticket));
    }
    ops::resolve(store, needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Index;
    use hotsheet_model::Timestamp;
    use hotsheet_ticketing::StoreMetadata;
    use std::cell::Cell;

    fn store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn create(store: &FsStore, title: &str) -> Ticket {
        ops::create(
            store,
            Ulid::new(),
            "HS",
            Timestamp::new("2026-10-15T00:00:00Z"),
            ops::NewTicket {
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn indexed(store: &FsStore) -> Index {
        let index = Index::open_in_memory(store.root().display().to_string()).unwrap();
        index.rebuild_from_store(store).unwrap();
        index
    }

    #[test]
    fn an_indexed_slug_reads_only_its_candidate_file() {
        let (_dir, store) = store();
        let wanted = create(&store, "wanted");
        create(&store, "other");
        let index = indexed(&store);
        // An unparseable file declaring the same slug fails the store walk, so a success
        // proves the fast path read only the indexed candidate.
        let decoy = store.ticket_path(&Ulid::new());
        std::fs::create_dir_all(decoy.parent().unwrap()).unwrap();
        std::fs::write(&decoy, format!("---\nslug: {}\n---\nbroken", wanted.slug)).unwrap();
        assert!(ops::resolve(&store, &wanted.slug).is_err());
        let found = resolve_ticket(&store, &wanted.slug.to_lowercase(), |slug| {
            index.slug_candidates(slug)
        })
        .unwrap()
        .unwrap();
        assert_eq!(found.id, wanted.id);
        assert_eq!(
            index.slug_candidates(&wanted.slug).unwrap(),
            vec![wanted.id]
        );
    }

    #[test]
    fn an_index_miss_falls_back_to_the_store_walk() {
        let (_dir, store) = store();
        let index = indexed(&store);
        // Written after the index was built, as an external CLI write would be.
        let late = create(&store, "late");
        let found = resolve_ticket(&store, &late.slug, |slug| index.slug_candidates(slug))
            .unwrap()
            .unwrap();
        assert_eq!(found.id, late.id);
        assert!(
            resolve_ticket(&store, "HS-ZZZZZZ", |slug| index.slug_candidates(slug))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn an_unavailable_index_falls_back_to_the_store_walk() {
        let (_dir, store) = store();
        let ticket = create(&store, "no index");
        let found = resolve_ticket(&store, &ticket.slug, |_| {
            Err(IndexError::Store("index unavailable".into()))
        })
        .unwrap()
        .unwrap();
        assert_eq!(found.id, ticket.id);
    }

    #[test]
    fn a_stale_entry_is_verified_against_its_file() {
        let (_dir, store) = store();
        let renamed = create(&store, "renamed");
        let index = indexed(&store);
        let old_slug = renamed.slug.clone();

        // The indexed ticket no longer carries the slug, and another ticket now does.
        let mut moved_away = store.read_ticket(&renamed.id).unwrap();
        moved_away.slug = "HS-MOVED1".into();
        store.write_ticket(&moved_away).unwrap();
        let mut taker = create(&store, "taker");
        taker.slug = old_slug.clone();
        store.write_ticket(&taker).unwrap();

        let found = resolve_ticket(&store, &old_slug, |slug| index.slug_candidates(slug))
            .unwrap()
            .unwrap();
        assert_eq!(found.id, taker.id, "stale index row must not win");
        // The renamed ticket resolves by its new slug through the fallback walk.
        let by_new = resolve_ticket(&store, "hs-moved1", |slug| index.slug_candidates(slug))
            .unwrap()
            .unwrap();
        assert_eq!(by_new.id, renamed.id);
    }

    #[test]
    fn a_deleted_indexed_file_resolves_to_nothing() {
        let (_dir, store) = store();
        let gone = create(&store, "gone");
        let index = indexed(&store);
        std::fs::remove_file(store.ticket_path(&gone.id)).unwrap();
        assert!(
            resolve_ticket(&store, &gone.slug, |slug| index.slug_candidates(slug))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_moved_tombstone_still_resolves_by_its_slug() {
        let (dir, store) = store();
        let ticket = create(&store, "to move");
        let dest_dir = tempfile::tempdir().unwrap();
        let dest = FsStore::init(dest_dir.path(), &StoreMetadata::new("OT")).unwrap();
        let moved = ops::move_ticket(
            &store,
            &dest,
            &ticket.id,
            "OT",
            Timestamp::new("2026-10-15T00:01:00Z"),
        )
        .unwrap();
        let index = indexed(&store);
        let via_index = resolve_ticket(&store, &ticket.slug, |slug| index.slug_candidates(slug))
            .unwrap()
            .unwrap();
        let via_walk = ops::resolve(&store, &ticket.slug).unwrap().unwrap();
        assert_eq!(via_index.id, moved.tombstone.id);
        assert_eq!(via_index.status, via_walk.status);
        drop(dir);
    }

    #[test]
    fn a_ulid_never_consults_the_index() {
        let (_dir, store) = store();
        let ticket = create(&store, "by id");
        let asked = Cell::new(false);
        let found = resolve_ticket(&store, &ticket.id.to_string(), |_| {
            asked.set(true);
            Ok(Vec::new())
        })
        .unwrap()
        .unwrap();
        assert_eq!(found.id, ticket.id);
        assert!(!asked.get());
    }
}
