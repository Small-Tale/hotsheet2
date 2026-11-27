//! HS2-KXKA7X: scheduled and event triggers over the shared runtime.
//!
//! A trigger is machine-local (the maintainer's decision "On one machine, like command
//! schedules") and only ever does two things: it applies one plan template through
//! [`OrchestrationRuntime::template_apply`] (a new goal handed to the planner; the
//! scheduler then places the steps within the policy's limits) and it starts or pauses
//! placement (the Start/Pause toggle of HS2-TZJYKH). It never dispatches, pushes or
//! deploys: those stay with each project's integrator, push policy and deploy script.
//!
//! - **Deliveries.** Every occurrence or event has a deterministic key whose hash is the
//!   template apply's operation id, so a redelivered event, a replay after a restart or
//!   a second reader answers the existing apply and opens no second goal.
//! - **Sources.** A schedule reuses the command-schedule engine and is due at its next
//!   instant (the host wakes then, never on an interval); missed occurrences run once
//!   on return. A run's failed CI gate is read from the run event journal. A ticket
//!   opened in a project is read from the existing target follower (`watch_step`). A
//!   branch's CI and the commits landing on it are followed by a branch watch that
//!   reuses the hosted CI gate watch (`hosted_gate_watch` and its long poll, here or on
//!   the machine that hosts the project) over the Job monitor's watch Jobs.
//! - **Runs ask.** Deliveries act as a system actor named for the trigger, so a
//!   template apply asks through the permission card like any orchestrator action
//!   (HS2-AMJNER resumes it once a person answers). The review says which will ask.
//! - **Turning On.** A person turns a trigger On as their own act. An AI session may
//!   too, through the existing permission card: it asks for what the trigger's
//!   deliveries will do and goes On once a person allows it.
//! - **Outages** warn on the trigger and notify once through the existing notification
//!   path; there is no failover.
use std::collections::{BTreeMap, BTreeSet};

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    ActiveRun, ApplyState, AuditOutcome, AuditRecord, BranchWatch, DeclarationState, Delivery,
    DeliveryDecision, DeliveryKind, DeliveryOutcome, DenialReason, EventField, GateKind, GateState,
    Overlap, PendingOn, PermissionAction, PermissionKey, RunEventJournal, RunEventKind,
    TargetAction, TemplateApplyStore, TemplateState, Trigger, TriggerError, TriggerHistoryEntry,
    TriggerOp, TriggerPlacement, TriggerProblem, TriggerReview, TriggerSource, TriggerSpec,
    TriggerState, TriggerStore, TriggerWarning, WouldRun, decide_delivery, delivery_operation_id,
    event_value, render_brief, resolve_inputs, spec_digest, trigger_keys, trigger_transition,
    validate_trigger,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::access::{ActionRequest, is_person};
use super::hosted_gates::{HostedGate, HostedGateWatch};
use super::policy::SetAutomatic;
use super::templates::{AccessNeed, ApplyTemplate, PreviewTemplate};
use super::tickets::{TargetTicket, encode, identifier};
use super::*;

/// The next occurrences a schedule trigger shows.
pub const TRIGGER_NEXT_RUNS: usize = 3;
/// The run events one pass reads for gate triggers, per page and in pages.
const GATE_EVENT_PAGE: usize = 200;
const GATE_EVENT_PAGES: usize = 10;
/// The longest one branch-watch long poll parks.
pub const MAX_BRANCH_WAIT_MS: u64 = super::hosted_gates::MAX_HOSTED_GATE_WAIT_MS;

/// Names one trigger.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerRef {
    pub trigger_id: String,
}

/// Add a trigger, or replace one's definition. A new trigger starts as Dry Run
/// (or Paused when asked); editing an On trigger sends it to Review.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTrigger {
    #[serde(default)]
    pub trigger_id: Option<String>,
    pub name: String,
    pub source: TriggerSource,
    pub action: hotsheet_ticketing::orchestration::TriggerAction,
    #[serde(default)]
    pub overlap: Overlap,
    /// `dry_run` (the default) or `paused`, for a new trigger.
    #[serde(default)]
    pub start: Option<TriggerState>,
}

impl SetTrigger {
    fn spec(&self) -> TriggerSpec {
        TriggerSpec {
            name: self.name.clone(),
            source: self.source.clone(),
            action: self.action.clone(),
            overlap: self.overlap,
        }
    }
}

/// Review a saved trigger, or a definition not saved yet.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewTrigger {
    #[serde(default)]
    pub trigger_id: Option<String>,
    #[serde(default)]
    pub trigger: Option<TriggerSpec>,
}

/// Fire a trigger by hand, as one delivery under a retained operation id.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FireTrigger {
    pub trigger_id: String,
    pub operation_id: String,
    /// Event values for the template's event inputs, by [`EventField`] code.
    #[serde(default)]
    pub evidence: BTreeMap<String, String>,
}

/// One trigger pass, as the server runs on its own.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerPassRequest {
    /// The instant the pass runs at. Honoured only by a test build (the isolated
    /// test permission vault), so a real-server test can reach a schedule's slot.
    #[serde(default)]
    pub at_ms: Option<u64>,
}

/// One trigger as listed and shown: the record and what it is computed into.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerView {
    #[serde(flatten)]
    pub trigger: Trigger,
    /// A schedule's next instants, while it fires.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_runs_ms: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_version: Option<u32>,
    /// The review on record covers the definition and template as they are.
    pub reviewed: bool,
    /// The last applied run is still working.
    pub running: bool,
    /// The goal ticket of the running delivery, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_goal: Option<String>,
}

/// This machine's triggers of one orchestration project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerList {
    pub triggers: Vec<TriggerView>,
}

/// What a trigger watches, as the review shows it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewWatch {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<BranchWatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// One project a delivery may change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewProject {
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub declared: bool,
    pub steps: u32,
}

/// What one delivery creates.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewPerDelivery {
    /// Goal tickets opened in the orchestration project (one per template apply).
    pub goals: u32,
    pub steps: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<TriggerPlacement>,
}

/// The review's scope: what it watches, which projects it may change, what one
/// delivery creates.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewScope {
    pub watches: ReviewWatch,
    pub projects: Vec<ReviewProject>,
    pub per_delivery: ReviewPerDelivery,
}

/// Whether one kind of action asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewNeed {
    /// Never asks (watching; adding the project was the consent).
    NotNeeded,
    /// A remembered rule allows it.
    Remembered,
    /// It asks through the permission card each time.
    Asks,
    /// Not changed by the trigger: the project's own integrator, push policy and deploy
    /// script decide.
    Unchanged,
}

/// One row of the review's permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPermission {
    /// `watch`, `tickets_write`, `session_start`, `placement` or `push_deploy`.
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub need: ReviewNeed,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

/// What happens next.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewNext {
    /// A schedule's next instants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs_ms: Vec<u64>,
    /// The event a delivery comes from, for an event source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    pub overlap: Overlap,
}

/// The side-effect-free review: scope, permissions and next actions, in identifiers
/// and codes. The digest is what turning On records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerReviewView {
    pub problems: Vec<TriggerProblem>,
    pub scope: ReviewScope,
    pub permissions: Vec<ReviewPermission>,
    pub next: ReviewNext,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_version: Option<u32>,
    /// Turning it On: the person's own act, or a request an AI session waits on.
    pub turn_on: AccessNeed,
}

/// What one delivery did.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeliveryView {
    pub trigger_id: String,
    pub delivery_key: String,
    pub outcome: DeliveryOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// What one trigger pass did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriggerPassReport {
    pub deliveries: Vec<DeliveryView>,
    /// The earliest instant a schedule is due next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_wake_ms: Option<u64>,
}

/// What a branch watch remembers between steps.
#[derive(Debug, Default, Clone)]
pub struct BranchFollowState {
    cursor: Option<u64>,
    started: bool,
    again: bool,
}

/// One branch-watch step.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BranchStep {
    /// False when the trigger no longer follows a branch: the loop should end.
    pub active: bool,
    /// The step could not read the branch; the caller backs off.
    pub failed: bool,
    pub deliveries: Vec<DeliveryView>,
}

/// One notification a trigger raised, worded for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TriggerNotification {
    pub owner_id: String,
    pub trigger_id: String,
    pub severity: &'static str,
    pub title: String,
    pub body: String,
    pub dedupe_key: String,
}

fn trigger_error(error: TriggerError) -> OrchestrationError {
    OrchestrationError::Trigger(error)
}

fn trigger_actor(trigger: &Trigger) -> Option<MutationActor> {
    MutationActor::parse("system", Some(trigger.actor_id())).ok()
}

fn as_view(delivery: &Delivery, trigger: &Trigger, entry: &TriggerHistoryEntry) -> DeliveryView {
    DeliveryView {
        trigger_id: trigger.trigger_id.clone(),
        delivery_key: delivery.key.clone(),
        outcome: entry.outcome,
        operation_id: entry.operation_id.clone(),
        reason: entry.reason.clone(),
    }
}

fn history_entry(delivery: &Delivery, outcome: DeliveryOutcome) -> TriggerHistoryEntry {
    TriggerHistoryEntry {
        delivery_key: delivery.key.clone(),
        kind: delivery.kind,
        at_ms: delivery.at_ms,
        outcome,
        count: 1,
        evidence: delivery.evidence.clone(),
        missed: delivery.missed,
        operation_id: None,
        goal_slug: None,
        placement: None,
        reason: None,
        would: None,
    }
}

/// A branch name a ref lookup may use.
fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 200
        && !branch.starts_with('-')
        && !branch.contains("..")
        && !branch.ends_with('/')
        && branch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

impl OrchestrationRuntime {
    fn trigger_store(&self, owner_id: &str) -> Result<TriggerStore, OrchestrationError> {
        TriggerStore::new(&self.home, owner_id)
    }

