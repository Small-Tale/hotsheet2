//! HS2-S13719 progress summary: each measure as a table, then empty, partial,
//! overlapping-attribution, out-of-order and truncated inputs. Synthetic ids only.

use std::collections::BTreeMap;

use serde_json::json;

use super::*;

const OWNER: &str = "owner-0123456789ab";
const OTHER_OWNER: &str = "owner-ba9876543210";
const SOURCE: &str = "source-a";

fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}

/// `(at, from, to, control)`; the first entry has no `from`.
type Step = (u64, Option<RunState>, RunState, Option<RunControl>);

fn run(n: u128, target: u128, ticket: u128, created: u64, steps: &[Step]) -> Run {
    let history: Vec<RunTransition> = steps
        .iter()
        .map(|(at_ms, from, to, control)| RunTransition {
            at_ms: *at_ms,
            from: *from,
            to: *to,
            reason: None,
            actor_id: None,
            control: *control,
        })
        .collect();
    Run {
        run_id: id(n),
        owner_checkout_id: OWNER.into(),
        operation_id: format!("op-{n}"),
        ticket: RunTicket {
            connection_id: "owner-source".into(),
            native_id: id(900),
        },
        target: RunTarget {
            target_id: id(target),
            connection_id: SOURCE.into(),
            native_id: id(ticket),
        },
        mode: DispatchMode::Direct,
        worker_id: None,
        state: history.last().map_or(RunState::Requested, |entry| entry.to),
        lease_expires_at_ms: None,
        cancel_requested: false,
        observed: None,
        unreachable: None,
        paused: None,
        reconciliation: None,
        created_at_ms: created,
        updated_at_ms: created,
        history,
    }
}

use RunState::{Cancelled, Completed, Dispatched, Failed, Intervened, Queued, Requested};

/// A run that was requested at `created`, claimed 10 later and completed at `done`.
fn finished(n: u128, target: u128, ticket: u128, created: u64, done: u64) -> Run {
    run(
        n,
        target,
        ticket,
        created,
        &[
            (created, None, Requested, None),
            (created + 10, Some(Requested), Dispatched, None),
            (done, Some(Dispatched), Completed, None),
        ],
    )
}

fn event(run: u128, at_ms: u64, kind: RunEventKind) -> RunEvent {
    RunEvent {
        at_ms,
        run_id: id(run),
        kind,
        source: RunEventSource::Orchestrator,
        state: None,
        reason: None,
        actor_id: None,
        dedupe: None,
        gate_id: None,
        gate_state: None,
    }
}

fn gate(n: u128, run: u128, kind: GateKind, state: GateState) -> Gate {
    serde_json::from_value(json!({
        "gate_id": id(n),
        "owner_checkout_id": OWNER,
        "run_id": id(run),
        "target_id": id(1),
        "kind": kind,
        "provider": "github-actions",
        "commit": "a".repeat(40),
        "state": state,
        "created_at_ms": 1,
        "updated_at_ms": 1,
        "history": [],
    }))
    .unwrap()
}

fn session(
    n: u128,
    target: u128,
    run: Option<u128>,
    hosted: Option<&str>,
    created: u64,
) -> OrchestratedSession {
    serde_json::from_value(json!({
        "session_id": id(n),
        "owner_checkout_id": OWNER,
        "operation_id": format!("session-{n}"),
        "target_id": id(target),
        "tool": "claude",
        "run_id": run.map(id),
        "state": "running",
        "hosted": hosted.map(|session| json!({
            "server_id": "server-a", "session_id": session, "worker_id": "worker-a",
        })),
        "created_at_ms": created,
        "updated_at_ms": created,
        "history": [],
    }))
    .unwrap()
}

/// One step of the runs' orchestration ticket with `(run, attempt)` results.
fn validation(step: &str, results: &[(u128, u32)]) -> ValidationEvidence {
    let results: Vec<_> = results
        .iter()
        .enumerate()
        .map(|(index, (run, attempt))| {
            json!({
                "result_id": format!("{step}-{index}"),
                "operation_id": format!("{step}-op"),
                "attempt": attempt,
                "step_revision": 1,
                "state": "failed",
                "run_id": id(*run),
                "created_at_ms": 1,
            })
        })
        .collect();
    serde_json::from_value(json!({
        "step": {
            "owner_checkout_id": OWNER,
            "ticket": { "connection_id": "owner-source", "native_id": id(900) },
            "definition": {
                "step_id": step,
                "target_id": id(1),
                "check": { "kind": "profile", "name": "smoke" },
            },
            "revision": 1,
            "created_at_ms": 1,
            "updated_at_ms": 1,
        },
        "state": "failed",
        "latest": results.last(),
        "history": results,
    }))
    .unwrap()
}

