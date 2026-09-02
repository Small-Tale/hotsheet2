//! Wire a plugin's declared drive to a **real** running tool and drive one turn,
//! streaming its events (HS2-109 — the reachability behind `hotsheet-cli trigger`). The
//! transport/protocol logic is fake-tested in each drive module (`docs/05` §5.10), so
//! this file is only the effectful glue: spawn the real process (with HS2-103 isolation),
//! build the [`DriveCtx`], run [`trigger`], and stream to completion.

use std::path::PathBuf;

use hotsheet_plugins::Plugin;

use crate::acp::{AcpSession, AcpStdio};
use crate::claude::{ClaudeChannel, ClaudeStreamTransport};
use crate::codex::{
    CodexAppServer, StdioTransport, UdsWsTransport, codex_control_socket_path,
    ensure_codex_daemon_in,
};
use crate::drive::{DoneReason, DriveCtx, TurnEvent};
use crate::host::{TriggerError, trigger};
use crate::registry::{ConnectionRegistry, Role};
use crate::system::SystemSpawner;

/// What to launch, and how to keep it from reaching HS1 (HS2-103 safety).
pub struct LiveTrigger {
    /// The project directory the tool runs in.
    pub cwd: PathBuf,
    /// The prompt/turn content (e.g. "work the top Up Next ticket").
    pub prompt: String,
    pub role: Role,
    /// Caller-minted connection id (session id).
    pub conn_id: String,
    /// Resume a prior session (channel `--resume`), if any.
    pub resume: Option<String>,
    /// Restrict a channel tool to only this MCP config (`--strict-mcp-config`), so it can
    /// reach the Hot Sheet shim but nothing else (ignored by other transports).
    pub mcp_config: Option<PathBuf>,
    /// Claude permission mode for headless work (e.g. `"acceptEdits"`); the real bridge is
    /// HS2-113.
    pub permission_mode: Option<String>,
    /// Extra env for the launched process (e.g. an isolated `CODEX_HOME`).
    pub env: Vec<(String, String)>,
    /// For an app-server tool: drive the **shared daemon** for this connection's
    /// `CODEX_HOME` (one codex instance reused across turns) instead of spawning a fresh
    /// `app-server` process per connection (HS2-B7C66H). Requires `CODEX_HOME` in `env`.
    pub shared_daemon: bool,
    /// A live permission bridge to **block approvals on a human** (HS2-Q1F6HV). `None` =
    /// auto-approve (the headless default; a bare CLI run has no human UI). Only the
    /// app-server (codex) transport consults it today.
    pub permission_bridge: Option<std::sync::Arc<crate::permission::SharedPermissionBridge>>,
    /// Injected clock.
    pub now_ms: u64,
}

/// The result of one driven turn: why it ended, plus the tool's session/thread id (when it
/// reports one) so the next turn can resume the same session (HS2-3C1XK3).
#[derive(Debug, Clone)]
pub struct TurnDone {
    pub reason: DoneReason,
    pub session_id: Option<String>,
}

/// A failed live trigger.
#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    #[error("'{0}' is not drivable (no [drive], or its transport isn't built yet)")]
    NotDrivable(String),
    #[error("launching '{program}': {source}")]
    Launch {
        program: String,
        source: std::io::Error,
    },
    #[error("connecting to '{0}': {1}")]
    Connect(String, String),
    #[error(transparent)]
    Drive(#[from] crate::drive::DriveError),
}

