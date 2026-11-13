//! HS2-95SWQ4: the scheduler placing real orchestrator-owned sessions through the
//! shared runtime, with a scripted terminal host and a clock the test moves. No AI
//! tool is launched: the host only remembers what it was asked. Synthetic tickets,
//! tools and identities only.
use std::cell::Cell;

use hotsheet_model::Ticket;
use hotsheet_ticketing::orchestration::{
    ApprovalGate, ApprovalPolicy, ApprovalState, AuditOutcome, CreateGrant, MachineLimits,
    NewSession, OrchestratedSession, PlacementAction, PlacementPolicy, PolicyBody, ReadinessLevel,
    RunState, ScheduledSession, SchedulerStore, SessionActivity, SessionIdleStore, SessionLoad,
    SessionOwner, SessionRegistry, SessionRole, SessionScope, SessionState, SetPolicy,
    TargetAction, TargetPolicy, ToolPolicy, ToolScope, WindDown,
};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::json;

use super::sessions::{Probe, ScriptedHost, roomy};
use super::tickets::{Machine, NOW, bind_and_grant, human, machine, unsupported, worker};
use super::*;

const MINUTE: u64 = 60_000;
const START: u64 = 1_000;

/// A clock the test moves.
struct At(Cell<u64>);
impl hotsheet_ticketing::Clock for At {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}

fn scope(tool: &str) -> ToolScope {
    ToolScope {
        tool: tool.into(),
        models: Vec::new(),
    }
}

/// Target 1 takes two `codex` sessions. Placement is automatic as `worker-1`, with no
/// cooldown and the shortest idle time, so a test decides every wait itself.
fn policy(required_for: Vec<ApprovalGate>) -> PolicyBody {
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 4,
            ..MachineLimits::default()
        },
        tools: vec![ToolPolicy {
            tool: "codex".into(),
            max_sessions: 4,
            weight: 50,
            accounts: Vec::new(),
        }],
        targets: vec![TargetPolicy {
            target_id: id(1),
            enabled: true,
            actions: vec![
                TargetAction::TicketsRead,
                TargetAction::GatesObserve,
                TargetAction::SessionsManage,
                TargetAction::WorkStart,
            ],
            tools: vec![scope("codex")],
            max_sessions: 2,
            machine: None,
            speculation: None,
        }],
        wind_down: WindDown {
            idle_minutes: 1,
            keep_warm: 0,
        },
        approval: ApprovalPolicy { required_for },
        placement: PlacementPolicy {
            automatic: true,
            actor: Some("worker-1".into()),
            cooldown_minutes: 0,
            ..PlacementPolicy::default()
        },
        ..PolicyBody::default()
    }
}

struct Placed {
    m: Machine,
    host: Arc<ScriptedHost>,
    probe: Arc<Probe>,
    orchestration: Ticket,
}

impl Placed {
    /// One machine that hosts target 1 in a clean git checkout, a roomy probe, a
    /// scripted terminal host and the real executor. `worker-1` holds a long grant.
    fn new(required_for: Vec<ApprovalGate>, tx: &mut Transaction<'_>) -> Self {
        let mut m = machine(10, false, unsupported);
        let host = Arc::new(ScriptedHost::default());
        let probe = Arc::new(Probe::default());
        probe.set(|observation| *observation = roomy());
        m.runtime = m
            .runtime
            .clone()
            .with_capacity_probe(probe.clone())
            .with_session_host(host.clone())
            .with_session_executor(Arc::new(OwnedSessions));
        bind_and_grant(&m, 1, &[], tx);
        m.runtime
            .grant(
                &m.reference(1),
                CreateGrant {
                    grant_id: None,
                    actor_id: "worker-1".into(),
                    actions: vec![
                        TargetAction::TicketsRead,
                        TargetAction::GatesObserve,
                        TargetAction::WorkStart,
                        TargetAction::SessionsManage,
                    ],
                    session_scope: Some(SessionScope {
                        tools: vec![scope("codex")],
                        max_concurrency: 8,
                    }),
                    expires_at_ms: 1_000_000_000,
                },
                Some(&human()),
                tx,
            )
            .unwrap();
        let orchestration = m.orchestration_ticket("Coordinate");
        let placed = Self {
            m,
            host,
            probe,
            orchestration,
        };
        // Whether work is uncommitted can only be established in a git checkout.
        placed.git(&["init", "-q"]);
        placed.set_policy(policy(required_for), tx);
        placed
    }
    fn owner(&self) -> String {
        self.m.owner.id.clone()
    }
    fn git(&self, args: &[&str]) {
        let status = hotsheet_ticketing::git::launch("git")
            .arg("-C")
            .arg(&self.m.target.root)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "{args:?}");
    }
    fn set_policy(&self, body: PolicyBody, tx: &mut Transaction<'_>) {
        let expected_revision = self.m.runtime.policy(&self.owner()).unwrap().revision;
        self.m
            .runtime
            .set_policy(
                &self.owner(),
                SetPolicy {
                    expected_revision,
                    policy: body,
                },
                Some(&human()),
                tx,
            )
            .unwrap();
    }
    /// Move the clock, and the probe's reading with it so capacity stays fresh.
    fn at(&self, clock: &At, now_ms: u64) {
        clock.0.set(now_ms);
        self.probe
            .set(|observation| observation.sampled_at_ms = Some(now_ms));
    }
    /// A new target ticket planned as a step with no prerequisite: confirmed work.
    fn ready(&self, title: &str, tx: &mut Transaction<'_>) -> String {
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
        self.m
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
            .step_id
    }
    /// HS2-YC2P7K: the latest run the scheduler dispatched for a step.
    fn run_of(&self, step_id: &str) -> hotsheet_ticketing::orchestration::Run {
        self.m
            .runtime
            .run_list(&self.owner(), false)
            .unwrap()
            .into_iter()
            .filter(|run| {
                run.operation_id
                    .starts_with(&format!("sched-run-{step_id}"))
            })
            .max_by_key(|run| {
                (
                    run.created_at_ms,
                    run.operation_id.len(),
                    run.operation_id.clone(),
                )
            })
            .unwrap_or_else(|| panic!("no run for {step_id}"))
    }
    /// The work is withdrawn: its run is cancelled (HS2-YC2P7K) and its step removed
    /// from the plan.
    fn gone(&self, step_id: &str, tx: &mut Transaction<'_>) {
        for run in self.m.runtime.run_list(&self.owner(), true).unwrap() {
            if run
                .operation_id
                .starts_with(&format!("sched-run-{step_id}"))
            {
                self.m
                    .runtime
                    .run_cancel(&self.owner(), &run.run_id, Some(&worker()), tx)
                    .unwrap();
            }
        }
        self.m
            .runtime
            .readiness_remove(
                &self.owner(),
                &serde_json::from_value(json!({"step_id": step_id})).unwrap(),
                Some(&worker()),
                tx,
            )
            .unwrap();
    }
    fn tick(&self, tx: &mut Transaction<'_>) -> TickView {
        self.m
            .runtime
            .scheduler_tick(&self.owner(), &TickRequest::default(), Some(&worker()), tx)
            .unwrap()
    }
    fn records(&self) -> Vec<OrchestratedSession> {
        self.m.runtime.session_list(&self.owner(), false).unwrap()
    }
    fn record(&self, operation: &str) -> OrchestratedSession {
        self.records()
            .into_iter()
            .find(|session| session.operation_id == operation)
            .unwrap_or_else(|| panic!("no session for {operation}"))
    }
    fn listed(&self, tx: &mut Transaction<'_>) -> Vec<ScheduledSession> {
        OwnedSessions
            .list(&self.m.runtime, &self.owner(), tx)
            .unwrap()
    }
    fn start_request(&self, step_id: &str, load: SessionLoad) -> PlacementStart {
        PlacementStart {
            owner_id: self.owner(),
            target_id: id(1),
            tool: "codex".into(),
            account: None,
            model: None,
            operation_id: format!("sched-start-{step_id}"),
            actor_id: "worker-1".into(),
            steps: vec![step_id.into()],
            load,
            limits: MachineLimits::default(),
            probe: false,
            resources: Vec::new(),
        }
    }
    fn stop_request(&self, session_id: &str) -> WindDownSession {
        WindDownSession {
            owner_id: self.owner(),
            target_id: id(1),
            session_id: session_id.into(),
            tool: "codex".into(),
            model: None,
            operation_id: format!("sched-stop-{session_id}"),
            actor_id: "worker-1".into(),
            reason: "idle".into(),
        }
    }
    fn terminal(&self, session: &OrchestratedSession) -> String {
        session.hosted.as_ref().unwrap().session_id.clone()
    }
    fn wake_token(&self) -> Option<String> {
        std::fs::read_to_string(self.m.home.join("orchestration-scheduler-wake/token")).ok()
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
}

fn no_load() -> SessionLoad {
    SessionLoad {
        target_sessions: 0,
        machine_sessions: 0,
        tool_sessions: 0,
        account: None,
        account_sessions: 0,
    }
}

