//! HS2-SHH7WW: several targets declared as sites of one orchestration project.
//!
//! A target names the project it is another site of with `site_of`; the project's own
//! target is its first site. Every site of a project reaches the same ticket store, so
//! one ticket of the project has one plan step and one active run whichever site it
//! is reached through. Records keep the concrete site they name; comparisons go
//! through [`ProjectSites`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{DeclarationState, TargetDeclaration, TargetDestination, TargetFailure};
use crate::orchestration::{HostCapabilities, RunTarget, TicketKey};

/// Why a site of a project was refused. Content-free codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteError {
    /// `site_of` names no active target of this manifest.
    ProjectUnknown,
    /// `site_of` names a target that is itself a site, or the target already has sites.
    Chain,
    /// A project target cannot be removed while another site of it is active.
    ProjectHasSites,
    /// The site declares a different normalized repository identity than its project.
    RepositoryMismatch,
    /// The site is bound on this machine to another ticket store than a site of its
    /// project.
    StoreMismatch,
    /// The checked-in plan has two steps on one ticket of the project.
    PlanConflict,
    /// This machine has two active runs on one ticket of the project.
    RunConflict,
}

impl SiteError {
    pub const ALL: [Self; 7] = [
        Self::ProjectUnknown,
        Self::Chain,
        Self::ProjectHasSites,
        Self::RepositoryMismatch,
        Self::StoreMismatch,
        Self::PlanConflict,
        Self::RunConflict,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::ProjectUnknown => "site_project_unknown",
            Self::Chain => "site_chain",
            Self::ProjectHasSites => "site_project_has_sites",
            Self::RepositoryMismatch => "site_repository_mismatch",
            Self::StoreMismatch => "site_store_mismatch",
            Self::PlanConflict => "site_plan_conflict",
            Self::RunConflict => "site_run_conflict",
        }
    }
}

/// Which project each target belongs to. A target without `site_of` is its own
/// project, so an empty map treats every target as a one-site project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSites {
    project_of: BTreeMap<String, String>,
    /// HS2-3JGPDR: a site's connection id for a source mapped to the home site's id
    /// for the same source. A Git source's connection id is a hash of its local path, so
    /// the one store every site shares has another id on each machine. Empty without
    /// [`Self::with_sources`], when every site's ids are compared as they are.
    connections: BTreeMap<(String, String), String>,
}

impl ProjectSites {
    /// The grouping the declarations describe, removed ones included so that their
    /// history still counts for their project.
    ///
    /// HS2-NAA3HV: a removed site keeps the `site_of` it had, so after a re-home it can
    /// name the old home, now a site of the new one. Each target's project follows
    /// `site_of` to the end of that chain, so its history still counts for the project.
    pub fn of(targets: &[TargetDeclaration]) -> Self {
        let direct: BTreeMap<&str, &str> = targets
            .iter()
            .filter_map(|target| {
                target
                    .site_of
                    .as_deref()
                    .map(|project| (target.target_id.as_str(), project))
            })
            .collect();
        let resolve = |start: &str| {
            let mut project = start;
            // Bounded: a merged manifest may hold a cycle, which ends where it repeats.
            for _ in 0..direct.len() {
                match direct.get(project) {
                    Some(next) if *next != start => project = next,
                    _ => break,
                }
            }
            project.to_owned()
        };
        Self {
            project_of: direct
                .keys()
                .map(|site| ((*site).to_owned(), resolve(site)))
                .collect(),
            connections: BTreeMap::new(),
        }
    }

    /// HS2-3JGPDR: the same grouping, with each site's sources joined to its home's by
    /// source identity. `bound` lists, per bound target, its sources as
    /// `(connection_id, identity)` the way this machine last confirmed them. A source
    /// the home is not bound to, or does not have, keeps its own connection id.
    #[must_use]
    pub fn with_sources<'a>(
        mut self,
        bound: impl IntoIterator<Item = (&'a str, Vec<(&'a str, &'a str)>)>,
    ) -> Self {
        let bound: BTreeMap<&str, Vec<(&str, &str)>> = bound.into_iter().collect();
        let mut connections = BTreeMap::new();
        for (site, sources) in &bound {
            let home = self.home_of(site);
            if home == *site {
                continue;
            }
            let Some(home_sources) = bound.get(home) else {
                continue;
            };
            for (connection, identity) in sources {
                if let Some((home_connection, _)) =
                    home_sources.iter().find(|(_, other)| other == identity)
                    && home_connection != connection
                {
                    connections.insert(
                        ((*site).to_owned(), (*connection).to_owned()),
                        (*home_connection).to_owned(),
                    );
                }
            }
        }
        self.connections = connections;
        self
    }

