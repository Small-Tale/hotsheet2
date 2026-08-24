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

/// What to launch in the terminal.
#[derive(Debug, Clone)]
pub struct TermSpec {
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
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            rows: 24,
            cols: 80,
        }
    }
}

/// A bounded byte ring — drops the oldest bytes once full.
struct Ring {
    buf: VecDeque<u8>,
    cap: usize,
}
impl Ring {
    fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            cap,
        }
    }
    fn push(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.buf.len() == self.cap {
                self.buf.pop_front();
            }
            self.buf.push_back(b);
        }
    }
    fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }
}

/// A running PTY terminal.
pub struct Terminal {
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    writer: Mutex<Box<dyn Write + Send>>,
    scrollback: Arc<Mutex<Ring>>,
    busy: Arc<Mutex<BusyDetector>>,
    /// Informational OSC 7/8/9 state (cwd / hyperlink / progress), parsed from output (HS2-RCKEJ9).
    osc: Arc<Mutex<OscScanner>>,
    /// Live output fan-out: every drained chunk is broadcast so a WS viewer streams new bytes
    /// as they arrive (after replaying the scrollback snapshot). HS2-XTTTMV.
    output_tx: broadcast::Sender<Vec<u8>>,
    /// Multi-viewer size arbiter — reconciles every attached viewport's size claim into one
    /// PTY size (HS2-BD7Q74). Shared across all viewers of this terminal.
    sizer: Arc<Mutex<SizeArbiter>>,
    /// Broadcasts the arbiter's chosen size to every attached viewer when it changes.
    size_tx: broadcast::Sender<Decision>,
}

impl Terminal {
    /// Spawn `spec` in a fresh PTY, scrubbing the environment and starting the drain thread.
    pub fn spawn(spec: TermSpec) -> Result<Terminal, TermError> {
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
        // Inherit the parent env, apply the spec's overrides, then scrub tool-marker vars —
        // so the child gets PATH etc. but never Hot Sheet's own leak-prone markers.
        let mut merged: std::collections::BTreeMap<String, String> = std::env::vars().collect();
        for (k, v) in &spec.env {
            merged.insert(k.clone(), v.clone());
        }
        cmd.env_clear();
        for (k, v) in scrub_env(merged) {
            cmd.env(k, v);
        }

        let child = pair.slave.spawn_command(cmd).map_err(pty_err)?;
        drop(pair.slave); // release the slave in the parent so EOF is seen on child exit
        let mut reader = pair.master.try_clone_reader().map_err(pty_err)?;
        let writer = pair.master.take_writer().map_err(pty_err)?;

        let scrollback = Arc::new(Mutex::new(Ring::new(SCROLLBACK_BYTES)));
        let busy = Arc::new(Mutex::new(BusyDetector::new()));
        let osc = Arc::new(Mutex::new(OscScanner::new()));
        let (output_tx, _) = broadcast::channel(OUTPUT_CHANNEL_CAP);
        let (sb, bz, oc, tx) = (
            scrollback.clone(),
            busy.clone(),
            osc.clone(),
            output_tx.clone(),
        );
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break, // EOF or the pty closed
                    Ok(n) => {
                        let chunk = &buf[..n];
                        if let Ok(mut s) = sb.lock() {
                            s.push(chunk);
                        }
                        if let Ok(mut d) = bz.lock() {
                            d.feed(chunk);
                        }
                        if let Ok(mut o) = oc.lock() {
                            o.feed(chunk);
                        }
                        // Fan out to live viewers (Err just means no one is attached).
                        let _ = tx.send(chunk.to_vec());
                    }
                }
            }
        });

        // Seed the arbiter with the spawn size so a lone viewer's first claim has a baseline.
        let mut sizer = SizeArbiter::default();
        sizer.set_applied(spec.cols, spec.rows);
        Ok(Terminal {
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
            writer: Mutex::new(writer),
            scrollback,
            busy,
            osc,
            output_tx,
            sizer: Arc::new(Mutex::new(sizer)),
            size_tx: broadcast::channel(OUTPUT_CHANNEL_CAP).0,
        })
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

    /// Subscribe to the live output stream — each drained PTY chunk, as it arrives. A viewer
    /// should first replay [`scrollback`](Self::scrollback), then stream from here. A slow
    /// subscriber may see `Lagged` and should re-sync from a fresh snapshot.
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
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
        self.scrollback
            .lock()
            .map(|s| s.snapshot())
            .unwrap_or_default()
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
    fn subscribe_streams_live_output_to_a_late_and_early_subscriber() {
        let term = Terminal::spawn(TermSpec::new("cat")).expect("spawn cat");
        // Subscribe BEFORE writing — the subscriber should see the live echo.
        let mut rx = term.subscribe();
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
