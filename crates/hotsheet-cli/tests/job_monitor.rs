//! HS2-5TC1GQ: the watch-Job monitor as a real process. Every test spawns the built
//! `hotsheet-cli` against a temporary machine home and drives a file-backed fixture
//! source, so nothing touches the network or the developer's own Job store.
//!
//! The fixture source exists only in debug builds of the CLI.

#![cfg(all(unix, debug_assertions))]

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hotsheet_jobs::{
    Capabilities, ChangeWaiter, ExternalRef, FIXTURE_KIND, Job, JobKind, JobStore, Liveness,
    MonitorSlot, NewJob, NoProbe, ProducerFamily, SlotState, Status, StoreConfig, SystemClock,
    UlidSource,
};
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(30);

struct Fixture {
    home: tempfile::TempDir,
    operations: tempfile::TempDir,
    checkout: tempfile::TempDir,
    checkout_id: String,
    store: JobStore,
    slot: MonitorSlot,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let checkout = tempfile::tempdir().unwrap();
        let checkout_id = hotsheet_ticketing::checkouts::checkout_id(checkout.path()).unwrap();
        Self {
            store: JobStore::at(home.path()),
            slot: MonitorSlot::at(home.path()),
            operations: tempfile::tempdir().unwrap(),
            home,
            checkout,
            checkout_id,
        }
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(workspace_bin::path("hotsheet-cli"))
            .args(args)
            .env("HOTSHEET_HOME", self.home.path())
            .env("HOTSHEET_JOB_WATCH_FIXTURES", self.operations.path())
            // HS2-RTEQ7R: no GitLab credential is looked up: the developer's own
            // `GITLAB_TOKEN` or `glab` sign-in is never read by a test.
            .env_remove("GITLAB_TOKEN")
            .env("HOTSHEET_GITLAB_API_URL", "http://gitlab.invalid")
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.cli(args);
        assert!(
            output.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn start(&self, timing: &[&str]) -> Value {
        let mut args = vec!["job", "watch", "start"];
        args.extend_from_slice(timing);
        self.json(&args)
    }

    fn operation(&self, name: &str, body: Value) {
        // Written beside the target and renamed, so the monitor never reads half a file.
        let temp = self.operations.path().join(format!("{name}.partial"));
        std::fs::write(&temp, serde_json::to_vec(&body).unwrap()).unwrap();
        std::fs::rename(temp, self.operations.path().join(format!("{name}.json"))).unwrap();
    }

    fn watch_job(&self, name: &str) -> Job {
        self.store
            .create(NewJob {
                checkout_id: self.checkout_id.clone(),
                kind: JobKind {
                    family: ProducerFamily::Watch,
                    name: FIXTURE_KIND.into(),
                },
                job_key: format!("fixture:{name}"),
                title: "Synthetic watched operation".into(),
                capabilities: Capabilities::default(),
                ticket: None,
                external: Some(ExternalRef {
                    system: FIXTURE_KIND.into(),
                    id: name.into(),
                }),
                retry_of: None,
                url: None,
            })
            .unwrap()
    }

    fn job(&self, id: &str) -> Job {
        self.store.get(&self.checkout_id, id).unwrap()
    }

    /// Wait for a condition on the Job directory, waking on its file changes.
    fn until<T>(&self, what: &str, mut check: impl FnMut() -> Option<T>) -> T {
        let waiter = ChangeWaiter::new(self.slot.dir()).unwrap();
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(value) = check() {
                return value;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            waiter.wait(Duration::from_millis(50));
        }
    }

    fn until_job(&self, id: &str, what: &str, check: impl Fn(&Job) -> bool) -> Job {
        self.until(what, || Some(self.job(id)).filter(|job| check(job)))
    }

    fn until_free(&self) {
        self.until("the monitor to exit", || {
            (self.slot.state().unwrap() == SlotState::Free).then_some(())
        });
    }

    fn monitor_pid(&self) -> Option<u32> {
        match self.slot.state().unwrap() {
            SlotState::Held(Some(info)) => Some(info.pid),
            _ => None,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Never leave a monitor behind, whatever the test did.
        let _ = self.cli(&["job", "watch", "stop"]);
        if let Some(pid) = self.monitor_pid() {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        }
    }
}

fn checkout_arg(fx: &Fixture) -> String {
    fx.checkout.path().to_string_lossy().into_owned()
}

#[test]
fn start_and_stop_are_idempotent() {
    let fx = Fixture::new();
    assert_eq!(
        fx.json(&["job", "watch", "status"]),
        json!({"running": false, "monitor": null, "gitlab_token": null})
    );
    // Stopping when nothing runs is a successful no-op and creates no monitor.
    assert_eq!(
        fx.json(&["job", "watch", "stop"]),
        json!({"running": false, "stopped": false, "monitor": null})
    );

    let first = fx.start(&[]);
    assert_eq!(
        (&first["running"], &first["started"]),
        (&json!(true), &json!(true))
    );
    let owner = first["monitor"]["owner"].as_str().unwrap().to_owned();
    assert!(owner.starts_with("monitor:"));

    // A second and third start find the same monitor and start nothing.
    for _ in 0..2 {
        let again = fx.start(&[]);
        assert_eq!(again["started"], json!(false));
        assert_eq!(again["monitor"], first["monitor"]);
    }
    assert_eq!(
        fx.json(&["job", "watch", "status"])["monitor"],
        first["monitor"]
    );

    // The launch is in the process journal under its own purpose, once.
    let processes = fx.json(&["processes"]);
    let launches: Vec<&Value> = processes
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["purpose"] == "job_monitor")
        .collect();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0]["pid"], first["monitor"]["pid"]);

    let stopped = fx.json(&["job", "watch", "stop"]);
    assert_eq!(stopped["stopped"], json!(true));
    assert_eq!(stopped["monitor"]["owner"], json!(owner));
    assert_eq!(fx.slot.state().unwrap(), SlotState::Free);
    assert_eq!(fx.json(&["job", "watch", "stop"])["stopped"], json!(false));
    assert_eq!(
        fx.json(&["job", "watch", "status"])["running"],
        json!(false)
    );

    // A later start is a new monitor with a new owner id.
    let next = fx.start(&[]);
    assert_eq!(next["started"], json!(true));
    assert_ne!(next["monitor"]["owner"], json!(owner));
}

