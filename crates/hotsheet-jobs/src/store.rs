//! The machine-local Job store: `${HOTSHEET_HOME}/jobs/<checkout-id>/<ulid>/` holding an
//! atomically replaced `job.json` and a bounded `feed.jsonl` (`docs/33-jobs.md` §33.3).
//!
//! Every mutation takes the checkout's advisory file lock, so CLI producers, a monitor
//! and a server on one machine apply one ordered state machine. Reads take no lock: the
//! snapshot is replaced by rename, and feed entries past the snapshot's cursor are ignored.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::clock::{Clock, IdSource, SystemClock, UlidSource};
use crate::error::JobError;
use crate::feed::{self, FeedLine, FeedPage, FeedWrite, MAX_FEED_BATCH};
use crate::lock::DirLock;
use crate::model::{
    self, ActiveStatus, Capabilities, ExternalRef, FeedState, FinalStatus, Job, JobKind, Lease,
    LeaseToken, MAX_KEY, MAX_LATEST, MAX_SUMMARY, MAX_TITLE, MAX_URL, MAX_VALUE, Outcome,
    ProducerFamily, RECORD_VERSION, Status, TicketLink,
};
use crate::probe::{OwnerProbe, OwnerState};
use crate::state::check_transition;
use crate::steps::{Step, validate_steps};

const MAX_JOB_BYTES: u64 = 1024 * 1024;
const MAX_HISTORY_BYTES: u64 = 256 * 1024;
const HISTORY_SAMPLES: usize = 16;
const HISTORY_KEYS: usize = 128;
/// Fewer completed runs than this give no historical ETA.
pub const MIN_HISTORY_SAMPLES: usize = 3;
const MIN_FEED_BYTES: u64 = 16 * 1024;

/// Lease, retention and size bounds. The defaults are the shipped behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoreConfig {
    /// How long one renewal keeps a producer live. After it the Job is stalled.
    pub lease_ms: u64,
    /// How long a stalled Job waits for a late renewal before it becomes `lost`.
    pub lost_after_ms: u64,
    /// Retention of `succeeded` and `cancelled` Jobs.
    pub retain_succeeded_ms: u64,
    /// Retention of `failed` and `lost` Jobs, longer so a failure can be read.
    pub retain_failed_ms: u64,
    /// Hard cap of Job directories per checkout.
    pub max_jobs: usize,
    /// Cap of one Job's `feed.jsonl`.
    pub max_feed_bytes: u64,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            lease_ms: 60_000,
            lost_after_ms: 5 * 60_000,
            retain_succeeded_ms: 15 * 60_000,
            retain_failed_ms: 24 * 60 * 60_000,
            max_jobs: 64,
            max_feed_bytes: 256 * 1024,
        }
    }
}

/// A Job to create. It starts `queued`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewJob {
    pub checkout_id: String,
    pub kind: JobKind,
    pub job_key: String,
    pub title: String,
    pub capabilities: Capabilities,
    pub ticket: Option<TicketLink>,
    pub external: Option<ExternalRef>,
    pub retry_of: Option<String>,
    pub url: Option<String>,
}

/// A producer taking a lease: `queued` to `running` or `waiting`, or a watch reattach.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acquire {
    /// Unique per producer instance, for example `monitor:<launch id>`.
    pub owner: String,
    pub status: ActiveStatus,
    /// Process-journal launch id of an owned process.
    pub launch_id: Option<String>,
    /// The external operation. A watch reattach must name the Job's own.
    pub external: Option<ExternalRef>,
}

/// Leave a value, remove it, or replace it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Change<T> {
    #[default]
    Keep,
    Clear,
    Set(T),
}

/// A report inside one lease. Every accepted report also renews the lease.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JobUpdate {
    /// The producer's own counter. A report at or below the last accepted one is refused.
    pub report_seq: Option<u64>,
    pub status: Option<ActiveStatus>,
    /// Replaces the whole step tree.
    pub steps: Option<Vec<Step>>,
    /// Producer ETA as an absolute finish time in milliseconds.
    pub eta: Change<u64>,
    /// Latest-value fields; `None` removes a key.
    pub latest: BTreeMap<String, Option<String>>,
    pub feed: Vec<FeedLine>,
    pub url: Option<String>,
    /// A cancel request could not be honored: clears the request and records why.
    pub cancel_failed: Option<String>,
}

/// Jobs of one checkout, oldest first, plus directories that could not be read.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Listing {
    pub jobs: Vec<Job>,
    pub unreadable: Vec<Unreadable>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Unreadable {
    pub id: String,
    /// `unsupported_version` or `damaged`.
    pub reason: &'static str,
}