fn outcomes(view: &TickView) -> Vec<(&str, Option<&str>)> {
    view.outcomes
        .iter()
        .map(|outcome| (outcome.outcome.as_str(), outcome.detail.as_deref()))
        .collect()
}

/// `(action, reason)` of the placement that names the session.
fn placement(view: &TickView, session_id: &str) -> (PlacementAction, &'static str) {
    let found = view
        .view
        .schedule
        .placements
        .iter()
        .find(|placement| placement.session_id.as_deref() == Some(session_id))
        .unwrap_or_else(|| panic!("no placement for {session_id}"));
    (found.action, found.reason.code())
}

macro_rules! clocked {
    ($clock:ident, $tx:ident) => {
        let $clock = At(Cell::new(START));
        let mut rng = Rng(100);
        let mut $tx = Transaction {
            clock: &$clock,
            rng: &mut rng,
        };
    };
}

#[test]
fn hs2_95swq4_an_automatic_pass_starts_one_session_and_later_passes_keep_it() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    let operation = format!("sched-start-{step}");
    // Nothing runs yet: the executor answers, with nothing to list.
    assert!(f.listed(&mut tx).is_empty());
    let before = f.wake_token();

    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    assert_eq!(tick.view.inputs.executor, "available");
    let session = f.record(&operation);
    assert_eq!(
        tick.outcomes[0].session_id.as_deref(),
        Some(session.session_id.as_str())
    );
    // HS2-YC2P7K: the step's run was dispatched for the target's dispatcher and the
    // session works it.
    let run = f.run_of(&step);
    assert_eq!(run.operation_id, format!("sched-run-{step}"));
    assert_eq!(
        (run.mode, run.state),
        (
            hotsheet_ticketing::orchestration::DispatchMode::Dispatcher,
            hotsheet_ticketing::orchestration::RunState::Queued
        )
    );
    assert_eq!(
        tick.outcomes[0].run_id.as_deref(),
        Some(run.run_id.as_str())
    );
    assert_eq!(
        (
            session.state,
            session.role,
            session.tool.as_str(),
            &session.run_id
        ),
        (
            SessionState::Running,
            SessionRole::Worker,
            "codex",
            &Some(run.run_id.clone())
        )
    );
    assert_eq!(f.host.launches().len(), 1);
    // The gate was passed under the scheduler's operation id, as the system acting
    // for the policy's actor.
    assert!(f.audited("sessions_manage").contains(&(
        AuditOutcome::Allowed,
        None,
        Some(operation.clone())
    )));
    assert_eq!(
        f.audited("scheduler_start"),
        [(
            AuditOutcome::Succeeded,
            Some("started".into()),
            Some(operation.clone())
        )]
    );
    // The step is in progress: it left the ready work.
    let _ = before;
    assert_eq!(
        f.m.runtime
            .readiness(&f.owner(), &step, &mut tx)
            .unwrap()
            .readiness
            .level,
        hotsheet_ticketing::orchestration::ReadinessLevel::InProgress
    );

    // Later passes find the session working its run and start nothing more.
    for _ in 0..3 {
        let again = f.tick(&mut tx);
        assert!(again.outcomes.is_empty());
        assert!(again.view.schedule.unplaced.is_empty());
        assert_eq!(
            placement(&again, &session.session_id),
            (PlacementAction::Keep, "busy")
        );
    }
    assert_eq!(f.m.runtime.run_list(&f.owner(), false).unwrap().len(), 1);
    assert_eq!(f.host.launches().len(), 1);
    let listed = f.listed(&mut tx);
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (
            listed[0].session_id.as_str(),
            &listed[0].run_id,
            listed[0].owner_id.as_deref(),
            listed[0].activity,
            listed[0].safe_point,
            listed[0].human_takeover,
            listed[0].winding_down,
            listed[0].started_at_ms,
        ),
        (
            session.session_id.as_str(),
            &Some(run.run_id.clone()),
            Some(f.owner().as_str()),
            // Between turns, but its run is unfinished: it is working.
            SessionActivity::Busy,
            true,
            false,
            false,
            START
        )
    );
    assert!(listed[0].validate().is_ok());
}

#[test]
fn hs2_95swq4_a_repeated_operation_id_is_the_same_start() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    // HS2-NPTKS8: the session never joins its role; it is given longer than this
    // test runs, so it is not replaced.
    let mut patient = policy(Vec::new());
    patient.placement.join_minutes = 60;
    f.set_policy(patient, &mut tx);
    let step = f.ready("First", &mut tx);
    let request = f.start_request(&step, no_load());
    let first = OwnedSessions.start(&f.m.runtime, &request, &mut tx);
    let StartOutcome::Started { session_id, .. } = &first else {
        panic!("{first:?}");
    };
    // Asked again, twice, and after time has passed: the same session, one launch.
    for now in [START, START + 5, START + 10 * MINUTE] {
        f.at(&clock, now);
        assert_eq!(OwnedSessions.start(&f.m.runtime, &request, &mut tx), first);
    }
    assert_eq!(f.host.launches().len(), 1);
    assert_eq!(f.records().len(), 1);
    assert_eq!(&f.records()[0].session_id, session_id);

    // A start left unconfirmed by a process that died is repeated under its own id.
    let other = f.ready("Second", &mut tx);
    let (planted, _) = SessionRegistry::new(&f.m.home)
        .begin(
            &f.owner(),
            NewSession {
                operation_id: format!("sched-start-{other}"),
                target_id: id(1),
                role: SessionRole::Worker,
                tool: "codex".into(),
                model: None,
                effort: None,
                account: None,
                run_id: None,
                replaces: None,
            },
            Some("worker-1".into()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(planted.state, SessionState::Requested);
    // It is not a session yet, so it is not listed.
    assert_eq!(f.listed(&mut tx).len(), 1);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    assert_eq!(
        tick.outcomes[0].session_id.as_deref(),
        Some(planted.session_id.as_str())
    );
    assert_eq!(f.records().len(), 2);
    assert_eq!(f.host.launches().len(), 2);

    // An actor that cannot be identified is not acted for.
    let mut anonymous = f.start_request(&step, no_load());
    anonymous.actor_id = " ".into();
    assert_eq!(
        OwnedSessions.start(&f.m.runtime, &anonymous, &mut tx),
        StartOutcome::Unavailable
    );
}

#[test]
fn hs2_95swq4_the_gate_sees_the_larger_of_the_decided_and_the_recorded_load() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    // The scheduler counted two sessions on the target that these records do not
    // hold (a person's, another owner's): the target's cap of two refuses the start.
    let crowded = SessionLoad {
        target_sessions: 2,
        machine_sessions: 2,
        tool_sessions: 2,
        account: None,
        account_sessions: 0,
    };
    assert_eq!(
        OwnedSessions.start(&f.m.runtime, &f.start_request(&step, crowded), &mut tx),
        StartOutcome::Refused {
            reason: "policy_denied".into()
        }
    );
    assert!(f.records().is_empty() && f.host.launches().is_empty());
    assert!(f.audited("sessions_manage").contains(&(
        AuditOutcome::Denied,
        Some("policy_target_cap".into()),
        Some(format!("sched-start-{step}"))
    )));

    // The records hold two sessions the scheduler did not count: started since its
    // listing, here directly. Its stale count of none does not undercut them.
    for operation in ["direct-1", "direct-2"] {
        f.m.runtime
            .session_start(
                &f.owner(),
                serde_json::from_value(
                    json!({"target_id": id(1), "operation_id": operation, "tool": "codex"}),
                )
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    }
    assert_eq!(
        OwnedSessions.start(&f.m.runtime, &f.start_request(&step, no_load()), &mut tx),
        StartOutcome::Refused {
            reason: "policy_denied".into()
        }
    );
    assert_eq!(f.host.launches().len(), 2);
    // And the pass itself, counting from the list, starts nothing for the step: a
    // free session of the target takes it, and with it the step's run (HS2-YC2P7K).
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("assigned", None)]);
    assert_eq!(f.host.launches().len(), 2);
    let run = f.run_of(&step);
    assert_eq!(
        tick.outcomes[0].run_id.as_deref(),
        Some(run.run_id.as_str())
    );
    assert_eq!(
        f.records()
            .iter()
            .filter(|session| session.run_id.as_deref() == Some(run.run_id.as_str()))
            .count(),
        1
    );
    // Repeated passes assign nothing more: the session works its run now.
    assert!(f.tick(&mut tx).outcomes.is_empty());
    assert_eq!(f.m.runtime.run_list(&f.owner(), false).unwrap().len(), 1);
    let _ = clock;
}

