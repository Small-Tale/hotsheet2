//! Checkout-local AI command schedules (HS2-G5J2E7).

use crate::api_error::ApiError;
use crate::dto::CreateReq;
use crate::events::ChangeEvent;
use crate::routes::checkout_tickets::checkout_source_for_create;
use crate::routes::project_tools::checkout_settings;
use crate::routes::providers::{do_provider_create, provider_for};
use crate::routes::ticket_writes::do_create;
use crate::routes::{claims, permissions};
use crate::{AppState, multistore};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_ticketing::command_schedules::{
    self, CommandSchedule, OccurrenceResult, Recurrence, ScheduleAction, ScheduleRecord,
};
use hotsheet_ticketing::commands::{self, CommandDefinition, CommandKind};
use hotsheet_ticketing::{ApiTicket, Scope, Settings, TicketQuery, ops};
use jiff::{
    Timestamp, civil,
    tz::{AmbiguousOffset, TimeZone},
};
use serde::{Deserialize, Serialize};
use time::{Date, Duration, Time};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScheduleInput {
    time_zone: String,
    local_time: String,
    action: ScheduleAction,
    #[serde(default)]
    paused: bool,
    recurrence: Recurrence,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScheduleView {
    schedule: CommandSchedule,
    next_runs: Vec<String>,
    history: Vec<OccurrenceResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<String>,
}

fn invalid(message: impl ToString) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, message.to_string())
}

fn emit_changed(state: &AppState, checkout: &str, command_id: &str) {
    state.emit(ChangeEvent {
        live: None,
        cursor: None,
        store: checkout.into(),
        kind: "command_schedules_updated".into(),
        id: command_id.into(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
}

fn records(settings: &Settings) -> Result<Vec<ScheduleRecord>, ApiError> {
    command_schedules::from_settings(settings).map_err(invalid)
}

fn write_records(
    settings: &Settings,
    prior: &[ScheduleRecord],
    next: &[ScheduleRecord],
) -> Result<(), ApiError> {
    let prior = serde_json::to_value(prior).map_err(invalid)?;
    let next = serde_json::to_value(next).map_err(invalid)?;
    let written = settings
        .set_if(command_schedules::SETTING, next, Scope::Local, |current| {
            current.cloned().unwrap_or_else(|| serde_json::json!([])) == prior
        })
        .map_err(invalid)?;
    if written {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::CONFLICT,
            "command schedules changed; reload and retry",
        ))
    }
}

fn view(record: ScheduleRecord, now: Timestamp, available: bool) -> Result<ScheduleView, ApiError> {
    let next_runs = if available {
        next_runs(&record.schedule, now)?
    } else {
        Vec::new()
    };
    Ok(ScheduleView {
        schedule: record.schedule,
        next_runs,
        history: record.history,
        unavailable: (!available)
            .then(|| "AI command is missing or no longer an AI command".into()),
    })
}

fn next_runs(schedule: &CommandSchedule, now: Timestamp) -> Result<Vec<String>, ApiError> {
    if schedule.paused {
        return Ok(Vec::new());
    }
    let zone = TimeZone::get(&schedule.time_zone).map_err(invalid)?;
    let local_today = zone.to_datetime(now).date();
    let after = Date::from_calendar_date(
        local_today.year() as i32,
        time::Month::try_from(local_today.month() as u8).map_err(invalid)?,
        local_today.day() as u8,
    )
    .map_err(invalid)?
    .previous_day()
    .ok_or_else(|| invalid("local date is outside supported range"))?;
    let through = after
        .checked_add(Duration::days(366 * 20))
        .unwrap_or(Date::MAX);
    let dates = command_schedules::dates_through(&schedule.recurrence, after, through, 8)
        .map_err(invalid)?;
    let mut next_runs = Vec::new();
    for date in dates {
        let instant = occurrence_instant(schedule, date)?;
        if instant > now {
            next_runs.push(instant.to_string());
            if next_runs.len() == 3 {
                break;
            }
        }
    }
    Ok(next_runs)
}

fn local_date(zone: &TimeZone, now: Timestamp) -> Result<Date, ApiError> {
    let day = zone.to_datetime(now).date();
    Date::from_calendar_date(
        day.year() as i32,
        time::Month::try_from(day.month() as u8).map_err(invalid)?,
        day.day() as u8,
    )
    .map_err(invalid)
}

fn date_before_today(schedule: &CommandSchedule, now: Timestamp) -> Result<String, ApiError> {
    let zone = TimeZone::get(&schedule.time_zone).map_err(invalid)?;
    local_date(&zone, now)?
        .previous_day()
        .map(|day| day.to_string())
        .ok_or_else(|| invalid("local date is outside supported range"))
}

/// Resolve a local date and time. A gap advances to the first valid minute; a fold
/// chooses its first occurrence. The occurrence's calendar date remains its identity.
pub(crate) fn occurrence_instant(
    schedule: &CommandSchedule,
    date: Date,
) -> Result<Timestamp, ApiError> {
    let zone = TimeZone::get(&schedule.time_zone).map_err(invalid)?;
    let (hour, minute) =
        command_schedules::parse_local_time(&schedule.local_time).map_err(invalid)?;
    let local = date.with_time(Time::from_hms(hour, minute, 0).map_err(invalid)?);
    for step in 0..=1440 {
        let candidate = local
            .checked_add(Duration::minutes(step))
            .ok_or_else(|| invalid("local time overflow"))?;
        let date = candidate.date();
        let time = candidate.time();
        let civil = civil::DateTime::new(
            date.year() as i16,
            date.month() as u8 as i8,
            date.day() as i8,
            time.hour() as i8,
            time.minute() as i8,
            0,
            0,
        )
        .map_err(invalid)?;
        let ambiguous = zone.to_ambiguous_timestamp(civil);
        if !matches!(ambiguous.offset(), AmbiguousOffset::Gap { .. }) {
            return ambiguous.earlier().map_err(invalid);
        }
    }
    Err(invalid("no valid local minute within a day"))
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<ScheduleView>>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let definitions = commands::from_settings(&settings).map_err(invalid)?;
    let mut result = Vec::new();
    for record in records(&settings)? {
        let available = definitions.iter().any(|command| {
            command.id == record.schedule.command_id && command.kind == CommandKind::Ai
        });
        result.push(view(record, Timestamp::now(), available)?);
    }
    Ok(Json(result))
}

/// Preview the draft's next three instants before it is saved.
pub(crate) async fn preview(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(input): Json<ScheduleInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    checkout_settings(&state, &reference)?;
    let schedule = CommandSchedule {
        command_id: "preview".into(),
        time_zone: input.time_zone,
        local_time: input.local_time,
        action: input.action,
        paused: input.paused,
        recurrence: input.recurrence,
    };
    schedule.validate().map_err(invalid)?;
    let zone = TimeZone::get(&schedule.time_zone).map_err(invalid)?;
    if zone.is_unknown() {
        return Err(invalid("unknown time zone"));
    }
    Ok(Json(
        serde_json::json!({"next_runs": next_runs(&schedule, Timestamp::now())?}),
    ))
}

pub(crate) async fn put(
    State(state): State<AppState>,
    Path((reference, command_id)): Path<(String, String)>,
    Json(input): Json<ScheduleInput>,
) -> Result<Json<ScheduleView>, ApiError> {
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    let definitions = commands::from_settings(&settings).map_err(invalid)?;
    if !definitions
        .iter()
        .any(|command| command.id == command_id && command.kind == CommandKind::Ai)
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "schedules require an existing AI command",
        ));
    }
    let schedule = CommandSchedule {
        command_id: command_id.clone(),
        time_zone: input.time_zone,
        local_time: input.local_time,
        action: input.action,
        paused: input.paused,
        recurrence: input.recurrence,
    };
    schedule.validate().map_err(invalid)?;
    let zone = TimeZone::get(&schedule.time_zone).map_err(invalid)?;
    if zone.is_unknown() {
        return Err(invalid("unknown time zone"));
    }
    let now = Timestamp::now();
    if schedule.recurrence.kind == command_schedules::RecurrenceKind::Once {
        let date = command_schedules::parse_date(
            schedule
                .recurrence
                .date
                .as_deref()
                .expect("validated once date"),
        )
        .map_err(invalid)?;
        if date < local_date(&zone, now)? {
            return Err(invalid("one-time schedule date is already past"));
        }
    }
    let initial_cursor = date_before_today(&schedule, now)?;
    let prior = records(&settings)?;
    let mut next = prior.clone();
    let record = if let Some(existing) = next
        .iter_mut()
        .find(|record| record.schedule.command_id == command_id)
    {
        // Editing the same recurrence keeps its occurrence cursor and history. Changing
        // the rule begins a new schedule generation while preserving visible history.
        if existing.schedule.recurrence != schedule.recurrence
            || existing.schedule.local_time != schedule.local_time
            || existing.schedule.time_zone != schedule.time_zone
        {
            existing.last_processed_date = Some(initial_cursor.clone());
        }
        existing.schedule = schedule;
        existing.clone()
    } else {
        let record = ScheduleRecord {
            schedule,
            last_processed_date: Some(initial_cursor),
            history: Vec::new(),
        };
        next.push(record.clone());
        record
    };
    write_records(&settings, &prior, &next)?;
    state.command_schedule_kick.notify_one();
    emit_changed(&state, &checkout.id, &command_id);
    Ok(Json(view(record, Timestamp::now(), true)?))
}

