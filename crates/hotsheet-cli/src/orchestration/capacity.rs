//! HS2-J8PRTV: a target machine's capacity and its admission answer.
//!
//! The hosted half runs where the target lives: its own measurements, its own
//! admission file, its own decision. The orchestrating half passes the gate and then
//! reaches that half in process or over the reviewed transport. Nothing here samples
//! on a timer: a snapshot exists only because a request asked for it. HS2-P5GTSW: a
//! request that names the `version` it has (`after`) and a `wait_ms` is a long poll:
//! the host samples again while that one subscriber waits and answers as soon as the
//! version changes, so sampling runs only while someone is subscribed.
use hotsheet_resource_claims::Namespace;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AccessRequest, AdmissionDecision, AdmissionRequest, AdmissionStore, CapacitySnapshot,
    DenialReason, HeldClaim, HostObservation, MAX_CLAIM_NAMESPACES, OfflinePolicy, SessionRequest,
    TargetAction, TargetSpeculation, decide_admission,
};
use serde::Deserialize;

use super::tickets::encode;
use super::*;

/// Where the hosted half gets the host's own measurements. The server that hosts
/// sessions injects its sampler; tests script one.
pub trait CapacityProbe: Send + Sync {
    /// What the host measured for one checkout it hosts. Unmeasured values are `None`.
    fn observe(&self, checkout: &Checkout) -> HostObservation;
}

/// No session host is reachable from this process: every value is unknown.
pub struct UnknownCapacity;
impl CapacityProbe for UnknownCapacity {
    fn observe(&self, _: &Checkout) -> HostObservation {
        HostObservation::default()
    }
}

/// A headless process hosts no sessions itself, so it asks the Hot Sheet server
/// running on this machine for that server's own snapshot. Without one, capacity is
/// unknown and admission fails closed.
pub struct LocalServerCapacity {
    home: PathBuf,
}
impl LocalServerCapacity {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }
}
impl CapacityProbe for LocalServerCapacity {
    fn observe(&self, checkout: &Checkout) -> HostObservation {
        crate::external_launch::running_servers(&self.home)
            .iter()
            .find_map(|server| {
                let url = format!(
                    "{}/checkouts/{}/target-capacity",
                    server.url.trim_end_matches('/'),
                    checkout.id
                );
                let response = ureq::get(&url)
                    .set("X-Hotsheet-Secret", &server.secret)
                    .timeout(std::time::Duration::from_secs(10))
                    .call()
                    .ok()?;
                let snapshot: CapacitySnapshot =
                    serde_json::from_reader(response.into_reader()).ok()?;
                Some(snapshot.into_host())
            })
            .unwrap_or_default()
    }
}

/// The orchestrating-side admission question about one target.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetAdmission {
    pub tool: String,
    #[serde(default)]
    pub model: Option<String>,
    /// How many sessions the orchestrator would like to start; one when omitted.
    #[serde(default)]
    pub sessions: Option<u32>,
}

/// The body form of the orchestrating capacity read.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadCapacity {
    /// Resource-claim namespaces whose live claims the snapshot should list.
    #[serde(default)]
    pub claim_namespaces: Vec<String>,
    /// HS2-P5GTSW: the snapshot `version` already seen; with `wait_ms`, wait for the
    /// next change instead of asking again.
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// HS2-P5GTSW: the longest one capacity wait parks, under the target transport's read
/// timeout. A subscriber with a later deadline asks again with the version it got.
pub const MAX_CAPACITY_WAIT_MS: u64 = 8_000;
/// HS2-P5GTSW: how often a waiting request samples again. The server's capacity reading
/// is shared for three seconds, so waiting subscribers share one reading.
pub const CAPACITY_RESAMPLE_MS: u64 = 3_000;

