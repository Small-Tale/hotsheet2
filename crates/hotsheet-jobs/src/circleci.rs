//! HS2-17HVNP: the CircleCI watch source. One watch Job follows the CI gate of one
//! commit: the gate is green when a pipeline of the project on the shipping branch, built
//! from exactly that commit, has its workflows (one named workflow, or every workflow
//! for `*`) succeed.
//!
//! CircleCI's API v2 (<https://circleci.com/docs/api/v2/index.html>) lists a project's
//! pipelines by branch (`GET /project/{slug}/pipeline`, `items` with `id`, `number`,
//! `state` and `vcs.revision`), a pipeline's workflows (`GET /pipeline/{id}/workflow`,
//! `name`, `status`, `created_at`), a workflow's jobs (`GET /workflow/{id}/job`) and
//! cancels a workflow (`POST /workflow/{id}/cancel`). It exposes no repository
//! ancestry, so a CircleCI gate attests exact coverage only (see
//! [`CiProvider::attests_ancestors`]).
//!
//! HS2-56MTYN: a project names the repository it builds (`GET project/{slug}`,
//! `vcs_info.vcs_url`). When that repository is on a host the source was given
//! ([`RepositoryHosts`]: GitHub, or the configured GitLab), a later pipeline of the
//! branch whose revision contains the commit covers it as an ancestor, and a branch
//! whose newest pipeline is on another line, with nothing covering the commit,
//! supersedes it. Without such a host the gate keeps exact coverage.
//!
//! All access goes through the shared REST port ([`GithubApi`]) rooted at `/api/v2`, so
//! tests run offline with fixtures. A rerun is a new workflow of the same name in the
//! same pipeline; the newest of each name counts, and the run floor is the pipeline
//! number.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::ancestry::{HostRepository, MAX_RELATIONS_PER_POLL, RepositoryHosts, walk};
use crate::github::{
    ApiMethod, ApiRequest, ApiResponse, CIRCLECI_KIND, CiProvider, CiTarget, GithubApi, RunFloor,
    clean, query_value, transient,
};
use crate::model::{Job, Outcome};
use crate::monitor::{CancelReply, Observation, Observed, Poll, WatchError, WatchSource};
use crate::steps::{Step, StepStatus};

/// Pipelines of the commit whose workflows one poll reads; older ones wait.
const MAX_PIPELINES_PER_POLL: usize = 3;
/// Workflows whose jobs one poll reads.
const MAX_JOB_LISTS_PER_POLL: usize = 4;

#[derive(Clone, Debug, Deserialize)]
struct Page<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Pipeline {
    id: String,
    number: u64,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    vcs: Option<Vcs>,
}

/// HS2-56MTYN: what the project read needs: the repository it builds.
#[derive(Clone, Debug, Default, Deserialize)]
struct Project {
    #[serde(default)]
    vcs_info: Option<VcsInfo>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct VcsInfo {
    #[serde(default)]
    vcs_url: Option<String>,
}

impl Pipeline {
    fn revision(&self) -> Option<&str> {
        self.vcs.as_ref().and_then(|vcs| vcs.revision.as_deref())
    }

