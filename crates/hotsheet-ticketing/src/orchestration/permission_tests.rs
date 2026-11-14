//! HS2-XHQ1EH: orchestrator permission requests, remembered rules and decisions.
//! Synthetic homes only. Covers the state-transition matrix and repeated, out-of-order,
//! interleaved and empty-then-refill sequences.

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
fn target(value: u128) -> TargetRef {
    TargetRef {
        owner_checkout_id: OWNER.into(),
        target_id: ulid::Ulid::from(value).to_string(),
    }
}
fn key(target_value: u128, action: PermissionAction, tool: Option<&str>) -> PermissionKey {
    PermissionKey {
        target: target(target_value),
        action,
        tool: tool.map(Into::into),
    }
}
fn ask(key: &PermissionKey, operation: &str) -> PermissionAsk {
    PermissionAsk {
        key: key.clone(),
        model: None,
        operation_id: operation.into(),
        actor_id: "scheduler".into(),
        each_time: false,
    }
}
fn pending_id(answer: Asked) -> String {
    match answer {
        Asked::Pending { request, .. } => request.request_id,
        other => panic!("expected a pending request, got {other:?}"),
    }
}
fn states(ledger: &PermissionLedger, now_ms: u64) -> Vec<PermissionState> {
    ledger
        .list(OWNER, now_ms)
        .unwrap()
        .into_iter()
        .map(|request| request.state)
        .collect()
}

struct Fixture {
    _home: tempfile::TempDir,
    ledger: PermissionLedger,
    clock: Clock,
    rng: Rng,
}
impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        Self {
            ledger: PermissionLedger::new(home.path()),
            _home: home,
            clock: Clock(1_000.into()),
            rng: Rng(1),
        }
    }
    fn tx(&mut self) -> Transaction<'_> {
        Transaction {
            clock: &self.clock,
            rng: &mut self.rng,
        }
    }
    fn ask(&mut self, ask: &PermissionAsk) -> Asked {
        let ledger = self.ledger.clone();
        ledger.ask(ask, &mut self.tx()).unwrap()
    }
    fn answer(
        &mut self,
        request_id: &str,
        answer: PermissionAnswer,
    ) -> Result<Answered, OrchestrationError> {
        let ledger = self.ledger.clone();
        ledger.answer(
            OWNER,
            request_id,
            answer,
            Some("maintainer".into()),
            &mut self.tx(),
        )
    }
    fn used(&mut self, request_id: &str) -> bool {
        let ledger = self.ledger.clone();
        ledger.used(request_id, &mut self.tx()).unwrap()
    }
}

#[test]
fn hs2_xhq1eh_the_action_vocabulary_decides_what_asks() {
    use TargetAction as A;
    let session = |starting| SessionRequest {
        tool: "codex".into(),
        model: None,
        running: 0,
        starting,
    };
    let cases: Vec<(AccessRequest, Option<PermissionAction>)> = vec![
        (AccessRequest::new("a", A::TicketsRead), None),
        (AccessRequest::new("a", A::GatesObserve), None),
        (
            AccessRequest::new("a", A::TicketsWrite),
            Some(PermissionAction::TicketsWrite),
        ),
        (
            AccessRequest::new("a", A::WorkStart),
            Some(PermissionAction::WorkStart),
        ),
        (
            AccessRequest::new("a", A::ValidationRun),
            Some(PermissionAction::ValidationRun),
        ),
        (
            AccessRequest::new("a", A::DeploymentsRun),
            Some(PermissionAction::DeploymentsRun),
        ),
        (
            AccessRequest {
                session: Some(session(true)),
                ..AccessRequest::new("a", A::SessionsManage)
            },
            Some(PermissionAction::SessionStart),
        ),
        // Stopping or winding down only reduces activity: it never asks.
        (
            AccessRequest {
                session: Some(session(false)),
                ..AccessRequest::new("a", A::SessionsManage)
            },
            None,
        ),
        // A provisional action asks under its own key, whatever it starts.
        (
            AccessRequest {
                provisional: true,
                ..AccessRequest::new("a", A::WorkStart)
            },
            Some(PermissionAction::ProvisionalDispatch),
        ),
        // A read stays a read even when called provisional.
        (
            AccessRequest {
                provisional: true,
                ..AccessRequest::new("a", A::TicketsRead)
            },
            None,
        ),
    ];
    for (request, expected) in cases {
        assert_eq!(
            PermissionAction::for_request(&request),
            expected,
            "{request:?}"
        );
    }
    assert!(PermissionAction::SessionStart.takes_tool());
    for action in PermissionAction::ALL {
        assert_eq!(
            action.takes_tool(),
            action == PermissionAction::SessionStart
        );
        let code = serde_json::to_value(action).unwrap();
        assert_eq!(code.as_str(), Some(action.code()));
    }
    // A key names a tool exactly when it is a session start.
    assert!(
        key(1, PermissionAction::SessionStart, None)
            .validate()
            .is_err()
    );
    assert!(
        key(1, PermissionAction::WorkStart, Some("codex"))
            .validate()
            .is_err()
    );
    assert!(
        key(1, PermissionAction::SessionStart, Some("codex"))
            .validate()
            .is_ok()
    );
    assert!(
        key(1, PermissionAction::SessionStart, Some("bad tool"))
            .validate()
            .is_err()
    );
}

