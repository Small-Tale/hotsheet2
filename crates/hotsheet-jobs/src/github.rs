//! The GitHub Actions watch source (HS2-3R9V0R): one watch Job follows the CI gate of one
//! commit, with the semantics of `scripts/ci-gate.mjs status --covers <sha>`. The gate is
//! green when a successful push run of the workflow on the shipping branch has the commit
//! as its head or as an ancestor of its head.
//!
//! All provider access goes through [`GithubApi`], so tests run offline with fixtures.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::model::{ExternalRef, Job, Outcome};
use crate::monitor::{CancelReply, Observation, Observed, Poll, WatchError, WatchSource};
use crate::steps::{Step, StepStatus};

/// The Job kind name and the `external.system` of a CI watch.
pub const GITHUB_ACTIONS_KIND: &str = "github-actions";

/// How many push runs of the branch one poll looks at.
const RUNS_PER_PAGE: usize = 30;
/// Ancestry lookups one poll may start; the rest wait for the next poll.
const MAX_COMPARES_PER_POLL: usize = 5;
/// Successful runs needed before a duration estimate is given.
const MIN_DURATIONS: usize = 3;
const MAX_DURATIONS: usize = 10;
/// HS2-XGV2J9: how long after a watch Job's creation the provider may keep answering
/// that it does not know the commit before the watch ends with gate `unknown`. GitHub
/// learns a pushed commit within seconds, so this only ends watches of a mistyped SHA
/// or a commit that was never pushed. Measured on the store's clock.
pub const UNKNOWN_COMMIT_LIMIT_MS: u64 = 30 * 60_000;

/// HS2-TPAHYA: the Job kind name and the `external.system` of a GitLab CI watch.
pub const GITLAB_CI_KIND: &str = "gitlab-ci";

/// HS2-17HVNP: the Job kind name and the `external.system` of a Buildkite watch.
pub const BUILDKITE_KIND: &str = "buildkite";
/// HS2-17HVNP: the Job kind name and the `external.system` of a CircleCI watch.
pub const CIRCLECI_KIND: &str = "circleci";
/// HS2-7H0S8R: the Job kind name and the `external.system` of a Jenkins watch.
pub const JENKINS_KIND: &str = "jenkins";
/// HS2-JZ1DD2: the Job kind name and the `external.system` of an Azure Pipelines watch.
pub const AZURE_PIPELINES_KIND: &str = "azure-pipelines";
/// HS2-BWF72S: the Job kind name and the `external.system` of a Bitbucket Pipelines
/// watch.
pub const BITBUCKET_PIPELINES_KIND: &str = "bitbucket-pipelines";

/// HS2-TPAHYA: which CI provider a watch observes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CiProvider {
    #[default]
    GithubActions,
    GitlabCi,
    /// HS2-17HVNP: Buildkite builds of one pipeline.
    Buildkite,
    /// HS2-17HVNP: CircleCI pipelines and their workflows.
    Circleci,
    /// HS2-7H0S8R: Jenkins builds of one job, or of a multibranch project's branch job.
    Jenkins,
    /// HS2-JZ1DD2: Azure Pipelines builds of one pipeline definition.
    AzurePipelines,
    /// HS2-BWF72S: Bitbucket Pipelines of one repository.
    BitbucketPipelines,
}

impl CiProvider {
    /// The Job kind name and external system of its watch.
    pub fn kind(self) -> &'static str {
        match self {
            Self::GithubActions => GITHUB_ACTIONS_KIND,
            Self::GitlabCi => GITLAB_CI_KIND,
            Self::Buildkite => BUILDKITE_KIND,
            Self::Circleci => CIRCLECI_KIND,
            Self::Jenkins => JENKINS_KIND,
            Self::AzurePipelines => AZURE_PIPELINES_KIND,
            Self::BitbucketPipelines => BITBUCKET_PIPELINES_KIND,
        }
    }

    pub const ALL: [Self; 7] = [
        Self::GithubActions,
        Self::GitlabCi,
        Self::Buildkite,
        Self::Circleci,
        Self::Jenkins,
        Self::AzurePipelines,
        Self::BitbucketPipelines,
    ];

    pub fn from_kind(kind: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.kind() == kind)
    }

    /// HS2-17HVNP: whether the provider's API can tell that a build of a later commit
    /// contains the watched one. Without it a gate attests exact coverage only, unless
    /// (HS2-56MTYN, Buildkite and CircleCI) the repository the provider names is on a
    /// host the source can ask ([`crate::RepositoryHosts`]).
    pub fn attests_ancestors(self) -> bool {
        matches!(self, Self::GithubActions | Self::GitlabCi)
    }
}

/// The commit whose CI gate is watched, and where to look for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CiTarget {
    /// HS2-TPAHYA: the provider that runs the CI.
    pub provider: CiProvider,
    /// `owner/name`; for GitLab the project's full path, `group/[subgroup/…]project`;
    /// for Buildkite `organization/pipeline` (slugs); for CircleCI the project slug
    /// `vcs/organization/project`, for example `gh/acme/widgets` (HS2-17HVNP); for
    /// Jenkins the job's full name, `[folder/…]job` (HS2-7H0S8R); for Azure Pipelines
    /// `organization/project/definition-id` (HS2-JZ1DD2); for Bitbucket Pipelines
    /// `workspace/repo_slug` (HS2-BWF72S).
    pub repo: String,
    /// Full 40-character commit SHA, lowercase.
    pub sha: String,
    /// The workflow's name as GitHub shows it, for example `CI`. For GitLab the
    /// pipeline's `name`, or `*` for any push pipeline of the branch.
    pub workflow: String,
    /// The shipping branch whose push runs count.
    pub branch: String,
    /// HS2-TQG6AT: the artifact whose digest a green verdict records, by name. Without
    /// one, a run's only artifact is recorded.
    pub artifact: Option<String>,
}

/// A run already judged, which a re-watch must not judge again. A re-run of the same run
/// keeps its id and raises its attempt, so both are compared.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RunFloor {
    pub run_id: u64,
    pub attempt: u32,
}

/// `owner/name` from a Git remote URL in any of its usual spellings.
pub fn repo_from_remote(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let path = match trimmed.split_once("://") {
        Some((_, rest)) => rest.split_once('/')?.1,
        // The scp-like form: git@host:owner/name
        None => trimmed.split_once(':')?.1,
    };
    let mut parts = path.rsplit('/');
    let (name, owner) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}"))
}

