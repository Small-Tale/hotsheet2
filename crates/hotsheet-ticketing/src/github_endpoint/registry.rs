use std::path::PathBuf;

use hotsheet_model::Ulid;

use super::state::{
    Choice, RESERVED, check_decision, credential, hash, record_hash, request_hash,
    without_operation,
};
use super::{
    Decision, Endpoint, EndpointError, Expected, Intent, Link, Observation, Operation,
    OperationState, Request, State, admit_read, guard_ordinary,
};
use crate::{ProviderConfigRegistry, ProviderConnection, ProviderError};

/// A metadata-only checkout observation port. Implementations must not perform provider traffic.
/// The later adapter owns checkout identity/locking; no cross-file atomicity is implied.
pub trait LinkObserver {
    fn is_linked(&self, connection_id: &str, link: &Link) -> Result<bool, EndpointError>;
}

#[derive(Clone)]
pub struct SourceProof {
    owner: PathBuf,
    pub(super) source_id: String,
    pub(super) expected: Expected,
    account: String,
    endpoint: Endpoint,
    unknown: bool,
}

impl std::fmt::Debug for SourceProof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceProof")
            .field("source_id", &self.source_id)
            .finish_non_exhaustive()
    }
}

/// Exact owned pending candidate, opaque and nonserializable. A restart reacquires it
/// through `pending`, not a caller-supplied digest pretending to be a saved candidate.
#[derive(Clone)]
pub struct Recovery {
    owner: PathBuf,
    connection_id: String,
    operation_id: Ulid,
    request_sha256: String,
    candidate_sha256: String,
    identity: super::Identity,
}

impl std::fmt::Debug for Recovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recovery")
            .field("operation_id", &self.operation_id)
            .finish_non_exhaustive()
    }
}

impl Recovery {
    /// Check the exact pending candidate while the checkout publisher already holds
    /// the provider owner. This does not acquire a second lock or turn a saved row
    /// into authority; only `apply` and `pending` can issue this opaque proof.
    pub(crate) fn check_link_candidate(
        &self,
        owner: &ProviderConfigRegistry,
        current: &ProviderConnection,
        link: &Link,
    ) -> Result<(), EndpointError> {
        if self.owner.as_path() != owner.path() {
            return Err(EndpointError::UnsupportedOwner);
        }
        exact_candidate(current, self)?;
        let state = State::load(current)?.ok_or(EndpointError::Conflict)?;
        if Expected::capture(current)?.identity != self.identity {
            return Err(EndpointError::Conflict);
        }
        let operation = matching_operation(&state, self.operation_id, &self.request_sha256)?;
        if operation.link.as_ref() != Some(link) {
            return Err(EndpointError::Conflict);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Mutation {
    pub connection: ProviderConnection,
    pub replayed: bool,
    pub recovery: Option<Recovery>,
}

impl Mutation {
    pub fn is_complete(&self) -> bool {
        self.recovery.is_none()
    }
}

#[derive(Debug, Clone)]
pub enum Compensation {
    Restored(ProviderConnection),
    Removed,
    /// Postcommit failure: retain the actual intervening record, never overwrite it.
    Retained(Option<ProviderConnection>),
}

pub struct EndpointRegistry<'a> {
    registry: &'a ProviderConfigRegistry,
}

/// HS2-6H8ZY6: ordinary metadata maintenance, never traffic authority. Private
/// construction binds the exact record including its retained operation receipt.
#[derive(Clone)]
pub struct OrdinaryWrite {
    owner: PathBuf,
    expected: Expected,
    whole_sha256: String,
    connection: ProviderConnection,
}

impl OrdinaryWrite {
    pub fn connection(&self) -> &ProviderConnection {
        &self.connection
    }
}

impl std::fmt::Debug for OrdinaryWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrdinaryWrite")
            .field("source_id", &self.connection.id)
            .finish_non_exhaustive()
    }
}

/// A selected source snapshot, not a transferable or serialized authority.
#[derive(Clone)]
pub struct ReadProof {
    owner: PathBuf,
    source_id: String,
    expected: Expected,
    whole_sha256: String,
}