#[derive(Default)]
struct World {
    runs: Vec<Run>,
    gates: Vec<Gate>,
    validations: Vec<ValidationEvidence>,
    sessions: Vec<OrchestratedSession>,
    events: Vec<RunEvent>,
    events_trimmed: bool,
    costs: BTreeMap<WorkItemKey, Metric<CostReading>>,
}

impl World {
    fn of(runs: Vec<Run>) -> Self {
        Self {
            runs,
            ..Self::default()
        }
    }

    fn cost(mut self, ticket: u128, cost: Metric<CostReading>) -> Self {
        self.costs.insert(key(ticket), cost);
        self
    }

    fn at(&self, now_ms: u64) -> ProgressSummary {
        summarize_progress(&ProgressInput {
            owner_checkout_id: OWNER,
            now_ms,
            runs: &self.runs,
            gates: &self.gates,
            validations: &self.validations,
            sessions: &self.sessions,
            events: &self.events,
            events_trimmed: self.events_trimmed,
            costs: &self.costs,
        })
    }
}

fn key(ticket: u128) -> WorkItemKey {
    WorkItemKey {
        connection_id: SOURCE.into(),
        native_id: id(ticket),
    }
}

fn usd(cost_usd: f64, events: u64, unpriced_events: u64) -> Metric<CostReading> {
    Metric::Known {
        value: CostReading {
            cost_usd,
            events,
            unpriced_events,
        },
    }
}

fn known<T>(value: T) -> Metric<T> {
    Metric::Known { value }
}

fn unknown<T>(reason: UnknownReason) -> Metric<T> {
    Metric::Unknown { reason }
}

fn counts(entries: &[(&str, u64)]) -> BTreeMap<String, u64> {
    entries
        .iter()
        .map(|(code, count)| ((*code).to_owned(), *count))
        .collect()
}

#[test]
fn hs2_s13719_nothing_recorded_is_an_empty_summary_and_not_a_zero_cost_total() {
    let summary = World::default().at(500);
    assert_eq!(summary.owner_checkout_id, OWNER);
    assert_eq!(summary.as_of_ms, 500);
    assert_eq!(summary.attribution, "work_item_earliest_run");
    assert!(summary.runs.is_empty() && summary.work_items.is_empty());
    assert!(summary.targets.is_empty());
    assert_eq!(
        summary.totals,
        ProgressRollup {
            cost: CostAggregate {
                complete: true,
                ..CostAggregate::default()
            },
            ..ProgressRollup::default()
        }
    );
    // Records of another owner are not this owner's.
    let mut other = finished(1, 1, 10, 100, 200);
    other.owner_checkout_id = OTHER_OWNER.into();
    assert!(World::of(vec![other]).at(500).runs.is_empty());
}

