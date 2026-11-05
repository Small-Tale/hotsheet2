//! ci-staged integration transitions (HS2-GT57BQ).

use std::collections::BTreeMap;

use super::batch::{BatchConclusion, BatchFailure, BatchState, LEDGER_FILE, LEDGER_VERSION};
use super::batch_staged::StagingOpen;
use super::strategy::CircuitBreaker;
use super::tests::{NOW, SHA, assignment, fixture, register};
use super::*;

const MAIN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MOVED_MAIN: &str = "9999999999999999999999999999999999999999";
const S1: &str = "1111111111111111111111111111111111111111";
const S2: &str = "2222222222222222222222222222222222222222";
const S3: &str = "3333333333333333333333333333333333333333";
const S4: &str = "4444444444444444444444444444444444444444";
const LIMITS: CircuitBreaker = CircuitBreaker {
    max_red_batches: 2,
    max_red_minutes: 120,
};
const I: &str = "integrator";
const G: &str = "generation-1";

fn request(strategy: IntegrationStrategy) -> StrategyRequest {
    StrategyRequest {
        requested: Some(strategy),
        default: IntegrationStrategy::Serial,
        all_participants_upgraded: true,
    }
}

fn register_integrator(c: &Coordinator, generation: &str, strategy: IntegrationStrategy) {
    c.register_with_strategy(
        Role::Integrator,
        I,
        generation,
        NOW,
        3600,
        ReturnRoute::HumanRelay,
        request(strategy),
    )
    .unwrap();
}

/// A schema-eight pool whose integrator runs `strategy`, with ready code handoffs `a0`...
fn pool(count: usize, strategy: IntegrationStrategy) -> (tempfile::TempDir, Coordinator) {
    let (root, c) = fixture();
    register_integrator(&c, G, strategy);
    for index in 0..count {
        let worker = format!("worker-{index}");
        let id = format!("a{index}");
        register(&c, Role::Worker, &worker, G, NOW);
        c.assign(
            I,
            G,
            NOW,
            assignment(&id, &worker, &format!("native-{index}")),
        )
        .unwrap();
        c.worker_transition(&worker, G, &id, Phase::Accepted, None)
            .unwrap();
        c.worker_transition(&worker, G, &id, Phase::Handoff, Some(SHA.into()))
            .unwrap();
    }
    (root, c)
}

fn staging() -> StagingOpen {
    StagingOpen {
        branch: "integration".into(),
        main_commit: MAIN.into(),
    }
}

fn members(ids: &[&str]) -> Vec<(String, Vec<String>)> {
    ids.iter()
        .map(|id| ((*id).to_owned(), vec![format!("src/{id}.rs")]))
        .collect()
}

fn shipping(ids: &[&str], sha: &str) -> BTreeMap<String, Vec<String>> {
    ids.iter()
        .map(|id| ((*id).to_owned(), vec![sha.to_owned()]))
        .collect()
}

fn open(c: &Coordinator, batch: &str, ids: &[&str], now: i64) -> super::batch::BatchManifest {
    c.batch_open_staged(I, G, now, batch, members(ids), LIMITS, staging())
        .unwrap()
}

fn stage(c: &Coordinator, batch: &str, ids: &[&str], sha: &str, now: i64) {
    open(c, batch, ids, now);
    c.batch_staged(
        I,
        G,
        now,
        batch,
        sha,
        shipping(ids, sha),
        vec!["cargo lint".into()],
    )
    .unwrap();
}

fn result(
    c: &Coordinator,
    batch: &str,
    run: &str,
    conclusion: BatchConclusion,
    now: i64,
) -> super::batch::BatchResultReport {
    c.batch_result(I, G, now, batch, run, conclusion, LIMITS)
        .unwrap()
}

fn phase(c: &Coordinator, id: &str) -> Phase {
    c.state().unwrap().assignments[id].phase
}

