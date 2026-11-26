//! HS2-17HVNP: the Buildkite watch source and the CI gate API for `buildkite`, offline.
//! A scripted REST port stands in for Buildkite's REST v2 with the JSON shapes and status
//! codes its documentation gives (<https://buildkite.com/docs/apis/rest-api/builds>:
//! "List builds for a pipeline" filtered by `branch` and `commit`, each build with
//! `number`, `state`, `commit`, `branch`, `pull_request`, `web_url` and `jobs`; "Cancel a
//! build" is `PUT .../builds/{number}/cancel`). HS2-56MTYN: "Get a pipeline" answers
//! its `repository` URL, and a second scripted port stands in for GitHub's "Compare two
//! commits" (`GET repos/{owner}/{repo}/compare/{base}...{head}`, `status` `ahead`,
//! `behind`, `identical` or `diverged`, 404 for an unknown commit). No test makes a
//! network call. Synthetic data only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::{CHECKOUT, Fixture, fixture};
use hotsheet_jobs::{
    ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, BUILDKITE_KIND, BuildkiteSource,
    CiProvider, CiRun, CiTarget, CiWatchOptions, GateState, GithubApi, Monitor, MonitorConfig,
    RepositoryHosts, Status, StepStatus, ci_outcome, ensure_ci_watch, gate_state, rewatch_ci,
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

// ---- HS2-56MTYN: ancestry through the repository host ----

const OLDER: &str = "3333333333333333333333333333333333333333";
const OTHER: &str = "4444444444444444444444444444444444444444";

/// The pipeline's own answer, naming the repository it builds.
fn pipeline_repository(api: &FakeBuildkite, repository: &str) {
    api.serve(
        PIPELINE,
        200,
        json!({
            "id": "synthetic", "slug": "widgets", "name": "Widgets",
            "repository": repository, "default_branch": "main",
            "provider": {"id": "github", "settings": {}}
        }),
    );
}

/// What `GET .../builds?branch=main` (every commit) answers.
fn branch_builds(api: &FakeBuildkite, builds: Vec<Value>) {
    api.serve(
        &format!("{PIPELINE}/builds?branch=main&per_page=30"),
        200,
        Value::Array(builds),
    );
}

/// GitHub's compare of the commit with `head`.
fn compare(github: &FakeBuildkite, head: &str, status: &str) {
    github.serve(
        &format!("repos/acme/widgets/compare/{SHA}...{head}?per_page=1"),
        200,
        json!({"status": status, "ahead_by": 1, "behind_by": 0, "total_commits": 1}),
    );
}

fn compares(github: &FakeBuildkite) -> usize {
    github.0.lock().unwrap().log.len()
}

fn hosted_watcher(fx: &Fixture, api: &FakeBuildkite, github: &FakeBuildkite) -> Monitor {
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
    monitor.register(Box::new(
        BuildkiteSource::new(api.clone())
            .with_hosts(RepositoryHosts::new().with_github(github.clone())),
    ));
    monitor
}

#[test]
fn hs2_56mtyn_a_later_passed_build_covers_the_commit_as_an_ancestor() {
    let fx = fixture();
    let api = FakeBuildkite::default();
    let github = FakeBuildkite::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    pipeline_repository(&api, "git@github.com:acme/widgets.git");
    // The commit's own build still runs; a later commit's build, containing it, passed.
    api.builds(vec![build(41, SHA, "running", json!([]))]);
    branch_builds(
        &api,
        vec![
            build(42, NEWER, "passed", json!([])),
            build(41, SHA, "running", json!([])),
        ],
    );
    compare(&github, NEWER, "ahead");
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    let run = CiRun {
        id: 42,
        attempt: 1,
        head_sha: NEWER.into(),
        url: Some("https://buildkite.com/acme/widgets/builds/42".into()),
        artifact: None,
    };
    assert_eq!(gate(&fx), GateState::Green { run: run.clone() });
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Succeeded);
    assert_eq!(ci_outcome(&done).unwrap().gate, GateState::Green { run });
    assert_eq!(compares(&github), 1);
    // The pipeline was read once, besides the two build lists.
    let reads: Vec<String> = api
        .0
        .lock()
        .unwrap()
        .log
        .iter()
        .map(|request| request.path.clone())
        .collect();
    assert_eq!(reads.iter().filter(|path| *path == PIPELINE).count(), 1);

    // A later red build covering the commit is the newest verdict when nothing passed;
    // a rewatch ignores it, and the commit's own later pass is green.
    let fx = fixture();
    let api = FakeBuildkite::default();
    let github = FakeBuildkite::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    pipeline_repository(&api, "https://github.com/acme/widgets");
    api.builds(vec![]);
    branch_builds(&api, vec![build(50, NEWER, "failed", json!([]))]);
    compare(&github, NEWER, "ahead");
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx), GateState::Red { run } if run.id == 50 && run.head_sha == NEWER));
    rewatch_ci(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx), GateState::Pending);
    api.builds(vec![build(51, SHA, "passed", json!([]))]);
    branch_builds(
        &api,
        vec![
            build(51, SHA, "passed", json!([])),
            build(50, NEWER, "failed", json!([])),
        ],
    );
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 51 && run.head_sha == SHA));
    // The relation was asked once and kept.
    assert_eq!(compares(&github), 1);
}

