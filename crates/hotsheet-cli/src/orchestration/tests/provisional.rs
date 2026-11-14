//! HS2-JSF985: provisional work across two targets through the shared runtime, with
//! real git ticket stores, a scripted CI adapter, a scripted capacity probe and the
//! recording session executor. Target 1 holds prerequisite ticket A; target 2, a
//! separate project with its own store and its own speculation setting, holds the
//! dependent B. Synthetic tickets and identities only; no AI tool is started.
use hotsheet_model::{NoteKind, Ticket};
use hotsheet_ticketing::orchestration::{
    AuditOutcome, BindTarget, Coverage, CpuCapacity, DeclarationState, DiskCapacity, GateKind,
    GateRegistry, GateState, HostObservation, MachineLimits, MemoryCapacity, PermissionAction,
    PermissionAnswer, PermissionState, PlacementPolicy, PolicyBody, ReadinessError, ReadinessLevel,
    ReplaceTargets, RunEventKind, SetPolicy, Speculation, TargetAction, TargetDeclaration,
    TargetDestination, TargetPolicy, ToolAvailability, ToolPolicy, ToolScope,
};
use hotsheet_ticketing::provisional::{GateResult, SpeculationPolicy};
use hotsheet_ticketing::{FsStore, NewTicket, Settings, StoreMetadata, TicketPatch, ops};
use serde_json::{Value, json};

use super::gates::{CI, Scripted, link, sha, verdict};
use super::tickets::{Machine, NOW, bind_and_allow, human, machine, unsupported, worker};
use super::*;
use crate::coordination::speculation::{Recovery, SpeculationState};

macro_rules! tx {
    ($tx:ident) => {
        let mut rng = Rng(100);
        let mut $tx = Transaction {
            clock: &Clock,
            rng: &mut rng,
        };
    };
}

/// A roomy machine with both tools, counting how often the runtime asked it.
#[derive(Default)]
struct Probe {
    asked: AtomicUsize,
}
impl CapacityProbe for Probe {
    fn observe(&self, _: &Checkout) -> HostObservation {
        self.asked.fetch_add(1, Ordering::SeqCst);
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
            tools: Some(vec![ToolAvailability {
                tool: "codex".into(),
                installed: true,
                configured: true,
            }]),
            sessions: Some(Vec::new()),
        }
    }
}

const ACTIONS: [TargetAction; 4] = [
    TargetAction::TicketsRead,
    TargetAction::GatesObserve,
    TargetAction::SessionsManage,
    TargetAction::WorkStart,
];
fn scope() -> ToolScope {
    ToolScope {
        tool: "codex".into(),
        models: Vec::new(),
    }
}

/// What a test varies about the orchestration policy.
#[derive(Clone, Default)]
struct Opt {
    /// `speculation.allowed`.
    allowed: bool,
    automatic: bool,
    max_depth: u32,
    /// HS2-XHQ1EH: remember no `provisional_dispatch` rule, so a person is asked.
    ask_ahead: bool,
    /// `targets[].speculation` for targets 1 and 2.
    overrides: [Option<bool>; 2],
    /// `targets[].max_sessions` for target 2.
    cap: u32,
}
impl Opt {
    fn on() -> Self {
        Self {
            allowed: true,
            ..Self::default()
        }
    }
    fn body(&self) -> PolicyBody {
        PolicyBody {
            machine: MachineLimits {
                max_sessions: 4,
                ..MachineLimits::default()
            },
            tools: vec![ToolPolicy {
                tool: "codex".into(),
                max_sessions: 4,
                weight: 60,
                accounts: Vec::new(),
            }],
            targets: [1, 2]
                .map(|n| TargetPolicy {
                    target_id: id(n),
                    enabled: true,
                    actions: None,
                    tools: vec![scope()],
                    max_sessions: if n == 2 { self.cap.max(1) } else { 2 },
                    machine: None,
                    speculation: self.overrides[usize::try_from(n - 1).unwrap()],
                })
                .into(),
            speculation: Speculation {
                allowed: self.allowed,
                max_depth: self.max_depth.max(1),
                ..Speculation::default()
            },
            placement: PlacementPolicy {
                automatic: self.automatic,
                actor: Some("worker-1".into()),
                ..PlacementPolicy::default()
            },
            ..PolicyBody::default()
        }
    }
}

struct Two {
    m: Machine,
    probe: Arc<Probe>,
    sessions: Arc<RecordingSessions>,
    ci: Arc<Scripted>,
    /// Target 2's project and store.
    second: Checkout,
    second_store: FsStore,
    orchestration: Ticket,
    /// Ticket A in target 1, the run working it and the gate of its commit `a`.
    a: Ticket,
    run_a: String,
    gate_a: String,
    /// Ticket B in target 2 and the step that requires A with CI.
    b: Ticket,
    step_b: String,
}

