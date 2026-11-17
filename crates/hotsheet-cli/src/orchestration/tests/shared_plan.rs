//! HS2-YDY5HJ: the checked-in plan through the shared runtime and its router. The
//! definitions land in the orchestration project's `.hotsheet2/orchestration-plan.json`;
//! a second machine with its own home reads the same steps and computes readiness from
//! its own reads alone; the step revision refuses a stale change; and a plan kept on
//! this machine before it was checked in is migrated once. Synthetic tickets only.
use hotsheet_ticketing::orchestration::{ReadinessLevel, SHARED_PLAN_PATH, TargetAction};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::{Value, json};

use super::tickets::{Machine, NOW, bind_and_allow, machine, unsupported};
use super::*;

fn target_ticket(m: &Machine, title: &str) -> hotsheet_model::Ticket {
    ops::create(
        &m.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

/// The same project on a second machine: its own home, the same checkouts.
fn second_machine(m: &Machine) -> (tempfile::TempDir, OrchestrationRuntime) {
    let other = tempfile::tempdir().unwrap();
    std::fs::copy(
        m.home.join("checkouts.json"),
        other.path().join("checkouts.json"),
    )
    .unwrap();
    let runtime = OrchestrationRuntime::new(
        other.path().to_path_buf(),
        CheckoutRegistry::new(other.path().join("checkouts.json")),
        ProviderConfigRegistry::new(other.path().join("providers.json")),
        Arc::new(Memory::default()),
        Arc::new(Peer::default()),
        id(11),
    )
    .unwrap();
    (other, runtime)
}

fn request(
    runtime: &OrchestrationRuntime,
    owner: &str,
    operation: &str,
    body: Value,
) -> Result<Value, OrchestrationError> {
    runtime.request_now(
        "POST",
        &format!("/checkouts/{owner}/orchestration/readiness/{operation}"),
        &body,
    )
}

#[test]
fn hs2_ydy5hj_the_plan_is_checked_in_and_a_second_machine_reads_it_from_its_own_records() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::TicketsRead], &mut tx);
    let goal = m.orchestration_ticket("Coordinate the synthetic release");
    let work = target_ticket(&m, "Synthetic work");
    let owner = m.owner.id.clone();
    let actor = json!({"role": "ai", "id": "worker-1"});
    let body = |expected: Option<u32>| {
        json!({"ticket": {"connection_id": m.owner_connection(), "native_id": goal.id.to_string()},
            "target_id": id(1), "connection_id": m.target_connection(),
            "native_id": work.id.to_string(), "expected_revision": expected, "actor": actor})
    };
    let defined = request(&m.runtime, &owner, "define", body(Some(0))).unwrap();
    let step_id = defined["step"]["step_id"].as_str().unwrap().to_owned();
    assert_eq!(defined["level"], "confirmed");
    // The definition is checked in with the manifest; this machine's ids and reads
    // are not.
    let checked_in =
        std::fs::read_to_string(Path::new(&m.owner.root).join(SHARED_PLAN_PATH)).unwrap();
    assert!(checked_in.contains(&step_id) && !checked_in.contains(&owner));
    assert!(!checked_in.contains("not_started"));
    let plan = request(&m.runtime, &owner, "plan", json!({})).unwrap();
    let revision = plan["revision"].as_str().unwrap().to_owned();
    assert_eq!(plan["counts"], json!({"confirmed": 1}));

    // A stale step revision is refused with a typed conflict, and changes nothing.
    let refused = request(&m.runtime, &owner, "define", body(Some(0))).unwrap_err();
    assert_eq!(refused.code(), "plan_conflict");
    assert_eq!(error_status(&refused), 409);
    assert_eq!(
        request(&m.runtime, &owner, "plan", json!({})).unwrap()["revision"],
        revision
    );

    // The second machine plans the same step, at a level from its own reads only:
    // it never read the ticket, so the step is stale there.
    let (_other, second) = second_machine(&m);
    let there = request(&second, &owner, "plan", json!({})).unwrap();
    assert_eq!(there["revision"], revision);
    assert_eq!(there["steps"][0]["step"]["step_id"], step_id);
    assert_eq!(there["steps"][0]["level"], "stale");
    let mut rng = Rng(300);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    assert!(second.ready_work(&owner, &mut tx).unwrap().is_empty());
    assert_eq!(
        m.runtime.readiness_plan(&owner, &mut tx).unwrap().steps[0]
            .readiness
            .level,
        ReadinessLevel::Confirmed
    );
    // A removal on the second machine is the plan on both.
    request(
        &second,
        &owner,
        "remove",
        json!({"step_id": step_id, "actor": {"role": "human", "id": "maintainer"}}),
    )
    .unwrap();
    assert!(
        m.runtime
            .readiness_plan(&owner, &mut tx)
            .unwrap()
            .steps
            .is_empty()
    );
}

