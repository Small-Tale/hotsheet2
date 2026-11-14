//! HS2-YCJDEB: the orchestration throughput policy.
//!
//! A versioned, checked-in statement of the orchestrator's limits: targets it may
//! place sessions on, tools and models, concurrency caps, resource floors, wind-down
//! rules, build-ahead (speculation) and placement. Since HS2-XHQ1EH it holds limits
//! only: what the orchestrator may do on a target is asked of a person through the
//! permission bridge, and the retired `targets[].actions` and `approval` sections are
//! read, ignored and reported by `validate`. Integer units only (megabytes, percent,
//! cents).

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{bounded_text, valid_identifier, valid_token};
use super::manifest::fresh_token;
use super::storage::{Storage, decode};
use super::*;

pub const MAX_POLICY_TOOLS: usize = 16;
pub const MAX_POLICY_ACCOUNTS: usize = 16;
pub const MAX_POLICY_SESSIONS: u32 = 64;
pub const MAX_SPECULATION_DEPTH: u32 = 8;
pub const MAX_POLICY_MODELS: usize = 32;
const MAX_POLICY_BYTES: usize = 1024 * 1024;

/// Limits one machine that hosts a target enforces before a session starts there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineLimits {
    #[serde(default = "default_machine_sessions")]
    pub max_sessions: u32,
    #[serde(default = "default_free_ram_mb")]
    pub min_free_ram_mb: u64,
    #[serde(default = "default_cpu_percent")]
    pub max_cpu_percent: u32,
    #[serde(default = "default_free_disk_mb")]
    pub min_free_disk_mb: u64,
}
fn default_machine_sessions() -> u32 {
    2
}
fn default_free_ram_mb() -> u64 {
    2048
}
fn default_cpu_percent() -> u32 {
    90
}
fn default_free_disk_mb() -> u64 {
    5120
}
impl Default for MachineLimits {
    fn default() -> Self {
        Self {
            max_sessions: default_machine_sessions(),
            min_free_ram_mb: default_free_ram_mb(),
            max_cpu_percent: default_cpu_percent(),
            min_free_disk_mb: default_free_disk_mb(),
        }
    }
}

impl MachineLimits {
    /// HS2-J8PRTV: the first thing wrong with limits received outside a policy.
    pub fn problem(&self) -> Option<&'static str> {
        let mut found = None;
        check_machine(self, None, &mut |_, message, _| {
            found.get_or_insert(message);
        });
        found
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountPolicy {
    pub account: String,
    pub max_sessions: u32,
    /// Optional daily spend ceiling in cents; the scheduler stops new starts above it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_cents_per_day: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPolicy {
    pub tool: String,
    pub max_sessions: u32,
    /// Relative preference from 1 to 100 when several tools could take the same work.
    #[serde(default = "default_weight")]
    pub weight: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<AccountPolicy>,
}
fn default_weight() -> u32 {
    50
}

/// One AI tool sessions on a target may use. No models means any model of the tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolScope {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPolicy {
    pub target_id: String,
    /// A disabled entry keeps its settings but the orchestrator acts on nothing there.
    /// Reducing activity (stopping and winding down) is never limited by it.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// HS2-XHQ1EH: retired. A file that still lists actions reads with them ignored,
    /// `validate` reports them, and they are never written again.
    #[serde(default, skip_serializing)]
    pub actions: Option<serde_json::Value>,
    /// Tools (and optionally models) sessions on this target may use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolScope>,
    #[serde(default = "default_target_sessions")]
    pub max_sessions: u32,
    /// Overrides the policy's default machine limits for this target's machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<MachineLimits>,
    /// Overrides the policy-wide speculation switch for this target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speculation: Option<bool>,
}
fn enabled() -> bool {
    true
}
fn default_target_sessions() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindDown {
    /// A session with no claim, pending handoff or uncommitted work is wound down
    /// after this long idle.
    #[serde(default = "default_idle_minutes")]
    pub idle_minutes: u32,
    /// Idle sessions to keep per target so refilling does not pay a cold start.
    #[serde(default)]
    pub keep_warm: u32,
}
fn default_idle_minutes() -> u32 {
    15
}
impl Default for WindDown {
    fn default() -> Self {
        Self {
            idle_minutes: default_idle_minutes(),
            keep_warm: 0,
        }
    }
}

