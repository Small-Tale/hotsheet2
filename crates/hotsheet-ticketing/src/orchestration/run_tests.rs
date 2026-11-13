//! HS2-EBHDSB run state machine and registry sequences; synthetic homes only.

use super::*;

struct Clock(std::cell::Cell<u64>);
impl crate::Clock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}
struct Rng(u128);
impl crate::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}
const OWNER: &str = "owner-0123456789ab";
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn new(operation: &str, native: &str, mode: DispatchMode, worker: Option<&str>) -> NewRun {
    NewRun {
        operation_id: operation.into(),
        ticket: RunTicket {
            connection_id: "owner-source".into(),
            native_id: id(500),
        },
        target: RunTarget {
            target_id: id(1),
            connection_id: "target-source".into(),
            native_id: native.into(),
        },
        mode,
        worker_id: worker.map(Into::into),
    }
}
fn to(state: RunState, reason: &str) -> RunUpdate {
    RunUpdate {
        to: Some(state),
        reason: Some(reason.into()),
        ..RunUpdate::default()
    }
}

#[test]
fn hs2_ebhdsb_every_state_edge_is_allowed_or_refused_exactly() {
    use RunState::{Cancelled, Completed, Dispatched, Failed, Intervened, Queued, Requested};
    let allowed = [
        (Requested, Queued),
        (Requested, Dispatched),
        (Requested, Failed),
        (Requested, Cancelled),
        (Queued, Dispatched),
        (Queued, Completed),
        (Queued, Failed),
        (Queued, Cancelled),
        (Queued, Intervened),
        (Dispatched, Completed),
        (Dispatched, Failed),
        (Dispatched, Cancelled),
        (Dispatched, Intervened),
        (Intervened, Dispatched),
        (Intervened, Completed),
        (Intervened, Cancelled),
    ];
    for from in RunState::ALL {
        for next in RunState::ALL {
            assert_eq!(
                from.may_become(next),
                allowed.contains(&(from, next)),
                "{from:?} -> {next:?}"
            );
        }
    }
    for state in RunState::ALL {
        assert_eq!(
            state.is_terminal(),
            matches!(state, Completed | Failed | Cancelled)
        );
        assert_eq!(
            state.is_active(),
            matches!(state, Requested | Queued | Dispatched)
        );
        // A finished run takes no edge at all.
        if state.is_terminal() {
            assert!(RunState::ALL.iter().all(|next| !state.may_become(*next)));
        }
    }
    // The registry enforces the same table through real updates.
    let home = tempfile::tempdir().unwrap();
    let registry = RunRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let mut counter = 0;
    for from in RunState::ALL {
        for next in RunState::ALL {
            counter += 1;
            let (run, _) = registry
                .begin(
                    OWNER,
                    new(
                        &format!("op-{counter}"),
                        &format!("t{counter}"),
                        DispatchMode::Direct,
                        Some(&format!("worker-{counter}")),
                    ),
                    None,
                    &mut tx,
                )
                .unwrap();
            // Walk to `from` along allowed edges.
            let path: &[RunState] = match from {
                Requested => &[],
                Queued => &[Queued],
                Dispatched => &[Dispatched],
                Completed => &[Dispatched, Completed],
                Failed => &[Failed],
                Cancelled => &[Cancelled],
                Intervened => &[Dispatched, Intervened],
            };
            for step in path {
                registry
                    .update(OWNER, &run.run_id, to(*step, "setup"), &mut tx)
                    .unwrap();
            }
            let result = registry.update(OWNER, &run.run_id, to(next, "edge"), &mut tx);
            if from == next && !from.is_terminal() {
                // Same state: an update, not a transition, and not an error.
                assert_eq!(result.unwrap().state, from);
            } else if from.is_terminal() {
                assert!(matches!(
                    result,
                    Err(OrchestrationError::Run(RunError::NotActive))
                ));
            } else if allowed.contains(&(from, next)) {
                assert_eq!(result.unwrap().state, next);
            } else {
                assert!(
                    matches!(
                        result,
                        Err(OrchestrationError::Run(RunError::InvalidTransition))
                    ),
                    "{from:?} -> {next:?}"
                );
                assert_eq!(registry.get(OWNER, &run.run_id).unwrap().state, from);
            }
        }
    }
}