#[test]
fn racing_starts_leave_exactly_one_monitor() {
    let fx = Fixture::new();
    let outputs: Vec<Value> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4).map(|_| scope.spawn(|| fx.start(&[]))).collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    let owner = &outputs[0]["monitor"]["owner"];
    for output in &outputs {
        assert_eq!(output["running"], json!(true));
        assert_eq!(
            &output["monitor"]["owner"], owner,
            "every start names one monitor"
        );
    }
    assert_eq!(
        fx.json(&["job", "watch", "status"])["monitor"]["owner"],
        *owner
    );
}

/// HS2-WEYR63: a descriptor the starting process holds without close-on-exec (on macOS a
/// pipe another thread was creating while this process was spawned) is not passed on to
/// the monitor. Before, the monitor kept the racing test's output pipe open, so that
/// start's output ended only when the monitor idled out 30 s later.
#[test]
fn the_monitor_inherits_no_descriptor_from_its_starter() {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let fx = Fixture::new();
    let mut fds = [0; 2];
    // SAFETY: pipe fills two new descriptors, which are owned below.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    // SAFETY: both descriptors were just created by pipe and are owned exactly once.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    // The write end stays open across exec, as a leaked pipe does; the read end does not.
    // SAFETY: plain descriptor flag calls on descriptors this test owns.
    unsafe {
        assert_eq!(libc::fcntl(write.as_raw_fd(), libc::F_SETFD, 0), 0);
        assert_eq!(
            libc::fcntl(read.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC),
            0
        );
        let flags = libc::fcntl(read.as_raw_fd(), libc::F_GETFL);
        assert_eq!(
            libc::fcntl(read.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK),
            0
        );
    }
    let started = fx.start(&[]);
    assert_eq!(started["started"], json!(true));
    drop(write);
    // The starter has exited; with the test's own end closed, only the monitor could still
    // hold a writer. No writer left reads as end of file, a held one as "try again".
    let mut byte = 0u8;
    // SAFETY: reads at most one byte into a local buffer from a descriptor this test owns.
    let read_bytes = unsafe { libc::read(read.as_raw_fd(), (&raw mut byte).cast(), 1) };
    assert_eq!(
        read_bytes,
        0,
        "the monitor holds the starter's descriptor: {}",
        std::io::Error::last_os_error()
    );
    assert_eq!(
        fx.json(&["job", "watch", "status"])["monitor"],
        started["monitor"]
    );
}

