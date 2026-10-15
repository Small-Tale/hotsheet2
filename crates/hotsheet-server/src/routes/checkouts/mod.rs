//! Project / checkout registration, sources, repository, code review, and checkout ticket listing.

mod listing;
pub(crate) use listing::{
    TRUNCATED_HEADER, create_corrupt_ticket_repair, list_checkout_corrupt_tickets,
    list_checkout_tickets, prewarm_corrupt_tickets,
};

use crate::api_error::ApiError;
use crate::multistore::{StoreEntry, StoreHost};
use crate::routes::checkout_tickets::{checkout_git_ticket, checkout_ticket_owner};
use crate::routes::providers::provider_transfer_error;
use crate::watcher::repository_change_event;
use crate::{AppState, code_review, repository_browser};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hotsheet_model::Ulid;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{FsStore, ProviderConfigRegistry, TicketQuery};
use serde::{Deserialize, Serialize};
use std::path::Path as FsPath;

#[derive(Debug, Deserialize)]
pub(crate) struct RegisterCheckoutBody {
    root: String,
    alias: Option<String>,
    repository: Option<String>,
    #[serde(default)]
    stores: Vec<String>,
    #[serde(default)]
    sources: Vec<hotsheet_ticketing::checkouts::TicketSource>,
    default_source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenProjectBody {
    root: String,
    alias: Option<String>,
    repository: Option<String>,
    /// Explicit git stores. When omitted, conservative filesystem discovery is used.
    stores: Option<Vec<String>>,
    /// Explicit provider-neutral sources. When supplied, these are authoritative.
    sources: Option<Vec<hotsheet_ticketing::checkouts::TicketSource>>,
    default_source: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenProjectResponse {
    checkout: hotsheet_ticketing::checkouts::Checkout,
    discovered: bool,
}

fn schedule_setup_freshness(state: &AppState, checkout: &hotsheet_ticketing::checkouts::Checkout) {
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

fn schedule_worklist_regeneration(
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
            Ok(Err(error)) => {
                tracing::warn!(checkout_root = %root, %error, "worklist regenerate failed")
            }
            Err(error) => {
                tracing::warn!(checkout_root = %root, %error, "worklist regenerate task failed")
            }
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
    provider: String,
    locator: String,
    #[serde(default)]
    make_default: bool,
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
    connection_id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CheckoutSourceColorBody {
    color: String,
}

#[derive(Deserialize)]
pub(crate) struct RelinkGitSourceBody {
    path: String,
    #[serde(default)]
    review_unverified_recovery: bool,
}

#[derive(Serialize)]
pub(crate) struct RelinkGitSourceResponse {
    checkouts: Vec<hotsheet_ticketing::checkouts::Checkout>,
    connection_id: String,
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
    remote: String,
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

fn repository_setup_api_error(error: repository_browser::RepositorySetupError) -> ApiError {
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

/// One ticket's usage history for the timeline (HS2-ZA7W6E, HS2-J14225). Usage events are
/// recorded in the git store that owns the ticket; a ticket from another provider has no
/// local usage, so it yields an empty history rather than a capability error.
pub(crate) async fn get_checkout_ticket_usage(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<hotsheet_ticketing::metrics::TicketUsage>, ApiError> {
    let (source, _) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        return Ok(Json(hotsheet_ticketing::metrics::TicketUsage::default()));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let ulid = ticket.id.to_string();
    tokio::task::spawn_blocking(move || {
        hotsheet_ticketing::metrics::ticket_usage(&entry.store, &ulid)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
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
