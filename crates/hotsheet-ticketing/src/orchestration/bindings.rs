use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{identity_authority, valid_digest, valid_identifier, valid_token};
use super::manifest::fresh_token;
use super::storage::{Storage, decode};
use super::*;

pub(crate) const MAX_BINDINGS: usize = 256;
pub(crate) const MAX_AUDIT: usize = 32;
const MAX_RECORD_BYTES: usize = 64 * 1024;
// Count-derived reservation leaves enough room for every record's bounded disable/audit state.
pub(crate) const MAX_BINDING_BYTES: usize = MAX_BINDINGS * MAX_RECORD_BYTES + 1024;

#[derive(Clone)]
pub struct BindingRegistry {
    pub(crate) storage: Storage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindingFile {
    pub(crate) schema_version: u32,
    pub(crate) bindings: Vec<BindingRecord>,
}

impl Default for BindingFile {
    fn default() -> Self {
        Self {
            schema_version: ORCHESTRATION_VERSION,
            bindings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindingRecord {
    pub(crate) target: TargetRef,
    pub(crate) revision: u64,
    pub(crate) destination: Option<TargetDestination>,
    pub(crate) observation_generation: String,
    pub(crate) checked_at_ms: Option<u64>,
    pub(crate) confirmed_at_ms: Option<u64>,
    pub(crate) last_failure: Option<TargetFailure>,
    pub(crate) sources: Vec<CachedTargetSource>,
    pub(crate) history: Vec<BindingAudit>,
}

impl BindingRecord {
    // A revision-zero intent reserves bounded per-key ordering only. It has no
    // destination, observation, audit, cached authority or usable resolution.
    pub(crate) fn unbound(target: TargetRef, generation: String) -> Self {
        Self {
            target,
            revision: 0,
            destination: None,
            observation_generation: generation,
            checked_at_ms: None,
            confirmed_at_ms: None,
            last_failure: None,
            sources: Vec::new(),
            history: Vec::new(),
        }
    }
    pub(crate) fn view(&self, stale: bool) -> BindingView {
        BindingView {
            target: self.target.clone(),
            revision: self.revision,
            destination: self.destination.clone(),
            checked_at_ms: self.checked_at_ms,
            confirmed_at_ms: self.confirmed_at_ms,
            last_failure: self.last_failure,
            last_confirmed_sources: self.sources.clone(),
            review_history: self.history.clone(),
            stale,
        }
    }

    pub(crate) fn audit(
        &mut self,
        action: BindingAction,
        previous: Option<TargetDestination>,
        now_ms: u64,
    ) -> Result<(), OrchestrationError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(OrchestrationError::Conflict)?;
        self.history.push(BindingAudit {
            revision: self.revision,
            action,
            previous_destination: previous,
            destination: self.destination.clone(),
            at_ms: now_ms,
        });
        if self.history.len() > MAX_AUDIT {
            self.history.remove(0);
        }
        Ok(())
    }
}

impl BindingRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine(home),
        }
    }

    pub(crate) fn read_file(&self) -> Result<BindingFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_BINDING_BYTES)? else {
            return Ok(BindingFile::default());
        };
        let file: BindingFile = decode(&bytes)?;
        validate(&file).map_err(|_| OrchestrationError::Corrupt)?;
        Ok(file)
    }

    pub(crate) fn write_file(&self, file: &BindingFile) -> Result<(), OrchestrationError> {
        validate(file)?;
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_BINDING_BYTES)
    }

    pub fn get(&self, target: &TargetRef) -> Result<BindingView, OrchestrationError> {
        target.validate()?;
        Ok(self
            .read_file()?
            .bindings
            .iter()
            .find(|binding| &binding.target == target)
            .map(|b| b.view(true))
            .unwrap_or_else(|| BindingView::absent(target.clone())))
    }

    /// Machine-only disable; shared manifest/source/remote availability is irrelevant.
    /// An absent key reserves an unobserved revision-zero fence. At capacity an
    /// unknown key fails safely: every valid probe already reserved an existing
    /// key, records are never deleted, and existing keys remain disableable.
    pub fn unbind(
        &self,
        request: UnbindTarget,
        tx: &mut Transaction<'_>,
    ) -> Result<BindingView, OrchestrationError> {
        request.target.validate()?;
        if let Some(previous) = &request.previous_destination {
            previous.validate()?;
        }
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let index = match file
            .bindings
            .iter()
            .position(|binding| binding.target == request.target)
        {
            Some(index) => index,
            None => {
                if request.expected_binding_revision != 0 || request.previous_destination.is_some()
                {
                    return Err(OrchestrationError::Conflict);
                }
                if file.bindings.len() >= MAX_BINDINGS {
                    return Err(OrchestrationError::Capacity);
                }
                // Every valid bind probe reserves its key before network. An absent
                // disable must also leave a durable fence for an in-flight activation.
                file.bindings.push(BindingRecord::unbound(
                    request.target.clone(),
                    fresh_token(tx, None)?,
                ));
                file.bindings.len() - 1
            }
        };
        let binding = &mut file.bindings[index];
        check_expected(
            binding,
            request.expected_binding_revision,
            request.previous_destination.as_ref(),
        )?;
        let old = binding.destination.take();
        if old.is_some() {
            binding.audit(BindingAction::Unbind, old, tx.clock.now_ms())?;
        }
        // Repeated unbind fences an in-flight reactivation even at the same public revision.
        binding.observation_generation = fresh_token(tx, Some(&binding.observation_generation))?;
        let result = binding.view(true);
        self.write_file(&file)?;
        Ok(result)
    }
}

