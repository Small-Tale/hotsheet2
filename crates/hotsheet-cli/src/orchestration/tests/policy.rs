//! HS2-YCJDEB: the throughput policy, approvals and dry-run through the shared
//! runtime, and their place in the authorization gate. Synthetic identities only.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    ApprovalGate, ApprovalPolicy, ApprovalState, AuditOutcome, CreateGrant, DenialReason,
    MachineLimits, PolicyBody, SessionLoad, SessionRequest, SessionScope, SetPolicy, TargetAction,
    TargetPolicy, ToolPolicy, ToolScope,
};
use serde_json::{Value, json};

use super::*;

fn human() -> MutationActor {
    MutationActor::parse("human", Some("maintainer".into())).unwrap()
}
fn worker() -> MutationActor {
    MutationActor::parse("ai", Some("worker-1".into())).unwrap()
}
fn codex() -> ToolScope {
    ToolScope {
        tool: "codex".into(),
        models: Vec::new(),
    }
}
fn policy(actions: &[TargetAction]) -> PolicyBody {
    let sessions = actions.contains(&TargetAction::SessionsManage);
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 2,
            ..MachineLimits::default()
        },
        tools: vec![ToolPolicy {
            tool: "codex".into(),
            max_sessions: 2,
            weight: 50,
            accounts: Vec::new(),
        }],
        targets: vec![TargetPolicy {
            target_id: id(1),
            enabled: true,
            actions: actions.to_vec(),
            tools: if sessions { vec![codex()] } else { Vec::new() },
            max_sessions: 1,
            machine: None,
            speculation: None,
        }],
        ..PolicyBody::default()
    }
}
fn bind_local(f: &Fixture, revision: String, tx: &mut Transaction<'_>) {
    let projection = f.runtime.checkout_projection(&f.target.id).unwrap();
    let destination = TargetDestination::Local {
        server_id: projection.server_id,
        checkout_id: projection.checkout_id,
        checkout_authority: projection.checkout_authority,
    };
    assert!(
        f.runtime
            .bind(request(f, revision, destination), tx)
            .unwrap()
            .usable
    );
}
fn grant(f: &Fixture, actions: &[TargetAction], tx: &mut Transaction<'_>) {
    let sessions = actions.contains(&TargetAction::SessionsManage);
    f.runtime
        .grant(
            &reference(f),
            CreateGrant {
                grant_id: None,
                actor_id: "worker-1".into(),
                actions: actions.to_vec(),
                session_scope: sessions.then(|| SessionScope {
                    tools: vec![codex()],
                    max_concurrency: 4,
                }),
                expires_at_ms: 10_000,
            },
            Some(&human()),
            tx,
        )
        .unwrap();
}
fn set(f: &Fixture, body: PolicyBody, tx: &mut Transaction<'_>) -> Option<String> {
    let expected_revision = f.runtime.policy(&f.owner.id).unwrap().revision;
    f.runtime
        .set_policy(
            &f.owner.id,
            SetPolicy {
                expected_revision,
                policy: body,
            },
            Some(&human()),
            tx,
        )
        .unwrap()
        .revision
}
fn refused(result: Result<Authorized, OrchestrationError>) -> DenialReason {
    match result {
        Err(OrchestrationError::Denied(reason)) => reason,
        Err(other) => panic!("expected a denial, got {other}"),
        Ok(_) => panic!("expected a denial, got authority"),
    }
}
fn last_audit(f: &Fixture) -> (String, AuditOutcome, Option<String>) {
    let entry = f
        .runtime
        .audit(&f.owner.id, 0, 500)
        .unwrap()
        .entries
        .pop()
        .unwrap();
    (
        entry.record.action,
        entry.record.outcome,
        entry.record.reason,
    )
}

