//! HS2-5JBYPV: CI and deployment gates linked to dispatch runs.
//!
//! A gate ties one run to one target commit and says whether a provider confirmed it.
//! Its state moves only on a typed [`GateReading`] from a provider adapter, or on a
//! person's approval of a deployment. Nothing here parses text: a verdict without
//! evidence that covers the gate's commit is recorded as `unknown`, never as passed.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{valid_digest, valid_identifier, valid_token};
use super::storage::{Storage, decode};
use super::*;

pub const MAX_GATES: usize = 2048;
pub const MAX_GATE_HISTORY: usize = 32;
const MAX_GATE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    Ci,
    Deployment,
    /// HS2-GD3NNK: a code review of the run's commit, marked by a person or an AI:
    /// requested (`awaiting_approval`), approved (`passed`) or changes requested
    /// (`failed`). No provider reports it.
    Review,
}

/// HS2-GD3NNK: the provider name every review gate records; nothing watches it.
pub const REVIEW_PROVIDER: &str = "review";

/// HS2-GD3NNK: the reason codes of a review gate's states.
pub const REVIEW_REQUESTED: &str = "review_requested";
pub const REVIEW_APPROVED: &str = "review_approved";
pub const REVIEW_CHANGES_REQUESTED: &str = "changes_requested";

/// HS2-GD3NNK: what a person or an AI marks a review as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewMark {
    Requested,
    Approved,
    ChangesRequested,
}

impl ReviewMark {
    /// The gate state and reason this mark records.
    pub fn state(self) -> (GateState, &'static str) {
        match self {
            Self::Requested => (GateState::AwaitingApproval, REVIEW_REQUESTED),
            Self::Approved => (GateState::Passed, REVIEW_APPROVED),
            Self::ChangesRequested => (GateState::Failed, REVIEW_CHANGES_REQUESTED),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateState {
    /// Nothing trustworthy is known: not watched, the watch was lost or stopped, or a
    /// verdict arrived without evidence that covers the commit.
    Unknown,
    /// A deployment waits for a person's answer, or a review was requested.
    AwaitingApproval,
    /// Watched or started; no verdict yet.
    Pending,
    Passed,
    Failed,
    /// The link no longer stands for the run: a newer commit was linked in its place,
    /// or the provider reports that the shipping branch no longer contains the commit.
    Superseded,
}

impl GateState {
    pub const ALL: [Self; 6] = [
        Self::Unknown,
        Self::AwaitingApproval,
        Self::Pending,
        Self::Passed,
        Self::Failed,
        Self::Superseded,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Pending => "pending",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
        }
    }

    /// A provider may still change this state, so its adapter is worth reading.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Unknown | Self::Pending)
    }

    /// The only edges a gate may take. A verdict is never replaced by `unknown`, a
    /// passed gate only by supersession, and a superseded gate by nothing.
    pub fn may_become(self, next: Self) -> bool {
        use GateState::{AwaitingApproval, Failed, Passed, Pending, Superseded, Unknown};
        matches!(
            (self, next),
            (
                Unknown,
                AwaitingApproval | Pending | Passed | Failed | Superseded
            ) | (AwaitingApproval, Pending | Unknown | Failed | Superseded)
                // HS2-GD3NNK: a requested review is approved.
                | (AwaitingApproval, Passed)
                | (Pending, Passed | Failed | Unknown | Superseded)
                // A failed gate can be watched again, and a later attempt can pass.
                | (Failed, Pending | Passed | Superseded)
                | (Passed, Superseded)
        )
    }
}

/// How the evidence's commit relates to the gate's commit, as the provider attests it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    /// The provider built or deployed exactly the gate's commit.
    Exact,
    /// The provider built a descendant on the shipping branch that contains it.
    Ancestor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub name: String,
    /// Lowercase SHA-256 of the artifact.
    pub digest: String,
}

impl ArtifactRef {
    /// HS2-3YC577: whether a gate record would accept this artifact, so an adapter can
    /// leave out a provider name the store refuses instead of failing the whole reading.
    pub fn is_valid(&self) -> bool {
        validate_artifact(self).is_ok()
    }
}

