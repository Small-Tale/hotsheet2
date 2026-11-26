//! HS2-TPAHYA: the GitLab CI watch source. One watch Job follows the CI gate of one
//! commit, with the semantics of the GitHub Actions watch (HS2-3R9V0R): the gate is
//! green when a successful push pipeline of the shipping branch has the commit as its
//! head or as an ancestor of its head.
//!
//! All provider access goes through the same REST port as GitHub ([`GithubApi`], which
//! takes paths relative to an API root), so tests run offline with fixtures. Paths are
//! relative to GitLab's `/api/v4`.
//!
//! GitLab's API exposes no digest for job artifacts, so a GitLab verdict records no
//! artifact identity (HS2-3YC577 applies to GitHub only).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::github::{
    ApiMethod, ApiRequest, ApiResponse, CiProvider, CiTarget, GITLAB_CI_KIND, GithubApi, RunFloor,
    clean, query_value, transient,
};
use crate::model::{Job, Outcome};
use crate::monitor::{CancelReply, Observation, Observed, Poll, WatchError, WatchSource};
use crate::steps::{Step, StepStatus};

/// How many push pipelines of the branch one poll looks at.
const PIPELINES_PER_PAGE: usize = 30;
/// Ancestry lookups one poll may start; the rest wait for the next poll.
const MAX_MERGE_BASES_PER_POLL: usize = 5;

#[derive(Clone, Debug, Deserialize)]
struct Pipeline {
    id: u64,
    sha: String,
    #[serde(default, rename = "ref")]
    reference: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    web_url: Option<String>,
}

impl Pipeline {
    fn status(&self) -> &str {
        self.status.as_deref().unwrap_or("")
    }

    fn finished(&self) -> bool {
        matches!(
            self.status(),
            "success" | "failed" | "canceled" | "skipped" | "manual"
        )
    }

    fn running(&self) -> bool {
        self.status() == "running"
    }
}

#[derive(Clone, Debug, Deserialize)]
struct PipelineJob {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Commit {
    id: String,
}

/// How a pipeline's head relates to the watched commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Relation {
    Covers,
    Before,
    Diverged,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decision {
    Pending { pipeline: Option<usize> },
    Green { pipeline: usize },
    Red { pipeline: usize },
    Cancelled { pipeline: usize },
    Superseded,
}

/// Judge the gate from push pipelines listed newest first (the same rules as GitHub).
fn decide(
    target: &CiTarget,
    floor: Option<RunFloor>,
    pipelines: &[Pipeline],
    mut relation: impl FnMut(&str) -> Result<Option<Relation>, WatchError>,
) -> Result<Decision, WatchError> {
    let mut covering = Vec::new();
    let mut diverged_first = false;
    for (index, pipeline) in pipelines.iter().enumerate() {
        let own = pipeline.sha.eq_ignore_ascii_case(&target.sha);
        let known = if own {
            Relation::Covers
        } else {
            match relation(&pipeline.sha)? {
                Some(known) => known,
                None => break,
            }
        };
        match known {
            Relation::Covers => covering.push(index),
            Relation::Diverged => {
                diverged_first |= covering.is_empty() && index == 0;
                continue;
            }
            Relation::Before | Relation::Unknown => break,
        }
        if own {
            break;
        }
    }
    // A GitLab retry keeps the pipeline id, so the floor is the pipeline id alone.
    let judged = |index: &usize| {
        floor.is_none_or(|floor| {
            RunFloor {
                run_id: pipelines[*index].id,
                attempt: 1,
            } > floor
        })
    };
    let covering: Vec<usize> = covering.into_iter().filter(judged).collect();
    if let Some(&green) = covering
        .iter()
        .find(|&&index| pipelines[index].status() == "success")
    {
        return Ok(Decision::Green { pipeline: green });
    }
    let Some(&newest) = covering.first() else {
        return Ok(if diverged_first {
            Decision::Superseded
        } else {
            Decision::Pending { pipeline: None }
        });
    };
    Ok(match pipelines[newest].status() {
        "failed" => Decision::Red { pipeline: newest },
        "canceled" => Decision::Cancelled { pipeline: newest },
        // Running, queued, skipped or waiting for a person: not the green the gate
        // needs, and not a failure.
        _ => Decision::Pending {
            pipeline: Some(newest),
        },
    })
}

#[derive(Default)]
struct Cached {
    pipelines_etag: Option<String>,
    pipelines: Vec<Pipeline>,
    jobs_pipeline: Option<u64>,
    jobs_etag: Option<String>,
    jobs: Vec<PipelineJob>,
    reported: bool,
    /// The newest covering pipeline, for cancel: (id, head, finished).
    current: Option<(u64, String, bool)>,
}

/// Observes GitLab CI gates through a [`GithubApi`] REST port rooted at `/api/v4`.
pub struct GitlabCiSource<A: GithubApi> {
    api: A,
    jobs: BTreeMap<String, Cached>,
    /// Ancestry never changes, so one answer per (commit, head) is kept.
    relations: BTreeMap<(String, String), Relation>,
}

/// The URL-encoded project path GitLab takes in place of a numeric id.
fn project(target: &CiTarget) -> String {
    query_value(&target.repo)
}

impl<A: GithubApi> GitlabCiSource<A> {
    pub fn new(api: A) -> Self {
        Self {
            api,
            jobs: BTreeMap::new(),
            relations: BTreeMap::new(),
        }
    }

