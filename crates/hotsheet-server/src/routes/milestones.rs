//! Milestone routes (HS2-9PYNCN, HS2-A820C1, docs/38 §38.2-§38.6): list and show with
//! roll-ups, create, edit, close, reopen, delete, and move open tickets, for the milestones of
//! one checkout; plus the ticket-link resolution the ticket routes share.
//!
//! Milestones belong to the project, so every route is checkout-scoped: the registry is
//! `<checkout>/.hotsheet2/milestones/<id>.json`, read and written through
//! `hotsheet_ticketing::milestones` like the CLI and MCP. Roll-ups read every member across
//! the checkout's Git stores from their indexes, in the project time zone. Each write that
//! changes a file is committed by the background registry committer (a burst becomes one
//! commit) and announced as one `milestones_changed` event; tickets a delete or move-open
//! rewrites are announced as ticket changes naming their old and new milestone. A write
//! that changes nothing announces nothing.

use crate::AppState;
use crate::api_error::{ActorReq, ApiError, parse_actor};
use crate::dto::ListParams;
use crate::routes::checkouts::{checkout_entries, checkout_lookup_error};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::milestone_project::{
    self as project, MemberOutcome, MilestoneListing, MilestoneOpError, MilestoneSummary,
    MilestoneWriteResult, ProjectMembers, RollupClock,
};
use hotsheet_ticketing::milestones::{
    CloseReason, MilestoneEdit, MilestoneError, MilestoneRegistry, MilestoneWrite,
};
use hotsheet_ticketing::{TicketCollection, TicketQuery};
use serde::Deserialize;

/// Map a registry refusal to its HTTP status, keeping the stable code.
pub(crate) fn milestone_error(error: MilestoneError) -> ApiError {
    let status = match &error {
        MilestoneError::Io { .. } => {
            return ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
        }
        MilestoneError::InvalidTitle(_)
        | MilestoneError::InvalidDueOn(_)
        | MilestoneError::InvalidCloseReason(_)
        | MilestoneError::RegistryRequired => StatusCode::BAD_REQUEST,
        MilestoneError::NotFound(_) => StatusCode::NOT_FOUND,
        MilestoneError::Ambiguous(_)
        | MilestoneError::TitleTaken { .. }
        | MilestoneError::Conflict { .. } => StatusCode::CONFLICT,
    };
    let code = error.code();
    ApiError::new(status, error.to_string()).with_code(code)
}

fn op_error(error: MilestoneOpError) -> ApiError {
    match error {
        MilestoneOpError::Milestone(error) => milestone_error(error),
        MilestoneOpError::Store(error) => error.into(),
    }
}

