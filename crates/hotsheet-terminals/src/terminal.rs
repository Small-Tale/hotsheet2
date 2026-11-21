//! A single **PTY-backed terminal** (`docs/05` §5.4): spawn a command in a pseudo-terminal,
//! keep a bounded **scrollback ring** so a newly-attached viewer sees recent output, feed a
//! [`BusyDetector`](crate::busy::BusyDetector) from the output, and expose write / resize /
//! kill. A background thread drains the PTY into the ring so the buffer is always current.
//!
//! Server-arbitrated PTY **sizing** across many viewers is its own concern (HS2-62); here a
//! terminal simply has one size that [`resize`](Terminal::resize) sets.

use hotsheet_sync::LockExt;
use std::collections::VecDeque;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::busy::{Activity, BusyDetector};
use crate::env::scrub_env;
use crate::input_queue::InputQueue;
use crate::osc::{OscScanner, TermState};
use crate::sizing::{Decision, SizeArbiter, SizeClaimOwner, SizePolicy, ViewportClaim};

/// How much recent PTY output to retain for a re-attaching viewer when no limit is
/// configured (HS2-Y5DYC0 sizes new terminals from `crate::limits`).
pub const SCROLLBACK_BYTES: usize = 256 * 1024;

/// Keep a spawn-time replay size within the accepted range.
fn clamp_replay_bytes(bytes: usize) -> usize {
    let (min, max) = crate::limits::REPLAY_BYTES_BOUNDS;
    bytes.clamp(min, max)
}

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
    /// How much recent output to retain for a reattaching viewer (HS2-Y5DYC0), clamped to
    /// [`REPLAY_BYTES_BOUNDS`](crate::limits::REPLAY_BYTES_BOUNDS) at spawn.
    pub replay_bytes: usize,
    /// Machine-local diagnostic identity for a Hot Sheet managed PTY (HS2-GDCMXE).
    pub provenance: Option<ProcessProvenance>,
}

#[derive(Debug, Clone)]
pub struct ProcessProvenance {
    pub machine_home: PathBuf,
    pub supervisor: String,
    pub project_id: String,
    pub session_id: String,
    pub binary_revision: Option<String>,
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
            replay_bytes: SCROLLBACK_BYTES,
            provenance: None,
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

/// Whether `bytes` (starting outside an escape sequence) position the cursor absolutely with
/// `CSI … H` or `CSI … f` (HS2-4PBGKF).
fn moves_cursor_absolutely(bytes: &[u8]) -> bool {
    let mut ansi = AnsiState::Ground;
    bytes.iter().any(|&byte| {
        let in_csi = matches!(ansi, AnsiState::Csi);
        ansi = ansi.advance(byte);
        in_csi && matches!(ansi, AnsiState::Ground) && matches!(byte, b'H' | b'f')
    })
}

/// Whether `bytes` (starting outside an escape sequence) print anything other than blanks and
/// control characters (HS2-4PBGKF).
fn shows_text(bytes: &[u8]) -> bool {
    let mut ansi = AnsiState::Ground;
    bytes.iter().any(|&byte| {
        let ground = matches!(ansi, AnsiState::Ground);
        ansi = ansi.advance(byte);
        ground && byte > b' ' && byte != 0x7f
    })
}

/// A bounded byte ring that trims only at terminal-parser-safe boundaries. Raw byte eviction
/// can expose the parameter tail of an ANSI command (for example `61m`) as visible text when a
/// viewer replays into a fresh emulator. UTF-8 continuation bytes are likewise never retained as
/// the first byte of a snapshot.
struct Ring {
    buf: VecDeque<RetainedByte>,
    cap: usize,
    ansi: AnsiState,
    /// Every byte ever pushed (HS2-K93QHA): the stream offset just past the newest byte.
    end: u64,
}
impl Ring {
    fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            cap,
            ansi: AnsiState::Ground,
            end: 0,
        }
    }
    fn push(&mut self, bytes: &[u8]) {
        self.end += bytes.len() as u64;
        for &b in bytes {
            let safe_start = matches!(self.ansi, AnsiState::Ground) && b & 0xc0 != 0x80;
            self.buf.push_back(RetainedByte {
                value: b,
                safe_start,
            });
            self.ansi = self.ansi.advance(b);
            self.trim();
        }
    }
    fn resize(&mut self, cap: usize) {
        self.cap = cap;
        self.trim();
    }
    fn trim(&mut self) {
        while self.buf.len() > self.cap {
            self.buf.pop_front();
            while self.buf.front().is_some_and(|front| !front.safe_start) {
                self.buf.pop_front();
            }
        }
    }
    fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().map(|byte| byte.value).collect()
    }
    /// Whether a snapshot could start at retained index `index` (the end counts when the parser
    /// is between sequences, so the next pushed byte starts cleanly).
    fn safe_start_at(&self, index: usize) -> bool {
        match self.buf.get(index) {
            Some(byte) => byte.safe_start,
            None => matches!(self.ansi, AnsiState::Ground),
        }
    }
    /// A shared clear (HS2-HA0Z3A): drop every retained byte before the start of the cursor's
    /// line, which is what xterm's `clear()` keeps, so a later replay shows the prompt line and
    /// nothing before it. The line starts after the last newline that ends at a parser-safe
    /// position; a newline inside an escape string does not count. Without one, the whole ring
    /// is the cursor's line and stays (a prompt alone, a repeated clear, or a full-screen program
    /// that addresses the cursor instead of writing newlines). The stream offset is unchanged: it
    /// counts produced bytes, not retained ones.
    ///
    /// A cursor line that moves the cursor to an absolute position (`CSI row;col H`) does not
    /// describe the cursor's line: the bytes since the last newline are the tail of a repaint.
    /// Windows ConPTY repaints the whole screen after every resize, ending with blank rows and a
    /// cursor move back to the prompt, so keeping only that tail cleared the prompt too
    /// (HS2-4PBGKF). Such a clear keeps from the start of the last line that shows text instead.
    fn clear_before_cursor_line(&mut self) {
        let mut starts = vec![0];
        starts.extend(
            (0..self.buf.len())
                .filter(|&index| self.buf[index].value == b'\n' && self.safe_start_at(index + 1))
                .map(|index| index + 1),
        );
        let bytes = self.snapshot();
        let mut line_start = starts[starts.len() - 1];
        if moves_cursor_absolutely(&bytes[line_start..]) {
            line_start = starts
                .windows(2)
                .rev()
                .find(|line| shows_text(&bytes[line[0]..line[1]]))
                .map_or(line_start, |line| line[0]);
        }
        self.buf.drain(..line_start);
    }
    fn replay(&self, stream: u64) -> Replay {
        Replay {
            data: self.snapshot(),
            position: StreamPosition {
                stream,
                end_offset: self.end,
            },
        }
    }
}

/// A point in one terminal's output stream (HS2-K93QHA). `stream` identifies this PTY's output
/// (a random id chosen when it spawns), so an offset from a previous PTY that reused the same
/// terminal id is never mistaken for one in this stream. `end_offset` counts every byte the
/// stream has produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamPosition {
    pub stream: u64,
    pub end_offset: u64,
}

/// A replay snapshot and the stream position just past its last byte (HS2-K93QHA), so a viewer
/// that has applied output up to some offset can be sent only what follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    pub data: Vec<u8>,
    pub position: StreamPosition,
}

