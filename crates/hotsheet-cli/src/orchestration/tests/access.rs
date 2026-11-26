//! HS2-XHQ1EH (HS2-SBQ3T8 before it): the authorization gate, permission requests,
//! remembered rules and the audit journal through the shared runtime, for local and
//! remote destinations. Synthetic identities only.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, DenialReason, PermissionAction, PermissionAnswer, PermissionState, PolicyBody,
    SessionLoad, SessionRequest, SetPolicy, TargetAction, TargetPolicy, ToolPolicy, ToolScope,
};
use serde_json::{Value, json};

use super::*;

fn human() -> MutationActor {
    MutationActor::parse("human", Some("maintainer".into())).unwrap()
}
fn ai(id: &str) -> MutationActor {
    MutationActor::parse("ai", Some(id.into())).unwrap()
}
fn system(id: &str) -> MutationActor {
    MutationActor::parse("system", Some(id.into())).unwrap()
}
fn act(
    action: TargetAction,
    session: Option<SessionRequest>,
    operation_id: Option<String>,
) -> ActionRequest {
    ActionRequest {
        // HS2-YCJDEB: a session start is only checked against caps with a known load.
        load: session.as_ref().map(|_| SessionLoad::default()),
        session,
        operation_id,
        ..ActionRequest::new(action)
    }
}
fn op(id: &str) -> Option<String> {
    Some(id.into())
}
fn bind_local(f: &Fixture, tx: &mut Transaction<'_>) -> TargetDestination {
    let revision = declaration(f, tx);
    let projection = f.runtime.checkout_projection(&f.target.id).unwrap();
    let destination = TargetDestination::Local {
        server_id: projection.server_id,
        checkout_id: projection.checkout_id,
        checkout_authority: projection.checkout_authority,
    };
    assert!(
        f.runtime
            .bind(request(f, revision, destination.clone()), tx)
            .unwrap()
            .usable
    );
    destination
}
fn denied(result: Result<Authorized, OrchestrationError>) -> DenialReason {
    match result {
        Err(OrchestrationError::Denied(reason)) => reason,
        Err(other) => panic!("expected a denial, got {other}"),
        Ok(_) => panic!("expected a denial, got authority"),
    }
}
fn outcomes(f: &Fixture) -> Vec<(String, AuditOutcome, Option<String>)> {
    f.runtime
        .audit(&f.owner.id, 0, 500)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| {
            (
                entry.record.action,
                entry.record.outcome,
                entry.record.reason,
            )
        })
        .collect()
}
fn codex_policy() -> PolicyBody {
    PolicyBody {
        tools: vec![ToolPolicy {
            tool: "codex".into(),
            max_sessions: Some(2),
            weight: Some(50),
            accounts: Vec::new(),
        }],
        targets: vec![TargetPolicy {
            target_id: id(1),
            enabled: true,
            actions: None,
            tools: vec![ToolScope {
                tool: "codex".into(),
                models: Vec::new(),
            }],
            max_sessions: Some(2),
            machine: None,
            speculation: None,
        }],
        ..PolicyBody::default()
    }
}
fn start() -> SessionRequest {
    SessionRequest {
        tool: "codex".into(),
        model: None,
        running: 0,
        starting: true,
    }
}
fn stop() -> SessionRequest {
    SessionRequest {
        starting: false,
        ..start()
    }
}

/// Records what the gate shows and settles, as the server's bridge prompt would.
#[derive(Default)]
struct Shown {
    raised: Mutex<Vec<String>>,
    settled: Mutex<Vec<(String, String)>>,
}
impl PermissionPrompt for Shown {
    fn raise(&self, request: &PromptRequest) {
        self.raised
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(format!(
                "{}|{}",
                request.request.request_id, request.summary
            ));
    }
    fn settle(&self, request_id: &str, decision: &str) {
        self.settled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((request_id.into(), decision.into()));
    }
}

