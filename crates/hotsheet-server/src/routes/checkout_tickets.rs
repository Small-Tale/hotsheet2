//! Checkout-scoped ticket CRUD and attachment routes.

use crate::*;

#[derive(Deserialize)]
pub(crate) struct CheckoutStoreQuery {
    pub(crate) store: Option<String>,
    pub(crate) source: Option<String>,
}
pub(crate) fn checkout_source_for_create(
    state: &AppState,
    reference: &str,
    requested: Option<&str>,
) -> Result<hotsheet_ticketing::checkouts::TicketSource, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    let requested = requested.or(checkout.default_source.as_deref());
    if let Some(id) = requested {
        return checkout
            .sources
            .into_iter()
            .find(|source| source.connection_id == id || source.locator == id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "requested source is not linked to checkout",
                )
            });
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "checkout has no default ticket source; specify ?source=<connection-id>",
    ))
}

pub(crate) fn checkout_ticket_owner(
    state: &AppState,
    reference: &str,
    id: &str,
) -> Result<(hotsheet_ticketing::checkouts::TicketSource, String), ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    if let Some((connection_id, native_id)) = id.split_once(':') {
        match state
            .checkout_registry
            .resolve_source(reference, connection_id)
        {
            Ok((_, source)) => return Ok((source, native_id.to_string())),
            Err(hotsheet_ticketing::checkouts::CheckoutError::NotFound(_)) => {}
            Err(error) => {
                return Err(ApiError::new(StatusCode::CONFLICT, error.to_string()));
            }
        }
    }
    let mut found = Vec::new();
    let mut unavailable = None;
    for source in checkout.sources {
        let exists = if source.provider == "git" {
            state
                .host
                .get(&source.connection_id)
                .map(|entry| ops::resolve(&entry.store, id))
                .transpose()?
                .flatten()
                .is_some()
        } else {
            match probe_provider_source(state, &source.connection_id, id) {
                Ok(ticket) => ticket.is_some(),
                // A source that could not answer only matters when no other source owns the id.
                Err(error) => {
                    unavailable.get_or_insert(error);
                    false
                }
            }
        };
        if exists {
            found.push(source);
        }
    }
    match found.as_slice() {
        [source] => Ok((source.clone(), id.to_string())),
        [] => Err(unavailable.unwrap_or_else(|| ApiError::not_found(id))),
        _ => Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("ticket {id} is ambiguous across checkout sources; use its qualified id"),
        )),
    }
}

pub(crate) fn resolve_project_ticket_ref(
    state: &AppState,
    reference: ProjectTicketRef,
) -> Result<ProjectTicketRef, ApiError> {
    let (checkout, source) = state
        .checkout_registry
        .resolve_source(&reference.project_id, &reference.connection_id)
        .map_err(checkout_lookup_error)?;
    let native_id = if source.provider == "git" {
        let entry = state.hosted_source(&source).ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                format!("checkout links an unhosted git source: {}", source.locator),
            )
        })?;
        ops::resolve(&entry.store, &reference.native_id)?
            .ok_or_else(|| ApiError::not_found(&reference.qualified()))?
            .id
            .to_string()
    } else {
        provider_for(state, &source.connection_id)?
            .get(&reference.native_id)
            .map_err(provider_transfer_error)?
            .native_id
    };
    Ok(ProjectTicketRef {
        project_id: checkout.id,
        connection_id: source.connection_id,
        native_id,
    })
}

pub(crate) async fn create_checkout_ticket(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(q): Query<CheckoutStoreQuery>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let source = checkout_source_for_create(
        &state,
        &reference,
        q.source.as_deref().or(q.store.as_deref()),
    )?;
    let ticket = if source.provider == "git" {
        let entry = state.hosted_source(&source).ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                format!("checkout links an unhosted git source: {}", source.locator),
            )
        })?;
        do_create(&state, &entry, req)?
    } else {
        do_provider_create(&state, &source.connection_id, req)?
    };
    Ok((
        StatusCode::CREATED,
        Json(contextualize_api_ticket(ticket, &settings)?),
    ))
}
/// Resolve ownership and retain the fetched payload. A successful qualified reference
/// reads its checkout/source once; an unqualified reference reads each candidate once
/// while preserving ambiguity detection (including remote providers).
pub(crate) fn resolve_checkout_ticket(
    state: &AppState,
    reference: &str,
    id: &str,
) -> Result<
    (
        hotsheet_ticketing::checkouts::Checkout,
        hotsheet_ticketing::checkouts::TicketSource,
        ResolvedTicket,
    ),
    ApiError,
