//! Project / checkout registration, sources, repository, code review, and checkout ticket listing.

use crate::*;

#[derive(Debug, Deserialize)]
pub(crate) struct RegisterCheckoutBody {
    pub(crate) root: String,
    pub(crate) alias: Option<String>,
    pub(crate) repository: Option<String>,
    #[serde(default)]
    pub(crate) stores: Vec<String>,
    #[serde(default)]
    pub(crate) sources: Vec<hotsheet_ticketing::checkouts::TicketSource>,
    pub(crate) default_source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenProjectBody {
    pub(crate) root: String,
    pub(crate) alias: Option<String>,
    pub(crate) repository: Option<String>,
    /// Explicit git stores. When omitted, conservative filesystem discovery is used.
    pub(crate) stores: Option<Vec<String>>,
    /// Explicit provider-neutral sources. When supplied, these are authoritative.
    pub(crate) sources: Option<Vec<hotsheet_ticketing::checkouts::TicketSource>>,
    pub(crate) default_source: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenProjectResponse {
    pub(crate) checkout: hotsheet_ticketing::checkouts::Checkout,
    pub(crate) discovered: bool,
}

pub(crate) fn schedule_setup_freshness(
    state: &AppState,
    checkout: &hotsheet_ticketing::checkouts::Checkout,
) {
    let settings = checkout.settings();
    let source = checkout
        .default_source
        .as_deref()
        .and_then(|id| checkout.source(id))
        .filter(|source| {
            source.provider == "git"
                && checkout
                    .store_instance_ids
                    .contains_key(&source.connection_id)
        })
        .or_else(|| {
            checkout.sources.iter().find(|source| {
                source.provider == "git"
                    && checkout
                        .store_instance_ids
                        .contains_key(&source.connection_id)
            })
        });
    let Some(source) = source else {
        tokio::task::spawn_blocking(move || {
            if let Err(error) = settings.migrate_existing() {
                tracing::warn!(%error, "settings migration failed");
            }
        });
        return;
    };
    let id = checkout.id.clone();
    if !state
        .setup_refreshes
        .with_lock(|active| active.insert(id.clone()))
    {
        return;
    }
    let active = state.setup_refreshes.clone();
    let project = std::path::PathBuf::from(&checkout.root);
    let store = std::path::PathBuf::from(&source.locator);
    let plugin_dirs = state.plugin_dirs.as_ref().clone();
    tokio::task::spawn_blocking(move || {
        if let Err(error) = settings.migrate_existing() {
            tracing::warn!(%error, "settings migration failed");
        }
        // Same resolution as the CLI: an explicit empty list disables every tool (HS2-8B3VJP).
        let enabled = hotsheet_plugins::enabled_plugins_from_setting(
            settings
                .get("enabled_plugins", hotsheet_ticketing::Scope::Shared)
                .ok()
                .flatten()
                .as_ref(),
        );
        let _ =
            hotsheet_plugins::refresh_setup_in(&store, &project, enabled.as_ref(), &plugin_dirs);
        {
            let mut active = active.lock_or_recover();
            active.remove(&id);
        }
    });
}

/// Keep the generated checkout projection fresh without making project opening wait for a
/// full scan of every linked ticket store. The short delay also gives the client's initial
/// ticket-index requests priority over this best-effort local projection refresh.
pub(crate) fn regenerate_checkout_worklist_indexed(
    host: &StoreHost,
    checkout: &hotsheet_ticketing::checkouts::Checkout,
) -> anyhow::Result<usize> {
    let query = TicketQuery {
        up_next_only: true,
        open_only: true,
        ..TicketQuery::default()
    };
    let mut tickets = std::collections::BTreeMap::new();
    for source in checkout.sources.iter().filter(|source| {
        source.provider == "git"
            && checkout
                .store_instance_ids
                .contains_key(&source.connection_id)
    }) {
        let entry = host.get(&source.connection_id).ok_or_else(|| {
            anyhow::anyhow!("checkout links an unhosted store: {}", source.locator)
        })?;
        let rows = entry.index.lock_or_recover().query(&query)?;
        for row in rows {
            let id = Ulid::from_string(&row.id)?;
            if let std::collections::btree_map::Entry::Vacant(ticket) = tickets.entry(id) {
                ticket.insert(entry.store.read_ticket(&id)?);
            }
        }
    }
    Ok(
        hotsheet_ticketing::worklist::regenerate_checkout_from_tickets(
            checkout,
            &tickets.into_values().collect::<Vec<_>>(),
        )?,
    )
}

pub(crate) fn schedule_worklist_regeneration(
    state: &AppState,
    checkout: &hotsheet_ticketing::checkouts::Checkout,
) {
    let checkout = checkout.clone();
    let host = state.host.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let root = checkout.root.clone();
        match tokio::task::spawn_blocking(move || {
            regenerate_checkout_worklist_indexed(&host, &checkout)
        })
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!("worklist regenerate failed for {root}: {error}"),
            Err(error) => tracing::warn!("worklist regenerate task failed for {root}: {error}"),
        }
    });
}

