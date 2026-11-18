//! HS2-GDJXGZ: orchestrator-owned sessions through the shared runtime with a scripted
//! terminal host. Synthetic stores, identities and tools only.
use std::sync::atomic::AtomicBool;

use hotsheet_model::Ticket;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, CpuCapacity, DenialReason, DiskCapacity, ExternalWork, HostObservation,
    HostedSessionEvent, HostedSessionRegistry, MachineLimits, MemoryCapacity, Observed,
    PermissionAction, PermissionAnswer, PermissionState, PolicyBody, RunEventKind,
    SafePointBlocker, SessionControl, SessionError, SessionOwner, SessionRole, SessionState,
    SetPolicy, TargetAction, TargetPolicy, ToolAvailability, ToolPolicy, ToolScope,
};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::{Value, json};

use super::tickets::{Machine, NOW, bind_and_allow, human, machine, unsupported, worker};
use super::*;

/// A terminal host the test drives: it launches by remembering, and the test decides
/// what is busy, what crashed and whether it answers at all.
#[derive(Default)]
pub(super) struct ScriptedHost {
    live: Mutex<Vec<HostSessionState>>,
    launches: Mutex<Vec<HostLaunch>>,
    joined: Mutex<Vec<(String, String)>>,
    /// HS2-2DPGDY: what each push answers in turn; `Delivered` once these run out.
    pub(super) push_answers: Mutex<std::collections::VecDeque<PushAttempt>>,
    /// HS2-2DPGDY: the tool takes its first prompt at launch.
    pub(super) prompt_at_launch: AtomicBool,
    pub(super) stops: AtomicUsize,
    pub(super) down: AtomicBool,
    pub(super) refusing: AtomicBool,
    /// HS2-VRC0CQ: the hosting server is shutting down.
    pub(super) stopping: AtomicBool,
    /// HS2-WEFJV8: every setup asked for, as (checkout root, tool), in order with the
    /// launches, and whether setup fails.
    prepared: Mutex<Vec<(String, String)>>,
    pub(super) setup_failing: AtomicBool,
    /// How many launches had happened when each setup was asked for.
    prepared_before: Mutex<Vec<usize>>,
    /// HS2-WHR4GS: runs once when the next setup is asked for, after the start
    /// reserved its terminal and before it launches: a stop that arrives meanwhile.
    pub(super) before_launch: OnceHook,
}

/// HS2-WHR4GS: a hook a test arms to run once.
#[derive(Default)]
pub(super) struct OnceHook(Mutex<Option<Box<dyn FnOnce() + Send>>>);
impl OnceHook {
    pub(super) fn arm(&self, hook: impl FnOnce() + Send + 'static) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(hook));
    }
    fn fire(&self) {
        let hook = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(hook) = hook {
            hook();
        }
    }
}
impl ScriptedHost {
    fn live(&self) -> std::sync::MutexGuard<'_, Vec<HostSessionState>> {
        self.live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(super) fn launches(&self) -> Vec<HostLaunch> {
        self.launches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub(super) fn set_busy(&self, session_id: &str, busy: bool) {
        for state in self.live().iter_mut() {
            if state.session_id == session_id {
                state.busy = busy;
            }
        }
    }
    /// HS2-RG91SK: what the host says about since when a session has been idle.
    pub(super) fn set_idle_since(&self, session_id: &str, since: Option<u64>) {
        for state in self.live().iter_mut() {
            if state.session_id == session_id {
                state.idle_since_ms = since;
            }
        }
    }
    /// HS2-R7QH35: every message pushed into a session, as (terminal, text), in order.
    pub(super) fn pushes(&self) -> Vec<(String, String)> {
        self.joined
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    /// HS2-HCW7MD: every setup asked for, as (checkout root, tool), in order.
    pub(super) fn prepared(&self) -> Vec<(String, String)> {
        self.prepared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub(super) fn crash(&self, session_id: &str) {
        self.live().retain(|state| state.session_id != session_id);
    }
}
impl SessionHost for ScriptedHost {
    fn prepare(&self, checkout: &Checkout, tool: &str) -> Result<ToolSetup, SessionHostError> {
        self.before_launch.fire();
        if self.down.load(Ordering::SeqCst) {
            return Err(SessionHostError::Unavailable);
        }
        if self.setup_failing.load(Ordering::SeqCst) {
            return Err(SessionHostError::Refused);
        }
        self.prepared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((checkout.root.clone(), tool.into()));
        self.prepared_before
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(self.launches().len());
        Ok(ToolSetup { wrote: 2 })
    }
    fn start(&self, _: &Checkout, launch: &HostLaunch) -> Result<Launched, SessionHostError> {
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
                idle_since_ms: None,
            });
        }
        Ok(Launched {
            prompt_at_launch: self.prompt_at_launch.load(Ordering::SeqCst),
        })
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
    fn join_role(&self, _: &Checkout, session_id: &str, text: &str) -> PushAttempt {
        self.joined
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((session_id.into(), text.into()));
        self.push_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .unwrap_or(PushAttempt::Delivered)
    }
    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }
}

