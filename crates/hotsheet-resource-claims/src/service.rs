//! The one resource-claim workflow behind `hotsheet-cli resource`, the
//! `hotsheet_resource_claim` MCP tool, and the server's `/resource-claims` routes
//! (HS2-FMHH9Y). Each surface is a thin adapter over [`ClaimService`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::backend::ClaimBackend;
use crate::cancel::WaitCancel;
use crate::config::ResolvedBackend;
use crate::error::ClaimError;
use crate::local::LocalBackend;
use crate::model::{AcquireRequest, Claim, Grant, Namespace, ResourceRef, WaitOutcome};
use crate::token::parse_renewal_token;

/// Route prefix of the secret-authenticated claim routes.
pub const ROUTE_PREFIX: &str = "/resource-claims";
/// Route prefix of the routes a renewal token alone authorizes.
pub const TOKEN_ROUTE_PREFIX: &str = "/resource-claim-token";

/// The backend that serves a machine-scoped namespace: the configuration the machine
/// registry binds it to (HS2-JJWESD), else the built-in local backend under `home`.
/// [`crate::resolve`] also consults a project's registry.
pub fn resolve_backend(
    home: &Path,
    namespace: &Namespace,
) -> Result<Arc<dyn ClaimBackend>, ClaimError> {
    Ok(crate::config::resolve(home, None, namespace)?.backend)
}

/// HS2-YDZAFH: every namespace this machine's claim backend keeps state for, sorted.
/// Namespaces are machine-scoped, so the machine backend answers for all of them.
pub fn held_namespaces(home: &Path) -> Result<Vec<Namespace>, ClaimError> {
    LocalBackend::machine(home).namespaces()
}

/// The live claim on `<namespace>/<resource>`, if any, without acquiring it. A scheduler
/// counts `Some` as unavailable capacity until `expires_at_ms`.
pub fn held(home: &Path, target: &ResourceRef) -> Result<Option<Claim>, ClaimError> {
    resolve_backend(home, target.namespace())?.get(target)
}

/// Block until the resource is free or `timeout` passes, without acquiring it. Woken by
/// the backend's release signal or the holder's expiry; never polls.
pub fn wait_until_free(
    home: &Path,
    target: &ResourceRef,
    timeout: Duration,
) -> Result<WaitOutcome, ClaimError> {
    resolve_backend(home, target.namespace())?.wait_free(target, timeout)
}

/// Parse a duration such as `90s`, `30m`, `1h30m`, `2d`, or `500ms`.
pub fn parse_duration(text: &str) -> Result<Duration, ClaimError> {
    let invalid = || {
        ClaimError::Invalid(format!(
            "'{text}' is not a duration such as 90s, 30m, 1h30m, or 500ms"
        ))
    };
    let mut total_ms: u64 = 0;
    let mut rest = text.trim();
    if rest.is_empty() {
        return Err(invalid());
    }
    while !rest.is_empty() {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let unit_len = rest[digits..]
            .bytes()
            .take_while(u8::is_ascii_alphabetic)
            .count();
        let amount: u64 = rest[..digits].parse().map_err(|_| invalid())?;
        let scale = match &rest[digits..digits + unit_len] {
            "ms" => 1,
            "s" => 1_000,
            "m" => 60_000,
            "h" => 3_600_000,
            "d" => 86_400_000,
            _ => return Err(invalid()),
        };
        total_ms = amount
            .checked_mul(scale)
            .and_then(|part| total_ms.checked_add(part))
            .ok_or_else(invalid)?;
        rest = &rest[digits + unit_len..];
    }
    Ok(Duration::from_millis(total_ms))
}

/// The HTTP status a surface answers a claim error with.
pub fn http_status(error: &ClaimError) -> u16 {
    match error {
        ClaimError::HeldBy { .. } | ClaimError::WaitTimedOut { .. } | ClaimError::NotHolder => 409,
        ClaimError::Expired => 410,
        ClaimError::TokenInvalid => 403,
        ClaimError::BackendUnreachable(_) | ClaimError::Interrupted => 503,
        ClaimError::Unsupported(_) => 501,
        ClaimError::Invalid(_) => 400,
        ClaimError::Capacity(_) => 507,
        ClaimError::Corrupt | ClaimError::Storage(_) => 500,
    }
}

