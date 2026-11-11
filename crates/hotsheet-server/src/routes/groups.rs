//! Ticket group routes (HS2-PDSHNN, HS2-8CDFKR): list, rename, and ungroup, for the
//! default store, one hosted store, one provider connection, and a whole checkout.
//!
//! A checkout is one namespace: its groups are merged across every connection with the
//! `groups` capability (its Git sources), and a rename or ungroup rewrites the members in
//! each of them. Each store's members change in one locked write and one commit.

use crate::api_error::{ActorReq, ApiError, parse_actor};
use crate::multistore::StoreEntry;
use crate::routes::checkouts::checkout_entries;
use crate::routes::providers::{provider_for, provider_transfer_error};
use crate::{AppState, now};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_model::Ulid;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::groups::{
    self, GroupDirectory, GroupError, GroupSummary, known_group_key, plan_rename,
};
use serde::{Deserialize, Serialize};

/// `404`: no live ticket names the group.
pub(crate) const GROUP_NOT_FOUND_CODE: &str = "group_not_found";
/// `409`: the requested name is a different existing group.
pub(crate) const GROUP_NAME_TAKEN_CODE: &str = "group_name_taken";
/// `400`: the requested name is empty, too long, or holds control characters.
pub(crate) const INVALID_GROUP_NAME_CODE: &str = "invalid_group_name";

fn invalid_name(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, error.to_string()).with_code(INVALID_GROUP_NAME_CODE)
}

