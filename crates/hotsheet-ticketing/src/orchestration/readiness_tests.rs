//! HS2-42TARA readiness levels, plan graph and plan registry sequences; synthetic
//! homes, ids and commit ids only.

use super::*;

struct Clock(std::cell::Cell<u64>);
impl crate::Clock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}
struct Rng(u128);
impl crate::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}
const OWNER: &str = "owner-0123456789ab";
const OTHER: &str = "owner-ba9876543210";
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn sha(digit: char) -> String {
    std::iter::repeat_n(digit, 40).collect()
}
/// A ticket of target project `project`.
fn ticket(project: u128, native: &str) -> RunTarget {
    RunTarget {
        target_id: id(project),
        connection_id: "tickets".into(),
        native_id: native.into(),
    }
}
fn orchestration_ticket() -> RunTicket {
    RunTicket {
        connection_id: "plan".into(),
        native_id: id(900),
    }
}
fn requires(target: &RunTarget, gates: GateRequirement) -> Prerequisite {
    Prerequisite::Ticket {
        target_id: target.target_id.clone(),
        connection_id: target.connection_id.clone(),
        native_id: target.native_id.clone(),
        gates,
        commit: None,
    }
}
fn validated(step: &str) -> Prerequisite {
    Prerequisite::Validation {
        ticket: orchestration_ticket(),
        step_id: step.into(),
    }
}
fn step(number: u128, target: &RunTarget, prerequisites: Vec<Prerequisite>) -> PlanStep {
    PlanStep {
        model: None,
        step_id: id(number),
        owner_checkout_id: OWNER.into(),
        ticket: orchestration_ticket(),
        target: target.clone(),
        prerequisites,
        resources: Vec::new(),
        tool: None,
        revision: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        updated_by: None,
    }
}
fn seen(status: &str, at_ms: u64) -> TicketSeen {
    TicketSeen {
        at_ms,
        exists: true,
        status: status.into(),
        priority: None,
        up_next: None,
        created_at_ms: None,
        opted_out: None,
        slug: None,
    }
}
fn down(since_ms: u64) -> RunUnreachable {
    RunUnreachable {
        since_ms,
        reason: "target_unavailable".into(),
    }
}
fn gate(ci: GateState, reason: Option<&str>, deployment: Option<GateState>) -> GateFacts {
    GateFacts {
        ci,
        ci_reason: reason.map(Into::into),
        deployment,
        commits: if reason == Some("no_gate") {
            Vec::new()
        } else {
            vec![sha('a')]
        },
        gate_ids: Vec::new(),
    }
}
#[derive(Default)]
struct World(PlanFacts);
impl World {
    fn status(mut self, target: &RunTarget, status: &str) -> Self {
        self.0.tickets.insert(
            ticket_key(target),
            TicketFacts {
                observed: Some(seen(status, 10)),
                unreachable: None,
            },
        );
        self
    }
    fn facts(mut self, target: &RunTarget, facts: TicketFacts) -> Self {
        self.0.tickets.insert(ticket_key(target), facts);
        self
    }
    fn gates(mut self, target: &RunTarget, facts: GateFacts) -> Self {
        self.0.gates.insert(ticket_key(target), facts);
        self
    }
    fn active(mut self, target: &RunTarget) -> Self {
        self.0.active.insert(ticket_key(target));
        self
    }
    fn validation(mut self, step: &str, state: ValidationState, reason: Option<&str>) -> Self {
        let ticket = orchestration_ticket();
        self.0.validations.insert(
            (ticket.connection_id, ticket.native_id, step.into()),
            (state, reason.map(Into::into)),
        );
        self
    }
}
fn level_of(answers: &[StepReadiness], number: u128) -> ReadinessLevel {
    answers
        .iter()
        .find(|answer| answer.step.step_id == id(number))
        .unwrap()
        .level
}
fn codes(answer: &StepReadiness) -> Vec<&str> {
    answer
        .reasons
        .iter()
        .map(|reason| reason.code.as_str())
        .collect()
}

#[test]
fn hs2_42tara_levels_have_stable_codes_and_only_unready_levels_refuse_dispatch() {
    use ReadinessLevel as L;
    let table = [
        (L::Confirmed, "confirmed", true, None),
        (
            L::Provisional,
            "provisional",
            true,
            Some("readiness_provisional"),
        ),
        (L::Blocked, "blocked", false, Some("readiness_blocked")),
        (L::Stale, "stale", false, Some("readiness_stale")),
        (L::Cyclic, "cyclic", false, Some("readiness_cyclic")),
        (L::InProgress, "in_progress", false, None),
        (L::Finished, "finished", false, None),
    ];
    assert_eq!(table.len(), L::ALL.len());
    for (level, code, ready, refusal) in table {
        assert_eq!(level.code(), code);
        assert_eq!(serde_json::to_value(level).unwrap(), code);
        assert_eq!(level.is_ready(), ready, "{code}");
        assert_eq!(level.refusal().map(ReadinessError::code), refusal, "{code}");
    }
    assert_eq!(
        OrchestrationError::Readiness(ReadinessError::StepNotFound).code(),
        "readiness_step_not_found"
    );
    assert_eq!(
        ReadinessError::OverrideDenied.code(),
        "readiness_override_denied"
    );
}