/// Open a code checkout for client use: discover or accept its git ticket stores, host
/// them in this machine server, and persist the checkout-to-store links atomically from
/// the client's point of view. An empty result is valid and lets a settings UI ask the
/// user to choose one or more providers explicitly.
///
/// Store discovery (a directory walk) and hosting each git source (which opens and
/// reconciles or rebuilds its index, parsing every ticket file) run on the blocking pool,
/// so opening a project with a large store never occupies an async request thread
/// (HS2-2VBN8Y).
pub(crate) async fn open_project(
    State(state): State<AppState>,
    Json(body): Json<OpenProjectBody>,
) -> Result<(StatusCode, Json<OpenProjectResponse>), ApiError> {
    let discovered = body.stores.is_none() && body.sources.is_none();
    let source_mode = if body.sources.is_some() {
        hotsheet_ticketing::checkouts::OpenSourceMode::Explicit
    } else if body.stores.is_some() {
        hotsheet_ticketing::checkouts::OpenSourceMode::SelectedGitStore
    } else {
        hotsheet_ticketing::checkouts::OpenSourceMode::Discovered
    };
    let hosting_state = state.clone();
    let discovery_root = body.root.clone();
    let explicit_stores = body.stores;
    let explicit_sources = body.sources;
    let existing = state
        .checkout_registry
        .list()
        .map_err(checkout_lookup_error)?
        .into_iter()
        .find(|checkout| {
            checkout.root
                == FsPath::new(&body.root)
                    .canonicalize()
                    .unwrap_or_default()
                    .to_string_lossy()
        });
    let sources = tokio::task::spawn_blocking(move || {
        let stores = match explicit_stores {
            Some(paths) => paths.into_iter().map(std::path::PathBuf::from).collect(),
            None if explicit_sources.is_none() => {
                hotsheet_ticketing::checkouts::discover_ticket_stores(FsPath::new(&discovery_root))
                    .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?
            }
            None => Vec::new(),
        };
        let mut sources = explicit_sources.unwrap_or_default();
        sources.extend(
            stores
                .iter()
                .cloned()
                .map(hotsheet_ticketing::checkouts::TicketSource::git),
        );
        for source in sources.iter().filter(|source| source.provider == "git") {
            if existing.as_ref().is_some_and(|checkout| {
                checkout.source(&source.connection_id).is_some()
                    && !checkout
                        .store_instance_ids
                        .contains_key(&source.connection_id)
            }) {
                continue;
            }
            let store = FsStore::open(&source.locator)
                .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
            hosting_state.host_project_store(store)?;
        }
        Ok::<_, ApiError>(sources)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    let root = FsPath::new(&body.root);
    let checkout = state
        .checkout_registry
        .open_registered_sources(
            &ProviderConfigRegistry::new(state.store.root().join("providers.json")),
            root,
            body.alias.as_deref(),
            body.repository,
            hotsheet_ticketing::checkouts::OpenSourceSelection {
                sources,
                default_source: body.default_source,
                mode: source_mode,
            },
        )
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    state.watch_checkout_repository(&checkout);
    schedule_worklist_regeneration(&state, &checkout);
    schedule_setup_freshness(&state, &checkout);
    Ok((
        StatusCode::CREATED,
        Json(OpenProjectResponse {
            checkout,
            discovered,
        }),
    ))
}

pub(crate) async fn list_checkouts(
    State(state): State<AppState>,
) -> Result<Json<Vec<hotsheet_ticketing::checkouts::Checkout>>, ApiError> {
    state
        .checkout_registry
        .list()
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub(crate) async fn register_checkout(
    State(state): State<AppState>,
    Json(body): Json<RegisterCheckoutBody>,
) -> Result<(StatusCode, Json<hotsheet_ticketing::checkouts::Checkout>), ApiError> {
    let mut sources = body.sources;
    sources.extend(
        body.stores
            .into_iter()
            .map(hotsheet_ticketing::checkouts::TicketSource::git),
    );
    let default_source = body
        .default_source
        .or_else(|| (sources.len() == 1).then(|| sources[0].connection_id.clone()));
    let entry = state
        .checkout_registry
        .register_registered_sources(
            &ProviderConfigRegistry::new(state.store.root().join("providers.json")),
            FsPath::new(&body.root),
            body.alias.as_deref(),
            body.repository,
            sources,
            default_source,
        )
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    hotsheet_ticketing::worklist::regenerate_checkout(&entry)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state.watch_checkout_repository(&entry);
    Ok((StatusCode::CREATED, Json(entry)))
}

pub(crate) async fn resolve_checkout(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<hotsheet_ticketing::checkouts::Checkout>, ApiError> {
    state
        .checkout_registry
        .describe(&reference)
        .map(Json)
        .map_err(checkout_lookup_error)
}

#[derive(Deserialize)]
pub(crate) struct CheckoutSourceBody {
    pub(crate) provider: String,
    pub(crate) locator: String,
    #[serde(default)]
    pub(crate) make_default: bool,
}

pub(crate) async fn add_checkout_source(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
    Json(body): Json<CheckoutSourceBody>,
) -> Result<Json<hotsheet_ticketing::checkouts::Checkout>, ApiError> {
    let source = if body.provider == "git" {
        // Hosting a new store builds its index (a full ticket parse): keep it off the
        // async request threads (HS2-2VBN8Y).
        let hosting_state = state.clone();
        let locator = body.locator;
        let store = tokio::task::spawn_blocking(move || {
            let store = FsStore::open(&locator)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
            hosting_state.host_store(store.clone())?;
            Ok::<_, ApiError>(store)
        })
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
        let source = hotsheet_ticketing::checkouts::TicketSource::git(store.root());
        if source.connection_id != connection_id {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("git source id must be {}", source.connection_id),
            ));
        }
        source
    } else {
        hotsheet_ticketing::checkouts::TicketSource {
            connection_id,
            provider: body.provider,
            locator: body.locator,
        }
    };
    state
        .checkout_registry
        .add_registered_source(
            &ProviderConfigRegistry::new(state.store.root().join("providers.json")),
            &reference,
            source,
            body.make_default,
        )
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

/// Remove a ticket source from one project (HS2-SM9PM8). A connection no other project
/// links afterwards is deleted with it; its account stays signed in.
pub(crate) async fn remove_checkout_source(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
) -> Result<Json<hotsheet_ticketing::connection_removal::SourceDetach>, ApiError> {
    tokio::task::spawn_blocking(move || {
        hotsheet_ticketing::connection_removal::detach_source(
            &ProviderConfigRegistry::new(state.store.root().join("providers.json")),
            &state.checkout_registry,
            &reference,
            &connection_id,
        )
        .map(Json)
        .map_err(|error| match error {
            hotsheet_ticketing::connection_removal::ConnectionRemovalError::Checkout(
                hotsheet_ticketing::checkouts::CheckoutError::NotFound(_),
            ) => ApiError::new(StatusCode::NOT_FOUND, error.to_string()),
            hotsheet_ticketing::connection_removal::ConnectionRemovalError::Provider(error) => {
                provider_transfer_error(error)
            }
            other => ApiError::new(StatusCode::BAD_REQUEST, other.to_string()),
        })
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

#[derive(Deserialize)]
pub(crate) struct CheckoutDefaultSourceBody {
    pub(crate) connection_id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CheckoutSourceColorBody {
    pub(crate) color: String,
}

#[derive(Deserialize)]
pub(crate) struct RelinkGitSourceBody {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) review_unverified_recovery: bool,
}

#[derive(Serialize)]
pub(crate) struct RelinkGitSourceResponse {
    pub(crate) checkouts: Vec<hotsheet_ticketing::checkouts::Checkout>,
    pub(crate) connection_id: String,
}

pub(crate) async fn relink_checkout_git_source(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
    Json(body): Json<RelinkGitSourceBody>,
) -> Result<Json<RelinkGitSourceResponse>, ApiError> {
    let hosting_state = state.clone();
    let path = body.path;
    let reviewed = body.review_unverified_recovery;
    let old_id = connection_id.clone();
    let (updated, new_id) = tokio::task::spawn_blocking(move || {
        let store = FsStore::open_without_maintenance(&path).map_err(|error| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("Choose a Hot Sheet ticket repository: {error}"),
            )
        })?;
        let replacement = hotsheet_ticketing::checkouts::TicketSource::git(store.root());
        let added = hosting_state.host_project_store(store)?;
        let result = if reviewed {
            hosting_state
                .checkout_registry
                .recover_unverified_git_source(
                    &reference,
                    &old_id,
                    std::path::Path::new(&replacement.locator),
                )
        } else {
            hosting_state.checkout_registry.relink_git_source(
                &reference,
                &old_id,
                std::path::Path::new(&replacement.locator),
            )
        };
        let updated = match result {
            Ok(updated) => updated,
            Err(error) => {
                if added {
                    hosting_state.unhost_store(&replacement.connection_id);
                }
                return Err(ApiError::new(StatusCode::BAD_REQUEST, error.to_string()));
            }
        };
        Ok::<_, ApiError>((updated, replacement.connection_id))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    for checkout in &updated {
        schedule_worklist_regeneration(&state, checkout);
        schedule_setup_freshness(&state, checkout);
    }
    if !updated.is_empty() && new_id != connection_id {
        state.unhost_store(&connection_id);
    } else if new_id != connection_id {
        state.unhost_store(&new_id);
    }
    Ok(Json(RelinkGitSourceResponse {
        checkouts: updated,
        connection_id: new_id,
    }))
}

pub(crate) async fn set_checkout_source_color(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
    Json(body): Json<CheckoutSourceColorBody>,
) -> Result<Json<hotsheet_ticketing::checkouts::Checkout>, ApiError> {
    state
        .checkout_registry
        .set_source_color(&reference, &connection_id, &body.color)
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

pub(crate) async fn set_checkout_default_source(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(body): Json<CheckoutDefaultSourceBody>,
) -> Result<Json<hotsheet_ticketing::checkouts::Checkout>, ApiError> {
    state
        .checkout_registry
        .set_default_source(&reference, body.connection_id.as_deref())
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

pub(crate) async fn checkout_repository_status(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<repository_browser::RepositoryOverview>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || repository_browser::discover(&root))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository discovery task failed: {error}"),
            )
        })?
        .map(Json)
        .map_err(repository_browser_api_error)
}

pub(crate) async fn initialize_checkout_repository(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<repository_browser::RepositoryOverview>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.clone().into();
    let overview = tokio::task::spawn_blocking(move || repository_browser::initialize(&root))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository initialization task failed: {error}"),
            )
        })?
        .map_err(repository_setup_api_error)?;
    state.watch_checkout_repository(&checkout);
    state.emit(repository_change_event(&checkout.id));
    Ok(Json(overview))
}

#[derive(Deserialize)]
pub(crate) struct RepositoryRemoteBody {
    pub(crate) remote: String,
}

pub(crate) async fn configure_checkout_repository_remote(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(body): Json<RepositoryRemoteBody>,
) -> Result<Json<repository_browser::RepositoryOverview>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.clone().into();
    let overview = tokio::task::spawn_blocking(move || {
        repository_browser::configure_origin(&root, &body.remote)
    })
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("repository remote task failed: {error}"),
        )
    })?
    .map_err(repository_setup_api_error)?;
    state.emit(repository_change_event(&checkout.id));
    Ok(Json(overview))
}

