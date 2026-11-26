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
use std::ffi::OsString;
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
    AuditOutcome, AuditRecord, ExecutionEnd, ExecutionOutcome, HostedValidationKnown,
    HostedValidationRegistry, MAX_STEP_TIMEOUT_SECONDS, NewResult, ProfileCheck, ResultUpdate,
    RunEvent, RunEventKind, RunEventSource, RunTicket, StepDefinition, StreamRef, TargetAction,
    ValidationCheck, ValidationEvidence, ValidationProfileStore, ValidationRegistry,
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

/// One command a hosted execution runs (HS2-G3VAK4: shared by validation steps and
/// deployments).
pub(super) struct HostedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub timeout_seconds: u32,
    pub root: PathBuf,
    /// Where small output is kept (HS2-ZCR3Y7); `None` keeps digests only.
    pub retain: Option<PathBuf>,
    /// HS2-G3VAK4: a checkout made for this execution, removed when it ends.
    pub worktree: Option<super::validation_runner::Worktree>,
    /// HS2-BWAPSX: what the machine's Job of a detached runner is titled.
    pub title: String,
}

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

/// HS2-17ABM2: stop the execution an id names and wait a bounded time for it to end.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedValidationCancel {
    pub execution_id: String,
    /// How long to wait for the stopped execution to end before answering `running`.
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
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
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
    /// HS2-17ABM2: set by a cancel; the step's own thread stops it.
    cancel: Arc<AtomicBool>,
    /// HS2-M7EV18: its detached runner ended without recording an end.
    lost: AtomicBool,
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
                outcome.is_none() && !self.lost.load(Ordering::SeqCst)
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.clone()
    }

    /// The reply after waiting at most `wait_ms`: `unknown` once its runner was lost.
    fn reply(&self, execution_id: &str, wait_ms: u64) -> HostedValidation {
        match self.wait(wait_ms) {
            None if self.lost.load(Ordering::SeqCst) => unknown_reply(execution_id),
            outcome => hosted_reply(execution_id, outcome),
        }
    }

    /// HS2-M7EV18: the runner is gone and recorded no end.
    fn lose(&self) {
        let _guard = self
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.lost.store(true, Ordering::SeqCst);
        self.done.notify_all();
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

/// HS2-7BTV98: a test stands in for a restart of the hosting process: every execution
/// it held in memory is gone, and only what the machine recorded remains.
#[cfg(test)]
pub(super) fn forget_executions() {
    executions().clear();
}

fn new_execution(key: String) -> Arc<Execution> {
    Arc::new(Execution {
        key,
        outcome: Mutex::new(None),
        done: Condvar::new(),
        cancel: Arc::new(AtomicBool::new(false)),
        lost: AtomicBool::new(false),
    })
}

/// Make room for one more execution by forgetting the oldest finished one; a running
/// one is never forgotten.
fn make_room(known: &mut Vec<Arc<Execution>>) -> Result<(), OrchestrationError> {
    while known.len() >= MAX_HOSTED_EXECUTIONS {
        let Some(index) = known
            .iter()
            .position(|execution| execution.finished().is_some())
        else {
            return Err(OrchestrationError::Capacity);
        };
        known.remove(index);
    }
    Ok(())
}

fn ended(end: ExecutionEnd, started_at_ms: u64, started: Instant) -> ExecutionOutcome {
    ExecutionOutcome {
        end,
        exit_code: None,
        signal: None,
        started_at_ms,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        stdout: None,
        stderr: None,
    }
}

/// HS2-H1EK8R: an outcome as [`execute_reporting`] starts it, for the settle tests.
#[cfg(all(test, unix))]
pub(super) fn ended_for_test(end: ExecutionEnd) -> ExecutionOutcome {
    ended(end, 0, Instant::now())
}

/// Read a stream to its end, keeping only its digest and size. Past `limit` it stops
/// reading and says so.
fn digest(
    mut stream: impl Read,
    limit: u64,
    over: &AtomicBool,
    mut retain: Option<super::validation_output::Retaining>,
) -> Option<StreamRef> {
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    let finish = |retain: Option<super::validation_output::Retaining>, digest: Option<&str>| {
        if let Some(retain) = retain {
            retain.finish(digest, std::time::SystemTime::now());
        }
    };
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                bytes += read as u64;
                if bytes > limit {
                    over.store(true, Ordering::SeqCst);
                    finish(retain, None);
                    return None;
                }
                hash.update(&buffer[..read]);
                // HS2-ZCR3Y7: a small stream is kept whole on this machine.
                if let Some(retain) = &mut retain {
                    retain.write(&buffer[..read]);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => {
                finish(retain, None);
                return None;
            }
        }
    }
    let sha256: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    finish(retain, Some(&sha256));
    Some(StreamRef { sha256, bytes })
}

