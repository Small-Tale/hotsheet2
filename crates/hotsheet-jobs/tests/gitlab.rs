//! HS2-TPAHYA: the GitLab CI watch source and the CI gate API for `gitlab-ci`, offline.
//! A scripted REST port stands in for GitLab's `/api/v4` with its JSON shapes, ETags and
//! status codes. No test makes a network call. Synthetic data only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::{CHECKOUT, Fixture, fixture};
use hotsheet_jobs::{
    ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, CiProvider, CiRun, CiTarget,
    CiWatchOptions, GITLAB_CI_KIND, GateState, GithubApi, GitlabCiSource, Monitor, MonitorConfig,
    Status, StepStatus, ci_outcome, ensure_ci_watch, gate_state, rewatch_ci,
};
use serde_json::{Value, json};

const SHA: &str = "1111111111111111111111111111111111111111";
const NEWER: &str = "2222222222222222222222222222222222222222";
const OTHER: &str = "3333333333333333333333333333333333333333";
const PROJECT: &str = "acme%2Fplatform%2Fwidgets";

#[derive(Default)]
struct Fake {
    routes: BTreeMap<String, (u16, Option<String>, String)>,
    versions: u64,
    log: Vec<ApiRequest>,
}

#[derive(Clone, Default)]
struct FakeGitlab(Arc<Mutex<Fake>>);

impl FakeGitlab {
    fn serve(&self, path: &str, status: u16, body: Value) {
        let mut fake = self.0.lock().unwrap();
        fake.versions += 1;
        let etag = (status == 200).then(|| format!("W/\"v{}\"", fake.versions));
        fake.routes
            .insert(path.to_owned(), (status, etag, body.to_string()));
    }

    fn pipelines(&self, pipelines: Vec<Value>) {
        self.serve(
            &format!(
                "projects/{PROJECT}/pipelines?ref=main&source=push&order_by=id&sort=desc&per_page=30"
            ),
            200,
            Value::Array(pipelines),
        );
    }

    fn jobs(&self, pipeline: u64, jobs: Value) {
        self.serve(
            &format!("projects/{PROJECT}/pipelines/{pipeline}/jobs?per_page=100"),
            200,
            jobs,
        );
    }

    fn merge_base(&self, head: &str, base: &str) {
        self.serve(
            &format!("projects/{PROJECT}/repository/merge_base?refs%5B%5D={SHA}&refs%5B%5D={head}"),
            200,
            json!({"id": base}),
        );
    }

    fn count(&self, needle: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|request| request.path.contains(needle))
            .count()
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

impl GithubApi for FakeGitlab {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let mut fake = self.0.lock().unwrap();
        fake.log.push(request.clone());
        let Some((status, etag, body)) = fake.routes.get(&request.path).cloned() else {
            return Ok(ApiResponse {
                status: 404,
                etag: None,
                retry_after_ms: None,
                body: json!({"message": "404 Not Found"}).to_string(),
            });
        };
        let unchanged =
            request.method == ApiMethod::Get && etag.is_some() && request.if_none_match == etag;
        Ok(ApiResponse {
            status: if unchanged { 304 } else { status },
            etag,
            retry_after_ms: None,
            body: if unchanged { String::new() } else { body },
        })
    }
}

fn pipeline(id: u64, sha: &str, status: &str) -> Value {
    json!({
        "id": id, "iid": id, "project_id": 7, "sha": sha, "ref": "main", "status": status,
        "source": "push", "name": null,
        "created_at": "2026-11-01T10:00:00.000Z", "updated_at": "2026-11-01T10:05:00.000Z",
        "web_url": format!("https://gitlab.example.test/acme/platform/widgets/-/pipelines/{id}")
    })
}

fn target() -> CiTarget {
    CiTarget::gitlab("acme/platform/widgets", SHA, "*", "main").unwrap()
}