/// A probe whose answer the test sets.
#[derive(Default)]
pub(super) struct Probe(Mutex<HostObservation>);
impl Probe {
    pub(super) fn set(&self, change: impl FnOnce(&mut HostObservation)) {
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
pub(super) fn roomy() -> HostObservation {
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
fn policy() -> PolicyBody {
    let target = |n: u128, tool: &str| TargetPolicy {
        target_id: id(n),
        enabled: true,
        actions: None,
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
    /// HS2-XHQ1EH: a person remembered session starts and dispatch on the targets in
    /// `remembered`; elsewhere `worker-1` is asked.
    fn new(remembered: &[u128], tx: &mut Transaction<'_>) -> Self {
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
            bind_and_allow(&m, n, &[], tx);
            if remembered.contains(&n) {
                allow_on(
                    &m.runtime,
                    &m.owner.id,
                    &id(n),
                    &[TargetAction::SessionsManage, TargetAction::WorkStart],
                    &["codex", "claude"],
                    tx,
                );
            }
        }
        let fixture = Self { m, host, probe };
        fixture.set_policy(policy(), tx);
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
    /// HS2-DG0YPA: link a session to a run behind the runtime's back, as an older
    /// build or another process could have left the records.
    fn plant_run(&self, session_id: &str, run_id: &str) {
        let path = self.m.home.join("orchestration-sessions.json");
        let mut file: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for session in file["sessions"].as_array_mut().unwrap() {
            if session["session_id"] == session_id {
                session["run_id"] = json!(run_id);
            }
        }
        std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
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
pub(super) fn assignment(
    worker: &str,
    phase: crate::coordination::Phase,
) -> crate::coordination::Assignment {
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
        worktree: None,
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
    let f = Fixture::new(&[1, 2], &mut tx);
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
            role: SessionRole::Worker,
            // HS2-YC2P7K: started for a run, it asks for the run's exact ticket.
            role_prompt: role_prompt_text(SessionRole::Worker, &hosted.worker_id, Some(&work.slug)),
        }
    );
    assert_eq!(
        *f.host.joined.lock().unwrap(),
        [(
            hosted.session_id.clone(),
            role_prompt_text(SessionRole::Worker, &hosted.worker_id, Some(&work.slug))
        )]
    );
    // HS2-2DPGDY: the record keeps the answer; the role prompt names the worker id.
    assert_eq!(session.role_prompt, Some(RolePrompt::Delivered));
    assert!(launches[0].role_prompt.contains(&hosted.worker_id));
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
                        account: None,
                        operation_id: "op-1".into(),
                        tool: "codex".into(),
                        model: Some("model-a".into()),
                        effort: Some("high".into()),
                        role: SessionRole::Worker,
                        origin: records[0].origin.clone(),
                    },
                    limits: None,
                    actor: None,
                    resources: Vec::new(),
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
    // HS2-2DPGDY: the running session, then how it was asked to take up its role.
    let reasons: Vec<_> = session_events
        .iter()
        .map(|entry| entry.record.reason.as_deref().unwrap())
        .collect();
    assert_eq!(reasons, ["session_running", "session_role_delivered"]);
    // What reconciliation will read (HS2-75Q254).
    assert_eq!(
        f.m.runtime.session_work(&owner, &run.run_id),
        Observed::Verified(ExternalWork {
            recorded: Some(session.session_id.clone()),
            live: Some(session.session_id.clone()),
            note: None,
        })
    );
    assert_eq!(
        f.m.runtime.session_work(&owner, &id(4242)),
        Observed::Verified(ExternalWork {
            note: Some("no_session".into()),
            ..ExternalWork::default()
        })
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
fn hs2_gdjxgz_policy_caps_and_permission_requests_govern_a_start() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1], &mut tx);
    let owner = f.m.owner.id.clone();
    // A tool the policy does not list for the target, and an unidentified actor.
    assert_eq!(
        denied(f.start("op-t", 1, "claude", json!({}), &mut tx)),
        DenialReason::PolicyDenied
    );
    assert_eq!(
        f.m.last_audit().2.as_deref(),
        Some("policy_tool_not_permitted")
    );
    assert!(matches!(
        f.m.runtime.session_start(
            &owner,
            serde_json::from_value(
                json!({"target_id": id(1), "operation_id": "op-s", "tool": "codex"})
            )
            .unwrap(),
            None,
            &mut tx,
        ),
        Err(OrchestrationError::Invalid(_))
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

    // HS2-XHQ1EH: nothing is remembered on target 2, so a start there asks a person
    // first, once, and starts after the answer.
    let integrator = json!({"role": "integrator"});
    let mut asked = Vec::new();
    for _ in 0..2 {
        asked.push(permission_request(f.start(
            "op-i",
            2,
            "claude",
            integrator.clone(),
            &mut tx,
        )));
    }
    assert_eq!(asked[0], asked[1]);
    let requests =
        f.m.runtime
            .permission_list(&owner, &mut tx)
            .unwrap()
            .requests;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].request.key.action,
        PermissionAction::SessionStart
    );
    assert_eq!(requests[0].request.key.tool.as_deref(), Some("claude"));
    assert_eq!(requests[0].summary, "Start a claude session in Target");
    assert_eq!(f.host.launches().len(), 3);
    answer(
        &f.m.runtime,
        &owner,
        &asked[0],
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    let started = f
        .start("op-i", 2, "claude", integrator.clone(), &mut tx)
        .unwrap();
    assert_eq!(started.session.role, SessionRole::Integrator);
    assert!(
        f.host
            .joined
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1
            .contains("`hotsheet-integrator`")
    );
    // The start happened, so Allow Once is used; the next start asks again, and
    // Always Allow remembers the key.
    assert_eq!(
        f.m.runtime
            .permission_list(&owner, &mut tx)
            .unwrap()
            .requests[0]
            .request
            .state,
        PermissionState::Used
    );
    let worker_start = permission_request(f.start("op-w", 2, "claude", json!({}), &mut tx));
    answer(
        &f.m.runtime,
        &owner,
        &worker_start,
        PermissionAnswer::AlwaysAllow,
        &mut tx,
    );
    f.started("op-w", 2, "claude", &mut tx);
    // A second integrator for the target is refused before anyone is asked.
    assert_eq!(
        session_error(f.start("op-i2", 2, "claude", integrator, &mut tx)),
        SessionError::RoleActive
    );

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
    let f = Fixture::new(&[1, 2], &mut tx);
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
            idle_since_ms: None,
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
    let f = Fixture::new(&[1, 2], &mut tx);
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

    // HS2-TRP2MJ: a person's edits in the main checkout are not this worker's work.
    std::fs::write(Path::new(&f.m.target.root).join("draft.txt"), "synthetic").unwrap();
    assert_eq!(f.safe_point(&sid, &mut tx), [B::HeldClaim]);
    // Uncommitted changes in the worktree its assignment names and the unfinished
    // assignment, all at once with the claim.
    f.git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@example.test",
        "commit",
        "--allow-empty",
        "-q",
        "-m",
        "Synthetic base",
    ]);
    let own = Path::new(&f.m.target.root)
        .parent()
        .unwrap()
        .join("target-worktree");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "synthetic-work",
        own.to_str().unwrap(),
    ]);
    std::fs::write(own.join("draft.txt"), "synthetic").unwrap();
    f.pool(|state| {
        let mut mine = assignment(&hosted.worker_id, crate::coordination::Phase::Accepted);
        mine.worktree = Some(own.display().to_string());
        state.assignments.insert("assignment-1".into(), mine);
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
    ops::release(
        &f.m.target_store,
        &mine.id,
        NOW.into(),
        &hosted.worker_id,
        false,
    )
    .unwrap();
    assert_eq!(
        f.safe_point(&sid, &mut tx),
        [B::UncommittedChanges, B::PendingHandoff]
    );
    refused(&f, &mut tx);
    std::fs::remove_file(own.join("draft.txt")).unwrap();
    assert_eq!(f.safe_point(&sid, &mut tx), [B::PendingHandoff]);
    f.pool(|state| {
        let mut finished = assignment(&hosted.worker_id, crate::coordination::Phase::Done);
        finished.worktree = Some(own.display().to_string());
        state.assignments.insert("assignment-1".into(), finished);
    });
    std::fs::remove_file(Path::new(&f.m.target.root).join("draft.txt")).unwrap();
    // A damaged pool file is unknown again: its handoffs, and which worktrees are the
    // worker's (HS2-TRP2MJ).
    let pool = Path::new(&f.m.target.root).join(".git/hotsheet-coordination/state.json");
    let good = std::fs::read(&pool).unwrap();
    std::fs::write(&pool, b"{").unwrap();
    assert_eq!(
        f.safe_point(&sid, &mut tx),
        [B::WorktreeUnknown, B::HandoffUnknown]
    );
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
    let f = Fixture::new(&[1, 2], &mut tx);
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
                        account: None,
                        operation_id: "op-unheard".into(),
                        tool: "codex".into(),
                        model: None,
                        effort: None,
                        role: SessionRole::Worker,
                        origin: hotsheet_ticketing::orchestration::SessionOrigin {
                            owner_checkout_id: owner.clone(),
                            project: f.m.owner.alias.clone(),
                            run_id: None,
                            ticket: None,
                        },
                    },
                    limits: None,
                    actor: None,
                    resources: Vec::new(),
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
    let f = Fixture::new(&[1, 2], &mut tx);
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
                    hand_back: false,
                    hand_back_operation_id: None,
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
                hand_back: false,
                hand_back_operation_id: None,
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
    let f = Fixture::new(&[1, 2], &mut tx);
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
                        account: None,
                        operation_id: "foreign-op".into(),
                        tool: "codex".into(),
                        model: None,
                        effort: None,
                        role: SessionRole::Dispatcher,
                        origin: hotsheet_ticketing::orchestration::SessionOrigin {
                            owner_checkout_id: "elsewhere-0123456789ab".into(),
                            project: "Another project".into(),
                            run_id: None,
                            ticket: None,
                        },
                    },
                    limits: None,
                    actor: None,
                    resources: Vec::new(),
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
    let f = Fixture::new(&[1, 2], &mut tx);
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

/// HS2-WEFJV8: a start sets up the target checkout for its tool before it launches,
/// once per launch; a setup that cannot be made launches nothing and fails the start.
#[test]
fn hs2_wefjv8_a_start_sets_up_the_target_checkout_before_it_launches() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    let owner = f.m.owner.id.clone();
    let view = f.started("op-1", 1, "codex", &mut tx);
    assert_eq!(view.tool_setup, Some(ToolSetup { wrote: 2 }));
    let prepared = f.host.prepared.lock().unwrap().clone();
    // The target's checkout, not the owner's and not a store.
    assert_eq!(prepared, [(f.m.target.root.clone(), "codex".to_owned())]);
    // Asked before the launch.
    assert_eq!(*f.host.prepared_before.lock().unwrap(), [0]);
    // A repeat is the same session: no second setup and no second launch.
    f.started("op-1", 1, "codex", &mut tx);
    assert_eq!(f.host.prepared.lock().unwrap().len(), 1);
    assert_eq!(f.host.launches().len(), 1);

    // A setup that fails: refused, recorded as failed with the reason, nothing ran.
    f.host.setup_failing.store(true, Ordering::SeqCst);
    assert_eq!(
        session_error(f.start("op-2", 1, "codex", json!({}), &mut tx)),
        SessionError::StartFailed
    );
    let failed =
        f.m.runtime
            .session_list(&owner, false)
            .unwrap()
            .into_iter()
            .find(|session| session.operation_id == "op-2")
            .unwrap();
    assert_eq!(failed.state, SessionState::Failed);
    assert_eq!(
        failed.history.last().unwrap().reason.as_deref(),
        Some(TOOL_SETUP_FAILED)
    );
    assert_eq!(f.host.launches().len(), 1);
    // The host forgot the reservation, so a later start under a new id may launch.
    f.host.setup_failing.store(false, Ordering::SeqCst);
    let after = f.started("op-3", 1, "codex", &mut tx);
    assert_eq!(after.session.state, SessionState::Running);
    assert_eq!(*f.host.prepared_before.lock().unwrap(), [0, 1]);
}

fn checkout_at(root: &Path, sources: Value) -> Checkout {
    serde_json::from_value(json!({
        "id": "target-checkout", "root": root, "alias": "target", "sources": sources,
    }))
    .unwrap()
}

