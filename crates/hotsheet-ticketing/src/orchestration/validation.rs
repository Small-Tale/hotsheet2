//! HS2-WN0MXZ: named validation steps of an orchestration ticket and their results.
//!
//! A step says what to run in which target and what counts as passing. A result is
//! one attempt at it. Results hold identifiers, codes, digests, sizes, an exit code
//! and timings only: the output of a step never enters this file, a journal or a
//! ticket. A finished result is immutable; only a `running` one may change.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{bounded_text, valid_digest, valid_identifier, valid_token};
use super::storage::{Storage, decode};
use super::*;

pub const MAX_VALIDATION_STEPS: usize = 512;
pub const MAX_STEPS_PER_TICKET: usize = 32;
pub const MAX_STEP_RESULTS: usize = 16;
pub const MAX_STEP_PREREQUISITES: usize = 8;
pub const MAX_STEP_ATTEMPTS: u32 = 5;
pub const DEFAULT_STEP_TIMEOUT_SECONDS: u32 = 300;
pub const MAX_STEP_TIMEOUT_SECONDS: u32 = 3600;
pub const MAX_COMMAND_ARGS: usize = 32;
/// The program and all arguments of one command together.
pub const MAX_COMMAND_BYTES: usize = 4096;
pub const MAX_VALIDATION_PROFILES: usize = 64;
/// How long after its timeout a `running` result is still believed to be running.
pub const VALIDATION_DEADLINE_GRACE_MS: u64 = 60_000;
const MAX_VALIDATION_BYTES: usize = 8 * 1024 * 1024;
const MAX_PROFILE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationState {
    /// The attempt was started and its outcome is not known yet.
    Running,
    /// It ran and produced the expected evidence.
    Passed,
    /// It ran and did not produce the expected evidence.
    Failed,
    /// It was not run because a prerequisite step has not passed.
    Skipped,
    /// It could not be run in the target environment at all.
    Unavailable,
    /// It may have run, but its outcome could not be confirmed.
    Unverified,
    /// HS2-17ABM2: it was stopped on request before it finished. Not a failure: the
    /// retry policy does not take another attempt, and a new request runs it again.
    Cancelled,
}

impl ValidationState {
    pub const ALL: [Self; 7] = [
        Self::Running,
        Self::Passed,
        Self::Failed,
        Self::Skipped,
        Self::Unavailable,
        Self::Unverified,
        Self::Cancelled,
    ];

    pub fn is_final(self) -> bool {
        self != Self::Running
    }

    /// A result may be recorded as started, or directly as never having run.
    pub fn may_begin(self) -> bool {
        matches!(self, Self::Running | Self::Skipped | Self::Unavailable)
    }

    /// The only edges a result may take. A started attempt is never `skipped`, and a
    /// finished result never changes.
    pub fn may_become(self, next: Self) -> bool {
        self == Self::Running
            && matches!(
                next,
                Self::Passed
                    | Self::Failed
                    | Self::Unavailable
                    | Self::Unverified
                    | Self::Cancelled
            )
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Unavailable => "unavailable",
            Self::Unverified => "unverified",
            Self::Cancelled => "cancelled",
        }
    }
}

/// What a step runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ValidationCheck {
    /// A program and its arguments, chosen by the orchestration project. No shell.
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// A test profile the target project defines in its own checked-in file.
    Profile { name: String },
}

impl ValidationCheck {
    /// A profile is what the target project itself offers, so a remembered
    /// `validation_run` rule covers it (HS2-XHQ1EH). A command chosen elsewhere is
    /// outside that scope and asks a person for each operation.
    pub fn in_granted_scope(&self) -> bool {
        matches!(self, Self::Profile { .. })
    }

    pub fn validate(&self) -> Result<(), OrchestrationError> {
        match self {
            Self::Profile { name } => valid_identifier(name),
            Self::Command { program, args } => command(program, args),
        }
    }
}

