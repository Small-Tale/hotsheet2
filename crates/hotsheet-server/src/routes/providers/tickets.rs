//! Provider ticket routes: read, create, update, feedback, not-working reports, close, assign, and copy/move (HS2-YMG48C).

use crate::api_error::{ActorReq, ApiError, parse_actor};
use crate::dto::{AiFeedbackReq, CloseReq, CreateReq, DuplicateOfReq, ListParams, UpdateReq};
use crate::multistore::StoreEntry;
use crate::routes::checkout_tickets::resolve_project_ticket_ref;
use crate::routes::providers::{
    ProviderTransferBody, hosted_provider_registry, provider_for, provider_read_get,
    provider_read_query, provider_transfer_error,
};
use crate::routes::ticket_writes::AssignReq;
use crate::routes::tickets::api_ticket;
use crate::{AppState, initial_status, now, opt_parse};
use axum::Json;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use hotsheet_model::{CloseReason, NoteKind, ReviewRequest, Status, Ticket, Ulid};
use hotsheet_ticketing::{
    ApiTicket, MutationContext, NewTicket, NotWorkingReport, ProviderDraft, ProviderEvidence,
    ProviderMutationTiming, ProviderPatch, copy_between, move_between, ops,
};
use serde::Deserialize;
use std::time::{Duration, Instant};

pub(crate) async fn list_provider_tickets(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<ApiTicket>>, ApiError> {
    let query = params.into_query(state.store.root())?;
    provider_read_query(&state, &connection_id, &query).map(Json)
}

pub(crate) async fn get_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    provider_read_get(&state, &connection_id, &id).map(Json)
}

pub(crate) async fn restore_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    let Some(entry) = state.host.get(&connection_id) else {
        // Preserve not-found diagnostics for unknown connections, then report the
        // capability boundary for every configured non-git provider.
        let _ = provider_for(&state, &connection_id)?;
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "provider connection '{connection_id}' does not support git-backed Hot Sheet Trash restore"
            ),
        ));
    };
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let restored = ops::restore(&entry.store, &ticket.id, now())?;
    state.changed_in(&entry, "updated", &restored);
    Ok(Json(api_ticket(&entry, &restored)?))
}

pub(crate) async fn create_provider_ticket(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    Ok((
        StatusCode::CREATED,
        Json(do_provider_create(&state, &connection_id, req)?),
    ))
}

pub(crate) fn do_provider_create(
    state: &AppState,
    connection_id: &str,
    req: CreateReq,
) -> Result<ApiTicket, ApiError> {
    let priority = opt_parse(req.priority.as_deref())?.unwrap_or_default();
    let status = initial_status(req.status.as_deref())?;
    let provider = provider_for(state, connection_id)?;
    let new = ops::normalize_new_ticket_input(NewTicket {
        title: req.title,
        category: req.category.unwrap_or_else(|| "issue".into()),
        priority,
        status,
        details: req.details.unwrap_or_default(),
        tags: req.tags.unwrap_or_default(),
        up_next: req.up_next.unwrap_or(false),
        blocked_by: Vec::new(),
    });
    provider
        .create(
            MutationContext {
                now: now(),
                generated_id: Ulid::new(),
            },
            ProviderDraft {
                title: new.title,
                category: new.category,
                priority: new.priority,
                status: new.status,
                details: new.details,
                tags: new.tags,
                up_next: new.up_next,
                blocked_by: req.blocked_by.unwrap_or_default(),
                transfer: None,
            },
        )
        .map_err(provider_transfer_error)
}

pub(crate) async fn update_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    Json(req): Json<UpdateReq>,
) -> Result<(HeaderMap, Json<ApiTicket>), ApiError> {
    let started = Instant::now();
    let (ticket, timing) = do_provider_update(&state, &connection_id, &id, req)?;
    Ok((
        provider_timing_headers(timing, started.elapsed()),
        Json(ticket),
    ))
}

/// A provider-scoped source ledger. Unsupported connections fail explicitly so a
/// synthesis caller cannot mistake an unreadable provider for zero feedback.
pub(crate) async fn list_provider_ai_feedback(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<Vec<hotsheet_ticketing::AiFeedbackRecord>>, ApiError> {
    let provider = provider_for(&state, &connection_id)?;
    provider
        .ai_feedback_records()
        .map(Json)
        .map_err(|error| match error {
            hotsheet_ticketing::ProviderError::Unsupported { .. } => {
                ApiError::new(StatusCode::NOT_IMPLEMENTED, error.to_string())
            }
            _ => provider_transfer_error(error),
        })
}

pub(crate) fn do_rate_ai_feedback(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
    feedback: AiFeedbackReq,
    actor: Option<&ActorReq>,
    expected_token: Option<&str>,
) -> Result<(ApiTicket, Ticket), ApiError> {
    let (feedback, note_actor) = parse_ai_feedback(feedback, actor)?;
    let ticket = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    if expected_token.is_some_and(|expected| expected != ticket.updated_at.as_str()) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ticket changed since it was read",
        ));
    }
    let updated = ops::rate_ai_content(
        &entry.store,
        &ticket.id,
        now(),
        &feedback.target,
        feedback.rating,
        feedback.explanation,
        Some(note_actor),
    )
    .map_err(|error| {
        if error.is_io_kind(std::io::ErrorKind::InvalidInput) {
            ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
        } else {
            ApiError::from(error)
        }
    })?;
    state.changed_in(entry, "updated", &updated);
    Ok((api_ticket(entry, &updated)?, updated))
}

