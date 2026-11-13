//! HS2-5TC1GQ: the monitor's step function against the real store, a hand-moved clock and
//! a scripted source. Process lifecycle is covered through the built CLI in
//! `crates/hotsheet-cli/tests/job_monitor.rs`.

mod common;

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use common::{CHECKOUT, Fixture, LEASE_MS, LOST_AFTER_MS, external, fixture, new_job};
use hotsheet_jobs::{
    CancelReply, Capabilities, ExternalRef, FIXTURE_KIND, FeedLine, FixtureWatchSource, Job,
    Liveness, Monitor, MonitorConfig, MonitorControl, MonitorExit, NoProbe, Observation, Observed,
    Outcome, Poll, ProducerFamily, Status, Step, StepStatus, WATCH_STATE_KEY, Wake, WatchError,
    WatchSource,
};

const POLL_MS: u64 = 100;
const RENEW_MS: u64 = LEASE_MS / 3;

#[derive(Default)]
struct Script {
    replies: VecDeque<Result<Poll, WatchError>>,
    polls: Vec<u64>,
    cancels: usize,
    cancel_reply: Option<CancelReply>,
    forgotten: Vec<String>,
}

/// Answers polls from a queue; an empty queue answers "not modified".
#[derive(Clone, Default)]
struct Scripted(Arc<Mutex<Script>>);

impl Scripted {
    fn push(&self, reply: Result<Poll, WatchError>) {
        self.0.lock().unwrap().replies.push_back(reply);
    }

    fn observe(&self, state: Observed) {
        self.push(Ok(Poll::Observed(Box::new(Observation::new(state)))));
    }

    fn polls(&self) -> Vec<u64> {
        self.0.lock().unwrap().polls.clone()
    }

    fn cancels(&self) -> usize {
        self.0.lock().unwrap().cancels
    }

    fn reply_to_cancel(&self, reply: CancelReply) {
        self.0.lock().unwrap().cancel_reply = Some(reply);
    }
}

impl WatchSource for Scripted {
    fn kind(&self) -> &str {
        "fixture"
    }

    fn poll(&mut self, _job: &Job, now_ms: u64) -> Result<Poll, WatchError> {
        let mut script = self.0.lock().unwrap();
        script.polls.push(now_ms);
        script.replies.pop_front().unwrap_or(Ok(Poll::NotModified))
    }

    fn cancel(&mut self, _job: &Job) -> CancelReply {
        let mut script = self.0.lock().unwrap();
        script.cancels += 1;
        script
            .cancel_reply
            .clone()
            .unwrap_or(CancelReply::Unsupported)
    }

    fn forget(&mut self, job_id: &str) {
        self.0.lock().unwrap().forgotten.push(job_id.to_owned());
    }
}

fn monitor_config() -> MonitorConfig {
    MonitorConfig {
        poll_ms: POLL_MS,
        max_poll_ms: 400,
        max_error_ms: 1_600,
        idle_exit_ms: 2_000,
    }
}

fn monitor(fx: &Fixture, owner: &str, source: &Scripted) -> Monitor {
    let mut monitor = Monitor::new(fx.store.clone(), owner, monitor_config());
    monitor.register(Box::new(source.clone()));
    monitor
}

fn outcome(text: &str) -> Outcome {
    Outcome {
        summary: Some(text.into()),
        refs: BTreeMap::new(),
    }
}

/// Moves the manual clock to each requested wake time and stops after a set number.
struct Stepper<'a> {
    fx: &'a Fixture,
    stop_after: Option<usize>,
    waits: usize,
}

impl MonitorControl for Stepper<'_> {
    fn wait_until(&mut self, wake_ms: u64) -> Wake {
        self.waits += 1;
        if self.stop_after.is_some_and(|limit| self.waits > limit) {
            return Wake::Stop;
        }
        assert!(self.waits < 10_000, "the monitor never finished");
        self.fx.clock.set(wake_ms.max(self.fx.store.now_ms()));
        Wake::Continue
    }
}