/// What one reconciliation pass changed.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Reconciled {
    /// Jobs that became `lost`.
    pub lost: Vec<String>,
    /// Job directories removed by retention or because they were damaged.
    pub removed: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct History {
    version: u32,
    keys: BTreeMap<String, KeyHistory>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct KeyHistory {
    updated_ms: u64,
    durations_ms: Vec<u64>,
}

enum Saved {
    Unchanged,
    Changed,
    WithFeed(FeedWrite),
}

/// Handle on the Job store of one machine home. Cheap to clone.
#[derive(Clone)]
pub struct JobStore {
    root: PathBuf,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
    config: StoreConfig,
}

impl std::fmt::Debug for JobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobStore")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl JobStore {
    /// The store under `machine_home` (the resolved `HOTSHEET_HOME`), with the wall
    /// clock, random ULIDs and default bounds.
    pub fn at(machine_home: impl AsRef<Path>) -> Self {
        Self::with(
            machine_home,
            Arc::new(SystemClock),
            Arc::new(UlidSource),
            StoreConfig::default(),
        )
    }

    pub fn with(
        machine_home: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
        config: StoreConfig,
    ) -> Self {
        Self {
            root: machine_home.as_ref().join("jobs"),
            clock,
            ids,
            config: StoreConfig {
                max_jobs: config.max_jobs.max(1),
                max_feed_bytes: config.max_feed_bytes.max(MIN_FEED_BYTES),
                ..config
            },
        }
    }

    pub fn config(&self) -> StoreConfig {
        self.config
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    /// The directory holding one checkout's Jobs. A caller may watch it for changes.
    pub fn checkout_dir(&self, checkout_id: &str) -> Result<PathBuf, JobError> {
        model::checkout_token(checkout_id)?;
        Ok(self.root.join(checkout_id))
    }

    /// Create a `queued` Job. Retention runs first; when the checkout is at its cap the
    /// oldest finished Job makes room, and a checkout full of unfinished Jobs refuses.
    pub fn create(&self, new: NewJob) -> Result<Job, JobError> {
        model::checkout_token(&new.checkout_id)?;
        model::token("kind.name", &new.kind.name)?;
        model::text("job_key", &new.job_key, MAX_KEY)?;
        model::text("title", &new.title, MAX_TITLE)?;
        model::validate_ticket(new.ticket.as_ref())?;
        model::validate_external(new.external.as_ref())?;
        if let Some(previous) = &new.retry_of {
            model::job_id(previous)?;
        }
        validate_url(new.url.as_deref(), new.capabilities)?;
        check_transition(None, Status::Queued, new.kind.family)?;

        let checkout = new.checkout_id.clone();
        let _lock = self.lock(&checkout)?;
        let now = self.clock.now_ms();
        self.prune_locked(&checkout, now)?;
        self.make_room(&checkout)?;

        let dir = self.root.join(&checkout);
        let mut id = self.ids.next_id(now);
        let mut attempts = 0;
        while dir.join(&id).exists() {
            attempts += 1;
            if attempts > 16 {
                return Err(JobError::Invalid("job_id"));
            }
            id = self.ids.next_id(now);
        }
        model::job_id(&id)?;
        let mut job = Job {
            version: RECORD_VERSION,
            id,
            checkout_id: checkout.clone(),
            kind: new.kind,
            job_key: new.job_key,
            title: new.title,
            capabilities: new.capabilities,
            ticket: new.ticket,
            external: new.external,
            retry_of: new.retry_of,
            url: new.url,
            status: Status::Queued,
            created_ms: now,
            updated_ms: now,
            started_ms: None,
            finished_ms: None,
            revision: 0,
            lease: None,
            cancel_requested_ms: None,
            cancel_failure: None,
            stop_watching_requested_ms: None,
            producer_eta_ms: None,
            history_estimate_ms: None,
            outcome: None,
            steps: Vec::new(),
            latest: BTreeMap::new(),
            feed: FeedState::default(),
        };
        fs::create_dir(dir.join(&job.id))?;
        self.save(&mut job, now)?;
        self.bump(&checkout)?;
        Ok(job)
    }

    /// One Job's snapshot.
    pub fn get(&self, checkout_id: &str, job_id: &str) -> Result<Job, JobError> {
        model::checkout_token(checkout_id)?;
        model::job_id(job_id)?;
        self.load(checkout_id, job_id)
    }

    /// Every Job of a checkout, oldest first. Never writes.
    pub fn list(&self, checkout_id: &str) -> Result<Listing, JobError> {
        model::checkout_token(checkout_id)?;
        let mut listing = Listing::default();
        for id in self.job_ids(checkout_id)? {
            match self.load(checkout_id, &id) {
                Ok(job) => listing.jobs.push(job),
                Err(JobError::NotFound) => {}
                Err(JobError::UnsupportedVersion(_)) => listing.unreadable.push(Unreadable {
                    id,
                    reason: "unsupported_version",
                }),
                Err(JobError::Corrupt) => listing.unreadable.push(Unreadable {
                    id,
                    reason: "damaged",
                }),
                Err(error) => return Err(error),
            }
        }
        listing
            .jobs
            .sort_by(|a, b| (a.created_ms, &a.id).cmp(&(b.created_ms, &b.id)));
        Ok(listing)
    }

    /// Checkout ids that have a Job directory on this machine.
    pub fn checkouts(&self) -> Result<Vec<String>, JobError> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if model::checkout_token(&name).is_ok() && entry.file_type()?.is_dir() {
                ids.push(name);
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// A counter that grows with every change in the checkout. Zero before the first.
    pub fn revision(&self, checkout_id: &str) -> Result<u64, JobError> {
        model::checkout_token(checkout_id)?;
        Ok(read_revision(&self.root.join(checkout_id)))
    }

    /// Take a lease. Valid for a `queued` Job, for a `lost` watch Job (reattach), and for
    /// an active watch Job whose lease expired: that takeover is the `lost` and reattach
    /// edges applied in one locked step. Each success starts a new lease generation, so
    /// the previous producer's later messages are refused.
    pub fn acquire(
        &self,
        checkout_id: &str,
        job_id: &str,
        request: Acquire,
    ) -> Result<(Job, LeaseToken), JobError> {
        model::token("lease.owner", &request.owner)?;
        if let Some(launch_id) = &request.launch_id {
            model::token("lease.launch_id", launch_id)?;
        }
        model::validate_external(request.external.as_ref())?;
        let target = Status::from(request.status);
        let job = self.mutate(checkout_id, job_id, |job, now| {
            let family = job.kind.family;
            match job.status {
                Status::Queued => check_transition(Some(Status::Queued), target, family)?,
                Status::Lost => {
                    check_transition(Some(Status::Lost), target, family)?;
                    verify_external(job, request.external.as_ref(), true)?;
                }
                Status::Running | Status::Waiting => {
                    let expired = job
                        .lease
                        .as_ref()
                        .is_none_or(|lease| now >= lease.expires_ms);
                    if !expired {
                        return Err(JobError::LeaseHeld);
                    }
                    check_transition(Some(job.status), Status::Lost, family)?;
                    check_transition(Some(Status::Lost), target, family)?;
                    verify_external(job, request.external.as_ref(), true)?;
                }
                finished => check_transition(Some(finished), target, family)?,
            }
            if job.status == Status::Queued {
                verify_external(job, request.external.as_ref(), false)?;
                if job.external.is_none() {
                    job.external.clone_from(&request.external);
                }
            }
            let generation = job.lease.as_ref().map_or(0, |lease| lease.generation) + 1;
            job.lease = Some(Lease {
                owner: request.owner.clone(),
                generation,
                expires_ms: now.saturating_add(self.config.lease_ms),
                launch_id: request.launch_id.clone(),
                last_report_seq: None,
            });
            job.status = target;
            job.finished_ms = None;
            if job.started_ms.is_none() {
                job.started_ms = Some(now);
                job.history_estimate_ms = self.history_estimate(&job.checkout_id, &job.job_key);
            }
            Ok(Saved::Changed)
        })?;
        let token = job.token().ok_or(JobError::Corrupt)?;
        Ok((job, token))
    }

    /// Heartbeat. Accepted while the Job is active, including after the lease expired but
    /// before reconciliation declared the Job lost.
    pub fn renew(
        &self,
        checkout_id: &str,
        job_id: &str,
        token: &LeaseToken,
    ) -> Result<Job, JobError> {
        self.update(checkout_id, job_id, token, JobUpdate::default())
    }

    /// Report progress inside the current lease and renew it. May move between `running`
    /// and `waiting`; anything else about status needs `finish` or `confirm_cancel`.
    pub fn update(
        &self,
        checkout_id: &str,
        job_id: &str,
        token: &LeaseToken,
        update: JobUpdate,
    ) -> Result<Job, JobError> {
        if update.feed.len() > MAX_FEED_BATCH {
            return Err(JobError::Invalid("feed"));
        }
        if let Some(steps) = &update.steps {
            validate_steps(steps)?;
        }
        model::optional_text(
            "cancel_failed",
            update.cancel_failed.as_deref(),
            MAX_SUMMARY,
        )?;
        let feed_path = self.feed_path(checkout_id, job_id)?;
        self.mutate(checkout_id, job_id, |job, now| {
            verify_token(job, token)?;
            if !job.status.is_active() {
                return Err(JobError::NotActive(job.status));
            }
            let lease = job.lease.as_mut().ok_or(JobError::Corrupt)?;
            if let Some(presented) = update.report_seq {
                if let Some(last) = lease.last_report_seq
                    && presented <= last
                {
                    return Err(JobError::StaleReport { presented, last });
                }
                lease.last_report_seq = Some(presented);
            }
            lease.expires_ms = now.saturating_add(self.config.lease_ms);
            if let Some(status) = update.status.map(Status::from)
                && status != job.status
            {
                check_transition(Some(job.status), status, job.kind.family)?;
                job.status = status;
            }
            if let Some(steps) = &update.steps {
                job.steps.clone_from(steps);
            }
            match update.eta {
                Change::Keep => {}
                Change::Clear => job.producer_eta_ms = None,
                Change::Set(finish_at_ms) => job.producer_eta_ms = Some(finish_at_ms),
            }
            for (key, value) in &update.latest {
                match value {
                    Some(value) => {
                        model::token("latest", key)?;
                        model::text("latest", value, MAX_VALUE)?;
                        job.latest.insert(key.clone(), value.clone());
                    }
                    None => {
                        job.latest.remove(key);
                    }
                }
            }
            if job.latest.len() > MAX_LATEST {
                return Err(JobError::Invalid("latest"));
            }
            if let Some(url) = &update.url {
                validate_url(Some(url), job.capabilities)?;
                job.url = Some(url.clone());
            }
            if let Some(reason) = &update.cancel_failed {
                job.cancel_requested_ms = None;
                job.cancel_failure = Some(reason.clone());
            }
            if update.feed.is_empty() {
                return Ok(Saved::Changed);
            }
            let write = feed::plan(
                &feed_path,
                &mut job.feed,
                &update.feed,
                now,
                self.config.max_feed_bytes,
            )?;
            Ok(Saved::WithFeed(write))
        })
    }

    /// Report the outcome. The outcome is immutable: an identical repeat from the same
    /// lease is accepted as a no-op, and any different repeat is a rejected transition.
    pub fn finish(
        &self,
        checkout_id: &str,
        job_id: &str,
        token: &LeaseToken,
        status: FinalStatus,
        outcome: Outcome,
    ) -> Result<Job, JobError> {
        self.end(checkout_id, job_id, token, Status::from(status), outcome)
    }

    /// The owning process or the external operation confirmed cancellation. This records
    /// an observed fact, so it does not need the `cancel` capability (that gates
    /// [`JobStore::request_cancel`]) or a prior request: an owner can be stopped, or a run
    /// cancelled, outside Hot Sheet.
    pub fn confirm_cancel(
        &self,
        checkout_id: &str,
        job_id: &str,
        token: &LeaseToken,
        outcome: Outcome,
    ) -> Result<Job, JobError> {
        self.end(checkout_id, job_id, token, Status::Cancelled, outcome)
    }

    fn end(
        &self,
        checkout_id: &str,
        job_id: &str,
        token: &LeaseToken,
        target: Status,
        outcome: Outcome,
    ) -> Result<Job, JobError> {
        model::validate_outcome(&outcome)?;
        let mut duration = None;
        let job = self.mutate(checkout_id, job_id, |job, now| {
            verify_token(job, token)?;
            if job.status == target && job.outcome.as_ref() == Some(&outcome) {
                return Ok(Saved::Unchanged);
            }
            check_transition(Some(job.status), target, job.kind.family)?;
            job.status = target;
            job.finished_ms = Some(now);
            job.outcome = Some(outcome.clone());
            job.producer_eta_ms = None;
            if target == Status::Succeeded {
                duration = job.started_ms.map(|started| now.saturating_sub(started));
            }
            Ok(Saved::Changed)
        })?;
        if let Some(duration) = duration {
            // History only feeds ETA. A failure to record it must not fail the outcome.
            let _ = self.record_duration(checkout_id, &job.job_key, duration, job.updated_ms);
        }
        Ok(job)
    }

    /// Ask for cancellation. A `queued` Job is cancelled at once, because no producer
    /// has started work. An active Job only records the request: its producer confirms
    /// with [`JobStore::confirm_cancel`] or reports the failure in an update.
    pub fn request_cancel(&self, checkout_id: &str, job_id: &str) -> Result<Job, JobError> {
        self.mutate(checkout_id, job_id, |job, now| {
            if !job.capabilities.cancel {
                return Err(JobError::Unsupported("cancel"));
            }
            match job.status {
                Status::Queued => {
                    check_transition(Some(Status::Queued), Status::Cancelled, job.kind.family)?;
                    job.status = Status::Cancelled;
                    job.finished_ms = Some(now);
                    job.cancel_requested_ms = Some(now);
                    Ok(Saved::Changed)
                }
                Status::Running | Status::Waiting => {
                    if job.cancel_requested_ms.is_some() {
                        return Ok(Saved::Unchanged);
                    }
                    job.cancel_requested_ms = Some(now);
                    job.cancel_failure = None;
                    Ok(Saved::Changed)
                }
                other => Err(JobError::NotActive(other)),
            }
        })
    }

    /// Stop watching a watch Job without touching its external operation. This is a local
    /// action, distinct from cancel, and every watch Job has it. A `queued` Job ends as
    /// `cancelled` at once; on an active Job the request is recorded and the monitor
    /// confirms it with [`JobStore::confirm_cancel`]. A lost watch Job has no monitor to
    /// stop: dismiss it instead.
    pub fn stop_watching(&self, checkout_id: &str, job_id: &str) -> Result<Job, JobError> {
        self.mutate(checkout_id, job_id, |job, now| {
            if job.kind.family != ProducerFamily::Watch {
                return Err(JobError::Unsupported("stop_watching"));
            }
            match job.status {
                Status::Queued => {
                    check_transition(Some(Status::Queued), Status::Cancelled, job.kind.family)?;
                    job.status = Status::Cancelled;
                    job.finished_ms = Some(now);
                    job.stop_watching_requested_ms = Some(now);
                    job.outcome = Some(stopped_watching_outcome());
                    Ok(Saved::Changed)
                }
                Status::Running | Status::Waiting => {
                    if job.stop_watching_requested_ms.is_some() {
                        return Ok(Saved::Unchanged);
                    }
                    job.stop_watching_requested_ms = Some(now);
                    Ok(Saved::Changed)
                }
                other => Err(JobError::NotActive(other)),
            }
        })
    }

    /// Give the lease up without an outcome, for a producer that stops deliberately. The
    /// Job stays active and reads as stalled at once, so a watch monitor that starts
    /// later can take it over without waiting for the lease to run out.
    pub fn release(
        &self,
        checkout_id: &str,
        job_id: &str,
        token: &LeaseToken,
    ) -> Result<Job, JobError> {
        self.mutate(checkout_id, job_id, |job, now| {
            verify_token(job, token)?;
            if !job.status.is_active() {
                return Err(JobError::NotActive(job.status));
            }
            let lease = job.lease.as_mut().ok_or(JobError::Corrupt)?;
            if lease.expires_ms <= now {
                return Ok(Saved::Unchanged);
            }
            lease.expires_ms = now;
            Ok(Saved::Changed)
        })
    }

    /// Detect lost producers, then apply retention. An active Job whose lease expired
    /// becomes `lost` when the probe reports its owner gone, or after the grace window
    /// when the probe cannot tell. A confirmed live owner is never declared lost.
    pub fn reconcile(
        &self,
        checkout_id: &str,
        probe: &dyn OwnerProbe,
    ) -> Result<Reconciled, JobError> {
        model::checkout_token(checkout_id)?;
        if !self.root.join(checkout_id).is_dir() {
            return Ok(Reconciled::default());
        }
        let _lock = self.lock(checkout_id)?;
        let now = self.clock.now_ms();
        let mut result = Reconciled::default();
        for id in self.job_ids(checkout_id)? {
            let Ok(mut job) = self.load(checkout_id, &id) else {
                continue;
            };
            if !job.status.is_active() {
                continue;
            }
            let expires = job.lease.as_ref().map_or(0, |lease| lease.expires_ms);
            if now < expires {
                continue;
            }
            let lost = match probe.owner_state(&job) {
                OwnerState::Alive => false,
                OwnerState::Gone => true,
                OwnerState::Unknown => now >= expires.saturating_add(self.config.lost_after_ms),
            };
            if lost {
                check_transition(Some(job.status), Status::Lost, job.kind.family)?;
                job.status = Status::Lost;
                job.finished_ms = Some(now);
                job.producer_eta_ms = None;
                self.save(&mut job, now)?;
                result.lost.push(id);
            }
        }
        result.removed = self.prune_locked(checkout_id, now)?;
        if !result.lost.is_empty() || !result.removed.is_empty() {
            self.bump(checkout_id)?;
        }
        Ok(result)
    }

    /// Remove a finished Job before its retention window ends.
    pub fn dismiss(&self, checkout_id: &str, job_id: &str) -> Result<(), JobError> {
        model::checkout_token(checkout_id)?;
        model::job_id(job_id)?;
        if !self.root.join(checkout_id).is_dir() {
            return Err(JobError::NotFound);
        }
        let _lock = self.lock(checkout_id)?;
        let job = self.load(checkout_id, job_id)?;
        if !job.status.is_finished() {
            return Err(JobError::Unfinished(job.status));
        }
        fs::remove_dir_all(self.root.join(checkout_id).join(job_id))?;
        self.bump(checkout_id)?;
        Ok(())
    }

    /// A retry is a new `queued` Job linked to the finished one; the old outcome stays.
    pub fn retry(&self, checkout_id: &str, job_id: &str) -> Result<Job, JobError> {
        let old = self.get(checkout_id, job_id)?;
        if !old.capabilities.retry {
            return Err(JobError::Unsupported("retry"));
        }
        if !old.status.is_finished() {
            return Err(JobError::Unfinished(old.status));
        }
        self.create(NewJob {
            checkout_id: old.checkout_id,
            kind: old.kind,
            job_key: old.job_key,
            title: old.title,
            capabilities: old.capabilities,
            ticket: old.ticket,
            external: None,
            retry_of: Some(old.id),
            url: None,
        })
    }

    /// Feed entries after `after` (0 reads from the start), at most `limit`.
    pub fn read_feed(
        &self,
        checkout_id: &str,
        job_id: &str,
        after: u64,
        limit: usize,
    ) -> Result<FeedPage, JobError> {
        let job = self.get(checkout_id, job_id)?;
        let path = self.feed_path(checkout_id, job_id)?;
        Ok(feed::read_page(&path, job.feed, after, limit)?)
    }

    /// Median duration of recent successful Jobs with this key, when at least
    /// [`MIN_HISTORY_SAMPLES`] exist.
    pub fn history_estimate(&self, checkout_id: &str, job_key: &str) -> Option<u64> {
        let history = read_history(&self.root.join(checkout_id))?;
        let mut samples = history.keys.get(job_key)?.durations_ms.clone();
        if samples.len() < MIN_HISTORY_SAMPLES {
            return None;
        }
        samples.sort_unstable();
        let mid = samples.len() / 2;
        Some(if samples.len() % 2 == 1 {
            samples[mid]
        } else {
            samples[mid - 1] / 2 + samples[mid] / 2 + (samples[mid - 1] % 2 + samples[mid] % 2) / 2
        })
    }

    fn mutate(
        &self,
        checkout_id: &str,
        job_id: &str,
        apply: impl FnOnce(&mut Job, u64) -> Result<Saved, JobError>,
    ) -> Result<Job, JobError> {
        model::checkout_token(checkout_id)?;
        model::job_id(job_id)?;
        if !self.root.join(checkout_id).is_dir() {
            return Err(JobError::NotFound);
        }
        let _lock = self.lock(checkout_id)?;
        let mut job = self.load(checkout_id, job_id)?;
        let now = self.clock.now_ms();
        match apply(&mut job, now)? {
            Saved::Unchanged => return Ok(job),
            Saved::Changed => self.save(&mut job, now)?,
            Saved::WithFeed(write) => {
                self.save(&mut job, now)?;
                feed::apply(&self.feed_path(checkout_id, job_id)?, write)?;
            }
        }
        self.bump(checkout_id)?;
        Ok(job)
    }

    fn lock(&self, checkout_id: &str) -> Result<DirLock, JobError> {
        let dir = self.root.join(checkout_id);
        fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&self.root, &dir] {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            }
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(".lock"))?;
        Ok(DirLock::acquire(file)?)
    }

    fn feed_path(&self, checkout_id: &str, job_id: &str) -> Result<PathBuf, JobError> {
        model::checkout_token(checkout_id)?;
        model::job_id(job_id)?;
        Ok(self.root.join(checkout_id).join(job_id).join("feed.jsonl"))
    }

    fn job_ids(&self, checkout_id: &str) -> Result<Vec<String>, JobError> {
        let entries = match fs::read_dir(self.root.join(checkout_id)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if model::job_id(&name).is_ok() && entry.file_type()?.is_dir() {
                ids.push(name);
            }
        }
        ids.sort();
        Ok(ids)
    }

    fn load(&self, checkout_id: &str, job_id: &str) -> Result<Job, JobError> {
        let path = self.root.join(checkout_id).join(job_id).join("job.json");
        let bytes = match read_bounded(&path, MAX_JOB_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Err(JobError::Corrupt),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(JobError::NotFound);
            }
            Err(error) => return Err(error.into()),
        };
        #[derive(Deserialize)]
        struct Versioned {
            version: u64,
        }
        let versioned: Versioned = serde_json::from_slice(&bytes).map_err(|_| JobError::Corrupt)?;
        if versioned.version != u64::from(RECORD_VERSION) {
            return Err(JobError::UnsupportedVersion(versioned.version));
        }
        let job: Job = serde_json::from_slice(&bytes).map_err(|_| JobError::Corrupt)?;
        if job.id != job_id || job.checkout_id != checkout_id {
            return Err(JobError::Corrupt);
        }
        Ok(job)
    }

    fn save(&self, job: &mut Job, now: u64) -> Result<(), JobError> {
        job.updated_ms = now;
        job.revision += 1;
        let bytes = serde_json::to_vec(job).map_err(io::Error::from)?;
        if bytes.len() as u64 > MAX_JOB_BYTES {
            return Err(JobError::Invalid("record_size"));
        }
        let dir = self.root.join(&job.checkout_id).join(&job.id);
        write_atomic(&dir, "job.json", &bytes, true)?;
        Ok(())
    }

    fn bump(&self, checkout_id: &str) -> Result<(), JobError> {
        let dir = self.root.join(checkout_id);
        let next = read_revision(&dir) + 1;
        write_atomic(&dir, "revision", next.to_string().as_bytes(), false)?;
        Ok(())
    }

    /// Retention and crash cleanup. The caller holds the checkout lock, so no writer is
    /// in flight: any temporary file is abandoned and a directory without a snapshot is
    /// a create that never finished.
    fn prune_locked(&self, checkout_id: &str, now: u64) -> Result<Vec<String>, JobError> {
        let dir = self.root.join(checkout_id);
        remove_temp_files(&dir)?;
        let mut removed = Vec::new();
        for id in self.job_ids(checkout_id)? {
            let job_dir = dir.join(&id);
            remove_temp_files(&job_dir)?;
            let expired = match self.load(checkout_id, &id) {
                Ok(job) => job.finished_ms.is_some_and(|finished| {
                    let window = match job.status {
                        Status::Succeeded | Status::Cancelled => self.config.retain_succeeded_ms,
                        Status::Failed | Status::Lost => self.config.retain_failed_ms,
                        _ => u64::MAX,
                    };
                    job.status.is_finished() && now >= finished.saturating_add(window)
                }),
                // A newer Hot Sheet owns this record. Leave it alone.
                Err(JobError::UnsupportedVersion(_)) => false,
                Err(JobError::Corrupt | JobError::NotFound) => true,
                Err(error) => return Err(error),
            };
            if expired {
                fs::remove_dir_all(&job_dir)?;
                removed.push(id);
            }
        }
        Ok(removed)
    }

    /// Enforce the hard cap before a create, evicting the oldest finished Jobs.
    fn make_room(&self, checkout_id: &str) -> Result<(), JobError> {
        let ids = self.job_ids(checkout_id)?;
        if ids.len() < self.config.max_jobs {
            return Ok(());
        }
        let mut finished: Vec<(u64, String)> = ids
            .iter()
            .filter_map(|id| self.load(checkout_id, id).ok())
            .filter(|job| job.status.is_finished())
            .map(|job| (job.finished_ms.unwrap_or(job.updated_ms), job.id))
            .collect();
        finished.sort();
        let excess = ids.len() + 1 - self.config.max_jobs;
        if finished.len() < excess {
            return Err(JobError::CheckoutFull);
        }
        for (_, id) in finished.into_iter().take(excess) {
            fs::remove_dir_all(self.root.join(checkout_id).join(id))?;
        }
        Ok(())
    }

    fn record_duration(
        &self,
        checkout_id: &str,
        job_key: &str,
        duration_ms: u64,
        now: u64,
    ) -> Result<(), JobError> {
        let _lock = self.lock(checkout_id)?;
        let dir = self.root.join(checkout_id);
        let mut history = match read_history_file(&dir) {
            HistoryFile::Missing | HistoryFile::Damaged => History::default(),
            HistoryFile::Current(history) => history,
            // Written by a newer Hot Sheet: do not overwrite it.
            HistoryFile::Newer => return Ok(()),
        };
        history.version = RECORD_VERSION;
        let entry = history.keys.entry(job_key.to_owned()).or_default();
        entry.updated_ms = now;
        entry.durations_ms.push(duration_ms);
        let extra = entry.durations_ms.len().saturating_sub(HISTORY_SAMPLES);
        entry.durations_ms.drain(..extra);
        while history.keys.len() > HISTORY_KEYS {
            let oldest = history
                .keys
                .iter()
                .min_by_key(|(key, value)| (value.updated_ms, (*key).clone()))
                .map(|(key, _)| key.clone());
            match oldest {
                Some(key) => history.keys.remove(&key),
                None => break,
            };
        }
        let bytes = serde_json::to_vec(&history).map_err(io::Error::from)?;
        write_atomic(&dir, "history.json", &bytes, false)?;
        Ok(())
    }
}

