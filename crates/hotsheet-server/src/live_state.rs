//! Per-store live state for the glance surfaces (HS2-JTTQMY, `docs/32-notifier-plugins.md`
//! §32.8.3): standby, the project and grid sidebars, the stats Right now section, and the
//! away digest all bind to one server-owned snapshot per hosted store.
//!
//! The state word comes from the notification router's claim tracker
//! ([`crate::project_activity::ProjectActivity::live`]), so clients see the states that
//! notifications use, plus **Waiting on you**. The rest is read from the store's index and
//! its metrics usage JSONL. Rates ship as timestamps and per-minute buckets, so clients
//! compute ages and decaying rates locally without asking again; only the rolling 30-day
//! peaks the gauges scale to are computed here and persisted per store.
//!
//! A snapshot is rebuilt when something that feeds it happens (a ticket write, activity, a
//! usage record, a halt, a tracker deadline), and on the router's minute timer so a quiet
//! store's rates decay (HS2-MHW3SX). It is pushed as one `live_state` change event only when
//! it changed. Rebuilds are coalesced per store. No client polls.
//!
//! The inputs that only live in memory (the tracker, progress fingerprints, ETA samples, and
//! the revision) are kept in the store's machine-local live-state record
//! ([`crate::live_record`]) and restored when the store is seeded (HS2-MHW3SX).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path as RoutePath, State};
use hotsheet_model::Status;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::ops::TicketQuery;
use hotsheet_ticketing::wire::TicketRow;
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::AppState;
use crate::api_error::ApiError;
use crate::events::ChangeEvent;
use crate::live_record::{EtaSample, LiveRecord, RECORD_VERSION};
use crate::project_activity::{ActivityWord, LiveActivity, PromptKind, UPDATED_WINDOW};

/// The change-event kind that carries a [`LiveStoreState`].
pub const LIVE_STATE_KIND: &str = "live_state";

/// Tokens per minute average over this many whole minutes, ending with the current one.
pub const TOKEN_WINDOW_MINUTES: i64 = 5;

/// Peaks are the maximum over this many UTC days, today included.
pub const PEAK_DAYS: i64 = 30;

/// Waiting tickets listed by slug; `count` still counts every one.
const WAITING_LIMIT: usize = 20;

/// Progress times sent for "tickets updated per hour"; the newest are kept.
const UPDATED_LIMIT: usize = 2000;

/// Planned claim durations kept per store for the runway estimate.
const ETA_SAMPLES: usize = 20;

/// Bursts of changes within this window share one rebuild.
const COALESCE: std::time::Duration = std::time::Duration::from_millis(250);

/// A last-progress move smaller than this alone does not push a new snapshot, so a
/// streaming session does not flood the change ring; ages are shown in minutes.
const PROGRESS_GRANULARITY_MS: i64 = 15_000;

/// The store's one word for glance surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveWord {
    Active,
    Stopped,
    MayBeStuck,
    Stalled,
    Halted,
    WaitingOnYou,
}

impl From<ActivityWord> for LiveWord {
    fn from(word: ActivityWord) -> Self {
        match word {
            ActivityWord::Active => LiveWord::Active,
            ActivityWord::Stopped => LiveWord::Stopped,
            ActivityWord::MayBeStuck => LiveWord::MayBeStuck,
            ActivityWord::Stalled => LiveWord::Stalled,
            ActivityWord::Halted => LiveWord::Halted,
        }
    }
}

/// The combined word: problems first, then Waiting on you, then the activity word.
pub fn combined_word(activity: ActivityWord, waiting: usize) -> LiveWord {
    match activity {
        ActivityWord::Halted | ActivityWord::Stalled | ActivityWord::MayBeStuck => activity.into(),
        _ if waiting > 0 => LiveWord::WaitingOnYou,
        other => other.into(),
    }
}

/// One hosted store's live snapshot. Times are Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveStoreState {
    /// The store's URL id, as in every change event's `store`.
    pub store: String,
    pub root: String,
    /// Display name: the first linked checkout's alias, else the store folder.
    pub name: String,
    /// Registered checkouts linked to this store (client project ids).
    pub checkouts: Vec<String>,
    /// When this server started; with `revision`, orders snapshots across restarts.
    pub epoch_ms: i64,
    /// Increases with every pushed change of this store's snapshot.
    pub revision: u64,
    pub state: LiveWord,
    pub activity: LiveActivityFacts,
    pub claims: Vec<LiveClaim>,
    pub workers: LiveWorkers,
    pub waiting: LiveWaiting,
    pub runway: LiveRunway,
    pub rates: LiveRates,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveActivityFacts {
    /// The claim tracker's word alone (no Waiting on you).
    pub word: LiveWord,
    pub active_since_ms: Option<i64>,
    pub stopped_at_ms: Option<i64>,
    /// The idle detector's progress: activity, turns, and ticket writes, not renewals.
    pub last_progress_ms: Option<i64>,
    /// When the store will read May be stuck without further progress.
    pub idle_deadline_ms: Option<i64>,
    pub stalled: Vec<LiveStalledClaim>,
    pub halted: Vec<LiveHalt>,
    /// Why held claims are not moving, as the tools report it (HS2-EGKKA0): usage-limit
    /// pauses first, then terminal prompts, each oldest first.
    pub causes: Vec<LiveCause>,
}

/// A stuck cause's kind (HS2-EGKKA0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCauseKind {
    /// A halted session's tool reported a usage or rate limit.
    UsageLimit,
    /// A session waits at a terminal prompt for the user.
    AwaitingInput,
}

/// Which terminal prompt an `awaiting_input` cause waits at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LivePrompt {
    Permission,
    Question,
}

