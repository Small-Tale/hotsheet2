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
use jiff::{Timestamp, tz::TimeZone};
use serde::{Deserialize, Serialize};
use time::{Date, Duration, PrimitiveDateTime};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScheduleInput {
    time_zone: String,
    local_time: String,
    /// Absent or null inherits the command's action (HS2-17Y2EC).
    #[serde(default)]
    action: Option<ScheduleAction>,
    #[serde(default)]
    paused: bool,
    recurrence: Recurrence,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScheduleView {
    schedule: CommandSchedule,
    /// The action the next occurrence performs: the override, else the command's action,
    /// else ticket-only. Absent while the command is unavailable (HS2-17Y2EC).
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_action: Option<ScheduleAction>,
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
        milestone: None,
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

fn view(
    record: ScheduleRecord,
    now: Timestamp,
    command: Option<&CommandDefinition>,
) -> Result<ScheduleView, ApiError> {
    let available = command.is_some();
    let next_runs = if available {
        next_runs(&record.schedule, now)?
    } else {
        Vec::new()
    };
    Ok(ScheduleView {
        effective_action: command.map(|command| record.schedule.effective_action(command.action)),
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
    if schedule.recurrence.kind.is_sub_day() {
        return sub_day_next_runs(schedule, now);
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
    command_schedules::zoned::local_date(zone, now).map_err(invalid)
}

fn date_before_today(schedule: &CommandSchedule, now: Timestamp) -> Result<String, ApiError> {
    let zone = TimeZone::get(&schedule.time_zone).map_err(invalid)?;
    local_date(&zone, now)?
        .previous_day()
        .map(|day| day.to_string())
        .ok_or_else(|| invalid("local date is outside supported range"))
}

/// HS2-KXKA7X: the schedule's recurrence as the shared host engine reads it.
fn zoned(schedule: &CommandSchedule) -> command_schedules::zoned::Zoned<'_> {
    command_schedules::zoned::Zoned {
        time_zone: &schedule.time_zone,
        local_time: &schedule.local_time,
        recurrence: &schedule.recurrence,
    }
}

/// Resolve a local date and time. A gap advances to the first valid minute; a fold
/// chooses its first occurrence. The occurrence's calendar date remains its identity.
pub(crate) fn occurrence_instant(
    schedule: &CommandSchedule,
    date: Date,
) -> Result<Timestamp, ApiError> {
    zoned(schedule).date_instant(date).map_err(invalid)
}

fn local_now(zone: &TimeZone, now: Timestamp) -> Result<PrimitiveDateTime, ApiError> {
    command_schedules::zoned::local_now(zone, now).map_err(invalid)
}

/// Resolve sub-day slots to instants (HS2-29V4ZG), through the shared host engine.
fn sub_day_instants(
    schedule: &CommandSchedule,
    zone: &TimeZone,
    after: PrimitiveDateTime,
    through: PrimitiveDateTime,
    limit: usize,
) -> Result<Vec<(PrimitiveDateTime, Timestamp)>, ApiError> {
    zoned(schedule)
        .sub_day_instants(zone, after, through, limit)
        .map_err(invalid)
}

const SUB_DAY_MARGIN: Duration = command_schedules::zoned::SUB_DAY_MARGIN;

fn sub_day_next_runs(schedule: &CommandSchedule, now: Timestamp) -> Result<Vec<String>, ApiError> {
    Ok(next_sub_day_instants(schedule, now, 3)?
        .into_iter()
        .map(|(_, instant)| instant.to_string())
        .collect())
}

fn next_sub_day_instants(
    schedule: &CommandSchedule,
    now: Timestamp,
    count: usize,
) -> Result<Vec<(PrimitiveDateTime, Timestamp)>, ApiError> {
    zoned(schedule)
        .next_sub_day_instants(now, count)
        .map_err(invalid)
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<ScheduleView>>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let definitions = commands::from_settings(&settings).map_err(invalid)?;
    let mut result = Vec::new();
    for record in records(&settings)? {
        let command = definitions.iter().find(|command| {
            command.id == record.schedule.command_id && command.kind == CommandKind::Ai
        });
        result.push(view(record, Timestamp::now(), command)?);
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
    let Some(command) = definitions
        .iter()
        .find(|command| command.id == command_id && command.kind == CommandKind::Ai)
    else {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "schedules require an existing AI command",
        ));
    };
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
    // Sub-day schedules start after the current local minute; they never backfill the
    // slots that already passed today (HS2-29V4ZG).
    let initial_time = if schedule.recurrence.kind.is_sub_day() {
        let local = local_now(&zone, now)?;
        Some((
            local.date().to_string(),
            command_schedules::format_local_time(local.time()),
        ))
    } else {
        None
    };
    let initial_date = initial_time
        .as_ref()
        .map_or(initial_cursor, |(date, _)| date.clone());
    let initial_time = initial_time.map(|(_, time)| time);
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
            existing.last_processed_date = Some(initial_date.clone());
            existing.last_processed_time = initial_time.clone();
        }
        existing.schedule = schedule;
        existing.clone()
    } else {
        let record = ScheduleRecord {
            schedule,
            last_processed_date: Some(initial_date),
            last_processed_time: initial_time,
            history: Vec::new(),
        };
        next.push(record.clone());
        record
    };
    write_records(&settings, &prior, &next)?;
    state.command_schedule_kick.notify_one();
    emit_changed(&state, &checkout.id, &command_id);
    Ok(Json(view(record, Timestamp::now(), Some(command))?))
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

/// How a manual or headless AI command run ended (HS2-17Y2EC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AiRunDelivery {
    /// Create Ticket Only: the ticket is queued for any worker.
    Queued,
    /// The selected session claimed the ticket and accepted its turn.
    Started,
    /// The ticket exists, but no session could take it; `reason` says why.
    Failed,
}

#[derive(Debug, Serialize)]
pub(crate) struct AiRunView {
    action: ScheduleAction,
    delivery: AiRunDelivery,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    ticket: ApiTicket,
    run: crate::commands::CommandRun,
}

/// Run an AI command's configured action (HS2-17Y2EC): create its command ticket, then for a
/// session action claim that exact ticket under the selected session worker and hand it off
/// through the same path a scheduled occurrence uses. A command without an action runs as
/// **Create Ticket Only** here; the client's legacy click adds its idle-session signal itself.
pub(crate) async fn run_ai(
    State(state): State<AppState>,
    Path((reference, command_id)): Path<(String, String)>,
) -> Result<Json<AiRunView>, ApiError> {
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    let definitions = commands::from_settings(&settings).map_err(invalid)?;
    let Some(command) = definitions
        .iter()
        .find(|command| command.id == command_id && command.kind == CommandKind::Ai)
        .cloned()
    else {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "only an existing AI command can run an AI action",
        ));
    };
    let action = command_schedules::resolve_action(None, command.action);
    let ticket = {
        let state = state.clone();
        let reference = reference.clone();
        let command = command.clone();
        tokio::task::spawn_blocking(move || {
            let source = checkout_source_for_create(&state, &reference, None)?;
            let ticket = create_occurrence_ticket(&state, &reference, &source, &command, None)?;
            // Each run also tidies earlier completed tickets no change event reached (HS2-27C161).
            crate::command_cleanup::sweep(&state, &source.connection_id, &command);
            Ok::<_, ApiError>(ticket)
        })
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??
    };
    let (delivery, reason, tool) = if action == ScheduleAction::CreateTicket {
        (AiRunDelivery::Queued, None, None)
    } else {
        match deliver_one(
            &state,
            DeliveryTarget {
                reference: &reference,
                project: &checkout.root,
                settings: &settings,
                command: &command,
                action,
                ticket_id: &ticket.qualified_id,
                ticket_slug: &ticket.slug,
                occurrence_date: None,
            },
        )
        .await
        {
            DeliveryAttempt::Accepted { completed, worker } => {
                watch_manual_turn(&state, &reference, &ticket.qualified_id, completed, worker);
                let tool = permissions::effective_ai_settings(&state, Some(&settings))
                    .await
                    .ok()
                    .map(|defaults| selected_ai(&command, &defaults).0);
                (AiRunDelivery::Started, None, tool)
            }
            DeliveryAttempt::AwaitClaim => {
                note_manual_failure(&state, &reference, &ticket.qualified_id, "ticket_claimed");
                (AiRunDelivery::Failed, Some("ticket_claimed"), None)
            }
            DeliveryAttempt::Failed(reason) => {
                note_manual_failure(&state, &reference, &ticket.qualified_id, reason);
                (AiRunDelivery::Failed, Some(reason), None)
            }
        }
    };
    state.commands.replace_project(
        checkout.id.clone(),
        checkout.root.clone().into(),
        definitions,
    );
    let run = state
        .commands
        .record_launch_for(
            &checkout.id,
            &command_id,
            crate::commands::CommandLaunch {
                kind: crate::commands::LaunchKind::Ai,
                ticket_id: Some(ticket.id.clone()),
                ticket_slug: Some(ticket.slug.clone()),
                tool,
                terminal_id: None,
                terminal_label: None,
            },
        )
        .map_err(invalid)?;
    Ok(Json(AiRunView {
        action,
        delivery,
        reason,
        ticket,
        run,
    }))
}