    fn get(&mut self, path: String, etag: Option<String>) -> Result<ApiResponse, WatchError> {
        let request = ApiRequest {
            method: ApiMethod::Get,
            path,
            if_none_match: etag,
            body: None,
        };
        self.api
            .send(&request)
            .map_err(|unavailable| WatchError::Transient {
                retry_after_ms: unavailable.retry_after_ms,
            })
    }

    /// Whether `head` contains the watched commit: their merge base is the commit.
    fn relation(
        &mut self,
        target: &CiTarget,
        head: &str,
        budget: &mut usize,
    ) -> Result<Option<Relation>, WatchError> {
        let key = (target.sha.clone(), head.to_ascii_lowercase());
        if let Some(known) = self.relations.get(&key) {
            return Ok(Some(*known));
        }
        if *budget == 0 || !head.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(None);
        }
        *budget -= 1;
        let path = format!(
            "projects/{}/repository/merge_base?refs%5B%5D={}&refs%5B%5D={}",
            project(target),
            target.sha,
            key.1
        );
        let response = self.get(path, None)?;
        let relation = match response.status {
            200 => {
                let base: Commit =
                    serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                let base = base.id.to_ascii_lowercase();
                if base == target.sha {
                    Relation::Covers
                } else if base == key.1 {
                    Relation::Before
                } else {
                    Relation::Diverged
                }
            }
            // A commit GitLab does not know (yet). Not cached: it can appear.
            400 | 404 => return Ok(Some(Relation::Unknown)),
            _ => return Err(transient(&response)),
        };
        self.relations.insert(key, relation);
        Ok(Some(relation))
    }
}

impl<A: GithubApi> WatchSource for GitlabCiSource<A> {
    fn kind(&self) -> &str {
        GITLAB_CI_KIND
    }

