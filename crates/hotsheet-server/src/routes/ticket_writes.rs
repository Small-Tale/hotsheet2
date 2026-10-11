//! Store-generic ticket write logic shared by unprefixed and store-scoped routes.

use crate::*;

/// Body for `POST /stores`: register another local store by its path.
#[derive(Deserialize)]
pub(crate) struct AddStoreBody {
    pub(crate) path: String,
}

/// `POST /stores` — open a store at `path` (building its own in-memory index) and host it.
/// Idempotent: registering an already-hosted store just returns it.
///
/// Hosting a new store opens and reconciles (or rebuilds) its index, which parses every
/// ticket file, and the response counts the added store's tickets. Both run on the blocking
/// pool, so registering a large store never occupies an async request thread (HS2-4XXRJP,
/// HS2-GM4FR2).
pub(crate) async fn add_store(
    State(state): State<AppState>,
    Json(body): Json<AddStoreBody>,
) -> Result<(StatusCode, Json<StoreInfo>), ApiError> {
    let (newly, info) = tokio::task::spawn_blocking(move || {
        let store = FsStore::open(&body.path)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
        let id = multistore::store_url_id(&store);
        let newly = state.host_store(store)?;
        let info = state
            .host
            .info(&id)
            .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "store vanished"))?;
        Ok::<_, ApiError>((newly, info))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    let code = if newly {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((code, Json(info)))
}

/// `GET /stores/{store_id}/tickets` — the store-scoped list, served from that store's own
/// index under the same bound as `GET /tickets`. Unknown id → 404.
pub(crate) async fn list_store_tickets(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Response, ApiError> {
    let entry = state
        .host
        .get(&store_id)
        .ok_or_else(|| ApiError::not_found(&store_id))?;
    list_entry_tickets(&entry, &store_id, params)
}

// The write logic is store-generic: it operates on a `StoreEntry` so the unprefixed
// (default store) routes and the `/stores/{id}/…` scoped routes share one implementation.

pub(crate) fn do_create(
    state: &AppState,
    entry: &StoreEntry,
    req: CreateReq,
) -> Result<ApiTicket, ApiError> {
    let prefix = entry.store.metadata()?.ticket_prefix;
    let status = initial_status(req.status.as_deref())?;
    let blocked_by =
        ops::resolve_blockers(&entry.store, None, &req.blocked_by.unwrap_or_default())?;
    let new = NewTicket {
        title: req.title,
        category: req.category.unwrap_or_else(|| "issue".to_string()),
        priority: opt_parse(req.priority.as_deref())?.unwrap_or_default(),
        status,
        details: req.details.unwrap_or_default(),
        tags: req.tags.unwrap_or_default(),
        up_next: req.up_next.unwrap_or(false),
        blocked_by,
    };
    let ticket = ops::create(&entry.store, Ulid::new(), &prefix, now(), new)?;
    state.changed_in(entry, "created", &ticket);
    api_ticket(entry, &ticket)
}

pub(crate) fn do_update(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
    req: UpdateReq,
) -> Result<ApiTicket, ApiError> {
    do_update_with_ticket(state, entry, id, req, true).map(|(response, _)| response)
}

pub(crate) fn do_update_with_ticket(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
    req: UpdateReq,
    publish: bool,
) -> Result<(ApiTicket, Ticket), ApiError> {
    if let Some(feedback) = req.ai_feedback.clone() {
        req.validate_feedback_only()?;
        return do_rate_ai_feedback(
            state,
            entry,
            id,
            feedback,
            req.actor.as_ref(),
            req.expected_token.as_deref(),
        );
    }
    let ticket = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    let note_text = req.note.clone();
    if req
        .expected_token
        .as_deref()
        .is_some_and(|token| token != ticket.updated_at.as_str())
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("ticket '{}' changed since it was read", ticket.slug),
        ));
    }
    let edit_note_id = req
        .note_id
        .as_deref()
        .map(|note_id| {
            Ulid::from_string(note_id).map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid note ULID '{note_id}'"),
                )
            })
        })
        .transpose()?;
    if let Some(note_id) = edit_note_id
        && !ticket.notes.iter().any(|note| note.id == note_id)
    {
        return Err(ApiError::not_found(&format!("note {note_id}")));
    }
    if edit_note_id.is_some() && req.note_summary.is_some() {
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
    let confidence_change = req.note_confidence_change(edit_note_id.is_some())?;
    // A present `blocked_by` (even []) replaces the set; absent leaves it unchanged.
    let blocked_by = match req.blocked_by {
        Some(needles) => Some(ops::resolve_blockers(
            &entry.store,
            Some(&ticket.id),
            &needles,
        )?),
        None => None,
    };
    let actor = parse_actor(req.actor.as_ref())?;
    let patch = TicketPatch {
        actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
        title: req.title,
        details: req.details,
        category: req.category,
        priority: opt_parse(req.priority.as_deref())?,
        status: opt_parse(req.status.as_deref())?,
        started_phase: req
            .started_phase
            .as_ref()
            .map(|phase| opt_parse(phase.as_deref()))
            .transpose()?,
        tags: req.tags,
        up_next: req.up_next,
        blocked_by,
        blocked_reason: req.blocked_reason,
    };
    // Role-specific rules run before any write (HS2-RD4M29).
    let scores_now = match edit_note_id {
        Some(_) => matches!(confidence_change, Some(Some(_))),
        None => {
            note_text.as_deref().is_some_and(|text| !text.is_empty())
                && confidence_change.flatten().is_some()
        }
    };
    hotsheet_ticketing::actor::check_completion(
        actor.as_ref(),
        &ticket.slug,
        hotsheet_ticketing::actor::completes(ticket.status, patch.status),
        scores_now || hotsheet_ticketing::actor::scored_in_current_cycle(&ticket),
    )?;
    let updated = ops::update(&entry.store, &ticket.id, now(), patch)?;
    // An optional note append/edit rides the same update call (parity with CLI + MCP).
    // An edit may change only the confidence (HS2-CY4CWC); empty text is never written.
    let note_text_edit = req.note.filter(|t| !t.is_empty());
    let latest = match edit_note_id {
        Some(note_id) => {
            let edit = ops::NoteEditInput {
                text: note_text_edit,
                confidence: confidence_change,
                actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
            };
            if edit.is_empty() {
                updated
            } else {
                ops::edit_note_with_metadata(&entry.store, &ticket.id, &note_id, now(), edit)?
            }
        }
        None => match note_text_edit {
            Some(text) => ops::add_note_with_metadata(
                &entry.store,
                &ticket.id,
                Ulid::new(),
                now(),
                req.note_kind.unwrap_or(NoteKind::Regular),
                ops::NoteMetadataInput {
                    human_edited: false,
                    summary: req.note_summary,
                    confidence: confidence_change.flatten(),
                    actor: hotsheet_ticketing::actor::note_actor(actor.as_ref()),
                    ai_feedback: None,
                },
                text,
            )?,
            None => updated,
        },
    };
    if publish {
        state.changed_in(entry, "updated", &latest);
    }
    let mut response = api_ticket(entry, &latest)?;
    if let Some(text) = note_text.filter(|text| !text.is_empty()) {
        response.warnings = ops::attachment_reference_warnings(&entry.store, &latest, &text);
    }
    Ok((response, latest))
}

