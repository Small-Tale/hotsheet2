//! HS2-5JBYPV gate state machine, coverage rule and registry sequences; synthetic
//! homes and commit ids only.

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
const PROVIDER: &str = "github-actions";
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn sha(digit: char) -> String {
    std::iter::repeat_n(digit, 40).collect()
}
fn ci(run: u128, commit: &str) -> NewGate {
    NewGate {
        run_id: id(run),
        target_id: id(1),
        kind: GateKind::Ci,
        provider: PROVIDER.into(),
        commit: commit.into(),
        ci: Some(CiScope {
            repository: "example/widgets".into(),
            workflow: "CI".into(),
            branch: "main".into(),
            artifact: None,
        }),
        deployment: None,
        state: GateState::Unknown,
        reason: Some("not_watched".into()),
        linked_by: Some("worker-1".into()),
    }
}
fn deployment(run: u128, commit: &str, operation: &str, requires: &str) -> NewGate {
    NewGate {
        run_id: id(run),
        target_id: id(1),
        kind: GateKind::Deployment,
        provider: "target-script".into(),
        commit: commit.into(),
        ci: None,
        deployment: Some(DeploymentScope {
            environment: "beta".into(),
            script: "scripts/deploy.sh".into(),
            production: false,
            operation_id: operation.into(),
            requires_gate_id: requires.into(),
            artifact: None,
        }),
        state: GateState::AwaitingApproval,
        reason: None,
        linked_by: None,
    }
}
fn evidence(order: u64, attempt: u32, commit: &str, coverage: Coverage) -> GateEvidence {
    GateEvidence {
        provider: PROVIDER.into(),
        reference: order.to_string(),
        order,
        attempt,
        commit: commit.into(),
        coverage,
        artifact: None,
        exit_code: None,
        duration_ms: None,
        at_ms: 1,
    }
}
fn verdict(state: GateState, order: u64, attempt: u32, commit: &str) -> GateReading {
    GateReading::new(state).evidence(evidence(order, attempt, commit, Coverage::Exact))
}
struct Fixture {
    _home: tempfile::TempDir,
    registry: GateRegistry,
    clock: Clock,
    rng: Rng,
}
impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        Self {
            registry: GateRegistry::new(home.path()),
            _home: home,
            clock: Clock(std::cell::Cell::new(1_000)),
            rng: Rng(1_000),
        }
    }
    fn link(&mut self, new: NewGate) -> Result<Linked, OrchestrationError> {
        self.clock.0.set(self.clock.0.get() + 1);
        self.registry.link(
            OWNER,
            new,
            &mut Transaction {
                clock: &self.clock,
                rng: &mut self.rng,
            },
        )
    }
    fn apply(&mut self, gate_id: &str, reading: GateReading) -> (Gate, GateOutcome) {
        self.clock.0.set(self.clock.0.get() + 1);
        self.registry
            .apply(
                OWNER,
                gate_id,
                reading,
                &mut Transaction {
                    clock: &self.clock,
                    rng: &mut self.rng,
                },
            )
            .unwrap()
    }
    fn resolve(
        &mut self,
        gate_id: &str,
        to: GateState,
        reason: Option<&str>,
    ) -> Result<(Gate, GateOutcome), OrchestrationError> {
        self.registry.resolve_approval(
            OWNER,
            gate_id,
            to,
            reason,
            &mut Transaction {
                clock: &self.clock,
                rng: &mut self.rng,
            },
        )
    }
    fn state(&self, gate_id: &str) -> GateState {
        self.registry.get(OWNER, gate_id).unwrap().state
    }
}

#[test]
fn hs2_5jbypv_every_gate_edge_is_allowed_or_refused_exactly() {
    use GateState::{AwaitingApproval, Failed, Passed, Pending, Superseded, Unknown};
    let allowed = [
        (Unknown, AwaitingApproval),
        (Unknown, Pending),
        (Unknown, Passed),
        (Unknown, Failed),
        (Unknown, Superseded),
        (AwaitingApproval, Pending),
        (AwaitingApproval, Unknown),
        (AwaitingApproval, Failed),
        (AwaitingApproval, Superseded),
        (AwaitingApproval, Passed),
        (Pending, Passed),
        (Pending, Failed),
        (Pending, Unknown),
        (Pending, Superseded),
        (Failed, Pending),
        (Failed, Passed),
        (Failed, Superseded),
        (Passed, Superseded),
    ];
    for from in GateState::ALL {
        for next in GateState::ALL {
            assert_eq!(
                from.may_become(next),
                allowed.contains(&(from, next)),
                "{from:?} -> {next:?}"
            );
        }
        assert_eq!(from.is_open(), matches!(from, Unknown | Pending));
        assert!(!from.may_become(from), "{from:?} is not its own edge");
    }
    assert!(
        GateState::ALL
            .iter()
            .all(|next| !Superseded.may_become(*next))
    );
    let codes: std::collections::BTreeSet<_> =
        GateState::ALL.iter().map(|state| state.code()).collect();
    assert_eq!(codes.len(), GateState::ALL.len());
    for state in GateState::ALL {
        assert_eq!(serde_json::to_value(state).unwrap(), state.code());
    }
}