fn command(program: &str, args: &[String]) -> Result<(), OrchestrationError> {
    let bytes = program.len() + args.iter().map(String::len).sum::<usize>();
    if !bounded_text(program, MAX_COMMAND_BYTES)
        || args.len() > MAX_COMMAND_ARGS
        || bytes > MAX_COMMAND_BYTES
        || args.iter().any(|arg| arg.contains('\0'))
    {
        return Err(OrchestrationError::Invalid(
            "bounded validation command required",
        ));
    }
    Ok(())
}

/// What a run must produce to pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedEvidence {
    /// The exit code that means success; zero when omitted.
    #[serde(default)]
    pub exit_code: i32,
    /// When set, the SHA-256 of everything the step wrote to standard output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryPolicy {
    /// Attempts one request may make while the result is `failed`; 1 means no retry.
    #[serde(default = "one")]
    pub max_attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 1 }
    }
}

fn one() -> u32 {
    1
}

fn default_timeout() -> u32 {
    DEFAULT_STEP_TIMEOUT_SECONDS
}

/// What a caller supplies to define a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDefinition {
    /// The step's name, unique within its orchestration ticket.
    pub step_id: String,
    /// The declared target project the step runs in.
    pub target_id: String,
    pub check: ValidationCheck,
    /// Steps of the same ticket whose latest evidence must be `passed` first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub evidence: ExpectedEvidence,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
    #[serde(default)]
    pub retry: RetryPolicy,
}

impl StepDefinition {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        valid_identifier(&self.step_id)?;
        valid_token(&self.target_id)?;
        self.check.validate()?;
        if self.prerequisites.len() > MAX_STEP_PREREQUISITES {
            return Err(OrchestrationError::Invalid("at most 8 prerequisite steps"));
        }
        for (index, step) in self.prerequisites.iter().enumerate() {
            valid_identifier(step)?;
            if *step == self.step_id || self.prerequisites[..index].contains(step) {
                return Err(OrchestrationError::Invalid(
                    "prerequisites name other steps once each",
                ));
            }
        }
        if let Some(digest) = &self.evidence.stdout_sha256 {
            valid_digest(digest)?;
        }
        if !(1..=MAX_STEP_TIMEOUT_SECONDS).contains(&self.timeout_seconds) {
            return Err(OrchestrationError::Invalid(
                "timeout seconds must be 1 to 3600",
            ));
        }
        if !(1..=MAX_STEP_ATTEMPTS).contains(&self.retry.max_attempts) {
            return Err(OrchestrationError::Invalid("max attempts must be 1 to 5"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationStep {
    pub owner_checkout_id: String,
    /// The orchestration ticket the step is attached to.
    pub ticket: RunTicket,
    pub definition: StepDefinition,
    /// Starts at 1 and grows with every changed definition. Evidence names the
    /// revision it was produced for.
    pub revision: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

/// An immutable reference to one output stream: what it was, never its content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct StreamRef {
    pub sha256: String,
    pub bytes: u64,
}

/// How an execution ended on the machine that hosts the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionEnd {
    Exited,
    TimedOut,
    Signalled,
    /// The step wrote more than the host reads; it was stopped.
    OutputLimit,
    /// The target project defines no profile of that name.
    ProfileNotDefined,
    ProgramNotFound,
    LaunchFailed,
    /// HS2-17ABM2: stopped on request before it ended by itself.
    Cancelled,
}

/// What the host observed. Codes, numbers and digests only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct ExecutionOutcome {
    pub end: ExecutionEnd,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<StreamRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<StreamRef>,
}

impl ExecutionOutcome {
    fn validate(&self) -> Result<(), OrchestrationError> {
        for stream in [&self.stdout, &self.stderr].into_iter().flatten() {
            valid_digest(&stream.sha256)?;
        }
        Ok(())
    }
}

/// The result state an outcome amounts to under a step's expected evidence, with the
/// reason code when it is not `passed`. Pure.
pub fn evaluate(
    expected: &ExpectedEvidence,
    outcome: &ExecutionOutcome,
) -> (ValidationState, Option<&'static str>) {
    use ValidationState::{Cancelled, Failed, Passed, Unavailable, Unverified};
    match outcome.end {
        ExecutionEnd::Cancelled => (Cancelled, Some("cancelled")),
        ExecutionEnd::ProfileNotDefined => (Unavailable, Some("profile_not_defined")),
        ExecutionEnd::ProgramNotFound => (Unavailable, Some("program_not_found")),
        ExecutionEnd::LaunchFailed => (Unavailable, Some("launch_failed")),
        ExecutionEnd::TimedOut => (Failed, Some("timed_out")),
        ExecutionEnd::Signalled => (Failed, Some("terminated_by_signal")),
        ExecutionEnd::OutputLimit => (Failed, Some("output_limit_exceeded")),
        ExecutionEnd::Exited => match outcome.exit_code {
            // An exit without a code proves nothing either way.
            None => (Unverified, Some("exit_code_unknown")),
            Some(code) if code != expected.exit_code => (Failed, Some("exit_code_mismatch")),
            Some(_) => match (&expected.stdout_sha256, &outcome.stdout) {
                (None, _) => (Passed, None),
                (Some(digest), Some(stream)) if *digest == stream.sha256 => (Passed, None),
                (Some(_), Some(_)) => (Failed, Some("output_mismatch")),
                (Some(_), None) => (Unverified, Some("output_unknown")),
            },
        },
    }
}

/// One attempt at a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationResult {
    /// Also the execution id the host knows the attempt by.
    pub result_id: String,
    /// The caller's idempotency key; every attempt of one request shares it.
    pub operation_id: String,
    pub attempt: u32,
    pub step_revision: u32,
    pub state: ValidationState,
    /// A stable code, never free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    /// The dispatch run this validation belongs to, when the caller named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The host confirmed that it accepted the execution.
    #[serde(default)]
    pub host_started: bool,
    pub created_at_ms: u64,
    /// While `running`: when the attempt stops being believed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ExecutionOutcome>,
}

/// What a caller supplies to record a new attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewResult {
    pub operation_id: String,
    /// `running`, or `skipped` / `unavailable` for an attempt that never ran.
    pub state: ValidationState,
    pub reason: Option<String>,
    pub actor_id: Option<String>,
    pub run_id: Option<String>,
}

