//! HS2-JZ1DD2: the Azure Pipelines watch source and the CI gate API for
//! `azure-pipelines`, offline. A scripted REST port stands in for the Azure DevOps Build
//! REST API 7.1 with the shapes and status codes its documentation gives ("Builds -
//! List", <https://learn.microsoft.com/en-us/rest/api/azure/devops/build/builds/list?view=azure-devops-rest-7.1>:
//! `value` with `id`, `buildNumber`, `status`, `result`, `sourceBranch`,
//! `sourceVersion`, `reason`, `_links.web.href`; "Timeline - Get",
//! <https://learn.microsoft.com/en-us/rest/api/azure/devops/build/timeline/get?view=azure-devops-rest-7.1>:
//! `records` with `type`, `name`, `order`, `state`, `result`, `attempt`; "Builds -
//! Update Build" with `{"status": "cancelling"}`). No test makes a network call.
//! Synthetic data only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::{CHECKOUT, Fixture, fixture};
use hotsheet_jobs::{
    AZURE_PIPELINES_KIND, ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, AzurePipelinesSource,
    CiProvider, CiRun, CiTarget, CiWatchOptions, GateState, GithubApi, Monitor, MonitorConfig,
    Status, StepStatus, ci_outcome, ensure_ci_watch, gate_state, rewatch_ci,
};
use serde_json::{Value, json};

const SHA: &str = "1111111111111111111111111111111111111111";
const NEWER: &str = "2222222222222222222222222222222222222222";
const PROJECT: &str = "acme/Widget%20Team";
const LIST: &str = "acme/Widget%20Team/_apis/build/builds?definitions=12&branchName=refs%2Fheads%2Fmain&queryOrder=queueTimeDescending&%24top=30&api-version=7.1";

#[derive(Default)]
struct Fake {
    routes: BTreeMap<String, (u16, String)>,
    log: Vec<ApiRequest>,
}

#[derive(Clone, Default)]
struct FakeAzure(Arc<Mutex<Fake>>);

impl FakeAzure {
    fn serve(&self, path: &str, status: u16, body: Value) {
        self.0
            .lock()
            .unwrap()
            .routes
            .insert(path.to_owned(), (status, body.to_string()));
    }

    fn builds(&self, builds: Vec<Value>) {
        self.serve(LIST, 200, json!({"count": builds.len(), "value": builds}));
    }

    fn timeline(&self, id: u64, records: Vec<Value>) {
        self.serve(
            &format!("{PROJECT}/_apis/build/builds/{id}/timeline?api-version=7.1"),
            200,
            json!({"id": "00000000-0000-4000-8000-000000000001", "changeId": 1,
                "records": records}),
        );
    }

    fn sent(&self, method: ApiMethod) -> Vec<ApiRequest> {
        self.0
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|request| request.method == method)
            .cloned()
            .collect()
    }
}