#[test]
fn hs2_5jbypv_a_provider_reading_moves_every_state_exactly_as_the_table_says() {
    use GateOutcome::{Changed, Ignored, Unchanged};
    use GateState::{Failed, Passed, Pending, Superseded, Unknown};
    let a = sha('a');
    // (state the gate is in, reading state, expected outcome, expected state)
    let reading = |state: GateState| match state {
        Passed | Failed => verdict(state, 7, 1, &a),
        other => GateReading::new(other),
    };
    let cases = [
        (Unknown, Unknown, Unchanged, Unknown),
        (Unknown, Pending, Changed, Pending),
        (Unknown, Passed, Changed, Passed),
        (Unknown, Failed, Changed, Failed),
        (Unknown, Superseded, Changed, Superseded),
        (Pending, Unknown, Changed, Unknown),
        (Pending, Pending, Unchanged, Pending),
        (Pending, Passed, Changed, Passed),
        (Pending, Failed, Changed, Failed),
        (Pending, Superseded, Changed, Superseded),
        // A verdict is never replaced by less.
        (Failed, Unknown, Ignored("stale_reading"), Failed),
        (Failed, Pending, Changed, Pending),
        (Failed, Passed, Changed, Passed),
        (Failed, Failed, Unchanged, Failed),
        (Failed, Superseded, Changed, Superseded),
        (Passed, Unknown, Ignored("gate_final"), Passed),
        (Passed, Pending, Ignored("gate_final"), Passed),
        (Passed, Failed, Ignored("gate_final"), Passed),
        (Passed, Passed, Unchanged, Passed),
        (Passed, Superseded, Changed, Superseded),
        (Superseded, Unknown, Ignored("gate_superseded"), Superseded),
        (Superseded, Pending, Ignored("gate_superseded"), Superseded),
        (Superseded, Passed, Ignored("gate_superseded"), Superseded),
        (Superseded, Failed, Ignored("gate_superseded"), Superseded),
        (
            Superseded,
            Superseded,
            Ignored("gate_superseded"),
            Superseded,
        ),
    ];
    for (from, read, outcome, to) in cases {
        let mut fixture = Fixture::new();
        let mut gate = fixture.link(ci(10, &a)).unwrap().gate;
        // Walk to `from` by provider readings alone.
        let path: &[GateState] = match from {
            Unknown => &[],
            Pending => &[Pending],
            Failed => &[Failed],
            Passed => &[Passed],
            _ => &[Superseded],
        };
        for step in path {
            assert_eq!(gate.apply(reading(*step), 5), Changed);
        }
        if from == Unknown {
            // The first state carries a reason; a bare reading of the same state must
            // compare equal to be `Unchanged`.
            gate.reason = None;
        }
        let history = gate.history.len();
        assert_eq!(
            gate.apply(reading(read), 9),
            outcome,
            "{from:?} read {read:?}"
        );
        assert_eq!(gate.state, to, "{from:?} read {read:?}");
        assert_eq!(
            gate.history.len(),
            history + usize::from(outcome == Changed),
            "{from:?} read {read:?}"
        );
    }
}

#[test]
fn hs2_5jbypv_a_verdict_counts_only_with_evidence_that_covers_the_gate_commit() {
    let (a, b) = (sha('a'), sha('b'));
    let mut fixture = Fixture::new();
    let gate = fixture.link(ci(10, &a)).unwrap().gate;
    let covering = |coverage, commit: &str, provider: &str| GateEvidence {
        provider: provider.into(),
        ..evidence(3, 1, commit, coverage)
    };
    // (evidence, covers)
    let cases = [
        (Some(covering(Coverage::Exact, &a, PROVIDER)), true),
        // A descendant on the shipping branch that the provider attests contains it.
        (Some(covering(Coverage::Ancestor, &b, PROVIDER)), true),
        // "Exact" for another commit, and "ancestor" of itself, are contradictions.
        (Some(covering(Coverage::Exact, &b, PROVIDER)), false),
        (Some(covering(Coverage::Ancestor, &a, PROVIDER)), false),
        // Another provider's run says nothing about this gate.
        (Some(covering(Coverage::Exact, &a, "other-ci")), false),
        (None, false),
    ];
    for verdict in [GateState::Passed, GateState::Failed] {
        for (evidence, covers) in &cases {
            let mut gate = gate.clone();
            assert_eq!(
                gate.apply(GateReading::new(GateState::Pending), 2),
                GateOutcome::Changed
            );
            let reading = GateReading {
                evidence: evidence.clone(),
                ..GateReading::new(verdict)
            };
            assert_eq!(gate.apply(reading, 3), GateOutcome::Changed);
            if *covers {
                assert_eq!(gate.state, verdict);
                assert_eq!(gate.evidence, *evidence);
                assert_eq!(gate.reason, None);
            } else {
                assert_eq!(gate.state, GateState::Unknown, "{evidence:?}");
                assert_eq!(gate.evidence, None);
                assert_eq!(
                    gate.reason.as_deref(),
                    Some(if evidence.is_some() {
                        "evidence_not_covering"
                    } else {
                        "evidence_missing"
                    })
                );
            }
        }
    }
    // A deployment counts only when it deployed exactly the gate's commit.
    let mut deploy = fixture
        .link(deployment(10, &a, "deploy-1", &gate.gate_id))
        .unwrap()
        .gate;
    deploy.state = GateState::Pending;
    let ran = |coverage, commit: &str| GateEvidence {
        provider: "target-script".into(),
        exit_code: Some(0),
        ..evidence(1, 1, commit, coverage)
    };
    assert!(deploy.covered_by(&ran(Coverage::Exact, &a)));
    assert!(!deploy.covered_by(&ran(Coverage::Ancestor, &b)));
    // Evidence on a state that is not a verdict is dropped, never stored.
    let mut pending = gate.clone();
    let reading =
        GateReading::new(GateState::Pending).evidence(evidence(9, 1, &a, Coverage::Exact));
    assert_eq!(pending.apply(reading, 4), GateOutcome::Changed);
    assert_eq!(pending.evidence, None);
}

