//! Batch integration transitions (HS2-9G0M3V).

use super::batch::{
    BatchCandidate, BatchConclusion, BatchFailure, BatchState, LEDGER_FILE, select_batch,
};
use std::collections::BTreeMap;

use super::strategy::CircuitBreaker;
use super::tests::{NOW, SHA, assignment, fixture, register};
use super::*;

const MAIN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MAIN_2: &str = "cccccccccccccccccccccccccccccccccccccccc";
const LIMITS: CircuitBreaker = CircuitBreaker {
    max_red_batches: 2,
    max_red_minutes: 120,
};

fn integrator_request() -> StrategyRequest {
    StrategyRequest {
        requested: Some(IntegrationStrategy::Batch),
        default: IntegrationStrategy::Serial,
        all_participants_upgraded: true,
    }
}

fn renew(c: &Coordinator, now: i64) {
    c.register_with_strategy(
        Role::Integrator,
        "integrator",
        "generation-1",
        now,
        3600,
        ReturnRoute::HumanRelay,
        integrator_request(),
    )
    .unwrap();
}

/// A schema-eight pool with a live batch integrator and `count` ready code handoffs
/// `a0`, `a1`, ...
fn pool(count: usize) -> (tempfile::TempDir, Coordinator) {
    let (root, c) = fixture();
    renew(&c, NOW);
    for index in 0..count {
        let worker = format!("worker-{index}");
        let id = format!("a{index}");
        register(&c, Role::Worker, &worker, "generation-1", NOW);
        c.assign(
            "integrator",
            "generation-1",
            NOW,
            assignment(&id, &worker, &format!("native-{index}")),
        )
        .unwrap();
        c.worker_transition(&worker, "generation-1", &id, Phase::Accepted, None)
            .unwrap();
        c.worker_transition(
            &worker,
            "generation-1",
            &id,
            Phase::Handoff,
            Some(SHA.into()),
        )
        .unwrap();
    }
    (root, c)
}

fn member(id: &str, path: &str) -> (String, Vec<String>) {
    (id.into(), vec![path.into()])
}

fn phase(c: &Coordinator, id: &str) -> Phase {
    c.state().unwrap().assignments[id].phase
}

fn shipping(ids: &[&str]) -> BTreeMap<String, Vec<String>> {
    ids.iter()
        .map(|id| ((*id).to_owned(), vec![MAIN.to_owned()]))
        .collect()
}

fn open_and_push(c: &Coordinator, batch: &str, ids: &[&str], now: i64, main: &str) {
    c.batch_open(
        "integrator",
        "generation-1",
        now,
        batch,
        ids.iter()
            .map(|id| member(id, &format!("src/{id}.rs")))
            .collect(),
        LIMITS,
    )
    .unwrap();
    c.batch_pushed(
        "integrator",
        "generation-1",
        now,
        batch,
        main,
        shipping(ids),
        vec!["cargo lint".into()],
    )
    .unwrap();
}

fn candidate(id: &str, rank: u8, at: &str, paths: &[&str]) -> BatchCandidate {
    BatchCandidate {
        assignment: id.into(),
        ticket: TicketRef {
            connection_id: "source-a".into(),
            native_id: id.into(),
        },
        slug: id.to_uppercase(),
        priority_rank: rank,
        handoff_at: at.into(),
        paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        blocker: None,
    }
}