/// A manual run's session turn that later fails releases its claim, so the ticket returns to
/// the queue instead of staying held by a session that stopped (HS2-17Y2EC).
fn watch_manual_turn(
    state: &AppState,
    reference: &str,
    ticket_id: &str,
    completed: tokio::sync::oneshot::Receiver<bool>,
    worker: String,
) {
    let state = state.clone();
    let reference = reference.to_owned();
    let ticket_id = ticket_id.to_owned();
    tokio::spawn(async move {
        if completed.await == Ok(true) {
            return;
        }
        note_manual_failure(&state, &reference, &ticket_id, "ai_turn_failed");
        if let Ok((entry, native_id)) = claims::checkout_claim_entry(&state, &reference, &ticket_id)
        {
            if let Ok(Some(ticket)) = entry.resolve(&native_id) {
                if let Ok(ticket) =
                    ops::release(&entry.store, &ticket.id, crate::now(), &worker, false)
                {
                    state.changed_in(&entry, "released", &ticket);
                }
            }
        }
    });
}

fn note_manual_failure(state: &AppState, reference: &str, ticket_id: &str, reason: &str) {
    let Ok((entry, native_id)) = claims::checkout_claim_entry(state, reference, ticket_id) else {
        return;
    };
    let Ok(Some(ticket)) = entry.resolve(&native_id) else {
        return;
    };
    if let Ok(ticket) = ops::add_note_with_metadata(
        &entry.store,
        &ticket.id,
        hotsheet_model::Ulid::new(),
        crate::now(),
        hotsheet_model::NoteKind::Regular,
        ops::NoteMetadataInput {
            summary: Some("AI command session did not start".into()),
            actor: Some(hotsheet_model::NoteActor {
                role: hotsheet_model::AttachmentActorRole::System,
                id: Some("hotsheet-command-runner".into()),
            }),
            ..ops::NoteMetadataInput::default()
        },
        format!(
            "The AI command's session could not take this ticket ({reason}). The ticket stays queued for any worker."
        ),
    ) {
        state.changed_in(&entry, "note_added", &ticket);
    }
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

/// A command ticket carries the command provenance tag; a scheduled one also carries its
/// occurrence identity, and a manual or headless run (HS2-17Y2EC) has none.
fn create_occurrence_ticket(
    state: &AppState,
    reference: &str,
    source: &hotsheet_ticketing::checkouts::TicketSource,
    command: &CommandDefinition,
    occurrence_tag: Option<String>,
) -> Result<ApiTicket, ApiError> {
    let req = CreateReq {
        title: command.title.trim().into(),
        category: Some("task".into()),
        priority: Some("highest".into()),
        status: Some("not_started".into()),
        details: command.prompt.as_deref().map(str::trim).map(str::to_owned),
        tags: Some(
            std::iter::once(command_schedules::command_tag(&command.id))
                .chain(occurrence_tag)
                .collect(),
        ),
        up_next: Some(true),
        blocked_by: None,
        follow_up_to: None,
        group: None,
        group_resolved: false,
        actor: None,
        due_on: None,
        milestone: None,
        milestone_resolved: false,
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
        // HS2-K9H38S: a provider without Up Next refuses it, so queue the command ticket only
        // where the provider can hold that state.
        let up_next = provider_for(state, &source.connection_id)?
            .descriptor()
            .capabilities
            .up_next;
        do_provider_create(
            state,
            &source.connection_id,
            CreateReq {
                up_next: Some(up_next),
                ..req
            },
        )
    }
}

/// Reuse the occurrence's ticket, skip when the command already has an open ticket, or
/// create the occurrence ticket. Shared by date and sub-day recurrences (HS2-29V4ZG).
fn occurrence_ticket(
    state: &AppState,
    reference: &str,
    schedule: &CommandSchedule,
    command: &CommandDefinition,
    occurrence_tag: String,
) -> Result<(ApiTicket, command_schedules::OccurrenceOutcome), ApiError> {
    let source = checkout_source_for_create(state, reference, None)?;
    // Each occurrence also tidies earlier completed tickets no change event reached (HS2-27C161).
    crate::command_cleanup::sweep(state, &source.connection_id, command);
    // HS2-17Y2EC: the schedule's override, else the command's action, else ticket-only.
    let queued = if schedule.effective_action(command.action) == ScheduleAction::CreateTicket {
        command_schedules::OccurrenceOutcome::Created
    } else {
        command_schedules::OccurrenceOutcome::DeliveryPending
    };
    if let Some(ticket) = query_tag(state, &source.connection_id, occurrence_tag.clone(), false)? {
        return Ok((ticket, queued));
    }
    if let Some(ticket) = query_tag(
        state,
        &source.connection_id,
        command_schedules::command_tag(&schedule.command_id),
        true,
    )? {
        return Ok((
            ticket,
            command_schedules::OccurrenceOutcome::SkippedOpenTicket,
        ));
    }
    Ok((
        create_occurrence_ticket(state, reference, &source, command, Some(occurrence_tag))?,
        queued,
    ))
}

/// Evaluate one sub-day schedule (HS2-29V4ZG). Missed slots coalesce into the latest due
/// slot, so a host that was offline runs once on return instead of replaying a backlog.
/// The overlap policy is the date kinds' `skipped_open_ticket`; consecutive skips against
/// the same open ticket record one history entry so the bounded history stays useful.
fn process_sub_day(
    state: &AppState,
    reference: &str,
    checkout_id: &str,
    settings: &Settings,
    snapshot: &ScheduleRecord,
    command: &CommandDefinition,
    now: Timestamp,
) -> Result<Option<Timestamp>, ApiError> {
    let schedule = &snapshot.schedule;
    let zone = TimeZone::get(&schedule.time_zone).map_err(invalid)?;
    let local = local_now(&zone, now)?;
    let cursor = snapshot.sub_day_cursor();
    let due = match cursor {
        Some(cursor) => {
            let after = cursor.max(local.saturating_sub(Duration::days(2)));
            // Two days and the margin at the five-minute floor is under 650 slots.
            sub_day_instants(
                schedule,
                &zone,
                after,
                local.saturating_add(SUB_DAY_MARGIN),
                2048,
            )?
            .into_iter()
            .rfind(|(_, instant)| *instant <= now)
        }
        None => None,
    };
    if due.is_some() || cursor.is_none() {
        // A record written without a cursor (for example through `settings set`) starts
        // at the current local minute rather than backfilling earlier slots.
        let (slot, ticket) = match due {
            Some((slot, _)) => {
                let date = slot.date().to_string();
                let time = command_schedules::format_local_time(slot.time());
                let tag = command_schedules::occurrence_tag_at(&schedule.command_id, &date, &time);
                (
                    slot,
                    Some(occurrence_ticket(state, reference, schedule, command, tag)?),
                )
            }
            None => (local.replace_second(0).map_err(invalid)?, None),
        };
        let date = slot.date().to_string();
        let time = command_schedules::format_local_time(slot.time());
        // Re-read after the side effect; a concurrent edit must not be overwritten.
        let prior = records(settings)?;
        let mut next = prior.clone();
        if let Some(record) = next
            .iter_mut()
            .find(|record| record.schedule.command_id == schedule.command_id)
            && record.schedule == *schedule
        {
            record.last_processed_date = Some(date.clone());
            record.last_processed_time = Some(time.clone());
            if let Some((ticket, outcome)) = ticket {
                let key = command_schedules::occurrence_key(&date, Some(&time));
                let repeated_skip = outcome
                    == command_schedules::OccurrenceOutcome::SkippedOpenTicket
                    && record.history.last().is_some_and(|last| {
                        last.outcome == command_schedules::OccurrenceOutcome::SkippedOpenTicket
                            && last.ticket_id.as_deref() == Some(ticket.qualified_id.as_str())
                    });
                if !repeated_skip && !record.history.iter().any(|item| item.key() == key) {
                    record.history.push(OccurrenceResult {
                        occurrence_date: date,
                        occurrence_time: Some(time),
                        outcome,
                        ticket_id: Some(ticket.qualified_id),
                        ticket_slug: Some(ticket.slug),
                        turn_completed: (schedule.effective_action(command.action)
                            != ScheduleAction::CreateTicket)
                            .then_some(false),
                    });
                    if record.history.len() > command_schedules::HISTORY_LIMIT {
                        record.history.remove(0);
                    }
                }
            }
            write_records(settings, &prior, &next)?;
            emit_changed(state, checkout_id, &schedule.command_id);
        }
    }
    Ok(next_sub_day_instants(schedule, now, 1)?
        .first()
        .map(|(_, instant)| *instant))
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
        if schedule.recurrence.kind.is_sub_day() {
            if let Some(instant) = process_sub_day(
                state,
                reference,
                &checkout.id,
                &settings,
                &snapshot,
                command,
                now,
            )? {
                next_wake =
                    Some(next_wake.map_or(instant, |previous: Timestamp| previous.min(instant)));
            }
            continue;
        }
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
            let occurrence_tag =
                command_schedules::occurrence_tag(&schedule.command_id, &date.to_string());
            let (ticket, outcome) =
                occurrence_ticket(state, reference, schedule, command, occurrence_tag)?;
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
                    occurrence_time: None,
                    outcome,
                    ticket_id: Some(ticket.qualified_id),
                    ticket_slug: Some(ticket.slug),
                    turn_completed: (schedule.effective_action(command.action)
                        != ScheduleAction::CreateTicket)
                        .then_some(false),
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
    /// `false` for a manual or headless command run (HS2-17Y2EC), which names its own
    /// new session after the command rather than the scheduler.
    scheduled: bool,
}

fn delivery_connection(state: &AppState, target: DeliverySession<'_>) -> (String, Option<String>) {
    let own_id = format!(
        "hotsheet-{}-{}",
        if target.scheduled {
            "scheduled"
        } else {
            "command"
        },
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
    /// The occurrence key: a date, or `date T time` for sub-day kinds (HS2-29V4ZG). `None`
    /// is a manual or headless run of the command (HS2-17Y2EC), which has no occurrence
    /// ledger to update.
    occurrence_date: Option<&'a str>,
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
            scheduled: target.occurrence_date.is_some(),
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
        Some(
            if target.occurrence_date.is_some() {
                "Scheduled AI command"
            } else {
                "AI command"
            }
            .into(),
        ),
        None,
    );
    match claimed {
        Ok(ticket) => state.changed_in(&entry, "claimed", &ticket),
        Err(ops::OpError::WrongWorker { .. } | ops::OpError::ClaimUnavailable { .. }) => {
            return DeliveryAttempt::AwaitClaim;
        }
        Err(_) => return DeliveryAttempt::Failed("ticket_claim_failed"),
    }
    // HS2-NR0VRF: commit the ambiguity boundary before sending a turn. If the
    // host dies after this write, restart must not replay a possibly sent prompt.
    // A manual run has no ledger and is never replayed (HS2-17Y2EC).
    if target.occurrence_date.is_some_and(|occurrence_date| {
        !matches!(
            set_delivery_outcome(
                target.settings,
                &target.command.id,
                occurrence_date,
                target.ticket_id,
                command_schedules::OccurrenceOutcome::DeliveryDispatching,
                false,
            ),
            Ok(Some(_))
        )
    }) {
        if let Ok(ticket) = ops::release(&entry.store, &ticket.id, crate::now(), &worker, false) {
            state.changed_in(&entry, "released", &ticket);
        }
        return DeliveryAttempt::Failed("occurrence_changed");
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
                    item.key() == occurrence_date && item.ticket_id.as_deref() == Some(ticket_id)
                })
            })
        else {
            return Ok(None);
        };
        let allowed = match outcome {
            command_schedules::OccurrenceOutcome::DeliveryDispatching => matches!(
                item.outcome,
                command_schedules::OccurrenceOutcome::DeliveryPending
                    | command_schedules::OccurrenceOutcome::DeliveryFailed
            ),
            command_schedules::OccurrenceOutcome::Delivered => {
                item.outcome == command_schedules::OccurrenceOutcome::DeliveryDispatching
            }
            command_schedules::OccurrenceOutcome::DeliveryFailed => {
                matches!(
                    item.outcome,
                    command_schedules::OccurrenceOutcome::DeliveryPending
                        | command_schedules::OccurrenceOutcome::DeliveryFailed
                        | command_schedules::OccurrenceOutcome::DeliveryDispatching
                ) || (allow_delivered
                    && item.outcome == command_schedules::OccurrenceOutcome::Delivered)
            }
            _ => false,
        };
        if !allowed {
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

fn mark_turn_completed(
    settings: &Settings,
    command_id: &str,
    occurrence_date: &str,
    ticket_id: &str,
) -> Result<bool, ApiError> {
    for _ in 0..4 {
        let prior = records(settings)?;
        let mut next = prior.clone();
        let Some(item) = next
            .iter_mut()
            .find(|record| record.schedule.command_id == command_id)
            .and_then(|record| {
                record.history.iter_mut().find(|item| {
                    item.key() == occurrence_date && item.ticket_id.as_deref() == Some(ticket_id)
                })
            })
        else {
            return Ok(false);
        };
        if item.outcome != command_schedules::OccurrenceOutcome::Delivered
            || item.turn_completed == Some(true)
        {
            return Ok(false);
        }
        item.turn_completed = Some(true);
        if write_records(settings, &prior, &next).is_ok() {
            return Ok(true);
        }
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "command completion history changed",
    ))
}