    /// HS2-3JGPDR: the home site's connection id for `connection` as `site` names it.
    pub fn home_connection<'a>(&'a self, site: &str, connection: &'a str) -> &'a str {
        self.connections
            .get(&(site.to_owned(), connection.to_owned()))
            .map_or(connection, String::as_str)
    }

    /// HS2-3JGPDR: the connection id `to_site` has for the source `from_site` names
    /// `connection`. Unchanged when `to_site` is not known to have it under another id.
    pub fn connection_on(&self, from_site: &str, connection: &str, to_site: &str) -> String {
        let home = self.home_connection(from_site, connection);
        self.connections
            .iter()
            .find(|((site, _), mapped)| site == to_site && mapped.as_str() == home)
            .map_or_else(|| home.to_owned(), |((_, own), _)| own.clone())
    }

    /// HS2-EPRWB6: `connection` as the checked-in plan names it: the home site's id for
    /// the source `site` names `connection`. The home's id is the one its bound
    /// checkout reports, the same on every machine that binds the home, so one ticket
    /// is written alike whichever site and machine defined it.
    pub fn shared_connection(&self, site: &str, connection: &str) -> String {
        self.home_connection(site, connection).to_owned()
    }

    /// HS2-EPRWB6: a connection id read from the checked-in plan as this machine names
    /// it for `site`: the site's own id for the home's source. Unchanged when this
    /// machine does not know the site under another id, so a plan written before the
    /// ids were shared reads as it was.
    pub fn local_connection(&self, site: &str, connection: &str) -> String {
        self.connection_on(site, connection, site)
    }

    /// HS2-9FKTB3: the grouping of the active declarations only, the sites a step may
    /// be placed on now. A removed site is not tried again.
    pub fn active(targets: &[TargetDeclaration]) -> Self {
        let active: Vec<TargetDeclaration> = targets
            .iter()
            .filter(|target| target.state == DeclarationState::Active)
            .cloned()
            .collect();
        Self::of(&active)
    }

    /// HS2-9FKTB3: the other sites of `target_id`'s project, by id. Empty for a
    /// one-site project.
    pub fn other_sites(&self, target_id: &str) -> Vec<String> {
        let mut others: Vec<String> = self
            .sites_of(self.project_of(target_id))
            .into_iter()
            .filter(|site| site != target_id)
            .collect();
        others.sort();
        others
    }

    pub fn is_empty(&self) -> bool {
        self.project_of.is_empty()
    }

    /// HS2-EX594V: every home of a project with more than one site, sorted.
    pub fn homes_with_sites(&self) -> Vec<String> {
        let mut homes: Vec<String> = self
            .project_of
            .iter()
            .filter(|(site, project)| site != project)
            .map(|(_, project)| project.clone())
            .collect();
        homes.sort();
        homes.dedup();
        homes
    }

    /// The project key of a target: the project target's id, or its own.
    pub fn project_of<'a>(&'a self, target_id: &'a str) -> &'a str {
        self.project_of
            .get(target_id)
            .map_or(target_id, String::as_str)
    }

    /// HS2-5P112Z: the home site of `target_id`'s project, the one site whose
    /// coordination pool assigns and integrates the whole project's work: the
    /// project's own target. There is no fallback to another site while the home is
    /// offline, since a second home would be a second pool and a second assignment
    /// authority for the same tickets.
    pub fn home_of<'a>(&'a self, target_id: &'a str) -> &'a str {
        self.project_of(target_id)
    }

    /// HS2-5P112Z: whether the project of `target_id` has a site other than its home.
    pub fn has_sites(&self, target_id: &str) -> bool {
        let home = self.home_of(target_id);
        self.project_of
            .iter()
            .any(|(site, project)| project == home && site != home)
    }

    /// HS2-5P112Z: the same grouping with `site` declared a site of `project`, for a
    /// caller that holds no declarations (the pure scheduler's tests).
    #[must_use]
    pub fn with_site(mut self, site: &str, project: &str) -> Self {
        self.project_of.insert(site.to_owned(), project.to_owned());
        self
    }

    pub fn same_project(&self, left: &str, right: &str) -> bool {
        self.project_of(left) == self.project_of(right)
    }

