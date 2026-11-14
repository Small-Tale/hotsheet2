//! HS2-JSF985: provisional work across orchestration targets.
//!
//! A step at the `provisional` readiness level (§26.15) may be worked ahead of its
//! prerequisites' pending gates when the orchestration policy and every involved
//! target's own policy opt in. The decision is the pure rule in
//! `hotsheet_ticketing::orchestration::provisional_standings`, which applies the
//! single-project rule of HS2-2SJPCH. This module gathers what that rule reads, admits
//! a dispatch, and keeps the ledger of work that was started that way.
//!
//! The ledger is the single-project one, `SpeculationLedger`, with its own storage
//! (`orchestration-provisional.json`): `record`, `apply`, `rebase`, `reopen` and the
//! owed-note bookkeeping are reused as they are. Gate results reach it where every
//! gate change is published and where interrupted runs are reconciled; nothing here
//! runs on a timer or contacts a provider.
use std::collections::{BTreeMap, BTreeSet};

use hotsheet_model::NoteKind;
use hotsheet_ticketing::TicketRef;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::ops;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, AuditRecord, DenialReason, Gate, GateFacts, GateFilter, GateKind, GateRegistry,
    GateState, PlanFacts, PlanStep, PolicyBody, Prerequisite, ProvisionalBasis,
    ProvisionalStanding, ProvisionalStore, ReadinessError, ReadinessLevel, RunEvent,
    RunEventJournal, RunEventKind, RunEventSource, RunTarget, RunTicket, SpeculationInputs,
    StepReadiness, TargetAction, TargetSpeculation, TicketKey, gate_observation,
    provisional_standings, summarize, ticket_key,
};
use hotsheet_ticketing::provisional::{GateObservation, GateResult, IntegrationRule};
use serde::{Deserialize, Serialize};

use super::tickets::timestamp;
use super::*;
use crate::coordination::speculation::{
    PendingGate, Rebase, Recovery, SpeculationEventKind, SpeculationLedger, SpeculationState,
    SpeculativeRecord,
};

const MAX_REASON_BYTES: usize = 64;

/// What the ledger cannot hold about one record: which step and targets it is about.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkRecord {
    step_id: String,
    ticket: RunTicket,
    target: RunTarget,
    operation_id: String,
    recorded_at_ms: u64,
    bases: Vec<ProvisionalBasis>,
    /// HS2-2K0DM8: the fix commit the working session was last told to rebase onto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    told_rebase: Option<String>,
}

/// A target's own speculation setting as its machine last reported it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetSeen {
    #[serde(default)]
    speculation: Option<TargetSpeculation>,
    at_ms: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(default)]
    ledger: SpeculationLedger,
    /// By the ledger's assignment key.
    #[serde(default)]
    work: BTreeMap<String, WorkRecord>,
    #[serde(default)]
    targets: BTreeMap<String, TargetSeen>,
}

/// HS2-J3SMMT: the ledger keeps at most `MAX_LEDGER_RECORDS`, forgetting the oldest
/// confirmed and reopened work first and never provisional or at-risk work, as the
/// gate registry forgets settled gates. A forgotten record's work entry goes with it.
impl hotsheet_ticketing::orchestration::Retiring for Document {
    fn retire(&mut self) -> bool {
        let retired = self
            .ledger
            .retire_settled(crate::coordination::speculation::MAX_LEDGER_RECORDS);
        for key in &retired {
            self.work.remove(key);
        }
        !retired.is_empty()
    }
}

/// One gate provisional work rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProvisionalGate {
    /// The prerequisite ticket the gate belongs to.
    pub target_id: String,
    pub connection_id: String,
    pub native_id: String,
    pub commit: String,
    /// The standing CI gates of that commit when the work was recorded or rebased.
    pub gate_ids: Vec<String>,
    pub result: GateResult,
    /// The gate, or the provider run of a verdict. Never output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

/// One piece of work that was started ahead of a pending gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProvisionalWork {
    pub assignment: String,
    pub step_id: String,
    /// The orchestration ticket the work is for.
    pub ticket: RunTicket,
    /// The target ticket being worked.
    pub target: RunTarget,
    pub operation_id: String,
    pub state: SpeculationState,
    pub depth: u32,
    /// `provisional on <ticket> CI <gate>` for each undecided gate, or the state.
    pub explain: String,
    pub gates: Vec<ProvisionalGate>,
    /// How at-risk work recovers once the failing prerequisite is repaired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Recovery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reopen_reason: Option<String>,
    pub recorded_at_ms: u64,
    /// HS2-ERYQDV: the working target project's own integration rule when the work
    /// was recorded.
    pub integration: IntegrationRule,
    /// The work must stay off the target's shipping branch now: its rule is `parked`
    /// and a gate is not green yet, it is at risk, or it was reopened. The target's own
    /// workflow integrates it; this machine reports the rule and does not drive it.
    pub held: bool,
}

/// A prerequisite whose gate failed under provisional work: its repair comes first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepairFirst {
    pub target_id: String,
    pub connection_id: String,
    pub native_id: String,
    pub commit: String,
    /// The steps whose provisional work stands on the failed gate.
    pub stacked: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProvisionalView {
    /// False while the orchestration policy does not opt in.
    pub enabled: bool,
    /// Unconfirmed work first.
    pub work: Vec<ProvisionalWork>,
    pub repair_first: Vec<RepairFirst>,
    /// Gates that were superseded, cancelled or lost under provisional work.
    pub reevaluate: Vec<ProvisionalGate>,
}

/// `readiness reopen`: the fix of a failed prerequisite invalidated this work.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReopenProvisional {
    pub step_id: String,
    /// A stable code, never free text.
    pub reason: String,
}

