//! HS2-95SWQ4 idle memory: the fold as a table, sequences across listings, and the
//! store. Synthetic homes and ids only.

use std::collections::BTreeMap;

use super::*;

const OWNER: &str = "owner-0123456789ab";
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn memory(entries: &[(u128, u64)]) -> BTreeMap<String, u64> {
    entries.iter().map(|(n, at)| (id(*n), *at)).collect()
}
fn seen(entries: &[(u128, Option<bool>)]) -> Vec<(String, Option<bool>)> {
    entries.iter().map(|(n, idle)| (id(*n), *idle)).collect()
}

#[test]
fn hs2_95swq4_every_observation_of_a_known_and_an_unknown_session() {
    // (remembered, observed) -> remembered afterwards, at time 500.
    for (before, observed, after) in [
        (None, Some(true), Some(500)),
        (None, Some(false), None),
        (None, None, None),
        (Some(100), Some(true), Some(100)),
        (Some(100), Some(false), None),
        (Some(100), None, Some(100)),
    ] {
        let known = before.map_or_else(BTreeMap::new, |at| memory(&[(1, at)]));
        let result = observe_idle(&known, &seen(&[(1, observed)]), 500);
        assert_eq!(
            result.get(&id(1)).copied(),
            after,
            "{before:?} {observed:?}"
        );
    }
    // A session the listing no longer names is finished and forgotten.
    assert!(observe_idle(&memory(&[(1, 100)]), &[], 500).is_empty());
    // Sessions are independent, and one named twice keeps the earlier time.
    let result = observe_idle(
        &memory(&[(1, 100), (2, 200)]),
        &seen(&[
            (1, Some(false)),
            (2, Some(true)),
            (3, Some(true)),
            (3, None),
        ]),
        500,
    );
    assert_eq!(result, memory(&[(2, 200), (3, 500)]));
}

#[test]
fn hs2_95swq4_idle_time_restarts_after_work_and_survives_an_unreachable_host() {
    let mut known = BTreeMap::new();
    let mut step = |observed: Option<bool>, now: u64| {
        known = observe_idle(&known, &seen(&[(1, observed)]), now);
        known.get(&id(1)).copied()
    };
    // Idle, repeated: the first sighting stands.
    assert_eq!(step(Some(true), 10), Some(10));
    assert_eq!(step(Some(true), 20), Some(10));
    // The host cannot be asked, twice: nothing is concluded either way.
    assert_eq!(step(None, 30), Some(10));
    assert_eq!(step(None, 40), Some(10));
    // It works, then idles again: the idle time starts afresh.
    assert_eq!(step(Some(false), 50), None);
    assert_eq!(step(None, 60), None);
    assert_eq!(step(Some(true), 70), Some(70));
    // Busy and idle interleaved never reach back to an earlier idle time.
    assert_eq!(step(Some(false), 80), None);
    assert_eq!(step(Some(true), 90), Some(90));
    assert_eq!(step(Some(false), 100), None);
    assert_eq!(step(Some(false), 110), None);
    assert_eq!(step(Some(true), 120), Some(120));
}

#[test]
fn hs2_95swq4_store_keeps_owners_apart_and_empties_then_refills() {
    let home = tempfile::tempdir().unwrap();
    let store = SessionIdleStore::new(home.path());
    let file = home.path().join("orchestration-session-idle.json");
    // A read, and a listing with nothing to remember, create nothing.
    assert!(store.idle_since(OWNER).unwrap().is_empty());
    assert!(
        store
            .observe(OWNER, &seen(&[(1, Some(false))]), 5)
            .unwrap()
            .is_empty()
    );
    assert!(!file.exists());

    let first = store.observe(OWNER, &seen(&[(1, Some(true))]), 10).unwrap();
    assert_eq!(first, memory(&[(1, 10)]));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Another owner's listing does not touch this owner's memory.
    let other = "other-0123456789ab";
    store.observe(other, &seen(&[(1, Some(true))]), 20).unwrap();
    assert_eq!(store.idle_since(OWNER).unwrap(), memory(&[(1, 10)]));
    assert_eq!(store.idle_since(other).unwrap(), memory(&[(1, 20)]));
    // An unchanged listing writes nothing.
    let written = std::fs::metadata(&file).unwrap().modified().unwrap();
    store.observe(OWNER, &seen(&[(1, Some(true))]), 30).unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        written
    );
    // Emptied, then refilled: the new idle time is the new sighting.
    assert!(store.observe(OWNER, &[], 40).unwrap().is_empty());
    assert!(store.idle_since(OWNER).unwrap().is_empty());
    assert_eq!(store.idle_since(other).unwrap(), memory(&[(1, 20)]));
    assert_eq!(
        store.observe(OWNER, &seen(&[(1, Some(true))]), 50).unwrap(),
        memory(&[(1, 50)])
    );

    // Identifiers only, and bounded.
    assert!(store.observe("bad owner", &[], 1).is_err());
    assert!(
        store
            .observe(OWNER, &[("not-a-ulid".into(), Some(true))], 1)
            .is_err()
    );
    // Damaged or newer data is an error and is never overwritten.
    std::fs::write(&file, br#"{"schema_version":2,"owners":{}}"#).unwrap();
    assert!(matches!(
        store.observe(OWNER, &seen(&[(1, Some(true))]), 60),
        Err(OrchestrationError::UnsupportedVersion { found: 2 })
    ));
    std::fs::write(&file, b"{").unwrap();
    assert!(matches!(
        store.idle_since(OWNER),
        Err(OrchestrationError::Corrupt)
    ));
    assert_eq!(std::fs::read(&file).unwrap(), b"{");
}

#[test]
fn hs2_95swq4_a_host_knows_which_terminals_an_orchestrator_started() {
    struct Clock;
    impl crate::Clock for Clock {
        fn now_ms(&self) -> u64 {
            10
        }
    }
    struct Rng(u128);
    impl crate::Rng for Rng {
        fn next_u128(&mut self) -> u128 {
            self.0 += 1;
            self.0
        }
    }
    let home = tempfile::tempdir().unwrap();
    let registry = HostedSessionRegistry::new(home.path());
    // Nothing recorded: no terminal is an orchestrator's, and no file appears.
    assert!(!registry.hosts(&id(1)).unwrap());
    assert!(
        !home
            .path()
            .join("orchestration-hosted-sessions.json")
            .exists()
    );
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let request = NewHostedSession {
        account: None,
        operation_id: "op-1".into(),
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
    };
    let (reserved, _) = registry
        .reserve("target-0123456789ab", &request, &mut tx)
        .unwrap();
    assert!(registry.hosts(&reserved.session_id).unwrap());
    assert!(!registry.hosts(&id(2)).unwrap());
    // A released reservation is no longer one.
    registry.release("target-0123456789ab", "op-1").unwrap();
    assert!(!registry.hosts(&reserved.session_id).unwrap());
}
