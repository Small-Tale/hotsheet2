//! HS2-J89JC7: the orchestration timeline through the shared runtime against real git
//! ticket stores: labels from local stores, waves from the plan and runs, the
//! permission seam, the goal ticket's decision, the policy's kick-off answer and the
//! version long poll. Synthetic tickets only.
use hotsheet_model::{NoteKind, Ticket};
use hotsheet_ticketing::orchestration::{
    DecisionJournal, DecisionRecord, ItemState, PermissionAnswer, TargetAction, TargetLocation,
    Timeline, TimelinePhase, WaveKind,
};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::json;

use super::tickets::{Machine, NOW, bind_and_allow, human, machine, unsupported, worker};
use super::*;

fn target_ticket(m: &Machine, title: &str) -> Ticket {
    ops::create(
        &m.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn define(m: &Machine, goal: &Ticket, native: &Ticket, after: Option<&Ticket>) -> String {
    let prerequisites = after.map_or_else(
        || json!([]),
        |after| {
            json!([{"kind": "ticket", "target_id": id(1), "connection_id": m.target_connection(),
                "native_id": after.id.to_string(), "gates": "none"}])
        },
    );
    let request: DefinePlanStep = serde_json::from_value(json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": goal.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(),
        "native_id": native.id.to_string(), "prerequisites": prerequisites,
    }))
    .unwrap();
    let mut rng = Rng(300);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    m.runtime
        .readiness_define(&m.owner.id, request, Some(&worker()), &mut tx)
        .unwrap()
        .readiness
        .step
        .step_id
}

fn read(m: &Machine) -> Timeline {
    let mut rng = Rng(400);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    m.runtime
        .orchestration_timeline(&m.owner.id, None, 0, &mut tx)
        .unwrap()
}

fn wave(view: &Timeline, kind: WaveKind) -> Vec<&str> {
    view.waves
        .iter()
        .filter(|wave| wave.kind == kind)
        .flat_map(|wave| wave.items.iter().map(|item| item.step_id.as_str()))
        .collect()
}

fn set_policy(m: &Machine, automatic: bool, actor: Option<&str>) {
    let mut rng = Rng(500);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let current = m.runtime.policy(&m.owner.id).unwrap();
    let mut placement = json!({"automatic": automatic});
    if let Some(actor) = actor {
        placement["actor"] = json!(actor);
    }
    let request = serde_json::from_value(json!({
        "expected_revision": current.revision,
        "policy": {
            "tools": [{"tool": "claude", "max_sessions": 2}],
            "targets": [{"target_id": id(1), "actions": ["tickets_read", "work_start", "sessions_manage"],
                "tools": [{"tool": "claude"}]}],
            "placement": placement,
        },
    }))
    .unwrap();
    m.runtime
        .set_policy(
            &m.owner.id,
            request,
            Some(&super::tickets::human()),
            &mut tx,
        )
        .unwrap();
}

