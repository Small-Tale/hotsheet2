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
//! The registry stays the one never-run-twice record. HS2-BWAPSX: each runner is also
//! shown among the machine's Jobs (docs/33) as an owned Job of kind `validation-step`
//! under the hosted checkout, keyed by its execution id, which mirrors its state: running
//! under a renewed lease while the step runs, then succeeded, failed or cancelled with
//! codes and numbers only. A runner that dies stops renewing, so its Job reads stalled
//! and then lost, as any owned Job whose producer is gone.
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

use super::step_process::process_started;
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
    /// HS2-G3VAK4: a checkout made for this execution, removed when it ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<Worktree>,
    /// HS2-BWAPSX: the title the runner's Job shows; ids and plain words only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// HS2-BWAPSX: the Job kind of a detached runner.
pub const RUNNER_JOB_KIND: &str = "validation-step";

/// The machine's Job that mirrors one runner while it runs.
struct RunnerJob {
    store: hotsheet_jobs::JobStore,
    checkout_id: String,
    job_id: String,
    token: hotsheet_jobs::LeaseToken,
    stop: std::sync::mpsc::Sender<()>,
    heartbeat: Option<std::thread::JoinHandle<()>>,
}

impl RunnerJob {
    /// Show the runner among the machine's Jobs. Best effort: a Job store that cannot
    /// be written never holds the step back.
    fn publish(home: &Path, spec: &StepSpec) -> Option<Self> {
        use hotsheet_jobs::{
            Acquire, ActiveStatus, Capabilities, ExternalRef, JobKind, JobStore, NewJob,
            ProducerFamily,
        };
        let store = JobStore::at(home);
        let external = ExternalRef {
            system: RUNNER_JOB_KIND.into(),
            id: spec.execution_id.clone(),
        };
        let (job, _) = store
            .find_or_create(
                NewJob {
                    checkout_id: spec.checkout_id.clone(),
                    kind: JobKind {
                        family: ProducerFamily::Owned,
                        name: RUNNER_JOB_KIND.into(),
                    },
                    job_key: format!("{RUNNER_JOB_KIND}:{}", spec.checkout_id),
                    title: spec
                        .title
                        .clone()
                        .unwrap_or_else(|| "Validation step".into()),
                    capabilities: Capabilities::default(),
                    ticket: None,
                    external: Some(external.clone()),
                    retry_of: None,
                    url: None,
                },
                |job| job.external.as_ref() == Some(&external),
            )
            .ok()?;
        let (_, token) = store
            .acquire(
                &spec.checkout_id,
                &job.id,
                Acquire {
                    owner: format!("runner-{}", std::process::id()),
                    status: ActiveStatus::Running,
                    launch_id: None,
                    external: Some(external),
                },
            )
            .ok()?;
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        // The lease is renewed at a third of its length while the step runs: a local
        // file write, like the watch monitor's, never a request. Ending the step ends
        // the wait at once.
        let heartbeat = {
            let (store, checkout_id, job_id, token) = (
                store.clone(),
                spec.checkout_id.clone(),
                job.id.clone(),
                token.clone(),
            );
            let every = Duration::from_millis((store.config().lease_ms / 3).max(100));
            std::thread::spawn(move || {
                while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                    stopped.recv_timeout(every)
                {
                    let _ = store.renew(&checkout_id, &job_id, &token);
                }
            })
        };
        Some(Self {
            store,
            checkout_id: spec.checkout_id.clone(),
            job_id: job.id,
            token,
            stop,
            heartbeat: Some(heartbeat),
        })
    }

