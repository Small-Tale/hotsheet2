//! HS2-GDJXGZ: AI sessions the orchestrator starts on the servers that host its targets.
//!
//! Two records, one per side. The orchestrating machine keeps what it asked for and
//! what the host answered ([`SessionRegistry`]). The hosting machine keeps which of its
//! terminals an orchestrator started and who owns each now ([`HostedSessionRegistry`]),
//! so a repeated start never launches twice and a session a person took over is never
//! wound down on an orchestrator's say-so. Both decisions about stopping are pure.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{bounded_text, valid_identifier, valid_token};
use super::storage::{Storage, decode};
use super::*;

pub const MAX_SESSIONS: usize = 1024;
pub const MAX_SESSION_HISTORY: usize = 32;
pub const MAX_HOSTED_SESSIONS: usize = 512;
/// The longest project name a session label carries.
pub const MAX_SESSION_PROJECT_CHARS: usize = 60;
const MAX_SESSION_BYTES: usize = 8 * 1024 * 1024;

/// The part a started session plays in its target's existing role model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    #[default]
    Worker,
    Dispatcher,
    Integrator,
}

impl SessionRole {
    pub const ALL: [Self; 3] = [Self::Worker, Self::Dispatcher, Self::Integrator];

    pub fn code(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Dispatcher => "dispatcher",
            Self::Integrator => "integrator",
        }
    }

    /// The role skill the session runs to join its target's pool.
    pub fn skill(self) -> &'static str {
        match self {
            Self::Worker => "hotsheet-worker",
            Self::Dispatcher => "hotsheet-dispatcher",
            Self::Integrator => "hotsheet-integrator",
        }
    }

    /// A target has at most one dispatcher and one integrator; the policy may ask a
    /// person before either is started.
    pub fn is_role_session(self) -> bool {
        !matches!(self, Self::Worker)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Recorded before the host was asked; nothing is known to run there.
    Requested,
    /// The host started the session.
    Running,
    /// Stopped on purpose: wound down at a safe point, or stopped by a person.
    Stopped,
    /// The host refused the start, or it could not be made.
    Failed,
    /// The host no longer has the session and nobody stopped it: it crashed or was
    /// closed at the host.
    Lost,
}

impl SessionState {
    pub const ALL: [Self; 5] = [
        Self::Requested,
        Self::Running,
        Self::Stopped,
        Self::Failed,
        Self::Lost,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Lost => "lost",
        }
    }

    /// The session still occupies, or may occupy, its machine and tool.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Requested | Self::Running)
    }

    pub fn is_terminal(self) -> bool {
        !self.is_active()
    }

    /// The only edges a session may take. A finished session never restarts: its
    /// replacement is a new session that names it.
    pub fn may_become(self, next: Self) -> bool {
        use SessionState::{Failed, Lost, Requested, Running, Stopped};
        matches!(
            (self, next),
            (Requested, Running | Stopped | Failed) | (Running, Stopped | Lost)
        )
    }
}

/// Who decides about a session now. Ownership only ever moves to a person.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOwner {
    #[default]
    Orchestrator,
    Human,
}

/// A deliberate act recorded in a session's history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionControl {
    /// The orchestrator stopped the session at a safe point.
    WindDown,
    /// A person stopped it.
    Stop,
    /// A person paused it: it keeps running and the orchestrator leaves it alone.
    Pause,
    /// A person took it over.
    Takeover,
}

impl SessionControl {
    pub const ALL: [Self; 4] = [Self::WindDown, Self::Stop, Self::Pause, Self::Takeover];