> {
    if let Some((connection_id, native_id)) = id.split_once(':') {
        match state
            .checkout_registry
            .resolve_source(reference, connection_id)
        {
            Ok((checkout, source)) => {
                let ticket = read_checkout_ticket_source(state, &source, native_id, true)?
                    .ok_or_else(|| ApiError::not_found(id))?;
                return Ok((checkout, source, ticket));
            }
            Err(hotsheet_ticketing::checkouts::CheckoutError::NotFound(_)) => {}
            Err(error) => return Err(ApiError::new(StatusCode::CONFLICT, error.to_string())),
        }
    }
    let (checkout, _) = checkout_settings(state, reference)?;
    let mut found = None;
    let mut unavailable = None;
    for source in &checkout.sources {
        // A source that could not answer only matters when no other source owns the id.
        let ticket = match read_checkout_ticket_source(state, source, id, false) {
            Ok(ticket) => ticket,
            Err(error) => {
                unavailable.get_or_insert(error);
                None
            }
        };
        if let Some(ticket) = ticket {
            if found.is_some() {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    format!(
                        "ticket {id} is ambiguous across checkout sources; use its qualified id"
                    ),
                ));
            }
            found = Some((source.clone(), ticket));
        }
    }
    let (source, ticket) =
        found.ok_or_else(|| unavailable.unwrap_or_else(|| ApiError::not_found(id)))?;
    Ok((checkout, source, ticket))
}

pub(crate) fn read_checkout_ticket_source(
    state: &AppState,
    source: &hotsheet_ticketing::checkouts::TicketSource,
    id: &str,
    required: bool,
) -> Result<Option<ResolvedTicket>, ApiError> {
    if source.provider == "git" {
        let Some(entry) = state.hosted_source(source) else {
            return if required {
                Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "checkout links an unhosted git source",
                ))
            } else {
                Ok(None)
            };
        };
        let Some(ticket) = ops::resolve(&entry.store, id)? else {
            return Ok(None);
        };
        let store = multistore::store_url_id(&entry.store);
        return Ok(Some(ResolvedTicket {
            ticket: ApiTicket::from_provider(&ticket, &store, None),
            store,
        }));
    }
    // A qualified reference names this source explicitly, so say why it cannot be read.
    if required && connection_disabled(state, &source.connection_id)? {
        return Err(provider_transfer_error(
            hotsheet_ticketing::ProviderError::Disabled {
                connection_id: source.connection_id.clone(),
            },
        ));
    }
    Ok(
        probe_provider_source(state, &source.connection_id, id)?.map(|ticket| ResolvedTicket {
            store: source.connection_id.clone(),
            ticket,
        }),
    )
}

/// Read `id` from an external provider source. `None` when the source does not hold it,
/// including an id the provider cannot represent: a git ULID is never a GitHub issue number,
/// so probing every linked source for an unqualified id must not fail on that (HS2-GKERTK).
pub(crate) fn probe_provider_source(
    state: &AppState,
    connection_id: &str,
    id: &str,
) -> Result<Option<ApiTicket>, ApiError> {
    // A disabled source is not read, so for an unqualified probe it holds nothing (HS2-SF6W34).
    if connection_disabled(state, connection_id)? {
        return Ok(None);
    }
    let provider = provider_for(state, connection_id)?;
    let result = if provider.descriptor().capabilities.write_behind {
        if let Some(outbox) = &state.jira_outbox {
            let guard = outbox.lock_or_recover();
            provider_overlay::get(provider.as_ref(), &guard, connection_id, id)
        } else {
            provider
                .get(id)
                .map_err(provider_overlay::OverlayReadError::from)
        }
    } else {
        provider
            .get(id)
            .map_err(provider_overlay::OverlayReadError::from)
    };
    match result {
        Ok(ticket) => Ok(Some(ticket)),
        Err(provider_overlay::OverlayReadError::Provider(
            hotsheet_ticketing::ProviderError::NotFound { .. }
            | hotsheet_ticketing::ProviderError::InvalidNativeId { .. },
        )) => Ok(None),
        Err(error) => Err(provider_transfer_error(error)),
    }
}

pub(crate) async fn get_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let (checkout, _, mut resolved) = resolve_checkout_ticket(&state, &reference, &id)?;
        resolved.ticket = contextualize_api_ticket(resolved.ticket, &checkout.settings())?;
        Ok(Json(resolved))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

