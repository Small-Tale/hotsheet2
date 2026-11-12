//! Ticket group routes (HS2-PDSHNN, HS2-8CDFKR, HS2-9KZSD4): list, create, rename,
//! ungroup, merge, and migrate, for the default store, one hosted store, one provider
//! connection, and a whole checkout.
//!
//! A checkout is one project: its groups are merged across every connection with the
//! `groups` capability (its Git sources) and named by the checkout's group registry
//! (`<checkout>/.hotsheet2/groups/<id>.json`). A store or connection has no registry, so
//! it lists ids under placeholders, accepts ids only, and refuses the operations that
//! need names (create, rename, migrate) with `group_registry_required`. Each store's
//! members change in one locked write and one commit.

use crate::AppState;
use crate::api_error::{ActorReq, ApiError, parse_actor};
use crate::multistore::StoreEntry;
use crate::routes::checkouts::{checkout_entries, checkout_lookup_error};
use crate::routes::providers::{provider_for, provider_transfer_error};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::groups::{
    self, GroupDirectory, GroupError, GroupOutcome, GroupRegistry, GroupRegistryError, GroupSummary,
};
use serde::{Deserialize, Serialize};

/// `404`: no listed group matches.
pub(crate) const GROUP_NOT_FOUND_CODE: &str = "group_not_found";
/// `409`: the requested name is a different existing group.
pub(crate) const GROUP_NAME_TAKEN_CODE: &str = "group_name_taken";
/// `409`: a name matches several groups (duplicates); use an id.
pub(crate) const GROUP_AMBIGUOUS_CODE: &str = "group_ambiguous";
/// `400`: the requested name is empty, too long, or holds control characters.
pub(crate) const INVALID_GROUP_NAME_CODE: &str = "invalid_group_name";
/// `400`: a name was given where only an id is accepted (no project registry).
pub(crate) const GROUP_REGISTRY_REQUIRED_CODE: &str = "group_registry_required";
/// `400`: a merge into itself or into a legacy name.
pub(crate) const INVALID_GROUP_MERGE_CODE: &str = "invalid_group_merge";

pub(crate) fn group_error(error: GroupError) -> ApiError {
    let (status, code) = match &error {
        GroupError::Registry(
            GroupRegistryError::InvalidName(_) | GroupRegistryError::EmptyName,
        )
        | GroupError::InvalidName(_)
        | GroupError::EmptyName => (StatusCode::BAD_REQUEST, INVALID_GROUP_NAME_CODE),
        GroupError::UnknownGroup(_) => (StatusCode::NOT_FOUND, GROUP_NOT_FOUND_CODE),
        GroupError::NameTaken(_) => (StatusCode::CONFLICT, GROUP_NAME_TAKEN_CODE),
        GroupError::Ambiguous(_) => (StatusCode::CONFLICT, GROUP_AMBIGUOUS_CODE),
        GroupError::RegistryRequired(_) => (StatusCode::BAD_REQUEST, GROUP_REGISTRY_REQUIRED_CODE),
        GroupError::InvalidMerge(_) => (StatusCode::BAD_REQUEST, INVALID_GROUP_MERGE_CODE),
        GroupError::Registry(_) => {
            return ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
        }
        GroupError::Store(_) => {
            let GroupError::Store(error) = error else {
                unreachable!("matched above")
            };
            return error.into();
        }
    };
    ApiError::new(status, error.to_string()).with_code(code)
}

/// The namespace `entries` share, read from their indexes (no store walk).
pub(crate) fn directory(entries: &[StoreEntry]) -> Result<GroupDirectory, ApiError> {
    let mut merged = GroupDirectory::default();
    for entry in entries {
        let members = entry.index.lock_or_recover().group_members()?;
        merged.merge(GroupDirectory::new(members));
    }
    Ok(merged)
}

