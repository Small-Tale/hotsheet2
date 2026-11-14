//! HS2-WN0MXZ: extra validation steps for orchestrated work.
//!
//! The hosted half runs a step where the target lives: in the target checkout, on the
//! machine that hosts it. It digests the step's output as it reads it and keeps none of
//! it, so only digests, sizes, the exit code and timings ever leave that machine. The
//! orchestrating half passes the gate as `validation_run`, starts or re-attaches to an
//! execution by a retained operation id, and records what the host observed.
//!
//! An execution outlives one request: the transport allows about ten seconds, a step
//! up to an hour. A request starts the execution and waits a bounded time; repeating it
//! with the same operation id waits again. Nothing polls on a timer between requests.
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use hotsheet_model::NoteKind;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::ops;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, AuditRecord, ExecutionEnd, ExecutionOutcome, MAX_STEP_TIMEOUT_SECONDS, NewResult,
    ResultUpdate, RunEvent, RunEventKind, RunEventSource, RunTicket, StepDefinition, StreamRef,
    TargetAction, ValidationCheck, ValidationEvidence, ValidationProfileStore, ValidationRegistry,
    ValidationResult, ValidationState, evaluate, may_retry,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::tickets::{encode, identifier, timestamp};
use super::*;

/// The longest one request waits for an execution; a caller repeats the request.
pub const MAX_VALIDATION_WAIT_MS: u64 = 8_000;
/// The most one output stream may write before the step is stopped.
pub const MAX_VALIDATION_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;
/// Executions one process remembers, and how many of them may run at once.
const MAX_HOSTED_EXECUTIONS: usize = 64;
const MAX_RUNNING_EXECUTIONS: usize = 4;
/// How long a stopped step's output readers are waited for before their streams are
/// reported as unknown.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

// ---- Hosted half ----

/// Start one execution on the machine that hosts the checkout, or return the one this
/// id already names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedValidationStart {
    /// Chosen by the orchestrator; the same id never runs twice in one host process.
    pub execution_id: String,
    pub check: ValidationCheck,
    pub timeout_seconds: u32,
    /// How long to wait for the execution to finish before answering `running`.
    #[serde(default)]
    pub wait_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedExecutionState {
    Running,
    Finished,
    /// This host process has no such execution: it never started, or the process that
    /// ran it is gone. The host cannot tell which.
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedValidation {
    pub execution_id: String,
    pub state: HostedExecutionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ExecutionOutcome>,
}

struct Execution {
    key: String,
    outcome: Mutex<Option<ExecutionOutcome>>,
    done: Condvar,
}

impl Execution {
    fn finished(&self) -> Option<ExecutionOutcome> {
        self.outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The outcome, waiting at most `wait_ms` for it; woken when the step ends.
    fn wait(&self, wait_ms: u64) -> Option<ExecutionOutcome> {
        let guard = self
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (guard, _) = self
            .done
            .wait_timeout_while(guard, Duration::from_millis(wait_ms), |outcome| {
                outcome.is_none()
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.clone()
    }

    fn finish(&self, outcome: ExecutionOutcome) {
        *self
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome);
        self.done.notify_all();
    }
}

/// Executions of this process, oldest first. They are deliberately not persisted: a
/// process that is gone cannot vouch for a step it was running.
static EXECUTIONS: OnceLock<Mutex<Vec<Arc<Execution>>>> = OnceLock::new();

fn executions() -> std::sync::MutexGuard<'static, Vec<Arc<Execution>>> {
    EXECUTIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn ended(end: ExecutionEnd, started_at_ms: u64, started: Instant) -> ExecutionOutcome {
    ExecutionOutcome {
        end,
        exit_code: None,
        started_at_ms,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        stdout: None,
        stderr: None,
    }
}

/// Read a stream to its end, keeping only its digest and size. Past `limit` it stops
/// reading and says so.
fn digest(mut stream: impl Read, limit: u64, over: &AtomicBool) -> Option<StreamRef> {
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                bytes += read as u64;
                if bytes > limit {
                    over.store(true, Ordering::SeqCst);
                    return None;
                }
                hash.update(&buffer[..read]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(StreamRef {
        sha256: hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        bytes,
    })
}

/// Stop a step and everything it started.
fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        // SAFETY: `killpg` takes plain integers and touches no memory. The group id is
        // the child's own process group, created by `process_group(0)` at spawn, and
        // the child has not been reaped yet, so the id cannot name another group.
        unsafe {
            libc::killpg(group, libc::SIGKILL);
        }
    }
    let _ = child.kill();
}

/// Run one command in `root` and report how it ended. Blocks until it has.
pub(super) fn execute(
    root: &Path,
    program: &str,
    args: &[String],
    timeout: Duration,
    output_limit: u64,
    started_at_ms: u64,
) -> ExecutionOutcome {
    let started = Instant::now();
    // A relative program with a directory part is the checkout's own script; a bare
    // name is looked up on the host's PATH.
    let path = Path::new(program);
    let resolved = if path.is_relative() && path.components().count() > 1 {
        root.join(path)
    } else {
        path.to_path_buf()
    };
    let mut command = hotsheet_ticketing::git::launch(&resolved);
    command
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ended(ExecutionEnd::ProgramNotFound, started_at_ms, started);
        }
        Err(_) => return ended(ExecutionEnd::LaunchFailed, started_at_ms, started),
    };
    let over = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let mut readers = 0;
    let streams: [Option<Box<dyn Read + Send>>; 2] = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ];
    for (index, stream) in streams.into_iter().enumerate() {
        let Some(stream) = stream else { continue };
        let (sender, over) = (sender.clone(), over.clone());
        readers += 1;
        std::thread::spawn(move || {
            let _ = sender.send((index, digest(stream, output_limit, &over)));
        });
    }
    drop(sender);
    let mut end = ExecutionEnd::Exited;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if over.load(Ordering::SeqCst) {
            end = ExecutionEnd::OutputLimit;
        } else if started.elapsed() >= timeout {
            end = ExecutionEnd::TimedOut;
        } else {
            // A local wait on this process's own child; no request is made.
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        stop(&mut child);
        break child.wait().ok();
    };
    let mut outcome = ended(end, started_at_ms, started);
    for _ in 0..readers {
        match receiver.recv_timeout(DRAIN_GRACE) {
            Ok((0, stream)) => outcome.stdout = stream,
            Ok((_, stream)) => outcome.stderr = stream,
            Err(_) => break,
        }
    }
    if over.load(Ordering::SeqCst) && end == ExecutionEnd::Exited {
        // It exited on its own after writing too much; its output was not all read.
        outcome.end = ExecutionEnd::OutputLimit;
    }
    if outcome.end == ExecutionEnd::Exited {
        match status.and_then(|status| status.code()) {
            Some(code) => outcome.exit_code = Some(code),
            None => outcome.end = ExecutionEnd::Signalled,
        }
    }
    outcome
}

