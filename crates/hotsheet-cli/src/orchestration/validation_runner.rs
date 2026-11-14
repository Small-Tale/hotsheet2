//! HS2-M7EV18: a hosted validation step runs in a detached runner process, so it
//! finishes and is collected after the hosting process restarts.
//!
//! The hosting process (the server, or a CLI host) accepts the execution in the
//! machine's hosted-validation registry (HS2-7BTV98), writes the step's launch to a
//! private spec file, and starts `hotsheet-cli orchestration step-runner` detached in
//! its own process group. The runner claims the spec once (an atomic rename, so a
//! second runner of the same id runs nothing), runs the step, and records how it ended
//! in the registry: the registry is the runner's outcome file. Any host process of the
//! machine answers from it. While the step runs, the registry names the runner and the
//! step's process group by pid and start time (HS2-HTS89S), which is how a host tells a
//! running step from one whose runner died (`execution_lost`). A cancel leaves a marker
//! the runner waits on with a file watcher. Nothing polls a provider or a peer.
//!
//! Evaluated against a `hotsheet-jobs` owned Job: the Job store has no owned-process
//! runner yet, and a Job record would duplicate the registry's never-run-twice record
//! with a second state machine. The registry plus process identity gives the same
//! guarantees with one record; showing a step among the machine's Jobs is HS2-BWAPSX.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hotsheet_ticketing::orchestration::{
    ExecutionEnd, ExecutionOutcome, HostedValidationKnown, HostedValidationRegistry, StepProcess,
};
use serde::{Deserialize, Serialize};

use super::step_process::{process_started, step_process};
use super::*;

/// The directory under a machine home that holds pending step launches.
pub const VALIDATION_STEPS_DIRECTORY: &str = "orchestration-validation-steps";

/// One step's launch, written by the host and claimed by its runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StepSpec {
    pub checkout_id: String,
    pub execution_id: String,
    pub root: PathBuf,
    pub program: String,
    pub args: Vec<String>,
    pub timeout_seconds: u32,
    pub started_at_ms: u64,
    pub output_limit: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain: Option<PathBuf>,
}

/// Where a checkout's step launches live.
pub(super) fn steps_dir(home: &Path, checkout_id: &str) -> PathBuf {
    home.join(VALIDATION_STEPS_DIRECTORY).join(checkout_id)
}

fn file(home: &Path, checkout_id: &str, execution_id: &str, suffix: &str) -> PathBuf {
    steps_dir(home, checkout_id).join(format!("{execution_id}.{suffix}"))
}

#[cfg(unix)]
fn private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn private_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(&mut options.open(path)?, bytes)
}

#[cfg(unix)]
fn detach(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn detach(_command: &mut std::process::Command) {}

/// A runner this process started.
pub(super) struct Launched {
    /// The runner's identity when it could be read at once; the runner records its own
    /// as soon as it runs.
    pub identity: Option<StepProcess>,
    /// Set once the runner process has exited and been reaped by this process.
    pub exited: Arc<AtomicBool>,
}

/// Write the spec and start its runner. The runner is reaped by a thread of this
/// process, so a runner that ends never reads as alive, and `exited` says it ended.
pub(super) fn launch(home: &Path, spec: &StepSpec) -> std::io::Result<Launched> {
    let directory = steps_dir(home, &spec.checkout_id);
    private_dir(&directory)?;
    let bytes = serde_json::to_vec(spec).map_err(std::io::Error::other)?;
    write_private(
        &file(home, &spec.checkout_id, &spec.execution_id, "json"),
        &bytes,
    )?;
    let cli = hotsheet_aitools::launch_safety::hotsheet_cli()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::NotFound, error.to_string()))?;
    let mut command = hotsheet_ticketing::git::launch(cli);
    command
        .args([
            "orchestration",
            "step-runner",
            "--checkout",
            &spec.checkout_id,
            "--execution",
            &spec.execution_id,
        ])
        .env("HOTSHEET_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    let mut child = hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(home),
        supervisor: "hotsheet-cli",
        purpose: hotsheet_process_journal::Purpose::ValidationStep,
        project_id: None,
        session_id: None,
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }
    .spawn(&mut command)?;
    let pid = child.id();
    let exited = Arc::new(AtomicBool::new(false));
    let reaped = exited.clone();
    std::thread::spawn(move || {
        let _ = child.wait();
        reaped.store(true, Ordering::SeqCst);
    });
    Ok(Launched {
        identity: super::step_process::spawned_started(pid).map(|started| StepProcess {
            pid,
            started,
            host_pid: pid,
            host_started: started,
        }),
        exited,
    })
}

