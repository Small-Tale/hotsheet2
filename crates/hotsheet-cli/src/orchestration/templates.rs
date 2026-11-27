//! HS2-A4RVYA: reusable plan templates over the shared runtime.
//!
//! Templates are checked in beside the plan (`.hotsheet2/orchestration-templates.json`)
//! and change only the orchestration project, so defining, editing, retiring and
//! saving one never asks anybody. Applying one is the maintainer's decision "the
//! planner AI adapts the template each run": it opens one new goal ticket whose
//! details carry the rendered brief, then hands that goal to the existing planner
//! flow (Propose Plan while the plan is empty, Update Plan once it has steps). What
//! the planner proposes is the preview a person reviews, through the plan as it
//! already is reviewed.
//!
//! [`OrchestrationRuntime::template_apply`] is the one way to apply a template, for a
//! person, an AI session or a trigger (HS2-KXKA7X): "apply template T with inputs I as
//! actor A under operation id O". It is idempotent by the operation id: the goal's id
//! and the planner's proposal id are recorded before they are acted on, so a repeat,
//! even after a crash, creates no second goal and starts no second planner.
use std::collections::{BTreeMap, BTreeSet};

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::ops;
use hotsheet_ticketing::orchestration::{
    AcceptWait, ApplyDecision, ApplyGoal, ApplyState, AuditOutcome, AuditRecord, DeclarationState,
    DenialReason, GateRequirement, InputKind, MAX_APPLY_OPERATION_ID, PermissionAction,
    PermissionKey, PlanHold, Prerequisite, ReadinessError, StepAfter, StepTicket,
    TEMPLATE_GOAL_TAG, TargetAction, Template, TemplateApply, TemplateApplyStore, TemplateError,
    TemplateGoal, TemplateInput, TemplateProblem, TemplateState, TemplateStep, TemplateStore,
    TemplatesFile, TicketKey, decide_apply, held_template_steps, render_brief, resolve_inputs,
    validate_template,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::access::{ActionRequest, Authorized, is_person};
use super::plan_update::UpdatePlan;
use super::planning::{ProposeOutcome, ProposePlan};
use super::tickets::{OrchestrationTicket, identifier};
use super::*;

/// The newest applies the list answer carries.
pub const MAX_LISTED_APPLIES: usize = 20;

/// Names one template.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateRef {
    pub template_id: String,
}

/// Add or replace templates, merged by `template_id` into the checked-in file. A
/// template without an id gets a fresh one. `expected_revision` refuses the write
/// when the file changed since it was read (`template_conflict`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTemplates {
    pub templates: Vec<Template>,
    #[serde(default)]
    pub expected_revision: Option<String>,
}

/// Check one template without writing it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateTemplate {
    pub template: Template,
}

/// Save a goal's current plan steps as a template (Save a Goal as Template…).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveGoalTemplate {
    pub goal: OrchestrationTicket,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Make each project the steps name an input (defaulting to that project), so the
    /// template can be applied to other projects.
    #[serde(default)]
    pub targets_as_inputs: bool,
}

/// What applying would do, before anything is applied.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewTemplate {
    pub template_id: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub choices: BTreeMap<String, StepTicket>,
}

/// Apply a template: the API a person, an AI session and a trigger share.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyTemplate {
    pub template_id: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    /// Steps the person switched from search-first to create (or back).
    #[serde(default)]
    pub choices: BTreeMap<String, StepTicket>,
    /// The AI tool a new planner runs in; an open planner keeps its own.
    pub tool: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Retained by the caller: the same id is the same apply.
    pub operation_id: String,
}

/// Names one apply.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyRef {
    pub operation_id: String,
}

/// HS2-36XDDX: a person accepts the plan the planner proposed for an apply's goal.
/// HS2-0YHA2S: or, with `all` and no operation id, every template plan held now (the
/// Orchestration header's Accept Plan).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptTemplate {
    #[serde(default)]
    pub operation_id: String,
    #[serde(default)]
    pub all: bool,
}

/// HS2-0YHA2S: an accept of every template plan held now: each apply's accept as
/// [`TemplateAcceptView`], and every step released, ordered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateAcceptAllView {
    /// `accepted` when steps were released, `existing` when nothing was held.
    pub outcome: String,
    pub accepted: Vec<TemplateAcceptView>,
    pub released: Vec<String>,
}

/// HS2-36XDDX: an accept's answer: the apply as recorded now and the steps this
/// accept released (empty when they were accepted before).
///
/// HS2-38MFXT: the apply's fields are present when this machine has its record; the
/// checked-in `hold` when the plan carries one (an accept on another machine than the
/// one that applied has only the hold).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateAcceptView {
    #[serde(flatten)]
    pub apply: Option<TemplateApply>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<PlanHold>,
    pub released: Vec<String>,
    /// `accepted` when this accept released steps, `existing` when every planned
    /// step was accepted before.
    pub outcome: String,
}

/// HS2-38MFXT: what one accept found and changed.
struct AcceptOutcome {
    apply: Option<TemplateApply>,
    hold: Option<PlanHold>,
    /// The goal's live steps this accept released.
    released: Vec<String>,
    /// The goal's live steps.
    steps: Vec<String>,
}

/// A recent apply, as the list shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplySummary {
    pub operation_id: String,
    pub template_id: String,
    pub template_name: String,
    pub template_version: u32,
    pub state: ApplyState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<ApplyGoal>,
    /// HS2-36XDDX: the template as applied waits for an accept.
    pub wait_for_accept: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at_ms: Option<u64>,
    pub created_at_ms: u64,
}

/// The checked-in templates and this machine's recent applies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateList {
    /// The file's revision; absent while nothing is checked in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub templates: Vec<Template>,
    /// Newest first, at most [`MAX_LISTED_APPLIES`].
    pub applies: Vec<ApplySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateValidation {
    pub valid: bool,
    pub problems: Vec<TemplateProblem>,
}

/// Whether one kind of action will ask a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessNeed {
    /// It is the applying person's own act.
    Person,
    /// A remembered rule allows it.
    Remembered,
    /// It asks through the permission card.
    Asks,
}

/// One project the brief names, and what will ask there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewTarget {
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The project is declared and active.
    pub declared: bool,
    /// The steps there.
    pub steps: Vec<String>,
    /// Applying: a person's own act, or a request an AI session waits on.
    pub apply: AccessNeed,
    /// The planner's ticket writes there (the planner is an AI session).
    pub ticket_writes: AccessNeed,
    /// HS2-K3ESZR: whether the test profiles the steps there name were checked
    /// against the profiles the project offers. Absent when no step there names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<ProfilesCheck>,
}

/// HS2-K3ESZR: what a preview read of one project's test profiles found.
enum ProfilesSeen {
    /// The profile file is valid (or absent, offering none): these names run.
    Offered(BTreeSet<String>),
    /// The profile file has problems, so no profile there runs by name.
    Invalid,
    /// The read failed or was refused.
    Unverified,
}

/// HS2-K3ESZR: how a project's test profiles were checked for the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilesCheck {
    /// The project's profiles were read; any profile it does not offer is a problem.
    Checked,
    /// They could not be read now (the project is offline, or the read was refused).
    /// Nothing is reported; the planner and the validation run check them later.
    Unverified,
}

