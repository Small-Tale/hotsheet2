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
//! All access goes through the shared REST port ([`GithubApi`]) rooted at Buildkite's
//! `/v2`, so tests run offline with fixtures. A rebuild is a new build with a new
//! number, so the run floor is the build number.

use std::collections::BTreeMap;

use serde::Deserialize;

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
    Pending { build: Option<usize> },
    Green { build: usize },
    Red { build: usize },
    Cancelled { build: usize },
}

/// Judge the gate from the commit's branch builds, newest first.
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
        .find(|&&index| builds[index].state() == "passed")
    {
        return Decision::Green { build: green };
    }
    let Some(&newest) = judged.first() else {
        return Decision::Pending { build: None };
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
    builds: Vec<Build>,
    reported: Option<Vec<Build>>,
    /// The newest judged build, for cancel: (number, finished).
    current: Option<(u64, bool)>,
}

/// Observes Buildkite CI gates through a [`GithubApi`] REST port rooted at `/v2`.
pub struct BuildkiteSource<A: GithubApi> {
    api: A,
    jobs: BTreeMap<String, Cached>,
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
        let response = self.get(format!(
            "{pipeline}/builds?branch={}&commit={}&per_page={BUILDS_PER_PAGE}",
            query_value(&target.branch),
            target.sha
        ))?;
        let builds: Vec<Build> = match response.status {
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
                            && build
                                .commit
                                .as_deref()
                                .is_some_and(|commit| commit.eq_ignore_ascii_case(&target.sha))
                    })
                    .collect();
                builds.sort_by_key(|build| std::cmp::Reverse(build.number));
                builds
            }
            // The pipeline does not exist or cannot be read with this token.
            404 => return Err(WatchError::Gone),
            _ => return Err(transient(&response)),
        };
        let decision = decide(floor, &builds);
        let current = match decision {
            Decision::Pending { build } => build,
            Decision::Green { build } | Decision::Red { build } | Decision::Cancelled { build } => {
                Some(build)
            }
        }
        .map(|index| builds[index].clone());
        let cached = self.jobs.entry(job.id.clone()).or_default();
        cached.current = current
            .as_ref()
            .map(|build| (build.number, build.finished()));
        cached.builds.clone_from(&builds);
        if cached.reported.as_ref() == Some(&builds) {
            return Ok(Poll::NotModified);
        }
        cached.reported = Some(builds);
        let state = match (decision, &current) {
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
        // Every build judged here built the commit itself; a finished one is left.
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
