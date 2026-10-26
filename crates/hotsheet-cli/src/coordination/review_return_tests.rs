use super::*;
use crate::coordination::tests::{assignment, fixture, register};
use hotsheet_model::{NoteKind, Timestamp, Ulid};
use hotsheet_ticketing::NewTicket;

const NOW: i64 = 1_800_000_000;
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn at() -> time::OffsetDateTime {
    time::OffsetDateTime::from_unix_timestamp(NOW).unwrap()
}

fn handoff(c: &Coordinator) -> (FsStore, hotsheet_model::Ticket, String) {
    let store = FsStore::open(&c.source).unwrap();
    let t = ops::create(
        &store,
        Ulid::new(),
        "HS",
        Timestamp::from_datetime(at()),
        NewTicket {
            title: "synthetic reviewed task".into(),
            up_next: true,
            ..NewTicket::default()
        },
    )
    .unwrap();
    register(c, Role::Integrator, "i", "generation-1", NOW);
    register(c, Role::Worker, "a", "generation-1", NOW);
    let mut a = assignment("original", "a", &t.id.to_string());
    a.ticket.connection_id = git_connection_id(&store);
    a.slug = t.slug.clone();
    c.assign("i", "generation-1", NOW, a).unwrap();
    c.worker_transition("a", "generation-1", "original", Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        "a",
        "generation-1",
        "original",
        Phase::Handoff,
        Some(SHA.into()),
    )
    .unwrap();
    let note = Ulid::new();
    ops::add_note(&store, &t.id, note, Timestamp::from_datetime(at()), NoteKind::Regular,
        format!("## Review return\noriginal {SHA}\nPreserve branch, worktree and commit; correct synthetic behavior.")).unwrap();
    (store, t, note.to_string())
}

#[test]
fn hs2_65etrj_return_replays_after_restart_and_refill_without_losing_handoff() {
    let (root, c) = fixture();
    let (_store, t, note) = handoff(&c);
    let before = c.state().unwrap().assignments["original"].clone();
    c.review_return("i", "generation-1", at(), "original", &note)
        .unwrap();
    let reopened = Coordinator::open(&root.path().join("project"), &c.source).unwrap();
    let mut returned = before.clone();
    returned.phase = Phase::Blocked;
    returned.recovery_note = Some(note.clone());
    assert_eq!(reopened.state().unwrap().version, 2);
    assert_eq!(reopened.state().unwrap().assignments["original"], returned);
    let mut correction = before;
    correction.id = "correction".into();
    correction.phase = Phase::Offered;
    correction.commit = None;
    correction.handoff = None;
    reopened
        .assign("i", "generation-1", NOW, correction.clone())
        .unwrap();
    let revision = reopened.state().unwrap().revision;
    reopened
        .review_return("i", "generation-1", at(), "original", &note)
        .unwrap();
    assert_eq!(
        reopened
            .assign("i", "generation-1", NOW, correction)
            .unwrap()
            .id,
        "correction"
    );
    assert_eq!(reopened.state().unwrap().revision, revision);
    assert!(
        reopened
            .worker_transition(
                "a",
                "generation-1",
                "original",
                Phase::Handoff,
                Some(SHA.into())
            )
            .is_err()
    );
    for phase in [Phase::Done, Phase::Integrating, Phase::Ci] {
        assert!(
            reopened
                .mark("i", "generation-1", NOW, "original", phase)
                .is_err()
        );
    }
    assert!(ops::is_open(
        &FsStore::open(&c.source)
            .unwrap()
            .read_ticket(&t.id)
            .unwrap()
    ));
    assert_eq!(reopened.state().unwrap().assignments["original"], returned);
}