#[derive(Debug, Serialize)]
pub(crate) struct DuplicateBacklink {
    pub(crate) reference: String,
    pub(crate) project_id: String,
    pub(crate) project_name: String,
    pub(crate) connection_id: String,
    pub(crate) native_id: String,
    pub(crate) qualified_id: String,
    pub(crate) slug: String,
    pub(crate) title: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DuplicateBacklinkProject {
    pub(crate) project_id: String,
    pub(crate) project_name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DuplicateBacklinkResponse {
    pub(crate) backlinks: Vec<DuplicateBacklink>,
    pub(crate) inaccessible_projects: Vec<DuplicateBacklinkProject>,
}

pub(crate) fn duplicate_reference_matches(reference: &str, target: &ProjectTicketRef) -> bool {
    if let Some(exact) = ProjectTicketRef::from_qualified(reference) {
        return exact == *target;
    }
    reference == target.native_id
}

pub(crate) async fn get_checkout_ticket_duplicate_backlinks(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<DuplicateBacklinkResponse>, ApiError> {
    let (backlinks, inaccessible_projects) = tokio::task::spawn_blocking(move || {
        let (checkout, source, resolved) = resolve_checkout_ticket(&state, &reference, &id)?;
        let target = ProjectTicketRef {
            project_id: checkout.id,
            connection_id: source.connection_id,
            native_id: resolved.ticket.native_id,
        };
        let checkouts = state
            .checkout_registry
            .list()
            .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        // Cold sources build an index once on this blocking worker; warm sources query
        // maintained reverse indexes. No host-registry lock spans indexing or file I/O.
        let mut backlinks = Vec::new();
        let mut inaccessible_projects = Vec::new();
        for checkout in checkouts {
            // A remembered checkout may outlive a temporary or deleted working directory.
            // It cannot contain a usable backlink while absent, and presenting it as a
            // transient source failure makes every ticket show a permanent warning.
            if !FsPath::new(&checkout.root).is_dir() {
                continue;
            }
            let mut inaccessible = false;
            for source in &checkout.sources {
                let tickets = if source.provider == "git" {
                    // A directory can be recreated after a remembered temporary checkout is
                    // deleted (for example by an old setup tool) without recreating its HS2
                    // ticket store. A locator without HS2 metadata is no longer a searchable
                    // source, not a transient lookup failure that should warn on every ticket.
                    if !FsPath::new(&source.locator)
                        .join(STORE_METADATA_FILE)
                        .is_file()
                    {
                        continue;
                    }
                    let indexed =
                        (|| -> Result<Vec<hotsheet_index::DuplicateBacklinkRow>, ApiError> {
                            let store = FsStore::open(&source.locator)?;
                            state.host_store(store.clone())?;
                            let entry = state
                                .host
                                .get(&multistore::store_url_id(&store))
                                .ok_or_else(|| ApiError::not_found(&source.connection_id))?;
                            let rows = entry
                                .index
                                .lock_or_recover()
                                .duplicate_backlinks(&target.qualified(), &target.native_id)?;
                            // A watcher retains the last healthy index row for corrupt files.
                            // Validate only indexed matches, outside the index lock, to retain
                            // the old resilient scan's omission of corrupt or removed sources.
                            Ok(rows
                                .into_iter()
                                .filter_map(|mut row| {
                                    let id = Ulid::from_string(&row.id).ok()?;
                                    let ticket = entry.store.read_ticket(&id).ok()?;
                                    if ticket.close_reason != Some(CloseReason::Duplicate)
                                        || !ticket.duplicate_of.as_deref().is_some_and(
                                            |reference| {
                                                duplicate_reference_matches(reference, &target)
                                            },
                                        )
                                    {
                                        return None;
                                    }
                                    row.slug = ticket.slug;
                                    row.title = ticket.title;
                                    Some(row)
                                })
                                .collect())
                        })();
                    match indexed {
                        Ok(rows) => {
                            for row in rows {
                                let source_reference = ProjectTicketRef {
                                    project_id: checkout.id.clone(),
                                    connection_id: source.connection_id.clone(),
                                    native_id: row.id.clone(),
                                };
                                backlinks.push(DuplicateBacklink {
                                    reference: source_reference.qualified(),
                                    project_id: checkout.id.clone(),
                                    project_name: checkout.alias.clone(),
                                    connection_id: source.connection_id.clone(),
                                    qualified_id: format!("{}:{}", source.connection_id, row.id),
                                    native_id: row.id,
                                    slug: row.slug,
                                    title: row.title,
                                });
                            }
                            continue;
                        }
                        Err(_) => {
                            inaccessible = true;
                            continue;
                        }
                    }
                } else if connection_disabled(&state, &source.connection_id).unwrap_or(false) {
                    // Disabled sources are not read; they are not "inaccessible" (HS2-SF6W34).
                    continue;
                } else {
                    match provider_for(&state, &source.connection_id).and_then(|provider| {
                        provider
                            .query(&TicketQuery::default())
                            .map_err(provider_transfer_error)
                    }) {
                        Ok(tickets) => tickets,
                        Err(_) => {
                            inaccessible = true;
                            continue;
                        }
                    }
                };
                for ticket in tickets {
                    let Some(duplicate_of) = ticket.duplicate_of.as_deref() else {
                        continue;
                    };
                    if ticket.close_reason != Some(CloseReason::Duplicate)
                        || !duplicate_reference_matches(duplicate_of, &target)
                    {
                        continue;
                    }
                    let source_reference = ProjectTicketRef {
                        project_id: checkout.id.clone(),
                        connection_id: ticket.connection_id.clone(),
                        native_id: ticket.native_id.clone(),
                    };
                    backlinks.push(DuplicateBacklink {
                        reference: source_reference.qualified(),
                        project_id: checkout.id.clone(),
                        project_name: checkout.alias.clone(),
                        connection_id: ticket.connection_id,
                        native_id: ticket.native_id,
                        qualified_id: ticket.qualified_id,
                        slug: ticket.slug,
                        title: ticket.title,
                    });
                }
            }
            if inaccessible {
                inaccessible_projects.push(DuplicateBacklinkProject {
                    project_id: checkout.id,
                    project_name: checkout.alias,
                });
            }
        }
        backlinks.sort_by(|left, right| {
            left.project_name
                .cmp(&right.project_name)
                .then(left.slug.cmp(&right.slug))
                .then(left.reference.cmp(&right.reference))
        });
        backlinks.dedup_by(|left, right| left.reference == right.reference);
        inaccessible_projects.sort_by(|left, right| {
            left.project_name
                .cmp(&right.project_name)
                .then(left.project_id.cmp(&right.project_id))
        });
        Ok::<_, ApiError>((backlinks, inaccessible_projects))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    Ok(Json(DuplicateBacklinkResponse {
        backlinks,
        inaccessible_projects,
    }))
}
pub(crate) async fn update_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<UpdateReq>,
) -> Result<(HeaderMap, Json<ResolvedTicket>), ApiError> {
    let started = Instant::now();
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        let (ticket, timing) = do_provider_update(&state, &source.connection_id, &native_id, req)?;
        return Ok((
            provider_timing_headers(timing, started.elapsed()),
            Json(ResolvedTicket {
                store: source.connection_id.clone(),
                ticket: contextualize_api_ticket(ticket, &settings)?,
            }),
        ));
    }
    let entry = state.hosted_source(&source).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "checkout links an unhosted git source",
        )
    })?;
    Ok((
        HeaderMap::new(),
        Json(ResolvedTicket {
            store: multistore::store_url_id(&entry.store),
            ticket: contextualize_api_ticket(
                do_update(&state, &entry, &native_id, req)?,
                &settings,
            )?,
        }),
    ))
}