#[test]
fn hs2_ycjdeb_policy_and_grant_must_both_allow_an_action() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let target = reference(&f);
    let revision = declaration(&f, &mut tx);
    bind_local(&f, revision, &mut tx);
    let write = || ActionRequest::new(TargetAction::TicketsWrite).operation("op-write");
    let start = |load: Option<SessionLoad>| ActionRequest {
        session: Some(SessionRequest {
            tool: "codex".into(),
            model: None,
            running: 0,
            starting: true,
        }),
        load,
        ..ActionRequest::new(TargetAction::SessionsManage).operation("op-start")
    };
    // No policy: an explicit ticket action is governed by its grant alone, while
    // session management is refused because nothing automatic runs without a policy.
    grant(
        &f,
        &[TargetAction::TicketsWrite, TargetAction::SessionsManage],
        &mut tx,
    );
    f.runtime
        .authorize(&target, Some(&worker()), write(), &mut tx)
        .unwrap();
    assert_eq!(
        refused(f.runtime.authorize(
            &target,
            Some(&worker()),
            start(Some(SessionLoad::default())),
            &mut tx
        )),
        DenialReason::PolicyDenied
    );
    assert_eq!(
        last_audit(&f),
        (
            "sessions_manage".into(),
            AuditOutcome::Denied,
            Some("policy_absent".into())
        )
    );
    // The policy allows work_start; no grant covers it. The grant decides, and no
    // approval request is filed for an action the actor was never authorized to take.
    let mut body = policy(&[
        TargetAction::TicketsWrite,
        TargetAction::WorkStart,
        TargetAction::SessionsManage,
    ]);
    body.approval = ApprovalPolicy {
        required_for: vec![ApprovalGate::WorkStart],
    };
    set(&f, body, &mut tx);
    assert_eq!(
        refused(f.runtime.authorize(
            &target,
            Some(&worker()),
            ActionRequest::new(TargetAction::WorkStart).operation("op-work"),
            &mut tx
        )),
        DenialReason::ActionNotGranted
    );
    assert!(
        f.runtime
            .approvals(&f.owner.id, &mut tx)
            .unwrap()
            .is_empty()
    );
    // Both allow.
    f.runtime
        .authorize(&target, Some(&worker()), write(), &mut tx)
        .unwrap();
    f.runtime
        .authorize(
            &target,
            Some(&worker()),
            start(Some(SessionLoad::default())),
            &mut tx,
        )
        .unwrap();
    // The grant allows four sessions; the policy caps the target at one.
    assert_eq!(
        refused(f.runtime.authorize(
            &target,
            Some(&worker()),
            start(Some(SessionLoad {
                target_sessions: 1,
                machine_sessions: 1,
                tool_sessions: 1,
                ..SessionLoad::default()
            })),
            &mut tx
        )),
        DenialReason::PolicyDenied
    );
    assert_eq!(last_audit(&f).2.as_deref(), Some("policy_target_cap"));
    assert_eq!(
        refused(
            f.runtime
                .authorize(&target, Some(&worker()), start(None), &mut tx)
        ),
        DenialReason::PolicyDenied
    );
    assert_eq!(last_audit(&f).2.as_deref(), Some("policy_load_unknown"));
    // The grant still allows tickets_write; the policy no longer lists it.
    set(&f, policy(&[TargetAction::TicketsRead]), &mut tx);
    assert_eq!(
        refused(
            f.runtime
                .authorize(&target, Some(&worker()), write(), &mut tx)
        ),
        DenialReason::PolicyDenied
    );
    assert_eq!(
        last_audit(&f).2.as_deref(),
        Some("policy_action_not_permitted")
    );
    // The dry check gives the same answer without auditing or contacting anything.
    let entries = f.runtime.audit(&f.owner.id, 0, 500).unwrap().entries.len();
    let decision = f
        .runtime
        .request(
            "POST",
            &format!(
                "/checkouts/{}/orchestration/targets/{}/access",
                f.owner.id,
                id(1)
            ),
            &json!({"actor_id":"worker-1","action":"tickets_write"}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        decision,
        json!({"allowed":false,"reason":"policy_denied","detail":"policy_action_not_permitted"})
    );
    assert_eq!(
        f.runtime.audit(&f.owner.id, 0, 500).unwrap().entries.len(),
        entries
    );
    // A corrupt policy is a denial, never a fall back to "no policy".
    std::fs::write(
        Path::new(&f.owner.root).join(".hotsheet2/orchestration-policy.json"),
        b"{broken",
    )
    .unwrap();
    assert_eq!(
        refused(
            f.runtime
                .authorize(&target, Some(&worker()), write(), &mut tx)
        ),
        DenialReason::PolicyDenied
    );
    assert_eq!(f.peer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.secrets.gets.load(Ordering::SeqCst), 0);
}

