//! HS2-ECWG50: the scheduler through the shared runtime against real git ticket
//! stores, a scripted capacity probe and a recording session executor. Synthetic
//! tickets and identities only.
use hotsheet_model::Ticket;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, CpuCapacity, DecisionKind, DecisionMode, DiskCapacity, GateKind, HostObservation,
    LimitSignal, MachineLimits, MemoryCapacity, PermissionAction, PermissionAnswer,
    PermissionState, PlacementAction, PlacementPolicy, PolicyBody, RunEvent, RunEventJournal,
    RunEventKind, RunEventSource, ScheduledSession, SchedulerStore, SessionActivity, SetPolicy,
    SignalKind, TargetAction, TargetPolicy, ToolAvailability, ToolPolicy, ToolScope,
};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::{Value, json};
use std::time::Duration;

use super::gates::{CI, Scripted as ScriptedGates, link, sha};
use super::tickets::{Machine, NOW, bind_and_allow, human, machine, unsupported, worker};
use super::*;

/// A probe whose answer the test sets, counting how often the runtime asked.
#[derive(Default)]
struct Probe {
    observation: Mutex<HostObservation>,
    asked: AtomicUsize,
}
impl Probe {
    fn set(&self, change: impl FnOnce(&mut HostObservation)) {
        change(
            &mut self
                .observation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}
impl CapacityProbe for Probe {
    fn observe(&self, _: &Checkout) -> HostObservation {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.observation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// An idle machine with room and both tools installed.
fn roomy() -> HostObservation {
    HostObservation {
        sampled_at_ms: Some(150),
        memory: MemoryCapacity {
            free_mb: Some(8192),
            total_mb: Some(16_384),
        },
        cpu: CpuCapacity {
            load_percent: Some(10),
            logical_cpus: Some(8),
        },
        disk: DiskCapacity {
            free_mb: Some(60_000),
            total_mb: Some(500_000),
        },
        tools: Some(
            ["codex", "claude"]
                .map(|tool| ToolAvailability {
                    tool: tool.into(),
                    installed: true,
                    configured: true,
                })
                .into(),
        ),
        sessions: Some(Vec::new()),
    }
}

fn scope(tool: &str) -> ToolScope {
    ToolScope {
        tool: tool.into(),
        models: Vec::new(),
    }
}
/// Target 1 takes two sessions of `codex` (preferred) or `claude`.
fn policy(automatic: bool) -> PolicyBody {
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 4,
            ..MachineLimits::default()
        },
        tools: [("codex", 60), ("claude", 50)]
            .map(|(tool, weight)| ToolPolicy {
                tool: tool.into(),
                max_sessions: 4,
                weight,
                accounts: Vec::new(),
            })
            .into(),
        targets: vec![TargetPolicy {
            target_id: id(1),
            enabled: true,
            actions: None,
            tools: vec![scope("codex"), scope("claude")],
            max_sessions: 2,
            machine: None,
            speculation: None,
        }],
        placement: PlacementPolicy {
            automatic,
            actor: Some("worker-1".into()),
            ..PlacementPolicy::default()
        },
        ..PolicyBody::default()
    }
}
fn set_policy(m: &Machine, body: PolicyBody, tx: &mut Transaction<'_>) {
    let expected_revision = m.runtime.policy(&m.owner.id).unwrap().revision;
    m.runtime
        .set_policy(
            &m.owner.id,
            SetPolicy {
                expected_revision,
                policy: body,
            },
            Some(&human()),
            tx,
        )
        .unwrap();
}

struct Setup {
    m: Machine,
    probe: Arc<Probe>,
    sessions: Arc<RecordingSessions>,
    orchestration: Ticket,
}

/// A bound target with a roomy machine, a recording executor, and rules a person
/// remembered: dispatch, and (when `grant_sessions`) codex and claude session starts
/// (HS2-XHQ1EH).
fn setup(grant_sessions: bool, tx: &mut Transaction<'_>) -> Setup {
    let mut m = machine(10, false, unsupported);
    let probe = Arc::new(Probe::default());
    probe.set(|observation| *observation = roomy());
    let sessions = Arc::new(RecordingSessions::default());
    m.runtime = m
        .runtime
        .clone()
        .with_capacity_probe(probe.clone())
        .with_session_executor(sessions.clone());
    bind_and_allow(
        &m,
        1,
        &[
            TargetAction::TicketsRead,
            TargetAction::GatesObserve,
            TargetAction::WorkStart,
        ],
        tx,
    );
    if grant_sessions {
        allow_on(
            &m.runtime,
            &m.owner.id,
            &id(1),
            &[TargetAction::SessionsManage],
            &["codex", "claude"],
            tx,
        );
    }
    let orchestration = m.orchestration_ticket("Coordinate");
    Setup {
        m,
        probe,
        sessions,
        orchestration,
    }
}

impl Setup {
    fn owner(&self) -> String {
        self.m.owner.id.clone()
    }
    /// A new target ticket planned as a step with no prerequisite: confirmed work.
    fn ready(&self, title: &str, tx: &mut Transaction<'_>) -> (Ticket, String) {
        let ticket = ops::create(
            &self.m.target_store,
            ulid::Ulid::new(),
            "TGT",
            NOW.into(),
            NewTicket {
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap();
        let step = self
            .m
            .runtime
            .readiness_define(
                &self.owner(),
                serde_json::from_value(json!({
                    "ticket": {"connection_id": self.m.owner_connection(),
                        "native_id": self.orchestration.id.to_string()},
                    "target_id": id(1), "connection_id": self.m.target_connection(),
                    "native_id": ticket.slug, "prerequisites": [],
                }))
                .unwrap(),
                Some(&worker()),
                tx,
            )
            .unwrap()
            .readiness
            .step
            .step_id;
        (ticket, step)
    }
    fn tick(&self, tx: &mut Transaction<'_>) -> TickView {
        self.m
            .runtime
            .scheduler_tick(&self.owner(), &TickRequest::default(), Some(&worker()), tx)
            .unwrap()
    }
    fn log(&self) -> Vec<hotsheet_ticketing::orchestration::DecisionRecord> {
        self.m
            .runtime
            .scheduler_decisions(&self.owner(), &DecisionsRequest::default())
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.record)
            .collect()
    }
    fn audited(&self, action: &str) -> Vec<(AuditOutcome, Option<String>, Option<String>)> {
        self.m
            .runtime
            .audit(&self.owner(), 0, 500)
            .unwrap()
            .entries
            .into_iter()
            .filter(|entry| entry.record.action == action)
            .map(|entry| {
                (
                    entry.record.outcome,
                    entry.record.reason,
                    entry.record.operation_id,
                )
            })
            .collect()
    }
    fn memory(&self) -> Vec<hotsheet_ticketing::orchestration::PlacementMemory> {
        SchedulerStore::new(&self.m.home)
            .memory(&self.owner())
            .unwrap()
    }
}

fn owned(name: &str, owner: &str, activity: SessionActivity) -> ScheduledSession {
    ScheduledSession {
        session_id: name.into(),
        target_id: id(1),
        tool: "codex".into(),
        account: None,
        model: None,
        run_id: None,
        owner_id: Some(owner.into()),
        activity,
        started_at_ms: 0,
        idle_since_ms: None,
        safe_point: activity == SessionActivity::Idle,
        human_takeover: false,
        winding_down: false,
    }
}

fn outcomes(view: &TickView) -> Vec<(&str, Option<&str>)> {
    view.outcomes
        .iter()
        .map(|outcome| (outcome.outcome.as_str(), outcome.detail.as_deref()))
        .collect()
}
fn unplaced(view: &ScheduleView) -> Vec<(&'static str, Option<&str>)> {
    view.schedule
        .unplaced
        .iter()
        .map(|item| (item.reason.code(), item.detail.as_deref()))
        .collect()
}

macro_rules! tx {
    ($tx:ident) => {
        let mut rng = Rng(100);
        let mut $tx = Transaction {
            clock: &Clock,
            rng: &mut rng,
        };
    };
}

#[test]
fn hs2_ecwg50_without_a_policy_nothing_is_scheduled_logged_or_contacted() {
    tx!(tx);
    let s = setup(true, &mut tx);
    s.ready("First", &mut tx);
    let owner = s.owner();
    let plan =
        s.m.runtime
            .scheduler_plan(&owner, Some(&worker()), &mut tx)
            .unwrap();
    assert!(!plan.enabled);
    assert_eq!(plan.mode, DecisionMode::DryRun);
    assert!(plan.schedule.placements.is_empty() && plan.schedule.ranking.is_empty());
    let tick = s.tick(&mut tx);
    assert!(!tick.view.enabled);
    assert_eq!((tick.logged, tick.outcomes.len()), (0, 0));
    assert!(s.log().is_empty());
    assert_eq!(s.probe.asked(), 0);
    assert!(s.sessions.calls().is_empty());
    for file in [
        "orchestration-scheduler.json",
        "orchestration-scheduler-decisions",
        "orchestration-scheduler-wake",
    ] {
        assert!(!s.m.home.join(file).exists(), "{file}");
    }
    assert!(s.m.runtime.scheduler_owners().is_empty());
    let explained =
        s.m.runtime
            .scheduler_explain(
                &owner,
                &serde_json::from_value(json!({"step_id": id(999)})).unwrap(),
                None,
                &mut tx,
            )
            .unwrap();
    assert_eq!(explained.standing, "not_planned");
}

#[test]
fn hs2_ecwg50_a_policy_without_automatic_placement_only_plans_and_logs() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let (_, first) = s.ready("First", &mut tx);
    let (_, second) = s.ready("Second", &mut tx);
    let (_, third) = s.ready("Third", &mut tx);
    set_policy(&s.m, policy(false), &mut tx);
    let owner = s.owner();
    assert!(s.m.runtime.scheduler_owners().is_empty());

    // The plan is a read: it reads capacity through the gate and writes nothing.
    let plan =
        s.m.runtime
            .scheduler_plan(&owner, Some(&worker()), &mut tx)
            .unwrap();
    assert!(plan.enabled);
    assert_eq!(plan.mode, DecisionMode::DryRun);
    assert_eq!(s.probe.asked(), 1, "one target, one reading");
    let starts: Vec<(&str, &str)> = plan
        .schedule
        .placements
        .iter()
        .map(|placement| (placement.steps[0].as_str(), placement.tool.as_str()))
        .collect();
    assert_eq!(
        starts,
        [(first.as_str(), "codex"), (second.as_str(), "codex")]
    );
    assert!(
        plan.schedule
            .placements
            .iter()
            .all(|placement| { placement.action == PlacementAction::Start && placement.probe })
    );
    assert_eq!(unplaced(&plan), [("target_cap", None)]);
    assert_eq!(
        (plan.inputs.work, plan.inputs.confirmed, plan.inputs.targets),
        (3, 3, 1)
    );
    assert_eq!(plan.inputs.executor, "available");
    assert!(!plan.inputs.automatic);
    assert!(s.log().is_empty());

    // Without an actor no target is contacted, and the plan says so.
    let anonymous = s.m.runtime.scheduler_plan(&owner, None, &mut tx).unwrap();
    assert_eq!(s.probe.asked(), 1);
    assert_eq!(
        unplaced(&anonymous),
        [("target_unavailable", Some("actor_required")); 3]
    );

    // A tick computes and logs the same decisions and applies none of them.
    let tick = s.tick(&mut tx);
    assert_eq!(tick.view.mode, DecisionMode::DryRun);
    assert_eq!(tick.trigger, "manual");
    assert!(tick.outcomes.is_empty());
    assert!(s.sessions.calls().is_empty());
    assert!(s.memory().is_empty());
    assert!(!s.m.home.join("orchestration-scheduler.json").exists());
    assert_eq!(tick.logged, 4);
    let log = s.log();
    assert_eq!(
        log.iter().map(|record| record.kind).collect::<Vec<_>>(),
        [
            DecisionKind::Tick,
            DecisionKind::Start,
            DecisionKind::Start,
            DecisionKind::Unplaced
        ]
    );
    for record in &log {
        assert_eq!(record.mode, DecisionMode::DryRun);
        assert_eq!(record.tick_id, tick.tick_id);
        assert_eq!(record.actor_id.as_deref(), Some("worker-1"));
        assert!(record.outcome.is_none() && record.operation_id.is_none());
    }
    let summary = log[0].tick.as_ref().unwrap();
    assert_eq!((summary.starts, summary.unplaced, summary.work), (2, 1, 3));
    assert!(summary.policy_revision.is_some());
    // Each decision carries its reason and the figures it was decided on.
    assert_eq!(log[1].reason.as_deref(), Some("ready_work"));
    assert_eq!(log[1].step_id.as_deref(), Some(first.as_str()));
    let inputs = log[1].inputs.as_ref().unwrap();
    assert_eq!(
        (
            inputs.target_sessions,
            inputs.target_cap,
            inputs.free_ram_mb
        ),
        (1, Some(2), Some(8192))
    );
    assert_eq!(inputs.tool_state.as_deref(), Some("unknown"));
    assert!(log[1].score.is_some());
    assert_eq!(log[3].reason.as_deref(), Some("target_cap"));
    assert_eq!(log[3].step_id.as_deref(), Some(third.as_str()));

    // The log pages by cursor, and a bad page size is refused.
    let page =
        s.m.runtime
            .scheduler_decisions(
                &owner,
                &serde_json::from_value(json!({"limit": 3})).unwrap(),
            )
            .unwrap();
    assert_eq!((page.entries.len(), page.more), (3, true));
    let rest =
        s.m.runtime
            .scheduler_decisions(
                &owner,
                &serde_json::from_value(json!({"after": page.cursor})).unwrap(),
            )
            .unwrap();
    assert_eq!(rest.entries.len(), 1);
    for limit in [0, 201] {
        assert!(
            s.m.runtime
                .scheduler_decisions(
                    &owner,
                    &serde_json::from_value(json!({"limit": limit})).unwrap()
                )
                .is_err()
        );
    }
    // An explicit dry run of an automatic policy applies nothing either.
    set_policy(&s.m, policy(true), &mut tx);
    assert_eq!(s.m.runtime.scheduler_owners(), std::slice::from_ref(&owner));
    let dry =
        s.m.runtime
            .scheduler_tick(
                &owner,
                &serde_json::from_value(json!({"dry_run": true})).unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(dry.view.mode, DecisionMode::DryRun);
    assert!(dry.outcomes.is_empty() && s.sessions.calls().is_empty());
    // Applying needs someone to act as.
    let error =
        s.m.runtime
            .scheduler_tick(&owner, &TickRequest::default(), None, &mut tx)
            .unwrap_err();
    assert_eq!(error.code(), "invalid_request");
}

#[test]
fn hs2_ecwg50_automatic_placement_starts_sessions_through_the_executor_once() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let (_, first) = s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);

    let tick = s.tick(&mut tx);
    assert_eq!(tick.view.mode, DecisionMode::Applied);
    assert_eq!(outcomes(&tick), [("started", None)]);
    assert_eq!(tick.outcomes[0].session_id.as_deref(), Some("session-1"));
    let operation = format!("sched-start-{first}");
    assert_eq!(tick.outcomes[0].operation_id, operation);
    let [SessionCall::Start(request)] = &s.sessions.calls()[..] else {
        panic!("one start");
    };
    assert_eq!(
        (
            request.target_id.clone(),
            request.tool.as_str(),
            request.actor_id.as_str(),
            request.steps.clone(),
            request.probe
        ),
        (id(1), "codex", "worker-1", vec![first.clone()], true)
    );
    assert_eq!(request.operation_id, operation);
    assert_eq!(
        (
            request.load.target_sessions,
            request.load.machine_sessions,
            request.load.tool_sessions
        ),
        (0, 0, 0)
    );
    assert_eq!(request.limits.max_sessions, 4);
    // Logged with its outcome, remembered, and audited.
    let log = s.log();
    assert_eq!(log[1].kind, DecisionKind::Start);
    assert_eq!(log[1].mode, DecisionMode::Applied);
    assert_eq!(log[1].outcome.as_deref(), Some("started"));
    assert_eq!(log[1].session_id.as_deref(), Some("session-1"));
    assert_eq!(log[1].operation_id.as_deref(), Some(operation.as_str()));
    assert_eq!(s.memory()[0].last_started_at_ms, Some(200));
    assert_eq!(
        s.audited("scheduler_start"),
        [(
            AuditOutcome::Succeeded,
            Some("started".into()),
            Some(operation)
        )]
    );

    // The step is still ready until the target's dispatcher assigns it. The session
    // just started takes it: however often a pass runs, nothing more starts.
    for _ in 0..3 {
        let again = s.tick(&mut tx);
        assert!(again.outcomes.is_empty());
        let placement = &again.view.schedule.placements[0];
        assert_eq!(
            (placement.action, placement.reason.code(), &placement.steps),
            (PlacementAction::Keep, "refill", &vec![first.clone()])
        );
    }
    assert_eq!(s.sessions.calls().len(), 1);
    // More work: one more session, to the target's cap of two, and no further.
    s.ready("Second", &mut tx);
    s.ready("Third", &mut tx);
    let more = s.tick(&mut tx);
    assert_eq!(outcomes(&more), [("started", None)]);
    assert_eq!(unplaced(&more.view), [("target_cap", None)]);
    let SessionCall::Start(second) = &s.sessions.calls()[1] else {
        panic!("a start");
    };
    assert_eq!(
        (second.load.target_sessions, second.load.tool_sessions),
        (1, 1)
    );
    assert!(s.tick(&mut tx).outcomes.is_empty());
    assert_eq!(s.sessions.calls().len(), 2);
}

#[test]
fn hs2_xhq1eh_a_start_no_rule_allows_files_a_permission_request_and_nothing_is_done() {
    tx!(tx);
    // HS2-XHQ1EH: no session start is remembered.
    let s = setup(false, &mut tx);
    let (_, first) = s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);
    let owner = s.owner();
    let requests = |tx: &mut Transaction<'_>| {
        s.m.runtime
            .permission_list(&owner, tx)
            .unwrap()
            .requests
            .into_iter()
            .map(|view| view.request)
            .collect::<Vec<_>>()
    };

    // A person is asked before a session starts: asked once, however often.
    for _ in 0..3 {
        let tick = s.tick(&mut tx);
        assert_eq!(tick.outcomes.len(), 1);
        assert_eq!(tick.outcomes[0].outcome, "permission_required");
    }
    assert!(s.sessions.calls().is_empty());
    assert!(s.memory().is_empty());
    let asked = requests(&mut tx);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        (
            asked[0].key.action,
            asked[0].key.tool.as_deref(),
            asked[0].state,
            asked[0].operation_id.clone(),
            asked[0].actor_id.as_str()
        ),
        (
            PermissionAction::SessionStart,
            Some("codex"),
            PermissionState::Pending,
            format!("sched-start-{first}"),
            "worker-1"
        )
    );
    // The decision log names the request to answer.
    assert_eq!(
        s.log()
            .iter()
            .filter(|record| record.kind == DecisionKind::Start)
            .map(|record| (record.outcome.clone(), record.outcome_detail.clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                Some("permission_required".into()),
                Some(asked[0].request_id.clone())
            );
            3
        ]
    );

    // A person allows it once: the next pass hands the start to the executor. The
    // recording executor starts nothing through the gate, so the scheduler uses the
    // answer itself once the start is reported.
    answer(
        &s.m.runtime,
        &owner,
        &asked[0].request_id,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    assert_eq!(outcomes(&s.tick(&mut tx)), [("started", None)]);
    assert_eq!(s.sessions.calls().len(), 1);
    assert_eq!(requests(&mut tx)[0].state, PermissionState::Used);
}

