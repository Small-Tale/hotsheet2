//! HS2-42TARA: the plan of prerequisites through the shared runtime against real git
//! ticket stores: canonical ids, the dispatch guard, the person's override, and
//! reevaluation on run, gate and validation changes. Synthetic tickets only.
use hotsheet_model::{NoteKind, Status, Ticket};
use hotsheet_ticketing::orchestration::{
    AuditOutcome, Coverage, GateKind, GateState, Prerequisite, ReadinessError, ReadinessLevel,
    RunEventKind, RunState, TargetAction, TargetOpError,
};
use hotsheet_ticketing::{NewTicket, TicketPatch, ops};
use serde_json::{Value, json};

use super::gates::{CI, Scripted, link, sha, verdict};
use super::tickets::{
    Machine, NOW, bind_and_allow, human, machine, rebind, unbind, unsupported, worker,
};
use super::*;

use super::fixture_program;

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
fn set_status(m: &Machine, ticket: &Ticket, status: Status) {
    ops::update(
        &m.target_store,
        &ticket.id,
        NOW.into(),
        TicketPatch {
            status: Some(status),
            ..Default::default()
        },
    )
    .unwrap();
}
fn requires(m: &Machine, native: &str, gates: &str) -> Value {
    json!({"kind": "ticket", "target_id": id(1), "connection_id": m.target_connection(),
        "native_id": native, "gates": gates})
}
fn define(
    m: &Machine,
    orchestration: &Ticket,
    native: &str,
    prerequisites: Value,
) -> DefinePlanStep {
    serde_json::from_value(json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(), "native_id": native,
        "prerequisites": prerequisites,
    }))
    .unwrap()
}
fn dispatch(
    m: &Machine,
    orchestration: &Ticket,
    native: &str,
    operation: &str,
    extra: Value,
) -> DispatchRun {
    let mut body = json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(),
        "native_id": native, "operation_id": operation, "worker_id": "worker-a",
    });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(body).unwrap()
}
fn refusal<T>(result: Result<T, OrchestrationError>) -> ReadinessError {
    match result {
        Err(OrchestrationError::Readiness(error)) => error,
        Err(other) => panic!("expected a readiness refusal, got {other}"),
        Ok(_) => panic!("expected a readiness refusal"),
    }
}
fn codes(view: &StepView) -> Vec<String> {
    view.readiness
        .reasons
        .iter()
        .map(|reason| reason.code.clone())
        .collect()
}
/// The readiness events of one step: the levels it entered, oldest first.
fn levels(m: &Machine, step_id: &str) -> Vec<String> {
    m.runtime
        .run_events(&m.owner.id, 0, 500, Some(step_id), 0)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| {
            assert_eq!(entry.record.kind, RunEventKind::Readiness);
            assert!(entry.record.state.is_none() && entry.record.gate_id.is_none());
            entry.record.reason.unwrap()
        })
        .collect()
}
fn audited(m: &Machine, action: &str) -> Vec<(AuditOutcome, Option<String>, Option<String>)> {
    m.runtime
        .audit(&m.owner.id, 0, 500)
        .unwrap()
        .entries
        .into_iter()
        .filter(|entry| entry.record.action == action)
        .map(|entry| {
            (
                entry.record.outcome,
                entry.record.reason,
                entry.record.operation_id,
            )
        })
        .collect()
}
fn setup(actions: &[TargetAction], tx: &mut Transaction<'_>) -> (Machine, Ticket) {
    let m = machine(10, false, unsupported);
    bind_and_allow(&m, 1, actions, tx);
    let orchestration = m.orchestration_ticket("Coordinate");
    (m, orchestration)
}
const READ_AND_START: [TargetAction; 2] = [TargetAction::TicketsRead, TargetAction::WorkStart];