#[test]
fn hs2_95swq4_a_refused_start_is_typed_backed_off_and_then_replaced() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    let operation = format!("sched-start-{step}");
    // The host will not launch the tool.
    f.host.refusing.store(true, Ordering::SeqCst);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("refused", Some("session_start_failed"))]);
    let failed = f.record(&operation);
    assert_eq!(failed.state, SessionState::Failed);
    assert!(f.listed(&mut tx).is_empty());
    let memory = SchedulerStore::new(&f.m.home).memory(&f.owner()).unwrap();
    assert_eq!(
        (memory[0].refusals, memory[0].refusal_reason.as_deref()),
        (1, Some("session_start_failed"))
    );
    // While the refusal is waited out nothing is asked, however often a pass runs.
    f.host.refusing.store(false, Ordering::SeqCst);
    for _ in 0..2 {
        let waiting = f.tick(&mut tx);
        assert!(waiting.outcomes.is_empty());
        assert_eq!(
            waiting.view.schedule.unplaced[0].reason.code(),
            "refusal_backoff"
        );
    }
    assert!(f.host.launches().is_empty());

    // After the backoff the work gets a new session that replaces the failed one;
    // the failed one is never restarted.
    f.at(&clock, START + 2 * MINUTE);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    let replacement = f.record(&format!("{operation}.2"));
    assert_eq!(replacement.state, SessionState::Running);
    assert_eq!(
        replacement.replaces.as_deref(),
        Some(failed.session_id.as_str())
    );
    assert_eq!(f.record(&operation).state, SessionState::Failed);
    assert_eq!(f.host.launches().len(), 1);
    assert_eq!(
        f.audited("scheduler_start")
            .into_iter()
            .map(|(outcome, reason, _)| (outcome, reason))
            .collect::<Vec<_>>(),
        [
            (AuditOutcome::Failed, Some("refused".into())),
            (AuditOutcome::Succeeded, Some("started".into()))
        ]
    );

    // A terminal host that cannot be reached is a refusal with its own code.
    let other = f.ready("Second", &mut tx);
    f.host.down.store(true, Ordering::SeqCst);
    let outcome = OwnedSessions.start(&f.m.runtime, &f.start_request(&other, no_load()), &mut tx);
    assert_eq!(
        outcome,
        StartOutcome::Refused {
            reason: "session_host_unavailable".into()
        }
    );
}

#[test]
fn hs2_95swq4_an_approval_given_for_the_schedulers_operation_lets_the_act_through() {
    clocked!(clock, tx);
    let f = Placed::new(
        vec![ApprovalGate::SessionStart, ApprovalGate::SessionStop],
        &mut tx,
    );
    let owner = f.owner();
    let step = f.ready("First", &mut tx);
    let operation = format!("sched-start-{step}");
    let approve = |gate: ApprovalGate, tx: &mut Transaction<'_>| {
        let pending: Vec<_> = f
            .m
            .runtime
            .approvals(&owner, tx)
            .unwrap()
            .into_iter()
            .filter(|approval| approval.gate == gate && approval.state == ApprovalState::Pending)
            .collect();
        assert_eq!(pending.len(), 1, "{gate:?}");
        f.m.runtime
            .decide_approval(&owner, &pending[0].approval_id, true, Some(&human()), tx)
            .unwrap();
        pending[0].operation_id.clone()
    };
    // Asked once, however often a pass runs; nothing starts meanwhile.
    for _ in 0..2 {
        assert_eq!(
            outcomes(&f.tick(&mut tx)),
            [("approval_required", Some("session_start"))]
        );
    }
    assert!(f.records().is_empty() && f.host.launches().is_empty());
    assert_eq!(approve(ApprovalGate::SessionStart, &mut tx), operation);
    // The executor's own gate uses the approval the person gave for that id.
    assert_eq!(outcomes(&f.tick(&mut tx)), [("started", None)]);
    let session = f.record(&operation);
    let approvals = f.m.runtime.approvals(&owner, &mut tx).unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].state, ApprovalState::Consumed);

    // The work goes away and the session idles out: the stop asks a person too.
    f.gone(&step, &mut tx);
    f.tick(&mut tx);
    f.at(&clock, START + 2 * MINUTE);
    for _ in 0..2 {
        assert_eq!(
            outcomes(&f.tick(&mut tx)),
            [("approval_required", Some("session_stop"))]
        );
    }
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);
    assert_eq!(
        approve(ApprovalGate::SessionStop, &mut tx),
        format!("sched-stop-{}", session.session_id)
    );
    assert_eq!(outcomes(&f.tick(&mut tx)), [("wind_down_requested", None)]);
    assert_eq!(f.record(&operation).state, SessionState::Stopped);
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 1);
    // One request per act was ever filed.
    assert_eq!(f.m.runtime.approvals(&owner, &mut tx).unwrap().len(), 2);
}

#[test]
fn hs2_95swq4_an_idle_session_is_wound_down_only_at_a_safe_point() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    let operation = format!("sched-start-{step}");
    f.tick(&mut tx);
    let session = f.record(&operation);
    let sid = session.session_id.clone();
    let terminal = f.terminal(&session);

    // The work is gone. The session was first seen idle now, so its idle time starts.
    f.gone(&step, &mut tx);
    let tick = f.tick(&mut tx);
    assert!(tick.outcomes.is_empty());
    assert_eq!(
        placement(&tick, &sid),
        (PlacementAction::Keep, "idle_grace")
    );
    assert_eq!(
        tick.view.schedule.retry_at_ms,
        Some(START + MINUTE),
        "the pass names when the idle time ends"
    );
    assert_eq!(f.listed(&mut tx)[0].idle_since_ms, Some(START));

    // Past the idle time, in the middle of a turn: busy, and nothing is asked.
    f.at(&clock, START + 2 * MINUTE);
    f.host.set_busy(&terminal, true);
    let tick = f.tick(&mut tx);
    assert!(tick.outcomes.is_empty());
    assert_eq!(placement(&tick, &sid), (PlacementAction::Keep, "busy"));
    assert_eq!(f.listed(&mut tx)[0].idle_since_ms, None);
    // A wind-down that arrives then is refused by the host and stops nothing.
    assert_eq!(
        OwnedSessions.wind_down(&f.m.runtime, &f.stop_request(&sid), &mut tx),
        WindDownOutcome::NotAtSafePoint
    );
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);
    assert_eq!(f.record(&operation).state, SessionState::Running);

    // Between turns again: the idle time starts afresh from this sighting.
    f.host.set_busy(&terminal, false);
    let tick = f.tick(&mut tx);
    assert_eq!(
        placement(&tick, &sid),
        (PlacementAction::Keep, "idle_grace")
    );
    assert_eq!(f.listed(&mut tx)[0].idle_since_ms, Some(START + 2 * MINUTE));

    // Idle long enough, with uncommitted work in the checkout: it waits.
    f.at(&clock, START + 4 * MINUTE);
    std::fs::write(Path::new(&f.m.target.root).join("draft.txt"), "synthetic").unwrap();
    let tick = f.tick(&mut tx);
    assert!(tick.outcomes.is_empty());
    assert_eq!(
        placement(&tick, &sid),
        (PlacementAction::Keep, "awaiting_safe_point")
    );
    assert_eq!(
        OwnedSessions.wind_down(&f.m.runtime, &f.stop_request(&sid), &mut tx),
        WindDownOutcome::NotAtSafePoint
    );
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);

    // Committed: it is at a safe point and is wound down, once.
    f.git(&["add", "-A"]);
    f.git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@example.test",
        "commit",
        "-q",
        "-m",
        "Synthetic work",
    ]);
    let before = f.wake_token();
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("wind_down_requested", None)]);
    assert_eq!(placement(&tick, &sid), (PlacementAction::WindDown, "idle"));
    let stopped = f.record(&operation);
    assert_eq!(stopped.state, SessionState::Stopped);
    assert_eq!(
        stopped.history.last().unwrap().reason.as_deref(),
        Some("wound_down")
    );
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 1);
    // Its run was cancelled with the work; its stop is published on that run.
    let _ = before;
    let events =
        f.m.runtime
            .run_events(&f.owner(), 0, 200, session.run_id.as_deref(), 0)
            .unwrap();
    assert!(
        events
            .entries
            .iter()
            .any(|entry| entry.record.reason.as_deref() == Some("session_stopped")),
        "its exit is on its run's stream"
    );
    assert!(f.audited("sessions_manage").contains(&(
        AuditOutcome::Allowed,
        None,
        Some(format!("sched-stop-{sid}"))
    )));
    // It is gone: nothing is listed, remembered or asked again.
    assert!(f.listed(&mut tx).is_empty());
    assert!(
        SessionIdleStore::new(&f.m.home)
            .idle_since(&f.owner())
            .unwrap()
            .is_empty()
    );
    assert!(f.tick(&mut tx).outcomes.is_empty());
    assert_eq!(
        OwnedSessions.wind_down(&f.m.runtime, &f.stop_request(&sid), &mut tx),
        WindDownOutcome::NotFound
    );
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 1);
}

