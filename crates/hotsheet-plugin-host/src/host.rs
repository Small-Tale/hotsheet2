//! The plugin supervisor (docs/38 §38.6). One long-lived process per registered plugin,
//! started lazily on first use after a trust check. A crash fails in-flight calls with
//! `unavailable`; the next call after an exponential backoff starts a fresh process; too
//! many crashes in a window open a circuit until [`PluginHost::reset`]. An idle plugin
//! exits after `idle_timeout`, measured from its last call, never by polling.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use hotsheet_plugin_protocol::{PROTOCOL_VERSION, Payload, PluginError, Transport, v1};
use serde::{Serialize, de::DeserializeOwned};

use crate::connection::{Connection, Event, LaunchSpec, StartError, lock};
use crate::manifest::CodePluginManifest;
use crate::trust::{TrustError, TrustStore};

/// Environment variables a plugin inherits; everything else is cleared (docs/38 §38.6).
pub const ALLOWED_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "USERNAME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "LANG",
    "TZ",
    "TMPDIR",
    "TEMP",
    "TMP",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// Plugin data directories, relative to the machine's Hot Sheet home.
pub const PLUGIN_DATA_DIR: &str = "plugin-data";

/// Supervisor settings. [`HostConfig::new`] gives the documented defaults.
#[derive(Clone, Debug)]
pub struct HostConfig {
    /// The machine's `HOTSHEET_HOME`: approvals and plugin data live under it.
    pub home: PathBuf,
    pub host_name: String,
    pub host_version: String,
    /// Transports offered, in preference order.
    pub transports: Vec<Transport>,
    pub handshake_timeout: Duration,
    /// Deadline for unary calls that set none.
    pub default_deadline: Duration,
    pub idle_timeout: Duration,
    /// Grace for each shutdown step.
    pub shutdown_grace: Duration,
    pub backoff_initial: Duration,
    pub backoff_max: Duration,
    /// This many crashes within `circuit_window` disable the plugin.
    pub circuit_crashes: usize,
    pub circuit_window: Duration,
    /// Plugins under these directories (the install's first-party plugins) need no
    /// approval.
    pub trusted_roots: Vec<PathBuf>,
}

impl HostConfig {
    /// The defaults from docs/38 §38.5–§38.6.
    pub fn new(home: PathBuf, host_name: &str, host_version: &str) -> Self {
        HostConfig {
            home,
            host_name: host_name.to_string(),
            host_version: host_version.to_string(),
            transports: vec![Transport::Protobuf, Transport::Json],
            handshake_timeout: Duration::from_secs(10),
            default_deadline: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(600),
            shutdown_grace: Duration::from_secs(2),
            backoff_initial: Duration::from_secs(1),
            backoff_max: Duration::from_secs(60),
            circuit_crashes: 5,
            circuit_window: Duration::from_secs(600),
            trusted_roots: Vec::new(),
        }
    }
}

/// A plugin's supervisor state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginStatus {
    /// No process; the next call starts one.
    Stopped,
    /// A process is running.
    Running {
        pid: u32,
        transport: Transport,
        plugin_version: String,
    },
    /// The last process crashed; calls fail until the backoff passes.
    BackingOff {
        retry_in: Duration,
        last_error: String,
    },
    /// Disabled after repeated crashes until [`PluginHost::reset`].
    CircuitOpen { reason: String },
}

struct Slot {
    id: String,
    dir: PathBuf,
    manifest: CodePluginManifest,
    state: Mutex<SlotState>,
    changed: Condvar,
}

struct SlotState {
    conn: Option<Arc<Connection>>,
    starting: bool,
    crashes: VecDeque<Instant>,
    backoff_until: Option<Instant>,
    next_backoff: Duration,
    last_error: String,
    circuit: Option<String>,
}

/// The supervisor shared by every caller in one host process.
pub struct PluginHost {
    config: HostConfig,
    trust: TrustStore,
    slots: Mutex<HashMap<String, Arc<Slot>>>,
}

impl PluginHost {
    pub fn new(config: HostConfig) -> Arc<Self> {
        Arc::new(PluginHost {
            trust: TrustStore::new(&config.home),
            config,
            slots: Mutex::default(),
        })
    }

    /// The configuration in use.
    pub fn config(&self) -> &HostConfig {
        &self.config
    }

