//! The backend an external plugin serves.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Map, Value, json};

use hotsheet_ticketing::claim_backends::BackendConfiguration;
use hotsheet_ticketing::secrets::SecretStore;

use super::approval::{self, plugin_root};
use super::manifest::{ManifestWait, PluginManifest, SettingType, valid_plugin_id};
use super::runner::{CommandRunner, LiveRunner, RunError, tail};
use crate::backend::ClaimBackend;
use crate::cancel::WaitCancel;
use crate::codec::{KeyProvider, MetadataCodecs};
use crate::config::CredentialKeyProvider;
use crate::error::ClaimError;
use crate::model::{
    AcquireRequest, Capabilities, Claim, Grant, Guarantee, Namespace, ResourceRef, WaitOutcome,
    lease_ms, rfc3339, validate_metadata, wait_ms,
};
use crate::token::{self, parse_renewal_token, sha256_hex};

/// The envelope protocol version Hot Sheet writes.
pub const PROTOCOL_VERSION: u32 = 1;
const MAX_ERROR_MESSAGE: usize = 500;
const MAX_CLAIM_ID_LEN: usize = 64;

#[derive(Debug, Deserialize)]
struct Reply {
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<WireError>,
}

#[derive(Debug, Deserialize)]
struct WireError {
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    holder: Option<WireClaim>,
}