#[test]
fn hs2_xhq1eh_consent_reads_freely_and_acting_asks_a_person_once_per_operation() {
    let mut f = fixture();
    let shown = Arc::new(Shown::default());
    f.runtime = f.runtime.clone().with_permission_prompt(shown.clone());
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let target = reference(&f);
    // Declared but unbound: nothing is consented to yet, and nothing is asked.
    declaration(&f, &mut tx);
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, op("op-1")),
            &mut tx
        )),
        DenialReason::TargetUnavailable
    );
    std::fs::remove_file(Path::new(&f.owner.root).join(".hotsheet2/orchestration.json")).unwrap();
    bind_local(&f, &mut tx);
    // Adding and binding the project is the consent to read and follow it.
    for action in [TargetAction::TicketsRead, TargetAction::GatesObserve] {
        let authorized = f
            .runtime
            .authorize(
                &target,
                Some(&ai("worker-1")),
                act(action, None, None),
                &mut tx,
            )
            .unwrap();
        assert_eq!(authorized.basis, "observation");
        assert!(authorized.resolution.usable);
    }
    // Acting asks a person: one request per operation, however often it is asked,
    // and nothing waits for the answer.
    let first = permission_request(f.runtime.authorize(
        &target,
        Some(&ai("worker-1")),
        act(TargetAction::TicketsWrite, None, op("op-2")),
        &mut tx,
    ));
    let again = permission_request(f.runtime.authorize(
        &target,
        Some(&system("scheduler")),
        act(TargetAction::TicketsWrite, None, op("op-2")),
        &mut tx,
    ));
    assert_eq!(first, again);
    // Shown on each ask (the bridge keeps one card per request id), with the words.
    let raised = shown.raised.lock().unwrap().clone();
    assert_eq!(raised.len(), 2);
    assert_eq!(
        raised[0],
        format!("{first}|Create or change tickets in Synthetic target")
    );
    // An acting request without an operation id cannot be asked about.
    assert!(matches!(
        f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, None),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    // A person's own act needs no further approval.
    let person = f
        .runtime
        .authorize(
            &target,
            Some(&human()),
            act(TargetAction::TicketsWrite, None, op("op-3")),
            &mut tx,
        )
        .unwrap();
    assert_eq!(person.basis, "person");
    // The governed AI cannot answer for itself.
    assert!(matches!(
        f.runtime.permission_answer(
            &f.owner.id,
            &first,
            PermissionAnswer::AllowOnce,
            Some(&ai("worker-1")),
            &mut tx
        ),
        Err(OrchestrationError::Forbidden(_))
    ));
    answer(
        &f.runtime,
        &f.owner.id,
        &first,
        PermissionAnswer::AllowOnce,
        &mut tx,
    );
    assert_eq!(
        shown.settled.lock().unwrap().clone(),
        [(first.clone(), "allow:once".to_owned())]
    );
    // HS2-8NJVXC / HS2-RTX8RJ: Allow Once is used only when the action happened.
    let allowed = f
        .runtime
        .authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, op("op-2")),
            &mut tx,
        )
        .unwrap();
    assert_eq!(allowed.basis, "allowed_once");
    assert_eq!(allowed.permission_id.as_deref(), Some(first.as_str()));
    f.runtime
        .record_outcome(&target, &allowed, false, Some("target_conflict"), &mut tx)
        .unwrap();
    let retried = f
        .runtime
        .authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, op("op-2")),
            &mut tx,
        )
        .unwrap();
    f.runtime
        .record_outcome(&target, &retried, true, None, &mut tx)
        .unwrap();
    // Used up: the same operation asks again.
    let next = permission_request(f.runtime.authorize(
        &target,
        Some(&ai("worker-1")),
        act(TargetAction::TicketsWrite, None, op("op-2")),
        &mut tx,
    ));
    assert_ne!(next, first);
    // Deny is final for its operation.
    answer(
        &f.runtime,
        &f.owner.id,
        &next,
        PermissionAnswer::Deny,
        &mut tx,
    );
    for _ in 0..2 {
        assert_eq!(
            denied(f.runtime.authorize(
                &target,
                Some(&ai("worker-1")),
                act(TargetAction::TicketsWrite, None, op("op-2")),
                &mut tx
            )),
            DenialReason::PermissionDenied
        );
    }
    // An unidentified caller is an invalid request, not an anonymous allowance.
    for actor in [None, Some(MutationActor::parse("ai", None).unwrap())] {
        assert!(matches!(
            f.runtime.authorize(
                &target,
                actor.as_ref(),
                act(TargetAction::TicketsRead, None, None),
                &mut tx
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    // Lost access: nothing survives an unusable target, not even a read.
    f.runtime
        .providers
        .set_disabled("synthetic-source", true)
        .unwrap();
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsRead, None, None),
            &mut tx
        )),
        DenialReason::TargetUnavailable
    );
    f.runtime
        .providers
        .set_disabled("synthetic-source", false)
        .unwrap();
    let log = outcomes(&f);
    let expected: Vec<(&str, AuditOutcome, Option<&str>)> = vec![
        (
            "tickets_write",
            AuditOutcome::Denied,
            Some("target_unavailable"),
        ),
        ("tickets_read", AuditOutcome::Allowed, Some("observation")),
        ("gates_observe", AuditOutcome::Allowed, Some("observation")),
        (
            "tickets_write",
            AuditOutcome::Denied,
            Some("permission_required"),
        ),
        (
            "tickets_write",
            AuditOutcome::Denied,
            Some("permission_required"),
        ),
        ("tickets_write", AuditOutcome::Allowed, Some("person")),
        (
            "permission_answer",
            AuditOutcome::Approved,
            Some("tickets_write"),
        ),
        ("tickets_write", AuditOutcome::Allowed, Some("allowed_once")),
        (
            "tickets_write",
            AuditOutcome::Failed,
            Some("target_conflict"),
        ),
        ("tickets_write", AuditOutcome::Allowed, Some("allowed_once")),
        ("tickets_write", AuditOutcome::Succeeded, None),
        (
            "tickets_write",
            AuditOutcome::Denied,
            Some("permission_required"),
        ),
        (
            "permission_answer",
            AuditOutcome::Rejected,
            Some("tickets_write"),
        ),
        (
            "tickets_write",
            AuditOutcome::Denied,
            Some("permission_denied"),
        ),
        (
            "tickets_write",
            AuditOutcome::Denied,
            Some("permission_denied"),
        ),
        (
            "tickets_read",
            AuditOutcome::Denied,
            Some("target_unavailable"),
        ),
    ];
    assert_eq!(
        log.iter()
            .map(|(action, outcome, reason)| (action.as_str(), *outcome, reason.as_deref()))
            .collect::<Vec<_>>(),
        expected
    );
    // A local target never needed credentials or the remote transport.
    assert_eq!(f.peer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.secrets.gets.load(Ordering::SeqCst), 0);
}