/// Per step id: the rule's answer for a provisional step, and the recorded work.
pub(super) type Annotations = (
    BTreeMap<String, ProvisionalStanding>,
    BTreeMap<String, ProvisionalWork>,
);

struct Audited<'a> {
    action: &'a str,
    outcome: AuditOutcome,
    reason: &'a str,
    operation_id: Option<&'a str>,
}

/// One state change to publish.
struct Change {
    step_id: String,
    reason: &'static str,
    gate_id: Option<String>,
    dedupe: String,
}

fn seconds(now_ms: u64) -> i64 {
    i64::try_from(now_ms / 1000).unwrap_or(i64::MAX)
}

fn refused() -> OrchestrationError {
    OrchestrationError::Invalid("the provisional ledger refused the change")
}

fn view_gates(record: &SpeculativeRecord, work: &WorkRecord) -> Vec<ProvisionalGate> {
    record
        .gates
        .iter()
        .map(|gate| {
            let basis = work.bases.iter().find(|basis| basis.commit == gate.commit);
            ProvisionalGate {
                target_id: basis.map(|b| b.target_id.clone()).unwrap_or_default(),
                connection_id: basis.map(|b| b.connection_id.clone()).unwrap_or_default(),
                native_id: gate.blocker.clone(),
                commit: gate.commit.clone(),
                gate_ids: basis.map(|b| b.gate_ids.clone()).unwrap_or_default(),
                result: gate.result,
                run: gate.run.clone(),
            }
        })
        .collect()
}

fn view_work(record: &SpeculativeRecord, work: &WorkRecord) -> ProvisionalWork {
    ProvisionalWork {
        assignment: record.assignment.clone(),
        step_id: work.step_id.clone(),
        ticket: work.ticket.clone(),
        target: work.target.clone(),
        operation_id: work.operation_id.clone(),
        state: record.state,
        depth: record.depth,
        explain: record.explain(),
        gates: view_gates(record, work),
        recovery: (record.state == SpeculationState::AtRisk)
            .then(|| record.risks.last().map(|risk| risk.recovery))
            .flatten(),
        reopen_reason: record.reopen_reason.clone(),
        recorded_at_ms: work.recorded_at_ms,
        integration: record.integration,
        held: match record.state {
            SpeculationState::Confirmed => false,
            SpeculationState::AtRisk | SpeculationState::Reopened => true,
            SpeculationState::Provisional => record.integration == IntegrationRule::Parked,
        },
    }
}

fn live(state: SpeculationState) -> bool {
    matches!(
        state,
        SpeculationState::Provisional | SpeculationState::AtRisk
    )
}

/// The newest record of a step: the live one when there is one.
fn of_step<'a>(
    document: &'a Document,
    step_id: &str,
) -> Option<(&'a SpeculativeRecord, &'a WorkRecord)> {
    document
        .work
        .iter()
        .filter(|(_, work)| work.step_id == step_id)
        .filter_map(|(assignment, work)| Some((document.ledger.records.get(assignment)?, work)))
        .max_by_key(|(record, work)| (live(record.state), work.recorded_at_ms))
}

/// The provider run a verdict rests on, as an identifier the ledger accepts; the
/// gate's own id otherwise.
fn run_name(gates: &[Gate], state: GateState) -> Option<String> {
    let gate = gates
        .iter()
        .find(|gate| gate.state == state)
        .or(gates.first())?;
    let named = gate
        .evidence
        .as_ref()
        .filter(|_| matches!(state, GateState::Failed | GateState::Passed))
        .map(|evidence| format!("{}.{}", evidence.reference, evidence.attempt))
        .filter(|name| {
            name.len() <= 128
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
        });
    Some(named.unwrap_or_else(|| gate.gate_id.clone()))
}

/// One transition of one record as an event; `dedupe` is the record's own history
/// length, so the same transition is published once however often it is found.
fn event(
    ledger: &SpeculationLedger,
    work: &BTreeMap<String, WorkRecord>,
    assignment: &str,
    reason: &'static str,
    gate_id: Option<String>,
) -> Option<Change> {
    let record = ledger.records.get(assignment)?;
    Some(Change {
        step_id: work.get(assignment)?.step_id.clone(),
        reason,
        gate_id,
        dedupe: format!("provisional-{assignment}-{}", record.events.len()),
    })
}

impl OrchestrationRuntime {
    fn provisional_store(&self) -> ProvisionalStore {
        ProvisionalStore::new(&self.home)
    }

    /// The policy, when it opts in to provisional work anywhere. `None` is the
    /// default, under which nothing in this module is read, computed or written. A
    /// policy that cannot be read opts in to nothing: reads that never depended on
    /// the policy keep answering, and the gate denies every action anyway (§26.8).
    pub(super) fn speculating(&self, owner_id: &str) -> Option<PolicyBody> {
        self.policy(owner_id)
            .ok()
            .and_then(|view| view.policy)
            .filter(PolicyBody::speculates)
    }

    /// The tag whose presence on a target ticket opts it out, while the policy opts
    /// in to provisional work. `None` otherwise: tags are then not read at all.
    pub(super) fn opt_out_tag(&self, owner_id: &str) -> Option<String> {
        self.speculating(owner_id)
            .map(|policy| policy.speculation.opt_out_tag)
    }

