//! HS2-WHR4GS: fences of withdrawn session operations, pure and through the hosted
//! registry, with repeated, out-of-order and interleaved start and stop sequences.
//! Synthetic homes only.

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
const HOSTED: &str = "target-0123456789ab";
const OTHER: &str = "other-0123456789ab";

fn hosted(operation: &str) -> NewHostedSession {
    NewHostedSession {
        operation_id: operation.into(),
        tool: "codex".into(),
        model: None,
        effort: None,
        role: SessionRole::Worker,
        origin: SessionOrigin {
            owner_checkout_id: OWNER.into(),
            project: "Release train".into(),
            run_id: None,
            ticket: None,
        },
    }
}

fn withdrawn(result: Result<impl Sized, OrchestrationError>) -> bool {
    matches!(
        result,
        Err(OrchestrationError::Session(SessionError::Withdrawn))
    )
}

#[test]
fn hs2_whr4gs_a_fence_holds_for_its_bound_and_then_expires() {
    let mut fences = WithdrawnFences::default();
    // Empty: nothing is fenced and pruning changes nothing.
    assert!(!fences.is_fenced(HOSTED, "op-1", 0));
    assert!(!fences.prune(0));
    fences.fence(HOSTED, "op-1", 1_000).unwrap();
    // (time, fenced) across the bound, including a clock that went back.
    for (now, expected) in [
        (0, true),
        (999, true),
        (1_000, true),
        (1_000 + WITHDRAWN_FENCE_MS - 1, true),
        (1_000 + WITHDRAWN_FENCE_MS, false),
        (u64::MAX, false),
    ] {
        assert_eq!(fences.is_fenced(HOSTED, "op-1", now), expected, "{now}");
    }
    // Another checkout's identical operation, or another operation, is not fenced.
    assert!(!fences.is_fenced(OTHER, "op-1", 1_000));
    assert!(!fences.is_fenced(HOSTED, "op-2", 1_000));
    // Fencing again restarts the time; it never shortens a fence or doubles it.
    fences
        .fence(HOSTED, "op-1", 1_000 + WITHDRAWN_FENCE_MS - 1)
        .unwrap();
    assert_eq!(fences.withdrawn.len(), 1);
    assert!(fences.is_fenced(HOSTED, "op-1", 1_000 + WITHDRAWN_FENCE_MS));
    // Pruning drops only expired fences; empty again, then refilled.
    assert!(fences.prune(1_000 + 3 * WITHDRAWN_FENCE_MS));
    assert!(fences.withdrawn.is_empty());
    fences.fence(OTHER, "op-9", 5).unwrap();
    assert!(fences.is_fenced(OTHER, "op-9", 5));
    // Malformed identifiers are refused and change nothing.
    assert!(fences.fence("", "op-1", 5).is_err());
    assert!(fences.fence(HOSTED, "has space", 5).is_err());
    assert_eq!(fences.withdrawn.len(), 1);
}

#[test]
fn hs2_whr4gs_fences_are_bounded_and_the_oldest_goes_first() {
    let mut fences = WithdrawnFences::default();
    for n in 0..MAX_WITHDRAWN_FENCES {
        fences.fence(HOSTED, &format!("op-{n}"), 10).unwrap();
    }
    assert_eq!(fences.withdrawn.len(), MAX_WITHDRAWN_FENCES);
    fences.fence(HOSTED, "op-new", 11).unwrap();
    assert_eq!(fences.withdrawn.len(), MAX_WITHDRAWN_FENCES);
    assert!(!fences.is_fenced(HOSTED, "op-0", 11));
    assert!(fences.is_fenced(HOSTED, "op-1", 11));
    assert!(fences.is_fenced(HOSTED, "op-new", 11));
    // Expired fences go before any standing one is evicted.
    fences
        .fence(HOSTED, "op-later", 10 + WITHDRAWN_FENCE_MS)
        .unwrap();
    assert_eq!(fences.withdrawn.len(), 2);
}

#[test]
fn hs2_whr4gs_a_stop_before_its_start_fences_the_operation() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedSessionRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(1_000));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let fence_file = home.path().join("orchestration-withdrawn-sessions.json");
    assert!(registry.withdrawn(1_000).unwrap().is_empty());
    assert!(!fence_file.exists());
    // Out of order: the stop arrives first and finds nothing; it fences.
    assert_eq!(
        registry.withdraw(HOSTED, "op-1", 1_000).unwrap(),
        Withdrawal::Unknown
    );
    assert!(fence_file.exists());
    // Repeated: a second stop fences again and is still unknown.
    clock.0.set(2_000);
    assert_eq!(
        registry.withdraw(HOSTED, "op-1", 2_000).unwrap(),
        Withdrawal::Unknown
    );
    assert_eq!(registry.withdrawn(2_000).unwrap().len(), 1);
    // The late start launches nothing, however often it arrives.
    for _ in 0..3 {
        assert!(withdrawn(registry.reserve(
            HOSTED,
            &hosted("op-1"),
            &mut tx
        )));
    }
    assert!(registry.list(HOSTED).unwrap().is_empty());
    // Other operations and other checkouts start normally meanwhile.
    let (other, created) = registry.reserve(HOSTED, &hosted("op-2"), &mut tx).unwrap();
    assert!(created && !other.started);
    assert!(registry.reserve(OTHER, &hosted("op-1"), &mut tx).is_ok());
    // After the bound the operation id is usable again.
    clock.0.set(2_000 + WITHDRAWN_FENCE_MS);
    let (late, created) = registry.reserve(HOSTED, &hosted("op-1"), &mut tx).unwrap();
    assert!(created && !late.started);
    assert!(registry.withdrawn(clock.0.get()).unwrap().is_empty());
}