impl Replay {
    /// The retained bytes after `offset`, when the replay still holds everything from there.
    pub fn after(&self, offset: u64) -> Option<&[u8]> {
        replay_after(&self.data, self.position.end_offset, offset)
    }
}

/// A fresh output-stream id: unique per spawned PTY in practice, never security-relevant.
fn new_stream_id() -> u64 {
    use std::hash::BuildHasher as _;
    static SPAWNED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let count = SPAWNED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::collections::hash_map::RandomState::new().hash_one((nanos, count, std::process::id()))
}

/// The bytes of a replay ending at `end_offset` that follow `offset`, when the replay still
/// holds everything from there (HS2-K93QHA). A future or evicted offset yields `None`.
pub fn replay_after(data: &[u8], end_offset: u64, offset: u64) -> Option<&[u8]> {
    let start = end_offset.checked_sub(data.len() as u64)?;
    if offset < start || offset > end_offset {
        return None;
    }
    data.get(usize::try_from(offset - start).ok()?..)
}

/// The longest suffix of `replay` no longer than `max` bytes that starts where the ring could
/// have started: outside an escape sequence and not on a UTF-8 continuation byte (HS2-K93QHA).
/// `replay` must itself begin at such a boundary, as every ring snapshot does. A suffix that
/// cannot fit (`max` shorter than the last safe span) is empty.
pub fn replay_suffix(replay: &[u8], max: usize) -> &[u8] {
    if replay.len() <= max {
        return replay;
    }
    let first = replay.len() - max;
    let mut ansi = AnsiState::Ground;
    for (index, &byte) in replay.iter().enumerate() {
        if index >= first && matches!(ansi, AnsiState::Ground) && byte & 0xc0 != 0x80 {
            return &replay[index..];
        }
        ansi = ansi.advance(byte);
    }
    &[]
}

/// Whether a chunk from a terminal's live output fan-out is the replay-rewritten marker
/// (HS2-HA0Z3A): an empty chunk, which real output never is. It means the retained replay was
/// rewritten by a shared clear; a viewer drops what it holds and resyncs from a fresh snapshot.
/// It travels on the output channel itself, so it is ordered exactly with the output around it,
/// and a byte consumer that does not know it (an OSC scanner) sees nothing.
pub fn is_replay_rewritten(chunk: &[u8]) -> bool {
    chunk.is_empty()
}

/// Owns retained and live output as one synchronization boundary. Publishing and taking a
/// snapshot+subscription are mutually exclusive, so a chunk is either in the snapshot or in
/// the receiver created with it, never both and never neither (HS2-5W0V9M).
struct OutputReplay {
    ring: Mutex<Ring>,
    tx: broadcast::Sender<Vec<u8>>,
    /// The output-stream id of the retained replay (HS2-K93QHA). A shared clear starts a new
    /// one (HS2-HA0Z3A), so no viewer resumes across it. Only changed under the ring lock.
    stream: std::sync::atomic::AtomicU64,
}

impl OutputReplay {
    fn new(bytes: usize, chunks: usize) -> Self {
        Self {
            ring: Mutex::new(Ring::new(bytes)),
            tx: broadcast::channel(chunks).0,
            stream: std::sync::atomic::AtomicU64::new(new_stream_id()),
        }
    }

    fn publish(&self, bytes: &[u8]) {
        // An empty chunk is the rewrite marker, never output.
        if bytes.is_empty() {
            return;
        }
        let mut ring = self.ring.lock_or_recover();
        ring.push(bytes);
        // Broadcast while the ring lock is held. `subscribe_with_snapshot` takes the same
        // lock before subscribing, which makes the handoff sequence-exact.
        let _ = self.tx.send(bytes.to_vec());
    }

    /// Clear the retained replay back to the cursor's line, start a new stream id, and tell
    /// live viewers to resync, all under the ring lock (HS2-HA0Z3A).
    fn clear(&self) -> StreamPosition {
        let mut ring = self.ring.lock_or_recover();
        ring.clear_before_cursor_line();
        let stream = new_stream_id();
        self.stream
            .store(stream, std::sync::atomic::Ordering::Relaxed);
        let _ = self.tx.send(Vec::new());
        StreamPosition {
            stream,
            end_offset: ring.end,
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        self.ring.lock_or_recover().snapshot()
    }

    fn resize(&self, bytes: usize) {
        self.ring
            .lock_or_recover()
            .resize(clamp_replay_bytes(bytes));
    }

    fn subscribe_with_snapshot(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let (replay, receiver) = self.subscribe_with_replay();
        (replay.data, receiver)
    }

    fn subscribe_with_replay(&self) -> (Replay, broadcast::Receiver<Vec<u8>>) {
        let ring = self.ring.lock_or_recover();
        let receiver = self.tx.subscribe();
        let stream = self.stream.load(std::sync::atomic::Ordering::Relaxed);
        (ring.replay(stream), receiver)
    }
}

/// What one arbiter pass asks the terminal to do.
enum Decided {
    /// Resize the PTY and broadcast the decision.
    Resize(Decision),
    /// Broadcast the unchanged applied size because the driver or the query answerer changed
    /// (HS2-0GZZHV, HS2-G1ADYV).
    Notice(Decision),
    Nothing,
}

/// A running PTY terminal.
pub struct Terminal {
    kind: TerminalKind,
    /// Directory chosen at launch; unlike OSC 7 cwd, it never follows shell navigation.
    launch_cwd: Option<String>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    /// Ordered input drained by a dedicated writer thread, so a PTY that is not accepting
    /// input never blocks an async worker (HS2-FERD7N).
    input: InputQueue,
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
    exit_hint: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// The pseudoconsole's slave, kept to start the buffer-clearing helper on it (HS2-G1YTWQ).
    /// It shares the master's pseudoconsole, so holding it does not delay end of output.
    #[cfg(windows)]
    slave: Mutex<Box<dyn portable_pty::SlavePty + Send>>,
}

/// HS2-G1YTWQ: the PowerShell that a Windows clear starts on the terminal's own pseudoconsole. It
/// scrolls ConPTY's screen buffer up by the cursor's row and moves the cursor with it, so the
/// cursor's line becomes the top row and nothing above it is left for a later resize repaint.
/// Windows Terminal's Clear Buffer clears ConPTY the same way (`ClearPseudoConsole`), which
/// `portable-pty` does not expose.
#[cfg(windows)]
const CONPTY_CLEAR_SCRIPT: &str = "$y=[Console]::CursorTop-[Console]::WindowTop;if($y -gt 0){[Console]::Out.Write([string][char]27+'['+$y+'S'+[char]27+'['+$y+'A');[Console]::Out.Flush()}";

/// The helper command for [`CONPTY_CLEAR_SCRIPT`].
#[cfg(windows)]
fn conpty_clear_command() -> CommandBuilder {
    let mut command = CommandBuilder::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        CONPTY_CLEAR_SCRIPT,
    ]);
    command
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
        .lock_or_recover()
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(pty_err)
}

