//! HS2-JZ1DD2: the Azure Pipelines watch source. One watch Job follows the CI gate of
//! one commit: the gate is green when a succeeded build of the pipeline definition on
//! the shipping branch built exactly that commit.
//!
//! The Azure DevOps Build REST API 7.1 lists a definition's builds of a branch
//! (<https://learn.microsoft.com/en-us/rest/api/azure/devops/build/builds/list?view=azure-devops-rest-7.1>:
//! `GET {organization}/{project}/_apis/build/builds?definitions=&branchName=`, `value`
//! with `id`, `buildNumber`, `status`, `result`, `sourceBranch`, `sourceVersion`,
//! `reason` and `_links.web.href`). A build's timeline
//! (<https://learn.microsoft.com/en-us/rest/api/azure/devops/build/timeline/get?view=azure-devops-rest-7.1>)
//! has its stages and jobs with `state`, `result` and `attempt`; a build is cancelled by
//! updating its `status` to `cancelling`
//! (<https://learn.microsoft.com/en-us/rest/api/azure/devops/build/builds/update-build?view=azure-devops-rest-7.1>).
//! The API exposes no repository ancestry, so an Azure Pipelines gate attests exact
//! coverage only (see [`CiProvider::attests_ancestors`]).
//!
//! Retrying failed stages keeps the build id and raises the attempt of the records it
//! runs again, so the run floor is the build id with the timeline's highest attempt.
//! All access goes through the shared REST port ([`GithubApi`]) rooted at
//! `https://dev.azure.com`, so tests run offline with fixtures.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::github::{
    AZURE_PIPELINES_KIND, ApiMethod, ApiRequest, ApiResponse, CiProvider, CiTarget, GithubApi,
    RunFloor, clean, query_value, transient,
};
use crate::model::{Job, Outcome};
use crate::monitor::{CancelReply, Observation, Observed, Poll, WatchError, WatchSource};
use crate::steps::{Step, StepStatus};

/// How many of the branch's newest builds one poll looks at.
const BUILDS_PER_POLL: usize = 30;
const API_VERSION: &str = "7.1";

