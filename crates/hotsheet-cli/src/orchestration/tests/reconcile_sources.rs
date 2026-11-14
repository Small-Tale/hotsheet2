//! HS2-7QNK7Y and its siblings: what reconciliation reports for the sources outside the
//! target ticket, through the shared runtime with the real GitHub Actions adapter over
//! a Job store. Synthetic repositories, commits and ids only.
use hotsheet_jobs::JobStore;
use hotsheet_ticketing::orchestration::{Disposition, ExternalSource, TargetAction, Verification};
use serde_json::json;

use super::gates::{judge, sha, with_run};
use super::recovery::{disposition, finding};
use super::tickets::worker;
use super::*;

fn github(commit: &str) -> LinkGate {
    serde_json::from_value(json!({"commit": commit, "repository": "example/widgets"})).unwrap()
}

/// HS2-G3VAK4: a deployment for a provider no adapter serves here, so it is approved
/// and never executed (the `target-script` provider now runs on the target's machine).
fn unrun(operation: &str, production: bool) -> RequestDeployment {
    let mut request = super::gates::deploy(operation, production);
    request.provider = Some("synthetic-deployer".into());
    request
}

#[test]
fn hs2_7qnk7y_reconciliation_reads_the_ci_runs_gating_a_run() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (m, run) = with_run(&[TargetAction::GatesObserve], &mut tx);
    let (a, b, owner) = (sha('a'), sha('b'), m.owner.id.clone());
    let reconcile = |tx: &mut Transaction<'_>| {
        m.runtime
            .run_reconcile(&owner, &run, Some(&worker()), tx)
            .unwrap()
    };
    let ci = |view: &RunView| finding(view, ExternalSource::CiRun);
    // No gate: verified, and the answer says why.
    let view = reconcile(&mut tx);
    assert_eq!(ci(&view), (Verification::Verified, Some("no_gate".into())));
    assert!(!view.unverified.contains(&"ci_run"));
    assert_eq!(disposition(&view), Disposition::Continue);
    // A linked gate whose watch Job is live: the same watch, verified.
    let linked = m
        .runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    let job = linked.gate.watch_ref.clone().unwrap();
    let view = reconcile(&mut tx);
    assert_eq!(ci(&view), (Verification::Verified, None));
    assert_eq!(disposition(&view), Disposition::Continue);
    // Reconciling the same world again is the same answer.
    assert_eq!(ci(&reconcile(&mut tx)), (Verification::Verified, None));
    // A live watch is the run's: linking again starts no second watch Job.
    m.runtime
        .gate_link(&owner, &run, github(&a), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(JobStore::at(&m.home).list(&owner).unwrap().jobs.len(), 1);
    // The CI run finishes red: the watch ended in the open, nothing is in flight.
    judge(&m.home, &owner, &job, "red", &a, 41);
    let view = reconcile(&mut tx);
    assert_eq!(ci(&view), (Verification::Verified, None));
    assert_eq!(disposition(&view), Disposition::Continue);
    // A new commit is linked and its watch Job disappears while nothing runs: the run
    // waits on a watch that is gone.
    let moved = m
        .runtime
        .gate_link(&owner, &run, github(&b), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        moved.gate.state,
        hotsheet_ticketing::orchestration::GateState::Pending
    );
    let jobs = JobStore::at(&m.home).checkout_dir(&owner).unwrap();
    std::fs::rename(&jobs, jobs.with_extension("moved")).unwrap();
    let view = reconcile(&mut tx);
    assert_eq!(
        ci(&view),
        (
            Verification::Mismatched,
            Some("recorded_work_missing".into())
        )
    );
    assert_eq!(disposition(&view), Disposition::NeedsAttention);
    // The Job store cannot be read at all: unverifiable, and the run is held.
    std::fs::write(&jobs, b"not a directory").unwrap();
    let view = reconcile(&mut tx);
    assert_eq!(
        ci(&view),
        (
            Verification::Unverifiable,
            Some("gate_provider_unavailable".into())
        )
    );
    assert!(view.unverified.contains(&"ci_run"));
    assert_eq!(disposition(&view), Disposition::HoldUnverified);
    // The store comes back with the watch: verified again, the run continues.
    std::fs::remove_file(&jobs).unwrap();
    std::fs::rename(jobs.with_extension("moved"), &jobs).unwrap();
    let view = reconcile(&mut tx);
    assert_eq!(ci(&view), (Verification::Verified, None));
    assert_eq!(disposition(&view), Disposition::Continue);
    // A gate whose adapter is not registered here cannot be verified.
    let plain = OrchestrationRuntime::clone(&m.runtime);
    let mut bare = plain;
    bare.gate_providers.clear();
    assert_eq!(
        bare.run_ci_work(&owner, &run),
        hotsheet_ticketing::orchestration::Observed::Unverifiable(
            "gate_provider_unsupported".into()
        )
    );
}

