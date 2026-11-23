//! HS2-NPTKS8: what is done about a session that has not joined its role. Pure table
//! and sequence tests.
use super::*;

const MINUTE: u64 = 60_000;

fn input(joined: bool, prompt: Option<RolePrompt>, replaced: u32, now_ms: u64) -> JoinInput {
    JoinInput {
        joined,
        role_prompt: prompt,
        running_since_ms: 10 * MINUTE,
        join_ms: 10 * MINUTE,
        replaced,
        now_ms,
    }
}

#[test]
fn hs2_nptks8_every_join_reading_decides_exactly() {
    use JoinState::{Joined, NeverJoined, RolePromptFailed, Waiting};
    let (before, at, after) = (15 * MINUTE, 20 * MINUTE, 30 * MINUTE);
    // (joined, prompt, replaced, now) -> (state, wind down, exhausted, retry)
    for prompt in RolePrompt::ALL.into_iter().map(Some).chain([None]) {
        for now in [before, at, after] {
            for replaced in [0, MAX_JOIN_REPLACEMENTS - 1, MAX_JOIN_REPLACEMENTS] {
                let decided = decide_join(&input(true, prompt, replaced, now));
                // A joined session is never acted on, whatever its prompt said.
                assert_eq!(
                    (
                        decided.state,
                        decided.wind_down,
                        decided.exhausted,
                        decided.retry_at_ms
                    ),
                    (Joined, false, false, None)
                );
                let decided = decide_join(&input(false, prompt, replaced, now));
                let state = if prompt == Some(RolePrompt::Failed) {
                    RolePromptFailed
                } else if now >= at {
                    NeverJoined
                } else {
                    Waiting
                };
                let acts = state != Waiting;
                let exhausted = acts && replaced >= MAX_JOIN_REPLACEMENTS;
                assert_eq!(
                    (
                        decided.state,
                        decided.wind_down,
                        decided.exhausted,
                        decided.retry_at_ms
                    ),
                    (
                        state,
                        acts && !exhausted,
                        exhausted,
                        (state == Waiting).then_some(at)
                    ),
                    "{prompt:?} {now} {replaced}"
                );
            }
        }
    }
    for state in [Joined, Waiting, RolePromptFailed, NeverJoined] {
        assert_eq!(serde_json::to_value(state).unwrap(), state.code());
    }
}

/// Sessions of one run in turn: the first never joins and is replaced, the second's
/// prompt fails and it is replaced, the third never joins and is left; a late join
/// in between is never acted on; a refilled run starts over.
#[test]
fn hs2_nptks8_replacements_of_one_run_are_bounded_and_a_late_join_is_left_alone() {
    let mut replaced = 0;
    let first = decide_join(&input(
        false,
        Some(RolePrompt::Delivered),
        replaced,
        25 * MINUTE,
    ));
    assert!(first.wind_down);
    replaced += 1;
    let second = decide_join(&input(
        false,
        Some(RolePrompt::Failed),
        replaced,
        11 * MINUTE,
    ));
    assert_eq!(
        (second.state, second.wind_down),
        (JoinState::RolePromptFailed, true)
    );
    replaced += 1;
    // The third joins late, after its time was up: joined wins.
    assert!(!decide_join(&input(true, None, replaced, 40 * MINUTE)).wind_down);
    let third = decide_join(&input(
        false,
        Some(RolePrompt::Unsupported),
        replaced,
        40 * MINUTE,
    ));
    assert_eq!(
        (third.state, third.wind_down, third.exhausted),
        (JoinState::NeverJoined, false, true)
    );
    // Repeated passes decide the same.
    assert_eq!(
        decide_join(&input(
            false,
            Some(RolePrompt::Unsupported),
            replaced,
            40 * MINUTE
        )),
        third
    );
    // Another run starts with no replacements.
    assert!(decide_join(&input(false, None, 0, 40 * MINUTE)).wind_down);
}

#[test]
fn hs2_nptks8_the_join_time_defaults_validates_and_is_written_only_when_set() {
    let mut body = PolicyBody::default();
    assert_eq!(body.placement.join_minutes, DEFAULT_JOIN_MINUTES);
    body.placement.automatic = Some(true);
    body.placement.actor = Some("scheduler".into());
    let json = serde_json::to_value(&body).unwrap();
    assert!(json["placement"].get("join_minutes").is_none());
    body.placement.join_minutes = 1;
    let json = serde_json::to_value(&body).unwrap();
    assert_eq!(json["placement"]["join_minutes"], 1);
    assert_eq!(serde_json::from_value::<PolicyBody>(json).unwrap(), body);
    let codes = |minutes: u32| {
        let mut body = body.clone();
        body.placement.join_minutes = minutes;
        body.problems(None)
            .into_iter()
            .map(|problem| problem.code)
            .filter(|code| code.contains("join"))
            .collect::<Vec<_>>()
    };
    assert_eq!(codes(0), ["invalid_join_minutes"]);
    assert!(codes(1).is_empty());
    assert!(codes(1440).is_empty());
    assert_eq!(codes(1441), ["invalid_join_minutes"]);
    for reason in [
        PlacementReason::NeverJoined,
        PlacementReason::RolePromptFailed,
    ] {
        assert_eq!(serde_json::to_value(reason).unwrap(), reason.code());
    }
}
