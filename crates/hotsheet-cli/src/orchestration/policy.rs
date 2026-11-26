//! HS2-YCJDEB: the throughput policy and its dry run over the shared runtime. Since
//! HS2-XHQ1EH the policy holds limits only; what acts on a target asks a person
//! through a permission request. Reads make no request and read no secret; `set` is
//! a person's act.
use std::collections::BTreeSet;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AccessRequest, AuditOutcome, AuditRecord, DeclarationState, IgnoredField, PermissionAction,
    PermissionKey, PlacementStartedStore, PolicyBody, PolicyDenial, PolicyProblem, PolicyQuestion,
    PolicyStore, PolicyVerdict, PolicyView, PolicyWarning, ProjectSites, RetiredAuthority,
    SchedulerStore, SessionLoad, SessionRequest, SetPolicy, TargetAction, retired_authority,
};
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Serialize)]
pub struct PolicyValidation {
    pub valid: bool,
    pub problems: Vec<PolicyProblem>,
    /// HS2-XHQ1EH: retired fields the policy still holds; each is ignored.
    pub ignored: Vec<IgnoredField>,
    /// HS2-SYZB33: entries that have no effect, such as role sessions on a site that
    /// is not its project's home. They never make the policy invalid.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PolicyWarning>,
    /// HS2-XHQ1EH: grant and approval files on this machine, never read as authority.
    pub retired: RetiredAuthority,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyProposal {
    /// A proposed policy; omit to examine the stored one.
    #[serde(default)]
    pub policy: Option<PolicyBody>,
}

#[derive(Debug, Serialize)]
pub struct DryRunAction {
    pub action: TargetAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// `allowed` or `denied` by the policy's limits.
    pub policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_detail: Option<&'static str>,
    /// HS2-XHQ1EH: `not_needed` (reading and observing), `remembered` (a rule allows
    /// it) or `asks` (a person is asked for each operation).
    pub permission: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
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
    /// HS2-SYZB33: entries that have no effect; see [`PolicyValidation::warnings`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PolicyWarning>,
    pub targets: Vec<DryRunTarget>,
}

/// HS2-NAA3HV: `POST …/targets/{target}/make-home`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MakeHome {
    pub expected_revision: String,
}

/// HS2-NAA3HV: the manifest after a re-home, the project's previous home, and the
/// policy revision (rotated only when entries moved).
#[derive(Debug, Serialize)]
pub struct MadeHome {
    #[serde(flatten)]
    pub manifest: ManifestView,
    pub previous_home: String,
    pub policy_revision: Option<String>,
}

/// The policy's limits for one action.
pub(super) enum PolicyAnswer {
    Allowed,
    Denied(PolicyDenial),
}

impl OrchestrationRuntime {
    fn policy_store(&self, owner_id: &str) -> Result<PolicyStore, OrchestrationError> {
        Ok(PolicyStore::new(Path::new(&self.owner(owner_id)?.root)))
    }

    pub(super) fn declared(&self, owner_id: &str) -> Result<BTreeSet<String>, OrchestrationError> {
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
    /// bounds, so an AI actor is refused.
    pub fn set_policy(
        &self,
        owner_id: &str,
        request: SetPolicy,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<PolicyView, OrchestrationError> {
        access::person(actor)?;
        let store = self.policy_store(owner_id)?;
        let stored = store.read()?;
        let before = stored.revision;
        let orchestrating = self.orchestrating(owner_id)?;
        let placing = |body: Option<&PolicyBody>| {
            orchestrating && body.is_some_and(|body| self.placement_on(owner_id, &body.placement))
        };
        let was_on = placing(stored.policy.as_ref());
        let view = store.set(request, Some(&self.declared(owner_id)?), tx)?;
        if view.revision != before {
            // HS2-38R8J2: placement on before or after this change means orchestration
            // was started, so a later pause reads Paused rather than Not Started. Only
            // what the view says depends on it, so a failed write leaves the policy set.
            if was_on || placing(view.policy.as_ref()) {
                let _ = PlacementStartedStore::new(&self.home).record(owner_id, tx.clock.now_ms());
            }
            // HS2-ECWG50: a scheduler following this machine plans under the new policy.
            // HS2-AWB7BX: new limits may give a machine room, so its capacity refusals
            // are asked again rather than waited out.
            let _ = SchedulerStore::new(&self.home).lift_capacity_refusals(owner_id);
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
                    permission_id: None,
                    requested_by: None,
                },
            )?;
        }
        Ok(view)
    }