/// What a provider adapter observed. Identifiers, counts and codes only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct GateEvidence {
    pub provider: String,
    /// The provider's own id of the CI run or deployment execution.
    pub reference: String,
    /// The adapter's rank of `reference`: a later run or execution ranks higher.
    pub order: u64,
    pub attempt: u32,
    /// The commit the provider built or deployed.
    pub commit: String,
    pub coverage: Coverage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// HS2-G3VAK4: how long a deployment execution ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub at_ms: u64,
}

impl GateEvidence {
    fn rank(&self) -> (u64, u32) {
        (self.order, self.attempt)
    }
}

/// One answer from a provider adapter about one gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateReading {
    pub state: GateState,
    /// A stable code, never free text.
    pub reason: Option<String>,
    pub evidence: Option<GateEvidence>,
    /// The adapter's own handle of the watch or execution, for example a Job id.
    pub watch_ref: Option<String>,
}

impl GateReading {
    pub fn new(state: GateState) -> Self {
        Self {
            state,
            reason: None,
            evidence: None,
            watch_ref: None,
        }
    }

    #[must_use]
    pub fn reason(mut self, reason: &str) -> Self {
        self.reason = Some(reason.into());
        self
    }

    #[must_use]
    pub fn evidence(mut self, evidence: GateEvidence) -> Self {
        self.evidence = Some(evidence);
        self
    }
}

/// What applying a reading did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    Changed,
    /// The reading repeats what is recorded.
    Unchanged,
    /// The reading was not applied; the code says why.
    Ignored(&'static str),
}

/// Where a CI provider looks for the commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiScope {
    /// The provider's repository name, for example `owner/name`.
    pub repository: String,
    pub workflow: String,
    /// The shipping branch whose runs count.
    pub branch: String,
    /// HS2-TQG6AT: the artifact whose digest a passed verdict records, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
}

/// What a deployment would run, and where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentScope {
    pub environment: String,
    /// The target's checked-in deploy script, relative to its repository root.
    pub script: String,
    /// An irreversible production change: a person is always asked first.
    pub production: bool,
    /// The caller's idempotency key; the approval is for this id.
    pub operation_id: String,
    /// The passed CI gate of the same commit this deployment rests on.
    pub requires_gate_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateTransition {
    pub at_ms: u64,
    /// Absent for the first entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<GateState>,
    pub to: GateState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub gate_id: String,
    pub owner_checkout_id: String,
    pub run_id: String,
    pub target_id: String,
    pub kind: GateKind,
    pub provider: String,
    /// The integrated target commit this gate is about: 40 lowercase hex characters.
    pub commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci: Option<CiScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment: Option<DeploymentScope>,
    pub state: GateState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<GateEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_by: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub history: Vec<GateTransition>,
}

impl Gate {
    /// Whether the evidence is about this gate's commit. A deployment counts only when
    /// it deployed exactly that commit.
    /// HS2-VVHVF0: the gate still waits on its provider: watched with no verdict, or
    /// its watch was lost and may be reattached. Such a gate needs an observer.
    pub fn awaits_provider(&self) -> bool {
        self.state == GateState::Pending
            || (self.state == GateState::Unknown && self.reason.as_deref() == Some("watch_lost"))
    }

    pub fn covered_by(&self, evidence: &GateEvidence) -> bool {
        evidence.provider == self.provider
            && match evidence.coverage {
                Coverage::Exact => evidence.commit == self.commit,
                Coverage::Ancestor => self.kind == GateKind::Ci && evidence.commit != self.commit,
            }
    }

    /// Gates that stand for the same thing on one run: linking a newer commit in the
    /// same slot supersedes the older gate.
    pub fn same_slot(&self, other: &Gate) -> bool {
        self.run_id == other.run_id
            && self.kind == other.kind
            && self.provider == other.provider
            && self.ci == other.ci
            && self.deployment.as_ref().map(|scope| &scope.environment)
                == other.deployment.as_ref().map(|scope| &scope.environment)
    }

