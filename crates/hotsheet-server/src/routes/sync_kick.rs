//! `POST /sync/kick` (HS2-D9ZGAE): a CLI write asks the running host to sync the ticket
//! store it just committed to (`docs/02-ticket-storage.md` §2.12). The CLI's own best-effort
//! `git push` cannot fetch or rebase, so a store whose remote moved on would otherwise keep
//! its commits local until a client opened the project. The host accepts only a store it
//! already syncs (a hosted store or a Git store linked to a registered checkout) and answers
//! at once; the sync itself runs on the background sync loop, coalesced with other kicks
//! and with its regular pass.
//!
//! An accepted kick also announces the store's current sync health right away (HS2-PQ7K4T).
//! `hotsheet-cli sync` kicks after it syncs a store itself, and a loop backed off by a refused
//! push ignores kicks until its deadline, so without this an open client kept showing the
//! "not publishing" strip for minutes after the store published.

use std::path::PathBuf;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api_error::ApiError;
use crate::sync_loop::StoreSyncRequest;

/// The store a CLI write committed to.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyncKickRequest {
    store: PathBuf,
}

/// The accepted request: the store's canonical root.
#[derive(Debug, Serialize)]
pub(crate) struct SyncKickAccepted {
    accepted: bool,
    store: String,
}

/// A refused kick's stable machine-readable code: the store is not registered with this host.
pub(crate) const STORE_NOT_REGISTERED_CODE: &str = "store_not_registered";
/// A refused kick's stable machine-readable code: this host runs no background sync loop.
pub(crate) const SYNC_NOT_RUNNING_CODE: &str = "sync_not_running";

pub(crate) async fn post_sync_kick(
    State(state): State<AppState>,
    Json(request): Json<SyncKickRequest>,
) -> Result<(StatusCode, Json<SyncKickAccepted>), ApiError> {
    if !request.store.is_absolute() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "store must be an absolute path",
        ));
    }
    match state.request_store_sync(&request.store) {
        StoreSyncRequest::Accepted => {
            // The health read runs git; keep it off the async workers (HS2-PQ7K4T).
            let (announcer, store) = (state.clone(), request.store.clone());
            drop(tokio::task::spawn_blocking(move || {
                announce_current_health(&announcer, &store);
            }));
            Ok((
                StatusCode::ACCEPTED,
                Json(SyncKickAccepted {
                    accepted: true,
                    store: request
                        .store
                        .canonicalize()
                        .unwrap_or(request.store)
                        .display()
                        .to_string(),
                }),
            ))
        }
        StoreSyncRequest::NotRegistered => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "this host does not sync that store: link it to a registered checkout",
        )
        .with_code(STORE_NOT_REGISTERED_CODE)),
        StoreSyncRequest::NotRunning => Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "this host runs no background ticket-store sync",
        )
        .with_code(SYNC_NOT_RUNNING_CODE)),
    }
}

/// Tell open clients the kicked store's current sync health (HS2-PQ7K4T): one
/// `ticket_store_sync` event per checkout linking it, from a few local reads. A client
/// re-reads the store's health on that event, so a sync the CLI ran itself is shown at once.
fn announce_current_health(state: &AppState, store: &std::path::Path) {
    let root = store.canonicalize().unwrap_or_else(|_| store.to_path_buf());
    let id = state
        .hosted_store_roots()
        .into_iter()
        .find(|(_, hosted)| crate::watcher::same_path(std::path::Path::new(hosted), &root))
        .map_or_else(|| root.display().to_string(), |(id, _)| id);
    let health = hotsheet_ticketing::sync_health::health(&root);
    state.announce_ticket_store_sync(&id, &root, health.state);
}