/// Drive one real turn of `plugin` for the project, invoking `on_event` for each streamed
/// event (including the terminal `Done`), and return why it finished. The connection is
/// registered busy in `registry` and set idle when the turn ends.
pub fn run_trigger(
    plugin: &Plugin,
    t: &LiveTrigger,
    registry: &mut ConnectionRegistry,
    on_event: &mut dyn FnMut(&TurnEvent),
) -> Result<TurnDone, LiveError> {
    let spec = plugin
        .manifest
        .drive
        .as_ref()
        .ok_or_else(|| LiveError::NotDrivable(plugin.id().to_string()))?;
    let program = spec.program.clone();
    let spawner = SystemSpawner;

    // Build the tool-specific live connection, then a DriveCtx, then drive within scope so
    // the connection outlives the turn.
    match spec.transport.as_str() {
        "claude-channel" => {
            let transport = ClaudeStreamTransport::spawn(
                &program,
                &t.cwd,
                t.resume.as_deref(),
                t.mcp_config.as_deref(),
                t.permission_mode.as_deref(),
                &t.env,
            )
            .map_err(|source| LiveError::Launch {
                program: program.clone(),
                source,
            })?;
            let channel = ClaudeChannel::connect(transport);
            let ctx = DriveCtx {
                cwd: t.cwd.clone(),
                spawner: &spawner,
                env: t.env.clone(),
                app_server: None,
                channel: Some(&channel),
                acp: None,
            };
            drive_and_stream(plugin, t, &ctx, registry, on_event)
        }
        "app-server" => {
            // Two shapes of the persistent app-server (docs/13 §13.5):
            //  - default: spawn `codex app-server` direct — one process for this connection;
            //  - shared_daemon: attach to the daemon for this connection's CODEX_HOME over a
            //    WebSocket, so many connections/turns reuse ONE codex instance (HS2-B7C66H).
            let app = if t.shared_daemon {
                connect_shared_daemon(&program, &t.env)?
            } else {
                let transport =
                    StdioTransport::spawn(&program, &t.cwd, &t.env).map_err(|source| {
                        LiveError::Launch {
                            program: program.clone(),
                            source,
                        }
                    })?;
                CodexAppServer::connect(transport)
                    .map_err(|e| LiveError::Connect(program.clone(), e.to_string()))?
            };
            // Attach the live permission bridge (if any): approvals now BLOCK the turn for a
            // human answering over the server route-back instead of auto-approving
            // (HS2-Q1F6HV). Without one, the isolated headless default (auto-approve) holds.
            if let Some(bridge) = &t.permission_bridge {
                app.set_permission_policy(crate::codex::PermissionPolicy {
                    bridge: bridge.clone(),
                    connection: t.conn_id.clone(),
                    default: crate::permission::Decision::Deny,
                    timeout: crate::permission::DEFAULT_PERMISSION_TIMEOUT,
                });
            }
            let ctx = DriveCtx {
                cwd: t.cwd.clone(),
                spawner: &spawner,
                env: t.env.clone(),
                app_server: Some(&app),
                channel: None,
                acp: None,
            };
            drive_and_stream(plugin, t, &ctx, registry, on_event)
        }
        "acp" => {
            let transport =
                AcpStdio::spawn(&program, &t.cwd, &t.env).map_err(|source| LiveError::Launch {
                    program: program.clone(),
                    source,
                })?;
            let acp = AcpSession::connect(transport).map_err(LiveError::Drive)?;
            let ctx = DriveCtx {
                cwd: t.cwd.clone(),
                spawner: &spawner,
                env: t.env.clone(),
                app_server: None,
                channel: None,
                acp: Some(&acp),
            };
            drive_and_stream(plugin, t, &ctx, registry, on_event)
        }
        "spawn" => {
            let ctx = DriveCtx {
                cwd: t.cwd.clone(),
                spawner: &spawner,
                env: t.env.clone(),
                app_server: None,
                channel: None,
                acp: None,
            };
            drive_and_stream(plugin, t, &ctx, registry, on_event)
        }
        _ => Err(LiveError::NotDrivable(plugin.id().to_string())),
    }
}

/// Attach to the shared codex daemon for the `CODEX_HOME` in `env`: start it (idempotent)
/// and connect its WebSocket control socket. Requires `CODEX_HOME` to be set — the caller
/// (`hotsheet-cli`) points it at a **daemon-ready isolated home** so MCP isolation holds
/// while one instance is reused (HS2-B7C66H).
fn connect_shared_daemon(
    program: &str,
    env: &[(String, String)],
) -> Result<CodexAppServer, LiveError> {
    let codex_home = env
        .iter()
        .find(|(k, _)| k == "CODEX_HOME")
        .map(|(_, v)| PathBuf::from(v))
        .ok_or_else(|| {
            LiveError::Connect(
                program.to_string(),
                "--shared-daemon needs a CODEX_HOME (none in env)".into(),
            )
        })?;
    ensure_codex_daemon_in(program, &codex_home).map_err(|source| LiveError::Launch {
        program: program.to_string(),
        source,
    })?;
    let socket = codex_control_socket_path(&codex_home);
    let transport = UdsWsTransport::connect(&socket).map_err(|source| LiveError::Launch {
        program: program.to_string(),
        source,
    })?;
    CodexAppServer::connect(transport)
        .map_err(|e| LiveError::Connect(program.to_string(), e.to_string()))
}