    fn enter(&mut self, to: GateState, reason: Option<String>, now_ms: u64) {
        self.history.push(GateTransition {
            at_ms: now_ms,
            from: Some(self.state),
            to,
            reason: reason.clone(),
        });
        // Keep how the gate began and the most recent entries.
        if self.history.len() > MAX_GATE_HISTORY {
            self.history.remove(1);
        }
        self.state = to;
        self.reason = reason;
        self.updated_at_ms = now_ms;
    }

    /// Apply one provider reading. Readings may arrive late, twice or out of order:
    /// one that is older than the recorded evidence, or that would replace a verdict
    /// with less, is ignored and changes nothing.
    pub fn apply(&mut self, reading: GateReading, now_ms: u64) -> GateOutcome {
        if self.state == GateState::Superseded {
            return GateOutcome::Ignored("gate_superseded");
        }
        if matches!(self.state, GateState::AwaitingApproval)
            || reading.state == GateState::AwaitingApproval
        {
            // Only a person's answer moves a gate out of, and nothing a provider says
            // moves one into, waiting for approval.
            return GateOutcome::Ignored("awaiting_approval");
        }
        let GateReading {
            mut state,
            mut reason,
            mut evidence,
            watch_ref,
        } = reading;
        if matches!(state, GateState::Passed | GateState::Failed) {
            let problem = match &evidence {
                None => Some("evidence_missing"),
                Some(evidence) if !self.covered_by(evidence) => Some("evidence_not_covering"),
                Some(_) => None,
            };
            if let Some(problem) = problem {
                state = GateState::Unknown;
                reason = Some(problem.into());
                evidence = None;
            }
        } else {
            // Only a verdict carries evidence.
            evidence = None;
        }
        if let (Some(current), Some(new)) = (&self.evidence, &evidence) {
            if new.rank() < current.rank() {
                return GateOutcome::Ignored("stale_reading");
            }
        }
        if state == self.state {
            let same = reason == self.reason && (evidence.is_none() || evidence == self.evidence);
            if same && (watch_ref.is_none() || watch_ref == self.watch_ref) {
                return GateOutcome::Unchanged;
            }
            if evidence.is_some() && evidence != self.evidence {
                // A newer run or attempt with the same verdict is still news.
                self.enter(state, reason, now_ms);
                self.evidence = evidence;
            } else {
                self.reason = reason;
                self.updated_at_ms = now_ms;
            }
            if watch_ref.is_some() {
                self.watch_ref = watch_ref;
            }
            return GateOutcome::Changed;
        }
        if !self.state.may_become(state) {
            return GateOutcome::Ignored(if self.state == GateState::Passed {
                "gate_final"
            } else {
                "stale_reading"
            });
        }
        self.enter(state, reason, now_ms);
        if evidence.is_some() {
            self.evidence = evidence;
        }
        if watch_ref.is_some() {
            self.watch_ref = watch_ref;
        }
        GateOutcome::Changed
    }
}

/// What a caller supplies to link a gate. The registry assigns the id and first state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGate {
    pub run_id: String,
    pub target_id: String,
    pub kind: GateKind,
    pub provider: String,
    pub commit: String,
    pub ci: Option<CiScope>,
    pub deployment: Option<DeploymentScope>,
    /// `unknown` for a CI gate not yet watched; `awaiting_approval` or `unknown` for a
    /// deployment.
    pub state: GateState,
    pub reason: Option<String>,
    pub linked_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateError {
    NotFound,
    /// No registered adapter serves this provider and kind.
    ProviderUnsupported,
    /// The provider adapter could not start or read its watch.
    ProviderUnavailable,
    /// A deployment needs a passed CI gate that covers the same commit.
    NotPassed,
    /// The gate does not allow the request in its current state.
    InvalidTransition,
    /// The repository is not the one the target declares.
    RepositoryMismatch,
    /// The slot already has a deployment in progress.
    DeploymentActive,
    /// HS2-3YC577: the deployment names an artifact other than the one the passed CI
    /// run built.
    ArtifactMismatch,
    /// HS2-G3VAK4: the deploy script is not tracked as an executable file at the gate's
    /// commit on the machine that hosts the target.
    DeploymentScriptUntracked,
}