/// One stuck cause of a worker holding a live claim in this store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveCause {
    pub kind: LiveCauseKind,
    pub worker: String,
    /// The Hot Sheet terminal the session runs in, for a direct link; `null` when unknown.
    pub terminal: Option<String>,
    /// When the halt was reported or the prompt appeared.
    pub since_ms: i64,
    /// `usage_limit`: when the tool said the limit resets. `null` means the tool did not
    /// state a reset time that needs no time-zone guess, so it is unknown. Always `null` for
    /// `awaiting_input`.
    pub resets_at_ms: Option<i64>,
    /// `awaiting_input`: the prompt; `null` for `usage_limit`.
    pub prompt: Option<LivePrompt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveStalledClaim {
    pub slug: String,
    pub worker: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveHalt {
    pub worker: String,
    pub message: String,
    pub at_ms: i64,
}

/// A live claim; the ETA overrun is `now - eta_at_ms` when positive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveClaim {
    pub ticket: String,
    pub slug: String,
    pub title: String,
    pub worker: String,
    pub phase: Option<String>,
    pub claimed_at_ms: Option<i64>,
    pub lease_expires_at_ms: i64,
    pub eta_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWorkers {
    /// Distinct workers holding a live claim.
    pub live: usize,
    /// Distinct workers that held a claim in the current (or stalled) active period.
    pub expected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWaiting {
    pub count: usize,
    pub tickets: Vec<LiveWaitingTicket>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWaitingTicket {
    pub ticket: String,
    pub slug: String,
    pub title: String,
    /// Always `feedback`: an open feedback request on a Started ticket (HS2-JGW031).
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveRunway {
    /// Open Up Next tickets that are unblocked, unclaimed, and not waiting on feedback.
    pub ready: usize,
    /// `ready` times the median planned claim duration (claim to ETA) seen recently,
    /// shared across the live workers; `None` without a planned duration.
    pub estimate_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveRates {
    pub tokens: LiveTokenRate,
    pub tickets_updated: LiveUpdatedRate,
}

/// Tokens per minute = the sum of `minutes` whose start lies in the last
/// `window_minutes` whole minutes (the current one included), divided by `window_minutes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveTokenRate {
    pub window_minutes: i64,
    pub minutes: Vec<LiveTokenMinute>,
    /// The highest tokens per minute in the last 30 days.
    pub peak_per_minute: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveTokenMinute {
    pub start_ms: i64,
    pub tokens: u64,
}

/// Tickets updated per hour = the entries of `at_ms` newer than `window_minutes` ago.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveUpdatedRate {
    pub window_minutes: i64,
    /// One time per ticket (its last progress write), oldest first.
    pub at_ms: Vec<i64>,
    /// The most distinct tickets updated in any hour of the last 30 days.
    pub peak_per_hour: u64,
}

/// `GET /live-state`.
#[derive(Debug, Clone, Serialize)]
pub struct LiveStateList {
    pub epoch_ms: i64,
    pub stores: Vec<LiveStoreState>,
}

/// Server-wide live-state registry, one entry per store.
#[derive(Clone)]
pub(crate) struct LiveStates {
    /// This server's start, raised above any restored record's epoch (HS2-MHW3SX).
    epoch_ms: Arc<AtomicI64>,
    stores: Arc<Mutex<HashMap<String, StoreLive>>>,
    /// Serializes rebuilds so revisions follow build order.
    building: Arc<Mutex<()>>,
}

impl Default for LiveStates {
    fn default() -> Self {
        LiveStates {
            epoch_ms: Arc::new(AtomicI64::new(unix_ms(OffsetDateTime::now_utc()))),
            stores: Arc::default(),
            building: Arc::default(),
        }
    }
}

#[derive(Default)]
struct StoreLive {
    revision: u64,
    /// The last pushed snapshot with `revision` zeroed.
    last: Option<LiveStoreState>,
    pending: bool,
    usage: UsageTail,
    peaks: Option<PeakBook>,
    /// `(ticket, claimed_at_ms, planned_ms)`, newest last.
    eta_samples: VecDeque<(String, i64, i64)>,
    /// The persisted record was restored into this entry (HS2-MHW3SX).
    restored: bool,
    /// The last record written, with `saved_at_ms` zeroed, so an unchanged one is not
    /// rewritten.
    saved: Option<LiveRecord>,
}

impl LiveStates {
    fn epoch(&self) -> i64 {
        self.epoch_ms.load(Ordering::SeqCst)
    }
}

/// Restore `store`'s persisted revision, ETA samples, and epoch floor (HS2-MHW3SX). Runs once
/// per store when the tracker is seeded; revisions continue above the record's, and this
/// server's epoch is raised above the record's so snapshots order after the old server's even
/// if the clock stepped back.
pub(crate) fn restore(state: &AppState, store: &str, record: Option<&LiveRecord>) {
    let mut stores = state.live_states.stores.lock_or_recover();
    let entry = stores.entry(store.to_string()).or_default();
    if entry.restored {
        return;
    }
    entry.restored = true;
    let Some(record) = record else {
        return;
    };
    entry.revision = entry.revision.max(record.revision);
    if entry.eta_samples.is_empty() {
        entry.eta_samples = record
            .eta_samples
            .iter()
            .map(|s| (s.ticket.clone(), s.claimed_at_ms, s.planned_ms))
            .collect();
        while entry.eta_samples.len() > ETA_SAMPLES {
            entry.eta_samples.pop_front();
        }
    }
    let epoch = state.live_states.epoch();
    if record.epoch_ms >= epoch {
        state
            .live_states
            .epoch_ms
            .fetch_max(record.epoch_ms + 1, Ordering::SeqCst);
    }
}

/// Rebuild every hosted store's snapshot and push the ones that changed (HS2-MHW3SX). The
/// notification router's minute timer calls it, so rates decay on a quiet store.
pub fn refresh_all(state: &AppState) {
    refresh_all_at(state, OffsetDateTime::now_utc());
}

/// [`refresh_all`] at a given clock (tests).
pub fn refresh_all_at(state: &AppState, now: OffsetDateTime) {
    let mut ids: Vec<String> = state
        .host
        .locations()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    for id in ids {
        refresh_at(state, &id, now);
    }
}

fn unix_ms(at: OffsetDateTime) -> i64 {
    (at.unix_timestamp_nanos() / 1_000_000) as i64
}

fn instant(text: Option<&str>) -> Option<OffsetDateTime> {
    text.and_then(|at| hotsheet_model::Timestamp::new(at).instant())
}

/// Something that feeds `store`'s snapshot changed: rebuild it soon and push it if it
/// differs. Bursts share one rebuild. Outside a runtime (synchronous tests) it rebuilds now.
pub(crate) fn touch(state: &AppState, store: &str) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        refresh(state, store);
        return;
    };
    {
        let mut stores = state.live_states.stores.lock_or_recover();
        let entry = stores.entry(store.to_string()).or_default();
        if entry.pending {
            return;
        }
        entry.pending = true;
    }
    let state = state.clone();
    let store = store.to_string();
    runtime.spawn(async move {
        tokio::time::sleep(COALESCE).await;
        let _ = tokio::task::spawn_blocking(move || {
            refresh(&state, &store);
        })
        .await;
    });
}

/// Rebuild `store`'s snapshot now and push it when it changed. Returns the current
/// snapshot, or `None` when the store is not hosted.
pub fn refresh(state: &AppState, store: &str) -> Option<LiveStoreState> {
    refresh_at(state, store, OffsetDateTime::now_utc())
}

/// [`refresh`] at a given clock (tests).
pub fn refresh_at(state: &AppState, store: &str, now: OffsetDateTime) -> Option<LiveStoreState> {
    let _building = state.live_states.building.lock_or_recover();
    if let Some(entry) = state.live_states.stores.lock_or_recover().get_mut(store) {
        entry.pending = false;
    }
    let mut snapshot = build(state, store, now)?;
    let changed = {
        let mut stores = state.live_states.stores.lock_or_recover();
        let entry = stores.entry(store.to_string()).or_default();
        let changed = entry
            .last
            .as_ref()
            .is_none_or(|last| materially_changed(last, &snapshot));
        if changed {
            entry.revision += 1;
            entry.last = Some(snapshot.clone());
        }
        snapshot.revision = entry.revision;
        if !changed {
            // Report the pushed snapshot so a read never shows what no event carried.
            if let Some(last) = &entry.last {
                let mut pushed = last.clone();
                pushed.revision = entry.revision;
                snapshot = pushed;
            }
        }
        changed
    };
    persist(state, store, now);
    if changed {
        state.emit(ChangeEvent {
            cursor: None,
            store: store.to_string(),
            kind: LIVE_STATE_KIND.into(),
            id: store.to_string(),
            slug: String::new(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
            live: Some(Box::new(snapshot.clone())),
        });
    }
    Some(snapshot)
}

/// Write `store`'s live-state record when its content changed (HS2-MHW3SX).
fn persist(state: &AppState, store: &str, now: OffsetDateTime) {
    let Some(entry) = state.host.get(store) else {
        return;
    };
    let (activity, fingerprints) = crate::notification_router::persisted_inputs(state, store, now);
    let record = {
        let mut stores = state.live_states.stores.lock_or_recover();
        let live = stores.entry(store.to_string()).or_default();
        let mut record = LiveRecord {
            version: RECORD_VERSION,
            saved_at_ms: 0,
            epoch_ms: state.live_states.epoch(),
            revision: live.revision,
            activity,
            fingerprints,
            eta_samples: live
                .eta_samples
                .iter()
                .map(|(ticket, claimed_at_ms, planned_ms)| EtaSample {
                    ticket: ticket.clone(),
                    claimed_at_ms: *claimed_at_ms,
                    planned_ms: *planned_ms,
                })
                .collect(),
        };
        record.cap_fingerprints();
        if live.saved.as_ref() == Some(&record) {
            return;
        }
        record
    };
    // Rebuilds are serialized, so no other write of this record runs meanwhile.
    if record.save(&entry.store, unix_ms(now)) {
        let mut stores = state.live_states.stores.lock_or_recover();
        stores.entry(store.to_string()).or_default().saved = Some(record);
    }
}

/// Whether `next` differs from `last` enough to push. Last progress and the idle deadline
/// it moves count only when they shift by [`PROGRESS_GRANULARITY_MS`] or appear/vanish.
fn materially_changed(last: &LiveStoreState, next: &LiveStoreState) -> bool {
    let near = |a: Option<i64>, b: Option<i64>| match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() < PROGRESS_GRANULARITY_MS,
        (None, None) => true,
        _ => false,
    };
    if !near(
        last.activity.last_progress_ms,
        next.activity.last_progress_ms,
    ) || !near(
        last.activity.idle_deadline_ms,
        next.activity.idle_deadline_ms,
    ) {
        return true;
    }
    let mut normalized = next.clone();
    normalized.revision = last.revision;
    normalized.activity.last_progress_ms = last.activity.last_progress_ms;
    normalized.activity.idle_deadline_ms = last.activity.idle_deadline_ms;
    normalized != *last
}

/// Every hosted store's current snapshot, by store id.
pub fn snapshot_all(state: &AppState) -> LiveStateList {
    let mut ids: Vec<String> = state
        .host
        .locations()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    let stores = ids.iter().filter_map(|id| refresh(state, id)).collect();
    LiveStateList {
        epoch_ms: state.live_states.epoch(),
        stores,
    }
}

fn build(state: &AppState, store: &str, now: OffsetDateTime) -> Option<LiveStoreState> {
    let entry = state.host.get(store)?;
    let activity = crate::notification_router::live_activity(state, store, now);
    let query = |q: TicketQuery| -> Vec<TicketRow> {
        entry
            .index
            .with_lock(|index| index.query(&q))
            .unwrap_or_else(|error| {
                tracing::warn!(store, %error, "live state could not read the index");
                Vec::new()
            })
    };
    let claims = live_claims(
        query(TicketQuery {
            claimed: Some(true),
            ..TicketQuery::default()
        }),
        now,
    );
    // Needs you counts only Started tickets waiting on a feedback answer (HS2-JGW031): a review
    // request or a feedback prompt on a ticket outside the Started column is not live work.
    let waiting_rows = query(TicketQuery {
        status: Some(Status::Started),
        needs_review: Some(true),
        ..TicketQuery::default()
    });
    let ready_rows = query(TicketQuery {
        open_only: true,
        up_next_only: true,
        blocked: Some(false),
        ..TicketQuery::default()
    });
    let ready = ready_count(&ready_rows, &claims);
    let waiting = waiting_of(&waiting_rows);
    let (checkouts, name) = crate::notification_router::store_identity(state, store);
    let mut stores = state.live_states.stores.lock_or_recover();
    let live = stores.entry(store.to_string()).or_default();
    let tokens = token_rate(live, &entry.store, now);
    let updated = updated_rate(live, &entry.store, &activity, now);
    let workers_live = {
        let mut workers: Vec<&str> = claims.iter().map(|c| c.worker.as_str()).collect();
        workers.sort_unstable();
        workers.dedup();
        workers.len()
    };
    let runway = LiveRunway {
        ready,
        estimate_ms: runway_estimate(live, &claims, ready, workers_live),
    };
    drop(stores);
    Some(LiveStoreState {
        store: store.to_string(),
        root: entry.store.root().display().to_string(),
        name,
        checkouts,
        epoch_ms: state.live_states.epoch(),
        revision: 0,
        state: combined_word(activity.word, waiting.count),
        activity: facts(&activity),
        workers: LiveWorkers {
            live: workers_live,
            expected: activity.expected_workers.max(workers_live),
        },
        claims,
        waiting,
        runway,
        rates: LiveRates {
            tokens,
            tickets_updated: updated,
        },
    })
}

fn facts(activity: &LiveActivity) -> LiveActivityFacts {
    LiveActivityFacts {
        word: activity.word.into(),
        active_since_ms: activity.active_since.map(unix_ms),
        stopped_at_ms: activity.stopped_at.map(unix_ms),
        last_progress_ms: activity.last_progress.map(unix_ms),
        idle_deadline_ms: activity.idle_deadline.map(unix_ms),
        stalled: activity
            .stalled
            .iter()
            .map(|(slug, worker)| LiveStalledClaim {
                slug: slug.clone(),
                worker: worker.clone(),
            })
            .collect(),
        halted: activity
            .halted
            .iter()
            .map(|halt| LiveHalt {
                worker: halt.worker.clone(),
                message: halt.message.clone(),
                at_ms: unix_ms(halt.at),
            })
            .collect(),
        causes: causes(activity),
    }
}

/// The snapshot's stuck causes (HS2-EGKKA0): usage-limit halts, then terminal prompts.
pub fn causes(activity: &LiveActivity) -> Vec<LiveCause> {
    let mut limits: Vec<LiveCause> = activity
        .halted
        .iter()
        .filter(|halt| halt.detail.usage_limit)
        .map(|halt| LiveCause {
            kind: LiveCauseKind::UsageLimit,
            worker: halt.worker.clone(),
            terminal: halt.detail.terminal.clone(),
            since_ms: unix_ms(halt.at),
            resets_at_ms: halt.detail.resets_at.map(unix_ms),
            prompt: None,
        })
        .collect();
    limits.sort_by(|a, b| (a.since_ms, &a.worker).cmp(&(b.since_ms, &b.worker)));
    limits.extend(activity.prompts.iter().map(|prompt| LiveCause {
        kind: LiveCauseKind::AwaitingInput,
        worker: prompt.worker.clone(),
        terminal: Some(prompt.terminal.clone()),
        since_ms: unix_ms(prompt.since),
        resets_at_ms: None,
        prompt: Some(match prompt.kind {
            PromptKind::Permission => LivePrompt::Permission,
            PromptKind::Question => LivePrompt::Question,
        }),
    }));
    limits
}

/// Rows with an unexpired lease, oldest claim first.
pub fn live_claims(rows: Vec<TicketRow>, now: OffsetDateTime) -> Vec<LiveClaim> {
    let mut claims: Vec<LiveClaim> = rows
        .into_iter()
        .filter_map(|row| {
            let worker = row.claimed_by?;
            let expires = instant(row.claim_lease_expires_at.as_deref())?;
            (expires > now).then(|| LiveClaim {
                ticket: row.id,
                slug: row.slug,
                title: row.title,
                worker,
                phase: row.started_phase,
                claimed_at_ms: instant(row.claim_started_at.as_deref()).map(unix_ms),
                lease_expires_at_ms: unix_ms(expires),
                eta_at_ms: instant(row.claim_eta_at.as_deref()).map(unix_ms),
            })
        })
        .collect();
    claims.sort_by(|a, b| (a.claimed_at_ms, &a.slug).cmp(&(b.claimed_at_ms, &b.slug)));
    claims
}

/// Up Next rows that a worker could take now.
pub fn ready_count(rows: &[TicketRow], claims: &[LiveClaim]) -> usize {
    rows.iter()
        .filter(|row| !row.feedback_needed && !claims.iter().any(|c| c.ticket == row.id))
        .count()
}

/// Started tickets blocked on a feedback answer, by slug (HS2-JGW031). Review requests and
/// tickets outside the Started column never count.
pub fn waiting_of(rows: &[TicketRow]) -> LiveWaiting {
    let mut tickets: Vec<LiveWaitingTicket> = rows
        .iter()
        .filter(|row| row.feedback_needed && row.status.as_deref() == Some("started"))
        .map(|row| LiveWaitingTicket {
            ticket: row.id.clone(),
            slug: row.slug.clone(),
            title: row.title.clone(),
            reason: "feedback".into(),
        })
        .collect();
    tickets.sort_by(|a, b| a.slug.cmp(&b.slug));
    let count = tickets.len();
    tickets.truncate(WAITING_LIMIT);
    LiveWaiting { count, tickets }
}

fn runway_estimate(
    live: &mut StoreLive,
    claims: &[LiveClaim],
    ready: usize,
    workers: usize,
) -> Option<i64> {
    for claim in claims {
        let (Some(start), Some(eta)) = (claim.claimed_at_ms, claim.eta_at_ms) else {
            continue;
        };
        if eta <= start
            || live
                .eta_samples
                .iter()
                .any(|(ticket, at, _)| *ticket == claim.ticket && *at == start)
        {
            continue;
        }
        live.eta_samples
            .push_back((claim.ticket.clone(), start, eta - start));
        while live.eta_samples.len() > ETA_SAMPLES {
            live.eta_samples.pop_front();
        }
    }
    let mut planned: Vec<i64> = live.eta_samples.iter().map(|(_, _, ms)| *ms).collect();
    if planned.is_empty() {
        return None;
    }
    planned.sort_unstable();
    let median = planned[planned.len() / 2];
    Some(median * ready as i64 / workers.max(1) as i64)
}

// ---- rates and peaks -----------------------------------------------------------------

const MINUTE_MS: i64 = 60_000;

fn minute_of(ms: i64) -> i64 {
    ms.div_euclid(MINUTE_MS) * MINUTE_MS
}

fn day_of(ms: i64) -> String {
    let at = OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}",
        at.year(),
        u8::from(at.month()),
        at.day()
    )
}