pub(crate) async fn delete(
    State(state): State<AppState>,
    Path((reference, command_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    let prior = records(&settings)?;
    let next: Vec<_> = prior
        .iter()
        .filter(|record| record.schedule.command_id != command_id)
        .cloned()
        .collect();
    if next.len() != prior.len() {
        write_records(&settings, &prior, &next)?;
        state.command_schedule_kick.notify_one();
        emit_changed(&state, &checkout.id, &command_id);
    }
    Ok(StatusCode::NO_CONTENT)
}

fn query_tag(
    state: &AppState,
    source_id: &str,
    tag: String,
    open_only: bool,
) -> Result<Option<ApiTicket>, ApiError> {
    provider_for(state, source_id)?
        .query(&TicketQuery {
            tags: vec![tag.clone()],
            open_only,
            ..TicketQuery::default()
        })
        .map(|tickets| {
            tickets.into_iter().find(|ticket| {
                ticket.tags.iter().any(|candidate| candidate == &tag)
                    && (!open_only
                        || !matches!(
                            ticket.status,
                            hotsheet_model::Status::Completed
                                | hotsheet_model::Status::Verified
                                | hotsheet_model::Status::Deleted
                                | hotsheet_model::Status::Archive
                                | hotsheet_model::Status::Moved
                        ))
            })
        })
        .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))
}

fn create_occurrence_ticket(
    state: &AppState,
    reference: &str,
    source: &hotsheet_ticketing::checkouts::TicketSource,
    command: &CommandDefinition,
    occurrence_tag: String,
) -> Result<ApiTicket, ApiError> {
    let req = CreateReq {
        title: command.title.trim().into(),
        category: Some("task".into()),
        priority: Some("highest".into()),
        status: Some("not_started".into()),
        details: command.prompt.as_deref().map(str::trim).map(str::to_owned),
        tags: Some(vec![
            command_schedules::command_tag(&command.id),
            occurrence_tag,
        ]),
        up_next: Some(true),
        blocked_by: None,
        follow_up_to: None,
        group: None,
        group_resolved: false,
        actor: None,
    };
    if source.provider == "git" {
        let entry = state.hosted_source(source).ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                format!("checkout {reference} has an unhosted ticket source"),
            )
        })?;
        do_create(state, &entry, req)
    } else {
        do_provider_create(state, &source.connection_id, req)
    }
}

