use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use super::contract::{MAX_MANIFEST_BYTES, bounded_text, valid_identifier, valid_token};
use super::shared_plan::SharedPlanStore;
use super::storage::Storage;
use super::*;

#[derive(Clone)]
pub struct ManifestStore {
    pub(crate) storage: Storage,
    /// HS2-SHH7WW: this machine's home and the owning checkout, so a change to which
    /// targets are sites of one project is checked against this machine's runs and
    /// bindings as well as the checked-in plan.
    machine: Option<(PathBuf, String)>,
}

impl ManifestStore {
    pub fn new(checkout_root: &Path) -> Self {
        Self {
            storage: Storage::shared(checkout_root),
            machine: None,
        }
    }

    /// HS2-SHH7WW: check grouping changes against this machine's records too.
    pub fn on_machine(mut self, home: &Path, owner_checkout_id: &str) -> Self {
        self.machine = Some((home.into(), owner_checkout_id.into()));
        self
    }

    pub fn read(&self) -> Result<ManifestView, OrchestrationError> {
        Ok(self.document()?.into())
    }

    /// HS2-SHH7WW: which targets are sites of one project. Empty when nothing is
    /// declared.
    pub fn sites(&self) -> Result<ProjectSites, OrchestrationError> {
        Ok(self
            .document()?
            .map(|document| ProjectSites::of(&document.targets))
            .unwrap_or_default())
    }

    pub(crate) fn document(&self) -> Result<Option<TargetManifest>, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_MANIFEST_BYTES)? else {
            return Ok(None);
        };
        let document = decode_manifest(&bytes)?;
        validate(&document).map_err(|_| OrchestrationError::Corrupt)?;
        Ok(Some(document))
    }

    /// Low-level transaction; the owning workflow also guards its registered source owner.
    pub fn replace(
        &self,
        request: ReplaceTargets,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        let _lock = self.storage.lock()?;
        self.replace_locked(request, tx)
    }

    pub(crate) fn replace_locked(
        &self,
        mut request: ReplaceTargets,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        let previous = self.document()?;
        if previous.as_ref().map(|doc| doc.revision.as_str())
            != request.expected_revision.as_deref()
        {
            return Err(OrchestrationError::Conflict);
        }
        for target in &mut request.targets {
            if let Some(hint) = target.expected_repository_identity.as_mut() {
                *hint = normalize_repository_hint(hint)?;
            }
        }
        validate_targets(&request.targets)?;
        validate_transition(previous.as_ref(), &request.targets)?;
        if previous
            .as_ref()
            .is_some_and(|doc| doc.targets == request.targets)
        {
            return Ok(previous.into());
        }
        validate_sites(&request.targets)?;
        let before = previous
            .as_ref()
            .map(|document| ProjectSites::of(&document.targets))
            .unwrap_or_default();
        let after = ProjectSites::of(&request.targets);
        if before != after {
            self.check_grouping(&after, &request.targets)?;
        }
        let revision = fresh_token(tx, previous.as_ref().map(|doc| doc.revision.as_str()))?;
        let document = TargetManifest {
            schema_version: manifest_version(&request.targets),
            revision,
            targets: request.targets,
        };
        validate(&document)?;
        let bytes = serde_json::to_vec(&document).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_MANIFEST_BYTES)?;
        Ok(Some(document).into())
    }

    pub fn add(
        &self,
        expected_revision: Option<String>,
        mut target: TargetDeclaration,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        let _lock = self.storage.lock()?;
        if let Some(hint) = target.expected_repository_identity.as_mut() {
            *hint = normalize_repository_hint(hint)?;
        }
        validate_targets(std::slice::from_ref(&target))?;
        let current = self.document()?;
        if current.as_ref().map(|d| d.revision.as_str()) != expected_revision.as_deref() {
            return Err(OrchestrationError::Conflict);
        }
        let mut targets = current.map(|d| d.targets).unwrap_or_default();
        if let Some(project) = &target.site_of {
            active_project(&targets, project)?;
        }
        if let Some(existing) = targets.iter().find(|t| t.target_id == target.target_id) {
            if existing.state == DeclarationState::Removed {
                return Err(OrchestrationError::Removed);
            }
            if existing != &target {
                return Err(OrchestrationError::Conflict);
            }
        } else {
            targets.push(target);
        }
        self.replace_locked(
            ReplaceTargets {
                expected_revision,
                targets,
            },
            tx,
        )
    }

    pub fn rename(
        &self,
        target_id: &str,
        request: RenameTarget,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        if request.expected_repository_identity.is_some()
            && request.clear_expected_repository_identity
        {
            return Err(OrchestrationError::Invalid(
                "set or clear repository hint, not both",
            ));
        }
        if request.site_of.is_some() && request.clear_site_of {
            return Err(OrchestrationError::Invalid(
                "set or clear the project a site belongs to, not both",
            ));
        }
        if let Some(project) = &request.site_of {
            valid_token(project)?;
        }
        let _lock = self.storage.lock()?;
        let current = self.document()?.ok_or(OrchestrationError::NotFound)?;
        if current.revision != request.expected_revision {
            return Err(OrchestrationError::Conflict);
        }
        let mut targets = current.targets;
        if let Some(project) = &request.site_of {
            active_project(&targets, project)?;
        }
        let target = targets
            .iter_mut()
            .find(|target| target.target_id == target_id)
            .ok_or(OrchestrationError::NotFound)?;
        if target.state == DeclarationState::Removed {
            return Err(OrchestrationError::Removed);
        }
        target.label = request.label;
        if let Some(project) = request.site_of {
            target.site_of = Some(project);
        } else if request.clear_site_of {
            target.site_of = None;
        }
        if let Some(hint) = request.expected_repository_identity {
            target.expected_repository_identity = Some(normalize_repository_hint(&hint)?);
        } else if request.clear_expected_repository_identity {
            target.expected_repository_identity = None;
        }
        self.replace_locked(
            ReplaceTargets {
                expected_revision: Some(request.expected_revision),
                targets,
            },
            tx,
        )
    }

    pub fn remove(
        &self,
        expected_revision: String,
        target_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        let _lock = self.storage.lock()?;
        let current = self.document()?.ok_or(OrchestrationError::NotFound)?;
        if current.revision != expected_revision {
            return Err(OrchestrationError::Conflict);
        }
        let mut targets = current.targets;
        targets
            .iter_mut()
            .find(|t| t.target_id == target_id)
            .ok_or(OrchestrationError::NotFound)?
            .state = DeclarationState::Removed;
        self.replace_locked(
            ReplaceTargets {
                expected_revision: Some(expected_revision),
                targets,
            },
            tx,
        )
    }
}