fn batch(c: &Coordinator, id: &str) -> super::batch::BatchManifest {
    c.batch_ledger().unwrap().unwrap().batches[id].clone()
}

fn failure(key: &str) -> BatchFailure {
    BatchFailure {
        key: key.into(),
        follow_up: "HS2-FOLLOW".into(),
        follow_up_ticket: None,
        suspects: vec!["A0".into()],
    }
}

fn error<T: std::fmt::Debug>(outcome: Result<T>) -> String {
    outcome.unwrap_err().to_string()
}

#[test]
fn hs2_gt57bq_staged_batch_reaches_main_only_after_green_and_exact_promotion() {
    let (root, c) = pool(2, IntegrationStrategy::CiStaged);
    let opened = open(&c, "batch-1", &["a0", "a1"], NOW);
    let staged = opened.staging.clone().unwrap();
    assert_eq!(
        (
            staged.branch.as_str(),
            staged.base_commit.as_str(),
            staged.parent
        ),
        ("integration", MAIN, None)
    );
    assert_eq!(phase(&c, "a0"), Phase::Integrating);
    // A staged batch never records a main push.
    assert!(
        error(c.batch_pushed(
            I,
            G,
            NOW,
            "batch-1",
            MAIN,
            shipping(&["a0", "a1"], S1),
            vec!["cargo lint".into()],
        ))
        .contains("batch-staged")
    );
    c.batch_staged(
        I,
        G,
        NOW + 1,
        "batch-1",
        S1,
        shipping(&["a0", "a1"], S1),
        vec!["cargo lint".into()],
    )
    .unwrap();
    // Replay is idempotent; a different staged push is refused.
    c.batch_staged(
        I,
        G,
        NOW + 2,
        "batch-1",
        S1,
        shipping(&["a0", "a1"], S1),
        vec!["cargo lint".into()],
    )
    .unwrap();
    assert!(
        error(c.batch_staged(
            I,
            G,
            NOW + 2,
            "batch-1",
            S2,
            shipping(&["a0", "a1"], S2),
            vec!["cargo lint".into()],
        ))
        .contains("different staged push")
    );
    let pushed = batch(&c, "batch-1");
    assert_eq!(pushed.state, BatchState::Ci);
    assert_eq!(pushed.main_commit, None);
    assert_eq!(pushed.staging.unwrap().staged_commit.as_deref(), Some(S1));
    assert_eq!(phase(&c, "a0"), Phase::Ci);
    // The ledger holding a staged batch is version two.
    let on_disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(c.directory.join(LEDGER_FILE)).unwrap()).unwrap();
    assert_eq!(on_disk["version"], LEDGER_VERSION);
    // Neither CI nor a green run alone lets a staged ticket complete.
    assert!(error(c.mark(I, G, NOW, "a0", Phase::Done)).contains("promoted"));
    assert!(error(c.batch_promote(I, G, NOW, "batch-1", S1)).contains("only a green"));
    let green = result(&c, "batch-1", "501", BatchConclusion::Success, NOW + 10);
    assert_eq!(green.batch.state, BatchState::Green);
    assert!(error(c.mark(I, G, NOW, "a0", Phase::Done)).contains("promoted"));
    let status = c.snapshot(NOW).unwrap().batches.unwrap();
    let summary = status.staging.unwrap();
    assert_eq!(summary.awaiting_promotion, ["batch-1"]);
    assert_eq!(summary.tip.as_deref(), Some(S1));
    // Promotion fast-forwards to exactly the green SHA: another (moved) main is refused.
    assert!(error(c.batch_promote(I, G, NOW, "batch-1", MOVED_MAIN)).contains("exactly the green"));
    let promoted = c.batch_promote(I, G, NOW + 20, "batch-1", S1).unwrap();
    assert_eq!(promoted.state, BatchState::Promoted);
    assert_eq!(promoted.main_commit.as_deref(), Some(S1));
    c.batch_promote(I, G, NOW + 21, "batch-1", S1).unwrap();
    assert!(error(c.batch_promote(I, G, NOW, "batch-1", S2)).contains("already promoted"));
    // A late or repeated green result changes nothing after promotion.
    result(&c, "batch-1", "502", BatchConclusion::Success, NOW + 22);
    assert_eq!(batch(&c, "batch-1").state, BatchState::Promoted);
    c.mark(I, G, NOW, "a0", Phase::Done).unwrap();
    c.mark(I, G, NOW, "a1", Phase::Done).unwrap();
    assert!(c.snapshot(NOW).unwrap().batches.unwrap().active.is_empty());
    drop(root);
}