    fn provisional_audit(
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
                reason: Some(entry.reason.into()),
                permission_id: None,
            },
        )
    }

    fn provisional_publish(&self, owner_id: &str, changes: Vec<Change>, now_ms: u64) {
        if changes.is_empty() {
            return;
        }
        let Ok(journal) = RunEventJournal::new(&self.home, owner_id) else {
            return;
        };
        for change in changes {
            let _ = journal.record(RunEvent {
                at_ms: now_ms,
                // Like a readiness event: the step may have no run yet.
                run_id: None,
                step_id: Some(change.step_id),
                kind: RunEventKind::Provisional,
                source: RunEventSource::Orchestrator,
                state: None,
                reason: Some(change.reason.into()),
                actor_id: None,
                dedupe: Some(change.dedupe),
                gate_id: change.gate_id,
                gate_state: None,
            });
        }
    }

    fn provisional_note(
        &self,
        owner_id: &str,
        ticket: &RunTicket,
        text: String,
        tx: &mut Transaction<'_>,
    ) -> bool {
        let ticket = OrchestrationTicket {
            connection_id: ticket.connection_id.clone(),
            native_id: ticket.native_id.clone(),
        };
        let Ok((store, id, _)) = self.orchestration_store(owner_id, &ticket) else {
            return false;
        };
        ops::add_note(
            &store,
            &id,
            hotsheet_ticketing::mint_ulid(tx.clock, tx.rng),
            timestamp(tx),
            NoteKind::Activity,
            text,
        )
        .is_ok()
    }

    /// HS2-2K0DM8: reopened work whose run is still active is a judgment call: whether
    /// to stop that run, keep it or hand it off is a person's decision, so the
    /// orchestrator asks on the orchestration ticket with a `FEEDBACK NEEDED` note and
    /// acts on nothing. The answer is given with the run's own controls; once the run
    /// is cancelled and the prerequisites are confirmed, the step is dispatched again
    /// from confirmed state, as any ready step is.
    fn provisional_reopen_decision(
        &self,
        owner_id: &str,
        ticket: &RunTicket,
        work: &ProvisionalWork,
        tx: &mut Transaction<'_>,
    ) {
        let Some(run) = self
            .run_list(owner_id, false)
            .unwrap_or_default()
            .into_iter()
            .find(|run| run.operation_id == work.operation_id && !run.state.is_terminal())
        else {
            return;
        };
        let orchestration = OrchestrationTicket {
            connection_id: ticket.connection_id.clone(),
            native_id: ticket.native_id.clone(),
        };
        let Ok((store, id, _)) = self.orchestration_store(owner_id, &orchestration) else {
            return;
        };
        let text = format!(
            "FEEDBACK NEEDED: provisional work of plan step `{step}` (operation `{operation}`) \
             was reopened, and its run `{run}` on target `{target}` is still active.\n\n\
             CHOICE: What should happen to that run?\n\
             - **Cancel it and redo the step (recommended).** `hotsheet-cli orchestration run \
             cancel {run} --project {owner_id}`. Once its prerequisites are confirmed, \
             dispatch the step again (`run dispatch`); that start is a new record on \
             confirmed state.\n\
             - **Hand it off.** `hotsheet-cli orchestration run handoff {run} --project \
             {owner_id}`: a person takes the work over on the target.\n\
             - **Keep it.** Nothing more is done; the work continues on the reopened base.",
            step = work.step_id,
            operation = work.operation_id,
            run = run.run_id,
            target = run.target.target_id,
        );
        let _ = ops::add_note(
            &store,
            &id,
            hotsheet_ticketing::mint_ulid(tx.clock, tx.rng),
            timestamp(tx),
            NoteKind::Regular,
            text,
        );
    }

    /// HS2-2K0DM8: tell the session working at-risk work, still on its branch, that its
    /// failed base gate was repaired: rebase onto the fix. Each fix commit is told once
    /// per piece of work; a tell that cannot be delivered is tried again by the next
    /// reconciliation.
    fn provisional_tell_rebase(&self, owner_id: &str, tx: &mut Transaction<'_>) {
        let Ok(Some(document)) = self.provisional_store().read::<Document>(owner_id) else {
            return;
        };
        let owed: Vec<(String, String, String)> = document
            .work
            .iter()
            .filter_map(|(assignment, work)| {
                let record = document.ledger.records.get(assignment)?;
                if !live(record.state) {
                    return None;
                }
                let fix = record
                    .events
                    .iter()
                    .rev()
                    .find(|event| event.kind == SpeculationEventKind::Rebased)?
                    .gate_commit
                    .clone()?;
                // Work already on its target's shipping branch is re-verified by the
                // next covering run; only work on its branch is rebased.
                let on_branch = record
                    .risks
                    .last()
                    .is_none_or(|risk| risk.recovery == Recovery::RebaseOntoFix);
                (on_branch && work.told_rebase.as_deref() != Some(fix.as_str()))
                    .then(|| (assignment.clone(), work.operation_id.clone(), fix))
            })
            .collect();
        if owed.is_empty() {
            return;
        }
        let runs = self.run_list(owner_id, false).unwrap_or_default();
        let sessions = self.session_list(owner_id, true).unwrap_or_default();
        for (assignment, operation_id, fix) in owed {
            let Some(run) = runs
                .iter()
                .find(|run| run.operation_id == operation_id && !run.state.is_terminal())
            else {
                continue;
            };
            let Some(session) = sessions.iter().find(|session| {
                session.run_id.as_deref() == Some(run.run_id.as_str())
                    && session.state == hotsheet_ticketing::orchestration::SessionState::Running
                    && session.owner != hotsheet_ticketing::orchestration::SessionOwner::Human
            }) else {
                continue;
            };
            let actor = super::watch::supervisor(run);
            if self
                .session_tell_rebase(owner_id, &session.session_id, &fix, actor.as_ref(), tx)
                .is_ok()
            {
                let _ = self
                    .provisional_store()
                    .update::<Document, _>(owner_id, |document| {
                        let changed = document.work.get_mut(&assignment).is_some_and(|work| {
                            let fresh = work.told_rebase.as_deref() != Some(fix.as_str());
                            work.told_rebase = Some(fix.clone());
                            fresh
                        });
                        Ok(((), changed))
                    });
            }
        }
    }

    /// HS2-ERYQDV: whether a step's live provisional work is under a `parked` rule.
    fn provisional_parked(&self, owner_id: &str, step_id: &str) -> bool {
        self.provisional_store()
            .read::<Document>(owner_id)
            .ok()
            .flatten()
            .and_then(|document| of_step(&document, step_id).map(|(record, _)| record.integration))
            == Some(IntegrationRule::Parked)
    }

    // ---- What the rule reads ----

    /// Record what each target's machine said of its own speculation setting. True
    /// when a stored setting changed.
    pub(super) fn provisional_targets_seen(
        &self,
        owner_id: &str,
        seen: &[(String, Option<TargetSpeculation>)],
        now_ms: u64,
    ) -> Result<bool, OrchestrationError> {
        if seen.is_empty() {
            return Ok(false);
        }
        self.provisional_store()
            .update::<Document, _>(owner_id, |document| {
                let mut changed = false;
                for (target_id, speculation) in seen {
                    let known = document.targets.get(target_id);
                    if known.is_none_or(|known| known.speculation != *speculation) {
                        document.targets.insert(
                            target_id.clone(),
                            TargetSeen {
                                speculation: *speculation,
                                at_ms: now_ms,
                            },
                        );
                        changed = true;
                    }
                }
                Ok((changed, changed))
            })
    }

    /// Ask each target's machine for its project's own speculation setting, through
    /// the gate as `gates_observe`. A target that answers without one has not opted
    /// in; a target that cannot be read is recorded as unknown. The rule refuses
    /// both, each with its own code.
    pub(super) fn provisional_targets_read(
        &self,
        owner_id: &str,
        target_ids: &BTreeSet<String>,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        let seen: Vec<(String, Option<TargetSpeculation>)> = target_ids
            .iter()
            .map(|target_id| {
                let speculation = Self::target_ref(owner_id, target_id)
                    .and_then(|target| self.target_speculation(&target, actor, tx))
                    .ok();
                (target_id.clone(), speculation)
            })
            .collect();
        self.provisional_targets_seen(owner_id, &seen, tx.clock.now_ms())
            .map(|_| ())
    }

    /// HS2-FJ6MDW: a target project's own speculation setting, read alone through the
    /// gate as `gates_observe`: no capacity sample is taken. An older host without the
    /// read answers from its capacity snapshot, as before.
    pub fn target_speculation(
        &self,
        target: &TargetRef,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetSpeculation, OrchestrationError> {
        let authorized = self.authorize(
            target,
            actor,
            ActionRequest::new(TargetAction::GatesObserve),
            tx,
        )?;
        let outcome = Self::destination(&authorized).and_then(|destination| {
            serde_json::from_value::<super::watch::HostedSpeculation>(self.hosted_call_at(
                destination,
                "/target-speculation",
                "GET",
                "",
                None,
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        });
        self.finish(target, &authorized, &outcome, tx)?;
        match outcome {
            Ok(hosted) => Ok(hosted.setting()),
            Err(OrchestrationError::TargetOperation(
                hotsheet_ticketing::orchestration::TargetOpError::PeerUnsupported,
            )) => self
                .target_capacity(target, &[], actor, tx)
                .map(|snapshot| {
                    snapshot
                        .speculation
                        .unwrap_or(TargetSpeculation::NOT_OPTED_IN)
                }),
            Err(error) => Err(error),
        }
    }

    /// The targets a step's provisional standing depends on: its own and those of its
    /// ticket prerequisites.
    pub(super) fn provisional_targets_of(step: &PlanStep) -> BTreeSet<String> {
        step.prerequisites
            .iter()
            .filter_map(Prerequisite::ticket_key)
            .map(|key| key.0)
            .chain([step.target.target_id.clone()])
            .collect()
    }

    /// The rule's answer per provisional step and the recorded work per step, for the
    /// readiness views. Both are empty while the policy does not opt in and nothing
    /// was ever recorded.
    pub(super) fn provisional_annotations(
        &self,
        owner_id: &str,
        answers: &[StepReadiness],
        facts: &PlanFacts,
    ) -> Result<Annotations, OrchestrationError> {
        let policy = self.speculating(owner_id);
        let document = self.provisional_store().read::<Document>(owner_id)?;
        let standings = match &policy {
            None => BTreeMap::new(),
            Some(policy) => {
                let targets: BTreeMap<String, TargetSpeculation> = document
                    .iter()
                    .flat_map(|document| &document.targets)
                    .filter_map(|(id, seen)| Some((id.clone(), seen.speculation?)))
                    .collect();
                provisional_standings(
                    answers,
                    facts,
                    &SpeculationInputs {
                        policy,
                        targets: &targets,
                    },
                )
            }
        };
        let mut work = BTreeMap::new();
        if let Some(document) = &document {
            for answer in answers {
                if let Some((record, recorded)) = of_step(document, &answer.step.step_id) {
                    work.insert(answer.step.step_id.clone(), view_work(record, recorded));
                }
            }
        }
        Ok((standings, work))
    }

    // ---- Admitting and recording ----

    /// Record that a step's work was started ahead of the gates in `standing`. A step
    /// with live provisional work keeps that record; nothing is recorded for a step
    /// that rests on no pending gate.
    pub(super) fn provisional_record(
        &self,
        owner_id: &str,
        step: &PlanStep,
        standing: &ProvisionalStanding,
        operation_id: &str,
        actor_id: &str,
        now_ms: u64,
    ) -> Result<Option<String>, OrchestrationError> {
        let Some(first) = standing.bases.first() else {
            return Ok(None);
        };
        let (assignment, created) =
            self.provisional_store()
                .update::<Document, _>(owner_id, |document| {
                    if let Some((record, _)) = of_step(document, &step.step_id)
                        && live(record.state)
                    {
                        return Ok(((record.assignment.clone(), false), false));
                    }
                    let earlier = document
                        .work
                        .values()
                        .filter(|work| work.step_id == step.step_id)
                        .count();
                    let assignment = format!("{}-{}", step.step_id, earlier + 1);
                    // HS2-ERYQDV: the working project's own rule, as its machine last
                    // reported it.
                    let integration = document
                        .targets
                        .get(&step.target.target_id)
                        .and_then(|seen| seen.speculation)
                        .map(|setting| setting.integration)
                        .unwrap_or_default();
                    document
                        .ledger
                        .record(
                            SpeculativeRecord {
                                assignment: assignment.clone(),
                                ticket: TicketRef {
                                    connection_id: step.target.connection_id.clone(),
                                    native_id: step.target.native_id.clone(),
                                },
                                slug: step.step_id.clone(),
                                worker: actor_id.into(),
                                // Across targets the base is the prerequisite's
                                // integrated commit the work assumes, not a branch point.
                                base: first.commit.clone(),
                                depth: standing.depth,
                                integration,
                                gates: standing
                                    .bases
                                    .iter()
                                    .map(|basis| PendingGate {
                                        blocker: basis.native_id.clone(),
                                        slug: basis.native_id.clone(),
                                        commit: basis.commit.clone(),
                                        result: basis.gate,
                                        run: basis.gate_ids.first().cloned(),
                                    })
                                    .collect(),
                                state: SpeculationState::Provisional,
                                risks: Vec::new(),
                                reopen_reason: None,
                                events: Vec::new(),
                            },
                            seconds(now_ms),
                        )
                        .map_err(|_| refused())?;
                    document.work.insert(
                        assignment.clone(),
                        WorkRecord {
                            step_id: step.step_id.clone(),
                            ticket: step.ticket.clone(),
                            target: step.target.clone(),
                            operation_id: operation_id.into(),
                            recorded_at_ms: now_ms,
                            bases: standing.bases.clone(),
                            told_rebase: None,
                        },
                    );
                    Ok(((assignment, true), true))
                })?;
        if created {
            self.provisional_publish(
                owner_id,
                vec![Change {
                    step_id: step.step_id.clone(),
                    reason: "provisional",
                    gate_id: first.gate_ids.first().cloned(),
                    dedupe: format!("provisional-{assignment}-1"),
                }],
                now_ms,
            );
        }
        Ok(Some(assignment))
    }

    /// The provisional half of the dispatch guard. Without the policy's opt-in for
    /// the step's target this is exactly the refusal it always was. With it, the
    /// involved targets' own settings are read fresh, the rule decides, a person is
    /// asked through a `provisional_dispatch` permission request unless it is their
    /// own act or a remembered rule allows it (HS2-XHQ1EH), and admitted work is
    /// recorded with the gates it rests on. `Ok(Some(id))` names the Allow Once
    /// answer to use once the run is recorded.
    pub(super) fn provisional_admit(
        &self,
        owner_id: &str,
        step: &PlanStep,
        operation_id: &str,
        authorized: &Authorized,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<String>, OrchestrationError> {
        let target_id = step.target.target_id.clone();
        if !self
            .speculating(owner_id)
            .is_some_and(|policy| policy.speculation_allowed(&target_id))
        {
            return Err(OrchestrationError::Readiness(ReadinessError::Provisional));
        }
        self.provisional_targets_read(owner_id, &Self::provisional_targets_of(step), actor, tx)?;
        // The tickets the step names are read again before anything is built on them:
        // their tags decide the opt-out (a plan defined before the opt-in never read
        // them), and a prerequisite that moved on since is judged as it stands now.
        self.readiness_refresh(
            owner_id,
            &RefreshReadiness {
                step_id: Some(step.step_id.clone()),
            },
            actor,
            tx,
        )?;
        let view = self.readiness(owner_id, &step.step_id, tx)?;
        if view.readiness.level != ReadinessLevel::Provisional {
            return match view.readiness.level.refusal() {
                None => Ok(None),
                Some(refusal) => Err(OrchestrationError::Readiness(refusal)),
            };
        }
        let standing = view
            .speculation
            .ok_or(OrchestrationError::Readiness(ReadinessError::Provisional))?;
        let audit = |outcome, reason: &str, tx: &mut Transaction<'_>| {
            self.provisional_audit(
                owner_id,
                &target_id,
                actor,
                Audited {
                    action: "provisional_dispatch",
                    outcome,
                    reason,
                    operation_id: Some(operation_id),
                },
                tx,
            )
        };
        if !standing.allowed {
            audit(
                AuditOutcome::Denied,
                standing.refusal.as_deref().unwrap_or("speculation_refused"),
                tx,
            )?;
            return Err(OrchestrationError::Readiness(
                ReadinessError::ProvisionalRefused,
            ));
        }
        let reference = Self::target_ref(owner_id, &target_id)?;
        let permission = match self.permission_gate(
            &reference,
            actor,
            &ActionRequest {
                provisional: true,
                ..ActionRequest::new(TargetAction::WorkStart).operation(operation_id)
            },
            tx,
        ) {
            Ok(permission) => permission,
            Err(OrchestrationError::Denied(DenialReason::PermissionDenied)) => {
                audit(AuditOutcome::Denied, "permission_denied", tx)?;
                return Err(OrchestrationError::Readiness(
                    ReadinessError::ProvisionalRefused,
                ));
            }
            Err(error) => {
                audit(AuditOutcome::Denied, error.code(), tx)?;
                return Err(error);
            }
        };
        audit(AuditOutcome::Allowed, "provisional", tx)?;
        let recorded = self.provisional_record(
            owner_id,
            step,
            &standing,
            operation_id,
            &authorized.actor_id,
            tx.clock.now_ms(),
        )?;
        if recorded.is_some() {
            // Best effort, like a run's own note; the audit entry above is the record.
            let gates: Vec<String> = standing
                .bases
                .iter()
                .map(|basis| {
                    format!(
                        "gate `{}` of commit `{}`",
                        basis.gate_ids.first().map_or("unknown", String::as_str),
                        basis.commit
                    )
                })
                .collect();
            self.provisional_note(
                owner_id,
                &step.ticket,
                format!(
                    "Provisional dispatch: plan step `{}` is dispatched under operation \
                     `{operation_id}` ahead of {} (depth {} of {}).{}",
                    step.step_id,
                    gates.join(", "),
                    standing.depth,
                    standing.max_depth,
                    // HS2-ERYQDV: the working project's own rule travels with the work.
                    if self.provisional_parked(owner_id, &step.step_id) {
                        " Integration is parked: keep the work off the target's shipping \
                         branch until its base gates are green."
                    } else {
                        ""
                    },
                ),
                tx,
            );
        }
        Ok(permission)
    }

    // ---- Gate results ----

    /// Bring the ledger in line with this machine's gate records: a green gate
    /// confirms the work on it, a red one marks it at risk and names the run, a gate
    /// replaced by a newer link rebases it onto the replacement, and a superseded,
    /// cancelled or lost gate is listed for re-evaluation. Every change is one
    /// `provisional` run event and every at-risk mark one note on the orchestration
    /// ticket. Applying the same records again changes nothing, so it runs wherever a
    /// gate change is published and wherever an interruption is reconciled, and a
    /// result that arrived while the orchestrator was down is applied exactly once.
    /// Returns how many records changed.
    pub fn provisional_reconcile(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<usize, OrchestrationError> {
        let count = self.provisional_apply(owner_id, tx.clock.now_ms())?;
        self.provisional_notes(owner_id, tx)?;
        // HS2-2K0DM8: sessions on at-risk work learn of the fix.
        self.provisional_tell_rebase(owner_id, tx);
        Ok(count)
    }

    /// The ledger half of [`Self::provisional_reconcile`], for the one place a gate
    /// change is published, which has no transaction to write a note with. A note it
    /// leaves owed is written by the operation that caused the change.
    pub(super) fn provisional_gate_changed(&self, owner_id: &str, now_ms: u64) {
        let _ = self.provisional_apply(owner_id, now_ms);
    }

    fn provisional_apply(&self, owner_id: &str, now_ms: u64) -> Result<usize, OrchestrationError> {
        let store = self.provisional_store();
        let Some(document) = store.read::<Document>(owner_id)? else {
            return Ok(0);
        };
        if document.ledger.watched_commits().is_empty() {
            return Ok(0);
        }
        let gates: Vec<Gate> = GateRegistry::new(&self.home)
            .list(owner_id, &GateFilter::default())?
            .into_iter()
            .filter(|gate| gate.kind == GateKind::Ci)
            .collect();
        // Work whose own run already links a CI gate has reached its target's
        // shipping branch: the next covering run re-verifies it.
        let runs = self.runs().list(owner_id, false)?;
        let landed: BTreeSet<TicketKey> = runs
            .iter()
            .filter(|run| {
                gates
                    .iter()
                    .any(|gate| gate.run_id == run.run_id && gate.state != GateState::Superseded)
            })
            .map(|run| ticket_key(&run.target))
            .collect();
        let changes = store.update::<Document, _>(owner_id, |document| {
            let mut changes: Vec<Change> = Vec::new();
            let Document { ledger, work, .. } = document;
            for commit in ledger.watched_commits() {
                let Some(basis) = work
                    .values()
                    .flat_map(|work| &work.bases)
                    .find(|basis| basis.commit == commit)
                    .cloned()
                else {
                    continue;
                };
                let key: TicketKey = (
                    basis.target_id.clone(),
                    basis.connection_id.clone(),
                    basis.native_id.clone(),
                );
                let standing_of = |keep: &dyn Fn(&Gate) -> bool| -> Vec<Gate> {
                    gates
                        .iter()
                        .filter(|gate| gate.state != GateState::Superseded && keep(gate))
                        .cloned()
                        .collect()
                };
                // What §26.15 reads for this prerequisite now: the standing gates of
                // the latest run on its ticket, unless the prerequisite pins a commit.
                let latest = (!basis.pinned)
                    .then(|| {
                        runs.iter()
                            .filter(|run| ticket_key(&run.target) == key)
                            .max_by_key(|run| (run.created_at_ms, &run.run_id))
                    })
                    .flatten();
                let current = latest.map_or_else(Vec::new, |run| {
                    standing_of(&|gate| gate.run_id == run.run_id)
                });
                let mut commits: Vec<&String> = current.iter().map(|gate| &gate.commit).collect();
                commits.sort();
                commits.dedup();
                if let [next] = commits.as_slice()
                    && **next != commit
                {
                    // The prerequisite's gate now stands for another commit: the fix.
                    let next = (*next).clone();
                    let summary = summarize(current.clone());
                    let (_, mut reading) = gate_observation(&GateFacts::from(&summary));
                    reading.run = run_name(&current, summary.ci);
                    let dependents: Vec<String> = ledger
                        .records
                        .values()
                        .filter(|record| {
                            live(record.state)
                                && record.gates.iter().any(|gate| {
                                    gate.commit == commit && gate.result != GateResult::Green
                                })
                        })
                        .map(|record| record.assignment.clone())
                        .collect();
                    for assignment in dependents {
                        let done = work
                            .get(&assignment)
                            .is_some_and(|recorded| landed.contains(&ticket_key(&recorded.target)));
                        // A replacement that is itself red is repaired first; the
                        // work stays at risk on the gate it knows.
                        let Ok(record) = ledger.rebase(
                            &assignment,
                            &Rebase {
                                replaces: commit.clone(),
                                gate_commit: next.clone(),
                                gate: reading.clone(),
                                base: Some(next.clone()),
                            },
                            done,
                            seconds(now_ms),
                            None,
                        ) else {
                            continue;
                        };
                        if let Some(recorded) = work.get_mut(&assignment) {
                            for basis in &mut recorded.bases {
                                if basis.commit == commit {
                                    basis.commit = next.clone();
                                    basis.gate_ids =
                                        current.iter().map(|gate| gate.gate_id.clone()).collect();
                                    basis.gate = reading.result;
                                }
                            }
                        }
                        changes.extend(event(
                            ledger,
                            work,
                            &assignment,
                            if record.state == SpeculationState::Confirmed {
                                "confirmed"
                            } else {
                                "rebased"
                            },
                            current.first().map(|gate| gate.gate_id.clone()),
                        ));
                    }
                    continue;
                }
                let on_commit = |gate: &Gate| gate.target_id == key.0 && gate.commit == commit;
                let standing = standing_of(&on_commit);
                let (reading, reason_for, gate_id) = if standing.is_empty() {
                    let Some(old) = gates.iter().find(|gate| on_commit(gate)) else {
                        // This machine holds no gate for the commit: nothing is known.
                        continue;
                    };
                    // Superseded with nothing in its place: the branch moved.
                    (
                        GateObservation::of(GateResult::Superseded),
                        None,
                        Some(old.gate_id.clone()),
                    )
                } else {
                    let summary = summarize(standing.clone());
                    let (_, mut reading) = gate_observation(&GateFacts::from(&summary));
                    reading.run = run_name(&standing, summary.ci);
                    (
                        reading,
                        Some(()),
                        standing.first().map(|gate| gate.gate_id.clone()),
                    )
                };
                let integrated = |assignment: &str| {
                    work.get(assignment)
                        .is_some_and(|recorded| landed.contains(&ticket_key(&recorded.target)))
                };
                for change in ledger
                    .apply(&commit, &reading, seconds(now_ms), &integrated)
                    .map_err(|_| refused())?
                {
                    let reason = match (reason_for, change.to, change.result) {
                        (Some(()), SpeculationState::Confirmed, _) => "confirmed",
                        (Some(()), SpeculationState::AtRisk, _) => "at_risk",
                        (Some(()), _, GateResult::Pending) => "provisional",
                        _ => "reevaluate",
                    };
                    changes.extend(event(
                        ledger,
                        work,
                        &change.assignment,
                        reason,
                        gate_id.clone(),
                    ));
                }
            }
            let changed = !changes.is_empty();
            Ok((changes, changed))
        })?;
        let count = changes.len();
        self.provisional_publish(owner_id, changes, now_ms);
        Ok(count)
    }

    /// Write the at-risk notes the ledger still owes, each once. The ledger's lock is
    /// held from finding a note owed to marking it written, so two reconciliations at
    /// once (a gate follower and a request) write it once. A note that could not be
    /// written stays owed and is written by the next reconciliation.
    fn provisional_notes(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        let store = self.provisional_store();
        if store
            .read::<Document>(owner_id)?
            .is_none_or(|document| document.ledger.owed_notes().is_empty())
        {
            return Ok(());
        }
        store.update::<Document, _>(owner_id, |document| {
            let mut changed = false;
            for owed in document.ledger.owed_notes() {
                let Some(work) = document.work.get(&owed.assignment) else {
                    continue;
                };
                let basis = work
                    .bases
                    .iter()
                    .find(|basis| basis.commit == owed.gate_commit);
                let text = format!(
                    "Provisional work at risk: plan step `{}` (operation `{}`) rests on commit \
                     `{}` of target ticket `{}` in target `{}`, whose CI run `{}` failed (gate \
                     `{}`). Recovery: {}.",
                    work.step_id,
                    work.operation_id,
                    owed.gate_commit,
                    owed.blocker,
                    basis.map_or("unknown", |basis| basis.target_id.as_str()),
                    owed.run,
                    basis
                        .and_then(|basis| basis.gate_ids.first())
                        .map_or("unknown", String::as_str),
                    match owed.recovery {
                        Recovery::RebaseOntoFix => "rebase onto the fix and rerun its gates",
                        Recovery::ReverifyByCoveringRun => {
                            "the next covering CI run of its own target re-verifies it"
                        }
                    },
                );
                let ticket = work.ticket.clone();
                if self.provisional_note(owner_id, &ticket, text, tx) {
                    document
                        .ledger
                        .noted(&owed.assignment, &owed.gate_commit, &owed.run);
                    changed = true;
                }
            }
            Ok(((), changed))
        })
    }

    /// Best effort, for the places a consequence must not fail its cause.
    pub(super) fn provisional_follow(&self, owner_id: &str, tx: &mut Transaction<'_>) {
        let _ = self.provisional_reconcile(owner_id, tx);
    }

    // ---- Views ----

    /// At-risk provisional work per failed prerequisite ticket: the steps stacked on
    /// each. The scheduler raises that ticket's repair by them (§26.17).
    pub(super) fn provisional_stacked(
        &self,
        owner_id: &str,
    ) -> Result<BTreeMap<TicketKey, BTreeSet<String>>, OrchestrationError> {
        let mut stacked: BTreeMap<TicketKey, BTreeSet<String>> = BTreeMap::new();
        let Some(document) = self.provisional_store().read::<Document>(owner_id)? else {
            return Ok(stacked);
        };
        for (assignment, record) in &document.ledger.records {
            let Some(work) = document.work.get(assignment) else {
                continue;
            };
            if record.state != SpeculationState::AtRisk {
                continue;
            }
            for gate in view_gates(record, work) {
                if gate.result == GateResult::Red {
                    stacked
                        .entry((gate.target_id, gate.connection_id, gate.native_id))
                        .or_default()
                        .insert(work.step_id.clone());
                }
            }
        }
        Ok(stacked)
    }

    /// Which work is provisional on which gate, from this machine's records. It
    /// contacts nothing and writes nothing.
    pub fn provisional_status(
        &self,
        owner_id: &str,
    ) -> Result<ProvisionalView, OrchestrationError> {
        let mut view = ProvisionalView {
            enabled: self.speculating(owner_id).is_some(),
            ..ProvisionalView::default()
        };
        let Some(document) = self.provisional_store().read::<Document>(owner_id)? else {
            return Ok(view);
        };
        for (assignment, record) in &document.ledger.records {
            let Some(work) = document.work.get(assignment) else {
                continue;
            };
            let row = view_work(record, work);
            for gate in &row.gates {
                match gate.result {
                    GateResult::Red if record.state == SpeculationState::AtRisk => {
                        let known = view.repair_first.iter_mut().find(|repair| {
                            repair.target_id == gate.target_id
                                && repair.connection_id == gate.connection_id
                                && repair.native_id == gate.native_id
                                && repair.commit == gate.commit
                        });
                        match known {
                            Some(repair) => repair.stacked.push(row.step_id.clone()),
                            None => view.repair_first.push(RepairFirst {
                                target_id: gate.target_id.clone(),
                                connection_id: gate.connection_id.clone(),
                                native_id: gate.native_id.clone(),
                                commit: gate.commit.clone(),
                                stacked: vec![row.step_id.clone()],
                            }),
                        }
                    }
                    GateResult::Superseded | GateResult::Cancelled | GateResult::Unknown
                        if record.state == SpeculationState::Provisional
                            && !view.reevaluate.contains(gate) =>
                    {
                        view.reevaluate.push(gate.clone());
                    }
                    _ => {}
                }
            }
            view.work.push(row);
        }
        view.work
            .sort_by_key(|row| (!live(row.state), row.recorded_at_ms, row.assignment.clone()));
        Ok(view)
    }

    /// The provisional work on one target ticket, for `run status`.
    pub(super) fn provisional_on_target(
        &self,
        owner_id: &str,
        target: &RunTarget,
    ) -> Option<ProvisionalWork> {
        let document = self
            .provisional_store()
            .read::<Document>(owner_id)
            .ok()
            .flatten()?;
        document
            .work
            .iter()
            .filter(|(_, work)| work.target == *target)
            .filter_map(|(assignment, work)| Some((document.ledger.records.get(assignment)?, work)))
            .max_by_key(|(record, work)| (live(record.state), work.recorded_at_ms))
            .map(|(record, work)| view_work(record, work))
    }

    /// The fix of a failed prerequisite invalidated this step's provisional work. The
    /// record and its history stay; the work is assigned again from confirmed state.
    /// It needs an identified actor and is audited. The run on the target is not
    /// touched: cancelling or handing it off is the run's own control (§26.14).
    pub fn readiness_reopen(
        &self,
        owner_id: &str,
        request: &ReopenProvisional,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ProvisionalWork, OrchestrationError> {
        let Some(actor_id) = actor.and_then(|actor| actor.id.clone()) else {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to reopen provisional work",
            ));
        };
        if request.reason.is_empty()
            || request.reason.len() > MAX_REASON_BYTES
            || !request
                .reason
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
        {
            return Err(OrchestrationError::Invalid(
                "a reopen reason is a short lowercase code",
            ));
        }
        let step = self.plan_step(owner_id, &request.step_id)?;
        let now_ms = tx.clock.now_ms();
        let (row, change) =
            self.provisional_store()
                .update::<Document, _>(owner_id, |document| {
                    let found = of_step(document, &step.step_id)
                        .map(|(record, _)| (record.assignment.clone(), record.state));
                    let Some((assignment, state)) = found else {
                        return Err(OrchestrationError::Readiness(
                            ReadinessError::NotProvisional,
                        ));
                    };
                    // A confirmed base cannot invalidate the work built on it.
                    if state == SpeculationState::Confirmed {
                        return Err(OrchestrationError::Readiness(
                            ReadinessError::NotProvisional,
                        ));
                    }
                    let record = document
                        .ledger
                        .reopen(
                            &assignment,
                            &request.reason,
                            seconds(now_ms),
                            Some(&actor_id),
                        )
                        .map_err(|_| refused())?;
                    let work = document
                        .work
                        .get(&assignment)
                        .ok_or(OrchestrationError::Corrupt)?;
                    let changed = state != SpeculationState::Reopened;
                    let change = changed.then(|| Change {
                        step_id: work.step_id.clone(),
                        reason: "reopened",
                        gate_id: None,
                        dedupe: format!("provisional-{assignment}-{}", record.events.len()),
                    });
                    Ok(((view_work(&record, work), change), changed))
                })?;
        if let Some(change) = change {
            self.provisional_audit(
                owner_id,
                &step.target.target_id,
                actor,
                Audited {
                    action: "provisional_reopen",
                    outcome: AuditOutcome::Changed,
                    reason: &request.reason,
                    operation_id: Some(&row.operation_id),
                },
                tx,
            )?;
            self.provisional_publish(owner_id, vec![change], now_ms);
            self.provisional_note(
                owner_id,
                &step.ticket,
                format!(
                    "Provisional work reopened: plan step `{}` (operation `{}`) by `{actor_id}` \
                     with reason `{}`. Its record is kept.",
                    step.step_id, row.operation_id, request.reason
                ),
                tx,
            );
            self.provisional_reopen_decision(owner_id, &step.ticket, &row, tx);
        }
        Ok(row)
    }
}
