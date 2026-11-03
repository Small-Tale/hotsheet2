//! HS2-4J6V46: process-local source-link proof; no paths or proof fields cross a wire.
use super::*;

/// Captured only by the checkout owner. Not serializable or printable as a public DTO.
#[derive(Clone)]
pub struct CheckoutSourceProof {
    registry: PathBuf,
    checkout: Checkout,
    root_identity: PathBuf,
    git_sources: Vec<crate::orchestration::TargetSource>,
}

impl CheckoutRegistry {
    /// Snapshot configuration even when an individual source is unavailable. Source
    /// availability is projected explicitly by its owner, rather than discarding the
    /// whole checkout or assuming every source uses Git.
    pub fn observe_sources(
        &self,
        reference: &str,
    ) -> Result<(Checkout, CheckoutSourceProof), CheckoutError> {
        let locked = self.lock_source_links()?;
        let checkout = locked.resolve(reference)?;
        if !Path::new(&checkout.root).is_dir() {
            return Err(CheckoutError::Missing(checkout.root.clone()));
        }
        let root_identity = Path::new(&checkout.root).canonicalize()?;
        let proof = CheckoutSourceProof {
            root_identity,
            registry: self.path.clone(),
            checkout: checkout.clone(),
            git_sources: checkout.git_source_observations()?,
        };
        Ok((checkout, proof))
    }

    /// Keep checkout links pinned through the callback. Callers nest provider,
    /// manifest, binding and remote guards in that order; no transport or re-entry
    /// into a checkout mutation is permitted inside this callback.
    ///
    /// This fences cooperating local registry writers. Source files and remote peers
    /// remain as-of observations; it is not a filesystem/distributed transaction.
    pub fn with_source_observation<T>(
        &self,
        reference: &str,
        expected_root: &Path,
        proof: Option<&CheckoutSourceProof>,
        commit: impl FnOnce(&Checkout) -> T,
    ) -> Result<T, CheckoutError> {
        let proofs = proof.into_iter().collect::<Vec<_>>();
        self.with_source_observations(reference, expected_root, &proofs, commit)
    }

    /// Owner and local destination may share this registry. Validate both under
    /// ONE lock, never by recursively nesting single-checkout guards.
    pub fn with_source_observations<T>(
        &self,
        owner_reference: &str,
        expected_owner_root: &Path,
        proofs: &[&CheckoutSourceProof],
        commit: impl FnOnce(&Checkout) -> T,
    ) -> Result<T, CheckoutError> {
        if proofs.len() > 2 {
            return Err(CheckoutError::Invalid("source proof limit exceeded".into()));
        }
        let locked = self.lock_source_links()?;
        let owner = locked.resolve(owner_reference)?;
        if Path::new(&owner.root).canonicalize()? != expected_owner_root.canonicalize()? {
            return Err(CheckoutError::Invalid(
                "checkout owner changed; observe again".into(),
            ));
        }
        for proof in proofs {
            if proof.registry != self.path {
                return Err(CheckoutError::Invalid(
                    "source observation changed; observe again".into(),
                ));
            }
            let current = locked.resolve(&proof.checkout.id)?;
            if current != proof.checkout
                || !Path::new(&current.root).is_dir()
                || Path::new(&current.root).canonicalize()? != proof.root_identity
                || proof.git_sources != current.git_source_observations()?
            {
                return Err(CheckoutError::Invalid(
                    "source observation changed; observe again".into(),
                ));
            }
        }
        Ok(commit(&owner))
    }
}

impl CheckoutSourceProof {
    /// Only the bounded path-free projection leaves this opaque proof.
    pub fn git_sources(&self) -> &[crate::orchestration::TargetSource] {
        &self.git_sources
    }

    /// Use the exact Git rows captured by this proof, preventing a later read
    /// from pairing a fresh projection with an older missing-state proof.
    pub fn resolution_projection(
        &self,
        server_id: &str,
        external: &[crate::orchestration::TargetSource],
    ) -> Result<crate::orchestration::CheckoutProjection, crate::orchestration::OrchestrationError>
    {
        self.checkout
            .resolution_projection_with_git(server_id, &self.git_sources, external)
    }
}

