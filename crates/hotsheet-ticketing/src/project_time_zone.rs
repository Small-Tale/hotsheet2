//! The project time zone and due-date deadlines (docs/38-milestones.md §38.3, HS2-SZF152).
//!
//! `time_zone` is a shared project setting: an IANA zone name from the zone database, or
//! `UTC`, committed in `.hotsheet2/settings.json` so every clone computes the same deadline.
//! Unset means `UTC`, never the device's zone. Only the shared scope is read: a value written
//! to another scope is refused, and one found there by hand is ignored.
//!
//! A date-only due date `D` ends at the end of day `D` in that zone: its deadline instant is
//! the start of `D + 1` there, minus one millisecond, so a daylight-saving day of 23 or 25
//! hours still ends at local midnight. The milestone roll-up, search and scheduler share the
//! helpers below (HS2-A820C1, HS2-QY5YRN).

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use serde_json::Value;

use crate::settings::{Scope, Settings, SettingsError};

/// The shared project settings key (docs/38 §38.3).
pub const TIME_ZONE_SETTING: &str = "time_zone";
/// The zone in effect when a project sets none.
pub const DEFAULT_TIME_ZONE: &str = "UTC";
/// Refusal code: `time_zone` written to the local or global scope.
pub const TIME_ZONE_SHARED_ONLY: &str = "time_zone_shared_only";
/// Refusal code: a value the zone database does not name.
pub const INVALID_TIME_ZONE: &str = "invalid_time_zone";

const DAY_MS: i64 = 86_400_000;

/// The project's zone in effect, with the hand-edited value it replaced, if any.
#[derive(Debug, Clone)]
pub struct ProjectTimeZone {
    /// The canonical zone name in effect (`UTC` when unset or invalid).
    pub name: String,
    /// The zone itself.
    pub zone: TimeZone,
    /// A stored value that names no zone; the project reads as `UTC` while it stays.
    pub invalid_value: Option<Value>,
}

impl ProjectTimeZone {
    /// The default: `UTC`.
    pub fn utc() -> Self {
        Self {
            name: DEFAULT_TIME_ZONE.to_owned(),
            zone: TimeZone::UTC,
            invalid_value: None,
        }
    }

    /// A plain-language diagnostic naming a hand-edited value that names no zone.
    pub fn diagnostic(&self) -> Option<String> {
        self.invalid_value.as_ref().map(|value| {
            format!(
                "The project time zone {value} in .hotsheet2/settings.json is not a known time zone, so due dates use UTC. Choose a zone such as Europe/Berlin, or UTC."
            )
        })
    }
}

/// Resolve a zone name through the zone database, returning its canonical name.
/// `UTC` is accepted in any letter case; other names must be ones the database lists.
pub fn resolve_time_zone(name: &str) -> Option<(String, TimeZone)> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed != name {
        return None;
    }
    if trimmed.eq_ignore_ascii_case(DEFAULT_TIME_ZONE) {
        return Some((DEFAULT_TIME_ZONE.to_owned(), TimeZone::UTC));
    }
    // A POSIX TZ string or a fixed offset is not a zone name.
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
    {
        return None;
    }
    let zone = TimeZone::get(trimmed).ok()?;
    let canonical = zone.iana_name().unwrap_or(trimmed).to_owned();
    Some((canonical, zone))
}

fn refused(code: &'static str, message: String) -> SettingsError {
    SettingsError::Refused { code, message }
}

/// Validate a `time_zone` write: shared scope only, and a name the zone database knows.
/// Returns the canonical value to store.
pub fn validate_time_zone_write(value: &Value, scope: Scope) -> Result<Value, SettingsError> {
    if scope != Scope::Shared {
        return Err(refused(
            TIME_ZONE_SHARED_ONLY,
            "The project time zone is shared by every clone, so it can only be set in the shared project settings.".to_owned(),
        ));
    }
    let Some((name, _)) = value.as_str().and_then(resolve_time_zone) else {
        return Err(refused(
            INVALID_TIME_ZONE,
            format!(
                "{value} is not a known time zone. Use a zone name such as Europe/Berlin, or UTC."
            ),
        ));
    };
    Ok(Value::String(name))
}

