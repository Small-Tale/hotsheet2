//! HS2-WDQZ37: a target's CI gate watched on the machine that hosts the target.
//!
//! The target's server stays authoritative for its CI evidence (§26.1). When a run's
//! target lives on another machine, linking a `github-actions` gate asks that machine
//! to start the watch Job there (`POST target-gates`), with its own GitHub
//! credentials and its own Job monitor, and the orchestrating machine follows it over
//! the reviewed target transport with a cursor (`GET target-gates`): a genuine long
//! poll on the host's Job store revision, so a reconnect with an old cursor is
//! answered at once and nothing is missed. The orchestrating machine needs no provider
//! credentials and keeps only the typed reading it is answered.
//!
//! A peer without the route (an older server) leaves the gate to this machine's own
//! adapter, as before. A peer that cannot be reached leaves the gate as it stands; the
//! follower backs off instead of asking again at once.
use std::time::{Duration, Instant};

use hotsheet_jobs::GITHUB_ACTIONS_KIND;
use hotsheet_ticketing::orchestration::{
    CiScope, Gate, GateError, GateEvidence, GateKind, GateReading, GateState, TargetDestination,
    TargetOpError, valid_commit,
};
use serde::{Deserialize, Serialize};

use super::gates::GateProvider;
use super::tickets::encode;
use super::*;

/// The longest one hosted gate read parks, under the target transport's read
/// timeout. A follower with a later deadline asks again.
pub const MAX_HOSTED_GATE_WAIT_MS: u64 = 8_000;

/// Which CI gate of a hosted checkout: the commit and where its runs are found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedGateWatch {
    pub commit: String,
    /// `owner/name`.
    pub repository: String,
    pub workflow: String,
    pub branch: String,
    /// Watch again after a verdict that failed or a watch that was stopped.
    #[serde(default)]
    pub again: bool,
}

/// What the hosting machine's adapter reads for one gate, with the cursor to wait
/// from next. Identifiers, codes and counts only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedGate {
    pub state: GateState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<GateEvidence>,
    /// The hosting machine's own watch Job id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch_ref: Option<String>,
    /// The host's record revision this reading was taken at. Pass it as `after`.
    pub cursor: u64,
    /// Whether something observes the provider on the hosting machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watching: Option<bool>,
}

/// One hosted refresh: the gate as it now stands, whether it changed, and the host's
/// cursor when the host was asked.
pub(super) struct HostedRefresh {
    pub gate: Gate,
    pub changed: bool,
    pub cursor: Option<u64>,
    pub watching: Option<bool>,
}

/// A hosted read that came back before half the wait it was asked for, unchanged, did
/// not wait: asking again at once would spin.
pub(super) fn hosted_did_not_wait(asked: Instant, wait_ms: u64) -> bool {
    wait_ms > 0 && asked.elapsed() < Duration::from_millis(wait_ms / 2)
}

/// Whether a refusal means the peer has no such route (an older server).
pub(super) fn peer_unsupported(error: &OrchestrationError) -> bool {
    matches!(
        error,
        OrchestrationError::TargetOperation(TargetOpError::PeerUnsupported)
    )
}

/// The gate a hosting machine reads for a request: its own adapter needs only the
/// commit, the CI scope and the checkout whose Job store holds the watch.
fn hosted_record(checkout_id: &str, watch: &HostedGateWatch) -> Result<Gate, OrchestrationError> {
    let commit = watch.commit.to_ascii_lowercase();
    valid_commit(&commit)?;
    Ok(Gate {
        gate_id: "hosted".into(),
        owner_checkout_id: checkout_id.into(),
        run_id: "hosted".into(),
        target_id: "hosted".into(),
        kind: GateKind::Ci,
        provider: GITHUB_ACTIONS_KIND.into(),
        commit,
        ci: Some(CiScope {
            repository: watch.repository.clone(),
            workflow: watch.workflow.clone(),
            branch: watch.branch.clone(),
        }),
        deployment: None,
        state: GateState::Pending,
        reason: None,
        evidence: None,
        watch_ref: None,
        superseded_by: None,
        linked_by: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        history: Vec::new(),
    })
}