/// The pre-run summary: the inputs as resolved, the problems that keep Apply
/// disabled, the steps as the brief describes them, the projects and what asks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplatePreview {
    pub template: Template,
    pub inputs: BTreeMap<String, Value>,
    pub problems: Vec<TemplateProblem>,
    pub goal_title: String,
    pub steps: Vec<hotsheet_ticketing::orchestration::BriefStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
    pub targets: Vec<PreviewTarget>,
    /// `propose` while the live plan is empty, `update` once it has steps.
    pub planning: String,
    /// Whether placement starts planned work by itself; otherwise the proposed plan
    /// waits for Start Orchestration.
    pub placement_automatic: bool,
    /// HS2-36XDDX: the template's Wait for Accept: with placement on, the planned
    /// steps wait for a person to accept the proposed plan.
    pub wait_for_accept: bool,
}

/// Where one apply stands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateApplyView {
    #[serde(flatten)]
    pub apply: TemplateApply,
    /// `applied` when this call did something, `existing` when it answered with a
    /// finished apply as recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

fn template_error(error: TemplateError) -> OrchestrationError {
    OrchestrationError::Template(error)
}

impl OrchestrationRuntime {
    fn template_store(&self, owner: &Checkout) -> TemplateStore {
        TemplateStore::new(std::path::Path::new(&owner.root))
    }

    fn template_applies(&self, owner_id: &str) -> Result<TemplateApplyStore, OrchestrationError> {
        TemplateApplyStore::new(&self.home, owner_id)
    }

    fn template_audit(
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

    /// The templates as checked in, and this machine's recent applies. Reads only.
    pub fn template_list(&self, owner_id: &str) -> Result<TemplateList, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let document = self.template_store(&owner).read()?;
        let mut applies = self.template_applies(owner_id)?.applies()?;
        applies.sort_by(|a, b| {
            b.created_at_ms
                .cmp(&a.created_at_ms)
                .then_with(|| b.operation_id.cmp(&a.operation_id))
        });
        applies.truncate(MAX_LISTED_APPLIES);
        // HS2-38MFXT: an accept made on another machine is on the checked-in hold.
        let holds = self.plan().holds(owner_id).unwrap_or_default();
        let accepted_at = |apply: &TemplateApply| {
            holds
                .iter()
                .find(|hold| hold.operation_id == apply.operation_id)
                .map_or(apply.accepted_at_ms, |hold| hold.accepted_at_ms)
        };
        Ok(TemplateList {
            revision: document.as_ref().map(|document| document.revision.clone()),
            templates: document
                .map(|document| document.templates)
                .unwrap_or_default(),
            applies: applies
                .into_iter()
                .map(|apply| ApplySummary {
                    accepted_at_ms: accepted_at(&apply),
                    template_name: apply.snapshot.name.clone(),
                    template_version: apply.snapshot.version,
                    operation_id: apply.operation_id,
                    template_id: apply.template_id,
                    state: apply.state,
                    wait_for_accept: apply.snapshot.wait_for_accept,
                    goal: apply.goal,
                    created_at_ms: apply.created_at_ms,
                })
                .collect(),
        })
    }

