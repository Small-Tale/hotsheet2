//! HS2-T16SK0: who assigns a queued run's ticket, and when its session claims it for
//! itself. Pure table and sequence tests.
use super::*;

const MINUTE: u64 = 60_000;

fn input(dispatcher: Option<bool>, integrator: Option<bool>, now_ms: u64) -> AssignmentInput {
    AssignmentInput {
        dispatcher_live: dispatcher,
        integrator_live: integrator,
        queued_at_ms: 10 * MINUTE,
        wait_ms: 2 * MINUTE,
        backoff_until_ms: None,
        now_ms,
    }
}

#[test]
fn hs2_t16sk0_every_pool_reading_decides_exactly() {
    use AssignmentState::{AwaitingDispatcher, NoLiveDispatcher, PoolUnknown};
    let (before, at, after) = (11 * MINUTE, 12 * MINUTE, 13 * MINUTE);
    // (dispatcher, integrator, now, state, claim, retry)
    let table = [
        (
            Some(true),
            Some(false),
            after,
            AwaitingDispatcher,
            false,
            None,
        ),
        (
            Some(false),
            Some(true),
            after,
            AwaitingDispatcher,
            false,
            None,
        ),
        (
            Some(true),
            Some(true),
            after,
            AwaitingDispatcher,
            false,
            None,
        ),
        // A live one decides, even beside an unreadable flag.
        (Some(true), None, after, AwaitingDispatcher, false, None),
        (None, Some(true), after, AwaitingDispatcher, false, None),
        (None, None, after, PoolUnknown, false, None),
        (Some(false), None, after, PoolUnknown, false, None),
        (None, Some(false), after, PoolUnknown, false, None),
        (
            Some(false),
            Some(false),
            before,
            NoLiveDispatcher,
            false,
            Some(at),
        ),
        (Some(false), Some(false), at, NoLiveDispatcher, true, None),
        (
            Some(false),
            Some(false),
            after,
            NoLiveDispatcher,
            true,
            None,
        ),
    ];
    for (dispatcher, integrator, now, state, claim, retry) in table {
        let decided = decide_assignment(&input(dispatcher, integrator, now));
        assert_eq!(
            (decided.state, decided.claim, decided.retry_at_ms),
            (state, claim, retry),
            "{dispatcher:?} {integrator:?} {now}"
        );
    }
    for state in [AwaitingDispatcher, NoLiveDispatcher, PoolUnknown] {
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!(state.code())
        );
    }
}

#[test]
fn hs2_t16sk0_a_wait_of_zero_and_a_backoff_move_the_claim_time() {
    let none = |now| AssignmentInput {
        wait_ms: 0,
        ..input(Some(false), Some(false), now)
    };
    // No wait: the run is claimed the moment it is seen queued.
    assert!(decide_assignment(&none(10 * MINUTE)).claim);
    // A failed claim's backoff outlasts the wait: nothing until it ends.
    let backed = |now| AssignmentInput {
        backoff_until_ms: Some(20 * MINUTE),
        ..input(Some(false), Some(false), now)
    };
    let early = decide_assignment(&backed(15 * MINUTE));
    assert_eq!((early.claim, early.retry_at_ms), (false, Some(20 * MINUTE)));
    assert!(decide_assignment(&backed(20 * MINUTE)).claim);
    // A backoff that ended before the wait does not shorten it.
    let past = AssignmentInput {
        backoff_until_ms: Some(5 * MINUTE),
        ..input(Some(false), Some(false), 11 * MINUTE)
    };
    assert_eq!(decide_assignment(&past).retry_at_ms, Some(12 * MINUTE));
    // A clock before the queue time (a skewed reading) waits, never claims early.
    assert_eq!(
        decide_assignment(&input(Some(false), Some(false), 0)).retry_at_ms,
        Some(12 * MINUTE)
    );
}

/// A pool that changes between passes: each pass decides from what it reads then,
/// so a dispatcher that comes up in time prevents the claim, one that leaves
/// brings it back, and a repeated pass decides the same.
#[test]
fn hs2_t16sk0_passes_over_a_changing_pool_claim_only_when_nobody_will_assign() {
    let passes = [
        // (minute, dispatcher, integrator, claim)
        (10, Some(false), Some(false), false),
        (10, Some(false), Some(false), false),
        (11, None, None, false),
        (11, Some(true), Some(false), false),
        (12, Some(true), Some(false), false),
        (12, Some(false), Some(true), false),
        (13, Some(false), Some(false), true),
        (13, Some(false), Some(false), true),
        (14, Some(true), Some(false), false),
    ];
    for (minute, dispatcher, integrator, claim) in passes {
        assert_eq!(
            decide_assignment(&input(dispatcher, integrator, minute * MINUTE)).claim,
            claim,
            "minute {minute}"
        );
    }
}

/// HS2-T16SK0: the policy's wait defaults, validates, and stays out of a stored
/// policy while it is the default.
#[test]
fn hs2_t16sk0_the_dispatcher_wait_defaults_validates_and_is_written_only_when_set() {
    let mut body = PolicyBody::default();
    assert_eq!(
        body.placement.dispatcher_wait_minutes,
        DEFAULT_DISPATCHER_WAIT_MINUTES
    );
    let json = serde_json::to_value(&body).unwrap();
    assert!(json.get("placement").is_none());
    body.placement.automatic = true;
    body.placement.actor = Some("scheduler".into());
    let json = serde_json::to_value(&body).unwrap();
    assert!(json["placement"].get("dispatcher_wait_minutes").is_none());
    body.placement.dispatcher_wait_minutes = 0;
    let json = serde_json::to_value(&body).unwrap();
    assert_eq!(json["placement"]["dispatcher_wait_minutes"], 0);
    assert_eq!(serde_json::from_value::<PolicyBody>(json).unwrap(), body);
    let codes = |minutes: u32| {
        let mut body = body.clone();
        body.placement.dispatcher_wait_minutes = minutes;
        body.problems(None)
            .into_iter()
            .map(|problem| problem.code)
            .filter(|code| code.contains("dispatcher"))
            .collect::<Vec<_>>()
    };
    assert!(codes(0).is_empty());
    assert!(codes(1440).is_empty());
    assert_eq!(codes(1441), ["invalid_dispatcher_wait_minutes"]);
    let kind = serde_json::to_value(DecisionKind::Assignment).unwrap();
    assert_eq!(kind, "assignment");
}
