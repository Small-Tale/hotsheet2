//! Orchestration capacity (HS2-J8PRTV, `docs/26-orchestration-projects.md` §26.10): what this
//! machine can take right now, for the orchestrator's capacity read and this server's own
//! admission answer.
//!
//! Nothing here runs on a timer. A reading is taken when a request asks for one, and kept
//! for [`CAPACITY_CACHE`] so several callers share it. Callers queue on one lock, so a burst
//! is one sample. Readings are numbers and tool names: no process path, command line or user
//! name is read (HS2-SW5ZK3).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use hotsheet_cli::orchestration::CapacityProbe;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::checkouts::Checkout;
use hotsheet_ticketing::orchestration::{
    CpuCapacity, DiskCapacity, HostCapabilities, HostObservation, MemoryCapacity, SessionFootprint,
    ToolAvailability,
};

use super::source;
use crate::AppState;

/// How long one reading answers further requests.
pub(crate) const CAPACITY_CACHE: Duration = Duration::from_secs(3);
/// The gap between the two CPU tick readings of one sample.
const CPU_WINDOW: Duration = Duration::from_millis(250);
const MIB: u64 = 1024 * 1024;

/// Memory and CPU of the whole machine. A value the platform cannot report is `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MachineReading {
    /// Memory a new process could use without swapping.
    pub memory_free_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    /// Whole-machine busy share, 0 to 100.
    pub cpu_percent: Option<f64>,
}

/// Where capacity readings come from. The server reads the real host; tests script one.
pub trait CapacitySource: Send {
    fn machine(&mut self) -> MachineReading;

    /// Free and total bytes of the volume that holds `path`.
    fn disk(&mut self, path: &Path) -> Option<(u64, u64)>;

    /// Resident bytes of each process tree rooted at one of `pids`. A pid the platform
    /// cannot report has no entry.
    fn resident_bytes(&mut self, pids: &[u32]) -> HashMap<u32, u64>;

    /// Every drivable AI tool the plugin registry knows, with whether its program is
    /// installed. Nothing is executed to find out.
    fn tools(&mut self, plugin_dirs: &[PathBuf]) -> Option<Vec<(String, bool)>>;

    /// HS2-ZY4Z4G: what this machine is, with the `toolchains` its owner declared. May
    /// run one bounded version probe per program; asked at most once per
    /// [`CAPABILITIES_TTL`](super::capabilities::CAPABILITIES_TTL). A source that cannot
    /// tell answers `None`.
    fn capabilities(
        &mut self,
        _plugin_dirs: &[PathBuf],
        _toolchains: &[String],
    ) -> Option<HostCapabilities> {
        None
    }

    /// HS2-0VTQFF: the model ids each tool's catalog reports, by tool, in the order a
    /// snapshot reports them (see [`reported_models`]). `discover` is the server's
    /// memoized AI-tool discovery, the one behind `/ai-tools` `models[]`. Asked at most
    /// once per [`MODELS_TTL`]. A source that cannot tell answers `None`, and every
    /// tool's models then read as unreported.
    fn models(
        &mut self,
        _discover: &dyn Fn() -> Vec<hotsheet_plugins::AiToolDescriptor>,
    ) -> Option<HashMap<String, Vec<String>>> {
        None
    }
}

/// The real host.
struct SystemCapacity;

/// HS2-YWN8JT: the environment switch a real-server test sets so admission does not
/// depend on how loaded the test machine is. It is honored only by a build with the
/// test permission vault (`hotsheet_aitools::test_permission_vault_available`), never by
/// a shipped server.
pub const ROOMY_CAPACITY_ENV: &str = "HOTSHEET_TEST_ROOMY_CAPACITY";

/// HS2-YWN8JT: a roomy machine for real-server tests: ample memory, an idle CPU and a
/// large disk, with the real host's tool detection and process sizes.
pub struct RoomyFixtureCapacity;

impl CapacitySource for RoomyFixtureCapacity {
    fn machine(&mut self) -> MachineReading {
        const MIB: u64 = 1024 * 1024;
        MachineReading {
            memory_free_bytes: Some(16_384 * MIB),
            memory_total_bytes: Some(32_768 * MIB),
            cpu_percent: Some(10.0),
        }
    }

    fn disk(&mut self, _: &Path) -> Option<(u64, u64)> {
        const MIB: u64 = 1024 * 1024;
        Some((80_000 * MIB, 500_000 * MIB))
    }

