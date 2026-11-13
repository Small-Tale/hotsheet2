//! The Job monitor (HS2-5TC1GQ): hosts watch Jobs. It owns no work. It observes an
//! external source through a [`WatchSource`], renews each Job's lease, and reports through
//! the Job store, with no server.
//!
//! One monitor serves every watch Job of a machine home whose kind it has a source for.
//! It attaches to `queued` Jobs, reattaches to `lost` ones, and takes over stalled ones,
//! because a watch Job's truth is remote. [`Monitor::tick`] is one deterministic step over
//! the store and its injected clock; [`Monitor::run`] repeats it between waits.

use std::collections::BTreeMap;

use crate::error::JobError;
use crate::feed::FeedLine;
use crate::model::{ActiveStatus, FinalStatus, Job, LeaseToken, Outcome, ProducerFamily, Status};
use crate::probe::NoProbe;
use crate::steps::Step;
use crate::store::{Acquire, Change, JobStore, JobUpdate, stopped_watching_outcome};

/// The latest-value key that says the last poll failed and is being retried.
pub const WATCH_STATE_KEY: &str = "watch";

/// One look at the external operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Poll {
    /// A conditional request reported no change since the previous poll.
    NotModified,
    Observed(Box<Observation>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Observed {
    /// Not started yet, for example a queued CI run.
    Waiting,
    Running,
    Succeeded(Outcome),
    Failed(Outcome),
    /// The external operation itself was cancelled, by anyone.
    Cancelled(Outcome),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    pub state: Observed,
    /// Replaces the step tree when present.
    pub steps: Option<Vec<Step>>,
    /// Absolute expected finish time, when the source can estimate one.
    pub eta_finish_at_ms: Option<u64>,
    pub latest: BTreeMap<String, String>,
    /// Published only when the Job's kind declares `open_url`.
    pub url: Option<String>,
    pub feed: Vec<FeedLine>,
}

impl Observation {
    pub fn new(state: Observed) -> Self {
        Self {
            state,
            steps: None,
            eta_finish_at_ms: None,
            latest: BTreeMap::new(),
            url: None,
            feed: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchError {
    /// Try again later: a network error, a rate limit, a server error. `retry_after_ms`
    /// is the provider's own delay when it gave one.
    Transient { retry_after_ms: Option<u64> },
    /// The external operation no longer exists. The Job fails.
    Gone,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CancelReply {
    /// The provider accepted the cancel. The Job is cancelled when a later poll sees it.
    Requested,
    /// This source or this operation cannot be cancelled by the user.
    Unsupported,
    /// The provider refused. The text is recorded as the Job's cancel failure.
    Failed(String),
}

/// A specialization that observes one kind of external operation.
pub trait WatchSource: Send {
    /// The Job kind name this source serves, for example `github-actions`.
    fn kind(&self) -> &str;

    /// Look at the operation named by `job.external`. A source that supports conditional
    /// requests keeps its validator per Job id and answers [`Poll::NotModified`].
    fn poll(&mut self, job: &Job, now_ms: u64) -> Result<Poll, WatchError>;

    /// Ask the provider to cancel the operation.
    fn cancel(&mut self, job: &Job) -> CancelReply;

    /// The monitor stopped serving this Job; drop anything cached for it.
    fn forget(&mut self, _job_id: &str) {}
}

/// Poll and lifetime bounds. The defaults are the shipped behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorConfig {
    /// Delay after a poll that saw a change.
    pub poll_ms: u64,
    /// Ceiling for the delay as unchanged polls back off (each adds half).
    pub max_poll_ms: u64,
    /// Ceiling for the delay as failed polls back off (each doubles).
    pub max_error_ms: u64,
    /// The monitor exits after this long with no Job to serve.
    pub idle_exit_ms: u64,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            poll_ms: 15_000,
            max_poll_ms: 120_000,
            max_error_ms: 300_000,
            idle_exit_ms: 30_000,
        }
    }
}

/// What one [`Monitor::tick`] left behind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tick {
    /// Jobs this monitor holds a lease on.
    pub watching: usize,
    /// When the next lease renewal or poll is due.
    pub next_wake_ms: u64,
    /// How long the monitor has had nothing to serve.
    pub idle_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wake {
    Continue,
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonitorExit {
    /// A stop was requested; the leases were released.
    Stopped,
    /// Nothing to serve for the idle window.
    Idle,
}

/// How [`Monitor::run`] waits between ticks. The process implementation wakes early on a
/// file change under the Job directory and reports a stop request.
pub trait MonitorControl {
    fn wait_until(&mut self, wake_ms: u64) -> Wake;
}

struct Watched {
    token: LeaseToken,
    next_poll_ms: u64,
    next_renew_ms: u64,
    interval_ms: u64,
    errors: u32,
    cancel_sent: bool,
}

type Key = (String, String);

pub struct Monitor {
    store: JobStore,
    owner: String,
    config: MonitorConfig,
    sources: Vec<Box<dyn WatchSource>>,
    watched: BTreeMap<Key, Watched>,
    idle_since_ms: Option<u64>,
}

impl Monitor {
    /// `owner` must be unique per monitor process: it is the lease owner on every Job.
    pub fn new(store: JobStore, owner: impl Into<String>, config: MonitorConfig) -> Self {
        Self {
            store,
            owner: owner.into(),
            config: MonitorConfig {
                poll_ms: config.poll_ms.max(1),
                ..config
            },
            sources: Vec::new(),
            watched: BTreeMap::new(),
            idle_since_ms: None,
        }
    }

    pub fn register(&mut self, source: Box<dyn WatchSource>) {
        self.sources.push(source);
    }

    /// Kind names this monitor can serve.
    pub fn kinds(&self) -> Vec<String> {
        self.sources
            .iter()
            .map(|source| source.kind().to_owned())
            .collect()
    }

    /// Tick until stopped or idle.
    pub fn run(&mut self, control: &mut dyn MonitorControl) -> Result<MonitorExit, JobError> {
        loop {
            let tick = self.tick()?;
            if tick.watching == 0 && tick.idle_ms >= self.config.idle_exit_ms {
                return Ok(MonitorExit::Idle);
            }
            if control.wait_until(tick.next_wake_ms) == Wake::Stop {
                self.release_all();
                return Ok(MonitorExit::Stopped);
            }
        }
    }

    /// One step: find Jobs to attach to, then serve each held Job (stop watching, cancel,
    /// poll, renew) as it falls due. Makes no store write when nothing is due.
    pub fn tick(&mut self) -> Result<Tick, JobError> {
        self.renew_overdue();
        self.attach()?;
        let keys: Vec<Key> = self.watched.keys().cloned().collect();
        for key in keys {
            if !self.serve(&key) {
                self.drop_job(&key);
            }
        }
        let now = self.store.now_ms();
        let idle_ms = if self.watched.is_empty() {
            now.saturating_sub(*self.idle_since_ms.get_or_insert(now))
        } else {
            self.idle_since_ms = None;
            0
        };
        let next_wake_ms = self
            .watched
            .values()
            .map(|watched| watched.next_poll_ms.min(watched.next_renew_ms))
            .min()
            .unwrap_or_else(|| {
                now.saturating_add(self.config.idle_exit_ms.saturating_sub(idle_ms).max(1))
            });
        Ok(Tick {
            watching: self.watched.len(),
            next_wake_ms,
            idle_ms,
        })
    }

    /// Give every lease up so the Jobs read as stalled at once and the next monitor can
    /// take them over without waiting for the lease to run out.
    pub fn release_all(&mut self) {
        let keys: Vec<Key> = self.watched.keys().cloned().collect();
        for key in keys {
            if let Some(watched) = self.watched.get(&key) {
                let _ = self.store.release(&key.0, &key.1, &watched.token);
            }
            self.drop_job(&key);
        }
    }

    fn renew_ms(&self) -> u64 {
        (self.store.config().lease_ms / 3).max(1)
    }

    fn source_index(&self, kind: &str) -> Option<usize> {
        self.sources.iter().position(|source| source.kind() == kind)
    }

    fn drop_job(&mut self, key: &Key) {
        self.watched.remove(key);
        for source in &mut self.sources {
            source.forget(&key.1);
        }
    }

    /// Renew held leases that are due before anything else. After a long pause (a
    /// sleeping laptop) the monitor's own leases have run out; renewing first keeps the
    /// reconciliation below from declaring its own Jobs lost.
    fn renew_overdue(&mut self) {
        let now = self.store.now_ms();
        let renew_ms = self.renew_ms();
        let due: Vec<Key> = self
            .watched
            .iter()
            .filter(|(_, watched)| now >= watched.next_renew_ms)
            .map(|(key, _)| key.clone())
            .collect();
        for key in due {
            let Some(token) = self.watched.get(&key).map(|w| w.token.clone()) else {
                continue;
            };
            if self.store.renew(&key.0, &key.1, &token).is_ok() {
                self.touch(&key, now, renew_ms);
            } else {
                self.drop_job(&key);
            }
        }
    }

    fn attach(&mut self) -> Result<(), JobError> {
        for checkout in self.store.checkouts()? {
            // Expired producers become lost here, which is also what makes a dead
            // monitor's Jobs available again. One unreadable checkout must not stop the rest.
            let _ = self.store.reconcile(&checkout, &NoProbe);
            let Ok(listing) = self.store.list(&checkout) else {
                continue;
            };
            let now = self.store.now_ms();
            for job in listing.jobs {
                let key = (checkout.clone(), job.id.clone());
                if job.kind.family != ProducerFamily::Watch
                    || self.source_index(&job.kind.name).is_none()
                    || self.watched.contains_key(&key)
                {
                    continue;
                }
                let free = match job.status {
                    Status::Queued | Status::Lost => true,
                    Status::Running | Status::Waiting => job
                        .lease
                        .as_ref()
                        .is_none_or(|lease| now >= lease.expires_ms),
                    _ => false,
                };
                // Without a recorded external operation there is no identity to verify.
                let Some(external) = job.external.clone().filter(|_| free) else {
                    continue;
                };
                let status = if job.status == Status::Running {
                    ActiveStatus::Running
                } else {
                    ActiveStatus::Waiting
                };
                let request = Acquire {
                    owner: self.owner.clone(),
                    status,
                    launch_id: None,
                    external: Some(external),
                };
                if let Ok((_, token)) = self.store.acquire(&checkout, &job.id, request) {
                    self.watched.insert(
                        key,
                        Watched {
                            token,
                            next_poll_ms: now,
                            next_renew_ms: now.saturating_add(self.renew_ms()),
                            interval_ms: self.config.poll_ms,
                            errors: 0,
                            cancel_sent: false,
                        },
                    );
                }
            }
        }
        Ok(())
    }

    /// Serve one held Job. `false` means this monitor no longer holds it.
    fn serve(&mut self, key: &Key) -> bool {
        let (checkout, id) = (key.0.as_str(), key.1.as_str());
        let Ok(job) = self.store.get(checkout, id) else {
            return false;
        };
        let Some(token) = self.watched.get(key).map(|watched| watched.token.clone()) else {
            return false;
        };
        if !job.status.is_active() || job.token().as_ref() != Some(&token) {
            return false;
        }
        let Some(source) = self.source_index(&job.kind.name) else {
            return false;
        };
        if job.stop_watching_requested_ms.is_some() {
            let _ = self
                .store
                .confirm_cancel(checkout, id, &token, stopped_watching_outcome());
            return false;
        }
        let now = self.store.now_ms();
        let renew_ms = self.renew_ms();
        let cancel_sent = self.watched.get(key).is_some_and(|w| w.cancel_sent);
        if job.cancel_requested_ms.is_some() && !cancel_sent {
            let failure = match self.sources[source].cancel(&job) {
                CancelReply::Requested => None,
                CancelReply::Unsupported => {
                    Some("This operation cannot be cancelled from here".to_owned())
                }
                CancelReply::Failed(reason) => Some(reason),
            };
            match failure {
                None => {
                    if let Some(watched) = self.watched.get_mut(key) {
                        watched.cancel_sent = true;
                        watched.next_poll_ms = now;
                    }
                }
                Some(reason) => {
                    let update = JobUpdate {
                        cancel_failed: Some(bounded(&reason)),
                        ..JobUpdate::default()
                    };
                    if self.store.update(checkout, id, &token, update).is_err() {
                        return false;
                    }
                    self.touch(key, now, renew_ms);
                }
            }
        } else if job.cancel_requested_ms.is_none() && cancel_sent {
            // The earlier request was answered; a later one is sent again.
            if let Some(watched) = self.watched.get_mut(key) {
                watched.cancel_sent = false;
            }
        }

        let (poll_due, renew_due) = self.watched.get(key).map_or((false, false), |w| {
            (now >= w.next_poll_ms, now >= w.next_renew_ms)
        });
        if poll_due {
            let result = self.sources[source].poll(&job, now);
            return self.apply(key, &job, &token, result);
        }
        if renew_due {
            if self.store.renew(checkout, id, &token).is_err() {
                return false;
            }
            self.touch(key, now, renew_ms);
        }
        true
    }

    fn touch(&mut self, key: &Key, now: u64, renew_ms: u64) {
        if let Some(watched) = self.watched.get_mut(key) {
            watched.next_renew_ms = now.saturating_add(renew_ms);
        }
    }

    fn apply(
        &mut self,
        key: &Key,
        job: &Job,
        token: &LeaseToken,
        result: Result<Poll, WatchError>,
    ) -> bool {
        let (checkout, id) = (key.0.as_str(), key.1.as_str());
        let now = self.store.now_ms();
        let renew_ms = self.renew_ms();
        let config = self.config;
        let retrying = job.latest.contains_key(WATCH_STATE_KEY);
        let Some(watched) = self.watched.get_mut(key) else {
            return false;
        };
        let observation = match result {
            Ok(Poll::Observed(observation)) => *observation,
            Ok(Poll::NotModified) => {
                watched.errors = 0;
                watched.interval_ms = watched
                    .interval_ms
                    .saturating_add(watched.interval_ms / 2)
                    .clamp(config.poll_ms, config.max_poll_ms.max(config.poll_ms));
                watched.next_poll_ms = now.saturating_add(watched.interval_ms);
                if !retrying && now < watched.next_renew_ms {
                    return true;
                }
                let update = JobUpdate {
                    latest: BTreeMap::from([(WATCH_STATE_KEY.to_owned(), None)]),
                    ..JobUpdate::default()
                };
                let renewed = self.store.update(checkout, id, token, update).is_ok();
                self.touch(key, now, renew_ms);
                return renewed;
            }
            Err(WatchError::Gone) => {
                let outcome = Outcome {
                    summary: Some("The watched operation no longer exists".into()),
                    refs: BTreeMap::from([(WATCH_STATE_KEY.to_owned(), "gone".to_owned())]),
                };
                let _ = self
                    .store
                    .finish(checkout, id, token, FinalStatus::Failed, outcome);
                return false;
            }
            Err(WatchError::Transient { retry_after_ms }) => {
                watched.errors = watched.errors.saturating_add(1);
                let doubled = config
                    .poll_ms
                    .saturating_mul(1_u64 << watched.errors.min(20))
                    .min(config.max_error_ms.max(config.poll_ms));
                watched.next_poll_ms = now.saturating_add(doubled.max(retry_after_ms.unwrap_or(0)));
                if retrying && now < watched.next_renew_ms {
                    return true;
                }
                // The first failure is recorded once; it also renews the lease, because
                // the monitor itself is alive.
                let update = JobUpdate {
                    latest: BTreeMap::from([(
                        WATCH_STATE_KEY.to_owned(),
                        Some("retrying".to_owned()),
                    )]),
                    ..JobUpdate::default()
                };
                let renewed = self.store.update(checkout, id, token, update).is_ok();
                self.touch(key, now, renew_ms);
                return renewed;
            }
        };

        watched.errors = 0;
        watched.interval_ms = config.poll_ms;
        watched.next_poll_ms = now.saturating_add(config.poll_ms);
        watched.next_renew_ms = now.saturating_add(renew_ms);

        let mut latest: BTreeMap<String, Option<String>> = observation
            .latest
            .into_iter()
            .map(|(key, value)| (key, Some(value)))
            .collect();
        latest.insert(WATCH_STATE_KEY.to_owned(), None);
        let status = match &observation.state {
            Observed::Waiting => Some(ActiveStatus::Waiting),
            Observed::Running => Some(ActiveStatus::Running),
            _ => None,
        };
        let update = JobUpdate {
            status,
            steps: observation.steps,
            eta: observation
                .eta_finish_at_ms
                .map_or(Change::Clear, Change::Set),
            latest,
            feed: observation.feed,
            url: observation.url.filter(|_| job.capabilities.open_url),
            ..JobUpdate::default()
        };
        if self.store.update(checkout, id, token, update).is_err() {
            return false;
        }
        match observation.state {
            Observed::Waiting | Observed::Running => true,
            Observed::Succeeded(outcome) => {
                let _ = self
                    .store
                    .finish(checkout, id, token, FinalStatus::Succeeded, outcome);
                false
            }
            Observed::Failed(outcome) => {
                let _ = self
                    .store
                    .finish(checkout, id, token, FinalStatus::Failed, outcome);
                false
            }
            Observed::Cancelled(outcome) => {
                let _ = self.store.confirm_cancel(checkout, id, token, outcome);
                false
            }
        }
    }
}

/// A provider's refusal text, cut to one bounded line for the record.
fn bounded(reason: &str) -> String {
    let line: String = reason
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(200)
        .collect();
    let line = line.trim();
    if line.is_empty() {
        "The provider refused the cancel".to_owned()
    } else {
        line.to_owned()
    }
}