/// A change to a `running` result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultUpdate {
    pub to: Option<ValidationState>,
    pub reason: Option<String>,
    pub outcome: Option<ExecutionOutcome>,
    pub host_started: Option<bool>,
}

/// The latest evidence for one step, and how far it can be relied on now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationEvidence {
    pub step: ValidationStep,
    /// What the latest evidence amounts to now. `unverified` when the step never ran,
    /// when its definition changed since, or when a running attempt outlived its
    /// deadline.
    pub state: ValidationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The most recent attempt, exactly as recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<ValidationResult>,
    /// Retained attempts, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<ValidationResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationError {
    StepNotFound,
    /// An attempt of another request is still running; it must end or lapse first.
    StepRunning,
    ResultNotFound,
    /// A finished result is immutable.
    ResultFinal,
    InvalidTransition,
}

impl ValidationError {
    pub fn code(self) -> &'static str {
        match self {
            Self::StepNotFound => "validation_step_not_found",
            Self::StepRunning => "validation_step_running",
            Self::ResultNotFound => "validation_result_not_found",
            Self::ResultFinal => "validation_result_final",
            Self::InvalidTransition => "validation_invalid_transition",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepRecord {
    step: ValidationStep,
    #[serde(default)]
    results: Vec<ValidationResult>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidationFile {
    schema_version: u32,
    steps: Vec<StepRecord>,
}

/// What the latest attempt amounts to at `now_ms`. Pure.
pub fn effective(
    step: &ValidationStep,
    latest: Option<&ValidationResult>,
    now_ms: u64,
) -> (ValidationState, Option<String>) {
    let Some(latest) = latest else {
        return (ValidationState::Unverified, Some("not_run".into()));
    };
    if latest.step_revision != step.revision {
        return (ValidationState::Unverified, Some("step_changed".into()));
    }
    if lapsed(latest, now_ms) {
        return (ValidationState::Unverified, Some("deadline_passed".into()));
    }
    (latest.state, latest.reason.clone())
}

/// Whether the request that made `previous` may take another attempt: only after a
/// `failed` one, for the same definition, while the retry policy has attempts left.
pub fn may_retry(step: &ValidationStep, previous: &ValidationResult) -> bool {
    previous.state == ValidationState::Failed
        && previous.step_revision == step.revision
        && previous.attempt < step.definition.retry.max_attempts
}

/// HS2-J7N3PQ: the first instant after `now_ms` at which a running attempt among this
/// evidence passes its deadline, so a level computed from it changes by time alone.
/// `None` when no running attempt is still within its deadline.
pub fn next_lapse_ms(evidence: &[ValidationEvidence], now_ms: u64) -> Option<u64> {
    evidence
        .iter()
        .filter_map(|evidence| evidence.latest.as_ref())
        .filter(|result| result.state == ValidationState::Running)
        .filter_map(|result| result.deadline_ms)
        .filter(|deadline| *deadline >= now_ms)
        .map(|deadline| deadline.saturating_add(1))
        .min()
}

fn lapsed(result: &ValidationResult, now_ms: u64) -> bool {
    result.state == ValidationState::Running
        && result.deadline_ms.is_some_and(|deadline| now_ms > deadline)
}

impl StepRecord {
    fn evidence(&self, now_ms: u64) -> ValidationEvidence {
        let (state, reason) = effective(&self.step, self.results.last(), now_ms);
        ValidationEvidence {
            step: self.step.clone(),
            state,
            reason,
            latest: self.results.last().cloned(),
            history: self.results.clone(),
        }
    }

    /// Record that running attempts past their deadline can no longer be confirmed.
    fn expire(&mut self, now_ms: u64) {
        for result in &mut self.results {
            if lapsed(result, now_ms) {
                result.state = ValidationState::Unverified;
                result.reason = Some("deadline_passed".into());
                result.finished_at_ms = Some(now_ms);
                result.deadline_ms = None;
            }
        }
    }

    fn running(&self) -> Option<&ValidationResult> {
        self.results
            .iter()
            .find(|result| result.state == ValidationState::Running)
    }
}

#[derive(Clone)]
pub struct ValidationRegistry {
    storage: Storage,
}

impl ValidationRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-validation.json"),
        }
    }

