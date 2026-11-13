//! HS2-WBREDM reconciliation rules, the control transitions and their registry
//! sequences; synthetic homes only.

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
const NOW: u64 = 1_000;
const LIVE: u64 = 5_000;
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}

struct Bench {
    _home: tempfile::TempDir,
    registry: RunRegistry,
    clock: Clock,
    rng: Rng,
    counter: u32,
}
impl Bench {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        Self {
            registry: RunRegistry::new(home.path()),
            _home: home,
            clock: Clock(NOW.into()),
            rng: Rng(1),
            counter: 0,
        }
    }
    fn update(&mut self, run: &Run, update: RunUpdate) -> Result<Run, OrchestrationError> {
        let mut tx = Transaction {
            clock: &self.clock,
            rng: &mut self.rng,
        };
        self.registry.update(OWNER, &run.run_id, update, &mut tx)
    }
    /// A run walked to `state` along allowed edges. Its worker is `worker-a`; a
    /// dispatcher run learns it when it is dispatched.
    fn run(&mut self, state: RunState, mode: DispatchMode) -> Run {
        use RunState::{Cancelled, Completed, Dispatched, Failed, Intervened, Queued, Requested};
        self.counter += 1;
        let mut tx = Transaction {
            clock: &self.clock,
            rng: &mut self.rng,
        };
        let (mut run, _) = self
            .registry
            .begin(
                OWNER,
                NewRun {
                    operation_id: format!("op-{}", self.counter),
                    ticket: RunTicket {
                        connection_id: "owner-source".into(),
                        native_id: id(500),
                    },
                    target: RunTarget {
                        target_id: id(u128::from(self.counter)),
                        connection_id: "target-source".into(),
                        native_id: format!("t{}", self.counter),
                    },
                    mode,
                    worker_id: (mode == DispatchMode::Direct).then(|| "worker-a".into()),
                },
                Some("scheduler".into()),
                &mut tx,
            )
            .unwrap();
        let path: &[RunState] = match (state, mode) {
            (Requested, _) => &[],
            (Queued, _) => &[Queued],
            (Dispatched, DispatchMode::Direct) => &[Dispatched],
            (Dispatched, DispatchMode::Dispatcher) => &[Queued, Dispatched],
            (Intervened, DispatchMode::Direct) => &[Dispatched, Intervened],
            (Intervened, DispatchMode::Dispatcher) => &[Queued, Dispatched, Intervened],
            (Completed, _) => &[Queued, Completed],
            (Failed, _) => &[Failed],
            (Cancelled, _) => &[Cancelled],
        };
        for step in path {
            run = self
                .update(
                    &run,
                    RunUpdate {
                        to: Some(*step),
                        reason: Some("setup".into()),
                        worker_id: (*step == Dispatched).then(|| "worker-a".into()),
                        lease_expires_at_ms: (*step == Dispatched).then_some(Some(LIVE)),
                        ..RunUpdate::default()
                    },
                )
                .unwrap();
        }
        assert_eq!(run.state, state);
        run
    }
    fn apply(&mut self, run: &Run, aligned: &Alignment) -> Run {
        self.update(
            run,
            RunUpdate {
                to: aligned.to,
                reason: aligned.reason.map(Into::into),
                worker_id: aligned.worker_id.clone(),
                lease_expires_at_ms: aligned.lease_expires_at_ms,
                ..RunUpdate::default()
            },
        )
        .unwrap()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claim {
    Free,
    Ours,
    OursLapsed,
    Other,
}
const CLAIMS: [Claim; 4] = [Claim::Free, Claim::Ours, Claim::OursLapsed, Claim::Other];
const MODES: [DispatchMode; 2] = [DispatchMode::Direct, DispatchMode::Dispatcher];

fn facts(status: &str, claim: Claim) -> TargetFacts {
    let (claimed_by, lease) = match claim {
        Claim::Free => (None, None),
        Claim::Ours => (Some("worker-a"), Some(LIVE)),
        Claim::OursLapsed => (Some("worker-a"), Some(NOW - 1)),
        Claim::Other => (Some("human-1"), Some(LIVE)),
    };
    TargetFacts {
        exists: true,
        status: status.into(),
        claimed_by: claimed_by.map(Into::into),
        lease_expires_at_ms: lease,
    }
}
fn target(facts: TargetFacts) -> Observations {
    Observations::target_only(Observed::Verified(facts))
}
fn reasons(assessment: &Assessment) -> Vec<(ExternalSource, Verification, Option<&str>)> {
    assessment
        .findings
        .iter()
        .map(|found| (found.source, found.verification, found.reason.as_deref()))
        .collect()
}

#[test]
fn hs2_wbredm_alignment_follows_the_target_ticket_for_every_state_mode_and_claim() {
    use Claim::{Free, Other, Ours, OursLapsed};
    use DispatchMode::{Direct, Dispatcher};
    use RunState::{Dispatched, Intervened, Queued, Requested};
    // (state, mode, claim) -> (new state, reason, worker learned).
    let expected = |state, mode, claim| match (state, mode, claim) {
        (Queued, _, Ours) => Some((Dispatched, "assigned_by_dispatcher", Some("worker-a"))),
        (Queued, _, Other) => Some((Dispatched, "assigned_by_dispatcher", Some("human-1"))),
        (Requested, Dispatcher, Ours) => {
            Some((Dispatched, "assigned_by_dispatcher", Some("worker-a")))
        }
        (Requested, Dispatcher, Other) => {
            Some((Dispatched, "assigned_by_dispatcher", Some("human-1")))
        }
        (Requested | Intervened, Direct, Ours) => Some((Dispatched, "claim_confirmed", None)),
        (Dispatched, _, Other) => Some((Intervened, "claim_taken", None)),
        (Dispatched, _, Free | OursLapsed) => Some((Intervened, "claim_released", None)),
        _ => None,
    };
    let mut bench = Bench::new();
    for state in RunState::ALL {
        for mode in MODES {
            for claim in CLAIMS {
                let run = bench.run(state, mode);
                let label = format!("{state:?} {mode:?} {claim:?}");
                let seen = facts("started", claim);
                let aligned = align(&run, &seen, NOW);
                if state.is_terminal() {
                    assert!(aligned.is_none(), "{label}");
                    // Nothing moves a finished run, whatever the target shows.
                    for gone in [TargetFacts::missing(), facts("completed", claim)] {
                        assert!(align(&run, &gone, NOW).is_none(), "{label}");
                    }
                    continue;
                }
                assert_eq!(
                    aligned.to.map(|to| (
                        to,
                        aligned.reason.unwrap(),
                        aligned.worker_id.as_deref()
                    )),
                    expected(state, mode, claim),
                    "{label}"
                );
                // Every step is an edge the state machine allows, and applying it
                // leaves nothing further to align: reconciling twice changes nothing.
                if let Some(to) = aligned.to {
                    assert!(state.may_become(to), "{label}");
                }
                let settled = bench.apply(&run, &aligned);
                assert!(align(&settled, &seen, NOW).is_none(), "{label}");
                // What is still mismatched afterwards is only what the target cannot
                // settle by itself.
                let assessment = assess(&settled, &target(seen.clone()), NOW);
                let left: Vec<_> = reasons(&assessment)
                    .into_iter()
                    .filter(|found| found.1 == Verification::Mismatched)
                    .collect();
                if (state, mode, claim) == (Requested, Direct, Other) {
                    assert_eq!(
                        left,
                        [(
                            ExternalSource::TargetClaim,
                            Verification::Mismatched,
                            Some("claim_held_by_other")
                        )],
                        "{label}"
                    );
                    assert_eq!(assessment.disposition, Disposition::NeedsAttention);
                } else {
                    assert!(left.is_empty(), "{label}: {left:?}");
                }
                // A finished, deleted or missing ticket ends any unfinished run.
                for (gone, to, reason) in [
                    (
                        facts("completed", claim),
                        RunState::Completed,
                        "target_completed",
                    ),
                    (
                        facts("verified", claim),
                        RunState::Completed,
                        "target_completed",
                    ),
                    (
                        facts("archive", claim),
                        RunState::Completed,
                        "target_completed",
                    ),
                    (facts("deleted", claim), RunState::Failed, "target_deleted"),
                    (
                        TargetFacts::missing(),
                        RunState::Failed,
                        "target_ticket_not_found",
                    ),
                ] {
                    let run = bench.run(state, mode);
                    let aligned = align(&run, &gone, NOW);
                    assert_eq!((aligned.to, aligned.reason), (Some(to), Some(reason)));
                    // Before it is applied the ticket source shows the mismatch.
                    assert_eq!(
                        assess(&run, &target(gone.clone()), NOW)
                            .finding(ExternalSource::TargetTicket)
                            .unwrap()
                            .reason
                            .as_deref(),
                        Some(reason),
                        "{label}"
                    );
                    // Always an allowed edge, so no unfinished run can get stuck on a
                    // ticket that is gone.
                    assert!(run.state.may_become(to), "{label} -> {to:?}");
                    let ended = bench.apply(&run, &aligned);
                    let over = assess(&ended, &target(gone), NOW);
                    assert_eq!(over.disposition, Disposition::Finished);
                    assert!(over.findings.is_empty() && over.unverified().is_empty());
                }
            }
        }
    }
}

#[test]
fn hs2_wbredm_a_renewed_lease_is_adopted_and_a_lapsed_one_is_named() {
    let mut bench = Bench::new();
    let run = bench.run(RunState::Dispatched, DispatchMode::Direct);
    let mut renewed = facts("started", Claim::Ours);
    renewed.lease_expires_at_ms = Some(LIVE + 60_000);
    assert_eq!(
        assess(&run, &target(renewed.clone()), NOW)
            .finding(ExternalSource::RunLease)
            .unwrap()
            .reason
            .as_deref(),
        Some("lease_differs")
    );
    let aligned = align(&run, &renewed, NOW);
    assert_eq!(aligned.to, None);
    assert_eq!(aligned.lease_expires_at_ms, Some(Some(LIVE + 60_000)));
    let settled = bench.apply(&run, &aligned);
    assert_eq!(settled.history.len(), run.history.len());
    let steady = assess(&settled, &target(renewed), NOW);
    assert_eq!(steady.disposition, Disposition::Continue);
    assert_eq!(steady.live, [ExternalSource::TargetClaim]);
    // The worker's name is still on the ticket but its lease ran out.
    let lapsed = assess(&run, &target(facts("started", Claim::OursLapsed)), NOW);
    assert_eq!(
        reasons(&lapsed)[2..4],
        [
            (
                ExternalSource::TargetClaim,
                Verification::Mismatched,
                Some("claim_released")
            ),
            (
                ExternalSource::RunLease,
                Verification::Mismatched,
                Some("lease_lapsed")
            ),
        ]
    );
    assert!(lapsed.live.is_empty());
    // A lease exactly at the clock is lapsed, one millisecond later is live.
    let mut edge = facts("started", Claim::Ours);
    edge.lease_expires_at_ms = Some(NOW);
    assert_eq!(edge.live_holder(NOW), None);
    edge.lease_expires_at_ms = Some(NOW + 1);
    assert_eq!(edge.live_holder(NOW), Some("worker-a"));
}

#[test]
fn hs2_wbredm_every_source_is_verified_mismatched_or_unverifiable_with_its_reason() {
    use ExternalSource::{
        CiRun, Deployment, RunLease, Session, TargetClaim, TargetReachability, TargetTicket,
    };
    use Verification::{Mismatched, Unverifiable, Verified};
    let mut bench = Bench::new();
    let run = bench.run(RunState::Dispatched, DispatchMode::Direct);
    // A healthy run: the target sources agree; the sources nobody reports yet are
    // named as unverifiable and do not hold the run back.
    let healthy = assess(&run, &target(facts("started", Claim::Ours)), NOW);
    assert_eq!(
        reasons(&healthy),
        [
            (TargetReachability, Verified, None),
            (TargetTicket, Verified, None),
            (TargetClaim, Verified, None),
            (RunLease, Verified, None),
            (CiRun, Unverifiable, Some(SOURCE_NOT_WIRED)),
            (Deployment, Unverifiable, Some(SOURCE_NOT_WIRED)),
            (Session, Unverifiable, Some(SOURCE_NOT_WIRED)),
        ]
    );
    assert_eq!(healthy.disposition, Disposition::Continue);
    assert_eq!(healthy.unverified(), [CiRun, Deployment, Session]);
    assert_eq!(
        healthy
            .findings
            .iter()
            .map(|f| f.source)
            .collect::<Vec<_>>(),
        ExternalSource::ALL
    );
    // New work is never started for a source that could not be verified, nor where
    // work is already going on.
    for source in [CiRun, Deployment, Session, TargetClaim] {
        assert!(!healthy.may_start(source), "{source:?}");
    }
    assert!(healthy.may_start(TargetTicket));
    // The target does not answer: all four target sources carry the reason, the run
    // is held, and nothing may be started.
    let dark = assess(
        &run,
        &Observations::target_only(Observed::Unverifiable("target_unavailable".into())),
        NOW,
    );
    assert_eq!(dark.disposition, Disposition::HoldUnverified);
    assert_eq!(dark.unverified(), ExternalSource::ALL);
    for found in &dark.findings[..4] {
        assert!(found.source.is_target());
        assert_eq!(found.reason.as_deref(), Some("target_unavailable"));
    }
    assert!(
        ExternalSource::ALL
            .iter()
            .all(|source| !dark.may_start(*source))
    );
    assert!(dark.live.is_empty());
    // Work outside the ticket: recorded and live must be the same thing.
    let work = |recorded: Option<&str>, live: Option<&str>| {
        Observed::Verified(ExternalWork {
            recorded: recorded.map(Into::into),
            live: live.map(Into::into),
        })
    };
    for (recorded, live, verification, reason, may_start, disposition) in [
        (None, None, Verified, None, true, Disposition::Continue),
        (
            Some("s1"),
            Some("s1"),
            Verified,
            None,
            false,
            Disposition::Continue,
        ),
        // A surviving session the run never recorded: adopt it, start no second one.
        (
            None,
            Some("s1"),
            Mismatched,
            Some("unrecorded_live_work"),
            false,
            Disposition::NeedsAttention,
        ),
        (
            Some("s1"),
            None,
            Mismatched,
            Some("recorded_work_missing"),
            true,
            Disposition::NeedsAttention,
        ),
        (
            Some("s1"),
            Some("s2"),
            Mismatched,
            Some("different_live_work"),
            false,
            Disposition::NeedsAttention,
        ),
    ] {
        for source in [CiRun, Deployment, Session] {
            let mut observed = target(facts("started", Claim::Ours));
            match source {
                CiRun => observed.ci = work(recorded, live),
                Deployment => observed.deployment = work(recorded, live),
                _ => observed.session = work(recorded, live),
            }
            let found = assess(&run, &observed, NOW);
            let label = format!("{source:?} {recorded:?} {live:?}");
            assert_eq!(
                reasons(&found)
                    .into_iter()
                    .find(|entry| entry.0 == source)
                    .unwrap(),
                (source, verification, reason),
                "{label}"
            );
            assert_eq!(found.may_start(source), may_start, "{label}");
            assert_eq!(found.live.contains(&source), live.is_some(), "{label}");
            assert_eq!(found.disposition, disposition, "{label}");
        }
    }
    // A wired source that fails to answer holds the run, unlike one not wired.
    let mut flaky = target(facts("started", Claim::Ours));
    flaky.deployment = Observed::Unverifiable("deployment_provider_unreachable".into());
    let held = assess(&run, &flaky, NOW);
    assert_eq!(held.disposition, Disposition::HoldUnverified);
    assert_eq!(
        held.finding(Deployment).unwrap().reason.as_deref(),
        Some("deployment_provider_unreachable")
    );
    // Codes are stable and distinct.
    let codes: std::collections::BTreeSet<_> =
        ExternalSource::ALL.iter().map(|s| s.code()).collect();
    assert_eq!(codes.len(), ExternalSource::ALL.len());
    let dispositions: std::collections::BTreeSet<_> =
        Disposition::ALL.iter().map(|d| d.code()).collect();
    assert_eq!(dispositions.len(), Disposition::ALL.len());
    for disposition in Disposition::ALL {
        assert_eq!(
            serde_json::to_value(disposition).unwrap(),
            disposition.code()
        );
    }
    for source in ExternalSource::ALL {
        assert_eq!(serde_json::to_value(source).unwrap(), source.code());
    }
}

#[test]
fn hs2_wbredm_the_disposition_says_what_may_happen_next_in_every_state() {
    use DispatchMode::{Direct, Dispatcher};
    use RunState::{Dispatched, Intervened, Queued, Requested};
    let mut bench = Bench::new();
    for (state, mode, claim, expected) in [
        // Nothing is held: repeating the dispatch with the same operation id is safe.
        (Requested, Direct, Claim::Free, Disposition::RetryDispatch),
        (
            Requested,
            Dispatcher,
            Claim::Free,
            Disposition::RetryDispatch,
        ),
        // A lapsed lease holds nothing either.
        (
            Requested,
            Direct,
            Claim::OursLapsed,
            Disposition::RetryDispatch,
        ),
        (Requested, Direct, Claim::Other, Disposition::NeedsAttention),
        (
            Queued,
            Dispatcher,
            Claim::Free,
            Disposition::WaitForDispatcher,
        ),
        (Dispatched, Direct, Claim::Ours, Disposition::Continue),
        (Dispatched, Dispatcher, Claim::Ours, Disposition::Continue),
        (Intervened, Direct, Claim::Free, Disposition::NeedsAttention),
        (
            Intervened,
            Direct,
            Claim::Other,
            Disposition::NeedsAttention,
        ),
    ] {
        let run = bench.run(state, mode);
        let seen = facts("started", claim);
        let label = format!("{state:?} {mode:?} {claim:?}");
        assert_eq!(
            assess(&run, &target(seen.clone()), NOW).disposition,
            expected,
            "{label}"
        );
        // A pending cancel is repeated before anything else is resumed.
        let cancelling = bench
            .update(
                &run,
                RunUpdate {
                    cancel_requested: Some(true),
                    ..RunUpdate::default()
                },
            )
            .unwrap();
        let pending = assess(&cancelling, &target(seen.clone()), NOW).disposition;
        assert_eq!(pending, Disposition::RetryCancel, "{label}");
        // But not while the target cannot be verified.
        let dark = Observations::target_only(Observed::Unverifiable("target_unavailable".into()));
        assert_eq!(
            assess(&cancelling, &dark, NOW).disposition,
            Disposition::HoldUnverified
        );
        // A pause outranks everything, verified or not.
        let paused = bench
            .update(
                &cancelling,
                RunUpdate {
                    control: Some(RunControl::Pause),
                    paused: Some(Some(RunPause {
                        since_ms: NOW,
                        reason: None,
                        actor_id: Some("maintainer".into()),
                    })),
                    ..RunUpdate::default()
                },
            )
            .unwrap();
        for observed in [target(seen.clone()), dark] {
            assert_eq!(
                assess(&paused, &observed, NOW).disposition,
                Disposition::Paused,
                "{label}"
            );
        }
    }
}

fn control(control: RunControl, reason: &str) -> RunUpdate {
    RunUpdate {
        control: Some(control),
        reason: Some(reason.into()),
        actor_id: Some("maintainer".into()),
        ..RunUpdate::default()
    }
}
fn pause() -> RunUpdate {
    RunUpdate {
        paused: Some(Some(RunPause {
            since_ms: NOW,
            reason: Some("maintenance".into()),
            actor_id: Some("maintainer".into()),
        })),
        ..control(RunControl::Pause, "maintenance")
    }
}
fn resume() -> RunUpdate {
    RunUpdate {
        paused: Some(None),
        ..control(RunControl::Resume, "resumed")
    }
}
fn correct(state: RunState) -> RunUpdate {
    RunUpdate {
        to: Some(state),
        ..control(RunControl::Correct, "operator_fix")
    }
}

#[test]
fn hs2_wbredm_a_correction_may_take_any_edge_of_an_unfinished_run_and_none_of_a_finished_one() {
    let mut bench = Bench::new();
    for from in RunState::ALL {
        for next in RunState::ALL {
            assert_eq!(
                from.may_be_corrected_to(next),
                !from.is_terminal() && from != next,
                "{from:?} -> {next:?}"
            );
            for mode in MODES {
                let run = bench.run(from, mode);
                let result = bench.update(&run, correct(next));
                let label = format!("{from:?} -> {next:?} {mode:?}");
                if from.is_terminal() {
                    assert!(
                        matches!(result, Err(OrchestrationError::Run(RunError::NotActive))),
                        "{label}"
                    );
                    continue;
                }
                let corrected = result.unwrap();
                assert_eq!(corrected.state, next, "{label}");
                if from == next {
                    // Nothing to correct: no entry, no write.
                    assert_eq!(corrected, run, "{label}");
                    continue;
                }
                let entry = corrected.history.last().unwrap();
                assert_eq!(
                    (entry.from, entry.to, entry.control, entry.reason.as_deref()),
                    (
                        Some(from),
                        next,
                        Some(RunControl::Correct),
                        Some("operator_fix")
                    ),
                    "{label}"
                );
                assert_eq!(corrected.history.len(), run.history.len() + 1);
                // Without the correction the same reversed edge is still refused.
                if !from.may_become(next) {
                    let other = bench.run(from, mode);
                    assert!(matches!(
                        bench.update(
                            &other,
                            RunUpdate {
                                to: Some(next),
                                ..control(RunControl::Handoff, "handed_off")
                            }
                        ),
                        Err(OrchestrationError::Run(RunError::InvalidTransition))
                    ));
                    assert_eq!(
                        bench.registry.get(OWNER, &other.run_id).unwrap().history,
                        other.history
                    );
                }
            }
        }
    }
    assert_eq!(RunError::Paused.code(), "run_paused");
    let codes: Vec<_> = RunControl::ALL.iter().map(|c| c.code()).collect();
    assert_eq!(codes, ["pause", "resume", "handoff", "correct"]);
    for item in RunControl::ALL {
        assert_eq!(serde_json::to_value(item).unwrap(), item.code());
    }
}

#[test]
fn hs2_wbredm_repeated_and_reversed_control_transitions_are_each_kept() {
    let mut bench = Bench::new();
    let run = bench.run(RunState::Dispatched, DispatchMode::Direct);
    let base = run.history.len();
    // Pause, pause again, resume, resume again, then the same once more.
    bench.clock.0.set(NOW + 10);
    let paused = bench.update(&run, pause()).unwrap();
    assert_eq!(paused.paused.as_ref().unwrap().since_ms, NOW);
    assert_eq!(paused.state, RunState::Dispatched);
    assert_eq!(paused.lease_expires_at_ms, Some(LIVE));
    bench.clock.0.set(NOW + 20);
    let again = bench.update(&paused, pause()).unwrap();
    // A repeated pause is not a transition and does not restart the pause.
    assert_eq!(again, paused);
    bench.clock.0.set(NOW + 30);
    let resumed = bench.update(&again, resume()).unwrap();
    assert_eq!(resumed.paused, None);
    assert_eq!(bench.update(&resumed, resume()).unwrap(), resumed);
    bench.clock.0.set(NOW + 40);
    let paused = bench.update(&resumed, pause()).unwrap();
    bench.clock.0.set(NOW + 50);
    let resumed = bench.update(&paused, resume()).unwrap();
    let kept: Vec<_> = resumed.history[base..]
        .iter()
        .map(|entry| {
            (
                entry.at_ms,
                entry.from,
                entry.to,
                entry.control,
                entry.reason.as_deref(),
                entry.actor_id.as_deref(),
            )
        })
        .collect();
    let dispatched = RunState::Dispatched;
    let entry = |at, control, reason| {
        (
            at,
            Some(dispatched),
            dispatched,
            Some(control),
            Some(reason),
            Some("maintainer"),
        )
    };
    assert_eq!(
        kept,
        [
            entry(NOW + 10, RunControl::Pause, "maintenance"),
            entry(NOW + 30, RunControl::Resume, "resumed"),
            entry(NOW + 40, RunControl::Pause, "maintenance"),
            entry(NOW + 50, RunControl::Resume, "resumed"),
        ]
    );
    // A reversed correction and its reversal: dispatched, back to requested, forward.
    let back = bench
        .update(&resumed, correct(RunState::Requested))
        .unwrap();
    assert_eq!(back.lease_expires_at_ms, None);
    let forward = bench.update(&back, correct(RunState::Dispatched)).unwrap();
    let again = bench
        .update(&forward, correct(RunState::Requested))
        .unwrap();
    assert_eq!(
        again.history[base + 4..]
            .iter()
            .map(|entry| (entry.from.unwrap(), entry.to))
            .collect::<Vec<_>>(),
        [
            (RunState::Dispatched, RunState::Requested),
            (RunState::Requested, RunState::Dispatched),
            (RunState::Dispatched, RunState::Requested),
        ]
    );
    // A handoff that only changes the worker is its own entry; one that changes
    // nothing is not.
    let forward = bench.update(&again, correct(RunState::Dispatched)).unwrap();
    let handed = bench
        .update(
            &forward,
            RunUpdate {
                to: Some(RunState::Dispatched),
                worker_id: Some("worker-b".into()),
                ..control(RunControl::Handoff, "handed_off")
            },
        )
        .unwrap();
    assert_eq!(handed.worker_id.as_deref(), Some("worker-b"));
    let last = handed.history.last().unwrap();
    assert_eq!(
        (last.from, last.to, last.control),
        (
            Some(RunState::Dispatched),
            RunState::Dispatched,
            Some(RunControl::Handoff)
        )
    );
    let same = bench
        .update(
            &handed,
            RunUpdate {
                worker_id: Some("worker-b".into()),
                ..control(RunControl::Handoff, "handed_off")
            },
        )
        .unwrap();
    assert_eq!(same, handed);
    // A pause ends with the run, and a finished run cannot be paused or resumed.
    let paused = bench.update(&handed, pause()).unwrap();
    let done = bench
        .update(
            &paused,
            RunUpdate {
                to: Some(RunState::Completed),
                ..RunUpdate::default()
            },
        )
        .unwrap();
    assert_eq!(done.paused, None);
    for update in [pause(), resume(), correct(RunState::Dispatched)] {
        assert!(matches!(
            bench.update(&done, update),
            Err(OrchestrationError::Run(RunError::NotActive))
        ));
    }
    // The record survives a reload exactly, and a pre-HS2-WBREDM record still reads.
    let stored = bench.registry.get(OWNER, &done.run_id).unwrap();
    assert_eq!(stored, done);
    let mut old = serde_json::to_value(&done).unwrap();
    for entry in old["history"].as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("control");
    }
    let old: Run = serde_json::from_value(old).unwrap();
    assert!(old.history.iter().all(|entry| entry.control.is_none()));
    assert_eq!(old.reconciliation, None);
    // A reason that is not a code is refused and nothing is written.
    let other = bench.run(RunState::Queued, DispatchMode::Dispatcher);
    assert!(
        bench
            .update(&other, control(RunControl::Correct, "free text reason"))
            .is_ok()
    );
    assert!(matches!(
        bench.update(
            &other,
            RunUpdate {
                to: Some(RunState::Intervened),
                ..control(RunControl::Correct, "free text reason")
            }
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(bench.registry.get(OWNER, &other.run_id).unwrap(), other);
}

#[test]
fn hs2_wbredm_a_full_history_keeps_how_the_run_began_and_the_latest_control_entries() {
    let mut bench = Bench::new();
    let mut run = bench.run(RunState::Dispatched, DispatchMode::Direct);
    for round in 0..MAX_RUN_HISTORY as u64 {
        bench.clock.0.set(NOW + 1 + round * 2);
        run = bench.update(&run, pause()).unwrap();
        bench.clock.0.set(NOW + 2 + round * 2);
        run = bench.update(&run, resume()).unwrap();
    }
    assert_eq!(run.history.len(), MAX_RUN_HISTORY);
    assert_eq!(run.history[0].to, RunState::Requested);
    assert_eq!(run.history[0].from, None);
    let last = run.history.last().unwrap();
    assert_eq!(last.control, Some(RunControl::Resume));
    assert_eq!(last.at_ms, NOW + MAX_RUN_HISTORY as u64 * 2);
    assert!(
        run.history[1..]
            .windows(2)
            .all(|pair| pair[0].at_ms < pair[1].at_ms)
    );
}

#[test]
fn hs2_wbredm_an_unchanged_outcome_is_not_rewritten_and_a_finished_run_keeps_none() {
    let mut bench = Bench::new();
    let run = bench.run(RunState::Dispatched, DispatchMode::Direct);
    let found = |at_ms, observed: &Observations| RunUpdate {
        reconciliation: Some(RunReconciliation::new(at_ms, assess(&run, observed, NOW))),
        ..RunUpdate::default()
    };
    let healthy = target(facts("started", Claim::Ours));
    let dark = Observations::target_only(Observed::Unverifiable("target_unavailable".into()));
    bench.clock.0.set(NOW + 1);
    let first = bench.update(&run, found(NOW + 1, &healthy)).unwrap();
    let stored = first.reconciliation.clone().unwrap();
    assert_eq!(stored.at_ms, NOW + 1);
    assert_eq!(stored.disposition, Disposition::Continue);
    assert_eq!(stored.unverified(), ["ci_run", "deployment", "session"]);
    assert_eq!(first.history.len(), run.history.len());
    // The same outcome later: nothing is written at all.
    bench.clock.0.set(NOW + 2);
    let same = bench.update(&first, found(NOW + 2, &healthy)).unwrap();
    assert_eq!(same, first);
    assert_eq!(same.updated_at_ms, NOW + 1);
    // A different outcome replaces it; flipping back is again a new outcome.
    bench.clock.0.set(NOW + 3);
    let held = bench.update(&same, found(NOW + 3, &dark)).unwrap();
    let outcome = held.reconciliation.clone().unwrap();
    assert_eq!(outcome.at_ms, NOW + 3);
    assert_eq!(outcome.disposition, Disposition::HoldUnverified);
    assert_eq!(outcome.unverified().len(), ExternalSource::ALL.len());
    bench.clock.0.set(NOW + 4);
    let back = bench.update(&held, found(NOW + 4, &healthy)).unwrap();
    assert_eq!(back.reconciliation.as_ref().unwrap().at_ms, NOW + 4);
    assert!(stored.same_facts(back.reconciliation.as_ref().unwrap()));
    assert_eq!(bench.registry.get(OWNER, &back.run_id).unwrap(), back);
    // A finished run depends on nothing: its outcome is dropped and none is accepted.
    let done = bench
        .update(
            &back,
            RunUpdate {
                to: Some(RunState::Completed),
                ..RunUpdate::default()
            },
        )
        .unwrap();
    assert_eq!(done.reconciliation, None);
    assert!(matches!(
        bench.update(&done, found(NOW + 5, &healthy)),
        Err(OrchestrationError::Run(RunError::NotActive))
    ));
    // Findings are bounded and their reasons are codes.
    let other = bench.run(RunState::Queued, DispatchMode::Dispatcher);
    let finding = |reason: &str| SourceFinding {
        source: ExternalSource::Session,
        verification: Verification::Unverifiable,
        reason: Some(reason.into()),
    };
    for findings in [
        vec![finding("not a code")],
        vec![finding("ok"); MAX_FINDINGS + 1],
    ] {
        assert!(matches!(
            bench.update(
                &other,
                RunUpdate {
                    reconciliation: Some(RunReconciliation {
                        at_ms: NOW,
                        disposition: Disposition::HoldUnverified,
                        findings,
                    }),
                    ..RunUpdate::default()
                }
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    assert_eq!(bench.registry.get(OWNER, &other.run_id).unwrap(), other);
}

#[test]
fn hs2_wbredm_unfinished_owners_empty_then_refilled() {
    let home = tempfile::tempdir().unwrap();
    let registry = RunRegistry::new(home.path());
    let clock = Clock(NOW.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    // No file, no owners, nothing created by asking.
    assert!(registry.unfinished_owners().unwrap().is_empty());
    assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());
    let mut begin = |owner: &str, n: u32| {
        registry
            .begin(
                owner,
                NewRun {
                    operation_id: format!("op-{n}"),
                    ticket: RunTicket {
                        connection_id: "owner-source".into(),
                        native_id: id(500),
                    },
                    target: RunTarget {
                        target_id: id(1),
                        connection_id: "target-source".into(),
                        native_id: format!("t{n}"),
                    },
                    mode: DispatchMode::Dispatcher,
                    worker_id: None,
                },
                None,
                &mut tx,
            )
            .unwrap()
            .0
    };
    let first = begin("owner-b", 1);
    let second = begin("owner-a", 2);
    let third = begin("owner-a", 3);
    assert_eq!(
        registry.unfinished_owners().unwrap(),
        ["owner-a", "owner-b"]
    );
    let finish = |owner: &str, run: &Run, to: RunState| {
        let mut rng = Rng(50);
        registry
            .update(
                owner,
                &run.run_id,
                RunUpdate {
                    to: Some(to),
                    ..RunUpdate::default()
                },
                &mut Transaction {
                    clock: &clock,
                    rng: &mut rng,
                },
            )
            .unwrap()
    };
    // An owner stays while any of its runs is unfinished, taken-over ones included.
    finish("owner-a", &second, RunState::Cancelled);
    finish("owner-a", &third, RunState::Queued);
    finish("owner-a", &third, RunState::Intervened);
    assert_eq!(
        registry.unfinished_owners().unwrap(),
        ["owner-a", "owner-b"]
    );
    finish("owner-a", &third, RunState::Completed);
    assert_eq!(registry.unfinished_owners().unwrap(), ["owner-b"]);
    finish("owner-b", &first, RunState::Failed);
    assert!(registry.unfinished_owners().unwrap().is_empty());
    // Refilled after being empty.
    let mut rng = Rng(90);
    let (again, _) = registry
        .begin(
            "owner-b",
            NewRun {
                operation_id: "op-9".into(),
                ticket: RunTicket {
                    connection_id: "owner-source".into(),
                    native_id: id(500),
                },
                target: RunTarget {
                    target_id: id(1),
                    connection_id: "target-source".into(),
                    native_id: "t1".into(),
                },
                mode: DispatchMode::Dispatcher,
                worker_id: None,
            },
            None,
            &mut Transaction {
                clock: &clock,
                rng: &mut rng,
            },
        )
        .unwrap();
    assert!(again.state.is_active());
    assert_eq!(registry.unfinished_owners().unwrap(), ["owner-b"]);
}
