//! Permission round-trip routes (HS2-9R9YZW).

use crate::api_error::ApiError;
use crate::routes::checkout_tickets::checkout_source_for_create;
use crate::routes::project_tools::checkout_settings;
use crate::routes::terminals::{
    TerminalAiConnectionReq, TerminalHookSource, connect_terminal_ai,
    emit_permission_rules_changed, release_session_claims, release_session_claims_blocking,
};
use crate::{AppState, ai_tool_discovery, client_drive, multistore, now, turn_stream};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use hotsheet_model::Ulid;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{FsStore, Settings};
use serde::{Deserialize, Serialize};
use std::path::Path as FsPath;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Deserialize)]
pub(crate) struct PermissionMigrationQuery {
    source: String,
}

#[derive(Serialize)]
pub(crate) struct PermissionMigrationPreview {
    checkout: String,
    project_name: String,
    source: String,
    rules: Vec<hotsheet_aitools::PermissionMigrationCandidate>,
}

#[derive(Deserialize)]
pub(crate) struct PermissionMigrationRequest {
    source: String,
    selected_ids: Vec<String>,
    consent: bool,
}

fn permission_migration_paths(
    state: &AppState,
    reference: &str,
    source_id: &str,
) -> Result<
    (
        hotsheet_ticketing::checkouts::Checkout,
        std::path::PathBuf,
        std::path::PathBuf,
    ),
    ApiError,
> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    let store = checkout
        .verified_git_source(source_id)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    let identity = checkout.permission_identity();
    if !state.register_checkout_permission_rules(&identity) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "checkout permission vault unavailable",
        ));
    }
    let paths = state.permission_rule_paths.lock_or_recover();
    let destination = paths.get(&identity).cloned().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "checkout permission vault unavailable",
        )
    })?;
    let source = crate::watcher::same_path(store.root(), state.store.root())
        .then(|| paths.get("").cloned())
        .flatten()
        .or_else(|| {
            paths
                .iter()
                .find(|(project, path)| {
                    !project.is_empty()
                        && !project.starts_with("checkout:")
                        && crate::watcher::same_path(FsPath::new(project), store.root())
                        && path.is_file()
                })
                .map(|(_, path)| path.clone())
        })
        .or_else(|| {
            paths
                .get("")
                .and_then(|path| path.parent())
                .map(|directory| {
                    directory.join(format!(
                        "{}.json",
                        hotsheet_ticketing::git_connection_id(&store)
                    ))
                })
        })
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "legacy permission vault unavailable",
            )
        })?;
    if !source.is_file() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "legacy permission vault missing",
        ));
    }
    Ok((checkout, source, destination))
}

/// Review exactly one linked Git source before a project-wide consent decision.
pub(crate) async fn preview_checkout_permission_migration(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(query): Query<PermissionMigrationQuery>,
) -> Result<Json<PermissionMigrationPreview>, ApiError> {
    let (checkout, source, _) = permission_migration_paths(&state, &reference, &query.source)?;
    let legacy_project = checkout.source(&query.source).unwrap().locator.clone();
    let rules = tokio::task::spawn_blocking(move || {
        hotsheet_aitools::preview_legacy_permission_rules(&source, &legacy_project)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "legacy permission vault unavailable",
        )
    })?;
    Ok(Json(PermissionMigrationPreview {
        checkout: checkout.permission_identity(),
        project_name: checkout.alias,
        source: query.source,
        rules,
    }))
}

/// Copy only reviewed rules after explicit consent; the source vault is retained.
pub(crate) async fn migrate_checkout_permissions(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(body): Json<PermissionMigrationRequest>,
) -> Result<Json<hotsheet_aitools::PermissionMigrationOutcome>, ApiError> {
    if !body.consent {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "explicit migration consent required",
        ));
    }
    let (checkout, source, destination) =
        permission_migration_paths(&state, &reference, &body.source)?;
    let legacy_project = checkout.source(&body.source).unwrap().locator.clone();
    let identity = checkout.permission_identity();
    let selected = body.selected_ids;
    let target = identity.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        hotsheet_aitools::migrate_legacy_permission_rules(
            &source,
            &destination,
            &legacy_project,
            &target,
            &selected,
        )
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| {
        let status = if error.kind() == std::io::ErrorKind::InvalidInput {
            StatusCode::CONFLICT
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        };
        ApiError::new(status, "permission migration failed or review is stale")
    })?;
    if !state.register_checkout_permission_rules(&identity) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "checkout permission vault unavailable after migration",
        ));
    }
    emit_permission_rules_changed(&state, &identity);
    Ok(Json(outcome))
}

