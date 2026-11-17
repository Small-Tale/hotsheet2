//! HS2-WN0MXZ: validation steps through the shared runtime against real processes in
//! a real target checkout. Synthetic stores, commands and identities only.
use std::sync::atomic::AtomicBool;

use hotsheet_model::{NoteKind, Ticket};
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, DenialReason, ExecutionEnd, NewResult, PermissionAction, PermissionAnswer,
    ResultUpdate, RunEventKind, RunTicket, TargetAction, ValidationError, ValidationEvidence,
    ValidationRegistry, ValidationState,
};
use hotsheet_ticketing::remote_servers::TargetReply;
use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
use serde_json::{Value, json};

use super::tickets::{
    Machine, NOW, RemotePeer, bind_and_allow, human, machine, unsupported, worker,
};
use super::*;
use crate::orchestration::validation::{
    STEP_EXECUTION_VARIABLE, execute, forget_executions, validation_environment,
};

/// SHA-256 of the two bytes `ok`.
const OK_SHA256: &str = "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df";

/// Write the target project's own profile file: each profile is a shell line.
fn profiles(root: &str, profiles: &[(&str, &str)]) {
    let directory = Path::new(root).join(".hotsheet2");
    std::fs::create_dir_all(&directory).unwrap();
    let profiles: Vec<Value> = profiles
        .iter()
        .map(|(name, line)| json!({"name": name, "program": "/bin/sh", "args": ["-c", line]}))
        .collect();
    std::fs::write(
        directory.join("validation-profiles.json"),
        json!({"schema_version": 1, "profiles": profiles}).to_string(),
    )
    .unwrap();
}
fn ticket_ref(ticket: &Ticket) -> Value {
    json!({"native_id": ticket.id.to_string()})
}
fn define(ticket: &Ticket, step: Value) -> DefineValidation {
    let mut step = step;
    step["target_id"] = id(1).into();
    serde_json::from_value(json!({"ticket": ticket_ref(ticket), "step": step})).unwrap()
}
fn profile(ticket: &Ticket, step: &str, name: &str) -> DefineValidation {
    define(
        ticket,
        json!({"step_id": step, "check": {"kind": "profile", "name": name}, "timeout_seconds": 20}),
    )
}
fn run(ticket: &Ticket, step: &str, operation: &str, wait_ms: u64) -> RunValidation {
    serde_json::from_value(json!({"ticket": ticket_ref(ticket), "step_id": step,
        "operation_id": operation, "wait_ms": wait_ms}))
    .unwrap()
}
fn step_ref(ticket: &Ticket, step: &str) -> ValidationStepRef {
    serde_json::from_value(json!({"ticket": ticket_ref(ticket), "step_id": step})).unwrap()
}
fn validation_error<T>(result: Result<T, OrchestrationError>) -> ValidationError {
    match result {
        Err(OrchestrationError::Validation(error)) => error,
        Err(other) => panic!("expected a validation error, got {other}"),
        Ok(_) => panic!("expected a validation error"),
    }
}
fn denied<T>(result: Result<T, OrchestrationError>) -> DenialReason {
    match result {
        Err(OrchestrationError::Denied(reason)) => reason,
        Err(other) => panic!("expected a denial, got {other}"),
        Ok(_) => panic!("expected a denial"),
    }
}
/// Repeat one request until its attempt is finished; each call waits on the host.
fn until_finished(
    m: &Machine,
    ticket: &Ticket,
    step: &str,
    operation: &str,
    tx: &mut Transaction<'_>,
) -> ValidationRunView {
    for _ in 0..10 {
        let view = m
            .runtime
            .validation_run(
                &m.owner.id,
                run(ticket, step, operation, MAX_VALIDATION_WAIT_MS),
                Some(&worker()),
                tx,
            )
            .unwrap();
        if view.result.state.is_final() {
            return view;
        }
    }
    panic!("the step did not finish");
}
fn evidence(
    m: &Machine,
    ticket: &Ticket,
    step: &str,
    tx: &mut Transaction<'_>,
) -> ValidationEvidence {
    m.runtime
        .validation_evidence(
            &m.owner.id,
            &OrchestrationTicket {
                connection_id: String::new(),
                native_id: ticket.slug.clone(),
            },
            step,
            tx,
        )
        .unwrap()
}

