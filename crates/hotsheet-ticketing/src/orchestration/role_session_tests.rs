//! HS2-TK9ADC: the scheduler's dispatcher and integrator sessions. A target opts in
//! with `placement.role_sessions`; the pool facts come with its capacity snapshot.
//! Synthetic targets, tools and times only.

use super::scheduler_tests::{MIN, T0, World, id, policy, session, target, work};
use super::*;

const D: RoleSessionKind = RoleSessionKind::Dispatcher;
/// One case: its name, the roles listed, the pool, whether work is ready, the starts.
type OptCase<'a> = (
    &'a str,
    &'a [RoleSessionKind],
    Option<PoolState>,
    bool,
    Vec<&'a str>,
);
/// One change to an otherwise valid policy, and the problem it must report.
type PolicyCase = (&'static str, Box<dyn Fn(&mut PolicyBody)>);
const I: RoleSessionKind = RoleSessionKind::Integrator;

fn opted(world: &mut World, roles: &[RoleSessionKind]) {
    world.policy.placement.role_sessions = vec![TargetRoleSessions {
        target_id: target(1),
        roles: roles.to_vec(),
    }];
}

fn pool(world: &mut World, pool: Option<PoolState>) {
    world.targets[0].capacity.as_mut().unwrap().pool = pool;
}

fn empty_pool() -> Option<PoolState> {
    Some(PoolState::default())
}

/// A role session of the owner on target 1, idle since a minute ago and at a safe point.
fn role_session(name: &str, role: SessionRole) -> ScheduledSession {
    ScheduledSession {
        role: Some(role),
        ..session(name, 1, SessionActivity::Idle)
    }
}

/// Every start as `role:<code>` or `step:<n>`, in placement order.
fn started(schedule: &Schedule) -> Vec<String> {
    schedule
        .placements
        .iter()
        .filter(|placement| placement.action == PlacementAction::Start)
        .map(|placement| match placement.role {
            Some(role) => format!("role:{}", role.code()),
            None => format!("step:{}", placement.steps[0]),
        })
        .collect()
}

fn decided(schedule: &Schedule, name: &str) -> (&'static str, &'static str, Option<String>) {
    let placement = schedule
        .placements
        .iter()
        .find(|placement| placement.session_id.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no decision for session {name}: {schedule:#?}"));
    (
        placement.action.code(),
        placement.reason.code(),
        placement.detail.clone(),
    )
}

#[test]
fn hs2_tk9adc_role_sessions_follow_a_target_from_no_work_to_wound_down() {
    let mut world = World::new(&[(1, 3)]);
    opted(&mut world, &[D, I]);
    pool(&mut world, empty_pool());

    // No work, nothing handed off: nothing starts.
    assert!(started(&world.now()).is_empty());
    // Repeated: still nothing.
    assert!(started(&world.now()).is_empty());

    // Work arrives: the dispatcher is placed before the worker, against the cap.
    world.work = vec![work(1, 1)];
    let schedule = world.now();
    assert_eq!(
        started(&schedule),
        ["role:dispatcher", format!("step:{}", id(1)).as_str()]
    );
    let dispatcher = &schedule.placements[0];
    assert_eq!(dispatcher.reason, PlacementReason::RoleNeeded);
    assert_eq!(dispatcher.detail.as_deref(), Some("dispatcher"));
    assert!(dispatcher.steps.is_empty());
    assert_eq!(dispatcher.inputs.target_sessions, 1);
    assert_eq!(schedule.placements[1].inputs.target_sessions, 2);

    // Both started and the worker's run took the step; the pool has not seen the
    // dispatcher join yet. A repeated pass starts no second dispatcher.
    world.work.clear();
    world.sessions = vec![
        role_session("dispatcher", SessionRole::Dispatcher),
        session("worker", 1, SessionActivity::Busy),
    ];
    for _ in 0..2 {
        let schedule = world.now();
        assert!(started(&schedule).is_empty(), "{schedule:#?}");
        assert_eq!(
            decided(&schedule, "dispatcher"),
            ("keep", "role_needed", Some("dispatcher".into()))
        );
        assert_eq!(decided(&schedule, "worker"), ("keep", "busy", None));
    }
    // The pool shows it live: the same.
    pool(
        &mut world,
        Some(PoolState {
            dispatcher_live: true,
            ..PoolState::default()
        }),
    );
    assert_eq!(decided(&world.now(), "dispatcher").1, "role_needed");

    // The work is done and handed off: the worker is idle, no work is left. The
    // integrator is placed; the dispatcher has nothing to serve and waits out its grace.
    world.work.clear();
    world.sessions[1] = session("worker", 1, SessionActivity::Idle);
    pool(
        &mut world,
        Some(PoolState {
            dispatcher_live: true,
            integrator_live: false,
            handoffs_pending: 1,
            ci_pending: 0,
        }),
    );
    let schedule = world.now();
    assert_eq!(started(&schedule), ["role:integrator"]);
    assert_eq!(decided(&schedule, "dispatcher").1, "idle_grace");

    // The integrator runs while the handoff is pending; past the idle time the
    // dispatcher winds down at its safe point, the integrator stays.
    world
        .sessions
        .push(role_session("integrator", SessionRole::Integrator));
    let later = T0 + 120 * MIN;
    let schedule = world.at(later);
    assert!(started(&schedule).is_empty());
    // HS2-4JQ41V: kept for its queue.
    assert_eq!(
        decided(&schedule, "integrator"),
        ("keep", "integrator_kept", Some("queue".into()))
    );
    assert_eq!(decided(&schedule, "dispatcher").0, "wind_down");

    // Integrated: nothing pending. The integrator winds down once at a safe point.
    world
        .sessions
        .retain(|session| session.session_id != "dispatcher");
    pool(&mut world, empty_pool());
    world.sessions[1].safe_point = false;
    assert_eq!(
        decided(&world.at(later), "integrator").1,
        "awaiting_safe_point"
    );
    world.sessions[1].safe_point = true;
    assert_eq!(
        decided(&world.at(later), "integrator"),
        ("wind_down", "integrator_wound_down", Some("idle".into()))
    );
    // Empty, then work again: the dispatcher is placed again.
    world.sessions.clear();
    world.work = vec![work(2, 1)];
    assert_eq!(started(&world.at(later))[0], "role:dispatcher");
}

#[test]
fn hs2_tk9adc_a_role_session_is_kept_while_a_worker_on_its_target_has_a_run() {
    let mut world = World::new(&[(1, 3)]);
    opted(&mut world, &[D, I]);
    pool(&mut world, empty_pool());
    world.sessions = vec![
        role_session("dispatcher", SessionRole::Dispatcher),
        role_session("integrator", SessionRole::Integrator),
        session("worker", 1, SessionActivity::Busy),
    ];
    let later = T0 + 600 * MIN;
    assert_eq!(decided(&world.at(later), "dispatcher").1, "role_needed");
    // HS2-4JQ41V: the integrator says what it is kept for.
    assert_eq!(
        decided(&world.at(later), "integrator"),
        ("keep", "integrator_kept", Some("worker_run".into()))
    );
    // The run finished: nothing depends on them any more.
    world.sessions[2] = session("worker", 1, SessionActivity::Idle);
    let schedule = world.at(later);
    for name in ["dispatcher", "integrator"] {
        assert_eq!(decided(&schedule, name).0, "wind_down", "{name}");
    }
}

#[test]
fn hs2_tk9adc_nothing_starts_without_the_opt_in_and_a_known_pool() {
    // (case, roles listed, pool, work, expected starts)
    let step = format!("step:{}", id(1));
    let cases: Vec<OptCase<'_>> = vec![
        ("opted out", &[], empty_pool(), true, vec![step.as_str()]),
        ("pool unknown", &[D, I], None, true, vec![step.as_str()]),
        (
            "dispatcher live already",
            &[D],
            Some(PoolState {
                dispatcher_live: true,
                ..PoolState::default()
            }),
            true,
            vec![step.as_str()],
        ),
        (
            "integrator live already",
            &[I],
            Some(PoolState {
                integrator_live: true,
                handoffs_pending: 3,
                ..PoolState::default()
            }),
            false,
            vec![],
        ),
        (
            "only the integrator listed",
            &[I],
            empty_pool(),
            true,
            vec![step.as_str()],
        ),
        (
            "only the dispatcher listed, handoffs pending",
            &[D],
            Some(PoolState {
                handoffs_pending: 2,
                ..PoolState::default()
            }),
            false,
            vec![],
        ),
        (
            "both needed",
            &[I, D],
            Some(PoolState {
                handoffs_pending: 1,
                ..PoolState::default()
            }),
            true,
            vec!["role:dispatcher", "role:integrator", step.as_str()],
        ),
    ];
    for (case, roles, state, has_work, expected) in cases {
        let mut world = World::new(&[(1, 3)]);
        // HS2-R2N33M: an entry with no roles opts out; an unlisted project is Auto.
        opted(&mut world, roles);
        pool(&mut world, state);
        if has_work {
            world.work = vec![work(1, 1)];
        }
        assert_eq!(started(&world.now()), expected, "{case}");
    }
}

/// HS2-R2N33M: a project `placement.role_sessions` does not list is Auto: with plan work
/// gets the dispatcher and integrator it needs, each only when it helps, and nothing
/// while its pool is unknown or it has no work and no handoff.
#[test]
fn hs2_r2n33m_unset_role_sessions_start_when_they_help() {
    let step = format!("step:{}", id(1));
    // (case, pool, work, expected starts)
    let cases: Vec<(&str, Option<PoolState>, bool, Vec<&str>)> = vec![
        (
            "ready work, empty pool",
            empty_pool(),
            true,
            vec!["role:dispatcher", step.as_str()],
        ),
        (
            "ready work and a handoff",
            Some(PoolState {
                handoffs_pending: 1,
                ..PoolState::default()
            }),
            true,
            vec!["role:dispatcher", "role:integrator", step.as_str()],
        ),
        ("pool unknown", None, true, vec![step.as_str()]),
        (
            "dispatcher live already",
            Some(PoolState {
                dispatcher_live: true,
                ..PoolState::default()
            }),
            true,
            vec![step.as_str()],
        ),
        ("no work, no session", empty_pool(), false, vec![]),
    ];
    for (case, state, has_work, expected) in cases {
        let mut world = World::new(&[(1, 3)]);
        assert!(world.policy.placement.role_sessions.is_empty(), "{case}");
        pool(&mut world, state);
        if has_work {
            world.work = vec![work(1, 1)];
        }
        assert_eq!(started(&world.now()), expected, "{case}");
    }
}

#[test]
fn hs2_tk9adc_role_sessions_count_against_the_target_cap() {
    // (cap, a worker already runs, handoffs, expected starts)
    let step = format!("step:{}", id(1));
    let cases: Vec<(u32, bool, u32, Vec<&str>)> = vec![
        // A dispatcher in the only slot would assign to nobody: the worker goes first.
        (1, false, 0, vec![step.as_str()]),
        (2, false, 0, vec!["role:dispatcher", step.as_str()]),
        // A running worker: the dispatcher takes the last slot, the new work waits.
        (2, true, 0, vec!["role:dispatcher"]),
        (1, true, 0, vec![]),
        // An integrator needs only its own slot. HS2-4JQ41V: it may take one over
        // the cap, so a full target still integrates; a cap of 0 starts nothing.
        (2, true, 1, vec!["role:dispatcher", "role:integrator"]),
        (3, true, 1, vec!["role:dispatcher", "role:integrator"]),
        (1, true, 1, vec!["role:integrator"]),
        (0, false, 1, vec![]),
    ];
    for (cap, worker, handoffs, expected) in cases {
        let mut world = World::new(&[(1, cap)]);
        opted(&mut world, &[D, I]);
        pool(
            &mut world,
            Some(PoolState {
                handoffs_pending: handoffs,
                ..PoolState::default()
            }),
        );
        world.work = vec![work(1, 1)];
        if worker {
            world.sessions = vec![session("worker", 1, SessionActivity::Busy)];
        }
        let schedule = world.now();
        assert_eq!(
            started(&schedule),
            expected,
            "cap {cap}, worker {worker}, handoffs {handoffs}"
        );
        let total = world.sessions.len() + started(&schedule).len();
        let reserve = usize::from(started(&schedule).iter().any(|s| s == "role:integrator"));
        assert!(
            total <= cap as usize + reserve,
            "cap {cap} exceeded: {schedule:#?}"
        );
    }
}

#[test]
fn hs2_tk9adc_a_role_session_is_never_refilled_with_worker_work() {
    // Not opted in: a person's integrator session of the owner is idle on the target.
    let mut world = World::new(&[(1, 3)]);
    world.sessions = vec![role_session("integrator", SessionRole::Integrator)];
    world.work = vec![work(1, 1)];
    let schedule = world.now();
    // The work gets a worker of its own; the integrator is not given it.
    assert_eq!(started(&schedule), [format!("step:{}", id(1))]);
    let kept = schedule
        .placements
        .iter()
        .find(|placement| placement.session_id.as_deref() == Some("integrator"))
        .unwrap();
    assert!(kept.steps.is_empty());
    assert_ne!(kept.reason, PlacementReason::Refill);
    assert_eq!(kept.role, Some(SessionRole::Integrator));
    // A lowered cap trims workers, never the role session. HS2-4JQ41V: the
    // integrator is the one session allowed over the cap, so one worker stays.
    let mut world = World::new(&[(1, 1)]);
    world.sessions = vec![
        role_session("integrator", SessionRole::Integrator),
        session("worker", 1, SessionActivity::Idle),
        session("worker2", 1, SessionActivity::Idle),
    ];
    let schedule = world.now();
    let trimmed: Vec<_> = ["worker", "worker2"]
        .into_iter()
        .filter(|name| decided(&schedule, name) == ("wind_down", "over_cap", None))
        .collect();
    assert_eq!(trimmed.len(), 1, "{schedule:#?}");
    assert_ne!(decided(&schedule, "integrator").1, "over_cap");
}

#[test]
fn hs2_tk9adc_a_role_start_waits_out_a_refusal_and_a_cooldown() {
    let mut world = World::new(&[(1, 3)]);
    opted(&mut world, &[D]);
    pool(&mut world, empty_pool());
    world.work = vec![work(1, 1)];
    world.memory = vec![PlacementMemory {
        target_id: target(1),
        last_wound_down_at_ms: Some(T0 - MIN),
        ..PlacementMemory::default()
    }];
    let schedule = world.now();
    assert!(!started(&schedule).contains(&"role:dispatcher".to_owned()));
    assert!(schedule.retry_at_ms.is_some());
    world.memory.clear();
    assert_eq!(started(&world.now())[0], "role:dispatcher");
}

#[test]
fn hs2_tk9adc_role_sessions_policy_validates_and_stays_out_of_old_files() {
    let base = || {
        let mut body = policy(&[(1, 2), (2, 2)]);
        body.placement.role_sessions = vec![TargetRoleSessions {
            target_id: target(1),
            roles: vec![D, I],
        }];
        body
    };
    assert!(
        base().problems(None).is_empty(),
        "{:?}",
        base().problems(None)
    );
    let codes = |body: &PolicyBody| {
        body.problems(None)
            .into_iter()
            .map(|problem| problem.code)
            .collect::<Vec<_>>()
    };
    let cases: Vec<PolicyCase> = vec![
        (
            "duplicate_role_sessions_target",
            Box::new(|body| {
                let again = body.placement.role_sessions[0].clone();
                body.placement.role_sessions.push(again);
            }),
        ),
        (
            "role_sessions_without_target",
            Box::new(|body| body.placement.role_sessions[0].target_id = target(9)),
        ),
        (
            "invalid_target",
            Box::new(|body| body.placement.role_sessions[0].target_id = "nope".into()),
        ),
        (
            "invalid_role_sessions",
            Box::new(|body| body.placement.role_sessions[0].roles = vec![D, D]),
        ),
    ];
    for (code, edit) in cases {
        let mut body = base();
        edit(&mut body);
        assert!(codes(&body).contains(&code), "{code}: {:?}", codes(&body));
    }
    // Spelled as the wire spells it, read back the same; unknown roles are refused.
    let json = serde_json::to_value(&base().placement).unwrap();
    assert_eq!(
        json["role_sessions"],
        serde_json::json!([{"target_id": target(1), "roles": ["dispatcher", "integrator"]}])
    );
    let back: PlacementPolicy = serde_json::from_value(json).unwrap();
    assert_eq!(back, base().placement);
    for wrong in ["worker", "planner", "Dispatcher"] {
        let mut json = serde_json::to_value(&base().placement).unwrap();
        json["role_sessions"][0]["roles"] = serde_json::json!([wrong]);
        assert!(
            serde_json::from_value::<PlacementPolicy>(json).is_err(),
            "{wrong}"
        );
    }
    // The default writes nothing, so a policy stored earlier reads back unchanged.
    let default = serde_json::to_value(PlacementPolicy::default()).unwrap();
    assert!(default.get("role_sessions").is_none());
    // HS2-R2N33M: an unlisted target is Auto (both roles, started only when they
    // help); a listed one keeps the person's choice, and an entry with no roles is a
    // valid choice of none that round-trips.
    assert_eq!(policy(&[(1, 1)]).role_sessions(&target(1)), [D, I]);
    assert_eq!(base().role_sessions(&target(1)), [D, I]);
    assert_eq!(base().role_sessions(&target(2)), [D, I]);
    let mut none = base();
    none.placement.role_sessions[0].roles.clear();
    assert!(none.problems(None).is_empty(), "{:?}", none.problems(None));
    assert!(none.role_sessions(&target(1)).is_empty());
    let json = serde_json::to_value(&none.placement).unwrap();
    assert_eq!(
        json["role_sessions"],
        serde_json::json!([{"target_id": target(1), "roles": []}])
    );
    let back: PlacementPolicy = serde_json::from_value(json).unwrap();
    assert_eq!(back, none.placement);
}

#[test]
fn hs2_tk9adc_the_pool_is_part_of_the_capacity_version_only_when_reported() {
    let plain = super::scheduler_tests::snapshot(1, 16_384, 0);
    let unreported = capacity_version(&plain);
    let reported = |pool: PoolState| capacity_version(&plain.clone().with_pool(Some(pool)));
    assert_eq!(capacity_version(&plain.clone().with_pool(None)), unreported);
    let empty = reported(PoolState::default());
    assert_ne!(empty, unreported);
    // Each fact changes it; the same facts give the same version.
    let changes = [
        PoolState {
            dispatcher_live: true,
            ..PoolState::default()
        },
        PoolState {
            integrator_live: true,
            ..PoolState::default()
        },
        PoolState {
            handoffs_pending: 1,
            ..PoolState::default()
        },
    ];
    for pool in changes {
        assert_ne!(reported(pool), empty, "{pool:?}");
        assert_eq!(reported(pool), reported(pool));
    }
    // An older peer's snapshot without the member still reads.
    let mut json = serde_json::to_value(&plain).unwrap();
    json.as_object_mut().unwrap().remove("pool");
    let read: CapacitySnapshot = serde_json::from_value(json).unwrap();
    assert_eq!(read.pool, None);
}
