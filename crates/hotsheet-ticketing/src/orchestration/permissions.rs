//! HS2-XHQ1EH: orchestrator permission requests and remembered rules.
//!
//! An action the orchestrator takes on a target project, and that no remembered rule
//! allows, is a permission request a person answers through the permission bridge
//! (`docs/05` §5.7): **Allow Once**, **Always Allow** or **Deny**. Nothing waits on the
//! answer. The request is one machine-local record per exact operation, so asking
//! again for the same operation files nothing new:
//!
//! - `pending` until a person answers, and `lapsed` after a day without an answer;
//! - `allowed` by Allow Once, until the action it allowed actually happens (`used`);
//!   an action that did not happen leaves it allowed, and it lapses after an hour;
//! - `denied`, which is final for that operation.
//!
//! **Always Allow** also remembers a rule keyed by the orchestration project, the
//! target project, the action and the AI tool. A rule answers every later request with
//! the same key; a person reviews and forgets rules in Settings → Permissions.
//!
//! Records hold identifiers and codes only.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{valid_identifier, valid_token};
use super::storage::{Storage, decode};
use super::*;

pub const MAX_PERMISSION_REQUESTS: usize = 512;
pub const MAX_PERMISSION_RULES: usize = 1024;
pub const MAX_DECISIONS: usize = 256;
/// An unanswered request lapses after a day.
pub const PERMISSION_PENDING_MS: u64 = 24 * 60 * 60 * 1000;
/// Allow Once must be used within an hour of the answer.
pub const PERMISSION_ALLOWED_MS: u64 = 60 * 60 * 1000;
const MAX_PERMISSION_BYTES: usize = 2 * 1024 * 1024;
/// The retired machine-local authority files of HS2-SBQ3T8 and HS2-YCJDEB.
pub const RETIRED_GRANTS_FILE: &str = "orchestration-grants.json";
pub const RETIRED_APPROVALS_FILE: &str = "orchestration-approvals.json";

/// What a permission request or a remembered rule is about. A provisional action asks
/// under `provisional_dispatch`, whatever it would start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAction {
    TicketsWrite,
    WorkStart,
    SessionStart,
    ValidationRun,
    DeploymentsRun,
    ProvisionalDispatch,
}

impl PermissionAction {
    pub const ALL: [Self; 6] = [
        Self::TicketsWrite,
        Self::WorkStart,
        Self::SessionStart,
        Self::ValidationRun,
        Self::DeploymentsRun,
        Self::ProvisionalDispatch,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::TicketsWrite => "tickets_write",
            Self::WorkStart => "work_start",
            Self::SessionStart => "session_start",
            Self::ValidationRun => "validation_run",
            Self::DeploymentsRun => "deployments_run",
            Self::ProvisionalDispatch => "provisional_dispatch",
        }
    }

    /// The permission an acting request needs, or None when it never asks: reads,
    /// gate observation, and stopping or winding down a session.
    pub fn for_request(request: &AccessRequest) -> Option<Self> {
        if request.action.observes() || request.reduces() {
            return None;
        }
        if request.provisional {
            return Some(Self::ProvisionalDispatch);
        }
        Some(match request.action {
            TargetAction::TicketsRead | TargetAction::GatesObserve => return None,
            TargetAction::TicketsWrite => Self::TicketsWrite,
            TargetAction::WorkStart => Self::WorkStart,
            TargetAction::ValidationRun => Self::ValidationRun,
            TargetAction::SessionsManage => Self::SessionStart,
            TargetAction::DeploymentsRun => Self::DeploymentsRun,
        })
    }

    /// Only a session start names a tool in its key.
    pub fn takes_tool(self) -> bool {
        self == Self::SessionStart
    }
}

/// The key a remembered rule answers: orchestration project and target project (the
/// target reference), the action, and the AI tool of a session start.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionKey {
    pub target: TargetRef,
    pub action: PermissionAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