impl GateError {
    pub fn code(self) -> &'static str {
        match self {
            Self::NotFound => "gate_not_found",
            Self::ProviderUnsupported => "gate_provider_unsupported",
            Self::ProviderUnavailable => "gate_provider_unavailable",
            Self::NotPassed => "gate_not_passed",
            Self::InvalidTransition => "gate_invalid_transition",
            Self::RepositoryMismatch => "gate_repository_mismatch",
            Self::DeploymentActive => "gate_deployment_active",
            Self::ArtifactMismatch => "gate_artifact_mismatch",
            Self::DeploymentScriptUntracked => "gate_deployment_script_untracked",
        }
    }

    /// HS2-G3VAK4: the error a peer named by its code, so a refusal from the machine
    /// that hosts a target keeps its type across the transport.
    pub fn from_code(code: &str) -> Option<Self> {
        [
            Self::NotFound,
            Self::ProviderUnsupported,
            Self::ProviderUnavailable,
            Self::NotPassed,
            Self::InvalidTransition,
            Self::RepositoryMismatch,
            Self::DeploymentActive,
            Self::ArtifactMismatch,
            Self::DeploymentScriptUntracked,
        ]
        .into_iter()
        .find(|error| error.code() == code)
    }
}

/// Which gates a read wants. Every field narrows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateFilter {
    pub run_id: Option<String>,
    pub target_id: Option<String>,
    pub commit: Option<String>,
    /// Leave out superseded gates.
    pub current_only: bool,
}

impl GateFilter {
    fn matches(&self, gate: &Gate) -> bool {
        self.run_id.as_ref().is_none_or(|id| &gate.run_id == id)
            && self
                .target_id
                .as_ref()
                .is_none_or(|id| &gate.target_id == id)
            && self.commit.as_ref().is_none_or(|sha| &gate.commit == sha)
            && !(self.current_only && gate.state == GateState::Superseded)
    }
}

/// The gates that currently stand, rolled up. `failed` outranks `unknown`, which
/// outranks `awaiting_approval`, then `pending`; `passed` needs every gate passed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSummary {
    /// The CI verdict. `unknown` with reason `no_gate` when no CI gate stands.
    pub ci: GateState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_reason: Option<String>,
    /// Absent when no deployment was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment: Option<GateState>,
    /// HS2-GD3NNK: the review verdict; absent when no review was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<GateState>,
    /// The commits the standing CI gates are about, oldest link first.
    pub commits: Vec<String>,
    pub gates: Vec<Gate>,
    /// How many superseded links were left out.
    pub superseded: usize,
}

fn rollup<'a>(gates: impl Iterator<Item = &'a Gate>) -> Option<(GateState, Option<String>)> {
    let rank = |state: GateState| match state {
        GateState::Failed => 5,
        GateState::Unknown => 4,
        GateState::AwaitingApproval => 3,
        GateState::Pending => 2,
        GateState::Passed => 1,
        GateState::Superseded => 0,
    };
    gates
        .max_by_key(|gate| rank(gate.state))
        .map(|gate| (gate.state, gate.reason.clone()))
}