#[test]
fn hs2_5jbypv_late_duplicate_and_out_of_order_readings_never_move_a_gate_backwards() {
    let a = sha('a');
    let mut fixture = Fixture::new();
    let gate = fixture.link(ci(10, &a)).unwrap().gate.gate_id;
    assert!(fixture.link(ci(10, &a)).is_ok());
    // pending -> failed on run 5 attempt 1.
    assert_eq!(
        fixture.apply(&gate, GateReading::new(GateState::Pending)).1,
        GateOutcome::Changed
    );
    assert_eq!(
        fixture.apply(&gate, verdict(GateState::Failed, 5, 1, &a)).1,
        GateOutcome::Changed
    );
    let stored = std::fs::read(fixture._home.path().join("orchestration-gates.json")).unwrap();
    // The same reading again, and an older run's verdict arriving late, write nothing.
    for (reading, outcome) in [
        (verdict(GateState::Failed, 5, 1, &a), GateOutcome::Unchanged),
        (
            verdict(GateState::Passed, 4, 9, &a),
            GateOutcome::Ignored("stale_reading"),
        ),
        (
            verdict(GateState::Failed, 4, 1, &a),
            GateOutcome::Ignored("stale_reading"),
        ),
        (
            GateReading::new(GateState::Unknown).reason("watch_lost"),
            GateOutcome::Ignored("stale_reading"),
        ),
    ] {
        assert_eq!(fixture.apply(&gate, reading).1, outcome);
        assert_eq!(
            std::fs::read(fixture._home.path().join("orchestration-gates.json")).unwrap(),
            stored
        );
    }
    // A re-run of the same run fails again: same verdict, newer evidence, still news.
    let (after, outcome) = fixture.apply(&gate, verdict(GateState::Failed, 5, 2, &a));
    assert_eq!(outcome, GateOutcome::Changed);
    assert_eq!(after.evidence.as_ref().unwrap().attempt, 2);
    assert_eq!(after.history.last().unwrap().from, Some(GateState::Failed));
    // Watched again, then the third attempt passes; the interleaved older failure and a
    // late "pending" are ignored afterwards.
    assert_eq!(
        fixture.apply(&gate, GateReading::new(GateState::Pending)).1,
        GateOutcome::Changed
    );
    assert_eq!(
        fixture.apply(&gate, verdict(GateState::Failed, 5, 1, &a)).1,
        GateOutcome::Ignored("stale_reading")
    );
    assert_eq!(fixture.state(&gate), GateState::Pending);
    assert_eq!(
        fixture.apply(&gate, verdict(GateState::Passed, 5, 3, &a)).1,
        GateOutcome::Changed
    );
    for reading in [
        GateReading::new(GateState::Pending),
        verdict(GateState::Failed, 6, 1, &a),
        GateReading::new(GateState::Unknown),
    ] {
        assert_eq!(
            fixture.apply(&gate, reading).1,
            GateOutcome::Ignored("gate_final")
        );
    }
    let gate = fixture.registry.get(OWNER, &gate).unwrap();
    assert_eq!(gate.state, GateState::Passed);
    assert_eq!(gate.evidence.as_ref().unwrap().attempt, 3);
    let walked: Vec<_> = gate.history.iter().map(|entry| entry.to).collect();
    assert_eq!(
        walked,
        [
            GateState::Unknown,
            GateState::Pending,
            GateState::Failed,
            GateState::Failed,
            GateState::Pending,
            GateState::Passed
        ]
    );
    // A lost watch downgrades a pending gate and a found one restores it.
    let other = fixture.link(ci(11, &a)).unwrap().gate.gate_id;
    for (state, reason) in [
        (GateState::Pending, None),
        (GateState::Unknown, Some("watch_lost")),
        (GateState::Unknown, Some("watch_cancelled")),
        (GateState::Pending, None),
    ] {
        let mut reading = GateReading::new(state);
        reading.reason = reason.map(Into::into);
        let (after, outcome) = fixture.apply(&other, reading);
        assert_eq!(outcome, GateOutcome::Changed);
        assert_eq!((after.state, after.reason.as_deref()), (state, reason));
    }
}

