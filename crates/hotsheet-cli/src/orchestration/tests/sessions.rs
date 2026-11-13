//! HS2-GDJXGZ: orchestrator-owned sessions through the shared runtime with a scripted
//! terminal host. Synthetic stores, identities and tools only.
use std::sync::atomic::AtomicBool;

use hotsheet_model::Ticket;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    ApprovalGate, ApprovalPolicy, ApprovalRegistry, AuditOutcome, CpuCapacity, CreateGrant,
    DenialReason, DiskCapacity, ExternalWork, HostObservation, HostedSessionRegistry,
    MachineLimits, MemoryCapacity, Observed, PolicyBody, RunEventKind, SafePointBlocker,
    SessionControl, SessionError, SessionOwner, SessionRole, SessionScope, SessionState, SetPolicy,
    TargetAction, TargetPolicy, ToolAvailability, ToolPolicy, ToolScope,
};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::{Value, json};

use super::tickets::{Machine, NOW, bind_and_grant, human, machine, unsupported, worker};
use super::*;

/// A terminal host the test drives: it launches by remembering, and the test decides
/// what is busy, what crashed and whether it answers at all.
#[derive(Default)]
struct ScriptedHost {
    live: Mutex<Vec<HostSessionState>>,
    launches: Mutex<Vec<HostLaunch>>,
    joined: Mutex<Vec<(String, SessionRole)>>,
    stops: AtomicUsize,
    down: AtomicBool,
    refusing: AtomicBool,
}
impl ScriptedHost {
    fn live(&self) -> std::sync::MutexGuard<'_, Vec<HostSessionState>> {
        self.live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn launches(&self) -> Vec<HostLaunch> {
        self.launches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    fn set_busy(&self, session_id: &str, busy: bool) {
        for state in self.live().iter_mut() {
            if state.session_id == session_id {
                state.busy = busy;
            }
        }
    }
    fn crash(&self, session_id: &str) {
        self.live().retain(|state| state.session_id != session_id);
    }
}
impl SessionHost for ScriptedHost {
    fn start(&self, _: &Checkout, launch: &HostLaunch) -> Result<(), SessionHostError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(SessionHostError::Unavailable);
        }
        if self.refusing.load(Ordering::SeqCst) {
            return Err(SessionHostError::Refused);
        }
        self.launches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(launch.clone());
        let mut live = self.live();
        if !live.iter().any(|s| s.session_id == launch.session_id) {
            live.push(HostSessionState {
                session_id: launch.session_id.clone(),
                busy: false,
            });
        }
        Ok(())
    }
    fn sessions(&self, _: &Checkout) -> Option<Vec<HostSessionState>> {
        (!self.down.load(Ordering::SeqCst)).then(|| self.live().clone())
    }
    fn stop(&self, _: &Checkout, session_id: &str) -> Result<(), SessionHostError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(SessionHostError::Unavailable);
        }
        self.stops.fetch_add(1, Ordering::SeqCst);
        self.crash(session_id);
        Ok(())
    }
    fn join_role(&self, _: &Checkout, session_id: &str, role: SessionRole) -> RolePrompt {
        self.joined
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((session_id.into(), role));
        RolePrompt::Delivered
    }
}

