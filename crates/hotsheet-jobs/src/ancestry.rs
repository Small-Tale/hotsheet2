//! HS2-56MTYN: repository ancestry for CI providers whose own API exposes none
//! (Buildkite, CircleCI). The provider names the repository it builds (a Buildkite
//! pipeline's `repository`, a CircleCI project's `vcs_info.vcs_url`); when that
//! repository is on GitHub or on the configured GitLab, its host answers whether a
//! later build's head contains the watched commit, as the GitHub Actions and GitLab CI
//! watches ask their own APIs:
//!
//! - GitHub: `GET repos/{owner}/{name}/compare/{commit}...{head}` and its `status`
//!   (`ahead` or `identical`: the head contains the commit; `behind`: the head is
//!   older; `diverged`; <https://docs.github.com/en/rest/commits/commits#compare-two-commits>).
//! - GitLab: `GET projects/{path}/repository/merge_base?refs[]={commit}&refs[]={head}`
//!   (<https://docs.gitlab.com/api/repositories/#merge-base>): a base equal to the
//!   commit means the head contains it, equal to the head means the head is older.
//!
//! A repository anywhere else (Bitbucket, GitHub Enterprise, another host) has no
//! host to ask, so its gate keeps exact coverage. Each answer is kept: ancestry never
//! changes. A commit the host does not know yet is not kept, since it can appear.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::github::{ApiMethod, ApiRequest, GithubApi, query_value, transient};
use crate::monitor::WatchError;

/// Ancestry lookups one poll of one watch may start; the rest wait for the next poll.
pub(crate) const MAX_RELATIONS_PER_POLL: usize = 5;

/// GitHub's host, the only one the GitHub port reaches.
const GITHUB_HOST: &str = "github.com";

/// A repository on a host that can answer ancestry.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HostRepository {
    /// `owner/name` on GitHub.
    Github(String),
    /// The full `group/[subgroup/…]project` path on the configured GitLab.
    Gitlab(String),
}

/// How a build's head relates to the watched commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Relation {
    /// The head is the commit or a descendant of it.
    Covers,
    /// The head is an ancestor of the commit: a build from before it.
    Before,
    /// Neither contains the other: the branch moved to another line.
    Diverged,
    /// The host does not know the commit (yet).
    Unknown,
}

/// The lowercased host and the path (no leading `/`, no `.git`) of a Git remote URL in
/// its usual spellings: `https://host/path`, `ssh://git@host:22/path` and the scp-like
/// `git@host:path`.
pub fn remote_host_path(url: &str) -> Option<(String, String)> {
    let trimmed = url.trim().trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let (authority, path) = match trimmed.split_once("://") {
        Some((_, rest)) => rest.split_once('/')?,
        None => trimmed.split_once(':')?,
    };
    let host = authority.rsplit('@').next()?;
    // A port in an ssh:// or https:// authority.
    let host = host.split(':').next()?.to_ascii_lowercase();
    let path = path.trim_start_matches('/');
    let valid = !host.is_empty()
        && path.split('/').count() >= 2
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != ".."
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        });
    valid.then(|| (host, path.to_owned()))
}

#[derive(Deserialize)]
struct Comparison {
    status: String,
}

#[derive(Deserialize)]
struct Commit {
    id: String,
}

/// The repository hosts a source may ask about ancestry. Without any, every gate of
/// the source keeps exact coverage.
#[derive(Default)]
pub struct RepositoryHosts {
    github: Option<Box<dyn GithubApi>>,
    /// The configured GitLab's host and its REST port.
    gitlab: Option<(String, Box<dyn GithubApi>)>,
    /// (repository, commit, head) → relation.
    relations: BTreeMap<(HostRepository, String, String), Relation>,
}

