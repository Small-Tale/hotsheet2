//! HS2-KXKA7X: scheduled and event triggers of an orchestration project.
//!
//! A trigger is a reviewed binding of **when** (a schedule, or an event in one declared
//! project: a ticket opened, a run's CI gate failed, a branch's CI failed, a commit
//! landed on a branch) to **what** (apply one plan template of HS2-A4RVYA, start or
//! pause placement, or both). It never dispatches, pushes or deploys itself: a
//! template apply hands a goal to the planner and the scheduler places the steps
//! within the policy's limits.
//!
//! Triggers are machine-local, like command schedules (the maintainer's decision "On
//! one machine"): `${HOTSHEET_HOME}/orchestration-triggers/<owner checkout id>.json`,
//! private, locked, replaced atomically, schema 1, at most [`MAX_TRIGGERS`]. Only the
//! machine where a trigger is On can fire it.
//!
//! This module holds the pure parts: the model and its validator, delivery keys and
//! the operation id a key becomes (so a redelivered event is the same template apply
//! all the way down to the goal ticket), the delivery decision (ignore, already
//! handled, dry run, apply, skip or queue while a run is active), state transitions,
//! the review digest and the bounded, coalescing history.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::contract::valid_identifier;
use super::storage::{Storage, decode};
use super::*;
use crate::command_schedules::Recurrence;

/// The directory under the machine home that holds one trigger file per orchestration
/// project.
pub const TRIGGER_DIR: &str = "orchestration-triggers";
/// The triggers one orchestration project keeps on this machine.
pub const MAX_TRIGGERS: usize = 64;
/// The history entries one trigger keeps; consecutive skips coalesce into one.
pub const MAX_TRIGGER_HISTORY: usize = 64;
/// The delivery keys one trigger remembers as handled, beyond its history.
pub const MAX_HANDLED_KEYS: usize = 256;
/// The fixed and event-filled inputs one trigger sets.
pub const MAX_TRIGGER_INPUTS: usize = 16;
/// A name, tag, branch, workflow or repository.
pub const MAX_TRIGGER_LINE: usize = 200;
const MAX_TRIGGERS_BYTES: usize = 4 * 1024 * 1024;
/// A delivery key's longest form.
pub const MAX_DELIVERY_KEY: usize = 300;
/// The prefix of the actor id a trigger acts as (`trigger-<trigger id>`).
pub const TRIGGER_ACTOR_PREFIX: &str = "trigger-";
/// The prefix of the operation id a delivery applies its template under.
pub const TRIGGER_OPERATION_PREFIX: &str = "trg-";

/// Why a trigger request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerError {
    /// No trigger has that id on this machine.
    NotFound,
    /// The definition is not valid (its `problems` say why).
    Invalid,
    /// The trigger is paused or waits for a review; it does not fire.
    NotFiring,
    /// The trigger's template is missing or retired.
    TemplateMissing,
}

impl TriggerError {
    pub const ALL: [Self; 4] = [
        Self::NotFound,
        Self::Invalid,
        Self::NotFiring,
        Self::TemplateMissing,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::NotFound => "trigger_not_found",
            Self::Invalid => "trigger_invalid",
            Self::NotFiring => "trigger_not_firing",
            Self::TemplateMissing => "trigger_template_missing",
        }
    }
}

/// What a trigger does with a delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerState {
    /// Each delivery applies the action.
    On,
    /// Each delivery records what it would have done; nothing is written and nobody
    /// is asked. New triggers start here.
    DryRun,
    /// Deliveries are ignored. Resuming never backfills.
    Paused,
    /// The template changed since a person reviewed the trigger; it does not fire
    /// until reviewed again.
    NeedsReview,
}

impl TriggerState {
    pub const ALL: [Self; 4] = [Self::On, Self::DryRun, Self::Paused, Self::NeedsReview];

