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
    let reopened = Coordinator::open_with_registry_home(
        &root.path().join("project"),
        &c.source,
        root.path().join("home"),
    )
    .unwrap();
    let mut returned = before.clone();
    returned.phase = Phase::Blocked;
    returned.recovery_note = Some(note.clone());
    returned
        .record_transition(TransitionKind::ReviewReturn, at(), Role::Integrator, "i")
        .unwrap();
    assert_eq!(reopened.state().unwrap().version, 3);
    assert_eq!(reopened.state().unwrap().assignments["original"], returned);
    let mut correction = before;
    correction.id = "correction".into();
    correction.phase = Phase::Offered;
    correction.commit = None;
    correction.handoff = None;
    correction.transitions.clear();
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

/// HS2-YADH9C: a review loop that returns the correction too. The original and the
/// first correction stay fenced in Blocked with their own evidence, each return needs
/// a note naming its own assignment and commit, the earlier return still replays
/// after the later one, and only the last correction integrates. No round loses a
/// handoff or lets an earlier one reach integration.
#[test]
fn hs2_yadh9c_a_returned_correction_can_be_returned_again_and_only_the_last_integrates() {
    const SECOND: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const THIRD: &str = "cccccccccccccccccccccccccccccccccccccccc";
    let (root, c) = fixture();
    let (store, t, first_note) = handoff(&c);
    c.review_return("i", "generation-1", at(), "original", &first_note)
        .unwrap();
    let returned_original = c.state().unwrap().assignments["original"].clone();
    let reassign = |id: &str, worker: &str| {
        let mut next = assignment(id, worker, &t.id.to_string());
        next.ticket.connection_id = git_connection_id(&store);
        next.slug = t.slug.clone();
        next
    };

    // Round one: the first correction is handed off with a new commit.
    c.assign("i", "generation-1", NOW, reassign("correction-1", "a"))
        .unwrap();
    c.worker_transition("a", "generation-1", "correction-1", Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        "a",
        "generation-1",
        "correction-1",
        Phase::Handoff,
        Some(SECOND.into()),
    )
    .unwrap();
    // The first note names the original, not this correction: it cannot return it.
    assert!(
        c.review_return("i", "generation-1", at(), "correction-1", &first_note)
            .is_err()
    );
    // Nor can a note that names the correction with the original's commit.
    let stale = Ulid::new();
    ops::add_note(
        &store,
        &t.id,
        stale,
        Timestamp::from_datetime(at()),
        NoteKind::Regular,
        format!("## Review return\ncorrection-1 {SHA}"),
    )
    .unwrap();
    assert!(
        c.review_return(
            "i",
            "generation-1",
            at(),
            "correction-1",
            &stale.to_string()
        )
        .is_err()
    );
    assert_eq!(
        c.state().unwrap().assignments["correction-1"].phase,
        Phase::Handoff
    );
    let second_note = Ulid::new();
    ops::add_note(
        &store,
        &t.id,
        second_note,
        Timestamp::from_datetime(at()),
        NoteKind::Regular,
        format!(
            "## Review return\ncorrection-1 {SECOND}\nPreserve both commits; still incomplete."
        ),
    )
    .unwrap();
    c.review_return(
        "i",
        "generation-1",
        at(),
        "correction-1",
        &second_note.to_string(),
    )
    .unwrap();
    let returned_first = c.state().unwrap().assignments["correction-1"].clone();
    assert_eq!(returned_first.phase, Phase::Blocked);
    assert_eq!(returned_first.commit.as_deref(), Some(SECOND));
    assert_eq!(
        returned_first.recovery_note.as_deref(),
        Some(second_note.to_string().as_str())
    );
    // Both earlier returns replay unchanged after the later one, from a reopened pool.
    let reopened = Coordinator::open_with_registry_home(
        &root.path().join("project"),
        &c.source,
        root.path().join("home"),
    )
    .unwrap();
    let revision = reopened.state().unwrap().revision;
    reopened
        .review_return("i", "generation-1", at(), "original", &first_note)
        .unwrap();
    reopened
        .review_return(
            "i",
            "generation-1",
            at(),
            "correction-1",
            &second_note.to_string(),
        )
        .unwrap();
    assert_eq!(reopened.state().unwrap().revision, revision);
    assert_eq!(
        reopened.state().unwrap().assignments["original"],
        returned_original
    );

    // Round two goes to another worker, who hands off and is integrated.
    register(&reopened, Role::Worker, "b", "generation-1", NOW);
    reopened
        .assign("i", "generation-1", NOW, reassign("correction-2", "b"))
        .unwrap();
    reopened
        .worker_transition("b", "generation-1", "correction-2", Phase::Accepted, None)
        .unwrap();
    reopened
        .worker_transition(
            "b",
            "generation-1",
            "correction-2",
            Phase::Handoff,
            Some(THIRD.into()),
        )
        .unwrap();
    // Neither returned record can be handed off again or integrated.
    for (worker, id, commit) in [("a", "original", SHA), ("a", "correction-1", SECOND)] {
        assert!(
            reopened
                .worker_transition(
                    worker,
                    "generation-1",
                    id,
                    Phase::Handoff,
                    Some(commit.into())
                )
                .is_err()
        );
        for phase in [Phase::Integrating, Phase::Ci, Phase::Done] {
            assert!(
                reopened.mark("i", "generation-1", NOW, id, phase).is_err(),
                "{id} reached {phase:?}"
            );
        }
    }
    for phase in [Phase::Integrating, Phase::Ci, Phase::Done] {
        reopened
            .mark("i", "generation-1", NOW, "correction-2", phase)
            .unwrap();
    }
    let state = reopened.state().unwrap();
    assert_eq!(state.assignments["correction-2"].phase, Phase::Done);
    assert_eq!(
        state.assignments["correction-2"].commit.as_deref(),
        Some(THIRD)
    );
    assert_eq!(state.assignments["original"], returned_original);
    assert_eq!(state.assignments["correction-1"], returned_first);
    // Each record's history names its own return exactly once.
    for id in ["original", "correction-1"] {
        assert_eq!(
            state.assignments[id]
                .transitions
                .iter()
                .filter(|transition| transition.kind == TransitionKind::ReviewReturn)
                .count(),
            1,
            "{id}"
        );
    }
    // Both workers are free for new work: the loop left nothing active.
    assert!(state.active_assignment("a").is_none());
    assert!(state.active_assignment("b").is_none());
}
