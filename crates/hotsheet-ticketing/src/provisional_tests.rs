//! HS2-2SJPCH: the provisional readiness rule, as a table over prerequisite state and
//! CI result, plus opt-out, depth and policy storage.

use std::cell::Cell;
use std::collections::BTreeMap;

use hotsheet_model::{Note, NoteKind, StartedPhase, Status, Ticket, Timestamp, Ulid};

use super::*;

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccc";

#[derive(Default)]
struct Probe {
    reachable: BTreeMap<String, bool>,
    gates: BTreeMap<String, GateObservation>,
    gate_reads: Cell<u32>,
}

impl Probe {
    fn pushed(mut self, commit: &str, result: GateResult) -> Self {
        self.reachable.insert(commit.into(), true);
        self.gates.insert(
            commit.into(),
            GateObservation::with_run(result, format!("run-{}", &commit[..1])),
        );
        self
    }
}

impl GateProbe for Probe {
    fn reachable(&self, commit: &str) -> Option<bool> {
        self.reachable.get(commit).copied()
    }

    fn gate(&self, commit: &str) -> GateObservation {
        self.gate_reads.set(self.gate_reads.get() + 1);
        self.gates.get(commit).cloned().unwrap_or_default()
    }
}

fn note(text: &str) -> Note {
    Note {
        id: Ulid::new(),
        kind: NoteKind::Regular,
        created_at: Timestamp::new("2026-11-12T00:00:00Z"),
        edited_at: Timestamp::new("2026-11-12T00:00:00Z"),
        summary: None,
        confidence: None,
        feedback_for: None,
        kind_explicit: false,
        feedback_request: false,
        response_to: None,
        ai_feedback: None,
        human_edited: false,
        actor: None,
        text: text.to_owned(),
    }
}

fn ticket(slug: &str, status: Status, phase: Option<StartedPhase>) -> Ticket {
    Ticket {
        id: Ulid::new(),
        slug: slug.into(),
        status,
        started_phase: phase,
        ..Ticket::default()
    }
}

fn awaiting_ci(slug: &str, commit: &str) -> Ticket {
    let mut ticket = ticket(slug, Status::Started, Some(StartedPhase::FinalTesting));
    ticket
        .notes
        .push(note(&format!("Pushed.\n\nCI gate commit: {commit}\n")));
    ticket
}

fn dependent_on(blockers: &[&Ticket]) -> Ticket {
    let mut ticket = ticket("HS2-DEPEND", Status::NotStarted, None);
    ticket.blocked_by = blockers.iter().map(|blocker| blocker.id).collect();
    ticket
}

fn enabled() -> SpeculationPolicy {
    SpeculationPolicy {
        enabled: true,
        ..SpeculationPolicy::default()
    }
}

fn blocked(reason: BlockReason) -> BlockerReadiness {
    BlockerReadiness::Blocked { reason }
}

#[derive(Clone, Copy, Debug)]
enum BlockerState {
    Started,
    FinalTestingPushed,
    FinalTestingUnrecorded,
    FinalTestingUnpushed,
    Completed,
}

fn blocker_in(state: BlockerState) -> (Ticket, bool) {
    match state {
        BlockerState::Started => (
            ticket("HS2-BLOCK", Status::Started, Some(StartedPhase::Working)),
            false,
        ),
        BlockerState::FinalTestingPushed => (awaiting_ci("HS2-BLOCK", SHA_A), true),
        BlockerState::FinalTestingUnrecorded => (
            ticket(
                "HS2-BLOCK",
                Status::Started,
                Some(StartedPhase::FinalTesting),
            ),
            false,
        ),
        BlockerState::FinalTestingUnpushed => (awaiting_ci("HS2-BLOCK", SHA_A), false),
        BlockerState::Completed => (
            ticket(
                "HS2-BLOCK",
                Status::Completed,
                Some(StartedPhase::FinalTesting),
            ),
            true,
        ),
    }
}

