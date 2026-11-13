//! HS2-7QNK7Y and siblings: the per-source observations reconciliation builds from
//! this machine's records. Every recorded state is crossed with every adapter answer,
//! then gates are combined in repeated, interleaved and empty-then-refilled sequences.
//! Synthetic ids only.

use super::*;

fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}

fn ci_gate(n: u128, state: GateState, reason: Option<&str>, watch: Option<&str>) -> Gate {
    Gate {
        gate_id: id(n),
        owner_checkout_id: "owner-0123456789ab".into(),
        run_id: id(900),
        target_id: id(1),
        kind: GateKind::Ci,
        provider: "github-actions".into(),
        commit: "a".repeat(40),
        ci: Some(CiScope {
            repository: "example/widgets".into(),
            workflow: "CI".into(),
            branch: "main".into(),
        }),
        deployment: None,
        state,
        reason: reason.map(Into::into),
        evidence: None,
        watch_ref: watch.map(Into::into),
        superseded_by: None,
        linked_by: Some("worker-1".into()),
        created_at_ms: 1,
        updated_at_ms: 1,
        history: Vec::new(),
    }
}

fn reading(state: GateState, reason: Option<&str>, watch: Option<&str>) -> GateReading {
    GateReading {
        reason: reason.map(Into::into),
        watch_ref: watch.map(Into::into),
        ..GateReading::new(state)
    }
}

fn work(recorded: Option<&str>, live: Option<&str>, note: Option<&str>) -> Observed<ExternalWork> {
    Observed::Verified(ExternalWork {
        recorded: recorded.map(Into::into),
        live: live.map(Into::into),
        note: note.map(Into::into),
    })
}

/// What a run with one CI observation amounts to, then how `assess` reports it.
fn finding_of(observed: &Observed<ExternalWork>) -> (Verification, Option<String>, bool) {
    finding_of_source(observed, ExternalSource::CiRun)
}

#[test]
fn hs2_7qnk7y_every_recorded_ci_state_against_every_adapter_answer() {
    use GateState::{AwaitingApproval, Failed, Passed, Pending, Superseded, Unknown};
    use Verification::{Mismatched, Unverifiable, Verified};
    let pending = || reading(Pending, None, Some("job-1"));
    let rewatched = || reading(Pending, None, Some("job-2"));
    let green = || reading(Passed, None, Some("job-1"));
    let red = || reading(Failed, None, Some("job-1"));
    let moved = || reading(Superseded, Some("branch_moved"), Some("job-1"));
    let cancelled = || reading(Unknown, Some("watch_cancelled"), Some("job-1"));
    let lost = || reading(Unknown, Some("watch_lost"), Some("job-1"));
    let gone = || reading(Unknown, Some("not_watched"), None);
    let odd = || reading(Unknown, Some("evidence_missing"), Some("job-1"));
    let unavailable = || Err::<GateReading, String>("gate_provider_unavailable".into());
    let unsupported = || Err::<GateReading, String>("gate_provider_unsupported".into());
    type Row = (
        GateState,
        Option<&'static str>,
        Result<GateReading, String>,
        Verification,
        Option<&'static str>,
        bool,
    );
    // (recorded state, recorded reason, adapter answer, verification, reason, may start)
    let rows: Vec<Row> = vec![
        // A gate that waits on its provider.
        (Pending, None, Ok(pending()), Verified, None, false),
        (
            Pending,
            None,
            Ok(rewatched()),
            Verified,
            Some("watch_adopted"),
            false,
        ),
        (Pending, None, Ok(green()), Verified, None, true),
        (Pending, None, Ok(red()), Verified, None, true),
        (Pending, None, Ok(moved()), Verified, None, true),
        (Pending, None, Ok(cancelled()), Verified, None, true),
        (
            Pending,
            None,
            Ok(lost()),
            Unverifiable,
            Some("watch_lost"),
            false,
        ),
        (
            Pending,
            None,
            Ok(gone()),
            Mismatched,
            Some("recorded_work_missing"),
            true,
        ),
        (
            Pending,
            None,
            Ok(odd()),
            Unverifiable,
            Some("evidence_missing"),
            false,
        ),
        (
            Pending,
            None,
            unavailable(),
            Unverifiable,
            Some("gate_provider_unavailable"),
            false,
        ),
        (
            Pending,
            None,
            unsupported(),
            Unverifiable,
            Some("gate_provider_unsupported"),
            false,
        ),
        // A lost watch is still waited on: the monitor reattaches it.
        (
            Unknown,
            Some("watch_lost"),
            Ok(pending()),
            Verified,
            None,
            false,
        ),
        (
            Unknown,
            Some("watch_lost"),
            Ok(lost()),
            Unverifiable,
            Some("watch_lost"),
            false,
        ),
        (
            Unknown,
            Some("watch_lost"),
            Ok(gone()),
            Mismatched,
            Some("recorded_work_missing"),
            true,
        ),
        // Never watched: nothing in flight, unless the adapter shows a watch, which is
        // adopted.
        (
            Unknown,
            Some("not_watched"),
            Ok(gone()),
            Verified,
            None,
            true,
        ),
        (
            Unknown,
            Some("not_watched"),
            Ok(pending()),
            Verified,
            Some("watch_adopted"),
            false,
        ),
        (
            Unknown,
            Some("not_watched"),
            Ok(lost()),
            Verified,
            None,
            true,
        ),
        (
            Unknown,
            Some("not_watched"),
            unavailable(),
            Unverifiable,
            Some("gate_provider_unavailable"),
            false,
        ),
        (
            Unknown,
            Some("watch_cancelled"),
            Ok(cancelled()),
            Verified,
            None,
            true,
        ),
        // A failed gate ended its watch; a later watch of it is adopted.
        (Failed, None, Ok(red()), Verified, None, true),
        (
            Failed,
            None,
            Ok(rewatched()),
            Verified,
            Some("watch_adopted"),
            false,
        ),
        (Failed, None, Ok(gone()), Verified, None, true),
        // Final: the adapter is not even consulted.
        (Passed, None, Ok(gone()), Verified, None, true),
        (Passed, None, unavailable(), Verified, None, true),
        (
            Superseded,
            Some("newer_gate_linked"),
            unavailable(),
            Verified,
            Some("no_gate"),
            true,
        ),
        // A CI gate never awaits approval, but the classification is total.
        (AwaitingApproval, None, Ok(gone()), Verified, None, true),
    ];
    for (state, why, answer, verification, reason, may_start) in rows {
        let gate = ci_gate(1, state, why, Some("job-1"));
        let observed = ci_work(&[(gate, answer.clone())]);
        assert_eq!(
            finding_of(&observed),
            (verification, reason.map(Into::into), may_start),
            "{state:?} {why:?} {answer:?}"
        );
    }
}