#[test]
fn hs2_xhq1eh_a_denied_start_is_a_typed_outcome_and_never_started() {
    tx!(tx);
    let s = setup(false, &mut tx);
    s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);
    let owner = s.owner();
    s.tick(&mut tx);
    let asked =
        s.m.runtime
            .permission_list(&owner, &mut tx)
            .unwrap()
            .requests
            .remove(0)
            .request;
    answer(
        &s.m.runtime,
        &owner,
        &asked.request_id,
        PermissionAnswer::Deny,
        &mut tx,
    );
    for _ in 0..2 {
        let tick = s.tick(&mut tx);
        assert_eq!(tick.outcomes.len(), 1);
        assert_eq!(tick.outcomes[0].outcome, "permission_denied");
    }
    assert!(s.sessions.calls().is_empty());
    assert_eq!(
        s.m.runtime
            .permission_list(&owner, &mut tx)
            .unwrap()
            .requests
            .len(),
        1
    );
    assert_eq!(
        s.audited("sessions_manage").last().unwrap().0,
        AuditOutcome::Denied
    );
}

#[test]
fn hs2_ecwg50_without_an_executor_the_outcome_is_typed_and_logged() {
    tx!(tx);
    let mut s = setup(true, &mut tx);
    s.m.runtime =
        s.m.runtime
            .clone()
            .with_session_executor(Arc::new(ExecutorUnavailable));
    s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);
    for _ in 0..2 {
        let tick = s.tick(&mut tx);
        assert_eq!(tick.view.inputs.executor, "unavailable");
        assert_eq!(tick.view.mode, DecisionMode::Applied);
        assert_eq!(outcomes(&tick), [("executor_unavailable", None)]);
    }
    let log = s.log();
    assert_eq!(log[0].tick.as_ref().unwrap().executor, "unavailable");
    assert_eq!(log[1].outcome.as_deref(), Some("executor_unavailable"));
    // Nothing happened, so nothing is remembered or audited as done.
    assert!(s.memory().is_empty());
    assert!(s.audited("scheduler_start").is_empty());

    // An executor that cannot list its sessions: the pass is marked, nothing is
    // started on a guess, and the listing is not treated as empty.
    let failing = Arc::new(RecordingSessions::default());
    *failing.fail_list.lock().unwrap() = Some("session_store_unreadable".into());
    s.m.runtime = s.m.runtime.clone().with_session_executor(failing.clone());
    let tick = s.tick(&mut tx);
    assert_eq!(tick.view.inputs.executor, "failed");
    assert_eq!(outcomes(&tick), [("executor_failed", None)]);
    assert!(failing.calls().is_empty());
    // A listing that carries anything but identifiers is refused whole.
    *failing.fail_list.lock().unwrap() = None;
    let mut bad = owned("s 1", &s.owner(), SessionActivity::Idle);
    bad.tool = "a tool".into();
    failing.sessions.lock().unwrap().push(bad);
    assert_eq!(s.tick(&mut tx).view.inputs.executor, "failed");
}

