//! The built-in Upstash Redis REST claim backend (HS2-K9J1RK, `docs/36` §36.17).
//!
//! A resource is a Redis hash that holds its claim; acquire, renew and release are Lua
//! scripts, so each is one atomic step on the Redis clock. A separate counter per resource
//! hands out the fencing token and is never expired. Waiting holds one `subscribe` stream
//! (server-sent events) open: a release publishes on the resource's channel, and expiry is
//! one read timeout to the instant Redis reports. Nothing polls on a timer; a failed stream
//! backs off in bounded steps and resubscribes, re-reading state each time, so a release
//! that landed in the gap is never missed.
//!
//! HS2-P234BH: a cancellable wait reads its stream on a helper thread and blocks on a
//! channel that the [`WaitCancel`] waker also feeds, so a cancel ends the wait at once.
//! `ureq` exposes no handle to close a response's socket from another thread, so the
//! abandoned stream ends on its own: after its next event or at its lifetime, whichever
//! comes first, without reading state or subscribing again.

pub mod scripts;
pub mod transport;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use hotsheet_ticketing::claim_backends::BackendConfiguration;
use hotsheet_ticketing::secrets::SecretStore;

use crate::backend::ClaimBackend;
use crate::cancel::WaitCancel;
use crate::codec::{KeyProvider, MetadataCodecs};
use crate::config::CredentialKeyProvider;
use crate::error::ClaimError;
use crate::external::{SharedSecrets, secret_account};
use crate::model::{
    AcquireRequest, Capabilities, Claim, Grant, Guarantee, MAX_LEASE, MAX_METADATA_BYTES,
    MAX_METADATA_ENTRIES, Namespace, ResourceRef, WaitMechanism, WaitOutcome, lease_ms, rfc3339,
    validate_metadata, wait_ms,
};
use crate::token::{self, parse_renewal_token, sha256_hex};
use transport::{Event, RestError, RestTransport, UreqTransport, validate_url};

/// The default key prefix; several deployments may share one database by choosing others.
pub const DEFAULT_PREFIX: &str = "hs2rc";
/// The credential-store setting that holds the REST token.
pub const TOKEN_SETTING: &str = "token";
const RATE_LIMIT_ATTEMPTS: u32 = 3;
const MAX_RETRY_WAIT: Duration = Duration::from_secs(2);
const BACKOFF_START: Duration = Duration::from_millis(250);
const BACKOFF_CAP: Duration = Duration::from_secs(5);
const MAX_STREAM_FAILURES: u32 = 6;
/// Slack after the instant Redis reports, so the stream ends just past the expiry.
const EXPIRY_SLACK: Duration = Duration::from_millis(150);

/// A key prefix: short, lowercase, and free of the `:` that separates key parts.
pub fn valid_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix.len() <= 32
        && prefix
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[derive(Clone)]
pub struct UpstashBackend {
    configuration: String,
    prefix: String,
    transport: Arc<dyn RestTransport>,
    keys: Option<Arc<dyn KeyProvider>>,
}

impl std::fmt::Debug for UpstashBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpstashBackend")
            .field("configuration", &self.configuration)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

/// What one stream's watch ended with: `Some(Free)`, `None` (read again), or a failure.
type Watched = Result<Option<WaitOutcome>, RestError>;

/// What a cancellable wait hears on its channel (HS2-P234BH).
enum Signal {
    /// The watch of the stream with this generation ended.
    Stream(u64, Watched),
    Cancelled,
}

/// What reading a resource found.
enum State {
    Free,
    /// The holder and how long Redis says it has left.
    Held(Claim, Duration),
}

impl UpstashBackend {
    pub fn new(
        configuration: &str,
        prefix: &str,
        transport: Arc<dyn RestTransport>,
        keys: Option<Arc<dyn KeyProvider>>,
    ) -> Self {
        Self {
            configuration: configuration.to_owned(),
            prefix: prefix.to_owned(),
            transport,
            keys,
        }
    }

