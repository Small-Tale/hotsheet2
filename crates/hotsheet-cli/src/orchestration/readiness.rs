//! HS2-42TARA: the plan of prerequisites and the readiness computed from this
//! machine's records.
//!
//! Reads (`readiness`, `readiness_plan`, `ready_work`) contact no target, pass no
//! gate and write nothing. Defining and refreshing read target tickets through the
//! gate. A level change is published once to the owner's run event stream, and it is
//! recomputed where run, gate and validation changes are published, never on a timer.
use std::collections::{BTreeMap, BTreeSet};

use hotsheet_model::NoteKind;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::ops;
use hotsheet_ticketing::orchestration::{
    ApprovalGate, ApprovalRegistry, ApprovalState, AuditOutcome, AuditRecord, DenialReason,
    GateFacts, GateFilter, GateRequirement, MAX_STEP_RESOURCES, NewPlanStep, PlanFacts,
    PlanRegistry, PlanStep, Prerequisite, ProvisionalStanding, ReadinessError, ReadinessLevel,
    ReadinessReason, ResourceNeed, RunEvent, RunEventJournal, RunEventKind, RunEventSource,
    RunTarget, RunTicket, RunUnreachable, StepReadiness, TargetAction, TargetOpError, TicketFacts,
    TicketKey, TicketSeen, TicketUpdate, ValidationRegistry, evaluate_plan, summarize, ticket_key,
    validate_needs,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::runs::{field_ms, text};
use super::tickets::{identifier, native, timestamp};
use super::*;

/// A prerequisite as a caller names it. Native ids may be slugs; the plan stores the
/// provider's canonical id.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrerequisiteRequest {
    Ticket {
        target_id: String,
        connection_id: String,
        native_id: String,
        #[serde(default)]
        gates: GateRequirement,
        #[serde(default)]
        commit: Option<String>,
    },
    Validation {
        /// The step's own orchestration ticket when omitted.
        #[serde(default)]
        ticket: Option<OrchestrationTicket>,
        step_id: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinePlanStep {
    pub ticket: OrchestrationTicket,
    #[serde(flatten)]
    pub target: TargetTicket,
    #[serde(default)]
    pub prerequisites: Vec<PrerequisiteRequest>,
    /// HS2-2FG91X: resources the session working the step must hold.
    #[serde(default)]
    pub resources: Vec<ResourceNeed>,
    /// HS2-HCW7MD: the AI tool preferred for the work, by plugin id. A preference
    /// recorded with the step; the scheduler does not read it yet (HS2-TH53NN).
    #[serde(default)]
    pub tool: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanStepRef {
    pub step_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshReadiness {
    /// One step's tickets; every step's when omitted.
    #[serde(default)]
    pub step_id: Option<String>,
}

/// A step's readiness with when its current level was first published.
#[derive(Debug, Clone, Serialize)]
pub struct StepView {
    #[serde(flatten)]
    pub readiness: StepReadiness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<u64>,
    /// HS2-JSF985: for a provisional step under a policy that opts in, whether the
    /// provisional rule lets it start now, on which gates, or why not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speculation: Option<ProvisionalStanding>,
    /// HS2-JSF985: the work that was started on this step ahead of a pending gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisional: Option<ProvisionalWork>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanView {
    pub steps: Vec<StepView>,
    /// How many steps stand at each level; levels with none are left out.
    pub counts: BTreeMap<&'static str, usize>,
    /// Each cycle once, as its sorted step ids.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cycles: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefreshView {
    #[serde(flatten)]
    pub plan: PlanView,
    /// Tickets whose read could not be confirmed; they are stale until one is.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unverified: Vec<RunTarget>,
}

/// One piece of work that could be started, for the scheduler (HS2-ECWG50).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadyItem {
    pub step_id: String,
    pub ticket: RunTicket,
    pub target: RunTarget,
    /// `confirmed`, or `provisional` when it rests on a pending gate.
    pub level: ReadinessLevel,
    /// Empty for confirmed work.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<ReadinessReason>,
    /// The commits provisional work would be built on.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub base_commits: Vec<String>,
    /// Unfinished steps waiting on this one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unblocks: Vec<String>,
    /// The target ticket's own priority, Up Next mark and creation time as last
    /// read. Absent when the read did not say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up_next: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket_created_at_ms: Option<u64>,
    /// When the step reached this level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_since_ms: Option<u64>,
    /// HS2-2FG91X: resources the step itself requires. The policy may require more
    /// of its whole target.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceNeed>,
    /// HS2-JSF985: the provisional rule's answer for a provisional item, when the
    /// policy opts in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speculation: Option<ProvisionalStanding>,
}

/// One audit entry of a plan change or an override.
struct Audited<'a> {
    action: &'a str,
    outcome: AuditOutcome,
    reason: Option<&'a str>,
    operation_id: Option<&'a str>,
}

/// The dispatch a guard decides about.
pub(super) struct GuardedDispatch<'a> {
    pub(super) target: &'a RunTarget,
    /// The target ticket as just read for this dispatch.
    pub(super) found: &'a Value,
    pub(super) operation_id: &'a str,
    pub(super) override_readiness: bool,
}

fn seen(ticket: &Value, now_ms: u64, opt_out_tag: Option<&str>) -> TicketSeen {
    TicketSeen {
        // HS2-JSF985: read only while the policy opts in to provisional work, so a
        // plan written without the opt-in is what it always was. A ticket without
        // tags carries none, so it is not opted out.
        opted_out: opt_out_tag.map(|opt_out_tag| {
            ticket
                .get("tags")
                .and_then(Value::as_array)
                .is_some_and(|tags| {
                    tags.iter()
                        .filter_map(Value::as_str)
                        .any(|tag| tag.trim().eq_ignore_ascii_case(opt_out_tag))
                })
        }),
        at_ms: now_ms,
        exists: true,
        status: text(ticket, "status").unwrap_or("unknown").into(),
        priority: text(ticket, "priority").map(Into::into),
        up_next: ticket.get("up_next").and_then(Value::as_bool),
        created_at_ms: field_ms(ticket, "created_at"),
        slug: text(ticket, "slug").map(Into::into),
    }
}

fn as_target(target: &RunTarget) -> TargetTicket {
    TargetTicket {
        target_id: target.target_id.clone(),
        connection_id: target.connection_id.clone(),
        native_id: target.native_id.clone(),
    }
}

fn named_tickets(step: &PlanStep) -> impl Iterator<Item = RunTarget> + '_ {
    step.prerequisites
        .iter()
        .filter_map(|prerequisite| match prerequisite {
            Prerequisite::Ticket {
                target_id,
                connection_id,
                native_id,
                ..
            } => Some(RunTarget {
                target_id: target_id.clone(),
                connection_id: connection_id.clone(),
                native_id: native_id.clone(),
            }),
            Prerequisite::Validation { .. } => None,
        })
        .chain([step.target.clone()])
}

