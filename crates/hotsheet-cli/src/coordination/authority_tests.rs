//! Assignment-authority transitions (HS2-TWZW7X).

use std::collections::BTreeSet;

use super::scope::{normalized_scope, scopes_overlap};
use super::tests::{NOW, SHA, assignment, fixture, register};
use super::*;
use contract::WaitRequest;

const OTHER_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn scoped(id: &str, worker: &str, native: &str, scope: &[&str]) -> Assignment {
    let mut a = assignment(id, worker, native);
    a.generation = format!("{worker}-generation");
    a.scope = scope.iter().map(|path| (*path).to_owned()).collect();
    a
}

fn worker(c: &Coordinator, id: &str, now: i64) {
    register(c, Role::Worker, id, &format!("{id}-generation"), now);
}

/// Integrator `root` (generation `root-1`) enables dispatcher authority and publishes `SHA`.
fn dispatcher_pool() -> (tempfile::TempDir, Coordinator) {
    let (root, c) = fixture();
    register(&c, Role::Integrator, "root", "root-1", NOW);
    c.set_authority("root", "root-1", NOW, Authority::Dispatcher, true)
        .unwrap();
    register(&c, Role::Dispatcher, "dispatcher", "dispatch-1", NOW);
    c.publish("root", "root-1", &|| NOW, SHA, Vec::new(), &[], &[], false)
        .unwrap();
    (root, c)
}

fn error(result: Result<impl std::fmt::Debug>) -> String {
    result.unwrap_err().to_string()
}

fn keys(value: &serde_json::Value) -> BTreeSet<String> {
    value.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn hs2_4273qb_legacy_pool_keeps_schema_three_with_optional_history() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "root", "root-1", NOW);
    worker(&c, "worker", NOW);
    c.assign("root", "root-1", NOW, scoped("op", "worker", "1", &[]))
        .unwrap();
    let raw: serde_json::Value =
        serde_json::from_slice(&fs::read(c.directory.join("state.json")).unwrap()).unwrap();
    assert_eq!(raw["version"], 3);
    // HS2-4273QB adds only optional history inside an assignment; the pool schema
    // and top-level field set stay compatible with legacy readers.
    assert_eq!(
        keys(&raw),
        [
            "version",
            "project_id",
            "source_id",
            "revision",
            "integrator",
            "workers",
            "assignments",
            "stopped"
        ]
        .map(String::from)
        .into()
    );
    assert_eq!(
        keys(&raw["assignments"]["op"]),
        [
            "id",
            "worker",
            "generation",
            "ticket",
            "slug",
            "base",
            "phase",
            "commit",
            "handoff",
            "return_route",
            "recovery_note",
            "transitions"
        ]
        .map(String::from)
        .into()
    );
}

#[test]
fn hs2_twzw7x_legacy_authority_refuses_every_dispatcher_feature() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "root", "root-1", NOW);
    worker(&c, "worker", NOW);
    assert!(
        error(c.register(
            Role::Dispatcher,
            "dispatcher",
            "d",
            NOW,
            120,
            ReturnRoute::HumanRelay
        ))
        .contains("assignment authority is the integrator")
    );
    assert!(
        error(c.publish("root", "root-1", &|| NOW, SHA, Vec::new(), &[], &[], false))
            .contains("require dispatcher authority")
    );
    assert!(
        error(c.assign("root", "root-1", NOW, scoped("op", "worker", "1", &["src"])))
            .contains("scopes require dispatcher authority")
    );
    // A dispatcher identity never acts as the integrator, and no state changed.
    assert!(
        error(c.assign("dispatcher", "d", NOW, scoped("op", "worker", "1", &[])))
            .contains("exact integrator")
    );
    assert_eq!(c.state().unwrap().version, 3);
    assert!(c.state().unwrap().assignments.is_empty());
}

