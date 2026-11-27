//! HS2-A4RVYA: reusable orchestration plan templates.
//!
//! A template is a checked-in, typed **brief** for the planner, not a deterministic
//! expansion (the maintainer's decision: "the planner AI adapts the template each
//! run"). Applying one opens a new goal ticket in the orchestration project whose
//! details carry the rendered brief (the template as it is now, the typed inputs and
//! the planner's rules), then hands that goal to the existing Propose Plan or Update
//! Plan flow. The goal is a snapshot: later template edits do not change it.
//!
//! This module holds the pure parts: the model and its validator, the checked-in file
//! `.hotsheet2/orchestration-templates.json` (merged by `template_id`, replaced under a
//! lock with a fresh revision, never created by a read, newer or damaged content
//! refused), typed input resolution, the brief, and the machine-local apply records
//! with the decision that makes a repeated apply under one operation id a no-op.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::contract::{valid_identifier, valid_token};
use super::manifest::fresh_token;
use super::storage::{Storage, decode};
use super::*;

/// The checked-in templates, relative to the orchestration project's checkout.
pub const TEMPLATES_PATH: &str = ".hotsheet2/orchestration-templates.json";
/// The directory under the machine home that holds the apply records, one file per
/// orchestration project.
pub const TEMPLATE_APPLY_DIR: &str = "orchestration-template-applies";
pub const MAX_TEMPLATES_BYTES: usize = 512 * 1024;
pub const MAX_TEMPLATES: usize = 64;
pub const MAX_TEMPLATE_INPUTS: usize = 16;
pub const MAX_TEMPLATE_STEPS: usize = 64;
pub const MAX_TEMPLATE_CHOICES: usize = 32;
pub const MAX_TEMPLATE_TAGS: usize = 8;
pub const MAX_TEMPLATE_VALIDATIONS: usize = 4;
/// A name, title, label, hint or choice.
pub const MAX_TEMPLATE_LINE: usize = 200;
/// A description, details or instructions.
pub const MAX_TEMPLATE_TEXT: usize = 4000;
/// The longest text input value, unless the input sets a shorter `max_len`.
pub const MAX_INPUT_TEXT: usize = 500;
/// The apply records one orchestration project keeps on this machine.
pub const MAX_TEMPLATE_APPLIES: usize = 256;
const MAX_APPLY_BYTES: usize = 4 * 1024 * 1024;
/// An apply's operation id leaves room for the derived ids (`<op>.p<n>`, `<op>.t<n>`).
pub const MAX_APPLY_OPERATION_ID: usize = 140;
/// The tag every goal a template opened carries.
pub const TEMPLATE_GOAL_TAG: &str = "orchestration-template";
/// HS2-36XDDX: the templates file's version while a template turns Wait for Accept off.
/// Version 1 never carries the field, so an older binary keeps reading a file whose
/// templates all wait (the default) and answers `upgrade_required` for one that does
/// not, before it could misread or overwrite it.
pub const TEMPLATES_ACCEPT_VERSION: u32 = 2;
/// HS2-36XDDX: why the scheduler leaves a step of a template's goal unplaced.
pub const TEMPLATE_PLAN_NOT_ACCEPTED: &str = "template_plan_not_accepted";

/// Why a template request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateError {
    /// No template has that id.
    NotFound,
    /// The template was retired; it can no longer be applied.
    Retired,
    /// The template definition is not valid (its `problems` say why).
    Invalid,
    /// The same template id with two different definitions, or a stale revision.
    Conflict,
    /// The inputs do not fit the template's typed inputs.
    InputsInvalid,
    /// A step names a project the orchestration project does not declare.
    TargetUnknown,
    /// The operation id was used for a different template or different inputs.
    ApplyConflict,
    /// No apply has that operation id on this machine.
    ApplyNotFound,
    /// The goal to save is not an open goal ticket, or it has no plan steps.
    GoalEmpty,
    /// HS2-36XDDX: the apply's goal has no plan step to accept yet.
    NothingToAccept,
}