#[derive(Debug, Deserialize)]
struct WireClaim {
    claim_id: String,
    fencing_token: u64,
    acquired_at_ms: u64,
    expires_at_ms: u64,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct WireListed {
    resource: String,
    #[serde(flatten)]
    claim: WireClaim,
}

/// The OS credential-store account of one secret setting of one configuration.
pub fn secret_account(configuration: &str, key: &str) -> String {
    format!("resource-claims.{configuration}.{key}")
}

/// Load the plugin a configuration names, without approving or running it.
pub fn load_plugin(
    home: &Path,
    configuration: &BackendConfiguration,
) -> Result<PluginManifest, ClaimError> {
    let id = configuration.setting("plugin").ok_or_else(|| {
        ClaimError::Invalid(format!(
            "configuration '{}' needs --setting plugin=<id>",
            configuration.id
        ))
    })?;
    if !valid_plugin_id(id) {
        return Err(ClaimError::Invalid(format!(
            "plugin id '{id}' is not valid"
        )));
    }
    let dir = plugin_root(home).join(id);
    if !dir.is_dir() {
        return Err(ClaimError::Unsupported(format!(
            "no claim-backend plugin '{id}': expected a directory {} with a manifest.toml",
            dir.display()
        )));
    }
    PluginManifest::load(&dir).map_err(ClaimError::Unsupported)
}

/// Check a configuration's settings against its plugin's manifest: no unknown keys, no
/// secret-typed key in a file that may be checked in, choices honoured, and every
/// required plain setting present. Secret settings are checked where they are used.
pub fn check_settings(
    manifest: &PluginManifest,
    configuration: &BackendConfiguration,
) -> Result<(), ClaimError> {
    for (key, value) in &configuration.settings {
        if key == "plugin" {
            continue;
        }
        let Some(spec) = manifest.setting(key) else {
            let known: Vec<&str> = manifest.settings.iter().map(|s| s.key.as_str()).collect();
            return Err(ClaimError::Invalid(format!(
                "plugin '{}' has no setting '{key}'; it takes {}",
                manifest.id,
                if known.is_empty() {
                    "none".to_owned()
                } else {
                    known.join(", ")
                }
            )));
        };
        match spec.kind {
            SettingType::Secret => {
                return Err(ClaimError::Invalid(format!(
                    "setting '{key}' is a secret: never put it in a configuration; run `hotsheet-cli resource backend secret {} {key}` and give the value on standard input",
                    configuration.id
                )));
            }
            SettingType::Choice if !spec.choices.contains(value) => {
                return Err(ClaimError::Invalid(format!(
                    "setting '{key}' must be one of {}",
                    spec.choices.join(", ")
                )));
            }
            _ => {}
        }
    }
    for spec in &manifest.settings {
        if spec.required
            && spec.kind != SettingType::Secret
            && spec.default.is_none()
            && !configuration.settings.contains_key(&spec.key)
        {
            return Err(ClaimError::Invalid(format!(
                "plugin '{}' needs setting '{}' ({}); add --setting {}=<value>",
                manifest.id, spec.key, spec.label, spec.key
            )));
        }
    }
    Ok(())
}

/// The settings object a plugin receives on stdin: plain settings from the
/// configuration, secrets from the credential store, defaults filled in.
pub fn resolve_settings<S: SecretStore + ?Sized>(
    manifest: &PluginManifest,
    configuration: &BackendConfiguration,
    secrets: &S,
) -> Result<Value, ClaimError> {
    check_settings(manifest, configuration)?;
    let mut resolved = Map::new();
    for spec in &manifest.settings {
        let value = match spec.kind {
            SettingType::Secret => secrets
                .get(&secret_account(&configuration.id, &spec.key))
                .map_err(|e| ClaimError::BackendUnreachable(format!("OS credential store: {e}")))?,
            _ => configuration
                .settings
                .get(&spec.key)
                .cloned()
                .or_else(|| spec.default.clone()),
        };
        match value {
            Some(value) => {
                resolved.insert(spec.key.clone(), Value::String(value));
            }
            None if spec.required => {
                return Err(ClaimError::Unsupported(format!(
                    "plugin '{}' needs the secret '{}': run `hotsheet-cli resource backend secret {} {}` and give the value on standard input",
                    manifest.id, spec.key, configuration.id, spec.key
                )));
            }
            None => {}
        }
    }
    Ok(Value::Object(resolved))
}

/// A credential store shared between the settings reader and the key provider.
#[derive(Clone)]
pub struct SharedSecrets(pub Arc<dyn SecretStore + Send + Sync>);

impl SecretStore for SharedSecrets {
    fn set(
        &self,
        account: &str,
        secret: &str,
    ) -> Result<(), hotsheet_ticketing::secrets::SecretError> {
        self.0.set(account, secret)
    }
    fn get(
        &self,
        account: &str,
    ) -> Result<Option<String>, hotsheet_ticketing::secrets::SecretError> {
        self.0.get(account)
    }
    fn delete(&self, account: &str) -> Result<bool, hotsheet_ticketing::secrets::SecretError> {
        self.0.delete(account)
    }
}

/// The backend a configuration of kind `external` selects, reading secrets from the OS
/// credential store. The plugin must be approved as it stands; every operation checks
/// that again.
pub fn backend_for(
    home: &Path,
    configuration: &BackendConfiguration,
) -> Result<Arc<dyn ClaimBackend>, ClaimError> {
    backend_with(
        home,
        configuration,
        SharedSecrets(Arc::new(hotsheet_ticketing::OsKeychain::at(home))),
    )
}

/// [`backend_for`] over an explicit credential store (tests inject a synthetic one).
pub fn backend_with(
    home: &Path,
    configuration: &BackendConfiguration,
    secrets: SharedSecrets,
) -> Result<Arc<dyn ClaimBackend>, ClaimError> {
    let manifest = load_plugin(home, configuration)?;
    approval::require(home, &manifest.id, &manifest.dir)?;
    let settings = resolve_settings(&manifest, configuration, &secrets)?;
    let keys: Option<Arc<dyn KeyProvider>> = match configuration.credential {
        Some(_) => Some(Arc::new(CredentialKeyProvider::new(
            secrets,
            configuration,
        )?)),
        None => None,
    };
    Ok(Arc::new(ExternalBackend::new(
        home,
        manifest,
        &configuration.id,
        settings,
        keys,
        Arc::new(LiveRunner),
    )))
}

pub struct ExternalBackend {
    home: std::path::PathBuf,
    manifest: PluginManifest,
    configuration: String,
    /// Resolved settings, secrets included. Written to the plugin's stdin and nowhere else.
    settings: Value,
    keys: Option<Arc<dyn KeyProvider>>,
    runner: Arc<dyn CommandRunner>,
}

impl std::fmt::Debug for ExternalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalBackend")
            .field("plugin", &self.manifest.id)
            .field("configuration", &self.configuration)
            .finish_non_exhaustive()
    }
}

