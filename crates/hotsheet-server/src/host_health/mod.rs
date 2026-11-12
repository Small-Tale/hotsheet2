//! Host health sampler (HS2-DTAVQA, `docs/host-health.md`): the server samples
//! **its own host** on a server-local cadence and pushes threshold crossings and coarse
//! snapshot notices over the existing change bus, so no client ever polls.
//!
//! - [`source`] reads the host (an injected [`source::HostSource`]; tests script one).
//! - [`Monitor`] is the pure evaluator: projections, sustained-CPU tracking, and hysteresis
//!   (escalate at once, recover only past a margin and after [`RECOVERY_SAMPLES`] samples).
//! - [`run_sampler`] is the server task: sample, evaluate, emit `host_health` change events.
//! - `routes` serves `GET /host-health` and `GET`/`PUT /host-health/thresholds`.
//! - [`live_cpu`] serves the live CPU long poll, `GET /host-health/cpu` (HS2-CPFBWP).
//!
//! Change events carry `kind: "host_health"`, `store: ""`, `id` = the signal (`disk`, `memory`,
//! `cpu`, `power`, `network`, `thermal`) or `snapshot`, `slug` = the new status, and `message` =
//! the reason code. They are cursored, so `/ws/poll?since=` replays them. A client reads the full
//! snapshot with `GET /host-health` on each event and on resync. Every value is a metric or a
//! code; the only name reported is the busiest process's executable name.

pub(crate) mod live_cpu;
pub(crate) mod routes;
pub(crate) mod source;
#[cfg(test)]
mod tests;

use crate::AppState;
use crate::events::ChangeEvent;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::host_health::HostHealthThresholds;
use hotsheet_ticketing::host_health_disks::{DiskChoices, STORE_VOLUME_ID};
use serde::Serialize;
use source::{HostReading, HostSource, Probe, VolumeReading};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

/// A condition's severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Level {
    Normal,
    Warning,
    Critical,
}

/// One signal's reported status. `unsupported` (no reader on this platform) and `unavailable`
/// (the reader failed) never produce a callout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Normal,
    Warning,
    Critical,
    Unsupported,
    Unavailable,
}

impl Status {
    fn severity(self) -> u8 {
        match self {
            Self::Critical => 2,
            Self::Warning => 1,
            Self::Normal | Self::Unsupported | Self::Unavailable => 0,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warning => "warning",
            Self::Critical => "critical",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
        }
    }
}

impl From<Level> for Status {
    fn from(level: Level) -> Self {
        match level {
            Level::Normal => Self::Normal,
            Level::Warning => Self::Warning,
            Level::Critical => Self::Critical,
        }
    }
}

/// Consecutive lower samples a signal needs before its callout clears (no flapping).
pub(crate) const RECOVERY_SAMPLES: u32 = 3;
/// The window swap growth is fitted over, and the shortest span a rate needs.
const RATE_WINDOW_MS: u64 = 60 * 60 * 1000;
const RATE_MIN_SPAN_MS: u64 = 10 * 60 * 1000;
const RATE_MAX_POINTS: usize = 400;
/// Disk consumption uses a longer, robust trend (HS2-Y2HGY9): the median pairwise slope
/// (Theil–Sen) over up to six hours, so one build's burst or an idle stretch does not swing the
/// time to full. Points closer than a minute replace each other, which bounds the fit at 360
/// points whatever `sample_seconds` is.
const DISK_TREND_WINDOW_MS: u64 = 6 * 60 * 60 * 1000;
const DISK_POINT_SPACING_MS: u64 = 60 * 1000;
/// A sustained fast fill must still call out promptly: the recent rate, the smaller of the
/// last hour's and last 20 minutes' robust slopes, replaces the long trend when it projects the
/// disk full sooner than the critical line.
const DISK_RECENT_WINDOW_MS: u64 = 60 * 60 * 1000;
const DISK_SHORT_WINDOW_MS: u64 = 20 * 60 * 1000;
/// Coarse snapshot notices go out at most this often, and only when the coarse values moved.
pub(crate) const SNAPSHOT_MIN_INTERVAL_MS: u64 = 5 * 60 * 1000;
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
/// The CPU sparkline's window (HS2-RBFR94): twelve five-minute averages, the last hour.
pub(crate) const CPU_HISTORY_BUCKETS: usize = 12;
pub(crate) const CPU_HISTORY_BUCKET_MS: u64 = 5 * 60 * 1000;
const MIB: f64 = 1024.0 * 1024.0;

/// Environment switch that keeps the sampler off; test fixtures set it to `off` (HS2-DTAVQA).
pub(crate) const SAMPLER_ENV: &str = "HOTSHEET_HOST_HEALTH_SAMPLER";

/// The signals, in wire order.
pub(crate) const SIGNALS: [&str; 6] = ["disk", "memory", "cpu", "power", "network", "thermal"];

/// The server's host, so remote clients (a phone) label the stats correctly.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct HostIdentity {
    pub(crate) name: String,
    pub(crate) os: &'static str,
    pub(crate) arch: &'static str,
}

impl HostIdentity {
    pub(crate) fn current() -> Self {
        Self {
            name: host_name().unwrap_or_else(|| "unknown host".into()),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }
}

#[cfg(unix)]
fn host_name() -> Option<String> {
    let mut buffer = [0_u8; 256];
    // SAFETY: the buffer is writable for its full length, which is what we pass.
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0)?;
    let name = String::from_utf8_lossy(&buffer[..end]).trim().to_string();
    // Show `studio` rather than `studio.local`.
    let short = name.strip_suffix(".local").unwrap_or(&name).to_string();
    (!short.is_empty()).then_some(short)
}

#[cfg(not(unix))]
fn host_name() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|name| !name.is_empty())
}

/// The status block every signal shares.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct SignalHeader {
    pub(crate) status: Status,
    /// A stable code for why the signal is not normal (`low_free_space`, `on_battery`, …).
    pub(crate) reason: Option<&'static str>,
    /// When the current status began (RFC 3339).
    pub(crate) since: Option<String>,
}