/// The outcome of a watch Job that stopped observing; `refs.cancel` tells it apart from a
/// cancelled external operation.
pub fn stopped_watching_outcome() -> Outcome {
    Outcome {
        summary: Some("Stopped watching; the external operation was not cancelled".into()),
        refs: BTreeMap::from([("cancel".to_owned(), "stopped_watching".to_owned())]),
    }
}

fn verify_token(job: &Job, token: &LeaseToken) -> Result<(), JobError> {
    let current = job.lease.as_ref().map_or(0, |lease| lease.generation);
    let owner_matches = job
        .lease
        .as_ref()
        .is_some_and(|lease| lease.owner == token.owner);
    if token.generation != current || !owner_matches {
        return Err(JobError::StaleLease {
            presented: token.generation,
            current,
        });
    }
    Ok(())
}

/// A reattach (`required`) must name the Job's external operation. A first acquire only
/// has to agree with a reference the Job already carries.
fn verify_external(
    job: &Job,
    presented: Option<&ExternalRef>,
    required: bool,
) -> Result<(), JobError> {
    match (job.external.as_ref(), presented) {
        (Some(known), Some(presented)) if known == presented => Ok(()),
        (Some(_), Some(_)) => Err(JobError::ExternalMismatch),
        (None, _) | (Some(_), None) if !required => Ok(()),
        _ => Err(JobError::ExternalMismatch),
    }
}