#[test]
fn hs2_ecwg50_a_refused_start_backs_off_and_leaves_the_target_alone() {
    tx!(tx);
    let s = setup(true, &mut tx);
    s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);
    *s.sessions.refuse_starts.lock().unwrap() = Some("below_ram_floor".into());

    let tick = s.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("refused", Some("below_ram_floor"))]);
    let memory = s.memory();
    assert_eq!(
        (
            memory[0].refusals,
            memory[0].refused_at_ms,
            memory[0].refusal_reason.as_deref()
        ),
        (1, Some(200), Some("below_ram_floor"))
    );
    assert_eq!(
        s.audited("scheduler_start"),
        [(
            AuditOutcome::Failed,
            Some("refused".into()),
            Some(tick.outcomes[0].operation_id.clone())
        )]
    );
    // While the backoff runs the target is neither read nor asked again.
    let (asked, calls) = (s.probe.asked(), s.sessions.calls().len());
    for _ in 0..3 {
        let tick = s.tick(&mut tx);
        assert!(tick.outcomes.is_empty());
        assert_eq!(
            unplaced(&tick.view),
            [("refusal_backoff", Some("below_ram_floor"))]
        );
        assert_eq!(tick.view.schedule.retry_at_ms, Some(200 + 60_000));
    }
    assert_eq!((s.probe.asked(), s.sessions.calls().len()), (asked, calls));
    assert_eq!(s.memory()[0].refusals, 1, "waiting is not another refusal");
}