    fn read_file(&self) -> Result<ValidationFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_VALIDATION_BYTES)? else {
            return Ok(ValidationFile {
                schema_version: ORCHESTRATION_VERSION,
                steps: Vec::new(),
            });
        };
        let file: ValidationFile = decode(&bytes)?;
        for record in &file.steps {
            validate(record).map_err(|_| OrchestrationError::Corrupt)?;
        }
        Ok(file)
    }

    fn write_file(&self, file: &ValidationFile) -> Result<(), OrchestrationError> {
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_VALIDATION_BYTES)
    }

    /// Define a step, or change its definition. The identical definition again changes
    /// nothing (`false`). A changed one gets the next revision, which makes earlier
    /// evidence `unverified`; it is refused while an attempt is running.
    pub fn define(
        &self,
        owner_checkout_id: &str,
        ticket: RunTicket,
        definition: StepDefinition,
        actor_id: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<(ValidationStep, bool), OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        definition.validate()?;
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let of_ticket = |record: &&mut StepRecord| {
            record.step.owner_checkout_id == owner_checkout_id && record.step.ticket == ticket
        };
        if let Some(record) = file
            .steps
            .iter_mut()
            .filter(of_ticket)
            .find(|record| record.step.definition.step_id == definition.step_id)
        {
            if record.step.definition == definition {
                return Ok((record.step.clone(), false));
            }
            record.expire(now_ms);
            if record.running().is_some() {
                return Err(OrchestrationError::Validation(ValidationError::StepRunning));
            }
            record.step.definition = definition;
            record.step.revision = record.step.revision.saturating_add(1);
            record.step.updated_at_ms = now_ms;
            record.step.updated_by = actor_id;
            let step = record.step.clone();
            validate(record)?;
            self.write_file(&file)?;
            return Ok((step, true));
        }
        if file.steps.iter_mut().filter(of_ticket).count() >= MAX_STEPS_PER_TICKET
            || file.steps.len() >= MAX_VALIDATION_STEPS
        {
            return Err(OrchestrationError::Capacity);
        }
        let record = StepRecord {
            step: ValidationStep {
                owner_checkout_id: owner_checkout_id.into(),
                ticket,
                definition,
                revision: 1,
                created_at_ms: now_ms,
                updated_at_ms: now_ms,
                updated_by: actor_id,
            },
            results: Vec::new(),
        };
        validate(&record)?;
        let step = record.step.clone();
        file.steps.push(record);
        self.write_file(&file)?;
        Ok((step, true))
    }

    /// Remove a step and its retained results. Refused while an attempt is running.
    pub fn remove(
        &self,
        owner_checkout_id: &str,
        ticket: &RunTicket,
        step_id: &str,
        now_ms: u64,
    ) -> Result<ValidationStep, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_identifier(step_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let index = position(&file, owner_checkout_id, ticket, step_id)?;
        if file.steps[index]
            .results
            .iter()
            .any(|result| result.state == ValidationState::Running && !lapsed(result, now_ms))
        {
            return Err(OrchestrationError::Validation(ValidationError::StepRunning));
        }
        let removed = file.steps.remove(index).step;
        self.write_file(&file)?;
        Ok(removed)
    }

    /// The latest evidence of every step of one owner, or of one of its tickets, in
    /// the order the steps were defined.
    pub fn steps(
        &self,
        owner_checkout_id: &str,
        ticket: Option<&RunTicket>,
        now_ms: u64,
    ) -> Result<Vec<ValidationEvidence>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(self
            .read_file()?
            .steps
            .iter()
            .filter(|record| record.step.owner_checkout_id == owner_checkout_id)
            .filter(|record| ticket.is_none_or(|ticket| record.step.ticket == *ticket))
            .map(|record| record.evidence(now_ms))
            .collect())
    }

    /// The latest validation evidence for one orchestration ticket and step. This is
    /// the read a readiness decision uses: it writes nothing and contacts nothing.
    pub fn evidence(
        &self,
        owner_checkout_id: &str,
        ticket: &RunTicket,
        step_id: &str,
        now_ms: u64,
    ) -> Result<ValidationEvidence, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_identifier(step_id)?;
        let file = self.read_file()?;
        let index = position(&file, owner_checkout_id, ticket, step_id)?;
        Ok(file.steps[index].evidence(now_ms))
    }

    /// The first prerequisite of a step whose latest evidence is not `passed` now,
    /// including one that is not defined.
    pub fn unmet_prerequisite(
        &self,
        owner_checkout_id: &str,
        ticket: &RunTicket,
        step_id: &str,
        now_ms: u64,
    ) -> Result<Option<String>, OrchestrationError> {
        let file = self.read_file()?;
        let index = position(&file, owner_checkout_id, ticket, step_id)?;
        let step = &file.steps[index].step;
        Ok(step
            .definition
            .prerequisites
            .iter()
            .find(|needed| {
                !file.steps.iter().any(|record| {
                    record.step.owner_checkout_id == owner_checkout_id
                        && record.step.ticket == *ticket
                        && record.step.definition.step_id == **needed
                        && effective(&record.step, record.results.last(), now_ms).0
                            == ValidationState::Passed
                })
            })
            .cloned())
    }

    /// Record a new attempt, or return the attempt an earlier request with the same
    /// operation id is at (`false`).
    ///
    /// - The same operation id continues its own request: an unfinished attempt is
    ///   returned as it is, a `failed` one is followed by the next attempt while the
    ///   step's retry policy allows, and any other finished result is returned.
    /// - The same operation id for another step conflicts.
    /// - A different operation id is refused while an attempt is running.
    pub fn begin(
        &self,
        owner_checkout_id: &str,
        ticket: &RunTicket,
        step_id: &str,
        new: NewResult,
        tx: &mut Transaction<'_>,
    ) -> Result<(ValidationResult, bool), OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_identifier(step_id)?;
        valid_identifier(&new.operation_id)?;
        if !new.state.may_begin() {
            return Err(OrchestrationError::Invalid(
                "a result begins running, skipped or unavailable",
            ));
        }
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let index = position(&file, owner_checkout_id, ticket, step_id)?;
        if file.steps.iter().enumerate().any(|(other, record)| {
            other != index
                && record.step.owner_checkout_id == owner_checkout_id
                && record
                    .results
                    .iter()
                    .any(|result| result.operation_id == new.operation_id)
        }) {
            return Err(OrchestrationError::Conflict);
        }
        let record = &mut file.steps[index];
        let before = record.results.clone();
        record.expire(now_ms);
        let step = &record.step;
        let mut attempt = 1;
        if let Some(previous) = record
            .results
            .iter()
            .rev()
            .find(|result| result.operation_id == new.operation_id)
        {
            let retry = new.state == ValidationState::Running && may_retry(step, previous);
            if !retry {
                let previous = previous.clone();
                if record.results != before {
                    self.write_file(&file)?;
                }
                return Ok((previous, false));
            }
            attempt = previous.attempt + 1;
        }
        if record.running().is_some() {
            return Err(OrchestrationError::Validation(ValidationError::StepRunning));
        }
        let running = new.state == ValidationState::Running;
        let result = ValidationResult {
            result_id: tx.token(),
            operation_id: new.operation_id,
            attempt,
            step_revision: step.revision,
            state: new.state,
            reason: new.reason,
            actor_id: new.actor_id,
            run_id: new.run_id,
            host_started: false,
            created_at_ms: now_ms,
            deadline_ms: running.then(|| {
                now_ms
                    .saturating_add(u64::from(step.definition.timeout_seconds) * 1000)
                    .saturating_add(VALIDATION_DEADLINE_GRACE_MS)
            }),
            finished_at_ms: (!running).then_some(now_ms),
            outcome: None,
        };
        record.results.push(result.clone());
        // Keep the newest results; the one just recorded is never the one dropped.
        while record.results.len() > MAX_STEP_RESULTS {
            record.results.remove(0);
        }
        validate(record)?;
        self.write_file(&file)?;
        Ok((result, true))
    }

    /// Apply one change to a `running` result. A state change must follow
    /// [`ValidationState::may_become`]. A finished result refuses every change except
    /// the identical report again, which is a no-op, so a result reference is stable.
    pub fn finish(
        &self,
        owner_checkout_id: &str,
        ticket: &RunTicket,
        step_id: &str,
        result_id: &str,
        update: ResultUpdate,
        tx: &mut Transaction<'_>,
    ) -> Result<ValidationResult, OrchestrationError> {
        self.settle(owner_checkout_id, ticket, step_id, result_id, update, tx)
            .map(|(result, _)| result)
    }

    /// [`Self::finish`], also saying whether this call changed the record. Of two
    /// callers that settle the same attempt at once (a cancel racing the request that
    /// observed the step's own end), only the one that wrote it reports it
    /// (HS2-17ABM2).
    pub fn settle(
        &self,
        owner_checkout_id: &str,
        ticket: &RunTicket,
        step_id: &str,
        result_id: &str,
        update: ResultUpdate,
        tx: &mut Transaction<'_>,
    ) -> Result<(ValidationResult, bool), OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_identifier(step_id)?;
        valid_token(result_id)?;
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let index = position(&file, owner_checkout_id, ticket, step_id)?;
        let record = &mut file.steps[index];
        let result = record
            .results
            .iter_mut()
            .find(|result| result.result_id == result_id)
            .ok_or(OrchestrationError::Validation(
                ValidationError::ResultNotFound,
            ))?;
        if result.state.is_final() {
            let same = update.to.is_none_or(|to| to == result.state)
                && update
                    .outcome
                    .as_ref()
                    .is_none_or(|outcome| Some(outcome) == result.outcome.as_ref())
                && update
                    .reason
                    .as_ref()
                    .is_none_or(|reason| Some(reason) == result.reason.as_ref());
            return if same {
                Ok((result.clone(), false))
            } else {
                Err(OrchestrationError::Validation(ValidationError::ResultFinal))
            };
        }
        let before = result.clone();
        if let Some(started) = update.host_started {
            // The host's acceptance is never taken back.
            result.host_started |= started;
        }
        if let Some(to) = update.to.filter(|to| *to != result.state) {
            if !result.state.may_become(to) {
                return Err(OrchestrationError::Validation(
                    ValidationError::InvalidTransition,
                ));
            }
            result.state = to;
            result.reason = update.reason;
            result.outcome = update.outcome;
            result.finished_at_ms = Some(now_ms);
            result.deadline_ms = None;
        }
        if *result == before {
            return Ok((before, false));
        }
        let updated = result.clone();
        validate(record)?;
        self.write_file(&file)?;
        Ok((updated, true))
    }
}