impl CiTarget {
    /// Validate and normalize. A refused value is never echoed.
    pub fn new(
        repo: &str,
        sha: &str,
        workflow: &str,
        branch: &str,
    ) -> Result<Self, crate::JobError> {
        let invalid = |field| crate::JobError::Invalid(field);
        match repo.split_once('/') {
            Some((owner, name)) if part(owner) && part(name) => {}
            _ => return Err(invalid("ci.repo")),
        }
        Self::checked(CiProvider::GithubActions, repo, sha, workflow, branch)
    }

    /// HS2-TPAHYA: a GitLab CI target. `project` is the project's full path, two to
    /// twenty segments. `workflow` is a pipeline name, or `*` for any push pipeline.
    pub fn gitlab(
        project: &str,
        sha: &str,
        workflow: &str,
        branch: &str,
    ) -> Result<Self, crate::JobError> {
        let segments: Vec<&str> = project.split('/').collect();
        if !(2..=20).contains(&segments.len())
            || project.len() > 255
            || !segments.iter().all(|s| part(s))
        {
            return Err(crate::JobError::Invalid("ci.repo"));
        }
        Self::checked(CiProvider::GitlabCi, project, sha, workflow, branch)
    }

    /// HS2-17HVNP: a Buildkite target: `pipeline` is `organization-slug/pipeline-slug`.
    /// Buildkite builds have no workflow of their own, so `workflow` must be `*`.
    pub fn buildkite(
        pipeline: &str,
        sha: &str,
        workflow: &str,
        branch: &str,
    ) -> Result<Self, crate::JobError> {
        match pipeline.split_once('/') {
            Some((organization, slug)) if slug_part(organization) && slug_part(slug) => {}
            _ => return Err(crate::JobError::Invalid("ci.repo")),
        }
        if workflow != "*" {
            return Err(crate::JobError::Invalid("ci.workflow"));
        }
        Self::checked(CiProvider::Buildkite, pipeline, sha, workflow, branch)
    }

    /// HS2-17HVNP: a CircleCI target: `project` is the API v2 project slug,
    /// `vcs/organization/project`. `workflow` is a workflow name, or `*` for every
    /// workflow of the pipeline.
    pub fn circleci(
        project: &str,
        sha: &str,
        workflow: &str,
        branch: &str,
    ) -> Result<Self, crate::JobError> {
        let segments: Vec<&str> = project.split('/').collect();
        let valid = segments.len() == 3
            && ["gh", "github", "bb", "bitbucket", "circleci"].contains(&segments[0])
            && segments[1..].iter().all(|s| part(s));
        if !valid {
            return Err(crate::JobError::Invalid("ci.repo"));
        }
        Self::checked(CiProvider::Circleci, project, sha, workflow, branch)
    }

    /// HS2-7H0S8R: a Jenkins target: `job` is the job's full name, one to ten
    /// `/`-separated segments (`folder/job`, or a multibranch project, whose branch job
    /// the watch reads). Jenkins builds have no workflow of their own, so `workflow`
    /// must be `*`.
    pub fn jenkins(
        job: &str,
        sha: &str,
        workflow: &str,
        branch: &str,
    ) -> Result<Self, crate::JobError> {
        let segments: Vec<&str> = job.split('/').collect();
        if !(1..=10).contains(&segments.len()) || !segments.iter().all(|s| part(s)) {
            return Err(crate::JobError::Invalid("ci.repo"));
        }
        if workflow != "*" {
            return Err(crate::JobError::Invalid("ci.workflow"));
        }
        Self::checked(CiProvider::Jenkins, job, sha, workflow, branch)
    }

    /// HS2-JZ1DD2: an Azure Pipelines target: `pipeline` is
    /// `organization/project/definition-id`. A project name may hold spaces; the
    /// definition is the pipeline's numeric id. Builds have no workflow of their own, so
    /// `workflow` must be `*`.
    pub fn azure_pipelines(
        pipeline: &str,
        sha: &str,
        workflow: &str,
        branch: &str,
    ) -> Result<Self, crate::JobError> {
        let valid = match pipeline.split('/').collect::<Vec<_>>().as_slice() {
            [organization, project, definition] => {
                part(organization)
                    && azure_project(project)
                    && (1..=10).contains(&definition.len())
                    && definition.bytes().all(|b| b.is_ascii_digit())
                    && definition.parse::<u32>().is_ok_and(|id| id > 0)
            }
            _ => false,
        };
        if !valid {
            return Err(crate::JobError::Invalid("ci.repo"));
        }
        if workflow != "*" {
            return Err(crate::JobError::Invalid("ci.workflow"));
        }
        Self::checked(CiProvider::AzurePipelines, pipeline, sha, workflow, branch)
    }

    /// HS2-BWF72S: a Bitbucket Pipelines target: `repository` is `workspace/repo_slug`.
    /// Its pipelines have no workflow of their own here, so `workflow` must be `*`.
    pub fn bitbucket_pipelines(
        repository: &str,
        sha: &str,
        workflow: &str,
        branch: &str,
    ) -> Result<Self, crate::JobError> {
        match repository.split_once('/') {
            Some((workspace, slug)) if part(workspace) && part(slug) => {}
            _ => return Err(crate::JobError::Invalid("ci.repo")),
        }
        if workflow != "*" {
            return Err(crate::JobError::Invalid("ci.workflow"));
        }
        Self::checked(
            CiProvider::BitbucketPipelines,
            repository,
            sha,
            workflow,
            branch,
        )
    }

    /// HS2-17HVNP: the target of any provider, as its own constructor validates it.
    pub fn for_provider(
        provider: CiProvider,
        repo: &str,
        sha: &str,
        workflow: &str,
        branch: &str,
    ) -> Result<Self, crate::JobError> {
        match provider {
            CiProvider::GithubActions => Self::new(repo, sha, workflow, branch),
            CiProvider::GitlabCi => Self::gitlab(repo, sha, workflow, branch),
            CiProvider::Buildkite => Self::buildkite(repo, sha, workflow, branch),
            CiProvider::Circleci => Self::circleci(repo, sha, workflow, branch),
            CiProvider::Jenkins => Self::jenkins(repo, sha, workflow, branch),
            CiProvider::AzurePipelines => Self::azure_pipelines(repo, sha, workflow, branch),
            CiProvider::BitbucketPipelines => {
                Self::bitbucket_pipelines(repo, sha, workflow, branch)
            }
        }
    }