#[test]
fn hs2_ecwg50_a_machine_without_room_is_reported_and_backed_off() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let (_, first) = s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);
    let owner = s.owner();
    // 2500 MB free against a 2048 MB floor and a 1024 MB session.
    s.probe
        .set(|observation| observation.memory.free_mb = Some(2500));
    let explain = |tx: &mut Transaction<'_>| {
        s.m.runtime
            .scheduler_explain(
                &owner,
                &serde_json::from_value(json!({"step_id": first})).unwrap(),
                Some(&worker()),
                tx,
            )
            .unwrap()
    };
    let before = explain(&mut tx);
    assert_eq!(
        (
            before.running,
            before.standing.as_str(),
            before.reason.as_str(),
            before.detail.as_deref(),
            before.rank
        ),
        (
            false,
            "not_placed",
            "machine_refused",
            Some("below_ram_floor"),
            Some(1)
        )
    );
    assert!(s.memory().is_empty(), "explaining remembers nothing");

    let tick = s.tick(&mut tx);
    assert!(tick.outcomes.is_empty() && s.sessions.calls().is_empty());
    assert_eq!(
        unplaced(&tick.view),
        [("machine_refused", Some("below_ram_floor"))]
    );
    assert_eq!(s.memory()[0].refusals, 1);
    let asked = s.probe.asked();
    let after = explain(&mut tx);
    assert_eq!(
        (after.reason.as_str(), after.retry_at_ms),
        ("refusal_backoff", Some(60_200))
    );
    assert_eq!(s.probe.asked(), asked);
}

#[test]
fn hs2_ecwg50_explain_answers_for_every_standing_a_ticket_can_have() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let (first_ticket, first) = s.ready("First", &mut tx);
    let (second_ticket, second) = s.ready("Second", &mut tx);
    let (third_ticket, third) = s.ready("Third", &mut tx);
    set_policy(&s.m, policy(false), &mut tx);
    let owner = s.owner();
    let ask = |body: Value, tx: &mut Transaction<'_>| {
        s.m.runtime.scheduler_explain(
            &owner,
            &serde_json::from_value(body).unwrap(),
            Some(&worker()),
            tx,
        )
    };
    // Would start, by step and by exact ticket; a dry run says it would not be applied.
    let by_ticket = ask(
        json!({"target_id": id(1), "connection_id": s.m.target_connection(),
            "native_id": first_ticket.id.to_string()}),
        &mut tx,
    )
    .unwrap();
    assert_eq!(by_ticket.step_id.as_deref(), Some(first.as_str()));
    assert_eq!(
        (
            by_ticket.standing.as_str(),
            by_ticket.reason.as_str(),
            by_ticket.mode
        ),
        ("start", "ready_work", DecisionMode::DryRun)
    );
    assert_eq!(by_ticket.placement.unwrap().tool, "codex");
    assert_eq!(by_ticket.level.unwrap().code(), "confirmed");
    // Not placed, with the limit that stops it.
    let capped = ask(json!({"step_id": third}), &mut tx).unwrap();
    assert_eq!(
        (
            capped.standing.as_str(),
            capped.reason.as_str(),
            capped.rank
        ),
        ("not_placed", "target_cap", Some(3))
    );
    assert!(capped.score.is_some());

    // Not ready: a step whose prerequisite is unfinished names the readiness reason.
    let blocked =
        s.m.runtime
            .readiness_define(
                &owner,
                serde_json::from_value(json!({
                    "ticket": {"connection_id": s.m.owner_connection(),
                        "native_id": s.orchestration.id.to_string()},
                    "target_id": id(1), "connection_id": s.m.target_connection(),
                    "native_id": third_ticket.slug,
                    "prerequisites": [{"kind": "ticket", "target_id": id(1),
                        "connection_id": s.m.target_connection(),
                        "native_id": second_ticket.slug, "gates": "none"}],
                }))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(blocked.readiness.step.step_id, third);
    let waiting = ask(json!({"step_id": third}), &mut tx).unwrap();
    assert_eq!(
        (
            waiting.standing.as_str(),
            waiting.reason.as_str(),
            waiting.readiness_reasons.clone()
        ),
        (
            "not_ready",
            "blocked",
            vec!["prerequisite_not_finished".to_owned()]
        )
    );
    // Running: an unfinished run stands on the ticket.
    s.m.runtime
        .run_dispatch(
            &owner,
            serde_json::from_value(json!({
                "ticket": {"connection_id": s.m.owner_connection(),
                    "native_id": s.orchestration.id.to_string()},
                "target_id": id(1), "connection_id": s.m.target_connection(),
                "native_id": second_ticket.slug, "operation_id": "op-second",
                "worker_id": "worker-a",
            }))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let running = ask(json!({"step_id": second}), &mut tx).unwrap();
    assert_eq!(
        (
            running.running,
            running.standing.as_str(),
            running.reason.as_str()
        ),
        (true, "running", "in_progress")
    );
    // Not planned, and requests that name nothing or too much.
    let unknown = ask(
        json!({"target_id": id(1), "connection_id": "git", "native_id": "nothing"}),
        &mut tx,
    )
    .unwrap();
    assert_eq!(
        (unknown.standing.as_str(), unknown.reason.as_str()),
        ("not_planned", "no_plan_step")
    );
    for body in [
        json!({}),
        json!({"step_id": first, "target_id": id(1)}),
        json!({"target_id": id(1)}),
    ] {
        assert_eq!(ask(body, &mut tx).unwrap_err().code(), "invalid_request");
    }
}

#[test]
fn hs2_ecwg50_a_tool_limit_moves_the_next_start_to_another_tool_and_wakes_followers() {
    tx!(tx);
    let s = setup(true, &mut tx);
    s.ready("First", &mut tx);
    set_policy(&s.m, policy(false), &mut tx);
    let owner = s.owner();
    let tool = |tx: &mut Transaction<'_>| {
        let plan =
            s.m.runtime
                .scheduler_plan(&owner, Some(&worker()), tx)
                .unwrap();
        (
            plan.schedule
                .placements
                .first()
                .map(|placement| placement.tool.clone()),
            plan.inputs.tools_limited,
        )
    };
    assert_eq!(tool(&mut tx), (Some("codex".into()), 0));
    let token = |m: &Machine| {
        std::fs::read_to_string(m.home.join("orchestration-scheduler-wake/token")).unwrap()
    };
    let before = token(&s.m);
    let limit = |tool: &str| LimitSignal {
        tool: tool.into(),
        account: None,
        kind: SignalKind::UsageLimit,
        at_ms: 150,
        resets_at_ms: Some(100_000),
        cost_cents: None,
        session: None,
    };
    s.m.runtime
        .tool_capacity_signal(&owner, limit("codex"), Some(&worker()), &mut tx)
        .unwrap();
    assert_ne!(token(&s.m), before, "a limit wakes the scheduler");
    assert_eq!(tool(&mut tx), (Some("claude".into()), 1));
    // The same signal again changes nothing and wakes nobody.
    let after = token(&s.m);
    s.m.runtime
        .tool_capacity_signal(&owner, limit("codex"), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(token(&s.m), after);
    // Every allowed tool limited: nothing starts, until the first reset.
    OrchestrationRuntime::record_tool_signal_at(&s.m.home, &limit("claude"), 200).unwrap();
    assert_ne!(token(&s.m), after);
    let plan =
        s.m.runtime
            .scheduler_plan(&owner, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(unplaced(&plan), [("no_tool_available", Some("limited"))]);
    assert_eq!(plan.schedule.retry_at_ms, Some(100_000));
}

#[test]
fn hs2_ecwg50_sessions_are_wound_down_through_the_gate_at_a_safe_point() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let owner = s.owner();
    set_policy(&s.m, policy(true), &mut tx);
    // Three idle sessions on a target whose cap is two; one a person took over.
    let mut taken = owned("taken", &owner, SessionActivity::Idle);
    taken.human_takeover = true;
    *s.sessions.sessions.lock().unwrap() = vec![
        owned("a", &owner, SessionActivity::Idle),
        owned("b", &owner, SessionActivity::Idle),
        taken,
    ];
    let tick = s.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("wind_down_requested", None)]);
    let [SessionCall::WindDown(request)] = &s.sessions.calls()[..] else {
        panic!("one wind-down");
    };
    assert_eq!(
        (
            request.session_id.as_str(),
            request.reason.as_str(),
            request.operation_id.as_str(),
            request.actor_id.as_str()
        ),
        ("a", "over_cap", "sched-stop-a", "worker-1")
    );
    assert_eq!(s.memory()[0].last_wound_down_at_ms, Some(200));
    assert_eq!(
        s.audited("scheduler_wind_down"),
        [(
            AuditOutcome::Succeeded,
            Some("wind_down_requested".into()),
            Some("sched-stop-a".into())
        )]
    );
    // It is leaving: the next pass asks nothing more of it or of anyone.
    let again = s.tick(&mut tx);
    assert!(again.outcomes.is_empty());
    assert_eq!(s.sessions.calls().len(), 1);

    // The policy stops permitting the target: the remaining owned session is asked
    // to stop on the strength of the grant, and the taken-over one is left alone.
    let mut disabled = policy(true);
    disabled.targets[0].enabled = false;
    set_policy(&s.m, disabled, &mut tx);
    let tick = s.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("wind_down_requested", None)]);
    let SessionCall::WindDown(request) = &s.sessions.calls()[1] else {
        panic!("a wind-down");
    };
    assert_eq!(
        (request.session_id.as_str(), request.reason.as_str()),
        ("b", "policy_not_permitted")
    );
    let log = s.log();
    let last = log
        .iter()
        .rev()
        .find(|record| record.kind == DecisionKind::WindDown)
        .unwrap();
    assert_eq!(last.detail.as_deref(), Some("policy_target_disabled"));
    assert_eq!(last.outcome.as_deref(), Some("wind_down_requested"));

    // Every other answer of the executor is a typed outcome.
    for (answer, expected) in [
        (WindDownOutcome::NotAtSafePoint, "not_at_safe_point"),
        (WindDownOutcome::NotFound, "session_not_found"),
        (
            WindDownOutcome::Refused {
                reason: "handoff_pending".into(),
            },
            "refused",
        ),
        (WindDownOutcome::Unavailable, "executor_unavailable"),
    ] {
        *s.sessions.sessions.lock().unwrap() = vec![owned("c", &owner, SessionActivity::Idle)];
        *s.sessions.wind_down_answer.lock().unwrap() = Some(answer);
        assert_eq!(s.tick(&mut tx).outcomes[0].outcome, expected);
    }
}