/// Validate a ticket's milestone on a route with no project registry (a store or the
/// default store): ids only (`milestone_registry_required` for a title).
pub(crate) fn resolve_link_for_store(
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>, ApiError> {
    project::resolve_link(None, change).map_err(milestone_error)
}

/// Resolve a ticket's milestone through the checkout's registry: an id or a title; an
/// unknown title is `milestone_not_found` (a link never creates a milestone).
pub(crate) fn resolve_link_for_checkout(
    state: &AppState,
    reference: &str,
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>, ApiError> {
    if !matches!(change, Some(Some(_))) {
        return Ok(change);
    }
    let registry = registry(state, reference)?;
    project::resolve_link(Some(&registry), change).map_err(milestone_error)
}

/// Validate a ticket's milestone for a provider-connection write. A connection has no
/// registry, so a capable provider takes an id; a provider without the capability gets the
/// value unchanged and refuses it itself before any request.
pub(crate) fn validate_for_connection(
    provider: &dyn hotsheet_ticketing::TicketProvider,
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>, ApiError> {
    if provider.descriptor().capabilities.milestones {
        resolve_link_for_store(change)
    } else {
        Ok(change)
    }
}

/// Resolve a checkout list's `milestone` filter to registry ids, and its `due_before`
/// bound to the open milestones due by then (docs/38 §38.3 effective due).
pub(crate) fn resolve_list_filters(
    state: &AppState,
    reference: &str,
    params: &mut ListParams,
) -> Result<(), ApiError> {
    if params.milestone.is_none() && params.due_before.is_none() {
        return Ok(());
    }
    let registry = registry(state, reference)?;
    if let Some(input) = params.milestone.clone() {
        params.milestone_ids = Some(project::filter_ids(Some(&registry), &input));
    }
    if let Some(date) = params.due_before.clone() {
        params.due_milestone_ids = project::due_milestone_ids(Some(&registry), date.trim());
    }
    Ok(())
}

/// Every member of the checkout's milestones, read from its Git stores' indexes, with the
/// stores themselves for writes. A store whose index cannot be read is named unavailable.
fn members(
    state: &AppState,
    reference: &str,
) -> Result<(ProjectMembers, Vec<crate::multistore::StoreEntry>), ApiError> {
    let entries: Vec<crate::multistore::StoreEntry> = checkout_entries(state, reference)?
        .into_iter()
        .map(|(_, entry)| entry)
        .collect();
    let mut members = ProjectMembers::default();
    for entry in &entries {
        let connection_id = hotsheet_ticketing::git_connection_id(&entry.store);
        let read = {
            let index = entry.index.lock_or_recover();
            index.milestone_members().and_then(|rows| {
                let unfinished = index.query(&TicketQuery {
                    collection: Some(TicketCollection::All),
                    open_only: true,
                    ..TicketQuery::default()
                })?;
                Ok((rows, unfinished))
            })
        };
        match read {
            Ok((rows, unfinished)) => members.add_rows(
                &connection_id,
                rows,
                unfinished.into_iter().map(|row| row.id),
            ),
            Err(error) => {
                tracing::warn!(store = %connection_id, %error, "milestone members unreadable");
                members.unavailable.push(connection_id);
            }
        }
    }
    Ok((members, entries))
}

/// The project time zone in effect, at the current instant.
fn clock(state: &AppState, reference: &str) -> Result<RollupClock, ApiError> {
    let (_, settings) = crate::routes::project_tools::checkout_settings(state, reference)?;
    let zone = hotsheet_ticketing::project_time_zone::project_time_zone(&settings)
        .unwrap_or_else(|_| hotsheet_ticketing::project_time_zone::ProjectTimeZone::utc());
    Ok(RollupClock::now(zone))
}

/// Index and announce each store's rewritten tickets, then the registry change.
fn announce_members(
    state: &AppState,
    registry: &MilestoneRegistry,
    entries: &[crate::multistore::StoreEntry],
    outcome: &MemberOutcome,
) {
    for (entry, write) in entries.iter().zip(&outcome.writes) {
        if !write.tickets.is_empty() {
            state.changed_many_in(entry, "updated", &write.tickets);
        }
    }
    if outcome.changed {
        state.announce_milestones_changed(registry.checkout_root());
    }
}

fn registry(state: &AppState, reference: &str) -> Result<MilestoneRegistry, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    Ok(MilestoneRegistry::for_checkout(std::path::Path::new(
        &checkout.root,
    )))
}

/// `?all=true` adds closed milestones.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListMilestonesQuery {
    #[serde(default)]
    all: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMilestoneReq {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    due_on: Option<String>,
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditMilestoneReq {
    #[serde(default)]
    title: Option<String>,
    /// Set, clear with JSON null, or leave unchanged when absent.
    #[serde(default, deserialize_with = "crate::dto::deserialize_nullable_patch")]
    description: Option<Option<String>>,
    /// Set, clear with JSON null, or leave unchanged when absent.
    #[serde(default, deserialize_with = "crate::dto::deserialize_nullable_patch")]
    due_on: Option<Option<String>>,
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CloseMilestoneReq {
    /// `completed` (the default) or `not_planned`.
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReopenMilestoneReq {
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MoveOpenReq {
    /// The milestone to move the open tickets to: an id or a title.
    to: String,
    #[serde(default)]
    actor: Option<ActorReq>,
}

fn write_response(
    state: &AppState,
    reference: &str,
    write: MilestoneWrite,
) -> Result<MilestoneWriteResult, ApiError> {
    let (members, _) = members(state, reference)?;
    Ok(MilestoneWriteResult::of(
        write,
        &members,
        &clock(state, reference)?,
    ))
}

/// A JSON body that may be empty (close and reopen need no fields).
fn optional_body<T: Default + serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(body)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, format!("invalid body: {error}")))
}

/// Announce a write that changed the registry to every client of the checkout.
fn announce(state: &AppState, registry: &MilestoneRegistry, write: &MilestoneWrite) {
    if write.changed {
        state.announce_milestones_changed(registry.checkout_root());
    }
}

/// `GET /checkouts/{reference}/milestones` — open and conflicted milestones, with
/// `?all=true` closed ones too, plus the registry's publication status.
pub(crate) async fn list_checkout_milestones(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(query): Query<ListMilestonesQuery>,
) -> Result<Json<MilestoneListing>, ApiError> {
    let registry = registry(&state, &reference)?;
    let (members, _) = members(&state, &reference)?;
    project::listing(
        Some(&registry),
        query.all,
        &members,
        &clock(&state, &reference)?,
    )
    .map(Json)
    .map_err(milestone_error)
}

/// `POST /checkouts/{reference}/milestones` — create an open milestone.
pub(crate) async fn create_checkout_milestone(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<CreateMilestoneReq>,
) -> Result<(StatusCode, Json<MilestoneWriteResult>), ApiError> {
    parse_actor(req.actor.as_ref())?;
    let registry = registry(&state, &reference)?;
    let milestone = registry
        .create(
            &req.title,
            req.description.as_deref(),
            req.due_on.as_deref(),
        )
        .map_err(milestone_error)?;
    let write = MilestoneWrite {
        milestone,
        changed: true,
    };
    announce(&state, &registry, &write);
    Ok((
        StatusCode::CREATED,
        Json(write_response(&state, &reference, write)?),
    ))
}

/// `GET /checkouts/{reference}/milestones/{milestone}` — one milestone by id or title with
/// its roll-up; an id that tickets name but the registry does not reads as unknown.
pub(crate) async fn get_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
) -> Result<Json<MilestoneSummary>, ApiError> {
    let registry = registry(&state, &reference)?;
    let (members, _) = members(&state, &reference)?;
    project::show(
        Some(&registry),
        &members,
        &milestone,
        &clock(&state, &reference)?,
    )
    .map(Json)
    .map_err(milestone_error)
}

/// `DELETE /checkouts/{reference}/milestones/{milestone}` — Delete Milestone…: clear the
/// link from every member (one write and commit per store), then delete the registry file.
pub(crate) async fn delete_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Json<MemberOutcome>, ApiError> {
    let req: ReopenMilestoneReq = optional_body(&body)?;
    parse_actor(req.actor.as_ref())?;
    let registry = registry(&state, &reference)?;
    let (members, entries) = members(&state, &reference)?;
    let stores: Vec<&hotsheet_ticketing::FsStore> =
        entries.iter().map(|entry| &entry.store).collect();
    let outcome = project::delete_milestone(Some(&registry), &stores, &members, &milestone)
        .map_err(op_error)?;
    announce_members(&state, &registry, &entries, &outcome);
    Ok(Json(outcome))
}

/// `POST /checkouts/{reference}/milestones/{milestone}/move-open` — Move Open Tickets to…:
/// relink every unfinished member to `to`, one write per store.
pub(crate) async fn move_open_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
    Json(req): Json<MoveOpenReq>,
) -> Result<Json<MemberOutcome>, ApiError> {
    parse_actor(req.actor.as_ref())?;
    let registry = registry(&state, &reference)?;
    let (members, entries) = members(&state, &reference)?;
    let stores: Vec<&hotsheet_ticketing::FsStore> =
        entries.iter().map(|entry| &entry.store).collect();
    let outcome =
        project::move_open_members(Some(&registry), &stores, &members, &milestone, &req.to)
            .map_err(op_error)?;
    // Moving tickets changes no registry file: announce the tickets, and the roll-ups of
    // both milestones through one registry event so clients refetch once.
    announce_members(&state, &registry, &entries, &outcome);
    Ok(Json(outcome))
}

/// `PATCH /checkouts/{reference}/milestones/{milestone}` — change the title, description,
/// or due date. A change to nothing returns `changed: false` and commits nothing.
pub(crate) async fn edit_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
    Json(req): Json<EditMilestoneReq>,
) -> Result<Json<MilestoneWriteResult>, ApiError> {
    parse_actor(req.actor.as_ref())?;
    let registry = registry(&state, &reference)?;
    let id = registry.resolve(&milestone).map_err(milestone_error)?.id;
    let write = registry
        .edit(
            &id,
            &MilestoneEdit {
                title: req.title,
                description: req.description,
                due_on: req.due_on,
            },
        )
        .map_err(milestone_error)?;
    announce(&state, &registry, &write);
    Ok(Json(write_response(&state, &reference, write)?))
}