pub(crate) async fn checkout_repository_files(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(query): Query<repository_browser::RepositoryFilePageQuery>,
) -> Result<
    Json<repository_browser::RepositoryPage<hotsheet_ticketing::repository_status::RepositoryFile>>,
    ApiError,
> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || repository_browser::files_page(&root, query))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository file discovery task failed: {error}"),
            )
        })?
        .map(Json)
        .map_err(repository_browser_api_error)
}

pub(crate) async fn checkout_repository_commits(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(query): Query<repository_browser::RepositoryPageQuery>,
) -> Result<Json<code_review::CodeReviewPage>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || repository_browser::commits_page(&root, query))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository commit discovery task failed: {error}"),
            )
        })?
        .map(Json)
        .map_err(repository_browser_api_error)
}

pub(crate) async fn open_checkout_repository_review(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(target): Json<code_review::ReviewTarget>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || {
        let status = hotsheet_ticketing::repository_status::snapshot(&root)?;
        code_review::launch_repository(&root, status.ahead as usize, &target)
            .map_err(repository_browser::RepositoryBrowserError::from)
    })
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("repository review launch task failed: {error}"),
        )
    })?
    .map_err(repository_browser_api_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn checkout_repository_file_action(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(request): Json<repository_browser::RepositoryFileActionRequest>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || repository_browser::act_on_file(&root, &request))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("repository file action task failed: {error}"),
            )
        })?
        .map_err(repository_browser_api_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn repository_browser_api_error(
    error: repository_browser::RepositoryBrowserError,
) -> ApiError {
    use repository_browser::RepositoryBrowserError;
    let status = match error {
        RepositoryBrowserError::UnknownFile
        | RepositoryBrowserError::UnsafePath
        | RepositoryBrowserError::MissingFile
        | RepositoryBrowserError::Review(code_review::CodeReviewError::InvalidTarget) => {
            StatusCode::BAD_REQUEST
        }
        RepositoryBrowserError::Review(code_review::CodeReviewError::DifftoolNotConfigured) => {
            StatusCode::CONFLICT
        }
        RepositoryBrowserError::Status(_)
        | RepositoryBrowserError::Review(code_review::CodeReviewError::NotRepository) => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        RepositoryBrowserError::Review(_) | RepositoryBrowserError::Launch(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    ApiError::new(status, error.to_string())
}

pub(crate) fn repository_setup_api_error(
    error: repository_browser::RepositorySetupError,
) -> ApiError {
    use repository_browser::RepositorySetupError;
    let status = match error {
        RepositorySetupError::InvalidRemote => StatusCode::BAD_REQUEST,
        RepositorySetupError::NotRepository | RepositorySetupError::OriginAlreadyConfigured(_) => {
            StatusCode::CONFLICT
        }
        RepositorySetupError::Git { .. } | RepositorySetupError::Discovery(_) => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        RepositorySetupError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.to_string())
}

pub(crate) async fn get_checkout_code_review(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<code_review::CodeReview>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let (_, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let slug = ticket.slug;
    let classification = checkout
        .settings()
        .get_effective("code_review_file_classes")
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
        .map(serde_json::from_value::<code_review::CodeReviewClassification>)
        .transpose()
        .map_err(|error| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("invalid code_review_file_classes setting: {error}"),
            )
        })?
        .unwrap_or_default();
    let root: std::path::PathBuf = checkout.root.into();
    tokio::task::spawn_blocking(move || {
        code_review::discover_with_classification(&root, &slug, &classification)
    })
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("code review discovery task failed: {error}"),
        )
    })?
    .map(Json)
    .map_err(code_review_api_error)
}

