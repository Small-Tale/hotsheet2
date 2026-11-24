//! Local AI-command schedules and durable occurrence state (HS2-G5J2E7).
//!
//! A schedule belongs to one command in one checkout. The local settings file is the
//! authority on this host; ticket tags provide the cross-restart occurrence identity.

use serde::{Deserialize, Serialize};
use time::{Date, Month};

pub const SETTING: &str = "command_schedules";
pub const HISTORY_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleAction {
    CreateTicket,
    NewAiSession,
    ReuseOrNewAiSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecurrenceKind {
    Once,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    SelectedDates,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DayRule {
    DayOfMonth { day: u8 },
    LastDay,
    WeekdayOrdinal { ordinal: i8, weekday: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recurrence {
    pub kind: RecurrenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weekdays: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<DayRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dates: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_dates: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_dates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSchedule {
    pub command_id: String,
    pub time_zone: String,
    pub local_time: String,
    pub action: ScheduleAction,
    #[serde(default)]
    pub paused: bool,
    pub recurrence: Recurrence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceOutcome {
    Created,
    SkippedOpenTicket,
    DeliveryPending,
    Delivered,
    DeliveryFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OccurrenceResult {
    pub occurrence_date: String,
    pub outcome: OccurrenceOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRecord {
    pub schedule: CommandSchedule,
    /// The latest evaluated local date, not a UTC clock instant. A repeated DST hour
    /// therefore cannot evaluate the same occurrence twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_processed_date: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<OccurrenceResult>,
}

pub fn parse_date(raw: &str) -> Result<Date, String> {
    let mut parts = raw.split('-');
    let year = parts.next().and_then(|part| part.parse::<i32>().ok());
    let month = parts.next().and_then(|part| part.parse::<u8>().ok());
    let day = parts.next().and_then(|part| part.parse::<u8>().ok());
    if parts.next().is_some() || raw.len() != 10 {
        return Err(format!("invalid date '{raw}'; expected YYYY-MM-DD"));
    }
    Date::from_calendar_date(
        year.ok_or_else(|| format!("invalid date '{raw}'"))?,
        Month::try_from(month.ok_or_else(|| format!("invalid date '{raw}'"))?)
            .map_err(|_| format!("invalid date '{raw}'"))?,
        day.ok_or_else(|| format!("invalid date '{raw}'"))?,
    )
    .map_err(|_| format!("invalid date '{raw}'"))
}

pub fn parse_local_time(raw: &str) -> Result<(u8, u8), String> {
    let mut parts = raw.split(':');
    let hour = parts.next().and_then(|part| part.parse::<u8>().ok());
    let minute = parts.next().and_then(|part| part.parse::<u8>().ok());
    if parts.next().is_some()
        || raw.len() != 5
        || hour.is_none_or(|hour| hour > 23)
        || minute.is_none_or(|minute| minute > 59)
    {
        return Err(format!("invalid local time '{raw}'; expected HH:mm"));
    }
    Ok((hour.unwrap(), minute.unwrap()))
}

impl CommandSchedule {
    pub fn validate(&self) -> Result<(), String> {
        if self.command_id.trim().is_empty() || self.command_id != self.command_id.trim() {
            return Err("schedule command_id must be non-empty and trimmed".into());
        }
        if self.time_zone.trim().is_empty() {
            return Err("schedule time_zone is required".into());
        }
        parse_local_time(&self.local_time)?;
        self.recurrence.validate()
    }
}

impl Recurrence {
    pub fn validate(&self) -> Result<(), String> {
        let date = self.date.as_deref().map(parse_date).transpose()?;
        let start = self.start_date.as_deref().map(parse_date).transpose()?;
        let end = self.end_date.as_deref().map(parse_date).transpose()?;
        if end.zip(start).is_some_and(|(end, start)| end < start) {
            return Err("end_date precedes start_date".into());
        }
        if self.count == Some(0) || self.count.is_some_and(|count| count > 1000) {
            return Err("count must be between 1 and 1000".into());
        }
        if self.every == Some(0) || self.every.is_some_and(|every| every > 365) {
            return Err("every must be between 1 and 365".into());
        }
        for value in self
            .dates
            .iter()
            .chain(&self.include_dates)
            .chain(&self.exclude_dates)
        {
            parse_date(value)?;
        }
        if self.weekdays.iter().any(|day| !(1..=7).contains(day)) {
            return Err("weekdays must be Monday=1 through Sunday=7".into());
        }
        if let Some(rule) = &self.on {
            match rule {
                DayRule::DayOfMonth { day } if !(1..=31).contains(day) => {
                    return Err("day_of_month must be 1 through 31".into());
                }
                DayRule::WeekdayOrdinal { ordinal, weekday }
                    if !((-1..=5).contains(ordinal)
                        && *ordinal != 0
                        && (1..=7).contains(weekday)) =>
                {
                    return Err(
                        "weekday_ordinal needs an ordinal of 1..5 or -1 and weekday 1..7".into(),
                    );
                }
                _ => {}
            }
        }
        match self.kind {
            RecurrenceKind::Once if date.is_none() => Err("once requires date".into()),
            RecurrenceKind::Daily
            | RecurrenceKind::Weekly
            | RecurrenceKind::Monthly
            | RecurrenceKind::Yearly
                if start.is_none() || self.every.is_none() =>
            {
                Err("repeating schedule requires start_date and every".into())
            }
            RecurrenceKind::Weekly if self.weekdays.is_empty() => {
                Err("weekly requires weekdays".into())
            }
            RecurrenceKind::Monthly if self.on.is_none() => Err("monthly requires on".into()),
            RecurrenceKind::Yearly
                if self.on.is_none()
                    || self.month.is_none_or(|month| !(1..=12).contains(&month)) =>
            {
                Err("yearly requires month 1..12 and on".into())
            }
            RecurrenceKind::SelectedDates if self.dates.is_empty() => {
                Err("selected_dates requires dates".into())
            }
            _ => Ok(()),
        }
    }
}

pub fn from_settings(
    settings: &crate::Settings,
) -> Result<Vec<ScheduleRecord>, crate::SettingsError> {
    settings
        .get(SETTING, crate::Scope::Local)?
        .map(serde_json::from_value)
        .transpose()
        .map(|records| records.unwrap_or_default())
        .map_err(|source| crate::SettingsError::Invalid {
            key: SETTING.into(),
            source,
        })
}

pub fn command_tag(command_id: &str) -> String {
    format!("hs2-command:{command_id}")
}

pub fn occurrence_tag(command_id: &str, date: &str) -> String {
    format!("hs2-occurrence:{command_id}:{date}")
}

/// Enumerate local calendar dates in a bounded window. Wall-clock conversion belongs to
/// the host, so a DST fold cannot produce a second calendar occurrence (HS2-G5J2E7).
pub fn dates_through(
    recurrence: &Recurrence,
    after: Date,
    through: Date,
    limit: usize,
) -> Result<Vec<Date>, String> {
    recurrence.validate()?;
    if limit == 0 || through <= after {
        return Ok(Vec::new());
    }
    let start = recurrence
        .start_date
        .as_deref()
        .map(parse_date)
        .transpose()?;
    let end = recurrence.end_date.as_deref().map(parse_date).transpose()?;
    let include: std::collections::HashSet<Date> = recurrence
        .include_dates
        .iter()
        .map(|date| parse_date(date))
        .collect::<Result<_, _>>()?;
    let exclude: std::collections::HashSet<Date> = recurrence
        .exclude_dates
        .iter()
        .map(|date| parse_date(date))
        .collect::<Result<_, _>>()?;
    let selected: std::collections::HashSet<Date> = recurrence
        .dates
        .iter()
        .map(|date| parse_date(date))
        .collect::<Result<_, _>>()?;
    let once = recurrence.date.as_deref().map(parse_date).transpose()?;
    let mut result = Vec::new();
    let mut ordinal = 0usize;
    let base_first = match recurrence.kind {
        RecurrenceKind::Once => once.expect("validated once date"),
        RecurrenceKind::SelectedDates => *selected.iter().min().expect("validated dates"),
        _ => start.expect("validated start date"),
    };
    let first = include
        .iter()
        .min()
        .copied()
        .map_or(base_first, |date| date.min(base_first));
    let mut date = first;
    // A twenty-year scan bounds malicious inputs while covering ordinary calendar
    // recurrence and host-offline recovery. The caller can continue from the last date.
    let scan_end = through.min(
        first
            .checked_add(time::Duration::days(366 * 20))
            .unwrap_or(Date::MAX),
    );
    while date <= scan_end {
        let base = match recurrence.kind {
            RecurrenceKind::Once => Some(date) == once,
            RecurrenceKind::SelectedDates => selected.contains(&date),
            RecurrenceKind::Daily => {
                date >= start.unwrap()
                    && (date.to_julian_day() - start.unwrap().to_julian_day())
                        % i32::from(recurrence.every.unwrap())
                        == 0
            }
            RecurrenceKind::Weekly => {
                let anchor = start.unwrap();
                let week =
                    date.to_julian_day() - i32::from(date.weekday().number_from_monday()) + 1;
                let anchor_week =
                    anchor.to_julian_day() - i32::from(anchor.weekday().number_from_monday()) + 1;
                date >= anchor
                    && (week - anchor_week) / 7 % i32::from(recurrence.every.unwrap()) == 0
                    && recurrence
                        .weekdays
                        .contains(&date.weekday().number_from_monday())
            }
            RecurrenceKind::Monthly => {
                let anchor = start.unwrap();
                let months = (date.year() - anchor.year()) * 12 + i32::from(date.month() as u8)
                    - i32::from(anchor.month() as u8);
                date >= anchor
                    && months % i32::from(recurrence.every.unwrap()) == 0
                    && matches_day_rule(date, recurrence.on.as_ref().unwrap())
            }
            RecurrenceKind::Yearly => {
                let anchor = start.unwrap();
                date >= anchor
                    && (date.year() - anchor.year()) % i32::from(recurrence.every.unwrap()) == 0
                    && date.month() as u8 == recurrence.month.unwrap()
                    && matches_day_rule(date, recurrence.on.as_ref().unwrap())
            }
        };
        if (base || include.contains(&date))
            && !exclude.contains(&date)
            && end.is_none_or(|end| date <= end)
        {
            ordinal += 1;
            if date > after {
                result.push(date);
                if result.len() == limit {
                    break;
                }
            }
            if recurrence
                .count
                .is_some_and(|count| ordinal >= usize::from(count))
            {
                break;
            }
        }
        let Some(tomorrow) = date.next_day() else {
            break;
        };
        date = tomorrow;
    }
    Ok(result)
}

fn matches_day_rule(date: Date, rule: &DayRule) -> bool {
    match rule {
        DayRule::DayOfMonth { day } => date.day() == *day,
        DayRule::LastDay => date
            .next_day()
            .is_some_and(|next| next.month() != date.month()),
        DayRule::WeekdayOrdinal { ordinal, weekday } => {
            if date.weekday().number_from_monday() != *weekday {
                return false;
            }
            if *ordinal == -1 {
                date.checked_add(time::Duration::days(7))
                    .is_some_and(|next| next.month() != date.month())
            } else {
                (date.day() - 1) / 7 + 1 == *ordinal as u8
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recurrence(kind: RecurrenceKind, start: &str) -> Recurrence {
        Recurrence {
            kind,
            date: None,
            start_date: Some(start.into()),
            every: Some(1),
            weekdays: Vec::new(),
            on: None,
            month: None,
            dates: Vec::new(),
            end_date: None,
            count: None,
            include_dates: Vec::new(),
            exclude_dates: Vec::new(),
        }
    }

    #[test]
    fn hs2_g5j2e7_monthly_ordinal_and_yearly_full_week_dates() {
        let mut monthly = recurrence(RecurrenceKind::Monthly, "2026-11-24");
        monthly.on = Some(DayRule::WeekdayOrdinal {
            ordinal: 1,
            weekday: 2,
        });
        let dates = dates_through(
            &monthly,
            parse_date("2026-11-24").unwrap(),
            parse_date("2027-02-28").unwrap(),
            3,
        )
        .unwrap();
        assert_eq!(
            dates.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["2026-12-01", "2027-01-05", "2027-02-02"]
        );
        let mut yearly = recurrence(RecurrenceKind::Yearly, "2026-11-24");
        yearly.month = Some(2);
        yearly.on = Some(DayRule::WeekdayOrdinal {
            ordinal: 3,
            weekday: 1,
        });
        let dates = dates_through(
            &yearly,
            parse_date("2026-11-24").unwrap(),
            parse_date("2029-12-31").unwrap(),
            3,
        )
        .unwrap();
        // The third full Monday-to-Sunday week starts at the third Monday.
        assert_eq!(
            dates.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["2027-02-15", "2028-02-21", "2029-02-19"]
        );
    }

    #[test]
    fn hs2_g5j2e7_missing_days_bounds_and_exceptions() {
        let mut monthly = recurrence(RecurrenceKind::Monthly, "2026-01-31");
        monthly.on = Some(DayRule::DayOfMonth { day: 31 });
        monthly.end_date = Some("2026-05-31".into());
        monthly.exclude_dates.push("2026-03-31".into());
        monthly.include_dates.push("2026-04-30".into());
        let dates = dates_through(
            &monthly,
            parse_date("2026-01-30").unwrap(),
            parse_date("2026-12-31").unwrap(),
            10,
        )
        .unwrap();
        assert_eq!(
            dates.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["2026-01-31", "2026-04-30", "2026-05-31"]
        );
    }
}