#[test]
fn hs2_xhq1eh_transition_matrix_from_every_state() {
    // From each state, every answer and use: the state reached, or a conflict.
    use PermissionAnswer as Ans;
    use PermissionState as S;
    #[derive(Clone, Copy, Debug)]
    enum Step {
        Answer(PermissionAnswer),
        Use,
    }
    let reach = |fixture: &mut Fixture, state: S| -> String {
        let id = pending_id(fixture.ask(&ask(&key(1, PermissionAction::WorkStart, None), "op")));
        match state {
            S::Pending => {}
            S::Allowed => {
                fixture.answer(&id, Ans::AllowOnce).unwrap();
            }
            S::Denied => {
                fixture.answer(&id, Ans::Deny).unwrap();
            }
            S::Used => {
                fixture.answer(&id, Ans::AllowOnce).unwrap();
                assert!(fixture.used(&id));
            }
            S::Lapsed => fixture.clock.0.set(1_000 + PERMISSION_PENDING_MS),
        }
        id
    };
    let table: Vec<(S, Step, Result<S, ()>)> = vec![
        (S::Pending, Step::Answer(Ans::AllowOnce), Ok(S::Allowed)),
        (S::Pending, Step::Answer(Ans::AlwaysAllow), Ok(S::Allowed)),
        (S::Pending, Step::Answer(Ans::Deny), Ok(S::Denied)),
        (S::Pending, Step::Use, Ok(S::Pending)),
        (S::Allowed, Step::Answer(Ans::AllowOnce), Ok(S::Allowed)),
        (S::Allowed, Step::Answer(Ans::AlwaysAllow), Err(())),
        (S::Allowed, Step::Answer(Ans::Deny), Err(())),
        (S::Allowed, Step::Use, Ok(S::Used)),
        (S::Denied, Step::Answer(Ans::AllowOnce), Err(())),
        (S::Denied, Step::Answer(Ans::AlwaysAllow), Err(())),
        (S::Denied, Step::Answer(Ans::Deny), Ok(S::Denied)),
        (S::Denied, Step::Use, Ok(S::Denied)),
        (S::Used, Step::Answer(Ans::AllowOnce), Ok(S::Used)),
        (S::Used, Step::Answer(Ans::Deny), Err(())),
        (S::Used, Step::Use, Ok(S::Used)),
        (S::Lapsed, Step::Answer(Ans::AllowOnce), Err(())),
        (S::Lapsed, Step::Answer(Ans::Deny), Err(())),
        (S::Lapsed, Step::Use, Ok(S::Lapsed)),
    ];
    for (from, step, expected) in table {
        let mut fixture = Fixture::new();
        let id = reach(&mut fixture, from);
        let result = match step {
            Step::Answer(answer) => fixture.answer(&id, answer).map(|_| ()),
            Step::Use => {
                fixture.used(&id);
                Ok(())
            }
        };
        let now = fixture.clock.0.get();
        let reached = fixture.ledger.get(OWNER, &id, now).unwrap().state;
        match expected {
            Ok(state) => {
                assert!(result.is_ok(), "{from:?} {step:?}: {result:?}");
                assert_eq!(reached, state, "{from:?} {step:?}");
            }
            Err(()) => {
                assert!(
                    matches!(result, Err(OrchestrationError::Conflict)),
                    "{from:?} {step:?}: {result:?}"
                );
                assert_eq!(reached, from, "a refused answer changes nothing");
            }
        }
    }
}

