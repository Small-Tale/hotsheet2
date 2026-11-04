//! HS2-AV80C4 transition tests for project claim-next across several local stores.

use super::*;
use crate::store::StoreMetadata;
use std::sync::{Arc, Barrier};

fn store(prefix: &str) -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new(prefix)).unwrap();
    (dir, store)
}

fn ts(s: &str) -> Timestamp {
    Timestamp::new(s)
}

fn add(store: &FsStore, id: u128, title: &str, up_next: bool, priority: Priority) -> Ticket {
    create(
        store,
        Ulid(id),
        &store.metadata().unwrap().ticket_prefix,
        ts("2026-11-01T00:00:00Z"),
        NewTicket {
            title: title.into(),
            up_next,
            priority,
            ..Default::default()
        },
    )
    .unwrap()
}

fn next(stores: &[&FsStore], worker: &str, at: &str) -> Option<(usize, String)> {
    let now = ts(at);
    claim_next_across_with_eta(stores, &now, now.plus_minutes(30), worker, None, None)
        .unwrap()
        .map(|(index, ticket)| (index, ticket.title))
}

fn patch(store: &FsStore, id: &Ulid, at: &str, patch: TicketPatch) {
    update(store, id, ts(at), patch).unwrap();
}

#[test]
fn hs2_av80c4_sources_form_one_queue_with_source_precedence_as_the_last_tie() {
    let (_a, default) = store("DEF");
    let (_b, sibling) = store("SIB");
    add(
        &default,
        10,
        "default up-next normal",
        true,
        Priority::Default,
    );
    add(
        &default,
        11,
        "default backlog urgent",
        false,
        Priority::Highest,
    );
    add(&sibling, 20, "sibling up-next high", true, Priority::High);
    add(
        &sibling,
        5,
        "sibling up-next normal older",
        true,
        Priority::Default,
    );
    // The same ticket identity in both stores: the default source wins the tie.
    add(
        &default,
        30,
        "default backlog low twin",
        false,
        Priority::Low,
    );
    add(
        &sibling,
        30,
        "sibling backlog low twin",
        false,
        Priority::Low,
    );
    let stores = [&default, &sibling];
    let at = "2026-11-01T01:00:00Z";
    let order: Vec<_> = std::iter::from_fn(|| next(&stores, "agent", at)).collect();
    assert_eq!(
        order,
        [
            (1, "sibling up-next high".to_owned()),
            (1, "sibling up-next normal older".to_owned()),
            (0, "default up-next normal".to_owned()),
            (0, "default backlog urgent".to_owned()),
            (0, "default backlog low twin".to_owned()),
            (1, "sibling backlog low twin".to_owned()),
        ]
    );
    assert_eq!(next(&stores, "agent", at), None);
}

#[test]
fn hs2_av80c4_single_store_keeps_the_store_local_claim_next() {
    let (_a, only) = store("ONE");
    add(&only, 2, "second", true, Priority::Default);
    add(&only, 1, "first", true, Priority::Default);
    assert_eq!(
        next(&[&only], "agent", "2026-11-01T01:00:00Z"),
        Some((0, "first".to_owned()))
    );
    assert_eq!(next(&[], "agent", "2026-11-01T01:00:00Z"), None);
}

