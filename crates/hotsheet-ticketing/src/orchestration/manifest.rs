use std::collections::HashMap;
use std::path::Path;

use super::contract::{MAX_MANIFEST_BYTES, bounded_text, valid_token};
use super::storage::{Storage, decode};
use super::*;

#[derive(Clone)]
pub struct ManifestStore {
    pub(crate) storage: Storage,
}

impl ManifestStore {
    pub fn new(checkout_root: &Path) -> Self {
        Self {
            storage: Storage::shared(checkout_root),
        }
    }

    pub fn read(&self) -> Result<ManifestView, OrchestrationError> {
        Ok(self.document()?.into())
    }

    pub(crate) fn document(&self) -> Result<Option<TargetManifest>, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_MANIFEST_BYTES)? else {
            return Ok(None);
        };
        let document: TargetManifest = decode(&bytes)?;
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
        let revision = fresh_token(tx, previous.as_ref().map(|doc| doc.revision.as_str()))?;
        let document = TargetManifest {
            schema_version: ORCHESTRATION_VERSION,
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
        let _lock = self.storage.lock()?;
        let current = self.document()?.ok_or(OrchestrationError::NotFound)?;
        if current.revision != request.expected_revision {
            return Err(OrchestrationError::Conflict);
        }
        let mut targets = current.targets;
        let target = targets
            .iter_mut()
            .find(|target| target.target_id == target_id)
            .ok_or(OrchestrationError::NotFound)?;
        if target.state == DeclarationState::Removed {
            return Err(OrchestrationError::Removed);
        }
        target.label = request.label;
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

fn validate(document: &TargetManifest) -> Result<(), OrchestrationError> {
    if document.schema_version != ORCHESTRATION_VERSION {
        return Err(OrchestrationError::UnsupportedVersion {
            found: document.schema_version.into(),
        });
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