    fn resident_bytes(&mut self, pids: &[u32]) -> HashMap<u32, u64> {
        SystemCapacity.resident_bytes(pids)
    }

    fn tools(&mut self, plugin_dirs: &[PathBuf]) -> Option<Vec<(String, bool)>> {
        SystemCapacity.tools(plugin_dirs)
    }

    fn capabilities(
        &mut self,
        plugin_dirs: &[PathBuf],
        toolchains: &[String],
    ) -> Option<HostCapabilities> {
        SystemCapacity.capabilities(plugin_dirs, toolchains)
    }

    fn models(
        &mut self,
        discover: &dyn Fn() -> Vec<hotsheet_plugins::AiToolDescriptor>,
    ) -> Option<HashMap<String, Vec<String>>> {
        SystemCapacity.models(discover)
    }
}

impl CapacitySource for SystemCapacity {
    fn machine(&mut self) -> MachineReading {
        let memory = source::system_memory();
        MachineReading {
            memory_free_bytes: memory.map(|memory| memory.free_bytes),
            memory_total_bytes: memory.map(|memory| memory.total_bytes),
            cpu_percent: source::system_cpu_percent(CPU_WINDOW),
        }
    }

    fn disk(&mut self, path: &Path) -> Option<(u64, u64)> {
        source::system_disk(path).map(|disk| (disk.free_bytes, disk.total_bytes))
    }

    fn resident_bytes(&mut self, pids: &[u32]) -> HashMap<u32, u64> {
        source::system_resident_bytes(pids)
    }

    fn tools(&mut self, plugin_dirs: &[PathBuf]) -> Option<Vec<(String, bool)>> {
        let installed: Vec<String> = hotsheet_plugins::detected_drivable_plugins(plugin_dirs)
            .iter()
            .map(|plugin| plugin.id().to_string())
            .collect();
        Some(
            hotsheet_plugins::all_plugins(plugin_dirs)
                .iter()
                .filter(|plugin| plugin.manifest.drive.is_some())
                .map(|plugin| {
                    let id = plugin.id().to_string();
                    let detected = installed.contains(&id);
                    (id, detected)
                })
                .collect(),
        )
    }

    fn capabilities(
        &mut self,
        plugin_dirs: &[PathBuf],
        toolchains: &[String],
    ) -> Option<HostCapabilities> {
        Some(super::capabilities::detect(
            plugin_dirs,
            toolchains,
            hotsheet_aitools::ai_tool_probes_enabled(),
        ))
    }

    fn models(
        &mut self,
        discover: &dyn Fn() -> Vec<hotsheet_plugins::AiToolDescriptor>,
    ) -> Option<HashMap<String, Vec<String>>> {
        Some(reported_models(&discover()))
    }
}

/// One live AI session as the server knows it. The launch directory stays here: it decides
/// `on_checkout` and never leaves the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveSession {
    pub(crate) id: String,
    pub(crate) tool: Option<String>,
    pub(crate) busy: bool,
    pub(crate) launch_cwd: Option<String>,
}

struct Reading {
    taken: Instant,
    sampled_at_ms: u64,
    machine: MachineReading,
    tools: Option<Vec<(String, bool)>>,
    sessions: Option<Vec<(LiveSession, Option<u64>)>>,
    /// Disk readings by checkout root, each taken once per reading.
    disks: HashMap<PathBuf, Option<(u64, u64)>>,
}

struct Inner {
    source: Box<dyn CapacitySource>,
    reading: Option<Reading>,
    /// HS2-ZY4Z4G: the last capability detection, for the toolchains it was asked about.
    capabilities: Option<CapabilityReading>,
    /// HS2-0VTQFF: the last model-catalog reading, by tool.
    models: Option<ModelsReading>,
}

struct ModelsReading {
    taken: Instant,
    generation: u64,
    report: Option<HashMap<String, Vec<String>>>,
}

/// HS2-0VTQFF: how long one model-catalog reading answers capacity reads. Discovery runs
/// installed tools' version and catalog commands, so it is shared like capabilities.
pub(crate) const MODELS_TTL: Duration = super::capabilities::CAPABILITIES_TTL;