impl ManifestStore {
    /// HS2-SHH7WW: a change to which targets are sites of one project must leave every
    /// ticket of a project with at most one plan step and one active run, and every
    /// bound site of a project on the same ticket store. Existing records keep the
    /// site they name; they are read through the new grouping, never rewritten.
    ///
    /// HS2-RDBMVM: on a machine, the new grouping is joined by source identity with
    /// the sources this machine's bindings last confirmed, as the plan registry joins
    /// it (HS2-EPRWB6). A plan written before the ids were shared, naming one ticket
    /// through a site's own connection id and through the home's, is then two steps
    /// on one ticket and refused. Without a machine, ids compare as they are.
    fn check_grouping(
        &self,
        sites: &ProjectSites,
        targets: &[TargetDeclaration],
    ) -> Result<(), OrchestrationError> {
        let machine = match &self.machine {
            Some((home, owner)) => {
                valid_identifier(owner)?;
                Some((
                    home.as_path(),
                    owner.as_str(),
                    BindingRegistry::new(home).read_file()?,
                ))
            }
            None => None,
        };
        let joined;
        let sites = match &machine {
            Some((_, owner, file)) => {
                joined = sites.clone().with_sources(
                    file.bindings
                        .iter()
                        .filter(|binding| binding.target.owner_checkout_id == **owner)
                        .map(|binding| {
                            (
                                binding.target.target_id.as_str(),
                                binding
                                    .sources
                                    .iter()
                                    .map(|source| {
                                        (source.connection_id.as_str(), source.identity.as_str())
                                    })
                                    .collect(),
                            )
                        }),
                );
                &joined
            }
            None => sites,
        };
        let plan = SharedPlanStore::new(self.storage.anchor()).read()?;
        let mut steps = std::collections::BTreeSet::new();
        for step in plan.iter().flat_map(|plan| &plan.steps) {
            if !steps.insert(sites.ticket_key(&step.target)) {
                return Err(OrchestrationError::Site(SiteError::PlanConflict));
            }
        }
        let Some((home, owner, file)) = &machine else {
            return Ok(());
        };
        let mut active = std::collections::BTreeSet::new();
        for run in RunRegistry::new(home).list(owner, true)? {
            if !active.insert(sites.ticket_key(&run.target)) {
                return Err(OrchestrationError::Site(SiteError::RunConflict));
            }
        }
        let live = |id: &str| {
            targets
                .iter()
                .any(|target| target.target_id == id && target.state == DeclarationState::Active)
        };
        let mut stores: BTreeMap<&str, String> = BTreeMap::new();
        for binding in &file.bindings {
            let Some(destination) = &binding.destination else {
                continue;
            };
            if binding.target.owner_checkout_id != *owner || !live(&binding.target.target_id) {
                continue;
            }
            let authority = destination.authority();
            if *stores
                .entry(sites.project_of(&binding.target.target_id))
                .or_insert_with(|| authority.to_owned())
                != authority
            {
                return Err(OrchestrationError::Site(SiteError::StoreMismatch));
            }
        }
        Ok(())
    }
}