#[derive(Debug, Deserialize)]
pub(crate) struct AssignReq {
    /// Present replaces the assignee set; absent leaves it unchanged.
    pub(crate) assignees: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) reviews: Vec<ReviewInput>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct ReviewInput {
    pub(crate) who: String,
    pub(crate) kind: ReviewKind,
}

pub(crate) fn do_assign(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
    req: AssignReq,
) -> Result<ApiTicket, ApiError> {
    let before = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    let at = now();
    let reviews = req
        .reviews
        .into_iter()
        .map(|r| ReviewRequest {
            who: r.who,
            kind: r.kind,
            by: Ulid::new(),
            at: at.clone(),
            requested_by: None,
        })
        .collect();
    let ticket = ops::assign(&entry.store, &before.id, at, req.assignees, reviews)?;
    let newly_assigned = ticket
        .assignees
        .iter()
        .filter(|who| !before.assignees.contains(who))
        .cloned()
        .collect::<Vec<_>>();
    let review_requested = ticket
        .review_requests
        .iter()
        .filter(|r| !before.review_requests.iter().any(|old| old.by == r.by))
        .map(|r| r.who.clone())
        .collect::<Vec<_>>();
    state.changed_in(entry, "assigned", &ticket);
    let requested_by = hotsheet_ticketing::current_user_email(entry.store.root());
    state.emit(ChangeEvent {
        cursor: None,
        store: multistore::store_url_id(&entry.store),
        kind: "assignment".into(),
        id: ticket.id.to_string(),
        slug: ticket.slug.clone(),
        message: None,
        activity: None,
        assignment: Some(AssignmentEvent {
            newly_assigned: newly_assigned.clone(),
            review_requested: review_requested.clone(),
            requested_by: requested_by.clone(),
        }),
        turn: None,
    });
    for (recipient, action) in newly_assigned
        .iter()
        .map(|v| (v, "assigned"))
        .chain(review_requested.iter().map(|v| (v, "review-requested")))
    {
        state.notifications.publish(notifications::NewNotification {
            message: format!("{action}: {} — {}", ticket.slug, ticket.title),
            severity: "info".into(),
            checkout: None,
            store: Some(multistore::store_url_id(&entry.store)),
            ticket: Some(ticket.slug.clone()),
            recipient: Some(recipient.clone()),
            dedupe_key: Some(format!("{action}:{}:{recipient}", ticket.id)),
        });
    }
    api_ticket(entry, &ticket)
}