#[test]
fn hs2_56mtyn_an_older_or_unknown_head_never_covers_and_another_line_supersedes() {
    // A build of an older commit is not evidence for this one.
    let fx = fixture();
    let api = FakeBuildkite::default();
    let github = FakeBuildkite::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    pipeline_repository(&api, "https://github.com/acme/widgets.git");
    api.builds(vec![]);
    branch_builds(&api, vec![build(39, OLDER, "passed", json!([]))]);
    compare(&github, OLDER, "behind");
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Waiting);
    assert_eq!(gate(&fx), GateState::Pending);

    // A head GitHub does not know yet is asked again on the next poll, then covers.
    branch_builds(&api, vec![build(43, NEWER, "passed", json!([]))]);
    github.serve(
        &format!("repos/acme/widgets/compare/{SHA}...{NEWER}?per_page=1"),
        404,
        json!({"message": "Not Found"}),
    );
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx), GateState::Pending);
    let asked = compares(&github);
    step(&fx, &mut monitor);
    assert_eq!(compares(&github), asked + 1);
    compare(&github, NEWER, "identical");
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 43));

    // The branch moved to another line and nothing covers the commit: superseded.
    let fx = fixture();
    let api = FakeBuildkite::default();
    let github = FakeBuildkite::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    pipeline_repository(&api, "https://github.com/acme/widgets.git");
    api.builds(vec![]);
    branch_builds(&api, vec![build(60, OTHER, "passed", json!([]))]);
    compare(&github, OTHER, "diverged");
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx), GateState::Superseded);
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Failed);
    assert_eq!(done.outcome.unwrap().refs["gate"], "superseded");

    // Another line on top does not hide the commit's own pass.
    let fx = fixture();
    let api = FakeBuildkite::default();
    let github = FakeBuildkite::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    pipeline_repository(&api, "https://github.com/acme/widgets.git");
    api.builds(vec![build(59, SHA, "passed", json!([]))]);
    branch_builds(&api, vec![build(60, OTHER, "failed", json!([]))]);
    compare(&github, OTHER, "diverged");
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 59));
}

