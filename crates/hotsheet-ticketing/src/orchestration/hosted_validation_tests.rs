//! HS2-7BTV98: the hosting machine's record of accepted validation executions.
use super::*;

const CHECKOUT: &str = "target-1";

fn outcome(end: ExecutionEnd, exit_code: Option<i32>) -> ExecutionOutcome {
    ExecutionOutcome {
        end,
        exit_code,
        signal: None,
        started_at_ms: 5,
        duration_ms: 7,
        stdout: None,
        stderr: None,
    }
}

fn id(n: u128) -> String {
    ulid::Ulid::from(n).to_string()
}

/// Transition matrix: (what is recorded, the call) -> (its answer, what is recorded).
#[test]
fn hs2_7btv98_accept_finish_and_cancel_transitions() {
    let passed = outcome(ExecutionEnd::Exited, Some(0));
    let cancelled = outcome(ExecutionEnd::Cancelled, None);
    enum Call {
        Accept,
        Finish(ExecutionOutcome),
    }
    use HostedValidationKnown as K;
    let rows: Vec<(Option<Call>, Call, K, K)> = vec![
        // Never seen: accepting records it; a cancel before it runs records the end.
        (None, Call::Accept, K::Unknown, K::Accepted),
        (
            None,
            Call::Finish(cancelled.clone()),
            K::Unknown,
            K::Finished(cancelled.clone()),
        ),
        // Accepted: accepting again starts nothing; its end is recorded once.
        (Some(Call::Accept), Call::Accept, K::Accepted, K::Accepted),
        (
            Some(Call::Accept),
            Call::Finish(passed.clone()),
            K::Accepted,
            K::Finished(passed.clone()),
        ),
        // Finished: nothing changes it, neither a new start nor another end.
        (
            Some(Call::Finish(passed.clone())),
            Call::Accept,
            K::Finished(passed.clone()),
            K::Finished(passed.clone()),
        ),
        (
            Some(Call::Finish(passed.clone())),
            Call::Finish(cancelled.clone()),
            K::Finished(passed.clone()),
            K::Finished(passed.clone()),
        ),
    ];
    for (row, (before, call, answer, after)) in rows.into_iter().enumerate() {
        let home = tempfile::tempdir().unwrap();
        let registry = HostedValidationRegistry::new(home.path());
        let run = |call: &Call| match call {
            Call::Accept => registry.accept(CHECKOUT, &id(1), 3).unwrap(),
            Call::Finish(outcome) => registry
                .finish(CHECKOUT, &id(1), 3, outcome.clone())
                .unwrap(),
        };
        if let Some(before) = &before {
            run(before);
        }
        assert_eq!(run(&call), answer, "row {row}");
        assert_eq!(
            registry.known(CHECKOUT, &id(1)).unwrap(),
            after,
            "row {row}"
        );
        // Another checkout's same id is its own execution.
        assert_eq!(registry.known("target-2", &id(1)).unwrap(), K::Unknown);
    }
}

/// Bounded: the oldest finished record goes first; an accepted one goes only when
/// nothing finished is left. A damaged file is refused, never guessed.
#[test]
fn hs2_7btv98_the_record_is_bounded_and_refuses_damage() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedValidationRegistry::new(home.path());
    // Interleaved: a running one first, then finished ones up to the bound.
    registry.accept(CHECKOUT, &id(0), 1).unwrap();
    for n in 1..MAX_HOSTED_VALIDATION_RECORDS as u128 {
        registry
            .finish(CHECKOUT, &id(n), 1, outcome(ExecutionEnd::Exited, Some(0)))
            .unwrap();
    }
    registry.accept(CHECKOUT, &id(9_999), 2).unwrap();
    // The oldest finished one made room; the running one is kept.
    assert_eq!(
        registry.known(CHECKOUT, &id(1)).unwrap(),
        HostedValidationKnown::Unknown
    );
    assert_eq!(
        registry.known(CHECKOUT, &id(0)).unwrap(),
        HostedValidationKnown::Accepted
    );
    assert_eq!(
        registry.known(CHECKOUT, &id(9_999)).unwrap(),
        HostedValidationKnown::Accepted
    );
    assert!(registry.accept("not valid", &id(1), 1).is_err());
    std::fs::write(
        home.path().join("orchestration-hosted-validations.json"),
        b"{\"schema_version\":1,\"executions\":[{\"checkout_id\":\"\"}]}",
    )
    .unwrap();
    assert!(registry.known(CHECKOUT, &id(0)).is_err());
}

#[test]
fn hs2_hts89s_a_running_execution_records_its_group_until_it_ends() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedValidationRegistry::new(home.path());
    let process = StepProcess {
        pid: 4_000,
        started: 11,
        host_pid: 3_000,
        host_started: 7,
    };
    // Nothing is recorded for an id that was never accepted.
    registry.launched(CHECKOUT, &id(1), process).unwrap();
    assert_eq!(registry.record(CHECKOUT, &id(1)).unwrap(), None);
    registry.accept(CHECKOUT, &id(1), 1).unwrap();
    registry.launched(CHECKOUT, &id(1), process).unwrap();
    assert_eq!(registry.with_process().unwrap().len(), 1);
    // Forgotten when stopped, and set again only while it still runs.
    // HS2-M7EV18: a launcher's late note never replaces a recorded process.
    let late = StepProcess {
        pid: 9_000,
        ..process
    };
    registry.launching(CHECKOUT, &id(1), late).unwrap();
    assert_eq!(
        registry.record(CHECKOUT, &id(1)).unwrap().unwrap().process,
        Some(process)
    );
    registry.process_gone(CHECKOUT, &id(1)).unwrap();
    assert!(registry.with_process().unwrap().is_empty());
    registry.launched(CHECKOUT, &id(1), process).unwrap();
    // An end forgets the group, and a late launch report does not bring it back.
    registry
        .finish(CHECKOUT, &id(1), 2, outcome(ExecutionEnd::Exited, Some(0)))
        .unwrap();
    registry.launched(CHECKOUT, &id(1), process).unwrap();
    let ended = registry.record(CHECKOUT, &id(1)).unwrap().unwrap();
    assert_eq!(ended.process, None);
    assert!(ended.outcome.is_some());
    assert!(registry.with_process().unwrap().is_empty());
}
