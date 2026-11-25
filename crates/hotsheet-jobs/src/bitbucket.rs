//! HS2-BWF72S: the Bitbucket Pipelines watch source. One watch Job follows the CI gate
//! of one commit: the gate is green when a successful pipeline of the repository on the
//! shipping branch built exactly that commit.
//!
//! Bitbucket Cloud's REST API 2.0
//! (<https://developer.atlassian.com/cloud/bitbucket/rest/api-group-pipelines/>) lists a
//! repository's pipelines (`GET /repositories/{workspace}/{repo_slug}/pipelines/`,
//! filtered by `target.ref_name` and sorted by `-created_on`; `values` with `uuid`,
//! `build_number`, `run_number`, `state.name` and `state.result.name`, and `target`
//! with `ref_type`, `ref_name`, `commit.hash` and `selector`), a pipeline's steps
//! (`GET …/pipelines/{uuid}/steps/`) and stops one (`POST …/pipelines/{uuid}/stopPipeline`,
//! 204). Each poll reads the branch's newest pipelines and keeps the commit's own. It
//! exposes no repository ancestry, so a Bitbucket gate attests exact coverage only (see
//! [`CiProvider::attests_ancestors`]).
//!
//! HS2-EFCEJG: on a busy branch the commit's pipeline can fall out of that first page.
//! When the page holds none of the commit's pipelines and the listing has more, the
//! poll asks the list with the documented `target.commit.hash` filter (Bitbucket's
//! OpenAPI: "The revision to filter by."). An answer holding only the commit's
//! pipelines shows the filter is honoured; one holding other commits, or a refusal,
//! shows it is not, and the poll pages the branch's listing instead (`pagelen=100`, the
//! documented maximum, at most [`MAX_DEEP_PAGES`] pages per poll, de-duplicated by
//! uuid because pages shift as pipelines arrive). Pages are asked by number, never by
//! following an absolute `next` link. A spent budget leaves the gate pending.
//!
//! A rerun of the whole pipeline is a new pipeline with a new build number; rerunning
//! its failed steps keeps the build number and raises `run_number`. The run floor is
//! the build number with the run number. All access goes through the shared REST port
//! ([`GithubApi`]) rooted at `https://api.bitbucket.org/2.0`, so tests run offline with
//! fixtures.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::github::{
    ApiMethod, ApiRequest, ApiResponse, BITBUCKET_PIPELINES_KIND, CiProvider, CiTarget, GithubApi,
    RunFloor, clean, query_value, transient,
};
use crate::model::{Job, Outcome};
use crate::monitor::{CancelReply, Observation, Observed, Poll, WatchError, WatchSource};
use crate::steps::{Step, StepStatus};

/// How many of the branch's newest pipelines one poll looks at first.
const PIPELINES_PER_POLL: usize = 30;
/// HS2-EFCEJG: the page length of a deeper search, Bitbucket's documented maximum.
const DEEP_PAGELEN: usize = 100;
/// HS2-EFCEJG: how many pages of the branch's listing one poll may read beyond the
/// first when the commit's pipeline is not on it.
const MAX_DEEP_PAGES: u32 = 3;

#[derive(Clone, Debug, Deserialize)]
struct Page<T> {
    #[serde(default = "Vec::new")]
    values: Vec<T>,
    /// The total count, when Bitbucket answers it.
    #[serde(default)]
    size: Option<u64>,
    /// A link to the next page, present while there is one.
    #[serde(default)]
    next: Option<String>,
}

