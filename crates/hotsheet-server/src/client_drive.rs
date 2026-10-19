//! Client-owned AI connections: prepare once, send sequential turns, and expose an
//! interrupt action only when the resolved drive implements it (HS2-5DGFG2).

use hotsheet_sync::LockExt;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hotsheet_aitools::tool_progress::ToolTransition;
use hotsheet_aitools::{
    ConnectionRegistry, SafeTrigger, SharedPermissionBridge, TriggerOptions, TurnControl, TurnDone,
    TurnEvent, TurnRequest, prepare_trigger_with_home,
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
    /// The validated permission mode to launch with (HS2-NHRDMW); `None` = the plugin default.
    pub permission_mode: Option<String>,
}

pub struct ClientTurnRequest<'a> {
    pub prompt: &'a str,
    pub resume: Option<&'a str>,
    /// The resumed session's recorded cumulative cost, so the turn reports only its own
    /// cost (HS2-4JPDJA).
    pub session_cost_baseline_usd: Option<f64>,
    pub connection_id: &'a str,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    /// The chat's active permission mode for this turn (HS2-NHRDMW).
    pub permission_mode: Option<&'a str>,
    pub control: &'a TurnControl,
}

/// Prepared drive boundary. Tests inject a fake here; production delegates to SafeTrigger,
/// retaining its isolated home across turns.
pub trait PreparedClientDrive: Send + Sync {
    fn tool(&self) -> &str;
    fn supports_interrupt(&self) -> bool;
    /// The permission mode the drive was prepared with, when it has one (HS2-NHRDMW).
    fn permission_mode(&self) -> Option<&str> {
        None
    }
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
            TriggerOptions {
                project: Some(request.project_path),
                envs: request.env,
                // The shared daemon uses a Unix-domain control socket. Windows uses the
                // direct app-server transport while preserving the same client API/session.
                shared_daemon: cfg!(unix),
                persistent_codex_home: request.persistent_home,
                permission_mode: request.permission_mode,
                ..TriggerOptions::default()
            },
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

    fn permission_mode(&self) -> Option<&str> {
        Some(self.0.permission_mode())
    }

    fn run_turn(
        &self,
        request: ClientTurnRequest<'_>,
        on_event: &mut dyn FnMut(&TurnEvent),
    ) -> Result<TurnDone, String> {
        let mut registry = ConnectionRegistry::new(30_000);
        self.0
            .run_turn_controlled_with_options(
                TurnRequest {
                    prompt: request.prompt,
                    resume: request.resume,
                    session_cost_baseline_usd: request.session_cost_baseline_usd,
                    model: request.model,
                    effort: request.effort,
                    permission_mode: request.permission_mode,
                    worker: false,
                    conn_id: request.connection_id.to_owned(),
                },
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
    /// The chat's active permission mode (HS2-NHRDMW): the launch mode until a turn selects
    /// another, which then sticks for later turns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// The running turn's correlation id while `busy` (HS2-14X4AF); the same id every
    /// `turn_event` of that turn carries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// When the running turn began, in Unix milliseconds, while `busy` (HS2-XV1VCR).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_started_at_ms: Option<u64>,
    /// When the running turn last produced a stream event (its start until the first one),
    /// while `busy`. A client derives "last activity" and its stuck state from this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_at_ms: Option<u64>,
    /// The longest-running tool call still open in the running turn, while `busy`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_flight_tool: Option<InFlightTool>,
}

/// The tool call a running turn is waiting on (HS2-XV1VCR): the oldest still-open call, and
/// how many calls are open in all (parallel tool use or sub-agents).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InFlightTool {
    pub name: String,
    pub started_at_ms: u64,
    pub count: usize,
}

/// One open tool call of a running turn.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenTool {
    id: Option<String>,
    name: String,
    started_at_ms: u64,
}

/// A long or misbehaving turn cannot grow the open-call list without bound; the oldest calls
/// are what the indicator shows, so the newest are the ones dropped past this cap.
const MAX_OPEN_TOOLS: usize = 64;

/// Timing of a running turn (HS2-XV1VCR), updated from every stream event it produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TurnTiming {
    started_at_ms: u64,
    last_event_at_ms: u64,
    open_tools: Vec<OpenTool>,
}

impl TurnTiming {
    fn started(at_ms: u64) -> Self {
        Self {
            started_at_ms: at_ms,
            last_event_at_ms: at_ms,
            open_tools: Vec::new(),
        }
    }

