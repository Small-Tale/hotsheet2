//! Local session mailbox contract (HS2-76VMYG). Contains identifiers, never credentials.

use std::collections::BTreeMap;

use hotsheet_ticketing::TicketRef;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Integrator,
    Worker,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub generation: String,
    pub expires_at: i64,
    pub return_route: ReturnRoute,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "recipient", rename_all = "snake_case")]
pub enum ReturnRoute {
    NativeParent(String),
    Terminal(String),
    HumanRelay,
}

impl std::str::FromStr for ReturnRoute {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.split_once(':') {
            Some(("native", id)) if !id.trim().is_empty() => Ok(Self::NativeParent(id.into())),
            Some(("terminal", id)) if !id.trim().is_empty() => Ok(Self::Terminal(id.into())),
            None if raw == "human" => Ok(Self::HumanRelay),
            _ => Err("route must be native:<agent-id>, terminal:<connection-id>, or human".into()),
        }
    }
}

impl Session {
    pub fn live(&self, now: i64) -> bool {
        self.expires_at > now
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Offered,
    Accepted,
    Handoff,
    Verifying,
    Integrating,
    Ci,
    Done,
    Blocked,
}

/// Durable handoff proof. A findings note is scoped by its assignment's qualified
/// ticket and is checked again before the integrator completes it (HS2-A1F615).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandoffEvidence {
    Code { commit: String },
    Findings { note_id: String },
}

impl Phase {
    pub fn holds_worker(self) -> bool {
        matches!(self, Self::Offered | Self::Accepted)
    }

    pub fn pending(self) -> bool {
        self != Self::Done
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    /// Caller-generated operation id: retrying delivery never creates another assignment.
    pub id: String,
    pub worker: String,
    pub generation: String,
    pub ticket: TicketRef,
    pub slug: String,
    pub base: String,
    pub phase: Phase,
    /// Kept for version-one records and older code-handoff readers. Version two
    /// stores the typed outcome in `handoff` as well.
    pub commit: Option<String>,
    #[serde(default)]
    pub handoff: Option<HandoffEvidence>,
    pub return_route: ReturnRoute,
    #[serde(default)]
    pub recovery_note: Option<String>,
}

impl Assignment {
    /// HS2-65ETRJ/HS2-AEBJYN: returned code and findings retain their original evidence.
    pub fn returned_for_review(&self) -> bool {
        self.phase == Phase::Blocked
            && (self.commit.is_some() || self.handoff.is_some())
            && self.recovery_note.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    pub project_id: String,
    pub source_id: String,
    pub revision: u64,
    pub integrator: Option<Session>,
    pub workers: BTreeMap<String, Session>,
    pub assignments: BTreeMap<String, Assignment>,
    /// Explicit terminal outcome, separate from an empty actionable queue.
    pub stopped: Option<String>,
}

impl State {
    pub fn new(project_id: String, source_id: String) -> Self {
        Self {
            version: 3,
            project_id,
            source_id,
            revision: 0,
            integrator: None,
            workers: BTreeMap::new(),
            assignments: BTreeMap::new(),
            stopped: None,
        }
    }

    pub fn active_assignment(&self, worker: &str) -> Option<&Assignment> {
        self.assignments
            .values()
            .find(|a| a.worker == worker && a.phase.holds_worker())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub ticket: TicketRef,
    pub slug: String,
    pub updated_at: String,
    pub phase: Option<hotsheet_model::StartedPhase>,
    pub blocked: bool,
    pub claimed_by: Option<String>,
    pub claim_expires_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub cursor: String,
    pub state: State,
    pub live_workers: Vec<String>,
    pub live_integrator: bool,
    /// None means that this source requires its provider's query/event path.
    pub local_queue: Option<Vec<QueueEntry>>,
    pub queue_support: String,
    pub uncovered_sources: Vec<String>,
    pub live_claims: Vec<LiveClaim>,
    /// Metadata-only implementation support, never remote availability or claim evidence.
    /// Default permits reading snapshots from older CLI executables (HS2-FG7ZMF).
    #[serde(default)]
    pub source_support: Vec<SourceSupport>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConfiguration {
    Ready,
    Disabled,
    Missing,
    Invalid,
    Reconfigured,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSupport {
    pub connection_id: String,
    pub provider: String,
    pub configuration: SourceConfiguration,
    /// Canonical descriptor capabilities; None means metadata could not be validated.
    pub capabilities: Option<hotsheet_ticketing::ProviderCapabilities>,
    /// Implemented reads only: does not prove credentials, reachability or queue contents.
    pub native_query: bool,
    pub evidence_read: bool,
    pub queue_observed: bool,
    pub claim_verification: bool,
    pub event_subscription: bool,
    pub reasons: Vec<String>,
}

#[cfg(test)]
mod snapshot_compatibility_tests {
    use super::*;

    // Exact snapshot fields before HS2-FG7ZMF; persisted State is shared unchanged.
    #[derive(Serialize, Deserialize)]
    struct BeforeSourceSupport {
        cursor: String,
        state: State,
        live_workers: Vec<String>,
        live_integrator: bool,
        local_queue: Option<Vec<QueueEntry>>,
        queue_support: String,
        uncovered_sources: Vec<String>,
        live_claims: Vec<LiveClaim>,
    }

    #[test]
    fn hs2_fg7zmf_old_snapshot_consumer_preserves_all_legacy_fields_with_additive_report() {
        let (_root, c) = crate::coordination::tests::fixture();
        let current = c.snapshot(1_800_000_000).unwrap();
        let mut encoded = serde_json::to_value(&current).unwrap();
        assert!(!encoded["source_support"].as_array().unwrap().is_empty());
        let old: BeforeSourceSupport = serde_json::from_value(encoded.clone()).unwrap();
        encoded.as_object_mut().unwrap().remove("source_support");
        assert_eq!(serde_json::to_value(old).unwrap(), encoded);
        assert_eq!(current.state.version, 3);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveClaim {
    pub ticket: TicketRef,
    pub worker: String,
    pub expires_at: i64,
}

pub struct WaitRequest {
    pub role: Role,
    pub id: String,
    pub generation: String,
    pub lease_seconds: i64,
    pub return_route: ReturnRoute,
    pub after: Option<String>,
}