pub(crate) async fn open_checkout_code_review(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(target): Json<code_review::ReviewTarget>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    let (_, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let root: std::path::PathBuf = checkout.root.into();
    let slug = ticket.slug;
    tokio::task::spawn_blocking(move || {
        let review = code_review::discover(&root, &slug)?;
        code_review::launch(&root, &review, &target)
    })
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("code review launch task failed: {error}"),
        )
    })?
    .map_err(code_review_api_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn code_review_api_error(error: code_review::CodeReviewError) -> ApiError {
    let status = match error {
        code_review::CodeReviewError::NotRepository => StatusCode::UNPROCESSABLE_ENTITY,
        code_review::CodeReviewError::DifftoolNotConfigured => StatusCode::CONFLICT,
        code_review::CodeReviewError::InvalidTarget => StatusCode::BAD_REQUEST,
        code_review::CodeReviewError::Git(_) | code_review::CodeReviewError::Launch(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    ApiError::new(status, error.to_string())
}

pub(crate) fn checkout_lookup_error(
    error: hotsheet_ticketing::checkouts::CheckoutError,
) -> ApiError {
    use hotsheet_ticketing::checkouts::CheckoutError;
    let status = match error {
        CheckoutError::NotFound(_) => StatusCode::NOT_FOUND,
        CheckoutError::Invalid(_) | CheckoutError::Ambiguous(_) => StatusCode::CONFLICT,
        _ => StatusCode::BAD_REQUEST,
    };
    ApiError::new(status, error.to_string())
}

pub(crate) fn checkout_entries(
    state: &AppState,
    reference: &str,
) -> Result<Vec<(String, StoreEntry)>, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    let mut entries = Vec::new();
    for source in checkout
        .sources
        .into_iter()
        .filter(|source| source.provider == "git")
    {
        let store_path = source.locator;
        let canonical = FsPath::new(&store_path)
            .canonicalize()
            .unwrap_or_else(|_| store_path.clone().into());
        let find =
            || {
                state.host.locations().into_iter().find(|(_, root)| {
                    root.canonicalize().unwrap_or_else(|_| root.clone()) == canonical
                })
            };
        // A sweep may have unhosted it while the project stayed open (HS2-ARJ9J1).
        let found = find().or_else(|| {
            let store = FsStore::open(&store_path).ok()?;
            state.host_project_store(store).ok()?;
            find()
        });
        let Some((store_id, _)) = found else {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!("checkout {reference} links an unhosted store: {store_path}"),
            ));
        };
        if let Some(entry) = state.host.get(&store_id) {
            entries.push((store_id, entry));
        }
    }
    Ok(entries)
}