#[derive(Deserialize)]
pub(crate) struct CheckoutBatchUpdateReq {
    pub(crate) id: String,
    #[serde(flatten)]
    pub(crate) update: UpdateReq,
}

#[derive(Deserialize)]
pub(crate) struct CheckoutBatchReq {
    pub(crate) updates: Vec<CheckoutBatchUpdateReq>,
    /// One actor for the whole bulk operation (HS2-XF81CJ); an update naming its own wins.
    #[serde(default)]
    pub(crate) actor: Option<ActorReq>,
}

/// Apply a multi-selection update through one checkout-scoped request. All optimistic
/// concurrency tokens are validated before the first write, so a stale selection cannot
/// partially apply while the client still sees one logical bulk operation.
pub(crate) async fn batch_update_checkout_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<CheckoutBatchReq>,
) -> Result<(HeaderMap, Json<Vec<ResolvedTicket>>), ApiError> {
    let started = Instant::now();
    if req.updates.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bulk update requires at least one ticket",
        ));
    }
    let (_, settings) = checkout_settings(&state, &reference)?;
    let mut resolved = Vec::with_capacity(req.updates.len());
    for mut item in req.updates {
        if item.update.actor.is_none() {
            item.update.actor.clone_from(&req.actor);
        }
        let (entry, ticket) = checkout_git_ticket(&state, &reference, &item.id)?;
        if item
            .update
            .expected_token
            .as_deref()
            .is_some_and(|token| token != ticket.updated_at.as_str())
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!("ticket {} changed since it was read", item.id),
            ));
        }
        resolved.push((entry, ticket.id.to_string(), item));
    }
    let admission_time = started.elapsed();
    let mut updated = Vec::with_capacity(resolved.len());
    let mut pending =
        std::collections::BTreeMap::<std::path::PathBuf, (StoreEntry, Vec<Ticket>)>::new();
    let mut failure = None;
    for (entry, native_id, item) in resolved {
        let mut deferred_entry = entry.clone();
        deferred_entry.store = deferred_entry.store.with_deferred_autocommit();
        let result = do_update_with_ticket(&state, &deferred_entry, &native_id, item.update, false)
            .and_then(|(ticket, changed)| {
                contextualize_api_ticket(ticket, &settings).map(|ticket| (ticket, changed))
            });
        match result {
            Ok((ticket, changed)) => {
                updated.push(ResolvedTicket {
                    store: multistore::store_url_id(&entry.store),
                    ticket,
                });
                pending
                    .entry(entry.store.root().to_path_buf())
                    .or_insert_with(|| (entry, Vec::new()))
                    .1
                    .push(changed);
            }
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    let write_time = started.elapsed() - admission_time;
    let mut git_time = Duration::ZERO;
    let mut index_time = Duration::ZERO;
    let mut event_time = Duration::ZERO;
    let mut worklist_time = Duration::ZERO;
    for (_, (entry, tickets)) in pending {
        let paths = tickets
            .iter()
            .map(|ticket| entry.store.ticket_path(&ticket.id))
            .collect::<Vec<_>>();
        let git_started = Instant::now();
        if let Err(error) = entry
            .store
            .autocommit_paths("Update selected Hot Sheet tickets", &paths)
        {
            tracing::warn!("hotsheet batch autocommit failed: {error}");
        }
        git_time += git_started.elapsed();
        let [index, events, worklist] = state.changed_many_in(&entry, "updated", &tickets);
        index_time += index;
        event_time += events;
        worklist_time += worklist;
    }
    if let Some(error) = failure {
        return Err(error);
    }
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
    let timing = format!(
        "admission;dur={:.1}, write;dur={:.1}, git;dur={:.1}, index;dur={:.1}, events;dur={:.1}, worklist;dur={:.1}, total;dur={:.1}",
        ms(admission_time),
        ms(write_time),
        ms(git_time),
        ms(index_time),
        ms(event_time),
        ms(worklist_time),
        ms(started.elapsed()),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "server-timing",
        axum::http::HeaderValue::from_str(&timing).expect("numeric server timing"),
    );
    Ok((headers, Json(updated)))
}
pub(crate) async fn delete_checkout_ticket_note(
    State(state): State<AppState>,
    Path((reference, id, note_id)): Path<(String, String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let note_id = Ulid::from_string(&note_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid note ULID"))?;
    let updated = ops::delete_note(&entry.store, &ticket.id, &note_id, now())?;
    state.changed_in(&entry, "updated", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}
pub(crate) async fn close_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    let source_project = checkout.id.clone();
    let settings = checkout.settings();
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        let duplicate_of = match req.duplicate_of {
            Some(DuplicateOfReq::Legacy(reference)) => Some(reference),
            Some(DuplicateOfReq::Qualified(reference)) => {
                let target = resolve_project_ticket_ref(&state, reference)?;
                if target.project_id == source_project
                    && target.connection_id == source.connection_id
                    && target.native_id == native_id
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
        let ticket = provider_for(&state, &source.connection_id)?
            .close(
                &native_id,
                now(),
                opt_parse(Some(&req.reason))?.expect("required close reason"),
                duplicate_of,
            )
            .map_err(provider_transfer_error)?;
        return Ok(Json(ResolvedTicket {
            store: source.connection_id,
            ticket: contextualize_api_ticket(ticket, &settings)?,
        }));
    }
    let entry = state.hosted_source(&source).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "checkout links an unhosted git source",
        )
    })?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: contextualize_api_ticket(
            do_close(&state, &entry, &native_id, Some(&source_project), req)?,
            &settings,
        )?,
    }))
}
/// Restore a Trash ticket to its pre-deletion status (HS2-MWDR19). Trash is the git
/// provider's soft-delete lifecycle; other providers own deletion natively.
pub(crate) async fn restore_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (source, _) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "the {} provider has no Hot Sheet Trash to restore from",
                source.provider
            ),
        ));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: contextualize_api_ticket(
            do_restore(&state, &entry, &ticket.id.to_string())?,
            &settings,
        )?,
    }))
}