/// HS2-48H57C, HS2-XHQ1EH: a wind-down only reduces activity. It never asks a person,
/// and a policy that no longer permits the target does not deny it, whatever is
/// remembered.
#[test]
fn hs2_xhq1eh_a_wind_down_never_asks_and_is_never_denied_by_the_policy() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let owner = s.owner();
    let mut body = policy(true);
    body.targets[0].max_sessions = 1;
    set_policy(&s.m, body, &mut tx);
    *s.sessions.sessions.lock().unwrap() = vec![
        owned("a", &owner, SessionActivity::Idle),
        owned("b", &owner, SessionActivity::Idle),
    ];
    let tick = s.tick(&mut tx);
    assert_eq!(tick.outcomes[0].outcome, "wind_down_requested");
    assert!(!s.sessions.calls().is_empty());
    // Nothing remembered for session starts, and the target disabled: still stopped.
    let s = setup(false, &mut tx);
    let owner = s.owner();
    let mut disabled = policy(true);
    disabled.targets[0].enabled = false;
    set_policy(&s.m, disabled, &mut tx);
    *s.sessions.sessions.lock().unwrap() = vec![owned("a", &owner, SessionActivity::Idle)];
    assert_eq!(s.tick(&mut tx).outcomes[0].outcome, "wind_down_requested");
    assert!(
        s.m.runtime
            .permission_list(&owner, &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
}

#[test]
fn hs2_ecwg50_a_session_whose_run_waits_on_ci_is_free_capacity() {
    tx!(tx);
    let mut s = setup(true, &mut tx);
    let ci = ScriptedGates::new(CI, GateKind::Ci);
    s.m.runtime = s.m.runtime.clone().with_gate_provider(ci);
    let owner = s.owner();
    let mut body = policy(false);
    body.targets[0].max_sessions = 1;
    set_policy(&s.m, body, &mut tx);
    // A run on one ticket, integrated at a commit whose CI gate is pending.
    let (working, _) = s.ready("Working", &mut tx);
    let run =
        s.m.runtime
            .run_dispatch(
                &owner,
                serde_json::from_value(json!({
                    "ticket": {"connection_id": s.m.owner_connection(),
                        "native_id": s.orchestration.id.to_string()},
                    "target_id": id(1), "connection_id": s.m.target_connection(),
                    "native_id": working.slug, "operation_id": "op-working",
                    "worker_id": "worker-a",
                }))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap()
            .run;
    let mut session = owned("s1", &owner, SessionActivity::Busy);
    session.run_id = Some(run.run_id.clone());
    *s.sessions.sessions.lock().unwrap() = vec![session];
    let (_, independent) = s.ready("Independent", &mut tx);
    let fate = |tx: &mut Transaction<'_>| {
        let plan =
            s.m.runtime
                .scheduler_plan(&owner, Some(&worker()), tx)
                .unwrap();
        let placement = plan.schedule.placements[0].clone();
        (
            placement.reason.code(),
            placement.steps,
            unplaced(&plan)
                .into_iter()
                .map(|(code, _)| code)
                .collect::<Vec<_>>(),
        )
    };
    // Busy on its run: the target is full and the independent work waits.
    assert_eq!(fate(&mut tx), ("busy", Vec::new(), vec!["target_cap"]));
    // Its commit goes to CI: the session waits on the gate, and takes the work.
    s.m.runtime
        .gate_link(
            &owner,
            &run.run_id,
            link(&sha('a'), CI),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(fate(&mut tx), ("refill", vec![independent], Vec::new()));
}

fn event(kind: RunEventKind) -> RunEvent {
    RunEvent {
        at_ms: 200,
        run_id: id(500),
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

#[test]
fn hs2_ecwg50_the_follower_runs_a_pass_on_events_and_never_on_its_own() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let owner = s.owner();
    let (_, first) = s.ready("First", &mut tx);
    let mut state = SchedulerFollowState::default();
    let step = |state: &mut SchedulerFollowState, wait_ms: u64, tx: &mut Transaction<'_>| {
        s.m.runtime
            .scheduler_follow_step(&owner, state, wait_ms, tx)
    };
    // No policy, then a policy without automatic placement: there is no loop.
    assert_eq!(step(&mut state, 0, &mut tx), SchedulerStep::default());
    set_policy(&s.m, policy(false), &mut tx);
    assert!(!step(&mut state, 0, &mut tx).active);
    assert!(s.log().is_empty() && s.sessions.calls().is_empty());

    // Enabled: the first step is a pass, acting as the policy's actor.
    set_policy(&s.m, policy(true), &mut tx);
    let started = step(&mut state, 0, &mut tx);
    assert_eq!(
        (started.active, started.trigger, started.failed),
        (true, Some("start"), false)
    );
    let [SessionCall::Start(request)] = &s.sessions.calls()[..] else {
        panic!("one start");
    };
    assert_eq!(request.actor_id, "worker-1");
    assert_eq!(request.steps, [first]);
    let logged = s.log().len();
    assert_eq!(s.log()[0].trigger, "start");

    // Nothing changed: no pass, no reading, no log entry, however often it steps.
    let asked = s.probe.asked();
    for _ in 0..3 {
        let idle = step(&mut state, 0, &mut tx);
        assert_eq!((idle.active, idle.trigger), (true, None));
    }
    assert_eq!((s.probe.asked(), s.log().len()), (asked, logged));

    // A lease renewal is not a reason to plan. Any other run event is.
    let journal = RunEventJournal::new(&s.m.home, &owner).unwrap();
    journal.record(event(RunEventKind::Lease)).unwrap();
    assert_eq!(step(&mut state, 0, &mut tx).trigger, None);
    for kind in [
        RunEventKind::Session,
        RunEventKind::Readiness,
        RunEventKind::Gate,
        RunEventKind::Validation,
        RunEventKind::State,
    ] {
        journal.record(event(kind)).unwrap();
        assert_eq!(
            step(&mut state, 0, &mut tx).trigger,
            Some("event"),
            "{kind:?}"
        );
        assert_eq!(step(&mut state, 0, &mut tx).trigger, None);
    }
    // The decisions did not change, so those passes logged nothing new.
    assert_eq!(
        s.log().len(),
        logged + 2,
        "the refill after the start, once"
    );
    assert_eq!(s.sessions.calls().len(), 1);

    // A tool limit or a policy change is a signal.
    OrchestrationRuntime::scheduler_wake_at(&s.m.home);
    assert_eq!(step(&mut state, 0, &mut tx).trigger, Some("signal"));
    assert_eq!(step(&mut state, 0, &mut tx).trigger, None);
    // A time the last pass named has come.
    state.retry_at_ms = Some(150);
    let retried = step(&mut state, 0, &mut tx);
    assert_eq!(
        (retried.trigger, retried.retry_at_ms),
        (Some("retry"), None)
    );
    // A time still ahead is not a reason.
    state.retry_at_ms = Some(201);
    assert_eq!(step(&mut state, 0, &mut tx).trigger, None);
    state.retry_at_ms = None;

    // A waiting step is woken by the event itself.
    let home = s.m.home.clone();
    let waker = std::thread::spawn(move || OrchestrationRuntime::scheduler_wake_at(&home));
    let waited = std::time::Instant::now();
    let mut woken = None;
    while woken.is_none() && waited.elapsed() < Duration::from_secs(20) {
        woken = step(&mut state, 5_000, &mut tx).trigger;
    }
    waker.join().unwrap();
    assert_eq!(woken, Some("signal"));

    // Automatic placement is turned off: the change wakes the follower, which ends
    // without a pass.
    let logged = s.log().len();
    set_policy(&s.m, policy(false), &mut tx);
    assert_eq!(step(&mut state, 0, &mut tx), SchedulerStep::default());
    assert_eq!(s.log().len(), logged);
    set_policy(&s.m, policy(true), &mut tx);
    assert_eq!(step(&mut state, 0, &mut tx).trigger, Some("signal"));

    // An executor that fails marks the step, so the host backs off.
    *s.sessions.fail_list.lock().unwrap() = Some("session_store_unreadable".into());
    OrchestrationRuntime::scheduler_wake_at(&s.m.home);
    assert!(step(&mut state, 0, &mut tx).failed);
    // A policy that can no longer be read keeps the loop alive, and failing.
    std::fs::write(
        std::path::Path::new(&s.m.owner.root).join(".hotsheet2/orchestration-policy.json"),
        b"{not json",
    )
    .unwrap();
    let broken = step(&mut state, 0, &mut tx);
    assert_eq!(
        (broken.active, broken.failed, broken.trigger),
        (true, true, None)
    );
}

#[test]
fn hs2_sqs266_removing_a_step_wakes_the_follower_even_when_the_plan_empties() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let owner = s.owner();
    set_policy(&s.m, policy(true), &mut tx);
    let (_, first) = s.ready("First", &mut tx);
    let (_, second) = s.ready("Second", &mut tx);
    let mut state = SchedulerFollowState::default();
    let step = |state: &mut SchedulerFollowState, tx: &mut Transaction<'_>| {
        s.m.runtime
            .scheduler_follow_step(&owner, state, 0, tx)
            .trigger
    };
    let remove = |step_id: &str, tx: &mut Transaction<'_>| {
        s.m.runtime.readiness_remove(
            &owner,
            &serde_json::from_value(json!({ "step_id": step_id })).unwrap(),
            Some(&worker()),
            tx,
        )
    };
    let journal = RunEventJournal::new(&s.m.home, &owner).unwrap();
    // `(reason, dedupe)` of a step's readiness events, oldest first.
    let levels = |step_id: &str| -> Vec<(Option<String>, Option<String>)> {
        journal
            .read(0, 100, Some(step_id))
            .unwrap()
            .entries
            .into_iter()
            .filter(|entry| entry.record.kind == RunEventKind::Readiness)
            .map(|entry| (entry.record.reason, entry.record.dedupe))
            .collect()
    };
    assert_eq!(step(&mut state, &mut tx), Some("start"));
    assert_eq!(step(&mut state, &mut tx), None);

    // A step whose removal moves no other step's level.
    remove(&first, &mut tx).unwrap();
    assert_eq!(step(&mut state, &mut tx), Some("event"));
    assert_eq!(step(&mut state, &mut tx), None);
    // The last step: the plan is empty and no level is left to publish.
    remove(&second, &mut tx).unwrap();
    assert!(
        s.m.runtime
            .readiness_plan(&owner, &mut tx)
            .unwrap()
            .steps
            .is_empty()
    );
    assert_eq!(step(&mut state, &mut tx), Some("event"));
    assert_eq!(step(&mut state, &mut tx), None);
    // Each removal is its step's last event, published once.
    for removed in [&first, &second] {
        let published = levels(removed);
        assert_eq!(
            published.last().unwrap(),
            &(Some("removed".into()), Some(format!("removed-{removed}"))),
            "{published:?}"
        );
        assert_eq!(
            published
                .iter()
                .filter(|(reason, _)| reason.as_deref() == Some("removed"))
                .count(),
            1
        );
    }
    // Removing it again changes nothing and publishes nothing.
    assert!(matches!(
        remove(&second, &mut tx),
        Err(OrchestrationError::Readiness(
            hotsheet_ticketing::orchestration::ReadinessError::StepNotFound
        ))
    ));
    assert_eq!(step(&mut state, &mut tx), None);

    // Empty, then planned again: the new step is new work with its own levels.
    let (_, third) = s.ready("Third", &mut tx);
    assert_ne!(third, second);
    assert_eq!(step(&mut state, &mut tx), Some("event"));
    assert_eq!(
        levels(&third),
        [(Some("confirmed".into()), Some("readiness-1".into()))]
    );
    // And removed again: the loop hears of it once more.
    remove(&third, &mut tx).unwrap();
    assert_eq!(step(&mut state, &mut tx), Some("event"));
    assert_eq!(step(&mut state, &mut tx), None);
}

