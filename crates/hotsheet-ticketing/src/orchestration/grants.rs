//! HS2-SBQ3T8: delegated, machine-local authority over one bound target.
//!
//! A grant names one actor, a set of actions and an expiry, and pins the reviewed
//! destination it was issued for. It is never written to the shared manifest or a
//! ticket, carries no credential, and every unknown is a denial.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::contract::{bounded_text, valid_digest, valid_identifier, valid_token};
use super::storage::{Storage, decode};
use super::*;

pub const MAX_GRANTS: usize = 1024;
pub const MAX_GRANTS_PER_TARGET: usize = 32;
pub const MAX_GRANT_TOOLS: usize = 16;
pub const MAX_GRANT_MODELS: usize = 32;
pub const MAX_GRANT_CONCURRENCY: u32 = 64;
/// A grant must be renewed deliberately; ninety days is the longest single lifetime.
pub const MAX_GRANT_LIFETIME_MS: u64 = 90 * 24 * 60 * 60 * 1000;
const MAX_GRANT_BYTES: usize = 2 * 1024 * 1024;

/// What an orchestration project may be delegated to do in a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetAction {
    TicketsRead,
    TicketsWrite,
    WorkStart,
    GatesObserve,
    ValidationRun,
    SessionsManage,
    /// HS2-5JBYPV: request a deployment of a gated commit through the target's script.
    DeploymentsRun,
}

impl TargetAction {
    pub const ALL: [Self; 7] = [
        Self::TicketsRead,
        Self::TicketsWrite,
        Self::WorkStart,
        Self::GatesObserve,
        Self::ValidationRun,
        Self::SessionsManage,
        Self::DeploymentsRun,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::TicketsRead => "tickets_read",
            Self::TicketsWrite => "tickets_write",
            Self::WorkStart => "work_start",
            Self::GatesObserve => "gates_observe",
            Self::ValidationRun => "validation_run",
            Self::SessionsManage => "sessions_manage",
            Self::DeploymentsRun => "deployments_run",
        }
    }
}

/// One AI tool a `sessions_manage` grant covers. No models means any model of the tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolScope {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