/// Roll up the gates of one run, or of one target commit. Superseded links count for
/// nothing: a run with none left standing reads as `unknown`, never as passed.
pub fn summarize(gates: Vec<Gate>) -> GateSummary {
    let total = gates.len();
    let gates: Vec<Gate> = gates
        .into_iter()
        .filter(|gate| gate.state != GateState::Superseded)
        .collect();
    let of = |kind| gates.iter().filter(move |gate| gate.kind == kind);
    let (ci, ci_reason) =
        rollup(of(GateKind::Ci)).unwrap_or((GateState::Unknown, Some("no_gate".into())));
    let mut commits: Vec<String> = Vec::new();
    for gate in of(GateKind::Ci) {
        if !commits.contains(&gate.commit) {
            commits.push(gate.commit.clone());
        }
    }
    GateSummary {
        ci,
        ci_reason,
        deployment: rollup(of(GateKind::Deployment)).map(|(state, _)| state),
        review: rollup(of(GateKind::Review)).map(|(state, _)| state),
        commits,
        superseded: total - gates.len(),
        gates,
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateFile {
    schema_version: u32,
    gates: Vec<Gate>,
}

/// What linking did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub gate: Gate,
    /// False when an equal link already stood and was returned.
    pub created: bool,
    /// Gates this link superseded, after the change.
    pub superseded: Vec<Gate>,
}

#[derive(Clone)]
pub struct GateRegistry {
    storage: Storage,
}

impl GateRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-gates.json"),
        }
    }

    fn read_file(&self) -> Result<GateFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_GATE_BYTES)? else {
            return Ok(GateFile {
                schema_version: ORCHESTRATION_VERSION,
                gates: Vec::new(),
            });
        };
        let file: GateFile = decode(&bytes)?;
        for gate in &file.gates {
            validate(gate).map_err(|_| OrchestrationError::Corrupt)?;
        }
        Ok(file)
    }

    fn write_file(&self, file: &GateFile) -> Result<(), OrchestrationError> {
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_GATE_BYTES)
    }

    pub fn get(&self, owner_checkout_id: &str, gate_id: &str) -> Result<Gate, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(gate_id)?;
        self.read_file()?
            .gates
            .into_iter()
            .find(|gate| gate.gate_id == gate_id && gate.owner_checkout_id == owner_checkout_id)
            .ok_or(OrchestrationError::Gate(GateError::NotFound))
    }

    /// HS2-VVHVF0: owning checkouts that have a gate still waiting on a provider.
    pub fn waiting_owners(&self) -> Result<Vec<String>, OrchestrationError> {
        let owners: std::collections::BTreeSet<String> = self
            .read_file()?
            .gates
            .into_iter()
            .filter(Gate::awaits_provider)
            .map(|gate| gate.owner_checkout_id)
            .collect();
        Ok(owners.into_iter().collect())
    }

    /// Gates of one owning checkout, oldest link first.
    pub fn list(
        &self,
        owner_checkout_id: &str,
        filter: &GateFilter,
    ) -> Result<Vec<Gate>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        if let Some(run_id) = &filter.run_id {
            valid_token(run_id)?;
        }
        if let Some(target_id) = &filter.target_id {
            valid_token(target_id)?;
        }
        if let Some(commit) = &filter.commit {
            valid_commit(commit)?;
        }
        Ok(self
            .read_file()?
            .gates
            .into_iter()
            .filter(|gate| gate.owner_checkout_id == owner_checkout_id && filter.matches(gate))
            .collect())
    }

    /// Link a gate to a run. A standing gate in the same slot for the same commit is
    /// returned unchanged (for a deployment: the same operation id; the same id with
    /// different content conflicts). A standing gate for another commit is superseded,
    /// together with any deployment still waiting on it. A slot whose deployment is in
    /// progress refuses a second one.
    pub fn link(
        &self,
        owner_checkout_id: &str,
        new: NewGate,
        tx: &mut Transaction<'_>,
    ) -> Result<Linked, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        let now_ms = tx.clock.now_ms();
        let mut gate = Gate {
            gate_id: String::new(),
            owner_checkout_id: owner_checkout_id.into(),
            run_id: new.run_id,
            target_id: new.target_id,
            kind: new.kind,
            provider: new.provider,
            commit: new.commit,
            ci: new.ci,
            deployment: new.deployment,
            state: new.state,
            reason: new.reason.clone(),
            evidence: None,
            watch_ref: None,
            superseded_by: None,
            linked_by: new.linked_by,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            history: vec![GateTransition {
                at_ms: now_ms,
                from: None,
                to: new.state,
                reason: new.reason,
            }],
        };
        validate_fields(&gate)?;
        let first = match gate.kind {
            GateKind::Ci => gate.state == GateState::Unknown,
            GateKind::Deployment => {
                matches!(gate.state, GateState::Unknown | GateState::AwaitingApproval)
            }
            GateKind::Review => gate.state == GateState::AwaitingApproval,
        };
        if !first {
            return Err(OrchestrationError::Invalid(
                "a new gate starts unknown or awaiting approval",
            ));
        }
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let owned = |other: &&Gate| other.owner_checkout_id == owner_checkout_id;
        if let Some(operation_id) = gate.deployment.as_ref().map(|scope| &scope.operation_id) {
            if let Some(existing) = file.gates.iter().filter(owned).find(|other| {
                other
                    .deployment
                    .as_ref()
                    .is_some_and(|scope| &scope.operation_id == operation_id)
            }) {
                let same = existing.run_id == gate.run_id
                    && existing.commit == gate.commit
                    && existing.provider == gate.provider
                    && existing.deployment == gate.deployment;
                return if same {
                    Ok(Linked {
                        gate: existing.clone(),
                        created: false,
                        superseded: Vec::new(),
                    })
                } else {
                    Err(OrchestrationError::Conflict)
                };
            }
        }
        let standing = |other: &&Gate| {
            other.owner_checkout_id == owner_checkout_id
                && other.state != GateState::Superseded
                && other.same_slot(&gate)
        };
        match gate.kind {
            GateKind::Ci => {
                if let Some(existing) = file
                    .gates
                    .iter()
                    .filter(standing)
                    .find(|other| other.commit == gate.commit)
                {
                    return Ok(Linked {
                        gate: existing.clone(),
                        created: false,
                        superseded: Vec::new(),
                    });
                }
            }
            GateKind::Deployment => {
                if file
                    .gates
                    .iter()
                    .filter(standing)
                    .any(|other| other.state == GateState::Pending)
                {
                    return Err(OrchestrationError::Gate(GateError::DeploymentActive));
                }
            }
            // HS2-GD3NNK: a review already requested for this commit is the same request.
            GateKind::Review => {
                if let Some(existing) = file.gates.iter().filter(standing).find(|other| {
                    other.commit == gate.commit && other.state == GateState::AwaitingApproval
                }) {
                    return Ok(Linked {
                        gate: existing.clone(),
                        created: false,
                        superseded: Vec::new(),
                    });
                }
            }
        }
        gate.gate_id = tx.token();
        validate(&gate)?;
        let mut superseded = Vec::new();
        let mut replaced_ci = Vec::new();
        for other in file.gates.iter_mut() {
            if other.owner_checkout_id == owner_checkout_id
                && other.state != GateState::Superseded
                && other.same_slot(&gate)
            {
                other.enter(
                    GateState::Superseded,
                    Some("newer_gate_linked".into()),
                    now_ms,
                );
                other.superseded_by = Some(gate.gate_id.clone());
                if other.kind == GateKind::Ci {
                    replaced_ci.push(other.gate_id.clone());
                }
                superseded.push(other.clone());
            }
        }
        // A deployment that has not started must not proceed on a replaced CI gate.
        for other in file.gates.iter_mut() {
            let waiting = matches!(
                other.state,
                GateState::AwaitingApproval | GateState::Unknown
            );
            if other.owner_checkout_id == owner_checkout_id
                && waiting
                && other
                    .deployment
                    .as_ref()
                    .is_some_and(|scope| replaced_ci.contains(&scope.requires_gate_id))
            {
                other.enter(
                    GateState::Superseded,
                    Some("required_gate_superseded".into()),
                    now_ms,
                );
                superseded.push(other.clone());
            }
        }
        prune(&mut file);
        if file.gates.len() >= MAX_GATES {
            return Err(OrchestrationError::Capacity);
        }
        file.gates.push(gate.clone());
        self.write_file(&file)?;
        Ok(Linked {
            gate,
            created: true,
            superseded,
        })
    }

    fn change(
        &self,
        owner_checkout_id: &str,
        gate_id: &str,
        change: impl FnOnce(&mut Gate, u64) -> Result<GateOutcome, OrchestrationError>,
        tx: &mut Transaction<'_>,
    ) -> Result<(Gate, GateOutcome), OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(gate_id)?;
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let gate = file
            .gates
            .iter_mut()
            .find(|gate| gate.gate_id == gate_id && gate.owner_checkout_id == owner_checkout_id)
            .ok_or(OrchestrationError::Gate(GateError::NotFound))?;
        let outcome = change(gate, now_ms)?;
        let after = gate.clone();
        if outcome == GateOutcome::Changed {
            validate(&after)?;
            self.write_file(&file)?;
        }
        Ok((after, outcome))
    }

    /// Apply a provider reading. Writes only when something changed.
    pub fn apply(
        &self,
        owner_checkout_id: &str,
        gate_id: &str,
        reading: GateReading,
        tx: &mut Transaction<'_>,
    ) -> Result<(Gate, GateOutcome), OrchestrationError> {
        if let Some(evidence) = &reading.evidence {
            validate_evidence(evidence)?;
        }
        for value in [&reading.reason, &reading.watch_ref].into_iter().flatten() {
            valid_identifier(value)?;
        }
        self.change(
            owner_checkout_id,
            gate_id,
            |gate, now_ms| Ok(gate.apply(reading, now_ms)),
            tx,
        )
    }

    /// Move a deployment that waits for a person on to what their answer allows:
    /// `pending` (an executor took it), `unknown` (approved, nothing to run it) or
    /// `failed` (denied). Repeating the same answer changes nothing.
    pub fn resolve_approval(
        &self,
        owner_checkout_id: &str,
        gate_id: &str,
        to: GateState,
        reason: Option<&str>,
        tx: &mut Transaction<'_>,
    ) -> Result<(Gate, GateOutcome), OrchestrationError> {
        if let Some(reason) = reason {
            valid_identifier(reason)?;
        }
        self.change(
            owner_checkout_id,
            gate_id,
            |gate, now_ms| {
                let reason = reason.map(str::to_owned);
                if gate.state == to && gate.reason == reason {
                    return Ok(GateOutcome::Unchanged);
                }
                if gate.state != GateState::AwaitingApproval
                    || !matches!(
                        to,
                        GateState::Pending | GateState::Unknown | GateState::Failed
                    )
                {
                    return Err(OrchestrationError::Gate(GateError::InvalidTransition));
                }
                gate.enter(to, reason, now_ms);
                Ok(GateOutcome::Changed)
            },
            tx,
        )
    }
}

