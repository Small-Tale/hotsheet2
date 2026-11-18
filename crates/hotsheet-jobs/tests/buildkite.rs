//! HS2-17HVNP: the Buildkite watch source and the CI gate API for `buildkite`, offline.
//! A scripted REST port stands in for Buildkite's REST v2 with the JSON shapes and status
//! codes its documentation gives (<https://buildkite.com/docs/apis/rest-api/builds>:
//! "List builds for a pipeline" filtered by `branch` and `commit`, each build with
//! `number`, `state`, `commit`, `branch`, `pull_request`, `web_url` and `jobs`; "Cancel a
//! build" is `PUT .../builds/{number}/cancel`). No test makes a network call. Synthetic
//! data only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::{CHECKOUT, Fixture, fixture};
use hotsheet_jobs::{
    ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, BUILDKITE_KIND, BuildkiteSource,
    CiProvider, CiRun, CiTarget, CiWatchOptions, GateState, GithubApi, Monitor, MonitorConfig,
    Status, StepStatus, ci_outcome, ensure_ci_watch, gate_state, rewatch_ci,
};
use serde_json::{Value, json};

const SHA: &str = "1111111111111111111111111111111111111111";
const NEWER: &str = "2222222222222222222222222222222222222222";
const PIPELINE: &str = "organizations/acme/pipelines/widgets";

#[derive(Default)]
struct Fake {
    routes: BTreeMap<String, (u16, String)>,
    log: Vec<ApiRequest>,
}

#[derive(Clone, Default)]
struct FakeBuildkite(Arc<Mutex<Fake>>);

impl FakeBuildkite {
    fn serve(&self, path: &str, status: u16, body: Value) {
        self.0
            .lock()
            .unwrap()
            .routes
            .insert(path.to_owned(), (status, body.to_string()));
    }

    /// What `GET .../builds?branch=main&commit=<sha>` answers.
    fn builds(&self, builds: Vec<Value>) {
        self.serve(
            &format!("{PIPELINE}/builds?branch=main&commit={SHA}&per_page=30"),
            200,
            Value::Array(builds),
        );
    }

    fn puts(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|request| request.method == ApiMethod::Put)
            .map(|request| request.path.clone())
            .collect()
    }
}

impl GithubApi for FakeBuildkite {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let mut fake = self.0.lock().unwrap();
        fake.log.push(request.clone());
        let Some((status, body)) = fake.routes.get(&request.path).cloned() else {
            return Ok(ApiResponse {
                status: 404,
                etag: None,
                retry_after_ms: None,
                body: json!({"message": "No pipeline found"}).to_string(),
            });
        };
        Ok(ApiResponse {
            status,
            etag: None,
            retry_after_ms: (status == 429).then_some(30_000),
            body,
        })
    }
}

fn build_job(id: &str, kind: &str, name: Option<&str>, state: &str) -> Value {
    json!({
        "id": id, "graphql_id": "synthetic", "type": kind, "name": name, "step_key": name,
        "state": state, "web_url": null, "agent": null
    })
}

fn build(number: u64, commit: &str, state: &str, jobs: Value) -> Value {
    json!({
        "id": format!("0190c1a2-0000-7000-8000-{number:012}"),
        "graphql_id": "synthetic",
        "url": format!("https://api.buildkite.com/v2/{PIPELINE}/builds/{number}"),
        "web_url": format!("https://buildkite.com/acme/widgets/builds/{number}"),
        "number": number, "state": state, "blocked": false, "message": "Synthetic change",
        "commit": commit, "branch": "main", "tag": null, "env": {}, "source": "webhook",
        "creator": null, "jobs": jobs, "created_at": "2026-11-01T10:00:00.000Z",
        "scheduled_at": "2026-11-01T10:00:00.000Z", "started_at": null, "finished_at": null,
        "meta_data": {}, "pull_request": null, "rebuilt_from": null,
        "pipeline": {"id": "synthetic", "slug": "widgets"}
    })
}

fn target() -> CiTarget {
    CiTarget::buildkite("acme/widgets", SHA, "*", "main").unwrap()
}

fn watcher(fx: &Fixture, api: &FakeBuildkite) -> Monitor {
    let mut monitor = Monitor::new(
        fx.store.clone(),
        "monitor-a",
        MonitorConfig {
            poll_ms: 100,
            max_poll_ms: 400,
            max_error_ms: 1_600,
            idle_exit_ms: 2_000,
        },
    );
    monitor.register(Box::new(BuildkiteSource::new(api.clone())));
    monitor
}

fn step(fx: &Fixture, monitor: &mut Monitor) {
    fx.clock.advance(400);
    monitor.tick().unwrap();
}

fn gate(fx: &Fixture) -> GateState {
    gate_state(&fx.store, CHECKOUT, &target()).unwrap().state
}

#[test]
fn hs2_17hvnp_a_buildkite_target_names_its_pipeline_and_attests_exact_coverage_only() {
    let target = target();
    assert_eq!(target.provider, CiProvider::Buildkite);
    assert!(!target.provider.attests_ancestors());
    let external = target.external(None);
    assert_eq!(external.system, BUILDKITE_KIND);
    assert_eq!(CiTarget::parse(&external).unwrap().0, target);
    assert_eq!(target.job_key(), "buildkite:acme/widgets:main:*");
    for refused in [
        "widgets",
        "acme/Widgets",
        "acme/wid gets",
        "acme/widgets/x",
        "/widgets",
    ] {
        assert!(
            CiTarget::buildkite(refused, SHA, "*", "main").is_err(),
            "{refused}"
        );
    }
    // A Buildkite build has no workflow of its own.
    assert!(CiTarget::buildkite("acme/widgets", SHA, "CI", "main").is_err());
    assert_eq!(
        CiTarget::for_provider(CiProvider::Buildkite, "acme/widgets", SHA, "*", "main").unwrap(),
        target
    );
}

