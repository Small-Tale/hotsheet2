//! HS2-5JBYPV: CI and deployment gates of runs through the shared runtime, with a
//! scripted provider adapter and with the real GitHub Actions adapter over a Job store.
//! Synthetic repositories and commit ids only.
use std::collections::HashMap;
use std::sync::Condvar;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use hotsheet_jobs::{Acquire, ActiveStatus, FinalStatus, JobStore, Outcome};
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    ApprovalGate, ApprovalPolicy, ApprovalState, AuditOutcome, Coverage, CreateGrant, DenialReason,
    Gate, GateError, GateEvidence, GateFilter, GateKind, GateReading, GateState, PolicyBody, Run,
    RunEventKind, RunEventSource, SetPolicy, TargetAction, TargetPolicy,
};
use serde_json::json;

use super::tickets::{Machine, bind_and_grant, human, machine, unsupported, worker};
use super::*;

pub(super) const CI: &str = "scripted-ci";
pub(super) fn sha(digit: char) -> String {
    std::iter::repeat_n(digit, 40).collect()
}

/// A provider adapter a test moves by hand. `wait` blocks on a condition variable, so a
/// test proves a waiter is woken by the change and not by a timer.
pub(super) struct Scripted {
    name: &'static str,
    kind: GateKind,
    pub(super) readings: Mutex<HashMap<String, GateReading>>,
    starts: Mutex<Vec<(String, bool)>>,
    refuse: AtomicBool,
    cursor: Mutex<u64>,
    changed: Condvar,
    waits: AtomicUsize,
}
impl Scripted {
    pub(super) fn new(name: &'static str, kind: GateKind) -> Arc<Self> {
        Arc::new(Self {
            name,
            kind,
            readings: Mutex::default(),
            starts: Mutex::default(),
            refuse: AtomicBool::new(false),
            cursor: Mutex::new(0),
            changed: Condvar::new(),
            waits: AtomicUsize::new(0),
        })
    }
    pub(super) fn set(&self, commit: &str, reading: GateReading) {
        self.readings.lock().unwrap().insert(commit.into(), reading);
        *self.cursor.lock().unwrap() += 1;
        self.changed.notify_all();
    }
    pub(super) fn starts(&self) -> Vec<(String, bool)> {
        self.starts.lock().unwrap().clone()
    }
}
impl GateProvider for Scripted {
    fn provider(&self) -> &'static str {
        self.name
    }
    fn kind(&self) -> GateKind {
        self.kind
    }
    fn start(&self, gate: &Gate, _: &Run, again: bool) -> Result<(), GateError> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(GateError::ProviderUnavailable);
        }
        self.starts
            .lock()
            .unwrap()
            .push((gate.commit.clone(), again));
        let mut readings = self.readings.lock().unwrap();
        if again || !readings.contains_key(&gate.commit) {
            readings.insert(gate.commit.clone(), GateReading::new(GateState::Pending));
        }
        Ok(())
    }
    fn read(&self, gate: &Gate) -> Result<GateReading, GateError> {
        self.readings
            .lock()
            .unwrap()
            .get(&gate.commit)
            .cloned()
            .ok_or(GateError::ProviderUnavailable)
    }
    fn cursor(&self, _: &str) -> u64 {
        *self.cursor.lock().unwrap()
    }
    fn wait(&self, _: &str, cursor: u64, timeout: Duration) {
        self.waits.fetch_add(1, Ordering::SeqCst);
        let guard = self.cursor.lock().unwrap();
        let _ = self
            .changed
            .wait_timeout_while(guard, timeout, |now| *now == cursor)
            .unwrap();
    }
    fn watching(&self) -> Option<bool> {
        Some(true)
    }
}

