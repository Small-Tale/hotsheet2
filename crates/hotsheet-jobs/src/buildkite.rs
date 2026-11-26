//! HS2-17HVNP: the Buildkite watch source. One watch Job follows the CI gate of one
//! commit: the gate is green when a passed build of the pipeline on the shipping branch
//! built exactly that commit.
//!
//! Buildkite's REST API (<https://buildkite.com/docs/apis/rest-api/builds>) lists a
//! pipeline's builds filtered by `branch` and `commit`, newest first, each with its
//! `number`, `state`, `commit`, `branch`, `pull_request` and `jobs`. It exposes no
//! repository ancestry, so a Buildkite gate attests exact coverage only: a build of a
//! later commit never turns it green (see [`CiProvider::attests_ancestors`]).
//!
//! HS2-56MTYN: a pipeline names the repository it builds
//! (`GET organizations/{org}/pipelines/{slug}`, `repository`). When that repository is
//! on a host the source was given ([`RepositoryHosts`]: GitHub, or the configured
//! GitLab), each poll also reads the branch's newest builds and asks the host whether
//! a later build's head contains the commit, as the GitHub Actions watch does: a
//! passed later build covers the commit as an ancestor, and a branch whose newest build
//! is on another line, with nothing covering the commit, supersedes it. Without such a
//! host the gate keeps exact coverage.
//!
//! All access goes through the shared REST port ([`GithubApi`]) rooted at Buildkite's
//! `/v2`, so tests run offline with fixtures. A rebuild is a new build with a new
//! number, so the run floor is the build number.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::ancestry::{HostRepository, MAX_RELATIONS_PER_POLL, RepositoryHosts, walk};
use crate::github::{
    ApiMethod, ApiRequest, ApiResponse, BUILDKITE_KIND, CiProvider, CiTarget, GithubApi, RunFloor,
    clean, query_value, transient,
};
use crate::model::{Job, Outcome};
use crate::monitor::{CancelReply, Observation, Observed, Poll, WatchError, WatchSource};
use crate::steps::{Step, StepStatus};

/// How many builds of the commit one poll looks at.
const BUILDS_PER_PAGE: usize = 30;

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Build {
    number: u64,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    commit: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    /// Present for a pull request build; absent or null for a branch build.
    #[serde(default)]
    pull_request: Option<serde_json::Value>,
    #[serde(default)]
    web_url: Option<String>,
    #[serde(default)]
    jobs: Vec<BuildJob>,
}

impl Build {
    fn state(&self) -> &str {
        self.state.as_deref().unwrap_or("")
    }

    fn finished(&self) -> bool {
        matches!(
            self.state(),
            "passed" | "failed" | "canceled" | "skipped" | "not_run"
        )
    }

    fn running(&self) -> bool {
        matches!(self.state(), "running" | "failing" | "canceling")
    }

    /// Whether the build built exactly the commit.
    fn own(&self, sha: &str) -> bool {
        self.commit
            .as_deref()
            .is_some_and(|commit| commit.eq_ignore_ascii_case(sha))
    }
}

