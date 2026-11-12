//! Live CPU feed (HS2-CPFBWP, `docs/host-health.md` "Live CPU"): while a client shows the CPU
//! card, the server samples per-core CPU ticks every [`LIVE_CPU_INTERVAL`] and serves each frame
//! through a genuine long poll, `GET /host-health/cpu?after=<seq>`. The request returns as soon as
//! a frame newer than `after` exists, so the client's next request is paced by the server's
//! sampling, never by a client timer.
//!
//! The fast sampler runs only while someone asks: each request marks interest, and the task stops
//! [`LIVE_CPU_IDLE`] after the last one. Frames are not change events, so they never enter the
//! cursored ring the threshold crossings use. Values are metrics only.

use crate::AppState;
use hotsheet_sync::LockExt;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// How often the fast sampler reads per-core ticks while the card is shown.
pub(crate) const LIVE_CPU_INTERVAL: Duration = Duration::from_secs(2);
/// The first frame after a (re)start compares ticks this far apart, so the card fills quickly.
pub(crate) const LIVE_CPU_FIRST_INTERVAL: Duration = Duration::from_millis(500);
/// The fast sampler stops this long after the last request.
pub(crate) const LIVE_CPU_IDLE: Duration = Duration::from_secs(15);
/// How long a long-poll request waits for a newer frame by default, and at most.
const POLL_DEFAULT_MS: u64 = 10_000;
const POLL_MAX_MS: u64 = 25_000;

/// Cumulative per-core `(busy, total)` ticks. Tests inject a scripted source.
pub(crate) trait CoreSource: Send {
    /// `None` when the reader failed this tick.
    fn ticks(&mut self) -> Option<Vec<(u64, u64)>>;
}

/// Whether a frame carries values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LiveCpuStatus {
    /// Values from two tick readings `interval_ms` apart.
    Live,
    /// The sampler has one reading and waits for the next.
    Starting,
    /// The reader failed; the client keeps polling with backoff.
    Unavailable,
    /// No per-core reader on this platform, or the sampler is off. The client stops polling.
    Unsupported,
}

/// One live CPU frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct LiveCpuFrame {
    /// Increases with every frame; a client asks for frames after the one it holds.
    pub(crate) seq: u64,
    pub(crate) status: LiveCpuStatus,
    /// When the newer of the two readings was taken (RFC 3339).
    pub(crate) sampled_at: Option<String>,
    /// Whole-machine busy percent over the interval.
    pub(crate) busy_percent: Option<f64>,
    /// Each logical core's busy percent over the interval, in OS order.
    pub(crate) core_percent: Vec<f64>,
    /// The sampling cadence, so a client can label the frame's freshness.
    pub(crate) interval_ms: u64,
}

impl LiveCpuFrame {
    fn empty(status: LiveCpuStatus, interval: Duration) -> Self {
        Self {
            seq: 0,
            status,
            sampled_at: None,
            busy_percent: None,
            core_percent: Vec::new(),
            interval_ms: millis(interval),
        }
    }
}

/// The percent busy between two cumulative `(busy, total)` readings; `None` without elapsed ticks.
fn busy_between(before: (u64, u64), after: (u64, u64)) -> Option<f64> {
    let total = after.1.saturating_sub(before.1);
    (total > 0).then(|| {
        let busy = after.0.saturating_sub(before.0) as f64;
        (busy / total as f64 * 100.0).clamp(0.0, 100.0)
    })
}

/// Per-core and whole-machine busy percents between two readings, rounded to one decimal. A core
/// count change (a core went offline) restarts the comparison.
pub(crate) fn frame_values(before: &[(u64, u64)], after: &[(u64, u64)]) -> Option<(f64, Vec<f64>)> {
    if before.len() != after.len() || after.is_empty() {
        return None;
    }
    let round = |value: f64| (value * 10.0).round() / 10.0;
    let cores = before
        .iter()
        .zip(after)
        .map(|(before, after)| busy_between(*before, *after).map_or(0.0, round))
        .collect();
    let sum = |values: &[(u64, u64)]| {
        values.iter().fold((0_u64, 0_u64), |(busy, total), core| {
            (busy.saturating_add(core.0), total.saturating_add(core.1))
        })
    };
    let busy = busy_between(sum(before), sum(after))?;
    Some((round(busy), cores))
}

/// The live feed's shared state.
pub(crate) struct LiveCpu {
    frame: tokio::sync::watch::Sender<LiveCpuFrame>,
    /// Milliseconds since the epoch of the latest request.
    interest_ms: AtomicU64,
    running: AtomicBool,
    /// `None` while the host health sampler is off: frames say `unsupported`.
    source: Mutex<Option<Box<dyn CoreSource>>>,
    /// Cadence, first-frame delay, and idle stop, in milliseconds (tests shorten them).
    interval_ms: AtomicU64,
    first_interval_ms: AtomicU64,
    idle_ms: AtomicU64,
}