// ---- permission round-trip (HS2-9R9YZW) ------------------------------------------

/// `GET /permissions` — the requests a driven tool is currently blocked on, for a client
/// to render + answer. Each carries the raising connection + the `(tool, action)` asked.
pub(crate) async fn list_permissions(State(state): State<AppState>) -> Json<Vec<PermissionInfo>> {
    let rule_projects = state.permission_rule_paths.lock_or_recover().clone();
    Json(
        state
            .permissions
            .pending()
            .into_iter()
            .map(|request| {
                let suggestions =
                    hotsheet_aitools::suggest_secret_spans(&request.tool, &request.action);
                PermissionInfo {
                    id: request.id,
                    project: request.project.clone(),
                    connection: request.connection,
                    tool: request.tool,
                    action: request.action,
                    agent: request.agent,
                    always_allow_supported: rule_projects.get(&request.project).is_some_and(
                        |path| {
                            if path.is_file() {
                                hotsheet_aitools::permission_rules_health(path).is_ok()
                            } else {
                                !path.exists()
                                    && !request.project.starts_with("checkout:")
                                    && !state.permissions.durable_rules_locked()
                                    && !state.permissions.rules().iter().any(|rule| {
                                        rule.scope == hotsheet_aitools::PermissionScope::Always
                                            && rule.project == request.project
                                    })
                            }
                        },
                    ),
                    suggestions,
                }
            })
            .collect(),
    )
}

/// Client-facing pending permission request. The capability flag is explicit so a client
/// never offers a durable response when this server has no rule store configured.
#[derive(Serialize)]
pub(crate) struct PermissionInfo {
    id: u64,
    project: String,
    connection: String,
    tool: String,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    always_allow_supported: bool,
    suggestions: Vec<hotsheet_aitools::PermissionSecretSuggestion>,
}

/// Body for `POST /permissions/ask`: who's asking + what.
#[derive(Deserialize)]
pub(crate) struct AskBody {
    #[serde(default)]
    project: String,
    /// The live connection id (so a client attributes the prompt to the right tool).
    connection: String,
    /// The tool/action asking (e.g. `"Bash"`, `"Edit"`) — the rule-match key.
    tool: String,
    action: String,
    #[serde(default)]
    agent: Option<String>,
    /// A trusted interactive hook's hosting terminal. Presence proves its permission bridge is
    /// live even if its earlier SessionStart event never reached the server (HS2-XYSXVT).
    #[serde(default)]
    terminal_id: Option<String>,
    /// Identifies the reporting session so a late end from an earlier session cannot clear it.
    #[serde(default)]
    session_id: Option<String>,
}

/// A read-only authenticated probe for the exact route and secret a local hook would use.
/// It never announces a session or changes an AI terminal connection state.
pub(crate) async fn probe_permission_bridge(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if state.is_stopping() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "server is stopping",
        ));
    }
    Ok(Json(serde_json::json!({ "bridge_reachable": true })))
}

/// `POST /permissions/ask` `{connection, tool, action}` — raise a permission request and
/// **block** until a human answers over the route-back (`POST /permissions/{id}`), up to a
/// timeout then a safe `deny`. This is the *asking* side, for an external tool transport
/// like the Claude PreToolUse hook (HS2-YMR9HE). An allow-rule answers immediately.
pub(crate) async fn ask_permission(
    State(state): State<AppState>,
    Json(body): Json<AskBody>,
) -> Response {
    if state.is_stopping() {
        return permission_ask_stopping();
    }
    if let Some(terminal_id) = body.terminal_id.as_deref().filter(|id| !id.is_empty()) {
        // The authenticated hook has reached the permission bridge. Publish this before the
        // request parks for a human answer so the tab does not show a stale unplugged icon.
        // A terminal that ended meanwhile is ignored; it must not be resurrected.
        let _ = connect_terminal_ai(
            State(state.clone()),
            Path(terminal_id.to_owned()),
            Json(TerminalAiConnectionReq {
                agent: body.agent.clone(),
                session_id: body.session_id.clone(),
                codex_home: None,
                source: TerminalHookSource::PermissionRequest,
                messaging_socket: None,
                messaging_token: None,
                tool_version: None,
            }),
        )
        .await;
    }
    let project = if body.project.is_empty() {
        state.store.root().display().to_string()
    } else {
        body.project
    };
    if project.starts_with("checkout:") {
        state.register_checkout_permission_rules(&project);
    }
    let bridge = state.permissions.clone();
    // The wait is async (HS2-43YT58): a pending ask parks no thread, so hundreds of open
    // hooks cannot exhaust the blocking pool shared with offloaded API requests. A hook that
    // disconnects drops this future, which withdraws its pending prompt as a one-time deny
    // instead of leaving it visible until the 24-hour safety timeout.
    let ask = bridge.request_timeout_with_pending_async(
        hotsheet_aitools::PermissionAsk {
            project,
            connection: body.connection,
            tool: body.tool,
            action: body.action,
            agent: body.agent,
        },
        hotsheet_aitools::DEFAULT_PERMISSION_TIMEOUT,
        hotsheet_aitools::PermissionDecision::Deny,
        |_| {},
        |_| {},
    );
    // A stopping server must not hold the hook open (HS2-W1KJR4): answer 503 so the hook
    // falls back to the tool's native prompt. Dropping the ask withdraws the pending prompt.
    let decision = tokio::select! {
        decision = ask => decision,
        () = state.stopping() => return permission_ask_stopping(),
    };
    let allow = decision == hotsheet_aitools::PermissionDecision::Allow;
    Json(serde_json::json!({ "decision": if allow { "allow" } else { "deny" } })).into_response()
}