/// `POST /checkouts/{reference}/milestones/{milestone}/close` — close as `completed` (the
/// default) or `not_planned`. Closing with open tickets is allowed.
pub(crate) async fn close_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Json<MilestoneWriteResult>, ApiError> {
    let req: CloseMilestoneReq = optional_body(&body)?;
    parse_actor(req.actor.as_ref())?;
    let reason = req
        .reason
        .as_deref()
        .map(CloseReason::parse)
        .transpose()
        .map_err(milestone_error)?
        .unwrap_or_default();
    let registry = registry(&state, &reference)?;
    let id = registry.resolve(&milestone).map_err(milestone_error)?.id;
    let write = registry.close(&id, reason).map_err(milestone_error)?;
    announce(&state, &registry, &write);
    Ok(Json(write_response(&state, &reference, write)?))
}

/// `POST /checkouts/{reference}/milestones/{milestone}/reopen` — reopen a closed milestone.
pub(crate) async fn reopen_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Json<MilestoneWriteResult>, ApiError> {
    let req: ReopenMilestoneReq = optional_body(&body)?;
    parse_actor(req.actor.as_ref())?;
    let registry = registry(&state, &reference)?;
    let id = registry.resolve(&milestone).map_err(milestone_error)?.id;
    let write = registry.reopen(&id).map_err(milestone_error)?;
    announce(&state, &registry, &write);
    Ok(Json(write_response(&state, &reference, write)?))
}