    /// Record how the step ended, by codes and numbers.
    fn finish(mut self, outcome: &ExecutionOutcome) {
        let _ = self.stop.send(());
        if let Some(heartbeat) = self.heartbeat.take() {
            let _ = heartbeat.join();
        }
        let end = serde_json::to_value(outcome.end)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let mut refs = std::collections::BTreeMap::from([
            ("end".to_owned(), end.clone()),
            ("duration_ms".to_owned(), outcome.duration_ms.to_string()),
        ]);
        if let Some(code) = outcome.exit_code {
            refs.insert("exit_code".to_owned(), code.to_string());
        }
        let result = hotsheet_jobs::Outcome {
            summary: Some(match outcome.exit_code {
                Some(code) => format!("ended {end} with exit code {code}"),
                None => format!("ended {end}"),
            }),
            refs,
        };
        let (checkout, job) = (&self.checkout_id, &self.job_id);
        let _ = match outcome.end {
            ExecutionEnd::Cancelled => {
                self.store
                    .confirm_cancel(checkout, job, &self.token, result)
            }
            ExecutionEnd::Exited if outcome.exit_code == Some(0) => self.store.finish(
                checkout,
                job,
                &self.token,
                hotsheet_jobs::FinalStatus::Succeeded,
                result,
            ),
            _ => self.store.finish(
                checkout,
                job,
                &self.token,
                hotsheet_jobs::FinalStatus::Failed,
                result,
            ),
        };
    }
}

/// HS2-G3VAK4: a detached Git worktree of one commit, made for one execution so it runs
/// exactly that commit's tracked files and never a working tree's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worktree {
    /// The repository it is made from.
    pub repository: PathBuf,
    /// Where it is made; private to this machine's home.
    pub path: PathBuf,
    /// The full commit it holds.
    pub commit: String,
}

impl Worktree {
    fn git(&self, args: &[&std::ffi::OsStr]) -> std::io::Result<()> {
        let status = hotsheet_ticketing::git::command_in(&self.repository)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other("git worktree"))
        }
    }

    /// Make it. A leftover of an earlier attempt of the same execution is replaced.
    pub fn create(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            private_dir(parent)?;
        }
        self.remove();
        self.git(&[
            "worktree".as_ref(),
            "add".as_ref(),
            "--detach".as_ref(),
            "--force".as_ref(),
            self.path.as_os_str(),
            self.commit.as_ref(),
        ])
    }

    /// Remove it, best effort, and forget its administrative record.
    pub fn remove(&self) {
        let _ = self.git(&[
            "worktree".as_ref(),
            "remove".as_ref(),
            "--force".as_ref(),
            self.path.as_os_str(),
        ]);
        let _ = fs::remove_dir_all(&self.path);
        let _ = self.git(&["worktree".as_ref(), "prune".as_ref()]);
    }
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
    // HS2-BWAPSX: shown among the machine's Jobs while it runs.
    let job = RunnerJob::publish(home, spec);
    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let marker = file(home, &spec.checkout_id, &spec.execution_id, "cancel");
    let watcher = {
        let (cancel, done, marker) = (cancel.clone(), done.clone(), marker.clone());
        let directory = steps_dir(home, &spec.checkout_id);
        std::thread::spawn(move || {
            // HS2-WD475E: the marker is checked before and between waits, and the
            // watcher starts in the background. Started first and in the way, a start
            // that took seconds on a loaded machine kept a cancel unseen that long; a
            // watcher that could not start ended this thread, so a cancel was never seen.
            let waiter = hotsheet_jobs::ChangeWaiter::starting(&[directory.as_path()]).ok();
            while !done.load(Ordering::SeqCst) {
                if marker.exists() {
                    cancel.store(true, Ordering::SeqCst);
                    return;
                }
                match &waiter {
                    Some(waiter) => {
                        waiter.wait(Duration::from_secs(1));
                    }
                    // A local file check each second; no request is made.
                    None => std::thread::sleep(Duration::from_secs(1)),
                }
            }
        })
    };
    let record = |pid| {
        super::step_process::record_step(
            registry.clone(),
            spec.checkout_id.clone(),
            spec.execution_id.clone(),
            pid,
        );
    };
    let outcome = if marker.exists() {
        ExecutionOutcome {
            end: ExecutionEnd::Cancelled,
            exit_code: None,
            signal: None,
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
    if let Some(worktree) = &spec.worktree {
        worktree.remove();
    }
    if let Some(job) = job {
        job.finish(&outcome);
    }
    let _ = registry.finish(
        &spec.checkout_id,
        &spec.execution_id,
        spec.started_at_ms + outcome.duration_ms,
        outcome,
    );
    let _ = watcher.join();
}