#[test]
fn hs2_9g0m3v_selection_orders_by_priority_then_handoff_and_defers_conflicts() {
    let mut blocked = candidate("blocked", 4, "2026-01-01T00:00:00Z", &["x.rs"]);
    blocked.blocker = Some("its ticket claim is still live".into());
    let plan = select_batch(
        vec![
            candidate("late-default", 2, "2026-01-03T00:00:00Z", &["docs/a.md"]),
            candidate("early-default", 2, "2026-01-01T00:00:00Z", &["src/lib.rs"]),
            candidate("high", 3, "2026-01-05T00:00:00Z", &["src"]),
            candidate("separate", 1, "2026-01-01T00:00:00Z", &["clients/web/a.ts"]),
            candidate("empty", 2, "2026-01-02T00:00:00Z", &[]),
            blocked,
        ],
        None,
    );
    let selected: Vec<&str> = plan
        .selected
        .iter()
        .map(|c| c.assignment.as_str())
        .collect();
    // Priority first; `early-default` changes a path under `high`'s directory scope.
    assert_eq!(selected, ["high", "late-default", "separate"]);
    let deferred: BTreeMap<&str, &str> = plan
        .deferred
        .iter()
        .map(|d| (d.assignment.as_str(), d.reason.as_str()))
        .collect();
    assert!(deferred["early-default"].contains("overlap HIGH (high)"));
    assert!(deferred["empty"].contains("changes no paths"));
    assert!(deferred["blocked"].contains("claim is still live"));
    // Ties keep handoff time, then id; a cap defers the rest in that order.
    let capped = select_batch(
        vec![
            candidate("b", 2, "2026-01-01T00:00:00Z", &["b"]),
            candidate("a", 2, "2026-01-01T00:00:00Z", &["a"]),
            candidate("c", 2, "2025-12-31T00:00:00Z", &["c"]),
        ],
        Some(2),
    );
    let selected: Vec<&str> = capped
        .selected
        .iter()
        .map(|c| c.assignment.as_str())
        .collect();
    assert_eq!(selected, ["c", "a"]);
    assert_eq!(capped.deferred[0].assignment, "b");
    assert!(capped.deferred[0].reason.contains("full"));
}

#[test]
fn hs2_9g0m3v_batch_commands_are_refused_under_a_serial_session_and_never_create_a_ledger() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    register(&c, Role::Worker, "worker", "generation-1", NOW);
    c.assign(
        "integrator",
        "generation-1",
        NOW,
        assignment("a0", "worker", "native-0"),
    )
    .unwrap();
    c.worker_transition("worker", "generation-1", "a0", Phase::Accepted, None)
        .unwrap();
    c.worker_transition(
        "worker",
        "generation-1",
        "a0",
        Phase::Handoff,
        Some(SHA.into()),
    )
    .unwrap();
    let refused = c
        .batch_open(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![member("a0", "src/a.rs")],
            LIMITS,
        )
        .unwrap_err()
        .to_string();
    assert!(refused.contains("this session runs serial"), "{refused}");
    assert_eq!(phase(&c, "a0"), Phase::Handoff);
    assert!(!c.directory.join(LEDGER_FILE).exists());
    assert!(c.snapshot(NOW).unwrap().batches.is_none());
    assert!(c.batch_ledger().unwrap().is_none());
    // Serial marking still works and stays exclusive.
    c.mark("integrator", "generation-1", NOW, "a0", Phase::Integrating)
        .unwrap();
    assert!(!c.directory.join(LEDGER_FILE).exists());
}

