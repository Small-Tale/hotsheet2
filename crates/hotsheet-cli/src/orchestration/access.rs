//! HS2-XHQ1EH: the authorization gate and the audit journal over the shared runtime.
//!
//! Adding a project (declaring the target and binding it) is the consent to read and
//! follow it. Every target action passes `authorize`, which resolves the target freshly
//! and then decides by the kind of action:
//!
//! 1. reading and gate observation never ask;
//! 2. stopping or winding down a session only reduces activity: it never asks and the
//!    policy never denies it;
//! 3. anything else is held to the policy's limits, and then goes ahead when it is a
//!    person's own act, continues a decision already taken, a remembered rule allows
//!    it, or a person allowed this exact operation once. Otherwise a permission
//!    request is filed and shown through the permission bridge, and the call answers
//!    `permission_required` with the request id. Nothing waits for the answer.
//!
//! Every answer is written to the audit journal.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AccessDecision, AccessRequest, Asked, AuditJournal, AuditOutcome, AuditPage, AuditRecord,
    DenialReason, PermissionAction, PermissionAsk, PermissionKey, PermissionLedger, SessionLoad,
    SessionRequest, TargetAction, TargetResolution,
};

use super::*;

/// How often `authorize` resolves again after losing a race with another resolution
/// of the same target.
pub(super) const RESOLVE_ATTEMPTS: usize = 8;

/// Proof that one actor may perform one action now. Later steps report their result
/// with [`OrchestrationRuntime::record_outcome`] under the same operation id; a
/// success uses an Allow Once answer, a failure keeps it for the next attempt.
pub struct Authorized {
    pub resolution: TargetResolution,
    pub actor_id: String,
    pub action: TargetAction,
    pub operation_id: Option<String>,
    /// Why the action may go ahead: `observation`, `reduces_activity`, `person`,
    /// `continuing`, `remembered_rule` or `allowed_once`.
    pub basis: &'static str,
    /// The Allow Once request this action uses once it happens, or the remembered
    /// rule that allowed it.
    pub permission_id: Option<String>,
    /// HS2-ZYM6ZT: the person the actor acted for, recorded with every audit entry.
    pub requested_by: Option<String>,
}