/// A probe whose answer the test sets.
#[derive(Default)]
struct Probe(Mutex<HostObservation>);
impl Probe {
    fn set(&self, change: impl FnOnce(&mut HostObservation)) {
        change(
            &mut self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
}
impl CapacityProbe for Probe {
    fn observe(&self, _: &Checkout) -> HostObservation {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
fn roomy() -> HostObservation {
    HostObservation {
        sampled_at_ms: Some(150),
        memory: MemoryCapacity {
            free_mb: Some(16_384),
            total_mb: Some(32_768),
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
/// Target 1 runs codex and target 2 runs claude, two sessions each, four per machine.
fn policy(required_for: Vec<ApprovalGate>) -> PolicyBody {
    let target = |n: u128, tool: &str| TargetPolicy {
        target_id: id(n),
        enabled: true,
        actions: vec![
            TargetAction::SessionsManage,
            TargetAction::WorkStart,
            TargetAction::TicketsRead,
        ],
        tools: vec![scope(tool)],
        max_sessions: 2,
        machine: None,
        speculation: None,
    };
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 4,
            ..MachineLimits::default()
        },
        tools: ["codex", "claude"]
            .map(|tool| ToolPolicy {
                tool: tool.into(),
                max_sessions: 4,
                weight: 50,
                accounts: Vec::new(),
            })
            .into(),
        targets: vec![target(1, "codex"), target(2, "claude")],
        approval: ApprovalPolicy { required_for },
        ..PolicyBody::default()
    }
}

struct Fixture {
    m: Machine,
    host: Arc<ScriptedHost>,
    probe: Arc<Probe>,
}
impl Fixture {
    /// One machine that hosts both targets, a roomy probe and a scripted terminal host.
    /// `worker-1` may manage sessions and dispatch on both targets.
    fn new(required_for: Vec<ApprovalGate>, tx: &mut Transaction<'_>) -> Self {
        let mut m = machine(10, false, unsupported);
        let host = Arc::new(ScriptedHost::default());
        let probe = Arc::new(Probe::default());
        probe.set(|observation| *observation = roomy());
        m.runtime = m
            .runtime
            .clone()
            .with_capacity_probe(probe.clone())
            .with_session_host(host.clone());
        for n in [1, 2] {
            bind_and_grant(&m, n, &[], tx);
            m.runtime
                .grant(
                    &m.reference(n),
                    CreateGrant {
                        grant_id: None,
                        actor_id: "worker-1".into(),
                        actions: vec![
                            TargetAction::SessionsManage,
                            TargetAction::WorkStart,
                            TargetAction::TicketsRead,
                        ],
                        session_scope: Some(SessionScope {
                            tools: vec![scope("codex"), scope("claude")],
                            max_concurrency: 8,
                        }),
                        expires_at_ms: 10_000,
                    },
                    Some(&human()),
                    tx,
                )
                .unwrap();
        }
        let fixture = Self { m, host, probe };
        fixture.set_policy(policy(required_for), tx);
        fixture
    }
    fn set_policy(&self, body: PolicyBody, tx: &mut Transaction<'_>) {
        let expected_revision = self.m.runtime.policy(&self.m.owner.id).unwrap().revision;
        self.m
            .runtime
            .set_policy(
                &self.m.owner.id,
                SetPolicy {
                    expected_revision,
                    policy: body,
                },
                Some(&human()),
                tx,
            )
            .unwrap();
    }
    fn start(
        &self,
        operation: &str,
        target: u128,
        tool: &str,
        extra: Value,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        let mut body = json!({"target_id": id(target), "operation_id": operation, "tool": tool});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.m.runtime.session_start(
            &self.m.owner.id,
            serde_json::from_value(body).unwrap(),
            Some(&worker()),
            tx,
        )
    }
    fn started(
        &self,
        operation: &str,
        target: u128,
        tool: &str,
        tx: &mut Transaction<'_>,
    ) -> SessionView {
        self.start(operation, target, tool, json!({}), tx).unwrap()
    }
    fn stop(
        &self,
        session_id: &str,
        force: bool,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        self.m.runtime.session_stop(
            &self.m.owner.id,
            session_id,
            StopSession { force },
            Some(actor),
            tx,
        )
    }
    fn safe_point(&self, session_id: &str, tx: &mut Transaction<'_>) -> Vec<SafePointBlocker> {
        let view = self
            .m
            .runtime
            .session_safe_point(&self.m.owner.id, session_id, Some(&worker()), tx)
            .unwrap();
        let point = view.safe_point.unwrap();
        assert_eq!(point.safe, point.blockers.is_empty());
        point.blockers
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
    fn target_ticket(&self, title: &str) -> Ticket {
        ops::create(
            &self.m.target_store,
            ulid::Ulid::new(),
            "TGT",
            NOW.into(),
            NewTicket {
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap()
    }
    /// Write the target's coordination pool as a role session would have left it.
    fn pool(&self, change: impl FnOnce(&mut crate::coordination::State)) {
        let mut state = crate::coordination::State::new("project".into(), "source".into());
        change(&mut state);
        let directory = Path::new(&self.m.target.root)
            .join(".git")
            .join("hotsheet-coordination");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
    }
}
fn assignment(worker: &str, phase: crate::coordination::Phase) -> crate::coordination::Assignment {
    crate::coordination::Assignment {
        id: "assignment-1".into(),
        worker: worker.into(),
        generation: "g1".into(),
        ticket: hotsheet_ticketing::TicketRef {
            connection_id: "target-source".into(),
            native_id: id(700),
        },
        slug: "TGT-1".into(),
        base: "0".repeat(40),
        phase,
        commit: None,
        handoff: None,
        return_route: crate::coordination::ReturnRoute::HumanRelay,
        recovery_note: None,
        suspension: None,
        suspension_history: Vec::new(),
        scope: Vec::new(),
        transitions: Vec::new(),
        retirement_note: None,
    }
}
fn pool_session(expires_at: i64) -> crate::coordination::Session {
    serde_json::from_value(
        json!({"id":"someone","generation":"g1","expires_at":expires_at,
        "return_route":{"kind":"human_relay"}}),
    )
    .unwrap()
}
fn session_error(result: Result<SessionView, OrchestrationError>) -> SessionError {
    match result {
        Err(OrchestrationError::Session(error)) => error,
        Err(other) => panic!("expected a session error, got {other}"),
        Ok(view) => panic!("expected a session error, got {:?}", view.session.state),
    }
}
fn denied(result: Result<SessionView, OrchestrationError>) -> DenialReason {
    match result {
        Err(OrchestrationError::Denied(reason)) => reason,
        Err(other) => panic!("expected a denial, got {other}"),
        Ok(_) => panic!("expected a denial"),
    }
}

#[test]
fn hs2_gdjxgz_a_start_launches_once_on_the_targets_host_labels_it_and_joins_the_role() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(Vec::new(), &mut tx);
    let owner = f.m.owner.id.clone();
    // A run on target 1, so the session is labelled with it.
    let orchestration = f.m.orchestration_ticket("Coordinate");
    let work = f.target_ticket("Do the work");
    let run =
        f.m.runtime
            .run_dispatch(
                &owner,
                serde_json::from_value(json!({
                    "ticket": {"connection_id": f.m.owner_connection(),
                        "native_id": orchestration.id.to_string()},
                    "target_id": id(1), "connection_id": f.m.target_connection(),
                    "native_id": work.id.to_string(), "operation_id": "run-1",
                    "mode": "dispatcher",
                }))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap()
            .run;
    let request = json!({"model": "model-a", "effort": "high", "run_id": run.run_id});
    let view = f
        .start("op-1", 1, "codex", request.clone(), &mut tx)
        .unwrap();
    let session = &view.session;
    assert_eq!(session.state, SessionState::Running);
    assert_eq!(session.owner, SessionOwner::Orchestrator);
    assert_eq!(session.role, SessionRole::Worker);
    let hosted = session.hosted.as_ref().unwrap();
    assert_eq!(hosted.server_id, id(10));
    assert_eq!(hosted.worker_id, format!("codex-{}", hosted.session_id));
    // Labelled with the orchestration project and the run.
    let tail = &run.run_id[run.run_id.len() - 8..];
    let label = format!("{}: run {tail}", f.m.owner.alias);
    assert_eq!(view.label.as_deref(), Some(label.as_str()));
    assert_eq!(view.role_prompt, Some(RolePrompt::Delivered));
    let launches = f.host.launches();
    assert_eq!(launches.len(), 1);
    assert_eq!(
        launches[0],
        HostLaunch {
            session_id: hosted.session_id.clone(),
            tool: "codex".into(),
            model: Some("model-a".into()),
            effort: Some("high".into()),
            label,
        }
    );
    assert_eq!(
        *f.host.joined.lock().unwrap(),
        [(hosted.session_id.clone(), SessionRole::Worker)]
    );
    assert_eq!(
        f.m.last_audit(),
        ("sessions_manage".into(), AuditOutcome::Succeeded, None)
    );

    // The same operation id again, however often: the same session, no second launch.
    for _ in 0..3 {
        let again = f
            .start("op-1", 1, "codex", request.clone(), &mut tx)
            .unwrap();
        assert_eq!(again.session, view.session);
    }
    assert_eq!(f.host.launches().len(), 1);
    assert_eq!(f.m.runtime.session_list(&owner, true).unwrap().len(), 1);
    // The same id for something else conflicts and launches nothing.
    assert!(matches!(
        f.start("op-1", 1, "codex", json!({"model": "model-b"}), &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    // The host's own record: one reservation, started, owned by the orchestrator.
    let records = HostedSessionRegistry::new(&f.m.home)
        .list(&f.m.target.id)
        .unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].started);
    assert_eq!(records[0].origin.owner_checkout_id, owner);
    // The hosted half alone is idempotent too: asked directly, it answers `existing`.
    let direct =
        f.m.runtime
            .hosted_session_start(
                &f.m.target.id,
                HostedSessionStart {
                    session: hotsheet_ticketing::orchestration::NewHostedSession {
                        operation_id: "op-1".into(),
                        tool: "codex".into(),
                        model: Some("model-a".into()),
                        effort: Some("high".into()),
                        role: SessionRole::Worker,
                        origin: records[0].origin.clone(),
                    },
                    limits: None,
                    actor: None,
                },
                &mut tx,
            )
            .unwrap();
    assert_eq!(direct.outcome, HostedStartOutcome::Existing);
    assert_eq!(f.host.launches().len(), 1);

    // The run's event stream carries the session, identifiers and codes only.
    let events =
        f.m.runtime
            .run_events(&owner, 0, 100, Some(&run.run_id), 0)
            .unwrap();
    let session_events: Vec<_> = events
        .entries
        .iter()
        .filter(|entry| entry.record.kind == RunEventKind::Session)
        .collect();
    assert_eq!(session_events.len(), 1);
    assert_eq!(
        session_events[0].record.reason.as_deref(),
        Some("session_running")
    );
    // What reconciliation will read (HS2-75Q254).
    assert_eq!(
        f.m.runtime.session_work(&owner, &run.run_id),
        Observed::Verified(ExternalWork {
            recorded: Some(session.session_id.clone()),
            live: Some(session.session_id.clone()),
        })
    );
    assert_eq!(
        f.m.runtime.session_work(&owner, &id(4242)),
        Observed::Verified(ExternalWork::default())
    );
    // A run of another target cannot be named.
    assert!(matches!(
        f.start("op-x", 2, "claude", json!({"run_id": run.run_id}), &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));

    // A second target with another tool, on the same machine.
    let other = f.started("op-2", 2, "claude", &mut tx);
    assert_eq!(other.session.tool, "claude");
    assert_eq!(
        other.label.as_deref(),
        Some(format!("{}: worker", f.m.owner.alias).as_str())
    );
    assert_eq!(f.host.launches().len(), 2);
    let load =
        f.m.runtime
            .session_load(&owner, &id(2), "claude", None)
            .unwrap();
    assert_eq!(
        (
            load.target_sessions,
            load.machine_sessions,
            load.tool_sessions
        ),
        (1, 2, 1)
    );
    // The hosted listing shows both, alive and idle, and that the pool is unreadable
    // here because the target is not a git checkout yet.
    let listing =
        f.m.runtime
            .hosted_sessions(&f.m.target.id, None, false, &mut tx)
            .unwrap();
    assert_eq!(listing.sessions.len(), 2);
    assert!(
        listing
            .sessions
            .iter()
            .all(|s| s.alive == Some(true) && s.busy == Some(false))
    );
    assert_eq!(listing.roles, HostedRoles::default());
}

#[test]
fn hs2_gdjxgz_grants_policy_caps_and_approvals_govern_a_start() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(vec![ApprovalGate::RoleSessionStart], &mut tx);
    let owner = f.m.owner.id.clone();
    // A tool the policy does not list for the target, and an actor without a grant.
    assert_eq!(
        denied(f.start("op-t", 1, "claude", json!({}), &mut tx)),
        DenialReason::PolicyDenied
    );
    assert_eq!(
        f.m.last_audit().2.as_deref(),
        Some("policy_tool_not_permitted")
    );
    let stranger = MutationActor::parse("ai", Some("stranger".into())).unwrap();
    assert!(matches!(
        f.m.runtime.session_start(
            &owner,
            serde_json::from_value(
                json!({"target_id": id(1), "operation_id": "op-s", "tool": "codex"})
            )
            .unwrap(),
            Some(&stranger),
            &mut tx,
        ),
        Err(OrchestrationError::Denied(DenialReason::NoGrant))
    ));
    assert!(f.host.launches().is_empty());
    assert!(f.m.runtime.session_list(&owner, false).unwrap().is_empty());

    // The target cap is two: the third start is denied exactly at the bound.
    let first = f.started("op-1", 1, "codex", &mut tx);
    f.started("op-2", 1, "codex", &mut tx);
    assert_eq!(
        denied(f.start("op-3", 1, "codex", json!({}), &mut tx)),
        DenialReason::PolicyDenied
    );
    assert_eq!(f.m.last_audit().2.as_deref(), Some("policy_target_cap"));
    assert_eq!(f.host.launches().len(), 2);
    // A repeat of a start that already runs is not a new session and is not capped.
    assert_eq!(
        f.started("op-1", 1, "codex", &mut tx).session,
        first.session
    );
    // Empty, then refill: a stopped session frees its place.
    f.git(&["init", "-q"]);
    f.stop(&first.session.session_id, false, &worker(), &mut tx)
        .unwrap();
    f.started("op-3", 1, "codex", &mut tx);
    assert_eq!(f.host.launches().len(), 3);

    // A role session asks a person first, once, and starts after the approval.
    let integrator = json!({"role": "integrator"});
    for _ in 0..2 {
        assert_eq!(
            denied(f.start("op-i", 2, "claude", integrator.clone(), &mut tx)),
            DenialReason::ApprovalRequired
        );
    }
    let approvals = f.m.runtime.approvals(&owner, &mut tx).unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].gate, ApprovalGate::RoleSessionStart);
    assert_eq!(f.host.launches().len(), 3);
    f.m.runtime
        .decide_approval(
            &owner,
            &approvals[0].approval_id,
            true,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let started = f
        .start("op-i", 2, "claude", integrator.clone(), &mut tx)
        .unwrap();
    assert_eq!(started.session.role, SessionRole::Integrator);
    assert_eq!(
        f.host.joined.lock().unwrap().last().unwrap().1,
        SessionRole::Integrator
    );
    // A worker on the same target needs no approval.
    f.started("op-w", 2, "claude", &mut tx);
    // A second integrator for the target is refused before anyone is asked.
    assert_eq!(
        session_error(f.start("op-i2", 2, "claude", integrator, &mut tx)),
        SessionError::RoleActive
    );
    let _ = ApprovalRegistry::new(&f.m.home);

    // Without a policy nothing automatic runs.
    std::fs::remove_file(
        Path::new(&f.m.owner.root)
            .join(".hotsheet2")
            .join("orchestration-policy.json"),
    )
    .unwrap();
    assert_eq!(
        denied(f.start("op-n", 1, "codex", json!({}), &mut tx)),
        DenialReason::PolicyDenied
    );
    assert_eq!(f.m.last_audit().2.as_deref(), Some("policy_absent"));
}

#[test]
fn hs2_gdjxgz_the_targets_machine_may_refuse_and_a_refusal_never_launches() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(Vec::new(), &mut tx);
    let owner = f.m.owner.id.clone();
    let reason = |operation: &str| {
        f.m.runtime
            .session_list(&owner, false)
            .unwrap()
            .into_iter()
            .find(|session| session.operation_id == operation)
            .map(|session| {
                (
                    session.state,
                    session.history.last().unwrap().reason.clone(),
                )
            })
            .unwrap()
    };
    // Its own admission says no: below the RAM floor.
    f.probe
        .set(|observation| observation.memory.free_mb = Some(100));
    assert_eq!(
        session_error(f.start("op-1", 1, "codex", json!({}), &mut tx)),
        SessionError::AdmissionRefused
    );
    assert_eq!(
        reason("op-1"),
        (SessionState::Failed, Some("below_ram_floor".into()))
    );
    assert_eq!(
        f.m.last_audit(),
        (
            "sessions_manage".into(),
            AuditOutcome::Failed,
            Some("session_admission_refused".into())
        )
    );
    // The host forgot the reservation, and the same operation id keeps answering with
    // the failed session without asking again.
    assert!(
        HostedSessionRegistry::new(&f.m.home)
            .list(&f.m.target.id)
            .unwrap()
            .is_empty()
    );
    f.probe
        .set(|observation| observation.memory.free_mb = Some(16_384));
    let failed = f.start("op-1", 1, "codex", json!({}), &mut tx).unwrap();
    assert_eq!(failed.session.state, SessionState::Failed);
    assert!(f.host.launches().is_empty());
    // A failed start occupies nothing.
    assert!(f.m.runtime.session_list(&owner, true).unwrap().is_empty());

    // A tool the machine lacks.
    f.probe.set(|observation| {
        observation.tools.as_mut().unwrap()[0].installed = false;
    });
    assert_eq!(
        session_error(f.start("op-2", 1, "codex", json!({}), &mut tx)),
        SessionError::AdmissionRefused
    );
    assert_eq!(reason("op-2").1.as_deref(), Some("tool_not_installed"));
    f.probe.set(|observation| *observation = roomy());

    // The shared capacity reading is a few seconds old; the cap is checked again on a
    // fresh listing, so sessions the reading has not seen still count.
    for n in 0..4 {
        f.host.live().push(HostSessionState {
            session_id: format!("someone-elses-{n}"),
            busy: false,
        });
    }
    assert_eq!(
        session_error(f.start("op-3", 1, "codex", json!({}), &mut tx)),
        SessionError::AdmissionRefused
    );
    assert_eq!(reason("op-3").1.as_deref(), Some("session_cap_reached"));
    f.host.live().clear();

    // The tool would not launch, then no terminal host answers at all.
    f.host.refusing.store(true, Ordering::SeqCst);
    assert_eq!(
        session_error(f.start("op-4", 1, "codex", json!({}), &mut tx)),
        SessionError::StartFailed
    );
    f.host.refusing.store(false, Ordering::SeqCst);
    f.host.down.store(true, Ordering::SeqCst);
    assert_eq!(
        session_error(f.start("op-5", 1, "codex", json!({}), &mut tx)),
        SessionError::HostUnavailable
    );
    f.host.down.store(false, Ordering::SeqCst);
    assert!(f.host.launches().is_empty());
    assert!(
        HostedSessionRegistry::new(&f.m.home)
            .list(&f.m.target.id)
            .unwrap()
            .is_empty()
    );
    // A fresh operation now starts.
    assert_eq!(
        f.started("op-6", 1, "codex", &mut tx).session.state,
        SessionState::Running
    );
    assert_eq!(f.host.launches().len(), 1);
}

#[test]
fn hs2_gdjxgz_wind_down_happens_only_at_a_safe_point_the_host_confirms() {
    use SafePointBlocker as B;
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(Vec::new(), &mut tx);
    let owner = f.m.owner.id.clone();
    let view = f.started("op-1", 1, "codex", &mut tx);
    let sid = view.session.session_id.clone();
    let hosted = view.session.hosted.clone().unwrap();
    let refused = |f: &Fixture, tx: &mut Transaction<'_>| {
        assert_eq!(
            session_error(f.stop(&sid, false, &worker(), tx)),
            SessionError::NotSafe
        );
        assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);
        assert_eq!(
            f.m.runtime.session(&owner, &sid).unwrap().state,
            SessionState::Running
        );
    };
    // Not a git checkout: whether work is uncommitted or handed off is unknown, and
    // unknown is not safe.
    assert_eq!(
        f.safe_point(&sid, &mut tx),
        [B::WorktreeUnknown, B::HandoffUnknown]
    );
    refused(&f, &mut tx);
    f.git(&["init", "-q"]);
    assert_eq!(f.safe_point(&sid, &mut tx), []);

    // Mid-turn.
    f.host.set_busy(&hosted.session_id, true);
    assert_eq!(f.safe_point(&sid, &mut tx), [B::MidTurn]);
    refused(&f, &mut tx);
    f.host.set_busy(&hosted.session_id, false);

    // A claim held under the session's worker id. Someone else's claim is not its.
    let mine = f.target_ticket("Claimed by the session");
    let theirs = f.target_ticket("Claimed by a person");
    let lease = || hotsheet_model::Timestamp::new("2026-10-15T01:00:00Z");
    ops::claim_with_eta(
        &f.m.target_store,
        &theirs.id,
        &NOW.into(),
        lease(),
        "person-1",
        None,
        None,
    )
    .unwrap();
    assert_eq!(f.safe_point(&sid, &mut tx), []);
    ops::claim_with_eta(
        &f.m.target_store,
        &mine.id,
        &NOW.into(),
        lease(),
        &hosted.worker_id,
        None,
        None,
    )
    .unwrap();
    assert_eq!(f.safe_point(&sid, &mut tx), [B::HeldClaim]);
    refused(&f, &mut tx);
    assert_eq!(
        f.m.last_audit(),
        (
            "sessions_manage".into(),
            AuditOutcome::Failed,
            Some("session_not_safe".into())
        )
    );

    // Uncommitted changes and an unfinished assignment, all at once with the claim.
    std::fs::write(Path::new(&f.m.target.root).join("draft.txt"), "synthetic").unwrap();
    f.pool(|state| {
        state.assignments.insert(
            "assignment-1".into(),
            assignment(&hosted.worker_id, crate::coordination::Phase::Accepted),
        );
        // Another worker's assignment is not this session's.
        let mut other = assignment("someone-else", crate::coordination::Phase::Handoff);
        other.id = "assignment-2".into();
        state.assignments.insert("assignment-2".into(), other);
    });
    f.host.set_busy(&hosted.session_id, true);
    assert_eq!(
        f.safe_point(&sid, &mut tx),
        [
            B::MidTurn,
            B::HeldClaim,
            B::UncommittedChanges,
            B::PendingHandoff
        ]
    );
    refused(&f, &mut tx);
    // Cleared one at a time, in another order than they appeared.
    f.host.set_busy(&hosted.session_id, false);
    f.pool(|state| {
        state.assignments.insert(
            "assignment-1".into(),
            assignment(&hosted.worker_id, crate::coordination::Phase::Done),
        );
    });
    assert_eq!(
        f.safe_point(&sid, &mut tx),
        [B::HeldClaim, B::UncommittedChanges]
    );
    ops::release(
        &f.m.target_store,
        &mine.id,
        NOW.into(),
        &hosted.worker_id,
        false,
    )
    .unwrap();
    assert_eq!(f.safe_point(&sid, &mut tx), [B::UncommittedChanges]);
    refused(&f, &mut tx);
    std::fs::remove_file(Path::new(&f.m.target.root).join("draft.txt")).unwrap();
    // A damaged pool file is unknown again.
    let pool = Path::new(&f.m.target.root).join(".git/hotsheet-coordination/state.json");
    let good = std::fs::read(&pool).unwrap();
    std::fs::write(&pool, b"{").unwrap();
    assert_eq!(f.safe_point(&sid, &mut tx), [B::HandoffUnknown]);
    std::fs::write(&pool, good).unwrap();
    assert_eq!(f.safe_point(&sid, &mut tx), []);

    // The orchestrator can never force.
    assert_eq!(
        session_error(f.stop(&sid, true, &worker(), &mut tx)),
        SessionError::ForceNotPermitted
    );
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);
    // At the safe point the wind-down goes through, once.
    let stopped = f.stop(&sid, false, &worker(), &mut tx).unwrap();
    assert_eq!(stopped.session.state, SessionState::Stopped);
    let entry = stopped.session.history.last().unwrap();
    assert_eq!(entry.control, Some(SessionControl::WindDown));
    assert_eq!(entry.reason.as_deref(), Some("wound_down"));
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 1);
    assert!(f.host.live().is_empty());
    assert_eq!(
        session_error(f.stop(&sid, false, &worker(), &mut tx)),
        SessionError::NotActive
    );
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 1);
    // A finished session is trivially at a safe point and nobody is asked.
    assert_eq!(f.safe_point(&sid, &mut tx), []);
    let record = HostedSessionRegistry::new(&f.m.home)
        .get(&f.m.target.id, "op-1")
        .unwrap()
        .unwrap();
    assert_eq!(record.end_reason.as_deref(), Some("wound_down"));
}

#[test]
fn hs2_gdjxgz_a_crashed_session_is_lost_and_replaced_by_a_new_one() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(Vec::new(), &mut tx);
    let owner = f.m.owner.id.clone();
    f.git(&["init", "-q"]);
    let first = f.started("op-1", 1, "codex", &mut tx);
    let second = f.started("op-2", 1, "codex", &mut tx);
    let sid = first.session.session_id.clone();
    let replace = |f: &Fixture, operation: &str, tx: &mut Transaction<'_>| {
        f.start(operation, 1, "codex", json!({"replaces": sid}), tx)
    };
    // A running session cannot be replaced.
    assert_eq!(
        session_error(replace(&f, "op-r", &mut tx)),
        SessionError::InvalidTransition
    );
    // Syncing an unchanged world changes nothing, however often.
    for _ in 0..2 {
        let same =
            f.m.runtime
                .session_sync(&owner, &sid, Some(&worker()), &mut tx)
                .unwrap();
        assert_eq!(same.session, first.session);
    }
    // The terminal dies. The host is down at first: nothing is concluded from that.
    f.host
        .crash(&first.session.hosted.as_ref().unwrap().session_id);
    f.host.down.store(true, Ordering::SeqCst);
    let unknown =
        f.m.runtime
            .session_safe_point(&owner, &sid, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(unknown.session.state, SessionState::Running);
    assert!(
        unknown
            .safe_point
            .unwrap()
            .blockers
            .contains(&SafePointBlocker::SessionUnknown)
    );
    f.host.down.store(false, Ordering::SeqCst);
    let summary =
        f.m.runtime
            .sessions_sync(&owner, Some(&worker()), &mut tx)
            .unwrap();
    assert!(summary.failed.is_empty());
    let states: Vec<_> = summary
        .sessions
        .iter()
        .map(|view| view.session.state)
        .collect();
    assert_eq!(states, [SessionState::Lost, SessionState::Running]);
    let lost = f.m.runtime.session(&owner, &sid).unwrap();
    assert_eq!(
        lost.history.last().unwrap().reason.as_deref(),
        Some("host_lost")
    );
    // The host recorded the loss once; a later sync finds nothing new.
    let record = HostedSessionRegistry::new(&f.m.home)
        .get(&f.m.target.id, "op-1")
        .unwrap()
        .unwrap();
    assert_eq!(record.end_reason.as_deref(), Some("host_lost"));
    assert_eq!(
        f.m.runtime
            .sessions_sync(&owner, Some(&worker()), &mut tx)
            .unwrap()
            .sessions
            .len(),
        1
    );
    // The cap had no room for a third session; the lost one freed its place.
    let replacement = replace(&f, "op-r", &mut tx).unwrap();
    assert_eq!(replacement.session.state, SessionState::Running);
    assert_eq!(replacement.session.replaces.as_deref(), Some(sid.as_str()));
    assert_ne!(
        replacement.session.hosted.as_ref().unwrap().session_id,
        first.session.hosted.as_ref().unwrap().session_id
    );
    // Asked again: the same replacement, not another.
    assert_eq!(
        replace(&f, "op-r", &mut tx).unwrap().session,
        replacement.session
    );
    assert_eq!(f.host.launches().len(), 3);
    assert_eq!(f.m.runtime.session_list(&owner, true).unwrap().len(), 2);
    // A lost session cannot be stopped; the survivor still can.
    assert_eq!(
        session_error(f.stop(&sid, false, &worker(), &mut tx)),
        SessionError::NotActive
    );
    f.stop(&second.session.session_id, false, &worker(), &mut tx)
        .unwrap();

    // The host launched a session for a request whose answer never arrived, and the
    // session died before the request was repeated. The repeat records what really
    // happened instead of a running session, and launches nothing.
    let launched = f.host.launches().len();
    let unheard =
        f.m.runtime
            .hosted_session_start(
                &f.m.target.id,
                HostedSessionStart {
                    session: hotsheet_ticketing::orchestration::NewHostedSession {
                        operation_id: "op-unheard".into(),
                        tool: "codex".into(),
                        model: None,
                        effort: None,
                        role: SessionRole::Worker,
                        origin: hotsheet_ticketing::orchestration::SessionOrigin {
                            owner_checkout_id: owner.clone(),
                            project: f.m.owner.alias.clone(),
                            run_id: None,
                        },
                    },
                    limits: None,
                    actor: None,
                },
                &mut tx,
            )
            .unwrap();
    assert_eq!(unheard.outcome, HostedStartOutcome::Started);
    f.host.crash(&unheard.session.unwrap().session.session_id);
    let repeated = f.started("op-unheard", 1, "codex", &mut tx);
    assert_eq!(repeated.session.state, SessionState::Lost);
    let reasons: Vec<_> = repeated
        .session
        .history
        .iter()
        .filter_map(|entry| entry.reason.as_deref())
        .collect();
    assert_eq!(reasons, ["started", "host_lost"]);
    assert_eq!(f.host.launches().len(), launched + 1);

    // The host loses its whole record (a wiped machine home): a session it once
    // confirmed is lost, not assumed to be running.
    std::fs::remove_file(f.m.home.join("orchestration-hosted-sessions.json")).unwrap();
    let forgotten =
        f.m.runtime
            .session_sync(
                &owner,
                &replacement.session.session_id,
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(forgotten.session.state, SessionState::Lost);
    assert_eq!(
        forgotten.session.history.last().unwrap().reason.as_deref(),
        Some("host_forgot")
    );
}

#[test]
fn hs2_gdjxgz_a_person_can_take_over_pause_or_stop_any_session() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(Vec::new(), &mut tx);
    let owner = f.m.owner.id.clone();
    f.git(&["init", "-q"]);
    let taken = f.started("op-1", 1, "codex", &mut tx);
    let sid = taken.session.session_id.clone();
    let takeover = |sid: &str, pause: bool, actor: &MutationActor, tx: &mut Transaction<'_>| {
        f.m.runtime
            .session_takeover(&owner, sid, TakeoverSession { pause }, Some(actor), tx)
    };
    // The orchestrator cannot give itself a person's session, or pause one.
    assert!(matches!(
        takeover(&sid, false, &worker(), &mut tx),
        Err(OrchestrationError::Forbidden(_))
    ));
    let view = takeover(&sid, false, &human(), &mut tx).unwrap();
    assert_eq!(view.session.owner, SessionOwner::Human);
    assert!(view.unverified.is_empty());
    assert_eq!(view.session.state, SessionState::Running);
    assert_eq!(
        f.m.last_audit(),
        ("session_takeover".into(), AuditOutcome::Succeeded, None)
    );
    // The host knows too, and refuses an orchestrator's stop on its own.
    let hosted = HostedSessionRegistry::new(&f.m.home);
    assert_eq!(
        hosted.get(&f.m.target.id, "op-1").unwrap().unwrap().owner,
        SessionOwner::Human
    );
    let direct =
        f.m.runtime
            .hosted_session_stop(
                &f.m.target.id,
                HostedSessionStop {
                    operation_id: "op-1".into(),
                    force: false,
                    actor: Some(worker()),
                },
                &mut tx,
            )
            .unwrap();
    assert!(!direct.stopped);
    assert_eq!(direct.reason.as_deref(), Some("session_human_owned"));
    // The orchestrator treats it as human-owned: no wind-down, and no host call.
    assert_eq!(
        session_error(f.stop(&sid, false, &worker(), &mut tx)),
        SessionError::HumanOwned
    );
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);
    // Repeating the takeover changes nothing; a pause after it is recorded and stays.
    assert_eq!(
        takeover(&sid, false, &human(), &mut tx).unwrap().session,
        view.session
    );
    let paused = takeover(&sid, true, &human(), &mut tx).unwrap();
    assert!(paused.session.paused);
    assert_eq!(f.m.last_audit().0, "session_pause");
    assert!(hosted.get(&f.m.target.id, "op-1").unwrap().unwrap().paused);
    assert!(
        takeover(&sid, false, &human(), &mut tx)
            .unwrap()
            .session
            .paused
    );
    // It still occupies its place.
    assert_eq!(f.m.runtime.session_list(&owner, true).unwrap().len(), 1);

    // A person's plain stop respects the safe point; their explicit one does not.
    let hosted_id = taken.session.hosted.as_ref().unwrap().session_id.clone();
    f.host.set_busy(&hosted_id, true);
    assert_eq!(
        session_error(f.stop(&sid, false, &human(), &mut tx)),
        SessionError::NotSafe
    );
    assert_eq!(
        f.m.last_audit(),
        (
            "session_stop".into(),
            AuditOutcome::Failed,
            Some("session_not_safe".into())
        )
    );
    let stopped = f.stop(&sid, true, &human(), &mut tx).unwrap();
    assert_eq!(stopped.session.state, SessionState::Stopped);
    let entry = stopped.session.history.last().unwrap();
    assert_eq!(entry.control, Some(SessionControl::Stop));
    assert_eq!(entry.actor_id.as_deref(), Some("maintainer"));
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.m.last_audit(),
        ("session_stop".into(), AuditOutcome::Succeeded, None)
    );
    assert!(matches!(
        takeover(&sid, false, &human(), &mut tx),
        Err(OrchestrationError::Session(SessionError::NotActive))
    ));

    // A person takes a session over at its host. The orchestrator learns it when it
    // next looks, and from the host's refusal even if it does not look first.
    let learned = f.started("op-2", 1, "codex", &mut tx);
    let later = f.started("op-3", 1, "codex", &mut tx);
    for operation in ["op-2", "op-3"] {
        f.m.runtime
            .hosted_session_control(
                &f.m.target.id,
                HostedSessionControl {
                    operation_id: operation.into(),
                    pause: operation == "op-2",
                    actor: Some(human()),
                },
                &mut tx,
            )
            .unwrap()
            .session
            .unwrap();
    }
    assert!(matches!(
        f.m.runtime.hosted_session_control(
            &f.m.target.id,
            HostedSessionControl {
                operation_id: "op-2".into(),
                pause: false,
                actor: Some(worker()),
            },
            &mut tx,
        ),
        Err(OrchestrationError::Forbidden(_))
    ));
    let synced =
        f.m.runtime
            .session_sync(
                &owner,
                &learned.session.session_id,
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(synced.session.owner, SessionOwner::Human);
    assert!(synced.session.paused);
    assert_eq!(
        synced.session.history.last().unwrap().reason.as_deref(),
        Some("host_takeover")
    );
    // Not looked at first: the wind-down reaches the host, is refused, and the record
    // is a person's from then on.
    assert_eq!(
        session_error(f.stop(&later.session.session_id, false, &worker(), &mut tx)),
        SessionError::HumanOwned
    );
    assert_eq!(
        f.m.runtime
            .session(&owner, &later.session.session_id)
            .unwrap()
            .owner,
        SessionOwner::Human
    );
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 1);

    // While the target cannot be reached a takeover still holds here, and says what
    // it could not tell the host.
    let offline = f.started("op-4", 2, "claude", &mut tx);
    std::fs::remove_dir_all(&f.m.target.root).unwrap();
    let held = takeover(&offline.session.session_id, false, &human(), &mut tx).unwrap();
    assert_eq!(held.session.owner, SessionOwner::Human);
    assert_eq!(held.unverified, ["target_session"]);
}

