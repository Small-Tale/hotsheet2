//! HS2-K6KG1T transition tests: a claim starts the work it leases from Not Started or
//! Backlog, a phase edit never applies to a ticket outside Started, and an edit that ends a
//! claim says so instead of dropping it silently.

use super::*;
use crate::store::StoreMetadata;

fn store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, store)
}

fn ts(s: &str) -> Timestamp {
    Timestamp::new(s)
}

fn add(store: &FsStore, id: u128, status: Status) -> Ticket {
    create(
        store,
        Ulid(id),
        "HS",
        ts("2026-11-26T00:00:00Z"),
        NewTicket {
            title: format!("synthetic {id}"),
            status,
            ..Default::default()
        },
    )
    .unwrap()
}

fn claim_at(store: &FsStore, id: &Ulid, at: &str, worker: &str) -> Result<Ticket, OpError> {
    let now = ts(at);
    claim(store, id, &now, now.plus_minutes(30), worker, None)
}

fn set(store: &FsStore, id: &Ulid, at: &str, patch: TicketPatch) -> Result<Ticket, StoreError> {
    update(store, id, ts(at), patch)
}

fn status(s: Status) -> TicketPatch {
    TicketPatch {
        status: Some(s),
        ..Default::default()
    }
}

fn phase(p: StartedPhase) -> TicketPatch {
    TicketPatch {
        started_phase: Some(Some(p)),
        ..Default::default()
    }
}

fn last_note(t: &Ticket) -> &str {
    t.notes.last().map_or("", |note| note.text.as_str())
}

/// Write a live claim straight onto a ticket, as releases before HS2-K6KG1T could leave on
/// a Backlog ticket.
fn legacy_claim(store: &FsStore, id: &Ulid, worker: &str, until: &str) -> Ticket {
    let mut t = store.read_ticket(id).unwrap();
    t.claimed_by = Some(worker.into());
    t.claim_lease_expires_at = Some(ts(until));
    store.write_ticket_committing(&t).unwrap();
    t
}

#[test]
fn hs2_k6kg1t_claim_starts_not_started_and_backlog_tickets_at_analyzing() {
    for (n, from, label) in [
        (1, Status::NotStarted, "Not Started"),
        (2, Status::Backlog, "Backlog"),
    ] {
        let (_d, store) = store();
        let t = add(&store, n, from);
        let claimed = claim_at(&store, &t.id, "2026-11-26T00:01:00Z", "w1").unwrap();
        assert_eq!(claimed.status, Status::Started, "{label}");
        assert_eq!(claimed.started_phase, Some(StartedPhase::Analyzing));
        assert_eq!(claimed.claimed_by.as_deref(), Some("w1"));
        let texts: Vec<_> = claimed.notes.iter().map(|n| n.text.as_str()).collect();
        assert!(
            texts.contains(&format!("Status changed from {label} to Started").as_str()),
            "{texts:?}"
        );
        // The claim and the start are one durable write.
        assert_eq!(store.read_ticket(&t.id).unwrap(), claimed);
    }
}

#[test]
fn hs2_k6kg1t_claim_keeps_a_started_tickets_phase_and_refuses_closed_tickets() {
    let (_d, store) = store();
    let started = add(&store, 1, Status::Started);
    set(
        &store,
        &started.id,
        "2026-11-26T00:01:00Z",
        phase(StartedPhase::Planning),
    )
    .unwrap();
    let claimed = claim_at(&store, &started.id, "2026-11-26T00:02:00Z", "w1").unwrap();
    assert_eq!(claimed.status, Status::Started);
    assert_eq!(claimed.started_phase, Some(StartedPhase::Planning));

    for (n, closed) in [
        (2, Status::Completed),
        (3, Status::Verified),
        (4, Status::Archive),
        (5, Status::Deleted),
    ] {
        let t = add(&store, n, Status::NotStarted);
        let closed_ticket = set(&store, &t.id, "2026-11-26T00:03:00Z", status(closed)).unwrap();
        assert!(matches!(
            claim_at(&store, &t.id, "2026-11-26T00:04:00Z", "w1"),
            Err(OpError::ClaimUnavailable { .. })
        ));
        assert_eq!(
            store.read_ticket(&t.id).unwrap(),
            closed_ticket,
            "{closed:?}"
        );
    }
}