/// Validate a requested group change for a store with no project registry: an id is
/// kept, a name is refused with `group_registry_required`.
pub(crate) fn resolve_for_write(
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>, ApiError> {
    groups::resolve_group_change(None, change).map_err(group_error)
}

/// Resolve a requested group change through the checkout's registry: a name becomes the
/// id of the group with that name, created when none exists (HS2-9KZSD4).
pub(crate) fn resolve_for_checkout(
    state: &AppState,
    reference: &str,
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>, ApiError> {
    if !matches!(change, Some(Some(_))) {
        return Ok(change);
    }
    let registry = checkout_registry(state, reference)?;
    groups::resolve_group_change(Some(&registry), change).map_err(group_error)
}

/// Validate a group for a provider-connection write: an invalid name is the caller's
/// error whatever the provider; a connection that stores groups has no registry, so a
/// name is refused there (a provider without groups refuses the write itself).
pub(crate) fn validate_for_connection(
    provider: &dyn hotsheet_ticketing::TicketProvider,
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>, ApiError> {
    let Some(Some(input)) = change else {
        return Ok(change);
    };
    let Some(value) = hotsheet_model::normalize_group_name(&input).map_err(|error| {
        ApiError::new(StatusCode::BAD_REQUEST, error.to_string()).with_code(INVALID_GROUP_NAME_CODE)
    })?
    else {
        return Ok(Some(None));
    };
    if provider.descriptor().capabilities.groups && !hotsheet_model::is_group_id(&value) {
        return Err(group_error(GroupError::RegistryRequired(value)));
    }
    Ok(Some(Some(value)))
}

/// The group registry of a checkout.
pub(crate) fn checkout_registry(
    state: &AppState,
    reference: &str,
) -> Result<GroupRegistry, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    Ok(GroupRegistry::for_checkout(std::path::Path::new(
        &checkout.root,
    )))
}

/// The ids whose registry names match a typed group filter, so a checkout query finds
/// tickets that store ids (HS2-9KZSD4).
pub(crate) fn filter_ids_for_checkout(
    state: &AppState,
    reference: &str,
    input: &str,
) -> Result<Vec<String>, ApiError> {
    let Some(key) = hotsheet_model::group_key_of_input(input) else {
        return Ok(Vec::new());
    };
    let names = checkout_registry(state, reference)?
        .names()
        .map_err(|error| group_error(error.into()))?;
    Ok(names
        .into_iter()
        .filter(|(_, name)| hotsheet_model::group_key(name) == key)
        .map(|(id, _)| id)
        .collect())
}

/// The stores (and registry, for a checkout) one group route addresses.
struct Scope {
    entries: Vec<StoreEntry>,
    registry: Option<GroupRegistry>,
}

impl Scope {
    fn stores(&self) -> Vec<&FsStore> {
        self.entries.iter().map(|entry| &entry.store).collect()
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct GroupsResponse {
    /// Whether names come from a project registry. Without one, every id is listed
    /// under a placeholder.
    registry: bool,
    groups: Vec<GroupSummary>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateGroupReq {
    name: String,
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CreateGroupResponse {
    id: String,
    name: String,
    created: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RenameGroupReq {
    /// The group's id, or its current name. `from` is the pre-registry field name.
    #[serde(alias = "from")]
    group: String,
    to: String,
    /// Who is acting; validated like every other mutation's actor.
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GroupChangeResponse {
    id: String,
    name: String,
    /// Live member tickets rewritten (a rename of an id group rewrites none).
    updated: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UngroupReq {
    /// The group's id, or its name. `name` is the pre-registry field name.
    #[serde(alias = "name")]
    group: String,
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Serialize)]
pub(crate) struct UngroupResponse {
    updated: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MergeGroupsReq {
    from: String,
    into: String,
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct MigrateGroupsReq {
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrateGroupsResponse {
    created: usize,
    reused: usize,
    updated: usize,
}

/// Publish each store's rewritten tickets like a batch update: index rows, change
/// events, and one worklist rebuild per store.
fn publish(state: &AppState, scope: &Scope, writes: &[groups::GroupWrite]) {
    for (entry, write) in scope.entries.iter().zip(writes) {
        if !write.tickets.is_empty() {
            let _ = state.changed_many_in(entry, "updated", &write.tickets);
        }
    }
}

fn change_response(outcome: GroupOutcome) -> Json<GroupChangeResponse> {
    Json(GroupChangeResponse {
        id: outcome.id.unwrap_or_default(),
        name: outcome.name.unwrap_or_default(),
        updated: outcome.updated,
    })
}

fn do_list(scope: &Scope) -> Result<Json<GroupsResponse>, ApiError> {
    Ok(Json(GroupsResponse {
        registry: scope.registry.is_some(),
        groups: groups::list_groups(&directory(&scope.entries)?, scope.registry.as_ref())
            .map_err(group_error)?,
    }))
}

fn do_create(scope: &Scope, req: CreateGroupReq) -> Result<Json<CreateGroupResponse>, ApiError> {
    parse_actor(req.actor.as_ref())?;
    let (id, name, created) =
        groups::create_group(scope.registry.as_ref(), &req.name).map_err(group_error)?;
    Ok(Json(CreateGroupResponse { id, name, created }))
}

fn do_rename(
    state: &AppState,
    scope: &Scope,
    req: RenameGroupReq,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    parse_actor(req.actor.as_ref())?;
    let outcome = groups::rename_group(
        &scope.stores(),
        &directory(&scope.entries)?,
        scope.registry.as_ref(),
        &req.group,
        &req.to,
    )
    .map_err(group_error)?;
    publish(state, scope, &outcome.writes);
    Ok(change_response(outcome))
}

fn do_ungroup(
    state: &AppState,
    scope: &Scope,
    req: UngroupReq,
) -> Result<Json<UngroupResponse>, ApiError> {
    parse_actor(req.actor.as_ref())?;
    let outcome = groups::ungroup(
        &scope.stores(),
        &directory(&scope.entries)?,
        scope.registry.as_ref(),
        &req.group,
    )
    .map_err(group_error)?;
    publish(state, scope, &outcome.writes);
    Ok(Json(UngroupResponse {
        updated: outcome.updated,
    }))
}

fn do_merge(
    state: &AppState,
    scope: &Scope,
    req: MergeGroupsReq,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    parse_actor(req.actor.as_ref())?;
    let outcome = groups::merge_groups(
        &scope.stores(),
        &directory(&scope.entries)?,
        scope.registry.as_ref(),
        &req.from,
        &req.into,
    )
    .map_err(group_error)?;
    publish(state, scope, &outcome.writes);
    Ok(change_response(outcome))
}

fn do_migrate(
    state: &AppState,
    scope: &Scope,
    req: MigrateGroupsReq,
) -> Result<Json<MigrateGroupsResponse>, ApiError> {
    parse_actor(req.actor.as_ref())?;
    let registry = scope
        .registry
        .as_ref()
        .ok_or_else(|| group_error(GroupError::RegistryRequired("migrate".into())))?;
    let migration = groups::migrate_groups(&scope.stores(), &directory(&scope.entries)?, registry)
        .map_err(group_error)?;
    publish(state, scope, &migration.writes);
    Ok(Json(MigrateGroupsResponse {
        created: migration.created,
        reused: migration.reused,
        updated: migration.updated,
    }))
}

// ---- default store ---------------------------------------------------------------

fn default_scope(state: &AppState) -> Scope {
    Scope {
        entries: vec![state.default_entry()],
        registry: None,
    }
}

/// `GET /groups` — the default store's groups (ids under placeholders).
pub(crate) async fn list_groups(
    State(state): State<AppState>,
) -> Result<Json<GroupsResponse>, ApiError> {
    do_list(&default_scope(&state))
}

/// `POST /groups/rename` — refused with `group_registry_required`: a store alone has no
/// group names.
pub(crate) async fn rename_group(
    State(state): State<AppState>,
    Json(req): Json<RenameGroupReq>,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    do_rename(&state, &default_scope(&state), req)
}

/// `POST /groups/ungroup` — clear a group from every member in one write.
pub(crate) async fn ungroup(
    State(state): State<AppState>,
    Json(req): Json<UngroupReq>,
) -> Result<Json<UngroupResponse>, ApiError> {
    do_ungroup(&state, &default_scope(&state), req)
}

/// `POST /groups/merge` — move every member of one group into another.
pub(crate) async fn merge_groups(
    State(state): State<AppState>,
    Json(req): Json<MergeGroupsReq>,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    do_merge(&state, &default_scope(&state), req)
}

// ---- one hosted store ------------------------------------------------------------

fn store_scope(state: &AppState, store_id: &str) -> Result<Scope, ApiError> {
    state
        .host
        .get(store_id)
        .map(|entry| Scope {
            entries: vec![entry],
            registry: None,
        })
        .ok_or_else(|| ApiError::not_found(store_id))
}

pub(crate) async fn list_store_groups(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
) -> Result<Json<GroupsResponse>, ApiError> {
    do_list(&store_scope(&state, &store_id)?)
}

pub(crate) async fn rename_store_group(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
    Json(req): Json<RenameGroupReq>,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    do_rename(&state, &store_scope(&state, &store_id)?, req)
}

pub(crate) async fn ungroup_store(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
    Json(req): Json<UngroupReq>,
) -> Result<Json<UngroupResponse>, ApiError> {
    do_ungroup(&state, &store_scope(&state, &store_id)?, req)
}

pub(crate) async fn merge_store_groups(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
    Json(req): Json<MergeGroupsReq>,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    do_merge(&state, &store_scope(&state, &store_id)?, req)
}

// ---- one checkout: every connection with the `groups` capability -----------------

fn checkout_scope(state: &AppState, reference: &str) -> Result<Scope, ApiError> {
    let registry = checkout_registry(state, reference)?;
    Ok(Scope {
        entries: checkout_entries(state, reference)?
            .into_iter()
            .map(|(_, entry)| entry)
            .collect(),
        registry: Some(registry),
    })
}

/// `GET /checkouts/{reference}/groups` — the project's groups across its Git sources,
/// named by the checkout's registry.
pub(crate) async fn list_checkout_groups(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<GroupsResponse>, ApiError> {
    do_list(&checkout_scope(&state, &reference)?)
}

/// `POST /checkouts/{reference}/groups` — create (or find by name) a registry group.
pub(crate) async fn create_checkout_group(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<CreateGroupReq>,
) -> Result<Json<CreateGroupResponse>, ApiError> {
    do_create(&checkout_scope(&state, &reference)?, req)
}

pub(crate) async fn rename_checkout_group(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<RenameGroupReq>,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    do_rename(&state, &checkout_scope(&state, &reference)?, req)
}

pub(crate) async fn ungroup_checkout(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<UngroupReq>,
) -> Result<Json<UngroupResponse>, ApiError> {
    do_ungroup(&state, &checkout_scope(&state, &reference)?, req)
}

pub(crate) async fn merge_checkout_groups(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<MergeGroupsReq>,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    do_merge(&state, &checkout_scope(&state, &reference)?, req)
}

/// `POST /checkouts/{reference}/groups/migrate` — the explicit, idempotent migration of
/// legacy name values to registry ids, the same workflow as `hotsheet-cli group migrate`.
pub(crate) async fn migrate_checkout_groups(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    body: axum::body::Bytes,
) -> Result<Json<MigrateGroupsResponse>, ApiError> {
    // The body is optional: an empty request migrates with no actor.
    let req = if body.iter().all(u8::is_ascii_whitespace) {
        MigrateGroupsReq::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
    };
    do_migrate(&state, &checkout_scope(&state, &reference)?, req)
}

// ---- one provider connection -----------------------------------------------------

/// A hosted Git connection answers from its index; any other connection answers through
/// its provider, which refuses explicitly when it has no `groups` capability.
enum ConnectionScope {
    Hosted(Scope),
    Provider(std::sync::Arc<dyn hotsheet_ticketing::TicketProvider>),
}

fn connection_scope(state: &AppState, connection_id: &str) -> Result<ConnectionScope, ApiError> {
    match state.host.get(connection_id) {
        Some(entry) => Ok(ConnectionScope::Hosted(Scope {
            entries: vec![entry],
            registry: None,
        })),
        None => Ok(ConnectionScope::Provider(provider_for(
            state,
            connection_id,
        )?)),
    }
}

fn provider_directory(
    provider: &dyn hotsheet_ticketing::TicketProvider,
) -> Result<GroupDirectory, ApiError> {
    provider.group_directory().map_err(provider_transfer_error)
}

pub(crate) async fn list_provider_groups(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<GroupsResponse>, ApiError> {
    match connection_scope(&state, &connection_id)? {
        ConnectionScope::Hosted(scope) => do_list(&scope),
        ConnectionScope::Provider(provider) => Ok(Json(GroupsResponse {
            registry: false,
            groups: provider_directory(provider.as_ref())?.summaries(None),
        })),
    }
}

pub(crate) async fn rename_provider_group(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(req): Json<RenameGroupReq>,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    match connection_scope(&state, &connection_id)? {
        ConnectionScope::Hosted(scope) => do_rename(&state, &scope, req),
        ConnectionScope::Provider(provider) => {
            parse_actor(req.actor.as_ref())?;
            // A provider without groups refuses first; one with groups has no names.
            provider_directory(provider.as_ref())?;
            Err(group_error(GroupError::RegistryRequired(req.group)))
        }
    }
}

pub(crate) async fn ungroup_provider(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(req): Json<UngroupReq>,
) -> Result<Json<UngroupResponse>, ApiError> {
    match connection_scope(&state, &connection_id)? {
        ConnectionScope::Hosted(scope) => do_ungroup(&state, &scope, req),
        ConnectionScope::Provider(provider) => {
            parse_actor(req.actor.as_ref())?;
            let directory = provider_directory(provider.as_ref())?;
            let target = groups::find_group(&directory, None, &req.group).map_err(group_error)?;
            let write = provider
                .set_group_of_members(&directory, &target.key, None, "Ungroup tickets")
                .map_err(provider_transfer_error)?;
            Ok(Json(UngroupResponse {
                updated: write.updated,
            }))
        }
    }
}

pub(crate) async fn merge_provider_groups(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(req): Json<MergeGroupsReq>,
) -> Result<Json<GroupChangeResponse>, ApiError> {
    match connection_scope(&state, &connection_id)? {
        ConnectionScope::Hosted(scope) => do_merge(&state, &scope, req),
        ConnectionScope::Provider(provider) => {
            parse_actor(req.actor.as_ref())?;
            let directory = provider_directory(provider.as_ref())?;
            let from = groups::find_group(&directory, None, &req.from).map_err(group_error)?;
            let into = groups::find_group(&directory, None, &req.into).map_err(group_error)?;
            if from.key == into.key {
                return Err(group_error(GroupError::InvalidMerge(
                    "a group cannot be merged into itself",
                )));
            }
            if into.summary.legacy {
                return Err(group_error(GroupError::InvalidMerge(
                    "migrate legacy group names before merging into one",
                )));
            }
            let write = provider
                .set_group_of_members(
                    &directory,
                    &from.key,
                    Some(&into.summary.id),
                    "Merge ticket groups",
                )
                .map_err(provider_transfer_error)?;
            Ok(Json(GroupChangeResponse {
                id: into.summary.id,
                name: into.summary.name,
                updated: write.updated,
            }))
        }
    }
}