fn position(
    file: &ValidationFile,
    owner_checkout_id: &str,
    ticket: &RunTicket,
    step_id: &str,
) -> Result<usize, OrchestrationError> {
    file.steps
        .iter()
        .position(|record| {
            record.step.owner_checkout_id == owner_checkout_id
                && record.step.ticket == *ticket
                && record.step.definition.step_id == step_id
        })
        .ok_or(OrchestrationError::Validation(
            ValidationError::StepNotFound,
        ))
}

fn native(value: &str) -> Result<(), OrchestrationError> {
    if value.is_empty()
        || value.len() > 200
        || value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\'))
    {
        return Err(OrchestrationError::Invalid(
            "bounded native ticket id required",
        ));
    }
    Ok(())
}

fn validate(record: &StepRecord) -> Result<(), OrchestrationError> {
    let step = &record.step;
    valid_identifier(&step.owner_checkout_id)?;
    valid_identifier(&step.ticket.connection_id)?;
    native(&step.ticket.native_id)?;
    step.definition.validate()?;
    if step.revision == 0 || record.results.len() > MAX_STEP_RESULTS {
        return Err(OrchestrationError::Invalid(
            "bounded validation step required",
        ));
    }
    if let Some(actor) = &step.updated_by {
        valid_identifier(actor)?;
    }
    for result in &record.results {
        valid_token(&result.result_id)?;
        valid_identifier(&result.operation_id)?;
        for value in [&result.reason, &result.actor_id].into_iter().flatten() {
            valid_identifier(value)?;
        }
        if let Some(run) = &result.run_id {
            valid_token(run)?;
        }
        if let Some(outcome) = &result.outcome {
            outcome.validate()?;
        }
        if result.attempt == 0 || result.attempt > MAX_STEP_ATTEMPTS {
            return Err(OrchestrationError::Invalid("bounded attempt required"));
        }
    }
    Ok(())
}