fn parse_ai_feedback(
    feedback: AiFeedbackReq,
    actor: Option<&ActorReq>,
) -> Result<(hotsheet_model::AiFeedback, hotsheet_model::NoteActor), ApiError> {
    let Some(actor) = parse_actor(actor)? else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "AI feedback requires a rater actor",
        ));
    };
    let note_actor = hotsheet_ticketing::actor::note_actor(Some(&actor)).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "AI feedback requires a rater actor",
        )
    })?;
    if note_actor.id.as_deref().is_none_or(str::is_empty) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "AI feedback requires a stable rater id",
        ));
    }
    let rating =
        serde_json::from_value::<Option<hotsheet_model::AiFeedbackRating>>(feedback.rating)
            .map_err(|error| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("AI feedback rating: {error}"),
                )
            })?;
    Ok((
        hotsheet_model::AiFeedback {
            target: feedback.target,
            rating,
            explanation: feedback.explanation,
        },
        note_actor,
    ))
}

pub(crate) fn do_provider_update(
    state: &AppState,
    connection_id: &str,
    id: &str,
    req: UpdateReq,
) -> Result<(ApiTicket, ProviderMutationTiming), ApiError> {
    if let Some(feedback) = req.ai_feedback.clone() {
        req.validate_feedback_only()?;
        let provider = provider_for(state, connection_id)?;
        let (feedback, actor) = parse_ai_feedback(feedback, req.actor.as_ref())?;
        let ticket = provider
            .rate_ai_content(id, now(), feedback, actor, req.expected_token.as_deref())
            .map_err(|error| match error {
                hotsheet_ticketing::ProviderError::Unsupported { .. } => {
                    ApiError::new(StatusCode::NOT_IMPLEMENTED, error.to_string())
                }
                hotsheet_ticketing::ProviderError::Store(ref store_error)
                    if store_error.is_io_kind(std::io::ErrorKind::InvalidInput) =>
                {
                    ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
                }
                _ => provider_transfer_error(error),
            })?;
        reindex_hosted_provider_write(state, connection_id, id);
        return Ok((ticket, ProviderMutationTiming::default()));
    }
    let provider = provider_for(state, connection_id)?;
    if req.note_id.is_some() && !provider.supports_note_edit() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("provider connection '{connection_id}' does not support note editing"),
        ));
    }
    if req.note_id.is_some() && req.note_summary.is_some() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "note_summary is only valid when appending a note",
        ));
    }
    if req.note_summary.is_some() && req.note.as_deref().is_none_or(str::is_empty) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "note_summary requires a non-empty note",
        ));
    }
    let confidence_change = req.note_confidence_change(req.note_id.is_some())?;
    // An append's null means "no score"; only an edit can clear one (HS2-CY4CWC).
    let note_confidence = confidence_change.flatten();
    if (note_confidence.is_some() || (req.note_id.is_some() && confidence_change.is_some()))
        && !provider.supports_note_confidence()
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("provider connection '{connection_id}' does not support note confidence"),
        ));
    }
    let status = opt_parse(req.status.as_deref())?;
    let actor = parse_actor(req.actor.as_ref())?;
    if actor
        .as_ref()
        .is_some_and(hotsheet_ticketing::actor::MutationActor::is_ai)
        && status == Some(Status::Completed)
    {
        let current = provider.get(id).map_err(provider_transfer_error)?;
        let scores_now = match &req.note_id {
            Some(_) => matches!(confidence_change, Some(Some(_))),
            None => req.note.is_some() && note_confidence.is_some(),
        };
        hotsheet_ticketing::actor::check_completion(
            actor.as_ref(),
            &current.slug,
            hotsheet_ticketing::actor::completes(current.status, status),
            scores_now || hotsheet_ticketing::actor::api_scored_in_current_cycle(&current),
        )?;
    }
    let timestamp = now();
    let note = req.note.clone();
    let note_id = req.note_id.clone();
    let note_kind = req.note_kind.unwrap_or(NoteKind::Regular);
    let note_summary = req.note_summary.clone();
    let (mut ticket, timing) = provider
        .update_timed(
            id,
            timestamp.clone(),
            ProviderPatch {
                actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
                expected_token: req.expected_token,
                title: req.title,
                details: req.details,
                category: req.category,
                priority: opt_parse(req.priority.as_deref())?,
                status,
                started_phase: req
                    .started_phase
                    .as_ref()
                    .map(|phase| opt_parse(phase.as_deref()))
                    .transpose()?,
                tags: req.tags,
                up_next: req.up_next,
                blocked_by: req.blocked_by,
                blocked_reason: req.blocked_reason,
            },
        )
        .map_err(provider_transfer_error)?;
    let edit = ops::NoteEditInput {
        text: note,
        confidence: confidence_change,
        actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
    };
    match (note_id, edit) {
        (Some(note_id), edit) if !edit.is_empty() => {
            ticket = provider
                .edit_note_with_metadata(id, &note_id, timestamp, edit)
                .map_err(provider_transfer_error)?;
        }
        (
            None,
            ops::NoteEditInput {
                text: Some(note), ..
            },
        ) => {
            ticket = provider
                .add_note_with_metadata(
                    id,
                    MutationContext {
                        now: timestamp,
                        generated_id: Ulid::new(),
                    },
                    note_kind,
                    ops::NoteMetadataInput {
                        human_edited: false,
                        summary: note_summary,
                        confidence: note_confidence,
                        actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
                        ai_feedback: None,
                    },
                    note,
                )
                .map_err(provider_transfer_error)?;
        }
        _ => {}
    }
    reindex_hosted_provider_write(state, connection_id, id);
    Ok((ticket, timing))
}

