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
            deploy("deploy-prod", true),
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
    m.runtime
        .decide_approval(
            &owner,
            &waiting.approval_id.unwrap(),
            true,
            Some(&super::tickets::human()),
            &mut tx,
        )
        .unwrap();
    let approved = m
        .runtime
        .gate_deploy(
            &owner,
            &run,
            deploy("deploy-prod", true),
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