#[test]
fn it_attaches_to_a_queued_watch_job_and_reports_what_it_observes() {
    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    let mut steps = Step::new("build", "Build");
    steps.status = StepStatus::Active;
    source.push(Ok(Poll::Observed(Box::new(Observation {
        steps: Some(vec![steps.clone()]),
        eta_finish_at_ms: Some(2_000_000),
        latest: BTreeMap::from([("current_step".to_owned(), "Build".to_owned())]),
        url: Some("https://ci.example.test/run/1".into()),
        feed: vec![FeedLine::info("run started")],
        ..Observation::new(Observed::Running)
    }))));

    let mut monitor = monitor(&fx, "monitor-a", &source);
    assert_eq!(monitor.kinds(), ["fixture"]);
    let tick = monitor.tick().unwrap();
    assert_eq!((tick.watching, tick.idle_ms), (1, 0));
    assert_eq!(tick.next_wake_ms, fx.store.now_ms() + POLL_MS);

    let job = fx.get(&job.id);
    assert_eq!(job.status, Status::Running);
    let lease = job.lease.clone().unwrap();
    assert_eq!((lease.owner.as_str(), lease.generation), ("monitor-a", 1));
    assert_eq!(job.steps, [steps]);
    assert_eq!(job.eta().unwrap().finish_at_ms, 2_000_000);
    assert_eq!(job.latest["current_step"], "Build");
    assert_eq!(job.url.as_deref(), Some("https://ci.example.test/run/1"));
    let feed = fx.store.read_feed(CHECKOUT, &job.id, 0, 10).unwrap();
    assert_eq!(feed.entries[0].text, "run started");
    assert_eq!(source.polls().len(), 1);
}

#[test]
fn a_url_is_published_only_for_a_kind_that_declares_it() {
    let fx = fixture();
    let source = Scripted::default();
    let mut plain = new_job(ProducerFamily::Watch);
    plain.capabilities = Capabilities::default();
    let job = fx.store.create(plain).unwrap();
    source.push(Ok(Poll::Observed(Box::new(Observation {
        url: Some("https://ci.example.test/run/1".into()),
        ..Observation::new(Observed::Running)
    }))));
    monitor(&fx, "monitor-a", &source).tick().unwrap();
    let job = fx.get(&job.id);
    assert_eq!((job.status, job.url), (Status::Running, None));
}

#[test]
fn it_serves_only_watch_jobs_of_kinds_it_has_a_source_for() {
    let fx = fixture();
    let source = Scripted::default();
    let owned = fx.create(ProducerFamily::Owned);
    let reported = fx.create(ProducerFamily::Reported);
    let mut other_kind = new_job(ProducerFamily::Watch);
    other_kind.kind.name = "some-other-ci".into();
    let other_kind = fx.store.create(other_kind).unwrap();
    let mut anonymous = new_job(ProducerFamily::Watch);
    anonymous.external = None;
    let anonymous = fx.store.create(anonymous).unwrap();
    let (finished, _) = fx.job_in(ProducerFamily::Watch, Status::Succeeded);

    let before = fx.store.revision(CHECKOUT).unwrap();
    let mut monitor = monitor(&fx, "monitor-a", &source);
    let tick = monitor.tick().unwrap();
    assert_eq!(tick.watching, 0);
    assert_eq!(
        fx.store.revision(CHECKOUT).unwrap(),
        before,
        "nothing written"
    );
    for id in [&owned.id, &reported.id, &other_kind.id, &anonymous.id] {
        assert_eq!(fx.get(id).status, Status::Queued);
    }
    assert_eq!(fx.get(&finished.id).status, Status::Succeeded);
    assert!(source.polls().is_empty());
}

