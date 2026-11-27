//! HS2-YCJDEB: the throughput policy and its dry run through the shared runtime, and
//! its place in the authorization gate. Since HS2-XHQ1EH it holds limits only.
//! Synthetic identities only.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, DenialReason, MachineLimits, PolicyBody, SessionLoad, SessionRequest, SetPolicy,
    TargetAction, TargetPolicy, ToolPolicy, ToolScope,
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
fn policy(sessions: bool) -> PolicyBody {
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 2,
            ..MachineLimits::default()
        },
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
            tools: if sessions { vec![codex()] } else { Vec::new() },
            max_sessions: Some(1),
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
fn allow(f: &Fixture, actions: &[TargetAction], tx: &mut Transaction<'_>) {
    allow_on(&f.runtime, &f.owner.id, &id(1), actions, &["codex"], tx);
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
fn hs2_xhq1eh_the_policy_limits_and_a_person_allows() {
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
    // No policy: an explicit ticket action has no limits and asks a person, while a
    // session start is refused because nothing automatic runs without a policy, and
    // nobody is asked about it.
    permission_request(
        f.runtime
            .authorize(&target, Some(&worker()), write(), &mut tx),
    );
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
    assert_eq!(
        f.runtime
            .permission_list(&f.owner.id, &mut tx)
            .unwrap()
            .requests
            .len(),
        1
    );
    // Remembered rules and a policy that permits both: both go ahead.
    allow(
        &f,
        &[TargetAction::TicketsWrite, TargetAction::SessionsManage],
        &mut tx,
    );
    set(&f, policy(true), &mut tx);
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
    // A remembered rule never lifts a limit: the policy caps the target at one.
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
    // A target the policy disables limits every acting action, even a person's own.
    let mut disabled = policy(true);
    disabled.targets[0].enabled = false;
    set(&f, disabled, &mut tx);
    for actor in [worker(), human()] {
        assert_eq!(
            refused(f.runtime.authorize(&target, Some(&actor), write(), &mut tx)),
            DenialReason::PolicyDenied
        );
        assert_eq!(last_audit(&f).2.as_deref(), Some("policy_target_disabled"));
    }
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
        json!({"allowed":false,"reason":"policy_denied","detail":"policy_target_disabled"})
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
fn hs2_ycjdeb_set_validate_and_dry_run_against_fixture_targets() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    // Two declared targets: the first bound with a remembered rule, the second neither.
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
                        site_of: None,
                    })
                    .collect(),
            },
            &mut tx,
        )
        .unwrap();
    bind_local(&f, manifest.revision.unwrap(), &mut tx);
    allow(&f, &[TargetAction::WorkStart], &mut tx);
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
            "subject":id(3)}],
            // HS2-XHQ1EH: retired fields read, are ignored, and are reported.
            "ignored":[{"code":"approval_ignored","field":"approval"},
                {"code":"target_actions_ignored","field":"targets[].actions","subject":id(1)},
                {"code":"target_actions_ignored","field":"targets[].actions","subject":id(3)}],
            "retired":{"grants_file":false,"approvals_file":false}})
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
            &json!({"policy": valid}),
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
    // Reading needs no permission.
    assert_eq!(row(first, "tickets_read")["policy"], "allowed");
    assert_eq!(row(first, "tickets_read")["permission"], "not_needed");
    assert_eq!(row(first, "tickets_read")["automatic"], true);
    // The policy allows a session start; a person would be asked.
    let sessions = row(first, "sessions_manage");
    assert_eq!(sessions["tool"], "codex");
    assert_eq!(sessions["policy"], "allowed");
    assert_eq!(sessions["permission"], "asks");
    assert_eq!(sessions["automatic"], false);
    // A remembered rule answers work_start.
    assert_eq!(row(first, "work_start")["policy"], "allowed");
    assert_eq!(row(first, "work_start")["permission"], "remembered");
    assert!(row(first, "work_start")["rule_id"].is_string());
    assert_eq!(row(first, "work_start")["automatic"], true);
    let second = &dry["targets"][1];
    assert_eq!(second["bound"], false);
    assert_eq!(second["policy"], "not_listed");
    // An unlisted target is not limited for ticket actions; it is unbound, so
    // nothing there is automatic. HS2-N5BDE4: it is Auto, so a session start on a
    // tool the policy lists is within its limits too.
    assert_eq!(row(second, "tickets_read")["policy"], "allowed");
    assert_eq!(row(second, "tickets_read")["automatic"], false);
    assert_eq!(row(second, "sessions_manage")["policy"], "allowed");
    assert_eq!(row(second, "sessions_manage")["automatic"], false);
    // Nothing was written, requested or contacted by any of the above.
    assert!(
        f.runtime
            .permission_list(&f.owner.id, &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
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
    // The stored policy no longer holds the retired fields.
    assert_eq!(
        f.runtime
            .request(
                "POST",
                &format!("{base}/policy/validate"),
                &json!({}),
                &mut tx
            )
            .unwrap(),
        json!({"valid":true,"problems":[],"ignored":[],
            "retired":{"grants_file":false,"approvals_file":false}})
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
        (format!("{base}/permissions"), true),
        (format!("{base}/permissions/{}", id(9)), true),
    ] {
        assert!(is_path(&path), "{path}");
        assert_eq!(is_machine_path(&path), machine, "{path}");
    }
    for body in [json!({}), json!({"answer":"yes"})] {
        assert!(matches!(
            f.runtime.request(
                "POST",
                &format!("{base}/permissions/{}", id(9)),
                &body,
                &mut tx
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
}

/// HS2-TZJYKH: Start and Pause are one runtime edit of the stored policy at its
/// revision. The first Start saves the default policy with it; a repeat keeps the
/// revision; a stale revision is the policy's `conflict`; an AI actor is refused; and
/// every other member a person set survives.
#[test]
fn hs2_tzjykh_start_and_pause_edit_the_stored_policy_at_its_revision() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    declaration(&f, &mut tx);
    let path = format!("/checkouts/{}/orchestration/policy/automatic", f.owner.id);
    let person = json!({"role":"human","id":"maintainer"});
    let put = |body: Value, tx: &mut Transaction<'_>| f.runtime.request("PUT", &path, &body, tx);

    // An AI actor may not start it, and nothing is written.
    let refused = put(
        json!({"automatic":true,"actor":{"role":"ai","id":"worker-1"}}),
        &mut tx,
    )
    .unwrap_err();
    assert!(
        matches!(refused, OrchestrationError::Forbidden(_)),
        "{refused}"
    );
    assert_eq!(f.runtime.policy(&f.owner.id).unwrap().revision, None);
    // A revision when none is saved is stale.
    assert!(matches!(
        put(
            json!({"automatic":true,"expected_revision":id(99),"actor":person}),
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
    // An unknown member is refused rather than ignored.
    assert!(matches!(
        put(
            json!({"automatic":true,"automatic_edit":true,"actor":person}),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_) | OrchestrationError::UnknownField { .. })
    ));

    // No policy: the defaults, with placement on, in one write.
    let started = put(json!({"automatic":true,"actor":person}), &mut tx).unwrap();
    assert_eq!(started["saved_default"], true, "{started}");
    assert_eq!(started["enabled"], true);
    let mut expected = PolicyBody::default();
    expected.placement.automatic = Some(true);
    assert_eq!(
        started["policy"],
        serde_json::to_value(&expected).unwrap(),
        "every other member stays at its default"
    );
    let first = started["revision"].clone();
    assert_eq!(
        f.runtime.policy(&f.owner.id).unwrap().policy,
        Some(expected)
    );
    // HS2-38R8J2: placement was turned on, so a later pause reads Paused.
    assert!(
        f.runtime
            .placement_started_at(&f.owner.id)
            .unwrap()
            .is_some()
    );

    // Again: the same revision, and a saved policy is not the default one.
    let again = put(json!({"automatic":true,"actor":person}), &mut tx).unwrap();
    assert_eq!(
        (again["revision"].clone(), again["saved_default"].clone()),
        (first.clone(), json!(false))
    );

    // A person's own limits survive Pause and Start.
    let mut edited = policy(false);
    edited.placement.automatic = Some(true);
    edited.placement.aging_minutes = 30;
    let limits = set(&f, edited.clone(), &mut tx);
    // Pause at a stale revision conflicts and changes nothing.
    assert!(matches!(
        put(
            json!({"automatic":false,"expected_revision":first,"actor":person}),
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
    assert_eq!(f.runtime.policy(&f.owner.id).unwrap().revision, limits);
    // Pause at the current revision.
    let paused = put(
        json!({"automatic":false,"expected_revision":limits,"actor":person}),
        &mut tx,
    )
    .unwrap();
    assert_ne!(paused["revision"], json!(limits));
    edited.placement.automatic = Some(false);
    assert_eq!(
        f.runtime.policy(&f.owner.id).unwrap().policy,
        Some(edited.clone())
    );
    // Pause again, then Start: placement only, at each revision read.
    let repeat = put(json!({"automatic":false,"actor":person}), &mut tx).unwrap();
    assert_eq!(repeat["revision"], paused["revision"]);
    let resumed = put(json!({"automatic":true,"actor":person}), &mut tx).unwrap();
    edited.placement.automatic = Some(true);
    assert_eq!(f.runtime.policy(&f.owner.id).unwrap().policy, Some(edited));
    assert_eq!(resumed["saved_default"], false);
    let (action, outcome, _) = last_audit(&f);
    assert_eq!(
        (action.as_str(), outcome),
        ("policy_set", AuditOutcome::Changed)
    );
}

/// HS2-TZJYKH: one target's role sessions, edited by the runtime: added last,
/// replaced in place, none kept as a choice, Auto removing the entry, and a repeat
/// keeping the revision. Other targets' entries and every other member stay.
#[test]
fn hs2_tzjykh_role_sessions_edit_one_target_at_the_stored_revision() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    f.runtime
        .replace(
            &f.owner.id,
            ReplaceTargets {
                expected_revision: None,
                targets: [(1, None), (2, None), (3, Some(id(1)))]
                    .into_iter()
                    .map(|(n, site_of)| TargetDeclaration {
                        target_id: id(n),
                        state: DeclarationState::Active,
                        label: format!("Synthetic {n}"),
                        expected_repository_identity: None,
                        site_of,
                    })
                    .collect(),
            },
            &mut tx,
        )
        .unwrap();
    let path = format!(
        "/checkouts/{}/orchestration/policy/role-sessions",
        f.owner.id
    );
    let person = json!({"role":"human","id":"maintainer"});
    let put = |body: Value, tx: &mut Transaction<'_>| {
        let mut body = body;
        body["actor"] = person.clone();
        f.runtime.request("PUT", &path, &body, tx)
    };
    let stored = |f: &Fixture| {
        serde_json::to_value(f.runtime.policy(&f.owner.id).unwrap().policy).unwrap()["placement"]
            ["role_sessions"]
            .clone()
    };

    // Auto together with roles is refused; an AI actor is refused; nothing is written.
    assert!(matches!(
        put(
            json!({"target_id":id(1),"roles":["dispatcher"],"auto":true}),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    assert!(matches!(
        f.runtime.request(
            "PUT",
            &path,
            &json!({"target_id":id(1),"roles":[],"actor":{"role":"ai","id":"worker-1"}}),
            &mut tx
        ),
        Err(OrchestrationError::Forbidden(_))
    ));
    assert_eq!(f.runtime.policy(&f.owner.id).unwrap().revision, None);
    // Auto on a target with no entry and no policy saves the default policy unchanged.
    let auto = put(json!({"target_id":id(1),"auto":true}), &mut tx).unwrap();
    assert_eq!(
        auto["policy"],
        serde_json::to_value(PolicyBody::default()).unwrap()
    );
    assert!(auto.get("warnings").is_none());
    // Entries need the policy to list their targets.
    let listed = PolicyBody {
        targets: (1..=3)
            .map(|n| TargetPolicy {
                target_id: id(n),
                enabled: true,
                actions: None,
                tools: Vec::new(),
                max_sessions: None,
                machine: None,
                speculation: None,
            })
            .collect(),
        ..PolicyBody::default()
    };
    set(&f, listed.clone(), &mut tx);

    // Added last, in the order given.
    let one = put(
        json!({"target_id":id(1),"roles":["integrator","dispatcher"]}),
        &mut tx,
    )
    .unwrap();
    put(json!({"target_id":id(2),"roles":["dispatcher"]}), &mut tx).unwrap();
    assert_eq!(
        stored(&f),
        json!([{"target_id":id(1),"roles":["integrator","dispatcher"]},
            {"target_id":id(2),"roles":["dispatcher"]}])
    );
    // Replaced in place with none: the choice of none is kept.
    let none = put(json!({"target_id":id(1),"roles":[]}), &mut tx).unwrap();
    assert_ne!(none["revision"], one["revision"]);
    assert_eq!(
        stored(&f),
        json!([{"target_id":id(1),"roles":[]},{"target_id":id(2),"roles":["dispatcher"]}])
    );
    // A repeat keeps the revision.
    let repeat = put(json!({"target_id":id(1),"roles":[]}), &mut tx).unwrap();
    assert_eq!(repeat["revision"], none["revision"]);
    // A stale revision conflicts and changes nothing.
    assert!(matches!(
        put(
            json!({"target_id":id(1),"roles":["dispatcher"],"expected_revision":one["revision"]}),
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
    assert_eq!(
        f.runtime
            .policy(&f.owner.id)
            .unwrap()
            .revision
            .map(Value::from),
        Some(none["revision"].clone())
    );
    // At the current revision it applies.
    put(
        json!({"target_id":id(1),"roles":["dispatcher"],"expected_revision":none["revision"]}),
        &mut tx,
    )
    .unwrap();
    // HS2-SYZB33: an entry on a site that is not its project's home is said.
    let site = put(json!({"target_id":id(3),"roles":["integrator"]}), &mut tx).unwrap();
    assert_eq!(
        site["warnings"][0]["code"], "role_sessions_not_home",
        "{site}"
    );
    assert_eq!(site["warnings"][0]["subject"], id(3));
    // Auto removes the entries again, the last leaving no list at all.
    for n in [3, 1] {
        assert!(
            put(json!({"target_id":id(n),"auto":true}), &mut tx)
                .unwrap()
                .get("warnings")
                .is_none()
        );
    }
    assert_eq!(
        stored(&f),
        json!([{"target_id":id(2),"roles":["dispatcher"]}])
    );
    put(json!({"target_id":id(2),"auto":true}), &mut tx).unwrap();
    assert_eq!(stored(&f), Value::Null);
    assert_eq!(f.runtime.policy(&f.owner.id).unwrap().policy, Some(listed));
}

/// HS2-4F9VGQ: a policy carrying a member nothing reads is refused with the field named
/// by its path, through validate and set alike, and the refusal code stays
/// `invalid_request` (400). Nested unknown members name their place; a request whose
/// failure is not an unknown field keeps the generic refusal and echoes no value.
#[test]
fn hs2_4f9vgq_an_unknown_policy_field_is_named_by_its_path() {
    let f = fixture();
    let mut rng = Rng(400);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let base = format!("/checkouts/{}/orchestration", f.owner.id);
    let mut refused = |method: &str, path: &str, body: Value| {
        f.runtime
            .request(method, &format!("{base}/{path}"), &body, &mut tx)
            .unwrap_err()
    };
    let limits = json!({"machine": {"max_sessions": 2}});
    let mut extra = limits.clone();
    extra["schema_version"] = json!(1);
    for (method, path, body) in [
        ("POST", "policy/validate", json!({"policy": extra.clone()})),
        ("POST", "policy/dry-run", json!({"policy": extra.clone()})),
        (
            "PUT",
            "policy",
            json!({"expected_revision": null, "policy": extra.clone(),
                "actor": {"role": "human", "id": "maintainer"}}),
        ),
    ] {
        let error = refused(method, path, body);
        assert_eq!(error.code(), "invalid_request", "{path}");
        assert_eq!(error_status(&error), 400, "{path}");
        assert_eq!(
            error.to_string(),
            "invalid orchestration request: `policy.schema_version` is not a known field",
            "{path}"
        );
    }
    // A nested member names its place, and an unknown top-level request member too.
    let nested = refused(
        "POST",
        "policy/validate",
        json!({"policy": {"machine": {"max_sessions": 2, "max_sesions": 3}}}),
    );
    assert_eq!(
        nested.to_string(),
        "invalid orchestration request: `policy.machine.max_sesions` is not a known field"
    );
    let top = refused(
        "POST",
        "policy/validate",
        json!({"policy": limits.clone(), "dry": true}),
    );
    assert_eq!(
        top.to_string(),
        "invalid orchestration request: `dry` is not a known field"
    );
    // A field name is the caller's text: kept printable and short.
    let mut odd = limits.clone();
    odd[format!("bad name\n{}", "x".repeat(300))] = json!(1);
    let odd = refused("POST", "policy/validate", json!({"policy": odd}));
    let OrchestrationError::UnknownField { path } = &odd else {
        panic!("{odd:?}");
    };
    assert!(path.starts_with("policy.badname"), "{path}");
    assert_eq!(path.chars().count(), 120, "{path}");
    // A wrong type is not an unknown field: the generic refusal, no value echoed.
    let typed = refused(
        "POST",
        "policy/validate",
        json!({"policy": {"machine": {"max_sessions": "synthetic-secret"}}}),
    );
    assert!(matches!(typed, OrchestrationError::Invalid(_)), "{typed:?}");
    assert!(!typed.to_string().contains("synthetic-secret"));
    // Without the extra member the same policy validates.
    let valid = f
        .runtime
        .request(
            "POST",
            &format!("{base}/policy/validate"),
            &json!({"policy": limits}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(valid["valid"], true, "{valid}");
}