#[test]
fn hs2_av80c4_empty_then_refill_expiry_block_and_final_testing_transitions() {
    let (_a, default) = store("DEF");
    let (_b, sibling) = store("SIB");
    let stores = [&default, &sibling];
    assert_eq!(next(&stores, "agent", "2026-11-01T01:00:00Z"), None);
    let refilled = add(&sibling, 1, "refilled sibling", true, Priority::Default);
    assert_eq!(
        next(&stores, "agent", "2026-11-01T01:00:00Z"),
        Some((1, "refilled sibling".to_owned()))
    );
    // A live lease is unavailable; a repeated call by the holder never re-claims it.
    assert_eq!(next(&stores, "agent", "2026-11-01T01:10:00Z"), None);
    // An expired lease is a new acquisition by another worker.
    assert_eq!(
        next(&stores, "other", "2026-11-01T01:31:00Z"),
        Some((1, "refilled sibling".to_owned()))
    );
    let reclaimed = sibling.read_ticket(&refilled.id).unwrap();
    assert_eq!(reclaimed.claimed_by.as_deref(), Some("other"));
    assert_eq!(reclaimed.claim_count, 2);
    // A released final-testing ticket and a blocked ticket stay out of the queue.
    patch(
        &sibling,
        &refilled.id,
        "2026-11-01T01:32:00Z",
        TicketPatch {
            started_phase: Some(Some(StartedPhase::FinalTesting)),
            ..Default::default()
        },
    );
    release(
        &sibling,
        &refilled.id,
        ts("2026-11-01T01:33:00Z"),
        "other",
        false,
    )
    .unwrap();
    let blocked = add(&default, 2, "blocked default", true, Priority::Highest);
    patch(
        &default,
        &blocked.id,
        "2026-11-01T01:34:00Z",
        TicketPatch {
            blocked_reason: Some(Some("waiting on a decision".into())),
            ..Default::default()
        },
    );
    assert_eq!(next(&stores, "agent", "2026-11-01T01:35:00Z"), None);
    patch(
        &default,
        &blocked.id,
        "2026-11-01T01:36:00Z",
        TicketPatch {
            blocked_reason: Some(None),
            ..Default::default()
        },
    );
    assert_eq!(
        next(&stores, "agent", "2026-11-01T01:37:00Z"),
        Some((0, "blocked default".to_owned()))
    );
}

#[test]
fn hs2_av80c4_a_lost_selection_race_reselects_instead_of_reporting_empty() {
    let (_a, default) = store("DEF");
    let (_b, sibling) = store("SIB");
    let top = add(&sibling, 1, "contended", true, Priority::High);
    add(&default, 2, "runner-up", true, Priority::Default);
    let now = ts("2026-11-01T01:00:00Z");
    // Another worker claims the selected ticket between selection and claim.
    assert_eq!(peek_claim_next(&sibling, &now).unwrap().unwrap().id, top.id);
    claim(&sibling, &top.id, &now, now.plus_minutes(30), "rival", None).unwrap();
    let lost = claim_next_candidate(
        &sibling,
        &top.id,
        &now,
        now.plus_minutes(30),
        "agent",
        None,
        None,
    )
    .unwrap();
    assert!(lost.is_none());
    assert_eq!(
        next(&[&default, &sibling], "agent", "2026-11-01T01:00:00Z"),
        Some((0, "runner-up".to_owned()))
    );
    let held = sibling.read_ticket(&top.id).unwrap();
    assert_eq!(held.claimed_by.as_deref(), Some("rival"));
    assert_eq!(held.claim_count, 1);
}

#[test]
fn hs2_av80c4_racing_workers_claim_each_ticket_exactly_once() {
    let (_a, default) = store("DEF");
    let (_b, sibling) = store("SIB");
    let mut id = 1;
    for (index, store) in [&default, &sibling].into_iter().enumerate() {
        for n in 0..6 {
            let title = format!("store {index} ticket {n}");
            add(store, id, &title, n % 2 == 0, Priority::Default);
            id += 1;
        }
    }
    let workers = 8;
    let barrier = Arc::new(Barrier::new(workers));
    let roots = [default.root().to_path_buf(), sibling.root().to_path_buf()];
    let handles: Vec<_> = (0..workers)
        .map(|worker| {
            let barrier = barrier.clone();
            let roots = roots.clone();
            std::thread::spawn(move || {
                let stores: Vec<FsStore> = roots
                    .iter()
                    .map(|root| FsStore::open(root).unwrap())
                    .collect();
                let refs: Vec<&FsStore> = stores.iter().collect();
                let name = format!("worker-{worker}");
                barrier.wait();
                let mut claimed = Vec::new();
                while let Some((_, title)) = next(&refs, &name, "2026-11-01T01:00:00Z") {
                    claimed.push(title);
                }
                claimed
            })
        })
        .collect();
    let mut all: Vec<_> = handles
        .into_iter()
        .flat_map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(all.len(), 12, "every ticket is claimed once: {all:?}");
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 12, "no ticket is claimed twice");
    for store in [&default, &sibling] {
        for ticket in store.list_tickets().unwrap() {
            assert_eq!(ticket.claim_count, 1, "{}", ticket.title);
            assert_eq!(ticket.status, Status::Started);
        }
    }
}