#[test]
fn hs2_7qnk7y_gates_combine_unverifiable_first_then_missing_then_live() {
    use GateState::{Failed, Passed, Pending, Unknown};
    let a = ci_gate(1, Pending, None, Some("job-a"));
    let b = ci_gate(2, Pending, None, Some("job-b"));
    let c = ci_gate(3, Passed, None, Some("job-c"));
    let live = |watch: &str| Ok(reading(Pending, None, Some(watch)));
    let gone = || Ok(reading(Unknown, Some("not_watched"), None));
    let lost = || Ok(reading(Unknown, Some("watch_lost"), Some("job-x")));
    // Empty, then refilled, then empty again: no gate is verified as `no_gate`.
    assert_eq!(ci_work(&[]), work(None, None, Some("no_gate")));
    assert_eq!(
        ci_work(&[(a.clone(), live("job-a"))]),
        work(Some("job-a"), Some("job-a"), None)
    );
    assert_eq!(ci_work(&[]), work(None, None, Some("no_gate")));
    // A deployment gate is not a CI run.
    let deployment = Gate {
        kind: GateKind::Deployment,
        ..ci_gate(9, Pending, None, None)
    };
    assert_eq!(
        ci_work(&[(deployment, gone())]),
        work(None, None, Some("no_gate"))
    );
    // Settled gates contribute nothing in flight and no note.
    assert_eq!(ci_work(&[(c.clone(), gone())]), work(None, None, None));
    // In either order, one lost watch makes the whole source unverifiable, and a
    // missing watch outranks a live one.
    for gates in [
        vec![(a.clone(), live("job-a")), (b.clone(), lost())],
        vec![(b.clone(), lost()), (a.clone(), live("job-a"))],
    ] {
        assert_eq!(ci_work(&gates), Observed::Unverifiable("watch_lost".into()));
    }
    for gates in [
        vec![(a.clone(), live("job-a")), (b.clone(), gone())],
        vec![(b.clone(), gone()), (a.clone(), live("job-a"))],
    ] {
        assert_eq!(ci_work(&gates), work(Some("job-b"), None, None));
    }
    // The same gate read twice is the same answer: no state is kept between calls.
    let twice = [(a.clone(), live("job-a")), (a.clone(), live("job-a"))];
    assert_eq!(ci_work(&twice), ci_work(&twice));
    assert_eq!(ci_work(&twice), work(Some("job-a"), Some("job-a"), None));
    // A failed gate next to a passed one: nothing in flight.
    let failed = ci_gate(4, Failed, None, Some("job-d"));
    assert_eq!(
        ci_work(&[
            (failed, Ok(reading(Failed, None, Some("job-d")))),
            (c, gone())
        ]),
        work(None, None, None)
    );
    // A recorded watch without a Job id is named by its gate.
    let unnamed = ci_gate(5, Pending, None, None);
    assert_eq!(
        ci_work(&[(unnamed.clone(), gone())]),
        work(Some(&unnamed.gate_id), None, None)
    );
}

