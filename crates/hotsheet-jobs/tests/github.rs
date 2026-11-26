//! HS2-3R9V0R: the GitHub Actions watch source and the CI gate API, offline. A scripted
//! `GithubApi` stands in for the provider with the REST shapes, ETags and status codes of
//! the real endpoints. No test makes a network call. Synthetic data only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{CHECKOUT, Fixture, LEASE_MS, LOST_AFTER_MS, fixture};
use hotsheet_jobs::{
    ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, CiArtifact, CiRun, CiTarget,
    CiWatchOptions, GITHUB_ACTIONS_KIND, GateState, GithubActionsSource, GithubApi, JobError,
    Liveness, Monitor, MonitorConfig, NoProbe, Status, StepStatus, TicketLink,
    UNKNOWN_COMMIT_LIMIT_MS, WATCH_STATE_KEY, ci_outcome, ensure_ci_watch, find_ci_watch,
    gate_state, rewatch_ci, wait_for_gate_change,
};
use serde_json::{Value, json};

const SHA: &str = "1111111111111111111111111111111111111111";
const FIX: &str = "2222222222222222222222222222222222222222";
const OLD: &str = "0000000000000000000000000000000000000000";
const POLL_MS: u64 = 100;
const RUNS: &str = "repos/acme/widgets/actions/runs?branch=main&event=push&per_page=30";

#[derive(Clone)]
struct Route {
    status: u16,
    etag: Option<String>,
    retry_after_ms: Option<u64>,
    body: String,
}

#[derive(Default)]
struct Fake {
    routes: BTreeMap<String, Route>,
    versions: u64,
    log: Vec<ApiRequest>,
    down: bool,
}

/// Serves canned answers by exact path. An unchanged answer is a 304 to a conditional
/// request, as the real API does; an unknown path is a 404.
#[derive(Clone, Default)]
struct FakeApi(Arc<Mutex<Fake>>);

impl FakeApi {
    fn serve(&self, path: &str, status: u16, body: Value) {
        let mut fake = self.0.lock().unwrap();
        fake.versions += 1;
        let etag = format!("W/\"v{}\"", fake.versions);
        fake.routes.insert(
            path.to_owned(),
            Route {
                status,
                etag: (status == 200).then_some(etag),
                retry_after_ms: None,
                body: body.to_string(),
            },
        );
    }

    fn serve_raw(&self, path: &str, route: Route) {
        self.0.lock().unwrap().routes.insert(path.to_owned(), route);
    }

    fn runs(&self, runs: Vec<Value>) {
        self.serve(
            RUNS,
            200,
            json!({"total_count": runs.len(), "workflow_runs": runs}),
        );
    }

    fn jobs(&self, run_id: u64, jobs: Vec<Value>) {
        let path = format!("repos/acme/widgets/actions/runs/{run_id}/jobs?per_page=100");
        self.serve(&path, 200, json!({"total_count": jobs.len(), "jobs": jobs}));
    }

    fn compare(&self, head: &str, status: &str) {
        let path = format!("repos/acme/widgets/compare/{SHA}...{head}?per_page=1");
        self.serve(
            &path,
            200,
            json!({"status": status, "ahead_by": 1, "behind_by": 0}),
        );
    }

    fn set_down(&self, down: bool) {
        self.0.lock().unwrap().down = down;
    }

    fn log(&self) -> Vec<ApiRequest> {
        self.0.lock().unwrap().log.clone()
    }

    fn count(&self, needle: &str) -> usize {
        self.log()
            .iter()
            .filter(|request| request.path.contains(needle))
            .count()
    }
}

impl GithubApi for FakeApi {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let mut fake = self.0.lock().unwrap();
        fake.log.push(request.clone());
        if fake.down {
            return Err(ApiUnavailable {
                retry_after_ms: None,
            });
        }
        let Some(route) = fake.routes.get(&request.path).cloned() else {
            return Ok(ApiResponse {
                status: 404,
                etag: None,
                retry_after_ms: None,
                body: json!({"message": "Not Found"}).to_string(),
            });
        };
        let unchanged = request.method == ApiMethod::Get
            && route.etag.is_some()
            && request.if_none_match == route.etag;
        Ok(ApiResponse {
            status: if unchanged { 304 } else { route.status },
            etag: route.etag,
            retry_after_ms: route.retry_after_ms,
            body: if unchanged { String::new() } else { route.body },
        })
    }
}

fn run(id: u64, head: &str, status: &str, conclusion: Option<&str>) -> Value {
    json!({
        "id": id,
        "name": "CI",
        "head_branch": "main",
        "head_sha": head,
        "event": "push",
        "status": status,
        "conclusion": conclusion,
        "html_url": format!("https://github.example.test/acme/widgets/actions/runs/{id}"),
        "created_at": format!("2026-11-01T10:{:02}:00Z", id % 60),
        "updated_at": format!("2026-11-01T10:{:02}:30Z", id % 60),
        "run_started_at": format!("2026-11-01T10:{:02}:00Z", id % 60),
        "run_attempt": 1,
        "path": ".github/workflows/ci.yml"
    })
}