pub(crate) fn provider_timing_headers(
    timing: ProviderMutationTiming,
    total: Duration,
) -> HeaderMap {
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
    let value = format!(
        "provider_read;dur={:.1}, provider_token;dur={:.1}, provider_write;dur={:.1}, provider_ack;dur={:.1}, provider_queue;dur={:.1}, provider_total;dur={:.1}",
        ms(timing.remote_read),
        ms(timing.token_check),
        ms(timing.remote_write),
        ms(timing.acknowledgement),
        ms(timing.queue_wait),
        ms(total),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "server-timing",
        axum::http::HeaderValue::from_str(&value).expect("numeric provider timing"),
    );
    headers
}

/// A provider-route write to a hosted git store reindexes and broadcasts the ticket at
/// once, like the legacy route, so an immediate list read sees it (read-your-writes; the
/// derived `latest_confidence` column depends on it, HS2-RD4M29). External providers are
/// authoritative remotely and have no local index row.
fn reindex_hosted_provider_write(state: &AppState, connection_id: &str, native_id: &str) {
    let Some(entry) = state.host.get(connection_id) else {
        return;
    };
    let Ok(id) = Ulid::from_string(native_id) else {
        return;
    };
    if let Ok(ticket) = entry.store.read_ticket(&id) {
        state.changed_in(&entry, "updated", &ticket);
    }
}

pub(crate) async fn report_provider_ticket_not_working(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<Json<ApiTicket>, ApiError> {
    let provider = provider_for(&state, &connection_id)?;
    if !provider.descriptor().capabilities.not_working_report {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "provider connection '{connection_id}' does not support an atomic Not Working report"
            ),
        ));
    }
    let timestamp = now();
    let mut note = None;
    let mut expected_token = None;
    let mut evidence = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
    {
        match field.name() {
            Some("note") => {
                note =
                    Some(field.text().await.map_err(|error| {
                        ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
                    })?);
            }
            Some("expected_token") => {
                expected_token =
                    Some(field.text().await.map_err(|error| {
                        ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
                    })?);
            }
            Some("evidence") => {
                let filename = field.file_name().unwrap_or("attachment").to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
                evidence.push(ProviderEvidence {
                    id: Ulid::new(),
                    filename,
                    created_at: timestamp.clone(),
                    bytes: bytes.to_vec(),
                });
            }
            _ => {}
        }
    }
    let note = note.and_then(|text| (!text.trim().is_empty()).then(|| (Ulid::new(), text)));
    if note.is_none() && evidence.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "a Not Working report requires a note or at least one evidence attachment",
        ));
    }
    provider
        .report_not_working(
            &id,
            timestamp,
            NotWorkingReport {
                expected_token,
                note,
                evidence,
            },
        )
        .map(Json)
        .map_err(provider_transfer_error)
}

#[derive(Deserialize)]
pub(crate) struct NotWorkingJsonRequest {
    note: Option<String>,
    expected_token: Option<String>,
    #[serde(default)]
    evidence: Vec<NotWorkingJsonEvidence>,
}

#[derive(Deserialize)]
struct NotWorkingJsonEvidence {
    filename: String,
    content_base64: String,
}

