//! HS2-J89JC7 timeline: phases, kick-off, the item state matrix, wave placement in
//! running and proposed plans, estimates, and repeated, out-of-order, interleaved and
//! empty-then-refill inputs. Synthetic ids only.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::*;

const OWNER: &str = "owner-0123456789ab";
const SOURCE: &str = "source-a";
const MINUTE: u64 = 60_000;
const NOW: u64 = 1_000 * MINUTE;

fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}

fn target(n: u128) -> RunTarget {
    RunTarget {
        target_id: if n % 2 == 0 {
            "api".into()
        } else {
            "web".into()
        },
        connection_id: SOURCE.into(),
        native_id: id(n),
    }
}

fn prerequisite(n: u128) -> Prerequisite {
    let target = target(n);
    Prerequisite::Ticket {
        target_id: target.target_id,
        connection_id: target.connection_id,
        native_id: target.native_id,
        gates: GateRequirement::Ci,
        commit: None,
    }
}

/// Step `n` works ticket `n` and waits on the tickets of `after`.
fn step(n: u128, level: ReadinessLevel, after: &[u128]) -> TimelineStep {
    TimelineStep {
        readiness: StepReadiness {
            step: PlanStep {
                requirements: Default::default(),
                model: None,
                step_id: format!("step-{n}"),
                owner_checkout_id: OWNER.into(),
                ticket: RunTicket {
                    connection_id: "owner-source".into(),
                    native_id: id(900 + n % 2),
                },
                target: target(n),
                prerequisites: after.iter().copied().map(prerequisite).collect(),
                resources: Vec::new(),
                tool: None,
                revision: 1,
                created_at_ms: 1,
                updated_at_ms: 1,
                updated_by: None,
            },
            level,
            reasons: after
                .iter()
                .map(|n| ReadinessReason {
                    code: "prerequisite_not_finished".into(),
                    prerequisite: Some(prerequisite(*n)),
                    commits: Vec::new(),
                })
                .collect(),
            base_commits: Vec::new(),
            unblocks: Vec::new(),
            cycle: Vec::new(),
        },
        provisional_work: false,
        provisional: None,
    }
}