/// A test profile a target project offers: a name for a command it chose itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationProfile {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// HS2-YYNW9F: the longest the target project lets this profile run, 1 to 3600
    /// seconds. A step's own timeout still bounds it; the shorter one applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
}

impl ValidationProfile {
    /// How long a step with `step_timeout_seconds` may run this profile.
    pub fn effective_timeout(&self, step_timeout_seconds: u32) -> u32 {
        self.timeout_seconds
            .map_or(step_timeout_seconds, |own| own.min(step_timeout_seconds))
    }

    /// The first thing wrong with this profile on its own, as a stable code.
    fn problem(&self) -> Option<&'static str> {
        if valid_identifier(&self.name).is_err() {
            Some("invalid_name")
        } else if command(&self.program, &self.args).is_err() {
            Some("invalid_command")
        } else if self
            .timeout_seconds
            .is_some_and(|seconds| !(1..=MAX_STEP_TIMEOUT_SECONDS).contains(&seconds))
        {
            Some("invalid_timeout")
        } else {
            None
        }
    }
}

/// HS2-YYNW9F: one thing wrong with a profile file. Codes and positions only; a name
/// is echoed only when it is itself a valid profile name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct ProfileProblem {
    pub code: String,
    /// The profile's position in the file, from 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// HS2-YYNW9F: what a checkout's profile file offers, and everything wrong with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct ProfileCheck {
    /// Whether the file exists. An absent file offers no profiles and is valid.
    pub present: bool,
    /// No problems: every profile below can be run by name.
    pub valid: bool,
    /// The profiles, when the file could be read as a whole; empty otherwise.
    pub profiles: Vec<ValidationProfile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<ProfileProblem>,
}