fn clean(message: &str) -> String {
    let text: String = message.chars().filter(|c| !c.is_control()).collect();
    text.chars().take(MAX_ERROR_MESSAGE).collect()
}

impl ExternalBackend {
    pub fn new(
        home: &Path,
        manifest: PluginManifest,
        configuration: &str,
        settings: Value,
        keys: Option<Arc<dyn KeyProvider>>,
        runner: Arc<dyn CommandRunner>,
    ) -> Self {
        Self {
            home: home.to_path_buf(),
            manifest,
            configuration: configuration.to_owned(),
            settings,
            keys,
            runner,
        }
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// Run one operation. The plugin's approval is re-checked first, so edited code never
    /// runs.
    fn call(&self, op: &str, request: Value, timeout: Duration) -> Result<Value, ClaimError> {
        self.call_with(op, None, request, timeout, None)
    }

    /// [`Self::call`] about `target`, so a `held_by` holder attaches to it.
    fn call_for(
        &self,
        target: &ResourceRef,
        op: &str,
        request: Value,
        timeout: Duration,
    ) -> Result<Value, ClaimError> {
        self.call_with(op, Some(target), request, timeout, None)
    }

    fn call_with(
        &self,
        op: &str,
        target: Option<&ResourceRef>,
        request: Value,
        timeout: Duration,
        cancel: Option<&WaitCancel>,
    ) -> Result<Value, ClaimError> {
        approval::require(&self.home, &self.manifest.id, &self.manifest.dir)?;
        let input = json!({
            "protocol": PROTOCOL_VERSION,
            "backend": self.manifest.id,
            "configuration": { "id": self.configuration },
            "op": op,
            "settings": self.settings,
            "request": request,
        })
        .to_string();
        let (program, args) = (&self.manifest.program, &self.manifest.args);
        let output = match cancel {
            None => self.runner.run(program, args, input.as_bytes(), timeout),
            Some(cancel) => {
                self.runner
                    .run_cancellable(program, args, input.as_bytes(), timeout, cancel)
            }
        }
        .map_err(|e| match e {
            RunError::Cancelled { .. } => ClaimError::Interrupted,
            e => ClaimError::BackendUnreachable(format!("plugin '{}': {e}", self.manifest.id)),
        })?;
        if output.left_running {
            // HS2-BR14FM: the plugin's answer is still used; its leftovers were ended.
            // Identifiers only: no settings, request or output.
            tracing::warn!(
                plugin = %self.manifest.id,
                configuration = %self.configuration,
                op,
                "claim plugin exited but left a process running that held its output; \
                 that process was stopped"
            );
        }
        let reply: Option<Reply> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str(line.trim()).ok());
        let Some(reply) = reply else {
            return Err(ClaimError::BackendUnreachable(format!(
                "plugin '{}' exited with {} without a result: {}",
                self.manifest.id,
                output.code.map_or("a signal".into(), |c| c.to_string()),
                clean(&tail(&output.stderr, 300))
            )));
        };
        if reply.ok && !output.success {
            return Err(ClaimError::BackendUnreachable(format!(
                "plugin '{}' reported success but exited with {}",
                self.manifest.id,
                output.code.map_or("a signal".into(), |c| c.to_string())
            )));
        }
        if reply.ok {
            return Ok(reply.result);
        }
        let error = reply.error.unwrap_or(WireError {
            code: "unknown".into(),
            message: "the plugin reported a failure".into(),
            holder: None,
        });
        Err(self.map_error(target, error))
    }