    fn checked(
        provider: CiProvider,
        repo: &str,
        sha: &str,
        workflow: &str,
        branch: &str,
    ) -> Result<Self, crate::JobError> {
        let invalid = |field| crate::JobError::Invalid(field);
        if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("ci.sha"));
        }
        let branch_ok = !branch.is_empty()
            && branch.len() <= 100
            && !branch.contains("..")
            && branch.as_bytes()[0].is_ascii_alphanumeric()
            && branch
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b));
        if !branch_ok {
            return Err(invalid("ci.branch"));
        }
        if workflow.trim().is_empty()
            || workflow.len() > 100
            || workflow.chars().any(char::is_control)
        {
            return Err(invalid("ci.workflow"));
        }
        Ok(Self {
            provider,
            repo: repo.to_owned(),
            sha: sha.to_ascii_lowercase(),
            workflow: workflow.to_owned(),
            branch: branch.to_owned(),
            artifact: None,
        })
    }

    /// HS2-TQG6AT: name the artifact a green verdict records. A name is letters,
    /// digits, `-`, `_` and `.`, at most 160 characters.
    pub fn with_artifact(mut self, artifact: Option<&str>) -> Result<Self, crate::JobError> {
        if let Some(name) = artifact {
            let valid = !name.is_empty()
                && name.len() <= 160
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
            if !valid {
                return Err(crate::JobError::Invalid("ci.artifact"));
            }
        }
        self.artifact = artifact.map(str::to_owned);
        Ok(self)
    }

    /// Groups runs of one workflow on one branch for the history ETA.
    pub fn job_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.provider.kind(),
            self.repo,
            self.branch,
            self.workflow
        )
    }

    /// The external operation of a watch Job for this target.
    pub fn external(&self, floor: Option<RunFloor>) -> ExternalRef {
        let floor = floor.map_or_else(
            || "-".to_owned(),
            |floor| format!("{}.{}", floor.run_id, floor.attempt),
        );
        let mut parts = vec![
            self.sha.as_str(),
            floor.as_str(),
            self.repo.as_str(),
            self.branch.as_str(),
            self.workflow.as_str(),
        ];
        // HS2-TQG6AT: a sixth part only when an artifact is named, so an earlier
        // reference keeps its exact spelling.
        if let Some(artifact) = &self.artifact {
            parts.push(artifact);
        }
        let id = parts.into_iter().map(escape).collect::<Vec<_>>().join("|");
        ExternalRef {
            system: self.provider.kind().to_owned(),
            id,
        }
    }

    /// Read a target back from a Job's external operation.
    pub fn parse(external: &ExternalRef) -> Option<(Self, Option<RunFloor>)> {
        let provider = CiProvider::from_kind(&external.system)?;
        let parts: Vec<String> = external.id.split('|').map(unescape).collect();
        let (sha, floor, repo, branch, workflow, artifact) = match parts.as_slice() {
            [sha, floor, repo, branch, workflow] => (sha, floor, repo, branch, workflow, None),
            [sha, floor, repo, branch, workflow, artifact] => {
                (sha, floor, repo, branch, workflow, Some(artifact.as_str()))
            }
            _ => return None,
        };
        let floor = match floor.as_str() {
            "-" => None,
            text => {
                let (run_id, attempt) = text.split_once('.')?;
                Some(RunFloor {
                    run_id: run_id.parse().ok()?,
                    attempt: attempt.parse().ok()?,
                })
            }
        };
        let target = Self::for_provider(provider, repo, sha, workflow, branch)
            .ok()?
            .with_artifact(artifact)
            .ok()?;
        Some((target, floor))
    }

    pub(crate) fn short_sha(&self) -> &str {
        &self.sha[..7]
    }
}

/// HS2-17HVNP: a Buildkite slug: lowercase letters, digits and `-`.
fn slug_part(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// HS2-JZ1DD2: an Azure DevOps project name: letters, digits, spaces, `-`, `_` and
/// `.`, at most 64 characters, not starting or ending with a space or `.`.
fn azure_project(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with([' ', '.'])
        && !value.ends_with([' ', '.'])
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" -_.".contains(&b))
}

/// One segment of a repository or project path.
fn part(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn escape(part: &str) -> String {
    part.replace('%', "%25").replace('|', "%7C")
}

fn unescape(part: &str) -> String {
    part.replace("%7C", "|").replace("%25", "%")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiMethod {
    Get,
    Post,
    /// HS2-17HVNP: Buildkite cancels a build with `PUT`.
    Put,
    /// HS2-JZ1DD2: Azure Pipelines cancels a build with `PATCH`.
    Patch,
}

/// One REST request. `path` is relative to the API root, for example
/// `repos/acme/widgets/actions/runs?branch=main`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiRequest {
    pub method: ApiMethod,
    pub path: String,
    /// Sent as `If-None-Match`. A 304 answer costs no rate limit.
    pub if_none_match: Option<String>,
    /// HS2-JZ1DD2: a JSON request body, sent as `application/json`.
    pub body: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiResponse {
    pub status: u16,
    pub etag: Option<String>,
    /// From `Retry-After`, or from the rate-limit reset when the limit is spent.
    pub retry_after_ms: Option<u64>,
    pub body: String,
}

/// No answer at all: no network, no credentials, a timeout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiUnavailable {
    pub retry_after_ms: Option<u64>,
}

/// The only way this crate reaches GitHub. The CLI supplies the HTTP implementation;
/// tests supply fixtures.
pub trait GithubApi: Send {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable>;
}

#[derive(Clone, Debug, Deserialize)]
struct RunList {
    #[serde(default)]
    workflow_runs: Vec<Run>,
}

#[derive(Clone, Debug, Deserialize)]
struct Run {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    head_branch: Option<String>,
    head_sha: String,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    run_started_at: Option<String>,
    #[serde(default)]
    run_attempt: Option<u32>,
}

impl Run {
    fn attempt(&self) -> u32 {
        self.run_attempt.unwrap_or(1)
    }

    fn completed(&self) -> bool {
        self.status.as_deref() == Some("completed")
    }

    fn conclusion(&self) -> &str {
        self.conclusion.as_deref().unwrap_or("")
    }

    fn started_ms(&self) -> Option<u64> {
        self.run_started_at
            .as_deref()
            .or(self.created_at.as_deref())
            .and_then(parse_timestamp_ms)
    }
}

#[derive(Clone, Debug, Deserialize)]
struct JobList {
    #[serde(default)]
    jobs: Vec<WorkflowJob>,
}

#[derive(Clone, Debug, Deserialize)]
struct WorkflowJob {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    steps: Vec<WorkflowStep>,
}

#[derive(Clone, Debug, Deserialize)]
struct WorkflowStep {
    #[serde(default)]
    status: Option<String>,
}

/// HS2-3YC577: the artifacts a run uploaded.
#[derive(Clone, Debug, Deserialize)]
struct ArtifactList {
    #[serde(default)]
    artifacts: Vec<RunArtifact>,
}

#[derive(Clone, Debug, Deserialize)]
struct RunArtifact {
    #[serde(default)]
    name: Option<String>,
    /// `sha256:<hex>` for artifacts uploaded by `upload-artifact` v4 and later.
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    expired: bool,
}

/// The identity of one artifact a green run built, as the outcome records it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct BuiltArtifact {
    name: String,
    /// Lowercase hex SHA-256, without the `sha256:` prefix.
    digest: String,
}

