use super::*;
use hotsheet_model::{Timestamp, Ulid};
use hotsheet_ticketing::{NewTicket, StoreMetadata};

pub(super) const NOW: i64 = 1_800_000_000;
pub(super) const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub(super) fn fixture() -> (tempfile::TempDir, Coordinator) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    assert!(
        hotsheet_ticketing::git::launch("git")
            .args(["init", "-q"])
            .arg(&project)
            .status()
            .unwrap()
            .success()
    );
    let store = root.path().join("store");
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
    let coordinator = Coordinator::open(&project, &store).unwrap();
    (root, coordinator)
}

pub(super) fn register(c: &Coordinator, role: Role, id: &str, generation: &str, now: i64) {
    c.register(role, id, generation, now, 120, ReturnRoute::HumanRelay)
        .unwrap();
}

pub(super) fn assignment(id: &str, worker: &str, native: &str) -> Assignment {
    Assignment {
        id: id.into(),
        worker: worker.into(),
        generation: "generation-1".into(),
        ticket: TicketRef {
            connection_id: "source-a".into(),
            native_id: native.into(),
        },
        slug: "HS-1".into(),
        base: SHA.into(),
        phase: Phase::Offered,
        commit: None,
        handoff: None,
        return_route: ReturnRoute::HumanRelay,
        recovery_note: None,
        scope: Vec::new(),
        transitions: Vec::new(),
    }
}

#[test]
fn hs2_4273qb_assignment_history_survives_replay_restart_and_reversed_transitions() {
    let (root, c) = fixture();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    register(&c, Role::Integrator, "i", "generation-1", now);
    register(&c, Role::Worker, "a", "generation-1", now);
    let input = assignment("operation", "a", "synthetic");
    let offered = c.assign("i", "generation-1", now, input.clone()).unwrap();
    assert_eq!(offered.transitions[0].kind, TransitionKind::Offered);
    assert_eq!(
        offered.transitions[0].actor.as_ref().unwrap().role,
        Role::Integrator
    );
    assert_eq!(offered.transitions[0].actor.as_ref().unwrap().id, "i");
    assert_eq!(c.assign("i", "generation-1", now, input).unwrap(), offered);

    c.worker_transition("a", "generation-1", "operation", Phase::Accepted, None)
        .unwrap();
    c.worker_transition("a", "generation-1", "operation", Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        "a",
        "generation-1",
        "operation",
        Phase::Handoff,
        Some(SHA.into()),
    )
    .unwrap();
    c.mark(
        "i",
        "generation-1",
        now + 1,
        "operation",
        Phase::Integrating,
    )
    .unwrap();
    c.mark("i", "generation-1", now + 2, "operation", Phase::Ci)
        .unwrap();
    c.mark(
        "i",
        "generation-1",
        now + 3,
        "operation",
        Phase::Integrating,
    )
    .unwrap();
    c.mark("i", "generation-1", now + 4, "operation", Phase::Ci)
        .unwrap();
    c.mark("i", "generation-1", now + 4, "operation", Phase::Ci)
        .unwrap();
    c.mark("i", "generation-1", now + 5, "operation", Phase::Done)
        .unwrap();

    let reopened = Coordinator::open(&root.path().join("project"), &c.source).unwrap();
    let history = &reopened.snapshot(now + 5).unwrap().state.assignments["operation"].transitions;
    assert_eq!(
        history.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            TransitionKind::Offered,
            TransitionKind::Accepted,
            TransitionKind::Handoff,
            TransitionKind::Integrating,
            TransitionKind::Ci,
            TransitionKind::Integrating,
            TransitionKind::Ci,
            TransitionKind::Done,
        ]
    );
    for event in history {
        time::OffsetDateTime::parse(&event.at, &time::format_description::well_known::Rfc3339)
            .unwrap();
        assert!(!event.actor.as_ref().unwrap().id.is_empty());
    }
    assert_eq!(history[1].actor.as_ref().unwrap().role, Role::Worker);
    assert_eq!(history[3].actor.as_ref().unwrap().role, Role::Integrator);

    let mut legacy =
        serde_json::to_value(&reopened.state().unwrap().assignments["operation"]).unwrap();
    legacy.as_object_mut().unwrap().remove("transitions");
    let legacy: Assignment = serde_json::from_value(legacy).unwrap();
    assert!(legacy.transitions.is_empty());
}

