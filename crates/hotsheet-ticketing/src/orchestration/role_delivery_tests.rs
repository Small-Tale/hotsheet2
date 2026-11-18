//! HS2-2DPGDY: the role delivery's transitions, bounds and sequences.
use super::*;

const LAUNCH: u64 = 1_000;

fn delivery(prompt: Option<RolePrompt>, attempts: u32) -> RoleDelivery {
    RoleDelivery {
        prompt,
        attempts,
        delivered_at_ms: None,
    }
}

#[test]
fn hs2_2dpgdy_every_attempt_from_every_state() {
    use PushAttempt as A;
    use RolePrompt as P;
    let soon = LAUNCH + 1;
    let pending = delivery(Some(P::Pending), 1);
    // (before, attempt, expected prompt after)
    let table = [
        (RoleDelivery::default(), A::Delivered, P::Delivered),
        (RoleDelivery::default(), A::NotReady, P::Pending),
        (RoleDelivery::default(), A::Unsupported, P::Unsupported),
        (RoleDelivery::default(), A::Rejected, P::Failed),
        (pending, A::Delivered, P::Delivered),
        (pending, A::NotReady, P::Pending),
        (pending, A::Unsupported, P::Unsupported),
        (pending, A::Rejected, P::Failed),
    ];
    for (before, attempt, expected) in table {
        let after = before.after(attempt, LAUNCH, soon);
        assert_eq!(after.prompt, Some(expected), "{before:?} {attempt:?}");
        assert_eq!(after.attempts, before.attempts + 1);
        assert_eq!(
            after.delivered_at_ms,
            (expected == P::Delivered).then_some(soon)
        );
    }
    // A settled delivery never changes, whatever arrives later, in any order.
    for settled in [P::Delivered, P::AtLaunch, P::Unsupported, P::Failed] {
        let before = delivery(Some(settled), 2);
        for attempt in [A::Delivered, A::NotReady, A::Unsupported, A::Rejected] {
            assert_eq!(before.after(attempt, LAUNCH, soon), before);
        }
        assert!(!before.should_retry(LAUNCH, soon));
        assert!(settled.is_settled());
    }
    assert!(!P::Pending.is_settled());
    assert_eq!(
        P::ALL.map(P::code),
        ["delivered", "at_launch", "pending", "unsupported", "failed"]
    );
}

#[test]
fn hs2_2dpgdy_a_pending_prompt_is_bounded_in_tries_and_in_time() {
    // Not ready three times, then ready: delivered on the fourth try.
    let mut d = RoleDelivery::default();
    for n in 1..MAX_ROLE_ATTEMPTS {
        d = d.after(PushAttempt::NotReady, LAUNCH, LAUNCH + u64::from(n));
        assert_eq!(d.prompt, Some(RolePrompt::Pending));
        assert!(d.should_retry(LAUNCH, LAUNCH + u64::from(n)));
    }
    let delivered = d.after(PushAttempt::Delivered, LAUNCH, LAUNCH + 10);
    assert_eq!(delivered.prompt, Some(RolePrompt::Delivered));
    // Not ready a fourth time instead: failed, and nothing is tried again.
    let exhausted = d.after(PushAttempt::NotReady, LAUNCH, LAUNCH + 10);
    assert_eq!(exhausted.prompt, Some(RolePrompt::Failed));
    assert_eq!(exhausted.attempts, MAX_ROLE_ATTEMPTS);
    assert!(!exhausted.should_retry(LAUNCH, LAUNCH + 11));

    // A pending prompt whose window passed reads as failed without being touched...
    let pending = delivery(Some(RolePrompt::Pending), 1);
    let late = LAUNCH + ROLE_PROMPT_WINDOW_MS;
    assert_eq!(pending.at(LAUNCH, late - 1), Some(RolePrompt::Pending));
    assert_eq!(pending.at(LAUNCH, late), Some(RolePrompt::Failed));
    assert!(pending.should_retry(LAUNCH, late - 1));
    assert!(!pending.should_retry(LAUNCH, late));
    // ...and a try that arrives after it settles it as failed even when not ready.
    assert_eq!(
        pending.after(PushAttempt::NotReady, LAUNCH, late).prompt,
        Some(RolePrompt::Failed)
    );
    // A clock before the launch is not past the window.
    assert_eq!(pending.at(LAUNCH, 0), Some(RolePrompt::Pending));
    // Nothing asked yet reads as nothing.
    assert_eq!(RoleDelivery::default().at(LAUNCH, late), None);
    assert!(RoleDelivery::default().is_unasked());
}

#[test]
fn hs2_2dpgdy_a_prompt_at_launch_is_settled_and_survives_the_record() {
    let at_launch = RoleDelivery::at_launch(LAUNCH);
    assert_eq!(at_launch.prompt, Some(RolePrompt::AtLaunch));
    assert_eq!(at_launch.delivered_at_ms, Some(LAUNCH));
    assert!(!at_launch.should_retry(LAUNCH, LAUNCH + 1));
    // Serialized: an unasked delivery and every settled one round-trip.
    for value in [
        RoleDelivery::default(),
        at_launch,
        delivery(Some(RolePrompt::Pending), 2),
    ] {
        let text = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<RoleDelivery>(&text).unwrap(), value);
    }
    assert_eq!(
        serde_json::to_string(&RolePrompt::AtLaunch).unwrap(),
        "\"at_launch\""
    );
}

/// HS2-4R5MNJ: a prompt the host holds until the tool can take it (a Codex TUI behind its
/// first-run dialogs) stays pending without using up a try, however many readiness events
/// find it still held; the window still bounds it, and the paste settles it delivered.
#[test]
fn hs2_4r5mnj_a_held_prompt_waits_without_using_tries_and_settles_when_pasted() {
    let mut held = RoleDelivery::default();
    for minute in 0..(u64::from(MAX_ROLE_ATTEMPTS) * 3) {
        held = held.after(PushAttempt::Held, LAUNCH, LAUNCH + minute * 1_000);
        assert_eq!(held.prompt, Some(RolePrompt::Pending));
        assert_eq!(held.attempts, 0);
        assert!(held.should_retry(LAUNCH, LAUNCH + minute * 1_000));
    }
    let delivered = held.after(PushAttempt::Delivered, LAUNCH, LAUNCH + 60_000);
    assert_eq!(delivered.prompt, Some(RolePrompt::Delivered));
    assert_eq!(delivered.delivered_at_ms, Some(LAUNCH + 60_000));
    // Never ready within the window: failed, and nothing more is tried.
    let late = LAUNCH + ROLE_PROMPT_WINDOW_MS;
    assert_eq!(held.at(LAUNCH, late), Some(RolePrompt::Failed));
    assert_eq!(
        held.after(PushAttempt::Held, LAUNCH, late).prompt,
        Some(RolePrompt::Failed)
    );
    // A held try after real failed tries keeps their count.
    let tried = RoleDelivery::default()
        .after(PushAttempt::NotReady, LAUNCH, LAUNCH)
        .after(PushAttempt::Held, LAUNCH, LAUNCH + 1);
    assert_eq!(
        (tried.prompt, tried.attempts),
        (Some(RolePrompt::Pending), 1)
    );
}