impl OrchestrationRuntime {
    pub(super) fn plan(&self) -> PlanRegistry {
        PlanRegistry::new(&self.home)
    }

    pub(super) fn plan_step(
        &self,
        owner_id: &str,
        step_id: &str,
    ) -> Result<PlanStep, OrchestrationError> {
        self.plan().get(owner_id, step_id)
    }

    /// Everything evaluation reads, from this machine's records alone.
    fn plan_facts(
        &self,
        owner_id: &str,
        steps: &[PlanStep],
        now_ms: u64,
    ) -> Result<PlanFacts, OrchestrationError> {
        let mut facts = PlanFacts::default();
        let stored: BTreeMap<TicketKey, TicketFacts> = self
            .plan()
            .tickets(owner_id)?
            .into_iter()
            .map(|record| (ticket_key(&record.target), record.facts))
            .collect();
        let runs = self.runs().list(owner_id, false)?;
        // Standing gates only: a superseded link counts for nothing (§26.13).
        let gates = self.gate_summary(owner_id, &GateFilter::default())?.gates;
        let of = |keep: &dyn Fn(&hotsheet_ticketing::orchestration::Gate) -> bool| {
            GateFacts::from(&summarize(
                gates.iter().filter(|gate| keep(gate)).cloned().collect(),
            ))
        };
        for target in steps.iter().flat_map(named_tickets) {
            let key = ticket_key(&target);
            if facts.tickets.contains_key(&key) {
                continue;
            }
            let mut known = stored.get(&key).cloned().unwrap_or_default();
            let mut latest: Option<&hotsheet_ticketing::orchestration::Run> = None;
            for run in runs.iter().filter(|run| run.target == target) {
                // §26.9: a run's last confirmed observation and whether it is stale.
                known = known.merge(TicketFacts {
                    observed: run.observed.as_ref().map(|observed| TicketSeen {
                        at_ms: observed.at_ms,
                        exists: true,
                        status: observed.status.clone(),
                        priority: None,
                        up_next: None,
                        created_at_ms: None,
                        opted_out: None,
                        slug: None,
                    }),
                    unreachable: run.unreachable.clone(),
                });
                if !run.state.is_terminal() {
                    facts.active.insert(key.clone());
                }
                if latest.is_none_or(|other| {
                    (run.created_at_ms, &run.run_id) > (other.created_at_ms, &other.run_id)
                }) {
                    latest = Some(run);
                }
            }
            if let Some(run) = latest {
                facts
                    .gates
                    .insert(key.clone(), of(&|gate| gate.run_id == run.run_id));
            }
            facts.tickets.insert(key, known);
        }
        for step in steps {
            for prerequisite in &step.prerequisites {
                if let Prerequisite::Ticket {
                    target_id,
                    commit: Some(commit),
                    ..
                } = prerequisite
                {
                    facts.commit_gates.insert(
                        (target_id.clone(), commit.clone()),
                        of(&|gate| gate.target_id == *target_id && gate.commit == *commit),
                    );
                }
            }
        }
        // §26.12: the latest evidence per orchestration ticket and step.
        for evidence in ValidationRegistry::new(&self.home).steps(owner_id, None, now_ms)? {
            facts.validations.insert(
                (
                    evidence.step.ticket.connection_id.clone(),
                    evidence.step.ticket.native_id.clone(),
                    evidence.step.definition.step_id.clone(),
                ),
                (evidence.state, evidence.reason),
            );
        }
        Ok(facts)
    }

