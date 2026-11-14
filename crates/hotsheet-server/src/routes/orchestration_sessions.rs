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
use hotsheet_aitools::session_push::{PushError, PushMessage};
use hotsheet_cli::orchestration::{
    HostLaunch, HostSessionState, Launched, PushAttempt, SessionHost, SessionHostError, ToolSetup,
    plugin_role_paths, target_tool_setup,
};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::checkouts::Checkout;

/// The host the shared orchestration runtime asks from inside this server. It is
/// called on a blocking thread, so it may wait for the terminal host's answer.
pub(crate) struct ServerSessions {
    pub(crate) state: AppState,
    pub(crate) runtime: tokio::runtime::Handle,
}

impl SessionHost for ServerSessions {
    fn prepare(&self, checkout: &Checkout, tool: &str) -> Result<ToolSetup, SessionHostError> {
        // HS2-WEFJV8: the project's own headless setup, with this server's plugins.
        let setup = target_tool_setup(
            checkout,
            tool,
            Some(self.state.store.root()),
            &self.state.plugin_dirs,
        )?;
        // Setting a tool up changes its plugin state; the next discovery must rescan.
        self.state.invalidate_ai_tool_discovery();
        Ok(setup)
    }

    fn start(
        &self,
        checkout: &Checkout,
        launch: &HostLaunch,
    ) -> Result<Launched, SessionHostError> {
        // HS2-KM9CTG: a stopping server launches nothing. The start stays requested and
        // is repeated on whichever server hosts the checkout next.
        if self.state.is_stopping() {
            return Err(SessionHostError::Unavailable);
        }
        let prompt_at_launch = plugin_role_paths(&launch.tool, &self.state.plugin_dirs)
            .is_some_and(|(at_launch, _)| at_launch);
        let request = OpenTerminalReq {
            command: None,
            shell_command: None,
            args: Vec::new(),
            cwd: Some(checkout.root.clone()),
            id: Some(launch.session_id.clone()),
            connect: Some(launch.tool.clone()),
            model: launch.model.clone(),
            effort: launch.effort.clone(),
            session_role: Some(launch.role.code().to_owned()),
            initial_prompt: Some(launch.role_prompt.clone()),
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
        Ok(Launched { prompt_at_launch })
    }

    fn sessions(&self, _: &Checkout) -> Option<Vec<HostSessionState>> {
        // HS2-KM9CTG: a stopping server cannot vouch for its terminals, so nothing is
        // concluded from it: its sessions are not reported lost as they go down.
        if self.state.is_stopping() {
            return None;
        }
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

    fn stopping(&self) -> bool {
        self.state.is_stopping()
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

    fn join_role(&self, _: &Checkout, session_id: &str, text: &str) -> PushAttempt {
        let Some(adapter) = crate::routes::session_messages::adapter(&self.state, session_id)
        else {
            // No address yet. A tool that can take messages registers one when it is
            // ready (a SessionStart hook reports it); one that cannot never will.
            let tool = self
                .state
                .drive_registry
                .lock_or_recover()
                .get(session_id)
                .map(|connection| connection.tool.clone());
            let pushes = tool
                .and_then(|tool| plugin_role_paths(&tool, &self.state.plugin_dirs))
                .is_some_and(|(_, pushes)| pushes);
            return if pushes {
                PushAttempt::NotReady
            } else {
                PushAttempt::Unsupported
            };
        };
        match adapter.message(&PushMessage::Prompt {
            text: text.to_owned(),
            banner: None,
        }) {
            Ok(_) => PushAttempt::Delivered,
            Err(PushError::Unreachable(_)) => PushAttempt::NotReady,
            Err(PushError::Unsupported(_)) => PushAttempt::Unsupported,
            Err(PushError::Invalid(_) | PushError::Rejected { .. }) => PushAttempt::Rejected,
        }
    }
}

/// HS2-257Q2Z: the change-event kind of a hosted session's exit or finished turn, on
/// the event stream of its checkout. The hosted target feed carries it to an
/// orchestrator on another server. Identifiers only.
pub(crate) const SESSION_CHANGE_KIND: &str = "orchestration_session";

/// HS2-95SWQ4: an AI terminal of this server ended or finished a turn. When an
/// orchestrator started it, the scheduler following on this machine is woken through
/// its marker, so it learns of the exit or the safe point from the change itself and
/// never by asking on a timer. HS2-257Q2Z: the change is also published on this
/// server's event stream for the session's checkout, so an orchestrator on another
/// server following that target learns it the same way. Any other terminal changes
/// nothing.
pub(crate) fn note_session_change(state: &AppState, terminal_id: &str) {
    // HS2-2DPGDY: a finished turn is a sign the tool is ready for its role.
    retry_role(state, terminal_id);
    let state = state.clone();
    let terminal_id = terminal_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let home = state.machine_home().to_path_buf();
        let registry = hotsheet_ticketing::orchestration::HostedSessionRegistry::new(&home);
        if !registry.hosts(&terminal_id).unwrap_or(false) {
            return;
        }
        hotsheet_cli::orchestration::OrchestrationRuntime::scheduler_wake_at(&home);
        let Ok(Some(hosted)) = registry.by_session(&terminal_id) else {
            return;
        };
        state.emit(crate::ChangeEvent {
            live: None,
            cursor: None,
            store: hosted.checkout_id,
            kind: SESSION_CHANGE_KIND.into(),
            id: terminal_id,
            slug: String::new(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
    });
}

/// HS2-2DPGDY: an AI terminal of this server may just have become ready for a message:
/// its push address was registered, or it finished a turn. A role prompt an
/// orchestrator's start left pending is tried again, bounded in number and time.
/// Any other terminal changes nothing. Never called on a timer.
pub(crate) fn retry_role(state: &AppState, terminal_id: &str) {
    let hosted =
        hotsheet_ticketing::orchestration::HostedSessionRegistry::new(state.machine_home())
            .by_session(terminal_id)
            .ok()
            .flatten()
            .is_some_and(|session| {
                session
                    .role_delivery
                    .prompt
                    .is_some_and(|prompt| !prompt.is_settled())
            });
    if !hosted {
        return;
    }
    let state = state.clone();
    let terminal_id = terminal_id.to_owned();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let Ok(runtime) = crate::routes::orchestration::server_runtime(&state, handle) else {
            return;
        };
        if let Err(error) = runtime.hosted_role_retry_now(&terminal_id) {
            tracing::warn!(code = error.code(), "role prompt retry failed");
        }
    });
}
