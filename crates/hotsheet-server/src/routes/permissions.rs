//! Permission round-trip routes (HS2-9R9YZW).

use crate::*;

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
            .map(|request| PermissionInfo {
                id: request.id,
                project: request.project.clone(),
                connection: request.connection,
                tool: request.tool,
                action: request.action,
                agent: request.agent,
                always_allow_supported: rule_projects.contains_key(&request.project),
            })
            .collect(),
    )
}

/// Client-facing pending permission request. The capability flag is explicit so a client
/// never offers a durable response when this server has no rule store configured.
#[derive(Serialize)]
pub(crate) struct PermissionInfo {
    pub(crate) id: u64,
    pub(crate) project: String,
    pub(crate) connection: String,
    pub(crate) tool: String,
    pub(crate) action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) agent: Option<String>,
    pub(crate) always_allow_supported: bool,
}

/// Body for `POST /permissions/ask`: who's asking + what.
#[derive(Deserialize)]
pub(crate) struct AskBody {
    #[serde(default)]
    pub(crate) project: String,
    /// The live connection id (so a client attributes the prompt to the right tool).
    pub(crate) connection: String,
    /// The tool/action asking (e.g. `"Bash"`, `"Edit"`) — the rule-match key.
    pub(crate) tool: String,
    pub(crate) action: String,
    #[serde(default)]
    pub(crate) agent: Option<String>,
    /// A trusted interactive hook's hosting terminal. Presence proves its permission bridge is
    /// live even if its earlier SessionStart event never reached the server (HS2-XYSXVT).
    #[serde(default)]
    pub(crate) terminal_id: Option<String>,
    /// Identifies the reporting session so a late end from an earlier session cannot clear it.
    #[serde(default)]
    pub(crate) session_id: Option<String>,
}

/// A disconnected hook is no longer waiting for a decision. Remove its pending prompt
/// immediately instead of leaving it visible until the 24-hour safety timeout.
pub(crate) struct PermissionAskGuard {
    pub(crate) state: Arc<PermissionAskState>,
}

pub(crate) struct PermissionAskState {
    pub(crate) server: AppState,
    pub(crate) project: String,
    pub(crate) pending_id: AtomicU64,
    pub(crate) cancelled: AtomicBool,
}

impl PermissionAskState {
    pub(crate) fn mark_pending(&self, id: u64) {
        self.pending_id.store(id, Ordering::Release);
        if self.cancelled.load(Ordering::Acquire) {
            self.cancel_pending(id);
        }
    }

    pub(crate) fn cancel_pending(&self, id: u64) {
        if self
            .server
            .permissions
            .resolve(
                id,
                hotsheet_aitools::PermissionDecision::Deny,
                hotsheet_aitools::PermissionScope::Once,
            )
            .is_some()
        {
            self.emit_removed(id);
        }
    }

    pub(crate) fn emit_removed(&self, id: u64) {
        self.server.emit(ChangeEvent {
            cursor: None,
            store: self.project.clone(),
            kind: "permission_resolved".into(),
            id: id.to_string(),
            slug: String::new(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
    }
}

impl Drop for PermissionAskGuard {
    fn drop(&mut self) {
        self.state.cancelled.store(true, Ordering::Release);
        let id = self.state.pending_id.load(Ordering::Acquire);
        if id != 0 {
            self.state.cancel_pending(id);
        }
    }
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
                source: TerminalHookSource::PermissionRequest,
            }),
        )
        .await;
    }
    let stopping = state.clone();
    let bridge = state.permissions.clone();
    let project = if body.project.is_empty() {
        state.store.root().display().to_string()
    } else {
        body.project
    };
    let cancellation = Arc::new(PermissionAskState {
        server: state,
        project: project.clone(),
        pending_id: AtomicU64::new(0),
        cancelled: AtomicBool::new(false),
    });
    let guard = PermissionAskGuard {
        state: cancellation.clone(),
    };
    // request_blocking_timeout blocks (Condvar); run it off the async runtime.
    let blocking = tokio::task::spawn_blocking(move || {
        bridge.request_blocking_timeout_with_pending(
            hotsheet_aitools::PermissionAsk {
                project,
                connection: body.connection,
                tool: body.tool,
                action: body.action,
                agent: body.agent,
            },
            hotsheet_aitools::DEFAULT_PERMISSION_TIMEOUT,
            hotsheet_aitools::PermissionDecision::Deny,
            |id| cancellation.mark_pending(id),
            |_| {},
        )
    });
    // A stopping server must not hold the hook open (HS2-W1KJR4): answer 503 so the hook
    // falls back to the tool's native prompt. Dropping the guard denies + removes the
    // pending prompt, which also wakes the parked blocking thread.
    let decision = tokio::select! {
        joined = blocking => joined.unwrap_or(hotsheet_aitools::PermissionDecision::Deny),
        () = stopping.stopping() => {
            drop(guard);
            return permission_ask_stopping();
        }
    };
    drop(guard);
    let allow = decision == hotsheet_aitools::PermissionDecision::Allow;
    Json(serde_json::json!({ "decision": if allow { "allow" } else { "deny" } })).into_response()
}

/// `503` for a permission ask on a stopping server: the hook treats it as a transport
/// failure and falls back to the tool's native prompt (HS2-W1KJR4).
pub(crate) fn permission_ask_stopping() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "server is shutting down" })),
    )
        .into_response()
}