/// Stop a step and everything it started. On Windows its job holds the whole tree
/// (HS2-2R32SD); without one only the step's own process is stopped.
fn stop(child: &mut std::process::Child, job: Option<&StepJobHandle>) {
    #[cfg(windows)]
    if let Some(job) = job {
        job.terminate();
    }
    #[cfg(not(windows))]
    let _ = job;
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

/// HS2-W02PTA: the host variables a step sees, by exact name. Enough for a build to
/// find its tools, its home and its temporary directory and to behave predictably;
/// nothing on it names a credential. Every other variable the hosting process holds
/// (`HOTSHEET_*`, provider tokens, cloud credentials, bearer secrets) is withheld.
const STEP_ENVIRONMENT: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "TZ",
    // Windows needs these to start most programs at all.
    "SystemRoot",
    "windir",
    "ComSpec",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
];

/// HS2-W02PTA: the variable a step reads to name its own execution.
pub const STEP_EXECUTION_VARIABLE: &str = "HOTSHEET_VALIDATION_EXECUTION_ID";

/// HS2-W02PTA: the environment of one step, from the hosting process's own: the
/// allowlisted variables (and the locale's `LC_*`), then the step's identifiers. A
/// profile declares no environment of its own; its checked-in program sets what it
/// needs. Names compare exactly, except on Windows where they are case-insensitive.
pub fn validation_environment(
    host: impl IntoIterator<Item = (OsString, OsString)>,
    execution_id: &str,
) -> Vec<(OsString, OsString)> {
    let allowed = |name: &str| {
        name.starts_with("LC_")
            || STEP_ENVIRONMENT.iter().any(|allowed| {
                if cfg!(windows) {
                    allowed.eq_ignore_ascii_case(name)
                } else {
                    *allowed == name
                }
            })
    };
    let mut environment: Vec<(OsString, OsString)> = host
        .into_iter()
        .filter(|(name, _)| name.to_str().is_some_and(allowed))
        .collect();
    environment.sort();
    environment.push((STEP_EXECUTION_VARIABLE.into(), execution_id.into()));
    environment.push(("CI".into(), "true".into()));
    environment
}

/// Run one command in `root` and report how it ended. Blocks until it has. Setting
/// `cancel` stops it; a step that already ended by itself keeps its own end. The step
/// sees only [`validation_environment`] (HS2-W02PTA).
#[cfg(test)]
#[allow(clippy::too_many_arguments)] // HS2-W02PTA: one step's whole launch; grouped by the caller.
pub(super) fn execute(
    root: &Path,
    program: &str,
    args: &[String],
    timeout: Duration,
    output_limit: u64,
    started_at_ms: u64,
    cancel: &AtomicBool,
    execution_id: &str,
    retain: Option<&Path>,
) -> ExecutionOutcome {
    execute_reporting(
        root,
        program,
        args,
        timeout,
        output_limit,
        started_at_ms,
        cancel,
        execution_id,
        retain,
        &|_| {},
    )
}

