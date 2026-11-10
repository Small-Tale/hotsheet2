//! Why a held claim is not moving (HS2-EGKKA0, `docs/32-notifier-plugins.md` §32.8.4): pure
//! classification of the two stall causes the AI tools report exactly.
//!
//! - **Usage-limit pause.** A terminal halt whose error type is the tool's usage or rate
//!   limit. Claude's `StopFailure` hook reports `rate_limit`; a Codex rollout's failed turn
//!   reports `codex_error_info` `usage_limit_exceeded`. Neither carries a structured reset
//!   time, so [`reported_reset`] reads one from the message when its instant or time zone is
//!   explicit, or when the host's local zone is discoverable (HS2-Z28AD3).
//! - **Awaiting input** is read from hook state (a bridged permission request or a pending
//!   `AskUserQuestion`), not from terminal output, so it needs no classification here.

use jiff::{Timestamp, civil, tz::TimeZone};
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

/// The reset time a usage-limit message states:
///
/// - Claude's legacy `… usage limit reached|<unix seconds or ms>`.
/// - An RFC 3339 instant anywhere in the message.
/// - A relative time after "reset(s) in" or "try again in" (`2h 15m`, `3 hours`, `45 minutes`).
/// - A time of day after "reset(s) (at)" or "try again at" with UTC/GMT, an IANA zone, or
///   the host's discoverable local zone. It is the next such unambiguous time.
///
/// Returns `None` (unknown) for anything else and for any instant outside
/// `halted_at - 5 minutes ..= halted_at + 8 days`.
pub fn reported_reset(message: &str, halted_at: OffsetDateTime) -> Option<OffsetDateTime> {
    let local_zone = TimeZone::try_system().ok();
    reported_reset_with_local_zone(message, halted_at, local_zone.as_ref())
}