#[test]
fn hs2_whr4gs_a_stop_releases_an_unlaunched_reservation_and_fences_it() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedSessionRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(1_000));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    // Interleaved: the start reserved, the stop arrives before the launch.
    let (reserved, _) = registry.reserve(HOSTED, &hosted("op-1"), &mut tx).unwrap();
    assert_eq!(
        registry.withdraw(HOSTED, "op-1", 1_000).unwrap(),
        Withdrawal::Released
    );
    assert!(registry.get(HOSTED, "op-1").unwrap().is_none());
    // The start's launch finds no record to mark: its caller stops the terminal.
    assert_eq!(registry.mark_started(HOSTED, "op-1").unwrap(), None);
    assert!(!registry.hosts(&reserved.session_id).unwrap());
    // A retry of that start is refused, and a repeated stop is unknown and fenced.
    assert!(withdrawn(registry.reserve(
        HOSTED,
        &hosted("op-1"),
        &mut tx
    )));
    assert_eq!(
        registry.withdraw(HOSTED, "op-1", 1_500).unwrap(),
        Withdrawal::Unknown
    );
}

#[test]
fn hs2_whr4gs_a_launched_session_is_never_fenced() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedSessionRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(1_000));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let (reserved, _) = registry.reserve(HOSTED, &hosted("op-1"), &mut tx).unwrap();
    let started = registry.mark_started(HOSTED, "op-1").unwrap().unwrap();
    assert!(started.started);
    // The stop goes on as for any launched session; nothing is fenced or removed.
    match registry.withdraw(HOSTED, "op-1", 1_000).unwrap() {
        Withdrawal::Launched(session) => assert_eq!(session.session_id, reserved.session_id),
        other => panic!("expected a launched session, got {other:?}"),
    }
    assert!(registry.withdrawn(1_000).unwrap().is_empty());
    // The same start again is the same session, not a refusal.
    let (again, created) = registry.reserve(HOSTED, &hosted("op-1"), &mut tx).unwrap();
    assert!(!created && again.started);
    // Once ended, a stop still leaves it alone.
    registry.end(HOSTED, "op-1", "wound_down", 1_100).unwrap();
    assert!(matches!(
        registry.withdraw(HOSTED, "op-1", 1_200).unwrap(),
        Withdrawal::Launched(_)
    ));
    assert!(registry.withdrawn(1_200).unwrap().is_empty());
}

#[test]
fn hs2_whr4gs_older_files_read_and_damaged_fences_fail_closed() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedSessionRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(1_000));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    // A host that never fenced anything has no fence file; the sessions file it
    // wrote is untouched by fencing and reads as before.
    registry.reserve(HOSTED, &hosted("op-1"), &mut tx).unwrap();
    let sessions = home.path().join("orchestration-hosted-sessions.json");
    let before = std::fs::read(&sessions).unwrap();
    registry.withdraw(HOSTED, "op-2", 1_000).unwrap();
    assert_eq!(std::fs::read(&sessions).unwrap(), before);
    let text = String::from_utf8(before).unwrap();
    assert!(!text.contains("withdrawn"));
    // A damaged fence file refuses starts rather than letting one through.
    let fences = home.path().join("orchestration-withdrawn-sessions.json");
    std::fs::write(&fences, b"{").unwrap();
    assert!(matches!(
        registry.reserve(HOSTED, &hosted("op-3"), &mut tx),
        Err(OrchestrationError::Corrupt)
    ));
    std::fs::write(&fences, br#"{"schema_version":9,"withdrawn":[]}"#).unwrap();
    assert!(matches!(
        registry.reserve(HOSTED, &hosted("op-3"), &mut tx),
        Err(OrchestrationError::UnsupportedVersion { found: 9 })
    ));
    std::fs::write(
        &fences,
        br#"{"schema_version":1,"withdrawn":[{"checkout_id":"a b","operation_id":"x","withdrawn_at_ms":1}]}"#,
    )
    .unwrap();
    assert!(matches!(
        registry.withdrawn(1_000),
        Err(OrchestrationError::Corrupt)
    ));
    // The session error has a stable code.
    assert_eq!(SessionError::Withdrawn.code(), "session_withdrawn");
    assert_eq!(
        SessionError::from_code("session_withdrawn"),
        Some(SessionError::Withdrawn)
    );
}
