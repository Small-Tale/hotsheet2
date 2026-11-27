//! Index-first ticket resolution (HS2-ZXFNE6, `docs/03-indexing-and-query.md` §3.4).
//!
//! A slug lookup asks the index for the candidate ULIDs, reads only those files, and
//! verifies that each still carries the slug. The files stay the source of truth: a miss,
//! a stale entry (the ticket was renamed, moved, or deleted after it was indexed), or an
//! index that cannot answer falls back to [`ops::resolve`]'s store walk, so the result is
//! always what that walk would return.
//!
//! A **miss** is the exception (HS2-3R0C6G): while the store's watcher reports the index
//! in sync with the files ([`IndexFreshness::in_sync`]), an index miss is the answer and
//! the walk is skipped. Any pending reconcile — a watcher that is not live yet, a queued
//! watcher event, or a failed index write awaiting repair — sends a miss back to the walk.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use hotsheet_model::{Ticket, Ulid};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{FsStore, StoreError, ops};

use crate::IndexError;

/// Whether a store's index currently reflects every ticket file, as reported by the
/// store's watcher and index writers (HS2-3R0C6G). It starts **out of sync**, so a store
/// nobody watches always falls back to the walk on a miss.
///
/// In sync means all three hold:
/// - the watcher is **live**: its stream is running and the resync that covers writes made
///   while it started has been applied (cleared on a watcher error or rescan request, and
///   when the watcher stops);
/// - **no watcher event is queued**: every event the stream delivered has been applied;
/// - **no index repair is pending**: no failed index write is waiting to be retried.
///
/// The one window it cannot see is the few milliseconds between an external write and
/// the stream delivering its event; a lookup racing an external create can miss it,
/// exactly as one issued a moment earlier would.
///
/// A stream can also deliver late: under an `fseventsd` backlog a macOS stream that has
/// proven itself can report a write minutes after it landed (HS2-P702Y8). A watcher that
/// can prove its stream current registers a **verifier**. An in-sync miss is then final
/// only within [`VERIFIED_WINDOW`] of the last verification; otherwise the lookup asks for
/// one, waits up to [`MISS_VERIFY_WAIT`], and walks if it has not landed. The watcher
/// answers with a probe through its stream or, when that probe is overdue, by falling
/// back to its polling bridge and a full reconcile.
#[derive(Default)]
pub struct IndexFreshness {
    live: AtomicBool,
    queued: AtomicUsize,
    repairs_pending: AtomicBool,
    verification: Mutex<Verification>,
    verified: Condvar,
}

/// Asks a store's watcher to prove its stream current; it must not block (HS2-P702Y8).
pub type FreshnessVerifier = Arc<dyn Fn() + Send + Sync>;

/// How long after a verification an in-sync miss is final without another (HS2-P702Y8).
pub const VERIFIED_WINDOW: Duration = Duration::from_secs(1);
/// How long a miss outside [`VERIFIED_WINDOW`] waits for a verification before walking.
pub const MISS_VERIFY_WAIT: Duration = Duration::from_millis(250);

#[derive(Default)]
struct Verification {
    verifier: Option<FreshnessVerifier>,
    /// A request reached the verifier and has not been answered yet.
    requested: bool,
    /// Bumped by every verification, so a waiter sees one that lands after its request.
    epoch: u64,
    verified_at: Option<Instant>,
}

impl std::fmt::Debug for IndexFreshness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let verification = self.verification.lock_or_recover();
        f.debug_struct("IndexFreshness")
            .field("live", &self.live)
            .field("queued", &self.queued)
            .field("repairs_pending", &self.repairs_pending)
            .field("verifier", &verification.verifier.is_some())
            .field("requested", &verification.requested)
            .field("epoch", &verification.epoch)
            .finish()
    }
}

impl IndexFreshness {
    /// Whether an index miss is authoritative right now.
    pub fn in_sync(&self) -> bool {
        self.live.load(Ordering::SeqCst)
            && self.queued.load(Ordering::SeqCst) == 0
            && !self.repairs_pending.load(Ordering::SeqCst)
    }