impl PermissionKey {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        self.target.validate()?;
        match (&self.tool, self.action.takes_tool()) {
            (Some(tool), true) => valid_identifier(tool),
            (None, false) => Ok(()),
            (None, true) => Err(OrchestrationError::Invalid(
                "a session start permission names its tool",
            )),
            (Some(_), false) => Err(OrchestrationError::Invalid(
                "only a session start permission names a tool",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Pending,
    /// Allow Once (or the request an Always Allow answered), not yet used.
    Allowed,
    Denied,
    /// The action Allow Once allowed happened.
    Used,
    /// Unanswered for a day, or allowed once and not used within an hour.
    Lapsed,
}

/// One permission request for one exact operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    pub request_id: String,
    #[serde(flatten)]
    pub key: PermissionKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub operation_id: String,
    /// The orchestrator actor that asked.
    pub actor_id: String,
    /// No remembered rule answers this request and Always Allow is not offered: an
    /// action outside what the target configured itself, or one that cannot be undone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub each_time: bool,
    pub state: PermissionState,
    pub requested_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_by: Option<String>,
    /// The answer was Always Allow; `rule_id` names the rule it remembered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_at_ms: Option<u64>,
}

/// A remembered Always Allow answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRule {
    pub rule_id: String,
    #[serde(flatten)]
    pub key: PermissionKey,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// The request whose answer remembered it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// A person's answer, as the permission card offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAnswer {
    AllowOnce,
    AlwaysAllow,
    Deny,
}

/// The operation one permission question concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAsk {
    pub key: PermissionKey,
    pub model: Option<String>,
    pub operation_id: String,
    pub actor_id: String,
    /// Ask for this operation even when a remembered rule covers the key.
    pub each_time: bool,
}

/// The ledger's answer to one question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// A remembered rule allows the key.
    Remembered(PermissionRule),
    /// Allow Once (not used yet) allows this exact operation.
    AllowedOnce(PermissionRequest),
    /// Waiting for a person; `filed` is true when this call filed the request.
    Pending {
        request: PermissionRequest,
        filed: bool,
    },
    /// A person denied this operation.
    Denied(PermissionRequest),
}

/// The result of a person's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Answered {
    pub request: PermissionRequest,
    /// False when the same answer was already recorded.
    pub changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<PermissionRule>,
    /// Other pending requests with the same key the remembered rule answered.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub also_allowed: Vec<String>,
}

/// HS2-XHQ1EH: the retired authority files on this machine. They are never read as
/// authority; this only reports that they exist and how many entries they hold.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RetiredAuthority {
    pub grants_file: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grants: Option<usize>,
    pub approvals_file: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approvals: Option<usize>,
}

impl RetiredAuthority {
    pub fn found(&self) -> bool {
        self.grants_file || self.approvals_file
    }
}

/// Report the retired grant and approval files. A count is given when the file still
/// parses as the old shape; nothing in it is trusted.
pub fn retired_authority(home: &Path) -> RetiredAuthority {
    let count = |name: &str, field: &str| -> (bool, Option<usize>) {
        let storage = Storage::machine_named(home, name);
        match storage.read(16 * 1024 * 1024) {
            Ok(Some(bytes)) => (
                true,
                serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .and_then(|value| {
                        value
                            .get(field)
                            .and_then(|list| list.as_array())
                            .map(Vec::len)
                    }),
            ),
            Ok(None) => (false, None),
            // Present but unreadable still counts as found.
            Err(_) => (true, None),
        }
    };
    let (grants_file, grants) = count(RETIRED_GRANTS_FILE, "grants");
    let (approvals_file, approvals) = count(RETIRED_APPROVALS_FILE, "approvals");
    RetiredAuthority {
        grants_file,
        grants,
        approvals_file,
        approvals,
    }
}

/// HS2-XHQ1EH: a judgment call the orchestrator asked a person on the orchestration
/// ticket, as a `FEEDBACK NEEDED` note with `CHOICE` options. One per operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionAsked {
    pub decision_id: String,
    pub target: TargetRef,
    /// `readiness_override`.
    pub kind: String,
    pub operation_id: String,
    /// The orchestration ticket that holds the note.
    pub connection_id: String,
    pub native_id: String,
    pub note_id: String,
    pub actor_id: String,
    pub asked_at_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionFile {
    schema_version: u32,
    #[serde(default)]
    requests: Vec<PermissionRequest>,
    #[serde(default)]
    rules: Vec<PermissionRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    decisions: Vec<DecisionAsked>,
}

#[derive(Clone)]
pub struct PermissionLedger {
    storage: Storage,
}