#[test]
fn hs2_95swq4_a_session_a_person_took_over_is_reported_and_never_wound_down() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let owner = f.owner();
    let first = f.ready("First", &mut tx);
    let second = f.ready("Second", &mut tx);
    assert_eq!(f.tick(&mut tx).outcomes.len(), 2);
    let here = f.record(&format!("sched-start-{first}"));
    let there = f.record(&format!("sched-start-{second}"));

    // One is taken over on the orchestrating machine, the other at its host.
    f.m.runtime
        .session_takeover(
            &owner,
            &here.session_id,
            TakeoverSession { pause: false },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    f.m.runtime
        .hosted_session_control(
            &f.m.target.id,
            HostedSessionControl {
                operation_id: there.operation_id.clone(),
                pause: true,
                actor: Some(human()),
            },
            &mut tx,
        )
        .unwrap();
    let listed = f.listed(&mut tx);
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|session| session.human_takeover));
    // The listing learned the host-side takeover and recorded it.
    assert_eq!(
        f.record(&there.operation_id).owner,
        SessionOwner::Human,
        "learned from the host"
    );

    // No work, idle far past the idle time, at a safe point: both are left alone.
    f.gone(&first, &mut tx);
    f.gone(&second, &mut tx);
    f.tick(&mut tx);
    f.at(&clock, START + 30 * MINUTE);
    for _ in 0..2 {
        let tick = f.tick(&mut tx);
        assert!(tick.outcomes.is_empty());
        for session in [&here, &there] {
            assert_eq!(
                placement(&tick, &session.session_id),
                (PlacementAction::Keep, "human_takeover")
            );
        }
    }
    // Asked directly, the executor refuses without asking the host anything.
    for session in [&here, &there] {
        assert_eq!(
            OwnedSessions.wind_down(&f.m.runtime, &f.stop_request(&session.session_id), &mut tx),
            WindDownOutcome::Refused {
                reason: "session_human_owned".into()
            }
        );
    }
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);
    // They still occupy the target: with its cap of two, new work gets no session.
    f.ready("Third", &mut tx);
    let tick = f.tick(&mut tx);
    assert!(tick.outcomes.is_empty());
    assert_eq!(f.host.launches().len(), 2);
}

#[test]
fn hs2_95swq4_a_crashed_session_is_lost_and_replaced_each_time() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    let operation = format!("sched-start-{step}");
    f.tick(&mut tx);
    let mut previous = f.record(&operation);
    // Crash, twice over: each pass learns it from the host and starts a replacement
    // that names the session it replaces. Nothing is restarted in place.
    for generation in [2u32, 3] {
        f.at(&clock, START + u64::from(generation) * MINUTE);
        f.host.crash(&f.terminal(&previous));
        let before = f.wake_token();
        let tick = f.tick(&mut tx);
        assert_eq!(outcomes(&tick), [("started", None)]);
        let lost = f.record(&previous.operation_id);
        assert_eq!(lost.state, SessionState::Lost);
        assert_eq!(
            lost.history.last().unwrap().reason.as_deref(),
            Some("host_lost")
        );
        // HS2-YC2P7K: the loss is on the run's stream, and the replacement works
        // the same run: nothing is dispatched twice.
        let _ = before;
        let run = f.run_of(&step);
        assert_eq!(f.m.runtime.run_list(&f.owner(), false).unwrap().len(), 1);
        assert!(
            f.m.runtime
                .run_events(&f.owner(), 0, 500, Some(&run.run_id), 0)
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.record.reason.as_deref() == Some("session_lost"))
        );
        let replacement = f.record(&format!("{operation}.{generation}"));
        assert_eq!(replacement.run_id.as_deref(), Some(run.run_id.as_str()));
        assert_eq!(replacement.state, SessionState::Running);
        assert_eq!(
            replacement.replaces.as_deref(),
            Some(previous.session_id.as_str())
        );
        assert_ne!(f.terminal(&replacement), f.terminal(&previous));
        let listed = f.listed(&mut tx);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, replacement.session_id);
        previous = replacement;
    }
    assert_eq!(f.host.launches().len(), 3);
    assert_eq!(f.records().len(), 3);

    // A crash with the work gone is only recorded: nothing replaces it.
    f.gone(&step, &mut tx);
    f.host.crash(&f.terminal(&previous));
    let tick = f.tick(&mut tx);
    assert!(tick.outcomes.is_empty());
    assert!(tick.view.schedule.placements.is_empty());
    assert_eq!(f.record(&previous.operation_id).state, SessionState::Lost);
    assert_eq!(f.host.launches().len(), 3);
    // A wind-down for a session that is gone finds nothing.
    assert_eq!(
        OwnedSessions.wind_down(&f.m.runtime, &f.stop_request(&previous.session_id), &mut tx),
        WindDownOutcome::NotFound
    );
}

#[test]
fn hs2_95swq4_an_unreachable_host_concludes_nothing_and_stale_starts_are_ended() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    let operation = format!("sched-start-{step}");
    f.tick(&mut tx);
    let session = f.record(&operation);
    f.gone(&step, &mut tx);
    f.tick(&mut tx);
    assert_eq!(f.listed(&mut tx)[0].idle_since_ms, Some(START));

    // The terminal host cannot be asked: the session still counts, reads as busy and
    // unsafe, keeps the idle time already seen, and is not wound down on a guess.
    f.host.down.store(true, Ordering::SeqCst);
    f.at(&clock, START + 10 * MINUTE);
    let listed = f.listed(&mut tx);
    assert_eq!(
        (listed[0].activity, listed[0].safe_point),
        (SessionActivity::Busy, false)
    );
    let tick = f.tick(&mut tx);
    assert!(tick.outcomes.is_empty());
    assert_eq!(
        placement(&tick, &session.session_id),
        (PlacementAction::Keep, "busy")
    );
    assert_eq!(f.record(&operation).state, SessionState::Running);
    assert_eq!(
        OwnedSessions.wind_down(&f.m.runtime, &f.stop_request(&session.session_id), &mut tx),
        WindDownOutcome::NotAtSafePoint
    );
    // Reachable again: the idle time it was first seen with still stands.
    f.host.down.store(false, Ordering::SeqCst);
    assert_eq!(f.listed(&mut tx)[0].idle_since_ms, Some(START));
    assert_eq!(outcomes(&f.tick(&mut tx)), [("wind_down_requested", None)]);

    // A start nobody confirmed and nothing asks for any more: left alone while it
    // may still be on its way, then ended as never started. It never ran.
    let begun = clock.0.get();
    let (orphan, _) = SessionRegistry::new(&f.m.home)
        .begin(
            &f.owner(),
            NewSession {
                operation_id: "sched-start-orphan".into(),
                target_id: id(1),
                role: SessionRole::Worker,
                tool: "codex".into(),
                model: None,
                effort: None,
                account: None,
                run_id: None,
                replaces: None,
            },
            Some("worker-1".into()),
            &mut tx,
        )
        .unwrap();
    f.at(&clock, begun + UNCONFIRMED_START_MS - 1);
    assert!(f.listed(&mut tx).is_empty());
    assert_eq!(
        f.record("sched-start-orphan").state,
        SessionState::Requested
    );
    f.at(&clock, begun + UNCONFIRMED_START_MS);
    assert!(f.listed(&mut tx).is_empty());
    let ended = f.record("sched-start-orphan");
    assert_eq!(ended.session_id, orphan.session_id);
    assert_eq!(ended.state, SessionState::Stopped);
    assert_eq!(
        ended.history.last().unwrap().reason.as_deref(),
        Some("never_started")
    );
    assert_eq!(f.host.launches().len(), 1, "it was never launched");
}

#[test]
fn hs2_95swq4_without_a_placement_actor_no_host_is_asked_and_nothing_reads_safe() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    f.tick(&mut tx);
    let session = f.record(&format!("sched-start-{step}"));
    // The policy no longer enables placement and names no actor.
    let mut manual = policy(Vec::new());
    manual.placement = PlacementPolicy::default();
    f.set_policy(manual, &mut tx);
    f.gone(&step, &mut tx);
    f.host.crash(&f.terminal(&session));
    f.at(&clock, START + 30 * MINUTE);
    let reads = f.audited("sessions_manage").len();
    let listed = f.listed(&mut tx);
    // The record is listed as it stands: the crash is not learned, nothing is safe.
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (
            listed[0].activity,
            listed[0].safe_point,
            listed[0].idle_since_ms
        ),
        (SessionActivity::Busy, false, None)
    );
    assert_eq!(f.audited("sessions_manage").len(), reads);
    // Every pass is a dry run, and it starts and stops nothing.
    let tick = f.tick(&mut tx);
    assert!(tick.outcomes.is_empty());
    assert_eq!(f.record(&session.operation_id).state, SessionState::Running);
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);
}

#[test]
fn hs2_95swq4_a_session_the_policy_no_longer_permits_is_kept_for_a_person_to_stop() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    f.tick(&mut tx);
    let session = f.record(&format!("sched-start-{step}"));
    // The policy disables the target. The policy's gate then denies every act there,
    // a read of the session's safe point included (HS2-48H57C), so nothing reads as
    // safe: the session is kept until a person stops it, and is not stopped unseen.
    let mut disabled = policy(Vec::new());
    disabled.targets[0].enabled = false;
    f.set_policy(disabled, &mut tx);
    let tick = f.tick(&mut tx);
    assert!(tick.outcomes.is_empty());
    assert_eq!(
        placement(&tick, &session.session_id),
        (PlacementAction::Keep, "awaiting_safe_point")
    );
    // Asked directly, the stop is a typed refusal of the gate.
    assert_eq!(
        OwnedSessions.wind_down(&f.m.runtime, &f.stop_request(&session.session_id), &mut tx),
        WindDownOutcome::Refused {
            reason: "policy_denied".into()
        }
    );
    assert_eq!(f.record(&session.operation_id).state, SessionState::Running);
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);
    // A person can still stop it.
    f.m.runtime
        .session_stop(
            &f.owner(),
            &session.session_id,
            StopSession { force: false },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 1);
    let _ = clock;
}