fn reported_reset_with_local_zone(
    message: &str,
    halted_at: OffsetDateTime,
    local_zone: Option<&TimeZone>,
) -> Option<OffsetDateTime> {
    let lower = message.to_ascii_lowercase();
    let found = epoch_after_limit(&lower)
        .or_else(|| rfc3339_in(message))
        .or_else(|| relative_reset(&lower, halted_at))
        .or_else(|| clock_reset(&lower, halted_at, local_zone))?;
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

fn clock_reset(
    lower: &str,
    halted_at: OffsetDateTime,
    local_zone: Option<&TimeZone>,
) -> Option<OffsetDateTime> {
    after_keywords(
        lower,
        &["resets at ", "reset at ", "resets ", "try again at "],
    )
    .into_iter()
    .find_map(|rest| {
        let (time, zone) = clock_prefix(rest, local_zone)?;
        let halted = Timestamp::from_second(halted_at.unix_timestamp()).ok()?;
        let date = zone.to_datetime(halted).date();
        let candidate = |date: civil::Date| {
            let local = civil::DateTime::new(
                date.year(),
                date.month(),
                date.day(),
                time.hour() as i8,
                time.minute() as i8,
                0,
                0,
            )
            .ok()?;
            let instant = zone.to_ambiguous_timestamp(local).unambiguous().ok()?;
            OffsetDateTime::from_unix_timestamp(instant.as_second()).ok()
        };
        // A gap or repeated hour is unknown, even if tomorrow has the same clock time.
        let today = candidate(date)?;
        if today >= halted_at - EARLIEST_RESET {
            Some(today)
        } else {
            candidate(date.tomorrow().ok()?)
        }
    })
}

/// `3pm UTC`, `15:10 (America/Los_Angeles)`, or `3:10 pm.` at the start of `text`.
fn clock_prefix(text: &str, local_zone: Option<&TimeZone>) -> Option<(Time, TimeZone)> {
    let hour_len = text.bytes().take_while(u8::is_ascii_digit).count();
    if hour_len == 0 || hour_len > 2 {
        return None;
    }
    let mut hour: u8 = text[..hour_len].parse().ok()?;
    let mut rest = &text[hour_len..];
    let mut minute: u8 = 0;
    let mut has_minutes = false;
    if let Some(after) = rest.strip_prefix(':') {
        let minute_len = after.bytes().take_while(u8::is_ascii_digit).count();
        if minute_len != 2 {
            return None;
        }
        minute = after[..2].parse().ok()?;
        has_minutes = true;
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
    let rest = rest.trim_start();
    let (zone, trailing) = if let Some(parenthesized) = rest.strip_prefix('(') {
        let (name, trailing) = parenthesized.split_once(')')?;
        (Some(name), trailing)
    } else if rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
        let len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+'))
            .count();
        (Some(&rest[..len]), &rest[len..])
    } else {
        (None, rest)
    };
    if !trailing.trim_start().is_empty() && !trailing.trim_start().starts_with(['.', '!', '?']) {
        return None;
    }
    let zone = match zone {
        Some("gmt" | "utc") => TimeZone::UTC,
        Some(name) if !name.is_empty() => {
            TimeZone::get(name).ok().filter(|zone| !zone.is_unknown())?
        }
        Some(_) => return None,
        None if meridiem.is_some() || has_minutes => {
            local_zone.filter(|zone| !zone.is_unknown())?.clone()
        }
        None => return None,
    };
    Some((Time::from_hms(hour, minute, 0).ok()?, zone))
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
    fn hs2_egkka0_claude_fixtures_preserve_zone_free_resets() {
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
        // A named IANA zone resolves the next local clock time.
        assert_eq!(
            reset("You've hit your limit · resets 3pm (America/Los_Angeles)"),
            Some(22 * 60)
        );
        assert_eq!(reset("resets 3pm utc+2"), None);
        assert_eq!(reset("Claude usage limit reached."), None);
    }

    #[test]
    fn hs2_egkka0_codex_fixtures_preserve_zone_free_resets() {
        // A local clock is tested with an injected zone below.
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

    fn reset_in_zone(message: &str, at: OffsetDateTime, zone: Option<&str>) -> Option<i64> {
        let zone = zone.map(|name| TimeZone::get(name).unwrap());
        reported_reset_with_local_zone(message, at, zone.as_ref())
            .map(|found| (found - at).whole_minutes())
    }

    #[test]
    fn hs2_z28ad3_codex_local_and_claude_named_zone() {
        let codex = "You've hit your usage limit. Upgrade to Pro or try again at 3:10 PM.";
        assert_eq!(
            reset_in_zone(codex, halted(), Some("Asia/Manila")),
            Some(6 * 60 + 10)
        );
        assert_eq!(reset_in_zone(codex, halted(), None), None);
        assert_eq!(
            reset_in_zone("resets 3pm", halted(), Some("Asia/Manila")),
            Some(6 * 60)
        );
        assert_eq!(reset_in_zone("resets 3pm", halted(), None), None);
        assert_eq!(
            reset_in_zone("resets 3pm", halted(), Some("Etc/Unknown")),
            None
        );
        assert_eq!(
            reset_in_zone("resets 3.", halted(), Some("Asia/Manila")),
            None
        );
        assert_eq!(reset("resets 3pm (America/Los_Angeles)"), Some(22 * 60));
        assert_eq!(reset("resets 3pm (No/Such_Zone)"), None);
        assert_eq!(reset("resets 3pm (America/Los_Angeles"), None);
        assert_eq!(reset("presets 3pm (UTC)"), None);
        assert_eq!(reset("resets 3pm (UTC) or later"), None);
    }

    #[test]
    fn hs2_z28ad3_dst_gap_and_fold_are_unknown() {
        let before_gap = OffsetDateTime::parse("2026-03-08T09:00:00Z", &Rfc3339).unwrap();
        assert_eq!(
            reset_in_zone("resets 2:30am (America/Los_Angeles)", before_gap, None),
            None
        );
        assert_eq!(
            reset_in_zone("resets 3:30am (America/Los_Angeles)", before_gap, None),
            Some(90)
        );
        let before_fold = OffsetDateTime::parse("2026-11-01T07:00:00Z", &Rfc3339).unwrap();
        assert_eq!(
            reset_in_zone("resets 1:30am (America/Los_Angeles)", before_fold, None),
            None
        );
        assert_eq!(
            reset_in_zone("resets 2:30am (America/Los_Angeles)", before_fold, None),
            Some(210)
        );
    }
}
