//! HS2-BWF72S: the Bitbucket Pipelines watch source and the CI gate API for
//! `bitbucket-pipelines`, offline. A scripted REST port stands in for Bitbucket Cloud's
//! REST API 2.0 with the shapes and status codes its documentation gives
//! (<https://developer.atlassian.com/cloud/bitbucket/rest/api-group-pipelines/>: "List
//! pipelines" filtered by `target.ref_name` and sorted by `-created_on`, a paginated
//! `values` of pipelines with `uuid`, `build_number`, `run_number`, `state` (`name`,
//! `result.name`) and `target` (`type`, `ref_type`, `ref_name`, `commit.hash`,
//! `selector`); "List steps for a pipeline"; "Stop a pipeline" answered 204). No test
//! makes a network call. Synthetic data only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::{CHECKOUT, Fixture, fixture};
use hotsheet_jobs::{
    ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, BITBUCKET_PIPELINES_KIND,
    BitbucketPipelinesSource, CiProvider, CiRun, CiTarget, CiWatchOptions, GateState, GithubApi,
    Monitor, MonitorConfig, Status, StepStatus, ci_outcome, ensure_ci_watch, gate_state,
    rewatch_ci,
};
use serde_json::{Value, json};

const SHA: &str = "1111111111111111111111111111111111111111";
const NEWER: &str = "2222222222222222222222222222222222222222";
const REPO: &str = "repositories/acme/widgets";

#[derive(Default)]
struct Fake {
    routes: BTreeMap<String, (u16, String)>,
    log: Vec<ApiRequest>,
}

#[derive(Clone, Default)]
struct FakeBitbucket(Arc<Mutex<Fake>>);

fn uuid(number: u64) -> String {
    format!("{{00000000-0000-4000-8000-{number:012}}}")
}

fn encoded(number: u64) -> String {
    format!("%7B00000000-0000-4000-8000-{number:012}%7D")
}

impl FakeBitbucket {
    fn serve(&self, path: &str, status: u16, body: Value) {
        self.0
            .lock()
            .unwrap()
            .routes
            .insert(path.to_owned(), (status, body.to_string()));
    }

    fn pipelines(&self, pipelines: Vec<Value>) {
        self.serve(
            &format!("{REPO}/pipelines/?target.ref_name=main&sort=-created_on&pagelen=30"),
            200,
            json!({"page": 1, "pagelen": 30, "size": pipelines.len(), "values": pipelines}),
        );
    }

