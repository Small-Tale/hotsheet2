//! A single **PTY-backed terminal** (`docs/05` §5.4): spawn a command in a pseudo-terminal,
//! keep a bounded **scrollback ring** so a newly-attached viewer sees recent output, feed a
//! [`BusyDetector`](crate::busy::BusyDetector) from the output, and expose write / resize /
//! kill. A background thread drains the PTY into the ring so the buffer is always current.
//!
//! Server-arbitrated PTY **sizing** across many viewers is its own concern (HS2-62); here a
//! terminal simply has one size that [`resize`](Terminal::resize) sets.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::busy::{Activity, BusyDetector};
use crate::env::scrub_env;
use crate::osc::{OscScanner, TermState};
use crate::sizing::{Decision, SizeArbiter, SizePolicy, ViewportClaim};

/// How much recent PTY output to retain for a re-attaching viewer.
pub const SCROLLBACK_BYTES: usize = 256 * 1024;

/// How many recent output chunks the live fan-out buffers. A viewer that falls this far
/// behind gets a `Lagged` signal and should re-sync from the scrollback snapshot.
const OUTPUT_CHANNEL_CAP: usize = 256;

/// An error spawning or driving a terminal.
#[derive(Debug, thiserror::Error)]
pub enum TermError {
    #[error("pty: {0}")]
    Pty(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn pty_err<E: std::fmt::Display>(e: E) -> TermError {
    TermError::Pty(e.to_string())
}

/// Why a terminal was created. Output, command names, and later attachments cannot change it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    #[default]
    Shell,
    Ai,
}

/// What to launch in the terminal.
#[derive(Debug, Clone)]
pub struct TermSpec {
    pub kind: TerminalKind,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Extra/override environment vars for the child (e.g. `CODEX_HOME`). The parent
    /// process env is inherited too; the merged result is scrubbed of tool-marker vars.
    pub env: Vec<(String, String)>,
    pub rows: u16,
    pub cols: u16,
}

impl TermSpec {
    /// A shell-less command with a sensible default size and no extra env.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            kind: TerminalKind::Shell,
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            rows: 24,
            cols: 80,
        }
    }
}

#[derive(Clone, Copy)]
struct RetainedByte {
    value: u8,
    safe_start: bool,
}

#[derive(Clone, Copy, Default)]
enum AnsiState {
    #[default]
    Ground,
    Escape,
    Csi,
    String {
        bell_terminated: bool,
    },
    StringEscape {
        bell_terminated: bool,
    },
}

impl AnsiState {
    fn advance(self, byte: u8) -> Self {
        match self {
            Self::Ground => {
                if byte == 0x1b {
                    Self::Escape
                } else {
                    Self::Ground
                }
            }
            Self::Escape => match byte {
                b'[' => Self::Csi,
                b']' => Self::String {
                    bell_terminated: true,
                },
                b'P' | b'X' | b'^' | b'_' => Self::String {
                    bell_terminated: false,
                },
                0x20..=0x2f => Self::Escape,
                _ => Self::Ground,
            },
            Self::Csi => match byte {
                0x18 | 0x1a => Self::Ground,
                0x1b => Self::Escape,
                0x40..=0x7e => Self::Ground,
                _ => Self::Csi,
            },
            Self::String { bell_terminated } => match byte {
                0x07 if bell_terminated => Self::Ground,
                0x1b => Self::StringEscape { bell_terminated },
                _ => Self::String { bell_terminated },
            },
            Self::StringEscape { bell_terminated } => {
                if byte == b'\\' {
                    Self::Ground
                } else if byte == 0x1b {
                    Self::StringEscape { bell_terminated }
                } else {
                    Self::String { bell_terminated }
                }
            }
        }
    }
}

/// A bounded byte ring that trims only at terminal-parser-safe boundaries. Raw byte eviction
/// can expose the parameter tail of an ANSI command (for example `61m`) as visible text when a
/// viewer replays into a fresh emulator. UTF-8 continuation bytes are likewise never retained as
/// the first byte of a snapshot.
struct Ring {
    buf: VecDeque<RetainedByte>,
    cap: usize,
    ansi: AnsiState,
}
impl Ring {
    fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            cap,
            ansi: AnsiState::Ground,
        }
    }
    fn push(&mut self, bytes: &[u8]) {
        for &b in bytes {
            let safe_start = matches!(self.ansi, AnsiState::Ground) && b & 0xc0 != 0x80;
            self.buf.push_back(RetainedByte {
                value: b,
                safe_start,
            });
            self.ansi = self.ansi.advance(b);
            if self.buf.len() > self.cap {
                self.buf.pop_front();
                while self.buf.front().is_some_and(|front| !front.safe_start) {
                    self.buf.pop_front();
                }
            }
        }
    }
    fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().map(|byte| byte.value).collect()
    }
}