    /// Record one stream event observed at `at_ms`.
    fn observe(&mut self, event: &TurnEvent, at_ms: u64) {
        self.last_event_at_ms = self.last_event_at_ms.max(at_ms);
        match hotsheet_aitools::tool_progress::tool_transition(event) {
            Some(ToolTransition::Started { id, name }) => {
                let duplicate = id.is_some()
                    && self
                        .open_tools
                        .iter()
                        .any(|open| open.id.as_ref() == id.as_ref());
                if !duplicate && self.open_tools.len() < MAX_OPEN_TOOLS {
                    self.open_tools.push(OpenTool {
                        id,
                        name,
                        started_at_ms: at_ms,
                    });
                }
            }
            Some(ToolTransition::Finished { id, name }) => {
                // Prefer the exact call. A finish whose id matches nothing (or that has none)
                // closes the oldest open call of that name; one naming neither closes nothing.
                let by_id = id.as_ref().and_then(|id| {
                    self.open_tools
                        .iter()
                        .position(|open| open.id.as_ref() == Some(id))
                });
                let position = by_id.or_else(|| {
                    let name = name.as_ref()?;
                    self.open_tools.iter().position(|open| &open.name == name)
                });
                if let Some(position) = position {
                    self.open_tools.remove(position);
                }
            }
            None => {}
        }
        if matches!(event, TurnEvent::Done(_)) {
            self.open_tools.clear();
        }
    }

    fn in_flight_tool(&self) -> Option<InFlightTool> {
        let oldest = self.open_tools.first()?;
        Some(InFlightTool {
            name: oldest.name.clone(),
            started_at_ms: oldest.started_at_ms,
            count: self.open_tools.len(),
        })
    }
}

/// What one observed event changed in its turn's timing, stamped on its stream envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnEventTiming {
    pub at_ms: u64,
    pub in_flight_tool: Option<InFlightTool>,
}