    fn map_error(&self, target: Option<&ResourceRef>, error: WireError) -> ClaimError {
        let message = clean(&error.message);
        let described = |what: &str| {
            if message.is_empty() {
                what.to_owned()
            } else {
                format!("{what}: {message}")
            }
        };
        match error.code.as_str() {
            "held_by" => match error.holder {
                Some(holder) => match self.claim_from(target, holder) {
                    Ok(holder) => ClaimError::HeldBy {
                        holder: Box::new(holder),
                    },
                    Err(error) => error,
                },
                None => ClaimError::Invalid(format!(
                    "plugin '{}' reported held_by without a holder",
                    self.manifest.id
                )),
            },
            "expired" => ClaimError::Expired,
            "not_holder" => ClaimError::NotHolder,
            "token_invalid" => ClaimError::TokenInvalid,
            "backend_unreachable" => ClaimError::BackendUnreachable(described("plugin backend")),
            "unsupported" => ClaimError::Unsupported(described("plugin")),
            "invalid" => ClaimError::Invalid(described("plugin")),
            "capacity" => ClaimError::Capacity(described("plugin")),
            other => ClaimError::Storage(std::io::Error::other(format!(
                "plugin '{}' error '{}': {message}",
                self.manifest.id,
                clean(other)
            ))),
        }
    }

    /// Validate a claim a plugin reported. A holder whose target is not known here (a
    /// `held_by` error) is read against the request's own target by the caller.
    fn claim_from(
        &self,
        target: Option<&ResourceRef>,
        wire: WireClaim,
    ) -> Result<Claim, ClaimError> {
        let bad = |what: &str| {
            ClaimError::Invalid(format!(
                "plugin '{}' returned an invalid claim: {what}",
                self.manifest.id
            ))
        };
        if wire.claim_id.is_empty()
            || wire.claim_id.len() > MAX_CLAIM_ID_LEN
            || wire.claim_id.chars().any(char::is_control)
        {
            return Err(bad("claim_id"));
        }
        if self.manifest.guarantee == Guarantee::ExclusiveWithFencing && wire.fencing_token == 0 {
            return Err(bad(
                "an exclusive backend must return a fencing_token above 0",
            ));
        }
        if wire.expires_at_ms < wire.acquired_at_ms {
            return Err(bad("expires_at_ms before acquired_at_ms"));
        }
        validate_metadata(&wire.metadata).map_err(|_| bad("metadata"))?;
        let target = target.ok_or_else(|| bad("no target"))?;
        Ok(Claim {
            namespace: target.namespace().clone(),
            resource: target.resource().to_owned(),
            claim_id: wire.claim_id,
            fencing_token: wire.fencing_token,
            guarantee: self.manifest.guarantee,
            acquired_at_ms: wire.acquired_at_ms,
            expires_at_ms: wire.expires_at_ms,
            expires_at: rfc3339(wire.expires_at_ms),
            metadata: wire.metadata,
        })
    }

    fn claim_result(&self, target: &ResourceRef, result: &Value) -> Result<Claim, ClaimError> {
        let claim = result.get("claim").cloned().unwrap_or(Value::Null);
        let wire: WireClaim = serde_json::from_value(claim).map_err(|_| {
            ClaimError::Invalid(format!("plugin '{}' returned no claim", self.manifest.id))
        })?;
        self.claim_from(Some(target), wire)
    }

