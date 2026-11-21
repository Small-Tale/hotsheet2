//! HS2-GCHX8V: several writers at once, as threads and as separate OS processes, must
//! leave one ordered state machine and a strictly increasing feed sequence.

mod common;

use std::collections::BTreeSet;
use std::process::Command;
use std::sync::{Arc, Barrier};

use common::{CHECKOUT, acquire, fixture, new_job};
use hotsheet_jobs::{
    ActiveStatus, FeedLine, FinalStatus, JobError, JobStore, JobUpdate, Outcome, ProducerFamily,
    Status,
};

const CHILD_HOME: &str = "HOTSHEET_JOBS_TEST_CHILD_HOME";
const CHILD_JOB: &str = "HOTSHEET_JOBS_TEST_CHILD_JOB";
const CHILD_NAME: &str = "HOTSHEET_JOBS_TEST_CHILD_NAME";
const LINES_PER_WRITER: usize = 40;

fn append_lines(store: &JobStore, job_id: &str, name: &str) {
    let token = store.get(CHECKOUT, job_id).unwrap().token().unwrap();
    for index in 0..LINES_PER_WRITER {
        let update = JobUpdate {
            feed: vec![FeedLine::out(format!("{name} {index}"))],
            ..JobUpdate::default()
        };
        store.update(CHECKOUT, job_id, &token, update).unwrap();
    }
}

fn assert_feed_is_complete(store: &JobStore, job_id: &str, writers: &[String]) {
    let job = store.get(CHECKOUT, job_id).unwrap();
    let total = (writers.len() * LINES_PER_WRITER) as u64;
    assert_eq!(job.feed.next_seq, total + 1);
    let page = store.read_feed(CHECKOUT, job_id, 0, usize::MAX).unwrap();
    assert_eq!(page.gap, None);
    let seqs: Vec<u64> = page.entries.iter().map(|entry| entry.seq).collect();
    assert_eq!(seqs, (1..=total).collect::<Vec<u64>>());
    let texts: BTreeSet<&str> = page
        .entries
        .iter()
        .map(|entry| entry.text.as_str())
        .collect();
    assert_eq!(texts.len() as u64, total, "no line was lost or duplicated");
    for writer in writers {
        // Each writer's own lines keep their order.
        let own: Vec<usize> = page
            .entries
            .iter()
            .filter_map(|entry| entry.text.strip_prefix(&format!("{writer} ")))
            .map(|index| index.parse().unwrap())
            .collect();
        assert_eq!(own, (0..LINES_PER_WRITER).collect::<Vec<usize>>());
    }
}

#[test]
fn threads_appending_to_one_job_keep_one_monotonic_feed() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Reported);
    fx.start(&job.id, "producer-a");
    let before = fx.store.revision(CHECKOUT).unwrap();
    let writers: Vec<String> = (0..6).map(|index| format!("thread-{index}")).collect();
    let barrier = Arc::new(Barrier::new(writers.len()));
    let handles: Vec<_> = writers
        .iter()
        .map(|name| {
            let (store, id, name) = (fx.store.clone(), job.id.clone(), name.clone());
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                append_lines(&store, &id, &name);
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_feed_is_complete(&fx.store, &job.id, &writers);
    let writes = (writers.len() * LINES_PER_WRITER) as u64;
    assert_eq!(fx.store.revision(CHECKOUT).unwrap(), before + writes);
    assert_eq!(fx.get(&job.id).revision, job.revision + 1 + writes);
}

#[test]
fn racing_producers_get_exactly_one_lease_and_one_outcome() {
    let fx = fixture();
    let job = fx.create(ProducerFamily::Owned);
    let contenders = 8;
    let barrier = Arc::new(Barrier::new(contenders));
    let handles: Vec<_> = (0..contenders)
        .map(|index| {
            let (store, id, barrier) = (fx.store.clone(), job.id.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let owner = format!("producer-{index}");
                let token =
                    match store.acquire(CHECKOUT, &id, acquire(&owner, ActiveStatus::Running)) {
                        Ok((_, token)) => token,
                        Err(JobError::LeaseHeld | JobError::InvalidTransition { .. }) => {
                            return None;
                        }
                        Err(other) => panic!("unexpected {other}"),
                    };
                let outcome = Outcome {
                    summary: Some(owner.clone()),
                    ..Outcome::default()
                };
                store
                    .finish(CHECKOUT, &id, &token, FinalStatus::Succeeded, outcome)
                    .unwrap();
                Some(owner)
            })
        })
        .collect();
    let winners: Vec<String> = handles
        .into_iter()
        .filter_map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(winners.len(), 1);
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Succeeded);
    assert_eq!(done.outcome.unwrap().summary, Some(winners[0].clone()));
    assert_eq!(done.lease.unwrap().generation, 1);
}