impl TemplateError {
    pub const ALL: [Self; 10] = [
        Self::NotFound,
        Self::Retired,
        Self::Invalid,
        Self::Conflict,
        Self::InputsInvalid,
        Self::TargetUnknown,
        Self::ApplyConflict,
        Self::ApplyNotFound,
        Self::GoalEmpty,
        Self::NothingToAccept,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::NotFound => "template_not_found",
            Self::Retired => "template_retired",
            Self::Invalid => "template_invalid",
            Self::Conflict => "template_conflict",
            Self::InputsInvalid => "template_inputs_invalid",
            Self::TargetUnknown => "template_target_unknown",
            Self::ApplyConflict => "template_apply_conflict",
            Self::ApplyNotFound => "template_apply_not_found",
            Self::GoalEmpty => "template_goal_empty",
            Self::NothingToAccept => "template_accept_nothing_planned",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateState {
    #[default]
    Active,
    Retired,
}

/// The type of one input. Values are typed and substituted as text; nothing is
/// evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Text,
    Number,
    Boolean,
    /// One of `choices`.
    Choice,
    /// A ticket priority (`highest` … `lowest`).
    Priority,
    /// A project the orchestration project declares, by target id.
    Target,
    /// An AI tool, by plugin id.
    Tool,
}

pub const PRIORITIES: [&str; 5] = ["highest", "high", "default", "low", "lowest"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateInput {
    pub key: String,
    pub label: String,
    pub kind: InputKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// A text input's longest value; [`MAX_INPUT_TEXT`] when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_len: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateGoal {
    /// The new goal ticket's title; may hold `{{key}}` placeholders.
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// Extra tags of the goal ticket.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Whether a step looks for an existing target ticket first (the maintainer's
/// decision: search, propose reuse, let the person switch a step to create).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepTicket {
    /// Search the target's open tickets first and propose reusing a match.
    #[default]
    Search,
    /// Always create a new target ticket.
    Create,
}

impl StepTicket {
    fn is_search(&self) -> bool {
        *self == Self::Search
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepAfter {
    /// The step key this step waits for.
    pub step: String,
    /// `none`, `ci` (the default of a plan prerequisite), `deployment` or `review`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gates: Option<String>,
}

pub const STEP_GATES: [&str; 4] = ["none", "ci", "deployment", "review"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateStep {
    pub key: String,
    /// A declared target id, or `{{key}}` naming a `target` input.
    pub target: String,
    /// The target ticket's title; may hold placeholders.
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// A priority, or `{{key}}` naming a `priority` input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "StepTicket::is_search")]
    pub ticket: StepTicket,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<StepAfter>,
    /// Test profiles the target defines (§26.12) to validate this step's work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation: Vec<String>,
    /// A preferred AI tool, or `{{key}}` naming a `tool` input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// `{{key}}` naming a `boolean` input: the step applies only while it is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
}

/// One template as checked in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    /// A ULID, never reused: a template is retired, not deleted.
    #[serde(default)]
    pub template_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub state: TemplateState,
    /// Starts at 1 and grows with every changed definition.
    #[serde(default)]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<TemplateInput>,
    pub goal: TemplateGoal,
    pub steps: Vec<TemplateStep>,
    /// Free guidance the planner reads with the steps; may hold placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// HS2-36XDDX: while placement is on, the steps the planner defines for a goal this
    /// template opened wait until a person accepts the proposed plan. On by default
    /// (the maintainer's decision), so a template written before the setting existed
    /// waits; written only when off.
    #[serde(default = "accept_by_default", skip_serializing_if = "is_on")]
    pub wait_for_accept: bool,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

#[allow(clippy::unnecessary_wraps)] // HS2-36XDDX: serde's default needs a function.
const fn accept_by_default() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)] // HS2-36XDDX: serde's skip_serializing_if passes a reference.
const fn is_on(value: &bool) -> bool {
    *value
}

impl Template {
    /// The definition without the bookkeeping the store keeps (version, times,
    /// author), for telling whether a write changes anything.
    pub fn same_definition(&self, other: &Self) -> bool {
        let strip = |template: &Self| Self {
            version: 0,
            created_at_ms: 0,
            updated_at_ms: 0,
            updated_by: None,
            ..template.clone()
        };
        strip(self) == strip(other)
    }

    pub fn step(&self, key: &str) -> Option<&TemplateStep> {
        self.steps.iter().find(|step| step.key == key)
    }
}

/// One coded problem of a template, an input or an apply, with where it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TemplateProblem {
    pub code: String,
    /// A JSON-path-like location, such as `steps[1].after[0]` or `inputs.feature`.
    pub at: String,
}

fn problem(code: &str, at: impl Into<String>) -> TemplateProblem {
    TemplateProblem {
        code: code.into(),
        at: at.into(),
    }
}

/// The `{{key}}` placeholders of a text, in order.
pub fn placeholders(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        found.push(after[..end].trim());
        rest = &after[end + 2..];
    }
    found
}

/// The input key a whole-value reference (`{{key}}` and nothing else) names.
pub fn reference(text: &str) -> Option<&str> {
    let inner = text.trim().strip_prefix("{{")?.strip_suffix("}}")?.trim();
    (!inner.contains("{{") && !inner.contains("}}")).then_some(inner)
}

fn bounded(text: &str, limit: usize) -> bool {
    text.chars().count() <= limit && !text.chars().any(|c| c.is_control() && c != '\n')
}

fn line_ok(text: &str) -> bool {
    !text.trim().is_empty() && bounded(text, MAX_TEMPLATE_LINE) && !text.contains('\n')
}

/// Validate a template definition. Pure: every problem is listed, none is guessed
/// away. The template id and bookkeeping are checked by the store.
pub fn validate_template(template: &Template) -> Vec<TemplateProblem> {
    let mut problems = Vec::new();
    if !line_ok(&template.name) {
        problems.push(problem("name_invalid", "name"));
    }
    if !bounded(&template.description, MAX_TEMPLATE_TEXT) {
        problems.push(problem("text_too_long", "description"));
    }
    if template.inputs.len() > MAX_TEMPLATE_INPUTS {
        problems.push(problem("too_many_inputs", "inputs"));
    }
    let mut kinds: BTreeMap<&str, InputKind> = BTreeMap::new();
    for (n, input) in template.inputs.iter().enumerate() {
        let at = format!("inputs[{n}]");
        if valid_identifier(&input.key).is_err() || input.key.len() > 64 {
            problems.push(problem("input_key_invalid", format!("{at}.key")));
        } else if kinds.insert(input.key.as_str(), input.kind).is_some() {
            problems.push(problem("input_duplicate", format!("{at}.key")));
        }
        if !line_ok(&input.label) {
            problems.push(problem("input_label_invalid", format!("{at}.label")));
        }
        if input.hint.as_deref().is_some_and(|hint| !line_ok(hint)) {
            problems.push(problem("input_hint_invalid", format!("{at}.hint")));
        }
        match input.kind {
            InputKind::Choice if input.choices.is_empty() => {
                problems.push(problem("input_choices_missing", format!("{at}.choices")));
            }
            InputKind::Choice => {}
            _ if !input.choices.is_empty() => {
                problems.push(problem("input_choices_unexpected", format!("{at}.choices")));
            }
            _ => {}
        }
        let unique: BTreeSet<&String> = input.choices.iter().collect();
        if input.choices.len() > MAX_TEMPLATE_CHOICES
            || unique.len() != input.choices.len()
            || input.choices.iter().any(|choice| !line_ok(choice))
        {
            problems.push(problem("input_choices_invalid", format!("{at}.choices")));
        }
        if input.max_len.is_some()
            && (input.kind != InputKind::Text
                || input
                    .max_len
                    .is_some_and(|len| len == 0 || len as usize > MAX_INPUT_TEXT))
        {
            problems.push(problem("input_max_len_invalid", format!("{at}.max_len")));
        }
        if let Some(default) = &input.default
            && input_value(input, default).is_err()
        {
            problems.push(problem("input_default_invalid", format!("{at}.default")));
        }
    }
    let mut check_text = |text: &str, at: String, limit: usize, line: bool| {
        if (line && !line_ok(text)) || !bounded(text, limit) {
            problems.push(problem(
                if line {
                    "text_invalid"
                } else {
                    "text_too_long"
                },
                at.clone(),
            ));
        }
        for key in placeholders(text) {
            if !kinds.contains_key(key) {
                problems.push(problem("placeholder_unknown", at.clone()));
            }
        }
    };
    check_text(
        &template.goal.title,
        "goal.title".into(),
        MAX_TEMPLATE_LINE,
        true,
    );
    if let Some(details) = &template.goal.details {
        check_text(details, "goal.details".into(), MAX_TEMPLATE_TEXT, false);
    }
    if let Some(instructions) = &template.instructions {
        check_text(
            instructions,
            "instructions".into(),
            MAX_TEMPLATE_TEXT,
            false,
        );
    }
    for (n, step) in template.steps.iter().enumerate() {
        let at = format!("steps[{n}]");
        check_text(&step.title, format!("{at}.title"), MAX_TEMPLATE_LINE, true);
        if let Some(details) = &step.details {
            check_text(details, format!("{at}.details"), MAX_TEMPLATE_TEXT, false);
        }
    }
    if template.goal.tags.len() > MAX_TEMPLATE_TAGS
        || template
            .goal
            .tags
            .iter()
            .any(|tag| valid_identifier(tag).is_err() || tag.len() > 64)
    {
        problems.push(problem("goal_tags_invalid", "goal.tags"));
    }
    if template.steps.is_empty() {
        problems.push(problem("steps_missing", "steps"));
    }
    if template.steps.len() > MAX_TEMPLATE_STEPS {
        problems.push(problem("too_many_steps", "steps"));
    }
    let names_input = |text: &str, kind: InputKind| {
        reference(text).is_some_and(|key| kinds.get(key) == Some(&kind))
    };
    let mut keys: BTreeSet<&str> = BTreeSet::new();
    for (n, step) in template.steps.iter().enumerate() {
        let at = format!("steps[{n}]");
        if valid_identifier(&step.key).is_err() || step.key.len() > 64 {
            problems.push(problem("step_key_invalid", format!("{at}.key")));
        } else if !keys.insert(step.key.as_str()) {
            problems.push(problem("step_duplicate", format!("{at}.key")));
        }
        let literal_target = reference(&step.target).is_none()
            && valid_identifier(&step.target).is_ok()
            && !step.target.contains("{{");
        if !(literal_target || names_input(&step.target, InputKind::Target)) {
            problems.push(problem("step_target_invalid", format!("{at}.target")));
        }
        if let Some(category) = &step.category
            && (valid_identifier(category).is_err() || category.len() > 64)
        {
            problems.push(problem("step_category_invalid", format!("{at}.category")));
        }
        if let Some(priority) = &step.priority
            && !(PRIORITIES.contains(&priority.as_str())
                || names_input(priority, InputKind::Priority))
        {
            problems.push(problem("step_priority_invalid", format!("{at}.priority")));
        }
        if let Some(tool) = &step.tool
            && !((reference(tool).is_none() && valid_identifier(tool).is_ok())
                || names_input(tool, InputKind::Tool))
        {
            problems.push(problem("step_tool_invalid", format!("{at}.tool")));
        }
        if let Some(when) = &step.when
            && !names_input(when, InputKind::Boolean)
        {
            problems.push(problem("step_when_invalid", format!("{at}.when")));
        }
        if step.validation.len() > MAX_TEMPLATE_VALIDATIONS
            || step
                .validation
                .iter()
                .any(|name| valid_identifier(name).is_err() || name.len() > 64)
        {
            problems.push(problem(
                "step_validation_invalid",
                format!("{at}.validation"),
            ));
        }
        for (m, after) in step.after.iter().enumerate() {
            let here = format!("{at}.after[{m}]");
            if after.step == step.key {
                problems.push(problem("step_after_self", here.clone()));
            } else if template.step(&after.step).is_none() {
                problems.push(problem("step_after_unknown", here.clone()));
            }
            if after
                .gates
                .as_deref()
                .is_some_and(|gates| !STEP_GATES.contains(&gates))
            {
                problems.push(problem("step_gates_invalid", format!("{here}.gates")));
            }
        }
    }
    if let Some(cycle) = first_cycle(template) {
        problems.push(problem("step_cycle", format!("steps.{cycle}")));
    }
    problems.sort();
    problems.dedup();
    problems
}

/// A step key on a dependency cycle, when there is one.
fn first_cycle(template: &Template) -> Option<String> {
    // Kahn's algorithm over the known `after` edges.
    let mut waiting: BTreeMap<&str, usize> = template
        .steps
        .iter()
        .map(|step| {
            (
                step.key.as_str(),
                step.after
                    .iter()
                    .filter(|after| after.step != step.key && template.step(&after.step).is_some())
                    .count(),
            )
        })
        .collect();
    let mut ready: Vec<&str> = waiting
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(key, _)| *key)
        .collect();
    while let Some(done) = ready.pop() {
        waiting.remove(done);
        for step in &template.steps {
            if step.after.iter().any(|after| after.step == done)
                && let Some(count) = waiting.get_mut(step.key.as_str())
            {
                *count = count.saturating_sub(step.after.iter().filter(|a| a.step == done).count());
                if *count == 0 {
                    ready.push(step.key.as_str());
                }
            }
        }
    }
    waiting.keys().next().map(|key| (*key).to_owned())
}