#[test]
fn hs2_56mtyn_without_a_host_for_the_repository_the_gate_keeps_exact_coverage() {
    // No hosts given: the pipeline is not read, and a later pass never covers.
    let fx = fixture();
    let api = FakeBuildkite::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    pipeline_repository(&api, "https://github.com/acme/widgets.git");
    api.builds(vec![]);
    branch_builds(&api, vec![build(42, NEWER, "passed", json!([]))]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx), GateState::Pending);
    assert!(
        api.0
            .lock()
            .unwrap()
            .log
            .iter()
            .all(|request| request.path.contains("commit="))
    );

    // A repository on a host without a port (Bitbucket), or a token that may not read
    // pipelines (403): exact coverage, and the pipeline is read once.
    for (status, repository) in [
        (200, "git@bitbucket.org:acme/widgets.git"),
        (403, "https://github.com/acme/widgets.git"),
    ] {
        let fx = fixture();
        let api = FakeBuildkite::default();
        let github = FakeBuildkite::default();
        ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
        api.serve(PIPELINE, status, json!({"repository": repository}));
        api.builds(vec![]);
        branch_builds(&api, vec![build(42, NEWER, "passed", json!([]))]);
        compare(&github, NEWER, "ahead");
        let mut monitor = hosted_watcher(&fx, &api, &github);
        monitor.tick().unwrap();
        step(&fx, &mut monitor);
        assert_eq!(gate(&fx), GateState::Pending, "{status}");
        assert_eq!(compares(&github), 0);
        let log = api.0.lock().unwrap().log.clone();
        assert_eq!(log.iter().filter(|r| r.path == PIPELINE).count(), 1);
        // Exact coverage still judges the commit's own build.
        api.builds(vec![build(43, SHA, "passed", json!([]))]);
        step(&fx, &mut monitor);
        assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 43));
    }

    // A spent limit or a server error on the pipeline or the compare is retried.
    let fx = fixture();
    let api = FakeBuildkite::default();
    let github = FakeBuildkite::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.builds(vec![]);
    branch_builds(&api, vec![build(42, NEWER, "passed", json!([]))]);
    let mut monitor = hosted_watcher(&fx, &api, &github);
    for status in [429, 503] {
        api.serve(PIPELINE, status, json!({"message": "synthetic"}));
        fx.clock.advance(30_000);
        monitor.tick().unwrap();
        assert!(fx.get(&job.id).outcome.is_none(), "{status}");
    }
    pipeline_repository(&api, "https://github.com/acme/widgets.git");
    for status in [401, 403, 429, 500] {
        github.serve(
            &format!("repos/acme/widgets/compare/{SHA}...{NEWER}?per_page=1"),
            status,
            json!({"message": "synthetic"}),
        );
        fx.clock.advance(30_000);
        monitor.tick().unwrap();
        assert_eq!(gate(&fx), GateState::Pending, "{status}");
    }
    compare(&github, NEWER, "ahead");
    fx.clock.advance(30_000);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 42));
}

#[test]
fn hs2_56mtyn_lookups_are_budgeted_per_poll_and_cancel_reaches_only_the_commits_build() {
    // Five lookups per poll; a walk left unfinished is neither green nor superseded.
    let fx = fixture();
    let api = FakeBuildkite::default();
    let github = FakeBuildkite::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    pipeline_repository(&api, "https://github.com/acme/widgets.git");
    api.builds(vec![]);
    let heads: Vec<String> = (0..6).map(|n| format!("{:040x}", 0xa0 + n)).collect();
    let mut listed = Vec::new();
    for (index, head) in heads.iter().enumerate() {
        let number = 80 - index as u64;
        let covering = index == 5;
        compare(&github, head, if covering { "ahead" } else { "diverged" });
        listed.push(build(
            number,
            head,
            if covering { "passed" } else { "failed" },
            json!([]),
        ));
    }
    branch_builds(&api, listed);
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    assert_eq!(compares(&github), 5);
    assert_eq!(gate(&fx), GateState::Pending);
    step(&fx, &mut monitor);
    assert_eq!(compares(&github), 6);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 75));

    // Cancel: a later build covering the commit is never cancelled; the commit's own is.
    let fx = fixture();
    let api = FakeBuildkite::default();
    let github = FakeBuildkite::default();
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
    pipeline_repository(&api, "https://github.com/acme/widgets.git");
    api.builds(vec![build(70, SHA, "running", json!([]))]);
    branch_builds(
        &api,
        vec![
            build(71, NEWER, "running", json!([])),
            build(70, SHA, "running", json!([])),
        ],
    );
    compare(&github, NEWER, "ahead");
    api.serve(
        &format!("{PIPELINE}/builds/70/cancel"),
        200,
        build(70, SHA, "canceling", json!([])),
    );
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).latest["run_id"], "71");
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    step(&fx, &mut monitor);
    assert!(
        api.puts().is_empty(),
        "a later commit's build is not cancelled"
    );
}