impl ProfileCheck {
    fn refused(code: &str) -> Self {
        Self {
            present: true,
            valid: false,
            profiles: Vec::new(),
            problems: vec![ProfileProblem {
                code: code.into(),
                index: None,
                name: None,
            }],
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    schema_version: u32,
    #[serde(default)]
    profiles: Vec<ValidationProfile>,
}

/// The checked-in `.hotsheet2/validation-profiles.json` of a target checkout. It is
/// the target project's own statement of what may be run for validation; an
/// orchestration project names a profile and never supplies its command.
#[derive(Clone)]
pub struct ValidationProfileStore {
    storage: Storage,
}

impl ValidationProfileStore {
    pub fn new(checkout: &Path) -> Self {
        Self {
            storage: Storage::shared_named(checkout, ".hotsheet2/validation-profiles.json"),
        }
    }

    /// Every profile, or none when the file is absent. A read never creates it; a
    /// damaged, duplicated or newer file is an error and never a partial answer.
    pub fn read(&self) -> Result<Vec<ValidationProfile>, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_PROFILE_BYTES)? else {
            return Ok(Vec::new());
        };
        let file: ProfileFile = decode(&bytes)?;
        if file.profiles.len() > MAX_VALIDATION_PROFILES {
            return Err(OrchestrationError::Corrupt);
        }
        for (index, profile) in file.profiles.iter().enumerate() {
            if profile.problem().is_some() {
                return Err(OrchestrationError::Corrupt);
            }
            if file.profiles[..index]
                .iter()
                .any(|other| other.name == profile.name)
            {
                return Err(OrchestrationError::Corrupt);
            }
        }
        Ok(file.profiles)
    }