#[test]
fn it_renews_the_lease_between_polls_and_writes_nothing_when_nothing_is_due() {
    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Waiting);
    let mut monitor = Monitor::new(
        fx.store.clone(),
        "monitor-a",
        MonitorConfig {
            poll_ms: 10 * LEASE_MS,
            max_poll_ms: 10 * LEASE_MS,
            ..monitor_config()
        },
    );
    monitor.register(Box::new(source.clone()));
    let first = monitor.tick().unwrap();
    assert_eq!(first.next_wake_ms, fx.store.now_ms() + RENEW_MS);
    let attached = fx.get(&job.id);

    // Ticks before anything is due change nothing on disk.
    for _ in 0..3 {
        fx.clock.advance(RENEW_MS / 4);
        monitor.tick().unwrap();
        assert_eq!(fx.get(&job.id), attached);
    }
    // The poll interval is far longer than the lease: renewals alone keep the Job live.
    for _ in 0..12 {
        fx.clock.advance(RENEW_MS);
        let tick = monitor.tick().unwrap();
        assert_eq!(tick.watching, 1);
        let job = fx.get(&job.id);
        assert_eq!(job.liveness(fx.store.now_ms()), Liveness::Live);
        assert_eq!(job.lease.unwrap().expires_ms, fx.store.now_ms() + LEASE_MS);
    }
    assert_eq!(source.polls().len(), 1, "renewals alone, no extra poll");
    assert!(
        fx.store
            .reconcile(CHECKOUT, &NoProbe)
            .unwrap()
            .lost
            .is_empty()
    );
}

#[test]
fn unchanged_polls_back_off_to_a_ceiling_and_a_change_resets_the_interval() {
    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Waiting);
    let mut monitor = monitor(&fx, "monitor-a", &source);
    let start = fx.store.now_ms();
    let mut control = Stepper {
        fx: &fx,
        stop_after: Some(60),
        waits: 0,
    };
    // Six unchanged polls, then a change, then unchanged again.
    for _ in 0..6 {
        source.push(Ok(Poll::NotModified));
    }
    source.observe(Observed::Running);
    assert_eq!(monitor.run(&mut control).unwrap(), MonitorExit::Stopped);

    let polls: Vec<u64> = source.polls().iter().map(|at| at - start).collect();
    let gaps: Vec<u64> = polls.windows(2).map(|pair| pair[1] - pair[0]).collect();
    // 100 after the first observation, then +50% each time up to the 400 ceiling,
    // back to 100 after the change, and growing again.
    assert_eq!(
        &gaps[..10],
        [100, 150, 225, 337, 400, 400, 400, 100, 150, 225]
    );
    assert_eq!(fx.get(&job.id).status, Status::Running);
}

#[test]
fn failed_polls_back_off_keep_the_lease_and_clear_when_the_source_recovers() {
    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Running);
    for _ in 0..6 {
        source.push(Err(WatchError::Transient {
            retry_after_ms: None,
        }));
    }
    source.push(Err(WatchError::Transient {
        retry_after_ms: Some(5_000),
    }));
    source.observe(Observed::Running);
    let mut monitor = monitor(&fx, "monitor-a", &source);
    let start = fx.store.now_ms();
    let mut marked = false;
    while source.polls().len() < 9 {
        let tick = monitor.tick().unwrap();
        assert_eq!(tick.watching, 1);
        let job = fx.get(&job.id);
        // The monitor is alive throughout: the Job never stalls while polls fail.
        assert_eq!(job.liveness(fx.store.now_ms()), Liveness::Live);
        assert_eq!(job.status, Status::Running);
        marked |= job.latest.get(WATCH_STATE_KEY).map(String::as_str) == Some("retrying");
        fx.clock.set(tick.next_wake_ms);
    }
    assert!(marked, "the retry state was visible while polls failed");
    let polls: Vec<u64> = source.polls().iter().map(|at| at - start).collect();
    let gaps: Vec<u64> = polls.windows(2).map(|pair| pair[1] - pair[0]).collect();
    // Doubling from the base, capped at 1600, and the provider's own delay wins when longer.
    assert_eq!(gaps, [100, 200, 400, 800, 1_600, 1_600, 1_600, 5_000]);
    let job = fx.get(&job.id);
    assert!(!job.latest.contains_key(WATCH_STATE_KEY));
    assert_eq!(job.lease.unwrap().generation, 1);
}