impl GithubApi for FakeAzure {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let mut fake = self.0.lock().unwrap();
        fake.log.push(request.clone());
        let Some((status, body)) = fake.routes.get(&request.path).cloned() else {
            return Ok(ApiResponse {
                status: 404,
                etag: None,
                retry_after_ms: None,
                body:
                    json!({"$id": "1", "message": "TF200016: The following project does not exist",
                    "typeKey": "ProjectDoesNotExistWithNameException"})
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

fn build(id: u64, commit: &str, status: &str, result: Option<&str>) -> Value {
    json!({
        "_links": {
            "self": {"href": format!("https://dev.azure.com/acme/0000/_apis/build/Builds/{id}")},
            "web": {"href": format!("https://dev.azure.com/acme/0000/_build/results?buildId={id}")}
        },
        "id": id, "buildNumber": format!("20261101.{id}"), "status": status, "result": result,
        "queueTime": "2026-11-01T10:00:00.000Z", "startTime": null, "finishTime": null,
        "definition": {"id": 12, "name": "widgets-ci", "path": "\\", "type": "build"},
        "project": {"id": "00000000-0000-4000-8000-000000000000", "name": "Widget Team"},
        "sourceBranch": "refs/heads/main", "sourceVersion": commit, "reason": "individualCI",
        "priority": "normal", "repository": {"id": "synthetic", "type": "TfsGit"}
    })
}

fn record(
    id: &str,
    kind: &str,
    name: &str,
    order: i64,
    state: &str,
    result: Option<&str>,
    attempt: u32,
) -> Value {
    json!({"id": id, "parentId": null, "type": kind, "name": name, "order": order,
        "state": state, "result": result, "attempt": attempt, "identifier": name,
        "previousAttempts": [], "errorCount": 0, "warningCount": 0})
}

fn target() -> CiTarget {
    CiTarget::azure_pipelines("acme/Widget Team/12", SHA, "*", "main").unwrap()
}

fn watcher(fx: &Fixture, api: &FakeAzure) -> Monitor {
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
    monitor.register(Box::new(AzurePipelinesSource::new(api.clone())));
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
fn hs2_jz1dd2_an_azure_target_names_its_definition_and_attests_exact_coverage_only() {
    let target = target();
    assert_eq!(target.provider, CiProvider::AzurePipelines);
    assert!(!target.provider.attests_ancestors());
    let external = target.external(None);
    assert_eq!(external.system, AZURE_PIPELINES_KIND);
    assert_eq!(CiTarget::parse(&external).unwrap().0, target);
    assert_eq!(
        target.job_key(),
        "azure-pipelines:acme/Widget Team/12:main:*"
    );
    for refused in [
        "acme/widgets",
        "acme/widgets/ci",
        "acme/widgets/0",
        "acme/widgets/12/x",
        "acme/ widgets/12",
        "acme/widgets./12",
        "acme/wid|gets/12",
        "a b/widgets/12",
        "acme/widgets/99999999999",
    ] {
        assert!(
            CiTarget::azure_pipelines(refused, SHA, "*", "main").is_err(),
            "{refused}"
        );
    }
    assert!(CiTarget::azure_pipelines("acme/widgets/12", SHA, "CI", "main").is_err());
    assert_eq!(
        CiTarget::for_provider(
            CiProvider::AzurePipelines,
            "acme/Widget Team/12",
            SHA,
            "*",
            "main"
        )
        .unwrap(),
        target
    );
}

#[test]
fn hs2_jz1dd2_a_commits_build_is_followed_to_green_with_timeline_steps() {
    let fx = fixture();
    let api = FakeAzure::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    assert_eq!(job.kind.name, AZURE_PIPELINES_KIND);

    // Another commit's build is not it; then the commit's build is queued.
    api.builds(vec![build(40, NEWER, "completed", Some("succeeded"))]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Waiting);
    api.builds(vec![build(41, SHA, "notStarted", None)]);
    step(&fx, &mut monitor);
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    // In progress: the timeline's stages are the steps.
    api.builds(vec![build(41, SHA, "inProgress", None)]);
    api.timeline(
        41,
        vec![
            record("s1", "Stage", "Build", 1, "completed", Some("succeeded"), 1),
            record("j1", "Job", "Compile", 1, "completed", Some("succeeded"), 1),
            record("s2", "Stage", "Test", 2, "inProgress", None, 1),
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
            ("Test", StepStatus::Active)
        ]
    );
    assert_eq!(running.latest["current_step"], "Test");
    assert_eq!(running.latest["run_id"], "41");

    // The same answers again rewrite nothing.
    let revision = fx.get(&job.id).revision;
    fx.clock.advance(100);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).revision, revision);

    api.builds(vec![build(41, SHA, "completed", Some("succeeded"))]);
    step(&fx, &mut monitor);
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Succeeded);
    let run = CiRun {
        id: 41,
        attempt: 1,
        head_sha: SHA.into(),
        url: Some("https://dev.azure.com/acme/0000/_build/results?buildId=41".into()),
        artifact: None,
    };
    assert_eq!(gate(&fx), GateState::Green { run: run.clone() });
    let outcome = ci_outcome(&done).unwrap();
    assert_eq!(outcome.gate, GateState::Green { run });
    assert_eq!(
        outcome.summary.as_deref(),
        Some("Azure Pipelines build 20261101.41 succeeded for 1111111")
    );
    assert_eq!(
        done.outcome.as_ref().unwrap().refs["run_conclusion"],
        "succeeded"
    );
}

#[test]
fn hs2_jz1dd2_only_a_branch_build_of_the_commit_itself_counts() {
    let fx = fixture();
    let api = FakeAzure::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    // A later commit's success, a pull request build and another branch's build never
    // cover the commit: no ancestry is known.
    let mut pull = build(52, SHA, "completed", Some("succeeded"));
    pull["reason"] = json!("pullRequest");
    let mut branch = build(53, SHA, "completed", Some("succeeded"));
    branch["sourceBranch"] = json!("refs/heads/feature");
    api.builds(vec![
        build(54, NEWER, "completed", Some("succeeded")),
        branch,
        pull,
    ]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx), GateState::Pending);
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    // A partially succeeded build is red.
    api.builds(vec![build(
        55,
        SHA,
        "completed",
        Some("partiallySucceeded"),
    )]);
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Red { run } if run.id == 55 && run.head_sha == SHA));
}