/// HS2-WEFJV8: the setup the hosts share is the project's own headless setup, run in
/// the checkout with the checkout's own linked store.
#[test]
fn hs2_wefjv8_target_setup_writes_the_tools_files_into_the_checkout() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let linked = root.path().join("linked-store");
    let serving = root.path().join("serving-store");
    for store in [&linked, &serving] {
        hotsheet_ticketing::FsStore::init_git(
            store,
            &hotsheet_ticketing::StoreMetadata::new("TGT"),
        )
        .unwrap();
    }
    std::fs::create_dir_all(project.join(".git/info")).unwrap();
    std::fs::write(project.join(".git/info/exclude"), "").unwrap();
    let git = json!([{"connection_id":"linked","provider":"git","locator":linked}]);
    let checkout = checkout_at(&project, git.clone());

    // Which store: the linked Git source, else the serving store, else none.
    assert_eq!(setup_store(&checkout, Some(&serving)), Some(linked.clone()));
    let unlinked = checkout_at(&project, json!([]));
    assert_eq!(
        setup_store(&unlinked, Some(&serving)),
        Some(serving.clone())
    );
    assert_eq!(setup_store(&unlinked, None), None);
    let github = checkout_at(
        &project,
        json!([{"connection_id":"gh","provider":"github","locator":"synthetic/repo"}]),
    );
    assert_eq!(setup_store(&github, Some(&serving)), Some(serving.clone()));

    // The built-in Claude plugin: instructions, MCP and hooks land in the checkout.
    let setup = target_tool_setup(&checkout, "claude", Some(&serving), &[]).unwrap();
    assert!(setup.wrote > 0, "{setup:?}");
    assert!(project.join("CLAUDE.md").is_file());
    assert!(project.join(".mcp.json").is_file());
    let mcp = std::fs::read_to_string(project.join(".mcp.json")).unwrap();
    assert!(mcp.contains("hotsheet"), "{mcp}");
    // Nothing was written into either store.
    for store in [&linked, &serving] {
        assert!(!store.join("CLAUDE.md").exists());
        assert!(!store.join(".mcp.json").exists());
    }
    // Repeating it is idempotent.
    let before = std::fs::read_to_string(project.join("CLAUDE.md")).unwrap();
    target_tool_setup(&checkout, "claude", Some(&serving), &[]).unwrap();
    assert_eq!(
        std::fs::read_to_string(project.join("CLAUDE.md")).unwrap(),
        before
    );

    // Refused, writing nothing: an unknown tool, no store at all, or an HS1 store.
    assert_eq!(
        target_tool_setup(&checkout, "no-such-tool", None, &[]),
        Err(SessionHostError::Refused)
    );
    let bare = root.path().join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    assert_eq!(
        target_tool_setup(&checkout_at(&bare, json!([])), "claude", None, &[]),
        Err(SessionHostError::Refused)
    );
    assert!(!bare.join("CLAUDE.md").exists());
    std::fs::create_dir_all(bare.join(".hotsheet/db")).unwrap();
    std::fs::write(bare.join(".hotsheet/db/PG_VERSION"), "16").unwrap();
    assert_eq!(
        target_tool_setup(&checkout_at(&bare, git), "claude", None, &[]),
        Err(SessionHostError::Refused)
    );
    assert!(!bare.join("CLAUDE.md").exists());
}

fn live_session(id: &str, expires_at: i64) -> crate::coordination::Session {
    serde_json::from_value(
        json!({"id": id, "generation": "g1", "expires_at": expires_at,
        "return_route": {"kind": "human_relay"}}),
    )
    .unwrap()
}

/// HS2-2DPGDY: a role prompt the tool was not ready for is tried again only when the
/// host learns of readiness, a bounded number of times, and is then settled.
#[test]
fn hs2_2dpgdy_a_role_prompt_is_retried_on_readiness_and_then_settles() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    let owner = f.m.owner.id.clone();
    // Not ready at launch, nor at the first readiness event; ready at the second.
    f.host
        .push_answers
        .lock()
        .unwrap()
        .extend([PushAttempt::NotReady, PushAttempt::NotReady]);
    let view = f.started("op-1", 1, "codex", &mut tx);
    assert_eq!(view.role_prompt, Some(RolePrompt::Pending));
    assert_eq!(view.session.role_prompt, Some(RolePrompt::Pending));
    let terminal = view.session.hosted.as_ref().unwrap().session_id.clone();
    let tries = || f.host.joined.lock().unwrap().len();
    assert_eq!(tries(), 1);
    // A terminal no orchestrator started changes nothing and asks nothing.
    assert_eq!(
        f.m.runtime
            .hosted_role_retry("not-a-session", &mut tx)
            .unwrap(),
        None
    );
    assert_eq!(tries(), 1);
    assert_eq!(
        f.m.runtime.hosted_role_retry(&terminal, &mut tx).unwrap(),
        Some(RolePrompt::Pending)
    );
    assert_eq!(
        f.m.runtime.hosted_role_retry(&terminal, &mut tx).unwrap(),
        Some(RolePrompt::Delivered)
    );
    assert_eq!(tries(), 3);
    // Settled: later readiness events push nothing more.
    for _ in 0..3 {
        assert_eq!(
            f.m.runtime.hosted_role_retry(&terminal, &mut tx).unwrap(),
            Some(RolePrompt::Delivered)
        );
    }
    assert_eq!(tries(), 3);
    // The orchestrator learns the answer on its next read, once.
    let synced =
        f.m.runtime
            .session_sync(&owner, &view.session.session_id, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(synced.session.role_prompt, Some(RolePrompt::Delivered));

    // A tool that is never ready: four tries in all, then failed for good.
    f.host
        .push_answers
        .lock()
        .unwrap()
        .extend([PushAttempt::NotReady; 8]);
    let never = f.started("op-2", 1, "codex", &mut tx);
    let terminal = never.session.hosted.as_ref().unwrap().session_id.clone();
    let mut last = never.role_prompt;
    for _ in 0..6 {
        last = f.m.runtime.hosted_role_retry(&terminal, &mut tx).unwrap();
    }
    assert_eq!(last, Some(RolePrompt::Failed));
    assert_eq!(
        tries(),
        3 + hotsheet_ticketing::orchestration::MAX_ROLE_ATTEMPTS as usize
    );
}

#[test]
fn hs2_2dpgdy_a_prompt_at_launch_or_an_unsupported_tool_asks_nothing_more() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    // The plugin takes the prompt on its command line: nothing is pushed.
    f.host.prompt_at_launch.store(true, Ordering::SeqCst);
    let at_launch = f.started("op-1", 1, "codex", &mut tx);
    assert_eq!(at_launch.role_prompt, Some(RolePrompt::AtLaunch));
    assert!(f.host.joined.lock().unwrap().is_empty());
    assert!(
        f.host.launches()[0]
            .role_prompt
            .contains("`hotsheet-worker`")
    );
    let terminal = at_launch
        .session
        .hosted
        .as_ref()
        .unwrap()
        .session_id
        .clone();
    assert_eq!(
        f.m.runtime.hosted_role_retry(&terminal, &mut tx).unwrap(),
        Some(RolePrompt::AtLaunch)
    );
    assert!(f.host.joined.lock().unwrap().is_empty());
    // A tool that cannot take messages: unsupported, never retried.
    f.host.prompt_at_launch.store(false, Ordering::SeqCst);
    f.host
        .push_answers
        .lock()
        .unwrap()
        .push_back(PushAttempt::Unsupported);
    let unsupported = f.started("op-2", 2, "claude", &mut tx);
    assert_eq!(unsupported.role_prompt, Some(RolePrompt::Unsupported));
    let terminal = unsupported
        .session
        .hosted
        .as_ref()
        .unwrap()
        .session_id
        .clone();
    f.m.runtime.hosted_role_retry(&terminal, &mut tx).unwrap();
    assert_eq!(f.host.joined.lock().unwrap().len(), 1);
    // A rejected message fails the delivery.
    f.host
        .push_answers
        .lock()
        .unwrap()
        .push_back(PushAttempt::Rejected);
    assert_eq!(
        f.started("op-3", 1, "codex", &mut tx).role_prompt,
        Some(RolePrompt::Failed)
    );
}

/// HS2-2DPGDY: joining is what the target's pool shows for the session's worker id
/// in its role, learned once.
#[test]
fn hs2_2dpgdy_joining_is_confirmed_from_the_targets_pool() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    let owner = f.m.owner.id.clone();
    let view = f.started("op-1", 1, "codex", &mut tx);
    let session_id = view.session.session_id.clone();
    let worker_id = view.session.hosted.as_ref().unwrap().worker_id.clone();
    let sync = |tx: &mut Transaction<'_>| {
        f.m.runtime
            .session_sync(&owner, &session_id, Some(&worker()), tx)
            .unwrap()
            .session
    };
    let listing = |tx: &mut Transaction<'_>| {
        f.m.runtime
            .hosted_sessions(&f.m.target.id, Some("op-1"), false, tx)
            .unwrap()
            .sessions[0]
            .joined
    };
    // Not a git checkout: the pool cannot be read, so nothing is concluded.
    assert_eq!(listing(&mut tx), None);
    assert_eq!(sync(&mut tx).joined_at_ms, None);
    // A pool without the session; then someone else; then it, expired.
    f.git(&["init", "-q"]);
    assert_eq!(listing(&mut tx), Some(false));
    f.pool(|state| {
        state
            .workers
            .insert("someone".into(), live_session("someone", 10_000));
    });
    assert_eq!(listing(&mut tx), Some(false));
    f.pool(|state| {
        state
            .workers
            .insert(worker_id.clone(), live_session(&worker_id, 0));
    });
    assert_eq!(listing(&mut tx), Some(false));
    assert_eq!(sync(&mut tx).joined_at_ms, None);
    // Registered and live: joined, learned once, with one event.
    f.pool(|state| {
        state
            .workers
            .insert(worker_id.clone(), live_session(&worker_id, 10_000));
    });
    assert_eq!(listing(&mut tx), Some(true));
    let joined = sync(&mut tx);
    assert_eq!(joined.joined_at_ms, Some(200));
    assert_eq!(sync(&mut tx), joined);
    // Leaving the pool later does not unlearn it.
    f.pool(|_| {});
    assert_eq!(listing(&mut tx), Some(false));
    assert_eq!(sync(&mut tx).joined_at_ms, Some(200));
}