    pub fn code(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::DryRun => "dry_run",
            Self::Paused => "paused",
            Self::NeedsReview => "needs_review",
        }
    }

    /// Whether deliveries are taken (applied or recorded as a dry run).
    pub fn fires(self) -> bool {
        matches!(self, Self::On | Self::DryRun)
    }
}

/// A trigger's schedule: the command-schedule recurrence in a time zone at a local time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerSchedule {
    pub time_zone: String,
    /// `HH:mm`.
    pub local_time: String,
    pub recurrence: Recurrence,
}

/// The branch a branch watch follows, and where its CI runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchWatch {
    /// The CI provider, such as `gitlab-ci`; absent means `github-actions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// `owner/name`, as the target checkout's remotes name it.
    pub repository: String,
    pub workflow: String,
    pub branch: String,
}

impl BranchWatch {
    pub fn provider(&self) -> &str {
        self.provider.as_deref().unwrap_or("github-actions")
    }
}

/// When a trigger fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerSource {
    /// At each occurrence of a schedule. Missed occurrences run once on return.
    Schedule { schedule: TriggerSchedule },
    /// A ticket opened in a project, optionally only with a tag or category.
    TicketOpened {
        target_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tag: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
    },
    /// A CI gate of one of the project's own runs failed, optionally only in one
    /// project.
    GateFailed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_id: Option<String>,
    },
    /// The CI of a project's branch failed.
    BranchCiFailed {
        target_id: String,
        watch: BranchWatch,
    },
    /// A commit landed on a project's branch.
    CommitLanded {
        target_id: String,
        watch: BranchWatch,
    },
}

impl TriggerSource {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Schedule { .. } => "schedule",
            Self::TicketOpened { .. } => "ticket_opened",
            Self::GateFailed { .. } => "gate_failed",
            Self::BranchCiFailed { .. } => "branch_ci_failed",
            Self::CommitLanded { .. } => "commit_landed",
        }
    }

    /// The project the source watches, when it watches one.
    pub fn target_id(&self) -> Option<&str> {
        match self {
            Self::Schedule { .. } => None,
            Self::GateFailed { target_id } => target_id.as_deref(),
            Self::TicketOpened { target_id, .. }
            | Self::BranchCiFailed { target_id, .. }
            | Self::CommitLanded { target_id, .. } => Some(target_id),
        }
    }

    /// The branch watch, for a branch source.
    pub fn branch(&self) -> Option<&BranchWatch> {
        match self {
            Self::BranchCiFailed { watch, .. } | Self::CommitLanded { watch, .. } => Some(watch),
            _ => None,
        }
    }

    /// The event values a delivery of this source carries.
    pub fn event_fields(&self) -> &'static [EventField] {
        match self {
            Self::Schedule { .. } => &[EventField::Occurrence],
            Self::TicketOpened { .. } => &[EventField::Ticket],
            Self::GateFailed { .. } => &[EventField::Commit, EventField::Gate, EventField::Ticket],
            Self::BranchCiFailed { .. } => {
                &[EventField::Commit, EventField::CiRun, EventField::Branch]
            }
            Self::CommitLanded { .. } => &[EventField::Commit, EventField::Branch],
        }
    }
}

/// An event value a template input is filled from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventField {
    /// The commit: a failed or landed commit, or the gate's commit.
    Commit,
    /// The CI run, as `<run id>` or `<run id>.<attempt>`.
    CiRun,
    /// The ticket, as `<connection>:<native id>` (or a run's target ticket).
    Ticket,
    /// The branch.
    Branch,
    /// The schedule occurrence, `YYYY-MM-DD` or `YYYY-MM-DDTHH:mm`.
    Occurrence,
    /// The gate id of a failed run gate.
    Gate,
}

impl EventField {
    pub fn code(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::CiRun => "ci_run",
            Self::Ticket => "ticket",
            Self::Branch => "branch",
            Self::Occurrence => "occurrence",
            Self::Gate => "gate",
        }
    }
}

