//! HS2-XHQ1EH: orchestrator permission requests over the shared runtime.
//!
//! A request the gate files is shown to people through a [`PermissionPrompt`], which
//! the server implements with its permission bridge (`docs/05` §5.7), so it appears as
//! the ordinary permission card with Allow Once, Always Allow and Deny. Without a
//! server the request waits in this machine's ledger and is answered headlessly with
//! `orchestration permission answer`. Either way the answer goes through
//! [`OrchestrationRuntime::permission_answer`], is audited, and wakes the scheduler.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    Answered, AuditOutcome, AuditRecord, DecisionAsked, DeclarationState, PermissionAction,
    PermissionAnswer, PermissionRequest, PermissionRule, PermissionState, RetiredAuthority,
    retired_authority,
};
use serde::{Deserialize, Serialize};

use super::*;

/// One orchestrator request as people see it: the record and the words for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptRequest {
    #[serde(flatten)]
    pub request: PermissionRequest,
    /// The target project's label from the declaration.
    pub target_label: String,
    /// The orchestration project's checkout alias.
    pub owner_label: String,
    /// One sentence naming the action, for example "Start a codex session in API".
    pub summary: String,
}

/// Where a waiting request is shown. The server's implementation enqueues it in the
/// permission bridge with the `orchestrator` requester kind and no waiter.
pub trait PermissionPrompt: Send + Sync {
    /// Show a waiting request. Idempotent by request id: one already shown stays one.
    fn raise(&self, request: &PromptRequest);
    /// The request was answered or withdrawn elsewhere: stop showing it.
    fn settle(&self, request_id: &str, decision: &str);
}

/// A headless process shows nothing; the ledger keeps the request.
pub struct NoPrompt;
impl PermissionPrompt for NoPrompt {
    fn raise(&self, _request: &PromptRequest) {}
    fn settle(&self, _request_id: &str, _decision: &str) {}
}

/// A remembered rule as Settings → Permissions lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleView {
    #[serde(flatten)]
    pub rule: PermissionRule,
    pub target_label: String,
    pub owner_label: String,
    pub summary: String,
}

/// An orchestration project's requests and rules, and any retired authority found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionList {
    pub requests: Vec<PromptRequest>,
    pub rules: Vec<RuleView>,
    /// HS2-0RAZ1J: judgment calls asked on the goal tickets, waiting or answered, by
    /// identifiers: the note a person answers and, once answered, the response.
    pub decisions: Vec<DecisionAsked>,
    /// HS2-XHQ1EH: grant and approval files from before this model. They are never
    /// read as authority; remove them once nothing needs their history.
    pub retired: RetiredAuthority,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerPermission {
    pub answer: PermissionAnswer,
}

/// A person remembers a rule ahead of any request (headless setup).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RememberPermission {
    pub target_id: String,
    pub action: PermissionAction,
    /// The AI tool, for `session_start` only.
    #[serde(default)]
    pub tool: Option<String>,
}

/// The rule remembered and what it settled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Remembered {
    #[serde(flatten)]
    pub rule: RuleView,
    /// False when the rule was already remembered.
    pub created: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub also_allowed: Vec<String>,
}

/// HS2-E16FWJ: the directory a runtime touches whenever its permission ledger changes
/// (a request filed, answered, remembered or forgotten), so a server on this machine
/// shows a request a headless CLI filed, or settles one it answered, without waiting
/// for a client to read the pending permissions.
pub const PERMISSION_SIGNAL_DIRECTORY: &str = "orchestration-permission-signal";

/// The permission signal's directory under a machine home.
pub fn permission_signal_directory(home: &std::path::Path) -> std::path::PathBuf {
    home.join(PERMISSION_SIGNAL_DIRECTORY)
}

/// The permission signal's current token; `None` before the first change.
pub fn permission_signal_token(home: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(permission_signal_directory(home).join("token")).ok()
}

/// HS2-E16FWJ: waits for the permission signal to change. It holds one platform file
/// watcher for its lifetime; every wait also re-checks the token locally at least once
/// a second, so a missed file event cannot strand it. It never makes a request.
pub struct PermissionSignalWaiter {
    home: std::path::PathBuf,
    seen: Option<String>,
    waiter: hotsheet_jobs::ChangeWaiter,
}

