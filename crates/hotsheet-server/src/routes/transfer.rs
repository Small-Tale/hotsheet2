//! Cross-store copy / move (HS2-60 / HS2-S4H2AM).

use crate::*;

// ---- cross-store copy / move (HS2-60 / HS2-S4H2AM) -------------------------------

/// Body for copy: `to` names the hosted destination store (its URL id).
#[derive(Deserialize)]
pub(crate) struct CopyBody {
    pub(crate) to: String,
}

/// Body for move: destination store + the explicit `confirm` acknowledging that git
/// history in the source never forgets (the retention/exposure caveat, `docs/02` §2.13).
#[derive(Deserialize)]
pub(crate) struct MoveBody {
    pub(crate) to: String,
    #[serde(default)]
    pub(crate) confirm: bool,
}

/// A copy result: the new ticket + the destination store it now lives in.
#[derive(Serialize)]
pub(crate) struct CopyResult {
    /// URL id of the destination store.
    pub(crate) store: String,
    #[serde(flatten)]
    pub(crate) ticket: ApiTicket,
}

/// A move result: the live ticket (now in `store`), the source store it left, and the
/// tombstone slug left behind in the source.
#[derive(Serialize)]
pub(crate) struct MoveResult {
    /// URL id of the destination store (where the live ticket now is).
    pub(crate) store: String,
    /// URL id of the source store (which keeps a `moved` tombstone).
    pub(crate) source_store: String,
    /// Slug of the tombstone left in the source store.
    pub(crate) tombstone: String,
    #[serde(flatten)]
    pub(crate) ticket: ApiTicket,
}

/// `POST /tickets/{id}/copy` `{to:<store_id>}` — copy a default-store ticket into another
/// hosted store as a **new** ticket (new ULID, `copied_from` provenance). Source untouched.
pub(crate) async fn copy_ticket_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CopyBody>,
) -> Result<(StatusCode, Json<CopyResult>), ApiError> {
    let src = state.default_entry();
    let dest = scoped_entry(&state, &body.to)?;
    let ticket = ops::resolve(&src.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    let new = ops::copy_ticket(&src.store, &dest.store, &ticket.id, Ulid::new(), now())?;
    state.changed_in(&dest, "created", &new);
    Ok((
        StatusCode::CREATED,
        Json(CopyResult {
            store: multistore::store_url_id(&dest.store),
            ticket: api_ticket(&dest, &new)?,
        }),
    ))
}

/// `POST /tickets/{id}/move` `{to:<store_id>, confirm:true}` — move a default-store ticket
/// to another hosted store, keeping the same ULID and leaving a `moved` tombstone behind.
/// Requires `confirm:true` (the git-retention caveat); without it, 400.
pub(crate) async fn move_ticket_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<MoveResult>, ApiError> {
    if !body.confirm {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "move requires confirm=true: the source store's git history keeps the ticket \
             (and any attachments) even after the move — see docs/02 §2.13",
        ));
    }
    let src = state.default_entry();
    let dest = scoped_entry(&state, &body.to)?;
    let ticket = ops::resolve(&src.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    // Record the destination's canonical root as `moved_to_store` — the identity the
    // `StoreRegistry` follows when resolving the ULID to its live instance.
    let dest_id = StoreRegistry::store_id(&dest.store);
    let outcome = ops::move_ticket(&src.store, &dest.store, &ticket.id, &dest_id, now())?;
    state.changed_in(&dest, "created", &outcome.moved);
    state.changed_in(&src, "moved", &outcome.tombstone);
    Ok(Json(MoveResult {
        store: multistore::store_url_id(&dest.store),
        source_store: multistore::store_url_id(&src.store),
        tombstone: outcome.tombstone.slug.clone(),
        ticket: api_ticket(&dest, &outcome.moved)?,
    }))
}
