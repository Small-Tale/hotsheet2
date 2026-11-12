//! Whether a Job's owner can still be confirmed when its lease has expired.

use std::collections::BTreeSet;
use std::io;

use hotsheet_process_journal::ProcessJournal;

use crate::model::Job;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerState {
    /// Confirmed running: never declared lost.
    Alive,
    /// Confirmed ended without reporting: lost as soon as the lease has expired.
    Gone,
    /// Cannot tell: lost after the grace window.
    Unknown,
}

pub trait OwnerProbe {
    fn owner_state(&self, job: &Job) -> OwnerState;
}

/// Lease expiry alone decides.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoProbe;

impl OwnerProbe for NoProbe {
    fn owner_state(&self, _job: &Job) -> OwnerState {
        OwnerState::Unknown
    }
}

/// Liveness of owned processes by process-journal launch id, never by PID. A launch the
/// journal recorded as ended is gone. An unended record is not proof of life (the
/// supervisor may have crashed), so it stays unknown.
#[derive(Clone, Debug, Default)]
pub struct JournalProbe {
    ended: BTreeSet<String>,
}

impl JournalProbe {
    /// Read the journal once; one probe serves a whole reconciliation pass.
    pub fn load(journal: &ProcessJournal) -> io::Result<Self> {
        let ended = journal
            .list()?
            .into_iter()
            .filter(|record| record.ended_ms.is_some())
            .map(|record| record.launch_id)
            .collect();
        Ok(Self { ended })
    }
}

impl OwnerProbe for JournalProbe {
    fn owner_state(&self, job: &Job) -> OwnerState {
        let launch_id = job
            .lease
            .as_ref()
            .and_then(|lease| lease.launch_id.as_ref());
        match launch_id {
            Some(launch_id) if self.ended.contains(launch_id) => OwnerState::Gone,
            _ => OwnerState::Unknown,
        }
    }
}