/// One durable evaluation pass. A ticket carries its occurrence identity before the
/// local cursor advances, so a crash between create and cursor write reuses it.
pub(crate) fn process_checkout(
    state: &AppState,
    reference: &str,
    now: Timestamp,
) -> Result<Option<Timestamp>, ApiError> {
    let (checkout, settings) = checkout_settings(state, reference)?;
    let definitions = commands::from_settings(&settings).map_err(invalid)?;
    let mut next_wake = None;
    for snapshot in records(&settings)? {
        let schedule = &snapshot.schedule;
        if schedule.paused {
            continue;
        }
        let Some(command) = definitions
            .iter()
            .find(|command| command.id == schedule.command_id && command.kind == CommandKind::Ai)
        else {
            // Preserve the schedule and history if its command is edited or deleted.
            continue;
        };
        let zone = TimeZone::get(&schedule.time_zone).map_err(invalid)?;
        let today = local_date(&zone, now)?;
        let after = snapshot
            .last_processed_date
            .as_deref()
            .map(command_schedules::parse_date)
            .transpose()
            .map_err(invalid)?
            .unwrap_or_else(|| today.previous_day().unwrap_or(today));
        let through = today.next_day().unwrap_or(today);
        let dates = command_schedules::dates_through(&schedule.recurrence, after, through, 65)
            .map_err(invalid)?;
        let full_batch = dates.len() == 65;
        for date in dates {
            let instant = occurrence_instant(schedule, date)?;
            if instant > now {
                next_wake =
                    Some(next_wake.map_or(instant, |previous: Timestamp| previous.min(instant)));
                break;
            }
            let source = checkout_source_for_create(state, reference, None)?;
            let occurrence_tag =
                command_schedules::occurrence_tag(&schedule.command_id, &date.to_string());
            let (ticket, outcome) = if let Some(ticket) =
                query_tag(state, &source.connection_id, occurrence_tag.clone(), false)?
            {
                (
                    ticket,
                    if schedule.action == ScheduleAction::CreateTicket {
                        command_schedules::OccurrenceOutcome::Created
                    } else {
                        command_schedules::OccurrenceOutcome::DeliveryPending
                    },
                )
            } else if let Some(ticket) = query_tag(
                state,
                &source.connection_id,
                command_schedules::command_tag(&schedule.command_id),
                true,
            )? {
                (
                    ticket,
                    command_schedules::OccurrenceOutcome::SkippedOpenTicket,
                )
            } else {
                (
                    create_occurrence_ticket(state, reference, &source, command, occurrence_tag)?,
                    if schedule.action == ScheduleAction::CreateTicket {
                        command_schedules::OccurrenceOutcome::Created
                    } else {
                        command_schedules::OccurrenceOutcome::DeliveryPending
                    },
                )
            };
            // Re-read after the side effect; a concurrent edit must not be overwritten.
            let prior = records(&settings)?;
            let mut next = prior.clone();
            let Some(record) = next
                .iter_mut()
                .find(|record| record.schedule.command_id == schedule.command_id)
            else {
                break;
            };
            if record.schedule != *schedule {
                break;
            }
            let date_string = date.to_string();
            record.last_processed_date = Some(date_string.clone());
            if !record
                .history
                .iter()
                .any(|item| item.occurrence_date == date_string)
            {
                record.history.push(OccurrenceResult {
                    occurrence_date: date_string,
                    outcome,
                    ticket_id: Some(ticket.qualified_id),
                    ticket_slug: Some(ticket.slug),
                });
                if record.history.len() > command_schedules::HISTORY_LIMIT {
                    record.history.remove(0);
                }
            }
            write_records(&settings, &prior, &next)?;
            emit_changed(state, &checkout.id, &schedule.command_id);
        }
        if full_batch {
            // Large offline backlogs continue promptly, without skipping occurrences.
            next_wake = Some(now);
        }
        let after = today;
        let future = after
            .checked_add(Duration::days(366 * 20))
            .unwrap_or(Date::MAX);
        if let Some(date) = command_schedules::dates_through(&schedule.recurrence, after, future, 1)
            .map_err(invalid)?
            .first()
        {
            let instant = occurrence_instant(schedule, *date)?;
            next_wake =
                Some(next_wake.map_or(instant, |previous: Timestamp| previous.min(instant)));
        }
    }
    Ok(next_wake)
}

/// A refused claim is not a delivery failure: another worker owns the exact ticket.
enum DeliveryAttempt {
    Accepted {
        completed: tokio::sync::oneshot::Receiver<bool>,
        worker: String,
    },
    AwaitClaim,
    Failed(&'static str),
}

fn selected_ai(
    command: &CommandDefinition,
    defaults: &hotsheet_plugins::AiToolDefaults,
) -> (String, Option<String>, Option<String>) {
    let tool = command
        .tool
        .as_deref()
        .filter(|tool| !tool.trim().is_empty())
        .unwrap_or(&defaults.tool)
        .to_owned();
    let provider = defaults.providers.get(&tool);
    let primary = (tool == defaults.tool).then_some(defaults);
    (
        tool,
        command
            .model
            .clone()
            .or_else(|| provider.and_then(|entry| entry.model.clone()))
            .or_else(|| primary.and_then(|entry| entry.model.clone())),
        command
            .effort
            .clone()
            .or_else(|| provider.and_then(|entry| entry.effort.clone()))
            .or_else(|| primary.and_then(|entry| entry.effort.clone())),
    )
}

/// A live idle connection is compatible only when its checkout, ticket source, tool,
/// model, and effort agree. A scheduled new-session id is stable across retries.
struct DeliverySession<'a> {
    project: &'a str,
    source: &'a str,
    tool: &'a str,
    model: Option<&'a str>,
    effort: Option<&'a str>,
    action: ScheduleAction,
    ticket_id: &'a str,
}

fn delivery_connection(state: &AppState, target: DeliverySession<'_>) -> (String, Option<String>) {
    let own_id = format!(
        "hotsheet-scheduled-{}",
        target.ticket_id.to_ascii_lowercase()
    );
    if target.action == ScheduleAction::ReuseOrNewAiSession {
        let mut live = state.client_drives.list();
        live.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(connection) = live.into_iter().find(|connection| {
            connection.project == target.project
                && connection.source == target.source
                && connection.tool == target.tool
                && !connection.busy
                && connection.actions.iter().any(|item| item == "send_turn")
                && target
                    .model
                    .is_none_or(|value| connection.model.as_deref() == Some(value))
                && target
                    .effort
                    .is_none_or(|value| connection.effort.as_deref() == Some(value))
        }) {
            return (connection.id, None);
        }
        // After a host restart, the durable session catalog can resume a compatible
        // provider session even though its live connection has gone away.
        if target.model.is_none() && target.effort.is_none() {
            if let Some(session) = state
                .client_drives
                .sessions(target.project)
                .into_iter()
                .filter(|session| session.tool == target.tool)
                .max_by_key(|session| session.updated_at_ms)
            {
                return (own_id, Some(session.session_id));
            }
        }
    }
    (own_id, None)
}

struct DeliveryTarget<'a> {
    reference: &'a str,
    project: &'a str,
    settings: &'a Settings,
    command: &'a CommandDefinition,
    action: ScheduleAction,
    ticket_id: &'a str,
    ticket_slug: &'a str,
}

