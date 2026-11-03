use std::path::Path;

use super::bindings::{BindingFile, BindingRecord, MAX_BINDINGS, check_expected, same_snapshot};
use super::manifest::fresh_token;
use super::*;

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
        Ok(Self {
            owner_checkout_id,
            manifest,
            bindings,
        })
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