/// Whether a request path belongs to the claim routes [`ClaimService::request`] serves.
pub fn is_path(path: &str) -> bool {
    [ROUTE_PREFIX, TOKEN_ROUTE_PREFIX].iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// The shared workflow, rooted at one machine home.
#[derive(Debug, Clone)]
pub struct ClaimService {
    home: PathBuf,
    project: Option<PathBuf>,
    max_wait: Option<Duration>,
}

impl ClaimService {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            project: None,
            max_wait: None,
        }
    }

    /// Also consult this project's registry (`<project>/.hotsheet2/resource-claims.json`),
    /// whose bindings win over the machine's (HS2-JJWESD).
    pub fn with_project(mut self, project: impl Into<PathBuf>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Cap how long one call may wait. A server sets this so one long-poll request is
    /// bounded; a caller whose own deadline is later asks again.
    pub fn with_max_wait(mut self, max_wait: Duration) -> Self {
        self.max_wait = Some(max_wait);
        self
    }

    /// The backend serving a namespace, with the configuration that chose it.
    pub fn resolve(&self, namespace: &Namespace) -> Result<ResolvedBackend, ClaimError> {
        crate::config::resolve(&self.home, self.project.as_deref(), namespace)
    }

    fn backend(&self, namespace: &Namespace) -> Result<Arc<dyn ClaimBackend>, ClaimError> {
        Ok(self.resolve(namespace)?.backend)
    }

    fn bounded(&self, wait: Duration) -> Duration {
        self.max_wait.map_or(wait, |max| wait.min(max))
    }

    /// Acquire now, or wait up to `wait` when given. `metadata` holds values as typed:
    /// a codec prefix selects the codec, and no prefix stores `plain:`.
    pub fn acquire(
        &self,
        target: &ResourceRef,
        duration: Duration,
        wait: Option<Duration>,
        metadata: &BTreeMap<String, String>,
    ) -> Result<Grant, ClaimError> {
        let backend = self.backend(target.namespace())?;
        let mut encoded = BTreeMap::new();
        if !metadata.is_empty() {
            let codecs = backend.codecs()?;
            for (key, value) in metadata {
                encoded.insert(key.clone(), codecs.encode_input(value)?);
            }
        }
        let request = AcquireRequest::new(duration).with_metadata(encoded);
        match wait {
            None => backend.acquire(target, &request),
            Some(wait) => backend.acquire_wait(target, &request, self.bounded(wait)),
        }
    }

    /// Renew the token's own claim. With `expected`, the token must also be for that
    /// resource; a token for any other claim is [`ClaimError::TokenInvalid`].
    pub fn renew(
        &self,
        renewal_token: &str,
        duration: Duration,
        expected: Option<&ResourceRef>,
    ) -> Result<Claim, ClaimError> {
        let target = token_target(renewal_token, expected)?;
        self.backend(target.namespace())?
            .renew(renewal_token, duration)
    }

    /// Release the token's own claim, with the same `expected` check as [`Self::renew`].
    pub fn release(
        &self,
        renewal_token: &str,
        expected: Option<&ResourceRef>,
    ) -> Result<Claim, ClaimError> {
        let target = token_target(renewal_token, expected)?;
        self.backend(target.namespace())?.release(renewal_token)
    }

    /// The live claim on a resource, if any.
    pub fn show(&self, target: &ResourceRef) -> Result<Option<Claim>, ClaimError> {
        self.backend(target.namespace())?.get(target)
    }

    /// Wait for a resource to become free, without acquiring it.
    pub fn wait_free(
        &self,
        target: &ResourceRef,
        wait: Duration,
    ) -> Result<WaitOutcome, ClaimError> {
        self.backend(target.namespace())?
            .wait_free(target, self.bounded(wait))
    }

    /// HS2-2W2PTX: [`Self::wait_free`] that `cancel` ends early with
    /// [`ClaimError::Interrupted`].
    pub fn wait_free_cancellable(
        &self,
        target: &ResourceRef,
        wait: Duration,
        cancel: &WaitCancel,
    ) -> Result<WaitOutcome, ClaimError> {
        self.backend(target.namespace())?
            .wait_free_cancellable(target, self.bounded(wait), cancel)
    }

    /// Every live claim in a namespace.
    pub fn list(&self, namespace: &Namespace) -> Result<Vec<Claim>, ClaimError> {
        self.backend(namespace)?.list(namespace)
    }

    /// HS2-YDZAFH: every namespace with state on this machine.
    pub fn namespaces(&self) -> Result<Vec<Namespace>, ClaimError> {
        held_namespaces(&self.home)
    }

    fn status_json(&self, target: &ResourceRef, claim: Option<Claim>) -> Result<Value, ClaimError> {
        let resolved = self.resolve(target.namespace())?;
        Ok(json!({
            "target": target.to_string(),
            "held": claim.is_some(),
            "claim": claim,
            "capabilities": resolved.backend.capabilities(),
            "configuration": resolved.configuration,
            "offline": resolved.offline,
        }))
    }

    /// Serve one claim request in the server's route shape. The CLI, the serverless MCP
    /// backend and the server routes all answer through this, so their JSON is identical.
    ///
    /// An acquire that finds the resource held is an ordinary answer, not an error:
    /// `{ "acquired": false, "code": "held_by" | "wait_timeout", "holder": {...} }`.
    pub fn request(&self, method: &str, path: &str, body: &Value) -> Result<Value, ClaimError> {
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        let prefix = |name: &str| name.trim_start_matches('/').to_owned();
        match (method, parts.as_slice()) {
            ("GET", [root, namespace]) if *root == prefix(ROUTE_PREFIX) => {
                let namespace = Namespace::parse(namespace)?;
                let resolved = self.resolve(&namespace)?;
                Ok(json!({
                    "namespace": namespace,
                    "capabilities": resolved.backend.capabilities(),
                    "configuration": resolved.configuration,
                    "offline": resolved.offline,
                    "claims": resolved.backend.list(&namespace)?,
                }))
            }
            ("GET", [root, namespace, resource]) if *root == prefix(ROUTE_PREFIX) => {
                let target = ResourceRef::new(Namespace::parse(namespace)?, resource)?;
                let claim = self.show(&target)?;
                self.status_json(&target, claim)
            }
            ("POST", [root, namespace, resource, action]) if *root == prefix(ROUTE_PREFIX) => {
                let target = ResourceRef::new(Namespace::parse(namespace)?, resource)?;
                match *action {
                    "acquire" => {
                        let duration = required_ms(body, "duration_ms")?;
                        let wait = optional_ms(body, "wait_ms")?;
                        let metadata = metadata_input(body)?;
                        acquire_json(self.acquire(&target, duration, wait, &metadata))
                    }
                    "wait" => {
                        let wait = required_ms(body, "wait_ms")?;
                        let claim = match self.wait_free(&target, wait)? {
                            WaitOutcome::Free => None,
                            WaitOutcome::TimedOut { holder } => Some(*holder),
                        };
                        self.status_json(&target, claim)
                    }
                    "renew" => {
                        let token = required_str(body, "renewal_token")?;
                        let duration = required_ms(body, "duration_ms")?;
                        Ok(json!({ "claim": self.renew(token, duration, Some(&target))? }))
                    }
                    "release" => {
                        let token = required_str(body, "renewal_token")?;
                        let claim = self.release(token, Some(&target))?;
                        Ok(json!({ "released": true, "claim": claim }))
                    }
                    _ => Err(unknown_route()),
                }
            }
            ("POST", [root, action]) if *root == prefix(TOKEN_ROUTE_PREFIX) => {
                let token = required_str(body, "renewal_token")?;
                match *action {
                    "renew" => {
                        let duration = required_ms(body, "duration_ms")?;
                        Ok(json!({ "claim": self.renew(token, duration, None)? }))
                    }
                    "release" => {
                        Ok(json!({ "released": true, "claim": self.release(token, None)? }))
                    }
                    _ => Err(unknown_route()),
                }
            }
            _ => Err(unknown_route()),
        }
    }
}