    fn poll(&mut self, job: &Job, _now_ms: u64) -> Result<Poll, WatchError> {
        let (target, floor) = job
            .external
            .as_ref()
            .and_then(CiTarget::parse)
            .filter(|(target, _)| target.provider == CiProvider::GitlabCi)
            .ok_or(WatchError::Gone)?;
        let etag = self
            .jobs
            .get(&job.id)
            .and_then(|c| c.pipelines_etag.clone());
        let path = format!(
            "projects/{}/pipelines?ref={}&source=push&order_by=id&sort=desc&per_page={PIPELINES_PER_PAGE}",
            project(&target),
            query_value(&target.branch)
        );
        let response = self.get(path, etag)?;
        let mut changed = false;
        match response.status {
            304 => {}
            200 => {
                let listed: Vec<Pipeline> =
                    serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                let mut pipelines: Vec<Pipeline> = listed
                    .into_iter()
                    .filter(|pipeline| {
                        pipeline
                            .source
                            .as_deref()
                            .is_none_or(|source| source == "push")
                            && pipeline.reference.as_deref() == Some(target.branch.as_str())
                            && (target.workflow == "*"
                                || pipeline.name.as_deref() == Some(target.workflow.as_str()))
                    })
                    .collect();
                pipelines.sort_by_key(|pipeline| std::cmp::Reverse(pipeline.id));
                let cached = self.jobs.entry(job.id.clone()).or_default();
                cached.pipelines = pipelines;
                cached.pipelines_etag = response.etag.clone();
                changed = true;
            }
            // The project does not exist or cannot be read with these credentials.
            404 => return Err(WatchError::Gone),
            _ => return Err(transient(&response)),
        }
        let pipelines = self
            .jobs
            .get(&job.id)
            .map(|cached| cached.pipelines.clone())
            .unwrap_or_default();
        let mut budget = MAX_MERGE_BASES_PER_POLL;
        let mut unfinished = false;
        let decision = decide(&target, floor, &pipelines, |head| {
            let relation = self.relation(&target, head, &mut budget)?;
            unfinished |= relation.is_none();
            Ok(relation)
        })?;
        let current = match decision {
            Decision::Pending { pipeline } => pipeline,
            Decision::Green { pipeline }
            | Decision::Red { pipeline }
            | Decision::Cancelled { pipeline } => Some(pipeline),
            Decision::Superseded => None,
        }
        .map(|index| &pipelines[index]);

        // The pipeline's jobs are the Job's steps.
        if let Some(pipeline) = current {
            let same = self
                .jobs
                .get(&job.id)
                .is_some_and(|cached| cached.jobs_pipeline == Some(pipeline.id));
            let etag = self
                .jobs
                .get(&job.id)
                .filter(|_| same)
                .and_then(|cached| cached.jobs_etag.clone());
            let path = format!(
                "projects/{}/pipelines/{}/jobs?per_page=100",
                project(&target),
                pipeline.id
            );
            let response = self.get(path, etag)?;
            match response.status {
                304 => {}
                200 => {
                    let jobs: Vec<PipelineJob> =
                        serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                    let cached = self.jobs.entry(job.id.clone()).or_default();
                    cached.jobs = jobs;
                    cached.jobs_pipeline = Some(pipeline.id);
                    cached.jobs_etag = response.etag.clone();
                    changed = true;
                }
                _ => return Err(transient(&response)),
            }
        }

        let cached = self.jobs.entry(job.id.clone()).or_default();
        cached.current = current.map(|p| (p.id, p.sha.clone(), p.finished()));
        if !changed && cached.reported {
            return Ok(Poll::NotModified);
        }
        cached.reported = !unfinished;
        let steps = current.map(|_| steps_of(&cached.jobs));
        let current_step = cached
            .jobs
            .iter()
            .find(|job| job.status.as_deref() == Some("running"))
            .and_then(|job| job.name.as_deref())
            .map(|name| clean(name, 200));
        let state = match decision {
            Decision::Pending { pipeline: None } => Observed::Waiting,
            Decision::Pending {
                pipeline: Some(index),
            } => {
                if pipelines[index].running() {
                    Observed::Running
                } else {
                    Observed::Waiting
                }
            }
            Decision::Green { pipeline } => {
                Observed::Succeeded(outcome(&target, "green", Some(&pipelines[pipeline])))
            }
            Decision::Red { pipeline } => {
                Observed::Failed(outcome(&target, "red", Some(&pipelines[pipeline])))
            }
            Decision::Cancelled { pipeline } => {
                Observed::Cancelled(outcome(&target, "cancelled", Some(&pipelines[pipeline])))
            }
            Decision::Superseded => Observed::Failed(outcome(&target, "superseded", None)),
        };
        let mut latest = BTreeMap::new();
        if let Some(pipeline) = current {
            latest.insert("run_id".to_owned(), pipeline.id.to_string());
            latest.insert("run_attempt".to_owned(), "1".to_owned());
            latest.insert("run_head_sha".to_owned(), clean(&pipeline.sha, 64));
            latest.insert("run_status".to_owned(), clean(pipeline.status(), 64));
        }
        if let Some(step) = current_step {
            latest.insert("current_step".to_owned(), step);
        }
        Ok(Poll::Observed(Box::new(Observation {
            steps,
            latest,
            url: current.and_then(|pipeline| safe_url(pipeline.web_url.as_deref(), 2000)),
            ..Observation::new(state)
        })))
    }

