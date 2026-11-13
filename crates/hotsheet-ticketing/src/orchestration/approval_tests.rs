//! HS2-YCJDEB approval lifecycle; synthetic homes only.

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
fn states(registry: &ApprovalRegistry, now_ms: u64) -> Vec<ApprovalState> {
    registry
        .list(OWNER, now_ms)
        .unwrap()
        .into_iter()
        .map(|approval| approval.state)
        .collect()
}

#[test]
fn hs2_ycjdeb_an_approval_allows_exactly_one_matching_operation() {
    let home = tempfile::tempdir().unwrap();
    let registry = ApprovalRegistry::new(home.path());
    let clock = Clock(1_000.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert!(registry.list(OWNER, 1_000).unwrap().is_empty());
    assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());
    let ask = |tx: &mut Transaction<'_>, actor: &str, gate, operation: &str| {
        registry
            .consume_or_request(&target(1), actor, gate, operation, tx)
            .unwrap()
    };
    // Asking files one pending request; asking again does not add another.
    assert!(!ask(&mut tx, "worker-1", ApprovalGate::WorkStart, "op-1"));
    assert!(!ask(&mut tx, "worker-1", ApprovalGate::WorkStart, "op-1"));
    assert_eq!(states(&registry, 1_000), [ApprovalState::Pending]);
    let pending = registry.list(OWNER, 1_000).unwrap().remove(0);
    assert!(
        !registry
            .approved(
                &target(1),
                "worker-1",
                ApprovalGate::WorkStart,
                "op-1",
                1_000
            )
            .unwrap()
    );
    clock.0.set(2_000);
    let (approved, changed) = registry
        .decide(
            OWNER,
            &pending.approval_id,
            true,
            Some("maintainer".into()),
            &mut tx,
        )
        .unwrap();
    assert!(changed);
    assert_eq!(approved.state, ApprovalState::Approved);
    assert_eq!(approved.decided_by.as_deref(), Some("maintainer"));
    assert_eq!(approved.decided_at_ms, Some(2_000));
    // Repeating the same answer is a no-op; reversing it is refused.
    assert_eq!(
        registry
            .decide(
                OWNER,
                &pending.approval_id,
                true,
                Some("other".into()),
                &mut tx
            )
            .unwrap(),
        (approved, false)
    );
    assert!(matches!(
        registry.decide(OWNER, &pending.approval_id, false, None, &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    // The approval is for this exact actor, gate, target and operation only.
    assert!(
        registry
            .approved(
                &target(1),
                "worker-1",
                ApprovalGate::WorkStart,
                "op-1",
                2_000
            )
            .unwrap()
    );
    assert!(!ask(&mut tx, "worker-2", ApprovalGate::WorkStart, "op-1"));
    assert!(!ask(
        &mut tx,
        "worker-1",
        ApprovalGate::TicketsWrite,
        "op-1"
    ));
    assert!(!ask(&mut tx, "worker-1", ApprovalGate::WorkStart, "op-2"));
    assert!(
        !registry
            .consume_or_request(
                &target(2),
                "worker-1",
                ApprovalGate::WorkStart,
                "op-1",
                &mut tx
            )
            .unwrap()
    );
    // Used once, then a fresh request is needed.
    assert!(ask(&mut tx, "worker-1", ApprovalGate::WorkStart, "op-1"));
    assert!(!ask(&mut tx, "worker-1", ApprovalGate::WorkStart, "op-1"));
    let after = states(&registry, 2_000);
    assert_eq!(after[0], ApprovalState::Consumed);
    assert_eq!(
        after
            .iter()
            .filter(|s| **s == ApprovalState::Pending)
            .count(),
        5
    );
}

#[test]
fn hs2_ycjdeb_denied_and_lapsed_requests_never_authorize() {
    let home = tempfile::tempdir().unwrap();
    let registry = ApprovalRegistry::new(home.path());
    let clock = Clock(1_000.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let ask = |tx: &mut Transaction<'_>, operation: &str| {
        registry
            .consume_or_request(
                &target(1),
                "worker-1",
                ApprovalGate::SessionStart,
                operation,
                tx,
            )
            .unwrap()
    };
    assert!(!ask(&mut tx, "op-denied"));
    let denied = registry.list(OWNER, 1_000).unwrap().remove(0);
    let (answer, changed) = registry
        .decide(OWNER, &denied.approval_id, false, None, &mut tx)
        .unwrap();
    assert!(changed);
    assert_eq!(answer.state, ApprovalState::Denied);
    // A denied operation stays denied however often it asks, and files nothing new.
    assert!(!ask(&mut tx, "op-denied"));
    assert!(!ask(&mut tx, "op-denied"));
    assert_eq!(states(&registry, 1_000), [ApprovalState::Denied]);
    assert!(matches!(
        registry.decide(OWNER, &denied.approval_id, true, None, &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    // An approval that is not used within an hour lapses.
    assert!(!ask(&mut tx, "op-slow"));
    let slow = registry.list(OWNER, 1_000).unwrap().remove(1);
    registry
        .decide(OWNER, &slow.approval_id, true, None, &mut tx)
        .unwrap();
    clock.0.set(1_000 + APPROVAL_VALID_MS - 1);
    assert!(
        registry
            .approved(
                &target(1),
                "worker-1",
                ApprovalGate::SessionStart,
                "op-slow",
                clock.0.get()
            )
            .unwrap()
    );
    clock.0.set(1_000 + APPROVAL_VALID_MS);
    assert!(!ask(&mut tx, "op-slow"));
    let listed = states(&registry, clock.0.get());
    assert_eq!(
        listed,
        [
            ApprovalState::Denied,
            ApprovalState::Expired,
            ApprovalState::Pending
        ]
    );
    // An unanswered request lapses after a day and cannot be answered afterwards.
    let waiting = registry.list(OWNER, clock.0.get()).unwrap().remove(2);
    clock.0.set(clock.0.get() + APPROVAL_PENDING_MS);
    assert_eq!(states(&registry, clock.0.get())[2], ApprovalState::Expired);
    assert!(matches!(
        registry.decide(OWNER, &waiting.approval_id, true, None, &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    // Unknown ids and another owner's requests are not found.
    assert!(matches!(
        registry.decide(
            OWNER,
            &ulid::Ulid::from(999u128).to_string(),
            true,
            None,
            &mut tx
        ),
        Err(OrchestrationError::NotFound)
    ));
    assert!(matches!(
        registry.decide(
            "other-0123456789ab",
            &waiting.approval_id,
            true,
            None,
            &mut tx
        ),
        Err(OrchestrationError::NotFound)
    ));
    assert!(registry.list("other-0123456789ab", 0).unwrap().is_empty());
}

#[test]
fn hs2_ycjdeb_capacity_keeps_waiting_requests_and_drops_finished_ones() {
    let home = tempfile::tempdir().unwrap();
    let registry = ApprovalRegistry::new(home.path());
    let clock = Clock(1_000.into());
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    for index in 0..MAX_APPROVALS {
        registry
            .consume_or_request(
                &target(1),
                "worker-1",
                ApprovalGate::WorkStart,
                &format!("op-{index}"),
                &mut tx,
            )
            .unwrap();
    }
    // Full of waiting requests: a new one is refused rather than dropping any.
    assert!(matches!(
        registry.consume_or_request(
            &target(1),
            "worker-1",
            ApprovalGate::WorkStart,
            "op-overflow",
            &mut tx
        ),
        Err(OrchestrationError::Capacity)
    ));
    let first = registry.list(OWNER, 1_000).unwrap().remove(0);
    registry
        .decide(OWNER, &first.approval_id, false, None, &mut tx)
        .unwrap();
    registry
        .consume_or_request(
            &target(1),
            "worker-1",
            ApprovalGate::WorkStart,
            "op-overflow",
            &mut tx,
        )
        .unwrap();
    let listed = registry.list(OWNER, 1_000).unwrap();
    assert_eq!(listed.len(), MAX_APPROVALS);
    assert!(listed.iter().all(|a| a.state == ApprovalState::Pending));
    assert!(listed.iter().all(|a| a.approval_id != first.approval_id));
    // Corrupt or newer data fails closed and is not overwritten.
    let path = home.path().join("orchestration-approvals.json");
    for bytes in [
        &b"{nope"[..],
        &br#"{"schema_version":2,"approvals":[]}"#[..],
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert!(
            registry
                .consume_or_request(
                    &target(1),
                    "worker-1",
                    ApprovalGate::WorkStart,
                    "op",
                    &mut tx
                )
                .is_err()
        );
        assert!(registry.list(OWNER, 1_000).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
