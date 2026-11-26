//! Routing and running remote-change connections (HS2-8X2S08, `docs/37` §37.4–37.5).
//!
//! [`Router`] is the pure half: it decides whether a hint is for a bound source, drops
//! duplicates, and coalesces bursts, all against an injected clock. [`Hub`] is the running
//! half: one supervised thread per connection (a connection is shared by every source bound
//! to it), a dispatcher thread that wakes only for due trailing deliveries, bounded reconnect
//! backoff, typed health, and a recheck request after every connect that may have missed
//! events. Nothing here polls a network on a timer; the only periodic local wake is an
//! adapter checking its stop flag.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use hotsheet_sync::{CondvarExt, LockExt};
use hotsheet_ticketing::secrets::SecretStore;

use super::model::{AdapterError, ChangeAdapter, ChangeHint, ConnectionContext, Event, Health};
use super::registry::{Binding, Connection, Registry};

/// What a consumer is told about a hint that passed validation. It carries no ticket data:
/// it asks the consumer to check `source`'s authoritative remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChange {
    pub source: String,
    pub ref_name: String,
    /// What the event advertised. A hint to compare against, never a fact to store.
    pub advertised_tip: Option<String>,
    pub event_id: Option<String>,
}

/// Why a source should be checked without any hint naming it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecheckReason {
    /// The connection came up, or came back after a gap: events may have been missed.
    Connected,
}

/// Who acts on hints. Called from the hub's threads; keep it quick and non-blocking.
pub trait ChangeListener: Send + Sync {
    /// A hint for a bound source passed identity, ref and duplicate checks.
    fn changed(&self, change: &RemoteChange);
    /// Check this source against its authoritative remote.
    fn recheck(&self, source: &str, reason: RecheckReason);
    /// A connection's health changed.
    fn health(&self, _connection: &str, _health: &Health) {}
}

/// Why a hint was not delivered. Counted, never printed with the hint's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignored {
    /// No binding listens on this connection and channel.
    UnknownChannel,
    /// The hint names a repository the binding does not expect.
    WrongRepository,
    /// The ref is not one the binding follows.
    UnfollowedRef,
    /// The same event or the same (ref, tip) was already handled for this source.
    Duplicate,
}

const SEEN_CAPACITY: usize = 2048;

/// The pure routing half.
pub struct Router {
    bindings: Vec<Binding>,
    window_ms: u64,
    seen: VecDeque<String>,
    seen_set: std::collections::HashSet<String>,
    last_delivered: HashMap<(String, String), u64>,
    pending: BTreeMap<(String, String), (RemoteChange, u64)>,
    pub ignored: HashMap<&'static str, u64>,
}

fn reason_name(reason: Ignored) -> &'static str {
    match reason {
        Ignored::UnknownChannel => "unknown_channel",
        Ignored::WrongRepository => "wrong_repository",
        Ignored::UnfollowedRef => "unfollowed_ref",
        Ignored::Duplicate => "duplicate",
    }
}

impl Router {
    pub fn new(bindings: Vec<Binding>, window_ms: u64) -> Self {
        Self {
            bindings,
            window_ms,
            seen: VecDeque::new(),
            seen_set: std::collections::HashSet::new(),
            last_delivered: HashMap::new(),
            pending: BTreeMap::new(),
            ignored: HashMap::new(),
        }
    }

    pub fn set_bindings(&mut self, bindings: Vec<Binding>) {
        self.bindings = bindings;
    }

    fn remember(&mut self, key: String) -> bool {
        if !self.seen_set.insert(key.clone()) {
            return false;
        }
        self.seen.push_back(key);
        while self.seen.len() > SEEN_CAPACITY {
            if let Some(old) = self.seen.pop_front() {
                self.seen_set.remove(&old);
            }
        }
        true
    }

    fn ignore(&mut self, reason: Ignored) -> Ignored {
        *self.ignored.entry(reason_name(reason)).or_default() += 1;
        reason
    }