    fn cancel(&mut self, job: &Job) -> CancelReply {
        let Some((target, _)) = job.external.as_ref().and_then(CiTarget::parse) else {
            return CancelReply::Unsupported;
        };
        let Some((pipeline, head, finished)) = self
            .jobs
            .get(&job.id)
            .and_then(|cached| cached.current.clone())
        else {
            return CancelReply::Unsupported;
        };
        // Only the commit's own pipeline is ever cancelled from here.
        if finished || !head.eq_ignore_ascii_case(&target.sha) {
            return CancelReply::Unsupported;
        }
        let request = ApiRequest {
            method: ApiMethod::Post,
            path: format!("projects/{}/pipelines/{pipeline}/cancel", project(&target)),
            if_none_match: None,
            body: None,
        };
        match self.api.send(&request) {
            Ok(response) if (200..300).contains(&response.status) => CancelReply::Requested,
            Ok(response) if response.status == 403 || response.status == 404 => {
                CancelReply::Failed("GitLab did not permit cancelling this pipeline".into())
            }
            Ok(response) => {
                CancelReply::Failed(format!("GitLab answered {} to the cancel", response.status))
            }
            Err(_) => {
                CancelReply::Failed("GitLab could not be reached to cancel the pipeline".into())
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

/// The outcome record of a finished GitLab watch, with the same refs as GitHub's.
fn outcome(target: &CiTarget, gate: &str, pipeline: Option<&Pipeline>) -> Outcome {
    let mut refs = BTreeMap::from([
        ("gate".to_owned(), gate.to_owned()),
        ("commit".to_owned(), target.sha.clone()),
        ("repo".to_owned(), target.repo.clone()),
        ("branch".to_owned(), target.branch.clone()),
        ("workflow".to_owned(), clean(&target.workflow, 200)),
    ]);
    let summary = match pipeline {
        Some(pipeline) => {
            refs.insert("run_id".to_owned(), pipeline.id.to_string());
            refs.insert("run_attempt".to_owned(), "1".to_owned());
            refs.insert("run_head_sha".to_owned(), clean(&pipeline.sha, 64));
            if pipeline.finished() {
                refs.insert("run_conclusion".to_owned(), clean(pipeline.status(), 64));
            }
            if let Some(url) = safe_url(pipeline.web_url.as_deref(), 500) {
                refs.insert("run_url".to_owned(), url);
            }
            let verdict = match gate {
                "green" => "passed",
                "red" => "failed",
                _ => "was cancelled",
            };
            format!(
                "GitLab pipeline {} {verdict} for {}",
                pipeline.id,
                &target.sha[..7]
            )
        }
        None => format!(
            "{} is no longer on {}",
            &target.sha[..7],
            clean(&target.branch, 100)
        ),
    };
    Outcome {
        summary: Some(summary),
        refs,
    }
}

fn steps_of(jobs: &[PipelineJob]) -> Vec<Step> {
    jobs.iter()
        .take(crate::steps::MAX_STEPS)
        .map(|job| {
            let status = match job.status.as_deref() {
                Some("success") => StepStatus::Succeeded,
                Some("skipped" | "manual") => StepStatus::Skipped,
                Some("canceled") => StepStatus::Cancelled,
                Some("failed") => StepStatus::Failed,
                Some("running") => StepStatus::Active,
                _ => StepStatus::Pending,
            };
            let label = match (job.stage.as_deref(), job.name.as_deref()) {
                (Some(stage), Some(name)) => format!("{stage}: {name}"),
                (_, Some(name)) => name.to_owned(),
                _ => "job".to_owned(),
            };
            Step {
                id: format!("job-{}", job.id),
                label: clean(&label, 200),
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