pub(crate) async fn report_provider_ticket_not_working_json(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    Json(body): Json<NotWorkingJsonRequest>,
) -> Result<Json<ApiTicket>, ApiError> {
    use base64::Engine;

    let provider = provider_for(&state, &connection_id)?;
    if !provider.descriptor().capabilities.not_working_report {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "provider connection '{connection_id}' does not support an atomic Not Working report"
            ),
        ));
    }
    let timestamp = now();
    let note = body
        .note
        .and_then(|text| (!text.trim().is_empty()).then(|| (Ulid::new(), text)));
    let evidence = body
        .evidence
        .into_iter()
        .map(|item| {
            if item.filename.trim().is_empty() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "evidence filename is required",
                ));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(item.content_base64)
                .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid evidence base64"))?;
            Ok(ProviderEvidence {
                id: Ulid::new(),
                filename: item.filename,
                created_at: timestamp.clone(),
                bytes,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    if note.is_none() && evidence.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "a Not Working report requires a note or at least one evidence attachment",
        ));
    }
    provider
        .report_not_working(
            &id,
            timestamp,
            NotWorkingReport {
                expected_token: body.expected_token,
                note,
                evidence,
            },
        )
        .map(Json)
        .map_err(provider_transfer_error)
}

pub(crate) async fn delete_provider_ticket_note(
    State(state): State<AppState>,
    Path((connection_id, id, note_id)): Path<(String, String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    let provider = provider_for(&state, &connection_id)?;
    if !provider.supports_note_delete() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("provider connection '{connection_id}' does not support note deletion"),
        ));
    }
    provider
        .delete_note(&id, &note_id, now())
        .map(Json)
        .map_err(provider_transfer_error)
}

pub(crate) async fn close_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let duplicate_of = match req.duplicate_of {
        Some(DuplicateOfReq::Legacy(reference)) => Some(reference),
        Some(DuplicateOfReq::Qualified(reference)) => {
            let target = resolve_project_ticket_ref(&state, reference)?;
            // This compatibility route is provider-scoped rather than checkout-scoped,
            // so it has no source project identity to compare. Treat the same provider
            // connection/native pair as the same underlying ticket. Checkout routes use
            // all three ProjectTicketRef fields below.
            if target.connection_id == connection_id && target.native_id == id {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "a ticket cannot be a duplicate of itself",
                ));
            }
            Some(target.qualified())
        }
        None => None,
    };
    let provider = provider_for(&state, &connection_id)?;
    let reason: CloseReason = opt_parse(Some(&req.reason))?.expect("required close reason");
    let actor = parse_actor(req.actor.as_ref())?;
    if actor
        .as_ref()
        .is_some_and(hotsheet_ticketing::actor::MutationActor::is_ai)
    {
        let current = provider.get(&id).map_err(provider_transfer_error)?;
        hotsheet_ticketing::actor::check_completion(
            actor.as_ref(),
            &current.slug,
            hotsheet_ticketing::actor::close_completes(current.status, reason),
            hotsheet_ticketing::actor::api_scored_in_current_cycle(&current),
        )?;
    }
    let closed = provider
        .close(&id, now(), reason, duplicate_of)
        .map_err(provider_transfer_error)?;
    reindex_hosted_provider_write(&state, &connection_id, &id);
    Ok(Json(closed))
}

pub(crate) async fn assign_provider_ticket(
    State(state): State<AppState>,
    Path((connection_id, id)): Path<(String, String)>,
    Json(req): Json<AssignReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let at = now();
    let reviews = req
        .reviews
        .into_iter()
        .map(|review| ReviewRequest {
            who: review.who,
            kind: review.kind,
            by: Ulid::new(),
            at: at.clone(),
            requested_by: None,
        })
        .collect();
    provider_for(&state, &connection_id)?
        .assign(&id, at, req.assignees, reviews)
        .map(Json)
        .map_err(provider_transfer_error)
}

pub(crate) async fn provider_copy_route(
    State(state): State<AppState>,
    Json(body): Json<ProviderTransferBody>,
) -> Result<(StatusCode, Json<hotsheet_ticketing::TransferOutcome>), ApiError> {
    let outcome = copy_between(
        &hosted_provider_registry(&state)?,
        body.source,
        &body.destination_connection,
        &body.operation_id,
        now(),
    )
    .map_err(provider_transfer_error)?;
    Ok((StatusCode::CREATED, Json(outcome)))
}

pub(crate) async fn provider_move_route(
    State(state): State<AppState>,
    Json(body): Json<ProviderTransferBody>,
) -> Result<Json<hotsheet_ticketing::TransferOutcome>, ApiError> {
    if !body.confirm {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "provider move requires confirm=true",
        ));
    }
    let outcome = move_between(
        &hosted_provider_registry(&state)?,
        body.source,
        &body.destination_connection,
        &body.operation_id,
        now(),
    )
    .map_err(provider_transfer_error)?;
    Ok(Json(outcome))
}