#[test]
fn hs2_ecwg50_scheduler_routes_share_the_runtime_and_validate_their_bodies() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let (_, first) = s.ready("First", &mut tx);
    set_policy(&s.m, policy(false), &mut tx);
    let owner = s.owner();
    let base = format!("/checkouts/{owner}/orchestration/scheduler");
    let actor = json!({"role": "ai", "id": "worker-1"});
    let mut call = |operation: &str, mut body: Value| {
        body["actor"] = actor.clone();
        s.m.runtime
            .request("POST", &format!("{base}/{operation}"), &body, &mut tx)
    };
    let plan = call("plan", json!({})).unwrap();
    assert_eq!(plan["enabled"], true);
    assert_eq!(plan["mode"], "dry_run");
    assert_eq!(plan["placements"][0]["action"], "start");
    assert_eq!(plan["placements"][0]["steps"][0], first.as_str());
    assert_eq!(plan["ranking"][0]["score"]["priority"], 2000);
    let explained = call("explain", json!({"step_id": first})).unwrap();
    assert_eq!(explained["standing"], "start");
    let tick = call("tick", json!({})).unwrap();
    assert_eq!(tick["mode"], "dry_run");
    assert_eq!(tick["logged"], 2);
    assert_eq!(tick["outcomes"], json!([]));
    let log = call("decisions", json!({})).unwrap();
    assert_eq!(log["entries"][0]["kind"], "tick");
    assert_eq!(log["entries"][1]["kind"], "start");
    assert_eq!(log["entries"][1]["reason"], "ready_work");
    // Unknown members and operations are refused, and nothing was started.
    for (operation, body) in [
        ("plan", json!({"extra": 1})),
        ("tick", json!({"dry": true})),
        ("explain", json!({"step": first})),
        ("decisions", json!({"since": 0})),
        ("apply", json!({})),
    ] {
        assert_eq!(
            call(operation, body).unwrap_err().code(),
            "invalid_request",
            "{operation}"
        );
    }
    for path in [format!("{base}/plan"), format!("{base}/tick")] {
        assert!(is_path(&path) && is_machine_path(&path), "{path}");
    }
    assert!(s.sessions.calls().is_empty());
    let _ = MutationActor::parse("ai", Some("worker-1".into())).unwrap();
}
/// HS2-2FG91X: scheduling around claimable resources, on the same fixtures.
mod resources;