/// Apply this template on each delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerTemplate {
    pub template_id: String,
    /// Fixed inputs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Value>,
    /// Inputs filled from each delivery's event.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub event_inputs: BTreeMap<String, EventField>,
    /// The AI tool a new planner runs in.
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// Start or pause placement (the orchestration's Start/Pause, HS2-TZJYKH).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerPlacement {
    Start,
    Pause,
}

impl TriggerPlacement {
    pub fn automatic(self) -> bool {
        self == Self::Start
    }
}

/// What a delivery does: apply a template, set placement, or both (template first).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<TriggerTemplate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<TriggerPlacement>,
}

/// What a delivery does while the trigger's last run is still working.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overlap {
    /// Skip it and record the skip (consecutive skips coalesce).
    #[default]
    Skip,
    /// Keep the latest one and run it once the current run finishes.
    QueueLatest,
}

/// The reviewed definition of a trigger: what a person turns On.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerSpec {
    pub name: String,
    pub source: TriggerSource,
    pub action: TriggerAction,
    #[serde(default)]
    pub overlap: Overlap,
}

/// Who turned a trigger On, and what they reviewed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerReview {
    /// `human`, or `ai` when an AI session turned it On after a person's answer.
    pub role: String,
    pub actor_id: String,
    pub at_ms: u64,
    /// [`spec_digest`] of the definition and template version reviewed.
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_version: Option<u32>,
    /// The permission requests a person answered for an AI's turn On.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_requests: Vec<String>,
}

/// An AI session's turn On that waits for a person's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingOn {
    pub operation_id: String,
    pub digest: String,
    pub actor_id: String,
    pub requests: Vec<String>,
    pub at_ms: u64,
}

/// The kind of a delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryKind {
    Schedule,
    TicketOpened,
    GateFailed,
    CiFailed,
    CommitLanded,
    /// Fired by hand (`trigger fire`).
    Manual,
}

/// One delivery: an occurrence or an event, under its deterministic key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub key: String,
    pub kind: DeliveryKind,
    pub at_ms: u64,
    /// Identifiers only, by [`EventField`] code.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub evidence: BTreeMap<String, String>,
    /// Occurrences missed while this machine was off, coalesced into this one.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub missed: u32,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// What became of a delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryOutcome {
    /// The action was taken (the template apply may still wait on a permission).
    Applied,
    /// Recorded what would have run; nothing was written.
    DryRun,
    /// Skipped: the last run is still working.
    SkippedOverlap,
    /// Kept to run once the last run finishes (replacing an earlier queued one).
    Queued,
    /// The same delivery again: nothing more was done.
    AlreadyHandled,
    /// The template changed: the trigger waits for a review.
    NeedsReview,
    /// The action was refused (`reason` says why).
    Refused,
}

impl DeliveryOutcome {
    pub fn code(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::DryRun => "dry_run",
            Self::SkippedOverlap => "skipped_overlap",
            Self::Queued => "queued",
            Self::AlreadyHandled => "already_handled",
            Self::NeedsReview => "needs_review",
            Self::Refused => "refused",
        }
    }
}

/// What a dry-run delivery would have done. Identifiers and counts only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WouldRun {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_title: Option<String>,
    #[serde(default)]
    pub steps: u32,
    /// The projects the template's steps name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    /// The projects where it would ask through the permission card.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<TriggerPlacement>,
}

/// One entry of a trigger's history, newest last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerHistoryEntry {
    pub delivery_key: String,
    pub kind: DeliveryKind,
    pub at_ms: u64,
    pub outcome: DeliveryOutcome,
    /// Coalesced repeats of this outcome (skips, redeliveries); 1 for one.
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub evidence: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub missed: u32,
    /// The template apply's operation id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// The goal ticket the apply opened, once known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<TriggerPlacement>,
    /// A refusal or wait code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub would: Option<WouldRun>,
}

fn one() -> u32 {
    1
}