#[test]
fn hs2_j89jc7_timeline_reads_the_plan_runs_labels_and_kick_off() {
    let m = machine(10, false, unsupported);
    // Not an orchestration project yet.
    let empty = read(&m);
    assert_eq!(empty.phase, TimelinePhase::NoProjects);
    assert!(!empty.declared);
    assert!(empty.waves.is_empty() && empty.targets.is_empty());
    assert_eq!(empty.kick_off.start_blocked.as_deref(), Some("no_projects"));

    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let declared = read(&m);
    assert_eq!(declared.phase, TimelinePhase::NoPlan);
    assert!(declared.declared);
    assert_eq!(declared.targets.len(), 1);
    assert_eq!(declared.targets[0].location, TargetLocation::Local);
    assert_eq!(declared.targets[0].label, "Target");

    let goal = m.orchestration_ticket("Ship single sign-on");
    let (first, second) = (
        target_ticket(&m, "Add the token endpoint"),
        target_ticket(&m, "Add the login page"),
    );
    let a = define(&m, &goal, &first, None);
    let b = define(&m, &goal, &second, Some(&first));
    let proposed = read(&m);
    assert_eq!(proposed.phase, TimelinePhase::Proposed);
    assert_eq!(
        proposed.kick_off.start_blocked.as_deref(),
        Some("policy_absent")
    );
    assert_eq!(
        proposed
            .waves
            .iter()
            .map(|wave| (wave.kind, wave.ordinal))
            .collect::<Vec<_>>(),
        vec![(WaveKind::Planned, 0), (WaveKind::Planned, 1)]
    );
    let item = &proposed.waves[0].items[0];
    assert_eq!(item.step_id, a);
    assert_eq!(item.ticket.slug.as_deref(), Some(first.slug.as_str()));
    assert_eq!(item.ticket.title.as_deref(), Some("Add the token endpoint"));
    assert_eq!(item.goal_slug.as_deref(), Some(goal.slug.as_str()));
    assert_eq!(item.goal_title.as_deref(), Some("Ship single sign-on"));
    assert_eq!(item.state, ItemState::Ready);
    let waiting = &proposed.waves[1].items[0];
    assert_eq!(waiting.state, ItemState::WaitsForPrerequisite);
    assert_eq!(waiting.waits_on[0].step_id.as_deref(), Some(a.as_str()));
    assert_eq!(
        waiting.waits_on[0].slug.as_deref(),
        Some(first.slug.as_str())
    );

    // HS2-XYAHB4: a policy without a placement actor can be started, and so can one
    // that names one.
    set_policy(&m, false, None);
    let ready = read(&m).kick_off;
    assert!(ready.can_start && !ready.can_pause);
    assert!(ready.policy_revision.is_some());
    set_policy(&m, false, Some("worker-1"));
    assert!(read(&m).kick_off.can_start);

    // Dispatching the first step: work exists, placement is off, so it reads paused.
    let dispatch: DispatchRun = serde_json::from_value(json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": goal.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(),
        "native_id": first.id.to_string(), "operation_id": "op-1", "worker_id": "worker-1",
    }))
    .unwrap();
    m.runtime
        .run_dispatch(&m.owner.id, dispatch, Some(&worker()), &mut tx)
        .unwrap();
    let paused = read(&m);
    assert_eq!(paused.phase, TimelinePhase::Paused);
    assert_eq!(wave(&paused, WaveKind::WorkingNow), vec![a.as_str()]);
    assert_eq!(wave(&paused, WaveKind::Next), vec![b.as_str()]);
    let running = &paused.waves[1].items[0];
    assert_eq!(running.state, ItemState::Running);
    assert!(running.run.is_some());
    assert_eq!(paused.running_since_ms, Some(200));

    // Turning placement on, without naming an actor, is the running phase, which can
    // be paused (HS2-XYAHB4).
    set_policy(&m, true, None);
    let started = read(&m);
    assert_eq!(started.phase, TimelinePhase::Running);
    assert!(started.kick_off.can_pause && !started.kick_off.can_start);
    assert_eq!(started.kick_off.start_blocked.as_deref(), Some("running"));

    // HS2-J3ZF27: back to Software, the kept automatic policy does not read as running
    // and Start says why; forward again, it runs.
    let switch = |kind: &str, tx: &mut Transaction<'_>| {
        m.runtime
            .set_project_type(
                &m.owner.id,
                serde_json::from_value(json!({"project_type": kind, "keep_projects": true}))
                    .unwrap(),
                Some(&super::tickets::human()),
                tx,
            )
            .unwrap();
    };
    switch("software", &mut tx);
    let software = read(&m);
    assert_eq!(software.phase, TimelinePhase::Paused);
    assert_eq!(
        (
            software.kick_off.can_start,
            software.kick_off.can_pause,
            software.kick_off.start_blocked.as_deref()
        ),
        (false, false, Some("project_software"))
    );
    switch("orchestration", &mut tx);
    assert_eq!(read(&m).phase, TimelinePhase::Running);
}