impl Checkout {
    /// Metadata-only Git source projection; never initialize/migrate a store or
    /// invoke Git. Provider-only/source-free projects naturally return no Git rows.
    pub fn git_source_observations(
        &self,
    ) -> Result<Vec<crate::orchestration::TargetSource>, CheckoutError> {
        use crate::orchestration::{MAX_TARGET_SOURCES, SourceState, TargetSource};
        if self.sources.len() > MAX_TARGET_SOURCES {
            return Err(CheckoutError::Invalid(
                "source projection limit exceeded".into(),
            ));
        }
        Ok(self
            .sources
            .iter()
            .filter(|source| source.provider == "git")
            .map(|source| {
                let expected = self.store_instance_ids.get(&source.connection_id);
                let (state, identity) = if self
                    .unverified_store_sources
                    .contains_key(&source.connection_id)
                    || expected.is_none()
                {
                    (SourceState::ReviewRequired, None)
                } else {
                    match FsStore::open_without_maintenance(&source.locator)
                        .and_then(|store| store.metadata())
                    {
                        Err(_) => (SourceState::Missing, None),
                        Ok(metadata) if metadata.instance_id.as_ref() != expected => {
                            (SourceState::IdentityChanged, None)
                        }
                        Ok(_) => {
                            let mut hash = Sha256::new();
                            hash.update(b"hs2-git-source-identity-v1\0");
                            hash.update(expected.map(String::as_bytes).unwrap_or_default());
                            (
                                SourceState::Available,
                                Some(format!("{:x}", hash.finalize())),
                            )
                        }
                    }
                };
                TargetSource {
                    connection_id: source.connection_id.clone(),
                    provider: "git".into(),
                    identity,
                    capabilities: (state == SourceState::Available)
                        .then(crate::ProviderCapabilities::git),
                    state,
                }
            })
            .collect())
    }
}

impl Checkout {
    /// Compose only this checkout's registered sources. Missing provider records
    /// remain explicit unknown identities; never infer Git or reuse a cached row.
    /// External rows must come from the trusted provider metadata owner under its
    /// proof guard. This method performs no provider I/O and confers no grant.
    pub fn resolution_projection(
        &self,
        server_id: &str,
        external: &[crate::orchestration::TargetSource],
    ) -> Result<crate::orchestration::CheckoutProjection, crate::orchestration::OrchestrationError>
    {
        let git = self.git_source_observations().map_err(|_| {
            crate::orchestration::OrchestrationError::Observation(
                crate::orchestration::TargetFailure::InvalidResponse,
            )
        })?;
        self.resolution_projection_with_git(server_id, &git, external)
    }

