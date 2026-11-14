//! HS2-XHQ1EH: orchestrator permission requests through this server's permission
//! bridge (`docs/05` §5.7).
//!
//! The orchestration runtime files a request in its machine-local ledger and asks the
//! [`BridgePrompt`] to show it. The bridge queues it with the `orchestrator` requester
//! kind and no waiter, so it reaches clients as the ordinary `permission_asked` nudge
//! and permission card. `POST /permissions/{id}` routes the answer to the runtime,
//! which records it, remembers an Always Allow as a rule, wakes the scheduler, and
//! settles the card with the ordinary `permission_resolved` event. Remembered
//! orchestrator rules are listed and forgotten in Settings → Permissions with the
//! other rules.
use crate::api_error::ApiError;
use crate::{AppState, ChangeEvent};
use axum::http::StatusCode;
use hotsheet_cli::orchestration::{OrchestrationRuntime, PermissionPrompt, PromptRequest};
use serde_json::{Value, json};
use std::sync::Arc;

/// The connection id an orchestrator request is raised on: one per orchestration
/// project, so the card attributes it to that project's orchestrator.
fn connection(owner_id: &str) -> String {
    format!("orchestrator:{owner_id}")
}

/// Shows waiting orchestrator requests in this server's permission bridge.
pub(crate) struct BridgePrompt {
    pub(crate) state: AppState,
}

impl PermissionPrompt for BridgePrompt {
    fn raise(&self, request: &PromptRequest) {
        let key = &request.request.key;
        let owner = &key.target.owner_checkout_id;
        self.state.permissions.enqueue_orchestrator(
            format!("checkout:{owner}"),
            connection(owner),
            request.summary.clone(),
            hotsheet_aitools::OrchestratorAsk {
                request_id: request.request.request_id.clone(),
                owner_checkout_id: owner.clone(),
                owner_label: request.owner_label.clone(),
                target_id: key.target.target_id.clone(),
                target_label: request.target_label.clone(),
                action: key.action.code().into(),
                tool: key.tool.clone(),
                model: request.request.model.clone(),
                operation_id: request.request.operation_id.clone(),
                each_time: request.request.each_time,
            },
        );
    }

    fn settle(&self, request_id: &str, decision: &str) {
        if let Some(settled) = self
            .state
            .permissions
            .settle_orchestrator(None, Some(request_id))
        {
            resolved_event(&self.state, &settled, decision);
        }
    }
}

/// The ordinary nudge that moves a card into Notifications history.
fn resolved_event(state: &AppState, request: &hotsheet_aitools::PermissionRequest, decision: &str) {
    state.emit(ChangeEvent {
        live: None,
        cursor: None,
        store: request.project.clone(),
        kind: "permission_resolved".to_string(),
        id: request.id.to_string(),
        slug: String::new(),
        message: Some(decision.into()),
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// The shared runtime with this server's prompt, on a blocking thread.
async fn with_runtime<T: Send + 'static>(
    state: &AppState,
    work: impl FnOnce(Arc<OrchestrationRuntime>) -> T + Send + 'static,
) -> Option<T> {
    let state = state.clone();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        crate::routes::orchestration::server_runtime(&state, handle)
            .ok()
            .map(work)
    })
    .await
    .ok()
    .flatten()
}

/// Bring the bridge in line with the ledger: show every waiting request (one filed by
/// a headless CLI, or before this server started), and settle a card whose request
/// was answered elsewhere. Cheap: one machine-local file read.
pub(crate) async fn sync_waiting(state: &AppState) {
    let shown: Vec<hotsheet_aitools::PermissionRequest> = state
        .permissions
        .pending()
        .into_iter()
        .filter(|request| request.orchestrator.is_some())
        .collect();
    let settled = with_runtime(state, move |runtime| {
        let now_ms = crate::routes::permissions::now_ms();
        runtime.show_waiting_permissions(now_ms);
        shown
            .into_iter()
            .filter_map(|request| {
                let ask = request.orchestrator.as_ref()?;
                (!runtime.permission_waiting(&ask.owner_checkout_id, &ask.request_id, now_ms)).then(
                    || {
                        let decision = runtime.permission_decision(
                            &ask.owner_checkout_id,
                            &ask.request_id,
                            now_ms,
                        );
                        (request.id, decision)
                    },
                )
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    for (id, decision) in settled {
        if let Some(request) = state.permissions.settle_orchestrator(Some(id), None) {
            resolved_event(state, &request, decision);
        }
    }
}

/// `POST /permissions/{id}` for an orchestrator request: Allow Once (`allow`/`once`),
/// Always Allow (`allow`/`always`) or Deny (`deny`/`once`). Chat and session answers
/// and rule review do not apply to it.
pub(crate) async fn resolve(
    state: &AppState,
    request: hotsheet_aitools::PermissionRequest,
    decision: hotsheet_aitools::PermissionDecision,
    scope: hotsheet_aitools::PermissionScope,
    reviewed: bool,
) -> Result<(String, bool, Vec<u64>), ApiError> {
    use hotsheet_aitools::{PermissionDecision as D, PermissionScope as S};
    let answer = match (decision, scope) {
        (D::Allow, S::Once) => "allow_once",
        (D::Allow, S::Always) => "always_allow",
        (D::Deny, S::Once) => "deny",
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "an orchestrator request is allowed once, always allowed, or denied",
            ));
        }
    };
    if reviewed {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "an orchestrator request has no rule to review",
        ));
    }
    let Some(ask) = request.orchestrator.clone() else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "no pending orchestrator request",
        ));
    };
    let before: Vec<u64> = orchestrator_ids(state);
    let path = format!(
        "/checkouts/{}/orchestration/permissions/{}",
        ask.owner_checkout_id, ask.request_id
    );
    let body = json!({"answer": answer, "actor": {"role": "human"}});
    let answered = with_runtime(state, move |runtime| {
        runtime.request_now("POST", &path, &body)
    })
    .await
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "orchestration runtime unavailable",
        )
    })?
    .map_err(crate::routes::orchestration::orchestration_error)?;
    // The runtime settled the card and every request the rule answered; a repeated
    // answer settled nothing, so settle this card here.
    if state
        .permissions
        .settle_orchestrator(Some(request.id), None)
        .is_some()
    {
        resolved_event(
            state,
            &request,
            &format!("{}:{}", decision_code(decision), scope_code(scope)),
        );
    }
    let remembered = answered.get("rule").is_some_and(|rule| !rule.is_null());
    if remembered {
        crate::routes::terminals::emit_permission_rules_changed(state, &request.project);
    }
    let after = orchestrator_ids(state);
    let also = before
        .into_iter()
        .filter(|id| *id != request.id && !after.contains(id))
        .collect();
    Ok((request.connection, remembered, also))
}

