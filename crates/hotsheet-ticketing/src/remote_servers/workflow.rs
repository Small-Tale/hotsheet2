use super::contract::{bounded_text, valid_id};
use super::registry::{Binding, MAX_CREDENTIAL_OBLIGATIONS, RegistryFile, credential_obligations};
use super::*;
use crate::SecretStore;

impl RemoteRegistry {
    pub fn register(
        &self,
        request: RegisterRemote,
        secrets: &dyn SecretStore,
        transport: &dyn RemoteTransport,
        now_ms: u64,
    ) -> Result<RemoteDiscovery, RemoteError> {
        validate_registration(&request.id, &request.label, &request.expected_server_id)?;
        let origin = normalize_remote_origin(&request.origin)?;
        let identity =
            reviewed_identity(&request.expected_server_id, &origin, &request.credentials)?;
        let before = self.read()?;
        let observed = before
            .servers
            .iter()
            .find(|binding| binding.id == request.id);
        if let Some(existing) = observed {
            if existing.revoked {
                return Err(RemoteError::Revoked);
            }
            if existing.origin != origin
                || existing.label != request.label
                || existing.identity != identity
            {
                return Err(RemoteError::Conflict);
            }
        } else if before.servers.len() >= 64
            || before
                .servers
                .iter()
                .any(|binding| !binding.revoked && binding.origin == origin)
        {
            return Err(RemoteError::Conflict);
        }
        // No registry lock is held over a network call or an untrusted peer response.
        let snapshot = verified_probe(transport, &origin, &request.credentials, &identity)
            .map_err(RemoteError::Verification)?;
        let _lock = self.lock()?;
        let mut file = self.read()?;
        self.cleanup_locked(&mut file, secrets)?;
        if let Some(index) = file
            .servers
            .iter()
            .position(|binding| binding.id == request.id)
        {
            let existing = &file.servers[index];
            // An idempotent register is still an observation: a late probe may not
            // overwrite a newer result, including a concurrent first registration.
            if !observed.is_some_and(|before| same_observation(existing, before)) {
                return Err(RemoteError::Conflict);
            }
            if existing.revoked {
                return Err(RemoteError::Revoked);
            }
            if existing.origin != origin
                || existing.label != request.label
                || existing.identity != identity
            {
                return Err(RemoteError::Conflict);
            }
            let stored = read_credentials(existing, secrets).map_err(RemoteError::Verification)?;
            if stored != request.credentials {
                return Err(RemoteError::Conflict);
            }
            update_observation(&mut file.servers[index], Ok(&snapshot), now_ms);
            self.write(&file)?;
            return Ok(discovery(
                &file.servers[index],
                &file,
                Some(snapshot.checkouts),
            ));
        }
        if observed.is_some() {
            return Err(RemoteError::Conflict);
        }
        if file.servers.len() >= 64
            || file
                .servers
                .iter()
                .any(|binding| !binding.revoked && binding.origin == origin)
        {
            return Err(RemoteError::Conflict);
        }
        let binding = Binding {
            id: request.id,
            label: request.label,
            origin: origin.clone(),
            identity: identity.clone(),
            revision: 1,
            revoked: false,
            credential_ref: None,
            observation_generation: ulid::Ulid::new().to_string(),
            last_failure: None,
            checked_at_ms: Some(now_ms),
            confirmed_at_ms: Some(now_ms),
            checkouts: snapshot.checkouts.clone(),
            history: vec![RemoteReview {
                revision: 1,
                origin,
                identity,
                reviewed_at_ms: now_ms,
            }],
        };
        self.commit_credentials(&mut file, binding, &request.credentials, secrets)?;
        let binding = file.servers.last().ok_or(RemoteError::Corrupt)?;
        Ok(discovery(binding, &file, Some(snapshot.checkouts)))
    }