impl RepositoryHosts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask `github.com` through this REST port, rooted at `https://api.github.com`.
    #[must_use]
    pub fn with_github(mut self, api: impl GithubApi + 'static) -> Self {
        self.github = Some(Box::new(api));
        self
    }

    /// Ask the GitLab at `host` through this REST port, rooted at its `/api/v4`.
    #[must_use]
    pub fn with_gitlab(mut self, host: &str, api: impl GithubApi + 'static) -> Self {
        self.gitlab = Some((host.to_ascii_lowercase(), Box::new(api)));
        self
    }

    /// Whether any host can be asked.
    pub(crate) fn any(&self) -> bool {
        self.github.is_some() || self.gitlab.is_some()
    }

    /// The repository a remote URL names, when one of these hosts can answer for it.
    pub fn repository(&self, url: &str) -> Option<HostRepository> {
        let (host, path) = remote_host_path(url)?;
        if self.github.is_some() && host == GITHUB_HOST && path.split('/').count() == 2 {
            return Some(HostRepository::Github(path));
        }
        match &self.gitlab {
            Some((gitlab, _)) if *gitlab == host => Some(HostRepository::Gitlab(path)),
            _ => None,
        }
    }

    /// How `head` relates to `commit` in the repository. `None` when the poll's budget
    /// is spent or the head is not a commit id.
    pub(crate) fn relation(
        &mut self,
        repository: &HostRepository,
        commit: &str,
        head: &str,
        budget: &mut usize,
    ) -> Result<Option<Relation>, WatchError> {
        let commit = commit.to_ascii_lowercase();
        let head = head.to_ascii_lowercase();
        if head == commit {
            return Ok(Some(Relation::Covers));
        }
        let key = (repository.clone(), commit.clone(), head.clone());
        if let Some(known) = self.relations.get(&key) {
            return Ok(Some(*known));
        }
        if *budget == 0 || head.len() != 40 || !head.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(None);
        }
        let (api, path): (&mut Box<dyn GithubApi>, String) = match repository {
            HostRepository::Github(repo) => match self.github.as_mut() {
                Some(api) => (
                    api,
                    format!("repos/{repo}/compare/{commit}...{head}?per_page=1"),
                ),
                None => return Ok(None),
            },
            HostRepository::Gitlab(project) => match self.gitlab.as_mut() {
                Some((_, api)) => (
                    api,
                    format!(
                        "projects/{}/repository/merge_base?refs%5B%5D={commit}&refs%5B%5D={head}",
                        query_value(project)
                    ),
                ),
                None => return Ok(None),
            },
        };
        *budget -= 1;
        let request = ApiRequest {
            method: ApiMethod::Get,
            path,
            if_none_match: None,
            body: None,
        };
        let response = api
            .send(&request)
            .map_err(|unavailable| WatchError::Transient {
                retry_after_ms: unavailable.retry_after_ms,
            })?;
        let relation = match (repository, response.status) {
            (HostRepository::Github(_), 200) => {
                let comparison: Comparison =
                    serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                match comparison.status.as_str() {
                    "ahead" | "identical" => Relation::Covers,
                    "behind" => Relation::Before,
                    "diverged" => Relation::Diverged,
                    _ => return Err(transient(&response)),
                }
            }
            (HostRepository::Gitlab(_), 200) => {
                let base: Commit =
                    serde_json::from_str(&response.body).map_err(|_| transient(&response))?;
                let base = base.id.to_ascii_lowercase();
                if base == commit {
                    Relation::Covers
                } else if base == head {
                    Relation::Before
                } else {
                    Relation::Diverged
                }
            }
            // Not pushed yet, or not in this repository; GitLab answers 400 for an
            // unknown ref. Not kept: it can appear.
            (HostRepository::Github(_), 404) | (HostRepository::Gitlab(_), 400 | 404) => {
                return Ok(Some(Relation::Unknown));
            }
            // 401, 403 (the host's credentials), 429 and server errors are retried.
            _ => return Err(transient(&response)),
        };
        self.relations.insert(key, relation);
        Ok(Some(relation))
    }
}

/// Which builds cover the commit, from build heads listed newest first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Covering {
    /// Indexes of the covering builds, newest first.
    pub covering: Vec<usize>,
    /// The newest build is on another line and nothing newer covers the commit.
    pub diverged_first: bool,
    /// A lookup was left for the next poll: the verdict is not final.
    pub unfinished: bool,
}