fn workflow_job(
    id: u64,
    name: &str,
    status: &str,
    conclusion: Option<&str>,
    steps: &[&str],
) -> Value {
    let steps: Vec<Value> = steps
        .iter()
        .enumerate()
        .map(|(number, status)| json!({"name": format!("step {number}"), "status": status, "number": number + 1}))
        .collect();
    json!({"id": id, "name": name, "status": status, "conclusion": conclusion, "steps": steps})
}

fn target() -> CiTarget {
    CiTarget::new("acme/widgets", SHA, "CI", "main").unwrap()
}

fn monitor(fx: &Fixture, api: &FakeApi) -> Monitor {
    let mut monitor = Monitor::new(
        fx.store.clone(),
        "monitor-a",
        MonitorConfig {
            poll_ms: POLL_MS,
            max_poll_ms: 400,
            max_error_ms: 1_600,
            idle_exit_ms: 2_000,
        },
    );
    monitor.register(Box::new(GithubActionsSource::new(api.clone())));
    monitor
}

/// Advance to the next due poll and step.
fn step(fx: &Fixture, monitor: &mut Monitor) {
    fx.clock.advance(400);
    monitor.tick().unwrap();
}

fn gate(fx: &Fixture) -> GateState {
    gate_state(&fx.store, CHECKOUT, &target()).unwrap().state
}

#[test]
fn a_commits_run_is_followed_from_queued_to_green() {
    let fx = fixture();
    let api = FakeApi::default();
    let ticket = TicketLink {
        connection_id: "c5c4d57ccef3d9ba".into(),
        native_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
    };
    let options = CiWatchOptions {
        ticket: Some(ticket.clone()),
        can_cancel: false,
    };
    assert_eq!(gate(&fx), GateState::Unknown, "nothing is watched yet");
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &options).unwrap();
    assert_eq!(job.kind.name, GITHUB_ACTIONS_KIND);
    assert_eq!(job.title, "CI on main for 1111111");
    assert_eq!(job.job_key, "github-actions:acme/widgets:main:CI");
    assert_eq!(job.ticket, Some(ticket.clone()));
    assert!(!job.capabilities.cancel && job.capabilities.retry && job.capabilities.open_url);
    assert_eq!(gate(&fx), GateState::Pending);

    // Three earlier successful runs of the workflow, each 30 seconds long, then ours.
    let history = || {
        vec![
            run(3, OLD, "completed", Some("success")),
            run(2, OLD, "completed", Some("success")),
            run(1, OLD, "completed", Some("success")),
        ]
    };
    let with = |own: Value| {
        let mut runs = vec![own];
        runs.extend(history());
        runs
    };
    api.runs(with(run(9, SHA, "queued", None)));
    api.jobs(9, vec![workflow_job(91, "build", "queued", None, &[])]);
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();
    let waiting = fx.get(&job.id);
    assert_eq!(waiting.status, Status::Waiting);
    assert_eq!(waiting.steps[0].status, StepStatus::Pending);
    assert_eq!(
        waiting.eta(),
        None,
        "a queued run has no estimate from the provider"
    );
    assert_eq!(
        api.count("/compare/"),
        0,
        "the commit's own run needs no ancestry lookup"
    );

    api.runs(with(run(9, SHA, "in_progress", None)));
    api.jobs(
        9,
        vec![
            workflow_job(91, "build", "completed", Some("success"), &["completed"]),
            workflow_job(
                92,
                "test",
                "in_progress",
                None,
                &["completed", "in_progress", "queued", "queued"],
            ),
            workflow_job(93, "lint", "completed", Some("skipped"), &[]),
        ],
    );
    step(&fx, &mut monitor);
    let running = fx.get(&job.id);
    assert_eq!(running.status, Status::Running);
    let labels: Vec<(&str, StepStatus)> = running
        .steps
        .iter()
        .map(|step| (step.label.as_str(), step.status))
        .collect();
    assert_eq!(
        labels,
        [
            ("build", StepStatus::Succeeded),
            ("test", StepStatus::Active),
            ("lint", StepStatus::Skipped)
        ]
    );
    assert_eq!(
        (running.steps[1].done, running.steps[1].total),
        (Some(1), Some(4))
    );
    assert!((running.progress().unwrap().fraction - (1.0 + 0.25 + 1.0) / 3.0).abs() < 1e-9);
    assert_eq!(running.latest["run_id"], "9");
    assert_eq!(running.latest["run_status"], "in_progress");
    assert_eq!(running.latest["current_step"], "test");
    assert_eq!(
        running.url.as_deref(),
        Some("https://github.example.test/acme/widgets/actions/runs/9")
    );
    // Started 10:09:00 plus the 30 second median of the workflow's recent runs.
    let eta = running.eta().unwrap();
    assert_eq!(eta.source, hotsheet_jobs::EtaSource::Producer);
    assert_eq!(eta.finish_at_ms, 1_793_527_740_000 + 30_000);

    // Nothing changed upstream: both endpoints answer 304 and nothing is rewritten.
    let before = api.log().len();
    let revision = fx.get(&job.id).revision;
    fx.clock.advance(POLL_MS);
    monitor.tick().unwrap();
    let conditional: Vec<ApiRequest> = api.log()[before..].to_vec();
    assert_eq!(conditional.len(), 2);
    assert!(
        conditional
            .iter()
            .all(|request| request.if_none_match.is_some())
    );
    assert_eq!(fx.get(&job.id).revision, revision);

    api.runs(with(run(9, SHA, "completed", Some("success"))));
    api.jobs(
        9,
        vec![
            workflow_job(91, "build", "completed", Some("success"), &["completed"]),
            workflow_job(92, "test", "completed", Some("success"), &["completed"; 4]),
        ],
    );
    step(&fx, &mut monitor);
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Succeeded);
    assert_eq!(done.eta(), None);
    let run = CiRun {
        id: 9,
        attempt: 1,
        head_sha: SHA.into(),
        url: Some("https://github.example.test/acme/widgets/actions/runs/9".into()),
        artifact: None,
    };
    assert_eq!(gate(&fx), GateState::Green { run: run.clone() });

    // The outcome record carries identifiers for the ticket summary and the notifier.
    let outcome = ci_outcome(&done).unwrap();
    assert_eq!(outcome.gate, GateState::Green { run });
    assert_eq!(outcome.ticket, Some(ticket));
    assert_eq!(outcome.target, target());
    assert_eq!(
        outcome.summary.as_deref(),
        Some("CI run 9 passed for 1111111")
    );
    let refs = done.outcome.unwrap().refs;
    assert_eq!(refs["commit"], SHA);
    assert_eq!(refs["run_conclusion"], "success");
    assert_eq!(
        ci_outcome(&waiting),
        None,
        "an unfinished Job has no outcome record"
    );

    // Asking again finds the same Job and starts nothing.
    let again = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &options).unwrap();
    assert_eq!(again.id, job.id);
    assert_eq!(
        rewatch_ci(&fx.store, CHECKOUT, &target(), &options)
            .unwrap()
            .id,
        job.id
    );
    assert_eq!(fx.store.list(CHECKOUT).unwrap().jobs.len(), 1);
    let polls = api.log().len();
    step(&fx, &mut monitor);
    assert_eq!(api.log().len(), polls, "a finished gate is not polled");
}