/// `503` for a permission ask on a stopping server: the hook treats it as a transport
/// failure and falls back to the tool's native prompt (HS2-W1KJR4).
fn permission_ask_stopping() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "server is shutting down" })),
    )
        .into_response()
}

/// One driven connection as reported by `GET /connections`.
#[derive(Serialize)]
pub(crate) struct ConnectionInfo {
    id: String,
    tool: String,
    project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    /// `main` | `worker`.
    role: String,
    /// Whether the connection is busy (a turn is actively streaming) right now.
    busy: bool,
    /// Available semantic actions. Capability is represented by presence, never an inert
    /// `can_interrupt` boolean.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_mode: Option<String>,
    /// A busy client drive's running turn and its timing (HS2-14X4AF, HS2-XV1VCR).
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_started_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_event_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    in_flight_tool: Option<client_drive::InFlightTool>,
}

/// `GET /connections` — what the server's driving loop is currently running (HS2-TCV3BF):
/// one entry per in-flight driven ticket. Empty when nothing is being driven.
pub(crate) async fn list_connections(State(state): State<AppState>) -> Json<Vec<ConnectionInfo>> {
    let now = now_ms();
    let reg = state.drive_registry.lock_or_recover();
    let infos = reg
        .list()
        .into_iter()
        .map(|c| {
            // A live terminal session with a session-push adapter accepts messages
            // (HS2-PEJ962); its tool-native session id is reported once known.
            let push = crate::routes::session_messages::adapter(&state, &c.id);
            (c, push)
        })
        .map(|(c, push)| ConnectionInfo {
            id: c.id.clone(),
            tool: c.tool.clone(),
            project: c.project.clone(),
            source: None,
            role: format!("{:?}", c.role).to_lowercase(),
            busy: reg.is_busy(&c.id, now),
            actions: push
                .as_ref()
                .map(|_| vec![crate::routes::session_messages::MESSAGE_ACTION.to_string()])
                .unwrap_or_default(),
            session_id: push.as_ref().and_then(|push| push.session_id()),
            last_error: None,
            model: None,
            effort: None,
            permission_mode: None,
            turn_id: None,
            turn_started_at_ms: None,
            last_event_at_ms: None,
            in_flight_tool: None,
        })
        .collect::<Vec<_>>();
    drop(reg);
    let mut infos = infos;
    for client in state.client_drives.list() {
        if infos.iter().any(|existing| existing.id == client.id) {
            continue;
        }
        infos.push(ConnectionInfo {
            id: client.id,
            tool: client.tool,
            project: client.project,
            source: Some(client.source),
            role: client.role,
            busy: client.busy,
            actions: client.actions,
            session_id: client.session_id,
            last_error: client.last_error,
            model: client.model,
            effort: client.effort,
            permission_mode: client.permission_mode,
            turn_id: client.turn_id,
            turn_started_at_ms: client.turn_started_at_ms,
            last_event_at_ms: client.last_event_at_ms,
            in_flight_tool: client.in_flight_tool,
        });
    }
    infos.sort_by(|left, right| left.id.cmp(&right.id));
    Json(infos)
}