#[test]
fn hs2_xhq1eh_asking_again_files_nothing_and_allow_once_is_used_only_when_it_happens() {
    let mut f = Fixture::new();
    assert!(f.ledger.list(OWNER, 1_000).unwrap().is_empty());
    let work = key(1, PermissionAction::WorkStart, None);
    // Reading the empty ledger creates no file.
    assert!(f.ledger.peek(&ask(&work, "op-1"), 1_000).unwrap().is_none());
    let first = f.ask(&ask(&work, "op-1"));
    let Asked::Pending { request, filed } = first else {
        panic!("first ask files a request");
    };
    assert!(filed);
    // Repeated asks, as every scheduler pass makes, add nothing.
    for _ in 0..3 {
        assert_eq!(
            f.ask(&ask(&work, "op-1")),
            Asked::Pending {
                request: request.clone(),
                filed: false
            }
        );
    }
    assert_eq!(states(&f.ledger, 1_000), [PermissionState::Pending]);
    f.clock.0.set(2_000);
    let answered = f
        .answer(&request.request_id, PermissionAnswer::AllowOnce)
        .unwrap();
    assert!(answered.changed);
    assert!(answered.rule.is_none());
    assert_eq!(answered.request.answered_by.as_deref(), Some("maintainer"));
    // HS2-8NJVXC / HS2-RTX8RJ: the gate passing does not use it. A refused claim or an
    // unsafe stop asks again and is still allowed.
    for _ in 0..3 {
        assert!(matches!(f.ask(&ask(&work, "op-1")), Asked::AllowedOnce(_)));
    }
    assert!(f.used(&request.request_id));
    assert!(!f.used(&request.request_id), "using twice changes nothing");
    // Used: the same operation asks again with a new request.
    let again = pending_id(f.ask(&ask(&work, "op-1")));
    assert_ne!(again, request.request_id);
    assert_eq!(
        states(&f.ledger, 2_000),
        [PermissionState::Used, PermissionState::Pending]
    );
    // Allow Once for one operation says nothing about another.
    assert!(matches!(
        f.ask(&ask(&work, "op-2")),
        Asked::Pending { filed: true, .. }
    ));
}

#[test]
fn hs2_xhq1eh_lapses_deny_finality_and_out_of_order_answers() {
    let mut f = Fixture::new();
    let work = key(1, PermissionAction::WorkStart, None);
    let tickets = key(1, PermissionAction::TicketsWrite, None);
    let a = pending_id(f.ask(&ask(&work, "op-a")));
    let b = pending_id(f.ask(&ask(&tickets, "op-b")));
    // Answered in the other order: each answer reaches only its own request.
    f.answer(&b, PermissionAnswer::Deny).unwrap();
    f.answer(&a, PermissionAnswer::AllowOnce).unwrap();
    assert!(matches!(f.ask(&ask(&tickets, "op-b")), Asked::Denied(_)));
    assert!(matches!(f.ask(&ask(&work, "op-a")), Asked::AllowedOnce(_)));
    // Deny is final for its operation however often it is asked, even much later.
    f.clock.0.set(1_000 + 10 * PERMISSION_PENDING_MS);
    assert!(matches!(f.ask(&ask(&tickets, "op-b")), Asked::Denied(_)));
    // An unused Allow Once lapses after an hour, then the operation asks again.
    assert!(matches!(
        f.ask(&ask(&work, "op-a")),
        Asked::Pending { filed: true, .. }
    ));
    // An unanswered request lapses after a day; asking files a fresh one.
    let mut f = Fixture::new();
    let c = pending_id(f.ask(&ask(&work, "op-c")));
    f.clock.0.set(1_000 + PERMISSION_PENDING_MS - 1);
    assert_eq!(pending_id(f.ask(&ask(&work, "op-c"))), c);
    f.clock.0.set(1_000 + PERMISSION_PENDING_MS);
    let d = pending_id(f.ask(&ask(&work, "op-c")));
    assert_ne!(c, d);
    assert!(matches!(
        f.answer(&c, PermissionAnswer::AllowOnce),
        Err(OrchestrationError::Conflict)
    ));
    // An unknown request, or one of another owner, is not found.
    assert!(matches!(
        f.answer(
            &ulid::Ulid::from(999u128).to_string(),
            PermissionAnswer::Deny
        ),
        Err(OrchestrationError::NotFound)
    ));
    let ledger = f.ledger.clone();
    assert!(matches!(
        ledger.answer("other-owner", &d, PermissionAnswer::Deny, None, &mut f.tx()),
        Err(OrchestrationError::NotFound)
    ));
}