#[test]
fn hs2_twzw7x_only_the_live_integrator_enables_dispatcher_authority_explicitly() {
    let (_root, c) = fixture();
    assert!(
        error(c.set_authority("root", "root-1", NOW, Authority::Dispatcher, true))
            .contains("exact integrator")
    );
    register(&c, Role::Integrator, "root", "root-1", NOW);
    assert!(
        error(c.set_authority("root", "root-1", NOW, Authority::Dispatcher, false))
            .contains("--all-participants-upgraded")
    );
    assert!(
        error(c.set_authority("root", "stale", NOW, Authority::Dispatcher, true))
            .contains("exact integrator")
    );
    assert_eq!(c.state().unwrap().version, 3);
    c.set_authority("root", "root-1", NOW, Authority::Dispatcher, true)
        .unwrap();
    let state = c.state().unwrap();
    assert_eq!((state.version, state.authority), (4, Authority::Dispatcher));
    // Replaying the switch is idempotent and no longer needs the confirmation.
    c.set_authority("root", "root-1", NOW, Authority::Dispatcher, false)
        .unwrap();
}

#[test]
fn hs2_twzw7x_dispatcher_authority_excludes_the_integrator_and_stale_dispatchers() {
    let (_root, c) = dispatcher_pool();
    worker(&c, "worker", NOW);
    assert!(
        error(c.assign("root", "root-1", NOW, scoped("op", "worker", "1", &["a"])))
            .contains("assignment authority is the dispatcher")
    );
    // One live dispatcher generation; another waits for expiry or leave.
    assert!(
        error(c.register(
            Role::Dispatcher,
            "dispatcher",
            "dispatch-2",
            NOW + 1,
            120,
            ReturnRoute::HumanRelay
        ))
        .contains("another live session")
    );
    assert!(
        error(c.register(
            Role::Dispatcher,
            "other",
            "dispatch-3",
            NOW + 1,
            120,
            ReturnRoute::HumanRelay
        ))
        .contains("another live session")
    );
    let first = c
        .assign(
            "dispatcher",
            "dispatch-1",
            NOW,
            scoped("op", "worker", "1", &["a"]),
        )
        .unwrap();
    // Lost delivery replays the same reservation; a changed scope is a different operation.
    assert_eq!(
        c.assign(
            "dispatcher",
            "dispatch-1",
            NOW,
            scoped("op", "worker", "1", &["a"])
        )
        .unwrap(),
        first
    );
    assert!(
        error(c.assign(
            "dispatcher",
            "dispatch-1",
            NOW,
            scoped("op", "worker", "1", &["b"])
        ))
        .contains("different assignment")
    );
    // Takeover after expiry fences the old generation, including its replays.
    register(&c, Role::Dispatcher, "dispatcher", "dispatch-2", NOW + 200);
    assert!(
        error(c.assign(
            "dispatcher",
            "dispatch-1",
            NOW + 200,
            scoped("op", "worker", "1", &["a"])
        ))
        .contains("exact dispatcher")
    );
    c.leave(Role::Dispatcher, "dispatcher", "dispatch-1")
        .unwrap();
    assert_eq!(
        c.state().unwrap().dispatcher.unwrap().generation,
        "dispatch-2"
    );
    c.leave(Role::Dispatcher, "dispatcher", "dispatch-2")
        .unwrap();
    assert!(c.state().unwrap().dispatcher.is_none());
}