#[test]
fn hs2_76vmyg_startup_orders_reconnect_and_idle_renewal_preserve_state() {
    for worker_first in [true, false] {
        let (_root, c) = fixture();
        let roles = if worker_first {
            [Role::Worker, Role::Integrator]
        } else {
            [Role::Integrator, Role::Worker]
        };
        for role in roles {
            register(
                &c,
                role,
                if role == Role::Worker {
                    "worker"
                } else {
                    "integrator"
                },
                "generation-1",
                NOW,
            );
        }
        let first = c.snapshot(NOW).unwrap();
        assert_eq!(first.live_workers, ["worker"]);
        assert!(first.live_integrator);
        register(&c, Role::Worker, "worker", "generation-1", NOW + 30);
        assert_eq!(
            first.cursor,
            c.snapshot(NOW + 30).unwrap().cursor,
            "lease heartbeats must not masquerade as queue events"
        );
        let reopened =
            Coordinator::open(c.directory.parent().unwrap().parent().unwrap(), &c.source).unwrap();
        assert_eq!(reopened.state().unwrap(), c.state().unwrap());
    }
}

#[test]
fn hs2_76vmyg_duplicate_assignment_and_old_session_cannot_take_other_work() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "i", "generation-1", NOW);
    for worker in ["a", "b"] {
        register(&c, Role::Worker, worker, "generation-1", NOW);
    }
    let a = assignment("operation-1", "a", "opaque/native-id");
    let offered = c.assign("i", "generation-1", NOW, a.clone()).unwrap();
    assert_eq!(
        c.assign("i", "generation-1", NOW, a.clone()).unwrap(),
        offered
    );
    assert_eq!(offered.transitions.len(), 1);
    assert!(
        c.assign(
            "i",
            "generation-1",
            NOW,
            assignment("operation-2", "b", "opaque/native-id")
        )
        .is_err()
    );
    assert!(
        c.assign(
            "i",
            "generation-1",
            NOW,
            assignment("operation-3", "a", "other-id")
        )
        .is_err()
    );
    assert!(
        c.assign(
            "i",
            "generation-1",
            NOW,
            assignment("operation-1", "b", "other-id")
        )
        .is_err()
    );
    assert!(
        c.worker_transition("a", "old-generation", &a.id, Phase::Accepted, None)
            .is_err()
    );
    c.worker_transition("a", "generation-1", &a.id, Phase::Accepted, None)
        .unwrap();
    assert_eq!(
        c.assign("i", "generation-1", NOW, a).unwrap().phase,
        Phase::Accepted
    );
    assert!(
        c.register(
            Role::Worker,
            "a",
            "replacement",
            NOW + 200,
            120,
            ReturnRoute::HumanRelay
        )
        .is_err()
    );
    assert_eq!(
        c.state().unwrap().active_assignment("a").unwrap().phase,
        Phase::Accepted
    );
}