    /// Whether the pipeline was built from exactly the commit.
    fn own(&self, sha: &str) -> bool {
        self.revision()
            .is_some_and(|revision| revision.eq_ignore_ascii_case(sha))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Vcs {
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    branch: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Workflow {
    id: String,
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
}

impl Workflow {
    fn status(&self) -> &str {
        self.status.as_deref().unwrap_or("")
    }

    fn finished(&self) -> bool {
        matches!(
            self.status(),
            "success" | "failed" | "error" | "canceled" | "not_run" | "unauthorized"
        )
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct WorkflowJob {
    id: Option<String>,
    #[serde(default)]
    job_number: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

/// What a pipeline's counted workflows say, together.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Verdict {
    Success,
    Failed,
    Cancelled,
    Running,
    Waiting,
    /// HS2-56MTYN: the branch moved to another line and nothing covers the commit.
    Superseded,
}

/// The workflows a gate counts: the newest of each name, or of the one named.
fn counted<'a>(target: &CiTarget, workflows: &'a [Workflow]) -> Vec<&'a Workflow> {
    let mut newest: BTreeMap<&str, &Workflow> = BTreeMap::new();
    for workflow in workflows
        .iter()
        .filter(|workflow| target.workflow == "*" || workflow.name == target.workflow)
    {
        let entry = newest.entry(workflow.name.as_str()).or_insert(workflow);
        // RFC 3339 times of one provider compare in order as text.
        if workflow.created_at > entry.created_at {
            *entry = workflow;
        }
    }
    newest.into_values().collect()
}

fn verdict(pipeline: &Pipeline, workflows: &[&Workflow]) -> Verdict {
    // A pipeline whose configuration did not compile runs no workflow at all.
    if pipeline.state.as_deref() == Some("errored") {
        return Verdict::Failed;
    }
    if workflows.is_empty() {
        return Verdict::Waiting;
    }
    let any = |states: &[&str]| workflows.iter().any(|w| states.contains(&w.status()));
    if any(&["failed", "error", "unauthorized"]) {
        Verdict::Failed
    } else if any(&["canceled"]) {
        Verdict::Cancelled
    } else if workflows.iter().all(|w| w.status() == "success") {
        Verdict::Success
    } else if any(&["running", "failing", "on_hold"]) {
        Verdict::Running
    } else {
        Verdict::Waiting
    }
}

#[derive(Default)]
struct Cached {
    /// Workflows of a pipeline, by pipeline id. A finished workflow never changes.
    workflows: BTreeMap<String, Vec<Workflow>>,
    reported: Option<String>,
    /// The commit's newest judged pipeline's unfinished counted workflows, for cancel.
    cancellable: Vec<String>,
}

/// Observes CircleCI gates through a [`GithubApi`] REST port rooted at `/api/v2`.
pub struct CircleciSource<A: GithubApi> {
    api: A,
    jobs: BTreeMap<String, Cached>,
    /// HS2-56MTYN: the hosts that answer ancestry for a project's repository.
    hosts: RepositoryHosts,
    /// HS2-56MTYN: each project's repository on one of those hosts, once read.
    repositories: BTreeMap<String, Option<HostRepository>>,
}

impl<A: GithubApi> CircleciSource<A> {
    pub fn new(api: A) -> Self {
        Self {
            api,
            jobs: BTreeMap::new(),
            hosts: RepositoryHosts::new(),
            repositories: BTreeMap::new(),
        }
    }

    /// HS2-56MTYN: ask these hosts whether a later pipeline covers the commit.
    #[must_use]
    pub fn with_hosts(mut self, hosts: RepositoryHosts) -> Self {
        self.hosts = hosts;
        self
    }

    /// HS2-56MTYN: the repository the project builds, when a host can answer for it.
    /// A token that may not read the project (403) leaves exact coverage.
    fn repository(&mut self, project: &str) -> Result<Option<HostRepository>, WatchError> {
        if !self.hosts.any() {
            return Ok(None);
        }
        if let Some(known) = self.repositories.get(project) {
            return Ok(known.clone());
        }
        let response = self.get(format!("project/{project}"))?;
        let found = match response.status {
            200 => serde_json::from_str::<Project>(&response.body)
                .map_err(|_| transient(&response))?
                .vcs_info
                .and_then(|info| info.vcs_url)
                .and_then(|url| self.hosts.repository(&url)),
            403 => None,
            404 => return Err(WatchError::Gone),
            _ => return Err(transient(&response)),
        };
        self.repositories.insert(project.to_owned(), found.clone());
        Ok(found)
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

    fn page<T: serde::de::DeserializeOwned>(&mut self, path: String) -> Result<Vec<T>, WatchError> {
        let response = self.get(path)?;
        match response.status {
            200 => serde_json::from_str::<Page<T>>(&response.body)
                .map(|page| page.items)
                .map_err(|_| transient(&response)),
            404 => Err(WatchError::Gone),
            _ => Err(transient(&response)),
        }
    }
}

/// The app's address of a workflow, for the project slugs the app names the same way.
fn workflow_url(target: &CiTarget, pipeline: &Pipeline, workflow: &Workflow) -> Option<String> {
    let mut parts = target.repo.splitn(3, '/');
    let vcs = match parts.next()? {
        "gh" | "github" => "github",
        "bb" | "bitbucket" => "bitbucket",
        _ => return None,
    };
    let (organization, project) = (parts.next()?, parts.next()?);
    let id = workflow
        .id
        .bytes()
        .all(|b| b.is_ascii_hexdigit() || b == b'-')
        .then_some(workflow.id.as_str())?;
    Some(format!(
        "https://app.circleci.com/pipelines/{vcs}/{}/{}/{}/workflows/{id}",
        query_value(organization),
        query_value(project),
        pipeline.number
    ))
}

impl<A: GithubApi> WatchSource for CircleciSource<A> {
    fn kind(&self) -> &str {
        CIRCLECI_KIND
    }

    /// HS2-YA5S82: `GET project/{slug}`'s `vcs_info.vcs_url`. A token that may not read
    /// the project (403) names none; a missing project is gone.
    fn pipeline_repository(&mut self, slug: &str) -> Option<Result<Option<String>, WatchError>> {
        let project = slug
            .split('/')
            .map(query_value)
            .collect::<Vec<_>>()
            .join("/");
        Some(self.get(format!("project/{project}")).and_then(|response| {
            match response.status {
                200 => serde_json::from_str::<Project>(&response.body)
                    .map(|project| project.vcs_info.and_then(|info| info.vcs_url))
                    .map_err(|_| transient(&response)),
                403 => Ok(None),
                404 => Err(WatchError::Gone),
                _ => Err(transient(&response)),
            }
        }))
    }

    fn poll(&mut self, job: &Job, _now_ms: u64) -> Result<Poll, WatchError> {
        let (target, floor) = job
            .external
            .as_ref()
            .and_then(CiTarget::parse)
            .filter(|(target, _)| target.provider == CiProvider::Circleci)
            .ok_or(WatchError::Gone)?;
        let project = target
            .repo
            .split('/')
            .map(query_value)
            .collect::<Vec<_>>()
            .join("/");
        let listed: Vec<Pipeline> = self.page(format!(
            "project/{project}/pipeline?branch={}",
            query_value(&target.branch)
        ))?;
        let mut listed: Vec<Pipeline> = listed
            .into_iter()
            .filter(|pipeline| {
                pipeline
                    .vcs
                    .as_ref()
                    .is_some_and(|vcs| vcs.branch.as_deref() == Some(target.branch.as_str()))
            })
            .collect();
        listed.sort_by_key(|pipeline| std::cmp::Reverse(pipeline.number));
        let mut unfinished = false;
        let mut diverged_first = false;
        let covering: Vec<usize> = match self.repository(&project)? {
            None => (0..listed.len())
                .filter(|index| listed[*index].own(&target.sha))
                .collect(),
            Some(repository) => {
                // HS2-56MTYN: a later pipeline whose revision contains the commit.
                let heads: Vec<Option<&str>> = listed.iter().map(Pipeline::revision).collect();
                let mut budget = MAX_RELATIONS_PER_POLL;
                let hosts = &mut self.hosts;
                let found = walk(&target.sha, &heads, |head| {
                    hosts.relation(&repository, &target.sha, head, &mut budget)
                })?;
                unfinished = found.unfinished;
                diverged_first = found.diverged_first;
                let mut covering = found.covering;
                // The commit's own pipelines always cover it.
                covering.extend((0..listed.len()).filter(|index| listed[*index].own(&target.sha)));
                covering.sort_unstable();
                covering.dedup();
                covering
            }
        };
        let mut pipelines: Vec<Pipeline> = covering
            .into_iter()
            .map(|index| listed[index].clone())
            .filter(|pipeline| {
                floor.is_none_or(|floor| {
                    RunFloor {
                        run_id: pipeline.number,
                        attempt: 1,
                    } > floor
                })
            })
            .collect();
        pipelines.truncate(MAX_PIPELINES_PER_POLL);
        // Each pipeline's workflows; a pipeline whose workflows all finished is kept.
        let mut judged = Vec::new();
        for pipeline in &pipelines {
            let known = self
                .jobs
                .get(&job.id)
                .and_then(|cached| cached.workflows.get(&pipeline.id))
                .filter(|workflows| {
                    !workflows.is_empty() && workflows.iter().all(Workflow::finished)
                })
                .cloned();
            let workflows = match known {
                Some(workflows) => workflows,
                None => {
                    let read: Vec<Workflow> =
                        self.page(format!("pipeline/{}/workflow", query_value(&pipeline.id)))?;
                    self.jobs
                        .entry(job.id.clone())
                        .or_default()
                        .workflows
                        .insert(pipeline.id.clone(), read.clone());
                    read
                }
            };
            judged.push((pipeline.clone(), workflows));
        }
        let verdicts: Vec<Verdict> = judged
            .iter()
            .map(|(pipeline, workflows)| verdict(pipeline, &counted(&target, workflows)))
            .collect();
        // A succeeded pipeline of the commit is the green; otherwise the newest decides.
        let current = verdicts
            .iter()
            .position(|verdict| *verdict == Verdict::Success)
            .or((!judged.is_empty()).then_some(0));
        let (state_verdict, current) = match current {
            Some(index) => (verdicts[index], Some(&judged[index])),
            None if diverged_first => (Verdict::Superseded, None),
            None => (Verdict::Waiting, None),
        };
        // The counted workflows' jobs are the Job's steps.
        let mut steps = Vec::new();
        let mut running_step = None;
        if let Some((_, workflows)) = current {
            for workflow in counted(&target, workflows)
                .into_iter()
                .take(MAX_JOB_LISTS_PER_POLL)
            {
                let jobs: Vec<WorkflowJob> =
                    self.page(format!("workflow/{}/job", query_value(&workflow.id)))?;
                for job in &jobs {
                    let name = job.name.clone().unwrap_or_else(|| "job".to_owned());
                    if running_step.is_none() && job.status.as_deref() == Some("running") {
                        running_step = Some(format!("{}: {name}", workflow.name));
                    }
                    steps.push(step_of(workflow, job, &name));
                }
            }
        }
        steps.truncate(crate::steps::MAX_STEPS);
        let cached = self.jobs.entry(job.id.clone()).or_default();
        // Only the commit's own pipeline's workflows are ever cancelled.
        cached.cancellable = current
            .filter(|(pipeline, _)| pipeline.own(&target.sha))
            .map(|(_, workflows)| {
                counted(&target, workflows)
                    .into_iter()
                    .filter(|workflow| !workflow.finished())
                    .map(|workflow| workflow.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let fingerprint = format!(
            "{state_verdict:?}|{:?}|{:?}",
            current.map(|(pipeline, workflows)| (pipeline.number, workflows)),
            steps
                .iter()
                .map(|step| (&step.id, step.status))
                .collect::<Vec<_>>()
        );
        if cached.reported.as_deref() == Some(fingerprint.as_str()) {
            return Ok(Poll::NotModified);
        }
        // A lookup left for the next poll means this verdict is not final.
        cached.reported = (!unfinished).then_some(fingerprint);
        let outcome_of = |gate: &str| {
            current.map(|(pipeline, workflows)| outcome(&target, gate, pipeline, workflows))
        };
        let state = match (
            state_verdict,
            outcome_of("green"),
            outcome_of("red"),
            outcome_of("cancelled"),
        ) {
            (Verdict::Success, Some(green), _, _) => Observed::Succeeded(green),
            (Verdict::Failed, _, Some(red), _) => Observed::Failed(red),
            (Verdict::Cancelled, _, _, Some(cancelled)) => Observed::Cancelled(cancelled),
            (Verdict::Superseded, ..) => Observed::Failed(superseded(&target)),
            (Verdict::Running, ..) => Observed::Running,
            _ => Observed::Waiting,
        };
        let mut latest = BTreeMap::new();
        if let Some((pipeline, _)) = current {
            latest.insert("run_id".to_owned(), pipeline.number.to_string());
            latest.insert("run_attempt".to_owned(), "1".to_owned());
            latest.insert(
                "run_head_sha".to_owned(),
                clean(pipeline.revision().unwrap_or(&target.sha), 64),
            );
            latest.insert(
                "run_status".to_owned(),
                format!("{state_verdict:?}").to_ascii_lowercase(),
            );
        }
        if let Some(step) = running_step {
            latest.insert("current_step".to_owned(), clean(&step, 200));
        }
        let url = current.and_then(|(pipeline, workflows)| {
            counted(&target, workflows)
                .first()
                .and_then(|workflow| workflow_url(&target, pipeline, workflow))
        });
        Ok(Poll::Observed(Box::new(Observation {
            steps: current.map(|_| steps),
            latest,
            url,
            ..Observation::new(state)
        })))
    }

    fn cancel(&mut self, job: &Job) -> CancelReply {
        if job.external.as_ref().and_then(CiTarget::parse).is_none() {
            return CancelReply::Unsupported;
        }
        // Only the unfinished workflows of the commit's own pipeline are cancelled.
        let workflows = self
            .jobs
            .get(&job.id)
            .map(|cached| cached.cancellable.clone())
            .unwrap_or_default();
        if workflows.is_empty() {
            return CancelReply::Unsupported;
        }
        for workflow in workflows {
            let request = ApiRequest {
                method: ApiMethod::Post,
                path: format!("workflow/{}/cancel", query_value(&workflow)),
                if_none_match: None,
                body: None,
            };
            match self.api.send(&request) {
                Ok(response) if (200..300).contains(&response.status) => {}
                Ok(response) if matches!(response.status, 401 | 403 | 404) => {
                    return CancelReply::Failed(
                        "CircleCI did not permit cancelling this workflow".into(),
                    );
                }
                Ok(response) => {
                    return CancelReply::Failed(format!(
                        "CircleCI answered {} to the cancel",
                        response.status
                    ));
                }
                Err(_) => {
                    return CancelReply::Failed(
                        "CircleCI could not be reached to cancel the workflow".into(),
                    );
                }
            }
        }
        CancelReply::Requested
    }

    fn forget(&mut self, job_id: &str) {
        self.jobs.remove(job_id);
    }
}

/// The outcome record of a finished CircleCI watch, with the same refs as GitHub's.
fn outcome(target: &CiTarget, gate: &str, pipeline: &Pipeline, workflows: &[Workflow]) -> Outcome {
    let counted = counted(target, workflows);
    let mut refs = BTreeMap::from([
        ("gate".to_owned(), gate.to_owned()),
        ("commit".to_owned(), target.sha.clone()),
        ("repo".to_owned(), target.repo.clone()),
        ("branch".to_owned(), target.branch.clone()),
        ("workflow".to_owned(), clean(&target.workflow, 200)),
        ("run_id".to_owned(), pipeline.number.to_string()),
        ("run_attempt".to_owned(), "1".to_owned()),
        // The commit itself, or (HS2-56MTYN) a later revision containing it.
        (
            "run_head_sha".to_owned(),
            clean(pipeline.revision().unwrap_or(&target.sha), 64),
        ),
        (
            "run_conclusion".to_owned(),
            clean(
                &counted
                    .iter()
                    .map(|workflow| workflow.status())
                    .collect::<Vec<_>>()
                    .join(","),
                64,
            ),
        ),
    ]);
    if let Some(url) = counted
        .first()
        .and_then(|workflow| workflow_url(target, pipeline, workflow))
        .filter(|url| url.len() <= 500)
    {
        refs.insert("run_url".to_owned(), url);
    }
    let verdict = match gate {
        "green" => "passed",
        "red" => "failed",
        _ => "was cancelled",
    };
    Outcome {
        summary: Some(format!(
            "CircleCI pipeline {} {verdict} for {}",
            pipeline.number,
            &target.sha[..7]
        )),
        refs,
    }
}

/// HS2-56MTYN: the outcome of a gate the branch moved past, with no pipeline of its own.
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
            "CircleCI: {} moved past {} to another line",
            clean(&target.branch, 200),
            &target.sha[..7]
        )),
        refs,
    }
}

fn step_of(workflow: &Workflow, job: &WorkflowJob, name: &str) -> Step {
    let status = match job.status.as_deref() {
        Some("success") => StepStatus::Succeeded,
        Some("not_run" | "retried") => StepStatus::Skipped,
        Some("canceled") => StepStatus::Cancelled,
        Some("failed" | "infrastructure_fail" | "timedout" | "terminated-unknown") => {
            StepStatus::Failed
        }
        Some("running") => StepStatus::Active,
        _ => StepStatus::Pending,
    };
    let key = job
        .job_number
        .map(|number| number.to_string())
        .or_else(|| job.id.clone())
        .unwrap_or_else(|| name.to_owned());
    // A step id is a token: the workflow's id and the job's number, never their names.
    let id: String = format!("job-{}-{key}", workflow.id)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.:".contains(c) {
                c
            } else {
                '-'
            }
        })
        .take(120)
        .collect();
    Step {
        id,
        label: clean(&format!("{}: {name}", workflow.name), 200),
        status,
        done: None,
        total: None,
        unit: None,
        weight: 1,
        children: Vec::new(),
    }
}
