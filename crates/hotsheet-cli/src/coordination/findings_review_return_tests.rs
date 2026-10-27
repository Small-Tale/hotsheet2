use super::*;
use crate::coordination::tests::{assignment, fixture, register};
use hotsheet_model::{NoteKind, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::NewTicket;

const NOW: i64 = 1_800_000_000;
fn at() -> time::OffsetDateTime {
    time::OffsetDateTime::from_unix_timestamp(NOW).unwrap()
}
fn handoff(c: &Coordinator) -> (FsStore, Ticket, Ulid, Ulid) {
    let store = FsStore::open(&c.source).unwrap();
    let ticket = ops::create(
        &store,
        Ulid::new(),
        "HS",
        Timestamp::from_datetime(at()),
        NewTicket {
            title: "Synthetic findings task".into(),
            up_next: true,
            ..NewTicket::default()
        },
    )
    .unwrap();
    register(c, Role::Integrator, "i", "generation-1", NOW);
    register(c, Role::Worker, "a", "generation-1", NOW);
    let mut offered = assignment("original", "a", &ticket.id.to_string());
    offered.ticket.connection_id = git_connection_id(&store);
    offered.slug = ticket.slug.clone();
    c.assign("i", "generation-1", NOW, offered).unwrap();
    c.worker_transition("a", "generation-1", "original", Phase::Accepted, None)
        .unwrap();
    let findings = Ulid::new();
    ops::add_note(
        &store,
        &ticket.id,
        findings,
        Timestamp::from_datetime(at()),
        NoteKind::Regular,
        "## Findings handoff\nSynthetic verified result; no code change.".into(),
    )
    .unwrap();
    c.worker_findings_transition("a", "generation-1", "original", findings.to_string())
        .unwrap();
    let review = Ulid::new();
    ops::add_note(&store, &ticket.id, review, Timestamp::from_datetime(at()), NoteKind::Regular,
        format!("## Review return\noriginal {findings}\nClarify synthetic evidence; preserve original findings.")).unwrap();
    (store, ticket, findings, review)
}

#[test]
fn hs2_aebjyn_findings_return_survives_restart_replay_and_correction_completion() {
    for phase in [Phase::Handoff, Phase::Verifying] {
        let (root, c) = fixture();
        let (store, ticket, _findings, review) = handoff(&c);
        if phase == Phase::Verifying {
            c.mark("i", "generation-1", NOW, "original", phase).unwrap();
            ops::claim(
                &store,
                &ticket.id,
                &Timestamp::from_datetime(at()),
                Timestamp::from_datetime(at() + time::Duration::minutes(1)),
                "i",
                None,
            )
            .unwrap();
            assert!(
                c.review_return("i", "generation-1", at(), "original", &review.to_string())
                    .is_err()
            );
            ops::release(
                &store,
                &ticket.id,
                Timestamp::from_datetime(at()),
                "i",
                false,
            )
            .unwrap();
        }
        let original = c.state().unwrap().assignments["original"].clone();
        c.review_return("i", "generation-1", at(), "original", &review.to_string())
            .unwrap();
        let c = Coordinator::open(&root.path().join("project"), store.root()).unwrap();
        let returned = c.state().unwrap().assignments["original"].clone();
        assert!(returned.returned_for_review());
        assert_eq!(returned.handoff, original.handoff);
        assert_eq!(returned.commit, None);
        assert_eq!(returned.recovery_note, Some(review.to_string()));
        let mut correction = original.clone();
        correction.id = "correction".into();
        correction.phase = Phase::Offered;
        correction.handoff = None;
        correction.recovery_note = None;
        c.assign("i", "generation-1", NOW, correction).unwrap();
        ops::claim(
            &store,
            &ticket.id,
            &Timestamp::from_datetime(at()),
            Timestamp::from_datetime(at() + time::Duration::minutes(1)),
            "a",
            None,
        )
        .unwrap();
        let revision = c.state().unwrap().revision;
        c.review_return("i", "generation-1", at(), "original", &review.to_string())
            .unwrap();
        assert_eq!(c.state().unwrap().revision, revision);
        assert_eq!(
            store.read_ticket(&ticket.id).unwrap().claimed_by.as_deref(),
            Some("a")
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
        assert!(
            c.worker_findings_transition(
                "a",
                "generation-1",
                "original",
                match original.handoff.clone().unwrap() {
                    HandoffEvidence::Findings { note_id } => note_id,
                    _ => unreachable!(),
                }
            )
            .is_err()
        );
        c.worker_transition("a", "generation-1", "correction", Phase::Accepted, None)
            .unwrap();
        c.worker_findings_transition("a", "generation-1", "correction", Ulid::new().to_string())
            .unwrap();
        c.mark("i", "generation-1", NOW, "correction", Phase::Verifying)
            .unwrap();
        c.mark("i", "generation-1", NOW, "correction", Phase::Done)
            .unwrap();
        for phase in [Phase::Verifying, Phase::Integrating, Phase::Ci, Phase::Done] {
            assert!(c.mark("i", "generation-1", NOW, "original", phase).is_err());
        }
        assert_eq!(c.state().unwrap().assignments["original"], returned);
    }
}

#[test]
fn hs2_aebjyn_findings_return_rejects_wrong_evidence_source_session_and_live_claims() {
    let (_root, c) = fixture();
    let (store, ticket, findings, review) = handoff(&c);
    let before = c.state().unwrap();
    assert!(
        c.review_return("i", "stale", at(), "original", &review.to_string())
            .is_err()
    );
    assert!(
        c.review_return(
            "i",
            "generation-1",
            at() + time::Duration::seconds(121),
            "original",
            &review.to_string()
        )
        .is_err()
    );
    for (kind, text) in [
        (
            NoteKind::Activity,
            format!("## Review return\noriginal {findings}"),
        ),
        (
            NoteKind::Regular,
            format!("## Review return\nother {findings}"),
        ),
        (
            NoteKind::Regular,
            "## Review return\noriginal wrong-note".into(),
        ),
        (NoteKind::Regular, format!("Unrelated original {findings}")),
        (NoteKind::Regular, "## Review return".into()),
    ] {
        let bad = Ulid::new();
        ops::add_note(
            &store,
            &ticket.id,
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
            title: "Synthetic other ticket".into(),
            ..NewTicket::default()
        },
    )
    .unwrap();
    let bad = Ulid::new();
    ops::add_note(
        &store,
        &other.id,
        bad,
        Timestamp::from_datetime(at()),
        NoteKind::Regular,
        format!("## Review return\noriginal {findings}"),
    )
    .unwrap();
    assert!(
        c.review_return("i", "generation-1", at(), "original", &bad.to_string())
            .is_err()
    );
    for (kind, text) in [
        (NoteKind::Activity, "## Findings handoff\nSynthetic result"),
        (NoteKind::Regular, "## Findings handoff"),
        (NoteKind::Regular, "Wrong heading\nSynthetic result"),
    ] {
        let mut t = store.read_ticket(&ticket.id).unwrap();
        let n = t.notes.iter_mut().find(|n| n.id == findings).unwrap();
        n.kind = kind;
        n.text = text.into();
        store.write_ticket(&t).unwrap();
        assert!(
            c.review_return("i", "generation-1", at(), "original", &review.to_string())
                .is_err()
        );
    }
    let mut t = store.read_ticket(&ticket.id).unwrap();
    let n = t.notes.iter_mut().find(|n| n.id == findings).unwrap();
    n.kind = NoteKind::Regular;
    n.text = "## Findings handoff\nSynthetic result".into();
    store.write_ticket(&t).unwrap();
    assert_eq!(c.state().unwrap(), before);
    c.change(|s| {
        s.assignments
            .get_mut("original")
            .unwrap()
            .ticket
            .connection_id = "other-source".into();
        Ok(())
    })
    .unwrap();
    assert!(
        c.review_return("i", "generation-1", at(), "original", &review.to_string())
            .is_err()
    );
    c.change(|s| {
        s.assignments
            .get_mut("original")
            .unwrap()
            .ticket
            .connection_id = git_connection_id(&store);
        Ok(())
    })
    .unwrap();
    let expiry = at() + time::Duration::milliseconds(500);
    ops::claim(
        &store,
        &ticket.id,
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
            &review.to_string()
        )
        .is_err()
    );
    c.review_return("i", "generation-1", expiry, "original", &review.to_string())
        .unwrap();
}

#[test]
fn hs2_aebjyn_verifying_requires_release_even_after_claim_expiry_and_other_phases_stay_fenced() {
    for phase in [
        Phase::Offered,
        Phase::Accepted,
        Phase::Integrating,
        Phase::Ci,
        Phase::Done,
        Phase::Blocked,
    ] {
        let (_root, c) = fixture();
        let (_store, _ticket, _findings, review) = handoff(&c);
        c.change(|s| {
            s.assignments.get_mut("original").unwrap().phase = phase;
            Ok(())
        })
        .unwrap();
        let before = c.state().unwrap();
        assert!(
            c.review_return("i", "generation-1", at(), "original", &review.to_string())
                .is_err()
        );
        assert_eq!(c.state().unwrap(), before);
    }
    let (_root, c) = fixture();
    let (store, t, _findings, review) = handoff(&c);
    c.mark("i", "generation-1", NOW, "original", Phase::Verifying)
        .unwrap();
    let expiry = at() + time::Duration::milliseconds(500);
    ops::claim(
        &store,
        &t.id,
        &Timestamp::from_datetime(at()),
        Timestamp::from_datetime(expiry),
        "i",
        None,
    )
    .unwrap();
    assert!(
        c.review_return("i", "generation-1", expiry, "original", &review.to_string())
            .is_err()
    );
    ops::release(&store, &t.id, Timestamp::from_datetime(expiry), "i", false).unwrap();
    c.review_return("i", "generation-1", expiry, "original", &review.to_string())
        .unwrap();
}

#[test]
fn hs2_aebjyn_schema_two_migration_preserves_findings_code_ci_and_returned_evidence() {
    let (root, c) = fixture();
    let (_store, _ticket, _findings, review) = handoff(&c);
    register(&c, Role::Worker, "b", "generation-1", NOW);
    let code = assignment("code", "b", "synthetic-code-ticket");
    c.assign("i", "generation-1", NOW, code).unwrap();
    c.worker_transition("b", "generation-1", "code", Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        "b",
        "generation-1",
        "code",
        Phase::Handoff,
        Some("a".repeat(40)),
    )
    .unwrap();
    c.mark("i", "generation-1", NOW, "code", Phase::Integrating)
        .unwrap();
    c.mark("i", "generation-1", NOW, "code", Phase::Ci).unwrap();
    c.review_return("i", "generation-1", at(), "original", &review.to_string())
        .unwrap();
    let before = c.state().unwrap();
    let path = c.directory.join("state.json");
    let mut legacy = before.clone();
    legacy.version = 2;
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let c = Coordinator::open(&root.path().join("project"), &c.source).unwrap();
    let migrated = c.state().unwrap();
    assert_eq!(migrated.version, 3);
    assert_eq!(migrated.assignments, before.assignments);
    assert_eq!(migrated.workers, before.workers);
    assert_eq!(migrated.integrator, before.integrator);
    assert_eq!(migrated.revision, before.revision + 1);
    assert!(
        c.mark("i", "generation-1", NOW, "original", Phase::Done)
            .is_err()
    );
    assert_eq!(c.state().unwrap().assignments["code"].phase, Phase::Ci);
    // Unknown versions fail without rewriting the pool; the prior 1/2 runtimes likewise
    // reject this pool's new version instead of applying their old Blocked→Done semantics.
    // Schema four is dispatcher authority (HS2-TWZW7X) and five reviewed shared sections
    // (HS2-YHYNJ6), so six is the next unknown.
    let mut future = migrated;
    future.version = 6;
    let bytes = serde_json::to_vec(&future).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(Coordinator::open(&root.path().join("project"), &c.source).is_err());
    assert_eq!(fs::read(path).unwrap(), bytes);
}