#[test]
fn hs2_2dpgdy_pool_joined_matches_the_role_slot_and_the_lease() {
    let worker = "codex-T1";
    let mut state = crate::coordination::State::new("p".into(), "s".into());
    for role in SessionRole::ALL {
        assert!(!pool_joined(&state, role, worker, 5));
    }
    state
        .workers
        .insert(worker.into(), live_session(worker, 10));
    state.dispatcher = Some(live_session("other", 10));
    state.integrator = Some(live_session(worker, 4));
    // (role, now, joined)
    for (role, now, expected) in [
        (SessionRole::Worker, 5, true),
        (SessionRole::Worker, 10, false),
        (SessionRole::Dispatcher, 5, false),
        (SessionRole::Integrator, 3, true),
        (SessionRole::Integrator, 5, false),
    ] {
        assert_eq!(
            pool_joined(&state, role, worker, now),
            expected,
            "{role:?} {now}"
        );
    }
    state.dispatcher = Some(live_session(worker, 10));
    assert!(pool_joined(&state, SessionRole::Dispatcher, worker, 5));
    // A worker entry keyed by the id but naming another session is not it.
    state
        .workers
        .insert(worker.into(), live_session("other", 10));
    assert!(!pool_joined(&state, SessionRole::Worker, worker, 5));
}