/// HS2-2SJPCH / HS2-JSF985 consume these; this ticket only stores and validates them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Speculation {
    #[serde(default)]
    pub allowed: bool,
    /// How many unconfirmed gates provisional work may stack on.
    #[serde(default = "default_depth")]
    pub max_depth: u32,
    /// 0 ranks provisional work like confirmed work; 100 ranks it last.
    #[serde(default = "default_risk")]
    pub risk_weight: u32,
    /// A ticket carrying this tag is never dispatched provisionally.
    #[serde(default = "default_opt_out")]
    pub opt_out_tag: String,
}
fn default_depth() -> u32 {
    1
}
fn default_risk() -> u32 {
    50
}
fn default_opt_out() -> String {
    "speculate:never".into()
}
impl Default for Speculation {
    fn default() -> Self {
        Self {
            allowed: false,
            max_depth: default_depth(),
            risk_weight: default_risk(),
            opt_out_tag: default_opt_out(),
        }
    }
}

/// HS2-ECWG50: how the scheduler places sessions. Automatic placement is off unless
/// a person turns it on, so a policy written before this section starts nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementPolicy {
    /// When false every scheduling pass is a dry run: decisions are computed and
    /// logged, and no session is started or wound down.
    #[serde(default)]
    pub automatic: bool,
    /// The grant holder the server's scheduling loop acts as. Required with
    /// `automatic`; a one-shot pass acts as whoever asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// Waiting this long raises work by one priority band, so nothing starves.
    #[serde(default = "default_aging_minutes")]
    pub aging_minutes: u32,
    /// A target's session count does not reverse direction (a start after a
    /// wind-down, or the reverse) within this many minutes.
    #[serde(default = "default_cooldown_minutes")]
    pub cooldown_minutes: u32,
    /// HS2-2FG91X: resources every piece of work on a target must hold, such as the
    /// one VM its machine has. The session claims them; the scheduler only reads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<TargetResources>,
    /// HS2-T16SK0: how long a placed step's run waits for a live target dispatcher
    /// (or legacy integrator) before its session claims the ticket for itself. 0
    /// claims as soon as the pool shows neither.
    /// Written only when it differs from the default, so a policy stored earlier reads
    /// back the same.
    #[serde(
        default = "default_dispatcher_wait_minutes",
        skip_serializing_if = "is_default_dispatcher_wait"
    )]
    pub dispatcher_wait_minutes: u32,
    /// HS2-NPTKS8: how long a session the scheduler started has to join its role in
    /// the target's pool before it is wound down at its safe point and its step is
    /// placed again. Written only when it differs from the default.
    #[serde(
        default = "default_join_minutes",
        skip_serializing_if = "is_default_join"
    )]
    pub join_minutes: u32,
}
fn default_join_minutes() -> u32 {
    super::DEFAULT_JOIN_MINUTES
}
fn is_default_join(minutes: &u32) -> bool {
    *minutes == default_join_minutes()
}
fn default_dispatcher_wait_minutes() -> u32 {
    super::DEFAULT_DISPATCHER_WAIT_MINUTES
}
fn is_default_dispatcher_wait(minutes: &u32) -> bool {
    *minutes == default_dispatcher_wait_minutes()
}
fn default_aging_minutes() -> u32 {
    60
}
fn default_cooldown_minutes() -> u32 {
    5
}
impl Default for PlacementPolicy {
    fn default() -> Self {
        Self {
            automatic: false,
            actor: None,
            aging_minutes: default_aging_minutes(),
            cooldown_minutes: default_cooldown_minutes(),
            resources: Vec::new(),
            dispatcher_wait_minutes: default_dispatcher_wait_minutes(),
            join_minutes: default_join_minutes(),
        }
    }
}
impl PlacementPolicy {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Everything a person edits; the store adds the version and revision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyBody {
    #[serde(default)]
    pub machine: MachineLimits,
    #[serde(default)]
    pub tools: Vec<ToolPolicy>,
    #[serde(default)]
    pub targets: Vec<TargetPolicy>,
    #[serde(default)]
    pub wind_down: WindDown,
    /// HS2-XHQ1EH: retired. A file that still has an `approval` section reads with it
    /// ignored, `validate` reports it, and it is never written again.
    #[serde(default, skip_serializing)]
    pub approval: Option<serde_json::Value>,
    #[serde(default)]
    pub speculation: Speculation,
    /// HS2-ECWG50: left out of the file while it holds only defaults.
    #[serde(default, skip_serializing_if = "PlacementPolicy::is_default")]
    pub placement: PlacementPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDocument {
    schema_version: u32,
    revision: String,
    #[serde(rename = "policy")]
    body: PolicyBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyView {
    pub enabled: bool,
    pub revision: Option<String>,
    pub policy: Option<PolicyBody>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPolicy {
    pub expected_revision: Option<String>,
    pub policy: PolicyBody,
}

/// One thing wrong with a policy. Codes are stable; `subject` is an id from the policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyProblem {
    pub code: &'static str,
    pub message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

/// Sessions already running, as the caller just observed them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionLoad {
    pub target_sessions: u32,
    pub machine_sessions: u32,
    pub tool_sessions: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default)]
    pub account_sessions: u32,
}

pub struct PolicyQuestion<'a> {
    pub target_id: &'a str,
    pub action: TargetAction,
    pub session: Option<&'a SessionRequest>,
    pub load: Option<&'a SessionLoad>,
    pub provisional: bool,
    pub role_session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDenial {
    /// Session management always needs a policy; nothing automatic runs without one.
    NoPolicy,
    TargetNotPermitted,
    TargetDisabled,
    ToolNotPermitted,
    ModelNotPermitted,
    /// A start cannot be checked against caps without the current load.
    LoadUnknown,
    TargetCap,
    MachineCap,
    ToolCap,
    AccountNotPermitted,
    AccountCap,
    SpeculationNotAllowed,
    /// HS2-J3ZF27: the owning project's type is Software, so it orchestrates nothing.
    ProjectSoftware,
}

impl PolicyDenial {
    pub fn code(self) -> &'static str {
        match self {
            Self::NoPolicy => "policy_absent",
            Self::TargetNotPermitted => "policy_target_not_permitted",
            Self::TargetDisabled => "policy_target_disabled",
            Self::ToolNotPermitted => "policy_tool_not_permitted",
            Self::ModelNotPermitted => "policy_model_not_permitted",
            Self::LoadUnknown => "policy_load_unknown",
            Self::TargetCap => "policy_target_cap",
            Self::MachineCap => "policy_machine_cap",
            Self::ToolCap => "policy_tool_cap",
            Self::AccountNotPermitted => "policy_account_not_permitted",
            Self::AccountCap => "policy_account_cap",
            Self::SpeculationNotAllowed => "policy_speculation_not_allowed",
            Self::ProjectSoftware => "policy_project_software",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyVerdict {
    Allowed,
    Denied(PolicyDenial),
}

/// HS2-XHQ1EH: a retired field an older policy file still holds. It is ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IgnoredField {
    pub code: &'static str,
    pub field: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

impl PolicyBody {
    pub fn target(&self, target_id: &str) -> Option<&TargetPolicy> {
        self.targets
            .iter()
            .find(|target| target.target_id == target_id)
    }

    /// The limits the target's machine must satisfy before a session starts there.
    /// HS2-2FG91X: what the policy requires of every piece of work on a target.
    pub fn target_resources(&self, target_id: &str) -> &[ResourceNeed] {
        self.placement
            .resources
            .iter()
            .find(|entry| entry.target_id == target_id)
            .map_or(&[], |entry| entry.resources.as_slice())
    }

    pub fn machine_limits(&self, target_id: &str) -> MachineLimits {
        self.target(target_id)
            .and_then(|target| target.machine)
            .unwrap_or(self.machine)
    }

    /// HS2-JSF985: whether the policy opts in to provisional work anywhere. While it
    /// does not, nothing about readiness, dispatch or scheduling differs from a
    /// policy without a `speculation` section.
    pub fn speculates(&self) -> bool {
        self.speculation.allowed
            || self
                .targets
                .iter()
                .any(|target| target.speculation == Some(true))
    }

    pub fn speculation_allowed(&self, target_id: &str) -> bool {
        self.target(target_id)
            .and_then(|target| target.speculation)
            .unwrap_or(self.speculation.allowed)
    }

    /// Every problem, so one `validate` shows the whole repair. `declared` is the set
    /// of active target ids in the manifest when the caller can supply it.
    pub fn problems(&self, declared: Option<&BTreeSet<String>>) -> Vec<PolicyProblem> {
        let mut found = Vec::new();
        let mut report = |code, message, subject: Option<&str>| {
            found.push(PolicyProblem {
                code,
                message,
                subject: subject.map(Into::into),
            });
        };
        check_machine(&self.machine, None, &mut report);
        if self.tools.len() > MAX_POLICY_TOOLS {
            report("too_many_tools", "at most 16 tools", None);
        }
        let mut tools = BTreeSet::new();
        for tool in &self.tools {
            let name = Some(tool.tool.as_str());
            if valid_identifier(&tool.tool).is_err() {
                report("invalid_tool", "tool needs a bounded identifier", name);
            }
            if !tools.insert(tool.tool.as_str()) {
                report("duplicate_tool", "tool is configured twice", name);
            }
            if !(1..=MAX_POLICY_SESSIONS).contains(&tool.max_sessions) {
                report("invalid_tool_cap", "tool sessions must be 1 to 64", name);
            }
            if !(1..=100).contains(&tool.weight) {
                report("invalid_tool_weight", "tool weight must be 1 to 100", name);
            }
            if tool.accounts.len() > MAX_POLICY_ACCOUNTS {
                report("too_many_accounts", "at most 16 accounts per tool", name);
            }
            let mut accounts = BTreeSet::new();
            for account in &tool.accounts {
                let subject = Some(account.account.as_str());
                if valid_identifier(&account.account).is_err() {
                    report(
                        "invalid_account",
                        "account needs a bounded identifier",
                        subject,
                    );
                }
                if !accounts.insert(account.account.as_str()) {
                    report("duplicate_account", "account is listed twice", subject);
                }
                if account.max_sessions == 0 {
                    report(
                        "invalid_account_cap",
                        "account sessions must be at least 1",
                        subject,
                    );
                }
                if account.max_sessions > tool.max_sessions {
                    report(
                        "account_cap_exceeds_tool",
                        "an account cap cannot exceed its tool cap",
                        subject,
                    );
                }
            }
        }
        if self.targets.len() > MAX_TARGETS {
            report("too_many_targets", "at most 256 targets", None);
        }
        let mut targets = BTreeSet::new();
        for target in &self.targets {
            let id = Some(target.target_id.as_str());
            if valid_token(&target.target_id).is_err() {
                report("invalid_target", "target needs a canonical ULID", id);
            }
            if !targets.insert(target.target_id.as_str()) {
                report("duplicate_target", "target is listed twice", id);
            }
            if declared.is_some_and(|declared| !declared.contains(&target.target_id)) {
                report(
                    "undeclared_target",
                    "policy names a target the manifest does not actively declare",
                    id,
                );
            }
            if target.tools.len() > MAX_POLICY_TOOLS {
                report("too_many_tools", "at most 16 tools", id);
            }
            let mut scoped = BTreeSet::new();
            for scope in &target.tools {
                if !scoped.insert(scope.tool.as_str()) {
                    report("duplicate_tool", "tool is listed twice for a target", id);
                }
                if !tools.contains(scope.tool.as_str()) {
                    report(
                        "unconfigured_tool",
                        "a target tool must be configured under tools",
                        Some(&scope.tool),
                    );
                }
                let models: BTreeSet<_> = scope.models.iter().collect();
                if models.len() != scope.models.len()
                    || scope.models.len() > MAX_POLICY_MODELS
                    || scope
                        .models
                        .iter()
                        .any(|m| !bounded_text(m, 128) || m.chars().any(char::is_whitespace))
                {
                    report("invalid_models", "models must be unique bounded names", id);
                }
            }
            if !(1..=MAX_POLICY_SESSIONS).contains(&target.max_sessions) {
                report("invalid_target_cap", "target sessions must be 1 to 64", id);
            }
            if let Some(machine) = &target.machine {
                check_machine(machine, id, &mut report);
            }
            if target.max_sessions > self.machine_limits(&target.target_id).max_sessions {
                report(
                    "target_cap_exceeds_machine",
                    "a target cap cannot exceed its machine cap",
                    id,
                );
            }
        }
        if !(1..=24 * 60).contains(&self.wind_down.idle_minutes) {
            report(
                "invalid_idle_minutes",
                "idle minutes must be 1 to 1440",
                None,
            );
        }
        if self.wind_down.keep_warm > MAX_POLICY_SESSIONS {
            report("invalid_keep_warm", "keep warm must be 0 to 64", None);
        }
        if !(1..=MAX_SPECULATION_DEPTH).contains(&self.speculation.max_depth) {
            report(
                "invalid_speculation_depth",
                "speculation depth must be 1 to 8",
                None,
            );
        }
        if self.speculation.risk_weight > 100 {
            report("invalid_risk_weight", "risk weight must be 0 to 100", None);
        }
        let tag = &self.speculation.opt_out_tag;
        if !bounded_text(tag, 64) || tag.chars().any(char::is_whitespace) {
            report(
                "invalid_opt_out_tag",
                "opt-out tag must be a bounded tag without spaces",
                None,
            );
        }
        // HS2-ECWG50
        let placement = &self.placement;
        if !(1..=7 * 24 * 60).contains(&placement.aging_minutes) {
            report(
                "invalid_aging_minutes",
                "aging minutes must be 1 to 10080",
                None,
            );
        }
        if placement.cooldown_minutes > 24 * 60 {
            report(
                "invalid_cooldown_minutes",
                "cooldown minutes must be 0 to 1440",
                None,
            );
        }
        if !(1..=24 * 60).contains(&placement.join_minutes) {
            report(
                "invalid_join_minutes",
                "join minutes must be 1 to 1440",
                None,
            );
        }
        if placement.dispatcher_wait_minutes > 24 * 60 {
            report(
                "invalid_dispatcher_wait_minutes",
                "dispatcher wait minutes must be 0 to 1440",
                None,
            );
        }
        match &placement.actor {
            Some(actor) if valid_identifier(actor).is_err() => report(
                "invalid_placement_actor",
                "placement actor needs a bounded identifier",
                Some(actor),
            ),
            None if placement.automatic => report(
                "automatic_without_actor",
                "automatic placement names the grant holder it acts as",
                None,
            ),
            _ => {}
        }
        // HS2-2FG91X
        let mut requiring = BTreeSet::new();
        for entry in &placement.resources {
            let id = Some(entry.target_id.as_str());
            if let Some((code, message)) = entry.problem() {
                report(code, message, id);
            }
            if !requiring.insert(entry.target_id.as_str()) {
                report(
                    "duplicate_resource_target",
                    "a target's resources are listed once",
                    id,
                );
            }
            if !targets.contains(entry.target_id.as_str()) {
                report(
                    "resources_without_target",
                    "resources are required of a target the policy lists",
                    id,
                );
            }
        }
        found
    }

    /// HS2-XHQ1EH: retired fields this policy still holds; each is ignored.
    pub fn ignored_fields(&self) -> Vec<IgnoredField> {
        let mut found = Vec::new();
        if self.approval.is_some() {
            found.push(IgnoredField {
                code: "approval_ignored",
                field: "approval",
                subject: None,
            });
        }
        for target in self
            .targets
            .iter()
            .filter(|target| target.actions.is_some())
        {
            found.push(IgnoredField {
                code: "target_actions_ignored",
                field: "targets[].actions",
                subject: Some(target.target_id.clone()),
            });
        }
        found
    }

    /// The same policy without its retired fields, as it is stored and compared.
    pub fn without_retired(mut self) -> Self {
        self.approval = None;
        for target in &mut self.targets {
            target.actions = None;
        }
        self
    }

    /// Whether the policy's limits permit one action now. Pure; the caller supplies
    /// the load. HS2-XHQ1EH: the policy holds limits only, so it never asks a person.
    /// Observation and reducing activity are never limited here; the caller does not
    /// ask about them. A session start needs the target listed with the tool; other
    /// acting actions are limited only by a target the policy lists as disabled and by
    /// the build-ahead switch.
    pub fn evaluate(&self, question: &PolicyQuestion<'_>) -> PolicyVerdict {
        use PolicyDenial as D;
        let target = self.target(question.target_id);
        if target.is_some_and(|target| !target.enabled) {
            return PolicyVerdict::Denied(D::TargetDisabled);
        }
        if question.provisional && !self.speculation_allowed(question.target_id) {
            return PolicyVerdict::Denied(D::SpeculationNotAllowed);
        }
        if question.action != TargetAction::SessionsManage {
            return PolicyVerdict::Allowed;
        }
        let Some(target) = target else {
            return PolicyVerdict::Denied(D::TargetNotPermitted);
        };
        let Some(session) = question.session else {
            return PolicyVerdict::Denied(D::ToolNotPermitted);
        };
        let Some(scope) = target.tools.iter().find(|tool| tool.tool == session.tool) else {
            return PolicyVerdict::Denied(D::ToolNotPermitted);
        };
        if !scope.models.is_empty()
            && !session
                .model
                .as_ref()
                .is_some_and(|model| scope.models.contains(model))
        {
            return PolicyVerdict::Denied(D::ModelNotPermitted);
        }
        if session.starting {
            if let Err(denial) = self.check_caps(target, &session.tool, question.load) {
                return PolicyVerdict::Denied(denial);
            }
        }
        PolicyVerdict::Allowed
    }

    fn check_caps(
        &self,
        target: &TargetPolicy,
        tool: &str,
        load: Option<&SessionLoad>,
    ) -> Result<(), PolicyDenial> {
        use PolicyDenial as D;
        let load = load.ok_or(D::LoadUnknown)?;
        let tool = self
            .tools
            .iter()
            .find(|configured| configured.tool == tool)
            .ok_or(D::ToolNotPermitted)?;
        if load.target_sessions >= target.max_sessions {
            return Err(D::TargetCap);
        }
        if load.machine_sessions >= self.machine_limits(&target.target_id).max_sessions {
            return Err(D::MachineCap);
        }
        if load.tool_sessions >= tool.max_sessions {
            return Err(D::ToolCap);
        }
        // A tool with configured accounts only runs under one of them.
        match (&load.account, tool.accounts.is_empty()) {
            (None, true) => {}
            (None, false) | (Some(_), true) => return Err(D::AccountNotPermitted),
            (Some(account), false) => {
                let account = tool
                    .accounts
                    .iter()
                    .find(|configured| &configured.account == account)
                    .ok_or(D::AccountNotPermitted)?;
                if load.account_sessions >= account.max_sessions {
                    return Err(D::AccountCap);
                }
            }
        }
        Ok(())
    }
}

fn check_machine(
    machine: &MachineLimits,
    subject: Option<&str>,
    report: &mut impl FnMut(&'static str, &'static str, Option<&str>),
) {
    if !(1..=MAX_POLICY_SESSIONS).contains(&machine.max_sessions) {
        report(
            "invalid_machine_cap",
            "machine sessions must be 1 to 64",
            subject,
        );
    }
    if !(1..=100).contains(&machine.max_cpu_percent) {
        report(
            "invalid_cpu_percent",
            "CPU percent must be 1 to 100",
            subject,
        );
    }
    // A floor above a terabyte is a unit mistake, not a requirement.
    if machine.min_free_ram_mb > 1024 * 1024 || machine.min_free_disk_mb > 1024 * 1024 * 1024 {
        report(
            "invalid_resource_floor",
            "resource floors are megabytes",
            subject,
        );
    }
}

#[derive(Clone)]
pub struct PolicyStore {
    storage: Storage,
}

impl PolicyStore {
    pub fn new(checkout_root: &Path) -> Self {
        Self {
            storage: Storage::shared_named(checkout_root, ".hotsheet2/orchestration-policy.json"),
        }
    }

    fn document(&self) -> Result<Option<PolicyDocument>, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_POLICY_BYTES)? else {
            return Ok(None);
        };
        let document: PolicyDocument = decode(&bytes)?;
        valid_token(&document.revision).map_err(|_| OrchestrationError::Corrupt)?;
        // A stored policy that no longer validates must not be evaluated as if it did.
        if !document.body.problems(None).is_empty() {
            return Err(OrchestrationError::Corrupt);
        }
        Ok(Some(document))
    }