/// [`execute`], telling `launched` the step's pid (its process group) once it runs
/// (HS2-HTS89S).
#[allow(clippy::too_many_arguments)] // HS2-HTS89S: one step's whole launch; grouped by the caller.
pub(super) fn execute_reporting(
    root: &Path,
    program: &str,
    args: &[String],
    timeout: Duration,
    output_limit: u64,
    started_at_ms: u64,
    cancel: &AtomicBool,
    execution_id: &str,
    retain: Option<&Path>,
    launched: &dyn Fn(u32),
) -> ExecutionOutcome {
    let started = Instant::now();
    let environment = validation_environment(std::env::vars_os(), execution_id);
    // A relative program with a directory part is the checkout's own script; a bare
    // name is looked up on the step's own PATH. HS2-H1EK8R: resolved here so the step
    // starts with posix_spawn, never by forking this multi-threaded host. HS2-2R32SD:
    // Windows resolves by its own rules, PATHEXT included.
    let Some(resolved) = step_program(program, root, &environment) else {
        return ended(ExecutionEnd::ProgramNotFound, started_at_ms, started);
    };
    let mut command = hotsheet_ticketing::git::launch(&resolved);
    command
        .env_clear()
        .envs(environment)
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
    // HS2-2R32SD: on Windows the step starts suspended in its own kill-on-close job,
    // and runs once it is in the job, so nothing it starts escapes a timeout.
    #[cfg(windows)]
    let job = super::step_job::StepJob::new();
    #[cfg(windows)]
    if job.is_some() {
        use std::os::windows::process::CommandExt;
        command.creation_flags(super::step_job::StepJob::creation_flags());
    }
    #[cfg(not(windows))]
    let job: Option<StepJobHandle> = None;
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ended(ExecutionEnd::ProgramNotFound, started_at_ms, started);
        }
        Err(_) => return ended(ExecutionEnd::LaunchFailed, started_at_ms, started),
    };
    #[cfg(windows)]
    let job = job.and_then(|job| {
        let held = job.assign(&child);
        if !super::step_job::resume(child.id()) {
            // A step that cannot be resumed would only wait for its timeout.
            job.terminate();
            let _ = child.kill();
            let _ = child.wait();
            return Some(Err(()));
        }
        held.then_some(Ok(job))
    });
    #[cfg(windows)]
    let job = match job {
        Some(Err(())) => return ended(ExecutionEnd::LaunchFailed, started_at_ms, started),
        Some(Ok(job)) => Some(job),
        None => None,
    };
    launched(child.id());
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
        let retaining = retain.and_then(super::validation_output::Retaining::start);
        readers += 1;
        std::thread::spawn(move || {
            let _ = sender.send((index, digest(stream, output_limit, &over, retaining)));
        });
    }
    drop(sender);
    let mut end = ExecutionEnd::Exited;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            // HS2-H1EK8R: a signal delivered to this process during the wait interrupts
            // it; the child is still there to be asked again.
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break None,
        }
        if cancel.load(Ordering::SeqCst) {
            end = ExecutionEnd::Cancelled;
        } else if over.load(Ordering::SeqCst) {
            end = ExecutionEnd::OutputLimit;
        } else if started.elapsed() >= timeout {
            end = ExecutionEnd::TimedOut;
        } else {
            // A local wait on this process's own child; no request is made.
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        stop(&mut child, job.as_ref());
        break child.wait().ok();
    };
    // HS2-2R32SD: the step is over; on Windows whatever it left running ends with it,
    // so nothing holds its output streams open.
    #[cfg(windows)]
    if let Some(job) = &job {
        job.terminate();
    }
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
        settle(&mut outcome, status);
    }
    outcome
}

/// The job that holds a step's process tree: a Windows job object, nothing elsewhere.
#[cfg(windows)]
type StepJobHandle = super::step_job::StepJob;
#[cfg(not(windows))]
type StepJobHandle = ();

/// HS2-2R32SD: the file a step's program names, from the step's own environment.
fn step_program(
    program: &str,
    root: &Path,
    environment: &[(OsString, OsString)],
) -> Option<std::path::PathBuf> {
    if cfg!(windows) {
        use super::step_program::{variable, windows_step_program};
        windows_step_program(
            program,
            root,
            variable(environment, "PATH"),
            variable(environment, "PATHEXT"),
            &|path| path.is_file(),
        )
    } else {
        let search = hotsheet_ticketing::git::child_path(environment.iter().map(|(k, v)| (k, v)));
        hotsheet_ticketing::git::program_path(program, search.as_deref(), Some(root))
    }
}

/// HS2-H1EK8R: how a step that ended by itself ended, from the status the operating
/// system reported. Only a status without an exit code is `signalled`, with the signal
/// it names. A status this process could not read proves nothing either way, so the
/// step stays `exited` without a code (`exit_code_unknown`) instead of reporting a
/// signal nobody observed.
pub(super) fn settle(outcome: &mut ExecutionOutcome, status: Option<std::process::ExitStatus>) {
    let Some(status) = status else {
        return;
    };
    if let Some(code) = status.code() {
        outcome.exit_code = Some(code);
        return;
    }
    outcome.end = ExecutionEnd::Signalled;
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        outcome.signal = status.signal();
    }
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

fn unknown_reply(execution_id: &str) -> HostedValidation {
    HostedValidation {
        execution_id: execution_id.into(),
        state: HostedExecutionState::Unknown,
        outcome: None,
    }
}

pub(super) fn wait(wait_ms: u64) -> Result<u64, OrchestrationError> {
    if wait_ms > MAX_VALIDATION_WAIT_MS {
        return Err(OrchestrationError::Invalid("wait_ms must be at most 8000"));
    }
    Ok(wait_ms)
}

