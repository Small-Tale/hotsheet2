//! **Byte-stream busy inference** (`docs/05` §5.4/§5.6, HS2-10). A tool running in a PTY is
//! "busy" while a command is executing and "idle" at a prompt. The authoritative signal is
//! the shell-integration **OSC 133** sequence (`ESC ] 133 ; <A|B|C|D> ST`): `C` marks the
//! start of command output (busy), `A`/`D` a prompt / command-finished (idle). A spinner
//! heuristic ([`contains_spinner`]) is a secondary hint a caller can feed to the
//! sliding-window busy tracker (the connection registry, HS2-107) when a tool emits no
//! OSC 133.
//!
//! [`BusyDetector`] is a **streaming** parser — OSC sequences may be split across reads, so
//! it keeps a small state machine + a bounded payload buffer.

/// Whether the terminal is running a command (`Busy`) or sitting at a prompt (`Idle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Busy,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum OscState {
    #[default]
    Normal,
    Esc,
    Osc,
    OscEsc,
}

/// A streaming OSC-133 busy detector. Feed it PTY output bytes; it tracks busy/idle.
#[derive(Debug, Default)]
pub struct BusyDetector {
    state: OscState,
    payload: Vec<u8>,
    busy: bool,
    /// HS2-RG91SK: when the terminal last became idle (a turn finished), or started
    /// idle, in milliseconds since the epoch. `None` while busy or never stamped.
    idle_since_ms: Option<u64>,
}

impl BusyDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// The current activity.
    pub fn activity(&self) -> Activity {
        if self.busy {
            Activity::Busy
        } else {
            Activity::Idle
        }
    }

    /// HS2-RG91SK: a detector for a terminal that starts idle at `now_ms`.
    pub fn idle_from(now_ms: u64) -> Self {
        Self {
            idle_since_ms: Some(now_ms),
            ..Self::default()
        }
    }

    /// HS2-RG91SK: since when the terminal has been idle; `None` while busy.
    pub fn idle_since_ms(&self) -> Option<u64> {
        if self.busy { None } else { self.idle_since_ms }
    }

    /// Feed output bytes seen at `now_ms`. A turn that finishes stamps the time it went
    /// idle; one that starts clears it. Returns `true` if the busy state changed.
    ///
    /// HS2-NDX3WF: a turn that starts and finishes within these bytes stamps the time
    /// too, though the state did not change: its end is still news to a reader that
    /// compares stamps, where it was lost before.
    pub fn feed_at(&mut self, bytes: &[u8], now_ms: u64) -> bool {
        let before = self.busy;
        let finished = self.feed_turns(bytes);
        if self.busy {
            self.idle_since_ms = None;
        } else if finished {
            self.idle_since_ms = Some(now_ms);
        }
        self.busy != before
    }

    /// Feed output bytes. Returns `true` if the busy state changed (so a caller can emit).
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        let before = self.busy;
        self.feed_turns(bytes);
        self.busy != before
    }

    /// Feed output bytes; whether any turn finished among them (busy, then idle).
    fn feed_turns(&mut self, bytes: &[u8]) -> bool {
        let mut finished = false;
        for &b in bytes {
            let busy = self.busy;
            self.byte(b);
            finished |= busy && !self.busy;
        }
        finished
    }

    fn byte(&mut self, b: u8) {
        match self.state {
            OscState::Normal => {
                if b == 0x1b {
                    self.state = OscState::Esc;
                }
            }
            OscState::Esc => {
                if b == b']' {
                    self.state = OscState::Osc;
                    self.payload.clear();
                } else {
                    self.state = OscState::Normal;
                }
            }
            OscState::Osc => match b {
                0x07 => self.finish(),                 // BEL terminates the OSC
                0x1b => self.state = OscState::OscEsc, // maybe ESC \ terminator
                _ => {
                    if self.payload.len() < 64 {
                        self.payload.push(b);
                    }
                }
            },
            OscState::OscEsc => {
                if b == b'\\' {
                    self.finish(); // ESC \ (ST) terminates the OSC
                } else {
                    // Not a terminator — back to collecting.
                    self.state = OscState::Osc;
                    if b != 0x1b && self.payload.len() < 64 {
                        self.payload.push(b);
                    }
                }
            }
        }
    }

    fn finish(&mut self) {
        // `133;C`, `133;D`, `133;D;0`, `133;A`, `133;B` …
        if let Some(rest) = self.payload.strip_prefix(b"133;") {
            match rest.first() {
                Some(b'C') => self.busy = true,               // command output begins
                Some(b'A') | Some(b'D') => self.busy = false, // prompt / finished
                _ => {}                                       // B (input ready) — leave as-is
            }
        }
        self.state = OscState::Normal;
        self.payload.clear();
    }
}

