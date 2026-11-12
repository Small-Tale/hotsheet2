//! Jobs (`docs/33-jobs.md`, HS2-GCHX8V): transient, machine-local records of long-running
//! work such as a test run, a project command, or a wait for CI. A Job is not a ticket.
//!
//! - [`Job`] is the snapshot: identity, kind, job key, status, weighted step tree, ETA,
//!   feed cursor, capabilities and an optional qualified ticket link.
//! - [`transition_allowed`] is the state machine of §33.2; every unlisted edge is refused.
//! - [`JobStore`] persists Jobs under `${HOTSHEET_HOME}/jobs/<checkout-id>/<ulid>/` and is
//!   safe to use from several processes at once.
//!
//! A producer creates a Job, takes a lease with [`JobStore::acquire`], and presents the
//! returned [`LeaseToken`] with every report. A token from an earlier lease generation is
//! refused, so a late message cannot overwrite a newer generation or an outcome. A Job
//! whose lease is not renewed reads as [`Liveness::Stalled`] and becomes `lost` when
//! [`JobStore::reconcile`] runs after the grace window.
//!
//! Nothing here reads the wall clock or a random source except through [`Clock`] and
//! [`IdSource`].

mod clock;
mod error;
mod feed;
mod lock;
mod model;
mod probe;
mod state;
mod steps;
mod store;

pub use clock::{Clock, IdSource, ManualClock, SequenceIds, SystemClock, UlidSource};
pub use error::JobError;
pub use feed::{
    FeedEntry, FeedGap, FeedLine, FeedPage, FeedStream, MAX_FEED_BATCH, MAX_FEED_LINE_BYTES,
};
pub use model::{
    ActiveStatus, Capabilities, Eta, EtaSource, ExternalRef, FeedState, FinalStatus, Job, JobKind,
    Lease, LeaseToken, Liveness, Outcome, ProducerFamily, RECORD_VERSION, Status, TicketLink,
};
pub use probe::{JournalProbe, NoProbe, OwnerProbe, OwnerState};
pub use state::{check_transition, transition_allowed};
pub use steps::{
    FlatStep, Progress, Step, StepStatus, rollup, step_progress, tree_from_flat, validate_steps,
};
pub use store::{
    Acquire, Change, JobStore, JobUpdate, Listing, MIN_HISTORY_SAMPLES, NewJob, Reconciled,
    StoreConfig, Unreadable, stopped_watching_outcome,
};