#[test]
fn hs2_j89jc7_permission_seam_and_goal_decision() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::TicketsRead], &mut tx);
    let goal = m.orchestration_ticket("Coordinate");
    let work = target_ticket(&m, "Work");
    let step = define(&m, &goal, &work, None);
    // HS2-XHQ1EH: the scheduler's dispatch of the step's run waits on a person: the
    // gate files a permission request and the pass logs `permission_required` with
    // its id. The seam reports it as the permission request the card links to.
    let ask = |operation: &str, tx: &mut Transaction<'_>| match m.runtime.authorize(
        &m.reference(1),
        Some(&worker()),
        ActionRequest::new(TargetAction::WorkStart).operation(operation),
        tx,
    ) {
        Err(OrchestrationError::PermissionRequired { request_id }) => request_id,
        Err(other) => panic!("expected a permission request, got {other:?}"),
        Ok(_) => panic!("expected a permission request, the action was allowed"),
    };
    let log = |step_id: &str, operation: &str, request_id: &str| {
        let record: DecisionRecord = serde_json::from_value(json!({
            "at_ms": 150, "tick_id": id(900), "mode": "applied", "trigger": "event",
            "kind": "start", "actor_id": "worker-1", "target_id": id(1),
            "step_id": step_id, "outcome": "permission_required",
            "outcome_detail": request_id, "operation_id": operation,
        }))
        .unwrap();
        DecisionJournal::new(&m.home, &m.owner.id)
            .unwrap()
            .append(record)
            .unwrap();
    };
    let operation = format!("sched-run-{step}");
    let request_id = ask(&operation, &mut tx);
    log(&step, &operation, &request_id);
    let view = read(&m);
    let item = &view.waves[0].items[0];
    assert_eq!(item.state, ItemState::WaitsForPermission);
    let permission = item.permission.as_ref().unwrap();
    assert_eq!(
        (
            permission.kind.as_str(),
            permission.action.as_str(),
            permission.request_id.as_str(),
            permission.operation_id.as_deref()
        ),
        (
            "permission",
            "work_start",
            request_id.as_str(),
            Some(operation.as_str())
        )
    );
    // A request for another step is not this step's.
    let other = ask("sched-run-another", &mut tx);
    log("another", "sched-run-another", &other);
    assert_eq!(read(&m).counts.get("waits_for_permission"), Some(&1));

    // An unanswered feedback request on the goal ticket is recorded as the decision a
    // judgment call waits on; ready work is not held by it.
    ops::add_note(
        &m.owner_store,
        &goal.id,
        ulid::Ulid::new(),
        NOW.into(),
        NoteKind::FeedbackNeeded,
        "CHOICE: synthetic decision".into(),
    )
    .unwrap();
    let view = read(&m);
    assert_eq!(view.waves[0].items[0].state, ItemState::WaitsForPermission);
    assert!(view.waves[0].items[0].decision.is_none());

    // Once a person answers, the step no longer waits, before the next pass runs.
    m.runtime
        .permission_answer(
            &m.owner.id,
            &request_id,
            PermissionAnswer::AllowOnce,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let view = read(&m);
    assert_ne!(view.waves[0].items[0].state, ItemState::WaitsForPermission);
    assert!(view.waves[0].items[0].permission.is_none());
}

#[test]
fn hs2_j89jc7_a_version_waits_for_the_next_change() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let goal = m.orchestration_ticket("Coordinate");
    let work = target_ticket(&m, "Work");
    define(&m, &goal, &work, None);
    let first = read(&m);
    // A stale version answers at once; the current one waits to its deadline.
    let stale = m
        .runtime
        .orchestration_timeline(&m.owner.id, Some("000000000000"), 20_000, &mut tx)
        .unwrap();
    assert_eq!(stale.version, first.version);
    let started = std::time::Instant::now();
    let same = m
        .runtime
        .orchestration_timeline(&m.owner.id, Some(&first.version), 300, &mut tx)
        .unwrap();
    assert_eq!(same.version, first.version);
    assert!(started.elapsed() >= std::time::Duration::from_millis(250));
    // A dispatch from elsewhere ends the wait with the new version.
    let runtime = m.runtime.clone();
    let owner = m.owner.id.clone();
    let body = json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": goal.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(),
        "native_id": work.id.to_string(), "operation_id": "op-1", "worker_id": "worker-1",
    });
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut rng = Rng(900);
        let mut tx = Transaction {
            clock: &Clock,
            rng: &mut rng,
        };
        runtime
            .run_dispatch(
                &owner,
                serde_json::from_value(body).unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    });
    let started = std::time::Instant::now();
    let changed = m
        .runtime
        .orchestration_timeline(&m.owner.id, Some(&first.version), 15_000, &mut tx)
        .unwrap();
    writer.join().unwrap();
    assert_ne!(changed.version, first.version);
    assert_eq!(changed.phase, TimelinePhase::Paused);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    // A malformed version is refused by the router.
    assert!(
        m.runtime
            .request(
                "GET",
                &format!(
                    "/checkouts/{}/orchestration/timeline?after=a%20b&wait_ms=10",
                    m.owner.id
                ),
                &serde_json::Value::Null,
                &mut tx,
            )
            .is_err()
    );
}