/// HS2-0VTQFF: each discovered tool's model ids as a capacity snapshot reports them: the
/// catalog's default first, then the rest in the catalog's own order (newest first).
pub(crate) fn reported_models(
    tools: &[hotsheet_plugins::AiToolDescriptor],
) -> HashMap<String, Vec<String>> {
    tools
        .iter()
        .map(|tool| {
            let default = tool
                .default_model
                .as_ref()
                .filter(|id| tool.models.iter().any(|model| &model.id == *id));
            let models = default
                .into_iter()
                .cloned()
                .chain(
                    tool.models
                        .iter()
                        .map(|model| model.id.clone())
                        .filter(|id| Some(id) != default),
                )
                .collect();
            (tool.id.clone(), models)
        })
        .collect()
}

struct CapabilityReading {
    taken: Instant,
    toolchains: Vec<String>,
    report: Option<HostCapabilities>,
}

/// The server's capacity state: its source and the reading currently being shared.
pub(crate) struct Capacity {
    inner: Mutex<Inner>,
    cache_ms: AtomicU64,
    samples: AtomicU64,
    detections: AtomicU64,
    /// HS2-0VTQFF: bumped to drop the model reading without waiting behind one.
    models_generation: AtomicU64,
}

impl Default for Capacity {
    fn default() -> Self {
        Self::with_source(Box::new(SystemCapacity))
    }
}

impl std::fmt::Debug for Capacity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Capacity")
            .field("samples", &self.samples())
            .finish_non_exhaustive()
    }
}

impl Capacity {
    pub(crate) fn with_source(source: Box<dyn CapacitySource>) -> Self {
        Self {
            inner: Mutex::new(Inner {
                source,
                reading: None,
                capabilities: None,
                models: None,
            }),
            cache_ms: AtomicU64::new(CAPACITY_CACHE.as_millis() as u64),
            samples: AtomicU64::new(0),
            detections: AtomicU64::new(0),
            models_generation: AtomicU64::new(0),
        }
    }

    pub(crate) fn set_cache(&self, cache: Duration) {
        self.cache_ms
            .store(cache.as_millis() as u64, Ordering::SeqCst);
    }

    /// How many readings were taken since the server started.
    pub(crate) fn samples(&self) -> u64 {
        self.samples.load(Ordering::SeqCst)
    }

    /// How many capability detections were run since the server started.
    pub(crate) fn detections(&self) -> u64 {
        self.detections.load(Ordering::SeqCst)
    }

    /// HS2-ZY4Z4G: what this machine is. Detected when first asked, then shared for
    /// [`CAPABILITIES_TTL`](super::capabilities::CAPABILITIES_TTL), or until the owner
    /// declares other toolchains. Blocking: call it off the async runtime.
    pub(crate) fn capabilities(
        &self,
        plugin_dirs: &[PathBuf],
        toolchains: &[String],
    ) -> Option<HostCapabilities> {
        let mut inner = self.inner.lock_or_recover();
        let fresh = inner.capabilities.as_ref().is_some_and(|reading| {
            reading.toolchains == toolchains
                && reading.taken.elapsed() < super::capabilities::CAPABILITIES_TTL
        });
        if !fresh {
            self.detections.fetch_add(1, Ordering::SeqCst);
            let report = inner.source.capabilities(plugin_dirs, toolchains);
            inner.capabilities = Some(CapabilityReading {
                taken: Instant::now(),
                toolchains: toolchains.to_vec(),
                report,
            });
        }
        inner
            .capabilities
            .as_ref()
            .and_then(|reading| reading.report.clone())
    }

    /// HS2-0VTQFF: the model ids each tool's catalog reports, asked from the source when
    /// first wanted, then shared for [`MODELS_TTL`] or until [`Self::forget_models`].
    /// Blocking: call it off the async runtime.
    pub(crate) fn models(
        &self,
        discover: &dyn Fn() -> Vec<hotsheet_plugins::AiToolDescriptor>,
    ) -> Option<HashMap<String, Vec<String>>> {
        let mut inner = self.inner.lock_or_recover();
        let generation = self.models_generation.load(Ordering::SeqCst);
        let fresh = inner.models.as_ref().is_some_and(|reading| {
            reading.generation == generation && reading.taken.elapsed() < MODELS_TTL
        });
        if !fresh {
            let report = inner.source.models(discover);
            inner.models = Some(ModelsReading {
                taken: Instant::now(),
                generation,
                report,
            });
        }
        inner
            .models
            .as_ref()
            .and_then(|reading| reading.report.clone())
    }

    /// HS2-0VTQFF: drop the shared model reading after tool installation or plugin state
    /// changed, so the next capacity read asks again. Lock-free, like the discovery
    /// invalidation it follows.
    pub(crate) fn forget_models(&self) {
        self.models_generation.fetch_add(1, Ordering::SeqCst);
    }

