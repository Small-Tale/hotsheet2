use std::path::Path;

use super::bindings::{BindingFile, BindingRecord, MAX_BINDINGS, check_expected, same_snapshot};
use super::manifest::fresh_token;
use super::*;

/// HS2-S5RPAR: the Git ticket stores a site reaches, by provider-owned store identity.
/// Sites of one project share their ticket store, not every source: a checkout may also
/// link an external provider (GitHub Issues, Jira) that its other machine does not.
pub(crate) fn git_store_identities<'a>(
    sources: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> std::collections::BTreeSet<String> {
    sources
        .into_iter()
        .filter(|(provider, _)| *provider == "git")
        .filter_map(|(_, identity)| identity.map(str::to_owned))
        .collect()
}

pub(crate) fn binding_git_stores(binding: &BindingRecord) -> std::collections::BTreeSet<String> {
    git_store_identities(
        binding
            .sources
            .iter()
            .map(|source| (source.provider.as_str(), Some(source.identity.as_str()))),
    )
}

/// Two sites reach the same ticket store when they share a Git store identity. Without
/// Git identities on both sides, fall back to the whole-checkout authority (HS2-SHH7WW).
pub(crate) fn same_ticket_store(
    left_authority: &str,
    left_git: &std::collections::BTreeSet<String>,
    right_authority: &str,
    right_git: &std::collections::BTreeSet<String>,
) -> bool {
    if left_git.is_empty() || right_git.is_empty() {
        left_authority == right_authority
    } else {
        !left_git.is_disjoint(right_git)
    }
}

/// A trusted adapter's observation, not a deserializable HTTP/MCP proof.
pub struct ObservedTarget<P> {
    proof: P,
    result: Result<CheckoutProjection, TargetFailure>,
}

impl<P> ObservedTarget<P> {
    pub fn new(proof: P, result: Result<CheckoutProjection, TargetFailure>) -> Self {
        Self { proof, result }
    }
}

/// Network/provider adapters observe outside locks. Guard methods perform only local
/// revalidation and keep their guards held until the callback has completely returned.
/// Required order: checkout -> provider -> manifest -> bindings -> remote registry.
/// The remote guard fences LOCAL registry changes; peer data is as-of its bounded
/// authenticated response, not a distributed atomic transaction or delegated grant.
pub trait TargetObserver {
    type Proof;

    fn observe(
        &self,
        target: &TargetRef,
        destination: &TargetDestination,
    ) -> Result<ObservedTarget<Self::Proof>, OrchestrationError>;

