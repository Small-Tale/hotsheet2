//! HS2-SBQ3T8 grant state transitions; synthetic homes and identities only.

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
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn target(value: u128) -> TargetRef {
    TargetRef {
        owner_checkout_id: "owner-0123456789ab".into(),
        target_id: id(value),
    }
}
fn local(checkout: &str) -> TargetDestination {
    TargetDestination::Local {
        server_id: id(900),
        checkout_id: checkout.into(),
        checkout_authority: "a".repeat(64),
    }
}
fn request(actions: &[TargetAction], expires_at_ms: u64) -> CreateGrant {
    CreateGrant {
        grant_id: None,
        actor_id: "worker-1".into(),
        actions: actions.to_vec(),
        session_scope: None,
        expires_at_ms,
    }
}
fn ask(actor: &str, action: TargetAction) -> AccessRequest {
    AccessRequest {
        actor_id: actor.into(),
        action,
        session: None,
        operation_id: None,
    }
}
fn session(tool: &str, model: Option<&str>, running: u32, starting: bool) -> AccessRequest {
    AccessRequest {
        actor_id: "worker-1".into(),
        action: TargetAction::SessionsManage,
        session: Some(SessionRequest {
            tool: tool.into(),
            model: model.map(Into::into),
            running,
            starting,
        }),
        operation_id: None,
    }
}
fn reason(decision: AccessDecision) -> DenialReason {
    assert!(!decision.allowed);
    assert!(decision.grant_id.is_none());
    decision.reason.unwrap()
}

#[test]
fn hs2_sbq3t8_absent_registry_denies_without_creating_a_file() {
    let home = tempfile::tempdir().unwrap();
    let grants = GrantRegistry::new(home.path());
    let here = local("target-0123456789ab");
    assert!(grants.list(&target(1), Some(&here), 10).unwrap().is_empty());
    for action in TargetAction::ALL {
        assert_eq!(
            reason(
                grants
                    .decide(&target(1), Some(&here), &ask("worker-1", action), 10)
                    .unwrap()
            ),
            DenialReason::NoGrant
        );
    }
    assert_eq!(
        reason(
            grants
                .decide(
                    &target(1),
                    None,
                    &ask("worker-1", TargetAction::TicketsRead),
                    10
                )
                .unwrap()
        ),
        DenialReason::TargetUnavailable
    );
    assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());
}

#[test]
fn hs2_sbq3t8_denial_reason_wire_spelling_equals_its_code() {
    for reason in [
        DenialReason::NoGrant,
        DenialReason::ActionNotGranted,
        DenialReason::DestinationChanged,
        DenialReason::Revoked,
        DenialReason::Expired,
        DenialReason::SessionScopeRequired,
        DenialReason::ToolNotGranted,
        DenialReason::ModelNotGranted,
        DenialReason::ConcurrencyExceeded,
        DenialReason::TargetUnavailable,
        DenialReason::PolicyDenied,
        DenialReason::ApprovalRequired,
    ] {
        assert_eq!(serde_json::to_value(reason).unwrap(), reason.code());
    }
    for action in TargetAction::ALL {
        assert_eq!(serde_json::to_value(action).unwrap(), action.code());
    }
}

