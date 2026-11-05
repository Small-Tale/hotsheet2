//! Halt detection and per-ticket usage for Codex TUIs in Hot Sheet terminals (HS2-CYHAWB).
//!
//! Hot Sheet's Codex terminals host their own session (`--no-daemon`, HS2-4J7WCV), so the
//! server follows the session's own rollout file under the reported `CODEX_HOME`
//! ([`hotsheet_aitools::codex_rollout_watch`]). A turn that completed with an error records the
//! same terminal halt Claude's `StopFailure` hook reports; the next turn clears it. Each model
//! response's numeric usage is recorded once, attributed to the terminal session's unique live
//! claim at turn start.
//!
//! This replaces the read-only daemon thread rejoin of HS2-MN865E, which no Hot Sheet terminal
//! reached after HS2-4J7WCV and whose `thread/resume` could load a second copy of an
//! in-process thread into a user's own daemon. The server never connects to a Codex daemon.
//!
//! One watch runs per terminal. A newer session replaces it; `SessionEnd`, terminal kill, or
//! shell exit drops it, which stops its watcher.

use super::signals::{clear_terminal_halt_on_resume, record_terminal_halt};
use crate::{AppState, now};
use hotsheet_aitools::codex_rollout_watch::{RolloutSignal, RolloutWatch, watch_rollout};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::metrics;
use std::path::PathBuf;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The live rollout watch for one terminal's Codex session.
pub struct CodexSessionWatch {
    session_id: String,
    _watch: RolloutWatch,
}

/// Start (or keep) the watch matching terminal `id`'s reported connection: a Codex session
/// with a UUID session id is watched; anything else stops a previous watch. `reported_at` is
/// the connection's RFC 3339 report time; only rollout records from then on count.
pub(crate) fn sync_codex_session_watch(
    state: &AppState,
    id: &str,
    agent: Option<&str>,
    session_id: Option<&str>,
    codex_home: Option<&str>,
    reported_at: &str,
) {
    // Codex session ids are UUIDs; anything else cannot name a rollout.
    let Some(session_id) = session_id.filter(|id| agent == Some("codex") && is_uuid(id)) else {
        stop_codex_session_watch(state, id);
        return;
    };
    if state
        .codex_session_watches
        .lock_or_recover()
        .get(id)
        .is_some_and(|watch| watch.session_id == session_id)
    {
        return;
    }
    stop_codex_session_watch(state, id);
    let home = codex_home
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_codex_home());
    let live_after =
        OffsetDateTime::parse(reported_at, &Rfc3339).unwrap_or_else(|_| OffsetDateTime::now_utc());
    let signal_state = state.clone();
    let (signal_id, signal_session) = (id.to_owned(), session_id.to_owned());
    // The claim snapshot of the current turn; a turn already running when the session
    // reported takes its snapshot at its first response.
    let mut turn_ticket: Option<Option<String>> = None;
    let watch = watch_rollout(&home, session_id, live_after, move |signals| {
        // A late batch from a replaced or ended session changes nothing.
        if !is_current(&signal_state, &signal_id, &signal_session) {
            return;
        }
        let mut responses = Vec::new();
        for signal in signals {
            match signal {
                RolloutSignal::Halted {
                    error_type,
                    message,
                } => record_terminal_halt(
                    &signal_state,
                    &signal_id,
                    Some(error_type),
                    Some(message),
                    Some("codex".to_owned()),
                ),
                RolloutSignal::Resumed => {
                    clear_terminal_halt_on_resume(&signal_state, &signal_id);
                    turn_ticket = Some(terminal_ticket(&signal_state, &signal_id));
                }
                RolloutSignal::Usage(usage) => responses.push((
                    usage.response_id,
                    metrics::UsageEvent {
                        ts: now().as_str().to_owned(),
                        tool: "codex".to_owned(),
                        model: usage.model,
                        tokens_in: usage.tokens_in,
                        tokens_out: usage.tokens_out,
                        cost_usd: None,
                        ticket: turn_ticket
                            .get_or_insert_with(|| terminal_ticket(&signal_state, &signal_id))
                            .clone(),
                        session: None,
                    },
                )),
            }
        }
        if let Err(error) = metrics::codex_work::record_session_responses(
            &signal_state.store,
            &signal_session,
            responses,
        ) {
            tracing::warn!(%error, "recording Codex terminal usage failed");
        }
        // Tokens per minute moved (HS2-JTTQMY).
        crate::live_state::touch(
            &signal_state,
            &crate::multistore::store_url_id(&signal_state.store),
        );
    });
    // No Codex home, or it cannot be watched: nothing to follow.
    let Ok(watch) = watch else { return };
    state.codex_session_watches.lock_or_recover().insert(
        id.to_owned(),
        CodexSessionWatch {
            session_id: session_id.to_owned(),
            _watch: watch,
        },
    );
}

/// Drop terminal `id`'s watch, stopping its watcher.
pub(crate) fn stop_codex_session_watch(state: &AppState, id: &str) {
    let removed = state.codex_session_watches.lock_or_recover().remove(id);
    drop(removed);
}

/// The ticket the terminal's Codex or shell session worker uniquely holds now, if any.
fn terminal_ticket(state: &AppState, id: &str) -> Option<String> {
    let ai_worker = hotsheet_aitools::session_worker_id("codex", id);
    let shell_worker = super::launch::terminal_session_worker_id(id);
    metrics::active_ticket_for_workers(&state.store, &[&ai_worker, &shell_worker], &now())
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "looking up Codex terminal ticket failed");
            None
        })
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

#[cfg(test)]
mod tests {
    #[test]
    fn hs2_hba9z8_synthetic_session_does_not_name_a_rollout() {
        // The attribution fixture reports a hook without naming a real Codex session.
        // Keep its synthetic id outside the UUID boundary checked before home discovery.
        assert!(!super::is_uuid("synthetic-session"));
        assert!(super::is_uuid("00000000-0000-0000-0000-000000000001"));
    }
}