impl std::fmt::Debug for ReadProof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadProof")
            .field("source_id", &self.source_id)
            .finish_non_exhaustive()
    }
}

impl<'a> EndpointRegistry<'a> {
    pub fn new(registry: &'a ProviderConfigRegistry) -> Self {
        Self { registry }
    }

    /// Disabled and drifted nonpending sources may receive nonmaterial metadata
    /// maintenance. The target owner still refuses malformed/pending authority.
    pub fn capture_ordinary(&self, id: &str) -> Result<OrdinaryWrite, ProviderError> {
        self.registry
            .update_connection_domain(id, |records| {
                let record = records
                    .iter()
                    .find(|record| record.id == id && record.provider == "github")
                    .ok_or_else(|| ProviderError::UnknownConnection(id.into()))?;
                Ok(OrdinaryWrite {
                    owner: self.registry.path().into(),
                    expected: Expected::capture(record)?,
                    whole_sha256: hash(record)?,
                    connection: record.clone(),
                })
            })
            .map(|(snapshot, _)| snapshot)
    }

    fn ordinary_record<'b>(
        &self,
        snapshot: &OrdinaryWrite,
        records: &'b mut [ProviderConnection],
    ) -> Result<&'b mut ProviderConnection, ProviderError> {
        if snapshot.owner != self.registry.path() {
            return Err(EndpointError::UnsupportedOwner.into());
        }
        let record = records
            .iter_mut()
            .find(|record| record.id == snapshot.connection.id && record.provider == "github")
            .ok_or(EndpointError::Conflict)?;
        snapshot.expected.check(record)?;
        if hash(&*record)? != snapshot.whole_sha256 {
            return Err(EndpointError::Conflict.into());
        }
        Ok(record)
    }

    pub fn check_ordinary(&self, snapshot: &OrdinaryWrite) -> Result<(), ProviderError> {
        self.registry
            .update_connection_domain(&snapshot.connection.id, |records| {
                self.ordinary_record(snapshot, records).map(|_| ())
            })
            .map(|_| ())
    }

    /// The callback is metadata-only: no secret/network/checkout owner may be
    /// entered here. Reuse the SAME target and all-batch owner/publisher guards.
    pub fn update_ordinary(
        &self,
        snapshot: &OrdinaryWrite,
        change: impl FnOnce(&mut ProviderConnection) -> Result<(), ProviderError>,
    ) -> Result<ProviderConnection, ProviderError> {
        let (_, actual) = self.registry.update_connection_domain(
            &snapshot.connection.id,
            |records| -> Result<(), ProviderError> {
                let record = self.ordinary_record(snapshot, records)?;
                change(record)?;
                if record.id != snapshot.connection.id || record.provider != "github" {
                    return Err(EndpointError::Conflict.into());
                }
                Ok(())
            },
        )?;
        actual.ok_or_else(|| EndpointError::Conflict.into())
    }

    /// Capture the actual enabled source under its existing provider owner lock.
    pub fn capture_read(&self, snapshot: &ProviderConnection) -> Result<ReadProof, EndpointError> {
        self.registry.update_domain(|records| {
            let current = find(records, &snapshot.id)?;
            read_state(current)?;
            let expected = Expected::capture(current)?;
            let whole_sha256 = hash(current)?;
            if Expected::capture(snapshot)? != expected || hash(snapshot)? != whole_sha256 {
                return Err(EndpointError::Conflict);
            }
            Ok(ReadProof {
                owner: self.registry.path().into(),
                source_id: current.id.clone(),
                expected,
                whole_sha256,
            })
        })
    }

    /// A short current-record check; no metadata, secret or transport callback runs here.
    pub fn check_current(&self, proof: &ReadProof) -> Result<(), EndpointError> {
        self.check_read_inner(proof, None)
    }

    pub fn check_read(
        &self,
        proof: &ReadProof,
        observation: &Observation,
    ) -> Result<(), EndpointError> {
        self.check_read_inner(proof, Some(observation))
    }

    fn check_read_inner(
        &self,
        proof: &ReadProof,
        observation: Option<&Observation>,
    ) -> Result<(), EndpointError> {
        if proof.owner != self.registry.path() {
            return Err(EndpointError::UnsupportedOwner);
        }
        self.registry.update_domain(|records| {
            let current = find(records, &proof.source_id)?;
            read_state(current)?;
            proof.expected.check(current)?;
            if hash(current)? != proof.whole_sha256 {
                return Err(EndpointError::Conflict);
            }
            if let Some(observation) = observation {
                admit_read(current, observation)?;
            }
            Ok(())
        })
    }

    /// Uses the real provider owner lock; no provider bytes change during proof capture.
    /// This proof cannot be deserialized from HTTP/client assertions.
    pub fn capture_source(
        &self,
        id: &str,
        account: &str,
        endpoint: &Endpoint,
        observation: &Observation,
    ) -> Result<SourceProof, EndpointError> {
        self.registry.update_domain(|records| {
            let source = find(records, id)?;
            validate_record(source)?;
            admit_read(source, observation)?;
            if credential(source)? != account || Endpoint::from_record(source)? != *endpoint {
                return Err(EndpointError::Conflict);
            }
            let state = State::load(source)?;
            Ok(SourceProof {
                owner: self.registry.path().into(),
                source_id: id.into(),
                expected: Expected::capture(source)?,
                account: account.into(),
                endpoint: endpoint.clone(),
                unknown: state
                    .is_none_or(|state| matches!(state.decision, Decision::LegacyUnknown { .. })),
            })
        })
    }

    /// Atomic provider-domain decision only. Actual descriptor/token/link adapters are pending.
    pub fn apply(
        &self,
        request: &Request,
        observation: &Observation,
        links: &impl LinkObserver,
    ) -> Result<Mutation, EndpointError> {
        self.registry.update_domain(|records| {
            validate_record(&request.connection)?;
            if request
                .link
                .as_ref()
                .is_some_and(|link| link.checkout_id.is_empty())
            {
                return Err(EndpointError::Invalid);
            }
            let previous = records
                .iter()
                .find(|record| record.id == request.connection.id)
                .cloned();
            if previous
                .as_ref()
                .map(State::load)
                .transpose()?
                .flatten()
                .as_ref()
                .is_some_and(State::is_pending)
            {
                return Err(EndpointError::Pending);
            }
            let choice = request.intent.choice();
            let digest = request_digest(request, &choice)?;
            if let Some(current) = &previous {
                let state = State::load(current)?;
                if let Some(operation) = state.as_ref().and_then(|state| state.operation.as_ref()) {
                    if operation.id == request.operation_id {
                        if operation.request_sha256 != digest {
                            return Err(EndpointError::Conflict);
                        }
                        if operation.state == OperationState::AwaitingLink {
                            return Err(EndpointError::Pending);
                        }
                        if operation.result_sha256 != record_hash(current)? {
                            return Err(EndpointError::Conflict);
                        }
                        if operation.link.as_ref() != request.link.as_ref() {
                            return Err(EndpointError::Conflict);
                        }
                        if let Some(link) = operation.link.as_ref() {
                            if !links.is_linked(&current.id, link)? {
                                return Err(EndpointError::Conflict);
                            }
                        }
                        // A replay also observes known inference drift before returning success.
                        admit_read(current, observation)?;
                        return Ok(Mutation {
                            connection: current.clone(),
                            replayed: true,
                            recovery: None,
                        });
                    }
                }
                if state.as_ref().is_some_and(State::is_pending) {
                    return Err(EndpointError::Pending);
                }
                request
                    .expected
                    .as_ref()
                    .ok_or(EndpointError::Conflict)?
                    .check(current)?;
                if current.provider != request.connection.provider {
                    return Err(EndpointError::Conflict);
                }
            } else if request.expected.is_some() {
                return Err(EndpointError::Conflict); // Existing review never silently creates.
            }
            reject_reserved_input(previous.as_ref(), &request.connection)?;
            let decision = self.resolve(records, previous.as_ref(), request, observation)?;
            let mut saved = request.connection.clone();
            let incarnation = previous
                .as_ref()
                .map(State::load)
                .transpose()?
                .flatten()
                .map_or_else(Ulid::new, |state| state.incarnation);
            let revision = Ulid::new();
            let mut state = State {
                version: 1,
                incarnation,
                revision,
                decision,
                operation: None,
            };
            state.write(&mut saved)?;
            let before = if request.link.is_some() {
                // A pending operation needs a valid compensatable fixed routing image.
                // Repair invalid legacy routing without a new link before reconnecting it.
                if let Some(previous) = &previous {
                    validate_record(previous)?;
                }
                previous
                    .as_ref()
                    .map(without_operation)
                    .transpose()?
                    .map(Box::new)
            } else {
                None
            };
            let before_sha256 = before.as_ref().map(hash).transpose()?;
            state.operation = Some(Operation {
                id: request.operation_id,
                request_sha256: digest,
                expected: request.expected.clone().into(),
                choice,
                state: if request.link.is_some() {
                    OperationState::AwaitingLink
                } else {
                    OperationState::Completed
                },
                result_revision: revision,
                result_sha256: record_hash(&saved)?,
                link: request.link.clone().into(),
                before,
                before_sha256,
            });
            state.write(&mut saved)?;
            State::load(&saved)?;
            let recovery = state
                .is_pending()
                .then(|| recovery(self.registry.path(), &saved, &state))
                .transpose()?;
            replace(records, saved.clone());
            Ok(Mutation {
                connection: saved,
                replayed: false,
                recovery,
            })
        })
    }

    /// Domain-only ordinary update; actual generic writers must adopt this guard later.
    pub fn ordinary(
        &self,
        expected: &Expected,
        incoming: &ProviderConnection,
    ) -> Result<ProviderConnection, EndpointError> {
        self.registry.update_domain(|records| {
            let before = find(records, &incoming.id)?;
            if State::load(before)?.as_ref().is_some_and(State::is_pending) {
                return Err(EndpointError::Pending);
            }
            expected.check(before)?;
            let mut saved = incoming.clone();
            guard_ordinary(before, &mut saved)?;
            replace(records, saved.clone());
            Ok(saved)
        })
    }

    /// Reacquire only an existing valid pending operation, never promote a before-image.
    pub fn pending(
        &self,
        id: &str,
        operation_id: Ulid,
        request_sha256: &str,
    ) -> Result<Recovery, EndpointError> {
        self.registry.update_domain(|records| {
            let current = find(records, id)?;
            let state = State::load(current)?.ok_or(EndpointError::Conflict)?;
            matching_operation(&state, operation_id, request_sha256)?;
            recovery(self.registry.path(), current, &state)
        })
    }

    pub fn finalize(
        &self,
        proof: &Recovery,
        links: &impl LinkObserver,
    ) -> Result<ProviderConnection, EndpointError> {
        if proof.owner.as_path() != self.registry.path() {
            return Err(EndpointError::UnsupportedOwner);
        }
        self.registry.update_domain(|records| {
            let mut saved = find(records, &proof.connection_id)?.clone();
            let mut state = State::load(&saved)?.ok_or(EndpointError::Conflict)?;
            if let Some(operation) = &state.operation {
                if operation.state == OperationState::Completed
                    && operation.id == proof.operation_id
                    && operation.request_sha256 == proof.request_sha256
                    && Expected::capture(&saved)?.identity == proof.identity
                    && operation.result_sha256 == record_hash(&saved)?
                {
                    if let Some(link) = operation.link.as_ref() {
                        if !links.is_linked(&saved.id, link)? {
                            return Err(EndpointError::Conflict);
                        }
                    }
                    return Ok(saved);
                }
            }
            exact_candidate(&saved, proof)?;
            let operation = matching_operation(&state, proof.operation_id, &proof.request_sha256)?;
            if !links.is_linked(
                &saved.id,
                operation.link.as_ref().ok_or(EndpointError::Invalid)?,
            )? {
                return Err(EndpointError::Pending);
            }
            let operation = state.operation.as_mut().ok_or(EndpointError::Invalid)?;
            operation.state = OperationState::Completed;
            operation.before = None;
            operation.before_sha256 = None;
            state.write(&mut saved)?;
            replace(records, saved.clone());
            Ok(saved)
        })
    }

    /// Postcommit compensation is not zero-write rollback. Never erase an intervening writer.
    pub fn compensate(&self, proof: &Recovery) -> Result<Compensation, EndpointError> {
        if proof.owner.as_path() != self.registry.path() {
            return Err(EndpointError::UnsupportedOwner);
        }
        self.registry.update_domain(|records| {
            let current = records
                .iter()
                .find(|record| record.id == proof.connection_id)
                .cloned();
            let Some(current) = current else {
                return Ok(Compensation::Retained(None));
            };
            if exact_candidate(&current, proof).is_err() {
                return Ok(Compensation::Retained(Some(current)));
            }
            let state = State::load(&current)?.ok_or(EndpointError::Conflict)?;
            let operation = matching_operation(&state, proof.operation_id, &proof.request_sha256)?;
            if let Some(before) = &operation.before {
                let mut restored = (**before).clone();
                let old = State::load(&restored)?;
                let decision = old.as_ref().map(|state| state.decision.clone()).unwrap_or(
                    Decision::LegacyUnknown {
                        credential_ref: credential(&restored)?.into(),
                        endpoint: Endpoint::from_record(&restored)?,
                    },
                );
                State {
                    version: 1,
                    incarnation: state.incarnation,
                    revision: Ulid::new(),
                    decision,
                    operation: None,
                }
                .write(&mut restored)?;
                replace(records, restored.clone());
                Ok(Compensation::Restored(restored))
            } else {
                records.retain(|record| record.id != proof.connection_id);
                Ok(Compensation::Removed)
            }
        })
    }

    fn resolve(
        &self,
        records: &[ProviderConnection],
        previous: Option<&ProviderConnection>,
        request: &Request,
        observation: &Observation,
    ) -> Result<Decision, EndpointError> {
        let record = &request.connection;
        let endpoint = Endpoint::from_record(record)?;
        let account = credential(record)?.to_owned();
        match &request.intent {
            Intent::Legacy => {
                if let Some(previous) = previous {
                    if State::load(previous)?.is_some_and(|state| {
                        !matches!(state.decision, Decision::LegacyUnknown { .. })
                    }) {
                        return Err(EndpointError::Conflict);
                    }
                }
                Ok(Decision::LegacyUnknown {
                    credential_ref: account,
                    endpoint,
                })
            }
            Intent::Keep => {
                let previous = previous.ok_or(EndpointError::Invalid)?;
                if endpoint != Endpoint::from_record(previous)? || account != credential(previous)?
                {
                    return Err(EndpointError::Conflict);
                }
                Ok(Decision::Explicit {
                    credential_ref: account,
                    endpoint,
                })
            }
            Intent::Explicit {
                credential_ref,
                endpoint: choice,
            } => {
                if *credential_ref != account || *choice != endpoint {
                    return Err(EndpointError::Invalid);
                }
                Ok(Decision::Explicit {
                    credential_ref: account,
                    endpoint,
                })
            }
            Intent::InferSite {
                credential_ref,
                endpoint: choice,
                origin,
            } => {
                let normalized = crate::github_origin::normalize_web_base(origin)
                    .map_err(|_| EndpointError::Invalid)?;
                let inferred = crate::github_origin::api_base_for_web_origin(origin)
                    .map_err(|_| EndpointError::Invalid)?
                    .map_or(Endpoint::Public, Endpoint::Url);
                if *credential_ref != account
                    || *choice != endpoint
                    || inferred != endpoint
                    || normalized != *origin
                {
                    return Err(EndpointError::Invalid);
                }
                let decision = Decision::Inferred {
                    credential_ref: account,
                    endpoint,
                    origin: normalized,
                };
                check_decision(&decision, observation)?;
                Ok(decision)
            }
            Intent::Copy { proof, unknown } => {
                if proof.owner.as_path() != self.registry.path() {
                    return Err(EndpointError::UnsupportedOwner);
                }
                let source = find(records, &proof.source_id)?;
                proof.expected.check(source)?;
                validate_record(source)?;
                admit_read(source, observation)?;
                if credential(source)? != proof.account
                    || proof.account != account
                    || proof.endpoint != endpoint
                    || Endpoint::from_record(source)? != endpoint
                {
                    return Err(EndpointError::Conflict);
                }
                let source_state = State::load(source)?;
                let source_unknown = source_state
                    .as_ref()
                    .is_none_or(|state| matches!(state.decision, Decision::LegacyUnknown { .. }));
                if source_unknown != *unknown || source_unknown != proof.unknown {
                    return Err(EndpointError::Conflict);
                }
                if previous
                    .map(State::load)
                    .transpose()?
                    .flatten()
                    .is_some_and(|state| !matches!(state.decision, Decision::LegacyUnknown { .. }))
                    && source_unknown
                {
                    return Err(EndpointError::Conflict);
                }
                Ok(source_state
                    .map(|state| state.decision)
                    .unwrap_or(Decision::LegacyUnknown {
                        credential_ref: account,
                        endpoint,
                    }))
            }
        }
    }
}

