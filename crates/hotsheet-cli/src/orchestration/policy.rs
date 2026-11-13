//! HS2-YCJDEB: the throughput policy, approvals and dry-run over the shared runtime.
//! Reads make no request and read no secret; `set` and `decide` are a person's acts.
use std::collections::BTreeSet;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AccessDecision, AccessRequest, Approval, ApprovalRegistry, AuditOutcome, AuditRecord,
    DeclarationState, DenialReason, GrantRegistry, PolicyBody, PolicyDenial, PolicyProblem,
    PolicyQuestion, PolicyStore, PolicyVerdict, PolicyView, SessionLoad, SessionRequest, SetPolicy,
    TargetAction,
};
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Serialize)]
pub struct PolicyValidation {
    pub valid: bool,
    pub problems: Vec<PolicyProblem>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyProposal {
    /// A proposed policy; omit to examine the stored one.
    #[serde(default)]
    pub policy: Option<PolicyBody>,
    /// Also report whether this actor's grants cover each permitted action.
    #[serde(default)]
    pub actor_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DryRunAction {
    pub action: TargetAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// `allowed`, `approval_required` or `denied`.
    pub policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_detail: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant: Option<AccessDecision>,
    /// The orchestrator could take this action on its own right now.
    pub automatic: bool,
}

#[derive(Debug, Serialize)]
pub struct DryRunTarget {
    pub target_id: String,
    pub label: String,
    pub bound: bool,
    /// `permitted`, `disabled` or `not_listed`.
    pub policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_sessions: Option<u32>,
    pub machine: hotsheet_ticketing::orchestration::MachineLimits,
    pub speculation: bool,
    pub actions: Vec<DryRunAction>,
}

#[derive(Debug, Serialize)]
pub struct DryRun {
    /// `stored`, `proposed` or `absent`.
    pub source: &'static str,
    pub valid: bool,
    pub problems: Vec<PolicyProblem>,
    pub targets: Vec<DryRunTarget>,
}

/// The policy's answer folded into the grant decision's vocabulary.
pub(super) enum PolicyAnswer {
    Allowed,
    Denied(PolicyDenial),
    Approval(hotsheet_ticketing::orchestration::ApprovalGate),
}

impl OrchestrationRuntime {
    fn policy_store(&self, owner_id: &str) -> Result<PolicyStore, OrchestrationError> {
        Ok(PolicyStore::new(Path::new(&self.owner(owner_id)?.root)))
    }

    fn declared(&self, owner_id: &str) -> Result<BTreeSet<String>, OrchestrationError> {
        Ok(self
            .declarations(owner_id)?
            .targets
            .into_iter()
            .filter(|target| target.state == DeclarationState::Active)
            .map(|target| target.target_id)
            .collect())
    }

    pub fn policy(&self, owner_id: &str) -> Result<PolicyView, OrchestrationError> {
        self.policy_store(owner_id)?.read()
    }

    /// Replace the policy. The session a policy governs must not loosen its own
    /// bounds, so an AI actor is refused like it is for grants.
    pub fn set_policy(
        &self,
        owner_id: &str,
        request: SetPolicy,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<PolicyView, OrchestrationError> {
        access::person(actor)?;
        let store = self.policy_store(owner_id)?;
        let before = store.read()?.revision;
        let view = store.set(request, Some(&self.declared(owner_id)?), tx)?;
        if view.revision != before {
            // HS2-ECWG50: a scheduler following this machine plans under the new policy.
            Self::scheduler_wake_at(&self.home);
            self.audit_record(
                owner_id,
                AuditRecord {
                    at_ms: tx.clock.now_ms(),
                    actor_role: access::role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    target_id: None,
                    action: "policy_set".into(),
                    operation_id: None,
                    outcome: AuditOutcome::Changed,
                    reason: None,
                    grant_id: None,
                },
            )?;
        }
        Ok(view)
    }

    /// Every problem with a proposed policy, or with the stored one. Never writes.
    pub fn validate_policy(
        &self,
        owner_id: &str,
        proposal: PolicyProposal,
    ) -> Result<PolicyValidation, OrchestrationError> {
        let body = match proposal.policy {
            Some(body) => body,
            None => self
                .policy(owner_id)?
                .policy
                .ok_or(OrchestrationError::NotFound)?,
        };
        let problems = body.problems(Some(&self.declared(owner_id)?));
        Ok(PolicyValidation {
            valid: problems.is_empty(),
            problems,
        })
    }

    /// What a policy would let the orchestrator do to each declared target, and, for a
    /// named actor, whether this machine's grants agree. Writes nothing, files no
    /// approval request and contacts no target.
    pub fn dry_run_policy(
        &self,
        owner_id: &str,
        proposal: PolicyProposal,
        tx: &mut Transaction<'_>,
    ) -> Result<DryRun, OrchestrationError> {
        let stored = self.policy(owner_id)?.policy;
        let (source, body) = match (proposal.policy, stored) {
            (Some(body), _) => ("proposed", Some(body)),
            (None, Some(body)) => ("stored", Some(body)),
            (None, None) => ("absent", None),
        };
        let manifest = self.declarations(owner_id)?;
        let declared = self.declared(owner_id)?;
        let problems = body
            .as_ref()
            .map(|body| body.problems(Some(&declared)))
            .unwrap_or_default();
        let empty = PolicyBody::default();
        let policy = body.as_ref().unwrap_or(&empty);
        let now_ms = tx.clock.now_ms();
        let mut targets = Vec::new();
        for declaration in manifest
            .targets
            .iter()
            .filter(|target| target.state == DeclarationState::Active)
        {
            let target = TargetRef {
                owner_checkout_id: owner_id.into(),
                target_id: declaration.target_id.clone(),
            };
            let binding = Self::binding_at(&self.home, &target)?;
            let entry = policy.target(&declaration.target_id);
            let mut actions = Vec::new();
            for action in TargetAction::ALL {
                let tools: Vec<Option<&str>> = match (action, entry) {
                    (TargetAction::SessionsManage, Some(entry)) if !entry.tools.is_empty() => entry
                        .tools
                        .iter()
                        .map(|tool| Some(tool.tool.as_str()))
                        .collect(),
                    _ => vec![None],
                };
                for tool in tools {
                    // A start from an idle machine, on the tool's first listed model
                    // and account: the most permissive question this policy can answer.
                    let session = tool.map(|tool| SessionRequest {
                        tool: tool.into(),
                        model: entry
                            .and_then(|entry| entry.tools.iter().find(|scope| scope.tool == tool))
                            .and_then(|scope| scope.models.first().cloned()),
                        running: 0,
                        starting: true,
                    });
                    let load = tool.map(|tool| SessionLoad {
                        account: policy
                            .tools
                            .iter()
                            .find(|configured| configured.tool == tool)
                            .and_then(|configured| configured.accounts.first())
                            .map(|account| account.account.clone()),
                        ..SessionLoad::default()
                    });
                    let verdict = if body.is_none() && action != TargetAction::SessionsManage {
                        // Without a policy, explicit actions are governed by grants alone.
                        PolicyVerdict::Allowed
                    } else if body.is_none() {
                        PolicyVerdict::Denied(PolicyDenial::NoPolicy)
                    } else {
                        policy.evaluate(&PolicyQuestion {
                            target_id: &declaration.target_id,
                            action,
                            session: session.as_ref(),
                            load: load.as_ref(),
                            provisional: false,
                            role_session: false,
                        })
                    };
                    let (answer, detail) = match verdict {
                        PolicyVerdict::Allowed => ("allowed", None),
                        PolicyVerdict::ApprovalRequired(gate) => {
                            ("approval_required", Some(gate.code()))
                        }
                        PolicyVerdict::Denied(denial) => ("denied", Some(denial.code())),
                    };
                    let grant = match &proposal.actor_id {
                        None => None,
                        Some(actor_id) => Some(GrantRegistry::new(&self.home).decide(
                            &target,
                            binding.destination.as_ref(),
                            &AccessRequest {
                                session: session.clone(),
                                ..AccessRequest::new(actor_id.clone(), action)
                            },
                            now_ms,
                        )?),
                    };
                    actions.push(DryRunAction {
                        action,
                        tool: tool.map(Into::into),
                        policy: answer,
                        policy_detail: detail,
                        automatic: answer == "allowed"
                            && body.is_some()
                            && grant.as_ref().is_some_and(|grant| grant.allowed),
                        grant,
                    });
                }
            }
            targets.push(DryRunTarget {
                target_id: declaration.target_id.clone(),
                label: declaration.label.clone(),
                bound: binding.destination.is_some(),
                policy: match entry {
                    Some(entry) if entry.enabled => "permitted",
                    Some(_) => "disabled",
                    None => "not_listed",
                },
                max_sessions: entry.map(|entry| entry.max_sessions),
                machine: policy.machine_limits(&declaration.target_id),
                speculation: policy.speculation_allowed(&declaration.target_id),
                actions,
            });
        }
        Ok(DryRun {
            source,
            valid: problems.is_empty(),
            problems,
            targets,
        })
    }

    /// The policy half of the authorization gate. An unreadable or corrupt policy
    /// answers as a denial: a broken policy must not fall back to "no policy".
    pub(super) fn policy_answer(
        &self,
        owner_id: &str,
        request: &AccessRequest,
        target_id: &str,
    ) -> PolicyAnswer {
        let policy = match self.policy(owner_id) {
            Ok(view) => view.policy,
            Err(_) => return PolicyAnswer::Denied(PolicyDenial::NoPolicy),
        };
        let Some(policy) = policy else {
            return if request.action == TargetAction::SessionsManage {
                PolicyAnswer::Denied(PolicyDenial::NoPolicy)
            } else {
                PolicyAnswer::Allowed
            };
        };
        match policy.evaluate(&PolicyQuestion {
            target_id,
            action: request.action,
            session: request.session.as_ref(),
            load: request.load.as_ref(),
            provisional: request.provisional,
            role_session: request.role_session,
        }) {
            PolicyVerdict::Allowed => PolicyAnswer::Allowed,
            PolicyVerdict::Denied(denial) => PolicyAnswer::Denied(denial),
            PolicyVerdict::ApprovalRequired(gate) => PolicyAnswer::Approval(gate),
        }
    }

    pub fn approvals(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<Approval>, OrchestrationError> {
        ApprovalRegistry::new(&self.home).list(owner_id, tx.clock.now_ms())
    }

    /// A person approves or denies one pending request.
    pub fn decide_approval(
        &self,
        owner_id: &str,
        approval_id: &str,
        approve: bool,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Approval, OrchestrationError> {
        access::person(actor)?;
        let (approval, changed) = ApprovalRegistry::new(&self.home).decide(
            owner_id,
            approval_id,
            approve,
            actor.and_then(|actor| actor.id.clone()),
            tx,
        )?;
        if changed {
            self.audit_record(
                owner_id,
                AuditRecord {
                    at_ms: tx.clock.now_ms(),
                    actor_role: access::role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    target_id: Some(approval.target.target_id.clone()),
                    action: approval.gate.code().into(),
                    operation_id: Some(approval.operation_id.clone()),
                    outcome: if approve {
                        AuditOutcome::Approved
                    } else {
                        AuditOutcome::Rejected
                    },
                    reason: None,
                    grant_id: None,
                },
            )?;
        }
        Ok(approval)
    }
}

/// Fold a policy answer into a grant-allowed decision; `approved` says whether a
/// required approval is in hand.
pub(super) fn apply(
    decision: AccessDecision,
    answer: PolicyAnswer,
    approved: impl FnOnce(
        hotsheet_ticketing::orchestration::ApprovalGate,
    ) -> Result<bool, OrchestrationError>,
) -> Result<AccessDecision, OrchestrationError> {
    if !decision.allowed {
        return Ok(decision);
    }
    Ok(match answer {
        PolicyAnswer::Allowed => decision,
        PolicyAnswer::Denied(denial) => AccessDecision {
            detail: Some(denial.code().into()),
            ..AccessDecision::denied(DenialReason::PolicyDenied)
        },
        PolicyAnswer::Approval(gate) if approved(gate)? => decision,
        PolicyAnswer::Approval(gate) => AccessDecision {
            detail: Some(gate.code().into()),
            ..AccessDecision::denied(DenialReason::ApprovalRequired)
        },
    })
}