/// The typed value of one input, or why it does not fit.
pub fn input_value(input: &TemplateInput, value: &Value) -> Result<Value, &'static str> {
    match input.kind {
        InputKind::Text => {
            let text = value.as_str().ok_or("input_invalid")?.trim();
            let limit = input.max_len.map_or(MAX_INPUT_TEXT, |len| len as usize);
            if text.is_empty() {
                return Err("input_missing");
            }
            if !line_ok(text) || text.chars().count() > limit || text.contains("{{") {
                return Err("input_invalid");
            }
            Ok(Value::String(text.to_owned()))
        }
        InputKind::Number => value
            .as_f64()
            .filter(|number| number.is_finite())
            .map(|_| value.clone())
            .ok_or("input_invalid"),
        InputKind::Boolean => value.as_bool().map(Value::Bool).ok_or("input_invalid"),
        InputKind::Choice => {
            let text = value.as_str().ok_or("input_invalid")?;
            if input.choices.iter().any(|choice| choice == text) {
                Ok(Value::String(text.to_owned()))
            } else {
                Err("input_invalid")
            }
        }
        InputKind::Priority => {
            let text = value.as_str().ok_or("input_invalid")?;
            if PRIORITIES.contains(&text) {
                Ok(Value::String(text.to_owned()))
            } else {
                Err("input_invalid")
            }
        }
        InputKind::Target | InputKind::Tool => {
            let text = value.as_str().ok_or("input_invalid")?;
            if text.is_empty() {
                return Err("input_missing");
            }
            valid_identifier(text).map_err(|_| "input_invalid")?;
            Ok(Value::String(text.to_owned()))
        }
    }
}

