//! Persisted live state transitions and their REST read (HS2-9AN4DN, docs/34-glance-surfaces.md
//! §34.8). Each published snapshot whose overnight state differs from the last one recorded
//! appends a record through [`hotsheet_ticketing::live_history`]; `GET /live-history` replays
//! every hosted store's records into segments and adds the completions inside the window.

use crate::AppState;
use crate::api_error::ApiError;
use crate::live_state::{LiveStoreState, LiveWord};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::live_history::{
    HistoryCompletion, HistorySegment, HistoryState, append_transition, completions_between,
    history_path, read_records, segments,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// The default window: the last 12 hours.
const DEFAULT_WINDOW_MS: i64 = 12 * 60 * 60 * 1000;

/// The state last recorded per store root in this process, so an unchanged snapshot reads nothing.
#[derive(Default)]
pub(crate) struct LiveHistory {
    last: Mutex<HashMap<String, HistoryState>>,
}

/// A snapshot's overnight state: its word, or Waiting on CI while every live claim of an
/// otherwise active store is in its final remote-CI phase.
#[must_use]
pub(crate) fn history_state(snapshot: &LiveStoreState) -> HistoryState {
    match snapshot.state {
        LiveWord::Active
            if !snapshot.claims.is_empty()
                && snapshot
                    .claims
                    .iter()
                    .all(|claim| claim.phase.as_deref() == Some("final_testing")) =>
        {
            HistoryState::WaitingOnCi
        }
        LiveWord::Active => HistoryState::Active,
        LiveWord::Stopped => HistoryState::Stopped,
        LiveWord::MayBeStuck => HistoryState::MayBeStuck,
        LiveWord::Stalled => HistoryState::Stalled,
        LiveWord::Halted => HistoryState::Halted,
        LiveWord::WaitingOnYou => HistoryState::WaitingOnYou,
    }
}

/// Record `snapshot`'s state at `at_ms` when it differs from the last one recorded. A failure is
/// logged and retried on the next publish; history never blocks a live-state push.
pub(crate) fn observe(state: &AppState, snapshot: &LiveStoreState, at_ms: i64) {
    let next = history_state(snapshot);
    let mut last = state.live_history.last.lock_or_recover();
    let known = last.get(&snapshot.root).copied();
    if known == Some(next) {
        return;
    }
    // The history is machine-local: keep `metrics/local/` out of the store's Git history.
    if known.is_none() {
        if let Some(entry) = state.host.get(&snapshot.store) {
            if let Err(error) =
                hotsheet_ticketing::metrics::ensure_gitignored(&entry.store, "metrics/local/")
            {
                tracing::warn!(store = %snapshot.store, %error, "could not ignore metrics/local");
            }
        }
    }
    match append_transition(&history_path(Path::new(&snapshot.root)), at_ms, next) {
        Ok(_) => {
            last.insert(snapshot.root.clone(), next);
        }
        Err(error) => {
            tracing::warn!(store = %snapshot.store, %error, "could not record a live state transition");
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct LiveHistoryParams {
    since_ms: Option<i64>,
    until_ms: Option<i64>,
}

/// One store's lane: its segments and completion ticks inside the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiveHistoryStore {
    pub store: String,
    pub name: String,
    pub checkouts: Vec<String>,
    pub segments: Vec<HistorySegment>,
    pub completions: Vec<HistoryCompletion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiveHistoryList {
    pub since_ms: i64,
    pub until_ms: i64,
    pub stores: Vec<LiveHistoryStore>,
}

fn unix_ms(at: time::OffsetDateTime) -> i64 {
    i64::try_from(at.unix_timestamp_nanos() / 1_000_000).unwrap_or(i64::MAX)
}

/// Resolve the requested window: default the last 12 hours, never longer than the retention.
fn window(params: &LiveHistoryParams, now_ms: i64) -> Result<(i64, i64), ApiError> {
    let until = params.until_ms.unwrap_or(now_ms).min(now_ms);
    let since = params.since_ms.unwrap_or(until - DEFAULT_WINDOW_MS);
    if since >= until {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "since_ms must be before until_ms",
        ));
    }
    Ok((
        since.max(until - hotsheet_ticketing::live_history::RETENTION_MS),
        until,
    ))
}

fn build(state: &AppState, since: i64, until: i64) -> Result<LiveHistoryList, ApiError> {
    let live = crate::live_state::snapshot_all(state);
    let mut stores = Vec::new();
    for snapshot in live.stores {
        let records = read_records(&history_path(Path::new(&snapshot.root)))
            .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        let tickets = state
            .host
            .get(&snapshot.store)
            .and_then(|entry| entry.store.list_tickets_resilient().ok())
            .map(|listed| listed.tickets)
            .unwrap_or_default();
        stores.push(LiveHistoryStore {
            segments: segments(&records, since, until),
            completions: completions_between(&tickets, since, until),
            store: snapshot.store,
            name: snapshot.name,
            checkouts: snapshot.checkouts,
        });
    }
    Ok(LiveHistoryList {
        since_ms: since,
        until_ms: until,
        stores,
    })
}

/// `GET /live-history?since_ms&until_ms` — every hosted store's recorded state segments and
/// completion ticks inside the window (HS2-9AN4DN). Default: the last 12 hours.
pub(crate) async fn get_live_history(
    State(state): State<AppState>,
    Query(params): Query<LiveHistoryParams>,
) -> Result<Json<LiveHistoryList>, ApiError> {
    let (since, until) = window(&params, unix_ms(time::OffsetDateTime::now_utc()))?;
    tokio::task::spawn_blocking(move || build(&state, since, until).map(Json))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_9an4dn_window_defaults_clamps_and_rejects_inverted_ranges() {
        let now = 100 * 24 * 60 * 60 * 1000;
        let params = |since_ms, until_ms| LiveHistoryParams { since_ms, until_ms };
        assert_eq!(
            window(&params(None, None), now).unwrap(),
            (now - DEFAULT_WINDOW_MS, now)
        );
        // The future is clipped to now; a window past the retention is clipped to it.
        assert_eq!(
            window(&params(Some(0), Some(now + 5)), now).unwrap(),
            (now - hotsheet_ticketing::live_history::RETENTION_MS, now)
        );
        assert!(window(&params(Some(now), Some(now)), now).is_err());
    }
}