    /// The watcher became live (its startup resync was applied) or stopped / lost events.
    pub fn set_live(&self, live: bool) {
        self.live.store(live, Ordering::SeqCst);
    }

    /// A watcher event was queued for the watch loop. Call before sending it.
    pub fn event_queued(&self) {
        self.queued.fetch_add(1, Ordering::SeqCst);
    }

    /// The watch loop applied `count` queued events to the index.
    pub fn events_applied(&self, count: usize) {
        let mut queued = self.queued.load(Ordering::SeqCst);
        loop {
            match self.queued.compare_exchange_weak(
                queued,
                queued.saturating_sub(count),
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => queued = actual,
            }
        }
    }

    /// Whether a failed index write is queued for repair.
    pub fn set_repairs_pending(&self, pending: bool) {
        self.repairs_pending.store(pending, Ordering::SeqCst);
    }

    /// Registers the watcher that can prove its stream current (HS2-P702Y8), replacing
    /// any earlier one. An outstanding request is handed to the new verifier.
    pub fn set_verifier(&self, verifier: FreshnessVerifier) {
        let outstanding = {
            let mut verification = self.verification.lock_or_recover();
            verification.verifier = Some(verifier.clone());
            verification.requested
        };
        if outstanding {
            verifier();
        }
    }