impl SignalHeader {
    fn initial() -> Self {
        Self {
            status: Status::Unavailable,
            reason: None,
            since: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct DiskSignal {
    #[serde(flatten)]
    pub(crate) header: SignalHeader,
    pub(crate) free_bytes: Option<u64>,
    pub(crate) total_bytes: Option<u64>,
    /// Robust consumption trend over up to six hours (HS2-Y2HGY9); `0` when free space is
    /// steady or growing.
    pub(crate) consumption_bytes_per_hour: Option<u64>,
    /// Projected hours until the volume is full at that rate.
    pub(crate) hours_to_full: Option<f64>,
    /// Which monitored volume these values describe: the worst one, else the store's (HS2-NHK8V8).
    pub(crate) volume_id: Option<String>,
    pub(crate) volume_label: Option<String>,
}

/// The values one volume reports.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct DiskNumbers {
    free_bytes: Option<u64>,
    total_bytes: Option<u64>,
    consumption_bytes_per_hour: Option<u64>,
    hours_to_full: Option<f64>,
}

/// Whether a detected volume counts (HS2-NHK8V8): the user's saved choice, or `pending` until made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiskDecision {
    Included,
    Excluded,
    Pending,
}

/// One volume host health knows about (HS2-NHK8V8): the store's, each detected or added one, and
/// each saved choice whose volume is absent right now.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct DiskVolume {
    /// `store` for the store's volume, else a stable volume id (`uuid:…` or `mount:…`).
    pub(crate) id: String,
    pub(crate) label: String,
    /// Where the volume is (or was last) mounted; `null` for the store's volume.
    pub(crate) mount: Option<String>,
    /// The volume holding the primary store; always monitored.
    pub(crate) store: bool,
    /// Added by path rather than detected (HS2-KBVZV1).
    pub(crate) manual: bool,
    pub(crate) decision: DiskDecision,
    /// Mounted now; an absent volume keeps its choice and reports no values.
    pub(crate) present: bool,
    /// Its committed status while monitored and present; `null` otherwise.
    pub(crate) status: Option<Status>,
    pub(crate) reason: Option<&'static str>,
    pub(crate) free_bytes: Option<u64>,
    pub(crate) total_bytes: Option<u64>,
    pub(crate) consumption_bytes_per_hour: Option<u64>,
    pub(crate) hours_to_full: Option<f64>,
}