    /// [`ClaimBackend::wait_free`], ended early by `cancel` (HS2-P234BH). The protocol
    /// already ends a plugin process by stopping it (an operation that outlives its timeout
    /// is killed), so a cancel stops the `wait` process the same way and its answer is
    /// never read: no protocol change, and a plugin must already tolerate being stopped.
    fn wait_until(
        &self,
        target: &ResourceRef,
        timeout: Duration,
        cancel: Option<&WaitCancel>,
    ) -> Result<WaitOutcome, ClaimError> {
        if cancel.is_some_and(WaitCancel::is_cancelled) {
            return Err(ClaimError::Interrupted);
        }
        let timeout_ms = wait_ms(timeout)?;
        if self.manifest.wait == ManifestWait::None || timeout.is_zero() {
            return match self.get(target)? {
                None => Ok(WaitOutcome::Free),
                Some(holder) if timeout.is_zero() => Ok(WaitOutcome::TimedOut {
                    holder: Box::new(holder),
                }),
                Some(_) => Err(ClaimError::Unsupported(format!(
                    "plugin '{}' cannot wait; check again later",
                    self.manifest.id
                ))),
            };
        }
        // One blocking invocation: the plugin parks on its own push signal. The process
        // may run as long as the wait plus its ordinary operation allowance.
        let result = self.call_with(
            "wait",
            Some(target),
            json!({ "target": target.to_string(), "timeout_ms": timeout_ms }),
            timeout + self.timeout(),
            cancel,
        )?;
        if result.get("free").and_then(Value::as_bool) == Some(true) {
            return Ok(WaitOutcome::Free);
        }
        let holder: WireClaim = serde_json::from_value(
            result.get("holder").cloned().unwrap_or(Value::Null),
        )
        .map_err(|_| {
            ClaimError::Invalid(format!(
                "plugin '{}' ended a wait without saying free or naming the holder",
                self.manifest.id
            ))
        })?;
        Ok(WaitOutcome::TimedOut {
            holder: Box::new(self.claim_from(Some(target), holder)?),
        })
    }

    fn timeout(&self) -> Duration {
        self.manifest.timeout
    }
}

impl ClaimBackend for ExternalBackend {
    fn capabilities(&self) -> Capabilities {
        self.manifest.capabilities()
    }

    fn codecs(&self) -> Result<MetadataCodecs, ClaimError> {
        let keys = self.keys.as_ref().ok_or_else(|| {
            ClaimError::Unsupported(format!(
                "configuration '{}' has no credential, so only plain: metadata is accepted",
                self.configuration
            ))
        })?;
        Ok(MetadataCodecs::new(&keys.key()?)?)
    }

    fn acquire(&self, target: &ResourceRef, request: &AcquireRequest) -> Result<Grant, ClaimError> {
        validate_metadata(&request.metadata)?;
        if request.metadata.len() > self.manifest.max_metadata_entries {
            return Err(ClaimError::Invalid(format!(
                "plugin '{}' accepts at most {} metadata entries",
                self.manifest.id, self.manifest.max_metadata_entries
            )));
        }
        let lease = lease_ms(request.duration)?;
        if request.duration > self.manifest.max_lease {
            return Err(ClaimError::Invalid(format!(
                "plugin '{}' allows a lease of at most {} s",
                self.manifest.id,
                self.manifest.max_lease.as_secs()
            )));
        }
        let claim_id = ulid::Ulid::new().to_string();
        let renewal_token = token::mint(target, &claim_id)?;
        let result = self.call_for(
            target,
            "acquire",
            json!({
                "target": target.to_string(),
                "claim_id": claim_id,
                "token_digest": sha256_hex(&renewal_token),
                "duration_ms": lease,
                "metadata": request.metadata,
            }),
            self.timeout(),
        )?;
        let claim = self.claim_result(target, &result)?;
        if claim.claim_id != claim_id {
            return Err(ClaimError::Invalid(format!(
                "plugin '{}' granted a claim other than the one requested",
                self.manifest.id
            )));
        }
        Ok(Grant {
            claim,
            renewal_token: renewal_token.into(),
        })
    }