/// List a checkout's tickets. `page_size` selects the bounded page envelope with a
/// continuation cursor; otherwise the response is a plain array capped by `limit`,
/// the shape MCP `hotsheet_query` and client search/lookup use. Both shapes share one
/// global order across every local and hosted-provider source (HS2-M0YTB6).
/// Response header set when an explicitly `limit`ed unpaged checkout array omitted
/// further matching rows (HS2-CYXS0N).
pub(crate) const TRUNCATED_HEADER: &str = "x-hotsheet-truncated";

/// `GET /checkouts/{reference}/tickets`. With `page_size` it returns the bounded page
/// envelope. Without it, a plain row array in the same global order, bounded by
/// `CHECKOUT_READ_MAX_ROWS` (HS2-CYXS0N): a caller's explicit `limit` truncates, flagged by
/// `x-hotsheet-truncated: true`, while an implicit read that would exceed the bound fails
/// with 400 so a large checkout is never serialized into one response or silently cut.
pub(crate) async fn list_checkout_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<axum::response::Response, ApiError> {
    if params.page_size.is_some() {
        return list_checkout_ticket_page(&state, &reference, params)
            .map(|page| Json(page).into_response());
    }
    if params.cursor.is_some() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "cursor requires page_size",
        ));
    }
    let cap = match params.limit {
        None => CHECKOUT_READ_MAX_ROWS,
        Some(limit) if limit <= CHECKOUT_READ_MAX_ROWS => limit,
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!(
                    "limit must be at most {CHECKOUT_READ_MAX_ROWS}; page larger reads with page_size and cursor"
                ),
            ));
        }
    };
    if cap == 0 {
        // Still validate the checkout and query even though no row is wanted.
        merge_checkout_page(&state, &reference, &params, 1, false)?;
        return Ok(Json(serde_json::Value::Array(Vec::new())).into_response());
    }
    let (items, next_cursor, _, source_errors) =
        merge_checkout_page(&state, &reference, &params, cap, false)?;
    let truncated = next_cursor.is_some();
    if truncated && params.limit.is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "more than {CHECKOUT_READ_MAX_ROWS} tickets match; pass limit (at most {CHECKOUT_READ_MAX_ROWS}) or page with page_size and cursor"
            ),
        ));
    }
    let mut response = Json(serde_json::Value::Array(items)).into_response();
    if truncated {
        response.headers_mut().insert(
            TRUNCATED_HEADER,
            axum::http::HeaderValue::from_static("true"),
        );
    }
    if !source_errors.is_empty() {
        response.headers_mut().insert(
            "x-hotsheet-partial",
            axum::http::HeaderValue::from_static("true"),
        );
    }
    Ok(response)
}

impl From<checkout_page::CheckoutPageError> for ApiError {
    fn from(error: checkout_page::CheckoutPageError) -> Self {
        use checkout_page::CheckoutPageError;
        let status = match error {
            CheckoutPageError::StaleCursor
            | CheckoutPageError::InvalidCursor
            | CheckoutPageError::InvalidSummaryDays
            | CheckoutPageError::InvalidCounts => StatusCode::BAD_REQUEST,
            CheckoutPageError::NonAdvancing | CheckoutPageError::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        ApiError::new(status, error.to_string())
    }
}

/// Fold an index summary into checkout counts.
pub(crate) fn index_summary(summary: TicketSummary) -> hotsheet_ticketing::ProviderTicketSummary {
    hotsheet_ticketing::ProviderTicketSummary {
        total: summary.total,
        queued: summary.queued,
        backlog: summary.backlog,
        archive: summary.archive,
        trash: summary.trash,
        open: summary.open,
        up_next: summary.up_next,
        active: summary.active,
        started: summary.started,
        verified: summary.verified,
        completed_today: summary.completed_today,
        completion_trend: summary.completion_trend,
    }
}

use hotsheet_ticketing::checkout_order::CHECKOUT_READ_MAX_ROWS;

pub(crate) fn list_checkout_ticket_page(
    state: &AppState,
    reference: &str,
    params: ListParams,
) -> Result<serde_json::Value, ApiError> {
    let page_size = params.page_size.unwrap_or(checkout_page::DEFAULT_PAGE_SIZE);
    if page_size == 0 || page_size > CHECKOUT_READ_MAX_ROWS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("page_size must be between 1 and {CHECKOUT_READ_MAX_ROWS}"),
        ));
    }
    let with_counts = params.counts.unwrap_or(true);
    let (items, next_cursor, counts, source_errors) =
        merge_checkout_page(state, reference, &params, page_size, with_counts)?;
    serde_json::to_value(checkout_page::CheckoutTicketPage {
        items,
        next_cursor,
        counts: with_counts.then_some(counts),
        source_errors,
    })
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