pub(crate) async fn assign_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AssignReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    Ok(Json(do_assign(&state, &state.default_entry(), &id, req)?))
}

pub(crate) fn do_close(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
    source_project_id: Option<&str>,
    req: CloseReq,
) -> Result<ApiTicket, ApiError> {
    let ticket = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    let reason: CloseReason = opt_parse(Some(req.reason.as_str()))?.expect("reason present");
    let actor = parse_actor(req.actor.as_ref())?;
    hotsheet_ticketing::actor::check_completion(
        actor.as_ref(),
        &ticket.slug,
        hotsheet_ticketing::actor::close_completes(ticket.status, reason),
        hotsheet_ticketing::actor::scored_in_current_cycle(&ticket),
    )?;
    let dup = match req.duplicate_of {
        Some(DuplicateOfReq::Legacy(reference)) => {
            let target = ops::resolve(&entry.store, &reference)?
                .ok_or_else(|| ApiError::not_found(&reference))?;
            if target.id == ticket.id {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "a ticket cannot be a duplicate of itself",
                ));
            }
            Some(target.id.to_string())
        }
        Some(DuplicateOfReq::Qualified(reference)) => {
            let target = resolve_project_ticket_ref(state, reference)?;
            // Checkout routes compare the full project/connection/native identity. The
            // legacy default/store routes have no project identity, so they intentionally
            // fall back to same-underlying-store semantics.
            if source_project_id.is_none_or(|project_id| target.project_id == project_id)
                && target.connection_id == multistore::store_url_id(&entry.store)
                && target.native_id == ticket.id.to_string()
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "a ticket cannot be a duplicate of itself",
                ));
            }
            Some(target.qualified())
        }
        None => None,
    };
    let closed = ops::close_as(
        &entry.store,
        &ticket.id,
        now(),
        reason,
        dup,
        hotsheet_ticketing::actor::note_actor(actor.as_ref()).as_ref(),
    )?;
    state.changed_in(entry, "closed", &closed);
    api_ticket(entry, &closed)
}

pub(crate) async fn create_ticket(
    State(state): State<AppState>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let ticket = do_create(&state, &state.default_entry(), req)?;
    Ok((StatusCode::CREATED, Json(ticket)))
}

pub(crate) async fn update_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    Ok(Json(do_update(&state, &state.default_entry(), &id, req)?))
}

pub(crate) async fn delete_ticket_note(
    State(state): State<AppState>,
    Path((id, note_id)): Path<(String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = state.default_entry();
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let note_id = Ulid::from_string(&note_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid note ULID"))?;
    let updated = ops::delete_note(&entry.store, &ticket.id, &note_id, now())?;
    state.changed_in(&entry, "updated", &updated);
    Ok(Json(api_ticket(&entry, &updated)?))
}

pub(crate) async fn add_ticket_attachment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let entry = state.default_entry();
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let filename = attachment_filename(&headers)?;
    let metadata = attachment_metadata(&headers)?;
    let (updated, _) = entry.store.write_attachment_with_metadata(
        &ticket.id,
        Ulid::new(),
        now(),
        &filename,
        &body,
        metadata,
    )?;
    state.changed_in(&entry, "attachment_added", &updated);
    Ok((StatusCode::CREATED, Json(api_ticket(&entry, &updated)?)))
}

pub(crate) fn attachment_filename(headers: &HeaderMap) -> Result<String, ApiError> {
    let raw = headers
        .get("x-hotsheet-filename")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "missing x-hotsheet-filename"))?;
    if headers
        .get("x-hotsheet-filename-encoding")
        .and_then(|value| value.to_str().ok())
        != Some("percent")
    {
        return Ok(raw.to_owned());
    }
    decode_attachment_header(raw, "filename")
}

