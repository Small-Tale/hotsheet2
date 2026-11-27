//! HS2-P2TCQS (`docs/26-orchestration-projects.md` §26.10): when this machine re-checks
//! whether its AI tools are signed in, sooner than the ten-minute capability detection.
//!
//! Only while some tool reads signed out here, and only as part of a capacity read that
//! asks for it: the capacity watch of work a signed-out tool holds asks `held`, and the
//! reading is detected again once it is older than the current step (one minute,
//! doubling to ten). A re-check that still reads signed out grows the step; a sign-in,
//! or a held request after a gap of more than a step (the hold was released), starts it
//! over. A person looking (Systems, the held card's inspector) asks `now`, detected
//! again unless the reading is younger than [`SIGN_IN_RECHECK_DEBOUNCE`]. Nothing here
//! keeps time by itself: every decision takes the instant it is asked at.

use std::time::{Duration, Instant};

use hotsheet_cli::orchestration::SignInRecheck;

/// The first step of the held re-check.
pub(crate) const SIGN_IN_RECHECK_FIRST: Duration = Duration::from_secs(60);
/// The longest step of the held re-check, the capability detection's own lifetime.
pub(crate) const SIGN_IN_RECHECK_MAX: Duration = super::capabilities::CAPABILITIES_TTL;
/// The youngest reading an on-demand re-check detects again.
pub(crate) const SIGN_IN_RECHECK_DEBOUNCE: Duration = Duration::from_secs(15);

/// The held re-check's backoff and the on-demand debounce of one machine.
#[derive(Debug, Clone)]
pub(crate) struct SignInBackoff {
    first: Duration,
    max: Duration,
    debounce: Duration,
    /// The current held step; `None` until a held request finds a tool signed out.
    step: Option<Duration>,
    /// When a held request last asked.
    last_held: Option<Instant>,
}

impl Default for SignInBackoff {
    fn default() -> Self {
        Self::new(
            SIGN_IN_RECHECK_FIRST,
            SIGN_IN_RECHECK_MAX,
            SIGN_IN_RECHECK_DEBOUNCE,
        )
    }
}

impl SignInBackoff {
    pub(crate) fn new(first: Duration, max: Duration, debounce: Duration) -> Self {
        Self {
            first,
            max: max.max(first),
            debounce,
            step: None,
            last_held: None,
        }
    }

    /// The current held step, while one runs.
    #[cfg(test)]
    pub(crate) fn step(&self) -> Option<Duration> {
        self.step
    }

    /// Whether a reading taken at `taken`, which `signed_out` says has some tool signed
    /// out, is detected again for a read at `now` that asks `request`. A read that asks
    /// nothing, or a reading with every tool signed in or unknown, never is.
    pub(crate) fn due(
        &mut self,
        now: Instant,
        taken: Instant,
        signed_out: bool,
        request: Option<SignInRecheck>,
    ) -> bool {
        let age = now.saturating_duration_since(taken);
        match request {
            None => false,
            Some(SignInRecheck::Now) => signed_out && age >= self.debounce,
            Some(SignInRecheck::Held) => {
                // A hold that went away and came back starts at the first step.
                let released = self.step.is_some_and(|step| {
                    self.last_held
                        .is_none_or(|last| now.saturating_duration_since(last) > step)
                });
                if released {
                    self.step = None;
                }
                self.last_held = Some(now);
                if !signed_out {
                    self.step = None;
                    return false;
                }
                let step = *self.step.get_or_insert(self.first);
                age >= step
            }
        }
    }

    /// A detection ran for a read that asked `request` and now reads `signed_out`. A
    /// held re-check that still reads signed out doubles the step, up to the most; a
    /// sign-in ends it.
    pub(crate) fn checked(&mut self, signed_out: bool, request: Option<SignInRecheck>) {
        if !signed_out {
            self.step = None;
        } else if request == Some(SignInRecheck::Held)
            && let Some(step) = self.step
        {
            self.step = Some(step.saturating_mul(2).min(self.max));
        }
    }
}

#[cfg(test)]
#[path = "sign_in_recheck_tests.rs"]
mod tests;