impl<T> Page<T> {
    /// Whether the listing has entries beyond page `page` of `pagelen`.
    fn more(&self, page: u32, pagelen: usize) -> bool {
        self.next.is_some()
            || self
                .size
                .is_some_and(|size| size > u64::from(page) * pagelen as u64)
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct State {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    result: Option<Named>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Named {
    #[serde(default)]
    name: Option<String>,
}

impl State {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }

    /// The result, read only once completed.
    fn result(&self) -> &str {
        if self.name() == "COMPLETED" {
            self.result
                .as_ref()
                .and_then(|result| result.name.as_deref())
                .unwrap_or("")
        } else {
            ""
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Pipeline {
    uuid: String,
    build_number: u64,
    /// Raised by rerunning failed steps; absent before that feature, read as 1.
    #[serde(default)]
    run_number: Option<u32>,
    #[serde(default)]
    state: State,
    #[serde(default)]
    target: Option<Target>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Target {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    ref_type: Option<String>,
    #[serde(default)]
    ref_name: Option<String>,
    #[serde(default)]
    commit: Option<Commit>,
    #[serde(default)]
    selector: Option<Selector>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Commit {
    #[serde(default)]
    hash: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Selector {
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

impl Pipeline {
    /// The commit the pipeline built, whatever its target.
    fn commit(&self) -> Option<&str> {
        self.target
            .as_ref()
            .and_then(|target| target.commit.as_ref())
            .and_then(|commit| commit.hash.as_deref())
    }

    fn attempt(&self) -> u32 {
        self.run_number.unwrap_or(1).max(1)
    }

    fn completed(&self) -> bool {
        self.state.name() == "COMPLETED"
    }

    fn running(&self) -> bool {
        self.state.name() == "IN_PROGRESS"
    }

    /// A branch pipeline of exactly the commit. A pull request pipeline has another
    /// target type, and a custom pipeline is run by hand, not by the push.
    fn built(&self, target: &CiTarget) -> bool {
        let Some(built) = &self.target else {
            return false;
        };
        built
            .kind
            .as_deref()
            .is_none_or(|kind| kind == "pipeline_ref_target")
            && built
                .ref_type
                .as_deref()
                .is_none_or(|kind| kind == "branch")
            && built.ref_name.as_deref() == Some(target.branch.as_str())
            && built
                .selector
                .as_ref()
                .and_then(|selector| selector.kind.as_deref())
                .is_none_or(|kind| !matches!(kind, "custom" | "pull-requests"))
            && built
                .commit
                .as_ref()
                .and_then(|commit| commit.hash.as_deref())
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&target.sha))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct PipelineStep {
    uuid: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    state: State,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decision {
    Pending { pipeline: Option<usize> },
    Green { pipeline: usize },
    Red { pipeline: usize },
    Cancelled { pipeline: usize },
}

/// Judge the gate from the commit's branch pipelines, newest first.
fn decide(floor: Option<RunFloor>, pipelines: &[Pipeline]) -> Decision {
    let judged: Vec<usize> = (0..pipelines.len())
        .filter(|index| {
            floor.is_none_or(|floor| {
                RunFloor {
                    run_id: pipelines[*index].build_number,
                    attempt: pipelines[*index].attempt(),
                } > floor
            })
        })
        .collect();
    if let Some(&green) = judged
        .iter()
        .find(|&&index| pipelines[index].state.result() == "SUCCESSFUL")
    {
        return Decision::Green { pipeline: green };
    }
    let Some(&newest) = judged.first() else {
        return Decision::Pending { pipeline: None };
    };
    match pipelines[newest].state.result() {
        // Expired: a paused pipeline was not resumed in time; it finished unsuccessful.
        "FAILED" | "ERROR" | "EXPIRED" => Decision::Red { pipeline: newest },
        "STOPPED" => Decision::Cancelled { pipeline: newest },
        // Pending, in progress (running, paused or halted on a manual step), or a value
        // this source does not know: not green, and not a finished failure.
        _ => Decision::Pending {
            pipeline: Some(newest),
        },
    }
}

#[derive(Default)]
struct Cached {
    reported: Option<(Vec<Pipeline>, Option<Vec<PipelineStep>>)>,
    /// The newest judged pipeline, for stop: (uuid, completed).
    current: Option<(String, bool)>,
}

/// Observes Bitbucket Pipelines CI gates through a [`GithubApi`] REST port rooted at
/// `https://api.bitbucket.org/2.0`.
pub struct BitbucketPipelinesSource<A: GithubApi> {
    api: A,
    jobs: BTreeMap<String, Cached>,
    /// HS2-EFCEJG: whether the list honours `target.commit.hash`, once learned.
    commit_filter: Option<bool>,
}

/// `repositories/{workspace}/{repo_slug}` of a target.
fn repository_path(target: &CiTarget) -> Option<String> {
    let (workspace, slug) = target.repo.split_once('/')?;
    Some(format!(
        "repositories/{}/{}",
        query_value(workspace),
        query_value(slug)
    ))
}

impl<A: GithubApi> BitbucketPipelinesSource<A> {
    pub fn new(api: A) -> Self {
        Self {
            api,
            jobs: BTreeMap::new(),
            commit_filter: None,
        }
    }

    /// One page of the branch's pipelines, newest first; `extra` adds query
    /// parameters. A missing repository ends the watch; 401, 403, 429 and server
    /// errors are retried.
    fn list(
        &mut self,
        repository: &str,
        target: &CiTarget,
        extra: &str,
        pagelen: usize,
    ) -> Result<(ApiResponse, Option<Page<Pipeline>>), WatchError> {
        let response = self.get(format!(
            "{repository}/pipelines/?target.ref_name={}{extra}&sort=-created_on&pagelen={pagelen}",
            query_value(&target.branch)
        ))?;
        let page = match response.status {
            200 => Some(serde_json::from_str(&response.body).map_err(|_| transient(&response))?),
            _ => None,
        };
        Ok((response, page))
    }

    /// HS2-EFCEJG: the commit's pipelines beyond the first page, as listed (unfiltered
    /// by target kind). Empty when they are not found within this poll's budget.
    fn deeper(&mut self, repository: &str, target: &CiTarget) -> Result<Vec<Pipeline>, WatchError> {
        if self.commit_filter != Some(false) {
            let extra = format!("&target.commit.hash={}", target.sha);
            let (response, page) = self.list(repository, target, &extra, PIPELINES_PER_POLL)?;
            match page {
                Some(page) => {
                    let honoured = page.values.iter().all(|pipeline| {
                        pipeline
                            .commit()
                            .is_some_and(|hash| hash.eq_ignore_ascii_case(&target.sha))
                    });
                    self.commit_filter = Some(honoured);
                    if honoured {
                        return Ok(page.values);
                    }
                }
                None => match response.status {
                    429 | 500.. => return Err(transient(&response)),
                    404 => return Err(WatchError::Gone),
                    // A refused parameter: page the listing instead.
                    _ => self.commit_filter = Some(false),
                },
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut found = Vec::new();
        for number in 1..=MAX_DEEP_PAGES {
            let (response, page) =
                self.list(repository, target, &format!("&page={number}"), DEEP_PAGELEN)?;
            let Some(page) = page else {
                return Err(match response.status {
                    404 => WatchError::Gone,
                    _ => transient(&response),
                });
            };
            let more = page.more(number, DEEP_PAGELEN);
            let empty = page.values.is_empty();
            for pipeline in page.values {
                if seen.insert(pipeline.uuid.clone())
                    && pipeline
                        .commit()
                        .is_some_and(|hash| hash.eq_ignore_ascii_case(&target.sha))
                {
                    found.push(pipeline);
                }
            }
            if empty || !more || !found.is_empty() {
                break;
            }
        }
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

    /// A pipeline's steps. `None` when they cannot be read; a spent limit or an
    /// unreachable service is retried.
    fn steps(
        &mut self,
        repository: &str,
        uuid: &str,
    ) -> Result<Option<Vec<PipelineStep>>, WatchError> {
        let response = self.get(format!(
            "{repository}/pipelines/{}/steps/?pagelen=100",
            query_value(uuid)
        ))?;
        match response.status {
            200 => Ok(serde_json::from_str::<Page<PipelineStep>>(&response.body)
                .ok()
                .map(|page| page.values)),
            429 | 500.. => Err(transient(&response)),
            _ => Ok(None),
        }
    }
}

impl<A: GithubApi> WatchSource for BitbucketPipelinesSource<A> {
    fn kind(&self) -> &str {
        BITBUCKET_PIPELINES_KIND
    }

    fn poll(&mut self, job: &Job, _now_ms: u64) -> Result<Poll, WatchError> {
        let (target, floor) = job
            .external
            .as_ref()
            .and_then(CiTarget::parse)
            .filter(|(target, _)| target.provider == CiProvider::BitbucketPipelines)
            .ok_or(WatchError::Gone)?;
        let repository = repository_path(&target).ok_or(WatchError::Gone)?;
        let (response, listed) = self.list(&repository, &target, "", PIPELINES_PER_POLL)?;
        let Some(listed) = listed else {
            return Err(match response.status {
                // The repository does not exist, or cannot be read with these credentials.
                404 => WatchError::Gone,
                // 401, 403, 429 and server errors are retried.
                _ => transient(&response),
            });
        };
        let more = listed.more(1, PIPELINES_PER_POLL);
        let mut pipelines: Vec<Pipeline> = listed
            .values
            .into_iter()
            .filter(|pipeline| pipeline.built(&target))
            .collect();
        if pipelines.is_empty() && more {
            pipelines = self
                .deeper(&repository, &target)?
                .into_iter()
                .filter(|pipeline| pipeline.built(&target))
                .collect();
        }
        pipelines.sort_by_key(|pipeline| std::cmp::Reverse(pipeline.build_number));
        let decision = decide(floor, &pipelines);
        let current = match decision {
            Decision::Pending { pipeline } => pipeline,
            Decision::Green { pipeline }
            | Decision::Red { pipeline }
            | Decision::Cancelled { pipeline } => Some(pipeline),
        }
        .map(|index| pipelines[index].clone());
        let steps = match &current {
            Some(pipeline) => self.steps(&repository, &pipeline.uuid)?,
            None => None,
        };
        let cached = self.jobs.entry(job.id.clone()).or_default();
        cached.current = current
            .as_ref()
            .map(|pipeline| (pipeline.uuid.clone(), pipeline.completed()));
        let seen = (pipelines, steps);
        if cached.reported.as_ref() == Some(&seen) {
            return Ok(Poll::NotModified);
        }
        let steps = seen.1.clone();
        cached.reported = Some(seen);
        let state = match (decision, &current) {
            (Decision::Green { .. }, Some(pipeline)) => {
                Observed::Succeeded(outcome(&target, "green", pipeline))
            }
            (Decision::Red { .. }, Some(pipeline)) => {
                Observed::Failed(outcome(&target, "red", pipeline))
            }
            (Decision::Cancelled { .. }, Some(pipeline)) => {
                Observed::Cancelled(outcome(&target, "cancelled", pipeline))
            }
            (_, Some(pipeline)) if pipeline.running() => Observed::Running,
            _ => Observed::Waiting,
        };
        let mut latest = BTreeMap::new();
        if let Some(pipeline) = &current {
            latest.insert("run_id".to_owned(), pipeline.build_number.to_string());
            latest.insert("run_attempt".to_owned(), pipeline.attempt().to_string());
            latest.insert("run_head_sha".to_owned(), target.sha.clone());
            latest.insert("run_status".to_owned(), clean(pipeline.state.name(), 64));
            if let Some(step) = steps.as_ref().and_then(|steps| {
                steps
                    .iter()
                    .find(|step| step.state.name() == "IN_PROGRESS")
                    .and_then(|step| step.name.clone())
            }) {
                latest.insert("current_step".to_owned(), clean(&step, 200));
            }
        }
        Ok(Poll::Observed(Box::new(Observation {
            steps: steps.as_deref().map(steps_of),
            latest,
            url: current.as_ref().map(|pipeline| web_url(&target, pipeline)),
            ..Observation::new(state)
        })))
    }

    fn cancel(&mut self, job: &Job) -> CancelReply {
        let Some((target, _)) = job.external.as_ref().and_then(CiTarget::parse) else {
            return CancelReply::Unsupported;
        };
        let Some(repository) = repository_path(&target) else {
            return CancelReply::Unsupported;
        };
        // Every pipeline judged here built the commit itself; a completed one is left.
        let Some((uuid, completed)) = self
            .jobs
            .get(&job.id)
            .and_then(|cached| cached.current.clone())
        else {
            return CancelReply::Unsupported;
        };
        if completed {
            return CancelReply::Unsupported;
        }
        let request = ApiRequest {
            method: ApiMethod::Post,
            path: format!("{repository}/pipelines/{}/stopPipeline", query_value(&uuid)),
            if_none_match: None,
            body: None,
        };
        match self.api.send(&request) {
            Ok(response) if (200..300).contains(&response.status) => CancelReply::Requested,
            Ok(response) if matches!(response.status, 400 | 401 | 403 | 404) => {
                CancelReply::Failed("Bitbucket did not permit stopping this pipeline".into())
            }
            Ok(response) => CancelReply::Failed(format!(
                "Bitbucket answered {} to the stop",
                response.status
            )),
            Err(_) => {
                CancelReply::Failed("Bitbucket could not be reached to stop the pipeline".into())
            }
        }
    }

    fn forget(&mut self, job_id: &str) {
        self.jobs.remove(job_id);
    }
}

/// The pipeline's page on Bitbucket Cloud. The API answers no web link for a pipeline;
/// the workspace and slug are validated tokens, so the URL is well formed.
fn web_url(target: &CiTarget, pipeline: &Pipeline) -> String {
    format!(
        "https://bitbucket.org/{}/pipelines/results/{}",
        target.repo, pipeline.build_number
    )
}

/// The outcome record of a finished Bitbucket Pipelines watch, with the same refs as
/// GitHub's.
fn outcome(target: &CiTarget, gate: &str, pipeline: &Pipeline) -> Outcome {
    let mut refs = BTreeMap::from([
        ("gate".to_owned(), gate.to_owned()),
        ("commit".to_owned(), target.sha.clone()),
        ("repo".to_owned(), target.repo.clone()),
        ("branch".to_owned(), target.branch.clone()),
        ("workflow".to_owned(), clean(&target.workflow, 200)),
        ("run_id".to_owned(), pipeline.build_number.to_string()),
        ("run_attempt".to_owned(), pipeline.attempt().to_string()),
        // Only a pipeline of exactly the commit is ever judged.
        ("run_head_sha".to_owned(), target.sha.clone()),
        ("run_url".to_owned(), web_url(target, pipeline)),
    ]);
    if pipeline.completed() {
        refs.insert(
            "run_conclusion".to_owned(),
            clean(pipeline.state.result(), 64),
        );
    }
    let verdict = match gate {
        "green" => "succeeded",
        "red" => "failed",
        _ => "was stopped",
    };
    Outcome {
        summary: Some(format!(
            "Bitbucket pipeline #{} {verdict} for {}",
            pipeline.build_number,
            &target.sha[..7]
        )),
        refs,
    }
}

fn steps_of(steps: &[PipelineStep]) -> Vec<Step> {
    steps
        .iter()
        .take(crate::steps::MAX_STEPS)
        .map(|step| {
            let status = match (step.state.name(), step.state.result()) {
                (_, "SUCCESSFUL") => StepStatus::Succeeded,
                (_, "FAILED" | "ERROR" | "EXPIRED") => StepStatus::Failed,
                (_, "STOPPED") => StepStatus::Cancelled,
                (_, "NOT_RUN") => StepStatus::Skipped,
                ("IN_PROGRESS", _) => StepStatus::Active,
                _ => StepStatus::Pending,
            };
            Step {
                // A step id is a token; Bitbucket step uuids are braced GUIDs.
                id: format!("step-{}", step.uuid)
                    .chars()
                    .filter(|c| !"{}".contains(*c))
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || "-_.:".contains(c) {
                            c
                        } else {
                            '-'
                        }
                    })
                    .take(120)
                    .collect(),
                label: clean(step.name.as_deref().unwrap_or("step"), 200),
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

    fn pipeline(number: u64, run: Option<u32>, state: &str, result: Option<&str>) -> Pipeline {
        Pipeline {
            uuid: format!("{{{number}}}"),
            build_number: number,
            run_number: run,
            state: State {
                name: Some(state.to_owned()),
                result: result.map(|name| Named {
                    name: Some(name.to_owned()),
                }),
            },
            target: None,
        }
    }

    #[test]
    fn hs2_bwf72s_every_documented_state_and_result_maps_to_one_gate_decision() {
        for (state, result, expected) in [
            (
                "COMPLETED",
                Some("SUCCESSFUL"),
                Decision::Green { pipeline: 0 },
            ),
            ("COMPLETED", Some("FAILED"), Decision::Red { pipeline: 0 }),
            ("COMPLETED", Some("ERROR"), Decision::Red { pipeline: 0 }),
            ("COMPLETED", Some("EXPIRED"), Decision::Red { pipeline: 0 }),
            (
                "COMPLETED",
                Some("STOPPED"),
                Decision::Cancelled { pipeline: 0 },
            ),
            (
                "COMPLETED",
                Some("SOMETHING_NEW"),
                Decision::Pending { pipeline: Some(0) },
            ),
            ("COMPLETED", None, Decision::Pending { pipeline: Some(0) }),
            ("PENDING", None, Decision::Pending { pipeline: Some(0) }),
            ("IN_PROGRESS", None, Decision::Pending { pipeline: Some(0) }),
            // A result is read only once completed.
            (
                "IN_PROGRESS",
                Some("SUCCESSFUL"),
                Decision::Pending { pipeline: Some(0) },
            ),
            ("PARSING", None, Decision::Pending { pipeline: Some(0) }),
        ] {
            assert_eq!(
                decide(None, &[pipeline(7, None, state, result)]),
                expected,
                "{state} {result:?}"
            );
        }
        assert!(pipeline(1, None, "IN_PROGRESS", None).running());
        assert!(!pipeline(1, None, "PENDING", None).running());
        assert_eq!(decide(None, &[]), Decision::Pending { pipeline: None });
    }

    #[test]
    fn hs2_bwf72s_a_floor_hides_a_judged_run_until_its_failed_steps_are_rerun() {
        let floor = Some(RunFloor {
            run_id: 9,
            attempt: 1,
        });
        assert_eq!(
            decide(floor, &[pipeline(9, None, "COMPLETED", Some("SUCCESSFUL"))]),
            Decision::Pending { pipeline: None }
        );
        assert_eq!(
            decide(
                floor,
                &[pipeline(9, Some(2), "COMPLETED", Some("SUCCESSFUL"))]
            ),
            Decision::Green { pipeline: 0 }
        );
        assert_eq!(
            decide(floor, &[pipeline(10, None, "COMPLETED", Some("FAILED"))]),
            Decision::Red { pipeline: 0 }
        );
        assert_eq!(pipeline(1, Some(0), "PENDING", None).attempt(), 1);
    }

    #[test]
    fn hs2_bwf72s_steps_map_every_documented_state_and_result() {
        let step = |uuid: &str, state: &str, result: Option<&str>| PipelineStep {
            uuid: uuid.to_owned(),
            name: Some(format!("step {uuid}")),
            state: State {
                name: Some(state.to_owned()),
                result: result.map(|name| Named {
                    name: Some(name.to_owned()),
                }),
            },
        };
        let steps: Vec<(String, StepStatus)> = steps_of(&[
            step("{a}", "COMPLETED", Some("SUCCESSFUL")),
            step("{b}", "COMPLETED", Some("FAILED")),
            step("{c}", "COMPLETED", Some("ERROR")),
            step("{d}", "COMPLETED", Some("STOPPED")),
            step("{e}", "COMPLETED", Some("NOT_RUN")),
            step("{f}", "IN_PROGRESS", None),
            step("{g}", "PENDING", None),
            step("{h}", "READY", None),
        ])
        .into_iter()
        .map(|step| (step.id, step.status))
        .collect();
        assert_eq!(
            steps,
            [
                ("step-a".to_owned(), StepStatus::Succeeded),
                ("step-b".to_owned(), StepStatus::Failed),
                ("step-c".to_owned(), StepStatus::Failed),
                ("step-d".to_owned(), StepStatus::Cancelled),
                ("step-e".to_owned(), StepStatus::Skipped),
                ("step-f".to_owned(), StepStatus::Active),
                ("step-g".to_owned(), StepStatus::Pending),
                ("step-h".to_owned(), StepStatus::Pending),
            ]
        );
    }

    #[test]
    fn hs2_efcejg_a_page_has_more_by_its_next_link_or_its_size() {
        let page = |next: Option<&str>, size: Option<u64>| Page::<Pipeline> {
            values: Vec::new(),
            size,
            next: next.map(str::to_owned),
        };
        assert!(page(Some("https://api.bitbucket.org/2.0/x?page=2"), None).more(1, 30));
        assert!(page(None, Some(31)).more(1, 30));
        assert!(!page(None, Some(30)).more(1, 30));
        assert!(page(None, Some(201)).more(2, 100));
        assert!(!page(None, Some(200)).more(2, 100));
        assert!(!page(None, None).more(1, 30));
        // A page answer without `values` reads as empty.
        let empty: Page<Pipeline> = serde_json::from_str(r#"{"page": 3}"#).unwrap();
        assert!(empty.values.is_empty() && !empty.more(3, 100));
    }
}