/// One target action as the gate sees it.
#[derive(Debug, Clone)]
pub struct ActionRequest {
    pub action: TargetAction,
    pub session: Option<SessionRequest>,
    /// Required for an action that may ask a person: the request is for this id.
    pub operation_id: Option<String>,
    /// HS2-YCJDEB: sessions already running; a session start without it is denied.
    pub load: Option<SessionLoad>,
    pub provisional: bool,
    pub role_session: bool,
    /// HS2-EBHDSB: this continues a decision already taken (renewing or releasing a
    /// lease the run holds). Policy limits still apply; a person is not asked again.
    pub continuing: bool,
    /// HS2-WN0MXZ: the action is outside what the target configured itself (a command
    /// the orchestration project chose), so no remembered rule covers it: a person
    /// is asked for each operation. Continuing the same request does not ask again.
    pub ask_each_time: bool,
    /// HS2-5JBYPV: an act that cannot be undone (a production deployment). No
    /// remembered rule covers it, and continuing does not skip the question.
    pub irreversible: bool,
    /// HS2-XHQ1EH: the act only lowers activity on the target (cancelling a run and
    /// releasing its claim or queue mark). It never asks and the policy never denies
    /// it; it needs an identified actor and is audited.
    pub reducing: bool,
    /// HS2-ZYM6ZT: the person whose request the actor carries out, recorded in the
    /// audit beside the actor. Attribution only: the gate decides for the actor, so a
    /// request carried out for a person still asks where the actor would.
    pub requested_by: Option<String>,
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
            ask_each_time: false,
            irreversible: false,
            reducing: false,
            requested_by: None,
        }
    }

    pub fn reducing(mut self) -> Self {
        self.reducing = true;
        self
    }

    pub fn continuing(mut self) -> Self {
        self.continuing = true;
        self
    }

    pub fn operation(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = Some(operation_id.into());
        self
    }

    fn access(&self, actor_id: String) -> AccessRequest {
        AccessRequest {
            actor_id,
            action: self.action,
            session: self.session.clone(),
            operation_id: self.operation_id.clone(),
            load: self.load.clone(),
            provisional: self.provisional,
            role_session: self.role_session,
        }
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

/// A person's own act: neither an AI session nor the orchestrator's own loop.
pub(super) fn is_person(actor: Option<&MutationActor>) -> bool {
    actor.is_some_and(|actor| actor.role == hotsheet_ticketing::actor::ActorRole::Human)
}

/// Authority is given by a person at the serving machine, never by the AI it governs.
pub(super) fn person(actor: Option<&MutationActor>) -> Result<(), OrchestrationError> {
    if actor.is_some_and(MutationActor::is_ai) {
        return Err(OrchestrationError::Forbidden(
            "an AI actor cannot change orchestration policy or answer a permission request",
        ));
    }
    Ok(())
}

/// What the gate concluded before it wrote anything.
enum Verdict {
    Allowed(AccessDecision),
    Denied(AccessDecision),
    /// A person must be asked under this key.
    Ask(PermissionKey),
}

impl OrchestrationRuntime {
    pub(super) fn journal(&self, owner_id: &str) -> Result<AuditJournal, OrchestrationError> {
        AuditJournal::new(&self.home, owner_id)
    }

    pub(super) fn permission_ledger(&self) -> PermissionLedger {
        PermissionLedger::new(&self.home)
    }

    pub(super) fn audit_record(
        &self,
        owner_id: &str,
        record: AuditRecord,
    ) -> Result<(), OrchestrationError> {
        let journal = self.journal(owner_id)?;
        if !hotsheet_ticketing::orchestration::coalescible(&record) {
            return journal.append(record).map(|_| ());
        }
        // HS2-SV4SMY: an observation read that repeats the newest entry of its subject
        // within the window is not written again, so a busy scheduling loop does not
        // rotate decisions out of the bounded journal. Decided under the journal lock.
        let check = hotsheet_ticketing::orchestration::RepeatCheck::new(&record);
        journal
            .append_unless(record.clone(), |previous| check.sees(previous))
            .map(|_| ())
    }

    /// A fresh resolution that survives contention on this machine. Every resolution
    /// rewrites the binding's observation, so two of the same target at once (a
    /// follower and a request, or the CLI beside the server) leave the later writer
    /// with a conflict (HS2-WBREDM). That says nothing about the target: resolve
    /// again, a bounded number of times, rather than refuse (HS2-YZBCX5).
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

    /// The decision up to the point where a person would be asked. Pure but for
    /// reading the policy.
    fn verdict(
        &self,
        target: &TargetRef,
        person_acts: bool,
        action: &ActionRequest,
        request: &AccessRequest,
    ) -> Verdict {
        if request.action.observes() {
            return Verdict::Allowed(AccessDecision::allowed("observation"));
        }
        // HS2-48H57C: reducing activity is never asked and never denied by the policy.
        if request.reduces() || action.reducing {
            return Verdict::Allowed(AccessDecision::allowed("reduces_activity"));
        }
        if let policy::PolicyAnswer::Denied(denial) =
            self.policy_answer(&target.owner_checkout_id, request, &target.target_id)
        {
            return Verdict::Denied(AccessDecision {
                detail: Some(denial.code().into()),
                ..AccessDecision::denied(DenialReason::PolicyDenied)
            });
        }
        if action.continuing && !action.irreversible {
            return Verdict::Allowed(AccessDecision::allowed("continuing"));
        }
        // A person's own act needs no further approval; it stays audited.
        if person_acts {
            return Verdict::Allowed(AccessDecision::allowed("person"));
        }
        let Some(permission) = PermissionAction::for_request(request) else {
            return Verdict::Allowed(AccessDecision::allowed("observation"));
        };
        Verdict::Ask(PermissionKey {
            target: target.clone(),
            action: permission,
            tool: permission
                .takes_tool()
                .then(|| request.session.as_ref().map(|session| session.tool.clone()))
                .flatten(),
        })
    }

    /// Side-effect-free answer against the cached binding: what `authorize` would say
    /// if the target is still what this machine last reviewed. It files and uses no
    /// permission request and writes no audit entry. Never authority.
    pub fn access(
        &self,
        target: &TargetRef,
        request: &AccessRequest,
        person_acts: bool,
        tx: &mut Transaction<'_>,
    ) -> Result<AccessDecision, OrchestrationError> {
        target.validate()?;
        if let Some(session) = &request.session {
            session.validate()?;
        }
        let binding = Self::binding_at(&self.home, target)?;
        if binding.destination.is_none() {
            return Ok(AccessDecision::denied(DenialReason::TargetUnavailable));
        }
        let action = ActionRequest {
            session: request.session.clone(),
            operation_id: request.operation_id.clone(),
            load: request.load.clone(),
            provisional: request.provisional,
            role_session: request.role_session,
            ..ActionRequest::new(request.action)
        };
        let key = match self.verdict(target, person_acts, &action, request) {
            Verdict::Allowed(decision) | Verdict::Denied(decision) => return Ok(decision),
            Verdict::Ask(key) => key,
        };
        let Some(operation_id) = &request.operation_id else {
            // Without an operation only a remembered rule can answer.
            return Ok(match self.remembered(&key)? {
                Some(rule_id) => AccessDecision {
                    rule_id: Some(rule_id),
                    ..AccessDecision::allowed("remembered_rule")
                },
                None => AccessDecision::denied(DenialReason::PermissionRequired),
            });
        };
        let asked = self.permission_ledger().peek(
            &PermissionAsk {
                key,
                model: request.session.as_ref().and_then(|s| s.model.clone()),
                operation_id: operation_id.clone(),
                actor_id: request.actor_id.clone(),
                each_time: each_time(&action),
            },
            tx.clock.now_ms(),
        )?;
        Ok(decision_of(asked))
    }

    pub(super) fn remembered(
        &self,
        key: &PermissionKey,
    ) -> Result<Option<String>, OrchestrationError> {
        Ok(self
            .permission_ledger()
            .rules(&key.target.owner_checkout_id)?
            .into_iter()
            .find(|rule| &rule.key == key)
            .map(|rule| rule.rule_id))
    }

    /// The one gate every target action passes: fresh resolution, then the action's
    /// kind, the policy's limits and the permission request, with an audit entry for
    /// whichever answer results. Anything not provably allowed is refused.
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
        target.validate()?;
        // HS2-ZYM6ZT: attribution only; it is checked here and never decides anything.
        let requested_by = action.requested_by.clone();
        if let Some(person) = &requested_by {
            super::tickets::identifier(person, "bounded requested_by required")?;
        }
        let request = action.access(actor_id.clone());
        if let Some(session) = &request.session {
            session.validate()?;
        }
        // An unknown owner, tombstoned target or failed probe is a denial, not an error
        // the caller could mistake for "try the action anyway".
        let resolution = self.resolve_contended(target, tx);
        let usable = resolution
            .as_ref()
            .is_ok_and(|resolution| resolution.usable && resolution.binding.destination.is_some());
        let decision = if usable {
            match self.verdict(target, is_person(actor), &action, &request) {
                Verdict::Allowed(decision) | Verdict::Denied(decision) => decision,
                Verdict::Ask(key) => self.ask_person(target, key, &action, &request, tx)?,
            }
        } else {
            AccessDecision::denied(DenialReason::TargetUnavailable)
        };
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
                // The specific policy limit, or why no person was asked.
                reason: decision
                    .detail
                    .clone()
                    .or_else(|| decision.reason.map(|reason| reason.code().into()))
                    .or_else(|| decision.basis.clone()),
                permission_id: decision
                    .request_id
                    .clone()
                    .or_else(|| decision.rule_id.clone()),
                requested_by: requested_by.clone(),
            },
        )?;
        match (decision.allowed, resolution) {
            (true, Ok(resolution)) => Ok(Authorized {
                resolution,
                actor_id,
                action: request.action,
                operation_id: request.operation_id,
                basis: basis_code(decision.basis.as_deref()),
                permission_id: decision.request_id.or(decision.rule_id),
                requested_by,
            }),
            _ => Err(match (decision.reason, decision.request_id) {
                (Some(DenialReason::PermissionRequired), Some(request_id)) => {
                    OrchestrationError::PermissionRequired { request_id }
                }
                (reason, _) => {
                    OrchestrationError::Denied(reason.unwrap_or(DenialReason::TargetUnavailable))
                }
            }),
        }
    }

    /// Ask the permission ledger, and show a waiting request through the bridge.
    fn ask_person(
        &self,
        target: &TargetRef,
        key: PermissionKey,
        action: &ActionRequest,
        request: &AccessRequest,
        tx: &mut Transaction<'_>,
    ) -> Result<AccessDecision, OrchestrationError> {
        let Some(operation_id) = &request.operation_id else {
            if let Some(rule_id) = self.remembered(&key)?.filter(|_| !each_time(action)) {
                return Ok(AccessDecision {
                    rule_id: Some(rule_id),
                    ..AccessDecision::allowed("remembered_rule")
                });
            }
            return Err(OrchestrationError::Invalid(
                "an operation id is required for an action that asks a person",
            ));
        };
        let ask = PermissionAsk {
            key,
            model: request.session.as_ref().and_then(|s| s.model.clone()),
            operation_id: operation_id.clone(),
            actor_id: request.actor_id.clone(),
            each_time: each_time(action),
        };
        let asked = self.permission_ledger().ask(&ask, tx)?;
        if let Asked::Pending { request, filed } = &asked {
            // Shown on every ask: a bridge that restarted shows it again.
            self.show_permission(target, request);
            // HS2-E16FWJ: a request filed here (perhaps by a headless CLI) reaches the
            // servers on this machine without waiting for a client read.
            if *filed {
                self.permission_signal();
            }
        }
        Ok(decision_of(Some(asked)))
    }

    /// HS2-XHQ1EH: the permission half of the gate for a second decision inside an
    /// action that already passed `authorize` (building ahead during a dispatch),
    /// without resolving the target again. `Ok(Some(id))` names the Allow Once answer
    /// to use once the action happens; `Ok(None)` needs nothing used. The caller
    /// audits the answer.
    pub(super) fn permission_gate(
        &self,
        target: &TargetRef,
        actor: Option<&MutationActor>,
        action: &ActionRequest,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<String>, OrchestrationError> {
        let Some(actor_id) = actor.and_then(|actor| actor.id.clone()) else {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required for a target action",
            ));
        };
        let request = action.access(actor_id);
        let decision = match self.verdict(target, is_person(actor), action, &request) {
            Verdict::Allowed(decision) | Verdict::Denied(decision) => decision,
            Verdict::Ask(key) => self.ask_person(target, key, action, &request, tx)?,
        };
        if decision.allowed {
            return Ok(decision
                .request_id
                .filter(|_| decision.basis.as_deref() == Some("allowed_once")));
        }
        Err(match (decision.reason, decision.request_id) {
            (Some(DenialReason::PermissionRequired), Some(request_id)) => {
                OrchestrationError::PermissionRequired { request_id }
            }
            (reason, _) => {
                OrchestrationError::Denied(reason.unwrap_or(DenialReason::PermissionRequired))
            }
        })
    }

    /// Report how an authorized action ended, under the same operation id. A success
    /// uses the Allow Once answer that allowed it (HS2-8NJVXC, HS2-RTX8RJ); a failure
    /// leaves it for the next attempt.
    pub fn record_outcome(
        &self,
        target: &TargetRef,
        authorized: &Authorized,
        succeeded: bool,
        reason: Option<&'static str>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        if succeeded {
            self.permission_used(authorized, tx)?;
        }
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
                permission_id: authorized.permission_id.clone(),
                requested_by: authorized.requested_by.clone(),
            },
        )
    }

    /// The action an Allow Once answer allowed happened: that answer is used up.
    /// Nothing for any other basis. Idempotent.
    pub fn permission_used(
        &self,
        authorized: &Authorized,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        if authorized.basis != "allowed_once" {
            return Ok(());
        }
        if let Some(request_id) = &authorized.permission_id {
            self.permission_ledger().used(request_id, tx)?;
        }
        Ok(())
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

fn each_time(action: &ActionRequest) -> bool {
    action.ask_each_time || action.irreversible
}

fn basis_code(basis: Option<&str>) -> &'static str {
    match basis {
        Some("observation") => "observation",
        Some("reduces_activity") => "reduces_activity",
        Some("person") => "person",
        Some("continuing") => "continuing",
        Some("remembered_rule") => "remembered_rule",
        Some("allowed_once") => "allowed_once",
        _ => "unknown",
    }
}

/// The ledger's answer in the gate's vocabulary.
fn decision_of(asked: Option<Asked>) -> AccessDecision {
    match asked {
        Some(Asked::Remembered(rule)) => AccessDecision {
            rule_id: Some(rule.rule_id),
            ..AccessDecision::allowed("remembered_rule")
        },
        Some(Asked::AllowedOnce(request)) => AccessDecision {
            request_id: Some(request.request_id),
            ..AccessDecision::allowed("allowed_once")
        },
        Some(Asked::Pending { request, .. }) => AccessDecision {
            request_id: Some(request.request_id),
            ..AccessDecision::denied(DenialReason::PermissionRequired)
        },
        Some(Asked::Denied(request)) => AccessDecision {
            request_id: Some(request.request_id),
            ..AccessDecision::denied(DenialReason::PermissionDenied)
        },
        // Nothing asked yet: the action would ask.
        None => AccessDecision::denied(DenialReason::PermissionRequired),
    }
}
