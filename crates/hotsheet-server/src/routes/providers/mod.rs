//! Ticket-provider connection and provider-ticket routes.

#[cfg(test)]
pub(crate) mod admission_tests;
#[cfg(test)]
mod disabled_transfer_tests;
mod tickets;
pub(crate) use tickets::{
    assign_provider_current_user, assign_provider_ticket, close_provider_ticket,
    create_provider_ticket, delete_provider_ticket_note, do_provider_create, do_provider_update,
    do_rate_ai_feedback, get_provider_ticket, list_provider_ai_feedback, list_provider_tickets,
    provider_copy_route, provider_move_route, provider_timing_headers,
    report_provider_ticket_not_working, report_provider_ticket_not_working_json,
    restore_provider_ticket, update_provider_ticket,
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
            let mut descriptor =
                hotsheet_extsync::descriptor(&connection).map_err(provider_transfer_error)?;
            descriptor.capabilities.watch =
                crate::github_webhooks::watch_ready(&state, &connection);
            descriptors.push(descriptor);
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
            let mut descriptor =
                hotsheet_extsync::descriptor(connection).map_err(provider_transfer_error)?;
            descriptor.capabilities.watch = source.provider == connection.provider
                && source.locator == connection.locator
                && crate::github_webhooks::watch_ready(&state, connection);
            Some(descriptor)
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
    mut connection: ProviderConnection,
    replacing: Option<&str>,
) -> Result<ProviderConnection, ApiError> {
    let connection_id = connection.id.clone();
    ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .update_connection(&connection_id, |connections| {
            if let Some(id) = replacing {
                connection.disabled = connections
                    .iter()
                    .any(|current| current.id == id && current.disabled);
            }
            let updated = hotsheet_extsync::updated_connections(
                std::mem::take(connections),
                connection.clone(),
                replacing,
            )?;
            *connections = updated;
            Ok(())
        })
        .map_err(provider_connection_error)?
        .ok_or_else(|| ApiError::not_found(&connection_id))
}

/// The HTTP status of a rejected connection record.
fn provider_connection_error(error: hotsheet_ticketing::ProviderError) -> ApiError {
    match error {
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
    }
}

pub(crate) fn connection_token(
    read: &hotsheet_extsync::ConnectionRead<'_, impl hotsheet_ticketing::SecretStore>,
) -> Result<String, ApiError> {
    hotsheet_extsync::connection_access_token(read, OffsetDateTime::now_utc().unix_timestamp())
        .map_err(connection_credential_error)
}