/// Tokens per minute for the window ending with the minute that starts at `end`.
fn window_rate(minutes: &BTreeMap<i64, u64>, end: i64) -> f64 {
    let start = end - (TOKEN_WINDOW_MINUTES - 1) * MINUTE_MS;
    let total: u64 = minutes.range(start..=end).map(|(_, tokens)| *tokens).sum();
    total as f64 / TOKEN_WINDOW_MINUTES as f64
}

fn token_rate(live: &mut StoreLive, store: &FsStore, now: OffsetDateTime) -> LiveTokenRate {
    let now_ms = unix_ms(now);
    let book = live.peaks.get_or_insert_with(|| PeakBook::load(store));
    let backfill = !live.usage.primed && !book.has_tokens();
    let added = live.usage.ingest(store, now_ms, backfill);
    let mut raised = false;
    // A new minute raises every window that ends in it or in the next window-1 minutes.
    let mut ends: Vec<i64> = added
        .iter()
        .flat_map(|minute| (0..TOKEN_WINDOW_MINUTES).map(move |k| minute + k * MINUTE_MS))
        .collect();
    ends.sort_unstable();
    ends.dedup();
    for end in ends {
        let rate = window_rate(&live.usage.minutes, end);
        raised |= book.raise_tokens(&day_of(end), rate);
    }
    live.usage.prune(now_ms);
    let today = day_of(now_ms);
    if raised {
        book.save(store, &today);
    }
    let current = minute_of(now_ms);
    let first = current - (TOKEN_WINDOW_MINUTES - 1) * MINUTE_MS;
    LiveTokenRate {
        window_minutes: TOKEN_WINDOW_MINUTES,
        minutes: live
            .usage
            .minutes
            .range(first..)
            .map(|(start, tokens)| LiveTokenMinute {
                start_ms: *start,
                tokens: *tokens,
            })
            .collect(),
        peak_per_minute: book.peak_tokens(&today),
    }
}