#[derive(Deserialize)]
pub(crate) struct CreateDriveConnectionReq {
    tool: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    /// The chat's permission mode (HS2-NHRDMW); omitted = the project's saved default for
    /// this tool, else the plugin default.
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    checkout: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    connection_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

pub(crate) fn discovered_ai_tools(
    state: &AppState,
    refresh: bool,
) -> Vec<hotsheet_plugins::AiToolDescriptor> {
    discover_ai_tools_memoized(
        &state.model_catalogs,
        &state.ai_tool_generation,
        &state.ai_tool_discovery,
        &state.plugin_dirs,
        state.store.root(),
        refresh,
    )
}

/// Blocking AI-tool discovery through the short-lived memo (HS2-QV8B7R). The generation is
/// read under the catalog lock, so callers that queued behind a scan reuse its result
/// unless an invalidation landed after it started.
fn discover_ai_tools_memoized(
    catalogs: &Mutex<ai_tool_discovery::AiToolDiscoveryCache>,
    generation: &AtomicU64,
    config: &ai_tool_discovery::AiToolDiscoveryConfig,
    plugin_dirs: &[std::path::PathBuf],
    root: &FsPath,
    refresh: bool,
) -> Vec<hotsheet_plugins::AiToolDescriptor> {
    let mut cache = catalogs.lock_or_recover();
    let generation = generation.load(Ordering::Acquire);
    cache.discover(
        std::time::Instant::now(),
        generation,
        config.ttl,
        refresh,
        |catalogs, refresh| (config.scanner)(plugin_dirs, root, catalogs, refresh),
    )
}

impl AppState {
    /// Drop the memoized AI-tool discovery after tool installation or plugin state changed,
    /// without waiting for a discovery in progress (HS2-QV8B7R).
    pub(crate) fn invalidate_ai_tool_discovery(&self) {
        self.ai_tool_generation.fetch_add(1, Ordering::AcqRel);
    }
}

/// Discover AI tools **off the async runtime**.
///
/// AI-tool discovery launches blocking `--version`/`models` subprocesses for every
/// installed drivable tool (even a warm cache re-checks each tool's runtime version),
/// and it takes the single shared `model_catalogs` mutex for the whole scan. Running it
/// inline in an async handler both starves a tokio worker for the duration and serializes
/// unrelated concurrent requests behind it — the mechanism behind a second web client
/// loading far slower than the first (HS2-10R4VV). Move the whole scan (and its lock) to
/// the blocking pool so the async workers stay free. See HS2-S66BZZ.
///
/// The result is memoized for the configured TTL (default
/// [`ai_tool_discovery::AI_TOOL_DISCOVERY_TTL`]) under the same
/// lock, so project activation's `/ai-tools` and `/ai-settings` share one scan and
/// concurrent callers coalesce onto it; `refresh` bypasses and repopulates the memo
/// (HS2-QV8B7R).
pub(crate) async fn discovered_ai_tools_off_runtime(
    state: &AppState,
    refresh: bool,
) -> Vec<hotsheet_plugins::AiToolDescriptor> {
    let catalogs = state.model_catalogs.clone();
    let generation = state.ai_tool_generation.clone();
    let config = state.ai_tool_discovery.clone();
    let plugin_dirs = state.plugin_dirs.clone();
    let root = state.store.root().to_path_buf();
    // A detached thread, not the blocking pool (HS2-NPBZJ9): the scan runs installed tools'
    // `--version`/catalog subprocesses with no deadline of their own, and the runtime waits
    // for every `spawn_blocking` task when it drops. A probe that hangs past a stop (or past
    // its abandoned request) must not hold the process; its result is only a cache fill.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("ai-tool-discovery".into())
        .spawn(move || {
            let _ = sender.send(discover_ai_tools_memoized(
                &catalogs,
                &generation,
                &config,
                &plugin_dirs,
                &root,
                refresh,
            ));
        });
    if spawned.is_err() {
        return Vec::new();
    }
    receiver.await.unwrap_or_default()
}

#[derive(Default, Deserialize)]
pub(crate) struct AiToolsQuery {
    #[serde(default)]
    refresh: bool,
}

pub(crate) async fn list_ai_tools(
    State(state): State<AppState>,
    Query(query): Query<AiToolsQuery>,
) -> Json<Vec<hotsheet_plugins::AiToolDescriptor>> {
    Json(discovered_ai_tools_off_runtime(&state, query.refresh).await)
}

/// The settings key holding the default Drive tool, model, and effort.
const AI_DEFAULTS_SETTING: &str = "ai.defaults";

/// A saved AI default from one settings scope, kept only while it still names an installed tool.
fn saved_ai_defaults(
    settings: &Settings,
    scope: hotsheet_ticketing::Scope,
    tools: &[hotsheet_plugins::AiToolDescriptor],
) -> Result<Option<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(stored_ai_defaults(settings, scope)?
        .filter(|defaults| hotsheet_plugins::validate_ai_defaults(tools, defaults).is_ok())
        .map(|defaults| hotsheet_plugins::sanitize_ai_defaults(tools, defaults)))
}