#[test]
fn an_operation_that_no_longer_exists_fails_the_job() {
    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    source.push(Err(WatchError::Gone));
    let mut monitor = monitor(&fx, "monitor-a", &source);
    assert_eq!(monitor.tick().unwrap().watching, 0);
    let job = fx.get(&job.id);
    assert_eq!(job.status, Status::Failed);
    assert_eq!(job.outcome.unwrap().refs[WATCH_STATE_KEY], "gone");
}

#[test]
fn outcomes_finish_the_job_and_the_monitor_exits_when_idle() {
    let fx = fixture();
    let source = Scripted::default();
    let passed = fx.create(ProducerFamily::Watch);
    let mut second = new_job(ProducerFamily::Watch);
    second.checkout_id = "other-ba9876543210".into();
    let failed = fx.store.create(second).unwrap();
    // One monitor serves both checkouts; polls alternate between the two Jobs.
    source.observe(Observed::Waiting);
    source.observe(Observed::Waiting);
    source.observe(Observed::Succeeded(outcome("green")));
    source.observe(Observed::Failed(outcome("red")));

    let mut monitor = monitor(&fx, "monitor-a", &source);
    let started = fx.store.now_ms();
    let mut control = Stepper {
        fx: &fx,
        stop_after: None,
        waits: 0,
    };
    assert_eq!(monitor.run(&mut control).unwrap(), MonitorExit::Idle);

    let passed = fx.get(&passed.id);
    assert_eq!(passed.status, Status::Succeeded);
    assert_eq!(passed.outcome, Some(outcome("green")));
    let failed = fx.store.get("other-ba9876543210", &failed.id).unwrap();
    assert_eq!(failed.status, Status::Failed);
    assert_eq!(failed.outcome, Some(outcome("red")));
    assert_eq!(source.polls().len(), 4, "no poll after the outcomes");
    // It stayed for the idle window after the last Job finished, then left.
    assert_eq!(fx.store.now_ms(), started + POLL_MS + 2_000);
    let forgotten = source.0.lock().unwrap().forgotten.clone();
    assert!(forgotten.contains(&passed.id) && forgotten.contains(&failed.id));
}

#[test]
fn an_idle_monitor_picks_up_a_job_created_before_the_window_ends() {
    let fx = fixture();
    let source = Scripted::default();
    let mut monitor = monitor(&fx, "monitor-a", &source);
    let tick = monitor.tick().unwrap();
    assert_eq!((tick.watching, tick.idle_ms), (0, 0));
    assert_eq!(tick.next_wake_ms, fx.store.now_ms() + 2_000);
    fx.clock.advance(1_500);
    assert_eq!(monitor.tick().unwrap().idle_ms, 1_500);

    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Running);
    let tick = monitor.tick().unwrap();
    assert_eq!((tick.watching, tick.idle_ms), (1, 0));
    assert_eq!(fx.get(&job.id).status, Status::Running);
}

#[test]
fn a_cancel_is_sent_once_and_lands_when_the_source_reports_it() {
    let fx = fixture();
    let source = Scripted::default();
    source.reply_to_cancel(CancelReply::Requested);
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Running);
    let mut monitor = monitor(&fx, "monitor-a", &source);
    monitor.tick().unwrap();

    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    // The request is not a transition: the Job keeps running until the provider says so.
    source.observe(Observed::Running);
    monitor.tick().unwrap();
    assert_eq!(source.cancels(), 1);
    assert_eq!(
        source.polls().len(),
        2,
        "a cancel is followed by a prompt poll"
    );
    assert_eq!(fx.get(&job.id).status, Status::Running);
    fx.clock.advance(POLL_MS);
    monitor.tick().unwrap();
    assert_eq!(source.cancels(), 1, "the cancel is not sent again");

    source.observe(Observed::Cancelled(outcome("cancelled upstream")));
    fx.clock.advance(400);
    assert_eq!(monitor.tick().unwrap().watching, 0);
    let job = fx.get(&job.id);
    assert_eq!(job.status, Status::Cancelled);
    assert_eq!(job.outcome, Some(outcome("cancelled upstream")));
}