#[test]
fn hs2_wn0mxz_a_profile_step_runs_in_the_target_and_records_references_only() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::ValidationRun], &mut tx);
    // The step proves where it ran by writing into the target checkout.
    profiles(
        &m.target.root,
        &[(
            "smoke",
            "printf ok; printf secret-output >&2; pwd > ran-here; echo x >> runs",
        )],
    );
    let ticket = m.orchestration_ticket("Coordinate");
    let defined = m
        .runtime
        .validation_define(
            &m.owner.id,
            profile(&ticket, "smoke", "smoke"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(defined.state, ValidationState::Unverified);
    assert_eq!(defined.reason.as_deref(), Some("not_run"));
    assert_eq!(defined.step.revision, 1);
    assert_eq!(defined.step.ticket.native_id, ticket.id.to_string());
    assert_eq!(defined.step.updated_by.as_deref(), Some("worker-1"));
    assert_eq!(
        m.last_audit(),
        ("validation_define".into(), AuditOutcome::Changed, None)
    );
    // The identical definition again changes and records nothing more.
    let audited = m.runtime.audit(&m.owner.id, 0, 500).unwrap().entries.len();
    m.runtime
        .validation_define(
            &m.owner.id,
            profile(&ticket, "smoke", "smoke"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        m.runtime.audit(&m.owner.id, 0, 500).unwrap().entries.len(),
        audited,
        "an identical definition records nothing (HS2-XHQ1EH: it asks no gate)"
    );
    let view = until_finished(&m, &ticket, "smoke", "op-1", &mut tx);
    assert_eq!(view.result.state, ValidationState::Passed);
    assert_eq!(view.result.attempt, 1);
    assert_eq!(view.attempts_left, 0);
    assert!(view.unverified.is_empty());
    let outcome = view.result.outcome.clone().unwrap();
    assert_eq!(outcome.end, ExecutionEnd::Exited);
    assert_eq!(outcome.exit_code, Some(0));
    let stdout = outcome.stdout.unwrap();
    assert_eq!((stdout.sha256.as_str(), stdout.bytes), (OK_SHA256, 2));
    assert_eq!(outcome.stderr.unwrap().bytes, 13);
    // It ran in the target checkout, on this (the hosting) machine.
    let ran = std::fs::read_to_string(Path::new(&m.target.root).join("ran-here")).unwrap();
    assert_eq!(
        std::fs::canonicalize(ran.trim()).unwrap(),
        std::fs::canonicalize(&m.target.root).unwrap()
    );
    assert_eq!(
        m.last_audit(),
        ("validation_run".into(), AuditOutcome::Succeeded, None)
    );
    // Evidence is the public read: the same result, with the state it amounts to.
    let read = evidence(&m, &ticket, "smoke", &mut tx);
    assert_eq!(read.state, ValidationState::Passed);
    assert_eq!(read.latest, Some(view.result.clone()));
    assert_eq!(read, view.evidence);
    // No output reached the ticket, the record or the audit journal.
    let source = m.owner_store.read_ticket(&ticket.id).unwrap();
    let note = source.notes.last().unwrap();
    assert_eq!(note.kind, NoteKind::Activity);
    assert!(note.text.contains(&view.result.result_id));
    assert!(note.text.contains("passed"));
    let record = std::fs::read_to_string(m.home.join("orchestration-validation.json")).unwrap();
    let audit = serde_json::to_string(&m.runtime.audit(&m.owner.id, 0, 500).unwrap()).unwrap();
    for text in [note.text.as_str(), record.as_str(), audit.as_str()] {
        assert!(!text.contains("secret-output"));
        assert!(!text.contains("printf"));
    }
    // The same request again is the same result: nothing runs twice.
    let again = m
        .runtime
        .validation_run(
            &m.owner.id,
            run(&ticket, "smoke", "op-1", 0),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(again.result, view.result);
    let runs = || std::fs::read_to_string(Path::new(&m.target.root).join("runs")).unwrap();
    assert_eq!(runs().lines().count(), 1);
    assert_eq!(
        m.owner_store.read_ticket(&ticket.id).unwrap().notes.len(),
        source.notes.len()
    );
    // A new request runs again and becomes the latest evidence.
    let second = until_finished(&m, &ticket, "smoke", "op-2", &mut tx);
    assert_ne!(second.result.result_id, view.result.result_id);
    assert_eq!(runs().lines().count(), 2);
    assert_eq!(evidence(&m, &ticket, "smoke", &mut tx).history.len(), 2);
    // Listing covers the ticket or the whole project.
    let ticket_ref = OrchestrationTicket {
        connection_id: m.owner_connection(),
        native_id: ticket.id.to_string(),
    };
    assert_eq!(
        m.runtime
            .validation_list(&m.owner.id, Some(&ticket_ref), &mut tx)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        m.runtime
            .validation_list(&m.owner.id, None, &mut tx)
            .unwrap()
            .len(),
        1
    );
    let other = m.orchestration_ticket("Another");
    let other_ref = OrchestrationTicket {
        connection_id: String::new(),
        native_id: other.id.to_string(),
    };
    assert!(
        m.runtime
            .validation_list(&m.owner.id, Some(&other_ref), &mut tx)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hs2_wn0mxz_failed_steps_retry_as_allowed_and_expected_evidence_decides() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::ValidationRun], &mut tx);
    profiles(
        &m.target.root,
        &[
            // Fails the first time it runs and passes after that.
            (
                "flaky",
                "if [ -f seen ]; then exit 0; else touch seen; exit 3; fi",
            ),
            ("three", "exit 3"),
            ("says-ok", "printf ok"),
            ("says-no", "printf no"),
        ],
    );
    let ticket = m.orchestration_ticket("Coordinate");
    let step = |name: &str, extra: Value| {
        let mut step = json!({"step_id": name, "check": {"kind": "profile", "name": name},
            "timeout_seconds": 20});
        step.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        define(&ticket, step)
    };
    for definition in [
        step("flaky", json!({"retry": {"max_attempts": 2}})),
        step("three", json!({"evidence": {"exit_code": 3}})),
        step("says-ok", json!({"evidence": {"stdout_sha256": OK_SHA256}})),
        step("says-no", json!({"evidence": {"stdout_sha256": OK_SHA256}})),
    ] {
        m.runtime
            .validation_define(&m.owner.id, definition, Some(&worker()), &mut tx)
            .unwrap();
    }
    let first = until_finished(&m, &ticket, "flaky", "op-1", &mut tx);
    assert_eq!(first.result.state, ValidationState::Failed);
    assert_eq!(first.result.reason.as_deref(), Some("exit_code_mismatch"));
    assert_eq!(first.result.outcome.as_ref().unwrap().exit_code, Some(3));
    assert_eq!(first.attempts_left, 1);
    assert_eq!(
        m.last_audit(),
        (
            "validation_run".into(),
            AuditOutcome::Failed,
            Some("failed".into())
        )
    );
    // The same request takes the next attempt, which is a separate immutable result.
    let second = until_finished(&m, &ticket, "flaky", "op-1", &mut tx);
    assert_eq!(
        (second.result.attempt, second.result.state),
        (2, ValidationState::Passed)
    );
    assert_ne!(second.result.result_id, first.result.result_id);
    assert_eq!(second.attempts_left, 0);
    let history = evidence(&m, &ticket, "flaky", &mut tx).history;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0], first.result);
    // Expected evidence decides: a nonzero code can be the pass, and the output digest
    // can be the failure.
    assert_eq!(
        until_finished(&m, &ticket, "three", "op-2", &mut tx)
            .result
            .state,
        ValidationState::Passed
    );
    assert_eq!(
        until_finished(&m, &ticket, "says-ok", "op-3", &mut tx)
            .result
            .state,
        ValidationState::Passed
    );
    let mismatch = until_finished(&m, &ticket, "says-no", "op-4", &mut tx);
    assert_eq!(mismatch.result.state, ValidationState::Failed);
    assert_eq!(mismatch.result.reason.as_deref(), Some("output_mismatch"));
    assert_eq!(mismatch.attempts_left, 0);
    // An operation id belongs to one step.
    assert!(matches!(
        m.runtime.validation_run(
            &m.owner.id,
            run(&ticket, "three", "op-1", 0),
            Some(&worker()),
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
}

#[test]
fn hs2_wn0mxz_a_command_step_waits_for_a_person_and_a_profile_step_does_not() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::ValidationRun], &mut tx);
    profiles(&m.target.root, &[("smoke", "exit 0")]);
    let ticket = m.orchestration_ticket("Coordinate");
    m.runtime
        .validation_define(
            &m.owner.id,
            profile(&ticket, "smoke", "smoke"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    // Defining a command step asks nobody: nothing runs yet.
    m.runtime
        .validation_define(
            &m.owner.id,
            define(
                &ticket,
                json!({"step_id": "custom", "timeout_seconds": 20, "retry": {"max_attempts": 2},
                    "check": {"kind": "command", "program": "/bin/sh",
                        "args": ["-c", "echo x >> custom-runs; [ -f allow ]"]}}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let requests = |m: &Machine, tx: &mut Transaction<'_>| {
        m.runtime
            .permission_list(&m.owner.id, tx)
            .unwrap()
            .requests
            .into_iter()
            .map(|view| view.request)
            .collect::<Vec<_>>()
    };
    assert!(requests(&m, &mut tx).is_empty());
    // No policy is stored, and a remembered rule covers validation: a profile the
    // target configured runs at once.
    assert_eq!(
        until_finished(&m, &ticket, "smoke", "op-profile", &mut tx)
            .result
            .state,
        ValidationState::Passed
    );
    assert!(requests(&m, &mut tx).is_empty());
    // A command chosen by the orchestration project is outside what the target
    // configured: a person is asked for each operation, whatever is remembered.
    let custom = |operation: &str| run(&ticket, "custom", operation, MAX_VALIDATION_WAIT_MS);
    let runs = Path::new(&m.target.root).join("custom-runs");
    for _ in 0..2 {
        permission_request(m.runtime.validation_run(
            &m.owner.id,
            custom("op-1"),
            Some(&worker()),
            &mut tx,
        ));
    }
    assert!(!runs.exists());
    assert_eq!(
        evidence(&m, &ticket, "custom", &mut tx).reason.as_deref(),
        Some("not_run")
    );
    let mut pending = requests(&m, &mut tx);
    assert_eq!(pending.len(), 1, "asking again files nothing more");
    let pending = pending.remove(0);
    assert_eq!(pending.key.action, PermissionAction::ValidationRun);
    assert_eq!(pending.operation_id, "op-1");
    assert!(pending.each_time);
    // The AI it governs cannot answer.
    assert!(matches!(
        m.runtime.permission_answer(
            &m.owner.id,
            &pending.request_id,
            PermissionAnswer::AllowOnce,
            Some(&worker()),
            &mut tx
        ),
        Err(OrchestrationError::Forbidden(_))
    ));
    answer(
        &m.runtime,
        &m.owner.id,
        &pending.request_id,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    let first = until_finished(&m, &ticket, "custom", "op-1", &mut tx);
    assert_eq!(first.result.state, ValidationState::Failed);
    assert_eq!(first.attempts_left, 1);
    // The retry continues the allowed request; nobody is asked again.
    std::fs::write(Path::new(&m.target.root).join("allow"), "").unwrap();
    let second = until_finished(&m, &ticket, "custom", "op-1", &mut tx);
    assert_eq!(
        (second.result.attempt, second.result.state),
        (2, ValidationState::Passed)
    );
    assert_eq!(std::fs::read_to_string(&runs).unwrap().lines().count(), 2);
    assert_eq!(requests(&m, &mut tx).len(), 1);
    // A new request is a new decision, and a refusal runs nothing.
    let asked = permission_request(m.runtime.validation_run(
        &m.owner.id,
        custom("op-2"),
        Some(&worker()),
        &mut tx,
    ));
    answer(
        &m.runtime,
        &m.owner.id,
        &asked,
        PermissionAnswer::Deny,
        &mut tx,
    );
    assert_eq!(
        denied(
            m.runtime
                .validation_run(&m.owner.id, custom("op-2"), Some(&worker()), &mut tx)
        ),
        DenialReason::PermissionDenied
    );
    assert_eq!(std::fs::read_to_string(&runs).unwrap().lines().count(), 2);
    assert_eq!(
        evidence(&m, &ticket, "custom", &mut tx).latest,
        Some(second.result)
    );
}

#[test]
fn hs2_xhq1eh_running_a_step_asks_and_its_own_records_need_only_an_actor() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    // A remembered dispatch rule says nothing about validation.
    bind_and_allow(&m, 1, &[TargetAction::WorkStart], &mut tx);
    profiles(&m.target.root, &[("smoke", "echo x >> runs")]);
    let ticket = m.orchestration_ticket("Coordinate");
    // HS2-TZT505 / HS2-BJE1C1: defining a step changes only the orchestration
    // project's own record: it needs an identified actor and asks nobody.
    assert!(matches!(
        m.runtime.validation_define(
            &m.owner.id,
            profile(&ticket, "smoke", "smoke"),
            None,
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    let validator = MutationActor::parse("ai", Some("validator".into())).unwrap();
    m.runtime
        .validation_define(
            &m.owner.id,
            profile(&ticket, "smoke", "smoke"),
            Some(&validator),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        m.last_audit(),
        ("validation_define".into(), AuditOutcome::Changed, None)
    );
    // The step exists, but running it asks a person, and nothing runs meanwhile.
    permission_request(m.runtime.validation_run(
        &m.owner.id,
        run(&ticket, "smoke", "op-1", 0),
        Some(&worker()),
        &mut tx,
    ));
    assert!(!Path::new(&m.target.root).join("runs").exists());
    assert_eq!(evidence(&m, &ticket, "smoke", &mut tx).history.len(), 0);
    // Unknown steps, tickets and waits are refused before anything is asked.
    assert_eq!(
        validation_error(m.runtime.validation_run(
            &m.owner.id,
            run(&ticket, "nope", "op-1", 0),
            Some(&validator),
            &mut tx
        )),
        ValidationError::StepNotFound
    );
    assert!(matches!(
        m.runtime.validation_run(
            &m.owner.id,
            run(&ticket, "smoke", "op-1", MAX_VALIDATION_WAIT_MS + 1),
            Some(&validator),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    let mut unknown_run = run(&ticket, "smoke", "op-1", 0);
    unknown_run.run_id = Some(id(777));
    assert!(matches!(
        m.runtime
            .validation_run(&m.owner.id, unknown_run, Some(&validator), &mut tx),
        Err(OrchestrationError::Run(_))
    ));
    // A step is removed with the target gone: only the project's own record changes.
    OrchestrationRuntime::unbind_at(
        &m.home,
        UnbindTarget {
            target: m.reference(1),
            expected_binding_revision: 1,
            previous_destination: m
                .runtime
                .resolve(&m.reference(1), &mut tx)
                .unwrap()
                .binding
                .destination,
        },
        &mut tx,
    )
    .unwrap();
    // An unusable target is a denial, never a recorded result.
    assert_eq!(
        denied(m.runtime.validation_run(
            &m.owner.id,
            run(&ticket, "smoke", "op-1", 0),
            Some(&validator),
            &mut tx
        )),
        DenialReason::TargetUnavailable
    );
    assert_eq!(evidence(&m, &ticket, "smoke", &mut tx).history.len(), 0);
    let removed = m
        .runtime
        .validation_remove(
            &m.owner.id,
            step_ref(&ticket, "smoke"),
            Some(&validator),
            &mut tx,
        )
        .unwrap();
    assert_eq!(removed.step.definition.step_id, "smoke");
    assert_eq!(
        m.last_audit(),
        ("validation_remove".into(), AuditOutcome::Changed, None)
    );
    assert!(
        m.runtime
            .validation_list(&m.owner.id, None, &mut tx)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        validation_error(m.runtime.validation_remove(
            &m.owner.id,
            step_ref(&ticket, "smoke"),
            Some(&human()),
            &mut tx
        )),
        ValidationError::StepNotFound
    );
}

#[test]
fn hs2_wn0mxz_skipped_unavailable_and_timed_out_are_told_apart() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::ValidationRun], &mut tx);
    profiles(
        &m.target.root,
        &[("build", "[ -f built ]"), ("deploy", "echo x >> deploys")],
    );
    // A profile whose program does not exist on the host, beside the shell ones.
    let path = Path::new(&m.target.root).join(".hotsheet2/validation-profiles.json");
    let mut file: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    file["profiles"].as_array_mut().unwrap().extend([
        json!({"name": "missing-program", "program": "hotsheet-no-such-program-wn0mxz"}),
        json!({"name": "slow", "program": "/bin/sh", "args": ["-c", "sleep 30"]}),
        json!({"name": "script", "program": "scripts/check.sh", "args": ["one"]}),
    ]);
    std::fs::write(&path, file.to_string()).unwrap();
    let ticket = m.orchestration_ticket("Coordinate");
    let mut deploy = profile(&ticket, "deploy", "deploy");
    deploy.step.prerequisites = vec!["build".into()];
    let mut slow = profile(&ticket, "slow", "slow");
    slow.step.timeout_seconds = 1;
    for definition in [
        deploy,
        profile(&ticket, "build", "build"),
        profile(&ticket, "undefined", "not-in-the-file"),
        profile(&ticket, "missing-program", "missing-program"),
        profile(&ticket, "script", "script"),
        slow,
    ] {
        m.runtime
            .validation_define(&m.owner.id, definition, Some(&worker()), &mut tx)
            .unwrap();
    }
    // Skipped: the prerequisite never ran, then failed. Nothing is executed.
    let deploys = Path::new(&m.target.root).join("deploys");
    let skipped = m
        .runtime
        .validation_run(
            &m.owner.id,
            run(&ticket, "deploy", "op-d1", 0),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(skipped.result.state, ValidationState::Skipped);
    assert_eq!(
        skipped.result.reason.as_deref(),
        Some("prerequisite_not_passed")
    );
    assert!(skipped.result.outcome.is_none());
    assert_eq!(
        m.last_audit(),
        (
            "validation_run".into(),
            AuditOutcome::Failed,
            Some("skipped".into())
        )
    );
    assert_eq!(
        until_finished(&m, &ticket, "build", "op-b1", &mut tx)
            .result
            .state,
        ValidationState::Failed
    );
    assert_eq!(
        until_finished(&m, &ticket, "deploy", "op-d2", &mut tx)
            .result
            .state,
        ValidationState::Skipped
    );
    assert!(!deploys.exists());
    // Once the prerequisite passes, a new request runs; the skipped ones stay skipped.
    std::fs::write(Path::new(&m.target.root).join("built"), "").unwrap();
    assert_eq!(
        until_finished(&m, &ticket, "build", "op-b2", &mut tx)
            .result
            .state,
        ValidationState::Passed
    );
    assert_eq!(
        until_finished(&m, &ticket, "deploy", "op-d1", &mut tx)
            .result
            .state,
        ValidationState::Skipped
    );
    assert!(!deploys.exists());
    assert_eq!(
        until_finished(&m, &ticket, "deploy", "op-d3", &mut tx)
            .result
            .state,
        ValidationState::Passed
    );
    assert_eq!(
        std::fs::read_to_string(&deploys).unwrap().lines().count(),
        1
    );
    // Unavailable: the target offers no such profile, or its program is not there.
    let undefined = until_finished(&m, &ticket, "undefined", "op-u1", &mut tx);
    assert_eq!(undefined.result.state, ValidationState::Unavailable);
    assert_eq!(
        undefined.result.reason.as_deref(),
        Some("profile_not_defined")
    );
    assert_eq!(
        m.last_audit(),
        (
            "validation_run".into(),
            AuditOutcome::Failed,
            Some("unavailable".into())
        )
    );
    let missing = until_finished(&m, &ticket, "missing-program", "op-m1", &mut tx);
    assert_eq!(missing.result.state, ValidationState::Unavailable);
    assert_eq!(missing.result.reason.as_deref(), Some("program_not_found"));
    // A relative program with a directory part is the checkout's own script.
    assert_eq!(
        until_finished(&m, &ticket, "script", "op-s1", &mut tx)
            .result
            .reason
            .as_deref(),
        Some("program_not_found")
    );
    let scripts = Path::new(&m.target.root).join("scripts");
    std::fs::create_dir(&scripts).unwrap();
    std::fs::write(scripts.join("check.sh"), "#!/bin/sh\n[ \"$1\" = one ]\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            scripts.join("check.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    assert_eq!(
        until_finished(&m, &ticket, "script", "op-s2", &mut tx)
            .result
            .state,
        ValidationState::Passed
    );
    // Failed: the step ran past its timeout and was stopped.
    let started = std::time::Instant::now();
    let slow = until_finished(&m, &ticket, "slow", "op-t1", &mut tx);
    assert_eq!(slow.result.state, ValidationState::Failed);
    assert_eq!(slow.result.reason.as_deref(), Some("timed_out"));
    let outcome = slow.result.outcome.unwrap();
    assert_eq!(
        (outcome.end, outcome.exit_code),
        (ExecutionEnd::TimedOut, None)
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(15));
    // A damaged profile file is a refusal by the target, not a guessed command.
    std::fs::write(&path, "{not json").unwrap();
    let damaged = until_finished(&m, &ticket, "build", "op-b3", &mut tx);
    assert_eq!(damaged.result.state, ValidationState::Unavailable);
    assert_eq!(
        damaged.result.reason.as_deref(),
        Some("corrupt_orchestration_data")
    );
}

#[test]
fn hs2_wn0mxz_a_running_step_is_reattached_reported_to_its_run_and_never_run_twice() {
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
            TargetAction::ValidationRun,
            TargetAction::WorkStart,
            TargetAction::TicketsRead,
        ],
        &mut tx,
    );
    // The step waits for a file, so the test decides when it ends.
    profiles(
        &m.target.root,
        &[(
            "gated",
            "echo x >> starts; while [ ! -f go ]; do sleep 0.05; done",
        )],
    );
    let ticket = m.orchestration_ticket("Coordinate");
    let target_ticket = ops::create(
        &m.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: "Target work".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let dispatched = m
        .runtime
        .run_dispatch(
            &m.owner.id,
            serde_json::from_value(json!({
                "ticket": ticket_ref(&ticket), "target_id": id(1),
                "connection_id": m.target_connection(), "native_id": target_ticket.id.to_string(),
                "operation_id": "dispatch-1", "worker_id": "worker-a",
            }))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run;
    m.runtime
        .validation_define(
            &m.owner.id,
            profile(&ticket, "gated", "gated"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let mut request = run(&ticket, "gated", "op-1", 0);
    request.run_id = Some(dispatched.run_id.clone());
    let started = m
        .runtime
        .validation_run(&m.owner.id, request.clone(), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(started.result.state, ValidationState::Running);
    assert!(started.result.host_started);
    assert!(started.result.outcome.is_none());
    assert_eq!(started.evidence.state, ValidationState::Running);
    assert_eq!(
        started.result.run_id.as_deref(),
        Some(dispatched.run_id.as_str())
    );
    // Repeats wait on the same execution, and another request is refused meanwhile.
    for wait_ms in [0, 50] {
        let mut again = request.clone();
        again.wait_ms = Some(wait_ms);
        let again = m
            .runtime
            .validation_run(&m.owner.id, again, Some(&worker()), &mut tx)
            .unwrap();
        assert_eq!(again.result, started.result);
    }
    assert_eq!(
        validation_error(m.runtime.validation_run(
            &m.owner.id,
            run(&ticket, "gated", "op-2", 0),
            Some(&worker()),
            &mut tx
        )),
        ValidationError::StepRunning
    );
    assert_eq!(
        validation_error(m.runtime.validation_remove(
            &m.owner.id,
            step_ref(&ticket, "gated"),
            Some(&human()),
            &mut tx
        )),
        ValidationError::StepRunning
    );
    std::fs::write(Path::new(&m.target.root).join("go"), "").unwrap();
    let finished = until_finished(&m, &ticket, "gated", "op-1", &mut tx);
    assert_eq!(finished.result.state, ValidationState::Passed);
    assert_eq!(finished.result.result_id, started.result.result_id);
    let starts = std::fs::read_to_string(Path::new(&m.target.root).join("starts")).unwrap();
    assert_eq!(starts.lines().count(), 1);
    // The run's event stream reports the validation once per change, codes only.
    let events: Vec<_> = m
        .runtime
        .run_events(&m.owner.id, 0, 100, Some(&dispatched.run_id), 0)
        .unwrap()
        .entries
        .into_iter()
        .filter(|entry| entry.record.kind == RunEventKind::Validation)
        .map(|entry| (entry.record.reason.unwrap(), entry.record.dedupe.unwrap()))
        .collect();
    assert_eq!(
        events,
        [
            (
                "running".to_owned(),
                format!("{}.running", started.result.result_id)
            ),
            (
                "passed".to_owned(),
                format!("{}.passed", started.result.result_id)
            ),
        ]
    );
    // An attempt the host accepted and no longer knows cannot be confirmed either way,
    // and is not run a second time.
    let registry = ValidationRegistry::new(&m.home);
    let key = RunTicket {
        connection_id: m.owner_connection(),
        native_id: ticket.id.to_string(),
    };
    let (lost, _) = registry
        .begin(
            &m.owner.id,
            &key,
            "gated",
            NewResult {
                operation_id: "op-lost".into(),
                state: ValidationState::Running,
                reason: None,
                actor_id: Some("worker-1".into()),
                run_id: None,
            },
            &mut tx,
        )
        .unwrap();
    registry
        .finish(
            &m.owner.id,
            &key,
            "gated",
            &lost.result_id,
            ResultUpdate {
                host_started: Some(true),
                ..ResultUpdate::default()
            },
            &mut tx,
        )
        .unwrap();
    let unverified = m
        .runtime
        .validation_run(
            &m.owner.id,
            run(&ticket, "gated", "op-lost", 0),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(unverified.result.state, ValidationState::Unverified);
    assert_eq!(unverified.result.reason.as_deref(), Some("execution_lost"));
    assert_eq!(unverified.evidence.state, ValidationState::Unverified);
    assert_eq!(
        m.last_audit(),
        (
            "validation_run".into(),
            AuditOutcome::Failed,
            Some("unverified".into())
        )
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(&m.target.root).join("starts"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn hs2_wn0mxz_the_hosted_half_runs_an_execution_id_once_and_bounds_what_it_reads() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let start = |n: u128, line: &str, wait_ms: u64| HostedValidationStart {
        execution_id: id(n),
        check: hotsheet_ticketing::orchestration::ValidationCheck::Command {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), line.into()],
        },
        timeout_seconds: 20,
        wait_ms,
    };
    // Unknown until started; the same id again is the same execution.
    let unknown = m
        .runtime
        .hosted_validation(&m.target.id, &id(1), 0)
        .unwrap();
    assert_eq!(unknown.state, HostedExecutionState::Unknown);
    let first = m
        .runtime
        .hosted_validation_start(
            &m.target.id,
            start(1, "echo x >> hosted-runs; exit 4", 8_000),
            &mut tx,
        )
        .unwrap();
    assert_eq!(first.state, HostedExecutionState::Finished);
    assert_eq!(first.outcome.as_ref().unwrap().exit_code, Some(4));
    assert_eq!(first.outcome.as_ref().unwrap().started_at_ms, 200);
    // Even a different command under the same id starts nothing.
    let again = m
        .runtime
        .hosted_validation_start(
            &m.target.id,
            start(1, "echo y >> hosted-runs", 8_000),
            &mut tx,
        )
        .unwrap();
    assert_eq!(again.outcome, first.outcome);
    let read = m
        .runtime
        .hosted_validation(&m.target.id, &id(1), 0)
        .unwrap();
    assert_eq!(read.outcome, first.outcome);
    assert_eq!(
        std::fs::read_to_string(Path::new(&m.target.root).join("hosted-runs")).unwrap(),
        "x\n"
    );
    // The same id under another checkout of this machine is another execution.
    assert_eq!(
        m.runtime
            .hosted_validation(&m.owner.id, &id(1), 0)
            .unwrap()
            .state,
        HostedExecutionState::Unknown
    );
    // Requests are validated before anything starts.
    for bad in [
        HostedValidationStart {
            execution_id: "not-a-ulid".into(),
            ..start(2, "true", 0)
        },
        HostedValidationStart {
            timeout_seconds: 0,
            ..start(2, "true", 0)
        },
        HostedValidationStart {
            timeout_seconds: 3601,
            ..start(2, "true", 0)
        },
        start(2, "true", MAX_VALIDATION_WAIT_MS + 1),
    ] {
        assert!(matches!(
            m.runtime
                .hosted_validation_start(&m.target.id, bad, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    assert!(
        m.runtime
            .hosted_validation(&m.target.id, "nope", 0)
            .is_err()
    );
    assert!(
        m.runtime
            .hosted_validation(&m.target.id, &id(1), 8_001)
            .is_err()
    );
    assert!(
        m.runtime
            .hosted_validation_start("unknown-checkout", start(2, "true", 0), &mut tx)
            .is_err()
    );
    assert_eq!(
        m.runtime
            .hosted_validation(&m.target.id, &id(2), 0)
            .unwrap()
            .state,
        HostedExecutionState::Unknown
    );
    // At most four executions run at once; a fifth is refused and never remembered.
    let gate = Path::new(&m.target.root).join("release");
    for n in 10..14 {
        let running = m
            .runtime
            .hosted_validation_start(
                &m.target.id,
                start(n, "while [ ! -f release ]; do sleep 0.05; done", 0),
                &mut tx,
            )
            .unwrap();
        assert_eq!(running.state, HostedExecutionState::Running);
    }
    assert!(matches!(
        m.runtime
            .hosted_validation_start(&m.target.id, start(14, "true", 0), &mut tx),
        Err(OrchestrationError::Capacity)
    ));
    assert_eq!(
        m.runtime
            .hosted_validation(&m.target.id, &id(14), 0)
            .unwrap()
            .state,
        HostedExecutionState::Unknown
    );
    std::fs::write(&gate, "").unwrap();
    for n in 10..14 {
        let done = m
            .runtime
            .hosted_validation(&m.target.id, &id(n), 8_000)
            .unwrap();
        assert_eq!(done.outcome.unwrap().exit_code, Some(0));
    }
    assert_eq!(
        m.runtime
            .hosted_validation_start(&m.target.id, start(14, "true", 8_000), &mut tx)
            .unwrap()
            .state,
        HostedExecutionState::Finished
    );
    // Output is digested, never kept; past the limit the step is stopped.
    let root = Path::new(&m.target.root);
    let sh = |line: &str, limit: u64| {
        execute(
            root,
            "/bin/sh",
            &["-c".into(), line.into()],
            std::time::Duration::from_secs(20),
            limit,
            7,
            &AtomicBool::new(false),
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            None,
        )
    };
    let small = sh("printf ok; printf problem >&2; exit 2", 1024);
    assert_eq!(
        (small.end, small.exit_code, small.started_at_ms),
        (ExecutionEnd::Exited, Some(2), 7)
    );
    assert_eq!(small.stdout.unwrap().sha256, OK_SHA256);
    assert_eq!(small.stderr.unwrap().bytes, 7);
    let exact = sh("printf 0123456789", 10);
    assert_eq!(
        (exact.end, exact.stdout.unwrap().bytes),
        (ExecutionEnd::Exited, 10)
    );
    let loud = sh("while :; do echo 0123456789; done", 4096);
    assert_eq!(
        (loud.end, loud.exit_code),
        (ExecutionEnd::OutputLimit, None)
    );
    assert!(loud.stdout.is_none());
    let quick = sh("printf 0123456789ab", 10);
    assert_eq!(quick.end, ExecutionEnd::OutputLimit);
    let killed = sh("kill -9 $$", 1024);
    assert_eq!(
        (killed.end, killed.exit_code),
        (ExecutionEnd::Signalled, None)
    );
    // A step's children end with it.
    let started = std::time::Instant::now();
    let family = execute(
        root,
        "/bin/sh",
        &["-c".into(), "sleep 30 & sleep 30".into()],
        std::time::Duration::from_millis(200),
        1024,
        7,
        &AtomicBool::new(false),
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        None,
    );
    assert_eq!(family.end, ExecutionEnd::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(
        family.stdout.unwrap().bytes,
        0,
        "the stream ended: nothing still holds it"
    );
}

/// HS2-W02PTA: a step sees an allowlist of the host's variables plus its own
/// identifiers; credentials and Hot Sheet's own variables never reach it.
#[test]
fn hs2_w02pta_a_step_sees_only_the_reviewed_environment() {
    let host = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(name, value)| {
                (
                    std::ffi::OsString::from(name),
                    std::ffi::OsString::from(value),
                )
            })
            .collect::<Vec<_>>()
    };
    let names = |environment: &[(std::ffi::OsString, std::ffi::OsString)]| {
        environment
            .iter()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    // Empty host: only the step's own identifiers.
    let empty = validation_environment(Vec::new(), "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    assert_eq!(names(&empty), [STEP_EXECUTION_VARIABLE, "CI"]);
    assert_eq!(empty[0].1, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    // (variable, passed?)
    let table = [
        ("PATH", true),
        ("HOME", true),
        ("TMPDIR", true),
        ("LANG", true),
        ("LC_ALL", true),
        ("LC_CTYPE", true),
        ("TZ", true),
        ("HOTSHEET_HOME", false),
        ("HOTSHEET_SECRET", false),
        ("HOTSHEET_SERVER_URL", false),
        ("HOTSHEET_WORKER_ID", false),
        ("GH_TOKEN", false),
        ("GITHUB_TOKEN", false),
        ("ANTHROPIC_API_KEY", false),
        ("OPENAI_API_KEY", false),
        ("AWS_SECRET_ACCESS_KEY", false),
        ("SSH_AUTH_SOCK", false),
        ("NPM_TOKEN", false),
        ("CI", false),
        ("PATHS", false),
        ("XLC_ALL", false),
    ];
    let pairs: Vec<(&str, &str)> = table.iter().map(|(name, _)| (*name, "synthetic")).collect();
    let environment = validation_environment(host(&pairs), "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let kept = names(&environment);
    for (name, passed) in table {
        if name == "CI" {
            continue;
        }
        assert_eq!(kept.contains(&name.to_owned()), passed, "{name}");
    }
    // The host's own CI is not passed through; the step's is always `true`, once.
    assert_eq!(kept.iter().filter(|name| *name == "CI").count(), 1);
    assert_eq!(environment.last().unwrap().1, "true");
    // A real step: the test process's HOTSHEET_HOME (set for every test) is withheld,
    // PATH and the execution id are there.
    assert!(std::env::var_os("HOTSHEET_HOME").is_some());
    let root = tempfile::tempdir().unwrap();
    let probe = execute(
        root.path(),
        "/bin/sh",
        &[
            "-c".into(),
            "test -z \"${HOTSHEET_HOME+x}\" || exit 3; test -n \"$PATH\" || exit 4; \
             test \"$HOTSHEET_VALIDATION_EXECUTION_ID\" = 01ARZ3NDEKTSV4RRFFQ69G5FAV || exit 5; \
             test \"$CI\" = true || exit 6"
                .into(),
        ],
        std::time::Duration::from_secs(20),
        1024,
        7,
        &AtomicBool::new(false),
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        None,
    );
    assert_eq!(
        (probe.end, probe.exit_code),
        (ExecutionEnd::Exited, Some(0))
    );
}

/// HS2-ZCR3Y7: the machine that runs a step keeps a small stream whole, under the
/// digest the result records, and reads it back by digest for that checkout only. A
/// long stream keeps its digest only. Copies expire and stay within the machine's
/// bounds; a damaged copy is refused.
#[test]
fn hs2_zcr3y7_small_output_is_kept_on_the_host_and_read_back_by_digest() {
    use base64::Engine;
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let start = |n: u128, line: &str| HostedValidationStart {
        execution_id: id(n),
        check: hotsheet_ticketing::orchestration::ValidationCheck::Command {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), line.into()],
        },
        timeout_seconds: 20,
        wait_ms: 8_000,
    };
    // Nothing kept yet.
    assert!(matches!(
        m.runtime.validation_output(&m.target.id, OK_SHA256),
        Err(OrchestrationError::NotFound)
    ));
    assert!(matches!(
        m.runtime.validation_output(&m.target.id, "not-a-digest"),
        Err(OrchestrationError::Invalid(_))
    ));
    let done = m
        .runtime
        .hosted_validation_start(
            &m.target.id,
            start(40, "printf ok; printf problem >&2"),
            &mut tx,
        )
        .unwrap();
    let outcome = done.outcome.unwrap();
    let stdout = outcome.stdout.unwrap();
    let stderr = outcome.stderr.unwrap();
    assert_eq!(stdout.sha256, OK_SHA256);
    // Both streams are read back through the router, by digest, as they were.
    let read = |digest: &str, tx: &mut Transaction<'_>| {
        m.runtime.request(
            "GET",
            &format!(
                "/checkouts/{}/target-validation-output/{digest}",
                m.target.id
            ),
            &Value::Null,
            tx,
        )
    };
    let kept = read(OK_SHA256, &mut tx).unwrap();
    assert_eq!(kept["bytes"], 2);
    let decode = |value: &Value| {
        base64::engine::general_purpose::STANDARD
            .decode(value["content_base64"].as_str().unwrap())
            .unwrap()
    };
    assert_eq!(decode(&kept), b"ok");
    assert_eq!(decode(&read(&stderr.sha256, &mut tx).unwrap()), b"problem");
    // Another checkout of this machine cannot read it.
    assert!(matches!(
        m.runtime.validation_output(&m.owner.id, OK_SHA256),
        Err(OrchestrationError::NotFound)
    ));
    // The copy is private and sits on the hosting machine only.
    let directory = m.home.join(VALIDATION_OUTPUT_DIRECTORY).join(&m.target.id);
    let file = directory.join(OK_SHA256);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    // The same output again is kept once (its empty stderr is a third copy).
    m.runtime
        .hosted_validation_start(&m.target.id, start(41, "printf ok"), &mut tx)
        .unwrap();
    let copies = |dir: &Path| {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(copies(&directory).len(), 3, "{:?}", copies(&directory));
    // A stream over the cap keeps its digest only, and leaves no partial file.
    let big = m
        .runtime
        .hosted_validation_start(
            &m.target.id,
            start(42, "head -c 4194305 /dev/zero"),
            &mut tx,
        )
        .unwrap();
    let big = big.outcome.unwrap().stdout.unwrap();
    assert_eq!(big.bytes, MAX_RETAINED_STREAM_BYTES + 1);
    assert!(matches!(
        m.runtime.validation_output(&m.target.id, &big.sha256),
        Err(OrchestrationError::NotFound)
    ));
    assert_eq!(copies(&directory).len(), 3, "{:?}", copies(&directory));
    assert!(
        copies(&directory)
            .iter()
            .all(|name| !name.starts_with(".partial"))
    );
    // A damaged copy is refused, never served.
    std::fs::write(&file, b"tampered").unwrap();
    assert!(matches!(
        m.runtime.validation_output(&m.target.id, OK_SHA256),
        Err(OrchestrationError::Corrupt)
    ));
    // Pruning: an expired copy goes; beyond the count bound the oldest go first.
    let root = m.home.join(VALIDATION_OUTPUT_DIRECTORY);
    let now = std::time::SystemTime::now();
    let old = std::fs::File::options().write(true).open(&file).unwrap();
    old.set_modified(now - RETAINED_OUTPUT_TTL - std::time::Duration::from_secs(60))
        .unwrap();
    drop(old);
    crate::orchestration::validation_output::prune(&root, now);
    assert!(!file.exists());
    let filler = root.join("filler");
    std::fs::create_dir_all(&filler).unwrap();
    for n in 0..MAX_RETAINED_COPIES {
        let path = filler.join(format!("{n:064x}"));
        std::fs::write(&path, b"").unwrap();
        let handle = std::fs::File::options().write(true).open(&path).unwrap();
        handle
            .set_modified(now - std::time::Duration::from_secs(1_000 - (n as u64 % 1_000)))
            .unwrap();
    }
    crate::orchestration::validation_output::prune(&root, now);
    let remaining = copies(&filler).len() + copies(&directory).len();
    assert_eq!(remaining, MAX_RETAINED_COPIES);
}

/// HS2-7BTV98: after the hosting process restarts, a finished execution is answered
/// from the machine's record, one that was running then is never started again, and a
/// cancelled id stays cancelled.
#[test]
fn hs2_7btv98_hosted_executions_survive_a_restart_without_running_twice() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let start = |n: u128, line: &str, wait_ms: u64| HostedValidationStart {
        execution_id: id(n),
        check: hotsheet_ticketing::orchestration::ValidationCheck::Command {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), line.into()],
        },
        timeout_seconds: 20,
        wait_ms,
    };
    let runs = Path::new(&m.target.root).join("runs");
    // One finishes; one is still running when the "process" goes away.
    let finished = m
        .runtime
        .hosted_validation_start(
            &m.target.id,
            start(50, "echo x >> runs; exit 3", 8_000),
            &mut tx,
        )
        .unwrap();
    assert_eq!(finished.outcome.as_ref().unwrap().exit_code, Some(3));
    let gate = Path::new(&m.target.root).join("gate");
    let running = m
        .runtime
        .hosted_validation_start(
            &m.target.id,
            start(
                51,
                "echo y >> runs; while [ ! -f gate ]; do sleep 0.05; done",
                0,
            ),
            &mut tx,
        )
        .unwrap();
    assert_eq!(running.state, HostedExecutionState::Running);
    forget_executions();
    // Finished: answered from the record, by read and by a repeated start.
    for answer in [
        m.runtime
            .hosted_validation(&m.target.id, &id(50), 0)
            .unwrap(),
        m.runtime
            .hosted_validation_start(
                &m.target.id,
                start(50, "echo x >> runs; exit 3", 0),
                &mut tx,
            )
            .unwrap(),
    ] {
        assert_eq!(answer.state, HostedExecutionState::Finished);
        assert_eq!(answer.outcome.unwrap().exit_code, Some(3));
    }
    // Running when the process went: unknown, and a late start runs nothing.
    assert_eq!(
        m.runtime
            .hosted_validation(&m.target.id, &id(51), 0)
            .unwrap()
            .state,
        HostedExecutionState::Unknown
    );
    let late = m
        .runtime
        .hosted_validation_start(&m.target.id, start(51, "echo z >> runs", 8_000), &mut tx)
        .unwrap();
    assert_eq!(late.state, HostedExecutionState::Unknown);
    std::fs::write(&gate, "").unwrap();
    // A cancel of an id never seen is remembered across the restart too.
    let cancel = HostedValidationCancel {
        execution_id: id(52),
        wait_ms: 0,
    };
    assert_eq!(
        m.runtime
            .hosted_validation_cancel(&m.target.id, cancel.clone(), &mut tx)
            .unwrap()
            .state,
        HostedExecutionState::Unknown
    );
    forget_executions();
    let after = m
        .runtime
        .hosted_validation_start(&m.target.id, start(52, "echo c >> runs", 8_000), &mut tx)
        .unwrap();
    assert_eq!(after.state, HostedExecutionState::Finished);
    assert_eq!(after.outcome.unwrap().end, ExecutionEnd::Cancelled);
    // A cancel of an execution that finished before the restart keeps its own end.
    let kept = m
        .runtime
        .hosted_validation_cancel(
            &m.target.id,
            HostedValidationCancel {
                execution_id: id(50),
                wait_ms: 0,
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(kept.outcome.unwrap().exit_code, Some(3));
    // Each step ran exactly once: x and y; never z or c, and never x twice.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let lines = loop {
        let lines = std::fs::read_to_string(&runs).unwrap_or_default();
        if lines.lines().count() >= 2 || std::time::Instant::now() > deadline {
            break lines;
        }
    };
    let mut sorted: Vec<&str> = lines.lines().collect();
    sorted.sort_unstable();
    assert_eq!(sorted, ["x", "y"]);
}

/// A remote peer whose transport can be made to fail or to refuse.
struct Flaky {
    peer: RemotePeer,
    unreachable: AtomicBool,
}
impl RemoteTransport for Flaky {
    fn probe(&self, origin: &str, c: &RemoteCredentials) -> Result<RemoteSnapshot, RemoteFailure> {
        self.peer.probe(origin, c)
    }
}
impl RemoteCheckoutTransport for Flaky {
    fn checkout_projection(
        &self,
        origin: &str,
        credentials: &RemoteCredentials,
        checkout_id: &str,
    ) -> Result<CheckoutProjection, TargetFailure> {
        self.peer
            .checkout_projection(origin, credentials, checkout_id)
    }
    fn target_call(
        &self,
        origin: &str,
        credentials: &RemoteCredentials,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<TargetReply, TargetFailure> {
        if self.unreachable.load(Ordering::SeqCst) && path.contains("target-validations") {
            return Err(TargetFailure::Unreachable);
        }
        self.peer
            .target_call(origin, credentials, method, path, body)
    }
}

#[test]
fn hs2_wn0mxz_a_remote_target_runs_the_step_and_an_unconfirmed_one_stays_running() {
    // The peer machine hosts the target; the controller machine owns the orchestration.
    let peer = Arc::new(Flaky {
        peer: RemotePeer {
            hosted: machine(99, false, unsupported),
            calls: Mutex::new(Vec::new()),
            status_override: Mutex::new(None),
        },
        unreachable: AtomicBool::new(false),
    });
    let hosted = &peer.peer.hosted;
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let project = root.path().join("owner");
    std::fs::create_dir(&project).unwrap();
    let store_path = root.path().join("owner-store");
    let owner_store = FsStore::init_git(&store_path, &StoreMetadata::new("OWN")).unwrap();
    let checkouts = CheckoutRegistry::new(home.join("checkouts.json"));
    let owner = checkouts
        .register(&project, None, None, vec![store_path])
        .unwrap();
    let secrets = Arc::new(Memory::default());
    let runtime = OrchestrationRuntime::new(
        home.clone(),
        checkouts,
        ProviderConfigRegistry::new(root.path().join("providers.json")),
        secrets.clone(),
        peer.clone(),
        id(10),
    )
    .unwrap();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let target = TargetRef {
        owner_checkout_id: owner.id.clone(),
        target_id: id(1),
    };
    let manifest = runtime
        .replace(
            &owner.id,
            ReplaceTargets {
                expected_revision: None,
                targets: vec![TargetDeclaration {
                    target_id: id(1),
                    state: DeclarationState::Active,
                    label: "Remote target".into(),
                    expected_repository_identity: None,
                }],
            },
            &mut tx,
        )
        .unwrap();
    let registered = runtime
        .remotes
        .register(
            RegisterRemote {
                id: id(20),
                label: "Synthetic peer".into(),
                origin: "http://127.0.0.1:8765".into(),
                expected_server_id: id(99),
                credentials: credentials(),
            },
            secrets.as_ref(),
            peer.as_ref(),
            100,
        )
        .unwrap();
    let projection = hosted
        .runtime
        .checkout_projection(&hosted.target.id)
        .unwrap();
    assert!(
        runtime
            .bind(
                BindTarget {
                    target: target.clone(),
                    expected_manifest_revision: manifest.revision.unwrap(),
                    expected_binding_revision: 0,
                    previous_destination: None,
                    destination: TargetDestination::Remote {
                        registration_id: id(20),
                        identity: registered.remote.identity,
                        checkout_id: projection.checkout_id,
                        checkout_authority: projection.checkout_authority,
                    },
                },
                &mut tx
            )
            .unwrap()
            .usable
    );
    allow_on(
        &runtime,
        &target.owner_checkout_id,
        &target.target_id,
        &[TargetAction::ValidationRun],
        &[],
        &mut tx,
    );
    let ticket = ops::create(
        &owner_store,
        ulid::Ulid::new(),
        "OWN",
        NOW.into(),
        NewTicket {
            title: "Coordinate remotely".into(),
            ..Default::default()
        },
    )
    .unwrap();
    profiles(
        &hosted.target.root,
        &[
            ("smoke", "printf ok; echo x >> remote-runs"),
            (
                "gated",
                "echo x >> gated-runs; while [ ! -f go ]; do sleep 0.05; done",
            ),
        ],
    );
    for name in ["smoke", "gated", "refused"] {
        runtime
            .validation_define(
                &owner.id,
                profile(&ticket, name, name),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    }
    let go = |step: &str, operation: &str, wait_ms: u64, tx: &mut Transaction<'_>| {
        runtime
            .validation_run(
                &owner.id,
                run(&ticket, step, operation, wait_ms),
                Some(&worker()),
                tx,
            )
            .unwrap()
    };
    // The step runs on the peer, in its checkout; the controller holds references.
    let passed = go("smoke", "op-1", MAX_VALIDATION_WAIT_MS, &mut tx);
    assert_eq!(passed.result.state, ValidationState::Passed);
    assert_eq!(
        passed.result.outcome.unwrap().stdout.unwrap().sha256,
        OK_SHA256
    );
    assert!(Path::new(&hosted.target.root).join("remote-runs").exists());
    assert!(!project.join("remote-runs").exists());
    let calls = || {
        peer.peer
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, path)| path.contains("target-validations"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(calls().len(), 1);
    assert_eq!(calls()[0].0, "POST");
    // The peer cannot be reached before it confirmed the start: the attempt stays
    // running, nothing is concluded, and the same request asks under the same id.
    peer.unreachable.store(true, Ordering::SeqCst);
    let unknown = go("gated", "op-2", 0, &mut tx);
    assert_eq!(unknown.result.state, ValidationState::Running);
    assert!(!unknown.result.host_started);
    assert_eq!(unknown.unverified, ["target_execution"]);
    peer.unreachable.store(false, Ordering::SeqCst);
    let accepted = go("gated", "op-2", 0, &mut tx);
    assert_eq!(accepted.result.result_id, unknown.result.result_id);
    assert!(accepted.result.host_started);
    assert!(accepted.unverified.is_empty());
    // Unreachable, or refusing, after the start: still running, still one execution.
    peer.unreachable.store(true, Ordering::SeqCst);
    let waiting = go("gated", "op-2", 0, &mut tx);
    assert_eq!(waiting.result.state, ValidationState::Running);
    assert_eq!(waiting.unverified, ["target_execution"]);
    peer.unreachable.store(false, Ordering::SeqCst);
    *peer.peer.status_override.lock().unwrap() = Some(503);
    let refused_read = go("gated", "op-2", 0, &mut tx);
    assert_eq!(refused_read.result.state, ValidationState::Running);
    assert_eq!(refused_read.unverified, ["target_execution"]);
    // A target that refuses to start a step at all makes it unavailable.
    let refused = go("refused", "op-3", 0, &mut tx);
    assert_eq!(refused.result.state, ValidationState::Unavailable);
    assert_eq!(
        refused.result.reason.as_deref(),
        Some("target_operation_failed")
    );
    *peer.peer.status_override.lock().unwrap() = None;
    std::fs::write(Path::new(&hosted.target.root).join("go"), "").unwrap();
    let finished = go("gated", "op-2", MAX_VALIDATION_WAIT_MS, &mut tx);
    assert_eq!(finished.result.state, ValidationState::Passed);
    assert_eq!(finished.result.result_id, unknown.result.result_id);
    let gated = std::fs::read_to_string(Path::new(&hosted.target.root).join("gated-runs")).unwrap();
    assert_eq!(gated.lines().count(), 1);
    // The peer's own records hold no step: definitions and results are the controller's.
    assert!(!hosted.home.join("orchestration-validation.json").exists());
    assert!(home.join("orchestration-validation.json").exists());
}

fn cancel(ticket: &Ticket, step: &str, operation: Option<&str>, wait_ms: u64) -> CancelValidation {
    serde_json::from_value(json!({"ticket": ticket_ref(ticket), "step_id": step,
        "operation_id": operation, "wait_ms": wait_ms}))
    .unwrap()
}
fn notes_with(m: &Machine, ticket: &Ticket, word: &str) -> usize {
    m.owner_store
        .read_ticket(&ticket.id)
        .unwrap()
        .notes
        .iter()
        .filter(|note| note.text.contains(word))
        .count()
}

/// HS2-17ABM2: cancelling stops the step on its host, ends the attempt `cancelled`
/// (not `failed`), never asks anyone, and is idempotent.
#[test]
fn hs2_17abm2_cancelling_stops_a_running_step_and_records_cancelled() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::ValidationRun], &mut tx);
    profiles(
        &m.target.root,
        &[(
            "gated",
            "echo x >> starts; while [ ! -f go ]; do sleep 0.05; done; echo done >> ends",
        )],
    );
    let ticket = m.orchestration_ticket("Coordinate");
    m.runtime
        .validation_define(
            &m.owner.id,
            define(
                &ticket,
                json!({"step_id": "gated", "check": {"kind": "profile", "name": "gated"},
                    "timeout_seconds": 20, "retry": {"max_attempts": 3}}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    // Nothing has run: there is nothing to cancel.
    assert_eq!(
        validation_error(m.runtime.validation_cancel(
            &m.owner.id,
            cancel(&ticket, "gated", None, 0),
            Some(&worker()),
            &mut tx
        )),
        ValidationError::ResultNotFound
    );
    let started = m
        .runtime
        .validation_run(
            &m.owner.id,
            run(&ticket, "gated", "op-1", 0),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(started.result.state, ValidationState::Running);
    // An anonymous caller is refused before the host is told anything.
    assert!(
        m.runtime
            .validation_cancel(
                &m.owner.id,
                cancel(&ticket, "gated", None, 0),
                None,
                &mut tx
            )
            .is_err()
    );
    let cancelled = m
        .runtime
        .validation_cancel(
            &m.owner.id,
            cancel(&ticket, "gated", None, MAX_VALIDATION_WAIT_MS),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(cancelled.result.result_id, started.result.result_id);
    assert_eq!(cancelled.result.state, ValidationState::Cancelled);
    assert_eq!(cancelled.result.reason.as_deref(), Some("cancelled"));
    assert_eq!(
        cancelled.result.outcome.as_ref().unwrap().end,
        ExecutionEnd::Cancelled
    );
    assert_eq!(cancelled.result.outcome.as_ref().unwrap().exit_code, None);
    // Distinct from failed: the retry policy takes no further attempt.
    assert_eq!(cancelled.attempts_left, 0);
    assert_eq!(cancelled.evidence.state, ValidationState::Cancelled);
    assert!(cancelled.unverified.is_empty());
    // It never asked anyone, and the gate recorded why.
    assert!(
        m.runtime
            .permission_list(&m.owner.id, &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
    assert_eq!(
        m.last_audit(),
        ("validation_run".into(), AuditOutcome::Succeeded, None)
    );
    // The step was stopped: it never reached its end.
    std::fs::write(Path::new(&m.target.root).join("go"), "").unwrap();
    assert!(!Path::new(&m.target.root).join("ends").exists());
    assert_eq!(notes_with(&m, &ticket, "cancelled"), 1);
    // The same cancel again, by request or by operation, changes nothing.
    for operation in [None, Some("op-1")] {
        let again = m
            .runtime
            .validation_cancel(
                &m.owner.id,
                cancel(&ticket, "gated", operation, 0),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
        assert_eq!(again.result, cancelled.result);
    }
    assert_eq!(notes_with(&m, &ticket, "cancelled"), 1);
    // The cancelled request is a fact: running it again runs nothing.
    let rerun = m
        .runtime
        .validation_run(
            &m.owner.id,
            run(&ticket, "gated", "op-1", 0),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(rerun.result, cancelled.result);
    let starts = || {
        std::fs::read_to_string(Path::new(&m.target.root).join("starts"))
            .unwrap()
            .lines()
            .count()
    };
    assert_eq!(starts(), 1);
    // A new request runs it again; once it passed, a cancel answers the pass.
    let passed = until_finished(&m, &ticket, "gated", "op-2", &mut tx);
    assert_eq!(passed.result.state, ValidationState::Passed);
    assert_eq!(starts(), 2);
    let after = m
        .runtime
        .validation_cancel(
            &m.owner.id,
            cancel(&ticket, "gated", None, 0),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(after.result, passed.result);
    assert_eq!(
        validation_error(m.runtime.validation_cancel(
            &m.owner.id,
            cancel(&ticket, "gated", Some("op-unknown"), 0),
            Some(&worker()),
            &mut tx
        )),
        ValidationError::ResultNotFound
    );
}

/// HS2-17ABM2: a cancel that races the step's own end. Whoever settles the attempt
/// first, it records how the step really ended, and reports it once.
#[test]
fn hs2_17abm2_a_cancel_racing_the_steps_exit_records_how_it_ended() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::ValidationRun], &mut tx);
    profiles(
        &m.target.root,
        &[
            ("quick", "printf ok"),
            ("gated", "while [ ! -f go ]; do sleep 0.05; done"),
            ("marked", "echo x >> marked-runs"),
        ],
    );
    let ticket = m.orchestration_ticket("Coordinate");
    for name in ["quick", "gated", "marked"] {
        m.runtime
            .validation_define(
                &m.owner.id,
                profile(&ticket, name, name),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    }
    // 1. The step exits by itself before the cancel reaches its host: the cancel
    //    records the pass the host saw, not a cancellation.
    let started = m
        .runtime
        .validation_run(
            &m.owner.id,
            run(&ticket, "quick", "op-quick", 0),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let ended = m
        .runtime
        .hosted_validation(&m.target.id, &started.result.result_id, 8_000)
        .unwrap();
    assert_eq!(ended.outcome.unwrap().end, ExecutionEnd::Exited);
    let lost_race = m
        .runtime
        .validation_cancel(
            &m.owner.id,
            cancel(&ticket, "quick", Some("op-quick"), MAX_VALIDATION_WAIT_MS),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(lost_race.result.state, ValidationState::Passed);
    assert_eq!(notes_with(&m, &ticket, "`quick`"), 1);
    // The request that started it now answers the same pass and reports nothing more.
    let continued = until_finished(&m, &ticket, "quick", "op-quick", &mut tx);
    assert_eq!(continued.result, lost_race.result);
    assert_eq!(notes_with(&m, &ticket, "`quick`"), 1);

    // 2. A cancel and the waiting request settle the same attempt at once. Either may
    //    write it; both answer the same cancellation and it is reported once.
    let started = m
        .runtime
        .validation_run(
            &m.owner.id,
            run(&ticket, "gated", "op-gated", 0),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(started.result.host_started);
    let (waited, stopped) = std::thread::scope(|scope| {
        let waiter = scope.spawn(|| {
            let mut rng = Rng(200);
            let mut tx = Transaction {
                clock: &Clock,
                rng: &mut rng,
            };
            m.runtime
                .validation_run(
                    &m.owner.id,
                    run(&ticket, "gated", "op-gated", MAX_VALIDATION_WAIT_MS),
                    Some(&worker()),
                    &mut tx,
                )
                .unwrap()
        });
        let stopped = m
            .runtime
            .validation_cancel(
                &m.owner.id,
                cancel(&ticket, "gated", None, MAX_VALIDATION_WAIT_MS),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
        (waiter.join().unwrap(), stopped)
    });
    assert_eq!(stopped.result.state, ValidationState::Cancelled);
    if waited.result.state.is_final() {
        assert_eq!(waited.result, stopped.result);
    }
    assert_eq!(notes_with(&m, &ticket, "`gated`"), 1);

    // 3. A cancel reaches the host before the start does: the host remembers the id as
    //    cancelled, and the late start runs nothing.
    let registry = ValidationRegistry::new(&m.home);
    let key = RunTicket {
        connection_id: m.owner_connection(),
        native_id: ticket.id.to_string(),
    };
    let (pending, _) = registry
        .begin(
            &m.owner.id,
            &key,
            "marked",
            NewResult {
                operation_id: "op-marked".into(),
                state: ValidationState::Running,
                reason: None,
                actor_id: Some("worker-1".into()),
                run_id: None,
            },
            &mut tx,
        )
        .unwrap();
    let early = m
        .runtime
        .validation_cancel(
            &m.owner.id,
            cancel(&ticket, "marked", Some("op-marked"), 0),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(early.result.state, ValidationState::Cancelled);
    assert_eq!(early.result.outcome, None);
    let late = m
        .runtime
        .hosted_validation_start(
            &m.target.id,
            HostedValidationStart {
                execution_id: pending.result_id.clone(),
                check: hotsheet_ticketing::orchestration::ValidationCheck::Profile {
                    name: "marked".into(),
                },
                timeout_seconds: 20,
                wait_ms: 0,
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(late.outcome.as_ref().unwrap().end, ExecutionEnd::Cancelled);
    assert!(!Path::new(&m.target.root).join("marked-runs").exists());
    // The late start's outcome cannot replace what the cancel recorded; the attempt
    // is answered as recorded and nothing is reported again.
    let (recorded, changed) = m
        .runtime
        .validation_settle(
            &m.owner.id,
            &key,
            "marked",
            &pending.result_id,
            ResultUpdate {
                to: Some(ValidationState::Cancelled),
                reason: Some("cancelled".into()),
                outcome: late.outcome,
                host_started: None,
            },
            &mut tx,
        )
        .unwrap();
    assert!(!changed);
    assert_eq!(recorded, early.result);
}

/// HS2-17ABM2: the hosted stop is idempotent, keeps a step's own end, and an id the
/// host does not know is never started afterwards.
#[test]
fn hs2_17abm2_the_hosted_stop_ends_an_execution_once() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let start = |n: u128, line: &str, wait_ms: u64| HostedValidationStart {
        execution_id: id(n),
        check: hotsheet_ticketing::orchestration::ValidationCheck::Command {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), line.into()],
        },
        timeout_seconds: 20,
        wait_ms,
    };
    let stop = |n: u128, wait_ms: u64| HostedValidationCancel {
        execution_id: id(n),
        wait_ms,
    };
    let running = m
        .runtime
        .hosted_validation_start(&m.target.id, start(1, "sleep 30", 0), &mut tx)
        .unwrap();
    assert_eq!(running.state, HostedExecutionState::Running);
    let stopped = m
        .runtime
        .hosted_validation_cancel(&m.target.id, stop(1, 8_000), &mut tx)
        .unwrap();
    assert_eq!(stopped.state, HostedExecutionState::Finished);
    let outcome = stopped.outcome.clone().unwrap();
    assert_eq!(outcome.end, ExecutionEnd::Cancelled);
    assert!(outcome.duration_ms < 8_000);
    for _ in 0..2 {
        let again = m
            .runtime
            .hosted_validation_cancel(&m.target.id, stop(1, 0), &mut tx)
            .unwrap();
        assert_eq!(again.outcome, stopped.outcome);
    }
    // A step that already ended keeps its own end.
    let exited = m
        .runtime
        .hosted_validation_start(&m.target.id, start(2, "exit 3", 8_000), &mut tx)
        .unwrap();
    assert_eq!(exited.outcome.as_ref().unwrap().exit_code, Some(3));
    let late = m
        .runtime
        .hosted_validation_cancel(&m.target.id, stop(2, 0), &mut tx)
        .unwrap();
    assert_eq!(late.outcome, exited.outcome);
    // Unknown here: answered as unknown, and a later start under that id runs nothing.
    let unknown = m
        .runtime
        .hosted_validation_cancel(&m.target.id, stop(3, 0), &mut tx)
        .unwrap();
    assert_eq!(unknown.state, HostedExecutionState::Unknown);
    let after = m
        .runtime
        .hosted_validation_start(
            &m.target.id,
            start(3, "echo x >> stop-runs", 8_000),
            &mut tx,
        )
        .unwrap();
    assert_eq!(after.outcome.unwrap().end, ExecutionEnd::Cancelled);
    assert!(!Path::new(&m.target.root).join("stop-runs").exists());
    // Requests are validated first.
    for bad in [
        HostedValidationCancel {
            execution_id: "nope".into(),
            wait_ms: 0,
        },
        stop(4, MAX_VALIDATION_WAIT_MS + 1),
    ] {
        assert!(matches!(
            m.runtime
                .hosted_validation_cancel(&m.target.id, bad, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    assert!(
        m.runtime
            .hosted_validation_cancel("unknown-checkout", stop(5, 0), &mut tx)
            .is_err()
    );
}

/// HS2-YYNW9F: a hosting checkout's profiles are listed and checked locally, an
/// orchestrating project reads a target's through the gate without asking, and a
/// profile's own timeout shortens the step's.
#[test]
fn hs2_yynw9f_profiles_are_listed_checked_and_read_through_the_gate() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    // Nothing is offered yet, and that is valid.
    let none = m.runtime.hosted_validation_profiles(&m.target.id).unwrap();
    assert!(!none.present && none.valid && none.profiles.is_empty());
    assert!(
        m.runtime
            .hosted_validation_profiles("unknown-checkout")
            .is_err()
    );
    let directory = Path::new(&m.target.root).join(".hotsheet2");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("validation-profiles.json"),
        json!({"schema_version": 1, "profiles": [
            {"name": "smoke", "program": "/bin/sh", "args": ["-c", "printf ok"]},
            {"name": "slow", "program": "/bin/sh", "args": ["-c", "sleep 30"], "timeout_seconds": 1},
        ]})
        .to_string(),
    )
    .unwrap();
    let local = m.runtime.hosted_validation_profiles(&m.target.id).unwrap();
    assert!(local.present && local.valid);
    assert_eq!(
        local
            .profiles
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["smoke", "slow"]
    );
    // Through the gate: an observation, so no rule is needed and nobody is asked.
    bind_and_allow(&m, 1, &[], &mut tx);
    let read = |m: &Machine, tx: &mut Transaction<'_>| {
        m.runtime.validation_profiles(
            &m.owner.id,
            TargetProfiles { target_id: id(1) },
            Some(&worker()),
            tx,
        )
    };
    assert_eq!(read(&m, &mut tx).unwrap(), local);
    assert!(
        m.runtime
            .permission_list(&m.owner.id, &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
    assert_eq!(
        m.last_audit(),
        ("gates_observe".into(), AuditOutcome::Succeeded, None)
    );
    assert!(
        m.runtime
            .validation_profiles(
                &m.owner.id,
                TargetProfiles { target_id: id(9) },
                Some(&worker()),
                &mut tx
            )
            .is_err()
    );
    // A damaged file is answered with its problems, not an error.
    std::fs::write(
        directory.join("validation-profiles.json"),
        br#"{"schema_version":1,"profiles":[{"name":"a","program":"x"},{"name":"a","program":"y"}]}"#,
    )
    .unwrap();
    let damaged = read(&m, &mut tx).unwrap();
    assert!(!damaged.valid);
    assert_eq!(damaged.problems[0].code, "duplicate_name");
    // A profile's own timeout stops the step sooner than the step's 20 seconds.
    std::fs::write(
        directory.join("validation-profiles.json"),
        json!({"schema_version": 1, "profiles": [
            {"name": "slow", "program": "/bin/sh", "args": ["-c", "sleep 30"], "timeout_seconds": 1},
        ]})
        .to_string(),
    )
    .unwrap();
    let started = std::time::Instant::now();
    let slow = m
        .runtime
        .hosted_validation_start(
            &m.target.id,
            HostedValidationStart {
                execution_id: id(1),
                check: hotsheet_ticketing::orchestration::ValidationCheck::Profile {
                    name: "slow".into(),
                },
                timeout_seconds: 20,
                wait_ms: 8_000,
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(slow.outcome.unwrap().end, ExecutionEnd::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_secs(8));
}

/// HS2-5ZT4GY: every wait on a long step passes the gate again, but the journal
/// records a continued wait of the same operation once; a refusal in between and the
/// outcome are still recorded.
#[test]
fn hs2_5zt4gy_repeated_validation_waits_are_audited_once() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::ValidationRun], &mut tx);
    profiles(
        &m.target.root,
        &[("gated", "while [ ! -f go ]; do sleep 0.05; done")],
    );
    let ticket = m.orchestration_ticket("Coordinate");
    m.runtime
        .validation_define(
            &m.owner.id,
            profile(&ticket, "gated", "gated"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let wait = |m: &Machine, tx: &mut Transaction<'_>| {
        m.runtime.validation_run(
            &m.owner.id,
            run(&ticket, "gated", "op-1", 0),
            Some(&worker()),
            tx,
        )
    };
    let entries = |m: &Machine| {
        m.runtime
            .audit(&m.owner.id, 0, 500)
            .unwrap()
            .entries
            .into_iter()
            .filter(|entry| entry.record.operation_id.as_deref() == Some("op-1"))
            .map(|entry| (entry.record.outcome, entry.record.reason))
            .collect::<Vec<_>>()
    };
    for _ in 0..5 {
        assert_eq!(
            wait(&m, &mut tx).unwrap().result.state,
            ValidationState::Running
        );
    }
    let continuing = (AuditOutcome::Allowed, Some("continuing".to_owned()));
    let decided = entries(&m);
    assert_eq!(decided.len(), 2, "{decided:?}");
    assert_eq!(decided[1], continuing);
    // The destination is still checked on every wait: unbound, the wait is refused
    // and recorded; bound again, the next continued wait is recorded once more.
    super::tickets::unbind(&m, 1, &mut tx);
    assert!(wait(&m, &mut tx).is_err());
    super::tickets::rebind(&m, 1, &mut tx);
    for _ in 0..3 {
        wait(&m, &mut tx).unwrap();
    }
    std::fs::write(Path::new(&m.target.root).join("go"), "").unwrap();
    let finished = until_finished(&m, &ticket, "gated", "op-1", &mut tx);
    assert_eq!(finished.result.state, ValidationState::Passed);
    let recorded = entries(&m);
    assert_eq!(recorded.len(), 5, "{recorded:?}");
    assert_eq!(recorded[2].0, AuditOutcome::Denied);
    assert_eq!(recorded[3], continuing);
    assert_eq!(recorded[4], (AuditOutcome::Succeeded, None));
}

/// HS2-ZYM6ZT: a browser's Run Validation acts as the orchestrator, so a command step
/// still asks; the person who pressed it is recorded as `requested_by` on every audit
/// entry of that request, and changes nothing about the decision.
#[test]
fn hs2_zym6zt_the_audit_names_the_person_a_validation_run_was_requested_by() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::ValidationRun], &mut tx);
    let ticket = m.orchestration_ticket("Coordinate");
    m.runtime
        .validation_define(
            &m.owner.id,
            define(
                &ticket,
                json!({"step_id": "custom", "timeout_seconds": 20,
                    "check": {"kind": "command", "program": "/bin/sh", "args": ["-c", "exit 0"]}}),
            ),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    let orchestrator =
        MutationActor::parse("system", Some(format!("orchestrator.{}", m.owner.id))).unwrap();
    let request = |operation: &str, person: Option<&str>| -> RunValidation {
        let mut body = json!({"ticket": ticket_ref(&ticket), "step_id": "custom",
            "operation_id": operation, "wait_ms": MAX_VALIDATION_WAIT_MS});
        if let Some(person) = person {
            body["requested_by"] = person.into();
        }
        serde_json::from_value(body).unwrap()
    };
    let entries = |m: &Machine| m.runtime.audit(&m.owner.id, 0, 500).unwrap().entries;
    let before = entries(&m).len();
    // A malformed person is refused before anything is asked or recorded.
    for bad in ["a b", "", "x/y"] {
        assert!(matches!(
            m.runtime.validation_run(
                &m.owner.id,
                request("op-bad", Some(bad)),
                Some(&orchestrator),
                &mut tx
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    assert_eq!(entries(&m).len(), before);
    // The orchestrator still asks for a command step: the person is not the actor.
    let asked = permission_request(m.runtime.validation_run(
        &m.owner.id,
        request("op-1", Some("person-abc123")),
        Some(&orchestrator),
        &mut tx,
    ));
    answer(
        &m.runtime,
        &m.owner.id,
        &asked,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    let mut view = None;
    for _ in 0..10 {
        let answer = m
            .runtime
            .validation_run(
                &m.owner.id,
                request("op-1", Some("person-abc123")),
                Some(&orchestrator),
                &mut tx,
            )
            .unwrap();
        if answer.result.state != ValidationState::Running {
            view = Some(answer);
            break;
        }
    }
    assert_eq!(view.unwrap().result.state, ValidationState::Passed);
    let runs: Vec<_> = entries(&m)
        .into_iter()
        .skip(before)
        .filter(|entry| entry.record.action == "validation_run")
        .collect();
    let outcomes: Vec<AuditOutcome> = runs.iter().map(|entry| entry.record.outcome).collect();
    for expected in [
        AuditOutcome::Denied,
        AuditOutcome::Allowed,
        AuditOutcome::Succeeded,
    ] {
        assert!(outcomes.contains(&expected), "{expected:?} in {outcomes:?}");
    }
    for entry in &runs {
        assert_eq!(
            entry.record.actor_id.as_deref(),
            Some(format!("orchestrator.{}", m.owner.id).as_str())
        );
        assert_eq!(entry.record.requested_by.as_deref(), Some("person-abc123"));
    }
    // A request for nobody in particular records none, and is decided the same way.
    let seen = entries(&m).len();
    permission_request(m.runtime.validation_run(
        &m.owner.id,
        request("op-2", None),
        Some(&orchestrator),
        &mut tx,
    ));
    assert!(
        entries(&m)
            .into_iter()
            .skip(seen)
            .all(|entry| entry.record.requested_by.is_none())
    );
}
