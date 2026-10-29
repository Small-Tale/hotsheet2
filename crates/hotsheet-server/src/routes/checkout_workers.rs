//! Authenticated, registered-checkout worker read model (HS2-DBECD0).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Json;
use axum::extract::{Path as RoutePath, Query, State};
use axum::http::StatusCode;
use hotsheet_cli::coordination::Coordinator;
use hotsheet_cli::coordination::worker_view::{ConnectionObservation, WorkerView};
use hotsheet_sync::LockExt;
use serde::Deserialize;

use crate::AppState;
use crate::api_error::ApiError;
use crate::routes::checkouts::checkout_lookup_error;

#[derive(Deserialize)]
pub(crate) struct WorkerQuery {
    connection_id: Option<String>,
    after: Option<String>,
    wait_ms: Option<u64>,
}

struct CancelWatch(Arc<AtomicBool>);

impl Drop for CancelWatch {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(crate) async fn checkout_workers(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Query(query): Query<WorkerQuery>,
) -> Result<Json<WorkerView>, ApiError> {
    // The request supplies only a registered checkout reference and linked source id.
    // Never accept caller-provided filesystem paths for this read.
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(checkout_lookup_error)?;
    let source_id = match query.connection_id {
        Some(id) => id,
        None => checkout
            .default_source
            .clone()
            .or_else(|| {
                let git: Vec<_> = checkout
                    .sources
                    .iter()
                    .filter(|s| s.provider == "git")
                    .collect();
                (git.len() == 1).then(|| git[0].connection_id.clone())
            })
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    "select a linked Git source with connection_id",
                )
            })?,
    };
    let source = checkout.source(&source_id).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "source is not linked to this checkout",
        )
    })?;
    if source.provider != "git" {
        return Err(ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "worker coordination is unavailable for this source",
        ));
    }
    let source_locator = source.locator.clone();
    let store = checkout
        .verified_git_source(&source_id)
        .map_err(checkout_lookup_error)?;
    let project = checkout.root.clone();
    let source = store.root().to_path_buf();
    let registry_home = state
        .checkout_registry
        .path()
        .parent()
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid checkout registry path",
            )
        })?
        .to_path_buf();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let wait_ms = query.wait_ms.unwrap_or(30_000);
    if wait_ms > 30_000 || query.after.as_ref().is_some_and(|after| after.len() > 160) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid worker cursor or wait_ms",
        ));
    }
    let after = query.after;
    // Subscribe before sampling both authorities so a registration between reads
    // remains visible to the following wait.
    let mut connection_changes = state.drive_registry.lock_or_recover().subscribe_changes();
    let mut terminal_changes = state.subscribe();
    let coordinator = tokio::task::spawn_blocking(move || {
        Coordinator::open_with_registry_home(Path::new(&project), &source, registry_home)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    let coordinator = std::sync::Arc::new(coordinator);
    let snapshot = coordinator.snapshot(now).map_err(worker_error)?;
    let (observations, _) = observe_connections(&state, &checkout).await?;
    let view = WorkerView::from_snapshot(&snapshot, now, &observations, true);
    if after.as_deref() != Some(&view.cursor) || wait_ms == 0 {
        revalidate(&state, &checkout, &source_id, &source_locator)?;
        return Ok(Json(view));
    }
    let after = after.expect("checked above");
    let coordinator_for_wait = coordinator.clone();
    let coordination_cursor = WorkerView::coordination_cursor(&after).to_owned();
    let cancellation = CancelWatch(Arc::new(AtomicBool::new(false)));
    let flag = cancellation.0.clone();
    let mut coordination_wait = tokio::task::spawn_blocking(move || {
        coordinator_for_wait.watch_worker_snapshot_with_cancel(
            &coordination_cursor,
            std::time::Duration::from_millis(wait_ms),
            Some(&flag),
        )
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
    loop {
        let (_, busy_expiry) = observe_connections(&state, &checkout).await?;
        let busy_delay = busy_expiry.map_or(std::time::Duration::from_millis(wait_ms), |at| {
            std::time::Duration::from_millis(at.saturating_sub(now_millis()))
        });
        let wake = tokio::select! {
            result = &mut coordination_wait => {
                result.map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                    .map_err(worker_error)?;
                0
            },
            result = connection_changes.changed() => if result.is_ok() { 1 } else { 3 },
            result = terminal_changes.recv() => match result {
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => 1,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => 3,
            },
            () = tokio::time::sleep(busy_delay) => 2,
            () = tokio::time::sleep_until(deadline) => 3,
        };
        if wake == 3 {
            revalidate(&state, &checkout, &source_id, &source_locator)?;
            return Ok(Json(view));
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let snapshot = coordinator.snapshot(now).map_err(worker_error)?;
        let (observations, _) = observe_connections(&state, &checkout).await?;
        let updated = WorkerView::from_snapshot(&snapshot, now, &observations, true);
        if updated.cursor != after || wake == 0 {
            revalidate(&state, &checkout, &source_id, &source_locator)?;
            return Ok(Json(updated));
        }
    }
}

fn revalidate(
    state: &AppState,
    original: &hotsheet_ticketing::checkouts::Checkout,
    source_id: &str,
    source_locator: &str,
) -> Result<(), ApiError> {
    let current = state
        .checkout_registry
        .resolve(&original.id)
        .map_err(checkout_lookup_error)?;
    if current.id != original.id
        || current.root != original.root
        || !current
            .source(source_id)
            .is_some_and(|source| source.provider == "git" && source.locator == source_locator)
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "checkout source changed; reconnect",
        ));
    }
    current
        .verified_git_source(source_id)
        .map_err(checkout_lookup_error)?;
    Ok(())
}

fn worker_error(error: anyhow::Error) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, error.to_string())
}