/// HS2-DG0YPA: a start the executor answers by adopting a live session is logged as
/// `adopted` with that session and run, audited, and is not a start: the target's
/// memory is unchanged.
#[test]
fn hs2_dg0ypa_an_adopted_start_is_logged_and_audited_and_not_remembered() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let (_, first) = s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);
    *s.sessions
        .adopt_starts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(("session-held".into(), "run-held".into()));
    let tick = s.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("adopted", None)]);
    assert_eq!(
        (
            tick.outcomes[0].session_id.as_deref(),
            tick.outcomes[0].run_id.as_deref()
        ),
        (Some("session-held"), Some("run-held"))
    );
    let log = s.log();
    assert_eq!(log[1].kind, DecisionKind::Start);
    assert_eq!(log[1].outcome.as_deref(), Some("adopted"));
    assert_eq!(log[1].session_id.as_deref(), Some("session-held"));
    assert_eq!(log[1].run_id.as_deref(), Some("run-held"));
    assert!(s.memory().is_empty());
    assert_eq!(
        s.audited("scheduler_start"),
        [(
            AuditOutcome::Succeeded,
            Some("adopted".into()),
            Some(format!("sched-start-{first}"))
        )]
    );
}

/// HS2-J3ZF27: a Software project has no orchestration. Its kept automatic policy
/// places nothing, the loop ends, every acting action is refused by the gate with a
/// typed detail, while observation and winding down stay allowed; switching back to
/// Orchestration resumes placement from the same policy.
#[test]
fn hs2_j3zf27_a_software_project_places_and_dispatches_nothing() {
    use hotsheet_ticketing::orchestration::{
        AccessRequest, DenialReason, ProjectType, SessionRequest, SetProjectType, TargetRef,
    };
    tx!(tx);
    let s = setup(true, &mut tx);
    let owner = s.owner();
    s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);
    let switch = |kind: ProjectType, tx: &mut Transaction<'_>| {
        s.m.runtime
            .set_project_type(
                &owner,
                SetProjectType {
                    project_type: kind,
                    keep_projects: true,
                },
                Some(&human()),
                tx,
            )
            .unwrap()
    };
    // A project that declared targets before Project Type existed orchestrates.
    assert_eq!(
        s.m.runtime.project_type(&owner).unwrap().project_type,
        ProjectType::Orchestration
    );
    assert_eq!(s.m.runtime.scheduler_owners(), std::slice::from_ref(&owner));

    // Back to Software, keeping the projects and the automatic policy.
    let mut state = SchedulerFollowState::default();
    let view = switch(ProjectType::Software, &mut tx);
    assert!(view.changed && view.active_projects == 1);
    assert!(s.m.runtime.scheduler_owners().is_empty());
    // The loop ends at once, without a pass; repeated steps stay ended.
    for _ in 0..2 {
        assert_eq!(
            s.m.runtime
                .scheduler_follow_step(&owner, &mut state, 0, &mut tx),
            SchedulerStep::default()
        );
    }
    // A manual tick and a plan are dry runs: nothing starts.
    for _ in 0..2 {
        let tick = s.tick(&mut tx);
        assert_eq!(tick.view.mode, DecisionMode::DryRun);
        assert!(tick.outcomes.is_empty());
    }
    let planned =
        s.m.runtime
            .scheduler_plan(&owner, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(planned.mode, DecisionMode::DryRun);
    assert!(s.sessions.calls().is_empty());

    // Acting actions are refused by the gate, even a person's own, and audited.
    let target = TargetRef {
        owner_checkout_id: owner.clone(),
        target_id: id(1),
    };
    for (actor, operation) in [(worker(), "op-ai"), (human(), "op-person")] {
        let refused = s.m.runtime.authorize(
            &target,
            Some(&actor),
            ActionRequest {
                operation_id: Some(operation.into()),
                ..ActionRequest::new(TargetAction::WorkStart)
            },
            &mut tx,
        );
        assert!(matches!(
            refused,
            Err(OrchestrationError::Denied(DenialReason::PolicyDenied))
        ));
    }
    assert_eq!(
        s.audited("work_start")
            .into_iter()
            .map(|(outcome, reason, _)| (outcome, reason))
            .collect::<Vec<_>>(),
        vec![
            (
                AuditOutcome::Denied,
                Some("policy_project_software".to_owned())
            );
            2
        ]
    );
    // The side-effect-free check says why; the dry run agrees.
    let start = SessionRequest {
        tool: "codex".into(),
        model: None,
        running: 0,
        starting: true,
    };
    let check = |session: Option<SessionRequest>, action, tx: &mut Transaction<'_>| {
        let mut request = AccessRequest::new("worker-1".to_owned(), action);
        request.session = session;
        s.m.runtime.access(&target, &request, false, tx).unwrap()
    };
    let asked = check(Some(start.clone()), TargetAction::SessionsManage, &mut tx);
    assert_eq!(
        (asked.allowed, asked.detail.as_deref()),
        (false, Some("policy_project_software"))
    );
    // Observation and winding down stay allowed: running work stays as it is.
    assert!(check(None, TargetAction::TicketsRead, &mut tx).allowed);
    let stop = SessionRequest {
        starting: false,
        ..start
    };
    assert!(check(Some(stop), TargetAction::SessionsManage, &mut tx).allowed);
    let dry =
        s.m.runtime
            .dry_run_policy(&owner, serde_json::from_value(json!({})).unwrap())
            .unwrap();
    let details: Vec<_> = dry.targets[0]
        .actions
        .iter()
        .map(|action| (action.action, action.policy_detail))
        .collect();
    for (action, detail) in details {
        let expected = (!action.observes()).then_some("policy_project_software");
        assert_eq!(detail, expected, "{action:?}");
    }

    // Forward again: the kept policy places at once.
    switch(ProjectType::Orchestration, &mut tx);
    assert_eq!(s.m.runtime.scheduler_owners(), std::slice::from_ref(&owner));
    let resumed =
        s.m.runtime
            .scheduler_follow_step(&owner, &mut state, 0, &mut tx);
    assert_eq!((resumed.active, resumed.trigger), (true, Some("start")));
    assert_eq!(s.sessions.calls().len(), 1);
}