    /// The qualified ticket with its target replaced by the project key.
    pub fn ticket_key(&self, target: &RunTarget) -> TicketKey {
        (
            self.project_of(&target.target_id).to_owned(),
            self.home_connection(&target.target_id, &target.connection_id)
                .to_owned(),
            target.native_id.clone(),
        )
    }

    /// The same key for a key already built from a concrete site.
    pub fn canonical(&self, key: &TicketKey) -> TicketKey {
        (
            self.project_of(&key.0).to_owned(),
            self.home_connection(&key.0, &key.1).to_owned(),
            key.2.clone(),
        )
    }

    /// One ticket of the project, through whichever sites the two name.
    pub fn same_ticket(&self, left: &RunTarget, right: &RunTarget) -> bool {
        self.home_connection(&left.target_id, &left.connection_id)
            == self.home_connection(&right.target_id, &right.connection_id)
            && left.native_id == right.native_id
            && self.same_project(&left.target_id, &right.target_id)
    }

    /// Every target of the project `project_id`: the project target first, then its
    /// other sites by id.
    pub fn sites_of(&self, project_id: &str) -> Vec<String> {
        std::iter::once(project_id.to_owned())
            .chain(
                self.project_of
                    .iter()
                    .filter(|(site, project)| *project == project_id && *site != project_id)
                    .map(|(site, _)| site.clone()),
            )
            .collect()
    }
}

/// One site as this machine knows it: its binding, never a fresh probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteView {
    pub target_id: String,
    pub label: String,
    /// True for the project's own target.
    pub project: bool,
    /// Where this machine binds the site, when it does.
    pub destination: Option<TargetDestination>,
    pub confirmed_at_ms: Option<u64>,
    pub last_failure: Option<TargetFailure>,
    pub stale: bool,
    /// HS2-D4G81Q: what the site's machine is (OS, architecture, CPU, GPU, AI tool and
    /// toolchain versions), read through the gated capacity read only when the sites
    /// view is asked for capabilities. Absent otherwise, and when the machine did not
    /// report them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<HostCapabilities>,
    /// HS2-D4G81Q: the error code of a capabilities read that failed, such as
    /// `target_unavailable`; absent when none was read or the read succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities_error: Option<String>,
}

/// One project with its active sites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteProject {
    pub project_id: String,
    pub label: String,
    pub sites: Vec<SiteView>,
}

/// `GET /checkouts/{owner}/orchestration/sites`: every active project of the manifest,
/// one-site projects included, ordered by label then id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitesView {
    pub revision: Option<String>,
    pub projects: Vec<SiteProject>,
}

/// Static checks of the grouping a manifest write would produce. Reads stay lenient
/// so a grouping merged from another machine is reported rather than unreadable.
pub(crate) fn validate_sites(
    targets: &[TargetDeclaration],
) -> Result<(), crate::orchestration::OrchestrationError> {
    use crate::orchestration::OrchestrationError;
    let by_id: BTreeMap<&str, &TargetDeclaration> = targets
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect();
    for target in targets {
        let Some(project_id) = target.site_of.as_deref() else {
            continue;
        };
        super::valid_token(project_id)?;
        if project_id == target.target_id {
            return Err(OrchestrationError::Invalid(
                "a target cannot be a site of itself",
            ));
        }
        if target.state != DeclarationState::Active {
            continue;
        }
        let project = by_id
            .get(project_id)
            .ok_or(OrchestrationError::Site(SiteError::ProjectUnknown))?;
        if project.site_of.is_some()
            || targets.iter().any(|other| {
                other.site_of.as_deref() == Some(target.target_id.as_str())
                    && other.state == DeclarationState::Active
            })
        {
            return Err(OrchestrationError::Site(SiteError::Chain));
        }
        if project.state != DeclarationState::Active {
            return Err(OrchestrationError::Site(SiteError::ProjectHasSites));
        }
    }
    // Every declared repository identity of one project's active sites is the same.
    let mut hints: BTreeMap<&str, &str> = BTreeMap::new();
    for target in targets
        .iter()
        .filter(|target| target.state == DeclarationState::Active)
    {
        if let Some(hint) = target.expected_repository_identity.as_deref()
            && *hints.entry(target.project_id()).or_insert(hint) != hint
        {
            return Err(OrchestrationError::Site(SiteError::RepositoryMismatch));
        }
    }
    Ok(())
}