/// HS2-P5GTSW: a capacity version a subscriber passes back: bounded, printable.
pub(super) fn capacity_after(after: Option<&str>) -> Result<Option<&str>, OrchestrationError> {
    match after {
        Some(version)
            if version.is_empty()
                || version.len() > 64
                || !version.bytes().all(|byte| byte.is_ascii_alphanumeric()) =>
        {
            Err(OrchestrationError::Invalid(
                "bounded capacity version required",
            ))
        }
        other => Ok(other),
    }
}

/// HS2-YDZAFH: the most namespaces a capacity read that names none enumerates; beyond
/// it the claims read as unknown rather than as a partial list.
pub const MAX_ENUMERATED_NAMESPACES: usize = 64;

/// Parse `a,b` into distinct claim namespaces. An invalid name is the caller's error.
pub(super) fn claim_namespaces(text: Option<&str>) -> Result<Vec<String>, OrchestrationError> {
    let mut names: Vec<String> = Vec::new();
    for name in text
        .unwrap_or_default()
        .split(',')
        .filter(|name| !name.is_empty())
    {
        Namespace::parse(name)
            .map_err(|_| OrchestrationError::Invalid("invalid resource claim namespace"))?;
        if !names.iter().any(|known| known == name) {
            names.push(name.to_owned());
        }
    }
    if names.len() > MAX_CLAIM_NAMESPACES {
        return Err(OrchestrationError::Invalid(
            "at most 8 claim namespaces per capacity request",
        ));
    }
    Ok(names)
}

impl OrchestrationRuntime {
    /// Replace where host measurements come from (the session-hosting server, tests).
    #[must_use]
    pub fn with_capacity_probe(mut self, probe: Arc<dyn CapacityProbe>) -> Self {
        self.capacity = probe;
        self
    }

    /// HS2-P5GTSW: sample a waiting capacity request this often instead (tests).
    #[must_use]
    pub fn with_capacity_resample(mut self, every: std::time::Duration) -> Self {
        self.capacity_resample = every;
        self
    }

    // ---- Hosted half ----

    /// Live claims in the named namespaces, without paths, holders or metadata. An
    /// unreadable namespace makes the whole list unknown rather than partial, and
    /// the answer then carries the strictest offline policy among the unreadable
    /// ones (HS2-2FG91X): work may start on an unverified resource only when every
    /// backend that could not be read allows it.
    fn held_claims(
        &self,
        namespaces: &[String],
    ) -> (Option<Vec<HeldClaim>>, Option<OfflinePolicy>) {
        if namespaces.is_empty() {
            return (None, None);
        }
        self.held_in(namespaces)
    }

    /// HS2-YDZAFH: the namespaces a capacity read covers: the named ones, or, when none
    /// is named, every namespace with claim state on this machine. `None` when they
    /// cannot be enumerated or there are more than [`MAX_ENUMERATED_NAMESPACES`]: the
    /// claims are then unknown, never a partial list.
    fn claim_scope(&self, named: &[String]) -> Result<Vec<String>, OfflinePolicy> {
        if !named.is_empty() {
            return Ok(named.to_vec());
        }
        match self.claims.namespaces() {
            Ok(all) if all.len() <= MAX_ENUMERATED_NAMESPACES => Ok(all),
            Ok(_) => Err(OfflinePolicy::FailClosed),
            Err(unreachable) => Err(unreachable.offline),
        }
    }

    fn held_in(&self, namespaces: &[String]) -> (Option<Vec<HeldClaim>>, Option<OfflinePolicy>) {
        let mut held = Vec::new();
        let mut offline: Option<OfflinePolicy> = None;
        for name in namespaces {
            match self.claims.held(name) {
                Ok(claims) => held.extend(claims),
                Err(unreachable) => {
                    offline = Some(
                        offline.map_or(unreachable.offline, |known| known.min(unreachable.offline)),
                    );
                }
            }
        }
        match offline {
            Some(policy) => (None, Some(policy)),
            None => (Some(held), None),
        }
    }