/// The AI defaults stored in one scope, unvalidated.
fn stored_ai_defaults(
    settings: &Settings,
    scope: hotsheet_ticketing::Scope,
) -> Result<Option<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(settings
        .get(AI_DEFAULTS_SETTING, scope)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
        .and_then(|value| serde_json::from_value(value).ok()))
}

/// The permission mode saved for `tool` in one settings scope (HS2-NHRDMW): its provider entry,
/// else the top-level value when it is the default provider.
fn saved_permission_mode(
    settings: &Settings,
    scope: hotsheet_ticketing::Scope,
    tool: &str,
) -> Option<String> {
    let defaults = stored_ai_defaults(settings, scope).ok().flatten()?;
    defaults
        .providers
        .get(tool)
        .and_then(|entry| entry.permission_mode.clone())
        .or_else(|| {
            (defaults.tool == tool)
                .then_some(defaults.permission_mode)
                .flatten()
        })
}

/// The launch permission mode for a new drive (HS2-NHRDMW): the requested mode (validated when
/// the tool is discovered), else the mode the resumed chat last used (HS2-Z5QVXW), else the
/// project's saved default for the tool, else the machine-wide one, else `None` (the plugin's
/// declared default applies at launch). A remembered or saved mode the installed tool no longer
/// declares is skipped rather than failing the launch.
async fn launch_permission_mode(
    state: &AppState,
    tool: &str,
    requested: Option<String>,
    resumed: Option<String>,
    project: Option<&Settings>,
) -> Result<Option<String>, ApiError> {
    let tools = discovered_ai_tools_off_runtime(state, false).await;
    let descriptor = tools.iter().find(|candidate| candidate.id == tool);
    if let Some(mode) = requested {
        if let Some(descriptor) = descriptor {
            hotsheet_plugins::validate_permission_mode(descriptor, &mode)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
        }
        return Ok(Some(mode));
    }
    if let Some(mode) = resumed.filter(|mode| {
        descriptor.is_none_or(|descriptor| {
            hotsheet_plugins::validate_permission_mode(descriptor, mode).is_ok()
        })
    }) {
        return Ok(Some(mode));
    }
    let saved = project
        .and_then(|settings| {
            saved_permission_mode(settings, hotsheet_ticketing::Scope::Local, tool)
        })
        .or_else(|| {
            saved_permission_mode(
                &Settings::with_global_home(state.store.root(), state.machine_home()),
                hotsheet_ticketing::Scope::Global,
                tool,
            )
        });
    // A saved mode the installed tool no longer declares falls back to the plugin default.
    Ok(match descriptor {
        Some(descriptor) => saved
            .filter(|mode| hotsheet_plugins::validate_permission_mode(descriptor, mode).is_ok()),
        None => None,
    })
}

/// The effective AI defaults: the project's own choice (machine-local project settings, HS2-SW5S13),
/// else the legacy machine-wide value, else the discovered default tool.
async fn effective_ai_settings(
    state: &AppState,
    project: Option<&Settings>,
) -> Result<hotsheet_plugins::AiToolDefaults, ApiError> {
    let tools = discovered_ai_tools_off_runtime(state, false).await;
    let project_defaults = match project {
        Some(settings) => saved_ai_defaults(settings, hotsheet_ticketing::Scope::Local, &tools)?,
        None => None,
    };
    let saved = match project_defaults {
        Some(defaults) => Some(defaults),
        None => saved_ai_defaults(
            &Settings::with_global_home(state.store.root(), state.machine_home()),
            hotsheet_ticketing::Scope::Global,
            &tools,
        )?,
    };
    saved
        .or_else(|| hotsheet_plugins::default_ai_settings(&tools))
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no drivable AI tools are installed"))
}

/// Validate and store AI defaults in one settings scope, keeping the per-provider choices for
/// tools that are not installed right now (HS2-EK24KF). Returns the value as served back.
async fn save_ai_settings(
    state: &AppState,
    settings: &Settings,
    scope: hotsheet_ticketing::Scope,
    defaults: &hotsheet_plugins::AiToolDefaults,
) -> Result<hotsheet_plugins::AiToolDefaults, ApiError> {
    let tools = discovered_ai_tools_off_runtime(state, false).await;
    let previous = stored_ai_defaults(settings, scope)?;
    let prepared =
        hotsheet_plugins::prepare_ai_defaults_for_save(&tools, defaults, previous.as_ref())
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    settings
        .set(
            AI_DEFAULTS_SETTING,
            serde_json::to_value(&prepared)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?,
            scope,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(hotsheet_plugins::sanitize_ai_defaults(&tools, prepared))
}

/// A project's AI defaults (HS2-SW5S13): stored in the checkout's machine-local settings because the
/// installed tools and models differ per machine; a project without a choice inherits the
/// machine-wide value.
pub(crate) async fn get_checkout_ai_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<hotsheet_plugins::AiToolDefaults>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    Ok(Json(effective_ai_settings(&state, Some(&settings)).await?))
}