#[test]
fn hs2_42tara_ticket_prerequisite_table_covers_every_gate_state() {
    use GateRequirement as R;
    use GateState as G;
    use ReadinessLevel as L;
    let (a, b) = (ticket(1, "a"), ticket(2, "b"));
    // (prerequisite status, requirement, gates of its latest run) -> level and reason.
    type Row = (&'static str, R, Option<GateFacts>, L, Option<&'static str>);
    let table: Vec<Row> = vec![
        ("completed", R::None, None, L::Confirmed, None),
        ("verified", R::None, None, L::Confirmed, None),
        ("archive", R::None, None, L::Confirmed, None),
        (
            "started",
            R::None,
            None,
            L::Blocked,
            Some("prerequisite_not_finished"),
        ),
        // No gate requirement ignores a failed gate: the plan asked for the ticket only.
        (
            "completed",
            R::None,
            Some(gate(G::Failed, None, None)),
            L::Confirmed,
            None,
        ),
        (
            "completed",
            R::Ci,
            Some(gate(G::Passed, None, None)),
            L::Confirmed,
            None,
        ),
        (
            "completed",
            R::Ci,
            None,
            L::Blocked,
            Some("gate_not_linked"),
        ),
        (
            "completed",
            R::Ci,
            Some(gate(G::Unknown, Some("no_gate"), None)),
            L::Blocked,
            Some("gate_not_linked"),
        ),
        (
            "completed",
            R::Ci,
            Some(gate(G::Unknown, Some("watch_lost"), None)),
            L::Stale,
            Some("gate_unknown"),
        ),
        (
            "completed",
            R::Ci,
            Some(gate(G::Failed, None, None)),
            L::Blocked,
            Some("gate_failed"),
        ),
        (
            "completed",
            R::Ci,
            Some(gate(G::Pending, None, None)),
            L::Provisional,
            Some("gate_pending"),
        ),
        // Integrated and awaiting CI while its ticket is still open: provisional.
        (
            "started",
            R::Ci,
            Some(gate(G::Pending, None, None)),
            L::Provisional,
            Some("gate_pending"),
        ),
        // Green commit, ticket not closed yet: still only provisional.
        (
            "started",
            R::Ci,
            Some(gate(G::Passed, None, None)),
            L::Provisional,
            Some("prerequisite_not_finished"),
        ),
        (
            "started",
            R::Ci,
            Some(gate(G::Failed, None, None)),
            L::Blocked,
            Some("gate_failed"),
        ),
        (
            "started",
            R::Ci,
            None,
            L::Blocked,
            Some("prerequisite_not_finished"),
        ),
        (
            "started",
            R::Ci,
            Some(gate(G::Unknown, Some("watch_lost"), None)),
            L::Blocked,
            Some("prerequisite_not_finished"),
        ),
        (
            "completed",
            R::Deployment,
            Some(gate(G::Passed, None, Some(G::Passed))),
            L::Confirmed,
            None,
        ),
        (
            "completed",
            R::Deployment,
            Some(gate(G::Passed, None, None)),
            L::Blocked,
            Some("deployment_not_requested"),
        ),
        (
            "completed",
            R::Deployment,
            Some(gate(G::Passed, None, Some(G::Pending))),
            L::Blocked,
            Some("deployment_pending"),
        ),
        (
            "completed",
            R::Deployment,
            Some(gate(G::Passed, None, Some(G::AwaitingApproval))),
            L::Blocked,
            Some("deployment_pending"),
        ),
        (
            "completed",
            R::Deployment,
            Some(gate(G::Passed, None, Some(G::Failed))),
            L::Blocked,
            Some("deployment_failed"),
        ),
        (
            "completed",
            R::Deployment,
            Some(gate(G::Passed, None, Some(G::Unknown))),
            L::Stale,
            Some("deployment_unknown"),
        ),
        (
            "completed",
            R::Deployment,
            Some(gate(G::Pending, None, Some(G::Passed))),
            L::Provisional,
            Some("gate_pending"),
        ),
        (
            "started",
            R::Deployment,
            Some(gate(G::Passed, None, Some(G::Passed))),
            L::Provisional,
            Some("prerequisite_not_finished"),
        ),
    ];
    for (row, (status, requirement, gates, level, reason)) in table.into_iter().enumerate() {
        let mut world = World::default()
            .status(&a, status)
            .status(&b, "not_started");
        if let Some(gates) = gates {
            world = world.gates(&a, gates);
        }
        let answers = evaluate_plan(&[step(2, &b, vec![requires(&a, requirement)])], &world.0);
        assert_eq!(answers[0].level, level, "row {row}");
        assert_eq!(codes(&answers[0]).first().copied(), reason, "row {row}");
        // A provisional answer always records the base commit it rests on.
        assert_eq!(
            answers[0].base_commits,
            if level == L::Provisional {
                vec![sha('a')]
            } else {
                Vec::<String>::new()
            },
            "row {row}"
        );
    }
}

#[test]
fn hs2_42tara_named_commit_is_judged_by_that_commits_gates() {
    let (a, b) = (ticket(1, "a"), ticket(2, "b"));
    let pinned = |commit: char| Prerequisite::Ticket {
        target_id: a.target_id.clone(),
        connection_id: a.connection_id.clone(),
        native_id: a.native_id.clone(),
        gates: GateRequirement::Ci,
        commit: Some(sha(commit)),
    };
    let mut world = World::default()
        .status(&a, "completed")
        .status(&b, "not_started")
        // The latest run's gates passed, but for another commit.
        .gates(&a, gate(GateState::Passed, None, None));
    world.0.commit_gates.insert(
        (a.target_id.clone(), sha('c')),
        GateFacts {
            ci: GateState::Pending,
            ci_reason: None,
            deployment: None,
            commits: vec![sha('c')],
            gate_ids: Vec::new(),
        },
    );
    let answers = evaluate_plan(&[step(2, &b, vec![pinned('c')])], &world.0);
    assert_eq!(answers[0].level, ReadinessLevel::Provisional);
    assert_eq!(answers[0].base_commits, vec![sha('c')]);
    assert_eq!(answers[0].reasons[0].commits, vec![sha('c')]);
    // A commit nothing is linked for is not green by association.
    let answers = evaluate_plan(&[step(2, &b, vec![pinned('d')])], &world.0);
    assert_eq!(answers[0].level, ReadinessLevel::Blocked);
    assert_eq!(codes(&answers[0]), ["gate_not_linked"]);
}

#[test]
fn hs2_42tara_validation_prerequisite_table() {
    use ReadinessLevel as L;
    use ValidationState as V;
    let b = ticket(2, "b");
    let table = [
        (Some((V::Passed, None)), L::Confirmed, None),
        (
            Some((V::Failed, Some("exit_code_mismatch"))),
            L::Blocked,
            Some("validation_failed"),
        ),
        (
            Some((V::Running, None)),
            L::Blocked,
            Some("validation_running"),
        ),
        (
            Some((V::Skipped, Some("prerequisite_not_passed"))),
            L::Blocked,
            Some("validation_skipped"),
        ),
        (
            Some((V::Unavailable, Some("program_not_found"))),
            L::Blocked,
            Some("validation_unavailable"),
        ),
        // HS2-17ABM2: a cancelled step proves nothing and blocks until run again.
        (
            Some((V::Cancelled, Some("cancelled"))),
            L::Blocked,
            Some("validation_cancelled"),
        ),
        (
            Some((V::Unverified, Some("not_run"))),
            L::Blocked,
            Some("validation_not_run"),
        ),
        (
            Some((V::Unverified, Some("step_changed"))),
            L::Blocked,
            Some("validation_step_changed"),
        ),
        (
            Some((V::Unverified, Some("deadline_passed"))),
            L::Stale,
            Some("validation_unverified"),
        ),
        (
            Some((V::Unverified, Some("execution_lost"))),
            L::Stale,
            Some("validation_unverified"),
        ),
        (None, L::Blocked, Some("validation_step_missing")),
    ];
    for (row, (evidence, level, reason)) in table.into_iter().enumerate() {
        let mut world = World::default().status(&b, "not_started");
        if let Some((state, why)) = evidence {
            world = world.validation("smoke", state, why);
        }
        let answers = evaluate_plan(&[step(2, &b, vec![validated("smoke")])], &world.0);
        assert_eq!(answers[0].level, level, "row {row}");
        assert_eq!(codes(&answers[0]).first().copied(), reason, "row {row}");
    }
}

