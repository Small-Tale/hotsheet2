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
//! Watch Jobs are hosted by a [`Monitor`] (HS2-5TC1GQ): one process per machine home,
//! claimed through a [`MonitorSlot`], observing external operations through
//! [`WatchSource`] implementations.
//!
//! [`watch::PathWatcher`] is the platform file watcher every waiter shares: it registers
//! all of a waiter's paths before one start (HS2-N1QDZW). [`ChangeWaiter`] wraps it for
//! waits that only need to know that something changed.
//!
//! Nothing in the Job core reads the wall clock or a random source except through [`Clock`] and
//! [`IdSource`].

mod ancestry;
mod azure;
mod bitbucket;
mod buildkite;
mod ci;
mod circleci;
mod clock;
mod error;
mod feed;
mod fixture;
mod github;
mod gitlab;
mod jenkins;
mod lock;
mod model;
mod monitor;
mod pipelines;
mod probe;
mod signal;
mod slot;
mod state;
mod steps;
mod store;
pub mod watch;

pub use ancestry::{HostRepository, RepositoryHosts, remote_host_path};
pub use azure::AzurePipelinesSource;
pub use bitbucket::BitbucketPipelinesSource;
pub use buildkite::BuildkiteSource;
pub use ci::{
    CiArtifact, CiOutcome, CiRun, CiWatchOptions, GateReading, GateState, ci_outcome,
    ensure_ci_watch, find_ci_watch, gate_of, gate_state, rewatch_ci, wait_for_gate_change,
};
pub use circleci::CircleciSource;
pub use clock::{Clock, IdSource, ManualClock, SequenceIds, SystemClock, UlidSource};
pub use error::JobError;
pub use feed::{
    FeedEntry, FeedGap, FeedLine, FeedPage, FeedStream, MAX_FEED_BATCH, MAX_FEED_LINE_BYTES,
};
pub use fixture::{FIXTURE_KIND, FixtureWatchSource};
pub use github::{
    AZURE_PIPELINES_KIND, ApiMethod, ApiRequest, ApiResponse, ApiUnavailable,
    BITBUCKET_PIPELINES_KIND, BUILDKITE_KIND, CIRCLECI_KIND, CiProvider, CiTarget,
    GITHUB_ACTIONS_KIND, GITLAB_CI_KIND, GithubActionsSource, GithubApi, JENKINS_KIND, RunFloor,
    UNKNOWN_COMMIT_LIMIT_MS, repo_from_remote,
};
pub use gitlab::GitlabCiSource;
pub use jenkins::JenkinsSource;
pub use model::{
    ActiveStatus, Capabilities, Eta, EtaSource, ExternalRef, FeedState, FinalStatus, Job, JobKind,
    Lease, LeaseToken, Liveness, Outcome, ProducerFamily, RECORD_VERSION, Status, TicketLink,
};
pub use monitor::{
    CURRENT_STEP_KEY, CancelReply, Monitor, MonitorConfig, MonitorControl, MonitorExit,
    Observation, Observed, Poll, Tick, WATCH_STATE_KEY, Wake, WatchError, WatchSource,
};
pub use pipelines::{
    MAX_PIPELINE_ENTRIES, PIPELINE_REPOSITORIES_FILE, PIPELINE_REPOSITORY_REFRESH_MS,
    PipelineRepositories, PipelineRepository,
};
pub use probe::{JournalProbe, NoProbe, OwnerProbe, OwnerState};
pub use signal::{ChangeWaiter, KeptWaiter};
pub use slot::{MonitorClaim, MonitorInfo, MonitorSlot, SlotState, ensure_monitor_running};
pub use state::{check_transition, transition_allowed};
pub use steps::{
    FlatStep, Progress, Step, StepStatus, rollup, step_progress, tree_from_flat, validate_steps,
};
pub use store::{
    Acquire, Change, JobStore, JobUpdate, Listing, MIN_HISTORY_SAMPLES, NewJob, Reconciled,
    StoreConfig, Unreadable, stopped_watching_outcome,
};