/// One bounded page of the checkout-wide merge: at most `page_size` rows in the global
/// order, the continuation cursor when a row remains, and (when requested) exact counts.
/// Both the paged envelope and the capped unpaged array read through here; the merge and
/// cursor codec are shared with the serverless MCP backend (`checkout_page`, HS2-JVF20F).
pub(crate) type CheckoutMergeResult = (
    Vec<serde_json::Value>,
    Option<String>,
    checkout_page::CheckoutTicketCounts,
    Vec<String>,
);

pub(crate) fn merge_checkout_page(
    state: &AppState,
    reference: &str,
    params: &ListParams,
    page_size: usize,
    with_counts: bool,
) -> Result<CheckoutMergeResult, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    let contexts = auto_context::effective(&checkout.settings())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let entries = checkout_entries(state, reference)?;
    if let Some(source) = params.source.as_deref() {
        if source != "git" {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "source must be git"));
        }
    }
    // A disabled source is left out of the merged view rather than failing it (HS2-SF6W34).
    let mut external_sources = Vec::new();
    for source in checkout
        .sources
        .iter()
        .filter(|source| source.provider != "git" && params.source.is_none())
    {
        if !connection_disabled(state, &source.connection_id)? {
            external_sources.push(source);
        }
    }
    let source_keys = entries
        .iter()
        .map(|(store_id, _)| checkout_page::git_source_key(store_id))
        .chain(
            external_sources
                .iter()
                .map(|source| checkout_page::provider_source_key(&source.connection_id)),
        )
        .collect::<Vec<_>>();
    let merge_query = params.clone().into_query(state.store.root())?;
    let sort = merge_query.sort;
    let descending = merge_query.descending;
    let filters = checkout_page::filter_fingerprint(&merge_query, params.has_commit);
    // Validate the cursor before paying for counts.
    checkout_page::decode_cursor(
        params.cursor.as_deref(),
        &source_keys,
        sort,
        descending,
        &filters,
    )?;

    let now = OffsetDateTime::now_utc();
    let now_text = now
        .format(&Rfc3339)
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let day_starts = checkout_page::completion_day_starts(params.summary_days.as_deref(), now)?;
    let mut counts = checkout_page::CheckoutTicketCounts::for_days(&day_starts);
    for (_, entry) in entries.iter().filter(|_| with_counts) {
        let summary = entry
            .index
            .lock_or_recover()
            .summary(&now_text, &day_starts)?;
        counts.add(index_summary(summary));
    }
    let mut failed_sources = vec![None; external_sources.len()];
    let mut provider_summaries = vec![None; external_sources.len()];
    for (index, source) in external_sources.iter().enumerate().filter(|_| with_counts) {
        let result = provider_for(state, &source.connection_id).and_then(|provider| {
            provider
                .summary(&now_text, &day_starts)
                .map_err(provider_transfer_error)
        });
        match result {
            Ok(summary) => provider_summaries[index] = Some(summary),
            Err(error) => {
                failed_sources[index] =
                    Some(format!("{}: {}", source.connection_id, error.message));
            }
        }
    }

    let compact = params.compact.unwrap_or(true);
    let fields = parse_fields(&params.fields);
    // Batched `has_commit` evaluation: one repository scan per fetched batch, not per row.
    let commit_filter = |slugs: Vec<String>| -> Result<Option<HashSet<String>>, ApiError> {
        if params.has_commit.is_none() || slugs.is_empty() {
            return Ok(None);
        }
        match code_review::slugs_with_commits(FsPath::new(&checkout.root), &slugs) {
            Ok(matches) => Ok(Some(matches)),
            Err(code_review::CodeReviewError::NotRepository) => Ok(Some(HashSet::new())),
            Err(error) => Err(code_review_api_error(error)),
        }
    };
    let keeps = |matches: &Option<HashSet<String>>, slug: &str| {
        params
            .has_commit
            .is_none_or(|want| matches.as_ref().is_some_and(|set| set.contains(slug)) == want)
    };
    // One bounded batch of rows strictly after the last row a source fetched (a value
    // keyset, HS2-74H84S). Rows removed by post-filters still advance the source, and a
    // batch is one index query or one provider keyset read (HS2-BGZ0NY).
    let fetch =
        |request: checkout_page::SourceFetch<'_>| -> Result<checkout_page::SourceBatch, ApiError> {
            if request.source < entries.len() {
                let (store_id, entry) = &entries[request.source];
                let mut query = params.clone().into_query(entry.store.root())?;
                query.limit = Some(request.want);
                query.page_after = None;
                query.after_key = request.after.cloned().map(|key| AfterKey {
                    key,
                    connection_id: store_id.clone(),
                });
                let rows = entry.index.lock_or_recover().query(&query)?;
                let exhausted = rows.len() < request.want;
                let matches = commit_filter(rows.iter().map(|row| row.slug.clone()).collect())?;
                let mut batch = Vec::with_capacity(rows.len());
                for mut row in rows {
                    row.set_connection(store_id);
                    let key = MergeKey::from_row(&row);
                    if !keeps(&matches, &row.slug) {
                        batch.push(checkout_page::SourceRow {
                            key,
                            resume: None,
                            value: None,
                        });
                        continue;
                    }
                    if compact {
                        row.make_compact();
                    }
                    row.add_auto_context(&contexts);
                    let mut value = serde_json::to_value(row).map_err(|error| {
                        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    // Project first: every checkout row keeps its source `store`.
                    hotsheet_ticketing::wire::project_fields(
                        std::slice::from_mut(&mut value),
                        &fields,
                    );
                    if let Some(object) = value.as_object_mut() {
                        object.insert("store".into(), store_id.clone().into());
                    }
                    batch.push(checkout_page::SourceRow {
                        key,
                        resume: None,
                        value: Some(value),
                    });
                }
                return Ok(checkout_page::SourceBatch {
                    rows: batch,
                    exhausted,
                });
            }
            let source_index = request.source - entries.len();
            let source = external_sources[source_index];
            if failed_sources[source_index].is_some() {
                return Ok(checkout_page::SourceBatch {
                    exhausted: true,
                    ..Default::default()
                });
            }
            let query = params.clone().into_query(state.store.root())?;
            let page = match provider_read_query_after(
                state,
                &source.connection_id,
                &hotsheet_ticketing::unbounded_query(&query),
                request.after,
                request.resume,
                request.want,
            ) {
                Ok(page) => page,
                Err(error) => {
                    failed_sources[source_index] =
                        Some(format!("{}: {}", source.connection_id, error.message));
                    return Ok(checkout_page::SourceBatch {
                        exhausted: true,
                        ..Default::default()
                    });
                }
            };
            let matches = commit_filter(
                page.items
                    .iter()
                    .map(|item| item.ticket.slug.clone())
                    .collect(),
            )?;
            let mut batch = Vec::with_capacity(page.items.len());
            for item in page.items {
                let mut ticket = item.ticket;
                let key = MergeKey::from_ticket(&ticket);
                if !keeps(&matches, &ticket.slug) {
                    batch.push(checkout_page::SourceRow {
                        key,
                        resume: item.resume,
                        value: None,
                    });
                    continue;
                }
                ticket.auto_context =
                    auto_context::resolve_fields(&ticket.category, &ticket.tags, &contexts);
                if compact {
                    ticket.details.clear();
                }
                let mut value = serde_json::to_value(ticket).map_err(|error| {
                    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
                hotsheet_ticketing::wire::project_fields(std::slice::from_mut(&mut value), &fields);
                if let Some(object) = value.as_object_mut() {
                    object.insert("store".into(), source.connection_id.clone().into());
                }
                batch.push(checkout_page::SourceRow {
                    key,
                    resume: item.resume,
                    value: Some(value),
                });
            }
            Ok(checkout_page::SourceBatch {
                rows: batch,
                exhausted: page.exhausted,
            })
        };
    let page = checkout_page::merge_page(
        &source_keys,
        params.cursor.as_deref(),
        sort,
        descending,
        &filters,
        page_size,
        fetch,
    )?;
    // A provider can succeed at summary but fail while fetching its page. Its counts are
    // incomplete in that case too, so only add sources whose entire read succeeded.
    for (index, summary) in provider_summaries.into_iter().enumerate() {
        if failed_sources[index].is_none() {
            if let Some(summary) = summary {
                counts.add(summary);
            }
        }
    }
    Ok((
        page.items,
        page.next_cursor,
        counts,
        failed_sources.into_iter().flatten().collect(),
    ))
}

#[derive(Serialize)]
pub(crate) struct CheckoutCorruptTicket {
    pub(crate) store: String,
    pub(crate) store_path: String,
    pub(crate) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) slug: Option<String>,
    pub(crate) error: String,
    pub(crate) error_code: &'static str,
}

/// `GET /checkouts/{reference}/corrupt-tickets` — every unparseable ticket file across the
/// checkout's hosted git sources.
///
/// Each hosted store keeps a stat-validated [`hotsheet_ticketing::CorruptTicketCache`]
/// (HS2-KYSBT2): a request lists and `stat`s the ticket tree and re-parses only files
/// whose fingerprint changed, so the answer always matches the disk without a full parse
/// on every project activation. The cache is prewarmed when a store is hosted. The walk
/// still runs on the blocking pool so a cold cache or a slow disk never occupies an async
/// request thread (HS2-QV8B7R). It is not shared with the `/health` single-flight scan:
/// that scan covers only the primary store and may answer from a stale listing, whereas
/// the ticket list filters rows by this response and needs the current state of every
/// linked source.
pub(crate) async fn list_checkout_corrupt_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<CheckoutCorruptTicket>>, ApiError> {
    let result = tokio::task::spawn_blocking(move || {
        let mut result = Vec::new();
        for (store_id, entry) in checkout_entries(&state, &reference)? {
            let store_path = entry.store.root().display().to_string();
            let listed = entry
                .corrupt
                .lock_or_recover()
                .corrupt_tickets(&entry.store)?;
            for corrupt in listed {
                result.push(CheckoutCorruptTicket {
                    store: store_id.clone(),
                    store_path: store_path.clone(),
                    path: corrupt.path.display().to_string(),
                    id: corrupt.id.map(|id| id.to_string()),
                    slug: corrupt.slug,
                    error: corrupt.error,
                    error_code: corrupt.error_code,
                });
            }
        }
        Ok::<_, ApiError>(result)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    Ok(Json(result))
}

/// Fill a newly hosted store's corrupt-ticket cache in the background, so the first
/// project activation's `corrupt-tickets` request finds it warm (or coalesces onto the
/// scan in progress under the cache lock) instead of starting a full parse (HS2-KYSBT2).
pub(crate) fn prewarm_corrupt_tickets(entry: StoreEntry) {
    // A failed spawn only skips the warm-up.
    let _ = std::thread::Builder::new()
        .name("hs-corrupt-prewarm".into())
        .spawn(move || {
            let mut cache = entry.corrupt.lock_or_recover();
            // Warm-up only: a scan failure is reported by the next on-demand read.
            let _ = cache.corrupt_tickets(&entry.store);
        });
}

#[derive(Deserialize)]
pub(crate) struct CorruptTicketRepairReq {
    pub(crate) path: String,
}

/// Create (or return) an Up Next work item that directs an AI worker to repair one
/// currently-corrupt git ticket. The corrupt file itself is never modified by this API.
pub(crate) async fn create_corrupt_ticket_repair(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<CorruptTicketRepairReq>,
) -> Result<Response, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let requested = std::path::PathBuf::from(&req.path);
    for (_, entry) in checkout_entries(&state, &reference)? {
        let listing = entry.store.list_tickets_resilient()?;
        let Some(corrupt) = listing
            .corrupt
            .into_iter()
            .find(|item| item.path == requested)
        else {
            continue;
        };
        if corrupt.error_code == "upgrade_required" {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "This ticket requires a newer Hot Sheet 2 version and cannot be safely auto-repaired.",
            ));
        }

        let marker = format!(
            "<!-- hotsheet:corrupt-repair {} -->",
            serde_json::to_string(&req.path).unwrap_or_else(|_| "null".into())
        );
        if let Some(existing) = listing
            .tickets
            .iter()
            .find(|ticket| ticket.details.contains(&marker) && ops::is_open(ticket))
        {
            return Ok((
                StatusCode::OK,
                Json(api_ticket_with_settings(&entry, existing, &settings)?),
            )
                .into_response());
        }

        let identity = corrupt
            .slug
            .clone()
            .or_else(|| corrupt.id.map(|id| id.to_string()))
            .or_else(|| {
                corrupt
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "unreadable ticket".into());
        let details = format!(
            "Repair the corrupt Hot Sheet ticket file at `{}`.\n\nThe parser reported:\n\n```text\n{}\n```\n\nPreserve all recoverable ticket content, rewrite it with the current canonical ticket format, and verify that Hot Sheet can parse and list it. Do not delete the file or discard content merely to make parsing succeed.\n\n{}",
            req.path, corrupt.error, marker
        );
        let created = do_create(
            &state,
            &entry,
            CreateReq {
                title: format!("Repair corrupt ticket {identity}"),
                category: Some("bug".into()),
                priority: Some("high".into()),
                status: None,
                details: Some(details),
                tags: Some(vec!["corrupt-ticket".into(), "ai-repair".into()]),
                up_next: Some(true),
                blocked_by: None,
            },
        )?;
        return Ok((
            StatusCode::CREATED,
            Json(contextualize_api_ticket(created, &settings)?),
        )
            .into_response());
    }
    Err(ApiError::new(
        StatusCode::NOT_FOUND,
        "The corrupt ticket is no longer present in this checkout.",
    ))
}

#[cfg(test)]
mod checkout_pagination_tests {
    use super::ListParams;
    use hotsheet_ticketing::checkout_page::filter_fingerprint;

    fn fingerprint(query: &str) -> String {
        let params: ListParams = serde_urlencoded_params(query);
        let has_commit = params.has_commit;
        let query = params.into_query(std::path::Path::new(".")).unwrap();
        filter_fingerprint(&query, has_commit)
    }

    fn serde_urlencoded_params(query: &str) -> ListParams {
        let uri: axum::http::Uri = format!("/?{query}").parse().unwrap();
        axum::extract::Query::<ListParams>::try_from_uri(&uri)
            .unwrap()
            .0
    }

    #[test]
    fn filter_fingerprint_ignores_order_and_paging_but_binds_every_filter() {
        let base = fingerprint("tags=a,b&status=started&text=x");
        assert_eq!(base, fingerprint("text=x&tags=b,a,a&status=started"));
        assert_eq!(
            base,
            fingerprint(
                "tags=a,b&status=started&text=x&sort=title&direction=descending&limit=3&page_size=9&cursor=v1.00"
            ),
            "ordering is bound separately; paging parameters are not filters"
        );
        assert_eq!(fingerprint(""), fingerprint("tags=&attachment="));
        for changed in [
            "tags=a&status=started&text=x",
            "tags=a,b&status=completed&text=x",
            "tags=a,b&status=started&text=y",
            "tags=a,b&status=started&text=x&up_next=true",
            "tags=a,b&status=started&text=x&has_commit=true",
            "tags=a,b&status=started&text=x&has_commit=false",
            "tags=a,b&status=started&text=x&updated_after=2026-01-01",
            "tags=a,b&status=started&text=x&collection=queue",
        ] {
            assert_ne!(base, fingerprint(changed), "{changed}");
        }
    }
}