#[test]
fn hs2_42tara_the_steps_own_ticket_decides_before_its_prerequisites() {
    use ReadinessLevel as L;
    let (a, b) = (ticket(1, "a"), ticket(2, "b"));
    let plan = [step(2, &b, vec![requires(&a, GateRequirement::None)])];
    let base = || World::default().status(&a, "started");
    // An unfinished run on the ticket outranks everything, even unreadable facts.
    assert_eq!(
        evaluate_plan(&plan, &base().active(&b).0)[0].level,
        L::InProgress
    );
    assert_eq!(
        evaluate_plan(&plan, &base().status(&b, "completed").0)[0].level,
        L::Finished
    );
    // Never read.
    let answer = &evaluate_plan(&plan, &base().0)[0];
    assert_eq!(
        (answer.level, codes(answer)),
        (L::Stale, vec!["not_observed"])
    );
    // Read once, unreachable since: the last read is not confirmed state, even when
    // it said the ticket was finished.
    for status in ["not_started", "completed"] {
        let stale = TicketFacts {
            observed: Some(seen(status, 10)),
            unreachable: Some(down(20)),
        };
        let answer = &evaluate_plan(&plan, &base().facts(&b, stale).0)[0];
        assert_eq!(
            (answer.level, codes(answer)),
            (L::Stale, vec!["target_unreachable"])
        );
    }
    let missing = TicketFacts {
        observed: Some(TicketSeen {
            exists: false,
            status: String::new(),
            ..seen("", 10)
        }),
        unreachable: None,
    };
    let answer = &evaluate_plan(&plan, &base().facts(&b, missing.clone()).0)[0];
    assert_eq!(
        (answer.level, codes(answer)),
        (L::Blocked, vec!["target_ticket_missing"])
    );
    // A prerequisite ticket the target no longer has blocks; one never read or
    // unreachable is stale.
    let own = || World::default().status(&b, "not_started");
    let answer = &evaluate_plan(&plan, &own().facts(&a, missing).0)[0];
    assert_eq!(
        (answer.level, codes(answer)),
        (L::Blocked, vec!["prerequisite_ticket_missing"])
    );
    let answer = &evaluate_plan(&plan, &own().0)[0];
    assert_eq!(
        (answer.level, codes(answer)),
        (L::Stale, vec!["not_observed"])
    );
    let unreachable = TicketFacts {
        observed: Some(seen("completed", 10)),
        unreachable: Some(down(11)),
    };
    let answer = &evaluate_plan(&plan, &own().facts(&a, unreachable).0)[0];
    assert_eq!(
        (answer.level, codes(answer)),
        (L::Stale, vec!["target_unreachable"])
    );
}

#[test]
fn hs2_42tara_the_worst_prerequisite_sets_the_level_and_every_reason_is_listed() {
    use ReadinessLevel as L;
    let (a, b, c, d) = (
        ticket(1, "a"),
        ticket(1, "b"),
        ticket(2, "c"),
        ticket(3, "d"),
    );
    let plan = [step(
        9,
        &d,
        vec![
            requires(&a, GateRequirement::Ci),
            requires(&b, GateRequirement::None),
            requires(&c, GateRequirement::None),
            validated("smoke"),
        ],
    )];
    let world = |b_status: &str, c_known: bool| {
        let mut world = World::default()
            .status(&d, "not_started")
            .status(&a, "completed")
            .gates(&a, gate(GateState::Pending, None, None))
            .status(&b, b_status)
            .validation("smoke", ValidationState::Passed, None);
        if c_known {
            world = world.status(&c, "completed");
        }
        world.0
    };
    // provisional + blocked + stale: blocked, with all three reasons in plan order.
    let answer = &evaluate_plan(&plan, &world("started", false))[0];
    assert_eq!(answer.level, L::Blocked);
    assert_eq!(
        codes(answer),
        ["gate_pending", "prerequisite_not_finished", "not_observed"]
    );
    assert!(answer.base_commits.is_empty());
    // provisional + stale: stale.
    assert_eq!(
        evaluate_plan(&plan, &world("completed", false))[0].level,
        L::Stale
    );
    // provisional alone: provisional, and each reason names its prerequisite.
    let answer = &evaluate_plan(&plan, &world("completed", true))[0];
    assert_eq!(answer.level, L::Provisional);
    assert_eq!(answer.reasons.len(), 1);
    assert_eq!(
        answer.reasons[0].prerequisite.as_ref(),
        Some(&plan[0].prerequisites[0])
    );
    // A step with no prerequisites is confirmed as soon as its ticket is read.
    let answer = &evaluate_plan(&[step(9, &d, Vec::new())], &world("completed", true))[0];
    assert_eq!((answer.level, answer.reasons.len()), (L::Confirmed, 0));
}

#[test]
fn hs2_42tara_cycles_across_targets_are_found_and_never_ready() {
    use ReadinessLevel as L;
    let (a, b, c, d, e) = (
        ticket(1, "a"),
        ticket(2, "b"),
        ticket(3, "c"),
        ticket(4, "d"),
        ticket(5, "e"),
    );
    let none = GateRequirement::None;
    // a -> b -> c -> a across three target projects; d requires a; e is independent.
    let plan = [
        step(1, &a, vec![requires(&b, none)]),
        step(2, &b, vec![requires(&c, none)]),
        step(3, &c, vec![requires(&a, none)]),
        step(4, &d, vec![requires(&a, none)]),
        step(5, &e, Vec::new()),
    ];
    let mut world = World::default();
    for target in [&a, &b, &c, &d, &e] {
        world = world.status(target, "not_started");
    }
    let answers = evaluate_plan(&plan, &world.0);
    for member in 1..=3 {
        let answer = &answers[member - 1];
        assert_eq!(answer.level, L::Cyclic);
        assert_eq!(codes(answer), ["plan_cycle"]);
        assert_eq!(answer.cycle, [id(1), id(2), id(3)]);
    }
    assert_eq!(answers[3].level, L::Blocked);
    assert_eq!(codes(&answers[3]), ["prerequisite_in_cycle"]);
    assert!(answers[3].cycle.is_empty());
    assert_eq!(answers[4].level, L::Confirmed);

    // Even with every ticket finished elsewhere, an unfinished member stays cyclic:
    // the plan is wrong, whatever the world says.
    let mut world = World::default()
        .status(&a, "not_started")
        .status(&d, "not_started");
    for target in [&b, &c, &e] {
        world = world.status(target, "completed");
    }
    let answers = evaluate_plan(&plan, &world.0);
    assert_eq!(level_of(&answers, 1), L::Cyclic);
    assert_eq!(level_of(&answers, 2), L::Finished);
    // Removing one edge dissolves the cycle.
    let mut fixed = plan.to_vec();
    fixed[2].prerequisites.clear();
    let answers = evaluate_plan(&fixed, &world.0);
    assert_eq!(level_of(&answers, 1), L::Confirmed);
    assert!(answers.iter().all(|answer| answer.cycle.is_empty()));

    // A two-step cycle, and a prerequisite on a ticket no step works is not an edge.
    let pair = [
        step(1, &a, vec![requires(&b, none), requires(&e, none)]),
        step(2, &b, vec![requires(&a, none)]),
    ];
    let answers = evaluate_plan(&pair, &world.0);
    assert_eq!(answers[0].cycle, [id(1), id(2)]);
    assert_eq!(answers[1].level, L::Finished);
}