#[test]
fn a_red_gate_stays_red_until_a_rewatch_sees_a_newer_covering_run() {
    let fx = fixture();
    let api = FakeApi::default();
    let options = CiWatchOptions::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &options).unwrap();
    api.runs(vec![run(9, SHA, "completed", Some("failure"))]);
    api.jobs(
        9,
        vec![workflow_job(
            91,
            "test",
            "completed",
            Some("failure"),
            &["completed"],
        )],
    );
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();

    let failed = fx.get(&job.id);
    assert_eq!(failed.status, Status::Failed);
    assert_eq!(failed.steps[0].status, StepStatus::Failed);
    let GateState::Red { run: red } = gate(&fx) else {
        panic!("expected red, got {:?}", gate(&fx));
    };
    assert_eq!((red.id, red.attempt, red.head_sha.as_str()), (9, 1, SHA));
    assert_eq!(
        ci_outcome(&failed).unwrap().summary.as_deref(),
        Some("CI run 9 failed for 1111111")
    );
    // A caller that keeps asking does not pile up Jobs for the same red run.
    for _ in 0..3 {
        assert_eq!(
            ensure_ci_watch(&fx.store, CHECKOUT, &target(), &options)
                .unwrap()
                .id,
            job.id
        );
    }

    // A deliberate re-watch is a new linked Job that ignores the run already judged.
    let second = rewatch_ci(&fx.store, CHECKOUT, &target(), &options).unwrap();
    assert_ne!(second.id, job.id);
    assert_eq!(second.retry_of.as_deref(), Some(job.id.as_str()));
    assert_eq!(
        rewatch_ci(&fx.store, CHECKOUT, &target(), &options)
            .unwrap()
            .id,
        second.id
    );
    assert_eq!(
        gate(&fx),
        GateState::Pending,
        "the newest Job speaks for the target"
    );
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&second.id).status, Status::Waiting);

    // The fix lands: its run covers the commit, and its head is checked once.
    api.runs(vec![
        run(12, FIX, "completed", Some("success")),
        run(9, SHA, "completed", Some("failure")),
    ]);
    api.compare(FIX, "ahead");
    api.jobs(
        12,
        vec![workflow_job(
            121,
            "test",
            "completed",
            Some("success"),
            &["completed"],
        )],
    );
    step(&fx, &mut monitor);
    let GateState::Green { run: green } = gate(&fx) else {
        panic!("expected green, got {:?}", gate(&fx));
    };
    assert_eq!((green.id, green.head_sha.as_str()), (12, FIX));
    assert_eq!(fx.get(&second.id).outcome.unwrap().refs["commit"], SHA);
    assert_eq!(api.count("/compare/"), 1);
    assert_eq!(
        fx.get(&job.id).status,
        Status::Failed,
        "the old verdict is kept"
    );
}