pub(super) fn verdict(
    provider: &str,
    state: GateState,
    order: u64,
    commit: &str,
    coverage: Coverage,
) -> GateReading {
    GateReading::new(state).evidence(GateEvidence {
        provider: provider.into(),
        reference: format!("run-{order}"),
        order,
        attempt: 1,
        commit: commit.into(),
        coverage,
        artifact: None,
        exit_code: (provider != CI).then_some(i32::from(state != GateState::Passed)),
        at_ms: 150,
    })
}
/// A machine with a dispatched run on its target, and the actions granted to `worker-1`.
pub(super) fn with_run(actions: &[TargetAction], tx: &mut Transaction<'_>) -> (Machine, String) {
    let m = machine(10, false, unsupported);
    let mut granted = vec![TargetAction::TicketsRead, TargetAction::WorkStart];
    granted.extend_from_slice(actions);
    bind_and_grant(&m, 1, &granted, tx);
    let orchestration = m.orchestration_ticket("Coordinate");
    let target = hotsheet_ticketing::ops::create(
        &m.target_store,
        ulid::Ulid::new(),
        "TGT",
        super::tickets::NOW.into(),
        hotsheet_ticketing::NewTicket {
            title: "Target work".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let run = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            serde_json::from_value(json!({
                "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
                "target_id": id(1), "connection_id": m.target_connection(),
                "native_id": target.id.to_string(), "operation_id": "op-run",
                "worker_id": "worker-a",
            }))
            .unwrap(),
            Some(&worker()),
            tx,
        )
        .unwrap()
        .run
        .run_id;
    (m, run)
}
pub(super) fn link(commit: &str, provider: &str) -> LinkGate {
    serde_json::from_value(
        json!({"commit": commit, "provider": provider, "repository": "example/widgets"}),
    )
    .unwrap()
}
pub(super) fn deploy(operation: &str, production: bool) -> RequestDeployment {
    serde_json::from_value(json!({
        "environment": if production { "production" } else { "beta" },
        "script": "scripts/deploy.sh", "production": production, "operation_id": operation,
    }))
    .unwrap()
}
fn gate_events(m: &Machine, run_id: &str) -> Vec<(GateState, RunEventSource, Option<String>)> {
    m.runtime
        .run_events(&m.owner.id, 0, 500, Some(run_id), 0)
        .unwrap()
        .entries
        .into_iter()
        .filter(|entry| entry.record.kind == RunEventKind::Gate)
        .map(|entry| {
            assert!(entry.record.gate_id.is_some());
            (
                entry.record.gate_state.unwrap(),
                entry.record.source,
                entry.record.reason,
            )
        })
        .collect()
}
fn gate_error(result: Result<impl Sized, OrchestrationError>) -> GateError {
    match result {
        Err(OrchestrationError::Gate(error)) => error,
        Err(other) => panic!("expected a gate error, got {other}"),
        Ok(_) => panic!("expected a gate error"),
    }
}

#[test]
fn hs2_5jbypv_a_linked_ci_gate_follows_typed_readings_and_publishes_gate_events() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, run) = with_run(&[TargetAction::GatesObserve], &mut tx);
    let ci = Scripted::new(CI, GateKind::Ci);
    m.runtime = m.runtime.clone().with_gate_provider(ci.clone());
    let (a, b) = (sha('a'), sha('b'));
    let owner = m.owner.id.clone();
    // Nothing linked: unknown, never passed.
    let empty = m.runtime.run_gate_state(&owner, &run).unwrap();
    assert_eq!(
        (empty.ci, empty.ci_reason.as_deref(), empty.deployment),
        (GateState::Unknown, Some("no_gate"), None)
    );
    // An uppercase commit id is normalized; a short or symbolic one is refused.
    for bad in ["abc1234", "main", "HEAD"] {
        assert!(matches!(
            m.runtime
                .gate_link(&owner, &run, link(bad, CI), Some(&worker()), &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    let linked = m
        .runtime
        .gate_link(
            &owner,
            &run,
            link(&a.to_uppercase(), CI),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(linked.created && linked.superseded.is_empty());
    assert_eq!(linked.watching, Some(true));
    let gate = linked.gate.clone();
    assert_eq!(
        (gate.state, gate.commit.as_str(), gate.kind),
        (GateState::Pending, a.as_str(), GateKind::Ci)
    );
    assert_eq!(gate.linked_by.as_deref(), Some("worker-1"));
    assert_eq!(gate.target_id, id(1));
    assert_eq!(ci.starts(), [(a.clone(), false)]);
    assert_eq!(
        m.last_audit(),
        ("gates_observe".into(), AuditOutcome::Succeeded, None)
    );
    assert_eq!(
        gate_events(&m, &run),
        [
            (
                GateState::Unknown,
                RunEventSource::Orchestrator,
                Some("not_watched".into())
            ),
            (GateState::Pending, RunEventSource::Target, None),
        ]
    );
    // Linking again is the same gate: no second gate, no event, no fresh watch.
    let again = m
        .runtime
        .gate_link(&owner, &run, link(&a, CI), Some(&worker()), &mut tx)
        .unwrap();
    assert!(!again.created);
    assert_eq!(again.gate, gate);
    assert_eq!(ci.starts().last(), Some(&(a.clone(), false)));
    assert_eq!(gate_events(&m, &run).len(), 2);
    // A failed run on the commit itself.
    ci.set(&a, verdict(CI, GateState::Failed, 7, &a, Coverage::Exact));
    let synced = m
        .runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(synced.changed, *std::slice::from_ref(&gate.gate_id));
    assert_eq!(synced.summary.ci, GateState::Failed);
    assert_eq!(synced.summary.commits, *std::slice::from_ref(&a));
    // Reading the same answer again is not news.
    let unchanged = m
        .runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(unchanged.changed.is_empty());
    assert_eq!(gate_events(&m, &run).len(), 3);
    // Linking a failed gate again watches again.
    let rewatched = m
        .runtime
        .gate_link(&owner, &run, link(&a, CI), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(ci.starts().last(), Some(&(a.clone(), true)));
    assert_eq!(rewatched.gate.state, GateState::Pending);
    // A verdict whose evidence is about another commit is not a verdict.
    ci.set(&a, verdict(CI, GateState::Passed, 8, &b, Coverage::Exact));
    let doubtful = m
        .runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(doubtful.summary.ci, GateState::Unknown);
    assert_eq!(
        doubtful.summary.ci_reason.as_deref(),
        Some("evidence_not_covering")
    );
    // A later run on a descendant the provider attests contains the commit passes it.
    ci.set(
        &a,
        verdict(CI, GateState::Passed, 9, &b, Coverage::Ancestor),
    );
    m.runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let passed = m.runtime.run_gate_state(&owner, &run).unwrap();
    assert_eq!((passed.ci, passed.ci_reason), (GateState::Passed, None));
    let evidence = passed.gates[0].evidence.clone().unwrap();
    assert_eq!(
        (evidence.commit.as_str(), evidence.coverage, evidence.order),
        (b.as_str(), Coverage::Ancestor, 9)
    );
    // The same answer by target commit, whatever case the caller used.
    let by_commit = m
        .runtime
        .commit_gate_state(&owner, &id(1), &a.to_uppercase())
        .unwrap();
    assert_eq!(by_commit.ci, GateState::Passed);
    assert_eq!(
        m.runtime.commit_gate_state(&owner, &id(1), &b).unwrap().ci,
        GateState::Unknown
    );
    // A late failure of the older run changes nothing.
    ci.set(&a, verdict(CI, GateState::Failed, 7, &a, Coverage::Exact));
    assert!(
        m.runtime
            .gate_sync(
                &owner,
                &run,
                &SyncGates::default(),
                Some(&worker()),
                &mut tx
            )
            .unwrap()
            .changed
            .is_empty()
    );
    // The run moves to commit b: the old link is superseded and says by which gate.
    let moved = m
        .runtime
        .gate_link(&owner, &run, link(&b, CI), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(moved.superseded, *std::slice::from_ref(&gate.gate_id));
    let old = m.runtime.gate(&owner, &gate.gate_id).unwrap();
    assert_eq!(
        (old.state, old.superseded_by.as_deref()),
        (GateState::Superseded, Some(moved.gate.gate_id.as_str()))
    );
    let now = m.runtime.run_gate_state(&owner, &run).unwrap();
    assert_eq!(
        (now.ci, now.superseded, now.commits.as_slice()),
        (GateState::Pending, 1, std::slice::from_ref(&b))
    );
    assert_eq!(
        m.runtime.commit_gate_state(&owner, &id(1), &a).unwrap().ci,
        GateState::Unknown
    );
    let states: Vec<GateState> = gate_events(&m, &run).into_iter().map(|e| e.0).collect();
    assert_eq!(
        states,
        [
            GateState::Unknown,
            GateState::Pending,
            GateState::Failed,
            GateState::Pending,
            GateState::Unknown,
            GateState::Passed,
            GateState::Superseded,
            GateState::Unknown,
            GateState::Pending,
        ]
    );
    // An unknown run, gate or provider is named as such.
    assert!(matches!(
        m.runtime.run_gate_state(&owner, &id(404)),
        Err(OrchestrationError::Run(_))
    ));
    assert_eq!(
        gate_error(m.runtime.gate(&owner, &id(404))),
        GateError::NotFound
    );
    assert_eq!(
        gate_error(m.runtime.gate_link(
            &owner,
            &run,
            link(&a, "other-ci"),
            Some(&worker()),
            &mut tx
        )),
        GateError::ProviderUnsupported
    );
    assert_eq!(
        m.last_audit(),
        (
            "gates_observe".into(),
            AuditOutcome::Failed,
            Some("gate_provider_unsupported".into())
        )
    );
}

#[test]
fn hs2_5jbypv_gates_need_a_grant_a_repository_and_an_adapter_that_starts() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    // No gates_observe grant: nothing is linked and nothing is started.
    let (mut m, run) = with_run(&[], &mut tx);
    let ci = Scripted::new(CI, GateKind::Ci);
    m.runtime = m.runtime.clone().with_gate_provider(ci.clone());
    let (a, owner) = (sha('a'), m.owner.id.clone());
    for actor in [Some(worker()), None] {
        let result = m
            .runtime
            .gate_link(&owner, &run, link(&a, CI), actor.as_ref(), &mut tx);
        assert!(matches!(
            result,
            Err(OrchestrationError::Denied(DenialReason::ActionNotGranted)
                | OrchestrationError::Invalid(_))
        ));
    }
    assert!(matches!(
        m.runtime.gate_sync(
            &owner,
            &run,
            &SyncGates::default(),
            Some(&worker()),
            &mut tx
        ),
        Err(OrchestrationError::Denied(_))
    ));
    assert!(ci.starts().is_empty());
    assert!(
        m.runtime
            .gate_summary(&owner, &GateFilter::default())
            .unwrap()
            .gates
            .is_empty()
    );
    // A second grant allows it.
    m.runtime
        .grant(
            &m.reference(1),
            CreateGrant {
                grant_id: None,
                actor_id: "observer-1".into(),
                actions: vec![TargetAction::GatesObserve],
                session_scope: None,
                expires_at_ms: 10_000,
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let observer = MutationActor::parse("ai", Some("observer-1".into())).unwrap();
    // The target declares no repository, so the link must name one.
    let unnamed: LinkGate = serde_json::from_value(json!({"commit": a, "provider": CI})).unwrap();
    assert!(matches!(
        m.runtime
            .gate_link(&owner, &run, unnamed, Some(&observer), &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    // An adapter that cannot start leaves the link standing as unknown, and the same
    // request starts it once the adapter can.
    ci.refuse.store(true, Ordering::SeqCst);
    assert_eq!(
        gate_error(
            m.runtime
                .gate_link(&owner, &run, link(&a, CI), Some(&observer), &mut tx)
        ),
        GateError::ProviderUnavailable
    );
    let standing = m.runtime.run_gate_state(&owner, &run).unwrap();
    assert_eq!(
        (standing.ci, standing.ci_reason.as_deref()),
        (GateState::Unknown, Some("gate_provider_unavailable"))
    );
    ci.refuse.store(false, Ordering::SeqCst);
    let linked = m
        .runtime
        .gate_link(&owner, &run, link(&a, CI), Some(&observer), &mut tx)
        .unwrap();
    assert!(!linked.created);
    assert_eq!(linked.gate.state, GateState::Pending);
    assert_eq!(ci.starts(), [(a.clone(), true)]);
    // Declared repository identities: only a GitHub one names a repository, and a
    // request must agree with it.
    for (identity, repository) in [
        (
            "https://github.com/Example/Widgets",
            Some("Example/Widgets"),
        ),
        (
            "https://github.com/example/widgets.git",
            Some("example/widgets"),
        ),
        ("ssh://github.com/example/widgets/", Some("example/widgets")),
        ("https://gitlab.example/example/widgets", None),
        ("https://github.com/example", None),
        ("https://github.com/example/widgets/extra", None),
    ] {
        assert_eq!(
            super::super::gates::github_repository(identity).as_deref(),
            repository,
            "{identity}"
        );
    }
    let manifest = m.runtime.declarations(&owner).unwrap();
    m.runtime
        .rename(
            &m.reference(1),
            serde_json::from_value(json!({
                "expected_revision": manifest.revision, "label": "Target",
                "expected_repository_identity": "https://github.com/example/widgets",
            }))
            .unwrap(),
            &mut tx,
        )
        .unwrap();
    let repository = |requested: Option<&str>| {
        m.runtime
            .gate_repository(&owner, &id(1), requested.map(Into::into))
    };
    assert_eq!(repository(None).unwrap(), "example/widgets");
    assert_eq!(
        repository(Some("Example/Widgets")).unwrap(),
        "Example/Widgets"
    );
    assert_eq!(
        gate_error(repository(Some("example/other"))),
        GateError::RepositoryMismatch
    );
}

#[test]
fn hs2_5jbypv_a_sync_and_a_follower_wait_on_the_adapter_and_never_spin() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, run) = with_run(&[TargetAction::GatesObserve], &mut tx);
    let ci = Scripted::new(CI, GateKind::Ci);
    m.runtime = m.runtime.clone().with_gate_provider(ci.clone());
    let (a, owner) = (sha('a'), m.owner.id.clone());
    // No gate: a follower is idle and waits on nothing.
    let mut follow = GateFollowState::default();
    assert_eq!(
        m.runtime
            .gate_follow_step(&owner, &mut follow, 15_000, &mut tx),
        GateFollowStep::default()
    );
    assert_eq!(ci.waits.load(Ordering::SeqCst), 0);
    let gate = m
        .runtime
        .gate_link(&owner, &run, link(&a, CI), Some(&worker()), &mut tx)
        .unwrap()
        .gate
        .gate_id;
    // A wait with nothing new returns at its bound with nothing changed.
    let started = std::time::Instant::now();
    let quiet = m
        .runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates { wait_ms: 150 },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(quiet.changed.is_empty());
    assert!(started.elapsed() >= Duration::from_millis(140));
    assert_eq!(ci.waits.load(Ordering::SeqCst), 1);
    // A wait is ended by the adapter's change, long before its bound.
    let (provider, commit) = (ci.clone(), a.clone());
    let waits = ci.waits.load(Ordering::SeqCst);
    let change = std::thread::spawn(move || {
        while provider.waits.load(Ordering::SeqCst) == waits {
            std::thread::yield_now();
        }
        provider.set(
            &commit,
            verdict(CI, GateState::Failed, 3, &commit, Coverage::Exact),
        );
    });
    let started = std::time::Instant::now();
    let woken = m
        .runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates { wait_ms: 15_000 },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    change.join().unwrap();
    assert_eq!(woken.changed, *std::slice::from_ref(&gate));
    assert!(started.elapsed() < Duration::from_secs(10));
    // A failed gate is settled: the follower has nothing to wait for and ends.
    let waits = ci.waits.load(Ordering::SeqCst);
    assert!(
        !m.runtime
            .gate_follow_step(&owner, &mut follow, 15_000, &mut tx)
            .active
    );
    assert_eq!(ci.waits.load(Ordering::SeqCst), waits);
    // Watched again, the follower holds one wait open and reports the change.
    m.runtime
        .gate_link(&owner, &run, link(&a, CI), Some(&worker()), &mut tx)
        .unwrap();
    let (provider, commit) = (ci.clone(), a.clone());
    let change = std::thread::spawn(move || {
        while provider.waits.load(Ordering::SeqCst) == waits {
            std::thread::yield_now();
        }
        provider.set(
            &commit,
            verdict(CI, GateState::Passed, 4, &commit, Coverage::Exact),
        );
    });
    let step = m
        .runtime
        .gate_follow_step(&owner, &mut follow, 15_000, &mut tx);
    change.join().unwrap();
    assert_eq!(
        step,
        GateFollowStep {
            active: true,
            changed: vec![gate.clone()],
            // HS2-VVHVF0: the scripted adapter reports itself observed.
            watching: Some(true),
        }
    );
    assert_eq!(
        gate_events(&m, &run).last(),
        Some(&(GateState::Passed, RunEventSource::Target, None))
    );
    assert!(
        !m.runtime
            .gate_follow_step(&owner, &mut follow, 15_000, &mut tx)
            .active
    );
    // A lost watch is still followed, because the monitor may reattach it; a watch
    // that was stopped is not.
    let b = sha('b');
    let second = m
        .runtime
        .gate_link(&owner, &run, link(&b, CI), Some(&worker()), &mut tx)
        .unwrap()
        .gate
        .gate_id;
    ci.set(
        &b,
        GateReading::new(GateState::Unknown).reason("watch_lost"),
    );
    let lost = m.runtime.gate_follow_step(&owner, &mut follow, 0, &mut tx);
    assert_eq!((lost.active, lost.changed), (true, vec![second.clone()]));
    assert!(
        m.runtime
            .gate_follow_step(&owner, &mut follow, 0, &mut tx)
            .active
    );
    ci.set(
        &b,
        GateReading::new(GateState::Unknown).reason("watch_cancelled"),
    );
    assert_eq!(
        m.runtime
            .gate_follow_step(&owner, &mut follow, 0, &mut tx)
            .changed,
        *std::slice::from_ref(&second)
    );
    assert!(
        !m.runtime
            .gate_follow_step(&owner, &mut follow, 0, &mut tx)
            .active
    );
    // A follower acts under the linking actor's grant. Once that is revoked, a fresh
    // follower leaves the gate as it stands.
    m.runtime
        .gate_link(&owner, &run, link(&b, CI), Some(&worker()), &mut tx)
        .unwrap();
    let grant = m.runtime.grants(&m.reference(1), &mut tx).unwrap()[0]
        .grant
        .grant_id
        .clone();
    m.runtime
        .revoke(&m.reference(1), &grant, Some(&human()), &mut tx)
        .unwrap();
    ci.set(&b, verdict(CI, GateState::Passed, 5, &b, Coverage::Exact));
    let mut fresh = GateFollowState::default();
    let denied = m.runtime.gate_follow_step(&owner, &mut fresh, 0, &mut tx);
    assert_eq!((denied.active, denied.changed.len()), (true, 0));
    assert_eq!(
        m.runtime.gate(&owner, &second).unwrap().state,
        GateState::Pending
    );
}

#[test]
fn hs2_5jbypv_a_deployment_needs_a_passed_gate_a_grant_and_a_person_for_production() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, run) = with_run(
        &[TargetAction::GatesObserve, TargetAction::DeploymentsRun],
        &mut tx,
    );
    let ci = Scripted::new(CI, GateKind::Ci);
    m.runtime = m.runtime.clone().with_gate_provider(ci.clone());
    let (a, b, owner) = (sha('a'), sha('b'), m.owner.id.clone());
    let request = |m: &Machine, body: RequestDeployment, tx: &mut Transaction<'_>| {
        m.runtime
            .gate_deploy(&owner, &run, body, Some(&worker()), tx)
    };
    let approvals =
        |m: &Machine, tx: &mut Transaction<'_>| m.runtime.approvals(&owner, tx).unwrap();
    // No CI gate, a pending one and a failed one all refuse, and ask nobody.
    assert_eq!(
        gate_error(request(&m, deploy("deploy-1", true), &mut tx)),
        GateError::NotPassed
    );
    let ci_gate = m
        .runtime
        .gate_link(&owner, &run, link(&a, CI), Some(&worker()), &mut tx)
        .unwrap()
        .gate
        .gate_id;
    assert_eq!(
        gate_error(request(&m, deploy("deploy-1", true), &mut tx)),
        GateError::NotPassed
    );
    ci.set(&a, verdict(CI, GateState::Failed, 1, &a, Coverage::Exact));
    let sync = |m: &Machine, tx: &mut Transaction<'_>| {
        m.runtime
            .gate_sync(&owner, &run, &SyncGates::default(), Some(&worker()), tx)
            .unwrap()
    };
    sync(&m, &mut tx);
    assert_eq!(
        gate_error(request(&m, deploy("deploy-1", true), &mut tx)),
        GateError::NotPassed
    );
    assert!(approvals(&m, &mut tx).is_empty());
    ci.set(&a, verdict(CI, GateState::Passed, 2, &a, Coverage::Exact));
    sync(&m, &mut tx);
    // Requests are checked before anything is recorded.
    for body in [
        json!({"environment":"beta","script":"/opt/deploy.sh","operation_id":"deploy-x"}),
        json!({"environment":"beta","script":"../deploy.sh","operation_id":"deploy-x"}),
        json!({"environment":"be ta","script":"scripts/deploy.sh","operation_id":"deploy-x"}),
        json!({"environment":"beta","script":"scripts/deploy.sh","operation_id":"deploy x"}),
    ] {
        assert!(matches!(
            request(&m, serde_json::from_value(body).unwrap(), &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    // An actor without `deployments_run` is denied and nothing is recorded.
    let stranger = MutationActor::parse("ai", Some("worker-2".into())).unwrap();
    assert!(matches!(
        m.runtime.gate_deploy(
            &owner,
            &run,
            deploy("deploy-0", false),
            Some(&stranger),
            &mut tx
        ),
        Err(OrchestrationError::Denied(_))
    ));
    assert_eq!(
        m.runtime.run_gate_state(&owner, &run).unwrap().deployment,
        None
    );
    // A non-production deployment the policy does not gate is authorized at once. No
    // adapter runs deployments here, and the gate says so instead of pretending.
    let beta = request(&m, deploy("deploy-1", false), &mut tx).unwrap();
    assert!(beta.created && beta.approval_id.is_none());
    let scope = beta.gate.deployment.clone().unwrap();
    assert_eq!(
        (
            beta.gate.state,
            beta.gate.reason.as_deref(),
            beta.gate.commit.as_str(),
            scope.requires_gate_id.as_str(),
            scope.script.as_str(),
        ),
        (
            GateState::Unknown,
            Some("deployment_executor_unavailable"),
            a.as_str(),
            ci_gate.as_str(),
            "scripts/deploy.sh"
        )
    );
    assert_eq!(
        m.last_audit(),
        ("deployments_run".into(), AuditOutcome::Succeeded, None)
    );
    // The same operation again is the same gate; other content under it conflicts.
    let repeat = request(&m, deploy("deploy-1", false), &mut tx).unwrap();
    assert!(!repeat.created);
    assert_eq!(repeat.gate, beta.gate);
    let mut other = deploy("deploy-1", false);
    other.script = "scripts/other.sh".into();
    assert!(matches!(
        request(&m, other, &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    assert!(approvals(&m, &mut tx).is_empty());
    // Production always waits for a person, with no policy file at all.
    let waiting = request(&m, deploy("deploy-prod", true), &mut tx).unwrap();
    assert_eq!(waiting.gate.state, GateState::AwaitingApproval);
    let approval_id = waiting.approval_id.clone().unwrap();
    assert_eq!(
        m.last_audit(),
        (
            "deployments_run".into(),
            AuditOutcome::Denied,
            Some("deployment".into())
        )
    );
    // Asking again files nothing new and starts nothing.
    let still = request(&m, deploy("deploy-prod", true), &mut tx).unwrap();
    assert!(!still.created);
    assert_eq!(still.approval_id.as_deref(), Some(approval_id.as_str()));
    let filed = approvals(&m, &mut tx);
    assert_eq!(filed.len(), 1);
    assert_eq!(
        (
            filed[0].gate,
            filed[0].state,
            filed[0].operation_id.as_str()
        ),
        (
            ApprovalGate::Deployment,
            ApprovalState::Pending,
            "deploy-prod"
        )
    );
    let summary = m.runtime.run_gate_state(&owner, &run).unwrap();
    // Beta is unknown and production waits: the rollup shows the less certain one.
    assert_eq!(
        (summary.ci, summary.deployment),
        (GateState::Passed, Some(GateState::Unknown))
    );
    // The governed session cannot answer for itself; a person can.
    assert!(matches!(
        m.runtime
            .decide_approval(&owner, &approval_id, true, Some(&worker()), &mut tx),
        Err(OrchestrationError::Forbidden(_))
    ));
    m.runtime
        .decide_approval(&owner, &approval_id, true, Some(&human()), &mut tx)
        .unwrap();
    // The approval alone moves nothing; the repeated request consumes it.
    assert_eq!(
        m.runtime.gate(&owner, &waiting.gate.gate_id).unwrap().state,
        GateState::AwaitingApproval
    );
    let approved = request(&m, deploy("deploy-prod", true), &mut tx).unwrap();
    assert_eq!(
        (approved.gate.state, approved.gate.reason.as_deref()),
        (GateState::Unknown, Some("deployment_executor_unavailable"))
    );
    assert_eq!(approvals(&m, &mut tx)[0].state, ApprovalState::Consumed);
    // Answered: a repeat asks nobody again.
    request(&m, deploy("deploy-prod", true), &mut tx).unwrap();
    assert_eq!(approvals(&m, &mut tx).len(), 1);
    // A denied production request fails with the reason, and stays failed.
    let refused = request(&m, deploy("deploy-prod-2", true), &mut tx).unwrap();
    // The new request in the production slot superseded the one that never ran.
    assert_eq!(
        m.runtime.gate(&owner, &waiting.gate.gate_id).unwrap().state,
        GateState::Superseded
    );
    m.runtime
        .decide_approval(
            &owner,
            &refused.approval_id.unwrap(),
            false,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    for _ in 0..2 {
        let denied = request(&m, deploy("deploy-prod-2", true), &mut tx).unwrap();
        assert_eq!(
            (denied.gate.state, denied.gate.reason.as_deref()),
            (GateState::Failed, Some("approval_denied"))
        );
        assert!(denied.approval_id.is_none());
    }
    assert_eq!(approvals(&m, &mut tx).len(), 2);
    // The run moves to commit b while a production request waits: the request is
    // withdrawn with the CI gate it rested on, and repeating it asks nobody.
    let stale = request(&m, deploy("deploy-prod-3", true), &mut tx).unwrap();
    assert_eq!(stale.gate.state, GateState::AwaitingApproval);
    let moved = m
        .runtime
        .gate_link(&owner, &run, link(&b, CI), Some(&worker()), &mut tx)
        .unwrap();
    assert!(moved.superseded.contains(&ci_gate));
    assert!(moved.superseded.contains(&stale.gate.gate_id));
    let withdrawn = request(&m, deploy("deploy-prod-3", true), &mut tx).unwrap();
    assert_eq!(
        (withdrawn.gate.state, withdrawn.gate.reason.as_deref()),
        (GateState::Superseded, Some("required_gate_superseded"))
    );
    // And a new request waits for the new commit's verdict.
    assert_eq!(
        gate_error(request(&m, deploy("deploy-prod-4", true), &mut tx)),
        GateError::NotPassed
    );
    // Both deployments that never ran were withdrawn, each reported once: the waiting
    // production request and the authorized beta one nothing could run.
    let withdrawn = gate_events(&m, &run)
        .into_iter()
        .filter(|event| event.2.as_deref() == Some("required_gate_superseded"))
        .count();
    assert_eq!(withdrawn, 2);
    assert_eq!(
        m.runtime.gate(&owner, &beta.gate.gate_id).unwrap().state,
        GateState::Superseded
    );
}

#[test]
fn hs2_5jbypv_a_deployment_adapter_supplies_the_only_result_and_the_policy_can_ask_first() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, run) = with_run(
        &[TargetAction::GatesObserve, TargetAction::DeploymentsRun],
        &mut tx,
    );
    let ci = Scripted::new(CI, GateKind::Ci);
    let executor = Scripted::new(TARGET_SCRIPT_PROVIDER, GateKind::Deployment);
    m.runtime = m
        .runtime
        .clone()
        .with_gate_provider(ci.clone())
        .with_gate_provider(executor.clone());
    let (a, owner) = (sha('a'), m.owner.id.clone());
    // The policy asks a person before any deployment, production or not.
    let actions = vec![
        TargetAction::TicketsRead,
        TargetAction::WorkStart,
        TargetAction::GatesObserve,
        TargetAction::DeploymentsRun,
    ];
    m.runtime
        .set_policy(
            &owner,
            SetPolicy {
                expected_revision: None,
                policy: PolicyBody {
                    targets: vec![TargetPolicy {
                        target_id: id(1),
                        enabled: true,
                        actions,
                        tools: Vec::new(),
                        max_sessions: 1,
                        machine: None,
                        speculation: None,
                    }],
                    approval: ApprovalPolicy {
                        required_for: vec![ApprovalGate::Deployment],
                    },
                    ..PolicyBody::default()
                },
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    m.runtime
        .gate_link(&owner, &run, link(&a, CI), Some(&worker()), &mut tx)
        .unwrap();
    ci.set(&a, verdict(CI, GateState::Passed, 2, &a, Coverage::Exact));
    let sync = |m: &Machine, tx: &mut Transaction<'_>| {
        m.runtime
            .gate_sync(&owner, &run, &SyncGates::default(), Some(&worker()), tx)
            .unwrap()
    };
    sync(&m, &mut tx);
    let request = |m: &Machine, tx: &mut Transaction<'_>| {
        m.runtime
            .gate_deploy(&owner, &run, deploy("deploy-1", false), Some(&worker()), tx)
            .unwrap()
    };
    let waiting = request(&m, &mut tx);
    assert_eq!(waiting.gate.state, GateState::AwaitingApproval);
    assert!(executor.starts().is_empty());
    // A provider reading cannot move a gate that waits for a person.
    executor.set(
        &a,
        verdict(
            TARGET_SCRIPT_PROVIDER,
            GateState::Passed,
            1,
            &a,
            Coverage::Exact,
        ),
    );
    assert!(sync(&m, &mut tx).changed.is_empty());
    executor.readings.lock().unwrap().clear();
    m.runtime
        .decide_approval(
            &owner,
            &waiting.approval_id.unwrap(),
            true,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    // Approved: the adapter is started exactly once, however often the request repeats.
    let running = request(&m, &mut tx);
    assert_eq!(running.gate.state, GateState::Pending);
    request(&m, &mut tx);
    assert_eq!(executor.starts(), [(a.clone(), false)]);
    assert_eq!(
        m.runtime.run_gate_state(&owner, &run).unwrap().deployment,
        Some(GateState::Pending)
    );
    // A second deployment to the environment waits for the one in progress.
    assert_eq!(
        gate_error(m.runtime.gate_deploy(
            &owner,
            &run,
            deploy("deploy-2", false),
            Some(&worker()),
            &mut tx
        )),
        GateError::DeploymentActive
    );
    // A result for another commit, or one that only contains it, is not a result.
    let b = sha('b');
    for (commit, coverage) in [(&b, Coverage::Exact), (&b, Coverage::Ancestor)] {
        executor.set(
            &a,
            verdict(
                TARGET_SCRIPT_PROVIDER,
                GateState::Passed,
                1,
                commit,
                coverage,
            ),
        );
        sync(&m, &mut tx);
        let gate = m.runtime.gate(&owner, &running.gate.gate_id).unwrap();
        assert_eq!(
            (gate.state, gate.reason.as_deref()),
            (GateState::Unknown, Some("evidence_not_covering"))
        );
    }
    // The adapter's typed result for the gated commit settles it.
    executor.set(
        &a,
        verdict(
            TARGET_SCRIPT_PROVIDER,
            GateState::Passed,
            2,
            &a,
            Coverage::Exact,
        ),
    );
    assert_eq!(
        sync(&m, &mut tx).changed,
        *std::slice::from_ref(&running.gate.gate_id)
    );
    let done = m.runtime.run_gate_state(&owner, &run).unwrap();
    assert_eq!(
        (done.ci, done.deployment),
        (GateState::Passed, Some(GateState::Passed))
    );
    let evidence = done
        .gates
        .iter()
        .find(|gate| gate.kind == GateKind::Deployment)
        .and_then(|gate| gate.evidence.clone())
        .unwrap();
    assert_eq!(
        (evidence.exit_code, evidence.commit.as_str()),
        (Some(0), a.as_str())
    );
    let last = gate_events(&m, &run).pop().unwrap();
    assert_eq!(
        (last.0, last.1),
        (GateState::Passed, RunEventSource::Target)
    );
}

/// Finish the watch Job of a gate the way the Job monitor would.
pub(super) fn judge(home: &Path, owner: &str, job_id: &str, gate: &str, head: &str, run: u64) {
    let store = JobStore::at(home);
    let (_, token) = store
        .acquire(
            owner,
            job_id,
            Acquire {
                owner: "monitor:test".into(),
                status: ActiveStatus::Running,
                launch_id: None,
                external: None,
            },
        )
        .unwrap();
    let refs = [
        ("gate", gate.to_owned()),
        ("run_id", run.to_string()),
        ("run_attempt", "1".to_owned()),
        ("run_head_sha", head.to_owned()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    store
        .finish(
            owner,
            job_id,
            &token,
            if gate == "green" {
                FinalStatus::Succeeded
            } else {
                FinalStatus::Failed
            },
            Outcome {
                summary: None,
                refs,
            },
        )
        .unwrap();
}

#[test]
fn hs2_5jbypv_the_github_actions_adapter_reads_the_watch_job_and_wakes_on_its_change() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (m, run) = with_run(&[TargetAction::GatesObserve], &mut tx);
    let (a, b, c, owner) = (sha('a'), sha('b'), sha('c'), m.owner.id.clone());
    let github = |commit: &str| -> LinkGate {
        serde_json::from_value(json!({"commit": commit, "repository": "example/widgets"})).unwrap()
    };
    let sync = |wait_ms, tx: &mut Transaction<'_>| {
        m.runtime
            .gate_sync(&owner, &run, &SyncGates { wait_ms }, Some(&worker()), tx)
            .unwrap()
    };
    // Linking creates one watch Job, tied to the orchestration ticket of the run.
    let linked = m
        .runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    let gate = linked.gate.clone();
    assert_eq!(
        (gate.provider.as_str(), gate.state, linked.watching),
        ("github-actions", GateState::Pending, Some(false))
    );
    let scope = gate.ci.clone().unwrap();
    assert_eq!(
        (scope.workflow.as_str(), scope.branch.as_str()),
        ("CI", "main")
    );
    let store = JobStore::at(&m.home);
    let jobs = store.list(&owner).unwrap().jobs;
    assert_eq!(jobs.len(), 1);
    assert_eq!(gate.watch_ref.as_deref(), Some(jobs[0].id.as_str()));
    assert!(jobs[0].ticket.is_some());
    m.runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(store.list(&owner).unwrap().jobs.len(), 1);
    // The monitor judges the run red while a sync waits: the wait ends on that change.
    let (home, waiter, job, commit) =
        (m.home.clone(), owner.clone(), jobs[0].id.clone(), a.clone());
    let judged = std::thread::spawn(move || judge(&home, &waiter, &job, "red", &commit, 41));
    let started = std::time::Instant::now();
    let mut red = sync(15_000, &mut tx);
    if red.changed.is_empty() {
        // The verdict landed before the first read's cursor; the next read has it.
        red = sync(0, &mut tx);
    }
    judged.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(red.summary.ci, GateState::Failed);
    let evidence = red.summary.gates[0].evidence.clone().unwrap();
    assert_eq!(
        (
            evidence.reference.as_str(),
            evidence.coverage,
            evidence.commit.as_str()
        ),
        ("41", Coverage::Exact, a.as_str())
    );
    // The same Job read again is not news, and its evidence keeps its first time.
    assert!(sync(0, &mut tx).changed.is_empty());
    // Linking again re-watches: a second Job that ignores the judged run.
    let again = m
        .runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(again.gate.state, GateState::Pending);
    let jobs = store.list(&owner).unwrap().jobs;
    assert_eq!(jobs.len(), 2);
    assert_ne!(again.gate.watch_ref, gate.watch_ref);
    // Green on a descendant the watch source accepted: passed, as an ancestor.
    judge(
        &m.home,
        &owner,
        again.gate.watch_ref.as_deref().unwrap(),
        "green",
        &b.to_uppercase(),
        42,
    );
    let green = sync(0, &mut tx);
    assert_eq!(green.summary.ci, GateState::Passed);
    let evidence = green.summary.gates[0].evidence.clone().unwrap();
    assert_eq!(
        (evidence.coverage, evidence.commit.as_str(), evidence.order),
        (Coverage::Ancestor, b.as_str(), 42)
    );
    // The branch moved off another commit: superseded by the provider's own account.
    let dropped = m
        .runtime
        .gate_link(&owner, &run, github(&c), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(dropped.superseded, *std::slice::from_ref(&gate.gate_id));
    judge(
        &m.home,
        &owner,
        dropped.gate.watch_ref.as_deref().unwrap(),
        "superseded",
        &c,
        43,
    );
    sync(0, &mut tx);
    let gone = m.runtime.gate(&owner, &dropped.gate.gate_id).unwrap();
    assert_eq!(
        (gone.state, gone.reason.as_deref()),
        (GateState::Superseded, Some("branch_moved"))
    );
    let summary = m.runtime.run_gate_state(&owner, &run).unwrap();
    assert_eq!(
        (summary.ci, summary.ci_reason.as_deref(), summary.superseded),
        (GateState::Unknown, Some("no_gate"), 2)
    );
    // A repository name the provider cannot address is refused by the adapter.
    let odd: LinkGate =
        serde_json::from_value(json!({"commit": a, "repository": "not a repository"})).unwrap();
    assert_eq!(
        gate_error(
            m.runtime
                .gate_link(&owner, &run, odd, Some(&worker()), &mut tx)
        ),
        GateError::ProviderUnsupported
    );
}