fn updated_rate(
    live: &mut StoreLive,
    store: &FsStore,
    activity: &LiveActivity,
    now: OffsetDateTime,
) -> LiveUpdatedRate {
    let now_ms = unix_ms(now);
    let today = day_of(now_ms);
    let mut at_ms: Vec<i64> = activity.updated.iter().copied().map(unix_ms).collect();
    let count = at_ms.len() as u64;
    let book = live.peaks.get_or_insert_with(|| PeakBook::load(store));
    if book.raise_tickets(&today, count) {
        book.save(store, &today);
    }
    if at_ms.len() > UPDATED_LIMIT {
        at_ms.drain(..at_ms.len() - UPDATED_LIMIT);
    }
    LiveUpdatedRate {
        window_minutes: UPDATED_WINDOW.whole_minutes(),
        at_ms,
        peak_per_hour: book.peak_tickets(&today),
    }
}

/// Incremental reader of the store's raw usage JSONL (`metrics/raw/<day>.jsonl`).
#[derive(Default)]
struct UsageTail {
    primed: bool,
    /// Bytes consumed per file, up to its last complete line.
    offsets: HashMap<PathBuf, u64>,
    /// Tokens (in + out) per minute start, kept for two days.
    minutes: BTreeMap<i64, u64>,
}