#[test]
fn a_cancel_the_provider_cannot_do_leaves_the_job_as_it_was() {
    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Running);
    let mut monitor = monitor(&fx, "monitor-a", &source);
    monitor.tick().unwrap();

    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    monitor.tick().unwrap();
    let after = fx.get(&job.id);
    assert_eq!(after.status, Status::Running);
    assert_eq!(after.cancel_requested_ms, None);
    assert_eq!(
        after.cancel_failure.as_deref(),
        Some("This operation cannot be cancelled from here")
    );

    // A refusal's text is kept as one bounded line; a second request is tried again.
    source.reply_to_cancel(CancelReply::Failed(format!("no\n{}", "x".repeat(500))));
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    monitor.tick().unwrap();
    assert_eq!(source.cancels(), 2);
    let after = fx.get(&job.id);
    let failure = after.cancel_failure.unwrap();
    assert!(
        failure.starts_with("no x") && failure.len() == 200,
        "{failure}"
    );
    assert_eq!(
        (after.status, after.cancel_requested_ms),
        (Status::Running, None)
    );
}

#[test]
fn stop_watching_ends_the_job_without_cancelling_the_operation() {
    let fx = fixture();
    let source = Scripted::default();
    source.reply_to_cancel(CancelReply::Requested);
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Running);
    let mut monitor = monitor(&fx, "monitor-a", &source);
    monitor.tick().unwrap();

    fx.store.stop_watching(CHECKOUT, &job.id).unwrap();
    let tick = monitor.tick().unwrap();
    assert_eq!(tick.watching, 0);
    let job = fx.get(&job.id);
    assert_eq!(job.status, Status::Cancelled);
    assert_eq!(job.outcome.unwrap().refs["cancel"], "stopped_watching");
    assert_eq!(source.cancels(), 0, "the external operation was left alone");
    assert_eq!(source.polls().len(), 1);
}

#[test]
fn a_dead_monitors_job_stalls_and_the_next_monitor_takes_it_over() {
    let fx = fixture();
    let first_source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    first_source.observe(Observed::Running);
    let mut first = monitor(&fx, "monitor-a", &first_source);
    first.tick().unwrap();

    // The first monitor dies without a word. A second one cannot take a live lease.
    let second_source = Scripted::default();
    let mut second = monitor(&fx, "monitor-b", &second_source);
    assert_eq!(second.tick().unwrap().watching, 0);
    assert!(second_source.polls().is_empty());

    fx.clock.advance(LEASE_MS);
    assert_eq!(
        fx.get(&job.id).liveness(fx.store.now_ms()),
        Liveness::Stalled
    );
    second_source.observe(Observed::Running);
    assert_eq!(second.tick().unwrap().watching, 1);
    let taken = fx.get(&job.id);
    let lease = taken.lease.clone().unwrap();
    assert_eq!((lease.owner.as_str(), lease.generation), ("monitor-b", 2));
    assert_eq!(taken.liveness(fx.store.now_ms()), Liveness::Live);

    // If the first monitor was only slow, it finds its lease gone and lets go quietly.
    first_source.observe(Observed::Failed(outcome("stale")));
    fx.clock.advance(POLL_MS);
    assert_eq!(first.tick().unwrap().watching, 0);
    let after = fx.get(&job.id);
    assert_eq!(after.status, Status::Running);
    assert_eq!(after.lease.unwrap().owner, "monitor-b");
    assert_eq!(first_source.polls().len(), 1, "it did not even poll");
}

#[test]
fn a_lost_watch_job_is_reattached_with_its_recorded_operation() {
    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Waiting);
    monitor(&fx, "monitor-a", &source).tick().unwrap();

    fx.clock.advance(LEASE_MS + LOST_AFTER_MS);
    fx.store.reconcile(CHECKOUT, &NoProbe).unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Lost);

    let next = Scripted::default();
    next.observe(Observed::Succeeded(outcome("green")));
    let mut second = monitor(&fx, "monitor-b", &next);
    second.tick().unwrap();
    let job = fx.get(&job.id);
    assert_eq!(job.status, Status::Succeeded);
    assert_eq!(job.external, Some(external()));
    assert_eq!(job.lease.unwrap().generation, 2);
}