/// HS2-JSF985 on Windows: a monitor judged a watch Job while the server held the Job's
/// snapshot and the checkout revision open, and `acquire` failed with `Access is denied`
/// because the old publication renamed with `MoveFileExW`. Every transition must succeed
/// while readers keep both files open, and those readers keep the bytes they opened.
#[test]
fn hs2_jsf985_transitions_publish_while_readers_hold_the_snapshot_and_revision_open() {
    use std::io::Read;
    let fx = fixture();
    let job = fx.create(ProducerFamily::Watch);
    let checkout = fx.home.path().join("jobs").join(CHECKOUT);
    let snapshot = checkout.join(&job.id).join("job.json");
    let mut held_snapshot = std::fs::File::open(&snapshot).unwrap();
    let mut held_revision = std::fs::File::open(checkout.join("revision")).unwrap();
    let token = fx.start(&job.id, "monitor:test");
    let update = JobUpdate {
        feed: vec![FeedLine::out("watching".to_string())],
        ..JobUpdate::default()
    };
    fx.store.update(CHECKOUT, &job.id, &token, update).unwrap();
    fx.store
        .finish(
            CHECKOUT,
            &job.id,
            &token,
            FinalStatus::Failed,
            Outcome::default(),
        )
        .unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Failed);
    let mut old = String::new();
    held_snapshot.read_to_string(&mut old).unwrap();
    assert!(old.contains("\"queued\""), "{old}");
    let mut revision = String::new();
    held_revision.read_to_string(&mut revision).unwrap();
    assert!(!revision.is_empty());
}

#[test]
fn concurrent_creates_get_distinct_jobs_within_the_cap() {
    let fx = fixture();
    let cap = fx.store.config().max_jobs;
    let barrier = Arc::new(Barrier::new(cap * 2));
    let handles: Vec<_> = (0..cap * 2)
        .map(|_| {
            let (store, barrier) = (fx.store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.create(new_job(ProducerFamily::Reported))
            })
        })
        .collect();
    let mut created = BTreeSet::new();
    let mut full = 0;
    for handle in handles {
        match handle.join().unwrap() {
            Ok(job) => assert!(created.insert(job.id)),
            Err(JobError::CheckoutFull) => full += 1,
            Err(other) => panic!("unexpected {other}"),
        }
    }
    assert_eq!((created.len(), full), (cap, cap));
    assert_eq!(fx.store.list(CHECKOUT).unwrap().jobs.len(), cap);
    assert_eq!(fx.store.revision(CHECKOUT).unwrap(), cap as u64);
}

/// Not a test of its own: the body of one writer process for the test below. It runs
/// only when the parent re-executes this test binary with the child variables set.
#[test]
fn child_process_writer() {
    let (Ok(home), Ok(job_id), Ok(name)) = (
        std::env::var(CHILD_HOME),
        std::env::var(CHILD_JOB),
        std::env::var(CHILD_NAME),
    ) else {
        return;
    };
    // A second process opens the same home with its own handle and the wall clock.
    append_lines(&JobStore::at(home), &job_id, &name);
}

#[test]
fn separate_processes_appending_to_one_job_keep_one_monotonic_feed() {
    let home = tempfile::tempdir().unwrap();
    let store = JobStore::at(home.path());
    let job = store.create(new_job(ProducerFamily::Reported)).unwrap();
    store
        .acquire(
            CHECKOUT,
            &job.id,
            acquire("producer-a", ActiveStatus::Running),
        )
        .unwrap();
    let writers: Vec<String> = (0..4).map(|index| format!("process-{index}")).collect();
    let children: Vec<_> = writers
        .iter()
        .map(|name| {
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "child_process_writer", "--nocapture"])
                .env(CHILD_HOME, home.path())
                .env(CHILD_JOB, &job.id)
                .env(CHILD_NAME, name)
                .spawn()
                .unwrap()
        })
        .collect();
    // The parent writes too, so in-process and cross-process locking interleave.
    append_lines(&store, &job.id, "parent");
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let mut all = writers.clone();
    all.push("parent".into());
    assert_feed_is_complete(&store, &job.id, &all);
    let leftovers: Vec<_> = std::fs::read_dir(store.checkout_dir(CHECKOUT).unwrap().join(&job.id))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}