impl UsageTail {
    /// Read lines appended since the last call: today's and yesterday's files, or every
    /// file of the peak period when `backfill`. Returns the minutes that gained tokens.
    fn ingest(&mut self, store: &FsStore, now_ms: i64, backfill: bool) -> Vec<i64> {
        let dir = store.root().join("metrics").join("raw");
        let days: Vec<String> = if backfill {
            (0..PEAK_DAYS)
                .map(|back| day_of(now_ms - back * 86_400_000))
                .collect()
        } else {
            vec![day_of(now_ms - 86_400_000), day_of(now_ms)]
        };
        self.primed = true;
        let mut added = Vec::new();
        for day in days {
            let path = dir.join(format!("{day}.jsonl"));
            let offset = self.offsets.get(&path).copied().unwrap_or(0);
            let Some((text, consumed)) = read_complete_lines(&path, offset) else {
                continue;
            };
            self.offsets.insert(path, offset + consumed);
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                let Ok(event) =
                    serde_json::from_str::<hotsheet_ticketing::metrics::UsageEvent>(line)
                else {
                    continue;
                };
                let Some(at) = instant(Some(&event.ts)) else {
                    continue;
                };
                let tokens = event.tokens_in.saturating_add(event.tokens_out);
                if tokens == 0 {
                    continue;
                }
                let minute = minute_of(unix_ms(at));
                *self.minutes.entry(minute).or_default() += tokens;
                added.push(minute);
            }
        }
        added.sort_unstable();
        added.dedup();
        added
    }

    fn prune(&mut self, now_ms: i64) {
        let keep = now_ms - 2 * 86_400_000;
        self.minutes = self.minutes.split_off(&keep);
        let days = [day_of(now_ms - 86_400_000), day_of(now_ms)];
        self.offsets.retain(|path, _| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| days.iter().any(|day| day == stem))
        });
    }
}

/// The text from `offset` up to the file's last newline, and how many bytes that is.
fn read_complete_lines(path: &Path, offset: u64) -> Option<(String, u64)> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len < offset {
        // Rewritten (for example repriced): it holds nothing new we can place.
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    file.take(len - offset).read_to_end(&mut bytes).ok()?;
    let end = bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |at| at + 1);
    bytes.truncate(end);
    Some((String::from_utf8_lossy(&bytes).into_owned(), end as u64))
}

/// Per-day maxima behind the rolling 30-day peaks, machine-local and gitignored at
/// `metrics/local/live-peaks.json`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct PeakBook {
    #[serde(default)]
    days: BTreeMap<String, DayPeak>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct DayPeak {
    #[serde(default)]
    tokens_per_minute: f64,
    #[serde(default)]
    tickets_per_hour: u64,
}

impl PeakBook {
    fn path(store: &FsStore) -> PathBuf {
        store
            .root()
            .join("metrics")
            .join("local")
            .join("live-peaks.json")
    }