/// An included volume's trend and hysteresis.
#[derive(Debug, Default)]
struct VolumeTrack {
    history: VecDeque<(u64, f64)>,
    hold: Hold,
    reason: Option<&'static str>,
    numbers: DiskNumbers,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct MemorySignal {
    #[serde(flatten)]
    pub(crate) header: SignalHeader,
    pub(crate) pressure: Option<Level>,
    pub(crate) swap_used_bytes: Option<u64>,
    pub(crate) swap_growth_bytes_per_hour: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct CpuSignal {
    #[serde(flatten)]
    pub(crate) header: SignalHeader,
    pub(crate) busy_percent: Option<f64>,
    /// How long the machine has stayed at or above the busy threshold, in seconds.
    pub(crate) busy_for_seconds: Option<u64>,
    pub(crate) top_process: Option<String>,
    pub(crate) top_process_percent: Option<f64>,
    /// Busy-percent averages over the last hour (HS2-RBFR94).
    pub(crate) history: CpuHistory,
}

/// Bounded, server-local CPU history (HS2-RBFR94): [`CPU_HISTORY_BUCKETS`] dense buckets of
/// [`CPU_HISTORY_BUCKET_MS`], aligned to the epoch so every client agrees on the boundaries, oldest
/// first and ending with the bucket that holds the newest sample. A bucket with no sample (the
/// server was down, or the reader failed) is `null`, never a guessed value.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct CpuHistory {
    pub(crate) bucket_seconds: u64,
    /// The end of the newest bucket (RFC 3339); `null` before the first CPU sample.
    pub(crate) ends_at: Option<String>,
    pub(crate) average_percent: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct PowerSignal {
    #[serde(flatten)]
    pub(crate) header: SignalHeader,
    pub(crate) on_battery: Option<bool>,
    pub(crate) battery_percent: Option<u64>,
    pub(crate) lid_closed: Option<bool>,
    pub(crate) sleep_prevented: Option<bool>,
    pub(crate) idle_sleep_minutes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct NetworkSignal {
    #[serde(flatten)]
    pub(crate) header: SignalHeader,
    pub(crate) reachable: Option<bool>,
    pub(crate) interface: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct ThermalSignal {
    #[serde(flatten)]
    pub(crate) header: SignalHeader,
    pub(crate) throttled: Option<bool>,
    pub(crate) cpu_speed_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct Signals {
    pub(crate) disk: DiskSignal,
    pub(crate) memory: MemorySignal,
    pub(crate) cpu: CpuSignal,
    pub(crate) power: PowerSignal,
    pub(crate) network: NetworkSignal,
    pub(crate) thermal: ThermalSignal,
}

impl Signals {
    fn header_mut(&mut self, id: &str) -> &mut SignalHeader {
        match id {
            "disk" => &mut self.disk.header,
            "memory" => &mut self.memory.header,
            "cpu" => &mut self.cpu.header,
            "power" => &mut self.power.header,
            "network" => &mut self.network.header,
            _ => &mut self.thermal.header,
        }
    }

    fn headers(&self) -> [&SignalHeader; 6] {
        [
            &self.disk.header,
            &self.memory.header,
            &self.cpu.header,
            &self.power.header,
            &self.network.header,
            &self.thermal.header,
        ]
    }
}

/// `GET /host-health`: the latest evaluated sample of the server's host.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct HostHealthSnapshot {
    pub(crate) host: HostIdentity,
    /// When the sample was taken (RFC 3339); `null` before the first sample.
    pub(crate) sampled_at: Option<String>,
    /// The worst callout level; `normal` is the dark-cockpit "all normal" line.
    pub(crate) level: Level,
    pub(crate) signals: Signals,
    /// Every volume and its choice (HS2-NHK8V8), store first.
    pub(crate) disks: Vec<DiskVolume>,
}

/// One change event the sampler emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Emission {
    pub(crate) id: &'static str,
    pub(crate) status: &'static str,
    pub(crate) reason: Option<&'static str>,
}

/// Per-signal hysteresis state.
#[derive(Debug, Clone, Copy, Default)]
struct Hold {
    committed: Option<Status>,
    recovering: u32,
}

/// The pure evaluator: feed it readings with their time, read the snapshot back.
#[derive(Debug)]
pub(crate) struct Monitor {
    snapshot: HostHealthSnapshot,
    holds: [Hold; 6],
    disk_history: VecDeque<(u64, f64)>,
    /// The store volume's committed reason and values; the disk signal shows them unless another
    /// monitored volume is worse (HS2-NHK8V8).
    store_reason: Option<&'static str>,
    store_numbers: DiskNumbers,
    disk_choices: DiskChoices,
    volumes: Vec<VolumeReading>,
    volume_tracks: BTreeMap<String, VolumeTrack>,
    /// The combined disk signal's committed status.
    disk_signal: Option<Status>,
    disks_fingerprint: Option<String>,
    swap_history: VecDeque<(u64, f64)>,
    cpu_busy_since: Option<u64>,
    /// `(bucket start ms, sum of busy percents, sample count)`, oldest first (HS2-RBFR94).
    cpu_buckets: VecDeque<(u64, f64, u32)>,
    last_snapshot_notice: Option<(u64, String)>,
}

fn rfc3339(ms: u64) -> Option<String> {
    let nanos = i128::from(ms) * 1_000_000;
    time::OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .ok()?
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

/// Append a point and drop those outside the window.
fn push_point(history: &mut VecDeque<(u64, f64)>, now: u64, value: f64) {
    history.push_back((now, value));
    while history
        .front()
        .is_some_and(|(at, _)| now.saturating_sub(*at) > RATE_WINDOW_MS)
        || history.len() > RATE_MAX_POINTS
    {
        history.pop_front();
    }
}

/// Append a disk point, replacing the newest one while the point before it is no more than
/// [`DISK_POINT_SPACING_MS`] old, and drop points outside [`DISK_TREND_WINDOW_MS`].
fn push_disk_point(history: &mut VecDeque<(u64, f64)>, now: u64, value: f64) {
    if history.len() > 1
        && history
            .get(history.len() - 2)
            .is_some_and(|(before, _)| now.saturating_sub(*before) <= DISK_POINT_SPACING_MS)
    {
        history.pop_back();
    }
    history.push_back((now, value));
    while history
        .front()
        .is_some_and(|(at, _)| now.saturating_sub(*at) > DISK_TREND_WINDOW_MS)
    {
        history.pop_front();
    }
}

/// Theil–Sen slope in units per hour over the points no older than `window_ms` before the
/// newest: the median of every pairwise slope. A burst that is written and then deleted, or
/// one noisy sample, moves only the few pairs that touch it, so the median stays on the
/// trend. `None` until the points span [`RATE_MIN_SPAN_MS`].
fn robust_slope_per_hour(history: &VecDeque<(u64, f64)>, window_ms: u64) -> Option<f64> {
    let newest = history.back()?.0;
    let points: Vec<(u64, f64)> = history
        .iter()
        .filter(|(at, _)| newest.saturating_sub(*at) <= window_ms)
        .copied()
        .collect();
    let first = points.first()?.0;
    if points.len() < 3 || newest.saturating_sub(first) < RATE_MIN_SPAN_MS {
        return None;
    }
    let mut slopes = Vec::with_capacity(points.len() * (points.len() - 1) / 2);
    for (index, (at, value)) in points.iter().enumerate() {
        for (later_at, later_value) in &points[index + 1..] {
            let hours = later_at.saturating_sub(*at) as f64 / 3_600_000.0;
            if hours > 0.0 {
                slopes.push((later_value - value) / hours);
            }
        }
    }
    if slopes.is_empty() {
        return None;
    }
    let (count, middle) = (slopes.len(), slopes.len() / 2);
    let (lower, upper, _) = slopes.select_nth_unstable_by(middle, f64::total_cmp);
    let upper = *upper;
    Some(if count % 2 == 0 {
        let below = lower.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        f64::midpoint(below, upper)
    } else {
        upper
    })
}

/// The disk consumption rate in bytes per hour (`0` when free space is steady or growing):
/// the six-hour robust trend, or the recent rate when that alone projects the volume full
/// sooner than `critical_hours` (a sustained fast fill).
fn disk_consumption(history: &VecDeque<(u64, f64)>, free: f64, critical_hours: u64) -> Option<f64> {
    let trend = robust_slope_per_hour(history, DISK_TREND_WINDOW_MS).map(|slope| (-slope).max(0.0));
    let recent = robust_slope_per_hour(history, DISK_RECENT_WINDOW_MS)
        .zip(robust_slope_per_hour(history, DISK_SHORT_WINDOW_MS))
        .map(|(hour, short)| (-hour).min(-short).max(0.0));
    match (trend, recent) {
        (Some(trend), Some(recent))
            if recent > trend && critical_hours > 0 && free / recent < critical_hours as f64 =>
        {
            Some(recent)
        }
        (trend, _) => trend,
    }
}

/// One volume's values and raw status from its trend (HS2-Y2HGY9), with the hysteresis margins
/// while a level is held. Shared by the store's volume and every other monitored one (HS2-NHK8V8).
fn disk_eval(
    history: &VecDeque<(u64, f64)>,
    free_bytes: u64,
    total_bytes: u64,
    thresholds: &HostHealthThresholds,
    critical_held: bool,
    warning_held: bool,
) -> (DiskNumbers, Status, Option<&'static str>) {
    let consumption = disk_consumption(
        history,
        free_bytes as f64,
        thresholds.disk_critical_hours_to_full,
    );
    let hours = consumption
        .filter(|rate| *rate > 0.0)
        .map(|rate| free_bytes as f64 / rate);
    let numbers = DiskNumbers {
        free_bytes: Some(free_bytes),
        total_bytes: Some(total_bytes),
        consumption_bytes_per_hour: consumption.map(|rate| rate as u64),
        hours_to_full: hours.map(|hours| (hours * 10.0).round() / 10.0),
    };
    let free_gib = free_bytes as f64 / GIB;
    let projected = |limit: u64, held: bool| {
        limit > 0 && hours.is_some_and(|hours| below(hours, limit as f64, held, 1.25))
    };
    let (status, reason) = if below(
        free_gib,
        thresholds.disk_critical_free_gib as f64,
        critical_held,
        1.1,
    ) {
        (Status::Critical, Some("low_free_space"))
    } else if projected(thresholds.disk_critical_hours_to_full, critical_held) {
        (Status::Critical, Some("projected_full"))
    } else if below(
        free_gib,
        thresholds.disk_warning_free_gib as f64,
        warning_held,
        1.1,
    ) {
        (Status::Warning, Some("low_free_space"))
    } else if projected(thresholds.disk_warning_hours_to_full, warning_held) {
        (Status::Warning, Some("projected_full"))
    } else {
        (Status::Normal, None)
    };
    (numbers, status, reason)
}

/// Least-squares slope in units per hour, once the points span [`RATE_MIN_SPAN_MS`].
fn slope_per_hour(history: &VecDeque<(u64, f64)>) -> Option<f64> {
    let (first, last) = (history.front()?, history.back()?);
    if history.len() < 3 || last.0.saturating_sub(first.0) < RATE_MIN_SPAN_MS {
        return None;
    }
    let n = history.len() as f64;
    let hours = |at: u64| (at - first.0) as f64 / 3_600_000.0;
    let mean_t = history.iter().map(|(at, _)| hours(*at)).sum::<f64>() / n;
    let mean_v = history.iter().map(|(_, value)| value).sum::<f64>() / n;
    let (mut numerator, mut denominator) = (0.0, 0.0);
    for (at, value) in history {
        let dt = hours(*at) - mean_t;
        numerator += dt * (value - mean_v);
        denominator += dt * dt;
    }
    (denominator > 0.0).then(|| numerator / denominator)
}

/// The status of a probe that carries no value.
fn missing<T>(probe: &Probe<T>) -> Status {
    match probe {
        Probe::Unsupported => Status::Unsupported,
        Probe::Unavailable | Probe::Value(_) => Status::Unavailable,
    }
}

/// Whether `value` is below `threshold`, widened by `margin` while the condition already holds.
fn below(value: f64, threshold: f64, holding: bool, margin: f64) -> bool {
    value
        < if holding {
            threshold * margin
        } else {
            threshold
        }
}

/// Apply hysteresis to one hold. Returns `Some(previous)` when the committed status changes.
fn commit_hold(hold: &mut Hold, raw: Status) -> Option<Option<Status>> {
    let Some(committed) = hold.committed else {
        hold.committed = Some(raw);
        hold.recovering = 0;
        return Some(None);
    };
    if raw == committed {
        hold.recovering = 0;
        return None;
    }
    let change = if raw.severity() > committed.severity() {
        true
    } else if raw.severity() == committed.severity() && committed.severity() == 0 {
        // normal / unsupported / unavailable trade places freely: none is a callout.
        true
    } else {
        hold.recovering += 1;
        hold.recovering >= RECOVERY_SAMPLES
    };
    if change {
        hold.committed = Some(raw);
        hold.recovering = 0;
        Some(Some(committed))
    } else {
        None
    }
}

impl Monitor {
    pub(crate) fn new(host: HostIdentity) -> Self {
        let blank = SignalHeader::initial;
        Self {
            snapshot: HostHealthSnapshot {
                host,
                sampled_at: None,
                level: Level::Normal,
                signals: Signals {
                    disk: DiskSignal {
                        header: blank(),
                        free_bytes: None,
                        total_bytes: None,
                        consumption_bytes_per_hour: None,
                        hours_to_full: None,
                        volume_id: None,
                        volume_label: None,
                    },
                    memory: MemorySignal {
                        header: blank(),
                        pressure: None,
                        swap_used_bytes: None,
                        swap_growth_bytes_per_hour: None,
                    },
                    cpu: CpuSignal {
                        header: blank(),
                        busy_percent: None,
                        busy_for_seconds: None,
                        top_process: None,
                        top_process_percent: None,
                        history: CpuHistory {
                            bucket_seconds: CPU_HISTORY_BUCKET_MS / 1000,
                            ends_at: None,
                            average_percent: vec![None; CPU_HISTORY_BUCKETS],
                        },
                    },
                    power: PowerSignal {
                        header: blank(),
                        on_battery: None,
                        battery_percent: None,
                        lid_closed: None,
                        sleep_prevented: None,
                        idle_sleep_minutes: None,
                    },
                    network: NetworkSignal {
                        header: blank(),
                        reachable: None,
                        interface: None,
                    },
                    thermal: ThermalSignal {
                        header: blank(),
                        throttled: None,
                        cpu_speed_limit: None,
                    },
                },
                disks: Vec::new(),
            },
            holds: [Hold::default(); 6],
            disk_history: VecDeque::new(),
            store_reason: None,
            store_numbers: DiskNumbers::default(),
            disk_choices: DiskChoices::new(),
            volumes: Vec::new(),
            volume_tracks: BTreeMap::new(),
            disk_signal: None,
            disks_fingerprint: None,
            swap_history: VecDeque::new(),
            cpu_busy_since: None,
            cpu_buckets: VecDeque::new(),
            last_snapshot_notice: None,
        }
    }

    pub(crate) fn snapshot(&self) -> &HostHealthSnapshot {
        &self.snapshot
    }

    fn holding(&self, index: usize, at_least: Status) -> bool {
        self.holds[index]
            .committed
            .is_some_and(|status| status.severity() >= at_least.severity())
    }

    /// Evaluate one reading taken at `now` (ms since the epoch) and return the events to emit.
    pub(crate) fn observe(
        &mut self,
        reading: HostReading,
        now: u64,
        thresholds: &HostHealthThresholds,
    ) -> Vec<Emission> {
        self.eval_volumes(now, thresholds);
        let raw = [
            self.eval_disk(reading.disk, now, thresholds),
            self.eval_memory(reading.memory, now, thresholds),
            self.eval_cpu(reading.cpu, now, thresholds),
            self.eval_power(reading.power, thresholds),
            self.eval_network(reading.network),
            self.eval_thermal(reading.thermal),
        ];
        self.snapshot.sampled_at = rfc3339(now);
        let mut emissions = Vec::new();
        for (index, (status, reason)) in raw.into_iter().enumerate() {
            let id = SIGNALS[index];
            let (status, reason, change) = if index == 0 {
                self.commit_disk(status, reason)
            } else {
                (status, reason, self.commit(index, status))
            };
            if let Some(previous) = change {
                let header = self.snapshot.signals.header_mut(id);
                header.status = status;
                header.since = rfc3339(now);
                header.reason = reason;
                // Only a change in the set of callouts is a crossing worth pushing; moves
                // among normal, unsupported and unavailable ride the snapshot notice.
                let was_callout = previous.is_some_and(|previous| previous.severity() > 0);
                if was_callout || status.severity() > 0 {
                    emissions.push(Emission {
                        id,
                        status: status.as_str(),
                        reason,
                    });
                }
            } else {
                let header = self.snapshot.signals.header_mut(id);
                // Same status held: keep the reason current while the callout stays up.
                if header.status == status {
                    header.reason = reason;
                }
            }
        }
        self.snapshot.level = self
            .snapshot
            .signals
            .headers()
            .iter()
            .filter_map(|header| match header.status {
                Status::Warning => Some(Level::Warning),
                Status::Critical => Some(Level::Critical),
                _ => None,
            })
            .max()
            .unwrap_or(Level::Normal);
        self.rebuild_disks();
        // A volume appeared, left, or awaits a choice: clients re-read the snapshot (HS2-NHK8V8).
        let disks = self.disks_fingerprint();
        if self
            .disks_fingerprint
            .replace(disks.clone())
            .is_some_and(|previous| previous != disks)
        {
            emissions.push(Emission {
                id: "disks",
                status: "changed",
                reason: None,
            });
        }
        let fingerprint = self.coarse_fingerprint();
        if emissions.is_empty() {
            let due = match &self.last_snapshot_notice {
                None => true,
                Some((at, previous)) => {
                    *previous != fingerprint && now.saturating_sub(*at) >= SNAPSHOT_MIN_INTERVAL_MS
                }
            };
            if due {
                emissions.push(Emission {
                    id: "snapshot",
                    status: match self.snapshot.level {
                        Level::Normal => "normal",
                        Level::Warning => "warning",
                        Level::Critical => "critical",
                    },
                    reason: None,
                });
                self.last_snapshot_notice = Some((now, fingerprint));
            }
        } else {
            // A crossing already makes clients re-read the snapshot.
            self.last_snapshot_notice = Some((now, fingerprint));
        }
        emissions
    }

    /// Apply hysteresis. Returns `Some(previous)` when the committed status changes.
    fn commit(&mut self, index: usize, raw: Status) -> Option<Option<Status>> {
        commit_hold(&mut self.holds[index], raw)
    }

    /// Commit the store volume's reading, then combine it with every other monitored volume
    /// (HS2-NHK8V8): the disk signal takes the worst committed volume (the store's on a tie) and
    /// shows its values. Each volume keeps its own trend and hysteresis, so combining adds no
    /// second recovery delay. Returns the combined status, its reason, and `Some(previous)` when
    /// the combined status changed.
    fn commit_disk(
        &mut self,
        raw: Status,
        reason: Option<&'static str>,
    ) -> (Status, Option<&'static str>, Option<Option<Status>>) {
        let store = match self.commit(0, raw) {
            Some(_) => {
                self.store_reason = reason;
                raw
            }
            None => {
                let committed = self.holds[0].committed.unwrap_or(raw);
                if committed == raw {
                    self.store_reason = reason;
                }
                committed
            }
        };
        let mut worst = (store, self.store_reason, None::<&str>);
        for (id, track) in &self.volume_tracks {
            if let Some(status) = track.hold.committed
                && status.severity() > worst.0.severity()
            {
                worst = (status, track.reason, Some(id.as_str()));
            }
        }
        let (status, reason, volume) = worst;
        let numbers = volume
            .and_then(|id| self.volume_tracks.get(id))
            .map_or(self.store_numbers, |track| track.numbers);
        let label = match volume {
            Some(id) => self
                .volumes
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.label.clone()),
            None => Some("Ticket store".to_string()),
        };
        let signal = &mut self.snapshot.signals.disk;
        signal.free_bytes = numbers.free_bytes;
        signal.total_bytes = numbers.total_bytes;
        signal.consumption_bytes_per_hour = numbers.consumption_bytes_per_hour;
        signal.hours_to_full = numbers.hours_to_full;
        signal.volume_id = Some(volume.unwrap_or(STORE_VOLUME_ID).to_string());
        signal.volume_label = label;
        let previous = self.disk_signal;
        if previous == Some(status) {
            (status, reason, None)
        } else {
            self.disk_signal = Some(status);
            (status, reason, Some(previous))
        }
    }

    /// Use the saved disk choices (HS2-NHK8V8). Returns whether they changed.
    pub(crate) fn set_disk_choices(&mut self, choices: DiskChoices) -> bool {
        if choices == self.disk_choices {
            return false;
        }
        self.disk_choices = choices;
        self.rebuild_disks();
        true
    }

    /// The volumes other than the store's that this sample saw (HS2-NHK8V8).
    pub(crate) fn set_volumes(&mut self, volumes: Vec<VolumeReading>) {
        self.volumes = volumes;
    }

    fn included(&self, id: &str) -> bool {
        self.disk_choices
            .get(id)
            .is_some_and(|choice| choice.monitor)
    }

    /// Track every present, included volume: its robust trend and its own hysteresis, with the
    /// same margins as the store's. A volume that left or was excluded drops its track.
    fn eval_volumes(&mut self, now: u64, thresholds: &HostHealthThresholds) {
        let included: Vec<VolumeReading> = self
            .volumes
            .iter()
            .filter(|volume| self.included(&volume.id))
            .cloned()
            .collect();
        self.volume_tracks
            .retain(|id, _| included.iter().any(|volume| volume.id == *id));
        for volume in included {
            let track = self.volume_tracks.entry(volume.id.clone()).or_default();
            push_disk_point(&mut track.history, now, volume.free_bytes as f64);
            let held = |at_least: Status| {
                track
                    .hold
                    .committed
                    .is_some_and(|status| status.severity() >= at_least.severity())
            };
            let (critical_held, warning_held) = (held(Status::Critical), held(Status::Warning));
            let (numbers, raw, reason) = disk_eval(
                &track.history,
                volume.free_bytes,
                volume.total_bytes,
                thresholds,
                critical_held,
                warning_held,
            );
            track.numbers = numbers;
            let changed = commit_hold(&mut track.hold, raw).is_some();
            if changed || track.hold.committed == Some(raw) {
                track.reason = reason;
            }
        }
    }

    /// Rebuild the snapshot's volume list from the store reading, the tracks, and the choices.
    fn rebuild_disks(&mut self) {
        let store_status = self.snapshot.signals.disk.header.status;
        let mut disks = vec![DiskVolume {
            id: STORE_VOLUME_ID.into(),
            label: "Ticket store".into(),
            mount: None,
            store: true,
            manual: false,
            decision: DiskDecision::Included,
            present: self.store_numbers.free_bytes.is_some(),
            status: self.holds[0].committed.or(Some(store_status)),
            reason: self.store_reason,
            free_bytes: self.store_numbers.free_bytes,
            total_bytes: self.store_numbers.total_bytes,
            consumption_bytes_per_hour: self.store_numbers.consumption_bytes_per_hour,
            hours_to_full: self.store_numbers.hours_to_full,
        }];
        for volume in &self.volumes {
            let choice = self.disk_choices.get(&volume.id);
            let decision = match choice {
                Some(choice) if choice.monitor => DiskDecision::Included,
                Some(_) => DiskDecision::Excluded,
                None => DiskDecision::Pending,
            };
            let track = self.volume_tracks.get(&volume.id);
            let numbers = track.map_or(
                DiskNumbers {
                    free_bytes: Some(volume.free_bytes),
                    total_bytes: Some(volume.total_bytes),
                    ..DiskNumbers::default()
                },
                |track| track.numbers,
            );
            disks.push(DiskVolume {
                id: volume.id.clone(),
                label: volume.label.clone(),
                mount: Some(volume.mount.clone()),
                store: false,
                manual: volume.manual || choice.is_some_and(|choice| choice.manual),
                decision,
                present: true,
                status: track.and_then(|track| track.hold.committed),
                reason: track.and_then(|track| track.reason),
                free_bytes: numbers.free_bytes,
                total_bytes: numbers.total_bytes,
                consumption_bytes_per_hour: numbers.consumption_bytes_per_hour,
                hours_to_full: numbers.hours_to_full,
            });
        }
        let mut absent: Vec<DiskVolume> = self
            .disk_choices
            .iter()
            .filter(|(id, _)| !self.volumes.iter().any(|volume| volume.id == **id))
            .map(|(id, choice)| DiskVolume {
                id: id.clone(),
                label: choice.label.clone(),
                mount: Some(choice.mount.clone()),
                store: false,
                manual: choice.manual,
                decision: if choice.monitor {
                    DiskDecision::Included
                } else {
                    DiskDecision::Excluded
                },
                present: false,
                status: None,
                reason: None,
                free_bytes: None,
                total_bytes: None,
                consumption_bytes_per_hour: None,
                hours_to_full: None,
            })
            .collect();
        absent.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.id.cmp(&b.id)));
        disks.extend(absent);
        self.snapshot.disks = disks;
    }

    /// Which volumes are present and which await a choice.
    fn disks_fingerprint(&self) -> String {
        let mut present: Vec<String> = self
            .volumes
            .iter()
            .map(|volume| {
                let decision = match self.disk_choices.get(&volume.id) {
                    Some(choice) if choice.monitor => "in",
                    Some(_) => "out",
                    None => "pending",
                };
                format!("{}={decision}", volume.id)
            })
            .collect();
        present.sort();
        present.join(",")
    }

    fn eval_disk(
        &mut self,
        probe: Probe<source::DiskReading>,
        now: u64,
        thresholds: &HostHealthThresholds,
    ) -> (Status, Option<&'static str>) {
        let disk = match probe {
            Probe::Value(disk) => disk,
            other => {
                // Never show a stale value beside an unsupported or failed reader.
                self.store_numbers = DiskNumbers::default();
                return (missing(&other), None);
            }
        };
        push_disk_point(&mut self.disk_history, now, disk.free_bytes as f64);
        let critical_held = self.holding(0, Status::Critical);
        let warning_held = self.holding(0, Status::Warning);
        let (numbers, status, reason) = disk_eval(
            &self.disk_history,
            disk.free_bytes,
            disk.total_bytes,
            thresholds,
            critical_held,
            warning_held,
        );
        self.store_numbers = numbers;
        (status, reason)
    }

    fn eval_memory(
        &mut self,
        probe: Probe<source::MemoryReading>,
        now: u64,
        thresholds: &HostHealthThresholds,
    ) -> (Status, Option<&'static str>) {
        let memory = match probe {
            Probe::Value(memory) => memory,
            other => {
                let signal = &mut self.snapshot.signals.memory;
                (signal.pressure, signal.swap_used_bytes) = (None, None);
                signal.swap_growth_bytes_per_hour = None;
                return (missing(&other), None);
            }
        };
        let growth = memory.swap_used_bytes.and_then(|used| {
            push_point(&mut self.swap_history, now, used as f64);
            slope_per_hour(&self.swap_history)
        });
        let held = self.holding(1, Status::Warning);
        let signal = &mut self.snapshot.signals.memory;
        signal.pressure = memory.pressure;
        signal.swap_used_bytes = memory.swap_used_bytes;
        signal.swap_growth_bytes_per_hour = growth.map(|growth| growth as i64);
        let limit = thresholds.swap_warning_growth_mib_per_hour as f64;
        let swapping =
            limit > 0.0 && growth.is_some_and(|growth| !below(growth / MIB, limit, held, 0.8));
        match memory.pressure {
            Some(Level::Critical) => (Status::Critical, Some("pressure")),
            Some(Level::Warning) => (Status::Warning, Some("pressure")),
            _ if swapping => (Status::Warning, Some("swap_growth")),
            _ => (Status::Normal, None),
        }
    }

    fn eval_cpu(
        &mut self,
        probe: Probe<source::CpuReading>,
        now: u64,
        thresholds: &HostHealthThresholds,
    ) -> (Status, Option<&'static str>) {
        let cpu = match probe {
            Probe::Value(cpu) => cpu,
            other => {
                self.cpu_busy_since = None;
                let signal = &mut self.snapshot.signals.cpu;
                (signal.busy_percent, signal.busy_for_seconds) = (None, None);
                (signal.top_process, signal.top_process_percent) = (None, None);
                return (missing(&other), None);
            }
        };
        let held = self.holding(2, Status::Warning);
        let limit = thresholds.cpu_warning_percent as f64;
        let busy = cpu.busy_percent >= if held { limit - 5.0 } else { limit };
        if busy {
            self.cpu_busy_since.get_or_insert(now);
        } else {
            self.cpu_busy_since = None;
        }
        let busy_for = self.cpu_busy_since.map(|since| now.saturating_sub(since));
        let history = self.record_cpu_history(now, cpu.busy_percent);
        let signal = &mut self.snapshot.signals.cpu;
        signal.history = history;
        signal.busy_percent = Some((cpu.busy_percent * 10.0).round() / 10.0);
        signal.busy_for_seconds = busy_for.map(|ms| ms / 1000);
        signal.top_process = cpu.top_process;
        signal.top_process_percent = cpu.top_process_percent;
        if busy_for.is_some_and(|ms| ms >= thresholds.cpu_sustain_minutes * 60_000) {
            (Status::Warning, Some("sustained_cpu"))
        } else {
            (Status::Normal, None)
        }
    }

    /// Add one busy reading to its five-minute bucket and rebuild the dense history. Buckets
    /// older than the window drop off; a gap (the server was down) stays `null`.
    fn record_cpu_history(&mut self, now: u64, busy_percent: f64) -> CpuHistory {
        let start = now - now % CPU_HISTORY_BUCKET_MS;
        // A wall clock stepped backwards would misplace every bucket; start the window over.
        if self
            .cpu_buckets
            .back()
            .is_some_and(|(at, _, _)| *at > start)
        {
            self.cpu_buckets.clear();
        }
        match self.cpu_buckets.back_mut() {
            Some((at, sum, count)) if *at == start => {
                *sum += busy_percent;
                *count += 1;
            }
            _ => self.cpu_buckets.push_back((start, busy_percent, 1)),
        }
        let window = CPU_HISTORY_BUCKET_MS * CPU_HISTORY_BUCKETS as u64;
        let oldest = (start + CPU_HISTORY_BUCKET_MS).saturating_sub(window);
        while self
            .cpu_buckets
            .front()
            .is_some_and(|(at, _, _)| *at < oldest)
        {
            self.cpu_buckets.pop_front();
        }
        let mut average_percent = vec![None; CPU_HISTORY_BUCKETS];
        for (at, sum, count) in &self.cpu_buckets {
            let index = (at.saturating_sub(oldest) / CPU_HISTORY_BUCKET_MS) as usize;
            if let Some(slot) = average_percent.get_mut(index) {
                *slot = Some((sum / f64::from(*count) * 10.0).round() / 10.0);
            }
        }
        CpuHistory {
            bucket_seconds: CPU_HISTORY_BUCKET_MS / 1000,
            ends_at: rfc3339(start + CPU_HISTORY_BUCKET_MS),
            average_percent,
        }
    }

    fn eval_power(
        &mut self,
        probe: Probe<source::PowerReading>,
        thresholds: &HostHealthThresholds,
    ) -> (Status, Option<&'static str>) {
        let power = match probe {
            Probe::Value(power) => power,
            other => {
                let signal = &mut self.snapshot.signals.power;
                (signal.on_battery, signal.battery_percent) = (None, None);
                (signal.lid_closed, signal.sleep_prevented) = (None, None);
                signal.idle_sleep_minutes = None;
                return (missing(&other), None);
            }
        };
        let critical_held = self.holding(3, Status::Critical);
        let signal = &mut self.snapshot.signals.power;
        signal.on_battery = power.on_battery;
        signal.battery_percent = power.battery_percent;
        signal.lid_closed = power.lid_closed;
        signal.sleep_prevented = power.sleep_prevented;
        signal.idle_sleep_minutes = power.idle_sleep_minutes;
        let on_battery = power.on_battery == Some(true);
        let low_limit = thresholds.battery_critical_percent + if critical_held { 5 } else { 0 };
        if on_battery && power.lid_closed == Some(true) {
            (Status::Critical, Some("lid_closed_on_battery"))
        } else if on_battery
            && power
                .battery_percent
                .is_some_and(|percent| percent <= low_limit)
        {
            (Status::Critical, Some("battery_low"))
        } else if on_battery {
            (Status::Warning, Some("on_battery"))
        } else if power.idle_sleep_minutes.is_some_and(|minutes| minutes > 0)
            && power.sleep_prevented == Some(false)
        {
            (Status::Warning, Some("idle_sleep_allowed"))
        } else {
            (Status::Normal, None)
        }
    }

    fn eval_network(
        &mut self,
        probe: Probe<source::NetworkReading>,
    ) -> (Status, Option<&'static str>) {
        let network = match probe {
            Probe::Value(network) => network,
            other => {
                let signal = &mut self.snapshot.signals.network;
                (signal.reachable, signal.interface) = (None, None);
                return (missing(&other), None);
            }
        };
        let signal = &mut self.snapshot.signals.network;
        signal.reachable = Some(network.reachable);
        signal.interface = network.interface;
        if network.reachable {
            (Status::Normal, None)
        } else {
            (Status::Critical, Some("no_default_route"))
        }
    }

    fn eval_thermal(
        &mut self,
        probe: Probe<source::ThermalReading>,
    ) -> (Status, Option<&'static str>) {
        let thermal = match probe {
            Probe::Value(thermal) => thermal,
            other => {
                let signal = &mut self.snapshot.signals.thermal;
                (signal.throttled, signal.cpu_speed_limit) = (None, None);
                return (missing(&other), None);
            }
        };
        let signal = &mut self.snapshot.signals.thermal;
        signal.throttled = Some(thermal.throttled);
        signal.cpu_speed_limit = thermal.cpu_speed_limit;
        if thermal.throttled {
            (Status::Warning, Some("throttled"))
        } else {
            (Status::Normal, None)
        }
    }

    /// Rounded values whose change is worth a coarse snapshot notice.
    fn coarse_fingerprint(&self) -> String {
        let signals = &self.snapshot.signals;
        let round =
            |value: Option<f64>, step: f64| value.map(|value| (value / step).round() as i64);
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            signals
                .disk
                .free_bytes
                .map(|free| (free as f64 / GIB).round() as i64),
            round(signals.disk.hours_to_full, 1.0),
            signals
                .memory
                .swap_used_bytes
                .map(|used| (used as f64 / (256.0 * MIB)).round() as i64),
            round(signals.cpu.busy_percent, 10.0),
            signals.power.on_battery,
            signals.power.battery_percent.map(|percent| percent / 5),
            signals.network.reachable,
        )
    }
}