#[test]
fn hs2_gt57bq_red_batch_leaves_main_untouched_follows_the_follow_up_rule_and_restages() {
    let (_root, c) = pool(3, IntegrationStrategy::CiStaged);
    stage(&c, "batch-1", &["a0", "a1"], S1, NOW);
    let red = result(&c, "batch-1", "601", BatchConclusion::Failure, NOW + 10);
    assert_eq!(red.batch.state, BatchState::Red);
    assert_eq!(red.batch.main_commit, None);
    // Main was never red: no red main commit for a recovery run to cover.
    assert_eq!(red.breaker.last_red_main, None);
    assert_eq!(red.breaker.consecutive_red, 1);
    assert!(error(c.mark(I, G, NOW, "a0", Phase::Done)).contains("never reached main"));
    // Nothing stacks on a red batch.
    assert!(
        error(c.batch_open_staged(I, G, NOW, "batch-2", members(&["a2"]), LIMITS, staging()))
            .contains("is red")
    );
    // The follow-up rule comes first.
    assert!(
        error(c.batch_restage(I, G, NOW, "batch-1", "red", vec!["a0".into()]))
            .contains("follow-up for each failure")
    );
    c.batch_follow_ups(I, G, NOW, "batch-1", vec![failure("run-601:job:test")])
        .unwrap();
    assert!(
        error(c.batch_restage(I, G, NOW, "batch-1", "red", vec!["a9".into()]))
            .contains("not a member")
    );
    let restaged = c
        .batch_restage(I, G, NOW + 20, "batch-1", "red run 601", vec!["a0".into()])
        .unwrap();
    assert_eq!(restaged.state, BatchState::Restaged);
    assert_eq!(restaged.staging.as_ref().unwrap().held, ["a0"]);
    c.batch_restage(I, G, NOW + 21, "batch-1", "red run 601", vec!["a0".into()])
        .unwrap();
    assert!(
        error(c.batch_restage(I, G, NOW, "batch-1", "other", Vec::new()))
            .contains("already re-staged differently")
    );
    assert_eq!(phase(&c, "a0"), Phase::Handoff);
    assert_eq!(phase(&c, "a1"), Phase::Handoff);
    // The red result never moves to done or follows the members.
    assert!(c.batch_ledger().unwrap().unwrap().batch_of("a1").is_none());
    let status = c.snapshot(NOW).unwrap().batches.unwrap();
    assert_eq!(status.staging.as_ref().unwrap().held, ["a0"]);
    assert!(status.active.is_empty());
    // The held suspect waits for its review return; the rest re-stage on main.
    assert!(
        error(c.batch_open_staged(
            I,
            G,
            NOW,
            "batch-2",
            members(&["a0", "a1"]),
            LIMITS,
            staging()
        ))
        .contains("held as a suspect")
    );
    let again = open(&c, "batch-2", &["a1", "a2"], NOW + 30);
    let again = again.staging.unwrap();
    assert_eq!(again.parent, None);
    assert_eq!(again.base_commit, MAIN);
}