    /// The machine's approvals.
    pub fn trust_store(&self) -> &TrustStore {
        &self.trust
    }

    /// Loads `dir/manifest.toml` and registers the plugin, replacing (and stopping) an
    /// earlier registration of the same id. Returns the id.
    pub fn register(&self, dir: &Path) -> Result<String, String> {
        let manifest = CodePluginManifest::load(dir)?;
        let id = manifest.id.clone();
        let slot = Arc::new(Slot {
            id: id.clone(),
            dir: dir.to_path_buf(),
            manifest,
            state: Mutex::new(SlotState {
                conn: None,
                starting: false,
                crashes: VecDeque::new(),
                backoff_until: None,
                next_backoff: self.config.backoff_initial,
                last_error: String::new(),
                circuit: None,
            }),
            changed: Condvar::new(),
        });
        let previous = lock(&self.slots).insert(id.clone(), slot);
        if let Some(previous) = previous {
            self.stop_slot(&previous);
        }
        Ok(id)
    }

    /// The registered manifest of `id`.
    pub fn manifest(&self, id: &str) -> Option<CodePluginManifest> {
        lock(&self.slots).get(id).map(|slot| slot.manifest.clone())
    }

    /// Whether `id` may start: trusted root, or a current approval.
    pub fn check_trust(&self, id: &str) -> Result<(), TrustError> {
        let slot = lock(&self.slots)
            .get(id)
            .cloned()
            .ok_or_else(|| TrustError::Unreadable(format!("plugin {id} is not registered")))?;
        self.trusted(&slot)
    }

    fn trusted(&self, slot: &Slot) -> Result<(), TrustError> {
        let dir = slot.dir.canonicalize().unwrap_or_else(|_| slot.dir.clone());
        let first_party = self.config.trusted_roots.iter().any(|root| {
            root.canonicalize()
                .map(|root| dir.starts_with(root))
                .unwrap_or(false)
        });
        if first_party {
            return Ok(());
        }
        self.trust.check(&slot.id, &slot.dir)
    }

    /// The supervisor state of `id`.
    pub fn status(&self, id: &str) -> Option<PluginStatus> {
        let slot = lock(&self.slots).get(id).cloned()?;
        let state = lock(&slot.state);
        Some(status_of(&state))
    }