fn group_error(error: GroupError) -> ApiError {
    match error {
        GroupError::Store(error) => error.into(),
        GroupError::InvalidName(_) | GroupError::EmptyName => invalid_name(error),
        GroupError::UnknownGroup(_) => {
            ApiError::new(StatusCode::NOT_FOUND, error.to_string()).with_code(GROUP_NOT_FOUND_CODE)
        }
        GroupError::NameTaken(_) => {
            ApiError::new(StatusCode::CONFLICT, error.to_string()).with_code(GROUP_NAME_TAKEN_CODE)
        }
    }
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

/// Validate a requested group change and reuse the spelling the namespace already has.
/// `patched` are the tickets the caller is about to rewrite. The indexes are read only
/// when a name is being set.
pub(crate) fn resolve_for_write(
    entries: &[StoreEntry],
    change: Option<Option<String>>,
    patched: &[Ulid],
) -> Result<Option<Option<String>>, ApiError> {
    if !matches!(change, Some(Some(_))) {
        return Ok(change);
    }
    groups::resolve_group_change(&directory(entries)?, change, patched).map_err(invalid_name)
}

/// [`resolve_for_write`] over the Git sources of a checkout: one namespace per project.
pub(crate) fn resolve_for_checkout(
    state: &AppState,
    reference: &str,
    change: Option<Option<String>>,
    patched: &[Ulid],
) -> Result<Option<Option<String>>, ApiError> {
    if !matches!(change, Some(Some(_))) {
        return Ok(change);
    }
    resolve_for_write(&checkout_scope(state, reference)?, change, patched)
}

fn checkout_scope(state: &AppState, reference: &str) -> Result<Vec<StoreEntry>, ApiError> {
    Ok(checkout_entries(state, reference)?
        .into_iter()
        .map(|(_, entry)| entry)
        .collect())
}

#[derive(Debug, Serialize)]
pub(crate) struct GroupsResponse {
    groups: Vec<GroupSummary>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RenameGroupReq {
    from: String,
    to: String,
    /// Who is acting; validated like every other mutation's actor.
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RenameGroupResponse {
    name: String,
    updated: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UngroupReq {
    name: String,
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Serialize)]
pub(crate) struct UngroupResponse {
    updated: usize,
}

fn do_list(entries: &[StoreEntry]) -> Result<Json<GroupsResponse>, ApiError> {
    Ok(Json(GroupsResponse {
        groups: directory(entries)?.summaries(),
    }))
}

/// Rewrite the members of `from_key` in every store, publishing each store's change like
/// a batch update: index rows, change events, and one worklist rebuild per store.
fn write_members(
    state: &AppState,
    entries: &[StoreEntry],
    directory: &GroupDirectory,
    from_key: &str,
    to: Option<&str>,
) -> Result<usize, ApiError> {
    let mut updated = 0;
    for entry in entries {
        let write = groups::set_group_of_members(&entry.store, directory, from_key, to, now())?;
        updated += write.updated;
        if !write.tickets.is_empty() {
            let _ = state.changed_many_in(entry, "updated", &write.tickets);
        }
    }
    Ok(updated)
}

fn do_rename(
    state: &AppState,
    entries: &[StoreEntry],
    req: RenameGroupReq,
) -> Result<Json<RenameGroupResponse>, ApiError> {
    parse_actor(req.actor.as_ref())?;
    let directory = directory(entries)?;
    let plan = plan_rename(&directory, &req.from, &req.to).map_err(group_error)?;
    let updated = write_members(state, entries, &directory, &plan.from_key, Some(&plan.name))?;
    Ok(Json(RenameGroupResponse {
        name: plan.name,
        updated,
    }))
}

fn do_ungroup(
    state: &AppState,
    entries: &[StoreEntry],
    req: UngroupReq,
) -> Result<Json<UngroupResponse>, ApiError> {
    parse_actor(req.actor.as_ref())?;
    let directory = directory(entries)?;
    let key = known_group_key(&directory, &req.name).map_err(group_error)?;
    let updated = write_members(state, entries, &directory, &key, None)?;
    Ok(Json(UngroupResponse { updated }))
}

// ---- default store ---------------------------------------------------------------

/// `GET /groups` — the default store's groups with live member counts.
pub(crate) async fn list_groups(
    State(state): State<AppState>,
) -> Result<Json<GroupsResponse>, ApiError> {
    do_list(&[state.default_entry()])
}

/// `POST /groups/rename` — rename a group on every member in one write.
pub(crate) async fn rename_group(
    State(state): State<AppState>,
    Json(req): Json<RenameGroupReq>,
) -> Result<Json<RenameGroupResponse>, ApiError> {
    do_rename(&state, &[state.default_entry()], req)
}

/// `POST /groups/ungroup` — clear a group from every member in one write.
pub(crate) async fn ungroup(
    State(state): State<AppState>,
    Json(req): Json<UngroupReq>,
) -> Result<Json<UngroupResponse>, ApiError> {
    do_ungroup(&state, &[state.default_entry()], req)
}

// ---- one hosted store ------------------------------------------------------------

fn store_scope(state: &AppState, store_id: &str) -> Result<Vec<StoreEntry>, ApiError> {
    state
        .host
        .get(store_id)
        .map(|entry| vec![entry])
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
) -> Result<Json<RenameGroupResponse>, ApiError> {
    do_rename(&state, &store_scope(&state, &store_id)?, req)
}

pub(crate) async fn ungroup_store(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
    Json(req): Json<UngroupReq>,
) -> Result<Json<UngroupResponse>, ApiError> {
    do_ungroup(&state, &store_scope(&state, &store_id)?, req)
}

// ---- one checkout: every connection with the `groups` capability -----------------

/// `GET /checkouts/{reference}/groups` — the project's groups, counts merged by name
/// across its Git sources.
pub(crate) async fn list_checkout_groups(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<GroupsResponse>, ApiError> {
    do_list(&checkout_scope(&state, &reference)?)
}

pub(crate) async fn rename_checkout_group(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<RenameGroupReq>,
) -> Result<Json<RenameGroupResponse>, ApiError> {
    do_rename(&state, &checkout_scope(&state, &reference)?, req)
}

pub(crate) async fn ungroup_checkout(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<UngroupReq>,
) -> Result<Json<UngroupResponse>, ApiError> {
    do_ungroup(&state, &checkout_scope(&state, &reference)?, req)
}

// ---- one provider connection -----------------------------------------------------

/// A hosted Git connection answers from its index; any other connection answers through
/// its provider, which refuses explicitly when it has no `groups` capability.
enum ConnectionScope {
    Hosted(Vec<StoreEntry>),
    Provider(std::sync::Arc<dyn hotsheet_ticketing::TicketProvider>),
}

fn connection_scope(state: &AppState, connection_id: &str) -> Result<ConnectionScope, ApiError> {
    match state.host.get(connection_id) {
        Some(entry) => Ok(ConnectionScope::Hosted(vec![entry])),
        None => Ok(ConnectionScope::Provider(provider_for(
            state,
            connection_id,
        )?)),
    }
}

pub(crate) async fn list_provider_groups(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<GroupsResponse>, ApiError> {
    match connection_scope(&state, &connection_id)? {
        ConnectionScope::Hosted(entries) => do_list(&entries),
        ConnectionScope::Provider(provider) => Ok(Json(GroupsResponse {
            groups: provider
                .group_directory()
                .map_err(provider_transfer_error)?
                .summaries(),
        })),
    }
}

pub(crate) async fn rename_provider_group(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(req): Json<RenameGroupReq>,
) -> Result<Json<RenameGroupResponse>, ApiError> {
    match connection_scope(&state, &connection_id)? {
        ConnectionScope::Hosted(entries) => do_rename(&state, &entries, req),
        ConnectionScope::Provider(provider) => {
            parse_actor(req.actor.as_ref())?;
            let directory = provider
                .group_directory()
                .map_err(provider_transfer_error)?;
            let plan = plan_rename(&directory, &req.from, &req.to).map_err(group_error)?;
            let write = provider
                .set_group_of_members(&directory, &plan.from_key, Some(&plan.name), now())
                .map_err(provider_transfer_error)?;
            Ok(Json(RenameGroupResponse {
                name: plan.name,
                updated: write.updated,
            }))
        }
    }
}

pub(crate) async fn ungroup_provider(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(req): Json<UngroupReq>,
) -> Result<Json<UngroupResponse>, ApiError> {
    match connection_scope(&state, &connection_id)? {
        ConnectionScope::Hosted(entries) => do_ungroup(&state, &entries, req),
        ConnectionScope::Provider(provider) => {
            parse_actor(req.actor.as_ref())?;
            let directory = provider
                .group_directory()
                .map_err(provider_transfer_error)?;
            let key = known_group_key(&directory, &req.name).map_err(group_error)?;
            let write = provider
                .set_group_of_members(&directory, &key, None, now())
                .map_err(provider_transfer_error)?;
            Ok(Json(UngroupResponse {
                updated: write.updated,
            }))
        }
    }
}