impl Placed {
    /// HS2-YC2P7K: the target ticket of a step is finished, and its run learns it.
    fn finish(&self, step_id: &str, tx: &mut Transaction<'_>) {
        let run = self.run_of(step_id);
        let ticket = ops::resolve(&self.m.target_store, &run.target.native_id)
            .unwrap()
            .unwrap();
        ops::update(
            &self.m.target_store,
            &ticket.id,
            NOW.into(),
            hotsheet_ticketing::TicketPatch {
                status: Some(hotsheet_model::Status::Completed),
                ..Default::default()
            },
        )
        .unwrap();
        self.m
            .runtime
            .run_sync(&self.owner(), &run.run_id, Some(&worker()), tx)
            .unwrap();
    }
    fn level(&self, step_id: &str, tx: &mut Transaction<'_>) -> ReadinessLevel {
        self.m
            .runtime
            .readiness(&self.owner(), step_id, tx)
            .unwrap()
            .readiness
            .level
    }
}

/// HS2-YC2P7K: a start whose session is refused leaves its run queued. The step is
/// in progress, so nothing new is dispatched; once the refusal is waited out a
/// session starts for that same run.
#[test]
fn hs2_yc2p7k_a_refused_session_leaves_its_run_and_a_later_session_takes_it() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    f.host.refusing.store(true, Ordering::SeqCst);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("refused", Some("session_start_failed"))]);
    let run = f.run_of(&step);
    assert_eq!(run.state, RunState::Queued);
    assert_eq!(f.level(&step, &mut tx), ReadinessLevel::InProgress);
    // Waiting out the refusal: the orphaned run is work, deferred like any other.
    f.host.refusing.store(false, Ordering::SeqCst);
    let waiting = f.tick(&mut tx);
    assert!(waiting.outcomes.is_empty());
    assert_eq!(waiting.view.schedule.unplaced[0].step_id, step);
    assert_eq!(
        waiting.view.schedule.unplaced[0].reason.code(),
        "refusal_backoff"
    );
    f.at(&clock, START + 2 * MINUTE);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    assert_eq!(
        tick.outcomes[0].run_id.as_deref(),
        Some(run.run_id.as_str())
    );
    let session = f.record(&format!("sched-start-{step}.2"));
    assert_eq!(session.run_id.as_deref(), Some(run.run_id.as_str()));
    // One run in all, and repeated passes add nothing.
    for _ in 0..3 {
        assert!(f.tick(&mut tx).outcomes.is_empty());
    }
    assert_eq!(f.m.runtime.run_list(&f.owner(), false).unwrap().len(), 1);
    assert_eq!(f.host.launches().len(), 1);
}

/// HS2-YC2P7K: placing work is starting work. Without `work_start` in the grant or
/// the policy nothing is dispatched and no session starts.
#[test]
fn hs2_yc2p7k_placing_work_needs_work_start() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    let mut body = policy(Vec::new());
    body.targets[0]
        .actions
        .retain(|action| *action != TargetAction::WorkStart);
    f.set_policy(body, &mut tx);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("refused", Some("policy_denied"))]);
    assert!(f.m.runtime.run_list(&f.owner(), false).unwrap().is_empty());
    assert!(f.records().is_empty() && f.host.launches().is_empty());
    assert_eq!(f.level(&step, &mut tx), ReadinessLevel::Confirmed);
    let _ = clock;
}

/// HS2-YC2P7K: when a step's run finishes, its session is free: it is given the next
/// ready step's run, and the finished step is never dispatched again. A run that
/// failed is replaced by the next generation.
#[test]
fn hs2_yc2p7k_a_free_session_is_given_the_next_run_and_a_failed_run_is_replaced() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let first = f.ready("First", &mut tx);
    f.tick(&mut tx);
    let session = f.record(&format!("sched-start-{first}"));
    let first_run = f.run_of(&first);
    f.finish(&first, &mut tx);
    assert_eq!(f.run_of(&first).state, RunState::Completed);
    assert_eq!(f.level(&first, &mut tx), ReadinessLevel::Finished);
    // Free now: listed without a run and idle between turns.
    let listed = f.listed(&mut tx);
    assert_eq!(
        (listed[0].run_id.as_ref(), listed[0].activity),
        (None, SessionActivity::Idle)
    );

    // New work: the free session takes it, with its run; no session starts.
    let second = f.ready("Second", &mut tx);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("assigned", None)]);
    let second_run = f.run_of(&second);
    assert_eq!(
        tick.outcomes[0].run_id.as_deref(),
        Some(second_run.run_id.as_str())
    );
    assert_eq!(tick.outcomes[0].operation_id, format!("sched-run-{second}"));
    let relinked = f.record(&format!("sched-start-{first}"));
    assert_eq!(relinked.session_id, session.session_id);
    assert_eq!(relinked.run_id.as_deref(), Some(second_run.run_id.as_str()));
    assert_eq!(f.host.launches().len(), 1);
    // The link is published on the new run's stream.
    assert!(
        f.m.runtime
            .run_events(&f.owner(), 0, 500, Some(&second_run.run_id), 0)
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.record.reason.as_deref() == Some("session_assigned"))
    );
    assert!(f.tick(&mut tx).outcomes.is_empty());
    assert_eq!(
        f.audited("scheduler_assign"),
        [(
            AuditOutcome::Succeeded,
            Some("assigned".into()),
            Some(format!("sched-run-{second}"))
        )]
    );

    // The second run fails at its target (cancelled): the step is ready again and
    // its next run is a new generation; the finished first step never is.
    f.m.runtime
        .run_cancel(&f.owner(), &second_run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(f.level(&second, &mut tx), ReadinessLevel::Confirmed);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("assigned", None)]);
    let third_run = f.run_of(&second);
    assert_eq!(third_run.operation_id, format!("sched-run-{second}.2"));
    assert_ne!(third_run.run_id, second_run.run_id);
    let runs = f.m.runtime.run_list(&f.owner(), false).unwrap();
    assert_eq!(runs.len(), 3);
    assert_eq!(
        runs.iter()
            .filter(|run| run.run_id == first_run.run_id)
            .count(),
        1
    );
}

/// HS2-DG0YPA: the scheduler's start for a step whose run another live session
/// already works adopts that session and launches nothing; a person's holder refuses
/// the start; a refill of a held run is refused; once the holder is gone the next
/// pass replaces it on the same run.
#[test]
fn hs2_dg0ypa_the_executor_adopts_a_live_holder_and_never_doubles_a_run() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    let operation = format!("sched-start-{step}");
    f.tick(&mut tx);
    let first = f.record(&operation);
    let run = f.run_of(&step);
    // The scheduler's session crashes and, before any pass, someone starts a session
    // for the run by hand: the crash is learned from the host and the manual start
    // takes the run.
    f.host.crash(&f.terminal(&first));
    let manual =
        f.m.runtime
            .session_start(
                &f.owner(),
                serde_json::from_value(json!({"target_id": id(1), "operation_id": "manual-1",
                "tool": "codex", "run_id": run.run_id}))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap()
            .session;
    assert_eq!(f.record(&operation).state, SessionState::Lost);
    assert_eq!(f.host.launches().len(), 2);
    // A start placed for the step now adopts it, however often it is asked.
    for _ in 0..2 {
        assert_eq!(
            OwnedSessions.start(&f.m.runtime, &f.start_request(&step, no_load()), &mut tx),
            StartOutcome::Adopted {
                session_id: manual.session_id.clone(),
                run_id: run.run_id.clone(),
            }
        );
    }
    assert_eq!(f.host.launches().len(), 2);
    assert_eq!(f.records().len(), 2);
    // Passes see the run worked and start nothing.
    for _ in 0..2 {
        let tick = f.tick(&mut tx);
        assert!(tick.outcomes.is_empty());
        assert_eq!(
            placement(&tick, &manual.session_id),
            (PlacementAction::Keep, "busy")
        );
    }
    // A refill of the held run is refused, and links nothing.
    let free =
        f.m.runtime
            .session_start(
                &f.owner(),
                serde_json::from_value(
                    json!({"target_id": id(1), "operation_id": "free-1", "tool": "codex"}),
                )
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap()
            .session;
    assert_eq!(
        OwnedSessions.assign(
            &f.m.runtime,
            &PlacementAssign {
                owner_id: f.owner(),
                target_id: id(1),
                session_id: free.session_id.clone(),
                step_id: step.clone(),
                actor_id: "worker-1".into(),
            },
            &mut tx,
        ),
        AssignOutcome::Refused {
            reason: "session_run_held".into()
        }
    );
    assert_eq!(f.record("free-1").run_id, None);
    // A person takes the holder over: the start is refused, typed, not adopted.
    f.m.runtime
        .session_takeover(
            &f.owner(),
            &manual.session_id,
            TakeoverSession { pause: false },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        OwnedSessions.start(&f.m.runtime, &f.start_request(&step, no_load()), &mut tx),
        StartOutcome::Refused {
            reason: "session_run_held".into()
        }
    );
    // The person's session ends: the run is empty again, and the next pass gives it
    // to the free session, on the same run; nothing launches or is dispatched twice.
    f.host.crash(&f.terminal(&f.record("manual-1")));
    f.at(&clock, START + MINUTE);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("assigned", None)]);
    assert_eq!(
        (
            tick.outcomes[0].session_id.as_deref(),
            tick.outcomes[0].run_id.as_deref()
        ),
        (Some(free.session_id.as_str()), Some(run.run_id.as_str()))
    );
    assert_eq!(
        f.record("free-1").run_id.as_deref(),
        Some(run.run_id.as_str())
    );
    assert_eq!(f.m.runtime.run_list(&f.owner(), false).unwrap().len(), 1);
    assert_eq!(f.host.launches().len(), 3);
    assert!(f.tick(&mut tx).outcomes.is_empty());
}

