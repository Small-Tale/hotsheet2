//! HS2-J8PRTV: what a target machine can take, and whether a session may start there.
//!
//! A [`CapacitySnapshot`] is what the server that hosts a target measured just now.
//! Anything it could not measure is `None`; nothing is ever guessed. A process is
//! described by its tool name and metrics only: no path, command line or user name
//! (HS2-SW5ZK3).
//!
//! [`decide_admission`] answers "may sessions of this tool start here now" from one
//! snapshot, the limits the orchestrator's policy sent, and the machine owner's own
//! [`AdmissionConfig`]. The owner's file can only make the answer stricter. Pure:
//! the caller supplies the snapshot. Integer units only (megabytes, percent).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::capabilities::HostCapabilities;
use super::contract::{bounded_text, valid_identifier};
use super::policy::{MAX_POLICY_SESSIONS, MAX_POLICY_TOOLS, MachineLimits};
use super::storage::{Storage, decode};
use super::{ORCHESTRATION_VERSION, OfflinePolicy, OrchestrationError};

/// The estimate used for a tool the machine owner configured no footprint for.
pub const DEFAULT_SESSION_FOOTPRINT_MB: u64 = 1024;
pub const MAX_SESSION_FOOTPRINT_MB: u64 = 1024 * 1024;
/// Claim namespaces one snapshot request may name.
pub const MAX_CLAIM_NAMESPACES: usize = 8;
/// HS2-KCEHVR: requests of [`MAX_CLAIM_NAMESPACES`] each that one scheduling pass makes
/// to one target, so a target's work may name this many times as many namespaces before
/// one waits as `namespace_limit`.
pub const MAX_CLAIM_PAGES: usize = 4;

/// HS2-KCEHVR: one named namespace whose backend could not be read, with what its
/// configuration says to do meanwhile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreadableNamespace {
    pub namespace: String,
    pub offline: OfflinePolicy,
}
const MAX_ADMISSION_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCapacity {
    /// Memory a new process could use without swapping, in megabytes.
    pub free_mb: Option<u64>,
    pub total_mb: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuCapacity {
    /// Whole-machine busy share over the sampling window, 0 to 100.
    pub load_percent: Option<u32>,
    pub logical_cpus: Option<u32>,
}

/// The volume that holds the hosted checkout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskCapacity {
    pub free_mb: Option<u64>,
    pub total_mb: Option<u64>,
}

/// One drivable AI tool the host knows, by name only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolAvailability {
    pub tool: String,
    /// Its program is present on the host.
    pub installed: bool,
    /// The hosted checkout has not opted out of it.
    pub configured: bool,
}

/// One running AI session: an id, its tool and what it was observed to use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFootprint {
    pub session_id: String,
    /// `None` when the session does not name its tool.
    pub tool: Option<String>,
    pub busy: bool,
    /// Whether it was started in the hosted checkout; `None` when that is unknown.
    pub on_checkout: Option<bool>,
    /// Resident memory of the session's process and its descendants, in megabytes.
    /// `None` when the platform could not report it.
    pub resident_mb: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldClaim {
    pub namespace: String,
    pub resource: String,
    pub expires_at_ms: u64,
}

/// What the host itself measured. The runtime adds identity and held claims.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostObservation {
    /// `None` when nothing was sampled at all.
    pub sampled_at_ms: Option<u64>,
    pub memory: MemoryCapacity,
    pub cpu: CpuCapacity,
    pub disk: DiskCapacity,
    pub tools: Option<Vec<ToolAvailability>>,
    pub sessions: Option<Vec<SessionFootprint>>,
    /// HS2-ZY4Z4G: what the machine is (OS, architecture, CPU, GPU, tool versions,
    /// toolchains). `None` when the host did not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<HostCapabilities>,
}

/// HS2-JSF985: a target project's own committed opt-in to provisional readiness
/// (its shared `speculation` setting, HS2-2SJPCH), as its machine reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct TargetSpeculation {
    pub enabled: bool,
    /// How many unconfirmed gates the project lets stack beneath one dependent.
    pub max_depth: u32,
    /// HS2-ERYQDV: where the project lets provisional work go while its base gate is
    /// pending. Absent (the default) is `push-per-ticket`, so an older host and a
    /// project that keeps the default answer as before.
    #[serde(default, skip_serializing_if = "pushes_per_ticket")]
    pub integration: crate::provisional::IntegrationRule,
}

