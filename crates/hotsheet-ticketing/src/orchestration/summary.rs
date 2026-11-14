//! HS2-S13719: progress and cost of an owning checkout's runs, across targets.
//!
//! A pure fold over records other modules keep. It reads nothing and writes nothing.
//! Every measure is a [`Metric`]: a value, or the reason it cannot be computed. An
//! unknown is never reported as zero, and an aggregate says how many of its parts
//! were unknown.
//!
//! Attribution. A work item is one target ticket, `(connection_id, native_id)`. Runs
//! that dispatch the same target ticket again, or reach it through a second target
//! whose checkout shares the ticket source, are the same work item. Its cost is
//! counted once and belongs to the target of its earliest run. A session is counted
//! once by its host's id for it, and belongs to the target of its earliest record.
//!
//! Identifiers, states, counts and durations only: no ticket text reaches this module.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{
    Gate, GateKind, GateState, MAX_RUN_HISTORY, OrchestratedSession, Run, RunControl, RunEvent,
    RunEventKind, RunState, ValidationEvidence, summarize,
};

/// The stable name of the attribution rule above, carried in every summary.
pub const PROGRESS_ATTRIBUTION: &str = "work_item_earliest_run";

/// Why a measure has no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    /// The run has not finished, so it has no lead time yet.
    RunUnfinished,
    /// The run was never claimed on its target.
    NeverDispatched,
    /// The run's bounded history no longer holds the entries the measure needs.
    HistoryTruncated,
    /// Retention removed run events from before the oldest one kept.
    EventsTrimmed,
    /// Recorded times run backwards, so a duration would be negative.
    ClockInconsistent,
    /// No CI gate stands for the run.
    NoGateLinked,
    /// No deployment was requested for the run.
    NoDeploymentRequested,
    /// A gate stands, with no trustworthy verdict.
    GateUnverified,
    /// The target has no reviewed destination on this machine.
    TargetUnbound,
    /// The target lives on another machine whose server does not offer the hosted cost
    /// read (HS2-B6236S); its usage records are not read from here.
    TargetRemote,
    /// The target ticket's source is not a Git store, which is where usage is kept.
    UsageSourceUnsupported,
    /// The usage records could not be opened or read.
    UsageStoreUnavailable,
    /// The usage records hold nothing for the target ticket.
    NoUsageRecorded,
    /// The caller supplied no cost reading for the work item.
    CostNotRead,
    /// HS2-B6236S: the remote target's server could not be asked for the cost, or did
    /// not answer with one.
    TargetUnreachable,
}

impl UnknownReason {
    pub const ALL: [Self; 15] = [
        Self::RunUnfinished,
        Self::NeverDispatched,
        Self::HistoryTruncated,
        Self::EventsTrimmed,
        Self::ClockInconsistent,
        Self::NoGateLinked,
        Self::NoDeploymentRequested,
        Self::GateUnverified,
        Self::TargetUnbound,
        Self::TargetRemote,
        Self::UsageSourceUnsupported,
        Self::UsageStoreUnavailable,
        Self::NoUsageRecorded,
        Self::CostNotRead,
        Self::TargetUnreachable,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::RunUnfinished => "run_unfinished",
            Self::NeverDispatched => "never_dispatched",
            Self::HistoryTruncated => "history_truncated",
            Self::EventsTrimmed => "events_trimmed",
            Self::ClockInconsistent => "clock_inconsistent",
            Self::NoGateLinked => "no_gate_linked",
            Self::NoDeploymentRequested => "no_deployment_requested",
            Self::GateUnverified => "gate_unverified",
            Self::TargetUnbound => "target_unbound",
            Self::TargetRemote => "target_remote",
            Self::UsageSourceUnsupported => "usage_source_unsupported",
            Self::UsageStoreUnavailable => "usage_store_unavailable",
            Self::NoUsageRecorded => "no_usage_recorded",
            Self::CostNotRead => "cost_not_read",
            Self::TargetUnreachable => "target_unreachable",
        }
    }
}

/// A measure that is known, or says why it is not.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Metric<T> {
    Known { value: T },
    Unknown { reason: UnknownReason },
}

impl<T> Metric<T> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known { value } => Some(value),
            Self::Unknown { .. } => None,
        }
    }

    pub fn reason(&self) -> Option<UnknownReason> {
        match self {
            Self::Known { .. } => None,
            Self::Unknown { reason } => Some(*reason),
        }
    }
}