    /// Waits up to `timeout` for `id`'s status to satisfy `done`, without polling.
    pub fn wait_for_status(
        &self,
        id: &str,
        timeout: Duration,
        done: impl Fn(&PluginStatus) -> bool,
    ) -> bool {
        let Some(slot) = lock(&self.slots).get(id).cloned() else {
            return false;
        };
        let deadline = Instant::now() + timeout;
        let mut state = lock(&slot.state);
        loop {
            if done(&status_of(&state)) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            // A backoff ends by time, not by an event: wake when it would.
            let mut wait = deadline - now;
            if let Some(until) = state.backoff_until {
                wait = wait.min(until.saturating_duration_since(now) + Duration::from_millis(1));
            }
            state = slot
                .changed
                .wait_timeout(state, wait)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }

    /// Clears a crash circuit and backoff so the next call starts the plugin again.
    pub fn reset(&self, id: &str) -> bool {
        let Some(slot) = lock(&self.slots).get(id).cloned() else {
            return false;
        };
        let mut state = lock(&slot.state);
        state.circuit = None;
        state.backoff_until = None;
        state.crashes.clear();
        state.next_backoff = self.config.backoff_initial;
        slot.changed.notify_all();
        true
    }

    /// Stops `id`'s process, if running. The next call starts a new one.
    pub fn stop(&self, id: &str) {
        if let Some(slot) = lock(&self.slots).get(id).cloned() {
            self.stop_slot(&slot);
        }
    }

    /// Stops every plugin process.
    pub fn shutdown_all(&self) {
        let slots: Vec<Arc<Slot>> = lock(&self.slots).values().cloned().collect();
        for slot in slots {
            self.stop_slot(&slot);
        }
    }

    fn stop_slot(&self, slot: &Arc<Slot>) {
        let conn = lock(&slot.state).conn.take();
        if let Some(conn) = conn {
            conn.shutdown(self.config.shutdown_grace);
        }
        slot.changed.notify_all();
    }

    fn slot(&self, id: &str) -> Result<Arc<Slot>, PluginError> {
        lock(&self.slots).get(id).cloned().ok_or_else(|| {
            PluginError::new(
                hotsheet_plugin_protocol::code::NOT_FOUND,
                format!("no plugin {id} is registered"),
            )
        })
    }

    /// A unary call. `deadline` defaults to [`HostConfig::default_deadline`].
    pub fn call<Req, Resp>(
        self: &Arc<Self>,
        id: &str,
        method: &str,
        request: &Req,
        deadline: Option<Duration>,
    ) -> Result<Resp, PluginError>
    where
        Req: prost::Message + Serialize,
        Resp: prost::Message + Default + DeserializeOwned,
    {
        let deadline = deadline.unwrap_or(self.config.default_deadline);
        let (conn, call_id, rx) = self.send(id, method, request, deadline)?;
        match rx.recv_timeout(deadline) {
            Ok(Event::Done(outcome)) => outcome.and_then(|payload| payload.decode()),
            Ok(Event::Item(_)) => {
                conn.cancel(call_id);
                Err(PluginError::internal(format!(
                    "{method} streamed items on a unary call"
                )))
            }
            Err(RecvTimeoutError::Timeout) => {
                conn.cancel(call_id);
                Err(PluginError::deadline_exceeded(deadline.as_millis() as u64))
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(PluginError::unavailable(format!("plugin {id} stopped")))
            }
        }
    }

    /// A streaming call. With no `deadline`, it waits as long as the plugin streams.
    pub fn stream<Req, Item, Resp>(
        self: &Arc<Self>,
        id: &str,
        method: &str,
        request: &Req,
        deadline: Option<Duration>,
    ) -> Result<CallStream<Item, Resp>, PluginError>
    where
        Req: prost::Message + Serialize,
        Item: prost::Message + Default + DeserializeOwned,
        Resp: prost::Message + Default + DeserializeOwned,
    {
        let (conn, call_id, rx) = self.send(id, method, request, deadline.unwrap_or_default())?;
        Ok(CallStream {
            conn,
            id: call_id,
            rx,
            deadline: deadline.map(|d| (Instant::now() + d, d)),
            outcome: None,
            _types: PhantomData,
        })
    }

    fn send<Req: prost::Message + Serialize>(
        self: &Arc<Self>,
        id: &str,
        method: &str,
        request: &Req,
        deadline: Duration,
    ) -> Result<(Arc<Connection>, u64, Receiver<Event>), PluginError> {
        let slot = self.slot(id)?;
        // A connection that began an idle or replacement shutdown just after it was
        // handed out refuses new calls; one retry gets a fresh process.
        for _ in 0..2 {
            let conn = self.connection(&slot)?;
            let payload = Payload::encode(conn.transport, request);
            match conn.start_call(method, payload, deadline.as_millis() as u64) {
                Ok((call_id, rx)) => return Ok((conn, call_id, rx)),
                Err(StartError::Closing) => {
                    let mut state = lock(&slot.state);
                    if state.conn.as_ref().is_some_and(|c| Arc::ptr_eq(c, &conn)) {
                        state.conn = None;
                    }
                }
                Err(StartError::Failed(error)) => return Err(error),
            }
        }
        Err(PluginError::unavailable(format!(
            "plugin {id} is restarting"
        )))
    }

    fn connection(self: &Arc<Self>, slot: &Arc<Slot>) -> Result<Arc<Connection>, PluginError> {
        let mut state = lock(&slot.state);
        loop {
            if let Some(reason) = &state.circuit {
                return Err(PluginError::unavailable(format!(
                    "plugin {} is disabled after repeated crashes ({reason}); reset it to try again",
                    slot.id
                )));
            }
            if let Some(conn) = &state.conn {
                if conn.is_open() {
                    return Ok(conn.clone());
                }
                state.conn = None;
            }
            if state.starting {
                state = slot
                    .changed
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                continue;
            }
            if let Some(until) = state.backoff_until {
                let now = Instant::now();
                if until > now {
                    let mut error = PluginError::unavailable(format!(
                        "plugin {} is restarting after a crash ({})",
                        slot.id, state.last_error
                    ));
                    error.retry_after_seconds = Some((until - now).as_secs().max(1));
                    return Err(error);
                }
                state.backoff_until = None;
            }
            state.starting = true;
            break;
        }
        drop(state);
        let launched = self.launch(slot);
        let mut state = lock(&slot.state);
        state.starting = false;
        let result = match launched {
            Ok(conn) => {
                state.conn = Some(conn.clone());
                self.watch_idle(slot, &conn);
                Ok(conn)
            }
            Err(Launch::Untrusted(error)) => Err(PluginError::unavailable(error.to_string())),
            Err(Launch::Failed(error)) => {
                self.record_crash(&mut state, error.message.clone(), Duration::ZERO);
                Err(error)
            }
        };
        slot.changed.notify_all();
        result
    }

    fn launch(self: &Arc<Self>, slot: &Arc<Slot>) -> Result<Arc<Connection>, Launch> {
        self.trusted(slot).map_err(Launch::Untrusted)?;
        let program = slot
            .manifest
            .resolve_program(&slot.dir)
            .map_err(|e| Launch::Failed(PluginError::unavailable(e)))?;
        let data_dir = self.config.home.join(PLUGIN_DATA_DIR).join(&slot.id);
        let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(name, _)| {
                name.to_str()
                    .is_some_and(|name| ALLOWED_ENV.contains(&name) || name.starts_with("LC_"))
            })
            .collect();
        env.push(("HOTSHEET_PLUGIN_ID".into(), slot.id.clone().into()));
        env.push(("HOTSHEET_PLUGIN_DATA_DIR".into(), data_dir.clone().into()));
        let hello = v1::Hello {
            protocol_versions: vec![PROTOCOL_VERSION],
            transports: self
                .config
                .transports
                .iter()
                .map(|t| t.name().to_string())
                .collect(),
            host: Some(v1::HostInfo {
                name: self.config.host_name.clone(),
                version: self.config.host_version.clone(),
            }),
            plugin_id: slot.id.clone(),
            data_dir: data_dir.to_string_lossy().into_owned(),
        };
        let weak_slot: Weak<Slot> = Arc::downgrade(slot);
        let weak_host: Weak<PluginHost> = Arc::downgrade(self);
        Connection::start(
            LaunchSpec {
                plugin_id: slot.id.clone(),
                program,
                args: slot.manifest.executable.args.clone(),
                data_dir,
                env,
                hello,
                declared_capabilities: slot.manifest.capabilities.clone(),
                handshake_timeout: self.config.handshake_timeout,
            },
            Box::new(move |conn| {
                if let (Some(slot), Some(host)) = (weak_slot.upgrade(), weak_host.upgrade()) {
                    host.closed(&slot, conn);
                }
            }),
        )
        .map_err(Launch::Failed)
    }