#[test]
fn hs2_gdjxgz_a_role_session_starts_only_when_the_target_has_none() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(Vec::new(), &mut tx);
    let owner = f.m.owner.id.clone();
    f.git(&["init", "-q"]);
    // The pool already has a live integrator someone else started, and a dispatcher
    // whose registration lapsed.
    f.pool(|state| {
        state.integrator = Some(pool_session(i64::MAX));
        state.dispatcher = Some(pool_session(0));
    });
    let listing =
        f.m.runtime
            .hosted_sessions(&f.m.target.id, None, false, &mut tx)
            .unwrap();
    assert_eq!(
        listing.roles,
        HostedRoles {
            integrator_live: Some(true),
            dispatcher_live: Some(false)
        }
    );
    assert_eq!(
        session_error(f.start("op-i", 1, "codex", json!({"role": "integrator"}), &mut tx)),
        SessionError::RoleActive
    );
    assert!(f.host.launches().is_empty());
    let dispatcher = f
        .start("op-d", 1, "codex", json!({"role": "dispatcher"}), &mut tx)
        .unwrap();
    assert_eq!(dispatcher.session.role, SessionRole::Dispatcher);
    assert_eq!(
        dispatcher.label.as_deref(),
        Some(format!("{}: dispatcher", f.m.owner.alias).as_str())
    );
    // A dispatcher answers for every unfinished assignment of the pool, not only for
    // one worker's.
    f.pool(|state| {
        state.assignments.insert(
            "assignment-1".into(),
            assignment("some-worker", crate::coordination::Phase::Ci),
        );
    });
    assert_eq!(
        f.safe_point(&dispatcher.session.session_id, &mut tx),
        [SafePointBlocker::PendingHandoff]
    );
    // Another orchestration project asking the same host for a second dispatcher is
    // refused by the host, which knows the one it already started.
    let foreign =
        f.m.runtime
            .hosted_session_start(
                &f.m.target.id,
                HostedSessionStart {
                    session: hotsheet_ticketing::orchestration::NewHostedSession {
                        operation_id: "foreign-op".into(),
                        tool: "codex".into(),
                        model: None,
                        effort: None,
                        role: SessionRole::Dispatcher,
                        origin: hotsheet_ticketing::orchestration::SessionOrigin {
                            owner_checkout_id: "elsewhere-0123456789ab".into(),
                            project: "Another project".into(),
                            run_id: None,
                        },
                    },
                    limits: None,
                    actor: None,
                },
                &mut tx,
            )
            .unwrap();
    assert_eq!(foreign.outcome, HostedStartOutcome::Refused);
    assert_eq!(foreign.reason.as_deref(), Some("session_role_active"));
    assert_eq!(f.host.launches().len(), 1);
    // Once the dispatcher's terminal is gone, the role is free again on both sides.
    f.host
        .crash(&dispatcher.session.hosted.as_ref().unwrap().session_id);
    f.m.runtime
        .sessions_sync(&owner, Some(&worker()), &mut tx)
        .unwrap();
    let again = f
        .start("op-d2", 1, "codex", json!({"role": "dispatcher"}), &mut tx)
        .unwrap();
    assert_eq!(again.session.state, SessionState::Running);
}