#[test]
fn hs2_gt57bq_stacked_batches_green_tip_implies_ancestors_and_red_invalidates_above() {
    let (_root, c) = pool(6, IntegrationStrategy::CiStaged);
    stage(&c, "batch-1", &["a0"], S1, NOW);
    // The next batch stacks on the pending tip while batch-1 is still in CI.
    let stacked = open(&c, "batch-2", &["a1"], NOW + 1).staging.unwrap();
    assert_eq!(stacked.parent.as_deref(), Some("batch-1"));
    assert_eq!(stacked.base_commit, S1);
    c.batch_staged(
        I,
        G,
        NOW + 2,
        "batch-2",
        S2,
        shipping(&["a1"], S2),
        vec!["cargo lint".into()],
    )
    .unwrap();
    // A parent cannot be promoted ahead of being green, nor a child ahead of its parent.
    let green = result(&c, "batch-2", "701", BatchConclusion::Success, NOW + 10);
    assert_eq!(green.batch.state, BatchState::Green);
    let parent = batch(&c, "batch-1");
    assert_eq!(parent.state, BatchState::Green);
    assert_eq!(
        parent.staging.unwrap().implied_by.as_deref(),
        Some("batch-2")
    );
    // The parent's own later green run is accepted without change.
    result(&c, "batch-1", "700", BatchConclusion::Success, NOW + 11);
    let promoted = c.batch_promote(I, G, NOW + 20, "batch-2", S2).unwrap();
    assert_eq!(promoted.state, BatchState::Promoted);
    let parent = batch(&c, "batch-1");
    assert_eq!(parent.state, BatchState::Promoted);
    assert_eq!(parent.main_commit.as_deref(), Some(S2));

    // A red batch invalidates every batch stacked above it, pushed or still integrating.
    stage(&c, "batch-3", &["a2"], S3, NOW + 30);
    stage(&c, "batch-4", &["a3"], S4, NOW + 31);
    let integrating = open(&c, "batch-5", &["a4"], NOW + 32).staging.unwrap();
    assert_eq!(integrating.parent.as_deref(), Some("batch-4"));
    assert_eq!(integrating.base_commit, S4);
    let red = result(&c, "batch-3", "703", BatchConclusion::Failure, NOW + 40);
    assert_eq!(red.batch.state, BatchState::Red);
    for (id, member) in [("batch-4", "a3"), ("batch-5", "a4")] {
        let invalid = batch(&c, id);
        assert_eq!(invalid.state, BatchState::Invalidated, "{id}");
        assert_eq!(
            invalid.staging.unwrap().invalidated_by.as_deref(),
            Some("batch-3")
        );
        assert_eq!(phase(&c, member), Phase::Handoff);
    }
    // A late result for an invalidated batch is refused; it can never reach main.
    assert!(
        error(c.batch_result(
            I,
            G,
            NOW,
            "batch-4",
            "704",
            BatchConclusion::Success,
            LIMITS
        ))
        .contains("invalidated")
    );
    assert!(error(c.batch_promote(I, G, NOW, "batch-4", S4)).contains("invalidated"));
    // A rerun that turns the red batch green is accepted before it is re-staged.
    let rerun = result(&c, "batch-3", "705", BatchConclusion::Success, NOW + 50);
    assert_eq!(rerun.batch.state, BatchState::Green);
    assert_eq!(rerun.breaker.consecutive_red, 0);
}