    pub fn code(self) -> &'static str {
        match self {
            Self::WindDown => "wind_down",
            Self::Stop => "stop",
            Self::Pause => "pause",
            Self::Takeover => "takeover",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionError {
    NotFound,
    /// The session is finished; it cannot be stopped, taken over or changed.
    NotActive,
    InvalidTransition,
    /// A person owns the session; the orchestrator does not act on it.
    HumanOwned,
    /// The session holds a claim, uncommitted changes or a pending handoff, or is in
    /// the middle of a turn, or one of those could not be established.
    NotSafe,
    /// Only a person may stop a session that is not at a safe point.
    ForceNotPermitted,
    /// No terminal host could be reached to start, list or stop sessions.
    HostUnavailable,
    /// The target's machine refused the start (its admission answer says why).
    AdmissionRefused,
    /// The target already has a live session in that role.
    RoleActive,
    /// The host could not launch the tool.
    StartFailed,
    /// HS2-DG0YPA: another `requested` or `running` session still holds the run's work.
    RunHeld,
}

impl SessionError {
    pub const ALL: [Self; 11] = [
        Self::NotFound,
        Self::NotActive,
        Self::InvalidTransition,
        Self::HumanOwned,
        Self::NotSafe,
        Self::ForceNotPermitted,
        Self::HostUnavailable,
        Self::AdmissionRefused,
        Self::RoleActive,
        Self::StartFailed,
        Self::RunHeld,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::NotFound => "session_not_found",
            Self::NotActive => "session_not_active",
            Self::InvalidTransition => "session_invalid_transition",
            Self::HumanOwned => "session_human_owned",
            Self::NotSafe => "session_not_safe",
            Self::ForceNotPermitted => "session_force_not_permitted",
            Self::HostUnavailable => "session_host_unavailable",
            Self::AdmissionRefused => "session_admission_refused",
            Self::RoleActive => "session_role_active",
            Self::StartFailed => "session_start_failed",
            Self::RunHeld => "session_run_held",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|error| error.code() == code)
    }
}

// ---- Safe points ----

/// What the host established about one session. `None` means it could not find out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafePointFacts {
    /// Whether the session's process is still running.
    pub alive: Option<bool>,
    /// Whether the tool is in the middle of a turn.
    pub busy: Option<bool>,
    /// Live ticket claims held under the session's worker id.
    pub held_claims: Option<u32>,
    /// Worktrees of the checkout with uncommitted changes.
    pub dirty_worktrees: Option<u32>,
    /// Assignments of the session that are not finished: work not yet handed off, or a
    /// handoff the integrator has not finished.
    pub pending_handoffs: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafePointBlocker {
    MidTurn,
    HeldClaim,
    UncommittedChanges,
    PendingHandoff,
    /// The host could not say whether the session runs or is mid-turn.
    SessionUnknown,
    ClaimsUnknown,
    WorktreeUnknown,
    HandoffUnknown,
}

impl SafePointBlocker {
    pub const ALL: [Self; 8] = [
        Self::MidTurn,
        Self::HeldClaim,
        Self::UncommittedChanges,
        Self::PendingHandoff,
        Self::SessionUnknown,
        Self::ClaimsUnknown,
        Self::WorktreeUnknown,
        Self::HandoffUnknown,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::MidTurn => "mid_turn",
            Self::HeldClaim => "held_claim",
            Self::UncommittedChanges => "uncommitted_changes",
            Self::PendingHandoff => "pending_handoff",
            Self::SessionUnknown => "session_unknown",
            Self::ClaimsUnknown => "claims_unknown",
            Self::WorktreeUnknown => "worktree_unknown",
            Self::HandoffUnknown => "handoff_unknown",
        }
    }
}

/// Whether a session may be wound down now, and everything that says no.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafePoint {
    pub safe: bool,
    /// Every reason at once, in a fixed order, so one answer shows the whole picture.
    pub blockers: Vec<SafePointBlocker>,
    pub facts: SafePointFacts,
}

/// A session is at a safe point when it is between turns and holds nothing that
/// stopping it would strand. Anything unknown counts against it. A session whose
/// process is already gone has nothing left to interrupt.
pub fn decide_safe_point(facts: &SafePointFacts) -> SafePoint {
    use SafePointBlocker as B;
    let mut blockers = Vec::new();
    if facts.alive != Some(false) {
        match (facts.alive, facts.busy) {
            (None, _) | (_, None) => blockers.push(B::SessionUnknown),
            (_, Some(true)) => blockers.push(B::MidTurn),
            (_, Some(false)) => {}
        }
        let mut count = |value: Option<u32>, held: B, unknown: B| match value {
            None => blockers.push(unknown),
            Some(0) => {}
            Some(_) => blockers.push(held),
        };
        count(facts.held_claims, B::HeldClaim, B::ClaimsUnknown);
        count(
            facts.dirty_worktrees,
            B::UncommittedChanges,
            B::WorktreeUnknown,
        );
        count(facts.pending_handoffs, B::PendingHandoff, B::HandoffUnknown);
    }
    blockers.sort_by_key(|blocker| B::ALL.iter().position(|known| known == blocker));
    SafePoint {
        safe: blockers.is_empty(),
        blockers,
        facts: *facts,
    }
}

/// Who asks for a stop and how.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StopRequest {
    /// Stop even when the session is not at a safe point.
    pub force: bool,
    /// A person asked, not the orchestrator.
    pub human: bool,
}

/// Whether a stop may go ahead. The orchestrator stops only its own sessions and only
/// at a safe point; killing mid-turn is a person's explicit request.
pub fn decide_stop(
    owner: SessionOwner,
    request: StopRequest,
    safe: &SafePoint,
) -> Result<(), SessionError> {
    if request.force && !request.human {
        return Err(SessionError::ForceNotPermitted);
    }
    if owner == SessionOwner::Human && !request.human {
        return Err(SessionError::HumanOwned);
    }
    if !request.force && !safe.safe {
        return Err(SessionError::NotSafe);
    }
    Ok(())
}

