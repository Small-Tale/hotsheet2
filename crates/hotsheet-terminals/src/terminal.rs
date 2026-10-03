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
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
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
    /// The session worker id its launcher assigned through [`WORKER_ID_ENV`], if any.
    worker_id: Option<String>,
    /// A trailing re-decide is scheduled for a size change the min-interval deferred.
    trailing_resize: Arc<std::sync::atomic::AtomicBool>,
}

/// Wall-clock milliseconds, the same clock the terminal's callers pass as `now_ms`.
fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn resize_pty(
    master: &Mutex<Box<dyn MasterPty + Send>>,
    rows: u16,
    cols: u16,
) -> Result<(), TermError> {
    master
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

/// The launcher-assigned session worker id variable (mirrors `hotsheet_aitools::WORKER_ID_ENV`).
/// A terminal remembers it so a host that reattaches after a restart can still release the
/// session's claims when the terminal ends (HS2-RXWXQ8).
pub const WORKER_ID_ENV: &str = "HOTSHEET_WORKER_ID";

/// Test hook (one-shot: the next drain thread consumes it): delay the drain thread's first read, simulating a loaded machine where a fast
/// child exits before the reader starts (HS2-BCE5XG).
#[cfg(test)]
static DRAIN_START_DELAY_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How often the slave keeper re-checks child exit and the unread byte count. A local
/// process check, never a network request.
#[cfg(unix)]
const SLAVE_RELEASE_POLL: std::time::Duration = std::time::Duration::from_millis(25);

/// Hold the parent's PTY slave until the child has exited **and** the master has no unread
/// output, then drop it so the drain thread reads EOF (HS2-BCE5XG). Exits early (dropping
/// the slave) once the terminal itself is gone.
#[cfg(unix)]
fn release_slave_when_drained(
    slave: Box<dyn portable_pty::SlavePty + Send>,
    child: std::sync::Weak<Mutex<Box<dyn Child + Send + Sync>>>,
    master: std::sync::Weak<Mutex<Box<dyn MasterPty + Send>>>,
) {
    std::thread::spawn(move || {
        let _slave = slave;
        loop {
            let (Some(child), Some(master)) = (child.upgrade(), master.upgrade()) else {
                return;
            };
            let exited = child
                .lock()
                .ok()
                .and_then(|mut child| child.try_wait().ok())
                .is_none_or(|status| status.is_some());
            if exited {
                let unread = master
                    .lock()
                    .ok()
                    .and_then(|master| master.as_raw_fd())
                    .map(unread_bytes)
                    .unwrap_or(0);
                if unread == 0 {
                    return;
                }
            }
            drop((child, master));
            std::thread::sleep(SLAVE_RELEASE_POLL);
        }
    });
}

/// Bytes waiting to be read from a PTY master (`FIONREAD`); 0 when it cannot tell.
#[cfg(unix)]
fn unread_bytes(fd: std::os::unix::io::RawFd) -> usize {
    let mut available: libc::c_int = 0;
    // SAFETY: FIONREAD writes one c_int through the valid pointer; the fd belongs to a live
    // master the caller holds locked.
    let result = unsafe { libc::ioctl(fd, libc::FIONREAD, &mut available) };
    if result == 0 {
        usize::try_from(available).unwrap_or(0)
    } else {
        0
    }
}

impl Terminal {
    /// Spawn `spec` in a fresh PTY, scrubbing the environment and starting the drain thread.
    pub fn spawn(spec: TermSpec) -> Result<Terminal, TermError> {
        let worker_id = spec
            .env
            .iter()
            .rev()
            .find(|(key, _)| key == WORKER_ID_ENV)
            .map(|(_, value)| value.clone());
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
        let mut reader = pair.master.try_clone_reader().map_err(pty_err)?;
        let writer = pair.master.take_writer().map_err(pty_err)?;
        let master = Arc::new(Mutex::new(pair.master));
        let child = Arc::new(Mutex::new(child));
        // HS2-BCE5XG: on macOS the *last* close of a PTY slave waits only briefly (about half a
        // second here) for the master to read pending output, then discards it. A command that
        // writes and exits while the drain thread is starved under load therefore lost
        // all of its output. Keep the parent's slave open until the child has exited and the
        // master has nothing left to read; only then release it so the drain sees EOF.
        #[cfg(unix)]
        release_slave_when_drained(pair.slave, Arc::downgrade(&child), Arc::downgrade(&master));
        #[cfg(not(unix))]
        drop(pair.slave); // release the slave in the parent so EOF is seen on child exit

        let output = Arc::new(OutputReplay::new(SCROLLBACK_BYTES, OUTPUT_CHANNEL_CAP));
        let busy = Arc::new(Mutex::new(BusyDetector::new()));
        let osc = Arc::new(Mutex::new(OscScanner::with_initial_cwd(initial_cwd)));
        let (out, bz, oc) = (output.clone(), busy.clone(), osc.clone());
        std::thread::spawn(move || {
            #[cfg(test)]
            {
                let delay = DRAIN_START_DELAY_MS.swap(0, std::sync::atomic::Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
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
            master,
            child,
            writer: Mutex::new(writer),
            output,
            busy,
            osc,
            sizer: Arc::new(Mutex::new(sizer)),
            size_tx: broadcast::channel(OUTPUT_CHANNEL_CAP).0,
            worker_id,
            trailing_resize: Arc::default(),
        })
    }

    /// The session worker id the launcher assigned in the spawn environment, if any.
    pub fn worker_id(&self) -> Option<&str> {
        self.worker_id.as_deref()
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

    /// The PTY size currently applied and the viewport that drove it (HS2-7Y1BQ2). Attach
    /// paths send this right after the replay, because [`Self::subscribe_size`] only reports
    /// later changes and a viewer of a stable-size terminal would otherwise never learn it.
    pub fn current_size(&self) -> Option<Decision> {
        self.sizer.lock().ok().and_then(|s| s.applied())
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
        self.schedule_trailing_resize();
        decision
    }

    /// The trailing edge of the resize rate limit (HS2-GSRZX6): when the arbiter deferred a
    /// change, decide again once the min-interval has passed so the last size of a burst lands
    /// without waiting for a viewer's next claim. At most one trailing decide is pending.
    fn schedule_trailing_resize(&self) {
        let deferred = self.sizer.lock().ok().and_then(|s| s.deferred_until());
        if deferred.is_none()
            || self
                .trailing_resize
                .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let (sizer, master, size_tx, pending) = (
            self.sizer.clone(),
            self.master.clone(),
            self.size_tx.clone(),
            self.trailing_resize.clone(),
        );
        std::thread::spawn(move || {
            let mut due = deferred;
            loop {
                while let Some(at) = due {
                    std::thread::sleep(std::time::Duration::from_millis(
                        at.saturating_sub(wall_ms()),
                    ));
                    let (decision, next) = match sizer.lock() {
                        Ok(mut s) => (s.decide(wall_ms()), s.deferred_until()),
                        Err(_) => (None, None),
                    };
                    if let Some(d) = decision {
                        let _ = resize_pty(&master, d.rows, d.cols);
                        let _ = size_tx.send(d);
                    }
                    due = next;
                }
                pending.store(false, std::sync::atomic::Ordering::SeqCst);
                // A claim deferred between the last decide and clearing the flag saw a pending
                // trailing decide and did not schedule its own: pick it up here.
                due = sizer.lock().ok().and_then(|s| s.deferred_until());
                if due.is_none() || pending.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
            }
        });
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
        resize_pty(&self.master, rows, cols)
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

    /// Whether a command other than the terminal's own process holds the PTY foreground: a
    /// program the user started from the shell, until it exits back to the prompt (HS2-WQQYT1).
    /// Compares the PTY's foreground process group with the child's pid, as tmux does for its
    /// current command. `None` where the platform cannot tell.
    pub fn foreground_command_running(&self) -> Option<bool> {
        #[cfg(unix)]
        {
            let leader = self.master.lock().ok()?.process_group_leader()?;
            let own = self.child.lock().ok()?.process_id()?;
            Some(i64::from(leader) != i64::from(own))
        }
        #[cfg(not(unix))]
        {
            None
        }
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

    /// The last size of a burst inside the resize min-interval lands on its own, without a
    /// viewer re-claiming it later (HS2-GSRZX6).
    #[test]
    fn a_deferred_size_change_applies_at_the_end_of_the_min_interval() {
        let mut spec = TermSpec::new("sleep");
        spec.args = vec!["5".into()];
        let term = Terminal::spawn(spec).expect("spawn");
        let mut sizes = term.subscribe_size();
        let claim = |cols, rows| ViewportClaim {
            viewer_id: "v1".into(),
            cols,
            rows,
            focus: true,
            visible: true,
            interacting: false,
            activity_at_ms: wall_ms(),
        };
        assert!(term.claim_size(claim(100, 40), wall_ms()).is_some());
        assert!(
            term.claim_size(claim(110, 45), wall_ms()).is_none(),
            "rate-limited"
        );
        assert!(
            term.claim_size(claim(120, 50), wall_ms()).is_none(),
            "rate-limited"
        );
        let first = sizes.blocking_recv().expect("the applied size");
        assert_eq!((first.cols, first.rows), (100, 40));
        let started = std::time::Instant::now();
        let trailing = sizes.blocking_recv().expect("the trailing size");
        assert_eq!(
            (trailing.cols, trailing.rows),
            (120, 50),
            "the burst's last size"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(sizes.try_recv().is_err(), "one trailing resize, no repeats");
        let _ = term.kill();
    }

    /// A takeover held by the focus-hold lands when the hold ends, without any further claim
    /// (HS2-G4C082). Before, it waited for the next claim — up to a 5 s heartbeat.
    #[test]
    fn a_focus_hold_takeover_applies_when_the_hold_ends_without_another_claim() {
        let mut spec = TermSpec::new("sleep");
        spec.args = vec!["5".into()];
        let term = Terminal::spawn(spec).expect("spawn");
        let mut sizes = term.subscribe_size();
        let claim = |viewer: &str, cols, rows, interacting| ViewportClaim {
            viewer_id: viewer.into(),
            cols,
            rows,
            focus: true,
            visible: true,
            interacting,
            activity_at_ms: wall_ms(),
        };
        assert!(
            term.claim_size(claim("desk", 200, 50, true), wall_ms())
                .is_some()
        );
        let first = sizes.blocking_recv().expect("the desk size");
        assert_eq!(first.driven_by.as_deref(), Some("desk"));
        // Past the min-interval, so only the focus-hold defers the phone's takeover.
        std::thread::sleep(std::time::Duration::from_millis(150));
        let tapped = std::time::Instant::now();
        assert!(
            term.claim_size(claim("phone", 80, 24, true), wall_ms())
                .is_none(),
            "held inside the focus-hold"
        );
        let takeover = sizes
            .blocking_recv()
            .expect("the held takeover lands on its own");
        assert_eq!((takeover.cols, takeover.rows), (80, 24));
        assert_eq!(takeover.driven_by.as_deref(), Some("phone"));
        let elapsed = tapped.elapsed();
        assert!(
            elapsed >= std::time::Duration::from_millis(400)
                && elapsed < std::time::Duration::from_millis(1500),
            "lands at the end of the 500 ms hold, got {elapsed:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(sizes.try_recv().is_err(), "one takeover, no repeats");
        let _ = term.kill();
    }

    /// An interactive shell's foreground switches to a command while it runs and back to the
    /// shell when it exits (HS2-WQQYT1).
    #[cfg(unix)]
    #[test]
    fn reports_a_foreground_command_until_it_exits_back_to_the_shell() {
        let mut spec = TermSpec::new("/bin/sh");
        spec.args = vec!["-i".into()];
        spec.env = vec![("PS1".into(), "$ ".into())];
        let term = Terminal::spawn(spec).expect("spawn");
        assert!(
            wait_until(|| term.foreground_command_running() == Some(false), 5),
            "an idle shell holds its own foreground"
        );
        term.write(b"sleep 1\n").unwrap();
        assert!(
            wait_until(|| term.foreground_command_running() == Some(true), 5),
            "the command takes the foreground"
        );
        assert!(
            wait_until(|| term.foreground_command_running() == Some(false), 5),
            "the shell takes it back when the command exits"
        );
        assert!(term.is_alive(), "the shell stays open");
        let _ = term.kill();
    }

    #[test]
    fn records_the_launch_worker_id() {
        let mut spec = TermSpec::new("true");
        spec.env = vec![
            (WORKER_ID_ENV.into(), "first".into()),
            (WORKER_ID_ENV.into(), "terminal-t1".into()),
        ];
        let term = Terminal::spawn(spec).expect("spawn");
        assert_eq!(
            term.worker_id(),
            Some("terminal-t1"),
            "the last value wins, as in the env"
        );
        let plain = Terminal::spawn(TermSpec::new("true")).expect("spawn");
        assert_eq!(plain.worker_id(), None);
    }

    /// HS2-BCE5XG: a command that writes and exits before the drain thread starts reading
    /// keeps its output. Before the fix the parent dropped its slave at spawn, so the child's
    /// exit was the last slave close; macOS waits only briefly for a reader, then discards the
    /// unread bytes (scrollback empty). The 2 s delay outlasts that wait.
    #[test]
    fn a_child_that_exits_before_the_drain_starts_keeps_its_output() {
        DRAIN_START_DELAY_MS.store(2000, std::sync::atomic::Ordering::Relaxed);
        let mut spec = TermSpec::new("printf");
        spec.args = vec!["\x1b]7;file://host/tmp/late-reader\x07early-exit".into()];
        let term = Terminal::spawn(spec).expect("spawn");
        assert!(wait_until(|| !term.is_alive(), 5), "printf should exit");
        assert!(
            wait_until(
                || String::from_utf8_lossy(&term.scrollback()).contains("early-exit"),
                5
            ),
            "output written before the drain started must survive the child's exit"
        );
        assert_eq!(term.term_state().cwd.as_deref(), Some("/tmp/late-reader"));
    }

    /// HS2-BCE5XG regression at the PTY level: with the parent's slave dropped at spawn, a
    /// child that writes and exits while nobody reads for 2 s loses that output (macOS reports EOF
    /// with no data). The slave keeper holds the slave until the output has been read, so a
    /// reader that starts late still receives every byte and then EOF.
    #[cfg(unix)]
    #[test]
    fn the_slave_keeper_preserves_output_for_a_late_reader() {
        use std::io::Read;
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new("printf");
        cmd.arg("late-reader-bytes");
        let child = Arc::new(Mutex::new(pair.slave.spawn_command(cmd).unwrap()));
        let mut reader = pair.master.try_clone_reader().unwrap();
        let master = Arc::new(Mutex::new(pair.master));
        release_slave_when_drained(pair.slave, Arc::downgrade(&child), Arc::downgrade(&master));
        // Nobody reads yet: the child has written (and exited, or is blocked draining).
        std::thread::sleep(std::time::Duration::from_millis(2000));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n]);
            }
            let _ = tx.send(out);
        });
        let out = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the drain reaches EOF once the slave keeper releases the slave");
        assert_eq!(String::from_utf8_lossy(&out), "late-reader-bytes");
        assert!(wait_until(
            || child.lock().unwrap().try_wait().unwrap().is_some(),
            5
        ));
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