#[test]
fn a_run_cancelled_for_a_newer_push_stays_pending_until_the_newer_run_decides() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.runs(vec![
        run(12, FIX, "in_progress", None),
        run(9, SHA, "completed", Some("cancelled")),
    ]);
    api.compare(FIX, "ahead");
    api.jobs(
        12,
        vec![workflow_job(
            121,
            "test",
            "in_progress",
            None,
            &["in_progress"],
        )],
    );
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();
    let running = fx.get(&job.id);
    assert_eq!(running.status, Status::Running);
    assert_eq!(running.latest["run_id"], "12");
    assert_eq!(running.latest["run_head_sha"], FIX);
    assert_eq!(gate(&fx), GateState::Pending);

    // Ancestry never changes: later polls reuse the answer.
    for _ in 0..3 {
        step(&fx, &mut monitor);
    }
    assert_eq!(api.count("/compare/"), 1);

    api.runs(vec![
        run(12, FIX, "completed", Some("failure")),
        run(9, SHA, "completed", Some("cancelled")),
    ]);
    step(&fx, &mut monitor);
    let GateState::Red { run: red } = gate(&fx) else {
        panic!("expected red");
    };
    assert_eq!((red.id, red.head_sha.as_str()), (12, FIX));
}

#[test]
fn a_cancelled_run_with_nothing_newer_cancels_the_gate() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.runs(vec![run(9, SHA, "completed", Some("cancelled"))]);
    api.jobs(9, vec![]);
    monitor(&fx, &api).tick().unwrap();
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Cancelled);
    assert_eq!(done.outcome.as_ref().unwrap().refs["gate"], "cancelled");
    assert_eq!(gate(&fx), GateState::Cancelled);
    assert_eq!(ci_outcome(&done).unwrap().gate, GateState::Cancelled);
}

#[test]
fn a_branch_that_moved_without_the_commit_supersedes_the_gate() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.runs(vec![run(12, FIX, "completed", Some("success"))]);
    api.compare(FIX, "diverged");
    monitor(&fx, &api).tick().unwrap();
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Failed);
    assert_eq!(gate(&fx), GateState::Superseded);
    assert_eq!(
        ci_outcome(&done).unwrap().summary.as_deref(),
        Some("1111111 is no longer on main")
    );
    assert!(GateState::Superseded.is_final() && !GateState::Pending.is_final());
}

#[test]
fn a_commit_the_provider_does_not_know_yet_waits_for_its_run() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    // Only an earlier run exists, and the compare endpoint answers 404 for our commit.
    api.runs(vec![run(5, OLD, "completed", Some("success"))]);
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();
    let waiting = fx.get(&job.id);
    assert_eq!(waiting.status, Status::Waiting);
    assert!(waiting.steps.is_empty());
    assert_eq!(gate(&fx), GateState::Pending);

    // Runs of other workflows, branches and events never count.
    let mut other_workflow = run(10, SHA, "completed", Some("success"));
    other_workflow["name"] = json!("Platforms");
    let mut other_branch = run(11, SHA, "completed", Some("success"));
    other_branch["head_branch"] = json!("integration");
    let mut other_event = run(12, SHA, "completed", Some("success"));
    other_event["event"] = json!("workflow_dispatch");
    api.runs(vec![
        other_event,
        other_branch,
        other_workflow,
        run(5, OLD, "completed", Some("success")),
    ]);
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    api.runs(vec![run(13, SHA, "completed", Some("success"))]);
    api.jobs(13, vec![]);
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&job.id).status, Status::Succeeded);
}

#[test]
fn hs2_xgv2j9_a_commit_the_provider_never_learns_about_ends_with_gate_unknown() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    // The compare endpoint keeps answering 404 for the commit: a mistyped SHA, or a
    // commit that was never pushed.
    api.runs(vec![run(5, OLD, "completed", Some("success"))]);
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    // Just inside the bound it still waits, and unchanged polls keep asking.
    let created = fx.get(&job.id).created_ms;
    let asked = api.count("/compare/");
    // Large clock moves: the watch keeps its Job across them, as on a sleeping machine.
    while fx.store.now_ms() + 60_000 < created + UNKNOWN_COMMIT_LIMIT_MS {
        fx.clock.advance(60_000);
        monitor.tick().unwrap();
    }
    assert_eq!(fx.get(&job.id).status, Status::Waiting);
    assert_eq!(gate(&fx), GateState::Pending);
    assert!(api.count("/compare/") > asked);

    // At the bound, measured on the injected clock, the watch ends.
    fx.clock.advance(60_000);
    monitor.tick().unwrap();
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Failed);
    assert_eq!(done.outcome.as_ref().unwrap().refs["gate"], "unknown");
    assert_eq!(gate(&fx), GateState::Unknown);
    let outcome = ci_outcome(&done).unwrap();
    assert_eq!(outcome.gate, GateState::Unknown);
    assert_eq!(
        outcome.summary.as_deref(),
        Some("1111111 is not known to acme/widgets after 30 minutes")
    );

    // Polling stops with the Job.
    let polled = api.count("actions/runs?");
    step(&fx, &mut monitor);
    assert_eq!(api.count("actions/runs?"), polled);

    // A rewatch after the commit is finally pushed follows its run in a new Job.
    let again = rewatch_ci(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    assert_ne!(again.id, job.id);
    assert_eq!(again.retry_of.as_deref(), Some(job.id.as_str()));
    api.runs(vec![run(13, SHA, "completed", Some("success"))]);
    api.jobs(13, vec![]);
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&again.id).status, Status::Succeeded);
}