    fn with_source_guard<T>(
        &self,
        owner_checkout_id: &str,
        checkout_root: &Path,
        proof: Option<&Self::Proof>,
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError>;

    fn with_remote_guard<T>(
        &self,
        proof: &Self::Proof,
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError>;
}

pub struct Orchestration {
    owner_checkout_id: String,
    manifest: ManifestStore,
    bindings: BindingRegistry,
}

impl Orchestration {
    pub fn new(
        owner_checkout_id: String,
        manifest: ManifestStore,
        bindings: BindingRegistry,
    ) -> Result<Self, OrchestrationError> {
        super::contract::valid_identifier(&owner_checkout_id)?;
        // HS2-SHH7WW: grouping changes are checked against this machine's records.
        let manifest = manifest.on_machine(bindings.storage.anchor(), &owner_checkout_id);
        Ok(Self {
            owner_checkout_id,
            manifest,
            bindings,
        })
    }

    /// HS2-SHH7WW: every active project with its active sites and this machine's
    /// binding of each. Reads the manifest and the binding file; contacts nothing.
    pub fn sites(&self) -> Result<SitesView, OrchestrationError> {
        let Some(manifest) = self.manifest.document()? else {
            return Ok(SitesView {
                revision: None,
                projects: Vec::new(),
            });
        };
        let file = self.bindings.read_file()?;
        let active: Vec<&TargetDeclaration> = manifest
            .targets
            .iter()
            .filter(|target| target.state == DeclarationState::Active)
            .collect();
        let site = |target: &TargetDeclaration| {
            let binding = file.bindings.iter().find(|binding| {
                binding.target.owner_checkout_id == self.owner_checkout_id
                    && binding.target.target_id == target.target_id
            });
            SiteView {
                target_id: target.target_id.clone(),
                label: target.label.clone(),
                project: target.site_of.is_none(),
                destination: binding.and_then(|binding| binding.destination.clone()),
                confirmed_at_ms: binding.and_then(|binding| binding.confirmed_at_ms),
                last_failure: binding.and_then(|binding| binding.last_failure),
                stale: true,
                capabilities: None,
                capabilities_error: None,
            }
        };
        let mut projects: Vec<SiteProject> = active
            .iter()
            .filter(|target| target.site_of.is_none())
            .map(|project| {
                let mut sites = vec![site(project)];
                let mut others: Vec<SiteView> = active
                    .iter()
                    .filter(|target| target.site_of.as_deref() == Some(&project.target_id))
                    .map(|target| site(target))
                    .collect();
                others.sort_by(|a, b| (&a.label, &a.target_id).cmp(&(&b.label, &b.target_id)));
                sites.extend(others);
                SiteProject {
                    project_id: project.target_id.clone(),
                    label: project.label.clone(),
                    sites,
                }
            })
            .collect();
        projects.sort_by(|a, b| (&a.label, &a.project_id).cmp(&(&b.label, &b.project_id)));
        Ok(SitesView {
            revision: Some(manifest.revision),
            projects,
        })
    }

    /// HS2-SHH7WW: every other bound site of the target's project on this machine
    /// must reach the same ticket store as `destination`.
    fn check_site_store(
        &self,
        manifest: &TargetManifest,
        file: &super::bindings::BindingFile,
        target: &TargetRef,
        destination: &TargetDestination,
        sources: Option<&[TargetSource]>,
    ) -> Result<(), OrchestrationError> {
        // Before the probe the candidate's store identities are unknown; the check runs
        // again with the observed sources before anything is bound (HS2-S5RPAR).
        let Some(sources) = sources else {
            return Ok(());
        };
        let candidate = git_store_identities(
            sources
                .iter()
                .map(|source| (source.provider.as_str(), source.identity.as_deref())),
        );
        let sites = ProjectSites::of(&manifest.targets);
        let live = |id: &str| {
            manifest
                .targets
                .iter()
                .any(|entry| entry.target_id == id && entry.state == DeclarationState::Active)
        };
        let mismatch = file.bindings.iter().any(|binding| {
            binding.target.owner_checkout_id == self.owner_checkout_id
                && binding.target.target_id != target.target_id
                && live(&binding.target.target_id)
                && sites.same_project(&binding.target.target_id, &target.target_id)
                && binding.destination.as_ref().is_some_and(|other| {
                    !same_ticket_store(
                        other.authority(),
                        &binding_git_stores(binding),
                        destination.authority(),
                        &candidate,
                    )
                })
        });
        if mismatch {
            Err(OrchestrationError::Site(SiteError::StoreMismatch))
        } else {
            Ok(())
        }
    }

    /// HS2-E9VVQF: before declaring a new site of `project`, refuse a destination that
    /// reaches another ticket store than a bound site of that project on this machine,
    /// so a refused add leaves no declaration behind. Binding checks it again.
    pub fn check_site_candidate(
        &self,
        project: &str,
        destination: &TargetDestination,
        sources: &[TargetSource],
    ) -> Result<(), OrchestrationError> {
        let candidate = git_store_identities(
            sources
                .iter()
                .map(|source| (source.provider.as_str(), source.identity.as_deref())),
        );
        let Some(manifest) = self.manifest.document()? else {
            return Err(OrchestrationError::Site(SiteError::ProjectUnknown));
        };
        let sites = ProjectSites::of(&manifest.targets);
        let file = self.bindings.read_file()?;
        let live = |id: &str| {
            manifest
                .targets
                .iter()
                .any(|entry| entry.target_id == id && entry.state == DeclarationState::Active)
        };
        let mismatch = file.bindings.iter().any(|binding| {
            binding.target.owner_checkout_id == self.owner_checkout_id
                && live(&binding.target.target_id)
                && sites.project_of(&binding.target.target_id) == project
                && binding.destination.as_ref().is_some_and(|other| {
                    !same_ticket_store(
                        other.authority(),
                        &binding_git_stores(binding),
                        destination.authority(),
                        &candidate,
                    )
                })
        });
        if mismatch {
            Err(OrchestrationError::Site(SiteError::StoreMismatch))
        } else {
            Ok(())
        }
    }

    pub fn declarations(&self) -> Result<ManifestView, OrchestrationError> {
        self.manifest.read()
    }

    pub fn binding(&self, target: &TargetRef) -> Result<BindingView, OrchestrationError> {
        self.check_owner(target)?;
        self.bindings.get(target)
    }

    /// Shared mutation always runs inside the source-owner -> manifest lock subset.
    pub fn replace<O: TargetObserver>(
        &self,
        request: ReplaceTargets,
        observer: &O,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        observer.with_source_guard(
            &self.owner_checkout_id,
            self.manifest.storage.anchor(),
            None,
            || self.manifest.replace(request, tx),
        )
    }

    pub fn add<O: TargetObserver>(
        &self,
        request: AddTarget,
        observer: &O,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        let target = TargetDeclaration {
            target_id: request.target_id.unwrap_or_else(|| tx.token()),
            state: DeclarationState::Active,
            label: request.label,
            expected_repository_identity: request.expected_repository_identity,
            site_of: request.site_of,
        };
        observer.with_source_guard(
            &self.owner_checkout_id,
            self.manifest.storage.anchor(),
            None,
            || self.manifest.add(request.expected_revision, target, tx),
        )
    }

    pub fn rename<O: TargetObserver>(
        &self,
        target: &TargetRef,
        request: RenameTarget,
        observer: &O,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        self.check_owner(target)?;
        observer.with_source_guard(
            &self.owner_checkout_id,
            self.manifest.storage.anchor(),
            None,
            || self.manifest.rename(&target.target_id, request, tx),
        )
    }

    pub fn remove<O: TargetObserver>(
        &self,
        target: &TargetRef,
        request: RemoveTarget,
        observer: &O,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        self.check_owner(target)?;
        observer.with_source_guard(
            &self.owner_checkout_id,
            self.manifest.storage.anchor(),
            None,
            || {
                self.manifest
                    .remove(request.expected_revision, &target.target_id, tx)
            },
        )
    }

    /// Conservatively latest-started: admission reserves/rotates a per-key private
    /// fence before network, without changing reviewed destination/revision/cache.
    pub fn bind<O: TargetObserver>(
        &self,
        request: BindTarget,
        observer: &O,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetResolution, OrchestrationError> {
        self.check_owner(&request.target)?;
        request.destination.validate()?;
        if let Some(previous) = &request.previous_destination {
            previous.validate()?;
        }
        let (revision, reserved) = self.reserve_bind(&request, observer, tx)?;
        // Even an adapter error without a verifiable proof cannot undo the durable
        // reservation. It returns no authority and older attempts remain fenced out.
        let observation = observer.observe(&request.target, &request.destination)?;
        let outcome = observation
            .result
            .as_ref()
            .map_err(|failure| *failure)
            .and_then(|projection| {
                projection
                    .verified_for(&request.destination)
                    .map(|()| projection)
            });
        self.guarded(
            &request.target,
            &revision,
            Some(&reserved),
            &observation,
            observer,
            |declaration, file| {
                let manifest = self
                    .manifest
                    .document()?
                    .ok_or(OrchestrationError::NotFound)?;
                let observed = outcome
                    .as_ref()
                    .ok()
                    .map(|projection| projection.sources.as_slice());
                self.check_site_store(
                    &manifest,
                    file,
                    &request.target,
                    &request.destination,
                    // A failed probe binds nothing, so there is no store to compare.
                    Some(observed.unwrap_or(&[])),
                )?;
                let index = file
                    .bindings
                    .iter()
                    .position(|binding| binding.target == request.target)
                    .ok_or(OrchestrationError::Conflict)?;
                let binding = &mut file.bindings[index];
                let projection = match outcome {
                    Ok(projection) => projection,
                    Err(failure) => {
                        // Candidate B's failure is not an observation of reviewed A.
                        // Preserve public destination/revision/cache/last-failure; only
                        // rotate the ordering fence and report this candidate's error.
                        binding.observation_generation =
                            fresh_token(tx, Some(&binding.observation_generation))?;
                        self.bindings.write_file(file)?;
                        return Err(OrchestrationError::Observation(failure));
                    }
                };
                let previous = binding.destination.clone();
                if previous.as_ref() != Some(&request.destination) {
                    binding.destination = Some(request.destination.clone());
                    binding.audit(
                        if previous.is_some() {
                            BindingAction::Review
                        } else {
                            BindingAction::Bind
                        },
                        previous,
                        tx.clock.now_ms(),
                    )?;
                }
                observe(binding, Ok(projection), tx)?;
                let result = TargetResolution {
                    declaration,
                    binding: binding.view(false),
                    state: ResolutionState::Reachable,
                    reason: None,
                    stale: false,
                    usable: true,
                    sources: Some(projection.sources.clone()),
                };
                // Held through the durable local commit and live-result construction.
                self.bindings.write_file(file)?;
                Ok(result)
            },
        )
    }

    fn reserve_bind<O: TargetObserver>(
        &self,
        request: &BindTarget,
        observer: &O,
        tx: &mut Transaction<'_>,
    ) -> Result<(String, BindingRecord), OrchestrationError> {
        observer.with_source_guard(
            &self.owner_checkout_id,
            self.manifest.storage.anchor(),
            None,
            || {
                let _manifest_lock = self.manifest.storage.lock()?;
                let (manifest, declaration) = self.declaration(&request.target)?;
                if declaration.state != DeclarationState::Active {
                    return Err(OrchestrationError::Removed);
                }
                if manifest.revision != request.expected_manifest_revision {
                    return Err(OrchestrationError::Conflict);
                }
                let _binding_lock = self.bindings.storage.lock()?;
                let mut file = self.bindings.read_file()?;
                // HS2-SHH7WW: refuse another store before any probe.
                self.check_site_store(
                    &manifest,
                    &file,
                    &request.target,
                    &request.destination,
                    None,
                )?;
                let index = match file
                    .bindings
                    .iter()
                    .position(|binding| binding.target == request.target)
                {
                    Some(index) => {
                        let binding = &mut file.bindings[index];
                        check_expected(
                            binding,
                            request.expected_binding_revision,
                            request.previous_destination.as_ref(),
                        )?;
                        binding.observation_generation =
                            fresh_token(tx, Some(&binding.observation_generation))?;
                        index
                    }
                    None => {
                        if request.expected_binding_revision != 0
                            || request.previous_destination.is_some()
                        {
                            return Err(OrchestrationError::Conflict);
                        }
                        if file.bindings.len() >= MAX_BINDINGS {
                            return Err(OrchestrationError::Capacity);
                        }
                        file.bindings.push(BindingRecord::unbound(
                            request.target.clone(),
                            fresh_token(tx, None)?,
                        ));
                        file.bindings.len() - 1
                    }
                };
                let snapshot = file.bindings[index].clone();
                self.bindings.write_file(&file)?;
                Ok((manifest.revision, snapshot))
            },
        )
    }

    pub fn unbind(
        &self,
        request: UnbindTarget,
        tx: &mut Transaction<'_>,
    ) -> Result<BindingView, OrchestrationError> {
        self.check_owner(&request.target)?;
        // No declaration/source/remote locks: this subset cannot invert the lock order.
        self.bindings.unbind(request, tx)
    }

    pub fn resolve<O: TargetObserver>(
        &self,
        target: &TargetRef,
        observer: &O,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetResolution, OrchestrationError> {
        self.check_owner(target)?;
        let (manifest, declaration) = self.declaration(target)?;
        let file = self.bindings.read_file()?;
        let before = file
            .bindings
            .iter()
            .find(|binding| &binding.target == target)
            .cloned();
        let view = before
            .as_ref()
            .map(|binding| binding.view(true))
            .unwrap_or_else(|| BindingView::absent(target.clone()));
        if declaration.state == DeclarationState::Removed || view.destination.is_none() {
            return Ok(TargetResolution::inactive(declaration, view));
        }
        let destination = view
            .destination
            .as_ref()
            .ok_or(OrchestrationError::NotFound)?;
        let observation = observer.observe(target, destination)?;
        let result = observation
            .result
            .as_ref()
            .map_err(|failure| *failure)
            .and_then(|projection| projection.verified_for(destination).map(|()| projection));
        self.guarded(
            target,
            &manifest.revision,
            before.as_ref(),
            &observation,
            observer,
            |declaration, file| {
                let binding = file
                    .bindings
                    .iter_mut()
                    .find(|binding| &binding.target == target)
                    .ok_or(OrchestrationError::Conflict)?;
                observe(binding, result, tx)?;
                let failure = result.err();
                let response = TargetResolution {
                    declaration,
                    binding: binding.view(failure.is_some()),
                    state: failure.map_or(ResolutionState::Reachable, state_for_failure),
                    reason: failure,
                    stale: failure.is_some(),
                    usable: failure.is_none(),
                    sources: result.ok().map(|projection| projection.sources.clone()),
                };
                self.bindings.write_file(file)?;
                Ok(response)
            },
        )
    }

    fn check_owner(&self, target: &TargetRef) -> Result<(), OrchestrationError> {
        target.validate()?;
        if target.owner_checkout_id != self.owner_checkout_id {
            return Err(OrchestrationError::Invalid(
                "target belongs to another checkout",
            ));
        }
        Ok(())
    }

    fn declaration(
        &self,
        target: &TargetRef,
    ) -> Result<(TargetManifest, TargetDeclaration), OrchestrationError> {
        let manifest = self
            .manifest
            .document()?
            .ok_or(OrchestrationError::NotFound)?;
        let declaration = manifest
            .targets
            .iter()
            .find(|entry| entry.target_id == target.target_id)
            .cloned()
            .ok_or(OrchestrationError::NotFound)?;
        Ok((manifest, declaration))
    }

    fn guarded<O: TargetObserver, T>(
        &self,
        target: &TargetRef,
        revision: &str,
        before: Option<&BindingRecord>,
        observation: &ObservedTarget<O::Proof>,
        observer: &O,
        commit: impl FnOnce(TargetDeclaration, &mut BindingFile) -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError> {
        observer.with_source_guard(
            &self.owner_checkout_id,
            self.manifest.storage.anchor(),
            Some(&observation.proof),
            || {
                let _manifest_lock = self.manifest.storage.lock()?;
                let (manifest, declaration) = self.declaration(target)?;
                if manifest.revision != revision || declaration.state != DeclarationState::Active {
                    return Err(OrchestrationError::Conflict);
                }
                let _binding_lock = self.bindings.storage.lock()?;
                let mut file = self.bindings.read_file()?;
                let current = file
                    .bindings
                    .iter()
                    .find(|binding| &binding.target == target);
                if !same_snapshot(current, before) {
                    return Err(OrchestrationError::Conflict);
                }
                // This callback, including the file write and returned live result, remains
                // inside the remote guard. Validation followed by releasing it would race revoke.
                observer.with_remote_guard(&observation.proof, || commit(declaration, &mut file))
            },
        )
    }
}

fn observe(
    binding: &mut BindingRecord,
    result: Result<&CheckoutProjection, TargetFailure>,
    tx: &mut Transaction<'_>,
) -> Result<(), OrchestrationError> {
    binding.observation_generation = fresh_token(tx, Some(&binding.observation_generation))?;
    let now = tx.clock.now_ms();
    binding.checked_at_ms = Some(now);
    match result {
        Ok(projection) => {
            binding.confirmed_at_ms = Some(now);
            binding.last_failure = None;
            binding.sources = projection
                .sources
                .iter()
                .map(|source| {
                    Ok(CachedTargetSource {
                        connection_id: source.connection_id.clone(),
                        provider: source.provider.clone(),
                        identity: source.identity.clone().ok_or(
                            OrchestrationError::Observation(TargetFailure::SourceMissing),
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, OrchestrationError>>()?;
        }
        Err(failure) => binding.last_failure = Some(failure),
    }
    Ok(())
}

fn state_for_failure(failure: TargetFailure) -> ResolutionState {
    match failure {
        TargetFailure::Unauthorized => ResolutionState::Unauthorized,
        TargetFailure::SourceMissing | TargetFailure::DisabledSource => {
            ResolutionState::SourceMissing
        }
        TargetFailure::IdentityChanged | TargetFailure::ReviewRequired => {
            ResolutionState::IdentityChanged
        }
        _ => ResolutionState::Unreachable,
    }
}