/// Resolve the supplied inputs against the template's typed inputs: defaults fill
/// what is missing, a required input without a value is `input_missing`, a value
/// that does not fit its type is `input_invalid`, and a key the template does not
/// declare is `input_unknown`. Pure; the problems are sorted.
pub fn resolve_inputs(
    template: &Template,
    supplied: &BTreeMap<String, Value>,
) -> (BTreeMap<String, Value>, Vec<TemplateProblem>) {
    let mut values = BTreeMap::new();
    let mut problems = Vec::new();
    for key in supplied.keys() {
        if !template.inputs.iter().any(|input| &input.key == key) {
            problems.push(problem("input_unknown", format!("inputs.{key}")));
        }
    }
    for input in &template.inputs {
        let at = format!("inputs.{}", input.key);
        let given = supplied
            .get(&input.key)
            .filter(|value| !value.is_null() && value.as_str() != Some(""));
        match given.or(input.default.as_ref()) {
            Some(value) => match input_value(input, value) {
                Ok(value) => {
                    values.insert(input.key.clone(), value);
                }
                Err(code) if code == "input_missing" && !input.required => {}
                Err(code) => problems.push(problem(code, at)),
            },
            None if input.required => problems.push(problem("input_missing", at)),
            // An unset boolean reads as false, so a `when` on it drops the step.
            None if input.kind == InputKind::Boolean => {
                values.insert(input.key.clone(), Value::Bool(false));
            }
            None => {}
        }
    }
    problems.sort();
    (values, problems)
}

/// An input value as the brief shows it.
fn shown(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "yes".into(),
        Value::Bool(false) => "no".into(),
        other => other.to_string(),
    }
}