/// The run a trigger's last applied delivery started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveRun {
    pub delivery_key: String,
    pub operation_id: String,
    pub at_ms: u64,
}

/// Where each source continues from. Reset whenever the trigger starts firing again,
/// so turning it On, back to Dry Run or resuming never backfills.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerCursor {
    /// When the trigger last started firing. A ticket created before it is not new.
    #[serde(default)]
    pub armed_at_ms: u64,
    /// A schedule's occurrences after this instant are due.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_after_ms: Option<u64>,
    /// The run event journal's cursor (gate events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_events: Option<u64>,
    /// The branch head the branch watch follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_commit: Option<String>,
    /// HS2-W8S3TV: the branch head a `commit_landed` trigger last saw where the project
    /// lives (its remote-tracking ref), read from the branch itself, not from CI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_head: Option<String>,
}

/// A source that cannot be followed right now. Shown on the trigger and announced
/// once; there is no failover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerWarning {
    pub code: String,
    pub since_ms: u64,
    #[serde(default)]
    pub notified: bool,
}

/// One trigger, with its machine-local state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    pub trigger_id: String,
    pub name: String,
    pub source: TriggerSource,
    pub action: TriggerAction,
    #[serde(default)]
    pub overlap: Overlap,
    pub state: TriggerState,
    /// The state a resume returns to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_from: Option<TriggerState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<TriggerReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_on: Option<PendingOn>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default)]
    pub cursor: TriggerCursor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<ActiveRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued: Option<Delivery>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<TriggerHistoryEntry>,
    /// Delivery keys handled, newest last, at most [`MAX_HANDLED_KEYS`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handled: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<TriggerWarning>,
    /// The template version whose review notice was sent, so it is sent once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_notified: Option<u32>,
}

/// One problem with a trigger definition: a stable code and where.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TriggerProblem {
    pub code: String,
    pub at: String,
}

fn problem(code: &str, at: impl Into<String>) -> TriggerProblem {
    TriggerProblem {
        code: code.into(),
        at: at.into(),
    }
}

fn line_ok(text: &str) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= MAX_TRIGGER_LINE
        && !text.chars().any(char::is_control)
}

impl Trigger {
    /// The reviewed definition.
    pub fn spec(&self) -> TriggerSpec {
        TriggerSpec {
            name: self.name.clone(),
            source: self.source.clone(),
            action: self.action.clone(),
            overlap: self.overlap,
        }
    }

    /// The actor a trigger's deliveries act as: a system actor named for it, so its
    /// template applies ask through the permission card like any orchestrator action.
    pub fn actor_id(&self) -> String {
        format!("{TRIGGER_ACTOR_PREFIX}{}", self.trigger_id)
    }

    /// Whether this delivery key was handled before (applied, recorded, skipped,
    /// queued or running).
    pub fn handled(&self, key: &str) -> bool {
        self.handled.iter().any(|known| known == key)
            || self
                .active
                .as_ref()
                .is_some_and(|active| active.delivery_key == key)
            || self.queued.as_ref().is_some_and(|queued| queued.key == key)
    }

    /// Whether the review on record still covers the definition and template version.
    pub fn reviewed(&self, template_version: Option<u32>) -> bool {
        self.review
            .as_ref()
            .is_some_and(|review| review.digest == spec_digest(&self.spec(), template_version))
    }

    /// Start firing again: every cursor starts now, so nothing earlier is delivered.
    pub fn arm(&mut self, now_ms: u64, run_events: Option<u64>) {
        self.cursor = TriggerCursor {
            armed_at_ms: now_ms,
            schedule_after_ms: Some(now_ms),
            run_events,
            branch_commit: None,
            branch_head: None,
        };
        self.queued = None;
        self.warning = None;
    }