async fn deliver_one(state: &AppState, target: DeliveryTarget<'_>) -> DeliveryAttempt {
    let defaults = match permissions::effective_ai_settings(state, Some(target.settings)).await {
        Ok(defaults) => defaults,
        Err(_) => return DeliveryAttempt::Failed("ai_provider_unavailable"),
    };
    let (tool, model, effort) = selected_ai(target.command, &defaults);
    let (entry, native_id) =
        match claims::checkout_claim_entry(state, target.reference, target.ticket_id) {
            Ok(owner) => owner,
            Err(_) => return DeliveryAttempt::Failed("ticket_source_unavailable"),
        };
    let ticket = match entry.resolve(&native_id) {
        Ok(Some(ticket)) => ticket,
        _ => return DeliveryAttempt::Failed("ticket_unavailable"),
    };
    let source = multistore::store_url_id(&entry.store);
    let (connection_id, session_id) = delivery_connection(
        state,
        DeliverySession {
            project: target.project,
            source: &source,
            tool: &tool,
            model: model.as_deref(),
            effort: effort.as_deref(),
            action: target.action,
            ticket_id: &ticket.id.to_string(),
        },
    );
    let worker = hotsheet_aitools::session_worker_id(&tool, &connection_id);
    let now = crate::now();
    let claimed = ops::claim_with_eta(
        &entry.store,
        &ticket.id,
        &now,
        now.plus_minutes(30),
        &worker,
        Some("Scheduled AI command".into()),
        None,
    );
    match claimed {
        Ok(ticket) => state.changed_in(&entry, "claimed", &ticket),
        Err(ops::OpError::WrongWorker { .. } | ops::OpError::ClaimUnavailable { .. }) => {
            return DeliveryAttempt::AwaitClaim;
        }
        Err(_) => return DeliveryAttempt::Failed("ticket_claim_failed"),
    }
    let prompt = format!(
        "$hotsheet Work the exact ticket {} (qualified id {}) now. Read its full details and notes before acting. The ticket is already claimed by this AI session.",
        target.ticket_slug, target.ticket_id
    );
    let delivered = permissions::send_scheduled_drive_turn(
        state,
        permissions::ScheduledDriveTurn {
            checkout: target.reference.to_owned(),
            source,
            connection_id,
            session_id,
            tool,
            model,
            effort,
            content: prompt,
        },
    )
    .await;
    match delivered {
        Ok(completed) => DeliveryAttempt::Accepted { completed, worker },
        Err(_) => {
            if let Ok(ticket) = ops::release(&entry.store, &ticket.id, crate::now(), &worker, false)
            {
                state.changed_in(&entry, "released", &ticket);
            }
            DeliveryAttempt::Failed("ai_handoff_failed")
        }
    }
}

/// Persist the outcome against the same ticket identity, even if another settings
/// writer changed a different schedule while the provider was being prepared.
fn set_delivery_outcome(
    settings: &Settings,
    command_id: &str,
    occurrence_date: &str,
    ticket_id: &str,
    outcome: command_schedules::OccurrenceOutcome,
    allow_delivered: bool,
) -> Result<Option<command_schedules::OccurrenceOutcome>, ApiError> {
    for _ in 0..4 {
        let prior = records(settings)?;
        let mut next = prior.clone();
        let Some(item) = next
            .iter_mut()
            .find(|record| record.schedule.command_id == command_id)
            .and_then(|record| {
                record.history.iter_mut().find(|item| {
                    item.occurrence_date == occurrence_date
                        && item.ticket_id.as_deref() == Some(ticket_id)
                })
            })
        else {
            return Ok(None);
        };
        if !matches!(
            item.outcome,
            command_schedules::OccurrenceOutcome::DeliveryPending
                | command_schedules::OccurrenceOutcome::DeliveryFailed
        ) && !(allow_delivered
            && item.outcome == command_schedules::OccurrenceOutcome::Delivered)
        {
            return Ok(None);
        }
        let previous = item.outcome;
        item.outcome = outcome;
        if write_records(settings, &prior, &next).is_ok() {
            return Ok(Some(previous));
        }
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "command delivery history changed; retry on next wake",
    ))
}

fn note_delivery_failure(state: &AppState, reference: &str, ticket_id: &str, reason: &str) {
    let Ok((entry, native_id)) = claims::checkout_claim_entry(state, reference, ticket_id) else {
        return;
    };
    let Ok(Some(ticket)) = entry.resolve(&native_id) else {
        return;
    };
    let note = ops::add_note_with_metadata(
        &entry.store,
        &ticket.id,
        hotsheet_model::Ulid::new(),
        crate::now(),
        hotsheet_model::NoteKind::Regular,
        ops::NoteMetadataInput {
            summary: Some("Scheduled AI delivery will retry".into()),
            actor: Some(hotsheet_model::NoteActor {
                role: hotsheet_model::AttachmentActorRole::System,
                id: Some("hotsheet-command-scheduler".into()),
            }),
            ..ops::NoteMetadataInput::default()
        },
        format!(
            "The scheduled AI command could not be delivered ({reason}). This same ticket will be retried."
        ),
    );
    if let Ok(ticket) = note {
        state.changed_in(&entry, "note_added", &ticket);
    }
}

async fn deliver_pending(state: &AppState, reference: &str) -> Result<(), ApiError> {
    let (checkout, settings) = checkout_settings(state, reference)?;
    let definitions = commands::from_settings(&settings).map_err(invalid)?;
    for record in records(&settings)? {
        if record.schedule.action == ScheduleAction::CreateTicket {
            continue;
        }
        for item in record.history.iter().filter(|item| {
            matches!(
                item.outcome,
                command_schedules::OccurrenceOutcome::DeliveryPending
                    | command_schedules::OccurrenceOutcome::DeliveryFailed
            )
        }) {
            let Some(ticket_id) = item.ticket_id.as_deref() else {
                continue;
            };
            let Some(ticket_slug) = item.ticket_slug.as_deref() else {
                continue;
            };
            let result = if let Some(command) = definitions.iter().find(|command| {
                command.id == record.schedule.command_id && command.kind == CommandKind::Ai
            }) {
                deliver_one(
                    state,
                    DeliveryTarget {
                        reference,
                        project: &checkout.root,
                        settings: &settings,
                        command,
                        action: record.schedule.action,
                        ticket_id,
                        ticket_slug,
                    },
                )
                .await
            } else {
                DeliveryAttempt::Failed("ai_command_unavailable")
            };
            let (outcome, reason, completed) = match result {
                DeliveryAttempt::Accepted { completed, worker } => (
                    command_schedules::OccurrenceOutcome::Delivered,
                    None,
                    Some((completed, worker)),
                ),
                DeliveryAttempt::AwaitClaim => continue,
                DeliveryAttempt::Failed(reason) => (
                    command_schedules::OccurrenceOutcome::DeliveryFailed,
                    Some(reason),
                    None,
                ),
            };
            let previous = set_delivery_outcome(
                &settings,
                &record.schedule.command_id,
                &item.occurrence_date,
                ticket_id,
                outcome,
                false,
            )?;
            if previous.is_some() {
                emit_changed(state, &checkout.id, &record.schedule.command_id);
            }
            if let (Some(reason), Some(previous)) = (reason, previous) {
                if previous != command_schedules::OccurrenceOutcome::DeliveryFailed {
                    note_delivery_failure(state, reference, ticket_id, reason);
                }
            }
            if let Some((completed, worker)) = completed {
                let state = state.clone();
                let reference = reference.to_owned();
                let command_id = record.schedule.command_id.clone();
                let occurrence_date = item.occurrence_date.clone();
                let ticket_id = ticket_id.to_owned();
                tokio::spawn(async move {
                    if completed.await != Ok(true) {
                        if let Ok((checkout, settings)) = checkout_settings(&state, &reference) {
                            if let Ok(Some(previous)) = set_delivery_outcome(
                                &settings,
                                &command_id,
                                &occurrence_date,
                                &ticket_id,
                                command_schedules::OccurrenceOutcome::DeliveryFailed,
                                true,
                            ) {
                                emit_changed(&state, &checkout.id, &command_id);
                                if previous != command_schedules::OccurrenceOutcome::DeliveryFailed
                                {
                                    note_delivery_failure(
                                        &state,
                                        &reference,
                                        &ticket_id,
                                        "ai_turn_failed",
                                    );
                                }
                            }
                        }
                        if let Ok((entry, native_id)) =
                            claims::checkout_claim_entry(&state, &reference, &ticket_id)
                        {
                            if let Ok(Some(ticket)) = entry.resolve(&native_id) {
                                if let Ok(ticket) = ops::release(
                                    &entry.store,
                                    &ticket.id,
                                    crate::now(),
                                    &worker,
                                    false,
                                ) {
                                    state.changed_in(&entry, "released", &ticket);
                                }
                            }
                        }
                    }
                });
            }
        }
    }
    Ok(())
}