    pub fn review(
        &self,
        id: &str,
        request: ReviewRemote,
        secrets: &dyn SecretStore,
        transport: &dyn RemoteTransport,
        now_ms: u64,
    ) -> Result<RemoteDiscovery, RemoteError> {
        validate_registration(id, &request.label, &request.expected_server_id)?;
        let origin = normalize_remote_origin(&request.origin)?;
        let identity =
            reviewed_identity(&request.expected_server_id, &origin, &request.credentials)?;
        // Reject stale reviews before probing and recheck after the probe.
        let before = self.read()?;
        let observed = before
            .servers
            .iter()
            .find(|binding| binding.id == id)
            .ok_or(RemoteError::NotFound)?;
        check_review(observed, &request)?;
        let snapshot = verified_probe(transport, &origin, &request.credentials, &identity)
            .map_err(RemoteError::Verification)?;
        let _lock = self.lock()?;
        let mut file = self.read()?;
        self.cleanup_locked(&mut file, secrets)?;
        let existing = file
            .servers
            .iter()
            .find(|binding| binding.id == id)
            .ok_or(RemoteError::NotFound)?;
        check_review(existing, &request)?;
        // Explicit review intent also predates its probe: a newer observation
        // requires a fresh deliberate retry, even if public revision is unchanged.
        if !same_observation(existing, observed) {
            return Err(RemoteError::Conflict);
        }
        if file
            .servers
            .iter()
            .any(|binding| binding.id != id && !binding.revoked && binding.origin == origin)
        {
            return Err(RemoteError::Conflict);
        }
        let mut binding = existing.clone();
        binding.revision = binding
            .revision
            .checked_add(1)
            .ok_or(RemoteError::Conflict)?;
        binding.label = request.label;
        binding.origin = origin.clone();
        binding.identity = identity.clone();
        binding.revoked = false;
        binding.history.push(RemoteReview {
            revision: binding.revision,
            origin,
            identity,
            reviewed_at_ms: now_ms,
        });
        if binding.history.len() > 64 {
            binding.history.remove(0);
        }
        update_observation(&mut binding, Ok(&snapshot), now_ms);
        self.commit_credentials(&mut file, binding, &request.credentials, secrets)?;
        let binding = file
            .servers
            .iter()
            .find(|binding| binding.id == id)
            .ok_or(RemoteError::Corrupt)?;
        Ok(discovery(binding, &file, Some(snapshot.checkouts)))
    }

    pub fn revoke(
        &self,
        id: &str,
        expected_revision: u64,
        secrets: &dyn SecretStore,
    ) -> Result<RemoteView, RemoteError> {
        let _lock = self.lock()?;
        let mut file = self.read()?;
        let index = file
            .servers
            .iter()
            .position(|binding| binding.id == id)
            .ok_or(RemoteError::NotFound)?;
        let binding = &mut file.servers[index];
        if binding.revision != expected_revision
            && !(binding.revoked && binding.revision == expected_revision.saturating_add(1))
        {
            return Err(RemoteError::Conflict);
        }
        if !binding.revoked {
            binding.revoked = true;
            binding.revision = binding
                .revision
                .checked_add(1)
                .ok_or(RemoteError::Conflict)?;
            if let Some(reference) = binding.credential_ref.take() {
                file.pending_cleanup.push(reference);
            }
        }
        // Durable disable precedes cleanup. Moving active -> pending consumes no
        // new obligation, even at capacity; repeated revoke keeps its public revision.
        file.servers[index].observation_generation = ulid::Ulid::new().to_string();
        self.write(&file)?;
        self.cleanup_locked(&mut file, secrets)?;
        Ok(file.servers[index].view(&file))
    }

    pub fn discover(
        &self,
        id: &str,
        secrets: &dyn SecretStore,
        transport: &dyn RemoteTransport,
        now_ms: u64,
    ) -> Result<RemoteDiscovery, RemoteError> {
        let before = self.read()?;
        let binding = before
            .servers
            .iter()
            .find(|binding| binding.id == id)
            .ok_or(RemoteError::NotFound)?;
        if binding.revoked {
            return Ok(discovery(binding, &before, None));
        }
        let outcome = read_credentials(binding, secrets).and_then(|credentials| {
            match credentials.trust_fingerprint(&binding.origin) {
                Ok(fingerprint) if fingerprint == binding.identity.ca_fingerprint => {}
                _ => return Err(RemoteFailure::TrustFailed),
            }
            verified_probe(transport, &binding.origin, &credentials, &binding.identity)
        });
        let _lock = self.lock()?;
        let mut file = self.read()?;
        let current = file
            .servers
            .iter_mut()
            .find(|current| current.id == id)
            .ok_or(RemoteError::NotFound)?;
        if !same_observation(current, binding) || current.revoked {
            return Err(RemoteError::Conflict);
        }
        update_observation(
            current,
            outcome.as_ref().map_err(|failure| *failure),
            now_ms,
        );
        self.write(&file)?;
        let current = file
            .servers
            .iter()
            .find(|current| current.id == id)
            .ok_or(RemoteError::Corrupt)?;
        Ok(discovery(
            current,
            &file,
            outcome.ok().map(|snapshot| snapshot.checkouts),
        ))
    }

