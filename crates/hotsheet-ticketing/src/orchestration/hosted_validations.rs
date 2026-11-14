//! HS2-7BTV98: what a hosting machine knows about the validation executions it
//! accepted, kept across a restart of the process that ran them.
//!
//! The process that runs a step keeps its live executions in memory. This registry
//! records, by checkout and execution id, that an execution was accepted (before it
//! launches), how it ended, or that it was cancelled before it ran. After a restart:
//! a finished execution is answered from its record, one that was still running when
//! its process died is `Lost` and is never started again, and a cancelled id stays
//! cancelled. Nothing in it is output: codes, numbers and digests only.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::valid_identifier;
use super::storage::{Storage, decode};
use super::*;

/// Executions one machine remembers; the oldest finished one is forgotten first.
pub const MAX_HOSTED_VALIDATION_RECORDS: usize = 256;
const MAX_HOSTED_VALIDATION_BYTES: usize = 1024 * 1024;

/// HS2-HTS89S: the process group a running step was launched as, and the hosting
/// process that launched it, each named by its pid and its start time so a reused pid
/// is never mistaken for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepProcess {
    /// The step's own pid, which is also its process group id.
    pub pid: u32,
    /// The step's start time as the operating system reports it.
    pub started: u64,
    pub host_pid: u32,
    pub host_started: u64,
}

/// One accepted execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedValidationRecord {
    pub checkout_id: String,
    pub execution_id: String,
    pub accepted_at_ms: u64,
    /// How it ended; absent while it runs (or ran in a process that is gone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ExecutionOutcome>,
    /// HS2-HTS89S: while it runs, the process group it runs as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<StepProcess>,
}

/// What the registry says about one id, for a process that does not run it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedValidationKnown {
    /// Never accepted on this machine (or forgotten).
    Unknown,
    /// Accepted and still running in this process, or run by a process that is gone:
    /// only the process that holds it in memory can tell. Never start it again.
    Accepted,
    /// It ended, or was cancelled before it ran.
    Finished(ExecutionOutcome),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostedValidationFile {
    schema_version: u32,
    executions: Vec<HostedValidationRecord>,
}

/// The machine-local registry, `orchestration-hosted-validations.json` (mode 0600).
#[derive(Clone)]
pub struct HostedValidationRegistry {
    storage: Storage,
}

