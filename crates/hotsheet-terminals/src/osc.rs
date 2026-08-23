//! Streaming parser for the **informational** OSC sequences a PTY emits (`docs/05` §5.4,
//! HS2-RCKEJ9): **OSC 7** (working directory), **OSC 8** (hyperlinks), **OSC 9** (progress /
//! desktop notify). It complements the OSC-133 **busy** detector ([`crate::busy`]) — same
//! streaming-state-machine shape, different payloads — so a client can show "where am I",
//! render clickable links, and surface a tool's progress.
//!
//! Sequences may be split across reads, so it keeps a small state machine + a bounded payload
//! buffer. Unknown OSC codes (including 133, which `BusyDetector` owns) are ignored.

/// The latest informational terminal state parsed from OSC sequences.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TermState {
    /// The working directory the shell last reported (OSC 7).
    pub cwd: Option<String>,
    /// The URI of the currently-open hyperlink (OSC 8), if a link is open.
    pub link: Option<String>,
    /// Progress percent `0..=100` (OSC 9;4), if the tool reports it.
    pub progress: Option<u8>,
    /// The last desktop notification / message text (OSC 9, non-progress).
    pub notify: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Normal,
    Esc,
    Osc,
    OscEsc,
}

/// A streaming OSC 7/8/9 scanner. Feed it PTY output; read [`state`](Self::state).
#[derive(Debug, Default)]
pub struct OscScanner {
    state: State,
    payload: Vec<u8>,
    term: TermState,
}

/// URLs + paths can be long; cap the payload so a hostile stream can't grow it unbounded.
const MAX_PAYLOAD: usize = 2048;

