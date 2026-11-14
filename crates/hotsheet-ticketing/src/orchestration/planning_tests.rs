//! HS2-HCW7MD: the Propose Plan decision and planning state, as a table over every
//! planner state and as sequences of presses, launches, ends and host outages.
use super::*;

fn planner(op: &str, started: bool, ended: bool, alive: Option<bool>, at: u64) -> PlannerFacts {
    PlannerFacts {
        operation_id: op.into(),
        started,
        ended,
        alive,
        created_at_ms: at,
    }
}

fn fresh() -> String {
    "fresh".into()
}

/// Every shape one planner can have, with whether it is live, pending, and what a press
/// and a status read make of it.
#[test]
fn hs2_hcw7md_one_planner_in_every_state() {
    // (started, ended, alive, live, pending, state, press reuses)
    let table = [
        (
            false,
            false,
            None,
            false,
            true,
            PlanningState::Planning,
            true,
        ),
        (
            false,
            false,
            Some(false),
            false,
            true,
            PlanningState::Planning,
            true,
        ),
        (false, true, None, false, false, PlanningState::Ended, false),
        (
            true,
            false,
            Some(true),
            true,
            false,
            PlanningState::Planning,
            true,
        ),
        // The host could not be asked: never read as gone, so never doubled.
        (
            true,
            false,
            None,
            true,
            false,
            PlanningState::Planning,
            true,
        ),
        // The host says the terminal is gone before anyone recorded the end.
        (
            true,
            false,
            Some(false),
            false,
            false,
            PlanningState::Ended,
            false,
        ),
        (
            true,
            true,
            Some(false),
            false,
            false,
            PlanningState::Ended,
            false,
        ),
        (
            true,
            true,
            Some(true),
            false,
            false,
            PlanningState::Ended,
            false,
        ),
        (true, true, None, false, false, PlanningState::Ended, false),
    ];
    for (started, ended, alive, live, pending, state, reuses) in table {
        let facts = planner("op-a", started, ended, alive, 10);
        let case = format!("{started} {ended} {alive:?}");
        assert_eq!(facts.live(), live, "{case}");
        assert_eq!(facts.pending(), pending, "{case}");
        assert_eq!(
            planning_state(std::slice::from_ref(&facts)),
            state,
            "{case}"
        );
        assert_eq!(
            current_planner(std::slice::from_ref(&facts)),
            Some(&facts),
            "{case}"
        );
        let expected = if reuses {
            ProposeDecision::Reuse("op-a".into())
        } else {
            ProposeDecision::Start("fresh".into())
        };
        assert_eq!(decide_propose(&[facts], None, fresh), expected, "{case}");
    }
}

#[test]
fn hs2_hcw7md_nothing_planned_yet_is_idle_and_starts() {
    assert_eq!(planning_state(&[]), PlanningState::Idle);
    assert_eq!(current_planner(&[]), None);
    assert_eq!(
        decide_propose(&[], None, fresh),
        ProposeDecision::Start("fresh".into())
    );
    // A retained id the machine never saw is the new planner's id.
    assert_eq!(
        decide_propose(&[], Some("mine"), || unreachable!()),
        ProposeDecision::Start("mine".into())
    );
}

#[test]
fn hs2_hcw7md_a_retained_operation_id_is_always_that_planner() {
    let ended = planner("old", true, true, Some(false), 1);
    let live = planner("new", true, false, Some(true), 2);
    let both = [ended.clone(), live];
    // Repeating the old request answers with the old planner, never a new launch.
    assert_eq!(
        decide_propose(&both, Some("old"), || unreachable!()),
        ProposeDecision::Reuse("old".into())
    );
    // A new id while another planner works answers with the live one.
    assert_eq!(
        decide_propose(&both, Some("other"), || unreachable!()),
        ProposeDecision::Reuse("new".into())
    );
    // Once nothing is live, a new id starts.
    assert_eq!(
        decide_propose(&[ended], Some("other"), || unreachable!()),
        ProposeDecision::Start("other".into())
    );
}