    /// The owners with triggers on this machine.
    pub fn trigger_owners(&self) -> Vec<String> {
        TriggerStore::owners(&self.home)
    }

    /// The machine-local file of one owner's triggers, for a host that watches it.
    pub fn trigger_file(&self, owner_id: &str) -> std::path::PathBuf {
        TriggerStore::directory(&self.home).join(format!("{owner_id}.json"))
    }

    fn trigger_audit(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        action: &str,
        operation_id: Option<&str>,
        (outcome, reason): (AuditOutcome, Option<&str>),
        tx: &Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        self.audit_record(
            owner_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: None,
                action: action.into(),
                operation_id: operation_id.map(Into::into),
                outcome,
                reason: reason.map(Into::into),
                permission_id: None,
                requested_by: None,
            },
        )
    }

    /// The template a trigger applies, as it is now: its name, version and whether it
    /// can still be applied. `None` while the trigger applies no template.
    fn trigger_template(
        &self,
        owner: &Checkout,
        trigger_template: Option<&hotsheet_ticketing::orchestration::TriggerTemplate>,
    ) -> Option<Option<hotsheet_ticketing::orchestration::Template>> {
        let wanted = trigger_template?;
        Some(
            hotsheet_ticketing::orchestration::TemplateStore::new(std::path::Path::new(
                &owner.root,
            ))
            .templates()
            .ok()
            .and_then(|templates| {
                templates
                    .into_iter()
                    .find(|template| template.template_id == wanted.template_id)
            })
            .filter(|template| template.state == TemplateState::Active),
        )
    }

    fn template_version_of(&self, owner: &Checkout, trigger: &Trigger) -> Option<u32> {
        self.trigger_template(owner, trigger.action.template.as_ref())
            .flatten()
            .map(|template| template.version)
    }

    /// The run event journal's end, where a gate trigger starts reading.
    fn run_events_head(&self, owner_id: &str) -> Option<u64> {
        RunEventJournal::new(&self.home, owner_id)
            .and_then(|journal| journal.head())
            .ok()
    }

    /// Whether the run a trigger's last applied delivery started is still working: its
    /// apply waits, or its goal ticket is open.
    fn trigger_running(&self, owner: &Checkout, trigger: &Trigger) -> (bool, Option<String>) {
        let Some(active) = &trigger.active else {
            return (false, None);
        };
        let Ok(Some(apply)) = TemplateApplyStore::new(&self.home, &owner.id)
            .and_then(|store| store.find(&active.operation_id))
        else {
            return (false, None);
        };
        let slug = apply.goal.as_ref().and_then(|goal| goal.slug.clone());
        match apply.state {
            ApplyState::Denied => (false, slug),
            ApplyState::WaitingPermission | ApplyState::WaitingPlanner => (true, slug),
            ApplyState::Planning => {
                let open = apply.goal.as_ref().is_some_and(|goal| {
                    owner
                        .verified_git_source(&goal.connection_id)
                        .ok()
                        .and_then(|store| {
                            let id: ulid::Ulid = goal.native_id.parse().ok()?;
                            store.read_ticket(&id).ok()
                        })
                        .is_some_and(|ticket| hotsheet_ticketing::ops::is_open(&ticket))
                });
                (open, slug)
            }
        }
    }

    fn trigger_view(&self, owner: &Checkout, trigger: Trigger, now_ms: u64) -> TriggerView {
        let template = self
            .trigger_template(owner, trigger.action.template.as_ref())
            .flatten();
        let template_version = template.as_ref().map(|template| template.version);
        let next_runs_ms = match (&trigger.source, trigger.state.fires()) {
            (TriggerSource::Schedule { schedule }, true) => next_instants(schedule, now_ms),
            _ => Vec::new(),
        };
        let (running, running_goal) = self.trigger_running(owner, &trigger);
        TriggerView {
            reviewed: trigger.reviewed(template_version),
            template_name: template.map(|template| template.name),
            template_version,
            next_runs_ms,
            running,
            running_goal,
            trigger,
        }
    }

    /// This machine's triggers of the owner, with their next instants. Reads only.
    pub fn trigger_list(
        &self,
        owner_id: &str,
        tx: &Transaction<'_>,
    ) -> Result<TriggerList, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let now = tx.clock.now_ms();
        Ok(TriggerList {
            triggers: self
                .trigger_store(owner_id)?
                .triggers()?
                .into_iter()
                .map(|trigger| self.trigger_view(&owner, trigger, now))
                .collect(),
        })
    }

    /// One trigger (`trigger_not_found`).
    pub fn trigger_show(
        &self,
        owner_id: &str,
        reference: &TriggerRef,
        tx: &Transaction<'_>,
    ) -> Result<TriggerView, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let trigger = self
            .trigger_store(owner_id)?
            .find(&reference.trigger_id)?
            .ok_or(trigger_error(TriggerError::NotFound))?;
        Ok(self.trigger_view(&owner, trigger, tx.clock.now_ms()))
    }

    /// The problems of a definition, with the projects this orchestration project
    /// does not declare.
    fn trigger_problems(&self, owner_id: &str, spec: &TriggerSpec) -> Vec<TriggerProblem> {
        let mut problems = validate_trigger(spec);
        let active = self.active_target_labels(owner_id);
        if let Some(target) = spec.source.target_id()
            && !active.contains_key(target)
        {
            problems.push(TriggerProblem {
                code: "target_unknown".into(),
                at: "source.target_id".into(),
            });
        }
        if let (Some(wanted), Ok(owner)) = (&spec.action.template, self.owner(owner_id))
            && self
                .trigger_template(&owner, Some(wanted))
                .flatten()
                .is_none()
        {
            problems.push(TriggerProblem {
                code: "template_unknown".into(),
                at: "action.template.template_id".into(),
            });
        }
        problems.sort();
        problems.dedup();
        problems
    }

    fn active_target_labels(&self, owner_id: &str) -> BTreeMap<String, String> {
        self.declarations(owner_id)
            .map(|declared| {
                declared
                    .targets
                    .into_iter()
                    .filter(|target| target.state == DeclarationState::Active)
                    .map(|target| (target.target_id, target.label))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Add a trigger or replace one's definition. A new trigger starts as Dry Run (or
    /// Paused); it is never On before a review. Editing an On trigger sends it to
    /// Review. Changes only this machine's record, so it never asks. Refused as
    /// `trigger_invalid` (the review's `problems` say why), `trigger_not_found` or
    /// `capacity_reached`. Audited as `trigger_set`.
    pub fn trigger_set(
        &self,
        owner_id: &str,
        request: SetTrigger,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TriggerView, OrchestrationError> {
        let actor_id =
            actor
                .and_then(|actor| actor.id.clone())
                .ok_or(OrchestrationError::Invalid(
                    "an identified actor is required to change triggers",
                ))?;
        let owner = self.owner(owner_id)?;
        let spec = request.spec();
        if !self.trigger_problems(owner_id, &spec).is_empty() {
            return Err(trigger_error(TriggerError::Invalid));
        }
        let start = request.start.unwrap_or(TriggerState::DryRun);
        if !matches!(start, TriggerState::DryRun | TriggerState::Paused) {
            return Err(OrchestrationError::Invalid(
                "a new trigger starts as dry_run or paused; turning it on is a review",
            ));
        }
        let store = self.trigger_store(owner_id)?;
        let now = tx.clock.now_ms();
        let head = self.run_events_head(owner_id);
        let trigger = {
            let _lock = store.lock()?;
            let mut triggers = store.triggers()?;
            let trigger = match &request.trigger_id {
                Some(id) => {
                    let known = triggers
                        .iter_mut()
                        .find(|trigger| &trigger.trigger_id == id)
                        .ok_or(trigger_error(TriggerError::NotFound))?;
                    if known.spec() != spec {
                        let source_changed = known.source != spec.source;
                        known.name = spec.name.clone();
                        known.source = spec.source.clone();
                        known.action = spec.action.clone();
                        known.overlap = spec.overlap;
                        known.updated_at_ms = now;
                        known.updated_by = Some(actor_id.clone());
                        let version = self.template_version_of(&owner, known);
                        let reviewed = known.reviewed(version);
                        known.state = trigger_transition(
                            known.state,
                            known.paused_from,
                            TriggerOp::Edited,
                            reviewed,
                        );
                        if let Some(from) = known.paused_from {
                            known.paused_from =
                                Some(trigger_transition(from, None, TriggerOp::Edited, reviewed));
                        }
                        if source_changed && known.state.fires() {
                            known.arm(now, head);
                        }
                    }
                    known.clone()
                }
                None => {
                    let trigger_id =
                        hotsheet_ticketing::mint_ulid(tx.clock, &mut *tx.rng).to_string();
                    let mut trigger = Trigger {
                        trigger_id,
                        name: spec.name.clone(),
                        source: spec.source.clone(),
                        action: spec.action.clone(),
                        overlap: spec.overlap,
                        state: start,
                        paused_from: (start == TriggerState::Paused)
                            .then_some(TriggerState::DryRun),
                        review: None,
                        pending_on: None,
                        created_at_ms: now,
                        updated_at_ms: now,
                        updated_by: Some(actor_id.clone()),
                        cursor: Default::default(),
                        active: None,
                        queued: None,
                        history: Vec::new(),
                        handled: Vec::new(),
                        warning: None,
                        review_notified: None,
                    };
                    trigger.arm(now, head);
                    triggers.push(trigger.clone());
                    trigger
                }
            };
            store.write(&triggers)?;
            trigger
        };
        self.trigger_audit(
            owner_id,
            actor,
            "trigger_set",
            Some(&trigger.trigger_id),
            (AuditOutcome::Changed, None),
            tx,
        )?;
        Self::scheduler_wake_at(&self.home);
        Ok(self.trigger_view(&owner, trigger, now))
    }

    /// Forget a trigger. Work it applied is unchanged.
    pub fn trigger_remove(
        &self,
        owner_id: &str,
        reference: &TriggerRef,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TriggerRef, OrchestrationError> {
        let store = self.trigger_store(owner_id)?;
        {
            let _lock = store.lock()?;
            let mut triggers = store.triggers()?;
            let before = triggers.len();
            triggers.retain(|trigger| trigger.trigger_id != reference.trigger_id);
            if triggers.len() == before {
                return Err(trigger_error(TriggerError::NotFound));
            }
            store.write(&triggers)?;
        }
        self.trigger_audit(
            owner_id,
            actor,
            "trigger_remove",
            Some(&reference.trigger_id),
            (AuditOutcome::Changed, None),
            tx,
        )?;
        Ok(reference.clone())
    }

    /// The side-effect-free review of a saved trigger or an unsaved definition: what
    /// it watches, which projects it may change, what one delivery creates, what will
    /// ask, and the next instants or events. Writes nothing and asks nobody.
    pub fn trigger_review(
        &self,
        owner_id: &str,
        request: &ReviewTrigger,
        actor: Option<&MutationActor>,
        tx: &Transaction<'_>,
    ) -> Result<TriggerReviewView, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let spec = match (&request.trigger, &request.trigger_id) {
            (Some(spec), _) => spec.clone(),
            (None, Some(id)) => self
                .trigger_store(owner_id)?
                .find(id)?
                .ok_or(trigger_error(TriggerError::NotFound))?
                .spec(),
            (None, None) => {
                return Err(OrchestrationError::Invalid(
                    "review names a trigger_id or a trigger definition",
                ));
            }
        };
        Ok(self.review_of(&owner, &spec, actor, tx.clock.now_ms()))
    }

    fn review_of(
        &self,
        owner: &Checkout,
        spec: &TriggerSpec,
        actor: Option<&MutationActor>,
        now_ms: u64,
    ) -> TriggerReviewView {
        let owner_id = owner.id.as_str();
        let problems = self.trigger_problems(owner_id, spec);
        let labels = self.active_target_labels(owner_id);
        let template = self
            .trigger_template(owner, spec.action.template.as_ref())
            .flatten();
        let template_version = template.as_ref().map(|template| template.version);
        let mut watches = ReviewWatch {
            kind: spec.source.code().into(),
            target_id: spec.source.target_id().map(Into::into),
            label: spec
                .source
                .target_id()
                .and_then(|target| labels.get(target).cloned()),
            branch: spec.source.branch().cloned(),
            ..ReviewWatch::default()
        };
        if let TriggerSource::TicketOpened { tag, category, .. } = &spec.source {
            watches.tag.clone_from(tag);
            watches.category.clone_from(category);
        }
        // The projects the template's steps name, with the fixed inputs and defaults.
        let mut projects: BTreeMap<String, u32> = BTreeMap::new();
        let mut per_delivery = ReviewPerDelivery {
            placement: spec.action.placement,
            ..ReviewPerDelivery::default()
        };
        if let (Some(template), Some(wanted)) = (&template, &spec.action.template) {
            let mut supplied = wanted.inputs.clone();
            for (key, field) in &wanted.event_inputs {
                supplied.insert(key.clone(), Value::String(format!("<{}>", field.code())));
            }
            let (inputs, _) = resolve_inputs(template, &supplied);
            let brief = render_brief(template, &inputs, &BTreeMap::new(), "review", &labels);
            for step in &brief.steps {
                *projects.entry(step.target_id.clone()).or_default() += 1;
            }
            per_delivery.goals = 1;
            per_delivery.steps = u32::try_from(brief.steps.len()).unwrap_or(u32::MAX);
            per_delivery.goal_title = Some(brief.goal_title);
        }
        let mut permissions = vec![ReviewPermission {
            action: "watch".into(),
            target_id: spec.source.target_id().map(Into::into),
            need: ReviewNeed::NotNeeded,
            rule_id: None,
            tool: None,
        }];
        let remembered = |action: PermissionAction, target_id: &str, tool: Option<&str>| {
            self.remembered(&PermissionKey {
                target: TargetRef {
                    owner_checkout_id: owner_id.into(),
                    target_id: target_id.into(),
                },
                action,
                tool: tool.map(Into::into),
            })
            .ok()
            .flatten()
        };
        let tool = spec
            .action
            .template
            .as_ref()
            .map(|wanted| wanted.tool.clone());
        for target_id in projects.keys() {
            for (action, tool) in [
                (PermissionAction::TicketsWrite, None),
                (PermissionAction::SessionStart, tool.as_deref()),
            ] {
                let rule = remembered(action, target_id, tool);
                permissions.push(ReviewPermission {
                    action: action.code().into(),
                    target_id: Some(target_id.clone()),
                    need: if rule.is_some() {
                        ReviewNeed::Remembered
                    } else {
                        ReviewNeed::Asks
                    },
                    rule_id: rule,
                    tool: tool.map(Into::into),
                });
            }
        }
        if let Some(placement) = spec.action.placement {
            permissions.push(ReviewPermission {
                action: match placement {
                    TriggerPlacement::Start => "placement_start",
                    TriggerPlacement::Pause => "placement_pause",
                }
                .into(),
                target_id: None,
                need: ReviewNeed::NotNeeded,
                rule_id: None,
                tool: None,
            });
        }
        permissions.push(ReviewPermission {
            action: "push_deploy".into(),
            target_id: None,
            need: ReviewNeed::Unchanged,
            rule_id: None,
            tool: None,
        });
        let next = ReviewNext {
            runs_ms: match &spec.source {
                TriggerSource::Schedule { schedule } => next_instants(schedule, now_ms),
                _ => Vec::new(),
            },
            event: match &spec.source {
                TriggerSource::Schedule { .. } => None,
                other => Some(other.code().into()),
            },
            overlap: spec.overlap,
        };
        let turn_on_needs = self.turn_on_requests(owner_id, spec, &projects);
        TriggerReviewView {
            problems,
            scope: ReviewScope {
                watches,
                projects: projects
                    .into_iter()
                    .map(|(target_id, steps)| ReviewProject {
                        label: labels.get(&target_id).cloned(),
                        declared: labels.contains_key(&target_id),
                        steps,
                        target_id,
                    })
                    .collect(),
                per_delivery,
            },
            permissions,
            next,
            digest: spec_digest(spec, template_version),
            template_version,
            turn_on: if is_person(actor) {
                AccessNeed::Person
            } else if turn_on_needs
                .iter()
                .all(|(target, action)| remembered(*action, target, None).is_some())
            {
                AccessNeed::Remembered
            } else {
                AccessNeed::Asks
            },
        }
    }

    /// What an AI session's turn On asks for: ticket writes in every project the
    /// template may change, and for a placement start, starting work in every
    /// active project. Pausing placement only lowers activity and asks nothing.
    fn turn_on_requests(
        &self,
        owner_id: &str,
        spec: &TriggerSpec,
        projects: &BTreeMap<String, u32>,
    ) -> Vec<(String, PermissionAction)> {
        let mut asks: Vec<(String, PermissionAction)> = projects
            .keys()
            .map(|target| (target.clone(), PermissionAction::TicketsWrite))
            .collect();
        if spec.action.placement == Some(TriggerPlacement::Start) {
            asks.extend(
                self.active_target_labels(owner_id)
                    .into_keys()
                    .map(|target| (target, PermissionAction::WorkStart)),
            );
        }
        asks
    }

    /// Turn a trigger On. A person's turn On is their own act and records the review.
    /// An AI session's asks through the permission card for what the trigger's
    /// deliveries do (one request per project and action, under
    /// `<trigger>.on.<digest>.t<n>`) and answers `permission_required` until a person
    /// answers; the same call then turns it On (Deny refuses it). Every cursor starts
    /// now: turning On never backfills. Audited as `trigger_on`.
    pub fn trigger_on(
        &self,
        owner_id: &str,
        reference: &TriggerRef,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TriggerView, OrchestrationError> {
        let actor_id =
            actor
                .and_then(|actor| actor.id.clone())
                .ok_or(OrchestrationError::Invalid(
                    "an identified actor is required to turn a trigger on",
                ))?;
        let owner = self.owner(owner_id)?;
        let store = self.trigger_store(owner_id)?;
        let trigger = store
            .find(&reference.trigger_id)?
            .ok_or(trigger_error(TriggerError::NotFound))?;
        let spec = trigger.spec();
        let review = self.review_of(&owner, &spec, actor, tx.clock.now_ms());
        if !review.problems.is_empty() {
            return Err(
                if review.problems.iter().any(|p| p.code == "template_unknown") {
                    trigger_error(TriggerError::TemplateMissing)
                } else {
                    trigger_error(TriggerError::Invalid)
                },
            );
        }
        let mut answered = Vec::new();
        if !is_person(actor) {
            let projects: BTreeMap<String, u32> = review
                .scope
                .projects
                .iter()
                .map(|project| (project.target_id.clone(), project.steps))
                .collect();
            // One operation per turn On: a repeat after the answer reuses it, a later
            // turn On (after a Deny, or a change) asks afresh.
            let operation = match &trigger.pending_on {
                Some(pending) if pending.digest == review.digest => pending.operation_id.clone(),
                _ => format!(
                    "{}.on.{}",
                    trigger.trigger_id,
                    hotsheet_ticketing::mint_ulid(tx.clock, &mut *tx.rng)
                ),
            };
            let mut waiting = Vec::new();
            for (n, (target_id, action)) in self
                .turn_on_requests(owner_id, &spec, &projects)
                .into_iter()
                .enumerate()
            {
                let target = Self::target_ref(owner_id, &target_id)?;
                let target_action = match action {
                    PermissionAction::WorkStart => TargetAction::WorkStart,
                    _ => TargetAction::TicketsWrite,
                };
                match self.authorize(
                    &target,
                    actor,
                    ActionRequest::new(target_action).operation(format!("{operation}.t{n}")),
                    tx,
                ) {
                    Ok(authorized) => {
                        self.record_outcome(&target, &authorized, true, Some("trigger_on"), tx)?;
                        if let Some(id) = authorized.permission_id.clone() {
                            answered.push(id);
                        }
                    }
                    Err(OrchestrationError::PermissionRequired { request_id }) => {
                        waiting.push(request_id);
                    }
                    Err(error) => {
                        let _ = store.update(&trigger.trigger_id, |trigger| {
                            trigger.pending_on = None;
                            Ok(())
                        });
                        self.trigger_audit(
                            owner_id,
                            actor,
                            "trigger_on",
                            Some(&trigger.trigger_id),
                            (AuditOutcome::Denied, Some(error.code())),
                            tx,
                        )?;
                        return Err(error);
                    }
                }
            }
            if let Some(first) = waiting.first().cloned() {
                let now = tx.clock.now_ms();
                store.update(&trigger.trigger_id, |trigger| {
                    trigger.pending_on = Some(PendingOn {
                        operation_id: operation.clone(),
                        digest: review.digest.clone(),
                        actor_id: actor_id.clone(),
                        requests: waiting.clone(),
                        at_ms: now,
                    });
                    Ok(())
                })?;
                self.trigger_audit(
                    owner_id,
                    actor,
                    "trigger_on",
                    Some(&trigger.trigger_id),
                    (AuditOutcome::Succeeded, Some("permission_required")),
                    tx,
                )?;
                return Err(OrchestrationError::PermissionRequired { request_id: first });
            }
        }
        let now = tx.clock.now_ms();
        let head = self.run_events_head(owner_id);
        let (trigger, ()) = store.update(&trigger.trigger_id, |trigger| {
            let was = trigger.state;
            trigger.review = Some(TriggerReview {
                role: if is_person(actor) { "human" } else { "ai" }.into(),
                actor_id: actor_id.clone(),
                at_ms: now,
                digest: review.digest.clone(),
                template_version: review.template_version,
                permission_requests: trigger
                    .pending_on
                    .take()
                    .map(|pending| pending.requests)
                    .unwrap_or(answered.clone()),
            });
            trigger.state = trigger_transition(was, trigger.paused_from, TriggerOp::On, true);
            trigger.paused_from = None;
            trigger.review_notified = None;
            if was != TriggerState::On {
                trigger.arm(now, head);
            }
            trigger.updated_at_ms = now;
            trigger.updated_by = Some(actor_id.clone());
            Ok(())
        })?;
        self.trigger_audit(
            owner_id,
            actor,
            "trigger_on",
            Some(&trigger.trigger_id),
            (AuditOutcome::Changed, None),
            tx,
        )?;
        Self::scheduler_wake_at(&self.home);
        Ok(self.trigger_view(&owner, trigger, now))
    }

    /// Dry Run, Pause or Resume. Resume returns to the state paused from (an On
    /// trigger whose review no longer covers it goes to Review) and never backfills.
    /// Changes only this machine's record, so it never asks. Audited as
    /// `trigger_<op>`.
    pub fn trigger_state(
        &self,
        owner_id: &str,
        reference: &TriggerRef,
        op: TriggerOp,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TriggerView, OrchestrationError> {
        if !matches!(op, TriggerOp::DryRun | TriggerOp::Pause | TriggerOp::Resume) {
            return Err(OrchestrationError::Invalid(
                "unsupported trigger state change",
            ));
        }
        let actor_id =
            actor
                .and_then(|actor| actor.id.clone())
                .ok_or(OrchestrationError::Invalid(
                    "an identified actor is required to change a trigger",
                ))?;
        let owner = self.owner(owner_id)?;
        let now = tx.clock.now_ms();
        let head = self.run_events_head(owner_id);
        let store = self.trigger_store(owner_id)?;
        let version = store
            .find(&reference.trigger_id)?
            .as_ref()
            .and_then(|trigger| self.template_version_of(&owner, trigger));
        let (trigger, changed) = store.update(&reference.trigger_id, |trigger| {
            let was = trigger.state;
            let reviewed = trigger.reviewed(version);
            let next = trigger_transition(was, trigger.paused_from, op, reviewed);
            if next == was {
                return Ok(false);
            }
            trigger.paused_from = (next == TriggerState::Paused).then_some(was);
            trigger.state = next;
            trigger.pending_on = None;
            if next.fires() {
                trigger.arm(now, head);
            }
            trigger.updated_at_ms = now;
            trigger.updated_by = Some(actor_id.clone());
            Ok(true)
        })?;
        if changed {
            let action = match op {
                TriggerOp::DryRun => "trigger_dry_run",
                TriggerOp::Pause => "trigger_pause",
                _ => "trigger_resume",
            };
            self.trigger_audit(
                owner_id,
                actor,
                action,
                Some(&trigger.trigger_id),
                (AuditOutcome::Changed, None),
                tx,
            )?;
            Self::scheduler_wake_at(&self.home);
        }
        Ok(self.trigger_view(&owner, trigger, now))
    }

    /// Fire a trigger by hand: one `manual` delivery under the caller's retained
    /// operation id, taken like any other (applied, recorded as a dry run, skipped or
    /// queued while a run works, or already handled for a repeat).
    pub fn trigger_fire(
        &self,
        owner_id: &str,
        request: FireTrigger,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<DeliveryView, OrchestrationError> {
        if actor.and_then(|actor| actor.id.as_ref()).is_none() {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to fire a trigger",
            ));
        }
        identifier(&request.operation_id, "bounded operation id required")?;
        let owner = self.owner(owner_id)?;
        let delivery = Delivery {
            key: trigger_keys::manual(&request.operation_id),
            kind: DeliveryKind::Manual,
            at_ms: tx.clock.now_ms(),
            evidence: request
                .evidence
                .into_iter()
                .filter(|(key, value)| !key.is_empty() && value.len() <= 200)
                .take(8)
                .collect(),
            missed: 0,
        };
        let delivered = self.deliver(&owner, &request.trigger_id, delivery, tx)?;
        delivered.ok_or(trigger_error(TriggerError::NotFiring))
    }

    /// Take one delivery for one trigger: decide it, act, record it. `None` when the
    /// trigger does not fire (paused, waiting for a review).
    pub(super) fn deliver(
        &self,
        owner: &Checkout,
        trigger_id: &str,
        delivery: Delivery,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<DeliveryView>, OrchestrationError> {
        let store = self.trigger_store(&owner.id)?;
        let Some(mut trigger) = store.find(trigger_id)? else {
            return Err(trigger_error(TriggerError::NotFound));
        };
        // A template changed since the review: no delivery until a person reviews it.
        if self.review_lapsed(owner, &mut trigger, tx)? {
            return Ok(None);
        }
        let (running, _) = self.trigger_running(owner, &trigger);
        let decision = decide_delivery(&trigger, &delivery, running);
        let mut entry = match decision {
            DeliveryDecision::Ignore(_) => return Ok(None),
            DeliveryDecision::AlreadyHandled => {
                history_entry(&delivery, DeliveryOutcome::AlreadyHandled)
            }
            DeliveryDecision::SkipOverlap => {
                let mut entry = history_entry(&delivery, DeliveryOutcome::SkippedOverlap);
                entry.operation_id = trigger.active.as_ref().map(|a| a.operation_id.clone());
                entry
            }
            DeliveryDecision::QueueLatest => history_entry(&delivery, DeliveryOutcome::Queued),
            DeliveryDecision::DryRun => self.dry_run_entry(owner, &trigger, &delivery, tx),
            DeliveryDecision::Apply => self.apply_entry(owner, &trigger, &delivery, tx),
        };
        if entry.outcome == DeliveryOutcome::Applied
            && let Some(operation_id) = entry.operation_id.clone()
            && trigger.action.template.is_some()
        {
            entry.goal_slug = TemplateApplyStore::new(&self.home, &owner.id)
                .and_then(|store| store.find(&operation_id))
                .ok()
                .flatten()
                .and_then(|apply| apply.goal.and_then(|goal| goal.slug));
        }
        let view = as_view(&delivery, &trigger, &entry);
        let refused = entry.outcome == DeliveryOutcome::Refused;
        let reason = entry.reason.clone();
        store.update(trigger_id, |current| {
            // Another reader (a headless pass beside the server's) took the same
            // delivery meanwhile: its apply was the same one, so this is a repeat.
            if decision != DeliveryDecision::AlreadyHandled && current.handled(&delivery.key) {
                current.record(history_entry(&delivery, DeliveryOutcome::AlreadyHandled));
                return Ok(());
            }
            if entry.outcome == DeliveryOutcome::Applied
                && let Some(operation_id) = &entry.operation_id
                && current.action.template.is_some()
            {
                current.active = Some(ActiveRun {
                    delivery_key: delivery.key.clone(),
                    operation_id: operation_id.clone(),
                    at_ms: delivery.at_ms,
                });
            }
            if decision == DeliveryDecision::QueueLatest {
                current.queue_latest(delivery.clone());
            }
            if current.queued.as_ref().is_some_and(|queued| {
                queued.key == delivery.key && decision != DeliveryDecision::QueueLatest
            }) {
                current.queued = None;
            }
            current.record(entry.clone());
            Ok(())
        })?;
        self.trigger_audit(
            &owner.id,
            trigger_actor(&trigger).as_ref(),
            "trigger_delivery",
            Some(&delivery_operation_id(&trigger.trigger_id, &delivery.key)),
            (
                if refused {
                    AuditOutcome::Failed
                } else {
                    AuditOutcome::Succeeded
                },
                Some(reason.as_deref().unwrap_or(view.outcome.code())),
            ),
            tx,
        )?;
        if refused {
            self.notify_trigger(TriggerNotification {
                owner_id: owner.id.clone(),
                trigger_id: trigger.trigger_id.clone(),
                severity: "warning",
                title: format!("Trigger {} Could Not Run", trigger.name),
                body: format!(
                    "Its {} delivery was refused ({}). Nothing else changed.",
                    delivery_label(delivery.kind),
                    reason.as_deref().unwrap_or("refused")
                ),
                dedupe_key: format!("trigger:{}:{}", trigger.trigger_id, delivery.key),
            });
        }
        Ok(Some(view))
    }

    /// The inputs one delivery applies the template with: the fixed ones and those
    /// filled from the event.
    fn delivery_inputs(trigger: &Trigger, delivery: &Delivery) -> BTreeMap<String, Value> {
        let Some(wanted) = &trigger.action.template else {
            return BTreeMap::new();
        };
        let mut inputs = wanted.inputs.clone();
        for (key, field) in &wanted.event_inputs {
            if let Some(value) = event_value(delivery, *field) {
                inputs.insert(key.clone(), Value::String(value.to_owned()));
            }
        }
        inputs
    }

    /// What a dry-run delivery would have done: the template's preview, with the
    /// projects that would ask, and the placement change. Writes nothing.
    fn dry_run_entry(
        &self,
        owner: &Checkout,
        trigger: &Trigger,
        delivery: &Delivery,
        tx: &mut Transaction<'_>,
    ) -> TriggerHistoryEntry {
        let mut entry = history_entry(delivery, DeliveryOutcome::DryRun);
        let mut would = WouldRun {
            placement: trigger.action.placement,
            ..WouldRun::default()
        };
        if let Some(wanted) = &trigger.action.template {
            entry.operation_id = Some(delivery_operation_id(&trigger.trigger_id, &delivery.key));
            match self.template_preview(
                &owner.id,
                &PreviewTemplate {
                    template_id: wanted.template_id.clone(),
                    inputs: Self::delivery_inputs(trigger, delivery),
                    choices: BTreeMap::new(),
                },
                trigger_actor(trigger).as_ref(),
                tx,
            ) {
                Ok(preview) => {
                    would.goal_title = Some(preview.goal_title);
                    would.steps = u32::try_from(preview.steps.len()).unwrap_or(u32::MAX);
                    would.asks = preview
                        .targets
                        .iter()
                        .filter(|target| target.apply == AccessNeed::Asks)
                        .map(|target| target.target_id.clone())
                        .collect();
                    would.targets = preview
                        .targets
                        .into_iter()
                        .map(|target| target.target_id)
                        .collect();
                    if let Some(problem) = preview.problems.first() {
                        entry.reason = Some(problem.code.clone());
                    }
                }
                Err(error) => entry.reason = Some(error.code().into()),
            }
        }
        entry.would = Some(would);
        entry
    }

    /// Take the action: apply the template under the delivery's operation id as the
    /// trigger's actor (it asks like any orchestrator action), then set placement.
    fn apply_entry(
        &self,
        owner: &Checkout,
        trigger: &Trigger,
        delivery: &Delivery,
        tx: &mut Transaction<'_>,
    ) -> TriggerHistoryEntry {
        let mut entry = history_entry(delivery, DeliveryOutcome::Applied);
        let actor = trigger_actor(trigger);
        if let Some(wanted) = &trigger.action.template {
            let operation_id = delivery_operation_id(&trigger.trigger_id, &delivery.key);
            entry.operation_id = Some(operation_id.clone());
            let applied = self.template_apply(
                &owner.id,
                ApplyTemplate {
                    template_id: wanted.template_id.clone(),
                    inputs: Self::delivery_inputs(trigger, delivery),
                    choices: BTreeMap::new(),
                    tool: wanted.tool.clone(),
                    model: wanted.model.clone(),
                    effort: wanted.effort.clone(),
                    operation_id,
                },
                actor.as_ref(),
                tx,
            );
            match applied {
                Ok(view) => {
                    if view.apply.state == ApplyState::Denied {
                        entry.outcome = DeliveryOutcome::Refused;
                    }
                    entry.reason = view.apply.reason.clone();
                }
                // The apply waits for a person; HS2-AMJNER resumes it on the answer.
                Err(OrchestrationError::PermissionRequired { .. }) => {
                    entry.reason = Some(DenialReason::PermissionRequired.code().into());
                }
                Err(error) => {
                    entry.outcome = DeliveryOutcome::Refused;
                    entry.reason = Some(error.code().into());
                    return entry;
                }
            }
        }
        if let Some(placement) = trigger.action.placement {
            entry.placement = Some(placement);
            // HS2-36XDDX: a trigger's Start is nobody's review of the plan, so the
            // steps a template holds stay held until a person accepts them.
            if let Err(error) = self.set_automatic_unreviewed(
                &owner.id,
                SetAutomatic {
                    automatic: placement.automatic(),
                    expected_revision: None,
                },
                actor.as_ref(),
                tx,
            ) {
                entry.outcome = DeliveryOutcome::Refused;
                entry.reason = Some(error.code().into());
            }
        }
        entry
    }

    /// A template changed since the review of an On trigger: it goes to Review, its
    /// queued delivery is dropped, and a person is told once per template version.
    /// Answers whether the trigger no longer fires.
    fn review_lapsed(
        &self,
        owner: &Checkout,
        trigger: &mut Trigger,
        tx: &mut Transaction<'_>,
    ) -> Result<bool, OrchestrationError> {
        if trigger.state != TriggerState::On {
            return Ok(false);
        }
        let template = self.trigger_template(owner, trigger.action.template.as_ref());
        let version = template.clone().flatten().map(|template| template.version);
        if trigger.reviewed(version) {
            return Ok(false);
        }
        let now = tx.clock.now_ms();
        let notify = trigger.review_notified != Some(version.unwrap_or(0));
        let (updated, ()) = self
            .trigger_store(&owner.id)?
            .update(&trigger.trigger_id, |t| {
                t.state =
                    trigger_transition(t.state, t.paused_from, TriggerOp::TemplateChanged, false);
                t.queued = None;
                t.review_notified = Some(version.unwrap_or(0));
                t.record(TriggerHistoryEntry {
                    delivery_key: format!("review:{}", version.unwrap_or(0)),
                    kind: DeliveryKind::Manual,
                    at_ms: now,
                    outcome: DeliveryOutcome::NeedsReview,
                    count: 1,
                    evidence: BTreeMap::new(),
                    missed: 0,
                    operation_id: None,
                    goal_slug: None,
                    placement: None,
                    reason: Some(
                        if matches!(template, Some(None)) {
                            TriggerError::TemplateMissing.code()
                        } else {
                            "template_changed"
                        }
                        .into(),
                    ),
                    would: None,
                });
                Ok(())
            })?;
        *trigger = updated;
        if notify {
            self.notify_trigger(TriggerNotification {
                owner_id: owner.id.clone(),
                trigger_id: trigger.trigger_id.clone(),
                severity: "warning",
                title: format!("Trigger {} Needs Review", trigger.name),
                body: "Its template changed since it was turned on. It runs again once \
                       someone reviews it and turns it on."
                    .into(),
                dedupe_key: format!(
                    "trigger-review:{}:{}",
                    trigger.trigger_id,
                    version.unwrap_or(0)
                ),
            });
        }
        self.trigger_audit(
            &owner.id,
            trigger_actor(trigger).as_ref(),
            "trigger_needs_review",
            Some(&trigger.trigger_id),
            (AuditOutcome::Changed, Some("template_changed")),
            tx,
        )?;
        Ok(true)
    }

    fn notify_trigger(&self, notification: TriggerNotification) {
        if let Some(host) = &self.home_notices {
            host.notify_trigger(&notification);
        }
    }

    /// A source could not be followed: show it on the trigger and tell a person once.
    /// A source that answers again clears it.
    pub(super) fn trigger_warning(
        &self,
        owner_id: &str,
        trigger_id: &str,
        code: Option<&str>,
        now_ms: u64,
    ) {
        let Ok(store) = self.trigger_store(owner_id) else {
            return;
        };
        let Ok((trigger, notify)) = store.update(trigger_id, |trigger| match code {
            None => {
                trigger.warning = None;
                Ok(false)
            }
            Some(code) => {
                let same = trigger
                    .warning
                    .as_ref()
                    .is_some_and(|warning| warning.code == code);
                if !same {
                    trigger.warning = Some(TriggerWarning {
                        code: code.into(),
                        since_ms: now_ms,
                        notified: false,
                    });
                }
                let warning = trigger.warning.as_mut().expect("set above");
                let notify = !warning.notified;
                warning.notified = true;
                Ok(notify)
            }
        }) else {
            return;
        };
        if notify {
            self.notify_trigger(TriggerNotification {
                owner_id: owner_id.into(),
                trigger_id: trigger_id.into(),
                severity: "warning",
                title: format!("Trigger {} Cannot Watch Its Source", trigger.name),
                body: format!(
                    "It keeps trying and runs once the source answers again ({}). There is \
                     no failover.",
                    code.unwrap_or_default()
                ),
                dedupe_key: format!("trigger-warning:{trigger_id}:{}", code.unwrap_or_default()),
            });
        }
    }

    // ---- The pass: schedules, gate events, queued deliveries and AI turn-Ons ----

    /// One trigger pass for an owner at `now`: due schedule occurrences (missed ones
    /// coalesced into the latest), failed run gates since the cursor, a queued delivery
    /// whose run finished, a template change that needs a review, and an AI session's
    /// turn On whose permission a person answered. Answers what it delivered and the
    /// next instant a schedule is due.
    pub fn trigger_pass(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<TriggerPassReport, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let store = self.trigger_store(owner_id)?;
        let mut report = TriggerPassReport::default();
        let now = tx.clock.now_ms();
        for trigger in store.triggers()? {
            // An AI session's turn On, answered by a person since.
            if let Some(pending) = &trigger.pending_on
                && pending
                    .requests
                    .iter()
                    .all(|request| !self.permission_waiting(owner_id, request, now))
                && let Ok(actor) = MutationActor::parse("ai", Some(pending.actor_id.clone()))
            {
                let reference = TriggerRef {
                    trigger_id: trigger.trigger_id.clone(),
                };
                if let Err(error) = self.trigger_on(owner_id, &reference, Some(&actor), tx)
                    && !matches!(error, OrchestrationError::PermissionRequired { .. })
                {
                    // Refused (a Deny, a template gone): the AI session asks again
                    // itself if it still wants it On.
                    let _ = store.update(&trigger.trigger_id, |current| {
                        current.pending_on = None;
                        Ok(())
                    });
                }
            }
            let Some(mut trigger) = store.find(&trigger.trigger_id)? else {
                continue;
            };
            // The active delivery's entry follows its apply: the goal once it opens,
            // and the reason it waits (or none), after HS2-AMJNER resumed it.
            if let Some(active) = trigger.active.clone()
                && let Ok(Some(apply)) = TemplateApplyStore::new(&self.home, owner_id)
                    .and_then(|applies| applies.find(&active.operation_id))
            {
                let slug = apply.goal.as_ref().and_then(|goal| goal.slug.clone());
                let reason = match apply.state {
                    ApplyState::Planning => None,
                    _ => apply.reason.clone(),
                };
                let stale = trigger
                    .history
                    .iter()
                    .rev()
                    .find(|entry| {
                        entry.delivery_key == active.delivery_key
                            && entry.outcome == DeliveryOutcome::Applied
                    })
                    .is_some_and(|entry| entry.goal_slug != slug || entry.reason != reason);
                if stale {
                    let (updated, ()) = store.update(&trigger.trigger_id, |current| {
                        if let Some(entry) = current.history.iter_mut().rev().find(|entry| {
                            entry.delivery_key == active.delivery_key
                                && entry.outcome == DeliveryOutcome::Applied
                        }) {
                            entry.goal_slug.clone_from(&slug);
                            entry.reason.clone_from(&reason);
                        }
                        Ok(())
                    })?;
                    trigger = updated;
                }
            }
            if self.review_lapsed(&owner, &mut trigger, tx)? || !trigger.state.fires() {
                continue;
            }
            // A queued delivery runs once the run it waited for is done.
            if let Some(queued) = trigger.queued.clone()
                && !self.trigger_running(&owner, &trigger).0
            {
                store.update(&trigger.trigger_id, |current| {
                    current.queued = None;
                    Ok(())
                })?;
                if let Some(view) = self.deliver(&owner, &trigger.trigger_id, queued, tx)? {
                    report.deliveries.push(view);
                }
            }
            match &trigger.source {
                TriggerSource::Schedule { schedule } => {
                    let after = trigger.cursor.schedule_after_ms.unwrap_or(now);
                    if now > after {
                        // The cursor moves only past a due occurrence, so an idle pass
                        // writes nothing (and wakes nobody).
                        if let Some((latest, missed)) = schedule_between(schedule, after, now) {
                            store.update(&trigger.trigger_id, |current| {
                                current.cursor.schedule_after_ms = Some(now);
                                Ok(())
                            })?;
                            let delivery = Delivery {
                                key: trigger_keys::schedule(&latest),
                                kind: DeliveryKind::Schedule,
                                at_ms: now,
                                evidence: BTreeMap::from([(
                                    EventField::Occurrence.code().to_owned(),
                                    latest.clone(),
                                )]),
                                missed,
                            };
                            if let Some(view) =
                                self.deliver(&owner, &trigger.trigger_id, delivery, tx)?
                            {
                                report.deliveries.push(view);
                            }
                        }
                    }
                    if let Some(next) = next_instants(schedule, now.max(after)).first() {
                        report.next_wake_ms =
                            Some(report.next_wake_ms.map_or(*next, |known| known.min(*next)));
                    }
                }
                TriggerSource::GateFailed { target_id } => {
                    for view in self.gate_deliveries(&owner, &trigger, target_id.as_deref(), tx)? {
                        report.deliveries.push(view);
                    }
                }
                _ => {}
            }
        }
        Ok(report)
    }

    /// Failed CI gates of the owner's runs since the trigger's cursor, each one
    /// delivery keyed by the journal entry that recorded it.
    fn gate_deliveries(
        &self,
        owner: &Checkout,
        trigger: &Trigger,
        target_id: Option<&str>,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<DeliveryView>, OrchestrationError> {
        let journal = RunEventJournal::new(&self.home, &owner.id)?;
        let mut cursor = match trigger.cursor.run_events {
            Some(cursor) => cursor,
            None => {
                let head = journal.head()?;
                self.trigger_store(&owner.id)?
                    .update(&trigger.trigger_id, |current| {
                        current.cursor.run_events = Some(head);
                        Ok(())
                    })?;
                return Ok(Vec::new());
            }
        };
        let start = cursor;
        let mut failed = Vec::new();
        for _ in 0..GATE_EVENT_PAGES {
            let page = journal.read(cursor, GATE_EVENT_PAGE, None)?;
            for entry in &page.entries {
                let event = &entry.record;
                if event.kind == RunEventKind::Gate
                    && event.gate_state == Some(GateState::Failed)
                    && let (Some(run_id), Some(gate_id)) = (&event.run_id, &event.gate_id)
                {
                    failed.push((entry.seq, run_id.clone(), gate_id.clone()));
                }
            }
            cursor = page.cursor;
            if !page.more {
                break;
            }
        }
        if cursor != start {
            self.trigger_store(&owner.id)?
                .update(&trigger.trigger_id, |current| {
                    current.cursor.run_events = Some(cursor);
                    Ok(())
                })?;
        }
        let mut views = Vec::new();
        for (seq, run_id, gate_id) in failed {
            let Ok(gate) = self.gate(&owner.id, &gate_id) else {
                continue;
            };
            if gate.kind != GateKind::Ci || target_id.is_some_and(|target| gate.target_id != target)
            {
                continue;
            }
            let ticket = self
                .run(&owner.id, &run_id)
                .ok()
                .map(|run| format!("{}:{}", run.target.connection_id, run.target.native_id));
            let mut evidence = BTreeMap::from([
                (EventField::Commit.code().to_owned(), gate.commit.clone()),
                (EventField::Gate.code().to_owned(), gate_id.clone()),
            ]);
            if let Some(ticket) = ticket {
                evidence.insert(EventField::Ticket.code().to_owned(), ticket);
            }
            let delivery = Delivery {
                key: trigger_keys::gate(&run_id, &gate_id, seq),
                kind: DeliveryKind::GateFailed,
                at_ms: tx.clock.now_ms(),
                evidence,
                missed: 0,
            };
            if let Some(view) = self.deliver(owner, &trigger.trigger_id, delivery, tx)? {
                views.push(view);
            }
        }
        Ok(views)
    }

    // ---- Ticket opened: the existing target follower ----

    /// The project targets the target follower follows for `ticket_opened` triggers
    /// that fire.
    pub fn trigger_targets(&self, owner_id: &str) -> Vec<String> {
        let Ok(store) = self.trigger_store(owner_id) else {
            return Vec::new();
        };
        let mut targets: BTreeSet<String> = BTreeSet::new();
        for trigger in store.triggers().unwrap_or_default() {
            if trigger.state.fires()
                && let TriggerSource::TicketOpened { target_id, .. } = &trigger.source
            {
                targets.insert(target_id.clone());
            }
        }
        targets.into_iter().collect()
    }

    /// The actor a follower of `target_id` reads as for `ticket_opened` triggers: the
    /// first such trigger's actor (reading never asks).
    pub(super) fn trigger_follower(
        &self,
        owner_id: &str,
        target_id: &str,
    ) -> Option<MutationActor> {
        self.trigger_store(owner_id)
            .ok()?
            .triggers()
            .ok()?
            .into_iter()
            .find(|trigger| {
                trigger.state.fires()
                    && matches!(&trigger.source, TriggerSource::TicketOpened { target_id: t, .. } if t == target_id)
            })
            .and_then(|trigger| trigger_actor(&trigger))
    }

    /// One follower step's part for `ticket_opened` triggers: each ticket the page
    /// names, read through the gate, is one delivery when it is open, matches the
    /// trigger's tag and category, and was created after the trigger started firing.
    /// After a handshake or a reconnect (`events` is `None`) the open tickets with the
    /// trigger's tag or category are listed instead, one page. Redelivery (an edit of
    /// the same ticket) is already handled and records nothing more.
    pub(super) fn trigger_ticket_events(
        &self,
        (owner_id, target_id): (&str, &str),
        events: Option<&[super::TargetTicketEvent]>,
        authorized: &Authorized,
        tx: &mut Transaction<'_>,
    ) -> Vec<DeliveryView> {
        let Ok(owner) = self.owner(owner_id) else {
            return Vec::new();
        };
        let Ok(store) = self.trigger_store(owner_id) else {
            return Vec::new();
        };
        let triggers: Vec<Trigger> = store
            .triggers()
            .unwrap_or_default()
            .into_iter()
            .filter(|trigger| {
                trigger.state.fires()
                    && matches!(&trigger.source, TriggerSource::TicketOpened { target_id: t, .. } if t == target_id)
            })
            .collect();
        if triggers.is_empty() {
            return Vec::new();
        }
        let mut candidates: Vec<(String, String)> = Vec::new();
        match events {
            Some(events) => {
                for event in events {
                    let key = (event.connection_id.clone(), event.native_id.clone());
                    if !event.native_id.is_empty() && !candidates.contains(&key) {
                        candidates.push(key);
                    }
                }
            }
            None => {
                let armed = triggers
                    .iter()
                    .map(|trigger| trigger.cursor.armed_at_ms)
                    .min()
                    .unwrap_or(0);
                for source in authorized.resolution.sources.iter().flatten() {
                    let search = super::ticket_list::TargetTicketSearch {
                        limit: Some(super::ticket_list::MAX_TARGET_TICKET_PAGE),
                        ..Default::default()
                    };
                    if let Ok(page) =
                        self.ticket_page_authorized(authorized, &source.connection_id, &search, tx)
                    {
                        for row in page.tickets {
                            if parse_ms(&row.updated_at).is_none_or(|at| at >= armed) {
                                candidates.push((source.connection_id.clone(), row.native_id));
                            }
                        }
                    }
                }
            }
        }
        let mut views = Vec::new();
        for (connection_id, native_id) in candidates {
            let target = TargetTicket {
                target_id: target_id.into(),
                connection_id: connection_id.clone(),
                native_id: native_id.clone(),
            };
            let Ok(ticket) = self.read_authorized(authorized, &target, tx) else {
                continue;
            };
            let open = !matches!(
                ticket.get("status").and_then(Value::as_str),
                Some("completed" | "verified" | "deleted" | "archive" | "moved")
            );
            if !open {
                continue;
            }
            let created = ticket
                .get("created_at")
                .and_then(Value::as_str)
                .and_then(parse_ms);
            let canonical = ticket
                .get("native_id")
                .and_then(Value::as_str)
                .unwrap_or(&native_id)
                .to_owned();
            let slug = ticket
                .get("slug")
                .and_then(Value::as_str)
                .map(str::to_owned);
            for trigger in &triggers {
                let TriggerSource::TicketOpened { tag, category, .. } = &trigger.source else {
                    continue;
                };
                if created.is_some_and(|at| at < trigger.cursor.armed_at_ms) {
                    continue;
                }
                let has_tag = tag.as_deref().is_none_or(|wanted| {
                    ticket
                        .get("tags")
                        .and_then(Value::as_array)
                        .is_some_and(|tags| tags.iter().any(|t| t.as_str() == Some(wanted)))
                });
                let has_category = category.as_deref().is_none_or(|wanted| {
                    ticket.get("category").and_then(Value::as_str) == Some(wanted)
                });
                if !has_tag || !has_category {
                    continue;
                }
                let key = trigger_keys::ticket(&connection_id, &canonical);
                // An edit of a ticket already delivered is not a new event: nothing is
                // recorded for it.
                if trigger.handled(&key) {
                    continue;
                }
                let mut evidence = BTreeMap::from([(
                    EventField::Ticket.code().to_owned(),
                    format!("{connection_id}:{canonical}"),
                )]);
                if let Some(slug) = &slug {
                    evidence.insert("slug".into(), slug.clone());
                }
                let delivery = Delivery {
                    key,
                    kind: DeliveryKind::TicketOpened,
                    at_ms: tx.clock.now_ms(),
                    evidence,
                    missed: 0,
                };
                if let Ok(Some(view)) = self.deliver(&owner, &trigger.trigger_id, delivery, tx) {
                    views.push(view);
                }
            }
        }
        views
    }

    // ---- Branch watch: a branch's CI and the commits landing on it ----

    /// The triggers that follow a branch and fire.
    pub fn branch_triggers(&self, owner_id: &str) -> Vec<String> {
        self.trigger_store(owner_id)
            .and_then(|store| store.triggers())
            .unwrap_or_default()
            .into_iter()
            .filter(|trigger| trigger.state.fires() && trigger.source.branch().is_some())
            .map(|trigger| trigger.trigger_id)
            .collect()
    }

    /// HS2-KXKA7X hosted half: the commit a branch of a checkout this machine hosts
    /// points at, from its remote-tracking ref (`origin/<branch>`) or else its local
    /// branch. Reads git only; contacts nothing.
    ///
    /// HS2-W8S3TV: with `known` (the head the caller last saw) and a `wait_ms`, an
    /// unchanged head is a long poll: it waits on this server's target feed for the
    /// checkout (whose ref watch reports its remote-tracking refs moving, by kernel
    /// events) and answers as soon as the head differs, or with the unchanged head when
    /// the wait ends. A process without a feed (the bare CLI) answers at once.
    pub fn hosted_branch_head(
        &self,
        checkout_id: &str,
        branch: &str,
        known: Option<&str>,
        wait_ms: u64,
    ) -> Result<Value, OrchestrationError> {
        if !valid_branch(branch) {
            return Err(OrchestrationError::Invalid("a branch name is required"));
        }
        let checkout = self.owner(checkout_id)?;
        let resolve = |reference: String| {
            hotsheet_ticketing::git::command_in(&checkout.root)
                .args(["rev-parse", "--verify", "--quiet"])
                .arg(format!("{reference}^{{commit}}"))
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|text| text.trim().to_ascii_lowercase())
                .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        let read = || {
            resolve(format!("refs/remotes/origin/{branch}"))
                .or_else(|| resolve(format!("refs/heads/{branch}")))
        };
        let answer =
            |commit: Option<String>| serde_json::json!({ "branch": branch, "commit": commit });
        let known = known.map(str::to_ascii_lowercase);
        let wait_ms = wait_ms.min(MAX_BRANCH_WAIT_MS);
        let (Some(known), Some(events), true) = (known, &self.target_events, wait_ms > 0) else {
            return Ok(answer(read()));
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
        // Handshake first (it starts the ref watch), then read: a move in between is
        // either in this reading or reported after the handshake's cursor.
        let mut cursor = match events.poll(&checkout, None, 0) {
            Ok(page) => page.cursor,
            Err(_) => return Ok(answer(read())),
        };
        loop {
            let head = read();
            if head.as_deref() != Some(known.as_str()) {
                return Ok(answer(head));
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let remaining = u64::try_from(remaining.as_millis()).unwrap_or(0);
            if remaining == 0 {
                return Ok(answer(head));
            }
            match events.poll(&checkout, Some(cursor), remaining) {
                Ok(page) => cursor = page.cursor,
                // A stopping server answers with what it has.
                Err(_) => return Ok(answer(read())),
            }
        }
    }

    fn branch_call(
        &self,
        destination: &hotsheet_ticketing::orchestration::TargetDestination,
        watch: &HostedGateWatch,
        start: bool,
        after: Option<u64>,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedGate, OrchestrationError> {
        let reply = if start {
            let body = serde_json::to_value(watch).map_err(|_| OrchestrationError::Corrupt)?;
            self.hosted_call_at(destination, "/target-gates", "POST", "", Some(&body), tx)?
        } else {
            let mut query = format!(
                "?commit={}&repository={}&workflow={}&branch={}&wait_ms={}",
                encode(&watch.commit),
                encode(&watch.repository),
                encode(&watch.workflow),
                encode(&watch.branch),
                wait_ms.min(MAX_BRANCH_WAIT_MS)
            );
            if let Some(after) = after {
                query.push_str(&format!("&after={after}"));
            }
            if let Some(provider) = &watch.provider {
                query.push_str(&format!("&provider={}", encode(provider)));
            }
            self.hosted_call_at(destination, "/target-gates", "GET", &query, None, tx)?
        };
        serde_json::from_value(reply)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
    }

    /// [`Self::trigger_branch_step`] on the real clock, for a host's branch-watch loop.
    pub fn trigger_branch_step_now(
        &self,
        owner_id: &str,
        trigger_id: &str,
        state: &mut BranchFollowState,
        wait_ms: u64,
    ) -> BranchStep {
        let mut rng = super::request::Rng;
        self.trigger_branch_step(
            owner_id,
            trigger_id,
            state,
            wait_ms,
            &mut Transaction {
                clock: &super::request::Clock,
                rng: &mut rng,
            },
        )
    }

    /// One step of a branch watch: start or long-poll the CI watch of the branch's
    /// last known commit where the project lives (the hosted CI gate watch, with that
    /// machine's credentials and its repository check), and deliver what it learned: a
    /// red verdict is one `ci_failed` delivery per provider run and attempt; a newer
    /// commit the provider built is one `commit_landed` delivery. After a verdict the
    /// watch moves to the newest commit built (or watches the same commit again for
    /// the next run); a branch that moved away is read again from the project's git.
    /// The first step only learns the branch's head: nothing earlier is delivered.
    pub fn trigger_branch_step(
        &self,
        owner_id: &str,
        trigger_id: &str,
        state: &mut BranchFollowState,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> BranchStep {
        let ended = BranchStep::default();
        let Ok(owner) = self.owner(owner_id) else {
            return ended;
        };
        let Ok(store) = self.trigger_store(owner_id) else {
            return ended;
        };
        let Ok(Some(trigger)) = store.find(trigger_id) else {
            return ended;
        };
        let (Some(target_id), Some(watch)) = (trigger.source.target_id(), trigger.source.branch())
        else {
            return ended;
        };
        if !trigger.state.fires() {
            return ended;
        }
        let mut step = BranchStep {
            active: true,
            ..BranchStep::default()
        };
        let now = tx.clock.now_ms();
        let fail = |code: &str, step: &mut BranchStep| {
            self.trigger_warning(owner_id, trigger_id, Some(code), now);
            step.failed = true;
        };
        let Some(actor) = trigger_actor(&trigger) else {
            return ended;
        };
        let Ok(target) = Self::target_ref(owner_id, target_id) else {
            fail("target_unknown", &mut step);
            return step;
        };
        let authorized = match self.authorize(
            &target,
            Some(&actor),
            ActionRequest::new(TargetAction::GatesObserve),
            tx,
        ) {
            Ok(authorized) => authorized,
            Err(error) => {
                fail(error.code(), &mut step);
                return step;
            }
        };
        let Ok(destination) = Self::destination(&authorized).cloned() else {
            fail(DenialReason::TargetUnavailable.code(), &mut step);
            return step;
        };
        // Seed: the branch's head as the project's git has it. Nothing is delivered.
        let Some(commit) = trigger.cursor.branch_commit.clone() else {
            let head = self
                .hosted_call_at(
                    &destination,
                    "/target-branch-head",
                    "GET",
                    &format!("?branch={}", encode(&watch.branch)),
                    None,
                    tx,
                )
                .ok()
                .and_then(|reply| {
                    reply
                        .get("commit")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            let Some(head) = head else {
                fail("branch_unknown", &mut step);
                return step;
            };
            let _ = store.update(trigger_id, |current| {
                current.cursor.branch_commit = Some(head);
                Ok(())
            });
            self.trigger_warning(owner_id, trigger_id, None, now);
            *state = BranchFollowState::default();
            return step;
        };
        let hosted = HostedGateWatch {
            commit: commit.clone(),
            repository: watch.repository.clone(),
            workflow: watch.workflow.clone(),
            branch: watch.branch.clone(),
            artifact: None,
            provider: watch.provider.clone(),
            again: state.again,
        };
        let reading = if state.started {
            self.branch_call(&destination, &hosted, false, state.cursor, wait_ms, tx)
        } else {
            self.branch_call(&destination, &hosted, true, None, 0, tx)
        };
        let reading = match reading {
            Ok(reading) => reading,
            Err(error) => {
                state.started = false;
                fail(error.code(), &mut step);
                return step;
            }
        };
        self.trigger_warning(owner_id, trigger_id, None, now);
        state.started = true;
        state.again = false;
        state.cursor = Some(reading.cursor);
        if reading.state == GateState::Unknown && reading.reason.as_deref() == Some("not_watched") {
            state.started = false;
            return step;
        }
        let built = reading
            .evidence
            .as_ref()
            .map(|evidence| evidence.commit.to_ascii_lowercase());
        let mut deliveries = Vec::new();
        if let (TriggerSource::CommitLanded { .. }, Some(built)) = (&trigger.source, &built)
            && built != &commit
        {
            deliveries.push(Delivery {
                key: trigger_keys::commit(&watch.branch, built),
                kind: DeliveryKind::CommitLanded,
                at_ms: now,
                evidence: BTreeMap::from([
                    (EventField::Commit.code().to_owned(), built.clone()),
                    (EventField::Branch.code().to_owned(), watch.branch.clone()),
                ]),
                missed: 0,
            });
        }
        if let (TriggerSource::BranchCiFailed { .. }, GateState::Failed, Some(evidence)) =
            (&trigger.source, reading.state, &reading.evidence)
        {
            deliveries.push(Delivery {
                key: trigger_keys::ci(watch.provider(), &evidence.reference, evidence.attempt),
                kind: DeliveryKind::CiFailed,
                at_ms: now,
                evidence: BTreeMap::from([
                    (
                        EventField::Commit.code().to_owned(),
                        evidence.commit.clone(),
                    ),
                    (
                        EventField::CiRun.code().to_owned(),
                        format!("{}.{}", evidence.reference, evidence.attempt),
                    ),
                    (EventField::Branch.code().to_owned(), watch.branch.clone()),
                ]),
                missed: 0,
            });
        }
        // After a verdict the watch moves on: to the newest commit built, or the same
        // commit watched again for its next run. A branch that moved away is read again.
        match reading.state {
            GateState::Passed | GateState::Failed => {
                let next = built.clone().filter(|built| built != &commit);
                let _ = store.update(trigger_id, |current| {
                    if let Some(next) = &next {
                        current.cursor.branch_commit = Some(next.clone());
                    }
                    Ok(())
                });
                state.started = false;
                state.again = next.is_none();
                state.cursor = None;
            }
            GateState::Superseded => {
                let _ = store.update(trigger_id, |current| {
                    current.cursor.branch_commit = None;
                    Ok(())
                });
                *state = BranchFollowState::default();
            }
            _ => {}
        }
        for delivery in deliveries {
            if let Ok(Some(view)) = self.deliver(&owner, trigger_id, delivery, tx) {
                step.deliveries.push(view);
            }
        }
        step
    }

    // ---- Commit landed: the branch head itself (HS2-W8S3TV) ----

    /// The `commit_landed` triggers that fire: each also follows its branch's head.
    pub fn head_triggers(&self, owner_id: &str) -> Vec<String> {
        self.trigger_store(owner_id)
            .and_then(|store| store.triggers())
            .unwrap_or_default()
            .into_iter()
            .filter(|trigger| {
                trigger.state.fires()
                    && matches!(trigger.source, TriggerSource::CommitLanded { .. })
            })
            .map(|trigger| trigger.trigger_id)
            .collect()
    }

    /// [`Self::trigger_head_step`] on the real clock, for a host's head-watch loop.
    pub fn trigger_head_step_now(
        &self,
        owner_id: &str,
        trigger_id: &str,
        wait_ms: u64,
    ) -> BranchStep {
        let mut rng = super::request::Rng;
        self.trigger_head_step(
            owner_id,
            trigger_id,
            wait_ms,
            &mut Transaction {
                clock: &super::request::Clock,
                rng: &mut rng,
            },
        )
    }

    /// HS2-W8S3TV: one step of following a `commit_landed` trigger's branch by its head
    /// where the project lives, not by CI: a branch without CI still lands commits. The
    /// first step learns the head and delivers nothing. Each later step holds one
    /// bounded long poll open on `target-branch-head` with the head last seen; the host
    /// answers when the checkout's remote-tracking ref moves (its ref watch, by kernel
    /// events), and a new head is one `commit_landed` delivery, keyed as the CI watch
    /// keys a landed commit, so the two sources never deliver one commit twice. A host
    /// that cannot wait answers at once, and the caller backs off.
    pub fn trigger_head_step(
        &self,
        owner_id: &str,
        trigger_id: &str,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> BranchStep {
        let ended = BranchStep::default();
        let Ok(owner) = self.owner(owner_id) else {
            return ended;
        };
        let Ok(store) = self.trigger_store(owner_id) else {
            return ended;
        };
        let Ok(Some(trigger)) = store.find(trigger_id) else {
            return ended;
        };
        let (TriggerSource::CommitLanded { target_id, watch }, true) =
            (&trigger.source, trigger.state.fires())
        else {
            return ended;
        };
        let Some(actor) = trigger_actor(&trigger) else {
            return ended;
        };
        let mut step = BranchStep {
            active: true,
            ..BranchStep::default()
        };
        let now = tx.clock.now_ms();
        let fail = |code: &str, step: &mut BranchStep| {
            self.trigger_warning(owner_id, trigger_id, Some(code), now);
            step.failed = true;
        };
        let Ok(target) = Self::target_ref(owner_id, target_id) else {
            fail("target_unknown", &mut step);
            return step;
        };
        let authorized = match self.authorize(
            &target,
            Some(&actor),
            ActionRequest::new(TargetAction::GatesObserve),
            tx,
        ) {
            Ok(authorized) => authorized,
            Err(error) => {
                fail(error.code(), &mut step);
                return step;
            }
        };
        let Ok(destination) = Self::destination(&authorized).cloned() else {
            fail(DenialReason::TargetUnavailable.code(), &mut step);
            return step;
        };
        let known = trigger.cursor.branch_head.clone();
        let mut query = format!("?branch={}", encode(&watch.branch));
        if let Some(known) = &known {
            query.push_str(&format!(
                "&known={}&wait_ms={}",
                encode(known),
                wait_ms.min(MAX_BRANCH_WAIT_MS)
            ));
        }
        let head =
            match self.hosted_call_at(&destination, "/target-branch-head", "GET", &query, None, tx)
            {
                Ok(reply) => reply
                    .get("commit")
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase),
                Err(error) => {
                    fail(error.code(), &mut step);
                    return step;
                }
            };
        let Some(head) = head else {
            fail("branch_unknown", &mut step);
            return step;
        };
        self.trigger_warning(owner_id, trigger_id, None, now);
        if known.as_deref() == Some(head.as_str()) {
            return step;
        }
        let _ = store.update(trigger_id, |current| {
            current.cursor.branch_head = Some(head.clone());
            Ok(())
        });
        // Seed: the head the trigger starts from. Nothing earlier is delivered.
        if known.is_none() {
            return step;
        }
        let delivery = Delivery {
            key: trigger_keys::commit(&watch.branch, &head),
            kind: DeliveryKind::CommitLanded,
            at_ms: now,
            evidence: BTreeMap::from([
                (EventField::Commit.code().to_owned(), head.clone()),
                (EventField::Branch.code().to_owned(), watch.branch.clone()),
            ]),
            missed: 0,
        };
        if let Ok(Some(view)) = self.deliver(&owner, trigger_id, delivery, tx) {
            step.deliveries.push(view);
        }
        step
    }
}

fn next_instants(
    schedule: &hotsheet_ticketing::orchestration::TriggerSchedule,
    now_ms: u64,
) -> Vec<u64> {
    hotsheet_ticketing::orchestration::schedule_next_ms(schedule, now_ms, TRIGGER_NEXT_RUNS)
}

fn schedule_between(
    schedule: &hotsheet_ticketing::orchestration::TriggerSchedule,
    after_ms: u64,
    now_ms: u64,
) -> Option<(String, u32)> {
    hotsheet_ticketing::orchestration::schedule_due(schedule, after_ms, now_ms)
}

fn parse_ms(text: &str) -> Option<u64> {
    let parsed =
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).ok()?;
    u64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

fn delivery_label(kind: DeliveryKind) -> &'static str {
    match kind {
        DeliveryKind::Schedule => "scheduled",
        DeliveryKind::TicketOpened => "ticket opened",
        DeliveryKind::GateFailed => "CI gate failed",
        DeliveryKind::CiFailed => "branch CI failed",
        DeliveryKind::CommitLanded => "commit landed",
        DeliveryKind::Manual => "manual",
    }
}