/// The drawer label of an orchestrator-started session: the orchestration project and
/// the run it works for, or its role when it serves no single run.
pub fn session_label(project: &str, run_id: Option<&str>, role: SessionRole) -> String {
    let project: String = project
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_SESSION_PROJECT_CHARS)
        .collect();
    let project = project.trim();
    let project = if project.is_empty() {
        "Orchestration"
    } else {
        project
    };
    match run_id {
        // The tail of a ULID is its random part, which tells runs apart at a glance.
        Some(run) => {
            let tail = &run[run.len().saturating_sub(8)..];
            format!("{project}: run {tail}")
        }
        None => format!("{project}: {}", role.code()),
    }
}

fn valid_model(value: &str) -> Result<(), OrchestrationError> {
    if !bounded_text(value, 128) || value.chars().any(char::is_whitespace) {
        return Err(OrchestrationError::Invalid("bounded model name required"));
    }
    Ok(())
}

// ---- Orchestrating side ----

/// Where the host says the session lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedRef {
    /// The machine that hosts it.
    pub server_id: String,
    /// The host's id for the session: its terminal in that server's drawer.
    pub session_id: String,
    /// The worker id the session's claims and assignments carry.
    pub worker_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTransition {
    pub at_ms: u64,
    /// Absent for the first entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<SessionState>,
    pub to: SessionState,
    /// A stable code, never free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<SessionControl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestratedSession {
    pub session_id: String,
    pub owner_checkout_id: String,
    /// The caller's idempotency key for this start.
    pub operation_id: String,
    pub target_id: String,
    #[serde(default)]
    pub role: SessionRole,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The run the session was started for, when it serves one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The lost or failed session this one replaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    pub state: SessionState,
    #[serde(default)]
    pub owner: SessionOwner,
    /// A person paused it: it is theirs and the orchestrator gives it nothing new.
    #[serde(default)]
    pub paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosted: Option<HostedRef>,
    /// HS2-2DPGDY: how the session was last known to have been asked to take up its
    /// role, as its host answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_prompt: Option<RolePrompt>,
    /// HS2-2DPGDY: when this machine first learned that the session registered in the
    /// target's coordination pool in its role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined_at_ms: Option<u64>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub history: Vec<SessionTransition>,
}

/// What a caller supplies to start a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    pub operation_id: String,
    pub target_id: String,
    pub role: SessionRole,
    pub tool: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub account: Option<String>,
    pub run_id: Option<String>,
    pub replaces: Option<String>,
}

/// A change to an existing session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionUpdate {
    pub to: Option<SessionState>,
    pub reason: Option<String>,
    pub actor_id: Option<String>,
    pub control: Option<SessionControl>,
    /// Only [`SessionOwner::Human`] changes anything: ownership never returns.
    pub owner: Option<SessionOwner>,
    pub paused: Option<bool>,
    pub hosted: Option<HostedRef>,
    /// HS2-2DPGDY: the role prompt as its host now answers.
    pub role_prompt: Option<RolePrompt>,
    /// HS2-2DPGDY: the session was seen in the pool; only the first time is kept.
    pub joined_at_ms: Option<u64>,
    /// HS2-YC2P7K: the run the session now works, when the scheduler gave it one.
    pub run_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionFile {
    schema_version: u32,
    sessions: Vec<OrchestratedSession>,
}

#[derive(Clone)]
pub struct SessionRegistry {
    storage: Storage,
}