/// Owns retained and live output as one synchronization boundary. Publishing and taking a
/// snapshot+subscription are mutually exclusive, so a chunk is either in the snapshot or in
/// the receiver created with it, never both and never neither (HS2-5W0V9M).
struct OutputReplay {
    ring: Mutex<Ring>,
    tx: broadcast::Sender<Vec<u8>>,
}

impl OutputReplay {
    fn new(bytes: usize, chunks: usize) -> Self {
        Self {
            ring: Mutex::new(Ring::new(bytes)),
            tx: broadcast::channel(chunks).0,
        }
    }

    fn publish(&self, bytes: &[u8]) {
        let mut ring = self
            .ring
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ring.push(bytes);
        // Broadcast while the ring lock is held. `subscribe_with_snapshot` takes the same
        // lock before subscribing, which makes the handoff sequence-exact.
        let _ = self.tx.send(bytes.to_vec());
    }

    fn snapshot(&self) -> Vec<u8> {
        self.ring
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot()
    }

    fn subscribe_with_snapshot(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let ring = self
            .ring
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let receiver = self.tx.subscribe();
        (ring.snapshot(), receiver)
    }
}

/// A running PTY terminal.
pub struct Terminal {
    kind: TerminalKind,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    writer: Mutex<Box<dyn Write + Send>>,
    output: Arc<OutputReplay>,
    busy: Arc<Mutex<BusyDetector>>,
    /// Informational OSC 7/8/9 state (cwd / hyperlink / progress), parsed from output (HS2-RCKEJ9).
    osc: Arc<Mutex<OscScanner>>,
    /// Multi-viewer size arbiter — reconciles every attached viewport's size claim into one
    /// PTY size (HS2-BD7Q74). Shared across all viewers of this terminal.
    sizer: Arc<Mutex<SizeArbiter>>,
    /// Broadcasts the arbiter's chosen size to every attached viewer when it changes.
    size_tx: broadcast::Sender<Decision>,
}

impl Terminal {
    /// Spawn `spec` in a fresh PTY, scrubbing the environment and starting the drain thread.
    pub fn spawn(spec: TermSpec) -> Result<Terminal, TermError> {
        let initial_cwd = spec
            .cwd
            .as_ref()
            .map(|cwd| cwd.to_string_lossy().into_owned());
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(pty_err)?;

        let mut cmd = CommandBuilder::new(&spec.command);
        cmd.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            cmd.cwd(cwd);
        }
        // Scrub ambient tool markers first, then apply explicit spec variables. This prevents
        // accidental parent leakage while allowing the server to deliberately route a
        // launched tool back to its permission API with HOTSHEET_SERVER/HOTSHEET_SECRET.
        let mut merged: std::collections::BTreeMap<String, String> =
            scrub_env(std::env::vars()).into_iter().collect();
        for (k, v) in &spec.env {
            merged.insert(k.clone(), v.clone());
        }
        cmd.env_clear();
        for (k, v) in merged {
            cmd.env(k, v);
        }

        let child = pair.slave.spawn_command(cmd).map_err(pty_err)?;
        drop(pair.slave); // release the slave in the parent so EOF is seen on child exit
        let mut reader = pair.master.try_clone_reader().map_err(pty_err)?;
        let writer = pair.master.take_writer().map_err(pty_err)?;