fn deployment_gate(n: u128, state: GateState, reason: Option<&str>) -> Gate {
    Gate {
        kind: GateKind::Deployment,
        provider: "target-script".into(),
        ci: None,
        deployment: Some(DeploymentScope {
            environment: "beta".into(),
            script: "scripts/deploy.sh".into(),
            production: false,
            operation_id: format!("deploy-{n}"),
            requires_gate_id: id(1),
            artifact: None,
        }),
        ..ci_gate(n, state, reason, None)
    }
}

fn deployment_finding(observed: &Observed<ExternalWork>) -> (Verification, Option<String>, bool) {
    finding_of_source(observed, ExternalSource::Deployment)
}

/// Like `finding_of`, for any of the work sources.
fn finding_of_source(
    observed: &Observed<ExternalWork>,
    source: ExternalSource,
) -> (Verification, Option<String>, bool) {
    let mut observations = Observations::target_only(Observed::Verified(TargetFacts {
        exists: true,
        status: "started".into(),
        claimed_by: Some("w".into()),
        lease_expires_at_ms: Some(5_000),
    }));
    match source {
        ExternalSource::CiRun => observations.ci = observed.clone(),
        ExternalSource::Deployment => observations.deployment = observed.clone(),
        ExternalSource::Session => observations.session = observed.clone(),
        _ => panic!("not a work source"),
    }
    let run = test_run();
    let assessment = assess(&run, &observations, 1_000);
    let found = assessment.finding(source).unwrap().clone();
    (
        found.verification,
        found.reason,
        assessment.may_start(source),
    )
}

fn test_run() -> Run {
    Run {
        run_id: id(900),
        owner_checkout_id: "owner-0123456789ab".into(),
        operation_id: "op".into(),
        ticket: RunTicket {
            connection_id: "c".into(),
            native_id: "n".into(),
        },
        target: RunTarget {
            target_id: id(1),
            connection_id: "c".into(),
            native_id: "t".into(),
        },
        mode: DispatchMode::Direct,
        worker_id: Some("w".into()),
        state: RunState::Dispatched,
        lease_expires_at_ms: Some(5_000),
        cancel_requested: false,
        observed: None,
        unreachable: None,
        paused: None,
        reconciliation: None,
        created_at_ms: 1,
        updated_at_ms: 1,
        history: Vec::new(),
    }
}