/// HS2-YA5S82: the monitor reads the repository of each pipeline a hosted check wants,
/// with this machine's credentials, and records it: a known repository, a pipeline the
/// token may not read (403) or that is gone (404) as unreadable, and a transient failure
/// tried again later. While a want it can read is outstanding the monitor is not idle.
#[test]
fn hs2_ya5s82_the_monitor_reads_and_records_a_wanted_pipelines_repository() {
    use hotsheet_jobs::{PipelineRepositories, PipelineRepository};
    let fx = fixture();
    let api = FakeBuildkite::default();
    let cache = PipelineRepositories::at(fx.home.path());
    let mut monitor = watcher(&fx, &api).with_pipeline_repositories(cache.clone());
    // Nothing wanted: idle, and nothing is read.
    let idle = monitor.tick().unwrap();
    assert_eq!(idle.watching, 0);
    assert!(api.0.lock().unwrap().log.is_empty());
    api.serve(
        PIPELINE,
        200,
        json!({"repository": "git@github.com:acme/widgets.git"}),
    );
    api.serve("organizations/acme/pipelines/private", 403, json!({}));
    api.serve("organizations/acme/pipelines/flaky", 502, json!({}));
    let now = fx.store.now_ms();
    for slug in ["acme/widgets", "acme/private", "acme/missing", "acme/flaky"] {
        cache.want(BUILDKITE_KIND, slug, now).unwrap();
    }
    // A provider this monitor has no source for stays wanted and keeps nothing busy.
    cache.want("jenkins", "job/x", now).unwrap();
    let tick = monitor.tick().unwrap();
    assert_eq!(
        cache.lookup(BUILDKITE_KIND, "acme/widgets"),
        PipelineRepository::Known {
            repository: "git@github.com:acme/widgets.git".into()
        }
    );
    for unreadable in ["acme/private", "acme/missing"] {
        assert_eq!(
            cache.lookup(BUILDKITE_KIND, unreadable),
            PipelineRepository::Unreadable,
            "{unreadable}"
        );
    }
    assert_eq!(
        cache.lookup(BUILDKITE_KIND, "acme/flaky"),
        PipelineRepository::Unknown
    );
    // The transient failure keeps the monitor awake until its retry is due.
    assert_eq!(tick.idle_ms, 0);
    assert!(tick.next_wake_ms > now);
    let reads = |path: &str| {
        api.0
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|request| request.path == path)
            .count()
    };
    // Repeated steps before the retry is due read nothing again.
    monitor.tick().unwrap();
    assert_eq!(reads("organizations/acme/pipelines/flaky"), 1);
    assert_eq!(reads(PIPELINE), 1);
    // Due: read again; it answers now.
    api.serve(
        "organizations/acme/pipelines/flaky",
        200,
        json!({"repository": "https://github.com/acme/flaky"}),
    );
    fx.clock.set(tick.next_wake_ms);
    monitor.tick().unwrap();
    assert_eq!(reads("organizations/acme/pipelines/flaky"), 2);
    assert!(matches!(
        cache.lookup(BUILDKITE_KIND, "acme/flaky"),
        PipelineRepository::Known { .. }
    ));
    // Nothing it can read is wanted any more: idle again.
    let settled = monitor.tick().unwrap();
    fx.clock.set(settled.next_wake_ms);
    assert!(monitor.tick().unwrap().idle_ms > 0);
    assert_eq!(
        cache.wanted(),
        vec![("jenkins".to_owned(), "job/x".to_owned())]
    );
}