/// The shared monitor plus the sampler's run flag.
#[derive(Debug)]
pub(crate) struct HostHealth {
    pub(crate) monitor: Mutex<Monitor>,
    /// The live CPU feed (HS2-CPFBWP).
    pub(crate) live_cpu: live_cpu::LiveCpu,
    started: std::sync::atomic::AtomicBool,
}

impl Default for HostHealth {
    fn default() -> Self {
        Self {
            monitor: Mutex::new(Monitor::new(HostIdentity::current())),
            live_cpu: live_cpu::LiveCpu::default(),
            started: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// The machine-wide thresholds; an unreadable settings file falls back to the defaults.
pub(crate) fn thresholds(state: &AppState) -> HostHealthThresholds {
    routes::read_thresholds(state).map_or_else(
        |error| {
            tracing::warn!(error = %error.message, "host health thresholds unreadable; using defaults");
            hotsheet_ticketing::host_health::default_thresholds()
        },
        |view| view.effective,
    )
}

/// The saved disk choices (HS2-NHK8V8); an unreadable settings file reads as none.
pub(crate) fn disk_choices(state: &AppState) -> hotsheet_ticketing::host_health_disks::DiskChoices {
    routes::read_disk_choices(state).unwrap_or_else(|error| {
        tracing::warn!(error = %error.message, "host health disk choices unreadable");
        hotsheet_ticketing::host_health_disks::DiskChoices::new()
    })
}

/// Publish one `host_health` change event.
pub(crate) fn emit_event(state: &AppState, emission: &Emission) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "host_health".into(),
        id: emission.id.into(),
        slug: emission.status.into(),
        message: emission.reason.map(str::to_owned),
        activity: None,
        assignment: None,
        turn: None,
        live: None,
    });
}

