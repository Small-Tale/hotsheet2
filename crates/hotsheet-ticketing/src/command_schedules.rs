//! Local AI-command schedules and durable occurrence state (HS2-G5J2E7).
//!
//! A schedule belongs to one command in one checkout. The local settings file is the
//! authority on this host; ticket tags provide the cross-restart occurrence identity.

use serde::{Deserialize, Serialize};
use time::{Date, Month, PrimitiveDateTime, Time};

pub const SETTING: &str = "command_schedules";
pub const HISTORY_LIMIT: usize = 50;
/// The floor for sub-day repeats (HS2-29V4ZG). Each occurrence writes local settings and
/// creates or checks a ticket, and an AI action can start a session that takes minutes;
/// five minutes matches the common cron-service floor and bounds a runaway schedule to
/// 288 evaluations a day.
pub const MIN_INTERVAL_MINUTES: u16 = 5;
/// A minute interval may span at most one day; longer cadences use Daily or Hourly.
pub const MAX_INTERVAL_MINUTES: u16 = 1440;
/// An hour interval may span at most one day, so every date has a slot to include.
pub const MAX_INTERVAL_HOURS: u16 = 24;

/// What running an AI command does once its ticket exists. A command carries an optional
/// default and a schedule an optional override (HS2-17Y2EC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleAction {
    CreateTicket,
    NewAiSession,
    ReuseOrNewAiSession,
}

/// The action a scheduled occurrence or headless run performs (HS2-17Y2EC): the schedule's
/// explicit override, else the command's own action, else **Create Ticket Only**. A command
/// without an action keeps the legacy sidebar click (queue plus an optional idle-session
/// signal), which schedules never had, so schedules treat it as ticket-only.
pub fn resolve_action(
    schedule_override: Option<ScheduleAction>,
    command_action: Option<ScheduleAction>,
) -> ScheduleAction {
    schedule_override
        .or(command_action)
        .unwrap_or(ScheduleAction::CreateTicket)
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
    /// Every N minutes on the local wall clock from the start date and time (HS2-29V4ZG).
    Minutely,
    /// Every N hours on the local wall clock from the start date and time (HS2-29V4ZG).
    Hourly,
}