impl OscScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// A snapshot of the parsed terminal state.
    pub fn state(&self) -> TermState {
        self.term.clone()
    }

    /// Feed PTY output bytes.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        match self.state {
            State::Normal => {
                if b == 0x1b {
                    self.state = State::Esc;
                }
            }
            State::Esc => {
                if b == b']' {
                    self.state = State::Osc;
                    self.payload.clear();
                } else {
                    self.state = State::Normal;
                }
            }
            State::Osc => match b {
                0x07 => self.finish(),              // BEL terminates the OSC
                0x1b => self.state = State::OscEsc, // maybe ESC \ terminator
                _ => self.push(b),
            },
            State::OscEsc => {
                if b == b'\\' {
                    self.finish(); // ESC \ (ST) terminates the OSC
                } else {
                    self.state = State::Osc;
                    if b != 0x1b {
                        self.push(b);
                    }
                }
            }
        }
    }

    fn push(&mut self, b: u8) {
        if self.payload.len() < MAX_PAYLOAD {
            self.payload.push(b);
        }
    }

    fn finish(&mut self) {
        // Take the payload out first so the dispatch can borrow `self` mutably (the `rest`
        // slice would otherwise alias `self.payload`). This also clears the buffer.
        let payload = std::mem::take(&mut self.payload);
        if let Some(rest) = payload.strip_prefix(b"7;") {
            self.set_cwd(rest);
        } else if let Some(rest) = payload.strip_prefix(b"8;") {
            self.set_link(rest);
        } else if let Some(rest) = payload.strip_prefix(b"9;") {
            self.set_notify_or_progress(rest);
        }
        self.state = State::Normal;
    }

    /// OSC 7 payload is a `file://host/path` URI (or, from some shells, a bare path).
    fn set_cwd(&mut self, rest: &[u8]) {
        let Ok(s) = std::str::from_utf8(rest) else {
            return;
        };
        let path = match s.strip_prefix("file://") {
            // After `file://` comes an optional host, then the absolute path at the first `/`.
            Some(after) => match after.find('/') {
                Some(i) => &after[i..],
                None => after, // no path component
            },
            None => s, // a bare path
        };
        if !path.is_empty() {
            self.term.cwd = Some(path.to_string());
        }
    }

    /// OSC 8 payload is `params;uri`. An empty URI closes the current link.
    fn set_link(&mut self, rest: &[u8]) {
        let Ok(s) = std::str::from_utf8(rest) else {
            return;
        };
        let uri = s.split_once(';').map(|(_, u)| u).unwrap_or("");
        self.term.link = if uri.is_empty() {
            None
        } else {
            Some(uri.to_string())
        };
    }

    /// OSC 9 is `4;<state>;<pct>` for ConEmu-style **progress**, else an iTerm2-style
    /// **notification** message.
    fn set_notify_or_progress(&mut self, rest: &[u8]) {
        let Ok(s) = std::str::from_utf8(rest) else {
            return;
        };
        if let Some(prog) = s.strip_prefix("4;") {
            // `<state>;<pct>` — state 0 clears; 1/2 set the percent; 3/4 are indeterminate/paused.
            let mut parts = prog.split(';');
            let st = parts.next().unwrap_or("0");
            let pct = parts.next().and_then(|p| p.trim().parse::<u8>().ok());
            self.term.progress = match st {
                "0" => None,                     // clear
                "3" | "4" => self.term.progress, // indeterminate/paused — keep last
                _ => pct.map(|p| p.min(100)),    // 1 (default) / 2 (error): set percent
            };
        } else if !s.is_empty() {
            self.term.notify = Some(s.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(bytes: &[&[u8]]) -> TermState {
        let mut s = OscScanner::new();
        for b in bytes {
            s.feed(b);
        }
        s.state()
    }

    #[test]
    fn osc7_sets_cwd_from_a_file_uri_and_a_bare_path() {
        assert_eq!(
            scan(&[b"\x1b]7;file://host/Users/me/proj\x07"])
                .cwd
                .as_deref(),
            Some("/Users/me/proj")
        );
        // Empty host (`file:///path`).
        assert_eq!(
            scan(&[b"\x1b]7;file:///srv/app\x1b\\"]).cwd.as_deref(),
            Some("/srv/app")
        );
        // A bare path (no scheme).
        assert_eq!(scan(&[b"\x1b]7;/tmp/x\x07"]).cwd.as_deref(), Some("/tmp/x"));
    }

    #[test]
    fn osc8_opens_and_closes_a_hyperlink() {
        // Open a link.
        let mut s = OscScanner::new();
        s.feed(b"\x1b]8;;https://example.com/x\x07");
        assert_eq!(s.state().link.as_deref(), Some("https://example.com/x"));
        // An empty URI closes it.
        s.feed(b"\x1b]8;;\x07");
        assert_eq!(s.state().link, None);
    }

    #[test]
    fn osc9_progress_and_notify() {
        // ConEmu progress: state 1, 42%.
        assert_eq!(scan(&[b"\x1b]9;4;1;42\x07"]).progress, Some(42));
        // state 0 clears progress.
        let mut s = OscScanner::new();
        s.feed(b"\x1b]9;4;1;42\x07");
        s.feed(b"\x1b]9;4;0\x07");
        assert_eq!(s.state().progress, None);
        // A non-progress OSC 9 is a notification.
        assert_eq!(
            scan(&[b"\x1b]9;build finished\x07"]).notify.as_deref(),
            Some("build finished")
        );
        // Percent is clamped to 100.
        assert_eq!(scan(&[b"\x1b]9;4;1;250\x07"]).progress, Some(100));
    }

    #[test]
    fn a_sequence_split_across_feeds_still_parses() {
        let st = scan(&[b"\x1b]7;file://h", b"ost/a/b\x07"]);
        assert_eq!(st.cwd.as_deref(), Some("/a/b"));
    }

    #[test]
    fn osc133_and_plain_output_are_ignored() {
        // 133 belongs to BusyDetector; plain output changes nothing here.
        let st = scan(&[b"\x1b]133;C\x07building...\n"]);
        assert_eq!(st, TermState::default());
    }
}