#[test]
fn hs2_5jbypv_linking_is_idempotent_and_a_newer_commit_supersedes_the_older_link() {
    let (a, b) = (sha('a'), sha('b'));
    let mut fixture = Fixture::new();
    let first = fixture.link(ci(10, &a)).unwrap();
    assert!(first.created && first.superseded.is_empty());
    assert_eq!(first.gate.state, GateState::Unknown);
    // Repeating the link, whoever does it, returns the same gate and writes nothing.
    let again = fixture.link(ci(10, &a)).unwrap();
    assert!(!again.created);
    assert_eq!(again.gate, first.gate);
    // Another run, workflow or branch is its own slot.
    let other_run = fixture.link(ci(11, &a)).unwrap();
    let mut workflow = ci(10, &a);
    workflow.ci.as_mut().unwrap().workflow = "Platforms".into();
    let other_workflow = fixture.link(workflow).unwrap();
    assert!(other_run.created && other_workflow.created);
    assert!(other_run.superseded.is_empty() && other_workflow.superseded.is_empty());
    // The run moves to commit b: only the gate in the same slot is superseded.
    fixture.apply(&first.gate.gate_id, verdict(GateState::Passed, 1, 1, &a));
    let second = fixture.link(ci(10, &b)).unwrap();
    assert!(second.created);
    assert_eq!(second.superseded.len(), 1);
    let old = &second.superseded[0];
    assert_eq!(old.gate_id, first.gate.gate_id);
    assert_eq!(
        (
            old.state,
            old.reason.as_deref(),
            old.superseded_by.as_deref()
        ),
        (
            GateState::Superseded,
            Some("newer_gate_linked"),
            Some(second.gate.gate_id.as_str())
        )
    );
    // The verdict it had is kept in its history and evidence.
    assert_eq!(old.history[old.history.len() - 2].to, GateState::Passed);
    assert!(old.evidence.is_some());
    assert_eq!(fixture.state(&other_run.gate.gate_id), GateState::Unknown);
    assert_eq!(
        fixture.state(&other_workflow.gate.gate_id),
        GateState::Unknown
    );
    // Back to commit a: a new link, because the old one no longer stands.
    let third = fixture.link(ci(10, &a)).unwrap();
    assert!(third.created);
    assert_ne!(third.gate.gate_id, first.gate.gate_id);
    assert_eq!(third.superseded[0].gate_id, second.gate.gate_id);
    // Reads: by run, by commit, standing only.
    let list = |filter: GateFilter| {
        fixture
            .registry
            .list(OWNER, &filter)
            .unwrap()
            .into_iter()
            .map(|gate| gate.gate_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(list(GateFilter::default()).len(), 5);
    assert_eq!(
        list(GateFilter {
            run_id: Some(id(10)),
            current_only: true,
            ..GateFilter::default()
        }),
        [
            other_workflow.gate.gate_id.clone(),
            third.gate.gate_id.clone()
        ]
    );
    assert_eq!(
        list(GateFilter {
            commit: Some(b.clone()),
            ..GateFilter::default()
        }),
        *std::slice::from_ref(&second.gate.gate_id)
    );
    assert!(
        list(GateFilter {
            target_id: Some(id(2)),
            ..GateFilter::default()
        })
        .is_empty()
    );
    assert!(
        fixture
            .registry
            .list("other-owner", &GateFilter::default())
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        fixture.registry.get("other-owner", &first.gate.gate_id),
        Err(OrchestrationError::Gate(GateError::NotFound))
    ));
}

#[test]
fn hs2_5jbypv_a_deployment_waits_for_its_answer_and_follows_its_ci_gate() {
    let (a, b) = (sha('a'), sha('b'));
    let mut fixture = Fixture::new();
    let ci_gate = fixture.link(ci(10, &a)).unwrap().gate.gate_id;
    let first = fixture
        .link(deployment(10, &a, "deploy-1", &ci_gate))
        .unwrap();
    assert!(first.created);
    assert_eq!(first.gate.state, GateState::AwaitingApproval);
    let deploy = first.gate.gate_id.clone();
    // The same operation returns the same gate; the same id for other content conflicts.
    assert!(
        !fixture
            .link(deployment(10, &a, "deploy-1", &ci_gate))
            .unwrap()
            .created
    );
    let mut changed = deployment(10, &a, "deploy-1", &ci_gate);
    changed.deployment.as_mut().unwrap().script = "scripts/other.sh".into();
    assert!(matches!(
        fixture.link(changed),
        Err(OrchestrationError::Conflict)
    ));
    // No provider reading moves a gate that waits for a person.
    for state in [GateState::Pending, GateState::Passed, GateState::Unknown] {
        assert_eq!(
            fixture.apply(&deploy, GateReading::new(state)).1,
            GateOutcome::Ignored("awaiting_approval")
        );
    }
    // Nor does a reading put a gate into waiting.
    assert_eq!(
        fixture
            .apply(&ci_gate, GateReading::new(GateState::AwaitingApproval))
            .1,
        GateOutcome::Ignored("awaiting_approval")
    );
    // A person's answer allows exactly pending, unknown or failed, once.
    for refused in [GateState::Passed, GateState::Superseded] {
        assert!(matches!(
            fixture.resolve(&deploy, refused, None),
            Err(OrchestrationError::Gate(GateError::InvalidTransition))
        ));
    }
    let (gate, outcome) = fixture
        .resolve(
            &deploy,
            GateState::Unknown,
            Some("deployment_executor_unavailable"),
        )
        .unwrap();
    assert_eq!(outcome, GateOutcome::Changed);
    assert_eq!(gate.state, GateState::Unknown);
    assert_eq!(
        fixture
            .resolve(
                &deploy,
                GateState::Unknown,
                Some("deployment_executor_unavailable")
            )
            .unwrap()
            .1,
        GateOutcome::Unchanged
    );
    assert!(matches!(
        fixture.resolve(&deploy, GateState::Failed, Some("approval_denied")),
        Err(OrchestrationError::Gate(GateError::InvalidTransition))
    ));
    // A second request for the slot supersedes the one that never ran...
    let second = fixture
        .link(deployment(10, &a, "deploy-2", &ci_gate))
        .unwrap();
    assert_eq!(second.superseded[0].gate_id, deploy);
    let running = second.gate.gate_id.clone();
    fixture.resolve(&running, GateState::Pending, None).unwrap();
    // ...but one in progress refuses a second.
    assert!(matches!(
        fixture.link(deployment(10, &a, "deploy-3", &ci_gate)),
        Err(OrchestrationError::Gate(GateError::DeploymentActive))
    ));
    // Another environment is its own slot.
    let mut production = deployment(10, &a, "deploy-prod", &ci_gate);
    {
        let scope = production.deployment.as_mut().unwrap();
        scope.environment = "production".into();
        scope.production = true;
    }
    let waiting = fixture.link(production).unwrap().gate.gate_id;
    // The run moves to commit b: the waiting deployment is withdrawn with its CI gate,
    // the one in progress is a fact and stays.
    let moved = fixture.link(ci(10, &b)).unwrap();
    let withdrawn: Vec<_> = moved
        .superseded
        .iter()
        .map(|gate| (gate.gate_id.as_str(), gate.reason.as_deref()))
        .collect();
    assert_eq!(
        withdrawn,
        [
            (ci_gate.as_str(), Some("newer_gate_linked")),
            (waiting.as_str(), Some("required_gate_superseded"))
        ]
    );
    assert_eq!(fixture.state(&running), GateState::Pending);
    // Its typed result is still recorded.
    let ran = GateEvidence {
        provider: "target-script".into(),
        exit_code: Some(0),
        ..evidence(1, 1, &a, Coverage::Exact)
    };
    let (done, outcome) =
        fixture.apply(&running, GateReading::new(GateState::Passed).evidence(ran));
    assert_eq!(outcome, GateOutcome::Changed);
    assert_eq!(done.evidence.unwrap().exit_code, Some(0));
}

#[test]
fn hs2_5jbypv_the_summary_never_reads_missing_or_superseded_gates_as_passed() {
    let (a, b) = (sha('a'), sha('b'));
    let empty = summarize(Vec::new());
    assert_eq!(
        (empty.ci, empty.ci_reason.as_deref(), empty.deployment),
        (GateState::Unknown, Some("no_gate"), None)
    );
    let mut fixture = Fixture::new();
    let gate = |fixture: &mut Fixture, workflow: &str, commit: &str| {
        let mut new = ci(10, commit);
        new.ci.as_mut().unwrap().workflow = workflow.into();
        fixture.link(new).unwrap().gate
    };
    let states = [
        GateState::Passed,
        GateState::Pending,
        GateState::Unknown,
        GateState::Failed,
    ];
    // Every pair of CI gates rolls up to the worse one: failed > unknown > pending > passed.
    for (worse, left) in states.iter().enumerate() {
        for (better, right) in states.iter().enumerate() {
            let mut one = gate(&mut fixture, &format!("one-{worse}-{better}"), &a);
            let mut two = gate(&mut fixture, &format!("two-{worse}-{better}"), &a);
            one.state = *left;
            two.state = *right;
            let summary = summarize(vec![one, two]);
            assert_eq!(
                summary.ci,
                states[worse.max(better)],
                "{left:?} + {right:?}"
            );
            assert_eq!(summary.commits, *std::slice::from_ref(&a));
            assert_eq!(summary.deployment, None);
        }
    }
    // Only superseded links left: unknown again, with the count of what was left out.
    let mut old = gate(&mut fixture, "CI", &a);
    old.state = GateState::Superseded;
    let summary = summarize(vec![old.clone()]);
    assert_eq!(
        (summary.ci, summary.ci_reason.as_deref(), summary.superseded),
        (GateState::Unknown, Some("no_gate"), 1)
    );
    assert!(summary.gates.is_empty() && summary.commits.is_empty());
    // A passed gate beside a superseded one is passed, for the standing commit only.
    let mut current = gate(&mut fixture, "CI", &b);
    current.state = GateState::Passed;
    current.reason = None;
    let mut deploy = fixture
        .link(deployment(10, &b, "deploy-1", &current.gate_id))
        .unwrap()
        .gate;
    let summary = summarize(vec![old, current.clone(), deploy.clone()]);
    assert_eq!(
        (summary.ci, summary.ci_reason, summary.deployment),
        (GateState::Passed, None, Some(GateState::AwaitingApproval))
    );
    assert_eq!(summary.commits, *std::slice::from_ref(&b));
    assert_eq!(summary.gates.len(), 2);
    // The deployment rollup never changes the CI verdict, and the reverse.
    deploy.state = GateState::Failed;
    let summary = summarize(vec![current, deploy]);
    assert_eq!(
        (summary.ci, summary.deployment),
        (GateState::Passed, Some(GateState::Failed))
    );
}

#[test]
fn hs2_5jbypv_requests_are_validated_and_a_damaged_file_is_never_evaluated() {
    let a = sha('a');
    let mut fixture = Fixture::new();
    // A read of nothing creates nothing.
    assert!(
        fixture
            .registry
            .list(OWNER, &GateFilter::default())
            .unwrap()
            .is_empty()
    );
    assert!(
        std::fs::read_dir(fixture._home.path())
            .unwrap()
            .next()
            .is_none()
    );
    type Change = Box<dyn Fn(&mut NewGate)>;
    let bad: Vec<Change> = vec![
        Box::new(|new| new.commit = "abc1234".into()),
        Box::new(|new| new.commit = "main".into()),
        Box::new(|new| new.commit = sha('A')),
        Box::new(|new| new.provider = "git hub".into()),
        Box::new(|new| new.run_id = "run-1".into()),
        Box::new(|new| new.ci = None),
        Box::new(|new| new.ci.as_mut().unwrap().workflow = " ".into()),
        Box::new(|new| new.ci.as_mut().unwrap().branch = "ma\nin".into()),
        Box::new(|new| new.state = GateState::Passed),
        Box::new(|new| new.state = GateState::Pending),
        Box::new(|new| new.state = GateState::AwaitingApproval),
        Box::new(|new| new.reason = Some("it passed, trust me".into())),
    ];
    for change in &bad {
        let mut new = ci(10, &a);
        change(&mut new);
        assert!(matches!(
            fixture.link(new),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    let ci_gate = fixture.link(ci(10, &a)).unwrap().gate.gate_id;
    for script in [
        "/usr/local/bin/deploy",
        "../deploy.sh",
        "scripts/../../deploy.sh",
        "scripts//deploy.sh",
        "scripts/deploy.sh --force",
        "scripts\\deploy.sh",
        "",
    ] {
        let mut new = deployment(10, &a, "deploy-1", &ci_gate);
        new.deployment.as_mut().unwrap().script = script.into();
        assert!(
            matches!(fixture.link(new), Err(OrchestrationError::Invalid(_))),
            "{script:?}"
        );
        assert!(valid_script(script).is_err());
    }
    assert!(valid_script("scripts/deploy.sh").is_ok());
    let mut both = deployment(10, &a, "deploy-1", &ci_gate);
    both.ci = ci(10, &a).ci;
    assert!(matches!(
        fixture.link(both),
        Err(OrchestrationError::Invalid(_))
    ));
    // Evidence and reasons are identifiers; anything else is refused before it is stored.
    let mut reading = verdict(GateState::Passed, 1, 1, &a);
    reading.evidence.as_mut().unwrap().reference = "run 1: all green".into();
    let mut tx_rng = Rng(9_000);
    let mut tx = Transaction {
        clock: &fixture.clock,
        rng: &mut tx_rng,
    };
    assert!(matches!(
        fixture.registry.apply(OWNER, &ci_gate, reading, &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    assert!(matches!(
        fixture.registry.apply(
            OWNER,
            &ci_gate,
            GateReading::new(GateState::Unknown).reason("see the log above"),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    assert!(matches!(
        fixture.registry.apply(
            OWNER,
            &id(77),
            GateReading::new(GateState::Pending),
            &mut tx
        ),
        Err(OrchestrationError::Gate(GateError::NotFound))
    ));
    assert_eq!(fixture.state(&ci_gate), GateState::Unknown);
    // A stored gate that no longer validates, or a newer format, is an error.
    let path = fixture._home.path().join("orchestration-gates.json");
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    stored["gates"][0]["commit"] = "HEAD".into();
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    assert!(matches!(
        fixture.registry.list(OWNER, &GateFilter::default()),
        Err(OrchestrationError::Corrupt)
    ));
    stored["gates"][0]["commit"] = a.clone().into();
    stored["schema_version"] = 2.into();
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    assert!(
        fixture
            .registry
            .list(OWNER, &GateFilter::default())
            .is_err()
    );
    let codes: std::collections::BTreeSet<_> = [
        GateError::NotFound,
        GateError::ProviderUnsupported,
        GateError::ProviderUnavailable,
        GateError::NotPassed,
        GateError::InvalidTransition,
        GateError::RepositoryMismatch,
        GateError::DeploymentActive,
        GateError::ArtifactMismatch,
        GateError::DeploymentScriptUntracked,
    ]
    .map(GateError::code)
    .into_iter()
    .collect();
    assert_eq!(codes.len(), 9);
    for code in &codes {
        assert_eq!(GateError::from_code(code).map(GateError::code), Some(*code));
    }
    assert_eq!(GateError::from_code("conflict"), None);
    assert_eq!(
        OrchestrationError::Gate(GateError::NotPassed).code(),
        "gate_not_passed"
    );
}

#[test]
fn hs2_5jbypv_history_is_bounded_and_capacity_forgets_only_settled_gates() {
    let a = sha('a');
    let mut fixture = Fixture::new();
    let gate = fixture.link(ci(10, &a)).unwrap().gate.gate_id;
    // Flap between pending and unknown far past the history bound.
    for round in 0..MAX_GATE_HISTORY * 2 {
        let state = if round % 2 == 0 {
            GateState::Pending
        } else {
            GateState::Unknown
        };
        assert_eq!(
            fixture.apply(&gate, GateReading::new(state)).1,
            GateOutcome::Changed
        );
    }
    let flapped = fixture.registry.get(OWNER, &gate).unwrap();
    assert_eq!(flapped.history.len(), MAX_GATE_HISTORY);
    assert_eq!(flapped.history[0].from, None);
    assert_eq!(flapped.history.last().unwrap().to, GateState::Unknown);
    // A registry full of one run's superseded commits plus the open gate above: each
    // further link supersedes the last and forgets the oldest superseded gate, so
    // linking never fails.
    let commit = |n: usize| format!("{n:040x}");
    let seed = fixture.link(ci(20, &commit(0))).unwrap().gate;
    let mut gates = vec![flapped.clone()];
    gates.extend((1..MAX_GATES - 1).map(|n| Gate {
        gate_id: id(300_000 + n as u128),
        commit: commit(n),
        state: GateState::Superseded,
        ..seed.clone()
    }));
    gates.push(seed);
    std::fs::write(
        fixture._home.path().join("orchestration-gates.json"),
        serde_json::to_vec(&serde_json::json!({"schema_version": 1, "gates": gates})).unwrap(),
    )
    .unwrap();
    for n in MAX_GATES..MAX_GATES + 5 {
        assert!(fixture.link(ci(20, &commit(n))).unwrap().created);
    }
    let all = fixture
        .registry
        .list(OWNER, &GateFilter::default())
        .unwrap();
    assert_eq!(all.len(), MAX_GATES);
    // The open gate of the other run was never forgotten.
    assert!(all.iter().any(|other| other.gate_id == gate));
    let standing = fixture
        .registry
        .list(
            OWNER,
            &GateFilter {
                run_id: Some(id(20)),
                current_only: true,
                ..GateFilter::default()
            },
        )
        .unwrap();
    assert_eq!(standing.len(), 1);
    assert_eq!(standing[0].commit, commit(MAX_GATES + 4));
}

#[test]
fn hs2_5jbypv_only_open_gates_fill_the_registry_to_a_capacity_error() {
    let a = sha('a');
    let home = tempfile::tempdir().unwrap();
    let registry = GateRegistry::new(home.path());
    // Write a full file of open gates directly; linking one at a time would take long.
    let clock = Clock(std::cell::Cell::new(5));
    let mut rng = Rng(50_000);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let seed = registry.link(OWNER, ci(1, &a), &mut tx).unwrap().gate;
    let gates: Vec<Gate> = (0..MAX_GATES as u128)
        .map(|n| Gate {
            gate_id: id(100_000 + n),
            run_id: id(200_000 + n),
            ..seed.clone()
        })
        .collect();
    std::fs::write(
        home.path().join("orchestration-gates.json"),
        serde_json::to_vec(&serde_json::json!({"schema_version": 1, "gates": gates})).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        registry.link(OWNER, ci(2, &a), &mut tx),
        Err(OrchestrationError::Capacity)
    ));
    // One verdict frees a place.
    registry
        .apply(
            OWNER,
            &id(100_000),
            verdict(GateState::Passed, 1, 1, &a),
            &mut tx,
        )
        .unwrap();
    assert!(registry.link(OWNER, ci(2, &a), &mut tx).unwrap().created);
    assert!(matches!(
        registry.get(OWNER, &id(100_000)),
        Err(OrchestrationError::Gate(GateError::NotFound))
    ));
}

fn review(run: u128, commit: &str) -> NewGate {
    NewGate {
        run_id: id(run),
        target_id: id(1),
        kind: GateKind::Review,
        provider: REVIEW_PROVIDER.into(),
        commit: commit.into(),
        ci: None,
        deployment: None,
        state: GateState::AwaitingApproval,
        reason: Some(REVIEW_REQUESTED.into()),
        linked_by: Some("worker-1".into()),
    }
}

/// HS2-GD3NNK: a review gate starts requested; requesting it again for the same commit
/// is the same gate; approval and changes requested are each one edge from requested,
/// repeated without change, and refused from anywhere else. A new request after an
/// answer, or for another commit, supersedes the old review. Only the review roll-up
/// carries it; CI and deployment are untouched.
#[test]
fn hs2_gd3nnk_a_review_gate_is_requested_then_approved_or_sent_back() {
    let mut f = Fixture::new();
    let mark = |f: &mut Fixture, gate_id: &str, mark: ReviewMark| {
        f.clock.0.set(f.clock.0.get() + 1);
        f.registry.mark_review(
            OWNER,
            gate_id,
            mark,
            &mut Transaction {
                clock: &f.clock,
                rng: &mut f.rng,
            },
        )
    };
    // It starts requested and nowhere else.
    let mut approved_first = review(7, &sha('a'));
    approved_first.state = GateState::Passed;
    assert!(f.link(approved_first).is_err());
    let mut scoped = review(7, &sha('a'));
    scoped.deployment = deployment(7, &sha('a'), "op", &id(9)).deployment;
    assert!(f.link(scoped).is_err());
    let first = f.link(review(7, &sha('a'))).unwrap();
    assert!(first.created);
    let again = f.link(review(7, &sha('a'))).unwrap();
    assert!(!again.created);
    assert_eq!(again.gate.gate_id, first.gate.gate_id);
    let id_1 = first.gate.gate_id.clone();
    // Changes requested, the same mark again unchanged, and no way back to requested.
    let (sent_back, outcome) = mark(&mut f, &id_1, ReviewMark::ChangesRequested).unwrap();
    assert_eq!(outcome, GateOutcome::Changed);
    assert_eq!(
        (sent_back.state, sent_back.reason.as_deref()),
        (GateState::Failed, Some(REVIEW_CHANGES_REQUESTED))
    );
    assert_eq!(
        mark(&mut f, &id_1, ReviewMark::ChangesRequested).unwrap().1,
        GateOutcome::Unchanged
    );
    for refused in [ReviewMark::Approved, ReviewMark::Requested] {
        assert!(matches!(
            mark(&mut f, &id_1, refused),
            Err(OrchestrationError::Gate(GateError::InvalidTransition))
        ));
    }
    // A new request for the same commit after the answer is a new review.
    let second = f.link(review(7, &sha('a'))).unwrap();
    assert!(second.created);
    assert_eq!(second.superseded.len(), 1);
    assert_eq!(f.state(&id_1), GateState::Superseded);
    let id_2 = second.gate.gate_id.clone();
    let (approved, _) = mark(&mut f, &id_2, ReviewMark::Approved).unwrap();
    assert_eq!(
        (approved.state, approved.reason.as_deref()),
        (GateState::Passed, Some(REVIEW_APPROVED))
    );
    assert_eq!(
        approved
            .history
            .iter()
            .map(|entry| entry.to)
            .collect::<Vec<_>>(),
        [GateState::AwaitingApproval, GateState::Passed]
    );
    // A CI gate cannot be marked as a review.
    let ci_gate = f.link(ci(7, &sha('a'))).unwrap().gate.gate_id;
    assert!(mark(&mut f, &ci_gate, ReviewMark::Approved).is_err());
    let summary = summarize(f.registry.list(OWNER, &GateFilter::default()).unwrap());
    assert_eq!(summary.review, Some(GateState::Passed));
    assert_eq!(summary.deployment, None);
    assert_eq!(summary.commits, [sha('a')]);
    // Another commit's request supersedes the approved review.
    let third = f.link(review(7, &sha('b'))).unwrap();
    assert_eq!(third.superseded.len(), 1);
    let summary = summarize(f.registry.list(OWNER, &GateFilter::default()).unwrap());
    assert_eq!(summary.review, Some(GateState::AwaitingApproval));
    assert_eq!(
        ReviewMark::Requested.state(),
        (GateState::AwaitingApproval, REVIEW_REQUESTED)
    );
}