impl GateRegistry {
    /// HS2-GD3NNK: a requested review is approved or has changes requested. The same
    /// mark again changes nothing; any other edge is refused. A new request for a
    /// review that was answered is a new review gate, linked in its place.
    pub fn mark_review(
        &self,
        owner_checkout_id: &str,
        gate_id: &str,
        mark: ReviewMark,
        tx: &mut Transaction<'_>,
    ) -> Result<(Gate, GateOutcome), OrchestrationError> {
        let (to, reason) = mark.state();
        self.change(
            owner_checkout_id,
            gate_id,
            |gate, now_ms| {
                if gate.kind != GateKind::Review {
                    return Err(OrchestrationError::Gate(GateError::InvalidTransition));
                }
                if gate.state == to && gate.reason.as_deref() == Some(reason) {
                    return Ok(GateOutcome::Unchanged);
                }
                if gate.state != GateState::AwaitingApproval || mark == ReviewMark::Requested {
                    return Err(OrchestrationError::Gate(GateError::InvalidTransition));
                }
                gate.enter(to, Some(reason.into()), now_ms);
                Ok(GateOutcome::Changed)
            },
            tx,
        )
    }
}

/// At capacity, forget the oldest superseded gate, then the oldest settled one. A gate
/// that still waits for a verdict or a person stays.
fn prune(file: &mut GateFile) {
    while file.gates.len() >= MAX_GATES {
        let oldest =
            |wanted: fn(GateState) -> bool| file.gates.iter().position(|gate| wanted(gate.state));
        let Some(index) = oldest(|state| state == GateState::Superseded)
            .or_else(|| oldest(|state| matches!(state, GateState::Passed | GateState::Failed)))
        else {
            return;
        };
        file.gates.remove(index);
    }
}