        let output = Arc::new(OutputReplay::new(SCROLLBACK_BYTES, OUTPUT_CHANNEL_CAP));
        let busy = Arc::new(Mutex::new(BusyDetector::new()));
        let osc = Arc::new(Mutex::new(OscScanner::with_initial_cwd(initial_cwd)));
        let (out, bz, oc) = (output.clone(), busy.clone(), osc.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break, // EOF or the pty closed
                    Ok(n) => {
                        let chunk = &buf[..n];
                        out.publish(chunk);
                        if let Ok(mut d) = bz.lock() {
                            d.feed(chunk);
                        }
                        if let Ok(mut o) = oc.lock() {
                            o.feed(chunk);
                        }
                    }
                }
            }
        });

        // Seed the arbiter with the spawn size so a lone viewer's first claim has a baseline.
        let mut sizer = SizeArbiter::default();
        sizer.set_applied(spec.cols, spec.rows);
        Ok(Terminal {
            kind: spec.kind,
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
            writer: Mutex::new(writer),
            output,
            busy,
            osc,
            sizer: Arc::new(Mutex::new(sizer)),
            size_tx: broadcast::channel(OUTPUT_CHANNEL_CAP).0,
        })
    }

    /// The immutable creation kind, independent of the process's output or current command.
    pub fn kind(&self) -> TerminalKind {
        self.kind
    }

    /// A viewport's size claim (HS2-BD7Q74): update the arbiter and, if the reconciled size
    /// changed, resize the PTY and broadcast the decision to all viewers. Returns the decision
    /// when a resize happened. `now_ms` is the injected clock (real millis from the caller).
    pub fn claim_size(&self, claim: ViewportClaim, now_ms: u64) -> Option<Decision> {
        let decision = {
            let mut s = self.sizer.lock().ok()?;
            s.upsert(claim, now_ms);
            s.decide(now_ms)
        };
        self.apply_size(decision)
    }

    /// A viewport disconnected: drop its claim and recompute (self-heal).
    pub fn drop_viewer(&self, viewer_id: &str, now_ms: u64) -> Option<Decision> {
        let decision = {
            let mut s = self.sizer.lock().ok()?;
            s.remove(viewer_id);
            s.decide(now_ms)
        };
        self.apply_size(decision)
    }

    /// Set the per-terminal sizing policy (focus-follows | smallest | largest | pinned).
    pub fn set_size_policy(&self, policy: SizePolicy) {
        if let Ok(mut s) = self.sizer.lock() {
            s.set_policy(policy);
        }
    }

    /// Subscribe to reconciled-size changes (each viewer forwards these to its client).
    pub fn subscribe_size(&self) -> broadcast::Receiver<Decision> {
        self.size_tx.subscribe()
    }

    fn apply_size(&self, decision: Option<Decision>) -> Option<Decision> {
        if let Some(d) = &decision {
            let _ = self.resize(d.rows, d.cols);
            let _ = self.size_tx.send(d.clone());
        }
        decision
    }

    /// Atomically capture retained output and subscribe immediately after it. Every concurrently
    /// published chunk appears exactly once across the returned snapshot and receiver.
    pub fn subscribe_with_scrollback(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        self.output.subscribe_with_snapshot()
    }

    /// Write input bytes to the terminal (keystrokes / a command).
    pub fn write(&self, bytes: &[u8]) -> Result<(), TermError> {
        let mut w = self.writer.lock().map_err(|_| pty_err("writer poisoned"))?;
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    /// Resize the PTY (one size per terminal; multi-viewer arbitration is HS2-62).
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), TermError> {
        self.master
            .lock()
            .map_err(|_| pty_err("master poisoned"))?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(pty_err)
    }

    /// A snapshot of the scrollback ring (what a re-attaching viewer replays).
    pub fn scrollback(&self) -> Vec<u8> {
        self.output.snapshot()
    }

    /// The inferred busy/idle activity from the output stream.
    pub fn activity(&self) -> Activity {
        self.busy
            .lock()
            .map(|d| d.activity())
            .unwrap_or(Activity::Idle)
    }

    /// Informational terminal state parsed from OSC 7/8/9 (cwd / hyperlink / progress).
    pub fn term_state(&self) -> TermState {
        self.osc.lock().map(|o| o.state()).unwrap_or_default()
    }

    /// Whether the child process is still running.
    pub fn is_alive(&self) -> bool {
        self.child
            .lock()
            .ok()
            .and_then(|mut c| c.try_wait().ok())
            .map(|status| status.is_none())
            .unwrap_or(false)
    }

    /// Kill the child process.
    pub fn kill(&self) -> Result<(), TermError> {
        self.child
            .lock()
            .map_err(|_| pty_err("child poisoned"))?
            .kill()
            .map_err(TermError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::time::{Duration, Instant};

    fn wait_until(mut cond: impl FnMut() -> bool, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        cond()
    }

    #[test]
    fn bounded_replay_drops_partial_ansi_sequences_and_utf8_characters() {
        let mut ansi = Ring::new(5);
        ansi.push(b"prefix\x1b[38;5;");
        ansi.push(b"61mOK");
        assert_eq!(ansi.snapshot(), b"OK");

        let mut utf8 = Ring::new(2);
        utf8.push("A€B".as_bytes());
        assert_eq!(utf8.snapshot(), b"B");
    }

    #[test]
    fn bounded_replay_preserves_complete_split_ansi_sequences_before_eviction() {
        let mut ring = Ring::new(64);
        ring.push(b"\x1b]8;;https://example.com");
        ring.push(b"\x1b\\link\x1b[0");
        ring.push(b"m");
        assert_eq!(
            ring.snapshot(),
            b"\x1b]8;;https://example.com\x1b\\link\x1b[0m"
        );
    }

    #[test]
    fn spawns_captures_output_and_exits() {
        let mut spec = TermSpec::new("printf");
        spec.args = vec!["hello-pty".into()];
        let term = Terminal::spawn(spec).expect("spawn");

        // Output lands in the scrollback ring.
        assert!(
            wait_until(
                || String::from_utf8_lossy(&term.scrollback()).contains("hello-pty"),
                5
            ),
            "printf output should reach the scrollback"
        );
        // The short-lived child exits.
        assert!(wait_until(|| !term.is_alive(), 5), "printf should exit");
    }

    #[test]
    fn snapshot_and_live_subscription_partition_concurrent_output_exactly_once() {
        for index in 0..500 {
            let output = Arc::new(OutputReplay::new(1024, 8));
            let barrier = Arc::new(Barrier::new(3));
            let publishing = {
                let output = output.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    output.publish(format!("marker-{index}").as_bytes());
                })
            };
            let subscribing = {
                let output = output.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    output.subscribe_with_snapshot()
                })
            };
            barrier.wait();
            publishing.join().unwrap();
            let (mut combined, mut receiver) = subscribing.join().unwrap();
            while let Ok(chunk) = receiver.try_recv() {
                combined.extend_from_slice(&chunk);
            }
            assert_eq!(combined, format!("marker-{index}").as_bytes());
        }
    }

    #[test]
    fn lag_replacement_snapshot_starts_a_fresh_exact_live_boundary() {
        let output = OutputReplay::new(1024, 2);
        let (_, mut stale) = output.subscribe_with_snapshot();
        output.publish(b"one");
        output.publish(b"two");
        output.publish(b"three");
        assert!(matches!(
            stale.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));

        let (snapshot, mut fresh) = output.subscribe_with_snapshot();
        assert_eq!(snapshot, b"onetwothree");
        output.publish(b"four");
        assert_eq!(fresh.try_recv().unwrap(), b"four");
        assert!(fresh.try_recv().is_err());
    }

    #[test]
    fn subscribe_streams_live_output_to_a_late_and_early_subscriber() {
        let term = Terminal::spawn(TermSpec::new("cat")).expect("spawn cat");
        // Subscribe BEFORE writing — the subscriber should see the live echo.
        let (_, mut rx) = term.subscribe_with_scrollback();
        term.write(b"live-line\n").unwrap();

        let mut acc: Vec<u8> = Vec::new();
        let saw = wait_until(
            || {
                while let Ok(chunk) = rx.try_recv() {
                    acc.extend_from_slice(&chunk);
                }
                String::from_utf8_lossy(&acc).contains("live-line")
            },
            5,
        );
        assert!(saw, "a live subscriber streams new output as it arrives");

        // A brand-new subscriber only gets FUTURE output (the fan-out isn't a replay — that's
        // what `scrollback()` is for); the earlier line is available via the snapshot.
        assert!(
            String::from_utf8_lossy(&term.scrollback()).contains("live-line"),
            "the scrollback snapshot carries the earlier output for a late viewer"
        );
        term.kill().unwrap();
    }

    #[test]
    fn write_reaches_a_cat_child_and_kill_stops_it() {
        // `cat` echoes stdin back — proves the write path + a long-lived child + kill.
        let term = Terminal::spawn(TermSpec::new("cat")).expect("spawn cat");
        assert!(term.is_alive());
        term.write(b"ping\n").unwrap();
        assert!(
            wait_until(
                || String::from_utf8_lossy(&term.scrollback()).contains("ping"),
                5
            ),
            "cat should echo the written line"
        );
        term.kill().unwrap();
        assert!(wait_until(|| !term.is_alive(), 5), "kill stops cat");
    }
}
