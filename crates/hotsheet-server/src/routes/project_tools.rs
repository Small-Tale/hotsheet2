//! Reports, commands, custom views, notifications, and speech routes.

use crate::*;

pub(crate) async fn ticket_flow_summary(
    State(state): State<AppState>,
) -> Result<Json<hotsheet_ticketing::analytics::TicketFlowSummary>, ApiError> {
    let tickets = ops::query(&state.store, &TicketQuery::default())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(hotsheet_ticketing::analytics::ticket_flow(&tickets)))
}

/// `GET /confidence-report`: completion-confidence calibration for the default store
/// (HS2-Q1WCCY), the same report as `hotsheet-cli confidence-report --json`.
pub(crate) async fn confidence_report(
    State(state): State<AppState>,
) -> Result<Json<hotsheet_ticketing::calibration::CalibrationReport>, ApiError> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || {
        let tickets = store.list_tickets_resilient()?.tickets;
        Ok(Json(hotsheet_ticketing::calibration::calibration(&tickets)))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

/// `GET /checkouts/{reference}/confidence-report`: calibration across every git store the
/// checkout links (HS2-Q1WCCY). External trackers are not included; their reopen history
/// lives in native events.
pub(crate) async fn checkout_confidence_report(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<hotsheet_ticketing::calibration::CalibrationReport>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let mut tickets = Vec::new();
        for (_, entry) in checkout_entries(&state, &reference)? {
            tickets.extend(entry.store.list_tickets_resilient()?.tickets);
        }
        Ok(Json(hotsheet_ticketing::calibration::calibration(&tickets)))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

pub(crate) async fn usage_metrics_summary(
    State(state): State<AppState>,
) -> Result<Json<hotsheet_ticketing::metrics::Rollup>, ApiError> {
    hotsheet_ticketing::metrics::summary_settled(&state.store)
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub(crate) async fn list_commands(
    State(state): State<AppState>,
) -> Json<Vec<hotsheet_ticketing::commands::CommandDefinition>> {
    Json(state.commands.definitions())
}

pub(crate) fn validate_command_definitions(
    definitions: &[hotsheet_ticketing::commands::CommandDefinition],
) -> Result<(), ApiError> {
    use hotsheet_ticketing::commands::CommandKind;
    use std::collections::HashSet;
    let mut ids = HashSet::new();
    if definitions.iter().any(|definition| {
        definition.id.trim().is_empty()
            || definition.title.trim().is_empty()
            || match definition.kind {
                CommandKind::Program => definition.program.trim().is_empty(),
                CommandKind::Shell => definition
                    .command
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty()),
                CommandKind::Ai => definition
                    .prompt
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty()),
            }
            || !ids.insert(definition.id.as_str())
    }) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "commands require unique non-empty ids and titles plus their kind-specific executable content",
        ));
    }
    Ok(())
}

pub(crate) async fn save_commands(
    State(state): State<AppState>,
    Json(definitions): Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>,
) -> Result<Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>, ApiError> {
    validate_command_definitions(&definitions)?;
    Settings::new(state.store.root())
        .set(
            "commands",
            serde_json::to_value(&definitions)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?,
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state.commands.replace_definitions(definitions.clone());
    Ok(Json(definitions))
}

pub(crate) fn checkout_settings(
    state: &AppState,
    reference: &str,
) -> Result<(hotsheet_ticketing::checkouts::Checkout, Settings), ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    let settings = checkout.settings();
    Ok((checkout, settings))
}