    // Key layout (documented in docs/36 §36.17). Every part is a validated name, so no key
    // holds customer content, only the namespace and resource names the caller chose.
    fn claim_key(&self, t: &ResourceRef) -> String {
        format!("{}:v1:{}:c:{}", self.prefix, t.namespace(), t.resource())
    }
    fn fence_key(&self, t: &ResourceRef) -> String {
        format!("{}:v1:{}:f:{}", self.prefix, t.namespace(), t.resource())
    }
    fn resources_key(&self, ns: &Namespace) -> String {
        format!("{}:v1:{ns}:r", self.prefix)
    }
    fn namespaces_key(&self) -> String {
        format!("{}:v1:namespaces", self.prefix)
    }
    fn channel(&self, t: &ResourceRef) -> String {
        format!("{}:v1:{}:ch:{}", self.prefix, t.namespace(), t.resource())
    }

    fn error(&self, error: RestError) -> ClaimError {
        match error {
            RestError::Unreachable(message) => ClaimError::BackendUnreachable(format!(
                "configuration '{}': {message}",
                self.configuration
            )),
            RestError::RateLimited { .. } => ClaimError::BackendUnreachable(format!(
                "configuration '{}' is rate limited by the service; try again shortly",
                self.configuration
            )),
            RestError::Unauthorized => ClaimError::Unsupported(format!(
                "the service rejected the token of configuration '{0}'; run `hotsheet-cli resource backend secret {0} {TOKEN_SETTING}` with a valid REST token on standard input",
                self.configuration
            )),
            RestError::Command(message) => ClaimError::Storage(std::io::Error::other(format!(
                "the service refused a command: {message}"
            ))),
            RestError::Protocol(message) => ClaimError::Storage(std::io::Error::other(format!(
                "unexpected service reply: {message}"
            ))),
        }
    }

    /// Run `attempt`, retrying a rate limit a bounded number of times after the delay the
    /// service names (capped), so a burst does not fail a claim outright.
    fn retrying<T>(
        &self,
        mut attempt: impl FnMut() -> Result<T, RestError>,
    ) -> Result<T, ClaimError> {
        let mut tries = 0;
        loop {
            match attempt() {
                Err(RestError::RateLimited { retry_after }) if tries + 1 < RATE_LIMIT_ATTEMPTS => {
                    tries += 1;
                    // A bounded backoff after a refusal, not a periodic poll.
                    std::thread::sleep(retry_after.unwrap_or(BACKOFF_START).min(MAX_RETRY_WAIT));
                }
                other => return other.map_err(|e| self.error(e)),
            }
        }
    }

    fn eval(
        &self,
        script: &str,
        keys: &[String],
        argv: &[String],
    ) -> Result<Vec<String>, ClaimError> {
        let mut command = vec![json!("EVAL"), json!(script), json!(keys.len())];
        command.extend(keys.iter().map(|k| json!(k)));
        command.extend(argv.iter().map(|a| json!(a)));
        let reply = self.retrying(|| self.transport.command(&command))?;
        let Value::Array(items) = reply else {
            return Err(self.error(RestError::Protocol("a script answers with an array".into())));
        };
        items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_owned).ok_or_else(|| {
                    self.error(RestError::Protocol("a script reply holds non-text".into()))
                })
            })
            .collect()
    }

    /// A claim from the fields a script returns after its status and `now`.
    fn claim(&self, target: &ResourceRef, fields: &[String]) -> Result<Claim, ClaimError> {
        let bad = || self.error(RestError::Protocol("a claim reply is incomplete".into()));
        let [claim_id, fencing, acquired, expires, metadata] = fields else {
            return Err(bad());
        };
        let metadata: BTreeMap<String, String> =
            serde_json::from_str(metadata).map_err(|_| bad())?;
        let expires_at_ms: u64 = expires.parse().map_err(|_| bad())?;
        Ok(Claim {
            namespace: target.namespace().clone(),
            resource: target.resource().to_owned(),
            claim_id: claim_id.clone(),
            fencing_token: fencing.parse().map_err(|_| bad())?,
            guarantee: Guarantee::ExclusiveWithFencing,
            acquired_at_ms: acquired.parse().map_err(|_| bad())?,
            expires_at_ms,
            expires_at: rfc3339(expires_at_ms),
            metadata,
        })
    }

    fn claim_reply(&self, target: &ResourceRef, reply: &[String]) -> Result<Claim, ClaimError> {
        // ok, now, claim_id, fencing, acquired, expires, metadata
        let fields = reply.get(2..).unwrap_or_default();
        self.claim(target, fields)
    }

    fn status_error(status: &str) -> ClaimError {
        match status {
            "expired" => ClaimError::Expired,
            "not_holder" => ClaimError::NotHolder,
            _ => ClaimError::TokenInvalid,
        }
    }

    fn read_state(&self, target: &ResourceRef) -> Result<State, ClaimError> {
        let reply = self.eval(scripts::GET, &[self.claim_key(target)], &[])?;
        match reply.first().map(String::as_str) {
            Some("free") => Ok(State::Free),
            Some("held") => {
                let claim = self.claim_reply(target, &reply)?;
                let now: u64 = reply
                    .get(1)
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(claim.acquired_at_ms);
                let left = claim.expires_at_ms.saturating_sub(now);
                Ok(State::Held(claim, Duration::from_millis(left)))
            }
            _ => Err(self.error(RestError::Protocol("unknown GET reply".into()))),
        }
    }

    fn members(&self, key: String) -> Result<Vec<String>, ClaimError> {
        let command = vec![json!("SMEMBERS"), json!(key)];
        match self.retrying(|| self.transport.command(&command))? {
            Value::Array(items) => Ok(items
                .iter()
                .filter_map(|i| i.as_str().map(str::to_owned))
                .collect()),
            _ => Err(self.error(RestError::Protocol("SMEMBERS answers with an array".into()))),
        }
    }

    /// A failed stream waits a bounded, doubling interval before the next try.
    fn backoff(failures: u32, remaining: Duration) -> Duration {
        let step = BACKOFF_START
            .saturating_mul(1u32 << failures.min(5))
            .min(BACKOFF_CAP);
        step.min(remaining)
    }
}

