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

/// HS2-86MK1Y: the limit signal for a driven turn that failed with a typed limit (a spawn
/// tool's structured failure line, [`hotsheet_aitools::structured_failure`]). It goes
/// through [`halt_signal`], so it is a limit exactly when `is_usage_limit` says so; any
/// other ending records nothing.
pub fn driven_failure_signal(
    tool: &str,
    done: &hotsheet_aitools::DoneReason,
    at: OffsetDateTime,
) -> Option<LimitSignal> {
    let hotsheet_aitools::DoneReason::Failed {
        message,
        error_type: Some(error_type),
        ..
    } = done
    else {
        return None;
    };
    halt_signal(tool, error_type, message.as_deref().unwrap_or(""), at)
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
        session: Some(usage_discriminator(event)),
        ..signal(&event.tool, SignalKind::Usage, at)?
    })
}

/// HS2-XFMKHQ: the opaque discriminator of one recorded usage event: its session id
/// (when usable) and a short digest of the event itself. A retried append of the same
/// event yields the same value, so the registry still counts it once; two different
/// responses recorded in the same millisecond with the same rounded cost stay apart,
/// which a millisecond time and a cost alone could not tell. Identifiers only: the
/// digest covers numbers and ids, and nothing in it can be read back.
fn usage_discriminator(event: &UsageEvent) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for part in [
        event.ts.as_str(),
        event.tool.as_str(),
        event.model.as_deref().unwrap_or(""),
        event.ticket.as_deref().unwrap_or(""),
        event.session.as_deref().unwrap_or(""),
    ] {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    hash.update(event.tokens_in.to_le_bytes());
    hash.update(event.tokens_out.to_le_bytes());
    hash.update(event.cost_usd.map_or(u64::MAX, f64::to_bits).to_le_bytes());
    let digest: String = hash.finalize()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    match event.session.as_deref().and_then(identifier) {
        // Kept within the 160-character identifier bound with the digest appended.
        Some(session) => format!("{}.{digest}", &session[..session.len().min(140)]),
        None => format!("u.{digest}"),
    }
}

/// HS2-PFHRS9: the tool account the terminal `terminal_id` was started for, when an
/// orchestrator started it for one. Read from this machine's hosted-session records;
/// identifiers only, never credentials or provider profiles.
pub(crate) fn session_account(state: &AppState, terminal_id: &str) -> Option<String> {
    hotsheet_ticketing::orchestration::HostedSessionRegistry::new(state.machine_home())
        .account_of(terminal_id)
        .ok()
        .flatten()
        .and_then(|account| identifier(&account))
}

/// HS2-PFHRS9: a signal observed in terminal `terminal_id`, attributed to the account its
/// session was started for. A session without one stays unattributed.
pub(crate) fn attributed(
    state: &AppState,
    terminal_id: &str,
    signal: Option<LimitSignal>,
) -> Option<LimitSignal> {
    let signal = signal?;
    Some(LimitSignal {
        account: session_account(state, terminal_id),
        ..signal
    })
}

/// HS2-XFMKHQ: the one place an appended usage event becomes a capacity signal. Call it
/// right after each successful append with exactly the events written (priced, so a
/// budget sees the recorded cost). Events from a terminal are attributed to the account
/// its session was started for (HS2-PFHRS9); `None` stays unattributed. A retried append
/// of the same event yields the identical signal, which the registry answers as a
/// duplicate, so it is counted once.
pub(crate) fn usage_appended<'a>(
    state: &AppState,
    terminal: Option<&str>,
    events: impl IntoIterator<Item = &'a UsageEvent>,
) {
    let account = terminal.and_then(|terminal| session_account(state, terminal));
    for event in events {
        record(
            state,
            usage_signal(event).map(|signal| LimitSignal {
                account: account.clone(),
                ..signal
            }),
        );
    }
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
    /// HS2-86MK1Y: a driven turn's typed limit failure becomes a limit signal through
    /// the same halt path; other endings and untyped failures record nothing.
    #[test]
    fn hs2_86mk1y_a_driven_turn_failure_maps_through_the_halt_path() {
        use hotsheet_aitools::DoneReason;
        let failed = |error_type: Option<&str>, message: Option<&str>| DoneReason::Failed {
            exit_code: 3,
            message: message.map(str::to_owned),
            error_type: error_type.map(str::to_owned),
        };
        let usage = driven_failure_signal(
            "antigravity",
            &failed(Some("usage_limit_exceeded"), Some("Quota reached.")),
            halted(),
        )
        .unwrap();
        assert_eq!(
            (
                usage.tool.as_str(),
                usage.kind,
                usage.at_ms,
                usage.resets_at_ms
            ),
            ("antigravity", SignalKind::UsageLimit, AT_MS, None)
        );
        assert_eq!(usage.account, None);
        let rate = driven_failure_signal(
            "antigravity",
            &failed(Some("rate_limit"), Some("Resets in 2h 15m.")),
            halted(),
        )
        .unwrap();
        assert_eq!(rate.kind, SignalKind::RateLimit);
        assert_eq!(rate.resets_at_ms, Some(AT_MS + 135 * 60_000));
        for done in [
            failed(None, Some("RESOURCE_EXHAUSTED 429 quota")),
            failed(Some("overloaded"), None),
            DoneReason::Completed,
            DoneReason::Interrupted,
        ] {
            assert_eq!(
                driven_failure_signal("antigravity", &done, halted()),
                None,
                "{done:?}"
            );
        }
        assert_eq!(
            driven_failure_signal("not a tool id", &failed(Some("rate_limit"), None), halted()),
            None
        );
    }

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
            // HS2-2BAVAM: Codex's request-rate limit keeps its own kind.
            (
                "codex",
                "rate_limit_exceeded",
                "Rate limit reached. Try again in 20 minutes.",
                Some((SignalKind::RateLimit, Some(20 * 60_000))),
            ),
            (
                "codex",
                "rate_limit_exceeded",
                "Rate limit reached.",
                Some((SignalKind::RateLimit, None)),
            ),
            // Halts that are not limits produce no signal for any tool.
            (
                "codex",
                "session_budget_exceeded",
                "Try again in 5 minutes.",
                None,
            ),
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
        let mut value = serde_json::to_value(&priced).unwrap();
        let session = value["session"].as_str().unwrap().to_owned();
        assert!(
            session.starts_with("session-1.") && session.len() == "session-1.".len() + 16,
            "{session}"
        );
        value["session"] = "session-1".into();
        assert_eq!(
            value,
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
        // A session id of another shape is dropped, not rewritten: only the digest stays.
        let unusable = usage_signal(&event(None, Some("a b")))
            .unwrap()
            .session
            .unwrap();
        assert!(
            unusable.starts_with("u.") && !unusable.contains("a b"),
            "{unusable}"
        );
        // HS2-XFMKHQ: the same event again is the same signal (counted once), while two
        // responses in the same millisecond with the same rounded cost stay apart.
        let first = event(Some(0.01575), Some("cap-session"));
        let mut second = first.clone();
        second.tokens_out += 10;
        second.cost_usd = Some(0.0165);
        let (a, b) = (
            usage_signal(&first).unwrap(),
            usage_signal(&second).unwrap(),
        );
        assert_eq!((a.at_ms, a.cost_cents), (b.at_ms, b.cost_cents));
        assert_ne!(a, b);
        assert_eq!(usage_signal(&first.clone()).unwrap(), a);
        a.validate(AT_MS).unwrap();
        let mut undated = event(None, None);
        undated.ts = "yesterday".into();
        assert_eq!(usage_signal(&undated), None);
    }
}
