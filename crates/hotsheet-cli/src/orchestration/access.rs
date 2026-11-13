//! HS2-SBQ3T8: grants, the authorization gate and the audit journal over the shared
//! runtime. Reads use the machine's cached binding and never touch transport or
//! secrets; only `grant` and `authorize` confirm the destination freshly.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AccessDecision, AccessRequest, ApprovalRegistry, AuditJournal, AuditOutcome, AuditPage,
    AuditRecord, CreateGrant, DenialReason, Grant, GrantRegistry, GrantView, SessionLoad,
    SessionRequest, TargetAction, TargetResolution,
};

use super::*;

/// How often `authorize` resolves again after losing a race with another resolution
/// of the same target.
pub(super) const RESOLVE_ATTEMPTS: usize = 8;

/// Proof that one actor may perform one action now. Later steps report their result
/// with [`OrchestrationRuntime::record_outcome`] under the same operation id.
pub struct Authorized {
    pub resolution: TargetResolution,
    pub grant_id: String,
    pub actor_id: String,
    pub action: TargetAction,
    pub operation_id: Option<String>,
}

/// One target action as the gate sees it.
#[derive(Debug, Clone)]
pub struct ActionRequest {
    pub action: TargetAction,
    pub session: Option<SessionRequest>,
    /// Required when the policy asks a person first: the approval is for this id.
    pub operation_id: Option<String>,
    /// HS2-YCJDEB: sessions already running; a session start without it is denied.
    pub load: Option<SessionLoad>,
    pub provisional: bool,
    pub role_session: bool,
    /// HS2-EBHDSB: this continues a decision already taken (renewing or releasing a
    /// lease the run holds). Grants and policy limits still apply; a person is not
    /// asked again for something they already approved.
    pub continuing: bool,
    /// HS2-WN0MXZ: the action is outside what the grant automates, so a person is
    /// asked under this gate even when the policy would not ask.
    pub person_gate: Option<hotsheet_ticketing::orchestration::ApprovalGate>,
    /// HS2-5JBYPV: ask a person under this gate even when the policy does not, for an
    /// act that cannot be undone. Never skipped by `continuing`.
    pub approval: Option<hotsheet_ticketing::orchestration::ApprovalGate>,
}

impl ActionRequest {
    pub fn new(action: TargetAction) -> Self {
        Self {
            action,
            session: None,
            operation_id: None,
            load: None,
            provisional: false,
            role_session: false,
            continuing: false,
            person_gate: None,
            approval: None,
        }
    }

    pub fn continuing(mut self) -> Self {
        self.continuing = true;
        self
    }

    pub fn operation(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = Some(operation_id.into());
        self
    }
}

pub(super) fn role_code(actor: Option<&MutationActor>) -> Option<String> {
    actor.map(|actor| {
        serde_json::to_value(actor.role)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".into())
    })
}

/// Authority is given by a person at the serving machine, never by the AI it governs.
pub(super) fn person(actor: Option<&MutationActor>) -> Result<(), OrchestrationError> {
    if actor.is_some_and(MutationActor::is_ai) {
        return Err(OrchestrationError::Forbidden(
            "an AI actor cannot change orchestration authority, policy or approvals",
        ));
    }
    Ok(())
}

impl OrchestrationRuntime {
    fn grant_registry(&self) -> GrantRegistry {
        GrantRegistry::new(&self.home)
    }

    fn journal(&self, owner_id: &str) -> Result<AuditJournal, OrchestrationError> {
        AuditJournal::new(&self.home, owner_id)
    }

    pub(super) fn audit_record(
        &self,
        owner_id: &str,
        record: AuditRecord,
    ) -> Result<(), OrchestrationError> {
        self.journal(owner_id)?.append(record).map(|_| ())
    }