fn drive_and_stream(
    plugin: &Plugin,
    t: &LiveTrigger,
    ctx: &DriveCtx,
    registry: &mut ConnectionRegistry,
    on_event: &mut dyn FnMut(&TurnEvent),
) -> Result<TurnDone, LiveError> {
    let project = t.cwd.display().to_string();
    let out = trigger(
        plugin,
        &project,
        t.role,
        t.conn_id.clone(),
        &t.prompt,
        ctx,
        registry,
        t.now_ms,
    )
    .map_err(|e| match e {
        TriggerError::NotDrivable(id) => LiveError::NotDrivable(id),
        TriggerError::Drive(d) => LiveError::Drive(d),
    })?;

    let conn_id = out.connection_id.clone();
    let mut turn = out.turn;
    // Heartbeat the connection's busy state at a **live** clock so an external observer
    // (the server drive loop / a UI) sees `is_busy` track the running turn, not a stamp
    // frozen at the turn's start (HS2-34X6BW). Anchored at `now_ms` (wall-clock epoch ms in
    // production) + elapsed, so it stays on the same time base an observer queries with.
    let start = std::time::Instant::now();
    let base = t.now_ms;
    let mut clock = move || base + start.elapsed().as_millis() as u64;
    // A native payload is observable only when the plugin declares the activity
    // capability. The manifest's source is authoritative, keeping custom plugins and
    // capability omission honest even when they reuse a built-in transport.
    let activity_source = plugin.manifest.activity.as_ref().map(|spec| &spec.source);
    let mut capability_event = |event: &TurnEvent| match event {
        TurnEvent::NativeActivity { payload, .. } => {
            if let Some(source) = activity_source {
                on_event(&TurnEvent::NativeActivity {
                    source: source.clone(),
                    payload: payload.clone(),
                });
            }
        }
        _ => on_event(event),
    };
    let reason = pump_turn(
        turn.as_mut(),
        &conn_id,
        registry,
        &mut clock,
        &mut capability_event,
    );
    // Surface the session/thread id so the caller can resume the same session next turn
    // (HS2-3C1XK3): claude reports it from `system/init`, codex from `thread/start|resume`.
    let session_id = ctx
        .channel
        .and_then(|c| c.session_id())
        .or_else(|| ctx.app_server.and_then(|a| a.session_id()));
    Ok(TurnDone { reason, session_id })
}

/// Drain a turn's streaming events, heartbeating the connection busy at each one (via
/// `clock`, so an external observer's `is_busy` tracks the live turn) and setting it idle
/// on the terminal `Done` (`docs/13` §13.4, HS2-34X6BW). A non-streaming drive (spawn /
/// app-server: no events) falls back to the terminal wait — it registered busy at trigger
/// time and goes idle here. Pure over the injected `clock`, so the busy feed is testable
/// with a fake `TurnHandle` + a scripted clock, no real tool.
fn pump_turn(
    turn: &mut dyn crate::drive::TurnHandle,
    conn_id: &str,
    registry: &mut ConnectionRegistry,
    clock: &mut dyn FnMut() -> u64,
    on_event: &mut dyn FnMut(&TurnEvent),
) -> DoneReason {
    let reason = loop {
        match turn.next_event() {
            Some(TurnEvent::Done(r)) => break r,
            Some(ev) => {
                on_event(&ev);
                registry.note_activity(conn_id, clock());
            }
            // Non-streaming drives (spawn / app-server): no events, just the terminal wait.
            None => break turn.wait(),
        }
    };
    // Surface any usage the turn reported (codex), just before the terminal Done, so the
    // host can record a UsageEvent (HS2-0WCRZY).
    if let Some(usage) = turn.usage() {
        on_event(&TurnEvent::Usage(usage));
    }
    on_event(&TurnEvent::Done(reason));
    registry.set_idle(conn_id);
    reason
}

#[cfg(test)]
mod pump_tests {
    use super::*;
    use crate::drive::{DoneReason, Transport, TurnEvent, TurnHandle};
    use crate::registry::{Connection, ConnectionRegistry, Role};

    /// A streaming TurnHandle that yields a scripted queue of events (ending in `Done`).
    struct ScriptedTurn {
        events: std::collections::VecDeque<TurnEvent>,
    }
    impl TurnHandle for ScriptedTurn {
        fn is_busy(&mut self) -> bool {
            !self.events.is_empty()
        }
        fn wait(&mut self) -> DoneReason {
            DoneReason::Completed
        }
        fn next_event(&mut self) -> Option<TurnEvent> {
            self.events.pop_front()
        }
    }

    fn reg_with(conn: &str) -> ConnectionRegistry {
        let mut r = ConnectionRegistry::new(5_000);
        r.register(Connection {
            id: conn.into(),
            project: "/p".into(),
            tool: "fake".into(),
            role: Role::Main,
            transport: Transport::ClaudeChannel,
            pid: None,
            started_at_ms: 0,
        });
        r
    }