fn answer(
    provider: &dyn GateProvider,
    gate: &Gate,
    cursor: u64,
) -> Result<HostedGate, OrchestrationError> {
    let reading = provider.read(gate).map_err(OrchestrationError::Gate)?;
    // A gate that still waits needs an observer here; a settled one starts nothing.
    let waits = reading.state == GateState::Pending
        || (reading.state == GateState::Unknown && reading.reason.as_deref() == Some("watch_lost"));
    let watching = if waits {
        provider.ensure_watching()
    } else {
        provider.watching()
    };
    Ok(HostedGate {
        state: reading.state,
        reason: reading.reason,
        evidence: reading.evidence,
        watch_ref: reading.watch_ref,
        cursor,
        watching,
    })
}

impl OrchestrationRuntime {
    fn hosted_ci_provider(&self) -> Result<Arc<dyn GateProvider>, OrchestrationError> {
        self.gate_provider(GITHUB_ACTIONS_KIND, GateKind::Ci)
            .cloned()
            .ok_or(OrchestrationError::Gate(GateError::ProviderUnsupported))
    }

    // ---- Hosted half ----

    /// Start, or find, the CI watch of a commit in a checkout this machine hosts, with
    /// this machine's provider credentials. Idempotent like a local link: the same
    /// watch again starts nothing. Observing only, so it is ordinary authenticated
    /// data like the other hosted reads; the orchestrating machine passed its own gate.
    pub fn hosted_gate_watch(
        &self,
        checkout_id: &str,
        watch: HostedGateWatch,
    ) -> Result<HostedGate, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        let provider = self.hosted_ci_provider()?;
        let gate = hosted_record(&checkout.id, &watch)?;
        // Before the start: a change in between is then seen by the next wait.
        let cursor = provider.cursor(&checkout.id);
        provider
            .host(&gate, watch.again)
            .map_err(OrchestrationError::Gate)?;
        answer(provider.as_ref(), &gate, cursor)
    }

    /// Read the CI gate of a commit this machine watches. With `after` equal to the
    /// current cursor and a wait, parks until this machine's watch records change or
    /// the wait passes: a long poll woken by the change, never a timer loop. Starts no
    /// watch; it only makes sure a waiting one is observed.
    pub fn hosted_gate(
        &self,
        checkout_id: &str,
        watch: &HostedGateWatch,
        after: Option<u64>,
        wait_ms: u64,
    ) -> Result<HostedGate, OrchestrationError> {
        if wait_ms > MAX_HOSTED_GATE_WAIT_MS {
            return Err(OrchestrationError::Invalid("wait_ms must be at most 8000"));
        }
        let checkout = self.owner(checkout_id)?;
        let provider = self.hosted_ci_provider()?;
        let gate = hosted_record(&checkout.id, watch)?;
        let mut cursor = provider.cursor(&checkout.id);
        if after == Some(cursor) && wait_ms > 0 {
            provider.wait(&checkout.id, cursor, Duration::from_millis(wait_ms));
            cursor = provider.cursor(&checkout.id);
        }
        answer(provider.as_ref(), &gate, cursor)
    }

    // ---- Orchestrating half ----

    /// The machine to ask about a gate: `Some` only for a `github-actions` CI gate of
    /// a target bound to another machine. A same-machine target keeps the local
    /// adapter, which reads the same Job store.
    pub(super) fn hosted_gate_destination<'a>(
        authorized: &'a Authorized,
        gate: &Gate,
    ) -> Option<&'a TargetDestination> {
        if gate.kind != GateKind::Ci || gate.provider != GITHUB_ACTIONS_KIND || gate.ci.is_none() {
            return None;
        }
        Self::destination(authorized)
            .ok()
            .filter(|destination| matches!(destination, TargetDestination::Remote { .. }))
    }

    /// Ask the hosting machine about one gate: start its watch (`start` = `Some(again)`)
    /// or read it, waiting from `after` for at most `wait_ms`.
    pub(super) fn hosted_gate_call(
        &self,
        destination: &TargetDestination,
        gate: &Gate,
        start: Option<bool>,
        after: Option<u64>,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedGate, OrchestrationError> {
        let scope = gate
            .ci
            .as_ref()
            .ok_or(OrchestrationError::Gate(GateError::ProviderUnsupported))?;
        let reply = match start {
            Some(again) => {
                let body = serde_json::to_value(HostedGateWatch {
                    commit: gate.commit.clone(),
                    repository: scope.repository.clone(),
                    workflow: scope.workflow.clone(),
                    branch: scope.branch.clone(),
                    again,
                })
                .map_err(|_| OrchestrationError::Corrupt)?;
                self.hosted_call_at(destination, "/target-gates", "POST", "", Some(&body), tx)?
            }
            None => {
                let mut query = format!(
                    "?commit={}&repository={}&workflow={}&branch={}&wait_ms={}",
                    encode(&gate.commit),
                    encode(&scope.repository),
                    encode(&scope.workflow),
                    encode(&scope.branch),
                    wait_ms.min(MAX_HOSTED_GATE_WAIT_MS)
                );
                if let Some(after) = after {
                    query.push_str(&format!("&after={after}"));
                }
                self.hosted_call_at(destination, "/target-gates", "GET", &query, None, tx)?
            }
        };
        serde_json::from_value(reply)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
    }

    /// Record what the hosting machine read for a gate, as a local adapter reading is
    /// recorded. A verdict that repeats the recorded evidence keeps its first time.
    pub(super) fn gate_record_hosted(
        &self,
        gate: Gate,
        hosted: &HostedGate,
        tx: &mut Transaction<'_>,
    ) -> Result<(Gate, bool), OrchestrationError> {
        let mut reading = GateReading::new(hosted.state);
        reading.reason.clone_from(&hosted.reason);
        reading.evidence.clone_from(&hosted.evidence);
        reading.watch_ref.clone_from(&hosted.watch_ref);
        if let (Some(new), Some(old)) = (&mut reading.evidence, &gate.evidence) {
            if (new.order, new.attempt, &new.commit) == (old.order, old.attempt, &old.commit) {
                new.at_ms = old.at_ms;
            }
        }
        self.gate_record(gate, reading, tx)
    }

    /// Bring one gate in line with what the hosting machine reads. A settled gate is
    /// returned as it is. A host that no longer watches the gate (it was linked before
    /// the host watched, or the host lost its records) is asked to watch it; that only
    /// observes. A refusal is returned for the caller to decide: a peer without the
    /// route falls back to the local adapter, anything else leaves the gate as it is.
    pub(super) fn gate_refresh_hosted(
        &self,
        gate: Gate,
        destination: &TargetDestination,
        after: Option<u64>,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedRefresh, OrchestrationError> {
        if matches!(
            gate.state,
            GateState::Superseded | GateState::AwaitingApproval | GateState::Passed
        ) {
            return Ok(HostedRefresh {
                gate,
                changed: false,
                cursor: None,
                watching: None,
            });
        }
        let mut hosted = self.hosted_gate_call(destination, &gate, None, after, wait_ms, tx)?;
        if hosted.state == GateState::Unknown && hosted.reason.as_deref() == Some("not_watched") {
            hosted = self.hosted_gate_call(destination, &gate, Some(false), None, 0, tx)?;
        }
        let (gate, changed) = self.gate_record_hosted(gate, &hosted, tx)?;
        Ok(HostedRefresh {
            gate,
            changed,
            cursor: Some(hosted.cursor),
            watching: hosted.watching,
        })
    }
}