#[test]
fn hs2_twzw7x_dispatcher_reads_the_published_base_blockers_and_scopes() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "root", "root-1", NOW);
    c.set_authority("root", "root-1", NOW, Authority::Dispatcher, true)
        .unwrap();
    register(&c, Role::Dispatcher, "dispatcher", "dispatch-1", NOW);
    for id in ["one", "two", "three"] {
        worker(&c, id, NOW);
    }
    let assign = |a: Assignment| c.assign("dispatcher", "dispatch-1", NOW, a);
    assert!(error(assign(scoped("op-1", "one", "1", &["src"]))).contains("not published"));
    let blocked = TicketRef {
        connection_id: "source-a".into(),
        native_id: "9".into(),
    };
    c.publish(
        "root",
        "root-1",
        &|| NOW,
        SHA,
        vec![blocked],
        &["crates/server/".into()],
        &[],
        false,
    )
    .unwrap();
    let mut wrong_base = scoped("op-1", "one", "1", &["src"]);
    wrong_base.base = OTHER_SHA.into();
    assert!(error(assign(wrong_base)).contains("published safe shipping base"));
    assert!(error(assign(scoped("op-1", "one", "9", &["src"]))).contains("blocked this ticket"));
    assert!(
        error(assign(scoped("op-1", "one", "1", &[])))
            .contains("declare their repository path scope")
    );
    assert!(
        error(assign(scoped(
            "op-1",
            "one",
            "1",
            &["crates/server/src/lib.rs"]
        )))
        .contains("overlaps in-flight integration scope crates/server")
    );
    assign(scoped("op-1", "one", "1", &["./clients/web/"])).unwrap();
    assert_eq!(
        c.state().unwrap().assignments["op-1"].scope,
        ["clients/web"]
    );
    assert!(
        error(assign(scoped("op-2", "two", "2", &["clients"])))
            .contains("held by pending assignment op-1")
    );
    // A sibling path with a shared prefix is not an overlap.
    assign(scoped("op-2", "two", "2", &["clients/web-extra"])).unwrap();
    // Integration and CI still hold the scope; completion frees it.
    c.worker_transition("one", "one-generation", "op-1", Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        "one",
        "one-generation",
        "op-1",
        Phase::Handoff,
        Some(SHA.into()),
    )
    .unwrap();
    for phase in [Phase::Integrating, Phase::Ci] {
        c.mark("root", "root-1", NOW, "op-1", phase).unwrap();
        assert!(
            error(assign(scoped("op-3", "three", "3", &["clients/web/src"])))
                .contains("held by pending assignment op-1")
        );
    }
    c.mark("root", "root-1", NOW, "op-1", Phase::Done).unwrap();
    assign(scoped("op-3", "three", "3", &["clients/web/src"])).unwrap();
}

#[test]
fn hs2_twzw7x_a_replaced_integrator_generation_must_republish() {
    let (_root, c) = dispatcher_pool();
    worker(&c, "worker", NOW);
    register(&c, Role::Integrator, "root", "root-2", NOW + 200);
    register(&c, Role::Dispatcher, "dispatcher", "dispatch-1", NOW + 200);
    let snapshot = c.snapshot(NOW + 200).unwrap();
    assert!(
        snapshot
            .dispatch_blockers
            .contains(&"integration_report_from_replaced_generation".to_owned())
    );
    worker(&c, "worker", NOW + 200);
    assert!(
        error(c.assign(
            "dispatcher",
            "dispatch-1",
            NOW + 200,
            scoped("op", "worker", "1", &["a"])
        ))
        .contains("replaced integrator generation")
    );
    // The old generation can no longer publish; the replacement can.
    assert!(
        error(c.publish(
            "root",
            "root-1",
            &|| NOW + 200,
            SHA,
            Vec::new(),
            &[],
            &[],
            false
        ))
        .contains("exact integrator")
    );
    c.publish(
        "root",
        "root-2",
        &|| NOW + 200,
        OTHER_SHA,
        Vec::new(),
        &[],
        &[],
        false,
    )
    .unwrap();
    let mut a = scoped("op", "worker", "1", &["a"]);
    a.base = OTHER_SHA.into();
    c.assign("dispatcher", "dispatch-1", NOW + 200, a).unwrap();
}

#[test]
fn hs2_twzw7x_dispatcher_refills_a_worker_while_its_handoff_integrates_and_runs_ci() {
    let (_root, c) = dispatcher_pool();
    worker(&c, "worker", NOW);
    c.assign(
        "dispatcher",
        "dispatch-1",
        NOW,
        scoped("op-1", "worker", "1", &["a"]),
    )
    .unwrap();
    assert!(
        error(c.assign(
            "dispatcher",
            "dispatch-1",
            NOW,
            scoped("op-2", "worker", "2", &["b"])
        ))
        .contains("already has an active assignment")
    );
    c.worker_transition("worker", "worker-generation", "op-1", Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        "worker",
        "worker-generation",
        "op-1",
        Phase::Handoff,
        Some(SHA.into()),
    )
    .unwrap();
    c.mark("root", "root-1", NOW, "op-1", Phase::Integrating)
        .unwrap();
    c.assign(
        "dispatcher",
        "dispatch-1",
        NOW,
        scoped("op-2", "worker", "2", &["b"]),
    )
    .unwrap();
    c.mark("root", "root-1", NOW, "op-1", Phase::Ci).unwrap();
    // The integrator keeps its own transitions; the dispatcher has none.
    assert!(
        error(c.mark("dispatcher", "dispatch-1", NOW, "op-1", Phase::Done))
            .contains("exact integrator")
    );
    let state = c.state().unwrap();
    assert_eq!(state.assignments["op-1"].phase, Phase::Ci);
    assert_eq!(state.active_assignment("worker").unwrap().id, "op-2");
}

