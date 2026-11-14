//! HS2-WN0MXZ validation step model, result state machine and registry sequences;
//! synthetic homes only.

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
const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn ticket(n: u128) -> RunTicket {
    RunTicket {
        connection_id: "owner-source".into(),
        native_id: id(n),
    }
}
fn step(name: &str) -> StepDefinition {
    StepDefinition {
        step_id: name.into(),
        target_id: id(1),
        check: ValidationCheck::Profile {
            name: "smoke".into(),
        },
        prerequisites: Vec::new(),
        evidence: ExpectedEvidence::default(),
        timeout_seconds: 10,
        retry: RetryPolicy::default(),
    }
}
fn new(operation: &str, state: ValidationState) -> NewResult {
    NewResult {
        operation_id: operation.into(),
        state,
        reason: None,
        actor_id: Some("worker-a".into()),
        run_id: None,
    }
}
fn outcome(end: ExecutionEnd, exit_code: Option<i32>, stdout: Option<&str>) -> ExecutionOutcome {
    ExecutionOutcome {
        end,
        exit_code,
        started_at_ms: 1_000,
        duration_ms: 25,
        stdout: stdout.map(|sha256| StreamRef {
            sha256: sha256.into(),
            bytes: 12,
        }),
        stderr: None,
    }
}
fn to(state: ValidationState, reason: Option<&str>) -> ResultUpdate {
    ResultUpdate {
        to: Some(state),
        reason: reason.map(Into::into),
        outcome: Some(outcome(ExecutionEnd::Exited, Some(0), None)),
        host_started: None,
    }
}
fn validation_error<T: std::fmt::Debug>(result: Result<T, OrchestrationError>) -> ValidationError {
    match result {
        Err(OrchestrationError::Validation(error)) => error,
        other => panic!("expected a validation error, got {other:?}"),
    }
}
struct Bench {
    _home: tempfile::TempDir,
    registry: ValidationRegistry,
    clock: Clock,
    rng: Rng,
}
impl Bench {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        Self {
            registry: ValidationRegistry::new(home.path()),
            _home: home,
            clock: Clock(std::cell::Cell::new(1_000)),
            rng: Rng(100),
        }
    }
    fn define(&mut self, n: u128, definition: StepDefinition) -> (ValidationStep, bool) {
        self.try_define(n, definition).unwrap()
    }
    fn try_define(
        &mut self,
        n: u128,
        definition: StepDefinition,
    ) -> Result<(ValidationStep, bool), OrchestrationError> {
        self.registry.define(
            OWNER,
            ticket(n),
            definition,
            Some("worker-a".into()),
            &mut Transaction {
                clock: &self.clock,
                rng: &mut self.rng,
            },
        )
    }
    fn begin(
        &mut self,
        n: u128,
        step: &str,
        new: NewResult,
    ) -> Result<(ValidationResult, bool), OrchestrationError> {
        self.registry.begin(
            OWNER,
            &ticket(n),
            step,
            new,
            &mut Transaction {
                clock: &self.clock,
                rng: &mut self.rng,
            },
        )
    }
    fn finish(
        &mut self,
        n: u128,
        step: &str,
        result_id: &str,
        update: ResultUpdate,
    ) -> Result<ValidationResult, OrchestrationError> {
        self.registry.finish(
            OWNER,
            &ticket(n),
            step,
            result_id,
            update,
            &mut Transaction {
                clock: &self.clock,
                rng: &mut self.rng,
            },
        )
    }
    fn evidence(&self, n: u128, step: &str) -> ValidationEvidence {
        self.registry
            .evidence(OWNER, &ticket(n), step, self.clock.0.get())
            .unwrap()
    }
    fn state(&self, n: u128, step: &str) -> (ValidationState, Option<String>) {
        let evidence = self.evidence(n, step);
        (evidence.state, evidence.reason)
    }
    /// Run one attempt to a finished state and return it.
    fn attempt(
        &mut self,
        n: u128,
        step: &str,
        operation: &str,
        state: ValidationState,
    ) -> ValidationResult {
        let (running, created) = self
            .begin(n, step, new(operation, ValidationState::Running))
            .unwrap();
        assert!(created);
        self.finish(n, step, &running.result_id, to(state, None))
            .unwrap()
    }
}