impl PermissionLedger {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-permissions.json"),
        }
    }

    fn read_file(&self) -> Result<PermissionFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_PERMISSION_BYTES)? else {
            return Ok(PermissionFile {
                schema_version: ORCHESTRATION_VERSION,
                requests: Vec::new(),
                rules: Vec::new(),
                decisions: Vec::new(),
            });
        };
        let file: PermissionFile = decode(&bytes)?;
        validate_file(&file).map_err(|_| OrchestrationError::Corrupt)?;
        Ok(file)
    }

    fn write_file(&self, file: &PermissionFile) -> Result<(), OrchestrationError> {
        validate_file(file)?;
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_PERMISSION_BYTES)
    }

    /// The answer without writing anything: never files or uses a request.
    pub fn peek(
        &self,
        ask: &PermissionAsk,
        now_ms: u64,
    ) -> Result<Option<Asked>, OrchestrationError> {
        validate_ask(ask)?;
        let file = self.read_file()?;
        if let Some(rule) = rule_for(&file, &ask.key).filter(|_| !ask.each_time) {
            return Ok(Some(Asked::Remembered(rule.clone())));
        }
        Ok(newest(&file, ask)
            .map(|request| current(request.clone(), now_ms))
            .and_then(|request| match request.state {
                PermissionState::Allowed => Some(Asked::AllowedOnce(request)),
                PermissionState::Denied => Some(Asked::Denied(request)),
                PermissionState::Pending => Some(Asked::Pending {
                    request,
                    filed: false,
                }),
                PermissionState::Used | PermissionState::Lapsed => None,
            }))
    }

    /// The gate's question for one exact operation. A remembered rule or an unused
    /// Allow Once answers it; a pending or denied request stays as it is; otherwise
    /// (nothing yet, or the last one was used or lapsed) a new pending request is filed.
    /// Nothing is used here: [`Self::used`] marks Allow Once used once the action
    /// happened.
    pub fn ask(
        &self,
        ask: &PermissionAsk,
        tx: &mut Transaction<'_>,
    ) -> Result<Asked, OrchestrationError> {
        validate_ask(ask)?;
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        if let Some(rule) = rule_for(&file, &ask.key).filter(|_| !ask.each_time) {
            return Ok(Asked::Remembered(rule.clone()));
        }
        if let Some(request) = newest(&file, ask) {
            let request = current(request.clone(), now_ms);
            match request.state {
                PermissionState::Allowed => return Ok(Asked::AllowedOnce(request)),
                PermissionState::Denied => return Ok(Asked::Denied(request)),
                PermissionState::Pending => {
                    return Ok(Asked::Pending {
                        request,
                        filed: false,
                    });
                }
                PermissionState::Used | PermissionState::Lapsed => {}
            }
        }
        prune(&mut file, now_ms);
        if file.requests.len() >= MAX_PERMISSION_REQUESTS {
            return Err(OrchestrationError::Capacity);
        }
        let request = PermissionRequest {
            request_id: tx.token(),
            key: ask.key.clone(),
            model: ask.model.clone(),
            operation_id: ask.operation_id.clone(),
            actor_id: ask.actor_id.clone(),
            each_time: ask.each_time,
            state: PermissionState::Pending,
            requested_at_ms: now_ms,
            answered_at_ms: None,
            answered_by: None,
            rule_id: None,
            used_at_ms: None,
        };
        file.requests.push(request.clone());
        self.write_file(&file)?;
        Ok(Asked::Pending {
            request,
            filed: true,
        })
    }

    /// The action an Allow Once allowed happened. Idempotent; true when it changed.
    /// A request a rule answered, or one that is not allowed, is left as it is.
    pub fn used(
        &self,
        request_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<bool, OrchestrationError> {
        valid_token(request_id)?;
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let Some(request) = file
            .requests
            .iter_mut()
            .find(|request| request.request_id == request_id)
        else {
            return Err(OrchestrationError::NotFound);
        };
        if current(request.clone(), now_ms).state != PermissionState::Allowed {
            return Ok(false);
        }
        request.state = PermissionState::Used;
        request.used_at_ms = Some(now_ms);
        self.write_file(&file)?;
        Ok(true)
    }

    /// A person's answer. Repeating the same answer changes nothing; a request that is
    /// no longer pending cannot be answered differently. Always Allow remembers a rule
    /// and allows every other pending request with the same key.
    pub fn answer(
        &self,
        owner_checkout_id: &str,
        request_id: &str,
        answer: PermissionAnswer,
        answered_by: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<Answered, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(request_id)?;
        if let Some(actor) = &answered_by {
            valid_identifier(actor)?;
        }
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let index = file
            .requests
            .iter()
            .position(|request| {
                request.request_id == request_id
                    && request.key.target.owner_checkout_id == owner_checkout_id
            })
            .ok_or(OrchestrationError::NotFound)?;
        let existing = current(file.requests[index].clone(), now_ms);
        let wanted = match answer {
            PermissionAnswer::AllowOnce | PermissionAnswer::AlwaysAllow => PermissionState::Allowed,
            PermissionAnswer::Deny => PermissionState::Denied,
        };
        let remembered = answer == PermissionAnswer::AlwaysAllow;
        if remembered && existing.each_time {
            return Err(OrchestrationError::Invalid(
                "this permission request is asked each time and cannot be remembered",
            ));
        }
        match existing.state {
            PermissionState::Pending => {}
            // The same answer again: nothing changes. Allow Once already used still
            // counts as the same answer.
            PermissionState::Allowed | PermissionState::Used
                if wanted == PermissionState::Allowed
                    && existing.rule_id.is_some() == remembered =>
            {
                return Ok(Answered {
                    rule: existing
                        .rule_id
                        .as_ref()
                        .and_then(|id| file.rules.iter().find(|rule| &rule.rule_id == id).cloned()),
                    request: existing,
                    changed: false,
                    also_allowed: Vec::new(),
                });
            }
            PermissionState::Denied if wanted == PermissionState::Denied => {
                return Ok(Answered {
                    request: existing,
                    changed: false,
                    rule: None,
                    also_allowed: Vec::new(),
                });
            }
            _ => return Err(OrchestrationError::Conflict),
        }
        let rule = if remembered {
            let key = file.requests[index].key.clone();
            match rule_for(&file, &key) {
                Some(rule) => Some(rule.clone()),
                None => {
                    if file.rules.len() >= MAX_PERMISSION_RULES {
                        return Err(OrchestrationError::Capacity);
                    }
                    let rule = PermissionRule {
                        rule_id: tx.token(),
                        key,
                        created_at_ms: now_ms,
                        created_by: answered_by.clone(),
                        request_id: Some(request_id.into()),
                    };
                    file.rules.push(rule.clone());
                    Some(rule)
                }
            }
        } else {
            None
        };
        let request = &mut file.requests[index];
        request.state = wanted;
        request.answered_at_ms = Some(now_ms);
        request.answered_by = answered_by.clone();
        request.rule_id = rule.as_ref().map(|rule| rule.rule_id.clone());
        let answered = request.clone();
        let mut also_allowed = Vec::new();
        if let Some(rule) = &rule {
            for other in &mut file.requests {
                if other.request_id != request_id
                    && other.key == rule.key
                    && !other.each_time
                    && current(other.clone(), now_ms).state == PermissionState::Pending
                {
                    other.state = PermissionState::Allowed;
                    other.answered_at_ms = Some(now_ms);
                    other.answered_by = answered_by.clone();
                    other.rule_id = Some(rule.rule_id.clone());
                    also_allowed.push(other.request_id.clone());
                }
            }
        }
        self.write_file(&file)?;
        Ok(Answered {
            request: answered,
            changed: true,
            rule,
            also_allowed,
        })
    }

    /// A person remembers a rule ahead of any request, as a headless setup does.
    /// Idempotent by key: the rule already remembered is returned with `false`. Like
    /// Always Allow, it allows every pending request with the key.
    pub fn remember(
        &self,
        key: PermissionKey,
        created_by: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<(PermissionRule, bool, Vec<String>), OrchestrationError> {
        key.validate()?;
        if let Some(actor) = &created_by {
            valid_identifier(actor)?;
        }
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        if let Some(rule) = rule_for(&file, &key) {
            return Ok((rule.clone(), false, Vec::new()));
        }
        if file.rules.len() >= MAX_PERMISSION_RULES {
            return Err(OrchestrationError::Capacity);
        }
        let rule = PermissionRule {
            rule_id: tx.token(),
            key,
            created_at_ms: now_ms,
            created_by: created_by.clone(),
            request_id: None,
        };
        file.rules.push(rule.clone());
        let mut also_allowed = Vec::new();
        for other in &mut file.requests {
            if other.key == rule.key
                && !other.each_time
                && current(other.clone(), now_ms).state == PermissionState::Pending
            {
                other.state = PermissionState::Allowed;
                other.answered_at_ms = Some(now_ms);
                other.answered_by = created_by.clone();
                other.rule_id = Some(rule.rule_id.clone());
                also_allowed.push(other.request_id.clone());
            }
        }
        self.write_file(&file)?;
        Ok((rule, true, also_allowed))
    }

    /// Requests of one orchestration project, oldest first, lapse rules applied.
    pub fn list(
        &self,
        owner_checkout_id: &str,
        now_ms: u64,
    ) -> Result<Vec<PermissionRequest>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(self
            .read_file()?
            .requests
            .into_iter()
            .filter(|request| request.key.target.owner_checkout_id == owner_checkout_id)
            .map(|request| current(request, now_ms))
            .collect())
    }

    /// Every owner with a pending request, for a server that shows them at startup.
    pub fn pending_everywhere(
        &self,
        now_ms: u64,
    ) -> Result<Vec<PermissionRequest>, OrchestrationError> {
        Ok(self
            .read_file()?
            .requests
            .into_iter()
            .map(|request| current(request, now_ms))
            .filter(|request| request.state == PermissionState::Pending)
            .collect())
    }

    pub fn get(
        &self,
        owner_checkout_id: &str,
        request_id: &str,
        now_ms: u64,
    ) -> Result<PermissionRequest, OrchestrationError> {
        self.list(owner_checkout_id, now_ms)?
            .into_iter()
            .find(|request| request.request_id == request_id)
            .ok_or(OrchestrationError::NotFound)
    }

    /// Remembered rules of one orchestration project, oldest first.
    pub fn rules(
        &self,
        owner_checkout_id: &str,
    ) -> Result<Vec<PermissionRule>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(self
            .read_file()?
            .rules
            .into_iter()
            .filter(|rule| rule.key.target.owner_checkout_id == owner_checkout_id)
            .collect())
    }

    /// Every remembered rule on this machine, for Settings → Permissions.
    pub fn all_rules(&self) -> Result<Vec<PermissionRule>, OrchestrationError> {
        Ok(self.read_file()?.rules)
    }

    /// Forget one remembered rule. Needs no binding or reachable target. Forgetting
    /// a rule that is already gone answers `NotFound`.
    pub fn forget(
        &self,
        owner_checkout_id: &str,
        rule_id: &str,
    ) -> Result<PermissionRule, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(rule_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let index = file
            .rules
            .iter()
            .position(|rule| {
                rule.rule_id == rule_id && rule.key.target.owner_checkout_id == owner_checkout_id
            })
            .ok_or(OrchestrationError::NotFound)?;
        let rule = file.rules.remove(index);
        self.write_file(&file)?;
        Ok(rule)
    }
}