#[test]
fn hs2_xhq1eh_always_allow_remembers_a_rule_by_key_and_answers_waiting_requests() {
    let mut f = Fixture::new();
    let codex = key(1, PermissionAction::SessionStart, Some("codex"));
    let claude = key(1, PermissionAction::SessionStart, Some("claude"));
    let other_target = key(2, PermissionAction::SessionStart, Some("codex"));
    // Interleaved: three waiting starts on the same key, one on another tool, one on
    // another target.
    let first = pending_id(f.ask(&ask(&codex, "start-1")));
    let second = pending_id(f.ask(&ask(&codex, "start-2")));
    let other_tool = pending_id(f.ask(&ask(&claude, "start-3")));
    let elsewhere = pending_id(f.ask(&ask(&other_target, "start-4")));
    let third = pending_id(f.ask(&ask(&codex, "start-5")));
    let answered = f.answer(&second, PermissionAnswer::AlwaysAllow).unwrap();
    let rule = answered.rule.clone().unwrap();
    assert_eq!(rule.key, codex);
    assert_eq!(rule.request_id.as_deref(), Some(second.as_str()));
    assert_eq!(
        answered.request.rule_id.as_deref(),
        Some(rule.rule_id.as_str())
    );
    assert_eq!(answered.also_allowed, [first.clone(), third.clone()]);
    // The rule answers every later operation with the key, and only that key.
    assert_eq!(
        f.ask(&ask(&codex, "start-9")),
        Asked::Remembered(rule.clone())
    );
    assert!(matches!(
        f.ask(&ask(&claude, "start-3")),
        Asked::Pending { .. }
    ));
    assert!(matches!(
        f.ask(&ask(&other_target, "start-4")),
        Asked::Pending { .. }
    ));
    let now = f.clock.0.get();
    for id in [&other_tool, &elsewhere] {
        assert_eq!(
            f.ledger.get(OWNER, id, now).unwrap().state,
            PermissionState::Pending
        );
    }
    // Answering Always Allow again on the same key reuses the rule.
    let again = f.answer(&second, PermissionAnswer::AlwaysAllow).unwrap();
    assert!(!again.changed);
    assert_eq!(f.ledger.rules(OWNER).unwrap(), std::slice::from_ref(&rule));
    // An Always Allow answer never lapses into asking again while its rule lives.
    f.clock.0.set(now + 10 * PERMISSION_ALLOWED_MS);
    assert_eq!(
        f.ask(&ask(&codex, "start-1")),
        Asked::Remembered(rule.clone())
    );
    // Forgetting the rule: the key asks again (empty, then refilled).
    assert_eq!(f.ledger.forget(OWNER, &rule.rule_id).unwrap(), rule);
    assert!(matches!(
        f.ledger.forget(OWNER, &rule.rule_id),
        Err(OrchestrationError::NotFound)
    ));
    assert!(f.ledger.rules(OWNER).unwrap().is_empty());
    let refill = pending_id(f.ask(&ask(&codex, "start-10")));
    let answered = f.answer(&refill, PermissionAnswer::AlwaysAllow).unwrap();
    let new_rule = answered.rule.unwrap();
    assert_ne!(new_rule.rule_id, rule.rule_id);
    assert_eq!(
        f.ledger.all_rules().unwrap(),
        std::slice::from_ref(&new_rule)
    );
    // Another owner's rules are not listed or forgettable here.
    assert!(matches!(
        f.ledger.forget("other-owner", &new_rule.rule_id),
        Err(OrchestrationError::NotFound)
    ));
}