#[test]
fn hs2_2sjpch_matrix_of_blocker_state_and_ci_result() {
    use BlockReason as R;
    use BlockerState as S;
    use GateResult as G;
    let closed = BlockerReadiness::Confirmed {
        by: Confirmation::Closed,
    };
    for state in [
        S::Started,
        S::FinalTestingPushed,
        S::FinalTestingUnrecorded,
        S::FinalTestingUnpushed,
        S::Completed,
    ] {
        for result in G::ALL {
            let (blocker, pushed) = blocker_in(state);
            let dependent = dependent_on(&[&blocker]);
            let mut probe = Probe::default();
            probe.reachable.insert(SHA_A.into(), pushed);
            probe
                .gates
                .insert(SHA_A.into(), GateObservation::with_run(result, "77.1"));
            let tickets = [blocker.clone(), dependent.clone()];
            let expected = match (state, result) {
                (S::Completed, _) => closed.clone(),
                (S::Started, _) => blocked(R::NotAwaitingCi),
                (S::FinalTestingUnrecorded, _) => blocked(R::NoGateCommit),
                (S::FinalTestingUnpushed, _) => blocked(R::NotPushed),
                (S::FinalTestingPushed, G::Pending | G::Unknown) => {
                    BlockerReadiness::Provisional(ProvisionalBase {
                        blocker: blocker.id,
                        slug: "HS2-BLOCK".into(),
                        base_commit: SHA_A.into(),
                        gate: GateObservation::with_run(result, "77.1"),
                        depth: 1,
                    })
                }
                (S::FinalTestingPushed, G::Green) => BlockerReadiness::Confirmed {
                    by: Confirmation::GateGreen,
                },
                (S::FinalTestingPushed, G::Red) => blocked(R::GateRed),
                (S::FinalTestingPushed, G::Superseded) => blocked(R::GateSuperseded),
                (S::FinalTestingPushed, G::Cancelled) => blocked(R::GateCancelled),
            };
            assert_eq!(
                blocker_readiness(&enabled(), &dependent, &blocker.id, &tickets, &probe),
                expected,
                "{state:?} x {result:?}"
            );
            // The default policy: only a closed prerequisite satisfies, whatever CI says,
            // and the probe is never consulted.
            let quiet = Probe::default();
            let default = blocker_readiness(
                &SpeculationPolicy::default(),
                &dependent,
                &blocker.id,
                &tickets,
                &quiet,
            );
            assert_eq!(
                default,
                if matches!(state, S::Completed) {
                    closed.clone()
                } else {
                    blocked(R::PolicyDisabled)
                },
                "default {state:?} x {result:?}"
            );
            assert_eq!(quiet.gate_reads.get(), 0);
        }
    }
}

#[test]
fn hs2_2sjpch_every_closed_status_confirms_and_unknown_never_does() {
    for status in [
        Status::Completed,
        Status::Verified,
        Status::Archive,
        Status::Deleted,
        Status::Moved,
    ] {
        let blocker = ticket("HS2-BLOCK", status, None);
        let dependent = dependent_on(&[&blocker]);
        let readiness = ticket_readiness(
            &SpeculationPolicy::default(),
            &dependent,
            &[blocker],
            &Probe::default(),
        );
        assert_eq!(readiness.level, ReadinessLevel::Confirmed, "{status:?}");
        assert_eq!(readiness.explain(), "confirmed");
    }
    let missing = ticket("HS2-GONE", Status::Completed, None);
    let dependent = dependent_on(&[&missing]);
    let readiness = ticket_readiness(&enabled(), &dependent, &[], &Probe::default());
    assert_eq!(readiness.level, ReadinessLevel::Blocked);
    assert_eq!(
        readiness.prerequisites[0].readiness,
        blocked(BlockReason::UnknownPrerequisite)
    );
    assert!(readiness.explain().contains("prerequisite_unknown"));
    // No prerequisites at all is confirmed under either policy.
    let free = ticket("HS2-FREE", Status::NotStarted, None);
    assert_eq!(
        ticket_readiness(&enabled(), &free, &[], &Probe::default()).level,
        ReadinessLevel::Confirmed
    );
}

#[test]
fn hs2_2sjpch_reachability_unknown_is_never_pushed() {
    let blocker = awaiting_ci("HS2-BLOCK", SHA_A);
    let dependent = dependent_on(&[&blocker]);
    let tickets = [blocker.clone()];
    assert_eq!(
        blocker_readiness(
            &enabled(),
            &dependent,
            &blocker.id,
            &tickets,
            &Probe::default()
        ),
        blocked(BlockReason::ReachabilityUnknown)
    );
}