#[test]
fn hs2_gt57bq_stale_green_batch_restages_with_its_stack_and_holds_nothing() {
    let (_root, c) = pool(3, IntegrationStrategy::CiStaged);
    stage(&c, "batch-1", &["a0"], S1, NOW);
    stage(&c, "batch-2", &["a1"], S2, NOW + 1);
    result(&c, "batch-1", "801", BatchConclusion::Success, NOW + 10);
    // Main moved while batch-1 waited: promotion is refused, so the batch is re-staged.
    assert!(error(c.batch_promote(I, G, NOW, "batch-1", MOVED_MAIN)).contains("exactly"));
    assert!(
        error(c.batch_restage(I, G, NOW, "batch-1", "main moved", vec!["a0".into()]))
            .contains("only a red batch holds suspects")
    );
    let restaged = c
        .batch_restage(I, G, NOW + 20, "batch-1", "main moved", Vec::new())
        .unwrap();
    assert_eq!(restaged.state, BatchState::Restaged);
    assert_eq!(batch(&c, "batch-2").state, BatchState::Invalidated);
    assert_eq!(phase(&c, "a0"), Phase::Handoff);
    assert_eq!(phase(&c, "a1"), Phase::Handoff);
    assert!(error(c.batch_restage(I, G, NOW, "batch-2", "x", Vec::new())).contains("invalidated"));
    // Both re-stage together on the new main.
    let rebuilt = c
        .batch_open_staged(
            I,
            G,
            NOW + 30,
            "batch-3",
            members(&["a0", "a1"]),
            LIMITS,
            StagingOpen {
                branch: "integration".into(),
                main_commit: MOVED_MAIN.into(),
            },
        )
        .unwrap();
    assert_eq!(rebuilt.staging.unwrap().base_commit, MOVED_MAIN);
    // An unpushed staged batch is aborted like any batch, and its members return.
    c.batch_abort(I, G, NOW + 31, "batch-3", "smoke failed")
        .unwrap();
    assert_eq!(phase(&c, "a0"), Phase::Handoff);
}

#[test]
fn hs2_gt57bq_breaker_counts_staged_reds_without_red_main_and_recovers() {
    let (_root, c) = pool(3, IntegrationStrategy::CiStaged);
    for (index, (batch_id, sha)) in [("batch-1", S1), ("batch-2", S2)].into_iter().enumerate() {
        let now = NOW + 100 * index as i64;
        stage(&c, batch_id, &["a0"], sha, now);
        let red = result(
            &c,
            batch_id,
            &format!("90{index}"),
            BatchConclusion::Failure,
            now + 1,
        );
        if index == 0 {
            assert!(red.notify.is_none());
            c.batch_follow_ups(I, G, now, batch_id, vec![failure("k1")])
                .unwrap();
            c.batch_restage(I, G, now + 2, batch_id, "red", Vec::new())
                .unwrap();
        } else {
            let notify = red.notify.unwrap();
            assert!(
                notify.contains("ci-staged integration fell back to serial"),
                "{notify}"
            );
            assert!(notify.contains("Main is unchanged"), "{notify}");
        }
    }
    let status = c.snapshot(NOW).unwrap().batches.unwrap();
    assert_eq!(status.effective_strategy, IntegrationStrategy::Serial);
    assert_eq!(status.breaker.last_red_main, None);
    // Re-staging stays possible while tripped; opening a new batch does not.
    c.batch_follow_ups(I, G, NOW, "batch-2", vec![failure("k2")])
        .unwrap();
    c.batch_restage(I, G, NOW + 300, "batch-2", "red", Vec::new())
        .unwrap();
    assert!(
        error(c.batch_open_staged(
            I,
            G,
            NOW + 301,
            "batch-3",
            members(&["a1"]),
            LIMITS,
            staging()
        ))
        .contains("circuit breaker is tripped")
    );
    c.batch_recover(I, G, NOW + 400, "990", MAIN).unwrap();
    assert_eq!(
        c.snapshot(NOW).unwrap().batches.unwrap().effective_strategy,
        IntegrationStrategy::CiStaged
    );
    open(&c, "batch-3", &["a1"], NOW + 401);
}