pub(super) fn execution_id(value: &str) -> Result<(), OrchestrationError> {
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
    /// HS2-M7EV18: start a step's detached runner and follow it from this process: a
    /// collector thread waits (on file changes) for the runner's recorded end, or
    /// marks the execution lost when the runner is gone without one, and passes a
    /// cancel on to it.
    fn launch_detached(
        &self,
        checkout_id: &str,
        spec: super::validation_runner::StepSpec,
        execution: &Arc<Execution>,
    ) {
        let registry = self.hosted_validation_registry();
        let execution_id = spec.execution_id.clone();
        let exited = match super::validation_runner::launch(&self.home, &spec) {
            Ok(launched) => {
                if let Some(runner) = launched.identity {
                    let _ = registry.launching(checkout_id, &execution_id, runner);
                }
                launched.exited
            }
            Err(_) => {
                let outcome = ended(
                    ExecutionEnd::LaunchFailed,
                    spec.started_at_ms,
                    Instant::now(),
                );
                let _ = registry.finish(
                    checkout_id,
                    &execution_id,
                    spec.started_at_ms,
                    outcome.clone(),
                );
                execution.finish(outcome);
                return;
            }
        };
        let (home, checkout_id, running) =
            (self.home.clone(), checkout_id.to_owned(), execution.clone());
        std::thread::spawn(move || {
            let directory = super::validation_runner::steps_dir(&home, &checkout_id);
            // HS2-WD475E: started in the background; until it runs, each wait ends at
            // the one-second re-check below, so the record is still read promptly.
            let waiter = hotsheet_jobs::ChangeWaiter::starting(&[directory.as_path()]).ok();
            let registry = HostedValidationRegistry::new(&home);
            let mut asked_to_stop = false;
            loop {
                if !asked_to_stop && running.cancel.load(Ordering::SeqCst) {
                    super::validation_runner::request_cancel(&home, &checkout_id, &execution_id);
                    asked_to_stop = true;
                }
                match registry.record(&checkout_id, &execution_id) {
                    Ok(Some(record)) => {
                        if let Some(outcome) = record.outcome {
                            running.finish(outcome);
                            return;
                        }
                        // This process reaps its runner, so it knows when it ended; a
                        // runner that ended without recording an end lost the step.
                        if exited.load(Ordering::SeqCst) {
                            let ended = registry
                                .record(&checkout_id, &execution_id)
                                .ok()
                                .flatten()
                                .and_then(|record| record.outcome);
                            match ended {
                                Some(outcome) => running.finish(outcome),
                                None => running.lose(),
                            }
                            return;
                        }
                    }
                    Ok(None) | Err(_) => {
                        running.lose();
                        return;
                    }
                }
                // A local wait on this machine's own files; no request is made.
                match &waiter {
                    Some(waiter) => {
                        waiter.wait(Duration::from_secs(1));
                    }
                    None => std::thread::sleep(Duration::from_secs(1)),
                }
            }
        });
    }

    /// HS2-M7EV18: the reply for an execution this process does not hold: its recorded
    /// end, `running` while a detached runner of another process still runs it
    /// (waiting at most `wait_ms` for its end), or `unknown`.
    fn detached_reply(
        &self,
        checkout_id: &str,
        execution_id: &str,
        wait_ms: u64,
    ) -> HostedValidation {
        let directory = super::validation_runner::steps_dir(&self.home, checkout_id);
        let registry = self.hosted_validation_registry();
        let deadline = Instant::now() + Duration::from_millis(wait_ms);
        // HS2-WD475E: the watcher starts only when the record does not answer at once,
        // and in the background: a start that took seconds on a loaded machine delayed
        // even an immediate answer, long enough for a step's own timeout to pass.
        let mut waiter: Option<Option<hotsheet_jobs::ChangeWaiter>> = None;
        loop {
            match registry.record(checkout_id, execution_id) {
                Ok(Some(record)) => {
                    if let Some(outcome) = record.outcome {
                        return hosted_reply(execution_id, Some(outcome));
                    }
                    if !super::validation_runner::runner_alive(&record) {
                        return unknown_reply(execution_id);
                    }
                }
                Ok(None) | Err(_) => return unknown_reply(execution_id),
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return hosted_reply(execution_id, None);
            }
            let waiter = waiter.get_or_insert_with(|| {
                hotsheet_jobs::ChangeWaiter::starting(&[directory.as_path()]).ok()
            });
            match waiter {
                Some(waiter) => {
                    waiter.wait(left);
                }
                None => std::thread::sleep(left.min(Duration::from_secs(1))),
            }
        }
    }

    /// HS2-7BTV98: what this machine recorded about the executions it accepted.
    fn hosted_validation_registry(&self) -> HostedValidationRegistry {
        HostedValidationRegistry::new(&self.home)
    }

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
        let retain = self.validation_output_dir(&checkout.id);
        // Resolve before anything is remembered: a damaged profile file is an error,
        // never a guess. HS2-YYNW9F: a profile's own timeout shortens the step's.
        let resolve = || -> Result<Option<HostedCommand>, OrchestrationError> {
            let command = match &request.check {
                ValidationCheck::Command { program, args } => {
                    Some((program.clone(), args.clone(), request.timeout_seconds))
                }
                ValidationCheck::Profile { name } => ValidationProfileStore::new(&root)
                    .find(name)?
                    .map(|profile| {
                        let timeout = profile.effective_timeout(request.timeout_seconds);
                        (profile.program, profile.args, timeout)
                    }),
            };
            Ok(
                command.map(|(program, args, timeout_seconds)| HostedCommand {
                    program,
                    args,
                    timeout_seconds,
                    root: root.clone(),
                    retain: Some(retain.clone()),
                    worktree: None,
                    title: "Validation step".into(),
                }),
            )
        };
        self.start_hosted(
            &checkout.id,
            &request.execution_id,
            resolve,
            tx.clock.now_ms(),
            wait_ms,
        )
    }

    /// Start one hosted execution, or answer the one its id already names: in this
    /// process, in a detached runner (HS2-M7EV18), or from the machine's record
    /// (HS2-7BTV98). `resolve` names the command, or `None` when nothing is defined;
    /// it runs only for an id this process does not hold. Never runs an id twice.
    pub(super) fn start_hosted(
        &self,
        checkout_id: &str,
        execution_id: &str,
        resolve: impl FnOnce() -> Result<Option<HostedCommand>, OrchestrationError>,
        started_at_ms: u64,
        wait_ms: u64,
    ) -> Result<HostedValidation, OrchestrationError> {
        self.reap_once();
        let key = self.execution_key(checkout_id, execution_id);
        let execution = {
            let mut known = executions();
            if let Some(existing) = known.iter().find(|execution| execution.key == key) {
                existing.clone()
            } else {
                let command = resolve()?;
                make_room(&mut known)?;
                if command.is_some()
                    && known
                        .iter()
                        .filter(|execution| execution.finished().is_none())
                        .count()
                        >= MAX_RUNNING_EXECUTIONS
                {
                    return Err(OrchestrationError::Capacity);
                }
                // HS2-7BTV98: this machine may already know the id from a process that
                // is gone. A finished one is answered from its record; one that was
                // accepted is never started again.
                let registry = self.hosted_validation_registry();
                match registry.accept(checkout_id, execution_id, started_at_ms)? {
                    HostedValidationKnown::Unknown => {}
                    HostedValidationKnown::Finished(outcome) => {
                        return Ok(hosted_reply(execution_id, Some(outcome)));
                    }
                    HostedValidationKnown::Accepted => {
                        // HS2-M7EV18: a detached runner may still run it.
                        drop(known);
                        return Ok(self.detached_reply(checkout_id, execution_id, wait_ms));
                    }
                }
                let execution = new_execution(key);
                known.push(execution.clone());
                let launch_failed = |execution: &Execution| {
                    let outcome = ended(ExecutionEnd::LaunchFailed, started_at_ms, Instant::now());
                    let _ =
                        registry.finish(checkout_id, execution_id, started_at_ms, outcome.clone());
                    execution.finish(outcome);
                };
                match command {
                    None => {
                        let outcome = ended(
                            ExecutionEnd::ProfileNotDefined,
                            started_at_ms,
                            Instant::now(),
                        );
                        let _ = registry.finish(
                            checkout_id,
                            execution_id,
                            started_at_ms,
                            outcome.clone(),
                        );
                        execution.finish(outcome);
                    }
                    // HS2-G3VAK4: a checkout made for this execution must exist first.
                    Some(command)
                        if command
                            .worktree
                            .as_ref()
                            .is_some_and(|worktree| worktree.create().is_err()) =>
                    {
                        launch_failed(&execution);
                    }
                    Some(command) if self.detached_validation => {
                        self.launch_detached(
                            checkout_id,
                            super::validation_runner::StepSpec {
                                checkout_id: checkout_id.to_owned(),
                                execution_id: execution_id.to_owned(),
                                root: command.root,
                                program: command.program,
                                args: command.args,
                                timeout_seconds: command.timeout_seconds,
                                started_at_ms,
                                output_limit: MAX_VALIDATION_OUTPUT_BYTES,
                                retain: command.retain,
                                worktree: command.worktree,
                                title: Some(command.title),
                            },
                            &execution,
                        );
                    }
                    Some(command) => {
                        let running = execution.clone();
                        let step_execution = execution_id.to_owned();
                        let checkout_id = checkout_id.to_owned();
                        let timeout = Duration::from_secs(u64::from(command.timeout_seconds));
                        std::thread::spawn(move || {
                            // HS2-HTS89S: the group is recorded while it runs, so a
                            // later host can stop it if this process dies first.
                            let record = |pid| {
                                super::step_process::record_step(
                                    registry.clone(),
                                    checkout_id.clone(),
                                    step_execution.clone(),
                                    pid,
                                );
                            };
                            let outcome = execute_reporting(
                                &command.root,
                                &command.program,
                                &command.args,
                                timeout,
                                MAX_VALIDATION_OUTPUT_BYTES,
                                started_at_ms,
                                &running.cancel,
                                &step_execution,
                                command.retain.as_deref(),
                                &record,
                            );
                            if let Some(worktree) = &command.worktree {
                                worktree.remove();
                            }
                            // Recorded before it is answered, so a restart right after
                            // still finds how it ended.
                            let _ = registry.finish(
                                &checkout_id,
                                &step_execution,
                                started_at_ms,
                                outcome.clone(),
                            );
                            running.finish(outcome);
                        });
                    }
                }
                execution
            }
        };
        Ok(execution.reply(execution_id, wait_ms))
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
        self.reap_once();
        let key = self.execution_key(checkout_id, id);
        let execution = executions()
            .iter()
            .find(|execution| execution.key == key)
            .cloned();
        Ok(match execution {
            Some(execution) => execution.reply(id, wait_ms),
            // HS2-7BTV98: not in this process; what the machine recorded decides.
            // HS2-M7EV18: a detached runner of another process may still run it.
            None => self.detached_reply(checkout_id, id, wait_ms),
        })
    }

    /// HS2-YYNW9F: the test profiles a checkout this machine hosts offers, checked
    /// as a validator would. Reads the checked-in file only; runs and writes nothing.
    pub fn hosted_validation_profiles(
        &self,
        checkout_id: &str,
    ) -> Result<ProfileCheck, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        ValidationProfileStore::new(Path::new(&checkout.root)).check()
    }

    /// HS2-17ABM2: stop an execution this host process started and wait a bounded time
    /// for it to end. It only lowers activity, so it is ordinary authenticated data
    /// like the start. A step that already ended keeps its own outcome, so a cancel
    /// that races the step's exit answers with how it really ended. An id this process
    /// does not know answers `unknown` and is remembered as cancelled, so a start with
    /// that id that arrives later runs nothing. The same cancel again is a no-op.
    pub fn hosted_validation_cancel(
        &self,
        checkout_id: &str,
        request: HostedValidationCancel,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedValidation, OrchestrationError> {
        execution_id(&request.execution_id)?;
        let wait_ms = wait(request.wait_ms)?;
        self.owner(checkout_id)?;
        self.reap_once();
        let key = self.execution_key(checkout_id, &request.execution_id);
        let execution = {
            let mut known = executions();
            match known.iter().find(|execution| execution.key == key) {
                Some(execution) => execution.clone(),
                None => {
                    // HS2-M7EV18: a detached runner of another process that still runs
                    // it is asked to stop it, and records the end itself.
                    let registry = self.hosted_validation_registry();
                    if registry
                        .record(checkout_id, &request.execution_id)?
                        .is_some_and(|record| super::validation_runner::runner_alive(&record))
                    {
                        drop(known);
                        super::validation_runner::request_cancel(
                            &self.home,
                            checkout_id,
                            &request.execution_id,
                        );
                        return Ok(self.detached_reply(
                            checkout_id,
                            &request.execution_id,
                            wait_ms,
                        ));
                    }
                    // HS2-7BTV98: a finished execution of a process that is gone keeps
                    // its own end; an id never seen is remembered as cancelled, on disk
                    // too. HS2-HTS89S: one that was running in a process that is gone is
                    // stopped here, by its recorded process group.
                    self.stop_orphaned_step(checkout_id, &request.execution_id);
                    let cancelled =
                        ended(ExecutionEnd::Cancelled, tx.clock.now_ms(), Instant::now());
                    match self.hosted_validation_registry().finish(
                        checkout_id,
                        &request.execution_id,
                        tx.clock.now_ms(),
                        cancelled.clone(),
                    )? {
                        HostedValidationKnown::Finished(outcome) => {
                            return Ok(hosted_reply(&request.execution_id, Some(outcome)));
                        }
                        HostedValidationKnown::Accepted => {
                            return Ok(unknown_reply(&request.execution_id));
                        }
                        HostedValidationKnown::Unknown => {}
                    }
                    make_room(&mut known)?;
                    let tombstone = new_execution(key);
                    tombstone.finish(cancelled);
                    known.push(tombstone);
                    return Ok(unknown_reply(&request.execution_id));
                }
            }
        };
        execution.cancel.store(true, Ordering::SeqCst);
        // HS2-M7EV18: a step in a detached runner is stopped by its runner.
        if self.detached_validation {
            super::validation_runner::request_cancel(
                &self.home,
                checkout_id,
                &request.execution_id,
            );
        }
        Ok(execution.reply(&request.execution_id, wait_ms))
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
    /// HS2-ZYM6ZT: the person whose request the caller carries out (a browser's Run
    /// Validation acts as the orchestrator so that a command step still asks). It is
    /// recorded in the audit beside the actor and never changes the decision.
    #[serde(default)]
    pub requested_by: Option<String>,
}

/// HS2-YYNW9F: read the test profiles a declared target offers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetProfiles {
    pub target_id: String,
}