    /// Called on the reader thread once `conn` has closed.
    fn closed(&self, slot: &Slot, conn: &Arc<Connection>) {
        let mut state = lock(&slot.state);
        if state.conn.as_ref().is_some_and(|c| Arc::ptr_eq(c, conn)) {
            state.conn = None;
        }
        if !conn.shutdown_requested() {
            let reason = conn
                .close_reason()
                .unwrap_or_else(|| "it stopped".to_string());
            tracing::warn!(plugin = %slot.id, "plugin stopped unexpectedly: {reason}");
            self.record_crash(&mut state, reason, conn.started_at.elapsed());
        }
        slot.changed.notify_all();
    }

    fn record_crash(&self, state: &mut SlotState, reason: String, lived: Duration) {
        // A process that ran longer than the longest backoff was healthy; start over.
        if lived >= self.config.backoff_max {
            state.next_backoff = self.config.backoff_initial;
        }
        let now = Instant::now();
        state.crashes.push_back(now);
        while state
            .crashes
            .front()
            .is_some_and(|t| now.duration_since(*t) > self.config.circuit_window)
        {
            state.crashes.pop_front();
        }
        state.last_error = reason.clone();
        if state.crashes.len() >= self.config.circuit_crashes {
            state.circuit = Some(reason);
            state.backoff_until = None;
        } else {
            state.backoff_until = Some(now + state.next_backoff);
            state.next_backoff = (state.next_backoff * 2).min(self.config.backoff_max);
        }
    }