pub(crate) fn attachment_metadata(
    headers: &HeaderMap,
) -> Result<hotsheet_model::AttachmentMetadata, ApiError> {
    use hotsheet_model::{
        AttachmentActor, AttachmentActorRole, AttachmentMetadata, AttachmentPurpose,
    };
    let encoded = headers
        .get("x-hotsheet-metadata-encoding")
        .and_then(|value| value.to_str().ok())
        == Some("percent");
    let text = |name: &'static str| -> Result<Option<String>, ApiError> {
        let Some(raw) = headers.get(name).and_then(|value| value.to_str().ok()) else {
            return Ok(None);
        };
        let value = if encoded {
            decode_attachment_header(raw, name)?
        } else {
            raw.to_owned()
        };
        Ok((!value.trim().is_empty()).then_some(value))
    };
    let role = match text("x-hotsheet-actor-role")?.as_deref() {
        None => None,
        Some("human") => Some(AttachmentActorRole::Human),
        Some("ai") => Some(AttachmentActorRole::Ai),
        Some("system") => Some(AttachmentActorRole::System),
        Some("unknown") => Some(AttachmentActorRole::Unknown),
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "attachment actor role must be human, ai, system, or unknown",
            ));
        }
    };
    let identity = text("x-hotsheet-actor-identity")?;
    let display_name = text("x-hotsheet-actor-name")?;
    if role.is_none() && (identity.is_some() || display_name.is_some()) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "attachment actor identity/name requires actor role",
        ));
    }
    let purpose = match text("x-hotsheet-attachment-purpose")?.as_deref() {
        None => None,
        Some("problem_evidence") => Some(AttachmentPurpose::ProblemEvidence),
        Some("correctness_evidence") => Some(AttachmentPurpose::CorrectnessEvidence),
        Some("reference") => Some(AttachmentPurpose::Reference),
        Some("other") => Some(AttachmentPurpose::Other),
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "attachment purpose is invalid",
            ));
        }
    };
    let metadata = AttachmentMetadata {
        batch_id: text("x-hotsheet-attachment-batch")?,
        batch_label: text("x-hotsheet-attachment-batch-label")?,
        actor: role.map(|role| AttachmentActor {
            identity,
            display_name,
            role,
        }),
        purpose,
    };
    validate_attachment_metadata(&metadata)?;
    Ok(metadata)
}

pub(crate) fn validate_attachment_metadata(
    metadata: &hotsheet_model::AttachmentMetadata,
) -> Result<(), ApiError> {
    metadata
        .validate()
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, message))
}

pub(crate) fn decode_attachment_header(raw: &str, field: &str) -> Result<String, ApiError> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes
                .get(index + 1)
                .and_then(|value| (*value as char).to_digit(16));
            let low = bytes
                .get(index + 2)
                .and_then(|value| (*value as char).to_digit(16));
            let (Some(high), Some(low)) = (high, low) else {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid encoded attachment {field}"),
                ));
            };
            decoded.push(((high << 4) | low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("invalid encoded attachment {field}"),
        )
    })
}

pub(crate) async fn close_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    Ok(Json(do_close(
        &state,
        &state.default_entry(),
        &id,
        None,
        req,
    )?))
}

/// `POST /batch` — apply the same field update to many tickets (HS2-86). One bad id doesn't
/// abort the rest: each ticket's outcome is reported. Default-store.
pub(crate) async fn batch_update(
    State(state): State<AppState>,
    Json(req): Json<BatchReq>,
) -> Json<BatchResult> {
    let entry = state.default_entry();
    let mut updated = Vec::new();
    let mut errors = Vec::new();
    for id in &req.ids {
        match do_update(&state, &entry, id, req.update.clone()) {
            Ok(t) => updated.push(t.slug),
            Err(e) => errors.push(BatchError {
                id: id.clone(),
                message: e.message,
            }),
        }
    }
    Json(BatchResult { updated, errors })
}