pub(crate) fn do_restore(
    state: &AppState,
    entry: &StoreEntry,
    id: &str,
) -> Result<ApiTicket, ApiError> {
    let ticket = ops::resolve(&entry.store, id)?.ok_or_else(|| ApiError::not_found(id))?;
    let restored = ops::restore(&entry.store, &ticket.id, now())?;
    state.changed_in(entry, "updated", &restored);
    api_ticket(entry, &restored)
}

pub(crate) async fn empty_checkout_trash(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let entries = checkout_entries(&state, &reference)?;
    if entries.is_empty() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "this checkout has no git-backed Hot Sheet Trash capability",
        ));
    }
    let mut purged = Vec::new();
    for (_, entry) in entries {
        for ticket in ops::purge_trash(&entry.store, &now(), 0)? {
            state.removed_in(&entry, &ticket);
            purged.push(ticket.slug);
        }
    }
    Ok(Json(serde_json::json!({
        "purged": purged.len(),
        "tickets": purged,
    })))
}

pub(crate) async fn assign_checkout_ticket(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(req): Json<AssignReq>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: contextualize_api_ticket(
            do_assign(&state, &entry, &ticket.id.to_string(), req)?,
            &settings,
        )?,
    }))
}

pub(crate) async fn add_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<ResolvedTicket>), ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        // An external provider stores the file natively when its capability allows it
        // (GitHub through its assets repository, HS2-HSA64D); otherwise it refuses explicitly.
        let filename = attachment_filename(&headers)?;
        let metadata = attachment_metadata(&headers)?;
        let ticket = provider_for(&state, &source.connection_id)?
            .add_attachment(
                &native_id,
                ApiAttachment {
                    id: Ulid::new().to_string(),
                    filename,
                    created_at: now().as_str().to_string(),
                    batch_id: metadata.batch_id,
                    batch_label: metadata.batch_label,
                    actor: metadata.actor,
                    purpose: metadata.purpose,
                    annotations: vec![],
                    crop: None,
                    revision: None,
                },
                body.to_vec(),
            )
            .map_err(provider_transfer_error)?;
        return Ok((
            StatusCode::CREATED,
            Json(ResolvedTicket {
                store: source.connection_id,
                ticket: contextualize_api_ticket(ticket, &settings)?,
            }),
        ));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
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
    Ok((
        StatusCode::CREATED,
        Json(ResolvedTicket {
            store: multistore::store_url_id(&entry.store),
            ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
        }),
    ))
}