    /// Record a delivery's outcome: the key becomes handled, and a skip or a
    /// redelivery right after the same outcome coalesces into that entry.
    pub fn record(&mut self, entry: TriggerHistoryEntry) {
        // A queued delivery is not handled yet: it is the one `queued` delivery, and it
        // is taken once the run it waits for ends.
        if entry.outcome != DeliveryOutcome::Queued {
            self.mark_handled(&entry.delivery_key);
        }
        if let Some(last) = self.history.last_mut()
            && coalesces(last, &entry)
        {
            last.count = last.count.saturating_add(entry.count.max(1));
            last.at_ms = entry.at_ms;
            last.evidence = entry.evidence;
            if entry.outcome == DeliveryOutcome::SkippedOverlap {
                last.delivery_key = entry.delivery_key;
            }
            return;
        }
        self.history.push(entry);
        if self.history.len() > MAX_TRIGGER_HISTORY {
            let excess = self.history.len() - MAX_TRIGGER_HISTORY;
            self.history.drain(..excess);
        }
    }

    /// Remember a delivery key as handled (bounded, oldest forgotten first).
    pub fn mark_handled(&mut self, key: &str) {
        if !self.handled.iter().any(|known| known == key) {
            self.handled.push(key.to_owned());
            if self.handled.len() > MAX_HANDLED_KEYS {
                let excess = self.handled.len() - MAX_HANDLED_KEYS;
                self.handled.drain(..excess);
            }
        }
    }

    /// Keep `delivery` to run once the current run ends. The one it replaces counts as
    /// handled (it was skipped in favour of the latest).
    pub fn queue_latest(&mut self, delivery: Delivery) {
        if let Some(replaced) = self.queued.replace(delivery)
            && self
                .queued
                .as_ref()
                .is_some_and(|queued| queued.key != replaced.key)
        {
            self.mark_handled(&replaced.key);
        }
    }

    /// The history entry of one delivery key, newest first.
    pub fn entry_mut(&mut self, key: &str) -> Option<&mut TriggerHistoryEntry> {
        self.history
            .iter_mut()
            .rev()
            .find(|entry| entry.delivery_key == key)
    }
}

fn coalesces(last: &TriggerHistoryEntry, entry: &TriggerHistoryEntry) -> bool {
    match entry.outcome {
        // "Skipped 2 runs": consecutive skips are one entry.
        DeliveryOutcome::SkippedOverlap => last.outcome == DeliveryOutcome::SkippedOverlap,
        // The same event reported again and again is one entry.
        DeliveryOutcome::AlreadyHandled => {
            last.outcome == DeliveryOutcome::AlreadyHandled
                && last.delivery_key == entry.delivery_key
        }
        _ => false,
    }
}