pub(crate) fn connection_credential_error(
    error: hotsheet_extsync::GitHubCredentialError,
) -> ApiError {
    match error {
        hotsheet_extsync::GitHubCredentialError::Endpoint(error) => {
            ApiError::new(StatusCode::CONFLICT, error.to_string())
        }
        hotsheet_extsync::GitHubCredentialError::Provider(error) => provider_transfer_error(error),
        hotsheet_extsync::GitHubCredentialError::Secret(error) => provider_transfer_error(error),
        other => ApiError::new(StatusCode::UNAUTHORIZED, other.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateProviderConnectionBody {
    #[serde(flatten)]
    connection: ProviderConnection,
    #[serde(default)]
    explicit_public_api: bool,
    #[serde(default)]
    infer_site: bool,
}

pub(crate) async fn create_provider_connection(
    State(state): State<AppState>,
    Json(body): Json<CreateProviderConnectionBody>,
) -> Result<(StatusCode, Json<ProviderConnection>), ApiError> {
    let mut connection = body.connection;
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
    let explicit_endpoint = connection.settings.get("api_base").is_some();
    if !body.explicit_public_api {
        fill_reused_sign_in_endpoint(&state, &mut connection)?;
    }
    let connection = if connection.provider == "github" {
        let (intent, observation) = github_create_authority(
            &state,
            &connection,
            explicit_endpoint,
            body.explicit_public_api,
            body.infer_site,
        )?;
        hotsheet_extsync::connect_github_source(
            &ProviderConfigRegistry::new(state.store.root().join("providers.json")),
            connection,
            None,
            hotsheet_extsync::ConnectMode::Create,
            intent,
            &observation,
        )
        .map_err(connect_source_error)?
    } else {
        if body.explicit_public_api || body.infer_site {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "GitHub endpoint choice is only for GitHub",
            ));
        }
        save_provider_connections(&state, connection, None)?
    };
    Ok((StatusCode::CREATED, Json(connection)))
}

/// Resolve the caller's endpoint choice before any provider or checkout write.
/// An explicit `api_base` wins over sign-in metadata; a known site creates a
/// fixed inferred decision. A plain PAT without site remains legacy-unknown.
fn github_create_authority(
    state: &AppState,
    connection: &ProviderConnection,
    explicit_endpoint: bool,
    explicit_public: bool,
    infer_site: bool,
) -> Result<
    (
        hotsheet_ticketing::github_endpoint::Intent,
        hotsheet_ticketing::github_endpoint::Observation,
    ),
    ApiError,
> {
    use hotsheet_ticketing::github_endpoint::{Endpoint, EndpointError, Intent, Observation};
    let credential_ref = connection
        .settings
        .get("credential")
        .and_then(|value| value.get("secret"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, EndpointError::Invalid.to_string()))?
        .to_owned();
    let endpoint = Endpoint::from_record(connection)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    let site = state.key_registry().list().ok().and_then(|entries| {
        entries
            .into_iter()
            .find(|entry| entry.provider == credential_ref)
            .and_then(|entry| entry.site)
    });
    let observation =
        site.as_ref()
            .map_or(Observation::Unavailable, |site| Observation::Available {
                credential_ref: credential_ref.clone(),
                site: site.clone(),
            });
    if explicit_endpoint && explicit_public {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "choose either api_base or explicit_public_api",
        ));
    }
    if explicit_public && infer_site {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "choose either explicit_public_api or infer_site",
        ));
    }
    let intent = if infer_site {
        let site = site.ok_or_else(|| {
            ApiError::new(StatusCode::CONFLICT, EndpointError::Unavailable.to_string())
        })?;
        Intent::InferSite {
            credential_ref,
            endpoint,
            origin: hotsheet_ticketing::github_origin::normalize_web_base(&site).map_err(|_| {
                ApiError::new(StatusCode::CONFLICT, EndpointError::Invalid.to_string())
            })?,
        }
    } else if explicit_endpoint || explicit_public {
        Intent::Explicit {
            credential_ref,
            endpoint,
        }
    } else if let Some(site) = site {
        Intent::InferSite {
            credential_ref,
            endpoint,
            origin: hotsheet_ticketing::github_origin::normalize_web_base(&site).map_err(|_| {
                ApiError::new(StatusCode::CONFLICT, EndpointError::Invalid.to_string())
            })?,
        }
    } else if credential_ref.starts_with("github-app-") {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            EndpointError::Unavailable.to_string(),
        ));
    } else {
        Intent::Legacy
    };
    Ok((intent, observation))
}