/// HS2-75Q254: reconciliation rediscovers a run's sessions on their host, adopts a
/// surviving one, tells a deliberate wind-down from a loss, names a duplicate, and
/// winds the sessions of a finished run down at a safe point.
#[test]
fn hs2_75q254_reconciliation_reads_adopts_and_winds_down_the_sessions_of_a_run() {
    use hotsheet_ticketing::TicketPatch;
    use hotsheet_ticketing::orchestration::{Disposition, ExternalSource, RunState, Verification};
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    // A clean work tree, so a session between turns is at a safe point.
    f.git(&["init", "-q"]);
    let owner = f.m.owner.id.clone();
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
                    "worker_id": "worker-a",
                }))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap()
            .run;
    assert_eq!(run.state, RunState::Dispatched);
    let reconcile = |tx: &mut Transaction<'_>| {
        f.m.runtime
            .run_reconcile(&owner, &run.run_id, Some(&worker()), tx)
            .unwrap()
    };
    let session = |view: &RunView| super::recovery::finding(view, ExternalSource::Session);
    let disposition = |view: &RunView| super::recovery::disposition(view);
    let for_run = json!({"run_id": run.run_id});
    // No session yet.
    let view = reconcile(&mut tx);
    assert_eq!(
        session(&view),
        (Verification::Verified, Some("no_session".into()))
    );
    assert!(
        f.m.runtime
            .run_session_holder(&owner, &run.run_id)
            .unwrap()
            .is_none()
    );
    // A running session is the run's: verified, and it holds the run's work.
    let first = f
        .start("session-1", 1, "codex", for_run.clone(), &mut tx)
        .unwrap()
        .session;
    let view = reconcile(&mut tx);
    assert_eq!(session(&view), (Verification::Verified, None));
    assert_eq!(disposition(&view), Disposition::Continue);
    assert_eq!(
        f.m.runtime
            .run_session_holder(&owner, &run.run_id)
            .unwrap()
            .map(|session| session.session_id),
        Some(first.session_id.clone())
    );
    // It crashes while the run is being worked: rediscovered as lost, missing work.
    f.host
        .crash(&first.hosted.as_ref().unwrap().session_id.clone());
    let view = reconcile(&mut tx);
    assert_eq!(
        session(&view),
        (
            Verification::Mismatched,
            Some("recorded_work_missing".into())
        )
    );
    assert_eq!(disposition(&view), Disposition::NeedsAttention);
    assert_eq!(
        f.m.runtime
            .session(&owner, &first.session_id)
            .unwrap()
            .state,
        SessionState::Lost
    );
    assert!(
        f.m.runtime
            .run_session_holder(&owner, &run.run_id)
            .unwrap()
            .is_none()
    );
    // The target ticket moves to final testing: the run waits on CI and needed no
    // session for that, so the same loss is no longer missing work.
    hotsheet_ticketing::ops::update(
        &f.m.target_store,
        &work.id,
        NOW.into(),
        TicketPatch {
            started_phase: Some(Some(hotsheet_model::StartedPhase::FinalTesting)),
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..2 {
        let view = reconcile(&mut tx);
        assert_eq!(
            session(&view),
            (Verification::Verified, Some("waiting_on_gate".into()))
        );
        assert_eq!(disposition(&view), Disposition::Continue);
    }
    // HS2-N6ZATC: the lost session's exit released its worker's claim, so the run reads
    // intervened. It still waits on CI, so the loss is still no missing work.
    ops::release(&f.m.target_store, &work.id, NOW.into(), "worker-a", false).unwrap();
    let released =
        f.m.runtime
            .run_sync(&owner, &run.run_id, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(released.run.state, RunState::Intervened);
    let view = reconcile(&mut tx);
    assert_eq!(
        session(&view),
        (Verification::Verified, Some("waiting_on_gate".into()))
    );
    ops::claim_with_eta(
        &f.m.target_store,
        &work.id,
        &NOW.into(),
        "2030-01-01T00:00:00Z".into(),
        "worker-a",
        None,
        None,
    )
    .unwrap();
    let back =
        f.m.runtime
            .run_sync(&owner, &run.run_id, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(back.run.state, RunState::Dispatched);
    // A replacement is the run's again; wound down at a safe point it is never missing
    // work, whether or not the run waits (the case HS2-95SWQ4 found).
    let second = f
        .start(
            "session-2",
            1,
            "codex",
            json!({"run_id": run.run_id, "replaces": first.session_id}),
            &mut tx,
        )
        .unwrap()
        .session;
    assert_eq!(session(&reconcile(&mut tx)), (Verification::Verified, None));
    let stopped = f
        .stop(&second.session_id, false, &worker(), &mut tx)
        .unwrap();
    assert_eq!(stopped.session.state, SessionState::Stopped);
    hotsheet_ticketing::ops::update(
        &f.m.target_store,
        &work.id,
        NOW.into(),
        TicketPatch {
            started_phase: Some(Some(hotsheet_model::StartedPhase::Working)),
            ..Default::default()
        },
    )
    .unwrap();
    let view = reconcile(&mut tx);
    assert_eq!(
        session(&view),
        (Verification::Verified, Some("session_stopped".into()))
    );
    assert_eq!(disposition(&view), Disposition::Continue);
    // Two sessions running for one run: the older one is a duplicate the run does
    // not name. HS2-DG0YPA: the runtime refuses to make one, so the second is
    // planted in the records, as an older build or another process could have left it.
    let third = f
        .start("session-3", 1, "codex", for_run.clone(), &mut tx)
        .unwrap()
        .session;
    assert_eq!(
        session_error(f.start("session-4", 1, "codex", for_run.clone(), &mut tx)),
        SessionError::RunHeld
    );
    let fourth = f.started("session-4", 1, "codex", &mut tx).session;
    f.plant_run(&fourth.session_id, &run.run_id);
    let view = reconcile(&mut tx);
    assert_eq!(
        session(&view),
        (Verification::Mismatched, Some("different_live_work".into()))
    );
    // The host cannot be asked: nothing is concluded and the run is held.
    f.host.down.store(true, Ordering::SeqCst);
    let view = reconcile(&mut tx);
    assert_eq!(
        session(&view),
        (Verification::Unverifiable, Some("session_unknown".into()))
    );
    assert_eq!(disposition(&view), Disposition::HoldUnverified);
    assert_eq!(
        f.m.runtime
            .session(&owner, &third.session_id)
            .unwrap()
            .state,
        SessionState::Running
    );
    f.host.down.store(false, Ordering::SeqCst);
    // A person takes the newest over; then the target ticket is finished. The run is
    // over: its own session is wound down at a safe point, the person's is left alone.
    f.m.runtime
        .session_takeover(
            &owner,
            &fourth.session_id,
            TakeoverSession { pause: false },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let stops = f.host.stops.load(Ordering::SeqCst);
    hotsheet_ticketing::ops::update(
        &f.m.target_store,
        &work.id,
        NOW.into(),
        TicketPatch {
            status: Some(hotsheet_model::Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
    let view = reconcile(&mut tx);
    assert_eq!(view.run.state, RunState::Completed);
    assert_eq!(f.host.stops.load(Ordering::SeqCst), stops + 1);
    assert_eq!(
        f.m.runtime
            .session(&owner, &third.session_id)
            .unwrap()
            .state,
        SessionState::Stopped
    );
    assert_eq!(
        f.m.runtime
            .session(&owner, &fourth.session_id)
            .unwrap()
            .state,
        SessionState::Running
    );
}

/// HS2-DG0YPA: a dispatched run of target 1 for the fixture's orchestration ticket.
fn dispatched_run(
    f: &Fixture,
    operation: &str,
    tx: &mut Transaction<'_>,
) -> hotsheet_ticketing::orchestration::Run {
    let orchestration = f.m.orchestration_ticket("Coordinate");
    let work = f.target_ticket("Do the work");
    f.m.runtime
        .run_dispatch(
            &f.m.owner.id,
            serde_json::from_value(json!({
                "ticket": {"connection_id": f.m.owner_connection(),
                    "native_id": orchestration.id.to_string()},
                "target_id": id(1), "connection_id": f.m.target_connection(),
                "native_id": work.id.to_string(), "operation_id": operation,
                "worker_id": "worker-a",
            }))
            .unwrap(),
            Some(&worker()),
            tx,
        )
        .unwrap()
        .run
}

/// HS2-DG0YPA: a start for a run another live session still holds is refused with a
/// typed code, however often it is repeated; a holder that crashed is learned from
/// its host first and the start proceeds; a holder whose host cannot be asked keeps
/// the run; a person's holder keeps it too.
#[test]
fn hs2_dg0ypa_a_second_start_for_a_held_run_is_refused_until_its_holder_is_gone() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    let owner = f.m.owner.id.clone();
    let run = dispatched_run(&f, "run-1", &mut tx);
    let for_run = json!({"run_id": run.run_id});
    let holder = |tx: &mut Transaction<'_>| {
        let _ = tx;
        f.m.runtime
            .run_session_holder(&owner, &run.run_id)
            .unwrap()
            .map(|session| session.session_id)
    };
    let first = f
        .start("session-1", 1, "codex", for_run.clone(), &mut tx)
        .unwrap()
        .session;
    assert_eq!(holder(&mut tx), Some(first.session_id.clone()));
    // Repeated: refused every time, nothing recorded or launched.
    for _ in 0..3 {
        assert_eq!(
            session_error(f.start("session-2", 1, "codex", for_run.clone(), &mut tx)),
            SessionError::RunHeld
        );
    }
    assert_eq!(f.host.launches().len(), 1);
    assert_eq!(f.m.runtime.session_list(&owner, false).unwrap().len(), 1);
    // The holder's own start repeated is still itself.
    assert_eq!(
        f.start("session-1", 1, "codex", for_run.clone(), &mut tx)
            .unwrap()
            .session
            .session_id,
        first.session_id
    );
    // The holder crashes while its host cannot be asked: nothing is concluded, the
    // run stays held.
    let terminal = first.hosted.as_ref().unwrap().session_id.clone();
    f.host.crash(&terminal);
    f.host.down.store(true, Ordering::SeqCst);
    assert_eq!(
        session_error(f.start("session-2", 1, "codex", for_run.clone(), &mut tx)),
        SessionError::RunHeld
    );
    f.host.down.store(false, Ordering::SeqCst);
    // Its host answers: the crash is learned and the replacement takes the run.
    let second = f
        .start(
            "session-2",
            1,
            "codex",
            json!({"run_id": run.run_id, "replaces": first.session_id}),
            &mut tx,
        )
        .unwrap()
        .session;
    assert_eq!(second.state, SessionState::Running);
    assert_eq!(
        f.m.runtime
            .session(&owner, &first.session_id)
            .unwrap()
            .state,
        SessionState::Lost
    );
    assert_eq!(holder(&mut tx), Some(second.session_id.clone()));
    assert_eq!(f.host.launches().len(), 2);
    // Out of order: the first start repeated late is the same lost session.
    assert_eq!(
        f.start("session-1", 1, "codex", for_run.clone(), &mut tx)
            .unwrap()
            .session
            .state,
        SessionState::Lost
    );
    // A person takes the replacement over: still live work, still held.
    f.m.runtime
        .session_takeover(
            &owner,
            &second.session_id,
            TakeoverSession { pause: false },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        session_error(f.start("session-3", 1, "codex", for_run.clone(), &mut tx)),
        SessionError::RunHeld
    );
    // A free session cannot be linked to the held run either.
    let free = f.started("session-free", 1, "codex", &mut tx).session;
    assert_eq!(
        f.m.runtime
            .session_link(&owner, &free.session_id, &run.run_id, &worker(), &mut tx)
            .unwrap_err()
            .code(),
        "session_run_held"
    );
    // The person stops it: the run is empty and the next start takes it.
    f.stop(&second.session_id, true, &human(), &mut tx).unwrap();
    assert_eq!(holder(&mut tx), None);
    let third = f
        .start("session-3", 1, "codex", for_run.clone(), &mut tx)
        .unwrap()
        .session;
    assert_eq!(holder(&mut tx), Some(third.session_id));
}

/// HS2-WHR4GS: the late start that HS2-7VV79Q's immediate `never_started` left open.
fn late_start(f: &Fixture, operation: &str) -> hotsheet_ticketing::orchestration::NewHostedSession {
    hotsheet_ticketing::orchestration::NewHostedSession {
        account: None,
        operation_id: operation.into(),
        tool: "codex".into(),
        model: None,
        effort: None,
        role: SessionRole::Worker,
        origin: hotsheet_ticketing::orchestration::SessionOrigin {
            owner_checkout_id: f.m.owner.id.clone(),
            project: f.m.owner.alias.clone(),
            run_id: None,
            ticket: None,
        },
    }
}

fn hosted_start(f: &Fixture, operation: &str, tx: &mut Transaction<'_>) -> HostedStartReply {
    f.m.runtime
        .hosted_session_start(
            &f.m.target.id,
            HostedSessionStart {
                session: late_start(f, operation),
                limits: None,
                actor: None,
                resources: Vec::new(),
            },
            tx,
        )
        .unwrap()
}

fn hosted_stop(f: &Fixture, operation: &str, tx: &mut Transaction<'_>) -> HostedStopReply {
    f.m.runtime
        .hosted_session_stop(
            &f.m.target.id,
            HostedSessionStop {
                operation_id: operation.into(),
                force: false,
                actor: Some(worker()),
            },
            tx,
        )
        .unwrap()
}

#[test]
fn hs2_whr4gs_a_start_that_arrives_after_its_stop_launches_nothing() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    let owner = f.m.owner.id.clone();
    f.git(&["init", "-q"]);
    // The orchestrator recorded a start its host never answered.
    let admission = f.m.home.join("orchestration-admission.json");
    std::fs::write(&admission, b"{").unwrap();
    assert!(f.start("op-1", 1, "codex", json!({}), &mut tx).is_err());
    std::fs::remove_file(&admission).unwrap();
    let pending = f.m.runtime.session_list(&owner, true).unwrap();
    assert_eq!(pending[0].state, SessionState::Requested);
    // Its stop finds no record on the host, which fences the operation, and the
    // orchestrator ends it as never started.
    let ended = f
        .stop(&pending[0].session_id, false, &worker(), &mut tx)
        .unwrap();
    assert_eq!(
        ended.session.history.last().unwrap().reason.as_deref(),
        Some("never_started")
    );
    let fences = HostedSessionRegistry::new(&f.m.home)
        .withdrawn(hotsheet_ticketing::Clock::now_ms(&Clock))
        .unwrap();
    assert_eq!(fences.len(), 1);
    assert_eq!(fences[0].operation_id, "op-1");
    // The original start request, still in transit, reaches the host now: refused,
    // nothing launched, nothing reserved. Repeated, out of order, it stays refused.
    for _ in 0..2 {
        let late = hosted_start(&f, "op-1", &mut tx);
        assert_eq!(late.outcome, HostedStartOutcome::Refused);
        assert_eq!(late.reason.as_deref(), Some("session_withdrawn"));
        assert!(late.session.is_none());
    }
    assert!(f.host.launches().is_empty());
    assert!(
        HostedSessionRegistry::new(&f.m.home)
            .get(&f.m.target.id, "op-1")
            .unwrap()
            .is_none()
    );
    // The orchestrator's own record keeps answering with the stopped session.
    assert_eq!(
        f.started("op-1", 1, "codex", &mut tx).session.state,
        SessionState::Stopped
    );
    // Another operation starts normally on the same host.
    assert_eq!(
        f.started("op-2", 1, "codex", &mut tx).session.state,
        SessionState::Running
    );
    assert_eq!(f.host.launches().len(), 1);
}

#[test]
fn hs2_whr4gs_a_stop_while_the_start_launches_stops_the_launched_terminal() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    // Interleaved: the start reserved its terminal; the stop arrives before the launch
    // and is told nothing runs.
    let home = f.m.home.clone();
    let checkout = f.m.target.id.clone();
    let answer = Arc::new(Mutex::new(None));
    let seen = answer.clone();
    f.host.before_launch.arm(move || {
        let withdrawal = HostedSessionRegistry::new(&home)
            .withdraw(&checkout, "op-1", hotsheet_ticketing::Clock::now_ms(&Clock))
            .unwrap();
        *seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(withdrawal);
    });
    let reply = hosted_start(&f, "op-1", &mut tx);
    assert_eq!(
        *answer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        Some(hotsheet_ticketing::orchestration::Withdrawal::Released)
    );
    // The launch happened, found its reservation gone and was stopped again: nothing
    // runs untracked, and the start says why.
    assert_eq!(reply.outcome, HostedStartOutcome::Refused);
    assert_eq!(reply.reason.as_deref(), Some("session_withdrawn"));
    assert_eq!(f.host.launches().len(), 1);
    assert_eq!(f.host.stops.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.sessions(&f.m.target), Some(Vec::new()));
    // A repeat of that start is refused before anything launches.
    assert_eq!(
        hosted_start(&f, "op-1", &mut tx).reason.as_deref(),
        Some("session_withdrawn")
    );
    assert_eq!(f.host.launches().len(), 1);
}

#[test]
fn hs2_whr4gs_hosted_stop_sequences_fence_only_what_never_launched() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    f.git(&["init", "-q"]);
    let registry = HostedSessionRegistry::new(&f.m.home);
    // Unknown: refused as not found, fenced, and the reply says so; repeated alike.
    for _ in 0..2 {
        let reply = hosted_stop(&f, "op-unknown", &mut tx);
        assert!(!reply.stopped && reply.fenced);
        assert_eq!(reply.reason.as_deref(), Some("session_not_found"));
    }
    // Launched: stopped at its safe point and never fenced, so its own operation id
    // still answers with the same, now ended, session.
    let started = hosted_start(&f, "op-live", &mut tx);
    assert_eq!(started.outcome, HostedStartOutcome::Started);
    let stopped = hosted_stop(&f, "op-live", &mut tx);
    assert!(stopped.stopped && !stopped.fenced, "{stopped:?}");
    assert_eq!(stopped.reason.as_deref(), Some("wound_down"));
    let again = hosted_stop(&f, "op-live", &mut tx);
    assert!(again.stopped && !again.fenced);
    assert_eq!(
        hosted_start(&f, "op-live", &mut tx).outcome,
        HostedStartOutcome::Existing
    );
    let fenced: Vec<_> = registry
        .withdrawn(hotsheet_ticketing::Clock::now_ms(&Clock))
        .unwrap()
        .into_iter()
        .map(|fence| fence.operation_id)
        .collect();
    assert_eq!(fenced, ["op-unknown"]);
    // The wire shape: `fenced` is omitted when false, and a reply from a host without
    // the fence reads as unfenced.
    let wire = serde_json::to_value(&stopped).unwrap();
    assert!(wire.get("fenced").is_none());
    let older: HostedStopReply =
        serde_json::from_value(json!({"stopped": false, "reason": "session_not_found"})).unwrap();
    assert!(!older.fenced);
}

/// HS2-PFHRS9: a start for a tool account carries the account to the session's host,
/// which keeps it so the session's limit signals are attributed to that account. A
/// start without an account reaches the host without one.
#[test]
fn hs2_pfhrs9_a_start_for_an_account_carries_it_to_the_host() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    let mut body = policy();
    body.tools[0].accounts = vec![hotsheet_ticketing::orchestration::AccountPolicy {
        account: "team-a".into(),
        max_sessions: 2,
        budget_cents_per_day: None,
    }];
    f.set_policy(body, &mut tx);
    let hosts = hotsheet_ticketing::orchestration::HostedSessionRegistry::new(&f.m.home);
    let started = f
        .start("op-team", 1, "codex", json!({"account": "team-a"}), &mut tx)
        .unwrap();
    assert_eq!(started.session.account.as_deref(), Some("team-a"));
    let terminal = started.session.hosted.clone().unwrap().session_id;
    assert_eq!(
        hosts.account_of(&terminal).unwrap().as_deref(),
        Some("team-a")
    );
    let plain = f
        .start("op-plain", 2, "claude", json!({}), &mut tx)
        .unwrap();
    let terminal = plain.session.hosted.clone().unwrap().session_id;
    assert_eq!(hosts.account_of(&terminal).unwrap(), None);
}

/// HS2-TRP2MJ: each hosted session's `uncommitted_changes` counts only the worktrees it
/// owns: a worker the ones its unfinished assignments name (or, recorded before the
/// field, the one inferred from its branch), an integrator the main checkout, a
/// dispatcher nothing. Real git worktrees; synthetic content.
#[test]
fn hs2_trp2mj_each_session_is_held_only_by_its_own_worktrees() {
    use SafePointBlocker as B;
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1], &mut tx);
    f.git(&["init", "-q"]);
    f.git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@example.test",
        "commit",
        "--allow-empty",
        "-q",
        "-m",
        "Synthetic base",
    ]);
    // Room for two workers and both role sessions on the target.
    let mut roomy = policy();
    roomy.targets[0].max_sessions = 4;
    f.set_policy(roomy, &mut tx);
    let a = f.started("op-a", 1, "codex", &mut tx);
    let b = f.started("op-b", 1, "codex", &mut tx);
    let integrator = f
        .start("op-i", 1, "codex", json!({"role": "integrator"}), &mut tx)
        .unwrap();
    let dispatcher = f
        .start("op-d", 1, "codex", json!({"role": "dispatcher"}), &mut tx)
        .unwrap();
    let worker_of = |view: &SessionView| view.session.hosted.clone().unwrap().worker_id;
    let (worker_a, worker_b) = (worker_of(&a), worker_of(&b));
    let parent = Path::new(&f.m.target.root).parent().unwrap().to_owned();
    let (tree_a, tree_b) = (parent.join("wt-a"), parent.join("wt-b"));
    f.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "tgt-1-fix",
        tree_a.to_str().unwrap(),
    ]);
    f.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "other-work",
        tree_b.to_str().unwrap(),
    ]);
    // A's assignment predates the field (inferred from its branch); B's names its tree.
    let write_pool = |slug_a: &str, tree_of_a: Option<&Path>| {
        f.pool(|state| {
            let mut first = assignment(&worker_a, crate::coordination::Phase::Accepted);
            first.slug = slug_a.into();
            first.worktree = tree_of_a.map(|path| path.display().to_string());
            let mut second = assignment(&worker_b, crate::coordination::Phase::Accepted);
            second.id = "assignment-2".into();
            second.slug = "TGT-2".into();
            second.worktree = Some(tree_b.display().to_string());
            state.assignments.insert(first.id.clone(), first);
            state.assignments.insert(second.id.clone(), second);
        });
    };
    write_pool("TGT-1", None);
    let ids = [
        a.session.session_id.clone(),
        b.session.session_id.clone(),
        integrator.session.session_id.clone(),
        dispatcher.session.session_id.clone(),
    ];
    let points = |tx: &mut Transaction<'_>| -> Vec<bool> {
        ids.iter()
            .map(|id| f.safe_point(id, tx).contains(&B::UncommittedChanges))
            .collect()
    };
    // Clean everywhere: nobody is held by uncommitted work.
    assert_eq!(points(&mut tx), [false, false, false, false]);
    // B is busy in its own tree: only B is held, not A, the integrator or the dispatcher.
    std::fs::write(tree_b.join("draft.txt"), "synthetic").unwrap();
    assert_eq!(points(&mut tx), [false, true, false, false]);
    // A person edits the main checkout: only the integrator sees it.
    std::fs::write(Path::new(&f.m.target.root).join("person.txt"), "synthetic").unwrap();
    assert_eq!(points(&mut tx), [false, true, true, false]);
    // A's untracked file in the tree its branch names: the fallback holds A.
    std::fs::write(tree_a.join("new.txt"), "synthetic").unwrap();
    assert_eq!(points(&mut tx), [true, true, true, false]);
    // Neither field nor a matching branch: A owns no tree.
    write_pool("TGT-404", None);
    assert_eq!(points(&mut tx), [false, true, true, false]);
    // The field set, even to another tree: it wins over inference.
    write_pool("TGT-1", Some(&tree_b));
    assert_eq!(points(&mut tx), [true, true, true, false]);
    // Repeated reads answer the same.
    assert_eq!(points(&mut tx), [true, true, true, false]);
    // B's work committed in its tree, the person's file gone: only A's pointer to B's
    // (now clean) tree remains, and nothing holds anyone.
    std::fs::remove_file(tree_b.join("draft.txt")).unwrap();
    std::fs::remove_file(Path::new(&f.m.target.root).join("person.txt")).unwrap();
    assert_eq!(points(&mut tx), [false, false, false, false]);
}