impl SessionRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-sessions.json"),
        }
    }

    fn read_file(&self) -> Result<SessionFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_SESSION_BYTES)? else {
            return Ok(SessionFile {
                schema_version: ORCHESTRATION_VERSION,
                sessions: Vec::new(),
            });
        };
        let file: SessionFile = decode(&bytes)?;
        for session in &file.sessions {
            validate(session).map_err(|_| OrchestrationError::Corrupt)?;
        }
        Ok(file)
    }

    fn write_file(&self, file: &SessionFile) -> Result<(), OrchestrationError> {
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_SESSION_BYTES)
    }

    pub fn get(
        &self,
        owner_checkout_id: &str,
        session_id: &str,
    ) -> Result<OrchestratedSession, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(session_id)?;
        self.read_file()?
            .sessions
            .into_iter()
            .find(|session| {
                session.session_id == session_id && session.owner_checkout_id == owner_checkout_id
            })
            .ok_or(OrchestrationError::Session(SessionError::NotFound))
    }

    /// Sessions of one owning checkout, oldest first.
    pub fn list(
        &self,
        owner_checkout_id: &str,
        active_only: bool,
    ) -> Result<Vec<OrchestratedSession>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(self
            .read_file()?
            .sessions
            .into_iter()
            .filter(|session| session.owner_checkout_id == owner_checkout_id)
            .filter(|session| !active_only || session.state.is_active())
            .collect())
    }

    /// The sessions this owner started that still occupy a machine and a tool, counted
    /// the way the policy's caps ask. A session a person took over still runs, so it
    /// still counts until it stops. `server_id` is the machine the target lives on.
    pub fn load(
        &self,
        owner_checkout_id: &str,
        target_id: &str,
        server_id: Option<&str>,
        tool: &str,
        account: Option<&str>,
    ) -> Result<SessionLoad, OrchestrationError> {
        let active = self.list(owner_checkout_id, true)?;
        let count = |keep: &dyn Fn(&OrchestratedSession) -> bool| {
            u32::try_from(active.iter().filter(|session| keep(session)).count()).unwrap_or(u32::MAX)
        };
        Ok(SessionLoad {
            target_sessions: count(&|session| session.target_id == target_id),
            // A session not yet placed on a machine is counted on its target's.
            machine_sessions: count(&|session| {
                session.target_id == target_id
                    || server_id.is_some_and(|server| {
                        session
                            .hosted
                            .as_ref()
                            .is_some_and(|hosted| hosted.server_id == server)
                    })
            }),
            tool_sessions: count(&|session| session.tool == tool),
            account: account.map(str::to_owned),
            account_sessions: match account {
                Some(account) => count(&|session| {
                    session.tool == tool && session.account.as_deref() == Some(account)
                }),
                None => 0,
            },
        })
    }

    /// What [`Self::begin`] would answer, without recording anything: the session an
    /// earlier request with the same operation id recorded, or `None` when the request
    /// would start a new one. Refusals are the same, so nothing is asked of a person or
    /// a target for a start that cannot be recorded.
    pub fn check(
        &self,
        owner_checkout_id: &str,
        new: &NewSession,
    ) -> Result<Option<OrchestratedSession>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        let session = candidate(owner_checkout_id, new.clone(), None, 0);
        validate_fields(&session)?;
        admit(&self.read_file()?, &session)
    }

    /// Record a session in `requested`, or return the session an earlier request with
    /// the same operation id recorded (`false`), whatever became of it. The same
    /// operation id for a different target, tool, model, effort, role, account or run
    /// conflicts. A target has at most one active session per non-worker role.
    pub fn begin(
        &self,
        owner_checkout_id: &str,
        new: NewSession,
        actor_id: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<(OrchestratedSession, bool), OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        let mut session = candidate(owner_checkout_id, new, actor_id, tx.clock.now_ms());
        validate_fields(&session)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        if let Some(existing) = admit(&file, &session)? {
            return Ok((existing, false));
        }
        session.session_id = tx.token();
        validate(&session)?;
        prune(&mut file);
        if file.sessions.len() >= MAX_SESSIONS {
            return Err(OrchestrationError::Capacity);
        }
        file.sessions.push(session.clone());
        self.write_file(&file)?;
        Ok((session, true))
    }

    /// Apply one change. A state change must follow [`SessionState::may_become`]; a
    /// finished session refuses every change. An update that changes nothing writes
    /// nothing.
    pub fn update(
        &self,
        owner_checkout_id: &str,
        session_id: &str,
        update: SessionUpdate,
        tx: &mut Transaction<'_>,
    ) -> Result<OrchestratedSession, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(session_id)?;
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        // HS2-DG0YPA: a session is linked only to a run no other live session holds.
        let held = update.run_id.as_ref().is_some_and(|run_id| {
            run_holder(
                file.sessions.iter().filter(|other| {
                    other.owner_checkout_id == owner_checkout_id && other.session_id != session_id
                }),
                run_id,
            )
            .is_some()
        });
        let session = file
            .sessions
            .iter_mut()
            .find(|session| {
                session.session_id == session_id && session.owner_checkout_id == owner_checkout_id
            })
            .ok_or(OrchestrationError::Session(SessionError::NotFound))?;
        let before = session.clone();
        if session.state.is_terminal() {
            return Err(OrchestrationError::Session(SessionError::NotActive));
        }
        if held && update.run_id != before.run_id {
            return Err(OrchestrationError::Session(SessionError::RunHeld));
        }
        let mut entry = None;
        if let Some(to) = update.to.filter(|to| *to != session.state) {
            if !session.state.may_become(to) {
                return Err(OrchestrationError::Session(SessionError::InvalidTransition));
            }
            entry = Some(SessionTransition {
                at_ms: now_ms,
                from: Some(session.state),
                to,
                reason: update.reason.clone(),
                actor_id: update.actor_id.clone(),
                control: update.control,
            });
            session.state = to;
        }
        if update.owner == Some(SessionOwner::Human) {
            session.owner = SessionOwner::Human;
        }
        if let Some(paused) = update.paused {
            // Only a person's session can be paused, and a pause is never undone by
            // the orchestrator: resuming is whatever its person does with it.
            session.paused = session.paused || (paused && session.owner == SessionOwner::Human);
        }
        if let Some(hosted) = update.hosted {
            session.hosted = Some(hosted);
        }
        if let Some(prompt) = update.role_prompt {
            session.role_prompt = Some(prompt);
        }
        if session.joined_at_ms.is_none() {
            session.joined_at_ms = update.joined_at_ms;
        }
        if let Some(run_id) = update.run_id {
            session.run_id = Some(run_id);
        }
        // A control act that changed ownership or the pause without changing the state
        // is still its own history entry.
        if entry.is_none()
            && update.control.is_some()
            && (session.owner != before.owner || session.paused != before.paused)
        {
            entry = Some(SessionTransition {
                at_ms: now_ms,
                from: Some(session.state),
                to: session.state,
                reason: update.reason,
                actor_id: update.actor_id,
                control: update.control,
            });
        }
        if let Some(entry) = entry {
            session.history.push(entry);
            // Keep the first entry (how it began) and the most recent ones.
            if session.history.len() > MAX_SESSION_HISTORY {
                session.history.remove(1);
            }
        }
        if *session == before {
            return Ok(before);
        }
        session.updated_at_ms = now_ms;
        validate(session)?;
        let updated = session.clone();
        self.write_file(&file)?;
        Ok(updated)
    }
}