#[test]
fn hs2_xhq1eh_peek_never_writes_and_pending_everywhere_lists_waiting_requests() {
    let mut f = Fixture::new();
    let work = key(1, PermissionAction::WorkStart, None);
    assert!(f.ledger.pending_everywhere(1_000).unwrap().is_empty());
    let id = pending_id(f.ask(&ask(&work, "op")));
    assert!(matches!(
        f.ledger.peek(&ask(&work, "op"), 1_000).unwrap(),
        Some(Asked::Pending { filed: false, .. })
    ));
    assert!(
        f.ledger
            .peek(&ask(&work, "other"), 1_000)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.ledger.list(OWNER, 1_000).unwrap().len(), 1);
    assert_eq!(
        f.ledger
            .pending_everywhere(1_000)
            .unwrap()
            .into_iter()
            .map(|request| request.request_id)
            .collect::<Vec<_>>(),
        std::slice::from_ref(&id)
    );
    f.answer(&id, PermissionAnswer::AllowOnce).unwrap();
    assert!(f.ledger.pending_everywhere(1_000).unwrap().is_empty());
    assert!(matches!(
        f.ledger.peek(&ask(&work, "op"), 1_000).unwrap(),
        Some(Asked::AllowedOnce(_))
    ));
}

#[test]
fn hs2_xhq1eh_capacity_drops_finished_requests_first_and_rejects_bad_records() {
    let mut f = Fixture::new();
    let work = key(1, PermissionAction::WorkStart, None);
    let mut kept = Vec::new();
    for index in 0..MAX_PERMISSION_REQUESTS {
        let id = pending_id(f.ask(&ask(&work, &format!("op-{index}"))));
        if index % 2 == 0 {
            f.answer(&id, PermissionAnswer::AllowOnce).unwrap();
            f.used(&id);
        } else {
            kept.push(id);
        }
    }
    // Full: a new request drops the oldest used one; waiting ones stay.
    assert!(matches!(
        f.ask(&ask(&work, "op-new")),
        Asked::Pending { filed: true, .. }
    ));
    let now = f.clock.0.get();
    let listed = f.ledger.list(OWNER, now).unwrap();
    assert_eq!(listed.len(), MAX_PERMISSION_REQUESTS);
    for id in &kept {
        assert!(listed.iter().any(|request| &request.request_id == id));
    }
    // Nothing finished is left to drop: a full ledger of waiting requests refuses.
    let mut f = Fixture::new();
    for index in 0..MAX_PERMISSION_REQUESTS {
        f.ask(&ask(&work, &format!("op-{index}")));
    }
    let ledger = f.ledger.clone();
    assert!(matches!(
        ledger.ask(&ask(&work, "op-new"), &mut f.tx()),
        Err(OrchestrationError::Capacity)
    ));
    // Malformed questions never reach the file.
    let mut bad = ask(&work, "op with space");
    assert!(matches!(
        ledger.ask(&bad, &mut f.tx()),
        Err(OrchestrationError::Invalid(_))
    ));
    bad = ask(&key(1, PermissionAction::SessionStart, None), "op");
    assert!(ledger.ask(&bad, &mut f.tx()).is_err());
}

#[test]
fn hs2_xhq1eh_a_corrupt_or_newer_ledger_is_never_trusted() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("orchestration-permissions.json");
    let ledger = PermissionLedger::new(home.path());
    let work = key(1, PermissionAction::WorkStart, None);
    std::fs::write(&path, br#"{"schema_version":2,"requests":[]}"#).unwrap();
    assert!(matches!(
        ledger.peek(&ask(&work, "op"), 1),
        Err(OrchestrationError::UnsupportedVersion { found: 2 })
    ));
    std::fs::write(&path, b"not json").unwrap();
    assert!(matches!(
        ledger.peek(&ask(&work, "op"), 1),
        Err(OrchestrationError::Corrupt)
    ));
    // A rule naming a tool for a non-session action is rejected as a whole file.
    let bad = serde_json::json!({
        "schema_version": 1,
        "requests": [],
        "rules": [{
            "rule_id": ulid::Ulid::from(5u128).to_string(),
            "target": target(1),
            "action": "work_start",
            "tool": "codex",
            "created_at_ms": 1
        }]
    });
    std::fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
    assert!(matches!(
        ledger.all_rules(),
        Err(OrchestrationError::Corrupt)
    ));
}