async fn observe_connections(
    state: &AppState,
    checkout: &hotsheet_ticketing::checkouts::Checkout,
) -> Result<(BTreeMap<String, ConnectionObservation>, Option<u64>), ApiError> {
    let (mut observations, busy_expiry) = {
        let registry = state.drive_registry.lock_or_recover();
        let now_ms = now_millis();
        let mut observations = BTreeMap::new();
        for connection in registry.list() {
            if !in_checkout(checkout, &connection.project) {
                continue;
            }
            observations.insert(
                connection.id.clone(),
                ConnectionObservation {
                    connection_id: connection.id.clone(),
                    tool: connection.tool.clone(),
                    kind: match connection.transport {
                        hotsheet_aitools::Transport::ClaudeChannel => "claude_channel",
                        hotsheet_aitools::Transport::Spawn => "spawn",
                        hotsheet_aitools::Transport::AppServer => "app_server",
                        hotsheet_aitools::Transport::Acp => "acp",
                        hotsheet_aitools::Transport::Pty => "pty",
                    }
                    .into(),
                    role: Some(
                        match connection.role {
                            hotsheet_aitools::Role::Main => "main",
                            hotsheet_aitools::Role::Worker => "worker",
                            hotsheet_aitools::Role::DriveSpawned => "drive_spawned",
                        }
                        .into(),
                    ),
                    busy: Some(registry.is_busy(&connection.id, now_ms)),
                    observed_alive: None,
                },
            );
        }
        (observations, registry.next_busy_expiry_ms(now_ms))
    };
    // A shell hook can opt into a registered checkout id. A terminal cwd is not
    // a project identity, especially when registered checkouts are nested.
    let terminal_infos = crate::routes::terminals::try_live_terminal_infos(state)
        .await
        .map_err(|error| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, error))?;
    let live_ids: std::collections::BTreeSet<_> = terminal_infos
        .into_iter()
        .filter(|info| info.alive)
        .map(|info| info.id)
        .collect();
    let hooks = state.terminal_ai_connections.lock_or_recover();
    for (id, hook) in hooks.iter() {
        if hook.checkout_id.as_deref() != Some(&checkout.id) || !live_ids.contains(id) {
            continue;
        }
        let Some(tool) = hook.agent.as_ref() else {
            continue;
        };
        let observation = observations
            .entry(id.clone())
            .or_insert_with(|| ConnectionObservation {
                connection_id: id.clone(),
                tool: tool.clone(),
                kind: "terminal_hook".into(),
                role: None,
                busy: None,
                observed_alive: Some(true),
            });
        observation.observed_alive = Some(true);
    }
    Ok((observations, busy_expiry))
}

fn in_checkout(checkout: &hotsheet_ticketing::checkouts::Checkout, project: &str) -> bool {
    // Host PTY connections created before this view carry the registered root as
    // their project. Exact equality preserves them without cwd-prefix inference.
    project == checkout.id || project == checkout.root
}

fn now_millis() -> u64 {
    u64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .unwrap_or_default()
}
