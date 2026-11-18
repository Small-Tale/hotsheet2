//! HS2-9FXASE: the external-wait measures of the orchestration summary. Synthetic ids
//! and times only.
use serde_json::json;

use super::*;

const OWNER: &str = "owner-0123456789ab";

/// A gate of `run` whose history moves through `steps` (time, state); the first entry
/// has no `from` unless `truncated`.
fn gate(id: &str, run: &str, kind: GateKind, steps: &[(u64, GateState)], truncated: bool) -> Gate {
    let mut history = Vec::new();
    let mut from: Option<GateState> = truncated.then_some(GateState::Unknown);
    for (at_ms, to) in steps {
        history.push(json!({"at_ms": at_ms, "from": from, "to": to}));
        from = Some(*to);
    }
    let last = steps.last().unwrap();
    serde_json::from_value(json!({
        "gate_id": id, "owner_checkout_id": OWNER, "run_id": run, "target_id": "target-a",
        "kind": kind, "provider": "github_actions", "commit": "a".repeat(40),
        "state": last.1, "created_at_ms": steps[0].0, "updated_at_ms": last.0,
        "history": history,
    }))
    .unwrap()
}

fn decision(
    at_ms: u64,
    kind: DecisionKind,
    subject: &str,
    reason: &str,
    detail: Option<&str>,
) -> DecisionRecord {
    let session = matches!(kind, DecisionKind::Keep | DecisionKind::WindDown);
    serde_json::from_value(json!({
        "at_ms": at_ms, "tick_id": format!("tick-{at_ms}"), "mode": "applied",
        "trigger": "event", "kind": kind,
        "session_id": session.then_some(subject), "step_id": (!session).then_some(subject),
        "reason": reason, "detail": detail, "probe": false,
    }))
    .unwrap()
}

struct World {
    gates: Vec<Gate>,
    decisions: Vec<DecisionRecord>,
    trimmed: bool,
    provisional: ProvisionalTally,
}

impl World {
    fn new() -> Self {
        Self {
            gates: Vec::new(),
            decisions: Vec::new(),
            trimmed: false,
            provisional: ProvisionalTally::default(),
        }
    }

    fn at(&self, now_ms: u64) -> ExternalWait {
        summarize_external_wait(&ExternalWaitInput {
            owner_checkout_id: OWNER,
            now_ms,
            gates: &self.gates,
            decisions: &self.decisions,
            decisions_trimmed: self.trimmed,
            provisional: self.provisional,
        })
    }
}

fn known(value: u64) -> Metric<u64> {
    Metric::Known { value }
}

fn unknown(reason: UnknownReason) -> Metric<u64> {
    Metric::Unknown { reason }
}

fn wait(wait_ms: Metric<u64>, waited: u64) -> SystemWait {
    SystemWait { wait_ms, waited }
}

#[test]
fn hs2_9fxase_absent_gates_are_unknown_and_an_empty_journal_is_zero() {
    let summary = World::new().at(1_000);
    assert_eq!(summary.ci, wait(unknown(UnknownReason::NoGateLinked), 0));
    assert_eq!(
        summary.deployments,
        wait(unknown(UnknownReason::NoDeploymentRequested), 0)
    );
    assert_eq!(
        summary.reviews,
        wait(unknown(UnknownReason::NoReviewRequested), 0)
    );
    assert_eq!(summary.provider_limits, wait(known(0), 0));
    assert_eq!(summary.idle_waiting_ms, known(0));
    assert_eq!(summary.provisional.started, known(0));
}

#[test]
fn hs2_9fxase_gate_waits_join_per_run_and_split_by_system() {
    use GateKind::{Ci, Deployment};
    use GateState::{AwaitingApproval, Failed, Passed, Pending, Superseded};
    let mut world = World::new();
    // Run 1: CI pending 100..300, superseded by a second gate pending 250..400, so
    // the run waited 100..400 once, 300 ms. Then pending again 500..600 after a retry.
    world.gates.push(gate(
        "g1",
        "run-1",
        Ci,
        &[(100, Pending), (300, Superseded)],
        false,
    ));
    world.gates.push(gate(
        "g2",
        "run-1",
        Ci,
        &[(250, Pending), (400, Failed), (500, Pending), (600, Passed)],
        false,
    ));
    // Run 2: still pending since 900, measured to now.
    world
        .gates
        .push(gate("g3", "run-2", Ci, &[(900, Pending)], false));
    // A deployment that waited for a person 1000..1400, then on its provider 1400..1500.
    world.gates.push(gate(
        "d1",
        "run-1",
        Deployment,
        &[(1_000, AwaitingApproval), (1_400, Pending), (1_500, Passed)],
        false,
    ));
    // Another owner's gate is not this owner's wait.
    let mut other = gate("g4", "run-9", Ci, &[(0, Pending)], false);
    other.owner_checkout_id = "other-0123456789ab".into();
    world.gates.push(other);
    let summary = world.at(2_000);
    assert_eq!(summary.ci, wait(known(300 + 100 + 1_100), 2));
    assert_eq!(summary.deployments, wait(known(100), 1));
    assert_eq!(summary.reviews, wait(known(400), 1));
    // The same gates in any order answer the same.
    world.gates.reverse();
    assert_eq!(world.at(2_000), summary);

    // A gate whose bounded history lost its start makes its system unknown, not lower.
    world
        .gates
        .push(gate("g5", "run-3", Ci, &[(1_900, Pending)], true));
    let truncated = world.at(2_000);
    assert_eq!(
        truncated.ci.wait_ms,
        unknown(UnknownReason::HistoryTruncated)
    );
    assert_eq!(truncated.deployments, summary.deployments);
}