fn connect_source_error(error: hotsheet_extsync::ConnectSourceError) -> ApiError {
    match error {
        hotsheet_extsync::ConnectSourceError::Endpoint(error) => {
            ApiError::new(StatusCode::CONFLICT, error.to_string())
        }
        hotsheet_extsync::ConnectSourceError::Provider(error) => provider_connection_error(error),
        hotsheet_extsync::ConnectSourceError::Link(error) => {
            super::checkouts::checkout_source_error(error)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GithubRecoveryAction {
    Resume,
    Compensate,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GithubRecoveryBody {
    action: GithubRecoveryAction,
}

pub(crate) async fn recover_github_connection(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(body): Json<GithubRecoveryBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let action = match body.action {
        GithubRecoveryAction::Resume => hotsheet_extsync::RecoverGithubLink::Resume,
        GithubRecoveryAction::Compensate => hotsheet_extsync::RecoverGithubLink::Compensate,
    };
    let result = hotsheet_extsync::recover_github_link(
        &ProviderConfigRegistry::new(state.store.root().join("providers.json")),
        &state.checkout_registry,
        &connection_id,
        action,
    )
    .map_err(connect_source_error)?;
    use hotsheet_ticketing::github_endpoint::Compensation;
    let value = match result {
        hotsheet_extsync::GithubLinkRecovery::Finalized(connection) => {
            serde_json::json!({"status":"finalized","connection":connection})
        }
        hotsheet_extsync::GithubLinkRecovery::Compensated(Compensation::Restored(connection)) => {
            serde_json::json!({"status":"restored","connection":connection})
        }
        hotsheet_extsync::GithubLinkRecovery::Compensated(Compensation::Removed) => {
            serde_json::json!({"status":"removed"})
        }
        hotsheet_extsync::GithubLinkRecovery::Compensated(Compensation::Retained(_)) => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                hotsheet_ticketing::github_endpoint::EndpointError::Conflict.to_string(),
            ));
        }
    };
    Ok(Json(value))
}

/// A new GitHub source reusing a GitHub Enterprise sign-in gets that site's `api_base` even
/// when the client did not send one (HS2-16MYXN). Best effort: an unreadable key registry
/// leaves the connection as sent. Known malformed historical origins fail before writes.
fn fill_reused_sign_in_endpoint(
    state: &AppState,
    connection: &mut ProviderConnection,
) -> Result<(), ApiError> {
    if connection.provider != "github" {
        return Ok(());
    }
    // keys.json metadata only: creating a source never reads the keychain (the listing that
    // offered the account already backfilled any older sign-in's site).
    if let Ok(credentials) = state.key_registry().list() {
        hotsheet_ticketing::accounts::fill_reused_github_api_base(connection, &credentials)
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    }
    Ok(())
}

pub(crate) async fn update_provider_connection(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(body): Json<UpdateProviderConnectionBody>,
) -> Result<Json<ProviderConnection>, ApiError> {
    update_connection_record(
        &state,
        connection_id,
        body.connection,
        body.review_endpoint,
        body.endpoint_expected,
    )
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateProviderConnectionBody {
    #[serde(flatten)]
    connection: ProviderConnection,
    #[serde(default)]
    review_endpoint: bool,
    endpoint_expected: Option<hotsheet_ticketing::github_endpoint::Expected>,
}

#[derive(Serialize)]
pub(crate) struct EndpointReview {
    connection: ProviderConnection,
    expected: hotsheet_ticketing::github_endpoint::Expected,
    decision: Option<hotsheet_ticketing::github_endpoint::Decision>,
}

pub(crate) async fn get_provider_endpoint_review(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<EndpointReview>, ApiError> {
    use hotsheet_ticketing::github_endpoint::{Expected, State};
    let connection = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?
        .into_iter()
        .find(|row| row.id == connection_id && row.provider == "github")
        .ok_or_else(|| ApiError::not_found(&connection_id))?;
    let expected = Expected::capture(&connection)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    let decision = State::load(&connection)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?
        .map(|state| state.decision);
    Ok(Json(EndpointReview {
        connection,
        expected,
        decision,
    }))
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
    #[serde(default)]
    explicit_public_api: bool,
    #[serde(default)]
    infer_site: bool,
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
    let explicit_endpoint = connection.settings.get("api_base").is_some();
    let registry = ProviderConfigRegistry::new(state.store.root().join("providers.json"));
    if !body.explicit_public_api {
        fill_reused_sign_in_endpoint(&state, &mut connection)?;
    }
    // The same validated create-and-link workflow `hotsheet provider-connect` runs
    // (HS2-65KYVP). A project's default lives on its checkout, never on the shared record,
    // and a failed link removes the new record again.
    let link = Some(hotsheet_extsync::CheckoutLink {
        registry: &state.checkout_registry,
        reference: &checkout.id,
        make_default: body.make_default,
    });
    let connection = if connection.provider == "github" {
        let (intent, observation) = github_create_authority(
            &state,
            &connection,
            explicit_endpoint,
            body.explicit_public_api,
            body.infer_site,
        )?;
        hotsheet_extsync::connect_github_source(
            &registry,
            connection,
            link,
            hotsheet_extsync::ConnectMode::Create,
            intent,
            &observation,
        )
    } else {
        if body.explicit_public_api || body.infer_site {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "GitHub endpoint choice is only for GitHub",
            ));
        }
        hotsheet_extsync::connect_source(
            &registry,
            connection,
            link,
            hotsheet_extsync::ConnectMode::Create,
        )
    }
    .map_err(connect_source_error)?;
    let linked = state
        .checkout_registry
        .resolve(&checkout.id)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    super::checkouts::retry_pending_guidance(&state, &linked).await;
    let (_, checkouts) = account_listing_inputs(&state)?;
    Ok((
        StatusCode::CREATED,
        Json(project_connection(connection, &checkouts)),
    ))
}

pub(crate) async fn update_checkout_provider_connection(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
    Json(body): Json<UpdateProviderConnectionBody>,
) -> Result<Json<ProviderConnection>, ApiError> {
    checkout_linking(&state, &reference, &connection_id)?;
    update_connection_record(
        &state,
        connection_id,
        body.connection,
        body.review_endpoint,
        body.endpoint_expected,
    )
}

pub(crate) async fn set_checkout_provider_connection_disabled(
    State(state): State<AppState>,
    Path((reference, connection_id)): Path<(String, String)>,
    Json(body): Json<ProviderConnectionDisabledBody>,
) -> Result<Json<ProviderConnection>, ApiError> {
    checkout_linking(&state, &reference, &connection_id)?;
    let connection = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .set_disabled(&connection_id, body.disabled)
        .map_err(provider_transfer_error)?
        .ok_or_else(|| ApiError::not_found(&connection_id))?;
    crate::github_webhooks::forget_connection(&state, &connection_id);
    Ok(Json(connection))
}

fn update_connection_record(
    state: &AppState,
    connection_id: String,
    mut connection: ProviderConnection,
    review_endpoint: bool,
    endpoint_expected: Option<hotsheet_ticketing::github_endpoint::Expected>,
) -> Result<Json<ProviderConnection>, ApiError> {
    connection.id = connection_id.clone();
    let registry = ProviderConfigRegistry::new(state.store.root().join("providers.json"));
    let previous = registry
        .load()
        .map_err(provider_transfer_error)?
        .into_iter()
        .find(|row| row.id == connection_id)
        .ok_or_else(|| ApiError::not_found(&connection_id))?;
    let endpoint_changed = previous.provider == "github"
        && (previous.settings.get("api_base") != connection.settings.get("api_base")
            || previous.settings.get("credential") != connection.settings.get("credential"));
    // A user-edited material endpoint/account value is an explicit review. The
    // ordinary metadata route keeps existing provenance when those values match.
    let connection = if endpoint_changed && review_endpoint {
        use hotsheet_ticketing::github_endpoint::{
            Endpoint, EndpointError, EndpointRegistry, Intent, Link, LinkObserver, Observation,
            Request,
        };
        struct NoLink;
        impl LinkObserver for NoLink {
            fn is_linked(&self, _: &str, _: &Link) -> Result<bool, EndpointError> {
                Err(EndpointError::Conflict)
            }
        }
        if previous.locator != connection.locator {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "review the GitHub endpoint and repository locator in separate edits",
            ));
        }
        let endpoint = Endpoint::from_record(&connection)
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
        let credential_ref = connection
            .settings
            .get("credential")
            .and_then(|value| value.get("secret"))
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, EndpointError::Invalid.to_string()))?
            .to_owned();
        connection.disabled = previous.disabled;
        connection.default = previous.default;
        if let Some(webhook) = previous.settings.get("webhook").cloned() {
            connection.settings["webhook"] = webhook;
        }
        hotsheet_extsync::descriptor(&connection).map_err(provider_connection_error)?;
        EndpointRegistry::new(&registry)
            .apply(
                &Request {
                    operation_id: hotsheet_model::Ulid::new(),
                    expected: Some(endpoint_expected.ok_or_else(|| {
                        ApiError::new(
                            StatusCode::BAD_REQUEST,
                            "endpoint_expected is required for review",
                        )
                    })?),
                    connection,
                    intent: Intent::Explicit {
                        credential_ref,
                        endpoint,
                    },
                    link: None,
                },
                &Observation::Unavailable,
                &NoLink,
            )
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?
            .connection
    } else {
        if endpoint_changed {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                hotsheet_ticketing::github_endpoint::EndpointError::Conflict.to_string(),
            ));
        }
        if review_endpoint {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "endpoint review requires a changed GitHub endpoint or account",
            ));
        }
        if endpoint_expected.is_some() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "endpoint_expected requires review_endpoint",
            ));
        }
        // The ordinary transaction preserves the current disabled flag and webhook setup.
        save_provider_connections(state, connection, Some(&connection_id))?
    };
    forget_live_provider(state, &connection_id);
    crate::github_webhooks::forget_connection(state, &connection_id);
    // Checkout links copy the locator; only an actual locator edit needs their
    // second publication. An endpoint-only review must not fail after it commits
    // because unrelated checkout metadata is unavailable (HS2-6H8ZY6).
    if previous.locator != connection.locator {
        state
            .checkout_registry
            .update_registered_source_locator(&registry, &connection)
            .map_err(|error| match error {
                hotsheet_ticketing::checkouts::CheckoutError::Endpoint(error) => {
                    ApiError::new(StatusCode::CONFLICT, error.to_string())
                }
                other => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
            })?;
    }
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
    crate::github_webhooks::forget_connection(&state, &connection_id);
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
        crate::github_webhooks::forget_connection(&state, &connection_id);
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