/// The prior process cannot prove whether an accepted provider turn finished.
/// Keep the exact ticket visible for deliberate reconciliation instead of replaying it.
fn reconcile_interrupted_checkout(state: &AppState, reference: &str) -> Result<(), ApiError> {
    let (checkout, settings) = checkout_settings(state, reference)?;
    for _ in 0..4 {
        let prior = records(&settings)?;
        let mut next = prior.clone();
        let mut uncertain = Vec::new();
        let mut changed_commands = Vec::new();
        for record in &mut next {
            for item in &mut record.history {
                if item.outcome == command_schedules::OccurrenceOutcome::DeliveryDispatching
                    || (item.outcome == command_schedules::OccurrenceOutcome::Delivered
                        && item.turn_completed == Some(false))
                {
                    let completed = item.ticket_id.as_deref().is_some_and(|id| {
                        claims::checkout_claim_entry(state, reference, id)
                            .ok()
                            .and_then(|(entry, native_id)| entry.resolve(&native_id).ok().flatten())
                            .is_some_and(|ticket| {
                                matches!(
                                    ticket.status,
                                    hotsheet_model::Status::Completed
                                        | hotsheet_model::Status::Verified
                                        | hotsheet_model::Status::Archive
                                )
                            })
                    });
                    if completed {
                        item.outcome = command_schedules::OccurrenceOutcome::Delivered;
                        item.turn_completed = Some(true);
                    } else {
                        item.outcome = command_schedules::OccurrenceOutcome::DeliveryUncertain;
                    }
                    if !completed {
                        if let Some(id) = &item.ticket_id {
                            uncertain.push(id.clone());
                        }
                    }
                    changed_commands.push(record.schedule.command_id.clone());
                }
            }
        }
        if next == prior {
            return Ok(());
        }
        if write_records(&settings, &prior, &next).is_ok() {
            for command_id in changed_commands {
                emit_changed(state, &checkout.id, &command_id);
            }
            for ticket_id in uncertain {
                note_delivery_uncertain(state, reference, &ticket_id);
            }
            return Ok(());
        }
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "command recovery history changed",
    ))
}