/// Ask a step's runner to stop it. Best effort; the runner records the end.
pub(super) fn request_cancel(home: &Path, checkout_id: &str, execution_id: &str) {
    let path = file(home, checkout_id, execution_id, "cancel");
    if private_dir(&steps_dir(home, checkout_id)).is_ok() {
        let _ = fs::write(path, b"");
    }
}

/// Whether a runner of another process still runs this execution.
pub(super) fn runner_alive(
    record: &hotsheet_ticketing::orchestration::HostedValidationRecord,
) -> bool {
    record.outcome.is_none()
        && record.process.is_some_and(|process| {
            process.host_pid != std::process::id()
                && process_started(process.host_pid) == Some(process.host_started)
        })
}

/// The runner: claim the spec, run the step, record its end. Runs nothing when the
/// spec was already claimed or the execution is no longer waiting to run.
pub fn run_validation_step(
    home: &Path,
    checkout_id: &str,
    execution_id: &str,
) -> Result<(), OrchestrationError> {
    let spec_path = file(home, checkout_id, execution_id, "json");
    let taken = file(home, checkout_id, execution_id, "taken");
    // The claim: only one runner moves the spec.
    if fs::rename(&spec_path, &taken).is_err() {
        return Err(OrchestrationError::NotFound);
    }
    let spec: Result<StepSpec, OrchestrationError> = fs::read(&taken)
        .map_err(OrchestrationError::Storage)
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|_| OrchestrationError::Corrupt));
    let registry = HostedValidationRegistry::new(home);
    let result = spec.and_then(|spec| {
        if spec.checkout_id != checkout_id || spec.execution_id != execution_id {
            return Err(OrchestrationError::Corrupt);
        }
        if registry.known(checkout_id, execution_id)? != HostedValidationKnown::Accepted {
            return Ok(());
        }
        // Any host process can tell from now on that this runner holds the step.
        let me = std::process::id();
        if let Some(started) = process_started(me) {
            let runner = StepProcess {
                pid: me,
                started,
                host_pid: me,
                host_started: started,
            };
            let _ = registry.launched(checkout_id, execution_id, runner);
        }
        run_claimed(home, &registry, &spec);
        Ok(())
    });
    // Removing the claim is the change a waiting host wakes on.
    let _ = fs::remove_file(file(home, checkout_id, execution_id, "cancel"));
    let _ = fs::remove_file(&taken);
    result
}

fn run_claimed(home: &Path, registry: &HostedValidationRegistry, spec: &StepSpec) {
    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let marker = file(home, &spec.checkout_id, &spec.execution_id, "cancel");
    let watcher = {
        let (cancel, done, marker) = (cancel.clone(), done.clone(), marker.clone());
        let directory = steps_dir(home, &spec.checkout_id);
        std::thread::spawn(move || {
            let Ok(waiter) = hotsheet_jobs::ChangeWaiter::new(&directory) else {
                return;
            };
            while !done.load(Ordering::SeqCst) {
                if marker.exists() {
                    cancel.store(true, Ordering::SeqCst);
                    return;
                }
                waiter.wait(Duration::from_secs(1));
            }
        })
    };
    let record = |pid| {
        if let Some(process) = step_process(pid) {
            let _ = registry.launched(&spec.checkout_id, &spec.execution_id, process);
        }
    };
    let outcome = if marker.exists() {
        ExecutionOutcome {
            end: ExecutionEnd::Cancelled,
            exit_code: None,
            started_at_ms: spec.started_at_ms,
            duration_ms: 0,
            stdout: None,
            stderr: None,
        }
    } else {
        super::validation::execute_reporting(
            &spec.root,
            &spec.program,
            &spec.args,
            Duration::from_secs(u64::from(spec.timeout_seconds)),
            spec.output_limit,
            spec.started_at_ms,
            &cancel,
            &spec.execution_id,
            spec.retain.as_deref(),
            &record,
        )
    };
    done.store(true, Ordering::SeqCst);
    let _ = registry.finish(
        &spec.checkout_id,
        &spec.execution_id,
        spec.started_at_ms + outcome.duration_ms,
        outcome,
    );
    let _ = watcher.join();
}