    fn commit_credentials(
        &self,
        file: &mut RegistryFile,
        mut binding: Binding,
        credentials: &RemoteCredentials,
        secrets: &dyn SecretStore,
    ) -> Result<(), RemoteError> {
        let reference = format!("remote-{}-{}", binding.id, binding.revision);
        if file.pending_cleanup.contains(&reference) {
            return Err(RemoteError::Credentials);
        }
        if credential_obligations(file) >= MAX_CREDENTIAL_OBLIGATIONS {
            return Err(RemoteError::Invalid(
                "remote credential capacity reached; retry credential cleanup",
            ));
        }
        // HS2-1Y45MZ: write-ahead intent survives a crash after secret storage but before activation.
        file.pending_cleanup.push(reference.clone());
        self.write(file)?;
        let encoded = serde_json::to_string(credentials).map_err(|_| RemoteError::Credentials)?;
        if secrets.set(&reference, &encoded).is_err() {
            self.cleanup_locked(file, secrets)?;
            return Err(RemoteError::Credentials);
        }
        let mut activated = file.clone();
        activated
            .pending_cleanup
            .retain(|pending| pending != &reference);
        if let Some(old) = activated
            .servers
            .iter()
            .find(|current| current.id == binding.id)
            .and_then(|current| current.credential_ref.clone())
        {
            activated.pending_cleanup.push(old);
        }
        binding.credential_ref = Some(reference);
        if let Some(index) = activated
            .servers
            .iter()
            .position(|current| current.id == binding.id)
        {
            activated.servers[index] = binding;
        } else {
            activated.servers.push(binding);
        }
        self.write(&activated)?;
        *file = activated;
        self.cleanup_locked(file, secrets)
    }

    fn cleanup_locked(
        &self,
        file: &mut RegistryFile,
        secrets: &dyn SecretStore,
    ) -> Result<(), RemoteError> {
        let before = file.pending_cleanup.len();
        // Active references are disjoint from pending cleanup (validated by read()).
        file.pending_cleanup
            .retain(|reference| secrets.delete(reference).is_err());
        if before != file.pending_cleanup.len() {
            self.write(file)?;
        }
        Ok(())
    }
}

fn validate_registration(id: &str, label: &str, server_id: &str) -> Result<(), RemoteError> {
    valid_id(id)?;
    valid_id(server_id)?;
    if !bounded_text(label, 256) {
        return Err(RemoteError::Invalid("label must be bounded nonempty text"));
    }
    Ok(())
}

fn reviewed_identity(
    server_id: &str,
    origin: &str,
    credentials: &RemoteCredentials,
) -> Result<ReviewedRemoteIdentity, RemoteError> {
    Ok(ReviewedRemoteIdentity {
        server_id: server_id.to_string(),
        ca_fingerprint: credentials.trust_fingerprint(origin)?,
    })
}

fn check_review(binding: &Binding, request: &ReviewRemote) -> Result<(), RemoteError> {
    if binding.revision != request.expected_revision
        || binding.identity != request.previous_identity
    {
        return Err(RemoteError::Conflict);
    }
    Ok(())
}

fn read_credentials(
    binding: &Binding,
    secrets: &dyn SecretStore,
) -> Result<RemoteCredentials, RemoteFailure> {
    let reference = binding
        .credential_ref
        .as_deref()
        .ok_or(RemoteFailure::MissingCredentials)?;
    let encoded = secrets
        .get(reference)
        .ok()
        .flatten()
        .ok_or(RemoteFailure::MissingCredentials)?;
    serde_json::from_str(&encoded).map_err(|_| RemoteFailure::MissingCredentials)
}

fn verified_probe(
    transport: &dyn RemoteTransport,
    origin: &str,
    credentials: &RemoteCredentials,
    expected: &ReviewedRemoteIdentity,
) -> Result<RemoteSnapshot, RemoteFailure> {
    let snapshot = transport.probe(origin, credentials)?;
    snapshot.identity.validate()?;
    if snapshot.identity.server_id != expected.server_id {
        return Err(RemoteFailure::IdentityChanged);
    }
    validate_remote_checkouts(&snapshot.checkouts)?;
    Ok(snapshot)
}

fn update_observation(
    binding: &mut Binding,
    result: Result<&RemoteSnapshot, RemoteFailure>,
    now_ms: u64,
) {
    binding.observation_generation = ulid::Ulid::new().to_string();
    binding.checked_at_ms = Some(now_ms);
    match result {
        Ok(snapshot) => {
            binding.last_failure = None;
            binding.confirmed_at_ms = Some(now_ms);
            binding.checkouts = snapshot.checkouts.clone();
        }
        Err(failure) => binding.last_failure = Some(failure),
    }
}

fn same_observation(current: &Binding, before: &Binding) -> bool {
    current.revision == before.revision
        && current.credential_ref == before.credential_ref
        && current.revoked == before.revoked
        && current.observation_generation == before.observation_generation
}

fn discovery(
    binding: &Binding,
    file: &RegistryFile,
    checkouts: Option<Vec<RemoteCheckout>>,
) -> RemoteDiscovery {
    let mut remote = binding.view(file);
    remote.stale = checkouts.is_none();
    RemoteDiscovery { remote, checkouts }
}