#[test]
fn hs2_9g0m3v_open_push_green_lifecycle_replays_and_gates_completion() {
    let (_root, c) = pool(3);
    assert_eq!(c.state().unwrap().version, 8);
    let opened = c
        .batch_open(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![member("a1", "src/b.rs"), member("a0", "src/a.rs")],
            LIMITS,
        )
        .unwrap();
    assert_eq!(opened.state, BatchState::Integrating);
    // Members keep the requested integration order and their qualified tickets.
    assert_eq!(opened.tickets[0].assignment, "a1");
    assert_eq!(opened.tickets[0].ticket.native_id, "native-1");
    assert_eq!(opened.tickets[0].source_commit, SHA);
    assert_eq!(phase(&c, "a0"), Phase::Integrating);
    assert_eq!(phase(&c, "a1"), Phase::Integrating);
    // Same open replays; a different one under the same id is refused.
    c.batch_open(
        "integrator",
        "generation-1",
        NOW,
        "batch-1",
        vec![member("a1", "src/b.rs"), member("a0", "src/a.rs")],
        LIMITS,
    )
    .unwrap();
    assert!(
        c.batch_open(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![member("a2", "src/c.rs")],
            LIMITS
        )
        .is_err()
    );
    // The open batch is the one integration unit.
    let refused = c
        .batch_open(
            "integrator",
            "generation-1",
            NOW,
            "batch-2",
            vec![member("a2", "src/c.rs")],
            LIMITS,
        )
        .unwrap_err()
        .to_string();
    assert!(refused.contains("batch-1 is integrating"), "{refused}");
    assert!(
        c.mark("integrator", "generation-1", NOW, "a2", Phase::Integrating)
            .is_err()
    );
    // Members move only with their batch.
    for (id, target) in [("a0", Phase::Ci), ("a0", Phase::Done)] {
        let refused = c
            .mark("integrator", "generation-1", NOW, id, target)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("batch"), "{refused}");
    }
    assert!(
        c.suspend_integration(
            "integrator",
            "generation-1",
            NOW,
            "a0",
            IntegrationSuspension {
                main_commit: MAIN.into(),
                prerequisite: TicketRef {
                    connection_id: "source-a".into(),
                    native_id: "native-2".into(),
                },
                failure_note: hotsheet_model::Ulid::new().to_string(),
                resumed: None,
            },
            true,
        )
        .unwrap_err()
        .to_string()
        .contains("abort the batch")
    );
    // The push must name exactly the members.
    assert!(
        c.batch_pushed(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            MAIN,
            shipping(&["a0"]),
            vec!["cargo lint".into()],
        )
        .is_err()
    );
    let pushed = c
        .batch_pushed(
            "integrator",
            "generation-1",
            NOW + 10,
            "batch-1",
            MAIN,
            shipping(&["a0", "a1"]),
            vec!["cargo lint".into(), "npm run typecheck".into()],
        )
        .unwrap();
    assert_eq!(pushed.state, BatchState::Ci);
    assert_eq!(pushed.main_commit.as_deref(), Some(MAIN));
    assert_eq!(pushed.tickets[1].shipping_commits, [MAIN]);
    assert_eq!(phase(&c, "a0"), Phase::Ci);
    // Replay of the same push is idempotent; a different push is refused.
    c.batch_pushed(
        "integrator",
        "generation-1",
        NOW + 11,
        "batch-1",
        MAIN,
        shipping(&["a0", "a1"]),
        vec!["cargo lint".into(), "npm run typecheck".into()],
    )
    .unwrap();
    assert!(
        c.batch_pushed(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            MAIN_2,
            shipping(&["a0", "a1"]),
            vec!["cargo lint".into()],
        )
        .is_err()
    );
    // CI pending: tickets cannot complete; another batch may not open yet.
    assert!(
        c.mark("integrator", "generation-1", NOW, "a0", Phase::Done)
            .unwrap_err()
            .to_string()
            .contains("only after its CI result")
    );
    let status = c.snapshot(NOW).unwrap().batches.unwrap();
    assert_eq!(status.effective_strategy, IntegrationStrategy::Batch);
    assert_eq!(status.active.len(), 1);
    let green = c
        .batch_result(
            "integrator",
            "generation-1",
            NOW + 20,
            "batch-1",
            "1001",
            BatchConclusion::Success,
            LIMITS,
        )
        .unwrap();
    assert_eq!(green.batch.state, BatchState::Green);
    assert!(green.notify.is_none());
    // A green batch never turns red; replaying green is idempotent.
    assert!(
        c.batch_result(
            "integrator",
            "generation-1",
            NOW + 21,
            "batch-1",
            "1002",
            BatchConclusion::Failure,
            LIMITS,
        )
        .is_err()
    );
    c.batch_result(
        "integrator",
        "generation-1",
        NOW + 21,
        "batch-1",
        "1001",
        BatchConclusion::Success,
        LIMITS,
    )
    .unwrap();
    c.mark("integrator", "generation-1", NOW, "a0", Phase::Done)
        .unwrap();
    c.mark("integrator", "generation-1", NOW, "a1", Phase::Done)
        .unwrap();
    // Every member done: no active batch remains, and a new batch can open.
    assert!(c.snapshot(NOW).unwrap().batches.unwrap().active.is_empty());
    c.batch_open(
        "integrator",
        "generation-1",
        NOW + 30,
        "batch-2",
        vec![member("a2", "src/c.rs")],
        LIMITS,
    )
    .unwrap();
    // The pool schema did not move past eight: the ledger is beside the pool.
    assert_eq!(c.state().unwrap().version, 8);
}