fn pushes_per_ticket(rule: &crate::provisional::IntegrationRule) -> bool {
    *rule == crate::provisional::IntegrationRule::PushPerTicket
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacitySnapshot {
    pub schema_version: u32,
    pub server_id: String,
    pub checkout_id: String,
    pub sampled_at_ms: Option<u64>,
    pub memory: MemoryCapacity,
    pub cpu: CpuCapacity,
    pub disk: DiskCapacity,
    pub tools: Option<Vec<ToolAvailability>>,
    pub sessions: Option<Vec<SessionFootprint>>,
    /// Live claims in `claim_namespaces`: the namespaces the caller named or, when it
    /// named none, every namespace with claim state on the machine (HS2-YDZAFH). `None`
    /// when they could not be read or enumerated.
    pub claims: Option<Vec<HeldClaim>>,
    /// The namespaces `claims` covers.
    pub claim_namespaces: Vec<String>,
    /// HS2-2FG91X: set when a named namespace's backend could not be reached, with
    /// what its configuration says to do meanwhile. `claims` is then `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claims_offline: Option<OfflinePolicy>,
    /// HS2-KCEHVR: when only some named namespaces could be read, the claims of the
    /// ones that were. `claims` stays `None` and `claims_offline` the strictest policy,
    /// so a reader that knows neither member still treats every namespace as unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claims_readable: Option<Vec<HeldClaim>>,
    /// HS2-KCEHVR: each named namespace that could not be read, with its own policy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims_unreadable: Vec<UnreadableNamespace>,
    /// HS2-JSF985: the hosted project's own speculation setting, reported only when
    /// the project opts in. Absent, the project has not opted in, its machine does
    /// not say (an older peer) or the setting could not be read: never allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speculation: Option<TargetSpeculation>,
    /// HS2-P5GTSW: changes exactly when what admission and scheduling read changes,
    /// at the granularity of [`capacity_version`]. A subscriber passes it back as
    /// `after` to wait for the next change. Absent from an older peer, which offers no
    /// stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// HS2-TK9ADC: the hosted project's coordination pool, as the scheduler's role
    /// sessions need it. Absent when it could not be read, or from an older peer: the
    /// scheduler then starts no role session there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<PoolState>,
    /// HS2-ZY4Z4G: what the machine is, for plan steps that declare requirements.
    /// Absent from an older peer, or when the host could not tell: a requirement then
    /// reads as unknown, never as met.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<HostCapabilities>,
}

/// HS2-TK9ADC: what a target's coordination pool says about its single-holder roles
/// and the work handed off to its integrator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolState {
    /// A dispatcher holds a live lease in the pool.
    pub dispatcher_live: bool,
    /// An integrator holds a live lease in the pool.
    pub integrator_live: bool,
    /// Assignments handed off and not yet done: in handoff, verifying, integrating,
    /// suspended integration or CI.
    pub handoffs_pending: u32,
}

/// HS2-P5GTSW: free memory is compared in steps of this many MiB.
pub const CAPACITY_MEMORY_STEP_MB: u64 = 1024;
/// HS2-P5GTSW: CPU load is compared in steps of this many percent.
pub const CAPACITY_LOAD_STEP_PERCENT: u32 = 20;
/// HS2-P5GTSW: free disk is compared in steps of this many MiB.
pub const CAPACITY_DISK_STEP_MB: u64 = 4096;

