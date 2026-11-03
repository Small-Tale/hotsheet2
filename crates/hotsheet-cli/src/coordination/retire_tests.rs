//! Transition coverage for retiring stale records on closed tickets (HS2-V9BXWF).

use super::retire::RETIREMENT_HEADING;
use super::*;
use crate::coordination::tests::{assignment, fixture, register};
use hotsheet_model::{NoteKind, Status, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::NewTicket;

const NOW: i64 = 1_800_000_000;
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const GEN: &str = "generation-1";

fn at() -> time::OffsetDateTime {
    time::OffsetDateTime::from_unix_timestamp(NOW).unwrap()
}

fn ticket(store: &FsStore, title: &str) -> Ticket {
    ops::create(
        store,
        Ulid::new(),
        "HS",
        Timestamp::from_datetime(at()),
        NewTicket {
            title: title.into(),
            up_next: true,
            ..NewTicket::default()
        },
    )
    .unwrap()
}

fn set_status(store: &FsStore, id: &Ulid, status: Status) {
    let mut t = store.read_ticket(id).unwrap();
    t.status = status;
    store.write_ticket(&t).unwrap();
}

fn note(store: &FsStore, id: &Ulid, text: String) -> Ulid {
    let note = Ulid::new();
    ops::add_note(
        store,
        id,
        note,
        Timestamp::from_datetime(at()),
        NoteKind::Regular,
        text,
    )
    .unwrap();
    note
}

/// Register the integrator and a fresh worker, then reserve and accept `op` on `t`.
fn accepted(c: &Coordinator, store: &FsStore, op: &str, worker: &str, t: &Ticket) {
    register(c, Role::Integrator, "i", GEN, NOW);
    register(c, Role::Worker, worker, GEN, NOW);
    let mut a = assignment(op, worker, &t.id.to_string());
    a.ticket.connection_id = git_connection_id(store);
    a.slug = t.slug.clone();
    c.assign("i", GEN, NOW, a).unwrap();
    c.worker_transition(worker, GEN, op, Phase::Accepted, None)
        .unwrap();
}

fn findings_handoff(c: &Coordinator, store: &FsStore, op: &str, worker: &str, t: &Ticket) -> Ulid {
    accepted(c, store, op, worker, t);
    let findings = note(
        store,
        &t.id,
        "## Findings handoff\nSynthetic verified result; no code change.".into(),
    );
    c.worker_findings_transition(worker, GEN, op, findings.to_string())
        .unwrap();
    findings
}

fn retirement_notes(store: &FsStore, t: &Ticket, op: &str) -> usize {
    store
        .read_ticket(&t.id)
        .unwrap()
        .notes
        .iter()
        .filter(|n| {
            n.kind == NoteKind::Regular
                && n.text.lines().next() == Some(RETIREMENT_HEADING)
                && n.text.split_whitespace().any(|w| w == op)
        })
        .count()
}

fn assert_retired(c: &Coordinator, store: &FsStore, t: &Ticket, op: &str) -> Assignment {
    let a = c.state().unwrap().assignments[op].clone();
    assert_eq!(a.phase, Phase::Done);
    assert_eq!(a.transitions.last().unwrap().kind, TransitionKind::Done);
    let note = a.retirement_note.clone().expect("retirement note recorded");
    assert!(
        store
            .read_ticket(&t.id)
            .unwrap()
            .notes
            .iter()
            .any(|n| n.id.to_string() == note && n.text.starts_with(RETIREMENT_HEADING))
    );
    assert_eq!(retirement_notes(store, t, op), 1);
    assert!(c.snapshot(NOW).unwrap().finished_assignments.is_empty());
    a
}

#[test]
fn hs2_v9bxwf_review_returned_code_handoff_on_archived_ticket_retires_once() {
    let (_root, c) = fixture();
    let store = FsStore::open(&c.source).unwrap();
    let t = ticket(&store, "Synthetic reviewed task");
    accepted(&c, &store, "original", "a", &t);
    c.worker_transition("a", GEN, "original", Phase::Handoff, Some(SHA.into()))
        .unwrap();
    let review = note(
        &store,
        &t.id,
        format!("## Review return\noriginal {SHA}\nCorrect it."),
    );
    c.review_return("i", GEN, at(), "original", &review.to_string())
        .unwrap();
    // Stuck state 1: the transition table preserves the review return forever.
    let refused = c.mark("i", GEN, NOW, "original", Phase::Done).unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("review-returned handoff is preserved")
    );
    // While the ticket is open, the record is not finished and retirement leaves it.
    assert!(c.snapshot(NOW).unwrap().finished_assignments.is_empty());
    let report = c.retire_finished("i", GEN, at(), None, false).unwrap();
    assert!(report.retired.is_empty() && report.skipped.is_empty());

    set_status(&store, &t.id, Status::Archive);
    assert_eq!(c.snapshot(NOW).unwrap().finished_assignments, ["original"]);
    let before = c.state().unwrap().assignments["original"].clone();
    let report = c.retire_finished("i", GEN, at(), None, false).unwrap();
    assert_eq!(report.retired.len(), 1);
    let retired = &report.retired[0];
    assert!(retired.review_returned);
    assert_eq!(retired.previous_phase, Phase::Blocked);
    assert_eq!(retired.ticket_status, Status::Archive);
    let after = assert_retired(&c, &store, &t, "original");
    // Audit: the original evidence and the review return survive.
    assert_eq!(after.commit, before.commit);
    assert_eq!(after.handoff, before.handoff);
    assert_eq!(after.recovery_note, Some(review.to_string()));

    // A rerun changes nothing and writes no second note.
    let revision = c.state().unwrap().revision;
    let again = c.retire_finished("i", GEN, at(), None, false).unwrap();
    assert!(again.retired.is_empty());
    assert_eq!(c.state().unwrap().revision, revision);
    assert_eq!(retirement_notes(&store, &t, "original"), 1);
}

