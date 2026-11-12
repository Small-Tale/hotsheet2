//! Shared fixtures: a store on a temp home with a hand-moved clock. Synthetic data only.
#![allow(
    dead_code,
    reason = "HS2-GCHX8V: each test binary uses a different subset"
)]

use std::sync::Arc;

use hotsheet_jobs::{
    Acquire, ActiveStatus, Capabilities, ExternalRef, FinalStatus, Job, JobKind, JobStore,
    JobUpdate, LeaseToken, ManualClock, NewJob, NoProbe, Outcome, ProducerFamily, SequenceIds,
    Status, StoreConfig,
};

pub const CHECKOUT: &str = "demo-0123456789ab";
pub const LEASE_MS: u64 = 1_000;
pub const LOST_AFTER_MS: u64 = 5_000;

pub struct Fixture {
    pub home: tempfile::TempDir,
    pub clock: Arc<ManualClock>,
    pub store: JobStore,
}

pub fn config() -> StoreConfig {
    StoreConfig {
        lease_ms: LEASE_MS,
        lost_after_ms: LOST_AFTER_MS,
        retain_succeeded_ms: 60_000,
        retain_failed_ms: 600_000,
        max_jobs: 8,
        max_feed_bytes: 16 * 1024,
    }
}

pub fn fixture() -> Fixture {
    fixture_with(config())
}

pub fn fixture_with(config: StoreConfig) -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::at(1_000_000));
    let store = JobStore::with(
        home.path(),
        clock.clone(),
        Arc::new(SequenceIds::default()),
        config,
    );
    Fixture { home, clock, store }
}

pub fn external() -> ExternalRef {
    ExternalRef {
        system: "fake-ci".into(),
        id: "run-1".into(),
    }
}

pub fn new_job(family: ProducerFamily) -> NewJob {
    NewJob {
        checkout_id: CHECKOUT.into(),
        kind: JobKind {
            family,
            name: "fixture".into(),
        },
        job_key: "fixture:key".into(),
        title: "Synthetic job".into(),
        capabilities: Capabilities {
            cancel: true,
            retry: true,
            open_url: true,
        },
        ticket: None,
        external: (family == ProducerFamily::Watch).then(external),
        retry_of: None,
        url: None,
    }
}

pub fn acquire(owner: &str, status: ActiveStatus) -> Acquire {
    Acquire {
        owner: owner.into(),
        status,
        launch_id: None,
        external: Some(external()),
    }
}

impl Fixture {
    pub fn create(&self, family: ProducerFamily) -> Job {
        self.store.create(new_job(family)).unwrap()
    }

    pub fn start(&self, id: &str, owner: &str) -> LeaseToken {
        self.store
            .acquire(CHECKOUT, id, acquire(owner, ActiveStatus::Running))
            .unwrap()
            .1
    }

    pub fn get(&self, id: &str) -> Job {
        self.store.get(CHECKOUT, id).unwrap()
    }

    /// Let the lease expire and the grace window pass, then reconcile.
    pub fn lose(&self) {
        self.clock.advance(LEASE_MS + LOST_AFTER_MS);
        self.store.reconcile(CHECKOUT, &NoProbe).unwrap();
    }

    /// Drive a fresh Job of `family` into `status`. Returns the Job and the token of the
    /// lease it last held, when it ever held one.
    pub fn job_in(&self, family: ProducerFamily, status: Status) -> (Job, Option<LeaseToken>) {
        let job = self.create(family);
        if status == Status::Queued {
            return (job, None);
        }
        let token = self.start(&job.id, "producer-a");
        match status {
            Status::Queued | Status::Running => {}
            Status::Waiting => {
                let update = JobUpdate {
                    status: Some(ActiveStatus::Waiting),
                    ..JobUpdate::default()
                };
                self.store
                    .update(CHECKOUT, &job.id, &token, update)
                    .unwrap();
            }
            Status::Succeeded | Status::Failed => {
                let outcome = if status == Status::Succeeded {
                    FinalStatus::Succeeded
                } else {
                    FinalStatus::Failed
                };
                self.store
                    .finish(CHECKOUT, &job.id, &token, outcome, Outcome::default())
                    .unwrap();
            }
            Status::Cancelled => {
                self.store
                    .confirm_cancel(CHECKOUT, &job.id, &token, Outcome::default())
                    .unwrap();
            }
            Status::Lost => self.lose(),
        }
        let job = self.get(&job.id);
        assert_eq!(job.status, status);
        (job, Some(token))
    }
}
