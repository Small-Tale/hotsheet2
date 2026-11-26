//! HS2-EBHDSB: dispatch runs through the shared runtime against real git ticket
//! stores: direct claims, dispatcher queueing, duplicates, takeover and cancellation.
use hotsheet_model::{NoteKind, Status, Ticket};
use hotsheet_ticketing::orchestration::{
    AuditOutcome, DenialReason, DispatchMode, PermissionAnswer, PermissionState, PolicyBody,
    RunError, RunState, SetPolicy, TargetAction, TargetOpError, TargetPolicy,
};
use hotsheet_ticketing::{NewTicket, TicketPatch, ops};
use serde_json::{Value, json};

use super::tickets::{Machine, NOW, bind_and_allow, human, machine, op, unsupported, worker};
use super::*;

fn target_ticket(m: &Machine, title: &str) -> Ticket {
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
fn dispatch(
    m: &Machine,
    orchestration: &Ticket,
    target: &str,
    operation: &str,
    extra: Value,
) -> DispatchRun {
    let mut body = json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(),
        "native_id": target, "operation_id": operation,
    });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(body).unwrap()
}
fn run_error(result: Result<RunView, OrchestrationError>) -> RunError {
    match result {
        Err(OrchestrationError::Run(error)) => error,
        Err(other) => panic!("expected a run error, got {other}"),
        Ok(view) => panic!("expected a run error, got {:?}", view.run.state),
    }
}
/// Far enough ahead of the fixed test clock to be a live lease.
const LEASE: &str = "2030-01-01T00:00:00Z";