#[test]
fn hs2_42tara_define_reads_through_the_gate_and_stores_canonical_ids() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (m, orchestration) = setup(&[TargetAction::TicketsRead], &mut tx);
    let (a, b) = (target_ticket(&m, "First"), target_ticket(&m, "Second"));
    let owner = m.owner.id.clone();
    // Nothing planned: reads answer empty and create no file.
    assert!(
        m.runtime
            .readiness_plan(&owner, &mut tx)
            .unwrap()
            .steps
            .is_empty()
    );
    assert!(m.runtime.ready_work(&owner, &mut tx).unwrap().is_empty());
    assert!(!m.home.join("orchestration-plan.json").exists());

    // Slugs are labels; the plan stores what the provider calls the tickets.
    let view = m
        .runtime
        .readiness_define(
            &owner,
            define(
                &m,
                &orchestration,
                &b.slug,
                json!([requires(&m, &a.slug, "none")]),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let step = view.readiness.step.clone();
    assert_eq!(step.target.native_id, b.id.to_string());
    assert_eq!(step.ticket.native_id, orchestration.id.to_string());
    assert_eq!(step.revision, 1);
    assert_eq!(step.updated_by.as_deref(), Some("worker-1"));
    assert!(matches!(
        &step.prerequisites[0],
        Prerequisite::Ticket { native_id, .. } if *native_id == a.id.to_string()
    ));
    assert_eq!(view.readiness.level, ReadinessLevel::Blocked);
    assert_eq!(codes(&view), ["prerequisite_not_finished"]);
    assert_eq!(view.since_ms, Some(200));
    assert_eq!(levels(&m, &step.step_id), ["blocked"]);
    assert_eq!(
        audited(&m, "readiness_define"),
        [(AuditOutcome::Changed, None, None)]
    );
    assert_eq!(
        m.last_audit(),
        ("tickets_read".into(), AuditOutcome::Succeeded, None)
    );
    // Titles never enter the plan file.
    let stored = std::fs::read_to_string(m.home.join("orchestration-plan.json")).unwrap();
    assert!(!stored.contains("First") && !stored.contains("Second"));

    // The same definition again, by id this time: same step, nothing new recorded.
    let again = m
        .runtime
        .readiness_define(
            &owner,
            define(
                &m,
                &orchestration,
                &b.id.to_string(),
                json!([requires(&m, &a.id.to_string(), "none")]),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(again.readiness.step, step);
    assert_eq!(audited(&m, "readiness_define").len(), 1);
    assert_eq!(levels(&m, &step.step_id), ["blocked"]);
    assert_eq!(
        m.runtime
            .readiness(&owner, &step.step_id, &mut tx)
            .unwrap()
            .readiness,
        again.readiness
    );

    // A ticket the target does not have is not planned, nor is one asked for by an
    // unidentified caller, and neither leaves a step behind.
    assert!(matches!(
        m.runtime.readiness_define(
            &owner,
            define(
                &m,
                &orchestration,
                &a.slug,
                json!([requires(&m, "TGT-NOPE", "none")])
            ),
            Some(&worker()),
            &mut tx,
        ),
        Err(OrchestrationError::TargetOperation(TargetOpError::NotFound))
    ));
    assert!(matches!(
        m.runtime.readiness_define(
            &owner,
            define(&m, &orchestration, &a.slug, json!([])),
            None,
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    // A step cannot require its own ticket, under any name for it.
    assert!(matches!(
        m.runtime.readiness_define(
            &owner,
            define(
                &m,
                &orchestration,
                &a.slug,
                json!([requires(&m, &a.id.to_string(), "ci")])
            ),
            Some(&worker()),
            &mut tx,
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(
        m.runtime
            .readiness_plan(&owner, &mut tx)
            .unwrap()
            .steps
            .len(),
        1
    );

    // An unknown step is a typed 404, and removal needs someone to attribute it to.
    let unknown = PlanStepRef {
        step_id: id(999),
        plan_update: None,
    };
    assert_eq!(
        refusal(m.runtime.readiness(&owner, &unknown.step_id, &mut tx)),
        ReadinessError::StepNotFound
    );
    assert_eq!(
        error_status(&OrchestrationError::Readiness(ReadinessError::StepNotFound)),
        404
    );
    assert_eq!(
        error_status(&OrchestrationError::Readiness(ReadinessError::Blocked)),
        409
    );
    let reference = PlanStepRef {
        step_id: step.step_id.clone(),
        plan_update: None,
    };
    assert!(matches!(
        m.runtime
            .readiness_remove(&owner, &reference, None, &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(
        m.runtime
            .readiness_remove(&owner, &reference, Some(&worker()), &mut tx)
            .unwrap(),
        step
    );
    assert_eq!(
        m.last_audit(),
        ("readiness_remove".into(), AuditOutcome::Changed, None)
    );
    assert_eq!(
        refusal(
            m.runtime
                .readiness_remove(&owner, &reference, Some(&worker()), &mut tx)
        ),
        ReadinessError::StepNotFound
    );
}

#[test]
fn hs2_42tara_only_ready_work_is_dispatched_and_levels_follow_the_target() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (m, orchestration) = setup(&READ_AND_START, &mut tx);
    let (a, b, c) = (
        target_ticket(&m, "First"),
        target_ticket(&m, "Second"),
        target_ticket(&m, "Unplanned"),
    );
    ops::update(
        &m.target_store,
        &b.id,
        NOW.into(),
        TicketPatch {
            up_next: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    let owner = m.owner.id.clone();
    let step_a = m
        .runtime
        .readiness_define(
            &owner,
            define(&m, &orchestration, &a.slug, json!([])),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .readiness
        .step;
    let step_b = m
        .runtime
        .readiness_define(
            &owner,
            define(
                &m,
                &orchestration,
                &b.slug,
                json!([requires(&m, &a.slug, "none")]),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .readiness
        .step;
    // What the scheduler reads: a is ready and unblocks b; b is not listed.
    let ready = m.runtime.ready_work(&owner, &mut tx).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].step_id, step_a.step_id);
    assert_eq!(ready[0].level, ReadinessLevel::Confirmed);
    assert!(ready[0].reasons.is_empty() && ready[0].base_commits.is_empty());
    assert_eq!(ready[0].unblocks, *std::slice::from_ref(&step_b.step_id));
    assert_eq!(ready[0].target, step_a.target);
    assert_eq!(ready[0].priority.as_deref(), Some("default"));
    assert_eq!(ready[0].up_next, Some(false));
    assert!(ready[0].ticket_created_at_ms.is_some());
    assert_eq!(ready[0].ready_since_ms, Some(200));
    let plan = m.runtime.readiness_plan(&owner, &mut tx).unwrap();
    assert_eq!(plan.counts.get("confirmed"), Some(&1));
    assert_eq!(plan.counts.get("blocked"), Some(&1));
    assert!(plan.cycles.is_empty());

    // b is refused with a typed denial; nothing is claimed and no run is recorded.
    assert_eq!(
        refusal(m.runtime.run_dispatch(
            &owner,
            dispatch(&m, &orchestration, &b.slug, "op-b", json!({})),
            Some(&worker()),
            &mut tx,
        )),
        ReadinessError::Blocked
    );
    assert!(m.runtime.run_list(&owner, false).unwrap().is_empty());
    assert_eq!(m.target_store.read_ticket(&b.id).unwrap().claimed_by, None);
    assert_eq!(
        m.last_audit(),
        (
            "work_start".into(),
            AuditOutcome::Failed,
            Some("readiness_blocked".into())
        )
    );
    // A ticket no step works is not planned and is not gated.
    assert_eq!(
        m.runtime
            .run_dispatch(
                &owner,
                dispatch(
                    &m,
                    &orchestration,
                    &c.slug,
                    "op-c",
                    json!({"worker_id": "worker-c"})
                ),
                Some(&worker()),
                &mut tx,
            )
            .unwrap()
            .run
            .state,
        RunState::Dispatched
    );

    // a is dispatched: in progress, published once by the run's own events.
    let run_a = m
        .runtime
        .run_dispatch(
            &owner,
            dispatch(&m, &orchestration, &a.slug, "op-a", json!({})),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    assert_eq!(run_a.state, RunState::Dispatched);
    assert_eq!(levels(&m, &step_a.step_id), ["confirmed", "in_progress"]);
    assert!(m.runtime.ready_work(&owner, &mut tx).unwrap().is_empty());
    // Repeating the admitted dispatch is the same run and is not judged again.
    assert_eq!(
        m.runtime
            .run_dispatch(
                &owner,
                dispatch(&m, &orchestration, &a.slug, "op-a", json!({})),
                Some(&worker()),
                &mut tx,
            )
            .unwrap()
            .run
            .run_id,
        run_a.run_id
    );

    // The target finishes a. The run's sync is the target event: a is finished and
    // b becomes ready without anyone asking for a refresh.
    set_status(&m, &a, Status::Completed);
    m.runtime
        .run_sync(&owner, &run_a.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        levels(&m, &step_a.step_id),
        ["confirmed", "in_progress", "finished"]
    );
    assert_eq!(levels(&m, &step_b.step_id), ["blocked", "confirmed"]);
    let ready = m.runtime.ready_work(&owner, &mut tx).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].step_id, step_b.step_id);
    assert_eq!(ready[0].up_next, Some(true));
    assert!(ready[0].unblocks.is_empty());
    // Reading again publishes nothing: the events are the changes, exactly once.
    m.runtime
        .readiness_refresh(
            &owner,
            &RefreshReadiness::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(levels(&m, &step_b.step_id), ["blocked", "confirmed"]);
    let run_b = m
        .runtime
        .run_dispatch(
            &owner,
            dispatch(
                &m,
                &orchestration,
                &b.slug,
                "op-b",
                json!({"worker_id": "worker-b"}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    assert_eq!(run_b.state, RunState::Dispatched);
    assert_eq!(
        levels(&m, &step_b.step_id),
        ["blocked", "confirmed", "in_progress"]
    );
    // Events of real runs are untouched by the readiness kind.
    assert!(
        m.runtime
            .run_events(&owner, 0, 500, Some(&run_b.run_id), 0)
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.record.kind != RunEventKind::Readiness)
    );
}

#[test]
fn hs2_42tara_a_person_overrides_readiness_for_one_exact_operation() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (m, orchestration) = setup(&READ_AND_START, &mut tx);
    let (a, b) = (target_ticket(&m, "First"), target_ticket(&m, "Second"));
    let owner = m.owner.id.clone();
    let step = m
        .runtime
        .readiness_define(
            &owner,
            define(
                &m,
                &orchestration,
                &b.slug,
                json!([requires(&m, &a.slug, "none")]),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .readiness
        .step;
    let ask = |operation: &str| {
        dispatch(
            &m,
            &orchestration,
            &b.slug,
            operation,
            json!({"override_readiness": true}),
        )
    };
    // HS2-XHQ1EH: the orchestrator's own override is a judgment call. It adds one
    // FEEDBACK NEEDED note with CHOICE options to the orchestration ticket, however
    // often it is asked, and starts nothing.
    let mut notes = Vec::new();
    for _ in 0..2 {
        match m
            .runtime
            .run_dispatch(&owner, ask("op-1"), Some(&worker()), &mut tx)
        {
            Err(OrchestrationError::DecisionRequired { note_id }) => notes.push(note_id),
            other => panic!("expected decision_required, got {:?}", other.err()),
        }
    }
    assert_eq!(notes[0], notes[1]);
    let decision = m
        .owner_store
        .read_ticket(&orchestration.id)
        .unwrap()
        .notes
        .into_iter()
        .filter(|note| note.kind == NoteKind::FeedbackNeeded)
        .collect::<Vec<_>>();
    assert_eq!(decision.len(), 1);
    assert_eq!(decision[0].id.to_string(), notes[0]);
    assert!(decision[0].feedback_request);
    for part in [
        step.step_id.as_str(),
        "`blocked`",
        "op-1",
        "CHOICE:",
        "- Start anyway",
        "- Wait until it is ready",
    ] {
        assert!(decision[0].text.contains(part), "{part}");
    }
    assert!(!decision[0].text.contains("Second"));
    assert!(m.runtime.run_list(&owner, false).unwrap().is_empty());
    let listed = m.runtime.permission_list(&owner, &mut tx).unwrap();
    assert!(listed.requests.is_empty());
    // HS2-0RAZ1J: the wait is listed outside the ticket, by identifiers.
    assert_eq!(listed.decisions.len(), 1);
    assert_eq!(listed.decisions[0].note_id, notes[0]);
    assert_eq!(listed.decisions[0].operation_id, "op-1");
    assert!(listed.decisions[0].answered.is_none());
    assert_eq!(
        audited(&m, "readiness_override"),
        [
            (
                AuditOutcome::Denied,
                Some("decision_required".into()),
                Some("op-1".into())
            ),
            (
                AuditOutcome::Denied,
                Some("decision_required".into()),
                Some("op-1".into())
            ),
        ]
    );
    // A person answers through the existing feedback flow. The answer is for that
    // operation only: a request that does not ask is still refused.
    respond(
        &m,
        &orchestration,
        &notes[0],
        "**Should it start?**\n- A. Start anyway",
    );
    assert_eq!(
        refusal(m.runtime.run_dispatch(
            &owner,
            dispatch(&m, &orchestration, &b.slug, "op-1", json!({})),
            Some(&worker()),
            &mut tx,
        )),
        ReadinessError::Blocked
    );
    let run = m
        .runtime
        .run_dispatch(&owner, ask("op-1"), Some(&worker()), &mut tx)
        .unwrap()
        .run;
    assert_eq!(run.state, RunState::Dispatched);
    // HS2-0RAZ1J: the response that unblocked it is recorded once, with its time.
    let decided = m
        .runtime
        .permission_list(&owner, &mut tx)
        .unwrap()
        .decisions;
    let answered = decided[0].answered.clone().unwrap();
    assert_eq!(answered.option, 0);
    assert_eq!(answered.at_ms, tx.clock.now_ms());
    let response = m
        .owner_store
        .read_ticket(&orchestration.id)
        .unwrap()
        .notes
        .into_iter()
        .find(|note| note.response_to.map(|id| id.to_string()) == Some(notes[0].clone()))
        .unwrap();
    assert_eq!(answered.response_note_id, response.id.to_string());
    assert_eq!(
        audited(&m, "decision_answer"),
        [(
            AuditOutcome::Changed,
            Some("readiness_override".into()),
            Some("op-1".into())
        )]
    );
    assert_eq!(
        audited(&m, "readiness_override").last().unwrap(),
        &(
            AuditOutcome::Allowed,
            Some("blocked".into()),
            Some("op-1".into())
        )
    );
    // The activity note on the orchestration ticket: identifiers and codes only.
    let all = m.owner_store.read_ticket(&orchestration.id).unwrap().notes;
    let note = all
        .iter()
        .find(|note| note.text.starts_with("Readiness override"))
        .unwrap();
    assert_eq!(note.kind, NoteKind::Activity);
    for part in [step.step_id.as_str(), "`blocked`", "op-1"] {
        assert!(note.text.contains(part), "{part}");
    }
    assert!(!note.text.contains("Second"));
    // Admitted: the same request again is the same run, and asks nobody.
    assert_eq!(
        m.runtime
            .run_dispatch(&owner, ask("op-1"), Some(&worker()), &mut tx)
            .unwrap()
            .run
            .run_id,
        run.run_id
    );
    // A person's choice to wait is final for that operation.
    m.runtime
        .run_cancel(&owner, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    let second = match m
        .runtime
        .run_dispatch(&owner, ask("op-2"), Some(&worker()), &mut tx)
    {
        Err(OrchestrationError::DecisionRequired { note_id }) => note_id,
        other => panic!("expected decision_required, got {:?}", other.err()),
    };
    assert_ne!(second, notes[0]);
    respond(&m, &orchestration, &second, "- B. Wait until it is ready");
    assert_eq!(
        refusal(
            m.runtime
                .run_dispatch(&owner, ask("op-2"), Some(&worker()), &mut tx)
        ),
        ReadinessError::OverrideDenied
    );
    assert_eq!(
        audited(&m, "readiness_override").last().unwrap(),
        &(
            AuditOutcome::Denied,
            Some("decision_wait".into()),
            Some("op-2".into())
        )
    );
    // HS2-0RAZ1J: a choice to wait is recorded too; reading it again adds nothing.
    refusal(
        m.runtime
            .run_dispatch(&owner, ask("op-2"), Some(&worker()), &mut tx),
    );
    let decided = m
        .runtime
        .permission_list(&owner, &mut tx)
        .unwrap()
        .decisions;
    assert_eq!(decided.len(), 2);
    assert_eq!(decided[1].note_id, second);
    assert_eq!(decided[1].answered.as_ref().unwrap().option, 1);
    assert_eq!(audited(&m, "decision_answer").len(), 2);
    // A person's own override is their decision: nothing is asked.
    let person = m
        .runtime
        .run_dispatch(&owner, ask("op-person"), Some(&human()), &mut tx)
        .unwrap()
        .run;
    assert_eq!(person.state, RunState::Dispatched);
    m.runtime
        .run_cancel(&owner, &person.run_id, Some(&human()), &mut tx)
        .unwrap();
    // Ready work needs no override and files no request even when one is offered.
    set_status(&m, &a, Status::Completed);
    m.runtime
        .readiness_refresh(
            &owner,
            &RefreshReadiness::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        m.runtime
            .run_dispatch(&owner, ask("op-3"), Some(&worker()), &mut tx)
            .unwrap()
            .run
            .state,
        RunState::Dispatched
    );
    // Nothing more was asked: still two decision notes.
    assert_eq!(
        m.owner_store
            .read_ticket(&orchestration.id)
            .unwrap()
            .notes
            .iter()
            .filter(|note| note.kind == NoteKind::FeedbackNeeded)
            .count(),
        2
    );
}

/// A person's response to a decision note, through the existing feedback flow.
fn respond(m: &Machine, ticket: &Ticket, note_id: &str, text: &str) {
    ops::add_note_with_metadata(
        &m.owner_store,
        &ticket.id,
        ulid::Ulid::new(),
        NOW.into(),
        NoteKind::Regular,
        ops::NoteMetadataInput {
            response_to: Some(note_id.into()),
            actor: Some(human().note_actor()),
            ..ops::NoteMetadataInput::default()
        },
        text.into(),
    )
    .unwrap();
}

#[test]
fn hs2_42tara_gates_move_a_step_from_blocked_through_provisional_to_confirmed() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, orchestration) = setup(
        &[
            TargetAction::TicketsRead,
            TargetAction::WorkStart,
            TargetAction::GatesObserve,
        ],
        &mut tx,
    );
    let ci = Scripted::new(CI, GateKind::Ci);
    m.runtime = m.runtime.clone().with_gate_provider(ci.clone());
    let (a, b) = (target_ticket(&m, "First"), target_ticket(&m, "Second"));
    let owner = m.owner.id.clone();
    let step_b = m
        .runtime
        .readiness_define(
            &owner,
            // `gates` omitted: CI must be green, the default.
            define(
                &m,
                &orchestration,
                &b.slug,
                json!([{"kind": "ticket", "target_id": id(1),
                    "connection_id": m.target_connection(), "native_id": a.slug}]),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .readiness
        .step;
    let show = |m: &Machine, tx: &mut Transaction<'_>| {
        let view = m.runtime.readiness(&owner, &step_b.step_id, tx).unwrap();
        (
            view.readiness.level,
            codes(&view),
            view.readiness.base_commits,
        )
    };
    let none = Vec::<String>::new();
    assert_eq!(
        show(&m, &mut tx),
        (
            ReadinessLevel::Blocked,
            vec!["prerequisite_not_finished".to_owned()],
            none.clone()
        )
    );
    // a is worked by an unplanned run, integrated at a commit, and its gate linked.
    let run = m
        .runtime
        .run_dispatch(
            &owner,
            dispatch(&m, &orchestration, &a.slug, "op-a", json!({})),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    let commit = sha('a');
    m.runtime
        .gate_link(
            &owner,
            &run.run_id,
            link(&commit, CI),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    // Pending gate on a recorded commit: provisional, a distinct answer with its base.
    assert_eq!(
        show(&m, &mut tx),
        (
            ReadinessLevel::Provisional,
            vec!["gate_pending".to_owned()],
            vec![commit.clone()]
        )
    );
    assert_eq!(levels(&m, &step_b.step_id), ["blocked", "provisional"]);
    let ready = m.runtime.ready_work(&owner, &mut tx).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(
        (ready[0].level, &ready[0].base_commits),
        (ReadinessLevel::Provisional, &vec![commit.clone()])
    );
    assert_eq!(ready[0].reasons[0].code, "gate_pending");
    // Provisional dispatch is not built here: it is refused as its own code.
    assert_eq!(
        refusal(m.runtime.run_dispatch(
            &owner,
            dispatch(
                &m,
                &orchestration,
                &b.slug,
                "op-b",
                json!({"worker_id": "worker-b"})
            ),
            Some(&worker()),
            &mut tx,
        )),
        ReadinessError::Provisional
    );
    let sync = |m: &Machine, tx: &mut Transaction<'_>| {
        m.runtime
            .gate_sync(
                &owner,
                &run.run_id,
                &SyncGates::default(),
                Some(&worker()),
                tx,
            )
            .unwrap()
    };
    // The gate fails: blocked. A later attempt passes: provisional until a closes.
    ci.set(
        &commit,
        verdict(CI, GateState::Failed, 1, &commit, Coverage::Exact),
    );
    sync(&m, &mut tx);
    assert_eq!(
        show(&m, &mut tx),
        (
            ReadinessLevel::Blocked,
            vec!["gate_failed".to_owned()],
            none.clone()
        )
    );
    ci.set(
        &commit,
        verdict(CI, GateState::Passed, 2, &commit, Coverage::Exact),
    );
    sync(&m, &mut tx);
    assert_eq!(
        show(&m, &mut tx),
        (
            ReadinessLevel::Provisional,
            vec!["prerequisite_not_finished".to_owned()],
            vec![commit.clone()]
        )
    );
    // Reading the adapter again with nothing new publishes nothing.
    sync(&m, &mut tx);
    assert_eq!(
        levels(&m, &step_b.step_id),
        ["blocked", "provisional", "blocked", "provisional"]
    );
    set_status(&m, &a, Status::Completed);
    m.runtime
        .run_sync(&owner, &run.run_id, Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        show(&m, &mut tx),
        (ReadinessLevel::Confirmed, none.clone(), none.clone())
    );
    assert_eq!(
        levels(&m, &step_b.step_id),
        [
            "blocked",
            "provisional",
            "blocked",
            "provisional",
            "confirmed"
        ]
    );
    // A newer commit linked in the same slot supersedes the green gate: the old
    // verdict no longer counts, and the new pending one makes b provisional again.
    let newer = sha('b');
    m.runtime
        .gate_link(
            &owner,
            &run.run_id,
            link(&newer, CI),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        show(&m, &mut tx),
        (
            ReadinessLevel::Provisional,
            vec!["gate_pending".to_owned()],
            vec![newer.clone()]
        )
    );
    // Naming the old commit pins the prerequisite to it; nothing stands for it now.
    m.runtime
        .readiness_define(
            &owner,
            define(
                &m,
                &orchestration,
                &b.slug,
                json!([{"kind": "ticket", "target_id": id(1),
                    "connection_id": m.target_connection(), "native_id": a.slug,
                    "commit": commit.to_uppercase()}]),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        show(&m, &mut tx),
        (
            ReadinessLevel::Blocked,
            vec!["gate_not_linked".to_owned()],
            none
        )
    );
}

#[test]
fn hs2_42tara_unconfirmed_tickets_are_stale_and_cycles_are_exposed() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (m, orchestration) = setup(&READ_AND_START, &mut tx);
    let (a, b, c) = (
        target_ticket(&m, "First"),
        target_ticket(&m, "Second"),
        target_ticket(&m, "Third"),
    );
    let owner = m.owner.id.clone();
    let plan = |m: &Machine, native: &str, needs: &[&str], tx: &mut Transaction<'_>| {
        let prerequisites: Vec<Value> = needs.iter().map(|n| requires(m, n, "none")).collect();
        m.runtime
            .readiness_define(
                &owner,
                define(m, &orchestration, native, json!(prerequisites)),
                Some(&worker()),
                tx,
            )
            .unwrap()
            .readiness
            .step
    };
    // a requires b, b requires a; c requires a.
    let step_a = plan(&m, &a.slug, &[&b.slug], &mut tx);
    let step_b = plan(&m, &b.slug, &[&a.slug], &mut tx);
    let step_c = plan(&m, &c.slug, &[&a.slug], &mut tx);
    let view = m.runtime.readiness_plan(&owner, &mut tx).unwrap();
    let mut cycle = vec![step_a.step_id.clone(), step_b.step_id.clone()];
    cycle.sort();
    assert_eq!(view.cycles, [cycle.clone()]);
    assert_eq!(view.counts.get("cyclic"), Some(&2));
    let of = |view: &PlanView, step: &hotsheet_ticketing::orchestration::PlanStep| {
        view.steps
            .iter()
            .find(|other| other.readiness.step.step_id == step.step_id)
            .unwrap()
            .clone()
    };
    assert_eq!(of(&view, &step_a).readiness.cycle, cycle);
    assert_eq!(codes(&of(&view, &step_c)), ["prerequisite_in_cycle"]);
    // The step was blocked while b was unplanned and became cyclic when b closed the loop.
    assert_eq!(levels(&m, &step_a.step_id), ["blocked", "cyclic"]);
    assert_eq!(
        refusal(m.runtime.run_dispatch(
            &owner,
            dispatch(&m, &orchestration, &a.slug, "op-a", json!({})),
            Some(&worker()),
            &mut tx,
        )),
        ReadinessError::Cyclic
    );
    assert!(m.runtime.ready_work(&owner, &mut tx).unwrap().is_empty());
    // Dropping one edge dissolves it: b is ready, a waits on b, c on a.
    plan(&m, &b.slug, &[], &mut tx);
    let view = m.runtime.readiness_plan(&owner, &mut tx).unwrap();
    assert!(view.cycles.is_empty());
    assert_eq!(
        of(&view, &step_b).readiness.level,
        ReadinessLevel::Confirmed
    );
    assert_eq!(
        of(&view, &step_b).readiness.unblocks,
        *std::slice::from_ref(&step_a.step_id)
    );
    assert_eq!(codes(&of(&view, &step_c)), ["prerequisite_not_finished"]);

    // The target can no longer be confirmed: every reading is stale, the refresh
    // says what it could not verify, and a dispatch is refused before the claim.
    unbind(&m, 1, &mut tx);
    let refreshed = m
        .runtime
        .readiness_refresh(
            &owner,
            &RefreshReadiness::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(refreshed.unverified.len(), 3);
    assert_eq!(refreshed.plan.counts.get("stale"), Some(&3));
    assert_eq!(codes(&of(&refreshed.plan, &step_b)), ["target_unreachable"]);
    assert_eq!(
        levels(&m, &step_b.step_id),
        ["cyclic", "confirmed", "stale"]
    );
    assert!(m.runtime.ready_work(&owner, &mut tx).unwrap().is_empty());
    // The stale read is kept beside the outage, not thrown away.
    let stored: Value = serde_json::from_str(
        &std::fs::read_to_string(m.home.join("orchestration-plan.json")).unwrap(),
    )
    .unwrap();
    assert!(stored["tickets"].as_array().unwrap().iter().all(|record| {
        record["facts"]["observed"]["status"] == "not_started"
            && record["facts"]["unreachable"]["reason"] == "target_unavailable"
    }));

    // Confirmed again: one step's refresh reads only what that step names.
    rebind(&m, 1, &mut tx);
    let one = m
        .runtime
        .readiness_refresh(
            &owner,
            &RefreshReadiness {
                step_id: Some(step_b.step_id.clone()),
            },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(one.unverified.is_empty());
    assert_eq!(one.plan.steps.len(), 1);
    assert_eq!(one.plan.steps[0].readiness.level, ReadinessLevel::Confirmed);
    let view = m.runtime.readiness_plan(&owner, &mut tx).unwrap();
    // a's own ticket and c's own ticket were not read by b's refresh.
    assert_eq!(of(&view, &step_c).readiness.level, ReadinessLevel::Stale);
    m.runtime
        .readiness_refresh(
            &owner,
            &RefreshReadiness::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        levels(&m, &step_b.step_id),
        ["cyclic", "confirmed", "stale", "confirmed"]
    );

    // A prerequisite ticket deleted in the target blocks, as a fact.
    std::fs::remove_file(m.target_store.ticket_path(&b.id)).unwrap();
    let gone = m
        .runtime
        .readiness_refresh(
            &owner,
            &RefreshReadiness::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(gone.unverified.is_empty());
    assert_eq!(
        codes(&of(&gone.plan, &step_a)),
        ["prerequisite_ticket_missing"]
    );
    assert_eq!(codes(&of(&gone.plan, &step_b)), ["target_ticket_missing"]);

    // Empty, then refill: removing every step leaves nothing to compute or publish.
    for step in [&step_a, &step_b, &step_c] {
        m.runtime
            .readiness_remove(
                &owner,
                &PlanStepRef {
                    step_id: step.step_id.clone(),
                    plan_update: None,
                },
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    }
    assert!(
        m.runtime
            .readiness_plan(&owner, &mut tx)
            .unwrap()
            .steps
            .is_empty()
    );
    assert!(m.runtime.ready_work(&owner, &mut tx).unwrap().is_empty());
    let refilled = plan(&m, &c.slug, &[], &mut tx);
    assert_ne!(refilled.step_id, step_c.step_id);
    assert_eq!(levels(&m, &refilled.step_id), ["confirmed"]);
}

#[test]
fn hs2_42tara_validation_evidence_gates_a_step_and_its_result_reevaluates() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (m, orchestration) = setup(
        &[
            TargetAction::TicketsRead,
            TargetAction::WorkStart,
            TargetAction::ValidationRun,
        ],
        &mut tx,
    );
    let directory = std::path::Path::new(&m.target.root).join(".hotsheet2");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("validation-profiles.json"),
        json!({"schema_version": 1, "profiles": [
            {"name": "smoke", "program": fixture_program::path(), "args": ["--exit", "0"]}]})
        .to_string(),
    )
    .unwrap();
    let a = target_ticket(&m, "First");
    let owner = m.owner.id.clone();
    let step = m
        .runtime
        .readiness_define(
            &owner,
            define(
                &m,
                &orchestration,
                &a.slug,
                // The step's own orchestration ticket when none is named.
                json!([{"kind": "validation", "step_id": "smoke"}]),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(step.readiness.level, ReadinessLevel::Blocked);
    assert_eq!(codes(&step), ["validation_step_missing"]);
    let step_id = step.readiness.step.step_id.clone();
    assert!(matches!(
        &step.readiness.step.prerequisites[0],
        Prerequisite::Validation { ticket, .. } if ticket.native_id == orchestration.id.to_string()
    ));
    let ticket = json!({"native_id": orchestration.id.to_string()});
    m.runtime
        .validation_define(
            &owner,
            serde_json::from_value(json!({"ticket": ticket, "step": {"step_id": "smoke",
                "target_id": id(1), "check": {"kind": "profile", "name": "smoke"},
                "timeout_seconds": 20}}))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let view = m.runtime.readiness(&owner, &step_id, &mut tx).unwrap();
    assert_eq!(codes(&view), ["validation_not_run"]);
    assert_eq!(
        refusal(m.runtime.run_dispatch(
            &owner,
            dispatch(&m, &orchestration, &a.slug, "op-a", json!({})),
            Some(&worker()),
            &mut tx,
        )),
        ReadinessError::Blocked
    );
    // The attempt belongs to no run, so only the readiness hook can publish it.
    for _ in 0..10 {
        let run = m
            .runtime
            .validation_run(
                &owner,
                serde_json::from_value(json!({"ticket": ticket, "step_id": "smoke",
                    "operation_id": "check-1", "wait_ms": MAX_VALIDATION_WAIT_MS}))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
        if run.result.state.is_final() {
            break;
        }
    }
    assert_eq!(
        m.runtime
            .readiness(&owner, &step_id, &mut tx)
            .unwrap()
            .readiness
            .level,
        ReadinessLevel::Confirmed
    );
    assert_eq!(
        levels(&m, &step_id).last().map(String::as_str),
        Some("confirmed")
    );
    assert_eq!(
        levels(&m, &step_id).first().map(String::as_str),
        Some("blocked")
    );
}

#[test]
fn hs2_42tara_routes_are_machine_local_and_share_the_runtime() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (m, orchestration) = setup(&READ_AND_START, &mut tx);
    let (a, b) = (target_ticket(&m, "First"), target_ticket(&m, "Second"));
    let owner = m.owner.id.clone();
    let path = |operation: &str| format!("/checkouts/{owner}/orchestration/readiness/{operation}");
    for operation in ["define", "show", "plan", "ready", "refresh", "remove"] {
        assert!(is_path(&path(operation)), "{operation}");
        assert!(is_machine_path(&path(operation)), "{operation}");
    }
    assert!(!is_path(&format!(
        "/checkouts/{owner}/orchestration/readiness"
    )));
    let actor = json!({"role": "ai", "id": "worker-1"});
    let post = |operation: &str, mut body: Value, tx: &mut Transaction<'_>| {
        body["actor"] = actor.clone();
        m.runtime.request("POST", &path(operation), &body, tx)
    };
    let ticket =
        json!({"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()});
    let first = post(
        "define",
        json!({"ticket": ticket, "target_id": id(1), "connection_id": m.target_connection(),
            "native_id": a.slug}),
        &mut tx,
    )
    .unwrap();
    assert_eq!(first["level"], "confirmed");
    assert_eq!(first["step"]["target"]["native_id"], a.id.to_string());
    let second = post(
        "define",
        json!({"ticket": ticket, "target_id": id(1), "connection_id": m.target_connection(),
            "native_id": b.slug, "prerequisites": [requires(&m, &a.slug, "none")]}),
        &mut tx,
    )
    .unwrap();
    assert_eq!(second["level"], "blocked");
    assert_eq!(second["reasons"][0]["code"], "prerequisite_not_finished");
    assert_eq!(second["reasons"][0]["prerequisite"]["kind"], "ticket");
    let step = second["step"]["step_id"].as_str().unwrap().to_owned();
    assert_eq!(
        post("show", json!({"step_id": step}), &mut tx).unwrap()["since_ms"],
        200
    );
    let plan = post("plan", json!({}), &mut tx).unwrap();
    assert_eq!(plan["steps"].as_array().unwrap().len(), 2);
    assert_eq!(plan["counts"], json!({"blocked": 1, "confirmed": 1}));
    let ready = post("ready", json!({}), &mut tx).unwrap();
    assert_eq!(ready.as_array().unwrap().len(), 1);
    assert_eq!(ready[0]["level"], "confirmed");
    assert_eq!(ready[0]["unblocks"], json!([step]));
    assert_eq!(ready[0]["priority"], "default");
    // The dispatch route carries the typed refusal and accepts the override flag.
    let runs = format!("/checkouts/{owner}/orchestration/runs");
    let body = |extra: Value| {
        let mut body = json!({"actor": actor, "ticket": ticket, "target_id": id(1),
            "connection_id": m.target_connection(), "native_id": b.slug,
            "operation_id": "op-b", "worker_id": "worker-b"});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };
    let refused = m
        .runtime
        .request("POST", &runs, &body(json!({})), &mut tx)
        .unwrap_err();
    assert_eq!(
        (refused.code(), error_status(&refused)),
        ("readiness_blocked", 409)
    );
    let asked = m
        .runtime
        .request(
            "POST",
            &runs,
            &body(json!({"override_readiness": true})),
            &mut tx,
        )
        .unwrap_err();
    assert_eq!(
        (asked.code(), error_status(&asked)),
        ("decision_required", 403)
    );
    let refreshed = post("refresh", json!({"step_id": step}), &mut tx).unwrap();
    assert_eq!(refreshed["steps"].as_array().unwrap().len(), 1);
    assert!(refreshed.get("unverified").is_none());
    // Unknown fields and operations are refused; nothing is guessed.
    for (operation, body) in [
        ("show", json!({"step": step})),
        (
            "define",
            json!({"ticket": ticket, "target_id": id(1), "connection_id": m.target_connection(),
            "native_id": a.slug, "prerequisites": [{"kind": "milestone", "name": "m1"}]}),
        ),
        ("explain", json!({})),
    ] {
        assert!(
            matches!(
                post(operation, body, &mut tx),
                Err(OrchestrationError::Invalid(_))
            ),
            "{operation}"
        );
    }
    assert_eq!(
        post("remove", json!({"step_id": step}), &mut tx).unwrap()["step_id"],
        step
    );
    let gone = post("show", json!({"step_id": step}), &mut tx).unwrap_err();
    assert_eq!(
        (gone.code(), error_status(&gone)),
        ("readiness_step_not_found", 404)
    );
}

/// HS2-J7N3PQ: a running validation attempt that outlives its deadline makes the step
/// waiting on it stale, and that change is published when it happens, by a timer armed
/// for the deadline: not at the next unrelated event, and never on an interval.
#[test]
fn hs2_j7n3pq_a_lapsed_validation_is_published_at_its_deadline() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, orchestration) = setup(
        &[TargetAction::TicketsRead, TargetAction::ValidationRun],
        &mut tx,
    );
    // The timer's wait is recorded and skipped; the instant it was armed for is what the
    // plan is evaluated at.
    let waits = Arc::new(Mutex::new(Vec::<std::time::Duration>::new()));
    let seen = waits.clone();
    m.runtime = m.runtime.clone().with_timer_sleep(Arc::new(move |wait| {
        seen.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(wait);
    }));
    let owner = m.owner.id.clone();
    let a = target_ticket(&m, "First");
    let step_id = m
        .runtime
        .readiness_define(
            &owner,
            define(
                &m,
                &orchestration,
                &a.slug,
                json!([{"kind": "validation", "step_id": "smoke"}]),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .readiness
        .step
        .step_id;
    let ticket = json!({"native_id": orchestration.id.to_string()});
    m.runtime
        .validation_define(
            &owner,
            serde_json::from_value(json!({"ticket": ticket, "step": {"step_id": "smoke",
                "target_id": id(1), "check": {"kind": "profile", "name": "smoke"},
                "timeout_seconds": 20}}))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    // No running attempt: nothing is armed.
    m.runtime.readiness(&owner, &step_id, &mut tx).unwrap();
    assert_eq!(m.runtime.lapse_timer(&owner), None);
    assert!(waits.lock().unwrap().is_empty());
    // An attempt is running (recorded as a request would, outside any run).
    let run_ticket = hotsheet_ticketing::orchestration::RunTicket {
        connection_id: m.owner_connection(),
        native_id: orchestration.id.to_string(),
    };
    let (running, _) = hotsheet_ticketing::orchestration::ValidationRegistry::new(&m.home)
        .begin(
            &owner,
            &run_ticket,
            "smoke",
            hotsheet_ticketing::orchestration::NewResult {
                operation_id: "check-1".into(),
                state: hotsheet_ticketing::orchestration::ValidationState::Running,
                reason: None,
                actor_id: Some("worker-1".into()),
                run_id: None,
            },
            &mut tx,
        )
        .unwrap();
    let deadline = running.deadline_ms.unwrap();
    let before = m.runtime.readiness(&owner, &step_id, &mut tx).unwrap();
    assert_ne!(before.readiness.level, ReadinessLevel::Stale);
    // The read armed the timer for the instant after the deadline; it fires, evaluates
    // the plan then and publishes `stale` with no other event.
    let published = || levels(&m, &step_id);
    let began = std::time::Instant::now();
    while !published().iter().any(|level| level == "stale")
        && began.elapsed() < std::time::Duration::from_secs(10)
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(published().last().map(String::as_str), Some("stale"));
    assert_eq!(
        waits.lock().unwrap().first().copied(),
        Some(std::time::Duration::from_millis(deadline + 1 - 200))
    );
    let event = m
        .runtime
        .run_events(&owner, 0, 500, Some(&step_id), 0)
        .unwrap()
        .entries
        .pop()
        .unwrap();
    assert_eq!(event.record.at_ms, deadline + 1);
    // Nothing further is due: no timer stays armed, and reading again publishes nothing.
    let count = published().len();
    assert_eq!(m.runtime.lapse_timer(&owner), None);
    m.runtime.readiness(&owner, &step_id, &mut tx).unwrap();
    assert_eq!(published().len(), count);
}

/// HS2-061YTB: a manual dispatch of a planned ticket whose step names requirements
/// reads the target's machine through the gate and is refused, naming the
/// requirement, until the machine meets it. A person's override goes ahead and is
/// audited; the orchestrator's waits on a decision. Unplanned tickets and steps
/// without requirements read nothing.
#[test]
fn hs2_061ytb_a_manual_dispatch_checks_its_step_requirements() {
    use hotsheet_ticketing::orchestration::{HostCapabilities, OsFamily};
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let (mut m, orchestration) = setup(&READ_AND_START, &mut tx);
    let probe = std::sync::Arc::new(super::capacity::Scripted::default());
    probe.set(|observation| *observation = super::sessions::roomy());
    m.runtime = m.runtime.clone().with_capacity_probe(probe.clone());
    let owner = m.owner.id.clone();
    let (windows, plain, loose) = (
        target_ticket(&m, "Needs Windows"),
        target_ticket(&m, "Runs anywhere"),
        target_ticket(&m, "Not planned"),
    );
    let mut needs = define(&m, &orchestration, &windows.slug, json!([]));
    needs.requirements = serde_json::from_value(json!({"os": ["windows"]})).unwrap();
    m.runtime
        .readiness_define(&owner, needs, Some(&worker()), &mut tx)
        .unwrap();
    m.runtime
        .readiness_define(
            &owner,
            define(&m, &orchestration, &plain.slug, json!([])),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let refused = |result: Result<RunView, OrchestrationError>| match result {
        Err(error @ OrchestrationError::Requirement { .. }) => (
            error.code(),
            error.refusal_detail().map(|(_, value)| value.to_owned()),
            error_status(&error),
        ),
        Err(other) => panic!("expected a requirement refusal, got {other}"),
        Ok(_) => panic!("expected a requirement refusal"),
    };
    let asked = || probe.asked.load(std::sync::atomic::Ordering::SeqCst);

    // The machine has not said what it is: unknown, never a guess. Repeated, the same.
    for _ in 0..2 {
        assert_eq!(
            refused(m.runtime.run_dispatch(
                &owner,
                dispatch(&m, &orchestration, &windows.slug, "op-w", json!({})),
                Some(&human()),
                &mut tx,
            )),
            ("requirement_unknown", Some("os:windows".into()), 409)
        );
    }
    // A Linux machine misses it.
    probe.set(|observation| {
        observation.capabilities = Some(HostCapabilities::bare(OsFamily::Linux, "arm64"));
    });
    assert_eq!(
        refused(m.runtime.run_dispatch(
            &owner,
            dispatch(&m, &orchestration, &windows.slug, "op-w", json!({})),
            Some(&human()),
            &mut tx,
        )),
        ("requirement_unmet", Some("os:windows".into()), 409)
    );
    assert!(m.runtime.run_list(&owner, false).unwrap().is_empty());
    // The orchestrator's override is a judgment call: it waits on a decision.
    match m.runtime.run_dispatch(
        &owner,
        dispatch(
            &m,
            &orchestration,
            &windows.slug,
            "op-ai",
            json!({"override_requirements": true}),
        ),
        Some(&worker()),
        &mut tx,
    ) {
        Err(OrchestrationError::DecisionRequired { .. }) => {}
        other => panic!("expected decision_required, got {:?}", other.err()),
    }
    assert!(m.runtime.run_list(&owner, false).unwrap().is_empty());
    // A person's override goes ahead, audited with what was overridden.
    let run = m
        .runtime
        .run_dispatch(
            &owner,
            dispatch(
                &m,
                &orchestration,
                &windows.slug,
                "op-person",
                json!({"override_requirements": true}),
            ),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(run.run.operation_id, "op-person");
    assert!(audited(&m, "requirement_override").contains(&(
        AuditOutcome::Allowed,
        Some("requirement_unmet".into()),
        Some("op-person".into())
    )));
    // A repeat of the admitted dispatch is the same run and reads nothing again.
    let before = asked();
    let again = m
        .runtime
        .run_dispatch(
            &owner,
            dispatch(
                &m,
                &orchestration,
                &windows.slug,
                "op-person",
                json!({"override_requirements": true}),
            ),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(again.run.run_id, run.run.run_id);
    assert_eq!(asked(), before);
    // A step without requirements and an unplanned ticket never read the machine.
    for ticket in [&plain, &loose] {
        let before = asked();
        m.runtime
            .run_dispatch(
                &owner,
                dispatch(
                    &m,
                    &orchestration,
                    &ticket.slug,
                    &format!("op-{}", ticket.slug),
                    json!({"worker_id": format!("w-{}", ticket.slug)}),
                ),
                Some(&human()),
                &mut tx,
            )
            .unwrap();
        assert_eq!(asked(), before, "{}", ticket.slug);
    }
    // Over the route the refusal names the requirement too.
    let runs = format!("/checkouts/{owner}/orchestration/runs");
    let mut body = serde_json::to_value(json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
        "target_id": id(1), "connection_id": m.target_connection(),
        "native_id": windows.slug, "operation_id": "op-route", "worker_id": "worker-a",
    }))
    .unwrap();
    body["actor"] = json!({"role": "human", "id": "person-1"});
    let error = m
        .runtime
        .request("POST", &runs, &body, &mut tx)
        .unwrap_err();
    assert_eq!(
        (error.code(), error.refusal_detail(), error_status(&error)),
        (
            "requirement_unmet",
            Some(("requirement", "os:windows")),
            409
        )
    );
    // The machine is Windows now: the plain dispatch starts.
    probe.set(|observation| {
        observation.capabilities = Some(HostCapabilities::bare(OsFamily::Windows, "x86_64"));
    });
    m.runtime
        .run_dispatch(
            &owner,
            dispatch(&m, &orchestration, &windows.slug, "op-w2", json!({})),
            Some(&human()),
            &mut tx,
        )
        .map(|_| ())
        .or_else(|error| match error {
            // The person's run above already holds the ticket; the guard passed.
            OrchestrationError::Run(_) | OrchestrationError::Readiness(_) => Ok(()),
            other => Err(other),
        })
        .unwrap();
}