#[test]
fn hs2_gdjxgz_a_start_that_never_reached_the_host_can_be_repeated_or_dropped() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(Vec::new(), &mut tx);
    let owner = f.m.owner.id.clone();
    f.git(&["init", "-q"]);
    // The start was recorded here and the host never answered: a damaged admission
    // file on the host makes its half fail before anything launches.
    let admission = f.m.home.join("orchestration-admission.json");
    std::fs::write(&admission, b"{").unwrap();
    assert!(f.start("op-1", 1, "codex", json!({}), &mut tx).is_err());
    let pending = f.m.runtime.session_list(&owner, true).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, SessionState::Requested);
    assert!(f.host.launches().is_empty());
    // A sync learns nothing new and concludes nothing.
    let same =
        f.m.runtime
            .session_sync(&owner, &pending[0].session_id, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(same.session.state, SessionState::Requested);
    // The requested session counts against the cap (two per target), but never
    // against its own retry.
    assert!(f.start("op-2", 1, "codex", json!({}), &mut tx).is_err());
    assert_eq!(
        denied(f.start("op-3", 1, "codex", json!({}), &mut tx)),
        DenialReason::PolicyDenied
    );
    std::fs::remove_file(&admission).unwrap();
    let retried = f.started("op-1", 1, "codex", &mut tx);
    assert_eq!(retried.session.session_id, pending[0].session_id);
    assert_eq!(retried.session.state, SessionState::Running);
    assert_eq!(f.host.launches().len(), 1);
    // The other one is dropped instead: the host never had it, so nothing is stopped
    // there and the record ends as never started.
    let other =
        f.m.runtime
            .session_list(&owner, true)
            .unwrap()
            .into_iter()
            .find(|session| session.operation_id == "op-2")
            .unwrap();
    let dropped = f
        .stop(&other.session_id, false, &worker(), &mut tx)
        .unwrap();
    assert_eq!(dropped.session.state, SessionState::Stopped);
    assert_eq!(
        dropped.session.history.last().unwrap().reason.as_deref(),
        Some("never_started")
    );
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 0);
    // Its operation id keeps answering with the stopped session.
    assert_eq!(
        f.started("op-2", 1, "codex", &mut tx).session.state,
        SessionState::Stopped
    );
    assert_eq!(f.host.launches().len(), 1);
}