    /// HS2-YYNW9F: check the file as a validator would, reporting every problem
    /// rather than the first. Like [`Self::read`] it never creates the file. Only an
    /// I/O failure is an error; anything wrong with the content is in the answer.
    pub fn check(&self) -> Result<ProfileCheck, OrchestrationError> {
        let bytes = match self.storage.read(MAX_PROFILE_BYTES) {
            Ok(None) => {
                return Ok(ProfileCheck {
                    present: false,
                    valid: true,
                    profiles: Vec::new(),
                    problems: Vec::new(),
                });
            }
            Ok(Some(bytes)) => bytes,
            Err(OrchestrationError::Corrupt) => return Ok(ProfileCheck::refused("too_large")),
            Err(error) => return Err(error),
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return Ok(ProfileCheck::refused("invalid_json"));
        };
        match value.get("schema_version").map(serde_json::Value::as_u64) {
            None | Some(None) => return Ok(ProfileCheck::refused("schema_version_missing")),
            Some(Some(1)) => {}
            Some(Some(_)) => return Ok(ProfileCheck::refused("unsupported_schema_version")),
        }
        let Ok(file) = serde_json::from_value::<ProfileFile>(value) else {
            return Ok(ProfileCheck::refused("invalid_shape"));
        };
        let mut problems = Vec::new();
        if file.profiles.len() > MAX_VALIDATION_PROFILES {
            problems.push(ProfileProblem {
                code: "too_many_profiles".into(),
                index: None,
                name: None,
            });
        }
        for (index, profile) in file.profiles.iter().enumerate() {
            let name = valid_identifier(&profile.name)
                .is_ok()
                .then(|| profile.name.clone());
            let problem = profile.problem().or_else(|| {
                file.profiles[..index]
                    .iter()
                    .any(|other| other.name == profile.name)
                    .then_some("duplicate_name")
            });
            if let Some(code) = problem {
                problems.push(ProfileProblem {
                    code: code.into(),
                    index: Some(index),
                    name,
                });
            }
        }
        Ok(ProfileCheck {
            present: true,
            valid: problems.is_empty(),
            profiles: file.profiles,
            problems,
        })
    }

    pub fn find(&self, name: &str) -> Result<Option<ValidationProfile>, OrchestrationError> {
        valid_identifier(name)?;
        Ok(self
            .read()?
            .into_iter()
            .find(|profile| profile.name == name))
    }
}