#[test]
fn hs2_xhq1eh_retired_grant_and_approval_files_are_reported_never_read() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(retired_authority(home.path()), RetiredAuthority::default());
    assert!(!retired_authority(home.path()).found());
    std::fs::write(
        home.path().join(RETIRED_GRANTS_FILE),
        br#"{"schema_version":1,"grants":[{"grant_id":"x"},{"grant_id":"y"}]}"#,
    )
    .unwrap();
    std::fs::write(home.path().join(RETIRED_APPROVALS_FILE), b"garbled").unwrap();
    let report = retired_authority(home.path());
    assert!(report.found());
    assert_eq!(
        report,
        RetiredAuthority {
            grants_file: true,
            grants: Some(2),
            approvals_file: true,
            approvals: None,
        }
    );
    // The ledger is unaffected by the retired files.
    let ledger = PermissionLedger::new(home.path());
    assert!(ledger.all_rules().unwrap().is_empty());
}

#[test]
fn hs2_xhq1eh_a_decision_is_recorded_once_per_operation() {
    let home = tempfile::tempdir().unwrap();
    let ledger = PermissionLedger::new(home.path());
    let record = |note: u128| DecisionAsked {
        decision_id: ulid::Ulid::from(note + 100).to_string(),
        target: target(1),
        kind: "readiness_override".into(),
        operation_id: "dispatch-1".into(),
        connection_id: "git-main".into(),
        native_id: ulid::Ulid::from(42u128).to_string(),
        note_id: ulid::Ulid::from(note).to_string(),
        actor_id: "scheduler".into(),
        asked_at_ms: 1,
        answered: None,
    };
    assert!(
        ledger
            .decision(&target(1), "readiness_override", "dispatch-1")
            .unwrap()
            .is_none()
    );
    let first = ledger.record_decision(record(7)).unwrap();
    // A repeat, even with a different note, returns the first record.
    assert_eq!(ledger.record_decision(record(8)).unwrap(), first);
    assert_eq!(
        ledger
            .decision(&target(1), "readiness_override", "dispatch-1")
            .unwrap(),
        Some(first)
    );
    assert!(
        ledger
            .decision(&target(1), "readiness_override", "dispatch-2")
            .unwrap()
            .is_none()
    );
    let mut bad = record(9);
    bad.kind = "not a code".into();
    assert!(ledger.record_decision(bad).is_err());
}

/// HS2-0RAZ1J: recording the response that decides a judgment call. Transition
/// matrix over (current answer, incoming answer), then repeated, interleaved, unknown
/// and old-file sequences.
#[test]
fn hs2_0raz1j_a_decision_records_the_response_that_decided_it() {
    let home = tempfile::tempdir().unwrap();
    let ledger = PermissionLedger::new(home.path());
    let asked = |operation: &str, note: u128, owner: u128| DecisionAsked {
        decision_id: ulid::Ulid::from(note + 1_000).to_string(),
        target: TargetRef {
            owner_checkout_id: format!("launch-{owner}"),
            ..target(1)
        },
        kind: "readiness_override".into(),
        operation_id: operation.into(),
        connection_id: "git-main".into(),
        native_id: ulid::Ulid::from(42u128).to_string(),
        note_id: ulid::Ulid::from(note).to_string(),
        actor_id: "scheduler".into(),
        asked_at_ms: 1,
        answered: None,
    };
    let answer = |response: u128, option: u32, at_ms: u64| DecisionAnswered {
        response_note_id: ulid::Ulid::from(response).to_string(),
        option,
        at_ms,
    };
    // Empty ledger: nothing to answer, nothing listed.
    assert_eq!(
        ledger
            .answer_decision(&ulid::Ulid::from(1_007u128).to_string(), answer(20, 0, 5))
            .unwrap(),
        None
    );
    assert!(ledger.decisions("launch-1").unwrap().is_empty());
    let first = ledger.record_decision(asked("dispatch-1", 7, 1)).unwrap();
    let other = ledger.record_decision(asked("dispatch-2", 8, 1)).unwrap();
    let elsewhere = ledger.record_decision(asked("dispatch-1", 9, 2)).unwrap();
    // (current, incoming) -> changed?
    let table: [(Option<DecisionAnswered>, DecisionAnswered, bool); 5] = [
        (None, answer(20, 0, 5), true),
        (Some(answer(20, 0, 5)), answer(20, 0, 9), false),
        (Some(answer(20, 0, 5)), answer(21, 0, 9), true),
        (Some(answer(21, 0, 9)), answer(21, 1, 10), true),
        (Some(answer(21, 1, 10)), answer(22, 1, 11), true),
    ];
    for (current, incoming, changed) in table {
        assert_eq!(
            ledger
                .decisions("launch-1")
                .unwrap()
                .into_iter()
                .find(|decision| decision.decision_id == first.decision_id)
                .unwrap()
                .answered,
            current
        );
        let result = ledger
            .answer_decision(&first.decision_id, incoming.clone())
            .unwrap();
        assert_eq!(result.is_some(), changed, "{incoming:?}");
        if let Some(record) = result {
            assert_eq!(record.answered, Some(incoming));
        }
    }
    // Interleaved: answering one decision leaves the others waiting, per owner.
    let listed = ledger.decisions("launch-1").unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|decision| decision.decision_id.as_str())
            .collect::<Vec<_>>(),
        [first.decision_id.as_str(), other.decision_id.as_str()]
    );
    assert!(listed[1].answered.is_none());
    assert_eq!(
        ledger.decisions("launch-2").unwrap(),
        std::slice::from_ref(&elsewhere)
    );
    // The record of the decision asked is kept; asking again returns it, answered.
    let again = ledger.record_decision(asked("dispatch-1", 10, 1)).unwrap();
    assert_eq!(again.note_id, first.note_id);
    assert_eq!(again.answered, Some(answer(22, 1, 11)));
    // An invalid response id is refused before anything is written.
    let mut bad = answer(23, 0, 12);
    bad.response_note_id = "not a token".into();
    assert!(ledger.answer_decision(&other.decision_id, bad).is_err());
    assert!(ledger.decisions("launch-1").unwrap()[1].answered.is_none());
    // A file written before HS2-0RAZ1J, without `answered`, still reads.
    let path = home.path().join("orchestration-permissions.json");
    let mut stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for decision in stored["decisions"].as_array_mut().unwrap() {
        decision.as_object_mut().unwrap().remove("answered");
    }
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    assert!(
        ledger
            .decisions("launch-1")
            .unwrap()
            .iter()
            .all(|decision| decision.answered.is_none())
    );
    // Empty then refill: the old decision is answered again from nothing.
    assert!(
        ledger
            .answer_decision(&first.decision_id, answer(24, 0, 13))
            .unwrap()
            .is_some()
    );
}