#[test]
fn hs2_2sjpch_opt_out_tag_on_either_ticket_refuses_and_can_be_withdrawn() {
    let probe = Probe::default().pushed(SHA_A, GateResult::Pending);
    let blocker = awaiting_ci("HS2-BLOCK", SHA_A);
    let mut dependent = dependent_on(&[&blocker]);
    let ready = |dependent: &Ticket, blocker: &Ticket| {
        ticket_readiness(&enabled(), dependent, std::slice::from_ref(blocker), &probe).level
    };
    assert_eq!(ready(&dependent, &blocker), ReadinessLevel::Provisional);
    dependent.tags.push("Speculate:Never ".into());
    assert!(opted_out(&dependent));
    assert_eq!(ready(&dependent, &blocker), ReadinessLevel::Blocked);
    dependent.tags.clear();
    assert_eq!(ready(&dependent, &blocker), ReadinessLevel::Provisional);
    let mut risky = blocker.clone();
    risky.tags.push(OPT_OUT_TAG.into());
    assert_eq!(ready(&dependent, &risky), ReadinessLevel::Blocked);
    // An opted-out dependent still starts on a closed prerequisite.
    dependent.tags.push(OPT_OUT_TAG.into());
    let mut done = blocker.clone();
    done.status = Status::Completed;
    assert_eq!(ready(&dependent, &done), ReadinessLevel::Confirmed);
}

#[test]
fn hs2_2sjpch_depth_counts_stacked_unconfirmed_gates() {
    // A waits on CI; B was built on A and waits on CI; D depends on B.
    let a = awaiting_ci("HS2-A", SHA_A);
    let mut b = awaiting_ci("HS2-B", SHA_B);
    b.blocked_by = vec![a.id];
    let d = dependent_on(&[&b]);
    let tickets = [a.clone(), b.clone()];
    let probe = Probe::default()
        .pushed(SHA_A, GateResult::Pending)
        .pushed(SHA_B, GateResult::Pending);
    assert_eq!(
        blocker_readiness(&enabled(), &d, &b.id, &tickets, &probe),
        blocked(BlockReason::DepthExceeded),
        "default depth is one"
    );
    let two = SpeculationPolicy {
        max_depth: 2,
        ..enabled()
    };
    let readiness = ticket_readiness(&two, &d, &tickets, &probe);
    assert_eq!(readiness.level, ReadinessLevel::Provisional);
    assert_eq!(readiness.depth, 2);
    assert_eq!(readiness.explain(), "provisional on HS2-B CI run-b");
    // Once A's gate is green only B's gate is unconfirmed.
    let settled = Probe::default()
        .pushed(SHA_A, GateResult::Green)
        .pushed(SHA_B, GateResult::Pending);
    let readiness = ticket_readiness(&enabled(), &d, &tickets, &settled);
    assert_eq!(
        (readiness.level, readiness.depth),
        (ReadinessLevel::Provisional, 1)
    );
    // A closed A likewise.
    let mut closed_a = a.clone();
    closed_a.status = Status::Completed;
    assert_eq!(
        ticket_readiness(&enabled(), &d, &[closed_a, b.clone()], &probe).depth,
        1
    );
    // B built on an A that is still being worked: two unconfirmed prerequisites stack.
    let mut working_a = a.clone();
    working_a.started_phase = Some(StartedPhase::Working);
    assert_eq!(
        blocker_readiness(&enabled(), &d, &b.id, &[working_a, b.clone()], &probe),
        blocked(BlockReason::DepthExceeded)
    );
}

#[test]
fn hs2_2sjpch_dependency_cycle_is_never_provisional() {
    let mut a = awaiting_ci("HS2-A", SHA_A);
    let mut b = awaiting_ci("HS2-B", SHA_B);
    a.blocked_by = vec![b.id];
    b.blocked_by = vec![a.id];
    let d = dependent_on(&[&b]);
    let probe = Probe::default()
        .pushed(SHA_A, GateResult::Pending)
        .pushed(SHA_B, GateResult::Pending);
    let deepest = SpeculationPolicy {
        max_depth: MAX_DEPTH_LIMIT,
        ..enabled()
    };
    assert_eq!(
        blocker_readiness(&deepest, &d, &b.id, &[a, b.clone()], &probe),
        blocked(BlockReason::DepthExceeded)
    );
}

#[test]
fn hs2_2sjpch_several_prerequisites_combine_to_the_weakest() {
    let pending = awaiting_ci("HS2-A", SHA_A);
    let red = awaiting_ci("HS2-B", SHA_B);
    let green = awaiting_ci("HS2-C", SHA_C);
    let done = ticket("HS2-DONE", Status::Verified, None);
    let probe = Probe::default()
        .pushed(SHA_A, GateResult::Pending)
        .pushed(SHA_B, GateResult::Red)
        .pushed(SHA_C, GateResult::Green);
    let all = [pending.clone(), red.clone(), green.clone(), done.clone()];
    let level = |blockers: &[&Ticket]| {
        ticket_readiness(&enabled(), &dependent_on(blockers), &all, &probe).level
    };
    assert_eq!(level(&[&done, &green]), ReadinessLevel::Confirmed);
    assert_eq!(
        level(&[&done, &green, &pending]),
        ReadinessLevel::Provisional
    );
    assert_eq!(level(&[&pending, &red]), ReadinessLevel::Blocked);
    let both = ticket_readiness(&enabled(), &dependent_on(&[&pending, &red]), &all, &probe);
    assert_eq!(both.depth, 0);
    assert_eq!(both.explain(), "blocked by HS2-B (gate_red)");
    assert_eq!(both.provisional_bases().len(), 1);
}