pub(crate) async fn list_checkout_commands(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>, ApiError> {
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    let definitions = hotsheet_ticketing::commands::from_settings(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state
        .commands
        .replace_project(checkout.id, checkout.root.into(), definitions.clone());
    Ok(Json(definitions))
}

pub(crate) fn read_command_groups(settings: &Settings) -> Result<Json<Vec<String>>, ApiError> {
    hotsheet_ticketing::commands::groups_from_settings(settings)
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

/// Persist the kept (possibly empty) command groups beside `commands`, in the same
/// machine-local scope, so an "Add group" survives a reload (HS2-EZ5KMC).
pub(crate) fn write_command_groups(
    settings: &Settings,
    groups: Vec<String>,
) -> Result<Json<Vec<String>>, ApiError> {
    let groups = hotsheet_ticketing::commands::normalize_groups(groups);
    settings
        .set(
            hotsheet_ticketing::commands::COMMAND_GROUPS_KEY,
            serde_json::to_value(&groups)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?,
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(groups))
}

pub(crate) async fn list_command_groups(
    State(state): State<AppState>,
) -> Result<Json<Vec<String>>, ApiError> {
    read_command_groups(&Settings::new(state.store.root()))
}

pub(crate) async fn save_command_groups(
    State(state): State<AppState>,
    Json(groups): Json<Vec<String>>,
) -> Result<Json<Vec<String>>, ApiError> {
    write_command_groups(&Settings::new(state.store.root()), groups)
}

pub(crate) async fn list_checkout_command_groups(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<String>>, ApiError> {
    let (_checkout, settings) = checkout_settings(&state, &reference)?;
    read_command_groups(&settings)
}

pub(crate) async fn save_checkout_command_groups(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(groups): Json<Vec<String>>,
) -> Result<Json<Vec<String>>, ApiError> {
    let (_checkout, settings) = checkout_settings(&state, &reference)?;
    write_command_groups(&settings, groups)
}

pub(crate) async fn save_checkout_commands(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(definitions): Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>,
) -> Result<Json<Vec<hotsheet_ticketing::commands::CommandDefinition>>, ApiError> {
    validate_command_definitions(&definitions)?;
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            "commands",
            serde_json::to_value(&definitions)
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?,
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state
        .commands
        .replace_project(checkout.id, checkout.root.into(), definitions.clone());
    Ok(Json(definitions))
}

pub(crate) async fn list_custom_views(
    State(state): State<AppState>,
) -> Result<Json<Vec<custom_views::CustomView>>, ApiError> {
    custom_views::from_settings(&Settings::new(state.store.root()))
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

pub(crate) async fn save_custom_views(
    State(state): State<AppState>,
    Json(views): Json<Vec<custom_views::CustomView>>,
) -> Result<Json<Vec<custom_views::CustomView>>, ApiError> {
    custom_views::validate(&views)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    custom_views::replace(&Settings::new(state.store.root()), &views)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "views_updated".into(),
        id: String::new(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
    Ok(Json(views))
}

pub(crate) async fn list_checkout_custom_views(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<custom_views::CustomView>>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    custom_views::from_settings(&settings)
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
}

pub(crate) async fn save_checkout_custom_views(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(views): Json<Vec<custom_views::CustomView>>,
) -> Result<Json<Vec<custom_views::CustomView>>, ApiError> {
    custom_views::validate(&views)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    custom_views::replace(&settings, &views)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state.emit(ChangeEvent {
        cursor: None,
        store: checkout.id,
        kind: "views_updated".into(),
        id: String::new(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
    Ok(Json(views))
}

pub(crate) async fn run_command(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<commands::CommandRun>), ApiError> {
    state
        .commands
        .start(&id)
        .map(|r| (StatusCode::ACCEPTED, Json(r)))
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))
}

pub(crate) async fn run_checkout_command(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<commands::CommandRun>), ApiError> {
    let (checkout, settings) = checkout_settings(&state, &reference)?;
    let definitions = hotsheet_ticketing::commands::from_settings(&settings)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    state
        .commands
        .replace_project(checkout.id.clone(), checkout.root.into(), definitions);
    state
        .commands
        .start_for(&checkout.id, &id)
        .map(|run| (StatusCode::ACCEPTED, Json(run)))
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))
}

pub(crate) async fn list_command_runs(
    State(state): State<AppState>,
) -> Json<Vec<commands::CommandRun>> {
    Json(state.commands.list())
}

pub(crate) async fn list_checkout_command_runs(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<commands::CommandRun>>, ApiError> {
    let (checkout, _) = checkout_settings(&state, &reference)?;
    Ok(Json(state.commands.list_for(&checkout.id)))
}

#[derive(Deserialize)]
pub(crate) struct RunOutputQuery {
    #[serde(default)]
    pub(crate) after: u64,
}

pub(crate) async fn get_command_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RunOutputQuery>,
) -> Result<Json<commands::CommandRun>, ApiError> {
    state
        .commands
        .get(&id, query.after)
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown command run"))
}

pub(crate) async fn get_checkout_command_run(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Query(query): Query<RunOutputQuery>,
) -> Result<Json<commands::CommandRun>, ApiError> {
    let (checkout, _) = checkout_settings(&state, &reference)?;
    state
        .commands
        .get_for(&checkout.id, &id, query.after)
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown command run"))
}

pub(crate) async fn cancel_command_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<commands::CommandRun>, ApiError> {
    state
        .commands
        .cancel(&id)
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::CONFLICT, e))
}

pub(crate) async fn cancel_checkout_command_run(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
) -> Result<Json<commands::CommandRun>, ApiError> {
    let (checkout, _) = checkout_settings(&state, &reference)?;
    state
        .commands
        .cancel_for(&checkout.id, &id)
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error))
}

#[derive(Deserialize)]
pub(crate) struct NotificationQuery {
    pub(crate) checkout: Option<String>,
    pub(crate) store: Option<String>,
    pub(crate) ticket: Option<String>,
    pub(crate) recipient: Option<String>,
}
pub(crate) async fn list_notifications(
    State(state): State<AppState>,
    Query(q): Query<NotificationQuery>,
) -> Json<Vec<notifications::Notification>> {
    Json(state.notifications.list(
        q.checkout.as_deref(),
        q.store.as_deref(),
        q.ticket.as_deref(),
        q.recipient.as_deref(),
    ))
}
pub(crate) async fn publish_notification(
    State(state): State<AppState>,
    Json(body): Json<notifications::NewNotification>,
) -> (StatusCode, Json<notifications::Notification>) {
    let n = state.notifications.publish(body);
    state.emit(ChangeEvent {
        cursor: None,
        store: n.store.clone().unwrap_or_default(),
        kind: "notification".into(),
        id: n.id.clone(),
        slug: n.ticket.clone().unwrap_or_default(),
        message: Some(n.message.clone()),
        activity: None,
        assignment: None,
        turn: None,
    });
    (StatusCode::CREATED, Json(n))
}
pub(crate) async fn acknowledge_notification(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<notifications::Notification>, ApiError> {
    state
        .notifications
        .acknowledge(&id)
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown notification"))
}
pub(crate) async fn synthesize_speech(
    State(state): State<AppState>,
    Json(body): Json<tts::TtsRequest>,
) -> Result<Response, ApiError> {
    let audio = state
        .tts
        .synthesize(&body)
        .map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, e))?;
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", audio.content_type)
        .body(axum::body::Body::from(audio.bytes))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}
