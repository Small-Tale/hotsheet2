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

/// What a run with one gate amounts to, then how `assess` reports it.
fn finding_of(observed: &Observed<ExternalWork>) -> (Verification, Option<String>, bool) {
    let run = Run {
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
    };
    let observations = Observations {
        ci: observed.clone(),
        ..Observations::target_only(Observed::Verified(TargetFacts {
            exists: true,
            status: "started".into(),
            claimed_by: Some("w".into()),
            lease_expires_at_ms: Some(5_000),
        }))
    };
    let assessment = assess(&run, &observations, 1_000);
    let found = assessment.finding(ExternalSource::CiRun).unwrap().clone();
    (
        found.verification,
        found.reason,
        assessment.may_start(ExternalSource::CiRun),
    )
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