fn read_state(record: &ProviderConnection) -> Result<(), EndpointError> {
    if record.provider != "github" || record.disabled {
        return Err(EndpointError::Conflict);
    }
    if State::load(record)?.is_some_and(|state| state.is_pending()) {
        return Err(EndpointError::Pending);
    }
    Ok(())
}

fn validate_record(record: &ProviderConnection) -> Result<(), EndpointError> {
    if record.provider != "github" || record.id.is_empty() || !record.settings.is_object() {
        return Err(EndpointError::Invalid);
    }
    credential(record)?;
    Endpoint::from_record(record)?.validate()
}

fn find<'a>(
    records: &'a [ProviderConnection],
    id: &str,
) -> Result<&'a ProviderConnection, EndpointError> {
    records
        .iter()
        .find(|record| record.id == id)
        .ok_or(EndpointError::Conflict)
}

fn replace(records: &mut Vec<ProviderConnection>, saved: ProviderConnection) {
    records.retain(|record| record.id != saved.id);
    records.push(saved);
    records.sort_by(|a, b| a.id.cmp(&b.id));
}

fn reject_reserved_input(
    previous: Option<&ProviderConnection>,
    incoming: &ProviderConnection,
) -> Result<(), EndpointError> {
    if let Some(value) = incoming.settings.get(RESERVED) {
        if previous.and_then(|record| record.settings.get(RESERVED)) != Some(value) {
            return Err(EndpointError::Invalid);
        }
    }
    Ok(())
}