impl Placed {
    /// HS2-T16SK0: write the target's coordination pool, as role sessions leave it.
    fn pool(&self, change: impl FnOnce(&mut crate::coordination::State)) {
        let mut state = crate::coordination::State::new("project".into(), "source".into());
        change(&mut state);
        let directory = std::path::Path::new(&self.m.target.root)
            .join(".git")
            .join("hotsheet-coordination");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
    }
    fn assignment_log(&self) -> Vec<(Option<String>, Option<String>, Option<u64>)> {
        self.m
            .runtime
            .scheduler_decisions(&self.owner(), &DecisionsRequest::default())
            .unwrap()
            .entries
            .into_iter()
            .filter(|entry| {
                entry.record.kind == hotsheet_ticketing::orchestration::DecisionKind::Assignment
            })
            .map(|entry| {
                (
                    entry.record.reason,
                    entry.record.outcome,
                    entry.record.retry_at_ms,
                )
            })
            .collect()
    }
    fn explain(&self, step_id: &str, tx: &mut Transaction<'_>) -> Explanation {
        self.m
            .runtime
            .scheduler_explain(
                &self.owner(),
                &ExplainRequest {
                    step_id: Some(step_id.into()),
                    ..ExplainRequest::default()
                },
                Some(&worker()),
                tx,
            )
            .unwrap()
    }
}

fn role(id: &str, expires_at: i64) -> crate::coordination::Session {
    serde_json::from_value(
        json!({"id": id, "generation": "g1", "expires_at": expires_at,
        "return_route": {"kind": "human_relay"}}),
    )
    .unwrap()
}

/// HS2-T16SK0: a placed step's run that no target dispatcher will assign is reported
/// in the tick, the decision log and `explain`, with the time its session claims it;
/// a dispatcher that comes up in time prevents the claim, one that leaves brings it
/// back; at the named time the ticket is claimed for the session's own worker id, and
/// later passes have nothing left to report.
#[test]
fn hs2_t16sk0_a_run_nobody_assigns_is_reported_then_claimed_for_its_session() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    // No pool at all in the target: neither a dispatcher nor an integrator.
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    let session = f.record(&format!("sched-start-{step}"));
    let run = f.run_of(&step);
    let due = START + 2 * MINUTE;
    let [waiting] = &tick.view.assignments[..] else {
        panic!("one waiting run: {:?}", tick.view.assignments);
    };
    assert_eq!(
        (
            waiting.run_id.as_str(),
            waiting.session_id.as_str(),
            waiting.step_id.as_deref(),
            waiting.state.code(),
            waiting.dispatcher_live,
            waiting.integrator_live,
            waiting.claim_at_ms,
            waiting.outcome.as_deref(),
        ),
        (
            run.run_id.as_str(),
            session.session_id.as_str(),
            Some(step.as_str()),
            "no_live_dispatcher",
            Some(false),
            Some(false),
            Some(due),
            None
        )
    );
    // The loop wakes at the end of the wait, not before and not on a timer.
    assert_eq!(tick.view.schedule.retry_at_ms, Some(due));
    assert_eq!(
        f.assignment_log(),
        [(Some("no_live_dispatcher".into()), None, Some(due))]
    );
    let explained = f.explain(&step, &mut tx);
    assert_eq!(
        (
            explained.standing.as_str(),
            explained.reason.as_str(),
            explained.retry_at_ms
        ),
        ("running", "no_live_dispatcher", Some(due))
    );
    // Repeated passes before the time claim nothing.
    f.at(&clock, START + MINUTE);
    for _ in 0..2 {
        assert!(f.tick(&mut tx).outcomes.is_empty());
    }
    assert_eq!(
        f.m.runtime.run(&f.owner(), &run.run_id).unwrap().state,
        RunState::Queued
    );

    // At the time, a session that never took up its role in the pool is given no
    // claim and no time is named: its joining is the event that lets it go ahead.
    f.at(&clock, due);
    let tick = f.tick(&mut tx);
    let waiting = &tick.view.assignments[0];
    assert_eq!(
        (
            waiting.state.code(),
            waiting.joined,
            waiting.outcome.as_deref()
        ),
        ("no_live_dispatcher", false, None)
    );
    assert_ne!(tick.view.schedule.retry_at_ms, Some(due));
    assert_eq!(
        f.m.runtime.run(&f.owner(), &run.run_id).unwrap().state,
        RunState::Queued
    );

    // The session joins as a worker and a dispatcher comes up with it: the ticket is
    // left to the dispatcher.
    let worker_id = session.hosted.as_ref().unwrap().worker_id.clone();
    let now_s = |ms: u64| i64::try_from(ms / 1000).unwrap();
    let joined = |state: &mut crate::coordination::State| {
        state
            .workers
            .insert(worker_id.clone(), role(&worker_id, now_s(due) + 3600));
    };
    f.pool(|state| {
        joined(state);
        state.dispatcher = Some(role("dispatcher-1", now_s(due) + 60));
    });
    let tick = f.tick(&mut tx);
    assert!(tick.view.assignments[0].joined);
    assert_eq!(tick.view.assignments[0].state.code(), "awaiting_dispatcher");
    assert_eq!(tick.view.assignments[0].claim_at_ms, None);
    assert_eq!(
        f.m.runtime.run(&f.owner(), &run.run_id).unwrap().state,
        RunState::Queued
    );
    assert_eq!(f.explain(&step, &mut tx).reason, "in_progress");
    // A legacy integrator assigns too.
    f.pool(|state| {
        joined(state);
        state.integrator = Some(role("integrator-1", now_s(due) + 60));
    });
    assert_eq!(
        f.tick(&mut tx).view.assignments[0].state.code(),
        "awaiting_dispatcher"
    );
    // A pool that cannot be read concludes nothing and names no time.
    let state =
        std::path::Path::new(&f.m.target.root).join(".git/hotsheet-coordination/state.json");
    std::fs::write(&state, b"not a pool").unwrap();
    let tick = f.tick(&mut tx);
    assert_eq!(
        (
            tick.view.assignments[0].state.code(),
            tick.view.assignments[0].claim_at_ms
        ),
        ("pool_unknown", None)
    );
    assert_eq!(
        f.m.runtime.run(&f.owner(), &run.run_id).unwrap().state,
        RunState::Queued
    );

    // The roles lapse: nobody will assign it, and the wait is long over. The ticket
    // is claimed for the session's own worker id.
    f.pool(|state| {
        joined(state);
        state.dispatcher = Some(role("dispatcher-1", now_s(START)));
    });
    f.at(&clock, due + MINUTE);
    let tick = f.tick(&mut tx);
    let claimed = &tick.view.assignments[0];
    assert_eq!(
        (claimed.state.code(), claimed.outcome.as_deref()),
        ("no_live_dispatcher", Some("claimed"))
    );
    let after = f.m.runtime.run(&f.owner(), &run.run_id).unwrap();
    assert_eq!(
        (
            after.state,
            after.worker_id.as_deref(),
            after.history.last().unwrap().reason.as_deref()
        ),
        (
            RunState::Dispatched,
            Some(worker_id.as_str()),
            Some("claimed_for_session")
        )
    );
    let ticket = ops::resolve(&f.m.target_store, &run.target.native_id)
        .unwrap()
        .unwrap();
    assert_eq!(ticket.claimed_by.as_deref(), Some(worker_id.as_str()));
    assert_eq!(
        f.audited("scheduler_assignment"),
        [(
            AuditOutcome::Succeeded,
            Some("claimed".into()),
            Some(run.run_id.clone())
        )]
    );
    assert!(f.assignment_log().contains(&(
        Some("no_live_dispatcher".into()),
        Some("claimed".into()),
        Some(due)
    )));
    // Nothing is left waiting: later passes report and claim nothing more.
    for _ in 0..2 {
        let tick = f.tick(&mut tx);
        assert!(tick.view.assignments.is_empty());
        assert!(tick.outcomes.is_empty());
    }
    assert_eq!(f.host.launches().len(), 1);
}

