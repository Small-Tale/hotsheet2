//! HS2-4JQ41V: a project's integrator, started on demand when handoffs wait. The
//! states are none, starting (a session the pool has not seen join), live, idle,
//! winding down and failed; the events are a handoff arriving, the queue emptying,
//! a CI gate resolving, the cap filling, a prompt failing and the opt-out toggling.
//! Synthetic targets, tools and times only.

use super::scheduler_tests::{MIN, T0, World, policy, session, target, work};
use super::*;

/// The pool of target 1 with `queued` handoffs plus `ci` waiting on their CI gate.
fn pool(world: &mut World, queued: u32, ci: u32, live: bool) {
    world.targets[0].capacity.as_mut().unwrap().pool = Some(PoolState {
        dispatcher_live: false,
        integrator_live: live,
        handoffs_pending: queued + ci,
        ci_pending: ci,
    });
}

fn integrator(name: &str) -> ScheduledSession {
    ScheduledSession {
        role: Some(SessionRole::Integrator),
        ..session(name, 1, SessionActivity::Idle)
    }
}

/// The integrator starts of a schedule as `(reason, detail, target_sessions)`.
fn integrator_starts(schedule: &Schedule) -> Vec<(&'static str, Option<String>, u32)> {
    schedule
        .placements
        .iter()
        .filter(|placement| {
            placement.action == PlacementAction::Start
                && placement.role == Some(SessionRole::Integrator)
        })
        .map(|placement| {
            (
                placement.reason.code(),
                placement.detail.clone(),
                placement.inputs.target_sessions,
            )
        })
        .collect()
}