    #[test]
    fn pump_surfaces_turn_usage_just_before_done() {
        use crate::drive::Usage;
        // A non-streaming turn (spawn/app-server shape) that reports usage: pump falls back
        // to wait(), then emits a Usage event before the terminal Done (HS2-0WCRZY).
        struct UsageTurn {
            usage: Option<Usage>,
        }
        impl TurnHandle for UsageTurn {
            fn is_busy(&mut self) -> bool {
                false
            }
            fn wait(&mut self) -> DoneReason {
                DoneReason::Completed
            }
            fn usage(&mut self) -> Option<Usage> {
                self.usage.take()
            }
        }
        let mut turn = UsageTurn {
            usage: Some(Usage {
                model: Some("gpt".into()),
                tokens_in: 100,
                tokens_out: 20,
                cost_usd: None,
            }),
        };
        let mut reg = reg_with("c1");
        let mut clock = || 0u64;
        let mut seen = Vec::new();
        pump_turn(&mut turn, "c1", &mut reg, &mut clock, &mut |ev| {
            seen.push(format!("{ev:?}"))
        });
        // Usage is emitted, and it comes immediately before Done.
        let usage_idx = seen
            .iter()
            .position(|s| s.starts_with("Usage"))
            .expect("usage emitted");
        assert!(
            seen[usage_idx + 1].starts_with("Done"),
            "usage precedes Done"
        );
        assert!(seen[usage_idx].contains("100") && seen[usage_idx].contains("20"));
    }

    #[test]
    fn pump_heartbeats_once_per_streamed_event_at_the_live_clock() {
        // Three output events then Done. A live clock advances 1s per event; each streamed
        // (non-Done) event must heartbeat exactly once at the *current* time — not a frozen
        // start stamp — so an external observer's `is_busy` tracks the running turn.
        let mut turn = ScriptedTurn {
            events: [
                TurnEvent::Output("a".into()),
                TurnEvent::Output("b".into()),
                TurnEvent::Output("c".into()),
                TurnEvent::Done(DoneReason::Completed),
            ]
            .into_iter()
            .collect(),
        };
        let mut reg = reg_with("c1");
        // Record every clock read: proves one live-clock heartbeat per non-Done event.
        let reads = std::cell::RefCell::new(Vec::new());
        let ticks = std::cell::Cell::new(0u64);
        let mut clock = || {
            let t = 1_000 + ticks.get() * 1_000;
            ticks.set(ticks.get() + 1);
            reads.borrow_mut().push(t);
            t
        };
        let mut seen = Vec::new();

        let reason = pump_turn(&mut turn, "c1", &mut reg, &mut clock, &mut |ev| {
            seen.push(format!("{ev:?}"))
        });

        assert_eq!(reason, DoneReason::Completed);
        // Exactly one heartbeat per streamed output event, at the advancing clock (not a
        // single frozen `now_ms`); Done doesn't heartbeat.
        assert_eq!(reads.borrow().as_slice(), &[1_000, 2_000, 3_000]);
        // Every event (including the terminal Done) was surfaced to the observer, in order.
        assert_eq!(
            seen,
            vec![
                "Output(\"a\")",
                "Output(\"b\")",
                "Output(\"c\")",
                "Done(Completed)"
            ]
        );
        // Done idled the connection immediately (past the 3_000 heartbeat's window anyway).
        assert!(!reg.is_busy("c1", 3_100), "idle after Done");
    }

    #[test]
    fn done_sets_idle_immediately_even_inside_the_busy_window() {
        // One event heartbeat at 1_000, then Done — set_idle must win over the window.
        let mut turn = ScriptedTurn {
            events: [
                TurnEvent::Output("x".into()),
                TurnEvent::Done(DoneReason::Completed),
            ]
            .into_iter()
            .collect(),
        };
        let mut reg = reg_with("c1");
        let mut clock = || 1_000u64;
        let _ = pump_turn(&mut turn, "c1", &mut reg, &mut clock, &mut |_| {});
        // 1_100 is well inside the 5s window of the 1_000 heartbeat, yet Done idled it.
        assert!(!reg.is_busy("c1", 1_100), "Done drops busy immediately");
    }

    #[test]
    fn a_non_streaming_turn_falls_back_to_wait_and_idles() {
        // No events (spawn / app-server shape): pump waits, then idles. The clock is never
        // called (nothing to heartbeat) — the connection was marked busy at trigger time.
        struct SilentTurn;
        impl TurnHandle for SilentTurn {
            fn is_busy(&mut self) -> bool {
                false
            }
            fn wait(&mut self) -> DoneReason {
                DoneReason::Failed(3)
            }
        }
        let mut reg = reg_with("c1");
        reg.note_activity("c1", 1_000); // trigger-time busy
        let reason = pump_turn(
            &mut SilentTurn,
            "c1",
            &mut reg,
            &mut || panic!("a non-streaming turn must not heartbeat"),
            &mut |_| {},
        );
        assert_eq!(reason, DoneReason::Failed(3));
        assert!(!reg.is_busy("c1", 1_100), "idled on the terminal wait");
    }
}