/// Resolve a ticket reference — a qualified `{connection}:{native}` id, a bare ULID, or a slug —
/// to the hosted git store entry that owns it plus the ticket itself. Every checkout-scoped
/// handler that needs the store goes through here, so a qualified id the client routes by
/// (HS2-HX0VM9) resolves exactly like a bare one (HS2-QS9EQD).
pub(crate) fn checkout_git_ticket(
    state: &AppState,
    reference: &str,
    id: &str,
) -> Result<(StoreEntry, Ticket), ApiError> {
    let (source, native_id) = checkout_ticket_owner(state, reference, id)?;
    if source.provider != "git" {
        // Store-only operations (attachment edits, thumbnails, local file actions, note
        // deletion) name the provider instead of reporting a missing git store (HS2-HSA64D).
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "provider connection '{}' ({}) does not support this operation",
                source.connection_id, source.provider
            ),
        ));
    }
    let entry = state.hosted_source(&source).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "checkout links an unhosted git source",
        )
    })?;
    let ticket = ops::resolve(&entry.store, &native_id)?.ok_or_else(|| ApiError::not_found(id))?;
    Ok((entry, ticket))
}

/// Serve an external provider's attachment through the provider (HS2-HSA64D), so the
/// browser never needs the provider's credentials. Returns `None` for a git-owned ticket.
pub(crate) fn provider_attachment_response(
    state: &AppState,
    reference: &str,
    id: &str,
    matches: impl Fn(&ApiAttachment) -> bool,
    range: Option<&str>,
    original: bool,
) -> Result<Option<Response>, ApiError> {
    let (source, native_id) = checkout_ticket_owner(state, reference, id)?;
    if source.provider == "git" {
        return Ok(None);
    }
    let provider = provider_for(state, &source.connection_id)?;
    let ticket = provider.get(&native_id).map_err(provider_transfer_error)?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| matches(attachment))
        .ok_or_else(|| ApiError::not_found(id))?;
    let bytes = if original {
        provider.attachment_original_bytes(&native_id, &attachment.id)
    } else {
        provider.attachment_bytes(&native_id, &attachment.id)
    }
    .map_err(provider_transfer_error)?;
    Ok(Some(media::attachment_response(
        &attachment.filename,
        bytes,
        range,
    )))
}

pub(crate) async fn get_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(response) = provider_attachment_response(
        &state,
        &reference,
        &id,
        |attachment| attachment.id == attachment_id,
        headers.get("range").and_then(|value| value.to_str().ok()),
        false,
    )? {
        return Ok(response);
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| ApiError::not_found(&attachment_id.to_string()))?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    git_attachment_response(
        state.cache_dir(),
        &attachment.filename,
        &path,
        attachment.crop,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )
    .await
}

pub(crate) async fn get_checkout_ticket_attachment_original(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(response) = provider_attachment_response(
        &state,
        &reference,
        &id,
        |attachment| attachment.id == attachment_id,
        headers.get("range").and_then(|value| value.to_str().ok()),
        true,
    )? {
        return Ok(response);
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| ApiError::not_found(&attachment_id.to_string()))?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    git_attachment_response(
        state.cache_dir(),
        &attachment.filename,
        &path,
        None,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )
    .await
}

pub(crate) async fn git_attachment_response(
    cache_root: &std::path::Path,
    filename: &str,
    path: &std::path::Path,
    crop: Option<hotsheet_model::ImageCrop>,
    range: Option<&str>,
) -> Result<Response, ApiError> {
    if let Some(crop) = crop {
        let bytes = tokio::fs::read(path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ApiError::not_found(filename)
            } else {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            }
        })?;
        let filename_owned = filename.to_string();
        let cache_root = cache_root.to_path_buf();
        let rendition = tokio::task::spawn_blocking(move || {
            use sha2::{Digest, Sha256};
            let mut digest = Sha256::new();
            digest.update(&bytes);
            digest.update(filename_owned.as_bytes());
            digest.update(crop.x.to_be_bytes());
            digest.update(crop.y.to_be_bytes());
            digest.update(crop.width.to_be_bytes());
            digest.update(crop.height.to_be_bytes());
            let key = format!("{:x}", digest.finalize());
            if let Some(cached) = image_crop_cache::read(&cache_root, &key) {
                return Ok(cached);
            }
            let result =
                hotsheet_ticketing::image_crop::cropped_rendition(&filename_owned, &bytes, crop)?;
            image_crop_cache::store(&cache_root, &key, &result);
            Ok::<_, hotsheet_ticketing::image_crop::ImageCropError>(result)
        })
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
        return Ok(media::attachment_response(filename, rendition, range));
    }
    media::attachment_file_response(filename, path, range)
        .await
        .map_err(|error| {
            if matches!(&error, media::MediaError::Io(error) if error.kind() == std::io::ErrorKind::NotFound) {
                ApiError::not_found(filename)
            } else {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            }
        })
}

