//! HS2-WBREDM: reconciling interrupted runs and the control actions, through the
//! shared runtime against real git ticket stores. Synthetic identities only.
use hotsheet_model::{NoteKind, Status, Ticket};
use hotsheet_ticketing::orchestration::{
    AuditOutcome, DenialReason, DispatchMode, Disposition, ExternalSource, NewRun, Run, RunControl,
    RunError, RunEventKind, RunRegistry, RunState, RunTarget, RunTicket, SOURCE_NOT_WIRED,
    TargetAction, TargetOpError, Verification,
};
use hotsheet_ticketing::{NewTicket, TicketPatch, ops};
use serde_json::{Value, json};

use super::tickets::{Machine, NOW, bind_and_grant, human, machine, op, unsupported, worker};
use super::*;

const LEASE: &str = "2030-01-01T00:00:00Z";
const NOT_WIRED: [&str; 3] = ["ci_run", "deployment", "session"];

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
fn request(
    m: &Machine,
    orchestration: &Ticket,
    target: &Ticket,
    operation: &str,
    extra: Value,
) -> DispatchRun {
    let mut body = json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(),
        "native_id": target.id.to_string(), "operation_id": operation,
    });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(body).unwrap()
}
fn dispatch(
    m: &Machine,
    orchestration: &Ticket,
    target: &Ticket,
    operation: &str,
    worker_id: &str,
    tx: &mut Transaction<'_>,
) -> Run {
    m.runtime
        .run_dispatch(
            &m.owner.id,
            request(
                m,
                orchestration,
                target,
                operation,
                json!({"worker_id": worker_id}),
            ),
            Some(&worker()),
            tx,
        )
        .unwrap()
        .run
}
/// The record an orchestrator leaves when it stops between recording a dispatch and
/// hearing the target's answer.
fn interrupted(
    m: &Machine,
    orchestration: &Ticket,
    target: &Ticket,
    operation: &str,
    worker_id: Option<&str>,
    tx: &mut Transaction<'_>,
) -> Run {
    RunRegistry::new(&m.home)
        .begin(
            &m.owner.id,
            NewRun {
                operation_id: operation.into(),
                ticket: RunTicket {
                    connection_id: m.owner_connection(),
                    native_id: orchestration.id.to_string(),
                },
                target: RunTarget {
                    target_id: id(1),
                    connection_id: m.target_connection(),
                    native_id: target.id.to_string(),
                },
                mode: if worker_id.is_some() {
                    DispatchMode::Direct
                } else {
                    DispatchMode::Dispatcher
                },
                worker_id: worker_id.map(Into::into),
            },
            Some("worker-1".into()),
            tx,
        )
        .unwrap()
        .0
}
fn claim(m: &Machine, ticket: &Ticket, holder: &str) {
    ops::claim_with_eta(
        &m.target_store,
        &ticket.id,
        &NOW.into(),
        LEASE.into(),
        holder,
        None,
        None,
    )
    .unwrap();
}
fn release(m: &Machine, ticket: &Ticket, holder: &str) {
    ops::release(&m.target_store, &ticket.id, NOW.into(), holder, false).unwrap();
}
fn held(m: &Machine, ticket: &Ticket) -> (Option<String>, u32) {
    let ticket = m.target_store.read_ticket(&ticket.id).unwrap();
    (ticket.claimed_by, ticket.claim_count)
}
fn kinds(m: &Machine, run: &Run) -> Vec<(RunEventKind, Option<String>)> {
    m.runtime
        .run_events(&m.owner.id, 0, 500, Some(&run.run_id), 0)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| (entry.record.kind, entry.record.reason))
        .collect()
}
fn disposition(view: &RunView) -> Disposition {
    view.run.reconciliation.as_ref().unwrap().disposition
}
fn finding(view: &RunView, source: ExternalSource) -> (Verification, Option<String>) {
    let found = view
        .run
        .reconciliation
        .as_ref()
        .unwrap()
        .findings
        .iter()
        .find(|found| found.source == source)
        .unwrap();
    (found.verification, found.reason.clone())
}
/// Lose the target the way this fixture can: unbind it. Returns what binds it again.
fn lose_target(m: &Machine, tx: &mut Transaction<'_>) -> BindTarget {
    let binding = m.runtime.resolve(&m.reference(1), tx).unwrap().binding;
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
            tx,
        )
        .unwrap();
    BindTarget {
        target: m.reference(1),
        expected_manifest_revision: m
            .runtime
            .declarations(&m.owner.id)
            .unwrap()
            .revision
            .unwrap(),
        expected_binding_revision: binding.revision + 1,
        previous_destination: None,
        destination: binding.destination.unwrap(),
    }
}
fn run_error(result: Result<RunView, OrchestrationError>) -> RunError {
    match result {
        Err(OrchestrationError::Run(error)) => error,
        Err(other) => panic!("expected a run error, got {other}"),
        Ok(view) => panic!("expected a run error, got {:?}", view.run.state),
    }
}
fn granted(m: &Machine, tx: &mut Transaction<'_>) {
    bind_and_grant(
        m,
        1,
        &[
            TargetAction::TicketsRead,
            TargetAction::TicketsWrite,
            TargetAction::WorkStart,
        ],
        tx,
    );
}

