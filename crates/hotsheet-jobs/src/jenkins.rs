//! HS2-7H0S8R: the Jenkins watch source. One watch Job follows the CI gate of one
//! commit: the gate is green when a successful build of the job on the shipping branch
//! built exactly that commit.
//!
//! Jenkins' remote access API (<https://www.jenkins.io/doc/book/using/remote-access-api/>)
//! answers `GET job/<name>/api/json` for a job, narrowed with the `tree` parameter, and a
//! build's `result` is one of Jenkins' `hudson.model.Result` names (`SUCCESS`,
//! `UNSTABLE`, `FAILURE`, `NOT_BUILT`, `ABORTED`;
//! <https://javadoc.jenkins.io/hudson/model/Result.html>), `null` while `building` is
//! true. The commit a build checked out is in the Git plugin's `BuildData` action,
//! `lastBuiltRevision.SHA1` with the branch names it built
//! (<https://plugins.jenkins.io/git/#plugin-content-remote-api>). Jenkins has no
//! build-list-by-commit filter, so each poll scans the job's newest builds for the
//! commit. It exposes no repository ancestry, so a Jenkins gate attests exact coverage
//! only (see [`CiProvider::attests_ancestors`]).
//!
//! HS2-PJ64A4: on a busy job the commit's build can fall out of the newest
//! [`BUILDS_PER_POLL`]. When that first read is full and holds none of the commit's
//! builds, the poll reads deeper slices of `allBuilds` with the remote access API's
//! documented range syntax (`{M,N}`: from the M-th element, inclusive, to the N-th,
//! exclusive), [`DEEP_SLICE`] builds at a time and at most [`MAX_DEEP_SLICES`] per poll.
//! A new build shifts every offset, so slices are de-duplicated by build number. The
//! search stops at a short or empty slice, at the first slice holding the commit, or at
//! a slice reaching the rewatch floor. A spent budget leaves the gate pending.
//!
//! A multibranch project (`_class` ending in `MultiBranchProject`) holds one job per
//! branch, `job/<project>/job/<branch>`, whose name encodes `/` as `%2F`
//! (<https://www.jenkins.io/doc/book/pipeline/multibranch/>). Its builds are the
//! branch's own; a plain job's build counts only when its `BuildData` names the branch.
//!
//! All access goes through the shared REST port ([`GithubApi`]) rooted at the Jenkins
//! URL, so tests run offline with fixtures. A rebuild is a new build with a new number,
//! so the run floor is the build number.
//!
//! HS2-SSKBRB: a Pipeline job's stages are the watch's steps. Jenkins core exposes no
//! stage data; the Pipeline: REST API plugin
//! (<https://github.com/jenkinsci/pipeline-stage-view-plugin/blob/master/rest-api/README.md>)
//! answers `GET <build>/wfapi/describe` with `stages[]` (`id`, `name`, `status`). Only
//! the judged build is described. Without the plugin, or for a job that is not a
//! Pipeline, the describe answers 404 and the watch reports no steps.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::github::{
    ApiMethod, ApiRequest, ApiResponse, CiProvider, CiTarget, GithubApi, JENKINS_KIND, RunFloor,
    clean, query_value, transient,
};
use crate::model::{Job, Outcome};
use crate::monitor::{CancelReply, Observation, Observed, Poll, WatchError, WatchSource};
use crate::steps::{Step, StepStatus};

/// How many of the job's newest builds one poll scans for the commit.
const BUILDS_PER_POLL: usize = 30;
/// HS2-PJ64A4: how many builds one deeper read of `allBuilds` asks for.
const DEEP_SLICE: usize = 100;
/// HS2-PJ64A4: how many deeper reads one poll may make when the commit's build is not
/// among the newest.
const MAX_DEEP_SLICES: usize = 3;

/// What the gate needs of each build.
const BUILD_FIELDS: &str =
    "number,result,building,url,actions[lastBuiltRevision[SHA1,branch[name]]]";