#[test]
fn hs2_ycjdeb_a_gated_decision_waits_for_one_persons_approval() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let target = reference(&f);
    let revision = declaration(&f, &mut tx);
    bind_local(&f, revision, &mut tx);
    grant(&f, &[TargetAction::WorkStart], &mut tx);
    let mut body = policy(&[TargetAction::WorkStart]);
    body.approval = ApprovalPolicy {
        required_for: vec![ApprovalGate::WorkStart],
    };
    set(&f, body, &mut tx);
    let work = |operation: &str| ActionRequest::new(TargetAction::WorkStart).operation(operation);
    // An approval is for one operation id; without one the request is malformed.
    assert!(matches!(
        f.runtime.authorize(
            &target,
            Some(&worker()),
            ActionRequest::new(TargetAction::WorkStart),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(
        refused(
            f.runtime
                .authorize(&target, Some(&worker()), work("op-1"), &mut tx)
        ),
        DenialReason::ApprovalRequired
    );
    assert_eq!(
        last_audit(&f),
        (
            "work_start".into(),
            AuditOutcome::Denied,
            Some("work_start".into())
        )
    );
    // Asking again does not pile up requests.
    refused(
        f.runtime
            .authorize(&target, Some(&worker()), work("op-1"), &mut tx),
    );
    let pending = f.runtime.approvals(&f.owner.id, &mut tx).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, ApprovalState::Pending);
    assert_eq!(pending[0].operation_id, "op-1");
    // The governed AI cannot approve its own request.
    assert!(matches!(
        f.runtime.decide_approval(
            &f.owner.id,
            &pending[0].approval_id,
            true,
            Some(&worker()),
            &mut tx
        ),
        Err(OrchestrationError::Forbidden(_))
    ));
    let approved = f
        .runtime
        .decide_approval(
            &f.owner.id,
            &pending[0].approval_id,
            true,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(approved.state, ApprovalState::Approved);
    assert_eq!(
        last_audit(&f),
        ("work_start".into(), AuditOutcome::Approved, None)
    );
    // Repeating the answer is not audited twice.
    f.runtime
        .decide_approval(
            &f.owner.id,
            &pending[0].approval_id,
            true,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(last_audit(&f).1, AuditOutcome::Approved);
    let entries = f.runtime.audit(&f.owner.id, 0, 500).unwrap().entries.len();
    // The approval does not transfer to another operation, and is used exactly once.
    assert_eq!(
        refused(
            f.runtime
                .authorize(&target, Some(&worker()), work("op-2"), &mut tx)
        ),
        DenialReason::ApprovalRequired
    );
    f.runtime
        .authorize(&target, Some(&worker()), work("op-1"), &mut tx)
        .unwrap();
    assert_eq!(
        refused(
            f.runtime
                .authorize(&target, Some(&worker()), work("op-1"), &mut tx)
        ),
        DenialReason::ApprovalRequired
    );
    assert_eq!(
        f.runtime.audit(&f.owner.id, 0, 500).unwrap().entries.len(),
        entries + 3
    );
    let states: Vec<_> = f
        .runtime
        .approvals(&f.owner.id, &mut tx)
        .unwrap()
        .into_iter()
        .map(|approval| (approval.operation_id, approval.state))
        .collect();
    assert_eq!(
        states,
        [
            ("op-1".to_owned(), ApprovalState::Consumed),
            ("op-2".to_owned(), ApprovalState::Pending),
            ("op-1".to_owned(), ApprovalState::Pending),
        ]
    );
    // A person's refusal is final for that request and is recorded.
    let second = f.runtime.approvals(&f.owner.id, &mut tx).unwrap().remove(1);
    f.runtime
        .decide_approval(
            &f.owner.id,
            &second.approval_id,
            false,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(last_audit(&f).1, AuditOutcome::Rejected);
    assert_eq!(
        refused(
            f.runtime
                .authorize(&target, Some(&worker()), work("op-2"), &mut tx)
        ),
        DenialReason::ApprovalRequired
    );
    assert_eq!(f.runtime.approvals(&f.owner.id, &mut tx).unwrap().len(), 3);
}

#[test]
fn hs2_ycjdeb_set_validate_and_dry_run_against_fixture_targets() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    // Two declared targets: the first bound and granted, the second neither.
    let manifest = f
        .runtime
        .replace(
            &f.owner.id,
            ReplaceTargets {
                expected_revision: None,
                targets: [(1, "First"), (2, "Second")]
                    .into_iter()
                    .map(|(n, label)| TargetDeclaration {
                        target_id: id(n),
                        state: DeclarationState::Active,
                        label: label.into(),
                        expected_repository_identity: None,
                    })
                    .collect(),
            },
            &mut tx,
        )
        .unwrap();
    bind_local(&f, manifest.revision.unwrap(), &mut tx);
    grant(&f, &[TargetAction::TicketsRead], &mut tx);
    let base = format!("/checkouts/{}/orchestration", f.owner.id);
    let absent = f
        .runtime
        .request("GET", &format!("{base}/policy"), &Value::Null, &mut tx)
        .unwrap();
    assert_eq!(
        absent,
        json!({"enabled":false,"revision":null,"policy":null})
    );
    // Validating a stored policy that does not exist is a clear "not found".
    assert!(matches!(
        f.runtime.request(
            "POST",
            &format!("{base}/policy/validate"),
            &json!({}),
            &mut tx
        ),
        Err(OrchestrationError::NotFound)
    ));
    let proposed = json!({
        "machine": {"max_sessions": 2},
        "tools": [{"tool":"codex","max_sessions":2}],
        "targets": [
            {"target_id": id(1), "actions": ["tickets_read","work_start","sessions_manage"],
             "tools": [{"tool":"codex"}], "max_sessions": 2},
            {"target_id": id(3), "actions": ["tickets_read"]}
        ],
        "approval": {"required_for": ["work_start"]}
    });
    let validation = f
        .runtime
        .request(
            "POST",
            &format!("{base}/policy/validate"),
            &json!({"policy": proposed}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        validation,
        json!({"valid":false,"problems":[{"code":"undeclared_target",
            "message":"policy names a target the manifest does not actively declare",
            "subject":id(3)}]})
    );
    // Setting it is refused for the same reason, for an AI actor, and writes nothing.
    let invalid = f
        .runtime
        .request(
            "PUT",
            &format!("{base}/policy"),
            &json!({"expected_revision":null,"policy":proposed,
                "actor":{"role":"human","id":"maintainer"}}),
            &mut tx,
        )
        .unwrap_err();
    assert_eq!(error_status(&invalid), 400);
    let mut valid = proposed.clone();
    valid["targets"].as_array_mut().unwrap().pop();
    let by_ai = f
        .runtime
        .request(
            "PUT",
            &format!("{base}/policy"),
            &json!({"expected_revision":null,"policy":valid,"actor":{"role":"ai","id":"worker-1"}}),
            &mut tx,
        )
        .unwrap_err();
    assert_eq!(by_ai.code(), "forbidden");
    assert!(
        !Path::new(&f.owner.root)
            .join(".hotsheet2/orchestration-policy.json")
            .exists()
    );
    // A dry run of the proposal shows the outcome before anything is enabled.
    let dry = f
        .runtime
        .request(
            "POST",
            &format!("{base}/policy/dry-run"),
            &json!({"policy": valid, "actor_id": "worker-1"}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(dry["source"], "proposed");
    assert_eq!(dry["valid"], true);
    let first = &dry["targets"][0];
    assert_eq!(first["label"], "First");
    assert_eq!(first["bound"], true);
    assert_eq!(first["policy"], "permitted");
    assert_eq!(first["max_sessions"], 2);
    assert_eq!(first["machine"]["max_sessions"], 2);
    assert_eq!(first["machine"]["min_free_ram_mb"], 2048);
    let row = |target: &Value, action: &str| -> Value {
        target["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["action"] == action)
            .unwrap()
            .clone()
    };
    // Policy and grant agree.
    assert_eq!(row(first, "tickets_read")["policy"], "allowed");
    assert_eq!(row(first, "tickets_read")["grant"]["allowed"], true);
    assert_eq!(row(first, "tickets_read")["automatic"], true);
    // The policy allows a session start; the grant does not cover it.
    let sessions = row(first, "sessions_manage");
    assert_eq!(sessions["tool"], "codex");
    assert_eq!(sessions["policy"], "allowed");
    assert_eq!(sessions["grant"]["reason"], "action_not_granted");
    assert_eq!(sessions["automatic"], false);
    // A person is asked first.
    assert_eq!(row(first, "work_start")["policy"], "approval_required");
    assert_eq!(row(first, "work_start")["policy_detail"], "work_start");
    assert_eq!(row(first, "work_start")["automatic"], false);
    // Not listed by the policy.
    assert_eq!(row(first, "tickets_write")["policy"], "denied");
    assert_eq!(
        row(first, "tickets_write")["policy_detail"],
        "policy_action_not_permitted"
    );
    let second = &dry["targets"][1];
    assert_eq!(second["bound"], false);
    assert_eq!(second["policy"], "not_listed");
    assert_eq!(
        row(second, "tickets_read")["policy_detail"],
        "policy_target_not_permitted"
    );
    assert_eq!(
        row(second, "tickets_read")["grant"]["reason"],
        "target_unavailable"
    );
    // Nothing was written, requested or contacted by any of the above.
    assert!(!f.home.join("orchestration-approvals.json").exists());
    assert_eq!(f.peer.calls.load(Ordering::SeqCst), 0);
    let stored = f
        .runtime
        .request(
            "PUT",
            &format!("{base}/policy"),
            &json!({"expected_revision":null,"policy":valid,
                "actor":{"role":"human","id":"maintainer"}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(stored["enabled"], true);
    assert_eq!(stored["policy"]["wind_down"]["idle_minutes"], 15);
    assert_eq!(
        last_audit(&f),
        ("policy_set".into(), AuditOutcome::Changed, None)
    );
    let entries = f.runtime.audit(&f.owner.id, 0, 500).unwrap().entries.len();
    // An identical set is a no-op and is not audited; a stale revision conflicts.
    let again = f
        .runtime
        .request(
            "PUT",
            &format!("{base}/policy"),
            &json!({"expected_revision":stored["revision"],"policy":valid}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(again, stored);
    assert_eq!(
        f.runtime.audit(&f.owner.id, 0, 500).unwrap().entries.len(),
        entries
    );
    assert!(matches!(
        f.runtime.request(
            "PUT",
            &format!("{base}/policy"),
            &json!({"expected_revision":null,"policy":valid}),
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
    let stored_run = f
        .runtime
        .request(
            "POST",
            &format!("{base}/policy/dry-run"),
            &json!({}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(stored_run["source"], "stored");
    assert!(
        row(&stored_run["targets"][0], "tickets_read")
            .get("grant")
            .is_none()
    );
    assert_eq!(
        f.runtime
            .request(
                "POST",
                &format!("{base}/policy/validate"),
                &json!({}),
                &mut tx
            )
            .unwrap(),
        json!({"valid":true,"problems":[]})
    );
    // Removing a declared target later makes the stored policy report the conflict.
    let current = f
        .runtime
        .declarations(&f.owner.id)
        .unwrap()
        .revision
        .unwrap();
    f.runtime
        .remove(
            &reference(&f),
            RemoveTarget {
                expected_revision: current,
            },
            &mut tx,
        )
        .unwrap();
    let after = f
        .runtime
        .request(
            "POST",
            &format!("{base}/policy/validate"),
            &json!({}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(after["problems"][0]["code"], "undeclared_target");
    for (path, machine) in [
        (format!("{base}/policy"), false),
        (format!("{base}/policy/validate"), false),
        (format!("{base}/policy/dry-run"), true),
        (format!("{base}/approvals"), true),
        (format!("{base}/approvals/{}", id(9)), true),
    ] {
        assert!(is_path(&path), "{path}");
        assert_eq!(is_machine_path(&path), machine, "{path}");
    }
    for body in [json!({}), json!({"approve":"yes"})] {
        assert!(matches!(
            f.runtime.request(
                "POST",
                &format!("{base}/approvals/{}", id(9)),
                &body,
                &mut tx
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
}