#[test]
fn the_monitor_reports_a_watch_job_to_its_outcome_and_then_exits() {
    let fx = Fixture::new();
    fx.operation("build-1", json!({"etag": "1", "state": "waiting"}));
    let job = fx.watch_job("build-1");
    let started = fx.start(&["--poll-ms", "30", "--idle-exit-ms", "400"]);
    let owner = started["monitor"]["owner"].as_str().unwrap().to_owned();

    let waiting = fx.until_job(&job.id, "the monitor to attach", |job| {
        job.status == Status::Waiting
    });
    let lease = waiting.lease.unwrap();
    assert_eq!((lease.owner, lease.generation), (owner, 1));

    fx.operation(
        "build-1",
        json!({
            "etag": "2", "state": "running", "feed": ["run started"],
            "steps": [
                {"id": "build", "parent": null, "label": "Build", "status": "succeeded",
                 "done": null, "total": null, "unit": null, "weight": 1},
                {"id": "test", "parent": null, "label": "Test", "status": "pending",
                 "done": null, "total": null, "unit": null, "weight": 1}
            ]
        }),
    );
    fx.until_job(&job.id, "the run to start", |job| {
        job.status == Status::Running
    });

    // The headless read surfaces show what the monitor reported.
    let listed = fx.json(&["job", "list", "--checkout", &checkout_arg(&fx)]);
    assert_eq!(listed["checkout_id"], json!(fx.checkout_id));
    let view = &listed["jobs"][0];
    assert_eq!(view["id"], json!(job.id));
    assert_eq!(view["status"], json!("running"));
    assert_eq!(view["liveness"], json!("live"));
    assert_eq!(view["progress"]["fraction"], json!(0.5));
    let shown = fx.json(&[
        "job",
        "show",
        &job.id,
        "--checkout",
        &fx.checkout_id,
        "--feed-after",
        "0",
    ]);
    assert_eq!(shown["steps"][0]["label"], json!("Build"));
    assert_eq!(
        shown["feed_page"]["entries"][0]["text"],
        json!("run started")
    );
    assert_eq!(shown["feed_page"]["cursor"], json!(1));

    // While the file is unchanged the monitor asks conditionally instead of rereading.
    let polls_path = fx.operations.path().join("build-1.polls");
    fx.until("a conditional poll", || {
        std::fs::read_to_string(&polls_path)
            .ok()
            .filter(|polls| polls.lines().any(|line| line == "conditional"))
    });

    fx.operation(
        "build-1",
        json!({"etag": "3", "state": "succeeded", "summary": "all green"}),
    );
    let done = fx.until_job(&job.id, "the outcome", |job| job.status.is_finished());
    assert_eq!(done.status, Status::Succeeded);
    let outcome = done.outcome.unwrap();
    assert_eq!(outcome.summary.as_deref(), Some("all green"));
    assert_eq!(outcome.refs["operation"], "build-1");

    // Each change was read in full exactly once.
    let polls = std::fs::read_to_string(&polls_path).unwrap();
    assert_eq!(polls.lines().filter(|line| *line == "full").count(), 3);

    // With nothing left to watch the monitor leaves by itself.
    fx.until_free();
    assert_eq!(
        fx.json(&["job", "watch", "status"])["running"],
        json!(false)
    );
}