#[test]
fn hs2_9g0m3v_smoke_failure_aborts_before_push_and_returns_every_member() {
    let (_root, c) = pool(2);
    c.batch_open(
        "integrator",
        "generation-1",
        NOW,
        "batch-1",
        vec![member("a0", "src/a.rs"), member("a1", "src/b.rs")],
        LIMITS,
    )
    .unwrap();
    let aborted = c
        .batch_abort(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            "smoke gate failed: cargo lint",
        )
        .unwrap();
    assert_eq!(aborted.state, BatchState::Aborted);
    assert!(aborted.main_commit.is_none());
    assert_eq!(phase(&c, "a0"), Phase::Handoff);
    assert_eq!(phase(&c, "a1"), Phase::Handoff);
    // Replay with the same reason is idempotent; nothing about it can be pushed.
    c.batch_abort(
        "integrator",
        "generation-1",
        NOW,
        "batch-1",
        "smoke gate failed: cargo lint",
    )
    .unwrap();
    assert!(
        c.batch_pushed(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            MAIN,
            shipping(&["a0", "a1"]),
            vec!["cargo lint".into()],
        )
        .is_err()
    );
    assert!(c.snapshot(NOW).unwrap().batches.unwrap().active.is_empty());
    // The returned handoffs join a later batch, or integrate serially.
    c.batch_open(
        "integrator",
        "generation-1",
        NOW,
        "batch-2",
        vec![member("a0", "src/a.rs")],
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        c.batch_ledger()
            .unwrap()
            .unwrap()
            .batch_of("a0")
            .unwrap()
            .id,
        "batch-2"
    );
    // A pushed batch cannot be aborted.
    c.batch_pushed(
        "integrator",
        "generation-1",
        NOW,
        "batch-2",
        MAIN,
        shipping(&["a0"]),
        vec!["cargo lint".into()],
    )
    .unwrap();
    assert!(
        c.batch_abort("integrator", "generation-1", NOW, "batch-2", "late")
            .is_err()
    );
}

#[test]
fn hs2_9g0m3v_open_refuses_overlap_non_handoffs_and_foreign_sessions() {
    let (_root, c) = pool(3);
    let overlap = c
        .batch_open(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![
                ("a0".into(), vec!["src".into()]),
                ("a1".into(), vec!["src/lib.rs".into()]),
            ],
            LIMITS,
        )
        .unwrap_err()
        .to_string();
    assert!(overlap.contains("overlapping paths"), "{overlap}");
    assert!(
        c.batch_open(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![member("a0", "a"), member("a0", "b")],
            LIMITS
        )
        .is_err()
    );
    assert!(
        c.batch_open(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![("a0".into(), Vec::new())],
            LIMITS
        )
        .is_err()
    );
    assert!(
        c.batch_open(
            "integrator",
            "stale",
            NOW,
            "batch-1",
            vec![member("a0", "a")],
            LIMITS
        )
        .is_err()
    );
    assert!(
        c.batch_open(
            "integrator",
            "generation-1",
            NOW,
            "bad id",
            vec![member("a0", "a")],
            LIMITS
        )
        .is_err()
    );
    // A serially integrating ticket holds the fence.
    c.mark("integrator", "generation-1", NOW, "a2", Phase::Integrating)
        .unwrap();
    assert!(
        c.batch_open(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![member("a0", "a")],
            LIMITS
        )
        .unwrap_err()
        .to_string()
        .contains("integrating or verifying")
    );
    // Nothing above changed a member or wrote a batch.
    assert_eq!(phase(&c, "a0"), Phase::Handoff);
    assert!(
        c.batch_ledger()
            .unwrap()
            .is_none_or(|ledger| ledger.batches.is_empty())
    );
}

