//! Why a held claim is not moving (HS2-EGKKA0, `docs/32-notifier-plugins.md` §32.8.4): pure
//! classification of the two stall causes the AI tools report exactly.
//!
//! - **Usage-limit pause.** A terminal halt whose error type is the tool's usage or rate
//!   limit. Claude's `StopFailure` hook reports `rate_limit`; a Codex rollout's failed turn
//!   reports `codex_error_info` `usage_limit_exceeded`. Neither carries a structured reset
//!   time, so [`reported_reset`] reads one from the message only when the message states an
//!   absolute or relative instant that needs no time zone. A local or named-zone time such as
//!   "resets 3pm (America/Los_Angeles)" stays unknown (`None`): Hot Sheet does not guess.
//! - **Awaiting input** is read from hook state (a bridged permission request or a pending
//!   `AskUserQuestion`), not from terminal output, so it needs no classification here.

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime, Time};

/// A reported reset is believed only inside this window after the halt.
const LATEST_RESET: Duration = Duration::days(8);

/// A reset reported slightly before the halt (clock skew, a rounded time) is still accepted.
const EARLIEST_RESET: Duration = Duration::minutes(5);

/// Whether a halt's tool error type is a usage or rate limit: Claude `rate_limit`, Codex
/// `usage_limit_exceeded`. Other halts (`overloaded`, `billing_error`, …) are not pauses.
pub fn is_usage_limit(error_type: &str) -> bool {
    matches!(
        error_type.trim().to_ascii_lowercase().as_str(),
        "rate_limit" | "usage_limit_exceeded"
    )
}

/// The reset time a usage-limit message states, read from forms that need no time zone:
///
/// - Claude's legacy `… usage limit reached|<unix seconds or ms>`.
/// - An RFC 3339 instant anywhere in the message.
/// - A relative time after "reset(s) in" or "try again in" (`2h 15m`, `3 hours`, `45 minutes`).
/// - A time of day after "reset(s) (at)" or "try again at" that names UTC or GMT; it is the
///   next such time at or after `halted_at`.
///
/// Returns `None` (unknown) for anything else and for any instant outside
/// `halted_at - 5 minutes ..= halted_at + 8 days`.
pub fn reported_reset(message: &str, halted_at: OffsetDateTime) -> Option<OffsetDateTime> {
    let lower = message.to_ascii_lowercase();
    let found = epoch_after_limit(&lower)
        .or_else(|| rfc3339_in(message))
        .or_else(|| relative_reset(&lower, halted_at))
        .or_else(|| utc_time_of_day(&lower, halted_at))?;
    (found >= halted_at - EARLIEST_RESET && found <= halted_at + LATEST_RESET).then_some(found)
}

fn epoch_after_limit(lower: &str) -> Option<OffsetDateTime> {
    let rest = &lower[lower.find("limit reached|")? + "limit reached|".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let value: i64 = digits.parse().ok()?;
    match digits.len() {
        10 => OffsetDateTime::from_unix_timestamp(value).ok(),
        13 => OffsetDateTime::from_unix_timestamp_nanos(i128::from(value) * 1_000_000).ok(),
        _ => None,
    }
}

fn rfc3339_in(message: &str) -> Option<OffsetDateTime> {
    message
        .split(|c: char| c.is_whitespace() || "()[]{}<>,;\"'`".contains(c))
        .map(|token| token.trim_end_matches(['.', '!', '?']))
        .filter(|token| token.len() >= 20 && token.as_bytes()[0].is_ascii_digit())
        .find_map(|token| OffsetDateTime::parse(token, &Rfc3339).ok())
}

/// Every position just after one of `keywords`, in message order.
fn after_keywords<'a>(lower: &'a str, keywords: &[&str]) -> Vec<&'a str> {
    let mut found: Vec<(usize, &str)> = Vec::new();
    for keyword in keywords {
        let mut from = 0;
        while let Some(at) = lower[from..].find(keyword) {
            let start = from + at;
            let boundary = start == 0 || !lower.as_bytes()[start - 1].is_ascii_alphanumeric();
            if boundary {
                found.push((start, &lower[start + keyword.len()..]));
            }
            from = start + keyword.len();
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, rest)| rest).collect()
}