fn hosted_reply(execution_id: &str, outcome: Option<ExecutionOutcome>) -> HostedValidation {
    HostedValidation {
        execution_id: execution_id.into(),
        state: if outcome.is_some() {
            HostedExecutionState::Finished
        } else {
            HostedExecutionState::Running
        },
        outcome,
    }
}

fn wait(wait_ms: u64) -> Result<u64, OrchestrationError> {
    if wait_ms > MAX_VALIDATION_WAIT_MS {
        return Err(OrchestrationError::Invalid("wait_ms must be at most 8000"));
    }
    Ok(wait_ms)
}

fn execution_id(value: &str) -> Result<(), OrchestrationError> {
    if value
        .parse::<ulid::Ulid>()
        .ok()
        .map(|id| id.to_string())
        .as_deref()
        != Some(value)
    {
        return Err(OrchestrationError::Invalid(
            "canonical execution id required",
        ));
    }
    Ok(())
}

impl OrchestrationRuntime {
    fn execution_key(&self, checkout_id: &str, execution_id: &str) -> String {
        format!("{}|{checkout_id}|{execution_id}", self.home.display())
    }

    /// Start a validation execution in a checkout this machine hosts and wait a
    /// bounded time for it. The same execution id again returns the same execution and
    /// starts nothing. A profile is resolved from the checkout's own file here, so the
    /// caller never supplies its command.
    pub fn hosted_validation_start(
        &self,
        checkout_id: &str,
        request: HostedValidationStart,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedValidation, OrchestrationError> {
        execution_id(&request.execution_id)?;
        request.check.validate()?;
        let wait_ms = wait(request.wait_ms)?;
        if !(1..=MAX_STEP_TIMEOUT_SECONDS).contains(&request.timeout_seconds) {
            return Err(OrchestrationError::Invalid(
                "timeout seconds must be 1 to 3600",
            ));
        }
        let checkout = self.owner(checkout_id)?;
        let root = PathBuf::from(&checkout.root);
        let key = self.execution_key(checkout_id, &request.execution_id);
        let started_at_ms = tx.clock.now_ms();
        let execution = {
            let mut known = executions();
            if let Some(existing) = known.iter().find(|execution| execution.key == key) {
                existing.clone()
            } else {
                // Resolve before anything is remembered: a damaged profile file is an
                // error, never a guess.
                let command = match &request.check {
                    ValidationCheck::Command { program, args } => {
                        Some((program.clone(), args.clone()))
                    }
                    ValidationCheck::Profile { name } => ValidationProfileStore::new(&root)
                        .find(name)?
                        .map(|profile| (profile.program, profile.args)),
                };
                // Forget the oldest finished executions; a running one is never forgotten.
                while known.len() >= MAX_HOSTED_EXECUTIONS {
                    let Some(index) = known
                        .iter()
                        .position(|execution| execution.finished().is_some())
                    else {
                        return Err(OrchestrationError::Capacity);
                    };
                    known.remove(index);
                }
                if command.is_some()
                    && known
                        .iter()
                        .filter(|execution| execution.finished().is_none())
                        .count()
                        >= MAX_RUNNING_EXECUTIONS
                {
                    return Err(OrchestrationError::Capacity);
                }
                let execution = Arc::new(Execution {
                    key,
                    outcome: Mutex::new(None),
                    done: Condvar::new(),
                });
                known.push(execution.clone());
                match command {
                    None => execution.finish(ended(
                        ExecutionEnd::ProfileNotDefined,
                        started_at_ms,
                        Instant::now(),
                    )),
                    Some((program, args)) => {
                        let running = execution.clone();
                        let timeout = Duration::from_secs(u64::from(request.timeout_seconds));
                        std::thread::spawn(move || {
                            running.finish(execute(
                                &root,
                                &program,
                                &args,
                                timeout,
                                MAX_VALIDATION_OUTPUT_BYTES,
                                started_at_ms,
                            ));
                        });
                    }
                }
                execution
            }
        };
        Ok(hosted_reply(&request.execution_id, execution.wait(wait_ms)))
    }

    /// The state of an execution this host process started, waiting a bounded time for
    /// it to finish.
    pub fn hosted_validation(
        &self,
        checkout_id: &str,
        id: &str,
        wait_ms: u64,
    ) -> Result<HostedValidation, OrchestrationError> {
        execution_id(id)?;
        let wait_ms = wait(wait_ms)?;
        self.owner(checkout_id)?;
        let key = self.execution_key(checkout_id, id);
        let execution = executions()
            .iter()
            .find(|execution| execution.key == key)
            .cloned();
        Ok(match execution {
            Some(execution) => hosted_reply(id, execution.wait(wait_ms)),
            None => HostedValidation {
                execution_id: id.into(),
                state: HostedExecutionState::Unknown,
                outcome: None,
            },
        })
    }
}

// ---- Orchestrating half ----

/// Attach a step to an orchestration ticket, or change its definition.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefineValidation {
    pub ticket: OrchestrationTicket,
    pub step: StepDefinition,
}