#[test]
fn hs2_jz1dd2_a_rewatch_ignores_the_judged_build_until_its_stages_are_retried() {
    let fx = fixture();
    let api = FakeAzure::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.builds(vec![build(61, SHA, "completed", Some("failed"))]);
    api.timeline(
        61,
        vec![record(
            "s1",
            "Stage",
            "Test",
            1,
            "completed",
            Some("failed"),
            1,
        )],
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(matches!(gate(&fx), GateState::Red { run } if run.id == 61 && run.attempt == 1));
    rewatch_ci(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    step(&fx, &mut monitor);
    assert_eq!(
        gate(&fx),
        GateState::Pending,
        "the judged build is not judged again"
    );

    // "Rerun failed jobs" keeps the build id and raises the stage's attempt.
    api.builds(vec![build(61, SHA, "inProgress", Some("failed"))]);
    api.timeline(
        61,
        vec![record("s1", "Stage", "Test", 1, "inProgress", None, 2)],
    );
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx), GateState::Pending);
    api.builds(vec![build(61, SHA, "completed", Some("succeeded"))]);
    api.timeline(
        61,
        vec![record(
            "s1",
            "Stage",
            "Test",
            1,
            "completed",
            Some("succeeded"),
            2,
        )],
    );
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 61 && run.attempt == 2));
}

#[test]
fn hs2_jz1dd2_cancel_patches_only_the_commits_unfinished_build() {
    let fx = fixture();
    let api = FakeAzure::default();
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
    api.builds(vec![build(81, SHA, "inProgress", None)]);
    api.serve(
        &format!("{PROJECT}/_apis/build/builds/81?api-version=7.1"),
        200,
        build(81, SHA, "cancelling", None),
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(api.sent(ApiMethod::Patch).is_empty());
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    step(&fx, &mut monitor);
    let patches = api.sent(ApiMethod::Patch);
    assert_eq!(patches.len(), 1);
    assert_eq!(
        patches[0].path,
        format!("{PROJECT}/_apis/build/builds/81?api-version=7.1")
    );
    assert_eq!(patches[0].body, Some(json!({"status": "cancelling"})));
    api.builds(vec![build(81, SHA, "completed", Some("canceled"))]);
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx), GateState::Cancelled);
}

#[test]
fn hs2_jz1dd2_credentials_and_rate_limits_keep_the_last_observation_and_404_ends_it() {
    let fx = fixture();
    let api = FakeAzure::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.builds(vec![build(41, SHA, "inProgress", None)]);
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    // 203 is the sign-in page Azure DevOps answers for a credential it does not accept.
    for status in [203, 401, 403, 429, 503] {
        api.serve(LIST, status, json!("<html>Sign in</html>"));
        step(&fx, &mut monitor);
        let seen = fx.get(&job.id);
        assert_eq!(seen.status, Status::Running, "{status}");
        assert_eq!(seen.latest["run_id"], "41");
        assert!(seen.outcome.is_none());
        assert_eq!(gate(&fx), GateState::Pending);
    }
    let fx = fixture();
    let api = FakeAzure::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx), GateState::Unknown);
}