#[test]
fn hs2_byeb60_reconciliation_reads_deployments_by_their_operation_id() {
    use super::gates::{CI, Scripted, deploy, link, verdict};
    use hotsheet_ticketing::orchestration::{Coverage, GateKind, GateReading, GateState};
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
    let (a, owner) = (sha('a'), m.owner.id.clone());
    let reconcile = |m: &super::tickets::Machine, tx: &mut Transaction<'_>| {
        m.runtime
            .run_reconcile(&owner, &run, Some(&worker()), tx)
            .unwrap()
    };
    let deployment = |view: &RunView| finding(view, ExternalSource::Deployment);
    // Nothing requested.
    let view = reconcile(&m, &mut tx);
    assert_eq!(
        deployment(&view),
        (Verification::Verified, Some("no_deployment".into()))
    );
    m.runtime
        .gate_link(&owner, &run, link(&a, CI), Some(&worker()), &mut tx)
        .unwrap();
    ci.set(&a, verdict(CI, GateState::Passed, 2, &a, Coverage::Exact));
    m.runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    // Production waits for a person: nothing was executed, and the records say so.
    let waiting = m
        .runtime
        .gate_deploy(
            &owner,
            &run,
            unrun("deploy-prod", true),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(waiting.gate.state, GateState::AwaitingApproval);
    let view = reconcile(&m, &mut tx);
    assert_eq!(
        deployment(&view),
        (Verification::Verified, Some("awaiting_approval".into()))
    );
    // The shipped case: approved, and no executor exists to run it. Verified means
    // exactly that this machine's records show nothing was executed.
    answer(
        &m.runtime,
        &owner,
        &waiting.request_id.unwrap(),
        hotsheet_ticketing::orchestration::PermissionAnswer::AllowOnce,
        &mut tx,
    );
    let approved = m
        .runtime
        .gate_deploy(
            &owner,
            &run,
            unrun("deploy-prod", true),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        approved.gate.reason.as_deref(),
        Some("deployment_executor_unavailable")
    );
    let view = reconcile(&m, &mut tx);
    assert_eq!(
        deployment(&view),
        (
            Verification::Verified,
            Some("deployment_not_executed".into())
        )
    );
    assert_eq!(disposition(&view), Disposition::Continue);
    // With an executor registered, a beta deployment starts once and stays live.
    let executor = Scripted::new(TARGET_SCRIPT_PROVIDER, GateKind::Deployment);
    m.runtime = m.runtime.clone().with_gate_provider(executor.clone());
    let started = m
        .runtime
        .gate_deploy(
            &owner,
            &run,
            deploy("deploy-beta", false),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(started.gate.state, GateState::Pending);
    let view = reconcile(&m, &mut tx);
    assert_eq!(deployment(&view), (Verification::Verified, None));
    // A retry after the interruption, with the same operation id, starts nothing.
    m.runtime
        .gate_deploy(
            &owner,
            &run,
            deploy("deploy-beta", false),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(executor.starts().len(), 1);
    // The executor has no such execution: the recorded deployment is missing.
    executor.set(
        &a,
        GateReading::new(GateState::Unknown).reason("execution_not_found"),
    );
    let view = reconcile(&m, &mut tx);
    assert_eq!(
        deployment(&view),
        (
            Verification::Mismatched,
            Some("recorded_work_missing".into())
        )
    );
    assert_eq!(disposition(&view), Disposition::NeedsAttention);
    // The executor cannot answer: the run is held with the executor's code.
    executor.readings.lock().unwrap().clear();
    let view = reconcile(&m, &mut tx);
    assert_eq!(
        deployment(&view),
        (
            Verification::Unverifiable,
            Some("gate_provider_unavailable".into())
        )
    );
    assert_eq!(disposition(&view), Disposition::HoldUnverified);
    // The executor goes away: an execution nothing can confirm.
    let mut bare = OrchestrationRuntime::clone(&m.runtime);
    bare.gate_providers
        .retain(|provider| provider.kind() == GateKind::Ci);
    assert_eq!(
        bare.run_deployment_work(&owner, &run),
        hotsheet_ticketing::orchestration::Observed::Unverifiable(
            "deployment_executor_unavailable".into()
        )
    );
    // The execution finishes: nothing is in flight any more.
    executor.set(
        &a,
        verdict(
            TARGET_SCRIPT_PROVIDER,
            GateState::Passed,
            3,
            &a,
            Coverage::Exact,
        ),
    );
    let view = reconcile(&m, &mut tx);
    assert_eq!(
        deployment(&view),
        (
            Verification::Verified,
            Some("deployment_not_executed".into())
        )
    );
}

/// HS2-G5H9FQ: real hosted executions in the target checkout, and attempts whose host
/// lost or never confirmed them. Unix only, like the validation runtime tests.
#[cfg(unix)]
#[test]
fn hs2_g5h9fq_reconciliation_reads_the_validation_attempts_of_a_run() {
    use hotsheet_ticketing::orchestration::{
        NewResult, ResultUpdate, ValidationRegistry, ValidationState,
    };
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (m, run) = with_run(&[TargetAction::ValidationRun], &mut tx);
    let owner = m.owner.id.clone();
    let key = m.runtime.run(&owner, &run).unwrap().ticket;
    // The target project's own profiles: one that takes a moment, one that passes.
    let directory = std::path::Path::new(&m.target.root).join(".hotsheet2");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("validation-profiles.json"),
        json!({"schema_version": 1, "profiles": [
            {"name": "slow", "program": "/bin/sh", "args": ["-c", "sleep 1; printf ok"]},
        ]})
        .to_string(),
    )
    .unwrap();
    for step in ["slow", "lost", "unconfirmed"] {
        m.runtime
            .validation_define(
                &owner,
                serde_json::from_value(json!({"ticket": {"native_id": key.native_id},
                    "step": {"step_id": step, "target_id": id(1),
                        "check": {"kind": "profile", "name": "slow"}, "timeout_seconds": 20}}))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    }
    let reconcile = |tx: &mut Transaction<'_>| {
        m.runtime
            .run_reconcile(&owner, &run, Some(&worker()), tx)
            .unwrap()
    };
    let validation = |view: &RunView| finding(view, ExternalSource::Validation);
    // No attempt names the run.
    let view = reconcile(&mut tx);
    assert_eq!(
        validation(&view),
        (Verification::Verified, Some("no_validation".into()))
    );
    // A real attempt for the run, still running on its host: the same execution.
    let started = m
        .runtime
        .validation_run(
            &owner,
            serde_json::from_value(json!({"ticket": {"native_id": key.native_id},
                "step_id": "slow", "operation_id": "validate-slow", "wait_ms": 0,
                "run_id": run}))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(started.result.state, ValidationState::Running);
    let view = reconcile(&mut tx);
    assert_eq!(validation(&view), (Verification::Verified, None));
    assert_eq!(disposition(&view), Disposition::Continue);
    // It ends on its host; the next reconciliation records the outcome once, as a
    // continuing request would, and never runs it again.
    m.runtime
        .hosted_validation(&m.target.id, &started.result.result_id, 8_000)
        .unwrap();
    let view = reconcile(&mut tx);
    assert_eq!(validation(&view), (Verification::Verified, None));
    let registry = ValidationRegistry::new(&m.home);
    let evidence = registry.evidence(&owner, &key, "slow", 200).unwrap();
    assert_eq!(evidence.latest.unwrap().state, ValidationState::Passed);
    assert_eq!(evidence.history.len(), 1);
    // An attempt the host confirmed and no longer knows, as after a host restart: it
    // is recorded `unverified` with `execution_lost`, and the record now says so.
    let begin = |step: &str, operation: &str, tx: &mut Transaction<'_>| {
        registry
            .begin(
                &owner,
                &key,
                step,
                NewResult {
                    operation_id: operation.into(),
                    state: ValidationState::Running,
                    reason: None,
                    actor_id: Some("worker-1".into()),
                    run_id: Some(run.clone()),
                },
                tx,
            )
            .unwrap()
            .0
    };
    let lost = begin("lost", "validate-lost", &mut tx);
    registry
        .finish(
            &owner,
            &key,
            "lost",
            &lost.result_id,
            ResultUpdate {
                host_started: Some(true),
                ..ResultUpdate::default()
            },
            &mut tx,
        )
        .unwrap();
    for _ in 0..2 {
        let view = reconcile(&mut tx);
        assert_eq!(
            validation(&view),
            (Verification::Verified, Some("execution_lost".into()))
        );
        assert_eq!(disposition(&view), Disposition::Continue);
    }
    let recorded = registry.evidence(&owner, &key, "lost", 200).unwrap();
    let latest = recorded.latest.unwrap();
    assert_eq!(
        (latest.state, latest.reason.as_deref()),
        (ValidationState::Unverified, Some("execution_lost"))
    );
    assert_eq!(recorded.history.len(), 1, "never run a second time");
    // An attempt its host never confirmed: nothing can be concluded, and the run is
    // held until the same request repeats it or its deadline passes.
    let unconfirmed = begin("unconfirmed", "validate-unconfirmed", &mut tx);
    let view = reconcile(&mut tx);
    assert_eq!(
        validation(&view),
        (
            Verification::Unverifiable,
            Some("execution_unconfirmed".into())
        )
    );
    assert_eq!(disposition(&view), Disposition::HoldUnverified);
    // The target is unreachable: the attempt stays running and the reason is named.
    let rebind = super::recovery::lose_target(&m, &mut tx);
    let view = reconcile(&mut tx);
    let (verification, reason) = validation(&view);
    assert_eq!(verification, Verification::Unverifiable);
    assert_ne!(reason.as_deref(), Some("execution_unconfirmed"));
    assert_eq!(
        registry
            .evidence(&owner, &key, "unconfirmed", 200)
            .unwrap()
            .latest
            .unwrap()
            .state,
        ValidationState::Running
    );
    m.runtime.bind(rebind, &mut tx).unwrap();
    // Past its deadline and still unknown to its host: recorded `deadline_passed`. The
    // host is still asked; a remembered rule never expires.
    let deadline = unconfirmed.deadline_ms.unwrap();
    let late = super::job_monitor::At(std::sync::atomic::AtomicU64::new(deadline));
    let mut later = Transaction {
        clock: &late,
        rng: &mut Rng(500),
    };
    let lapsed = m
        .runtime
        .run_validation_work(&owner, &run, Some(&worker()), &mut later);
    assert_eq!(
        lapsed,
        hotsheet_ticketing::orchestration::Observed::Verified(
            hotsheet_ticketing::orchestration::ExternalWork {
                recorded: None,
                live: None,
                note: Some("execution_lost".into()),
            }
        ),
        "the first settled note, from the earlier lost attempt, qualifies the answer"
    );
    let latest = registry
        .evidence(&owner, &key, "unconfirmed", 200)
        .unwrap()
        .latest
        .unwrap();
    assert_eq!(
        (latest.state, latest.reason.as_deref()),
        (ValidationState::Unverified, Some("deadline_passed"))
    );
}