#[test]
fn hs2_65etrj_return_rejects_stale_sessions_claims_evidence_and_non_handoffs() {
    let (_root, c) = fixture();
    let (store, t, note) = handoff(&c);
    let before = c.state().unwrap();
    assert!(
        c.review_return("i", "old", at(), "original", &note)
            .is_err()
    );
    assert!(
        c.review_return(
            "i",
            "generation-1",
            at() + time::Duration::seconds(121),
            "original",
            &note
        )
        .is_err()
    );
    assert!(
        c.review_return("i", "generation-1", at(), "missing", &note)
            .is_err()
    );
    assert!(
        c.review_return(
            "i",
            "generation-1",
            at(),
            "original",
            &Ulid::new().to_string()
        )
        .is_err()
    );
    for (kind, text) in [
        (
            NoteKind::Activity,
            format!("## Review return\noriginal {SHA}"),
        ),
        (NoteKind::Regular, format!("## Review return\nother {SHA}")),
        (
            NoteKind::Regular,
            "## Review return\noriginal wrong-commit".into(),
        ),
        (NoteKind::Regular, format!("Unrelated note original {SHA}")),
    ] {
        let bad = Ulid::new();
        ops::add_note(
            &store,
            &t.id,
            bad,
            Timestamp::from_datetime(at()),
            kind,
            text,
        )
        .unwrap();
        assert!(
            c.review_return("i", "generation-1", at(), "original", &bad.to_string())
                .is_err()
        );
    }
    let other = ops::create(
        &store,
        Ulid::new(),
        "HS",
        Timestamp::from_datetime(at()),
        NewTicket {
            title: "synthetic other ticket".into(),
            ..NewTicket::default()
        },
    )
    .unwrap();
    let other_note = Ulid::new();
    ops::add_note(
        &store,
        &other.id,
        other_note,
        Timestamp::from_datetime(at()),
        NoteKind::Regular,
        format!("## Review return\noriginal {SHA}"),
    )
    .unwrap();
    assert!(
        c.review_return(
            "i",
            "generation-1",
            at(),
            "original",
            &other_note.to_string()
        )
        .is_err()
    );
    assert_eq!(c.state().unwrap(), before);
    // Subsecond expiry is authoritative; do not floor authority to the availability clock.
    let expiry = at() + time::Duration::milliseconds(500);
    ops::claim(
        &store,
        &t.id,
        &Timestamp::from_datetime(at()),
        Timestamp::from_datetime(expiry),
        "a",
        None,
    )
    .unwrap();
    assert!(
        c.review_return(
            "i",
            "generation-1",
            expiry - time::Duration::nanoseconds(1),
            "original",
            &note
        )
        .is_err()
    );
    c.review_return("i", "generation-1", expiry, "original", &note)
        .unwrap();
    assert!(
        c.review_return(
            "i",
            "generation-1",
            expiry,
            "original",
            &Ulid::new().to_string()
        )
        .is_err()
    );
    // Replaying the already-durable return does not alter a subsequent worker's claim.
    ops::claim(
        &store,
        &t.id,
        &Timestamp::from_datetime(expiry),
        Timestamp::from_datetime(expiry + time::Duration::minutes(1)),
        "b",
        None,
    )
    .unwrap();
    c.review_return("i", "generation-1", expiry, "original", &note)
        .unwrap();
    assert_eq!(
        store.read_ticket(&t.id).unwrap().claimed_by.as_deref(),
        Some("b")
    );
}

#[test]
fn hs2_65etrj_return_never_cancels_integration_ci_or_an_ordinary_blocker() {
    for phase in [
        Phase::Offered,
        Phase::Accepted,
        Phase::Integrating,
        Phase::Ci,
        Phase::Done,
        Phase::Blocked,
    ] {
        let (_root, c) = fixture();
        let (_store, _ticket, note) = handoff(&c);
        c.change(|state| {
            state.assignments.get_mut("original").unwrap().phase = phase;
            Ok(())
        })
        .unwrap();
        let before = c.state().unwrap();
        assert!(
            c.review_return("i", "generation-1", at(), "original", &note)
                .is_err()
        );
        assert_eq!(c.state().unwrap(), before);
    }
}