#[test]
fn hs2_v9bxwf_findings_handoff_on_archived_ticket_is_terminal_for_finish_and_retires() {
    let (_root, c) = fixture();
    let store = FsStore::open(&c.source).unwrap();
    let t = ticket(&store, "Synthetic findings task");
    findings_handoff(&c, &store, "findings", "a", &t);
    let busy = c.finish("i", GEN, NOW, "blocked").unwrap_err();
    assert!(busy.to_string().contains("work or CI remains in flight"));

    // Stuck state 2: an archived ticket refuses the claim verification needs, but the
    // record is now terminal for `finish` even before it is retired.
    set_status(&store, &t.id, Status::Archive);
    assert!(
        ops::claim(
            &store,
            &t.id,
            &Timestamp::from_datetime(at()),
            Timestamp::from_datetime(at() + time::Duration::minutes(1)),
            "i",
            None,
        )
        .is_err()
    );
    c.finish("i", GEN, NOW, "blocked").unwrap();
    c.resume().unwrap();

    let report = c.retire_finished("i", GEN, at(), None, false).unwrap();
    assert_eq!(report.retired.len(), 1);
    assert_eq!(report.retired[0].previous_phase, Phase::Handoff);
    assert!(!report.retired[0].review_returned);
    let retired = assert_retired(&c, &store, &t, "findings");
    assert!(matches!(
        retired.handoff,
        Some(HandoffEvidence::Findings { .. })
    ));
    c.finish("i", GEN, NOW, "blocked").unwrap();
}

#[test]
fn hs2_v9bxwf_review_returned_findings_on_archived_ticket_retires() {
    let (_root, c) = fixture();
    let store = FsStore::open(&c.source).unwrap();
    let t = ticket(&store, "Synthetic returned findings task");
    let findings = findings_handoff(&c, &store, "findings", "a", &t);
    let review = note(
        &store,
        &t.id,
        format!("## Review return\nfindings {findings}\nClarify the evidence."),
    );
    c.review_return("i", GEN, at(), "findings", &review.to_string())
        .unwrap();
    assert!(c.state().unwrap().assignments["findings"].returned_for_review());
    // Stuck state 3: blocked findings on an archived ticket cannot reach done.
    set_status(&store, &t.id, Status::Archive);
    assert!(c.mark("i", GEN, NOW, "findings", Phase::Done).is_err());

    let report = c.retire_finished("i", GEN, at(), None, false).unwrap();
    assert_eq!(report.retired.len(), 1);
    assert!(report.retired[0].review_returned);
    let retired = assert_retired(&c, &store, &t, "findings");
    assert_eq!(retired.recovery_note, Some(review.to_string()));
}