    /// HS2-NAA3HV: make a site the home of its project at one manifest revision, and
    /// move the policy's per-target entries with the home (the maintainer's default):
    /// those keyed by the old home and by the new one swap, in one policy write right
    /// after. Changing the policy is a person's act, so an AI actor is refused when the
    /// policy names either site; otherwise any actor that may rename a target may do
    /// it. Should the policy write fail, the manifest is put back the same way (the old
    /// home made home again), so neither half lands alone.
    pub fn make_home(
        &self,
        target: &TargetRef,
        request: MakeHome,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<MadeHome, OrchestrationError> {
        let owner_id = target.owner_checkout_id.clone();
        let declared = self.declarations(&owner_id)?;
        let old_home = declared
            .targets
            .iter()
            .find(|declared| declared.target_id == target.target_id)
            .and_then(|declared| declared.site_of.clone());
        let store = self.policy_store(&owner_id)?;
        let policy = store.read()?;
        let moved = old_home.as_deref().and_then(|old| {
            policy
                .policy
                .as_ref()
                .and_then(|body| body.with_home_moved(old, &target.target_id))
        });
        if moved.is_some() {
            access::person(actor)?;
        }
        let owner = self.owner(&owner_id)?;
        let flow = self.flow(&owner)?;
        let observer = RuntimeObserver::new(self, &owner);
        let (view, old_home) = flow.make_home(target, request.expected_revision, &observer, tx)?;
        let mut policy_revision = policy.revision.clone();
        if let Some(body) = moved {
            let written = store.set(
                SetPolicy {
                    expected_revision: policy.revision,
                    policy: body,
                },
                Some(&self.declared(&owner_id)?),
                tx,
            );
            match written {
                Ok(written) => policy_revision = written.revision,
                Err(error) => {
                    let back = TargetRef {
                        owner_checkout_id: owner_id.clone(),
                        target_id: old_home.clone(),
                    };
                    if let Some(revision) = view.revision.clone() {
                        let _ = flow.make_home(&back, revision, &observer, tx);
                    }
                    return Err(error);
                }
            }
            Self::scheduler_wake_at(&self.home);
        }
        self.audit_record(
            &owner_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: Some(target.target_id.clone()),
                action: "target_make_home".into(),
                operation_id: None,
                outcome: AuditOutcome::Changed,
                reason: None,
                permission_id: None,
                requested_by: None,
            },
        )?;
        Ok(MadeHome {
            manifest: view,
            previous_home: old_home,
            policy_revision,
        })
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
        let sites = ProjectSites::active(&self.declarations(owner_id)?.targets);
        Ok(PolicyValidation {
            valid: problems.is_empty(),
            problems,
            ignored: body.ignored_fields(),
            warnings: body.warnings(&sites),
            retired: retired_authority(&self.home),
        })
    }

    /// What a policy's limits would let the orchestrator do to each declared target,
    /// and whether a person would be asked or a remembered rule answers. Writes
    /// nothing, files no permission request and contacts no target.
    pub fn dry_run_policy(
        &self,
        owner_id: &str,
        proposal: PolicyProposal,
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
        let warnings = policy.warnings(&ProjectSites::active(&manifest.targets));
        let rules = self.permission_ledger().rules(owner_id)?;
        // HS2-J3ZF27: a Software project acts on nothing, whatever the policy says.
        let orchestrating = self.orchestrating(owner_id)?;
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
            let entry = policy.listed_target(&declaration.target_id);
            // HS2-N5BDE4: an unlisted project, or one listing no tools, is Auto: the dry
            // run asks about each tool the policy lists and does not turn off.
            let auto_tools: Vec<&str> = policy
                .tools
                .iter()
                .filter(|tool| !tool.is_off())
                .map(|tool| tool.tool.as_str())
                .collect();
            let mut actions = Vec::new();
            for action in TargetAction::ALL {
                let tools: Vec<Option<&str>> = match (action, entry) {
                    (TargetAction::SessionsManage, Some(entry)) if !entry.tools.is_empty() => entry
                        .tools
                        .iter()
                        .map(|tool| Some(tool.tool.as_str()))
                        .collect(),
                    (TargetAction::SessionsManage, _) if !auto_tools.is_empty() => {
                        auto_tools.iter().map(|tool| Some(*tool)).collect()
                    }
                    _ => vec![None],
                };
                for tool in tools {
                    // A start from an idle machine, on the tool's first listed model
                    // and account: the most permissive question this policy can answer.
                    let session = tool.map(|tool| SessionRequest {
                        tool: tool.into(),
                        model: entry
                            .and_then(|entry| entry.tools.iter().find(|scope| scope.tool == tool))
                            .and_then(hotsheet_ticketing::orchestration::ToolScope::start_model),
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
                    let verdict = if !orchestrating && !action.observes() {
                        PolicyVerdict::Denied(PolicyDenial::ProjectSoftware)
                    } else if body.is_none() && action != TargetAction::SessionsManage {
                        // Without a policy, explicit actions have no limits; a person
                        // is asked for them.
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
                        PolicyVerdict::Denied(denial) => ("denied", Some(denial.code())),
                    };
                    let request = AccessRequest {
                        session: session.clone(),
                        ..AccessRequest::new("dry-run", action)
                    };
                    let rule = PermissionAction::for_request(&request).map(|permission| {
                        let key = PermissionKey {
                            target: target.clone(),
                            action: permission,
                            tool: permission
                                .takes_tool()
                                .then(|| tool.map(str::to_owned))
                                .flatten(),
                        };
                        rules.iter().find(|rule| rule.key == key)
                    });
                    let (permission, rule_id) = match rule {
                        None => ("not_needed", None),
                        Some(Some(rule)) => ("remembered", Some(rule.rule_id.clone())),
                        Some(None) => ("asks", None),
                    };
                    actions.push(DryRunAction {
                        action,
                        tool: tool.map(Into::into),
                        policy: answer,
                        policy_detail: detail,
                        automatic: answer == "allowed"
                            && binding.destination.is_some()
                            && permission != "asks"
                            && (body.is_some() || action != TargetAction::SessionsManage),
                        permission,
                        rule_id,
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
                max_sessions: Some(policy.target_cap(&policy.target(&declaration.target_id))),
                machine: policy.machine_limits(&declaration.target_id),
                speculation: policy.speculation_allowed(&declaration.target_id),
                actions,
            });
        }
        Ok(DryRun {
            source,
            valid: problems.is_empty(),
            problems,
            warnings,
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
        // HS2-J3ZF27: a Software project has no orchestration, whatever its kept
        // policy says. A type that cannot be read refuses too.
        if !self.orchestrating(owner_id).unwrap_or(false) {
            return PolicyAnswer::Denied(PolicyDenial::ProjectSoftware);
        }
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
        }
    }
}