/// HS2-CRBR8X: the hosting machine's journal of one checkout, as (event, outcome,
/// reason) codes in order.
fn host_journal(f: &Fixture) -> Vec<hotsheet_ticketing::orchestration::HostedSessionAuditRecord> {
    f.m.runtime
        .hosted_audit(&f.m.target.id, 0, 100)
        .unwrap()
        .page
        .entries
        .into_iter()
        .map(|entry| entry.record)
        .collect()
}

fn journal_codes(f: &Fixture) -> Vec<(String, AuditOutcome, Option<String>)> {
    host_journal(f)
        .into_iter()
        .map(|record| {
            (
                serde_json::to_value(record.event)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned(),
                record.outcome,
                record.reason,
            )
        })
        .collect()
}

#[test]
fn hs2_crbr8x_the_host_journals_starts_stops_refusals_and_ends_once_each() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    let device = format!("cert-{}", "c0".repeat(32));
    let remote = f.m.runtime.clone().with_requester(device.clone());
    let start = |operation: &str, tx: &mut Transaction<'_>| {
        remote
            .hosted_session_start(
                &f.m.target.id,
                HostedSessionStart {
                    session: late_start(&f, operation),
                    limits: None,
                    actor: Some(MutationActor::parse("ai", Some("scheduler-1".into())).unwrap()),
                    resources: Vec::new(),
                },
                tx,
            )
            .unwrap()
    };
    // Nothing hosted yet: an empty journal, and no file is created by reading it.
    assert!(host_journal(&f).is_empty());
    assert!(!f.m.home.join("hosted-session-audit").exists());

    // A remote orchestrator's start is journaled with who asked and what launched.
    let started = start("op-1", &mut tx);
    assert_eq!(started.outcome, HostedStartOutcome::Started);
    let terminal = started.session.as_ref().unwrap().session.session_id.clone();
    let entries = host_journal(&f);
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.event, HostedSessionEvent::Start);
    assert_eq!(entry.outcome, AuditOutcome::Succeeded);
    assert_eq!(entry.operation_id, "op-1");
    assert_eq!(entry.session_id.as_deref(), Some(terminal.as_str()));
    assert_eq!(entry.tool.as_deref(), Some("codex"));
    assert_eq!(
        entry.origin_checkout_id.as_deref(),
        Some(f.m.owner.id.as_str())
    );
    assert_eq!(entry.requester.as_deref(), Some(device.as_str()));
    assert_eq!(entry.actor_role.as_deref(), Some("ai"));
    assert_eq!(entry.actor_id.as_deref(), Some("scheduler-1"));
    assert!(entry.at_ms > 0);
    // The same operation again launched nothing new and is not journaled again.
    for _ in 0..2 {
        assert_eq!(start("op-1", &mut tx).outcome, HostedStartOutcome::Existing);
    }
    assert_eq!(host_journal(&f).len(), 1);

    // The owner turns hosting off: `max_sessions: 0` refuses, and the refusal is
    // journaled with its reason. Nothing launched.
    let admission = f.m.home.join("orchestration-admission.json");
    std::fs::write(
        &admission,
        br#"{"schema_version":1,"admission":{"max_sessions":0}}"#,
    )
    .unwrap();
    let refused = start("op-2", &mut tx);
    assert_eq!(refused.outcome, HostedStartOutcome::Refused);
    let reason = refused.reason.clone().unwrap();
    // The admission's own code for a machine that allows no session.
    assert_eq!(reason, "session_cap_reached");
    assert_eq!(f.host.launches().len(), 1);
    std::fs::remove_file(&admission).unwrap();

    // A stop the host cannot confirm as safe (its worktrees are unknown) is refused,
    // and the refusal is journaled.
    let unsafe_stop = hosted_stop(&f, "op-1", &mut tx);
    assert_eq!(unsafe_stop.reason.as_deref(), Some("session_not_safe"));
    // The orchestrator winds the first session down at a safe point.
    f.git(&["init", "-q"]);
    let stopped = hosted_stop(&f, "op-1", &mut tx);
    assert!(stopped.stopped, "{stopped:?}");
    // Stopping an ended session again stops nothing and is not journaled again.
    assert!(hosted_stop(&f, "op-1", &mut tx).stopped);
    // A stop of an operation the host never saw is refused and journaled.
    assert!(!hosted_stop(&f, "op-unknown", &mut tx).stopped);

    // A session whose terminal went away is journaled as ended once, however many
    // readers notice it.
    let lost = start("op-3", &mut tx);
    let lost_terminal = lost.session.unwrap().session.session_id;
    f.host.crash(&lost_terminal);
    for _ in 0..3 {
        f.m.runtime
            .hosted_sessions(&f.m.target.id, None, false, &mut tx)
            .unwrap();
    }

    assert_eq!(
        journal_codes(&f),
        [
            ("start".into(), AuditOutcome::Succeeded, None),
            ("start".into(), AuditOutcome::Denied, Some(reason)),
            (
                "stop".into(),
                AuditOutcome::Denied,
                Some("session_not_safe".into())
            ),
            (
                "stop".into(),
                AuditOutcome::Succeeded,
                Some("wound_down".into())
            ),
            (
                "stop".into(),
                AuditOutcome::Denied,
                Some("session_not_found".into())
            ),
            ("start".into(), AuditOutcome::Succeeded, None),
            (
                "end".into(),
                AuditOutcome::Succeeded,
                Some("host_lost".into())
            ),
        ]
    );
    let entries = host_journal(&f);
    // This machine's own requests are `local`; an end nobody requested names nobody.
    assert_eq!(entries[3].requester.as_deref(), Some("local"));
    assert_eq!(entries[3].session_id.as_deref(), Some(terminal.as_str()));
    assert_eq!(entries[3].actor_id.as_deref(), Some("worker-1"));
    assert_eq!(entries[4].requester.as_deref(), Some("local"));
    assert_eq!(entries[5].requester.as_deref(), Some(device.as_str()));
    assert_eq!(entries[6].requester, None);
    assert_eq!(
        entries[6].session_id.as_deref(),
        Some(lost_terminal.as_str())
    );

    // The journal outlives the process: a new runtime on the same home reads the same
    // history through the machine-local route, and the cursor replays without loss.
    let restarted = f.m.runtime.clone();
    let page: Value = restarted
        .request(
            "GET",
            &format!(
                "/checkouts/{}/orchestration/hosted-audit?after=2&limit=2",
                f.m.target.id
            ),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    let seqs: Vec<u64> = page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["seq"].as_u64().unwrap())
        .collect();
    assert_eq!(seqs, [3, 4]);
    assert_eq!(page["more"], true);
    assert_eq!(page["cursor"], 4);
    assert!(crate::orchestration::is_machine_path(&format!(
        "/checkouts/{}/orchestration/hosted-audit",
        f.m.target.id
    )));
}