fn relative_reset(lower: &str, halted_at: OffsetDateTime) -> Option<OffsetDateTime> {
    after_keywords(lower, &["resets in ", "reset in ", "try again in "])
        .into_iter()
        .find_map(duration_prefix)
        .map(|duration| halted_at + duration)
}

/// A duration such as `2h 15m`, `1 day, 3 hours and 5 minutes`, or `45 min` at the start of
/// `text`; `None` when it does not start with one or it is zero.
fn duration_prefix(text: &str) -> Option<Duration> {
    let mut rest = text;
    let mut total = Duration::ZERO;
    let mut parts = 0;
    loop {
        rest = rest.trim_start_matches([' ', ',']);
        if parts > 0 {
            if let Some(after) = rest.strip_prefix("and ") {
                rest = after.trim_start();
            }
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 4 {
            break;
        }
        let value: i64 = rest[..digits].parse().ok()?;
        let after = rest[digits..].trim_start();
        let unit_len = after.bytes().take_while(u8::is_ascii_alphabetic).count();
        let unit = match &after[..unit_len] {
            "d" | "day" | "days" => Duration::days(value),
            "h" | "hr" | "hrs" | "hour" | "hours" => Duration::hours(value),
            "m" | "min" | "mins" | "minute" | "minutes" => Duration::minutes(value),
            "s" | "sec" | "secs" | "second" | "seconds" => Duration::seconds(value),
            _ => break,
        };
        total += unit;
        parts += 1;
        rest = &after[unit_len..];
    }
    (parts > 0 && total > Duration::ZERO).then_some(total)
}

fn utc_time_of_day(lower: &str, halted_at: OffsetDateTime) -> Option<OffsetDateTime> {
    after_keywords(
        lower,
        &["resets at ", "reset at ", "resets ", "try again at "],
    )
    .into_iter()
    .find_map(|rest| {
        let time = utc_clock_prefix(rest)?;
        let utc = halted_at.to_offset(time::UtcOffset::UTC);
        let today = utc.replace_time(time);
        Some(if today >= utc {
            today
        } else {
            today + Duration::days(1)
        })
    })
}

/// `3pm UTC`, `15:10 utc`, `3:10 pm (gmt)` at the start of `text`.
fn utc_clock_prefix(text: &str) -> Option<Time> {
    let hour_len = text.bytes().take_while(u8::is_ascii_digit).count();
    if hour_len == 0 || hour_len > 2 {
        return None;
    }
    let mut hour: u8 = text[..hour_len].parse().ok()?;
    let mut rest = &text[hour_len..];
    let mut minute: u8 = 0;
    if let Some(after) = rest.strip_prefix(':') {
        let minute_len = after.bytes().take_while(u8::is_ascii_digit).count();
        if minute_len != 2 {
            return None;
        }
        minute = after[..2].parse().ok()?;
        rest = &after[2..];
    }
    rest = rest.trim_start();
    let meridiem = if let Some(after) = rest.strip_prefix("am") {
        rest = after;
        Some(false)
    } else if let Some(after) = rest.strip_prefix("pm") {
        rest = after;
        Some(true)
    } else {
        None
    };
    if let Some(pm) = meridiem {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour = match (hour, pm) {
            (12, false) => 0,
            (12, true) => 12,
            (h, true) => h + 12,
            (h, false) => h,
        };
    }
    let zone = rest.trim_start().trim_start_matches('(');
    let named_utc = ["utc", "gmt"].iter().any(|name| {
        zone.strip_prefix(name).is_some_and(|after| {
            !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '+' || c == '-')
        })
    });
    if !named_utc {
        return None;
    }
    Time::from_hms(hour, minute, 0).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-07T01:00:00Z.
    fn halted() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_794_013_200).unwrap()
    }

    fn reset(message: &str) -> Option<i64> {
        reported_reset(message, halted()).map(|at| (at - halted()).whole_minutes())
    }

    #[test]
    fn hs2_egkka0_usage_limit_error_types_are_exact_per_tool() {
        // Claude StopFailure `error` and Codex `codex_error_info` variant names.
        for kind in ["rate_limit", "usage_limit_exceeded", " RATE_LIMIT "] {
            assert!(is_usage_limit(kind), "{kind}");
        }
        // Other halts are not pauses, even when their text mentions a limit.
        for kind in [
            "overloaded",
            "server_overloaded",
            "billing_error",
            "authentication_failed",
            "context_window_exceeded",
            "max_output_tokens",
            "unknown",
            "",
            "rate_limit_exceeded_but_not",
        ] {
            assert!(!is_usage_limit(kind), "{kind}");
        }
    }

    #[test]
    fn hs2_egkka0_claude_fixtures_read_only_zone_free_resets() {
        // Legacy Claude transcript form: unix seconds, or milliseconds, after the pipe.
        assert_eq!(
            reset("Claude AI usage limit reached|1794023400"),
            Some(170),
            "seconds"
        );
        assert_eq!(
            reset("Claude AI usage limit reached|1794023400000"),
            Some(170),
            "milliseconds"
        );
        assert_eq!(reset("Claude AI usage limit reached|17940"), None);
        // A relative reset.
        assert_eq!(reset("5-hour limit reached ∙ resets in 2h 15m"), Some(135));
        assert_eq!(
            reset("You've hit your limit. It resets in 1 day, 3 hours and 5 minutes."),
            Some(24 * 60 + 185)
        );
        // An explicit UTC time of day is the next one after the halt.
        assert_eq!(
            reset("You've hit your limit · resets 3pm (UTC)"),
            Some(14 * 60)
        );
        assert_eq!(
            reset("Limit reached. Resets at 00:30 UTC."),
            Some(23 * 60 + 30)
        );
        assert_eq!(reset("limit reached, resets 1:00 am gmt"), Some(0));
        // Local and named-zone times need a zone database: unknown, never guessed.
        assert_eq!(
            reset("You've hit your limit · resets 3pm (America/Los_Angeles)"),
            None
        );
        assert_eq!(reset("You've hit your limit · resets 3pm"), None);
        assert_eq!(reset("resets 3pm utc+2"), None);
        assert_eq!(reset("Claude usage limit reached."), None);
    }

    #[test]
    fn hs2_egkka0_codex_fixtures_read_only_zone_free_resets() {
        // Codex prints a local time; that is unknown here.
        assert_eq!(
            reset("You've hit your usage limit. Upgrade to Pro or try again at 3:10 PM."),
            None
        );
        assert_eq!(
            reset("You've hit your usage limit. Try again in 45 minutes."),
            Some(45)
        );
        assert_eq!(
            reset("You've hit your usage limit. Try again at 2026-11-07T03:10:00Z."),
            Some(130)
        );
        assert_eq!(
            reset("usage limit (resets_at 2026-11-07T04:10:00+01:00)"),
            Some(130),
            "an offset instant"
        );
    }

    #[test]
    fn hs2_egkka0_false_positive_guards() {
        // Implausible instants: far past, far future, zero.
        assert_eq!(reset("usage limit reached|1000000000"), None, "2001");
        assert_eq!(
            reset("try again at 2030-01-01T00:00:00Z"),
            None,
            "years out"
        );
        assert_eq!(reset("resets in 0 minutes"), None);
        assert_eq!(reset("resets in 30 days"), None, "beyond the window");
        // A keyword inside another word, or followed by no duration.
        assert_eq!(reset("presets in 5 minutes"), None);
        assert_eq!(reset("resets in a while"), None);
        assert_eq!(reset("resets in 5 bananas"), None);
        // Version numbers and bare times are not instants.
        assert_eq!(reset("codex 0.166.0 at 15:10"), None);
        assert_eq!(reset("resets 25pm utc"), None);
        assert_eq!(reset("resets 3:1 utc"), None);
        assert_eq!(reset("resets 99:00 utc"), None);
        assert_eq!(reset(""), None);
        // A reset a few minutes before the halt is clock skew; an hour before is not.
        assert_eq!(reset("try again at 2026-11-07T00:57:00Z"), Some(-3));
        assert_eq!(reset("try again at 2026-11-07T00:00:00Z"), None);
    }
}
