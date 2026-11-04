//! HS2-BAK4BC transition tests: an explicit claim takes a blocked ticket, while automatic
//! selection keeps skipping it. A blocked reason is a re-evaluable note, never a claim fence.

use super::*;
use crate::store::StoreMetadata;
use std::sync::{Arc, Barrier};

const REASON: &str = "Waiting on a synthetic dependency";

fn store(prefix: &str) -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new(prefix)).unwrap();
    (dir, store)
}

fn ts(s: &str) -> Timestamp {
    Timestamp::new(s)
}

fn add(store: &FsStore, id: u128, title: &str) -> Ticket {
    create(
        store,
        Ulid(id),
        &store.metadata().unwrap().ticket_prefix,
        ts("2026-11-04T00:00:00Z"),
        NewTicket {
            title: title.into(),
            up_next: true,
            ..Default::default()
        },
    )
    .unwrap()
}

fn set_reason(store: &FsStore, id: &Ulid, at: &str, reason: Option<&str>) -> Ticket {
    update(
        store,
        id,
        ts(at),
        TicketPatch {
            blocked_reason: Some(reason.map(str::to_string)),
            ..Default::default()
        },
    )
    .unwrap()
}

fn claim_at(store: &FsStore, id: &Ulid, at: &str, worker: &str) -> Result<Ticket, OpError> {
    let now = ts(at);
    claim(store, id, &now, now.plus_minutes(30), worker, None)
}