#[test]
fn hs2_k6kg1t_phase_edits_on_a_claimed_backlog_ticket_keep_the_claim() {
    let (_d, store) = store();
    let t = add(&store, 1, Status::Backlog);
    claim_at(&store, &t.id, "2026-11-26T00:01:00Z", "w1").unwrap();
    for (at, p) in [
        ("2026-11-26T00:02:00Z", StartedPhase::Working),
        ("2026-11-26T00:03:00Z", StartedPhase::InitialTesting),
        ("2026-11-26T00:04:00Z", StartedPhase::Working),
    ] {
        let edited = set(&store, &t.id, at, phase(p)).unwrap();
        assert_eq!(edited.claimed_by.as_deref(), Some("w1"));
        assert_eq!(edited.started_phase, Some(p));
        assert!(last_note(&edited).starts_with("Started phase changed from"));
    }
    let history: Vec<_> = store
        .read_ticket(&t.id)
        .unwrap()
        .claim_history
        .iter()
        .map(|event| event.kind)
        .collect();
    assert_eq!(history, [ClaimEventKind::Claim], "no release was recorded");
}

#[test]
fn hs2_k6kg1t_a_phase_outside_started_is_refused_and_writes_nothing() {
    let (_d, store) = store();
    for (n, from) in [(1, Status::NotStarted), (2, Status::Backlog)] {
        let t = add(&store, n, from);
        let error = set(
            &store,
            &t.id,
            "2026-11-26T00:01:00Z",
            phase(StartedPhase::Working),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            StoreError::StartedPhaseRequiresStarted { .. }
        ));
        assert!(error.to_string().contains("not Started"), "{error}");
        assert_eq!(store.read_ticket(&t.id).unwrap(), t);
        // Clearing a phase that does not exist stays a harmless no-op.
        let cleared = set(
            &store,
            &t.id,
            "2026-11-26T00:02:00Z",
            TicketPatch {
                started_phase: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cleared, t);
        // Status and phase in one edit start the ticket at that phase.
        let started = set(
            &store,
            &t.id,
            "2026-11-26T00:03:00Z",
            TicketPatch {
                status: Some(Status::Started),
                started_phase: Some(Some(StartedPhase::Working)),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(started.status, Status::Started);
        assert_eq!(started.started_phase, Some(StartedPhase::Working));
    }
    // A phase alongside a status that leaves Started is refused as a whole.
    let t = add(&store, 3, Status::Started);
    let before = store.read_ticket(&t.id).unwrap();
    assert!(matches!(
        set(
            &store,
            &t.id,
            "2026-11-26T00:04:00Z",
            TicketPatch {
                status: Some(Status::Backlog),
                started_phase: Some(Some(StartedPhase::Working)),
                ..Default::default()
            },
        ),
        Err(StoreError::StartedPhaseRequiresStarted { .. })
    ));
    assert_eq!(store.read_ticket(&t.id).unwrap(), before);
}

#[test]
fn hs2_k6kg1t_repeated_claim_release_and_reclaim_keep_status_and_phase() {
    let (_d, store) = store();
    let t = add(&store, 1, Status::Backlog);
    let first = claim_at(&store, &t.id, "2026-11-26T00:01:00Z", "w1").unwrap();
    let retry = claim_at(&store, &t.id, "2026-11-26T00:02:00Z", "w1").unwrap();
    assert_eq!(
        retry.claim_count, 1,
        "a holder retry is not a new acquisition"
    );
    assert_eq!(
        retry.notes.len(),
        first.notes.len(),
        "no repeated transition"
    );
    set(
        &store,
        &t.id,
        "2026-11-26T00:03:00Z",
        phase(StartedPhase::Working),
    )
    .unwrap();
    let released = release(&store, &t.id, ts("2026-11-26T00:04:00Z"), "w1", false).unwrap();
    assert_eq!(released.status, Status::Started);
    assert_eq!(released.started_phase, Some(StartedPhase::Working));
    let again = claim_at(&store, &t.id, "2026-11-26T00:05:00Z", "w2").unwrap();
    assert_eq!(again.status, Status::Started);
    assert_eq!(again.started_phase, Some(StartedPhase::Working));
    assert_eq!(again.claim_count, 2);
    assert!(matches!(
        claim_at(&store, &t.id, "2026-11-26T00:06:00Z", "w1"),
        Err(OpError::WrongWorker { .. })
    ));
}

#[test]
fn hs2_k6kg1t_a_legacy_claim_on_backlog_is_taken_over_or_kept_explicitly() {
    let (_d, store) = store();
    // An expired legacy claim: a new worker's claim takes it and starts the ticket.
    let expired = add(&store, 1, Status::Backlog);
    legacy_claim(&store, &expired.id, "old", "2026-11-26T00:05:00Z");
    let taken = claim_at(&store, &expired.id, "2026-11-26T00:10:00Z", "w1").unwrap();
    assert_eq!(taken.claimed_by.as_deref(), Some("w1"));
    assert_eq!(taken.status, Status::Started);

    // A live legacy claim: another worker is refused and nothing changes.
    let live = add(&store, 2, Status::Backlog);
    let held = legacy_claim(&store, &live.id, "w1", "2026-11-26T01:00:00Z");
    assert!(matches!(
        claim_at(&store, &live.id, "2026-11-26T00:10:00Z", "w2"),
        Err(OpError::WrongWorker { .. })
    ));
    assert_eq!(store.read_ticket(&live.id).unwrap(), held);
    // Its phase edit is refused without touching the claim.
    assert!(
        set(
            &store,
            &live.id,
            "2026-11-26T00:11:00Z",
            phase(StartedPhase::Working)
        )
        .is_err()
    );
    assert_eq!(store.read_ticket(&live.id).unwrap(), held);
    // Starting it keeps the holder's claim.
    let started = set(
        &store,
        &live.id,
        "2026-11-26T00:12:00Z",
        status(Status::Started),
    )
    .unwrap();
    assert_eq!(started.claimed_by.as_deref(), Some("w1"));
    assert_eq!(ops_notice(&held, &started), None);
    // The holder's own claim retry also starts a still-Backlog legacy ticket.
    let retried = add(&store, 3, Status::Backlog);
    legacy_claim(&store, &retried.id, "w1", "2026-11-26T01:00:00Z");
    let retry = claim_at(&store, &retried.id, "2026-11-26T00:13:00Z", "w1").unwrap();
    assert_eq!(retry.status, Status::Started);
}

fn ops_notice(before: &Ticket, after: &Ticket) -> Option<String> {
    claim_released_by_edit_notice(before, after)
}

#[test]
fn hs2_k6kg1t_an_edit_that_ends_a_claim_names_it() {
    let (_d, store) = store();
    let t = add(&store, 1, Status::NotStarted);
    let claimed = claim_at(&store, &t.id, "2026-11-26T00:01:00Z", "w1").unwrap();
    // An edit that keeps the claim has no notice.
    let titled = set(
        &store,
        &t.id,
        "2026-11-26T00:02:00Z",
        TicketPatch {
            title: Some("renamed".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(ops_notice(&claimed, &titled), None);
    // Moving it to Backlog ends the claim, and the notice says so.
    let shelved = set(
        &store,
        &t.id,
        "2026-11-26T00:03:00Z",
        status(Status::Backlog),
    )
    .unwrap();
    assert!(shelved.claimed_by.is_none());
    let notice = ops_notice(&titled, &shelved).unwrap();
    assert!(notice.contains("Released w1's claim on HS-"), "{notice}");
    assert!(notice.contains("Backlog"), "{notice}");
    // Without a prior claim there is nothing to report.
    assert_eq!(ops_notice(&shelved, &shelved), None);

    // A legacy live claim on Backlog is healed by an unrelated edit, with the notice.
    let legacy = legacy_claim(&store, &t.id, "w2", "2026-11-26T01:00:00Z");
    let healed = set(
        &store,
        &t.id,
        "2026-11-26T00:04:00Z",
        TicketPatch {
            title: Some("renamed again".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(healed.claimed_by.is_none());
    assert!(ops_notice(&legacy, &healed).unwrap().contains("w2"));

    // Claim from Backlog again: started, and a phase edit keeps it.
    let reclaimed = claim_at(&store, &t.id, "2026-11-26T00:05:00Z", "w1").unwrap();
    assert_eq!(reclaimed.status, Status::Started);
    let working = set(
        &store,
        &t.id,
        "2026-11-26T00:06:00Z",
        phase(StartedPhase::Working),
    )
    .unwrap();
    assert_eq!(ops_notice(&reclaimed, &working), None);
}

#[test]
fn hs2_k6kg1t_claim_next_starts_a_backlog_ticket_it_selects() {
    let (_d, store) = store();
    let t = add(&store, 1, Status::Backlog);
    let now = ts("2026-11-26T00:01:00Z");
    let claimed = claim_next(&store, &now, now.plus_minutes(30), "w1", None)
        .unwrap()
        .expect("the only open ticket is selected");
    assert_eq!(claimed.id, t.id);
    assert_eq!(claimed.status, Status::Started);
    assert_eq!(claimed.started_phase, Some(StartedPhase::Analyzing));
    let working = set(
        &store,
        &t.id,
        "2026-11-26T00:02:00Z",
        phase(StartedPhase::Working),
    )
    .unwrap();
    assert_eq!(working.claimed_by.as_deref(), Some("w1"));
}