/// A full commit id: 40 lowercase hex characters. A short or symbolic name is refused.
pub fn valid_commit(value: &str) -> Result<(), OrchestrationError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(OrchestrationError::Invalid(
            "full lowercase 40-character commit id required",
        ));
    }
    Ok(())
}

fn plain(value: &str, limit: usize) -> Result<(), OrchestrationError> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(OrchestrationError::Invalid("bounded gate name required"));
    }
    Ok(())
}

/// A script inside the target's repository: relative, forward slashes, no parent or
/// hidden-root segments, no whitespace.
pub fn valid_script(value: &str) -> Result<(), OrchestrationError> {
    let segment = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if value.len() > 200 || !value.split('/').all(segment) {
        return Err(OrchestrationError::Invalid(
            "repository-relative deploy script path required",
        ));
    }
    Ok(())
}

fn validate_artifact(artifact: &ArtifactRef) -> Result<(), OrchestrationError> {
    valid_identifier(&artifact.name)?;
    valid_digest(&artifact.digest)
}

fn validate_evidence(evidence: &GateEvidence) -> Result<(), OrchestrationError> {
    valid_identifier(&evidence.provider)?;
    valid_identifier(&evidence.reference)?;
    valid_commit(&evidence.commit)?;
    evidence.artifact.as_ref().map_or(Ok(()), validate_artifact)
}