/// Substitute `{{key}}` placeholders with the inputs' values as text. A key with no
/// value becomes empty. Nothing is evaluated, and a value never adds a placeholder.
pub fn substitute(text: &str, inputs: &BTreeMap<String, Value>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        if let Some(value) = inputs.get(after[..end].trim()) {
            out.push_str(&shown(value));
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// One step of an apply's brief, with its inputs substituted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefStep {
    pub key: String,
    pub target_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    pub ticket: StepTicket,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<StepAfter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

/// What an apply hands the planner: the goal ticket's title and details, and the
/// steps the details describe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brief {
    pub goal_title: String,
    pub goal_details: String,
    pub steps: Vec<BriefStep>,
    /// Steps a false `when` input left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
}

impl Brief {
    /// The distinct projects the steps name, sorted.
    pub fn targets(&self) -> Vec<String> {
        let set: BTreeSet<&String> = self.steps.iter().map(|step| &step.target_id).collect();
        set.into_iter().cloned().collect()
    }
}

/// Render the brief of one apply. `choices` switches steps between search-first and
/// create (the person's choice before applying). Pure: the same template, inputs,
/// choices and operation id give the same brief.
pub fn render_brief(
    template: &Template,
    inputs: &BTreeMap<String, Value>,
    choices: &BTreeMap<String, StepTicket>,
    operation_id: &str,
    target_names: &BTreeMap<String, String>,
) -> Brief {
    let resolve = |text: &str| -> String {
        reference(text)
            .and_then(|key| inputs.get(key))
            .map_or_else(|| text.to_owned(), shown)
    };
    let applies = |step: &TemplateStep| {
        step.when.as_deref().is_none_or(|when| {
            reference(when)
                .and_then(|key| inputs.get(key))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
    };
    let skipped: Vec<String> = template
        .steps
        .iter()
        .filter(|step| !applies(step))
        .map(|step| step.key.clone())
        .collect();
    let steps: Vec<BriefStep> = template
        .steps
        .iter()
        .filter(|step| applies(step))
        .map(|step| BriefStep {
            key: step.key.clone(),
            target_id: resolve(&step.target),
            title: substitute(&step.title, inputs),
            details: step
                .details
                .as_deref()
                .map(|details| substitute(details, inputs))
                .filter(|details| !details.trim().is_empty()),
            category: step.category.clone(),
            priority: step
                .priority
                .as_deref()
                .map(resolve)
                .filter(|priority| !priority.is_empty()),
            ticket: choices.get(&step.key).copied().unwrap_or(step.ticket),
            after: step
                .after
                .iter()
                .filter(|after| !skipped.contains(&after.step))
                .cloned()
                .collect(),
            validation: step.validation.clone(),
            tool: step
                .tool
                .as_deref()
                .map(resolve)
                .filter(|tool| !tool.is_empty()),
        })
        .collect();
    let goal_title = substitute(&template.goal.title, inputs);
    let name = |target: &str| {
        target_names.get(target).map_or_else(
            || format!("`{target}`"),
            |name| format!("{name} (`{target}`)"),
        )
    };
    let mut text = format!(
        "This goal was applied from the orchestration template **{}** (`{}`, version {}) under the operation `{operation_id}`. It is a snapshot: later edits of the template do not change it.\n\n",
        template.name, template.template_id, template.version
    );
    if let Some(details) = template
        .goal
        .details
        .as_deref()
        .map(|details| substitute(details, inputs))
        .filter(|details| !details.trim().is_empty())
    {
        text.push_str(&details);
        text.push_str("\n\n");
    }
    text.push_str("## Template brief\n\nPlan this goal with the `hotsheet-planner` skill's template rules. Adapt the steps below to the projects as they are now: they say what the work is, not exactly which tickets exist.\n");
    if !template.inputs.is_empty() {
        text.push_str("\n### Inputs\n\n");
        for input in &template.inputs {
            let value = inputs.get(&input.key).map_or_else(
                || "(not set)".to_owned(),
                |value| match input.kind {
                    InputKind::Target => name(&shown(value)),
                    _ => shown(value),
                },
            );
            text.push_str(&format!("- {}: {value}\n", input.label));
        }
    }
    text.push_str("\n### Steps\n\n");
    for (n, step) in steps.iter().enumerate() {
        let mut line = format!(
            "{}. `{}` in {}: {} \"{}\"",
            n + 1,
            step.key,
            name(&step.target_id),
            match step.ticket {
                StepTicket::Search => "find or create",
                StepTicket::Create => "create a new ticket",
            },
            step.title
        );
        if let Some(category) = &step.category {
            line.push_str(&format!(" · category {category}"));
        }
        if let Some(priority) = &step.priority {
            line.push_str(&format!(" · priority {priority}"));
        }
        if let Some(tool) = &step.tool {
            line.push_str(&format!(" · preferred tool {tool}"));
        }
        for after in &step.after {
            line.push_str(&format!(
                " · after `{}` (gates: {})",
                after.step,
                after.gates.as_deref().unwrap_or("ci")
            ));
        }
        for profile in &step.validation {
            line.push_str(&format!(" · validate with the profile `{profile}`"));
        }
        text.push_str(&line);
        text.push('\n');
        if let Some(details) = &step.details {
            for detail in details.lines() {
                text.push_str(&format!("   {detail}\n"));
            }
        }
    }
    if !skipped.is_empty() {
        text.push_str(&format!(
            "\nLeft out by the inputs: {}.\n",
            skipped
                .iter()
                .map(|key| format!("`{key}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(instructions) = template
        .instructions
        .as_deref()
        .map(|instructions| substitute(instructions, inputs))
        .filter(|instructions| !instructions.trim().is_empty())
    {
        text.push_str("\n### Instructions\n\n");
        text.push_str(&instructions);
        text.push('\n');
    }
    text.push_str(&format!(
        "\n### Rules\n\n- A step marked \"find or create\" searches its project's open tickets (`ticket list`) first. When one matches, reuse it (`ticket link`), name it in the `## Proposed plan` note as found by search, and ask once, in one `FEEDBACK NEEDED` note on this goal with a `CHOICE` per reused step, whether to keep reusing it or create a new ticket for that step; plan the rest meanwhile.\n- A step marked \"create a new ticket\" creates one without reusing a match.\n- Create a step's ticket under the operation id `tpl-{operation_id}-<step key>`, so proposing again never creates a second ticket.\n- Define one plan step per target ticket for this goal, with the `after` steps as prerequisites (their gates as written) and the preferred tool.\n- Attach each named validation profile to this goal as a validation step for that step's project.\n"
    ));
    // HS2-36XDDX: the planner says in its proposal that the plan waits for a person.
    if template.wait_for_accept {
        text.push_str("- This template waits for an accept: the steps you define for this goal start only after a person accepts the proposed plan in the Orchestration view, so finish as usual with the `## Proposed plan` note and do not ask anyone to start them.\n");
    }
    Brief {
        goal_title,
        goal_details: text,
        steps,
        skipped,
    }
}

/// The checked-in document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplatesFile {
    pub schema_version: u32,
    /// Changes with every write.
    pub revision: String,
    /// Ordered by template id.
    pub templates: Vec<Template>,
}

/// Merge two sets of templates by template id: one side only is kept, the same id on
/// both sides must be identical, and anything else is a conflict naming the ids.
pub fn merge_templates(
    ours: &[Template],
    theirs: &[Template],
) -> Result<Vec<Template>, Vec<String>> {
    let mut merged: BTreeMap<String, Template> = BTreeMap::new();
    let mut conflicts = Vec::new();
    for template in ours.iter().chain(theirs) {
        match merged.get(&template.template_id) {
            Some(existing) if existing != template => {
                conflicts.push(template.template_id.clone());
            }
            Some(_) => {}
            None => {
                merged.insert(template.template_id.clone(), template.clone());
            }
        }
    }
    if conflicts.is_empty() {
        Ok(merged.into_values().collect())
    } else {
        conflicts.sort();
        conflicts.dedup();
        Err(conflicts)
    }
}

/// HS2-36XDDX: version 1 while every template waits for an accept (the default);
/// [`TEMPLATES_ACCEPT_VERSION`] once one does not.
pub fn templates_version(templates: &[Template]) -> u32 {
    if templates.iter().any(|template| !template.wait_for_accept) {
        TEMPLATES_ACCEPT_VERSION
    } else {
        ORCHESTRATION_VERSION
    }
}

/// The version is read before the strict field set, as every orchestration file does.
fn decode_templates(bytes: &[u8]) -> Result<TemplatesFile, OrchestrationError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| OrchestrationError::Corrupt)?;
    let found = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or(OrchestrationError::Corrupt)?;
    if found != u64::from(ORCHESTRATION_VERSION) && found != u64::from(TEMPLATES_ACCEPT_VERSION) {
        return Err(OrchestrationError::UnsupportedVersion { found });
    }
    serde_json::from_value(value).map_err(|_| OrchestrationError::Corrupt)
}

/// The checked-in templates of one orchestration project's checkout.
#[derive(Clone)]
pub struct TemplateStore {
    storage: Storage,
}

impl TemplateStore {
    pub fn new(checkout_root: &Path) -> Self {
        Self {
            storage: Storage::shared_named(checkout_root, TEMPLATES_PATH),
        }
    }

    /// The document, or `None` when nothing was checked in yet. Never creates it; a
    /// newer schema is `upgrade_required`, and damaged content (merge markers, an
    /// invalid template, unsorted or duplicate ids) is `corrupt_orchestration_data`.
    pub fn read(&self) -> Result<Option<TemplatesFile>, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_TEMPLATES_BYTES)? else {
            return Ok(None);
        };
        let document = decode_templates(&bytes)?;
        valid_token(&document.revision).map_err(|_| OrchestrationError::Corrupt)?;
        // Version 1 never turns Wait for Accept off, so an older reader never ignores it.
        if document.schema_version != templates_version(&document.templates) {
            return Err(OrchestrationError::Corrupt);
        }
        if document.templates.len() > MAX_TEMPLATES
            || document
                .templates
                .windows(2)
                .any(|pair| pair[0].template_id >= pair[1].template_id)
            || document.templates.iter().any(|template| {
                valid_token(&template.template_id).is_err()
                    || template.version == 0
                    || !validate_template(template).is_empty()
            })
        {
            return Err(OrchestrationError::Corrupt);
        }
        Ok(Some(document))
    }

    /// The templates, empty when nothing was checked in.
    pub fn templates(&self) -> Result<Vec<Template>, OrchestrationError> {
        Ok(self
            .read()?
            .map(|document| document.templates)
            .unwrap_or_default())
    }

    pub fn lock(&self) -> Result<crate::FileLock, OrchestrationError> {
        self.storage.lock()
    }

    /// Replace the templates under the caller's lock. The same templates again change
    /// nothing and keep the revision; otherwise a fresh revision is written.
    pub fn write_locked(
        &self,
        previous: Option<&TemplatesFile>,
        mut templates: Vec<Template>,
        tx: &mut Transaction<'_>,
    ) -> Result<TemplatesFile, OrchestrationError> {
        templates.sort_by(|a, b| a.template_id.cmp(&b.template_id));
        if let Some(previous) = previous
            && previous.templates == templates
        {
            return Ok(previous.clone());
        }
        if templates.len() > MAX_TEMPLATES {
            return Err(OrchestrationError::Capacity);
        }
        if merge_templates(&templates, &[]).is_err() {
            return Err(OrchestrationError::Conflict);
        }
        let document = TemplatesFile {
            schema_version: templates_version(&templates),
            revision: fresh_token(tx, previous.map(|document| document.revision.as_str()))?,
            templates,
        };
        let mut bytes =
            serde_json::to_vec_pretty(&document).map_err(|_| OrchestrationError::Corrupt)?;
        bytes.push(b'\n');
        self.storage.write(&bytes, MAX_TEMPLATES_BYTES)?;
        Ok(document)
    }
}

/// Where one apply stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyState {
    /// An AI session or the orchestrator applied it, and a person has not answered
    /// the permission request yet. Repeat the apply after the answer.
    WaitingPermission,
    /// The goal is open, but the planner was at work on another proposal (or could
    /// not start). The goal stays open, so any later Propose Plan or Update Plan
    /// plans it too; repeating the apply asks the planner again.
    WaitingPlanner,
    /// The planner took the goal. A repeated apply changes nothing.
    Planning,
    /// A person denied the permission request; final for this operation id.
    Denied,
}

impl ApplyState {
    pub const ALL: [Self; 4] = [
        Self::WaitingPermission,
        Self::WaitingPlanner,
        Self::Planning,
        Self::Denied,
    ];

    /// No further step will be taken under this operation id.
    pub fn is_final(self) -> bool {
        matches!(self, Self::Planning | Self::Denied)
    }
}

/// The goal ticket an apply opened, by its canonical id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyGoal {
    pub connection_id: String,
    pub native_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// The ticket exists (its id is recorded before it is created).
    #[serde(default)]
    pub created: bool,
}

/// One apply of one template on this machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateApply {
    pub operation_id: String,
    pub template_id: String,
    /// The template as applied: the snapshot a repeat uses, whatever changed since.
    pub snapshot: Template,
    /// The resolved inputs.
    pub inputs: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub choices: BTreeMap<String, StepTicket>,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    pub state: ApplyState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<ApplyGoal>,
    /// The planner proposal (or plan update) last asked for; recorded before asking,
    /// so a repeat after a crash asks under the same id and never starts twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    /// `propose` or `update`: the flow the planner was asked through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning: Option<String>,
    /// How many times the planner was asked.
    #[serde(default)]
    pub attempts: u32,
    /// The permission requests a non-person apply waits on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_requests: Vec<String>,
    /// Why it waits or was denied, as a refusal code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// HS2-36XDDX: the plan steps of the goal a person accepted, by step id. While
    /// the snapshot waits for an accept, a step of the goal not listed here is held.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepted_steps: Vec<String>,
    /// HS2-36XDDX: the latest accept: when, and the person's actor id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_by: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl TemplateApply {
    /// HS2-36XDDX: the goal's new plan steps wait for a person's accept: the template
    /// as applied waits, and the goal was opened (a denied apply opened none).
    pub fn waits_for_accept(&self) -> bool {
        self.snapshot.wait_for_accept
            && self.state != ApplyState::Denied
            && self.goal.as_ref().is_some_and(|goal| goal.created)
    }

    /// HS2-36XDDX: whether the plan step `step_id` of `goal` is held for an accept:
    /// it belongs to this apply's goal, which waits, and was not accepted.
    pub fn holds(&self, step_id: &str, goal: (&str, &str)) -> bool {
        self.waits_for_accept()
            && self
                .goal
                .as_ref()
                .is_some_and(|own| (own.connection_id.as_str(), own.native_id.as_str()) == goal)
            && !self
                .accepted_steps
                .iter()
                .any(|accepted| accepted == step_id)
    }

    /// HS2-36XDDX: record a person's accept of `steps` (the goal's plan steps as they
    /// were shown), added to those accepted before. Answers whether anything was new;
    /// a repeat with nothing new changes nothing, not even the time.
    pub fn accept(&mut self, steps: &[String], by: Option<&str>, now_ms: u64) -> bool {
        let mut changed = false;
        for step in steps {
            if !self.accepted_steps.contains(step) {
                self.accepted_steps.push(step.clone());
                changed = true;
            }
        }
        if changed {
            self.accepted_steps.sort();
            self.accepted_at_ms = Some(now_ms);
            self.accepted_by = by.map(Into::into);
            self.updated_at_ms = now_ms;
        }
        changed
    }
}