    /// Retained grants with their state against this machine's cached destination.
    pub fn grants(
        &self,
        target: &TargetRef,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<GrantView>, OrchestrationError> {
        let binding = Self::binding_at(&self.home, target)?;
        self.grant_registry()
            .list(target, binding.destination.as_ref(), tx.clock.now_ms())
    }

    /// Issue a grant against the freshly confirmed destination. An unusable target
    /// cannot be granted: there is no reviewed destination to pin.
    pub fn grant(
        &self,
        target: &TargetRef,
        request: CreateGrant,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<GrantView, OrchestrationError> {
        person(actor)?;
        // HS2-YZBCX5: a follower resolving the same target is no reason to refuse.
        let resolution = self.resolve_contended(target, tx)?;
        let Some(destination) = resolution
            .binding
            .destination
            .as_ref()
            .filter(|_| resolution.usable)
        else {
            return Err(OrchestrationError::Denied(DenialReason::TargetUnavailable));
        };
        let created_by = actor.and_then(|actor| actor.id.clone());
        let known = request.grant_id.as_ref().is_some_and(|id| {
            self.grant_registry()
                .list(target, Some(destination), tx.clock.now_ms())
                .is_ok_and(|grants| grants.iter().any(|view| &view.grant.grant_id == id))
        });
        let view = self
            .grant_registry()
            .create(target, request, destination, created_by, tx)?;
        if !known {
            self.audit_record(
                &target.owner_checkout_id,
                AuditRecord {
                    at_ms: tx.clock.now_ms(),
                    actor_role: role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    target_id: Some(target.target_id.clone()),
                    action: "grant_create".into(),
                    operation_id: None,
                    outcome: AuditOutcome::Granted,
                    reason: None,
                    grant_id: Some(view.grant.grant_id.clone()),
                },
            )?;
        }
        Ok(view)
    }

    /// A fresh resolution that survives contention on this machine. Every resolution
    /// rewrites the binding's observation, so two of the same target at once (a
    /// follower and a request, or the CLI beside the server) leave the later writer
    /// with a conflict (HS2-WBREDM). That says nothing about the target: resolve
    /// again, a bounded number of times, rather than refuse. Used by `authorize` and,
    /// since HS2-YZBCX5, by a person's `grant`.
    fn resolve_contended(
        &self,
        target: &TargetRef,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetResolution, OrchestrationError> {
        let mut resolution = self.resolve(target, tx);
        for _ in 0..RESOLVE_ATTEMPTS {
            if !matches!(resolution, Err(OrchestrationError::Conflict)) {
                break;
            }
            resolution = self.resolve(target, tx);
        }
        resolution
    }

    /// Withdraw a grant. Needs no registered owner, manifest, binding or reachable
    /// target, so authority can always be removed.
    pub fn revoke(
        &self,
        target: &TargetRef,
        grant_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Grant, OrchestrationError> {
        person(actor)?;
        let now_ms = tx.clock.now_ms();
        let (grant, changed) = self.grant_registry().revoke(
            target,
            grant_id,
            actor.and_then(|actor| actor.id.clone()),
            tx,
        )?;
        if changed {
            self.audit_record(
                &target.owner_checkout_id,
                AuditRecord {
                    at_ms: now_ms,
                    actor_role: role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    target_id: Some(target.target_id.clone()),
                    action: "grant_revoke".into(),
                    operation_id: None,
                    outcome: AuditOutcome::Revoked,
                    reason: None,
                    grant_id: Some(grant.grant_id.clone()),
                },
            )?;
        }
        Ok(grant)
    }

    /// Side-effect-free answer against the cached destination: what `authorize` would
    /// say if the target is still what this machine last reviewed. Never authority.
    pub fn access(
        &self,
        target: &TargetRef,
        request: &AccessRequest,
        tx: &mut Transaction<'_>,
    ) -> Result<AccessDecision, OrchestrationError> {
        let binding = Self::binding_at(&self.home, target)?;
        let now_ms = tx.clock.now_ms();
        let decision =
            self.grant_registry()
                .decide(target, binding.destination.as_ref(), request, now_ms)?;
        // HS2-YCJDEB: both must allow. The check never files or consumes an approval.
        let answer = self.policy_answer(&target.owner_checkout_id, request, &target.target_id);
        policy::apply(decision, answer, |gate| match &request.operation_id {
            Some(operation_id) => ApprovalRegistry::new(&self.home).approved(
                target,
                &request.actor_id,
                gate,
                operation_id,
                now_ms,
            ),
            None => Ok(false),
        })
    }

    /// The one gate every target action passes: fresh resolution, then the grant, then
    /// the policy, with an audit entry for whichever answer results. Anything not
    /// provably allowed is denied.
    pub fn authorize(
        &self,
        target: &TargetRef,
        actor: Option<&MutationActor>,
        action: ActionRequest,
        tx: &mut Transaction<'_>,
    ) -> Result<Authorized, OrchestrationError> {
        let Some(actor_id) = actor.and_then(|actor| actor.id.clone()) else {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required for a target action",
            ));
        };
        let request = AccessRequest {
            actor_id: actor_id.clone(),
            action: action.action,
            session: action.session,
            operation_id: action.operation_id,
            load: action.load,
            provisional: action.provisional,
            role_session: action.role_session,
        };
        // An unknown owner, tombstoned target or failed probe is a denial, not an error
        // the caller could mistake for "try the action anyway".
        let resolution = self.resolve_contended(target, tx);
        let destination = resolution.as_ref().ok().and_then(|resolution| {
            resolution
                .binding
                .destination
                .as_ref()
                .filter(|_| resolution.usable)
        });
        let granted =
            self.grant_registry()
                .decide(target, destination, &request, tx.clock.now_ms())?;
        // HS2-YCJDEB: the policy is consulted only for a granted action, so an approval
        // request is never filed for something the actor was not authorized to do.
        let answer = if granted.allowed {
            match (
                self.policy_answer(&target.owner_checkout_id, &request, &target.target_id),
                action.person_gate,
            ) {
                (policy::PolicyAnswer::Approval(_), _) if action.continuing => {
                    policy::PolicyAnswer::Allowed
                }
                // HS2-WN0MXZ: a policy denial still wins; an allowed action outside the
                // granted automation scope waits for a person.
                (policy::PolicyAnswer::Allowed, Some(gate)) if !action.continuing => {
                    policy::PolicyAnswer::Approval(gate)
                }
                // HS2-5JBYPV: an irreversible act asks whatever the policy allows.
                (policy::PolicyAnswer::Allowed, _) => match action.approval {
                    Some(gate) => policy::PolicyAnswer::Approval(gate),
                    None => policy::PolicyAnswer::Allowed,
                },
                (answer, _) => answer,
            }
        } else {
            policy::PolicyAnswer::Allowed
        };
        let decision = policy::apply(granted, answer, |gate| {
            let Some(operation_id) = &request.operation_id else {
                return Err(OrchestrationError::Invalid(
                    "an operation id is required for a decision that needs approval",
                ));
            };
            ApprovalRegistry::new(&self.home).consume_or_request(
                target,
                &actor_id,
                gate,
                operation_id,
                tx,
            )
        })?;
        self.audit_record(
            &target.owner_checkout_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: role_code(actor),
                actor_id: Some(actor_id.clone()),
                target_id: Some(target.target_id.clone()),
                action: request.action.code().into(),
                operation_id: request.operation_id.clone(),
                outcome: if decision.allowed {
                    AuditOutcome::Allowed
                } else {
                    AuditOutcome::Denied
                },
                // The specific policy rule or approval gate when there is one.
                reason: decision
                    .detail
                    .clone()
                    .or_else(|| decision.reason.map(|reason| reason.code().into())),
                grant_id: decision.grant_id.clone(),
            },
        )?;
        match (decision.grant_id, resolution) {
            (Some(grant_id), Ok(resolution)) if decision.allowed => Ok(Authorized {
                resolution,
                grant_id,
                actor_id,
                action: request.action,
                operation_id: request.operation_id,
            }),
            (_, _) => Err(OrchestrationError::Denied(
                decision.reason.unwrap_or(DenialReason::NoGrant),
            )),
        }
    }

    /// Report how an authorized action ended, under the same operation id.
    pub fn record_outcome(
        &self,
        target: &TargetRef,
        authorized: &Authorized,
        succeeded: bool,
        reason: Option<&'static str>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        self.audit_record(
            &target.owner_checkout_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: None,
                actor_id: Some(authorized.actor_id.clone()),
                target_id: Some(target.target_id.clone()),
                action: authorized.action.code().into(),
                operation_id: authorized.operation_id.clone(),
                outcome: if succeeded {
                    AuditOutcome::Succeeded
                } else {
                    AuditOutcome::Failed
                },
                reason: reason.map(Into::into),
                grant_id: Some(authorized.grant_id.clone()),
            },
        )
    }

    pub fn audit(
        &self,
        owner_id: &str,
        after: u64,
        limit: usize,
    ) -> Result<AuditPage, OrchestrationError> {
        self.journal(owner_id)?.read(after, limit)
    }
}