#[test]
fn hs2_76vmyg_worker_refills_while_handoff_or_ci_waits_for_integrator() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "i", "generation-1", NOW);
    register(&c, Role::Worker, "a", "generation-1", NOW);
    c.assign("i", "generation-1", NOW, assignment("one", "a", "1"))
        .unwrap();
    assert!(
        c.worker_transition("a", "generation-1", "one", Phase::Handoff, Some(SHA.into()))
            .is_err()
    );
    c.worker_transition("a", "generation-1", "one", Phase::Accepted, None)
        .unwrap();
    c.worker_transition("a", "generation-1", "one", Phase::Handoff, Some(SHA.into()))
        .unwrap();
    c.worker_transition("a", "generation-1", "one", Phase::Handoff, Some(SHA.into()))
        .unwrap();
    assert_eq!(c.snapshot(NOW).unwrap().live_workers, ["a"]);
    c.assign("i", "generation-1", NOW, assignment("two", "a", "2"))
        .unwrap();
    assert!(
        c.mark("i", "generation-1", NOW, "one", Phase::Done)
            .is_err()
    );
    c.mark("i", "generation-1", NOW, "one", Phase::Integrating)
        .unwrap();
    c.mark("i", "generation-1", NOW, "one", Phase::Ci).unwrap();
    assert!(c.finish("i", "generation-1", NOW, "drained").is_err());
    c.leave(Role::Integrator, "i", "generation-1").unwrap();
    register(&c, Role::Integrator, "i2", "generation-2", NOW);
    assert_eq!(c.state().unwrap().assignments["one"].phase, Phase::Ci);
    assert!(
        c.mark("i", "generation-1", NOW, "one", Phase::Done)
            .is_err()
    );
    c.mark("i2", "generation-2", NOW, "one", Phase::Integrating)
        .unwrap();
    c.mark("i2", "generation-2", NOW, "one", Phase::Ci).unwrap();
    c.mark("i2", "generation-2", NOW, "one", Phase::Done)
        .unwrap();
    c.worker_transition("a", "generation-1", "two", Phase::Blocked, None)
        .unwrap();
    let blocked = c.state().unwrap().assignments["two"].clone();
    assert_eq!(
        blocked.transitions.last().unwrap().kind,
        TransitionKind::Blocked
    );
    assert_eq!(
        blocked
            .transitions
            .last()
            .unwrap()
            .actor
            .as_ref()
            .unwrap()
            .role,
        Role::Worker
    );
    assert!(c.state().unwrap().active_assignment("a").is_none());
    c.finish("i2", "generation-2", NOW, "blocked").unwrap();
    assert_eq!(c.state().unwrap().stopped.as_deref(), Some("blocked"));
    c.resume().unwrap();
    c.assign("i2", "generation-2", NOW, assignment("three", "a", "3"))
        .unwrap();
}

#[test]
fn hs2_a1f615_findings_handoff_is_fenced_and_never_enters_code_ci() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "i", "generation-1", NOW);
    register(&c, Role::Worker, "a", "generation-1", NOW);
    c.assign("i", "generation-1", NOW, assignment("findings", "a", "1"))
        .unwrap();
    c.worker_transition("a", "generation-1", "findings", Phase::Accepted, None)
        .unwrap();
    let note_id = Ulid::new().to_string();
    assert!(
        c.worker_findings_transition("a", "stale-generation", "findings", note_id.clone())
            .is_err()
    );
    assert!(
        c.worker_findings_transition("a", "generation-1", "findings", "invalid".into())
            .is_err()
    );
    c.worker_findings_transition("a", "generation-1", "findings", note_id.clone())
        .unwrap();
    c.worker_findings_transition("a", "generation-1", "findings", note_id.clone())
        .unwrap();
    let reopened =
        Coordinator::open(c.directory.parent().unwrap().parent().unwrap(), &c.source).unwrap();
    let evidence = &reopened.state().unwrap().assignments["findings"];
    assert_eq!(evidence.commit, None);
    assert_eq!(
        evidence.handoff,
        Some(HandoffEvidence::Findings { note_id })
    );
    assert!(
        c.mark("i", "generation-1", NOW, "findings", Phase::Integrating)
            .is_err()
    );
    assert!(
        c.mark("i", "generation-1", NOW, "findings", Phase::Ci)
            .is_err()
    );
    c.mark("i", "generation-1", NOW, "findings", Phase::Verifying)
        .unwrap();
    assert!(
        c.mark("i", "generation-1", NOW, "findings", Phase::Ci)
            .is_err()
    );
    c.mark("i", "generation-1", NOW, "findings", Phase::Done)
        .unwrap();

    c.assign("i", "generation-1", NOW, assignment("code", "a", "2"))
        .unwrap();
    c.worker_transition("a", "generation-1", "code", Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        "a",
        "generation-1",
        "code",
        Phase::Handoff,
        Some(SHA.into()),
    )
    .unwrap();
    assert!(
        c.mark("i", "generation-1", NOW, "code", Phase::Verifying)
            .is_err()
    );
    c.mark("i", "generation-1", NOW, "code", Phase::Integrating)
        .unwrap();
    c.mark("i", "generation-1", NOW, "code", Phase::Ci).unwrap();
    assert_eq!(
        c.state().unwrap().assignments["code"].commit.as_deref(),
        Some(SHA)
    );
}

