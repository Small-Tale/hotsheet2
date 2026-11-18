//! HS2-2DPGDY: delivering a started session's role, and confirming it joined.
//!
//! The host asks a new session to take up its role (`hotsheet-worker`,
//! `hotsheet-dispatcher`, `hotsheet-integrator`). A tool is rarely ready the instant
//! its terminal starts, so a prompt that could not be delivered yet is `pending` and
//! is tried again only when the host learns something that may have made the tool
//! ready: its session-push address was registered, or it finished a turn. Nothing
//! asks on a timer. The tries are bounded in number and in time, and a pending
//! delivery whose window passed reads as `failed` without anyone having to notice.
//!
//! Joining is confirmed only by the target's coordination pool: the session's worker
//! id registered, unexpired, in the role it was started for.

use serde::{Deserialize, Serialize};

/// How many times a role prompt is tried in all, the first included.
pub const MAX_ROLE_ATTEMPTS: u32 = 4;
/// How long after the launch a pending role prompt may still be delivered.
pub const ROLE_PROMPT_WINDOW_MS: u64 = 10 * 60 * 1000;

/// How the session was asked to take up its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolePrompt {
    /// Pushed into the running session.
    Delivered,
    /// Passed as the tool's first prompt when it was launched.
    AtLaunch,
    /// Not delivered yet; tried again when the tool may have become ready.
    Pending,
    /// The tool has no way to receive a message from its host. The session's
    /// environment still names its role.
    Unsupported,
    /// Every try failed, or the window passed.
    Failed,
}

impl RolePrompt {
    pub const ALL: [Self; 5] = [
        Self::Delivered,
        Self::AtLaunch,
        Self::Pending,
        Self::Unsupported,
        Self::Failed,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::AtLaunch => "at_launch",
            Self::Pending => "pending",
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
        }
    }

    /// Whether nothing more will be tried.
    pub fn is_settled(self) -> bool {
        self != Self::Pending
    }
}

/// What one try at pushing the prompt answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushAttempt {
    Delivered,
    /// The tool cannot take a message from its host, and never will.
    Unsupported,
    /// The tool may take it later: its address is not known yet, or it did not
    /// answer.
    NotReady,
    /// The tool answered and refused the message.
    Rejected,
    /// HS2-4R5MNJ: the host keeps the prompt and delivers it itself once the tool can take
    /// it (a Codex TUI behind its first-run dialogs). Still pending, and not a try: waiting
    /// on a person at a dialog uses up none of the bounded tries, only the window.
    Held,
}

/// The role delivery as recorded for one hosted session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleDelivery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<RolePrompt>,
    /// Tries so far.
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at_ms: Option<u64>,
}

impl RoleDelivery {
    /// Nothing was asked yet: the record leaves it out.
    pub fn is_unasked(&self) -> bool {
        *self == Self::default()
    }

    /// What the delivery reads as at `now_ms`: a pending one past its window, or out
    /// of tries, is failed.
    pub fn at(self, launched_at_ms: u64, now_ms: u64) -> Option<RolePrompt> {
        match self.prompt {
            Some(RolePrompt::Pending)
                if self.attempts >= MAX_ROLE_ATTEMPTS
                    || now_ms.saturating_sub(launched_at_ms) >= ROLE_PROMPT_WINDOW_MS =>
            {
                Some(RolePrompt::Failed)
            }
            other => other,
        }
    }

    /// Whether a readiness event should try the push again now.
    pub fn should_retry(self, launched_at_ms: u64, now_ms: u64) -> bool {
        self.at(launched_at_ms, now_ms) == Some(RolePrompt::Pending)
    }

    /// Fold one try into the delivery. A settled delivery never changes.
    #[must_use]
    pub fn after(self, attempt: PushAttempt, launched_at_ms: u64, now_ms: u64) -> Self {
        if self.prompt.is_some_and(RolePrompt::is_settled) {
            return self;
        }
        let attempts = if attempt == PushAttempt::Held {
            self.attempts
        } else {
            self.attempts.saturating_add(1)
        };
        let prompt = match attempt {
            PushAttempt::Delivered => RolePrompt::Delivered,
            PushAttempt::Unsupported => RolePrompt::Unsupported,
            PushAttempt::Rejected => RolePrompt::Failed,
            PushAttempt::NotReady | PushAttempt::Held => RolePrompt::Pending,
        };
        let next = Self {
            prompt: Some(prompt),
            attempts,
            delivered_at_ms: (prompt == RolePrompt::Delivered).then_some(now_ms),
        };
        // The last try that was not ready settles it as failed.
        match next.at(launched_at_ms, now_ms) {
            Some(RolePrompt::Failed) => Self {
                prompt: Some(RolePrompt::Failed),
                ..next
            },
            _ => next,
        }
    }

    /// The prompt went with the launch itself.
    pub fn at_launch(now_ms: u64) -> Self {
        Self {
            prompt: Some(RolePrompt::AtLaunch),
            attempts: 1,
            delivered_at_ms: Some(now_ms),
        }
    }
}