#[test]
fn hs2_byeb60_every_recorded_deployment_state_against_every_executor_answer() {
    use GateState::{AwaitingApproval, Failed, Passed, Pending, Superseded, Unknown};
    use Verification::{Mismatched, Unverifiable, Verified};
    let running = || Ok(reading(Pending, None, Some("exec-1")));
    let deployed = || Ok(reading(Passed, None, Some("exec-1")));
    let broke = || Ok(reading(Failed, None, Some("exec-1")));
    let unknown = || Ok(reading(Unknown, Some(EXECUTION_NOT_FOUND), None));
    let silent = || Ok(reading(Unknown, None, None));
    let offline = || Ok(reading(Unknown, Some("executor_offline"), None));
    let none = || Err::<GateReading, String>("gate_provider_unsupported".into());
    let unreadable = || Err::<GateReading, String>("gate_provider_unavailable".into());
    type Row = (
        GateState,
        Option<&'static str>,
        Result<GateReading, String>,
        Verification,
        Option<&'static str>,
        bool,
    );
    let rows: Vec<Row> = vec![
        // Nothing executed: verified from the records whatever an executor says.
        (
            AwaitingApproval,
            None,
            none(),
            Verified,
            Some("awaiting_approval"),
            true,
        ),
        (
            AwaitingApproval,
            None,
            running(),
            Verified,
            Some("awaiting_approval"),
            true,
        ),
        (
            Unknown,
            Some("deployment_executor_unavailable"),
            none(),
            Verified,
            Some("deployment_not_executed"),
            true,
        ),
        (
            Unknown,
            Some("required_gate_superseded"),
            none(),
            Verified,
            None,
            true,
        ),
        (
            Failed,
            Some("approval_denied"),
            none(),
            Verified,
            None,
            true,
        ),
        (Passed, None, unreadable(), Verified, None, true),
        (
            Superseded,
            Some("newer_gate_linked"),
            none(),
            Verified,
            Some("no_deployment"),
            true,
        ),
        // Recorded as executing: only the executor can say.
        (Pending, None, running(), Verified, None, false),
        (Pending, None, deployed(), Verified, None, true),
        (Pending, None, broke(), Verified, None, true),
        (
            Pending,
            None,
            unknown(),
            Mismatched,
            Some("recorded_work_missing"),
            true,
        ),
        (
            Pending,
            None,
            silent(),
            Unverifiable,
            Some("deployment_state_unknown"),
            false,
        ),
        (
            Pending,
            None,
            offline(),
            Unverifiable,
            Some("executor_offline"),
            false,
        ),
        (
            Pending,
            None,
            none(),
            Unverifiable,
            Some("deployment_executor_unavailable"),
            false,
        ),
        (
            Pending,
            None,
            unreadable(),
            Unverifiable,
            Some("gate_provider_unavailable"),
            false,
        ),
        (
            Unknown,
            Some("deployment_starting"),
            running(),
            Verified,
            None,
            false,
        ),
        (
            Unknown,
            Some("deployment_starting"),
            unknown(),
            Mismatched,
            Some("recorded_work_missing"),
            true,
        ),
        (
            Unknown,
            Some("deployment_starting"),
            none(),
            Unverifiable,
            Some("deployment_executor_unavailable"),
            false,
        ),
    ];
    for (state, why, answer, verification, reason, may_start) in rows {
        let gate = deployment_gate(7, state, why);
        let observed = deployment_work(&[(gate, answer.clone())]);
        assert_eq!(
            deployment_finding(&observed),
            (verification, reason.map(Into::into), may_start),
            "{state:?} {why:?} {answer:?}"
        );
    }
    // A live execution is named by its retained operation id.
    assert_eq!(
        deployment_work(&[(deployment_gate(7, Pending, None), running())]),
        work(Some("deploy-7"), Some("deploy-7"), None)
    );
}

#[test]
fn hs2_byeb60_deployments_combine_and_ci_gates_are_not_deployments() {
    use GateState::{AwaitingApproval, Pending, Unknown};
    let none = || Err::<GateReading, String>("gate_provider_unsupported".into());
    let waiting = deployment_gate(1, AwaitingApproval, None);
    let approved = deployment_gate(2, Unknown, Some("deployment_executor_unavailable"));
    let executing = deployment_gate(3, Pending, None);
    // Empty, then refilled, then empty.
    assert_eq!(
        deployment_work(&[]),
        work(None, None, Some("no_deployment"))
    );
    assert_eq!(
        deployment_work(&[(waiting.clone(), none())]),
        work(None, None, Some("awaiting_approval"))
    );
    assert_eq!(
        deployment_work(&[]),
        work(None, None, Some("no_deployment"))
    );
    // A CI gate is not a deployment, and a deployment gate is not a CI run.
    let ci = ci_gate(4, Pending, None, Some("job-4"));
    assert_eq!(
        deployment_work(&[(ci.clone(), none())]),
        work(None, None, Some("no_deployment"))
    );
    assert_eq!(
        ci_work(&[(waiting.clone(), none())]),
        work(None, None, Some("no_gate"))
    );
    // The first settled note qualifies the answer, in either order.
    assert_eq!(
        deployment_work(&[(waiting.clone(), none()), (approved.clone(), none())]),
        work(None, None, Some("awaiting_approval"))
    );
    assert_eq!(
        deployment_work(&[(approved.clone(), none()), (waiting.clone(), none())]),
        work(None, None, Some("deployment_not_executed"))
    );
    // One execution nothing can confirm makes the whole source unverifiable.
    for gates in [
        vec![(approved.clone(), none()), (executing.clone(), none())],
        vec![(executing.clone(), none()), (approved.clone(), none())],
    ] {
        assert_eq!(
            deployment_work(&gates),
            Observed::Unverifiable("deployment_executor_unavailable".into())
        );
    }
}