/// The `tree` filter of every job read: the class, and per build what the gate needs.
fn tree() -> String {
    format!("_class,builds[{BUILD_FIELDS}]{{0,{BUILDS_PER_POLL}}}")
}

/// HS2-PJ64A4: the `tree` filter of a deeper read, the builds `from..from + DEEP_SLICE`.
fn deep_tree(from: usize) -> String {
    format!("allBuilds[{BUILD_FIELDS}]{{{from},{}}}", from + DEEP_SLICE)
}

/// HS2-PJ64A4: a deeper read. `allBuilds` is absent when the job does not export it.
#[derive(Clone, Debug, Default, Deserialize)]
struct DeepPage {
    #[serde(default, rename = "allBuilds")]
    all_builds: Option<Vec<Build>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct JobPage {
    #[serde(default, rename = "_class")]
    class: Option<String>,
    #[serde(default)]
    builds: Vec<Build>,
}

impl JobPage {
    fn multibranch(&self) -> bool {
        self.class
            .as_deref()
            .is_some_and(|class| class.ends_with("MultiBranchProject"))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Build {
    number: u64,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    building: bool,
    #[serde(default)]
    url: Option<String>,
    /// Heterogeneous actions; the `tree` filter leaves `{}` for the ones without a
    /// `lastBuiltRevision`, and a `null` for some.
    #[serde(default)]
    actions: Vec<Option<Action>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Action {
    #[serde(default, rename = "lastBuiltRevision")]
    last_built_revision: Option<Revision>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Revision {
    #[serde(default, rename = "SHA1")]
    sha1: Option<String>,
    #[serde(default)]
    branch: Vec<BranchName>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct BranchName {
    #[serde(default)]
    name: Option<String>,
}

impl Build {
    fn result(&self) -> &str {
        if self.building {
            ""
        } else {
            self.result.as_deref().unwrap_or("")
        }
    }

    fn finished(&self) -> bool {
        !self.building && self.result.is_some()
    }

    /// The commit's own build: a `BuildData` with exactly the commit, and for a plain
    /// job the branch among the names it built. A pipeline that also checks out a
    /// shared library has more than one `BuildData`; only the matching one counts.
    fn built(&self, target: &CiTarget, branch_job: bool) -> bool {
        self.actions
            .iter()
            .flatten()
            .filter_map(|action| action.last_built_revision.as_ref())
            .any(|revision| {
                revision
                    .sha1
                    .as_deref()
                    .is_some_and(|sha| sha.eq_ignore_ascii_case(&target.sha))
                    && (branch_job
                        || revision.branch.iter().any(|name| {
                            name.name
                                .as_deref()
                                .is_some_and(|name| names_branch(name, &target.branch))
                        }))
            })
    }
}

/// Whether a Git plugin branch name is the branch: `main`, `refs/heads/main`,
/// `origin/main` or `refs/remotes/<remote>/main`.
fn names_branch(name: &str, branch: &str) -> bool {
    name == branch
        || name.strip_prefix("refs/heads/") == Some(branch)
        || name.strip_prefix("origin/") == Some(branch)
        || name
            .strip_prefix("refs/remotes/")
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(_, rest)| rest == branch)
}

/// `GET <build>/wfapi/describe` of the Pipeline: REST API plugin: the run and its stages.
#[derive(Clone, Debug, Default, Deserialize)]
struct Describe {
    #[serde(default)]
    stages: Vec<Stage>,
}

/// One stage. `status` is the plugin's `StatusExt`: `SUCCESS`, `FAILED`,
/// `IN_PROGRESS`, `NOT_EXECUTED`, `ABORTED`, `UNSTABLE` or `PAUSED_PENDING_INPUT`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Stage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

impl Stage {
    fn status(&self) -> StepStatus {
        match self.status.as_deref() {
            Some("SUCCESS") => StepStatus::Succeeded,
            // An unstable stage finished with failed tests, as the gate reads the build.
            Some("FAILED" | "UNSTABLE") => StepStatus::Failed,
            Some("ABORTED") => StepStatus::Cancelled,
            Some("NOT_EXECUTED") => StepStatus::Skipped,
            // Waiting for a person's input is still the stage the build is on.
            Some("IN_PROGRESS" | "PAUSED_PENDING_INPUT") => StepStatus::Active,
            _ => StepStatus::Pending,
        }
    }
}

/// The stages as steps, in the plugin's order.
fn steps_of(stages: &[Stage]) -> Vec<Step> {
    let mut seen = std::collections::BTreeSet::new();
    stages
        .iter()
        .take(crate::steps::MAX_STEPS)
        .enumerate()
        .map(|(index, stage)| {
            // A stage id is a flow node id (digits); anything else is made a token, and
            // a missing or repeated one falls back to the position.
            let id: String = stage
                .id
                .as_deref()
                .unwrap_or("")
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || "-_.:".contains(c) {
                        c
                    } else {
                        '-'
                    }
                })
                .take(100)
                .collect();
            let mut id = format!("stage-{id}");
            if id == "stage-" || !seen.insert(id.clone()) {
                id = format!("stage-at-{index}");
                seen.insert(id.clone());
            }
            Step {
                id,
                label: clean(stage.name.as_deref().unwrap_or("stage"), 200),
                status: stage.status(),
                done: None,
                total: None,
                unit: None,
                weight: 1,
                children: Vec::new(),
            }
        })
        .collect()
}

/// The stage the build is on: the first one running or waiting for input.
fn current_stage(stages: &[Stage]) -> Option<String> {
    stages
        .iter()
        .find(|stage| {
            matches!(
                stage.status.as_deref(),
                Some("IN_PROGRESS" | "PAUSED_PENDING_INPUT")
            )
        })
        .and_then(|stage| stage.name.as_deref())
        .map(|name| clean(name, 200))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decision {
    Pending { build: Option<usize> },
    Green { build: usize },
    Red { build: usize },
    Cancelled { build: usize },
}

/// Judge the gate from the commit's builds, newest first.
fn decide(floor: Option<RunFloor>, builds: &[Build]) -> Decision {
    let judged: Vec<usize> = (0..builds.len())
        .filter(|index| {
            floor.is_none_or(|floor| {
                RunFloor {
                    run_id: builds[*index].number,
                    attempt: 1,
                } > floor
            })
        })
        .collect();
    if let Some(&green) = judged
        .iter()
        .find(|&&index| builds[index].result() == "SUCCESS")
    {
        return Decision::Green { build: green };
    }
    let Some(&newest) = judged.first() else {
        return Decision::Pending { build: None };
    };
    match builds[newest].result() {
        // An unstable build finished with failed tests: not the green the gate needs.
        "FAILURE" | "UNSTABLE" => Decision::Red { build: newest },
        "ABORTED" => Decision::Cancelled { build: newest },
        // Building, not built (a skipped stage), or a result this source does not know:
        // not green, and not a finished failure.
        _ => Decision::Pending {
            build: Some(newest),
        },
    }
}

#[derive(Default)]
struct Cached {
    /// Whether the target job is a multibranch project, once read.
    multibranch: Option<bool>,
    reported: Option<(Vec<Build>, Option<Vec<Stage>>)>,
    /// The newest judged build, for cancel: (job path, number, finished).
    current: Option<(String, u64, bool)>,
    /// HS2-SSKBRB: the job answered 404 to a describe: no Pipeline: REST API.
    no_wfapi: bool,
    /// The stages last read: (build number, whether the build had finished, stages).
    /// A finished build's stages do not change, so they are not read again.
    stages: Option<(u64, bool, Option<Vec<Stage>>)>,
    /// HS2-PJ64A4: the job answered a deeper read without `allBuilds`.
    no_all_builds: bool,
}

/// Observes Jenkins CI gates through a [`GithubApi`] REST port rooted at the Jenkins URL.
pub struct JenkinsSource<A: GithubApi> {
    api: A,
    jobs: BTreeMap<String, Cached>,
}

/// `job/<a>/job/<b>` of a target's job full name.
fn job_path(target: &CiTarget) -> String {
    target
        .repo
        .split('/')
        .map(|segment| format!("job/{}", query_value(segment)))
        .collect::<Vec<_>>()
        .join("/")
}

/// The branch job of a multibranch project: its name is the branch with `/` as `%2F`,
/// percent-encoded again in the path.
fn branch_job_path(target: &CiTarget) -> String {
    format!(
        "{}/job/{}",
        job_path(target),
        query_value(&target.branch.replace('/', "%2F"))
    )
}

impl<A: GithubApi> JenkinsSource<A> {
    pub fn new(api: A) -> Self {
        Self {
            api,
            jobs: BTreeMap::new(),
        }
    }

    fn read(&mut self, path: &str) -> Result<ApiResponse, WatchError> {
        self.read_tree(path, &tree())
    }

    fn read_tree(&mut self, path: &str, tree: &str) -> Result<ApiResponse, WatchError> {
        let request = ApiRequest {
            method: ApiMethod::Get,
            path: format!("{path}/api/json?tree={}", query_value(tree)),
            if_none_match: None,
            body: None,
        };
        self.api
            .send(&request)
            .map_err(|unavailable| WatchError::Transient {
                retry_after_ms: unavailable.retry_after_ms,
            })
    }

    /// The page of the job whose builds count, with its path and whether it is a
    /// branch job. `None` when a multibranch project has no job for the branch yet.
    fn page(
        &mut self,
        job_id: &str,
        target: &CiTarget,
    ) -> Result<Option<(String, JobPage, bool)>, WatchError> {
        let known = self.jobs.get(job_id).and_then(|cached| cached.multibranch);
        if known != Some(true) {
            let path = job_path(target);
            let response = self.read(&path)?;
            let page = parse(&response)?;
            let multibranch = page.multibranch();
            self.jobs.entry(job_id.to_owned()).or_default().multibranch = Some(multibranch);
            if !multibranch {
                return Ok(Some((path, page, false)));
            }
        }
        let path = branch_job_path(target);
        let response = self.read(&path)?;
        if response.status == 404 {
            // The project has not indexed the branch yet.
            return Ok(None);
        }
        Ok(Some((path, parse(&response)?, true)))
    }

    /// HS2-PJ64A4: the commit's builds beyond the newest [`BUILDS_PER_POLL`], as listed.
    /// Empty when they are not found within this poll's budget.
    fn deeper(
        &mut self,
        job_id: &str,
        path: &str,
        target: &CiTarget,
        branch_job: bool,
        floor: Option<RunFloor>,
    ) -> Result<Vec<Build>, WatchError> {
        if self
            .jobs
            .get(job_id)
            .is_some_and(|cached| cached.no_all_builds)
        {
            return Ok(Vec::new());
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut found = Vec::new();
        for slice in 0..MAX_DEEP_SLICES {
            let from = BUILDS_PER_POLL + slice * DEEP_SLICE;
            let response = self.read_tree(path, &deep_tree(from))?;
            let page: DeepPage = match response.status {
                200 => serde_json::from_str(&response.body).map_err(|_| transient(&response))?,
                404 => return Err(WatchError::Gone),
                _ => return Err(transient(&response)),
            };
            let Some(builds) = page.all_builds else {
                self.jobs
                    .entry(job_id.to_owned())
                    .or_default()
                    .no_all_builds = true;
                break;
            };
            let short = builds.len() < DEEP_SLICE;
            let floored = reaches_floor(floor, &builds);
            for build in builds {
                if seen.insert(build.number) && build.built(target, branch_job) {
                    found.push(build);
                }
            }
            if short || floored || !found.is_empty() {
                break;
            }
        }
        Ok(found)
    }
}

impl<A: GithubApi> JenkinsSource<A> {
    /// HS2-SSKBRB: the judged build's stages. `None` when the job has no Pipeline: REST
    /// API (404) or the answer cannot be read; a spent limit or a server error is
    /// retried.
    fn stages(
        &mut self,
        job_id: &str,
        path: &str,
        build: &Build,
    ) -> Result<Option<Vec<Stage>>, WatchError> {
        let cached = self.jobs.entry(job_id.to_owned()).or_default();
        if cached.no_wfapi {
            return Ok(None);
        }
        if let Some((number, true, stages)) = &cached.stages
            && *number == build.number
        {
            return Ok(stages.clone());
        }
        let request = ApiRequest {
            method: ApiMethod::Get,
            path: format!("{path}/{}/wfapi/describe", build.number),
            if_none_match: None,
            body: None,
        };
        let response = self
            .api
            .send(&request)
            .map_err(|unavailable| WatchError::Transient {
                retry_after_ms: unavailable.retry_after_ms,
            })?;
        let stages = match response.status {
            200 => serde_json::from_str::<Describe>(&response.body)
                .ok()
                .map(|describe| describe.stages),
            404 => {
                self.jobs.entry(job_id.to_owned()).or_default().no_wfapi = true;
                return Ok(None);
            }
            429 | 500.. => return Err(transient(&response)),
            // 401 and 403 on the describe alone: the gate still reads; no steps.
            _ => None,
        };
        self.jobs.entry(job_id.to_owned()).or_default().stages =
            Some((build.number, build.finished(), stages.clone()));
        Ok(stages)
    }
}

/// HS2-PJ64A4: whether a read reached the rewatch floor: every build at or below it was
/// judged already, and so is every older one.
fn reaches_floor(floor: Option<RunFloor>, builds: &[Build]) -> bool {
    floor.is_some_and(|floor| builds.iter().any(|build| build.number <= floor.run_id))
}

fn parse(response: &ApiResponse) -> Result<JobPage, WatchError> {
    match response.status {
        200 => serde_json::from_str(&response.body).map_err(|_| transient(response)),
        // The job does not exist, or cannot be read with these credentials.
        404 => Err(WatchError::Gone),
        // 401 and 403 (credentials), 429, 5xx: retried with backoff.
        _ => Err(transient(response)),
    }
}

impl<A: GithubApi> WatchSource for JenkinsSource<A> {
    fn kind(&self) -> &str {
        JENKINS_KIND
    }

    fn poll(&mut self, job: &Job, _now_ms: u64) -> Result<Poll, WatchError> {
        let (target, floor) = job
            .external
            .as_ref()
            .and_then(CiTarget::parse)
            .filter(|(target, _)| target.provider == CiProvider::Jenkins)
            .ok_or(WatchError::Gone)?;
        let (path, builds) = match self.page(&job.id, &target)? {
            Some((path, page, branch_job)) => {
                // A full first read may have more beyond it; one reaching the floor has
                // nothing older the gate could judge.
                let full =
                    page.builds.len() >= BUILDS_PER_POLL && !reaches_floor(floor, &page.builds);
                let mut builds: Vec<Build> = page
                    .builds
                    .into_iter()
                    .filter(|build| build.built(&target, branch_job))
                    .collect();
                if builds.is_empty() && full {
                    builds = self.deeper(&job.id, &path, &target, branch_job, floor)?;
                }
                builds.sort_by_key(|build| std::cmp::Reverse(build.number));
                (path, builds)
            }
            None => (branch_job_path(&target), Vec::new()),
        };
        let decision = decide(floor, &builds);
        let current = match decision {
            Decision::Pending { build } => build,
            Decision::Green { build } | Decision::Red { build } | Decision::Cancelled { build } => {
                Some(build)
            }
        }
        .map(|index| builds[index].clone());
        let stages = match &current {
            Some(build) => self.stages(&job.id, &path, build)?,
            None => None,
        };
        let cached = self.jobs.entry(job.id.clone()).or_default();
        cached.current = current
            .as_ref()
            .map(|build| (path.clone(), build.number, build.finished()));
        let seen = (builds, stages);
        if cached.reported.as_ref() == Some(&seen) {
            return Ok(Poll::NotModified);
        }
        let stages = seen.1.clone();
        cached.reported = Some(seen);
        let state = match (decision, &current) {
            (Decision::Green { .. }, Some(build)) => {
                Observed::Succeeded(outcome(&target, "green", build))
            }
            (Decision::Red { .. }, Some(build)) => Observed::Failed(outcome(&target, "red", build)),
            (Decision::Cancelled { .. }, Some(build)) => {
                Observed::Cancelled(outcome(&target, "cancelled", build))
            }
            (_, Some(build)) if build.building => Observed::Running,
            _ => Observed::Waiting,
        };
        let mut latest = BTreeMap::new();
        if let Some(build) = &current {
            latest.insert("run_id".to_owned(), build.number.to_string());
            latest.insert("run_attempt".to_owned(), "1".to_owned());
            latest.insert("run_head_sha".to_owned(), target.sha.clone());
            let status = if build.building {
                "building"
            } else {
                build.result.as_deref().unwrap_or("unknown")
            };
            latest.insert("run_status".to_owned(), clean(status, 64));
            if let Some(stage) = stages.as_deref().and_then(current_stage) {
                latest.insert("current_step".to_owned(), stage);
            }
        }
        Ok(Poll::Observed(Box::new(Observation {
            steps: stages.as_deref().map(steps_of),
            latest,
            url: current
                .as_ref()
                .and_then(|build| safe_url(build.url.as_deref(), 2000)),
            ..Observation::new(state)
        })))
    }

    fn cancel(&mut self, job: &Job) -> CancelReply {
        if job.external.as_ref().and_then(CiTarget::parse).is_none() {
            return CancelReply::Unsupported;
        }
        // Every build judged here built the commit itself; a finished one is left.
        let Some((path, number, finished)) = self
            .jobs
            .get(&job.id)
            .and_then(|cached| cached.current.clone())
        else {
            return CancelReply::Unsupported;
        };
        if finished {
            return CancelReply::Unsupported;
        }
        let request = ApiRequest {
            method: ApiMethod::Post,
            path: format!("{path}/{number}/stop"),
            if_none_match: None,
            body: None,
        };
        match self.api.send(&request) {
            // Jenkins answers the stop with a redirect to the build.
            Ok(response) if (200..400).contains(&response.status) => CancelReply::Requested,
            Ok(response) if matches!(response.status, 401 | 403 | 404) => {
                CancelReply::Failed("Jenkins did not permit stopping this build".into())
            }
            Ok(response) => {
                CancelReply::Failed(format!("Jenkins answered {} to the stop", response.status))
            }
            Err(_) => CancelReply::Failed("Jenkins could not be reached to stop the build".into()),
        }
    }

    fn forget(&mut self, job_id: &str) {
        self.jobs.remove(job_id);
    }
}

fn safe_url(url: Option<&str>, max: usize) -> Option<String> {
    url.filter(|url| {
        url.starts_with("https://") && url.len() <= max && !url.chars().any(char::is_control)
    })
    .map(str::to_owned)
}

/// The outcome record of a finished Jenkins watch, with the same refs as GitHub's.
fn outcome(target: &CiTarget, gate: &str, build: &Build) -> Outcome {
    let mut refs = BTreeMap::from([
        ("gate".to_owned(), gate.to_owned()),
        ("commit".to_owned(), target.sha.clone()),
        ("repo".to_owned(), target.repo.clone()),
        ("branch".to_owned(), target.branch.clone()),
        ("workflow".to_owned(), clean(&target.workflow, 200)),
        ("run_id".to_owned(), build.number.to_string()),
        ("run_attempt".to_owned(), "1".to_owned()),
        // Only a build of exactly the commit is ever judged.
        ("run_head_sha".to_owned(), target.sha.clone()),
    ]);
    if let Some(result) = build.result.as_deref().filter(|_| build.finished()) {
        refs.insert("run_conclusion".to_owned(), clean(result, 64));
    }
    if let Some(url) = safe_url(build.url.as_deref(), 500) {
        refs.insert("run_url".to_owned(), url);
    }
    let verdict = match gate {
        "green" => "passed",
        "red" => "failed",
        _ => "was aborted",
    };
    Outcome {
        summary: Some(format!(
            "Jenkins build {} {verdict} for {}",
            build.number,
            &target.sha[..7]
        )),
        refs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "1111111111111111111111111111111111111111";

    fn build(number: u64, result: Option<&str>, building: bool) -> Build {
        Build {
            number,
            result: result.map(str::to_owned),
            building,
            url: None,
            actions: Vec::new(),
        }
    }

    #[test]
    fn hs2_7h0s8r_every_documented_result_maps_to_one_gate_decision() {
        for (result, building, expected) in [
            (Some("SUCCESS"), false, Decision::Green { build: 0 }),
            (Some("FAILURE"), false, Decision::Red { build: 0 }),
            (Some("UNSTABLE"), false, Decision::Red { build: 0 }),
            (Some("ABORTED"), false, Decision::Cancelled { build: 0 }),
            (
                Some("NOT_BUILT"),
                false,
                Decision::Pending { build: Some(0) },
            ),
            (None, true, Decision::Pending { build: Some(0) }),
            // A stale result while building again is not read.
            (Some("SUCCESS"), true, Decision::Pending { build: Some(0) }),
            (
                Some("SOMETHING_NEW"),
                false,
                Decision::Pending { build: Some(0) },
            ),
            (None, false, Decision::Pending { build: Some(0) }),
        ] {
            assert_eq!(
                decide(None, &[build(7, result, building)]),
                expected,
                "{result:?} building={building}"
            );
        }
        assert_eq!(decide(None, &[]), Decision::Pending { build: None });
    }

    #[test]
    fn hs2_7h0s8r_a_floor_hides_judged_builds_and_a_green_wins_over_a_newer_red() {
        let builds = [
            build(9, Some("FAILURE"), false),
            build(8, Some("SUCCESS"), false),
        ];
        assert_eq!(decide(None, &builds), Decision::Green { build: 1 });
        let floor = Some(RunFloor {
            run_id: 9,
            attempt: 1,
        });
        assert_eq!(decide(floor, &builds), Decision::Pending { build: None });
    }

    #[test]
    fn hs2_7h0s8r_branch_names_match_in_the_git_plugins_spellings_only() {
        for name in [
            "main",
            "refs/heads/main",
            "origin/main",
            "refs/remotes/upstream/main",
        ] {
            assert!(names_branch(name, "main"), "{name}");
        }
        for name in [
            "feature/main",
            "mainline",
            "refs/heads/feature/main",
            "upstream",
        ] {
            assert!(!names_branch(name, "main"), "{name}");
        }
        assert!(names_branch("origin/release/1.0", "release/1.0"));
    }

    #[test]
    fn hs2_7h0s8r_job_paths_encode_folders_and_multibranch_branch_names() {
        let target = CiTarget::jenkins("team/widgets", SHA, "*", "release/1.0").unwrap();
        assert_eq!(job_path(&target), "job/team/job/widgets");
        assert_eq!(
            branch_job_path(&target),
            "job/team/job/widgets/job/release%252F1.0"
        );
    }

    fn stage(id: Option<&str>, name: Option<&str>, status: Option<&str>) -> Stage {
        Stage {
            id: id.map(str::to_owned),
            name: name.map(str::to_owned),
            status: status.map(str::to_owned),
        }
    }

    #[test]
    fn hs2_sskbrb_every_documented_stage_status_maps_to_one_step_status() {
        let stages: Vec<Stage> = [
            ("SUCCESS", StepStatus::Succeeded),
            ("FAILED", StepStatus::Failed),
            ("UNSTABLE", StepStatus::Failed),
            ("ABORTED", StepStatus::Cancelled),
            ("NOT_EXECUTED", StepStatus::Skipped),
            ("IN_PROGRESS", StepStatus::Active),
            ("PAUSED_PENDING_INPUT", StepStatus::Active),
            ("SOMETHING_NEW", StepStatus::Pending),
        ]
        .iter()
        .enumerate()
        .map(|(index, (status, expected))| {
            let stage = stage(Some(&index.to_string()), Some(status), Some(status));
            assert_eq!(stage.status(), *expected, "{status}");
            stage
        })
        .collect();
        assert_eq!(stage(Some("1"), None, None).status(), StepStatus::Pending);
        let steps = steps_of(&stages);
        assert_eq!(steps.len(), stages.len());
        assert_eq!(steps[0].id, "stage-0");
        assert!(crate::steps::validate_steps(&steps).is_ok());
        // The first running or paused stage is the current one.
        assert_eq!(current_stage(&stages).as_deref(), Some("IN_PROGRESS"));
        let paused = [
            stage(Some("5"), Some("Build"), Some("SUCCESS")),
            stage(Some("10"), Some("Deploy"), Some("PAUSED_PENDING_INPUT")),
        ];
        assert_eq!(current_stage(&paused).as_deref(), Some("Deploy"));
        assert_eq!(current_stage(&paused[..1]), None);
        assert_eq!(current_stage(&[]), None);
        assert!(steps_of(&[]).is_empty());
    }

    #[test]
    fn hs2_sskbrb_stage_ids_become_unique_tokens_and_labels_are_cleaned() {
        let steps = steps_of(&[
            stage(Some("5"), Some("Build"), Some("SUCCESS")),
            stage(Some("5"), Some("Again"), Some("SUCCESS")),
            stage(None, None, Some("SUCCESS")),
            stage(Some("a/b {c}"), Some("line\nbreak"), None),
        ]);
        let ids: Vec<&str> = steps.iter().map(|step| step.id.as_str()).collect();
        assert_eq!(
            ids,
            ["stage-5", "stage-at-1", "stage-at-2", "stage-a-b--c-"]
        );
        assert_eq!(steps[2].label, "stage");
        assert!(crate::steps::validate_steps(&steps).is_ok());
        let many: Vec<Stage> = (0..crate::steps::MAX_STEPS + 3)
            .map(|index| stage(Some(&index.to_string()), Some("s"), Some("SUCCESS")))
            .collect();
        assert_eq!(steps_of(&many).len(), crate::steps::MAX_STEPS);
    }

    #[test]
    fn hs2_pj64a4_deeper_reads_ask_documented_allbuilds_ranges_and_stop_at_the_floor() {
        assert_eq!(
            deep_tree(30),
            "allBuilds[number,result,building,url,actions[lastBuiltRevision[SHA1,branch[name]]]]{30,130}"
        );
        assert!(tree().ends_with("{0,30}"));
        let builds = [build(40, None, true), build(31, Some("SUCCESS"), false)];
        assert!(!reaches_floor(None, &builds));
        for (run_id, reached) in [(30, false), (31, true), (35, true), (40, true), (41, true)] {
            assert_eq!(
                reaches_floor(Some(RunFloor { run_id, attempt: 1 }), &builds),
                reached,
                "{run_id}"
            );
        }
        assert!(!reaches_floor(
            Some(RunFloor {
                run_id: 9,
                attempt: 1
            }),
            &[]
        ));
    }
}