#[test]
fn hs2_wn0mxz_every_result_state_edge_is_allowed_or_refused_exactly() {
    use ValidationState::{Cancelled, Failed, Passed, Running, Skipped, Unavailable, Unverified};
    let allowed = [
        (Running, Passed),
        (Running, Failed),
        (Running, Unavailable),
        (Running, Unverified),
        // HS2-17ABM2: only a running attempt can be cancelled.
        (Running, Cancelled),
    ];
    for from in ValidationState::ALL {
        for next in ValidationState::ALL {
            assert_eq!(
                from.may_become(next),
                allowed.contains(&(from, next)),
                "{from:?} -> {next:?}"
            );
        }
        assert_eq!(from.is_final(), from != Running);
        assert_eq!(
            from.may_begin(),
            matches!(from, Running | Skipped | Unavailable),
            "{from:?}"
        );
    }
    assert_eq!(
        ValidationState::ALL.map(ValidationState::code),
        [
            "running",
            "passed",
            "failed",
            "skipped",
            "unavailable",
            "unverified",
            "cancelled"
        ]
    );
    // The registry enforces the same matrix on a stored result, from every state.
    for from in ValidationState::ALL {
        for next in ValidationState::ALL {
            let mut bench = Bench::new();
            bench.define(1, step("a"));
            let result = if from.may_begin() {
                bench.begin(1, "a", new("op", from)).unwrap().0
            } else {
                bench.attempt(1, "a", "op", from)
            };
            let moved = bench.finish(1, "a", &result.result_id, to(next, None));
            if from == next {
                // Reporting the state a result already has never changes it.
                let same = from == Running || result.outcome.is_some();
                assert_eq!(moved.is_ok(), same, "{from:?} -> {next:?}");
            } else if from.may_become(next) {
                assert_eq!(moved.unwrap().state, next);
            } else if from.is_final() {
                assert_eq!(validation_error(moved), ValidationError::ResultFinal);
            } else {
                assert_eq!(validation_error(moved), ValidationError::InvalidTransition);
            }
            if !from.may_become(next) {
                assert_eq!(bench.evidence(1, "a").latest.unwrap().state, from);
            }
        }
    }
}

#[test]
fn hs2_wn0mxz_an_outcome_is_judged_against_the_expected_evidence() {
    use ExecutionEnd::{
        Cancelled, Exited, LaunchFailed, OutputLimit, ProfileNotDefined, ProgramNotFound,
        Signalled, TimedOut,
    };
    use ValidationState::{Failed, Passed, Unavailable, Unverified};
    let plain = ExpectedEvidence::default();
    let nonzero = ExpectedEvidence {
        exit_code: 3,
        stdout_sha256: None,
    };
    let exact = ExpectedEvidence {
        exit_code: 0,
        stdout_sha256: Some(DIGEST.into()),
    };
    let cases = [
        (&plain, Exited, Some(0), None, Passed, None),
        (
            &plain,
            Exited,
            Some(1),
            None,
            Failed,
            Some("exit_code_mismatch"),
        ),
        (
            &plain,
            Exited,
            None,
            None,
            Unverified,
            Some("exit_code_unknown"),
        ),
        (&nonzero, Exited, Some(3), None, Passed, None),
        (
            &nonzero,
            Exited,
            Some(0),
            None,
            Failed,
            Some("exit_code_mismatch"),
        ),
        (&exact, Exited, Some(0), Some(DIGEST), Passed, None),
        (
            &exact,
            Exited,
            Some(0),
            Some(OTHER),
            Failed,
            Some("output_mismatch"),
        ),
        (
            &exact,
            Exited,
            Some(0),
            None,
            Unverified,
            Some("output_unknown"),
        ),
        // The exit code is judged before the output.
        (
            &exact,
            Exited,
            Some(2),
            Some(DIGEST),
            Failed,
            Some("exit_code_mismatch"),
        ),
        (&plain, TimedOut, None, None, Failed, Some("timed_out")),
        // HS2-17ABM2: stopped on request is neither a pass nor a failure.
        (
            &exact,
            Cancelled,
            None,
            Some(DIGEST),
            ValidationState::Cancelled,
            Some("cancelled"),
        ),
        (
            &plain,
            Signalled,
            None,
            None,
            Failed,
            Some("terminated_by_signal"),
        ),
        (
            &plain,
            OutputLimit,
            None,
            None,
            Failed,
            Some("output_limit_exceeded"),
        ),
        (
            &plain,
            ProfileNotDefined,
            None,
            None,
            Unavailable,
            Some("profile_not_defined"),
        ),
        (
            &plain,
            ProgramNotFound,
            None,
            None,
            Unavailable,
            Some("program_not_found"),
        ),
        (
            &plain,
            LaunchFailed,
            None,
            None,
            Unavailable,
            Some("launch_failed"),
        ),
        // A step that never exited cannot pass on an exit code it happens to carry.
        (&plain, TimedOut, Some(0), None, Failed, Some("timed_out")),
    ];
    for (expected, end, exit_code, stdout, state, reason) in cases {
        assert_eq!(
            evaluate(expected, &outcome(end, exit_code, stdout)),
            (state, reason),
            "{end:?} {exit_code:?} {stdout:?}"
        );
    }
}