#[test]
fn hs2_xgv2j9_a_commit_that_appears_before_the_bound_is_followed_normally() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.runs(vec![run(5, OLD, "completed", Some("success"))]);
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();
    // Pushed late but inside the bound: the run is followed, and the bound no longer
    // applies once a run covers the commit, even long after it.
    fx.clock.advance(UNKNOWN_COMMIT_LIMIT_MS - 1_000);
    api.runs(vec![
        run(13, SHA, "in_progress", None),
        run(5, OLD, "completed", Some("success")),
    ]);
    api.jobs(13, vec![]);
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&job.id).status, Status::Running);
    fx.clock.advance(UNKNOWN_COMMIT_LIMIT_MS);
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&job.id).status, Status::Running);
    assert_eq!(gate(&fx), GateState::Pending);
}

#[test]
fn cancel_reaches_only_the_commits_own_run_and_only_when_declared() {
    let fx = fixture();
    let api = FakeApi::default();
    let cancel_path = "repos/acme/widgets/actions/runs/9/cancel";

    // Not declared: the action is absent, and stop watching remains.
    let plain =
        ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    assert!(matches!(
        fx.store.request_cancel(CHECKOUT, &plain.id).unwrap_err(),
        JobError::Unsupported("cancel")
    ));
    fx.store.stop_watching(CHECKOUT, &plain.id).unwrap();
    assert_eq!(gate(&fx), GateState::Cancelled);
    assert_eq!(
        api.log().len(),
        0,
        "stop watching never contacts the provider"
    );

    let options = CiWatchOptions {
        ticket: None,
        can_cancel: true,
    };
    let job = rewatch_ci(&fx.store, CHECKOUT, &target(), &options).unwrap();
    assert!(job.capabilities.cancel);
    api.runs(vec![run(9, SHA, "in_progress", None)]);
    api.jobs(
        9,
        vec![workflow_job(
            91,
            "test",
            "in_progress",
            None,
            &["in_progress"],
        )],
    );
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();

    // The provider refuses: the Job keeps running and records why.
    api.serve(
        cancel_path,
        403,
        json!({"message": "Must have admin rights"}),
    );
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    monitor.tick().unwrap();
    let refused = fx.get(&job.id);
    assert_eq!(refused.status, Status::Running);
    assert_eq!(refused.cancel_requested_ms, None);
    assert_eq!(
        refused.cancel_failure.as_deref(),
        Some("GitHub did not permit cancelling this run")
    );

    // The provider accepts: the Job is cancelled when the run reports it.
    api.serve(cancel_path, 202, json!({}));
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Running);
    let posts: Vec<ApiRequest> = api
        .log()
        .into_iter()
        .filter(|request| request.method == ApiMethod::Post)
        .collect();
    assert_eq!(posts.len(), 2);
    assert!(posts.iter().all(|post| post.path == cancel_path));
    api.runs(vec![run(9, SHA, "completed", Some("cancelled"))]);
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&job.id).status, Status::Cancelled);
    assert_eq!(gate(&fx), GateState::Cancelled);
}

#[test]
fn a_covering_run_of_a_newer_commit_is_never_cancelled() {
    let fx = fixture();
    let api = FakeApi::default();
    let options = CiWatchOptions {
        ticket: None,
        can_cancel: true,
    };
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &options).unwrap();
    api.runs(vec![
        run(12, FIX, "in_progress", None),
        run(9, SHA, "completed", Some("cancelled")),
    ]);
    api.compare(FIX, "ahead");
    api.jobs(12, vec![]);
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    monitor.tick().unwrap();
    let after = fx.get(&job.id);
    assert_eq!(after.status, Status::Running);
    assert_eq!(
        after.cancel_failure.as_deref(),
        Some("This operation cannot be cancelled from here")
    );
    assert!(
        api.log()
            .iter()
            .all(|request| request.method == ApiMethod::Get),
        "no cancel was sent for someone else's run"
    );
}

#[test]
fn provider_failures_are_retried_with_backoff_and_a_missing_repository_fails_the_job() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    let mut monitor = monitor(&fx, &api);

    // No answer at all, then a spent rate limit, then a body that is not the API's shape.
    api.set_down(true);
    let tick = monitor.tick().unwrap();
    let retrying = fx.get(&job.id);
    assert_eq!(retrying.status, Status::Waiting);
    assert_eq!(retrying.latest[WATCH_STATE_KEY], "retrying");
    assert_eq!(retrying.liveness(fx.store.now_ms()), Liveness::Live);
    assert_eq!(gate(&fx), GateState::Pending);
    assert!(tick.next_wake_ms <= fx.store.now_ms() + 2 * POLL_MS);

    api.set_down(false);
    api.serve_raw(
        RUNS,
        Route {
            status: 403,
            etag: None,
            retry_after_ms: Some(60_000),
            body: json!({"message": "API rate limit exceeded"}).to_string(),
        },
    );
    fx.clock.advance(2 * POLL_MS);
    monitor.tick().unwrap();
    let limited = api.count("actions/runs?");
    // The provider asked for a minute: no request is made before it has passed, while
    // lease renewals keep the Job live.
    for _ in 0..50 {
        fx.clock.advance(1_000);
        monitor.tick().unwrap();
        assert_eq!(fx.get(&job.id).liveness(fx.store.now_ms()), Liveness::Live);
    }
    assert_eq!(api.count("actions/runs?"), limited);

    api.serve_raw(
        RUNS,
        Route {
            status: 200,
            etag: Some("W/\"odd\"".into()),
            retry_after_ms: None,
            body: "<html>maintenance</html>".into(),
        },
    );
    fx.clock.advance(10_000);
    monitor.tick().unwrap();
    assert_eq!(api.count("actions/runs?"), limited + 1);
    assert_eq!(fx.get(&job.id).latest[WATCH_STATE_KEY], "retrying");

    // Recovery clears the retry state.
    api.runs(vec![run(9, SHA, "in_progress", None)]);
    api.jobs(9, vec![]);
    fx.clock.advance(10_000);
    monitor.tick().unwrap();
    let recovered = fx.get(&job.id);
    assert_eq!(recovered.status, Status::Running);
    assert!(!recovered.latest.contains_key(WATCH_STATE_KEY));

    // The repository is gone, or these credentials cannot read it.
    api.serve(RUNS, 404, json!({"message": "Not Found"}));
    fx.clock.advance(10_000);
    monitor.tick().unwrap();
    let gone = fx.get(&job.id);
    assert_eq!(gone.status, Status::Failed);
    assert_eq!(gone.outcome.unwrap().refs[WATCH_STATE_KEY], "gone");
    assert_eq!(
        gate(&fx),
        GateState::Unknown,
        "that failure says nothing about the commit"
    );
    // HS2-507DGZ: and nothing watches it any more, unlike a lost watch.
    assert!(gate_state(&fx.store, CHECKOUT, &target()).unwrap().ended);
}