fn unknown<T>(reason: UnknownReason) -> Metric<T> {
    Metric::Unknown { reason }
}

/// The target ticket a run works on, whichever target reached it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WorkItemKey {
    pub connection_id: String,
    pub native_id: String,
}

impl WorkItemKey {
    pub fn of(run: &Run) -> Self {
        Self {
            connection_id: run.target.connection_id.clone(),
            native_id: run.target.native_id.clone(),
        }
    }
}

/// What the usage records say one work item cost.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CostReading {
    pub cost_usd: f64,
    /// Usage events still held as raw records.
    pub events: u64,
    /// Of those, events with no cost. When above zero, `cost_usd` is a lower bound.
    pub unpriced_events: u64,
}

/// Time a run could not advance: paused, taken over by a person, or its target
/// unreachable. `total_ms` counts overlapping causes once, so it can be less than the
/// sum of the parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedTime {
    pub total_ms: u64,
    pub paused_ms: u64,
    pub intervened_ms: u64,
    pub unreachable_ms: u64,
}

/// Repeated work on one run. Counts of retained records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRetries {
    /// Validation attempts after the first.
    pub validation: u64,
    /// Sessions started to replace a lost or failed one.
    pub session_replacements: u64,
    /// Times the run was handed to another worker.
    pub handoffs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunProgress {
    pub run_id: String,
    pub target_id: String,
    pub connection_id: String,
    pub native_id: String,
    pub state: RunState,
    /// 1 for the first run of its work item, 2 for the next dispatch, and so on.
    pub attempt: u64,
    pub created_at_ms: u64,
    /// From the run's creation to the state it finished in.
    pub lead_time_ms: Metric<u64>,
    /// From the run's creation to its first claim on the target.
    pub queue_wait_ms: Metric<u64>,
    /// Measured to the finish, or to `as_of_ms` for an unfinished run.
    pub blocked: Metric<BlockedTime>,
    pub retries: RunRetries,
    pub ci: Metric<GateState>,
    pub deployment: Metric<GateState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkItemProgress {
    pub connection_id: String,
    pub native_id: String,
    /// The target of the earliest run: the one this item's cost counts toward.
    pub attributed_target_id: String,
    /// Every target a run reached this item through, sorted.
    pub target_ids: Vec<String>,
    /// Oldest first.
    pub run_ids: Vec<String>,
    /// Runs after the first.
    pub redispatches: u64,
    pub cost: Metric<CostReading>,
}

/// Durations of several runs. `total_ms` and `max_ms` cover the `known` runs only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurationAggregate {
    pub known: u64,
    pub unknown: u64,
    pub total_ms: u64,
    pub max_ms: u64,
    /// Reason code to how many runs gave it.
    pub unknown_reasons: BTreeMap<String, u64>,
}

impl DurationAggregate {
    fn add(&mut self, metric: &Metric<u64>) {
        match metric {
            Metric::Known { value } => {
                self.known += 1;
                self.total_ms = self.total_ms.saturating_add(*value);
                self.max_ms = self.max_ms.max(*value);
            }
            Metric::Unknown { reason } => {
                self.unknown += 1;
                *self
                    .unknown_reasons
                    .entry(reason.code().into())
                    .or_default() += 1;
            }
        }
    }
}

/// Gate verdicts of several runs: state code to count, and reason code to count for
/// the runs with no verdict.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeTally {
    pub known: BTreeMap<String, u64>,
    pub unknown: BTreeMap<String, u64>,
}

impl OutcomeTally {
    fn add(&mut self, metric: &Metric<GateState>) {
        let (map, code) = match metric {
            Metric::Known { value } => (&mut self.known, value.code()),
            Metric::Unknown { reason } => (&mut self.unknown, reason.code()),
        };
        *map.entry(code.into()).or_default() += 1;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryTotals {
    pub redispatches: u64,
    pub validation: u64,
    pub session_replacements: u64,
    pub handoffs: u64,
}

/// Cost of several work items. `known_usd` is a total only when `complete`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CostAggregate {
    pub known_usd: f64,
    /// Every item has a reading and no counted event lacks a cost.
    pub complete: bool,
    pub items_known: u64,
    pub items_unknown: u64,
    pub events: u64,
    pub unpriced_events: u64,
    /// Reason code to how many work items gave it.
    pub unknown_reasons: BTreeMap<String, u64>,
}

impl CostAggregate {
    fn add(&mut self, metric: &Metric<CostReading>) {
        match metric {
            Metric::Known { value } => {
                self.items_known += 1;
                self.known_usd += value.cost_usd;
                self.events += value.events;
                self.unpriced_events += value.unpriced_events;
            }
            Metric::Unknown { reason } => {
                self.items_unknown += 1;
                *self
                    .unknown_reasons
                    .entry(reason.code().into())
                    .or_default() += 1;
            }
        }
    }