impl AppState {
    /// Run once at startup and after schedule writes; the fallback wake only reads
    /// checkout-local settings, never the network (HS2-G5J2E7).
    pub fn spawn_command_schedule_runner(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            loop {
                if state.is_stopping() {
                    return;
                }
                let work = state.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut next = None;
                    let mut checkouts = Vec::new();
                    for checkout in work.checkout_registry.list().map_err(invalid)? {
                        match process_checkout(&work, &checkout.id, Timestamp::now()) {
                            Ok(Some(instant)) => next = Some(next.map_or(instant, |old: Timestamp| old.min(instant))),
                            Ok(None) => {},
                            Err(error) => tracing::warn!(checkout = %checkout.id, error = %error.message, "command schedule evaluation failed"),
                        }
                        checkouts.push(checkout.id);
                    }
                    Ok::<_, ApiError>((next, checkouts))
                }).await;
                let (next, checkouts) = match result {
                    Ok(Ok(result)) => result,
                    Ok(Err(error)) => {
                        tracing::warn!(error = %error.message, "command schedule scan failed");
                        (None, Vec::new())
                    }
                    Err(error) => {
                        tracing::warn!(%error, "command schedule worker failed");
                        (None, Vec::new())
                    }
                };
                for checkout in checkouts {
                    if let Err(error) = deliver_pending(&state, &checkout).await {
                        tracing::warn!(checkout, error = %error.message, "scheduled AI delivery failed");
                    }
                }
                let sleep = next
                    .and_then(|instant| {
                        let nanos =
                            (instant.as_nanosecond() - Timestamp::now().as_nanosecond()).max(0);
                        u64::try_from(nanos)
                            .ok()
                            .map(std::time::Duration::from_nanos)
                    })
                    .unwrap_or(std::time::Duration::from_secs(60))
                    .min(std::time::Duration::from_secs(60));
                tokio::select! {
                    _ = tokio::time::sleep(sleep) => {},
                    _ = state.command_schedule_kick.notified() => {},
                    _ = state.stopping() => return,
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_drive::{
        ClientDriveBackend, ClientTurnRequest, PrepareDrive, PreparedClientDrive,
    };
    use axum::{body::Body, http::Request};
    use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
    use serde_json::json;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use tower::ServiceExt;

    type PreparationRecord = (String, Option<String>, Option<String>, Option<String>);

    #[derive(Default, Clone)]
    struct ScheduledFakeDrive {
        preparations: Arc<Mutex<Vec<PreparationRecord>>>,
        turns: Arc<Mutex<Vec<String>>>,
        fail_prepare_once: Arc<AtomicBool>,
        fail_turn_once: Arc<AtomicBool>,
    }

    struct ScheduledPreparedDrive {
        tool: String,
        turns: Arc<Mutex<Vec<String>>>,
        fail_turn_once: Arc<AtomicBool>,
    }

    impl ClientDriveBackend for ScheduledFakeDrive {
        fn prepare(&self, request: PrepareDrive) -> Result<Arc<dyn PreparedClientDrive>, String> {
            let store = FsStore::open(&request.store_path).map_err(|error| error.to_string())?;
            let claimed = ops::query(&store, &TicketQuery::default())
                .map_err(|error| error.to_string())?
                .first()
                .and_then(|ticket| ticket.claimed_by.clone());
            self.preparations.lock().unwrap().push((
                request.tool.clone(),
                request.model,
                request.effort,
                claimed,
            ));
            if self.fail_prepare_once.swap(false, Ordering::SeqCst) {
                return Err("synthetic provider unavailable".into());
            }
            Ok(Arc::new(ScheduledPreparedDrive {
                tool: request.tool,
                turns: self.turns.clone(),
                fail_turn_once: self.fail_turn_once.clone(),
            }))
        }
    }

    impl PreparedClientDrive for ScheduledPreparedDrive {
        fn tool(&self) -> &str {
            &self.tool
        }
        fn supports_interrupt(&self) -> bool {
            false
        }
        fn run_turn(
            &self,
            request: ClientTurnRequest<'_>,
            _on_event: &mut dyn FnMut(&hotsheet_aitools::TurnEvent),
        ) -> Result<hotsheet_aitools::TurnDone, String> {
            self.turns.lock().unwrap().push(request.prompt.into());
            if self.fail_turn_once.swap(false, Ordering::SeqCst) {
                return Ok(hotsheet_aitools::TurnDone {
                    reason: hotsheet_aitools::DoneReason::failed(1, "synthetic provider limit"),
                    session_id: Some("synthetic-session".into()),
                    session_cost_usd: None,
                });
            }
            Ok(hotsheet_aitools::TurnDone {
                reason: hotsheet_aitools::DoneReason::Completed,
                session_id: Some("synthetic-session".into()),
                session_cost_usd: None,
            })
        }
    }

    fn with_scheduled_backend(state: AppState, backend: ScheduledFakeDrive) -> AppState {
        state
            .with_ai_tool_discovery(std::time::Duration::from_secs(3600), |_, _, _, _| {
                vec![
                    hotsheet_plugins::AiToolDescriptor {
                        id: "codex".into(),
                        display_name: "Codex".into(),
                        ..Default::default()
                    },
                    hotsheet_plugins::AiToolDescriptor {
                        id: "claude".into(),
                        display_name: "Claude".into(),
                        ..Default::default()
                    },
                ]
            })
            .with_client_drive_backend(Arc::new(backend))
    }

    fn fixture() -> (tempfile::TempDir, AppState, String, Settings, FsStore) {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let store_path = root.path().join("tickets");
        std::fs::create_dir(&project).unwrap();
        assert!(
            hotsheet_ticketing::git::launch("git")
                .args(["init", "-q"])
                .arg(&project)
                .status()
                .unwrap()
                .success()
        );
        let store = FsStore::init_git(&store_path, &StoreMetadata::new("SY")).unwrap();
        let state = AppState::new(store.clone(), "synthetic-secret".into())
            .unwrap()
            .with_checkout_registry(root.path().join("checkouts.json"));
        let checkout = state
            .checkout_registry
            .register(&project, None, None, vec![store_path])
            .unwrap();
        let settings = checkout.settings();
        settings.set("commands", json!([{"id":"daily-summary","title":"Daily summary","kind":"ai","prompt":"Summarize synthetic work"}]), Scope::Local).unwrap();
        (root, state, checkout.id, settings, store)
    }

    fn sample_record(cursor: &str, action: ScheduleAction) -> ScheduleRecord {
        ScheduleRecord {
            schedule: CommandSchedule {
                command_id: "daily-summary".into(),
                time_zone: "America/New_York".into(),
                local_time: "09:00".into(),
                action,
                paused: false,
                recurrence: Recurrence {
                    kind: command_schedules::RecurrenceKind::Daily,
                    date: None,
                    start_date: Some("2026-03-07".into()),
                    every: Some(1),
                    weekdays: Vec::new(),
                    on: None,
                    month: None,
                    dates: Vec::new(),
                    end_date: None,
                    count: None,
                    include_dates: Vec::new(),
                    exclude_dates: Vec::new(),
                },
            },
            last_processed_date: Some(cursor.into()),
            history: Vec::new(),
        }
    }

    fn due_ticket(
        state: &AppState,
        checkout: &str,
        settings: &Settings,
        action: ScheduleAction,
    ) -> (String, String) {
        settings
            .set(
                command_schedules::SETTING,
                json!([sample_record("2026-03-07", action)]),
                Scope::Local,
            )
            .unwrap();
        process_checkout(
            state,
            checkout,
            Timestamp::from_second(1_772_980_200).unwrap(),
        )
        .unwrap();
        let item = records(settings).unwrap()[0].history[0].clone();
        (item.ticket_id.unwrap(), item.ticket_slug.unwrap())
    }

    #[test]
    fn hs2_mb3bqe_resolves_command_and_provider_defaults_without_cross_provider_model() {
        let (_root, _state, _checkout, settings, _store) = fixture();
        let mut command = commands::from_settings(&settings).unwrap().remove(0);
        let mut defaults = hotsheet_plugins::AiToolDefaults {
            tool: "claude".into(),
            model: Some("opus".into()),
            effort: Some("high".into()),
            ..Default::default()
        };
        defaults.providers.insert(
            "codex".into(),
            hotsheet_plugins::AiProviderDefaults {
                model: Some("codex-model".into()),
                effort: Some("medium".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            selected_ai(&command, &defaults),
            ("claude".into(), Some("opus".into()), Some("high".into()))
        );
        command.tool = Some("codex".into());
        assert_eq!(
            selected_ai(&command, &defaults),
            (
                "codex".into(),
                Some("codex-model".into()),
                Some("medium".into())
            )
        );
        command.model = Some("explicit-model".into());
        assert_eq!(
            selected_ai(&command, &defaults).1.as_deref(),
            Some("explicit-model")
        );
    }

    #[tokio::test]
    async fn hs2_mb3bqe_new_session_claims_exact_ticket_before_handoff() {
        let (_root, state, checkout, settings, store) = fixture();
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        let (ticket_id, ticket_slug) =
            due_ticket(&state, &checkout, &settings, ScheduleAction::NewAiSession);
        deliver_pending(&state, &checkout).await.unwrap();
        let history = records(&settings).unwrap()[0].history.clone();
        assert_eq!(
            history[0].outcome,
            command_schedules::OccurrenceOutcome::Delivered
        );
        assert_eq!(history[0].ticket_id.as_deref(), Some(ticket_id.as_str()));
        let ticket = ops::query(&store, &TicketQuery::default())
            .unwrap()
            .remove(0);
        assert_eq!(ticket.slug, ticket_slug);
        let expected_worker = hotsheet_aitools::session_worker_id(
            "codex",
            &format!(
                "hotsheet-scheduled-{}",
                ticket.id.to_string().to_ascii_lowercase()
            ),
        );
        assert_eq!(ticket.claimed_by.as_deref(), Some(expected_worker.as_str()));
        {
            let preparations = backend.preparations.lock().unwrap();
            assert_eq!(preparations.len(), 1);
            assert_eq!(preparations[0].0, "codex");
            assert_eq!(preparations[0].3.as_deref(), Some(expected_worker.as_str()));
        }
        deliver_pending(&state, &checkout).await.unwrap();
        assert_eq!(backend.preparations.lock().unwrap().len(), 1);
        assert_eq!(
            ops::query(&store, &TicketQuery::default()).unwrap().len(),
            1
        );
        let response = crate::app(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/checkouts/{checkout}/command-schedules"))
                    .header("x-hotsheet-secret", "synthetic-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let listed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(listed[0]["history"][0]["outcome"], "delivered");
        assert_eq!(listed[0]["history"][0]["ticket_id"], ticket_id);
    }

    #[tokio::test]
    async fn hs2_mb3bqe_command_provider_override_and_ticket_only_action() {
        let (_root, state, checkout, settings, _store) = fixture();
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        due_ticket(&state, &checkout, &settings, ScheduleAction::CreateTicket);
        deliver_pending(&state, &checkout).await.unwrap();
        assert!(backend.preparations.lock().unwrap().is_empty());
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::Created
        );
        let (_root, state, checkout, settings, _store) = fixture();
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        settings
            .set(
                "commands",
                json!([{"id":"daily-summary","title":"Daily summary","kind":"ai","prompt":"Synthetic work","tool":"claude"}]),
                Scope::Local,
            )
            .unwrap();
        due_ticket(&state, &checkout, &settings, ScheduleAction::NewAiSession);
        deliver_pending(&state, &checkout).await.unwrap();
        assert_eq!(backend.preparations.lock().unwrap()[0].0, "claude");
    }

    #[tokio::test]
    async fn hs2_mb3bqe_reuse_waits_for_claim_then_uses_compatible_idle_session() {
        let (_root, state, checkout, settings, store) = fixture();
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        let source = multistore::store_url_id(&store);
        std::mem::drop(
            permissions::send_scheduled_drive_turn(
                &state,
                permissions::ScheduledDriveTurn {
                    checkout: checkout.clone(),
                    source: source.clone(),
                    connection_id: "existing-session".into(),
                    session_id: None,
                    tool: "codex".into(),
                    model: None,
                    effort: None,
                    content: "synthetic setup".into(),
                },
            )
            .await
            .unwrap(),
        );
        let project = state.checkout_registry.resolve(&checkout).unwrap();
        let (mismatched, _) = delivery_connection(
            &state,
            DeliverySession {
                project: &project.root,
                source: &source,
                tool: "codex",
                model: Some("another-model"),
                effort: None,
                action: ScheduleAction::ReuseOrNewAiSession,
                ticket_id: "synthetic-ticket-id",
            },
        );
        assert_ne!(mismatched, "existing-session");
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while state.client_drives.get("existing-session").unwrap().busy {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let (ticket_id, _) = due_ticket(
            &state,
            &checkout,
            &settings,
            ScheduleAction::ReuseOrNewAiSession,
        );
        let ticket = ops::query(&store, &TicketQuery::default())
            .unwrap()
            .remove(0);
        let now = crate::now();
        ops::claim(
            &store,
            &ticket.id,
            &now,
            now.plus_minutes(30),
            "another-worker",
            None,
        )
        .unwrap();
        deliver_pending(&state, &checkout).await.unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::DeliveryPending
        );
        assert_eq!(backend.preparations.lock().unwrap().len(), 1);
        ops::release(&store, &ticket.id, crate::now(), "another-worker", false).unwrap();
        deliver_pending(&state, &checkout).await.unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::Delivered
        );
        assert_eq!(
            records(&settings).unwrap()[0].history[0]
                .ticket_id
                .as_deref(),
            Some(ticket_id.as_str())
        );
        assert_eq!(backend.preparations.lock().unwrap().len(), 1);
        let ticket = ops::query(&store, &TicketQuery::default())
            .unwrap()
            .remove(0);
        assert_eq!(
            ticket.claimed_by.as_deref(),
            Some(hotsheet_aitools::session_worker_id("codex", "existing-session").as_str())
        );
    }

    #[tokio::test]
    async fn hs2_mb3bqe_failed_delivery_retries_same_ticket_after_host_restart() {
        let (root, state, checkout, settings, store) = fixture();
        let backend = ScheduledFakeDrive::default();
        backend.fail_prepare_once.store(true, Ordering::SeqCst);
        let state = with_scheduled_backend(state, backend.clone());
        let (ticket_id, _) = due_ticket(&state, &checkout, &settings, ScheduleAction::NewAiSession);
        deliver_pending(&state, &checkout).await.unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::DeliveryFailed
        );
        let ticket = ops::query(&store, &TicketQuery::default())
            .unwrap()
            .remove(0);
        assert!(ticket.claimed_by.is_none());
        assert!(
            ticket
                .notes
                .iter()
                .any(|note| note.text.contains("same ticket will be retried"))
        );
        let recovered = AppState::new(store.clone(), "synthetic-secret".into())
            .unwrap()
            .with_checkout_registry(root.path().join("checkouts.json"));
        let recovered = with_scheduled_backend(recovered, backend.clone());
        deliver_pending(&recovered, &checkout).await.unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::Delivered
        );
        assert_eq!(
            records(&settings).unwrap()[0].history[0]
                .ticket_id
                .as_deref(),
            Some(ticket_id.as_str())
        );
        assert_eq!(
            ops::query(&store, &TicketQuery::default()).unwrap().len(),
            1
        );
        assert_eq!(backend.preparations.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn hs2_mb3bqe_failed_provider_turn_releases_claim_and_retries_same_ticket() {
        let (_root, state, checkout, settings, store) = fixture();
        let backend = ScheduledFakeDrive::default();
        backend.fail_turn_once.store(true, Ordering::SeqCst);
        let state = with_scheduled_backend(state, backend.clone());
        let (ticket_id, _) = due_ticket(&state, &checkout, &settings, ScheduleAction::NewAiSession);
        deliver_pending(&state, &checkout).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while records(&settings).unwrap()[0].history[0].outcome
                != command_schedules::OccurrenceOutcome::DeliveryFailed
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let ticket = ops::query(&store, &TicketQuery::default())
            .unwrap()
            .remove(0);
        assert!(ticket.claimed_by.is_none());
        assert!(
            ticket
                .notes
                .iter()
                .any(|note| note.text.contains("ai_turn_failed"))
        );
        deliver_pending(&state, &checkout).await.unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::Delivered
        );
        assert_eq!(
            records(&settings).unwrap()[0].history[0]
                .ticket_id
                .as_deref(),
            Some(ticket_id.as_str())
        );
        assert_eq!(
            ops::query(&store, &TicketQuery::default()).unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn hs2_mb3bqe_host_startup_delivers_existing_pending_one_time_ticket() {
        let (_root, state, checkout, settings, store) = fixture();
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        let (ticket_id, _) = due_ticket(&state, &checkout, &settings, ScheduleAction::NewAiSession);
        let mut stored = records(&settings).unwrap();
        stored[0].schedule.recurrence.kind = command_schedules::RecurrenceKind::Once;
        stored[0].schedule.recurrence.date = Some("2026-03-08".into());
        stored[0].schedule.recurrence.start_date = None;
        stored[0].last_processed_date = Some("2026-03-08".into());
        settings
            .set(command_schedules::SETTING, json!(stored), Scope::Local)
            .unwrap();

        state.spawn_command_schedule_runner();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while records(&settings).unwrap()[0].history[0].outcome
                != command_schedules::OccurrenceOutcome::Delivered
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        state.begin_stopping();
        assert_eq!(
            records(&settings).unwrap()[0].history[0]
                .ticket_id
                .as_deref(),
            Some(ticket_id.as_str())
        );
        assert_eq!(
            ops::query(&store, &TicketQuery::default()).unwrap().len(),
            1
        );
        assert_eq!(backend.preparations.lock().unwrap().len(), 1);
    }

    #[test]
    fn hs2_g5j2e7_restart_reuses_occurrence_ticket_and_preserves_history() {
        let (_root, state, checkout, settings, store) = fixture();
        settings
            .set(
                command_schedules::SETTING,
                json!([sample_record("2026-03-07", ScheduleAction::CreateTicket)]),
                Scope::Local,
            )
            .unwrap();
        let now = Timestamp::from_second(1_772_980_200).unwrap(); // 2026-03-08 10:30 EDT
        process_checkout(&state, &checkout, now).unwrap();
        let created = ops::query(&store, &TicketQuery::default()).unwrap();
        assert_eq!(created.len(), 1);
        assert!(created[0].tags.contains(&command_schedules::occurrence_tag(
            "daily-summary",
            "2026-03-08"
        )));
        assert!(created[0].up_next);
        let mut stale = records(&settings).unwrap();
        stale[0].last_processed_date = Some("2026-03-07".into());
        stale[0].history.clear();
        settings
            .set(command_schedules::SETTING, json!(stale), Scope::Local)
            .unwrap();
        process_checkout(&state, &checkout, now).unwrap();
        assert_eq!(
            ops::query(&store, &TicketQuery::default()).unwrap().len(),
            1
        );
        let recovered = records(&settings).unwrap();
        assert_eq!(recovered[0].history.len(), 1);
        assert_eq!(recovered[0].history[0].occurrence_date, "2026-03-08");
    }

    #[test]
    fn hs2_g5j2e7_manual_incomplete_ticket_skips_scheduled_occurrence() {
        let (_root, state, checkout, settings, store) = fixture();
        settings
            .set(
                command_schedules::SETTING,
                json!([sample_record("2026-03-07", ScheduleAction::CreateTicket)]),
                Scope::Local,
            )
            .unwrap();
        let manual = ops::create(
            &store,
            hotsheet_model::Ulid::new(),
            "SY",
            hotsheet_model::Timestamp::new("2026-03-07T00:00:00Z"),
            NewTicket {
                title: "Manual summary".into(),
                category: "task".into(),
                tags: vec![command_schedules::command_tag("daily-summary")],
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap();
        let now = Timestamp::from_second(1_772_980_200).unwrap();
        process_checkout(&state, &checkout, now).unwrap();
        assert_eq!(
            ops::query(&store, &TicketQuery::default()).unwrap().len(),
            1
        );
        let history = &records(&settings).unwrap()[0].history;
        assert_eq!(
            history[0].outcome,
            command_schedules::OccurrenceOutcome::SkippedOpenTicket
        );
        assert_eq!(
            history[0].ticket_slug.as_deref(),
            Some(manual.slug.as_str())
        );
    }

    #[test]
    fn hs2_g5j2e7_dst_gap_runs_at_next_valid_time_and_fold_once() {
        let mut schedule = sample_record("2026-03-07", ScheduleAction::CreateTicket).schedule;
        schedule.local_time = "02:30".into();
        let gap = command_schedules::parse_date("2026-03-08").unwrap();
        let instant = occurrence_instant(&schedule, gap).unwrap();
        assert_eq!(instant.to_string(), "2026-03-08T07:00:00Z");
        schedule.local_time = "01:30".into();
        let fold = command_schedules::parse_date("2026-11-01").unwrap();
        let instant = occurrence_instant(&schedule, fold).unwrap();
        assert_eq!(instant.to_string(), "2026-11-01T05:30:00Z");
    }

    #[test]
    fn hs2_g5j2e7_missed_one_time_occurrence_is_created_on_host_return() {
        let (_root, state, checkout, settings, store) = fixture();
        let mut record = sample_record("2026-03-07", ScheduleAction::CreateTicket);
        record.schedule.recurrence.kind = command_schedules::RecurrenceKind::Once;
        record.schedule.recurrence.date = Some("2026-03-08".into());
        record.schedule.recurrence.start_date = None;
        record.schedule.recurrence.every = None;
        settings
            .set(command_schedules::SETTING, json!([record]), Scope::Local)
            .unwrap();
        let host_returns = Timestamp::from_second(1_773_152_000).unwrap();
        process_checkout(&state, &checkout, host_returns).unwrap();
        let tickets = ops::query(&store, &TicketQuery::default()).unwrap();
        assert_eq!(tickets.len(), 1);
        assert!(tickets[0].tags.contains(&command_schedules::occurrence_tag(
            "daily-summary",
            "2026-03-08"
        )));
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::Created
        );
    }

    #[tokio::test]
    async fn hs2_g5j2e7_schedule_api_persists_lists_and_deletes() {
        let (_root, state, checkout, _settings, _store) = fixture();
        let router = crate::app(state);
        let path = format!("/checkouts/{checkout}/command-schedules/daily-summary");
        let input = json!({
            "time_zone":"America/New_York", "local_time":"09:00",
            "action":"create_ticket", "paused":false,
            "recurrence":{"kind":"monthly", "start_date":"2026-12-01", "every":1,
                "on":{"kind":"weekday_ordinal", "ordinal":1,"weekday":2}}
        });
        let request = |method: &str, uri: &str, body: Option<String>| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("x-hotsheet-secret", "synthetic-secret")
                .header("content-type", "application/json")
                .body(body.map_or_else(Body::empty, Body::from))
                .unwrap()
        };
        let preview = router
            .clone()
            .oneshot(request(
                "POST",
                &format!("/checkouts/{checkout}/command-schedules/preview"),
                Some(input.to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(preview.status(), StatusCode::OK);
        let body = axum::body::to_bytes(preview.into_body(), usize::MAX)
            .await
            .unwrap();
        let draft: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(draft["next_runs"].as_array().unwrap().len(), 3);
        let saved = router
            .clone()
            .oneshot(request("PUT", &path, Some(input.to_string())))
            .await
            .unwrap();
        assert_eq!(saved.status(), StatusCode::OK);
        let list = router
            .clone()
            .oneshot(request(
                "GET",
                &format!("/checkouts/{checkout}/command-schedules"),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(list.status(), StatusCode::OK);
        let body = axum::body::to_bytes(list.into_body(), usize::MAX)
            .await
            .unwrap();
        let schedules: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(schedules[0]["schedule"]["command_id"], "daily-summary");
        assert_eq!(schedules[0]["next_runs"].as_array().unwrap().len(), 3);
        let deleted = router
            .clone()
            .oneshot(request("DELETE", &path, None))
            .await
            .unwrap();
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
        let list = router
            .oneshot(request(
                "GET",
                &format!("/checkouts/{checkout}/command-schedules"),
                None,
            ))
            .await
            .unwrap();
        let body = axum::body::to_bytes(list.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            json!([])
        );
    }
}