    fn load(store: &FsStore) -> PeakBook {
        std::fs::read(Self::path(store))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn has_tokens(&self) -> bool {
        self.days.values().any(|day| day.tokens_per_minute > 0.0)
    }

    fn raise_tokens(&mut self, day: &str, rate: f64) -> bool {
        let entry = self.days.entry(day.to_string()).or_default();
        if rate > entry.tokens_per_minute {
            entry.tokens_per_minute = rate;
            true
        } else {
            false
        }
    }

    fn raise_tickets(&mut self, day: &str, count: u64) -> bool {
        let entry = self.days.entry(day.to_string()).or_default();
        if count > entry.tickets_per_hour {
            entry.tickets_per_hour = count;
            true
        } else {
            false
        }
    }

    fn first_day(today: &str) -> String {
        let Ok(date) = time::Date::parse(
            today,
            time::macros::format_description!("[year]-[month]-[day]"),
        ) else {
            return today.to_string();
        };
        let first = date - Duration::days(PEAK_DAYS - 1);
        format!(
            "{:04}-{:02}-{:02}",
            first.year(),
            u8::from(first.month()),
            first.day()
        )
    }

    fn in_period<'a>(&'a self, today: &str) -> impl Iterator<Item = &'a DayPeak> {
        let first = Self::first_day(today);
        self.days
            .range(first..)
            .filter(move |(day, _)| day.as_str() <= today)
            .map(|(_, peak)| peak)
    }

    fn peak_tokens(&self, today: &str) -> f64 {
        self.in_period(today)
            .map(|day| day.tokens_per_minute)
            .fold(0.0, f64::max)
    }

    fn peak_tickets(&self, today: &str) -> u64 {
        self.in_period(today)
            .map(|day| day.tickets_per_hour)
            .max()
            .unwrap_or(0)
    }

    /// Drop days before the period and write atomically. Failures are logged; a peak that
    /// could not be saved is still used until the server restarts.
    fn save(&mut self, store: &FsStore, today: &str) {
        let first = Self::first_day(today);
        self.days = self.days.split_off(&first);
        let path = Self::path(store);
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            hotsheet_ticketing::metrics::ensure_gitignored(store, "metrics/local/")?;
            let temp = path.with_extension("json.tmp");
            std::fs::write(
                &temp,
                serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
            )?;
            std::fs::rename(&temp, &path)
        };
        if let Err(error) = write() {
            tracing::warn!(path = %path.display(), %error, "could not save live-state peaks");
        }
    }
}

// ---- routes ----------------------------------------------------------------------

/// `GET /live-state` — every hosted store's live snapshot (HS2-JTTQMY).
pub(crate) async fn list_live_states(State(state): State<AppState>) -> Json<LiveStateList> {
    Json(snapshot_all(&state))
}