#[test]
fn hs2_a1f615_version_one_code_records_migrate_without_losing_ci() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "i", "generation-1", NOW);
    register(&c, Role::Worker, "a", "generation-1", NOW);
    c.assign("i", "generation-1", NOW, assignment("old-code", "a", "1"))
        .unwrap();
    c.worker_transition("a", "generation-1", "old-code", Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        "a",
        "generation-1",
        "old-code",
        Phase::Handoff,
        Some(SHA.into()),
    )
    .unwrap();
    c.mark("i", "generation-1", NOW, "old-code", Phase::Integrating)
        .unwrap();
    c.mark("i", "generation-1", NOW, "old-code", Phase::Ci)
        .unwrap();
    let path = c.directory.join("state.json");
    let mut old = serde_json::to_value(c.state().unwrap()).unwrap();
    old["version"] = serde_json::json!(1);
    old["assignments"]["old-code"]
        .as_object_mut()
        .unwrap()
        .remove("handoff");
    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let reopened =
        Coordinator::open(c.directory.parent().unwrap().parent().unwrap(), &c.source).unwrap();
    let state = reopened.state().unwrap();
    assert_eq!(state.version, 3);
    assert_eq!(state.assignments["old-code"].commit.as_deref(), Some(SHA));
    assert_eq!(
        state.assignments["old-code"].handoff,
        Some(HandoffEvidence::Code { commit: SHA.into() })
    );
    reopened
        .mark("i", "generation-1", NOW, "old-code", Phase::Integrating)
        .unwrap();
    reopened
        .mark("i", "generation-1", NOW, "old-code", Phase::Ci)
        .unwrap();
    reopened
        .mark("i", "generation-1", NOW, "old-code", Phase::Done)
        .unwrap();
}

#[test]
fn hs2_76vmyg_expired_claim_or_availability_never_reassigns_existing_work() {
    let (_root, c) = fixture();
    let store = FsStore::open(&c.source).unwrap();
    let at = |seconds| {
        Timestamp::from_datetime(time::OffsetDateTime::from_unix_timestamp(seconds).unwrap())
    };
    let t = ops::create(
        &store,
        Ulid::new(),
        "HS",
        at(NOW),
        NewTicket {
            title: "synthetic".into(),
            up_next: true,
            ..NewTicket::default()
        },
    )
    .unwrap();
    ops::claim(&store, &t.id, &at(NOW), at(NOW + 20), "a", None).unwrap();
    register(&c, Role::Integrator, "i", "generation-1", NOW);
    register(&c, Role::Worker, "a", "generation-1", NOW);
    c.assign(
        "i",
        "generation-1",
        NOW,
        assignment("one", "a", &t.id.to_string()),
    )
    .unwrap();
    c.worker_transition("a", "generation-1", "one", Phase::Accepted, None)
        .unwrap();
    assert_eq!(
        c.snapshot(NOW).unwrap().local_queue.unwrap()[0]
            .claimed_by
            .as_deref(),
        Some("a")
    );
    assert!(
        c.snapshot(NOW + 21).unwrap().local_queue.unwrap()[0]
            .claimed_by
            .is_none()
    );
    assert!(c.snapshot(NOW + 200).unwrap().live_workers.is_empty());
    register(&c, Role::Integrator, "i2", "generation-2", NOW + 200);
    register(&c, Role::Worker, "b", "generation-1", NOW + 200);
    assert!(
        c.assign(
            "i2",
            "generation-2",
            NOW + 200,
            assignment("two", "b", &t.id.to_string())
        )
        .is_err()
    );
    assert!(
        c.finish("i2", "generation-2", NOW + 200, "drained")
            .is_err()
    );
    assert_eq!(c.state().unwrap().assignments["one"].worker, "a");
}