#[test]
fn hs2_xhq1eh_a_person_remembers_a_rule_ahead_and_each_time_requests_ignore_rules() {
    let mut f = Fixture::new();
    let codex = key(1, PermissionAction::SessionStart, Some("codex"));
    let deploy = key(1, PermissionAction::DeploymentsRun, None);
    let waiting = pending_id(f.ask(&ask(&codex, "start-1")));
    let ledger = f.ledger.clone();
    let (rule, created, allowed) = ledger
        .remember(codex.clone(), Some("maintainer".into()), &mut f.tx())
        .unwrap();
    assert!(created);
    assert_eq!(allowed, std::slice::from_ref(&waiting));
    assert_eq!(rule.request_id, None);
    // Remembering the same key again is a no-op returning the same rule.
    let (again, created, allowed) = ledger.remember(codex.clone(), None, &mut f.tx()).unwrap();
    assert_eq!((again, created, allowed.len()), (rule.clone(), false, 0));
    assert_eq!(f.ask(&ask(&codex, "start-2")), Asked::Remembered(rule));
    // A production deployment asks for each operation whatever is remembered, and a
    // rule never answers its request, now or by cascade.
    let (deploy_rule, ..) = ledger.remember(deploy.clone(), None, &mut f.tx()).unwrap();
    let each = PermissionAsk {
        each_time: true,
        ..ask(&deploy, "deploy-1")
    };
    let id = pending_id(f.ask(&each));
    assert!(matches!(
        f.answer(&id, PermissionAnswer::AlwaysAllow),
        Err(OrchestrationError::Invalid(_))
    ));
    assert!(f.ledger.forget(OWNER, &deploy_rule.rule_id).is_ok());
    let (_, _, allowed) = ledger.remember(deploy, None, &mut f.tx()).unwrap();
    assert!(
        allowed.is_empty(),
        "a rule never allows an each-time request"
    );
    f.answer(&id, PermissionAnswer::AllowOnce).unwrap();
    assert!(matches!(f.ask(&each), Asked::AllowedOnce(_)));
    // The bridge-facing record says so.
    let now = f.clock.0.get();
    assert!(f.ledger.get(OWNER, &id, now).unwrap().each_time);
}