fn lines(m: &Machine, checkout: &str) -> hotsheet_ticketing::orchestration::TargetLines {
    let mut rng = Rng(600);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    m.runtime
        .orchestration_target_lines(checkout, None, 0, &mut tx)
        .unwrap()
}

#[test]
fn hs2_s3fkj7_target_lines_read_the_owner_plan_from_the_target_side() {
    let m = machine(10, false, unsupported);
    // Nothing binds the target checkout yet; the owner itself is not a target either.
    let none = lines(&m, &m.target.id);
    assert!(!none.orchestrated && none.lines.is_empty());
    assert!(!lines(&m, &m.owner.id).orchestrated);

    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let bound = lines(&m, &m.target.id);
    assert!(bound.orchestrated && bound.lines.is_empty());
    assert_ne!(bound.version, none.version);
    // The owner is still not a target of itself.
    assert!(!lines(&m, &m.owner.id).orchestrated);

    let goal = m.orchestration_ticket("Ship single sign-on");
    let (first, second) = (
        target_ticket(&m, "Add the token endpoint"),
        target_ticket(&m, "Add the login page"),
    );
    let a = define(&m, &goal, &first, None);
    let b = define(&m, &goal, &second, Some(&first));
    let planned = lines(&m, &m.target.id);
    assert_eq!(planned.lines.len(), 2);
    let by_step = |step: &str| {
        planned
            .lines
            .iter()
            .find(|line| line.item.step_id == step)
            .unwrap()
    };
    let line = by_step(&a);
    assert_eq!(line.owner_checkout_id, m.owner.id);
    assert!(line.proposed);
    assert_eq!(line.item.target.native_id, first.id.to_string());
    assert_eq!(line.item.goal_slug.as_deref(), Some(goal.slug.as_str()));
    assert_eq!(line.item.state, ItemState::Ready);
    assert_eq!(by_step(&b).item.state, ItemState::WaitsForPrerequisite);
    assert_eq!(by_step(&b).also_planned, 0);
    // The same answer as the owner's timeline, item for item.
    let view = read(&m);
    for wave in &view.waves {
        for item in &wave.items {
            assert_eq!(&by_step(&item.step_id).item, item);
        }
    }

    // A dispatch moves the line; a read with the old version answers at once.
    let stale = planned.version.clone();
    let dispatch: DispatchRun = serde_json::from_value(json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": goal.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(),
        "native_id": first.id.to_string(), "operation_id": "op-1", "worker_id": "worker-1",
    }))
    .unwrap();
    m.runtime
        .run_dispatch(&m.owner.id, dispatch, Some(&worker()), &mut tx)
        .unwrap();
    let mut rng = Rng(700);
    let mut waiting = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let moved = m
        .runtime
        .orchestration_target_lines(&m.target.id, Some(&stale), 5_000, &mut waiting)
        .unwrap();
    assert_ne!(moved.version, stale);
    let running = moved
        .lines
        .iter()
        .find(|line| line.item.step_id == a)
        .unwrap();
    assert_eq!(running.item.state, ItemState::Running);
    assert!(!running.proposed);
    // The current version waits to its deadline and answers the same lines.
    let started = std::time::Instant::now();
    let same = m
        .runtime
        .orchestration_target_lines(&m.target.id, Some(&moved.version), 300, &mut waiting)
        .unwrap();
    assert_eq!(same.version, moved.version);
    assert!(started.elapsed() >= std::time::Duration::from_millis(250));
    // An unregistered checkout is refused, not read as unbound.
    assert!(
        m.runtime
            .orchestration_target_lines("unknown-0123456789ab", None, 0, &mut waiting)
            .is_err()
    );
}

/// Wait on the timeline at `version` while `change` runs on another thread after a
/// short delay; answers the version the wait returned and how long it took.
fn wait_through(
    m: &Machine,
    version: &str,
    change: impl FnOnce() + Send + 'static,
) -> (Timeline, std::time::Duration) {
    let mut rng = Rng(700);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        change();
    });
    let started = std::time::Instant::now();
    let view = m
        .runtime
        .orchestration_timeline(&m.owner.id, Some(version), MAX_TIMELINE_WAIT_MS, &mut tx)
        .unwrap();
    writer.join().unwrap();
    (view, started.elapsed())
}

/// Well before the wait's deadline, so a read that was not woken (and answered only at
/// its deadline) fails, while a loaded machine still passes (HS2-ANQXQ4).
const WOKEN_WITHIN: std::time::Duration = std::time::Duration::from_secs(15);