impl ClaimBackend for UpstashBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            backend: "upstash_redis".into(),
            guarantee: Guarantee::ExclusiveWithFencing,
            wait: WaitMechanism::PushSignal,
            max_metadata_entries: MAX_METADATA_ENTRIES,
            max_metadata_bytes: MAX_METADATA_BYTES,
            max_lease_ms: u64::try_from(MAX_LEASE.as_millis()).unwrap_or(u64::MAX),
        }
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
        let lease = lease_ms(request.duration)?;
        let claim_id = ulid::Ulid::new().to_string();
        let renewal_token = token::mint(target, &claim_id)?;
        let metadata = serde_json::to_string(&request.metadata)
            .map_err(|e| ClaimError::Invalid(e.to_string()))?;
        let reply = self.eval(
            scripts::ACQUIRE,
            &[
                self.claim_key(target),
                self.fence_key(target),
                self.resources_key(target.namespace()),
                self.namespaces_key(),
            ],
            &[
                claim_id.clone(),
                sha256_hex(&renewal_token),
                lease.to_string(),
                metadata.clone(),
                target.resource().to_owned(),
                target.namespace().to_string(),
                scripts::GRACE_MS.to_string(),
            ],
        )?;
        match reply.first().map(String::as_str) {
            Some("ok") => {
                // ok, now, fencing, acquired, expires
                let [_, _, fencing, acquired, expires] = reply.as_slice() else {
                    return Err(self.error(RestError::Protocol("incomplete acquire reply".into())));
                };
                let claim = self.claim(
                    target,
                    &[
                        claim_id,
                        fencing.clone(),
                        acquired.clone(),
                        expires.clone(),
                        metadata,
                    ],
                )?;
                Ok(Grant {
                    claim,
                    renewal_token: renewal_token.into(),
                })
            }
            Some("held") => Err(ClaimError::HeldBy {
                holder: Box::new(self.claim_reply(target, &reply)?),
            }),
            _ => Err(self.error(RestError::Protocol("unknown acquire reply".into()))),
        }
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
        let reply = self.eval(
            scripts::RENEW,
            &[self.claim_key(&parsed.target), self.channel(&parsed.target)],
            &[
                parsed.claim_id,
                sha256_hex(renewal_token),
                lease.to_string(),
                scripts::GRACE_MS.to_string(),
            ],
        )?;
        match reply.first().map(String::as_str) {
            Some("ok") => self.claim_reply(&parsed.target, &reply),
            Some(status) => Err(Self::status_error(status)),
            None => Err(self.error(RestError::Protocol("empty renew reply".into()))),
        }
    }

    fn release(&self, renewal_token: &str) -> Result<Claim, ClaimError> {
        let parsed = parse_renewal_token(renewal_token)?;
        let reply = self.eval(
            scripts::RELEASE,
            &[self.claim_key(&parsed.target), self.channel(&parsed.target)],
            &[parsed.claim_id, sha256_hex(renewal_token)],
        )?;
        match reply.first().map(String::as_str) {
            Some("ok") => self.claim_reply(&parsed.target, &reply),
            Some(status) => Err(Self::status_error(status)),
            None => Err(self.error(RestError::Protocol("empty release reply".into()))),
        }
    }

    fn get(&self, target: &ResourceRef) -> Result<Option<Claim>, ClaimError> {
        Ok(match self.read_state(target)? {
            State::Free => None,
            State::Held(claim, _) => Some(claim),
        })
    }

    fn list(&self, namespace: &Namespace) -> Result<Vec<Claim>, ClaimError> {
        let mut resources = self.members(self.resources_key(namespace))?;
        resources.sort();
        if resources.is_empty() {
            return Ok(Vec::new());
        }
        let targets: Vec<ResourceRef> = resources
            .iter()
            .filter_map(|name| ResourceRef::new(namespace.clone(), name).ok())
            .collect();
        let commands: Vec<Vec<Value>> = targets
            .iter()
            .map(|t| {
                vec![
                    json!("EVAL"),
                    json!(scripts::GET),
                    json!(1),
                    json!(self.claim_key(t)),
                ]
            })
            .collect();
        let replies = self.retrying(|| self.transport.pipeline(&commands))?;
        let mut claims = Vec::new();
        for (target, reply) in targets.iter().zip(replies) {
            let fields: Vec<String> = reply
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| i.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if fields.first().map(String::as_str) == Some("held") {
                claims.push(self.claim_reply(target, &fields)?);
            }
        }
        Ok(claims)
    }

    fn namespaces(&self) -> Result<Vec<Namespace>, ClaimError> {
        let mut names: Vec<Namespace> = self
            .members(self.namespaces_key())?
            .iter()
            .filter_map(|n| Namespace::parse(n).ok())
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
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
        let reply = self.eval(
            scripts::VERIFY,
            &[self.claim_key(target), self.fence_key(target)],
            &[fencing_token.to_string()],
        )?;
        match reply.first().map(String::as_str) {
            Some("ok") => Ok(()),
            Some("expired") => Err(ClaimError::Expired),
            Some("not_holder") => Err(ClaimError::NotHolder),
            _ => Err(ClaimError::Invalid("unknown fencing token".into())),
        }
    }
}

