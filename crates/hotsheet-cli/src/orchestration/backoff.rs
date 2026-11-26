//! HS2-H1CSSN: the one bounded backoff every orchestration follower loop uses.
//!
//! A follower step that failed, or that returned at once without waiting or learning
//! anything, waits before the next step instead of spinning: the first delay, then
//! double the last, up to the cap. A step that waited or made progress resets it. The
//! follower keeps its cursor across the delay, so nothing is missed.
use std::time::Duration;

/// The first delay after a failed or idle step.
pub const BACKOFF_FIRST: Duration = Duration::from_secs(1);
/// The longest delay between two steps.
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// A step that returned sooner than this without learning anything did not wait: an
/// adapter that cannot wait, or a refusal, answers at once.
pub const BACKOFF_FAST_STEP: Duration = Duration::from_millis(250);

/// The delay before a follower's next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            next: BACKOFF_FIRST,
        }
    }
}

impl Backoff {
    /// The delay a failed or idle step waits now; the one after it doubles, up to
    /// [`BACKOFF_MAX`].
    pub fn delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = (self.next * 2).min(BACKOFF_MAX);
        delay
    }

    /// A step that waited or made progress: the next delay is the first again.
    pub fn reset(&mut self) {
        self.next = BACKOFF_FIRST;
    }

    /// After a step: the delay to wait when it `backs_off`, `None` (and a reset) when
    /// it does not.
    pub fn after_step(&mut self, backs_off: bool) -> Option<Duration> {
        if backs_off {
            Some(self.delay())
        } else {
            self.reset();
            None
        }
    }
}

/// Whether a step that took `elapsed` returned at once rather than waiting.
pub fn is_fast_step(elapsed: Duration) -> bool {
    elapsed < BACKOFF_FAST_STEP
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(values: &[u64]) -> Vec<Duration> {
        values.iter().copied().map(Duration::from_secs).collect()
    }

    #[test]
    fn hs2_h1cssn_backoff_doubles_from_one_second_to_a_sixty_second_cap() {
        let mut backoff = Backoff::default();
        let delays: Vec<Duration> = (0..9).map(|_| backoff.delay()).collect();
        assert_eq!(delays, secs(&[1, 2, 4, 8, 16, 32, 60, 60, 60]));
    }

    #[test]
    fn hs2_h1cssn_backoff_resets_after_progress_and_refills_from_the_start() {
        let mut backoff = Backoff::default();
        assert_eq!(backoff.after_step(true), Some(Duration::from_secs(1)));
        assert_eq!(backoff.after_step(true), Some(Duration::from_secs(2)));
        assert_eq!(backoff.after_step(true), Some(Duration::from_secs(4)));
        // Progress resets; the next failure starts at the first delay again.
        assert_eq!(backoff.after_step(false), None);
        assert_eq!(backoff, Backoff::default());
        assert_eq!(backoff.after_step(true), Some(Duration::from_secs(1)));
        // Repeated resets stay at the first delay; interleaving never skips a step.
        backoff.reset();
        backoff.reset();
        assert_eq!(backoff.after_step(false), None);
        assert_eq!(backoff.after_step(true), Some(Duration::from_secs(1)));
        assert_eq!(backoff.after_step(false), None);
        assert_eq!(backoff.after_step(true), Some(Duration::from_secs(1)));
        assert_eq!(backoff.after_step(true), Some(Duration::from_secs(2)));
    }

    #[test]
    fn hs2_h1cssn_backoff_stays_at_the_cap_and_resets_from_it() {
        let mut backoff = Backoff::default();
        for _ in 0..20 {
            backoff.delay();
        }
        assert_eq!(backoff.delay(), BACKOFF_MAX);
        backoff.reset();
        assert_eq!(backoff.delay(), BACKOFF_FIRST);
    }

    #[test]
    fn hs2_h1cssn_a_fast_step_is_one_shorter_than_the_guard() {
        assert!(is_fast_step(Duration::ZERO));
        assert!(is_fast_step(BACKOFF_FAST_STEP - Duration::from_millis(1)));
        assert!(!is_fast_step(BACKOFF_FAST_STEP));
        assert!(!is_fast_step(Duration::from_secs(8)));
    }
}
