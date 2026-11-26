//! HS2-17HVNP: the CircleCI watch source and the CI gate API for `circleci`, offline.
//! A scripted REST port stands in for CircleCI's API v2 with the JSON shapes and status
//! codes its documentation gives (<https://circleci.com/docs/api/v2/index.html>:
//! `GET /project/{project-slug}/pipeline?branch=` with `items[]` of `id`, `number`,
//! `state` and `vcs.revision`/`vcs.branch`; `GET /pipeline/{id}/workflow` with `name`,
//! `status` and `created_at`; `GET /workflow/{id}/job` with `job_number`, `name` and
//! `status`; `POST /workflow/{id}/cancel` answering 202). HS2-56MTYN: `GET
//! /project/{project-slug}` answers `vcs_info.vcs_url`, and a second scripted port
//! stands in for GitHub's "Compare two commits" (`status` `ahead`, `behind`,
//! `identical` or `diverged`). No test makes a network call. Synthetic data only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::{CHECKOUT, Fixture, fixture};
use hotsheet_jobs::{
    ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, CIRCLECI_KIND, CiProvider, CiTarget,
    CiWatchOptions, CircleciSource, GateState, GithubApi, Monitor, MonitorConfig, RepositoryHosts,
    Status, StepStatus, ci_outcome, ensure_ci_watch, gate_state, rewatch_ci,
};
use serde_json::{Value, json};

const SHA: &str = "1111111111111111111111111111111111111111";
const NEWER: &str = "2222222222222222222222222222222222222222";
const PROJECT: &str = "gh/acme/widgets";

#[derive(Default)]
struct Fake {
    routes: BTreeMap<String, (u16, String)>,
    log: Vec<ApiRequest>,
}

#[derive(Clone, Default)]
struct FakeCircleci(Arc<Mutex<Fake>>);

impl FakeCircleci {
    fn serve(&self, path: &str, status: u16, body: Value) {
        self.0
            .lock()
            .unwrap()
            .routes
            .insert(path.to_owned(), (status, body.to_string()));
    }

    fn pipelines(&self, items: Vec<Value>) {
        self.serve(
            &format!("project/{PROJECT}/pipeline?branch=main"),
            200,
            json!({"next_page_token": null, "items": items}),
        );
    }

    fn workflows(&self, pipeline: &str, items: Vec<Value>) {
        self.serve(
            &format!("pipeline/{pipeline}/workflow"),
            200,
            json!({"next_page_token": null, "items": items}),
        );
    }

    fn jobs(&self, workflow: &str, items: Vec<Value>) {
        self.serve(
            &format!("workflow/{workflow}/job"),
            200,
            json!({"next_page_token": null, "items": items}),
        );
    }

    fn posts(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|request| request.method == ApiMethod::Post)
            .map(|request| request.path.clone())
            .collect()
    }
}

impl GithubApi for FakeCircleci {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let mut fake = self.0.lock().unwrap();
        fake.log.push(request.clone());
        let Some((status, body)) = fake.routes.get(&request.path).cloned() else {
            return Ok(ApiResponse {
                status: 404,
                etag: None,
                retry_after_ms: None,
                body: json!({"message": "Not found."}).to_string(),
            });
        };
        Ok(ApiResponse {
            status,
            etag: None,
            retry_after_ms: None,
            body,
        })
    }
}

fn pipeline_id(number: u64) -> String {
    format!("5034460f-c7c4-4c43-9457-{number:012}")
}

fn workflow_id(number: u64, name: &str) -> String {
    format!("{:08x}-0000-4000-8000-{number:012}", name.len())
}

fn pipeline(number: u64, revision: &str, state: &str) -> Value {
    json!({
        "id": pipeline_id(number), "errors": [], "project_slug": PROJECT,
        "updated_at": "2026-11-01T10:05:00.000Z", "number": number, "state": state,
        "created_at": "2026-11-01T10:00:00.000Z",
        "trigger": {"received_at": "2026-11-01T10:00:00.000Z", "type": "webhook",
            "actor": {"login": "synthetic", "avatar_url": null}},
        "vcs": {"origin_repository_url": "https://github.com/acme/widgets",
            "target_repository_url": "https://github.com/acme/widgets", "revision": revision,
            "provider_name": "GitHub", "commit": {"body": "", "subject": "Synthetic change"},
            "branch": "main"}
    })
}

fn workflow(number: u64, name: &str, status: &str, created: &str) -> Value {
    json!({
        "pipeline_id": pipeline_id(number), "id": workflow_id(number, name), "name": name,
        "project_slug": PROJECT, "status": status, "started_by": "synthetic",
        "pipeline_number": number, "created_at": created, "stopped_at": null
    })
}

