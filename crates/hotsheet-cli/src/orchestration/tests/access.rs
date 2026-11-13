//! HS2-SBQ3T8: grants, the authorization gate and the audit journal through the
//! shared runtime, for local and remote destinations. Synthetic identities only.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, CreateGrant, DenialReason, GrantState, PolicyBody, SessionLoad, SessionRequest,
    SessionScope, SetPolicy, TargetAction, TargetPolicy, ToolPolicy, ToolScope,
};
use serde_json::{Value, json};

use super::*;

fn human() -> MutationActor {
    MutationActor::parse("human", Some("maintainer".into())).unwrap()
}
fn ai(id: &str) -> MutationActor {
    MutationActor::parse("ai", Some(id.into())).unwrap()
}
fn create(actions: &[TargetAction]) -> CreateGrant {
    CreateGrant {
        grant_id: None,
        actor_id: "worker-1".into(),
        actions: actions.to_vec(),
        session_scope: None,
        expires_at_ms: 10_000,
    }
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

#[test]
fn hs2_sbq3t8_local_target_authority_is_granted_audited_and_fails_closed() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let target = reference(&f);
    // Unbound: nothing can be granted and nothing is authorized.
    declaration(&f, &mut tx);
    assert!(matches!(
        f.runtime.grant(
            &target,
            create(&[TargetAction::TicketsWrite]),
            Some(&human()),
            &mut tx
        ),
        Err(OrchestrationError::Denied(DenialReason::TargetUnavailable))
    ));
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, Some("op-1".into())),
            &mut tx
        )),
        DenialReason::TargetUnavailable
    );
    std::fs::remove_file(Path::new(&f.owner.root).join(".hotsheet2/orchestration.json")).unwrap();
    bind_local(&f, &mut tx);
    // Binding alone authorizes nothing.
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, Some("op-2".into())),
            &mut tx
        )),
        DenialReason::NoGrant
    );
    // The governed AI cannot give itself authority, nor withdraw someone's.
    assert!(matches!(
        f.runtime.grant(
            &target,
            create(&[TargetAction::TicketsWrite]),
            Some(&ai("worker-1")),
            &mut tx
        ),
        Err(OrchestrationError::Forbidden(_))
    ));
    let granted = f
        .runtime
        .grant(
            &target,
            create(&[TargetAction::TicketsWrite]),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(granted.state, GrantState::Active);
    assert_eq!(granted.grant.created_by.as_deref(), Some("maintainer"));
    assert!(matches!(
        f.runtime.revoke(
            &target,
            &granted.grant.grant_id,
            Some(&ai("worker-1")),
            &mut tx
        ),
        Err(OrchestrationError::Forbidden(_))
    ));
    let authorized = f
        .runtime
        .authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, Some("op-3".into())),
            &mut tx,
        )
        .unwrap();
    assert_eq!(authorized.grant_id, granted.grant.grant_id);
    assert!(authorized.resolution.usable);
    f.runtime
        .record_outcome(&target, &authorized, true, None, &mut tx)
        .unwrap();
    // An unidentified caller is an invalid request, not an anonymous allowance.
    for actor in [None, Some(MutationActor::parse("ai", None).unwrap())] {
        assert!(matches!(
            f.runtime.authorize(
                &target,
                actor.as_ref(),
                act(TargetAction::TicketsWrite, None, None),
                &mut tx
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-2")),
            act(TargetAction::TicketsWrite, None, None),
            &mut tx
        )),
        DenialReason::NoGrant
    );
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::WorkStart, None, None),
            &mut tx
        )),
        DenialReason::ActionNotGranted
    );
    // Lost access: a valid grant does not survive an unusable target.
    f.runtime
        .providers
        .set_disabled("synthetic-source", true)
        .unwrap();
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, Some("op-4".into())),
            &mut tx
        )),
        DenialReason::TargetUnavailable
    );
    f.runtime
        .providers
        .set_disabled("synthetic-source", false)
        .unwrap();
    f.runtime
        .authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, Some("op-5".into())),
            &mut tx,
        )
        .unwrap();
    let revoked = f
        .runtime
        .revoke(&target, &granted.grant.grant_id, Some(&human()), &mut tx)
        .unwrap();
    assert_eq!(revoked.revoked_by.as_deref(), Some("maintainer"));
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::TicketsWrite, None, Some("op-6".into())),
            &mut tx
        )),
        DenialReason::Revoked
    );
    let log = outcomes(&f);
    let expected: Vec<(&str, AuditOutcome, Option<&str>)> = vec![
        (
            "tickets_write",
            AuditOutcome::Denied,
            Some("target_unavailable"),
        ),
        ("tickets_write", AuditOutcome::Denied, Some("no_grant")),
        ("grant_create", AuditOutcome::Granted, None),
        ("tickets_write", AuditOutcome::Allowed, None),
        ("tickets_write", AuditOutcome::Succeeded, None),
        ("tickets_write", AuditOutcome::Denied, Some("no_grant")),
        (
            "work_start",
            AuditOutcome::Denied,
            Some("action_not_granted"),
        ),
        (
            "tickets_write",
            AuditOutcome::Denied,
            Some("target_unavailable"),
        ),
        ("tickets_write", AuditOutcome::Allowed, None),
        ("grant_revoke", AuditOutcome::Revoked, None),
        ("tickets_write", AuditOutcome::Denied, Some("grant_revoked")),
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
fn hs2_sbq3t8_remote_grant_follows_its_reviewed_destination_only() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let target = reference(&f);
    let revision = declaration(&f, &mut tx);
    let remote = register(&f);
    let bound = f
        .runtime
        .bind(request(&f, revision.clone(), remote.clone()), &mut tx)
        .unwrap();
    assert!(bound.usable);
    let mut scoped = create(&[TargetAction::SessionsManage, TargetAction::WorkStart]);
    scoped.session_scope = Some(SessionScope {
        tools: vec![ToolScope {
            tool: "codex".into(),
            models: Vec::new(),
        }],
        max_concurrency: 1,
    });
    let granted = f
        .runtime
        .grant(&target, scoped, Some(&human()), &mut tx)
        .unwrap();
    // HS2-YCJDEB: session management also needs a policy that permits it.
    f.runtime
        .set_policy(
            &f.owner.id,
            SetPolicy {
                expected_revision: None,
                policy: PolicyBody {
                    tools: vec![ToolPolicy {
                        tool: "codex".into(),
                        max_sessions: 2,
                        weight: 50,
                        accounts: Vec::new(),
                    }],
                    targets: vec![TargetPolicy {
                        target_id: id(1),
                        enabled: true,
                        actions: vec![TargetAction::SessionsManage, TargetAction::WorkStart],
                        tools: vec![ToolScope {
                            tool: "codex".into(),
                            models: Vec::new(),
                        }],
                        max_sessions: 2,
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
    let start = |running| SessionRequest {
        tool: "codex".into(),
        model: None,
        running,
        starting: true,
    };
    let calls = f.peer.calls.load(Ordering::SeqCst);
    f.runtime
        .authorize(
            &target,
            Some(&ai("worker-1")),
            act(
                TargetAction::SessionsManage,
                Some(start(0)),
                Some("op-1".into()),
            ),
            &mut tx,
        )
        .unwrap();
    // Authority was confirmed against the peer, not a cached observation.
    assert!(f.peer.calls.load(Ordering::SeqCst) > calls);
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::SessionsManage, Some(start(1)), None),
            &mut tx
        )),
        DenialReason::ConcurrencyExceeded
    );
    // The dry check agrees without contacting the peer or reading a secret.
    let (calls, gets) = (
        f.peer.calls.load(Ordering::SeqCst),
        f.secrets.gets.load(Ordering::SeqCst),
    );
    let decision = f
        .runtime
        .request(
            "POST",
            &format!(
                "/checkouts/{}/orchestration/targets/{}/access",
                f.owner.id,
                id(1)
            ),
            &json!({"actor_id":"worker-1","action":"work_start"}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(decision["allowed"], true);
    assert_eq!(decision["grant_id"], granted.grant.grant_id);
    assert_eq!(f.peer.calls.load(Ordering::SeqCst), calls);
    assert_eq!(f.secrets.gets.load(Ordering::SeqCst), gets);
    // Lost remote credentials: fail closed even though the grant is still active.
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
            Some(&ai("worker-1")),
            act(TargetAction::WorkStart, None, None),
            &mut tx
        )),
        DenialReason::TargetUnavailable
    );
    *f.secrets
        .values
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = saved;
    f.runtime
        .authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::WorkStart, None, None),
            &mut tx,
        )
        .unwrap();
    // Reviewing the target onto a different destination leaves the grant behind.
    let projection = f.runtime.checkout_projection(&f.target.id).unwrap();
    let local = TargetDestination::Local {
        server_id: projection.server_id,
        checkout_id: projection.checkout_id,
        checkout_authority: projection.checkout_authority,
    };
    let current = f
        .runtime
        .resolve(&target, &mut tx)
        .unwrap()
        .binding
        .revision;
    let rebound = f
        .runtime
        .bind(
            BindTarget {
                target: target.clone(),
                expected_manifest_revision: revision,
                expected_binding_revision: current,
                previous_destination: Some(remote),
                destination: local,
            },
            &mut tx,
        )
        .unwrap();
    assert!(rebound.usable);
    assert_eq!(
        denied(f.runtime.authorize(
            &target,
            Some(&ai("worker-1")),
            act(TargetAction::WorkStart, None, None),
            &mut tx
        )),
        DenialReason::DestinationChanged
    );
    assert_eq!(
        f.runtime.grants(&target, &mut tx).unwrap()[0].state,
        GrantState::DestinationChanged
    );
}