/// Whether a chunk of output contains a **braille spinner** glyph (`⠋⠙⠹…`, U+2800–U+28FF)
/// — a coarse "still working" hint for tools that don't emit OSC 133. Callers feed this to
/// the connection registry's sliding-window `note_activity` (HS2-107), which handles the
/// idle timeout.
pub fn contains_spinner(bytes: &[u8]) -> bool {
    // Braille Patterns block is UTF-8 `E2 A0..A3 xx` (U+2800..U+28FF).
    bytes
        .windows(2)
        .any(|w| w[0] == 0xE2 && (0xA0..=0xA3).contains(&w[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc133_command_markers_drive_busy() {
        let mut d = BusyDetector::new();
        assert_eq!(d.activity(), Activity::Idle);
        // Command output begins → busy (and the state changed).
        assert!(d.feed(b"\x1b]133;C\x07"));
        assert_eq!(d.activity(), Activity::Busy);
        // Ordinary output doesn't change it.
        assert!(!d.feed(b"building...\n"));
        assert_eq!(d.activity(), Activity::Busy);
        // Command finished → idle.
        assert!(d.feed(b"\x1b]133;D;0\x07"));
        assert_eq!(d.activity(), Activity::Idle);
        // A prompt is idle too.
        d.feed(b"\x1b]133;C\x07");
        assert!(d.feed(b"\x1b]133;A\x07"));
        assert_eq!(d.activity(), Activity::Idle);
    }

    #[test]
    fn osc_split_across_feeds_and_esc_backslash_terminator() {
        let mut d = BusyDetector::new();
        // Sequence split across two feeds.
        assert!(!d.feed(b"\x1b]133;"));
        assert!(d.feed(b"C\x07"));
        assert_eq!(d.activity(), Activity::Busy);
        // ST as ESC-backslash instead of BEL.
        assert!(d.feed(b"\x1b]133;D\x1b\\"));
        assert_eq!(d.activity(), Activity::Idle);
    }

    #[test]
    fn spinner_glyphs_are_detected() {
        assert!(contains_spinner("⠋ working".as_bytes()));
        assert!(contains_spinner("⣾".as_bytes()));
        assert!(!contains_spinner(b"plain ascii output"));
    }
}

#[cfg(test)]
mod hs2_rg91sk_tests {
    use super::*;

    const START: &[u8] = b"\x1b]133;C\x07";
    const DONE: &[u8] = b"\x1b]133;D;0\x07";

    #[test]
    fn hs2_rg91sk_the_detector_says_since_when_it_is_idle() {
        // Started idle at launch: idle since then.
        let mut d = BusyDetector::idle_from(100);
        assert_eq!(d.idle_since_ms(), Some(100));
        // Output that changes nothing keeps the time.
        assert!(!d.feed_at(b"plain text", 150));
        assert_eq!(d.idle_since_ms(), Some(100));
        // A turn starts: busy, no idle time.
        assert!(d.feed_at(START, 200));
        assert_eq!(d.idle_since_ms(), None);
        // More output mid-turn changes nothing.
        assert!(!d.feed_at(b"working", 250));
        // The turn finishes: idle since then, not since launch.
        assert!(d.feed_at(DONE, 300));
        assert_eq!(d.idle_since_ms(), Some(300));
        // A repeated finish keeps the first time; a new turn and its end move it.
        assert!(!d.feed_at(DONE, 350));
        assert_eq!(d.idle_since_ms(), Some(300));
        d.feed_at(START, 400);
        d.feed_at(DONE, 500);
        assert_eq!(d.idle_since_ms(), Some(500));
        // A detector never stamped knows nothing.
        assert_eq!(BusyDetector::new().idle_since_ms(), None);
    }

    /// HS2-NDX3WF: a turn that starts and finishes within one read still stamps its
    /// end, though the state did not change, so a sampler comparing stamps sees it.
    /// A read ending mid-turn clears the stamp; plain output and a finish without a
    /// start keep it.
    #[test]
    fn hs2_ndx3wf_a_turn_within_one_read_stamps_its_end() {
        let mut d = BusyDetector::idle_from(100);
        // Started and finished together: no change, a new stamp.
        let both = [START, b"listening\n", DONE].concat();
        assert!(!d.feed_at(&both, 200));
        assert_eq!(d.idle_since_ms(), Some(200));
        // A finish without a start, or plain output, keeps it.
        assert!(!d.feed_at(DONE, 250));
        assert!(!d.feed_at(b"plain", 260));
        assert_eq!(d.idle_since_ms(), Some(200));
        // A turn finished and the next started in one read: busy, no stamp.
        let next = [START, DONE, START].concat();
        assert!(d.feed_at(&next, 300));
        assert_eq!(d.idle_since_ms(), None);
        // Finished and started again, then finished: the last end stamps.
        let again = [DONE, START, DONE].concat();
        assert!(d.feed_at(&again, 400));
        assert_eq!(d.idle_since_ms(), Some(400));
        // A sequence split across reads stamps when its finish arrives.
        assert!(d.feed_at(b"\x1b]133;C\x07\x1b]133;", 500));
        assert!(d.feed_at(b"D\x07", 510));
        assert_eq!(d.idle_since_ms(), Some(510));
    }
}
