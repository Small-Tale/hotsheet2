//! HS2-G3VAK4: the `target-script` deployment adapter. An authorized deployment gate
//! runs the target's own checked-in deploy script on the machine that hosts the target.
//!
//! The hosted half (`POST /checkouts/{checkout}/target-deployments`) checks that the
//! script is tracked as an executable file at the gate's commit, checks that commit
//! out into a private detached worktree, and runs `<script> <environment>` there as a
//! hosted execution: the step environment of HS2-W02PTA (an allowlist, never the
//! orchestrating machine's credentials), its own process group (HS2-HTS89S), a detached
//! runner where available (HS2-M7EV18), recorded once under the gate's id so a repeat
//! never runs it twice. No output is kept. The worktree is removed when it ends.
//! Reading it (`GET ...?execution_id=&wait_ms=`) is the hosted execution read.
//!
//! The orchestrating half starts it after `gate_deploy` passed the permission gate
//! (`deployments_run`: a remembered rule, an Allow Once or a person; a production
//! deployment asks every time), and gate sync and the follower read it like a hosted
//! CI gate. Evidence is ids, the exit code and timings only.
use std::path::{Path, PathBuf};
use std::process::Stdio;

use hotsheet_ticketing::orchestration::{
    Coverage, ExecutionEnd, Gate, GateError, GateEvidence, GateReading, GateState,
    TargetDestination, valid_commit, valid_script,
};
use serde::{Deserialize, Serialize};

use super::gates::TARGET_SCRIPT_PROVIDER;
use super::tickets::{encode, identifier};
use super::validation::{HostedCommand, HostedExecutionState, HostedValidation};
use super::validation_runner::Worktree;
use super::*;

/// The directory under a machine home that holds deployment worktrees.
pub const DEPLOYMENT_WORKTREES_DIRECTORY: &str = "orchestration-deployments";
/// The longest one deployment runs before it is stopped.
pub const MAX_DEPLOYMENT_SECONDS: u32 = 3600;

/// Run one deployment on the machine that hosts the target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedDeploymentStart {
    /// The deployment gate's id: the same id never runs twice on this machine.
    pub execution_id: String,
    /// The gate's commit, all 40 hex characters.
    pub commit: String,
    /// The target's checked-in deploy script, relative to its repository root.
    pub script: String,
    /// Passed to the script as its only argument.
    pub environment: String,
    /// How long to wait for it to finish before answering `running`.
    #[serde(default)]
    pub wait_ms: u64,
}

/// Whether `script` is tracked as an executable file at `commit` in `repository`.
fn tracked_executable(repository: &Path, commit: &str, script: &str) -> bool {
    let Ok(output) = hotsheet_ticketing::git::command_in(repository)
        .args(["ls-tree", "-z", commit, "--", script])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    // `<mode> blob <object>\t<path>\0`, exactly one entry for exactly this path.
    let entries: Vec<&[u8]> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .collect();
    match entries.as_slice() {
        [entry] => {
            let text = String::from_utf8_lossy(entry);
            let Some((meta, path)) = text.split_once('\t') else {
                return false;
            };
            path == script && meta.starts_with("100755 blob ")
        }
        _ => false,
    }
}

/// The full commit `commit` names in `repository`, when it is a commit there.
fn resolved_commit(repository: &Path, commit: &str) -> Option<String> {
    let output = hotsheet_ticketing::git::command_in(repository)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{commit}^{{commit}}"),
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|full| full == commit)
}

/// What a deployment execution's answer says about its gate.
pub(super) fn deployment_reading(
    gate: &Gate,
    reply: &HostedValidation,
    now_ms: u64,
) -> GateReading {
    match (reply.state, &reply.outcome) {
        (HostedExecutionState::Finished, Some(outcome)) => {
            let evidence = GateEvidence {
                provider: TARGET_SCRIPT_PROVIDER.into(),
                reference: reply.execution_id.clone(),
                order: outcome.started_at_ms,
                attempt: 1,
                // The worktree held exactly the gate's commit.
                commit: gate.commit.clone(),
                coverage: Coverage::Exact,
                artifact: gate
                    .deployment
                    .as_ref()
                    .and_then(|scope| scope.artifact.clone()),
                exit_code: outcome.exit_code,
                duration_ms: Some(outcome.duration_ms),
                at_ms: now_ms,
            };
            let passed = outcome.end == ExecutionEnd::Exited && outcome.exit_code == Some(0);
            if passed {
                GateReading::new(GateState::Passed).evidence(evidence)
            } else {
                let reason = serde_json::to_value(outcome.end)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "failed".into());
                GateReading::new(GateState::Failed)
                    .reason(&format!("deployment_{reason}"))
                    .evidence(evidence)
            }
        }
        (HostedExecutionState::Running, _) => GateReading::new(GateState::Pending),
        _ => GateReading::new(GateState::Unknown).reason("execution_lost"),
    }
}

