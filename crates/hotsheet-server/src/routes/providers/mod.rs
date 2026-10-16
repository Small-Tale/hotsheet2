//! Ticket-provider connection and provider-ticket routes.

mod tickets;
pub(crate) use tickets::{
    assign_provider_ticket, close_provider_ticket, create_provider_ticket,
    delete_provider_ticket_note, do_provider_create, do_provider_update, do_rate_ai_feedback,
    get_provider_ticket, list_provider_ai_feedback, list_provider_tickets, provider_copy_route,
    provider_move_route, provider_timing_headers, report_provider_ticket_not_working,
    report_provider_ticket_not_working_json, restore_provider_ticket, update_provider_ticket,
};

use crate::api_error::ApiError;
use crate::routes::accounts::account_listing_inputs;
use crate::routes::checkouts::checkout_entries;
use crate::{AppState, multistore, now, provider_overlay};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_model::Ulid;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::checkout_order::MergeKey;
use hotsheet_ticketing::wire::ApiAttachment;
use hotsheet_ticketing::{
    ApiTicket, FsStore, GitProvider, ProviderConfigRegistry, ProviderConnection, ProviderRegistry,
    TicketQuery, TicketRef,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use time::OffsetDateTime;

/// `GET /providers` — capability-bearing ticket-provider connections. The current
/// implementation registers every hosted store as a built-in git provider.
pub(crate) async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<hotsheet_ticketing::ProviderDescriptor>>, ApiError> {
    let default_id = multistore::store_url_id(&state.store);
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    let external_default = connections.iter().any(|connection| connection.default)
        || state
            .injected_providers
            .descriptors()
            .iter()
            .any(|descriptor| descriptor.default);
    // Store metadata only — never a ticket parse (HS2-4XXRJP).
    let mut descriptors = state
        .host
        .summaries()
        .into_iter()
        .map(|info| {
            let is_default = info.id == default_id && !external_default;
            info.provider_descriptor(is_default)
        })
        .collect::<Vec<_>>();
    for connection in connections {
        if connection.provider != "git" {
            descriptors
                .push(hotsheet_extsync::descriptor(&connection).map_err(provider_transfer_error)?);
        }
    }
    descriptors.extend(state.injected_providers.descriptors());
    descriptors.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
    Ok(Json(descriptors))
}

/// `GET /checkouts/{reference}/providers` (HS2-3SCH1K): only the ticket sources this checkout
/// links, in its source order, marked default by the checkout's own default source rather than
/// the machine-wide registry. A linked connection that no longer exists is omitted.
pub(crate) async fn list_checkout_providers(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<hotsheet_ticketing::ProviderDescriptor>>, ApiError> {
    let checkout = state
        .checkout_registry
        .describe(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    // A legacy source without a recorded identity must remain visible for review.
    // Do not host it, or read any of its tickets, while constructing settings metadata.
    let has_unreviewed = checkout.sources.iter().any(|source| {
        source.provider == "git"
            && !checkout
                .store_instance_ids
                .contains_key(&source.connection_id)
    });
    // Settings inspect only identity metadata. Checkout resolution would reject the whole
    // checkout for a changed, missing, or unreadable store, leaving no repair controls.
    let mut identity_mismatches = std::collections::HashSet::new();
    let mut unavailable_stores = std::collections::HashSet::new();
    for source in checkout
        .sources
        .iter()
        .filter(|source| source.provider == "git")
    {
        let Some(expected) = checkout.store_instance_ids.get(&source.connection_id) else {
            continue;
        };
        match FsStore::open_without_maintenance(&source.locator).and_then(|store| store.metadata())
        {
            Ok(metadata) if metadata.instance_id.as_deref() != Some(expected) => {
                identity_mismatches.insert(source.connection_id.clone());
            }
            Err(_) => {
                unavailable_stores.insert(source.connection_id.clone());
            }
            _ => {}
        }
    }
    let metadata_only =
        has_unreviewed || !identity_mismatches.is_empty() || !unavailable_stores.is_empty();
    let hosted = if metadata_only {
        std::collections::HashSet::new()
    } else {
        checkout_entries(&state, &reference)?
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    };
    let summaries = state.host.summaries();
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    let injected = state.injected_providers.descriptors();
    let mut descriptors = Vec::new();
    for source in &checkout.sources {
        let is_default = checkout.default_source.as_deref() == Some(source.connection_id.as_str());
        let descriptor = if source.provider == "git" {
            summaries
                .iter()
                .find(|info| info.id == source.connection_id && hosted.contains(&info.id))
                .map(|info| info.provider_descriptor(is_default))
                .or_else(|| {
                    metadata_only.then(|| hotsheet_ticketing::ProviderDescriptor {
                        connection_id: source.connection_id.clone(),
                        provider: "git".into(),
                        display_name: "Git tickets".into(),
                        locator: source.locator.clone(),
                        default: is_default,
                        color: None,
                        unverified_recovery: false,
                        identity_review_required: false,
                        identity_mismatch: false,
                        store_unavailable: false,
                        capabilities: hotsheet_ticketing::ProviderCapabilities::git(),
                    })
                })
        } else if let Some(connection) = connections
            .iter()
            .find(|connection| connection.id == source.connection_id)
        {
            Some(hotsheet_extsync::descriptor(connection).map_err(provider_transfer_error)?)
        } else {
            injected
                .iter()
                .find(|descriptor| descriptor.connection_id == source.connection_id)
                .cloned()
        };
        if let Some(mut descriptor) = descriptor {
            descriptor.default = is_default;
            descriptor.unverified_recovery = checkout
                .unverified_store_sources
                .contains_key(&source.connection_id);
            descriptor.identity_review_required = source.provider == "git"
                && !checkout
                    .store_instance_ids
                    .contains_key(&source.connection_id);
            descriptor.identity_mismatch = identity_mismatches.contains(&source.connection_id);
            descriptor.store_unavailable = unavailable_stores.contains(&source.connection_id);
            descriptor.color = Some(
                hotsheet_ticketing::checkouts::effective_source_color(
                    checkout
                        .source_colors
                        .get(&source.connection_id)
                        .map(String::as_str),
                )
                .to_owned(),
            );
            descriptors.push(descriptor);
        }
    }
    Ok(Json(descriptors))
}

/// Project-scoped, non-secret provider configuration. Credential values stay in the
/// server's key registry; this surface only carries a credential reference.
pub(crate) async fn list_provider_connections(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProviderConnection>>, ApiError> {
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map(Json)
        .map_err(provider_transfer_error)
}

fn save_provider_connections(
    state: &AppState,
    connections: Vec<ProviderConnection>,
    connection: ProviderConnection,
    replacing: Option<&str>,
) -> Result<(), ApiError> {
    let connections = hotsheet_extsync::updated_connections(connections, connection, replacing)
        .map_err(|error| match error {
            hotsheet_ticketing::ProviderError::UnknownConnection(id) => ApiError::not_found(&id),
            hotsheet_ticketing::ProviderError::Conflict { message, .. }
                if message == "provider connection id already exists" =>
            {
                ApiError::new(StatusCode::CONFLICT, message)
            }
            hotsheet_ticketing::ProviderError::Conflict { message, .. }
                if message == "git connections are managed through the store registry" =>
            {
                ApiError::new(StatusCode::BAD_REQUEST, message)
            }
            other => provider_transfer_error(other),
        })?;
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .save(&connections)
        .map_err(provider_transfer_error)
}

pub(crate) fn connection_token(
    state: &AppState,
    connection: &ProviderConnection,
) -> Result<String, ApiError> {
    hotsheet_extsync::connection_access_token(
        connection,
        &state.key_registry(),
        OffsetDateTime::now_utc().unix_timestamp(),
    )
    .map_err(|error| match error {
        hotsheet_extsync::GitHubCredentialError::Provider(error) => provider_transfer_error(error),
        hotsheet_extsync::GitHubCredentialError::Secret(error) => provider_transfer_error(error),
        other => ApiError::new(StatusCode::UNAUTHORIZED, other.to_string()),
    })
}

pub(crate) async fn create_provider_connection(
    State(state): State<AppState>,
    Json(mut connection): Json<ProviderConnection>,
) -> Result<(StatusCode, Json<ProviderConnection>), ApiError> {
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    // Clients no longer ask users for an id; an empty one is generated (HS2-48GA17).
    if connection.id.trim().is_empty() {
        connection.id = hotsheet_ticketing::generate_connection_id(
            &connections,
            &connection.provider,
            &connection.locator,
        );
    }
    fill_reused_sign_in_endpoint(&state, &mut connection);
    save_provider_connections(&state, connections, connection.clone(), None)?;
    Ok((StatusCode::CREATED, Json(connection)))
}

/// A new GitHub source reusing a GitHub Enterprise sign-in gets that site's `api_base` even
/// when the client did not send one (HS2-16MYXN). Best effort: an unreadable key registry
/// leaves the connection as sent.
fn fill_reused_sign_in_endpoint(state: &AppState, connection: &mut ProviderConnection) {
    if connection.provider != "github" {
        return;
    }
    // keys.json metadata only: creating a source never reads the keychain (the listing that
    // offered the account already backfilled any older sign-in's site).
    if let Ok(credentials) = state.key_registry().list() {
        hotsheet_ticketing::accounts::fill_reused_github_api_base(connection, &credentials);
    }
}

pub(crate) async fn update_provider_connection(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(connection): Json<ProviderConnection>,
) -> Result<Json<ProviderConnection>, ApiError> {
    update_connection_record(&state, connection_id, connection)
}

/// A connection as one project sees it (HS2-SM9PM8): its record plus every project that
/// owns it, so a source shared through `hotsheet checkout add-source` says so.
#[derive(Debug, Serialize)]
pub(crate) struct ProjectConnection {
    #[serde(flatten)]
    connection: ProviderConnection,
    projects: Vec<hotsheet_ticketing::accounts::AccountProject>,
}

fn project_connection(
    connection: ProviderConnection,
    checkouts: &[hotsheet_ticketing::checkouts::Checkout],
) -> ProjectConnection {
    let projects = hotsheet_ticketing::accounts::connection_projects(checkouts, &connection.id);
    ProjectConnection {
        connection,
        projects,
    }
}

/// The checkout `reference`, when it links `connection_id`; otherwise 404, so a project can
/// never read or change another project's ticket source through its own routes.
fn checkout_linking(
    state: &AppState,
    reference: &str,
    connection_id: &str,
) -> Result<hotsheet_ticketing::checkouts::Checkout, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    if checkout.source(connection_id).is_none() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            format!("this project has no ticket source '{connection_id}'"),
        ));
    }
    Ok(checkout)
}