fn ticket_in(store: &FsStore, prefix: &str, title: &str) -> Ticket {
    ops::create(
        store,
        ulid::Ulid::new(),
        prefix,
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

/// Whether a target project's own committed setting opts in, and how deep.
fn own_policy(root: &str, enabled: bool, max_depth: u32) {
    SpeculationPolicy {
        enabled,
        max_depth,
        ..SpeculationPolicy::default()
    }
    .write(&Settings::for_project(root))
    .unwrap();
}

impl Two {
    fn owner(&self) -> String {
        self.m.owner.id.clone()
    }
    fn second_connection(&self) -> String {
        self.second.sources[0].connection_id.clone()
    }
    fn set_policy(&self, opt: &Opt, tx: &mut Transaction<'_>) {
        let expected_revision = self.m.runtime.policy(&self.owner()).unwrap().revision;
        self.m
            .runtime
            .set_policy(
                &self.owner(),
                SetPolicy {
                    expected_revision,
                    policy: opt.body(),
                },
                Some(&human()),
                tx,
            )
            .unwrap();
    }
    /// Plan a step for a ticket of target `n` with ticket prerequisites.
    fn define(
        &self,
        n: u128,
        native: &str,
        requires: &[(u128, &str)],
        tx: &mut Transaction<'_>,
    ) -> String {
        let connection = |n: u128| {
            if n == 1 {
                self.m.target_connection()
            } else {
                self.second_connection()
            }
        };
        let prerequisites: Vec<Value> = requires
            .iter()
            .map(|(target, native)| {
                json!({"kind": "ticket", "target_id": id(*target),
                    "connection_id": connection(*target), "native_id": native, "gates": "ci"})
            })
            .collect();
        self.m
            .runtime
            .readiness_define(
                &self.owner(),
                serde_json::from_value(json!({
                    "ticket": {"connection_id": self.m.owner_connection(),
                        "native_id": self.orchestration.id.to_string()},
                    "target_id": id(n), "connection_id": connection(n), "native_id": native,
                    "prerequisites": prerequisites,
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
    fn dispatch(
        &self,
        n: u128,
        ticket: &Ticket,
        operation: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        let connection = if n == 1 {
            self.m.target_connection()
        } else {
            self.second_connection()
        };
        self.m.runtime.run_dispatch(
            &self.owner(),
            serde_json::from_value(json!({
                "ticket": {"connection_id": self.m.owner_connection(),
                    "native_id": self.orchestration.id.to_string()},
                "target_id": id(n), "connection_id": connection,
                "native_id": ticket.id.to_string(), "operation_id": operation,
                "worker_id": format!("worker-{operation}"),
            }))
            .unwrap(),
            Some(&worker()),
            tx,
        )
    }
    fn dispatch_b(&self, tx: &mut Transaction<'_>) -> Result<RunView, OrchestrationError> {
        self.dispatch(2, &self.b, "op-b", tx)
    }
    fn link(&self, run_id: &str, commit: &str, tx: &mut Transaction<'_>) -> String {
        self.m
            .runtime
            .gate_link(&self.owner(), run_id, link(commit, CI), Some(&worker()), tx)
            .unwrap()
            .gate
            .gate_id
    }
    /// The CI adapter reports a verdict for a commit and the run's gates are synced.
    fn ci_says(
        &self,
        run_id: &str,
        commit: &str,
        state: GateState,
        order: u64,
        tx: &mut Transaction<'_>,
    ) {
        self.ci
            .set(commit, verdict(CI, state, order, commit, Coverage::Exact));
        self.sync(run_id, tx);
    }
    fn sync(&self, run_id: &str, tx: &mut Transaction<'_>) {
        self.m
            .runtime
            .gate_sync(
                &self.owner(),
                run_id,
                &SyncGates::default(),
                Some(&worker()),
                tx,
            )
            .unwrap();
    }
    fn show(&self, step_id: &str, tx: &mut Transaction<'_>) -> StepView {
        self.m
            .runtime
            .readiness(&self.owner(), step_id, tx)
            .unwrap()
    }
    fn status(&self) -> ProvisionalView {
        self.m.runtime.provisional_status(&self.owner()).unwrap()
    }
    /// The one recorded piece of provisional work.
    fn only(&self) -> ProvisionalWork {
        let mut work = self.status().work;
        assert_eq!(work.len(), 1, "{work:#?}");
        work.remove(0)
    }
    /// The `provisional` events of one step: the state entered and the gate behind it.
    fn events(&self, step_id: &str) -> Vec<(String, Option<String>)> {
        self.m
            .runtime
            .run_events(&self.owner(), 0, 500, Some(step_id), 0)
            .unwrap()
            .entries
            .into_iter()
            .filter(|entry| entry.record.kind == RunEventKind::Provisional)
            .map(|entry| (entry.record.reason.unwrap(), entry.record.gate_id))
            .collect()
    }
    fn states(&self, step_id: &str) -> Vec<String> {
        self.events(step_id)
            .into_iter()
            .map(|(state, _)| state)
            .collect()
    }
    /// The orchestration ticket's activity notes that start with `prefix`.
    fn notes(&self, prefix: &str) -> Vec<String> {
        self.m
            .owner_store
            .read_ticket(&self.orchestration.id)
            .unwrap()
            .notes
            .into_iter()
            .filter(|note| note.kind == NoteKind::Activity && note.text.starts_with(prefix))
            .map(|note| note.text)
            .collect()
    }
    fn audited(&self, action: &str) -> Vec<(AuditOutcome, Option<String>)> {
        self.m
            .runtime
            .audit(&self.owner(), 0, 500)
            .unwrap()
            .entries
            .into_iter()
            .filter(|entry| entry.record.action == action)
            .map(|entry| (entry.record.outcome, entry.record.reason))
            .collect()
    }
    fn tick(&self, tx: &mut Transaction<'_>) -> TickView {
        self.m
            .runtime
            .scheduler_tick(&self.owner(), &TickRequest::default(), Some(&worker()), tx)
            .unwrap()
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
    fn file(&self) -> std::path::PathBuf {
        self.m.home.join("orchestration-provisional.json")
    }
}

/// Two bound targets on one machine. A is dispatched and integrated at commit `a`,
/// whose CI gate pends; B requires A with CI, so its step is `provisional`. Both
/// target projects opt in with depth 1 unless a test changes that; the orchestration
/// policy is whatever `opt` says, or absent.
fn two(opt: Option<&Opt>, tx: &mut Transaction<'_>) -> Two {
    let mut m = machine(10, false, unsupported);
    let probe = Arc::new(Probe::default());
    let sessions = Arc::new(RecordingSessions::default());
    let ci = Scripted::new(CI, GateKind::Ci);
    m.runtime = m
        .runtime
        .clone()
        .with_capacity_probe(probe.clone())
        .with_session_executor(sessions.clone())
        .with_gate_provider(ci.clone());
    bind_and_allow(&m, 1, &ACTIONS[..2], tx);
    // A second project with its own ticket store, bound as target 2.
    let project = m._root.path().join("second");
    std::fs::create_dir(&project).unwrap();
    let store_path = m._root.path().join("second-store");
    let second_store = FsStore::init_git(&store_path, &StoreMetadata::new("SEC")).unwrap();
    let second = m
        .runtime
        .checkouts
        .register(&project, None, None, vec![store_path])
        .unwrap();
    let before = m.runtime.declarations(&m.owner.id).unwrap();
    let mut targets = before.targets;
    targets.push(TargetDeclaration {
        target_id: id(2),
        state: DeclarationState::Active,
        label: "Second".into(),
        expected_repository_identity: None,
    });
    let manifest = m
        .runtime
        .replace(
            &m.owner.id,
            ReplaceTargets {
                expected_revision: before.revision,
                targets,
            },
            tx,
        )
        .unwrap();
    let projection = m.runtime.checkout_projection(&second.id).unwrap();
    assert!(
        m.runtime
            .bind(
                BindTarget {
                    target: m.reference(2),
                    expected_manifest_revision: manifest.revision.unwrap(),
                    expected_binding_revision: 0,
                    previous_destination: None,
                    destination: TargetDestination::Local {
                        server_id: projection.server_id,
                        checkout_id: projection.checkout_id,
                        checkout_authority: projection.checkout_authority,
                    },
                },
                tx
            )
            .unwrap()
            .usable
    );
    for n in [1, 2] {
        allow_on(
            &m.runtime,
            &m.owner.id,
            &id(n),
            &[TargetAction::WorkStart, TargetAction::SessionsManage],
            &["codex"],
            tx,
        );
        // HS2-XHQ1EH: building ahead is its own decision, remembered unless the test
        // wants a person asked.
        if !opt.is_some_and(|opt| opt.ask_ahead) {
            m.runtime
                .permission_remember(
                    &m.owner.id,
                    RememberPermission {
                        target_id: id(n),
                        action: PermissionAction::ProvisionalDispatch,
                        tool: None,
                    },
                    Some(&human()),
                    tx,
                )
                .unwrap();
        }
    }
    own_policy(&m.target.root, true, 1);
    own_policy(&second.root, true, 1);
    let orchestration = m.orchestration_ticket("Coordinate");
    let a = ticket_in(&m.target_store, "TGT", "Prerequisite work");
    let b = ticket_in(&second_store, "SEC", "Dependent work");
    let mut two = Two {
        m,
        probe,
        sessions,
        ci,
        second,
        second_store,
        orchestration,
        a,
        run_a: String::new(),
        gate_a: String::new(),
        b,
        step_b: String::new(),
    };
    if let Some(opt) = opt {
        two.set_policy(opt, tx);
    }
    let a = two.a.clone();
    two.run_a = two.dispatch(1, &a, "op-a", tx).unwrap().run.run_id;
    two.gate_a = two.link(&two.run_a.clone(), &sha('a'), tx);
    two.step_b = two.define(2, &two.b.slug.clone(), &[(1, &two.a.slug.clone())], tx);
    assert_eq!(
        two.show(&two.step_b.clone(), tx).readiness.level,
        ReadinessLevel::Provisional
    );
    two
}

fn refusal<T>(result: Result<T, OrchestrationError>) -> ReadinessError {
    match result {
        Err(OrchestrationError::Readiness(error)) => error,
        Err(other) => panic!("expected a readiness refusal, got {other}"),
        Ok(_) => panic!("expected a readiness refusal"),
    }
}

#[test]
fn hs2_jsf985_without_the_opt_in_dispatch_scheduling_and_output_are_unchanged() {
    tx!(tx);
    // No policy at all, then a policy that places automatically and does not opt in.
    // Both target projects opt in themselves: that alone changes nothing.
    for policy in [
        None,
        Some(Opt {
            automatic: true,
            ..Opt::default()
        }),
    ] {
        let t = two(policy.as_ref(), &mut tx);
        let owner = t.owner();
        let asked = t.probe.asked.load(Ordering::SeqCst);
        assert_eq!(
            refusal(t.dispatch_b(&mut tx)),
            ReadinessError::Provisional,
            "the refusal is the one before this ticket"
        );
        assert_eq!(
            OrchestrationError::Readiness(ReadinessError::Provisional).code(),
            "readiness_provisional"
        );
        // Every output is what it was: no new member anywhere.
        let show = serde_json::to_value(t.show(&t.step_b, &mut tx)).unwrap();
        assert_eq!(show["level"], "provisional");
        assert_eq!(show["base_commits"], json!([sha('a')]));
        for key in ["speculation", "provisional"] {
            assert!(show.get(key).is_none(), "{key}");
        }
        let ready = serde_json::to_value(t.m.runtime.ready_work(&owner, &mut tx).unwrap()).unwrap();
        assert_eq!(ready[0]["level"], "provisional");
        assert!(ready[0].get("speculation").is_none());
        let status =
            serde_json::to_value(t.m.runtime.run_status(&owner, &t.run_a).unwrap()).unwrap();
        assert!(status.get("provisional").is_none());
        let explained = serde_json::to_value(t.explain(&t.step_b, &mut tx)).unwrap();
        for key in ["speculation", "provisional", "provisional_dependents"] {
            assert!(explained.get(key).is_none(), "{key}");
        }
        if policy.is_some() {
            assert_eq!(
                (&explained["standing"], &explained["reason"]),
                (&json!("not_placed"), &json!("provisional_not_dispatched"))
            );
            assert!(explained.get("detail").is_none());
            // An applied pass places nothing, starts nothing and contacts no target
            // for provisional work.
            let tick = t.tick(&mut tx);
            let unplaced = serde_json::to_value(&tick.view.schedule.unplaced).unwrap();
            assert_eq!(
                unplaced,
                json!([{"step_id": t.step_b, "target_id": id(2),
                    "reason": "provisional_not_dispatched",
                    "score": unplaced[0]["score"].clone()}])
            );
            assert!(tick.view.schedule.placements.is_empty() && tick.outcomes.is_empty());
            assert_eq!(tick.view.inputs.targets, 0);
        }
        assert!(t.sessions.calls().is_empty());
        assert_eq!(t.probe.asked.load(Ordering::SeqCst), asked);
        // A gate result changes readiness as before and nothing else.
        t.ci_says(&t.run_a, &sha('a'), GateState::Failed, 1, &mut tx);
        t.ci_says(&t.run_a, &sha('a'), GateState::Passed, 2, &mut tx);
        t.m.runtime
            .runs_reconcile(&owner, Some(&worker()), &mut tx)
            .unwrap();
        assert!(t.events(&t.step_b).is_empty());
        assert!(t.audited("provisional_dispatch").is_empty());
        assert!(t.notes("Provisional").is_empty());
        assert_eq!(
            t.status(),
            ProvisionalView {
                enabled: false,
                ..ProvisionalView::default()
            }
        );
        assert!(!t.file().exists(), "nothing is recorded without the opt-in");
        // The plan file holds nothing new either: no tag was read.
        let plan = std::fs::read_to_string(t.m.home.join("orchestration-plan.json")).unwrap();
        assert!(!plan.contains("opted_out"));
    }
    // A project that does not opt in itself answers a capacity read as it always did;
    // one that does says so.
    let t = two(None, &mut tx);
    own_policy(&t.second.root, false, 1);
    let capacity = |checkout: &Checkout| {
        serde_json::to_value(t.m.runtime.hosted_capacity(&checkout.id, &[]).unwrap()).unwrap()
    };
    assert!(capacity(&t.second).get("speculation").is_none());
    assert_eq!(
        capacity(&t.m.target)["speculation"],
        json!({"enabled": true, "max_depth": 1})
    );
}

#[test]
fn hs2_jsf985_work_built_on_a_pending_gate_is_confirmed_when_it_turns_green() {
    tx!(tx);
    let t = two(Some(&Opt::on()), &mut tx);
    let owner = t.owner();
    // A read contacts nothing, so until a target was asked its own setting is unknown.
    let before = t.show(&t.step_b, &mut tx).speculation.unwrap();
    assert_eq!(
        (before.allowed, before.refusal.as_deref()),
        (false, Some("target_speculation_unknown"))
    );
    assert!(!t.file().exists());

    let run_b = t.dispatch_b(&mut tx).unwrap().run.run_id;
    assert_eq!(
        t.audited("provisional_dispatch"),
        [(AuditOutcome::Allowed, Some("provisional".into()))]
    );
    let work = t.only();
    assert_eq!(
        (
            work.step_id.as_str(),
            work.operation_id.as_str(),
            work.state,
            work.depth
        ),
        (t.step_b.as_str(), "op-b", SpeculationState::Provisional, 1)
    );
    assert_eq!(work.target.target_id, id(2));
    assert_eq!(work.gates.len(), 1);
    let gate = &work.gates[0];
    assert_eq!(
        (
            gate.target_id.clone(),
            gate.native_id.clone(),
            gate.commit.clone()
        ),
        (id(1), t.a.id.to_string(), sha('a'))
    );
    assert_eq!(
        (gate.gate_ids.clone(), gate.result, gate.run.clone()),
        (
            vec![t.gate_a.clone()],
            GateResult::Pending,
            Some(t.gate_a.clone())
        )
    );
    // The work names its gate wherever it is shown.
    assert_eq!(
        work.explain,
        format!("provisional on {} CI {}", t.a.id, t.gate_a)
    );
    assert_eq!(
        t.events(&t.step_b),
        [("provisional".into(), Some(t.gate_a.clone()))]
    );
    assert_eq!(
        t.m.runtime.run_status(&owner, &run_b).unwrap().provisional,
        Some(work.clone())
    );
    assert_eq!(
        t.m.runtime
            .run_status(&owner, &t.run_a)
            .unwrap()
            .provisional,
        None
    );
    let shown = t.show(&t.step_b, &mut tx);
    assert_eq!(shown.readiness.level, ReadinessLevel::InProgress);
    assert_eq!(shown.provisional, Some(work.clone()));
    let explained = t.explain(&t.step_b, &mut tx);
    assert_eq!(
        (explained.standing.as_str(), explained.provisional),
        ("running", Some(work.clone()))
    );
    let notes = t.notes("Provisional dispatch");
    assert_eq!(notes.len(), 1);
    for part in [&t.step_b, &t.gate_a, &sha('a'), &"op-b".to_owned()] {
        assert!(notes[0].contains(part.as_str()), "{part}");
    }
    // Identifiers only: no title reaches the note, the events or the ledger file.
    let stored = std::fs::read_to_string(t.file()).unwrap();
    for text in [&notes[0], &stored] {
        assert!(!text.contains("Dependent work") && !text.contains("Prerequisite work"));
    }

    // The same dispatch again is the same run and records nothing more.
    assert_eq!(t.dispatch_b(&mut tx).unwrap().run.run_id, run_b);
    assert_eq!(t.status().work.len(), 1);
    assert_eq!(t.states(&t.step_b), ["provisional"]);
    assert_eq!(t.notes("Provisional dispatch").len(), 1);

    // Still pending: reading the adapter again changes nothing.
    t.sync(&t.run_a, &mut tx);
    assert_eq!(t.states(&t.step_b), ["provisional"]);
    // Green: confirmed once, however often it is read afterwards.
    t.ci_says(&t.run_a, &sha('a'), GateState::Passed, 1, &mut tx);
    t.sync(&t.run_a, &mut tx);
    assert_eq!(
        t.events(&t.step_b),
        [
            ("provisional".into(), Some(t.gate_a.clone())),
            ("confirmed".into(), Some(t.gate_a.clone()))
        ]
    );
    let confirmed = t.only();
    assert_eq!(
        (
            confirmed.state,
            confirmed.gates[0].result,
            confirmed.gates[0].run.as_deref()
        ),
        (
            SpeculationState::Confirmed,
            GateResult::Green,
            Some("run-1.1")
        )
    );
    assert_eq!(confirmed.explain, "confirmed");
    let view = t.status();
    assert!(view.enabled && view.repair_first.is_empty() && view.reevaluate.is_empty());
    assert_eq!(
        t.m.runtime.provisional_reconcile(&owner, &mut tx).unwrap(),
        0
    );
    assert!(t.notes("Provisional work at risk").is_empty());
}

#[test]
fn hs2_jsf985_a_red_gate_puts_the_work_at_risk_and_its_fix_rebases_it() {
    tx!(tx);
    let t = two(Some(&Opt::on()), &mut tx);
    let owner = t.owner();
    let step_a = t.define(1, &t.a.slug, &[], &mut tx);
    t.dispatch_b(&mut tx).unwrap();

    t.ci_says(&t.run_a, &sha('a'), GateState::Failed, 1, &mut tx);
    assert_eq!(t.states(&t.step_b), ["provisional", "at_risk"]);
    let risky = t.only();
    assert_eq!(
        (risky.state, risky.recovery, risky.gates[0].result),
        (
            SpeculationState::AtRisk,
            Some(Recovery::RebaseOntoFix),
            GateResult::Red
        )
    );
    assert_eq!(risky.gates[0].run.as_deref(), Some("run-1.1"));
    assert_eq!(risky.explain, format!("at risk: {} CI run-1.1 red", t.a.id));
    // One note on the orchestration ticket names the run, by identifiers only.
    let notes = t.notes("Provisional work at risk");
    assert_eq!(notes.len(), 1);
    for part in [
        t.step_b.as_str(),
        "run-1.1",
        t.gate_a.as_str(),
        &sha('a'),
        &t.a.id.to_string(),
        "rebase onto the fix",
    ] {
        assert!(notes[0].contains(part), "{part}: {}", notes[0]);
    }
    // The repair comes first, and the work stacked on the gate is named with it.
    let view = t.status();
    assert_eq!(view.repair_first.len(), 1);
    assert_eq!(
        (
            view.repair_first[0].native_id.clone(),
            view.repair_first[0].commit.clone(),
            view.repair_first[0].stacked.clone()
        ),
        (t.a.id.to_string(), sha('a'), vec![t.step_b.clone()])
    );
    assert_eq!(
        t.explain(&step_a, &mut tx).provisional_dependents,
        std::slice::from_ref(&t.step_b)
    );
    // Reading the same red result again marks, notes and publishes nothing more.
    t.sync(&t.run_a, &mut tx);
    t.m.runtime.provisional_reconcile(&owner, &mut tx).unwrap();
    assert_eq!(t.states(&t.step_b), ["provisional", "at_risk"]);
    assert_eq!(t.notes("Provisional work at risk").len(), 1);

    // The failed run ends, so A's repair is ready work again. Its urgency carries the
    // work stacked on the red gate: 2000 for the gate and 250 for B.
    t.m.runtime
        .run_cancel(&owner, &t.run_a, Some(&worker()), &mut tx)
        .unwrap();
    let plan =
        t.m.runtime
            .scheduler_plan(&owner, Some(&worker()), &mut tx)
            .unwrap();
    let repair = plan
        .schedule
        .ranking
        .iter()
        .find(|ranked| ranked.step_id == step_a)
        .expect("the repair is ready");
    assert_eq!(repair.score.red_gate, 2250);
    assert_eq!(plan.schedule.ranking[0].step_id, step_a);

    // The fix is dispatched as a new run. Until it is integrated nothing changes.
    let a = t.a.clone();
    let fix_run = t.dispatch(1, &a, "op-a-fix", &mut tx).unwrap().run.run_id;
    t.m.runtime.provisional_reconcile(&owner, &mut tx).unwrap();
    assert_eq!(t.only().state, SpeculationState::AtRisk);
    // Its commit `f` is linked: the work is rebased onto the fix's gate, which is
    // first unwatched and then pending. Each is its own event about that gate.
    let gate_fix = t.link(&fix_run, &sha('f'), &mut tx);
    assert_eq!(
        t.events(&t.step_b)[2..],
        [
            ("rebased".to_owned(), Some(gate_fix.clone())),
            ("provisional".to_owned(), Some(gate_fix.clone()))
        ]
    );
    let rebased = t.only();
    assert_eq!(
        (rebased.state, rebased.recovery, rebased.gates[0].result),
        (SpeculationState::Provisional, None, GateResult::Pending)
    );
    assert_eq!(
        (
            rebased.gates[0].commit.clone(),
            rebased.gates[0].gate_ids.clone()
        ),
        (sha('f'), vec![gate_fix.clone()])
    );
    let view = t.status();
    assert!(view.repair_first.is_empty());
    assert!(
        t.explain(&step_a, &mut tx)
            .provisional_dependents
            .is_empty()
    );
    // A late, repeated red reading of the replaced commit changes nothing.
    t.sync(&t.run_a, &mut tx);
    assert_eq!(t.only().state, SpeculationState::Provisional);

    // The fix fails once, then passes on a later attempt: at risk again with its own
    // note, then confirmed.
    t.ci_says(&fix_run, &sha('f'), GateState::Failed, 3, &mut tx);
    assert_eq!(t.only().state, SpeculationState::AtRisk);
    assert_eq!(t.notes("Provisional work at risk").len(), 2);
    t.ci_says(&fix_run, &sha('f'), GateState::Passed, 4, &mut tx);
    assert_eq!(
        t.states(&t.step_b),
        [
            "provisional",
            "at_risk",
            "rebased",
            "provisional",
            "at_risk",
            "confirmed"
        ]
    );
    assert_eq!(t.only().state, SpeculationState::Confirmed);
    assert_eq!(t.notes("Provisional work at risk").len(), 2);
    // Nothing was discarded: one record, with its whole history.
    assert_eq!(t.status().work.len(), 1);
}

#[test]
fn hs2_jsf985_a_fix_linked_on_the_same_run_and_work_already_integrated() {
    tx!(tx);
    let t = two(Some(&Opt::on()), &mut tx);
    let run_b = t.dispatch_b(&mut tx).unwrap().run.run_id;
    // B's own commit reached its target's shipping branch before A's gate failed.
    t.link(&run_b, &sha('b'), &mut tx);
    t.ci_says(&t.run_a, &sha('a'), GateState::Failed, 1, &mut tx);
    let risky = t.only();
    assert_eq!(
        (risky.state, risky.recovery),
        (
            SpeculationState::AtRisk,
            Some(Recovery::ReverifyByCoveringRun)
        )
    );
    assert!(t.notes("Provisional work at risk")[0].contains("covering CI run"));
    // The fix is linked in the same slot of the same run: the red gate is superseded.
    let gate_fix = t.link(&t.run_a, &sha('f'), &mut tx);
    assert_eq!(
        t.events(&t.step_b)[2..],
        [
            ("rebased".to_owned(), Some(gate_fix.clone())),
            ("provisional".to_owned(), Some(gate_fix))
        ]
    );
    assert_eq!(t.only().state, SpeculationState::Provisional);
    assert_eq!(t.only().gates[0].commit, sha('f'));
    // A fix that is green by the time it is seen confirms at once.
    let u = two(Some(&Opt::on()), &mut tx);
    u.dispatch_b(&mut tx).unwrap();
    u.ci_says(&u.run_a, &sha('a'), GateState::Failed, 1, &mut tx);
    u.ci.set(
        &sha('f'),
        verdict(CI, GateState::Passed, 2, &sha('f'), Coverage::Exact),
    );
    u.link(&u.run_a, &sha('f'), &mut tx);
    u.sync(&u.run_a, &mut tx);
    assert_eq!(u.only().state, SpeculationState::Confirmed);
    assert_eq!(u.states(&u.step_b).last().unwrap(), "confirmed");
}

#[test]
fn hs2_jsf985_a_target_that_disallows_speculation_is_never_built_on() {
    tx!(tx);
    type Arrange = fn(&Two, &mut Transaction<'_>);
    let cases: [(&str, Opt, Arrange, ReadinessError, Option<&str>); 6] = [
        (
            "the prerequisite's project does not opt in",
            Opt::on(),
            |t, _| own_policy(&t.m.target.root, false, 1),
            ReadinessError::ProvisionalRefused,
            Some("target_speculation_disabled"),
        ),
        (
            "the working project does not opt in",
            Opt::on(),
            |t, _| own_policy(&t.second.root, false, 1),
            ReadinessError::ProvisionalRefused,
            Some("target_speculation_disabled"),
        ),
        (
            "the working project's setting cannot be read, so it is not reported",
            Opt::on(),
            |t, _| {
                std::fs::write(
                    std::path::Path::new(&t.second.root).join(".hotsheet2/settings.json"),
                    b"{not json",
                )
                .unwrap();
            },
            ReadinessError::ProvisionalRefused,
            Some("target_speculation_disabled"),
        ),
        (
            "the orchestration policy switches the prerequisite's target off",
            Opt {
                overrides: [Some(false), None],
                ..Opt::on()
            },
            |_, _| {},
            ReadinessError::ProvisionalRefused,
            Some("prerequisite_speculation_not_allowed"),
        ),
        (
            "the orchestration policy switches the working target off",
            Opt {
                overrides: [None, Some(false)],
                ..Opt::on()
            },
            |_, _| {},
            // Not opted in for this target: the refusal it always was.
            ReadinessError::Provisional,
            None,
        ),
        (
            "the dependent ticket carries the opt-out tag",
            Opt::on(),
            |t, _| {
                ops::update(
                    &t.second_store,
                    &t.b.id,
                    NOW.into(),
                    TicketPatch {
                        tags: Some(vec!["speculate:never".into()]),
                        ..Default::default()
                    },
                )
                .unwrap();
            },
            ReadinessError::ProvisionalRefused,
            Some("speculation_opted_out"),
        ),
    ];
    for (name, opt, arrange, expected, code) in cases {
        let mut opt = opt;
        opt.automatic = true;
        let t = two(Some(&opt), &mut tx);
        arrange(&t, &mut tx);
        assert_eq!(refusal(t.dispatch_b(&mut tx)), expected, "{name}");
        let standing = t.show(&t.step_b, &mut tx).speculation;
        let tick = t.tick(&mut tx);
        let unplaced = &tick.view.schedule.unplaced;
        assert_eq!(unplaced.len(), 1, "{name}");
        match code {
            Some(code) => {
                let standing = standing.unwrap_or_else(|| panic!("{name}"));
                assert_eq!(
                    (standing.allowed, standing.refusal.as_deref()),
                    (false, Some(code)),
                    "{name}"
                );
                assert_eq!(
                    t.audited("provisional_dispatch"),
                    [(AuditOutcome::Denied, Some(code.into()))],
                    "{name}"
                );
                assert_eq!(
                    (unplaced[0].reason.code(), unplaced[0].detail.as_deref()),
                    ("speculation_refused", Some(code)),
                    "{name}"
                );
                assert_eq!(
                    t.explain(&t.step_b, &mut tx)
                        .speculation
                        .and_then(|standing| standing.refusal)
                        .as_deref(),
                    Some(code),
                    "{name}"
                );
            }
            None => {
                assert_eq!(
                    (unplaced[0].reason.code(), unplaced[0].detail.as_deref()),
                    ("provisional_not_dispatched", None),
                    "{name}"
                );
                assert!(t.audited("provisional_dispatch").is_empty(), "{name}");
            }
        }
        // Nothing was dispatched, started or recorded.
        assert!(t.sessions.calls().is_empty(), "{name}");
        assert!(tick.outcomes.is_empty(), "{name}");
        assert!(t.status().work.is_empty(), "{name}");
        assert!(t.events(&t.step_b).is_empty(), "{name}");
        assert!(
            t.m.runtime
                .run_list(&t.owner(), false)
                .unwrap()
                .iter()
                .all(|run| run.run_id == t.run_a),
            "{name}"
        );
    }

    // The project opts in later: its setting is read fresh and the work goes ahead.
    let t = two(Some(&Opt::on()), &mut tx);
    own_policy(&t.m.target.root, false, 1);
    assert_eq!(
        refusal(t.dispatch_b(&mut tx)),
        ReadinessError::ProvisionalRefused
    );
    own_policy(&t.m.target.root, true, 1);
    t.dispatch_b(&mut tx).unwrap();
    assert_eq!(t.only().state, SpeculationState::Provisional);
    // And off again afterwards: recorded work is not unrecorded, new work is refused.
    own_policy(&t.m.target.root, false, 1);
    let c = ticket_in(&t.second_store, "SEC", "Another dependent");
    t.define(2, &c.slug, &[(1, &t.a.slug)], &mut tx);
    assert_eq!(
        refusal(t.dispatch(2, &c, "op-c", &mut tx)),
        ReadinessError::ProvisionalRefused
    );
    assert_eq!(t.status().work.len(), 1);
}

#[test]
fn hs2_jsf985_a_plan_defined_before_the_opt_in_reads_its_tags_when_first_needed() {
    tx!(tx);
    let plan_file =
        |t: &Two| std::fs::read_to_string(t.m.home.join("orchestration-plan.json")).unwrap();
    // Defined with no policy: no tag is read, and the plan file is what it always was.
    let t = two(None, &mut tx);
    assert!(!plan_file(&t).contains("opted_out"));
    t.set_policy(&Opt::on(), &mut tx);
    // A dispatch reads the named tickets again first, then decides.
    t.dispatch_b(&mut tx).unwrap();
    assert_eq!(t.only().state, SpeculationState::Provisional);
    assert!(plan_file(&t).contains("\"opted_out\":false"));

    // The same through the scheduler: one pass reads them and places the work.
    let s = two(None, &mut tx);
    s.set_policy(
        &Opt {
            automatic: true,
            ..Opt::on()
        },
        &mut tx,
    );
    let tick = s.tick(&mut tx);
    assert_eq!(tick.outcomes.len(), 1, "{:?}", tick.view.schedule.unplaced);
    assert_eq!(tick.outcomes[0].outcome, "started");
    assert_eq!(s.only().state, SpeculationState::Provisional);

    // A tag set before the opt-in is found by that first read.
    let tagged = two(None, &mut tx);
    ops::update(
        &tagged.m.target_store,
        &tagged.a.id,
        NOW.into(),
        TicketPatch {
            tags: Some(vec!["Speculate:Never".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    tagged.set_policy(&Opt::on(), &mut tx);
    assert_eq!(
        refusal(tagged.dispatch_b(&mut tx)),
        ReadinessError::ProvisionalRefused
    );
    assert_eq!(
        tagged
            .show(&tagged.step_b, &mut tx)
            .speculation
            .unwrap()
            .refusal
            .as_deref(),
        Some("speculation_opted_out")
    );
    assert!(tagged.status().work.is_empty());
}

#[test]
fn hs2_jsf985_the_depth_limit_stops_a_second_layer_of_unconfirmed_gates() {
    tx!(tx);
    let t = two(Some(&Opt::on()), &mut tx);
    let run_b = t.dispatch_b(&mut tx).unwrap().run.run_id;
    // B is integrated at commit `b` while A's gate still pends; C requires B.
    let gate_b = t.link(&run_b, &sha('b'), &mut tx);
    let c = ticket_in(&t.second_store, "SEC", "Second layer");
    let step_c = t.define(2, &c.slug, &[(2, &t.b.slug)], &mut tx);
    assert_eq!(
        t.show(&step_c, &mut tx).readiness.level,
        ReadinessLevel::Provisional
    );
    let refused = |t: &Two, tx: &mut Transaction<'_>| {
        assert_eq!(
            refusal(t.dispatch(2, &c, "op-c", tx)),
            ReadinessError::ProvisionalRefused
        );
        let standing = t.show(&step_c, tx).speculation.unwrap();
        assert_eq!(
            standing.refusal.as_deref(),
            Some("speculation_depth_exceeded")
        );
        standing.max_depth
    };
    // Every limit is 1: two gates would stack beneath C.
    assert_eq!(refused(&t, &mut tx), 1);
    // The orchestration policy alone allows 2: each project still says 1.
    t.set_policy(
        &Opt {
            max_depth: 2,
            ..Opt::on()
        },
        &mut tx,
    );
    assert_eq!(refused(&t, &mut tx), 1);
    // The working project allows 2. C and its prerequisite B are both in it, so its
    // limit is the only target limit that applies to C; B's own record on target 1's
    // gate stays as it is.
    own_policy(&t.second.root, true, 2);
    let run_c = t.dispatch(2, &c, "op-c", &mut tx).unwrap().run.run_id;
    let work =
        t.m.runtime
            .run_status(&t.owner(), &run_c)
            .unwrap()
            .provisional
            .unwrap();
    assert_eq!((work.step_id.clone(), work.depth), (step_c.clone(), 2));
    assert_eq!(
        (work.gates[0].commit.clone(), work.gates[0].gate_ids.clone()),
        (sha('b'), vec![gate_b])
    );
    assert_eq!(t.status().work.len(), 2);
    // A third layer is refused at 2 as the second was at 1.
    let gate_c = t.link(&run_c, &sha('c'), &mut tx);
    let d = ticket_in(&t.second_store, "SEC", "Third layer");
    let step_d = t.define(2, &d.slug, &[(2, &c.slug)], &mut tx);
    assert_eq!(
        refusal(t.dispatch(2, &d, "op-d", &mut tx)),
        ReadinessError::ProvisionalRefused
    );
    assert_eq!(
        t.show(&step_d, &mut tx)
            .speculation
            .unwrap()
            .refusal
            .as_deref(),
        Some("speculation_depth_exceeded")
    );
    // A's gate turns green: B is confirmed, C still rests on B's own pending gate,
    // and only two gates are left beneath D, which may now start.
    t.ci_says(&t.run_a, &sha('a'), GateState::Passed, 1, &mut tx);
    assert_eq!(t.states(&t.step_b), ["provisional", "confirmed"]);
    assert_eq!(t.states(&step_c), ["provisional"]);
    let run_d = t.dispatch(2, &d, "op-d", &mut tx).unwrap().run.run_id;
    let work =
        t.m.runtime
            .run_status(&t.owner(), &run_d)
            .unwrap()
            .provisional
            .unwrap();
    assert_eq!(
        (work.depth, work.gates[0].gate_ids.clone()),
        (2, vec![gate_c])
    );
}

#[test]
fn hs2_jsf985_a_gate_result_that_arrived_during_a_restart_is_applied_once() {
    tx!(tx);
    let t = two(Some(&Opt::on()), &mut tx);
    let owner = t.owner();
    t.dispatch_b(&mut tx).unwrap();
    // The orchestrator is down: the watch records a verdict on this machine, and
    // nothing that would publish it runs.
    let gates = GateRegistry::new(&t.m.home);
    let arrive = |state, order, tx: &mut Transaction<'_>| {
        gates
            .apply(
                &owner,
                &t.gate_a,
                verdict(CI, state, order, &sha('a'), Coverage::Exact),
                tx,
            )
            .unwrap();
    };
    arrive(GateState::Failed, 1, &mut tx);
    assert_eq!(t.only().state, SpeculationState::Provisional);
    assert_eq!(t.states(&t.step_b), ["provisional"]);

    // It starts again: reconciliation applies the result, with its note, once.
    let restarted = t.m.runtime.clone();
    restarted
        .runs_reconcile(&owner, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(t.states(&t.step_b), ["provisional", "at_risk"]);
    assert_eq!(t.only().state, SpeculationState::AtRisk);
    assert_eq!(t.notes("Provisional work at risk").len(), 1);
    for _ in 0..2 {
        restarted
            .runs_reconcile(&owner, Some(&worker()), &mut tx)
            .unwrap();
        restarted
            .run_reconcile(&owner, &t.run_a, Some(&worker()), &mut tx)
            .unwrap();
    }
    assert_eq!(t.states(&t.step_b), ["provisional", "at_risk"]);
    assert_eq!(t.notes("Provisional work at risk").len(), 1);

    // Down again while a later attempt passes. This time the first thing to run is a
    // scheduling pass, which applies it before it decides anything.
    arrive(GateState::Passed, 2, &mut tx);
    assert_eq!(t.only().state, SpeculationState::AtRisk);
    let tick = restarted
        .scheduler_tick(
            &owner,
            &TickRequest { dry_run: true },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(tick.outcomes.is_empty());
    assert_eq!(t.states(&t.step_b), ["provisional", "at_risk", "confirmed"]);
    assert_eq!(t.only().state, SpeculationState::Confirmed);
    // An older result delivered late is ignored by the gate and so by the ledger.
    arrive(GateState::Failed, 1, &mut tx);
    restarted
        .runs_reconcile(&owner, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(t.only().state, SpeculationState::Confirmed);
    assert_eq!(t.states(&t.step_b).len(), 3);

    // A note that could not be written while the ticket store was away stays owed
    // and is written by the next reconciliation, once.
    let u = two(Some(&Opt::on()), &mut tx);
    u.dispatch_b(&mut tx).unwrap();
    GateRegistry::new(&u.m.home)
        .apply(
            &u.owner(),
            &u.gate_a,
            verdict(CI, GateState::Failed, 1, &sha('a'), Coverage::Exact),
            &mut tx,
        )
        .unwrap();
    // The ledger half alone, as the gate publish point runs it.
    u.m.runtime.provisional_gate_changed(&u.owner(), 200);
    assert_eq!(u.only().state, SpeculationState::AtRisk);
    assert!(u.notes("Provisional work at risk").is_empty());
    u.m.runtime
        .provisional_reconcile(&u.owner(), &mut tx)
        .unwrap();
    u.m.runtime
        .provisional_reconcile(&u.owner(), &mut tx)
        .unwrap();
    assert_eq!(u.notes("Provisional work at risk").len(), 1);
    assert_eq!(u.states(&u.step_b), ["provisional", "at_risk"]);
}

#[test]
fn hs2_jsf985_the_scheduler_places_provisional_work_only_under_automatic_placement() {
    tx!(tx);
    // Opted in, automatic placement off: the pass is a dry run. It shows the start
    // it would make and starts and records nothing.
    let dry = two(Some(&Opt::on()), &mut tx);
    let tick = dry.tick(&mut tx);
    let starts: Vec<_> = tick
        .view
        .schedule
        .placements
        .iter()
        .map(|placement| {
            (
                placement.steps.clone(),
                placement.reason.code(),
                placement.provisional,
            )
        })
        .collect();
    assert_eq!(
        starts,
        [(vec![dry.step_b.clone()], "provisional_work", true)]
    );
    assert!(tick.outcomes.is_empty() && dry.sessions.calls().is_empty());
    assert!(dry.status().work.is_empty() && dry.events(&dry.step_b).is_empty());
    let explained = dry.explain(&dry.step_b, &mut tx);
    assert_eq!(
        (explained.standing.as_str(), explained.reason.as_str()),
        ("start", "provisional_work")
    );
    // `explain` names the gate the work would be provisional on.
    let standing = explained.speculation.unwrap();
    assert_eq!(
        (standing.allowed, standing.bases[0].gate_ids.clone()),
        (true, vec![dry.gate_a.clone()])
    );

    // Automatic placement on: the session is started and the work recorded with its
    // gate, under the start's own operation.
    let t = two(
        Some(&Opt {
            automatic: true,
            ..Opt::on()
        }),
        &mut tx,
    );
    let tick = t.tick(&mut tx);
    assert_eq!(tick.outcomes.len(), 1);
    assert_eq!(
        (
            tick.outcomes[0].outcome.as_str(),
            tick.outcomes[0].step_id.clone()
        ),
        ("started", Some(t.step_b.clone()))
    );
    let [SessionCall::Start(request)] = &t.sessions.calls()[..] else {
        panic!("one start");
    };
    assert_eq!(
        (request.target_id.clone(), request.steps.clone()),
        (id(2), vec![t.step_b.clone()])
    );
    let work = t.only();
    assert_eq!(
        (
            work.operation_id.clone(),
            work.state,
            work.gates[0].gate_ids.clone()
        ),
        (
            format!("sched-start-{}", t.step_b),
            SpeculationState::Provisional,
            vec![t.gate_a.clone()]
        )
    );
    assert_eq!(t.states(&t.step_b), ["provisional"]);
    let logged =
        t.m.runtime
            .scheduler_decisions(&t.owner(), &DecisionsRequest::default())
            .unwrap()
            .entries
            .into_iter()
            .filter_map(|entry| entry.record.reason)
            .collect::<Vec<_>>();
    assert!(logged.contains(&"provisional_work".to_owned()));
    // The next pass hands the work to that session; nothing is started or recorded
    // twice, and a later dispatch of the same step keeps the one record.
    let again = t.tick(&mut tx);
    assert!(again.outcomes.is_empty());
    assert_eq!(t.sessions.calls().len(), 1);
    t.dispatch_b(&mut tx).unwrap();
    assert_eq!(t.status().work.len(), 1);
    assert_eq!(t.states(&t.step_b), ["provisional"]);

    // Confirmed work of the same priority goes first: with one session allowed on
    // the target it gets the session and the provisional work waits for the cap.
    let ranked = two(
        Some(&Opt {
            automatic: true,
            cap: 1,
            ..Opt::on()
        }),
        &mut tx,
    );
    let confirmed = ticket_in(&ranked.second_store, "SEC", "Confirmed work");
    let step_confirmed = ranked.define(2, &confirmed.slug, &[], &mut tx);
    let tick = ranked.tick(&mut tx);
    assert_eq!(tick.view.schedule.ranking[0].step_id, step_confirmed);
    assert_eq!(tick.view.schedule.ranking[0].score.risk_percent, 0);
    assert_eq!(tick.view.schedule.ranking[1].score.risk_percent, 50);
    assert_eq!(tick.outcomes.len(), 1);
    assert_eq!(tick.outcomes[0].step_id, Some(step_confirmed));
    assert_eq!(
        tick.view.schedule.unplaced[0].reason.code(),
        "target_cap",
        "the provisional item waits"
    );
    assert!(ranked.status().work.is_empty());
}

#[test]
fn hs2_xhq1eh_a_person_is_asked_before_building_ahead_unless_remembered() {
    tx!(tx);
    let opt = Opt {
        automatic: true,
        ask_ahead: true,
        ..Opt::on()
    };
    let pending = |t: &Two, tx: &mut Transaction<'_>| {
        t.m.runtime
            .permission_list(&t.owner(), tx)
            .unwrap()
            .requests
            .into_iter()
            .map(|view| view.request)
            .filter(|request| request.key.action == PermissionAction::ProvisionalDispatch)
            .map(|request| (request.request_id, request.operation_id, request.state))
            .collect::<Vec<_>>()
    };
    // A dispatch asks once, however often it is repeated, and proceeds once allowed.
    let t = two(Some(&opt), &mut tx);
    for _ in 0..2 {
        permission_request(t.dispatch_b(&mut tx));
    }
    let asked = pending(&t, &mut tx);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        (asked[0].1.as_str(), asked[0].2),
        ("op-b", PermissionState::Pending)
    );
    assert!(t.status().work.is_empty());
    answer(
        &t.m.runtime,
        &t.owner(),
        &asked[0].0,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    t.dispatch_b(&mut tx).unwrap();
    // Used once the run was recorded.
    assert_eq!(pending(&t, &mut tx)[0].2, PermissionState::Used);
    assert_eq!(t.only().operation_id, "op-b");
    assert_eq!(
        t.audited("provisional_dispatch"),
        [
            (AuditOutcome::Denied, Some("permission_required".into())),
            (AuditOutcome::Denied, Some("permission_required".into())),
            (AuditOutcome::Allowed, Some("provisional".into())),
        ]
    );

    // A person's no is final for that operation.
    let denied = two(Some(&opt), &mut tx);
    assert!(denied.dispatch_b(&mut tx).is_err());
    let asked = pending(&denied, &mut tx);
    answer(
        &denied.m.runtime,
        &denied.owner(),
        &asked[0].0,
        PermissionAnswer::Deny,
        &mut tx,
    );
    assert_eq!(
        refusal(denied.dispatch_b(&mut tx)),
        ReadinessError::ProvisionalRefused
    );
    assert!(denied.status().work.is_empty());

    // The scheduler's start waits the same way, and the answer is used by it.
    let s = two(Some(&opt), &mut tx);
    let tick = s.tick(&mut tx);
    assert_eq!(tick.outcomes[0].outcome, "permission_required");
    assert!(s.sessions.calls().is_empty() && s.status().work.is_empty());
    let asked = pending(&s, &mut tx);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        tick.outcomes[0].detail.as_deref(),
        Some(asked[0].0.as_str())
    );
    assert_eq!(asked[0].1, format!("sched-start-{}", s.step_b));
    answer(
        &s.m.runtime,
        &s.owner(),
        &asked[0].0,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    let tick = s.tick(&mut tx);
    assert_eq!(tick.outcomes[0].outcome, "started");
    assert_eq!(pending(&s, &mut tx)[0].2, PermissionState::Used);
    assert_eq!(s.only().state, SpeculationState::Provisional);
}

#[test]
fn hs2_jsf985_nothing_is_read_on_a_timer_while_a_gate_is_pending() {
    tx!(tx);
    let t = two(
        Some(&Opt {
            automatic: true,
            ..Opt::on()
        }),
        &mut tx,
    );
    let owner = t.owner();
    let mut state = SchedulerFollowState::default();
    let step = |state: &mut SchedulerFollowState, tx: &mut Transaction<'_>| {
        t.m.runtime.scheduler_follow_step(&owner, state, 0, tx)
    };
    // The loop starts and places the provisional work; the pass that follows the
    // start's own events hands the work to the session it began.
    assert_eq!(step(&mut state, &mut tx).trigger, Some("start"));
    while step(&mut state, &mut tx).trigger.is_some() {}
    assert_eq!(t.sessions.calls().len(), 1);
    assert_eq!(t.only().state, SpeculationState::Provisional);

    // The gate pends. However often the loop steps, it runs no pass, contacts no
    // target, reads no adapter and rewrites nothing.
    let log = |t: &Two| {
        t.m.runtime
            .scheduler_decisions(&owner, &DecisionsRequest::default())
            .unwrap()
            .entries
            .len()
    };
    let before = (
        t.probe.asked.load(Ordering::SeqCst),
        log(&t),
        std::fs::read(t.file()).unwrap(),
        t.audited("gates_observe").len(),
        t.states(&t.step_b),
    );
    for _ in 0..5 {
        let idle = step(&mut state, &mut tx);
        assert_eq!(
            (idle.active, idle.trigger, idle.retry_at_ms),
            (true, None, None)
        );
    }
    assert_eq!(
        (
            t.probe.asked.load(Ordering::SeqCst),
            log(&t),
            std::fs::read(t.file()).unwrap(),
            t.audited("gates_observe").len(),
            t.states(&t.step_b),
        ),
        before
    );
    // The result itself is the wake-up: one pass, then quiet again.
    t.ci_says(&t.run_a, &sha('a'), GateState::Passed, 1, &mut tx);
    assert_eq!(t.states(&t.step_b), ["provisional", "confirmed"]);
    assert_eq!(step(&mut state, &mut tx).trigger, Some("event"));
    while step(&mut state, &mut tx).trigger.is_some() {}
    for _ in 0..3 {
        assert_eq!(step(&mut state, &mut tx).trigger, None);
    }
    assert_eq!(t.sessions.calls().len(), 1);
}

#[test]
fn hs2_jsf985_reopening_keeps_the_record_and_is_audited() {
    tx!(tx);
    let t = two(Some(&Opt::on()), &mut tx);
    let owner = t.owner();
    let reopen = |step_id: &str, reason: &str, actor, tx: &mut Transaction<'_>| {
        t.m.runtime.readiness_reopen(
            &owner,
            &ReopenProvisional {
                step_id: step_id.into(),
                reason: reason.into(),
            },
            actor,
            tx,
        )
    };
    let who = worker();
    // No step, then a step with no provisional work.
    assert_eq!(
        refusal(reopen(
            &id(999),
            "fix_changed_contract",
            Some(&who),
            &mut tx
        )),
        ReadinessError::StepNotFound
    );
    assert_eq!(
        refusal(reopen(
            &t.step_b,
            "fix_changed_contract",
            Some(&who),
            &mut tx
        )),
        ReadinessError::NotProvisional
    );
    assert_eq!(
        OrchestrationError::Readiness(ReadinessError::NotProvisional).code(),
        "readiness_not_provisional"
    );
    t.dispatch_b(&mut tx).unwrap();
    // Who reopens is recorded, and the reason is a code, never prose.
    for (reason, actor) in [
        ("fix_changed_contract", None),
        ("", Some(&who)),
        ("The fix changed the API", Some(&who)),
    ] {
        assert!(
            matches!(
                reopen(&t.step_b, reason, actor, &mut tx),
                Err(OrchestrationError::Invalid(_))
            ),
            "{reason:?}"
        );
    }
    t.ci_says(&t.run_a, &sha('a'), GateState::Failed, 1, &mut tx);
    let row = reopen(&t.step_b, "fix_changed_contract", Some(&who), &mut tx).unwrap();
    assert_eq!(
        (
            row.state,
            row.reopen_reason.as_deref(),
            row.explain.as_str()
        ),
        (
            SpeculationState::Reopened,
            Some("fix_changed_contract"),
            "reopened"
        )
    );
    assert_eq!(
        t.audited("provisional_reopen"),
        [(AuditOutcome::Changed, Some("fix_changed_contract".into()))]
    );
    assert_eq!(t.states(&t.step_b), ["provisional", "at_risk", "reopened"]);
    assert_eq!(t.notes("Provisional work reopened").len(), 1);
    // Again: the same answer, and nothing more is recorded.
    assert_eq!(
        reopen(&t.step_b, "another_reason", Some(&who), &mut tx).unwrap(),
        row
    );
    assert_eq!(t.states(&t.step_b).len(), 3);
    assert_eq!(t.audited("provisional_reopen").len(), 1);
    // The record stays, off the repair list, and a later green result does not
    // quietly confirm work a person said was invalidated.
    assert_eq!(t.status().work, std::slice::from_ref(&row));
    assert!(t.status().repair_first.is_empty());
    t.ci_says(&t.run_a, &sha('a'), GateState::Passed, 2, &mut tx);
    assert_eq!(t.only().state, SpeculationState::Reopened);
    assert_eq!(t.states(&t.step_b).len(), 3);

    // Confirmed work cannot be reopened: a confirmed base invalidates nothing.
    let u = two(Some(&Opt::on()), &mut tx);
    u.dispatch_b(&mut tx).unwrap();
    u.ci_says(&u.run_a, &sha('a'), GateState::Passed, 1, &mut tx);
    assert_eq!(
        refusal(u.m.runtime.readiness_reopen(
            &u.owner(),
            &ReopenProvisional {
                step_id: u.step_b.clone(),
                reason: "fix_changed_contract".into(),
            },
            Some(&who),
            &mut tx,
        )),
        ReadinessError::NotProvisional
    );
    assert_eq!(u.only().state, SpeculationState::Confirmed);
}

#[test]
fn hs2_jsf985_a_gate_that_is_superseded_or_lost_is_listed_for_re_evaluation() {
    tx!(tx);
    let t = two(Some(&Opt::on()), &mut tx);
    let owner = t.owner();
    t.dispatch_b(&mut tx).unwrap();
    // The watch is lost: nothing is known, and nothing is concluded from that.
    GateRegistry::new(&t.m.home)
        .apply(
            &owner,
            &t.gate_a,
            hotsheet_ticketing::orchestration::GateReading::new(GateState::Unknown)
                .reason("watch_lost"),
            &mut tx,
        )
        .unwrap();
    t.m.runtime.provisional_reconcile(&owner, &mut tx).unwrap();
    assert_eq!(t.states(&t.step_b), ["provisional", "reevaluate"]);
    let view = t.status();
    assert_eq!(view.work[0].state, SpeculationState::Provisional);
    assert_eq!(view.reevaluate.len(), 1);
    assert_eq!(
        (view.reevaluate[0].commit.clone(), view.reevaluate[0].result),
        (sha('a'), GateResult::Unknown)
    );
    assert!(view.repair_first.is_empty());
    // Watched again and pending: provisional again, as its own event.
    t.ci.set(
        &sha('a'),
        hotsheet_ticketing::orchestration::GateReading::new(GateState::Pending),
    );
    t.sync(&t.run_a, &mut tx);
    assert_eq!(
        t.states(&t.step_b),
        ["provisional", "reevaluate", "provisional"]
    );
    assert!(t.status().reevaluate.is_empty());
    // The provider says the branch moved: the gate is superseded with nothing in its
    // place. The work is listed for re-evaluation, never confirmed and never dropped.
    t.ci.set(
        &sha('a'),
        hotsheet_ticketing::orchestration::GateReading::new(GateState::Superseded)
            .reason("branch_moved"),
    );
    t.sync(&t.run_a, &mut tx);
    t.sync(&t.run_a, &mut tx);
    assert_eq!(
        t.states(&t.step_b),
        ["provisional", "reevaluate", "provisional", "reevaluate"]
    );
    let view = t.status();
    assert_eq!(
        (view.work[0].state, view.reevaluate[0].result),
        (SpeculationState::Provisional, GateResult::Superseded)
    );
}

#[test]
fn hs2_jsf985_routes_share_the_runtime_and_are_machine_local() {
    tx!(tx);
    let t = two(Some(&Opt::on()), &mut tx);
    let owner = t.owner();
    let path = |operation: &str| format!("/checkouts/{owner}/orchestration/readiness/{operation}");
    for operation in ["speculation", "reopen"] {
        assert!(is_path(&path(operation)) && is_machine_path(&path(operation)));
    }
    let post = |operation: &str, mut body: Value, tx: &mut Transaction<'_>| {
        body["actor"] = json!({"role": "ai", "id": "worker-1"});
        t.m.runtime.request("POST", &path(operation), &body, tx)
    };
    let empty = post("speculation", json!({}), &mut tx).unwrap();
    assert_eq!(
        empty,
        json!({"enabled": true, "work": [], "repair_first": [], "reevaluate": []})
    );
    t.dispatch_b(&mut tx).unwrap();
    t.ci_says(&t.run_a, &sha('a'), GateState::Failed, 1, &mut tx);
    let listed = post("speculation", json!({}), &mut tx).unwrap();
    assert_eq!(listed["work"][0]["state"], "at_risk");
    assert_eq!(listed["work"][0]["gates"][0]["gate_ids"], json!([t.gate_a]));
    assert_eq!(listed["repair_first"][0]["stacked"], json!([t.step_b]));
    // The step's own views carry the same record.
    let shown = post("show", json!({"step_id": t.step_b}), &mut tx).unwrap();
    assert_eq!(shown["provisional"], listed["work"][0]);
    let reopened = post(
        "reopen",
        json!({"step_id": t.step_b, "reason": "fix_changed_contract"}),
        &mut tx,
    )
    .unwrap();
    assert_eq!(reopened["state"], "reopened");
    for (body, code) in [
        (json!({"step_id": t.step_b}), "invalid_request"),
        (
            json!({"step_id": t.step_b, "reason": "x", "extra": true}),
            "invalid_request",
        ),
        (
            json!({"step_id": id(999), "reason": "fix_changed_contract"}),
            "readiness_step_not_found",
        ),
    ] {
        assert_eq!(post("reopen", body, &mut tx).unwrap_err().code(), code);
    }
}