#[test]
fn hs2_s13719_lead_time_and_queue_wait_for_every_shape_of_history() {
    use UnknownReason::{ClockInconsistent, HistoryTruncated, NeverDispatched, RunUnfinished};
    let requested = (100, None, Requested, None);
    let full: Vec<Step> = std::iter::once(requested)
        .chain((0..MAX_RUN_HISTORY as u64 - 1).map(|n| {
            if n % 2 == 0 {
                (200 + n, Some(Dispatched), Intervened, None)
            } else {
                (200 + n, Some(Intervened), Dispatched, None)
            }
        }))
        .collect();
    let mut full_done = full.clone();
    full_done.remove(1);
    full_done.push((900, Some(Intervened), Completed, None));
    type Case = (&'static str, Vec<Step>, Metric<u64>, Metric<u64>);
    let cases: Vec<Case> = vec![
        (
            "requested only",
            vec![requested],
            unknown(RunUnfinished),
            unknown(NeverDispatched),
        ),
        (
            "queued",
            vec![requested, (150, Some(Requested), Queued, None)],
            unknown(RunUnfinished),
            unknown(NeverDispatched),
        ),
        (
            "dispatched",
            vec![requested, (160, Some(Requested), Dispatched, None)],
            unknown(RunUnfinished),
            known(60),
        ),
        (
            "completed",
            vec![
                requested,
                (160, Some(Requested), Dispatched, None),
                (400, Some(Dispatched), Completed, None),
            ],
            known(300),
            known(60),
        ),
        (
            "cancelled before any claim",
            vec![requested, (130, Some(Requested), Cancelled, None)],
            known(30),
            unknown(NeverDispatched),
        ),
        (
            "claimed twice: the first claim is the wait",
            vec![
                requested,
                (160, Some(Requested), Dispatched, None),
                (200, Some(Dispatched), Intervened, None),
                (300, Some(Intervened), Dispatched, None),
                (350, Some(Dispatched), Failed, None),
            ],
            known(250),
            known(60),
        ),
        (
            "finished at the instant it began",
            vec![requested, (100, Some(Requested), Completed, None)],
            known(0),
            unknown(NeverDispatched),
        ),
        (
            "a clock that ran backwards",
            vec![
                requested,
                (90, Some(Requested), Dispatched, None),
                (80, Some(Dispatched), Completed, None),
            ],
            unknown(ClockInconsistent),
            unknown(ClockInconsistent),
        ),
        (
            "a full history may have lost the first claim",
            full,
            unknown(RunUnfinished),
            unknown(HistoryTruncated),
        ),
        (
            "a full history still holds the finish",
            full_done,
            known(800),
            unknown(HistoryTruncated),
        ),
    ];
    for (name, steps, lead, wait) in cases {
        let summary = World::of(vec![run(1, 1, 10, 100, &steps)]).at(1_000);
        assert_eq!(summary.runs[0].lead_time_ms, lead, "{name}: lead time");
        assert_eq!(summary.runs[0].queue_wait_ms, wait, "{name}: queue wait");
    }
    // A finished state whose transition is missing is not guessed from other times.
    let mut lost = run(1, 1, 10, 100, &[requested]);
    lost.state = Completed;
    lost.updated_at_ms = 700;
    let summary = World::of(vec![lost]).at(1_000);
    assert_eq!(summary.runs[0].lead_time_ms, unknown(HistoryTruncated));
    assert_eq!(summary.runs[0].blocked, unknown(HistoryTruncated));
}

#[test]
fn hs2_s13719_blocked_time_counts_overlapping_causes_once() {
    use RunControl::{Handoff, Pause, Resume};
    use RunEventKind::{TargetReachable, TargetUnreachable};
    let start: [Step; 2] = [
        (100, None, Requested, None),
        (110, Some(Requested), Dispatched, None),
    ];
    let blocked = |total_ms, paused_ms, intervened_ms, unreachable_ms| {
        known(BlockedTime {
            total_ms,
            paused_ms,
            intervened_ms,
            unreachable_ms,
        })
    };
    type Case = (
        &'static str,
        Vec<Step>,
        Vec<(u64, RunEventKind)>,
        Metric<BlockedTime>,
    );
    let cases: Vec<Case> = vec![
        ("never blocked", vec![], vec![], blocked(0, 0, 0, 0)),
        (
            "paused then resumed",
            vec![
                (200, Some(Dispatched), Dispatched, Some(Pause)),
                (260, Some(Dispatched), Dispatched, Some(Resume)),
            ],
            vec![],
            blocked(60, 60, 0, 0),
        ),
        (
            "still paused: measured to now",
            vec![(200, Some(Dispatched), Dispatched, Some(Pause))],
            vec![],
            blocked(800, 800, 0, 0),
        ),
        (
            "paused twice in a row keeps the first, resumed twice closes once",
            vec![
                (200, Some(Dispatched), Dispatched, Some(Pause)),
                (220, Some(Dispatched), Dispatched, Some(Pause)),
                (260, Some(Dispatched), Dispatched, Some(Resume)),
                (300, Some(Dispatched), Dispatched, Some(Resume)),
            ],
            vec![],
            blocked(60, 60, 0, 0),
        ),
        (
            "pause, resume, pause again: separate intervals",
            vec![
                (200, Some(Dispatched), Dispatched, Some(Pause)),
                (210, Some(Dispatched), Dispatched, Some(Resume)),
                (300, Some(Dispatched), Dispatched, Some(Pause)),
                (330, Some(Dispatched), Dispatched, Some(Resume)),
            ],
            vec![],
            blocked(40, 40, 0, 0),
        ),
        (
            "a handoff is not blocked time",
            vec![(200, Some(Dispatched), Dispatched, Some(Handoff))],
            vec![],
            blocked(0, 0, 0, 0),
        ),
        (
            "taken over and handed back",
            vec![
                (200, Some(Dispatched), Intervened, None),
                (500, Some(Intervened), Dispatched, None),
            ],
            vec![],
            blocked(300, 0, 300, 0),
        ),
        (
            "an outage",
            vec![],
            vec![(300, TargetUnreachable), (340, TargetReachable)],
            blocked(40, 0, 0, 40),
        ),
        (
            "a repeated outage report does not restart it; a stray recovery is ignored",
            vec![],
            vec![
                (250, TargetReachable),
                (300, TargetUnreachable),
                (320, TargetUnreachable),
                (340, TargetReachable),
                (360, TargetReachable),
            ],
            blocked(40, 0, 0, 40),
        ),
        (
            "events that arrived out of order are read in the order they were true",
            vec![],
            vec![
                (340, TargetReachable),
                (600, TargetUnreachable),
                (300, TargetUnreachable),
                (650, TargetReachable),
            ],
            blocked(90, 0, 0, 90),
        ),
        (
            "paused during an outage during a takeover: the union, not the sum",
            vec![
                (200, Some(Dispatched), Intervened, None),
                (250, Some(Intervened), Intervened, Some(Pause)),
                (400, Some(Intervened), Intervened, Some(Resume)),
                (500, Some(Intervened), Dispatched, None),
            ],
            vec![(450, TargetUnreachable), (700, TargetReachable)],
            blocked(500, 150, 300, 250),
        ),
        (
            "an outage that began before the run counts from the run's creation",
            vec![],
            vec![(50, TargetUnreachable), (150, TargetReachable)],
            blocked(50, 0, 0, 50),
        ),
    ];
    for (name, extra, events, expected) in &cases {
        let steps: Vec<Step> = start.iter().chain(extra).copied().collect();
        let mut world = World::of(vec![run(1, 1, 10, 100, &steps)]);
        world.events = events
            .iter()
            .map(|(at_ms, kind)| event(1, *at_ms, *kind))
            .collect();
        // Another run's events never count.
        world.events.push(event(2, 120, TargetUnreachable));
        assert_eq!(world.at(1_000).runs[0].blocked, *expected, "{name}");
    }
    // A finished run stops being blocked at its finish, whatever was still open.
    let mut world = World::of(vec![run(
        1,
        1,
        10,
        100,
        &[
            start[0],
            start[1],
            (200, Some(Dispatched), Dispatched, Some(Pause)),
            (300, Some(Dispatched), Intervened, None),
            (450, Some(Intervened), Cancelled, None),
        ],
    )]);
    world.events = vec![event(1, 400, TargetUnreachable)];
    assert_eq!(world.at(9_000).runs[0].blocked, blocked(250, 250, 150, 50));
    // A resume recorded before its pause cannot be measured.
    let world = World::of(vec![run(
        1,
        1,
        10,
        100,
        &[
            start[0],
            start[1],
            (300, Some(Dispatched), Dispatched, Some(Pause)),
            (250, Some(Dispatched), Dispatched, Some(Resume)),
        ],
    )]);
    assert_eq!(
        world.at(1_000).runs[0].blocked,
        unknown(UnknownReason::ClockInconsistent)
    );
}

#[test]
fn hs2_s13719_trimmed_events_make_older_runs_unknown_and_leave_newer_ones_known() {
    use RunEventKind::{State, TargetReachable, TargetUnreachable};
    let mut world = World::of(vec![
        finished(1, 1, 10, 100, 400),
        finished(2, 1, 11, 500, 900),
    ]);
    world.events = vec![
        event(2, 600, TargetUnreachable),
        event(2, 640, TargetReachable),
        event(2, 300, State),
    ];
    let summary = world.at(1_000);
    assert!(summary.runs.iter().all(|run| run.blocked.known().is_some()));
    world.events_trimmed = true;
    let summary = world.at(1_000);
    assert!(summary.events_trimmed);
    // The oldest event kept is from 300: run 1 began before it, run 2 after.
    assert_eq!(
        summary.runs[0].blocked,
        unknown(UnknownReason::EventsTrimmed)
    );
    assert_eq!(summary.runs[1].blocked.known().unwrap().unreachable_ms, 40);
    assert_eq!(
        summary.totals.blocked,
        DurationAggregate {
            known: 1,
            unknown: 1,
            total_ms: 40,
            max_ms: 40,
            unknown_reasons: counts(&[("events_trimmed", 1)]),
        }
    );
    // Trimmed down to nothing: no run's outages can be known.
    world.events.clear();
    assert!(
        world
            .at(1_000)
            .runs
            .iter()
            .all(|run| run.blocked == unknown(UnknownReason::EventsTrimmed))
    );
    // A full history hides pauses whatever the events say.
    let steps: Vec<Step> = (0..MAX_RUN_HISTORY as u64)
        .map(|n| (100 + n, Some(Dispatched), Dispatched, None))
        .collect();
    let summary = World::of(vec![run(1, 1, 10, 100, &steps)]).at(1_000);
    assert_eq!(
        summary.runs[0].blocked,
        unknown(UnknownReason::HistoryTruncated)
    );
}

#[test]
fn hs2_s13719_gate_outcomes_follow_the_standing_gates_of_the_run() {
    use GateKind::{Ci, Deployment};
    use GateState::{AwaitingApproval, Passed, Pending, Superseded};
    use UnknownReason::{GateUnverified, NoDeploymentRequested, NoGateLinked};
    type Case = (
        &'static str,
        Vec<(GateKind, GateState)>,
        Metric<GateState>,
        Metric<GateState>,
    );
    let cases: Vec<Case> = vec![
        (
            "no gate",
            vec![],
            unknown(NoGateLinked),
            unknown(NoDeploymentRequested),
        ),
        (
            "ci passed",
            vec![(Ci, Passed)],
            known(Passed),
            unknown(NoDeploymentRequested),
        ),
        (
            "ci failed outranks a passed one",
            vec![(Ci, Passed), (Ci, GateState::Failed)],
            known(GateState::Failed),
            unknown(NoDeploymentRequested),
        ),
        (
            "a gate without a verdict is unknown, not pending",
            vec![(Ci, GateState::Unknown), (Ci, Pending)],
            unknown(GateUnverified),
            unknown(NoDeploymentRequested),
        ),
        (
            "only superseded links stand for nothing",
            vec![(Ci, Superseded), (Deployment, Superseded)],
            unknown(NoGateLinked),
            unknown(NoDeploymentRequested),
        ),
        (
            "a deployment waiting for a person",
            vec![(Ci, Passed), (Deployment, AwaitingApproval)],
            known(Passed),
            known(AwaitingApproval),
        ),
        (
            "a deployment without a ci gate",
            vec![(Deployment, GateState::Failed)],
            unknown(NoGateLinked),
            known(GateState::Failed),
        ),
    ];
    for (name, gates, ci, deployment) in cases {
        let mut world = World::of(vec![finished(1, 1, 10, 100, 200)]);
        world.gates = gates
            .iter()
            .enumerate()
            .map(|(index, (kind, state))| gate(50 + index as u128, 1, *kind, *state))
            .collect();
        // Another run's gate, and another owner's gate of this run id, never count.
        world.gates.push(gate(90, 2, Ci, GateState::Failed));
        let mut foreign = gate(91, 1, Ci, GateState::Failed);
        foreign.owner_checkout_id = OTHER_OWNER.into();
        world.gates.push(foreign);
        let summary = world.at(500);
        assert_eq!(summary.runs[0].ci, ci, "{name}: ci");
        assert_eq!(summary.runs[0].deployment, deployment, "{name}: deployment");
    }
    let mut world = World::of(vec![
        finished(1, 1, 10, 100, 200),
        finished(2, 1, 11, 100, 200),
        finished(3, 2, 12, 100, 200),
    ]);
    world.gates = vec![
        gate(50, 1, Ci, Passed),
        gate(51, 2, Ci, Passed),
        gate(52, 2, Deployment, Passed),
    ];
    let totals = world.at(500).totals;
    assert_eq!(totals.ci.known, counts(&[("passed", 2)]));
    assert_eq!(totals.ci.unknown, counts(&[("no_gate_linked", 1)]));
    assert_eq!(totals.deployment.known, counts(&[("passed", 1)]));
    assert_eq!(
        totals.deployment.unknown,
        counts(&[("no_deployment_requested", 2)])
    );
}

#[test]
fn hs2_s13719_retries_count_repeated_work_once_each() {
    let mut handed = finished(1, 1, 10, 100, 400);
    handed.history.insert(
        2,
        RunTransition {
            at_ms: 150,
            from: Some(Dispatched),
            to: Dispatched,
            reason: None,
            actor_id: None,
            control: Some(RunControl::Handoff),
        },
    );
    let mut world = World::of(vec![
        handed,
        finished(2, 1, 10, 500, 700),
        finished(3, 2, 11, 100, 200),
    ]);
    world.validations = vec![
        // Run 1: attempts 1, 2, 3 of one step and 1, 2 of another: three retries.
        validation("build", &[(1, 1), (1, 2), (1, 3), (2, 1)]),
        validation("test", &[(1, 1), (1, 2), (3, 1)]),
    ];
    // A step of another orchestration ticket never counts, even naming the run.
    let mut elsewhere = validation("lint", &[(1, 2)]);
    elsewhere.step.ticket.native_id = id(901);
    world.validations.push(elsewhere);
    let mut replacement = session(20, 1, Some(1), None, 300);
    replacement.replaces = Some(id(19));
    let mut unrelated = session(21, 1, Some(2), None, 300);
    unrelated.owner_checkout_id = OTHER_OWNER.into();
    unrelated.replaces = Some(id(19));
    world.sessions = vec![session(19, 1, Some(1), None, 200), replacement, unrelated];
    let summary = world.at(1_000);
    assert_eq!(
        summary.runs[0].retries,
        RunRetries {
            validation: 3,
            session_replacements: 1,
            handoffs: 1,
        }
    );
    assert_eq!(summary.runs[1].retries, RunRetries::default());
    assert_eq!(summary.runs[2].retries, RunRetries::default());
    // Runs 1 and 2 dispatched the same target ticket: one redispatch.
    assert_eq!(
        summary
            .runs
            .iter()
            .map(|run| run.attempt)
            .collect::<Vec<_>>(),
        [1, 1, 2]
    );
    assert_eq!(
        summary.totals.retries,
        RetryTotals {
            redispatches: 1,
            validation: 3,
            session_replacements: 1,
            handoffs: 1,
        }
    );
}

#[test]
fn hs2_s13719_cost_is_known_unknown_or_a_lower_bound_and_never_zero_by_default() {
    use UnknownReason::{CostNotRead, NoUsageRecorded, TargetRemote};
    let runs = vec![
        finished(1, 1, 10, 100, 200),
        finished(2, 1, 11, 100, 200),
        finished(3, 2, 12, 100, 200),
        finished(4, 2, 13, 100, 200),
    ];
    // Every item known and priced: a complete total.
    let world = World::of(runs.clone())
        .cost(10, usd(1.25, 4, 0))
        .cost(11, usd(0.5, 1, 0))
        .cost(12, usd(2.0, 3, 0))
        .cost(13, usd(0.25, 1, 0));
    let cost = world.at(500).totals.cost;
    assert!(cost.complete);
    assert_eq!((cost.known_usd, cost.items_known, cost.events), (4.0, 4, 9));
    // Partial: one remote, one with nothing recorded, one never read, one unpriced.
    let world = World::of(runs)
        .cost(10, usd(1.25, 4, 2))
        .cost(11, unknown(NoUsageRecorded))
        .cost(12, unknown(TargetRemote));
    let summary = world.at(500);
    assert_eq!(
        summary.totals.cost,
        CostAggregate {
            known_usd: 1.25,
            complete: false,
            items_known: 1,
            items_unknown: 3,
            events: 4,
            unpriced_events: 2,
            unknown_reasons: counts(&[
                ("cost_not_read", 1),
                ("no_usage_recorded", 1),
                ("target_remote", 1),
            ]),
        }
    );
    assert_eq!(summary.work_items[3].cost, unknown(CostNotRead));
    // Unpriced events alone keep a total from being complete.
    let world = World::of(vec![finished(1, 1, 10, 100, 200)]).cost(10, usd(1.0, 2, 1));
    assert!(!world.at(500).totals.cost.complete);
    // A reading for a ticket no run names is not counted.
    let world = World::of(vec![finished(1, 1, 10, 100, 200)])
        .cost(10, usd(1.0, 2, 0))
        .cost(77, usd(50.0, 9, 0));
    assert_eq!(world.at(500).totals.cost.known_usd, 1.0);
    // No serialized unknown carries a number that could be read as a value.
    let text = serde_json::to_value(unknown::<CostReading>(TargetRemote)).unwrap();
    assert_eq!(
        text,
        json!({ "status": "unknown", "reason": "target_remote" })
    );
    let text = serde_json::to_value(known(7_u64)).unwrap();
    assert_eq!(text, json!({ "status": "known", "value": 7 }));
}

#[test]
fn hs2_s13719_a_work_item_reached_twice_or_through_two_targets_is_counted_once() {
    // Ticket 10: run 1 through target 2 (earliest), then runs 2 and 3 through target 1.
    // Ticket 11: one run through target 1. The same native id in another source is
    // another work item.
    let mut other_source = finished(5, 1, 10, 150, 250);
    other_source.target.connection_id = "source-b".into();
    let world = World {
        runs: vec![
            finished(3, 1, 10, 600, 700),
            finished(2, 1, 10, 300, 500),
            finished(1, 2, 10, 100, 200),
            finished(4, 1, 11, 100, 200),
            other_source,
        ],
        sessions: vec![
            // One hosted session known through two records, the earlier on target 2.
            session(30, 1, Some(2), Some("terminal-1"), 400),
            session(31, 2, Some(1), Some("terminal-1"), 120),
            // The same host id on another machine is another session.
            {
                let mut elsewhere = session(32, 1, None, Some("terminal-1"), 130);
                elsewhere.hosted.as_mut().unwrap().server_id = "server-b".into();
                elsewhere
            },
            // Never hosted: counted by its own id.
            session(33, 1, None, None, 140),
        ],
        ..World::default()
    }
    .cost(10, usd(3.0, 6, 0))
    .cost(11, usd(1.0, 1, 0));
    let summary = world.at(1_000);

    let item = &summary.work_items[0];
    assert_eq!(
        (&item.connection_id, &item.native_id),
        (&SOURCE.to_owned(), &id(10))
    );
    assert_eq!(item.attributed_target_id, id(2));
    assert_eq!(item.target_ids, [id(1), id(2)]);
    assert_eq!(item.run_ids, [id(1), id(2), id(3)]);
    assert_eq!(item.redispatches, 2);
    assert_eq!(summary.work_items.len(), 3);
    assert_eq!(summary.work_items[2].connection_id, "source-b");

    // Cost of ticket 10 is counted once, in the totals and in target 2 only.
    assert_eq!(summary.totals.cost.known_usd, 4.0);
    assert_eq!(summary.totals.cost.items_known, 2);
    assert_eq!(summary.totals.work_items, 3);
    assert_eq!(summary.totals.sessions, 3);
    assert_eq!(summary.totals.retries.redispatches, 2);
    let target = |n: u128| {
        &summary
            .targets
            .iter()
            .find(|target| target.target_id == id(n))
            .unwrap()
            .figures
    };
    assert_eq!((target(1).runs, target(2).runs), (4, 1));
    assert_eq!((target(1).work_items, target(2).work_items), (2, 1));
    assert_eq!(
        (target(1).cost.known_usd, target(2).cost.known_usd),
        (1.0, 3.0)
    );
    assert_eq!((target(1).sessions, target(2).sessions), (2, 1));
    assert_eq!(
        (
            target(1).retries.redispatches,
            target(2).retries.redispatches
        ),
        (0, 2)
    );
    // The targets add up to the totals, whatever was shared.
    let sum = |field: fn(&ProgressRollup) -> u64| {
        summary
            .targets
            .iter()
            .map(|target| field(&target.figures))
            .sum::<u64>()
    };
    assert_eq!(sum(|figures| figures.runs), summary.totals.runs);
    assert_eq!(sum(|figures| figures.work_items), summary.totals.work_items);
    assert_eq!(sum(|figures| figures.sessions), summary.totals.sessions);
    assert_eq!(
        sum(|figures| figures.cost.items_known + figures.cost.items_unknown),
        summary.totals.work_items
    );
    assert_eq!(
        sum(|figures| figures.lead_time.total_ms),
        summary.totals.lead_time.total_ms
    );
}

#[test]
fn hs2_s13719_the_answer_does_not_depend_on_the_order_or_repetition_of_records() {
    use RunEventKind::{TargetReachable, TargetUnreachable};
    let build = |order: &[usize], repeat: bool| {
        let runs = [
            finished(1, 2, 10, 100, 200),
            finished(2, 1, 10, 300, 500),
            run(
                3,
                1,
                11,
                300,
                &[
                    (300, None, Requested, None),
                    (320, Some(Requested), Dispatched, None),
                    (400, Some(Dispatched), Intervened, None),
                ],
            ),
            // Two runs created at the same instant are ordered by id.
            finished(4, 3, 12, 300, 600),
        ];
        let events = [
            event(2, 350, TargetUnreachable),
            event(2, 380, TargetReachable),
            event(3, 330, TargetUnreachable),
            event(3, 360, TargetReachable),
        ];
        let gates = [
            gate(50, 1, GateKind::Ci, GateState::Passed),
            gate(51, 2, GateKind::Ci, GateState::Failed),
            gate(52, 3, GateKind::Ci, GateState::Pending),
            gate(53, 4, GateKind::Deployment, GateState::Passed),
        ];
        let sessions = [
            session(30, 1, Some(2), Some("terminal-1"), 310),
            session(31, 2, Some(1), Some("terminal-1"), 110),
            session(32, 1, Some(3), Some("terminal-2"), 310),
            session(33, 3, Some(4), None, 310),
        ];
        let mut world = World::default()
            .cost(10, usd(1.5, 2, 0))
            .cost(12, unknown(UnknownReason::TargetRemote));
        for index in order {
            world.runs.push(runs[*index].clone());
            world.events.push(events[*index].clone());
            world.gates.push(gates[*index].clone());
            world.sessions.push(sessions[*index].clone());
            if repeat {
                world.runs.push(runs[*index].clone());
                world.sessions.push(sessions[*index].clone());
            }
        }
        world.validations = vec![validation("build", &[(2, 1), (2, 2)])];
        world.at(1_000)
    };
    let expected = build(&[0, 1, 2, 3], false);
    assert_eq!(expected.runs.len(), 4);
    assert_eq!(
        expected
            .runs
            .iter()
            .map(|run| &run.run_id)
            .collect::<Vec<_>>(),
        [&id(1), &id(2), &id(3), &id(4)]
    );
    assert_eq!(expected.totals.sessions, 3);
    for order in [[3, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1]] {
        assert_eq!(build(&order, false), expected, "{order:?}");
        assert_eq!(build(&order, true), expected, "{order:?} repeated");
    }
    // Empty, filled, then empty again: nothing is remembered between summaries.
    assert_eq!(World::default().at(1_000).totals.runs, 0);
    assert_eq!(build(&[0, 1, 2, 3], false), expected);
    assert_eq!(World::default().at(1_000).totals.runs, 0);
    // Later, the unfinished run has been blocked longer and nothing else moved.
    let later = World::of(vec![expected_run_three()]).at(2_000);
    assert_eq!(later.runs[0].blocked.known().unwrap().intervened_ms, 1_600);
    assert_eq!(expected.runs[2].blocked.known().unwrap().intervened_ms, 600);
}

fn expected_run_three() -> Run {
    run(
        3,
        1,
        11,
        300,
        &[
            (300, None, Requested, None),
            (320, Some(Requested), Dispatched, None),
            (400, Some(Dispatched), Intervened, None),
        ],
    )
}

#[test]
fn hs2_s13719_every_state_and_reason_has_a_stable_code_and_round_trips() {
    for reason in UnknownReason::ALL {
        let metric: Metric<u64> = unknown(reason);
        let text = serde_json::to_value(metric).unwrap();
        assert_eq!(text["reason"], reason.code());
        assert_eq!(serde_json::from_value::<Metric<u64>>(text).unwrap(), metric);
        assert_eq!(metric.reason(), Some(reason));
    }
    let codes: std::collections::BTreeSet<_> = UnknownReason::ALL
        .iter()
        .map(|reason| reason.code())
        .collect();
    assert_eq!(codes.len(), UnknownReason::ALL.len());
    // One run in every state: each is counted under its own code.
    let chains: [&[RunState]; 7] = [
        &[Requested],
        &[Requested, Queued],
        &[Requested, Dispatched],
        &[Requested, Completed],
        &[Requested, Failed],
        &[Requested, Cancelled],
        &[Requested, Queued, Intervened],
    ];
    let runs: Vec<Run> = chains
        .iter()
        .enumerate()
        .map(|(index, chain)| {
            let steps: Vec<Step> = chain
                .iter()
                .enumerate()
                .map(|(step, to)| {
                    let from = step.checked_sub(1).map(|before| chain[before]);
                    (100 + step as u64, from, *to, None)
                })
                .collect();
            run(index as u128 + 1, 1, 10 + index as u128, 100, &steps)
        })
        .collect();
    let summary = World::of(runs).at(1_000);
    let by_state = &summary.totals.runs_by_state;
    assert_eq!(by_state.len(), RunState::ALL.len());
    for state in RunState::ALL {
        let code = serde_json::to_value(state).unwrap();
        assert_eq!(by_state[code.as_str().unwrap()], 1, "{state:?}");
    }
    assert_eq!(
        summary.totals.lead_time.unknown_reasons,
        counts(&[("run_unfinished", 4)])
    );
    assert_eq!(summary.totals.lead_time.known, 3);
    // The whole summary survives its own wire form.
    let text = serde_json::to_string(&summary).unwrap();
    assert_eq!(
        serde_json::from_str::<ProgressSummary>(&text).unwrap(),
        summary
    );
}
