//! The **drive** interface — how the host tells a running AI tool to do something and
//! observes it (`docs/13`). One interface with optional capabilities each tool conforms
//! to as applicable; the caller **never branches on the tool id** (`docs/05` §5.1).
//!
//! `run` is the only required method. Optional capabilities are probed by presence
//! (`drive.supports_interrupt()`, a `TurnHandle` that acts), never a bool flag that can
//! drift from reality.

use std::path::PathBuf;

use crate::ports::{AppServerClient, ProcessSpawner};

/// The transport a tool speaks — a **declarative data tag** (identity, not behavior),
/// so a client and the server agree without a mirror (`docs/13` §13.3). It routes `run`
/// to the right drive; it does not carry behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// A long-lived session driven by injecting a `<channel>` event (Claude).
    ClaudeChannel,
    /// A one-shot process per turn (Codex `exec`, Antigravity `--print`).
    Spawn,
    /// A long-lived backing daemon driven over JSON-RPC (Codex app-server).
    AppServer,
    /// The Agent Client Protocol (OpenCode, Goose).
    Acp,
}

/// A drive's declarative identity.
#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub transport: Transport,
}

/// Which live connection a `run` targets when several exist (the main channel vs. a
/// git-worktree worker's). `None` selects the default/only one.
#[derive(Debug, Clone, Default)]
pub struct Target(pub Option<String>);

/// Host-provided context a drive needs for one `run`. v1 carries the working directory
/// and the injected process spawner; the permission sink and connection registry are
/// added here as those land (`docs/13` §13.3).
pub struct DriveCtx<'a> {
    pub cwd: PathBuf,
    /// Spawn drives launch through this. (Always provide one; app-server drives ignore it.)
    pub spawner: &'a dyn ProcessSpawner,
    /// Extra environment for a spawn-transport launch (e.g. the HS2-103 safety `PATH`
    /// shim, or `--env` pairs the caller passed). The stream transports get their env at
    /// spawn time instead, so they ignore this (`docs/13` §13.0, HS2-0TWTZ4).
    pub env: Vec<(String, String)>,
    /// Present for the app-server (persistent daemon) drive; other transports ignore it.
    pub app_server: Option<&'a dyn AppServerClient>,
    /// Present for the Claude channel drive (a turn on a running `claude` stream-json
    /// session); other transports ignore it.
    pub channel: Option<&'a dyn ClaudeChannelClient>,
}

/// Why a turn finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoneReason {
    Completed,
    Failed(i32),
    Interrupted,
}

/// A tool wants approval mid-turn (the channel/ACP shapes surface these; `docs/13` §13.4,
/// wired to the permission bridge in §5.7 later). Minimal for now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermReq {
    /// The tool/action asking (e.g. `"Bash"`, `"Edit"`).
    pub tool: String,
    /// A short human-facing description of what it wants to do.
    pub summary: String,
}

/// One event from a streaming turn (the async view of a running turn). A drive that can
/// stream yields these in order, ending with exactly one [`TurnEvent::Done`]; a drive that
/// can't returns `None` from [`TurnHandle::next_event`] and the caller uses
/// [`TurnHandle::wait`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEvent {
    /// Assistant output text produced during the turn.
    Output(String),
    /// A permission request the host must answer (unwired for now — the drive runs a
    /// non-blocking permission mode, so this is informational).
    PermissionAsked(PermReq),
    /// Token usage the turn reported (`docs/14`, HS2-0WCRZY) — emitted just before `Done`
    /// when the tool exposes it, so the host can record a `UsageEvent`.
    Usage(Usage),
    /// The turn finished; terminal.
    Done(DoneReason),
}