pub(crate) async fn put_checkout_ai_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(defaults): Json<hotsheet_plugins::AiToolDefaults>,
) -> Result<Json<hotsheet_plugins::AiToolDefaults>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    Ok(Json(
        save_ai_settings(
            &state,
            &settings,
            hotsheet_ticketing::Scope::Local,
            &defaults,
        )
        .await?,
    ))
}

/// The machine-wide AI defaults: the fallback for projects without their own choice.
pub(crate) async fn get_ai_settings(
    State(state): State<AppState>,
) -> Result<Json<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(Json(effective_ai_settings(&state, None).await?))
}

pub(crate) async fn put_ai_settings(
    State(state): State<AppState>,
    Json(defaults): Json<hotsheet_plugins::AiToolDefaults>,
) -> Result<Json<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(Json(
        save_ai_settings(
            &state,
            &Settings::with_global_home(state.store.root(), state.machine_home()),
            hotsheet_ticketing::Scope::Global,
            &defaults,
        )
        .await?,
    ))
}

pub(crate) async fn create_drive_connection(
    State(state): State<AppState>,
    Json(request): Json<CreateDriveConnectionReq>,
) -> Result<(StatusCode, Json<client_drive::ClientConnectionInfo>), ApiError> {
    if request.tool.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "tool is required"));
    }
    if request.model.is_some() || request.effort.is_some() {
        let tools = discovered_ai_tools_off_runtime(&state, false).await;
        if tools.iter().any(|tool| tool.id == request.tool) {
            hotsheet_plugins::validate_ai_defaults(
                &tools,
                &hotsheet_plugins::AiToolDefaults {
                    tool: request.tool.clone(),
                    model: request.model.clone(),
                    effort: request.effort.clone(),
                    ..Default::default()
                },
            )
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
        }
    }
    let mut project_settings = None;
    let mut permission_identity = format!("unregistered:{}", state.store.root().display());
    let (project_path, source_id, drive_store) =
        if let Some(reference) = request.checkout.as_deref() {
            let checkout = state
                .checkout_registry
                .resolve(reference)
                .map_err(|error| {
                    let status = match error {
                        hotsheet_ticketing::checkouts::CheckoutError::NotFound(_) => {
                            StatusCode::NOT_FOUND
                        }
                        hotsheet_ticketing::checkouts::CheckoutError::Ambiguous(_) => {
                            StatusCode::CONFLICT
                        }
                        _ => StatusCode::BAD_REQUEST,
                    };
                    ApiError::new(status, error.to_string())
                })?;
            let project_path = std::path::PathBuf::from(&checkout.root);
            permission_identity = checkout.permission_identity();
            state.register_checkout_permission_rules(&permission_identity);
            project_settings = Some(checkout.settings());
            let source = checkout_source_for_create(&state, reference, request.source.as_deref())?;
            if source.provider != "git" {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format!(
                        "ticket source '{}' does not support local client-drive activity",
                        source.connection_id
                    ),
                ));
            }
            let entry = state.host.get(&source.connection_id).ok_or_else(|| {
                ApiError::new(
                    StatusCode::CONFLICT,
                    format!(
                        "ticket source '{}' is not hosted by this server",
                        source.connection_id
                    ),
                )
            })?;
            (project_path, source.connection_id, entry.store)
        } else {
            (
                state.store.root().to_path_buf(),
                multistore::store_url_id(&state.store),
                state.store.clone(),
            )
        };
    let resumed_mode = request.session_id.as_deref().and_then(|session_id| {
        state.client_drives.session_permission_mode(
            &project_path.display().to_string(),
            &request.tool,
            session_id,
        )
    });
    let permission_mode = launch_permission_mode(
        &state,
        &request.tool,
        request.permission_mode,
        resumed_mode,
        project_settings.as_ref(),
    )
    .await?;
    let mut env = vec![
        format!("HOTSHEET_PROJECT={}", project_path.display()),
        format!("HOTSHEET_PERMISSION_PROJECT={permission_identity}"),
    ];
    if let Some(url) = state.terminal_server_url.with_lock(|value| value.clone()) {
        env.push(format!("HOTSHEET_SERVER={url}"));
        env.push(format!("HOTSHEET_SECRET={}", state.secret));
    }
    let info = state
        .client_drives
        .create_or_attach(
            client_drive::PrepareDrive {
                store_path: drive_store.root().to_path_buf(),
                source_id,
                project_path,
                tool: request.tool,
                env,
                permission_bridge: state.permission_bridge(),
                persistent_home: None,
                model: request.model,
                effort: request.effort,
                permission_mode,
            },
            request.connection_id,
            request.session_id,
        )
        .map_err(client_drive_api_error)?;
    state.emit_drive_updated(&info);
    Ok((StatusCode::CREATED, Json(info)))
}

