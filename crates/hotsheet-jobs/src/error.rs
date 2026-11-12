//! One error type for the Job core. A refused value is never echoed: it may be local
//! output or a machine-local path, so errors name the field, not its content.

use crate::model::{ProducerFamily, Status};

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    /// A field failed validation. The string names the field only.
    #[error("invalid job field: {0}")]
    Invalid(&'static str),
    #[error("job not found")]
    NotFound,
    /// The edge is not in the `docs/33-jobs.md` §33.2 table for this producer family.
    /// `from` is `None` for a Job that does not exist yet.
    #[error("job transition {} -> {to} is not allowed for {family} producers", from.map_or("new", Status::as_str))]
    InvalidTransition {
        from: Option<Status>,
        to: Status,
        family: ProducerFamily,
    },
    /// A producer write reached a Job that is not `running` or `waiting`.
    #[error("job is {0}, not active")]
    NotActive(Status),
    /// The action needs a finished Job.
    #[error("job is still {0}")]
    Unfinished(Status),
    /// The token belongs to an earlier lease generation (or another owner); its
    /// message cannot overwrite the newer generation or an outcome.
    #[error("stale job lease: generation {presented}, current {current}")]
    StaleLease { presented: u64, current: u64 },
    /// A report arrived after a later one from the same lease.
    #[error("out-of-order job report: {presented} after {last}")]
    StaleReport { presented: u64, last: u64 },
    /// Another producer holds a lease that has not expired.
    #[error("job lease is held by a live producer")]
    LeaseHeld,
    /// The Job's kind did not declare this capability, so the action is absent.
    #[error("job kind does not support {0}")]
    Unsupported(&'static str),
    /// A watch reattach named a different external operation.
    #[error("external operation identity does not match the job")]
    ExternalMismatch,
    #[error("checkout already holds the maximum number of active jobs")]
    CheckoutFull,
    /// Written by a newer Hot Sheet. The record is left untouched.
    #[error("unsupported job record version {0}")]
    UnsupportedVersion(u64),
    #[error("damaged job record")]
    Corrupt,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
