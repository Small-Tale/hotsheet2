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