fn note_delivery_uncertain(state: &AppState, reference: &str, ticket_id: &str) {
    let Ok((entry, native_id)) = claims::checkout_claim_entry(state, reference, ticket_id) else {
        return;
    };
    let Ok(Some(ticket)) = entry.resolve(&native_id) else {
        return;
    };
    if let Ok(ticket) = ops::add_note_with_metadata(
        &entry.store,
        &ticket.id,
        hotsheet_model::Ulid::new(),
        crate::now(),
        hotsheet_model::NoteKind::Regular,
        ops::NoteMetadataInput {
            summary: Some("Scheduled AI turn needs reconciliation".into()),
            actor: Some(hotsheet_model::NoteActor {
                role: hotsheet_model::AttachmentActorRole::System,
                id: Some("hotsheet-command-scheduler".into()),
            }),
            ..ops::NoteMetadataInput::default()
        },
        "The host restarted while this scheduled AI turn may have been running. Inspect this exact ticket and the provider session before resuming; the scheduler will not send the prompt again automatically.".into(),
    ) {
        state.changed_in(&entry, "note_added", &ticket);
    }
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
        let command = definitions.iter().find(|command| {
            command.id == record.schedule.command_id && command.kind == CommandKind::Ai
        });
        // Resolve at delivery time (HS2-17Y2EC): an inheriting schedule follows its command's
        // current action, and an occurrence whose command is gone waits until it returns.
        let action = record
            .schedule
            .effective_action(command.and_then(|command| command.action));
        if action == ScheduleAction::CreateTicket {
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
            let result = if let Some(command) = command {
                deliver_one(
                    state,
                    DeliveryTarget {
                        reference,
                        project: &checkout.root,
                        settings: &settings,
                        command,
                        action,
                        ticket_id,
                        ticket_slug,
                        occurrence_date: Some(&item.key()),
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
                &item.key(),
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
                let occurrence_date = item.key();
                let ticket_id = ticket_id.to_owned();
                tokio::spawn(async move {
                    if completed.await == Ok(true) {
                        if let Ok((checkout, settings)) = checkout_settings(&state, &reference) {
                            if let Ok(true) = mark_turn_completed(
                                &settings,
                                &command_id,
                                &occurrence_date,
                                &ticket_id,
                            ) {
                                emit_changed(&state, &checkout.id, &command_id);
                            }
                        }
                    } else {
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
            let mut startup = true;
            loop {
                if state.is_stopping() {
                    return;
                }
                let work = state.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut next = None;
                    let mut checkouts = Vec::new();
                    let mut recovery_failed = false;
                    for checkout in work.checkout_registry.list().map_err(invalid)? {
                        if startup {
                            if let Err(error) = reconcile_interrupted_checkout(&work, &checkout.id) {
                                tracing::warn!(checkout = %checkout.id, error = %error.message, "scheduled AI recovery failed");
                                recovery_failed = true;
                            }
                        }
                        match process_checkout(&work, &checkout.id, Timestamp::now()) {
                            Ok(Some(instant)) => next = Some(next.map_or(instant, |old: Timestamp| old.min(instant))),
                            Ok(None) => {},
                            Err(error) => tracing::warn!(checkout = %checkout.id, error = %error.message, "command schedule evaluation failed"),
                        }
                        checkouts.push(checkout.id);
                    }
                    Ok::<_, ApiError>((next, checkouts, recovery_failed))
                }).await;
                let (next, checkouts, recovery_failed) = match result {
                    Ok(Ok(result)) => result,
                    Ok(Err(error)) => {
                        tracing::warn!(error = %error.message, "command schedule scan failed");
                        (None, Vec::new(), true)
                    }
                    Err(error) => {
                        tracing::warn!(%error, "command schedule worker failed");
                        (None, Vec::new(), true)
                    }
                };
                startup = recovery_failed;
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
    use time::Time;
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
            .with_ai_tool_discovery(std::time::Duration::from_secs(3600), |_, _, _| {
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
                action: Some(action),
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
            last_processed_time: None,
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
                scheduled: true,
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
    async fn hs2_nr0vrf_restart_preserves_uncertain_exact_turn_without_replay() {
        for accepted in [false, true] {
            let (root, state, checkout, settings, store) = fixture();
            let backend = ScheduledFakeDrive::default();
            let state = with_scheduled_backend(state, backend.clone());
            let (ticket_id, _) =
                due_ticket(&state, &checkout, &settings, ScheduleAction::NewAiSession);
            let date = records(&settings).unwrap()[0].history[0]
                .occurrence_date
                .clone();
            let outcome = if accepted {
                command_schedules::OccurrenceOutcome::Delivered
            } else {
                command_schedules::OccurrenceOutcome::DeliveryDispatching
            };
            let mut stored = records(&settings).unwrap();
            stored[0].history[0].outcome = outcome;
            settings
                .set(command_schedules::SETTING, json!(stored), Scope::Local)
                .unwrap();

            let recovered = AppState::new(store.clone(), "synthetic-secret".into())
                .unwrap()
                .with_checkout_registry(root.path().join("checkouts.json"));
            let recovered = with_scheduled_backend(recovered, backend.clone());
            reconcile_interrupted_checkout(&recovered, &checkout).unwrap();
            assert_eq!(
                records(&settings).unwrap()[0].history[0].outcome,
                command_schedules::OccurrenceOutcome::DeliveryUncertain,
            );
            deliver_pending(&recovered, &checkout).await.unwrap();
            assert!(backend.preparations.lock().unwrap().is_empty());
            assert!(backend.turns.lock().unwrap().is_empty());
            assert_eq!(
                records(&settings).unwrap()[0].history[0]
                    .ticket_id
                    .as_deref(),
                Some(ticket_id.as_str())
            );
            assert_eq!(
                records(&settings).unwrap()[0].history[0].occurrence_date,
                date
            );
            assert_eq!(
                ops::query(&store, &TicketQuery::default()).unwrap().len(),
                1
            );
            let notes = &ops::query(&store, &TicketQuery::default()).unwrap()[0].notes;
            assert!(
                notes
                    .iter()
                    .any(|note| note.text.contains("will not send the prompt again"))
            );
            // A second restart is idempotent, including its note history.
            let note_count = notes.len();
            reconcile_interrupted_checkout(&recovered, &checkout).unwrap();
            assert_eq!(
                ops::query(&store, &TicketQuery::default()).unwrap()[0]
                    .notes
                    .len(),
                note_count
            );
        }
    }

    #[tokio::test]
    async fn hs2_nr0vrf_confirmed_turn_stays_delivered_on_restart() {
        let (_root, state, checkout, settings, _store) = fixture();
        let (ticket_id, _) = due_ticket(&state, &checkout, &settings, ScheduleAction::NewAiSession);
        let mut stored = records(&settings).unwrap();
        stored[0].history[0].outcome = command_schedules::OccurrenceOutcome::Delivered;
        stored[0].history[0].turn_completed = Some(true);
        settings
            .set(command_schedules::SETTING, json!(stored), Scope::Local)
            .unwrap();
        reconcile_interrupted_checkout(&state, &checkout).unwrap();
        let item = &records(&settings).unwrap()[0].history[0];
        assert_eq!(
            item.outcome,
            command_schedules::OccurrenceOutcome::Delivered
        );
        assert_eq!(item.turn_completed, Some(true));
        assert_eq!(item.ticket_id.as_deref(), Some(ticket_id.as_str()));
        // Older accepted records lacked completion evidence. Preserve their
        // historical outcome rather than marking every prior run uncertain.
        let mut legacy = records(&settings).unwrap();
        legacy[0].history[0].turn_completed = None;
        settings
            .set(command_schedules::SETTING, json!(legacy), Scope::Local)
            .unwrap();
        reconcile_interrupted_checkout(&state, &checkout).unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::Delivered
        );
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

    fn ts(raw: &str) -> Timestamp {
        raw.parse().unwrap()
    }

    fn hourly_record(cursor_date: &str, cursor_time: &str) -> ScheduleRecord {
        let mut record = sample_record(cursor_date, ScheduleAction::CreateTicket);
        record.schedule.recurrence.kind = command_schedules::RecurrenceKind::Hourly;
        record.last_processed_time = Some(cursor_time.into());
        record
    }

    fn sub_day_runs(schedule: &CommandSchedule, after: &str, through: &str) -> Vec<String> {
        let zone = TimeZone::get(&schedule.time_zone).unwrap();
        let parse = |raw: &str| {
            let (date, time) = raw.split_once('T').unwrap();
            let (hour, minute) = command_schedules::parse_local_time(time).unwrap();
            command_schedules::parse_date(date)
                .unwrap()
                .with_time(Time::from_hms(hour, minute, 0).unwrap())
        };
        sub_day_instants(schedule, &zone, parse(after), parse(through), 100)
            .unwrap()
            .into_iter()
            .map(|(slot, instant)| {
                format!(
                    "{}T{} {instant}",
                    slot.date(),
                    command_schedules::format_local_time(slot.time())
                )
            })
            .collect()
    }

    #[test]
    fn hs2_29v4zg_dst_gap_and_fold_never_double_run_sub_day_slots() {
        let mut schedule = hourly_record("2026-03-07", "00:00").schedule;
        schedule.local_time = "00:00".into();
        schedule.recurrence.start_date = Some("2026-03-08".into());
        // Spring forward in New York: 02:00 does not exist and the 03:00 slot covers it.
        assert_eq!(
            sub_day_runs(&schedule, "2026-03-08T00:30", "2026-03-08T04:00"),
            [
                "2026-03-08T01:00 2026-03-08T06:00:00Z",
                "2026-03-08T03:00 2026-03-08T07:00:00Z",
                "2026-03-08T04:00 2026-03-08T08:00:00Z"
            ]
        );
        // Every 40 minutes: 02:00 and 02:40 both fall in the gap; only the later one runs,
        // at the first valid minute, and 03:20 keeps its own instant.
        schedule.recurrence.kind = command_schedules::RecurrenceKind::Minutely;
        schedule.recurrence.every = Some(40);
        assert_eq!(
            sub_day_runs(&schedule, "2026-03-08T01:00", "2026-03-08T03:20"),
            [
                "2026-03-08T01:20 2026-03-08T06:20:00Z",
                "2026-03-08T02:40 2026-03-08T07:00:00Z",
                "2026-03-08T03:20 2026-03-08T07:20:00Z"
            ]
        );
        // Fall back: the repeated 01:00 hour runs once, at its first (EDT) instant.
        schedule.recurrence.kind = command_schedules::RecurrenceKind::Hourly;
        schedule.recurrence.every = Some(1);
        schedule.recurrence.start_date = Some("2026-11-01".into());
        assert_eq!(
            sub_day_runs(&schedule, "2026-10-31T23:30", "2026-11-01T02:00"),
            [
                "2026-11-01T00:00 2026-11-01T04:00:00Z",
                "2026-11-01T01:00 2026-11-01T05:00:00Z",
                "2026-11-01T02:00 2026-11-01T07:00:00Z"
            ]
        );
    }

    #[test]
    fn hs2_29v4zg_hourly_occurrence_identity_wake_and_restart() {
        let (_root, state, checkout, settings, store) = fixture();
        settings
            .set(
                command_schedules::SETTING,
                json!([hourly_record("2026-03-08", "09:00")]),
                Scope::Local,
            )
            .unwrap();
        // 10:30 EDT: the 10:00 slot is due, and the scheduler wakes for 11:00 EDT.
        let now = ts("2026-03-08T14:30:00Z");
        let wake = process_checkout(&state, &checkout, now).unwrap();
        assert_eq!(wake, Some(ts("2026-03-08T15:00:00Z")));
        let tickets = ops::query(&store, &TicketQuery::default()).unwrap();
        assert_eq!(tickets.len(), 1);
        assert!(
            tickets[0]
                .tags
                .contains(&"hs2-occurrence:daily-summary:2026-03-08t1000".to_owned())
        );
        let record = records(&settings).unwrap().remove(0);
        assert_eq!(record.last_processed_date.as_deref(), Some("2026-03-08"));
        assert_eq!(record.last_processed_time.as_deref(), Some("10:00"));
        assert_eq!(record.history.len(), 1);
        assert_eq!(record.history[0].key(), "2026-03-08T10:00");
        // A second pass in the same slot is a no-op.
        assert_eq!(
            process_checkout(&state, &checkout, now).unwrap(),
            Some(ts("2026-03-08T15:00:00Z"))
        );
        assert_eq!(records(&settings).unwrap()[0].history.len(), 1);
        // A crash after the ticket write but before the cursor write reuses the ticket.
        let mut stale = records(&settings).unwrap();
        stale[0].last_processed_time = Some("09:00".into());
        stale[0].history.clear();
        settings
            .set(command_schedules::SETTING, json!(stale), Scope::Local)
            .unwrap();
        process_checkout(&state, &checkout, now).unwrap();
        assert_eq!(
            ops::query(&store, &TicketQuery::default()).unwrap().len(),
            1
        );
        let record = records(&settings).unwrap().remove(0);
        assert_eq!(record.history.len(), 1);
        assert_eq!(
            record.history[0].outcome,
            command_schedules::OccurrenceOutcome::Created
        );
        assert_eq!(
            next_runs(&record.schedule, now).unwrap(),
            [
                "2026-03-08T15:00:00Z",
                "2026-03-08T16:00:00Z",
                "2026-03-08T17:00:00Z"
            ]
        );
    }

    #[test]
    fn hs2_29v4zg_open_ticket_skips_once_then_next_slot_runs_after_completion() {
        let (_root, state, checkout, settings, store) = fixture();
        settings
            .set(
                command_schedules::SETTING,
                json!([hourly_record("2026-03-08", "09:00")]),
                Scope::Local,
            )
            .unwrap();
        process_checkout(&state, &checkout, ts("2026-03-08T14:05:00Z")).unwrap();
        // The 10:00 ticket is still open at 11:00 and 12:00: one skip entry, no ticket.
        process_checkout(&state, &checkout, ts("2026-03-08T15:05:00Z")).unwrap();
        process_checkout(&state, &checkout, ts("2026-03-08T16:05:00Z")).unwrap();
        let tickets = ops::query(&store, &TicketQuery::default()).unwrap();
        assert_eq!(tickets.len(), 1);
        let record = records(&settings).unwrap().remove(0);
        assert_eq!(record.last_processed_time.as_deref(), Some("12:00"));
        let outcomes: Vec<_> = record
            .history
            .iter()
            .map(|item| (item.key(), item.outcome))
            .collect();
        assert_eq!(
            outcomes,
            [
                (
                    "2026-03-08T10:00".to_owned(),
                    command_schedules::OccurrenceOutcome::Created
                ),
                (
                    "2026-03-08T11:00".to_owned(),
                    command_schedules::OccurrenceOutcome::SkippedOpenTicket
                ),
            ]
        );
        assert_eq!(record.history[1].ticket_slug, record.history[0].ticket_slug);
        ops::update(
            &store,
            &tickets[0].id,
            crate::now(),
            ops::TicketPatch {
                status: Some(hotsheet_model::Status::Completed),
                ..Default::default()
            },
        )
        .unwrap();
        process_checkout(&state, &checkout, ts("2026-03-08T17:05:00Z")).unwrap();
        assert_eq!(
            ops::query(&store, &TicketQuery::default()).unwrap().len(),
            2
        );
        let record = records(&settings).unwrap().remove(0);
        assert_eq!(
            record.history.last().map(|item| (item.key(), item.outcome)),
            Some((
                "2026-03-08T13:00".to_owned(),
                command_schedules::OccurrenceOutcome::Created
            ))
        );
    }

    #[test]
    fn hs2_29v4zg_missed_slots_coalesce_and_fold_does_not_replay() {
        let (_root, state, checkout, settings, store) = fixture();
        settings
            .set(
                command_schedules::SETTING,
                json!([hourly_record("2026-03-08", "10:00")]),
                Scope::Local,
            )
            .unwrap();
        // Back at 16:20 EDT after six missed slots: only 16:00 runs.
        process_checkout(&state, &checkout, ts("2026-03-08T20:20:00Z")).unwrap();
        let tickets = ops::query(&store, &TicketQuery::default()).unwrap();
        assert_eq!(tickets.len(), 1);
        assert!(
            tickets[0]
                .tags
                .contains(&"hs2-occurrence:daily-summary:2026-03-08t1600".to_owned())
        );
        assert_eq!(records(&settings).unwrap()[0].history.len(), 1);

        let (_root, state, checkout, settings, store) = fixture();
        settings
            .set(
                command_schedules::SETTING,
                json!([hourly_record("2026-11-01", "01:00")]),
                Scope::Local,
            )
            .unwrap();
        // 01:30 EST, the second pass through the 01:00 hour: nothing is due again.
        let wake = process_checkout(&state, &checkout, ts("2026-11-01T06:30:00Z")).unwrap();
        assert!(
            ops::query(&store, &TicketQuery::default())
                .unwrap()
                .is_empty()
        );
        assert_eq!(wake, Some(ts("2026-11-01T07:00:00Z")));
    }

    #[test]
    fn hs2_29v4zg_record_without_cursor_starts_now_instead_of_backfilling() {
        let (_root, state, checkout, settings, store) = fixture();
        let mut record = hourly_record("2026-03-08", "09:00");
        record.last_processed_date = None;
        record.last_processed_time = None;
        settings
            .set(command_schedules::SETTING, json!([record]), Scope::Local)
            .unwrap();
        let wake = process_checkout(&state, &checkout, ts("2026-03-08T14:30:45Z")).unwrap();
        assert_eq!(wake, Some(ts("2026-03-08T15:00:00Z")));
        assert!(
            ops::query(&store, &TicketQuery::default())
                .unwrap()
                .is_empty()
        );
        let record = records(&settings).unwrap().remove(0);
        assert_eq!(record.last_processed_date.as_deref(), Some("2026-03-08"));
        assert_eq!(record.last_processed_time.as_deref(), Some("10:30"));
    }

    #[tokio::test]
    async fn hs2_29v4zg_schedule_api_accepts_hourly_and_enforces_minimum_interval() {
        let (_root, state, checkout, settings, _store) = fixture();
        let router = crate::app(state);
        let request = |method: &str, uri: String, body: serde_json::Value| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("x-hotsheet-secret", "synthetic-secret")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        let input = |kind: &str, every: u16| {
            json!({
                "time_zone": "UTC", "local_time": "00:00", "action": "create_ticket",
                "recurrence": {"kind": kind, "start_date": "2026-01-01", "every": every}
            })
        };
        let preview = router
            .clone()
            .oneshot(request(
                "POST",
                format!("/checkouts/{checkout}/command-schedules/preview"),
                input("hourly", 2),
            ))
            .await
            .unwrap();
        assert_eq!(preview.status(), StatusCode::OK);
        let body = axum::body::to_bytes(preview.into_body(), usize::MAX)
            .await
            .unwrap();
        let runs: Vec<Timestamp> =
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["next_runs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|run| run.as_str().unwrap().parse().unwrap())
                .collect();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1].as_second() - runs[0].as_second(), 7200);
        assert_eq!(runs[0].as_second() % 7200, 0);
        let refused = router
            .clone()
            .oneshot(request(
                "PUT",
                format!("/checkouts/{checkout}/command-schedules/daily-summary"),
                input("minutely", 4),
            ))
            .await
            .unwrap();
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        let saved = router
            .oneshot(request(
                "PUT",
                format!("/checkouts/{checkout}/command-schedules/daily-summary"),
                input("minutely", 5),
            ))
            .await
            .unwrap();
        assert_eq!(saved.status(), StatusCode::OK);
        let record = records(&settings).unwrap().remove(0);
        assert_eq!(
            record.schedule.recurrence.kind,
            command_schedules::RecurrenceKind::Minutely
        );
        // The cursor starts at the current minute, so passed slots are not backfilled.
        assert!(record.sub_day_cursor().is_some());
        // The headless settings path validates the same rule.
        let mut invalid_record = record.clone();
        invalid_record.schedule.recurrence.every = Some(1);
        assert!(
            settings
                .set(
                    command_schedules::SETTING,
                    json!([invalid_record]),
                    Scope::Local
                )
                .is_err()
        );
    }

    #[tokio::test]
    async fn hs2_29v4zg_sub_day_delivery_updates_its_exact_occurrence() {
        let (_root, state, checkout, settings, _store) = fixture();
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        let mut record = hourly_record("2026-03-08", "09:00");
        record.schedule.action = Some(ScheduleAction::NewAiSession);
        settings
            .set(command_schedules::SETTING, json!([record]), Scope::Local)
            .unwrap();
        process_checkout(&state, &checkout, ts("2026-03-08T14:05:00Z")).unwrap();
        // The open ticket skips 11:00; the skip shares the ticket id but not the key.
        process_checkout(&state, &checkout, ts("2026-03-08T15:05:00Z")).unwrap();
        deliver_pending(&state, &checkout).await.unwrap();
        let history = records(&settings).unwrap()[0].history.clone();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].key(), "2026-03-08T10:00");
        assert_eq!(
            history[0].outcome,
            command_schedules::OccurrenceOutcome::Delivered
        );
        assert_eq!(
            history[1].outcome,
            command_schedules::OccurrenceOutcome::SkippedOpenTicket
        );
        assert_eq!(backend.preparations.lock().unwrap().len(), 1);
    }

    async fn send_json(
        router: &axum::Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("x-hotsheet-secret", "synthetic-secret")
                    .header("content-type", "application/json")
                    .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    fn set_command_action(settings: &Settings, action: Option<&str>) {
        let mut command = json!({"id":"daily-summary","title":"Daily summary","kind":"ai","prompt":"Summarize synthetic work"});
        if let Some(action) = action {
            command["action"] = action.into();
        }
        settings
            .set("commands", json!([command]), Scope::Local)
            .unwrap();
    }

    async fn wait_for_turns(backend: &ScheduledFakeDrive, count: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while backend.turns.lock().unwrap().len() < count {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fake drive turn");
    }

    /// HS2-17Y2EC: a command whose action starts a new session runs headlessly over HTTP: the
    /// exact ticket is created, claimed by the new session's worker before the handoff, and
    /// recorded as the command's launch.
    #[tokio::test]
    async fn hs2_17y2ec_ai_run_new_session_claims_ticket_and_records_launch() {
        let (_root, state, checkout, settings, store) = fixture();
        set_command_action(&settings, Some("new_ai_session"));
        let backend = ScheduledFakeDrive::default();
        let router = crate::app(with_scheduled_backend(state, backend.clone()));
        let (status, body) = send_json(
            &router,
            "POST",
            &format!("/checkouts/{checkout}/commands/daily-summary/ai-run"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["action"], "new_ai_session");
        assert_eq!(body["delivery"], "started");
        assert!(body.get("reason").is_none());
        let ticket = ops::query(&store, &TicketQuery::default())
            .unwrap()
            .remove(0);
        assert_eq!(body["ticket"]["slug"], ticket.slug.as_str());
        assert!(
            ticket
                .tags
                .iter()
                .any(|tag| tag == "hs2-command:daily-summary")
        );
        let worker = hotsheet_aitools::session_worker_id(
            "codex",
            &format!(
                "hotsheet-command-{}",
                ticket.id.to_string().to_ascii_lowercase()
            ),
        );
        assert_eq!(ticket.claimed_by.as_deref(), Some(worker.as_str()));
        assert_eq!(
            backend.preparations.lock().unwrap()[0].3.as_deref(),
            Some(worker.as_str())
        );
        wait_for_turns(&backend, 1).await;
        assert!(backend.turns.lock().unwrap()[0].contains(&ticket.slug));
        assert_eq!(body["run"]["launch"]["kind"], "ai");
        assert_eq!(body["run"]["launch"]["tool"], "codex");
        assert_eq!(body["run"]["launch"]["ticket_slug"], ticket.slug.as_str());
        let (_, runs) = send_json(
            &router,
            "GET",
            &format!("/checkouts/{checkout}/command-runs"),
            None,
        )
        .await;
        assert_eq!(runs[0]["command_id"], "daily-summary");
    }

    /// HS2-17Y2EC: a command without an action runs headlessly as Create Ticket Only, the
    /// same queued ticket the legacy click creates, and never prepares a session.
    #[tokio::test]
    async fn hs2_17y2ec_ai_run_without_action_only_queues_the_ticket() {
        let (_root, state, checkout, _settings, store) = fixture();
        let backend = ScheduledFakeDrive::default();
        let router = crate::app(with_scheduled_backend(state, backend.clone()));
        let (status, body) = send_json(
            &router,
            "POST",
            &format!("/checkouts/{checkout}/commands/daily-summary/ai-run"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["action"], "create_ticket");
        assert_eq!(body["delivery"], "queued");
        assert!(body["run"]["launch"].get("tool").is_none());
        let ticket = ops::query(&store, &TicketQuery::default())
            .unwrap()
            .remove(0);
        assert_eq!(ticket.claimed_by, None);
        assert!(ticket.up_next);
        assert!(backend.preparations.lock().unwrap().is_empty());
        let (status, _) = send_json(
            &router,
            "POST",
            &format!("/checkouts/{checkout}/commands/missing/ai-run"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// HS2-17Y2EC: a session action whose provider cannot start still leaves the ticket
    /// queued, unclaimed, with a note, and reports why.
    #[tokio::test]
    async fn hs2_17y2ec_ai_run_failed_handoff_keeps_unclaimed_ticket() {
        let (_root, state, checkout, settings, store) = fixture();
        set_command_action(&settings, Some("reuse_or_new_ai_session"));
        let backend = ScheduledFakeDrive::default();
        backend.fail_prepare_once.store(true, Ordering::SeqCst);
        let router = crate::app(with_scheduled_backend(state, backend.clone()));
        let (status, body) = send_json(
            &router,
            "POST",
            &format!("/checkouts/{checkout}/commands/daily-summary/ai-run"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["delivery"], "failed");
        assert_eq!(body["reason"], "ai_handoff_failed");
        let ticket = ops::query(&store, &TicketQuery::default())
            .unwrap()
            .remove(0);
        assert_eq!(ticket.claimed_by, None);
        assert!(
            ticket
                .notes
                .iter()
                .any(|note| note.text.contains("ai_handoff_failed"))
        );
    }

    /// HS2-17Y2EC: an inheriting schedule follows its command's action, an explicit override
    /// wins over it, and the HTTP view reports the effective action.
    #[tokio::test]
    async fn hs2_17y2ec_schedule_inherits_command_action_until_overridden() {
        let (_root, state, checkout, settings, _store) = fixture();
        set_command_action(&settings, Some("new_ai_session"));
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        let mut inherit = sample_record("2026-03-07", ScheduleAction::CreateTicket);
        inherit.schedule.action = None;
        settings
            .set(command_schedules::SETTING, json!([inherit]), Scope::Local)
            .unwrap();
        process_checkout(
            &state,
            &checkout,
            Timestamp::from_second(1_772_980_200).unwrap(),
        )
        .unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::DeliveryPending
        );
        deliver_pending(&state, &checkout).await.unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::Delivered
        );
        assert_eq!(backend.preparations.lock().unwrap().len(), 1);
        // The stored schedule still inherits; the view resolves it.
        let router = crate::app(state.clone());
        let (_, listed) = send_json(
            &router,
            "GET",
            &format!("/checkouts/{checkout}/command-schedules"),
            None,
        )
        .await;
        assert!(listed[0]["schedule"].get("action").is_none());
        assert_eq!(listed[0]["effective_action"], "new_ai_session");

        // An explicit Create Ticket Only override wins over the command's session action.
        let (_root, state, checkout, settings, _store) = fixture();
        set_command_action(&settings, Some("new_ai_session"));
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        due_ticket(&state, &checkout, &settings, ScheduleAction::CreateTicket);
        deliver_pending(&state, &checkout).await.unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::Created
        );
        assert!(backend.preparations.lock().unwrap().is_empty());
        let router = crate::app(state);
        let (_, listed) = send_json(
            &router,
            "GET",
            &format!("/checkouts/{checkout}/command-schedules"),
            None,
        )
        .await;
        assert_eq!(listed[0]["schedule"]["action"], "create_ticket");
        assert_eq!(listed[0]["effective_action"], "create_ticket");
    }

    /// HS2-17Y2EC with HS2-29V4ZG: an inheriting hourly schedule resolves the command's
    /// action in the shared occurrence helper, records its sub-day key, and delivers it.
    #[tokio::test]
    async fn hs2_17y2ec_sub_day_schedule_inherits_command_action() {
        let (_root, state, checkout, settings, _store) = fixture();
        set_command_action(&settings, Some("new_ai_session"));
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        let mut record = hourly_record("2026-03-08", "09:00");
        record.schedule.action = None;
        settings
            .set(command_schedules::SETTING, json!([record]), Scope::Local)
            .unwrap();
        process_checkout(&state, &checkout, ts("2026-03-08T14:05:00Z")).unwrap();
        let history = records(&settings).unwrap()[0].history.clone();
        assert_eq!(history[0].key(), "2026-03-08T10:00");
        assert_eq!(
            history[0].outcome,
            command_schedules::OccurrenceOutcome::DeliveryPending
        );
        assert_eq!(history[0].turn_completed, Some(false));
        deliver_pending(&state, &checkout).await.unwrap();
        assert_eq!(
            records(&settings).unwrap()[0].history[0].outcome,
            command_schedules::OccurrenceOutcome::Delivered
        );
        assert_eq!(backend.preparations.lock().unwrap().len(), 1);

        // The same inheriting hourly schedule stays ticket-only for a command without an action.
        let (_root, state, checkout, settings, _store) = fixture();
        let backend = ScheduledFakeDrive::default();
        let state = with_scheduled_backend(state, backend.clone());
        settings
            .set(command_schedules::SETTING, json!([record]), Scope::Local)
            .unwrap();
        process_checkout(&state, &checkout, ts("2026-03-08T14:05:00Z")).unwrap();
        deliver_pending(&state, &checkout).await.unwrap();
        let item = records(&settings).unwrap()[0].history[0].clone();
        assert_eq!(item.outcome, command_schedules::OccurrenceOutcome::Created);
        assert_eq!(item.turn_completed, None);
        assert!(backend.preparations.lock().unwrap().is_empty());
    }

    /// HS2-17Y2EC: the schedule API accepts an omitted action as "Use command default" and
    /// stores no action, so a later command change still applies.
    #[tokio::test]
    async fn hs2_17y2ec_schedule_api_saves_inherited_action() {
        let (_root, state, checkout, settings, _store) = fixture();
        let router = crate::app(state);
        let input = json!({
            "time_zone":"UTC", "local_time":"09:00", "paused":false,
            "recurrence":{"kind":"daily", "start_date":"2026-12-01", "every":1}
        });
        let path = format!("/checkouts/{checkout}/command-schedules/daily-summary");
        let (status, saved) = send_json(&router, "PUT", &path, Some(input.clone())).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert!(saved["schedule"].get("action").is_none());
        assert_eq!(saved["effective_action"], "create_ticket");
        assert_eq!(records(&settings).unwrap()[0].schedule.action, None);
        set_command_action(&settings, Some("reuse_or_new_ai_session"));
        let (_, listed) = send_json(
            &router,
            "GET",
            &format!("/checkouts/{checkout}/command-schedules"),
            None,
        )
        .await;
        assert_eq!(listed[0]["effective_action"], "reuse_or_new_ai_session");
        let mut explicit = input;
        explicit["action"] = "new_ai_session".into();
        let (_, saved) = send_json(&router, "PUT", &path, Some(explicit)).await;
        assert_eq!(saved["schedule"]["action"], "new_ai_session");
        assert_eq!(saved["effective_action"], "new_ai_session");
    }

    /// A GitLab Issues fake that answers issue creation and records each request body.
    #[derive(Default)]
    struct CreatingGitLab {
        bodies: Mutex<Vec<serde_json::Value>>,
    }

    impl hotsheet_extsync::GitHubTransport for CreatingGitLab {
        fn request(
            &self,
            method: &str,
            _: &str,
            _: &[(&str, String)],
            body: Option<&serde_json::Value>,
        ) -> Result<hotsheet_extsync::HttpResponse, String> {
            assert_eq!(method, "POST");
            self.bodies.lock().unwrap().push(body.cloned().unwrap());
            Ok(hotsheet_extsync::HttpResponse {
                status: 201,
                headers: std::collections::HashMap::new(),
                body: json!({
                    "id": 9005, "iid": 5, "project_id": 42,
                    "title": "Daily summary", "description": "Summarize synthetic work",
                    "state": "opened",
                    "web_url": "https://gitlab.test/acme/repo/-/issues/5",
                    "created_at": "2026-11-01T00:00:00.000Z",
                    "updated_at": "2026-11-01T00:00:00.000Z",
                    "closed_at": null, "labels": ["category:task"], "assignees": [],
                    "type": "ISSUE"
                })
                .to_string(),
            })
        }
    }

    /// HS2-K9H38S: GitLab refuses Up Next, so a command ticket created in a GitLab source
    /// is not queued there instead of failing the whole command run.
    #[test]
    fn hs2_k9h38s_command_ticket_skips_up_next_where_the_provider_has_none() {
        let (_root, state, checkout, settings, _store) = fixture();
        let gitlab = Arc::new(CreatingGitLab::default());
        let state = state.with_ticket_provider(Arc::new(hotsheet_extsync::GitLabProvider::new(
            hotsheet_extsync::GitLabConfig {
                connection_id: "gitlab-main".into(),
                project: "acme/repo".into(),
                api_base: "https://gitlab.test/api/v4".into(),
                token: "fixture-token".into(),
                default: false,
            },
            gitlab.clone(),
        )));
        let command = commands::from_settings(&settings).unwrap().remove(0);
        let source = hotsheet_ticketing::checkouts::TicketSource {
            connection_id: "gitlab-main".into(),
            provider: "gitlab".into(),
            locator: "acme/repo".into(),
        };
        let ticket = create_occurrence_ticket(&state, &checkout, &source, &command, None).unwrap();
        assert!(!ticket.up_next);
        assert_eq!(ticket.native_id, "5");
        assert_eq!(gitlab.bodies.lock().unwrap().len(), 1);
    }
}