/// HS2-T16SK0: a dry run reports and never claims; a ticket someone else claimed
/// first is a typed refusal the run's sync then learns, so nothing is retried; and a
/// restarted runtime over the same records names the same claim time.
#[test]
fn hs2_t16sk0_a_dry_run_claims_nothing_a_taken_ticket_is_learned_and_a_restart_agrees() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    f.tick(&mut tx);
    let run = f.run_of(&step);
    let due = START + 2 * MINUTE;
    // A restart: a new runtime over the same home sees the same wait.
    let restarted = OrchestrationRuntime::new(
        f.m.home.clone(),
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(f.m.home.join("checkouts.json")),
        hotsheet_ticketing::ProviderConfigRegistry::new(f.m._root.path().join("providers.json")),
        Arc::new(super::Memory::default()),
        Arc::new(super::Peer::default()),
        id(10),
    )
    .unwrap()
    .with_capacity_probe(f.probe.clone())
    .with_session_host(f.host.clone())
    .with_session_executor(Arc::new(OwnedSessions));
    let view = restarted
        .scheduler_plan(&f.owner(), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(view.assignments[0].claim_at_ms, Some(due));
    assert_eq!(view.schedule.retry_at_ms, Some(due));
    // Automatic placement off: past the time, a pass only reports.
    let mut manual = policy(Vec::new());
    manual.placement.automatic = false;
    f.set_policy(manual, &mut tx);
    f.at(&clock, due + MINUTE);
    let dry = f.tick(&mut tx);
    assert_eq!(dry.view.assignments[0].state.code(), "no_live_dispatcher");
    assert_eq!(dry.view.assignments[0].outcome, None);
    assert_eq!(
        f.m.runtime.run(&f.owner(), &run.run_id).unwrap().state,
        RunState::Queued
    );
    // Someone else claims the ticket before the loop does.
    let ticket = ops::resolve(&f.m.target_store, &run.target.native_id)
        .unwrap()
        .unwrap();
    ops::claim_with_eta(
        &f.m.target_store,
        &ticket.id,
        &hotsheet_model::Timestamp::from(NOW),
        hotsheet_model::Timestamp::from("2030-01-01T00:00:00Z"),
        "someone-else",
        None,
        None,
    )
    .unwrap();
    // The session takes up its role, so the loop would claim for it now.
    let worker_id = f
        .record(&format!("sched-start-{step}"))
        .hosted
        .unwrap()
        .worker_id;
    f.pool(|state| {
        state
            .workers
            .insert(worker_id.clone(), role(&worker_id, 4_000_000_000));
    });
    f.set_policy(policy(Vec::new()), &mut tx);
    let tick = f.tick(&mut tx);
    let refused = &tick.view.assignments[0];
    assert_eq!(refused.outcome.as_deref(), Some("refused"));
    assert!(refused.outcome_detail.is_some());
    // The run learned who holds it, so nothing waits and nothing backs the target off.
    let after = f.m.runtime.run(&f.owner(), &run.run_id).unwrap();
    assert_ne!(after.state, RunState::Queued);
    assert_eq!(after.worker_id.as_deref(), Some("someone-else"));
    assert!(f.tick(&mut tx).view.assignments.is_empty());
    assert!(
        SchedulerStore::new(&f.m.home)
            .memory(&f.owner())
            .unwrap()
            .iter()
            .all(|entry| entry.refusals == 0)
    );
}

/// `(action, reason, detail)` of the placement that names the session.
fn placed(view: &TickView, session_id: &str) -> (PlacementAction, &'static str, Option<String>) {
    let found = view
        .view
        .schedule
        .placements
        .iter()
        .find(|placement| placement.session_id.as_deref() == Some(session_id))
        .unwrap_or_else(|| panic!("no placement for {session_id}"));
    (found.action, found.reason.code(), found.detail.clone())
}

/// HS2-NPTKS8: a session that never joins its role is wound down when its time is up
/// and its step's run is placed again; a replacement whose role prompt fails is wound
/// down at once; after two replacements the next is kept and reported; a session that
/// joins is never acted on.
#[test]
fn hs2_nptks8_a_session_that_never_joins_is_replaced_a_bounded_number_of_times() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    let operation = format!("sched-start-{step}");
    f.tick(&mut tx);
    let first = f.record(&operation);
    let run = f.run_of(&step);
    let deadline = START + 10 * MINUTE;
    // Waiting: kept busy, and the loop wakes when its time is up.
    f.at(&clock, START + 5 * MINUTE);
    for _ in 0..2 {
        let tick = f.tick(&mut tx);
        assert!(tick.outcomes.is_empty());
        assert_eq!(
            placed(&tick, &first.session_id),
            (PlacementAction::Keep, "busy", None)
        );
        assert_eq!(tick.view.schedule.retry_at_ms, Some(deadline));
    }
    // Its time is up: wound down at its safe point, without a cooldown.
    f.at(&clock, deadline);
    // The replacement's role prompt will be refused by its tool.
    f.host
        .push_answers
        .lock()
        .unwrap()
        .extend([PushAttempt::Rejected]);
    let tick = f.tick(&mut tx);
    assert_eq!(
        placed(&tick, &first.session_id),
        (PlacementAction::WindDown, "never_joined", None)
    );
    assert_eq!(outcomes(&tick), [("wind_down_requested", None)]);
    assert_eq!(f.record(&operation).state, SessionState::Stopped);
    assert!(
        SchedulerStore::new(&f.m.home)
            .memory(&f.owner())
            .unwrap()
            .iter()
            .all(|entry| entry.last_wound_down_at_ms.is_none())
    );
    // The next pass places the run again, through the ordinary start.
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    let second = f.record(&format!("{operation}.2"));
    assert_eq!(
        (second.replaces.as_deref(), second.run_id.as_deref()),
        (Some(first.session_id.as_str()), Some(run.run_id.as_str()))
    );
    // Its prompt failed: it is learned on the next read and acted on at once.
    let tick = f.tick(&mut tx);
    assert_eq!(
        placed(&tick, &second.session_id),
        (PlacementAction::WindDown, "role_prompt_failed", None)
    );
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    let third = f.record(&format!("{operation}.3"));
    // The third never joins either: its run had its replacements, so it is kept.
    f.at(&clock, deadline + 10 * MINUTE);
    for _ in 0..2 {
        let tick = f.tick(&mut tx);
        assert!(tick.outcomes.is_empty(), "{:?}", tick.outcomes);
        assert_eq!(
            placed(&tick, &third.session_id),
            (
                PlacementAction::Keep,
                "never_joined",
                Some("replacements_exhausted".into())
            )
        );
    }
    let log =
        f.m.runtime
            .scheduler_decisions(&f.owner(), &DecisionsRequest::default())
            .unwrap()
            .entries;
    assert!(log.iter().any(|entry| {
        entry.record.reason.as_deref() == Some("never_joined")
            && entry.record.outcome.as_deref() == Some("wind_down_requested")
    }));
    // It joins late: never acted on again.
    let worker_id = third.hosted.as_ref().unwrap().worker_id.clone();
    f.pool(|state| {
        state
            .workers
            .insert(worker_id.clone(), role(&worker_id, 4_000_000_000));
    });
    let tick = f.tick(&mut tx);
    assert_eq!(
        placed(&tick, &third.session_id),
        (PlacementAction::Keep, "busy", None)
    );
    assert_eq!(f.m.runtime.run_list(&f.owner(), false).unwrap().len(), 1);
    assert_eq!(f.host.launches().len(), 3);
}