#[test]
fn hostile_provider_text_cannot_break_the_record() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    let mut own = run(9, SHA, "in_progress", None);
    own["html_url"] = json!("javascript:alert(1)");
    api.runs(vec![own]);
    let mut jobs: Vec<Value> = (0..600)
        .map(|index| workflow_job(index, "matrix", "queued", None, &[]))
        .collect();
    jobs[0] = workflow_job(
        0,
        &format!("line one\nline two {}", "x".repeat(900)),
        "in_progress",
        None,
        &["completed"],
    );
    jobs[1] = workflow_job(1, "\u{7}", "in_progress", None, &[]);
    jobs[2] = json!({"id": 2});
    api.jobs(9, jobs);
    monitor(&fx, &api).tick().unwrap();

    let running = fx.get(&job.id);
    assert_eq!(running.status, Status::Running, "the report was accepted");
    assert_eq!(running.url, None, "only https links are published");
    assert_eq!(running.steps.len(), 512);
    assert!(running.steps[0].label.starts_with("line one line two"));
    assert_eq!(running.steps[0].label.len(), 200);
    assert_eq!(running.steps[1].label, "unnamed");
    assert_eq!(running.steps[2].label, "job");
    assert!(!running.latest.contains_key(WATCH_STATE_KEY));
}

#[test]
fn ensure_is_idempotent_across_concurrent_callers_and_distinct_per_target() {
    let fx = fixture();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (store, barrier) = (fx.store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                ensure_ci_watch(&store, CHECKOUT, &target(), &CiWatchOptions::default())
                    .unwrap()
                    .id
            })
        })
        .collect();
    let ids: std::collections::BTreeSet<String> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(ids.len(), 1, "every caller got the same Job");
    assert_eq!(fx.store.list(CHECKOUT).unwrap().jobs.len(), 1);

    // Another commit, branch or workflow is another gate.
    let others = [
        CiTarget::new("acme/widgets", FIX, "CI", "main").unwrap(),
        CiTarget::new("acme/widgets", SHA, "CI", "integration").unwrap(),
        CiTarget::new("acme/widgets", SHA, "Platforms", "main").unwrap(),
        CiTarget::new("acme/gadgets", SHA, "CI", "main").unwrap(),
    ];
    for other in &others {
        let job = ensure_ci_watch(&fx.store, CHECKOUT, other, &CiWatchOptions::default()).unwrap();
        assert!(!ids.contains(&job.id));
        assert_eq!(
            find_ci_watch(&fx.store, CHECKOUT, other)
                .unwrap()
                .unwrap()
                .id,
            job.id
        );
    }
    assert_eq!(fx.store.list(CHECKOUT).unwrap().jobs.len(), 5);
    let unwatched = CiTarget::new("acme/widgets", OLD, "CI", "main").unwrap();
    assert_eq!(
        find_ci_watch(&fx.store, CHECKOUT, &unwatched).unwrap(),
        None
    );
    assert_eq!(
        gate_state(&fx.store, CHECKOUT, &unwatched).unwrap().state,
        GateState::Unknown
    );
}

