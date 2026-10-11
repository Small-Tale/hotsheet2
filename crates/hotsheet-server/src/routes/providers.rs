//! Ticket-provider connection and provider-ticket routes.

use crate::*;

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

pub(crate) fn save_provider_connections(
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
pub(crate) fn fill_reused_sign_in_endpoint(state: &AppState, connection: &mut ProviderConnection) {
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
    pub(crate) connection: ProviderConnection,
    pub(crate) projects: Vec<hotsheet_ticketing::accounts::AccountProject>,
}

pub(crate) fn project_connection(
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
pub(crate) fn checkout_linking(
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
    pub(crate) connection: ProviderConnection,
    #[serde(default)]
    pub(crate) make_default: bool,
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

pub(crate) fn update_connection_record(
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
    pub(crate) disabled: bool,
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
    pub(crate) source: TicketRef,
    pub(crate) destination_connection: String,
    pub(crate) operation_id: String,
    #[serde(default)]
    pub(crate) confirm: bool,
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
                GitProvider::new(id.clone(), entry.store)
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
        return Ok(Arc::new(GitProvider::new(connection_id, entry.store)));
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

pub(crate) fn forget_live_provider(state: &AppState, connection_id: &str) {
    {
        let mut providers = state.live_providers.lock_or_recover();
        providers.remove(connection_id);
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AttachmentCopyRef {
    pub(crate) connection_id: String,
    pub(crate) native_id: String,
    pub(crate) attachment_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AttachmentTicketRef {
    pub(crate) connection_id: String,
    pub(crate) native_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CopyAttachmentReq {
    pub(crate) source: AttachmentCopyRef,
    pub(crate) destination: AttachmentTicketRef,
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

pub(crate) fn parse_ai_feedback(
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
pub(crate) fn reindex_hosted_provider_write(
    state: &AppState,
    connection_id: &str,
    native_id: &str,
) {
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
    pub(crate) note: Option<String>,
    pub(crate) expected_token: Option<String>,
    #[serde(default)]
    pub(crate) evidence: Vec<NotWorkingJsonEvidence>,
}

#[derive(Deserialize)]
pub(crate) struct NotWorkingJsonEvidence {
    pub(crate) filename: String,
    pub(crate) content_base64: String,
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