#[test]
fn hs2_crbr8x_an_actor_id_that_is_not_an_identifier_does_not_lose_the_entry() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    let reply =
        f.m.runtime
            .hosted_session_start(
                &f.m.target.id,
                HostedSessionStart {
                    session: late_start(&f, "op-1"),
                    limits: None,
                    actor: Some(MutationActor::parse("human", Some("Pat Example".into())).unwrap()),
                    resources: Vec::new(),
                },
                &mut tx,
            )
            .unwrap();
    assert_eq!(reply.outcome, HostedStartOutcome::Started);
    let entries = host_journal(&f);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor_role.as_deref(), Some("human"));
    assert_eq!(entries[0].actor_id, None);
}

/// HS2-KR28FS: a person hands a session they took over or paused back to the
/// orchestrator, only while the tool is idle between turns, and only with what a
/// session start needs. Walks takeover → hand back → takeover again, pause → hand
/// back, a mid-turn refusal, repeats, and a denied permission.
#[test]
fn hs2_kr28fs_a_person_hands_a_session_back_only_while_idle_and_permitted() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1], &mut tx);
    let owner = f.m.owner.id.clone();
    let hosted = HostedSessionRegistry::new(&f.m.home);
    let taken = f.started("op-1", 1, "codex", &mut tx);
    let sid = taken.session.session_id.clone();
    let host_id = taken.session.hosted.as_ref().unwrap().session_id.clone();
    let hand_back = |sid: &str, actor: &MutationActor, tx: &mut Transaction<'_>| {
        f.m.runtime
            .session_hand_back(&owner, sid, HandBackSession::default(), Some(actor), tx)
    };
    let takeover = |sid: &str, pause: bool, tx: &mut Transaction<'_>| {
        f.m.runtime
            .session_takeover(&owner, sid, TakeoverSession { pause }, Some(&human()), tx)
            .unwrap()
    };

    // The orchestrator's own session: a hand-back changes nothing and asks nothing.
    let unchanged = hand_back(&sid, &human(), &mut tx).unwrap();
    assert_eq!(unchanged.session, taken.session);
    // Only a person hands back.
    takeover(&sid, false, &mut tx);
    assert!(matches!(
        hand_back(&sid, &worker(), &mut tx),
        Err(OrchestrationError::Forbidden(_))
    ));

    // Takeover → hand back while idle: the orchestrator owns it again, at the host too.
    let back = hand_back(&sid, &human(), &mut tx).unwrap();
    assert_eq!(back.session.owner, SessionOwner::Orchestrator);
    assert!(!back.session.paused);
    let entry = back.session.history.last().unwrap();
    assert_eq!(entry.control, Some(SessionControl::HandBack));
    assert_eq!(entry.actor_id.as_deref(), Some("maintainer"));
    assert_eq!(
        hosted.get(&f.m.target.id, "op-1").unwrap().unwrap().owner,
        SessionOwner::Orchestrator
    );
    assert_eq!(
        f.m.last_audit(),
        ("session_hand_back".into(), AuditOutcome::Succeeded, None)
    );
    // A later sync keeps it the orchestrator's, and the orchestrator may wind it down
    // again (it is no longer refused as a person's).
    let synced =
        f.m.runtime
            .session_sync(&owner, &sid, Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(synced.session.owner, SessionOwner::Orchestrator);
    // Repeating the hand-back changes nothing.
    assert_eq!(
        hand_back(&sid, &human(), &mut tx).unwrap().session,
        synced.session
    );

    // Takeover again; mid-turn the hand-back is refused with its reason and the session
    // stays the person's, at both ends.
    assert_eq!(
        takeover(&sid, false, &mut tx).session.owner,
        SessionOwner::Human
    );
    f.host.set_busy(&host_id, true);
    assert_eq!(
        session_error(hand_back(&sid, &human(), &mut tx)),
        SessionError::MidTurn
    );
    assert_eq!(
        f.m.last_audit(),
        (
            "session_hand_back".into(),
            AuditOutcome::Failed,
            Some("session_mid_turn".into())
        )
    );
    assert_eq!(
        f.m.runtime.session(&owner, &sid).unwrap().owner,
        SessionOwner::Human
    );
    assert_eq!(
        hosted.get(&f.m.target.id, "op-1").unwrap().unwrap().owner,
        SessionOwner::Human
    );
    // The turn ends: the same request succeeds.
    f.host.set_busy(&host_id, false);
    assert_eq!(
        hand_back(&sid, &human(), &mut tx).unwrap().session.owner,
        SessionOwner::Orchestrator
    );

    // Pause → hand back: no longer paused, at both ends.
    assert!(takeover(&sid, true, &mut tx).session.paused);
    assert!(hosted.get(&f.m.target.id, "op-1").unwrap().unwrap().paused);
    let resumed = hand_back(&sid, &human(), &mut tx).unwrap();
    assert!(!resumed.session.paused);
    assert_eq!(resumed.session.owner, SessionOwner::Orchestrator);
    assert!(!hosted.get(&f.m.target.id, "op-1").unwrap().unwrap().paused);

    // Target 2 remembers no session start: a hand-back asks a person like a start. A
    // denial leaves the session the person's; a new request asks again, and allowing it
    // hands it back.
    let asked = permission_request(f.start("op-2", 2, "claude", json!({}), &mut tx));
    answer(
        &f.m.runtime,
        &owner,
        &asked,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    let second = f.started("op-2", 2, "claude", &mut tx).session.session_id;
    takeover(&second, false, &mut tx);
    let first_ask = permission_request(hand_back(&second, &human(), &mut tx));
    let pending =
        f.m.runtime
            .permission_list(&owner, &mut tx)
            .unwrap()
            .requests
            .into_iter()
            .find(|request| request.request.request_id == first_ask)
            .unwrap();
    assert_eq!(pending.request.key.action, PermissionAction::SessionStart);
    assert_eq!(pending.request.key.tool.as_deref(), Some("claude"));
    answer(
        &f.m.runtime,
        &owner,
        &first_ask,
        PermissionAnswer::Deny,
        &mut tx,
    );
    assert!(hand_back(&second, &human(), &mut tx).is_err());
    assert_eq!(
        f.m.runtime.session(&owner, &second).unwrap().owner,
        SessionOwner::Human
    );
    assert_eq!(
        hosted.get(&f.m.target.id, "op-2").unwrap().unwrap().owner,
        SessionOwner::Human
    );
    let again = |tx: &mut Transaction<'_>| {
        f.m.runtime.session_hand_back(
            &owner,
            &second,
            HandBackSession {
                operation_id: Some("hand-back-retry".into()),
            },
            Some(&human()),
            tx,
        )
    };
    let second_ask = permission_request(again(&mut tx));
    assert_ne!(second_ask, first_ask);
    answer(
        &f.m.runtime,
        &owner,
        &second_ask,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    assert_eq!(
        again(&mut tx).unwrap().session.owner,
        SessionOwner::Orchestrator
    );
}

/// HS2-KR28FS: the hosting half. A person at the host is passed to the orchestrating
/// checkout on this machine; the orchestrator's own request is decided by the tool's
/// turn; an unknown operation is refused with its reason.
#[test]
fn hs2_kr28fs_the_hosting_machine_hands_a_session_back_through_its_owner() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1], &mut tx);
    let owner = f.m.owner.id.clone();
    let started = f.started("op-1", 1, "codex", &mut tx);
    let sid = started.session.session_id.clone();
    let host_id = started.session.hosted.as_ref().unwrap().session_id.clone();
    let control =
        |operation: &str, hand_back: bool, actor: MutationActor, tx: &mut Transaction<'_>| {
            f.m.runtime.hosted_session_control(
                &f.m.target.id,
                HostedSessionControl {
                    operation_id: operation.into(),
                    pause: false,
                    hand_back,
                    hand_back_operation_id: None,
                    actor: Some(actor),
                },
                tx,
            )
        };
    control("op-1", false, human(), &mut tx).unwrap();
    // Unknown operation.
    let unknown = control("op-none", true, human(), &mut tx).unwrap();
    assert_eq!(unknown.refused.as_deref(), Some("session_not_found"));
    // The orchestrator's request mid-turn is refused with the reason and the view.
    f.host.set_busy(&host_id, true);
    let busy = control("op-1", true, worker(), &mut tx).unwrap();
    assert_eq!(busy.refused.as_deref(), Some("session_mid_turn"));
    assert_eq!(busy.session.unwrap().session.owner, SessionOwner::Human);
    f.host.set_busy(&host_id, false);
    // A person at the host: passed to the owner here, so the record follows.
    let back = control("op-1", true, human(), &mut tx).unwrap();
    assert!(back.refused.is_none());
    assert_eq!(
        back.session.unwrap().session.owner,
        SessionOwner::Orchestrator
    );
    assert_eq!(
        f.m.runtime.session(&owner, &sid).unwrap().owner,
        SessionOwner::Orchestrator
    );
    assert_eq!(f.m.last_audit().0, "session_hand_back");
    // An unidentified actor is refused.
    assert!(matches!(
        f.m.runtime.hosted_session_control(
            &f.m.target.id,
            HostedSessionControl {
                operation_id: "op-1".into(),
                pause: false,
                hand_back: true,
                hand_back_operation_id: None,
                actor: None,
            },
            &mut tx,
        ),
        Err(OrchestrationError::Invalid(_))
    ));
}