/// HS2-P5GTSW: a stable digest of what a snapshot says about room on the machine.
/// Measurements are compared in coarse steps, so ordinary jitter is not a change; the
/// sampling time is never part of it. Sessions (which run, their tool, on the
/// checkout; not whether one is mid-turn, which changes every turn and is not room),
/// tools, claims, the offline policy and the speculation setting are compared
/// exactly, in an order of their own. The same capacity always gives the same
/// version, also after a restart.
pub fn capacity_version(snapshot: &CapacitySnapshot) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    let step = |value: Option<u64>, by: u64| value.map_or("?".to_owned(), |v| (v / by).to_string());
    let _ = write!(
        text,
        "m{}/{};c{}/{};d{}/{};",
        step(snapshot.memory.free_mb, CAPACITY_MEMORY_STEP_MB),
        step(snapshot.memory.total_mb, CAPACITY_MEMORY_STEP_MB),
        step(
            snapshot.cpu.load_percent.map(u64::from),
            u64::from(CAPACITY_LOAD_STEP_PERCENT)
        ),
        step(snapshot.cpu.logical_cpus.map(u64::from), 1),
        step(snapshot.disk.free_mb, CAPACITY_DISK_STEP_MB),
        step(snapshot.disk.total_mb, CAPACITY_DISK_STEP_MB),
    );
    match &snapshot.tools {
        None => text.push_str("t?;"),
        Some(tools) => {
            let mut tools: Vec<String> = tools
                .iter()
                .map(|tool| {
                    format!(
                        "{}:{}{}",
                        tool.tool,
                        u8::from(tool.installed),
                        u8::from(tool.configured)
                    )
                })
                .collect();
            tools.sort();
            let _ = write!(text, "t{};", tools.join(","));
        }
    }
    match &snapshot.sessions {
        None => text.push_str("s?;"),
        Some(sessions) => {
            let mut sessions: Vec<String> = sessions
                .iter()
                .map(|session| {
                    format!(
                        "{}:{}:{:?}",
                        session.session_id,
                        session.tool.as_deref().unwrap_or("?"),
                        session.on_checkout
                    )
                })
                .collect();
            sessions.sort();
            let _ = write!(text, "s{};", sessions.join(","));
        }
    }
    match &snapshot.claims {
        None => text.push_str("h?;"),
        Some(claims) => {
            let mut claims: Vec<String> = claims
                .iter()
                .map(|claim| format!("{}/{}", claim.namespace, claim.resource))
                .collect();
            claims.sort();
            let _ = write!(text, "h{};", claims.join(","));
        }
    }
    let _ = write!(
        text,
        "o{:?};p{:?}",
        snapshot.claims_offline, snapshot.speculation
    );
    // HS2-KCEHVR: only when reported, so a snapshot without it keeps its version.
    if !snapshot.claims_unreadable.is_empty() {
        let mut readable: Vec<String> = snapshot
            .claims_readable
            .iter()
            .flatten()
            .map(|claim| format!("{}/{}", claim.namespace, claim.resource))
            .collect();
        readable.sort();
        let unreadable: Vec<String> = snapshot
            .claims_unreadable
            .iter()
            .map(|entry| format!("{}={:?}", entry.namespace, entry.offline))
            .collect();
        let _ = write!(text, ";u{};{}", unreadable.join(","), readable.join(","));
    }
    // HS2-TK9ADC: only when reported, so a snapshot without it keeps its version.
    if let Some(pool) = snapshot.pool {
        let _ = write!(
            text,
            ";r{}/{}/{}",
            u8::from(pool.dispatcher_live),
            u8::from(pool.integrator_live),
            pool.handoffs_pending
        );
    }
    // HS2-ZY4Z4G: likewise only when reported. It changes when a tool or toolchain is
    // installed, removed or upgraded.
    if let Some(capabilities) = &snapshot.capabilities {
        let _ = write!(
            text,
            ";k{}",
            serde_json::to_string(capabilities).unwrap_or_default()
        );
    }
    fnv(&text)
}