/// Check a definition. Problems are stable codes, sorted.
pub fn validate_spec(spec: &TriggerSpec) -> Vec<TriggerProblem> {
    let mut problems = Vec::new();
    if !line_ok(&spec.name) {
        problems.push(problem("name_invalid", "name"));
    }
    let target_ok = |target: &str| valid_identifier(target).is_ok();
    match &spec.source {
        TriggerSource::Schedule { schedule } => {
            if crate::command_schedules::zoned::zone(&schedule.time_zone).is_err() {
                problems.push(problem("time_zone_invalid", "source.schedule.time_zone"));
            }
            if crate::command_schedules::parse_local_time(&schedule.local_time).is_err() {
                problems.push(problem("local_time_invalid", "source.schedule.local_time"));
            }
            if schedule.recurrence.validate().is_err() {
                problems.push(problem("recurrence_invalid", "source.schedule.recurrence"));
            }
        }
        TriggerSource::TicketOpened {
            target_id,
            tag,
            category,
        } => {
            if !target_ok(target_id) {
                problems.push(problem("target_invalid", "source.target_id"));
            }
            if tag.as_deref().is_some_and(|tag| !line_ok(tag)) {
                problems.push(problem("tag_invalid", "source.tag"));
            }
            if category
                .as_deref()
                .is_some_and(|category| !line_ok(category))
            {
                problems.push(problem("category_invalid", "source.category"));
            }
        }
        TriggerSource::GateFailed { target_id } => {
            if target_id
                .as_deref()
                .is_some_and(|target| !target_ok(target))
            {
                problems.push(problem("target_invalid", "source.target_id"));
            }
        }
        TriggerSource::BranchCiFailed { target_id, watch }
        | TriggerSource::CommitLanded { target_id, watch } => {
            if !target_ok(target_id) {
                problems.push(problem("target_invalid", "source.target_id"));
            }
            for (field, value) in [
                ("repository", &watch.repository),
                ("workflow", &watch.workflow),
                ("branch", &watch.branch),
            ] {
                if !line_ok(value) || value.chars().any(char::is_whitespace) {
                    problems.push(problem("watch_invalid", format!("source.watch.{field}")));
                }
            }
            if watch
                .provider
                .as_deref()
                .is_some_and(|provider| valid_identifier(provider).is_err())
            {
                problems.push(problem("watch_invalid", "source.watch.provider"));
            }
        }
    }
    let action = &spec.action;
    if action.template.is_none() && action.placement.is_none() {
        problems.push(problem("action_empty", "action"));
    }
    if let Some(template) = &action.template {
        if valid_identifier(&template.template_id).is_err() {
            problems.push(problem("template_invalid", "action.template.template_id"));
        }
        if valid_identifier(&template.tool).is_err() {
            problems.push(problem("tool_invalid", "action.template.tool"));
        }
        if template.inputs.len() + template.event_inputs.len() > MAX_TRIGGER_INPUTS {
            problems.push(problem("inputs_too_many", "action.template.inputs"));
        }
        for key in template.inputs.keys().chain(template.event_inputs.keys()) {
            if valid_identifier(key).is_err() {
                problems.push(problem(
                    "input_invalid",
                    format!("action.template.inputs.{key}"),
                ));
            }
        }
        for key in template.event_inputs.keys() {
            if template.inputs.contains_key(key) {
                problems.push(problem(
                    "input_twice",
                    format!("action.template.event_inputs.{key}"),
                ));
            }
        }
        let available = spec.source.event_fields();
        for (key, field) in &template.event_inputs {
            if !available.contains(field) {
                problems.push(problem(
                    "event_input_unavailable",
                    format!("action.template.event_inputs.{key}"),
                ));
            }
        }
    }
    problems.sort();
    problems.dedup();
    problems
}

/// The digest a review records: the definition and the template version reviewed.
/// Any change of either means the trigger is no longer the one a person reviewed.
pub fn spec_digest(spec: &TriggerSpec, template_version: Option<u32>) -> String {
    let canonical = serde_json::json!({
        "spec": spec,
        "template_version": template_version,
    });
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    hex(&Sha256::digest(&bytes))[..32].to_owned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The template apply's operation id for one delivery: the trigger and a hash of the
/// key, so a redelivered or replayed event is the same apply (`trg-<id>-<hash>`).
pub fn delivery_operation_id(trigger_id: &str, key: &str) -> String {
    let digest = hex(&Sha256::digest(key.as_bytes()));
    format!("{TRIGGER_OPERATION_PREFIX}{trigger_id}-{}", &digest[..16])
}

/// Delivery keys, one shape per source. They name the fact, never the time it
/// arrived, so the same fact reported twice has one key.
pub mod keys {
    pub fn schedule(occurrence: &str) -> String {
        format!("schedule:{occurrence}")
    }
    pub fn ticket(connection_id: &str, native_id: &str) -> String {
        format!("ticket:{connection_id}:{native_id}")
    }
    /// A run's gate entering `failed`, by the journal entry that recorded it.
    pub fn gate(run_id: &str, gate_id: &str, seq: u64) -> String {
        format!("gate:{run_id}:{gate_id}:{seq}")
    }
    /// A branch CI run's red verdict, by provider run and attempt.
    pub fn ci(provider: &str, run: &str, attempt: u32) -> String {
        format!("ci:{provider}:{run}:{attempt}")
    }
    pub fn commit(branch: &str, commit: &str) -> String {
        format!("commit:{branch}:{commit}")
    }
    pub fn manual(operation_id: &str) -> String {
        format!("manual:{operation_id}")
    }
}

/// What to do with one delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryDecision {
    /// Not firing (paused or waiting for a review): nothing is recorded.
    Ignore(TriggerError),
    /// The same delivery again.
    AlreadyHandled,
    /// Record what would have run.
    DryRun,
    /// Take the action.
    Apply,
    /// The last run is still working: skip.
    SkipOverlap,
    /// The last run is still working: keep this one for when it finishes.
    QueueLatest,
}

