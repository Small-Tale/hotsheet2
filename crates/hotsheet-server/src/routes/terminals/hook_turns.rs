//! HS2-2AN9C8: a Claude or Codex terminal's turn state, told by the tool itself.
//!
//! Terminal busy inference ([`hotsheet_terminals::BusyDetector`]) reads OSC 133 markers,
//! which neither Claude nor Codex emits, so their terminals would always read idle.
//! Claude reports each turn through its `UserPromptSubmit` and `Stop`/`StopFailure`
//! hooks (`claude_usage`), and Codex through its rollout's `task_started` and
//! `task_complete` (`codex_watch`). Once a terminal's tool has reported a turn boundary,
//! that report is the terminal's `busy` and `idle_since_ms`; a terminal whose tool has
//! never reported keeps the byte-stream inference.

use std::collections::HashMap;

/// A turn boundary a tool reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnBoundary {
    /// A prompt was submitted or a turn started: the tool is at work.
    Started,
    /// The turn ended, normally or on an error: the tool waits for its user.
    Ended,
}

/// One terminal's reported turn state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HookTurn {
    pub(crate) busy: bool,
    /// When the turn ended, in milliseconds since the epoch; `None` while busy.
    pub(crate) idle_since_ms: Option<u64>,
}

/// Every terminal's reported turn state, keyed by terminal id.
#[derive(Debug, Default)]
pub(crate) struct HookTurns(HashMap<String, HookTurn>);

impl HookTurns {
    /// Apply a boundary seen at `now_ms`. A repeated boundary changes nothing, so a
    /// duplicate `Stop` keeps the time the turn really ended. Returns whether the
    /// terminal's busy state changed, including its first report.
    pub(crate) fn record(&mut self, terminal: &str, boundary: TurnBoundary, now_ms: u64) -> bool {
        let busy = boundary == TurnBoundary::Started;
        match self.0.get_mut(terminal) {
            Some(turn) if turn.busy == busy => false,
            Some(turn) => {
                *turn = HookTurn::at(busy, now_ms);
                true
            }
            None => {
                self.0
                    .insert(terminal.to_owned(), HookTurn::at(busy, now_ms));
                true
            }
        }
    }

    pub(crate) fn get(&self, terminal: &str) -> Option<HookTurn> {
        self.0.get(terminal).copied()
    }

    /// The terminal or its session ended; a new session starts unreported.
    pub(crate) fn forget(&mut self, terminal: &str) -> bool {
        self.0.remove(terminal).is_some()
    }

    /// The busy state to report: the tool's own report once there is one, otherwise
    /// the byte-stream inference.
    pub(crate) fn busy_or(&self, terminal: &str, inferred: bool) -> bool {
        self.get(terminal).map_or(inferred, |turn| turn.busy)
    }

    /// Overlay a reported turn onto a terminal's listed `busy` and `idle_since_ms`.
    pub(crate) fn overlay(&self, terminal: &str, busy: &mut bool, idle_since_ms: &mut Option<u64>) {
        if let Some(turn) = self.get(terminal) {
            *busy = turn.busy;
            *idle_since_ms = turn.idle_since_ms;
        }
    }
}

impl HookTurn {
    fn at(busy: bool, now_ms: u64) -> Self {
        Self {
            busy,
            idle_since_ms: (!busy).then_some(now_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HookTurn, HookTurns, TurnBoundary};
    use TurnBoundary::{Ended, Started};

    fn listed(
        turns: &HookTurns,
        terminal: &str,
        inferred: (bool, Option<u64>),
    ) -> (bool, Option<u64>) {
        let (mut busy, mut idle) = inferred;
        turns.overlay(terminal, &mut busy, &mut idle);
        (busy, idle)
    }

    /// HS2-2AN9C8: every transition between unreported, busy and idle, with repeats,
    /// stops before any prompt, interleaved terminals and a forget-then-refill.
    #[test]
    fn hs2_2an9c8_turn_transitions_drive_busy_and_idle_since() {
        let mut turns = HookTurns::default();
        // Unreported: the byte-stream inference stands.
        assert_eq!(listed(&turns, "a", (false, Some(5))), (false, Some(5)));
        assert!(!turns.busy_or("a", false));
        assert!(turns.busy_or("a", true));

        // A prompt makes it busy whatever the bytes say.
        assert!(turns.record("a", Started, 100));
        assert_eq!(listed(&turns, "a", (false, Some(5))), (true, None));
        assert!(turns.busy_or("a", false));
        // A repeated prompt hook changes nothing.
        assert!(!turns.record("a", Started, 150));
        assert_eq!(
            turns.get("a"),
            Some(HookTurn {
                busy: true,
                idle_since_ms: None
            })
        );

        // The turn ends: idle since then.
        assert!(turns.record("a", Ended, 200));
        assert_eq!(listed(&turns, "a", (true, None)), (false, Some(200)));
        assert!(!turns.busy_or("a", true));
        // A duplicate Stop keeps the real end time.
        assert!(!turns.record("a", Ended, 260));
        assert_eq!(turns.get("a").unwrap().idle_since_ms, Some(200));

        // The next turn and its end.
        assert!(turns.record("a", Started, 300));
        assert_eq!(turns.get("a").unwrap().idle_since_ms, None);
        assert!(turns.record("a", Ended, 400));
        assert_eq!(turns.get("a").unwrap().idle_since_ms, Some(400));

        // A stop seen before any prompt (a session that reported late) is idle from then.
        assert!(turns.record("b", Ended, 410));
        assert_eq!(listed(&turns, "b", (true, None)), (false, Some(410)));
        // Interleaved terminals stay independent.
        assert!(turns.record("b", Started, 420));
        assert_eq!(
            turns.get("a").unwrap(),
            HookTurn {
                busy: false,
                idle_since_ms: Some(400)
            }
        );
        assert!(turns.get("b").unwrap().busy);

        // Forgetting returns the terminal to inference; a new session refills it.
        assert!(turns.forget("a"));
        assert!(!turns.forget("a"));
        assert_eq!(listed(&turns, "a", (false, Some(5))), (false, Some(5)));
        assert!(turns.record("a", Started, 500));
        assert!(turns.busy_or("a", false));
    }
}