pub(crate) async fn delete_drive_connection(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|error| {
            let status = match error {
                hotsheet_ticketing::checkouts::CheckoutError::NotFound(_) => StatusCode::NOT_FOUND,
                hotsheet_ticketing::checkouts::CheckoutError::Ambiguous(_) => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            ApiError::new(status, error.to_string())
        })?;
    let project = std::path::PathBuf::from(checkout.root)
        .display()
        .to_string();
    let closed = state
        .client_drives
        .close(&project, &id)
        .map_err(client_drive_api_error)?;
    // An idle drive's session ends now; a busy one's when its interrupted turn finishes.
    if let client_drive::DriveClosed::Ended { worker_id } = &closed {
        release_session_claims(state.host.clone(), worker_id.clone()).await;
    }
    if closed != client_drive::DriveClosed::NotFound {
        let permissions = state.permission_bridge();
        // The chat is gone, so are its "Allow for this chat" answers (HS2-JNHAYB).
        if permissions.forget_connection(&id) > 0 {
            emit_permission_rules_changed(&state, &project);
        }
        for request in permissions
            .pending()
            .into_iter()
            .filter(|request| request.project == project && request.connection == id)
        {
            permissions.resolve(
                request.id,
                hotsheet_aitools::PermissionDecision::Deny,
                hotsheet_aitools::PermissionScope::Once,
            );
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn list_drive_sessions(
    State(state): State<AppState>,
) -> Json<Vec<client_drive::ClientSessionInfo>> {
    Json(
        state
            .client_drives
            .sessions(&state.store.root().display().to_string()),
    )
}

#[derive(Deserialize)]
pub(crate) struct SendDriveTurnReq {
    content: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    /// Switch the chat's permission mode from this turn on (HS2-NHRDMW).
    #[serde(default)]
    permission_mode: Option<String>,
    /// Optional client-chosen correlation id for this turn's stream events (HS2-14X4AF).
    #[serde(default)]
    turn_id: Option<String>,
}

pub(crate) async fn send_drive_turn(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<SendDriveTurnReq>,
) -> Result<(StatusCode, Json<client_drive::ClientConnectionInfo>), ApiError> {
    if request.content.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "content is required",
        ));
    }
    let connection = state
        .client_drives
        .get(&id)
        .map_err(client_drive_api_error)?;
    if request.model.is_some() || request.effort.is_some() {
        let tools = discovered_ai_tools_off_runtime(&state, false).await;
        if let Some(descriptor) = tools.iter().find(|tool| tool.id == connection.tool) {
            if request.model.is_some()
                && !descriptor
                    .actions
                    .iter()
                    .any(|action| action == "change_model")
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "this AI tool cannot change model within a session",
                ));
            }
            if request.effort.is_some()
                && !descriptor
                    .actions
                    .iter()
                    .any(|action| action == "change_effort")
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "this AI tool cannot change effort within a session",
                ));
            }
            hotsheet_plugins::validate_ai_defaults(
                &tools,
                &hotsheet_plugins::AiToolDefaults {
                    tool: connection.tool.clone(),
                    model: request.model.clone(),
                    effort: request.effort.clone(),
                    ..Default::default()
                },
            )
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
        }
    }
    if let Some(mode) = request.permission_mode {
        if connection.permission_mode.as_deref() != Some(mode.as_str()) {
            let tools = discovered_ai_tools_off_runtime(&state, false).await;
            if let Some(descriptor) = tools.iter().find(|tool| tool.id == connection.tool) {
                if !descriptor
                    .actions
                    .iter()
                    .any(|action| action == "change_permission_mode")
                {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "this AI tool cannot change permission mode within a session",
                    ));
                }
                hotsheet_plugins::validate_permission_mode(descriptor, &mode)
                    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
            }
            state
                .client_drives
                .set_permission_mode(&id, mode)
                .map_err(client_drive_api_error)?;
        }
    }
    let job = state
        .client_drives
        .begin_correlated_turn(
            &id,
            request.session_id,
            request.model,
            request.effort,
            request.turn_id,
        )
        .map_err(client_drive_api_error)?;
    let info = state
        .client_drives
        .get(&id)
        .map_err(client_drive_api_error)?;
    state.emit_drive_updated(&info);

    let manager = state.client_drives.clone();
    let thread_state = state.clone();
    let thread_store = FsStore::open(job.store_path()).map_err(ApiError::from)?;
    let worker = job.worker_id().to_owned();
    let tool = info.tool.clone();
    let selected_model = job.selected_model().map(str::to_owned);
    let activity_project = info.project.clone();
    let prompt = request.content;
    let thread_id = id.clone();
    let turn_id = job.turn_id().to_owned();
    std::thread::spawn(move || {
        let ticket = hotsheet_ticketing::metrics::active_ticket_for_workers(
            &thread_store,
            &[&worker],
            &now(),
        )
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "looking up client turn ticket failed");
            None
        });
        let mut guard = turn_stream::TurnStreamGuard::default();
        let result = job.run(&prompt, &mut |event| {
            let projected = match event {
                hotsheet_aitools::TurnEvent::Usage(usage) => {
                    let priced = hotsheet_ticketing::metrics::price_event(
                        &thread_store,
                        hotsheet_ticketing::metrics::UsageEvent {
                            ts: now().as_str().to_string(),
                            tool: tool.clone(),
                            model: usage.model.clone().or_else(|| selected_model.clone()),
                            tokens_in: usage.tokens_in,
                            tokens_out: usage.tokens_out,
                            cost_usd: usage.cost_usd,
                            ticket: ticket.clone(),
                            session: Some(thread_id.clone()),
                        },
                    );
                    if let Err(error) = hotsheet_ticketing::metrics::record(&thread_store, &priced)
                    {
                        tracing::warn!(%error, "recording turn usage metrics failed");
                    }
                    hotsheet_aitools::TurnEvent::Usage(hotsheet_aitools::Usage {
                        model: priced.model,
                        tokens_in: priced.tokens_in,
                        tokens_out: priced.tokens_out,
                        cost_usd: priced.cost_usd,
                    })
                }
                hotsheet_aitools::TurnEvent::NativeActivity { source, payload } => {
                    let id = Ulid::new().to_string();
                    let ts = now().as_str().to_string();
                    let mapped = match source.as_str() {
                        "codex-transcript" => {
                            hotsheet_ticketing::activity::codex_activity(payload, &id, &ts)
                        }
                        "claude-hooks" => {
                            hotsheet_ticketing::activity::claude_activity(payload, &id, &ts)
                        }
                        _ => None,
                    };
                    if let Some(mut activity) = mapped {
                        activity.session = Some(thread_id.clone());
                        activity.project = Some(activity_project.clone());
                        if let Err(error) = thread_state.record_activity(&thread_store, activity) {
                            tracing::warn!(%error, "recording native activity failed");
                        }
                    }
                    event.clone()
                }
                _ => event.clone(),
            };
            let timing = job.observe_event(&projected);
            for event in guard.observe(&projected) {
                thread_state.emit_timed_turn_event(
                    &thread_store,
                    &thread_id,
                    &turn_id,
                    &tool,
                    event,
                    &timing,
                );
            }
        });
        if let Err(message) = &result {
            for event in guard.transport_failed(message) {
                thread_state.emit_turn_event(
                    &thread_store,
                    &thread_id,
                    None,
                    Some(&turn_id),
                    &tool,
                    event,
                );
            }
        }
        if let Some(worker) = manager.finish_turn(&job, &result) {
            release_session_claims_blocking(&thread_state.host, &worker);
        }
        if let Ok(info) = manager.get(&thread_id) {
            thread_state.emit_drive_updated(&info);
        }
    });
    Ok((StatusCode::ACCEPTED, Json(info)))
}

pub(crate) async fn interrupt_drive_turn(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<client_drive::ClientConnectionInfo>), ApiError> {
    state
        .client_drives
        .interrupt(&id)
        .map_err(client_drive_api_error)?;
    let info = state
        .client_drives
        .get(&id)
        .map_err(client_drive_api_error)?;
    state.emit_drive_updated(&info);
    Ok((StatusCode::ACCEPTED, Json(info)))
}

fn client_drive_api_error(error: client_drive::ClientDriveError) -> ApiError {
    use client_drive::ClientDriveError as Error;
    let status = match error {
        Error::NotFound(_) => StatusCode::NOT_FOUND,
        Error::Conflict(_) => StatusCode::CONFLICT,
        Error::Unsupported(_) => StatusCode::METHOD_NOT_ALLOWED,
        Error::Prepare(_) => StatusCode::BAD_REQUEST,
        Error::Unavailable | Error::Persistence(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.to_string())
}

/// Wall-clock epoch milliseconds (the driving loop's busy-tracking time base).
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