#[test]
fn hs2_wbredm_a_restart_reconciles_each_interrupted_dispatch_without_a_duplicate() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    // Nothing to recover: nothing is listed and nothing is created by asking.
    assert!(m.runtime.run_owners().unwrap().is_empty());
    granted(&m, &mut tx);
    let orchestration = m.orchestration_ticket("Coordinate");
    // 1. Stopped after the target took the claim, before the answer was recorded.
    let claimed = target_ticket(&m, "Claimed before the crash");
    let run = interrupted(
        &m,
        &orchestration,
        &claimed,
        "op-1",
        Some("worker-a"),
        &mut tx,
    );
    claim(&m, &claimed, "worker-a");
    assert_eq!(
        m.runtime.run_owners().unwrap(),
        std::slice::from_ref(&m.owner.id)
    );
    let view = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(view.run.state, RunState::Dispatched);
    assert_eq!(
        view.run.history.last().unwrap().reason.as_deref(),
        Some("claim_confirmed")
    );
    assert!(view.run.lease_expires_at_ms.is_some());
    assert_eq!(disposition(&view), Disposition::Continue);
    // Exactly the sources nobody reports yet are named, with the reason.
    assert_eq!(view.unverified, NOT_WIRED);
    for source in [
        ExternalSource::CiRun,
        ExternalSource::Deployment,
        ExternalSource::Session,
    ] {
        assert_eq!(
            finding(&view, source),
            (Verification::Unverifiable, Some(SOURCE_NOT_WIRED.into()))
        );
    }
    for source in ExternalSource::ALL.into_iter().filter(|s| s.is_target()) {
        assert_eq!(finding(&view, source), (Verification::Verified, None));
    }
    // The claim was adopted, never taken a second time.
    assert_eq!(held(&m, &claimed), (Some("worker-a".into()), 1));
    // Reconciling an unchanged world again writes nothing and reports nothing.
    let events = kinds(&m, &run);
    assert_eq!(
        events.iter().map(|event| event.0).collect::<Vec<_>>(),
        [
            RunEventKind::State,
            RunEventKind::Observed,
            RunEventKind::Reconciled
        ]
    );
    assert_eq!(events[2].1.as_deref(), Some("continue"));
    for _ in 0..3 {
        let again = m
            .runtime
            .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
            .unwrap();
        assert_eq!(again.run, view.run);
        assert_eq!(again.unverified, view.unverified);
    }
    assert_eq!(kinds(&m, &run), events);
    // Repeating the interrupted dispatch answers with the same run and claims nothing.
    let repeated = dispatch(&m, &orchestration, &claimed, "op-1", "worker-a", &mut tx);
    assert_eq!(repeated, view.run);
    assert_eq!(held(&m, &claimed).1, 1);

    // 2. Stopped before the target was asked: nothing is held, so the same dispatch
    //    may be repeated, and it is still one run and one claim.
    let unasked = target_ticket(&m, "Never asked");
    let run = interrupted(
        &m,
        &orchestration,
        &unasked,
        "op-2",
        Some("worker-b"),
        &mut tx,
    );
    let view = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(view.run.state, RunState::Requested);
    assert_eq!(disposition(&view), Disposition::RetryDispatch);
    assert_eq!(held(&m, &unasked), (None, 0));
    let total = m.runtime.run_list(&m.owner.id, false).unwrap().len();
    let retried = dispatch(&m, &orchestration, &unasked, "op-2", "worker-b", &mut tx);
    assert_eq!(retried.run_id, run.run_id);
    assert_eq!(retried.state, RunState::Dispatched);
    assert_eq!(held(&m, &unasked), (Some("worker-b".into()), 1));
    assert_eq!(m.runtime.run_list(&m.owner.id, false).unwrap().len(), total);
    let settled = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(disposition(&settled), Disposition::Continue);
    assert_eq!(
        kinds(&m, &run)
            .into_iter()
            .filter(|event| event.0 == RunEventKind::Reconciled)
            .map(|event| event.1.unwrap())
            .collect::<Vec<_>>(),
        ["retry_dispatch", "continue"]
    );

    // 3. Someone else holds the ticket: nothing is forced, and a person must decide.
    let taken = target_ticket(&m, "Held by a person");
    let run = interrupted(
        &m,
        &orchestration,
        &taken,
        "op-3",
        Some("worker-c"),
        &mut tx,
    );
    claim(&m, &taken, "human-1");
    let view = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(view.run.state, RunState::Requested);
    assert_eq!(disposition(&view), Disposition::NeedsAttention);
    assert_eq!(
        finding(&view, ExternalSource::TargetClaim),
        (Verification::Mismatched, Some("claim_held_by_other".into()))
    );
    assert_eq!(view.unverified, NOT_WIRED);
    assert_eq!(held(&m, &taken), (Some("human-1".into()), 1));

    // 4. Queued for the target's dispatcher with the answer lost: the worker its
    //    dispatcher assigned holds the work, and the ticket is never queued twice.
    let pooled = target_ticket(&m, "Assigned by the target");
    let run = interrupted(&m, &orchestration, &pooled, "op-4", None, &mut tx);
    claim(&m, &pooled, "pool-1");
    let view = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(view.run.state, RunState::Dispatched);
    assert_eq!(view.run.worker_id.as_deref(), Some("pool-1"));
    assert_eq!(
        view.run.history.last().unwrap().reason.as_deref(),
        Some("assigned_by_dispatcher")
    );
    assert_eq!(held(&m, &pooled), (Some("pool-1".into()), 1));

    // 5. The ticket was finished, or disappeared, while nobody was looking.
    let finished = target_ticket(&m, "Finished meanwhile");
    let run = interrupted(
        &m,
        &orchestration,
        &finished,
        "op-5",
        Some("worker-e"),
        &mut tx,
    );
    ops::update(
        &m.target_store,
        &finished.id,
        NOW.into(),
        TicketPatch {
            status: Some(Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
    let view = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(view.run.state, RunState::Completed);
    // A finished run depends on nothing: it keeps no outcome and names nothing.
    assert_eq!(view.run.reconciliation, None);
    assert!(view.unverified.is_empty());
    let note = m.owner_store.read_ticket(&orchestration.id).unwrap();
    let note = note.notes.last().unwrap();
    assert_eq!(note.kind, NoteKind::Activity);
    assert!(note.text.contains(&run.run_id) && !note.text.contains("Finished meanwhile"));
    // A finished run is reported as it is, without contacting anything.
    let audits = m.runtime.audit(&m.owner.id, 0, 500).unwrap().entries.len();
    let over = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, None, &mut tx)
        .unwrap();
    assert_eq!(over.run, view.run);
    assert_eq!(
        m.runtime.audit(&m.owner.id, 0, 500).unwrap().entries.len(),
        audits
    );

    // 6. Every unfinished run in one request, acting as whoever dispatched each.
    let summary = m
        .runtime
        .runs_reconcile(&m.owner.id, None, &mut tx)
        .unwrap();
    assert!(summary.failed.is_empty());
    assert_eq!(
        summary
            .runs
            .iter()
            .map(|view| (view.run.operation_id.as_str(), disposition(view)))
            .collect::<Vec<_>>(),
        [
            ("op-1", Disposition::Continue),
            ("op-2", Disposition::Continue),
            ("op-3", Disposition::NeedsAttention),
            ("op-4", Disposition::Continue),
        ]
    );
    assert!(summary.runs.iter().all(|view| view.unverified == NOT_WIRED));

    // 7. A target ticket created for the work is never created twice either.
    let create = json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(),
        "operation_id": "create-1", "title": "Synthetic follow-up",
        "actor": {"role":"ai","id":"worker-1"},
    });
    let path = format!("/checkouts/{}/orchestration/tickets/create", m.owner.id);
    let before = m.target_store.list_tickets().unwrap().len();
    let first = m.runtime.request("POST", &path, &create, &mut tx).unwrap();
    let second = m.runtime.request("POST", &path, &create, &mut tx).unwrap();
    assert_eq!(first["ticket"]["native_id"], second["ticket"]["native_id"]);
    assert_eq!(m.target_store.list_tickets().unwrap().len(), before + 1);
}