/// HS2-56MTYN: what the pipeline read needs: the repository it builds.
#[derive(Clone, Debug, Default, Deserialize)]
struct PipelineInfo {
    #[serde(default)]
    repository: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct BuildJob {
    id: String,
    /// `script`, `trigger`, `manual` or `waiter`; a waiter is a wait step, not work.
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decision {
    Pending {
        build: Option<usize>,
    },
    Green {
        build: usize,
    },
    Red {
        build: usize,
    },
    Cancelled {
        build: usize,
    },
    /// HS2-56MTYN: the branch moved to another line and nothing covers the commit.
    Superseded,
}

/// Judge the gate from branch builds listed newest first, of which `covering` (indexes,
/// newest first) cover the commit. `diverged_first`: the newest build is on another
/// line.
fn decide(
    floor: Option<RunFloor>,
    builds: &[Build],
    covering: &[usize],
    diverged_first: bool,
) -> Decision {
    let judged: Vec<usize> = covering
        .iter()
        .copied()
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
        .find(|&&index| builds[index].state() == "passed")
    {
        return Decision::Green { build: green };
    }
    let Some(&newest) = judged.first() else {
        return if diverged_first {
            Decision::Superseded
        } else {
            Decision::Pending { build: None }
        };
    };
    match builds[newest].state() {
        "failed" => Decision::Red { build: newest },
        "canceled" => Decision::Cancelled { build: newest },
        // Scheduled, running, failing (still running), blocked on a person, skipped or
        // not run: not the green the gate needs, and not a finished failure.
        _ => Decision::Pending {
            build: Some(newest),
        },
    }
}

#[derive(Default)]
struct Cached {
    reported: Option<(Vec<Build>, Decision)>,
    /// The newest judged build when it is the commit's own, for cancel:
    /// (number, finished).
    current: Option<(u64, bool)>,
}

/// Observes Buildkite CI gates through a [`GithubApi`] REST port rooted at `/v2`.
pub struct BuildkiteSource<A: GithubApi> {
    api: A,
    jobs: BTreeMap<String, Cached>,
    /// HS2-56MTYN: the hosts that answer ancestry for a pipeline's repository.
    hosts: RepositoryHosts,
    /// HS2-56MTYN: each pipeline's repository on one of those hosts, once read.
    repositories: BTreeMap<String, Option<HostRepository>>,
}

/// `organizations/{organization}/pipelines/{pipeline}` of a target.
fn pipeline_path(target: &CiTarget) -> Option<String> {
    let (organization, pipeline) = target.repo.split_once('/')?;
    Some(format!(
        "organizations/{}/pipelines/{}",
        query_value(organization),
        query_value(pipeline)
    ))
}

impl<A: GithubApi> BuildkiteSource<A> {
    pub fn new(api: A) -> Self {
        Self {
            api,
            jobs: BTreeMap::new(),
            hosts: RepositoryHosts::new(),
            repositories: BTreeMap::new(),
        }
    }

    /// HS2-56MTYN: ask these hosts whether a later build covers the commit.
    #[must_use]
    pub fn with_hosts(mut self, hosts: RepositoryHosts) -> Self {
        self.hosts = hosts;
        self
    }

    /// HS2-56MTYN: the repository the pipeline builds, when a host can answer for it.
    /// A token that may not read pipelines (403) leaves exact coverage.
    fn repository(&mut self, pipeline: &str) -> Result<Option<HostRepository>, WatchError> {
        if !self.hosts.any() {
            return Ok(None);
        }
        if let Some(known) = self.repositories.get(pipeline) {
            return Ok(known.clone());
        }
        let response = self.get(pipeline.to_owned())?;
        let found = match response.status {
            200 => serde_json::from_str::<PipelineInfo>(&response.body)
                .map_err(|_| transient(&response))?
                .repository
                .and_then(|url| self.hosts.repository(&url)),
            403 => None,
            404 => return Err(WatchError::Gone),
            _ => return Err(transient(&response)),
        };
        self.repositories.insert(pipeline.to_owned(), found.clone());
        Ok(found)
    }

    /// The branch's builds, newest first, of the commit only when `commit` is given.
    /// Pull request builds and other branches' builds never count.
    fn list(
        &mut self,
        pipeline: &str,
        target: &CiTarget,
        commit: Option<&str>,
    ) -> Result<Vec<Build>, WatchError> {
        let filter = commit.map_or_else(String::new, |commit| format!("&commit={commit}"));
        let response = self.get(format!(
            "{pipeline}/builds?branch={}{filter}&per_page={BUILDS_PER_PAGE}",
            query_value(&target.branch),
        ))?;
        match response.status {
            200 => {
                let listed: Vec<Build> =
                    serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                let mut builds: Vec<Build> = listed
                    .into_iter()
                    .filter(|build| {
                        build
                            .pull_request
                            .as_ref()
                            .is_none_or(serde_json::Value::is_null)
                            && build.branch.as_deref() == Some(target.branch.as_str())
                            && commit.is_none_or(|commit| {
                                build
                                    .commit
                                    .as_deref()
                                    .is_some_and(|built| built.eq_ignore_ascii_case(commit))
                            })
                    })
                    .collect();
                builds.sort_by_key(|build| std::cmp::Reverse(build.number));
                Ok(builds)
            }
            // The pipeline does not exist or cannot be read with this token.
            404 => Err(WatchError::Gone),
            _ => Err(transient(&response)),
        }
    }

    fn get(&mut self, path: String) -> Result<ApiResponse, WatchError> {
        let request = ApiRequest {
            method: ApiMethod::Get,
            path,
            if_none_match: None,
            body: None,
        };
        self.api
            .send(&request)
            .map_err(|unavailable| WatchError::Transient {
                retry_after_ms: unavailable.retry_after_ms,
            })
    }
}

impl<A: GithubApi> WatchSource for BuildkiteSource<A> {
    fn kind(&self) -> &str {
        BUILDKITE_KIND
    }

    fn poll(&mut self, job: &Job, _now_ms: u64) -> Result<Poll, WatchError> {
        let (target, floor) = job
            .external
            .as_ref()
            .and_then(CiTarget::parse)
            .filter(|(target, _)| target.provider == CiProvider::Buildkite)
            .ok_or(WatchError::Gone)?;
        let pipeline = pipeline_path(&target).ok_or(WatchError::Gone)?;
        let own = self.list(&pipeline, &target, Some(&target.sha))?;
        let mut unfinished = false;
        let (builds, covering, diverged_first) = match self.repository(&pipeline)? {
            None => {
                let covering: Vec<usize> = (0..own.len()).collect();
                (own, covering, false)
            }
            Some(repository) => {
                // HS2-56MTYN: the branch's newest builds, with the commit's own builds
                // that fell out of that page.
                let mut builds = self.list(&pipeline, &target, None)?;
                for build in own {
                    if !builds.iter().any(|listed| listed.number == build.number) {
                        builds.push(build);
                    }
                }
                builds.sort_by_key(|build| std::cmp::Reverse(build.number));
                let heads: Vec<Option<&str>> =
                    builds.iter().map(|build| build.commit.as_deref()).collect();
                let mut budget = MAX_RELATIONS_PER_POLL;
                let hosts = &mut self.hosts;
                let found = walk(&target.sha, &heads, |head| {
                    hosts.relation(&repository, &target.sha, head, &mut budget)
                })?;
                unfinished = found.unfinished;
                // The commit's own builds always cover it.
                let mut covering = found.covering;
                covering.extend((0..builds.len()).filter(|index| builds[*index].own(&target.sha)));
                covering.sort_unstable();
                covering.dedup();
                (builds, covering, found.diverged_first)
            }
        };
        let decision = decide(floor, &builds, &covering, diverged_first);
        let current = match decision {
            Decision::Pending { build } => build,
            Decision::Green { build } | Decision::Red { build } | Decision::Cancelled { build } => {
                Some(build)
            }
            Decision::Superseded => None,
        }
        .map(|index| builds[index].clone());
        let covered: Vec<Build> = covering
            .iter()
            .map(|index| builds[*index].clone())
            .collect();
        let cached = self.jobs.entry(job.id.clone()).or_default();
        // Only the commit's own build is ever cancelled.
        cached.current = current
            .as_ref()
            .filter(|build| build.own(&target.sha))
            .map(|build| (build.number, build.finished()));
        let seen = (covered, decision);
        if cached.reported.as_ref() == Some(&seen) {
            return Ok(Poll::NotModified);
        }
        // A lookup left for the next poll means this verdict is not final.
        cached.reported = (!unfinished).then_some(seen);
        let state = match (decision, &current) {
            (Decision::Superseded, _) => Observed::Failed(superseded(&target)),
            (Decision::Green { .. }, Some(build)) => {
                Observed::Succeeded(outcome(&target, "green", build))
            }
            (Decision::Red { .. }, Some(build)) => Observed::Failed(outcome(&target, "red", build)),
            (Decision::Cancelled { .. }, Some(build)) => {
                Observed::Cancelled(outcome(&target, "cancelled", build))
            }
            (_, Some(build)) if build.running() => Observed::Running,
            _ => Observed::Waiting,
        };
        let mut latest = BTreeMap::new();
        if let Some(build) = &current {
            latest.insert("run_id".to_owned(), build.number.to_string());
            latest.insert("run_attempt".to_owned(), "1".to_owned());
            if let Some(commit) = &build.commit {
                latest.insert("run_head_sha".to_owned(), clean(commit, 64));
            }
            latest.insert("run_status".to_owned(), clean(build.state(), 64));
            if let Some(step) = build
                .jobs
                .iter()
                .find(|job| job.state.as_deref() == Some("running"))
                .and_then(job_label)
            {
                latest.insert("current_step".to_owned(), clean(&step, 200));
            }
        }
        Ok(Poll::Observed(Box::new(Observation {
            steps: current.as_ref().map(|build| steps_of(&build.jobs)),
            latest,
            url: current
                .as_ref()
                .and_then(|build| safe_url(build.web_url.as_deref(), 2000)),
            ..Observation::new(state)
        })))
    }

    fn cancel(&mut self, job: &Job) -> CancelReply {
        let Some((target, _)) = job.external.as_ref().and_then(CiTarget::parse) else {
            return CancelReply::Unsupported;
        };
        let Some(pipeline) = pipeline_path(&target) else {
            return CancelReply::Unsupported;
        };
        // Only the commit's own unfinished build is kept for cancel.
        let Some((number, finished)) = self.jobs.get(&job.id).and_then(|cached| cached.current)
        else {
            return CancelReply::Unsupported;
        };
        if finished {
            return CancelReply::Unsupported;
        }
        let request = ApiRequest {
            method: ApiMethod::Put,
            path: format!("{pipeline}/builds/{number}/cancel"),
            if_none_match: None,
            body: None,
        };
        match self.api.send(&request) {
            Ok(response) if (200..300).contains(&response.status) => CancelReply::Requested,
            Ok(response) if matches!(response.status, 403 | 404 | 422) => {
                CancelReply::Failed("Buildkite did not permit cancelling this build".into())
            }
            Ok(response) => CancelReply::Failed(format!(
                "Buildkite answered {} to the cancel",
                response.status
            )),
            Err(_) => {
                CancelReply::Failed("Buildkite could not be reached to cancel the build".into())
            }
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

/// The outcome record of a finished Buildkite watch, with the same refs as GitHub's.
fn outcome(target: &CiTarget, gate: &str, build: &Build) -> Outcome {
    let mut refs = BTreeMap::from([
        ("gate".to_owned(), gate.to_owned()),
        ("commit".to_owned(), target.sha.clone()),
        ("repo".to_owned(), target.repo.clone()),
        ("branch".to_owned(), target.branch.clone()),
        ("workflow".to_owned(), clean(&target.workflow, 200)),
        ("run_id".to_owned(), build.number.to_string()),
        ("run_attempt".to_owned(), "1".to_owned()),
    ]);
    if let Some(commit) = &build.commit {
        refs.insert("run_head_sha".to_owned(), clean(commit, 64));
    }
    if build.finished() {
        refs.insert("run_conclusion".to_owned(), clean(build.state(), 64));
    }
    if let Some(url) = safe_url(build.web_url.as_deref(), 500) {
        refs.insert("run_url".to_owned(), url);
    }
    let verdict = match gate {
        "green" => "passed",
        "red" => "failed",
        _ => "was cancelled",
    };
    Outcome {
        summary: Some(format!(
            "Buildkite build {} {verdict} for {}",
            build.number,
            &target.sha[..7]
        )),
        refs,
    }
}

/// HS2-56MTYN: the outcome of a gate the branch moved past, with no build of its own.
fn superseded(target: &CiTarget) -> Outcome {
    let refs = BTreeMap::from([
        ("gate".to_owned(), "superseded".to_owned()),
        ("commit".to_owned(), target.sha.clone()),
        ("repo".to_owned(), target.repo.clone()),
        ("branch".to_owned(), target.branch.clone()),
        ("workflow".to_owned(), clean(&target.workflow, 200)),
    ]);
    Outcome {
        summary: Some(format!(
            "Buildkite: {} moved past {} to another line",
            clean(&target.branch, 200),
            &target.sha[..7]
        )),
        refs,
    }
}

fn job_label(job: &BuildJob) -> Option<String> {
    job.name.clone().or_else(|| job.label.clone())
}

fn steps_of(jobs: &[BuildJob]) -> Vec<Step> {
    jobs.iter()
        .filter(|job| job.kind.as_deref() != Some("waiter"))
        .take(crate::steps::MAX_STEPS)
        .map(|job| {
            let status = match job.state.as_deref() {
                Some("passed") => StepStatus::Succeeded,
                Some("skipped" | "broken" | "expired") => StepStatus::Skipped,
                Some("canceled" | "canceling") => StepStatus::Cancelled,
                Some("failed" | "timed_out" | "timing_out") => StepStatus::Failed,
                Some("running" | "assigned" | "accepted") => StepStatus::Active,
                _ => StepStatus::Pending,
            };
            Step {
                // A step id is a token; Buildkite job ids are UUIDs.
                id: format!("job-{}", job.id)
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || "-_.:".contains(c) {
                            c
                        } else {
                            '-'
                        }
                    })
                    .take(120)
                    .collect(),
                label: clean(&job_label(job).unwrap_or_else(|| "job".to_owned()), 200),
                status,
                done: None,
                total: None,
                unit: None,
                weight: 1,
                children: Vec::new(),
            }
        })
        .collect()
}
