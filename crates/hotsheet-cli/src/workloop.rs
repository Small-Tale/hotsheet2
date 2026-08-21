//! Pure helpers for the `hotsheet-cli work` loop (HS2-118) — kept out of the binary so
//! they unit-test without driving a real tool.
//!
//! The loop drives one turn, then decides whether to continue: it stops when Up Next is
//! empty, when a turn cap is hit, or when the queue stops changing (a **thrash guard**,
//! so a tool that can't make progress doesn't spin forever).

use hotsheet_model::Ticket;

/// A cheap signature of the Up Next queue — each ticket's slug + `updated_at` — used to
/// tell whether a turn changed anything at all (a completed ticket leaves the queue; any
/// edit bumps `updated_at`). Two equal signatures across a turn mean no progress.
pub fn queue_signature(up_next: &[Ticket]) -> Vec<(String, String)> {
    up_next
        .iter()
        .map(|t| (t.slug.clone(), t.updated_at.as_str().to_string()))
        .collect()
}

/// Tracks consecutive turns that changed nothing. `record` returns the running count;
/// the loop stops once it reaches the configured limit.
#[derive(Debug, Default)]
pub struct Stall {
    count: u32,
}

impl Stall {
    /// Fold in one turn's outcome: reset to 0 on progress, else increment. Returns the
    /// new consecutive-no-progress count.
    pub fn record(&mut self, progressed: bool) -> u32 {
        self.count = if progressed { 0 } else { self.count + 1 };
        self.count
    }

    /// The current consecutive-no-progress count.
    pub fn count(&self) -> u32 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{Timestamp, Ulid, derive_slug};

    fn ticket(slug_seed: &str, updated: &str) -> Ticket {
        let id = Ulid::new();
        let now = Timestamp::new("2026-08-21T00:00:00Z");
        let mut t = Ticket::new(
            id,
            derive_slug(&id, "HS"),
            slug_seed,
            "task",
            now.clone(),
            now,
        );
        t.updated_at = Timestamp::new(updated);
        t
    }

    #[test]
    fn signature_reflects_slug_and_updated_at() {
        let a = ticket("a", "2026-08-21T00:00:00Z");
        let sig1 = queue_signature(std::slice::from_ref(&a));
        // Same queue → identical signature (no progress).
        assert_eq!(sig1, queue_signature(std::slice::from_ref(&a)));
        // An edit (bumped updated_at) → different signature (progress).
        let mut a2 = a.clone();
        a2.updated_at = Timestamp::new("2026-08-21T00:05:00Z");
        assert_ne!(sig1, queue_signature(std::slice::from_ref(&a2)));
        // A ticket leaving the queue → different signature.
        assert_ne!(sig1, queue_signature(&[]));
    }

    #[test]
    fn stall_counts_consecutive_no_progress_and_resets() {
        let mut s = Stall::default();
        assert_eq!(s.record(false), 1);
        assert_eq!(s.record(false), 2);
        // Progress resets the streak.
        assert_eq!(s.record(true), 0);
        assert_eq!(s.record(false), 1);
        // Interleaved progress keeps it from ever reaching a limit.
        assert_eq!(s.record(true), 0);
        assert_eq!(s.record(false), 1);
        assert_eq!(s.count(), 1);
    }
}
