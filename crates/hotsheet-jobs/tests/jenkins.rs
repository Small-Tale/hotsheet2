//! HS2-7H0S8R: the Jenkins watch source and the CI gate API for `jenkins`, offline. A
//! scripted REST port stands in for Jenkins' JSON API with the shapes and status codes
//! its documentation gives (<https://www.jenkins.io/doc/book/using/remote-access-api/>:
//! `GET job/<name>/api/json?tree=...`, a job's `_class` and `builds` with `number`,
//! `result`, `building`, `url` and the Git plugin's `BuildData` action
//! `lastBuiltRevision.SHA1` and `branch[].name`
//! (<https://plugins.jenkins.io/git/#plugin-content-remote-api>); a multibranch project's
//! branch job is `job/<project>/job/<branch>`; a build is stopped with
//! `POST <build>/stop`, answered with a redirect). No test makes a network call.
//! Synthetic data only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::{CHECKOUT, Fixture, fixture};
use hotsheet_jobs::{
    ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, CiProvider, CiRun, CiTarget,
    CiWatchOptions, GateState, GithubApi, JENKINS_KIND, JenkinsSource, Monitor, MonitorConfig,
    Status, ci_outcome, ensure_ci_watch, gate_state, rewatch_ci,
};
use serde_json::{Value, json};

const SHA: &str = "1111111111111111111111111111111111111111";
const NEWER: &str = "2222222222222222222222222222222222222222";
/// The `tree` filter every read sends, percent-encoded.
const TREE: &str = "_class%2Cbuilds%5Bnumber%2Cresult%2Cbuilding%2Curl%2Cactions%5BlastBuiltRevision%5BSHA1%2Cbranch%5Bname%5D%5D%5D%5D%7B0%2C30%7D";
const FREESTYLE: &str = "hudson.model.FreeStyleProject";
const MULTIBRANCH: &str = "org.jenkinsci.plugins.workflow.multibranch.WorkflowMultiBranchProject";

#[derive(Default)]
struct Fake {
    routes: BTreeMap<String, (u16, String)>,
    log: Vec<ApiRequest>,
}

#[derive(Clone, Default)]
struct FakeJenkins(Arc<Mutex<Fake>>);

impl FakeJenkins {
    fn serve(&self, path: &str, status: u16, body: Value) {
        self.0
            .lock()
            .unwrap()
            .routes
            .insert(path.to_owned(), (status, body.to_string()));
    }

    /// What `GET <job>/api/json?tree=...` answers.
    fn job(&self, job: &str, class: &str, builds: Vec<Value>) {
        self.serve(
            &format!("{job}/api/json?tree={TREE}"),
            200,
            json!({"_class": class, "builds": builds}),
        );
    }

    fn requests(&self, method: ApiMethod) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|request| request.method == method)
            .map(|request| request.path.clone())
            .collect()
    }
}