/// HS2-NPTKS8: a person's session and a session started by hand are never acted on,
/// and a dry run only shows the wind-down.
#[test]
fn hs2_nptks8_a_persons_session_is_left_and_a_dry_run_only_shows_it() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let first = f.ready("First", &mut tx);
    let second = f.ready("Second", &mut tx);
    f.tick(&mut tx);
    let mine = f.record(&format!("sched-start-{first}"));
    let theirs = f.record(&format!("sched-start-{second}"));
    f.m.runtime
        .session_takeover(
            &f.owner(),
            &theirs.session_id,
            TakeoverSession { pause: false },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let mut manual_policy = policy(Vec::new());
    manual_policy.placement.automatic = false;
    manual_policy.targets[0].max_sessions = 3;
    f.set_policy(manual_policy, &mut tx);
    let manual =
        f.m.runtime
            .session_start(
                &f.owner(),
                serde_json::from_value(
                    json!({"target_id": id(1), "operation_id": "manual-1", "tool": "codex"}),
                )
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap()
            .session;
    f.at(&clock, START + 10 * MINUTE);
    let dry = f.tick(&mut tx);
    assert!(dry.outcomes.is_empty());
    assert_eq!(
        placed(&dry, &mine.session_id),
        (PlacementAction::WindDown, "never_joined", None)
    );
    assert_eq!(placed(&dry, &theirs.session_id).1, "human_takeover");
    assert_ne!(placed(&dry, &manual.session_id).1, "never_joined");
    assert_eq!(f.record(&mine.operation_id).state, SessionState::Running);
}

impl Placed {
    /// HS2-R7QH35: the texts pushed into one terminal that name a request.
    fn requests(&self, terminal: &str) -> Vec<String> {
        self.host
            .pushes()
            .into_iter()
            .filter(|(id, text)| id == terminal && !text.contains("started this session"))
            .map(|(_, text)| text)
            .collect()
    }
    fn slug(&self, run: &hotsheet_ticketing::orchestration::Run) -> String {
        ops::resolve(&self.m.target_store, &run.target.native_id)
            .unwrap()
            .unwrap()
            .slug
    }
}

/// HS2-R7QH35: a refilled session is told its new ticket through its host's session
/// push; a tool that is not ready gets it on its next readiness event, after its
/// role, and only once; the same request again is not pushed twice; a person's
/// session is never sent one.
#[test]
fn hs2_r7qh35_a_refilled_session_is_told_its_ticket_once_when_its_tool_is_ready() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let first = f.ready("First", &mut tx);
    f.tick(&mut tx);
    let session = f.record(&format!("sched-start-{first}"));
    let terminal = f.terminal(&session);
    f.finish(&first, &mut tx);
    assert!(
        f.requests(&terminal).is_empty(),
        "a start's role prompt names it"
    );
    // The tool is busy elsewhere when the refill comes: the push is not ready.
    f.host
        .push_answers
        .lock()
        .unwrap()
        .extend([PushAttempt::NotReady]);
    let second = f.ready("Second", &mut tx);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("assigned", None)]);
    let run = f.run_of(&second);
    let slug = f.slug(&run);
    let worker_id = session.hosted.as_ref().unwrap().worker_id.clone();
    let tried = f.requests(&terminal);
    assert_eq!(tried.len(), 1);
    assert!(tried[0].contains(&format!("`{slug}`")), "{tried:?}");
    assert!(tried[0].contains("from the dispatcher"), "{tried:?}");
    assert!(tried[0].contains(&worker_id), "{tried:?}");
    let listing =
        f.m.runtime
            .hosted_sessions(&f.m.target.id, Some(&session.operation_id), false, &mut tx)
            .unwrap();
    assert_eq!(
        listing.sessions[0].request_prompt,
        Some(RolePrompt::Pending)
    );
    assert!(
        f.m.runtime
            .run_events(&f.owner(), 0, 500, Some(&run.run_id), 0)
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.record.reason.as_deref() == Some("session_request_pending"))
    );
    // The tool becomes ready: the readiness event delivers it, once; the role prompt,
    // delivered long ago, is not pushed again.
    let before = f.host.pushes().len();
    f.m.runtime.hosted_role_retry(&terminal, &mut tx).unwrap();
    assert_eq!(f.host.pushes().len(), before + 1);
    assert_eq!(f.requests(&terminal).len(), 2);
    f.m.runtime.hosted_role_retry(&terminal, &mut tx).unwrap();
    assert_eq!(f.host.pushes().len(), before + 1);
    let listing =
        f.m.runtime
            .hosted_sessions(&f.m.target.id, Some(&session.operation_id), false, &mut tx)
            .unwrap();
    assert_eq!(
        listing.sessions[0].request_prompt,
        Some(RolePrompt::Delivered)
    );
    // Telling it again changes nothing at the host.
    let told =
        f.m.runtime
            .session_tell_ticket(&f.owner(), &session.session_id, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(told.request_prompt, Some(RolePrompt::Delivered));
    assert_eq!(f.requests(&terminal).len(), 2);
    // A person takes it over: nothing is sent to their session.
    f.m.runtime
        .session_takeover(
            &f.owner(),
            &session.session_id,
            TakeoverSession { pause: false },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        f.m.runtime
            .session_tell_ticket(&f.owner(), &session.session_id, Some(&worker()), &mut tx)
            .unwrap_err()
            .code(),
        "session_human_owned"
    );
    assert_eq!(f.requests(&terminal).len(), 2);
}

/// HS2-R7QH35: after HS2-T16SK0 claims a ticket for a session because no dispatcher
/// is live, the session is told the ticket is its own.
#[test]
fn hs2_r7qh35_a_session_given_a_claim_is_told_to_work_it() {
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let mut at_once = policy(Vec::new());
    at_once.placement.dispatcher_wait_minutes = 0;
    f.set_policy(at_once, &mut tx);
    let step = f.ready("First", &mut tx);
    f.tick(&mut tx);
    let session = f.record(&format!("sched-start-{step}"));
    let worker_id = session.hosted.as_ref().unwrap().worker_id.clone();
    f.pool(|state| {
        state
            .workers
            .insert(worker_id.clone(), role(&worker_id, 4_000_000_000));
    });
    let tick = f.tick(&mut tx);
    assert_eq!(tick.view.assignments[0].outcome.as_deref(), Some("claimed"));
    let run = f.run_of(&step);
    let told = f.requests(&f.terminal(&session));
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains(&format!("`{}`", f.slug(&run))), "{told:?}");
    assert!(told[0].contains("claimed for you"), "{told:?}");
    assert!(told[0].contains(&worker_id), "{told:?}");
}

impl Placed {
    /// HS2-3REXQ7: the target ticket of a step, as its session's worker works it:
    /// claimed under `worker` in `phase`, and the run synced from the target.
    fn worked(
        &self,
        step_id: &str,
        holder: &str,
        phase: hotsheet_model::StartedPhase,
        tx: &mut Transaction<'_>,
    ) -> hotsheet_ticketing::orchestration::Run {
        let run = self.run_of(step_id);
        let ticket = ops::resolve(&self.m.target_store, &run.target.native_id)
            .unwrap()
            .unwrap();
        if ticket.claimed_by.as_deref() != Some(holder) {
            ops::claim_with_eta(
                &self.m.target_store,
                &ticket.id,
                &hotsheet_model::Timestamp::from(NOW),
                hotsheet_model::Timestamp::from("2030-01-01T00:00:00Z"),
                holder,
                None,
                None,
            )
            .unwrap();
        }
        ops::update(
            &self.m.target_store,
            &ticket.id,
            NOW.into(),
            hotsheet_ticketing::TicketPatch {
                started_phase: Some(Some(phase)),
                ..Default::default()
            },
        )
        .unwrap();
        self.m
            .runtime
            .run_sync(&self.owner(), &run.run_id, Some(&worker()), tx)
            .unwrap()
            .run
    }
}

/// HS2-3REXQ7: a session lost while its run waits (its ticket in `final_testing`)
/// is not replaced, as reconciliation finds no lost work, however often a pass runs;
/// once the run needs work again the next pass replaces it on the same run. A session
/// is never refilled with the run it holds.
#[test]
fn hs2_3rexq7_a_run_that_waits_needs_no_session_and_no_refill_with_itself() {
    use hotsheet_model::StartedPhase;
    clocked!(clock, tx);
    let f = Placed::new(Vec::new(), &mut tx);
    let step = f.ready("First", &mut tx);
    let operation = format!("sched-start-{step}");
    f.tick(&mut tx);
    let session = f.record(&operation);
    let worker_id = session.hosted.as_ref().unwrap().worker_id.clone();
    let run = f.worked(&step, &worker_id, StartedPhase::FinalTesting, &mut tx);
    assert_eq!(run.state, RunState::Dispatched);
    // Refilling it with the run it holds is refused, and links nothing new.
    assert_eq!(
        OwnedSessions.assign(
            &f.m.runtime,
            &PlacementAssign {
                owner_id: f.owner(),
                target_id: id(1),
                session_id: session.session_id.clone(),
                step_id: step.clone(),
                actor_id: "worker-1".into(),
            },
            &mut tx,
        ),
        AssignOutcome::Refused {
            reason: "session_holds_run".into()
        }
    );
    // The session dies while the run waits.
    f.host.crash(&f.terminal(&session));
    for minutes in [1, 2, 3] {
        f.at(&clock, START + minutes * MINUTE);
        let tick = f.tick(&mut tx);
        assert!(tick.outcomes.is_empty(), "{:?}", tick.outcomes);
        assert!(tick.view.schedule.unplaced.is_empty());
    }
    assert_eq!(f.record(&operation).state, SessionState::Lost);
    assert_eq!(f.host.launches().len(), 1);
    // Reconciliation agrees: no work was lost.
    let view =
        f.m.runtime
            .run_reconcile(&f.owner(), &run.run_id, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(
        super::recovery::finding(
            &view,
            hotsheet_ticketing::orchestration::ExternalSource::Session
        ),
        (
            hotsheet_ticketing::orchestration::Verification::Verified,
            Some("waiting_on_gate".into())
        )
    );
    // The run needs work again: the next pass replaces the session on the same run.
    f.worked(&step, &worker_id, StartedPhase::Working, &mut tx);
    let tick = f.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    let replacement = f.record(&format!("{operation}.2"));
    assert_eq!(replacement.run_id.as_deref(), Some(run.run_id.as_str()));
    assert_eq!(f.m.runtime.run_list(&f.owner(), false).unwrap().len(), 1);
}