/// Evaluate one reading and publish its events on the change bus.
pub(crate) fn record(
    state: &AppState,
    reading: HostReading,
    now: u64,
    thresholds: &HostHealthThresholds,
) {
    let emissions = state
        .host_health
        .monitor
        .with_lock(|monitor| monitor.observe(reading, now, thresholds));
    for emission in &emissions {
        emit_event(state, emission);
    }
}

/// The server task: sample on the server-local cadence until the server stops. It issues no
/// network requests; only local tools and files are read.
pub(crate) async fn run_sampler(state: AppState, source: Box<dyn HostSource>) {
    let source = Arc::new(Mutex::new(source));
    loop {
        let reading_state = state.clone();
        let reading_source = source.clone();
        let Ok((reading, thresholds, choices, volumes)) = tokio::task::spawn_blocking(move || {
            let thresholds = thresholds(&reading_state);
            let choices = disk_choices(&reading_state);
            let added: Vec<(String, std::path::PathBuf)> = choices
                .iter()
                .filter(|(_, choice)| choice.manual)
                .map(|(id, choice)| (id.clone(), std::path::PathBuf::from(&choice.mount)))
                .collect();
            let (reading, volumes) =
                reading_source.with_lock(|source| (source.sample(), source.volumes(&added)));
            (reading, thresholds, choices, volumes)
        })
        .await
        else {
            return;
        };
        state.host_health.monitor.with_lock(|monitor| {
            monitor.set_disk_choices(choices);
            monitor.set_volumes(volumes);
        });
        record(&state, reading, crate::client_drive::now_ms(), &thresholds);
        let wait = std::time::Duration::from_secs(thresholds.sample_seconds);
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = state.stopping() => return,
        }
    }
}