impl GithubApi for FakeJenkins {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let mut fake = self.0.lock().unwrap();
        fake.log.push(request.clone());
        let Some((status, body)) = fake.routes.get(&request.path).cloned() else {
            // Jenkins answers an unknown job with an HTML 404 page.
            return Ok(ApiResponse {
                status: 404,
                etag: None,
                retry_after_ms: None,
                body: "<html><body>Not Found</body></html>".into(),
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

/// A build of `commit` on `branch` as the Git plugin's `BuildData` records it; the
/// first action is another action the tree filter leaves empty.
fn build(number: u64, commit: &str, branch: &str, result: Option<&str>, building: bool) -> Value {
    json!({
        "_class": "hudson.model.FreeStyleBuild",
        "number": number,
        "result": result,
        "building": building,
        "url": format!("https://jenkins.example.test/job/widgets/{number}/"),
        "actions": [
            {"_class": "hudson.model.CauseAction"},
            {"_class": "hudson.plugins.git.util.BuildData",
             "lastBuiltRevision": {"SHA1": commit, "branch": [{"name": branch}]}}
        ]
    })
}

fn target() -> CiTarget {
    CiTarget::jenkins("widgets", SHA, "*", "main").unwrap()
}

fn watcher(fx: &Fixture, api: &FakeJenkins) -> Monitor {
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
    monitor.register(Box::new(JenkinsSource::new(api.clone())));
    monitor
}

fn step(fx: &Fixture, monitor: &mut Monitor) {
    fx.clock.advance(400);
    monitor.tick().unwrap();
}

fn gate_of(fx: &Fixture, target: &CiTarget) -> GateState {
    gate_state(&fx.store, CHECKOUT, target).unwrap().state
}

fn gate(fx: &Fixture) -> GateState {
    gate_of(fx, &target())
}

#[test]
fn hs2_7h0s8r_a_jenkins_target_names_its_job_and_attests_exact_coverage_only() {
    let target = target();
    assert_eq!(target.provider, CiProvider::Jenkins);
    assert!(!target.provider.attests_ancestors());
    let external = target.external(None);
    assert_eq!(external.system, JENKINS_KIND);
    assert_eq!(CiTarget::parse(&external).unwrap().0, target);
    assert_eq!(target.job_key(), "jenkins:widgets:main:*");
    assert!(CiTarget::jenkins("team/sub/widgets", SHA, "*", "main").is_ok());
    for refused in [
        "",
        "team//widgets",
        "/widgets",
        "wid gets",
        "team/../x",
        "a/b/c/d/e/f/g/h/i/j/k",
    ] {
        assert!(
            CiTarget::jenkins(refused, SHA, "*", "main").is_err(),
            "{refused:?}"
        );
    }
    // A Jenkins build has no workflow of its own.
    assert!(CiTarget::jenkins("widgets", SHA, "CI", "main").is_err());
    assert_eq!(
        CiTarget::for_provider(CiProvider::Jenkins, "widgets", SHA, "*", "main").unwrap(),
        target
    );
}

#[test]
fn hs2_7h0s8r_a_commits_build_is_followed_to_green_without_client_polling() {
    let fx = fixture();
    let api = FakeJenkins::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    assert_eq!(job.kind.name, JENKINS_KIND);
    assert_eq!(gate(&fx), GateState::Pending);

    // No build of the commit yet: another commit's build is not it.
    api.job(
        "job/widgets",
        FREESTYLE,
        vec![build(40, NEWER, "origin/main", Some("SUCCESS"), false)],
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    // Building: no result yet.
    api.job(
        "job/widgets",
        FREESTYLE,
        vec![
            build(41, SHA, "origin/main", None, true),
            build(40, NEWER, "origin/main", Some("SUCCESS"), false),
        ],
    );
    step(&fx, &mut monitor);
    let running = fx.get(&job.id);
    assert_eq!(running.status, Status::Running);
    assert_eq!(running.latest["run_id"], "41");
    assert_eq!(running.latest["run_status"], "building");

    // The same answer again rewrites nothing.
    let revision = fx.get(&job.id).revision;
    fx.clock.advance(100);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).revision, revision);

    api.job(
        "job/widgets",
        FREESTYLE,
        vec![build(
            41,
            SHA,
            "refs/remotes/origin/main",
            Some("SUCCESS"),
            false,
        )],
    );
    step(&fx, &mut monitor);
    let done = fx.get(&job.id);
    assert_eq!(done.status, Status::Succeeded);
    let run = CiRun {
        id: 41,
        attempt: 1,
        head_sha: SHA.into(),
        url: Some("https://jenkins.example.test/job/widgets/41/".into()),
        artifact: None,
    };
    assert_eq!(gate(&fx), GateState::Green { run: run.clone() });
    let outcome = ci_outcome(&done).unwrap();
    assert_eq!(outcome.gate, GateState::Green { run });
    assert_eq!(
        outcome.summary.as_deref(),
        Some("Jenkins build 41 passed for 1111111")
    );
    assert_eq!(
        done.outcome.as_ref().unwrap().refs["run_conclusion"],
        "SUCCESS"
    );
    // Every read is a GET of the job with the tree filter; nothing else was asked.
    assert!(api.requests(ApiMethod::Post).is_empty());
}

#[test]
fn hs2_7h0s8r_only_a_build_of_the_commit_itself_on_the_branch_counts() {
    let fx = fixture();
    let api = FakeJenkins::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    // A later commit's success never covers the commit: no ancestry is known. The
    // commit built for another branch, or by a build without BuildData, does not count.
    let mut library_only = build(53, NEWER, "origin/main", Some("SUCCESS"), false);
    library_only["actions"].as_array_mut().unwrap().push(
        json!({"_class": "hudson.plugins.git.util.BuildData",
            "lastBuiltRevision": {"SHA1": "3333333333333333333333333333333333333333",
                "branch": [{"name": "origin/main"}]}}),
    );
    api.job(
        "job/widgets",
        FREESTYLE,
        vec![
            build(54, NEWER, "origin/main", Some("SUCCESS"), false),
            library_only,
            build(52, SHA, "origin/feature/main", Some("SUCCESS"), false),
            json!({"number": 51, "result": "SUCCESS", "building": false, "actions": [{}, null]}),
        ],
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx), GateState::Pending);
    assert_eq!(fx.get(&job.id).status, Status::Waiting);

    // A shared library checkout adds a second BuildData; the matching one counts. An
    // older successful build of the commit wins over a newer unstable rebuild.
    let mut with_library = build(55, SHA, "origin/main", Some("SUCCESS"), false);
    with_library["actions"].as_array_mut().unwrap().insert(
        1,
        json!({"_class": "hudson.plugins.git.util.BuildData",
            "lastBuiltRevision": {"SHA1": NEWER, "branch": [{"name": "master"}]}}),
    );
    api.job(
        "job/widgets",
        FREESTYLE,
        vec![
            build(56, SHA, "origin/main", Some("UNSTABLE"), false),
            with_library,
        ],
    );
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 55 && run.head_sha == SHA));
}