/// HS2-ANQXQ4: a person's answer ends a waiting timeline read at once, without a run
/// event or a scheduler decision: the answer to the permission request a step waits
/// on (the ledger's permission signal), and the response to the goal ticket's
/// decision note a step waits on (the owner's ticket store).
#[test]
fn hs2_anqxq4_a_persons_answer_wakes_a_waiting_timeline() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::TicketsRead], &mut tx);
    let goal = m.orchestration_ticket("Coordinate");
    let work = target_ticket(&m, "Work");
    let step = define(&m, &goal, &work, None);
    let operation = format!("sched-run-{step}");
    let request_id = match m.runtime.authorize(
        &m.reference(1),
        Some(&worker()),
        ActionRequest::new(TargetAction::WorkStart).operation(&operation),
        &mut tx,
    ) {
        Err(OrchestrationError::PermissionRequired { request_id }) => request_id,
        Err(other) => panic!("expected a permission request, got {other:?}"),
        Ok(_) => panic!("expected a permission request, the action was allowed"),
    };
    let record: DecisionRecord = serde_json::from_value(json!({
        "at_ms": 150, "tick_id": id(900), "mode": "applied", "trigger": "event",
        "kind": "start", "actor_id": "worker-1", "target_id": id(1),
        "step_id": step, "outcome": "permission_required",
        "outcome_detail": request_id, "operation_id": operation,
    }))
    .unwrap();
    DecisionJournal::new(&m.home, &m.owner.id)
        .unwrap()
        .append(record)
        .unwrap();
    let waiting = read(&m);
    assert_eq!(
        waiting.waves[0].items[0].state,
        ItemState::WaitsForPermission
    );
    // The owner's policy runs no scheduler, so nothing else would wake the read.
    let (runtime, owner, request) = (m.runtime.clone(), m.owner.id.clone(), request_id.clone());
    let (answered, took) = wait_through(&m, &waiting.version, move || {
        let mut rng = Rng(800);
        let mut tx = Transaction {
            clock: &Clock,
            rng: &mut rng,
        };
        runtime
            .permission_answer(
                &owner,
                &request,
                PermissionAnswer::AllowOnce,
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    });
    assert_ne!(answered.version, waiting.version);
    assert_ne!(
        answered.waves[0].items[0].state,
        ItemState::WaitsForPermission
    );
    assert!(took < WOKEN_WITHIN, "{took:?}");

    // A step whose readiness can no longer be confirmed (its target unbound) waits on
    // the goal ticket's decision while a feedback request is unanswered there.
    let request = ops::add_note(
        &m.owner_store,
        &goal.id,
        ulid::Ulid::new(),
        NOW.into(),
        NoteKind::FeedbackNeeded,
        "CHOICE: synthetic decision".into(),
    )
    .unwrap()
    .notes
    .last()
    .unwrap()
    .id;
    super::tickets::unbind(&m, 1, &mut tx);
    m.runtime
        .readiness_refresh(
            &m.owner.id,
            &RefreshReadiness::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let later_step = step.clone();
    let deciding = read(&m);
    let item = deciding
        .waves
        .iter()
        .flat_map(|wave| wave.items.iter())
        .find(|item| item.step_id == later_step)
        .unwrap();
    assert_eq!(item.state, ItemState::WaitsForDecision, "{:?}", item.state);
    assert_eq!(
        item.decision.as_ref().unwrap().note_id.as_deref(),
        Some(request.to_string().as_str())
    );
    let (store, goal_id) = (m.owner_store.clone(), goal.id);
    let (decided, took) = wait_through(&m, &deciding.version, move || {
        ops::add_note_with_metadata(
            &store,
            &goal_id,
            ulid::Ulid::new(),
            NOW.into(),
            NoteKind::Regular,
            ops::NoteMetadataInput {
                kind_explicit: true,
                response_to: Some(request.to_string()),
                ..Default::default()
            },
            "Go ahead".into(),
        )
        .unwrap();
    });
    assert_ne!(decided.version, deciding.version);
    let item = decided
        .waves
        .iter()
        .flat_map(|wave| wave.items.iter())
        .find(|item| item.step_id == later_step)
        .unwrap();
    assert_ne!(item.state, ItemState::WaitsForDecision);
    assert!(item.decision.is_none());
    assert!(took < WOKEN_WITHIN, "{took:?}");
}