#[test]
fn hs2_twzw7x_returning_authority_waits_for_the_dispatcher_and_never_downgrades() {
    let (_root, c) = dispatcher_pool();
    assert!(
        error(c.set_authority("root", "root-1", NOW, Authority::Integrator, false))
            .contains("live dispatcher")
    );
    c.leave(Role::Dispatcher, "dispatcher", "dispatch-1")
        .unwrap();
    c.set_authority("root", "root-1", NOW, Authority::Integrator, false)
        .unwrap();
    let state = c.state().unwrap();
    assert_eq!((state.version, state.authority), (4, Authority::Integrator));
    assert!(
        error(c.register(
            Role::Dispatcher,
            "dispatcher",
            "dispatch-2",
            NOW,
            120,
            ReturnRoute::HumanRelay
        ))
        .contains("assignment authority is the integrator")
    );
    worker(&c, "worker", NOW);
    c.assign("root", "root-1", NOW, scoped("op", "worker", "1", &[]))
        .unwrap();
}

#[test]
fn hs2_twzw7x_newer_schemas_and_smuggled_fields_are_refused() {
    let (root, c) = fixture();
    let path = c.directory.join("state.json");
    let mut raw: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    raw["authority"] = "dispatcher".into();
    fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(error(c.state()).contains("enabling dispatcher authority"));
    // Schema six is the newest this executable reads (HS2-2D3099); seven is refused.
    raw["version"] = 7.into();
    fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let project = root.path().join("project");
    let store = root.path().join("store");
    assert!(
        Coordinator::open(&project, &store)
            .err()
            .unwrap()
            .to_string()
            .contains("incompatible coordination state")
    );
}

#[test]
fn hs2_twzw7x_dispatch_blockers_name_each_missing_role_and_fact() {
    let (_root, c) = fixture();
    let blockers = |now| c.snapshot(now).unwrap().dispatch_blockers;
    assert_eq!(
        blockers(NOW),
        [
            "assignment_authority_is_integrator",
            "no_live_integrator",
            "no_available_worker"
        ]
    );
    register(&c, Role::Integrator, "root", "root-1", NOW);
    c.set_authority("root", "root-1", NOW, Authority::Dispatcher, true)
        .unwrap();
    assert_eq!(
        blockers(NOW),
        [
            "no_live_dispatcher",
            "no_safe_base_published",
            "no_available_worker"
        ]
    );
    register(&c, Role::Dispatcher, "dispatcher", "dispatch-1", NOW);
    c.publish("root", "root-1", &|| NOW, SHA, Vec::new(), &[], &[], false)
        .unwrap();
    worker(&c, "worker", NOW);
    assert!(blockers(NOW).is_empty());
    assert!(c.snapshot(NOW).unwrap().live_dispatcher);
    c.leave(Role::Integrator, "root", "root-1").unwrap();
    assert_eq!(blockers(NOW), ["no_integrator_registered"]);
}

#[test]
fn hs2_twzw7x_scope_paths_are_relative_and_overlap_by_component() {
    assert_eq!(
        normalized_scope(&["a/b/".into(), "./a/b".into(), "c\\d".into()]).unwrap(),
        ["a/b", "c/d"]
    );
    for bad in ["/etc", "../x", "a/../b", ".", "", "C:/x"] {
        assert!(normalized_scope(&[bad.into()]).is_err(), "{bad}");
    }
    assert!(scopes_overlap("a", "a/b"));
    assert!(scopes_overlap("a/b", "a"));
    assert!(scopes_overlap("a/b", "a/b"));
    assert!(!scopes_overlap("a/b", "a/bc"));
    assert!(!scopes_overlap("a", "b"));
}

fn dispatcher_wait() -> WaitRequest {
    WaitRequest {
        role: Role::Dispatcher,
        id: "dispatcher".into(),
        generation: "dispatch-1".into(),
        lease_seconds: 600,
        return_route: ReturnRoute::HumanRelay,
        after: None,
    }
}