impl PermissionLedger {
    /// The decision asked for this exact operation, if any.
    pub fn decision(
        &self,
        target: &TargetRef,
        kind: &str,
        operation_id: &str,
    ) -> Result<Option<DecisionAsked>, OrchestrationError> {
        target.validate()?;
        Ok(self.read_file()?.decisions.into_iter().find(|decision| {
            &decision.target == target
                && decision.kind == kind
                && decision.operation_id == operation_id
        }))
    }

    /// Remember a decision asked. The first record for an operation wins: a second
    /// call returns the one already recorded and writes nothing.
    pub fn record_decision(
        &self,
        record: DecisionAsked,
    ) -> Result<DecisionAsked, OrchestrationError> {
        validate_decision(&record)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        if let Some(existing) = file.decisions.iter().find(|decision| {
            decision.target == record.target
                && decision.kind == record.kind
                && decision.operation_id == record.operation_id
        }) {
            return Ok(existing.clone());
        }
        while file.decisions.len() >= MAX_DECISIONS {
            file.decisions.remove(0);
        }
        file.decisions.push(record.clone());
        self.write_file(&file)?;
        Ok(record)
    }
}

fn validate_decision(decision: &DecisionAsked) -> Result<(), OrchestrationError> {
    valid_token(&decision.decision_id)?;
    decision.target.validate()?;
    valid_identifier(&decision.kind)?;
    valid_identifier(&decision.operation_id)?;
    valid_identifier(&decision.connection_id)?;
    valid_identifier(&decision.native_id)?;
    valid_token(&decision.note_id)?;
    valid_identifier(&decision.actor_id)
}

