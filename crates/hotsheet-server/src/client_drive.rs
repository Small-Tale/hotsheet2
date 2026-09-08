//! Client-owned AI connections: prepare once, send sequential turns, and expose an
//! interrupt action only when the resolved drive implements it (HS2-5DGFG2).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hotsheet_aitools::{
    ConnectionRegistry, SafeTrigger, SharedPermissionBridge, TurnControl, TurnDone, TurnEvent,
    prepare_trigger,
};
use serde::Serialize;

pub struct PrepareDrive {
    pub store_path: PathBuf,
    pub project_path: PathBuf,
    pub tool: String,
    pub env: Vec<String>,
    pub permission_bridge: Arc<SharedPermissionBridge>,
}

/// Prepared drive boundary. Tests inject a fake here; production delegates to SafeTrigger,
/// retaining its isolated home across turns.
pub trait PreparedClientDrive: Send + Sync {
    fn tool(&self) -> &str;
    fn supports_interrupt(&self) -> bool;
    fn run_turn(
        &self,
        prompt: &str,
        resume: Option<&str>,
        connection_id: &str,
        control: &TurnControl,
        on_event: &mut dyn FnMut(&TurnEvent),
    ) -> Result<TurnDone, String>;
}

pub trait ClientDriveBackend: Send + Sync {
    fn prepare(&self, request: PrepareDrive) -> Result<Arc<dyn PreparedClientDrive>, String>;
}

#[derive(Default)]
pub struct NativeClientDriveBackend;

struct NativePreparedDrive(SafeTrigger);

impl ClientDriveBackend for NativeClientDriveBackend {
    fn prepare(&self, request: PrepareDrive) -> Result<Arc<dyn PreparedClientDrive>, String> {
        let trigger = prepare_trigger(
            &request.store_path,
            &request.tool,
            Some(request.project_path),
            None,
            None,
            request.env,
            // The shared daemon uses a Unix-domain control socket. Windows uses the
            // direct app-server transport while preserving the same client API/session.
            cfg!(unix),
        )
        .map_err(|error| error.to_string())?
        .with_permission_bridge(request.permission_bridge);
        Ok(Arc::new(NativePreparedDrive(trigger)))
    }
}

impl PreparedClientDrive for NativePreparedDrive {
    fn tool(&self) -> &str {
        self.0.tool()
    }

    fn supports_interrupt(&self) -> bool {
        self.0.supports_interrupt()
    }