#[test]
fn hs2_7h0s8r_a_multibranch_project_is_read_through_its_branch_job() {
    let fx = fixture();
    let api = FakeJenkins::default();
    let target = CiTarget::jenkins("team/widgets", SHA, "*", "release/1.0").unwrap();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target, &CiWatchOptions::default()).unwrap();
    let project = "job/team/job/widgets";
    let branch_job = format!("{project}/job/release%252F1.0");
    api.serve(
        &format!("{project}/api/json?tree={TREE}"),
        200,
        json!({"_class": MULTIBRANCH}),
    );
    // Not indexed yet: the branch job answers 404, and the gate waits.
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(fx.get(&job.id).status, Status::Waiting);
    assert_eq!(gate_of(&fx, &target), GateState::Pending);

    // A branch job's builds are the branch's own; BuildData names the plain branch.
    api.job(
        &branch_job,
        "org.jenkinsci.plugins.workflow.job.WorkflowJob",
        vec![build(3, SHA, "release/1.0", Some("FAILURE"), false)],
    );
    step(&fx, &mut monitor);
    assert!(matches!(gate_of(&fx, &target), GateState::Red { run } if run.id == 3));
    // The project's class is read once; later polls go to the branch job directly.
    let reads = api.requests(ApiMethod::Get);
    assert_eq!(
        reads
            .iter()
            .filter(|path| path.starts_with(&format!("{project}/api/json")))
            .count(),
        1
    );
}

#[test]
fn hs2_7h0s8r_red_is_not_judged_again_and_stop_reaches_only_the_commits_build() {
    let fx = fixture();
    let api = FakeJenkins::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.job(
        "job/widgets",
        FREESTYLE,
        vec![build(61, SHA, "origin/main", Some("FAILURE"), false)],
    );
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
    api.job(
        "job/widgets",
        FREESTYLE,
        vec![
            build(62, SHA, "origin/main", Some("SUCCESS"), false),
            build(61, SHA, "origin/main", Some("FAILURE"), false),
        ],
    );
    step(&fx, &mut monitor);
    assert!(matches!(gate(&fx), GateState::Green { run } if run.id == 62));

    // Stop: a POST to the commit's own unfinished build, once asked.
    let fx = fixture();
    let api = FakeJenkins::default();
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
    api.job(
        "job/widgets",
        FREESTYLE,
        vec![build(81, SHA, "origin/main", None, true)],
    );
    api.serve("job/widgets/81/stop", 302, json!(null));
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert!(api.requests(ApiMethod::Post).is_empty());
    fx.store.request_cancel(CHECKOUT, &job.id).unwrap();
    step(&fx, &mut monitor);
    assert_eq!(api.requests(ApiMethod::Post), ["job/widgets/81/stop"]);
    api.job(
        "job/widgets",
        FREESTYLE,
        vec![build(81, SHA, "origin/main", Some("ABORTED"), false)],
    );
    step(&fx, &mut monitor);
    assert_eq!(gate(&fx), GateState::Cancelled);
}

#[test]
fn hs2_7h0s8r_credentials_and_rate_limits_keep_the_last_observation_and_404_ends_it() {
    let fx = fixture();
    let api = FakeJenkins::default();
    let job = ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    api.job(
        "job/widgets",
        FREESTYLE,
        vec![build(41, SHA, "origin/main", None, true)],
    );
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    // 401 (bad token), 403 (no permission) and 429 are transient: still running,
    // nothing judged.
    for status in [401, 403, 429, 503] {
        api.serve(
            &format!("job/widgets/api/json?tree={TREE}"),
            status,
            json!({"message": "synthetic"}),
        );
        step(&fx, &mut monitor);
        let seen = fx.get(&job.id);
        assert_eq!(seen.status, Status::Running, "{status}");
        assert_eq!(seen.latest["run_id"], "41");
        assert!(seen.outcome.is_none());
        assert_eq!(gate(&fx), GateState::Pending);
    }
    // A job that does not exist (or is hidden from these credentials) ends the watch
    // without a verdict.
    let fx = fixture();
    let api = FakeJenkins::default();
    ensure_ci_watch(&fx.store, CHECKOUT, &target(), &CiWatchOptions::default()).unwrap();
    let mut monitor = watcher(&fx, &api);
    monitor.tick().unwrap();
    assert_eq!(gate(&fx), GateState::Unknown);
}