/// Read a stored value: a known zone, else `UTC` with the bad value kept for diagnostics.
pub fn time_zone_from_value(value: Option<&Value>) -> ProjectTimeZone {
    let Some(value) = value else {
        return ProjectTimeZone::utc();
    };
    match value.as_str().and_then(resolve_time_zone) {
        Some((name, zone)) => ProjectTimeZone {
            name,
            zone,
            invalid_value: None,
        },
        None => ProjectTimeZone {
            invalid_value: Some(value.clone()),
            ..ProjectTimeZone::utc()
        },
    }
}

/// The project's zone in effect. Only the shared scope is read.
pub fn project_time_zone(settings: &Settings) -> Result<ProjectTimeZone, SettingsError> {
    Ok(time_zone_from_value(
        settings.get(TIME_ZONE_SETTING, Scope::Shared)?.as_ref(),
    ))
}

/// Every zone name the zone database lists, sorted, with `UTC` first.
pub fn available_time_zones() -> Vec<String> {
    let mut names: Vec<String> = jiff::tz::db()
        .available()
        .map(|name| name.as_str().to_owned())
        .filter(|name| !name.eq_ignore_ascii_case(DEFAULT_TIME_ZONE))
        .collect();
    names.sort();
    names.dedup();
    names.insert(0, DEFAULT_TIME_ZONE.to_owned());
    names
}

/// A due-date parse failure: the value is not a `YYYY-MM-DD` calendar date.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a due date; use YYYY-MM-DD")]
pub struct DueDateError(pub String);

/// Parse a date-only due date, strictly `YYYY-MM-DD`.
pub fn parse_due_on(text: &str) -> Result<Date, DueDateError> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    if !shaped {
        return Err(DueDateError(text.to_owned()));
    }
    text.parse::<Date>()
        .map_err(|_| DueDateError(text.to_owned()))
}

/// The deadline instant of due date `due_on` in `zone`, in Unix milliseconds: the start of
/// the next day there, minus one millisecond. Computed through the zone database, so a day
/// with a daylight-saving change still ends at local midnight; a zone whose midnight is
/// skipped starts the next day at its first instant. Saturates at the end of the supported
/// range.
pub fn deadline_instant_ms(due_on: Date, zone: &TimeZone) -> i64 {
    let max = Timestamp::MAX.as_millisecond();
    let Ok(next) = due_on.tomorrow() else {
        return max;
    };
    match next
        .to_zoned(zone.clone())
        .and_then(|day| day.start_of_day())
    {
        Ok(start) => start.timestamp().as_millisecond() - 1,
        Err(_) => max,
    }
}

/// Overdue: `now` is after the deadline instant.
pub fn is_overdue(deadline_ms: i64, now_ms: i64) -> bool {
    now_ms > deadline_ms
}

/// Due within `days`: not overdue, and the deadline instant is at most `days` × 24 hours
/// from `now` (deadline instants are compared, not calendar dates).
pub fn is_due_within(deadline_ms: i64, now_ms: i64, days: u32) -> bool {
    !is_overdue(deadline_ms, now_ms)
        && deadline_ms.saturating_sub(now_ms) <= i64::from(days).saturating_mul(DAY_MS)
}

/// Calendar days from today in `zone` (at `now_ms`) to `due_on`: 0 is "today", 1
/// "tomorrow", −3 "3 days overdue". Every viewer of a project reads the same word
/// whatever their device zone, because the project zone decides "today".
pub fn due_day_offset(due_on: Date, zone: &TimeZone, now_ms: i64) -> i64 {
    let now = Timestamp::from_millisecond(now_ms).unwrap_or(if now_ms < 0 {
        Timestamp::MIN
    } else {
        Timestamp::MAX
    });
    let today = now.to_zoned(zone.clone()).date();
    today
        .until(due_on)
        .map(|span| i64::from(span.get_days()))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "project_time_zone_tests.rs"]
mod tests;