    fn resolution_projection_with_git(
        &self,
        server_id: &str,
        git: &[crate::orchestration::TargetSource],
        external: &[crate::orchestration::TargetSource],
    ) -> Result<crate::orchestration::CheckoutProjection, crate::orchestration::OrchestrationError>
    {
        use crate::orchestration::{
            CheckoutProjection, MAX_TARGET_SOURCES, ORCHESTRATION_VERSION, OrchestrationError,
            SourceState, TargetFailure, TargetSource, checkout_authority,
        };
        if self.sources.len() > MAX_TARGET_SOURCES || external.len() > MAX_TARGET_SOURCES {
            return Err(OrchestrationError::Capacity);
        }
        let mut ids = std::collections::HashSet::new();
        for row in external {
            if !ids.insert(&row.connection_id)
                || !self.sources.iter().any(|source| {
                    source.provider != "git" && source.connection_id == row.connection_id
                })
            {
                return Err(OrchestrationError::Invalid("unlinked source projection"));
            }
        }
        let sources = self
            .sources
            .iter()
            .map(|source| {
                if source.provider == "git" {
                    return git
                        .iter()
                        .find(|row| row.connection_id == source.connection_id)
                        .cloned()
                        .ok_or(OrchestrationError::Observation(
                            TargetFailure::InvalidResponse,
                        ));
                }
                match external
                    .iter()
                    .find(|row| row.connection_id == source.connection_id)
                {
                    Some(row) if row.provider == source.provider => Ok(row.clone()),
                    other => Ok(TargetSource {
                        connection_id: source.connection_id.clone(),
                        provider: source.provider.clone(),
                        identity: None,
                        state: if other.is_some() {
                            SourceState::IdentityChanged
                        } else {
                            SourceState::Missing
                        },
                        capabilities: None,
                    }),
                }
            })
            .collect::<Result<Vec<_>, OrchestrationError>>()?;
        let authority = if sources
            .iter()
            .all(|source| source.state == SourceState::Available)
        {
            checkout_authority(&sources)?
        } else {
            // Unknown/missing identities cannot reconstruct complete authority;
            // verified_for prioritizes their explicit state before this sentinel.
            checkout_authority(&[])?
        };
        let projection = CheckoutProjection {
            schema_version: ORCHESTRATION_VERSION,
            server_id: server_id.into(),
            checkout_id: self.id.clone(),
            checkout_authority: authority,
            sources,
        };
        projection.validate()?;
        Ok(projection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_4j6v46_same_owner_destination_batch_keeps_one_checkout_lock() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let checkout = registry
            .register_sources(&project, None, None, Vec::new(), None)
            .unwrap();
        let (_, proof) = registry.observe_sources(&checkout.id).unwrap();
        assert!(proof.git_sources().is_empty()); // A source-free project is not secretly Git.
        let value = registry
            .with_source_observations(&checkout.id, &project, &[&proof, &proof], |current| {
                let fd = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(registry.path.with_extension("json.lock"))
                    .unwrap();
                assert!(FileLock::try_acquire(fd).unwrap().is_none());
                assert_eq!(current.id, checkout.id);
                7
            })
            .unwrap();
        assert_eq!(value, 7);
        registry
            .register_sources(&project, Some("Changed label"), None, Vec::new(), None)
            .unwrap();
        assert!(
            registry
                .with_source_observation(&checkout.id, &project, Some(&proof), |_| panic!(
                    "stale proof committed"
                ))
                .is_err()
        );
    }

    #[test]
    fn hs2_4j6v46_checkout_proof_rejects_other_registry_or_owner_root() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let other = temp.path().join("other");
        std::fs::create_dir(&project).unwrap();
        std::fs::create_dir(&other).unwrap();
        let a = CheckoutRegistry::new(temp.path().join("a.json"));
        let b = CheckoutRegistry::new(temp.path().join("b.json"));
        let checkout = a
            .register_sources(&project, None, None, Vec::new(), None)
            .unwrap();
        b.register_sources(&project, None, None, Vec::new(), None)
            .unwrap();
        let (_, proof) = a.observe_sources(&checkout.id).unwrap();
        assert!(
            b.with_source_observation(&checkout.id, &project, Some(&proof), |_| panic!(
                "cross-registry proof"
            ))
            .is_err()
        );
        assert!(
            a.with_source_observation(&checkout.id, &other, Some(&proof), |_| panic!(
                "wrong owner root"
            ))
            .is_err()
        );
    }

    #[test]
    fn hs2_4j6v46_provider_only_projection_missing_config_and_unlinked_rows_are_explicit() {
        use crate::orchestration::{SourceState, TargetSource};
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let mut checkout = registry
            .register_sources(&project, None, None, Vec::new(), None)
            .unwrap();
        let server = ulid::Ulid::from(10u128).to_string();
        assert!(
            checkout
                .resolution_projection(&server, &[])
                .unwrap()
                .sources
                .is_empty()
        );
        checkout.sources.push(TicketSource {
            connection_id: "synthetic".into(),
            provider: "synthetic".into(),
            locator: "not-a-Git-path".into(),
        });
        let source = TargetSource {
            connection_id: "synthetic".into(),
            provider: "synthetic".into(),
            identity: Some("a".repeat(64)),
            state: SourceState::Available,
            capabilities: Some(crate::ProviderCapabilities::git()),
        };
        let live = checkout
            .resolution_projection(&server, std::slice::from_ref(&source))
            .unwrap();
        assert_eq!(live.sources, vec![source.clone()]);
        assert!(checkout.git_source_observations().unwrap().is_empty());
        let missing = checkout.resolution_projection(&server, &[]).unwrap();
        assert_eq!(missing.sources[0].state, SourceState::Missing);
        assert!(missing.sources[0].identity.is_none());
        let mut unknown = source;
        unknown.connection_id = "unlinked".into();
        assert!(checkout.resolution_projection(&server, &[unknown]).is_err());
    }