#[test]
fn hs2_9g0m3v_red_batch_completes_only_once_every_failure_has_a_follow_up() {
    let (_root, c) = pool(2);
    open_and_push(&c, "batch-1", &["a0", "a1"], NOW, MAIN);
    assert!(
        c.batch_follow_ups(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![failure("k1", "HS-9")],
        )
        .unwrap_err()
        .to_string()
        .contains("follow-ups record a red batch")
    );
    let red = c
        .batch_result(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            "2001",
            BatchConclusion::Failure,
            LIMITS,
        )
        .unwrap();
    assert_eq!(red.batch.state, BatchState::Red);
    assert_eq!(red.breaker.consecutive_red, 1);
    assert!(red.notify.is_none());
    let blocked = c
        .mark("integrator", "generation-1", NOW, "a0", Phase::Done)
        .unwrap_err()
        .to_string();
    assert!(
        blocked.contains("record a follow-up for each failure"),
        "{blocked}"
    );
    let recorded = c
        .batch_follow_ups(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![failure("k1", "HS-9"), failure("k2", "HS-10")],
        )
        .unwrap();
    assert!(recorded.attributed);
    assert_eq!(recorded.failures.len(), 2);
    // A replay merges; a key cannot move to another follow-up.
    c.batch_follow_ups(
        "integrator",
        "generation-1",
        NOW,
        "batch-1",
        vec![failure("k1", "HS-9")],
    )
    .unwrap();
    assert!(
        c.batch_follow_ups(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            vec![failure("k1", "HS-11")],
        )
        .is_err()
    );
    c.mark("integrator", "generation-1", NOW, "a0", Phase::Done)
        .unwrap();
    c.mark("integrator", "generation-1", NOW, "a1", Phase::Done)
        .unwrap();
    // The breaker still remembers main is red until a green batch or recovery.
    let status = c.snapshot(NOW).unwrap().batches.unwrap();
    assert_eq!(status.breaker.consecutive_red, 1);
    assert_eq!(status.breaker.last_red_main.as_deref(), Some(MAIN));
}

fn failure(key: &str, follow_up: &str) -> BatchFailure {
    BatchFailure {
        key: key.into(),
        follow_up: follow_up.into(),
        follow_up_ticket: None,
        suspects: Vec::new(),
    }
}

#[test]
fn hs2_9g0m3v_red_batch_rerun_green_clears_the_red_count() {
    let (_root, c) = pool(1);
    open_and_push(&c, "batch-1", &["a0"], NOW, MAIN);
    c.batch_result(
        "integrator",
        "generation-1",
        NOW,
        "batch-1",
        "3001",
        BatchConclusion::Failure,
        LIMITS,
    )
    .unwrap();
    let green = c
        .batch_result(
            "integrator",
            "generation-1",
            NOW + 60,
            "batch-1",
            "3002",
            BatchConclusion::Success,
            LIMITS,
        )
        .unwrap();
    assert_eq!(green.batch.state, BatchState::Green);
    assert_eq!(green.breaker.consecutive_red, 0);
    assert!(green.breaker.red_since.is_none());
    c.mark("integrator", "generation-1", NOW, "a0", Phase::Done)
        .unwrap();
}