    /// One template by id (`template_not_found`).
    pub fn template_show(
        &self,
        owner_id: &str,
        reference: &TemplateRef,
    ) -> Result<Template, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        self.template_store(&owner)
            .templates()?
            .into_iter()
            .find(|template| template.template_id == reference.template_id)
            .ok_or(template_error(TemplateError::NotFound))
    }

    /// Check a definition without writing it: the validator's problems, plus a
    /// literal project id the orchestration project does not declare.
    pub fn template_validate(
        &self,
        owner_id: &str,
        request: &ValidateTemplate,
    ) -> Result<TemplateValidation, OrchestrationError> {
        let mut problems = validate_template(&request.template);
        problems.extend(self.undeclared_literals(owner_id, &request.template)?);
        problems.sort();
        Ok(TemplateValidation {
            valid: problems.is_empty(),
            problems,
        })
    }

    fn active_targets(
        &self,
        owner_id: &str,
    ) -> Result<BTreeMap<String, String>, OrchestrationError> {
        Ok(self
            .declarations(owner_id)?
            .targets
            .into_iter()
            .filter(|target| target.state == DeclarationState::Active)
            .map(|target| (target.target_id, target.label))
            .collect())
    }

    fn undeclared_literals(
        &self,
        owner_id: &str,
        template: &Template,
    ) -> Result<Vec<TemplateProblem>, OrchestrationError> {
        let active = self.active_targets(owner_id)?;
        Ok(template
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| {
                hotsheet_ticketing::orchestration::reference(&step.target).is_none()
                    && !active.contains_key(&step.target)
            })
            .map(|(n, _)| TemplateProblem {
                code: "target_unknown".into(),
                at: format!("steps[{n}].target"),
            })
            .collect())
    }

    /// Add or replace templates, merged by template id. A changed definition gets the
    /// next version; the same definition changes nothing. Only the orchestration
    /// project changes, so this never asks. Refused as `template_invalid` (a problem
    /// in a definition), `template_conflict` (one id twice with different
    /// definitions, or a stale `expected_revision`) or `capacity_reached`. Audited
    /// as `template_set`.
    pub fn template_set(
        &self,
        owner_id: &str,
        request: SetTemplates,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TemplatesFile, OrchestrationError> {
        if actor.and_then(|actor| actor.id.as_ref()).is_none() {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to change templates",
            ));
        }
        let owner = self.owner(owner_id)?;
        let store = self.template_store(&owner);
        let _lock = store.lock()?;
        let previous = store.read()?;
        if let Some(expected) = &request.expected_revision
            && previous.as_ref().map(|document| &document.revision) != Some(expected)
        {
            return Err(template_error(TemplateError::Conflict));
        }
        let mut templates: BTreeMap<String, Template> = previous
            .as_ref()
            .map(|document| document.templates.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|template| (template.template_id.clone(), template))
            .collect();
        let mut seen: BTreeMap<String, Template> = BTreeMap::new();
        let now = tx.clock.now_ms();
        for mut template in request.templates {
            if template.template_id.is_empty() {
                template.template_id =
                    hotsheet_ticketing::mint_ulid(tx.clock, &mut *tx.rng).to_string();
            }
            if template
                .template_id
                .parse::<ulid::Ulid>()
                .map(|id| id.to_string())
                != Ok(template.template_id.clone())
            {
                return Err(OrchestrationError::Invalid(
                    "a template id is a canonical ULID",
                ));
            }
            if !validate_template(&template).is_empty() {
                return Err(template_error(TemplateError::Invalid));
            }
            if let Some(other) = seen.get(&template.template_id) {
                if !other.same_definition(&template) {
                    return Err(template_error(TemplateError::Conflict));
                }
                continue;
            }
            seen.insert(template.template_id.clone(), template.clone());
            let next = match templates.get(&template.template_id) {
                Some(known) if known.same_definition(&template) => known.clone(),
                Some(known) => Template {
                    version: known.version + 1,
                    created_at_ms: known.created_at_ms,
                    updated_at_ms: now,
                    updated_by: actor.and_then(|actor| actor.id.clone()),
                    ..template
                },
                None => Template {
                    version: 1,
                    created_at_ms: now,
                    updated_at_ms: now,
                    updated_by: actor.and_then(|actor| actor.id.clone()),
                    ..template
                },
            };
            templates.insert(next.template_id.clone(), next);
        }
        let document =
            store.write_locked(previous.as_ref(), templates.into_values().collect(), tx)?;
        let changed =
            previous.as_ref().map(|previous| &previous.revision) != Some(&document.revision);
        self.template_audit(
            owner_id,
            actor,
            "template_set",
            Some(&document.revision),
            (
                if changed {
                    AuditOutcome::Changed
                } else {
                    AuditOutcome::Succeeded
                },
                None,
            ),
            tx,
        )?;
        Ok(document)
    }

    /// Retire a template: it can no longer be applied, its id is never reused, and
    /// work applied from it is unchanged. Retiring again changes nothing.
    pub fn template_retire(
        &self,
        owner_id: &str,
        reference: &TemplateRef,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Template, OrchestrationError> {
        let mut template = self.template_show(owner_id, reference)?;
        template.state = TemplateState::Retired;
        let document = self.template_set(
            owner_id,
            SetTemplates {
                templates: vec![template],
                expected_revision: None,
            },
            actor,
            tx,
        )?;
        document
            .templates
            .into_iter()
            .find(|template| template.template_id == reference.template_id)
            .ok_or(OrchestrationError::Corrupt)
    }

    /// Save a Goal as Template…: the goal's title and details and its current plan
    /// steps become a new template. Each step searches first; its prerequisites on
    /// other steps of the goal become `after` with their gates. With
    /// `targets_as_inputs`, each project becomes a `target` input defaulting to it.
    pub fn template_save_goal(
        &self,
        owner_id: &str,
        request: SaveGoalTemplate,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Template, OrchestrationError> {
        let (store, id, connection_id) = self.orchestration_store(owner_id, &request.goal)?;
        let goal = store
            .read_ticket(&id)
            .map_err(|_| template_error(TemplateError::GoalEmpty))?;
        let native = id.to_string();
        let mut steps: Vec<_> = self
            .plan()
            .records(owner_id)?
            .into_iter()
            .map(|record| record.step)
            .filter(|step| {
                step.ticket.connection_id == connection_id && step.ticket.native_id == native
            })
            .collect();
        if steps.is_empty() {
            return Err(template_error(TemplateError::GoalEmpty));
        }
        steps.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then_with(|| a.step_id.cmp(&b.step_id))
        });
        let (_, targets) = self.timeline_targets(owner_id)?;
        let keys: BTreeMap<TicketKey, String> = steps
            .iter()
            .enumerate()
            .map(|(n, step)| {
                (
                    hotsheet_ticketing::orchestration::ticket_key(&step.target),
                    format!("step-{}", n + 1),
                )
            })
            .collect();
        let mut target_inputs: BTreeMap<String, String> = BTreeMap::new();
        let mut inputs: Vec<TemplateInput> = Vec::new();
        if request.targets_as_inputs {
            for step in &steps {
                if target_inputs.contains_key(&step.target.target_id) {
                    continue;
                }
                let key = format!("project-{}", target_inputs.len() + 1);
                let label = targets
                    .iter()
                    .find(|target| target.target_id == step.target.target_id)
                    .map_or_else(|| "Project".to_owned(), |target| target.label.clone());
                inputs.push(TemplateInput {
                    key: key.clone(),
                    label: format!("{label} Project").chars().take(200).collect(),
                    kind: InputKind::Target,
                    required: true,
                    default: Some(Value::String(step.target.target_id.clone())),
                    choices: Vec::new(),
                    max_len: None,
                    hint: None,
                });
                target_inputs.insert(step.target.target_id.clone(), key);
            }
        }
        let template_steps: Vec<TemplateStep> = steps
            .iter()
            .map(|step| {
                let key = hotsheet_ticketing::orchestration::ticket_key(&step.target);
                let title = targets
                    .iter()
                    .find(|target| target.target_id == step.target.target_id)
                    .and_then(|target| self.local_ticket_label(target, &key))
                    .and_then(|label| label.title.or(label.slug))
                    .unwrap_or_else(|| format!("Work on {}", step.target.native_id));
                TemplateStep {
                    key: keys[&key].clone(),
                    target: target_inputs.get(&step.target.target_id).map_or_else(
                        || step.target.target_id.clone(),
                        |input| format!("{{{{{input}}}}}"),
                    ),
                    title: sanitize_line(&title),
                    details: None,
                    category: None,
                    priority: None,
                    ticket: StepTicket::Search,
                    after: step
                        .prerequisites
                        .iter()
                        .filter_map(|prerequisite| match prerequisite {
                            Prerequisite::Ticket { gates, .. } => prerequisite
                                .ticket_key()
                                .and_then(|other| keys.get(&other))
                                .map(|other| StepAfter {
                                    step: other.clone(),
                                    gates: Some(
                                        match gates {
                                            GateRequirement::None => "none",
                                            GateRequirement::Ci => "ci",
                                            GateRequirement::Deployment => "deployment",
                                            GateRequirement::Review => "review",
                                        }
                                        .into(),
                                    ),
                                }),
                            Prerequisite::Validation { .. } => None,
                        })
                        .collect(),
                    validation: Vec::new(),
                    tool: step.tool.clone(),
                    when: None,
                }
            })
            .collect();
        let template = Template {
            template_id: String::new(),
            name: request.name,
            description: request.description.unwrap_or_default(),
            state: TemplateState::Active,
            version: 0,
            inputs,
            goal: TemplateGoal {
                title: sanitize_line(&goal.title),
                details: Some(goal.details.chars().take(4000).collect::<String>())
                    .filter(|details| !details.trim().is_empty() && !details.contains("{{")),
                tags: Vec::new(),
            },
            steps: template_steps,
            instructions: None,
            // HS2-36XDDX: a saved goal's template waits for an accept, as any new one.
            wait_for_accept: true,
            created_at_ms: 0,
            updated_at_ms: 0,
            updated_by: None,
        };
        let before: BTreeSet<String> = self
            .template_list(owner_id)?
            .templates
            .into_iter()
            .map(|template| template.template_id)
            .collect();
        let document = self.template_set(
            owner_id,
            SetTemplates {
                templates: vec![template],
                expected_revision: None,
            },
            actor,
            tx,
        )?;
        document
            .templates
            .into_iter()
            .find(|template| !before.contains(&template.template_id))
            .ok_or(OrchestrationError::Corrupt)
    }

    fn active_template(
        &self,
        owner: &Checkout,
        template_id: &str,
    ) -> Result<Template, OrchestrationError> {
        let template = self
            .template_store(owner)
            .templates()?
            .into_iter()
            .find(|template| template.template_id == template_id)
            .ok_or(template_error(TemplateError::NotFound))?;
        if template.state == TemplateState::Retired {
            return Err(template_error(TemplateError::Retired));
        }
        Ok(template)
    }

    /// What the planner's ticket writes in a project will need.
    fn ticket_writes_need(&self, owner_id: &str, target_id: &str) -> AccessNeed {
        let key = PermissionKey {
            target: TargetRef {
                owner_checkout_id: owner_id.into(),
                target_id: target_id.into(),
            },
            action: PermissionAction::TicketsWrite,
            tool: None,
        };
        match self.remembered(&key) {
            Ok(Some(_)) => AccessNeed::Remembered,
            _ => AccessNeed::Asks,
        }
    }

    /// The pre-run summary of an apply. Writes nothing and asks nobody. HS2-K3ESZR:
    /// the test profiles the steps name are read from each project through the gate
    /// as an observation (audited, never asking), so a profile a project does not
    /// offer is a problem here rather than something only the planner finds.
    pub fn template_preview(
        &self,
        owner_id: &str,
        request: &PreviewTemplate,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TemplatePreview, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let template = self.active_template(&owner, &request.template_id)?;
        let (inputs, mut problems) = resolve_inputs(&template, &request.inputs);
        let active = self.active_targets(owner_id)?;
        let brief = render_brief(&template, &inputs, &request.choices, "preview", &active);
        for (n, step) in brief.steps.iter().enumerate() {
            if !active.contains_key(&step.target_id) && !step.target_id.contains("{{") {
                problems.push(TemplateProblem {
                    code: "target_unknown".into(),
                    at: format!("steps[{n}].target"),
                });
            }
        }
        for key in request.choices.keys() {
            if template.step(key).is_none() {
                problems.push(TemplateProblem {
                    code: "choice_unknown".into(),
                    at: format!("choices.{key}"),
                });
            }
        }
        let profiles = self.preview_profiles(owner_id, &brief.steps, &active, actor, tx);
        for (n, step) in brief.steps.iter().enumerate() {
            match profiles.get(&step.target_id) {
                Some(ProfilesSeen::Offered(offered)) => {
                    for (m, profile) in step.validation.iter().enumerate() {
                        if !offered.contains(profile) {
                            problems.push(TemplateProblem {
                                code: "validation_profile_unknown".into(),
                                at: format!("steps[{n}].validation[{m}]"),
                            });
                        }
                    }
                }
                Some(ProfilesSeen::Invalid) if !step.validation.is_empty() => {
                    problems.push(TemplateProblem {
                        code: "validation_profiles_invalid".into(),
                        at: format!("steps[{n}].validation"),
                    });
                }
                _ => {}
            }
        }
        problems.sort();
        problems.dedup();
        let person = is_person(actor);
        let targets = brief
            .targets()
            .into_iter()
            .map(|target_id| {
                let need = self.ticket_writes_need(owner_id, &target_id);
                PreviewTarget {
                    profiles: profiles.get(&target_id).map(|seen| match seen {
                        ProfilesSeen::Unverified => ProfilesCheck::Unverified,
                        _ => ProfilesCheck::Checked,
                    }),
                    label: active.get(&target_id).cloned(),
                    declared: active.contains_key(&target_id),
                    steps: brief
                        .steps
                        .iter()
                        .filter(|step| step.target_id == target_id)
                        .map(|step| step.key.clone())
                        .collect(),
                    apply: if person { AccessNeed::Person } else { need },
                    ticket_writes: need,
                    target_id,
                }
            })
            .collect();
        let placement_automatic = self
            .policy(owner_id)?
            .policy
            .is_some_and(|policy| self.placement_on(owner_id, &policy.placement));
        Ok(TemplatePreview {
            planning: if self.plan().records(owner_id)?.is_empty() {
                "propose".into()
            } else {
                "update".into()
            },
            goal_title: brief.goal_title,
            steps: brief.steps,
            skipped: brief.skipped,
            wait_for_accept: template.wait_for_accept,
            template,
            inputs,
            problems,
            targets,
            placement_automatic,
        })
    }

    /// HS2-K3ESZR: the test profiles each declared project offers, read once per
    /// project whose steps name a profile. A project that cannot be read now is
    /// `Unverified`, never a problem: the preview stays usable while it is offline.
    fn preview_profiles(
        &self,
        owner_id: &str,
        steps: &[hotsheet_ticketing::orchestration::BriefStep],
        active: &BTreeMap<String, String>,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> BTreeMap<String, ProfilesSeen> {
        let named: BTreeSet<&String> = steps
            .iter()
            .filter(|step| !step.validation.is_empty() && active.contains_key(&step.target_id))
            .map(|step| &step.target_id)
            .collect();
        named
            .into_iter()
            .map(|target_id| {
                let read = self.validation_profiles(
                    owner_id,
                    TargetProfiles {
                        target_id: target_id.clone(),
                    },
                    actor,
                    tx,
                );
                let seen = match read {
                    Ok(check) if check.valid => ProfilesSeen::Offered(
                        check
                            .profiles
                            .into_iter()
                            .map(|profile| profile.name)
                            .collect(),
                    ),
                    Ok(_) => ProfilesSeen::Invalid,
                    Err(_) => ProfilesSeen::Unverified,
                };
                (target_id.clone(), seen)
            })
            .collect()
    }

    /// HS2-36XDDX: the live plan steps held for a person's accept of their template's
    /// proposed plan, by step id, with the apply to accept. Reads only.
    ///
    /// HS2-38MFXT: a hold checked in with the plan holds on every machine that reads
    /// the plan, so it is authoritative for its operation; this machine's apply record
    /// holds only an apply that has no checked-in hold (one applied before the hold
    /// travelled with the plan).
    pub(super) fn template_holds(
        &self,
        owner_id: &str,
    ) -> Result<BTreeMap<String, AcceptWait>, OrchestrationError> {
        let plan = self.plan();
        let checked_in = plan.holds(owner_id)?;
        let mut held: BTreeMap<String, AcceptWait> = plan
            .held_steps(owner_id)?
            .into_iter()
            .map(|(step_id, hold)| {
                (
                    step_id,
                    AcceptWait {
                        operation_id: hold.operation_id,
                        template_name: hold.template_name,
                    },
                )
            })
            .collect();
        let applies: Vec<TemplateApply> = self
            .template_applies(owner_id)?
            .applies()?
            .into_iter()
            .filter(|apply| {
                apply.waits_for_accept()
                    && !checked_in
                        .iter()
                        .any(|hold| hold.operation_id == apply.operation_id)
            })
            .collect();
        if applies.is_empty() {
            return Ok(held);
        }
        let records = plan.records(owner_id)?;
        for (step_id, apply) in held_template_steps(
            &applies,
            records.iter().map(|record| {
                (
                    record.step.step_id.as_str(),
                    record.step.ticket.connection_id.as_str(),
                    record.step.ticket.native_id.as_str(),
                )
            }),
        ) {
            held.entry(step_id).or_insert_with(|| AcceptWait {
                operation_id: apply.operation_id.clone(),
                template_name: apply.snapshot.name.clone(),
            });
        }
        Ok(held)
    }

    /// HS2-38MFXT: check the apply's Wait for Accept in with the plan, once its goal's
    /// id is recorded and before the goal ticket opens or the planner runs, so a
    /// machine that reads one of the goal's steps also reads its hold. A repeat changes
    /// nothing; a template that does not wait checks nothing in.
    fn template_hold_check_in(
        &self,
        owner_id: &str,
        apply: &TemplateApply,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        let Some(goal) = apply.goal.as_ref() else {
            return Ok(());
        };
        if !apply.snapshot.wait_for_accept || apply.state == ApplyState::Denied {
            return Ok(());
        }
        let now = tx.clock.now_ms();
        self.plan().update_holds(owner_id, tx, |holds, _| {
            match holds
                .iter_mut()
                .find(|hold| hold.operation_id == apply.operation_id)
            {
                Some(hold) => {
                    if hold.goal_slug.is_none() {
                        hold.goal_slug.clone_from(&goal.slug);
                    }
                }
                None => holds.push(PlanHold {
                    operation_id: apply.operation_id.clone(),
                    template_id: apply.template_id.clone(),
                    template_name: apply.snapshot.name.clone(),
                    goal: hotsheet_ticketing::orchestration::RunTicket {
                        connection_id: goal.connection_id.clone(),
                        native_id: goal.native_id.clone(),
                    },
                    goal_slug: goal.slug.clone(),
                    accepted_steps: apply.accepted_steps.clone(),
                    accepted_at_ms: apply.accepted_at_ms,
                    accepted_by: apply.accepted_by.clone(),
                    created_at_ms: now,
                }),
            }
            Ok(())
        })
    }

    /// HS2-36XDDX: the live plan steps of a goal, by step id. HS2-38MFXT: matched by the
    /// goal's native id, since a Git source's connection id differs per machine.
    fn goal_steps(
        &self,
        owner_id: &str,
        native_id: &str,
    ) -> Result<Vec<String>, OrchestrationError> {
        Ok(self
            .plan()
            .records(owner_id)?
            .into_iter()
            .filter(|record| record.step.ticket.native_id == native_id)
            .map(|record| record.step.step_id)
            .collect())
    }

    /// HS2-36XDDX / HS2-38MFXT: record a person's accept of the goal's live steps on
    /// the checked-in hold (when there is one) and on this machine's apply record (when
    /// there is one). The caller holds the apply records' lock. Answers the steps this
    /// accept released, or `None` when the operation is unknown here and in the plan.
    fn template_accept_locked(
        &self,
        owner_id: &str,
        store: &TemplateApplyStore,
        operation_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<AcceptOutcome>, OrchestrationError> {
        let local = store.find(operation_id)?;
        let hold = self
            .plan()
            .holds(owner_id)?
            .into_iter()
            .find(|hold| hold.operation_id == operation_id);
        let goal = match (&hold, &local) {
            (Some(hold), _) => hold.goal.native_id.clone(),
            (None, Some(apply)) => match apply.goal.as_ref().filter(|goal| goal.created) {
                Some(goal) => goal.native_id.clone(),
                None => {
                    return Ok(Some(AcceptOutcome {
                        apply: local,
                        hold: None,
                        released: Vec::new(),
                        steps: Vec::new(),
                    }));
                }
            },
            (None, None) => return Ok(None),
        };
        let steps = self.goal_steps(owner_id, &goal)?;
        let accepted: &[String] = match (&hold, &local) {
            (Some(hold), _) => &hold.accepted_steps,
            (None, Some(apply)) => &apply.accepted_steps,
            (None, None) => &[],
        };
        let released: Vec<String> = steps
            .iter()
            .filter(|step| !accepted.contains(step))
            .cloned()
            .collect();
        let by = actor.and_then(|actor| actor.id.as_deref());
        let now = tx.clock.now_ms();
        let mut local = local;
        if let Some(apply) = local.as_mut()
            && !steps.is_empty()
            && apply.accept(&steps, by, now)
        {
            store.write(apply)?;
        }
        let hold = if hold.is_some() && !steps.is_empty() {
            self.plan().update_holds(owner_id, tx, |holds, _| {
                Ok(holds
                    .iter_mut()
                    .find(|hold| hold.operation_id == operation_id)
                    .map(|hold| {
                        hold.accept(&steps, by, now);
                        hold.clone()
                    }))
            })?
        } else {
            hold
        };
        Ok(Some(AcceptOutcome {
            apply: local,
            hold,
            released,
            steps,
        }))
    }

    /// HS2-36XDDX: a person accepts the plan the planner proposed for an apply's goal,
    /// as they review any plan: the goal's live plan steps are released, and placement
    /// starts them within the policy as usual. Steps the planner adds later wait for
    /// the next accept. Only a person accepts (an AI actor is `forbidden`); a goal with
    /// nothing planned yet is `template_accept_nothing_planned`, and an accept with
    /// nothing new answers `existing`. Audited as `template_accept`.
    ///
    /// HS2-38MFXT: any machine that reads the plan accepts: the operation is found on
    /// this machine's apply record or in the plan's checked-in hold, and the accept is
    /// checked in, so every machine's scheduler releases the steps.
    pub fn template_accept(
        &self,
        owner_id: &str,
        request: &AcceptTemplate,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TemplateAcceptView, OrchestrationError> {
        access::person(actor)?;
        self.owner(owner_id)?;
        if request.all || request.operation_id.is_empty() {
            return Err(OrchestrationError::Invalid(
                "name one operation id, or accept every held plan with all",
            ));
        }
        let store = self.template_applies(owner_id)?;
        let result = (|| {
            let _lock = store.lock()?;
            let outcome = self
                .template_accept_locked(owner_id, &store, &request.operation_id, actor, tx)?
                .ok_or(template_error(TemplateError::ApplyNotFound))?;
            if outcome.steps.is_empty() {
                return Err(template_error(TemplateError::NothingToAccept));
            }
            Ok(TemplateAcceptView {
                outcome: if outcome.released.is_empty() {
                    "existing"
                } else {
                    "accepted"
                }
                .into(),
                apply: outcome.apply,
                hold: outcome.hold,
                released: outcome.released,
            })
        })();
        self.template_audit(
            owner_id,
            actor,
            "template_accept",
            Some(&request.operation_id),
            match &result {
                Ok(view) if view.outcome == "accepted" => {
                    (AuditOutcome::Succeeded, Some("template_accepted"))
                }
                Ok(_) => (AuditOutcome::Succeeded, Some("template_accept_existing")),
                Err(error) => (AuditOutcome::Failed, Some(error.code())),
            },
            tx,
        )?;
        let view = result?;
        if view.outcome == "accepted" {
            // The scheduler places the released steps; a timeline reader sees them move.
            Self::scheduler_wake_at(&self.home);
        }
        Ok(view)
    }

    /// HS2-0YHA2S: the Orchestration header's Accept Plan: a person accepts every
    /// template plan held now, each as its own [`Self::template_accept`] (audited
    /// `template_accept` per apply). Only a person accepts; with nothing held the answer
    /// is `existing` and nothing is written.
    pub fn template_accept_held(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TemplateAcceptAllView, OrchestrationError> {
        access::person(actor)?;
        self.owner(owner_id)?;
        let operations: BTreeSet<String> = self
            .template_holds(owner_id)?
            .into_values()
            .map(|wait| wait.operation_id)
            .collect();
        let mut accepted = Vec::new();
        for operation_id in operations {
            accepted.push(self.template_accept(
                owner_id,
                &AcceptTemplate {
                    operation_id,
                    all: false,
                },
                actor,
                tx,
            )?);
        }
        let mut released: Vec<String> = accepted
            .iter()
            .flat_map(|view| view.released.iter().cloned())
            .collect();
        released.sort();
        Ok(TemplateAcceptAllView {
            outcome: if released.is_empty() {
                "existing"
            } else {
                "accepted"
            }
            .into(),
            accepted,
            released,
        })
    }

    /// HS2-36XDDX: Start Orchestration is a person's review of the whole plan, so it
    /// accepts every template plan held now (Manual placement stays as it was: its
    /// plan waits for Start and then starts). Audited as `template_accept` with
    /// `start_orchestration`. Answers the operation ids accepted. HS2-38MFXT: checked
    /// in, as [`Self::template_accept`].
    pub(super) fn template_accept_all(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<String>, OrchestrationError> {
        let held = self.template_holds(owner_id)?;
        let operations: BTreeSet<String> = held
            .values()
            .map(|wait| wait.operation_id.clone())
            .collect();
        if operations.is_empty() {
            return Ok(Vec::new());
        }
        let store = self.template_applies(owner_id)?;
        {
            let _lock = store.lock()?;
            for operation_id in &operations {
                self.template_accept_locked(owner_id, &store, operation_id, actor, tx)?;
            }
        }
        for operation_id in &operations {
            self.template_audit(
                owner_id,
                actor,
                "template_accept",
                Some(operation_id),
                (AuditOutcome::Succeeded, Some("start_orchestration")),
                tx,
            )?;
        }
        Self::scheduler_wake_at(&self.home);
        Ok(operations.into_iter().collect())
    }

    /// One apply as recorded (`template_apply_not_found`).
    pub fn template_apply_show(
        &self,
        owner_id: &str,
        reference: &ApplyRef,
    ) -> Result<TemplateApplyView, OrchestrationError> {
        self.owner(owner_id)?;
        let apply = self
            .template_applies(owner_id)?
            .find(&reference.operation_id)?
            .ok_or(template_error(TemplateError::ApplyNotFound))?;
        Ok(TemplateApplyView {
            apply,
            outcome: None,
        })
    }

    /// Apply a template as `actor` under the caller's retained `operation_id`:
    ///
    /// 1. A person's apply is their own act. Any other actor asks through the
    ///    permission card for ticket writes in each project the brief names (one
    ///    request per project, under `<operation id>.t<n>`), and the call answers
    ///    `permission_required` until a person answers; then the same call goes on.
    /// 2. One new goal ticket opens in the orchestration project with the rendered
    ///    brief. Its id is recorded first, so a repeat creates the same ticket.
    /// 3. The planner takes the goal: Propose Plan while the live plan is empty,
    ///    Update Plan once it has steps, under the proposal id `<operation id>` (a
    ///    later attempt `<operation id>.p<n>`), recorded before asking.
    ///
    /// A repeat of a finished apply changes nothing (`outcome: existing`); a repeat
    /// with another template, other inputs or other choices is `template_apply_conflict`.
    /// The record keeps the template as applied, so a repeat after the template
    /// changed still uses the snapshot. A planner at work on another proposal leaves
    /// the apply `waiting_planner` with its goal open: a repeat asks again, and any
    /// later Propose Plan or Update Plan plans that goal as well. Audited as
    /// `template_apply`.
    pub fn template_apply(
        &self,
        owner_id: &str,
        request: ApplyTemplate,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TemplateApplyView, OrchestrationError> {
        if actor.and_then(|actor| actor.id.as_ref()).is_none() {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to apply a template",
            ));
        }
        identifier(&request.tool, "bounded tool id required")?;
        identifier(&request.operation_id, "bounded operation id required")?;
        if request.operation_id.len() > MAX_APPLY_OPERATION_ID {
            return Err(OrchestrationError::Invalid(
                "an apply's operation id is at most 140 characters",
            ));
        }
        let owner = self.owner(owner_id)?;
        let store = self.template_applies(owner_id)?;
        let _lock = store.lock()?;
        let existing = store.find(&request.operation_id)?;
        let mut apply = match decide_apply(
            existing.as_ref(),
            &request.template_id,
            &request.inputs,
            &request.choices,
        ) {
            ApplyDecision::Conflict => {
                self.template_audit(
                    owner_id,
                    actor,
                    "template_apply",
                    Some(&request.operation_id),
                    (
                        AuditOutcome::Failed,
                        Some(TemplateError::ApplyConflict.code()),
                    ),
                    tx,
                )?;
                return Err(template_error(TemplateError::ApplyConflict));
            }
            ApplyDecision::Answer => {
                let apply = existing.ok_or(OrchestrationError::Corrupt)?;
                return Ok(TemplateApplyView {
                    apply,
                    outcome: Some("existing".into()),
                });
            }
            ApplyDecision::Resume => existing.ok_or(OrchestrationError::Corrupt)?,
            ApplyDecision::New => {
                let template = self.active_template(&owner, &request.template_id)?;
                let (inputs, problems) = resolve_inputs(&template, &request.inputs);
                if !problems.is_empty()
                    || request
                        .choices
                        .keys()
                        .any(|key| template.step(key).is_none())
                {
                    return Err(template_error(TemplateError::InputsInvalid));
                }
                let now = tx.clock.now_ms();
                TemplateApply {
                    operation_id: request.operation_id.clone(),
                    template_id: template.template_id.clone(),
                    snapshot: template,
                    inputs,
                    choices: request.choices.clone(),
                    tool: request.tool.clone(),
                    model: request.model.clone(),
                    effort: request.effort.clone(),
                    actor_role: access::role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    state: ApplyState::WaitingPermission,
                    goal: None,
                    proposal_id: None,
                    planning: None,
                    attempts: 0,
                    permission_requests: Vec::new(),
                    reason: None,
                    accepted_steps: Vec::new(),
                    accepted_at_ms: None,
                    accepted_by: None,
                    created_at_ms: now,
                    updated_at_ms: now,
                }
            }
        };
        let result = self.template_apply_steps(&owner, &store, &mut apply, actor, tx);
        let reason = match &result {
            Ok(()) => match apply.state {
                ApplyState::Planning => "template_planning",
                ApplyState::WaitingPlanner => "template_waiting_planner",
                ApplyState::WaitingPermission => "permission_required",
                ApplyState::Denied => "permission_denied",
            },
            Err(error) => error.code(),
        };
        self.template_audit(
            owner_id,
            actor,
            "template_apply",
            Some(&request.operation_id),
            (
                if result.is_ok() {
                    AuditOutcome::Succeeded
                } else {
                    AuditOutcome::Failed
                },
                Some(reason),
            ),
            tx,
        )?;
        result?;
        Ok(TemplateApplyView {
            apply,
            outcome: Some("applied".into()),
        })
    }

    fn template_apply_steps(
        &self,
        owner: &Checkout,
        store: &TemplateApplyStore,
        apply: &mut TemplateApply,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        let owner_id = owner.id.as_str();
        let active = self.active_targets(owner_id)?;
        let brief = render_brief(
            &apply.snapshot,
            &apply.inputs,
            &apply.choices,
            &apply.operation_id,
            &active,
        );
        let targets = brief.targets();
        if targets.iter().any(|target| !active.contains_key(target)) {
            return Err(template_error(TemplateError::TargetUnknown));
        }
        let goal_created = apply.goal.as_ref().is_some_and(|goal| goal.created);
        // 1. A non-person apply asks for ticket writes in each project, until the goal
        // exists (after that the decision was taken and used).
        let mut allowed: Vec<(TargetRef, Authorized)> = Vec::new();
        if !goal_created && !is_person(actor) {
            let mut waiting = Vec::new();
            for (n, target_id) in targets.iter().enumerate() {
                let target = TargetRef {
                    owner_checkout_id: owner_id.into(),
                    target_id: target_id.clone(),
                };
                match self.authorize(
                    &target,
                    actor,
                    ActionRequest::new(TargetAction::TicketsWrite)
                        .operation(format!("{}.t{n}", apply.operation_id)),
                    tx,
                ) {
                    Ok(authorized) => allowed.push((target, authorized)),
                    Err(OrchestrationError::PermissionRequired { request_id }) => {
                        waiting.push(request_id);
                    }
                    Err(OrchestrationError::Denied(DenialReason::PermissionDenied)) => {
                        apply.state = ApplyState::Denied;
                        apply.reason = Some(DenialReason::PermissionDenied.code().into());
                        apply.updated_at_ms = tx.clock.now_ms();
                        store.write(apply)?;
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                }
            }
            if let Some(first) = waiting.first().cloned() {
                apply.state = ApplyState::WaitingPermission;
                apply.permission_requests = waiting;
                apply.reason = Some(DenialReason::PermissionRequired.code().into());
                apply.updated_at_ms = tx.clock.now_ms();
                store.write(apply)?;
                return Err(OrchestrationError::PermissionRequired { request_id: first });
            }
            apply.permission_requests.clear();
        }
        // 2. The goal ticket, under an id recorded before it is created.
        if !goal_created {
            let source = owner
                .sources
                .iter()
                .find(|source| source.provider == "git")
                .ok_or(OrchestrationError::TargetOperation(
                    hotsheet_ticketing::orchestration::TargetOpError::LinksUnsupported,
                ))?;
            if apply.goal.is_none() {
                apply.goal = Some(ApplyGoal {
                    connection_id: source.connection_id.clone(),
                    native_id: hotsheet_ticketing::mint_ulid(tx.clock, &mut *tx.rng).to_string(),
                    slug: None,
                    created: false,
                });
                apply.state = ApplyState::WaitingPlanner;
                apply.reason = None;
                apply.updated_at_ms = tx.clock.now_ms();
                store.write(apply)?;
            }
            // HS2-38MFXT: the hold is checked in before the goal opens.
            self.template_hold_check_in(owner_id, apply, tx)?;
            let goal = apply.goal.clone().ok_or(OrchestrationError::Corrupt)?;
            let ticket_store = owner
                .verified_git_source(&goal.connection_id)
                .map_err(checkout_error)?;
            let id: ulid::Ulid = goal
                .native_id
                .parse()
                .map_err(|_| OrchestrationError::Corrupt)?;
            let failed = |_| {
                OrchestrationError::TargetOperation(
                    hotsheet_ticketing::orchestration::TargetOpError::Failed,
                )
            };
            let slug = match ticket_store.read_ticket(&id) {
                Ok(ticket) => ticket.slug,
                Err(_) => {
                    let prefix = ticket_store.metadata().map_err(failed)?.ticket_prefix;
                    let mut tags = vec![TEMPLATE_GOAL_TAG.to_owned()];
                    tags.extend(apply.snapshot.goal.tags.iter().cloned());
                    ops::create_by(
                        &ticket_store,
                        id,
                        &prefix,
                        super::tickets::timestamp(tx),
                        ops::NewTicket {
                            title: brief.goal_title.chars().take(200).collect(),
                            category: "task".into(),
                            details: brief.goal_details.clone(),
                            tags,
                            ..ops::NewTicket::default()
                        },
                        hotsheet_ticketing::actor::note_actor(actor),
                    )
                    .map_err(failed)?
                    .slug
                }
            };
            for (target, authorized) in &allowed {
                self.record_outcome(target, authorized, true, Some("template_goal_opened"), tx)?;
            }
            apply.goal = Some(ApplyGoal {
                slug: Some(slug),
                created: true,
                ..goal
            });
            apply.state = ApplyState::WaitingPlanner;
            apply.updated_at_ms = tx.clock.now_ms();
            store.write(apply)?;
        }
        // HS2-38MFXT: and again with the goal's slug (or for an apply recorded before the
        // hold was checked in), before the planner defines any step.
        self.template_hold_check_in(owner_id, apply, tx)?;
        // 3. The planner takes the goal. A proposal asked for and not answered (a
        // crash in between) is asked again under the same id.
        let interrupted = apply.state == ApplyState::WaitingPlanner
            && apply.reason.is_none()
            && apply.proposal_id.is_some();
        let proposal_id = if interrupted {
            apply
                .proposal_id
                .clone()
                .ok_or(OrchestrationError::Corrupt)?
        } else {
            let next = if apply.attempts == 0 {
                apply.operation_id.clone()
            } else {
                format!("{}.p{}", apply.operation_id, apply.attempts)
            };
            apply.attempts += 1;
            apply.proposal_id = Some(next.clone());
            apply.reason = None;
            apply.updated_at_ms = tx.clock.now_ms();
            store.write(apply)?;
            next
        };
        let goal = apply.goal.clone().ok_or(OrchestrationError::Corrupt)?;
        let goal_ref = OrchestrationTicket {
            connection_id: goal.connection_id.clone(),
            native_id: goal.native_id.clone(),
        };
        let update = apply.planning.as_deref() == Some("update")
            || (apply.planning.is_none() && !self.plan().records(owner_id)?.is_empty());
        apply.planning = Some(if update { "update" } else { "propose" }.into());
        let taken: Result<bool, OrchestrationError> = if update {
            let mut request = UpdatePlan {
                tool: apply.tool.clone(),
                model: apply.model.clone(),
                effort: apply.effort.clone(),
                operation_id: Some(proposal_id.clone()),
                automatic: false,
                goal: None,
            };
            request.goal = Some(goal_ref);
            match self.plan_update(owner_id, request, actor, tx) {
                Ok(view) => Ok(view.update_id == proposal_id
                    && view.state == hotsheet_ticketing::orchestration::PlanUpdateState::Drafting),
                Err(OrchestrationError::Readiness(ReadinessError::PlannerBusy)) => Ok(false),
                Err(error) => Err(error),
            }
        } else {
            self.plan_propose(
                owner_id,
                ProposePlan {
                    tool: apply.tool.clone(),
                    model: apply.model.clone(),
                    effort: apply.effort.clone(),
                    goal: Some(goal_ref),
                    operation_id: Some(proposal_id.clone()),
                    plan_update: false,
                },
                actor,
                tx,
            )
            .map(|proposal| {
                matches!(
                    proposal.outcome,
                    ProposeOutcome::Started | ProposeOutcome::Replanned
                ) || proposal.proposal_id == proposal_id
            })
        };
        apply.updated_at_ms = tx.clock.now_ms();
        match taken {
            Ok(true) => {
                apply.state = ApplyState::Planning;
                apply.reason = None;
            }
            Ok(false) => {
                apply.state = ApplyState::WaitingPlanner;
                apply.reason = Some(ReadinessError::PlannerBusy.code().into());
                // The next attempt chooses again between Propose and Update.
                apply.planning = None;
            }
            Err(error) => {
                apply.state = ApplyState::WaitingPlanner;
                apply.reason = Some(error.code().into());
                apply.planning = None;
            }
        }
        store.write(apply)
    }
}

/// One line of at most 200 characters, for a title taken from a ticket.
fn sanitize_line(text: &str) -> String {
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(200)
        .collect::<String>()
        .replace("{{", "{ {");
    let line = line.trim().to_owned();
    if line.is_empty() {
        "Untitled".into()
    } else {
        line
    }
}

/// HS2-AMJNER: the longest a resume that left an apply waiting is put off before the
/// next try. A resume that did something, or a changed apply, starts over.
pub const MAX_RESUME_BACKOFF_MS: u64 = 60_000;
const FIRST_RESUME_BACKOFF_MS: u64 = 1_000;

/// HS2-AMJNER: what a resume pass remembers between passes: the applies it tried that
/// are still waiting, so one that stays blocked is tried again with a growing pause
/// rather than at every wake. Kept by the caller (the server's automation thread);
/// a headless pass starts empty.
#[derive(Debug, Default, Clone)]
pub struct ResumeMemory {
    tried: BTreeMap<String, (u64, u64, u64)>,
}

impl ResumeMemory {
    /// The earliest instant a put-off resume may be tried again.
    pub fn next_try_ms(&self) -> Option<u64> {
        self.tried.values().map(|(_, next_at, _)| *next_at).min()
    }
}

/// HS2-AMJNER: one apply a resume pass continued, and where it now stands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumedApply {
    pub operation_id: String,
    pub state: ApplyState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// HS2-AMJNER: what a resume pass did.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumeReport {
    pub resumed: Vec<ResumedApply>,
}

impl OrchestrationRuntime {
    /// HS2-AMJNER: whether the planner can take a goal now: no plan update drafting and
    /// no planner at work on another proposal (one that finished its turn can).
    pub(super) fn planner_free(&self, owner: &Checkout, tx: &mut Transaction<'_>) -> bool {
        if self
            .replan_latest(owner, tx)
            .map_or(true, |latest| latest.is_some_and(|(_, drafting)| drafting))
        {
            return false;
        }
        let Ok(planners) = self.planners(owner, tx) else {
            return false;
        };
        let facts = Self::planner_facts(&planners);
        hotsheet_ticketing::orchestration::planning_state(&facts)
            != hotsheet_ticketing::orchestration::PlanningState::Planning
            || hotsheet_ticketing::orchestration::current_planner(&facts)
                .is_some_and(|planner| planner.replannable())
    }

    /// HS2-AMJNER: continue every apply of the owner that waits and can go on now,
    /// under its own operation id and as the actor who applied it, exactly as that
    /// caller repeating the apply would:
    ///
    /// - `waiting_permission` once a person answered every request it waits on
    ///   (Allow goes on, Deny makes it `denied`);
    /// - `waiting_planner` (the planner was busy, or a crash came between asking and
    ///   the answer) once the planner is free.
    ///
    /// Anything else waits for its caller. An apply left waiting is tried again only
    /// after a pause that doubles up to [`MAX_RESUME_BACKOFF_MS`], so a planner that
    /// stays busy is not asked at every wake. Audited as the ordinary `template_apply`.
    pub fn template_resume_waiting(
        &self,
        owner_id: &str,
        memory: &mut ResumeMemory,
        tx: &mut Transaction<'_>,
    ) -> Result<ResumeReport, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let now = tx.clock.now_ms();
        let applies = self.template_applies(owner_id)?.applies()?;
        memory
            .tried
            .retain(|operation, _| applies.iter().any(|apply| &apply.operation_id == operation));
        let mut planner_free: Option<bool> = None;
        let mut report = ResumeReport::default();
        for apply in applies {
            if let Some((_, next_at, seen)) = memory.tried.get(&apply.operation_id)
                && *seen == apply.updated_at_ms
                && now < *next_at
            {
                continue;
            }
            let ready = match apply.state {
                ApplyState::WaitingPermission => {
                    !apply.permission_requests.is_empty()
                        && apply
                            .permission_requests
                            .iter()
                            .all(|request| !self.permission_waiting(owner_id, request, now))
                }
                ApplyState::WaitingPlanner => {
                    let resumable = apply.reason.is_none()
                        || apply.reason.as_deref() == Some(ReadinessError::PlannerBusy.code());
                    resumable && *planner_free.get_or_insert_with(|| self.planner_free(&owner, tx))
                }
                ApplyState::Planning | ApplyState::Denied => false,
            };
            if !ready {
                continue;
            }
            let Ok(actor) = MutationActor::parse(
                apply.actor_role.as_deref().unwrap_or("system"),
                apply.actor_id.clone(),
            ) else {
                continue;
            };
            let request = ApplyTemplate {
                template_id: apply.template_id.clone(),
                inputs: apply.inputs.clone(),
                choices: apply.choices.clone(),
                tool: apply.tool.clone(),
                model: apply.model.clone(),
                effort: apply.effort.clone(),
                operation_id: apply.operation_id.clone(),
            };
            let (state, reason, updated) =
                match self.template_apply(owner_id, request, Some(&actor), tx) {
                    Ok(view) => (
                        view.apply.state,
                        view.apply.reason.clone(),
                        view.apply.updated_at_ms,
                    ),
                    Err(error) => {
                        let current = self
                            .template_applies(owner_id)?
                            .find(&apply.operation_id)?
                            .unwrap_or_else(|| apply.clone());
                        (
                            current.state,
                            Some(error.code().to_owned()),
                            current.updated_at_ms,
                        )
                    }
                };
            if matches!(state, ApplyState::Planning | ApplyState::Denied) {
                memory.tried.remove(&apply.operation_id);
            } else {
                let failures = memory
                    .tried
                    .get(&apply.operation_id)
                    .map_or(0, |(failures, _, _)| *failures)
                    + 1;
                let pause = FIRST_RESUME_BACKOFF_MS
                    .saturating_mul(1 << failures.min(6).saturating_sub(1))
                    .min(MAX_RESUME_BACKOFF_MS);
                memory.tried.insert(
                    apply.operation_id.clone(),
                    (failures, now.saturating_add(pause), updated),
                );
            }
            report.resumed.push(ResumedApply {
                operation_id: apply.operation_id,
                state,
                reason,
            });
        }
        Ok(report)
    }
}