pub(crate) fn check_expected(
    binding: &BindingRecord,
    revision: u64,
    previous: Option<&TargetDestination>,
) -> Result<(), OrchestrationError> {
    if binding.revision != revision || binding.destination.as_ref() != previous {
        return Err(OrchestrationError::Conflict);
    }
    Ok(())
}

pub(crate) fn same_snapshot(
    current: Option<&BindingRecord>,
    before: Option<&BindingRecord>,
) -> bool {
    match (current, before) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.target == b.target
                && a.revision == b.revision
                && a.destination == b.destination
                && a.observation_generation == b.observation_generation
        }
        _ => false,
    }
}

fn validate(file: &BindingFile) -> Result<(), OrchestrationError> {
    if file.schema_version != ORCHESTRATION_VERSION {
        return Err(OrchestrationError::UnsupportedVersion {
            found: file.schema_version.into(),
        });
    }
    if file.bindings.len() > MAX_BINDINGS {
        return Err(OrchestrationError::Capacity);
    }
    let mut targets = HashSet::new();
    for binding in &file.bindings {
        binding.target.validate()?;
        valid_token(&binding.observation_generation)?;
        if !targets.insert(&binding.target) {
            return Err(OrchestrationError::Invalid("duplicate machine binding"));
        }
        if binding.history.len() > MAX_AUDIT || binding.sources.len() > MAX_TARGET_SOURCES {
            return Err(OrchestrationError::Invalid("binding bounds exceeded"));
        }
        if binding.revision == 0
            && (binding.destination.is_some()
                || !binding.history.is_empty()
                || binding.checked_at_ms.is_some()
                || binding.confirmed_at_ms.is_some()
                || binding.last_failure.is_some()
                || !binding.sources.is_empty())
        {
            return Err(OrchestrationError::Invalid(
                "revision-zero intent must be unbound and unobserved",
            ));
        }
        if binding.history.len() as u64 != binding.revision.min(MAX_AUDIT as u64) {
            return Err(OrchestrationError::Invalid(
                "binding audit window is incomplete",
            ));
        }
        if let Some(destination) = &binding.destination {
            destination.validate()?;
        }
        if binding.confirmed_at_ms.is_some() != !binding.sources.is_empty()
            || (binding.confirmed_at_ms.is_some() && binding.checked_at_ms.is_none())
            || (binding.destination.is_some() && binding.confirmed_at_ms.is_none())
            || (binding.last_failure.is_some() && binding.checked_at_ms.is_none())
        {
            return Err(OrchestrationError::Invalid(
                "incomplete binding observation",
            ));
        }
        let mut sources = HashSet::new();
        for source in &binding.sources {
            valid_identifier(&source.connection_id)?;
            valid_identifier(&source.provider)?;
            valid_digest(&source.identity)?;
            if source.provider.len() > 80 || !sources.insert(&source.connection_id) {
                return Err(OrchestrationError::Invalid("cached source bounds exceeded"));
            }
        }
        if let Some(destination) = &binding.destination {
            let authority = identity_authority(
                binding
                    .sources
                    .iter()
                    .map(|source| (source.provider.as_str(), source.identity.as_str()))
                    .collect(),
            )?;
            if authority != destination.authority() {
                return Err(OrchestrationError::Invalid(
                    "cached source authority changed",
                ));
            }
        }
        let mut revision = 0;
        let mut previous_destination = None;
        for (index, audit) in binding.history.iter().enumerate() {
            if audit.revision == 1
                && (audit.action != BindingAction::Bind || audit.previous_destination.is_some())
            {
                return Err(OrchestrationError::Invalid(
                    "first binding audit must bind from unbound",
                ));
            }
            if audit.revision <= revision || audit.revision > binding.revision {
                return Err(OrchestrationError::Invalid(
                    "invalid binding audit revision",
                ));
            }
            if index > 0 && audit.revision.checked_sub(revision) != Some(1) {
                return Err(OrchestrationError::Invalid("binding audit revision gap"));
            }
            if let Some(destination) = &audit.destination {
                destination.validate()?;
            }
            if let Some(destination) = &audit.previous_destination {
                destination.validate()?;
            }
            if index > 0 && audit.previous_destination.as_ref() != previous_destination {
                return Err(OrchestrationError::Invalid(
                    "binding audit identity chain changed",
                ));
            }
            let valid_action = match audit.action {
                BindingAction::Bind => {
                    audit.previous_destination.is_none() && audit.destination.is_some()
                }
                BindingAction::Review => {
                    audit.previous_destination.is_some()
                        && audit.destination.is_some()
                        && audit.previous_destination != audit.destination
                }
                BindingAction::Unbind => {
                    audit.previous_destination.is_some() && audit.destination.is_none()
                }
            };
            if !valid_action {
                return Err(OrchestrationError::Invalid("binding audit action changed"));
            }
            revision = audit.revision;
            previous_destination = audit.destination.as_ref();
        }
        if revision != binding.revision || previous_destination != binding.destination.as_ref() {
            return Err(OrchestrationError::Invalid(
                "binding audit does not match current state",
            ));
        }
        if serde_json::to_vec(binding)
            .map_err(|_| OrchestrationError::Corrupt)?
            .len()
            > MAX_RECORD_BYTES
        {
            return Err(OrchestrationError::Capacity);
        }
    }
    Ok(())
}