#[test]
fn hs2_xhq1eh_remembered_rules_policy_limits_and_reducing_activity_on_a_remote_target() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let target = reference(&f);
    let revision = declaration(&f, &mut tx);
    let remote = register(&f);
    assert!(
        f.runtime
            .bind(request(&f, revision, remote), &mut tx)
            .unwrap()
            .usable
    );
    // HS2-YCJDEB: without a policy no session starts, and no person is asked about it.
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&system("scheduler")),
            act(TargetAction::SessionsManage, Some(start()), op("start-0")),
            &mut tx
        )),
        DenialReason::PolicyDenied
    );
    assert!(
        f.runtime
            .permission_list(&f.owner.id, &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
    f.runtime
        .set_policy(
            &f.owner.id,
            SetPolicy {
                expected_revision: None,
                policy: codex_policy(),
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    // Always Allow: the waiting start is allowed and the key stops asking.
    let waiting = permission_request(f.runtime.authorize(
        &target,
        Some(&system("scheduler")),
        act(TargetAction::SessionsManage, Some(start()), op("start-1")),
        &mut tx,
    ));
    answer(
        &f.runtime,
        &f.owner.id,
        &waiting,
        PermissionAnswer::AlwaysAllow,
        &mut tx,
    );
    let calls = f.peer.calls.load(Ordering::SeqCst);
    let remembered = f
        .runtime
        .authorize(
            &target,
            Some(&system("scheduler")),
            act(TargetAction::SessionsManage, Some(start()), op("start-2")),
            &mut tx,
        )
        .unwrap();
    assert_eq!(remembered.basis, "remembered_rule");
    // Authority was confirmed against the peer, not a cached observation.
    assert!(f.peer.calls.load(Ordering::SeqCst) > calls);
    // A tool the policy does not list is refused before anyone is asked.
    let claude = SessionRequest {
        tool: "claude".into(),
        ..start()
    };
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&system("scheduler")),
            act(TargetAction::SessionsManage, Some(claude), op("start-3")),
            &mut tx
        )),
        DenialReason::PolicyDenied
    );
    // The dry check agrees without contacting the peer or reading a secret.
    let (calls, gets) = (
        f.peer.calls.load(Ordering::SeqCst),
        f.secrets.gets.load(Ordering::SeqCst),
    );
    let prefix = format!(
        "/checkouts/{}/orchestration/targets/{}/access",
        f.owner.id,
        id(1)
    );
    let decision = f
        .runtime
        .request(
            "POST",
            &prefix,
            &json!({"actor_id":"scheduler","action":"sessions_manage",
                "session":{"tool":"codex","starting":true},
                "load":{"target_sessions":0,"machine_sessions":0,"tool_sessions":0}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(decision["allowed"], true);
    assert_eq!(decision["basis"], "remembered_rule");
    let decision = f
        .runtime
        .request(
            "POST",
            &prefix,
            &json!({"actor_id":"scheduler","action":"work_start","operation_id":"op-9"}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        decision,
        json!({"allowed":false,"reason":"permission_required"})
    );
    // The check filed nothing.
    assert_eq!(
        f.runtime
            .permission_list(&f.owner.id, &mut tx)
            .unwrap()
            .requests
            .len(),
        1
    );
    assert_eq!(f.peer.calls.load(Ordering::SeqCst), calls);
    assert_eq!(f.secrets.gets.load(Ordering::SeqCst), gets);
    // HS2-48H57C: stopping only reduces activity. A policy that disables the target
    // does not deny it, and it never asks.
    let mut disabled = codex_policy();
    disabled.targets[0].enabled = false;
    let stored = f.runtime.policy(&f.owner.id).unwrap().revision;
    f.runtime
        .set_policy(
            &f.owner.id,
            SetPolicy {
                expected_revision: stored,
                policy: disabled,
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let stopped = f
        .runtime
        .authorize(
            &target,
            Some(&system("scheduler")),
            act(TargetAction::SessionsManage, Some(stop()), op("stop-1")),
            &mut tx,
        )
        .unwrap();
    assert_eq!(stopped.basis, "reduces_activity");
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&system("scheduler")),
            act(TargetAction::SessionsManage, Some(start()), op("start-4")),
            &mut tx
        )),
        DenialReason::PolicyDenied
    );
    // Lost remote credentials: fail closed, remembered rule or not.
    let saved = std::mem::take(
        &mut *f
            .secrets
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&system("scheduler")),
            act(TargetAction::SessionsManage, Some(stop()), op("stop-2")),
            &mut tx
        )),
        DenialReason::TargetUnavailable
    );
    *f.secrets
        .values
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = saved;
    // Forgetting the rule withdraws it: the key asks again.
    let rules = f
        .runtime
        .permission_list(&f.owner.id, &mut tx)
        .unwrap()
        .rules;
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0].summary,
        "Start a codex session in Synthetic target"
    );
    f.runtime
        .permission_forget(
            &f.owner.id,
            &rules[0].rule.rule_id,
            Some(&ai("worker-1")),
            &mut tx,
        )
        .unwrap();
    let stored = f.runtime.policy(&f.owner.id).unwrap().revision;
    f.runtime
        .set_policy(
            &f.owner.id,
            SetPolicy {
                expected_revision: stored,
                policy: codex_policy(),
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    permission_request(f.runtime.authorize(
        &target,
        Some(&system("scheduler")),
        act(TargetAction::SessionsManage, Some(start()), op("start-5")),
        &mut tx,
    ));
}

#[test]
fn hs2_xhq1eh_routes_carry_the_actor_and_forgetting_works_for_an_orphaned_owner() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_local(&f, &mut tx);
    let base = format!("/checkouts/{}/orchestration", f.owner.id);
    let body = json!({"target_id":id(1),"action":"work_start"});
    let mut by_ai = body.clone();
    by_ai["actor"] = json!({"role":"ai","id":"worker-1"});
    let refused = f
        .runtime
        .request("POST", &format!("{base}/permission-rules"), &by_ai, &mut tx)
        .unwrap_err();
    assert_eq!(refused.code(), "forbidden");
    assert_eq!(error_status(&refused), 403);
    for invalid in [json!({"role":"robot"}), json!({"role":"human","id":7})] {
        let mut bad = body.clone();
        bad["actor"] = invalid;
        assert!(matches!(
            f.runtime
                .request("POST", &format!("{base}/permission-rules"), &bad, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    // A rule for an undeclared target is refused; a tool on a non-session key too.
    let mut undeclared = body.clone();
    undeclared["target_id"] = json!(id(2));
    assert!(matches!(
        f.runtime.request(
            "POST",
            &format!("{base}/permission-rules"),
            &undeclared,
            &mut tx
        ),
        Err(OrchestrationError::NotFound)
    ));
    let mut tooled = body.clone();
    tooled["tool"] = json!("codex");
    assert!(matches!(
        f.runtime.request(
            "POST",
            &format!("{base}/permission-rules"),
            &tooled,
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    let mut by_human = body.clone();
    by_human["actor"] = json!({"role":"human","id":"maintainer"});
    let created = f
        .runtime
        .request(
            "POST",
            &format!("{base}/permission-rules"),
            &by_human,
            &mut tx,
        )
        .unwrap();
    assert_eq!(created["created"], true);
    assert_eq!(created["created_by"], "maintainer");
    assert_eq!(
        created["summary"],
        "Start work on a ticket in Synthetic target"
    );
    // A repeat is a no-op and is not audited twice. An unspecified actor is not
    // assumed to be an AI.
    let retried = f
        .runtime
        .request("POST", &format!("{base}/permission-rules"), &body, &mut tx)
        .unwrap();
    assert_eq!(retried["rule_id"], created["rule_id"]);
    assert_eq!(retried["created"], false);
    // A waiting request is answered through its route.
    let waiting = permission_request(f.runtime.authorize(
        &reference(&f),
        Some(&ai("worker-1")),
        act(TargetAction::ValidationRun, None, op("validate-1")),
        &mut tx,
    ));
    let listed = f
        .runtime
        .request("GET", &format!("{base}/permissions"), &Value::Null, &mut tx)
        .unwrap();
    assert_eq!(listed["requests"][0]["request_id"], waiting.as_str());
    assert_eq!(listed["requests"][0]["state"], "pending");
    assert_eq!(listed["requests"][0]["action"], "validation_run");
    assert_eq!(listed["requests"][0]["target_label"], "Synthetic target");
    assert_eq!(listed["rules"].as_array().unwrap().len(), 1);
    assert_eq!(listed["retired"]["grants_file"], false);
    let refused = f
        .runtime
        .request(
            "POST",
            &format!("{base}/permissions/{waiting}"),
            &json!({"answer":"allow_once","actor":{"role":"ai","id":"worker-1"}}),
            &mut tx,
        )
        .unwrap_err();
    assert_eq!(refused.code(), "forbidden");
    assert!(matches!(
        f.runtime.request(
            "POST",
            &format!("{base}/permissions/{waiting}"),
            &json!({"answer":"sometimes"}),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    let answered = f
        .runtime
        .request(
            "POST",
            &format!("{base}/permissions/{waiting}"),
            &json!({"answer":"deny","actor":{"role":"human","id":"maintainer"}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(answered["request"]["state"], "denied");
    assert_eq!(answered["changed"], true);
    // Answering differently later is a conflict.
    assert!(matches!(
        f.runtime.request(
            "POST",
            &format!("{base}/permissions/{waiting}"),
            &json!({"answer":"allow_once"}),
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
    let audit = format!("{base}/audit");
    let page = f
        .runtime
        .request(
            "GET",
            &format!("{audit}?after=0&limit=1"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["entries"][0]["action"], "permission_remember");
    assert_eq!(page["entries"][0]["actor_role"], "human");
    assert_eq!(page["more"], true);
    for bad in ["?after=x", "?limit=-1", "?after=1&after=2"] {
        assert!(matches!(
            f.runtime
                .request("GET", &format!("{audit}{bad}"), &Value::Null, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    let rule = created["rule_id"].as_str().unwrap().to_owned();
    for path in [
        format!("{base}/permissions"),
        format!("{base}/permissions/{waiting}"),
        format!("{base}/permission-rules"),
        format!("{base}/permission-rules/{rule}"),
        format!("{base}/targets/{}/access", id(1)),
        audit.clone(),
        format!("{audit}?after=3"),
    ] {
        assert!(is_path(&path), "{path}");
        assert!(is_machine_path(&path), "{path}");
        assert!(!is_cleanup_request("DELETE", &path), "{path}");
    }
    // The retired grant and approval routes are gone.
    for path in [
        format!("{base}/targets/{}/grants", id(1)),
        format!("{base}/approvals"),
    ] {
        assert!(!is_machine_path(&path), "{path}");
    }
    // The owner's registration, manifest and provider configuration are gone; a rule
    // can still be forgotten and the journal still read.
    std::fs::remove_file(f.runtime.checkouts.path()).unwrap();
    std::fs::write(
        Path::new(&f.owner.root).join(".hotsheet2/orchestration.json"),
        b"corrupt",
    )
    .unwrap();
    std::fs::write(f.runtime.providers.path(), b"corrupt").unwrap();
    // Forgetting needs an identified actor.
    assert!(matches!(
        f.runtime.request(
            "DELETE",
            &format!("{base}/permission-rules/{rule}"),
            &json!({}),
            &mut tx,
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    let forgotten = f
        .runtime
        .request(
            "DELETE",
            &format!("{base}/permission-rules/{rule}"),
            &json!({"actor":{"role":"human","id":"maintainer"}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(forgotten["rule_id"], rule.as_str());
    assert!(matches!(
        f.runtime.request(
            "DELETE",
            &format!("{base}/permission-rules/{rule}"),
            &json!({"actor":{"role":"ai","id":"worker-1"}}),
            &mut tx,
        ),
        Err(OrchestrationError::NotFound)
    ));
    let page = f
        .runtime
        .request("GET", &audit, &Value::Null, &mut tx)
        .unwrap();
    let actions: Vec<_> = page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["action"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        actions,
        [
            "permission_remember",
            "validation_run",
            "permission_answer",
            "permission_forget"
        ]
    );
}

#[test]
fn hs2_xhq1eh_retired_grants_are_reported_and_never_trusted() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_local(&f, &mut tx);
    // A grant file from before this model, naming this exact actor and action.
    std::fs::write(
        f.home.join("orchestration-grants.json"),
        serde_json::to_vec(&json!({"schema_version":1,"grants":[{
            "grant_id": id(70), "target": reference(&f), "actor_id": "worker-1",
            "actions": ["work_start"], "destination_key": "0".repeat(64),
            "created_at_ms": 1, "expires_at_ms": u64::MAX }]}))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        f.home.join("orchestration-approvals.json"),
        br#"{"schema_version":1,"approvals":[]}"#,
    )
    .unwrap();
    // It is not authority: the actor still asks.
    permission_request(f.runtime.authorize(
        &reference(&f),
        Some(&ai("worker-1")),
        act(TargetAction::WorkStart, None, op("op-1")),
        &mut tx,
    ));
    let listed = f.runtime.permission_list(&f.owner.id, &mut tx).unwrap();
    assert!(listed.retired.found());
    assert_eq!(listed.retired.grants, Some(1));
    assert_eq!(listed.retired.approvals, Some(0));
    let validation = f
        .runtime
        .request(
            "POST",
            &format!("/checkouts/{}/orchestration/policy/validate", f.owner.id),
            &json!({"policy":{}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(validation["retired"]["grants_file"], true);
    // The files are left alone.
    assert!(f.home.join("orchestration-grants.json").exists());
    let request = &listed.requests[0].request;
    assert_eq!(request.key.action, PermissionAction::WorkStart);
    assert_eq!(request.state, PermissionState::Pending);
}

/// HS2-JWV5S1: a person removes the retired grant and approval files after seeing
/// their counts. An AI or system actor is refused and nothing is removed; removing
/// again finds nothing and audits nothing; files written back later are found and
/// removed again (empty, then refilled). The files never become authority.
#[test]
fn hs2_jwv5s1_a_person_removes_the_retired_files_once() {
    let f = fixture();
    let mut rng = Rng(500);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_local(&f, &mut tx);
    let path = format!(
        "/checkouts/{}/orchestration/retired-permissions",
        f.owner.id
    );
    let write_retired = || {
        std::fs::write(
            f.home.join("orchestration-grants.json"),
            br#"{"schema_version":1,"grants":[{},{}]}"#,
        )
        .unwrap();
        std::fs::write(f.home.join("orchestration-approvals.json"), b"not json").unwrap();
    };
    // Nothing to remove: an empty report, nothing audited.
    let empty = f
        .runtime
        .retired_permissions_remove(&f.owner.id, Some(&human()), &mut tx)
        .unwrap();
    assert!(!empty.removed.found() && !empty.remaining.found());
    write_retired();
    // Shown with their counts; an unreadable file is found without a count.
    let shown = f
        .runtime
        .request("GET", &path, &Value::Null, &mut tx)
        .unwrap();
    assert_eq!(
        shown,
        json!({"grants_file": true, "grants": 2, "approvals_file": true})
    );
    for actor in [ai("worker-1"), system("scheduler")] {
        assert!(matches!(
            f.runtime
                .retired_permissions_remove(&f.owner.id, Some(&actor), &mut tx),
            Err(OrchestrationError::Forbidden(_))
        ));
    }
    assert!(matches!(
        f.runtime
            .retired_permissions_remove(&f.owner.id, None, &mut tx),
        Err(OrchestrationError::Forbidden(_))
    ));
    assert!(f.home.join("orchestration-grants.json").exists());
    // A person removes them through the router, with the counts removed.
    let removed = f
        .runtime
        .request(
            "DELETE",
            &path,
            &json!({"actor": {"role": "human", "id": "maintainer"}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(removed["removed"]["grants"], 2);
    assert_eq!(removed["removed"]["approvals_file"], true);
    assert_eq!(
        removed["remaining"],
        json!({"grants_file": false, "approvals_file": false})
    );
    assert!(!f.home.join("orchestration-grants.json").exists());
    assert!(!f.home.join("orchestration-approvals.json").exists());
    let audited = || {
        f.runtime
            .audit(&f.owner.id, 0, 500)
            .unwrap()
            .entries
            .into_iter()
            .filter(|entry| entry.record.action == "retired_remove")
            .map(|entry| (entry.record.outcome, entry.record.reason))
            .collect::<Vec<_>>()
    };
    let expected = vec![
        (AuditOutcome::Revoked, Some("grants_file".to_owned())),
        (AuditOutcome::Revoked, Some("approvals_file".to_owned())),
    ];
    assert_eq!(audited(), expected);
    // Repeated: nothing found, nothing audited.
    let again = f
        .runtime
        .retired_permissions_remove(&f.owner.id, Some(&human()), &mut tx)
        .unwrap();
    assert!(!again.removed.found());
    assert_eq!(audited().len(), 2);
    // Refilled by an old process: found and removed again.
    write_retired();
    assert!(
        f.runtime
            .permission_list(&f.owner.id, &mut tx)
            .unwrap()
            .retired
            .found()
    );
    let refilled = f
        .runtime
        .retired_permissions_remove(&f.owner.id, Some(&human()), &mut tx)
        .unwrap();
    assert_eq!(refilled.removed.grants, Some(2));
    assert_eq!(audited().len(), 4);
    // An unknown owner is refused before anything is touched.
    write_retired();
    assert!(
        f.runtime
            .retired_permissions_remove("unknown-owner", Some(&human()), &mut tx)
            .is_err()
    );
    assert!(f.home.join("orchestration-grants.json").exists());
}

/// The next `rounds` resolutions of the target are each interrupted by one complete
/// concurrent resolve, as a follower's would be.
fn race(f: &Fixture, rounds: usize) {
    fn arm(runtime: OrchestrationRuntime, peer: Arc<Peer>, target: TargetRef, left: usize) {
        if left == 0 {
            return;
        }
        let (again, hook_peer) = (runtime.clone(), peer.clone());
        *peer
            .before_metadata
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(move || {
            let mut rng = Rng(9_000 + left as u128);
            let mut tx = Transaction {
                clock: &Clock,
                rng: &mut rng,
            };
            assert!(again.resolve(&target, &mut tx).unwrap().usable);
            arm(again, hook_peer, target, left - 1);
        }));
    }
    arm(f.runtime.clone(), f.peer.clone(), reference(f), rounds);
}

/// HS2-5JBYPV: a follower and a request confirm authority at the same moment. The one
/// whose resolve loses the race reads again; it is not told the target is unavailable.
#[test]
fn hs2_5jbypv_authorize_reads_again_after_losing_a_race_to_a_concurrent_resolve() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let revision = declaration(&f, &mut tx);
    let destination = register(&f);
    assert!(
        f.runtime
            .bind(request(&f, revision, destination), &mut tx)
            .unwrap()
            .usable
    );
    // Without the retry this is the plain conflict a caller of resolve sees.
    race(&f, 1);
    assert!(matches!(
        f.runtime.resolve(&reference(&f), &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    // The gate reads again, once or as often as it is allowed to.
    let allowed = super::super::access::RESOLVE_ATTEMPTS;
    for rounds in [1, allowed] {
        race(&f, rounds);
        let authorized = f
            .runtime
            .authorize(
                &reference(&f),
                Some(&ai("worker-1")),
                act(TargetAction::GatesObserve, None, None),
                &mut tx,
            )
            .unwrap();
        assert!(authorized.resolution.usable);
    }
    // A race that never ends is still a denial, never an allowance.
    race(&f, allowed + 1);
    assert_eq!(
        denied(f.runtime.authorize(
            &reference(&f),
            Some(&ai("worker-1")),
            act(TargetAction::GatesObserve, None, None),
            &mut tx,
        )),
        DenialReason::TargetUnavailable
    );
}

/// HS2-E16FWJ: every change to the permission ledger writes the permission signal a
/// server on this machine waits on: a request filed, answered, remembered ahead and
/// forgotten. Asking again for a request already waiting, reading, and a repeated
/// answer change nothing and signal nothing. A waiter sees each change once.
#[test]
fn hs2_e16fwj_every_ledger_change_signals_the_servers_on_this_machine() {
    let f = fixture();
    let mut rng = Rng(400);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let target = reference(&f);
    bind_local(&f, &mut tx);
    let token = || f.runtime.permission_signal_now();
    assert_eq!(token(), None);
    let mut waiter = f.runtime.permission_signal_waiter().unwrap();
    assert!(!waiter.wait(std::time::Duration::ZERO));
    assert_eq!(
        f.runtime.permission_signal_dir(),
        f.home.join(PERMISSION_SIGNAL_DIRECTORY)
    );
    let ask = |operation: &str, tx: &mut Transaction<'_>| {
        permission_request(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, op(operation)),
            tx,
        ))
    };
    // Filing signals; asking again for the same operation does not.
    let first = ask("op-1", &mut tx);
    let filed = token().expect("a filed request signals");
    assert!(waiter.wait(std::time::Duration::ZERO));
    assert!(!waiter.wait(std::time::Duration::ZERO));
    assert_eq!(ask("op-1", &mut tx), first);
    f.runtime.permission_list(&f.owner.id, &mut tx).unwrap();
    assert_eq!(token().as_deref(), Some(filed.as_str()));
    assert!(!waiter.wait(std::time::Duration::ZERO));
    // Answering signals once; the same answer again changes nothing.
    answer(
        &f.runtime,
        &f.owner.id,
        &first,
        PermissionAnswer::Deny,
        &mut tx,
    );
    let answered = token().unwrap();
    assert_ne!(answered, filed);
    answer(
        &f.runtime,
        &f.owner.id,
        &first,
        PermissionAnswer::Deny,
        &mut tx,
    );
    assert_eq!(token().as_deref(), Some(answered.as_str()));
    // Interleaved: two operations filed, then a rule remembered ahead allows both.
    ask("op-2", &mut tx);
    ask("op-3", &mut tx);
    let before = token().unwrap();
    let remembered = f
        .runtime
        .permission_remember(
            &f.owner.id,
            RememberPermission {
                target_id: target.target_id.clone(),
                action: PermissionAction::TicketsWrite,
                tool: None,
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(remembered.also_allowed.len(), 2);
    let after_remember = token().unwrap();
    assert_ne!(after_remember, before);
    // Remembering the same key again is not a change.
    f.runtime
        .permission_remember(
            &f.owner.id,
            RememberPermission {
                target_id: target.target_id.clone(),
                action: PermissionAction::TicketsWrite,
                tool: None,
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(token().as_deref(), Some(after_remember.as_str()));
    // Forgetting signals; the waiter coalesces everything since its last wait.
    f.runtime
        .permission_forget(
            &f.owner.id,
            &remembered.rule.rule.rule_id,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_ne!(token().as_deref(), Some(after_remember.as_str()));
    assert!(waiter.wait(std::time::Duration::ZERO));
    assert!(!waiter.wait(std::time::Duration::from_millis(50)));
    // Empty then refill: a new operation after the rule is gone asks and signals.
    let refilled = ask("op-4", &mut tx);
    assert_ne!(refilled, first);
    assert!(waiter.wait(std::time::Duration::ZERO));
}

fn remember(f: &Fixture, target_id: &str, action: PermissionAction, tx: &mut Transaction<'_>) {
    f.runtime
        .permission_remember(
            &f.owner.id,
            RememberPermission {
                target_id: target_id.into(),
                action,
                tool: None,
            },
            Some(&human()),
            tx,
        )
        .unwrap();
}

fn listed_rules(f: &Fixture) -> Vec<(String, PermissionAction)> {
    f.runtime
        .permission_rules_everywhere()
        .into_iter()
        .map(|view| (view.rule.key.target.target_id, view.rule.key.action))
        .collect()
}

fn orphan_forgets(f: &Fixture) -> Vec<(Option<String>, Option<String>)> {
    f.runtime
        .audit(&f.owner.id, 0, 500)
        .unwrap()
        .entries
        .into_iter()
        .filter(|entry| entry.record.action == "permission_forget")
        .map(|entry| {
            assert_eq!(entry.record.outcome, AuditOutcome::Revoked);
            assert_eq!(entry.record.actor_id, None);
            (entry.record.target_id, entry.record.reason)
        })
        .collect()
}

fn declare_two(f: &Fixture, tx: &mut Transaction<'_>) {
    let revision = f.runtime.declarations(&f.owner.id).unwrap().revision;
    let target = |n: u128, label: &str| TargetDeclaration {
        target_id: id(n),
        state: DeclarationState::Active,
        label: label.into(),
        expected_repository_identity: None,
        site_of: None,
    };
    f.runtime
        .replace(
            &f.owner.id,
            ReplaceTargets {
                expected_revision: revision,
                targets: vec![target(1, "Synthetic target"), target(2, "Second target")],
            },
            tx,
        )
        .unwrap();
}

/// HS2-5675T5: removing a target forgets its remembered rules, audited, and Settings →
/// Permissions no longer lists them; the other target's rules keep answering. A
/// manifest that cannot be read forgets nothing; an owner checkout that is no longer
/// registered forgets its rules. Repeats change nothing.
#[test]
fn hs2_5675t5_removing_a_target_or_its_owner_forgets_only_its_rules() {
    let f = fixture();
    let mut rng = Rng(500);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_local(&f, &mut tx);
    declare_two(&f, &mut tx);
    remember(&f, &id(1), PermissionAction::WorkStart, &mut tx);
    remember(&f, &id(1), PermissionAction::TicketsWrite, &mut tx);
    remember(&f, &id(2), PermissionAction::WorkStart, &mut tx);
    // Every rule is live: listing forgets nothing, however often.
    for _ in 0..2 {
        assert_eq!(listed_rules(&f).len(), 3);
    }
    assert!(orphan_forgets(&f).is_empty());
    // Removing the second target forgets its one rule at once.
    let before = f.runtime.permission_signal_now();
    let removed = f
        .runtime
        .remove_project(
            &TargetRef {
                owner_checkout_id: f.owner.id.clone(),
                target_id: id(2),
            },
            RemoveProject::default(),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert!(removed.removed);
    assert_ne!(f.runtime.permission_signal_now(), before);
    assert_eq!(
        listed_rules(&f),
        vec![
            (id(1), PermissionAction::WorkStart),
            (id(1), PermissionAction::TicketsWrite),
        ]
    );
    assert_eq!(
        orphan_forgets(&f),
        vec![(Some(id(2)), Some("target_removed".to_owned()))]
    );
    // Repeating the removal, and listing again, forget nothing more.
    f.runtime
        .remove_project(
            &TargetRef {
                owner_checkout_id: f.owner.id.clone(),
                target_id: id(2),
            },
            RemoveProject::default(),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(listed_rules(&f).len(), 2);
    assert_eq!(orphan_forgets(&f).len(), 1);
    // The live target's rule still answers its action without asking.
    let allowed = f.runtime.authorize(
        &reference(&f),
        Some(&ai("worker-1")),
        act(TargetAction::TicketsWrite, None, op("write-1")),
        &mut tx,
    );
    assert!(allowed.is_ok(), "the live rule answers");
    // A manifest that cannot be read is no proof of removal: nothing is forgotten.
    let manifest = std::path::Path::new(&f.owner.root).join(".hotsheet2/orchestration.json");
    let kept = std::fs::read(&manifest).unwrap();
    std::fs::write(&manifest, b"not json").unwrap();
    assert_eq!(listed_rules(&f).len(), 2);
    std::fs::write(&manifest, &kept).unwrap();
    assert_eq!(listed_rules(&f).len(), 2);
    assert_eq!(orphan_forgets(&f).len(), 1);
    // The owner checkout is unregistered: its remaining rules go, audited in its
    // journal, and the listing is empty; a later listing changes nothing.
    hotsheet_ticketing::checkout_removal::remove_checkout(
        &f.runtime.providers,
        &f.runtime.checkouts,
        &f.owner.id,
        false,
    )
    .unwrap();
    assert!(listed_rules(&f).is_empty());
    assert!(listed_rules(&f).is_empty());
    assert_eq!(
        orphan_forgets(&f),
        vec![
            (Some(id(2)), Some("target_removed".to_owned())),
            (Some(id(1)), Some("owner_removed".to_owned())),
            (Some(id(1)), Some("owner_removed".to_owned())),
        ]
    );
}

/// HS2-5675T5: at the 1,024-rule cap, Always Allow and remember first forget the rules
/// of removed targets and then succeed; with every rule live they are still refused.
#[test]
fn hs2_5675t5_the_rule_cap_makes_room_from_removed_targets_only() {
    use hotsheet_ticketing::orchestration::MAX_PERMISSION_RULES;
    let f = fixture();
    let mut rng = Rng(600);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_local(&f, &mut tx);
    declare_two(&f, &mut tx);
    // The ledger is full: `second` rules for target 2, the rest for live target 1.
    let seed = |second: usize| {
        let rules: Vec<Value> = (0..MAX_PERMISSION_RULES as u128)
            .map(|index| {
                json!({
                    "rule_id": ulid::Ulid::from((1u128 << 102) + index).to_string(),
                    "target": {
                        "owner_checkout_id": f.owner.id,
                        "target_id": id(if (index as usize) < second { 2 } else { 1 }),
                    },
                    "action": "session_start",
                    "tool": format!("tool-{index}"),
                    "created_at_ms": 1_000,
                })
            })
            .collect();
        std::fs::write(
            f.home.join("orchestration-permissions.json"),
            serde_json::to_vec(&json!({"schema_version": 1, "requests": [], "rules": rules}))
                .unwrap(),
        )
        .unwrap();
    };
    let remember_work = |tx: &mut Transaction<'_>| {
        f.runtime.permission_remember(
            &f.owner.id,
            RememberPermission {
                target_id: id(1),
                action: PermissionAction::WorkStart,
                tool: None,
            },
            Some(&human()),
            tx,
        )
    };
    // Every rule live: still refused, nothing forgotten.
    seed(0);
    let refused = remember_work(&mut tx);
    assert!(
        matches!(refused, Err(OrchestrationError::Capacity)),
        "{refused:?}"
    );
    assert_eq!(listed_rules(&f).len(), MAX_PERMISSION_RULES);
    // Target 2's rules while it is declared are live too: still refused.
    seed(4);
    assert!(matches!(
        remember_work(&mut tx),
        Err(OrchestrationError::Capacity)
    ));
    // Target 2 removed by hand in the shared manifest (as another machine would):
    // the next remember forgets its rules and succeeds.
    let revision = f.runtime.declarations(&f.owner.id).unwrap().revision;
    f.runtime
        .replace(
            &f.owner.id,
            ReplaceTargets {
                expected_revision: revision,
                targets: vec![
                    TargetDeclaration {
                        target_id: id(1),
                        state: DeclarationState::Active,
                        label: "Synthetic target".into(),
                        expected_repository_identity: None,
                        site_of: None,
                    },
                    TargetDeclaration {
                        target_id: id(2),
                        state: DeclarationState::Removed,
                        label: "Second target".into(),
                        expected_repository_identity: None,
                        site_of: None,
                    },
                ],
            },
            &mut tx,
        )
        .unwrap();
    let remembered = remember_work(&mut tx).unwrap();
    assert!(remembered.created);
    let listed = listed_rules(&f);
    assert_eq!(listed.len(), MAX_PERMISSION_RULES - 3);
    assert!(listed.iter().all(|(target, _)| *target == id(1)));
    assert_eq!(listed.last(), Some(&(id(1), PermissionAction::WorkStart)));
    assert_eq!(
        orphan_forgets(&f),
        vec![(Some(id(2)), Some("target_removed".to_owned())); 4]
    );
}