// HS2-DJWTB0: persisted policy also covers injected external winners. Skipped Git
// metadata cannot disable a hosted-only Git object, nor exempt a constructed external one.
fn admit_selected_transfers(
    connections: &[ProviderConnection],
    winners: &[hotsheet_ticketing::ProviderDescriptor],
    source: &str,
    destination: &str,
) -> Result<(), ApiError> {
    for id in [source, destination] {
        if let Some(connection) = connections.iter().find(|connection| connection.id == id) {
            let external = connection.provider != "git"
                || winners
                    .iter()
                    .any(|winner| winner.connection_id == id && winner.provider != "git");
            if connection.disabled && external {
                return Err(provider_transfer_error(
                    hotsheet_ticketing::ProviderError::Disabled {
                        connection_id: id.into(),
                    },
                ));
            }
        }
        if source == destination {
            break;
        }
    }
    Ok(())
}

pub(crate) fn hosted_provider_registry(
    state: &AppState,
    source: &str,
    destination: &str,
) -> Result<ProviderRegistry, ApiError> {
    // HS2-6H8ZY6: unrelated source authority cannot veto an explicitly selected transfer.
    let selected = |id: &str| id == source || id == destination;
    let registry = ProviderRegistry::default();
    let default_id = multistore::store_url_id(&state.store);
    let config = ProviderConfigRegistry::new(state.store.root().join("providers.json"));
    let connections = config.load().map_err(provider_transfer_error)?;
    let injected = state.injected_providers.descriptors();
    admit_selected_transfers(&connections, &injected, source, destination)?;
    let keys = state.key_registry();
    let mut reads = Vec::new();
    let external_default = connections.iter().any(|connection| connection.default)
        || injected.iter().any(|descriptor| descriptor.default);
    for (id, _) in state.host.locations() {
        if !selected(&id) {
            continue;
        }
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
        if connection.provider == "git" || !selected(&connection.id) {
            continue;
        }
        let read = hotsheet_extsync::ConnectionRead::capture(&config, &connection, &keys)
            .map_err(connection_credential_error)?;
        let token = connection_token(&read)?;
        read.check().map_err(connection_credential_error)?;
        registry
            .register(
                hotsheet_extsync::live_provider(&connection, token)
                    .map_err(provider_transfer_error)?,
            )
            .map_err(provider_transfer_error)?;
        reads.push(read);
    }
    for descriptor in injected {
        if !selected(&descriptor.connection_id) {
            continue;
        }
        if let Some(read) = reads
            .iter()
            .find(|read| read.connection().id == descriptor.connection_id)
        {
            read.check().map_err(connection_credential_error)?;
        }
        registry
            .register(
                state
                    .injected_providers
                    .get(&descriptor.connection_id)
                    .map_err(provider_transfer_error)?,
            )
            .map_err(provider_transfer_error)?;
    }
    for read in &reads {
        read.check().map_err(connection_credential_error)?;
    }
    // Observe actual constructed winners before the fresh policy read: a concurrent
    // external→Git metadata change must not exempt the still-external object.
    let winners = registry.descriptors();
    let current = config.load().map_err(provider_transfer_error)?;
    admit_selected_transfers(&current, &winners, source, destination)?;
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
    let config = ProviderConfigRegistry::new(state.store.root().join("providers.json"));
    let keys = state.key_registry();
    let connection = config
        .load()
        .map_err(provider_transfer_error)?
        .into_iter()
        .find(|connection| connection.id == connection_id);
    let read = connection
        .as_ref()
        .map(|connection| hotsheet_extsync::ConnectionRead::capture(&config, connection, &keys))
        .transpose()
        .map_err(connection_credential_error)?;
    let Some(read) = read else {
        return state
            .injected_providers
            .get(connection_id)
            .map_err(|_| ApiError::not_found(connection_id));
    };
    provider_from_read(state, &read)
}