    fn run_turn(
        &self,
        prompt: &str,
        resume: Option<&str>,
        connection_id: &str,
        control: &TurnControl,
        on_event: &mut dyn FnMut(&TurnEvent),
    ) -> Result<TurnDone, String> {
        let mut registry = ConnectionRegistry::new(30_000);
        self.0
            .run_turn_controlled(
                prompt,
                resume,
                false,
                connection_id.to_owned(),
                &mut registry,
                control,
                on_event,
            )
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClientConnectionInfo {
    pub id: String,
    pub tool: String,
    pub project: String,
    pub role: String,
    pub busy: bool,
    pub actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

struct ConnectionState {
    busy: bool,
    session_id: Option<String>,
    control: Option<TurnControl>,
    last_error: Option<String>,
}

struct ClientConnection {
    id: String,
    tool: String,
    project: String,
    drive: Arc<dyn PreparedClientDrive>,
    state: Mutex<ConnectionState>,
}

#[derive(Clone)]
pub struct ClientDriveManager {
    backend: Arc<dyn ClientDriveBackend>,
    connections: Arc<Mutex<HashMap<String, Arc<ClientConnection>>>>,
}

impl Default for ClientDriveManager {
    fn default() -> Self {
        Self::new(Arc::new(NativeClientDriveBackend))
    }
}

impl ClientDriveManager {
    pub fn new(backend: Arc<dyn ClientDriveBackend>) -> Self {
        Self {
            backend,
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn create_or_attach(
        &self,
        request: PrepareDrive,
        requested_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<ClientConnectionInfo, ClientDriveError> {
        let id = requested_id.unwrap_or_else(|| {
            format!("ai-{}", ulid::Ulid::new().to_string().to_ascii_lowercase())
        });
        if let Some(existing) = self.connection(&id)? {
            if existing.tool != request.tool
                || existing.project != request.project_path.display().to_string()
            {
                return Err(ClientDriveError::Conflict(format!(
                    "connection '{id}' belongs to another project or tool"
                )));
            }
            return Ok(connection_info(&existing));
        }

        let project = request.project_path.display().to_string();
        let drive = self
            .backend
            .prepare(request)
            .map_err(ClientDriveError::Prepare)?;
        let tool = drive.tool().to_owned();
        let connection = Arc::new(ClientConnection {
            id: id.clone(),
            tool,
            project,
            drive,
            state: Mutex::new(ConnectionState {
                busy: false,
                session_id,
                control: None,
                last_error: None,
            }),
        });
        self.connections
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?
            .insert(id, connection.clone());
        Ok(connection_info(&connection))
    }

    pub fn begin_turn(
        &self,
        id: &str,
        explicit_session: Option<String>,
    ) -> Result<ClientTurnJob, ClientDriveError> {
        let connection = self
            .connection(id)?
            .ok_or_else(|| ClientDriveError::NotFound(id.into()))?;
        let mut state = connection
            .state
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?;
        if state.busy {
            return Err(ClientDriveError::Conflict(format!(
                "connection '{id}' already has a running turn"
            )));
        }
        let control = TurnControl::default();
        state.busy = true;
        state.last_error = None;
        state.control = Some(control.clone());
        let resume = explicit_session.or_else(|| state.session_id.clone());
        drop(state);
        Ok(ClientTurnJob {
            connection,
            control,
            resume,
        })
    }

    pub fn finish_turn(&self, id: &str, result: &Result<TurnDone, String>) {
        let Ok(Some(connection)) = self.connection(id) else {
            return;
        };
        let Ok(mut state) = connection.state.lock() else {
            return;
        };
        state.busy = false;
        state.control = None;
        match result {
            Ok(done) => {
                if done.session_id.is_some() {
                    state.session_id.clone_from(&done.session_id);
                }
            }
            Err(error) => state.last_error = Some(error.clone()),
        }
    }

    pub fn interrupt(&self, id: &str) -> Result<(), ClientDriveError> {
        let connection = self
            .connection(id)?
            .ok_or_else(|| ClientDriveError::NotFound(id.into()))?;
        if !connection.drive.supports_interrupt() {
            return Err(ClientDriveError::Unsupported(format!(
                "connection '{id}' does not expose an interrupt action"
            )));
        }
        let state = connection
            .state
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?;
        let control = state.control.as_ref().ok_or_else(|| {
            ClientDriveError::Conflict(format!("connection '{id}' has no running turn"))
        })?;
        control.request_interrupt();
        Ok(())
    }

    pub fn list(&self) -> Vec<ClientConnectionInfo> {
        let Ok(connections) = self.connections.lock() else {
            return Vec::new();
        };
        let mut infos = connections
            .values()
            .map(|connection| connection_info(connection))
            .collect::<Vec<_>>();
        infos.sort_by(|left, right| left.id.cmp(&right.id));
        infos
    }

    pub fn get(&self, id: &str) -> Result<ClientConnectionInfo, ClientDriveError> {
        self.connection(id)?
            .map(|connection| connection_info(&connection))
            .ok_or_else(|| ClientDriveError::NotFound(id.into()))
    }

    fn connection(&self, id: &str) -> Result<Option<Arc<ClientConnection>>, ClientDriveError> {
        self.connections
            .lock()
            .map(|connections| connections.get(id).cloned())
            .map_err(|_| ClientDriveError::Unavailable)
    }
}

pub struct ClientTurnJob {
    connection: Arc<ClientConnection>,
    control: TurnControl,
    resume: Option<String>,
}

impl ClientTurnJob {
    pub fn run(
        &self,
        prompt: &str,
        on_event: &mut dyn FnMut(&TurnEvent),
    ) -> Result<TurnDone, String> {
        self.connection.drive.run_turn(
            prompt,
            self.resume.as_deref(),
            &self.connection.id,
            &self.control,
            on_event,
        )
    }
}

fn connection_info(connection: &ClientConnection) -> ClientConnectionInfo {
    let state = connection.state.lock().ok();
    let mut actions = vec!["send_turn".into()];
    if connection.drive.supports_interrupt() {
        actions.push("interrupt".into());
    }
    ClientConnectionInfo {
        id: connection.id.clone(),
        tool: connection.tool.clone(),
        project: connection.project.clone(),
        role: "main".into(),
        busy: state.as_ref().is_some_and(|state| state.busy),
        actions,
        session_id: state.as_ref().and_then(|state| state.session_id.clone()),
        last_error: state.and_then(|state| state.last_error.clone()),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientDriveError {
    #[error("preparing client drive: {0}")]
    Prepare(String),
    #[error("connection '{0}' was not found")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("client drive state is unavailable")]
    Unavailable,
}