fn request_digest(request: &Request, choice: &Choice) -> Result<String, EndpointError> {
    request_hash(
        &request.connection,
        request.operation_id,
        &request.expected,
        choice,
        &request.link,
    )
}

fn matching_operation<'a>(
    state: &'a State,
    id: Ulid,
    digest: &str,
) -> Result<&'a Operation, EndpointError> {
    let operation = state.operation.as_ref().ok_or(EndpointError::Conflict)?;
    if operation.state != OperationState::AwaitingLink
        || operation.id != id
        || operation.request_sha256 != digest
    {
        return Err(EndpointError::Conflict);
    }
    Ok(operation)
}

fn recovery(
    owner: &std::path::Path,
    record: &ProviderConnection,
    state: &State,
) -> Result<Recovery, EndpointError> {
    let operation = state.operation.as_ref().ok_or(EndpointError::Conflict)?;
    if !state.is_pending() || operation.result_sha256 != record_hash(record)? {
        return Err(EndpointError::Conflict);
    }
    Ok(Recovery {
        owner: owner.into(),
        connection_id: record.id.clone(),
        operation_id: operation.id,
        request_sha256: operation.request_sha256.clone(),
        candidate_sha256: hash(record)?,
        identity: super::Identity::Known {
            incarnation: state.incarnation,
            revision: state.revision,
        },
    })
}

fn exact_candidate(record: &ProviderConnection, proof: &Recovery) -> Result<(), EndpointError> {
    if record.id != proof.connection_id || hash(record)? != proof.candidate_sha256 {
        return Err(EndpointError::Conflict);
    }
    Ok(())
}