#[test]
fn hs2_sbq3t8_grant_is_scoped_to_target_actor_action_and_expiry() {
    let home = tempfile::tempdir().unwrap();
    let grants = GrantRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let here = local("target-0123456789ab");
    let view = grants
        .create(
            &target(1),
            request(
                &[TargetAction::TicketsWrite, TargetAction::TicketsRead],
                1_000,
            ),
            &here,
            Some("maintainer".into()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(view.state, GrantState::Active);
    // Stored ordered and unique regardless of request order.
    assert_eq!(
        view.grant.actions,
        vec![TargetAction::TicketsRead, TargetAction::TicketsWrite]
    );
    assert_eq!(view.grant.created_by.as_deref(), Some("maintainer"));
    let allowed = grants
        .decide(
            &target(1),
            Some(&here),
            &ask("worker-1", TargetAction::TicketsWrite),
            999,
        )
        .unwrap();
    assert!(allowed.allowed);
    assert_eq!(
        allowed.grant_id.as_deref(),
        Some(view.grant.grant_id.as_str())
    );
    // Each scope dimension denies independently.
    let cases = [
        (
            target(1),
            "worker-1",
            TargetAction::WorkStart,
            999,
            DenialReason::ActionNotGranted,
        ),
        (
            target(1),
            "worker-2",
            TargetAction::TicketsRead,
            999,
            DenialReason::NoGrant,
        ),
        (
            target(2),
            "worker-1",
            TargetAction::TicketsRead,
            999,
            DenialReason::NoGrant,
        ),
        (
            target(1),
            "worker-1",
            TargetAction::TicketsRead,
            1_000,
            DenialReason::Expired,
        ),
        (
            target(1),
            "worker-1",
            TargetAction::TicketsRead,
            5_000,
            DenialReason::Expired,
        ),
    ];
    for (target, actor, action, now, expected) in cases {
        assert_eq!(
            reason(
                grants
                    .decide(&target, Some(&here), &ask(actor, action), now)
                    .unwrap()
            ),
            expected
        );
    }
    // Another owner with the same target id is a different key.
    let other_owner = TargetRef {
        owner_checkout_id: "other-0123456789ab".into(),
        target_id: id(1),
    };
    assert_eq!(
        reason(
            grants
                .decide(
                    &other_owner,
                    Some(&here),
                    &ask("worker-1", TargetAction::TicketsRead),
                    999
                )
                .unwrap()
        ),
        DenialReason::NoGrant
    );
    assert_eq!(
        grants.list(&target(1), Some(&here), 1_000).unwrap()[0].state,
        GrantState::Expired
    );
}

#[test]
fn hs2_sbq3t8_rebind_or_unbind_makes_a_grant_inert_and_rebinding_back_restores_it() {
    let home = tempfile::tempdir().unwrap();
    let grants = GrantRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let first = local("target-0123456789ab");
    let second = local("moved-0123456789ab");
    let remote = TargetDestination::Local {
        server_id: id(901),
        checkout_id: "target-0123456789ab".into(),
        checkout_authority: "a".repeat(64),
    };
    grants
        .create(
            &target(1),
            request(&[TargetAction::TicketsRead], 1_000),
            &first,
            None,
            &mut tx,
        )
        .unwrap();
    let read = ask("worker-1", TargetAction::TicketsRead);
    for other in [&second, &remote] {
        assert_eq!(
            reason(grants.decide(&target(1), Some(other), &read, 200).unwrap()),
            DenialReason::DestinationChanged
        );
        assert_eq!(
            grants.list(&target(1), Some(other), 200).unwrap()[0].state,
            GrantState::DestinationChanged
        );
    }
    assert_eq!(
        reason(grants.decide(&target(1), None, &read, 200).unwrap()),
        DenialReason::TargetUnavailable
    );
    assert_eq!(
        grants.list(&target(1), None, 200).unwrap()[0].state,
        GrantState::DestinationChanged
    );
    assert!(
        grants
            .decide(&target(1), Some(&first), &read, 200)
            .unwrap()
            .allowed
    );
}

#[test]
fn hs2_sbq3t8_create_and_revoke_are_idempotent_and_revocation_is_final() {
    let home = tempfile::tempdir().unwrap();
    let grants = GrantRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let here = local("target-0123456789ab");
    let mut retained = request(&[TargetAction::WorkStart], 1_000);
    retained.grant_id = Some(id(77));
    let first = grants
        .create(&target(1), retained.clone(), &here, None, &mut tx)
        .unwrap();
    let path = home.path().join("orchestration-grants.json");
    let bytes = std::fs::read(&path).unwrap();
    clock.0.set(150);
    let again = grants
        .create(&target(1), retained.clone(), &here, None, &mut tx)
        .unwrap();
    assert_eq!(again, first);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    // The same id with any different scope conflicts rather than widening silently.
    let mut widened = retained.clone();
    widened.actions.push(TargetAction::TicketsWrite);
    let mut longer = retained.clone();
    longer.expires_at_ms = 2_000;
    let mut other_actor = retained.clone();
    other_actor.actor_id = "worker-2".into();
    for changed in [widened, longer, other_actor] {
        assert!(matches!(
            grants.create(&target(1), changed, &here, None, &mut tx),
            Err(OrchestrationError::Conflict)
        ));
    }
    assert!(matches!(
        grants.create(&target(2), retained.clone(), &here, None, &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    assert!(matches!(
        grants.revoke(&target(2), &id(77), None, &mut tx),
        Err(OrchestrationError::NotFound)
    ));
    assert!(matches!(
        grants.revoke(&target(1), &id(78), None, &mut tx),
        Err(OrchestrationError::NotFound)
    ));
    clock.0.set(200);
    let (revoked, changed) = grants
        .revoke(&target(1), &id(77), Some("maintainer".into()), &mut tx)
        .unwrap();
    assert!(changed);
    assert_eq!(revoked.revoked_at_ms, Some(200));
    clock.0.set(300);
    let repeated = grants
        .revoke(&target(1), &id(77), Some("someone-else".into()), &mut tx)
        .unwrap();
    assert_eq!(repeated, (revoked, false));
    let start = ask("worker-1", TargetAction::WorkStart);
    assert_eq!(
        reason(grants.decide(&target(1), Some(&here), &start, 300).unwrap()),
        DenialReason::Revoked
    );
    // A retry of the original create returns the revoked record; it never revives it.
    let retried = grants
        .create(&target(1), retained, &here, None, &mut tx)
        .unwrap();
    assert_eq!(retried.state, GrantState::Revoked);
    // A fresh grant restores access while the revoked one stays in history.
    grants
        .create(
            &target(1),
            request(&[TargetAction::WorkStart], 1_000),
            &here,
            None,
            &mut tx,
        )
        .unwrap();
    assert!(
        grants
            .decide(&target(1), Some(&here), &start, 300)
            .unwrap()
            .allowed
    );
    assert_eq!(grants.list(&target(1), Some(&here), 300).unwrap().len(), 2);
}

#[test]
fn hs2_sbq3t8_session_scope_limits_tool_model_and_concurrency() {
    let home = tempfile::tempdir().unwrap();
    let grants = GrantRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let here = local("target-0123456789ab");
    let mut create = request(&[TargetAction::SessionsManage], 1_000);
    create.session_scope = Some(SessionScope {
        tools: vec![
            ToolScope {
                tool: "claude".into(),
                models: vec!["model-a".into()],
            },
            ToolScope {
                tool: "codex".into(),
                models: Vec::new(),
            },
        ],
        max_concurrency: 2,
    });
    grants
        .create(&target(1), create, &here, None, &mut tx)
        .unwrap();
    let decide = |request: AccessRequest| {
        grants
            .decide(&target(1), Some(&here), &request, 200)
            .unwrap()
    };
    assert!(decide(session("claude", Some("model-a"), 0, true)).allowed);
    assert!(decide(session("claude", Some("model-a"), 1, true)).allowed);
    assert!(decide(session("codex", None, 1, true)).allowed);
    assert!(decide(session("codex", Some("anything"), 1, true)).allowed);
    assert_eq!(
        reason(decide(session("claude", Some("model-b"), 0, true))),
        DenialReason::ModelNotGranted
    );
    // A restricted tool cannot be started on an unnamed default model.
    assert_eq!(
        reason(decide(session("claude", None, 0, true))),
        DenialReason::ModelNotGranted
    );
    assert_eq!(
        reason(decide(session("opencode", None, 0, true))),
        DenialReason::ToolNotGranted
    );
    assert_eq!(
        reason(decide(session("codex", None, 2, true))),
        DenialReason::ConcurrencyExceeded
    );
    // Stopping or winding down is never blocked by the cap it helps restore.
    assert!(decide(session("codex", None, 5, false)).allowed);
    assert_eq!(
        reason(decide(ask("worker-1", TargetAction::SessionsManage))),
        DenialReason::SessionScopeRequired
    );
}

#[test]
fn hs2_sbq3t8_invalid_grants_are_rejected_before_any_write() {
    let home = tempfile::tempdir().unwrap();
    let grants = GrantRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let here = local("target-0123456789ab");
    let scope = |tools: Vec<ToolScope>, max_concurrency| {
        Some(SessionScope {
            tools,
            max_concurrency,
        })
    };
    let tool = |name: &str| ToolScope {
        tool: name.into(),
        models: Vec::new(),
    };
    let mut cases = vec![
        request(&[], 1_000),
        request(&[TargetAction::TicketsRead], 100),
        request(&[TargetAction::TicketsRead], 50),
        request(
            &[TargetAction::TicketsRead],
            100 + MAX_GRANT_LIFETIME_MS + 1,
        ),
        request(&[TargetAction::SessionsManage], 1_000),
    ];
    let mut bad_actor = request(&[TargetAction::TicketsRead], 1_000);
    bad_actor.actor_id = "has space".into();
    cases.push(bad_actor);
    let mut scope_without_action = request(&[TargetAction::TicketsRead], 1_000);
    scope_without_action.session_scope = scope(vec![tool("claude")], 1);
    cases.push(scope_without_action);
    for session_scope in [
        scope(Vec::new(), 1),
        scope(vec![tool("claude")], 0),
        scope(vec![tool("claude")], MAX_GRANT_CONCURRENCY + 1),
        scope(vec![tool("claude"), tool("claude")], 1),
        scope(vec![tool("bad tool")], 1),
        scope(
            vec![ToolScope {
                tool: "claude".into(),
                models: vec!["m".into(), "m".into()],
            }],
            1,
        ),
    ] {
        let mut create = request(&[TargetAction::SessionsManage], 1_000);
        create.session_scope = session_scope;
        cases.push(create);
    }
    let mut bad_id = request(&[TargetAction::TicketsRead], 1_000);
    bad_id.grant_id = Some("not-a-ulid".into());
    cases.push(bad_id);
    for create in cases {
        assert!(matches!(
            grants.create(&target(1), create, &here, None, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    let invalid_destination = TargetDestination::Local {
        server_id: "nope".into(),
        checkout_id: "target".into(),
        checkout_authority: "a".repeat(64),
    };
    assert!(
        grants
            .create(
                &target(1),
                request(&[TargetAction::TicketsRead], 1_000),
                &invalid_destination,
                None,
                &mut tx
            )
            .is_err()
    );
    assert!(!home.path().join("orchestration-grants.json").exists());
    // The longest allowed lifetime is accepted exactly at its bound.
    grants
        .create(
            &target(1),
            request(&[TargetAction::TicketsRead], 100 + MAX_GRANT_LIFETIME_MS),
            &here,
            None,
            &mut tx,
        )
        .unwrap();
}

#[test]
fn hs2_sbq3t8_capacity_prunes_only_inert_grants_and_never_an_active_one() {
    let home = tempfile::tempdir().unwrap();
    let grants = GrantRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let here = local("target-0123456789ab");
    let mut ids = Vec::new();
    for index in 0..MAX_GRANTS_PER_TARGET {
        // The first two expire early; the rest stay active.
        let expiry = if index < 2 { 200 } else { 10_000 };
        ids.push(
            grants
                .create(
                    &target(1),
                    request(&[TargetAction::TicketsRead], expiry),
                    &here,
                    None,
                    &mut tx,
                )
                .unwrap()
                .grant
                .grant_id,
        );
    }
    // Full of active grants: a new one is refused and nothing is dropped.
    assert!(matches!(
        grants.create(
            &target(1),
            request(&[TargetAction::TicketsRead], 10_000),
            &here,
            None,
            &mut tx
        ),
        Err(OrchestrationError::Capacity)
    ));
    // Another target is unaffected by this target's cap.
    grants
        .create(
            &target(2),
            request(&[TargetAction::TicketsRead], 10_000),
            &here,
            None,
            &mut tx,
        )
        .unwrap();
    clock.0.set(300);
    grants.revoke(&target(1), &ids[5], None, &mut tx).unwrap();
    let added = grants
        .create(
            &target(1),
            request(&[TargetAction::TicketsRead], 10_000),
            &here,
            None,
            &mut tx,
        )
        .unwrap();
    let kept: Vec<_> = grants
        .list(&target(1), Some(&here), 300)
        .unwrap()
        .into_iter()
        .map(|view| view.grant.grant_id)
        .collect();
    // Exactly one inert grant (the oldest) made room; the others remain as history.
    assert_eq!(kept.len(), MAX_GRANTS_PER_TARGET);
    assert!(!kept.contains(&ids[0]));
    assert!(kept.contains(&ids[1]));
    assert!(kept.contains(&ids[5]));
    assert!(kept.contains(&added.grant.grant_id));
    assert_eq!(grants.list(&target(2), Some(&here), 300).unwrap().len(), 1);
}

#[test]
fn hs2_sbq3t8_corrupt_or_newer_grant_data_fails_closed_and_is_not_overwritten() {
    let home = tempfile::tempdir().unwrap();
    let grants = GrantRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let here = local("target-0123456789ab");
    let path = home.path().join("orchestration-grants.json");
    for (bytes, newer) in [
        (&b"{not json"[..], false),
        (&br#"{"schema_version":2,"grants":[]}"#[..], true),
        (
            &br#"{"schema_version":1,"grants":[{"grant_id":"x"}]}"#[..],
            false,
        ),
    ] {
        std::fs::write(&path, bytes).unwrap();
        let read = ask("worker-1", TargetAction::TicketsRead);
        let decided = grants.decide(&target(1), Some(&here), &read, 200);
        let created = grants.create(
            &target(1),
            request(&[TargetAction::TicketsRead], 1_000),
            &here,
            None,
            &mut tx,
        );
        if newer {
            assert!(matches!(
                decided,
                Err(OrchestrationError::UnsupportedVersion { found: 2 })
            ));
            assert!(matches!(
                created,
                Err(OrchestrationError::UnsupportedVersion { found: 2 })
            ));
        } else {
            assert!(matches!(decided, Err(OrchestrationError::Corrupt)));
            assert!(matches!(created, Err(OrchestrationError::Corrupt)));
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[cfg(unix)]
#[test]
fn hs2_sbq3t8_grant_file_is_private_and_holds_no_destination_identity() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let grants = GrantRegistry::new(home.path());
    let clock = Clock(100.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let here = local("target-0123456789ab");
    grants
        .create(
            &target(1),
            request(&[TargetAction::TicketsRead], 1_000),
            &here,
            None,
            &mut tx,
        )
        .unwrap();
    let path = home.path().join("orchestration-grants.json");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let text = std::fs::read_to_string(&path).unwrap();
    // Only the digest is stored; the destination's checkout and server ids are not.
    assert!(!text.contains("target-0123456789ab"));
    assert!(!text.contains(&id(900)));
    assert!(text.contains(&destination_key(&here).unwrap()));
}