/// HS2-DG0YPA: the session that holds a run's work: the latest of `sessions` linked
/// to the run that is still `requested` or `running`. A finished session holds
/// nothing, so a run whose sessions all ended may take a new one.
pub fn run_holder<'a>(
    sessions: impl IntoIterator<Item = &'a OrchestratedSession>,
    run_id: &str,
) -> Option<&'a OrchestratedSession> {
    sessions
        .into_iter()
        .filter(|session| session.run_id.as_deref() == Some(run_id) && session.state.is_active())
        .last()
}

fn candidate(
    owner_checkout_id: &str,
    new: NewSession,
    actor_id: Option<String>,
    now_ms: u64,
) -> OrchestratedSession {
    OrchestratedSession {
        session_id: String::new(),
        owner_checkout_id: owner_checkout_id.into(),
        operation_id: new.operation_id,
        target_id: new.target_id,
        role: new.role,
        tool: new.tool,
        model: new.model,
        effort: new.effort,
        account: new.account,
        run_id: new.run_id,
        replaces: new.replaces,
        state: SessionState::Requested,
        owner: SessionOwner::Orchestrator,
        paused: false,
        hosted: None,
        role_prompt: None,
        joined_at_ms: None,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        history: vec![SessionTransition {
            at_ms: now_ms,
            from: None,
            to: SessionState::Requested,
            reason: None,
            actor_id,
            control: None,
        }],
    }
}

/// Whether `session` may be recorded: `Some` is the session its operation id already
/// names, `None` a new one. Every refusal of a start that needs no host is here.
fn admit(
    file: &SessionFile,
    session: &OrchestratedSession,
) -> Result<Option<OrchestratedSession>, OrchestrationError> {
    let owned = |other: &&OrchestratedSession| other.owner_checkout_id == session.owner_checkout_id;
    if let Some(existing) = file
        .sessions
        .iter()
        .filter(owned)
        .find(|other| other.operation_id == session.operation_id)
    {
        let same = existing.target_id == session.target_id
            && existing.role == session.role
            && existing.tool == session.tool
            && existing.model == session.model
            && existing.effort == session.effort
            && existing.account == session.account
            && existing.run_id == session.run_id
            && existing.replaces == session.replaces;
        return if same {
            Ok(Some(existing.clone()))
        } else {
            Err(OrchestrationError::Conflict)
        };
    }
    if let Some(replaced) = &session.replaces {
        let known = file
            .sessions
            .iter()
            .filter(owned)
            .find(|other| &other.session_id == replaced)
            .ok_or(OrchestrationError::Session(SessionError::NotFound))?;
        // Only a session that is gone can be replaced.
        if known.state.is_active() {
            return Err(OrchestrationError::Session(SessionError::InvalidTransition));
        }
    }
    // HS2-DG0YPA: one live session per run. A crashed holder is learned from its host
    // before a start asks (`lost` is not active), so only a surviving one refuses.
    if let Some(run_id) = &session.run_id
        && run_holder(file.sessions.iter().filter(owned), run_id).is_some()
    {
        return Err(OrchestrationError::Session(SessionError::RunHeld));
    }
    if session.role.is_role_session()
        && file.sessions.iter().filter(owned).any(|other| {
            other.state.is_active()
                && other.role == session.role
                && other.target_id == session.target_id
        })
    {
        return Err(OrchestrationError::Session(SessionError::RoleActive));
    }
    Ok(None)
}

/// At capacity, forget the oldest finished session; an active one stays.
fn prune(file: &mut SessionFile) {
    while file.sessions.len() >= MAX_SESSIONS {
        let Some(index) = file
            .sessions
            .iter()
            .position(|session| session.state.is_terminal())
        else {
            return;
        };
        file.sessions.remove(index);
    }
}

fn validate(session: &OrchestratedSession) -> Result<(), OrchestrationError> {
    valid_token(&session.session_id)?;
    validate_fields(session)
}