impl AppState {
    /// Start the host health sampler over the primary store's volume (HS2-DTAVQA). Call once,
    /// after the runtime is up and this process is the serving instance; repeats are no-ops.
    /// `HOTSHEET_HOST_HEALTH_SAMPLER=off` skips it: test fixtures force that, like
    /// `HOTSHEET_AI_TOOL_PROBES`, so host-dependent events never reach their change streams.
    pub fn spawn_host_health_sampler(&self) {
        if std::env::var(SAMPLER_ENV).is_ok_and(|value| value == "off") {
            tracing::info!("host health sampler disabled by {SAMPLER_ENV}=off");
            return;
        }
        self.spawn_host_health_sampler_with(Box::new(source::SystemSource::new(self.store.root())));
        // The live CPU feed reads per-core ticks only where the platform has a reader; elsewhere
        // it answers `unsupported` and the client stops asking (HS2-CPFBWP).
        if source::core_ticks_supported() {
            self.host_health
                .live_cpu
                .set_source(Box::new(source::SystemCoreSource));
        }
    }

    pub(crate) fn spawn_host_health_sampler_with(&self, source: Box<dyn HostSource>) {
        if self
            .host_health
            .started
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let state = self.clone();
        tokio::spawn(run_sampler(state, source));
    }
}