    pub(super) fn plan_evaluate(
        &self,
        owner_id: &str,
        now_ms: u64,
    ) -> Result<Vec<StepView>, OrchestrationError> {
        let records = self.plan().records(owner_id)?;
        let steps: Vec<PlanStep> = records.iter().map(|record| record.step.clone()).collect();
        if steps.is_empty() {
            return Ok(Vec::new());
        }
        let facts = self.plan_facts(owner_id, &steps, now_ms)?;
        let answers = evaluate_plan(&steps, &facts);
        // HS2-JSF985: both are empty while the policy does not opt in.
        let (mut standings, mut work) = self.provisional_annotations(owner_id, &answers, &facts)?;
        Ok(answers
            .into_iter()
            .map(|readiness| StepView {
                speculation: standings.remove(&readiness.step.step_id),
                provisional: work.remove(&readiness.step.step_id),
                since_ms: records
                    .iter()
                    .find(|record| record.step.step_id == readiness.step.step_id)
                    .and_then(|record| record.published.as_ref())
                    .filter(|published| published.level == readiness.level)
                    .map(|published| published.since_ms),
                readiness,
            })
            .collect())
    }

    /// Recompute the owner's plan and publish each level that changed as one
    /// `readiness` run event. Best effort, like every event: the operation that
    /// caused it is never failed because its consequence could not be published.
    pub(super) fn readiness_reevaluate(&self, owner_id: &str, now_ms: u64) {
        let _ = self.readiness_publish(owner_id, now_ms);
    }

    fn readiness_publish(&self, owner_id: &str, now_ms: u64) -> Result<(), OrchestrationError> {
        let answers = self.plan_evaluate(owner_id, now_ms)?;
        if answers.is_empty() {
            return Ok(());
        }
        let levels: Vec<(String, ReadinessLevel)> = answers
            .iter()
            .map(|view| (view.readiness.step.step_id.clone(), view.readiness.level))
            .collect();
        let changes = self.plan().publish(owner_id, &levels, now_ms)?;
        if changes.is_empty() {
            return Ok(());
        }
        let journal = RunEventJournal::new(&self.home, owner_id)?;
        for change in changes {
            let _ = journal.record(RunEvent {
                at_ms: now_ms,
                // A step may have no run yet, so the event is about the step itself.
                run_id: change.step_id,
                kind: RunEventKind::Readiness,
                source: RunEventSource::Orchestrator,
                state: None,
                reason: Some(change.to.code().into()),
                actor_id: None,
                dedupe: Some(format!("readiness-{}", change.seq)),
                gate_id: None,
                gate_state: None,
            });
        }
        Ok(())
    }