/// HS2-6H8ZY6: resolve using the caller's original source proof, including on
/// injected and cached returns. Never recapture after a webhook effect.
pub(crate) fn provider_from_read(
    state: &AppState,
    read: &hotsheet_extsync::ConnectionRead<'_, impl hotsheet_ticketing::SecretStore>,
) -> Result<Arc<dyn hotsheet_ticketing::TicketProvider>, ApiError> {
    let connection = read.connection();
    let connection_id = connection.id.as_str();
    if let Some(entry) = state.host.get(connection_id) {
        read.check().map_err(connection_credential_error)?;
        return Ok(Arc::new(
            GitProvider::new(connection_id, entry.store.clone()).with_resolver(entry.resolver()),
        ));
    }
    if let Ok(provider) = state.injected_providers.get(connection_id) {
        read.check().map_err(connection_credential_error)?;
        return Ok(provider);
    }
    let token = connection_token(read)?;
    use sha2::{Digest, Sha256};
    let mut fingerprint = Sha256::new();
    fingerprint.update(serde_json::to_vec(&connection).map_err(provider_transfer_error)?);
    fingerprint.update(token.as_bytes());
    let fingerprint = format!("{:x}", fingerprint.finalize());
    if let Some(provider) = cached_provider(state, connection_id, &fingerprint, read)? {
        return Ok(provider);
    }
    let provider =
        hotsheet_extsync::live_provider(connection, token).map_err(provider_transfer_error)?;
    read.check().map_err(connection_credential_error)?;
    state
        .live_providers
        .lock_or_recover()
        .insert(connection_id.into(), (fingerprint, provider.clone()));
    Ok(provider)
}