    /// Offer one hint. Returns the deliveries to make now (the leading edge of a burst) and,
    /// when nothing is deliverable, why. A hint a source already handled, by event id or by
    /// (ref, tip), is a duplicate; a hint inside a source's coalescing window is held and
    /// delivered once, latest first, by [`Router::due`].
    pub fn offer(
        &mut self,
        connection: &str,
        channel: &str,
        hint: &ChangeHint,
        now_ms: u64,
    ) -> Result<Vec<RemoteChange>, Ignored> {
        let listening: Vec<&Binding> = self
            .bindings
            .iter()
            .filter(|b| b.connection == connection && b.channel == channel)
            .collect();
        if listening.is_empty() {
            return Err(self.ignore(Ignored::UnknownChannel));
        }
        let mut repository_seen = false;
        let mut matched: Vec<String> = Vec::new();
        for binding in listening {
            if !hint.repository.matches(&binding.repository) {
                continue;
            }
            repository_seen = true;
            if binding.follows(&hint.ref_name) && !matched.contains(&binding.source) {
                matched.push(binding.source.clone());
            }
        }
        if matched.is_empty() {
            let reason = if repository_seen {
                Ignored::UnfollowedRef
            } else {
                Ignored::WrongRepository
            };
            return Err(self.ignore(reason));
        }
        let mut deliver = Vec::new();
        for source in matched {
            let by_event = hint
                .event_id
                .as_ref()
                .map(|id| format!("{source}\u{1}e\u{1}{id}"));
            let by_tip = hint
                .advertised_tip
                .as_ref()
                .map(|tip| format!("{source}\u{1}t\u{1}{}\u{1}{tip}", hint.ref_name));
            // Both keys are recorded, so a delivery repeated under a new id still collapses.
            let fresh_event = by_event.is_none_or(|k| self.remember(k));
            let fresh_tip = by_tip.is_none_or(|k| self.remember(k));
            if !(fresh_event && fresh_tip) {
                self.ignore(Ignored::Duplicate);
                continue;
            }
            let change = RemoteChange {
                source: source.clone(),
                ref_name: hint.ref_name.clone(),
                advertised_tip: hint.advertised_tip.clone(),
                event_id: hint.event_id.clone(),
            };
            let key = (source, hint.ref_name.clone());
            let quiet = self
                .last_delivered
                .get(&key)
                .is_none_or(|last| now_ms >= last.saturating_add(self.window_ms));
            if quiet && !self.pending.contains_key(&key) {
                self.last_delivered.insert(key, now_ms);
                deliver.push(change);
            } else {
                let due = self
                    .last_delivered
                    .get(&key)
                    .map_or(now_ms, |last| last.saturating_add(self.window_ms));
                self.pending.insert(key, (change, due));
            }
        }
        if deliver.is_empty() && self.pending.is_empty() {
            return Err(Ignored::Duplicate);
        }
        Ok(deliver)
    }

    /// The trailing deliveries whose window has ended.
    pub fn due(&mut self, now_ms: u64) -> Vec<RemoteChange> {
        let ready: Vec<(String, String)> = self
            .pending
            .iter()
            .filter(|(_, (_, due))| *due <= now_ms)
            .map(|(key, _)| key.clone())
            .collect();
        let mut out = Vec::new();
        for key in ready {
            if let Some((change, _)) = self.pending.remove(&key) {
                self.last_delivered.insert(key, now_ms);
                out.push(change);
            }
        }
        out
    }

    /// When the next trailing delivery falls due, if any is waiting.
    pub fn next_due(&self) -> Option<u64> {
        self.pending.values().map(|(_, due)| *due).min()
    }

    /// Forget what was held for a source (it was unbound).
    pub fn forget_source(&mut self, source: &str) {
        self.pending.retain(|(s, _), _| s != source);
        self.last_delivered.retain(|(s, _), _| s != source);
    }
}

/// A bounded doubling reconnect delay.
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    pub start: Duration,
    pub cap: Duration,
}

impl Backoff {
    pub const DEFAULT: Backoff = Backoff {
        start: Duration::from_millis(500),
        cap: Duration::from_secs(60),
    };