/// HS2-SHH7WW: the project a new site names must be an active target.
fn active_project(targets: &[TargetDeclaration], project: &str) -> Result<(), OrchestrationError> {
    valid_token(project)?;
    if targets
        .iter()
        .any(|target| target.target_id == project && target.state == DeclarationState::Active)
    {
        Ok(())
    } else {
        Err(OrchestrationError::Site(SiteError::ProjectUnknown))
    }
}

/// HS2-SHH7WW: version 1 while no target is a site, so an older binary keeps reading
/// an ungrouped manifest; version 2 once one is, which an older binary refuses as
/// `upgrade_required` before it could misread or overwrite it.
fn manifest_version(targets: &[TargetDeclaration]) -> u32 {
    if targets.iter().any(|target| target.site_of.is_some()) {
        MANIFEST_SITES_VERSION
    } else {
        ORCHESTRATION_VERSION
    }
}

/// The version is read before the strict field set, as every orchestration file does.
fn decode_manifest(bytes: &[u8]) -> Result<TargetManifest, OrchestrationError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| OrchestrationError::Corrupt)?;
    let found = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(OrchestrationError::Corrupt)?;
    if found != u64::from(ORCHESTRATION_VERSION) && found != u64::from(MANIFEST_SITES_VERSION) {
        return Err(OrchestrationError::UnsupportedVersion { found });
    }
    serde_json::from_value(value).map_err(|_| OrchestrationError::Corrupt)
}

fn validate(document: &TargetManifest) -> Result<(), OrchestrationError> {
    if document.schema_version != ORCHESTRATION_VERSION
        && document.schema_version != MANIFEST_SITES_VERSION
    {
        return Err(OrchestrationError::UnsupportedVersion {
            found: document.schema_version.into(),
        });
    }
    // Version 1 never carries a site, so an older reader never ignores one.
    if document.schema_version == ORCHESTRATION_VERSION
        && document
            .targets
            .iter()
            .any(|target| target.site_of.is_some())
    {
        return Err(OrchestrationError::Corrupt);
    }
    valid_token(&document.revision)?;
    validate_targets(&document.targets)
}

fn validate_targets(targets: &[TargetDeclaration]) -> Result<(), OrchestrationError> {
    if targets.len() > MAX_TARGETS {
        return Err(OrchestrationError::Capacity);
    }
    let mut ids = std::collections::HashSet::new();
    for target in targets {
        valid_token(&target.target_id)?;
        if !ids.insert(&target.target_id) {
            return Err(OrchestrationError::Invalid("duplicate target id"));
        }
        if !bounded_text(&target.label, 256) {
            return Err(OrchestrationError::Invalid("bounded target label required"));
        }
        if let Some(hint) = &target.expected_repository_identity {
            if normalize_repository_hint(hint)? != *hint {
                return Err(OrchestrationError::Invalid(
                    "repository hint must be normalized",
                ));
            }
        }
        if let Some(project) = &target.site_of {
            valid_token(project)?;
            if *project == target.target_id {
                return Err(OrchestrationError::Invalid(
                    "a target cannot be a site of itself",
                ));
            }
        }
    }
    Ok(())
}

fn validate_transition(
    previous: Option<&TargetManifest>,
    next: &[TargetDeclaration],
) -> Result<(), OrchestrationError> {
    let next_by_id: HashMap<_, _> = next
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect();
    let previous_by_id: HashMap<_, _> = previous
        .into_iter()
        .flat_map(|document| &document.targets)
        .map(|target| (target.target_id.as_str(), target))
        .collect();
    for (id, old) in &previous_by_id {
        let target = next_by_id.get(id).ok_or(OrchestrationError::Invalid(
            "existing target ids must be retained as tombstones",
        ))?;
        if old.state == DeclarationState::Removed && *target != *old {
            return Err(OrchestrationError::Removed);
        }
    }
    if next.iter().any(|target| {
        !previous_by_id.contains_key(target.target_id.as_str())
            && target.state != DeclarationState::Active
    }) {
        return Err(OrchestrationError::Invalid("new targets must be active"));
    }
    Ok(())
}

pub(crate) fn fresh_token(
    tx: &mut Transaction<'_>,
    previous: Option<&str>,
) -> Result<String, OrchestrationError> {
    for _ in 0..8 {
        let next = tx.token();
        valid_token(&next)?;
        if previous != Some(next.as_str()) {
            return Ok(next);
        }
    }
    Err(OrchestrationError::Conflict)
}