/// `GET /checkouts/{reference}/provider-connections`: only this project's connections.
pub(crate) async fn list_checkout_provider_connections(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<ProjectConnection>>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    let (connections, checkouts) = account_listing_inputs(&state)?;
    Ok(Json(
        connections
            .into_iter()
            .filter(|connection| checkout.source(&connection.id).is_some())
            .map(|connection| project_connection(connection, &checkouts))
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateCheckoutConnectionBody {
    #[serde(flatten)]
    connection: ProviderConnection,
    #[serde(default)]
    make_default: bool,
}

/// `POST /checkouts/{reference}/provider-connections`: create a ticket source owned by this
/// project — the connection record and the checkout link in one request (HS2-SM9PM8). If
/// the link cannot be written the new record is removed again, so no ownerless catalog
/// entry is left behind.
pub(crate) async fn create_checkout_provider_connection(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(body): Json<CreateCheckoutConnectionBody>,
) -> Result<(StatusCode, Json<ProjectConnection>), ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    let mut connection = body.connection;
    let registry = ProviderConfigRegistry::new(state.store.root().join("providers.json"));
    let connections = registry.load().map_err(provider_transfer_error)?;
    if connection.id.trim().is_empty() {
        connection.id = hotsheet_ticketing::generate_connection_id(
            &connections,
            &connection.provider,
            &connection.locator,
        );
    }
    // A project's default lives on its checkout, never on the shared record.
    connection.default = false;
    fill_reused_sign_in_endpoint(&state, &mut connection);
    save_provider_connections(&state, connections, connection.clone(), None)?;
    let linked = state.checkout_registry.add_registered_source(
        &registry,
        &checkout.id,
        hotsheet_ticketing::checkouts::TicketSource {
            connection_id: connection.id.clone(),
            provider: connection.provider.clone(),
            locator: connection.locator.clone(),
        },
        body.make_default,
    );
    if let Err(error) = linked {
        if let Ok(mut current) = registry.load() {
            current.retain(|item| item.id != connection.id);
            if let Err(error) = registry.save(&current) {
                tracing::warn!(connection = %connection.id, %error, "rolling back a failed provider link failed");
            }
        }
        return Err(ApiError::new(StatusCode::BAD_REQUEST, error.to_string()));
    }
    let (_, checkouts) = account_listing_inputs(&state)?;
    Ok((
        StatusCode::CREATED,
        Json(project_connection(connection, &checkouts)),
    ))
}

pub(crate) async fn update_checkout_provider_connection(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
    Json(connection): Json<ProviderConnection>,
) -> Result<Json<ProviderConnection>, ApiError> {
    checkout_linking(&state, &reference, &connection_id)?;
    update_connection_record(&state, connection_id, connection)
}

pub(crate) async fn set_checkout_provider_connection_disabled(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
    Json(body): Json<ProviderConnectionDisabledBody>,
) -> Result<Json<ProviderConnection>, ApiError> {
    checkout_linking(&state, &reference, &connection_id)?;
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .set_disabled(&connection_id, body.disabled)
        .map_err(provider_transfer_error)?
        .map(Json)
        .ok_or_else(|| ApiError::not_found(&connection_id))
}

fn update_connection_record(
    state: &AppState,
    connection_id: String,
    mut connection: ProviderConnection,
) -> Result<Json<ProviderConnection>, ApiError> {
    connection.id = connection_id.clone();
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    // Only the dedicated toggle changes whether a connection is disabled (HS2-SF6W34); an
    // ordinary settings edit keeps it as it was.
    connection.disabled = connections
        .iter()
        .any(|existing| existing.id == connection_id && existing.disabled);
    save_provider_connections(state, connections, connection.clone(), Some(&connection_id))?;
    forget_live_provider(state, &connection_id);
    // Checkout links copy the locator; an edit reaches every project that shares the
    // connection (HS2-RCBKA3).
    state
        .checkout_registry
        .update_source_locator(&connection_id, &connection.locator)
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(connection))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderConnectionDisabledBody {
    disabled: bool,
}

/// Temporarily switch a connection off (or back on). While disabled, Hot Sheet neither reads
/// from nor writes to it: aggregate views skip it and direct operations fail explicitly
/// (HS2-SF6W34).
pub(crate) async fn set_provider_connection_disabled(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(body): Json<ProviderConnectionDisabledBody>,
) -> Result<Json<ProviderConnection>, ApiError> {
    let connection = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .set_disabled(&connection_id, body.disabled)
        .map_err(provider_transfer_error)?
        .ok_or_else(|| ApiError::not_found(&connection_id))?;
    forget_live_provider(&state, &connection_id);
    Ok(Json(connection))
}

/// Whether `connection_id` names a disabled external connection in `providers.json`.
pub(crate) fn connection_disabled(state: &AppState, connection_id: &str) -> Result<bool, ApiError> {
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .disabled(connection_id)
        .map_err(provider_transfer_error)
}

/// Permanently remove a connection from every project: checkout links and defaults and the
/// `providers.json` entry. Its account credential stays signed in (HS2-SM9PM8). Idempotent —
/// removing an already-removed id still cleans dangling checkout links (HS2-724S9N).
pub(crate) async fn delete_provider_connection(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<hotsheet_ticketing::connection_removal::ConnectionRemoval>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let removed = hotsheet_ticketing::connection_removal::remove_provider_connection(
            &ProviderConfigRegistry::new(state.store.root().join("providers.json")),
            &state.checkout_registry,
            &connection_id,
        )
        .map_err(|error| match error {
            hotsheet_ticketing::connection_removal::ConnectionRemovalError::GitSource(_) => {
                ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
            }
            hotsheet_ticketing::connection_removal::ConnectionRemovalError::Provider(error) => {
                provider_transfer_error(error)
            }
            other => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        })?;
        forget_live_provider(&state, &connection_id);
        Ok(Json(removed))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderTransferBody {
    source: TicketRef,
    destination_connection: String,
    operation_id: String,
    #[serde(default)]
    confirm: bool,
}

pub(crate) fn hosted_provider_registry(state: &AppState) -> Result<ProviderRegistry, ApiError> {
    let registry = ProviderRegistry::default();
    let default_id = multistore::store_url_id(&state.store);
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    let external_default = connections.iter().any(|connection| connection.default)
        || state
            .injected_providers
            .descriptors()
            .iter()
            .any(|descriptor| descriptor.default);
    for (id, _) in state.host.locations() {
        let entry = state
            .host
            .get(&id)
            .ok_or_else(|| ApiError::not_found(&id))?;
        registry
            .register(Arc::new(
                GitProvider::new(id.clone(), entry.store.clone())
                    .with_resolver(entry.resolver())
                    .with_default(id == default_id && !external_default),
            ))
            .map_err(provider_transfer_error)?;
    }
    for connection in connections {
        if connection.provider == "git" {
            continue;
        }
        let token = connection_token(state, &connection)?;
        registry
            .register(
                hotsheet_extsync::live_provider(&connection, token)
                    .map_err(provider_transfer_error)?,
            )
            .map_err(provider_transfer_error)?;
    }
    for descriptor in state.injected_providers.descriptors() {
        registry
            .register(
                state
                    .injected_providers
                    .get(&descriptor.connection_id)
                    .map_err(provider_transfer_error)?,
            )
            .map_err(provider_transfer_error)?;
    }
    Ok(registry)
}

pub(crate) fn provider_for(
    state: &AppState,
    connection_id: &str,
) -> Result<Arc<dyn hotsheet_ticketing::TicketProvider>, ApiError> {
    if let Some(entry) = state.host.get(connection_id) {
        return Ok(Arc::new(
            GitProvider::new(connection_id, entry.store.clone()).with_resolver(entry.resolver()),
        ));
    }
    // Refuse before building a client, so a disabled source is never contacted (HS2-SF6W34).
    if connection_disabled(state, connection_id)? {
        return Err(provider_transfer_error(
            hotsheet_ticketing::ProviderError::Disabled {
                connection_id: connection_id.into(),
            },
        ));
    }
    if let Ok(provider) = state.injected_providers.get(connection_id) {
        return Ok(provider);
    }
    let connection = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?
        .into_iter()
        .find(|connection| connection.id == connection_id)
        .ok_or_else(|| ApiError::not_found(connection_id))?;
    let token = connection_token(state, &connection)?;
    use sha2::{Digest, Sha256};
    let mut fingerprint = Sha256::new();
    fingerprint.update(serde_json::to_vec(&connection).map_err(provider_transfer_error)?);
    fingerprint.update(token.as_bytes());
    let fingerprint = format!("{:x}", fingerprint.finalize());
    let mut cached = state.live_providers.lock_or_recover();
    if let Some((key, provider)) = cached.get(connection_id) {
        if key == &fingerprint {
            return Ok(provider.clone());
        }
    }
    let provider =
        hotsheet_extsync::live_provider(&connection, token).map_err(provider_transfer_error)?;
    cached.insert(connection_id.into(), (fingerprint, provider.clone()));
    Ok(provider)
}

pub(crate) fn provider_read_get(
    state: &AppState,
    connection_id: &str,
    id: &str,
) -> Result<ApiTicket, ApiError> {
    let provider = provider_for(state, connection_id)?;
    if provider.descriptor().capabilities.write_behind {
        if let Some(outbox) = &state.jira_outbox {
            let guard = outbox.lock_or_recover();
            return provider_overlay::get(provider.as_ref(), &guard, connection_id, id)
                .map_err(provider_transfer_error);
        }
    }
    provider.get(id).map_err(provider_transfer_error)
}

pub(crate) fn provider_read_query(
    state: &AppState,
    connection_id: &str,
    query: &TicketQuery,
) -> Result<Vec<ApiTicket>, ApiError> {
    let provider = provider_for(state, connection_id)?;
    if provider.descriptor().capabilities.write_behind {
        if let Some(outbox) = &state.jira_outbox {
            let guard = outbox.lock_or_recover();
            return provider_overlay::query(provider.as_ref(), &guard, connection_id, query)
                .map_err(provider_transfer_error);
        }
    }
    provider.query(query).map_err(provider_transfer_error)
}

pub(crate) fn provider_read_query_after(
    state: &AppState,
    connection_id: &str,
    query: &TicketQuery,
    after: Option<&MergeKey>,
    resume: Option<&str>,
    limit: usize,
) -> Result<hotsheet_ticketing::ProviderKeysetPage, ApiError> {
    let provider = provider_for(state, connection_id)?;
    if provider.descriptor().capabilities.write_behind {
        if let Some(outbox) = &state.jira_outbox {
            let guard = outbox.lock_or_recover();
            return provider_overlay::query_after(
                provider.as_ref(),
                &guard,
                connection_id,
                query,
                after,
                resume,
                limit,
            )
            .map_err(provider_transfer_error);
        }
    }
    provider
        .query_after(query, after, resume, limit)
        .map_err(provider_transfer_error)
}

fn forget_live_provider(state: &AppState, connection_id: &str) {
    {
        let mut providers = state.live_providers.lock_or_recover();
        providers.remove(connection_id);
    }
}

#[derive(Debug, Deserialize)]
struct AttachmentCopyRef {
    connection_id: String,
    native_id: String,
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
struct AttachmentTicketRef {
    connection_id: String,
    native_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CopyAttachmentReq {
    source: AttachmentCopyRef,
    destination: AttachmentTicketRef,
}

pub(crate) async fn copy_provider_attachment(
    State(state): State<AppState>,
    Json(req): Json<CopyAttachmentReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let source = provider_for(&state, &req.source.connection_id)?;
    let destination = provider_for(&state, &req.destination.connection_id)?;
    let source_ticket = source
        .get(&req.source.native_id)
        .map_err(provider_transfer_error)?;
    let metadata = source_ticket
        .attachments
        .into_iter()
        .find(|item| item.id == req.source.attachment_id)
        .ok_or_else(|| ApiError::not_found(&req.source.attachment_id))?;
    if metadata.crop.is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "cropped attachments cannot be transferred until the destination preserves the crop",
        ));
    }
    let bytes = source
        .attachment_bytes(&req.source.native_id, &req.source.attachment_id)
        .map_err(provider_transfer_error)?;
    let copied = destination
        .add_attachment(
            &req.destination.native_id,
            ApiAttachment {
                id: Ulid::new().to_string(),
                filename: metadata.filename,
                created_at: now().to_string(),
                // A copied standalone file is intentionally Uncategorized: retaining its
                // source batch id could falsely merge it with an unrelated destination batch.
                batch_id: None,
                batch_label: None,
                actor: None,
                purpose: None,
                annotations: metadata.annotations,
                crop: None,
                revision: None,
            },
            bytes,
        )
        .map_err(provider_transfer_error)?;
    Ok((StatusCode::CREATED, Json(copied)))
}

pub(crate) fn provider_transfer_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, error.to_string())
}