    /// The wait after the `failures`-th consecutive failure (1-based).
    pub fn delay(&self, failures: u32) -> Duration {
        self.start
            .saturating_mul(1u32 << failures.saturating_sub(1).min(16))
            .min(self.cap)
    }
}

/// A stop request a sleeping or running connection thread can be woken by.
#[derive(Default)]
struct Stop {
    flag: AtomicBool,
    gate: Mutex<()>,
    wake: Condvar,
}

impl Stop {
    fn set(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let _held = self.gate.lock_or_recover();
        self.wake.notify_all();
    }

    fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Sleep up to `duration`, waking early when stopped. Returns whether stopped.
    fn sleep(&self, duration: Duration) -> bool {
        let guard = self.gate.lock_or_recover();
        let _ = self
            .wake
            .wait_timeout_while_or_recover(guard, duration, |_| !self.is_set());
        self.is_set()
    }
}

struct Running {
    signature: (Connection, Vec<String>),
    stop: Arc<Stop>,
    handle: JoinHandle<()>,
}

struct State {
    router: Router,
    registry: Registry,
    running: BTreeMap<String, Running>,
    health: BTreeMap<String, Health>,
    shutting_down: bool,
}

struct Shared {
    adapters: BTreeMap<String, Arc<dyn ChangeAdapter>>,
    secrets: Arc<dyn SecretStore + Send + Sync>,
    listener: Arc<dyn ChangeListener>,
    backoff: Backoff,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    state: Mutex<State>,
    wake: Condvar,
}

/// The running hub: owns the connection threads and the dispatcher.
pub struct Hub {
    home: PathBuf,
    shared: Arc<Shared>,
    dispatcher: Mutex<Option<JoinHandle<()>>>,
}

impl Shared {
    fn publish(&self, connection: &str, health: Health) {
        {
            let mut state = self.state.lock_or_recover();
            if state.health.get(connection) == Some(&health) {
                return;
            }
            state.health.insert(connection.to_owned(), health.clone());
        }
        self.listener.health(connection, &health);
    }

    fn sources_of(&self, connection: &str) -> Vec<String> {
        let state = self.state.lock_or_recover();
        let mut sources: Vec<String> = state
            .registry
            .bindings_of(connection)
            .iter()
            .map(|b| b.source.clone())
            .collect();
        sources.sort();
        sources.dedup();
        sources
    }

    fn hint(&self, connection: &str, channel: &str, hint: &ChangeHint) {
        let now = (self.now_ms)();
        let deliveries = {
            let mut state = self.state.lock_or_recover();
            let out = state.router.offer(connection, channel, hint, now);
            if state.router.next_due().is_some() {
                self.wake.notify_all();
            }
            out.unwrap_or_default()
        };
        for change in deliveries {
            self.listener.changed(&change);
        }
    }