#[test]
fn hs2_17hvnp_a_commits_build_is_followed_to_green_without_client_polling() {
    let fx = fixture();
    let api = FakeBuildkite::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    assert_eq!(job.kind.name, BUILDKITE_KIND);
    assert_eq!(gate(&fx), GateState::Pending);

    // No build yet, then a scheduled one.
    api.builds(vec![]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Waiting);
    api.builds(vec![build(
        41,
        SHA,
        "scheduled",
        json!([build_job(
            "a1",
            "script",
            Some(":hammer: build"),
            "scheduled"
        )]),
    )]);
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    // Running: its jobs are the steps; a wait step is not one.
    api.builds(vec![build(
        41,
        SHA,
        "running",
        json!([
            build_job("a1", "script", Some(":hammer: build"), "passed"),
            build_job("a2", "waiter", None, "passed"),
            build_job("a3", "script", Some(":test_tube: test"), "running"),
        ]),
    )]);
    step(&fx, &mut monitor);
    let running = fx.get(&job.id);
    assert_eq!(running.status, Status::Running);
    let steps: Vec<(&str, StepStatus)> = running
        .steps
        .iter()
        .map(|step| (step.label.as_str(), step.status))
        .collect();
    assert_eq!(
        steps,
        [
            (":hammer: build", StepStatus::Succeeded),
            (":test_tube: test", StepStatus::Active)
        ]
    );
    assert_eq!(running.latest["current_step"], ":test_tube: test");
    assert_eq!(running.latest["run_id"], "41");

    // The same answer again rewrites nothing.
    let revision = fx.get(&job.id).revision;
    fx.clock.advance(100);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).revision, revision);

    api.builds(vec![build(41, SHA, "passed", json!([]))]);
    step(&fx, &mut monitor);
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Succeeded);
    let run = CiRun {
        id: 41,
        attempt: 1,
        head_sha: SHA.into(),
        url: Some("https://buildkite.com/acme/widgets/builds/41".into()),
        artifact: None,
    };
    assert_eq!(gate(&fx), GateState::Green { run: run.clone() });
    let outcome = ci_outcome(&done).unwrap();
    assert_eq!(outcome.gate, GateState::Green { run });
    assert_eq!(
        outcome.summary.as_deref(),
        Some("Buildkite build 41 passed for 1111111")
    );
    assert_eq!(
        done.outcome.as_ref().unwrap().refs["run_conclusion"],
        "passed"
    );
}

#[test]
fn hs2_17hvnp_only_a_branch_build_of_the_commit_itself_counts() {
    let fx = fixture();
    let api = FakeBuildkite::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    // A passed build of a later commit, a pull request build and another branch's build
    // never cover the commit: no ancestry is known.
    let mut pull = build(52, SHA, "passed", json!([]));
    pull["pull_request"] = json!({"id": "7", "base": "main", "repository": "acme/widgets"});
    let mut branch = build(53, SHA, "passed", json!([]));
    branch["branch"] = json!("feature");
    api.builds(vec![build(54, NEWER, "passed", json!([])), pull, branch]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx), GateState::Pending);
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    // An older passed build of the commit wins over a newer failing rebuild of it.
    api.builds(vec![
        build(56, SHA, "failed", json!([])),
        build(55, SHA, "passed", json!([])),
    ]);
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 55 && run.head_sha == SHA));
}

#[test]
fn hs2_17hvnp_red_is_not_judged_again_and_cancel_reaches_only_the_commits_build() {
    let fx = fixture();
    let api = FakeBuildkite::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.builds(vec![build(61, SHA, "failed", json!([]))]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx), GateState::Red { run } if run.id == 61));
    rewatch_ci(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    step(&fx, &mut monitor);
    assert_eq!(
        gate(&fx),
        GateState::Pending,
        "the judged build is not judged again"
    );
    // A rebuild is a new build number: judged.
    api.builds(vec![
        build(62, SHA, "passed", json!([])),
        build(61, SHA, "failed", json!([])),
    ]);
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 62));

    // Cancel: a PUT to the commit's own unfinished build, once asked.
    let fx = fixture();
    let api = FakeBuildkite::default();
    let job = ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target(),
        &CiWatchOptions {
            ticket: None,
            can_cancel: true,
        },
    )
    .unwrap();
    api.builds(vec![build(81, SHA, "running", json!([]))]);
    api.serve(
        &format!("{PIPELINE}/builds/81/cancel"),
        200,
        build(81, SHA, "canceling", json!([])),
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(api.puts().is_empty());
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    step(&fx, &mut monitor);
    assert_eq!(api.puts(), [format!("{PIPELINE}/builds/81/cancel")]);
    api.builds(vec![build(81, SHA, "canceled", json!([]))]);
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx), GateState::Cancelled);
}

#[test]
fn hs2_17hvnp_a_rate_limit_keeps_the_last_observation() {
    let fx = fixture();
    let api = FakeBuildkite::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.builds(vec![build(41, SHA, "running", json!([]))]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    api.serve(
        &format!("{PIPELINE}/builds?branch=main&commit={SHA}&per_page=30"),
        429,
        json!({"message": "You have exceeded your API rate limit"}),
    );
    step(&fx, &mut monitor);
    // A spent limit is a transient answer: the build is still running, nothing judged.
    let job = fx.get(&job.id);
    assert_eq!(job.status, Status::Running);
    assert_eq!(job.latest["run_id"], "41");
    assert!(job.outcome.is_none());
    assert_eq!(gate(&fx), GateState::Pending);
}