#[test]
fn hs2_twzw7x_try_register_is_atomic_and_keeps_real_conflicts() {
    let (_root, c) = fixture();
    let try_dispatcher = |c: &Coordinator, generation: &str, now| {
        c.try_register(
            Role::Dispatcher,
            "dispatcher",
            generation,
            now,
            120,
            ReturnRoute::HumanRelay,
        )
    };
    let before = c.state().unwrap();
    assert!(!try_dispatcher(&c, "dispatch-1", NOW).unwrap());
    assert_eq!(
        c.state().unwrap(),
        before,
        "legacy refusal leaves state untouched"
    );
    register(&c, Role::Integrator, "root", "root-1", NOW);
    c.set_authority("root", "root-1", NOW, Authority::Dispatcher, true)
        .unwrap();
    assert!(try_dispatcher(&c, "dispatch-1", NOW).unwrap());
    assert!(
        error(try_dispatcher(&c, "dispatch-2", NOW + 1)).contains("another live session"),
        "a live duplicate generation stays a conflict, not an unregistered wait"
    );
    assert!(try_dispatcher(&c, "dispatch-1", NOW + 1).unwrap());
}

#[test]
fn hs2_twzw7x_waiting_dispatcher_survives_a_revert_between_observation_and_registration() {
    let (root, c) = fixture();
    let project = root.path().join("project");
    let store = root.path().join("store");
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    register(&c, Role::Integrator, "root", "root-1", now);
    let (hook_project, hook_store) = (project.clone(), store.clone());
    // Runs exactly at the interleaving point: authority was observed, registration not yet run.
    *c.before_dispatcher_register.lock().unwrap() = Some(Box::new(move || {
        let other = Coordinator::open(&hook_project, &hook_store).unwrap();
        other
            .set_authority("root", "root-1", now, Authority::Integrator, false)
            .unwrap();
    }));
    let (hook_project, hook_store) = (project.clone(), store.clone());
    *c.after_dispatcher_register.lock().unwrap() = Some(Box::new(move || {
        let other = Coordinator::open(&hook_project, &hook_store).unwrap();
        assert_eq!(other.state().unwrap().authority, Authority::Integrator);
        assert!(other.state().unwrap().dispatcher.is_none());
        other
            .set_authority("root", "root-1", now, Authority::Dispatcher, false)
            .unwrap();
    }));
    let (hook_project, hook_store) = (project.clone(), store.clone());
    *c.waiting_unregistered.lock().unwrap() = Some(Box::new(move || {
        // The subscription already exists. Publish the first transition from this
        // readiness hook so host scheduling cannot consume a short test deadline.
        Coordinator::open(&hook_project, &hook_store)
            .unwrap()
            .set_authority("root", "root-1", now, Authority::Dispatcher, true)
            .unwrap();
    }));
    let snapshot = c
        .next(&dispatcher_wait())
        .expect("the same waiter registers after re-enable");
    assert!(snapshot.live_dispatcher);
    assert_eq!(snapshot.state.dispatcher.unwrap().generation, "dispatch-1");
}

#[test]
fn hs2_twzw7x_reservation_checks_are_serialized_with_authority_switches() {
    let (root, c) = fixture();
    let project = root.path().join("project");
    let store = root.path().join("store");
    register(&c, Role::Integrator, "root", "root-1", NOW);
    worker(&c, "worker", NOW);
    let (done_tx, done) = std::sync::mpsc::channel();
    let switch = std::cell::RefCell::new(None);
    let reserved = c
        .assign_verified(
            "root",
            "root-1",
            &|| NOW,
            scoped("op", "worker", "1", &[]),
            Box::new(|current, replayed| {
                assert_eq!(current.authority, Authority::Integrator);
                assert!(replayed.is_none());
                let (project, store, done_tx) = (project.clone(), store.clone(), done_tx.clone());
                *switch.borrow_mut() = Some(std::thread::spawn(move || {
                    Coordinator::open(&project, &store)
                        .unwrap()
                        .set_authority("root", "root-1", NOW, Authority::Dispatcher, true)
                        .unwrap();
                    done_tx.send(()).unwrap();
                }));
                // The switch cannot complete while this reservation's checks hold the locks.
                assert!(
                    done.recv_timeout(std::time::Duration::from_millis(500))
                        .is_err()
                );
                Ok(())
            }),
        )
        .unwrap();
    switch.into_inner().unwrap().join().unwrap();
    let state = c.state().unwrap();
    assert_eq!(state.assignments["op"], reserved);
    assert_eq!(state.authority, Authority::Dispatcher);
    // After the switch, the same integrator identity can no longer reserve.
    worker(&c, "other", NOW);
    assert!(
        error(c.assign("root", "root-1", NOW, scoped("op-2", "other", "2", &["a"])))
            .contains("assignment authority is the dispatcher")
    );
}