fn run(n: u128, ticket: u128, state: RunState, created: u64, finished: Option<u64>) -> Run {
    let mut history = vec![RunTransition {
        at_ms: created,
        from: None,
        to: RunState::Requested,
        reason: None,
        actor_id: None,
        control: None,
    }];
    if state != RunState::Requested {
        history.push(RunTransition {
            at_ms: finished.unwrap_or(created + 1),
            from: Some(RunState::Requested),
            to: state,
            reason: None,
            actor_id: None,
            control: None,
        });
    }
    Run {
        run_id: id(500 + n),
        owner_checkout_id: OWNER.into(),
        operation_id: format!("op-{n}"),
        ticket: RunTicket {
            connection_id: "owner-source".into(),
            native_id: id(900),
        },
        target: target(ticket),
        mode: DispatchMode::Dispatcher,
        worker_id: Some("worker-a".into()),
        state,
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

fn session(n: u128, run: u128, tool: &str) -> OrchestratedSession {
    serde_json::from_value(json!({
        "session_id": id(700 + n),
        "owner_checkout_id": OWNER,
        "operation_id": format!("session-{n}"),
        "target_id": "api",
        "tool": tool,
        "run_id": id(500 + run),
        "state": "running",
        "created_at_ms": 5,
        "updated_at_ms": 5,
        "history": [],
    }))
    .unwrap()
}

fn targets() -> Vec<TimelineTarget> {
    vec![
        TimelineTarget {
            target_id: "api".into(),
            label: "API".into(),
            location: TargetLocation::Remote,
            machine_label: Some("Build host".into()),
            checkout_id: Some("api-0123456789ab".into()),
        },
        TimelineTarget {
            target_id: "web".into(),
            label: "Web".into(),
            location: TargetLocation::Local,
            machine_label: None,
            checkout_id: Some("web-0123456789ab".into()),
        },
    ]
}

#[derive(Default)]
struct Fixture {
    declared: bool,
    targets: Vec<TimelineTarget>,
    steps: Vec<TimelineStep>,
    runs: Vec<Run>,
    step_runs: BTreeMap<String, BTreeSet<String>>,
    /// HS2-CQ0PNT
    sites: ProjectSites,
    sessions: Vec<OrchestratedSession>,
    hints: BTreeMap<String, PlacementHint>,
    permissions: BTreeMap<String, PermissionWait>,
    decisions: BTreeMap<(String, String), DecisionWait>,
    tickets: BTreeMap<TicketKey, TicketLabel>,
    policy: TimelinePolicy,
    now_ms: u64,
}

impl Fixture {
    fn new(steps: Vec<TimelineStep>) -> Self {
        Self {
            declared: true,
            targets: targets(),
            steps,
            policy: TimelinePolicy {
                present: true,
                revision: Some("rev-1".into()),
                automatic: true,
                auto: false,
                software: false,
            },
            now_ms: NOW,
            ..Self::default()
        }
    }

    fn proposed(mut self) -> Self {
        self.policy.automatic = false;
        self
    }

    fn view(&self) -> Timeline {
        let goals = BTreeMap::from([(
            ("owner-source".to_owned(), id(900)),
            TicketLabel {
                slug: Some("LCH-1".into()),
                title: Some("Ship single sign-on".into()),
                ..TicketLabel::default()
            },
        )]);
        timeline(&TimelineInput {
            owner_checkout_id: OWNER,
            now_ms: self.now_ms,
            declared: self.declared,
            targets: &self.targets,
            steps: &self.steps,
            runs: &self.runs,
            step_runs: &self.step_runs,
            sites: &self.sites,
            sessions: &self.sessions,
            hints: &self.hints,
            permissions: &self.permissions,
            decisions: &self.decisions,
            goals: &goals,
            tickets: &self.tickets,
            policy: &self.policy,
        })
    }
}

/// Wave kinds with the step numbers in each, oldest first.
fn shape(view: &Timeline) -> Vec<(WaveKind, Vec<String>)> {
    view.waves
        .iter()
        .map(|wave| {
            (
                wave.kind,
                wave.items.iter().map(|item| item.step_id.clone()).collect(),
            )
        })
        .collect()
}

fn ids(steps: &[u128]) -> Vec<String> {
    steps.iter().map(|n| format!("step-{n}")).collect()
}

fn permission() -> PermissionWait {
    PermissionWait {
        kind: "approval".into(),
        request_id: id(42),
        action: "session_start".into(),
        operation_id: Some("sched-start-step-2".into()),
        requested_at_ms: Some(3),
    }
}

use ReadinessLevel as L;

#[test]
fn hs2_j89jc7_phase_follows_declaration_plan_policy_and_runs() {
    let one = || vec![step(2, L::Confirmed, &[])];
    let cases: Vec<(&str, Fixture, TimelinePhase)> = vec![
        (
            "undeclared",
            Fixture {
                declared: false,
                ..Fixture::new(one())
            },
            TimelinePhase::NoProjects,
        ),
        (
            "no targets",
            Fixture {
                targets: Vec::new(),
                ..Fixture::new(one())
            },
            TimelinePhase::NoProjects,
        ),
        ("no plan", Fixture::new(Vec::new()), TimelinePhase::NoPlan),
        (
            "proposed",
            Fixture::new(one()).proposed(),
            TimelinePhase::Proposed,
        ),
        ("running", Fixture::new(one()), TimelinePhase::Running),
        (
            "paused",
            Fixture {
                runs: vec![run(1, 2, RunState::Cancelled, 10, Some(20))],
                ..Fixture::new(one()).proposed()
            },
            TimelinePhase::Paused,
        ),
    ];
    for (name, fixture, phase) in cases {
        let view = fixture.view();
        assert_eq!(view.phase, phase, "{name}");
        if matches!(phase, TimelinePhase::NoProjects | TimelinePhase::NoPlan) {
            assert!(view.waves.is_empty(), "{name}");
        }
    }
}

#[test]
fn hs2_j89jc7_kick_off_says_why_start_is_refused() {
    let one = || vec![step(2, L::Confirmed, &[])];
    let mut absent = Fixture::new(one()).proposed();
    absent.policy.present = false;
    // HS2-XYAHB4: no placement actor is needed; HS2-J3ZF27: a Software project
    // cannot start, and its kept automatic policy reads as not running.
    let mut software = Fixture::new(one()).proposed();
    software.policy.software = true;
    let cases = [
        (
            "no projects",
            Fixture::new(one()).proposed().tap(|f| f.targets.clear()),
            Some("no_projects"),
            false,
        ),
        (
            "no plan",
            Fixture::new(Vec::new()).proposed(),
            Some("no_plan"),
            false,
        ),
        ("policy absent", absent, Some("policy_absent"), false),
        ("software", software, Some("project_software"), false),
        ("running", Fixture::new(one()), Some("running"), true),
        ("ready", Fixture::new(one()).proposed(), None, false),
    ];
    for (name, fixture, blocked, pause) in cases {
        let kick_off = fixture.view().kick_off;
        assert_eq!(kick_off.start_blocked.as_deref(), blocked, "{name}");
        assert_eq!(kick_off.can_start, blocked.is_none(), "{name}");
        assert_eq!(kick_off.can_pause, pause, "{name}");
        assert_eq!(kick_off.policy_revision.as_deref(), Some("rev-1"), "{name}");
    }
}

trait Tap: Sized {
    fn tap(self, change: impl FnOnce(&mut Self)) -> Self;
}
impl Tap for Fixture {
    fn tap(mut self, change: impl FnOnce(&mut Self)) -> Self {
        change(&mut self);
        self
    }
}

#[test]
fn hs2_j89jc7_item_state_matrix_over_level_permission_and_decision() {
    use ItemState as S;
    // (level, provisional work, permission, decision) -> state
    let table: &[(ReadinessLevel, bool, bool, bool, ItemState)] = &[
        (L::Finished, false, true, true, S::Done),
        (L::InProgress, false, true, true, S::Running),
        (L::InProgress, true, false, false, S::Provisional),
        (L::Confirmed, false, false, false, S::Ready),
        (L::Confirmed, false, true, false, S::WaitsForPermission),
        (L::Confirmed, false, false, true, S::Ready),
        (L::Provisional, false, false, false, S::WaitsForPrerequisite),
        (L::Provisional, false, false, true, S::WaitsForDecision),
        (L::Provisional, false, true, true, S::WaitsForPermission),
        (L::Stale, false, false, false, S::WaitsForPrerequisite),
        (L::Stale, false, false, true, S::WaitsForDecision),
        (L::Blocked, false, false, false, S::WaitsForPrerequisite),
        (L::Blocked, false, false, true, S::WaitsForPrerequisite),
        (L::Cyclic, false, false, true, S::WaitsForPrerequisite),
    ];
    for (level, provisional, permission_wait, decision, expected) in table.iter().copied() {
        let mut planned = step(2, level, &[]);
        planned.provisional_work = provisional;
        let mut fixture = Fixture::new(vec![planned]);
        if level == L::InProgress {
            fixture.runs.push(run(1, 2, RunState::Dispatched, 10, None));
        }
        if permission_wait {
            fixture.permissions.insert("step-2".into(), permission());
        }
        if decision {
            fixture.decisions.insert(
                ("owner-source".into(), id(900)),
                DecisionWait {
                    note_id: Some(id(77)),
                },
            );
        }
        let view = fixture.view();
        let item = view
            .waves
            .iter()
            .flat_map(|wave| &wave.items)
            .next()
            .unwrap();
        let label = format!(
            "{level:?} provisional={provisional} permission={permission_wait} decision={decision}"
        );
        assert_eq!(item.state, expected, "{label}");
        // What it waits on is shown exactly when that is its state.
        assert_eq!(
            item.permission.is_some(),
            expected == S::WaitsForPermission,
            "{label}"
        );
        assert_eq!(
            item.decision.is_some(),
            expected == S::WaitsForDecision,
            "{label}"
        );
        assert_eq!(view.counts.get(expected.code()), Some(&1), "{label}");
        assert_eq!(item.goal_slug.as_deref(), Some("LCH-1"), "{label}");
    }
}

/// The chain 4 -> 6 -> 8 -> 10 waits on 2, which runs; 3 is ready and 12 is finished.
fn chain() -> Vec<TimelineStep> {
    vec![
        step(2, L::InProgress, &[]),
        step(4, L::Blocked, &[2]),
        step(6, L::Blocked, &[4]),
        step(8, L::Blocked, &[6]),
        step(10, L::Blocked, &[8]),
        step(3, L::Confirmed, &[]),
        step(12, L::Finished, &[]),
    ]
}

#[test]
fn hs2_j89jc7_running_waves_put_done_oldest_and_later_newest() {
    let mut fixture = Fixture::new(chain());
    fixture.runs = vec![
        run(1, 2, RunState::Dispatched, NOW - 30 * MINUTE, None),
        run(
            2,
            12,
            RunState::Completed,
            NOW - 300 * MINUTE,
            Some(NOW - 200 * MINUTE),
        ),
    ];
    let view = fixture.view();
    assert_eq!(
        shape(&view),
        vec![
            (WaveKind::Done, ids(&[12])),
            (WaveKind::WorkingNow, ids(&[2])),
            // Ordered by declared target (api before web), then step id.
            (WaveKind::Next, ids(&[4, 3])),
            (WaveKind::Then, ids(&[6])),
            (WaveKind::Later, ids(&[10, 8])),
        ]
    );
    // One measured run of 100 minutes: Next when the running work should end.
    assert_eq!(
        (view.estimate, view.wave_minutes),
        (EstimateBasis::Measured, 100)
    );
    let starts: Vec<Option<u64>> = view
        .waves
        .iter()
        .map(|wave| wave.estimated_start_ms)
        .collect();
    assert_eq!(
        starts,
        vec![
            None,
            Some(NOW - 30 * MINUTE),
            Some(NOW + 70 * MINUTE),
            Some(NOW + 170 * MINUTE),
            Some(NOW + 270 * MINUTE),
        ]
    );
    let working = &view.waves[1].items[0];
    assert_eq!(working.estimated_start_ms, None);
    assert_eq!(working.started_at_ms, Some(NOW - 30 * MINUTE));
    let done = &view.waves[0].items[0];
    assert_eq!(done.finished_at_ms, Some(NOW - 200 * MINUTE));
    assert_eq!(view.running_since_ms, Some(NOW - 30 * MINUTE));
    // What waits names the step it waits on.
    let next = &view.waves[2].items[0];
    assert_eq!(next.waits_on.len(), 1);
    assert_eq!(next.waits_on[0].step_id.as_deref(), Some("step-2"));
}

#[test]
fn hs2_j89jc7_empty_waves_still_show_and_default_estimates() {
    let view = Fixture::new(vec![step(2, L::Confirmed, &[])]).view();
    assert_eq!(
        shape(&view),
        vec![
            (WaveKind::Done, vec![]),
            (WaveKind::WorkingNow, vec![]),
            (WaveKind::Next, ids(&[2])),
            (WaveKind::Then, vec![]),
            (WaveKind::Later, vec![]),
        ]
    );
    assert_eq!(
        (view.estimate, view.wave_minutes),
        (EstimateBasis::Default, DEFAULT_WAVE_MINUTES)
    );
    assert_eq!(view.waves[1].estimated_start_ms, None);
    assert_eq!(view.waves[2].estimated_start_ms, Some(NOW));
    assert_eq!(view.waves[2].items[0].estimated_start_ms, Some(NOW));
}

#[test]
fn hs2_j89jc7_proposed_plan_numbers_its_waves_after_done() {
    let mut steps = chain();
    steps[0] = step(2, L::Confirmed, &[]);
    let view = Fixture::new(steps).proposed().view();
    assert_eq!(view.phase, TimelinePhase::Proposed);
    let kinds: Vec<(WaveKind, u32, Vec<String>)> = view
        .waves
        .iter()
        .map(|wave| {
            (
                wave.kind,
                wave.ordinal,
                wave.items.iter().map(|item| item.step_id.clone()).collect(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            (WaveKind::Done, 0, ids(&[12])),
            (WaveKind::Planned, 0, ids(&[2, 3])),
            (WaveKind::Planned, 1, ids(&[4])),
            (WaveKind::Planned, 2, ids(&[6])),
            (WaveKind::Planned, 3, ids(&[8])),
            (WaveKind::Planned, 4, ids(&[10])),
        ]
    );
    // First on kick-off, then a wave length apart.
    assert_eq!(view.waves[1].estimated_start_ms, Some(NOW));
    assert_eq!(
        view.waves[2].estimated_start_ms,
        Some(NOW + DEFAULT_WAVE_MINUTES * MINUTE)
    );
    // Without anything finished there is no Done column before kick-off.
    let view = Fixture::new(vec![step(2, L::Confirmed, &[])])
        .proposed()
        .view();
    assert_eq!(shape(&view), vec![(WaveKind::Planned, ids(&[2]))]);
}

#[test]
fn hs2_j89jc7_a_cycle_goes_last_and_does_not_hang() {
    let steps = vec![
        step(2, L::Cyclic, &[4]),
        step(4, L::Cyclic, &[2]),
        step(6, L::Blocked, &[2]),
        step(3, L::Confirmed, &[]),
    ];
    let running = Fixture::new(steps.clone()).view();
    assert_eq!(
        shape(&running),
        vec![
            (WaveKind::Done, vec![]),
            (WaveKind::WorkingNow, vec![]),
            (WaveKind::Next, ids(&[3])),
            (WaveKind::Then, vec![]),
            (WaveKind::Later, ids(&[2, 4, 6])),
        ]
    );
    let proposed = Fixture::new(steps).proposed().view();
    assert_eq!(
        shape(&proposed).last().unwrap(),
        &(WaveKind::Planned, ids(&[2, 4, 6]))
    );
}

#[test]
fn hs2_j89jc7_order_of_inputs_never_changes_the_answer() {
    let fixture = Fixture::new(chain());
    let view = fixture.view();
    let mut reversed = Fixture::new(chain().into_iter().rev().collect());
    reversed.runs = fixture.runs.clone();
    assert_eq!(reversed.view(), view);
    // Repeated reads are identical.
    assert_eq!(fixture.view(), view);
    // Runs out of order: the unfinished run of a ticket wins over an older finished one
    // and over a later failed one.
    let mut runs = Fixture::new(vec![step(2, L::InProgress, &[])]);
    runs.runs = vec![
        run(
            3,
            2,
            RunState::Failed,
            NOW - 5 * MINUTE,
            Some(NOW - 4 * MINUTE),
        ),
        run(1, 2, RunState::Dispatched, NOW - 20 * MINUTE, None),
        run(
            2,
            2,
            RunState::Cancelled,
            NOW - 40 * MINUTE,
            Some(NOW - 30 * MINUTE),
        ),
    ];
    let item = runs.view().waves[1].items[0].clone();
    assert_eq!(item.run.unwrap().run_id, id(501));
    runs.runs.reverse();
    assert_eq!(
        runs.view().waves[1].items[0].run.as_ref().unwrap().run_id,
        id(501)
    );
}

#[test]
fn hs2_j89jc7_version_ignores_the_clock_and_tracks_content() {
    let mut fixture = Fixture::new(chain());
    let first = fixture.view();
    fixture.now_ms += 17 * MINUTE;
    let later = fixture.view();
    assert_ne!(first.now_ms, later.now_ms);
    assert_eq!(first.version, later.version);
    // A step finishing moves it to Done and changes the version.
    fixture.steps[0] = step(2, L::Finished, &[]);
    let finished = fixture.view();
    assert_ne!(finished.version, first.version);
    assert_eq!(finished.waves[0].items.len(), 2);
    // Back again: the same content has the same version.
    fixture.steps[0] = step(2, L::InProgress, &[]);
    assert_eq!(fixture.view().version, first.version);
    // Empty, then refilled: the refilled plan has the original version.
    let steps = std::mem::take(&mut fixture.steps);
    let empty = fixture.view();
    assert_eq!(empty.phase, TimelinePhase::NoPlan);
    assert_ne!(empty.version, first.version);
    fixture.steps = steps;
    assert_eq!(fixture.view().version, first.version);
}

#[test]
fn hs2_j89jc7_interleaved_progress_moves_items_one_wave_at_a_time() {
    // 2 runs, then finishes while 4 starts, then 4 finishes: each answer is right.
    let mut fixture = Fixture::new(vec![step(2, L::InProgress, &[]), step(4, L::Blocked, &[2])]);
    fixture.runs = vec![run(1, 2, RunState::Dispatched, NOW - MINUTE, None)];
    assert_eq!(
        shape(&fixture.view())[1..3],
        [
            (WaveKind::WorkingNow, ids(&[2])),
            (WaveKind::Next, ids(&[4]))
        ]
    );
    fixture.steps = vec![step(2, L::Finished, &[]), step(4, L::InProgress, &[])];
    fixture.runs = vec![
        run(1, 2, RunState::Completed, NOW - MINUTE, Some(NOW)),
        run(2, 4, RunState::Dispatched, NOW, None),
    ];
    assert_eq!(
        shape(&fixture.view())[0..2],
        [
            (WaveKind::Done, ids(&[2])),
            (WaveKind::WorkingNow, ids(&[4]))
        ]
    );
    fixture.steps = vec![step(2, L::Finished, &[]), step(4, L::Finished, &[])];
    fixture.runs[1] = run(2, 4, RunState::Completed, NOW, Some(NOW + MINUTE));
    let view = fixture.view();
    assert_eq!(view.waves[0].items.len(), 2);
    assert!(view.waves[1..].iter().all(|wave| wave.items.is_empty()));
    assert_eq!(view.counts, BTreeMap::from([("done".to_owned(), 2)]));
}

#[test]
fn hs2_j89jc7_placement_comes_from_the_session_then_the_scheduler() {
    let mut fixture = Fixture::new(vec![
        step(2, L::InProgress, &[]),
        step(4, L::Confirmed, &[]),
    ]);
    fixture.runs = vec![run(1, 2, RunState::Dispatched, NOW, None)];
    fixture.sessions = vec![session(1, 1, "codex")];
    for step_id in ["step-2", "step-4"] {
        fixture.hints.insert(
            step_id.into(),
            PlacementHint {
                at_ms: 9,
                decision: "unplaced".into(),
                reason: Some("target_cap".into()),
                tool: Some("claude".into()),
                ..PlacementHint::default()
            },
        );
    }
    fixture.tickets.insert(
        ticket_key(&target(2)),
        TicketLabel {
            slug: Some("API-12".into()),
            title: Some("Add the token endpoint".into()),
            ..TicketLabel::default()
        },
    );
    let view = fixture.view();
    let working = &view.waves[1].items[0];
    assert_eq!(working.tool.as_deref(), Some("codex"));
    assert_eq!(working.session_id, Some(id(701)));
    assert_eq!(working.session_state, Some(SessionState::Running));
    assert_eq!(working.ticket.slug.as_deref(), Some("API-12"));
    assert_eq!(
        (working.project_label.as_str(), working.location),
        ("API", TargetLocation::Remote)
    );
    assert_eq!(working.machine_label.as_deref(), Some("Build host"));
    let ready = &view.waves[2].items[0];
    assert_eq!(ready.tool.as_deref(), Some("claude"));
    // Without a session or a logged decision, the plan step's preferred tool (HS2-HCW7MD).
    let mut preferred = fixture.steps[1].clone();
    preferred.readiness.step.tool = Some("gemini".into());
    let mut plain = Fixture::new(vec![preferred]);
    plain.hints.clear();
    assert_eq!(
        plain.view().waves[2].items[0].tool.as_deref(),
        Some("gemini")
    );
    assert_eq!(ready.session_id, None);
    assert_eq!(
        ready.placement.as_ref().unwrap().reason.as_deref(),
        Some("target_cap")
    );
    assert_eq!(view.sessions, 1);
    // A target that is not declared any more is unbound and keeps its id as label.
    fixture.targets.clear();
    fixture.targets.push(targets().remove(1));
    let view = fixture.view();
    let orphan = &view.waves[1].items[0];
    assert_eq!(
        (orphan.project_label.as_str(), orphan.location),
        ("api", TargetLocation::Unbound)
    );
}

#[test]
fn hs2_j89jc7_waits_on_is_deduplicated_and_empty_once_running() {
    let mut waiting = step(4, L::Blocked, &[2, 6]);
    waiting.readiness.reasons.push(ReadinessReason {
        code: "gate_pending".into(),
        prerequisite: Some(prerequisite(2)),
        commits: Vec::new(),
    });
    let mut fixture = Fixture::new(vec![step(2, L::Confirmed, &[]), waiting]);
    fixture.tickets.insert(
        ticket_key(&target(6)),
        TicketLabel {
            slug: Some("API-6".into()),
            ..TicketLabel::default()
        },
    );
    let view = fixture.view();
    let item = view
        .waves
        .iter()
        .flat_map(|wave| &wave.items)
        .find(|item| item.step_id == "step-4")
        .unwrap();
    let waits: Vec<(Option<&str>, Option<&str>)> = item
        .waits_on
        .iter()
        .map(|wait| (wait.step_id.as_deref(), wait.slug.as_deref()))
        .collect();
    assert_eq!(waits, vec![(Some("step-2"), None), (None, Some("API-6"))]);
    fixture.steps[1] = step(4, L::InProgress, &[2, 6]);
    fixture.runs = vec![run(1, 4, RunState::Dispatched, NOW, None)];
    let view = fixture.view();
    assert!(view.waves[1].items[0].waits_on.is_empty());
}

#[test]
fn hs2_h5h9qp_a_provisional_item_carries_its_ledger_record_with_gate_slugs() {
    use crate::orchestration::{TimelineProvisional, TimelineProvisionalGate};
    let record = |state: &str, result: &str| TimelineProvisional {
        state: state.into(),
        gates: vec![TimelineProvisionalGate {
            target_id: target(2).target_id,
            connection_id: target(2).connection_id,
            native_id: target(2).native_id,
            slug: None,
            result: result.into(),
        }],
    };
    let item_of = |planned: TimelineStep, label: bool| {
        let mut fixture = Fixture::new(vec![planned]);
        fixture.runs.push(run(1, 4, RunState::Dispatched, 10, None));
        if label {
            fixture.tickets.insert(
                ticket_key(&target(2)),
                TicketLabel {
                    slug: Some("API-2".into()),
                    ..TicketLabel::default()
                },
            );
        }
        let view = fixture.view();
        let item = view
            .waves
            .iter()
            .flat_map(|wave| wave.items.clone())
            .next()
            .unwrap();
        (item, view.version)
    };
    // Running ahead of a pending gate, then at risk, then reopened: the record travels with
    // its prerequisite's slug, and every change moves the timeline's version.
    let mut versions = Vec::new();
    for (state, result) in [
        ("provisional", "pending"),
        ("at_risk", "red"),
        ("reopened", "red"),
    ] {
        let mut planned = step(4, L::InProgress, &[]);
        planned.provisional_work = true;
        planned.provisional = Some(record(state, result));
        let (item, version) = item_of(planned, true);
        assert_eq!(item.state, ItemState::Provisional, "{state}");
        let carried = item.provisional.expect(state);
        assert_eq!(carried.state, state);
        assert_eq!(carried.gates[0].slug.as_deref(), Some("API-2"));
        assert_eq!(carried.gates[0].result, result);
        let wire = serde_json::to_value(&carried).unwrap();
        assert_eq!(wire["gates"][0]["slug"], "API-2");
        versions.push(version);
    }
    versions.dedup();
    assert_eq!(versions.len(), 3, "each ledger change is a new version");
    // An unknown prerequisite keeps no slug and serializes none.
    let mut planned = step(4, L::InProgress, &[]);
    planned.provisional_work = true;
    planned.provisional = Some(record("provisional", "unknown"));
    let carried = item_of(planned, false).0.provisional.unwrap();
    assert_eq!(carried.gates[0].slug, None);
    assert!(
        serde_json::to_value(&carried).unwrap()["gates"][0]
            .get("slug")
            .is_none()
    );
    // Finished work, and a step without a record, carry nothing, and serialize nothing.
    let mut finished = step(4, L::Finished, &[]);
    finished.provisional = Some(record("confirmed", "green"));
    let (done, _) = item_of(finished, true);
    assert_eq!(done.provisional, None);
    let (plain, _) = item_of(step(4, L::InProgress, &[]), true);
    assert_eq!(plain.provisional, None);
    assert!(
        serde_json::to_value(&plain)
            .unwrap()
            .get("provisional")
            .is_none()
    );
}

/// HS2-6GE7WM: a step names the project's home site, and the scheduler placed its run on
/// another site. The item reports the run's site (its id, label, location and machine),
/// not the one the step names, and sorts with that site; before a run it keeps the named
/// site.
#[test]
fn hs2_6ge7wm_an_item_reports_the_site_its_run_was_placed_on() {
    // Steps 1 and 3 both name "web" (local); step 3's run went to the "api" site on the
    // build host. "api" is declared first.
    let mut fixture = Fixture::new(vec![
        step(1, L::InProgress, &[]),
        step(3, L::InProgress, &[]),
    ]);
    let mut placed = run(3, 3, RunState::Requested, 10, None);
    placed.target.target_id = "api".into();
    placed.target.connection_id = "api-source".into();
    fixture.runs = vec![placed.clone()];
    let item = |view: &Timeline, step: &str| -> TimelineItem {
        view.waves
            .iter()
            .flat_map(|wave| wave.items.iter())
            .find(|item| item.step_id == step)
            .cloned()
            .unwrap()
    };
    let working_now = |view: &Timeline| -> Vec<(String, String)> {
        view.waves
            .iter()
            .find(|wave| wave.kind == WaveKind::WorkingNow)
            .unwrap()
            .items
            .iter()
            .map(|item| (item.step_id.clone(), item.site_id.clone()))
            .collect()
    };
    // Without knowing the run was placed for the step, the step's own site is shown and
    // no run is attached: the defect the trial saw.
    let unknown = fixture.view();
    let before = item(&unknown, "step-3");
    assert!(before.run.is_none());
    assert_eq!(before.location, TargetLocation::Local);
    assert_eq!(before.project_label, "Web");

    fixture
        .step_runs
        .insert("step-3".into(), BTreeSet::from([placed.run_id.clone()]));
    let view = fixture.view();
    let working = item(&view, "step-3");
    assert_eq!(working.run.as_ref().unwrap().run_id, placed.run_id);
    assert_eq!(working.site_id, "api");
    assert_eq!(working.location, TargetLocation::Remote);
    assert_eq!(working.project_label, "API");
    assert_eq!(working.machine_label.as_deref(), Some("Build host"));
    // The step itself still names its home site.
    assert_eq!(working.target.target_id, "web");
    // A step without a run keeps the site it names.
    let waiting = item(&view, "step-1");
    assert!(waiting.run.is_none());
    assert_eq!(
        (
            waiting.location,
            waiting.site_id.as_str(),
            waiting.project_label.as_str()
        ),
        (TargetLocation::Local, "web", "Web")
    );
    // Cards gather by the site the work runs on, in declaration order.
    assert_eq!(
        working_now(&view),
        [
            ("step-3".into(), "api".into()),
            ("step-1".into(), "web".into())
        ]
    );
    assert_eq!(
        working_now(&unknown),
        [
            ("step-1".into(), "web".into()),
            ("step-3".into(), "web".into())
        ]
    );

    // The same again changes nothing; a later placement back on the home wins once it is
    // the unfinished one, and the cancelled remote run no longer decides the site.
    assert_eq!(fixture.view(), view);
    fixture.runs[0].state = RunState::Cancelled;
    let mut home = run(4, 3, RunState::Requested, 30, None);
    home.operation_id = "op-home".into();
    fixture
        .step_runs
        .get_mut("step-3")
        .unwrap()
        .insert(home.run_id.clone());
    fixture.runs.push(home.clone());
    let moved = item(&fixture.view(), "step-3");
    assert_eq!(moved.run.as_ref().unwrap().run_id, home.run_id);
    assert_eq!(moved.location, TargetLocation::Local);
    assert_eq!(moved.machine_label, None);

    // A placed run on a site no longer declared shows its id and no location.
    let mut gone = Fixture::new(vec![step(1, L::InProgress, &[])]);
    let mut orphan = run(5, 1, RunState::Requested, 10, None);
    orphan.target.target_id = "retired".into();
    gone.step_runs
        .insert("step-1".into(), BTreeSet::from([orphan.run_id.clone()]));
    gone.runs = vec![orphan];
    let lost = item(&gone.view(), "step-1");
    assert_eq!(
        (lost.site_id.as_str(), lost.project_label.as_str()),
        ("retired", "retired")
    );
    assert_eq!(lost.location, TargetLocation::Unbound);
}

/// HS2-CQ0PNT: "api" is a site of the "web" project. A step planned through one site
/// keeps its run, its label and the prerequisite that names it through the other
/// site, as readiness does; moving the step to the other site while the run stands
/// changes none of that. Without the grouping, targets compare exactly as before.
#[test]
fn hs2_cq0pnt_a_step_keeps_its_run_label_and_dependents_through_any_site() {
    let item = |view: &Timeline, step: &str| -> TimelineItem {
        view.waves
            .iter()
            .flat_map(|wave| wave.items.iter())
            .find(|item| item.step_id == step)
            .cloned()
            .unwrap()
    };
    // Step 3 works ticket 3 and is in progress; step 5 waits on ticket 3, named on web.
    let mut fixture = Fixture::new(vec![step(3, L::InProgress, &[]), step(5, L::Blocked, &[3])]);
    // The run was dispatched through the "api" site, by hand: not a scheduler run.
    let mut through_api = run(3, 3, RunState::Requested, 10, None);
    through_api.target.target_id = "api".into();
    fixture.runs = vec![through_api.clone()];
    // The ticket's label was last read through the api site.
    fixture.tickets.insert(
        ("api".into(), SOURCE.into(), id(3)),
        TicketLabel {
            slug: Some("TGT-3".into()),
            ..TicketLabel::default()
        },
    );
    fixture.steps[1].readiness.reasons = vec![ReadinessReason {
        code: "prerequisite_open".into(),
        prerequisite: Some(prerequisite(3)),
        commits: Vec::new(),
    }];

    // Two separate projects: exactly as before, nothing joins.
    let apart = fixture.view();
    assert!(item(&apart, "step-3").run.is_none());
    assert_eq!(item(&apart, "step-3").ticket.slug, None);

    // One project: the run and the label join the step.
    fixture.sites = ProjectSites::default().with_site("api", "web");
    for moved in [false, true] {
        if moved {
            // The step is planned through the api site now, the run still stands.
            fixture.steps[0].readiness.step.target.target_id = "api".into();
        }
        let view = fixture.view();
        let working = item(&view, "step-3");
        assert_eq!(
            working.run.as_ref().map(|run| run.run_id.clone()),
            Some(through_api.run_id.clone()),
            "moved {moved}"
        );
        assert_eq!(working.site_id, "api", "moved {moved}");
        assert_eq!(
            working.ticket.slug.as_deref(),
            Some("TGT-3"),
            "moved {moved}"
        );
        // The prerequisite named through web still finds the step, wherever it moved.
        let waiting = item(&view, "step-5");
        assert_eq!(waiting.waits_on.len(), 1, "moved {moved}");
        assert_eq!(
            waiting.waits_on[0].step_id.as_deref(),
            Some("step-3"),
            "moved {moved}"
        );
        assert_eq!(waiting.waits_on[0].slug.as_deref(), Some("TGT-3"));
    }
    // A run of a different ticket on the other site never joins.
    let mut other = run(4, 7, RunState::Requested, 20, None);
    other.target.target_id = "web".into();
    fixture.runs = vec![other];
    assert!(item(&fixture.view(), "step-3").run.is_none());
}