/// Everything but the session id, so a request is checked before an id is minted.
fn validate_fields(session: &OrchestratedSession) -> Result<(), OrchestrationError> {
    valid_identifier(&session.owner_checkout_id)?;
    valid_identifier(&session.operation_id)?;
    valid_token(&session.target_id)?;
    valid_identifier(&session.tool)?;
    if let Some(model) = &session.model {
        valid_model(model)?;
    }
    for value in [&session.effort, &session.account].into_iter().flatten() {
        valid_identifier(value)?;
    }
    for value in [&session.run_id, &session.replaces].into_iter().flatten() {
        valid_token(value)?;
    }
    if let Some(hosted) = &session.hosted {
        valid_token(&hosted.server_id)?;
        valid_token(&hosted.session_id)?;
        valid_identifier(&hosted.worker_id)?;
    }
    if session.paused && session.owner != SessionOwner::Human {
        return Err(OrchestrationError::Invalid(
            "only a person's session is paused",
        ));
    }
    if session.history.is_empty() || session.history.len() > MAX_SESSION_HISTORY {
        return Err(OrchestrationError::Invalid(
            "bounded session history required",
        ));
    }
    for entry in &session.history {
        for value in [&entry.reason, &entry.actor_id].into_iter().flatten() {
            valid_identifier(value)?;
        }
    }
    Ok(())
}

// ---- Hosting side ----

/// The orchestration project a hosted session works for. Identifiers and a display
/// name only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionOrigin {
    /// The orchestration project's checkout id on its own machine.
    pub owner_checkout_id: String,
    /// The orchestration project's display name, for the drawer label.
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// HS2-YC2P7K: the exact target ticket the session was started for, by its
    /// canonical id. Its role prompt asks the target's dispatcher for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
}

/// One terminal of this machine that an orchestrator started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedSession {
    pub checkout_id: String,
    pub operation_id: String,
    /// This machine's terminal id, minted when the start was reserved.
    pub session_id: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub role: SessionRole,
    pub origin: SessionOrigin,
    #[serde(default)]
    pub owner: SessionOwner,
    #[serde(default)]
    pub paused: bool,
    /// The terminal was launched. A reservation that never launched may be retried.
    #[serde(default)]
    pub started: bool,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    /// A stable code for how it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
    /// HS2-2DPGDY: how the session was asked to take up its role.
    #[serde(default, skip_serializing_if = "RoleDelivery::is_unasked")]
    pub role_delivery: RoleDelivery,
    /// HS2-R7QH35: the latest request naming one exact ticket, pushed into the
    /// running session after it was given that ticket's run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<HostedRequest>,
}

/// HS2-R7QH35: why a running session is told about one exact ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    /// It was given the ticket's run: request that ticket from the dispatcher.
    Assigned,
    /// No dispatcher is live, so the ticket is claimed under its worker id: work it.
    Claimed,
    /// HS2-HV9ED3: a person cancelled the ticket's run: stop working it and let go.
    Cancelled,
}

impl RequestKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Assigned => "assigned",
            Self::Claimed => "claimed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// HS2-R7QH35: one request recorded on a hosted session, delivered like its role
/// prompt: once now, then on the tool's readiness events, within the same bounds,
/// counted from `requested_at_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedRequest {
    /// The ticket's canonical id.
    pub ticket: String,
    pub kind: RequestKind,
    pub requested_at_ms: u64,
    #[serde(default, skip_serializing_if = "RoleDelivery::is_unasked")]
    pub delivery: RoleDelivery,
}

impl HostedRequest {
    /// How the request stands at `now_ms`; a pending one past its bounds is failed.
    pub fn at(&self, now_ms: u64) -> Option<RolePrompt> {
        self.delivery.at(self.requested_at_ms, now_ms)
    }
}

impl HostedSession {
    /// The label this session carries in the hosting server's drawer.
    pub fn label(&self) -> String {
        session_label(
            &self.origin.project,
            self.origin.run_id.as_deref(),
            self.role,
        )
    }
}

/// What the orchestrator asks a host to launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewHostedSession {
    pub operation_id: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub role: SessionRole,
    pub origin: SessionOrigin,
}