struct ConnectionState {
    busy: bool,
    turn_id: Option<String>,
    /// The running turn's timing while `busy` (HS2-XV1VCR).
    timing: Option<TurnTiming>,
    closing: bool,
    session_id: Option<String>,
    control: Option<TurnControl>,
    last_error: Option<String>,
    active_session_key: Option<String>,
    /// The active permission mode, per chat (HS2-NHRDMW). It is persisted with the chat's
    /// session catalog entry so a restarted server resumes the chat in it (HS2-Z5QVXW).
    permission_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientSessionInfo {
    pub connection_id: String,
    pub tool: String,
    pub project: String,
    pub session_id: String,
    pub updated_at_ms: u64,
    /// The chat's last permission mode (HS2-Z5QVXW), restored when the session resumes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct StoredClientSession {
    connection_id: String,
    tool: String,
    project: String,
    session_id: String,
    updated_at_ms: u64,
    #[serde(default)]
    home_id: String,
    /// The chat's permission mode when this session was last recorded or switched
    /// (HS2-Z5QVXW). Catalogs written before it read as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    permission_mode: Option<String>,
    /// The session's cumulative cost after its last turn. Claude restores its
    /// `total_cost_usd` from the session on `--resume`, so the next turn subtracts this to
    /// report only its own cost (HS2-4JPDJA).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_cost_usd: Option<f64>,
}

impl From<&StoredClientSession> for ClientSessionInfo {
    fn from(session: &StoredClientSession) -> Self {
        Self {
            connection_id: session.connection_id.clone(),
            tool: session.tool.clone(),
            project: session.project.clone(),
            session_id: session.session_id.clone(),
            updated_at_ms: session.updated_at_ms,
            permission_mode: session.permission_mode.clone(),
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
        // The drive is an AI session: its CLI/MCP mutations act as `ai` (HS2-RD4M29).
        for (key, value) in hotsheet_aitools::ai_session_actor_env(&worker_id) {
            request.env.push(format!("{key}={value}"));
        }
        let model = request.model.clone();
        let effort = request.effort.clone();
        let requested_mode = request.permission_mode.clone();
        let source = request.source_id.clone();
        let store_path = request.store_path.clone();
        let drive = self
            .backend
            .prepare(request)
            .map_err(ClientDriveError::Prepare)?;
        let tool = drive.tool().to_owned();
        let permission_mode = requested_mode.or_else(|| drive.permission_mode().map(str::to_owned));
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
                turn_id: None,
                timing: None,
                busy: false,
                closing: false,
                session_id: session_id.clone(),
                control: None,
                last_error: None,
                active_session_key: None,
                permission_mode,
            }),
        });
        self.connections
            .lock_or_recover()
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
            tracing::warn!(drive = %connection.id, %error, "recording drive failed");
        }
        if let Some(session_id) = session_id {
            if let Err(error) = self.record_session(&connection, session_id, None) {
                {
                    let mut connections = self.connections.lock_or_recover();
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
        self.begin_correlated_turn(id, explicit_session, model, effort, None)
    }

    /// Switch an idle chat's permission mode (HS2-NHRDMW). The mode sticks for later turns and
    /// is never applied to a running turn. A chat with a recorded session persists the switch
    /// right away (HS2-Z5QVXW); a new chat persists it with its first recorded session.
    pub fn set_permission_mode(
        &self,
        id: &str,
        mode: String,
    ) -> Result<ClientConnectionInfo, ClientDriveError> {
        let connection = self
            .connection(id)?
            .ok_or_else(|| ClientDriveError::NotFound(id.into()))?;
        let session_id = {
            let mut state = connection.state.lock_or_recover();
            if state.closing {
                return Err(ClientDriveError::NotFound(id.into()));
            }
            if state.busy {
                return Err(ClientDriveError::Conflict(format!(
                    "connection '{id}' already has a running turn"
                )));
            }
            state.permission_mode = Some(mode.clone());
            state.session_id.clone()
        };
        if let Some(session_id) = session_id
            && let Err(error) = self.update_catalog(|catalog| {
                for session in &mut catalog.sessions {
                    if session.project == connection.project
                        && session.tool == connection.tool
                        && session.session_id == session_id
                    {
                        session.permission_mode = Some(mode.clone());
                    }
                }
            })
        {
            // The switch still applies for this run; the next recorded turn retries persisting.
            tracing::warn!(connection = %id, %error, "persisting the permission mode failed");
        }
        Ok(connection_info(&connection))
    }

    /// The permission mode last recorded for a resumable session (HS2-Z5QVXW), so a drive
    /// recreated after a server restart resumes the chat in the mode the user picked.
    pub fn session_permission_mode(
        &self,
        project: &str,
        tool: &str,
        session_id: &str,
    ) -> Option<String> {
        self.stored_session(project, tool, session_id)
            .and_then(|session| session.permission_mode)
    }

    /// [`Self::begin_turn`] with the caller's turn correlation id (HS2-14X4AF). A client that
    /// names its turn before sending can match stream events that arrive before the send
    /// response; without one the server assigns a fresh ULID.
    pub fn begin_correlated_turn(
        &self,
        id: &str,
        explicit_session: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        turn_id: Option<String>,
    ) -> Result<ClientTurnJob, ClientDriveError> {
        let turn_id = match turn_id {
            Some(turn_id) if valid_turn_id(&turn_id) => turn_id,
            Some(_) => {
                return Err(ClientDriveError::Prepare(
                    "turn_id must be 1-128 ASCII letters, digits, '-', '_', '.' or ':'".into(),
                ));
            }
            None => hotsheet_model::Ulid::new().to_string(),
        };
        let connection = self
            .connection(id)?
            .ok_or_else(|| ClientDriveError::NotFound(id.into()))?;
        let mut active_sessions = self.active_sessions.lock_or_recover();
        let mut state = connection.state.lock_or_recover();
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
        let permission_mode = state.permission_mode.clone();
        state.busy = true;
        state.turn_id = Some(turn_id.clone());
        state.timing = Some(TurnTiming::started(now_ms()));
        state.last_error = None;
        state.control = Some(control.clone());
        state.active_session_key = Some(active_key);
        drop(state);
        drop(active_sessions);
        let session_cost_baseline_usd = resume.as_deref().and_then(|session_id| {
            self.stored_session(&connection.project, &connection.tool, session_id)
                .and_then(|session| session.session_cost_usd)
        });
        Ok(ClientTurnJob {
            connection,
            control,
            turn_id,
            resume,
            session_cost_baseline_usd,
            model,
            effort,
            permission_mode,
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
        let mut active_sessions = self.active_sessions.lock_or_recover();
        let mut state = connection.state.lock_or_recover();
        let ended = state.closing.then(|| connection.worker_id.clone());
        if let Some(worker) = &ended {
            self.forget_live_drive(worker);
        }
        state.busy = false;
        state.turn_id = None;
        state.timing = None;
        state.control = None;
        if let Some(key) = state.active_session_key.take() {
            active_sessions.remove(&key);
        }
        match result {
            Ok(done) => {
                state.last_error = done.reason.failure_message();
                if done.session_id.is_some() {
                    state.session_id.clone_from(&done.session_id);
                }
            }
            Err(error) => state.last_error = Some(error.clone()),
        }
        let session_id = state.session_id.clone();
        drop(state);
        let session_cost_usd = result.as_ref().ok().and_then(|done| done.session_cost_usd);
        if let Some(session_id) = session_id {
            if let Err(error) = self.record_session(connection, session_id, session_cost_usd) {
                connection.state.lock_or_recover().last_error = Some(error.to_string());
            }
        }
        ended
    }

    /// Close one client-owned drive without exposing whether the id belongs to another
    /// checkout. A busy drive is removed only after its interrupt signal is accepted; the
    /// running job retains its `Arc` so `finish_turn` can still release session ownership.
    pub fn close(&self, project: &str, id: &str) -> Result<DriveClosed, ClientDriveError> {
        let mut connections = self.connections.lock_or_recover();
        let Some(connection) = connections.get(id).cloned() else {
            return Ok(DriveClosed::NotFound);
        };
        if connection.project != project {
            return Ok(DriveClosed::NotFound);
        }
        let mut state = connection.state.lock_or_recover();
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
        let state = connection.state.lock_or_recover();
        let control = state.control.as_ref().ok_or_else(|| {
            ClientDriveError::Conflict(format!("connection '{id}' has no running turn"))
        })?;
        control.request_interrupt();
        Ok(())
    }

    pub fn list(&self) -> Vec<ClientConnectionInfo> {
        let connections = self.connections.lock_or_recover();
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
        let catalog = self.sessions.lock_or_recover();
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
        session_cost_usd: Option<f64>,
    ) -> Result<(), ClientDriveError> {
        let permission_mode = connection.state.lock_or_recover().permission_mode.clone();
        self.update_catalog(|next| {
            let mut previous_cost = None;
            next.sessions.retain(|session| {
                let same = session.project == connection.project
                    && session.tool == connection.tool
                    && session.session_id == session_id;
                if same {
                    previous_cost = session.session_cost_usd;
                }
                !same
            });
            // A turn that reported no cost keeps the last recorded total.
            let session_cost_usd = session_cost_usd.or(previous_cost);
            next.sessions.push(StoredClientSession {
                connection_id: connection.id.clone(),
                tool: connection.tool.clone(),
                project: connection.project.clone(),
                session_id,
                updated_at_ms: now_ms(),
                home_id: connection.home_id.clone(),
                permission_mode,
                session_cost_usd,
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
            tracing::warn!(worker = %worker_id, %error, "forgetting drive failed");
        }
    }

    /// The drives the previous server run left open (their tool sessions ended with it), taken
    /// once so their claims can be released (HS2-VFXEF4).
    pub fn take_orphaned_drives(&self) -> Vec<LiveDrive> {
        self.orphaned_drives
            .with_lock(|drives| std::mem::take(&mut *drives))
    }

    /// Apply `change` to the session catalog and persist it atomically when persistence is on.
    fn update_catalog(
        &self,
        change: impl FnOnce(&mut SessionCatalog),
    ) -> Result<(), ClientDriveError> {
        let mut catalog = self.sessions.lock_or_recover();
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
        Ok(self
            .connections
            .with_lock(|connections| connections.get(id).cloned()))
    }

    fn stored_session(
        &self,
        project: &str,
        tool: &str,
        session_id: &str,
    ) -> Option<StoredClientSession> {
        self.sessions
            .lock_or_recover()
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

pub(crate) fn now_ms() -> u64 {
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
    turn_id: String,
    resume: Option<String>,
    /// The resumed session's recorded cumulative cost (HS2-4JPDJA).
    session_cost_baseline_usd: Option<f64>,
    model: Option<String>,
    effort: Option<String>,
    permission_mode: Option<String>,
}

impl ClientTurnJob {
    pub fn store_path(&self) -> &std::path::Path {
        &self.connection.store_path
    }

    pub fn worker_id(&self) -> &str {
        &self.connection.worker_id
    }

    pub fn selected_model(&self) -> Option<&str> {
        self.model.as_deref().or(self.connection.model.as_deref())
    }

    /// The correlation id every stream event of this turn carries (HS2-14X4AF).
    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    /// Record one of this turn's stream events now and return the timing its envelope
    /// carries (HS2-XV1VCR).
    pub fn observe_event(&self, event: &TurnEvent) -> TurnEventTiming {
        self.observe_event_at(event, now_ms())
    }

    fn observe_event_at(&self, event: &TurnEvent, at_ms: u64) -> TurnEventTiming {
        let mut state = self.connection.state.lock_or_recover();
        // A stale job (its turn already finished) never touches a newer turn's timing.
        let current = state.turn_id.as_deref() == Some(self.turn_id.as_str());
        let in_flight_tool = state
            .timing
            .as_mut()
            .filter(|_| current)
            .and_then(|timing| {
                timing.observe(event, at_ms);
                timing.in_flight_tool()
            });
        TurnEventTiming {
            at_ms,
            in_flight_tool,
        }
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
                session_cost_baseline_usd: self.session_cost_baseline_usd,
                connection_id: &self.connection.id,
                model: self.model.as_deref().or(self.connection.model.as_deref()),
                effort: self.effort.as_deref().or(self.connection.effort.as_deref()),
                permission_mode: self.permission_mode.as_deref(),
                control: &self.control,
            },
            on_event,
        )
    }
}

fn connection_info(connection: &ClientConnection) -> ClientConnectionInfo {
    let state = connection.state.lock_or_recover();
    let busy_timing = state.timing.as_ref().filter(|_| state.busy);
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
        busy: state.busy,
        actions,
        session_id: state.session_id.clone(),
        last_error: state.last_error.clone(),
        model: connection.model.clone(),
        effort: connection.effort.clone(),
        permission_mode: state.permission_mode.clone(),
        turn_id: state.turn_id.clone(),
        turn_started_at_ms: busy_timing.map(|timing| timing.started_at_ms),
        last_event_at_ms: busy_timing.map(|timing| timing.last_event_at_ms),
        in_flight_tool: busy_timing.and_then(TurnTiming::in_flight_tool),
    }
}

/// A client-chosen turn id is echoed on every stream event, so keep it short and inert.
fn valid_turn_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
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
            self.envs.lock_or_recover().push(request.env.clone());
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

        fn permission_mode(&self) -> Option<&str> {
            Some("plugin-default")
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
            session_cost_usd: None,
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
            permission_mode: None,
        }
    }

    fn tool_start(name: &str, id: Option<&str>) -> TurnEvent {
        let mut payload = serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": name});
        if let Some(id) = id {
            payload["tool_use_id"] = id.into();
        }
        TurnEvent::NativeActivity {
            source: "claude-hooks".into(),
            payload,
        }
    }

    fn tool_finish(name: Option<&str>, id: Option<&str>) -> TurnEvent {
        let mut payload = serde_json::json!({"hook_event_name": "PostToolUse"});
        if let Some(name) = name {
            payload["tool_name"] = name.into();
        }
        if let Some(id) = id {
            payload["tool_use_id"] = id.into();
        }
        TurnEvent::NativeActivity {
            source: "claude-hooks".into(),
            payload,
        }
    }

    fn in_flight(timing: &TurnTiming) -> Option<(String, u64, usize)> {
        timing
            .in_flight_tool()
            .map(|tool| (tool.name, tool.started_at_ms, tool.count))
    }

    /// HS2-XV1VCR transition matrix for one turn's timing: idle → started → tool open →
    /// parallel tools → out-of-order, repeated, unknown and nameless finishes → refill → done.
    #[test]
    fn turn_timing_tracks_open_tools_through_interleaved_sequences() {
        let mut timing = TurnTiming::started(1_000);
        assert_eq!(
            (timing.started_at_ms, timing.last_event_at_ms),
            (1_000, 1_000)
        );
        assert_eq!(in_flight(&timing), None);

        timing.observe(&TurnEvent::Output("thinking".into()), 1_500);
        assert_eq!(timing.last_event_at_ms, 1_500);
        assert_eq!(in_flight(&timing), None, "text is not a tool");

        timing.observe(&tool_start("Task", Some("a")), 2_000);
        timing.observe(&tool_start("Bash", Some("b")), 2_100);
        timing.observe(&tool_start("Task", Some("a")), 2_200);
        assert_eq!(
            in_flight(&timing),
            Some(("Task".into(), 2_000, 2)),
            "the oldest open call is shown; a repeated start is not a second call"
        );

        // A late event never moves last activity backwards.
        timing.observe(&TurnEvent::Output("late".into()), 1_200);
        assert_eq!(timing.last_event_at_ms, 2_200);

        timing.observe(&tool_finish(None, Some("unknown")), 2_300);
        assert_eq!(
            in_flight(&timing).map(|tool| tool.2),
            Some(2),
            "unknown id closes nothing"
        );
        timing.observe(&tool_finish(None, None), 2_350);
        assert_eq!(
            in_flight(&timing).map(|tool| tool.2),
            Some(2),
            "nameless, idless closes nothing"
        );

        timing.observe(&tool_finish(None, Some("a")), 2_400);
        assert_eq!(in_flight(&timing), Some(("Bash".into(), 2_100, 1)));
        timing.observe(&tool_finish(None, Some("a")), 2_450);
        assert_eq!(
            in_flight(&timing),
            Some(("Bash".into(), 2_100, 1)),
            "repeated finish"
        );

        // Without ids a finish closes the oldest call of the same name.
        timing.observe(&tool_start("Read", None), 2_500);
        timing.observe(&tool_finish(Some("Read"), Some("other")), 2_600);
        assert_eq!(in_flight(&timing), Some(("Bash".into(), 2_100, 1)));

        timing.observe(&tool_finish(Some("Bash"), Some("b")), 2_700);
        assert_eq!(in_flight(&timing), None, "empty");
        timing.observe(&tool_start("Grep", Some("c")), 2_800);
        assert_eq!(
            in_flight(&timing),
            Some(("Grep".into(), 2_800, 1)),
            "refill"
        );

        timing.observe(
            &TurnEvent::Done(hotsheet_aitools::DoneReason::Completed),
            2_900,
        );
        assert_eq!(in_flight(&timing), None, "done closes every open call");
        assert_eq!(timing.last_event_at_ms, 2_900);
    }

    /// HS2-90FMR2: Codex items open on `item/started` and close on their `item/completed`,
    /// matched by item id even when another call of the same kind is open.
    #[test]
    fn turn_timing_tracks_codex_items_by_id() {
        let codex = |source: &str, id: &str, kind: &str| TurnEvent::NativeActivity {
            source: source.into(),
            payload: serde_json::json!({"type": kind, "id": id, "tool": "hotsheet_get"}),
        };
        let mut timing = TurnTiming::started(0);
        timing.observe(&codex("codex-item-started", "a", "commandExecution"), 10);
        timing.observe(&codex("codex-item-started", "b", "commandExecution"), 20);
        timing.observe(&codex("codex-item-started", "c", "mcpToolCall"), 30);
        timing.observe(&codex("codex-item-started", "r", "reasoning"), 35);
        assert_eq!(in_flight(&timing), Some(("Shell".into(), 10, 3)));
        timing.observe(&codex("codex-transcript", "b", "commandExecution"), 40);
        assert_eq!(in_flight(&timing), Some(("Shell".into(), 10, 2)));
        timing.observe(&codex("codex-transcript", "a", "commandExecution"), 50);
        assert_eq!(in_flight(&timing), Some(("hotsheet_get".into(), 30, 1)));
        // A completed item that never started (an older Codex) closes nothing else.
        timing.observe(&codex("codex-transcript", "z", "webSearch"), 60);
        assert_eq!(in_flight(&timing), Some(("hotsheet_get".into(), 30, 1)));
        timing.observe(&codex("codex-transcript", "c", "mcpToolCall"), 70);
        assert_eq!(in_flight(&timing), None);
    }

    #[test]
    fn turn_timing_bounds_its_open_tool_list() {
        let mut timing = TurnTiming::started(0);
        for index in 0..(MAX_OPEN_TOOLS + 10) {
            timing.observe(&tool_start("Bash", Some(&index.to_string())), index as u64);
        }
        assert_eq!(in_flight(&timing), Some(("Bash".into(), 0, MAX_OPEN_TOOLS)));
    }

    /// The manager exposes timing only while its turn runs, and a stale job from a finished
    /// turn cannot touch the next turn's timing (a reconnect mid-turn reads the same fields).
    #[test]
    fn connection_info_reports_turn_timing_only_while_busy() {
        let manager = manager(false);
        manager
            .create_or_attach(prepare("/project"), Some("timed".into()), None)
            .unwrap();
        let idle = manager.get("timed").unwrap();
        assert_eq!(
            (
                idle.turn_started_at_ms,
                idle.last_event_at_ms,
                idle.in_flight_tool
            ),
            (None, None, None)
        );

        let first = manager.begin_turn("timed", None, None, None).unwrap();
        let started = manager.get("timed").unwrap();
        let started_at = started.turn_started_at_ms.unwrap();
        assert_eq!(started.last_event_at_ms, Some(started_at));
        let stamped = first.observe_event_at(&tool_start("Bash", Some("b")), started_at + 5);
        assert_eq!(stamped.at_ms, started_at + 5);
        assert_eq!(stamped.in_flight_tool.as_ref().unwrap().name, "Bash");
        let busy = manager.get("timed").unwrap();
        assert_eq!(busy.last_event_at_ms, Some(started_at + 5));
        assert_eq!(busy.in_flight_tool, stamped.in_flight_tool);
        // Each read (a reload or a second client) sees the same in-flight state.
        assert_eq!(manager.list()[0].in_flight_tool, busy.in_flight_tool);

        manager.finish_turn(&first, &done(hotsheet_aitools::DoneReason::Completed));
        let finished = manager.get("timed").unwrap();
        assert_eq!(
            (
                finished.turn_started_at_ms,
                finished.last_event_at_ms,
                finished.in_flight_tool
            ),
            (None, None, None)
        );

        let second = manager.begin_turn("timed", None, None, None).unwrap();
        let late = first.observe_event_at(&tool_start("Old", Some("x")), u64::MAX);
        assert_eq!(late.in_flight_tool, None, "a stale job reports nothing");
        let fresh = manager.get("timed").unwrap();
        assert_eq!(fresh.in_flight_tool, None);
        assert!(fresh.last_event_at_ms < Some(u64::MAX));
        manager.finish_turn(&second, &done(hotsheet_aitools::DoneReason::Completed));
    }

    /// HS2-NHRDMW transition matrix: launch mode (requested | drive default) → per-chat
    /// switch while idle → refused while busy → sticky across turns → isolated per chat.
    #[test]
    fn permission_mode_is_per_chat_sticky_and_never_changes_a_running_turn() {
        let manager = manager(false);
        manager
            .create_or_attach(prepare("/project"), Some("default".into()), None)
            .unwrap();
        let mut requested = prepare("/project");
        requested.permission_mode = Some("auto".into());
        manager
            .create_or_attach(requested, Some("requested".into()), None)
            .unwrap();
        assert_eq!(
            manager.get("default").unwrap().permission_mode.as_deref(),
            Some("plugin-default"),
            "without a request the drive's own default is reported"
        );
        assert_eq!(
            manager.get("requested").unwrap().permission_mode.as_deref(),
            Some("auto")
        );

        // A turn carries the active mode; switching while it runs is refused.
        let job = manager.begin_turn("requested", None, None, None).unwrap();
        assert_eq!(job.permission_mode.as_deref(), Some("auto"));
        assert!(matches!(
            manager.set_permission_mode("requested", "plan".into()),
            Err(ClientDriveError::Conflict(_))
        ));
        manager.finish_turn(&job, &done(hotsheet_aitools::DoneReason::Completed));

        // An idle switch sticks for every later turn, and only for this chat.
        let info = manager
            .set_permission_mode("requested", "plan".into())
            .unwrap();
        assert_eq!(info.permission_mode.as_deref(), Some("plan"));
        for _ in 0..2 {
            let job = manager.begin_turn("requested", None, None, None).unwrap();
            assert_eq!(job.permission_mode.as_deref(), Some("plan"));
            manager.finish_turn(&job, &done(hotsheet_aitools::DoneReason::Completed));
        }
        assert_eq!(
            manager.get("default").unwrap().permission_mode.as_deref(),
            Some("plugin-default")
        );
        // Re-attaching keeps the chat's switched mode rather than resetting it.
        let mut again = prepare("/project");
        again.permission_mode = Some("auto".into());
        let attached = manager
            .create_or_attach(again, Some("requested".into()), None)
            .unwrap();
        assert_eq!(attached.permission_mode.as_deref(), Some("plan"));
        assert!(matches!(
            manager.set_permission_mode("missing", "plan".into()),
            Err(ClientDriveError::NotFound(_))
        ));
    }

    /// HS2-Z5QVXW transition matrix over a persisted catalog: a new chat's switch persists
    /// with its first recorded session; a later idle switch persists immediately; a restart
    /// (a fresh manager over the same file) reports the remembered mode; a catalog written
    /// before the field reads as no remembered mode.
    #[test]
    fn a_chats_permission_mode_is_persisted_with_its_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        let backend = Arc::new(StubBackend {
            supports_interrupt: false,
            envs: Mutex::default(),
        });
        let open = || {
            ClientDriveManager::with_persistence(
                backend.clone(),
                path.clone(),
                dir.path().join("homes"),
            )
            .unwrap()
        };
        let manager = open();
        manager
            .create_or_attach(prepare("/project"), Some("chat".into()), None)
            .unwrap();
        // No session yet: the switch is in memory only.
        manager.set_permission_mode("chat", "auto".into()).unwrap();
        assert_eq!(
            manager.session_permission_mode("/project", "fake", "thread-1"),
            None
        );
        let job = manager.begin_turn("chat", None, None, None).unwrap();
        manager.finish_turn(
            &job,
            &Ok(TurnDone {
                reason: hotsheet_aitools::DoneReason::Completed,
                session_id: Some("thread-1".into()),
                session_cost_usd: None,
            }),
        );
        assert_eq!(
            manager
                .session_permission_mode("/project", "fake", "thread-1")
                .as_deref(),
            Some("auto"),
            "the first recorded session carries the chat's mode"
        );
        assert_eq!(
            manager.sessions("/project")[0].permission_mode.as_deref(),
            Some("auto")
        );
        // An idle switch on a chat with a session persists without waiting for a turn.
        manager.set_permission_mode("chat", "plan".into()).unwrap();
        drop(manager);

        let restarted = open();
        assert_eq!(
            restarted
                .session_permission_mode("/project", "fake", "thread-1")
                .as_deref(),
            Some("plan")
        );
        // Another tool or project never sees this chat's mode.
        assert_eq!(
            restarted.session_permission_mode("/project", "other", "thread-1"),
            None
        );
        assert_eq!(
            restarted.session_permission_mode("/elsewhere", "fake", "thread-1"),
            None
        );
        drop(restarted);

        // A catalog from before HS2-Z5QVXW has no mode field.
        std::fs::write(
            &path,
            r#"{"sessions":[{"connection_id":"chat","tool":"fake","project":"/project","session_id":"thread-1","updated_at_ms":1}]}"#,
        )
        .unwrap();
        let legacy = open();
        assert_eq!(
            legacy.session_permission_mode("/project", "fake", "thread-1"),
            None
        );
        assert_eq!(legacy.sessions("/project")[0].permission_mode, None);
    }

    /// HS2-4JPDJA: a resumed Claude session reports its cost cumulatively, so the catalog keeps
    /// each session's total and the next turn on it starts from that baseline — across a
    /// cost-less turn, a restart, and a reattach — while a new session starts from none.
    #[test]
    fn a_sessions_cumulative_cost_is_recorded_and_becomes_the_next_turns_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        let backend = Arc::new(StubBackend {
            supports_interrupt: false,
            envs: Mutex::default(),
        });
        let open = || {
            ClientDriveManager::with_persistence(
                backend.clone(),
                path.clone(),
                dir.path().join("homes"),
            )
            .unwrap()
        };
        let finish = |manager: &ClientDriveManager, job: &ClientTurnJob, cost: Option<f64>| {
            manager.finish_turn(
                job,
                &Ok(TurnDone {
                    reason: hotsheet_aitools::DoneReason::Completed,
                    session_id: Some("thread-1".into()),
                    session_cost_usd: cost,
                }),
            );
        };
        let manager = open();
        manager
            .create_or_attach(prepare("/project"), Some("chat".into()), None)
            .unwrap();
        let first = manager.begin_turn("chat", None, None, None).unwrap();
        assert_eq!(first.session_cost_baseline_usd, None, "a new session");
        finish(&manager, &first, Some(0.25));
        let second = manager.begin_turn("chat", None, None, None).unwrap();
        assert_eq!(second.session_cost_baseline_usd, Some(0.25));
        // A turn that reports no cost (an error) keeps the recorded total.
        finish(&manager, &second, None);
        let third = manager.begin_turn("chat", None, None, None).unwrap();
        assert_eq!(third.session_cost_baseline_usd, Some(0.25));
        finish(&manager, &third, Some(0.5));
        drop(manager);

        // After a restart the chat resumes its stored session from the same baseline.
        let restarted = open();
        restarted
            .create_or_attach(
                prepare("/project"),
                Some("chat".into()),
                Some("thread-1".into()),
            )
            .unwrap();
        let resumed = restarted.begin_turn("chat", None, None, None).unwrap();
        assert_eq!(resumed.resume.as_deref(), Some("thread-1"));
        assert_eq!(resumed.session_cost_baseline_usd, Some(0.5));
        restarted.finish_turn(&resumed, &done(hotsheet_aitools::DoneReason::Completed));
        // An explicitly chosen session that was never recorded has no baseline.
        let other = restarted
            .begin_turn("chat", Some("thread-2".into()), None, None)
            .unwrap();
        assert_eq!(other.session_cost_baseline_usd, None);
    }

    #[test]
    fn turn_ids_are_exposed_only_while_busy_and_client_ids_are_validated() {
        let manager = manager(false);
        manager
            .create_or_attach(prepare("/project"), Some("connection".into()), None)
            .unwrap();
        assert_eq!(manager.get("connection").unwrap().turn_id, None);
        for invalid in ["", "has space", "x".repeat(129).as_str(), "slash/turn"] {
            assert!(matches!(
                manager.begin_correlated_turn("connection", None, None, None, Some(invalid.into())),
                Err(ClientDriveError::Prepare(_))
            ));
            assert!(
                !manager.get("connection").unwrap().busy,
                "a rejected id must not leave the connection busy"
            );
        }
        let job = manager
            .begin_correlated_turn("connection", None, None, None, Some("web:turn-1".into()))
            .unwrap();
        assert_eq!(job.turn_id(), "web:turn-1");
        assert_eq!(
            manager.get("connection").unwrap().turn_id.as_deref(),
            Some("web:turn-1")
        );
        manager.finish_turn(&job, &done(hotsheet_aitools::DoneReason::Completed));
        assert_eq!(manager.get("connection").unwrap().turn_id, None);
        let first = manager.begin_turn("connection", None, None, None).unwrap();
        let first_id = first.turn_id().to_owned();
        manager.finish_turn(&first, &done(hotsheet_aitools::DoneReason::Completed));
        let second = manager.begin_turn("connection", None, None, None).unwrap();
        assert_ne!(
            first_id,
            second.turn_id(),
            "every server-assigned turn is distinct"
        );
        manager.finish_turn(&second, &done(hotsheet_aitools::DoneReason::Completed));
    }

    #[test]
    fn failed_turn_diagnostic_is_saved_and_cleared_on_retry_success_or_interrupt() {
        let manager = manager(true);
        manager
            .create_or_attach(prepare("/project"), Some("connection".into()), None)
            .unwrap();
        let job = manager.begin_turn("connection", None, None, None).unwrap();
        manager.finish_turn(
            &job,
            &done(hotsheet_aitools::DoneReason::failed(1, "Model at capacity")),
        );
        assert_eq!(
            manager.get("connection").unwrap().last_error.as_deref(),
            Some("Model at capacity")
        );
        let job = manager.begin_turn("connection", None, None, None).unwrap();
        assert_eq!(manager.get("connection").unwrap().last_error, None);
        manager.finish_turn(&job, &done(hotsheet_aitools::DoneReason::Completed));
        assert_eq!(manager.get("connection").unwrap().last_error, None);
        let job = manager.begin_turn("connection", None, None, None).unwrap();
        manager.finish_turn(
            &job,
            &done(hotsheet_aitools::DoneReason::Failed {
                exit_code: 7,
                message: None,
            }),
        );
        assert_eq!(
            manager.get("connection").unwrap().last_error.as_deref(),
            Some("The tool turn failed.")
        );
        let job = manager.begin_turn("connection", None, None, None).unwrap();
        manager.finish_turn(&job, &done(hotsheet_aitools::DoneReason::Interrupted));
        assert_eq!(manager.get("connection").unwrap().last_error, None);
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
        assert!(manager.active_sessions.lock_or_recover().is_empty());
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
            backend.envs.lock_or_recover().as_slice(),
            // The drive is an AI session, so it also acts as `ai` (HS2-RD4M29).
            [vec![
                "HOTSHEET_WORKER_ID=fake-connection-1".to_string(),
                "HOTSHEET_ACTOR_ROLE=ai".to_string(),
                "HOTSHEET_ACTOR_ID=fake-connection-1".to_string(),
            ]]
        );
        // Attaching again reuses the drive, and its worker id, without preparing again.
        manager
            .create_or_attach(prepare("/project"), Some("connection-1".into()), None)
            .unwrap();
        assert_eq!(backend.envs.lock_or_recover().len(), 1);

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