/// HS2-36XDDX: the plan steps held for an accept, by step id, with the apply that
/// holds each. `steps` are the live plan's `(step_id, goal connection, goal native id)`.
/// Pure.
pub fn held_template_steps<'a, 's>(
    applies: &'a [TemplateApply],
    steps: impl IntoIterator<Item = (&'s str, &'s str, &'s str)>,
) -> BTreeMap<String, &'a TemplateApply> {
    let waiting: Vec<&TemplateApply> = applies
        .iter()
        .filter(|apply| apply.waits_for_accept())
        .collect();
    if waiting.is_empty() {
        return BTreeMap::new();
    }
    steps
        .into_iter()
        .filter_map(|(step_id, connection_id, native_id)| {
            waiting
                .iter()
                .find(|apply| apply.holds(step_id, (connection_id, native_id)))
                .map(|apply| (step_id.to_owned(), *apply))
        })
        .collect()
}

/// What a request to apply does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyDecision {
    /// No apply has this operation id: start one.
    New,
    /// The same apply, unfinished: continue it from where it stopped.
    Resume,
    /// The same apply, finished: answer with it and change nothing.
    Answer,
    /// The operation id was used for another template or other inputs.
    Conflict,
}

/// Decide a request to apply `template_id` with `inputs` and `choices` under an
/// operation id that may already have an apply. Pure. The inputs compared are the
/// ones the caller supplied, so a repeat after the template changed (a new default,
/// a new version) is still the same apply and keeps its snapshot.
pub fn decide_apply(
    existing: Option<&TemplateApply>,
    template_id: &str,
    supplied: &BTreeMap<String, Value>,
    choices: &BTreeMap<String, StepTicket>,
) -> ApplyDecision {
    let Some(apply) = existing else {
        return ApplyDecision::New;
    };
    let same_inputs = supplied.iter().all(|(key, value)| {
        value.is_null() || value.as_str() == Some("") || apply.inputs.get(key) == Some(value)
    });
    if apply.template_id != template_id || !same_inputs || &apply.choices != choices {
        return ApplyDecision::Conflict;
    }
    if apply.state.is_final() {
        ApplyDecision::Answer
    } else {
        ApplyDecision::Resume
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppliesFile {
    schema_version: u32,
    applies: Vec<TemplateApply>,
}

/// The machine-local apply records of one orchestration project, private and bounded.
#[derive(Clone)]
pub struct TemplateApplyStore {
    storage: Storage,
}

impl TemplateApplyStore {
    pub fn new(home: &Path, owner_checkout_id: &str) -> Result<Self, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(Self {
            storage: Storage::machine_named(
                home,
                &format!("{TEMPLATE_APPLY_DIR}/{owner_checkout_id}.json"),
            ),
        })
    }

    pub fn lock(&self) -> Result<crate::FileLock, OrchestrationError> {
        self.storage.lock()
    }

    fn read(&self) -> Result<AppliesFile, OrchestrationError> {
        match self.storage.read(MAX_APPLY_BYTES)? {
            Some(bytes) => decode(&bytes),
            None => Ok(AppliesFile {
                schema_version: ORCHESTRATION_VERSION,
                applies: Vec::new(),
            }),
        }
    }

    /// Every apply, oldest first.
    pub fn applies(&self) -> Result<Vec<TemplateApply>, OrchestrationError> {
        Ok(self.read()?.applies)
    }

    pub fn find(&self, operation_id: &str) -> Result<Option<TemplateApply>, OrchestrationError> {
        Ok(self
            .applies()?
            .into_iter()
            .find(|apply| apply.operation_id == operation_id))
    }

    /// Insert or replace one apply under the caller's lock. Past the bound the oldest
    /// finished apply is forgotten first; an unfinished one is never dropped, so a
    /// list full of them refuses with `capacity_reached`.
    pub fn write(&self, apply: &TemplateApply) -> Result<(), OrchestrationError> {
        let mut file = self.read()?;
        match file
            .applies
            .iter_mut()
            .find(|known| known.operation_id == apply.operation_id)
        {
            Some(known) => *known = apply.clone(),
            None => file.applies.push(apply.clone()),
        }
        while file.applies.len() > MAX_TEMPLATE_APPLIES {
            let forgettable = |known: &TemplateApply| {
                known.state.is_final() && known.operation_id != apply.operation_id
            };
            // HS2-36XDDX: forgetting a record releases any hold it keeps, so a finished
            // apply that never had an accept and still waits for one goes last.
            let Some(oldest) = file
                .applies
                .iter()
                .position(|known| {
                    forgettable(known)
                        && !(known.waits_for_accept() && known.accepted_at_ms.is_none())
                })
                .or_else(|| file.applies.iter().position(forgettable))
            else {
                return Err(OrchestrationError::Capacity);
            };
            file.applies.remove(oldest);
        }
        file.schema_version = ORCHESTRATION_VERSION;
        let mut bytes =
            serde_json::to_vec_pretty(&file).map_err(|_| OrchestrationError::Corrupt)?;
        bytes.push(b'\n');
        self.storage.write(&bytes, MAX_APPLY_BYTES)
    }
}
