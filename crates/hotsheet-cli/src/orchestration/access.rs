//! HS2-SBQ3T8: grants, the authorization gate and the audit journal over the shared
//! runtime. Reads use the machine's cached binding and never touch transport or
//! secrets; only `grant` and `authorize` confirm the destination freshly.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AccessDecision, AccessRequest, AuditJournal, AuditOutcome, AuditPage, AuditRecord, CreateGrant,
    DenialReason, Grant, GrantRegistry, GrantView, SessionRequest, TargetAction, TargetResolution,
};

use super::*;

/// Proof that one actor may perform one action now. Later steps report their result
/// with [`OrchestrationRuntime::record_outcome`] under the same operation id.
pub struct Authorized {
    pub resolution: TargetResolution,
    pub grant_id: String,
    pub actor_id: String,
    pub action: TargetAction,
    pub operation_id: Option<String>,
}

fn role_code(actor: Option<&MutationActor>) -> Option<String> {
    actor.map(|actor| {
        serde_json::to_value(actor.role)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".into())
    })
}

/// Authority is given by a person at the serving machine, never by the AI it governs.
fn person(actor: Option<&MutationActor>) -> Result<(), OrchestrationError> {
    if actor.is_some_and(MutationActor::is_ai) {
        return Err(OrchestrationError::Forbidden(
            "an AI actor cannot grant or revoke orchestration authority",
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

    fn audit_record(&self, owner_id: &str, record: AuditRecord) -> Result<(), OrchestrationError> {
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
        let resolution = self.resolve(target, tx)?;
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
        self.grant_registry().decide(
            target,
            binding.destination.as_ref(),
            request,
            tx.clock.now_ms(),
        )
    }

    /// The one gate every target action passes: fresh resolution, then the grant, with
    /// an audit entry for either answer. Anything not provably allowed is denied.
    pub fn authorize(
        &self,
        target: &TargetRef,
        actor: Option<&MutationActor>,
        action: TargetAction,
        session: Option<SessionRequest>,
        operation_id: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<Authorized, OrchestrationError> {
        let Some(actor_id) = actor.and_then(|actor| actor.id.clone()) else {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required for a target action",
            ));
        };
        let request = AccessRequest {
            actor_id: actor_id.clone(),
            action,
            session,
            operation_id: operation_id.clone(),
        };
        // An unknown owner, tombstoned target or failed probe is a denial, not an error
        // the caller could mistake for "try the action anyway".
        let resolution = self.resolve(target, tx);
        let destination = resolution.as_ref().ok().and_then(|resolution| {
            resolution
                .binding
                .destination
                .as_ref()
                .filter(|_| resolution.usable)
        });
        let decision =
            self.grant_registry()
                .decide(target, destination, &request, tx.clock.now_ms())?;
        self.audit_record(
            &target.owner_checkout_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: role_code(actor),
                actor_id: Some(actor_id.clone()),
                target_id: Some(target.target_id.clone()),
                action: action.code().into(),
                operation_id: operation_id.clone(),
                outcome: if decision.allowed {
                    AuditOutcome::Allowed
                } else {
                    AuditOutcome::Denied
                },
                reason: decision.reason.map(|reason| reason.code().into()),
                grant_id: decision.grant_id.clone(),
            },
        )?;
        match (decision.grant_id, resolution) {
            (Some(grant_id), Ok(resolution)) if decision.allowed => Ok(Authorized {
                resolution,
                grant_id,
                actor_id,
                action,
                operation_id,
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