#[test]
fn hs2_2sjpch_gate_commit_is_the_newest_exact_marker_line() {
    let mut ticket = ticket("HS2-X", Status::Started, Some(StartedPhase::FinalTesting));
    assert_eq!(ci_gate_commit(&ticket), None);
    ticket.notes.push(note(&format!("CI gate commit: {SHA_A}")));
    ticket
        .notes
        .push(note("ci gate commit: not-a-sha\n> CI gate commit: quoted"));
    assert_eq!(ci_gate_commit(&ticket).as_deref(), Some(SHA_A));
    ticket.notes.push(note(&format!(
        "## Repair\n\nCI gate commit:   `{}`  \ntrailing",
        SHA_B.to_ascii_uppercase()
    )));
    assert_eq!(ci_gate_commit(&ticket).as_deref(), Some(SHA_B));
    // A short SHA, or text after the SHA, is not a gate commit.
    let mut short = ticket.clone();
    short.notes = vec![
        note("CI gate commit: abc1234"),
        note(&format!("CI gate commit: {SHA_C} and more")),
        note("CI gate commit: 日本語"),
    ];
    assert_eq!(ci_gate_commit(&short), None);
}

#[test]
fn hs2_2sjpch_explain_names_the_run_or_the_gate_state() {
    assert_eq!(
        explain_base(
            "HS2-A",
            &GateObservation::with_run(GateResult::Pending, "9.2")
        ),
        "provisional on HS2-A CI 9.2"
    );
    assert_eq!(
        explain_base("HS2-A", &GateObservation::of(GateResult::Unknown)),
        "provisional on HS2-A CI unknown"
    );
}

#[test]
fn hs2_2sjpch_policy_defaults_validates_and_round_trips_shared_settings_only() {
    let default = SpeculationPolicy::default();
    assert!(!default.enabled);
    assert_eq!(default.max_depth, 1);
    assert_eq!(default.integration, IntegrationRule::PushPerTicket);
    assert_eq!(
        serde_json::from_str::<SpeculationPolicy>("{}").unwrap(),
        default
    );
    assert!(serde_json::from_str::<SpeculationPolicy>(r#"{"depth":2}"#).is_err());
    for depth in [0, MAX_DEPTH_LIMIT + 1] {
        assert_eq!(
            SpeculationPolicy {
                max_depth: depth,
                ..enabled()
            }
            .validate(),
            Err(SpeculationPolicyError::Depth)
        );
    }
    for branch in ["", "-x", "a..b", "a b", "/a", "a/", "a//b", "x;y", ".a"] {
        assert_eq!(
            SpeculationPolicy {
                base_branch: Some(branch.into()),
                ..enabled()
            }
            .validate(),
            Err(SpeculationPolicyError::Branch),
            "{branch}"
        );
    }
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::for_project(root.path());
    assert_eq!(SpeculationPolicy::read(&settings).unwrap(), default);
    let policy = SpeculationPolicy {
        enabled: true,
        max_depth: 2,
        integration: IntegrationRule::Parked,
        base_branch: Some("release/next".into()),
    };
    policy.write(&settings).unwrap();
    assert_eq!(SpeculationPolicy::read(&settings).unwrap(), policy);
    assert_eq!(
        settings
            .get(SPECULATION_POLICY_SETTING, Scope::Shared)
            .unwrap()
            .unwrap(),
        serde_json::json!({
            "enabled": true, "max_depth": 2, "integration": "parked",
            "base_branch": "release/next"
        })
    );
    // A machine-local value is never the project policy.
    settings
        .set(
            SPECULATION_POLICY_SETTING,
            serde_json::json!({"enabled": false}),
            Scope::Local,
        )
        .unwrap();
    assert_eq!(SpeculationPolicy::read(&settings).unwrap(), policy);
    settings
        .set(
            SPECULATION_POLICY_SETTING,
            serde_json::json!({"enabled": true, "max_depth": 0}),
            Scope::Shared,
        )
        .unwrap();
    assert_eq!(
        SpeculationPolicy::read(&settings),
        Err(SpeculationPolicyError::Depth)
    );
    assert!(
        SpeculationPolicy {
            max_depth: 0,
            ..enabled()
        }
        .write(&settings)
        .is_err()
    );
}
