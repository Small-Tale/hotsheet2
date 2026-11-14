//! Authenticated, registered-checkout worker read model (HS2-DBECD0).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Json;
use axum::extract::{Path as RoutePath, Query, State};
use axum::http::StatusCode;
use hotsheet_cli::coordination::development_strategy::{DevelopmentPolicy, DevelopmentReport};
use hotsheet_cli::coordination::strategy::StrategyReport;
use hotsheet_cli::coordination::worker_view::{ConnectionObservation, WorkerView};
use hotsheet_cli::coordination::{
    Assignment, Coordinator, NextPlanEntry, PlanEvent, StrategyPolicy,
};
use hotsheet_sync::LockExt;
use serde::Deserialize;
use serde::Serialize;

use crate::AppState;
use crate::api_error::ApiError;
use crate::routes::checkouts::checkout_lookup_error;

#[derive(Deserialize)]
pub(crate) struct WorkerQuery {
    connection_id: Option<String>,
    after: Option<String>,
    wait_ms: Option<u64>,
    /// Also list past workers (HS2-V9BXWF); the default view reports only their count.
    #[serde(default)]
    include_past: bool,
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
    let project = if query.include_past {
        WorkerView::from_snapshot_including_past
    } else {
        WorkerView::from_snapshot
    };
    let view = project(&snapshot, now, &observations, true);
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
        let updated = project(&snapshot, now, &observations, true);
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

/// The checkout's committed integration strategy policy and the strategy registry
/// (HS2-NEY1QH); `coordination strategies` prints the same report.
pub(crate) async fn get_integration_strategies(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
) -> Result<Json<StrategyReport>, ApiError> {
    let (_, settings) = crate::routes::project_tools::checkout_settings(&state, &reference)?;
    let policy = StrategyPolicy::read(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(policy.report()))
}

/// Replace the policy through the same validated write as the `coordination strategy-*`
/// commands. A live integrator keeps its recorded strategy; the policy applies to the next
/// registration of a new integrator session.
pub(crate) async fn put_integration_strategies(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Json(policy): Json<StrategyPolicy>,
) -> Result<Json<StrategyReport>, ApiError> {
    let (checkout, settings) = crate::routes::project_tools::checkout_settings(&state, &reference)?;
    let checkout_id = checkout.id;
    policy
        .write(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    let policy = StrategyPolicy::read(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    note_strategy_change(&state, &checkout_id);
    Ok(Json(policy.report()))
}

/// Shared versioned development policy. The CLI and REST handlers call the same
/// read, resolve and write operations (HS2-BF79YX).
pub(crate) async fn get_development_strategy(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Query(query): Query<DevelopmentStrategyQuery>,
) -> Result<Json<DevelopmentReport>, ApiError> {
    let (_, settings) = crate::routes::project_tools::checkout_settings(&state, &reference)?;
    let policy = DevelopmentPolicy::read(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    let integration = StrategyPolicy::read(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    let mut report = policy
        .report(&integration)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    report.effective = policy
        .resolve_named(
            &integration,
            query.session.as_deref(),
            query.ticket.as_deref(),
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(report))
}

#[derive(Deserialize)]
pub(crate) struct DevelopmentStrategyQuery {
    session: Option<String>,
    ticket: Option<String>,
}

pub(crate) async fn put_development_strategy(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Json(policy): Json<DevelopmentPolicy>,
) -> Result<Json<DevelopmentReport>, ApiError> {
    let (checkout, settings) = crate::routes::project_tools::checkout_settings(&state, &reference)?;
    if policy.default_custom.is_some() {
        policy
            .validate_project_capabilities(std::path::Path::new(&checkout.root))
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    }
    policy
        .write(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    let policy = DevelopmentPolicy::read(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    let integration = StrategyPolicy::read(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    note_strategy_change(&state, &checkout.id);
    Ok(Json(policy.report(&integration).map_err(|error| {
        ApiError::new(StatusCode::CONFLICT, error.to_string())
    })?))
}

/// HS2-A8HP69: the change-event kind a strategy write publishes. `id` is the checkout.
pub(crate) const STRATEGY_CHANGE_KIND: &str = "development_strategy_changed";

/// HS2-A8HP69: a strategy write changes the Workers view's cadence. A waiting Workers long
/// poll wakes on this event and reads again, so the new cadence no longer waits for the
/// platform file watcher to report the settings file, which a busy machine delivers late.
/// Identifiers only; a client reads again.
fn note_strategy_change(state: &AppState, checkout_id: &str) {
    state.emit(crate::ChangeEvent {
        live: None,
        cursor: None,
        store: checkout_id.to_owned(),
        kind: STRATEGY_CHANGE_KIND.into(),
        id: checkout_id.to_owned(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
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

#[derive(Deserialize)]
pub(crate) struct PlanReplaceBody {
    authority: String,
    generation: String,
    expected_revision: u64,
    entries: Vec<NextPlanEntry>,
    #[serde(default)]
    all_participants_upgraded: bool,
    connection_id: String,
}

#[derive(Serialize)]
pub(crate) struct PlanRevision {
    plan_revision: u64,
}

#[derive(Deserialize)]
pub(crate) struct PlanEventsQuery {
    connection_id: String,
    after_revision: u64,
}

#[derive(Deserialize)]
pub(crate) struct PlanDispatchBody {
    authority: String,
    generation: String,
    expected_revision: u64,
    operation: String,
    base: String,
    connection_id: String,
}

fn registered_coordinator(
    state: &AppState,
    reference: &str,
    source_id: &str,
) -> Result<Coordinator, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    let source = checkout.source(source_id).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "source is not linked to this checkout",
        )
    })?;
    if source.provider != "git" {
        return Err(ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "worker planning is unavailable for this source",
        ));
    }
    let store = checkout
        .verified_git_source(source_id)
        .map_err(checkout_lookup_error)?;
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
    Coordinator::open_with_registry_home(Path::new(&checkout.root), store.root(), registry_home)
        .map_err(worker_error)
}

pub(crate) async fn replace_worker_plan(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Json(body): Json<PlanReplaceBody>,
) -> Result<Json<PlanRevision>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let coordinator = registered_coordinator(&state, &reference, &body.connection_id)?;
        let revision = coordinator
            .replace_next_plan(
                &body.authority,
                &body.generation,
                &|| time::OffsetDateTime::now_utc().unix_timestamp(),
                body.expected_revision,
                body.entries,
                body.all_participants_upgraded,
            )
            .map_err(worker_error)?;
        Ok(Json(PlanRevision {
            plan_revision: revision,
        }))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

pub(crate) async fn worker_plan_events(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Query(query): Query<PlanEventsQuery>,
) -> Result<Json<Vec<PlanEvent>>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let coordinator = registered_coordinator(&state, &reference, &query.connection_id)?;
        coordinator
            .next_plan_events(query.after_revision)
            .map(Json)
            .map_err(worker_error)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

pub(crate) async fn dispatch_worker_plan(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Json(body): Json<PlanDispatchBody>,
) -> Result<Json<Assignment>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let coordinator = registered_coordinator(&state, &reference, &body.connection_id)?;
        coordinator
            .dispatch_next_plan(
                &body.authority,
                &body.generation,
                &|| time::OffsetDateTime::now_utc().unix_timestamp(),
                body.expected_revision,
                &body.operation,
                &body.base,
            )
            .map(Json)
            .map_err(worker_error)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

/// HS2-7J2ZSE: the project root, local ticket source and pool of a registered
/// checkout's linked Git source, as the worker view selects them. No caller path.
fn speculation_scope(
    state: &AppState,
    reference: &str,
    connection_id: Option<&str>,
) -> Result<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf), ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    let source_id = match connection_id {
        Some(id) => id.to_owned(),
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
            "provisional readiness is unavailable for this source",
        ));
    }
    let store = checkout
        .verified_git_source(&source_id)
        .map_err(checkout_lookup_error)?;
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
    Ok((
        std::path::PathBuf::from(&checkout.root),
        store.root().to_path_buf(),
        registry_home,
    ))
}

#[derive(Deserialize)]
pub(crate) struct SpeculationQuery {
    connection_id: Option<String>,
    ticket: Option<String>,
}

/// One provisional readiness request through the shared library, on a blocking thread.
async fn speculation_answer(
    state: &AppState,
    reference: &str,
    connection_id: Option<&str>,
    request: hotsheet_cli::coordination::speculation_api::SpeculationRequest,
    actor: Option<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (project, source, registry_home) = speculation_scope(state, reference, connection_id)?;
    tokio::task::spawn_blocking(move || {
        let coordinator = Coordinator::open_with_registry_home(&project, &source, registry_home)?;
        let store = hotsheet_ticketing::FsStore::open_without_maintenance(&source)?;
        hotsheet_cli::coordination::speculation_api::speculation_request(
            &coordinator,
            &project,
            Some(&store),
            request,
            actor.as_deref(),
            hotsheet_cli::coordination::speculation_api::MAX_HTTP_SYNC_WAIT_MS,
            time::OffsetDateTime::now_utc().unix_timestamp(),
        )
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))
}

/// `GET /checkouts/{reference}/speculation/policy`: the committed provisional readiness
/// policy (HS2-7J2ZSE); `coordination speculation-policy` prints the same.
pub(crate) async fn get_speculation_policy(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
) -> Result<Json<hotsheet_ticketing::provisional::SpeculationPolicy>, ApiError> {
    let (checkout, _) = crate::routes::project_tools::checkout_settings(&state, &reference)?;
    hotsheet_cli::coordination::speculation_api::speculation_policy(
        std::path::Path::new(&checkout.root),
        &hotsheet_cli::coordination::speculation_api::PolicyChange::default(),
    )
    .map(Json)
    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

/// `PUT /checkouts/{reference}/speculation/policy`: change the policy through the same
/// validated write as the command. The same change again writes nothing.
pub(crate) async fn put_speculation_policy(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Json(change): Json<hotsheet_cli::coordination::speculation_api::PolicyChange>,
) -> Result<Json<hotsheet_ticketing::provisional::SpeculationPolicy>, ApiError> {
    let (checkout, _) = crate::routes::project_tools::checkout_settings(&state, &reference)?;
    hotsheet_cli::coordination::speculation_api::speculation_policy(
        std::path::Path::new(&checkout.root),
        &change,
    )
    .map(Json)
    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

/// `GET /checkouts/{reference}/speculation/{status|explain}`: read-only answers.
pub(crate) async fn get_speculation(
    State(state): State<AppState>,
    RoutePath((reference, operation)): RoutePath<(String, String)>,
    Query(query): Query<SpeculationQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use hotsheet_cli::coordination::speculation_api::SpeculationRequest as R;
    let request = match operation.as_str() {
        "status" => R::Status,
        "explain" => R::Explain {
            ticket: query
                .ticket
                .clone()
                .filter(|ticket| !ticket.is_empty() && ticket.len() <= 160)
                .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "ticket is required"))?,
        },
        _ => {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "unknown provisional readiness read",
            ));
        }
    };
    speculation_answer(
        &state,
        &reference,
        query.connection_id.as_deref(),
        request,
        None,
    )
    .await
}

/// `POST /checkouts/{reference}/speculation/{result|sync|rebase|reopen}`: the
/// requests that change the ledger or a ticket. The body is the request's members,
/// with optional `connection_id` and `actor_id`.
pub(crate) async fn post_speculation(
    State(state): State<AppState>,
    RoutePath((reference, operation)): RoutePath<(String, String)>,
    Json(mut body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !matches!(operation.as_str(), "result" | "sync" | "rebase" | "reopen") {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "unknown provisional readiness request",
        ));
    }
    let object = body
        .as_object_mut()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "a JSON object is required"))?;
    let text = |value: Option<serde_json::Value>| -> Result<Option<String>, ApiError> {
        match value {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(text)) if !text.is_empty() && text.len() <= 160 => {
                Ok(Some(text))
            }
            Some(_) => Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "connection_id and actor_id are bounded strings",
            )),
        }
    };
    let connection_id = text(object.remove("connection_id"))?;
    let actor = text(object.remove("actor_id"))?;
    object.insert("operation".into(), operation.clone().into());
    let request: hotsheet_cli::coordination::speculation_api::SpeculationRequest =
        serde_json::from_value(body)
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    speculation_answer(&state, &reference, connection_id.as_deref(), request, actor).await
}