    fn steps(&self, number: u64, steps: Vec<Value>) {
        self.serve(
            &format!("{REPO}/pipelines/{}/steps/?pagelen=100", encoded(number)),
            200,
            json!({"page": 1, "pagelen": 100, "size": steps.len(), "values": steps}),
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

impl GithubApi for FakeBitbucket {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let mut fake = self.0.lock().unwrap();
        fake.log.push(request.clone());
        let Some((status, body)) = fake.routes.get(&request.path).cloned() else {
            return Ok(ApiResponse {
                status: 404,
                etag: None,
                retry_after_ms: None,
                body: json!({"type": "error", "error": {"message": "Repository not found"}})
                    .to_string(),
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

/// A pipeline as "List pipelines" answers it: `state` with a `result` only once
/// completed, and a branch target.
fn pipeline(number: u64, commit: &str, state: &str, result: Option<&str>) -> Value {
    let state_type = match state {
        "PENDING" => "pipeline_state_pending",
        "IN_PROGRESS" => "pipeline_state_in_progress",
        _ => "pipeline_state_completed",
    };
    let mut state_value = json!({"name": state, "type": state_type});
    if let Some(result) = result {
        state_value["result"] = json!({"name": result,
            "type": format!("pipeline_state_completed_{}", result.to_ascii_lowercase())});
    }
    json!({
        "type": "pipeline", "uuid": uuid(number), "build_number": number, "run_number": 1,
        "creator": {"type": "user", "uuid": "{synthetic}"},
        "repository": {"type": "repository", "full_name": "acme/widgets"},
        "target": {
            "type": "pipeline_ref_target", "ref_type": "branch", "ref_name": "main",
            "commit": {"type": "commit", "hash": commit},
            "selector": {"type": "branches", "pattern": "main"}
        },
        "trigger": {"type": "pipeline_trigger_push", "name": "PUSH"},
        "state": state_value,
        "created_on": "2026-11-01T10:00:00.000000+00:00", "completed_on": null,
        "build_seconds_used": 0
    })
}

fn pipeline_step(id: u64, name: &str, state: &str, result: Option<&str>) -> Value {
    let mut state_value = json!({"name": state, "type": "pipeline_step_state"});
    if let Some(result) = result {
        state_value["result"] = json!({"name": result});
    }
    json!({"type": "pipeline_step", "uuid": uuid(id), "name": name, "state": state_value})
}

fn target() -> CiTarget {
    CiTarget::bitbucket_pipelines("acme/widgets", SHA, "*", "main").unwrap()
}

fn watcher(fx: &Fixture, api: &FakeBitbucket) -> Monitor {
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
    monitor.register(Box::new(BitbucketPipelinesSource::new(api.clone())));
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
fn hs2_bwf72s_a_bitbucket_target_names_its_repository_and_attests_exact_coverage_only() {
    let target = target();
    assert_eq!(target.provider, CiProvider::BitbucketPipelines);
    assert!(!target.provider.attests_ancestors());
    let external = target.external(None);
    assert_eq!(external.system, BITBUCKET_PIPELINES_KIND);
    assert_eq!(CiTarget::parse(&external).unwrap().0, target);
    assert_eq!(target.job_key(), "bitbucket-pipelines:acme/widgets:main:*");
    for refused in [
        "widgets",
        "acme/widgets/x",
        "acme/wid gets",
        "/widgets",
        "acme/..",
    ] {
        assert!(
            CiTarget::bitbucket_pipelines(refused, SHA, "*", "main").is_err(),
            "{refused}"
        );
    }
    assert!(CiTarget::bitbucket_pipelines("acme/widgets", SHA, "default", "main").is_err());
    assert_eq!(
        CiTarget::for_provider(
            CiProvider::BitbucketPipelines,
            "acme/widgets",
            SHA,
            "*",
            "main"
        )
        .unwrap(),
        target
    );
}

#[test]
fn hs2_bwf72s_a_commits_pipeline_is_followed_to_green_with_its_steps() {
    let fx = fixture();
    let api = FakeBitbucket::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    assert_eq!(job.kind.name, BITBUCKET_PIPELINES_KIND);

    api.pipelines(vec![pipeline(40, NEWER, "COMPLETED", Some("SUCCESSFUL"))]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Waiting);
    api.pipelines(vec![pipeline(41, SHA, "PENDING", None)]);
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    api.pipelines(vec![pipeline(41, SHA, "IN_PROGRESS", None)]);
    api.steps(
        41,
        vec![
            pipeline_step(1, "Build", "COMPLETED", Some("SUCCESSFUL")),
            pipeline_step(2, "Test", "IN_PROGRESS", None),
            pipeline_step(3, "Deploy", "PENDING", None),
        ],
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
            ("Build", StepStatus::Succeeded),
            ("Test", StepStatus::Active),
            ("Deploy", StepStatus::Pending)
        ]
    );
    assert_eq!(running.latest["current_step"], "Test");
    assert_eq!(running.latest["run_id"], "41");

    let revision = fx.get(&job.id).revision;
    fx.clock.advance(100);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).revision, revision);

    api.pipelines(vec![pipeline(41, SHA, "COMPLETED", Some("SUCCESSFUL"))]);
    step(&fx, &mut monitor);
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Succeeded);
    let run = CiRun {
        id: 41,
        attempt: 1,
        head_sha: SHA.into(),
        url: Some("https://bitbucket.org/acme/widgets/pipelines/results/41".into()),
        artifact: None,
    };
    assert_eq!(gate(&fx), GateState::Green { run: run.clone() });
    let outcome = ci_outcome(&done).unwrap();
    assert_eq!(outcome.gate, GateState::Green { run });
    assert_eq!(
        outcome.summary.as_deref(),
        Some("Bitbucket pipeline #41 succeeded for 1111111")
    );
    assert_eq!(
        done.outcome.as_ref().unwrap().refs["run_conclusion"],
        "SUCCESSFUL"
    );
}

#[test]
fn hs2_bwf72s_only_a_branch_pipeline_of_the_commit_itself_counts() {
    let fx = fixture();
    let api = FakeBitbucket::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    // A later commit's success, a pull request pipeline, a custom (manual) pipeline and
    // a tag pipeline never cover the commit: the API filters by ref only.
    let mut pull = pipeline(52, SHA, "COMPLETED", Some("SUCCESSFUL"));
    pull["target"] = json!({"type": "pipeline_pullrequest_target", "source": "main",
        "destination": "release", "commit": {"hash": SHA},
        "selector": {"type": "pull-requests", "pattern": "**"}});
    let mut custom = pipeline(53, SHA, "COMPLETED", Some("SUCCESSFUL"));
    custom["target"]["selector"] = json!({"type": "custom", "pattern": "nightly"});
    let mut tag = pipeline(51, SHA, "COMPLETED", Some("SUCCESSFUL"));
    tag["target"]["ref_type"] = json!("tag");
    api.pipelines(vec![
        pipeline(54, NEWER, "COMPLETED", Some("SUCCESSFUL")),
        custom,
        pull,
        tag,
    ]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx), GateState::Pending);
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    // An older successful pipeline of the commit wins over a newer failing rerun.
    api.pipelines(vec![
        pipeline(56, SHA, "COMPLETED", Some("FAILED")),
        pipeline(55, SHA, "COMPLETED", Some("SUCCESSFUL")),
    ]);
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 55 && run.head_sha == SHA));
}

#[test]
fn hs2_bwf72s_red_is_not_judged_again_until_rerun_and_stop_reaches_only_the_commits_pipeline() {
    let fx = fixture();
    let api = FakeBitbucket::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.pipelines(vec![pipeline(61, SHA, "COMPLETED", Some("ERROR"))]);
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
    // Rerunning the failed steps keeps the build number and raises the run number.
    let mut rerun = pipeline(61, SHA, "COMPLETED", Some("SUCCESSFUL"));
    rerun["run_number"] = json!(2);
    api.pipelines(vec![rerun]);
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 61 && run.attempt == 2));

    // Stop: a POST to the commit's own unfinished pipeline, once asked.
    let fx = fixture();
    let api = FakeBitbucket::default();
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
    api.pipelines(vec![pipeline(81, SHA, "IN_PROGRESS", None)]);
    let stop = format!("{REPO}/pipelines/{}/stopPipeline", encoded(81));
    api.serve(&stop, 204, json!(null));
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(api.posts().is_empty());
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    step(&fx, &mut monitor);
    assert_eq!(api.posts(), [stop]);
    api.pipelines(vec![pipeline(81, SHA, "COMPLETED", Some("STOPPED"))]);
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx), GateState::Cancelled);
}

#[test]
fn hs2_bwf72s_credentials_and_rate_limits_keep_the_last_observation_and_404_ends_it() {
    let fx = fixture();
    let api = FakeBitbucket::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.pipelines(vec![pipeline(41, SHA, "IN_PROGRESS", None)]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    for status in [401, 403, 429, 500] {
        api.serve(
            &format!("{REPO}/pipelines/?target.ref_name=main&sort=-created_on&pagelen=30"),
            status,
            json!({"type": "error", "error": {"message": "synthetic"}}),
        );
        step(&fx, &mut monitor);
        let seen = fx.get(&job.id);
        assert_eq!(seen.status, Status::Running, "{status}");
        assert_eq!(seen.latest["run_id"], "41");
        assert!(seen.outcome.is_none());
        assert_eq!(gate(&fx), GateState::Pending);
    }
    let fx = fixture();
    let api = FakeBitbucket::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx), GateState::Unknown);
}