pub(crate) async fn get_checkout_ticket_attachment_by_name(
    State(state): State<AppState>,
    Path((reference, id, filename)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(response) = provider_attachment_response(
        &state,
        &reference,
        &id,
        |attachment| attachment.filename == filename,
        headers.get("range").and_then(|value| value.to_str().ok()),
        false,
    )? {
        return Ok(response);
    }
    let (entry, ticket, attachment_id) =
        checkout_attachment_by_name(&state, &reference, &id, &filename)?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| ApiError::not_found(&attachment_id.to_string()))?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    git_attachment_response(
        state.cache_dir(),
        &attachment.filename,
        &path,
        attachment.crop,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )
    .await
}

pub(crate) async fn get_checkout_ticket_attachment_thumbnail(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, bytes) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    let filename = attachment.filename;
    let thumbnail = {
        let cache_dir = Arc::clone(&state.cache_dir);
        tokio::task::spawn_blocking(move || {
            media::optional_video_poster(&cache_dir, &filename, &bytes)
        })
    }
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("video thumbnail task failed: {error}"),
        )
    })?
    .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "video poster has not been generated"))?;
    let mut response = media::attachment_response("thumbnail.jpg", thumbnail, None);
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(response)
}

pub(crate) async fn put_checkout_ticket_attachment_thumbnail(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    if headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or_default().trim())
        != Some("image/jpeg")
    {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "video posters must be JPEG images",
        ));
    }
    if body.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "video poster cannot be empty",
        ));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, bytes) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    if !media::is_video(&attachment.filename) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "attachment is not a supported video",
        ));
    }
    let cache_dir = Arc::clone(&state.cache_dir);
    tokio::task::spawn_blocking(move || media::cache_video_poster(&cache_dir, &bytes, &body))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("video poster task failed: {error}"),
            )
        })?
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(crate) struct AttachmentHostActionRequest {
    pub(crate) action: AttachmentHostAction,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttachmentHostAction {
    Open,
    Reveal,
    Path,
}

#[derive(Debug, Serialize)]
pub(crate) struct AttachmentHostActionResponse {
    pub(crate) path: String,
}

pub(crate) fn checkout_attachment_by_name(
    state: &AppState,
    reference: &str,
    id: &str,
    filename: &str,
) -> Result<(StoreEntry, Ticket, Ulid), ApiError> {
    let (entry, ticket) = checkout_git_ticket(state, reference, id)?;
    let attachment_id = ticket
        .attachments
        .iter()
        .rev()
        .filter(|attachment| filename.starts_with(&attachment.filename))
        .fold(
            None::<&hotsheet_model::Attachment>,
            |best, attachment| match best {
                Some(current) if current.filename.len() >= attachment.filename.len() => {
                    Some(current)
                }
                _ => Some(attachment),
            },
        )
        .map(|attachment| attachment.id)
        .ok_or_else(|| ApiError::not_found(filename))?;
    Ok((entry, ticket, attachment_id))
}

pub(crate) fn attachment_disk_path(
    entry: &StoreEntry,
    ticket_id: &Ulid,
    attachment_id: &Ulid,
    filename: &str,
) -> std::path::PathBuf {
    let nested = entry
        .store
        .attachment_dir(ticket_id)
        .join(attachment_id.to_string())
        .join(filename);
    if nested.is_file() {
        nested
    } else {
        entry.store.attachment_dir(ticket_id).join(filename)
    }
}

pub(crate) async fn act_on_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(request): Json<AttachmentHostActionRequest>,
) -> Result<Json<AttachmentHostActionResponse>, ApiError> {
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, _) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    if request.action != AttachmentHostAction::Path {
        repository_browser::act_on_host_path(
            &path,
            match request.action {
                AttachmentHostAction::Open => repository_browser::RepositoryFileAction::Open,
                AttachmentHostAction::Reveal => repository_browser::RepositoryFileAction::Reveal,
                AttachmentHostAction::Path => unreachable!(),
            },
        )
        .map_err(repository_browser_api_error)?;
    }
    Ok(Json(AttachmentHostActionResponse {
        path: path.display().to_string(),
    }))
}

pub(crate) async fn act_on_checkout_ticket_attachment_by_name(
    State(state): State<AppState>,
    Path((reference, id, filename)): Path<(String, String, String)>,
    Json(request): Json<AttachmentHostActionRequest>,
) -> Result<Json<AttachmentHostActionResponse>, ApiError> {
    let (entry, ticket, attachment_id) =
        checkout_attachment_by_name(&state, &reference, &id, &filename)?;
    let (attachment, _) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    if request.action != AttachmentHostAction::Path {
        repository_browser::act_on_host_path(
            &path,
            match request.action {
                AttachmentHostAction::Open => repository_browser::RepositoryFileAction::Open,
                AttachmentHostAction::Reveal => repository_browser::RepositoryFileAction::Reveal,
                AttachmentHostAction::Path => unreachable!(),
            },
        )
        .map_err(repository_browser_api_error)?;
    }
    Ok(Json(AttachmentHostActionResponse {
        path: path.display().to_string(),
    }))
}