    fn supervise(self: &Arc<Self>, connection: Connection, channels: Vec<String>, stop: Arc<Stop>) {
        let Some(adapter) = self.adapters.get(&connection.plugin).cloned() else {
            self.publish(
                &connection.id,
                Health::Misconfigured {
                    error: format!(
                        "no adapter '{}' is available in this build",
                        connection.plugin
                    ),
                },
            );
            return;
        };
        let mut failures = 0u32;
        let mut first = true;
        while !stop.is_set() {
            self.publish(&connection.id, Health::Connecting);
            let credential = match resolve_credential(self.secrets.as_ref(), &connection, &adapter)
            {
                Ok(credential) => credential,
                Err(error) => {
                    self.publish(&connection.id, Health::Misconfigured { error });
                    return;
                }
            };
            let context = ConnectionContext {
                connection: connection.id.clone(),
                settings: connection.settings.clone(),
                credential: credential.map(hotsheet_sync::secret::SecretString::from),
            };
            let mut announced = false;
            let mut emit = |event: Event| match event {
                Event::Connected { resumed } => {
                    announced = true;
                    failures = 0;
                    self.publish(
                        &connection.id,
                        Health::Connected {
                            since_ms: (self.now_ms)(),
                        },
                    );
                    // A first connect, or a reconnect that may have missed events, asks every
                    // bound source for an authoritative check.
                    if first || !resumed {
                        for source in self.sources_of(&connection.id) {
                            self.listener.recheck(&source, RecheckReason::Connected);
                        }
                    }
                    first = false;
                }
                Event::Hint { channel, hint } => self.hint(&connection.id, &channel, &hint),
            };
            let result = adapter.run(&context, &channels, &mut emit, &stop.flag);
            if stop.is_set() {
                return;
            }
            let _ = announced;
            match result {
                Ok(()) => return,
                Err(AdapterError::Auth(error)) => {
                    self.publish(&connection.id, Health::AuthFailed { error });
                    return;
                }
                Err(AdapterError::Config(error)) => {
                    self.publish(&connection.id, Health::Misconfigured { error });
                    return;
                }
                Err(AdapterError::Transient(error)) => {
                    failures += 1;
                    let delay = self.backoff.delay(failures);
                    self.publish(
                        &connection.id,
                        Health::Reconnecting {
                            error,
                            retry_in_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                        },
                    );
                    // A bounded backoff after a failed connection, woken early by a stop.
                    if stop.sleep(delay) {
                        return;
                    }
                }
            }
        }
    }

    fn dispatch(self: &Arc<Self>) {
        loop {
            let (deliveries, wait) = {
                let mut state = self.state.lock_or_recover();
                if state.shutting_down {
                    return;
                }
                let now = (self.now_ms)();
                let due = state.router.due(now);
                let wait = state
                    .router
                    .next_due()
                    .map(|at| Duration::from_millis(at.saturating_sub(now).max(1)));
                if due.is_empty() {
                    // Nothing due: sleep to the next deadline, or until something is pending.
                    let state = match wait {
                        Some(wait) => self.wake.wait_timeout_or_recover(state, wait).0,
                        None => self.wake.wait_or_recover(state),
                    };
                    drop(state);
                    continue;
                }
                (due, wait)
            };
            let _ = wait;
            for change in deliveries {
                self.listener.changed(&change);
            }
        }
    }
}

fn resolve_credential(
    secrets: &dyn SecretStore,
    connection: &Connection,
    adapter: &Arc<dyn ChangeAdapter>,
) -> Result<Option<String>, String> {
    if !adapter.descriptor().credential {
        return Ok(None);
    }
    let account = connection
        .credential
        .clone()
        .unwrap_or_else(|| Connection::default_credential(&connection.id));
    match secrets.get(&account) {
        Ok(Some(value)) if !value.is_empty() => Ok(Some(value)),
        Ok(_) => Err(format!(
            "no credential for connection '{0}'; run `hotsheet-cli remote-change connection secret {0}` and give it on standard input",
            connection.id
        )),
        Err(error) => Err(format!("OS credential store: {error}")),
    }
}