impl std::fmt::Debug for PermissionSignalWaiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionSignalWaiter")
            .field("seen", &self.seen)
            .finish_non_exhaustive()
    }
}

impl PermissionSignalWaiter {
    /// Start watching `home`'s permission signal; a change already written counts as
    /// seen.
    pub fn new(home: &std::path::Path) -> std::io::Result<Self> {
        let waiter = hotsheet_jobs::ChangeWaiter::new(&permission_signal_directory(home))?;
        Ok(Self {
            home: home.to_path_buf(),
            seen: permission_signal_token(home),
            waiter,
        })
    }

    /// Wait at most `timeout` for the signal to change since the last change seen.
    /// Returns whether it changed.
    pub fn wait(&mut self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let token = permission_signal_token(&self.home);
            if token != self.seen {
                self.seen = token;
                return true;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            self.waiter.wait(remaining);
        }
    }
}

/// The sentence for an action on a target, by identifiers and labels only.
pub fn permission_summary(
    action: PermissionAction,
    tool: Option<&str>,
    model: Option<&str>,
    target_label: &str,
) -> String {
    let tool = tool.unwrap_or("AI");
    let model = model.map(|model| format!(" ({model})")).unwrap_or_default();
    match action {
        PermissionAction::TicketsWrite => format!("Create or change tickets in {target_label}"),
        PermissionAction::WorkStart => format!("Start work on a ticket in {target_label}"),
        PermissionAction::SessionStart => {
            format!("Start a {tool}{model} session in {target_label}")
        }
        PermissionAction::ValidationRun => format!("Run a validation step in {target_label}"),
        PermissionAction::DeploymentsRun => format!("Run a deployment in {target_label}"),
        PermissionAction::ProvisionalDispatch => {
            format!("Build ahead of a pending check in {target_label}")
        }
    }
}

impl OrchestrationRuntime {
    /// HS2-XHQ1EH: show this runtime's waiting requests through `prompt`.
    #[must_use]
    pub fn with_permission_prompt(mut self, prompt: Arc<dyn PermissionPrompt>) -> Self {
        self.prompt = prompt;
        self
    }

    /// HS2-E16FWJ: the directory this runtime's permission signal is written in.
    pub fn permission_signal_dir(&self) -> std::path::PathBuf {
        permission_signal_directory(&self.home)
    }

    /// HS2-E16FWJ: the permission signal's current token; `None` before any change.
    pub fn permission_signal_now(&self) -> Option<String> {
        permission_signal_token(&self.home)
    }

    /// HS2-E16FWJ: a waiter on this runtime's permission signal.
    pub fn permission_signal_waiter(&self) -> std::io::Result<PermissionSignalWaiter> {
        PermissionSignalWaiter::new(&self.home)
    }

    /// HS2-E16FWJ: tell every server on this machine that the permission ledger
    /// changed. Best effort: a missed signal is caught up by the next client read.
    pub(super) fn permission_signal(&self) {
        let directory = permission_signal_directory(&self.home);
        if std::fs::create_dir_all(&directory).is_ok() {
            let _ = std::fs::write(directory.join("token"), ulid::Ulid::new().to_string());
        }
    }

    /// Labels of the owner and of one of its targets. A target that is no longer
    /// declared keeps its id as the label.
    fn permission_labels(&self, owner_id: &str, target_id: &str) -> (String, String) {
        let owner_label = self
            .checkouts
            .describe(owner_id)
            .map(|checkout| checkout.alias)
            .ok()
            .filter(|alias| !alias.is_empty())
            .unwrap_or_else(|| owner_id.to_owned());
        let target_label = self
            .declarations(owner_id)
            .ok()
            .and_then(|view| {
                view.targets
                    .into_iter()
                    .find(|target| target.target_id == target_id)
                    .filter(|target| target.state == DeclarationState::Active)
                    .map(|target| target.label)
            })
            .unwrap_or_else(|| target_id.to_owned());
        (owner_label, target_label)
    }

