//! HS2-2R32SD: a Windows job object that holds one validation step and everything it
//! starts, so a timeout or a cancel ends the whole tree, as a process group does on
//! Unix. The job itself lives in `hotsheet_process_journal::tree_job` (HS2-CYDK9Z), shared
//! with the external claim-backend plugin runner; see there for how it is used.

#[cfg(windows)]
pub(super) use hotsheet_process_journal::tree_job::{TreeJob as StepJob, resume};