impl Hub {
    /// Start a hub for the registry in `home`. Nothing connects until [`Hub::reload`].
    pub fn start(
        home: PathBuf,
        adapters: Vec<Arc<dyn ChangeAdapter>>,
        secrets: Arc<dyn SecretStore + Send + Sync>,
        listener: Arc<dyn ChangeListener>,
        backoff: Backoff,
        window: Duration,
        now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        let adapters = adapters
            .into_iter()
            .map(|a| (a.descriptor().id.to_owned(), a))
            .collect();
        let shared = Arc::new(Shared {
            adapters,
            secrets,
            listener,
            backoff,
            now_ms,
            state: Mutex::new(State {
                router: Router::new(
                    Vec::new(),
                    u64::try_from(window.as_millis()).unwrap_or(u64::MAX),
                ),
                registry: Registry::default(),
                running: BTreeMap::new(),
                health: BTreeMap::new(),
                shutting_down: false,
            }),
            wake: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        let dispatcher = std::thread::spawn(move || worker.dispatch());
        Self {
            home,
            shared,
            dispatcher: Mutex::new(Some(dispatcher)),
        }
    }

    /// Re-read the registry and reconcile: start a thread for each bound connection, restart
    /// one whose definition or channel set changed, and stop one that is no longer bound.
    /// An unchanged connection keeps its connection, so a reload never drops a healthy one.
    pub fn reload(&self) -> Result<(), super::registry::RegistryError> {
        let registry = Registry::load(&self.home)?;
        let desired = registry.subscriptions();
        let mut stopped = Vec::new();
        let mut start = Vec::new();
        {
            let mut state = self.shared.state.lock_or_recover();
            let sources_before: Vec<String> = state
                .registry
                .bindings
                .iter()
                .map(|b| b.source.clone())
                .collect();
            state.router.set_bindings(registry.bindings.clone());
            for source in sources_before {
                if !registry.bindings.iter().any(|b| b.source == source) {
                    state.router.forget_source(&source);
                }
            }
            state.registry = registry.clone();
            let names: Vec<String> = state.running.keys().cloned().collect();
            for name in names {
                let keep = match (desired.get(&name), registry.connection(&name)) {
                    (Some(channels), Some(connection)) => {
                        state.running[&name].signature == (connection.clone(), channels.clone())
                    }
                    _ => false,
                };
                if !keep && let Some(running) = state.running.remove(&name) {
                    stopped.push((name, running));
                }
            }
            for (name, channels) in &desired {
                if state.running.contains_key(name) {
                    continue;
                }
                if let Some(connection) = registry.connection(name) {
                    start.push((connection.clone(), channels.clone()));
                }
            }
            let bound: Vec<&String> = desired.keys().collect();
            let idle: Vec<String> = state
                .health
                .keys()
                .filter(|k| !bound.contains(k))
                .cloned()
                .collect();
            for name in idle {
                state.health.insert(name, Health::Idle);
            }
        }
        for (name, running) in stopped {
            running.stop.set();
            let _ = running.handle.join();
            self.shared.publish(&name, Health::Idle);
        }
        for (connection, channels) in start {
            let stop = Arc::new(Stop::default());
            let shared = Arc::clone(&self.shared);
            let signature = (connection.clone(), channels.clone());
            let name = connection.id.clone();
            let thread_stop = Arc::clone(&stop);
            let handle =
                std::thread::spawn(move || shared.supervise(connection, channels, thread_stop));
            self.shared.state.lock_or_recover().running.insert(
                name,
                Running {
                    signature,
                    stop,
                    handle,
                },
            );
        }
        Ok(())
    }

    /// Every connection's health, by id. A connection nothing is bound to reads `idle`.
    pub fn health(&self) -> BTreeMap<String, Health> {
        let state = self.shared.state.lock_or_recover();
        let mut out: BTreeMap<String, Health> = state
            .registry
            .connections
            .iter()
            .map(|c| (c.id.clone(), Health::Idle))
            .collect();
        out.extend(state.health.clone());
        out
    }

    /// How many hints the router has ignored, by reason.
    pub fn ignored(&self) -> HashMap<&'static str, u64> {
        self.shared.state.lock_or_recover().router.ignored.clone()
    }

    /// Poison the shared state lock the way a panicking connection or dispatcher thread
    /// would, so a test can prove the hub keeps serving (HS2-GSW2AA).
    #[cfg(test)]
    pub(super) fn poison_state_for_test(&self) {
        let shared = Arc::clone(&self.shared);
        let _ = std::thread::spawn(move || {
            let _guard = shared.state.lock();
            panic!("poison the hub state");
        })
        .join();
        assert!(self.shared.state.is_poisoned());
    }

    /// Stop every connection and the dispatcher, waiting for their threads.
    pub fn shutdown(&self) {
        let running: Vec<Running> = {
            let mut state = self.shared.state.lock_or_recover();
            state.shutting_down = true;
            self.shared.wake.notify_all();
            std::mem::take(&mut state.running).into_values().collect()
        };
        for running in &running {
            running.stop.set();
        }
        for running in running {
            let _ = running.handle.join();
        }
        if let Some(handle) = self.dispatcher.lock_or_recover().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        self.shutdown();
    }
}