fn rule_for<'a>(file: &'a PermissionFile, key: &PermissionKey) -> Option<&'a PermissionRule> {
    file.rules.iter().find(|rule| &rule.key == key)
}

/// The newest request for this exact operation decides.
fn newest<'a>(file: &'a PermissionFile, ask: &PermissionAsk) -> Option<&'a PermissionRequest> {
    file.requests
        .iter()
        .rev()
        .find(|request| request.key == ask.key && request.operation_id == ask.operation_id)
}

/// Apply lapse rules without writing: the stored state stays the last explicit event.
fn current(mut request: PermissionRequest, now_ms: u64) -> PermissionRequest {
    let lapsed = match request.state {
        PermissionState::Pending => {
            now_ms.saturating_sub(request.requested_at_ms) >= PERMISSION_PENDING_MS
        }
        // An Allow Once lapses unused; an Always Allow's rule keeps answering anyway.
        PermissionState::Allowed => {
            request.rule_id.is_none()
                && request
                    .answered_at_ms
                    .is_none_or(|at| now_ms.saturating_sub(at) >= PERMISSION_ALLOWED_MS)
        }
        _ => false,
    };
    if lapsed {
        request.state = PermissionState::Lapsed;
    }
    request
}

/// At capacity, drop the oldest finished requests; waiting or usable ones stay.
fn prune(file: &mut PermissionFile, now_ms: u64) {
    while file.requests.len() >= MAX_PERMISSION_REQUESTS {
        let Some(index) = file.requests.iter().position(|request| {
            !matches!(
                current(request.clone(), now_ms).state,
                PermissionState::Pending | PermissionState::Allowed | PermissionState::Denied
            )
        }) else {
            return;
        };
        file.requests.remove(index);
    }
}