fn workflow_job(number: u64, name: &str, status: &str) -> Value {
    json!({
        "dependencies": [], "job_number": number, "id": format!("job-{number}"),
        "started_at": null, "name": name, "project_slug": PROJECT, "status": status,
        "type": "build", "stopped_at": null
    })
}

fn target(workflow: &str) -> CiTarget {
    CiTarget::circleci(PROJECT, SHA, workflow, "main").unwrap()
}

fn watcher(fx: &Fixture, api: &FakeCircleci) -> Monitor {
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
    monitor.register(Box::new(CircleciSource::new(api.clone())));
    monitor
}

fn step(fx: &Fixture, monitor: &mut Monitor) {
    fx.clock.advance(400);
    monitor.tick().unwrap();
}

fn gate(fx: &Fixture, workflow: &str) -> GateState {
    gate_state(&fx.store, CHECKOUT, &target(workflow))
        .unwrap()
        .state
}

const T1: &str = "2026-11-01T10:00:00.000Z";
const T2: &str = "2026-11-01T11:00:00.000Z";

#[test]
fn hs2_17hvnp_a_circleci_target_names_its_project_slug_and_attests_exact_coverage_only() {
    let target = target("*");
    assert_eq!(target.provider, CiProvider::Circleci);
    assert!(!target.provider.attests_ancestors());
    let external = target.external(None);
    assert_eq!(external.system, CIRCLECI_KIND);
    assert_eq!(CiTarget::parse(&external).unwrap().0, target);
    assert_eq!(target.job_key(), "circleci:gh/acme/widgets:main:*");
    for refused in [
        "acme/widgets",
        "xx/acme/widgets",
        "gh/acme",
        "gh/acme/wid gets",
    ] {
        assert!(
            CiTarget::circleci(refused, SHA, "*", "main").is_err(),
            "{refused}"
        );
    }
    assert!(CiTarget::circleci("bb/acme/widgets", SHA, "build-and-test", "main").is_ok());
}

#[test]
fn hs2_17hvnp_every_workflow_of_the_commits_pipeline_is_followed_to_green() {
    let fx = fixture();
    let api = FakeCircleci::default();
    let job_id = ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions::default(),
    )
    .unwrap()
    .id;
    assert_eq!(gate(&fx, "*"), GateState::Pending);
    api.pipelines(vec![pipeline(41, SHA, "created")]);
    api.workflows(&pipeline_id(41), vec![]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job_id).status, Status::Waiting);

    // Two workflows; their jobs are the steps.
    let build = workflow_id(41, "build-and-test");
    let lint = workflow_id(41, "lint");
    api.workflows(
        &pipeline_id(41),
        vec![
            workflow(41, "build-and-test", "running", T1),
            workflow(41, "lint", "success", T1),
        ],
    );
    api.jobs(
        &build,
        vec![
            workflow_job(101, "build", "success"),
            workflow_job(102, "test", "running"),
        ],
    );
    api.jobs(&lint, vec![workflow_job(103, "lint", "success")]);
    step(&fx, &mut monitor);
    let running = fx.get(&job_id);
    assert_eq!(running.status, Status::Running);
    let steps: Vec<(&str, StepStatus)> = running
        .steps
        .iter()
        .map(|step| (step.label.as_str(), step.status))
        .collect();
    assert_eq!(
        steps,
        [
            ("build-and-test: build", StepStatus::Succeeded),
            ("build-and-test: test", StepStatus::Active),
            ("lint: lint", StepStatus::Succeeded),
        ]
    );
    assert_eq!(running.latest["current_step"], "build-and-test: test");

    // The same answers rewrite nothing.
    let revision = fx.get(&job_id).revision;
    fx.clock.advance(100);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job_id).revision, revision);

    api.workflows(
        &pipeline_id(41),
        vec![
            workflow(41, "build-and-test", "success", T1),
            workflow(41, "lint", "success", T1),
        ],
    );
    api.jobs(
        &build,
        vec![
            workflow_job(101, "build", "success"),
            workflow_job(102, "test", "success"),
        ],
    );
    step(&fx, &mut monitor);
    let done = fx.get(&job_id);
    assert_eq!(done.status, Status::Succeeded);
    let GateState::Green { run } = gate(&fx, "*") else {
        panic!("{:?}", gate(&fx, "*"));
    };
    assert_eq!((run.id, run.head_sha.as_str()), (41, SHA));
    assert_eq!(
        run.url.as_deref(),
        Some(
            format!("https://app.circleci.com/pipelines/github/acme/widgets/41/workflows/{build}")
                .as_str()
        )
    );
    let outcome = ci_outcome(&done).unwrap();
    assert_eq!(
        outcome.summary.as_deref(),
        Some("CircleCI pipeline 41 passed for 1111111")
    );
    assert_eq!(
        done.outcome.as_ref().unwrap().refs["run_conclusion"],
        "success,success"
    );
}