/// Walk presses through a planner's life: press, repeated press, launch, host outage,
/// end, re-propose, and a second planner after the first ended.
#[test]
fn hs2_hcw7md_presses_across_a_planners_life() {
    let mut planners: Vec<PlannerFacts> = Vec::new();
    let press = |planners: &mut Vec<PlannerFacts>, at: u64| {
        let decision = decide_propose(planners, None, || format!("op-{at}"));
        if let ProposeDecision::Start(op) = &decision {
            planners.push(planner(op, false, false, None, at));
        }
        decision
    };
    assert_eq!(
        press(&mut planners, 1),
        ProposeDecision::Start("op-1".into())
    );
    // Pressed again before it launched: the same reservation is repeated.
    assert_eq!(
        press(&mut planners, 2),
        ProposeDecision::Reuse("op-1".into())
    );
    planners[0].started = true;
    planners[0].alive = Some(true);
    assert_eq!(planning_state(&planners), PlanningState::Planning);
    // Pressed repeatedly while it works: still one planner.
    for at in 3..6 {
        assert_eq!(
            press(&mut planners, at),
            ProposeDecision::Reuse("op-1".into())
        );
    }
    // The host cannot be asked: nothing is doubled.
    planners[0].alive = None;
    assert_eq!(
        press(&mut planners, 6),
        ProposeDecision::Reuse("op-1".into())
    );
    // It finished and its terminal is gone.
    planners[0].alive = Some(false);
    assert_eq!(planning_state(&planners), PlanningState::Ended);
    planners[0].ended = true;
    assert_eq!(planning_state(&planners), PlanningState::Ended);
    // Re-proposing starts the next planner; the earlier one stays in history.
    assert_eq!(
        press(&mut planners, 7),
        ProposeDecision::Start("op-7".into())
    );
    assert_eq!(planners.len(), 2);
    assert_eq!(planning_state(&planners), PlanningState::Planning);
    assert_eq!(
        current_planner(&planners).map(|p| p.operation_id.as_str()),
        Some("op-7")
    );
    // And once it ended too, the newest ended one is the one shown.
    planners[1].started = true;
    planners[1].ended = true;
    assert_eq!(planning_state(&planners), PlanningState::Ended);
    assert_eq!(
        current_planner(&planners).map(|p| p.operation_id.as_str()),
        Some("op-7")
    );
}

/// The records come in any order: the answer does not depend on it.
#[test]
fn hs2_hcw7md_the_answer_does_not_depend_on_record_order() {
    let records = [
        planner("a", true, true, Some(false), 5),
        planner("b", true, false, Some(true), 3),
        planner("c", true, true, None, 9),
    ];
    let mut orders = vec![records.to_vec()];
    let mut reversed = records.to_vec();
    reversed.reverse();
    orders.push(reversed);
    orders.push(vec![
        records[1].clone(),
        records[2].clone(),
        records[0].clone(),
    ]);
    for order in orders {
        assert_eq!(planning_state(&order), PlanningState::Planning);
        assert_eq!(
            current_planner(&order).map(|p| p.operation_id.as_str()),
            Some("b")
        );
        assert_eq!(
            decide_propose(&order, None, fresh),
            ProposeDecision::Reuse("b".into())
        );
    }
    // Without a live one, the newest ended planner is shown; equal times break on
    // the operation id so the order is total.
    let ended = [
        planner("x", true, true, None, 4),
        planner("z", true, true, None, 4),
        planner("y", true, true, None, 2),
    ];
    for rotation in 0..3 {
        let mut order = ended.to_vec();
        order.rotate_left(rotation);
        assert_eq!(
            current_planner(&order).map(|p| p.operation_id.as_str()),
            Some("z")
        );
    }
}

#[test]
fn hs2_hcw7md_states_serialize_as_their_codes() {
    for state in PlanningState::ALL {
        assert_eq!(serde_json::to_value(state).unwrap(), state.code());
    }
    for (error, code) in [
        (ReadinessError::NoTargets, "plan_no_targets"),
        (ReadinessError::NoGoals, "plan_no_goals"),
        (ReadinessError::GoalNotOpen, "plan_goal_not_open"),
    ] {
        assert_eq!(error.code(), code);
        assert_eq!(OrchestrationError::Readiness(error).code(), code);
    }
}

/// The planner is a role of its own: its skill, never a target role, and refused by
/// a hosted start that names a run.
#[test]
fn hs2_hcw7md_the_planner_role_is_not_a_target_role() {
    assert_eq!(
        SessionRole::from_code("planner"),
        Some(SessionRole::Planner)
    );
    assert_eq!(SessionRole::Planner.skill(), "hotsheet-planner");
    assert!(SessionRole::Planner.is_role_session());
    for role in SessionRole::ALL {
        assert_eq!(SessionRole::from_code(role.code()), Some(role));
        assert_eq!(role.is_target_role(), role != SessionRole::Planner);
    }
    assert_eq!(SessionRole::from_code("orchestrator"), None);
    let mut hosted = NewHostedSession {
        account: None,
        operation_id: "plan-1".into(),
        tool: "claude".into(),
        model: None,
        effort: None,
        role: SessionRole::Planner,
        origin: SessionOrigin {
            owner_checkout_id: "owner-1".into(),
            project: "Launch".into(),
            run_id: None,
            ticket: None,
        },
    };
    hosted.validate().unwrap();
    hosted.origin.run_id = Some("01M7BX05RR4PGW2D840ZSRV3HZ".into());
    assert!(matches!(
        hosted.validate(),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(
        session_label("Launch", None, SessionRole::Planner),
        "Launch: planner"
    );
}