    pub(super) fn prompt_request(&self, request: &PermissionRequest) -> PromptRequest {
        let (owner_label, target_label) = self.permission_labels(
            &request.key.target.owner_checkout_id,
            &request.key.target.target_id,
        );
        PromptRequest {
            summary: permission_summary(
                request.key.action,
                request.key.tool.as_deref(),
                request.model.as_deref(),
                &target_label,
            ),
            request: request.clone(),
            target_label,
            owner_label,
        }
    }

    fn rule_view(&self, rule: PermissionRule) -> RuleView {
        let (owner_label, target_label) = self.permission_labels(
            &rule.key.target.owner_checkout_id,
            &rule.key.target.target_id,
        );
        RuleView {
            summary: permission_summary(
                rule.key.action,
                rule.key.tool.as_deref(),
                None,
                &target_label,
            ),
            rule,
            target_label,
            owner_label,
        }
    }

    /// Show a waiting request through the prompt. Never fails the gate.
    pub(super) fn show_permission(&self, _target: &TargetRef, request: &PermissionRequest) {
        self.prompt.raise(&self.prompt_request(request));
    }

    /// An orchestration project's permission requests (oldest first) and remembered
    /// rules. Reads this machine's ledger only.
    pub fn permission_list(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<PermissionList, OrchestrationError> {
        let ledger = self.permission_ledger();
        Ok(PermissionList {
            requests: ledger
                .list(owner_id, tx.clock.now_ms())?
                .iter()
                .map(|request| self.prompt_request(request))
                .collect(),
            rules: ledger
                .rules(owner_id)?
                .into_iter()
                .map(|rule| self.rule_view(rule))
                .collect(),
            decisions: ledger.decisions(owner_id)?,
            retired: retired_authority(&self.home),
        })
    }

    /// Every waiting request on this machine, as a server shows them after it starts.
    pub fn pending_permissions(&self, now_ms: u64) -> Vec<PromptRequest> {
        self.permission_ledger()
            .pending_everywhere(now_ms)
            .unwrap_or_default()
            .iter()
            .map(|request| self.prompt_request(request))
            .collect()
    }

    /// Every remembered rule on this machine, for Settings → Permissions.
    pub fn permission_rules_everywhere(&self) -> Vec<RuleView> {
        self.permission_ledger()
            .all_rules()
            .unwrap_or_default()
            .into_iter()
            .map(|rule| self.rule_view(rule))
            .collect()
    }

    /// A person's answer to one request: Allow Once, Always Allow (which remembers a
    /// rule and allows every waiting request with the same key) or Deny. An AI actor
    /// is refused. The answer is audited, the prompt stops showing every request it
    /// settled, and the scheduler is woken.
    pub fn permission_answer(
        &self,
        owner_id: &str,
        request_id: &str,
        answer: PermissionAnswer,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Answered, OrchestrationError> {
        access::person(actor)?;
        let answered = self.permission_ledger().answer(
            owner_id,
            request_id,
            answer,
            actor.and_then(|actor| actor.id.clone()),
            tx,
        )?;
        if answered.changed {
            let (decision, outcome) = match answer {
                PermissionAnswer::Deny => ("deny:once", AuditOutcome::Rejected),
                PermissionAnswer::AllowOnce => ("allow:once", AuditOutcome::Approved),
                PermissionAnswer::AlwaysAllow => ("allow:always", AuditOutcome::Approved),
            };
            let request = &answered.request;
            self.audit_record(
                owner_id,
                AuditRecord {
                    at_ms: tx.clock.now_ms(),
                    actor_role: access::role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    target_id: Some(request.key.target.target_id.clone()),
                    action: "permission_answer".into(),
                    operation_id: Some(request.operation_id.clone()),
                    outcome,
                    reason: Some(request.key.action.code().into()),
                    permission_id: Some(request.request_id.clone()),
                },
            )?;
            if let Some(rule) = &answered.rule {
                self.audit_record(
                    owner_id,
                    AuditRecord {
                        at_ms: tx.clock.now_ms(),
                        actor_role: access::role_code(actor),
                        actor_id: actor.and_then(|actor| actor.id.clone()),
                        target_id: Some(rule.key.target.target_id.clone()),
                        action: "permission_remember".into(),
                        operation_id: None,
                        outcome: AuditOutcome::Changed,
                        reason: Some(rule.key.action.code().into()),
                        permission_id: Some(rule.rule_id.clone()),
                    },
                )?;
            }
            self.prompt.settle(request_id, decision);
            for other in &answered.also_allowed {
                self.prompt.settle(other, "allow:always");
            }
            // HS2-XHQ1EH: an answer is news to a scheduler that was waiting on it.
            Self::scheduler_wake_at(&self.home);
            self.permission_signal();
        }
        Ok(answered)
    }

    /// A person remembers a rule ahead of any request: the headless counterpart of
    /// Always Allow, for setup scripts and fixtures. The target must be declared
    /// (adding the project is the consent the rule builds on). An AI actor is refused.
    pub fn permission_remember(
        &self,
        owner_id: &str,
        request: RememberPermission,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Remembered, OrchestrationError> {
        access::person(actor)?;
        let target = Self::target_ref(owner_id, &request.target_id)?;
        if !self.declared(owner_id)?.contains(&request.target_id) {
            return Err(OrchestrationError::NotFound);
        }
        let (rule, created, also_allowed) = self.permission_ledger().remember(
            hotsheet_ticketing::orchestration::PermissionKey {
                target,
                action: request.action,
                tool: request.tool,
            },
            actor.and_then(|actor| actor.id.clone()),
            tx,
        )?;
        if created {
            self.audit_record(
                owner_id,
                AuditRecord {
                    at_ms: tx.clock.now_ms(),
                    actor_role: access::role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    target_id: Some(rule.key.target.target_id.clone()),
                    action: "permission_remember".into(),
                    operation_id: None,
                    outcome: AuditOutcome::Changed,
                    reason: Some(rule.key.action.code().into()),
                    permission_id: Some(rule.rule_id.clone()),
                },
            )?;
            for other in &also_allowed {
                self.prompt.settle(other, "allow:always");
            }
            // A waiting request it allowed is news to a scheduler.
            if !also_allowed.is_empty() {
                Self::scheduler_wake_at(&self.home);
            }
            self.permission_signal();
        }
        Ok(Remembered {
            rule: self.rule_view(rule),
            created,
            also_allowed,
        })
    }

    /// Forget one remembered rule. Withdrawing authority needs an identified actor
    /// and no binding, manifest or reachable target, so it always works.
    pub fn permission_forget(
        &self,
        owner_id: &str,
        rule_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<PermissionRule, OrchestrationError> {
        if actor.and_then(|actor| actor.id.as_ref()).is_none() && !access::is_person(actor) {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to forget a permission rule",
            ));
        }
        let rule = self.permission_ledger().forget(owner_id, rule_id)?;
        self.audit_record(
            owner_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: Some(rule.key.target.target_id.clone()),
                action: "permission_forget".into(),
                operation_id: None,
                outcome: AuditOutcome::Revoked,
                reason: Some(rule.key.action.code().into()),
                permission_id: Some(rule.rule_id.clone()),
            },
        )?;
        self.permission_signal();
        Ok(rule)
    }

    /// Show every waiting request through the prompt, as a server does when it starts
    /// or when a client reads the bridge. Idempotent.
    pub fn show_waiting_permissions(&self, now_ms: u64) {
        for request in self.pending_permissions(now_ms) {
            self.prompt.raise(&request);
        }
    }

    /// How a request that no longer waits was settled, in the bridge's
    /// `decision:scope` vocabulary; `external` when it lapsed or is unknown.
    pub fn permission_decision(
        &self,
        owner_id: &str,
        request_id: &str,
        now_ms: u64,
    ) -> &'static str {
        match self.permission_ledger().get(owner_id, request_id, now_ms) {
            Ok(request) => match (request.state, request.rule_id.is_some()) {
                (PermissionState::Denied, _) => "deny:once",
                (PermissionState::Allowed | PermissionState::Used, true) => "allow:always",
                (PermissionState::Allowed | PermissionState::Used, false) => "allow:once",
                _ => "external",
            },
            Err(_) => "external",
        }
    }

    /// Whether a request is still waiting; a settled one is not shown again.
    pub fn permission_waiting(&self, owner_id: &str, request_id: &str, now_ms: u64) -> bool {
        self.permission_ledger()
            .get(owner_id, request_id, now_ms)
            .is_ok_and(|request| request.state == PermissionState::Pending)
    }
}
