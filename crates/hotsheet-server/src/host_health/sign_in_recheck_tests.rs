//! HS2-P2TCQS: the held sign-in re-check's backoff and the on-demand debounce.
use super::*;

const MINUTE: Duration = Duration::from_secs(60);
const SECOND: Duration = Duration::from_secs(1);

/// Held reads every five seconds from `now` until one is due for the reading taken at
/// `taken`; the re-check then runs and reads `still_out`. Returns when it ran.
fn held_recheck(backoff: &mut SignInBackoff, now: &mut Instant, taken: Instant, still_out: bool) {
    loop {
        *now += 5 * SECOND;
        if backoff.due(*now, taken, true, Some(SignInRecheck::Held)) {
            backoff.checked(still_out, Some(SignInRecheck::Held));
            return;
        }
    }
}

/// The held schedule a machine follows while a tool stays signed out: each re-check is
/// due once the reading is as old as the step, and each one that still reads signed out
/// doubles the step up to ten minutes.
#[test]
fn hs2_p2tcqs_held_rechecks_back_off_from_one_minute_to_ten() {
    let mut backoff = SignInBackoff::default();
    let start = Instant::now();
    let mut taken = start;
    let mut now = start;
    let mut steps = Vec::new();
    for _ in 0..7 {
        // Asked every few seconds while the hold lasts; due exactly at the step.
        let step = loop {
            now += 5 * SECOND;
            if backoff.due(now, taken, true, Some(SignInRecheck::Held)) {
                break now.duration_since(taken);
            }
        };
        steps.push(step.as_secs().div_ceil(5) * 5);
        backoff.checked(true, Some(SignInRecheck::Held));
        taken = now;
    }
    assert_eq!(steps, [60, 120, 240, 480, 600, 600, 600]);
}

#[test]
fn hs2_p2tcqs_held_rechecks_start_only_while_a_tool_reads_signed_out() {
    let mut backoff = SignInBackoff::default();
    let taken = Instant::now();
    let late = taken + 20 * MINUTE;
    // Signed in or unknown: never re-checked, and no step starts.
    assert!(!backoff.due(late, taken, false, Some(SignInRecheck::Held)));
    assert_eq!(backoff.step(), None);
    // Nothing held (a read that asks nothing): never, signed out or not.
    assert!(!backoff.due(late, taken, true, None));
    assert_eq!(backoff.step(), None);
    // Held and signed out: the first step starts.
    assert!(!backoff.due(taken + 30 * SECOND, taken, true, Some(SignInRecheck::Held)));
    assert_eq!(backoff.step(), Some(MINUTE));
}

#[test]
fn hs2_p2tcqs_a_sign_in_ends_the_backoff_and_a_later_sign_out_starts_over() {
    let mut backoff = SignInBackoff::default();
    let mut now = Instant::now();
    for _ in 0..3 {
        let taken = now;
        held_recheck(&mut backoff, &mut now, taken, true);
    }
    assert_eq!(backoff.step(), Some(8 * MINUTE));
    // Signed in at the next re-check: the step ends.
    let taken = now;
    held_recheck(&mut backoff, &mut now, taken, false);
    assert_eq!(backoff.step(), None);
    // Signed out again later, held again: one minute first.
    let taken = now + 30 * MINUTE;
    assert!(!backoff.due(taken + 5 * SECOND, taken, true, Some(SignInRecheck::Held)));
    assert_eq!(backoff.step(), Some(MINUTE));
    assert!(backoff.due(taken + MINUTE, taken, true, Some(SignInRecheck::Held)));
}

/// A released hold asks nothing, so nothing is re-checked; held again after a gap
/// longer than its step, the backoff starts over at the first step.
#[test]
fn hs2_p2tcqs_a_released_hold_stops_and_a_new_hold_starts_over() {
    let mut backoff = SignInBackoff::default();
    let mut now = Instant::now();
    for _ in 0..2 {
        let taken = now;
        held_recheck(&mut backoff, &mut now, taken, true);
    }
    assert_eq!(backoff.step(), Some(4 * MINUTE));
    let taken = now;
    // Released: reads that ask nothing re-check nothing, however old the reading.
    for minutes in [5, 30, 120] {
        assert!(!backoff.due(taken + minutes * MINUTE, taken, true, None));
    }
    // Held again two hours on, against a reading a moment old: the first step
    // applies, not four minutes.
    let fresh = taken + 120 * MINUTE;
    assert!(!backoff.due(fresh + 30 * SECOND, fresh, true, Some(SignInRecheck::Held)));
    assert_eq!(backoff.step(), Some(MINUTE));
    assert!(backoff.due(fresh + MINUTE, fresh, true, Some(SignInRecheck::Held)));
}

#[test]
fn hs2_p2tcqs_an_on_demand_recheck_is_debounced() {
    let mut backoff = SignInBackoff::default();
    let taken = Instant::now();
    // A reading a moment old is not detected again, however often a person asks.
    for seconds in [0, 1, 5, 14] {
        assert!(!backoff.due(
            taken + seconds * SECOND,
            taken,
            true,
            Some(SignInRecheck::Now)
        ));
    }
    assert!(backoff.due(taken + 15 * SECOND, taken, true, Some(SignInRecheck::Now)));
    // Nothing signed out: nothing to re-check.
    assert!(!backoff.due(taken + MINUTE, taken, false, Some(SignInRecheck::Now)));
    // An on-demand re-check leaves the held step alone.
    backoff.checked(true, Some(SignInRecheck::Now));
    assert_eq!(backoff.step(), None);
}

#[test]
fn hs2_p2tcqs_interleaved_held_and_on_demand_rechecks() {
    let mut backoff = SignInBackoff::default();
    let mut now = Instant::now();
    // Held: the first re-check at one minute; still signed out, the step is two.
    let taken = now;
    held_recheck(&mut backoff, &mut now, taken, true);
    assert_eq!(now - taken, MINUTE);
    assert_eq!(backoff.step(), Some(2 * MINUTE));
    // A person opens Systems thirty seconds on: re-checked at once.
    let taken = now;
    now += 30 * SECOND;
    assert!(!backoff.due(now, taken, true, Some(SignInRecheck::Held)));
    assert!(backoff.due(now, taken, true, Some(SignInRecheck::Now)));
    backoff.checked(true, Some(SignInRecheck::Now));
    // The held step was not grown by it, and counts from that reading.
    assert_eq!(backoff.step(), Some(2 * MINUTE));
    let taken = now;
    held_recheck(&mut backoff, &mut now, taken, true);
    assert_eq!(now - taken, 2 * MINUTE);
    assert_eq!(backoff.step(), Some(4 * MINUTE));
}

#[test]
fn hs2_p2tcqs_steps_are_configurable_and_bounded() {
    let mut backoff = SignInBackoff::new(SECOND, 3 * SECOND, Duration::ZERO);
    let mut taken = Instant::now();
    let mut seen = Vec::new();
    for _ in 0..4 {
        let step = backoff.step().unwrap_or(SECOND);
        assert!(backoff.due(taken + step, taken, true, Some(SignInRecheck::Held)));
        seen.push(backoff.step().unwrap());
        backoff.checked(true, Some(SignInRecheck::Held));
        taken += step;
    }
    assert_eq!(seen, [SECOND, 2 * SECOND, 3 * SECOND, 3 * SECOND]);
    // A longest step shorter than the first is the first.
    assert_eq!(
        SignInBackoff::new(MINUTE, SECOND, SECOND).max,
        MINUTE,
        "the longest step is never shorter than the first"
    );
}