/// `GET /stores/{store_id}/live-state` — one hosted store's live snapshot.
pub(crate) async fn store_live_state(
    State(state): State<AppState>,
    RoutePath(store_id): RoutePath<String>,
) -> Result<Json<LiveStoreState>, ApiError> {
    refresh(&state, &store_id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found(&store_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, slug: &str) -> TicketRow {
        let ticket = hotsheet_model::Ticket::new(
            hotsheet_model::Ulid::from_string("01M6N3H10PF7JHA3VHWS72N5Z1").unwrap(),
            slug,
            format!("Title {slug}"),
            "task",
            hotsheet_model::Timestamp::new("2027-01-15T06:00:00Z"),
            hotsheet_model::Timestamp::new("2027-01-15T06:00:00Z"),
        );
        let mut row = TicketRow::from(&ticket);
        row.id = id.to_string();
        row
    }

    fn at(ms: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).unwrap()
    }

    #[test]
    fn hs2_jttqmy_combined_word_puts_problems_before_waiting_before_activity() {
        assert_eq!(combined_word(ActivityWord::Active, 0), LiveWord::Active);
        assert_eq!(
            combined_word(ActivityWord::Active, 2),
            LiveWord::WaitingOnYou
        );
        assert_eq!(
            combined_word(ActivityWord::Stopped, 1),
            LiveWord::WaitingOnYou
        );
        assert_eq!(combined_word(ActivityWord::Stopped, 0), LiveWord::Stopped);
        for problem in [
            ActivityWord::Halted,
            ActivityWord::Stalled,
            ActivityWord::MayBeStuck,
        ] {
            assert_eq!(combined_word(problem, 5), LiveWord::from(problem));
        }
        assert_eq!(
            serde_json::to_value(LiveWord::WaitingOnYou).unwrap(),
            "waiting_on_you"
        );
        assert_eq!(
            serde_json::to_value(LiveWord::MayBeStuck).unwrap(),
            "may_be_stuck"
        );
    }

    #[test]
    fn hs2_jttqmy_claims_keep_live_leases_and_ready_excludes_claimed_and_feedback() {
        let now = at(1_800_000_000_000);
        let mut live = row("a", "HS2-A");
        live.claimed_by = Some("w1".into());
        live.claim_lease_expires_at = Some("2027-01-15T09:00:00Z".into());
        live.claim_started_at = Some("2027-01-15T06:00:00Z".into());
        live.claim_eta_at = Some("2027-01-15T07:00:00Z".into());
        live.started_phase = Some("working".into());
        let mut expired = row("b", "HS2-B");
        expired.claimed_by = Some("w2".into());
        expired.claim_lease_expires_at = Some("2020-01-01T00:00:00Z".into());
        let claims = live_claims(vec![expired, live], now);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].slug, "HS2-A");
        assert_eq!(claims[0].phase.as_deref(), Some("working"));
        assert_eq!(
            claims[0].eta_at_ms.unwrap() - claims[0].claimed_at_ms.unwrap(),
            3_600_000
        );
        let mut feedback = row("c", "HS2-C");
        feedback.feedback_needed = true;
        let ready = [row("a", "HS2-A"), feedback, row("d", "HS2-D")];
        assert_eq!(ready_count(&ready, &claims), 1);
    }

    #[test]
    fn hs2_jgw031_waiting_counts_only_started_feedback_and_caps_the_list() {
        let started_feedback = |n: usize| {
            let mut r = row(&format!("r{n:02}"), &format!("HS2-R{n:02}"));
            r.status = Some("started".into());
            r.feedback_needed = true;
            r.needs_review = true;
            r
        };
        let mut rows: Vec<TicketRow> = (0..25).map(started_feedback).collect();
        // A review request alone, and feedback outside the Started column, never count.
        let mut review = row("rv", "HS2-RV");
        review.status = Some("started".into());
        review.needs_review = true;
        let mut not_started = row("ns", "HS2-NS");
        not_started.status = Some("not_started".into());
        not_started.feedback_needed = true;
        let mut backlog = row("bl", "HS2-BL");
        backlog.status = Some("backlog".into());
        backlog.feedback_needed = true;
        rows.extend([review, not_started, backlog]);
        let waiting = waiting_of(&rows);
        assert_eq!(waiting.count, 25);
        assert_eq!(waiting.tickets.len(), WAITING_LIMIT);
        assert_eq!(waiting.tickets[0].slug, "HS2-R00");
        assert!(waiting.tickets.iter().all(|t| t.reason == "feedback"));
        assert_eq!(waiting_of(&[]).count, 0);
    }

    #[test]
    fn hs2_jttqmy_runway_uses_the_median_planned_claim_shared_by_workers() {
        let mut live = StoreLive::default();
        assert_eq!(
            runway_estimate(&mut live, &[], 4, 0),
            None,
            "no planned claims"
        );
        let claim = |ticket: &str, start: i64, minutes: i64| LiveClaim {
            ticket: ticket.into(),
            slug: ticket.into(),
            title: String::new(),
            worker: "w".into(),
            phase: None,
            claimed_at_ms: Some(start),
            lease_expires_at_ms: start + 1,
            eta_at_ms: Some(start + minutes * MINUTE_MS),
        };
        let claims = [claim("a", 0, 10), claim("b", 0, 30), claim("c", 0, 20)];
        assert_eq!(
            runway_estimate(&mut live, &claims, 3, 1),
            Some(3 * 20 * MINUTE_MS)
        );
        // The same claims seen again are not new samples; two workers halve the estimate.
        assert_eq!(
            runway_estimate(&mut live, &claims, 4, 2),
            Some(2 * 20 * MINUTE_MS)
        );
        assert_eq!(live.eta_samples.len(), 3);
        assert_eq!(runway_estimate(&mut live, &claims, 0, 2), Some(0));
        // A claim without an ETA, or one with an ETA before its start, is no sample.
        let mut no_eta = claim("d", 0, 5);
        no_eta.eta_at_ms = None;
        let backwards = claim("e", 100, -1);
        runway_estimate(&mut live, &[no_eta, backwards], 1, 1);
        assert_eq!(live.eta_samples.len(), 3);
        // The ring keeps the newest samples.
        let many: Vec<LiveClaim> = (0..30).map(|n| claim(&format!("m{n}"), n, 1)).collect();
        runway_estimate(&mut live, &many, 1, 1);
        assert_eq!(live.eta_samples.len(), ETA_SAMPLES);
    }

    #[test]
    fn hs2_jttqmy_token_window_rate_and_peak_book_roll_over_thirty_days() {
        let mut minutes = BTreeMap::new();
        minutes.insert(0, 500u64);
        minutes.insert(4 * MINUTE_MS, 500);
        minutes.insert(5 * MINUTE_MS, 1000);
        assert_eq!(window_rate(&minutes, 4 * MINUTE_MS), 200.0);
        // The first minute leaves the window.
        assert_eq!(window_rate(&minutes, 5 * MINUTE_MS), 300.0);
        assert_eq!(window_rate(&minutes, 20 * MINUTE_MS), 0.0);

        let mut book = PeakBook::default();
        assert!(!book.has_tokens());
        assert!(book.raise_tokens("2026-10-01", 50.0));
        assert!(
            !book.raise_tokens("2026-10-01", 40.0),
            "only a higher value raises"
        );
        assert!(book.raise_tokens("2026-11-05", 20.0));
        assert!(book.raise_tickets("2026-11-05", 3));
        assert!(!book.raise_tickets("2026-11-05", 3));
        assert!(book.has_tokens());
        // 2026-10-01 is inside the 30 days ending 2026-10-30, outside those ending 11-05.
        assert_eq!(book.peak_tokens("2026-10-30"), 50.0);
        assert_eq!(book.peak_tokens("2026-11-05"), 20.0);
        assert_eq!(book.peak_tickets("2026-11-05"), 3);
        assert_eq!(PeakBook::first_day("2026-11-05"), "2026-10-07");
        assert_eq!(PeakBook::first_day("not a day"), "not a day");
    }

    #[test]
    fn hs2_jttqmy_usage_tail_reads_only_appended_complete_lines_and_persists_peaks() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(
            dir.path().join("demo.hs2"),
            &hotsheet_ticketing::StoreMetadata::new("HS2"),
        )
        .unwrap();
        let now = at(1_800_000_000_000); // 2027-01-15T08:00:00Z
        let raw = store.root().join("metrics").join("raw");
        std::fs::create_dir_all(&raw).unwrap();
        let file = raw.join(format!("{}.jsonl", day_of(unix_ms(now))));
        let line = |ts: &str, tokens: u64| {
            format!(r#"{{"ts":"{ts}","tool":"claude","tokens_in":{tokens},"tokens_out":0}}"#)
        };
        std::fs::write(
            &file,
            format!(
                "{}\n{}\n{{\"ts\":\"2027-01-15T07:59:30Z\",\"tool\":\"claude\",\"tokens_in\":7",
                line("2027-01-15T07:58:10Z", 1000),
                line("2027-01-15T07:59:00Z", 1500)
            ),
        )
        .unwrap();
        let mut live = StoreLive::default();
        let rate = token_rate(&mut live, &store, now);
        assert_eq!(rate.window_minutes, 5);
        assert_eq!(
            rate.minutes.iter().map(|m| m.tokens).sum::<u64>(),
            2500,
            "the partial last line waits for its newline"
        );
        assert_eq!(rate.peak_per_minute, 500.0);
        // Finish the partial line and append a new one: only those are read.
        let mut text = std::fs::read_to_string(&file).unwrap();
        text.push_str(",\"tokens_out\":3}\n");
        text.push_str(&line("2027-01-15T08:00:00Z", 2500));
        text.push('\n');
        std::fs::write(&file, text).unwrap();
        let rate = token_rate(&mut live, &store, now);
        assert_eq!(rate.minutes.iter().map(|m| m.tokens).sum::<u64>(), 5010);
        assert_eq!(rate.peak_per_minute, 1002.0);
        // The peak survives a restart through the machine-local book.
        let saved = PeakBook::load(&store);
        assert_eq!(saved.peak_tokens(&day_of(unix_ms(now))), 1002.0);
        let ignore = std::fs::read_to_string(store.root().join(".gitignore")).unwrap();
        assert!(ignore.lines().any(|l| l.trim() == "metrics/local/"));
        // Ten minutes later the window is empty but the peak stays.
        let later = at(1_800_000_000_000 + 10 * MINUTE_MS);
        let rate = token_rate(&mut live, &store, later);
        assert!(rate.minutes.is_empty());
        assert_eq!(rate.peak_per_minute, 1002.0);
        // A missing raw directory is no usage, not an error.
        let empty = FsStore::init(
            dir.path().join("empty.hs2"),
            &hotsheet_ticketing::StoreMetadata::new("HS2"),
        )
        .unwrap();
        let rate = token_rate(&mut StoreLive::default(), &empty, now);
        assert!(rate.minutes.is_empty());
        assert_eq!(rate.peak_per_minute, 0.0);
    }

    #[test]
    fn hs2_jttqmy_material_change_ignores_small_progress_moves() {
        let base = LiveStoreState {
            store: "s".into(),
            root: "/r".into(),
            name: "n".into(),
            checkouts: Vec::new(),
            epoch_ms: 1,
            revision: 3,
            state: LiveWord::Active,
            activity: LiveActivityFacts {
                word: LiveWord::Active,
                active_since_ms: Some(0),
                stopped_at_ms: None,
                last_progress_ms: Some(100_000),
                idle_deadline_ms: Some(1_900_000),
                stalled: Vec::new(),
                halted: Vec::new(),
                causes: Vec::new(),
            },
            claims: Vec::new(),
            workers: LiveWorkers {
                live: 1,
                expected: 1,
            },
            waiting: LiveWaiting {
                count: 0,
                tickets: Vec::new(),
            },
            runway: LiveRunway {
                ready: 0,
                estimate_ms: None,
            },
            rates: LiveRates {
                tokens: LiveTokenRate {
                    window_minutes: 5,
                    minutes: Vec::new(),
                    peak_per_minute: 0.0,
                },
                tickets_updated: LiveUpdatedRate {
                    window_minutes: 60,
                    at_ms: Vec::new(),
                    peak_per_hour: 0,
                },
            },
        };
        let mut next = base.clone();
        next.revision = 0;
        assert!(
            !materially_changed(&base, &next),
            "revision alone is no change"
        );
        next.activity.last_progress_ms = Some(110_000);
        next.activity.idle_deadline_ms = Some(1_910_000);
        assert!(
            !materially_changed(&base, &next),
            "a 10 s progress move waits"
        );
        next.activity.last_progress_ms = Some(116_000);
        assert!(materially_changed(&base, &next));
        let mut word = base.clone();
        word.state = LiveWord::MayBeStuck;
        assert!(materially_changed(&base, &word));
        let mut cleared = base.clone();
        cleared.activity.idle_deadline_ms = None;
        assert!(materially_changed(&base, &cleared));
        let mut tokens = base.clone();
        tokens.rates.tokens.minutes.push(LiveTokenMinute {
            start_ms: 0,
            tokens: 1,
        });
        assert!(materially_changed(&base, &tokens));
        let mut cause = base.clone();
        cause.activity.causes.push(LiveCause {
            kind: LiveCauseKind::AwaitingInput,
            worker: "w".into(),
            terminal: Some("t".into()),
            since_ms: 0,
            resets_at_ms: None,
            prompt: Some(LivePrompt::Question),
        });
        assert!(materially_changed(&base, &cause), "a new cause pushes");
    }

    #[test]
    fn hs2_egkka0_causes_list_usage_limits_then_prompts_and_serialize_unknown_as_null() {
        use crate::project_activity::{HaltDetail, HaltNote, PromptNote};
        let now = at(1_800_000_000_000);
        let mut activity = crate::project_activity::ProjectActivity::default().live("none", now);
        assert!(causes(&activity).is_empty(), "nothing reported");
        let halt = |worker: &str, ms: i64, detail: HaltDetail| HaltNote {
            worker: worker.into(),
            message: "m".into(),
            at: at(ms),
            detail,
        };
        activity.halted = vec![
            halt(
                "w2",
                20,
                HaltDetail {
                    terminal: Some("t2".into()),
                    usage_limit: true,
                    resets_at: None,
                },
            ),
            // An overload halt is no usage-limit cause.
            halt("w3", 5, HaltDetail::default()),
            halt(
                "w1",
                10,
                HaltDetail {
                    terminal: Some("t1".into()),
                    usage_limit: true,
                    resets_at: Some(at(3_600_010)),
                },
            ),
        ];
        activity.prompts = vec![PromptNote {
            worker: "w4".into(),
            terminal: "t4".into(),
            kind: PromptKind::Permission,
            since: at(1),
        }];
        let list = causes(&activity);
        assert_eq!(
            list.iter().map(|c| c.worker.as_str()).collect::<Vec<_>>(),
            ["w1", "w2", "w4"]
        );
        assert_eq!(list[0].resets_at_ms, Some(3_600_010));
        assert_eq!(list[1].resets_at_ms, None);
        assert_eq!(list[2].kind, LiveCauseKind::AwaitingInput);
        let json = serde_json::to_value(&list).unwrap();
        assert_eq!(
            json[0],
            serde_json::json!({"kind":"usage_limit","worker":"w1","terminal":"t1","since_ms":10,
                "resets_at_ms":3_600_010,"prompt":null})
        );
        assert_eq!(json[1]["resets_at_ms"], serde_json::Value::Null, "unknown");
        assert_eq!(json[2]["kind"], "awaiting_input");
        assert_eq!(json[2]["prompt"], "permission");
        assert_eq!(json[2]["terminal"], "t4");
    }
}