/// One step of one orchestration ticket.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationStepRef {
    pub ticket: OrchestrationTicket,
    pub step_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListValidations {
    /// Omit for every step of the owning checkout.
    #[serde(default)]
    pub ticket: Option<OrchestrationTicket>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunValidation {
    pub ticket: OrchestrationTicket,
    pub step_id: String,
    /// Retain across repeats: the same id is the same request and never runs twice.
    pub operation_id: String,
    /// The dispatch run this validation belongs to; its event stream then reports it.
    #[serde(default)]
    pub run_id: Option<String>,
    /// How long to wait for the attempt to finish, at most 8000.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// What one run request is at, with the step's latest evidence.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationRunView {
    /// This request's latest attempt.
    pub result: ValidationResult,
    /// Attempts the retry policy still allows this request after a `failed` result.
    pub attempts_left: u32,
    pub evidence: ValidationEvidence,
    /// External state this answer could not confirm, such as `target_execution`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unverified: Vec<&'static str>,
}

impl OrchestrationRuntime {
    pub(super) fn validations(&self) -> ValidationRegistry {
        ValidationRegistry::new(&self.home)
    }

    /// The orchestration ticket as steps are keyed by: its source and canonical id.
    fn validation_ticket(
        &self,
        owner_id: &str,
        ticket: &OrchestrationTicket,
    ) -> Result<RunTicket, OrchestrationError> {
        let (_, id, connection_id) = self.orchestration_store(owner_id, ticket)?;
        Ok(RunTicket {
            connection_id,
            native_id: id.to_string(),
        })
    }

    fn validation_audit(
        &self,
        owner_id: &str,
        target_id: &str,
        actor: Option<&MutationActor>,
        action: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        self.audit_record(
            owner_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: Some(target_id.into()),
                action: action.into(),
                operation_id: None,
                outcome: AuditOutcome::Changed,
                reason: None,
                permission_id: None,
            },
        )
    }

    /// Define a step or change its definition. HS2-XHQ1EH: this changes only the
    /// orchestration project's own record, so it runs nothing, asks nobody and needs
    /// no reachable target; it needs an identified actor and is audited.
    pub fn validation_define(
        &self,
        owner_id: &str,
        request: DefineValidation,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ValidationEvidence, OrchestrationError> {
        request.step.validate()?;
        let target = Self::target_ref(owner_id, &request.step.target_id)?;
        let ticket = self.validation_ticket(owner_id, &request.ticket)?;
        let Some(actor_id) = actor.and_then(|actor| actor.id.clone()) else {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to change a validation step",
            ));
        };
        let step_id = request.step.step_id.clone();
        let (_, changed) = self.validations().define(
            owner_id,
            ticket.clone(),
            request.step,
            Some(actor_id),
            tx,
        )?;
        if changed {
            self.validation_audit(owner_id, &target.target_id, actor, "validation_define", tx)?;
        }
        self.validations()
            .evidence(owner_id, &ticket, &step_id, tx.clock.now_ms())
    }

    /// Remove a step and its retained results. HS2-XHQ1EH: only the orchestration
    /// project's own record changes, so a step of an unbound or unreachable target can
    /// always be cleaned up; it needs an identified actor and is audited.
    pub fn validation_remove(
        &self,
        owner_id: &str,
        request: ValidationStepRef,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ValidationEvidence, OrchestrationError> {
        let ticket = self.validation_ticket(owner_id, &request.ticket)?;
        let now_ms = tx.clock.now_ms();
        let evidence = self
            .validations()
            .evidence(owner_id, &ticket, &request.step_id, now_ms)?;
        let target_id = evidence.step.definition.target_id.clone();
        if actor.and_then(|actor| actor.id.as_ref()).is_none() && !access::is_person(actor) {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to change a validation step",
            ));
        }
        self.validations()
            .remove(owner_id, &ticket, &request.step_id, now_ms)?;
        self.validation_audit(owner_id, &target_id, actor, "validation_remove", tx)?;
        Ok(evidence)
    }

    /// The latest validation evidence for one orchestration ticket and step: the
    /// step, what its latest attempt amounts to now, and the retained attempts. Reads
    /// this machine's record only; it contacts no target and writes nothing.
    pub fn validation_evidence(
        &self,
        owner_id: &str,
        ticket: &OrchestrationTicket,
        step_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<ValidationEvidence, OrchestrationError> {
        let ticket = self.validation_ticket(owner_id, ticket)?;
        self.validations()
            .evidence(owner_id, &ticket, step_id, tx.clock.now_ms())
    }

    /// The latest evidence of every step of one orchestration ticket, or of the whole
    /// owning checkout, in definition order.
    pub fn validation_list(
        &self,
        owner_id: &str,
        ticket: Option<&OrchestrationTicket>,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<ValidationEvidence>, OrchestrationError> {
        let ticket = ticket
            .map(|ticket| self.validation_ticket(owner_id, ticket))
            .transpose()?;
        self.validations()
            .steps(owner_id, ticket.as_ref(), tx.clock.now_ms())
    }

    fn validation_view(
        &self,
        owner_id: &str,
        ticket: &RunTicket,
        step_id: &str,
        result: ValidationResult,
        unverified: Vec<&'static str>,
        tx: &mut Transaction<'_>,
    ) -> Result<ValidationRunView, OrchestrationError> {
        let evidence = self
            .validations()
            .evidence(owner_id, ticket, step_id, tx.clock.now_ms())?;
        let attempts_left = if may_retry(&evidence.step, &result) {
            evidence.step.definition.retry.max_attempts - result.attempt
        } else {
            0
        };
        Ok(ValidationRunView {
            result,
            attempts_left,
            evidence,
            unverified,
        })
    }

    /// Tell the orchestration ticket and, when the validation belongs to a run, that
    /// run's event stream. Identifiers and codes only; best effort.
    pub(super) fn validation_report(
        &self,
        owner_id: &str,
        ticket: &RunTicket,
        step_id: &str,
        target_id: &str,
        result: &ValidationResult,
        tx: &mut Transaction<'_>,
    ) {
        if let Some(run_id) = &result.run_id {
            self.run_event(
                owner_id,
                RunEvent {
                    at_ms: tx.clock.now_ms(),
                    run_id: Some(run_id.clone()),
                    step_id: None,
                    kind: RunEventKind::Validation,
                    source: if result.state == ValidationState::Running {
                        RunEventSource::Orchestrator
                    } else {
                        RunEventSource::Target
                    },
                    state: None,
                    reason: Some(result.state.code().into()),
                    actor_id: result.actor_id.clone(),
                    dedupe: Some(format!("{}.{}", result.result_id, result.state.code())),
                    gate_id: None,
                    gate_state: None,
                },
            );
        }
        // HS2-42TARA: evidence outside any run is published nowhere else.
        if result.run_id.is_none() {
            self.readiness_reevaluate(owner_id, tx.clock.now_ms());
        }
        if !result.state.is_final() {
            return;
        }
        let ticket = OrchestrationTicket {
            connection_id: ticket.connection_id.clone(),
            native_id: ticket.native_id.clone(),
        };
        if let Ok((store, id, _)) = self.orchestration_store(owner_id, &ticket) {
            let _ = ops::add_note(
                &store,
                &id,
                hotsheet_ticketing::mint_ulid(tx.clock, tx.rng),
                timestamp(tx),
                NoteKind::Activity,
                format!(
                    "Validation step `{step_id}` attempt {} {}: result `{}`, target `{target_id}`.",
                    result.attempt,
                    result.state.code(),
                    result.result_id,
                ),
            );
        }
    }

    /// Run a step in its target, or continue the request an operation id names.
    ///
    /// One call takes at most one attempt and waits at most `wait_ms` for it. While
    /// the answer is `running`, or `failed` with `attempts_left`, the same request
    /// again waits further or takes the next attempt. A finished request is answered
    /// from the record without contacting the target.
    pub fn validation_run(
        &self,
        owner_id: &str,
        request: RunValidation,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ValidationRunView, OrchestrationError> {
        identifier(&request.operation_id, "bounded operation id required")?;
        let wait_ms = wait(request.wait_ms.unwrap_or(0))?;
        let ticket = self.validation_ticket(owner_id, &request.ticket)?;
        let registry = self.validations();
        let evidence = registry.evidence(owner_id, &ticket, &request.step_id, tx.clock.now_ms())?;
        let step = &evidence.step;
        let definition = &step.definition;
        if let Some(run_id) = &request.run_id {
            self.run(owner_id, run_id)?;
        }
        let previous = evidence
            .history
            .iter()
            .rev()
            .find(|result| result.operation_id == request.operation_id)
            .cloned();
        // A finished request is a fact: it is answered without the gate or the target.
        if let Some(previous) = &previous {
            if previous.state.is_final() && !may_retry(step, previous) {
                return self.validation_view(
                    owner_id,
                    &ticket,
                    &request.step_id,
                    previous.clone(),
                    Vec::new(),
                    tx,
                );
            }
        }
        // Another request's attempt is still running: refuse before anyone is asked.
        if evidence.state == ValidationState::Running
            && evidence
                .latest
                .as_ref()
                .is_some_and(|latest| latest.operation_id != request.operation_id)
        {
            return Err(OrchestrationError::Validation(
                hotsheet_ticketing::orchestration::ValidationError::StepRunning,
            ));
        }
        let target = Self::target_ref(owner_id, &definition.target_id)?;
        let new = |state, reason: Option<&str>| NewResult {
            operation_id: request.operation_id.clone(),
            state,
            reason: reason.map(Into::into),
            actor_id: actor.and_then(|actor| actor.id.clone()),
            run_id: request.run_id.clone(),
        };
        // A step whose prerequisites have not passed is not run, and nobody is asked
        // to approve something that will not happen.
        if previous.is_none()
            && registry
                .unmet_prerequisite(owner_id, &ticket, &request.step_id, tx.clock.now_ms())?
                .is_some()
        {
            let authorized = self.authorize(
                &target,
                actor,
                ActionRequest::new(TargetAction::ValidationRun)
                    .operation(request.operation_id.clone())
                    .continuing(),
                tx,
            )?;
            let (result, _) = registry.begin(
                owner_id,
                &ticket,
                &request.step_id,
                new(ValidationState::Skipped, Some("prerequisite_not_passed")),
                tx,
            )?;
            self.record_outcome(&target, &authorized, false, Some("skipped"), tx)?;
            self.validation_report(
                owner_id,
                &ticket,
                &request.step_id,
                &target.target_id,
                &result,
                tx,
            );
            return self.validation_view(
                owner_id,
                &ticket,
                &request.step_id,
                result,
                Vec::new(),
                tx,
            );
        }
        // A person decided once per request: continuing it does not ask again. A
        // command the orchestration project chose is outside what the target
        // configured, so no remembered rule covers it (HS2-WN0MXZ, HS2-XHQ1EH).
        let mut gate =
            ActionRequest::new(TargetAction::ValidationRun).operation(request.operation_id.clone());
        gate.continuing = previous.is_some();
        gate.ask_each_time = !definition.check.in_granted_scope();
        let authorized = self.authorize(&target, actor, gate, tx)?;
        let (result, _) = registry.begin(
            owner_id,
            &ticket,
            &request.step_id,
            new(ValidationState::Running, None),
            tx,
        )?;
        if result.state.is_final() {
            // It lapsed, or another caller finished it, since this request read it.
            return self.validation_view(
                owner_id,
                &ticket,
                &request.step_id,
                result,
                Vec::new(),
                tx,
            );
        }
        if !result.host_started {
            self.validation_report(
                owner_id,
                &ticket,
                &request.step_id,
                &target.target_id,
                &result,
                tx,
            );
        }
        let hosted = Self::destination(&authorized).and_then(|destination| {
            let reply = if result.host_started {
                self.hosted_call_at(
                    destination,
                    "/target-validations",
                    "GET",
                    &format!(
                        "?execution_id={}&wait_ms={wait_ms}",
                        encode(&result.result_id)
                    ),
                    None,
                    tx,
                )
            } else {
                let body = serde_json::to_value(HostedValidationStart {
                    execution_id: result.result_id.clone(),
                    check: definition.check.clone(),
                    timeout_seconds: definition.timeout_seconds,
                    wait_ms,
                })
                .map_err(|_| OrchestrationError::Corrupt)?;
                self.hosted_call_at(
                    destination,
                    "/target-validations",
                    "POST",
                    "",
                    Some(&body),
                    tx,
                )
            }?;
            serde_json::from_value::<HostedValidation>(reply)
                .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        });
        let finish = |update: ResultUpdate, tx: &mut Transaction<'_>| {
            registry.finish(
                owner_id,
                &ticket,
                &request.step_id,
                &result.result_id,
                update,
                tx,
            )
        };
        let ended = |state, reason: Option<&str>, outcome| ResultUpdate {
            to: Some(state),
            reason: reason.map(Into::into),
            outcome,
            host_started: None,
        };
        let mut unverified = Vec::new();
        let result = match hosted {
            Ok(HostedValidation {
                outcome: Some(outcome),
                ..
            }) => {
                let (state, reason) = evaluate(&definition.evidence, &outcome);
                finish(ended(state, reason, Some(outcome)), tx)?
            }
            Ok(HostedValidation {
                state: HostedExecutionState::Unknown,
                ..
            }) => {
                // The host accepted it earlier and no longer knows it: whatever
                // happened cannot be confirmed, and it is not run a second time.
                finish(
                    ended(ValidationState::Unverified, Some("execution_lost"), None),
                    tx,
                )?
            }
            Ok(_) => finish(
                ResultUpdate {
                    host_started: Some(true),
                    ..ResultUpdate::default()
                },
                tx,
            )?,
            // The target may or may not have heard the request. The attempt stays
            // `running`; the same request again asks under the same execution id.
            Err(OrchestrationError::Observation(_) | OrchestrationError::Storage(_)) => {
                unverified.push("target_execution");
                result
            }
            // An accepted execution is not written off because one later read was
            // refused; it stays `running` until the host answers or it lapses.
            Err(_) if result.host_started => {
                unverified.push("target_execution");
                result
            }
            // The target refused to run it at all.
            Err(error) => finish(
                ended(ValidationState::Unavailable, Some(error.code()), None),
                tx,
            )?,
        };
        if result.state.is_final() {
            self.record_outcome(
                &target,
                &authorized,
                result.state == ValidationState::Passed,
                (result.state != ValidationState::Passed).then(|| result.state.code()),
                tx,
            )?;
            self.validation_report(
                owner_id,
                &ticket,
                &request.step_id,
                &target.target_id,
                &result,
                tx,
            );
        }
        self.validation_view(owner_id, &ticket, &request.step_id, result, unverified, tx)
    }
}