#[test]
fn hs2_v9bxwf_retirement_frees_worker_and_integration_fence_for_completed_tickets() {
    let (_root, c) = fixture();
    let store = FsStore::open(&c.source).unwrap();
    let held = ticket(&store, "Synthetic accepted task");
    let integrating = ticket(&store, "Synthetic integrating task");
    let next = ticket(&store, "Synthetic next task");
    accepted(&c, &store, "held", "a", &held);
    accepted(&c, &store, "integrating", "b", &integrating);
    c.worker_transition("b", GEN, "integrating", Phase::Handoff, Some(SHA.into()))
        .unwrap();
    c.mark("i", GEN, NOW, "integrating", Phase::Integrating)
        .unwrap();
    set_status(&store, &held.id, Status::Completed);
    set_status(&store, &integrating.id, Status::Verified);
    let mut finished = c.snapshot(NOW).unwrap().finished_assignments;
    finished.sort();
    assert_eq!(finished, ["held", "integrating"]);

    let report = c.retire_finished("i", GEN, at(), None, false).unwrap();
    assert_eq!(report.retired.len(), 2);
    assert_retired(&c, &store, &held, "held");
    assert_retired(&c, &store, &integrating, "integrating");
    // The worker reservation and the serial integration fence are both released.
    assert!(
        !c.state()
            .unwrap()
            .assignments
            .values()
            .any(|a| a.phase.holds_worker() || a.phase == Phase::Integrating)
    );
    let mut again = assignment("next", "a", &next.id.to_string());
    again.ticket.connection_id = git_connection_id(&store);
    again.slug = next.slug.clone();
    c.assign("i", GEN, NOW, again).unwrap();
}

#[test]
fn hs2_v9bxwf_retirement_skips_live_claims_reuses_notes_and_needs_the_integrator() {
    let (_root, c) = fixture();
    let store = FsStore::open(&c.source).unwrap();
    let claimed = ticket(&store, "Synthetic claimed task");
    let interrupted = ticket(&store, "Synthetic interrupted task");
    let open = ticket(&store, "Synthetic open task");
    findings_handoff(&c, &store, "claimed", "a", &claimed);
    findings_handoff(&c, &store, "interrupted", "b", &interrupted);
    findings_handoff(&c, &store, "open", "d", &open);
    ops::claim(
        &store,
        &claimed.id,
        &Timestamp::from_datetime(at()),
        Timestamp::from_datetime(at() + time::Duration::minutes(5)),
        "i",
        None,
    )
    .unwrap();
    set_status(&store, &claimed.id, Status::Completed);
    set_status(&store, &interrupted.id, Status::Verified);
    // A run interrupted after its note was written: the rerun reuses that note.
    let earlier = note(
        &store,
        &interrupted.id,
        format!("{RETIREMENT_HEADING}\n\n- assignment: interrupted\n"),
    );

    // Without the exact live integrator nothing is written.
    let notes = store.read_ticket(&interrupted.id).unwrap().notes.len();
    assert!(
        c.retire_finished("i", "other-generation", at(), None, false)
            .is_err()
    );
    assert_eq!(
        store.read_ticket(&interrupted.id).unwrap().notes.len(),
        notes
    );

    let preview = c.retire_finished("i", GEN, at(), None, true).unwrap();
    assert!(preview.dry_run);
    assert_eq!(preview.retired.len(), 1);
    assert_eq!(preview.retired[0].note, None);
    assert_eq!(
        c.state().unwrap().assignments["interrupted"].phase,
        Phase::Handoff
    );

    let report = c.retire_finished("i", GEN, at(), None, false).unwrap();
    assert_eq!(report.retired.len(), 1);
    assert_eq!(report.retired[0].note, Some(earlier.to_string()));
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].assignment, "claimed");
    assert!(report.skipped[0].reason.contains("live claim"));
    let state = c.state().unwrap();
    assert_eq!(state.assignments["claimed"].phase, Phase::Handoff);
    assert_eq!(state.assignments["open"].phase, Phase::Handoff);
    assert_eq!(
        state.assignments["interrupted"].retirement_note,
        Some(earlier.to_string())
    );
    assert_eq!(retirement_notes(&store, &interrupted, "interrupted"), 1);
    // The claimed record stays finished (terminal for display and finish) until released.
    assert_eq!(c.snapshot(NOW).unwrap().finished_assignments, ["claimed"]);
}

#[test]
fn hs2_v9bxwf_foreign_source_records_are_skipped_not_guessed() {
    let (_root, c) = fixture();
    let store = FsStore::open(&c.source).unwrap();
    register(&c, Role::Integrator, "i", GEN, NOW);
    register(&c, Role::Worker, "a", GEN, NOW);
    let t = ticket(&store, "Synthetic foreign task");
    // The shared fixture assignment names another connection id than the local store.
    c.assign("i", GEN, NOW, assignment("foreign", "a", &t.id.to_string()))
        .unwrap();
    set_status(&store, &t.id, Status::Archive);
    assert!(c.snapshot(NOW).unwrap().finished_assignments.is_empty());
    let report = c.retire_finished("i", GEN, at(), None, false).unwrap();
    assert!(report.retired.is_empty());
    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].reason.contains("another source"));
    assert_eq!(
        c.state().unwrap().assignments["foreign"].phase,
        Phase::Offered
    );
}
