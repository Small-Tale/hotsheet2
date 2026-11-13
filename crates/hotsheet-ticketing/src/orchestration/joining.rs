//! HS2-NPTKS8: an orchestrator-started session confirms it took up its role only by
//! registering in the target's coordination pool (HS2-2DPGDY). One whose role prompt
//! failed, or that has not joined within a bounded time, would otherwise go on
//! counting as capacity for its step. This is the pure decision: wait, or wind it
//! down at its safe point so the step's run is placed again, a bounded number of
//! times per run.
use serde::{Deserialize, Serialize};

use super::RolePrompt;

/// How long a started session has to join its role, unless the policy says otherwise
/// (`placement.join_minutes`). Ten minutes is also the role prompt's own delivery
/// window, so a prompt still pending at the end of it has failed.
pub const DEFAULT_JOIN_MINUTES: u32 = 10;

/// How many sessions of one run may be wound down for never joining before the
/// scheduler stops replacing them and leaves the last for a person.
pub const MAX_JOIN_REPLACEMENTS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinState {
    /// The pool shows it in its role.
    Joined,
    /// Not yet, and its time is not up.
    #[serde(rename = "waiting_to_join")]
    Waiting,
    /// Its role prompt failed: it was never asked, so it will not join.
    RolePromptFailed,
    /// Its time to join is up.
    NeverJoined,
}

impl JoinState {
    pub fn code(self) -> &'static str {
        match self {
            Self::Joined => "joined",
            Self::Waiting => "waiting_to_join",
            Self::RolePromptFailed => "role_prompt_failed",
            Self::NeverJoined => "never_joined",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinInput {
    pub joined: bool,
    /// How its role prompt stands, as its host last answered.
    pub role_prompt: Option<RolePrompt>,
    /// When its host confirmed it running.
    pub running_since_ms: u64,
    pub join_ms: u64,
    /// Sessions of the same run already wound down for never joining.
    pub replaced: u32,
    pub now_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinDecision {
    pub state: JoinState,
    /// Wind it down at its safe point, so its step's run is placed again.
    pub wind_down: bool,
    /// The run had its replacements: the session is left as it is, for a person.
    pub exhausted: bool,
    /// When this changes by time alone: the end of its time to join.
    pub retry_at_ms: Option<u64>,
}

/// Decide what to do about a running session that may not have joined its role.
/// Pure. A joined session is never acted on; a failed prompt acts at once; otherwise
/// the session has `join_ms` from when it ran. After [`MAX_JOIN_REPLACEMENTS`]
/// sessions of the run were wound down for it, nothing more is done.
pub fn decide_join(input: &JoinInput) -> JoinDecision {
    let deadline = input.running_since_ms.saturating_add(input.join_ms);
    let state = if input.joined {
        JoinState::Joined
    } else if input.role_prompt == Some(RolePrompt::Failed) {
        JoinState::RolePromptFailed
    } else if input.now_ms >= deadline {
        JoinState::NeverJoined
    } else {
        JoinState::Waiting
    };
    let acts = matches!(state, JoinState::RolePromptFailed | JoinState::NeverJoined);
    let exhausted = acts && input.replaced >= MAX_JOIN_REPLACEMENTS;
    JoinDecision {
        state,
        wind_down: acts && !exhausted,
        exhausted,
        retry_at_ms: (state == JoinState::Waiting).then_some(deadline),
    }
}