/// One driven connection as reported by `GET /connections`.
#[derive(Serialize)]
pub(crate) struct ConnectionInfo {
    pub(crate) id: String,
    pub(crate) tool: String,
    pub(crate) project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    /// `main` | `worker`.
    pub(crate) role: String,
    /// Whether the connection is busy (a turn is actively streaming) right now.
    pub(crate) busy: bool,
    /// Available semantic actions. Capability is represented by presence, never an inert
    /// `can_interrupt` boolean.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) effort: Option<String>,
}

/// `GET /connections` — what the server's driving loop is currently running (HS2-TCV3BF):
/// one entry per in-flight driven ticket. Empty when nothing is being driven.
pub(crate) async fn list_connections(State(state): State<AppState>) -> Json<Vec<ConnectionInfo>> {
    let now = now_ms();
    let reg = state.drive_registry.lock_or_recover();
    let infos = reg
        .list()
        .into_iter()
        .map(|c| ConnectionInfo {
            id: c.id.clone(),
            tool: c.tool.clone(),
            project: c.project.clone(),
            source: None,
            role: format!("{:?}", c.role).to_lowercase(),
            busy: reg.is_busy(&c.id, now),
            actions: Vec::new(),
            session_id: None,
            last_error: None,
            model: None,
            effort: None,
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
        });
    }
    infos.sort_by(|left, right| left.id.cmp(&right.id));
    Json(infos)
}

#[derive(Deserialize)]
pub(crate) struct CreateDriveConnectionReq {
    pub(crate) tool: String,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) effort: Option<String>,
    #[serde(default)]
    pub(crate) checkout: Option<String>,
    #[serde(default)]
    pub(crate) source: Option<String>,
    #[serde(default)]
    pub(crate) connection_id: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
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
pub(crate) fn discover_ai_tools_memoized(
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
    pub(crate) refresh: bool,
}

pub(crate) async fn list_ai_tools(
    State(state): State<AppState>,
    Query(query): Query<AiToolsQuery>,
) -> Json<Vec<hotsheet_plugins::AiToolDescriptor>> {
    Json(discovered_ai_tools_off_runtime(&state, query.refresh).await)
}

/// The settings key holding the default Drive tool, model, and effort.
pub(crate) const AI_DEFAULTS_SETTING: &str = "ai.defaults";

/// A saved AI default from one settings scope, kept only while it still names an installed tool.
pub(crate) fn saved_ai_defaults(
    settings: &Settings,
    scope: hotsheet_ticketing::Scope,
    tools: &[hotsheet_plugins::AiToolDescriptor],
) -> Result<Option<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(stored_ai_defaults(settings, scope)?
        .filter(|defaults| hotsheet_plugins::validate_ai_defaults(tools, defaults).is_ok())
        .map(|defaults| hotsheet_plugins::sanitize_ai_defaults(tools, defaults)))
}

/// The AI defaults stored in one scope, unvalidated.
pub(crate) fn stored_ai_defaults(
    settings: &Settings,
    scope: hotsheet_ticketing::Scope,
) -> Result<Option<hotsheet_plugins::AiToolDefaults>, ApiError> {
    Ok(settings
        .get(AI_DEFAULTS_SETTING, scope)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
        .and_then(|value| serde_json::from_value(value).ok()))
}

/// The effective AI defaults: the project's own choice (machine-local project settings, HS2-SW5S13),
/// else the legacy machine-wide value, else the discovered default tool.
pub(crate) async fn effective_ai_settings(
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
            &Settings::new(state.store.root()),
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
pub(crate) async fn save_ai_settings(
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
            &Settings::new(state.store.root()),
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
    let mut env = vec![format!("HOTSHEET_PROJECT={}", project_path.display())];
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
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) effort: Option<String>,
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
                    tool: connection.tool,
                    model: request.model.clone(),
                    effort: request.effort.clone(),
                    ..Default::default()
                },
            )
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
        }
    }
    let job = state
        .client_drives
        .begin_turn(&id, request.session_id, request.model, request.effort)
        .map_err(client_drive_api_error)?;
    let info = state
        .client_drives
        .get(&id)
        .map_err(client_drive_api_error)?;
    state.emit_drive_updated(&info);

    let manager = state.client_drives.clone();
    let thread_state = state.clone();
    let thread_store = FsStore::open(job.store_path()).map_err(ApiError::from)?;
    let tool = info.tool.clone();
    let activity_project = info.project.clone();
    let prompt = request.content;
    let thread_id = id.clone();
    std::thread::spawn(move || {
        let mut guard = turn_stream::TurnStreamGuard::default();
        let result = job.run(&prompt, &mut |event| {
            let projected = match event {
                hotsheet_aitools::TurnEvent::Usage(usage) => {
                    let priced = hotsheet_ticketing::metrics::price_event(
                        &thread_store,
                        hotsheet_ticketing::metrics::UsageEvent {
                            ts: now().as_str().to_string(),
                            tool: tool.clone(),
                            model: usage.model.clone(),
                            tokens_in: usage.tokens_in,
                            tokens_out: usage.tokens_out,
                            cost_usd: usage.cost_usd,
                            ticket: None,
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
            for event in guard.observe(&projected) {
                thread_state.emit_turn_event(&thread_store, &thread_id, None, &tool, event);
            }
        });
        if let Err(message) = &result {
            for event in guard.transport_failed(message) {
                thread_state.emit_turn_event(&thread_store, &thread_id, None, &tool, event);
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

pub(crate) fn client_drive_api_error(error: client_drive::ClientDriveError) -> ApiError {
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