/// Decide one delivery. `running`: the trigger's last applied run is still working.
/// Pure: the same trigger and delivery always decide the same way.
pub fn decide_delivery(trigger: &Trigger, delivery: &Delivery, running: bool) -> DeliveryDecision {
    if !trigger.state.fires() {
        return DeliveryDecision::Ignore(TriggerError::NotFiring);
    }
    if trigger.handled(&delivery.key) {
        return DeliveryDecision::AlreadyHandled;
    }
    if trigger.state == TriggerState::DryRun {
        return DeliveryDecision::DryRun;
    }
    if running && trigger.action.template.is_some() {
        return match trigger.overlap {
            Overlap::Skip => DeliveryDecision::SkipOverlap,
            Overlap::QueueLatest => DeliveryDecision::QueueLatest,
        };
    }
    DeliveryDecision::Apply
}

/// A request to change a trigger's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerOp {
    On,
    DryRun,
    Pause,
    Resume,
    /// The template changed since the review.
    TemplateChanged,
    /// The definition was edited.
    Edited,
}

/// The state after `op`. `reviewed`: the review on record covers the definition and
/// template version as they are now. Pure; a repeat answers the same state.
pub fn transition(
    state: TriggerState,
    paused_from: Option<TriggerState>,
    op: TriggerOp,
    reviewed: bool,
) -> TriggerState {
    use TriggerState as S;
    match op {
        TriggerOp::On => S::On,
        TriggerOp::DryRun => S::DryRun,
        TriggerOp::Pause => S::Paused,
        TriggerOp::Resume => match (state, paused_from) {
            (S::Paused, Some(S::On) | Some(S::NeedsReview)) if reviewed => S::On,
            (S::Paused, Some(S::On) | Some(S::NeedsReview)) => S::NeedsReview,
            (S::Paused, _) => S::DryRun,
            (other, _) => other,
        },
        TriggerOp::TemplateChanged | TriggerOp::Edited => match state {
            S::On if !reviewed => S::NeedsReview,
            other => other,
        },
    }
}

/// A schedule's next `count` instants after `now_ms`, in epoch milliseconds, through
/// the command-schedule engine. Empty for a schedule that never runs again.
pub fn schedule_next_ms(schedule: &TriggerSchedule, now_ms: u64, count: usize) -> Vec<u64> {
    let Some(now) = instant(now_ms) else {
        return Vec::new();
    };
    zoned(schedule)
        .next(now, count)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|occurrence| u64::try_from(occurrence.instant.as_millisecond()).ok())
        .collect()
}

/// The latest occurrence due in `(after_ms, now_ms]`, by its key, and how many earlier
/// ones it covers (missed while this machine was off: they run once, as this one).
pub fn schedule_due(
    schedule: &TriggerSchedule,
    after_ms: u64,
    now_ms: u64,
) -> Option<(String, u32)> {
    let due = zoned(schedule)
        .between(instant(after_ms)?, instant(now_ms)?, 1024)
        .ok()?;
    let latest = due.last()?.key.clone();
    Some((
        latest,
        u32::try_from(due.len().saturating_sub(1)).unwrap_or(u32::MAX),
    ))
}

