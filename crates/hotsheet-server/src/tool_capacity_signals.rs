//! HS2-AHG7HW: the server's existing detections as tool-capacity signals
//! (`docs/26-orchestration-projects.md` §26.11).
//!
//! Nothing here reads a provider message a second way. A halt is a limit exactly when
//! [`crate::stuck_causes::is_usage_limit`] says so, and its reset time is exactly what
//! [`crate::stuck_causes::reported_reset`] reads. The signal keeps the tool id, the
//! kind and the times; the message itself never enters it.

use hotsheet_ticketing::metrics::UsageEvent;
use hotsheet_ticketing::orchestration::{LimitSignal, SignalKind};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::AppState;
use crate::live_state::{LiveCause, LiveCauseKind};

fn unix_ms(at: OffsetDateTime) -> Option<u64> {
    u64::try_from(at.unix_timestamp_nanos() / 1_000_000).ok()
}

/// A bounded identifier or nothing: an id of another shape is dropped, never rewritten.
fn identifier(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    .then(|| value.to_owned())
}

fn signal(tool: &str, kind: SignalKind, at: OffsetDateTime) -> Option<LimitSignal> {
    Some(LimitSignal {
        tool: identifier(tool)?,
        account: None,
        kind,
        at_ms: unix_ms(at)?,
        resets_at_ms: None,
        cost_cents: None,
        session: None,
    })
}

/// The limit signal for a halted turn, or `None` when the halt is not a usage or rate
/// limit. A request-rate error type keeps its own kind; every other limit is a usage
/// limit. The reset time is present only when the message states one.
pub fn halt_signal(
    tool: &str,
    error_type: &str,
    message: &str,
    halted_at: OffsetDateTime,
) -> Option<LimitSignal> {
    if !crate::stuck_causes::is_usage_limit(error_type) {
        return None;
    }
    let kind = if error_type.trim().to_ascii_lowercase().starts_with("rate_") {
        SignalKind::RateLimit
    } else {
        SignalKind::UsageLimit
    };
    Some(LimitSignal {
        resets_at_ms: crate::stuck_causes::reported_reset(message, halted_at).and_then(unix_ms),
        ..signal(tool, kind, halted_at)?
    })
}

/// The limit signal for a live-state usage-limit cause. The cause no longer says which
/// limit it was, so it maps to a usage limit.
pub fn cause_signal(tool: &str, cause: &LiveCause) -> Option<LimitSignal> {
    if cause.kind != LiveCauseKind::UsageLimit {
        return None;
    }
    Some(LimitSignal {
        tool: identifier(tool)?,
        account: None,
        kind: SignalKind::UsageLimit,
        at_ms: u64::try_from(cause.since_ms).ok()?,
        resets_at_ms: cause.resets_at_ms.and_then(|at| u64::try_from(at).ok()),
        cost_cents: None,
        session: None,
    })
}

/// A session that was halted on a limit took a new turn.
pub fn recovered_signal(tool: &str, at: OffsetDateTime) -> Option<LimitSignal> {
    signal(tool, SignalKind::Recovered, at)
}

/// A completed turn's usage event. A cost is carried only when the event has one;
/// fractions of a cent round up so a budget is never understated.
pub fn usage_signal(event: &UsageEvent) -> Option<LimitSignal> {
    let at = OffsetDateTime::parse(&event.ts, &Rfc3339).ok()?;
    let cost_cents = event
        .cost_usd
        .filter(|usd| usd.is_finite() && *usd >= 0.0)
        .map(|usd| (usd * 100.0).ceil())
        .filter(|cents| *cents <= 1e15)
        .map(|cents| cents as u64);
    Some(LimitSignal {
        cost_cents,
        session: event.session.as_deref().and_then(identifier),
        ..signal(&event.tool, SignalKind::Usage, at)?
    })
}

