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
    /// HS2-65ETRJ: schema-1 review return preserves the handoff and fences its integration.
    pub fn returned_for_review(&self) -> bool {
        self.phase == Phase::Blocked && self.commit.is_some() && self.recovery_note.is_some()
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
            version: 2,
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