#[test]
fn hs2_twzw7x_idle_unregistered_wait_has_no_synthetic_deadline() {
    // Without peers, claims or a registration, the only wake source is a genuine event.
    let (_root, c) = fixture();
    let snapshot = c.snapshot(NOW).unwrap();
    assert!(super::wait::claim_deadline(Role::Dispatcher, &snapshot, NOW).is_none());
    assert!(snapshot.state.integrator.is_none() && snapshot.state.workers.is_empty());
}

#[test]
fn hs2_twzw7x_reservation_samples_its_clock_inside_the_serialized_boundary() {
    let (_root, c) = dispatcher_pool();
    worker(&c, "worker", NOW);
    let unchanged = c.state().unwrap();
    // The caller's pre-lock time is irrelevant: the boundary clock is past both leases.
    let sampled = std::cell::Cell::new(0);
    let late = || {
        sampled.set(sampled.get() + 1);
        NOW + 121
    };
    let refused = c.assign_verified(
        "dispatcher",
        "dispatch-1",
        &late,
        scoped("op", "worker", "1", &["a"]),
        Box::new(|_, _| Ok(())),
    );
    assert!(error(refused).contains("exact dispatcher session"));
    assert_eq!(sampled.get(), 1, "sampled once, inside the reservation");
    assert_eq!(c.state().unwrap(), unchanged);
    // A live dispatcher with an expired worker is refused on the worker instead.
    register(&c, Role::Dispatcher, "dispatcher", "dispatch-1", NOW + 121);
    assert!(
        error(c.assign_verified(
            "dispatcher",
            "dispatch-1",
            &|| NOW + 121,
            scoped("op", "worker", "1", &["a"]),
            Box::new(|_, _| Ok(())),
        ))
        .contains("worker is not currently available")
    );
    assert!(c.state().unwrap().assignments.is_empty());
}

#[test]
fn hs2_twzw7x_reservation_rechecks_sessions_after_ticket_validation() {
    for (expired_role, replay) in [
        (Role::Dispatcher, false),
        (Role::Worker, false),
        (Role::Dispatcher, true),
    ] {
        let (_root, c) = dispatcher_pool();
        c.register(
            Role::Dispatcher,
            "dispatcher",
            "dispatch-1",
            NOW,
            if expired_role == Role::Dispatcher {
                120
            } else {
                600
            },
            ReturnRoute::HumanRelay,
        )
        .unwrap();
        c.register(
            Role::Worker,
            "worker",
            "worker-generation",
            NOW,
            if expired_role == Role::Worker {
                120
            } else {
                600
            },
            ReturnRoute::HumanRelay,
        )
        .unwrap();
        let reservation = scoped("op", "worker", "1", &["a"]);
        if replay {
            c.assign("dispatcher", "dispatch-1", NOW, reservation.clone())
                .unwrap();
        }
        let path = c.directory.join("state.json");
        let unchanged = fs::read(&path).unwrap();
        let clock = std::cell::Cell::new(NOW);
        let refused = c.assign_verified(
            "dispatcher",
            "dispatch-1",
            &|| clock.get(),
            reservation,
            Box::new(|_, _| {
                // Ticket validation can outlast availability even after both locks are held.
                clock.set(NOW + 121);
                Ok(())
            }),
        );
        let expected = if expired_role == Role::Dispatcher {
            "exact dispatcher session"
        } else {
            "worker is not currently available"
        };
        assert!(
            error(refused).contains(expected),
            "{expired_role:?}, replay={replay}"
        );
        assert_eq!(fs::read(path).unwrap(), unchanged);
    }
}
