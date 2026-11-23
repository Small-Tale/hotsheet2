//! HS2-HCW7MD: Propose Plan. One press starts a planning AI session in the
//! orchestration project itself, through the hosted half of the orchestrator-owned
//! sessions (§26.16) with this project as its own checkout: the drawer's ordinary AI
//! terminal launch through the injected [`SessionHost`], the project's own tool
//! setup, this machine's admission, the saved tab label and the readiness-aware role
//! prompt. The planner then proposes the dry-run plan with the existing ticket and
//! readiness actions; nothing here creates a ticket or a step, and nothing is placed
//! until a person starts orchestration.
use std::collections::BTreeMap;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::ops;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, AuditRecord, NewHostedSession, PlannerFacts, PlanningState, ProposeDecision,
    ReadinessError, SessionError, SessionOrigin, SessionRole, TargetOpError, current_planner,
    decide_propose, planning_state,
};
use serde::{Deserialize, Serialize};

use super::sessions::{
    HostedSessionStart, HostedSessionView, HostedStartOutcome, PushAttempt, RolePrompt,
    TOOL_SETUP_FAILED, ToolSetup, planner_prompt_text, project_name,
};
use super::tickets::identifier;
use super::*;

/// The most goal tickets an answer lists; `goal_count` still counts every one.
pub const MAX_LISTED_GOALS: usize = 256;

/// A press of Propose Plan.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposePlan {
    /// The AI tool the planner runs in, by plugin id.
    pub tool: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Plan only this goal ticket; every open goal ticket when omitted.
    #[serde(default)]
    pub goal: Option<OrchestrationTicket>,
    /// A retained id: the same id answers with the same planner and never launches
    /// twice. A fresh one is minted when omitted.
    #[serde(default)]
    pub operation_id: Option<String>,
    /// HS2-190RDX: the press is an Update Plan, whose proposal id is the plan update
    /// the planner drafts. Never read from a request.
    #[serde(skip)]
    pub(super) plan_update: bool,
}

/// An open goal ticket of the orchestration project, with the plan steps it has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalTicket {
    pub connection_id: String,
    /// The canonical id.
    pub native_id: String,
    pub slug: String,
    /// Plan steps this machine holds for the goal.
    pub steps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposeOutcome {
    /// A planner was launched for this press.
    Started,
    /// The planner this operation id, or the live planner, names answers instead.
    Existing,
    /// HS2-FDS95D: the live planner had finished its turn, and the new proposal was
    /// pushed into it. No second planner was launched.
    Replanned,
}

/// What a press of Propose Plan did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanProposal {
    pub outcome: ProposeOutcome,
    /// The planner's operation id, which the planner's prompt names.
    pub proposal_id: String,
    pub goals: Vec<GoalTicket>,
    pub goal_count: u32,
    /// Declared projects the planner may plan work in.
    pub targets: u32,
    pub session: HostedSessionView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_prompt: Option<RolePrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_setup: Option<ToolSetup>,
}

/// Where planning stands, for the Orchestration view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningView {
    pub state: PlanningState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    /// The live planner, or the latest one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<HostedSessionView>,
    pub goals: Vec<GoalTicket>,
    pub goal_count: u32,
    /// Every plan step this machine holds for the project.
    pub steps: u32,
    /// Whether the policy places sessions. While false the plan is a dry run.
    pub placement_automatic: bool,
    /// HS2-FDS95D: the live planner finished its turn, so a press gives it a new
    /// proposal rather than answering with it.
    #[serde(default)]
    pub replannable: bool,
    /// HS2-190RDX: the latest plan update: drafting beside the live plan, or how the
    /// last one ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<super::PlanUpdateView>,
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