#[test]
fn hs2_9g0m3v_circuit_breaker_trips_by_count_falls_back_to_serial_and_recovers() {
    let (_root, c) = pool(4);
    open_and_push(&c, "batch-1", &["a0"], NOW, MAIN);
    let first = c
        .batch_result(
            "integrator",
            "generation-1",
            NOW,
            "batch-1",
            "4001",
            BatchConclusion::Failure,
            LIMITS,
        )
        .unwrap();
    assert!(first.notify.is_none());
    open_and_push(&c, "batch-2", &["a1"], NOW + 60, MAIN_2);
    let second = c
        .batch_result(
            "integrator",
            "generation-1",
            NOW + 60,
            "batch-2",
            "4002",
            BatchConclusion::Failure,
            LIMITS,
        )
        .unwrap();
    let notify = second.notify.unwrap();
    assert!(notify.contains("fell back to serial"), "{notify}");
    assert!(notify.contains("2 consecutive red batches"), "{notify}");
    let trip = second.breaker.tripped.clone().unwrap();
    assert_eq!(trip.at, NOW + 60);
    // A replayed red result does not trip or count again.
    let replay = c
        .batch_result(
            "integrator",
            "generation-1",
            NOW + 61,
            "batch-2",
            "4002",
            BatchConclusion::Failure,
            LIMITS,
        )
        .unwrap();
    assert!(replay.notify.is_none());
    assert_eq!(replay.breaker.consecutive_red, 2);
    // Fallback: no new batch, the status reports serial, single marks still integrate.
    let refused = c
        .batch_open(
            "integrator",
            "generation-1",
            NOW + 70,
            "batch-3",
            vec![member("a2", "src/a2.rs")],
            LIMITS,
        )
        .unwrap_err()
        .to_string();
    assert!(refused.contains("circuit breaker is tripped"), "{refused}");
    let status = c.snapshot(NOW + 70).unwrap().batches.unwrap();
    assert_eq!(status.effective_strategy, IntegrationStrategy::Serial);
    assert!(status.breaker.tripped.is_some());
    c.mark(
        "integrator",
        "generation-1",
        NOW + 70,
        "a2",
        Phase::Integrating,
    )
    .unwrap();
    assert!(
        c.mark(
            "integrator",
            "generation-1",
            NOW + 70,
            "a3",
            Phase::Integrating
        )
        .is_err()
    );
    c.mark("integrator", "generation-1", NOW + 70, "a2", Phase::Ci)
        .unwrap();
    // Recovery needs a tripped breaker and clears it once.
    let recovered = c
        .batch_recover("integrator", "generation-1", NOW + 90, "4003", MAIN_2)
        .unwrap();
    assert!(recovered.tripped.is_none());
    assert_eq!(recovered.consecutive_red, 0);
    assert_eq!(recovered.recovered.as_ref().unwrap().tripped_at, NOW + 60);
    c.batch_recover("integrator", "generation-1", NOW + 91, "4003", MAIN_2)
        .unwrap();
    assert!(
        c.batch_recover("integrator", "generation-1", NOW + 92, "4004", MAIN_2)
            .is_err()
    );
    assert_eq!(
        c.snapshot(NOW + 92)
            .unwrap()
            .batches
            .unwrap()
            .effective_strategy,
        IntegrationStrategy::Batch
    );
    c.batch_open(
        "integrator",
        "generation-1",
        NOW + 93,
        "batch-3",
        vec![member("a3", "src/a3.rs")],
        LIMITS,
    )
    .unwrap();
}

#[test]
fn hs2_9g0m3v_circuit_breaker_trips_when_main_stays_red_past_the_time_limit() {
    let (_root, c) = pool(2);
    let limits = CircuitBreaker {
        max_red_batches: 5,
        max_red_minutes: 30,
    };
    c.batch_open(
        "integrator",
        "generation-1",
        NOW,
        "batch-1",
        vec![member("a0", "src/a0.rs")],
        limits,
    )
    .unwrap();
    c.batch_pushed(
        "integrator",
        "generation-1",
        NOW,
        "batch-1",
        MAIN,
        shipping(&["a0"]),
        vec!["cargo lint".into()],
    )
    .unwrap();
    c.batch_result(
        "integrator",
        "generation-1",
        NOW,
        "batch-1",
        "5001",
        BatchConclusion::Failure,
        limits,
    )
    .unwrap();
    // Before the limit a new batch still opens; once main has been red 30 minutes the
    // next open records the trip and refuses.
    let later = NOW + 30 * 60;
    renew(&c, later);
    let refused = c
        .batch_open(
            "integrator",
            "generation-1",
            later,
            "batch-2",
            vec![member("a1", "src/a1.rs")],
            limits,
        )
        .unwrap_err()
        .to_string();
    assert!(refused.contains("red for 30 minutes"), "{refused}");
    let ledger = c.batch_ledger().unwrap().unwrap();
    assert_eq!(ledger.breaker.tripped.unwrap().at, later);
    assert!(!ledger.batches.contains_key("batch-2"));
    assert_eq!(phase(&c, "a1"), Phase::Handoff);
}

#[test]
fn hs2_9g0m3v_ledger_refuses_an_unknown_version() {
    let (_root, c) = pool(1);
    c.batch_open(
        "integrator",
        "generation-1",
        NOW,
        "batch-1",
        vec![member("a0", "src/a0.rs")],
        LIMITS,
    )
    .unwrap();
    let path = c.directory.join(LEDGER_FILE);
    let mut raw: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(raw["version"], 1);
    raw["version"] = 2.into();
    fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(c.batch_ledger().is_err());
    assert!(
        c.mark("integrator", "generation-1", NOW, "a0", Phase::Ci)
            .is_err()
    );
}