fn zoned(schedule: &TriggerSchedule) -> crate::command_schedules::zoned::Zoned<'_> {
    crate::command_schedules::zoned::Zoned {
        time_zone: &schedule.time_zone,
        local_time: &schedule.local_time,
        recurrence: &schedule.recurrence,
    }
}

fn instant(ms: u64) -> Option<jiff::Timestamp> {
    jiff::Timestamp::from_millisecond(i64::try_from(ms).ok()?).ok()
}

/// The value one event field takes in a delivery, when it carries it.
pub fn event_value(delivery: &Delivery, field: EventField) -> Option<&str> {
    delivery.evidence.get(field.code()).map(String::as_str)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggersFile {
    schema_version: u32,
    triggers: Vec<Trigger>,
}

/// The machine-local triggers of one orchestration project, private and bounded.
#[derive(Clone)]
pub struct TriggerStore {
    storage: Storage,
}

impl TriggerStore {
    pub fn new(home: &Path, owner_checkout_id: &str) -> Result<Self, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(Self {
            storage: Storage::machine_named(
                home,
                &format!("{TRIGGER_DIR}/{owner_checkout_id}.json"),
            ),
        })
    }

    /// The directory a waiter watches to learn of a change without polling.
    pub fn directory(home: &Path) -> std::path::PathBuf {
        home.join(TRIGGER_DIR)
    }

    /// The owners with a trigger file on this machine.
    pub fn owners(home: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(Self::directory(home)) else {
            return Vec::new();
        };
        let mut owners: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let owner = name.strip_suffix(".json")?;
                valid_identifier(owner).ok().map(|()| owner.to_owned())
            })
            .collect();
        owners.sort();
        owners
    }

    pub fn lock(&self) -> Result<crate::FileLock, OrchestrationError> {
        self.storage.lock()
    }

    /// Every trigger, in the order they were added. A missing file is none; a newer
    /// schema is refused (`unsupported_version`) and a damaged file is `corrupt`.
    pub fn triggers(&self) -> Result<Vec<Trigger>, OrchestrationError> {
        match self.storage.read(MAX_TRIGGERS_BYTES)? {
            Some(bytes) => decode::<TriggersFile>(&bytes).map(|file| file.triggers),
            None => Ok(Vec::new()),
        }
    }

    pub fn find(&self, trigger_id: &str) -> Result<Option<Trigger>, OrchestrationError> {
        Ok(self
            .triggers()?
            .into_iter()
            .find(|trigger| trigger.trigger_id == trigger_id))
    }

    /// Replace every trigger under the caller's lock. More than [`MAX_TRIGGERS`] is
    /// refused with `capacity_reached`.
    pub fn write(&self, triggers: &[Trigger]) -> Result<(), OrchestrationError> {
        if triggers.len() > MAX_TRIGGERS {
            return Err(OrchestrationError::Capacity);
        }
        let file = TriggersFile {
            schema_version: ORCHESTRATION_VERSION,
            triggers: triggers.to_vec(),
        };
        let mut bytes =
            serde_json::to_vec_pretty(&file).map_err(|_| OrchestrationError::Corrupt)?;
        bytes.push(b'\n');
        self.storage.write(&bytes, MAX_TRIGGERS_BYTES)
    }

    /// Change one trigger under the store's lock, when the change changes it.
    pub fn update<T>(
        &self,
        trigger_id: &str,
        change: impl FnOnce(&mut Trigger) -> Result<T, OrchestrationError>,
    ) -> Result<(Trigger, T), OrchestrationError> {
        let _lock = self.lock()?;
        let mut triggers = self.triggers()?;
        let trigger = triggers
            .iter_mut()
            .find(|trigger| trigger.trigger_id == trigger_id)
            .ok_or(OrchestrationError::Trigger(TriggerError::NotFound))?;
        let before = trigger.clone();
        let answer = change(trigger)?;
        let after = trigger.clone();
        if after != before {
            self.write(&triggers)?;
        }
        Ok((after, answer))
    }
}