#[test]
fn a_lost_watch_reads_unknown_and_resumes_under_the_same_job() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.runs(vec![run(9, SHA, "in_progress", None)]);
    api.jobs(9, vec![]);
    monitor(&fx, &api).tick().unwrap();
    assert_eq!(gate(&fx), GateState::Pending);

    // The monitor died and the grace window passed.
    fx.clock.advance(LEASE_MS + LOST_AFTER_MS);
    fx.store.reconcile(CHECKOUT, &NoProbe).unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Lost);
    assert_eq!(gate(&fx), GateState::Unknown);
    // HS2-507DGZ: a lost watch has not ended: the monitor reattaches it.
    assert!(!gate_state(&fx.store, CHECKOUT, &target()).unwrap().ended);
    // Neither call starts a second Job: the monitor reattaches to this one.
    let options = CiWatchOptions::default();
    assert_eq!(
        ensure_ci_watch(&fx.store, CHECKOUT, &target(), &options)
            .unwrap()
            .id,
        job.id
    );
    assert_eq!(
        rewatch_ci(&fx.store, CHECKOUT, &target(), &options)
            .unwrap()
            .id,
        job.id
    );

    api.runs(vec![run(9, SHA, "completed", Some("success"))]);
    let mut next = Monitor::new(fx.store.clone(), "monitor-b", MonitorConfig::default());
    next.register(Box::new(GithubActionsSource::new(api.clone())));
    next.tick().unwrap();
    let done = fx.get(&job.id);
    assert_eq!(
        (done.status, done.lease.unwrap().generation),
        (Status::Succeeded, 2)
    );
    assert!(matches!(gate(&fx), GateState::Green { .. }));
}

#[test]
fn a_caller_waits_for_the_gate_without_polling() {
    let fx = fixture();
    let api = FakeApi::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();

    // Nothing changes: the wait ends at its timeout and reports the same state.
    let started = Instant::now();
    let reading = wait_for_gate_change(
        &fx.store,
        CHECKOUT,
        &target(),
        &GateState::Pending,
        Duration::from_millis(200),
    )
    .unwrap();
    assert_eq!(reading.state, GateState::Pending);
    assert!(started.elapsed() >= Duration::from_millis(200));

    // The monitor reports progress first (still pending) and the verdict later. The
    // caller sleeps through the first and wakes on the second.
    api.runs(vec![run(9, SHA, "in_progress", None)]);
    api.jobs(9, vec![]);
    let observer = {
        let (fx_store, api) = (fx.store.clone(), api.clone());
        let clock = fx.clock.clone();
        std::thread::spawn(move || {
            let mut monitor = Monitor::new(
                fx_store,
                "monitor-a",
                MonitorConfig {
                    poll_ms: POLL_MS,
                    ..MonitorConfig::default()
                },
            );
            monitor.register(Box::new(GithubActionsSource::new(api.clone())));
            std::thread::sleep(Duration::from_millis(100));
            monitor.tick().unwrap();
            std::thread::sleep(Duration::from_millis(100));
            api.runs(vec![run(9, SHA, "completed", Some("success"))]);
            clock.advance(POLL_MS);
            monitor.tick().unwrap();
        })
    };
    let started = Instant::now();
    let reading = wait_for_gate_change(
        &fx.store,
        CHECKOUT,
        &target(),
        &GateState::Pending,
        Duration::from_secs(60),
    )
    .unwrap();
    observer.join().unwrap();
    assert!(
        matches!(reading.state, GateState::Green { .. }),
        "{:?}",
        reading.state
    );
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(reading.job_id.is_some());

    // Already different from what the caller last saw: no wait at all.
    let started = Instant::now();
    let reading = wait_for_gate_change(
        &fx.store,
        CHECKOUT,
        &target(),
        &GateState::Pending,
        Duration::from_secs(60),
    )
    .unwrap();
    assert!(reading.state.is_final());
    assert!(started.elapsed() < Duration::from_secs(30));
}

fn artifact(name: &str, digest: Option<&str>, expired: bool) -> Value {
    json!({
        "id": 7,
        "name": name,
        "size_in_bytes": 1024,
        "digest": digest,
        "expired": expired,
        "archive_download_url": "https://github.example.test/artifacts/7/zip"
    })
}

const ARTIFACTS: &str = "repos/acme/widgets/actions/runs/9/artifacts?per_page=100";

fn green_gate_with(artifacts: Option<(u16, Value)>) -> (GateState, BTreeMap<String, String>) {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.runs(vec![run(9, SHA, "completed", Some("success"))]);
    api.jobs(9, vec![]);
    if let Some((status, body)) = artifacts {
        api.serve(ARTIFACTS, status, body);
    }
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();
    let refs = fx
        .get(&job.id)
        .outcome
        .map(|outcome| outcome.refs)
        .unwrap_or_default();
    (gate(&fx), refs)
}