    /// The readiness of one step, from this machine's records.
    pub fn readiness(
        &self,
        owner_id: &str,
        step_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<StepView, OrchestrationError> {
        self.plan().get(owner_id, step_id)?;
        self.plan_evaluate(owner_id, tx.clock.now_ms())?
            .into_iter()
            .find(|view| view.readiness.step.step_id == step_id)
            .ok_or(OrchestrationError::Readiness(ReadinessError::StepNotFound))
    }

    fn plan_view(steps: Vec<StepView>) -> PlanView {
        let mut counts = BTreeMap::new();
        let mut cycles: BTreeSet<Vec<String>> = BTreeSet::new();
        for view in &steps {
            *counts.entry(view.readiness.level.code()).or_insert(0) += 1;
            if !view.readiness.cycle.is_empty() {
                cycles.insert(view.readiness.cycle.clone());
            }
        }
        PlanView {
            steps,
            counts,
            cycles: cycles.into_iter().collect(),
        }
    }

    /// Every step of the owner's plan with its level and reasons, so blocked, stale
    /// and cyclic plans are visible.
    pub fn readiness_plan(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<PlanView, OrchestrationError> {
        Ok(Self::plan_view(
            self.plan_evaluate(owner_id, tx.clock.now_ms())?,
        ))
    }

    /// The work that could be started now: confirmed items first, then provisional
    /// ones, each by step id. Only `confirmed` work is dispatched today; the
    /// provisional level is listed so a policy can allow it later (HS2-2SJPCH,
    /// HS2-JSF985). Nothing blocked, stale, cyclic, in progress or finished is listed.
    pub fn ready_work(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<ReadyItem>, OrchestrationError> {
        let stored = self.plan().tickets(owner_id)?;
        let mut items: Vec<ReadyItem> = self
            .plan_evaluate(owner_id, tx.clock.now_ms())?
            .into_iter()
            .filter(|view| view.readiness.level.is_ready())
            .map(|view| {
                let speculation = view.speculation;
                let StepReadiness {
                    step,
                    level,
                    reasons,
                    base_commits,
                    unblocks,
                    ..
                } = view.readiness;
                let read = stored
                    .iter()
                    .find(|record| record.target == step.target)
                    .and_then(|record| record.facts.observed.as_ref());
                ReadyItem {
                    step_id: step.step_id,
                    ticket: step.ticket,
                    resources: step.resources,
                    priority: read.and_then(|seen| seen.priority.clone()),
                    up_next: read.and_then(|seen| seen.up_next),
                    ticket_created_at_ms: read.and_then(|seen| seen.created_at_ms),
                    target: step.target,
                    level,
                    reasons,
                    base_commits,
                    unblocks,
                    ready_since_ms: view.since_ms,
                    speculation,
                }
            })
            .collect();
        items.sort_by(|a, b| (a.level, &a.step_id).cmp(&(b.level, &b.step_id)));
        Ok(items)
    }

    fn readiness_audit(
        &self,
        owner_id: &str,
        target_id: &str,
        actor: Option<&MutationActor>,
        entry: Audited<'_>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        self.audit_record(
            owner_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: Some(target_id.into()),
                action: entry.action.into(),
                operation_id: entry.operation_id.map(Into::into),
                outcome: entry.outcome,
                reason: entry.reason.map(Into::into),
                grant_id: None,
            },
        )
    }

    /// Plan a step: one orchestration ticket's work on one exact target ticket, with
    /// what must hold first. Every ticket named is read through the gate
    /// (`tickets_read`), so the plan stores canonical ids and starts from confirmed
    /// state; a ticket that cannot be read is not planned.
    pub fn readiness_define(
        &self,
        owner_id: &str,
        request: DefinePlanStep,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<StepView, OrchestrationError> {
        // HS2-2FG91X: a malformed requirement is refused before any target is read.
        validate_needs(&request.resources, MAX_STEP_RESOURCES)?;
        if let Some(tool) = &request.tool {
            identifier(tool, "bounded tool id required")?;
        }
        let (_, source_id, source_connection) =
            self.orchestration_store(owner_id, &request.ticket)?;
        let ticket = RunTicket {
            connection_id: source_connection,
            native_id: source_id.to_string(),
        };
        let mut wanted = vec![request.target.clone()];
        for prerequisite in &request.prerequisites {
            if let PrerequisiteRequest::Ticket {
                target_id,
                connection_id,
                native_id,
                ..
            } = prerequisite
            {
                wanted.push(TargetTicket {
                    target_id: target_id.clone(),
                    connection_id: connection_id.clone(),
                    native_id: native_id.clone(),
                });
            }
        }
        for target in &wanted {
            identifier(&target.connection_id, "bounded connection id required")?;
            native(&target.native_id)?;
        }
        let opt_out_tag = self.opt_out_tag(owner_id);
        // One gate answer per target project, however many of its tickets are named.
        let mut gates: Vec<(TargetRef, Authorized)> = Vec::new();
        let mut read: Vec<(RunTarget, TicketSeen)> = Vec::new();
        let outcome = (|| {
            for target in &wanted {
                if !gates
                    .iter()
                    .any(|(reference, _)| reference.target_id == target.target_id)
                {
                    let reference = Self::target_ref(owner_id, &target.target_id)?;
                    let authorized = self.authorize(
                        &reference,
                        actor,
                        ActionRequest::new(TargetAction::TicketsRead),
                        tx,
                    )?;
                    gates.push((reference, authorized));
                }
                let authorized = gates
                    .iter()
                    .find(|(reference, _)| reference.target_id == target.target_id)
                    .map(|(_, authorized)| authorized)
                    .ok_or(OrchestrationError::Corrupt)?;
                let found = self.read_authorized(authorized, target, tx)?;
                read.push((
                    RunTarget {
                        target_id: target.target_id.clone(),
                        connection_id: target.connection_id.clone(),
                        native_id: text(&found, "native_id")
                            .unwrap_or(&target.native_id)
                            .to_owned(),
                    },
                    seen(&found, tx.clock.now_ms(), opt_out_tag.as_deref()),
                ));
            }
            let mut canonical = read.iter().map(|(target, _)| target.clone());
            let target = canonical.next().ok_or(OrchestrationError::Corrupt)?;
            let prerequisites = request
                .prerequisites
                .iter()
                .map(|prerequisite| {
                    Ok(match prerequisite {
                        PrerequisiteRequest::Ticket { gates, commit, .. } => {
                            let named = canonical.next().ok_or(OrchestrationError::Corrupt)?;
                            Prerequisite::Ticket {
                                target_id: named.target_id,
                                connection_id: named.connection_id,
                                native_id: named.native_id,
                                gates: *gates,
                                commit: commit.as_ref().map(|sha| sha.to_ascii_lowercase()),
                            }
                        }
                        PrerequisiteRequest::Validation {
                            ticket: other,
                            step_id,
                        } => Prerequisite::Validation {
                            ticket: match other {
                                None => ticket.clone(),
                                Some(other) => {
                                    let (_, id, connection_id) =
                                        self.orchestration_store(owner_id, other)?;
                                    RunTicket {
                                        connection_id,
                                        native_id: id.to_string(),
                                    }
                                }
                            },
                            step_id: step_id.clone(),
                        },
                    })
                })
                .collect::<Result<Vec<_>, OrchestrationError>>()?;
            let (step, changed) = self.plan().define(
                owner_id,
                NewPlanStep {
                    ticket: ticket.clone(),
                    target,
                    prerequisites,
                    resources: request.resources.clone(),
                    tool: request.tool.clone(),
                },
                actor.and_then(|actor| actor.id.clone()),
                tx,
            )?;
            for (target, seen) in &read {
                self.plan().observe(
                    owner_id,
                    target,
                    TicketUpdate {
                        observed: Some(seen.clone()),
                        unreachable: Some(None),
                    },
                )?;
            }
            if changed {
                // HS2-2FG91X: a changed requirement moves no level, so no readiness
                // event would tell a running scheduler to look again.
                self.scheduler_wake(owner_id);
                self.readiness_audit(
                    owner_id,
                    &step.target.target_id,
                    actor,
                    Audited {
                        action: "readiness_define",
                        outcome: AuditOutcome::Changed,
                        reason: None,
                        operation_id: None,
                    },
                    tx,
                )?;
            }
            Ok(step)
        })();
        for (reference, authorized) in &gates {
            self.finish(reference, authorized, &outcome, tx)?;
        }
        let step = outcome?;
        self.readiness_publish(owner_id, tx.clock.now_ms())?;
        self.readiness(owner_id, &step.step_id, tx)
    }

    /// Withdraw a step from the plan. Only this machine's record changes, so it works
    /// while a target is unbound or unreachable; it needs an identified actor and is
    /// audited.
    pub fn readiness_remove(
        &self,
        owner_id: &str,
        request: &PlanStepRef,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<PlanStep, OrchestrationError> {
        if actor.and_then(|actor| actor.id.as_ref()).is_none() {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to change the plan",
            ));
        }
        let step = self.plan().remove(owner_id, &request.step_id)?;
        self.readiness_audit(
            owner_id,
            &step.target.target_id,
            actor,
            Audited {
                action: "readiness_remove",
                outcome: AuditOutcome::Changed,
                reason: None,
                operation_id: None,
            },
            tx,
        )?;
        self.readiness_reevaluate(owner_id, tx.clock.now_ms());
        Ok(step)
    }

    /// Read again every ticket a step (or the whole plan) names, through the gate as
    /// `tickets_read`. A ticket that cannot be confirmed is recorded as unreachable
    /// and listed under `unverified`; its last read stays, as stale. This is the
    /// explicit way to confirm a prerequisite no run follows.
    pub fn readiness_refresh(
        &self,
        owner_id: &str,
        request: &RefreshReadiness,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RefreshView, OrchestrationError> {
        let steps: Vec<PlanStep> = match &request.step_id {
            Some(step_id) => vec![self.plan().get(owner_id, step_id)?],
            None => self
                .plan()
                .records(owner_id)?
                .into_iter()
                .map(|record| record.step)
                .collect(),
        };
        let mut by_target: BTreeMap<String, Vec<RunTarget>> = BTreeMap::new();
        for target in steps.iter().flat_map(named_tickets) {
            let tickets = by_target.entry(target.target_id.clone()).or_default();
            if !tickets.contains(&target) {
                tickets.push(target);
            }
        }
        let mut unverified = Vec::new();
        let opt_out_tag = self.opt_out_tag(owner_id);
        for (target_id, tickets) in by_target {
            let reference = Self::target_ref(owner_id, &target_id)?;
            let authorized = self.authorize(
                &reference,
                actor,
                ActionRequest::new(TargetAction::TicketsRead),
                tx,
            );
            let mut failed = None;
            for target in tickets {
                let update =
                    match authorized
                        .as_ref()
                        .map_err(|error| error.code())
                        .and_then(|authorized| {
                            match self.read_authorized(authorized, &as_target(&target), tx) {
                                Ok(found) => Ok(Some(found)),
                                // The target answered: it has no such ticket.
                                Err(OrchestrationError::TargetOperation(
                                    TargetOpError::NotFound,
                                )) => Ok(None),
                                Err(error) => Err(error.code()),
                            }
                        }) {
                        Ok(found) => TicketUpdate {
                            observed: Some(match found {
                                Some(found) => {
                                    seen(&found, tx.clock.now_ms(), opt_out_tag.as_deref())
                                }
                                None => TicketSeen {
                                    at_ms: tx.clock.now_ms(),
                                    exists: false,
                                    status: String::new(),
                                    priority: None,
                                    up_next: None,
                                    created_at_ms: None,
                                    opted_out: None,
                                    slug: None,
                                },
                            }),
                            unreachable: Some(None),
                        },
                        Err(code) => {
                            failed = Some(code);
                            unverified.push(target.clone());
                            TicketUpdate {
                                observed: None,
                                unreachable: Some(Some(RunUnreachable {
                                    since_ms: tx.clock.now_ms(),
                                    reason: code.into(),
                                })),
                            }
                        }
                    };
                self.plan().observe(owner_id, &target, update)?;
            }
            if let Ok(authorized) = &authorized {
                self.record_outcome(&reference, authorized, failed.is_none(), failed, tx)?;
            }
        }
        self.readiness_publish(owner_id, tx.clock.now_ms())?;
        let mut plan = self.plan_evaluate(owner_id, tx.clock.now_ms())?;
        if let Some(step_id) = &request.step_id {
            plan.retain(|view| view.readiness.step.step_id == *step_id);
        }
        Ok(RefreshView {
            plan: Self::plan_view(plan),
            unverified,
        })
    }

    /// The dispatch guard: work on a planned target ticket starts only at the
    /// confirmed level. A ticket no step names is not planned and is not gated. The
    /// freshly read target ticket becomes the step's own confirmed state first.
    ///
    /// With `override_readiness` a person is asked for this exact operation under the
    /// `readiness_override` approval (§26.8); the answer and its use are audited and
    /// noted on the orchestration ticket by identifiers only.
    pub(super) fn readiness_guard(
        &self,
        owner_id: &str,
        dispatch: &GuardedDispatch<'_>,
        authorized: &Authorized,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        let GuardedDispatch {
            target,
            found,
            operation_id,
            override_readiness,
        } = *dispatch;
        let Some(step) = self.plan().find_target(owner_id, target)? else {
            return Ok(());
        };
        // Repeating an admitted dispatch continues that decision (§26.9); an override
        // was used once and is not asked for again.
        if self
            .runs()
            .list(owner_id, false)?
            .iter()
            .any(|run| run.operation_id == operation_id)
        {
            return Ok(());
        }
        let now_ms = tx.clock.now_ms();
        self.plan().observe(
            owner_id,
            target,
            TicketUpdate {
                observed: Some(seen(found, now_ms, self.opt_out_tag(owner_id).as_deref())),
                unreachable: Some(None),
            },
        )?;
        self.readiness_reevaluate(owner_id, now_ms);
        let view = self.readiness(owner_id, &step.step_id, tx)?;
        let Some(refusal) = view.readiness.level.refusal() else {
            return Ok(());
        };
        if !override_readiness {
            // HS2-JSF985: a provisional step is the policy's decision. Without its
            // opt-in this answers the refusal it always did.
            if view.readiness.level == ReadinessLevel::Provisional {
                return self.provisional_admit(
                    owner_id,
                    &step,
                    operation_id,
                    authorized,
                    actor,
                    tx,
                );
            }
            return Err(OrchestrationError::Readiness(refusal));
        }
        let reference = Self::target_ref(owner_id, &target.target_id)?;
        let approvals = ApprovalRegistry::new(&self.home);
        let asked = |approval: &hotsheet_ticketing::orchestration::Approval| {
            approval.target == reference
                && approval.actor_id == authorized.actor_id
                && approval.gate == ApprovalGate::ReadinessOverride
                && approval.operation_id == operation_id
        };
        let level = view.readiness.level.code();
        let audit = |outcome, reason: &str, tx: &mut Transaction<'_>| {
            self.readiness_audit(
                owner_id,
                &target.target_id,
                actor,
                Audited {
                    action: "readiness_override",
                    outcome,
                    reason: Some(reason),
                    operation_id: Some(operation_id),
                },
                tx,
            )
        };
        if approvals
            .list(owner_id, now_ms)?
            .iter()
            .rev()
            .find(|approval| asked(approval))
            .is_some_and(|approval| approval.state == ApprovalState::Denied)
        {
            audit(AuditOutcome::Denied, "approval_denied", tx)?;
            return Err(OrchestrationError::Readiness(
                ReadinessError::OverrideDenied,
            ));
        }
        if !approvals.consume_or_request(
            &reference,
            &authorized.actor_id,
            ApprovalGate::ReadinessOverride,
            operation_id,
            tx,
        )? {
            audit(AuditOutcome::Denied, "approval_required", tx)?;
            return Err(OrchestrationError::Denied(DenialReason::ApprovalRequired));
        }
        // The level that was overridden is the audit reason.
        audit(AuditOutcome::Allowed, level, tx)?;
        let approval = approvals
            .list(owner_id, now_ms)?
            .into_iter()
            .rev()
            .find(|approval| asked(approval));
        let ticket = OrchestrationTicket {
            connection_id: step.ticket.connection_id.clone(),
            native_id: step.ticket.native_id.clone(),
        };
        // Best effort, like a run's own note; the audit entry above is the record.
        if let Ok((store, id, _)) = self.orchestration_store(owner_id, &ticket) {
            let _ = ops::add_note(
                &store,
                &id,
                hotsheet_ticketing::mint_ulid(tx.clock, tx.rng),
                timestamp(tx),
                NoteKind::Activity,
                format!(
                    "Readiness override: plan step `{}` was `{level}` and is dispatched \
                     anyway under operation `{operation_id}`. Approval `{}` by `{}`.",
                    step.step_id,
                    approval
                        .as_ref()
                        .map_or("unknown", |approval| approval.approval_id.as_str()),
                    approval
                        .as_ref()
                        .and_then(|approval| approval.decided_by.as_deref())
                        .unwrap_or("unknown"),
                ),
            );
        }
        Ok(())
    }
}