impl OrchestrationRuntime {
    /// The open goal tickets of the orchestration project, by canonical id, with the
    /// plan steps each has. `only` names one goal, which must be open.
    pub(super) fn plan_goals(
        &self,
        owner: &Checkout,
        only: Option<&OrchestrationTicket>,
    ) -> Result<(Vec<GoalTicket>, u32), OrchestrationError> {
        let mut steps: BTreeMap<(String, String), u32> = BTreeMap::new();
        for record in self.plan().records(&owner.id)? {
            *steps
                .entry((
                    record.step.ticket.connection_id.clone(),
                    record.step.ticket.native_id.clone(),
                ))
                .or_default() += 1;
        }
        let open = |connection_id: &str| -> Result<Vec<GoalTicket>, OrchestrationError> {
            let store = owner
                .verified_git_source(connection_id)
                .map_err(checkout_error)?;
            let tickets = ops::query(
                &store,
                &ops::TicketQuery {
                    open_only: true,
                    ..ops::TicketQuery::default()
                },
            )
            .map_err(|_| OrchestrationError::TargetOperation(TargetOpError::Failed))?;
            Ok(tickets
                .into_iter()
                .map(|ticket| {
                    let native_id = ticket.id.to_string();
                    GoalTicket {
                        steps: steps
                            .get(&(connection_id.to_owned(), native_id.clone()))
                            .copied()
                            .unwrap_or(0),
                        connection_id: connection_id.to_owned(),
                        native_id,
                        slug: ticket.slug,
                    }
                })
                .collect())
        };
        let mut goals = match only {
            Some(goal) => {
                let (_, id, connection_id) = self.orchestration_store(&owner.id, goal)?;
                let id = id.to_string();
                let found: Vec<GoalTicket> = open(&connection_id)?
                    .into_iter()
                    .filter(|candidate| candidate.native_id == id)
                    .collect();
                if found.is_empty() {
                    return Err(OrchestrationError::Readiness(ReadinessError::GoalNotOpen));
                }
                found
            }
            None => {
                let mut all = Vec::new();
                for source in owner
                    .sources
                    .iter()
                    .filter(|source| source.provider == "git")
                {
                    all.extend(open(&source.connection_id)?);
                }
                all
            }
        };
        // Canonical ids are ULIDs: oldest goal first, the same order on every read.
        goals.sort_by(|a, b| {
            (a.native_id.as_str(), a.connection_id.as_str())
                .cmp(&(b.native_id.as_str(), b.connection_id.as_str()))
        });
        let total = count(goals.len());
        goals.truncate(MAX_LISTED_GOALS);
        Ok((goals, total))
    }

    /// Every planner this machine hosts for the project, newest decisions computed
    /// from what the terminal host says now. A planner whose terminal is gone is
    /// recorded as ended on the way.
    pub(super) fn planners(
        &self,
        owner: &Checkout,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<HostedSessionView>, OrchestrationError> {
        Ok(self
            .hosted_sessions(&owner.id, None, false, tx)?
            .sessions
            .into_iter()
            .filter(|view| view.session.role == SessionRole::Planner)
            .collect())
    }

    pub(super) fn planner_facts(planners: &[HostedSessionView]) -> Vec<PlannerFacts> {
        planners
            .iter()
            .map(|view| PlannerFacts {
                operation_id: view.session.operation_id.clone(),
                started: view.session.started,
                ended: view.session.ended_at_ms.is_some(),
                alive: view.alive,
                created_at_ms: view.session.created_at_ms,
                proposals: view.session.proposals.clone(),
                finished_turn: finished_turn(view),
            })
            .collect()
    }

    fn plan_audit(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        operation_id: Option<&str>,
        outcome: &Result<PlanProposal, OrchestrationError>,
        tx: &Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        let (outcome, reason) = match outcome {
            Ok(proposal) => (
                AuditOutcome::Succeeded,
                Some(match proposal.outcome {
                    ProposeOutcome::Started => "plan_started",
                    ProposeOutcome::Existing => "plan_existing",
                    ProposeOutcome::Replanned => "plan_replanned",
                }),
            ),
            Err(error) => (AuditOutcome::Failed, Some(error.code())),
        };
        self.audit_record(
            owner_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: None,
                action: "plan_propose".into(),
                operation_id: operation_id.map(Into::into),
                outcome,
                reason: reason.map(Into::into),
                permission_id: None,
                requested_by: None,
            },
        )
    }