#[test]
fn hs2_ebhdsb_direct_dispatch_claims_once_and_follows_takeover_to_completion() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let first = target_ticket(&m, "First target ticket");
    let second = target_ticket(&m, "Second target ticket");
    let view = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            // The slug is a label; the run records the canonical native id.
            dispatch(
                &m,
                &orchestration,
                &first.slug,
                "op-1",
                json!({"worker_id":"worker-a","worker_label":"Worker A","lease_minutes":45}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let run = view.run.clone();
    assert!(view.unverified.is_empty());
    assert_eq!(run.state, RunState::Dispatched);
    assert_eq!(run.mode, DispatchMode::Direct);
    assert_eq!(run.worker_id.as_deref(), Some("worker-a"));
    assert_eq!(run.target.native_id, first.id.to_string());
    assert!(run.lease_expires_at_ms.is_some());
    assert_eq!(
        run.history.iter().map(|e| e.to).collect::<Vec<_>>(),
        [RunState::Requested, RunState::Dispatched]
    );
    // The target's own claim is the lock: the ticket is started and held there.
    let held = m.target_store.read_ticket(&first.id).unwrap();
    assert_eq!(held.claimed_by.as_deref(), Some("worker-a"));
    assert_eq!(held.worker_label.as_deref(), Some("Worker A"));
    assert_eq!(held.status, Status::Started);
    assert_eq!(held.claim_count, 1);
    // The orchestration ticket carries the link and an identifiers-only note.
    let source = m.owner_store.read_ticket(&orchestration.id).unwrap();
    assert_eq!(source.orchestration_links.len(), 1);
    assert_eq!(
        source.orchestration_links[0].native_id,
        Some(first.id.to_string())
    );
    let note = source.notes.last().unwrap();
    assert_eq!(note.kind, NoteKind::Activity);
    assert!(note.text.contains(&run.run_id));
    assert!(!note.text.contains("First target ticket"));
    assert_eq!(
        m.last_audit(),
        ("work_start".into(), AuditOutcome::Succeeded, None)
    );
    // A duplicate request is the same run and does not claim again.
    let again = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &first.id.to_string(),
                "op-1",
                json!({"worker_id":"worker-a"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(again.run, run);
    assert_eq!(
        m.target_store.read_ticket(&first.id).unwrap().claim_count,
        1
    );
    // One active run per target ticket; a busy worker takes no second ticket.
    assert_eq!(
        run_error(m.runtime.run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &first.id.to_string(),
                "op-2",
                json!({"worker_id":"worker-b"})
            ),
            Some(&worker()),
            &mut tx
        )),
        RunError::TargetActive
    );
    assert_eq!(
        run_error(m.runtime.run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &second.id.to_string(),
                "op-3",
                json!({"worker_id":"worker-a"})
            ),
            Some(&worker()),
            &mut tx
        )),
        RunError::WorkerBusy
    );
    assert!(
        m.target_store
            .read_ticket(&second.id)
            .unwrap()
            .claimed_by
            .is_none()
    );
    assert_eq!(
        m.last_audit(),
        (
            "work_start".into(),
            AuditOutcome::Failed,
            Some("run_worker_busy".into())
        )
    );
    // Renew and sync leave a healthy run where it is.
    let renewed = m
        .runtime
        .run_renew(&m.owner.id, &run.run_id, Some(60), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(renewed.run.state, RunState::Dispatched);
    assert_eq!(renewed.run.history.len(), 2);
    let steady = m
        .runtime
        .run_sync(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(steady.run.state, RunState::Dispatched);
    // A person takes the ticket over in the target.
    ops::release(&m.target_store, &first.id, NOW.into(), "worker-a", false).unwrap();
    let released = m
        .runtime
        .run_sync(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(released.run.state, RunState::Intervened);
    assert_eq!(
        released.run.history.last().unwrap().reason.as_deref(),
        Some("claim_released")
    );
    assert_eq!(released.run.lease_expires_at_ms, None);
    // An intervened run cannot be renewed on the orchestrator's say-so.
    assert_eq!(
        run_error(
            m.runtime
                .run_renew(&m.owner.id, &run.run_id, None, Some(&worker()), &mut tx)
        ),
        RunError::NotActive
    );
    // HS2-0P90V8: the intervened run still owns the ticket. A second dispatch there
    // is refused before anything is claimed, whatever the worker or mode.
    for (operation, body) in [
        ("op-0p90v8-a", json!({"worker_id":"worker-b"})),
        ("op-0p90v8-b", json!({"worker_id":"worker-a"})),
        ("op-0p90v8-c", json!({"mode":"dispatcher"})),
    ] {
        assert_eq!(
            run_error(m.runtime.run_dispatch(
                &m.owner.id,
                dispatch(&m, &orchestration, &first.id.to_string(), operation, body),
                Some(&worker()),
                &mut tx
            )),
            RunError::TargetActive
        );
        let unclaimed = m.target_store.read_ticket(&first.id).unwrap();
        assert!(unclaimed.claimed_by.is_none());
        assert!(!unclaimed.up_next);
    }
    ops::claim_with_eta(
        &m.target_store,
        &first.id,
        &NOW.into(),
        LEASE.into(),
        "human-1",
        None,
        None,
    )
    .unwrap();
    // Someone else holding it does not hand the run back.
    let still = m
        .runtime
        .run_sync(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(still.run.state, RunState::Intervened);
    // The person hands it back to the run's worker.
    ops::release(&m.target_store, &first.id, NOW.into(), "human-1", false).unwrap();
    ops::claim_with_eta(
        &m.target_store,
        &first.id,
        &NOW.into(),
        LEASE.into(),
        "worker-a",
        None,
        None,
    )
    .unwrap();
    let back = m
        .runtime
        .run_sync(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(back.run.state, RunState::Dispatched);
    assert_eq!(
        back.run.history.last().unwrap().reason.as_deref(),
        Some("claim_confirmed")
    );
    // Another worker taking a dispatched run's claim is an intervention too.
    ops::release(&m.target_store, &first.id, NOW.into(), "worker-a", false).unwrap();
    ops::claim_with_eta(
        &m.target_store,
        &first.id,
        &NOW.into(),
        LEASE.into(),
        "worker-z",
        None,
        None,
    )
    .unwrap();
    let taken = m
        .runtime
        .run_sync(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        taken.run.history.last().unwrap().reason.as_deref(),
        Some("claim_taken")
    );
    // The target ticket is completed by whoever finished it: the run completes.
    ops::update(
        &m.target_store,
        &first.id,
        NOW.into(),
        TicketPatch {
            status: Some(Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
    let done = m
        .runtime
        .run_sync(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(done.run.state, RunState::Completed);
    assert_eq!(
        done.run.history.iter().map(|e| e.to).collect::<Vec<_>>(),
        [
            RunState::Requested,
            RunState::Dispatched,
            RunState::Intervened,
            RunState::Dispatched,
            RunState::Intervened,
            RunState::Completed
        ]
    );
    let source = m.owner_store.read_ticket(&orchestration.id).unwrap();
    assert!(source.notes.last().unwrap().text.contains("finished"));
    // A finished run is final: it cannot be renewed or cancelled, and syncing is a read.
    assert_eq!(
        run_error(
            m.runtime
                .run_cancel(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        ),
        RunError::NotActive
    );
    assert_eq!(
        m.runtime
            .run_sync(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
            .unwrap()
            .run,
        done.run
    );
    // The worker and the ticket's slot are free again.
    let next = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &second.id.to_string(),
                "op-4",
                json!({"worker_id":"worker-a"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(next.run.state, RunState::Dispatched);
    assert_eq!(m.runtime.run_list(&m.owner.id, true).unwrap().len(), 1);
    assert_eq!(m.runtime.run_list(&m.owner.id, false).unwrap().len(), 2);
}

#[test]
fn hs2_ebhdsb_refused_and_unavailable_dispatches_record_exactly_what_happened() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Target ticket");
    let request = |operation: &str| {
        dispatch(
            &m,
            &orchestration,
            &ticket.id.to_string(),
            operation,
            json!({"worker_id":"worker-a"}),
        )
    };
    // Malformed requests never reach the gate.
    for extra in [
        json!({}),
        json!({"mode":"dispatcher","worker_id":"worker-a"}),
        json!({"worker_id":"worker-a","lease_minutes":1}),
        json!({"worker_id":"worker-a","lease_minutes":1000}),
        json!({"worker_id":"bad worker"}),
    ] {
        assert!(matches!(
            m.runtime.run_dispatch(
                &m.owner.id,
                dispatch(&m, &orchestration, &ticket.id.to_string(), "op-0", extra),
                Some(&worker()),
                &mut tx
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    // HS2-XHQ1EH: a bound target is consented to, but an AI's dispatch asks a person
    // first, and no run is recorded until one answers.
    bind_and_allow(&m, 1, &[], &mut tx);
    permission_request(m.runtime.run_dispatch(
        &m.owner.id,
        request("op-1"),
        Some(&worker()),
        &mut tx,
    ));
    assert!(m.runtime.run_list(&m.owner.id, false).unwrap().is_empty());
    assert!(
        m.target_store
            .read_ticket(&ticket.id)
            .unwrap()
            .claimed_by
            .is_none()
    );
    allow_on(
        &m.runtime,
        &m.owner.id,
        &id(1),
        &[TargetAction::WorkStart],
        &[],
        &mut tx,
    );
    // A ticket that does not exist is never recorded as a run.
    assert_eq!(
        op(m.runtime.run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &id(4242),
                "op-2",
                json!({"worker_id":"worker-a"})
            ),
            Some(&worker()),
            &mut tx
        )),
        TargetOpError::NotFound
    );
    assert!(m.runtime.run_list(&m.owner.id, false).unwrap().is_empty());
    // Someone already holds the ticket: the run fails with the target's refusal and
    // their claim is untouched.
    ops::claim_with_eta(
        &m.target_store,
        &ticket.id,
        &NOW.into(),
        LEASE.into(),
        "human-1",
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        op(m.runtime
            .run_dispatch(&m.owner.id, request("op-3"), Some(&worker()), &mut tx)),
        TargetOpError::Conflict
    );
    let failed = m.runtime.run_list(&m.owner.id, false).unwrap().remove(0);
    assert_eq!(failed.state, RunState::Failed);
    assert_eq!(
        failed.history.last().unwrap().reason.as_deref(),
        Some("target_conflict")
    );
    assert_eq!(
        m.target_store
            .read_ticket(&ticket.id)
            .unwrap()
            .claimed_by
            .as_deref(),
        Some("human-1")
    );
    assert_eq!(
        m.last_audit(),
        (
            "work_start".into(),
            AuditOutcome::Failed,
            Some("target_conflict".into())
        )
    );
    // Repeating that operation answers with the failed run; it does not try again.
    let repeated = m
        .runtime
        .run_dispatch(&m.owner.id, request("op-3"), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(repeated.run, failed);
    // Once the ticket is free, a new operation dispatches it.
    ops::release(&m.target_store, &ticket.id, NOW.into(), "human-1", false).unwrap();
    let run = m
        .runtime
        .run_dispatch(&m.owner.id, request("op-4"), Some(&worker()), &mut tx)
        .unwrap()
        .run;
    assert_eq!(run.state, RunState::Dispatched);
    // The target becomes unreachable: a cancel records the intent and says what it
    // could not verify; the run stays active and the claim stays where it is.
    let binding = m.runtime.resolve(&m.reference(1), &mut tx).unwrap().binding;
    m.runtime
        .request(
            "DELETE",
            &format!(
                "/checkouts/{}/orchestration/targets/{}/binding",
                m.owner.id,
                id(1)
            ),
            &json!({"target":m.reference(1),"expected_binding_revision":binding.revision,
                "previous_destination":binding.destination}),
            &mut tx,
        )
        .unwrap();
    let pending = m
        .runtime
        .run_cancel(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(pending.unverified, ["target_claim"]);
    assert_eq!(pending.run.state, RunState::Dispatched);
    assert!(pending.run.cancel_requested);
    assert_eq!(
        m.target_store
            .read_ticket(&ticket.id)
            .unwrap()
            .claimed_by
            .as_deref(),
        Some("worker-a")
    );
    assert!(matches!(
        m.runtime
            .run_sync(&m.owner.id, &run.run_id, Some(&worker()), &mut tx),
        Err(OrchestrationError::Denied(DenialReason::TargetUnavailable))
    ));
    assert!(matches!(
        m.runtime.run(&m.owner.id, &id(999)),
        Err(OrchestrationError::Run(RunError::NotFound))
    ));
}

#[test]
fn hs2_ebhdsb_dispatcher_mode_queues_and_never_assigns_in_the_targets_pool() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[
            TargetAction::TicketsRead,
            TargetAction::TicketsWrite,
            TargetAction::WorkStart,
        ],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "For the target's dispatcher");
    let queued = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &ticket.id.to_string(),
                "op-1",
                json!({"mode":"dispatcher"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    assert_eq!(queued.state, RunState::Queued);
    assert_eq!(queued.worker_id, None);
    // The orchestrator only marked it Up Next; it took no claim.
    let marked = m.target_store.read_ticket(&ticket.id).unwrap();
    assert!(marked.up_next);
    assert!(marked.claimed_by.is_none());
    assert_eq!(marked.claim_count, 0);
    assert_eq!(
        m.runtime
            .run_sync(&m.owner.id, &queued.run_id, Some(&worker()), &mut tx)
            .unwrap()
            .run
            .state,
        RunState::Queued
    );
    // A queued run has no lease of its own to renew.
    assert_eq!(
        run_error(
            m.runtime
                .run_renew(&m.owner.id, &queued.run_id, None, Some(&worker()), &mut tx)
        ),
        RunError::NotActive
    );
    // The target's dispatcher assigns it and its worker claims it.
    ops::claim_with_eta(
        &m.target_store,
        &ticket.id,
        &NOW.into(),
        LEASE.into(),
        "pool-worker-3",
        None,
        None,
    )
    .unwrap();
    let assigned = m
        .runtime
        .run_sync(&m.owner.id, &queued.run_id, Some(&worker()), &mut tx)
        .unwrap()
        .run;
    assert_eq!(assigned.state, RunState::Dispatched);
    assert_eq!(assigned.worker_id.as_deref(), Some("pool-worker-3"));
    assert_eq!(
        assigned.history.last().unwrap().reason.as_deref(),
        Some("assigned_by_dispatcher")
    );
    // Cancelling a pooled worker's run lets go of the run; the claim is not the
    // orchestrator's to release.
    let cancelled = m
        .runtime
        .run_cancel(&m.owner.id, &queued.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(cancelled.run.state, RunState::Cancelled);
    assert!(cancelled.unverified.is_empty());
    assert_eq!(
        m.target_store
            .read_ticket(&ticket.id)
            .unwrap()
            .claimed_by
            .as_deref(),
        Some("pool-worker-3")
    );
    // A still-queued run withdraws its Up Next mark when cancelled.
    let other = target_ticket(&m, "Queued then withdrawn");
    let second = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &other.id.to_string(),
                "op-2",
                json!({"mode":"dispatcher"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    assert!(m.target_store.read_ticket(&other.id).unwrap().up_next);
    let withdrawn = m
        .runtime
        .run_cancel(&m.owner.id, &second.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(withdrawn.run.state, RunState::Cancelled);
    assert!(!m.target_store.read_ticket(&other.id).unwrap().up_next);
    // Cancel is idempotent in effect: a second cancel reports the run is finished.
    assert_eq!(
        run_error(
            m.runtime
                .run_cancel(&m.owner.id, &second.run_id, Some(&worker()), &mut tx)
        ),
        RunError::NotActive
    );
    // A direct cancel releases the run's own claim.
    let third = target_ticket(&m, "Direct then cancelled");
    let direct = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &third.id.to_string(),
                "op-3",
                json!({"worker_id":"worker-a"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    m.runtime
        .run_cancel(&m.owner.id, &direct.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert!(
        m.target_store
            .read_ticket(&third.id)
            .unwrap()
            .claimed_by
            .is_none()
    );
}

#[test]
fn hs2_ebhdsb_an_approved_dispatch_is_not_asked_again_to_keep_its_lease() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    // HS2-XHQ1EH: no rule is remembered, so the dispatch asks a person.
    bind_and_allow(&m, 1, &[], &mut tx);
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Needs a person first");
    let request = || {
        dispatch(
            &m,
            &orchestration,
            &ticket.id.to_string(),
            "op-1",
            json!({"worker_id":"worker-a"}),
        )
    };
    let asked = permission_request(m.runtime.run_dispatch(
        &m.owner.id,
        request(),
        Some(&worker()),
        &mut tx,
    ));
    assert!(m.runtime.run_list(&m.owner.id, false).unwrap().is_empty());
    answer(
        &m.runtime,
        &m.owner.id,
        &asked,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    let run = m
        .runtime
        .run_dispatch(&m.owner.id, request(), Some(&worker()), &mut tx)
        .unwrap()
        .run;
    assert_eq!(run.state, RunState::Dispatched);
    // The dispatch happened, so Allow Once is used up.
    let list = m.runtime.permission_list(&m.owner.id, &mut tx).unwrap();
    assert_eq!(list.requests[0].request.state, PermissionState::Used);
    // Keeping and releasing the lease continue that decision and ask nobody.
    m.runtime
        .run_renew(&m.owner.id, &run.run_id, None, Some(&worker()), &mut tx)
        .unwrap();
    m.runtime
        .run_cancel(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        m.runtime
            .permission_list(&m.owner.id, &mut tx)
            .unwrap()
            .requests
            .len(),
        1
    );
    // A new dispatch is a new decision.
    let again = dispatch(
        &m,
        &orchestration,
        &ticket.id.to_string(),
        "op-2",
        json!({"worker_id":"worker-a"}),
    );
    let second =
        permission_request(
            m.runtime
                .run_dispatch(&m.owner.id, again, Some(&worker()), &mut tx),
        );
    assert_ne!(second, asked);
}

/// HS2-8NJVXC: a dispatch whose claim the target refuses does not use the Allow Once
/// answer; repeating the same operation proceeds without asking again.
#[test]
fn hs2_8njvxc_a_refused_claim_keeps_the_allow_once_answer() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[], &mut tx);
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Someone else holds it");
    // Another worker holds the ticket: the claim will be refused.
    m.runtime
        .request(
            "POST",
            &format!("/checkouts/{}/target-claims", m.target.id),
            &json!({"connection_id":m.target_connection(),"native_id":ticket.id.to_string(),
                "worker":"worker-b","lease_minutes":30}),
            &mut tx,
        )
        .unwrap();
    let request = || {
        dispatch(
            &m,
            &orchestration,
            &ticket.id.to_string(),
            "op-held",
            json!({"worker_id":"worker-a"}),
        )
    };
    let asked = permission_request(m.runtime.run_dispatch(
        &m.owner.id,
        request(),
        Some(&worker()),
        &mut tx,
    ));
    answer(
        &m.runtime,
        &m.owner.id,
        &asked,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    assert!(
        m.runtime
            .run_dispatch(&m.owner.id, request(), Some(&worker()), &mut tx)
            .is_err()
    );
    let list = m.runtime.permission_list(&m.owner.id, &mut tx).unwrap();
    assert_eq!(list.requests.len(), 1);
    assert_eq!(list.requests[0].request.state, PermissionState::Allowed);
}

#[test]
fn hs2_ebhdsb_hosted_claim_routes_and_run_routes() {
    let m = machine(10, true, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let ticket = target_ticket(&m, "Hosted claim");
    let hosted = format!("/checkouts/{}/target-claims", m.target.id);
    let body = |worker: &str| {
        json!({"connection_id":m.target_connection(),"native_id":ticket.id.to_string(),
            "worker":worker,"lease_minutes":30})
    };
    let claimed = m
        .runtime
        .request("POST", &hosted, &body("worker-a"), &mut tx)
        .unwrap();
    assert_eq!(claimed["ticket"]["claimed_by"], "worker-a");
    assert_eq!(claimed["ticket"]["status"], "started");
    // Another worker cannot take, renew or release someone else's claim.
    for method in ["POST", "PATCH", "DELETE"] {
        assert_eq!(
            op(m.runtime
                .request(method, &hosted, &body("worker-b"), &mut tx)),
            TargetOpError::Conflict,
            "{method}"
        );
    }
    m.runtime
        .request("PATCH", &hosted, &body("worker-a"), &mut tx)
        .unwrap();
    let released = m
        .runtime
        .request("DELETE", &hosted, &body("worker-a"), &mut tx)
        .unwrap();
    assert_eq!(released["ticket"]["claimed_by"], Value::Null);
    // Releasing again is idempotent; renewing a released claim is refused.
    m.runtime
        .request("DELETE", &hosted, &body("worker-a"), &mut tx)
        .unwrap();
    assert_eq!(
        op(m.runtime
            .request("PATCH", &hosted, &body("worker-a"), &mut tx)),
        TargetOpError::Conflict
    );
    // Exact claims exist only for git sources; another provider says so.
    assert_eq!(
        op(m.runtime.request(
            "POST",
            &hosted,
            &json!({"connection_id":"external-source","native_id":"17","worker":"worker-a"}),
            &mut tx
        )),
        TargetOpError::Unsupported
    );
    assert!(!m.external.opened.load(Ordering::SeqCst));
    for bad in [
        json!({"connection_id":m.target_connection(),"native_id":ticket.id.to_string()}),
        json!({"connection_id":m.target_connection(),"native_id":"a/b","worker":"w"}),
        json!({"connection_id":m.target_connection(),"native_id":ticket.id.to_string(),
            "worker":"w","lease_minutes":2}),
        json!({"connection_id":m.target_connection(),"native_id":ticket.id.to_string(),
            "worker":"has space"}),
    ] {
        assert!(matches!(
            m.runtime.request("POST", &hosted, &bad, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    assert_eq!(
        op(m.runtime.request(
            "POST",
            &hosted,
            &json!({"connection_id":m.target_connection(),"native_id":id(4242),"worker":"w"}),
            &mut tx
        )),
        TargetOpError::NotFound
    );
    assert!(is_path(&hosted));
    assert!(!is_machine_path(&hosted));
    let runs = format!("/checkouts/{}/orchestration/runs", m.owner.id);
    for path in [
        runs.clone(),
        format!("{runs}?active=true"),
        format!("{runs}/{}", id(5)),
        format!("{runs}/{}/sync", id(5)),
    ] {
        assert!(is_path(&path), "{path}");
        assert!(is_machine_path(&path), "{path}");
    }
    assert_eq!(
        m.runtime
            .request("GET", &format!("{runs}?active=true"), &Value::Null, &mut tx)
            .unwrap(),
        json!([])
    );
    let missing = m
        .runtime
        .request("GET", &format!("{runs}/{}", id(5)), &Value::Null, &mut tx)
        .unwrap_err();
    assert_eq!(missing.code(), "run_not_found");
    assert_eq!(error_status(&missing), 404);
    for (operation, body) in [
        ("explode", json!({})),
        ("renew", json!({"lease_minutes":"soon"})),
    ] {
        assert!(matches!(
            m.runtime.request(
                "POST",
                &format!("{runs}/{}/{operation}", id(5)),
                &body,
                &mut tx
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
}

/// HS2-2C5N1W: several syncs of one run at once, as a person's request beside the
/// server's follower, after its target ticket was completed. Every one answers the
/// finished run; none is told `run_not_active`, and the run finishes once.
#[test]
fn hs2_2c5n1w_concurrent_syncs_of_a_run_that_finishes_all_answer_the_finished_run() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(300);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    for round in 0..4u128 {
        let work = target_ticket(&m, "Finished while synced");
        let view = m
            .runtime
            .run_dispatch(
                &m.owner.id,
                dispatch(
                    &m,
                    &orchestration,
                    &work.slug,
                    &format!("op-race-{round}"),
                    json!({"worker_id": "worker-race", "lease_minutes": 30}),
                ),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
        ops::update(
            &m.target_store,
            &work.id,
            NOW.into(),
            TicketPatch {
                status: Some(Status::Completed),
                ..Default::default()
            },
        )
        .unwrap();
        let run_id = view.run.run_id.clone();
        let barrier = std::sync::Barrier::new(6);
        let answers: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..6u128)
                .map(|thread| {
                    let (runtime, owner, run_id, barrier) =
                        (&m.runtime, &m.owner.id, &run_id, &barrier);
                    scope.spawn(move || {
                        let mut rng = Rng(400 + round * 10 + thread);
                        let mut tx = Transaction {
                            clock: &Clock,
                            rng: &mut rng,
                        };
                        barrier.wait();
                        runtime
                            .run_sync(owner, run_id, Some(&worker()), &mut tx)
                            .map(|view| view.run.state)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        for answer in answers {
            assert_eq!(answer.unwrap(), RunState::Completed);
        }
        let run = m.runtime.run(&m.owner.id, &run_id).unwrap();
        let completions = run
            .history
            .iter()
            .filter(|entry| entry.to == RunState::Completed)
            .count();
        assert_eq!(completions, 1);
    }
}

/// HS2-WC3W2G, HS2-XHQ1EH: cancelling only reduces activity. A person, or another
/// identified actor with no remembered rule, cancels a queued run without being asked
/// and without a policy denial; the mark is withdrawn and nothing is filed.
#[test]
fn hs2_xhq1eh_a_cancel_never_asks_and_is_never_denied_by_the_policy() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsWrite, TargetAction::WorkStart],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Queued for the dispatcher");
    let queued = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &ticket.id.to_string(),
                "op-1",
                json!({"mode":"dispatcher"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    assert_eq!(queued.state, RunState::Queued);
    // A policy that disables the target limits acting there, never cancelling.
    m.runtime
        .set_policy(
            &m.owner.id,
            SetPolicy {
                expected_revision: None,
                policy: PolicyBody {
                    targets: vec![TargetPolicy {
                        target_id: id(1),
                        enabled: false,
                        actions: None,
                        tools: Vec::new(),
                        max_sessions: Some(1),
                        machine: None,
                        speculation: None,
                    }],
                    ..PolicyBody::default()
                },
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let other =
        hotsheet_ticketing::actor::MutationActor::parse("ai", Some("other-ai".into())).unwrap();
    let cancelled = m
        .runtime
        .run_cancel(&m.owner.id, &queued.run_id, Some(&other), &mut tx)
        .unwrap();
    assert_eq!(cancelled.run.state, RunState::Cancelled);
    assert!(cancelled.unverified.is_empty());
    assert!(!m.target_store.read_ticket(&ticket.id).unwrap().up_next);
    assert!(
        m.runtime
            .permission_list(&m.owner.id, &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
    assert!(
        m.runtime
            .audit(&m.owner.id, 0, 500)
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.record.outcome != AuditOutcome::Denied)
    );
}

/// HS2-68DWAM: `run status` never shows an outcome assessed on an earlier state. A
/// queued dispatcher-mode run is reconciled (`wait_for_dispatcher`); a sync then
/// moves it to `dispatched` for the worker that claimed it, and the status shows no
/// outcome until the next reconciliation stores the one for `dispatched`.
#[test]
fn hs2_68dwam_a_sync_that_moves_the_run_on_drops_the_shown_reconciliation() {
    use hotsheet_ticketing::orchestration::Disposition;
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[
            TargetAction::TicketsRead,
            TargetAction::TicketsWrite,
            TargetAction::WorkStart,
        ],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Queued then claimed");
    let queued = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &ticket.id.to_string(),
                "op-1",
                json!({"mode":"dispatcher"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    let waiting = m
        .runtime
        .run_reconcile(&m.owner.id, &queued.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        waiting.run.reconciliation.as_ref().unwrap().disposition,
        Disposition::WaitForDispatcher
    );
    ops::claim_with_eta(
        &m.target_store,
        &ticket.id,
        &NOW.into(),
        "2030-01-01T00:00:00Z".into(),
        "pool-worker-3",
        None,
        None,
    )
    .unwrap();
    let synced = m
        .runtime
        .run_sync(&m.owner.id, &queued.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(synced.run.state, RunState::Dispatched);
    let status = m.runtime.run_status(&m.owner.id, &queued.run_id).unwrap();
    assert_eq!(status.run.state, RunState::Dispatched);
    assert_eq!(status.run.reconciliation, None);
    // The next reconciliation stores the outcome for the state it found.
    let again = m
        .runtime
        .run_reconcile(&m.owner.id, &queued.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        again.run.reconciliation.as_ref().unwrap().disposition,
        Disposition::Continue
    );
    assert_eq!(
        m.runtime
            .run_status(&m.owner.id, &queued.run_id)
            .unwrap()
            .run
            .reconciliation,
        again.run.reconciliation
    );
}

#[test]
fn hs2_znet66_a_queued_run_learns_a_claim_taken_and_released_between_two_syncs() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[
            TargetAction::TicketsRead,
            TargetAction::TicketsWrite,
            TargetAction::WorkStart,
        ],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Worked between two reads");
    let queued = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            dispatch(
                &m,
                &orchestration,
                &ticket.id.to_string(),
                "op-1",
                json!({"mode":"dispatcher"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    let sync = |tx: &mut Transaction<'_>| {
        m.runtime
            .run_sync(&m.owner.id, &queued.run_id, Some(&worker()), tx)
            .unwrap()
            .run
    };
    // The first read records the ticket's claim count.
    let waiting = sync(&mut tx);
    assert_eq!(waiting.state, RunState::Queued);
    assert_eq!(waiting.observed.as_ref().unwrap().claim_count, Some(0));
    // Read again with nothing changed: still waiting for the target's dispatcher.
    assert_eq!(sync(&mut tx).state, RunState::Queued);
    // The pool's worker claims, works and releases before the run reads again.
    ops::claim_with_eta(
        &m.target_store,
        &ticket.id,
        &NOW.into(),
        LEASE.into(),
        "pool-worker-3",
        None,
        None,
    )
    .unwrap();
    ops::release(
        &m.target_store,
        &ticket.id,
        NOW.into(),
        "pool-worker-3",
        false,
    )
    .unwrap();
    let released = sync(&mut tx);
    assert_eq!(released.state, RunState::Intervened);
    assert_eq!(
        released.history.last().unwrap().reason.as_deref(),
        Some("claim_released")
    );
    assert_eq!(released.observed.as_ref().unwrap().claim_count, Some(1));
    assert!(released.observed.as_ref().unwrap().claimed_by.is_none());
    // Reading the same ticket again changes nothing more.
    let again = sync(&mut tx);
    assert_eq!(again.state, RunState::Intervened);
    assert_eq!(again.history.len(), released.history.len());
}