#[test]
fn hs2_42tara_diamond_walks_from_blocked_to_confirmed_and_counts_what_it_unblocks() {
    use ReadinessLevel as L;
    let (a, b, c, d) = (
        ticket(1, "a"),
        ticket(2, "b"),
        ticket(2, "c"),
        ticket(3, "d"),
    );
    let ci = GateRequirement::Ci;
    let plan = [
        step(1, &a, Vec::new()),
        step(2, &b, vec![requires(&a, ci)]),
        step(3, &c, vec![requires(&a, ci)]),
        step(4, &d, vec![requires(&b, ci), requires(&c, ci)]),
    ];
    let levels = |world: &World| -> Vec<L> {
        evaluate_plan(&plan, &world.0)
            .iter()
            .map(|answer| answer.level)
            .collect()
    };
    let start = || {
        World::default()
            .status(&b, "not_started")
            .status(&c, "not_started")
            .status(&d, "not_started")
    };
    let world = start().status(&a, "not_started");
    assert_eq!(
        levels(&world),
        [L::Confirmed, L::Blocked, L::Blocked, L::Blocked]
    );
    let answers = evaluate_plan(&plan, &world.0);
    assert_eq!(answers[0].unblocks, [id(2), id(3)]);
    assert_eq!(answers[1].unblocks, [id(4)]);
    assert_eq!(answers[2].unblocks, [id(4)]);
    assert!(answers[3].unblocks.is_empty());

    // a is dispatched, integrated with CI pending, then green, then closed.
    let world = start().status(&a, "started").active(&a);
    assert_eq!(
        levels(&world),
        [L::InProgress, L::Blocked, L::Blocked, L::Blocked]
    );
    let pending = world.gates(&a, gate(GateState::Pending, None, None));
    assert_eq!(
        levels(&pending),
        [L::InProgress, L::Provisional, L::Provisional, L::Blocked]
    );
    let green = start()
        .status(&a, "completed")
        .gates(&a, gate(GateState::Passed, None, None));
    assert_eq!(
        levels(&green),
        [L::Finished, L::Confirmed, L::Confirmed, L::Blocked]
    );
    // b done and green, c done with a failed gate: d is blocked by c alone.
    let half = start()
        .status(&a, "completed")
        .gates(&a, gate(GateState::Passed, None, None))
        .status(&b, "completed")
        .gates(&b, gate(GateState::Passed, None, None))
        .status(&c, "completed")
        .gates(&c, gate(GateState::Failed, None, None));
    let answers = evaluate_plan(&plan, &half.0);
    assert_eq!(answers[3].level, L::Blocked);
    assert_eq!(codes(&answers[3]), ["gate_failed"]);
    // A finished step unblocks nothing further by being listed, and finished
    // dependents are not counted.
    assert_eq!(answers[0].unblocks, Vec::<String>::new());
    assert_eq!(answers[1].unblocks, [id(4)]);
    let done = half.gates(&c, gate(GateState::Passed, None, None));
    assert_eq!(
        levels(&done),
        [L::Finished, L::Finished, L::Finished, L::Confirmed]
    );
}

#[test]
fn hs2_42tara_evaluation_is_deterministic_whatever_order_steps_arrive_in() {
    let (a, b, c) = (ticket(1, "a"), ticket(2, "b"), ticket(3, "c"));
    let plan = vec![
        step(3, &c, vec![requires(&b, GateRequirement::None)]),
        step(1, &a, Vec::new()),
        step(2, &b, vec![requires(&a, GateRequirement::None)]),
    ];
    let world = World::default()
        .status(&a, "completed")
        .status(&b, "started")
        .status(&c, "not_started");
    let expected = evaluate_plan(&plan, &world.0);
    assert_eq!(
        expected
            .iter()
            .map(|answer| answer.step.step_id.clone())
            .collect::<Vec<_>>(),
        [id(1), id(2), id(3)]
    );
    let mut reversed = plan.clone();
    reversed.reverse();
    assert_eq!(evaluate_plan(&reversed, &world.0), expected);
    assert_eq!(evaluate_plan(&plan, &world.0), expected);
    assert!(evaluate_plan(&[], &world.0).is_empty());
}

#[test]
fn hs2_42tara_merging_two_accounts_of_a_ticket_keeps_the_newest_truth() {
    let read = |status: &str, at_ms| TicketFacts {
        observed: Some(seen(status, at_ms)),
        unreachable: None,
    };
    let outage = |since_ms| TicketFacts {
        observed: None,
        unreachable: Some(down(since_ms)),
    };
    // The newer read wins in either order.
    for (left, right) in [
        (read("started", 10), read("completed", 20)),
        (read("completed", 20), read("started", 10)),
    ] {
        assert_eq!(left.merge(right).observed.unwrap().status, "completed");
    }
    // Two reads in the same instant: the account passed second is the live one.
    assert_eq!(
        read("started", 10)
            .merge(read("completed", 10))
            .observed
            .unwrap()
            .status,
        "completed"
    );
    // An outage that began after the newest read makes it stale; one that began
    // before it was already over when the read succeeded.
    assert!(read("started", 10).merge(outage(10)).unreachable.is_some());
    assert!(read("started", 10).merge(outage(11)).unreachable.is_some());
    assert!(read("started", 10).merge(outage(9)).unreachable.is_none());
    assert!(outage(9).merge(read("started", 10)).unreachable.is_none());
    // Nothing read at all: the outage stands.
    assert!(
        TicketFacts::default()
            .merge(outage(1))
            .unreachable
            .is_some()
    );
    assert_eq!(outage(5).merge(outage(7)).unreachable, Some(down(7)));
    assert_eq!(
        TicketFacts::default().merge(TicketFacts::default()),
        TicketFacts::default()
    );
    // Priority inputs known to the older read survive a newer read without them.
    let mut rich = read("not_started", 10);
    let observed = rich.observed.as_mut().unwrap();
    observed.priority = Some("high".into());
    observed.up_next = Some(true);
    observed.created_at_ms = Some(3);
    let merged = rich.merge(read("started", 20)).observed.unwrap();
    assert_eq!(merged.status, "started");
    assert_eq!(merged.priority.as_deref(), Some("high"));
    assert_eq!(merged.created_at_ms, Some(3));
    assert_eq!(merged.up_next, Some(true));
    // A newer read that does say wins.
    let mut newer = read("started", 30);
    newer.observed.as_mut().unwrap().up_next = Some(false);
    let mut older = read("not_started", 10);
    older.observed.as_mut().unwrap().up_next = Some(true);
    assert_eq!(older.merge(newer).observed.unwrap().up_next, Some(false));
}