/// HS2-S0FDKR: a hosted session journal entry that cannot be written changes nothing
/// about the start or stop answer, is counted beside the journal with its error kind,
/// and the journal read reports the count, so the host's owner can tell the journal is
/// incomplete. Once the journal can be written again, entries land and the count of
/// what was lost stays.
#[test]
fn hs2_s0fdkr_a_journal_entry_that_cannot_be_written_is_counted_and_reported() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[1, 2], &mut tx);
    let start = |operation: &str, tx: &mut Transaction<'_>| {
        f.m.runtime
            .hosted_session_start(
                &f.m.target.id,
                HostedSessionStart {
                    session: late_start(&f, operation),
                    limits: None,
                    actor: Some(MutationActor::parse("ai", Some("scheduler-1".into())).unwrap()),
                    resources: Vec::new(),
                },
                tx,
            )
            .unwrap()
    };
    // Nothing lost: no count is reported.
    assert_eq!(
        f.m.runtime
            .hosted_audit(&f.m.target.id, 0, 100)
            .unwrap()
            .write_failures,
        None
    );
    // The journal's file is a directory: every append fails.
    let journal = f.m.home.join("hosted-session-audit");
    let current = journal.join(format!("{}.jsonl", f.m.target.id));
    std::fs::create_dir_all(&current).unwrap();
    for operation in ["op-1", "op-2"] {
        assert_eq!(
            start(operation, &mut tx).outcome,
            HostedStartOutcome::Started,
            "a lost entry does not change the answer"
        );
    }
    let failures =
        std::fs::read_to_string(journal.join(format!("{}.write-failures.json", f.m.target.id)))
            .unwrap();
    let failures: crate::orchestration::HostedJournalFailures =
        serde_json::from_str(&failures).unwrap();
    assert_eq!(failures.count, 2);
    assert!(failures.last_at_ms > 0);
    assert!(!failures.last_error.is_empty());
    // Identifiers and a code only: nothing about the request's content.
    assert!(
        failures
            .last_error
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '_'),
        "{failures:?}"
    );
    // Writable again: the next entry lands, and the count of what was lost stays.
    std::fs::remove_dir(&current).unwrap();
    // (Two sessions already run here, so this start is refused; a refusal is journaled
    // as a start is.)
    assert_ne!(start("op-3", &mut tx).outcome, HostedStartOutcome::Existing);
    let read = f.m.runtime.hosted_audit(&f.m.target.id, 0, 100).unwrap();
    assert_eq!(
        read.page
            .entries
            .iter()
            .map(|entry| entry.record.operation_id.as_str())
            .collect::<Vec<_>>(),
        ["op-3"]
    );
    assert_eq!(read.write_failures, Some(failures.clone()));
    let wire = serde_json::to_value(&read).unwrap();
    assert_eq!(wire["write_failures"]["count"], 2);
    assert!(
        wire["entries"].is_array(),
        "the page keeps its shape: {wire}"
    );
}

/// HS2-YWN8JT: a headless launch tells a server that is not there from one that took
/// the launch and has not answered yet, and a launch whose answer was lost is the
/// terminal the host now lists.
#[test]
fn hs2_ywn8jt_a_launch_the_host_took_is_never_read_as_no_host() {
    use ureq::ErrorKind;
    for kind in [
        ErrorKind::ConnectionFailed,
        ErrorKind::Dns,
        ErrorKind::InvalidUrl,
    ] {
        assert_eq!(classify_transport(kind), HostCall::Unreached, "{kind:?}");
    }
    for kind in [
        ErrorKind::Io,
        ErrorKind::BadStatus,
        ErrorKind::TooManyRedirects,
    ] {
        assert_eq!(classify_transport(kind), HostCall::Unanswered, "{kind:?}");
    }
    // No server listened: nothing launched, whatever a listing would say.
    for listed in [false, true] {
        assert_eq!(
            launch_outcome(HostCall::Unreached, listed),
            Err(SessionHostError::Unavailable)
        );
        assert_eq!(
            launch_outcome(HostCall::Status(409), listed),
            Err(SessionHostError::Refused)
        );
    }
    // Taken and unanswered: the listing decides.
    assert_eq!(launch_outcome(HostCall::Unanswered, true), Ok(()));
    assert_eq!(
        launch_outcome(HostCall::Unanswered, false),
        Err(SessionHostError::Unavailable)
    );
    // The launch waits as long as a person may need for a prompt the host shows.
    assert!(LAUNCH_ANSWER >= std::time::Duration::from_secs(5 * 60));
}