    fn finish(&mut self) {
        self.complete = self.items_unknown == 0 && self.unpriced_events == 0;
    }
}

/// The figures of one target, or of every target together. Each run, work item and
/// session is in exactly one target's figures, so the targets add up to the totals.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgressRollup {
    pub runs: u64,
    /// State code to count.
    pub runs_by_state: BTreeMap<String, u64>,
    pub work_items: u64,
    pub sessions: u64,
    pub lead_time: DurationAggregate,
    pub queue_wait: DurationAggregate,
    pub blocked: DurationAggregate,
    pub retries: RetryTotals,
    pub ci: OutcomeTally,
    pub deployment: OutcomeTally,
    pub cost: CostAggregate,
}

impl ProgressRollup {
    fn add_run(&mut self, run: &RunProgress) {
        self.runs += 1;
        *self
            .runs_by_state
            .entry(state_code(run.state).into())
            .or_default() += 1;
        self.lead_time.add(&run.lead_time_ms);
        self.queue_wait.add(&run.queue_wait_ms);
        self.blocked.add(&match &run.blocked {
            Metric::Known { value } => Metric::Known {
                value: value.total_ms,
            },
            Metric::Unknown { reason } => unknown(*reason),
        });
        self.retries.validation += run.retries.validation;
        self.retries.session_replacements += run.retries.session_replacements;
        self.retries.handoffs += run.retries.handoffs;
        self.ci.add(&run.ci);
        self.deployment.add(&run.deployment);
    }

