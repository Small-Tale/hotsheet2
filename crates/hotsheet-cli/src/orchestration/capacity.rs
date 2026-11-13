//! HS2-J8PRTV: a target machine's capacity and its admission answer.
//!
//! The hosted half runs where the target lives: its own measurements, its own
//! admission file, its own decision. The orchestrating half passes the gate and then
//! reaches that half in process or over the reviewed transport. Nothing here samples
//! on a timer: a snapshot exists only because a request asked for it.
use hotsheet_resource_claims::Namespace;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AdmissionDecision, AdmissionRequest, AdmissionStore, CapacitySnapshot, HeldClaim,
    HostObservation, MAX_CLAIM_NAMESPACES, OfflinePolicy, SessionRequest, TargetAction,
    decide_admission,
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
}

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
        let (claims, offline) = self.held_claims(namespaces);
        Ok(CapacitySnapshot::new(
            self.server_id.clone(),
            checkout.id.clone(),
            self.capacity.observe(&checkout),
            namespaces.to_vec(),
            claims,
        )
        .with_claims_offline(offline))
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
        let authorized = self.authorize(
            target,
            actor,
            ActionRequest::new(TargetAction::GatesObserve),
            tx,
        )?;
        let outcome = (|| {
            let query = if namespaces.is_empty() {
                String::new()
            } else {
                format!("?claim_namespaces={}", encode(&namespaces.join(",")))
            };
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
    /// start itself still passes the gate with its load and any approval.
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
        let authorized = self.authorize(
            target,
            actor,
            ActionRequest {
                session: Some(SessionRequest {
                    tool: hosted.tool.clone(),
                    model: hosted.model.clone(),
                    running: 0,
                    starting: false,
                }),
                ..ActionRequest::new(TargetAction::SessionsManage).continuing()
            },
            tx,
        )?;
        let outcome = (|| {
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