/// HS2-6H8ZY6: the actual cache-hit return boundary checks the SAME captured proof.
fn cached_provider(
    state: &AppState,
    connection_id: &str,
    fingerprint: &str,
    read: &hotsheet_extsync::ConnectionRead<'_, impl hotsheet_ticketing::SecretStore>,
) -> Result<Option<Arc<dyn hotsheet_ticketing::TicketProvider>>, ApiError> {
    let cached = state
        .live_providers
        .lock_or_recover()
        .get(connection_id)
        .filter(|(key, _)| key == fingerprint)
        .map(|(_, provider)| provider.clone());
    if cached.is_some() {
        read.check().map_err(connection_credential_error)?;
    }
    Ok(cached)
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

pub(crate) fn provider_transfer_error(error: impl std::fmt::Display + 'static) -> ApiError {
    let stale = (&error as &dyn std::any::Any)
        .downcast_ref::<hotsheet_ticketing::ProviderError>()
        .is_some_and(hotsheet_ticketing::ProviderError::is_changed_since_read);
    if stale {
        ApiError::ticket_changed(error.to_string())
    } else {
        ApiError::new(StatusCode::CONFLICT, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_error::TICKET_CHANGED_CODE;
    use axum::response::IntoResponse;
    use hotsheet_ticketing::ProviderError;
    use hotsheet_ticketing::provider::{ISSUE_CHANGED_SINCE_READ, TICKET_CHANGED_SINCE_READ};

    #[tokio::test]
    async fn hs2_hwc0fs_partial_metadata_error_is_409_without_stale_ticket_code() {
        let error = hotsheet_ticketing::SecretError::MetadataCommitIncomplete;
        let expected = error.to_string();
        let (status, body) = rendered(provider_transfer_error(error)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], expected);
        assert!(body.get("code").is_none());
    }

    async fn rendered(error: ApiError) -> (StatusCode, serde_json::Value) {
        let response = error.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn hs2_6h8zy6_typed_registry_refusals_are_409_without_stale_ticket_code() {
        use hotsheet_ticketing::github_endpoint::EndpointError;
        for error in [
            EndpointError::Invalid,
            EndpointError::Pending,
            EndpointError::Conflict,
        ] {
            let (status, body) =
                rendered(provider_connection_error(ProviderError::Endpoint(error))).await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(body["error"], error.to_string());
            assert!(body.get("code").is_none());
        }
    }

    #[tokio::test]
    async fn hs2_6h8zy6_endpoint_credentials_are_409_without_changing_other_credential_statuses() {
        use hotsheet_extsync::GitHubCredentialError;
        use hotsheet_ticketing::github_endpoint::EndpointError;
        for error in [
            EndpointError::Pending,
            EndpointError::Invalid,
            EndpointError::Conflict,
            EndpointError::Drift,
        ] {
            let (status, body) = rendered(connection_credential_error(
                GitHubCredentialError::Endpoint(error),
            ))
            .await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert!(body.get("code").is_none());
        }
        for error in [
            GitHubCredentialError::Expired,
            GitHubCredentialError::Invalid("synthetic-invalid".into()),
        ] {
            assert_eq!(
                rendered(connection_credential_error(error)).await.0,
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            rendered(connection_credential_error(GitHubCredentialError::Secret(
                hotsheet_ticketing::SecretError::NotFound("synthetic-missing".into())
            )))
            .await
            .0,
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn only_stale_provider_tokens_carry_the_ticket_changed_code() {
        // HS2-9S8CV1: every provider's expected-token wording gets the typed code; other
        // conflicts, and non-provider errors that happen to share the text, stay code-free.
        for (ticket, message) in [
            ("01M5", TICKET_CHANGED_SINCE_READ),
            ("github:7", ISSUE_CHANGED_SINCE_READ),
        ] {
            let (status, body) = rendered(provider_transfer_error(ProviderError::Conflict {
                ticket: ticket.into(),
                message: message.into(),
            }))
            .await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(body["code"], TICKET_CHANGED_CODE);
            assert_eq!(
                body["error"],
                format!("provider conflict for '{ticket}': {message}")
            );
        }
        let (status, body) = rendered(provider_transfer_error(ProviderError::Conflict {
            ticket: "github:7".into(),
            message: "attachment changed during crop upload".into(),
        }))
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body.get("code").is_none(), "{body}");
        let (_, text) = rendered(provider_transfer_error(TICKET_CHANGED_SINCE_READ)).await;
        assert!(text.get("code").is_none(), "{text}");
    }
}