#[test]
fn hs2_wn0mxz_a_definition_is_bounded_and_names_only_other_steps() {
    assert!(step("smoke").validate().is_ok());
    let command = |program: &str, args: Vec<String>| StepDefinition {
        check: ValidationCheck::Command {
            program: program.into(),
            args,
        },
        ..step("a")
    };
    assert!(
        command("cargo", vec!["test".into(), "--all".into()])
            .validate()
            .is_ok()
    );
    assert!(
        command("cargo", vec!["line one\nline two".into()])
            .validate()
            .is_ok()
    );
    let invalid: Vec<(&str, StepDefinition)> = vec![
        ("empty step id", step("")),
        ("step id with a space", step("smoke test")),
        (
            "target id",
            StepDefinition {
                target_id: "target".into(),
                ..step("a")
            },
        ),
        (
            "profile name",
            StepDefinition {
                check: ValidationCheck::Profile {
                    name: "not a name".into(),
                },
                ..step("a")
            },
        ),
        ("empty program", command("", Vec::new())),
        (
            "program with a control character",
            command("a\nb", Vec::new()),
        ),
        ("argument with NUL", command("a", vec!["b\0c".into()])),
        ("too many arguments", command("a", vec!["b".into(); 33])),
        ("too long", command("a", vec!["b".repeat(4096)])),
        (
            "own prerequisite",
            StepDefinition {
                prerequisites: vec!["a".into()],
                ..step("a")
            },
        ),
        (
            "repeated prerequisite",
            StepDefinition {
                prerequisites: vec!["b".into(), "b".into()],
                ..step("a")
            },
        ),
        (
            "too many prerequisites",
            StepDefinition {
                prerequisites: (0..9).map(|n| format!("p{n}")).collect(),
                ..step("a")
            },
        ),
        (
            "digest",
            StepDefinition {
                evidence: ExpectedEvidence {
                    exit_code: 0,
                    stdout_sha256: Some("abc".into()),
                },
                ..step("a")
            },
        ),
        (
            "zero timeout",
            StepDefinition {
                timeout_seconds: 0,
                ..step("a")
            },
        ),
        (
            "long timeout",
            StepDefinition {
                timeout_seconds: 3601,
                ..step("a")
            },
        ),
        (
            "zero attempts",
            StepDefinition {
                retry: RetryPolicy { max_attempts: 0 },
                ..step("a")
            },
        ),
        (
            "too many attempts",
            StepDefinition {
                retry: RetryPolicy { max_attempts: 6 },
                ..step("a")
            },
        ),
    ];
    let mut bench = Bench::new();
    for (what, definition) in invalid {
        assert!(definition.validate().is_err(), "{what}");
        assert!(bench.try_define(1, definition).is_err(), "{what}");
    }
    assert!(bench.registry.steps(OWNER, None, 0).unwrap().is_empty());
    // Only a profile is inside the granted scope.
    assert!(step("a").check.in_granted_scope());
    assert!(!command("cargo", Vec::new()).check.in_granted_scope());
    // Defaults: exit code zero, five minutes, one attempt; unknown members are refused.
    let parsed: StepDefinition = serde_json::from_value(serde_json::json!({
        "step_id": "a", "target_id": id(1), "check": {"kind": "profile", "name": "smoke"}
    }))
    .unwrap();
    assert_eq!(parsed.evidence, ExpectedEvidence::default());
    assert_eq!(parsed.timeout_seconds, DEFAULT_STEP_TIMEOUT_SECONDS);
    assert_eq!(parsed.retry.max_attempts, 1);
    for body in [
        serde_json::json!({"step_id":"a","target_id":id(1),"check":{"kind":"profile","name":"s","args":[]}}),
        serde_json::json!({"step_id":"a","target_id":id(1),"check":{"kind":"shell","line":"x"}}),
        serde_json::json!({"step_id":"a","target_id":id(1),"check":{"kind":"profile","name":"s"},"output":"x"}),
    ] {
        assert!(serde_json::from_value::<StepDefinition>(body).is_err());
    }
}

#[test]
fn hs2_wn0mxz_define_is_idempotent_and_a_change_makes_old_evidence_unverified() {
    let mut bench = Bench::new();
    let (first, created) = bench.define(1, step("a"));
    assert!(created);
    assert_eq!(first.revision, 1);
    assert_eq!(
        bench.state(1, "a"),
        (ValidationState::Unverified, Some("not_run".into()))
    );
    bench.clock.0.set(2_000);
    let (again, changed) = bench.define(1, step("a"));
    assert!(!changed);
    assert_eq!(again, first);
    let passed = bench.attempt(1, "a", "op-1", ValidationState::Passed);
    assert_eq!(bench.state(1, "a"), (ValidationState::Passed, None));
    // A changed definition is a new revision; what passed before proves nothing now.
    bench.clock.0.set(3_000);
    let (changed_step, changed) = bench.define(
        1,
        StepDefinition {
            timeout_seconds: 20,
            ..step("a")
        },
    );
    assert!(changed);
    assert_eq!(changed_step.revision, 2);
    assert_eq!(changed_step.created_at_ms, 1_000);
    assert_eq!(changed_step.updated_at_ms, 3_000);
    let evidence = bench.evidence(1, "a");
    assert_eq!(evidence.state, ValidationState::Unverified);
    assert_eq!(evidence.reason.as_deref(), Some("step_changed"));
    // The recorded result itself is untouched.
    assert_eq!(evidence.latest, Some(passed.clone()));
    // Changing it back is another revision, not a return to the old evidence.
    assert_eq!(bench.define(1, step("a")).0.revision, 3);
    assert_eq!(bench.state(1, "a").1.as_deref(), Some("step_changed"));
    // A finished request is not rerun under its old operation id.
    let (same, created) = bench
        .begin(1, "a", new("op-1", ValidationState::Running))
        .unwrap();
    assert!(!created);
    assert_eq!(same, passed);
    let fresh = bench.attempt(1, "a", "op-2", ValidationState::Passed);
    assert_eq!(fresh.step_revision, 3);
    assert_eq!(bench.state(1, "a"), (ValidationState::Passed, None));
    assert_eq!(bench.evidence(1, "a").history.len(), 2);
}

