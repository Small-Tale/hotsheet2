//! Best-effort halt detection for Codex TUIs in Hot Sheet terminals (HS2-MN865E).
//!
//! When a terminal's Codex session reports its thread id, the server rejoins that thread on
//! the shared app-server daemon ([`hotsheet_aitools::codex_thread_watch`]) and maps a failed
//! turn to the same terminal halt Claude's `StopFailure` hook reports. A new turn clears it.
//! One watch runs per terminal. It is replaced when a newer session reports and dropped on
//! `SessionEnd`, terminal kill, or shell exit; dropping it closes the daemon connection.

use super::signals::{clear_terminal_halt_on_resume, record_terminal_halt};
use crate::{AppState, now};
use hotsheet_aitools::codex_thread_watch::{ThreadSignal, ThreadWatch, watch_thread};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::metrics;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The live watch for one terminal's Codex thread.
pub struct CodexThreadWatch {
    session_id: String,
    generation: u64,
    _watch: ThreadWatch,
}

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// Start (or keep) the watch matching terminal `id`'s reported connection: a Codex session
/// with a thread id is watched; anything else stops a previous watch.
pub(crate) fn sync_codex_thread_watch(
    state: &AppState,
    id: &str,
    agent: Option<&str>,
    session_id: Option<&str>,
    codex_home: Option<&str>,
) {
    // Codex thread ids are UUIDs; anything else cannot name a daemon thread.
    let Some(session_id) = session_id.filter(|id| agent == Some("codex") && is_uuid(id)) else {
        stop_codex_thread_watch(state, id);
        return;
    };
    if state
        .codex_thread_watches
        .lock_or_recover()
        .get(id)
        .is_some_and(|watch| watch.session_id == session_id)
    {
        return;
    }
    stop_codex_thread_watch(state, id);
    let home = codex_home
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_codex_home());
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    let (state, id, session_id) = (state.clone(), id.to_owned(), session_id.to_owned());
    // Connecting performs a WebSocket handshake; keep it off the request path.
    std::thread::spawn(move || {
        let signal_state = state.clone();
        let (signal_id, signal_session) = (id.clone(), session_id.clone());
        let turn_ticket = Arc::new(Mutex::new(None::<String>));
        let signal_ticket = turn_ticket.clone();
        let end_state = state.clone();
        let end_id = id.clone();
        let watch = watch_thread(
            &home,
            &session_id,
            move |signal| {
                // A late notification from a replaced or ended session changes nothing.
                if !is_current(&signal_state, &signal_id, &signal_session) {
                    return;
                }
                match signal {
                    ThreadSignal::Halted {
                        error_type,
                        message,
                    } => record_terminal_halt(
                        &signal_state,
                        &signal_id,
                        Some(error_type),
                        Some(message),
                        Some("codex".to_owned()),
                    ),
                    ThreadSignal::Resumed => {
                        clear_terminal_halt_on_resume(&signal_state, &signal_id);
                        let ai_worker = hotsheet_aitools::session_worker_id("codex", &signal_id);
                        let shell_worker = super::launch::terminal_session_worker_id(&signal_id);
                        let ticket = metrics::active_ticket_for_workers(
                            &signal_state.store,
                            &[&ai_worker, &shell_worker],
                            &now(),
                        )
                        .unwrap_or_else(|error| {
                            tracing::warn!(%error, "looking up Codex terminal ticket failed");
                            None
                        });
                        *signal_ticket.lock_or_recover() = ticket;
                    }
                    ThreadSignal::Usage(usage) => {
                        let event = metrics::UsageEvent {
                            ts: now().as_str().to_owned(),
                            tool: "codex".to_owned(),
                            model: usage.model,
                            tokens_in: usage.tokens_in,
                            tokens_out: usage.tokens_out,
                            cost_usd: usage.cost_usd,
                            ticket: signal_ticket.lock_or_recover().take(),
                            session: Some(signal_session.clone()),
                        };
                        if let Err(error) = metrics::record_priced(&signal_state.store, event) {
                            tracing::warn!(%error, "recording Codex terminal usage failed");
                        }
                    }
                }
            },
            move || {
                let mut watches = end_state.codex_thread_watches.lock_or_recover();
                if watches
                    .get(&end_id)
                    .is_some_and(|watch| watch.generation == generation)
                {
                    watches.remove(&end_id);
                }
            },
        );
        // No daemon socket (a `--no-daemon` TUI) or a failed handshake: nothing to watch.
        let Ok(watch) = watch else { return };
        if !is_current(&state, &id, &session_id) {
            return; // replaced or ended while connecting; dropping `watch` disconnects
        }
        state.codex_thread_watches.lock_or_recover().insert(
            id,
            CodexThreadWatch {
                session_id,
                generation,
                _watch: watch,
            },
        );
    });
}

/// Drop terminal `id`'s watch, closing its daemon connection.
pub(crate) fn stop_codex_thread_watch(state: &AppState, id: &str) {
    let removed = state.codex_thread_watches.lock_or_recover().remove(id);
    drop(removed);
}

fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Whether terminal `id`'s reported connection is still the Codex session `session_id`.
fn is_current(state: &AppState, id: &str, session_id: &str) -> bool {
    state
        .terminal_ai_connections
        .lock_or_recover()
        .get(id)
        .is_some_and(|connection| {
            connection.agent.as_deref() == Some("codex")
                && connection.session_id.as_deref() == Some(session_id)
        })
}
