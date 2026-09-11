//! Client-owned AI connections: prepare once, send sequential turns, and expose an
//! interrupt action only when the resolved drive implements it (HS2-5DGFG2).

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hotsheet_aitools::{
    ConnectionRegistry, SafeTrigger, SharedPermissionBridge, TurnControl, TurnDone, TurnEvent,
    prepare_trigger_with_home,
};
use serde::{Deserialize, Serialize};

pub struct PrepareDrive {
    pub store_path: PathBuf,
    pub project_path: PathBuf,
    pub tool: String,
    pub env: Vec<String>,
    pub permission_bridge: Arc<SharedPermissionBridge>,
    pub persistent_home: Option<PathBuf>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

pub struct ClientTurnRequest<'a> {
    pub prompt: &'a str,
    pub resume: Option<&'a str>,
    pub connection_id: &'a str,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub control: &'a TurnControl,
}

/// Prepared drive boundary. Tests inject a fake here; production delegates to SafeTrigger,
/// retaining its isolated home across turns.
pub trait PreparedClientDrive: Send + Sync {
    fn tool(&self) -> &str;
    fn supports_interrupt(&self) -> bool;
    fn run_turn(
        &self,
        request: ClientTurnRequest<'_>,
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
        let trigger = prepare_trigger_with_home(
            &request.store_path,
            &request.tool,
            Some(request.project_path),
            None,
            None,
            request.env,
            // The shared daemon uses a Unix-domain control socket. Windows uses the
            // direct app-server transport while preserving the same client API/session.
            cfg!(unix),
            request.persistent_home,
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
        request: ClientTurnRequest<'_>,
        on_event: &mut dyn FnMut(&TurnEvent),
    ) -> Result<TurnDone, String> {
        let mut registry = ConnectionRegistry::new(30_000);
        self.0
            .run_turn_controlled_with_options(
                request.prompt,
                request.resume,
                request.model,
                request.effort,
                false,
                request.connection_id.to_owned(),
                &mut registry,
                request.control,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

struct ConnectionState {
    busy: bool,
    closing: bool,
    session_id: Option<String>,
    control: Option<TurnControl>,
    last_error: Option<String>,
    active_session_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientSessionInfo {
    pub connection_id: String,
    pub tool: String,
    pub project: String,
    pub session_id: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredClientSession {
    connection_id: String,
    tool: String,
    project: String,
    session_id: String,
    updated_at_ms: u64,
    #[serde(default)]
    home_id: String,
}

impl From<&StoredClientSession> for ClientSessionInfo {
    fn from(session: &StoredClientSession) -> Self {
        Self {
            connection_id: session.connection_id.clone(),
            tool: session.tool.clone(),
            project: session.project.clone(),
            session_id: session.session_id.clone(),
            updated_at_ms: session.updated_at_ms,
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct SessionCatalog {
    #[serde(default)]
    sessions: Vec<StoredClientSession>,
}

struct ClientConnection {
    id: String,
    tool: String,
    project: String,
    home_id: String,
    drive: Arc<dyn PreparedClientDrive>,
    model: Option<String>,
    effort: Option<String>,
    state: Mutex<ConnectionState>,
}

#[derive(Clone)]
pub struct ClientDriveManager {
    backend: Arc<dyn ClientDriveBackend>,
    connections: Arc<Mutex<HashMap<String, Arc<ClientConnection>>>>,
    sessions: Arc<Mutex<SessionCatalog>>,
    session_path: Option<PathBuf>,
    home_root: Option<PathBuf>,
    active_sessions: Arc<Mutex<HashSet<String>>>,
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
            sessions: Arc::new(Mutex::new(SessionCatalog::default())),
            session_path: None,
            home_root: None,
            active_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn with_persistence(
        backend: Arc<dyn ClientDriveBackend>,
        session_path: PathBuf,
        home_root: PathBuf,
    ) -> Result<Self, ClientDriveError> {
        let sessions = if session_path.exists() {
            let text = std::fs::read_to_string(&session_path)
                .map_err(|error| ClientDriveError::Persistence(error.to_string()))?;
            serde_json::from_str(&text)
                .map_err(|error| ClientDriveError::Persistence(error.to_string()))?
        } else {
            SessionCatalog::default()
        };
        Ok(Self {
            backend,
            connections: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(sessions)),
            session_path: Some(session_path),
            home_root: Some(home_root),
            active_sessions: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    pub fn create_or_attach(
        &self,
        mut request: PrepareDrive,
        requested_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<ClientConnectionInfo, ClientDriveError> {
        let id = requested_id.unwrap_or_else(|| {
            format!("ai-{}", ulid::Ulid::new().to_string().to_ascii_lowercase())
        });
        let project = request.project_path.display().to_string();
        let home_id = session_id
            .as_deref()
            .and_then(|session_id| self.stored_session(&project, &request.tool, session_id))
            .map(|session| {
                if session.home_id.is_empty() {
                    connection_home_id(&session.connection_id)
                } else {
                    session.home_id
                }
            })
            .unwrap_or_else(|| connection_home_id(&id));
        if let Some(root) = &self.home_root {
            request.persistent_home = Some(root.join(&home_id));
        }
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

        let model = request.model.clone();
        let effort = request.effort.clone();
        let drive = self
            .backend
            .prepare(request)
            .map_err(ClientDriveError::Prepare)?;
        let tool = drive.tool().to_owned();
        let connection = Arc::new(ClientConnection {
            id: id.clone(),
            tool,
            project,
            home_id,
            drive,
            model,
            effort,
            state: Mutex::new(ConnectionState {
                busy: false,
                closing: false,
                session_id: session_id.clone(),
                control: None,
                last_error: None,
                active_session_key: None,
            }),
        });
        self.connections
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?
            .insert(id, connection.clone());
        if let Some(session_id) = session_id {
            if let Err(error) = self.record_session(&connection, session_id) {
                if let Ok(mut connections) = self.connections.lock() {
                    connections.remove(&connection.id);
                }
                return Err(error);
            }
        }
        Ok(connection_info(&connection))
    }

    pub fn begin_turn(
        &self,
        id: &str,
        explicit_session: Option<String>,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<ClientTurnJob, ClientDriveError> {
        let connection = self
            .connection(id)?
            .ok_or_else(|| ClientDriveError::NotFound(id.into()))?;
        let mut active_sessions = self
            .active_sessions
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?;
        let mut state = connection
            .state
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?;
        if state.closing {
            return Err(ClientDriveError::NotFound(id.into()));
        }
        if state.busy {
            return Err(ClientDriveError::Conflict(format!(
                "connection '{id}' already has a running turn"
            )));
        }
        let resume = explicit_session.or_else(|| state.session_id.clone());
        let active_key = format!(
            "{}\u{1f}{}\u{1f}{}",
            connection.project,
            connection.tool,
            resume.as_deref().unwrap_or(&connection.id)
        );
        if !active_sessions.insert(active_key.clone()) {
            return Err(ClientDriveError::Conflict(format!(
                "session '{}' already has a running turn",
                resume.as_deref().unwrap_or(&connection.id)
            )));
        }
        let control = TurnControl::default();
        state.busy = true;
        state.last_error = None;
        state.control = Some(control.clone());
        state.active_session_key = Some(active_key);
        drop(state);
        Ok(ClientTurnJob {
            connection,
            control,
            resume,
            model,
            effort,
        })
    }

    pub fn finish_turn(&self, job: &ClientTurnJob, result: &Result<TurnDone, String>) {
        let connection = &job.connection;
        let Ok(mut active_sessions) = self.active_sessions.lock() else {
            return;
        };
        let Ok(mut state) = connection.state.lock() else {
            return;
        };
        state.busy = false;
        state.control = None;
        if let Some(key) = state.active_session_key.take() {
            active_sessions.remove(&key);
        }
        match result {
            Ok(done) => {
                if done.session_id.is_some() {
                    state.session_id.clone_from(&done.session_id);
                }
            }
            Err(error) => state.last_error = Some(error.clone()),
        }
        let session_id = state.session_id.clone();
        drop(state);
        if let Some(session_id) = session_id {
            if let Err(error) = self.record_session(connection, session_id)
                && let Ok(mut state) = connection.state.lock()
            {
                state.last_error = Some(error.to_string());
            }
        }
    }

    /// Close one client-owned drive without exposing whether the id belongs to another
    /// checkout. A busy drive is removed only after its interrupt signal is accepted; the
    /// running job retains its `Arc` so `finish_turn` can still release session ownership.
    pub fn close(&self, project: &str, id: &str) -> Result<bool, ClientDriveError> {
        let mut connections = self
            .connections
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?;
        let Some(connection) = connections.get(id).cloned() else {
            return Ok(false);
        };
        if connection.project != project {
            return Ok(false);
        }
        let mut state = connection
            .state
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?;
        if state.busy {
            if !connection.drive.supports_interrupt() {
                return Err(ClientDriveError::Unsupported(format!(
                    "connection '{id}' cannot be closed while its turn is running"
                )));
            }
            state
                .control
                .as_ref()
                .ok_or(ClientDriveError::Unavailable)?
                .request_interrupt();
        }
        state.closing = true;
        connections.remove(id);
        Ok(true)
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

    pub fn sessions(&self, project: &str) -> Vec<ClientSessionInfo> {
        let Ok(catalog) = self.sessions.lock() else {
            return Vec::new();
        };
        let mut sessions = catalog
            .sessions
            .iter()
            .filter(|session| session.project == project)
            .map(ClientSessionInfo::from)
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
        sessions
    }

    fn record_session(
        &self,
        connection: &ClientConnection,
        session_id: String,
    ) -> Result<(), ClientDriveError> {
        let mut catalog = self
            .sessions
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?;
        let mut next = catalog.clone();
        next.sessions.retain(|session| {
            !(session.project == connection.project
                && session.tool == connection.tool
                && session.session_id == session_id)
        });
        next.sessions.push(StoredClientSession {
            connection_id: connection.id.clone(),
            tool: connection.tool.clone(),
            project: connection.project.clone(),
            session_id,
            updated_at_ms: now_ms(),
            home_id: connection.home_id.clone(),
        });
        if let Some(path) = &self.session_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| ClientDriveError::Persistence(error.to_string()))?;
            }
            let body = serde_json::to_string_pretty(&next)
                .map_err(|error| ClientDriveError::Persistence(error.to_string()))?;
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty());
            let parent = parent.unwrap_or_else(|| std::path::Path::new("."));
            let mut temp = tempfile::NamedTempFile::new_in(parent)
                .map_err(|error| ClientDriveError::Persistence(error.to_string()))?;
            temp.write_all((body + "\n").as_bytes())
                .map_err(|error| ClientDriveError::Persistence(error.to_string()))?;
            temp.persist(path)
                .map_err(|error| ClientDriveError::Persistence(error.error.to_string()))?;
        }
        *catalog = next;
        Ok(())
    }

    fn connection(&self, id: &str) -> Result<Option<Arc<ClientConnection>>, ClientDriveError> {
        self.connections
            .lock()
            .map(|connections| connections.get(id).cloned())
            .map_err(|_| ClientDriveError::Unavailable)
    }

    fn stored_session(
        &self,
        project: &str,
        tool: &str,
        session_id: &str,
    ) -> Option<StoredClientSession> {
        self.sessions
            .lock()
            .ok()?
            .sessions
            .iter()
            .find_map(|session| {
                (session.project == project
                    && session.tool == tool
                    && session.session_id == session_id)
                    .then(|| session.clone())
            })
    }
}

fn connection_home_id(connection_id: &str) -> String {
    hotsheet_index::hash_bytes(connection_id.as_bytes())[..16].to_owned()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

pub struct ClientTurnJob {
    connection: Arc<ClientConnection>,
    control: TurnControl,
    resume: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

impl ClientTurnJob {
    pub fn run(
        &self,
        prompt: &str,
        on_event: &mut dyn FnMut(&TurnEvent),
    ) -> Result<TurnDone, String> {
        self.connection.drive.run_turn(
            ClientTurnRequest {
                prompt,
                resume: self.resume.as_deref(),
                connection_id: &self.connection.id,
                model: self.model.as_deref().or(self.connection.model.as_deref()),
                effort: self.effort.as_deref().or(self.connection.effort.as_deref()),
                control: &self.control,
            },
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
    actions.push("close".into());
    ClientConnectionInfo {
        id: connection.id.clone(),
        tool: connection.tool.clone(),
        project: connection.project.clone(),
        role: "main".into(),
        busy: state.as_ref().is_some_and(|state| state.busy),
        actions,
        session_id: state.as_ref().and_then(|state| state.session_id.clone()),
        last_error: state.and_then(|state| state.last_error.clone()),
        model: connection.model.clone(),
        effort: connection.effort.clone(),
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
    #[error("client session persistence: {0}")]
    Persistence(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubBackend {
        supports_interrupt: bool,
    }

    struct StubDrive {
        tool: String,
        supports_interrupt: bool,
    }

    impl ClientDriveBackend for StubBackend {
        fn prepare(&self, request: PrepareDrive) -> Result<Arc<dyn PreparedClientDrive>, String> {
            Ok(Arc::new(StubDrive {
                tool: request.tool,
                supports_interrupt: self.supports_interrupt,
            }))
        }
    }

    impl PreparedClientDrive for StubDrive {
        fn tool(&self) -> &str {
            &self.tool
        }

        fn supports_interrupt(&self) -> bool {
            self.supports_interrupt
        }

        fn run_turn(
            &self,
            _: ClientTurnRequest<'_>,
            _: &mut dyn FnMut(&TurnEvent),
        ) -> Result<TurnDone, String> {
            unreachable!("manager tests do not execute the prepared drive")
        }
    }

    fn manager(supports_interrupt: bool) -> ClientDriveManager {
        ClientDriveManager::new(Arc::new(StubBackend { supports_interrupt }))
    }

    fn prepare(project: &str) -> PrepareDrive {
        PrepareDrive {
            store_path: PathBuf::from("/store"),
            project_path: PathBuf::from(project),
            tool: "fake".into(),
            env: Vec::new(),
            permission_bridge: Arc::new(SharedPermissionBridge::default()),
            persistent_home: None,
            model: None,
            effort: None,
        }
    }

    #[test]
    fn close_is_scoped_idempotent_and_releases_an_interrupted_turn() {
        let manager = manager(true);
        manager
            .create_or_attach(prepare("/project-a"), Some("connection-1".into()), None)
            .unwrap();
        let job = manager
            .begin_turn("connection-1", None, None, None)
            .unwrap();

        assert!(!manager.close("/project-b", "connection-1").unwrap());
        assert!(manager.get("connection-1").unwrap().busy);
        assert!(manager.close("/project-a", "connection-1").unwrap());
        assert!(job.control.interrupt_requested());
        assert!(matches!(
            manager.get("connection-1"),
            Err(ClientDriveError::NotFound(_))
        ));
        assert!(!manager.close("/project-a", "connection-1").unwrap());

        manager.finish_turn(
            &job,
            &Ok(TurnDone {
                reason: hotsheet_aitools::DoneReason::Interrupted,
                session_id: None,
            }),
        );
        assert!(manager.active_sessions.lock().unwrap().is_empty());
    }

    #[test]
    fn close_keeps_a_busy_non_interruptible_connection_attached() {
        let manager = manager(false);
        manager
            .create_or_attach(prepare("/project"), Some("connection-1".into()), None)
            .unwrap();
        let job = manager
            .begin_turn("connection-1", None, None, None)
            .unwrap();

        assert!(matches!(
            manager.close("/project", "connection-1"),
            Err(ClientDriveError::Unsupported(_))
        ));
        assert!(manager.get("connection-1").unwrap().busy);

        manager.finish_turn(
            &job,
            &Ok(TurnDone {
                reason: hotsheet_aitools::DoneReason::Completed,
                session_id: None,
            }),
        );
    }
}