/// Walk build heads newest first, as the GitHub Actions watch does: a head containing
/// the commit covers it; a head on another line is passed over; a head older than the
/// commit, one the host does not know, or a spent budget ends the walk. The commit's
/// own build is the oldest that can cover it.
pub(crate) fn walk(
    commit: &str,
    heads: &[Option<&str>],
    mut relation: impl FnMut(&str) -> Result<Option<Relation>, WatchError>,
) -> Result<Covering, WatchError> {
    let mut found = Covering::default();
    for (index, head) in heads.iter().enumerate() {
        let Some(head) = head else {
            continue;
        };
        let own = head.eq_ignore_ascii_case(commit);
        let known = if own {
            Relation::Covers
        } else {
            match relation(head)? {
                Some(known) => known,
                None => {
                    // Something newer may still cover the commit: not superseded yet.
                    found.unfinished = true;
                    found.diverged_first = false;
                    break;
                }
            }
        };
        match known {
            Relation::Covers => found.covering.push(index),
            Relation::Diverged => {
                found.diverged_first |= found.covering.is_empty() && index == 0;
                continue;
            }
            Relation::Before | Relation::Unknown => break,
        }
        if own {
            break;
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{ApiResponse, ApiUnavailable};
    use std::sync::{Arc, Mutex};

    const COMMIT: &str = "1111111111111111111111111111111111111111";
    const LATER: &str = "2222222222222222222222222222222222222222";
    const EARLIER: &str = "3333333333333333333333333333333333333333";
    const OTHER: &str = "4444444444444444444444444444444444444444";

    #[test]
    fn hs2_56mtyn_remote_urls_name_their_host_and_path_in_every_spelling() {
        for (url, host, path) in [
            (
                "https://github.com/acme/widgets.git",
                "github.com",
                "acme/widgets",
            ),
            (
                "https://github.com/acme/widgets/",
                "github.com",
                "acme/widgets",
            ),
            (
                "git@github.com:acme/widgets.git",
                "github.com",
                "acme/widgets",
            ),
            (
                "ssh://git@GitHub.com:22/acme/widgets",
                "github.com",
                "acme/widgets",
            ),
            (
                "https://gitlab.example.test/group/sub/project.git",
                "gitlab.example.test",
                "group/sub/project",
            ),
            ("https://token@gitlab.com/g/p", "gitlab.com", "g/p"),
        ] {
            assert_eq!(
                remote_host_path(url),
                Some((host.to_owned(), path.to_owned())),
                "{url}"
            );
        }
        for url in [
            "",
            "github.com",
            "https://github.com/acme",
            "https://github.com//widgets",
            "https://github.com/acme/../widgets",
            "https://github.com/acme/wid gets",
            "/local/path/repo",
        ] {
            assert_eq!(remote_host_path(url), None, "{url}");
        }
    }

    #[derive(Clone, Default)]
    struct Port(
        Arc<Mutex<Vec<String>>>,
        Arc<Mutex<BTreeMap<String, (u16, String)>>>,
    );

    impl Port {
        fn serve(&self, path: &str, status: u16, body: &str) {
            self.1
                .lock()
                .unwrap()
                .insert(path.to_owned(), (status, body.to_owned()));
        }
    }

    impl GithubApi for Port {
        fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
            self.0.lock().unwrap().push(request.path.clone());
            let (status, body) = self
                .1
                .lock()
                .unwrap()
                .get(&request.path)
                .cloned()
                .unwrap_or((404, "{}".into()));
            Ok(ApiResponse {
                status,
                etag: None,
                retry_after_ms: None,
                body,
            })
        }
    }

    #[test]
    fn hs2_56mtyn_only_github_com_and_the_configured_gitlab_answer_for_a_repository() {
        let none = RepositoryHosts::new();
        assert!(!none.any());
        assert_eq!(none.repository("https://github.com/acme/widgets"), None);
        let hosts = RepositoryHosts::new()
            .with_github(Port::default())
            .with_gitlab("GitLab.Example.Test", Port::default());
        assert!(hosts.any());
        assert_eq!(
            hosts.repository("git@github.com:acme/widgets.git"),
            Some(HostRepository::Github("acme/widgets".into()))
        );
        assert_eq!(
            hosts.repository("https://gitlab.example.test/group/sub/project"),
            Some(HostRepository::Gitlab("group/sub/project".into()))
        );
        for url in [
            // GitHub has no nested paths; Enterprise and Bitbucket have no port here.
            "https://github.com/acme/widgets/extra",
            "https://github.example.test/acme/widgets",
            "https://bitbucket.org/acme/widgets",
            "https://gitlab.com/group/project",
        ] {
            assert_eq!(hosts.repository(url), None, "{url}");
        }
    }

    #[test]
    fn hs2_56mtyn_github_compare_and_gitlab_merge_base_map_to_one_relation_each() {
        let github = Port::default();
        let gitlab = Port::default();
        let mut hosts = RepositoryHosts::new()
            .with_github(github.clone())
            .with_gitlab("gitlab.example.test", gitlab.clone());
        let repo = HostRepository::Github("acme/widgets".into());
        let compare =
            |head: &str| format!("repos/acme/widgets/compare/{COMMIT}...{head}?per_page=1");
        github.serve(&compare(LATER), 200, r#"{"status":"ahead"}"#);
        github.serve(&compare(EARLIER), 200, r#"{"status":"behind"}"#);
        github.serve(&compare(OTHER), 200, r#"{"status":"diverged"}"#);
        let mut budget = 10;
        for (head, expected) in [
            (LATER, Relation::Covers),
            (EARLIER, Relation::Before),
            (OTHER, Relation::Diverged),
            (COMMIT, Relation::Covers),
        ] {
            assert_eq!(
                hosts.relation(&repo, COMMIT, head, &mut budget).unwrap(),
                Some(expected),
                "{head}"
            );
        }
        // The commit itself needs no lookup; known answers are kept.
        assert_eq!(budget, 7);
        hosts.relation(&repo, COMMIT, LATER, &mut budget).unwrap();
        assert_eq!(budget, 7);
        assert_eq!(github.0.lock().unwrap().len(), 3);

        // An unknown status and server errors are retried; an unknown commit is not kept.
        let unknown = "5555555555555555555555555555555555555555";
        github.serve(&compare(unknown), 200, r#"{"status":"something-new"}"#);
        assert!(hosts.relation(&repo, COMMIT, unknown, &mut budget).is_err());
        for status in [401, 403, 429, 500] {
            github.serve(&compare(unknown), status, "{}");
            assert!(hosts.relation(&repo, COMMIT, unknown, &mut budget).is_err());
        }
        github.serve(&compare(unknown), 404, "{}");
        assert_eq!(
            hosts.relation(&repo, COMMIT, unknown, &mut budget).unwrap(),
            Some(Relation::Unknown)
        );
        let before = budget;
        hosts.relation(&repo, COMMIT, unknown, &mut budget).unwrap();
        assert_eq!(budget, before - 1, "an unknown commit is asked again");

        // A spent budget, or a head that is not a commit id, asks nothing.
        let mut spent = 0;
        assert_eq!(
            hosts.relation(&repo, COMMIT, OTHER, &mut spent).unwrap(),
            Some(Relation::Diverged)
        );
        assert_eq!(
            hosts
                .relation(
                    &repo,
                    COMMIT,
                    "6666666666666666666666666666666666666666",
                    &mut spent
                )
                .unwrap(),
            None
        );
        assert_eq!(
            hosts.relation(&repo, COMMIT, "HEAD", &mut budget).unwrap(),
            None
        );

        // GitLab: the merge base decides.
        let project = HostRepository::Gitlab("group/sub/project".into());
        let base = |head: &str| {
            format!(
                "projects/group%2Fsub%2Fproject/repository/merge_base?refs%5B%5D={COMMIT}&refs%5B%5D={head}"
            )
        };
        gitlab.serve(&base(LATER), 200, &format!(r#"{{"id":"{COMMIT}"}}"#));
        gitlab.serve(&base(EARLIER), 200, &format!(r#"{{"id":"{EARLIER}"}}"#));
        gitlab.serve(&base(OTHER), 200, &format!(r#"{{"id":"{unknown}"}}"#));
        gitlab.serve(&base(unknown), 400, r#"{"message":"404 Commit Not Found"}"#);
        let mut budget = 10;
        for (head, expected) in [
            (LATER, Relation::Covers),
            (EARLIER, Relation::Before),
            (OTHER, Relation::Diverged),
            (unknown, Relation::Unknown),
        ] {
            assert_eq!(
                hosts.relation(&project, COMMIT, head, &mut budget).unwrap(),
                Some(expected),
                "{head}"
            );
        }
        // Without the host's port nothing is asked.
        let mut github_only = RepositoryHosts::new().with_github(Port::default());
        assert_eq!(
            github_only
                .relation(&project, COMMIT, LATER, &mut budget)
                .unwrap(),
            None
        );
    }

    #[test]
    fn hs2_56mtyn_the_walk_covers_newer_heads_passes_other_lines_and_stops_before_the_commit() {
        let relations = BTreeMap::from([
            (LATER, Relation::Covers),
            (EARLIER, Relation::Before),
            (OTHER, Relation::Diverged),
        ]);
        let ask = |head: &str| Ok(relations.get(head).copied());
        // Newest first: a later head, another line, the commit, then older builds.
        let found = walk(
            COMMIT,
            &[
                Some(LATER),
                Some(OTHER),
                None,
                Some(COMMIT),
                Some(LATER),
                Some(EARLIER),
            ],
            ask,
        )
        .unwrap();
        assert_eq!(found.covering, [0, 3]);
        assert!(!found.diverged_first && !found.unfinished);
        // The branch moved to another line: superseded unless something covers.
        let found = walk(COMMIT, &[Some(OTHER), Some(EARLIER)], ask).unwrap();
        assert!(found.diverged_first && found.covering.is_empty());
        let found = walk(COMMIT, &[Some(OTHER), Some(COMMIT)], ask).unwrap();
        assert_eq!(found.covering, [1]);
        // An older head ends the walk; an unknown or unasked head leaves it unfinished.
        assert_eq!(
            walk(COMMIT, &[Some(EARLIER), Some(COMMIT)], ask)
                .unwrap()
                .covering,
            Vec::<usize>::new()
        );
        let unasked = walk(
            COMMIT,
            &[
                Some(LATER),
                Some("7777777777777777777777777777777777777777"),
            ],
            ask,
        )
        .unwrap();
        assert_eq!(unasked.covering, [0]);
        assert!(unasked.unfinished);
        // Another line first, then a head left for the next poll: not superseded yet.
        let pending = walk(
            COMMIT,
            &[
                Some(OTHER),
                Some("7777777777777777777777777777777777777777"),
            ],
            ask,
        )
        .unwrap();
        assert!(pending.unfinished && !pending.diverged_first);
        assert_eq!(walk(COMMIT, &[], ask).unwrap(), Covering::default());
    }
}