/// HS2-AWB7BX: wait (on state) until `done` or the bound passes.
fn until_state(bound: Duration, mut done: impl FnMut() -> bool) -> bool {
    let began = std::time::Instant::now();
    while began.elapsed() < bound {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    done()
}

/// HS2-AWB7BX: follow steps until one is triggered, within `bound`.
fn followed_until(
    s: &Setup,
    state: &mut SchedulerFollowState,
    bound: Duration,
    tx: &mut Transaction<'_>,
) -> Option<&'static str> {
    let began = std::time::Instant::now();
    let mut trigger = None;
    while trigger.is_none() && began.elapsed() < bound {
        trigger =
            s.m.runtime
                .scheduler_follow_step(&s.owner(), state, 1_000, tx)
                .trigger;
    }
    trigger
}

#[test]
fn hs2_awb7bx_a_machine_that_gets_room_wakes_the_follower() {
    tx!(tx);
    let mut s = setup(true, &mut tx);
    s.m.runtime =
        s.m.runtime
            .clone()
            .with_capacity_resample(Duration::from_millis(20));
    let (_, first) = s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);
    let owner = s.owner();
    s.probe
        .set(|observation| observation.memory.free_mb = Some(2500));
    let mut state = SchedulerFollowState::default();
    let pass =
        s.m.runtime
            .scheduler_follow_step(&owner, &mut state, 0, &mut tx);
    assert_eq!(pass.trigger, Some("start"));
    assert!(s.sessions.calls().is_empty());
    // The target is followed from the version the pass read.
    assert_eq!(s.m.runtime.capacity_watches(&owner), [id(1)]);
    let asked = s.probe.asked();
    assert!(until_state(Duration::from_secs(5), || s.probe.asked() > asked));
    // While the capacity stands still nothing wakes the loop.
    assert_eq!(
        s.m.runtime
            .scheduler_follow_step(&owner, &mut state, 300, &mut tx)
            .trigger,
        None
    );
    // Room appears on the machine: the follow sees the change, lifts the refusal and
    // wakes the loop, which starts the work long before the backoff ends.
    s.probe.set(|observation| *observation = roomy());
    assert_eq!(
        followed_until(&s, &mut state, Duration::from_secs(20), &mut tx),
        Some("signal")
    );
    let [SessionCall::Start(request)] = &s.sessions.calls()[..] else {
        panic!("one start: {:?}", s.sessions.calls());
    };
    assert_eq!(request.steps, [first]);
    assert!(s.memory().iter().all(|entry| entry.refusals == 0));
    assert!(until_state(Duration::from_secs(5), || s
        .m
        .runtime
        .capacity_watches(&owner)
        .is_empty()));
}

#[test]
fn hs2_awb7bx_a_followed_capacity_refusal_waits_for_the_change_not_the_time() {
    tx!(tx);
    let mut s = setup(true, &mut tx);
    s.m.runtime =
        s.m.runtime
            .clone()
            .with_capacity_resample(Duration::from_millis(20));
    s.ready("First", &mut tx);
    set_policy(&s.m, policy(true), &mut tx);
    let owner = s.owner();
    // The host refuses the start for its own reason after the pass saw room.
    *s.sessions.refuse_starts.lock().unwrap() = Some("below_ram_floor".into());
    let mut state = SchedulerFollowState::default();
    let first =
        s.m.runtime
            .scheduler_follow_step(&owner, &mut state, 0, &mut tx);
    assert_eq!(first.trigger, Some("start"));
    assert_eq!(
        s.memory()[0].refusal_reason.as_deref(),
        Some("below_ram_floor")
    );
    // Without a follow the backoff retry stood; the next pass, with the target
    // followed, waits for the change instead of a time.
    OrchestrationRuntime::scheduler_wake_at(&s.m.home);
    let next =
        s.m.runtime
            .scheduler_follow_step(&owner, &mut state, 0, &mut tx);
    assert_eq!(next.trigger, Some("signal"));
    assert_eq!(s.m.runtime.capacity_watches(&owner), [id(1)]);
    let view = s.tick(&mut tx).view;
    assert_eq!(
        unplaced(&view),
        [("refusal_backoff", Some("below_ram_floor"))]
    );
    assert_eq!(view.schedule.unplaced[0].retry_at_ms, None);
    // The machine changes: the refusal is lifted and the target asked again.
    *s.sessions.refuse_starts.lock().unwrap() = None;
    s.probe
        .set(|observation| observation.memory.free_mb = Some(12_288));
    assert_eq!(
        followed_until(&s, &mut state, Duration::from_secs(20), &mut tx),
        Some("signal")
    );
    assert!(s.memory().iter().all(|entry| entry.refusals == 0));
    assert!(
        s.sessions
            .calls()
            .iter()
            .filter(|call| matches!(call, SessionCall::Start(_)))
            .count()
            >= 2
    );
}

#[test]
fn hs2_awb7bx_a_follow_ends_without_an_event_and_keeps_the_backoff() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let target = TargetRef {
        owner_checkout_id: s.owner(),
        target_id: id(1),
    };
    // A version that is not current answers at once as a change.
    let changed = s.m.runtime.follow_capacity_change(
        &target,
        &worker(),
        &[],
        "0000000000000000",
        std::time::Instant::now() + Duration::from_secs(30),
        &mut tx,
    );
    assert!(matches!(changed, CapacityFollow::Changed(_)));
    // A deadline already passed asks nothing.
    let asked = s.probe.asked();
    assert_eq!(
        s.m.runtime.follow_capacity_change(
            &target,
            &worker(),
            &[],
            "0000000000000000",
            std::time::Instant::now(),
            &mut tx,
        ),
        CapacityFollow::Ended("deadline".into())
    );
    assert_eq!(s.probe.asked(), asked);
    // A target the gate cannot reach ends the follow before the host is asked.
    let undeclared = TargetRef {
        owner_checkout_id: s.owner(),
        target_id: id(7),
    };
    assert!(matches!(
        s.m.runtime.follow_capacity_change(
            &undeclared,
            &worker(),
            &[],
            "0000000000000000",
            std::time::Instant::now() + Duration::from_secs(30),
            &mut tx,
        ),
        CapacityFollow::Ended(code) if code != "deadline"
    ));
    assert_eq!(s.probe.asked(), asked);
}