    /// An absent file reads as disabled and is never created by a read.
    pub fn read(&self) -> Result<PolicyView, OrchestrationError> {
        Ok(match self.document()? {
            Some(document) => PolicyView {
                enabled: true,
                revision: Some(document.revision),
                policy: Some(document.body),
            },
            None => PolicyView {
                enabled: false,
                revision: None,
                policy: None,
            },
        })
    }

    /// Replace the policy at the expected revision. Identical content is a no-op that
    /// keeps the revision; every real change rotates it.
    pub fn set(
        &self,
        request: SetPolicy,
        declared: Option<&BTreeSet<String>>,
        tx: &mut Transaction<'_>,
    ) -> Result<PolicyView, OrchestrationError> {
        let request = SetPolicy {
            policy: request.policy.without_retired(),
            ..request
        };
        if let Some(problem) = request.policy.problems(declared).first() {
            return Err(OrchestrationError::Invalid(problem.message));
        }
        let _lock = self.storage.lock()?;
        let previous = self.document()?;
        if previous.as_ref().map(|doc| doc.revision.as_str())
            != request.expected_revision.as_deref()
        {
            return Err(OrchestrationError::Conflict);
        }
        if let Some(previous) = previous
            .as_ref()
            .filter(|doc| doc.body.clone().without_retired() == request.policy)
        {
            return Ok(PolicyView {
                enabled: true,
                revision: Some(previous.revision.clone()),
                policy: Some(request.policy),
            });
        }
        let document = PolicyDocument {
            schema_version: ORCHESTRATION_VERSION,
            revision: fresh_token(tx, previous.as_ref().map(|doc| doc.revision.as_str()))?,
            body: request.policy,
        };
        let mut bytes =
            serde_json::to_vec_pretty(&document).map_err(|_| OrchestrationError::Corrupt)?;
        bytes.push(b'\n');
        self.storage.write(&bytes, MAX_POLICY_BYTES)?;
        Ok(PolicyView {
            enabled: true,
            revision: Some(document.revision),
            policy: Some(document.body),
        })
    }
}