#[test]
fn a_killed_monitor_leaves_the_job_stalled_then_lost_and_a_new_monitor_reattaches() {
    let fx = Fixture::new();
    let timing = [
        "--lease-ms",
        "600",
        "--lost-after-ms",
        "300",
        "--poll-ms",
        "30",
        "--idle-exit-ms",
        "60000",
    ];
    fx.operation("build-2", json!({"etag": "1", "state": "running"}));
    let job = fx.watch_job("build-2");
    let first = fx.start(&timing);
    fx.until_job(&job.id, "the first monitor to attach", |job| {
        job.status == Status::Running
    });

    // Kill it outright: no release, no outcome, no cleanup.
    let pid = first["monitor"]["pid"].as_u64().unwrap().to_string();
    assert!(
        Command::new("kill")
            .args(["-9", &pid])
            .status()
            .unwrap()
            .success()
    );
    fx.until_free();
    let orphaned = fx.job(&job.id);
    assert_eq!(orphaned.status, Status::Running, "a crash reports nothing");
    assert_eq!(orphaned.lease.as_ref().unwrap().generation, 1);

    // Unrenewed, the Job reads as stalled, and then reconciliation declares it lost.
    let recovery = JobStore::with(
        fx.home.path(),
        Arc::new(SystemClock),
        Arc::new(UlidSource),
        StoreConfig {
            lease_ms: 600,
            lost_after_ms: 300,
            ..StoreConfig::default()
        },
    );
    fx.until("the lease to expire", || {
        (fx.job(&job.id).liveness(recovery.now_ms()) == Liveness::Stalled).then_some(())
    });
    let listed = fx.json(&["job", "list", "--checkout", &fx.checkout_id]);
    assert_eq!(listed["jobs"][0]["liveness"], json!("stalled"));
    assert_eq!(listed["jobs"][0]["status"], json!("running"));
    fx.until("the Job to be lost", || {
        recovery.reconcile(&fx.checkout_id, &NoProbe).unwrap();
        (fx.job(&job.id).status == Status::Lost).then_some(())
    });

    // A new monitor reattaches to the same operation under a new lease generation.
    let second = fx.start(&timing);
    assert_eq!(second["started"], json!(true));
    assert_ne!(second["monitor"]["owner"], first["monitor"]["owner"]);
    let reattached = fx.until_job(&job.id, "the reattach", |job| job.status.is_active());
    let lease = reattached.lease.unwrap();
    assert_eq!(lease.generation, 2);
    assert_eq!(json!(lease.owner), second["monitor"]["owner"]);
    assert_eq!(reattached.external.unwrap().id, "build-2");

    fx.operation(
        "build-2",
        json!({"etag": "2", "state": "failed", "summary": "red"}),
    );
    let done = fx.until_job(&job.id, "the outcome", |job| job.status.is_finished());
    assert_eq!(done.status, Status::Failed);
    assert_eq!(done.outcome.unwrap().summary.as_deref(), Some("red"));
}

#[test]
fn a_stopped_monitor_releases_its_jobs_and_the_next_start_resumes_them_at_once() {
    let fx = Fixture::new();
    // The default 60 second lease: a resume inside this test proves the release.
    let timing = ["--poll-ms", "30"];
    fx.operation("build-3", json!({"etag": "1", "state": "waiting"}));
    let job = fx.watch_job("build-3");
    fx.start(&timing);
    fx.until_job(&job.id, "the monitor to attach", |job| {
        job.status == Status::Waiting
    });

    assert_eq!(fx.json(&["job", "watch", "stop"])["stopped"], json!(true));
    let released = fx.job(&job.id);
    assert_eq!(released.status, Status::Waiting, "a stop is not an outcome");
    assert_eq!(released.liveness(fx.store.now_ms()), Liveness::Stalled);

    let second = fx.start(&timing);
    let resumed = fx.until_job(&job.id, "the resume", |job| {
        job.lease
            .as_ref()
            .is_some_and(|lease| lease.generation == 2)
    });
    assert_eq!(
        json!(resumed.lease.unwrap().owner),
        second["monitor"]["owner"]
    );

    // A stop-watching request ends the Job without touching the operation.
    fx.store.stop_watching(&fx.checkout_id, &job.id).unwrap();
    let stopped = fx.until_job(&job.id, "stop watching", |job| job.status.is_finished());
    assert_eq!(stopped.status, Status::Cancelled);
    assert_eq!(stopped.outcome.unwrap().refs["cancel"], "stopped_watching");
    assert!(!fx.operations.path().join("build-3.cancels").exists());
}

#[test]
fn read_commands_refuse_unknown_checkouts_and_jobs_without_creating_anything() {
    let fx = Fixture::new();
    let unknown = fx.cli(&["job", "list", "--checkout", "no-such-checkout"]);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown checkout"));

    let empty = fx.json(&["job", "list", "--checkout", &checkout_arg(&fx)]);
    assert_eq!(empty["jobs"], json!([]));
    assert_eq!(empty["unreadable"], json!([]));

    let missing = fx.cli(&[
        "job",
        "show",
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "--checkout",
        &checkout_arg(&fx),
    ]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("job not found"));
    let malformed = fx.cli(&["job", "show", "../x", "--checkout", &checkout_arg(&fx)]);
    assert!(!malformed.status.success());

    let jobs_dir: PathBuf = fx.home.path().join("jobs");
    assert!(!jobs_dir.exists(), "reads create no Job directory");
}