/// FNV-1a: stable across processes and versions, unlike the std hasher.
fn fnv(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

impl TargetSpeculation {
    /// What a target that answered without the setting is taken to say.
    pub const NOT_OPTED_IN: Self = Self {
        enabled: false,
        max_depth: 1,
        integration: crate::provisional::IntegrationRule::PushPerTicket,
    };
}

impl CapacitySnapshot {
    pub fn new(
        server_id: String,
        checkout_id: String,
        host: HostObservation,
        claim_namespaces: Vec<String>,
        claims: Option<Vec<HeldClaim>>,
    ) -> Self {
        Self {
            schema_version: ORCHESTRATION_VERSION,
            server_id,
            checkout_id,
            sampled_at_ms: host.sampled_at_ms,
            memory: host.memory,
            cpu: host.cpu,
            disk: host.disk,
            tools: host.tools,
            sessions: host.sessions,
            claims,
            claim_namespaces,
            claims_offline: None,
            claims_readable: None,
            claims_unreadable: Vec::new(),
            speculation: None,
            version: None,
            pool: None,
            capabilities: host.capabilities,
        }
    }

    /// HS2-TK9ADC: say what the hosted project's coordination pool shows.
    #[must_use]
    pub fn with_pool(mut self, pool: Option<PoolState>) -> Self {
        self.pool = pool;
        self
    }

    /// HS2-P5GTSW: stamp the snapshot with its [`capacity_version`], after every
    /// other member is set.
    #[must_use]
    pub fn with_version(mut self) -> Self {
        self.version = None;
        self.version = Some(capacity_version(&self));
        self
    }

    /// HS2-AWB7BX: stamp the snapshot with a version that also changes when the
    /// machine owner's own admission limits change, since those decide room as much
    /// as the measurements do. The default (no limits) gives [`Self::with_version`].
    #[must_use]
    pub fn with_admission_version(self, admission: &AdmissionConfig) -> Self {
        let mut stamped = self.with_version();
        if *admission != AdmissionConfig::default() {
            let limits = serde_json::to_string(admission).unwrap_or_default();
            let base = stamped.version.take().unwrap_or_default();
            stamped.version = Some(fnv(&format!("{base};a{limits}")));
        }
        stamped
    }

    /// HS2-JSF985: say what the hosted project's own speculation setting is.
    #[must_use]
    pub fn with_speculation(mut self, speculation: Option<TargetSpeculation>) -> Self {
        self.speculation = speculation;
        self
    }

    /// HS2-2FG91X: the claims could not be read; say what the backend's offline
    /// policy is. Ignored while the claims are known.
    #[must_use]
    pub fn with_claims_offline(mut self, offline: Option<OfflinePolicy>) -> Self {
        self.claims_offline = offline.filter(|_| self.claims.is_none());
        self
    }

    /// HS2-KCEHVR: the claims could not all be read; say which namespaces could not,
    /// each with its own policy, and give the claims of the rest. Ignored while the
    /// claims are known or when no namespace is unreadable.
    #[must_use]
    pub fn with_partial_claims(
        mut self,
        readable: Vec<HeldClaim>,
        unreadable: Vec<UnreadableNamespace>,
    ) -> Self {
        if self.claims.is_none() && !unreadable.is_empty() {
            self.claims_readable = Some(readable);
            self.claims_unreadable = unreadable;
        }
        self
    }

    /// The host's own part, as a probe that relays another server's snapshot needs it.
    pub fn into_host(self) -> HostObservation {
        HostObservation {
            sampled_at_ms: self.sampled_at_ms,
            memory: self.memory,
            cpu: self.cpu,
            disk: self.disk,
            tools: self.tools,
            sessions: self.sessions,
            capabilities: self.capabilities,
        }
    }
}

/// Limits the machine's owner sets for sessions an orchestrator asks to start here.
/// Every field is optional; an unset limit leaves the orchestrator's policy value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_sessions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_free_ram_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cpu_percent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_free_disk_mb: Option<u64>,
    /// Admit on a measurement the host could not take. Off unless the owner says so.
    #[serde(default)]
    pub allow_unknown: bool,
    /// Estimate for a tool without its own entry; [`DEFAULT_SESSION_FOOTPRINT_MB`] when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_footprint_mb: Option<u64>,
    /// Estimated resident memory of one session of a tool, in megabytes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_footprint_mb: BTreeMap<String, u64>,
    /// HS2-ZY4Z4G: the toolchains this machine reports in its capabilities, replacing
    /// [`DEFAULT_TOOLCHAINS`](super::capabilities::DEFAULT_TOOLCHAINS). Not a limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchains: Option<Vec<String>>,
}