#[test]
fn hs2_ydy5hj_a_plan_kept_on_this_machine_is_migrated_once_and_a_discard_is_a_persons() {
    let m = machine(10, false, unsupported);
    let owner = m.owner.id.clone();
    let step = |number: u128, native: &str| {
        json!({"step": {"step_id": id(number), "owner_checkout_id": owner,
            "ticket": {"connection_id": m.owner_connection(), "native_id": id(900)},
            "target": {"target_id": id(1), "connection_id": m.target_connection(), "native_id": native},
            "revision": 1, "created_at_ms": 5, "updated_at_ms": 5},
            "published": {"level": "blocked", "since_ms": 5, "seq": 1}})
    };
    std::fs::write(
        m.home.join("orchestration-plan.json"),
        serde_json::to_vec(
            &json!({"schema_version": 1, "steps": [step(1, "a"), step(2, "b")],
            "tickets": []}),
        )
        .unwrap(),
    )
    .unwrap();
    let ai = json!({"role": "ai", "id": "worker-1"});
    let moved = request(&m.runtime, &owner, "migrate", json!({"actor": ai})).unwrap();
    assert_eq!(moved["migrated"], json!([id(1), id(2)]));
    let again = request(&m.runtime, &owner, "migrate", json!({"actor": ai})).unwrap();
    assert_eq!(again["migrated"], json!([]));
    assert_eq!(again["revision"], moved["revision"]);
    // The levels this machine published stayed with it.
    let plan = request(&m.runtime, &owner, "plan", json!({})).unwrap();
    assert_eq!(plan["steps"].as_array().unwrap().len(), 2);
    // Only a person may discard local definitions.
    let refused = request(
        &m.runtime,
        &owner,
        "migrate",
        json!({"discard_local": true, "actor": ai}),
    )
    .unwrap_err();
    assert_eq!(refused.code(), "forbidden");
    request(
        &m.runtime,
        &owner,
        "migrate",
        json!({"discard_local": true, "actor": {"role": "human", "id": "maintainer"}}),
    )
    .unwrap();
}

/// HS2-F1BKQ0: where every plan read answers `plan_conflict`, `health` names the
/// conflicting steps through the shared router, and after a person's discard the plan
/// reads again.
#[test]
fn hs2_f1bkq0_health_reads_a_conflict_the_plan_refuses_and_clears_after_a_discard() {
    let m = machine(10, false, unsupported);
    let owner = m.owner.id.clone();
    let step = |number: u128, native: &str, tool: Option<&str>| {
        json!({"step": {"step_id": id(number), "owner_checkout_id": owner,
            "ticket": {"connection_id": m.owner_connection(), "native_id": id(900)},
            "target": {"target_id": id(1), "connection_id": m.target_connection(), "native_id": native},
            "tool": tool, "revision": 1, "created_at_ms": 5, "updated_at_ms": 5}})
    };
    let local = |steps: Vec<serde_json::Value>| {
        std::fs::write(
            m.home.join("orchestration-plan.json"),
            serde_json::to_vec(&json!({"schema_version": 1, "steps": steps, "tickets": []}))
                .unwrap(),
        )
        .unwrap();
    };
    assert_eq!(
        request(&m.runtime, &owner, "health", json!({})).unwrap()["state"],
        "ok"
    );
    local(vec![step(1, "a", None)]);
    let ai = json!({"role": "ai", "id": "worker-1"});
    request(&m.runtime, &owner, "migrate", json!({"actor": ai})).unwrap();
    // This machine kept step 1 differently: every read refuses; health names it.
    local(vec![step(1, "a", Some("codex"))]);
    let refused = request(&m.runtime, &owner, "plan", json!({})).unwrap_err();
    assert_eq!(refused.code(), "plan_conflict");
    let health = request(&m.runtime, &owner, "health", json!({})).unwrap();
    assert_eq!(
        (
            &health["state"],
            &health["conflicts"],
            &health["local"],
            &health["discard_resolves"]
        ),
        (
            &json!("conflict"),
            &json!([id(1)]),
            &json!([id(1)]),
            &json!(true)
        )
    );
    request(
        &m.runtime,
        &owner,
        "migrate",
        json!({"discard_local": true, "actor": {"role": "human", "id": "maintainer"}}),
    )
    .unwrap();
    assert_eq!(
        request(&m.runtime, &owner, "health", json!({})).unwrap()["state"],
        "ok"
    );
    assert_eq!(
        request(&m.runtime, &owner, "plan", json!({})).unwrap()["steps"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