/// Apply one arbiter decision: broadcast it, then resize the PTY. Callers hold the arbiter lock,
/// so decisions reach the PTY and the viewers in the order the arbiter made them (HS2-AYVQ9Q).
fn apply_decided(
    size_tx: &broadcast::Sender<Decision>,
    master: &Mutex<Box<dyn MasterPty + Send>>,
    decided: Decided,
) -> Option<Decision> {
    #[cfg(test)]
    pause_before_apply();
    match decided {
        Decided::Resize(d) => {
            // Announce before resizing: the program's reaction to SIGWINCH (often a cursor
            // position query) must reach each viewer after the frame naming the new answerer,
            // or the old driver answers it too (HS2-0GZZHV).
            let _ = size_tx.send(d.clone());
            let _ = resize_pty(master, d.rows, d.cols);
            Some(d)
        }
        // Same size, new driver or answerer: every viewer learns who drives and who answers
        // queries now (HS2-0GZZHV, HS2-G1ADYV).
        Decided::Notice(d) => {
            let _ = size_tx.send(d);
            None
        }
        Decided::Nothing => None,
    }
}

#[cfg(test)]
type ApplyPause = (std::sync::Arc<std::sync::Barrier>, std::time::Duration);

#[cfg(test)]
thread_local! {
    /// Test hook (one-shot, this thread only): stall the next decision this thread makes between
    /// deciding and applying it, as a descheduled thread on a loaded machine does. The barrier
    /// tells the test the stall has begun (HS2-AYVQ9Q).
    static APPLY_PAUSE: std::cell::Cell<Option<ApplyPause>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn pause_before_apply() {
    if let Some((reached, stall)) = APPLY_PAUSE.with(std::cell::Cell::take) {
        reached.wait();
        std::thread::sleep(stall);
    }
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
                .with_lock(|child| child.try_wait().ok())
                .is_none_or(|status| status.is_some());
            if exited {
                let unread = master
                    .with_lock(|master| master.as_raw_fd())
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

/// Switch the master to non-blocking I/O and wrap its reader and writer so both wait in
/// `poll(2)`; the writer gives up once the child has exited or the terminal is gone
/// (HS2-RA494E, see [`crate::pty_io`]).
#[cfg(unix)]
fn interruptible_pty_io(
    master: &dyn MasterPty,
    reader: Box<dyn std::io::Read + Send>,
    writer: Box<dyn std::io::Write + Send>,
    child: &Arc<Mutex<Box<dyn Child + Send + Sync>>>,
) -> Result<(crate::pty_io::PtyReader, crate::pty_io::PtyWriter), TermError> {
    use crate::pty_io::{PtyReader, PtyWriter, dup_owned, set_nonblocking};
    let fd = master
        .as_raw_fd()
        .ok_or_else(|| TermError::Pty("the PTY master has no file descriptor".into()))?;
    let read_fd = dup_owned(fd).map_err(TermError::Io)?;
    let write_fd = dup_owned(fd).map_err(TermError::Io)?;
    set_nonblocking(&write_fd).map_err(TermError::Io)?;
    let child = Arc::downgrade(child);
    let abandoned = move || {
        child.upgrade().is_none_or(|child| {
            child
                .with_lock(|child| child.try_wait().ok())
                .is_none_or(|status| status.is_some())
        })
    };
    Ok((
        PtyReader::new(reader, read_fd),
        PtyWriter::new(writer, write_fd, abandoned),
    ))
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
        let mut spec = spec;
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
        let reader = pair.master.try_clone_reader().map_err(pty_err)?;
        let writer = pair.master.take_writer().map_err(pty_err)?;
        let child = Arc::new(Mutex::new(child));
        let exit_hint = if let Some(provenance) = spec.provenance.take() {
            use hotsheet_process_journal::{ExitReason, LaunchMeta, ProcessJournal, Purpose};
            let pid = child.with_lock(|child| child.process_id());
            let journal = ProcessJournal::at(provenance.machine_home);
            let purpose = match spec.kind {
                TerminalKind::Shell => Purpose::TerminalShell,
                TerminalKind::Ai => Purpose::TerminalAi,
            };
            let record = pid
                .ok_or_else(|| std::io::Error::other("spawned terminal PID unavailable"))
                .and_then(|pid| {
                    journal.start(LaunchMeta {
                        pid,
                        supervisor_pid: std::process::id(),
                        supervisor: provenance.supervisor,
                        purpose,
                        project_id: Some(provenance.project_id),
                        session_id: Some(provenance.session_id),
                        binary_revision: provenance.binary_revision,
                    })
                });
            match record {
                Ok(record) => {
                    let hint = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let watched = child.clone();
                    let hint_for_watch = hint.clone();
                    std::thread::spawn(move || {
                        loop {
                            match watched.with_lock(|child| child.try_wait()) {
                                Ok(Some(status)) => {
                                    let reason = if hint_for_watch
                                        .load(std::sync::atomic::Ordering::Relaxed)
                                    {
                                        ExitReason::Killed
                                    } else if status.success() {
                                        ExitReason::Completed
                                    } else {
                                        ExitReason::Failed
                                    };
                                    let _ = journal.finish(&record.launch_id, reason);
                                    break;
                                }
                                Ok(None) => {
                                    std::thread::sleep(std::time::Duration::from_millis(100))
                                }
                                Err(_) => break,
                            }
                        }
                    });
                    Some(hint)
                }
                Err(error) => {
                    eprintln!(
                        "Hot Sheet terminal process journal unavailable: {}",
                        hotsheet_process_journal::start_error_summary(&error)
                    );
                    None
                }
            }
        } else {
            None
        };
        // HS2-RA494E: a non-blocking master so a killed child can never strand the input
        // writer in a kernel write on a full input buffer (Linux does not wake it).
        #[cfg(unix)]
        let (mut reader, writer) = {
            let (reader, writer) = interruptible_pty_io(&*pair.master, reader, writer, &child)?;
            let writer: Box<dyn std::io::Write + Send> = Box::new(writer);
            (reader, writer)
        };
        #[cfg(not(unix))]
        let mut reader = reader;
        let master = Arc::new(Mutex::new(pair.master));
        // HS2-BCE5XG: on macOS the *last* close of a PTY slave waits only briefly (about half a
        // second here) for the master to read pending output, then discards it. A command that
        // writes and exits while the drain thread is starved under load therefore lost
        // all of its output. Keep the parent's slave open until the child has exited and the
        // master has nothing left to read; only then release it so the drain sees EOF.
        #[cfg(unix)]
        release_slave_when_drained(pair.slave, Arc::downgrade(&child), Arc::downgrade(&master));
        // Windows keeps the slave for the clear helper (HS2-G1YTWQ). It is a second handle on the
        // master's pseudoconsole, so output still ends when the master is dropped.
        #[cfg(windows)]
        let slave = Mutex::new(pair.slave);
        #[cfg(not(any(unix, windows)))]
        drop(pair.slave);

        let output = Arc::new(OutputReplay::new(
            clamp_replay_bytes(spec.replay_bytes),
            OUTPUT_CHANNEL_CAP,
        ));
        // HS2-RG91SK: a terminal starts idle at its launch.
        let busy = Arc::new(Mutex::new(BusyDetector::idle_from(epoch_ms())));
        let osc = Arc::new(Mutex::new(OscScanner::with_initial_cwd(
            initial_cwd.clone(),
        )));
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
                        {
                            let mut d = bz.lock_or_recover();
                            d.feed_at(chunk, epoch_ms());
                        }
                        {
                            let mut o = oc.lock_or_recover();
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
            launch_cwd: initial_cwd,
            master,
            child,
            input: InputQueue::spawn(writer),
            output,
            busy,
            osc,
            sizer: Arc::new(Mutex::new(sizer)),
            size_tx: broadcast::channel(OUTPUT_CHANNEL_CAP).0,
            worker_id,
            trailing_resize: Arc::default(),
            exit_hint,
            #[cfg(windows)]
            slave,
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

    pub fn launch_cwd(&self) -> Option<&str> {
        self.launch_cwd.as_deref()
    }

    /// A viewport's size claim (HS2-BD7Q74): update the arbiter and, if the reconciled size
    /// changed, resize the PTY and broadcast the decision to all viewers. Returns the decision
    /// when a resize happened. `now_ms` is the injected clock (real millis from the caller).
    pub fn claim_size(&self, claim: ViewportClaim, now_ms: u64) -> Option<Decision> {
        self.arbitrate(
            |s| {
                s.upsert(claim, now_ms);
                true
            },
            now_ms,
        )
    }

    /// [`Self::claim_size`] for one attach connection, which owns the claim until another
    /// connection claims the same viewer id (HS2-CCNRVW).
    pub fn claim_size_owned(
        &self,
        claim: ViewportClaim,
        owner: SizeClaimOwner,
        now_ms: u64,
    ) -> Option<Decision> {
        self.arbitrate(
            |s| {
                s.upsert_owned(claim, owner, now_ms);
                true
            },
            now_ms,
        )
    }

    /// An attach connection ended: drop the claims it still owns and recompute (self-heal). A
    /// viewport a newer connection reclaimed under the same id keeps its claim (HS2-CCNRVW).
    pub fn release_owner(&self, owner: SizeClaimOwner, now_ms: u64) -> Option<Decision> {
        self.arbitrate(|s| s.release_owner(owner) > 0, now_ms)
    }

    /// A viewport disconnected: drop its claim and recompute (self-heal).
    pub fn drop_viewer(&self, viewer_id: &str, now_ms: u64) -> Option<Decision> {
        self.arbitrate(
            |s| {
                s.remove(viewer_id);
                true
            },
            now_ms,
        )
    }

    /// Apply `change` to the arbiter and, when it returns true, decide and apply the decision
    /// while still holding the arbiter lock (HS2-AYVQ9Q). Deciding under the lock but
    /// broadcasting and resizing after releasing it let two concurrent passes (a viewer's claim
    /// and the trailing re-decide, or two viewers' claims) apply in the opposite order to the
    /// one they decided in. The older size then reached the PTY and every viewer last, while
    /// the arbiter recorded the newer size as applied and never resized again. Applying in
    /// decision order keeps the PTY, the viewers, and the arbiter's applied size equal.
    fn arbitrate(
        &self,
        change: impl FnOnce(&mut SizeArbiter) -> bool,
        now_ms: u64,
    ) -> Option<Decision> {
        let decision = {
            let mut s = self.sizer.lock_or_recover();
            if !change(&mut s) {
                return None;
            }
            let decided = Self::decide_or_notice(&mut s, now_ms);
            apply_decided(&self.size_tx, &self.master, decided)
        };
        self.schedule_trailing_resize();
        decision
    }

    /// Decide the PTY size; when nothing resizes but the driver or the query answerer changed,
    /// the applied size naming them to broadcast instead (HS2-0GZZHV, HS2-G1ADYV).
    fn decide_or_notice(s: &mut SizeArbiter, now_ms: u64) -> Decided {
        match s.decide(now_ms) {
            Some(decision) => Decided::Resize(decision),
            None => s.notice().map_or(Decided::Nothing, Decided::Notice),
        }
    }

    /// Set the per-terminal sizing policy (focus-follows | smallest | largest | pinned).
    pub fn set_size_policy(&self, policy: SizePolicy) {
        {
            let mut s = self.sizer.lock_or_recover();
            s.set_policy(policy);
        }
    }

    /// The PTY size currently applied and the viewport that drove it (HS2-7Y1BQ2). Attach
    /// paths send this right after the replay, because [`Self::subscribe_size`] only reports
    /// later changes and a viewer of a stable-size terminal would otherwise never learn it.
    pub fn current_size(&self) -> Option<Decision> {
        self.sizer.with_lock(|s| s.applied())
    }

    /// Subscribe to reconciled-size changes (each viewer forwards these to its client).
    pub fn subscribe_size(&self) -> broadcast::Receiver<Decision> {
        self.size_tx.subscribe()
    }

    /// The trailing edge of the resize rate limit (HS2-GSRZX6): when the arbiter deferred a
    /// change, decide again once the min-interval has passed so the last size of a burst lands
    /// without waiting for a viewer's next claim. At most one trailing decide is pending.
    fn schedule_trailing_resize(&self) {
        let deferred = self.sizer.with_lock(|s| s.deferred_until());
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
                    // Applied under the arbiter lock, in decision order (HS2-AYVQ9Q). A focus-hold
                    // takeover by a viewport with the same grid lands here without a resize, so
                    // the notice announces its new driver (HS2-G1ADYV).
                    due = sizer.with_lock(|s| {
                        let decided = Self::decide_or_notice(s, wall_ms());
                        apply_decided(&size_tx, &master, decided);
                        s.deferred_until()
                    });
                }
                pending.store(false, std::sync::atomic::Ordering::SeqCst);
                // A claim deferred between the last decide and clearing the flag saw a pending
                // trailing decide and did not schedule its own: pick it up here.
                due = sizer.with_lock(|s| s.deferred_until());
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

    /// [`subscribe_with_scrollback`](Self::subscribe_with_scrollback) with the snapshot's end
    /// offset, so the first live chunk the receiver yields starts exactly there (HS2-K93QHA).
    pub fn subscribe_with_replay(&self) -> (Replay, broadcast::Receiver<Vec<u8>>) {
        self.output.subscribe_with_replay()
    }

    /// Write input bytes to the terminal (keystrokes / a command) and wait for the PTY to take
    /// them. Blocks the calling thread; async callers use [`write_async`](Self::write_async) or
    /// [`queue_input`](Self::queue_input). All three share one FIFO, so input stays ordered.
    pub fn write(&self, bytes: &[u8]) -> Result<(), TermError> {
        Ok(self.input.write(bytes.to_vec())?)
    }

    /// Write input and wait for the PTY's result without holding any thread (HS2-FERD7N).
    pub async fn write_async(&self, bytes: Vec<u8>) -> Result<(), TermError> {
        Ok(self.input.write_async(bytes).await?)
    }

    /// Queue a viewer's input behind earlier input and return without waiting for the PTY, so
    /// an attach keeps reading its control frames while a child is not reading stdin
    /// (HS2-FERD7N). Waits, parking no thread, only while [`INPUT_BUDGET_BYTES`] of earlier
    /// input is still unwritten.
    ///
    /// [`INPUT_BUDGET_BYTES`]: crate::input_queue::INPUT_BUDGET_BYTES
    pub async fn queue_input(&self, bytes: Vec<u8>) -> Result<(), TermError> {
        self.input
            .queue(bytes)
            .await
            .map_err(|closed| TermError::Io(closed.into()))
    }

    /// Input accepted but not yet written to the PTY (diagnostics and tests).
    pub fn queued_input_bytes(&self) -> usize {
        self.input.queued_bytes()
    }

    /// Resize the PTY (one size per terminal; multi-viewer arbitration is HS2-62).
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), TermError> {
        resize_pty(&self.master, rows, cols)
    }

    /// A snapshot of the scrollback ring (what a re-attaching viewer replays).
    pub fn scrollback(&self) -> Vec<u8> {
        self.output.snapshot()
    }

    /// Clear the terminal for every viewer (HS2-HA0Z3A): the replay ring keeps only the cursor's
    /// line, the output stream gets a new id so no viewer resumes from before the clear, and
    /// every attached viewer receives a replacement snapshot. The shell is not told. Returns the
    /// new stream position.
    ///
    /// On Windows, ConPTY keeps its own screen buffer and repaints it after every resize, which
    /// would bring the cleared output back to every viewer. The clear therefore also moves the
    /// cursor's line to the top of that buffer (HS2-G1YTWQ). Its repaint arrives after the clear and
    /// shows only that line, as the viewers already do. A program reading its own screen buffer
    /// sees the same change, as it does after Windows Terminal's Clear Buffer.
    pub fn clear_replay(&self) -> StreamPosition {
        let position = self.output.clear();
        #[cfg(windows)]
        self.clear_conpty_buffer();
        position
    }

    /// Start [`CONPTY_CLEAR_SCRIPT`] on this terminal's pseudoconsole and reap it on a thread. A
    /// helper that cannot start leaves the replay cleared and is only logged.
    #[cfg(windows)]
    fn clear_conpty_buffer(&self) {
        if self
            .child
            .with_lock(|child| child.try_wait().ok())
            .is_none_or(|status| status.is_some())
        {
            return;
        }
        match self
            .slave
            .with_lock(|slave| slave.spawn_command(conpty_clear_command()))
        {
            Ok(mut helper) => {
                std::thread::spawn(move || {
                    let _ = helper.wait();
                });
            }
            Err(error) => {
                eprintln!("Hot Sheet terminal could not clear the ConPTY buffer: {error}")
            }
        }
    }

    /// How many bytes of recent output the scrollback ring retains (HS2-Y5DYC0).
    pub fn replay_capacity(&self) -> usize {
        self.output.ring.lock_or_recover().cap
    }

    /// Apply a new replay limit to this running PTY. Shrinking evicts old bytes at a safe
    /// parser boundary; growing retains future output without inventing discarded history.
    pub fn resize_replay(&self, bytes: usize) {
        self.output.resize(bytes);
    }

    /// The inferred busy/idle activity from the output stream.
    pub fn activity(&self) -> Activity {
        self.busy.with_lock(|d| d.activity())
    }

    /// HS2-RG91SK: since when the terminal has been idle (its last turn finished, or
    /// its launch); `None` while busy.
    pub fn idle_since_ms(&self) -> Option<u64> {
        self.busy.with_lock(|d| d.idle_since_ms())
    }

    /// Informational terminal state parsed from OSC 7/8/9 (cwd / hyperlink / progress).
    pub fn term_state(&self) -> TermState {
        self.osc.with_lock(|o| o.state())
    }

    /// Whether a command other than the terminal's own process holds the PTY foreground: a
    /// program the user started from the shell, until it exits back to the prompt (HS2-WQQYT1).
    /// Compares the PTY's foreground process group with the child's pid, as tmux does for its
    /// current command. `None` where the platform cannot tell.
    pub fn foreground_command_running(&self) -> Option<bool> {
        #[cfg(unix)]
        {
            let leader = self.master.lock_or_recover().process_group_leader()?;
            let own = self.child.lock_or_recover().process_id()?;
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
            .with_lock(|c| c.try_wait().ok())
            .map(|status| status.is_none())
            .unwrap_or(false)
    }

    /// Kill the child process.
    pub fn kill(&self) -> Result<(), TermError> {
        if let Some(hint) = &self.exit_hint {
            hint.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.child.lock_or_recover().kill().map_err(TermError::Io)
    }
}

/// HS2-RG91SK: milliseconds since the epoch, for the idle time a terminal reports.
fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::time::{Duration, Instant};

    #[test]
    fn hs2_gdcmxe_terminal_launch_records_session_and_exit() {
        let home = tempfile::tempdir().unwrap();
        let mut spec = TermSpec::new("true");
        spec.provenance = Some(ProcessProvenance {
            machine_home: home.path().to_path_buf(),
            supervisor: "test-host".into(),
            project_id: "project-1".into(),
            session_id: "session-1".into(),
            binary_revision: Some("rev-1".into()),
        });
        let term = Terminal::spawn(spec).unwrap();
        let journal = hotsheet_process_journal::ProcessJournal::at(home.path());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let records = journal.list().unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].session_id.as_deref(), Some("session-1"));
            if records[0].exit_reason == Some(hotsheet_process_journal::ExitReason::Completed) {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!term.is_alive());
    }

    /// HS2-G1YTWQ: a clear on Windows also clears ConPTY's own screen buffer, so the repaint a
    /// later resize produces shows the prompt line and none of the cleared output. Before, the
    /// repaint brought rows 177 to 199 back to every viewer and into the replay.
    #[cfg(windows)]
    #[test]
    fn hs2_g1ytwq_a_resize_after_a_clear_does_not_repaint_the_cleared_output() {
        let mut spec = TermSpec::new("powershell.exe");
        spec.args = [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "1..199|%{'OLD-OUTPUT {0:D3}' -f $_}; [Console]::Out.Write('PROMPT$ '); Start-Sleep 60",
        ]
        .map(String::from)
        .to_vec();
        let term = Terminal::spawn(spec).expect("spawn");
        let text = |term: &Terminal| String::from_utf8_lossy(&term.scrollback()).into_owned();
        assert!(
            wait_until(|| text(&term).contains("PROMPT$"), 30),
            "{}",
            text(&term)
        );
        term.clear_replay();
        // The helper's repaint moves the prompt line to the top row of ConPTY's screen.
        assert!(
            wait_until(|| text(&term).contains("\x1b[1;9H"), 30),
            "{:?}",
            text(&term)
        );
        term.resize(50, 120).expect("resize");
        assert!(
            wait_until(|| text(&term).contains("\x1b[8;50;120t"), 30),
            "{:?}",
            text(&term)
        );
        std::thread::sleep(Duration::from_millis(500));
        let replay = text(&term);
        assert!(!replay.contains("OLD-OUTPUT"), "{replay:?}");
        assert!(replay.contains("PROMPT$"), "{replay:?}");
        let _ = term.kill();
    }

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

    /// HS2-K93QHA: a viewer-sized replay cap trims exactly where the ring itself would.
    #[test]
    fn hs2_k93qha_replay_suffix_matches_ring_trimming() {
        assert_eq!(replay_suffix(b"prefix\x1b[38;5;61mOK", 5), b"OK");
        assert_eq!(replay_suffix("A€B".as_bytes(), 2), b"B");
        assert_eq!(replay_suffix(b"short", 64), b"short");
        assert_eq!(replay_suffix(b"\x1b]0;title\x07", 3), b"");
        assert_eq!(replay_suffix(b"abc", 0), b"");
        let output: Vec<u8> = (0..400)
            .flat_map(|line| format!("\x1b[3{}m{line} é\x1b[0m\r\n", line % 8).into_bytes())
            .collect();
        for cap in [1, 7, 64, 333, 1024, output.len()] {
            let mut ring = Ring::new(cap);
            ring.push(&output);
            assert_eq!(replay_suffix(&output, cap), ring.snapshot(), "cap {cap}");
        }
    }

    /// HS2-K93QHA: the end offset counts every byte, survives trimming and resizing, and a
    /// replay answers exactly the retained bytes after an offset.
    #[test]
    fn hs2_k93qha_replay_offsets_survive_trimming_and_resize() {
        let at = |end_offset| StreamPosition {
            stream: 7,
            end_offset,
        };
        let mut ring = Ring::new(8);
        assert_eq!(
            ring.replay(7),
            Replay {
                data: Vec::new(),
                position: at(0)
            }
        );
        assert_eq!(ring.replay(7).after(0), Some(&b""[..]));
        ring.push(b"abcdef");
        ring.push(b"ghij");
        let replay = ring.replay(7);
        assert_eq!(
            replay,
            Replay {
                data: b"cdefghij".to_vec(),
                position: at(10)
            }
        );
        assert_eq!(replay.after(10), Some(&b""[..]));
        assert_eq!(replay.after(7), Some(&b"hij"[..]));
        assert_eq!(replay.after(2), Some(&b"cdefghij"[..]));
        assert_eq!(replay.after(1), None, "evicted bytes cannot be resumed");
        assert_eq!(replay.after(11), None, "a future offset is never trusted");
        ring.resize(3);
        ring.push(b"k");
        let replay = ring.replay(7);
        assert_eq!(
            replay,
            Replay {
                data: b"ijk".to_vec(),
                position: at(11)
            }
        );
        assert_eq!(replay.after(8), Some(&b"ijk"[..]));
        assert_eq!(replay.after(7), None);
    }

    /// HS2-HA0Z3A: a shared clear keeps exactly the cursor's line, whatever surrounds it.
    #[test]
    fn hs2_ha0z3a_clear_keeps_only_the_cursor_line() {
        let cleared = |chunks: &[&[u8]]| {
            let mut ring = Ring::new(1024);
            for chunk in chunks {
                ring.push(chunk);
            }
            ring.clear_before_cursor_line();
            ring.snapshot()
        };
        // The prompt after the last newline stays; everything above it goes.
        assert_eq!(
            cleared(&[b"one\r\ntwo\r\n\x1b[1m$ \x1b[0m"]),
            b"\x1b[1m$ \x1b[0m"
        );
        // At the start of an empty line nothing is kept.
        assert_eq!(cleared(&[b"one\r\ntwo\r\n"]), b"");
        // An empty ring stays empty.
        assert_eq!(cleared(&[]), b"");
        // A newline inside an escape string is not a line start (OSC title with a newline).
        assert_eq!(cleared(&[b"one\n\x1b]0;a\nb\x07$ "]), b"\x1b]0;a\nb\x07$ ");
        // A trailing newline inside an unfinished escape string is not a line start either.
        assert_eq!(cleared(&[b"one\n\x1b]0;a\n"]), b"\x1b]0;a\n");
        // No newline at all: the whole ring is the cursor's line and stays.
        assert_eq!(cleared(&[b"$ "]), b"$ ");
        assert_eq!(
            cleared(&[b"\x1b[Hscreen\x1b[38;5"]),
            b"\x1b[Hscreen\x1b[38;5"
        );
        assert_eq!(cleared(&["é".as_bytes()]), "é".as_bytes());
    }

    /// HS2-4PBGKF: Windows ConPTY repaints the screen after a resize and ends the repaint with
    /// blank rows and an absolute cursor move back to the prompt. The clear keeps the prompt's
    /// line from that repaint, not only the trailing cursor move, and clearing again keeps it.
    #[test]
    fn hs2_4pbgkf_clear_after_a_conpty_repaint_keeps_the_prompt_line() {
        let mut ring = Ring::new(4096);
        ring.push(b"OLD-OUTPUT 198\r\nOLD-OUTPUT 199\r\nPROMPT$\x1b[1C");
        // The repaint a resize produces (captured from ConPTY on Windows 11 ARM64).
        ring.push(b"\x1b[?25l\x1b[8;50;120t\x1b[HOLD-OUTPUT 198\x1b[K\r\nOLD-OUTPUT 199\x1b[K\r\n");
        ring.push(b"PROMPT$\x1b[K\r\n\x1b[K\r\n\x1b[K\r\n\x1b[K\x1b[3;9H\x1b[?25h");
        ring.clear_before_cursor_line();
        let kept = b"PROMPT$\x1b[K\r\n\x1b[K\r\n\x1b[K\r\n\x1b[K\x1b[3;9H\x1b[?25h";
        assert_eq!(ring.snapshot(), kept);
        ring.clear_before_cursor_line();
        assert_eq!(
            ring.snapshot(),
            kept,
            "clearing twice keeps the same prompt line"
        );
        // Later output after the prompt line, then another clear, keeps the new cursor line.
        ring.push(b"echo:after\r\nPROMPT$\x1b[1C");
        ring.clear_before_cursor_line();
        assert_eq!(ring.snapshot(), b"PROMPT$\x1b[1C");
        // A repaint with no text at all keeps its own tail, as before.
        let mut blank = Ring::new(4096);
        blank.push(b"\x1b[H\x1b[K\r\n\x1b[K\x1b[2;1H");
        blank.clear_before_cursor_line();
        assert_eq!(blank.snapshot(), b"\x1b[K\x1b[2;1H");
        // A relative cursor move on the prompt line is still just the prompt line.
        let mut relative = Ring::new(4096);
        relative.push(b"old\r\n$ \x1b[2C\x1b[1A");
        relative.clear_before_cursor_line();
        assert_eq!(relative.snapshot(), b"$ \x1b[2C\x1b[1A");
    }

    #[test]
    fn hs2_4pbgkf_absolute_moves_and_visible_text_ignore_escape_contents() {
        assert!(moves_cursor_absolutely(b"\x1b[H"));
        assert!(moves_cursor_absolutely(b"x\x1b[12;4f"));
        assert!(!moves_cursor_absolutely(b"H\x1b[2A\x1b]0;H\x07"));
        assert!(shows_text(b"\x1b[K$"));
        assert!(shows_text("é".as_bytes()));
        assert!(!shows_text(b"\x1b[K \t\r\n\x1b]0;title\x07\x1b[32m"));
    }

    /// HS2-HA0Z3A: clear then output, clear twice, and the stream offset across a clear.
    #[test]
    fn hs2_ha0z3a_clear_transitions_keep_offsets_and_later_output() {
        let mut ring = Ring::new(1024);
        ring.push(b"old\n$ ");
        ring.clear_before_cursor_line();
        assert_eq!(ring.snapshot(), b"$ ");
        // The offset still counts produced bytes; the replay window starts after the clear.
        let replay = ring.replay(9);
        assert_eq!(replay.position.end_offset, 6);
        assert_eq!(replay.after(4), Some(&b"$ "[..]));
        assert_eq!(
            replay.after(3),
            None,
            "a cleared byte cannot be resumed from"
        );
        ring.push(b"ls\nfile\n$ ");
        assert_eq!(ring.snapshot(), b"$ ls\nfile\n$ ");
        ring.clear_before_cursor_line();
        assert_eq!(ring.snapshot(), b"$ ");
        ring.clear_before_cursor_line();
        assert_eq!(ring.snapshot(), b"$ ", "clearing twice is idempotent");
        // Trimming after a clear still lands on safe starts.
        ring.resize(4);
        ring.push(b"\x1b[31mred");
        assert_eq!(ring.snapshot(), b"red");
    }

    /// HS2-HA0Z3A: the clear marker reaches live receivers in order with output, a fresh
    /// subscription after the clear sees only the cursor line on a new stream, and the marker is
    /// never stored as output.
    #[test]
    fn hs2_ha0z3a_clear_marks_live_receivers_in_order_and_starts_a_new_stream() {
        let output = OutputReplay::new(1024, 16);
        output.publish(b"old\n$ ");
        let (before, mut live) = output.subscribe_with_replay();
        output.publish(b"x");
        let position = output.clear();
        output.publish(b"y");
        assert_eq!(live.try_recv().unwrap(), b"x");
        assert!(is_replay_rewritten(&live.try_recv().unwrap()));
        assert_eq!(live.try_recv().unwrap(), b"y");
        assert_ne!(position.stream, before.position.stream);
        assert_eq!(position.end_offset, 7);
        let (after, _) = output.subscribe_with_replay();
        assert_eq!(after.data, b"$ xy");
        assert_eq!(after.position.stream, position.stream);
        assert_eq!(after.position.end_offset, 8);
        // Publishing an empty chunk is not output and never looks like a clear.
        output.publish(b"");
        assert!(live.try_recv().is_err());
        assert!(!is_replay_rewritten(b"x"));
    }

    #[test]
    fn replay_resize_trims_at_safe_boundaries_and_retains_new_output_after_growth() {
        let mut ring = Ring::new(64);
        ring.push(b"old\x1b[38;5;61mOK");
        ring.resize(3);
        assert_eq!(ring.snapshot(), b"OK");
        ring.resize(64);
        ring.push(b"-new");
        assert_eq!(ring.snapshot(), b"OK-new");
        ring.resize(2);
        assert_eq!(ring.snapshot(), b"ew");
        ring.push("€!".as_bytes());
        assert_eq!(ring.snapshot(), b"!");
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
            answers_queries: true,
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
            answers_queries: true,
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

    /// HS2-G1ADYV: a focus-hold takeover by a viewport with the same grid lands on the trailing
    /// re-decide without a resize. Before, that pass broadcast only resizes, so every viewer kept
    /// the old driver and the new driver's viewer never started answering terminal queries.
    #[test]
    fn a_same_size_takeover_is_announced_once_without_a_resize() {
        let mut spec = TermSpec::new("sleep");
        spec.args = vec!["5".into()];
        let term = Terminal::spawn(spec).expect("spawn");
        let mut sizes = term.subscribe_size();
        let claim = |viewer: &str, interacting| ViewportClaim {
            viewer_id: viewer.into(),
            cols: 100,
            rows: 30,
            focus: true,
            visible: true,
            interacting,
            answers_queries: true,
            activity_at_ms: 0,
        };
        assert!(term.claim_size(claim("desk", true), wall_ms()).is_some());
        let first = sizes.blocking_recv().expect("the desk size");
        assert_eq!(first.driven_by.as_deref(), Some("desk"));
        // A later interaction than the desk's, so the phone is the takeover candidate.
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(
            term.claim_size(claim("phone", true), wall_ms()).is_none(),
            "held inside the focus-hold"
        );
        let handoff = recv_within(&mut sizes, std::time::Duration::from_secs(5))
            .expect("the held handoff is announced on its own");
        assert_eq!(
            (
                handoff.cols,
                handoff.rows,
                handoff.driven_by.as_deref(),
                handoff.answered_by.as_deref()
            ),
            (100, 30, Some("phone"), Some("phone"))
        );
        assert_eq!(pty_grid(&term), (100, 30), "nothing resized");
        // The old driver's connection ends and the phone heartbeats: nothing more to announce.
        assert!(term.drop_viewer("desk", wall_ms()).is_none());
        assert!(term.claim_size(claim("phone", false), wall_ms()).is_none());
        settle_trailing_resize(&term);
        assert!(sizes.try_recv().is_err(), "one notice, no repeats");
        assert_eq!(
            term.current_size().and_then(|d| d.driven_by).as_deref(),
            Some("phone")
        );
        let _ = term.kill();
    }

    /// The next broadcast size within `limit`, so a missing frame fails instead of hanging.
    fn recv_within(
        sizes: &mut broadcast::Receiver<Decision>,
        limit: std::time::Duration,
    ) -> Option<Decision> {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            match sizes.try_recv() {
                Ok(decision) => return Some(decision),
                Err(broadcast::error::TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(_) => return None,
            }
        }
        None
    }

    /// The PTY's real grid, as the program sees it through `TIOCGWINSZ`.
    fn pty_grid(term: &Terminal) -> (u16, u16) {
        let size = term.master.lock_or_recover().get_size().expect("pty size");
        (size.cols, size.rows)
    }

    /// Wait (bounded) until no trailing re-decide is pending or running.
    fn settle_trailing_resize(term: &Terminal) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while term
            .trailing_resize
            .load(std::sync::atomic::Ordering::SeqCst)
            || term.sizer.with_lock(|s| s.deferred_until()).is_some()
        {
            assert!(Instant::now() < deadline, "the trailing re-decide settles");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The last broadcast size, the PTY's real size, and the arbiter's applied size agree.
    fn assert_applied_everywhere(term: &Terminal, sizes: &mut broadcast::Receiver<Decision>) {
        let applied = term.current_size().expect("an applied size");
        let mut last = None;
        loop {
            match sizes.try_recv() {
                Ok(decision) => last = Some((decision.cols, decision.rows)),
                Err(broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(_) => break,
            }
        }
        assert_eq!(
            last,
            Some((applied.cols, applied.rows)),
            "viewers were told the applied size last"
        );
        assert_eq!(
            pty_grid(term),
            (applied.cols, applied.rows),
            "the PTY has the applied size"
        );
    }

    fn lone_claim(cols: u16, rows: u16) -> ViewportClaim {
        ViewportClaim {
            viewer_id: "zoomed".into(),
            cols,
            rows,
            focus: true,
            visible: true,
            interacting: false,
            answers_queries: true,
            activity_at_ms: 0,
        }
    }

    /// HS2-AYVQ9Q: a pass that decided first but was descheduled before applying its decision
    /// must not apply it after a newer pass. Before, the decision was broadcast and applied to the
    /// PTY after the arbiter lock was released, so the older size landed last: the zoomed Workspace
    /// Grid viewer saw the PTY stay at the other device's size while the arbiter believed its own
    /// size was applied, and no later claim changed anything.
    #[test]
    fn a_stalled_older_decision_never_lands_after_a_newer_one() {
        let mut spec = TermSpec::new("sleep");
        spec.args = vec!["5".into()];
        let term = Arc::new(Terminal::spawn(spec).expect("spawn"));
        let mut sizes = term.subscribe_size();
        let started = wall_ms();
        let reached = Arc::new(Barrier::new(2));
        let older = {
            let (term, reached) = (term.clone(), reached.clone());
            std::thread::spawn(move || {
                APPLY_PAUSE.with(|pause| {
                    pause.set(Some((reached, Duration::from_millis(300))));
                });
                term.claim_size(lone_claim(60, 16), started)
            })
        };
        // The older pass has decided 60x16 and is stalled before applying it.
        reached.wait();
        // A newer claim, past the resize min-interval on the injected clock.
        let newer = term.claim_size(lone_claim(80, 24), started + 1_000);
        let older = older.join().expect("older pass");
        assert_eq!(older.map(|d| (d.cols, d.rows)), Some((60, 16)));
        assert_eq!(newer.map(|d| (d.cols, d.rows)), Some((80, 24)));
        settle_trailing_resize(&term);
        assert_applied_everywhere(&term, &mut sizes);
        assert_eq!(pty_grid(&term), (80, 24), "the newest claim wins");
        let _ = term.kill();
    }

    /// HS2-AYVQ9Q: many concurrent claims with interleaved, out-of-order clocks and stalls between
    /// deciding and applying, repeated, always settle with the viewers, the PTY, and the arbiter
    /// on one size, including after an empty-then-refill pass (the viewer leaves and returns).
    #[test]
    fn concurrent_out_of_order_claims_settle_on_one_applied_size() {
        let mut spec = TermSpec::new("sleep");
        spec.args = vec!["30".into()];
        let term = Arc::new(Terminal::spawn(spec).expect("spawn"));
        let mut sizes = term.subscribe_size();
        // A clock in the past, so a deferred trailing decide is due at once on the wall clock.
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(wall_ms() - 3_600_000));
        for round in 0..3u16 {
            let start = Arc::new(Barrier::new(6));
            let workers: Vec<_> = (0..6u16)
                .map(|worker| {
                    let (term, clock, start) = (term.clone(), clock.clone(), start.clone());
                    std::thread::spawn(move || {
                        start.wait();
                        for step in 0..30u16 {
                            // Take the clock before contending for the lock, as an attach does;
                            // a thread that loses the race carries an older time.
                            let now = clock.fetch_add(150, std::sync::atomic::Ordering::SeqCst);
                            if (step + worker) % 3 == 0 {
                                APPLY_PAUSE.with(|pause| {
                                    pause.set(Some((
                                        Arc::new(Barrier::new(1)),
                                        Duration::from_millis(u64::from(worker % 3)),
                                    )));
                                });
                            }
                            let cols = 40 + 2 * ((worker * 31 + step * 7 + round) % 60);
                            let rows = 10 + 2 * ((worker + step) % 20);
                            term.claim_size(lone_claim(cols, rows), now);
                        }
                    })
                })
                .collect();
            for worker in workers {
                worker.join().expect("worker");
            }
            settle_trailing_resize(&term);
            assert_applied_everywhere(&term, &mut sizes);
            // Empty, then refill: the viewer leaves (the size holds) and claims again.
            term.drop_viewer("zoomed", clock.load(std::sync::atomic::Ordering::SeqCst));
            settle_trailing_resize(&term);
            let refill = clock.fetch_add(1_000, std::sync::atomic::Ordering::SeqCst) + 1_000;
            term.claim_size(lone_claim(100 + round * 4, 30), refill);
            settle_trailing_resize(&term);
            assert_applied_everywhere(&term, &mut sizes);
            assert_eq!(pty_grid(&term), (100 + round * 4, 30));
        }
        let _ = term.kill();
    }

    /// HS2-0GZZHV: an answerer change that resizes nothing is still broadcast, once, so the new
    /// answerer starts replying to the program's queries and the old one stops.
    #[test]
    fn an_answerer_change_without_a_resize_is_broadcast_once() {
        let mut spec = TermSpec::new("sleep");
        spec.args = vec!["5".into()];
        let term = Terminal::spawn(spec).expect("spawn");
        let mut sizes = term.subscribe_size();
        let claim = |viewer: &str, interacting| ViewportClaim {
            viewer_id: viewer.into(),
            cols: 120,
            rows: 40,
            focus: true,
            visible: true,
            interacting,
            answers_queries: true,
            activity_at_ms: wall_ms(),
        };
        assert!(term.claim_size(claim("desk", true), wall_ms()).is_some());
        let first = sizes.blocking_recv().expect("the desk size");
        assert_eq!(first.answered_by.as_deref(), Some("desk"));
        assert!(term.claim_size(claim("phone", false), wall_ms()).is_none());
        assert!(
            sizes.try_recv().is_err(),
            "a passive arrival changes nothing"
        );
        assert!(term.drop_viewer("desk", wall_ms()).is_none(), "no resize");
        let notice = sizes.blocking_recv().expect("the answerer notice");
        assert_eq!(
            (
                notice.cols,
                notice.rows,
                notice.driven_by.as_deref(),
                notice.answered_by.as_deref()
            ),
            // The focused survivor also drives the held size (HS2-G1ADYV).
            (120, 40, Some("phone"), Some("phone"))
        );
        assert!(term.claim_size(claim("phone", false), wall_ms()).is_none());
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(sizes.try_recv().is_err(), "announced once");
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

    #[test]
    fn hs2_s46j0w_launch_directory_survives_osc_cwd_changes() {
        let launch = tempfile::tempdir().unwrap();
        let mut spec = TermSpec::new("printf");
        spec.cwd = Some(launch.path().to_path_buf());
        spec.args = vec!["\x1b]7;file://host/tmp/sibling-checkout\x07".into()];
        let term = Terminal::spawn(spec).expect("spawn");
        assert!(wait_until(
            || term.term_state().cwd.as_deref() == Some("/tmp/sibling-checkout"),
            5
        ));
        assert_eq!(term.launch_cwd(), launch.path().to_str());
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
            || child.lock_or_recover().try_wait().unwrap().is_some(),
            5
        ));
    }

    #[test]
    fn spawns_captures_output_and_exits() {
        let mut spec = TermSpec::new("printf");
        spec.args = vec!["hello-pty".into()];
        let term = Terminal::spawn(spec).expect("spawn");
        term.resize_replay(128 * 1024);
        assert_eq!(term.replay_capacity(), 128 * 1024);

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

    /// HS2-FERD7N adversarial: a child that never reads stdin plus a paste larger than the PTY
    /// input buffer must not stall the runtime, another terminal, or later input. A
    /// current-thread runtime makes a blocked worker fatal: the timeouts could not fire.
    #[tokio::test(flavor = "current_thread")]
    async fn a_child_that_never_reads_stdin_stalls_neither_the_runtime_nor_other_terminals() {
        use std::time::Duration;
        let mut spec = TermSpec::new("sh");
        spec.args = vec![
            "-c".into(),
            "stty raw -echo; echo stuck-ready; exec sleep 60".into(),
        ];
        let stuck = Terminal::spawn(spec).expect("spawn a child that never reads stdin");
        for _ in 0..500 {
            if String::from_utf8_lossy(&stuck.scrollback()).contains("stuck-ready") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let paste = vec![b'x'; 512 * 1024];
        tokio::time::timeout(Duration::from_secs(2), stuck.queue_input(paste))
            .await
            .expect("queueing a paste waited for the PTY")
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            stuck.queued_input_bytes() > 0,
            "the PTY input buffer filled, so its writer thread is blocked"
        );

        let other = Terminal::spawn(TermSpec::new("cat")).expect("spawn cat");
        tokio::time::timeout(
            Duration::from_secs(2),
            other.queue_input(b"still-live\n".to_vec()),
        )
        .await
        .expect("another terminal's input waited behind the stuck one")
        .unwrap();
        let mut echoed = false;
        for _ in 0..500 {
            if String::from_utf8_lossy(&other.scrollback()).contains("still-live") {
                echoed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(echoed, "another terminal keeps round-tripping input");
        // Later keystrokes to the stuck terminal still queue promptly, in order.
        tokio::time::timeout(Duration::from_secs(2), stuck.queue_input(b"more".to_vec()))
            .await
            .expect("later input waited for the PTY")
            .unwrap();

        // Killing the child unblocks the writer; the queue drains (as errors) and closes.
        stuck.kill().unwrap();
        let mut drained = false;
        for _ in 0..500 {
            if stuck.queued_input_bytes() == 0 {
                drained = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            drained,
            "a killed terminal's writer thread drains its queue"
        );
        other.kill().unwrap();
    }
}