#[test]
fn hs2_ebhdsb_duplicate_busy_and_conflicting_dispatches() {
    let home = tempfile::tempdir().unwrap();
    let registry = RunRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert!(registry.list(OWNER, false).unwrap().is_empty());
    assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());
    let (first, created) = registry
        .begin(
            OWNER,
            new("op-1", "t1", DispatchMode::Direct, Some("worker-1")),
            Some("scheduler".into()),
            &mut tx,
        )
        .unwrap();
    assert!(created);
    assert_eq!(first.state, RunState::Requested);
    assert_eq!(first.history.len(), 1);
    assert_eq!(first.history[0].actor_id.as_deref(), Some("scheduler"));
    let path = home.path().join("orchestration-runs.json");
    let bytes = std::fs::read(&path).unwrap();
    // The same operation again is the same run and writes nothing.
    clock.0.set(200);
    let (again, created) = registry
        .begin(
            OWNER,
            new("op-1", "t1", DispatchMode::Direct, Some("worker-1")),
            None,
            &mut tx,
        )
        .unwrap();
    assert!(!created);
    assert_eq!(again, first);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    // The same operation id for anything else conflicts.
    for different in [
        new("op-1", "t2", DispatchMode::Direct, Some("worker-1")),
        new("op-1", "t1", DispatchMode::Direct, Some("worker-2")),
        new("op-1", "t1", DispatchMode::Dispatcher, None),
    ] {
        assert!(matches!(
            registry.begin(OWNER, different, None, &mut tx),
            Err(OrchestrationError::Conflict)
        ));
    }
    // One active run per target ticket, whatever the operation or worker.
    for other in [
        new("op-2", "t1", DispatchMode::Direct, Some("worker-2")),
        new("op-3", "t1", DispatchMode::Dispatcher, None),
    ] {
        assert!(matches!(
            registry.begin(OWNER, other, None, &mut tx),
            Err(OrchestrationError::Run(RunError::TargetActive))
        ));
    }
    // A worker with an active direct run on this target is busy for another ticket.
    assert!(matches!(
        registry.begin(
            OWNER,
            new("op-4", "t2", DispatchMode::Direct, Some("worker-1")),
            None,
            &mut tx
        ),
        Err(OrchestrationError::Run(RunError::WorkerBusy))
    ));
    // Another worker, another owner and the dispatcher path are unaffected.
    registry
        .begin(
            OWNER,
            new("op-5", "t2", DispatchMode::Direct, Some("worker-2")),
            None,
            &mut tx,
        )
        .unwrap();
    registry
        .begin(
            "other-0123456789ab",
            new("op-1", "t1", DispatchMode::Direct, Some("worker-1")),
            None,
            &mut tx,
        )
        .unwrap();
    let (queued, _) = registry
        .begin(
            OWNER,
            new("op-6", "t3", DispatchMode::Dispatcher, None),
            None,
            &mut tx,
        )
        .unwrap();
    // A dispatcher run learns its worker later; a retry is still the same run.
    registry
        .update(
            OWNER,
            &queued.run_id,
            RunUpdate {
                to: Some(RunState::Queued),
                ..RunUpdate::default()
            },
            &mut tx,
        )
        .unwrap();
    registry
        .update(
            OWNER,
            &queued.run_id,
            RunUpdate {
                to: Some(RunState::Dispatched),
                worker_id: Some("pool-worker".into()),
                lease_expires_at_ms: Some(Some(9_000)),
                ..RunUpdate::default()
            },
            &mut tx,
        )
        .unwrap();
    let (retried, created) = registry
        .begin(
            OWNER,
            new("op-6", "t3", DispatchMode::Dispatcher, None),
            None,
            &mut tx,
        )
        .unwrap();
    assert!(!created);
    assert_eq!(retried.worker_id.as_deref(), Some("pool-worker"));
    // Once the first run finishes, its ticket and worker are free again.
    registry
        .update(
            OWNER,
            &first.run_id,
            to(RunState::Failed, "target_conflict"),
            &mut tx,
        )
        .unwrap();
    registry
        .begin(
            OWNER,
            new("op-7", "t1", DispatchMode::Direct, Some("worker-1")),
            None,
            &mut tx,
        )
        .unwrap();
    assert_eq!(registry.list(OWNER, true).unwrap().len(), 3);
    assert_eq!(registry.list(OWNER, false).unwrap().len(), 4);
    // Mode and worker must agree, and nothing is written for an invalid request.
    let count = registry.list(OWNER, false).unwrap().len();
    for invalid in [
        new("op-8", "t8", DispatchMode::Direct, None),
        new("op-8", "t8", DispatchMode::Dispatcher, Some("worker-1")),
        new("op 8", "t8", DispatchMode::Direct, Some("worker-1")),
        new("op-8", "a/b", DispatchMode::Direct, Some("worker-1")),
        new("op-8", "t8", DispatchMode::Direct, Some("bad worker")),
    ] {
        assert!(matches!(
            registry.begin(OWNER, invalid, None, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    assert_eq!(registry.list(OWNER, false).unwrap().len(), count);
}

#[test]
fn hs2_ebhdsb_history_keeps_repeated_and_reversed_transitions() {
    let home = tempfile::tempdir().unwrap();
    let registry = RunRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let (run, _) = registry
        .begin(
            OWNER,
            new("op-1", "t1", DispatchMode::Direct, Some("worker-1")),
            None,
            &mut tx,
        )
        .unwrap();
    let dispatched = registry
        .update(
            OWNER,
            &run.run_id,
            RunUpdate {
                lease_expires_at_ms: Some(Some(5_000)),
                ..to(RunState::Dispatched, "claimed")
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(dispatched.lease_expires_at_ms, Some(5_000));
    // A renewal updates the lease without a history entry.
    clock.0.set(300);
    let renewed = registry
        .update(
            OWNER,
            &run.run_id,
            RunUpdate {
                to: Some(RunState::Dispatched),
                lease_expires_at_ms: Some(Some(8_000)),
                ..RunUpdate::default()
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(renewed.lease_expires_at_ms, Some(8_000));
    assert_eq!(renewed.history.len(), 2);
    assert_eq!(renewed.updated_at_ms, 300);
    // An identical update writes nothing.
    let bytes = std::fs::read(home.path().join("orchestration-runs.json")).unwrap();
    clock.0.set(400);
    registry
        .update(
            OWNER,
            &run.run_id,
            RunUpdate {
                to: Some(RunState::Dispatched),
                lease_expires_at_ms: Some(Some(8_000)),
                ..RunUpdate::default()
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        std::fs::read(home.path().join("orchestration-runs.json")).unwrap(),
        bytes
    );
    // Taken over, handed back, taken over again: every reversal is kept in order.
    for _ in 0..2 {
        let taken = registry
            .update(
                OWNER,
                &run.run_id,
                to(RunState::Intervened, "claim_lost"),
                &mut tx,
            )
            .unwrap();
        // Leaving `dispatched` drops the lease the run no longer holds.
        assert_eq!(taken.lease_expires_at_ms, None);
        registry
            .update(
                OWNER,
                &run.run_id,
                RunUpdate {
                    lease_expires_at_ms: Some(Some(9_000)),
                    ..to(RunState::Dispatched, "handed_back")
                },
                &mut tx,
            )
            .unwrap();
    }
    // A cancel that could not reach the target is remembered until the run ends.
    let pending = registry
        .update(
            OWNER,
            &run.run_id,
            RunUpdate {
                cancel_requested: Some(true),
                ..RunUpdate::default()
            },
            &mut tx,
        )
        .unwrap();
    assert!(pending.cancel_requested);
    let done = registry
        .update(
            OWNER,
            &run.run_id,
            to(RunState::Cancelled, "released"),
            &mut tx,
        )
        .unwrap();
    assert!(!done.cancel_requested);
    let states: Vec<_> = done.history.iter().map(|entry| entry.to).collect();
    use RunState::{Cancelled, Dispatched, Intervened, Requested};
    assert_eq!(
        states,
        [
            Requested, Dispatched, Intervened, Dispatched, Intervened, Dispatched, Cancelled
        ]
    );
    assert_eq!(done.history[2].from, Some(Dispatched));
    assert_eq!(done.history[2].reason.as_deref(), Some("claim_lost"));
    // History is bounded but always keeps how the run began.
    let (long, _) = registry
        .begin(
            OWNER,
            new("op-2", "t2", DispatchMode::Direct, Some("worker-2")),
            Some("first-actor".into()),
            &mut tx,
        )
        .unwrap();
    registry
        .update(
            OWNER,
            &long.run_id,
            to(RunState::Dispatched, "claimed"),
            &mut tx,
        )
        .unwrap();
    for _ in 0..MAX_RUN_HISTORY {
        registry
            .update(
                OWNER,
                &long.run_id,
                to(RunState::Intervened, "claim_lost"),
                &mut tx,
            )
            .unwrap();
        registry
            .update(
                OWNER,
                &long.run_id,
                to(RunState::Dispatched, "handed_back"),
                &mut tx,
            )
            .unwrap();
    }
    let long = registry.get(OWNER, &long.run_id).unwrap();
    assert_eq!(long.history.len(), MAX_RUN_HISTORY);
    assert_eq!(long.history[0].to, Requested);
    assert_eq!(long.history[0].actor_id.as_deref(), Some("first-actor"));
    // Free text is refused; reasons are codes.
    assert!(matches!(
        registry.update(
            OWNER,
            &long.run_id,
            to(RunState::Failed, "the worker gave up"),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    // Unknown runs and other owners are not found.
    assert!(matches!(
        registry.get(OWNER, &id(999)),
        Err(OrchestrationError::Run(RunError::NotFound))
    ));
    assert!(matches!(
        registry.update(
            "other-0123456789ab",
            &long.run_id,
            to(RunState::Failed, "x"),
            &mut tx
        ),
        Err(OrchestrationError::Run(RunError::NotFound))
    ));
}

#[test]
fn hs2_ebhdsb_capacity_forgets_finished_runs_and_data_fails_closed() {
    let home = tempfile::tempdir().unwrap();
    let registry = RunRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let mut ids = Vec::new();
    for index in 0..MAX_RUNS {
        let (run, _) = registry
            .begin(
                OWNER,
                new(
                    &format!("op-{index}"),
                    &format!("t{index}"),
                    DispatchMode::Dispatcher,
                    None,
                ),
                None,
                &mut tx,
            )
            .unwrap();
        ids.push(run.run_id);
    }
    // Full of active runs: a new one is refused rather than forgetting live work.
    assert!(matches!(
        registry.begin(
            OWNER,
            new("op-more", "t-more", DispatchMode::Dispatcher, None),
            None,
            &mut tx
        ),
        Err(OrchestrationError::Capacity)
    ));
    registry
        .update(OWNER, &ids[3], to(RunState::Cancelled, "released"), &mut tx)
        .unwrap();
    registry
        .begin(
            OWNER,
            new("op-more", "t-more", DispatchMode::Dispatcher, None),
            None,
            &mut tx,
        )
        .unwrap();
    let runs = registry.list(OWNER, false).unwrap();
    assert_eq!(runs.len(), MAX_RUNS);
    assert!(runs.iter().all(|run| run.run_id != ids[3]));
    assert!(runs.iter().all(|run| run.state.is_active()));
    let path = home.path().join("orchestration-runs.json");
    for bytes in [&b"{nope"[..], &br#"{"schema_version":2,"runs":[]}"#[..]] {
        std::fs::write(&path, bytes).unwrap();
        assert!(registry.list(OWNER, false).is_err());
        assert!(
            registry
                .begin(
                    OWNER,
                    new("op-x", "tx", DispatchMode::Dispatcher, None),
                    None,
                    &mut tx
                )
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