fn validate_ask(ask: &PermissionAsk) -> Result<(), OrchestrationError> {
    ask.key.validate()?;
    valid_identifier(&ask.operation_id)?;
    valid_identifier(&ask.actor_id)?;
    if let Some(model) = &ask.model {
        super::actions::valid_model(model)?;
    }
    Ok(())
}

fn validate_file(file: &PermissionFile) -> Result<(), OrchestrationError> {
    if file.schema_version != ORCHESTRATION_VERSION {
        return Err(OrchestrationError::UnsupportedVersion {
            found: file.schema_version.into(),
        });
    }
    if file.requests.len() > MAX_PERMISSION_REQUESTS
        || file.rules.len() > MAX_PERMISSION_RULES
        || file.decisions.len() > MAX_DECISIONS
    {
        return Err(OrchestrationError::Capacity);
    }
    let mut ids = BTreeSet::new();
    for request in &file.requests {
        valid_token(&request.request_id)?;
        request.key.validate()?;
        valid_identifier(&request.operation_id)?;
        valid_identifier(&request.actor_id)?;
        if let Some(model) = &request.model {
            super::actions::valid_model(model)?;
        }
        for actor in request.answered_by.iter() {
            valid_identifier(actor)?;
        }
        if !ids.insert(request.request_id.as_str()) {
            return Err(OrchestrationError::Invalid("duplicate permission request"));
        }
    }
    let mut keys = BTreeSet::new();
    for rule in &file.rules {
        valid_token(&rule.rule_id)?;
        rule.key.validate()?;
        for actor in rule.created_by.iter() {
            valid_identifier(actor)?;
        }
        if !ids.insert(rule.rule_id.as_str()) || !keys.insert(&rule.key) {
            return Err(OrchestrationError::Invalid("duplicate permission rule"));
        }
    }
    for decision in &file.decisions {
        validate_decision(decision)?;
    }
    Ok(())
}