pub(crate) async fn delete_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let updated = entry
        .store
        .remove_attachment(&ticket.id, &attachment_id, now())
        .map_err(|error| {
            if error.is_io_kind(std::io::ErrorKind::NotFound) {
                ApiError::not_found(&attachment_id.to_string())
            } else {
                error.into()
            }
        })?;
    state.changed_in(&entry, "attachment_removed", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateAttachmentMetadataBody {
    pub(crate) attachment_ids: Vec<String>,
    #[serde(flatten)]
    pub(crate) metadata: hotsheet_model::AttachmentMetadata,
}

pub(crate) async fn update_checkout_ticket_attachment_metadata(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(body): Json<UpdateAttachmentMetadataBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    if body.attachment_ids.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "attachment_ids must not be empty",
        ));
    }
    validate_attachment_metadata(&body.metadata)?;
    let mut seen = std::collections::HashSet::new();
    let attachment_ids = body
        .attachment_ids
        .iter()
        .map(|id| {
            let parsed = Ulid::from_string(id)
                .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid attachment ULID"))?;
            if !seen.insert(parsed) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "attachment_ids must be unique",
                ));
            }
            Ok(parsed)
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let updated =
        entry
            .store
            .set_attachment_metadata(&ticket.id, &attachment_ids, body.metadata, now())?;
    if updated.attachments != ticket.attachments {
        state.changed_in(&entry, "attachment_metadata_updated", &updated);
    }
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct RenameAttachmentBody {
    pub(crate) filename: String,
}

pub(crate) async fn rename_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<RenameAttachmentBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    if body.filename.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "filename is required",
        ));
    }
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id = Ulid::from_string(&attachment_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid attachment ULID"))?;
    let updated =
        entry
            .store
            .rename_attachment(&ticket.id, &attachment_id, now(), &body.filename)?;
    if updated.attachments != ticket.attachments {
        state.changed_in(&entry, "attachment_renamed", &updated);
    }
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateAttachmentAnnotationsBody {
    pub(crate) annotations: Vec<hotsheet_model::MediaAnnotation>,
    #[serde(default)]
    pub(crate) actor: Option<ActorReq>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateAttachmentMarkupBody {
    pub(crate) annotations: Vec<hotsheet_model::MediaAnnotation>,
    pub(crate) crop: Option<hotsheet_model::ImageCrop>,
    #[serde(default)]
    pub(crate) expected_revision: Option<String>,
    #[serde(default)]
    pub(crate) actor: Option<ActorReq>,
}

pub(crate) async fn update_checkout_ticket_attachment_markup(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<UpdateAttachmentMarkupBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        let provider = provider_for(&state, &source.connection_id)?;
        let updated = provider
            .set_attachment_markup(
                &native_id,
                &attachment_id,
                body.expected_revision.as_deref(),
                hotsheet_ticketing::store::AttachmentMarkup {
                    annotations: body.annotations,
                    crop: body.crop,
                },
            )
            .map_err(provider_transfer_error)?;
        state.emit(ChangeEvent {
            cursor: None,
            store: source.connection_id.clone(),
            kind: "attachment_markup_updated".into(),
            id: native_id.clone(),
            slug: updated.slug.clone(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
        return Ok(Json(ResolvedTicket {
            store: source.connection_id,
            ticket: contextualize_api_ticket(updated, &settings)?,
        }));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id = Ulid::from_string(&attachment_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid attachment ULID"))?;
    let actor = parse_actor(body.actor.as_ref())?;
    let updated = entry.store.set_attachment_markup_with_activity(
        &ticket.id,
        &attachment_id,
        hotsheet_ticketing::store::AttachmentMarkup {
            annotations: body.annotations,
            crop: body.crop,
        },
        hotsheet_ticketing::actor::note_actor(actor.as_ref()),
        Ulid::new(),
        now(),
    )?;
    state.changed_in(&entry, "attachment_markup_updated", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

pub(crate) async fn update_ticket_attachment_annotations(
    State(state): State<AppState>,
    Path((id, attachment_id)): Path<(String, String)>,
    Json(body): Json<UpdateAttachmentAnnotationsBody>,
) -> Result<Json<ApiTicket>, ApiError> {
    let ticket = ops::resolve(&state.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let actor = parse_actor(body.actor.as_ref())?;
    let updated = state.store.set_attachment_annotations_with_activity(
        &ticket.id,
        &attachment_id,
        body.annotations,
        hotsheet_ticketing::actor::note_actor(actor.as_ref()),
        Ulid::new(),
        now(),
    )?;
    state.changed_in(
        &state.default_entry(),
        "attachment_annotations_updated",
        &updated,
    );
    Ok(Json(api_ticket(&state.default_entry(), &updated)?))
}

pub(crate) async fn update_checkout_ticket_attachment_annotations(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<UpdateAttachmentAnnotationsBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let actor = parse_actor(body.actor.as_ref())?;
    let updated = entry
        .store
        .set_attachment_annotations_with_activity(
            &ticket.id,
            &attachment_id,
            body.annotations,
            hotsheet_ticketing::actor::note_actor(actor.as_ref()),
            Ulid::new(),
            now(),
        )
        .map_err(ApiError::from)?;
    state.changed_in(&entry, "attachment_annotations_updated", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}