#[test]
fn hs2_42tara_a_superseded_gate_no_longer_counts_for_a_prerequisite() {
    let home = tempfile::tempdir().unwrap();
    let registry = GateRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(1_000));
    let mut rng = Rng(1_000);
    let (a, b) = (ticket(1, "a"), ticket(2, "b"));
    let ci = |commit: char| NewGate {
        run_id: id(50),
        target_id: a.target_id.clone(),
        kind: GateKind::Ci,
        provider: "github-actions".into(),
        commit: sha(commit),
        ci: Some(CiScope {
            repository: "example/widgets".into(),
            workflow: "CI".into(),
            branch: "main".into(),
            artifact: None,
        }),
        deployment: None,
        state: GateState::Unknown,
        reason: Some("not_watched".into()),
        linked_by: None,
    };
    let reading = |state, commit: char| {
        GateReading::new(state).evidence(GateEvidence {
            provider: "github-actions".into(),
            reference: "1".into(),
            order: 1,
            attempt: 1,
            commit: sha(commit),
            coverage: Coverage::Exact,
            artifact: None,
            exit_code: None,
            duration_ms: None,
            at_ms: 1,
        })
    };
    let plan = [step(2, &b, vec![requires(&a, GateRequirement::Ci)])];
    let level = |registry: &GateRegistry| {
        let summary = summarize(
            registry
                .list(
                    OWNER,
                    &GateFilter {
                        run_id: Some(id(50)),
                        ..GateFilter::default()
                    },
                )
                .unwrap(),
        );
        let world = World::default()
            .status(&a, "completed")
            .status(&b, "not_started")
            .gates(&a, GateFacts::from(&summary));
        let answer = evaluate_plan(&plan, &world.0).remove(0);
        (answer.level, answer.base_commits, summary.superseded)
    };
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let first = registry.link(OWNER, ci('a'), &mut tx).unwrap().gate;
    registry
        .apply(
            OWNER,
            &first.gate_id,
            reading(GateState::Passed, 'a'),
            &mut tx,
        )
        .unwrap();
    assert_eq!(level(&registry), (ReadinessLevel::Confirmed, Vec::new(), 0));
    // A newer commit is linked in the same slot: the green verdict was for the old
    // commit and proves nothing now.
    clock.0.set(2_000);
    let second = registry.link(OWNER, ci('b'), &mut tx).unwrap().gate;
    assert_eq!(level(&registry), (ReadinessLevel::Stale, Vec::new(), 1));
    registry
        .apply(
            OWNER,
            &second.gate_id,
            GateReading::new(GateState::Pending),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        level(&registry),
        (ReadinessLevel::Provisional, vec![sha('b')], 1)
    );
    registry
        .apply(
            OWNER,
            &second.gate_id,
            reading(GateState::Passed, 'b'),
            &mut tx,
        )
        .unwrap();
    assert_eq!(level(&registry), (ReadinessLevel::Confirmed, Vec::new(), 1));
}

struct Fixture {
    home: tempfile::TempDir,
    registry: PlanRegistry,
    clock: Clock,
    rng: Rng,
    /// HS2-2FG91X: what the next definition requires; taken by it.
    resources: Vec<ResourceNeed>,
}
impl Fixture {
    fn remove(&mut self, owner: &str, step_id: &str) -> Result<PlanStep, OrchestrationError> {
        self.registry.remove(
            owner,
            step_id,
            &mut Transaction {
                clock: &self.clock,
                rng: &mut self.rng,
            },
        )
    }
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        // HS2-YDY5HJ: each owner's checkout is a directory under the home here.
        let checkouts = home.path().join("checkouts");
        Self {
            registry: PlanRegistry::new(home.path(), move |owner| {
                let root = checkouts.join(owner);
                std::fs::create_dir_all(&root).map_err(OrchestrationError::Storage)?;
                Ok(root)
            }),
            home,
            clock: Clock(std::cell::Cell::new(1_000)),
            rng: Rng(1_000),
            resources: Vec::new(),
        }
    }
    fn define_for(
        &mut self,
        owner: &str,
        target: &RunTarget,
        prerequisites: Vec<Prerequisite>,
    ) -> Result<(PlanStep, bool), OrchestrationError> {
        self.clock.0.set(self.clock.0.get() + 1);
        self.registry.define(
            owner,
            NewPlanStep {
                model: None,
                ticket: orchestration_ticket(),
                target: target.clone(),
                prerequisites,
                resources: std::mem::take(&mut self.resources),
                tool: None,
            },
            Some("worker-1".into()),
            &mut Transaction {
                clock: &self.clock,
                rng: &mut self.rng,
            },
        )
    }
    fn define(&mut self, target: &RunTarget, prerequisites: Vec<Prerequisite>) -> PlanStep {
        self.define_for(OWNER, target, prerequisites).unwrap().0
    }
}

#[test]
fn hs2_42tara_define_is_idempotent_revises_on_change_and_validates() {
    let mut fixture = Fixture::new();
    let (a, b) = (ticket(1, "a"), ticket(2, "b"));
    // A read of an absent plan creates nothing.
    assert!(fixture.registry.is_empty(OWNER).unwrap());
    assert!(!fixture.home.path().join("orchestration-plan.json").exists());

    let none = GateRequirement::None;
    let (first, created) = fixture
        .define_for(OWNER, &b, vec![requires(&a, none)])
        .unwrap();
    assert!(created);
    assert_eq!(first.revision, 1);
    let (again, changed) = fixture
        .define_for(OWNER, &b, vec![requires(&a, none)])
        .unwrap();
    assert!(!changed);
    assert_eq!(again, first);
    let (revised, changed) = fixture
        .define_for(OWNER, &b, vec![requires(&a, GateRequirement::Ci)])
        .unwrap();
    assert!(changed);
    assert_eq!(
        (revised.step_id.as_str(), revised.revision),
        (first.step_id.as_str(), 2)
    );
    assert!(revised.updated_at_ms > first.updated_at_ms);
    assert_eq!(
        fixture.registry.get(OWNER, &first.step_id).unwrap(),
        revised
    );
    assert_eq!(
        fixture.registry.find_target(OWNER, &b).unwrap(),
        Some(revised)
    );
    assert_eq!(fixture.registry.find_target(OWNER, &a).unwrap(), None);
    // Another owner with the same target ticket is a different plan.
    assert!(fixture.registry.is_empty(OTHER).unwrap());
    let (other, _) = fixture.define_for(OTHER, &b, Vec::new()).unwrap();
    assert_ne!(other.step_id, first.step_id);
    assert!(matches!(
        fixture.registry.get(OTHER, &first.step_id),
        Err(OrchestrationError::Readiness(ReadinessError::StepNotFound))
    ));

    // The same target ticket for another orchestration ticket conflicts.
    let conflict = fixture.registry.define(
        OWNER,
        NewPlanStep {
            model: None,
            ticket: RunTicket {
                connection_id: "plan".into(),
                native_id: id(901),
            },
            target: b.clone(),
            prerequisites: Vec::new(),
            resources: Vec::new(),
            tool: None,
        },
        None,
        &mut Transaction {
            clock: &fixture.clock,
            rng: &mut fixture.rng,
        },
    );
    assert!(matches!(conflict, Err(OrchestrationError::Conflict)));

    let invalid: Vec<Vec<Prerequisite>> = vec![
        // Its own ticket.
        vec![requires(&b, none)],
        // The same ticket twice, even with another requirement.
        vec![requires(&a, none), requires(&a, GateRequirement::Ci)],
        vec![validated("smoke"), validated("smoke")],
        // A short commit id.
        vec![Prerequisite::Ticket {
            target_id: a.target_id.clone(),
            connection_id: a.connection_id.clone(),
            native_id: a.native_id.clone(),
            gates: GateRequirement::Ci,
            commit: Some("abc123".into()),
        }],
        vec![validated("not a code")],
        (0..17).map(|n| validated(&format!("step-{n}"))).collect(),
    ];
    for (row, prerequisites) in invalid.into_iter().enumerate() {
        assert!(
            matches!(
                fixture.define_for(OWNER, &b, prerequisites),
                Err(OrchestrationError::Invalid(_))
            ),
            "row {row}"
        );
    }
    // A refused change left the step as it was.
    assert_eq!(
        fixture
            .registry
            .get(OWNER, &first.step_id)
            .unwrap()
            .revision,
        2
    );
    let full: Vec<Prerequisite> = (0..16).map(|n| validated(&format!("step-{n}"))).collect();
    assert_eq!(fixture.define(&b, full).prerequisites.len(), 16);
}