    #[test]
    fn hs2_4j6v46_git_metadata_observation_is_read_only_and_rechecks_actual_store_identity() {
        use crate::orchestration::SourceState;
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let store = FsStore::init(
            temp.path().join("store"),
            &crate::store::StoreMetadata::new("SYN"),
        )
        .unwrap();
        store.ensure_instance_id().unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let checkout = registry
            .register_sources(
                &project,
                None,
                None,
                vec![TicketSource::git(store.root())],
                None,
            )
            .unwrap();
        let path = store.root().join(STORE_METADATA_FILE);
        let before = std::fs::read(&path).unwrap();
        let (_, proof) = registry.observe_sources(&checkout.id).unwrap();
        assert_eq!(proof.git_sources()[0].state, SourceState::Available);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let wire = serde_json::to_string(proof.git_sources()).unwrap();
        assert!(!wire.contains(&store.root().to_string_lossy().to_string()));
        let mut changed = store.metadata().unwrap();
        changed.instance_id = Some(ulid::Ulid::from(99u128).to_string());
        std::fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(
            registry
                .with_source_observation(&checkout.id, &project, Some(&proof), |_| panic!(
                    "changed store committed"
                ))
                .is_err()
        );
        assert_eq!(
            checkout.git_source_observations().unwrap()[0].state,
            SourceState::IdentityChanged
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            checkout.git_source_observations().unwrap()[0].state,
            SourceState::Missing
        );
    }

    #[test]
    fn hs2_4j6v46_destination_directory_disappearance_invalidates_source_proof() {
        let temp = tempfile::tempdir().unwrap();
        let owner_root = temp.path().join("owner");
        let target_root = temp.path().join("target");
        std::fs::create_dir(&owner_root).unwrap();
        std::fs::create_dir(&target_root).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let owner = registry
            .register_sources(&owner_root, None, None, Vec::new(), None)
            .unwrap();
        let target = registry
            .register_sources(&target_root, None, None, Vec::new(), None)
            .unwrap();
        let (_, owner_proof) = registry.observe_sources(&owner.id).unwrap();
        let (_, target_proof) = registry.observe_sources(&target.id).unwrap();
        std::fs::remove_dir(&target_root).unwrap();
        assert!(registry.observe_sources(&target.id).is_err());
        assert!(
            registry
                .with_source_observations(
                    &owner.id,
                    &owner_root,
                    &[&owner_proof, &target_proof],
                    |_| panic!("missing destination committed")
                )
                .is_err()
        );
        assert!(owner_root.is_dir());
    }

    #[test]
    fn hs2_4j6v46_projection_uses_captured_git_state_across_missing_available_missing_aba() {
        use crate::orchestration::SourceState;
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let store = FsStore::init(
            temp.path().join("store"),
            &crate::store::StoreMetadata::new("SYN"),
        )
        .unwrap();
        store.ensure_instance_id().unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let checkout = registry
            .register_sources(
                &project,
                None,
                None,
                vec![TicketSource::git(store.root())],
                None,
            )
            .unwrap();
        let path = store.root().join(STORE_METADATA_FILE);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let (_, proof) = registry.observe_sources(&checkout.id).unwrap();
        assert_eq!(proof.git_sources()[0].state, SourceState::Missing);
        std::fs::write(&path, &bytes).unwrap();
        let server = ulid::Ulid::from(10u128).to_string();
        assert_eq!(
            checkout
                .resolution_projection(&server, &[])
                .unwrap()
                .sources[0]
                .state,
            SourceState::Available
        );
        let captured = proof.resolution_projection(&server, &[]).unwrap();
        assert_eq!(captured.sources[0].state, SourceState::Missing);
        std::fs::remove_file(&path).unwrap();
        registry
            .with_source_observation(&checkout.id, &project, Some(&proof), |_| {
                assert_eq!(captured.sources[0].state, SourceState::Missing);
                assert!(captured.sources[0].identity.is_none());
            })
            .unwrap();
    }
}