fn validate_url(url: Option<&str>, capabilities: Capabilities) -> Result<(), JobError> {
    let Some(url) = url else { return Ok(()) };
    if !capabilities.open_url {
        return Err(JobError::Unsupported("open_url"));
    }
    model::text("url", url, MAX_URL)?;
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(())
    } else {
        Err(JobError::Invalid("url"))
    }
}

/// `Ok(None)` when the file is larger than `max`.
fn read_bounded(path: &Path, max: u64) -> io::Result<Option<Vec<u8>>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= max).then_some(bytes))
}

fn write_atomic(dir: &Path, name: &str, bytes: &[u8], sync: bool) -> io::Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(bytes)?;
    temp.flush()?;
    if sync {
        temp.as_file().sync_all()?;
    }
    temp.persist(dir.join(name)).map_err(|error| error.error)?;
    Ok(())
}

fn remove_temp_files(dir: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(".tmp") && entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn read_revision(dir: &Path) -> u64 {
    fs::read_to_string(dir.join("revision"))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

enum HistoryFile {
    Missing,
    Damaged,
    Newer,
    Current(History),
}

fn read_history_file(dir: &Path) -> HistoryFile {
    let bytes = match read_bounded(&dir.join("history.json"), MAX_HISTORY_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return HistoryFile::Damaged,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return HistoryFile::Missing,
        Err(_) => return HistoryFile::Damaged,
    };
    match serde_json::from_slice::<History>(&bytes) {
        Ok(history) if history.version == RECORD_VERSION => HistoryFile::Current(history),
        Ok(_) => HistoryFile::Newer,
        Err(_) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(value)
                if value
                    .get("version")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|version| version != u64::from(RECORD_VERSION)) =>
            {
                HistoryFile::Newer
            }
            _ => HistoryFile::Damaged,
        },
    }
}

fn read_history(dir: &Path) -> Option<History> {
    match read_history_file(dir) {
        HistoryFile::Current(history) => Some(history),
        _ => None,
    }
}