impl std::fmt::Debug for LiveCpu {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LiveCpu")
            .field("running", &self.running.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

const fn millis(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

impl Default for LiveCpu {
    fn default() -> Self {
        Self {
            frame: tokio::sync::watch::Sender::new(LiveCpuFrame::empty(
                LiveCpuStatus::Unsupported,
                LIVE_CPU_INTERVAL,
            )),
            interest_ms: AtomicU64::new(0),
            running: AtomicBool::new(false),
            source: Mutex::new(None),
            interval_ms: AtomicU64::new(millis(LIVE_CPU_INTERVAL)),
            first_interval_ms: AtomicU64::new(millis(LIVE_CPU_FIRST_INTERVAL)),
            idle_ms: AtomicU64::new(millis(LIVE_CPU_IDLE)),
        }
    }
}

impl LiveCpu {
    /// Shorten the cadence for tests.
    #[cfg(test)]
    pub(crate) fn set_timing(&self, interval: Duration, first_interval: Duration, idle: Duration) {
        self.interval_ms.store(millis(interval), Ordering::SeqCst);
        self.first_interval_ms
            .store(millis(first_interval), Ordering::SeqCst);
        self.idle_ms.store(millis(idle), Ordering::SeqCst);
        self.frame
            .send_modify(|frame| frame.interval_ms = millis(interval));
    }

    fn duration(value: &AtomicU64) -> Duration {
        Duration::from_millis(value.load(Ordering::SeqCst))
    }

    /// Install the per-core reader; the host health sampler does this when it starts.
    pub(crate) fn set_source(&self, source: Box<dyn CoreSource>) {
        self.source.with_lock(|slot| *slot = Some(source));
        self.frame.send_modify(|frame| {
            if frame.status == LiveCpuStatus::Unsupported {
                frame.status = LiveCpuStatus::Starting;
            }
        });
    }

    fn has_source(&self) -> bool {
        self.source.with_lock(|slot| slot.is_some())
    }

    fn read_ticks(&self) -> Option<Option<Vec<(u64, u64)>>> {
        self.source
            .with_lock(|slot| slot.as_mut().map(|source| source.ticks()))
    }

    /// Whether the sampler task runs (for tests).
    #[cfg(test)]
    pub(crate) fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn publish(&self, status: LiveCpuStatus, values: Option<(f64, Vec<f64>)>) {
        let sampled_at = super::rfc3339(crate::client_drive::now_ms());
        self.frame.send_modify(|frame| {
            frame.seq += 1;
            frame.status = status;
            frame.sampled_at = sampled_at;
            (frame.busy_percent, frame.core_percent) = match values {
                Some((busy, cores)) => (Some(busy), cores),
                None => (None, Vec::new()),
            };
        });
    }

    fn interested(&self) -> bool {
        let last = self.interest_ms.load(Ordering::SeqCst);
        crate::client_drive::now_ms().saturating_sub(last) < self.idle_ms.load(Ordering::SeqCst)
    }
}

/// The fast sampler: read ticks on the live cadence until nobody has asked for the idle window.
async fn run(state: AppState) {
    let live = &state.host_health.live_cpu;
    let mut previous: Option<Vec<(u64, u64)>> = None;
    let mut wait = LiveCpu::duration(&live.first_interval_ms);
    while let Some(reading) = live.read_ticks() {
        match (reading, previous.take()) {
            (Some(now), Some(before)) => {
                match frame_values(&before, &now) {
                    Some(values) => live.publish(LiveCpuStatus::Live, Some(values)),
                    None => live.publish(LiveCpuStatus::Starting, None),
                }
                previous = Some(now);
            }
            (Some(now), None) => previous = Some(now),
            (None, _) => live.publish(LiveCpuStatus::Unavailable, None),
        }
        if !live.interested() {
            live.running.store(false, Ordering::SeqCst);
            // A request that arrived while stopping either sees `running` false and starts a new
            // task, or set its interest before this check and keeps this one going.
            if !(live.interested() && !live.running.swap(true, Ordering::SeqCst)) {
                return;
            }
        }
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = state.stopping() => {
                live.running.store(false, Ordering::SeqCst);
                return;
            }
        }
        wait = LiveCpu::duration(&live.interval_ms);
    }
    live.running.store(false, Ordering::SeqCst);
}

#[derive(Debug, Deserialize)]
pub(crate) struct LiveCpuParams {
    /// Return the first frame whose `seq` is greater than this; omit for the current frame.
    after: Option<u64>,
    timeout_ms: Option<u64>,
}

/// `GET /host-health/cpu?after=<seq>&timeout_ms=<n>`: the live CPU long poll (HS2-CPFBWP).
pub(crate) async fn get_live_cpu(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(params): axum::extract::Query<LiveCpuParams>,
) -> axum::Json<LiveCpuFrame> {
    let live = &state.host_health.live_cpu;
    if !live.has_source() {
        return axum::Json(live.frame.borrow().clone());
    }
    live.interest_ms
        .store(crate::client_drive::now_ms(), Ordering::SeqCst);
    if !live.running.swap(true, Ordering::SeqCst) {
        tokio::spawn(run(state.clone()));
    }
    let current = live.frame.borrow().seq;
    // No cursor, or one from a restarted server (ahead of ours): answer with the current frame.
    let Some(after) = params.after.filter(|after| *after <= current) else {
        return axum::Json(live.frame.borrow().clone());
    };
    let timeout = Duration::from_millis(
        params
            .timeout_ms
            .unwrap_or(POLL_DEFAULT_MS)
            .min(POLL_MAX_MS),
    );
    let mut receiver = live.frame.subscribe();
    let newer = receiver.wait_for(|frame| frame.seq > after);
    let _ = tokio::time::timeout(timeout, newer).await;
    let frame = live.frame.borrow().clone();
    axum::Json(frame)
}
