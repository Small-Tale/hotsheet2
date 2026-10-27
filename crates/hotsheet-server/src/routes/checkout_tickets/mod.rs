//! Checkout-scoped ticket CRUD and attachment routes.

mod attachments;
pub(crate) use attachments::{
    act_on_checkout_ticket_attachment, act_on_checkout_ticket_attachment_by_name,
    add_checkout_ticket_attachment, checkout_git_ticket, delete_checkout_ticket_attachment,
    get_checkout_ticket_attachment, get_checkout_ticket_attachment_by_name,
    get_checkout_ticket_attachment_original, get_checkout_ticket_attachment_thumbnail,
    put_checkout_ticket_attachment_thumbnail, rename_checkout_ticket_attachment,
    update_checkout_ticket_attachment_annotations, update_checkout_ticket_attachment_markup,
    update_checkout_ticket_attachment_metadata, update_ticket_attachment_annotations,
};

use crate::api_error::{ActorReq, ApiError};
use crate::dto::{CloseReq, CreateReq, DuplicateOfReq, UpdateReq};
use crate::multistore::StoreEntry;
use crate::routes::checkouts::{checkout_entries, checkout_lookup_error};
use crate::routes::project_tools::checkout_settings;
use crate::routes::providers::{
    connection_disabled, do_provider_create, do_provider_update, provider_for,
    provider_timing_headers, provider_transfer_error,
};
use crate::routes::store_scoped::ResolvedTicket;
use crate::routes::ticket_writes::{
    AssignReq, do_assign, do_close, do_create, do_update, do_update_with_ticket,
};
use crate::routes::tickets::{api_ticket, api_ticket_with_settings, contextualize_api_ticket};
use crate::{AppState, multistore, now, opt_parse, provider_overlay};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use hotsheet_model::{CloseReason, Ticket, Ulid};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{
    ApiTicket, FsStore, ProjectTicketRef, STORE_METADATA_FILE, TicketQuery, ops,
};
use serde::{Deserialize, Serialize};
use std::path::Path as FsPath;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
pub(crate) struct CheckoutStoreQuery {
    store: Option<String>,
    source: Option<String>,
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

/// The sources an unqualified ticket id can belong to, each with the native id to ask it
/// for. Local git sources come first and always qualify. A remote provider qualifies only
/// when `id` has that provider's native shape (an issue number, or a Jira key of its
/// project), so a git slug or ULID never costs a third-party network call (HS2-FXGKBD).
fn unqualified_candidates(
    sources: Vec<hotsheet_ticketing::checkouts::TicketSource>,
    id: &str,
) -> Vec<(hotsheet_ticketing::checkouts::TicketSource, String)> {
    let (local, remote): (Vec<_>, Vec<_>) = sources
        .into_iter()
        .partition(|source| source.provider == "git");
    local
        .into_iter()
        .map(|source| (source, id.to_string()))
        .chain(remote.into_iter().filter_map(|source| {
            let native_id =
                hotsheet_extsync::unqualified_native_id(&source.provider, &source.locator, id)?;
            Some((source, native_id))
        }))
        .collect()
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
    for (source, native_id) in unqualified_candidates(checkout.sources, id) {
        let exists = if source.provider == "git" {
            state
                .host
                .get(&source.connection_id)
                .map(|entry| entry.resolve(&native_id))
                .transpose()?
                .flatten()
                .is_some()
        } else {
            match probe_provider_source(state, &source.connection_id, &native_id) {
                Ok(ticket) => ticket.is_some(),
                // A source that could not answer only matters when no other source owns the id.
                Err(error) => {
                    unavailable.get_or_insert(error);
                    false
                }
            }
        };
        if exists {
            found.push((source, native_id));
        }
    }
    match found.as_slice() {
        [(source, native_id)] => Ok((source.clone(), native_id.clone())),
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
        entry
            .resolve(&reference.native_id)?
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
fn resolve_checkout_ticket(
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
    for (source, native_id) in unqualified_candidates(checkout.sources.clone(), id) {
        // A source that could not answer only matters when no other source owns the id.
        let ticket = match read_checkout_ticket_source(state, &source, &native_id, false) {
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
            found = Some((source, ticket));
        }
    }
    let (source, ticket) =
        found.ok_or_else(|| unavailable.unwrap_or_else(|| ApiError::not_found(id)))?;
    Ok((checkout, source, ticket))
}

fn read_checkout_ticket_source(
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
        let Some(ticket) = entry.resolve(id)? else {
            return Ok(None);
        };
        let store = multistore::store_url_id(&entry.store);
        let mut api = ApiTicket::from_provider(&ticket, &store, None);
        crate::routes::tickets::apply_detail_phase_estimate(&entry, &ticket, &mut api)?;
        return Ok(Some(ResolvedTicket { ticket: api, store }));
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
fn probe_provider_source(
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
    reference: String,
    project_id: String,
    project_name: String,
    connection_id: String,
    native_id: String,
    qualified_id: String,
    slug: String,
    title: String,
}

#[derive(Debug, Serialize)]
struct DuplicateBacklinkProject {
    project_id: String,
    project_name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DuplicateBacklinkResponse {
    pub(crate) backlinks: Vec<DuplicateBacklink>,
    inaccessible_projects: Vec<DuplicateBacklinkProject>,
}

fn duplicate_reference_matches(reference: &str, target: &ProjectTicketRef) -> bool {
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
struct CheckoutBatchUpdateReq {
    id: String,
    #[serde(flatten)]
    update: UpdateReq,
}

#[derive(Deserialize)]
pub(crate) struct CheckoutBatchReq {
    updates: Vec<CheckoutBatchUpdateReq>,
    /// One actor for the whole bulk operation (HS2-XF81CJ); an update naming its own wins.
    #[serde(default)]
    actor: Option<ActorReq>,
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
            return Err(ApiError::ticket_changed(format!(
                "ticket {} changed since it was read",
                item.id
            )));
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
        entry
            .store
            .queue_autocommit_paths("Update selected Hot Sheet tickets", &paths);
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

fn do_restore(state: &AppState, entry: &StoreEntry, id: &str) -> Result<ApiTicket, ApiError> {
    let ticket = entry.resolve(id)?.ok_or_else(|| ApiError::not_found(id))?;
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

#[derive(Deserialize)]
pub(crate) struct TrashPurgePreviewQuery {
    days: u32,
}

/// How many Trash tickets applying a `days` retention would purge right now, across the
/// checkout's git ticket sources (HS2-KPD942). A checkout without git Trash previews zero.
pub(crate) async fn preview_checkout_trash_purge(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(query): Query<TrashPurgePreviewQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut count = 0;
    for (_, entry) in checkout_entries(&state, &reference)? {
        count += ops::trash_due_for_purge(&entry.store, &now(), i64::from(query.days))?.len();
    }
    Ok(Json(serde_json::json!({
        "trash_cleanup_days": query.days,
        "purge_count": count,
    })))
}

/// Purge every checkout Trash ticket past `days`, announcing each removal, so a retention
/// applied from Settings takes effect now instead of on the next background sweep.
pub(crate) fn purge_checkout_trash_past(
    state: &AppState,
    reference: &str,
    days: u32,
) -> Result<usize, ApiError> {
    let mut purged = 0;
    for (_, entry) in checkout_entries(state, reference)? {
        for ticket in ops::purge_trash(&entry.store, &now(), i64::from(days))? {
            state.removed_in(&entry, &ticket);
            purged += 1;
        }
    }
    Ok(purged)
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