#[test]
fn hs2_wn0mxz_a_finished_result_is_immutable_and_repeats_are_no_ops() {
    let mut bench = Bench::new();
    bench.define(1, step("a"));
    let (running, created) = bench
        .begin(1, "a", new("op-1", ValidationState::Running))
        .unwrap();
    assert!(created);
    assert_eq!(
        (running.attempt, running.state),
        (1, ValidationState::Running)
    );
    assert_eq!(
        running.deadline_ms,
        Some(1_000 + 10_000 + VALIDATION_DEADLINE_GRACE_MS)
    );
    assert!(!running.host_started);
    // The same request again is the same attempt, however often.
    for _ in 0..3 {
        let (again, created) = bench
            .begin(1, "a", new("op-1", ValidationState::Running))
            .unwrap();
        assert!(!created);
        assert_eq!(again, running);
    }
    // Another request waits for the running one.
    assert_eq!(
        validation_error(bench.begin(1, "a", new("op-2", ValidationState::Running))),
        ValidationError::StepRunning
    );
    assert_eq!(
        validation_error(bench.begin(1, "a", new("op-2", ValidationState::Skipped))),
        ValidationError::StepRunning
    );
    assert_eq!(
        validation_error(bench.try_define(
            1,
            StepDefinition {
                timeout_seconds: 9,
                ..step("a")
            }
        )),
        ValidationError::StepRunning
    );
    assert_eq!(
        validation_error(bench.registry.remove(OWNER, &ticket(1), "a", 1_000)),
        ValidationError::StepRunning
    );
    // The host's acceptance is recorded once and never taken back.
    let started = ResultUpdate {
        host_started: Some(true),
        ..ResultUpdate::default()
    };
    assert!(
        bench
            .finish(1, "a", &running.result_id, started.clone())
            .unwrap()
            .host_started
    );
    let not_started = ResultUpdate {
        host_started: Some(false),
        ..ResultUpdate::default()
    };
    assert!(
        bench
            .finish(1, "a", &running.result_id, not_started)
            .unwrap()
            .host_started
    );
    assert_eq!(
        bench.evidence(1, "a").latest.unwrap().state,
        ValidationState::Running
    );
    bench.clock.0.set(5_000);
    let failed = bench
        .finish(
            1,
            "a",
            &running.result_id,
            to(ValidationState::Failed, Some("exit_code_mismatch")),
        )
        .unwrap();
    assert_eq!(failed.finished_at_ms, Some(5_000));
    assert_eq!(failed.deadline_ms, None);
    assert_eq!(failed.reason.as_deref(), Some("exit_code_mismatch"));
    assert!(failed.outcome.is_some());
    // The identical report again changes nothing; any different one is refused.
    bench.clock.0.set(6_000);
    assert_eq!(
        bench
            .finish(
                1,
                "a",
                &running.result_id,
                to(ValidationState::Failed, Some("exit_code_mismatch"))
            )
            .unwrap(),
        failed
    );
    assert_eq!(
        bench.finish(1, "a", &running.result_id, started).unwrap(),
        failed
    );
    for update in [
        to(ValidationState::Passed, None),
        to(ValidationState::Failed, Some("timed_out")),
        ResultUpdate {
            outcome: Some(outcome(ExecutionEnd::Exited, Some(7), None)),
            ..ResultUpdate::default()
        },
    ] {
        assert_eq!(
            validation_error(bench.finish(1, "a", &running.result_id, update)),
            ValidationError::ResultFinal
        );
    }
    assert_eq!(bench.evidence(1, "a").latest, Some(failed.clone()));
    // An unknown result, step, ticket or owner is not found.
    assert_eq!(
        validation_error(bench.finish(1, "a", &id(999), to(ValidationState::Passed, None))),
        ValidationError::ResultNotFound
    );
    assert_eq!(
        validation_error(bench.finish(
            1,
            "b",
            &running.result_id,
            to(ValidationState::Passed, None)
        )),
        ValidationError::StepNotFound
    );
    assert_eq!(
        validation_error(bench.begin(2, "a", new("op-9", ValidationState::Running))),
        ValidationError::StepNotFound
    );
    assert_eq!(
        validation_error(bench.registry.evidence("other-owner", &ticket(1), "a", 0)),
        ValidationError::StepNotFound
    );
    // A result may not begin in a state that claims it already ran.
    for state in [
        ValidationState::Passed,
        ValidationState::Failed,
        ValidationState::Unverified,
    ] {
        assert!(matches!(
            bench.begin(1, "a", new("op-3", state)),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    // One retained result; nothing above added one.
    assert_eq!(bench.evidence(1, "a").history, vec![failed]);
}

#[test]
fn hs2_wn0mxz_a_failed_attempt_is_retried_only_as_the_step_allows() {
    let mut bench = Bench::new();
    bench.define(
        1,
        StepDefinition {
            retry: RetryPolicy { max_attempts: 3 },
            ..step("a")
        },
    );
    let mut seen = Vec::new();
    for attempt in 1..=3 {
        let (running, created) = bench
            .begin(1, "a", new("op-1", ValidationState::Running))
            .unwrap();
        assert!(created, "attempt {attempt}");
        assert_eq!(running.attempt, attempt);
        assert!(!seen.contains(&running.result_id));
        seen.push(running.result_id.clone());
        bench
            .finish(
                1,
                "a",
                &running.result_id,
                to(ValidationState::Failed, Some("timed_out")),
            )
            .unwrap();
    }
    // The policy is spent: the request stays at its last failed attempt.
    let (last, created) = bench
        .begin(1, "a", new("op-1", ValidationState::Running))
        .unwrap();
    assert!(!created);
    assert_eq!((last.attempt, last.state), (3, ValidationState::Failed));
    assert_eq!(bench.evidence(1, "a").history.len(), 3);
    // Only `failed` is retried: every other finished state ends its request.
    for (operation, state) in [
        ("op-2", ValidationState::Passed),
        ("op-3", ValidationState::Unavailable),
        ("op-4", ValidationState::Unverified),
    ] {
        let finished = bench.attempt(1, "a", operation, state);
        let (again, created) = bench
            .begin(1, "a", new(operation, ValidationState::Running))
            .unwrap();
        assert!(!created, "{state:?}");
        assert_eq!(again, finished);
    }
    // A retry is not taken while another request's attempt runs, and a failed attempt
    // for an older definition is not retried under the new one.
    let (first, _) = bench
        .begin(1, "a", new("op-5", ValidationState::Running))
        .unwrap();
    bench
        .finish(1, "a", &first.result_id, to(ValidationState::Failed, None))
        .unwrap();
    let (other, _) = bench
        .begin(1, "a", new("op-6", ValidationState::Running))
        .unwrap();
    assert_eq!(
        validation_error(bench.begin(1, "a", new("op-5", ValidationState::Running))),
        ValidationError::StepRunning
    );
    bench
        .finish(1, "a", &other.result_id, to(ValidationState::Passed, None))
        .unwrap();
    bench.define(
        1,
        StepDefinition {
            retry: RetryPolicy { max_attempts: 5 },
            ..step("a")
        },
    );
    let (stale, created) = bench
        .begin(1, "a", new("op-5", ValidationState::Running))
        .unwrap();
    assert!(!created);
    assert_eq!(stale.result_id, first.result_id);
    // A skip is never a retry of a failure either.
    let (skip, created) = bench
        .begin(1, "a", new("op-5", ValidationState::Skipped))
        .unwrap();
    assert!(!created);
    assert_eq!(skip.state, ValidationState::Failed);
}

#[test]
fn hs2_wn0mxz_a_running_attempt_past_its_deadline_is_unverified() {
    let mut bench = Bench::new();
    bench.define(1, step("a"));
    let (running, _) = bench
        .begin(1, "a", new("op-1", ValidationState::Running))
        .unwrap();
    let deadline = running.deadline_ms.unwrap();
    bench.clock.0.set(deadline);
    assert_eq!(bench.state(1, "a"), (ValidationState::Running, None));
    bench.clock.0.set(deadline + 1);
    // A read says so without writing: the stored result is still as recorded.
    let evidence = bench.evidence(1, "a");
    assert_eq!(evidence.state, ValidationState::Unverified);
    assert_eq!(evidence.reason.as_deref(), Some("deadline_passed"));
    assert_eq!(evidence.latest.unwrap().state, ValidationState::Running);
    // The next request records the lapse and then takes its own attempt.
    let (next, created) = bench
        .begin(1, "a", new("op-2", ValidationState::Running))
        .unwrap();
    assert!(created);
    let history = bench.evidence(1, "a").history;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].state, ValidationState::Unverified);
    assert_eq!(history[0].reason.as_deref(), Some("deadline_passed"));
    assert_eq!(history[0].finished_at_ms, Some(deadline + 1));
    // A late report for the lapsed attempt cannot rewrite it.
    assert_eq!(
        validation_error(bench.finish(
            1,
            "a",
            &running.result_id,
            to(ValidationState::Passed, None)
        )),
        ValidationError::ResultFinal
    );
    bench
        .finish(1, "a", &next.result_id, to(ValidationState::Passed, None))
        .unwrap();
    assert_eq!(bench.state(1, "a"), (ValidationState::Passed, None));
    // The lapsed request itself is answered with what was recorded, and that is kept.
    let (own, _) = bench
        .begin(1, "a", new("op-3", ValidationState::Running))
        .unwrap();
    bench.clock.0.set(own.deadline_ms.unwrap() + 5);
    let (lapsed, created) = bench
        .begin(1, "a", new("op-3", ValidationState::Running))
        .unwrap();
    assert!(!created);
    assert_eq!(lapsed.state, ValidationState::Unverified);
    assert_eq!(bench.evidence(1, "a").latest, Some(lapsed));
    // A lapsed attempt blocks neither a new definition nor removal.
    let (blocked, _) = bench
        .begin(1, "a", new("op-4", ValidationState::Running))
        .unwrap();
    bench.clock.0.set(blocked.deadline_ms.unwrap() + 1);
    assert_eq!(
        bench
            .define(
                1,
                StepDefinition {
                    timeout_seconds: 30,
                    ..step("a")
                }
            )
            .0
            .revision,
        2
    );
    assert_eq!(
        bench.evidence(1, "a").latest.unwrap().state,
        ValidationState::Unverified
    );
    let (held, _) = bench
        .begin(1, "a", new("op-5", ValidationState::Running))
        .unwrap();
    let after = held.deadline_ms.unwrap() + 1;
    assert!(bench.registry.remove(OWNER, &ticket(1), "a", after).is_ok());
}

#[test]
fn hs2_wn0mxz_prerequisites_follow_the_latest_evidence_in_any_order() {
    let mut bench = Bench::new();
    let needs = |name: &str, prerequisites: &[&str]| StepDefinition {
        prerequisites: prerequisites
            .iter()
            .map(|step| (*step).to_owned())
            .collect(),
        ..step(name)
    };
    // A step may be defined before the steps it needs.
    bench.define(1, needs("deploy", &["build", "test"]));
    let unmet = |bench: &Bench| {
        bench
            .registry
            .unmet_prerequisite(OWNER, &ticket(1), "deploy", bench.clock.0.get())
            .unwrap()
    };
    assert_eq!(unmet(&bench).as_deref(), Some("build"));
    bench.define(1, step("build"));
    assert_eq!(unmet(&bench).as_deref(), Some("build"));
    // Out of order: the second prerequisite passes first.
    bench.define(1, step("test"));
    bench.attempt(1, "test", "op-test-1", ValidationState::Passed);
    assert_eq!(unmet(&bench).as_deref(), Some("build"));
    let (building, _) = bench
        .begin(1, "build", new("op-build-1", ValidationState::Running))
        .unwrap();
    assert_eq!(unmet(&bench).as_deref(), Some("build"));
    bench
        .finish(
            1,
            "build",
            &building.result_id,
            to(ValidationState::Failed, None),
        )
        .unwrap();
    assert_eq!(unmet(&bench).as_deref(), Some("build"));
    bench.attempt(1, "build", "op-build-2", ValidationState::Passed);
    assert_eq!(unmet(&bench), None);
    // Every state but `passed` leaves it unmet, whatever came before.
    for (n, state) in [
        ValidationState::Failed,
        ValidationState::Unavailable,
        ValidationState::Unverified,
    ]
    .into_iter()
    .enumerate()
    {
        bench.attempt(1, "test", &format!("op-test-bad-{n}"), state);
        assert_eq!(unmet(&bench).as_deref(), Some("test"), "{state:?}");
        bench.attempt(
            1,
            "test",
            &format!("op-test-good-{n}"),
            ValidationState::Passed,
        );
        assert_eq!(unmet(&bench), None);
    }
    bench
        .begin(1, "test", new("op-test-skip", ValidationState::Skipped))
        .unwrap();
    assert_eq!(unmet(&bench).as_deref(), Some("test"));
    bench.attempt(1, "test", "op-test-again", ValidationState::Passed);
    // A redefined prerequisite must pass again; a removed one is unmet.
    bench.define(
        1,
        StepDefinition {
            timeout_seconds: 99,
            ..step("build")
        },
    );
    assert_eq!(unmet(&bench).as_deref(), Some("build"));
    bench.attempt(1, "build", "op-build-3", ValidationState::Passed);
    assert_eq!(unmet(&bench), None);
    bench.registry.remove(OWNER, &ticket(1), "test", 0).unwrap();
    assert_eq!(unmet(&bench).as_deref(), Some("test"));
    // The same step name on another ticket or owner never satisfies it.
    bench.define(2, step("test"));
    bench.attempt(2, "test", "op-other-ticket", ValidationState::Passed);
    assert_eq!(unmet(&bench).as_deref(), Some("test"));
    // A step without prerequisites has none unmet; an unknown step is not found.
    assert_eq!(
        bench
            .registry
            .unmet_prerequisite(OWNER, &ticket(1), "build", 0)
            .unwrap(),
        None
    );
    assert_eq!(
        validation_error(
            bench
                .registry
                .unmet_prerequisite(OWNER, &ticket(1), "nope", 0)
        ),
        ValidationError::StepNotFound
    );
}

#[test]
fn hs2_wn0mxz_steps_of_tickets_and_owners_interleave_without_crossing() {
    let mut bench = Bench::new();
    bench.define(1, step("a"));
    bench.define(1, step("b"));
    bench.define(2, step("a"));
    let other = bench
        .registry
        .define(
            "other-owner",
            ticket(1),
            step("a"),
            None,
            &mut Transaction {
                clock: &bench.clock,
                rng: &mut bench.rng,
            },
        )
        .unwrap()
        .0;
    assert_eq!(other.revision, 1);
    // Interleaved attempts on three steps finish out of order.
    let (one_a, _) = bench
        .begin(1, "a", new("op-1a", ValidationState::Running))
        .unwrap();
    let (one_b, _) = bench
        .begin(1, "b", new("op-1b", ValidationState::Running))
        .unwrap();
    let (two_a, _) = bench
        .begin(2, "a", new("op-2a", ValidationState::Running))
        .unwrap();
    bench
        .finish(2, "a", &two_a.result_id, to(ValidationState::Failed, None))
        .unwrap();
    bench
        .finish(1, "b", &one_b.result_id, to(ValidationState::Passed, None))
        .unwrap();
    assert_eq!(bench.state(1, "a").0, ValidationState::Running);
    assert_eq!(bench.state(1, "b").0, ValidationState::Passed);
    assert_eq!(bench.state(2, "a").0, ValidationState::Failed);
    // A result belongs to its step: the id of one is unknown to another.
    assert_eq!(
        validation_error(bench.finish(1, "b", &one_a.result_id, to(ValidationState::Passed, None))),
        ValidationError::ResultNotFound
    );
    bench
        .finish(
            1,
            "a",
            &one_a.result_id,
            to(ValidationState::Unavailable, None),
        )
        .unwrap();
    // An operation id names one step of its owner; another owner may reuse it.
    assert!(matches!(
        bench.begin(1, "b", new("op-1a", ValidationState::Running)),
        Err(OrchestrationError::Conflict)
    ));
    assert!(matches!(
        bench.begin(2, "a", new("op-1b", ValidationState::Running)),
        Err(OrchestrationError::Conflict)
    ));
    let (foreign, created) = bench
        .registry
        .begin(
            "other-owner",
            &ticket(1),
            "a",
            new("op-1a", ValidationState::Running),
            &mut Transaction {
                clock: &bench.clock,
                rng: &mut bench.rng,
            },
        )
        .unwrap();
    assert!(created);
    assert_ne!(foreign.result_id, one_a.result_id);
    // Listing is per owner, optionally per ticket, in definition order.
    let names = |evidence: Vec<ValidationEvidence>| {
        evidence
            .into_iter()
            .map(|e| (e.step.ticket.native_id, e.step.definition.step_id, e.state))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(bench.registry.steps(OWNER, None, 1_000).unwrap()),
        [
            (id(1), "a".into(), ValidationState::Unavailable),
            (id(1), "b".into(), ValidationState::Passed),
            (id(2), "a".into(), ValidationState::Failed),
        ]
    );
    assert_eq!(
        names(
            bench
                .registry
                .steps(OWNER, Some(&ticket(2)), 1_000)
                .unwrap()
        ),
        [(id(2), "a".into(), ValidationState::Failed)]
    );
    assert_eq!(
        names(bench.registry.steps("other-owner", None, 1_000).unwrap()),
        [(id(1), "a".into(), ValidationState::Running)]
    );
    assert!(
        bench
            .registry
            .steps("nobody", None, 1_000)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hs2_wn0mxz_empty_then_refill_starts_from_nothing_and_bounds_hold() {
    let mut bench = Bench::new();
    // A read of an absent file creates nothing.
    assert!(bench.registry.steps(OWNER, None, 0).unwrap().is_empty());
    assert!(
        !bench
            ._home
            .path()
            .join("orchestration-validation.json")
            .exists()
    );
    bench.define(1, step("a"));
    let old = bench.attempt(1, "a", "op-1", ValidationState::Passed);
    bench.define(
        1,
        StepDefinition {
            timeout_seconds: 11,
            ..step("a")
        },
    );
    bench.registry.remove(OWNER, &ticket(1), "a", 0).unwrap();
    assert!(bench.registry.steps(OWNER, None, 0).unwrap().is_empty());
    assert_eq!(
        validation_error(bench.registry.remove(OWNER, &ticket(1), "a", 0)),
        ValidationError::StepNotFound
    );
    // Refilled: revision 1 again, no inherited evidence, and the old operation id
    // starts a new attempt because nothing remembers the removed one.
    let (again, created) = bench.define(1, step("a"));
    assert!(created);
    assert_eq!(again.revision, 1);
    assert_eq!(
        bench.state(1, "a"),
        (ValidationState::Unverified, Some("not_run".into()))
    );
    let (fresh, created) = bench
        .begin(1, "a", new("op-1", ValidationState::Running))
        .unwrap();
    assert!(created);
    assert_ne!(fresh.result_id, old.result_id);
    bench
        .finish(1, "a", &fresh.result_id, to(ValidationState::Passed, None))
        .unwrap();
    // Results are bounded: the oldest are forgotten and the newest is always kept.
    for n in 0..(MAX_STEP_RESULTS + 4) {
        bench.attempt(1, "a", &format!("fill-{n}"), ValidationState::Failed);
    }
    let history = bench.evidence(1, "a").history;
    assert_eq!(history.len(), MAX_STEP_RESULTS);
    assert_eq!(
        history.last().unwrap().operation_id,
        format!("fill-{}", MAX_STEP_RESULTS + 3)
    );
    assert_eq!(history[0].operation_id, "fill-4");
    // A ticket holds at most 32 steps; another ticket still has room.
    for n in 1..MAX_STEPS_PER_TICKET {
        bench.define(1, step(&format!("s{n}")));
    }
    assert!(matches!(
        bench.try_define(1, step("one-too-many")),
        Err(OrchestrationError::Capacity)
    ));
    assert!(
        bench
            .try_define(
                1,
                StepDefinition {
                    timeout_seconds: 12,
                    ..step("a")
                }
            )
            .is_ok()
    );
    assert!(bench.try_define(2, step("a")).is_ok());
    // The file is private to the machine owner.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = bench._home.path().join("orchestration-validation.json");
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn hs2_wn0mxz_damaged_or_newer_validation_data_is_an_error_and_is_not_overwritten() {
    for (bytes, newer) in [
        (b"{not json".to_vec(), false),
        (
            br#"{"schema_version":1,"steps":[{"step":{}}]}"#.to_vec(),
            false,
        ),
        (br#"{"schema_version":2,"steps":[]}"#.to_vec(), true),
    ] {
        let mut bench = Bench::new();
        let path = bench._home.path().join("orchestration-validation.json");
        std::fs::write(&path, &bytes).unwrap();
        let read = bench.registry.steps(OWNER, None, 0);
        let written = bench.try_define(1, step("a"));
        if newer {
            assert!(matches!(
                read,
                Err(OrchestrationError::UnsupportedVersion { found: 2 })
            ));
            assert!(matches!(
                written,
                Err(OrchestrationError::UnsupportedVersion { found: 2 })
            ));
        } else {
            assert!(matches!(read, Err(OrchestrationError::Corrupt)));
            assert!(matches!(written, Err(OrchestrationError::Corrupt)));
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn hs2_wn0mxz_target_profiles_are_read_from_the_checked_in_file_only() {
    let checkout = tempfile::tempdir().unwrap();
    let store = ValidationProfileStore::new(checkout.path());
    // Absent: no profiles, and the read creates nothing.
    assert!(store.read().unwrap().is_empty());
    assert_eq!(store.find("smoke").unwrap(), None);
    assert!(!checkout.path().join(".hotsheet2").exists());
    std::fs::create_dir(checkout.path().join(".hotsheet2")).unwrap();
    let path = checkout.path().join(".hotsheet2/validation-profiles.json");
    std::fs::write(
        &path,
        br#"{"schema_version":1,"profiles":[
            {"name":"smoke","program":"scripts/check.sh","args":["--smoke"]},
            {"name":"lint","program":"cargo"}]}"#,
    )
    .unwrap();
    assert_eq!(
        store.find("smoke").unwrap(),
        Some(ValidationProfile {
            name: "smoke".into(),
            program: "scripts/check.sh".into(),
            args: vec!["--smoke".into()],
        })
    );
    assert_eq!(
        store.find("lint").unwrap().unwrap().args,
        Vec::<String>::new()
    );
    assert_eq!(store.find("deploy").unwrap(), None);
    assert!(store.find("not a name").is_err());
    for (bytes, newer) in [
        (&br#"{"schema_version":1,"profiles":[{"name":"a","program":"x"},{"name":"a","program":"y"}]}"#[..], false),
        (&br#"{"schema_version":1,"profiles":[{"name":"a b","program":"x"}]}"#[..], false),
        (&br#"{"schema_version":1,"profiles":[{"name":"a","program":""}]}"#[..], false),
        (&br#"{"schema_version":1,"profiles":[{"name":"a","program":"x","shell":true}]}"#[..], false),
        (&br#"{"schema_version":3,"profiles":[]}"#[..], true),
    ] {
        std::fs::write(&path, bytes).unwrap();
        match store.find("a") {
            Err(OrchestrationError::UnsupportedVersion { found: 3 }) => assert!(newer),
            Err(OrchestrationError::Corrupt) => assert!(!newer),
            other => panic!("expected an error, got {other:?}"),
        }
    }
}

/// HS2-J7N3PQ: the first instant a running attempt's deadline changes a level.
#[test]
fn hs2_j7n3pq_the_next_lapse_is_the_first_running_deadline_ahead() {
    let result = |state: ValidationState, deadline_ms: Option<u64>| ValidationResult {
        result_id: "r".into(),
        operation_id: "op".into(),
        attempt: 1,
        step_revision: 1,
        state,
        reason: None,
        actor_id: None,
        run_id: None,
        host_started: false,
        created_at_ms: 0,
        deadline_ms,
        finished_at_ms: None,
        outcome: None,
    };
    let evidence = |latest: Option<ValidationResult>| ValidationEvidence {
        step: ValidationStep {
            owner_checkout_id: OWNER.into(),
            ticket: ticket(1),
            definition: step("smoke"),
            revision: 1,
            created_at_ms: 0,
            updated_at_ms: 0,
            updated_by: None,
        },
        state: ValidationState::Running,
        reason: None,
        latest,
        history: Vec::new(),
    };
    // Empty, then only finished or deadline-free attempts: nothing is due.
    assert_eq!(next_lapse_ms(&[], 10), None);
    assert_eq!(
        next_lapse_ms(
            &[
                evidence(None),
                evidence(Some(result(ValidationState::Passed, Some(50)))),
                evidence(Some(result(ValidationState::Running, None))),
            ],
            10
        ),
        None
    );
    // The earliest running deadline ahead, one millisecond after it (a deadline is
    // passed once now is beyond it).
    let both = [
        evidence(Some(result(ValidationState::Running, Some(500)))),
        evidence(Some(result(ValidationState::Running, Some(300)))),
    ];
    assert_eq!(next_lapse_ms(&both, 10), Some(301));
    // At the deadline it is still ahead; past it, the next one is.
    assert_eq!(next_lapse_ms(&both, 300), Some(301));
    assert_eq!(next_lapse_ms(&both, 301), Some(501));
    assert_eq!(next_lapse_ms(&both, 501), None);
}