    fn acquire_wait(
        &self,
        target: &ResourceRef,
        request: &AcquireRequest,
        timeout: Duration,
    ) -> Result<Grant, ClaimError> {
        wait_ms(timeout)?;
        let deadline = Instant::now() + timeout;
        loop {
            match self.acquire(target, request) {
                Err(ClaimError::HeldBy { holder }) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(ClaimError::WaitTimedOut { holder });
                    }
                    if self.manifest.wait == ManifestWait::None {
                        return Err(ClaimError::Unsupported(format!(
                            "plugin '{}' cannot wait; acquire again later",
                            self.manifest.id
                        )));
                    }
                    if let WaitOutcome::TimedOut { holder } = self.wait_free(target, remaining)? {
                        return Err(ClaimError::WaitTimedOut { holder });
                    }
                }
                other => return other,
            }
        }
    }

    fn renew(&self, renewal_token: &str, duration: Duration) -> Result<Claim, ClaimError> {
        let lease = lease_ms(duration)?;
        let parsed = parse_renewal_token(renewal_token)?;
        let result = self.call_for(
            &parsed.target,
            "renew",
            json!({
                "target": parsed.target.to_string(),
                "claim_id": parsed.claim_id,
                "token_digest": sha256_hex(renewal_token),
                "duration_ms": lease,
            }),
            self.timeout(),
        )?;
        self.claim_result(&parsed.target, &result)
    }

    fn release(&self, renewal_token: &str) -> Result<Claim, ClaimError> {
        let parsed = parse_renewal_token(renewal_token)?;
        let result = self.call_for(
            &parsed.target,
            "release",
            json!({
                "target": parsed.target.to_string(),
                "claim_id": parsed.claim_id,
                "token_digest": sha256_hex(renewal_token),
            }),
            self.timeout(),
        )?;
        self.claim_result(&parsed.target, &result)
    }

    fn get(&self, target: &ResourceRef) -> Result<Option<Claim>, ClaimError> {
        let result = self.call_for(
            target,
            "get",
            json!({ "target": target.to_string() }),
            self.timeout(),
        )?;
        match result.get("claim") {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.claim_result(target, &result).map(Some),
        }
    }

    fn list(&self, namespace: &Namespace) -> Result<Vec<Claim>, ClaimError> {
        let result = self.call(
            "list",
            json!({ "namespace": namespace.as_str() }),
            self.timeout(),
        )?;
        let listed: Vec<WireListed> = serde_json::from_value(
            result.get("claims").cloned().unwrap_or(Value::Null),
        )
        .map_err(|_| {
            ClaimError::Invalid(format!(
                "plugin '{}' returned no claim list",
                self.manifest.id
            ))
        })?;
        let mut claims = Vec::new();
        for entry in listed {
            let target = ResourceRef::new(namespace.clone(), &entry.resource)?;
            claims.push(self.claim_from(Some(&target), entry.claim)?);
        }
        claims.sort_by(|a, b| a.resource.cmp(&b.resource));
        Ok(claims)
    }

    fn namespaces(&self) -> Result<Vec<Namespace>, ClaimError> {
        let result = self.call("namespaces", json!({}), self.timeout())?;
        let names: Vec<String> =
            serde_json::from_value(result.get("namespaces").cloned().unwrap_or(Value::Null))
                .map_err(|_| {
                    ClaimError::Invalid(format!(
                        "plugin '{}' returned no namespace list",
                        self.manifest.id
                    ))
                })?;
        let mut parsed = names
            .iter()
            .map(|name| Namespace::parse(name))
            .collect::<Result<Vec<_>, _>>()?;
        parsed.sort();
        parsed.dedup();
        Ok(parsed)
    }

    fn wait_free(
        &self,
        target: &ResourceRef,
        timeout: Duration,
    ) -> Result<WaitOutcome, ClaimError> {
        self.wait_until(target, timeout, None)
    }

    fn wait_free_cancellable(
        &self,
        target: &ResourceRef,
        timeout: Duration,
        cancel: &WaitCancel,
    ) -> Result<WaitOutcome, ClaimError> {
        self.wait_until(target, timeout, Some(cancel))
    }

    fn verify_fencing(&self, target: &ResourceRef, fencing_token: u64) -> Result<(), ClaimError> {
        self.call_for(
            target,
            "verify_fencing",
            json!({ "target": target.to_string(), "fencing_token": fencing_token }),
            self.timeout(),
        )
        .map(|_| ())
    }
}