#[test]
fn hs2_17hvnp_a_named_workflow_reruns_and_exact_coverage_decide_the_gate() {
    // Only the named workflow counts, and its newest rerun is the one judged.
    let fx = fixture();
    let api = FakeCircleci::default();
    ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("lint"),
        &CiWatchOptions::default(),
    )
    .unwrap();
    api.pipelines(vec![pipeline(51, SHA, "created")]);
    let rerun = {
        let mut rerun = workflow(51, "lint", "success", T2);
        rerun["id"] = json!("aaaaaaaa-0000-4000-8000-000000000051");
        rerun
    };
    api.workflows(
        &pipeline_id(51),
        vec![
            workflow(51, "build-and-test", "failed", T1),
            workflow(51, "lint", "failed", T1),
            rerun,
        ],
    );
    api.jobs(
        "aaaaaaaa-0000-4000-8000-000000000051",
        vec![workflow_job(201, "lint", "success")],
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx, "lint"), GateState::Green { run } if run.id == 51));

    // With every workflow counted the same pipeline is red.
    let fx = fixture();
    let api = FakeCircleci::default();
    ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions::default(),
    )
    .unwrap();
    api.pipelines(vec![pipeline(61, SHA, "created")]);
    api.workflows(
        &pipeline_id(61),
        vec![
            workflow(61, "build-and-test", "failed", T1),
            workflow(61, "lint", "success", T1),
        ],
    );
    api.jobs(&workflow_id(61, "build-and-test"), vec![]);
    api.jobs(&workflow_id(61, "lint"), vec![]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx, "*"), GateState::Red { run } if run.id == 61));
    // A rewatch does not judge the same pipeline again.
    rewatch_ci(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions::default(),
    )
    .unwrap();
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx, "*"), GateState::Pending);

    // A pipeline whose configuration did not compile is red.
    let fx = fixture();
    let api = FakeCircleci::default();
    ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions::default(),
    )
    .unwrap();
    api.pipelines(vec![pipeline(71, SHA, "errored")]);
    api.workflows(&pipeline_id(71), vec![]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx, "*"), GateState::Red { run } if run.id == 71));

    // A green pipeline of a later commit never covers this one.
    let fx = fixture();
    let api = FakeCircleci::default();
    ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions::default(),
    )
    .unwrap();
    api.pipelines(vec![pipeline(81, NEWER, "created")]);
    api.workflows(&pipeline_id(81), vec![workflow(81, "lint", "success", T1)]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx, "*"), GateState::Pending);
}

#[test]
fn hs2_17hvnp_cancel_reaches_only_the_commits_unfinished_workflows() {
    let fx = fixture();
    let api = FakeCircleci::default();
    let job = ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions {
            ticket: None,
            can_cancel: true,
        },
    )
    .unwrap();
    let running = workflow_id(91, "build-and-test");
    api.pipelines(vec![pipeline(91, SHA, "created")]);
    api.workflows(
        &pipeline_id(91),
        vec![
            workflow(91, "build-and-test", "running", T1),
            workflow(91, "lint", "success", T1),
        ],
    );
    api.jobs(&running, vec![workflow_job(301, "test", "running")]);
    api.jobs(&workflow_id(91, "lint"), vec![]);
    api.serve(
        &format!("workflow/{running}/cancel"),
        202,
        json!({"message": "Accepted."}),
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    step(&fx, &mut monitor);
    assert_eq!(api.posts(), [format!("workflow/{running}/cancel")]);
    api.workflows(
        &pipeline_id(91),
        vec![
            workflow(91, "build-and-test", "canceled", T1),
            workflow(91, "lint", "success", T1),
        ],
    );
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx, "*"), GateState::Cancelled);
}

// ---- HS2-56MTYN: ancestry through the repository host ----

const OTHER: &str = "4444444444444444444444444444444444444444";

/// The project's own answer, naming the repository it builds.
fn project_repository(api: &FakeCircleci, url: &str) {
    api.serve(
        &format!("project/{PROJECT}"),
        200,
        json!({
            "slug": PROJECT, "name": "widgets", "id": "synthetic",
            "organization_name": "acme", "organization_slug": "gh/acme",
            "organization_id": "synthetic",
            "vcs_info": {"vcs_url": url, "provider": "GitHub", "default_branch": "main"}
        }),
    );
}

fn compare(github: &FakeCircleci, head: &str, status: &str) {
    github.serve(
        &format!("repos/acme/widgets/compare/{SHA}...{head}?per_page=1"),
        200,
        json!({"status": status}),
    );
}

fn hosted_watcher(fx: &Fixture, api: &FakeCircleci, github: &FakeCircleci) -> Monitor {
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
        CircleciSource::new(api.clone())
            .with_hosts(RepositoryHosts::new().with_github(github.clone())),
    ));
    monitor
}