/// Record one observed signal in this machine's capacity registry. Capacity is
/// advisory to the session that produced it: a refused or failed record is logged by
/// code and never fails the halt or resume that carried it.
pub(crate) fn record(state: &AppState, signal: Option<LimitSignal>) {
    let Some(signal) = signal else {
        return;
    };
    let Some(now_ms) = unix_ms(OffsetDateTime::now_utc()) else {
        return;
    };
    if let Err(error) = hotsheet_cli::orchestration::OrchestrationRuntime::record_tool_signal_at(
        state.machine_home(),
        &signal,
        now_ms,
    ) {
        tracing::warn!(
            tool = %signal.tool,
            kind = signal.kind.code(),
            code = error.code(),
            "recording a tool capacity signal failed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-07T01:00:00Z.
    fn halted() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_794_013_200).unwrap()
    }
    const AT_MS: u64 = 1_794_013_200_000;

    /// One fixture per signal a bundled plugin declares it can report: the error type
    /// its halt source sends and a synthetic message in the provider's shape.
    #[test]
    fn hs2_ahg7hw_each_declared_limit_error_maps_to_its_signal_and_reset_time() {
        let fixtures = [
            // Claude's StopFailure hook: one error type for rate and usage limits.
            (
                "claude",
                "rate_limit",
                "Usage limit reached. Resets in 2h 15m.",
                Some((SignalKind::RateLimit, Some(135 * 60_000))),
            ),
            (
                "claude",
                "rate_limit",
                "Claude AI usage limit reached|1794020400",
                Some((SignalKind::RateLimit, Some(2 * 3_600_000))),
            ),
            (
                "claude",
                "rate_limit",
                "Usage limit reached.",
                Some((SignalKind::RateLimit, None)),
            ),
            // Codex's rollout: a failed turn's `codex_error_info`.
            (
                "codex",
                "usage_limit_exceeded",
                "You've hit your usage limit. Try again at 3:10 AM UTC.",
                Some((SignalKind::UsageLimit, Some(130 * 60_000))),
            ),
            (
                "codex",
                "usage_limit_exceeded",
                "You've hit your usage limit.",
                Some((SignalKind::UsageLimit, None)),
            ),
            (
                "codex",
                " Usage_Limit_Exceeded ",
                "Try again in 45 minutes.",
                Some((SignalKind::UsageLimit, Some(45 * 60_000))),
            ),
            // Halts that are not limits produce no signal for any tool.
            (
                "claude",
                "overloaded",
                "Selected model is at capacity.",
                None,
            ),
            ("claude", "billing_error", "Resets in 2h.", None),
            (
                "codex",
                "server_overloaded",
                "Try again in 5 minutes.",
                None,
            ),
            (
                "opencode",
                "unknown",
                "The AI session stopped on an error.",
                None,
            ),
            ("antigravity", "", "", None),
        ];
        for (tool, error_type, message, expected) in fixtures {
            let found = halt_signal(tool, error_type, message, halted());
            assert_eq!(
                found
                    .as_ref()
                    .map(|signal| (signal.kind, signal.resets_at_ms.map(|at| at - AT_MS))),
                expected,
                "{tool} {error_type} {message}"
            );
            if let Some(found) = found {
                assert_eq!((found.tool.as_str(), found.at_ms), (tool, AT_MS));
                assert_eq!(
                    (&found.account, found.cost_cents, &found.session),
                    (&None, None, &None)
                );
                found.validate(AT_MS).unwrap();
                // The message is used to read a time and then dropped.
                let wire = serde_json::to_string(&found).unwrap();
                assert!(!wire.contains("limit reached") && !wire.contains("Try again"));
            }
        }
        // A tool id that is not a bounded identifier yields no signal.
        assert_eq!(halt_signal("two words", "rate_limit", "x", halted()), None);
        assert_eq!(halt_signal("", "rate_limit", "x", halted()), None);
    }

    #[test]
    fn hs2_ahg7hw_live_causes_recoveries_and_usage_events_map_to_signals() {
        let cause = |kind, resets_at_ms| LiveCause {
            kind,
            worker: "terminal-a".into(),
            terminal: Some("a".into()),
            since_ms: AT_MS as i64,
            resets_at_ms,
            prompt: None,
        };
        let limited = cause_signal(
            "codex",
            &cause(LiveCauseKind::UsageLimit, Some(AT_MS as i64 + 60_000)),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&limited).unwrap(),
            serde_json::json!({"tool":"codex","kind":"usage_limit","at_ms":AT_MS,
                "resets_at_ms":AT_MS + 60_000})
        );
        assert_eq!(
            cause_signal("codex", &cause(LiveCauseKind::UsageLimit, None))
                .unwrap()
                .resets_at_ms,
            None
        );
        assert_eq!(
            cause_signal("codex", &cause(LiveCauseKind::AwaitingInput, None)),
            None
        );
        // The same halt read from the halt and from its live cause is one signal.
        let from_halt = halt_signal(
            "codex",
            "usage_limit_exceeded",
            "Try again in 1 minute.",
            halted(),
        );
        assert_eq!(from_halt, Some(limited));

        let recovered = recovered_signal("claude", halted()).unwrap();
        assert_eq!(
            (recovered.kind, recovered.at_ms),
            (SignalKind::Recovered, AT_MS)
        );
        recovered.validate(AT_MS).unwrap();

        let event = |cost_usd, session: Option<&str>| UsageEvent {
            ts: "2026-11-07T01:00:00Z".into(),
            tool: "claude".into(),
            model: Some("synthetic-model".into()),
            tokens_in: 1200,
            tokens_out: 300,
            cost_usd,
            ticket: Some("01M6N3H10PF7JHA3VHWS72N5Z1".into()),
            session: session.map(Into::into),
        };
        let priced = usage_signal(&event(Some(0.1234), Some("session-1"))).unwrap();
        assert_eq!(
            serde_json::to_value(&priced).unwrap(),
            serde_json::json!({"tool":"claude","kind":"usage","at_ms":AT_MS,"cost_cents":13,
                "session":"session-1"}),
            "identifiers and metrics only: no model, ticket or token text"
        );
        priced.validate(AT_MS).unwrap();
        assert_eq!(
            usage_signal(&event(Some(0.0), None)).unwrap().cost_cents,
            Some(0)
        );
        // An absent or unusable cost stays unknown instead of becoming zero.
        for cost in [None, Some(f64::NAN), Some(-1.0), Some(f64::INFINITY)] {
            assert_eq!(usage_signal(&event(cost, None)).unwrap().cost_cents, None);
        }
        // A session id of another shape is dropped, not rewritten.
        assert_eq!(
            usage_signal(&event(None, Some("a b"))).unwrap().session,
            None
        );
        let mut undated = event(None, None);
        undated.ts = "yesterday".into();
        assert_eq!(usage_signal(&undated), None);
    }
}