#[test]
fn hs2_bak4bc_claim_renew_release_and_reclaim_a_blocked_ticket_keep_its_reason() {
    let (_d, store) = store("HS");
    let ticket = add(&store, 1, "blocked work");
    set_reason(&store, &ticket.id, "2026-11-04T00:01:00Z", Some(REASON));

    let claimed = claim_at(&store, &ticket.id, "2026-11-04T00:02:00Z", "w1").unwrap();
    assert_eq!(claimed.claimed_by.as_deref(), Some("w1"));
    assert_eq!(claimed.status, Status::Started);
    assert_eq!(claimed.started_phase, Some(StartedPhase::Analyzing));
    assert_eq!(claimed.claim_count, 1);
    assert_eq!(claimed.blocked_reason.as_deref(), Some(REASON));
    let notice = blocked_claim_notice(&claimed).unwrap();
    assert!(
        notice.starts_with(&format!("Blocked: {REASON}.")),
        "{notice}"
    );
    assert!(notice.contains(&format!("edit {} --clear-blocked-reason", claimed.slug)));

    // The same holder's retry stays idempotent on a blocked ticket.
    let retried = claim_at(&store, &ticket.id, "2026-11-04T00:03:00Z", "w1").unwrap();
    assert_eq!(retried.claim_count, 1);

    let now = ts("2026-11-04T00:10:00Z");
    let renewed = renew(&store, &ticket.id, now.clone(), now.plus_minutes(30), "w1").unwrap();
    assert_eq!(renewed.blocked_reason.as_deref(), Some(REASON));
    assert!(blocked_claim_notice(&renewed).is_some());

    // Another worker is still refused by the live claim, not by the block.
    assert!(matches!(
        claim_at(&store, &ticket.id, "2026-11-04T00:11:00Z", "w2"),
        Err(OpError::WrongWorker { .. })
    ));

    let released = release(&store, &ticket.id, ts("2026-11-04T00:12:00Z"), "w1", false).unwrap();
    assert_eq!(released.claimed_by, None);
    assert_eq!(released.blocked_reason.as_deref(), Some(REASON));
    assert_eq!(released.status, Status::Started, "release keeps status");

    let reclaimed = claim_at(&store, &ticket.id, "2026-11-04T00:13:00Z", "w2").unwrap();
    assert_eq!(reclaimed.claimed_by.as_deref(), Some("w2"));
    assert_eq!(reclaimed.claim_count, 2);
    assert_eq!(reclaimed.blocked_reason.as_deref(), Some(REASON));
    assert_eq!(
        reclaimed
            .claim_history
            .iter()
            .map(|event| (event.kind, event.worker.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (ClaimEventKind::Claim, "w1"),
            // The holder's retry extended the lease, then the explicit renew.
            (ClaimEventKind::Renew, "w1"),
            (ClaimEventKind::Renew, "w1"),
            (ClaimEventKind::Release, "w1"),
            (ClaimEventKind::Claim, "w2"),
        ]
    );
}

#[test]
fn hs2_bak4bc_clearing_then_setting_the_reason_while_claimed_keeps_the_claim() {
    let (_d, store) = store("HS");
    let ticket = add(&store, 1, "re-evaluated work");
    set_reason(&store, &ticket.id, "2026-11-04T00:01:00Z", Some(REASON));
    claim_at(&store, &ticket.id, "2026-11-04T00:02:00Z", "w1").unwrap();

    // The holder re-evaluates and clears the stale reason.
    let cleared = set_reason(&store, &ticket.id, "2026-11-04T00:03:00Z", None);
    assert_eq!(cleared.claimed_by.as_deref(), Some("w1"));
    assert_eq!(cleared.blocked_reason, None);
    assert!(!is_blocked(&cleared, &HashSet::new()));
    assert_eq!(blocked_claim_notice(&cleared), None);
    let now = ts("2026-11-04T00:04:00Z");
    let renewed = renew(&store, &ticket.id, now.clone(), now.plus_minutes(30), "w1").unwrap();
    assert_eq!(blocked_claim_notice(&renewed), None);

    // New information blocks it again while still claimed; the claim stays live.
    let reblocked = set_reason(
        &store,
        &ticket.id,
        "2026-11-04T00:05:00Z",
        Some("New block"),
    );
    assert_eq!(reblocked.claimed_by.as_deref(), Some("w1"));
    assert!(is_blocked(&reblocked, &HashSet::new()));
    let now = ts("2026-11-04T00:06:00Z");
    let renewed = renew(&store, &ticket.id, now.clone(), now.plus_minutes(30), "w1").unwrap();
    assert!(
        blocked_claim_notice(&renewed)
            .unwrap()
            .starts_with("Blocked: New block.")
    );
    // A whitespace-only reason is not a block and has no notice.
    let blank = set_reason(&store, &ticket.id, "2026-11-04T00:07:00Z", Some("   "));
    assert_eq!(blocked_claim_notice(&blank), None);
    assert_eq!(blank.claimed_by.as_deref(), Some("w1"));
}

#[test]
fn hs2_bak4bc_claim_next_skips_a_blocked_ticket_that_an_explicit_claim_takes() {
    let (_a, default) = store("DEF");
    let (_b, sibling) = store("SIB");
    let blocked = add(&default, 1, "blocked top");
    set_reason(&default, &blocked.id, "2026-11-04T00:01:00Z", Some(REASON));
    let sibling_blocked = add(&sibling, 2, "sibling blocked");
    set_reason(
        &sibling,
        &sibling_blocked.id,
        "2026-11-04T00:01:00Z",
        Some(REASON),
    );
    add(&default, 3, "ready later");

    let now = ts("2026-11-04T00:02:00Z");
    let picked = claim_next(&default, &now, now.plus_minutes(30), "auto", None)
        .unwrap()
        .unwrap();
    assert_eq!(picked.title, "ready later", "claim-next skips blocked");
    let across = claim_next_across_with_eta(
        &[&default, &sibling],
        &now,
        now.plus_minutes(30),
        "auto-2",
        None,
        None,
    )
    .unwrap();
    assert!(across.is_none(), "cross-source claim-next skips blocked");

    let explicit = claim_at(&default, &blocked.id, "2026-11-04T00:03:00Z", "person").unwrap();
    assert_eq!(explicit.claimed_by.as_deref(), Some("person"));
    let explicit = claim_at(
        &sibling,
        &sibling_blocked.id,
        "2026-11-04T00:03:00Z",
        "person",
    )
    .unwrap();
    assert_eq!(explicit.blocked_reason.as_deref(), Some(REASON));

    // After the explicit holder releases, auto-selection still skips the blocked ticket.
    release(
        &default,
        &blocked.id,
        ts("2026-11-04T00:04:00Z"),
        "person",
        false,
    )
    .unwrap();
    let now = ts("2026-11-04T00:05:00Z");
    assert!(
        claim_next(&default, &now, now.plus_minutes(30), "auto-3", None)
            .unwrap()
            .is_none()
    );
    // Clearing the reason makes it eligible again.
    set_reason(&default, &blocked.id, "2026-11-04T00:06:00Z", None);
    let now = ts("2026-11-04T00:07:00Z");
    let picked = claim_next(&default, &now, now.plus_minutes(30), "auto-3", None)
        .unwrap()
        .unwrap();
    assert_eq!(picked.id, blocked.id);
}

#[test]
fn hs2_bak4bc_concurrent_explicit_claims_of_one_blocked_ticket_have_one_winner() {
    let (_d, store) = store("HS");
    let ticket = add(&store, 1, "contended blocked work");
    set_reason(&store, &ticket.id, "2026-11-04T00:01:00Z", Some(REASON));
    let workers = 8;
    let barrier = Arc::new(Barrier::new(workers));
    let root = store.root().to_path_buf();
    let handles: Vec<_> = (0..workers)
        .map(|worker| {
            let barrier = barrier.clone();
            let root = root.clone();
            let id = ticket.id;
            std::thread::spawn(move || {
                let store = FsStore::open(&root).unwrap();
                barrier.wait();
                claim_at(&store, &id, "2026-11-04T00:02:00Z", &format!("w{worker}"))
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let winners: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(winners.len(), 1, "exactly one explicit claim wins");
    assert!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| matches!(e, OpError::WrongWorker { .. })),
        "losers see the live holder, not a blocked refusal"
    );
    let saved = store.read_ticket(&ticket.id).unwrap();
    assert_eq!(saved.claimed_by, winners[0].claimed_by);
    assert_eq!(saved.claim_count, 1);
    assert_eq!(saved.blocked_reason.as_deref(), Some(REASON));
}