#[test]
fn hs2_3yc577_a_green_gate_records_the_one_artifact_its_run_built() {
    let digest = "AB".repeat(32);
    let list = |artifacts: Vec<Value>| {
        (
            200,
            json!({"total_count": artifacts.len(), "artifacts": artifacts}),
        )
    };
    // One usable artifact among an expired one and one without a digest.
    let (state, refs) = green_gate_with(Some(list(vec![
        artifact(
            "old-build",
            Some(&format!("sha256:{}", "c".repeat(64))),
            true,
        ),
        artifact("report", None, false),
        artifact("widgets-bundle", Some(&format!("sha256:{digest}")), false),
    ])));
    let GateState::Green { run: green } = state else {
        panic!("a green run is green: {state:?}");
    };
    assert_eq!(
        green.artifact,
        Some(CiArtifact {
            name: "widgets-bundle".into(),
            digest: digest.to_ascii_lowercase(),
        })
    );
    assert_eq!(refs["artifact_count"], "1");
    assert_eq!(refs["artifact_digest"], digest.to_ascii_lowercase());

    // Two candidates: nothing says which one ships, so none is recorded.
    let (state, refs) = green_gate_with(Some(list(vec![
        artifact("a", Some(&format!("sha256:{}", "a".repeat(64))), false),
        artifact("b", Some(&format!("sha256:{}", "b".repeat(64))), false),
    ])));
    assert!(matches!(state, GateState::Green { run } if run.artifact.is_none()));
    assert_eq!(refs["artifact_count"], "2");
    assert!(!refs.contains_key("artifact_digest"));

    // A digest that is not SHA-256 does not identify anything.
    let (state, refs) = green_gate_with(Some(list(vec![artifact("a", Some("md5:0123"), false)])));
    assert!(matches!(state, GateState::Green { run } if run.artifact.is_none()));
    assert_eq!(refs["artifact_count"], "0");

    // Unreadable with these credentials, or not there: the verdict stands without it.
    for status in [403, 404] {
        let (state, refs) = green_gate_with(Some((status, json!({"message": "no"}))));
        assert!(matches!(state, GateState::Green { run } if run.artifact.is_none()));
        assert_eq!(refs["artifact_count"], "0");
    }
    let (state, _) = green_gate_with(None);
    assert!(matches!(state, GateState::Green { .. }));

    // A red run reads no artifacts at all.
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.runs(vec![run(9, SHA, "completed", Some("failure"))]);
    api.jobs(9, vec![]);
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx), GateState::Red { run } if run.artifact.is_none()));
    assert_eq!(api.count("/artifacts"), 0);
    let refs = fx.get(&job.id).outcome.unwrap().refs;
    assert!(!refs.contains_key("artifact_count"));
}

#[test]
fn hs2_3yc577_an_artifact_read_that_fails_is_retried_before_the_verdict() {
    let fx = fixture();
    let api = FakeApi::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.runs(vec![run(9, SHA, "completed", Some("success"))]);
    api.jobs(9, vec![]);
    api.serve(ARTIFACTS, 502, json!({"message": "Server Error"}));
    let mut monitor = monitor(&fx, &api);
    monitor.tick().unwrap();
    assert!(
        !fx.get(&job.id).status.is_finished(),
        "a provider failure is not a verdict without the artifact"
    );
    let digest = "d".repeat(64);
    api.serve(
        ARTIFACTS,
        200,
        json!({"total_count": 1, "artifacts": [artifact("bundle", Some(&format!("sha256:{digest}")), false)]}),
    );
    for _ in 0..8 {
        if fx.get(&job.id).status.is_finished() {
            break;
        }
        step(&fx, &mut monitor);
    }
    assert!(matches!(
        gate(&fx),
        GateState::Green { run } if run.artifact.as_ref().map(|a| a.digest.as_str()) == Some(digest.as_str())
    ));
    assert_eq!(
        api.count("/artifacts"),
        2,
        "read once more after the failure, then never"
    );
}

#[test]
fn hs2_tqg6at_a_named_artifact_is_recorded_among_several() {
    let named = target().with_artifact(Some("widgets-bundle")).unwrap();
    // The name travels in the watch Job's external reference; an unnamed one keeps the
    // five-part spelling earlier Jobs carry.
    assert_eq!(CiTarget::parse(&named.external(None)).unwrap().0, named);
    assert_eq!(target().external(None).id.split('|').count(), 5);
    assert_eq!(
        CiTarget::parse(&target().external(None)).unwrap().0,
        target()
    );
    assert!(target().with_artifact(Some("not a name")).is_err());
    assert!(target().with_artifact(Some("")).is_err());

    let digest = |c: char| format!("sha256:{}", c.to_string().repeat(64));
    let artifacts = json!({"total_count": 3, "artifacts": [
        artifact("test-report", Some(&digest('a')), false),
        artifact("widgets-bundle", Some(&digest('b')), false),
        artifact("coverage", Some(&digest('c')), false),
    ]});
    let judge = |watched: &CiTarget| {
        let fx = fixture();
        let api = FakeApi::default();
        let job =
            ensure_ci_watch(&fx.store, CHECKOUT, watched, &CiWatchOptions::default()).unwrap();
        api.runs(vec![run(9, SHA, "completed", Some("success"))]);
        api.jobs(9, vec![]);
        api.serve(ARTIFACTS, 200, artifacts.clone());
        let mut monitor = monitor(&fx, &api);
        monitor.tick().unwrap();
        let state = gate_state(&fx.store, CHECKOUT, watched).unwrap().state;
        (state, fx.get(&job.id).outcome.unwrap().refs)
    };
    let (state, refs) = judge(&named);
    let GateState::Green { run: green } = state else {
        panic!("green: {state:?}");
    };
    assert_eq!(
        green.artifact,
        Some(CiArtifact {
            name: "widgets-bundle".into(),
            digest: "b".repeat(64),
        })
    );
    assert_eq!(refs["artifact_count"], "3");
    // Unnamed, several record none; a name the run did not build records none.
    let (state, _) = judge(&target());
    assert!(matches!(state, GateState::Green { run } if run.artifact.is_none()));
    let missing = target().with_artifact(Some("installer")).unwrap();
    let (state, refs) = judge(&missing);
    assert!(matches!(state, GateState::Green { run } if run.artifact.is_none()));
    assert!(!refs.contains_key("artifact_digest"));
}