impl HostedValidationRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-hosted-validations.json"),
        }
    }

    fn read_file(&self) -> Result<HostedValidationFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_HOSTED_VALIDATION_BYTES)? else {
            return Ok(HostedValidationFile {
                schema_version: ORCHESTRATION_VERSION,
                executions: Vec::new(),
            });
        };
        let file: HostedValidationFile = decode(&bytes)?;
        if file.executions.len() > MAX_HOSTED_VALIDATION_RECORDS {
            return Err(OrchestrationError::Corrupt);
        }
        for record in &file.executions {
            validate(record).map_err(|_| OrchestrationError::Corrupt)?;
        }
        Ok(file)
    }

    fn write_file(&self, file: &HostedValidationFile) -> Result<(), OrchestrationError> {
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_HOSTED_VALIDATION_BYTES)
    }

    /// What this machine recorded about one execution.
    pub fn known(
        &self,
        checkout_id: &str,
        execution_id: &str,
    ) -> Result<HostedValidationKnown, OrchestrationError> {
        valid_identifier(checkout_id)?;
        valid_identifier(execution_id)?;
        Ok(
            match self.read_file()?.executions.into_iter().find(|record| {
                record.checkout_id == checkout_id && record.execution_id == execution_id
            }) {
                None => HostedValidationKnown::Unknown,
                Some(HostedValidationRecord {
                    outcome: Some(outcome),
                    ..
                }) => HostedValidationKnown::Finished(outcome),
                Some(_) => HostedValidationKnown::Accepted,
            },
        )
    }

    /// Record an execution before it launches. Answers what was already known instead
    /// when the id is not new, so a caller never launches an id twice.
    pub fn accept(
        &self,
        checkout_id: &str,
        execution_id: &str,
        now_ms: u64,
    ) -> Result<HostedValidationKnown, OrchestrationError> {
        self.put(checkout_id, execution_id, now_ms, None)
    }

    /// Record how an execution ended, or a cancel of an id that never ran. The first
    /// outcome recorded stays: a later one for the same id changes nothing.
    pub fn finish(
        &self,
        checkout_id: &str,
        execution_id: &str,
        now_ms: u64,
        outcome: ExecutionOutcome,
    ) -> Result<HostedValidationKnown, OrchestrationError> {
        self.put(checkout_id, execution_id, now_ms, Some(outcome))
    }

    /// HS2-HTS89S: record the process group an accepted execution runs as. Changes
    /// nothing for an execution that already ended or is not known.
    pub fn launched(
        &self,
        checkout_id: &str,
        execution_id: &str,
        process: StepProcess,
    ) -> Result<(), OrchestrationError> {
        self.set_process(checkout_id, execution_id, Some(process), true)
    }

    /// HS2-M7EV18: record a process only when none is recorded yet, so a launcher's
    /// late note of the runner never replaces what the runner already recorded.
    pub fn launching(
        &self,
        checkout_id: &str,
        execution_id: &str,
        process: StepProcess,
    ) -> Result<(), OrchestrationError> {
        valid_identifier(checkout_id)?;
        valid_identifier(execution_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let Some(record) = file.executions.iter_mut().find(|record| {
            record.checkout_id == checkout_id && record.execution_id == execution_id
        }) else {
            return Ok(());
        };
        if record.outcome.is_some() || record.process.is_some() {
            return Ok(());
        }
        record.process = Some(process);
        self.write_file(&file)
    }

    /// HS2-HTS89S: the process group is gone (it ended, or was stopped).
    pub fn process_gone(
        &self,
        checkout_id: &str,
        execution_id: &str,
    ) -> Result<(), OrchestrationError> {
        self.set_process(checkout_id, execution_id, None, false)
    }

    fn set_process(
        &self,
        checkout_id: &str,
        execution_id: &str,
        process: Option<StepProcess>,
        only_running: bool,
    ) -> Result<(), OrchestrationError> {
        valid_identifier(checkout_id)?;
        valid_identifier(execution_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let Some(record) = file.executions.iter_mut().find(|record| {
            record.checkout_id == checkout_id && record.execution_id == execution_id
        }) else {
            return Ok(());
        };
        if (only_running && record.outcome.is_some()) || record.process == process {
            return Ok(());
        }
        record.process = process;
        self.write_file(&file)
    }

    /// HS2-HTS89S: one execution's record.
    pub fn record(
        &self,
        checkout_id: &str,
        execution_id: &str,
    ) -> Result<Option<HostedValidationRecord>, OrchestrationError> {
        valid_identifier(checkout_id)?;
        valid_identifier(execution_id)?;
        Ok(self.read_file()?.executions.into_iter().find(|record| {
            record.checkout_id == checkout_id && record.execution_id == execution_id
        }))
    }

    /// HS2-HTS89S: every execution that still records a process group.
    pub fn with_process(&self) -> Result<Vec<HostedValidationRecord>, OrchestrationError> {
        Ok(self
            .read_file()?
            .executions
            .into_iter()
            .filter(|record| record.process.is_some())
            .collect())
    }

    fn put(
        &self,
        checkout_id: &str,
        execution_id: &str,
        now_ms: u64,
        outcome: Option<ExecutionOutcome>,
    ) -> Result<HostedValidationKnown, OrchestrationError> {
        valid_identifier(checkout_id)?;
        valid_identifier(execution_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        if let Some(record) = file
            .executions
            .iter_mut()
            .find(|record| record.checkout_id == checkout_id && record.execution_id == execution_id)
        {
            let before = match &record.outcome {
                Some(outcome) => HostedValidationKnown::Finished(outcome.clone()),
                None => HostedValidationKnown::Accepted,
            };
            if record.outcome.is_none() && outcome.is_some() {
                record.outcome = outcome;
                // An execution that ended no longer runs as a process group.
                record.process = None;
                self.write_file(&file)?;
            }
            return Ok(before);
        }
        while file.executions.len() >= MAX_HOSTED_VALIDATION_RECORDS {
            let index = file
                .executions
                .iter()
                .position(|record| record.outcome.is_some())
                .unwrap_or(0);
            file.executions.remove(index);
        }
        file.executions.push(HostedValidationRecord {
            checkout_id: checkout_id.into(),
            execution_id: execution_id.into(),
            accepted_at_ms: now_ms,
            outcome,
            process: None,
        });
        self.write_file(&file)?;
        Ok(HostedValidationKnown::Unknown)
    }
}

fn validate(record: &HostedValidationRecord) -> Result<(), OrchestrationError> {
    valid_identifier(&record.checkout_id)?;
    valid_identifier(&record.execution_id)
}