    /// The capacity of this machine for the checkout rooted at `root`. Blocking: call it
    /// off the async runtime. `sessions` lists the live AI sessions when a reading is due,
    /// `pids` maps their ids to process ids, and `configured` says whether the checkout
    /// has a tool enabled.
    pub(crate) fn observe(
        &self,
        root: &Path,
        plugin_dirs: &[PathBuf],
        sessions: impl FnOnce() -> Option<Vec<LiveSession>>,
        pids: impl FnOnce() -> HashMap<String, u32>,
        configured: impl Fn(&str) -> bool,
    ) -> HostObservation {
        let mut inner = self.inner.lock_or_recover();
        let cache = Duration::from_millis(self.cache_ms.load(Ordering::SeqCst));
        let fresh = inner
            .reading
            .as_ref()
            .is_some_and(|reading| reading.taken.elapsed() < cache);
        if !fresh {
            self.samples.fetch_add(1, Ordering::SeqCst);
            let machine = inner.source.machine();
            let tools = inner.source.tools(plugin_dirs);
            let sessions = sessions().map(|sessions| {
                let pids = pids();
                let roots: Vec<u32> = sessions
                    .iter()
                    .filter_map(|session| pids.get(&session.id).copied())
                    .collect();
                let resident = inner.source.resident_bytes(&roots);
                sessions
                    .into_iter()
                    .map(|session| {
                        let bytes = pids
                            .get(&session.id)
                            .and_then(|pid| resident.get(pid))
                            .copied();
                        (session, bytes)
                    })
                    .collect()
            });
            inner.reading = Some(Reading {
                taken: Instant::now(),
                sampled_at_ms: crate::client_drive::now_ms(),
                machine,
                tools,
                sessions,
                disks: HashMap::new(),
            });
        }
        let Inner {
            source, reading, ..
        } = &mut *inner;
        let Some(reading) = reading.as_mut() else {
            return HostObservation::default();
        };
        let disk = *reading
            .disks
            .entry(root.to_path_buf())
            .or_insert_with(|| source.disk(root));
        // A session started below the checkout's root belongs to it.
        let canonical =
            hotsheet_sync::paths::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        HostObservation {
            capabilities: None,
            sampled_at_ms: Some(reading.sampled_at_ms),
            memory: MemoryCapacity {
                free_mb: reading.machine.memory_free_bytes.map(|bytes| bytes / MIB),
                total_mb: reading.machine.memory_total_bytes.map(|bytes| bytes / MIB),
            },
            cpu: CpuCapacity {
                load_percent: reading
                    .machine
                    .cpu_percent
                    .filter(|percent| percent.is_finite())
                    .map(|percent| percent.clamp(0.0, 100.0).round() as u32),
                logical_cpus: std::thread::available_parallelism()
                    .ok()
                    .and_then(|count| u32::try_from(count.get()).ok()),
            },
            disk: DiskCapacity {
                free_mb: disk.map(|(free, _)| free / MIB),
                total_mb: disk.map(|(_, total)| total / MIB),
            },
            tools: reading.tools.as_ref().map(|tools| {
                tools
                    .iter()
                    .map(|(tool, installed)| ToolAvailability {
                        tool: tool.clone(),
                        installed: *installed,
                        configured: configured(tool),
                        // Filled by the probe from the model catalogs (HS2-0VTQFF).
                        models: None,
                    })
                    .collect()
            }),
            sessions: reading.sessions.as_ref().map(|sessions| {
                sessions
                    .iter()
                    .map(|(session, resident)| SessionFootprint {
                        session_id: session.id.clone(),
                        tool: session.tool.clone(),
                        busy: session.busy,
                        on_checkout: session.launch_cwd.as_ref().map(|cwd| {
                            let cwd = Path::new(cwd);
                            hotsheet_sync::paths::canonicalize(cwd)
                                .unwrap_or_else(|_| cwd.to_path_buf())
                                .starts_with(&canonical)
                        }),
                        // Rounded up, so a small live process never reads as zero.
                        resident_mb: resident.map(|bytes| bytes.div_ceil(MIB)),
                    })
                    .collect()
            }),
        }
    }
}