/// Limits on the sessions a `sessions_manage` grant may start, stop or wind down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionScope {
    pub tools: Vec<ToolScope>,
    pub max_concurrency: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub grant_id: String,
    pub target: TargetRef,
    pub actor_id: String,
    pub actions: Vec<TargetAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_scope: Option<SessionScope>,
    /// Digest of the reviewed destination at issue time; a rebind makes the grant inert.
    pub destination_key: String,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub expires_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    Active,
    Expired,
    Revoked,
    /// The target is unbound or bound to a destination other than the one granted.
    DestinationChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantView {
    #[serde(flatten)]
    pub grant: Grant,
    pub state: GrantState,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateGrant {
    /// Retain an id across retries; omission mints one.
    #[serde(default)]
    pub grant_id: Option<String>,
    pub actor_id: String,
    pub actions: Vec<TargetAction>,
    #[serde(default)]
    pub session_scope: Option<SessionScope>,
    pub expires_at_ms: u64,
}

/// The session a `sessions_manage` request concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Orchestrator-owned sessions already running on the target for this actor.
    /// Stopping or winding down never counts against the cap; starting does.
    #[serde(default)]
    pub running: u32,
    #[serde(default)]
    pub starting: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessRequest {
    pub actor_id: String,
    pub action: TargetAction,
    #[serde(default)]
    pub session: Option<SessionRequest>,
    #[serde(default)]
    pub operation_id: Option<String>,
    /// HS2-YCJDEB: sessions already running, for the policy's concurrency caps.
    #[serde(default)]
    pub load: Option<SessionLoad>,
    /// The work would start on a prerequisite whose gate is still pending.
    #[serde(default)]
    pub provisional: bool,
    /// The session is a target's integrator or dispatcher rather than a worker.
    #[serde(default)]
    pub role_session: bool,
}

impl AccessRequest {
    pub fn new(actor_id: impl Into<String>, action: TargetAction) -> Self {
        Self {
            actor_id: actor_id.into(),
            action,
            session: None,
            operation_id: None,
            load: None,
            provisional: false,
            role_session: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenialReason {
    // Ordered from "nothing applies" to "almost allowed"; the closest reason is reported.
    NoGrant,
    /// The actor holds grants for this target, but none ever covered the action.
    ActionNotGranted,
    DestinationChanged,
    // The wire spelling equals `code()`, the audit reason and the error code.
    #[serde(rename = "grant_revoked")]
    Revoked,
    #[serde(rename = "grant_expired")]
    Expired,
    SessionScopeRequired,
    ToolNotGranted,
    ModelNotGranted,
    ConcurrencyExceeded,
    /// The target could not be freshly resolved as usable; no grant is consulted.
    TargetUnavailable,
    /// The orchestration policy does not permit the action (HS2-YCJDEB).
    PolicyDenied,
    /// The policy requires a person to approve this decision first (HS2-YCJDEB).
    ApprovalRequired,
}

impl DenialReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::NoGrant => "no_grant",
            Self::DestinationChanged => "destination_changed",
            Self::Revoked => "grant_revoked",
            Self::Expired => "grant_expired",
            Self::ActionNotGranted => "action_not_granted",
            Self::SessionScopeRequired => "session_scope_required",
            Self::ToolNotGranted => "tool_not_granted",
            Self::ModelNotGranted => "model_not_granted",
            Self::ConcurrencyExceeded => "concurrency_exceeded",
            Self::TargetUnavailable => "target_unavailable",
            Self::PolicyDenied => "policy_denied",
            Self::ApprovalRequired => "approval_required",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessDecision {
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<DenialReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
    /// HS2-YCJDEB: the specific policy rule or approval gate behind a
    /// `policy_denied` or `approval_required` answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AccessDecision {
    pub fn denied(reason: DenialReason) -> Self {
        Self {
            allowed: false,
            reason: Some(reason),
            grant_id: None,
            detail: None,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantFile {
    schema_version: u32,
    grants: Vec<Grant>,
}

/// Stable digest of a reviewed destination's identity.
pub fn destination_key(destination: &TargetDestination) -> Result<String, OrchestrationError> {
    destination.validate()?;
    let bytes = serde_json::to_vec(destination).map_err(|_| OrchestrationError::Corrupt)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[derive(Clone)]
pub struct GrantRegistry {
    storage: Storage,
}

impl GrantRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-grants.json"),
        }
    }

    fn read_file(&self) -> Result<GrantFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_GRANT_BYTES)? else {
            return Ok(GrantFile {
                schema_version: ORCHESTRATION_VERSION,
                grants: Vec::new(),
            });
        };
        let file: GrantFile = decode(&bytes)?;
        validate_file(&file).map_err(|_| OrchestrationError::Corrupt)?;
        Ok(file)
    }

    fn write_file(&self, file: &GrantFile) -> Result<(), OrchestrationError> {
        validate_file(file)?;
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_GRANT_BYTES)
    }

    /// Every retained grant for the target with its state against `destination`, the
    /// machine's current reviewed destination (None when unbound).
    pub fn list(
        &self,
        target: &TargetRef,
        destination: Option<&TargetDestination>,
        now_ms: u64,
    ) -> Result<Vec<GrantView>, OrchestrationError> {
        target.validate()?;
        let key = destination.map(destination_key).transpose()?;
        Ok(self
            .read_file()?
            .grants
            .into_iter()
            .filter(|grant| &grant.target == target)
            .map(|grant| GrantView {
                state: state(&grant, key.as_deref(), now_ms),
                grant,
            })
            .collect())
    }

    /// Idempotent by `grant_id`: an identical retry returns the stored grant, a
    /// different body under the same id conflicts. Nothing is written for a no-op.
    pub fn create(
        &self,
        target: &TargetRef,
        request: CreateGrant,
        destination: &TargetDestination,
        created_by: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<GrantView, OrchestrationError> {
        target.validate()?;
        let now_ms = tx.clock.now_ms();
        let destination_key = destination_key(destination)?;
        let mut actions: Vec<_> = request
            .actions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        actions.sort();
        let supplied_id = request.grant_id.is_some();
        let mut grant = Grant {
            grant_id: request.grant_id.unwrap_or_default(),
            target: target.clone(),
            actor_id: request.actor_id,
            actions,
            session_scope: request.session_scope,
            destination_key,
            created_at_ms: now_ms,
            created_by,
            expires_at_ms: request.expires_at_ms,
            revoked_at_ms: None,
            revoked_by: None,
        };
        if grant.expires_at_ms <= now_ms {
            return Err(OrchestrationError::Invalid("grant expiry must be future"));
        }
        if grant.expires_at_ms - now_ms > MAX_GRANT_LIFETIME_MS {
            return Err(OrchestrationError::Invalid(
                "grant lifetime exceeds ninety days",
            ));
        }
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        if supplied_id {
            valid_token(&grant.grant_id)?;
            if let Some(existing) = file.grants.iter().find(|g| g.grant_id == grant.grant_id) {
                let same = existing.target == grant.target
                    && existing.actor_id == grant.actor_id
                    && existing.actions == grant.actions
                    && existing.session_scope == grant.session_scope
                    && existing.destination_key == grant.destination_key
                    && existing.expires_at_ms == grant.expires_at_ms;
                if !same {
                    return Err(OrchestrationError::Conflict);
                }
                return Ok(GrantView {
                    state: state(existing, Some(&grant.destination_key), now_ms),
                    grant: existing.clone(),
                });
            }
        } else {
            grant.grant_id = tx.token();
        }
        validate_grant(&grant)?;
        prune(&mut file, target, now_ms);
        if file.grants.iter().filter(|g| &g.target == target).count() >= MAX_GRANTS_PER_TARGET
            || file.grants.len() >= MAX_GRANTS
        {
            return Err(OrchestrationError::Capacity);
        }
        file.grants.push(grant.clone());
        self.write_file(&file)?;
        Ok(GrantView {
            grant,
            state: GrantState::Active,
        })
    }

    /// Idempotent: revoking an already-revoked grant returns it unchanged, with `false`
    /// for "nothing changed". Needs no binding, manifest or reachable target, so access
    /// can always be withdrawn.
    pub fn revoke(
        &self,
        target: &TargetRef,
        grant_id: &str,
        revoked_by: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<(Grant, bool), OrchestrationError> {
        target.validate()?;
        valid_token(grant_id)?;
        if let Some(actor) = &revoked_by {
            valid_actor(actor)?;
        }
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let grant = file
            .grants
            .iter_mut()
            .find(|grant| grant.grant_id == grant_id && &grant.target == target)
            .ok_or(OrchestrationError::NotFound)?;
        if grant.revoked_at_ms.is_some() {
            return Ok((grant.clone(), false));
        }
        grant.revoked_at_ms = Some(tx.clock.now_ms());
        grant.revoked_by = revoked_by;
        let revoked = grant.clone();
        self.write_file(&file)?;
        Ok((revoked, true))
    }

    /// Pure read. `destination` is the destination the caller has just confirmed; None
    /// (unbound or unverified) denies before any grant is consulted.
    pub fn decide(
        &self,
        target: &TargetRef,
        destination: Option<&TargetDestination>,
        request: &AccessRequest,
        now_ms: u64,
    ) -> Result<AccessDecision, OrchestrationError> {
        target.validate()?;
        valid_actor(&request.actor_id)?;
        if let Some(session) = &request.session {
            valid_identifier(&session.tool)?;
            if let Some(model) = &session.model {
                valid_model(model)?;
            }
        }
        let Some(destination) = destination else {
            return Ok(AccessDecision::denied(DenialReason::TargetUnavailable));
        };
        let key = destination_key(destination)?;
        let mut closest = DenialReason::NoGrant;
        for grant in self
            .read_file()?
            .grants
            .iter()
            .filter(|grant| &grant.target == target && grant.actor_id == request.actor_id)
        {
            match evaluate(grant, &key, request, now_ms) {
                Ok(()) => {
                    return Ok(AccessDecision {
                        allowed: true,
                        reason: None,
                        grant_id: Some(grant.grant_id.clone()),
                        detail: None,
                    });
                }
                Err(reason) => closest = closest.max(reason),
            }
        }
        Ok(AccessDecision::denied(closest))
    }
}

fn state(grant: &Grant, destination_key: Option<&str>, now_ms: u64) -> GrantState {
    if grant.revoked_at_ms.is_some() {
        GrantState::Revoked
    } else if grant.expires_at_ms <= now_ms {
        GrantState::Expired
    } else if destination_key != Some(grant.destination_key.as_str()) {
        GrantState::DestinationChanged
    } else {
        GrantState::Active
    }
}

fn evaluate(
    grant: &Grant,
    destination_key: &str,
    request: &AccessRequest,
    now_ms: u64,
) -> Result<(), DenialReason> {
    // A grant that never covered the action says nothing about why it is denied now.
    if !grant.actions.contains(&request.action) {
        return Err(DenialReason::ActionNotGranted);
    }
    match state(grant, Some(destination_key), now_ms) {
        GrantState::Active => {}
        GrantState::Revoked => return Err(DenialReason::Revoked),
        GrantState::Expired => return Err(DenialReason::Expired),
        GrantState::DestinationChanged => return Err(DenialReason::DestinationChanged),
    }
    if request.action != TargetAction::SessionsManage {
        return Ok(());
    }
    // A session action without its exact tool can never be matched against the scope.
    let (Some(scope), Some(session)) = (&grant.session_scope, &request.session) else {
        return Err(DenialReason::SessionScopeRequired);
    };
    let tool = scope
        .tools
        .iter()
        .find(|tool| tool.tool == session.tool)
        .ok_or(DenialReason::ToolNotGranted)?;
    if !tool.models.is_empty() {
        // A restricted tool needs a named, listed model; "tool default" is not provable.
        match &session.model {
            Some(model) if tool.models.contains(model) => {}
            _ => return Err(DenialReason::ModelNotGranted),
        }
    }
    if session.starting && session.running >= scope.max_concurrency {
        return Err(DenialReason::ConcurrencyExceeded);
    }
    Ok(())
}

/// Drop the oldest inert grants of this target once it is full; active grants stay.
fn prune(file: &mut GrantFile, target: &TargetRef, now_ms: u64) {
    while file.grants.iter().filter(|g| &g.target == target).count() >= MAX_GRANTS_PER_TARGET {
        let Some(index) = file
            .grants
            .iter()
            .enumerate()
            .filter(|(_, grant)| {
                &grant.target == target
                    && (grant.revoked_at_ms.is_some() || grant.expires_at_ms <= now_ms)
            })
            .min_by_key(|(_, grant)| grant.created_at_ms)
            .map(|(index, _)| index)
        else {
            return;
        };
        file.grants.remove(index);
    }
}

fn valid_actor(actor: &str) -> Result<(), OrchestrationError> {
    valid_identifier(actor).map_err(|_| OrchestrationError::Invalid("bounded actor id required"))
}

fn valid_model(model: &str) -> Result<(), OrchestrationError> {
    if !bounded_text(model, 128) || model.chars().any(char::is_whitespace) {
        return Err(OrchestrationError::Invalid("bounded model name required"));
    }
    Ok(())
}

fn validate_grant(grant: &Grant) -> Result<(), OrchestrationError> {
    valid_token(&grant.grant_id)?;
    grant.target.validate()?;
    valid_actor(&grant.actor_id)?;
    valid_digest(&grant.destination_key)?;
    if let Some(actor) = &grant.created_by {
        valid_actor(actor)?;
    }
    if let Some(actor) = &grant.revoked_by {
        valid_actor(actor)?;
    }
    if grant.actions.is_empty() {
        return Err(OrchestrationError::Invalid("grant needs an action"));
    }
    if grant.actions.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(OrchestrationError::Invalid(
            "grant actions must be unique and ordered",
        ));
    }
    if grant.expires_at_ms <= grant.created_at_ms {
        return Err(OrchestrationError::Invalid("grant expiry must be future"));
    }
    let manages = grant.actions.contains(&TargetAction::SessionsManage);
    match (&grant.session_scope, manages) {
        (None, false) => {}
        (None, true) => {
            return Err(OrchestrationError::Invalid(
                "sessions_manage needs a session scope",
            ));
        }
        (Some(_), false) => {
            return Err(OrchestrationError::Invalid(
                "session scope needs sessions_manage",
            ));
        }
        (Some(scope), true) => validate_scope(scope)?,
    }
    Ok(())
}

fn validate_scope(scope: &SessionScope) -> Result<(), OrchestrationError> {
    if scope.tools.is_empty() || scope.tools.len() > MAX_GRANT_TOOLS {
        return Err(OrchestrationError::Invalid(
            "session scope needs one to sixteen tools",
        ));
    }
    if scope.max_concurrency == 0 || scope.max_concurrency > MAX_GRANT_CONCURRENCY {
        return Err(OrchestrationError::Invalid(
            "session scope concurrency must be 1 to 64",
        ));
    }
    let mut tools = BTreeSet::new();
    for tool in &scope.tools {
        valid_identifier(&tool.tool)?;
        if !tools.insert(&tool.tool) {
            return Err(OrchestrationError::Invalid("duplicate session tool"));
        }
        if tool.models.len() > MAX_GRANT_MODELS {
            return Err(OrchestrationError::Capacity);
        }
        let mut models = BTreeSet::new();
        for model in &tool.models {
            valid_model(model)?;
            if !models.insert(model) {
                return Err(OrchestrationError::Invalid("duplicate session model"));
            }
        }
    }
    Ok(())
}

fn validate_file(file: &GrantFile) -> Result<(), OrchestrationError> {
    if file.schema_version != ORCHESTRATION_VERSION {
        return Err(OrchestrationError::UnsupportedVersion {
            found: file.schema_version.into(),
        });
    }
    if file.grants.len() > MAX_GRANTS {
        return Err(OrchestrationError::Capacity);
    }
    let mut ids = BTreeSet::new();
    for grant in &file.grants {
        validate_grant(grant)?;
        if !ids.insert(&grant.grant_id) {
            return Err(OrchestrationError::Invalid("duplicate grant id"));
        }
    }
    Ok(())
}