#[test]
fn a_stopped_monitor_releases_its_jobs_for_the_next_one() {
    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Running);
    let mut first = monitor(&fx, "monitor-a", &source);
    let mut control = Stepper {
        fx: &fx,
        stop_after: Some(0),
        waits: 0,
    };
    assert_eq!(first.run(&mut control).unwrap(), MonitorExit::Stopped);
    let released = fx.get(&job.id);
    assert_eq!(released.status, Status::Running, "a stop is not an outcome");
    assert_eq!(released.liveness(fx.store.now_ms()), Liveness::Stalled);

    // No clock movement: the next monitor resumes at once.
    let next = Scripted::default();
    next.observe(Observed::Running);
    let mut second = monitor(&fx, "monitor-b", &next);
    assert_eq!(second.tick().unwrap().watching, 1);
    assert_eq!(fx.get(&job.id).lease.unwrap().owner, "monitor-b");
}

#[test]
fn the_fixture_source_answers_unchanged_files_as_conditional_polls() {
    let fx = fixture();
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: serde_json::Value| {
        std::fs::write(
            dir.path().join(format!("{name}.json")),
            serde_json::to_vec(&body).unwrap(),
        )
        .unwrap();
    };
    let mut new = new_job(ProducerFamily::Watch);
    new.kind.name = FIXTURE_KIND.into();
    new.external = Some(ExternalRef {
        system: FIXTURE_KIND.into(),
        id: "op-1".into(),
    });
    let job = fx.store.create(new).unwrap();
    let mut monitor = Monitor::new(fx.store.clone(), "monitor-a", monitor_config());
    monitor.register(Box::new(FixtureWatchSource::new(dir.path())));

    // No file yet: a transient failure, retried.
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).latest[WATCH_STATE_KEY], "retrying");

    write(
        "op-1",
        serde_json::json!({
            "etag": "1", "state": "running", "cancel": "requested", "feed": ["started"],
            "steps": [
                {"id": "build", "parent": null, "label": "Build", "status": "succeeded",
                 "done": null, "total": null, "unit": null, "weight": 1},
                {"id": "test", "parent": null, "label": "Test", "status": "active",
                 "done": 1, "total": 2, "unit": "suites", "weight": 1}
            ]
        }),
    );
    for _ in 0..3 {
        fx.clock.advance(2_000);
        monitor.tick().unwrap();
    }
    let running = fx.get(&job.id);
    assert_eq!(running.status, Status::Running);
    assert!((running.progress().unwrap().fraction - 0.75).abs() < 1e-9);
    assert!(!running.latest.contains_key(WATCH_STATE_KEY));
    let polls = std::fs::read_to_string(dir.path().join("op-1.polls")).unwrap();
    assert_eq!(polls, "full\nconditional\nconditional\n");

    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    monitor.tick().unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("op-1.cancels")).unwrap(),
        "cancel\n"
    );
    write(
        "op-1",
        serde_json::json!({"etag": "2", "state": "cancelled", "summary": "stopped by request"}),
    );
    fx.clock.advance(2_000);
    monitor.tick().unwrap();
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Cancelled);
    let outcome = done.outcome.unwrap();
    assert_eq!(outcome.summary.as_deref(), Some("stopped by request"));
    assert_eq!(outcome.refs["operation"], "op-1");
}

