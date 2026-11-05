//! Integration strategy transitions across the schema-eight upgrade (HS2-NEY1QH).

use super::tests::{NOW, SHA, assignment, fixture, register};
use super::worker_view::WorkerView;
use super::*;

fn batch(upgraded: bool) -> StrategyRequest {
    StrategyRequest {
        requested: Some(IntegrationStrategy::Batch),
        default: IntegrationStrategy::Serial,
        all_participants_upgraded: upgraded,
    }
}

fn register_integrator(
    c: &Coordinator,
    generation: &str,
    now: i64,
    request: StrategyRequest,
) -> Result<()> {
    c.register_with_strategy(
        Role::Integrator,
        "integrator",
        generation,
        now,
        120,
        ReturnRoute::HumanRelay,
        request,
    )
}

fn state_json(c: &Coordinator) -> serde_json::Value {
    serde_json::from_slice(&fs::read(c.directory.join("state.json")).unwrap()).unwrap()
}

#[test]
fn hs2_ney1qh_serial_registration_keeps_the_pool_schema_and_bytes_unchanged() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    let explicit_serial = StrategyRequest {
        requested: Some(IntegrationStrategy::Serial),
        ..StrategyRequest::default()
    };
    register_integrator(&c, "generation-1", NOW + 1, explicit_serial).unwrap();
    let state = c.state().unwrap();
    assert_eq!(state.version, 3);
    assert_eq!(
        state.integrator.as_ref().unwrap().strategy,
        IntegrationStrategy::Serial
    );
    // Serial is skipped on disk, so a schema-three artifact reads exactly what it wrote.
    let raw = state_json(&c);
    assert!(raw["integrator"].get("strategy").is_none(), "{raw}");
    let snapshot = c.snapshot(NOW + 1).unwrap();
    assert_eq!(
        snapshot.integration_strategy,
        Some(IntegrationStrategy::Serial)
    );
}

#[test]
fn hs2_ney1qh_non_serial_strategy_requires_an_explicit_upgrade_and_persists_schema_eight() {
    let (_root, c) = fixture();
    let before = fs::read(c.directory.join("state.json")).unwrap();
    let refused = register_integrator(&c, "generation-1", NOW, batch(false))
        .unwrap_err()
        .to_string();
    assert!(refused.contains("--all-participants-upgraded"), "{refused}");
    // A refused registration changes nothing: no session, no schema bump.
    assert_eq!(fs::read(c.directory.join("state.json")).unwrap(), before);
    register_integrator(&c, "generation-1", NOW, batch(true)).unwrap();
    let state = c.state().unwrap();
    assert_eq!(state.version, 8);
    assert_eq!(state_json(&c)["integrator"]["strategy"], "batch");
    assert_eq!(
        c.snapshot(NOW).unwrap().integration_strategy,
        Some(IntegrationStrategy::Batch)
    );
}

#[test]
fn hs2_ney1qh_strategy_is_fixed_per_generation_and_renewal_keeps_it() {
    let (_root, c) = fixture();
    register_integrator(&c, "generation-1", NOW, batch(true)).unwrap();
    // Plain renewals (as `next` performs while waiting) keep the recorded strategy.
    register(&c, Role::Integrator, "integrator", "generation-1", NOW + 30);
    register_integrator(&c, "generation-1", NOW + 31, batch(false)).unwrap();
    assert_eq!(
        Coordinator::active_strategy(&c.state().unwrap()),
        IntegrationStrategy::Batch
    );
    let changed = register_integrator(
        &c,
        "generation-1",
        NOW + 32,
        StrategyRequest {
            requested: Some(IntegrationStrategy::Serial),
            ..StrategyRequest::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(changed.contains("register a new session"), "{changed}");
    // A live session cannot be replaced by another generation...
    assert!(register_integrator(&c, "generation-2", NOW + 33, StrategyRequest::default()).is_err());
    // ...but after expiry a new generation resolves its own strategy; the schema never lowers.
    register_integrator(&c, "generation-2", NOW + 1_000, StrategyRequest::default()).unwrap();
    let state = c.state().unwrap();
    assert_eq!(state.version, 8);
    assert_eq!(
        Coordinator::active_strategy(&state),
        IntegrationStrategy::Serial
    );
    assert!(state_json(&c)["integrator"].get("strategy").is_none());
    // A project default other than serial applies to a new generation.
    c.leave(Role::Integrator, "integrator", "generation-2")
        .unwrap();
    register_integrator(
        &c,
        "generation-3",
        NOW + 1_001,
        StrategyRequest {
            requested: None,
            default: IntegrationStrategy::CiStaged,
            all_participants_upgraded: false,
        },
    )
    .unwrap();
    assert_eq!(
        Coordinator::active_strategy(&c.state().unwrap()),
        IntegrationStrategy::CiStaged
    );
}

#[test]
fn hs2_ney1qh_only_the_integrator_selects_a_strategy() {
    let (_root, c) = fixture();
    for role in [Role::Worker, Role::Dispatcher] {
        let refused = c
            .register_with_strategy(
                role,
                "other",
                "generation-1",
                NOW,
                120,
                ReturnRoute::HumanRelay,
                batch(true),
            )
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("only the integrator") || refused.contains("assignment authority"),
            "{refused}"
        );
    }
    assert_eq!(c.state().unwrap().version, 3);
}

#[test]
fn hs2_ney1qh_schema_fence_refuses_a_strategy_recorded_below_schema_eight() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    let mut raw = state_json(&c);
    raw["version"] = 7.into();
    raw["integrator"]["strategy"] = "batch".into();
    fs::write(
        c.directory.join("state.json"),
        serde_json::to_vec_pretty(&raw).unwrap(),
    )
    .unwrap();
    let refused = c.state().unwrap_err().to_string();
    assert!(refused.contains("schema eight"), "{refused}");
    // Newer than this executable is refused too, as every older executable refuses eight.
    raw["version"] = 10.into();
    fs::write(
        c.directory.join("state.json"),
        serde_json::to_vec_pretty(&raw).unwrap(),
    )
    .unwrap();
    assert!(c.state().is_err());
}