impl RecurrenceKind {
    /// Sub-day kinds identify an occurrence by local date and time, not date alone.
    pub fn is_sub_day(self) -> bool {
        matches!(self, Self::Minutely | Self::Hourly)
    }
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
    /// The explicit action override. Absent means "Use command default" (HS2-17Y2EC); a
    /// record stored before HS2-17Y2EC always has one, so it keeps its behavior unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ScheduleAction>,
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
    /// The exact ticket is claimed and the turn may have been sent. A restart must
    /// reconcile this state rather than send the prompt again (HS2-NR0VRF).
    DeliveryDispatching,
    Delivered,
    DeliveryFailed,
    /// The host died before it could confirm whether the provider turn finished.
    DeliveryUncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OccurrenceResult {
    pub occurrence_date: String,
    /// Local `HH:mm` of a sub-day occurrence (HS2-29V4ZG). Absent for date-only kinds,
    /// whose records keep their original shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence_time: Option<String>,
    pub outcome: OccurrenceOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_slug: Option<String>,
    /// None is a pre-HS2-NR0VRF record, whose accepted turns cannot be audited
    /// retroactively. New records use Some(false) until completion is observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_completed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRecord {
    pub schedule: CommandSchedule,
    /// The latest evaluated local date, not a UTC clock instant. A repeated DST hour
    /// therefore cannot evaluate the same occurrence twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_processed_date: Option<String>,
    /// Local `HH:mm` paired with `last_processed_date` for sub-day kinds (HS2-29V4ZG). The
    /// local civil slot, not an instant, is the cursor, so a DST fold cannot replay it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_processed_time: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<OccurrenceResult>,
}

impl OccurrenceResult {
    /// The durable occurrence key: `YYYY-MM-DD`, or `YYYY-MM-DDTHH:mm` for sub-day kinds.
    pub fn key(&self) -> String {
        occurrence_key(&self.occurrence_date, self.occurrence_time.as_deref())
    }
}

impl ScheduleRecord {
    /// The sub-day cursor, when both halves are present and valid.
    pub fn sub_day_cursor(&self) -> Option<PrimitiveDateTime> {
        let date = parse_date(self.last_processed_date.as_deref()?).ok()?;
        let (hour, minute) = parse_local_time(self.last_processed_time.as_deref()?).ok()?;
        Some(date.with_time(Time::from_hms(hour, minute, 0).ok()?))
    }
}

pub fn occurrence_key(date: &str, time: Option<&str>) -> String {
    match time {
        Some(time) => format!("{date}T{time}"),
        None => date.to_owned(),
    }
}

pub fn format_local_time(time: Time) -> String {
    format!("{:02}:{:02}", time.hour(), time.minute())
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
    /// The effective action for this schedule given its command's action (HS2-17Y2EC).
    pub fn effective_action(&self, command_action: Option<ScheduleAction>) -> ScheduleAction {
        resolve_action(self.action, command_action)
    }

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
        match self.kind {
            RecurrenceKind::Minutely => {
                if self.every.is_some_and(|every| {
                    !(MIN_INTERVAL_MINUTES..=MAX_INTERVAL_MINUTES).contains(&every)
                }) {
                    return Err(format!(
                        "minutely every must be between {MIN_INTERVAL_MINUTES} and {MAX_INTERVAL_MINUTES} minutes"
                    ));
                }
            }
            RecurrenceKind::Hourly => {
                if self
                    .every
                    .is_some_and(|every| !(1..=MAX_INTERVAL_HOURS).contains(&every))
                {
                    return Err(format!(
                        "hourly every must be between 1 and {MAX_INTERVAL_HOURS} hours"
                    ));
                }
            }
            _ => {
                if self.every == Some(0) || self.every.is_some_and(|every| every > 365) {
                    return Err("every must be between 1 and 365".into());
                }
            }
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
            | RecurrenceKind::Minutely
            | RecurrenceKind::Hourly
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

/// The sub-day occurrence tag, `hs2-occurrence:<command>:<YYYY-MM-DD>t<HHmm>`. It stays
/// lowercase and colon-free after the date so case-folding providers keep it exact.
pub fn occurrence_tag_at(command_id: &str, date: &str, time: &str) -> String {
    format!(
        "hs2-occurrence:{command_id}:{date}t{}",
        time.replace(':', "")
    )
}

impl Recurrence {
    /// The sub-day step in minutes, or None for date-only kinds.
    pub fn interval_minutes(&self) -> Option<i64> {
        let every = i64::from(self.every?);
        match self.kind {
            RecurrenceKind::Minutely => Some(every),
            RecurrenceKind::Hourly => Some(every * 60),
            _ => None,
        }
    }
}

fn ceil_div(value: i64, divisor: i64) -> i64 {
    -((-value).div_euclid(divisor))
}

/// Enumerate local civil slots of a sub-day recurrence in `(after, through]` (HS2-29V4ZG).
///
/// Slots lie on the local wall-clock grid `start_date + local_time + k * interval`. A slot
/// before the anchor counts only on an include date; an exclude date drops every slot on
/// it; slots after `end_date` stop the sequence; `count` counts grid occurrences from the
/// first one. Time-zone resolution (gaps and folds) belongs to the host.
pub fn slots_through(
    recurrence: &Recurrence,
    local_time: &str,
    after: PrimitiveDateTime,
    through: PrimitiveDateTime,
    limit: usize,
) -> Result<Vec<PrimitiveDateTime>, String> {
    recurrence.validate()?;
    let step = recurrence
        .interval_minutes()
        .ok_or("slots_through requires a minutely or hourly recurrence")?
        * 60;
    if limit == 0 || through <= after {
        return Ok(Vec::new());
    }
    let (hour, minute) = parse_local_time(local_time)?;
    let anchor = parse_date(recurrence.start_date.as_deref().expect("validated start"))?
        .with_time(Time::from_hms(hour, minute, 0).map_err(|error| error.to_string())?);
    let end = recurrence.end_date.as_deref().map(parse_date).transpose()?;
    let parse_all = |values: &[String]| -> Result<std::collections::BTreeSet<Date>, String> {
        values.iter().map(|value| parse_date(value)).collect()
    };
    let include = parse_all(&recurrence.include_dates)?;
    let exclude = parse_all(&recurrence.exclude_dates)?;
    let first = include
        .first()
        .map_or(anchor, |date| date.midnight().min(anchor));
    // Counting needs every earlier occurrence; otherwise start at the window.
    let lower = if recurrence.count.is_some() {
        first
    } else {
        first.max(after)
    };
    let slot_at = |k: i64| anchor.checked_add(time::Duration::seconds(k * step));
    let index_at_or_after =
        |instant: PrimitiveDateTime| ceil_div((instant - anchor).whole_seconds(), step);
    let mut k = index_at_or_after(lower);
    let mut ordinal = 0usize;
    let mut result = Vec::new();
    while let Some(slot) = slot_at(k) {
        if slot > through || end.is_some_and(|end| slot.date() > end) {
            break;
        }
        let date = slot.date();
        if exclude.contains(&date) {
            let Some(next_day) = date.next_day() else {
                break;
            };
            k = index_at_or_after(next_day.midnight());
            continue;
        }
        if slot < anchor && !include.contains(&date) {
            let next = include
                .range(date.next_day().unwrap_or(Date::MAX)..)
                .next()
                .map_or(anchor, |next| next.midnight().min(anchor));
            k = index_at_or_after(next);
            continue;
        }
        ordinal += 1;
        if slot > after {
            result.push(slot);
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
        k += 1;
    }
    Ok(result)
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
    if recurrence.kind.is_sub_day() {
        return Err("sub-day recurrences enumerate slots, not dates".into());
    }
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
            // Rejected above; sub-day kinds use `slots_through`.
            RecurrenceKind::Minutely | RecurrenceKind::Hourly => false,
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
    fn hs2_17y2ec_resolves_override_then_command_then_ticket_only() {
        use ScheduleAction::*;
        assert_eq!(resolve_action(None, None), CreateTicket);
        assert_eq!(resolve_action(None, Some(NewAiSession)), NewAiSession);
        assert_eq!(
            resolve_action(Some(CreateTicket), Some(ReuseOrNewAiSession)),
            CreateTicket
        );
        assert_eq!(
            resolve_action(Some(ReuseOrNewAiSession), None),
            ReuseOrNewAiSession
        );
    }

    #[test]
    fn hs2_17y2ec_legacy_schedule_keeps_stored_action_as_override() {
        // A record written before HS2-17Y2EC, when `action` was required.
        let legacy = serde_json::json!({
            "schedule": {
                "command_id": "daily",
                "time_zone": "UTC",
                "local_time": "09:00",
                "action": "new_ai_session",
                "paused": false,
                "recurrence": {"kind": "once", "date": "2026-12-01"}
            },
            "last_processed_date": "2026-11-30"
        });
        let record: ScheduleRecord = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(record.schedule.action, Some(ScheduleAction::NewAiSession));
        // A command default never replaces the migrated explicit choice.
        assert_eq!(
            record
                .schedule
                .effective_action(Some(ScheduleAction::CreateTicket)),
            ScheduleAction::NewAiSession
        );
        // It round-trips unchanged, so loading never rewrites the stored action.
        assert_eq!(serde_json::to_value(&record).unwrap(), legacy);
        // An inheriting schedule omits the field and follows its command.
        let mut inherit = record.schedule.clone();
        inherit.action = None;
        let value = serde_json::to_value(&inherit).unwrap();
        assert!(value.get("action").is_none());
        let parsed: CommandSchedule = serde_json::from_value(value).unwrap();
        assert_eq!(
            parsed.effective_action(Some(ScheduleAction::ReuseOrNewAiSession)),
            ScheduleAction::ReuseOrNewAiSession
        );
        assert_eq!(parsed.effective_action(None), ScheduleAction::CreateTicket);
        // Unknown action names still fail closed.
        let mut bad = legacy;
        bad["schedule"]["action"] = "run_everything".into();
        assert!(serde_json::from_value::<ScheduleRecord>(bad).is_err());
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

    fn at(raw: &str) -> PrimitiveDateTime {
        let (date, time) = raw.split_once('T').unwrap();
        let (hour, minute) = parse_local_time(time).unwrap();
        parse_date(date)
            .unwrap()
            .with_time(Time::from_hms(hour, minute, 0).unwrap())
    }

    fn render(slots: &[PrimitiveDateTime]) -> Vec<String> {
        slots
            .iter()
            .map(|slot| {
                occurrence_key(
                    &slot.date().to_string(),
                    Some(&format_local_time(slot.time())),
                )
            })
            .collect()
    }

    #[test]
    fn hs2_29v4zg_hourly_and_minutely_slots_follow_the_wall_clock_anchor() {
        let mut hourly = recurrence(RecurrenceKind::Hourly, "2026-11-25");
        hourly.every = Some(5);
        let slots = slots_through(
            &hourly,
            "21:30",
            at("2026-11-25T00:00"),
            at("2026-11-27T00:00"),
            4,
        )
        .unwrap();
        // Continuous across midnight from the anchor, never before it.
        assert_eq!(
            render(&slots),
            [
                "2026-11-25T21:30",
                "2026-11-26T02:30",
                "2026-11-26T07:30",
                "2026-11-26T12:30"
            ]
        );
        let mut minutely = recurrence(RecurrenceKind::Minutely, "2026-11-25");
        minutely.every = Some(15);
        // `after` is exclusive; a slot exactly at `after` is already processed.
        let slots = slots_through(
            &minutely,
            "09:00",
            at("2026-11-25T09:15"),
            at("2026-11-25T10:00"),
            10,
        )
        .unwrap();
        assert_eq!(
            render(&slots),
            ["2026-11-25T09:30", "2026-11-25T09:45", "2026-11-25T10:00"]
        );
        // A window far after the anchor jumps straight to it.
        let slots = slots_through(
            &minutely,
            "09:00",
            at("2030-01-01T00:07"),
            at("2030-01-02T00:00"),
            1,
        )
        .unwrap();
        assert_eq!(render(&slots), ["2030-01-01T00:15"]);
    }

    #[test]
    fn hs2_29v4zg_end_count_include_and_exclude_bound_sub_day_slots() {
        let mut hourly = recurrence(RecurrenceKind::Hourly, "2026-11-25");
        hourly.every = Some(12);
        hourly.end_date = Some("2026-11-28".into());
        hourly.exclude_dates.push("2026-11-26".into());
        hourly.include_dates.push("2026-11-24".into());
        let slots = slots_through(
            &hourly,
            "06:00",
            at("2026-11-20T00:00"),
            at("2026-12-31T00:00"),
            100,
        )
        .unwrap();
        assert_eq!(
            render(&slots),
            [
                // The include date admits grid slots before the anchor.
                "2026-11-24T06:00",
                "2026-11-24T18:00",
                "2026-11-25T06:00",
                "2026-11-25T18:00",
                // 2026-11-26 is excluded; the end date is inclusive.
                "2026-11-27T06:00",
                "2026-11-27T18:00",
                "2026-11-28T06:00",
                "2026-11-28T18:00"
            ]
        );
        hourly.count = Some(3);
        // Count includes occurrences before the window, so only the third remains.
        let slots = slots_through(
            &hourly,
            "06:00",
            at("2026-11-25T00:00"),
            at("2026-12-31T00:00"),
            100,
        )
        .unwrap();
        assert_eq!(render(&slots), ["2026-11-25T06:00"]);
        let slots = slots_through(
            &hourly,
            "06:00",
            at("2026-11-25T06:00"),
            at("2026-12-31T00:00"),
            100,
        )
        .unwrap();
        assert!(slots.is_empty());
    }

    #[test]
    fn hs2_29v4zg_minimum_interval_and_kind_validation() {
        let mut minutely = recurrence(RecurrenceKind::Minutely, "2026-11-25");
        minutely.every = Some(MIN_INTERVAL_MINUTES - 1);
        assert!(
            minutely
                .validate()
                .unwrap_err()
                .contains("between 5 and 1440")
        );
        minutely.every = Some(MIN_INTERVAL_MINUTES);
        minutely.validate().unwrap();
        minutely.every = Some(MAX_INTERVAL_MINUTES + 1);
        assert!(minutely.validate().is_err());
        let mut hourly = recurrence(RecurrenceKind::Hourly, "2026-11-25");
        hourly.every = Some(25);
        assert!(hourly.validate().unwrap_err().contains("between 1 and 24"));
        hourly.every = None;
        assert!(hourly.validate().is_err());
        hourly.every = Some(1);
        hourly.start_date = None;
        assert!(hourly.validate().is_err());
        // Date kinds keep their own bounds and refuse slot enumeration.
        let daily = recurrence(RecurrenceKind::Daily, "2026-11-25");
        assert!(
            slots_through(
                &daily,
                "09:00",
                at("2026-11-25T00:00"),
                at("2026-11-26T00:00"),
                1
            )
            .is_err()
        );
        let mut hourly = recurrence(RecurrenceKind::Hourly, "2026-11-25");
        hourly.every = Some(1);
        assert!(
            dates_through(
                &hourly,
                parse_date("2026-11-24").unwrap(),
                parse_date("2026-11-26").unwrap(),
                1
            )
            .is_err()
        );
        assert_eq!(hourly.interval_minutes(), Some(60));
        assert_eq!(daily.interval_minutes(), None);
    }

    #[test]
    fn hs2_29v4zg_old_records_load_and_reserialize_unchanged() {
        let stored = serde_json::json!([{
            "schedule": {
                "command_id": "daily-summary",
                "time_zone": "America/New_York",
                "local_time": "09:00",
                "action": "create_ticket",
                "recurrence": {"kind": "daily", "start_date": "2026-03-07", "every": 1}
            },
            "last_processed_date": "2026-03-08",
            "history": [{"occurrence_date": "2026-03-08", "outcome": "created", "ticket_slug": "SY-1"}]
        }]);
        let records: Vec<ScheduleRecord> = serde_json::from_value(stored.clone()).unwrap();
        assert_eq!(records[0].last_processed_time, None);
        assert_eq!(records[0].history[0].occurrence_time, None);
        assert_eq!(records[0].history[0].key(), "2026-03-08");
        assert_eq!(records[0].sub_day_cursor(), None);
        let mut round_trip = serde_json::to_value(&records).unwrap();
        // `paused: false` is the only field the current writer always emits.
        round_trip[0]["schedule"]
            .as_object_mut()
            .unwrap()
            .remove("paused");
        assert_eq!(round_trip, stored);
        assert_eq!(
            occurrence_tag("c", "2026-03-08"),
            "hs2-occurrence:c:2026-03-08"
        );

        let sub_day = serde_json::json!({
            "schedule": {
                "command_id": "cleanup", "time_zone": "UTC", "local_time": "09:00",
                "action": "create_ticket", "paused": false,
                "recurrence": {"kind": "hourly", "start_date": "2026-11-25", "every": 1}
            },
            "last_processed_date": "2026-11-25", "last_processed_time": "10:00",
            "history": [{"occurrence_date": "2026-11-25", "occurrence_time": "10:00", "outcome": "created"}]
        });
        let record: ScheduleRecord = serde_json::from_value(sub_day.clone()).unwrap();
        record.schedule.validate().unwrap();
        assert_eq!(record.sub_day_cursor(), Some(at("2026-11-25T10:00")));
        assert_eq!(record.history[0].key(), "2026-11-25T10:00");
        assert_eq!(serde_json::to_value(&record).unwrap(), sub_day);
        assert_eq!(
            occurrence_tag_at("cleanup", "2026-11-25", "10:00"),
            "hs2-occurrence:cleanup:2026-11-25t1000"
        );
    }
}