#[derive(Clone, Debug, Deserialize)]
struct BuildList {
    #[serde(default)]
    value: Vec<Build>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Build {
    id: u64,
    #[serde(default)]
    build_number: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    source_branch: Option<String>,
    #[serde(default)]
    source_version: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default, rename = "_links")]
    links: Option<Links>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Links {
    #[serde(default)]
    web: Option<Link>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Link {
    #[serde(default)]
    href: Option<String>,
}

impl Build {
    fn status(&self) -> &str {
        self.status.as_deref().unwrap_or("")
    }

    fn completed(&self) -> bool {
        self.status() == "completed"
    }

    /// The result, read only once the build completed: a retried build keeps its
    /// earlier result while it runs again.
    fn result(&self) -> &str {
        if self.completed() {
            self.result.as_deref().unwrap_or("")
        } else {
            ""
        }
    }

    fn running(&self) -> bool {
        matches!(self.status(), "inProgress" | "cancelling")
    }

    fn web_url(&self) -> Option<&str> {
        self.links.as_ref()?.web.as_ref()?.href.as_deref()
    }

    fn label(&self) -> String {
        self.build_number
            .as_deref()
            .map_or_else(|| self.id.to_string(), |number| clean(number, 100))
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Timeline {
    #[serde(default)]
    records: Vec<Record>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Record {
    #[serde(default)]
    id: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    order: Option<i64>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    attempt: Option<u32>,
}

impl Timeline {
    /// The build's attempt: the highest attempt any record ran, at least 1.
    fn attempt(&self) -> u32 {
        self.records
            .iter()
            .filter_map(|record| record.attempt)
            .max()
            .unwrap_or(1)
            .max(1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decision {
    Pending { build: Option<usize> },
    Green { build: usize },
    Red { build: usize },
    Cancelled { build: usize },
}

/// Judge the gate from the commit's branch builds, newest first. `attempts` holds the
/// attempt of a build whose id is the floor's, read from its timeline.
fn decide(floor: Option<RunFloor>, builds: &[Build], attempts: &BTreeMap<u64, u32>) -> Decision {
    let judged: Vec<usize> = (0..builds.len())
        .filter(|index| {
            let id = builds[*index].id;
            floor.is_none_or(|floor| {
                RunFloor {
                    run_id: id,
                    attempt: attempts.get(&id).copied().unwrap_or(1),
                } > floor
            })
        })
        .collect();
    if let Some(&green) = judged
        .iter()
        .find(|&&index| builds[index].result() == "succeeded")
    {
        return Decision::Green { build: green };
    }
    let Some(&newest) = judged.first() else {
        return Decision::Pending { build: None };
    };
    match builds[newest].result() {
        // Partially succeeded: a task failed that was allowed to; not a clean green.
        "failed" | "partiallySucceeded" => Decision::Red { build: newest },
        "canceled" => Decision::Cancelled { build: newest },
        // Not started, postponed, in progress, cancelling, or a value this source does
        // not know: not the green the gate needs, and not a finished failure.
        _ => Decision::Pending {
            build: Some(newest),
        },
    }
}

#[derive(Default)]
struct Cached {
    reported: Option<(Vec<Build>, Option<Timeline>)>,
    /// The newest judged build, for cancel: (id, completed).
    current: Option<(u64, bool)>,
}

/// Observes Azure Pipelines CI gates through a [`GithubApi`] REST port rooted at
/// `https://dev.azure.com`.
pub struct AzurePipelinesSource<A: GithubApi> {
    api: A,
    jobs: BTreeMap<String, Cached>,
}

/// `{organization}/{project}` and the definition id of a target.
fn project_of(target: &CiTarget) -> Option<(String, &str)> {
    let mut parts = target.repo.split('/');
    let (organization, project, definition) = (parts.next()?, parts.next()?, parts.next()?);
    Some((
        format!("{}/{}", query_value(organization), query_value(project)),
        definition,
    ))
}

impl<A: GithubApi> AzurePipelinesSource<A> {
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

    /// A build's timeline. `None` when the build has none yet or it cannot be read;
    /// a spent limit or an unreachable service is retried.
    fn timeline(&mut self, project: &str, id: u64) -> Result<Option<Timeline>, WatchError> {
        let response = self.get(format!(
            "{project}/_apis/build/builds/{id}/timeline?api-version={API_VERSION}"
        ))?;
        match response.status {
            200 => Ok(serde_json::from_str(&response.body).ok()),
            429 | 500.. => Err(transient(&response)),
            _ => Ok(None),
        }
    }
}

impl<A: GithubApi> WatchSource for AzurePipelinesSource<A> {
    fn kind(&self) -> &str {
        AZURE_PIPELINES_KIND
    }

    fn poll(&mut self, job: &Job, _now_ms: u64) -> Result<Poll, WatchError> {
        let (target, floor) = job
            .external
            .as_ref()
            .and_then(CiTarget::parse)
            .filter(|(target, _)| target.provider == CiProvider::AzurePipelines)
            .ok_or(WatchError::Gone)?;
        let (project, definition) = project_of(&target).ok_or(WatchError::Gone)?;
        let branch = format!("refs/heads/{}", target.branch);
        let response = self.get(format!(
            "{project}/_apis/build/builds?definitions={definition}&branchName={}\
             &queryOrder=queueTimeDescending&%24top={BUILDS_PER_POLL}&api-version={API_VERSION}",
            query_value(&branch)
        ))?;
        let builds: Vec<Build> = match response.status {
            200 => {
                let listed: BuildList =
                    serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                let mut builds: Vec<Build> = listed
                    .value
                    .into_iter()
                    .filter(|build| {
                        build.reason.as_deref() != Some("pullRequest")
                            && build.source_branch.as_deref() == Some(branch.as_str())
                            && build
                                .source_version
                                .as_deref()
                                .is_some_and(|commit| commit.eq_ignore_ascii_case(&target.sha))
                    })
                    .collect();
                builds.sort_by_key(|build| std::cmp::Reverse(build.id));
                builds
            }
            // The organization, project or definition does not exist, or cannot be
            // read with this token.
            404 => return Err(WatchError::Gone),
            // 203 is Azure DevOps' sign-in page for a credential it does not accept;
            // it, 401, 403, 429 and server errors are retried.
            _ => return Err(transient(&response)),
        };
        // A rewatch floor names a build that may have been retried: its attempt decides.
        let mut attempts = BTreeMap::new();
        let mut timelines = BTreeMap::new();
        if let Some(floor) = floor
            && builds.iter().any(|build| build.id == floor.run_id)
        {
            let timeline = self.timeline(&project, floor.run_id)?;
            attempts.insert(floor.run_id, timeline.as_ref().map_or(1, Timeline::attempt));
            timelines.insert(floor.run_id, timeline);
        }
        let decision = decide(floor, &builds, &attempts);
        let current = match decision {
            Decision::Pending { build } => build,
            Decision::Green { build } | Decision::Red { build } | Decision::Cancelled { build } => {
                Some(build)
            }
        }
        .map(|index| builds[index].clone());
        let timeline = match &current {
            Some(build) => match timelines.remove(&build.id) {
                Some(timeline) => timeline,
                None => self.timeline(&project, build.id)?,
            },
            None => None,
        };
        let attempt = timeline.as_ref().map_or(1, Timeline::attempt);
        let cached = self.jobs.entry(job.id.clone()).or_default();
        cached.current = current.as_ref().map(|build| (build.id, build.completed()));
        let seen = (builds, timeline);
        if cached.reported.as_ref() == Some(&seen) {
            return Ok(Poll::NotModified);
        }
        let timeline = seen.1.clone();
        cached.reported = Some(seen);
        let state = match (decision, &current) {
            (Decision::Green { .. }, Some(build)) => {
                Observed::Succeeded(outcome(&target, "green", build, attempt))
            }
            (Decision::Red { .. }, Some(build)) => {
                Observed::Failed(outcome(&target, "red", build, attempt))
            }
            (Decision::Cancelled { .. }, Some(build)) => {
                Observed::Cancelled(outcome(&target, "cancelled", build, attempt))
            }
            (_, Some(build)) if build.running() => Observed::Running,
            _ => Observed::Waiting,
        };
        let mut latest = BTreeMap::new();
        if let Some(build) = &current {
            latest.insert("run_id".to_owned(), build.id.to_string());
            latest.insert("run_attempt".to_owned(), attempt.to_string());
            latest.insert("run_head_sha".to_owned(), target.sha.clone());
            latest.insert("run_status".to_owned(), clean(build.status(), 64));
            if let Some(step) = timeline.as_ref().and_then(|timeline| {
                shown(timeline)
                    .into_iter()
                    .find(|record| record.state.as_deref() == Some("inProgress"))
                    .and_then(|record| record.name.clone())
            }) {
                latest.insert("current_step".to_owned(), clean(&step, 200));
            }
        }
        Ok(Poll::Observed(Box::new(Observation {
            steps: timeline.as_ref().map(steps_of),
            latest,
            url: current
                .as_ref()
                .and_then(|build| safe_url(build.web_url(), 2000)),
            ..Observation::new(state)
        })))
    }

    fn cancel(&mut self, job: &Job) -> CancelReply {
        let Some((target, _)) = job.external.as_ref().and_then(CiTarget::parse) else {
            return CancelReply::Unsupported;
        };
        let Some((project, _)) = project_of(&target) else {
            return CancelReply::Unsupported;
        };
        // Every build judged here built the commit itself; a completed one is left.
        let Some((id, completed)) = self.jobs.get(&job.id).and_then(|cached| cached.current) else {
            return CancelReply::Unsupported;
        };
        if completed {
            return CancelReply::Unsupported;
        }
        let request = ApiRequest {
            method: ApiMethod::Patch,
            path: format!("{project}/_apis/build/builds/{id}?api-version={API_VERSION}"),
            if_none_match: None,
            body: Some(serde_json::json!({"status": "cancelling"})),
        };
        match self.api.send(&request) {
            Ok(response) if response.status == 200 => CancelReply::Requested,
            Ok(response) if matches!(response.status, 203 | 401 | 403 | 404) => {
                CancelReply::Failed("Azure Pipelines did not permit cancelling this build".into())
            }
            Ok(response) => CancelReply::Failed(format!(
                "Azure Pipelines answered {} to the cancel",
                response.status
            )),
            Err(_) => CancelReply::Failed(
                "Azure Pipelines could not be reached to cancel the build".into(),
            ),
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

/// The outcome record of a finished Azure Pipelines watch, with the same refs as
/// GitHub's.
fn outcome(target: &CiTarget, gate: &str, build: &Build, attempt: u32) -> Outcome {
    let mut refs = BTreeMap::from([
        ("gate".to_owned(), gate.to_owned()),
        ("commit".to_owned(), target.sha.clone()),
        ("repo".to_owned(), target.repo.clone()),
        ("branch".to_owned(), target.branch.clone()),
        ("workflow".to_owned(), clean(&target.workflow, 200)),
        ("run_id".to_owned(), build.id.to_string()),
        ("run_attempt".to_owned(), attempt.to_string()),
        // Only a build of exactly the commit is ever judged.
        ("run_head_sha".to_owned(), target.sha.clone()),
    ]);
    if build.completed()
        && let Some(result) = &build.result
    {
        refs.insert("run_conclusion".to_owned(), clean(result, 64));
    }
    if let Some(url) = safe_url(build.web_url(), 500) {
        refs.insert("run_url".to_owned(), url);
    }
    let verdict = match gate {
        "green" => "succeeded",
        "red" => "failed",
        _ => "was cancelled",
    };
    Outcome {
        summary: Some(format!(
            "Azure Pipelines build {} {verdict} for {}",
            build.label(),
            &target.sha[..7]
        )),
        refs,
    }
}

/// The records shown as steps: the stages, or the jobs of a pipeline without stages,
/// in their order.
fn shown(timeline: &Timeline) -> Vec<&Record> {
    let has_stages = timeline
        .records
        .iter()
        .any(|record| record.kind.as_deref() == Some("Stage"));
    let kind = if has_stages { "Stage" } else { "Job" };
    let mut records: Vec<&Record> = timeline
        .records
        .iter()
        .filter(|record| record.kind.as_deref() == Some(kind))
        .collect();
    records.sort_by_key(|record| record.order.unwrap_or(i64::MAX));
    records
}

fn steps_of(timeline: &Timeline) -> Vec<Step> {
    shown(timeline)
        .into_iter()
        .take(crate::steps::MAX_STEPS)
        .enumerate()
        .map(|(index, record)| {
            let status = match (record.state.as_deref(), record.result.as_deref()) {
                (Some("completed"), Some("succeeded" | "succeededWithIssues")) => {
                    StepStatus::Succeeded
                }
                (Some("completed"), Some("failed")) => StepStatus::Failed,
                (Some("completed"), Some("canceled" | "abandoned")) => StepStatus::Cancelled,
                (Some("completed"), Some("skipped")) => StepStatus::Skipped,
                (Some("inProgress"), _) => StepStatus::Active,
                _ => StepStatus::Pending,
            };
            // A step id is a token; Azure record ids are GUIDs.
            let id = record
                .id
                .as_deref()
                .map_or_else(|| format!("record-{index}"), |id| format!("record-{id}"))
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
                label: clean(record.name.as_deref().unwrap_or("stage"), 200),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn build(id: u64, status: &str, result: Option<&str>) -> Build {
        Build {
            id,
            build_number: None,
            status: Some(status.to_owned()),
            result: result.map(str::to_owned),
            source_branch: None,
            source_version: None,
            reason: None,
            links: None,
        }
    }

    #[test]
    fn hs2_jz1dd2_every_documented_status_and_result_maps_to_one_gate_decision() {
        let none = BTreeMap::new();
        for (status, result, expected) in [
            ("completed", Some("succeeded"), Decision::Green { build: 0 }),
            (
                "completed",
                Some("partiallySucceeded"),
                Decision::Red { build: 0 },
            ),
            ("completed", Some("failed"), Decision::Red { build: 0 }),
            (
                "completed",
                Some("canceled"),
                Decision::Cancelled { build: 0 },
            ),
            (
                "completed",
                Some("none"),
                Decision::Pending { build: Some(0) },
            ),
            ("completed", None, Decision::Pending { build: Some(0) }),
            (
                "completed",
                Some("somethingNew"),
                Decision::Pending { build: Some(0) },
            ),
            ("notStarted", None, Decision::Pending { build: Some(0) }),
            ("postponed", None, Decision::Pending { build: Some(0) }),
            ("none", None, Decision::Pending { build: Some(0) }),
            ("inProgress", None, Decision::Pending { build: Some(0) }),
            // A retried build keeps its earlier result while it runs again.
            (
                "inProgress",
                Some("failed"),
                Decision::Pending { build: Some(0) },
            ),
            ("cancelling", None, Decision::Pending { build: Some(0) }),
            (
                "somethingNew",
                Some("succeeded"),
                Decision::Pending { build: Some(0) },
            ),
        ] {
            assert_eq!(
                decide(None, &[build(7, status, result)], &none),
                expected,
                "{status} {result:?}"
            );
        }
        assert!(build(1, "inProgress", None).running());
        assert!(build(1, "cancelling", None).running());
        assert!(!build(1, "notStarted", None).running());
        assert_eq!(decide(None, &[], &none), Decision::Pending { build: None });
    }

    #[test]
    fn hs2_jz1dd2_a_floor_hides_a_judged_build_until_it_is_retried() {
        let floor = Some(RunFloor {
            run_id: 9,
            attempt: 1,
        });
        let builds = [build(9, "completed", Some("succeeded"))];
        assert_eq!(
            decide(floor, &builds, &BTreeMap::new()),
            Decision::Pending { build: None }
        );
        assert_eq!(
            decide(floor, &builds, &BTreeMap::from([(9, 1)])),
            Decision::Pending { build: None }
        );
        assert_eq!(
            decide(floor, &builds, &BTreeMap::from([(9, 2)])),
            Decision::Green { build: 0 }
        );
        // A newer build is judged whatever its attempt.
        let builds = [build(10, "completed", Some("failed"))];
        assert_eq!(
            decide(floor, &builds, &BTreeMap::new()),
            Decision::Red { build: 0 }
        );
    }

    #[test]
    fn hs2_jz1dd2_timeline_records_become_steps_and_the_attempt() {
        let record =
            |id: &str, kind: &str, order: i64, state: &str, result: Option<&str>, attempt| Record {
                id: Some(id.to_owned()),
                kind: Some(kind.to_owned()),
                name: Some(format!("{kind} {id}")),
                order: Some(order),
                state: Some(state.to_owned()),
                result: result.map(str::to_owned),
                attempt: Some(attempt),
            };
        let timeline = Timeline {
            records: vec![
                record("b", "Stage", 2, "inProgress", None, 2),
                record("a", "Stage", 1, "completed", Some("succeededWithIssues"), 1),
                record("c", "Stage", 3, "completed", Some("skipped"), 1),
                record("d", "Stage", 4, "completed", Some("abandoned"), 1),
                record("e", "Stage", 5, "completed", Some("failed"), 1),
                record("f", "Stage", 6, "pending", None, 1),
                record("j", "Job", 1, "completed", Some("failed"), 3),
            ],
        };
        assert_eq!(timeline.attempt(), 3);
        let steps: Vec<(String, StepStatus)> = steps_of(&timeline)
            .into_iter()
            .map(|step| (step.id, step.status))
            .collect();
        assert_eq!(
            steps,
            [
                ("record-a".to_owned(), StepStatus::Succeeded),
                ("record-b".to_owned(), StepStatus::Active),
                ("record-c".to_owned(), StepStatus::Skipped),
                ("record-d".to_owned(), StepStatus::Cancelled),
                ("record-e".to_owned(), StepStatus::Failed),
                ("record-f".to_owned(), StepStatus::Pending),
            ]
        );
        // Without stages, the jobs are the steps; an empty timeline is attempt 1.
        let jobs_only = Timeline {
            records: vec![record("j", "Job", 1, "inProgress", None, 1)],
        };
        assert_eq!(steps_of(&jobs_only)[0].status, StepStatus::Active);
        assert_eq!(Timeline::default().attempt(), 1);
    }
}