#[test]
fn hs2_gt57bq_staged_commands_are_gated_by_strategy() {
    // A batch integrator neither stages nor promotes.
    let (_root, c) = pool(1, IntegrationStrategy::Batch);
    assert!(
        error(c.batch_open_staged(I, G, NOW, "batch-1", members(&["a0"]), LIMITS, staging()))
            .contains("only a ci-staged integrator")
    );
    c.batch_open(I, G, NOW, "batch-1", members(&["a0"]), LIMITS)
        .unwrap();
    assert!(
        error(c.batch_staged(
            I,
            G,
            NOW,
            "batch-1",
            S1,
            shipping(&["a0"], S1),
            vec!["x".into()]
        ))
        .contains("not a staged batch")
    );
    assert!(error(c.batch_promote(I, G, NOW, "batch-1", S1)).contains("not a staged batch"));
    assert!(
        error(c.batch_restage(I, G, NOW, "batch-1", "x", Vec::new()))
            .contains("not a staged batch")
    );
    // A ci-staged integrator must stage: the plain open lacks the staging branch.
    let (_root, c) = pool(1, IntegrationStrategy::CiStaged);
    assert!(
        error(c.batch_open(I, G, NOW, "batch-1", members(&["a0"]), LIMITS))
            .contains("staging branch")
    );
    assert!(
        error(c.batch_open_staged(
            I,
            G,
            NOW,
            "batch-1",
            members(&["a0"]),
            LIMITS,
            StagingOpen {
                branch: "main".into(),
                main_commit: MAIN.into()
            }
        ))
        .contains("must not be main")
    );
    // A serial session runs none of it.
    let (_root, c) = pool(1, IntegrationStrategy::Serial);
    assert!(
        error(c.batch_open_staged(I, G, NOW, "batch-1", members(&["a0"]), LIMITS, staging()))
            .contains("batch strategy")
    );
}

#[test]
fn hs2_gt57bq_integrator_restart_mid_stage_resumes_from_the_ledger() {
    let (_root, c) = pool(2, IntegrationStrategy::CiStaged);
    stage(&c, "batch-1", &["a0"], S1, NOW);
    open(&c, "batch-2", &["a1"], NOW + 1);
    // The integrator process dies; a new session generation picks up the same ledger.
    c.leave(Role::Integrator, I, G).unwrap();
    register_integrator(&c, "generation-2", IntegrationStrategy::CiStaged);
    assert!(error(c.batch_promote(I, G, NOW, "batch-1", S1)).contains("exact integrator session"));
    let status = c.snapshot(NOW).unwrap().batches.unwrap();
    let summary = status.staging.unwrap();
    assert_eq!(summary.in_ci, ["batch-1"]);
    assert_eq!(summary.waiting_tickets, ["HS-1"]);
    // The interrupted open replays; its staged push and the CI result complete normally.
    c.batch_open_staged(
        I,
        "generation-2",
        NOW + 2,
        "batch-2",
        members(&["a1"]),
        LIMITS,
        staging(),
    )
    .unwrap();
    c.batch_staged(
        I,
        "generation-2",
        NOW + 3,
        "batch-2",
        S2,
        shipping(&["a1"], S2),
        vec!["cargo lint".into()],
    )
    .unwrap();
    c.batch_result(
        I,
        "generation-2",
        NOW + 10,
        "batch-2",
        "1001",
        BatchConclusion::Success,
        LIMITS,
    )
    .unwrap();
    c.batch_promote(I, "generation-2", NOW + 11, "batch-2", S2)
        .unwrap();
    c.mark(I, "generation-2", NOW, "a0", Phase::Done).unwrap();
    c.mark(I, "generation-2", NOW, "a1", Phase::Done).unwrap();
}

#[test]
fn hs2_gt57bq_ledger_version_one_cannot_hold_staged_batches() {
    let (_root, c) = pool(1, IntegrationStrategy::CiStaged);
    stage(&c, "batch-1", &["a0"], S1, NOW);
    let path = c.directory.join(LEDGER_FILE);
    let mut ledger: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for (version, message) in [(1, "cannot hold staged batches"), (3, "not supported")] {
        ledger["version"] = version.into();
        std::fs::write(&path, serde_json::to_vec(&ledger).unwrap()).unwrap();
        assert!(error(c.batch_ledger()).contains(message), "{version}");
    }
}