fn validate(gate: &Gate) -> Result<(), OrchestrationError> {
    valid_token(&gate.gate_id)?;
    validate_fields(gate)
}

/// Everything but the gate id, so a request is checked before an id is minted.
fn validate_fields(gate: &Gate) -> Result<(), OrchestrationError> {
    valid_identifier(&gate.owner_checkout_id)?;
    valid_token(&gate.run_id)?;
    valid_token(&gate.target_id)?;
    valid_identifier(&gate.provider)?;
    valid_commit(&gate.commit)?;
    match (gate.kind, &gate.ci, &gate.deployment) {
        (GateKind::Ci, Some(scope), None) => {
            plain(&scope.repository, 200)?;
            plain(&scope.workflow, 100)?;
            plain(&scope.branch, 100)?;
            if let Some(artifact) = &scope.artifact {
                valid_identifier(artifact)?;
            }
        }
        (GateKind::Review, None, None) => {}
        (GateKind::Deployment, None, Some(scope)) => {
            valid_identifier(&scope.environment)?;
            valid_script(&scope.script)?;
            valid_identifier(&scope.operation_id)?;
            valid_token(&scope.requires_gate_id)?;
            scope.artifact.as_ref().map_or(Ok(()), validate_artifact)?;
        }
        _ => {
            return Err(OrchestrationError::Invalid(
                "a gate carries exactly the scope of its kind",
            ));
        }
    }
    if gate.kind == GateKind::Ci && gate.state == GateState::AwaitingApproval {
        return Err(OrchestrationError::Invalid(
            "only a deployment waits for approval",
        ));
    }
    for value in [
        &gate.reason,
        &gate.watch_ref,
        &gate.superseded_by,
        &gate.linked_by,
    ]
    .into_iter()
    .flatten()
    {
        valid_identifier(value)?;
    }
    if let Some(evidence) = &gate.evidence {
        validate_evidence(evidence)?;
    }
    if gate.history.is_empty() || gate.history.len() > MAX_GATE_HISTORY {
        return Err(OrchestrationError::Invalid("bounded gate history required"));
    }
    for entry in &gate.history {
        if let Some(reason) = &entry.reason {
            valid_identifier(reason)?;
        }
    }
    Ok(())
}