#[test]
fn hs2_wbredm_an_unreachable_target_is_named_unverified_and_a_pending_cancel_waits_for_it() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    granted(&m, &mut tx);
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Out of sight");
    let run = dispatch(&m, &orchestration, &ticket, "op-1", "worker-a", &mut tx);
    let rebind = lose_target(&m, &mut tx);
    // Not an error: the answer says exactly what could not be verified, and why.
    let dark = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(dark.run.state, RunState::Dispatched);
    assert_eq!(disposition(&dark), Disposition::HoldUnverified);
    assert_eq!(
        dark.unverified,
        ExternalSource::ALL.map(ExternalSource::code)
    );
    for source in ExternalSource::ALL.into_iter().filter(|s| s.is_target()) {
        assert_eq!(
            finding(&dark, source),
            (
                Verification::Unverifiable,
                Some(DenialReason::TargetUnavailable.code().into())
            )
        );
    }
    assert_eq!(
        dark.run.unreachable.as_ref().unwrap().reason,
        "target_unavailable"
    );
    // The last confirmed state is kept as it was, and nothing was done to the claim.
    assert_eq!(dark.run.observed, None);
    assert_eq!(held(&m, &ticket), (Some("worker-a".into()), 1));
    // A cancel that cannot reach the target only records the intent.
    let pending = m
        .runtime
        .run_cancel(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert!(pending.run.cancel_requested);
    // However often it is reconciled meanwhile, the outage and the hold are one event each.
    for _ in 0..3 {
        let still = m
            .runtime
            .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
            .unwrap();
        assert_eq!(disposition(&still), Disposition::HoldUnverified);
        assert!(still.run.cancel_requested);
    }
    let count = |kind| {
        kinds(&m, &run)
            .iter()
            .filter(|event| event.0 == kind)
            .count()
    };
    assert_eq!(count(RunEventKind::TargetUnreachable), 1);
    assert_eq!(count(RunEventKind::Reconciled), 1);
    // A pause works while the target is out of sight, and it is audited.
    let paused = m
        .runtime
        .run_pause(
            &m.owner.id,
            &run.run_id,
            PauseRun {
                reason: Some("target_maintenance".into()),
            },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        paused.run.paused.as_ref().unwrap().reason.as_deref(),
        Some("target_maintenance")
    );
    assert_eq!(
        m.last_audit(),
        (
            "run_pause".into(),
            AuditOutcome::Succeeded,
            Some("target_maintenance".into())
        )
    );
    // The target answers again. A paused run is read but nothing is done for it: the
    // pending cancel is not repeated and the claim stays.
    m.runtime.bind(rebind, &mut tx).unwrap();
    let read = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(disposition(&read), Disposition::Paused);
    assert_eq!(read.unverified, NOT_WIRED);
    assert_eq!(read.run.unreachable, None);
    assert!(read.run.cancel_requested);
    assert_eq!(read.run.observed.as_ref().unwrap().status, "started");
    assert_eq!(held(&m, &ticket), (Some("worker-a".into()), 1));
    // Resuming reconciles at once: the cancel reaches the target and the run ends.
    let resumed = m
        .runtime
        .run_resume(&m.owner.id, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(resumed.run.state, RunState::Cancelled);
    assert!(!resumed.run.cancel_requested);
    assert_eq!(resumed.run.paused, None);
    assert_eq!(resumed.run.reconciliation, None);
    assert_eq!(held(&m, &ticket), (None, 1));
    assert_eq!(
        kinds(&m, &run)
            .into_iter()
            .map(|event| (event.0, event.1.unwrap_or_default()))
            .collect::<Vec<_>>(),
        [
            (RunEventKind::State, "claimed".into()),
            (RunEventKind::TargetUnreachable, "target_unavailable".into()),
            (RunEventKind::Reconciled, "hold_unverified".into()),
            (RunEventKind::CancelRequested, String::new()),
            (RunEventKind::Paused, "target_maintenance".into()),
            (RunEventKind::Observed, "started".into()),
            (RunEventKind::TargetReachable, String::new()),
            (RunEventKind::Reconciled, "paused".into()),
            (RunEventKind::Resumed, "resumed".into()),
            (RunEventKind::State, "cancelled".into()),
        ]
    );
    assert!(m.runtime.run_owners().unwrap().is_empty());
    // An actor without a grant learns nothing about the target, and the answer says so
    // rather than failing.
    let other = target_ticket(&m, "Second");
    let run = dispatch(&m, &orchestration, &other, "op-2", "worker-b", &mut tx);
    let stranger =
        hotsheet_ticketing::actor::MutationActor::parse("ai", Some("stranger".into())).unwrap();
    let denied = m
        .runtime
        .run_reconcile(&m.owner.id, &run.run_id, Some(&stranger), &mut tx)
        .unwrap();
    assert_eq!(disposition(&denied), Disposition::HoldUnverified);
    assert_eq!(
        finding(&denied, ExternalSource::TargetReachability),
        (
            Verification::Unverifiable,
            Some(DenialReason::NoGrant.code().into())
        )
    );
    assert!(matches!(
        m.runtime
            .run_reconcile(&m.owner.id, &id(999), None, &mut tx),
        Err(OrchestrationError::Run(RunError::NotFound))
    ));
}

#[test]
fn hs2_wbredm_pause_resume_handoff_and_correction_are_each_an_audited_transition() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    granted(&m, &mut tx);
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Controlled");
    let run = dispatch(&m, &orchestration, &ticket, "op-1", "worker-a", &mut tx);
    let owner = m.owner.id.clone();
    let pause = |tx: &mut Transaction<'_>| {
        m.runtime.run_pause(
            &owner,
            &run.run_id,
            PauseRun::default(),
            Some(&worker()),
            tx,
        )
    };
    // Control needs someone to answer for it, and a reason is a code.
    assert!(matches!(
        m.runtime
            .run_pause(&owner, &run.run_id, PauseRun::default(), None, &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    assert!(matches!(
        m.runtime.run_pause(
            &owner,
            &run.run_id,
            PauseRun {
                reason: Some("not a code".into())
            },
            Some(&worker()),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    let paused = pause(&mut tx).unwrap();
    assert_eq!(paused.run.state, RunState::Dispatched);
    assert_eq!(
        paused.run.paused.as_ref().unwrap().actor_id.as_deref(),
        Some("worker-1")
    );
    // The target was not told: the claim is where it was.
    assert_eq!(held(&m, &ticket), (Some("worker-a".into()), 1));
    // Pausing again is the same pause.
    assert_eq!(pause(&mut tx).unwrap().run, paused.run);
    // A paused run asks nothing of its target.
    assert_eq!(
        run_error(
            m.runtime
                .run_renew(&owner, &run.run_id, None, Some(&worker()), &mut tx)
        ),
        RunError::Paused
    );
    assert_eq!(
        run_error(m.runtime.run_handoff(
            &owner,
            &run.run_id,
            HandoffRun {
                to_worker: Some("worker-b".into()),
                ..HandoffRun::default()
            },
            Some(&worker()),
            &mut tx
        )),
        RunError::Paused
    );
    let resumed = m
        .runtime
        .run_resume(&owner, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(resumed.run.paused, None);
    assert_eq!(disposition(&resumed), Disposition::Continue);
    assert_eq!(m.last_audit().0, "tickets_read");
    // Pause and resume once more: each is its own entry.
    pause(&mut tx).unwrap();
    let resumed = m
        .runtime
        .run_resume(&owner, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        resumed.run.history[2..]
            .iter()
            .map(|entry| entry.control.unwrap())
            .collect::<Vec<_>>(),
        [
            RunControl::Pause,
            RunControl::Resume,
            RunControl::Pause,
            RunControl::Resume
        ]
    );
    // An interrupted dispatch that was paused is not repeated until it is resumed.
    let waiting = target_ticket(&m, "Paused before dispatch");
    let stalled = interrupted(
        &m,
        &orchestration,
        &waiting,
        "op-2",
        Some("worker-w"),
        &mut tx,
    );
    m.runtime
        .run_pause(
            &owner,
            &stalled.run_id,
            PauseRun::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        run_error(m.runtime.run_dispatch(
            &owner,
            request(
                &m,
                &orchestration,
                &waiting,
                "op-2",
                json!({"worker_id":"worker-w"})
            ),
            Some(&worker()),
            &mut tx
        )),
        RunError::Paused
    );
    assert_eq!(held(&m, &waiting), (None, 0));

    // Handoff to another worker: the run's own claim is released and the ticket is
    // claimed for the new worker.
    let handoff = |to: Option<&str>, tx: &mut Transaction<'_>| {
        m.runtime.run_handoff(
            &owner,
            &run.run_id,
            HandoffRun {
                to_worker: to.map(Into::into),
                worker_label: Some("Worker B".into()),
                lease_minutes: Some(45),
            },
            Some(&worker()),
            tx,
        )
    };
    let handed = handoff(Some("worker-b"), &mut tx).unwrap().run;
    assert_eq!(handed.state, RunState::Dispatched);
    assert_eq!(handed.worker_id.as_deref(), Some("worker-b"));
    let entry = handed.history.last().unwrap();
    assert_eq!(
        (entry.control, entry.reason.as_deref(), entry.from, entry.to),
        (
            Some(RunControl::Handoff),
            Some("handed_off"),
            Some(RunState::Dispatched),
            RunState::Dispatched
        )
    );
    assert_eq!(held(&m, &ticket), (Some("worker-b".into()), 2));
    assert_eq!(
        m.last_audit(),
        ("work_start".into(), AuditOutcome::Succeeded, None)
    );
    // Repeating the handoff adopts the claim it already made: no entry, no claim.
    let again = handoff(Some("worker-b"), &mut tx).unwrap().run;
    assert_eq!(again, handed);
    assert_eq!(held(&m, &ticket).1, 2);
    // Someone else holds the ticket: their claim is never forced away.
    release(&m, &ticket, "worker-b");
    claim(&m, &ticket, "human-1");
    assert_eq!(
        op(handoff(Some("worker-c"), &mut tx)),
        TargetOpError::Conflict
    );
    assert_eq!(held(&m, &ticket).0.as_deref(), Some("human-1"));
    assert_eq!(
        m.last_audit(),
        (
            "work_start".into(),
            AuditOutcome::Failed,
            Some("target_conflict".into())
        )
    );
    // Naming the holder adopts their claim without touching it.
    let adopted = handoff(Some("human-1"), &mut tx).unwrap().run;
    assert_eq!(adopted.worker_id.as_deref(), Some("human-1"));
    assert_eq!(
        adopted.history.last().unwrap().reason.as_deref(),
        Some("handoff_adopted")
    );
    assert_eq!(held(&m, &ticket), (Some("human-1".into()), 3));
    // A worker that already holds a direct run on this target takes no second one.
    let busy = target_ticket(&m, "Busy worker's ticket");
    dispatch(&m, &orchestration, &busy, "op-3", "worker-z", &mut tx);
    assert_eq!(
        run_error(handoff(Some("worker-z"), &mut tx)),
        RunError::WorkerBusy
    );
    // Handing the run to a person lets go of the claim and keeps following the ticket.
    let released = handoff(None, &mut tx).unwrap().run;
    assert_eq!(released.state, RunState::Intervened);
    assert_eq!(
        released.history.last().unwrap().control,
        Some(RunControl::Handoff)
    );
    assert_eq!(held(&m, &ticket).0, None);
    let note = m.owner_store.read_ticket(&orchestration.id).unwrap();
    assert!(note.notes.last().unwrap().text.contains("handed off"));
    // The target's dispatcher chooses its workers: such a run can only adopt.
    let pooled = target_ticket(&m, "Pooled");
    let queued = m
        .runtime
        .run_dispatch(
            &owner,
            request(
                &m,
                &orchestration,
                &pooled,
                "op-4",
                json!({"mode":"dispatcher"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    let pooled_handoff = |to: &str, tx: &mut Transaction<'_>| {
        m.runtime.run_handoff(
            &owner,
            &queued.run_id,
            HandoffRun {
                to_worker: Some(to.into()),
                ..HandoffRun::default()
            },
            Some(&worker()),
            tx,
        )
    };
    assert!(matches!(
        pooled_handoff("worker-q", &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(held(&m, &pooled), (None, 0));
    claim(&m, &pooled, "pool-1");
    let adopted = pooled_handoff("pool-1", &mut tx).unwrap().run;
    assert_eq!(
        (adopted.state, adopted.worker_id.as_deref()),
        (RunState::Dispatched, Some("pool-1"))
    );
    for bad in [
        HandoffRun {
            to_worker: Some("has space".into()),
            ..HandoffRun::default()
        },
        HandoffRun {
            lease_minutes: Some(1),
            ..HandoffRun::default()
        },
    ] {
        assert!(matches!(
            m.runtime
                .run_handoff(&owner, &run.run_id, bad, Some(&worker()), &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }

    // Correction: the run was let go, but the work is to be dispatched again under the
    // same operation. A person moves the record backwards; an AI may not.
    let correct = |state,
                   reason: &str,
                   actor: Option<&hotsheet_ticketing::actor::MutationActor>,
                   tx: &mut Transaction<'_>| {
        m.runtime.run_correct(
            &owner,
            &run.run_id,
            CorrectRun {
                state,
                reason: reason.into(),
                worker_id: Some("worker-a".into()),
            },
            actor,
            tx,
        )
    };
    assert!(matches!(
        correct(RunState::Requested, "redispatch", Some(&worker()), &mut tx),
        Err(OrchestrationError::Forbidden(_))
    ));
    assert!(matches!(
        correct(RunState::Requested, "redispatch", None, &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    assert!(matches!(
        correct(RunState::Requested, "free text", Some(&human()), &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(m.runtime.run(&owner, &run.run_id).unwrap(), released);
    let corrected = correct(RunState::Requested, "redispatch", Some(&human()), &mut tx)
        .unwrap()
        .run;
    assert_eq!(corrected.state, RunState::Requested);
    assert_eq!(corrected.worker_id.as_deref(), Some("worker-a"));
    let entry = corrected.history.last().unwrap();
    assert_eq!(
        (
            entry.control,
            entry.from,
            entry.to,
            entry.actor_id.as_deref()
        ),
        (
            Some(RunControl::Correct),
            Some(RunState::Intervened),
            RunState::Requested,
            Some("maintainer")
        )
    );
    assert_eq!(
        m.last_audit(),
        (
            "run_correct".into(),
            AuditOutcome::Succeeded,
            Some("redispatch".into())
        )
    );
    // The same correction again changes nothing and is not audited twice.
    let audits = m.runtime.audit(&owner, 0, 500).unwrap().entries.len();
    assert_eq!(
        correct(RunState::Requested, "redispatch", Some(&human()), &mut tx)
            .unwrap()
            .run,
        corrected
    );
    assert_eq!(
        m.runtime.audit(&owner, 0, 500).unwrap().entries.len(),
        audits
    );
    // The original dispatch, repeated, now claims again: still the same run.
    let redone = dispatch(&m, &orchestration, &ticket, "op-1", "worker-a", &mut tx);
    assert_eq!(redone.run_id, run.run_id);
    assert_eq!(redone.state, RunState::Dispatched);
    assert_eq!(held(&m, &ticket), (Some("worker-a".into()), 4));
    // Every repeated and reversed step is in the history, in order.
    assert_eq!(
        redone
            .history
            .iter()
            .map(|entry| (entry.to, entry.control.map(RunControl::code)))
            .collect::<Vec<_>>(),
        [
            (RunState::Requested, None),
            (RunState::Dispatched, None),
            (RunState::Dispatched, Some("pause")),
            (RunState::Dispatched, Some("resume")),
            (RunState::Dispatched, Some("pause")),
            (RunState::Dispatched, Some("resume")),
            (RunState::Dispatched, Some("handoff")),
            (RunState::Dispatched, Some("handoff")),
            (RunState::Intervened, Some("handoff")),
            (RunState::Requested, Some("correct")),
            (RunState::Dispatched, None),
        ]
    );
    // And in the event stream, each as what it was.
    let stream: Vec<_> = kinds(&m, &run)
        .into_iter()
        .map(|event| event.0)
        .filter(|kind| {
            !matches!(
                kind,
                RunEventKind::Observed | RunEventKind::Reconciled | RunEventKind::Lease
            )
        })
        .collect();
    use RunEventKind::{Corrected, Handoff, Paused, Resumed, State};
    assert_eq!(
        stream,
        [
            State, Paused, Resumed, Paused, Resumed, Handoff, Handoff, Handoff, Corrected, State
        ]
    );
    // A correction can end a run nothing else can settle; the target is not told.
    let ended = correct(RunState::Cancelled, "abandoned", Some(&human()), &mut tx)
        .unwrap()
        .run;
    assert_eq!(ended.state, RunState::Cancelled);
    assert_eq!(held(&m, &ticket).0.as_deref(), Some("worker-a"));
    let note = m.owner_store.read_ticket(&orchestration.id).unwrap();
    assert!(
        note.notes
            .last()
            .unwrap()
            .text
            .contains("finished by a correction")
    );
    // A finished run stays finished.
    assert_eq!(
        run_error(correct(
            RunState::Dispatched,
            "reopen",
            Some(&human()),
            &mut tx
        )),
        RunError::NotActive
    );
    assert_eq!(run_error(pause(&mut tx)), RunError::NotActive);
    assert_eq!(
        run_error(
            m.runtime
                .run_resume(&owner, &run.run_id, Some(&worker()), &mut tx)
        ),
        RunError::NotActive
    );
    assert_eq!(run_error(handoff(None, &mut tx)), RunError::NotActive);
}

#[test]
fn hs2_wbredm_recovery_routes_reach_the_same_runtime() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    granted(&m, &mut tx);
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Routed");
    let run = dispatch(&m, &orchestration, &ticket, "op-1", "worker-a", &mut tx);
    let runs = format!("/checkouts/{}/orchestration/runs", m.owner.id);
    let all = format!("/checkouts/{}/orchestration/run-reconcile", m.owner.id);
    for path in [
        all.clone(),
        format!("{runs}/{}/reconcile", run.run_id),
        format!("{runs}/{}/pause", run.run_id),
        format!("{runs}/{}/resume", run.run_id),
        format!("{runs}/{}/handoff", run.run_id),
        format!("{runs}/{}/correct", run.run_id),
    ] {
        assert!(is_path(&path), "{path}");
        assert!(is_machine_path(&path), "{path}");
    }
    let ai = json!({"role":"ai","id":"worker-1"});
    let mut post = |operation: &str, mut body: Value, actor: &Value| {
        body["actor"] = actor.clone();
        m.runtime.request(
            "POST",
            &format!("{runs}/{}/{operation}", run.run_id),
            &body,
            &mut tx,
        )
    };
    let reconciled = post("reconcile", json!({}), &ai).unwrap();
    assert_eq!(reconciled["state"], "dispatched");
    assert_eq!(reconciled["reconciliation"]["disposition"], "continue");
    assert_eq!(reconciled["unverified"], json!(NOT_WIRED));
    assert_eq!(
        reconciled["reconciliation"]["findings"][0],
        json!({"source":"target_reachability","verification":"verified"})
    );
    assert_eq!(
        reconciled["reconciliation"]["findings"][6],
        json!({"source":"session","verification":"unverifiable","reason":"source_not_wired"})
    );
    let paused = post("pause", json!({"reason":"maintenance"}), &ai).unwrap();
    assert_eq!(paused["paused"]["reason"], "maintenance");
    assert_eq!(paused["history"][2]["control"], "pause");
    let refused = post("renew", json!({}), &ai).unwrap_err();
    assert_eq!(refused.code(), "run_paused");
    assert_eq!(error_status(&refused), 409);
    let resumed = post("resume", json!({}), &ai).unwrap();
    assert_eq!(resumed["paused"], Value::Null);
    let handed = post(
        "handoff",
        json!({"to_worker":"worker-b","lease_minutes":30}),
        &ai,
    )
    .unwrap();
    assert_eq!(handed["worker_id"], "worker-b");
    let forbidden = post(
        "correct",
        json!({"state":"intervened","reason":"operator_fix"}),
        &ai,
    )
    .unwrap_err();
    assert_eq!(error_status(&forbidden), 403);
    let person = json!({"role":"human","id":"maintainer"});
    let corrected = post(
        "correct",
        json!({"state":"intervened","reason":"operator_fix"}),
        &person,
    )
    .unwrap();
    assert_eq!(corrected["state"], "intervened");
    assert_eq!(
        corrected["history"].as_array().unwrap().last().unwrap()["control"],
        "correct"
    );
    for (operation, body) in [
        ("pause", json!({"because":"unknown field"})),
        ("handoff", json!({"to_worker":7})),
        ("correct", json!({"state":"paused","reason":"x"})),
        ("correct", json!({"state":"queued"})),
    ] {
        assert!(
            matches!(
                post(operation, body, &person),
                Err(OrchestrationError::Invalid(_))
            ),
            "{operation}"
        );
    }
    let summary = m
        .runtime
        .request("POST", &all, &json!({"actor": ai}), &mut tx)
        .unwrap();
    assert_eq!(summary["runs"].as_array().unwrap().len(), 1);
    assert_eq!(summary["runs"][0]["run_id"], run.run_id);
    // The correction said `intervened`, but worker-b still holds a live claim: the
    // next successful read of the target wins, and the reversal is kept in history.
    assert_eq!(summary["runs"][0]["state"], "dispatched");
    assert_eq!(
        summary["runs"][0]["history"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["reason"],
        "claim_confirmed"
    );
    assert_eq!(
        summary["runs"][0]["reconciliation"]["disposition"],
        "continue"
    );
    assert_eq!(summary.get("failed"), None);
    // With nobody named, each run is reconciled as whoever dispatched it.
    let unnamed = m
        .runtime
        .request("POST", &all, &json!({}), &mut tx)
        .unwrap();
    assert_eq!(unnamed["runs"], summary["runs"]);
    // The status view carries the outcome without contacting anything.
    let status = m
        .runtime
        .request(
            "GET",
            &format!("{runs}/{}/status", run.run_id),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        status["reconciliation"],
        summary["runs"][0]["reconciliation"]
    );
}

/// Two resolutions of one target at once (a follower and a request, or the CLI beside
/// the server) are contention on this machine, not an unreachable target.
#[test]
fn hs2_wbredm_a_resolution_raced_by_another_is_retried_rather_than_denied() {
    let f = super::fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let revision = super::declaration(&f, &mut tx);
    let destination = super::register(&f);
    assert!(
        f.runtime
            .bind(super::request(&f, revision, destination), &mut tx)
            .unwrap()
            .usable
    );
    let target = super::reference(&f);
    f.runtime
        .grant(
            &target,
            hotsheet_ticketing::orchestration::CreateGrant {
                grant_id: None,
                actor_id: "worker-1".into(),
                actions: vec![TargetAction::TicketsRead],
                session_scope: None,
                expires_at_ms: 10_000,
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let raced = Arc::new(AtomicUsize::new(0));
    // Another resolution of the same target lands between this one's read of the
    // binding and its write: the first attempt conflicts.
    let race = |times: usize| {
        let (racing, target, raced) = (f.runtime.clone(), target.clone(), raced.clone());
        *f.peer
            .before_metadata
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(move || {
            for round in 0..times {
                let mut rng = Rng(900 + round as u128);
                let resolved = racing.resolve(
                    &target,
                    &mut Transaction {
                        clock: &Clock,
                        rng: &mut rng,
                    },
                );
                assert!(resolved.unwrap().usable);
                raced.fetch_add(1, Ordering::SeqCst);
            }
        }));
    };
    race(1);
    assert!(matches!(
        f.runtime.resolve(&target, &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    assert_eq!(raced.load(Ordering::SeqCst), 1);
    // The gate resolves again instead of reporting the target as unavailable.
    race(2);
    let authorized = f
        .runtime
        .authorize(
            &target,
            Some(&worker()),
            ActionRequest::new(TargetAction::TicketsRead),
            &mut tx,
        )
        .unwrap();
    assert!(authorized.resolution.usable);
    assert_eq!(raced.load(Ordering::SeqCst), 3);
    let entry = f.runtime.audit(&f.owner.id, 0, 500).unwrap().entries;
    assert_eq!(entry.last().unwrap().record.outcome, AuditOutcome::Allowed);
}