#[test]
fn hs2_76vmyg_qualified_refs_and_unknown_provider_queue_do_not_imply_empty() {
    let (root, c) = fixture();
    assert!(
        Coordinator::open(&root.path().join("project"), &root.path().join("project")).is_err(),
        "the same project cannot silently switch source scopes"
    );
    register(&c, Role::Integrator, "i", "generation-1", NOW);
    for worker in ["a", "b"] {
        register(&c, Role::Worker, worker, "generation-1", NOW);
    }
    c.assign("i", "generation-1", NOW, assignment("one", "a", "123"))
        .unwrap();
    let mut other = assignment("two", "b", "123");
    other.ticket.connection_id = "source-b".into();
    c.assign("i", "generation-1", NOW, other).unwrap();
    assert!(
        c.register(
            Role::Worker,
            "a",
            "generation-1",
            NOW,
            0,
            ReturnRoute::HumanRelay
        )
        .is_err()
    );
    assert!("terminal:".parse::<ReturnRoute>().is_err());
    assert!("token:secret".parse::<ReturnRoute>().is_err());
    assert!("terminal:connection-id".parse::<ReturnRoute>().is_ok());
}

#[test]
fn hs2_76vmyg_explicit_recovery_and_serial_integration_preserve_old_artifacts() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "i", "generation-1", NOW);
    for worker in ["a", "b"] {
        register(&c, Role::Worker, worker, "generation-1", NOW);
    }
    c.assign("i", "generation-1", NOW, assignment("one", "a", "1"))
        .unwrap();
    let evidence = Ulid::new().to_string();
    assert!(
        c.recover("i", "generation-1", NOW, "one", &evidence)
            .is_err()
    );
    register(&c, Role::Integrator, "i", "generation-1", NOW + 200);
    c.recover("i", "generation-1", NOW + 200, "one", &evidence)
        .unwrap();
    let recovered = c.state().unwrap().assignments["one"].clone();
    assert_eq!(
        recovered.transitions.last().unwrap().kind,
        TransitionKind::Blocked
    );
    assert_eq!(
        recovered
            .transitions
            .last()
            .unwrap()
            .actor
            .as_ref()
            .unwrap()
            .role,
        Role::Integrator
    );
    assert_eq!(
        c.state().unwrap().assignments["one"]
            .recovery_note
            .as_deref(),
        Some(evidence.as_str())
    );
    c.mark("i", "generation-1", NOW + 200, "one", Phase::Done)
        .unwrap();
    register(&c, Role::Worker, "a", "replacement", NOW + 200);
    register(&c, Role::Worker, "b", "generation-1", NOW + 200);
    let mut replacement = assignment("two", "a", "1");
    replacement.generation = "replacement".into();
    c.assign("i", "generation-1", NOW + 200, replacement)
        .unwrap();
    c.assign(
        "i",
        "generation-1",
        NOW + 200,
        assignment("three", "b", "2"),
    )
    .unwrap();
    for (id, worker, generation) in [("two", "a", "replacement"), ("three", "b", "generation-1")] {
        c.worker_transition(worker, generation, id, Phase::Accepted, None)
            .unwrap();
        c.worker_transition(worker, generation, id, Phase::Handoff, Some(SHA.into()))
            .unwrap();
    }
    c.mark("i", "generation-1", NOW + 200, "two", Phase::Integrating)
        .unwrap();
    assert!(
        c.mark("i", "generation-1", NOW + 200, "three", Phase::Integrating)
            .is_err()
    );
    c.mark("i", "generation-1", NOW + 200, "two", Phase::Ci)
        .unwrap();
    c.mark("i", "generation-1", NOW + 200, "three", Phase::Integrating)
        .unwrap();
    assert!(
        c.worker_transition("a", "generation-1", "two", Phase::Accepted, None)
            .is_err()
    );
}