/// A handle to observe one running turn, **uniform across transports** (`docs/13`
/// §13.4). v1 exposes busy + a terminal wait + interrupt; the streaming `TurnEvent`
/// view (Output/PermissionAsked/…) lands with the async persistent-channel drive.
pub trait TurnHandle {
    /// Whether the turn is still working (derived from the transport's native signal).
    fn is_busy(&mut self) -> bool;
    /// Block until the turn finishes; returns why.
    fn wait(&mut self) -> DoneReason;
    /// Interrupt this turn if the drive supports it; returns whether it acted.
    fn interrupt(&mut self) -> bool {
        false
    }
    /// Pull the next streaming [`TurnEvent`], blocking until one is available. A streaming
    /// drive yields events in order ending with one [`TurnEvent::Done`], then `None`. The
    /// default is a non-streaming drive: it returns `None` and the caller falls back to
    /// [`wait`](Self::wait). Absence is the signal — no bool flag to drift.
    fn next_event(&mut self) -> Option<TurnEvent> {
        None
    }
    /// Token usage the turn reported, if the transport exposes it (`docs/14`, HS2-0WCRZY).
    /// Default `None`; the app-server (codex) handle overrides it.
    fn usage(&mut self) -> Option<Usage> {
        None
    }
}

/// Token usage observed for one turn, mapped from a tool's native telemetry (`docs/14`,
/// HS2-8PSAFE). The host turns this + the active ticket into a `UsageEvent` it records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    /// The model that ran, when the tool reports it.
    pub model: Option<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

/// A failed `run`.
#[derive(Debug, thiserror::Error)]
pub enum DriveError {
    #[error("spawning '{program}': {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    /// A channel/ACP drive with no live session to hit (`docs/13` §13.8).
    #[error("{0}")]
    NotConnected(String),
    /// The app-server (persistent daemon) transport failed.
    #[error("app-server: {0}")]
    AppServer(String),
}

/// A live, persistent `claude` session driven by the channel transport — a turn is one
/// user message injected into the running stream-json process (`docs/13` §13.6, the
/// interface's acceptance shape). Injected into [`DriveCtx`] so [`ClaudeChannelDrive`] is
/// testable against a scripted claude, never a real process.
///
/// [`ClaudeChannelDrive`]: crate::claude::ClaudeChannelDrive
pub trait ClaudeChannelClient {
    /// The running session id (from the `system/init` event), for `Target`/resume; `None`
    /// until it's been observed.
    fn session_id(&self) -> Option<String>;
    /// Inject one user message; returns a streaming [`TurnHandle`] for that turn.
    fn start_turn(&self, content: &str) -> Result<Box<dyn TurnHandle>, DriveError>;
}

/// Steer a running (or one-shot) AI tool. `run` is the only required method.
pub trait Drive {
    /// Declarative identity (the transport tag).
    fn info(&self) -> DriveInfo;

    /// Send one prompt/turn to `target`, returning a handle the host observes. May
    /// start a process (spawn shapes) or POST to a running session (channel shapes).
    fn run(
        &self,
        target: &Target,
        content: &str,
        ctx: &DriveCtx,
    ) -> Result<Box<dyn TurnHandle>, DriveError>;

    /// Whether a running turn can be interrupted (via its `TurnHandle`). Absence is the
    /// signal — a tool that can't declares `false` and nothing calls it.
    fn supports_interrupt(&self) -> bool {
        false
    }

    /// The long-lived [`BackingService`] this drive depends on, if any. A generic caller
    /// warms/health-checks it **without importing the tool's daemon module** (closes the
    /// `docs/13` §13.5 leak by construction). Spawn/channel shapes own their process per
    /// turn and return the `None` default; the app-server shape returns its daemon.
    fn service(&self) -> Option<&dyn BackingService> {
        None
    }
}

/// A persistent process a drive connects to across turns (e.g. the Codex `app-server`
/// daemon), rather than spawning per turn. Exposed through [`Drive::service`] so a
/// tool-id-free caller can prestart/health-check it without depending on a concrete
/// tool's daemon module (`docs/13` §13.5). Live-only: the fakes used in tests return a
/// no-op service or `None`.
pub trait BackingService {
    /// Stable identity for logs/metrics (e.g. `"codex-app-server"`).
    fn name(&self) -> &str;

    /// Idempotently start the service if it is not already running, so the next
    /// connection succeeds. Cheap to call repeatedly — HS1's `daemon start` semantics.
    fn prestart(&self) -> std::io::Result<()>;
}
