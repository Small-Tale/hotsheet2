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

    /// Feed output bytes. Returns `true` if the busy state changed (so a caller can emit).
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        let before = self.busy;
        for &b in bytes {
            self.byte(b);
        }
        self.busy != before
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