/// Every live AI session of this server. `None` when the terminal host could not be asked,
/// so a failed listing never reads as an idle machine.
async fn live_sessions(state: &AppState) -> Option<Vec<LiveSession>> {
    let infos = crate::routes::terminals::try_live_terminal_infos(state)
        .await
        .ok()?;
    Some(
        infos
            .into_iter()
            .filter(|info| info.alive && info.kind == hotsheet_terminals::TerminalKind::Ai)
            .map(|info| LiveSession {
                id: info.id,
                tool: info.tool,
                busy: info.busy,
                launch_cwd: info.launch_cwd,
            })
            .collect(),
    )
}

/// The process id of each running AI terminal, from the launch journal Hot Sheet already
/// keeps (ids and purposes only). The most recent unended launch of a session wins.
fn session_pids(home: &Path) -> HashMap<String, u32> {
    hotsheet_process_journal::ProcessJournal::at(home)
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|record| {
            record.purpose == hotsheet_process_journal::Purpose::TerminalAi
                && record.ended_ms.is_none()
        })
        .filter_map(|record| Some((record.session_id?, record.pid)))
        .collect()
}

/// The checkout's `enabled_plugins` setting: unset enables every tool.
fn enabled_tools(root: &Path) -> Option<std::collections::HashSet<String>> {
    let value = hotsheet_ticketing::Settings::for_project(root)
        .get("enabled_plugins", hotsheet_ticketing::Scope::Shared)
        .ok()
        .flatten();
    hotsheet_plugins::enabled_plugins_from_setting(value.as_ref())
}

/// The probe the shared orchestration runtime asks from inside this server. It is called
/// on a blocking thread, so it may wait for the terminal host's answer.
pub(crate) struct ServerCapacity {
    pub(crate) state: AppState,
    pub(crate) runtime: tokio::runtime::Handle,
}

impl CapacityProbe for ServerCapacity {
    fn observe(&self, checkout: &Checkout) -> HostObservation {
        let root = Path::new(&checkout.root);
        let enabled = enabled_tools(root);
        // HS2-ZY4Z4G: the toolchains the machine owner declared, else the default list.
        let toolchains =
            hotsheet_ticketing::orchestration::AdmissionStore::new(self.state.machine_home())
                .read()
                .map(|view| view.admission.declared_toolchains())
                .unwrap_or_else(|_| {
                    hotsheet_ticketing::orchestration::AdmissionConfig::default()
                        .declared_toolchains()
                });
        let capabilities = self
            .state
            .capacity
            .capabilities(&self.state.plugin_dirs, &toolchains);
        let mut observation = self.state.capacity.observe(
            root,
            &self.state.plugin_dirs,
            || self.runtime.block_on(live_sessions(&self.state)),
            || session_pids(self.state.machine_home()),
            |tool| {
                enabled
                    .as_ref()
                    .is_none_or(|enabled| enabled.contains(tool))
            },
        );
        observation.capabilities = capabilities;
        // HS2-0VTQFF: each detected tool's catalog models, from the same discovery as
        // `/ai-tools` `models[]`, so a pattern-only policy resolves to one at launch.
        if let Some(tools) = observation.tools.as_mut()
            && let Some(models) = self
                .state
                .capacity
                .models(&|| crate::routes::permissions::discovered_ai_tools(&self.state, false))
        {
            fill_models(tools, &models);
        }
        observation
    }
}

/// HS2-0VTQFF: give each tool the models its catalog reports; a tool discovery did not
/// report (not detected) keeps `None`.
pub(crate) fn fill_models(tools: &mut [ToolAvailability], models: &HashMap<String, Vec<String>>) {
    for tool in tools {
        tool.models = models.get(&tool.tool).cloned();
    }
}

impl AppState {
    /// Read capacity from `source` instead of the real host (tests, embedders).
    #[must_use]
    pub fn with_capacity_source(mut self, source: Box<dyn CapacitySource>) -> Self {
        self.capacity = std::sync::Arc::new(Capacity::with_source(source));
        self
    }

    /// How long one capacity reading answers further requests.
    #[must_use]
    pub fn with_capacity_cache(self, cache: Duration) -> Self {
        self.capacity.set_cache(cache);
        self
    }

    /// How many capacity readings this server has taken.
    pub fn capacity_samples(&self) -> u64 {
        self.capacity.samples()
    }

    /// HS2-ZY4Z4G: how many capability detections this server has run.
    pub fn capability_detections(&self) -> u64 {
        self.capacity.detections()
    }
}

#[cfg(test)]
#[path = "capacity_tests.rs"]
mod tests;