    /// Propose Plan: start the project's planning session, or answer with the one
    /// already there. One planner per orchestration project at a time; the same
    /// operation id never launches twice. Refused before anything launches when the
    /// project declares no target (`plan_no_targets`), has no open goal ticket
    /// (`plan_no_goals`), or the named goal is not open (`plan_goal_not_open`). Needs
    /// an identified actor and is audited as `plan_propose`.
    pub fn plan_propose(
        &self,
        owner_id: &str,
        request: ProposePlan,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<PlanProposal, OrchestrationError> {
        if actor.and_then(|actor| actor.id.as_ref()).is_none() {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to propose a plan",
            ));
        }
        identifier(&request.tool, "bounded tool id required")?;
        if let Some(operation_id) = &request.operation_id {
            identifier(operation_id, "bounded operation id required")?;
        }
        let owner = self.owner(owner_id)?;
        let mut operation_id = request.operation_id.clone();
        let outcome = self.plan_propose_inner(&owner, &request, &mut operation_id, actor, tx);
        self.plan_audit(owner_id, actor, operation_id.as_deref(), &outcome, tx)?;
        outcome
    }

    pub(super) fn plan_propose_inner(
        &self,
        owner: &Checkout,
        request: &ProposePlan,
        operation_id: &mut Option<String>,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<PlanProposal, OrchestrationError> {
        let targets = count(self.declarations(&owner.id)?.targets.len());
        if targets == 0 {
            return Err(OrchestrationError::Readiness(ReadinessError::NoTargets));
        }
        let (goals, goal_count) = self.plan_goals(owner, request.goal.as_ref())?;
        if goal_count == 0 {
            return Err(OrchestrationError::Readiness(ReadinessError::NoGoals));
        }
        let planners = self.planners(owner, tx)?;
        let facts = Self::planner_facts(&planners);
        let decision = decide_propose(&facts, request.operation_id.as_deref(), || {
            hotsheet_ticketing::mint_ulid(tx.clock, &mut *tx.rng).to_string()
        });
        // The proposal a reused planner answers for: the retained id when it names one
        // of its proposals, else the one it works on now.
        let answered = |planner: &str| {
            facts
                .iter()
                .find(|known| known.operation_id == planner)
                .map(|known| {
                    request
                        .operation_id
                        .as_deref()
                        .filter(|requested| known.answers_for(requested))
                        .unwrap_or_else(|| known.latest_proposal())
                        .to_owned()
                })
        };
        if let ProposeDecision::Replan { planner, proposal } = &decision {
            *operation_id = Some(proposal.clone());
            let view = planners
                .iter()
                .find(|view| &view.session.operation_id == planner)
                .ok_or(OrchestrationError::Corrupt)?;
            if let Some(replanned) = self.plan_replan(owner, view, proposal, request, &goals, tx)? {
                return Ok(PlanProposal {
                    goals,
                    goal_count,
                    targets,
                    ..replanned
                });
            }
            // Not delivered: nothing was recorded, and the planner answers as it is.
            *operation_id = answered(planner);
            return Ok(PlanProposal {
                outcome: ProposeOutcome::Existing,
                proposal_id: answered(planner).unwrap_or_else(|| planner.clone()),
                goals,
                goal_count,
                targets,
                role_prompt: view.role_prompt,
                tool_setup: None,
                session: view.clone(),
            });
        }
        let session = match &decision {
            // The press answers with that planner as it was asked for, so the start
            // is repeated with its own fields and never conflicts with them.
            ProposeDecision::Reuse(reused) => planners
                .iter()
                .find(|view| &view.session.operation_id == reused)
                .map(|view| NewHostedSession {
                    account: None,
                    operation_id: view.session.operation_id.clone(),
                    tool: view.session.tool.clone(),
                    model: view.session.model.clone(),
                    effort: view.session.effort.clone(),
                    role: SessionRole::Planner,
                    origin: view.session.origin.clone(),
                })
                .ok_or(OrchestrationError::Corrupt)?,
            ProposeDecision::Replan { .. } => return Err(OrchestrationError::Corrupt),
            ProposeDecision::Start(started) => NewHostedSession {
                account: None,
                operation_id: started.clone(),
                tool: request.tool.clone(),
                model: request.model.clone(),
                effort: request.effort.clone(),
                role: SessionRole::Planner,
                origin: SessionOrigin {
                    owner_checkout_id: owner.id.clone(),
                    project: project_name(owner),
                    run_id: None,
                    // One named goal is the ticket its prompt names.
                    ticket: request
                        .goal
                        .as_ref()
                        .and_then(|_| goals.first())
                        .map(|goal| goal.native_id.clone()),
                },
            },
        };
        *operation_id = Some(session.operation_id.clone());
        let reply = self.hosted_session_start(
            &owner.id,
            HostedSessionStart {
                session,
                // HS2-30VKV9: the planner runs in this project, so the host applies
                // the project's own checked-in machine limits (`hosted_admission`).
                limits: None,
                actor: actor.cloned(),
                resources: Vec::new(),
            },
            tx,
        )?;
        let Some(view) = reply
            .session
            .filter(|_| reply.outcome != HostedStartOutcome::Refused)
        else {
            let reason = reply.reason.unwrap_or_default();
            // A session code is reported as itself; an admission reason as a refused
            // admission, as for a target session.
            return Err(OrchestrationError::Session(
                SessionError::from_code(&reason).unwrap_or(if reason == TOOL_SETUP_FAILED {
                    SessionError::StartFailed
                } else {
                    SessionError::AdmissionRefused
                }),
            ));
        };
        let proposal_id = match &decision {
            ProposeDecision::Reuse(planner) => answered(planner),
            _ => None,
        }
        .unwrap_or_else(|| view.session.operation_id.clone());
        if let ProposeDecision::Reuse(_) = &decision {
            *operation_id = Some(proposal_id.clone());
        }
        Ok(PlanProposal {
            outcome: match reply.outcome {
                HostedStartOutcome::Started => ProposeOutcome::Started,
                _ => ProposeOutcome::Existing,
            },
            proposal_id,
            goals,
            goal_count,
            targets,
            role_prompt: reply.role_prompt.or(view.role_prompt),
            tool_setup: reply.tool_setup,
            session: view,
        })
    }

    /// Where planning stands: the live planner or the latest one, the open goal
    /// tickets with their plan steps, and whether the policy places sessions yet.
    /// Starts nothing and asks only this machine's terminal host.
    pub fn plan_status(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<PlanningView, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let planners = self.planners(&owner, tx)?;
        let facts = Self::planner_facts(&planners);
        let current = current_planner(&facts);
        let session = current.and_then(|current| {
            planners
                .iter()
                .find(|view| view.session.operation_id == current.operation_id)
                .cloned()
        });
        let replannable = current.is_some_and(PlannerFacts::replannable);
        let current = current.map(|current| current.latest_proposal().to_owned());
        let (goals, goal_count) = self.plan_goals(&owner, None)?;
        let placement_automatic = self
            .policy(owner_id)?
            .policy
            .is_some_and(|policy| policy.placement.automatic);
        Ok(PlanningView {
            state: planning_state(&facts),
            proposal_id: current,
            session,
            goals,
            goal_count,
            steps: count(self.plan().records(&owner.id)?.len()),
            placement_automatic,
            replannable,
            update: self.plan_update_status(&owner, tx)?,
        })
    }

    /// HS2-FDS95D: push a new proposal into a planner that finished its turn. Only a
    /// delivered push is recorded, so an undelivered one can be pressed again. `None`
    /// when it was not delivered.
    fn plan_replan(
        &self,
        owner: &Checkout,
        view: &HostedSessionView,
        proposal: &str,
        request: &ProposePlan,
        goals: &[GoalTicket],
        tx: &mut Transaction<'_>,
    ) -> Result<Option<PlanProposal>, OrchestrationError> {
        // One named goal is the goal this proposal plans, by its slug.
        let goal = request
            .goal
            .as_ref()
            .and_then(|_| goals.first())
            .map(|goal| goal.slug.as_str());
        let text = planner_prompt_text(
            &owner.id,
            &view.worker_id,
            proposal,
            goal,
            request.plan_update,
        );
        let attempt = self
            .sessions
            .join_role(owner, &view.session.session_id, &text);
        if attempt != PushAttempt::Delivered {
            return Ok(None);
        }
        self.hosted_sessions_registry().add_proposal(
            &owner.id,
            &view.session.operation_id,
            proposal,
        )?;
        let session = self
            .planners(owner, tx)?
            .into_iter()
            .find(|planner| planner.session.operation_id == view.session.operation_id)
            .ok_or(OrchestrationError::Corrupt)?;
        Ok(Some(PlanProposal {
            outcome: ProposeOutcome::Replanned,
            proposal_id: proposal.to_owned(),
            goals: Vec::new(),
            goal_count: 0,
            targets: 0,
            role_prompt: Some(RolePrompt::Delivered),
            tool_setup: None,
            session,
        }))
    }
}

/// HS2-FDS95D: a planner finished its turn: its terminal runs, its prompt reached it,
/// its tool is past any first-run question (HS2-5RYXNN), and its host says it is not
/// busy. A tool that reports no busy state reads as not busy, so a press may reach a
/// planner still at work; it then gets one more prompt, and the plan is idempotent.
fn finished_turn(view: &HostedSessionView) -> bool {
    view.alive == Some(true)
        && view.busy == Some(false)
        && view.tool_ready != Some(false)
        && matches!(
            view.role_prompt,
            Some(RolePrompt::Delivered | RolePrompt::AtLaunch)
        )
}
