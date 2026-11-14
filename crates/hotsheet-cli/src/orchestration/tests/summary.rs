//! HS2-S13719: the progress and cost summary through the shared runtime, against real
//! git ticket stores and their usage records. Synthetic ids and amounts only.
use hotsheet_model::Ticket;
use hotsheet_ticketing::metrics::{self, UsageEvent};
use hotsheet_ticketing::orchestration::{
    Metric, ProgressSummary, RunState, RunTicket, TargetAction, UnbindTarget, UnknownReason,
};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::{Value, json};

use super::tickets::{Machine, NOW, bind_and_allow, machine, unsupported, worker};
use super::*;

fn target_ticket(m: &Machine) -> Ticket {
    ops::create(
        &m.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: "Target work".into(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn dispatch(
    m: &Machine,
    orchestration: &Ticket,
    target: u128,
    native: &Ticket,
    operation: &str,
) -> DispatchRun {
    serde_json::from_value(json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
        "target_id": id(target), "connection_id": m.target_connection(),
        "native_id": native.id.to_string(), "operation_id": operation, "worker_id": "worker-a",
    }))
    .unwrap()
}

fn usage(ticket: Option<&Ticket>, ts: &str, cost_usd: Option<f64>) -> UsageEvent {
    UsageEvent {
        ts: ts.into(),
        tool: "claude".into(),
        model: Some("model-a".into()),
        tokens_in: 10,
        tokens_out: 2,
        cost_usd,
        ticket: ticket.map(|ticket| ticket.id.to_string()),
        session: Some("session-a".into()),
    }
}

fn unbind(m: &Machine, target: u128, tx: &mut Transaction<'_>) {
    let binding = OrchestrationRuntime::binding_at(&m.home, &m.reference(target)).unwrap();
    OrchestrationRuntime::unbind_at(
        &m.home,
        UnbindTarget {
            target: m.reference(target),
            expected_binding_revision: binding.revision,
            previous_destination: binding.destination,
        },
        tx,
    )
    .unwrap();
}

fn cost_reason(summary: &ProgressSummary) -> Option<UnknownReason> {
    summary.work_items[0].cost.reason()
}

#[test]
fn hs2_s13719_summary_reads_runs_and_usage_once_per_work_item_across_targets() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let read =
        |tx: &mut Transaction<'_>| m.runtime.progress_summary(&m.owner.id, None, tx).unwrap();
    // Nothing dispatched: an empty summary whose cost total is complete because it
    // covers no work, with no run to be unknown about.
    let empty = read(&mut tx);
    assert!(empty.runs.is_empty() && empty.targets.is_empty());
    assert_eq!(empty.as_of_ms, 200);
    assert_eq!(empty.totals.cost.items_unknown, 0);

    // Two declared targets that are the same checkout, so they share a ticket source.
    for target in [1, 2] {
        bind_and_allow(
            &m,
            target,
            &[TargetAction::TicketsRead, TargetAction::WorkStart],
            &mut tx,
        );
    }
    let orchestration = m.orchestration_ticket("Coordinate");
    let work = target_ticket(&m);
    let first = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(&m, &orchestration, 1, &work, "op-1"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    let summary = read(&mut tx);
    assert_eq!(summary.totals.runs, 1);
    assert_eq!(summary.runs[0].run_id, first.run_id);
    assert_eq!(summary.runs[0].state, RunState::Dispatched);
    assert_eq!(
        summary.runs[0].lead_time_ms.reason(),
        Some(UnknownReason::RunUnfinished)
    );
    assert_eq!(summary.runs[0].queue_wait_ms, Metric::Known { value: 0 });
    assert_eq!(
        summary.runs[0].ci.reason(),
        Some(UnknownReason::NoGateLinked)
    );
    // No usage is recorded for the ticket: unknown, never a cost of zero.
    assert_eq!(cost_reason(&summary), Some(UnknownReason::NoUsageRecorded));
    assert_eq!(summary.totals.cost.known_usd, 0.0);
    assert!(!summary.totals.cost.complete);
    assert_eq!(summary.totals.cost.unknown_reasons["no_usage_recorded"], 1);

    // The usage records of docs/14, in the target's store: two events of this ticket
    // (one without a cost), one of another ticket and one unattributed.
    let other = target_ticket(&m);
    for event in [
        usage(Some(&work), "2026-10-15T01:00:00Z", Some(0.75)),
        usage(Some(&work), "2026-10-15T02:00:00Z", None),
        usage(Some(&other), "2026-10-15T03:00:00Z", Some(5.0)),
        usage(None, "2026-10-15T04:00:00Z", Some(7.0)),
    ] {
        metrics::record(&m.target_store, &event).unwrap();
    }
    let summary = read(&mut tx);
    let cost = summary.work_items[0].cost.known().unwrap();
    assert_eq!(
        (cost.cost_usd, cost.events, cost.unpriced_events),
        (0.75, 2, 1)
    );
    assert!(
        !summary.totals.cost.complete,
        "an unpriced event is a lower bound"
    );

    // The run ends and the same target ticket is dispatched again through the other
    // target: one work item, two runs, two targets, and its cost counted once.
    let cancelled = m
        .runtime
        .run_cancel(&m.owner.id, &first.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(cancelled.run.state, RunState::Cancelled);
    let second = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(&m, &orchestration, 2, &work, "op-2"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    let summary = read(&mut tx);
    assert_eq!((summary.totals.runs, summary.totals.work_items), (2, 1));
    assert_eq!(summary.totals.retries.redispatches, 1);
    assert_eq!(summary.runs[0].lead_time_ms, Metric::Known { value: 0 });
    let item = &summary.work_items[0];
    assert_eq!(item.run_ids, [first.run_id.clone(), second.run_id.clone()]);
    assert_eq!(item.attributed_target_id, id(1));
    assert_eq!(item.target_ids, [id(1), id(2)]);
    assert_eq!(summary.totals.cost.known_usd, 0.75);
    assert_eq!(
        summary
            .targets
            .iter()
            .map(|target| (target.figures.runs, target.figures.cost.items_known))
            .collect::<Vec<_>>(),
        [(1, 1), (1, 0)]
    );
    // Identifiers and metrics only: no ticket text reaches the answer.
    let text = serde_json::to_string(&summary).unwrap();
    assert!(!text.contains("Target work") && !text.contains("Coordinate"));

    // Narrowed to an orchestration ticket: its runs, or none for another ticket.
    let own = RunTicket {
        connection_id: String::new(),
        native_id: orchestration.id.to_string(),
    };
    assert_eq!(
        m.runtime
            .progress_summary(&m.owner.id, Some(&own), &mut tx)
            .unwrap(),
        summary
    );
    let elsewhere = m.orchestration_ticket("Other coordination");
    let path = format!("/checkouts/{}/orchestration/summary", m.owner.id);
    let narrowed = m
        .runtime
        .request(
            "GET",
            &format!(
                "{path}?ticket={}&connection_id={}",
                elsewhere.id,
                m.owner_connection()
            ),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(narrowed["totals"]["runs"], 0);
    assert_eq!(narrowed["runs"], json!([]));
    // The route answers what the runtime answers.
    assert_eq!(
        m.runtime
            .request("GET", &path, &Value::Null, &mut tx)
            .unwrap(),
        serde_json::to_value(&summary).unwrap()
    );
    let wire = m
        .runtime
        .request("GET", &path, &Value::Null, &mut tx)
        .unwrap();
    assert_eq!(wire["attribution"], "work_item_earliest_run");
    assert_eq!(
        wire["work_items"][0]["cost"],
        json!({"status":"known","value":{"cost_usd":0.75,"events":2,"unpriced_events":1}})
    );
    assert_eq!(
        wire["runs"][1]["lead_time_ms"],
        json!({"status":"unknown","reason":"run_unfinished"})
    );
    assert!(is_path(&path) && is_machine_path(&path));
    for bad in [
        "?ticket=a/b",
        "?ticket=x&ticket=y",
        "?ticket=ok&connection_id=has%20space",
    ] {
        assert!(
            matches!(
                m.runtime
                    .request("GET", &format!("{path}{bad}"), &Value::Null, &mut tx),
                Err(OrchestrationError::Invalid(_))
            ),
            "{bad}"
        );
    }
    assert!(matches!(
        m.runtime.request("POST", &path, &json!({}), &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));

    // The first target loses its destination: the item is still read through the
    // other target that reaches it. With neither bound, its cost is unknown.
    unbind(&m, 1, &mut tx);
    let summary = read(&mut tx);
    assert_eq!(summary.work_items[0].cost.known().unwrap().cost_usd, 0.75);
    unbind(&m, 2, &mut tx);
    let summary = read(&mut tx);
    assert_eq!(cost_reason(&summary), Some(UnknownReason::TargetUnbound));
    assert_eq!(summary.totals.cost.known_usd, 0.0);
    assert_eq!(summary.totals.cost.items_unknown, 1);
    // Reading changed nothing: the same question has the same answer.
    assert_eq!(read(&mut tx), summary);
    // An owner with no records has an empty summary rather than an error.
    assert!(
        m.runtime
            .progress_summary(&m.target.id, None, &mut tx)
            .unwrap()
            .runs
            .is_empty()
    );
}