#[test]
fn hs2_sbq3t8_routes_carry_the_actor_and_revoke_works_for_an_orphaned_owner() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_local(&f, &mut tx);
    let prefix = format!("/checkouts/{}/orchestration/targets/{}", f.owner.id, id(1));
    let body = json!({"actor_id":"worker-1","actions":["tickets_read"],"expires_at_ms":10_000,
        "grant_id":id(55)});
    let mut by_ai = body.clone();
    by_ai["actor"] = json!({"role":"ai","id":"worker-1"});
    let refused = f
        .runtime
        .request("POST", &format!("{prefix}/grants"), &by_ai, &mut tx)
        .unwrap_err();
    assert_eq!(refused.code(), "forbidden");
    assert_eq!(error_status(&refused), 403);
    for invalid in [json!({"role":"robot"}), json!({"role":"human","id":7})] {
        let mut bad = body.clone();
        bad["actor"] = invalid;
        assert!(matches!(
            f.runtime
                .request("POST", &format!("{prefix}/grants"), &bad, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    let mut by_human = body.clone();
    by_human["actor"] = json!({"role":"human","id":"maintainer"});
    let created = f
        .runtime
        .request("POST", &format!("{prefix}/grants"), &by_human, &mut tx)
        .unwrap();
    assert_eq!(created["state"], "active");
    assert_eq!(created["created_by"], "maintainer");
    // A retry with the retained id is a no-op and is not audited twice. An unspecified
    // actor is not assumed to be an AI.
    let retried = f
        .runtime
        .request("POST", &format!("{prefix}/grants"), &body, &mut tx)
        .unwrap();
    assert_eq!(retried["grant_id"], created["grant_id"]);
    let listed = f
        .runtime
        .request("GET", &format!("{prefix}/grants"), &Value::Null, &mut tx)
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let denied = f
        .runtime
        .request(
            "POST",
            &format!("{prefix}/access"),
            &json!({"actor_id":"worker-1","action":"tickets_write"}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        denied,
        json!({"allowed":false,"reason":"action_not_granted"})
    );
    let audit = format!("/checkouts/{}/orchestration/audit", f.owner.id);
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
    assert_eq!(page["entries"][0]["action"], "grant_create");
    assert_eq!(page["entries"][0]["actor_role"], "human");
    assert_eq!(page["more"], false);
    for bad in ["?after=x", "?limit=-1", "?after=1&after=2"] {
        assert!(matches!(
            f.runtime
                .request("GET", &format!("{audit}{bad}"), &Value::Null, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    for path in [
        format!("{prefix}/grants"),
        format!("{prefix}/grants/{}", id(55)),
        format!("{prefix}/access"),
        audit.clone(),
        format!("{audit}?after=3"),
    ] {
        assert!(is_path(&path), "{path}");
        assert!(is_machine_path(&path), "{path}");
        assert!(!is_cleanup_request("DELETE", &path), "{path}");
    }
    // The owner's registration, manifest and provider configuration are gone; authority
    // can still be withdrawn and the journal still read.
    std::fs::remove_file(f.runtime.checkouts.path()).unwrap();
    std::fs::write(
        Path::new(&f.owner.root).join(".hotsheet2/orchestration.json"),
        b"corrupt",
    )
    .unwrap();
    std::fs::write(f.runtime.providers.path(), b"corrupt").unwrap();
    let revoked = f
        .runtime
        .request(
            "DELETE",
            &format!("{prefix}/grants/{}", id(55)),
            &json!({"actor":{"role":"human","id":"maintainer"}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(revoked["revoked_by"], "maintainer");
    let again = f
        .runtime
        .request(
            "DELETE",
            &format!("{prefix}/grants/{}", id(55)),
            &json!({}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(again, revoked);
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
    assert_eq!(actions, ["grant_create", "grant_revoke"]);
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
    f.runtime
        .grant(
            &reference(&f),
            create(&[TargetAction::GatesObserve]),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let race = |f: &Fixture, rounds: usize| {
        // Each armed observation is interrupted by one complete concurrent resolve.
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
    };
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