#[test]
fn hs2_42tara_prerequisite_wire_shape_defaults_to_ci_gates() {
    let parsed: Prerequisite = serde_json::from_value(serde_json::json!({
        "kind": "ticket", "target_id": id(1), "connection_id": "tickets", "native_id": "a"
    }))
    .unwrap();
    assert_eq!(parsed, requires(&ticket(1, "a"), GateRequirement::Ci));
    assert!(
        serde_json::from_value::<Prerequisite>(serde_json::json!({
            "kind": "ticket", "target_id": id(1), "connection_id": "tickets",
            "native_id": "a", "blocked_by": "x"
        }))
        .is_err()
    );
    assert_eq!(
        serde_json::to_value(validated("smoke")).unwrap()["kind"],
        "validation"
    );
}

#[test]
fn hs2_42tara_plan_capacity_is_per_owner() {
    let mut fixture = Fixture::new();
    for n in 0..MAX_PLAN_STEPS {
        fixture.define(&ticket(1, &format!("t{n}")), Vec::new());
    }
    assert!(matches!(
        fixture.define_for(OWNER, &ticket(1, "one-more"), Vec::new()),
        Err(OrchestrationError::Capacity)
    ));
    // Changing an existing step still works at capacity, and another owner has room.
    let changed = fixture.define(&ticket(1, "t0"), vec![validated("smoke")]);
    assert_eq!(changed.revision, 2);
    assert!(
        fixture
            .define_for(OTHER, &ticket(1, "one-more"), Vec::new())
            .is_ok()
    );
}

#[test]
fn hs2_42tara_observations_keep_the_newest_read_and_only_named_tickets() {
    let mut fixture = Fixture::new();
    let (a, b, c) = (ticket(1, "a"), ticket(2, "b"), ticket(3, "c"));
    let step = fixture.define(&b, vec![requires(&a, GateRequirement::None)]);
    let read = |status: &str, at_ms| TicketUpdate {
        observed: Some(seen(status, at_ms)),
        unreachable: Some(None),
    };
    let facts = |fixture: &Fixture, target: &RunTarget| {
        fixture
            .registry
            .tickets(OWNER)
            .unwrap()
            .into_iter()
            .find(|record| record.target == *target)
            .map(|record| record.facts)
    };
    // A ticket no step names is not kept.
    assert!(
        !fixture
            .registry
            .observe(OWNER, &c, read("started", 5))
            .unwrap()
    );
    assert_eq!(facts(&fixture, &c), None);
    assert!(
        fixture
            .registry
            .observe(OWNER, &a, read("started", 20))
            .unwrap()
    );
    // The same read again, and an older one arriving late, change nothing.
    assert!(
        !fixture
            .registry
            .observe(OWNER, &a, read("started", 20))
            .unwrap()
    );
    assert!(
        !fixture
            .registry
            .observe(OWNER, &a, read("not_started", 10))
            .unwrap()
    );
    assert_eq!(
        facts(&fixture, &a).unwrap().observed.unwrap().status,
        "started"
    );
    // An outage keeps when it first began, however often it is reported.
    let outage = |since_ms| TicketUpdate {
        observed: None,
        unreachable: Some(Some(down(since_ms))),
    };
    assert!(fixture.registry.observe(OWNER, &a, outage(30)).unwrap());
    assert!(!fixture.registry.observe(OWNER, &a, outage(40)).unwrap());
    assert_eq!(facts(&fixture, &a).unwrap().unreachable, Some(down(30)));
    // The stale read is still shown beside the outage, and a new read ends it.
    assert_eq!(facts(&fixture, &a).unwrap().observed.unwrap().at_ms, 20);
    assert!(
        fixture
            .registry
            .observe(OWNER, &a, read("completed", 50))
            .unwrap()
    );
    assert_eq!(facts(&fixture, &a).unwrap().unreachable, None);
    // Another owner's reading of the same ticket is its own.
    assert!(
        !fixture
            .registry
            .observe(OTHER, &a, read("started", 60))
            .unwrap()
    );
    // Free text is refused at the boundary.
    assert!(matches!(
        fixture.registry.observe(OWNER, &a, read("in progress", 70)),
        Err(OrchestrationError::Invalid(_))
    ));

    // Dropping the prerequisite forgets the reading kept only for it; removing the
    // step forgets the rest.
    fixture
        .registry
        .observe(OWNER, &b, read("not_started", 50))
        .unwrap();
    fixture.define(&b, Vec::new());
    assert_eq!(facts(&fixture, &a), None);
    assert!(facts(&fixture, &b).is_some());
    assert_eq!(
        fixture.remove(OWNER, &step.step_id).unwrap().step_id,
        step.step_id
    );
    assert!(fixture.registry.tickets(OWNER).unwrap().is_empty());
    assert!(matches!(
        fixture.remove(OWNER, &step.step_id),
        Err(OrchestrationError::Readiness(ReadinessError::StepNotFound))
    ));
}

#[test]
fn hs2_42tara_published_levels_report_each_change_once_through_every_sequence() {
    use ReadinessLevel as L;
    let mut fixture = Fixture::new();
    let (a, b) = (ticket(1, "a"), ticket(2, "b"));
    let first = fixture.define(&a, Vec::new());
    let second = fixture.define(&b, Vec::new());
    let publish = |fixture: &Fixture, levels: &[(&PlanStep, L)], now_ms| {
        let levels: Vec<(String, L)> = levels
            .iter()
            .map(|(step, level)| (step.step_id.clone(), *level))
            .collect();
        fixture
            .registry
            .publish(OWNER, &levels, now_ms)
            .unwrap()
            .into_iter()
            .map(|change| (change.step_id, change.from, change.to, change.seq))
            .collect::<Vec<_>>()
    };
    // The first publication is a change from nothing.
    assert_eq!(
        publish(
            &fixture,
            &[(&first, L::Confirmed), (&second, L::Blocked)],
            10
        ),
        [
            (first.step_id.clone(), None, L::Confirmed, 1),
            (second.step_id.clone(), None, L::Blocked, 1)
        ]
    );
    // Repeated: nothing to report, and when the level began is kept.
    assert!(
        publish(
            &fixture,
            &[(&first, L::Confirmed), (&second, L::Blocked)],
            20
        )
        .is_empty()
    );
    let published = |fixture: &Fixture, step: &PlanStep| {
        fixture
            .registry
            .records(OWNER)
            .unwrap()
            .into_iter()
            .find(|record| record.step.step_id == step.step_id)
            .unwrap()
            .published
            .unwrap()
    };
    assert_eq!(published(&fixture, &first).since_ms, 10);
    // Interleaved: only the step that changed is reported.
    assert_eq!(
        publish(
            &fixture,
            &[(&first, L::Confirmed), (&second, L::Provisional)],
            30
        ),
        [(second.step_id.clone(), Some(L::Blocked), L::Provisional, 2)]
    );
    // Reversed: going back is its own change with its own sequence number.
    assert_eq!(
        publish(&fixture, &[(&second, L::Blocked)], 40),
        [(second.step_id.clone(), Some(L::Provisional), L::Blocked, 3)]
    );
    assert_eq!(published(&fixture, &second).since_ms, 40);
    // A level for an unknown step, or for another owner's, is ignored.
    assert!(
        fixture
            .registry
            .publish(OTHER, &[(first.step_id.clone(), L::Stale)], 50)
            .unwrap()
            .is_empty()
    );
    assert!(
        fixture
            .registry
            .publish(OWNER, &[(id(77), L::Stale)], 50)
            .unwrap()
            .is_empty()
    );
    // Changing the definition keeps the published level; only a new level is news.
    fixture.define(&b, vec![requires(&a, GateRequirement::None)]);
    assert_eq!(published(&fixture, &second).seq, 3);

    // Empty, then refill: a step planned again starts from nothing.
    fixture.remove(OWNER, &first.step_id).unwrap();
    fixture.remove(OWNER, &second.step_id).unwrap();
    assert!(fixture.registry.is_empty(OWNER).unwrap());
    assert!(publish(&fixture, &[(&first, L::Confirmed)], 60).is_empty());
    let refilled = fixture.define(&a, Vec::new());
    assert_ne!(refilled.step_id, first.step_id);
    assert_eq!(
        publish(&fixture, &[(&refilled, L::Confirmed)], 70),
        [(refilled.step_id.clone(), None, L::Confirmed, 1)]
    );
}