/// The JSON answer for an acquire: the grant, or the refusal naming the holder.
pub(crate) fn acquire_json(result: Result<Grant, ClaimError>) -> Result<Value, ClaimError> {
    match result {
        Ok(grant) => {
            let mut value = serde_json::to_value(&grant).map_err(|_| ClaimError::Corrupt)?;
            value["acquired"] = Value::Bool(true);
            Ok(value)
        }
        Err(error @ (ClaimError::HeldBy { .. } | ClaimError::WaitTimedOut { .. })) => Ok(json!({
            "acquired": false,
            "code": error.code(),
            "error": error.to_string(),
            "holder": error.holder(),
        })),
        Err(error) => Err(error),
    }
}

fn token_target(
    renewal_token: &str,
    expected: Option<&ResourceRef>,
) -> Result<ResourceRef, ClaimError> {
    let target = parse_renewal_token(renewal_token)?.target;
    match expected {
        Some(expected) if *expected != target => Err(ClaimError::TokenInvalid),
        _ => Ok(target),
    }
}

fn unknown_route() -> ClaimError {
    ClaimError::Unsupported("no such resource-claim route".into())
}

fn required_str<'a>(body: &'a Value, name: &str) -> Result<&'a str, ClaimError> {
    body.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ClaimError::Invalid(format!("{name} is required")))
}

fn optional_ms(body: &Value, name: &str) -> Result<Option<Duration>, ClaimError> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(|ms| Some(Duration::from_millis(ms)))
            .ok_or_else(|| {
                ClaimError::Invalid(format!("{name} must be a whole number of milliseconds"))
            }),
    }
}

fn required_ms(body: &Value, name: &str) -> Result<Duration, ClaimError> {
    optional_ms(body, name)?.ok_or_else(|| ClaimError::Invalid(format!("{name} is required")))
}

fn metadata_input(body: &Value) -> Result<BTreeMap<String, String>, ClaimError> {
    let invalid = || ClaimError::Invalid("metadata must be an object of strings".into());
    match body.get("metadata") {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(entries)) => entries
            .iter()
            .map(|(key, value)| Ok((key.clone(), value.as_str().ok_or_else(invalid)?.to_owned())))
            .collect(),
        Some(_) => Err(invalid()),
    }
}