#[test]
fn hs2_9fxase_provider_limits_and_idle_waiting_follow_the_journal() {
    use DecisionKind::{Keep, Start, Unplaced, WindDown};
    let mut world = World::new();
    // Step a: limited by its provider 100..400, then by a cap (not a provider limit)
    // 400..500, then started.
    world.decisions.extend([
        decision(
            100,
            Unplaced,
            "step-a",
            "no_tool_available",
            Some("limited"),
        ),
        decision(
            400,
            Unplaced,
            "step-a",
            "no_tool_available",
            Some("tool_cap"),
        ),
        decision(500, Start, "step-a", "ready_work", None),
        // Step b: limited since 800, still at now.
        decision(
            800,
            Unplaced,
            "step-b",
            "no_tool_available",
            Some("limited"),
        ),
        // Step c: held by a resource, which is not an external system here.
        decision(100, Unplaced, "step-c", "resource_held", None),
        // A session waits on its gate 200..700, then winds down.
        decision(200, Keep, "session-1", "waiting_on_gate", None),
        decision(700, WindDown, "session-1", "idle", None),
        // Another kept warm, which is idle but not waiting.
        decision(200, Keep, "session-2", "keep_warm", None),
    ]);
    let summary = world.at(1_000);
    assert_eq!(summary.provider_limits, wait(known(300 + 200), 2));
    assert_eq!(summary.idle_waiting_ms, known(500));
    // A decision logged twice counts once, in any order.
    let again = world.decisions[0].clone();
    world.decisions.push(again);
    world.decisions.reverse();
    assert_eq!(world.at(1_000), summary);
    // A trimmed journal: unknown, and the counts of what is kept stay.
    world.trimmed = true;
    let trimmed = world.at(1_000);
    assert_eq!(
        trimmed.provider_limits,
        wait(unknown(UnknownReason::DecisionsTrimmed), 2)
    );
    assert_eq!(
        trimmed.idle_waiting_ms,
        unknown(UnknownReason::DecisionsTrimmed)
    );
}

#[test]
fn hs2_9fxase_provisional_counts_and_a_full_ledger_is_unknown() {
    let mut world = World::new();
    world.provisional = ProvisionalTally {
        started: 3,
        rebased: 1,
        reopened: 1,
        trimmed: false,
    };
    let summary = world.at(0);
    assert_eq!(
        (
            summary.provisional.started,
            summary.provisional.rebased,
            summary.provisional.reopened
        ),
        (known(3), known(1), known(1))
    );
    world.provisional.trimmed = true;
    assert_eq!(
        world.at(0).provisional.reopened,
        unknown(UnknownReason::HistoryTruncated)
    );
}

#[test]
fn hs2_9fxase_the_wire_carries_numbers_and_codes_only() {
    let mut world = World::new();
    world.gates.push(gate(
        "g1",
        "run-1",
        GateKind::Ci,
        &[(0, GateState::Pending)],
        false,
    ));
    let wire = serde_json::to_value(world.at(50)).unwrap();
    assert_eq!(
        wire["ci"],
        json!({"wait_ms":{"status":"known","value":50},"waited":1})
    );
    assert_eq!(
        wire["deployments"]["wait_ms"],
        json!({"status":"unknown","reason":"no_deployment_requested"})
    );
    let keys: Vec<&str> = wire
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "ci",
            "deployments",
            "idle_waiting_ms",
            "provider_limits",
            "provisional",
            "reviews"
        ]
    );
    let back: ExternalWait = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(back).unwrap(), wire);
    assert!(!wire.to_string().contains("github_actions"));
}

/// HS2-GD3NNK: requested code reviews count in `reviews` alongside deployments that
/// wait for a person, joined per run; neither counts in `deployments`. Without either
/// the wait is unknown with `no_review_requested`.
#[test]
fn hs2_gd3nnk_requested_reviews_join_approvals_in_the_review_wait() {
    use GateKind::{Deployment, Review};
    use GateState::{AwaitingApproval, Failed, Passed, Pending, Superseded};
    let mut world = World::new();
    // Run 1: a review requested 100..300 that had changes requested, then a new review
    // requested 400..700 and approved; a deployment approval wait overlaps 600..800.
    world.gates.push(gate(
        "r1",
        "run-1",
        Review,
        &[(100, AwaitingApproval), (300, Failed), (400, Superseded)],
        false,
    ));
    world.gates.push(gate(
        "r2",
        "run-1",
        Review,
        &[(400, AwaitingApproval), (700, Passed)],
        false,
    ));
    world.gates.push(gate(
        "d1",
        "run-1",
        Deployment,
        &[(600, AwaitingApproval), (800, Pending), (900, Passed)],
        false,
    ));
    // Run 2: a review still requested since 1_500, measured to now.
    world.gates.push(gate(
        "r3",
        "run-2",
        Review,
        &[(1_500, AwaitingApproval)],
        false,
    ));
    let summary = world.at(2_000);
    assert_eq!(summary.reviews, wait(known(200 + 400 + 500), 2));
    assert_eq!(summary.deployments, wait(known(100), 1));
    // Reviews alone, with no deployment: still measured; no deployment is unknown.
    let reviews_only = World {
        gates: world
            .gates
            .iter()
            .filter(|gate| gate.kind == Review)
            .cloned()
            .collect(),
        ..World::new()
    }
    .at(2_000);
    assert_eq!(reviews_only.reviews, wait(known(200 + 300 + 500), 2));
    assert_eq!(
        reviews_only.deployments,
        wait(unknown(UnknownReason::NoDeploymentRequested), 0)
    );
}