    fn add_item(&mut self, item: &WorkItemProgress) {
        self.work_items += 1;
        self.retries.redispatches += item.redispatches;
        self.cost.add(&item.cost);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetProgress {
    pub target_id: String,
    #[serde(flatten)]
    pub figures: ProgressRollup,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressSummary {
    pub owner_checkout_id: String,
    /// Open intervals are measured to this time.
    pub as_of_ms: u64,
    /// [`PROGRESS_ATTRIBUTION`].
    pub attribution: String,
    /// Retention removed run events; affected runs report `events_trimmed`.
    pub events_trimmed: bool,
    pub totals: ProgressRollup,
    /// Sorted by target id.
    pub targets: Vec<TargetProgress>,
    /// Sorted by connection id, then native id.
    pub work_items: Vec<WorkItemProgress>,
    /// Oldest first.
    pub runs: Vec<RunProgress>,
}

/// Everything a summary is computed from. Records of other owners are ignored.
pub struct ProgressInput<'a> {
    pub owner_checkout_id: &'a str,
    pub now_ms: u64,
    pub runs: &'a [Run],
    pub gates: &'a [Gate],
    pub validations: &'a [ValidationEvidence],
    pub sessions: &'a [OrchestratedSession],
    pub events: &'a [RunEvent],
    /// Retention removed events older than the ones given.
    pub events_trimmed: bool,
    /// One reading per work item. A missing one reports `cost_not_read`.
    pub costs: &'a BTreeMap<WorkItemKey, Metric<CostReading>>,
}

fn state_code(state: RunState) -> &'static str {
    match state {
        RunState::Requested => "requested",
        RunState::Queued => "queued",
        RunState::Dispatched => "dispatched",
        RunState::Completed => "completed",
        RunState::Failed => "failed",
        RunState::Cancelled => "cancelled",
        RunState::Intervened => "intervened",
    }
}

/// Spans of time as `(start, end)` in milliseconds.
type Intervals = Vec<(u64, u64)>;

/// Closed intervals, clipped to `[from, to]`. `None` when one ends before it starts.
fn clip(intervals: &[(u64, u64)], from: u64, to: u64) -> Option<Intervals> {
    let mut clipped = Vec::new();
    for (start, end) in intervals {
        if end < start {
            return None;
        }
        let (start, end) = ((*start).clamp(from, to), (*end).clamp(from, to));
        if end > start {
            clipped.push((start, end));
        }
    }
    Some(clipped)
}

/// The time covered by any of the intervals, counting an overlap once.
fn covered(mut intervals: Intervals) -> u64 {
    intervals.sort_unstable();
    let mut total = 0;
    let mut open: Option<(u64, u64)> = None;
    for (start, end) in intervals {
        open = match open {
            Some((from, to)) if start <= to => Some((from, to.max(end))),
            Some((from, to)) => {
                total += to - from;
                Some((start, end))
            }
            None => Some((start, end)),
        };
    }
    total + open.map_or(0, |(from, to)| to - from)
}

struct RunTimes {
    /// When the run reached the state it finished in.
    finished_at_ms: Option<u64>,
    truncated: bool,
}

fn run_times(run: &Run) -> RunTimes {
    RunTimes {
        finished_at_ms: run
            .history
            .iter()
            .rev()
            .find(|entry| entry.to.is_terminal() && entry.from != Some(entry.to))
            .map(|entry| entry.at_ms)
            .filter(|_| run.state.is_terminal()),
        // The registry drops the second-oldest entry once the history is full.
        truncated: run.history.len() >= MAX_RUN_HISTORY,
    }
}

fn lead_time(run: &Run, times: &RunTimes) -> Metric<u64> {
    if !run.state.is_terminal() {
        return unknown(UnknownReason::RunUnfinished);
    }
    match times.finished_at_ms {
        None => unknown(UnknownReason::HistoryTruncated),
        Some(at) if at < run.created_at_ms => unknown(UnknownReason::ClockInconsistent),
        Some(at) => Metric::Known {
            value: at - run.created_at_ms,
        },
    }
}

fn queue_wait(run: &Run, times: &RunTimes) -> Metric<u64> {
    let first = run
        .history
        .iter()
        .find(|entry| entry.to == RunState::Dispatched && entry.from != Some(entry.to));
    // The first claim may be one of the entries a full history dropped, in which case
    // the earliest one kept would overstate the wait.
    if times.truncated {
        return unknown(UnknownReason::HistoryTruncated);
    }
    match first {
        None => unknown(UnknownReason::NeverDispatched),
        Some(entry) if entry.at_ms < run.created_at_ms => unknown(UnknownReason::ClockInconsistent),
        Some(entry) => Metric::Known {
            value: entry.at_ms - run.created_at_ms,
        },
    }
}

/// Intervals in which the run was paused, and in which a person had taken it over,
/// in the order the history recorded them. An interval still open ends at `until`.
fn history_intervals(run: &Run, until: u64) -> (Intervals, Intervals) {
    let (mut paused, mut intervened) = (Vec::new(), Vec::new());
    let (mut pause_from, mut intervened_from) = (None, None);
    for entry in &run.history {
        match entry.control {
            // Pausing a paused run keeps when it was first paused.
            Some(RunControl::Pause) => pause_from = pause_from.or(Some(entry.at_ms)),
            Some(RunControl::Resume) => {
                if let Some(from) = pause_from.take() {
                    paused.push((from, entry.at_ms));
                }
            }
            _ => {}
        }
        if entry.from != Some(entry.to) {
            if entry.to == RunState::Intervened {
                intervened_from = intervened_from.or(Some(entry.at_ms));
            } else if let Some(from) = intervened_from.take() {
                intervened.push((from, entry.at_ms));
            }
            // A finished run is no longer paused.
            if entry.to.is_terminal() {
                if let Some(from) = pause_from.take() {
                    paused.push((from, entry.at_ms));
                }
            }
        }
    }
    paused.extend(pause_from.map(|from| (from, until.max(from))));
    intervened.extend(intervened_from.map(|from| (from, until.max(from))));
    (paused, intervened)
}

/// Intervals in which the run's target could not be confirmed, from its events in the
/// order the facts were true. A repeated report does not restart an outage.
fn unreachable_intervals(events: &[&RunEvent], until: u64) -> Intervals {
    let mut ordered: Vec<&RunEvent> = events.to_vec();
    ordered.sort_by_key(|event| event.at_ms);
    let mut intervals = Vec::new();
    let mut from = None;
    for event in ordered {
        match event.kind {
            RunEventKind::TargetUnreachable => from = from.or(Some(event.at_ms)),
            RunEventKind::TargetReachable => {
                if let Some(start) = from.take() {
                    intervals.push((start, event.at_ms));
                }
            }
            _ => {}
        }
    }
    intervals.extend(from.map(|start| (start, until.max(start))));
    intervals
}

fn blocked(
    run: &Run,
    times: &RunTimes,
    events: &[&RunEvent],
    oldest_event_ms: Option<u64>,
    input: &ProgressInput<'_>,
) -> Metric<BlockedTime> {
    if times.truncated {
        return unknown(UnknownReason::HistoryTruncated);
    }
    // An outage of a run older than the oldest event kept may have been removed.
    if input.events_trimmed && oldest_event_ms.is_none_or(|oldest| run.created_at_ms < oldest) {
        return unknown(UnknownReason::EventsTrimmed);
    }
    let until = match (run.state.is_terminal(), times.finished_at_ms) {
        (true, Some(at)) => at,
        (true, None) => return unknown(UnknownReason::HistoryTruncated),
        (false, _) => input.now_ms,
    };
    if until < run.created_at_ms {
        return unknown(UnknownReason::ClockInconsistent);
    }
    let (paused, intervened) = history_intervals(run, until);
    let unreachable = unreachable_intervals(events, until);
    let clipped = |intervals: &[(u64, u64)]| clip(intervals, run.created_at_ms, until);
    let (Some(paused), Some(intervened), Some(unreachable)) = (
        clipped(&paused),
        clipped(&intervened),
        clipped(&unreachable),
    ) else {
        return unknown(UnknownReason::ClockInconsistent);
    };
    let all = [paused.clone(), intervened.clone(), unreachable.clone()].concat();
    Metric::Known {
        value: BlockedTime {
            total_ms: covered(all),
            paused_ms: covered(paused),
            intervened_ms: covered(intervened),
            unreachable_ms: covered(unreachable),
        },
    }
}

fn gate_outcomes(run: &Run, gates: &[Gate]) -> (Metric<GateState>, Metric<GateState>) {
    let own: Vec<Gate> = gates
        .iter()
        .filter(|gate| gate.run_id == run.run_id && gate.owner_checkout_id == run.owner_checkout_id)
        .cloned()
        .collect();
    let summary = summarize(own);
    let verdict = |state: GateState| match state {
        GateState::Unknown | GateState::Superseded => unknown(UnknownReason::GateUnverified),
        state => Metric::Known { value: state },
    };
    let has_ci = summary.gates.iter().any(|gate| gate.kind == GateKind::Ci);
    (
        if has_ci {
            verdict(summary.ci)
        } else {
            unknown(UnknownReason::NoGateLinked)
        },
        summary
            .deployment
            .map_or(unknown(UnknownReason::NoDeploymentRequested), verdict),
    )
}

fn retries(run: &Run, input: &ProgressInput<'_>) -> RunRetries {
    let mut attempts = BTreeSet::new();
    for evidence in input.validations.iter().filter(|evidence| {
        evidence.step.owner_checkout_id == run.owner_checkout_id
            && evidence.step.ticket == run.ticket
    }) {
        for result in evidence.history.iter().chain(&evidence.latest) {
            if result.run_id.as_deref() == Some(run.run_id.as_str()) && result.attempt > 1 {
                attempts.insert(result.result_id.as_str());
            }
        }
    }
    let replaced: BTreeSet<&str> = input
        .sessions
        .iter()
        .filter(|session| {
            session.owner_checkout_id == run.owner_checkout_id
                && session.run_id.as_deref() == Some(run.run_id.as_str())
                && session.replaces.is_some()
        })
        .map(|session| session.session_id.as_str())
        .collect();
    RunRetries {
        validation: attempts.len() as u64,
        session_replacements: replaced.len() as u64,
        handoffs: run
            .history
            .iter()
            .filter(|entry| entry.control == Some(RunControl::Handoff))
            .count() as u64,
    }
}

/// Each distinct session's target: one hosted session known through several records
/// belongs to the target of the earliest of them.
fn session_targets(input: &ProgressInput<'_>) -> BTreeMap<(String, String), String> {
    let mut records: Vec<&OrchestratedSession> = input
        .sessions
        .iter()
        .filter(|session| session.owner_checkout_id == input.owner_checkout_id)
        .collect();
    records.sort_by(|left, right| {
        (left.created_at_ms, &left.session_id).cmp(&(right.created_at_ms, &right.session_id))
    });
    let mut targets = BTreeMap::new();
    for session in records {
        let key = match &session.hosted {
            Some(hosted) => (hosted.server_id.clone(), hosted.session_id.clone()),
            None => (String::new(), session.session_id.clone()),
        };
        targets
            .entry(key)
            .or_insert_with(|| session.target_id.clone());
    }
    targets
}

/// Summarize the owner's runs. The answer depends only on the records given, not on
/// the order they are given in.
pub fn summarize_progress(input: &ProgressInput<'_>) -> ProgressSummary {
    let mut owned: Vec<&Run> = input
        .runs
        .iter()
        .filter(|run| run.owner_checkout_id == input.owner_checkout_id)
        .collect();
    owned.sort_by(|left, right| {
        (left.created_at_ms, &left.run_id).cmp(&(right.created_at_ms, &right.run_id))
    });
    // A run given twice is one run.
    let mut seen = BTreeSet::new();
    owned.retain(|run| seen.insert(run.run_id.as_str()));
    let oldest_event_ms = input.events.iter().map(|event| event.at_ms).min();

    let mut items: BTreeMap<WorkItemKey, WorkItemProgress> = BTreeMap::new();
    let mut runs = Vec::with_capacity(owned.len());
    for run in owned {
        let key = WorkItemKey::of(run);
        let item = items
            .entry(key.clone())
            .or_insert_with(|| WorkItemProgress {
                connection_id: key.connection_id.clone(),
                native_id: key.native_id.clone(),
                attributed_target_id: run.target.target_id.clone(),
                target_ids: Vec::new(),
                run_ids: Vec::new(),
                redispatches: 0,
                cost: input
                    .costs
                    .get(&key)
                    .copied()
                    .unwrap_or(unknown(UnknownReason::CostNotRead)),
            });
        item.run_ids.push(run.run_id.clone());
        item.redispatches = item.run_ids.len() as u64 - 1;
        if !item.target_ids.contains(&run.target.target_id) {
            item.target_ids.push(run.target.target_id.clone());
            item.target_ids.sort();
        }
        let times = run_times(run);
        let events: Vec<&RunEvent> = input
            .events
            .iter()
            .filter(|event| event.run_id.as_deref() == Some(run.run_id.as_str()))
            .collect();
        let (ci, deployment) = gate_outcomes(run, input.gates);
        runs.push(RunProgress {
            run_id: run.run_id.clone(),
            target_id: run.target.target_id.clone(),
            connection_id: key.connection_id,
            native_id: key.native_id,
            state: run.state,
            attempt: item.run_ids.len() as u64,
            created_at_ms: run.created_at_ms,
            lead_time_ms: lead_time(run, &times),
            queue_wait_ms: queue_wait(run, &times),
            blocked: blocked(run, &times, &events, oldest_event_ms, input),
            retries: retries(run, input),
            ci,
            deployment,
        });
    }

    let mut totals = ProgressRollup::default();
    let mut targets: BTreeMap<String, ProgressRollup> = BTreeMap::new();
    for run in &runs {
        totals.add_run(run);
        targets
            .entry(run.target_id.clone())
            .or_default()
            .add_run(run);
    }
    for item in items.values() {
        totals.add_item(item);
        targets
            .entry(item.attributed_target_id.clone())
            .or_default()
            .add_item(item);
    }
    for target_id in session_targets(input).into_values() {
        totals.sessions += 1;
        targets.entry(target_id).or_default().sessions += 1;
    }
    totals.cost.finish();
    ProgressSummary {
        owner_checkout_id: input.owner_checkout_id.into(),
        as_of_ms: input.now_ms,
        attribution: PROGRESS_ATTRIBUTION.into(),
        events_trimmed: input.events_trimmed,
        totals,
        targets: targets
            .into_iter()
            .map(|(target_id, mut figures)| {
                figures.cost.finish();
                TargetProgress { target_id, figures }
            })
            .collect(),
        work_items: items.into_values().collect(),
        runs,
    }
}
