//! Milestone registry routes (HS2-9PYNCN, docs/38 §38.2, §38.6): list, show, create, edit,
//! close, and reopen the milestones of one checkout.
//!
//! Milestones belong to the project, so every route is checkout-scoped: the registry is
//! `<checkout>/.hotsheet2/milestones/<id>.json`, read and written through
//! `hotsheet_ticketing::milestones` like the CLI and MCP. Each write that changes a file is
//! committed by the background registry committer (a burst becomes one commit) and announced
//! as one `milestones_changed` event; a write that changes nothing announces nothing. Ticket
//! links, roll-ups, Delete Milestone…, and Move Open Tickets to… come with HS2-A820C1.

use crate::AppState;
use crate::api_error::{ActorReq, ApiError, parse_actor};
use crate::routes::checkouts::checkout_lookup_error;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hotsheet_ticketing::milestones::{
    self, CloseReason, Milestone, MilestoneEdit, MilestoneError, MilestoneListing,
    MilestoneRegistry, MilestoneWrite,
};
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
    milestones::listing(Some(&registry), query.all)
        .map(Json)
        .map_err(milestone_error)
}

/// `POST /checkouts/{reference}/milestones` — create an open milestone.
pub(crate) async fn create_checkout_milestone(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<CreateMilestoneReq>,
) -> Result<(StatusCode, Json<MilestoneWrite>), ApiError> {
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
    Ok((StatusCode::CREATED, Json(write)))
}

/// `GET /checkouts/{reference}/milestones/{milestone}` — one milestone by id or title.
pub(crate) async fn get_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
) -> Result<Json<Milestone>, ApiError> {
    registry(&state, &reference)?
        .resolve(&milestone)
        .map(Json)
        .map_err(milestone_error)
}

/// `PATCH /checkouts/{reference}/milestones/{milestone}` — change the title, description,
/// or due date. A change to nothing returns `changed: false` and commits nothing.
pub(crate) async fn edit_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
    Json(req): Json<EditMilestoneReq>,
) -> Result<Json<MilestoneWrite>, ApiError> {
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
    Ok(Json(write))
}

/// `POST /checkouts/{reference}/milestones/{milestone}/close` — close as `completed` (the
/// default) or `not_planned`. Closing with open tickets is allowed.
pub(crate) async fn close_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Json<MilestoneWrite>, ApiError> {
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
    Ok(Json(write))
}

/// `POST /checkouts/{reference}/milestones/{milestone}/reopen` — reopen a closed milestone.
pub(crate) async fn reopen_checkout_milestone(
    State(state): State<AppState>,
    Path((reference, milestone)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Json<MilestoneWrite>, ApiError> {
    let req: ReopenMilestoneReq = optional_body(&body)?;
    parse_actor(req.actor.as_ref())?;
    let registry = registry(&state, &reference)?;
    let id = registry.resolve(&milestone).map_err(milestone_error)?.id;
    let write = registry.reopen(&id).map_err(milestone_error)?;
    announce(&state, &registry, &write);
    Ok(Json(write))
}