impl AdmissionConfig {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        if self.max_sessions.is_some_and(|n| n > MAX_POLICY_SESSIONS) {
            return Err(OrchestrationError::Invalid(
                "admission sessions must be 0 to 64",
            ));
        }
        if self
            .max_cpu_percent
            .is_some_and(|percent| !(1..=100).contains(&percent))
        {
            return Err(OrchestrationError::Invalid("CPU percent must be 1 to 100"));
        }
        if self.min_free_ram_mb.is_some_and(|mb| mb > 1024 * 1024)
            || self
                .min_free_disk_mb
                .is_some_and(|mb| mb > 1024 * 1024 * 1024)
        {
            return Err(OrchestrationError::Invalid("resource floors are megabytes"));
        }
        let footprint = |mb: &u64| (1..=MAX_SESSION_FOOTPRINT_MB).contains(mb);
        if !self.default_footprint_mb.as_ref().is_none_or(footprint)
            || !self.tool_footprint_mb.values().all(footprint)
        {
            return Err(OrchestrationError::Invalid(
                "a session footprint is 1 to 1048576 megabytes",
            ));
        }
        if self.tool_footprint_mb.len() > MAX_POLICY_TOOLS {
            return Err(OrchestrationError::Invalid(
                "at most 16 tool footprints may be configured",
            ));
        }
        if let Some(toolchains) = &self.toolchains {
            super::capabilities::validate_toolchains(toolchains)?;
        }
        self.tool_footprint_mb
            .keys()
            .try_for_each(|tool| valid_identifier(tool))
    }

    /// HS2-ZY4Z4G: the toolchains this machine reports: the owner's list, or the
    /// default one.
    pub fn declared_toolchains(&self) -> Vec<String> {
        self.toolchains.clone().unwrap_or_else(|| {
            super::capabilities::DEFAULT_TOOLCHAINS
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        })
    }
}

/// The limits actually enforced: for each one, the stricter of the orchestrator's
/// policy and the machine owner's file. The owner can never loosen the policy.
pub fn effective_limits(policy: &MachineLimits, local: &AdmissionConfig) -> MachineLimits {
    MachineLimits {
        max_sessions: local
            .max_sessions
            .map_or(policy.max_sessions, |own| own.min(policy.max_sessions)),
        min_free_ram_mb: local.min_free_ram_mb.map_or(policy.min_free_ram_mb, |own| {
            own.max(policy.min_free_ram_mb)
        }),
        max_cpu_percent: local.max_cpu_percent.map_or(policy.max_cpu_percent, |own| {
            own.min(policy.max_cpu_percent)
        }),
        min_free_disk_mb: local
            .min_free_disk_mb
            .map_or(policy.min_free_disk_mb, |own| {
                own.max(policy.min_free_disk_mb)
            }),
    }
}

/// "May `sessions` sessions of `tool` start on this checkout now?"
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRequest {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default = "one_session")]
    pub sessions: u32,
    /// The orchestrator's policy limits for this target's machine. Without them the
    /// policy defaults stand in, so a bare question is never looser than a policy's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<MachineLimits>,
}
fn one_session() -> u32 {
    1
}

impl AdmissionRequest {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        valid_identifier(&self.tool)?;
        if let Some(model) = &self.model
            && (!bounded_text(model, 128) || model.chars().any(char::is_whitespace))
        {
            return Err(OrchestrationError::Invalid("bounded model name required"));
        }
        if !(1..=MAX_POLICY_SESSIONS).contains(&self.sessions) {
            return Err(OrchestrationError::Invalid(
                "an admission asks for 1 to 64 sessions",
            ));
        }
        match self.limits.as_ref().and_then(MachineLimits::problem) {
            Some(problem) => Err(OrchestrationError::Invalid(problem)),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionOutcome {
    /// Every session asked for may start.
    Accepted,
    /// Fewer may start than asked: `admitted` says how many.
    Reduced,
    Refused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionReason {
    ToolNotInstalled,
    ToolNotConfigured,
    SessionCapReached,
    AboveCpuCeiling,
    BelowDiskFloor,
    BelowRamFloor,
    /// A needed measurement is unknown and the owner did not allow that.
    CapacityUnknown,
}

impl AdmissionReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::ToolNotInstalled => "tool_not_installed",
            Self::ToolNotConfigured => "tool_not_configured",
            Self::SessionCapReached => "session_cap_reached",
            Self::AboveCpuCeiling => "above_cpu_ceiling",
            Self::BelowDiskFloor => "below_disk_floor",
            Self::BelowRamFloor => "below_ram_floor",
            Self::CapacityUnknown => "capacity_unknown",
        }
    }
}