#[test]
fn hs2_56mtyn_a_later_pipeline_covers_the_commit_and_another_line_supersedes_it() {
    let fx = fixture();
    let api = FakeCircleci::default();
    let github = FakeCircleci::default();
    let job = ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions::default(),
    )
    .unwrap();
    project_repository(&api, "https://github.com/acme/widgets");
    api.pipelines(vec![
        pipeline(92, NEWER, "created"),
        pipeline(91, SHA, "created"),
    ]);
    api.workflows(&pipeline_id(92), vec![workflow(92, "build", "success", T1)]);
    api.workflows(&pipeline_id(91), vec![workflow(91, "build", "running", T1)]);
    api.jobs(
        &workflow_id(92, "build"),
        vec![workflow_job(401, "test", "success")],
    );
    compare(&github, NEWER, "ahead");
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    let green = gate(&fx, "*");
    assert!(matches!(&green, GateState::Green { run } if run.id == 92 && run.head_sha == NEWER));
    let done = fx.get(&job.id);
    assert_eq!(done.outcome.unwrap().refs["run_head_sha"], NEWER);

    // An older revision, and a project on a host without a port, never cover.
    for (url, status) in [
        ("https://github.com/acme/widgets", "behind"),
        ("https://bitbucket.org/acme/widgets", "ahead"),
    ] {
        let fx = fixture();
        let api = FakeCircleci::default();
        let github = FakeCircleci::default();
        ensure_ci_watch(
            &fx.store,
            CHECKOUT,
            &target("*"),
            &CiWatchOptions::default(),
        )
        .unwrap();
        project_repository(&api, url);
        api.pipelines(vec![pipeline(92, NEWER, "created")]);
        api.workflows(&pipeline_id(92), vec![workflow(92, "build", "success", T1)]);
        compare(&github, NEWER, status);
        let mut monitor = hosted_watcher(&fx, &api, &github);
        monitor.tick().unwrap();
        step(&fx, &mut monitor);
        assert_eq!(gate(&fx, "*"), GateState::Pending, "{url}");
    }

    // The branch moved to another line: superseded, and nothing is cancelled.
    let fx = fixture();
    let api = FakeCircleci::default();
    let github = FakeCircleci::default();
    let job = ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions::default(),
    )
    .unwrap();
    project_repository(&api, "git@github.com:acme/widgets.git");
    api.pipelines(vec![pipeline(95, OTHER, "created")]);
    compare(&github, OTHER, "diverged");
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx, "*"), GateState::Superseded);
    assert_eq!(fx.get(&job.id).status, Status::Failed);
}

#[test]
fn hs2_56mtyn_a_project_read_refused_keeps_exact_coverage_and_cancel_skips_later_pipelines() {
    // 403 on the project: exact coverage, read once.
    let fx = fixture();
    let api = FakeCircleci::default();
    let github = FakeCircleci::default();
    ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions::default(),
    )
    .unwrap();
    api.serve(
        &format!("project/{PROJECT}"),
        403,
        json!({"message": "Permission denied"}),
    );
    api.pipelines(vec![pipeline(92, NEWER, "created")]);
    api.workflows(&pipeline_id(92), vec![workflow(92, "build", "success", T1)]);
    compare(&github, NEWER, "ahead");
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx, "*"), GateState::Pending);
    let log = api.0.lock().unwrap().log.clone();
    assert_eq!(
        log.iter()
            .filter(|request| request.path == format!("project/{PROJECT}"))
            .count(),
        1
    );
    assert!(github.0.lock().unwrap().log.is_empty());

    // The newest covering pipeline is a later commit's: its workflows are not
    // cancelled.
    let fx = fixture();
    let api = FakeCircleci::default();
    let github = FakeCircleci::default();
    let job = ensure_ci_watch(
        &fx.store,
        CHECKOUT,
        &target("*"),
        &CiWatchOptions {
            ticket: None,
            can_cancel: true,
        },
    )
    .unwrap();
    project_repository(&api, "https://github.com/acme/widgets");
    api.pipelines(vec![
        pipeline(92, NEWER, "created"),
        pipeline(91, SHA, "created"),
    ]);
    api.workflows(&pipeline_id(92), vec![workflow(92, "build", "running", T1)]);
    api.workflows(&pipeline_id(91), vec![workflow(91, "build", "running", T1)]);
    api.jobs(&workflow_id(92, "build"), vec![]);
    compare(&github, NEWER, "ahead");
    let mut monitor = hosted_watcher(&fx, &api, &github);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).latest["run_head_sha"], NEWER);
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    step(&fx, &mut monitor);
    assert!(api.posts().is_empty());
}