/// Unexpired artifacts with a SHA-256 digest; the others cannot identify what was built.
fn built_artifacts(list: ArtifactList) -> Vec<BuiltArtifact> {
    list.artifacts
        .into_iter()
        .filter(|artifact| !artifact.expired)
        .filter_map(|artifact| {
            let digest = artifact
                .digest?
                .strip_prefix("sha256:")?
                .to_ascii_lowercase();
            let hex = digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            hex.then(|| BuiltArtifact {
                name: clean(artifact.name.as_deref().unwrap_or(""), 200),
                digest,
            })
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize)]
struct Comparison {
    status: String,
}

/// How a run's head relates to the watched commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Relation {
    /// The head is the commit or a descendant of it.
    Covers,
    /// The head is an ancestor of the commit: a run from before it.
    Before,
    /// Neither contains the other: the branch moved to another line.
    Diverged,
    /// GitHub does not know the commit (yet).
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Decision {
    /// No verdict yet. `run` is the newest covering run, when one exists.
    Pending {
        run: Option<usize>,
    },
    Green {
        run: usize,
    },
    Red {
        run: usize,
    },
    Cancelled {
        run: usize,
    },
    Superseded,
}

/// Judge the gate from push runs listed newest first. `relation` answers how a run head
/// relates to the commit; `None` means the lookup budget is spent for this poll.
fn decide(
    target: &CiTarget,
    floor: Option<RunFloor>,
    runs: &[Run],
    mut relation: impl FnMut(&str) -> Result<Option<Relation>, WatchError>,
) -> Result<Decision, WatchError> {
    let mut covering = Vec::new();
    let mut diverged_first = false;
    for (index, run) in runs.iter().enumerate() {
        let known = if run.head_sha.eq_ignore_ascii_case(&target.sha) {
            Relation::Covers
        } else {
            match relation(&run.head_sha)? {
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
        // The commit's own run is the oldest that can cover it.
        if run.head_sha.eq_ignore_ascii_case(&target.sha) {
            break;
        }
    }
    let judged = |index: &usize| {
        let run = &runs[*index];
        floor.is_none_or(|floor| {
            RunFloor {
                run_id: run.id,
                attempt: run.attempt(),
            } > floor
        })
    };
    let covering: Vec<usize> = covering.into_iter().filter(judged).collect();
    if let Some(&green) = covering
        .iter()
        .find(|&&index| runs[index].completed() && runs[index].conclusion() == "success")
    {
        return Ok(Decision::Green { run: green });
    }
    let Some(&newest) = covering.first() else {
        return Ok(if diverged_first {
            Decision::Superseded
        } else {
            Decision::Pending { run: None }
        });
    };
    let run = &runs[newest];
    if !run.completed() {
        return Ok(Decision::Pending { run: Some(newest) });
    }
    Ok(match run.conclusion() {
        "failure" | "timed_out" | "startup_failure" | "action_required" => {
            Decision::Red { run: newest }
        }
        "cancelled" => Decision::Cancelled { run: newest },
        // Skipped, neutral or stale: not a failure, and not the green run the gate needs.
        _ => Decision::Pending { run: Some(newest) },
    })
}

#[derive(Default)]
struct Cached {
    runs_etag: Option<String>,
    runs: Vec<Run>,
    jobs_run: Option<(u64, u32)>,
    jobs_etag: Option<String>,
    jobs: Vec<WorkflowJob>,
    /// HS2-3YC577: the artifacts of a green run, read once per run and attempt.
    artifacts_run: Option<(u64, u32)>,
    artifacts_etag: Option<String>,
    artifacts: Vec<BuiltArtifact>,
    /// The last observation was reported, so an unchanged poll may say "not modified".
    reported: bool,
    /// The newest covering run, for cancel.
    current: Option<(u64, String, bool)>,
}

/// Observes CI gates through a [`GithubApi`].
pub struct GithubActionsSource<A: GithubApi> {
    api: A,
    jobs: BTreeMap<String, Cached>,
    /// Ancestry never changes, so one answer per (commit, head) is kept.
    relations: BTreeMap<(String, String), Relation>,
}

impl<A: GithubApi> GithubActionsSource<A> {
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
            "repos/{}/compare/{}...{}?per_page=1",
            target.repo, target.sha, key.1
        );
        let response = self.get(path, None)?;
        let relation = match response.status {
            200 => {
                let comparison: Comparison =
                    serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                match comparison.status.as_str() {
                    "ahead" | "identical" => Relation::Covers,
                    "behind" => Relation::Before,
                    "diverged" => Relation::Diverged,
                    _ => return Err(transient(&response)),
                }
            }
            // Not pushed yet, or not in this repository. Not cached: it can appear.
            404 => return Ok(Some(Relation::Unknown)),
            _ => return Err(transient(&response)),
        };
        self.relations.insert(key, relation);
        Ok(Some(relation))
    }
}

pub(crate) fn transient(response: &ApiResponse) -> WatchError {
    WatchError::Transient {
        retry_after_ms: response.retry_after_ms,
    }
}

impl<A: GithubApi> WatchSource for GithubActionsSource<A> {
    fn kind(&self) -> &str {
        GITHUB_ACTIONS_KIND
    }

    fn poll(&mut self, job: &Job, now_ms: u64) -> Result<Poll, WatchError> {
        let (target, floor) = job
            .external
            .as_ref()
            .and_then(CiTarget::parse)
            .ok_or(WatchError::Gone)?;

        let etag = self.jobs.get(&job.id).and_then(|c| c.runs_etag.clone());
        let path = format!(
            "repos/{}/actions/runs?branch={}&event=push&per_page={RUNS_PER_PAGE}",
            target.repo,
            query_value(&target.branch)
        );
        let response = self.get(path, etag)?;
        let mut changed = false;
        match response.status {
            304 => {}
            200 => {
                let list: RunList =
                    serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                let mut runs: Vec<Run> = list
                    .workflow_runs
                    .into_iter()
                    .filter(|run| {
                        run.event.as_deref() == Some("push")
                            && run.head_branch.as_deref() == Some(target.branch.as_str())
                            && run.name.as_deref() == Some(target.workflow.as_str())
                    })
                    .collect();
                runs.sort_by(|a, b| (&b.created_at, b.id).cmp(&(&a.created_at, a.id)));
                let cached = self.jobs.entry(job.id.clone()).or_default();
                cached.runs = runs;
                cached.runs_etag = response.etag.clone();
                changed = true;
            }
            // The repository does not exist or cannot be read with these credentials.
            404 => return Err(WatchError::Gone),
            _ => return Err(transient(&response)),
        }

        let runs = self
            .jobs
            .get(&job.id)
            .map(|cached| cached.runs.clone())
            .unwrap_or_default();
        let mut budget = MAX_COMPARES_PER_POLL;
        // A lookup left for the next poll means this verdict is not final, so the next
        // poll must judge again even when the provider reports nothing new.
        let mut unfinished = false;
        let mut commit_unknown = false;
        let decision = decide(&target, floor, &runs, |head| {
            let relation = self.relation(&target, head, &mut budget)?;
            unfinished |= relation.is_none();
            commit_unknown |= relation == Some(Relation::Unknown);
            Ok(relation)
        })?;
        // HS2-XGV2J9: a commit the provider still does not know long after the watch
        // began will never get a run: end the watch instead of polling it forever.
        let never_reached = matches!(decision, Decision::Pending { run: None })
            && commit_unknown
            && now_ms.saturating_sub(job.created_ms) >= UNKNOWN_COMMIT_LIMIT_MS;
        if never_reached {
            return Ok(Poll::Observed(Box::new(Observation::new(
                Observed::Failed(outcome(&target, "unknown", None, &[])),
            ))));
        }
        let current = match decision {
            Decision::Pending { run } => run,
            Decision::Green { run } | Decision::Red { run } | Decision::Cancelled { run } => {
                Some(run)
            }
            Decision::Superseded => None,
        }
        .map(|index| &runs[index]);

        // The run's own jobs are the Job's steps.
        if let Some(run) = current {
            let same_run = self
                .jobs
                .get(&job.id)
                .is_some_and(|cached| cached.jobs_run == Some((run.id, run.attempt())));
            let etag = self
                .jobs
                .get(&job.id)
                .filter(|_| same_run)
                .and_then(|cached| cached.jobs_etag.clone());
            let path = format!(
                "repos/{}/actions/runs/{}/jobs?per_page=100",
                target.repo, run.id
            );
            let response = self.get(path, etag)?;
            match response.status {
                304 => {}
                200 => {
                    let list: JobList =
                        serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                    let cached = self.jobs.entry(job.id.clone()).or_default();
                    cached.jobs = list.jobs;
                    cached.jobs_run = Some((run.id, run.attempt()));
                    cached.jobs_etag = response.etag.clone();
                    changed = true;
                }
                _ => return Err(transient(&response)),
            }
        }

        // HS2-3YC577: a green verdict records what the run built. 403 and 404 mean the
        // artifacts cannot be read with these credentials (or there are none): the verdict
        // stands without them. Anything else is retried like the other reads.
        if let Decision::Green { run } = decision {
            let run = &runs[run];
            let key = Some((run.id, run.attempt()));
            let etag = self
                .jobs
                .get(&job.id)
                .filter(|cached| cached.artifacts_run == key)
                .and_then(|cached| cached.artifacts_etag.clone());
            let path = format!(
                "repos/{}/actions/runs/{}/artifacts?per_page=100",
                target.repo, run.id
            );
            let response = self.get(path, etag)?;
            let found = match response.status {
                304 => None,
                200 => Some(
                    serde_json::from_str::<ArtifactList>(&response.body)
                        .map(built_artifacts)
                        .map_err(|_| transient(&response))?,
                ),
                403 | 404 => Some(Vec::new()),
                _ => return Err(transient(&response)),
            };
            if let Some(found) = found {
                let cached = self.jobs.entry(job.id.clone()).or_default();
                cached.artifacts = found;
                cached.artifacts_run = key;
                cached.artifacts_etag = response.etag.clone().filter(|_| response.status == 200);
                changed = true;
            }
        }

        let cached = self.jobs.entry(job.id.clone()).or_default();
        cached.current = current.map(|run| (run.id, run.head_sha.clone(), run.completed()));
        if !changed && cached.reported {
            return Ok(Poll::NotModified);
        }
        cached.reported = !unfinished;
        let steps = current.map(|_| steps_of(&cached.jobs));
        let current_step = cached
            .jobs
            .iter()
            .find(|job| job.status.as_deref() == Some("in_progress"))
            .and_then(|job| job.name.as_deref())
            .map(|name| clean(name, 200));

        let state = match decision {
            Decision::Pending { run: None } => Observed::Waiting,
            Decision::Pending { run: Some(index) } => {
                if runs[index].status.as_deref() == Some("in_progress") {
                    Observed::Running
                } else {
                    Observed::Waiting
                }
            }
            Decision::Green { run } => Observed::Succeeded(outcome(
                &target,
                "green",
                Some(&runs[run]),
                &cached.artifacts,
            )),
            Decision::Red { run } => {
                Observed::Failed(outcome(&target, "red", Some(&runs[run]), &[]))
            }
            Decision::Cancelled { run } => {
                Observed::Cancelled(outcome(&target, "cancelled", Some(&runs[run]), &[]))
            }
            Decision::Superseded => Observed::Failed(outcome(&target, "superseded", None, &[])),
        };
        let mut latest = BTreeMap::new();
        if let Some(run) = current {
            latest.insert("run_id".to_owned(), run.id.to_string());
            latest.insert("run_attempt".to_owned(), run.attempt().to_string());
            latest.insert("run_head_sha".to_owned(), clean(&run.head_sha, 64));
            let status = if run.completed() {
                run.conclusion()
            } else {
                run.status.as_deref().unwrap_or("queued")
            };
            latest.insert("run_status".to_owned(), clean(status, 64));
        }
        if let Some(step) = current_step {
            latest.insert("current_step".to_owned(), step);
        }
        // A queued run has not started, so there is nothing to add a duration to yet.
        let running = current.filter(|run| run.status.as_deref() == Some("in_progress"));
        Ok(Poll::Observed(Box::new(Observation {
            steps,
            eta_finish_at_ms: running.and_then(|run| estimate_finish(&runs, run, now_ms)),
            latest,
            url: current.and_then(|run| run.html_url.clone()).filter(|url| {
                url.starts_with("https://")
                    && url.len() <= 2000
                    && !url.chars().any(char::is_control)
            }),
            ..Observation::new(state)
        })))
    }

    fn cancel(&mut self, job: &Job) -> CancelReply {
        let Some((target, _)) = job.external.as_ref().and_then(CiTarget::parse) else {
            return CancelReply::Unsupported;
        };
        let Some((run_id, head, completed)) = self
            .jobs
            .get(&job.id)
            .and_then(|cached| cached.current.clone())
        else {
            return CancelReply::Unsupported;
        };
        // A covering run that belongs to a newer commit is someone else's run. It is
        // never cancelled from here; stop watching is the action for that case.
        if completed || !head.eq_ignore_ascii_case(&target.sha) {
            return CancelReply::Unsupported;
        }
        let request = ApiRequest {
            method: ApiMethod::Post,
            path: format!("repos/{}/actions/runs/{run_id}/cancel", target.repo),
            if_none_match: None,
            body: None,
        };
        match self.api.send(&request) {
            Ok(response) if (200..300).contains(&response.status) => CancelReply::Requested,
            Ok(response) if response.status == 403 || response.status == 404 => {
                CancelReply::Failed("GitHub did not permit cancelling this run".into())
            }
            Ok(response) if response.status == 409 => {
                CancelReply::Failed("The run can no longer be cancelled".into())
            }
            Ok(response) => {
                CancelReply::Failed(format!("GitHub answered {} to the cancel", response.status))
            }
            Err(_) => CancelReply::Failed("GitHub could not be reached to cancel the run".into()),
        }
    }

    fn forget(&mut self, job_id: &str) {
        self.jobs.remove(job_id);
    }
}

/// The outcome record of a finished CI watch. It carries identifiers only; the ticket
/// result summary (HS2-BDH12X), the notifier (HS2-FCBYC8) and the orchestration layer
/// read it through [`crate::ci_outcome`].
///
/// HS2-3YC577: a green outcome also records `artifact_count`, and the artifact's
/// `artifact_name` and `artifact_digest` when the run built exactly one with a digest.
/// Several artifacts record none, since nothing says which one a deployment ships,
/// unless the target names one (HS2-TQG6AT): then exactly the artifact of that name.
fn outcome(
    target: &CiTarget,
    gate: &str,
    run: Option<&Run>,
    artifacts: &[BuiltArtifact],
) -> Outcome {
    let mut refs = BTreeMap::from([
        ("gate".to_owned(), gate.to_owned()),
        ("commit".to_owned(), target.sha.clone()),
        ("repo".to_owned(), target.repo.clone()),
        ("branch".to_owned(), target.branch.clone()),
        ("workflow".to_owned(), clean(&target.workflow, 200)),
    ]);
    let summary = match run {
        Some(run) => {
            refs.insert("run_id".to_owned(), run.id.to_string());
            refs.insert("run_attempt".to_owned(), run.attempt().to_string());
            refs.insert("run_head_sha".to_owned(), clean(&run.head_sha, 64));
            if !run.conclusion().is_empty() {
                refs.insert("run_conclusion".to_owned(), clean(run.conclusion(), 64));
            }
            if gate == "green" {
                refs.insert("artifact_count".to_owned(), artifacts.len().to_string());
                // HS2-TQG6AT: a named artifact is looked for by name; otherwise the
                // run's only one is recorded.
                let named: Vec<&BuiltArtifact> = match &target.artifact {
                    Some(name) => artifacts.iter().filter(|a| &a.name == name).collect(),
                    None => artifacts.iter().collect(),
                };
                if let [artifact] = named.as_slice() {
                    refs.insert("artifact_name".to_owned(), artifact.name.clone());
                    refs.insert("artifact_digest".to_owned(), artifact.digest.clone());
                }
            }
            if let Some(url) = run.html_url.as_deref().filter(|url| {
                url.starts_with("https://")
                    && url.len() <= 500
                    && !url.chars().any(char::is_control)
            }) {
                refs.insert("run_url".to_owned(), url.to_owned());
            }
            let verdict = match gate {
                "green" => "passed",
                "red" => "failed",
                _ => "was cancelled",
            };
            format!(
                "{} run {} {verdict} for {}",
                clean(&target.workflow, 100),
                run.id,
                target.short_sha()
            )
        }
        None if gate == "unknown" => format!(
            "{} is not known to {} after {} minutes",
            target.short_sha(),
            clean(&target.repo, 200),
            UNKNOWN_COMMIT_LIMIT_MS / 60_000
        ),
        None => format!(
            "{} is no longer on {}",
            target.short_sha(),
            clean(&target.branch, 100)
        ),
    };
    Outcome {
        summary: Some(summary),
        refs,
    }
}

fn steps_of(jobs: &[WorkflowJob]) -> Vec<Step> {
    jobs.iter()
        .take(crate::steps::MAX_STEPS)
        .map(|job| {
            let status = match (job.status.as_deref(), job.conclusion.as_deref()) {
                (Some("completed"), Some("success")) => StepStatus::Succeeded,
                (Some("completed"), Some("skipped" | "neutral")) => StepStatus::Skipped,
                (Some("completed"), Some("cancelled")) => StepStatus::Cancelled,
                (Some("completed"), _) => StepStatus::Failed,
                (Some("in_progress"), _) => StepStatus::Active,
                _ => StepStatus::Pending,
            };
            let total = job.steps.len() as u64;
            let done = job
                .steps
                .iter()
                .filter(|step| step.status.as_deref() == Some("completed"))
                .count() as u64;
            let name = job.name.as_deref().unwrap_or("job");
            Step {
                id: format!("job-{}", job.id),
                label: clean(name, 200),
                status,
                done: (total > 0).then_some(done),
                total: (total > 0).then_some(total),
                unit: (total > 0).then(|| "steps".to_owned()),
                weight: 1,
                children: Vec::new(),
            }
        })
        .collect()
}

/// The run's start plus the median duration of recent successful runs of the workflow.
fn estimate_finish(runs: &[Run], current: &Run, now_ms: u64) -> Option<u64> {
    let mut durations: Vec<u64> = runs
        .iter()
        .filter(|run| run.completed() && run.conclusion() == "success")
        .filter_map(|run| {
            let started = run.started_ms()?;
            let ended = parse_timestamp_ms(run.updated_at.as_deref()?)?;
            ended.checked_sub(started).filter(|duration| *duration > 0)
        })
        .take(MAX_DURATIONS)
        .collect();
    if durations.len() < MIN_DURATIONS {
        return None;
    }
    durations.sort_unstable();
    let median = durations[durations.len() / 2];
    Some(
        current
            .started_ms()
            .unwrap_or(now_ms)
            .saturating_add(median),
    )
}

/// Provider text made safe for a bounded single-line record field.
pub(crate) fn clean(text: &str, max: usize) -> String {
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut line = line.trim().to_owned();
    while line.len() > max {
        line.pop();
    }
    if line.trim().is_empty() {
        "unnamed".to_owned()
    } else {
        line.trim().to_owned()
    }
}

pub(crate) fn query_value(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `2026-11-01T10:00:00Z` as milliseconds since the Unix epoch. GitHub's REST API
/// answers in this one UTC form.
pub(crate) fn parse_timestamp_ms(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<u64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if year < 1970
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Days from the civil date (proleptic Gregorian), with March as the first month.
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year / 400;
    let year_of_era = shifted_year - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = (era * 146_097 + day_of_era).checked_sub(719_468)?;
    Some(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "1111111111111111111111111111111111111111";

    fn target() -> CiTarget {
        CiTarget::new("acme/widgets", SHA, "CI", "main").unwrap()
    }

    fn run(id: u64, head: &str, status: &str, conclusion: Option<&str>) -> Run {
        Run {
            id,
            name: Some("CI".into()),
            head_branch: Some("main".into()),
            head_sha: head.into(),
            event: Some("push".into()),
            status: Some(status.into()),
            conclusion: conclusion.map(Into::into),
            html_url: None,
            created_at: None,
            updated_at: None,
            run_started_at: None,
            run_attempt: Some(1),
        }
    }

    fn judge(runs: &[Run], relations: &[(&str, Relation)]) -> Decision {
        decide(&target(), None, runs, |head| {
            Ok(relations
                .iter()
                .find(|(known, _)| *known == head)
                .map(|(_, relation)| *relation))
        })
        .unwrap()
    }

    #[test]
    fn timestamps_parse_to_epoch_milliseconds() {
        assert_eq!(parse_timestamp_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_timestamp_ms("2000-03-01T00:00:00Z"),
            Some(951_868_800_000)
        );
        assert_eq!(
            parse_timestamp_ms("2024-02-29T12:30:15Z"),
            Some(1_709_209_815_000)
        );
        assert_eq!(
            parse_timestamp_ms("2026-11-12T01:02:03Z"),
            Some(1_794_445_323_000)
        );
        for bad in [
            "",
            "2026-11-12",
            "2026-11-12T01:02:03",
            "2026-11-12T01:02:03+00:00",
            "2026-13-12T01:02:03Z",
            "1969-12-31T23:59:59Z",
            "2026-11-12 01:02:03Z",
            "2026-11-1ZT01:02:03Z",
        ] {
            assert_eq!(parse_timestamp_ms(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_target_round_trips_through_its_external_reference() {
        let odd = CiTarget::new("acme/widgets", SHA, "CI | 100% checks", "release/1.x").unwrap();
        for floor in [
            None,
            Some(RunFloor {
                run_id: 42,
                attempt: 3,
            }),
        ] {
            let external = odd.external(floor);
            assert_eq!(external.system, GITHUB_ACTIONS_KIND);
            assert_eq!(CiTarget::parse(&external), Some((odd.clone(), floor)));
        }
        let upper = CiTarget::new("acme/widgets", &SHA.replace('1', "A"), "CI", "main").unwrap();
        assert_eq!(upper.sha, "a".repeat(40));
        let other = ExternalRef {
            system: "fixture".into(),
            id: target().external(None).id,
        };
        assert_eq!(CiTarget::parse(&other), None);
        let short = ExternalRef {
            system: GITHUB_ACTIONS_KIND.into(),
            id: "a|b".into(),
        };
        assert_eq!(CiTarget::parse(&short), None);
    }

    #[test]
    fn invalid_targets_are_refused_by_field() {
        let cases = [
            ("widgets", SHA, "CI", "main", "ci.repo"),
            ("acme/../x", SHA, "CI", "main", "ci.repo"),
            ("acme/wid gets", SHA, "CI", "main", "ci.repo"),
            ("acme/widgets", "abc123", "CI", "main", "ci.sha"),
            ("acme/widgets", &"g".repeat(40), "CI", "main", "ci.sha"),
            ("acme/widgets", SHA, " ", "main", "ci.workflow"),
            ("acme/widgets", SHA, "CI\nx", "main", "ci.workflow"),
            ("acme/widgets", SHA, "CI", "", "ci.branch"),
            ("acme/widgets", SHA, "CI", "-main", "ci.branch"),
            ("acme/widgets", SHA, "CI", "a..b", "ci.branch"),
            ("acme/widgets", SHA, "CI", "a b", "ci.branch"),
        ];
        for (repo, sha, workflow, branch, field) in cases {
            match CiTarget::new(repo, sha, workflow, branch) {
                Err(crate::JobError::Invalid(found)) => assert_eq!(found, field),
                other => panic!("expected {field}, got {other:?}"),
            }
        }
    }

    #[test]
    fn the_commits_own_run_decides_when_nothing_newer_covers_it() {
        let own = |status, conclusion| {
            vec![
                run(2, SHA, status, conclusion),
                run(1, "old", "completed", Some("success")),
            ]
        };
        let cases = [
            ("queued", None, Decision::Pending { run: Some(0) }),
            ("in_progress", None, Decision::Pending { run: Some(0) }),
            ("completed", Some("success"), Decision::Green { run: 0 }),
            ("completed", Some("failure"), Decision::Red { run: 0 }),
            ("completed", Some("timed_out"), Decision::Red { run: 0 }),
            (
                "completed",
                Some("startup_failure"),
                Decision::Red { run: 0 },
            ),
            (
                "completed",
                Some("action_required"),
                Decision::Red { run: 0 },
            ),
            (
                "completed",
                Some("cancelled"),
                Decision::Cancelled { run: 0 },
            ),
            (
                "completed",
                Some("skipped"),
                Decision::Pending { run: Some(0) },
            ),
            (
                "completed",
                Some("neutral"),
                Decision::Pending { run: Some(0) },
            ),
            (
                "completed",
                Some("stale"),
                Decision::Pending { run: Some(0) },
            ),
        ];
        for (status, conclusion, expected) in cases {
            // The older run is never even looked up: the commit's own run ends the search.
            assert_eq!(
                judge(&own(status, conclusion), &[]),
                expected,
                "{status} {conclusion:?}"
            );
        }
    }

    #[test]
    fn a_newer_run_covers_the_commit_when_its_head_descends_from_it() {
        // The commit's own run failed, but a later green run contains the commit.
        let runs = vec![
            run(3, "newer", "completed", Some("success")),
            run(2, SHA, "completed", Some("failure")),
        ];
        assert_eq!(
            judge(&runs, &[("newer", Relation::Covers)]),
            Decision::Green { run: 0 }
        );
        // A newer covering run still in progress keeps the gate pending, even though the
        // commit's own run was cancelled when the newer push superseded it.
        let runs = vec![
            run(3, "newer", "in_progress", None),
            run(2, SHA, "completed", Some("cancelled")),
        ];
        assert_eq!(
            judge(&runs, &[("newer", Relation::Covers)]),
            Decision::Pending { run: Some(0) }
        );
        // The newest covering run failing is red even when an older covering run is pending.
        let runs = vec![
            run(3, "newer", "completed", Some("failure")),
            run(2, SHA, "in_progress", None),
        ];
        assert_eq!(
            judge(&runs, &[("newer", Relation::Covers)]),
            Decision::Red { run: 0 }
        );
        // An older green covering run still makes the gate green.
        let runs = vec![
            run(3, "newer", "completed", Some("failure")),
            run(2, SHA, "completed", Some("success")),
        ];
        assert_eq!(
            judge(&runs, &[("newer", Relation::Covers)]),
            Decision::Green { run: 1 }
        );
    }

    #[test]
    fn runs_from_before_the_commit_never_count() {
        let runs = vec![
            run(2, "older", "completed", Some("success")),
            run(1, "oldest", "completed", Some("success")),
        ];
        let mut asked = Vec::new();
        let decision = decide(&target(), None, &runs, |head| {
            asked.push(head.to_owned());
            Ok(Some(Relation::Before))
        })
        .unwrap();
        assert_eq!(decision, Decision::Pending { run: None });
        assert_eq!(
            asked,
            ["older"],
            "the search stops at the first earlier run"
        );
        assert_eq!(judge(&[], &[]), Decision::Pending { run: None });
        // A commit GitHub does not know yet is simply not there yet.
        assert_eq!(
            judge(&runs, &[("older", Relation::Unknown)]),
            Decision::Pending { run: None }
        );
    }

    #[test]
    fn a_branch_that_moved_without_the_commit_supersedes_it() {
        let runs = vec![
            run(3, "elsewhere", "completed", Some("success")),
            run(2, "older", "completed", Some("success")),
        ];
        assert_eq!(
            judge(
                &runs,
                &[
                    ("elsewhere", Relation::Diverged),
                    ("older", Relation::Before)
                ]
            ),
            Decision::Superseded
        );
        // A diverged run below a covering one does not supersede: the tip still covers.
        let runs = vec![
            run(3, "newer", "in_progress", None),
            run(2, "elsewhere", "completed", Some("success")),
            run(1, SHA, "completed", Some("failure")),
        ];
        assert_eq!(
            judge(
                &runs,
                &[
                    ("newer", Relation::Covers),
                    ("elsewhere", Relation::Diverged)
                ]
            ),
            Decision::Pending { run: Some(0) }
        );
    }

    #[test]
    fn a_spent_lookup_budget_leaves_the_gate_pending_for_this_poll() {
        let runs = vec![
            run(3, "newer", "completed", Some("success")),
            run(2, SHA, "queued", None),
        ];
        // `None` means the lookup could not be made now. Nothing is concluded from it.
        assert_eq!(judge(&runs, &[]), Decision::Pending { run: None });
    }

    #[test]
    fn a_rewatch_ignores_the_run_it_already_judged_but_not_its_rerun() {
        let floor = Some(RunFloor {
            run_id: 2,
            attempt: 1,
        });
        let failed = vec![run(2, SHA, "completed", Some("failure"))];
        let again = |runs: &[Run]| decide(&target(), floor, runs, |_| Ok(None)).unwrap();
        assert_eq!(again(&failed), Decision::Pending { run: None });

        let mut rerun = run(2, SHA, "in_progress", None);
        rerun.run_attempt = Some(2);
        assert_eq!(again(&[rerun.clone()]), Decision::Pending { run: Some(0) });
        rerun.status = Some("completed".into());
        rerun.conclusion = Some("success".into());
        assert_eq!(again(&[rerun]), Decision::Green { run: 0 });

        let fixed = vec![
            run(3, "fix", "completed", Some("success")),
            run(2, SHA, "completed", Some("failure")),
        ];
        let decision = decide(&target(), floor, &fixed, |_| Ok(Some(Relation::Covers))).unwrap();
        assert_eq!(decision, Decision::Green { run: 0 });
    }

    #[test]
    fn provider_text_is_cleaned_before_it_enters_a_record() {
        assert_eq!(clean("  build\n\tand test  ", 200), "build  and test");
        assert_eq!(clean("\u{7}\n", 200), "unnamed");
        assert_eq!(clean(&"x".repeat(500), 200).len(), 200);
        assert_eq!(query_value("release/1.x"), "release%2F1.x");
    }
}