#[test]
fn the_fixture_source_refuses_names_that_could_leave_its_directory() {
    let fx = fixture();
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.json");
    std::fs::write(&outside, br#"{"etag":"1","state":"succeeded"}"#).unwrap();
    let inner = dir.path().join("inner");
    std::fs::create_dir(&inner).unwrap();
    let mut new = new_job(ProducerFamily::Watch);
    new.kind.name = FIXTURE_KIND.into();
    new.external = Some(ExternalRef {
        system: FIXTURE_KIND.into(),
        id: "../outside".into(),
    });
    let job = fx.store.create(new).unwrap();
    let mut monitor = Monitor::new(fx.store.clone(), "monitor-a", monitor_config());
    monitor.register(Box::new(FixtureWatchSource::new(&inner)));
    monitor.tick().unwrap();
    let job = fx.get(&job.id);
    assert_eq!(job.status, Status::Waiting);
    assert_eq!(job.latest[WATCH_STATE_KEY], "retrying");
}

/// File watchers can replay events from just before they started; wait those out.
fn settle(waiter: &hotsheet_jobs::ChangeWaiter) {
    while waiter.wait(std::time::Duration::from_millis(300)) {}
}

#[test]
fn wait_for_change_wakes_on_a_store_write_and_times_out_without_one() {
    use std::time::{Duration, Instant};

    let fx = fixture();
    let job = fx.create(ProducerFamily::Watch);
    let since = fx.store.revision(CHECKOUT).unwrap();

    // Nothing changes: the call returns the same revision after its timeout.
    let started = Instant::now();
    let unchanged = fx
        .store
        .wait_for_change(CHECKOUT, since, Duration::from_millis(150))
        .unwrap();
    assert_eq!(unchanged, since);
    assert!(started.elapsed() >= Duration::from_millis(150));

    // A write from another thread ends the wait long before its timeout.
    let writer = {
        let (store, id) = (fx.store.clone(), job.id.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            store.stop_watching(CHECKOUT, &id).unwrap();
        })
    };
    let started = Instant::now();
    let changed = fx
        .store
        .wait_for_change(CHECKOUT, since, Duration::from_secs(60))
        .unwrap();
    writer.join().unwrap();
    assert_eq!(changed, since + 1);
    assert!(started.elapsed() < Duration::from_secs(30));

    // A caller that is already behind does not wait at all.
    let started = Instant::now();
    let behind = fx
        .store
        .wait_for_change(CHECKOUT, since, Duration::from_secs(60))
        .unwrap();
    assert_eq!(behind, since + 1);
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn an_idle_monitor_step_wakes_no_watcher() {
    use std::time::Duration;

    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Waiting);
    let mut monitor = Monitor::new(
        fx.store.clone(),
        "monitor-a",
        MonitorConfig {
            poll_ms: 10 * LEASE_MS,
            max_poll_ms: 10 * LEASE_MS,
            ..monitor_config()
        },
    );
    monitor.register(Box::new(source.clone()));
    monitor.tick().unwrap();

    let waiter = hotsheet_jobs::ChangeWaiter::new(&fx.home.path().join("jobs")).unwrap();
    settle(&waiter);
    // Steps with nothing due take locks and read, but change nothing a watcher reports.
    // Otherwise every step would wake the monitor again, forever.
    for _ in 0..5 {
        monitor.tick().unwrap();
        fx.store.reconcile(CHECKOUT, &NoProbe).unwrap();
        fx.store.list(CHECKOUT).unwrap();
    }
    assert!(
        !waiter.wait(Duration::from_millis(700)),
        "an idle step woke the watcher"
    );

    // A real change does wake it.
    fx.store.stop_watching(CHECKOUT, &job.id).unwrap();
    let mut woke = false;
    for _ in 0..20 {
        woke |= waiter.wait(Duration::from_millis(500));
        if woke {
            break;
        }
    }
    assert!(woke, "a store write must wake the watcher");
}

#[test]
fn a_monitor_that_slept_past_its_lease_keeps_its_jobs() {
    let fx = fixture();
    let source = Scripted::default();
    let job = fx.create(ProducerFamily::Watch);
    source.observe(Observed::Running);
    let mut monitor = monitor(&fx, "monitor-a", &source);
    monitor.tick().unwrap();

    // The machine slept far past the lease and the grace window, and nobody took over.
    fx.clock.advance(10 * (LEASE_MS + LOST_AFTER_MS));
    source.observe(Observed::Running);
    let tick = monitor.tick().unwrap();
    assert_eq!(tick.watching, 1);
    let after = fx.get(&job.id);
    assert_eq!(
        after.status,
        Status::Running,
        "it did not declare its own Job lost"
    );
    assert_eq!(after.lease.as_ref().unwrap().generation, 1);
    assert_eq!(after.liveness(fx.store.now_ms()), Liveness::Live);
    assert_eq!(source.polls().len(), 2);
}