impl OrchestrationRuntime {
    // ---- Hosted half ----

    /// Start, or answer, one deployment in a checkout this machine hosts.
    pub fn hosted_deployment_start(
        &self,
        checkout_id: &str,
        request: HostedDeploymentStart,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedValidation, OrchestrationError> {
        super::validation::execution_id(&request.execution_id)?;
        let commit = request.commit.to_ascii_lowercase();
        valid_commit(&commit)?;
        valid_script(&request.script)?;
        identifier(&request.environment, "bounded environment name required")?;
        let wait_ms = super::validation::wait(request.wait_ms)?;
        let checkout = self.owner(checkout_id)?;
        let repository = PathBuf::from(&checkout.root);
        let path = self
            .home
            .join(DEPLOYMENT_WORKTREES_DIRECTORY)
            .join(&checkout.id)
            .join(&request.execution_id);
        let resolve = || -> Result<Option<HostedCommand>, OrchestrationError> {
            // Only the target's own tracked script at the gate's commit ever runs.
            if resolved_commit(&repository, &commit).is_none()
                || !tracked_executable(&repository, &commit, &request.script)
            {
                return Err(OrchestrationError::Gate(
                    GateError::DeploymentScriptUntracked,
                ));
            }
            Ok(Some(HostedCommand {
                program: path.join(&request.script).to_string_lossy().into_owned(),
                args: vec![request.environment.clone()],
                timeout_seconds: MAX_DEPLOYMENT_SECONDS,
                root: path.clone(),
                // A deployment's output may name secrets: only digests are kept.
                retain: None,
                worktree: Some(Worktree {
                    repository: repository.clone(),
                    path: path.clone(),
                    commit: commit.clone(),
                }),
            }))
        };
        self.start_hosted(
            &checkout.id,
            &request.execution_id,
            resolve,
            tx.clock.now_ms(),
            wait_ms,
        )
    }

    // ---- Orchestrating half ----

    /// Start (`start`) or read one deployment gate's execution on the machine that
    /// hosts its target, and record what it says.
    pub(super) fn deployment_refresh_hosted(
        &self,
        gate: Gate,
        destination: &TargetDestination,
        start: bool,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> Result<(Gate, bool), OrchestrationError> {
        let scope = gate
            .deployment
            .as_ref()
            .ok_or(OrchestrationError::Gate(GateError::ProviderUnsupported))?;
        let wait_ms = wait_ms.min(super::validation::MAX_VALIDATION_WAIT_MS);
        let reply = if start {
            let body = serde_json::to_value(HostedDeploymentStart {
                execution_id: gate.gate_id.clone(),
                commit: gate.commit.clone(),
                script: scope.script.clone(),
                environment: scope.environment.clone(),
                wait_ms,
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            self.hosted_call_at(
                destination,
                "/target-deployments",
                "POST",
                "",
                Some(&body),
                tx,
            )
        } else {
            self.hosted_call_at(
                destination,
                "/target-deployments",
                "GET",
                &format!("?execution_id={}&wait_ms={wait_ms}", encode(&gate.gate_id)),
                None,
                tx,
            )
        };
        let reading = match reply {
            Ok(value) => {
                let reply: HostedValidation = serde_json::from_value(value)
                    .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
                deployment_reading(&gate, &reply, tx.clock.now_ms())
            }
            // The host refused to run it. Nothing ran, so there is no verdict: the gate
            // is unknown with the host's code, and nothing follows it again.
            Err(OrchestrationError::Gate(GateError::DeploymentScriptUntracked)) => {
                GateReading::new(GateState::Unknown)
                    .reason(GateError::DeploymentScriptUntracked.code())
            }
            Err(error) => return Err(error),
        };
        self.gate_record(gate, reading, tx)
    }
}
