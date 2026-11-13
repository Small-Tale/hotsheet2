//! The server's side of asking an AI to finish a ticket-store sync it cannot finish alone
//! (HS2-QR1K8R, `docs/02` §2.12.4). The sync loop calls [`ask_after_pass`] after every
//! pass; the shared workflow ([`hotsheet_ticketing::sync_ai::ask_for_help`]) decides
//! whether to ask, and [`ServerReach`] reaches the AI: for `start` it starts the
//! project's default AI tool as a new background drive whose approvals go through the
//! server's permission bridge; for `session` it pushes the prompt into the project's
//! primary push-capable terminal.

use std::path::{Path, PathBuf};

use hotsheet_aitools::session_push::PushMessage;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::Settings;
use hotsheet_ticketing::sync_ai::{
    AiReach, AskContext, AskOutcome, SessionDone, ask_ai_mode, ask_for_help, configured_ai_tool,
    store_reference,
};

use crate::AppState;
use crate::routes::{feedback_session, session_messages};

/// The project a store is asked about: the first checkout that links it, else the store.
struct Project {
    root: PathBuf,
    settings: Settings,
    label: String,
}

fn project_for(state: &AppState, store_root: &Path) -> Project {
    let label = store_root.file_name().map_or_else(
        || store_root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let checkout = state.checkout_registry.list().ok().and_then(|checkouts| {
        checkouts.into_iter().find(|checkout| {
            checkout
                .stores
                .iter()
                .any(|root| crate::watcher::same_path(Path::new(root), store_root))
        })
    });
    match checkout {
        Some(checkout) => Project {
            root: PathBuf::from(&checkout.root),
            settings: checkout.settings(),
            label,
        },
        None => Project {
            root: store_root.to_path_buf(),
            settings: Settings::for_project(store_root),
            label,
        },
    }
}

/// Ask for help with the store's stall after a sync pass, if it has one (see the module
/// docs). Cheap when the store is healthy: a few local reads.
pub fn ask_after_pass(state: &AppState, store_id: &str, store_root: &Path) -> AskOutcome {
    let project = project_for(state, store_root);
    let context = AskContext {
        mode: ask_ai_mode(&project.settings),
        label: project.label.clone(),
        store_reference: store_reference(&project.root, store_root),
    };
    let reach = ServerReach {
        state: state.clone(),
        store_id: store_id.to_string(),
        store_root: store_root.to_path_buf(),
        project,
    };
    let outcome = ask_for_help(store_root, &context, &reach);
    if matches!(outcome, AskOutcome::Asked(_)) {
        announce(state, store_id, store_root);
    }
    outcome
}

/// Tell open clients the store's sync health changed (the ask is part of it).
fn announce(state: &AppState, store_id: &str, store_root: &Path) {
    let health = hotsheet_ticketing::sync_health::health(store_root);
    state.announce_ticket_store_sync(store_id, store_root, health.state);
}

/// How the server reaches an AI for one store.
pub struct ServerReach {
    state: AppState,
    store_id: String,
    store_root: PathBuf,
    project: Project,
}

impl AiReach for ServerReach {
    fn push_to_session(&self, prompt: &str) -> Result<Option<String>, String> {
        let project = self.project.root.to_string_lossy();
        // The project's primary session: the oldest push-capable terminal (HS2-DB2FQ2).
        let primary = feedback_session::push_capable_terminals(&self.state, None)
            .into_iter()
            .filter(|terminal| {
                crate::watcher::same_path(Path::new(&terminal.project), Path::new(&*project))
            })
            .min_by(|a, b| (a.started_at_ms, &a.id).cmp(&(b.started_at_ms, &b.id)));
        let Some(terminal) = primary else {
            return Ok(None);
        };
        let adapter = session_messages::adapter(&self.state, &terminal.id)
            .ok_or_else(|| "the AI session is no longer reachable".to_string())?;
        adapter
            .message(&PushMessage::Prompt {
                text: prompt.to_string(),
                banner: None,
            })
            .map_err(|error| error.to_string())?;
        Ok(Some(terminal.id))
    }

    fn drivable_tool(&self) -> Result<String, String> {
        let tool = configured_ai_tool(&self.project.settings).ok_or_else(|| {
            "no default AI tool is configured for this project (choose one in the AI settings)"
                .to_string()
        })?;
        let plugin = hotsheet_plugins::find_in(&tool, &self.state.plugin_dirs)
            .ok_or_else(|| format!("the default AI tool {tool} has no Hot Sheet plugin"))?;
        if plugin.manifest.drive.is_none() {
            return Err(format!(
                "the default AI tool {tool} cannot be driven by Hot Sheet"
            ));
        }
        let detected = hotsheet_plugins::detected_drivable_plugins(&self.state.plugin_dirs)
            .iter()
            .any(|candidate| candidate.id() == tool);
        if !detected {
            return Err(format!(
                "the default AI tool {tool} is not installed on this machine"
            ));
        }
        Ok(tool)
    }

    fn start_session(&self, tool: &str, prompt: &str, done: SessionDone) -> Result<String, String> {
        let state = &self.state;
        let mut envs = vec![
            format!("HOTSHEET_PROJECT={}", self.project.root.display()),
            format!(
                "HOTSHEET_PERMISSION_PROJECT=unregistered:{}",
                self.store_root.display()
            ),
        ];
        // Lets a driven tool's permission hook reach the route-back (HS2-XCTAHM).
        if let Some(url) = state.terminal_server_url.lock_or_recover().clone() {
            envs.push(format!("HOTSHEET_SERVER={url}"));
            envs.push(format!("HOTSHEET_SECRET={}", state.secret));
        }
        let trigger = hotsheet_aitools::prepare_trigger_with_home(
            &self.store_root,
            tool,
            hotsheet_aitools::TriggerOptions {
                project: Some(self.project.root.clone()),
                envs,
                journal_home: Some(state.machine_home().to_path_buf()),
                plugin_dirs: Some(state.plugin_dirs.to_vec()),
                ..hotsheet_aitools::TriggerOptions::default()
            },
        )
        .map_err(|error| error.to_string())?
        // Approvals block for a person answering over POST /permissions, never auto-approve.
        .with_permission_bridge(state.permission_bridge());
        let connection = format!("sync-repair-{}", self.store_id);
        {
            let mut registry = state.drive_registry.lock_or_recover();
            registry.register(hotsheet_aitools::Connection {
                id: connection.clone(),
                project: self.project.root.display().to_string(),
                tool: tool.to_string(),
                role: hotsheet_aitools::Role::Worker,
                transport: hotsheet_aitools::Transport::AppServer,
                pid: None,
                started_at_ms: crate::now_ms(),
            });
            registry.note_activity(&connection, crate::now_ms());
        }
        let state = state.clone();
        let (store_id, store_root) = (self.store_id.clone(), self.store_root.clone());
        let (prompt, id) = (prompt.to_string(), connection.clone());
        std::thread::Builder::new()
            .name("hotsheet-sync-repair".into())
            .spawn(move || {
                let mut turns = hotsheet_aitools::ConnectionRegistry::new(30_000);
                let result =
                    trigger.run_turn(&prompt, None, true, id.clone(), &mut turns, &mut |_| {});
                state.drive_registry.lock_or_recover().unregister(&id);
                let result = match result {
                    Ok(done) => match done.reason {
                        hotsheet_aitools::DoneReason::Completed => Ok(()),
                        hotsheet_aitools::DoneReason::Interrupted => {
                            Err("the session was interrupted".to_string())
                        }
                        hotsheet_aitools::DoneReason::Failed { exit_code, message } => Err(message
                            .unwrap_or_else(|| format!("the tool exited with status {exit_code}"))),
                    },
                    Err(error) => Err(error.to_string()),
                };
                done(result);
                announce(&state, &store_id, &store_root);
            })
            .map_err(|error| error.to_string())?;
        Ok(connection)
    }
}