impl UpstashBackend {
    /// [`ClaimBackend::wait_free`], ended early by `cancel` (HS2-P234BH). Without a cancel
    /// the stream is read on this thread. With one, each stream is watched on a helper
    /// thread and this thread blocks on a channel the cancel's waker also feeds; the flag
    /// is read after the waker is registered and before every request, so no cancel is
    /// lost and none is followed by a request from this wait.
    fn wait_until(
        &self,
        target: &ResourceRef,
        timeout: Duration,
        cancel: Option<&WaitCancel>,
    ) -> Result<WaitOutcome, ClaimError> {
        let cancelled = || cancel.is_some_and(WaitCancel::is_cancelled);
        if cancelled() {
            return Err(ClaimError::Interrupted);
        }
        wait_ms(timeout)?;
        let deadline = Instant::now() + timeout;
        let channel = self.channel(target);
        let (signals, heard) = mpsc::channel::<Signal>();
        let _waker = cancel.map(|cancel| {
            let signals = signals.clone();
            cancel.register(move || {
                let _ = signals.send(Signal::Cancelled);
            })
        });
        let mut failures = 0u32;
        let mut generation = 0u64;
        loop {
            if cancelled() {
                return Err(ClaimError::Interrupted);
            }
            let (holder, left) = match self.read_state(target)? {
                State::Free => return Ok(WaitOutcome::Free),
                State::Held(holder, left) => (holder, left),
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(WaitOutcome::TimedOut {
                    holder: Box::new(holder),
                });
            }
            // The stream lives until the holder's lease runs out (or the wait does), so
            // expiry is one timeout rather than a series of looks.
            let lifetime = remaining.min(left + EXPIRY_SLACK);
            let outcome = match cancel {
                None => self.watch_stream(&channel, target, lifetime, None),
                Some(cancel) => {
                    generation += 1;
                    self.spawn_watch(&channel, target, lifetime, cancel, generation, &signals);
                    Self::outcome_of(&heard, generation)?
                }
            };
            match outcome {
                Ok(Some(WaitOutcome::Free)) => return Ok(WaitOutcome::Free),
                Ok(_) => failures = 0,
                Err(RestError::Unreachable(_) | RestError::RateLimited { .. }) => {
                    failures += 1;
                    if failures > MAX_STREAM_FAILURES {
                        return Err(self.error(RestError::Unreachable(
                            "the subscription kept failing".into(),
                        )));
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    // A bounded backoff after a failed stream, then resubscribe and re-read.
                    let pause = Self::backoff(failures - 1, remaining);
                    match cancel {
                        None => std::thread::sleep(pause),
                        Some(_) => Self::pause(&heard, pause)?,
                    }
                }
                Err(error) => return Err(self.error(error)),
            }
        }
    }

    /// Subscribe and watch one stream that lasts `lifetime`.
    fn watch_stream(
        &self,
        channel: &str,
        target: &ResourceRef,
        lifetime: Duration,
        cancel: Option<&WaitCancel>,
    ) -> Watched {
        let ends = Instant::now() + lifetime;
        let mut stream = self.transport.subscribe(channel, lifetime)?;
        self.watch(&mut *stream, target, ends, cancel)
    }

    /// Watch one stream on a helper thread that reports to `signals` once and ends.
    fn spawn_watch(
        &self,
        channel: &str,
        target: &ResourceRef,
        lifetime: Duration,
        cancel: &WaitCancel,
        generation: u64,
        signals: &Sender<Signal>,
    ) {
        let (backend, channel, target, cancel, signals) = (
            self.clone(),
            channel.to_owned(),
            target.clone(),
            cancel.clone(),
            signals.clone(),
        );
        let failed = signals.clone();
        let spawned = std::thread::Builder::new()
            .name("upstash-claim-wait".into())
            .spawn(move || {
                let watched = backend.watch_stream(&channel, &target, lifetime, Some(&cancel));
                // A cancelled wait has stopped listening; its answer is dropped unread.
                let _ = signals.send(Signal::Stream(generation, watched));
            });
        if let Err(error) = spawned {
            let _ = failed.send(Signal::Stream(
                generation,
                Err(RestError::Unreachable(format!(
                    "could not start the subscription reader: {error}"
                ))),
            ));
        }
    }

    /// The watch of stream `generation`, or `interrupted` once the cancel fires.
    fn outcome_of(heard: &Receiver<Signal>, generation: u64) -> Result<Watched, ClaimError> {
        loop {
            match heard.recv() {
                Ok(Signal::Cancelled) => return Err(ClaimError::Interrupted),
                Ok(Signal::Stream(id, watched)) if id == generation => return Ok(watched),
                // An earlier stream's late answer: that stream was already given up.
                Ok(Signal::Stream(..)) => {}
                // The wait holds a sender, so the channel cannot close under it.
                Err(_) => return Err(ClaimError::Interrupted),
            }
        }
    }

    /// Wait out `pause`, or end with `interrupted` once the cancel fires.
    fn pause(heard: &Receiver<Signal>, pause: Duration) -> Result<(), ClaimError> {
        let until = Instant::now() + pause;
        loop {
            let rest = until.saturating_duration_since(Instant::now());
            match heard.recv_timeout(rest) {
                Ok(Signal::Cancelled) => return Err(ClaimError::Interrupted),
                Ok(Signal::Stream(..)) => {}
                Err(RecvTimeoutError::Timeout) => return Ok(()),
                Err(RecvTimeoutError::Disconnected) => return Err(ClaimError::Interrupted),
            }
        }
    }

    /// Read one open stream. Once the service confirms the subscription the state is read
    /// again, closing the gap between the first read and the subscribe: a release that
    /// landed in it is found here instead of waited for.
    ///
    /// HS2-QX9QQE: a read timeout can end before the stream's lifetime (`ends`). On Windows
    /// ARM64 the socket timer returns early and the timeout is truncated to whole
    /// milliseconds, so the wait loop saw a few milliseconds left and read and subscribed
    /// again. The rest of the lifetime is slept here instead, so the stream's end costs one
    /// read, as on every other platform. A release in those milliseconds is found by the
    /// read that follows.
    ///
    /// HS2-P234BH: a watch whose wait was cancelled reads no state; it only lets its
    /// stream end.
    fn watch(
        &self,
        stream: &mut dyn transport::Subscription,
        target: &ResourceRef,
        ends: Instant,
        cancel: Option<&WaitCancel>,
    ) -> Watched {
        let cancelled = || cancel.is_some_and(WaitCancel::is_cancelled);
        loop {
            match stream.next()? {
                Event::Subscribed if cancelled() => return Ok(None),
                Event::Subscribed => {
                    if let State::Free = self
                        .read_state(target)
                        .map_err(|e| RestError::Protocol(e.to_string()))?
                    {
                        return Ok(Some(WaitOutcome::Free));
                    }
                }
                // A release or a shortened lease: read again at the top of the wait loop.
                Event::Message(_) => return Ok(None),
                Event::Timeout => {
                    let rest = ends.saturating_duration_since(Instant::now());
                    if !rest.is_zero() && !cancelled() {
                        std::thread::sleep(rest);
                    }
                    return Ok(None);
                }
                Event::Closed => {
                    return Err(RestError::Unreachable("the subscription closed".into()));
                }
            }
        }
    }
}

/// The settings a configuration of kind `upstash_redis` needs, checked before anything is
/// written: a safe REST URL and a valid key prefix.
pub fn check_settings(
    configuration: &BackendConfiguration,
) -> Result<(String, String), ClaimError> {
    let url = configuration.setting("url").ok_or_else(|| {
        ClaimError::Invalid(format!(
            "configuration '{}' needs --setting url=<the database's REST URL>",
            configuration.id
        ))
    })?;
    validate_url(url).map_err(ClaimError::Invalid)?;
    let prefix = configuration.setting("prefix").unwrap_or(DEFAULT_PREFIX);
    if !valid_prefix(prefix) {
        return Err(ClaimError::Invalid(
            "the key prefix is 1-32 characters of a-z, 0-9, '_' or '-'".into(),
        ));
    }
    Ok((url.to_owned(), prefix.to_owned()))
}

/// The backend a configuration of kind `upstash_redis` selects, reading the REST token from
/// the OS credential store.
pub fn backend_for(
    home: &Path,
    configuration: &BackendConfiguration,
) -> Result<Arc<dyn ClaimBackend>, ClaimError> {
    backend_with(
        configuration,
        SharedSecrets(Arc::new(hotsheet_ticketing::OsKeychain::at(home))),
        |url, token| Arc::new(UreqTransport::new(url, token)),
    )
}

/// [`backend_for`] over an explicit credential store and transport factory.
pub fn backend_with(
    configuration: &BackendConfiguration,
    secrets: SharedSecrets,
    transport: impl FnOnce(&str, &str) -> Arc<dyn RestTransport>,
) -> Result<Arc<dyn ClaimBackend>, ClaimError> {
    let (url, prefix) = check_settings(configuration)?;
    let token = secrets
        .get(&secret_account(&configuration.id, TOKEN_SETTING))
        .map_err(|e| ClaimError::BackendUnreachable(format!("OS credential store: {e}")))?
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            ClaimError::Unsupported(format!(
                "no REST token for configuration '{0}'; run `hotsheet-cli resource backend secret {0} {TOKEN_SETTING}` and give the token on standard input",
                configuration.id
            ))
        })?;
    let keys: Option<Arc<dyn KeyProvider>> = match configuration.credential {
        Some(_) => Some(Arc::new(CredentialKeyProvider::new(
            secrets,
            configuration,
        )?)),
        None => None,
    };
    Ok(Arc::new(UpstashBackend::new(
        &configuration.id,
        &prefix,
        transport(&url, &token),
        keys,
    )))
}