#[test]
fn hs2_42tara_damaged_or_newer_plan_data_is_an_error_and_never_overwritten() {
    let mut fixture = Fixture::new();
    let path = fixture.home.path().join("orchestration-plan.json");
    for (bytes, unsupported) in [
        (&b"{not json"[..], false),
        (&br#"{"schema_version":2,"steps":[]}"#[..], true),
        (
            &br#"{"schema_version":1,"steps":[{"step":{"step_id":"x"}}]}"#[..],
            false,
        ),
    ] {
        std::fs::write(&path, bytes).unwrap();
        let read = fixture.registry.records(OWNER);
        if unsupported {
            assert!(matches!(
                read,
                Err(OrchestrationError::UnsupportedVersion { .. })
            ));
        } else {
            assert!(matches!(read, Err(OrchestrationError::Corrupt)));
        }
        assert!(
            fixture
                .define_for(OWNER, &ticket(1, "a"), Vec::new())
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn hs2_2fg91x_a_step_declares_the_resources_its_session_must_hold() {
    let need = |text: &str| ResourceNeed::parse(text).unwrap();
    let mut fixture = Fixture::new();
    let a = ticket(1, "a");
    // A step without requirements is stored as it always was.
    let plain = fixture.define(&a, Vec::new());
    assert!(plain.resources.is_empty());
    assert!(
        serde_json::to_value(&plain)
            .unwrap()
            .get("resources")
            .is_none()
    );

    // Declared in any order, stored sorted; the same set again changes nothing.
    fixture.resources = vec![need("vm/macos-vm"), need("ports/8080")];
    let (declared, changed) = fixture.define_for(OWNER, &a, Vec::new()).unwrap();
    assert!(changed);
    assert_eq!(declared.revision, 2);
    assert_eq!(
        declared.resources,
        [need("ports/8080"), need("vm/macos-vm")]
    );
    fixture.resources = vec![need("ports/8080"), need("vm/macos-vm")];
    let (same, changed) = fixture.define_for(OWNER, &a, Vec::new()).unwrap();
    assert!(!changed);
    assert_eq!(same, declared);
    fixture.resources = vec![need("vm/macos-vm"), need("ports/8080")];
    assert!(!fixture.define_for(OWNER, &a, Vec::new()).unwrap().1);

    // A changed set is a new revision; a definition without any withdraws them.
    fixture.resources = vec![need("vm/macos-vm")];
    let (fewer, changed) = fixture.define_for(OWNER, &a, Vec::new()).unwrap();
    assert!(changed);
    assert_eq!((fewer.revision, fewer.resources.len()), (3, 1));
    assert_eq!(fixture.registry.get(OWNER, &fewer.step_id).unwrap(), fewer);
    let (none, changed) = fixture.define_for(OWNER, &a, Vec::new()).unwrap();
    assert!(changed);
    assert_eq!((none.revision, none.resources.len()), (4, 0));

    // Requirements are not prerequisites: readiness does not look at them.
    fixture.resources = vec![need("vm/macos-vm")];
    let step = fixture.define_for(OWNER, &a, Vec::new()).unwrap().0;
    let levels = evaluate_plan(std::slice::from_ref(&step), &PlanFacts::default());
    let mut bare = step.clone();
    bare.resources.clear();
    assert_eq!(
        levels[0].level,
        evaluate_plan(&[bare], &PlanFacts::default())[0].level
    );

    // An invalid, repeated or oversized set is refused and nothing is written.
    let before = fixture.registry.records(OWNER).unwrap();
    let bad = ResourceNeed {
        namespace: "VM".into(),
        resource: "macos-vm".into(),
    };
    for resources in [
        vec![bad],
        vec![need("vm/a"), need("vm/a")],
        (0..=MAX_STEP_RESOURCES)
            .map(|n| need(&format!("vm/r{n}")))
            .collect(),
    ] {
        fixture.resources = resources;
        assert!(matches!(
            fixture.define_for(OWNER, &a, Vec::new()),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    assert_eq!(fixture.registry.records(OWNER).unwrap(), before);
}

/// HS2-E7HR4R: a step and the reads of the tickets it names are one write, so any
/// evaluation that sees the new step also sees its tickets observed, never a
/// prerequisite missing a read (which published `stale`). A read for a ticket no step
/// names is dropped, an unchanged definition still takes a newer read, and a malformed
/// read refuses the whole definition before anything is written.
#[test]
fn hs2_e7hr4r_a_definition_and_its_reads_are_one_write() {
    let mut fixture = Fixture::new();
    let (a, b, c) = (ticket(1, "a"), ticket(2, "b"), ticket(3, "c"));
    let read = |status: &str, at_ms| TicketUpdate {
        observed: Some(seen(status, at_ms)),
        unreachable: Some(None),
    };
    let new = |target: &RunTarget, prerequisites| NewPlanStep {
        model: None,
        ticket: orchestration_ticket(),
        target: target.clone(),
        prerequisites,
        resources: Vec::new(),
        tool: None,
    };
    let facts = |fixture: &Fixture, target: &RunTarget| {
        fixture
            .registry
            .tickets(OWNER)
            .unwrap()
            .into_iter()
            .find(|record| record.target == *target)
            .and_then(|record| record.facts.observed)
            .map(|seen| (seen.status, seen.at_ms))
    };
    let define = |fixture: &mut Fixture, prerequisites, observed| {
        let (clock, rng) = (&fixture.clock, &mut fixture.rng);
        fixture.registry.define_observed(
            OWNER,
            new(&b, prerequisites),
            None,
            observed,
            Some("worker-1".into()),
            &mut Transaction { clock, rng },
        )
    };
    let (step, created) = define(
        &mut fixture,
        vec![requires(&a, GateRequirement::None)],
        vec![
            (b.clone(), read("not_started", 10)),
            (a.clone(), read("started", 10)),
            (c.clone(), read("started", 10)),
        ],
    )
    .unwrap();
    assert!(created);
    // Both named tickets are observed by the same write as the step; the unnamed one
    // is not kept.
    assert_eq!(facts(&fixture, &a), Some(("started".into(), 10)));
    assert_eq!(facts(&fixture, &b), Some(("not_started".into(), 10)));
    assert_eq!(facts(&fixture, &c), None);

    // The same definition again changes no step but keeps a newer read, and ignores an
    // older one.
    let (again, changed) = define(
        &mut fixture,
        vec![requires(&a, GateRequirement::None)],
        vec![
            (a.clone(), read("completed", 20)),
            (b.clone(), read("started", 5)),
        ],
    )
    .unwrap();
    assert!(!changed);
    assert_eq!(again, step);
    assert_eq!(facts(&fixture, &a), Some(("completed".into(), 20)));
    assert_eq!(facts(&fixture, &b), Some(("not_started".into(), 10)));

    // A malformed read refuses the definition: no revision, no read recorded.
    let mut broken = read("started", 30);
    broken.unreachable = Some(Some(RunUnreachable {
        since_ms: 30,
        reason: "not a code".into(),
    }));
    assert!(
        define(
            &mut fixture,
            vec![requires(&a, GateRequirement::Ci)],
            vec![(a.clone(), broken)],
        )
        .is_err()
    );
    assert_eq!(
        fixture.registry.records(OWNER).unwrap()[0].step.revision,
        step.revision
    );
    assert_eq!(facts(&fixture, &a), Some(("completed".into(), 20)));
}

/// HS2-TH53NN: a plan step keeps a preferred tool and model. Defining it again with the
/// same preferences changes nothing; a changed, added or removed model is a new revision
/// in any order; a model that is not a bounded name is refused.
#[test]
fn hs2_th53nn_a_plan_step_keeps_its_preferred_tool_and_model() {
    let mut fixture = Fixture::new();
    let target = ticket(1, "T-1");
    let mut define = |tool: Option<&str>, model: Option<&str>| {
        fixture.clock.0.set(fixture.clock.0.get() + 1);
        fixture.registry.define(
            OWNER,
            NewPlanStep {
                ticket: orchestration_ticket(),
                target: target.clone(),
                prerequisites: Vec::new(),
                resources: Vec::new(),
                tool: tool.map(Into::into),
                model: model.map(Into::into),
            },
            Some("worker-1".into()),
            &mut Transaction {
                clock: &fixture.clock,
                rng: &mut fixture.rng,
            },
        )
    };
    let (first, created) = define(Some("codex"), Some("gpt-5-codex")).unwrap();
    assert!(created);
    assert_eq!(
        (
            first.tool.as_deref(),
            first.model.as_deref(),
            first.revision
        ),
        (Some("codex"), Some("gpt-5-codex"), 1)
    );
    let (same, changed) = define(Some("codex"), Some("gpt-5-codex")).unwrap();
    assert!(!changed && same.revision == 1);
    let mut revision = 1;
    for (tool, model) in [
        (Some("codex"), Some("gpt-5")),
        (Some("codex"), None),
        (None, Some("gpt-5")),
        (Some("codex"), Some("gpt-5-codex")),
    ] {
        let (step, changed) = define(tool, model).unwrap();
        revision += 1;
        assert!(changed);
        assert_eq!(
            (step.tool.as_deref(), step.model.as_deref(), step.revision),
            (tool, model, revision)
        );
    }
    for bad in ["has space", ""] {
        assert!(matches!(
            define(Some("codex"), Some(bad)),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    // Stored and read back as written; an absent model is left out.
    let wire = serde_json::to_value(define(Some("codex"), None).unwrap().0).unwrap();
    assert!(wire.get("model").is_none());
}

/// HS2-G052RV: since HS2-YDY5HJ a step lives in the checked-in plan and its reads in the
/// machine-local file, two files. A lock-free reader (any evaluation that publishes
/// levels) reads the checked-in plan and then the machine file, so the reads must be
/// written first: a reader that sees the new step then always finds its tickets
/// observed, never `not_observed`, which published a spurious `stale` under load.
#[test]
fn hs2_g052rv_a_concurrent_reader_never_sees_a_step_before_its_reads() {
    let mut fixture = Fixture::new();
    let reader = fixture.registry.clone();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop = done.clone();
    let watcher = std::thread::spawn(move || {
        let (mut reads, mut missing) = (0_u32, Vec::new());
        while !stop.load(std::sync::atomic::Ordering::SeqCst) {
            let steps = reader.records(OWNER).unwrap_or_default();
            let tickets = reader.tickets(OWNER).unwrap_or_default();
            reads += 1;
            for record in steps {
                let observed = tickets.iter().any(|ticket| {
                    ticket.target == record.step.target && ticket.facts.observed.is_some()
                });
                if !observed {
                    missing.push(record.step.target.native_id.clone());
                }
            }
        }
        (reads, missing)
    });
    for n in 0..120 {
        let target = ticket(1, &format!("t{n}"));
        let (clock, rng) = (&fixture.clock, &mut fixture.rng);
        fixture
            .registry
            .define_observed(
                OWNER,
                NewPlanStep {
                    model: None,
                    ticket: orchestration_ticket(),
                    target: target.clone(),
                    prerequisites: Vec::new(),
                    resources: Vec::new(),
                    tool: None,
                },
                None,
                vec![(
                    target,
                    TicketUpdate {
                        observed: Some(seen("not_started", 10)),
                        unreachable: Some(None),
                    },
                )],
                None,
                &mut Transaction { clock, rng },
            )
            .unwrap();
    }
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let (reads, missing) = watcher.join().unwrap();
    assert!(reads > 0);
    assert_eq!(
        missing,
        Vec::<String>::new(),
        "a step was visible before its reads"
    );
}

/// HS2-QBERH6 (maintainer's choice A): a deployment that is pending or awaiting
/// approval on a recorded commit keeps dependent work blocked (`deployment_pending`)
/// until it finishes. Nothing builds ahead on it: not when the CI gate passed, not when
/// the prerequisite is finished or still started, and not when the prerequisite names
/// the commit itself.
#[test]
fn hs2_qberh6_a_pending_deployment_on_a_recorded_commit_is_never_provisional() {
    let (a, b) = (ticket(1, "a"), ticket(2, "b"));
    let named = Prerequisite::Ticket {
        target_id: a.target_id.clone(),
        connection_id: a.connection_id.clone(),
        native_id: a.native_id.clone(),
        gates: GateRequirement::Deployment,
        commit: Some(sha('a')),
    };
    for deployment in [GateState::Pending, GateState::AwaitingApproval] {
        for status in ["completed", "verified", "started"] {
            for prerequisite in [requires(&a, GateRequirement::Deployment), named.clone()] {
                let facts = gate(GateState::Passed, None, Some(deployment));
                let mut world = World::default()
                    .status(&a, status)
                    .status(&b, "not_started")
                    .gates(&a, facts.clone());
                world
                    .0
                    .commit_gates
                    .insert((a.target_id.clone(), sha('a')), facts);
                let answers = evaluate_plan(&[step(2, &b, vec![prerequisite])], &world.0);
                let case = format!("{deployment:?} on a {status} prerequisite");
                assert_eq!(answers[0].level, ReadinessLevel::Blocked, "{case}");
                assert_eq!(codes(&answers[0]), ["deployment_pending"], "{case}");
                assert!(answers[0].base_commits.is_empty(), "{case}");
            }
        }
    }
}