    /// This machine's capacity for one checkout it hosts.
    pub fn hosted_capacity(
        &self,
        checkout_id: &str,
        namespaces: &[String],
    ) -> Result<CapacitySnapshot, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        // HS2-YDZAFH: with no namespace named, every held claim of this machine.
        let (scope, claims, offline) = match self.claim_scope(namespaces) {
            Ok(scope) if scope.is_empty() => (scope, Some(Vec::new()), None),
            Ok(scope) => {
                let (claims, offline) = self.held_claims(&scope);
                (scope, claims, offline)
            }
            // Unknown, exactly as a read that named nothing reported before.
            Err(_) => (namespaces.to_vec(), None, None),
        };
        Ok(CapacitySnapshot::new(
            self.server_id.clone(),
            checkout.id.clone(),
            self.capacity.observe(&checkout),
            scope,
            claims,
        )
        .with_claims_offline(offline)
        // HS2-JSF985: the hosted project's own committed opt-in. It is reported only
        // when the project opts in, so the answer of every project that does not is
        // what it always was; an orchestrator reads its absence as not allowed.
        .with_speculation(checkout_speculation(&checkout.root))
        // HS2-TK9ADC: the project's pool, for the orchestrator's role sessions.
        .with_pool(super::sessions::hosted_pool(
            Path::new(&checkout.root),
            hotsheet_ticketing::Clock::now_ms(&super::request::Clock),
        ))
        // HS2-AWB7BX: the owner's own admission limits are part of the room.
        .with_admission_version(
            &AdmissionStore::new(&self.home)
                .read()
                .map(|view| view.admission)
                .unwrap_or_default(),
        ))
    }

    /// HS2-P5GTSW hosted half: this machine's capacity once its version differs from
    /// `after`, or when `wait_ms` (at most [`MAX_CAPACITY_WAIT_MS`]) passes. Without
    /// `after`, or with a version that is not current, it answers at once. While it
    /// waits it samples again every [`CAPACITY_RESAMPLE_MS`]; nothing samples once
    /// the request ends, so a host nobody subscribes to reads nothing.
    pub fn hosted_capacity_wait(
        &self,
        checkout_id: &str,
        namespaces: &[String],
        after: Option<&str>,
        wait_ms: u64,
    ) -> Result<CapacitySnapshot, OrchestrationError> {
        let after = capacity_after(after)?;
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(wait_ms.min(MAX_CAPACITY_WAIT_MS));
        loop {
            let snapshot = self.hosted_capacity(checkout_id, namespaces)?;
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if after.is_none() || snapshot.version.as_deref() != after || remaining.is_zero() {
                return Ok(snapshot);
            }
            std::thread::sleep(remaining.min(self.capacity_resample));
        }
    }

    /// This machine's own answer: its snapshot, its admission file, the limits the
    /// caller sent. A damaged admission file is an error, never a looser default.
    pub fn hosted_admission(
        &self,
        checkout_id: &str,
        request: AdmissionRequest,
    ) -> Result<AdmissionDecision, OrchestrationError> {
        request.validate()?;
        let config = AdmissionStore::new(&self.home).read()?.admission;
        let snapshot = self.hosted_capacity(checkout_id, &[])?;
        Ok(decide_admission(&snapshot, &config, &request))
    }

    // ---- Orchestrating half ----

    /// Read a target's capacity from the machine that hosts it (`gates_observe`).
    pub fn target_capacity(
        &self,
        target: &TargetRef,
        namespaces: &[String],
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<CapacitySnapshot, OrchestrationError> {
        self.target_capacity_wait(target, namespaces, None, 0, actor, tx)
    }

    /// HS2-P5GTSW: the same read as a long poll: answered once the host's capacity
    /// `version` differs from `after`, or when `wait_ms` passes.
    pub fn target_capacity_wait(
        &self,
        target: &TargetRef,
        namespaces: &[String],
        after: Option<&str>,
        wait_ms: u64,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<CapacitySnapshot, OrchestrationError> {
        let after = capacity_after(after)?;
        let authorized = self.authorize(
            target,
            actor,
            ActionRequest::new(TargetAction::GatesObserve),
            tx,
        )?;
        let outcome = (|| {
            let query = capacity_query(namespaces, after, wait_ms);
            serde_json::from_value(self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-capacity",
                "GET",
                &query,
                None,
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        })();
        self.finish(target, &authorized, &outcome, tx)?;
        outcome
    }

    /// Ask the target's machine whether sessions of a tool may start there now. The
    /// question starts nothing: it passes the gate as `sessions_manage` for that tool
    /// and model, and the policy's machine limits for the target travel with it. The
    /// start itself still passes the gate with its load and any permission request.
    pub fn target_admission(
        &self,
        target: &TargetRef,
        request: TargetAdmission,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<AdmissionDecision, OrchestrationError> {
        let mut hosted = AdmissionRequest {
            tool: request.tool,
            model: request.model,
            sessions: request.sessions.unwrap_or(1),
            limits: None,
        };
        hosted.validate()?;
        let question = SessionRequest {
            tool: hosted.tool.clone(),
            model: hosted.model.clone(),
            running: 0,
            starting: false,
        };
        let authorized = self.authorize(
            target,
            actor,
            ActionRequest {
                session: Some(question.clone()),
                ..ActionRequest::new(TargetAction::SessionsManage).continuing()
            },
            tx,
        )?;
        let outcome = (|| {
            // HS2-XHQ1EH: the question starts nothing, so the gate neither asks nor
            // limits it; the policy must still list the tool and model on the target.
            let mut asked =
                AccessRequest::new(authorized.actor_id.clone(), TargetAction::SessionsManage);
            asked.session = Some(question);
            if let super::policy::PolicyAnswer::Denied(_) =
                self.policy_answer(&target.owner_checkout_id, &asked, &target.target_id)
            {
                return Err(OrchestrationError::Denied(DenialReason::PolicyDenied));
            }
            // The gate just evaluated this policy; one that vanished since is a conflict.
            let policy = self
                .policy(&target.owner_checkout_id)?
                .policy
                .ok_or(OrchestrationError::Conflict)?;
            hosted.limits = Some(policy.machine_limits(&target.target_id));
            let body = serde_json::to_value(&hosted).map_err(|_| OrchestrationError::Corrupt)?;
            serde_json::from_value(self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-admission",
                "POST",
                "",
                Some(&body),
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        })();
        self.finish(target, &authorized, &outcome, tx)?;
        outcome
    }
}

/// The hosted capacity route's query: claim namespaces and, for a wait, the version
/// already seen and how long to wait.
pub(super) fn capacity_query(namespaces: &[String], after: Option<&str>, wait_ms: u64) -> String {
    let mut parts = Vec::new();
    if !namespaces.is_empty() {
        parts.push(format!(
            "claim_namespaces={}",
            encode(&namespaces.join(","))
        ));
    }
    if let Some(after) = after {
        parts.push(format!(
            "after={}&wait_ms={}",
            encode(after),
            wait_ms.min(MAX_CAPACITY_WAIT_MS)
        ));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

/// HS2-JSF985: a hosted project's own committed opt-in to provisional readiness, as
/// its machine reports it: only when the project opts in, so every project that does
/// not answers what it always did. HS2-FJ6MDW: also the cheap `target-speculation`
/// read and the change feed's report.
pub(super) fn checkout_speculation(root: &str) -> Option<TargetSpeculation> {
    hotsheet_ticketing::provisional::SpeculationPolicy::read(
        &hotsheet_ticketing::Settings::for_project(root),
    )
    .ok()
    .filter(|policy| policy.enabled)
    .map(|policy| TargetSpeculation {
        enabled: policy.enabled,
        max_depth: policy.max_depth,
        integration: policy.integration,
    })
}