/// HS2-17ABM2: stop a step's running attempt.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelValidation {
    pub ticket: OrchestrationTicket,
    pub step_id: String,
    /// The request to stop; omit for whichever attempt of the step is running.
    #[serde(default)]
    pub operation_id: Option<String>,
    /// How long to wait for the stopped attempt to end, at most 8000.
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
                requested_by: None,
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
        let named_run = match &request.run_id {
            Some(run_id) => Some(self.run(owner_id, run_id)?),
            None => None,
        };
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
        // HS2-4F1SWS: a step run for a paused run is refused with `run_paused` before
        // anyone is asked or anything runs; a finished request is still answered above.
        if named_run.as_ref().is_some_and(|run| run.paused.is_some()) {
            return Err(OrchestrationError::Run(
                hotsheet_ticketing::orchestration::RunError::Paused,
            ));
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
                ActionRequest {
                    requested_by: request.requested_by.clone(),
                    ..ActionRequest::new(TargetAction::ValidationRun)
                        .operation(request.operation_id.clone())
                        .continuing()
                },
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
        gate.requested_by = request.requested_by.clone();
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
            self.validation_settle(
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
        let (result, changed) = match hosted {
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
                (result, false)
            }
            // An accepted execution is not written off because one later read was
            // refused; it stays `running` until the host answers or it lapses.
            Err(_) if result.host_started => {
                unverified.push("target_execution");
                (result, false)
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
            // A cancel that settled it first has already reported it (HS2-17ABM2).
            if changed {
                self.validation_report(
                    owner_id,
                    &ticket,
                    &request.step_id,
                    &target.target_id,
                    &result,
                    tx,
                );
            }
        }
        self.validation_view(owner_id, &ticket, &request.step_id, result, unverified, tx)
    }

    /// Apply one change to a running attempt, saying whether this call wrote it. An
    /// attempt another caller settled differently in the meantime (a cancel, or the
    /// request that saw the step's own end) is answered as recorded, unchanged.
    pub(super) fn validation_settle(
        &self,
        owner_id: &str,
        ticket: &RunTicket,
        step_id: &str,
        result_id: &str,
        update: ResultUpdate,
        tx: &mut Transaction<'_>,
    ) -> Result<(ValidationResult, bool), OrchestrationError> {
        let registry = self.validations();
        match registry.settle(owner_id, ticket, step_id, result_id, update, tx) {
            Err(OrchestrationError::Validation(
                hotsheet_ticketing::orchestration::ValidationError::ResultFinal,
            )) => registry
                .evidence(owner_id, ticket, step_id, tx.clock.now_ms())?
                .history
                .into_iter()
                .find(|result| result.result_id == result_id)
                .map(|result| (result, false))
                .ok_or(OrchestrationError::Validation(
                    hotsheet_ticketing::orchestration::ValidationError::ResultNotFound,
                )),
            other => other,
        }
    }

    /// HS2-YYNW9F: the test profiles a declared target offers, read from its
    /// checked-in file on the machine that hosts it. Reading a target never asks: it
    /// passes the gate as an observation, is audited, and runs or writes nothing.
    pub fn validation_profiles(
        &self,
        owner_id: &str,
        request: TargetProfiles,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ProfileCheck, OrchestrationError> {
        let target = Self::target_ref(owner_id, &request.target_id)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::GatesObserve),
            tx,
        )?;
        let outcome = Self::destination(&authorized).and_then(|destination| {
            serde_json::from_value(self.hosted_call_at(
                destination,
                "/target-validation-profiles",
                "GET",
                "",
                None,
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        });
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// HS2-17ABM2: stop a step's running attempt.
    ///
    /// Cancelling only lowers activity on the target, so it passes the
    /// `validation_run` gate without asking anyone and the policy never refuses it;
    /// it needs an identified actor and is audited. The attempt ends `cancelled`,
    /// distinct from `failed`, unless the step ended by itself first: then it records
    /// how the step really ended. An attempt that is already finished is answered from
    /// the record without the gate or the target. While the host has not confirmed
    /// the stop, the attempt stays `running` and the same cancel again waits further.
    pub fn validation_cancel(
        &self,
        owner_id: &str,
        request: CancelValidation,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ValidationRunView, OrchestrationError> {
        if let Some(operation) = &request.operation_id {
            identifier(operation, "bounded operation id required")?;
        }
        let wait_ms = wait(request.wait_ms.unwrap_or(0))?;
        let ticket = self.validation_ticket(owner_id, &request.ticket)?;
        let evidence =
            self.validations()
                .evidence(owner_id, &ticket, &request.step_id, tx.clock.now_ms())?;
        let found = match &request.operation_id {
            Some(operation) => evidence
                .history
                .iter()
                .rev()
                .find(|result| result.operation_id == *operation),
            None => evidence
                .history
                .iter()
                .rev()
                .find(|result| result.state == ValidationState::Running)
                .or(evidence.latest.as_ref()),
        }
        .cloned()
        .ok_or(OrchestrationError::Validation(
            hotsheet_ticketing::orchestration::ValidationError::ResultNotFound,
        ))?;
        // Nothing is running: the finished attempt is a fact and nothing is asked.
        if found.state.is_final() {
            return self.validation_view(
                owner_id,
                &ticket,
                &request.step_id,
                found,
                Vec::new(),
                tx,
            );
        }
        let definition = &evidence.step.definition;
        let target = Self::target_ref(owner_id, &definition.target_id)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::ValidationRun)
                .operation(found.operation_id.clone())
                .reducing(),
            tx,
        )?;
        // Asked even when the host never confirmed the start: a start in flight may
        // have reached it, and an id it does not know is then never started.
        let hosted = Self::destination(&authorized).and_then(|destination| {
            let body = serde_json::to_value(HostedValidationCancel {
                execution_id: found.result_id.clone(),
                wait_ms,
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            let reply = self.hosted_call_at(
                destination,
                "/target-validations",
                "DELETE",
                "",
                Some(&body),
                tx,
            )?;
            serde_json::from_value::<HostedValidation>(reply)
                .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        });
        let ended = |state, reason: &str, outcome| ResultUpdate {
            to: Some(state),
            reason: Some(reason.into()),
            outcome,
            host_started: None,
        };
        let update = match hosted {
            Ok(HostedValidation {
                outcome: Some(outcome),
                ..
            }) => {
                let (state, reason) = evaluate(&definition.evidence, &outcome);
                Some(ResultUpdate {
                    to: Some(state),
                    reason: reason.map(Into::into),
                    outcome: Some(outcome),
                    host_started: None,
                })
            }
            // Accepted earlier and no longer known there: it cannot be confirmed.
            Ok(HostedValidation {
                state: HostedExecutionState::Unknown,
                ..
            }) if found.host_started => {
                Some(ended(ValidationState::Unverified, "execution_lost", None))
            }
            // Never started there, and now it never will be.
            Ok(HostedValidation {
                state: HostedExecutionState::Unknown,
                ..
            }) => Some(ended(ValidationState::Cancelled, "cancelled", None)),
            // Stopping, or the host could not be told: it stays `running`.
            Ok(_) | Err(_) => None,
        };
        let (result, changed, unverified) = match update {
            Some(update) => {
                let (result, changed) = self.validation_settle(
                    owner_id,
                    &ticket,
                    &request.step_id,
                    &found.result_id,
                    update,
                    tx,
                )?;
                (result, changed, Vec::new())
            }
            None => (found, false, vec!["target_execution"]),
        };
        self.record_outcome(
            &target,
            &authorized,
            result.state.is_final(),
            (!result.state.is_final()).then_some("cancel_unconfirmed"),
            tx,
        )?;
        if changed {
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
