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
    pub source_id: String,
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
    pub source: String,
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
    /// Drives open when the catalog was last written. Drives live only in this server's memory,
    /// so any left here at startup ended with the previous run (HS2-VFXEF4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    live_drives: Vec<LiveDrive>,
}

/// A drive whose session may hold ticket claims, persisted so a restart can release them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveDrive {
    pub connection_id: String,
    /// The session worker id its tool claims with.
    pub worker_id: String,
    /// The ticket store the drive works in.
    pub store_path: PathBuf,
}

struct ClientConnection {
    id: String,
    tool: String,
    project: String,
    source: String,
    store_path: PathBuf,
    home_id: String,
    /// The session worker id the drive's tool claims with (HS2-RXWXQ8).
    worker_id: String,
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
    /// Drives the previous server run left open, taken once at startup.
    orphaned_drives: Arc<Mutex<Vec<LiveDrive>>>,
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
            orphaned_drives: Arc::default(),
        }
    }

    pub fn with_persistence(
        backend: Arc<dyn ClientDriveBackend>,
        session_path: PathBuf,
        home_root: PathBuf,
    ) -> Result<Self, ClientDriveError> {
        let mut sessions: SessionCatalog = if session_path.exists() {
            let text = std::fs::read_to_string(&session_path)
                .map_err(|error| ClientDriveError::Persistence(error.to_string()))?;
            serde_json::from_str(&text)
                .map_err(|error| ClientDriveError::Persistence(error.to_string()))?
        } else {
            SessionCatalog::default()
        };
        let orphaned = std::mem::take(&mut sessions.live_drives);
        Ok(Self {
            backend,
            connections: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(sessions)),
            session_path: Some(session_path),
            home_root: Some(home_root),
            active_sessions: Arc::new(Mutex::new(HashSet::new())),
            orphaned_drives: Arc::new(Mutex::new(orphaned)),
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
                || existing.source != request.source_id
            {
                return Err(ClientDriveError::Conflict(format!(
                    "connection '{id}' belongs to another project, source, or tool"
                )));
            }
            return Ok(connection_info(&existing));
        }

        // The drive's session worker id: its claims are released when the drive closes.
        let worker_id = hotsheet_aitools::session_worker_id(&request.tool, &id);
        request
            .env
            .push(format!("{}={worker_id}", hotsheet_aitools::WORKER_ID_ENV));
        let model = request.model.clone();
        let effort = request.effort.clone();
        let source = request.source_id.clone();
        let store_path = request.store_path.clone();
        let drive = self
            .backend
            .prepare(request)
            .map_err(ClientDriveError::Prepare)?;
        let tool = drive.tool().to_owned();
        let connection = Arc::new(ClientConnection {
            id: id.clone(),
            tool,
            project,
            source,
            store_path,
            home_id,
            worker_id,
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
        // Best effort: without the ledger entry a restart falls back to lease expiry.
        if let Err(error) = self.update_catalog(|catalog| {
            catalog
                .live_drives
                .retain(|drive| drive.worker_id != connection.worker_id);
            catalog.live_drives.push(LiveDrive {
                connection_id: connection.id.clone(),
                worker_id: connection.worker_id.clone(),
                store_path: connection.store_path.clone(),
            });
        }) {
            eprintln!("recording drive {}: {error}", connection.id);
        }
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

    /// Record a finished turn. Returns the drive's worker id when the drive was closed while
    /// the turn ran: its session ended with this turn, so the caller releases its claims.
    pub fn finish_turn(
        &self,
        job: &ClientTurnJob,
        result: &Result<TurnDone, String>,
    ) -> Option<String> {
        let connection = &job.connection;
        let Ok(mut active_sessions) = self.active_sessions.lock() else {
            return None;
        };
        let Ok(mut state) = connection.state.lock() else {
            return None;
        };
        let ended = state.closing.then(|| connection.worker_id.clone());
        if let Some(worker) = &ended {
            self.forget_live_drive(worker);
        }
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
        ended
    }

    /// Close one client-owned drive without exposing whether the id belongs to another
    /// checkout. A busy drive is removed only after its interrupt signal is accepted; the
    /// running job retains its `Arc` so `finish_turn` can still release session ownership.
    pub fn close(&self, project: &str, id: &str) -> Result<DriveClosed, ClientDriveError> {
        let mut connections = self
            .connections
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?;
        let Some(connection) = connections.get(id).cloned() else {
            return Ok(DriveClosed::NotFound);
        };
        if connection.project != project {
            return Ok(DriveClosed::NotFound);
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
        let closed = if state.busy {
            DriveClosed::Ending
        } else {
            DriveClosed::Ended {
                worker_id: connection.worker_id.clone(),
            }
        };
        connections.remove(id);
        drop(state);
        drop(connections);
        if matches!(closed, DriveClosed::Ended { .. }) {
            self.forget_live_drive(&connection.worker_id);
        }
        Ok(closed)
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
        self.update_catalog(|next| {
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
        })
    }

    /// Drop an ended drive from the persisted live-drive ledger.
    fn forget_live_drive(&self, worker_id: &str) {
        if let Err(error) = self.update_catalog(|catalog| {
            catalog
                .live_drives
                .retain(|drive| drive.worker_id != worker_id);
        }) {
            eprintln!("forgetting drive {worker_id}: {error}");
        }
    }

    /// The drives the previous server run left open (their tool sessions ended with it), taken
    /// once so their claims can be released (HS2-VFXEF4).
    pub fn take_orphaned_drives(&self) -> Vec<LiveDrive> {
        self.orphaned_drives
            .lock()
            .map(|mut drives| std::mem::take(&mut *drives))
            .unwrap_or_default()
    }

    /// Apply `change` to the session catalog and persist it atomically when persistence is on.
    fn update_catalog(
        &self,
        change: impl FnOnce(&mut SessionCatalog),
    ) -> Result<(), ClientDriveError> {
        let mut catalog = self
            .sessions
            .lock()
            .map_err(|_| ClientDriveError::Unavailable)?;
        let mut next = catalog.clone();
        change(&mut next);
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

/// What closing a drive did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveClosed {
    /// No drive with that id belongs to the project.
    NotFound,
    /// The idle drive's session ended now; release its worker's claims.
    Ended { worker_id: String },
    /// The running turn was asked to stop; its session ends when [`ClientDriveManager::finish_turn`]
    /// returns the worker id.
    Ending,
}

pub struct ClientTurnJob {
    connection: Arc<ClientConnection>,
    control: TurnControl,
    resume: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

impl ClientTurnJob {
    pub fn store_path(&self) -> &std::path::Path {
        &self.connection.store_path
    }

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
        source: connection.source.clone(),
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
        envs: Mutex<Vec<Vec<String>>>,
    }

    struct StubDrive {
        tool: String,
        supports_interrupt: bool,
    }

    impl ClientDriveBackend for StubBackend {
        fn prepare(&self, request: PrepareDrive) -> Result<Arc<dyn PreparedClientDrive>, String> {
            self.envs.lock().unwrap().push(request.env.clone());
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
        stub_manager(supports_interrupt).0
    }

    fn stub_manager(supports_interrupt: bool) -> (ClientDriveManager, Arc<StubBackend>) {
        let backend = Arc::new(StubBackend {
            supports_interrupt,
            envs: Mutex::default(),
        });
        (ClientDriveManager::new(backend.clone()), backend)
    }

    fn done(reason: hotsheet_aitools::DoneReason) -> Result<TurnDone, String> {
        Ok(TurnDone {
            reason,
            session_id: None,
        })
    }

    fn prepare(project: &str) -> PrepareDrive {
        PrepareDrive {
            store_path: PathBuf::from("/store"),
            source_id: "git-store".into(),
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

        assert_eq!(
            manager.close("/project-b", "connection-1").unwrap(),
            DriveClosed::NotFound
        );
        assert!(manager.get("connection-1").unwrap().busy);
        assert_eq!(
            manager.close("/project-a", "connection-1").unwrap(),
            DriveClosed::Ending,
            "a busy drive's session ends when its interrupted turn finishes"
        );
        assert!(job.control.interrupt_requested());
        assert!(matches!(
            manager.get("connection-1"),
            Err(ClientDriveError::NotFound(_))
        ));
        assert_eq!(
            manager.close("/project-a", "connection-1").unwrap(),
            DriveClosed::NotFound
        );

        assert_eq!(
            manager.finish_turn(&job, &done(hotsheet_aitools::DoneReason::Interrupted)),
            Some("fake-connection-1".into()),
            "the interrupted turn ends the closed drive's session"
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

        assert_eq!(
            manager.finish_turn(&job, &done(hotsheet_aitools::DoneReason::Completed)),
            None,
            "a turn on an open drive does not end its session"
        );
    }

    /// Drives open when a server stops are reported once by the next run, so their claims can
    /// be released; drives that ended cleanly are not (HS2-VFXEF4).
    #[test]
    fn drives_left_open_by_a_previous_run_are_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("sessions.json");
        let backend = || {
            Arc::new(StubBackend {
                supports_interrupt: true,
                envs: Mutex::default(),
            })
        };
        let first = ClientDriveManager::with_persistence(
            backend(),
            catalog.clone(),
            dir.path().join("homes"),
        )
        .unwrap();
        for id in ["open", "closed", "interrupted"] {
            first
                .create_or_attach(prepare("/project"), Some(id.into()), None)
                .unwrap();
        }
        assert!(matches!(
            first.close("/project", "closed").unwrap(),
            DriveClosed::Ended { .. }
        ));
        let job = first.begin_turn("interrupted", None, None, None).unwrap();
        assert_eq!(
            first.close("/project", "interrupted").unwrap(),
            DriveClosed::Ending
        );
        assert!(
            first
                .finish_turn(&job, &done(hotsheet_aitools::DoneReason::Interrupted))
                .is_some()
        );
        assert!(
            first.take_orphaned_drives().is_empty(),
            "a fresh catalog has no orphans"
        );

        // "Restart": only the drive still open is orphaned, reported once.
        let second = ClientDriveManager::with_persistence(
            backend(),
            catalog.clone(),
            dir.path().join("homes"),
        )
        .unwrap();
        assert_eq!(
            second.take_orphaned_drives(),
            [LiveDrive {
                connection_id: "open".into(),
                worker_id: "fake-open".into(),
                store_path: PathBuf::from("/store"),
            }]
        );
        assert!(second.take_orphaned_drives().is_empty());

        // The next write drops the stale ledger; a new drive is recorded on its own.
        second
            .create_or_attach(prepare("/project"), Some("next".into()), None)
            .unwrap();
        let third =
            ClientDriveManager::with_persistence(backend(), catalog, dir.path().join("homes"))
                .unwrap();
        let orphaned: Vec<String> = third
            .take_orphaned_drives()
            .into_iter()
            .map(|drive| drive.connection_id)
            .collect();
        assert_eq!(orphaned, ["next"]);
    }

    /// A drive's tool gets a session worker id, and only closing the drive ends the session
    /// whose claims are released (HS2-RXWXQ8).
    #[test]
    fn a_drive_session_worker_id_is_released_only_when_the_drive_closes() {
        let (manager, backend) = stub_manager(true);
        manager
            .create_or_attach(prepare("/project"), Some("connection-1".into()), None)
            .unwrap();
        assert_eq!(
            backend.envs.lock().unwrap().as_slice(),
            [vec!["HOTSHEET_WORKER_ID=fake-connection-1".to_string()]]
        );
        // Attaching again reuses the drive, and its worker id, without preparing again.
        manager
            .create_or_attach(prepare("/project"), Some("connection-1".into()), None)
            .unwrap();
        assert_eq!(backend.envs.lock().unwrap().len(), 1);

        // Turns on an open drive keep the session (and its claims) alive.
        for _ in 0..2 {
            let job = manager
                .begin_turn("connection-1", None, None, None)
                .unwrap();
            assert_eq!(
                manager.finish_turn(&job, &done(hotsheet_aitools::DoneReason::Completed)),
                None
            );
        }
        assert_eq!(
            manager.close("/project", "connection-1").unwrap(),
            DriveClosed::Ended {
                worker_id: "fake-connection-1".into()
            }
        );
        assert_eq!(
            manager.close("/project", "connection-1").unwrap(),
            DriveClosed::NotFound
        );
    }
}