fn watcher(fx: &Fixture, api: &FakeGitlab) -> Monitor {
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
    monitor.register(Box::new(GitlabCiSource::new(api.clone())));
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
fn hs2_tpahya_a_gitlab_target_names_a_nested_project_and_its_own_provider() {
    let target = target();
    assert_eq!(target.provider, CiProvider::GitlabCi);
    let external = target.external(None);
    assert_eq!(external.system, GITLAB_CI_KIND);
    assert_eq!(CiTarget::parse(&external).unwrap().0, target);
    assert_eq!(target.job_key(), "gitlab-ci:acme/platform/widgets:main:*");
    for refused in [
        "widgets",
        "acme//widgets",
        "acme/../widgets",
        "acme/wid gets",
    ] {
        assert!(
            CiTarget::gitlab(refused, SHA, "*", "main").is_err(),
            "{refused}"
        );
    }
    // A GitHub repository is still exactly owner/name.
    assert!(CiTarget::new("acme/platform/widgets", SHA, "CI", "main").is_err());
}

#[test]
fn hs2_tpahya_a_commits_pipeline_is_followed_to_green_without_client_polling() {
    let fx = fixture();
    let api = FakeGitlab::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    assert_eq!(job.kind.name, GITLAB_CI_KIND);
    assert_eq!(gate(&fx), GateState::Pending);

    api.pipelines(vec![pipeline(41, SHA, "pending")]);
    api.jobs(
        41,
        json!([{"id": 1, "name": "build", "stage": "build", "status": "created"}]),
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    api.pipelines(vec![pipeline(41, SHA, "running")]);
    api.jobs(
        41,
        json!([
            {"id": 1, "name": "build", "stage": "build", "status": "success"},
            {"id": 2, "name": "test", "stage": "test", "status": "running"}
        ]),
    );
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
            ("build: build", StepStatus::Succeeded),
            ("test: test", StepStatus::Active)
        ]
    );
    assert_eq!(running.latest["current_step"], "test");

    // Nothing new: conditional requests answer 304 and nothing is rewritten.
    let revision = fx.get(&job.id).revision;
    fx.clock.advance(100);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).revision, revision);

    api.pipelines(vec![pipeline(41, SHA, "success")]);
    step(&fx, &mut monitor);
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Succeeded);
    let run = CiRun {
        id: 41,
        attempt: 1,
        head_sha: SHA.into(),
        url: Some("https://gitlab.example.test/acme/platform/widgets/-/pipelines/41".into()),
        artifact: None,
    };
    assert_eq!(gate(&fx), GateState::Green { run: run.clone() });
    let outcome = ci_outcome(&done).unwrap();
    assert_eq!(outcome.gate, GateState::Green { run });
    assert_eq!(
        outcome.summary.as_deref(),
        Some("GitLab pipeline 41 passed for 1111111")
    );
    assert_eq!(
        api.count("/merge_base"),
        0,
        "its own pipeline needs no lookup"
    );
}

#[test]
fn hs2_tpahya_ancestry_red_superseded_and_cancel_follow_the_github_rules() {
    // A newer pipeline whose head contains the commit covers it.
    let fx = fixture();
    let api = FakeGitlab::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.pipelines(vec![
        pipeline(52, NEWER, "success"),
        pipeline(51, SHA, "canceled"),
    ]);
    api.jobs(52, json!([]));
    api.merge_base(NEWER, SHA);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx), GateState::Green { run } if run.head_sha == NEWER));
    assert_eq!(fx.get(&job.id).status, Status::Succeeded);

    // The newest covering pipeline failed: red, and a rewatch ignores that pipeline.
    let fx = fixture();
    let api = FakeGitlab::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.pipelines(vec![pipeline(61, SHA, "failed")]);
    api.jobs(61, json!([]));
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx), GateState::Red { run } if run.id == 61));
    rewatch_ci(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    step(&fx, &mut monitor);
    assert_eq!(
        gate(&fx),
        GateState::Pending,
        "the judged pipeline is not judged again"
    );

    // The branch moved to a line without the commit: superseded.
    let fx = fixture();
    let api = FakeGitlab::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.pipelines(vec![pipeline(71, OTHER, "running")]);
    api.jobs(71, json!([]));
    api.merge_base(OTHER, "4444444444444444444444444444444444444444");
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx), GateState::Superseded);

    // Cancel reaches only the commit's own unfinished pipeline.
    let fx = fixture();
    let api = FakeGitlab::default();
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
    api.pipelines(vec![pipeline(81, SHA, "running")]);
    api.jobs(81, json!([]));
    api.serve(
        &format!("projects/{PROJECT}/pipelines/81/cancel"),
        200,
        json!({"id": 81, "status": "canceling"}),
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    step(&fx, &mut monitor);
    assert_eq!(
        api.posts(),
        [format!("projects/{PROJECT}/pipelines/81/cancel")]
    );
    api.pipelines(vec![pipeline(81, SHA, "canceled")]);
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx), GateState::Cancelled);
}