impl NewHostedSession {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        valid_identifier(&self.operation_id)?;
        valid_identifier(&self.tool)?;
        if let Some(model) = &self.model {
            valid_model(model)?;
        }
        if let Some(effort) = &self.effort {
            valid_identifier(effort)?;
        }
        valid_identifier(&self.origin.owner_checkout_id)?;
        if !bounded_text(&self.origin.project, 4 * MAX_SESSION_PROJECT_CHARS)
            || self.origin.project.chars().count() > MAX_SESSION_PROJECT_CHARS
        {
            return Err(OrchestrationError::Invalid(
                "bounded orchestration project name required",
            ));
        }
        if let Some(run) = &self.origin.run_id {
            valid_token(run)?;
        }
        if let Some(ticket) = &self.origin.ticket {
            valid_identifier(ticket)?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostedFile {
    schema_version: u32,
    sessions: Vec<HostedSession>,
}

#[derive(Clone)]
pub struct HostedSessionRegistry {
    storage: Storage,
}

impl HostedSessionRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-hosted-sessions.json"),
        }
    }

    fn read_file(&self) -> Result<HostedFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_SESSION_BYTES)? else {
            return Ok(HostedFile {
                schema_version: ORCHESTRATION_VERSION,
                sessions: Vec::new(),
            });
        };
        let file: HostedFile = decode(&bytes)?;
        for session in &file.sessions {
            validate_hosted(session).map_err(|_| OrchestrationError::Corrupt)?;
        }
        Ok(file)
    }

    fn write_file(&self, file: &HostedFile) -> Result<(), OrchestrationError> {
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_SESSION_BYTES)
    }

    /// Sessions orchestrators started on one checkout, oldest first.
    pub fn list(&self, checkout_id: &str) -> Result<Vec<HostedSession>, OrchestrationError> {
        valid_identifier(checkout_id)?;
        Ok(self
            .read_file()?
            .sessions
            .into_iter()
            .filter(|session| session.checkout_id == checkout_id)
            .collect())
    }

    /// HS2-95SWQ4: whether an orchestrator started the terminal with this id, on any
    /// checkout of this machine. The terminal host asks when one of its AI terminals
    /// ends or finishes a turn.
    pub fn hosts(&self, session_id: &str) -> Result<bool, OrchestrationError> {
        Ok(self
            .read_file()?
            .sessions
            .iter()
            .any(|session| session.session_id == session_id))
    }

    pub fn get(
        &self,
        checkout_id: &str,
        operation_id: &str,
    ) -> Result<Option<HostedSession>, OrchestrationError> {
        valid_identifier(operation_id)?;
        Ok(self
            .list(checkout_id)?
            .into_iter()
            .find(|session| session.operation_id == operation_id))
    }

    /// Reserve a terminal id for a start, or return the reservation an earlier request
    /// with the same operation id made (`false`). The same operation id for a different
    /// tool, model, effort, role or origin conflicts.
    pub fn reserve(
        &self,
        checkout_id: &str,
        new: &NewHostedSession,
        tx: &mut Transaction<'_>,
    ) -> Result<(HostedSession, bool), OrchestrationError> {
        valid_identifier(checkout_id)?;
        new.validate()?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        if let Some(existing) = file.sessions.iter().find(|session| {
            session.checkout_id == checkout_id && session.operation_id == new.operation_id
        }) {
            let same = existing.tool == new.tool
                && existing.model == new.model
                && existing.effort == new.effort
                && existing.role == new.role
                && existing.origin == new.origin;
            return if same {
                Ok((existing.clone(), false))
            } else {
                Err(OrchestrationError::Conflict)
            };
        }
        while file.sessions.len() >= MAX_HOSTED_SESSIONS {
            let Some(index) = file
                .sessions
                .iter()
                .position(|session| session.ended_at_ms.is_some())
            else {
                return Err(OrchestrationError::Capacity);
            };
            file.sessions.remove(index);
        }
        let session = HostedSession {
            checkout_id: checkout_id.into(),
            operation_id: new.operation_id.clone(),
            session_id: tx.token(),
            tool: new.tool.clone(),
            model: new.model.clone(),
            effort: new.effort.clone(),
            role: new.role,
            origin: new.origin.clone(),
            owner: SessionOwner::Orchestrator,
            paused: false,
            started: false,
            created_at_ms: tx.clock.now_ms(),
            ended_at_ms: None,
            end_reason: None,
            role_delivery: RoleDelivery::default(),
            request: None,
        };
        validate_hosted(&session)?;
        file.sessions.push(session.clone());
        self.write_file(&file)?;
        Ok((session, true))
    }

    fn change(
        &self,
        checkout_id: &str,
        operation_id: &str,
        change: impl FnOnce(&mut HostedSession),
    ) -> Result<Option<HostedSession>, OrchestrationError> {
        valid_identifier(checkout_id)?;
        valid_identifier(operation_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let Some(session) = file.sessions.iter_mut().find(|session| {
            session.checkout_id == checkout_id && session.operation_id == operation_id
        }) else {
            return Ok(None);
        };
        let before = session.clone();
        change(session);
        let after = session.clone();
        if after != before {
            validate_hosted(&after)?;
            self.write_file(&file)?;
        }
        Ok(Some(after))
    }

    /// The terminal was launched. Repeating it changes nothing.
    pub fn mark_started(
        &self,
        checkout_id: &str,
        operation_id: &str,
    ) -> Result<Option<HostedSession>, OrchestrationError> {
        self.change(checkout_id, operation_id, |session| {
            if session.ended_at_ms.is_none() {
                session.started = true;
            }
        })
    }

    /// Forget a reservation whose terminal never launched, so the start can be asked
    /// again. A launched session is never forgotten this way.
    pub fn release(&self, checkout_id: &str, operation_id: &str) -> Result<(), OrchestrationError> {
        valid_identifier(checkout_id)?;
        valid_identifier(operation_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let before = file.sessions.len();
        file.sessions.retain(|session| {
            session.started
                || session.checkout_id != checkout_id
                || session.operation_id != operation_id
        });
        if file.sessions.len() != before {
            self.write_file(&file)?;
        }
        Ok(())
    }

    /// The session ended. The first reason stands; ending it again changes nothing.
    pub fn end(
        &self,
        checkout_id: &str,
        operation_id: &str,
        reason: &str,
        now_ms: u64,
    ) -> Result<Option<HostedSession>, OrchestrationError> {
        valid_identifier(reason)?;
        self.change(checkout_id, operation_id, |session| {
            if session.ended_at_ms.is_none() {
                session.ended_at_ms = Some(now_ms);
                session.end_reason = Some(reason.into());
            }
        })
    }

    /// HS2-2DPGDY: the started session with this terminal id, on any checkout of this
    /// machine, for the host's readiness events.
    pub fn by_session(
        &self,
        session_id: &str,
    ) -> Result<Option<HostedSession>, OrchestrationError> {
        Ok(self
            .read_file()?
            .sessions
            .into_iter()
            .find(|session| session.session_id == session_id && session.started))
    }

    /// HS2-2DPGDY: fold one try at delivering the role into the record. A settled
    /// delivery, or one of an ended session, does not change.
    pub fn role_attempt(
        &self,
        checkout_id: &str,
        operation_id: &str,
        attempt: Option<PushAttempt>,
        now_ms: u64,
    ) -> Result<Option<HostedSession>, OrchestrationError> {
        self.change(checkout_id, operation_id, |session| {
            if session.ended_at_ms.is_some() {
                return;
            }
            session.role_delivery = match attempt {
                Some(attempt) => {
                    session
                        .role_delivery
                        .after(attempt, session.created_at_ms, now_ms)
                }
                None if session.role_delivery.is_unasked() => RoleDelivery::at_launch(now_ms),
                None => session.role_delivery,
            };
        })
    }

    /// HS2-R7QH35: record a request naming `ticket`. The same ticket and kind again
    /// keep the request as it is, so a repeat is never pushed twice; another ticket or
    /// kind replaces it. `Ok(None)` when no session has the operation id; an ended
    /// session keeps what it had.
    pub fn request(
        &self,
        checkout_id: &str,
        operation_id: &str,
        ticket: &str,
        kind: RequestKind,
        now_ms: u64,
    ) -> Result<Option<HostedSession>, OrchestrationError> {
        valid_identifier(ticket)?;
        self.change(checkout_id, operation_id, |session| {
            if session.ended_at_ms.is_some() {
                return;
            }
            let same = session
                .request
                .as_ref()
                .is_some_and(|request| request.ticket == ticket && request.kind == kind);
            if !same {
                session.request = Some(HostedRequest {
                    ticket: ticket.into(),
                    kind,
                    requested_at_ms: now_ms,
                    delivery: RoleDelivery::default(),
                });
            }
        })
    }

    /// HS2-R7QH35: fold one try at delivering the request into the record. A settled
    /// delivery, an ended session or one without a request does not change.
    pub fn request_attempt(
        &self,
        checkout_id: &str,
        operation_id: &str,
        attempt: PushAttempt,
        now_ms: u64,
    ) -> Result<Option<HostedSession>, OrchestrationError> {
        self.change(checkout_id, operation_id, |session| {
            if session.ended_at_ms.is_some() {
                return;
            }
            if let Some(request) = session.request.as_mut() {
                request.delivery = request
                    .delivery
                    .after(attempt, request.requested_at_ms, now_ms);
            }
        })
    }

    /// A person took the session over, optionally pausing it. Never undone.
    pub fn take_over(
        &self,
        checkout_id: &str,
        operation_id: &str,
        paused: bool,
    ) -> Result<Option<HostedSession>, OrchestrationError> {
        self.change(checkout_id, operation_id, |session| {
            if session.ended_at_ms.is_none() {
                session.owner = SessionOwner::Human;
                session.paused = session.paused || paused;
            }
        })
    }
}

fn validate_hosted(session: &HostedSession) -> Result<(), OrchestrationError> {
    valid_identifier(&session.checkout_id)?;
    valid_token(&session.session_id)?;
    NewHostedSession {
        operation_id: session.operation_id.clone(),
        tool: session.tool.clone(),
        model: session.model.clone(),
        effort: session.effort.clone(),
        role: session.role,
        origin: session.origin.clone(),
    }
    .validate()?;
    if let Some(reason) = &session.end_reason {
        valid_identifier(reason)?;
    }
    if let Some(request) = &session.request {
        valid_identifier(&request.ticket)?;
    }
    if session.paused && session.owner != SessionOwner::Human {
        return Err(OrchestrationError::Invalid(
            "only a person's session is paused",
        ));
    }
    Ok(())
}