/// A part of the snapshot a decision needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityField {
    Tools,
    Sessions,
    Cpu,
    Disk,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FootprintSource {
    /// [`DEFAULT_SESSION_FOOTPRINT_MB`].
    Default,
    /// The machine owner's admission file.
    Configured,
    /// A running session of the tool uses more than the estimate.
    Observed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionDecision {
    pub outcome: AdmissionOutcome,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub requested: u32,
    /// How many of the requested sessions may start now.
    pub admitted: u32,
    /// The limit that refused or reduced the request; `None` when accepted.
    pub reason: Option<AdmissionReason>,
    /// Measurements this decision needed and the snapshot did not have.
    pub unknown: Vec<CapacityField>,
    /// The limits enforced, after the machine owner's file was applied.
    pub limits: MachineLimits,
    pub footprint_mb: u64,
    pub footprint_source: FootprintSource,
    pub sampled_at_ms: Option<u64>,
}

/// One session's estimated resident memory: the owner's estimate, raised to what a
/// running session of the same tool was actually observed to use.
pub fn estimated_footprint(
    config: &AdmissionConfig,
    sessions: Option<&[SessionFootprint]>,
    tool: &str,
) -> (u64, FootprintSource) {
    let (estimate, source) = match config
        .tool_footprint_mb
        .get(tool)
        .or(config.default_footprint_mb.as_ref())
    {
        Some(configured) => (*configured, FootprintSource::Configured),
        None => (DEFAULT_SESSION_FOOTPRINT_MB, FootprintSource::Default),
    };
    let observed = sessions
        .into_iter()
        .flatten()
        .filter(|session| session.tool.as_deref() == Some(tool))
        .filter_map(|session| session.resident_mb)
        .max();
    match observed {
        Some(observed) if observed > estimate => (observed, FootprintSource::Observed),
        _ => (estimate.max(1), source),
    }
}

/// Decide one admission. A limit a known measurement breaks refuses with that limit's
/// reason. Otherwise an unknown measurement refuses as `capacity_unknown` unless the
/// owner allowed unknowns, in which case that check is skipped. What remains is how
/// many sessions fit under the session cap and above the RAM floor.
pub fn decide_admission(
    snapshot: &CapacitySnapshot,
    config: &AdmissionConfig,
    request: &AdmissionRequest,
) -> AdmissionDecision {
    use AdmissionReason as R;
    let limits = effective_limits(&request.limits.unwrap_or_default(), config);
    let (footprint_mb, footprint_source) =
        estimated_footprint(config, snapshot.sessions.as_deref(), &request.tool);
    let mut unknown = Vec::new();
    let mut refusal = None;
    let mut refuse = |reason| {
        refusal.get_or_insert(reason);
    };
    match &snapshot.tools {
        None => unknown.push(CapacityField::Tools),
        Some(tools) => match tools.iter().find(|known| known.tool == request.tool) {
            Some(known) if !known.installed => refuse(R::ToolNotInstalled),
            Some(known) if !known.configured => refuse(R::ToolNotConfigured),
            Some(_) => {}
            None => refuse(R::ToolNotInstalled),
        },
    }
    // Each `room` is how many more sessions that limit leaves space for.
    let session_room = match &snapshot.sessions {
        None => {
            unknown.push(CapacityField::Sessions);
            None
        }
        Some(sessions) => {
            let running = u32::try_from(sessions.len()).unwrap_or(u32::MAX);
            let room = limits.max_sessions.saturating_sub(running);
            if room == 0 {
                refuse(R::SessionCapReached);
            }
            Some(room)
        }
    };
    match snapshot.cpu.load_percent {
        None => unknown.push(CapacityField::Cpu),
        Some(load) if load > limits.max_cpu_percent => refuse(R::AboveCpuCeiling),
        Some(_) => {}
    }
    match snapshot.disk.free_mb {
        None => unknown.push(CapacityField::Disk),
        Some(free) if free < limits.min_free_disk_mb => refuse(R::BelowDiskFloor),
        Some(_) => {}
    }
    let ram_room = match snapshot.memory.free_mb {
        None => {
            unknown.push(CapacityField::Memory);
            None
        }
        Some(free) => {
            let room = free.saturating_sub(limits.min_free_ram_mb) / footprint_mb;
            if room == 0 {
                refuse(R::BelowRamFloor);
            }
            Some(u32::try_from(room).unwrap_or(u32::MAX))
        }
    };
    if !unknown.is_empty() && !config.allow_unknown {
        refuse(R::CapacityUnknown);
    }
    let (outcome, admitted, reason) = match refusal {
        Some(reason) => (AdmissionOutcome::Refused, 0, Some(reason)),
        None => {
            // The tighter room names the reason; the session cap wins a tie.
            let (room, reason) = match (session_room, ram_room) {
                (Some(sessions), Some(ram)) if ram < sessions => (ram, R::BelowRamFloor),
                (Some(sessions), _) => (sessions, R::SessionCapReached),
                (None, Some(ram)) => (ram, R::BelowRamFloor),
                (None, None) => (u32::MAX, R::CapacityUnknown),
            };
            if room >= request.sessions {
                (AdmissionOutcome::Accepted, request.sessions, None)
            } else {
                (AdmissionOutcome::Reduced, room, Some(reason))
            }
        }
    };
    AdmissionDecision {
        outcome,
        tool: request.tool.clone(),
        model: request.model.clone(),
        requested: request.sessions,
        admitted,
        reason,
        unknown,
        limits,
        footprint_mb,
        footprint_source,
        sampled_at_ms: snapshot.sampled_at_ms,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionDocument {
    schema_version: u32,
    admission: AdmissionConfig,
}

/// The machine's admission file as a caller sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionView {
    /// Whether the file exists; an absent file is all defaults.
    pub configured: bool,
    pub admission: AdmissionConfig,
    /// The footprint estimate that applies when none is configured.
    pub default_footprint_mb: u64,
}

/// `<HOTSHEET_HOME>/orchestration-admission.json`: private to the machine's owner.
#[derive(Clone)]
pub struct AdmissionStore {
    storage: Storage,
}

impl AdmissionStore {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-admission.json"),
        }
    }

    fn document(&self) -> Result<Option<AdmissionConfig>, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_ADMISSION_BYTES)? else {
            return Ok(None);
        };
        let document: AdmissionDocument = decode(&bytes)?;
        // A stored file that no longer validates must not be enforced as if it did.
        document
            .admission
            .validate()
            .map_err(|_| OrchestrationError::Corrupt)?;
        Ok(Some(document.admission))
    }

    fn view(config: Option<AdmissionConfig>) -> AdmissionView {
        AdmissionView {
            configured: config.is_some(),
            admission: config.unwrap_or_default(),
            default_footprint_mb: DEFAULT_SESSION_FOOTPRINT_MB,
        }
    }

    /// An absent file is the default configuration and is never created by a read.
    /// A malformed one is an error, so a damaged file can never loosen admission.
    pub fn read(&self) -> Result<AdmissionView, OrchestrationError> {
        self.document().map(Self::view)
    }

    /// Change the configuration under the file's lock. Unchanged content writes
    /// nothing, so repeating a setup command is a no-op.
    pub fn update(
        &self,
        change: impl FnOnce(&mut AdmissionConfig),
    ) -> Result<AdmissionView, OrchestrationError> {
        let _lock = self.storage.lock()?;
        let previous = self.document()?;
        let mut next = previous.clone().unwrap_or_default();
        change(&mut next);
        next.validate()?;
        if previous.as_ref() != Some(&next) {
            let mut bytes = serde_json::to_vec_pretty(&AdmissionDocument {
                schema_version: ORCHESTRATION_VERSION,
                admission: next.clone(),
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            bytes.push(b'\n');
            self.storage.write(&bytes, MAX_ADMISSION_BYTES)?;
        }
        Ok(Self::view(Some(next)))
    }
}