#[test]
fn hs2_76vmyg_additional_enabled_sources_prevent_false_drained() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "i", "generation-1", NOW);
    let registry = hotsheet_ticketing::ProviderConfigRegistry::new(c.source.join("providers.json"));
    let mut source = hotsheet_ticketing::ProviderConnection {
        id: "other-source".into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: None,
        default: false,
        settings: serde_json::json!({}),
        disabled: false,
    };
    registry.save(&[source.clone()]).unwrap();
    let checkouts =
        checkouts::CheckoutRegistry::new(hotsheet_plugins::hotsheet_home().join("checkouts.json"));
    let project = c.directory.parent().unwrap().parent().unwrap();
    checkouts
        .register_sources(
            project,
            None,
            None,
            vec![
                checkouts::TicketSource::git(&c.source),
                checkouts::TicketSource {
                    connection_id: source.id.clone(),
                    provider: source.provider.clone(),
                    locator: source.locator.clone(),
                },
            ],
            None,
        )
        .unwrap();
    assert_eq!(c.snapshot(NOW).unwrap().uncovered_sources, ["other-source"]);
    assert!(c.finish("i", "generation-1", NOW, "drained").is_err());
    source.disabled = true;
    registry.save(&[source]).unwrap();
    assert!(
        c.snapshot(NOW).unwrap().uncovered_sources.is_empty(),
        "a disabled linked provider must not be re-added from checkout sources"
    );
    c.finish("i", "generation-1", NOW, "drained").unwrap();
}

#[test]
fn hs2_76vmyg_live_claim_outside_up_next_is_neither_idle_nor_drained() {
    let (_root, c) = fixture();
    let store = FsStore::open(&c.source).unwrap();
    let at = |seconds| {
        Timestamp::from_datetime(time::OffsetDateTime::from_unix_timestamp(seconds).unwrap())
    };
    let t = ops::create(
        &store,
        Ulid::new(),
        "HS",
        at(NOW),
        NewTicket {
            title: "synthetic".into(),
            ..NewTicket::default()
        },
    )
    .unwrap();
    ops::claim(&store, &t.id, &at(NOW), at(NOW + 30), "a", None).unwrap();
    register(&c, Role::Integrator, "i", "generation-1", NOW);
    register(&c, Role::Worker, "a", "generation-1", NOW);
    let snapshot = c.snapshot(NOW).unwrap();
    assert_eq!(
        wait::claim_deadline(Role::Integrator, &snapshot, NOW),
        Some(NOW + 30)
    );
    assert_eq!(wait::claim_deadline(Role::Worker, &snapshot, NOW), None);
    assert_eq!(
        wait::claim_deadline(Role::Integrator, &snapshot, NOW + 30),
        None
    );
    assert!(snapshot.local_queue.unwrap().is_empty());
    assert!(snapshot.live_workers.is_empty());
    assert_eq!(snapshot.live_claims.len(), 1);
    assert!(c.finish("i", "generation-1", NOW, "drained").is_err());
    ops::release(&store, &t.id, at(NOW), "a", false).unwrap();
    assert_eq!(c.snapshot(NOW).unwrap().live_workers, ["a"]);
    c.finish("i", "generation-1", NOW, "drained").unwrap();
}