    /// Unregisters `verifier` if it is still the registered one (its watcher stopped).
    /// Misses then follow [`IndexFreshness::in_sync`] alone, as before any verifier.
    pub fn clear_verifier(&self, verifier: &FreshnessVerifier) {
        let mut verification = self.verification.lock_or_recover();
        if verification
            .verifier
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, verifier))
        {
            verification.verifier = None;
            verification.requested = false;
        }
        self.verified.notify_all();
    }

    /// Asks the registered verifier to prove the stream current, unless a request is
    /// already outstanding. Never blocks; without a verifier it does nothing.
    pub fn request_verification(&self) {
        let verifier = {
            let mut verification = self.verification.lock_or_recover();
            if verification.requested {
                return;
            }
            let Some(verifier) = verification.verifier.clone() else {
                return;
            };
            verification.requested = true;
            verifier
        };
        verifier();
    }

    /// [`IndexFreshness::request_verification`] unless the stream was verified within
    /// [`VERIFIED_WINDOW`]: a cheap hint from a writer that expects its own echo.
    pub fn request_verification_if_stale(&self) {
        if !self.verified_within(VERIFIED_WINDOW) {
            self.request_verification();
        }
    }

    /// The watcher applied a verification: every write made before the request is in the
    /// index, or a full reconcile ran. Wakes waiting lookups.
    pub fn mark_verified(&self) {
        let mut verification = self.verification.lock_or_recover();
        verification.requested = false;
        verification.epoch = verification.epoch.wrapping_add(1);
        verification.verified_at = Some(Instant::now());
        self.verified.notify_all();
    }

    /// Whether a verifier is registered.
    pub fn has_verifier(&self) -> bool {
        self.verification.lock_or_recover().verifier.is_some()
    }

    /// Whether no verifier is registered or the last verification landed within `window`.
    pub fn verified_within(&self, window: Duration) -> bool {
        let verification = self.verification.lock_or_recover();
        verification.verifier.is_none()
            || verification
                .verified_at
                .is_some_and(|at| at.elapsed() <= window)
    }

    /// Requests a verification and waits up to `wait` for one to land after this call.
    /// `true` when it did (or no verifier is registered), `false` on timeout.
    pub fn verify(&self, wait: Duration) -> bool {
        let start = {
            let verification = self.verification.lock_or_recover();
            if verification.verifier.is_none() {
                return true;
            }
            verification.epoch
        };
        self.request_verification();
        let verification = self.verification.lock_or_recover();
        let (verification, _) = self
            .verified
            .wait_timeout_while(verification, wait, |verification| {
                verification.epoch == start && verification.verifier.is_some()
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        verification.epoch != start || verification.verifier.is_none()
    }
}

/// Resolve `needle` (an exact ULID or an ASCII case-insensitive slug) in `store`.
///
/// `candidates` returns the indexed ULIDs whose slug matches, lowest first; it is a
/// closure so the caller can hold its index lock only for that query and never across
/// file reads. ULIDs go straight to their file, as in [`ops::resolve`]. A miss always
/// walks the store; [`resolve_ticket_with_freshness`] skips that walk while in sync.
pub fn resolve_ticket(
    store: &FsStore,
    needle: &str,
    candidates: impl FnMut(&str) -> Result<Vec<Ulid>, IndexError>,
) -> Result<Option<Ticket>, StoreError> {
    resolve(store, needle, candidates, None)
}

/// [`resolve_ticket`], but an index miss is final while `freshness` reports the index in
/// sync both before and after the candidate query (an event queued in between sends the
/// lookup back to the walk). A stale candidate still falls back to the walk.
pub fn resolve_ticket_with_freshness(
    store: &FsStore,
    needle: &str,
    candidates: impl FnMut(&str) -> Result<Vec<Ulid>, IndexError>,
    freshness: &IndexFreshness,
) -> Result<Option<Ticket>, StoreError> {
    resolve(store, needle, candidates, Some(freshness))
}

fn resolve(
    store: &FsStore,
    needle: &str,
    mut candidates: impl FnMut(&str) -> Result<Vec<Ulid>, IndexError>,
    freshness: Option<&IndexFreshness>,
) -> Result<Option<Ticket>, StoreError> {
    if Ulid::from_string(needle).is_ok() {
        return ops::resolve(store, needle);
    }
    match query(store, needle, &mut candidates, freshness) {
        Queried::Found(ticket) => return Ok(Some(*ticket)),
        Queried::Missed => {}
        Queried::Unsettled => return ops::resolve(store, needle),
    }
    let Some(freshness) = freshness else {
        return ops::resolve(store, needle);
    };
    if freshness.verified_within(VERIFIED_WINDOW) {
        return Ok(None);
    }
    // HS2-P702Y8: the stream may lag. Once a verification lands, every write made before
    // it is in the index, so asking again settles the miss; otherwise walk.
    if freshness.verify(MISS_VERIFY_WAIT) {
        match query(store, needle, &mut candidates, Some(freshness)) {
            Queried::Found(ticket) => return Ok(Some(*ticket)),
            Queried::Missed => return Ok(None),
            Queried::Unsettled => {}
        }
    }
    ops::resolve(store, needle)
}

/// One candidate query's verdict.
enum Queried {
    /// The lowest candidate's file still carries the slug.
    Found(Box<Ticket>),
    /// No candidate, and the index stayed in sync across the query.
    Missed,
    /// A stale candidate, an index error, or a miss the freshness cannot vouch for.
    Unsettled,
}

fn query(
    store: &FsStore,
    needle: &str,
    candidates: &mut impl FnMut(&str) -> Result<Vec<Ulid>, IndexError>,
    freshness: Option<&IndexFreshness>,
) -> Queried {
    // Sampled before the query: an event applied between the query and a later check
    // could otherwise vouch for a candidate list read before that event landed.
    let in_sync_before = freshness.is_some_and(IndexFreshness::in_sync);
    let candidates = candidates(needle);
    // The lowest indexed candidate is the walk's answer whenever its file still carries
    // the slug. Otherwise it was renamed, moved, deleted, or became unreadable after it
    // was indexed, and the walk decides exactly as it would without an index.
    match candidates.as_deref() {
        Ok([id, ..]) => {
            if let Ok(ticket) = store.read_ticket(id)
                && ticket.slug.eq_ignore_ascii_case(needle)
            {
                return Queried::Found(Box::new(ticket));
            }
            Queried::Unsettled
        }
        Ok([]) if in_sync_before && freshness.is_some_and(IndexFreshness::in_sync) => {
            Queried::Missed
        }
        _ => Queried::Unsettled,
    }
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

    fn live() -> IndexFreshness {
        let freshness = IndexFreshness::default();
        freshness.set_live(true);
        freshness
    }

    /// Whether `slug` resolves under `freshness`; a ticket written after the index was
    /// built is found only when the miss falls back to the walk.
    fn found_unindexed(
        store: &FsStore,
        index: &Index,
        slug: &str,
        freshness: &IndexFreshness,
    ) -> bool {
        resolve_ticket_with_freshness(store, slug, |s| index.slug_candidates(s), freshness)
            .unwrap()
            .is_some()
    }

    #[test]
    fn a_new_freshness_is_out_of_sync_so_a_miss_walks() {
        let (_dir, store) = store();
        let index = indexed(&store);
        let late = create(&store, "late");
        let freshness = IndexFreshness::default();
        assert!(!freshness.in_sync());
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
    }

    #[test]
    fn an_in_sync_miss_is_final_and_skips_the_walk() {
        let (_dir, store) = store();
        let index = indexed(&store);
        let freshness = live();
        assert!(freshness.in_sync());
        // An unparseable file declaring the slug fails the walk, so `Ok(None)` proves the
        // walk never ran.
        let decoy = store.ticket_path(&Ulid::new());
        std::fs::create_dir_all(decoy.parent().unwrap()).unwrap();
        std::fs::write(&decoy, "---\nslug: HS-NOPE00\n---\nbroken").unwrap();
        assert!(ops::resolve(&store, "HS-NOPE00").is_err());
        assert!(
            resolve_ticket_with_freshness(
                &store,
                "HS-NOPE00",
                |s| index.slug_candidates(s),
                &freshness
            )
            .unwrap()
            .is_none()
        );
    }

    /// Transition matrix over the three freshness inputs: every out-of-sync state walks
    /// on a miss, and returning to in sync makes the miss final again.
    #[test]
    fn every_pending_reconcile_state_sends_a_miss_back_to_the_walk() {
        let (_dir, store) = store();
        let index = indexed(&store);
        let late = create(&store, "late");
        let freshness = live();
        assert!(!found_unindexed(&store, &index, &late.slug, &freshness));

        // A queued event (the late write's own echo) → walk, until it is applied.
        freshness.event_queued();
        freshness.event_queued();
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
        freshness.events_applied(1);
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
        freshness.events_applied(1);
        assert!(!found_unindexed(&store, &index, &late.slug, &freshness));

        // A pending repair → walk, until it clears.
        freshness.set_repairs_pending(true);
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
        freshness.set_repairs_pending(false);
        assert!(!found_unindexed(&store, &index, &late.slug, &freshness));

        // The watcher stopped (or lost events) → walk, until it is live again.
        freshness.set_live(false);
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
        freshness.set_live(true);
        assert!(!found_unindexed(&store, &index, &late.slug, &freshness));

        // Overlapping causes: each must clear before the miss is final.
        freshness.set_live(false);
        freshness.event_queued();
        freshness.set_repairs_pending(true);
        freshness.set_live(true);
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
        freshness.events_applied(1);
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
        freshness.set_repairs_pending(false);
        assert!(!found_unindexed(&store, &index, &late.slug, &freshness));

        // Once indexed, the late ticket resolves through the index in sync.
        index.rebuild_from_store(&store).unwrap();
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
    }

    #[test]
    fn an_event_queued_or_applied_during_the_query_sends_the_miss_to_the_walk() {
        let (_dir, store) = store();
        let index = indexed(&store);
        let late = create(&store, "late");

        let freshness = live();
        let queued_mid_query = resolve_ticket_with_freshness(
            &store,
            &late.slug,
            |s| {
                freshness.event_queued();
                index.slug_candidates(s)
            },
            &freshness,
        )
        .unwrap();
        assert_eq!(queued_mid_query.unwrap().id, late.id);

        // Out of sync before the query, in sync after: the candidates may predate the
        // event that was just applied.
        let applied_mid_query = resolve_ticket_with_freshness(
            &store,
            &late.slug,
            |s| {
                freshness.events_applied(1);
                index.slug_candidates(s)
            },
            &freshness,
        )
        .unwrap();
        assert!(freshness.in_sync());
        assert_eq!(applied_mid_query.unwrap().id, late.id);
    }

    #[test]
    fn in_sync_still_walks_for_a_stale_candidate_or_a_failed_query() {
        let (_dir, store) = store();
        let renamed = create(&store, "renamed");
        let index = indexed(&store);
        let freshness = live();
        let old_slug = renamed.slug.clone();
        let mut moved_away = store.read_ticket(&renamed.id).unwrap();
        moved_away.slug = "HS-MOVED1".into();
        store.write_ticket(&moved_away).unwrap();
        let mut taker = create(&store, "taker");
        taker.slug = old_slug.clone();
        store.write_ticket(&taker).unwrap();
        let found = resolve_ticket_with_freshness(
            &store,
            &old_slug,
            |s| index.slug_candidates(s),
            &freshness,
        )
        .unwrap()
        .unwrap();
        assert_eq!(found.id, taker.id);
        let unavailable = resolve_ticket_with_freshness(
            &store,
            "hs-moved1",
            |_| Err(IndexError::Store("index unavailable".into())),
            &freshness,
        )
        .unwrap()
        .unwrap();
        assert_eq!(unavailable.id, renamed.id);
    }

    #[test]
    fn applying_more_events_than_were_queued_never_underflows() {
        let freshness = live();
        freshness.events_applied(3);
        assert!(freshness.in_sync());
        freshness.event_queued();
        assert!(!freshness.in_sync());
        freshness.events_applied(1);
        assert!(freshness.in_sync());
    }

    /// A verifier that counts requests and, when `answer` is set, verifies at once on its
    /// own thread, as a prompt stream's probe would.
    fn counting_verifier(
        freshness: &Arc<IndexFreshness>,
        answer: bool,
    ) -> (FreshnessVerifier, Arc<AtomicUsize>) {
        let requests = Arc::new(AtomicUsize::new(0));
        let counted = requests.clone();
        let answering = Arc::downgrade(freshness);
        let verifier: FreshnessVerifier = Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            if answer && let Some(freshness) = answering.upgrade() {
                std::thread::spawn(move || freshness.mark_verified());
            }
        });
        (verifier, requests)
    }

    /// HS2-P702Y8: the verification transitions. No verifier keeps the old trust; a
    /// registered one is asked once per outstanding request; a recent verification
    /// suppresses the stale-only hint; an outstanding request is handed to a new
    /// verifier; only the registered verifier can be cleared, which restores the old trust.
    #[test]
    fn hs2_p702y8_verification_requests_coalesce_and_expire() {
        let freshness = Arc::new(IndexFreshness::default());
        assert!(!freshness.has_verifier());
        assert!(freshness.verified_within(Duration::ZERO));
        assert!(freshness.verify(Duration::ZERO));
        freshness.request_verification();

        let (silent, requests) = counting_verifier(&freshness, false);
        freshness.set_verifier(silent.clone());
        assert!(freshness.has_verifier());
        assert!(
            !freshness.verified_within(VERIFIED_WINDOW),
            "never verified"
        );
        assert!(!freshness.verify(Duration::from_millis(1)));
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        // Outstanding: neither another request nor the stale-only hint asks again.
        freshness.request_verification();
        freshness.request_verification_if_stale();
        assert!(!freshness.verify(Duration::from_millis(1)));
        assert_eq!(requests.load(Ordering::SeqCst), 1);

        freshness.mark_verified();
        assert!(freshness.verified_within(VERIFIED_WINDOW));
        freshness.request_verification_if_stale();
        assert_eq!(requests.load(Ordering::SeqCst), 1, "verified just now");
        freshness.request_verification();
        assert_eq!(
            requests.load(Ordering::SeqCst),
            2,
            "an explicit request asks"
        );

        // Re-registration hands the outstanding request to the new verifier.
        let (answering, answered) = counting_verifier(&freshness, true);
        freshness.set_verifier(answering.clone());
        assert_eq!(answered.load(Ordering::SeqCst), 1);
        while freshness.verification.lock_or_recover().requested {
            std::thread::yield_now();
        }
        assert!(freshness.verify(Duration::from_secs(10)));
        assert_eq!(answered.load(Ordering::SeqCst), 2);

        freshness.clear_verifier(&silent);
        assert!(freshness.has_verifier(), "a replaced verifier cannot clear");
        freshness.clear_verifier(&answering);
        assert!(!freshness.has_verifier());
        assert!(freshness.verified_within(Duration::ZERO));
        assert!(freshness.verify(Duration::ZERO));
    }

    /// A lookup waiting on a verification is released when its verifier is unregistered.
    #[test]
    fn hs2_p702y8_clearing_the_verifier_releases_a_waiting_lookup() {
        let freshness = Arc::new(IndexFreshness::default());
        let (silent, requests) = counting_verifier(&freshness, false);
        freshness.set_verifier(silent.clone());
        let waiting = {
            let freshness = freshness.clone();
            std::thread::spawn(move || freshness.verify(Duration::from_secs(30)))
        };
        while requests.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        freshness.clear_verifier(&silent);
        assert!(waiting.join().unwrap());
    }

    /// HS2-P702Y8: with a verifier registered, an in-sync miss is final only after a
    /// verification. A lagging stream (no answer) sends it to the walk; a recent
    /// verification skips the wait; a prompt one settles it by asking the index again,
    /// which also finds a ticket the index learned of meanwhile; a verification landing
    /// while out of sync still walks.
    #[test]
    fn hs2_p702y8_an_in_sync_miss_needs_a_recent_verification() {
        let (_dir, store) = store();
        let index = indexed(&store);
        let late = create(&store, "late");
        let freshness = Arc::new(live());

        let (lagging, requests) = counting_verifier(&freshness, false);
        freshness.set_verifier(lagging.clone());
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
        assert_eq!(requests.load(Ordering::SeqCst), 1);

        freshness.mark_verified();
        assert!(!found_unindexed(&store, &index, &late.slug, &freshness));
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "a recent verification is enough"
        );
        freshness.clear_verifier(&lagging);

        let (prompt, answered) = counting_verifier(&freshness, true);
        freshness.set_verifier(prompt);
        std::thread::sleep(VERIFIED_WINDOW + Duration::from_millis(50));
        let asked = Cell::new(0);
        let missed = resolve_ticket_with_freshness(
            &store,
            &late.slug,
            |slug| {
                asked.set(asked.get() + 1);
                index.slug_candidates(slug)
            },
            &freshness,
        )
        .unwrap();
        assert!(missed.is_none(), "a verified miss is final");
        assert_eq!(asked.get(), 2, "the index is asked again after verifying");
        assert_eq!(answered.load(Ordering::SeqCst), 1);

        std::thread::sleep(VERIFIED_WINDOW + Duration::from_millis(50));
        let learned = Cell::new(false);
        let found = resolve_ticket_with_freshness(
            &store,
            &late.slug,
            |slug| {
                if learned.replace(true) {
                    Ok(vec![late.id])
                } else {
                    index.slug_candidates(slug)
                }
            },
            &freshness,
        )
        .unwrap();
        assert_eq!(found.unwrap().id, late.id);

        std::thread::sleep(VERIFIED_WINDOW + Duration::from_millis(50));
        freshness.set_live(false);
        assert!(found_unindexed(&store, &index, &late.slug, &freshness));
    }
}
