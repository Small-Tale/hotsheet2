//! HS2-GDJXGZ: this server as the terminal host of orchestrator-started sessions
//! (`docs/26-orchestration-projects.md` §26.16).
//!
//! Nothing here is a second launch path. A session starts through the same
//! `POST /terminals` connect handler the drawer uses, is listed from the same terminal
//! list, stops through the same kill handler, and is labelled by the saved tab name
//! every client of this server already shows.
use crate::AppState;
use crate::routes::terminals::{OpenTerminalReq, try_live_terminal_infos};
use axum::Json;
use axum::extract::{Path, State};
use hotsheet_aitools::session_push::PushMessage;
use hotsheet_cli::orchestration::{
    HostLaunch, HostSessionState, RolePrompt, SessionHost, SessionHostError, role_prompt_text,
};
use hotsheet_ticketing::checkouts::Checkout;
use hotsheet_ticketing::orchestration::SessionRole;

/// The host the shared orchestration runtime asks from inside this server. It is
/// called on a blocking thread, so it may wait for the terminal host's answer.
pub(crate) struct ServerSessions {
    pub(crate) state: AppState,
    pub(crate) runtime: tokio::runtime::Handle,
}

impl SessionHost for ServerSessions {
    fn start(&self, checkout: &Checkout, launch: &HostLaunch) -> Result<(), SessionHostError> {
        let request = OpenTerminalReq {
            command: None,
            shell_command: None,
            args: Vec::new(),
            cwd: Some(checkout.root.clone()),
            id: Some(launch.session_id.clone()),
            connect: Some(launch.tool.clone()),
            model: launch.model.clone(),
            effort: launch.effort.clone(),
        };
        self.runtime
            .block_on(crate::open_terminal(
                State(self.state.clone()),
                Json(request),
            ))
            .map(|_| ())
            .map_err(|_| SessionHostError::Refused)?;
        // The label is the saved tab name; a session that could not be named still runs.
        if let Ok(name) = serde_json::from_value(serde_json::json!({"name": launch.label})) {
            let _ = self.runtime.block_on(crate::rename_terminal(
                State(self.state.clone()),
                Path(launch.session_id.clone()),
                Json(name),
            ));
        }
        Ok(())
    }

    fn sessions(&self, _: &Checkout) -> Option<Vec<HostSessionState>> {
        let infos = self
            .runtime
            .block_on(try_live_terminal_infos(&self.state))
            .ok()?;
        Some(
            infos
                .into_iter()
                .filter(|info| info.alive && info.kind == hotsheet_terminals::TerminalKind::Ai)
                .map(|info| HostSessionState {
                    session_id: info.id,
                    busy: info.busy,
                })
                .collect(),
        )
    }

    fn stop(&self, _: &Checkout, session_id: &str) -> Result<(), SessionHostError> {
        self.runtime
            .block_on(crate::kill_terminal(
                State(self.state.clone()),
                Path(session_id.to_owned()),
            ))
            .map(|_| ())
            .map_err(|_| SessionHostError::Refused)
    }

    fn join_role(&self, _: &Checkout, session_id: &str, role: SessionRole) -> RolePrompt {
        let Some(adapter) = crate::routes::session_messages::adapter(&self.state, session_id)
        else {
            return RolePrompt::Unsupported;
        };
        match adapter.message(&PushMessage::Prompt {
            text: role_prompt_text(role),
            banner: None,
        }) {
            Ok(_) => RolePrompt::Delivered,
            Err(_) => RolePrompt::Failed,
        }
    }
}