#[test]
fn hs2_ney1qh_older_schema_seven_pool_reads_as_serial_and_is_not_bumped() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    let mut raw = state_json(&c);
    raw["version"] = 7.into();
    fs::write(
        c.directory.join("state.json"),
        serde_json::to_vec_pretty(&raw).unwrap(),
    )
    .unwrap();
    register(&c, Role::Integrator, "integrator", "generation-1", NOW + 10);
    register(&c, Role::Worker, "worker", "generation-1", NOW + 10);
    let state = c.state().unwrap();
    assert_eq!(state.version, 7);
    assert_eq!(
        Coordinator::active_strategy(&state),
        IntegrationStrategy::Serial
    );
    assert!(
        CAPABILITIES.schemas_read.contains(&8)
            && CAPABILITIES.schema_written_with_integration_strategy == 8
    );
}

#[test]
fn hs2_ney1qh_workers_and_serial_rules_keep_working_on_a_schema_eight_pool() {
    let (_root, c) = fixture();
    register_integrator(&c, "generation-1", NOW, batch(true)).unwrap();
    register(&c, Role::Worker, "worker", "generation-1", NOW);
    register(&c, Role::Worker, "second", "generation-1", NOW);
    c.assign(
        "integrator",
        "generation-1",
        NOW,
        assignment("first", "worker", "one"),
    )
    .unwrap();
    let mut second = assignment("second", "second", "two");
    second.slug = "HS-2".into();
    c.assign("integrator", "generation-1", NOW, second).unwrap();
    for (worker, id) in [("worker", "first"), ("second", "second")] {
        c.worker_transition(worker, "generation-1", id, Phase::Accepted, None)
            .unwrap();
        c.worker_transition(worker, "generation-1", id, Phase::Handoff, Some(SHA.into()))
            .unwrap();
    }
    c.mark(
        "integrator",
        "generation-1",
        NOW,
        "first",
        Phase::Integrating,
    )
    .unwrap();
    // Batch is unimplemented, so it keeps serial's exclusive integration rule.
    let serialized = c
        .mark(
            "integrator",
            "generation-1",
            NOW,
            "second",
            Phase::Integrating,
        )
        .unwrap_err()
        .to_string();
    assert!(
        serialized.contains("serialize integrator work"),
        "{serialized}"
    );
    let snapshot = c.snapshot(NOW).unwrap();
    let view = WorkerView::from_snapshot(&snapshot, NOW, &Default::default(), false);
    assert_eq!(view.integration_strategy, Some(IntegrationStrategy::Batch));
    let rows: Vec<_> = view
        .workers
        .iter()
        .map(|row| (row.id.as_str(), row.integration_strategy))
        .collect();
    assert!(rows.contains(&("integrator", Some(IntegrationStrategy::Batch))));
    assert!(rows.contains(&("worker", None)));
    // A disconnected integrator keeps its recorded row strategy but is not active.
    let later = c.snapshot(NOW + 1_000).unwrap();
    assert_eq!(later.integration_strategy, None);
    let view = WorkerView::from_snapshot(&later, NOW + 1_000, &Default::default(), false);
    assert_eq!(view.integration_strategy, None);
}