fn decided(schedule: &Schedule, name: &str) -> (&'static str, &'static str, Option<String>) {
    let placement = schedule
        .placements
        .iter()
        .find(|placement| placement.session_id.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no decision for {name}: {schedule:#?}"));
    (
        placement.action.code(),
        placement.reason.code(),
        placement.detail.clone(),
    )
}

/// Why the pass started no integrator on target 1, as `(reason, detail)`.
fn not_started(schedule: &Schedule) -> Option<(&'static str, Option<String>)> {
    schedule
        .roles_unplaced
        .iter()
        .find(|item| item.target_id == target(1) && item.role == SessionRole::Integrator)
        .map(|item| (item.reason.code(), item.detail.clone()))
}

#[test]
fn hs2_4jq41v_a_handoff_alone_starts_one_integrator_on_an_unlisted_project() {
    // No role_sessions entry, no work, no session: only the pool's handoff.
    let mut world = World::new(&[(1, 3)]);
    assert!(world.policy.placement.role_sessions.is_empty());
    pool(&mut world, 0, 0, false);
    assert!(integrator_starts(&world.now()).is_empty());
    assert_eq!(not_started(&world.now()), None);

    // The first handoff: exactly one integrator, in one pass.
    pool(&mut world, 1, 0, false);
    let schedule = world.now();
    assert_eq!(
        integrator_starts(&schedule),
        [("integrator_started", Some("handoff_pending".into()), 1)]
    );
    assert_eq!(not_started(&schedule), None);

    // Two handoffs arriving together still start one.
    pool(&mut world, 2, 0, false);
    assert_eq!(integrator_starts(&world.now()).len(), 1);

    // Repeated passes while it starts (the pool has not seen it join): no second.
    world.sessions = vec![integrator("integrator")];
    for _ in 0..3 {
        let schedule = world.now();
        assert!(integrator_starts(&schedule).is_empty());
        assert_eq!(
            decided(&schedule, "integrator"),
            ("keep", "integrator_kept", Some("queue".into()))
        );
        assert_eq!(not_started(&schedule), None);
    }
}

#[test]
fn hs2_4jq41v_integrator_walks_live_ci_idle_wound_down_and_refills() {
    let mut world = World::new(&[(1, 3)]);
    world.sessions = vec![integrator("integrator")];
    let later = T0 + 120 * MIN;

    // Live with a queue, then the queue empties into CI: kept for each.
    pool(&mut world, 2, 0, true);
    assert_eq!(
        decided(&world.at(later), "integrator"),
        ("keep", "integrator_kept", Some("queue".into()))
    );
    pool(&mut world, 1, 1, true);
    assert_eq!(
        decided(&world.at(later), "integrator").2,
        Some("queue".into())
    );
    pool(&mut world, 0, 1, true);
    assert_eq!(
        decided(&world.at(later), "integrator"),
        ("keep", "integrator_kept", Some("ci_pending".into()))
    );

    // The CI gate resolves: idle. Within the grace it stays, past it and at a safe
    // point it winds down; holding a claim (no safe point) it waits.
    pool(&mut world, 0, 0, true);
    world.sessions[0].idle_since_ms = Some(T0);
    assert_eq!(decided(&world.at(T0 + MIN), "integrator").1, "idle_grace");
    world.sessions[0].safe_point = false;
    assert_eq!(
        decided(&world.at(later), "integrator").1,
        "awaiting_safe_point"
    );
    world.sessions[0].safe_point = true;
    assert_eq!(
        decided(&world.at(later), "integrator"),
        ("wind_down", "integrator_wound_down", Some("idle".into()))
    );

    // Winding down while a new handoff arrives: no second one beside it.
    world.sessions[0].winding_down = true;
    pool(&mut world, 1, 0, false);
    let schedule = world.at(later);
    assert_eq!(decided(&schedule, "integrator").1, "winding_down");
    assert!(integrator_starts(&schedule).is_empty());

    // Gone: within the cooldown it says so; after it a later handoff starts a new one.
    world.sessions.clear();
    world.memory = vec![PlacementMemory {
        target_id: target(1),
        last_wound_down_at_ms: Some(later),
        ..PlacementMemory::default()
    }];
    let schedule = world.at(later + MIN);
    assert!(integrator_starts(&schedule).is_empty());
    assert_eq!(
        not_started(&schedule),
        Some(("integrator_not_started", Some("cooldown".into())))
    );
    assert!(schedule.retry_at_ms.is_some());
    let schedule = world.at(later + 60 * MIN);
    assert_eq!(integrator_starts(&schedule).len(), 1);
    assert_eq!(not_started(&schedule), None);

    // Empty, then refilled: nothing pending starts nothing, and nothing is reported.
    pool(&mut world, 0, 0, false);
    let schedule = world.at(later + 60 * MIN);
    assert!(integrator_starts(&schedule).is_empty());
    assert_eq!(not_started(&schedule), None);
    pool(&mut world, 1, 0, false);
    assert_eq!(integrator_starts(&world.at(later + 60 * MIN)).len(), 1);
}

#[test]
fn hs2_4jq41v_workers_filling_the_cap_never_starve_the_integrator() {
    // (cap, busy workers, expected integrator starts, the reason when none)
    let cases: Vec<(u32, usize, usize, Option<&str>)> = vec![
        (2, 0, 1, None),
        (2, 1, 1, None),
        // At the cap: the integrator takes the one slot over it.
        (2, 2, 1, None),
        (1, 1, 1, None),
        // Over by one already (a person's session): not twice.
        (2, 3, 0, Some("target_cap")),
        // A cap of 0 starts nothing at all.
        (0, 0, 0, Some("target_cap")),
    ];
    for (cap, workers, expected, reason) in cases {
        let mut world = World::new(&[(1, cap)]);
        pool(&mut world, 1, 0, false);
        world.sessions = (0..workers)
            .map(|n| session(&format!("w{n}"), 1, SessionActivity::Busy))
            .collect();
        let schedule = world.now();
        let starts = integrator_starts(&schedule);
        assert_eq!(starts.len(), expected, "cap {cap}, workers {workers}");
        if let Some(&(_, _, sessions)) = starts.first() {
            assert!(sessions <= cap + 1, "cap {cap}: {schedule:#?}");
        }
        assert_eq!(
            not_started(&schedule).and_then(|(_, detail)| detail),
            reason.map(str::to_owned),
            "cap {cap}, workers {workers}"
        );
        // Busy workers are never wound down to make that room.
        assert!(
            schedule
                .placements
                .iter()
                .all(|placement| placement.action != PlacementAction::WindDown),
            "cap {cap}: {schedule:#?}"
        );
    }
    // New work at the full cap waits; the integrator does not take its slot away.
    let mut world = World::new(&[(1, 2)]);
    pool(&mut world, 1, 0, false);
    world.sessions = vec![
        session("w0", 1, SessionActivity::Busy),
        session("w1", 1, SessionActivity::Busy),
    ];
    world.work = vec![work(1, 1)];
    let schedule = world.now();
    assert_eq!(integrator_starts(&schedule).len(), 1);
    assert_eq!(schedule.unplaced.len(), 1, "{schedule:#?}");
}

#[test]
fn hs2_4jq41v_opt_out_toggles_and_other_reasons_are_reported() {
    let mut world = World::new(&[(1, 3)]);
    pool(&mut world, 1, 0, false);

    // Opted out: the person's entry leaves the integrator out.
    world.policy.placement.role_sessions = vec![TargetRoleSessions {
        target_id: target(1),
        roles: vec![RoleSessionKind::Dispatcher],
    }];
    for _ in 0..2 {
        let schedule = world.now();
        assert!(integrator_starts(&schedule).is_empty());
        assert_eq!(
            not_started(&schedule),
            Some(("integrator_not_started", Some("policy_opt_out".into())))
        );
    }
    // Toggled back to Auto, and explicitly listed: it starts.
    world.policy.placement.role_sessions.clear();
    assert_eq!(integrator_starts(&world.now()).len(), 1);
    world.policy.placement.role_sessions = vec![TargetRoleSessions {
        target_id: target(1),
        roles: vec![RoleSessionKind::Integrator],
    }];
    assert_eq!(integrator_starts(&world.now()).len(), 1);
    world.policy.placement.role_sessions.clear();

    // A live integrator (a person's elsewhere) needs no start and no reason.
    pool(&mut world, 1, 0, true);
    let schedule = world.now();
    assert!(integrator_starts(&schedule).is_empty());
    assert_eq!(not_started(&schedule), None);
    pool(&mut world, 1, 0, false);

    // A disabled target.
    world.policy.targets[0].enabled = false;
    assert_eq!(
        not_started(&world.now()),
        Some(("integrator_not_started", Some("target_disabled".into())))
    );
    world.policy.targets[0].enabled = true;

    // The machine's own cap refuses.
    world.policy.machine.max_sessions = 1;
    world.targets[0].capacity.as_mut().unwrap().sessions = Some(vec![SessionFootprint {
        session_id: "m0".into(),
        tool: Some("alpha".into()),
        busy: true,
        on_checkout: Some(true),
        resident_mb: None,
    }]);
    assert_eq!(
        not_started(&world.now()),
        Some(("integrator_not_started", Some("machine_refused".into())))
    );

    // An unknown pool says nothing: nothing starts on a guess.
    let mut world = World::new(&[(1, 3)]);
    world.work = vec![work(1, 1)];
    let schedule = world.now();
    assert!(integrator_starts(&schedule).is_empty());
    assert_eq!(not_started(&schedule), None);
}

#[test]
fn hs2_4jq41v_the_policy_gate_allows_an_integrator_one_over_the_target_cap() {
    use PolicyDenial as D;
    use PolicyVerdict as V;
    let body = policy(&[(1, 2)]);
    let session = SessionRequest {
        tool: "alpha".into(),
        model: None,
        running: 2,
        starting: true,
    };
    let target_id = target(1);
    let ask = |target_sessions: u32, integrator: bool, cap: Option<u32>| {
        let mut body = body.clone();
        if let Some(cap) = cap {
            body.targets[0].max_sessions = Some(cap);
        }
        let load = SessionLoad {
            target_sessions,
            integrator,
            ..SessionLoad::default()
        };
        body.evaluate(&PolicyQuestion {
            target_id: &target_id,
            action: TargetAction::SessionsManage,
            session: Some(&session),
            load: Some(&load),
            provisional: false,
            role_session: integrator,
        })
    };
    assert_eq!(ask(1, false, None), V::Allowed);
    assert_eq!(ask(2, false, None), V::Denied(D::TargetCap));
    assert_eq!(ask(2, true, None), V::Allowed);
    assert_eq!(ask(3, true, None), V::Denied(D::TargetCap));
    // A cap of 0 starts nothing, an integrator included.
    assert_eq!(ask(0, true, Some(0)), V::Denied(D::TargetCap));
    // The flag reads and writes only when set, so older loads are unchanged.
    let plain = serde_json::to_value(SessionLoad::default()).unwrap();
    assert!(plain.get("integrator").is_none());
    let back: SessionLoad = serde_json::from_value(plain).unwrap();
    assert!(!back.integrator);
}

#[test]
fn hs2_4jq41v_ci_pending_enters_the_capacity_version_only_when_reported() {
    let mut snapshot = super::scheduler_tests::snapshot(1, 16_384, 0);
    snapshot.pool = Some(PoolState {
        handoffs_pending: 1,
        ..PoolState::default()
    });
    let without = capacity_version(&snapshot);
    let json = serde_json::to_value(snapshot.pool.unwrap()).unwrap();
    assert!(json.get("ci_pending").is_none());
    snapshot.pool = Some(PoolState {
        handoffs_pending: 1,
        ci_pending: 1,
        ..PoolState::default()
    });
    assert_ne!(capacity_version(&snapshot), without);
}

/// HS2-0VTQFF: an integrator start resolves a pattern-only list like a worker start;
/// with nothing to resolve it says why instead of starting the tool's default.
#[test]
fn hs2_0vtqff_an_integrator_start_names_the_resolved_model() {
    let mut world = World::new(&[(1, 3)]);
    pool(&mut world, 1, 0, false);
    world.policy.targets[0].tools.truncate(1);
    world.policy.targets[0].tools[0].models = vec!["syn-large-*".into()];
    assert!(integrator_starts(&world.now()).is_empty());
    assert_eq!(
        not_started(&world.now()),
        Some(("integrator_not_started", Some("model_unresolved".into())))
    );

    let known = &mut world.targets[0]
        .capacity
        .as_mut()
        .unwrap()
        .tools
        .as_mut()
        .unwrap()[0];
    known.models = Some(vec!["syn-small-1".into()]);
    assert_eq!(
        not_started(&world.now()),
        Some(("integrator_not_started", Some("no_matching_model".into())))
    );

    let known = &mut world.targets[0]
        .capacity
        .as_mut()
        .unwrap()
        .tools
        .as_mut()
        .unwrap()[0];
    known.models = Some(vec!["syn-small-1".into(), "syn-large-2".into()]);
    let schedule = world.now();
    let start = schedule
        .placements
        .iter()
        .find(|placement| placement.role == Some(SessionRole::Integrator))
        .unwrap();
    assert_eq!(
        (start.tool.as_str(), start.model.as_deref()),
        ("alpha", Some("syn-large-2"))
    );
    assert_eq!(not_started(&schedule), None);
}