    /// Stops `conn` once it has had no call in flight for `idle_timeout`. The thread
    /// sleeps until the earliest moment that could be true, so an idle host makes no
    /// requests and wakes at most once per idle period.
    fn watch_idle(&self, slot: &Arc<Slot>, conn: &Arc<Connection>) {
        let idle = self.config.idle_timeout;
        let grace = self.config.shutdown_grace;
        let slot = Arc::downgrade(slot);
        let conn = Arc::downgrade(conn);
        // Without the watcher the plugin simply stays up until the host exits.
        let _ = std::thread::Builder::new()
            .name("plugin-idle".to_string())
            .spawn(move || {
                loop {
                    let Some(current) = conn.upgrade() else {
                        return;
                    };
                    if !current.is_open() {
                        return;
                    }
                    let wait = match current.idle_for() {
                        Some(quiet) if quiet >= idle => {
                            let Some(slot) = slot.upgrade() else { return };
                            {
                                let mut state = lock(&slot.state);
                                if state
                                    .conn
                                    .as_ref()
                                    .is_some_and(|c| Arc::ptr_eq(c, &current))
                                {
                                    state.conn = None;
                                }
                            }
                            tracing::debug!(plugin = %current.plugin_id, "stopping idle plugin");
                            current.shutdown(grace);
                            slot.changed.notify_all();
                            return;
                        }
                        Some(quiet) => idle - quiet,
                        None => idle,
                    };
                    // Returns early only if the process exits.
                    if current.wait_exited(wait) {
                        return;
                    }
                }
            });
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

enum Launch {
    Untrusted(TrustError),
    Failed(PluginError),
}

fn status_of(state: &SlotState) -> PluginStatus {
    if let Some(reason) = &state.circuit {
        return PluginStatus::CircuitOpen {
            reason: reason.clone(),
        };
    }
    if let Some(conn) = state.conn.as_ref().filter(|c| c.is_open()) {
        return PluginStatus::Running {
            pid: conn.pid,
            transport: conn.transport,
            plugin_version: conn.plugin_version.clone(),
        };
    }
    if let Some(until) = state.backoff_until {
        let now = Instant::now();
        if until > now {
            return PluginStatus::BackingOff {
                retry_in: until - now,
                last_error: state.last_error.clone(),
            };
        }
    }
    PluginStatus::Stopped
}

/// The items of a streaming call, then its final response through
/// [`CallStream::finish`]. Dropping it early cancels the call.
pub struct CallStream<Item, Resp> {
    conn: Arc<Connection>,
    id: u64,
    rx: Receiver<Event>,
    deadline: Option<(Instant, Duration)>,
    outcome: Option<Result<Resp, PluginError>>,
    _types: PhantomData<fn() -> (Item, Resp)>,
}

impl<Item, Resp> CallStream<Item, Resp>
where
    Item: prost::Message + Default + DeserializeOwned,
    Resp: prost::Message + Default + DeserializeOwned,
{
    /// Drains any remaining items and returns the final response.
    pub fn finish(mut self) -> Result<Resp, PluginError> {
        while self.next().is_some() {}
        self.outcome
            .take()
            .unwrap_or_else(|| Err(PluginError::internal("the stream ended without a response")))
    }
}

impl<Item, Resp> Iterator for CallStream<Item, Resp>
where
    Item: prost::Message + Default + DeserializeOwned,
    Resp: prost::Message + Default + DeserializeOwned,
{
    type Item = Result<Item, PluginError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.outcome.is_some() {
            return None;
        }
        let event = match self.deadline {
            Some((at, budget)) => match self
                .rx
                .recv_timeout(at.saturating_duration_since(Instant::now()))
            {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    self.conn.cancel(self.id);
                    self.outcome = Some(Err(PluginError::deadline_exceeded(
                        budget.as_millis() as u64
                    )));
                    return None;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.outcome = Some(Err(PluginError::unavailable("the plugin stopped")));
                    return None;
                }
            },
            None => match self.rx.recv() {
                Ok(event) => event,
                Err(_) => {
                    self.outcome = Some(Err(PluginError::unavailable("the plugin stopped")));
                    return None;
                }
            },
        };
        match event {
            Event::Item(payload) => Some(payload.decode()),
            Event::Done(outcome) => {
                self.outcome = Some(outcome.and_then(|payload| payload.decode()));
                None
            }
        }
    }
}

impl<Item, Resp> Drop for CallStream<Item, Resp> {
    fn drop(&mut self) {
        if self.outcome.is_none() {
            self.conn.cancel(self.id);
        }
    }
}