fn orchestrator_ids(state: &AppState) -> Vec<u64> {
    state
        .permissions
        .pending()
        .into_iter()
        .filter(|request| request.orchestrator.is_some())
        .map(|request| request.id)
        .collect()
}

fn decision_code(decision: hotsheet_aitools::PermissionDecision) -> &'static str {
    match decision {
        hotsheet_aitools::PermissionDecision::Allow => "allow",
        hotsheet_aitools::PermissionDecision::Deny => "deny",
    }
}

fn scope_code(scope: hotsheet_aitools::PermissionScope) -> &'static str {
    match scope {
        hotsheet_aitools::PermissionScope::Once => "once",
        hotsheet_aitools::PermissionScope::Chat => "chat",
        hotsheet_aitools::PermissionScope::Session => "session",
        hotsheet_aitools::PermissionScope::Always => "always",
    }
}

/// Remembered orchestrator rules in the shape Settings → Permissions lists.
pub(crate) async fn rules(state: &AppState) -> Vec<hotsheet_aitools::PermissionRuleInfo> {
    with_runtime(state, |runtime| runtime.permission_rules_everywhere())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|view| {
            let key = &view.rule.key;
            hotsheet_aitools::PermissionRuleInfo {
                project: format!("checkout:{}", key.target.owner_checkout_id),
                connection: None,
                tool: hotsheet_aitools::ORCHESTRATOR_TOOL.into(),
                action: format!("orchestrator:{}", view.rule.rule_id),
                exact_action: Some(view.summary.clone()),
                match_type: "exact",
                pattern: None,
                description: Some(view.summary.clone()),
                decision: hotsheet_aitools::PermissionDecision::Allow,
                scope: hotsheet_aitools::PermissionScope::Always,
                orchestrator: Some(hotsheet_aitools::OrchestratorRule {
                    rule_id: view.rule.rule_id.clone(),
                    owner_checkout_id: key.target.owner_checkout_id.clone(),
                    owner_label: view.owner_label.clone(),
                    target_id: key.target.target_id.clone(),
                    target_label: view.target_label.clone(),
                    action: key.action.code().into(),
                    tool: key.tool.clone(),
                }),
            }
        })
        .collect()
}

/// `DELETE /permissions/rules` for an orchestrator rule: forget it in the runtime.
pub(crate) async fn forget(state: &AppState, project: &str, action: &str) -> Result<(), ApiError> {
    let (Some(owner), Some(rule_id)) = (
        project.strip_prefix("checkout:"),
        action.strip_prefix("orchestrator:"),
    ) else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "no such remembered rule",
        ));
    };
    let path = format!("/checkouts/{owner}/orchestration/permission-rules/{rule_id}");
    let body: Value = json!({"actor": {"role": "human"}});
    with_runtime(state, move |runtime| {
        runtime.request_now("DELETE", &path, &body)
    })
    .await
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "orchestration runtime unavailable",
        )
    })?
    .map_err(crate::routes::orchestration::orchestration_error)?;
    crate::routes::terminals::emit_permission_rules_changed(state, project);
    Ok(())
}

/// Whether the bridge request with this id is an orchestrator request.
pub(crate) fn pending_orchestrator(
    state: &AppState,
    id: u64,
) -> Option<hotsheet_aitools::PermissionRequest> {
    state
        .permissions
        .pending()
        .into_iter()
        .find(|request| request.id == id && request.orchestrator.is_some())
}
