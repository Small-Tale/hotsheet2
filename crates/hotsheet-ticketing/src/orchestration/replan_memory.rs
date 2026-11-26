//! HS2-BQN0M5, HS2-G6F5BP: what the automatic default-goal re-plan remembers about
//! target tickets, kept machine-locally per owner so a server restart neither re-plans
//! a ticket the planner chose to leave out nor drops one still waiting for an update.
//!
//! The memory holds two things. *Handed* tickets were given to a re-plan once and do
//! not start another while they stay open, even when the planner left them out.
//! *Pending* tickets wait for an update, each with the latest plan update when it was
//! seen: an update newer than that one covers it, because its planner saw the ticket.
//! Every follower of the owner's targets shares the one file under its lock. It is
//! advisory: a file that cannot be decoded reads as empty and is replaced.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::valid_identifier;
use super::storage::{Storage, decode};
use super::*;

/// The directory under the machine home that holds one memory file per owner.
pub const REPLAN_MEMORY_DIR: &str = "orchestration-replan";
/// The most tickets the memory remembers as handed to a re-plan; the oldest key is
/// forgotten first.
pub const MAX_REPLAN_HANDED: usize = 4_096;
/// The longest connection or native id remembered.
const MAX_KEY_BYTES: usize = 512;
const MAX_MEMORY_BYTES: usize = 4 * 1024 * 1024;

/// A target ticket, by connection and canonical native id.
pub type ReplanKey = (String, String);

/// What the automatic re-plan remembers between follower steps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReplanMemory {
    /// Open tickets no step named that were handed to a re-plan.
    handed: BTreeSet<ReplanKey>,
    /// Tickets waiting for an update, with the latest plan update when each was seen.
    pending: BTreeMap<ReplanKey, Option<String>>,
}

impl ReplanMemory {
    /// A read of a ticket no step names. An open one not handed yet becomes pending
    /// (true); a closed or missing one is forgotten, so reopening it counts again.
    pub fn observe(&mut self, key: ReplanKey, open: bool, latest: Option<&str>) -> bool {
        if !open {
            self.handed.remove(&key);
            self.pending.remove(&key);
            return false;
        }
        if self.handed.contains(&key) {
            return false;
        }
        if self.handed.len() >= MAX_REPLAN_HANDED {
            self.handed.pop_first();
        }
        self.handed.insert(key.clone());
        self.pending.insert(key, latest.map(Into::into));
        true
    }

    /// Settle what no longer needs an update: tickets a step names now, and tickets an
    /// update newer than the one they recorded covered.
    pub fn settle(&mut self, planned: impl Fn(&ReplanKey) -> bool, latest: Option<&str>) {
        self.pending
            .retain(|key, after| !planned(key) && after.as_deref() == latest);
    }

    /// An update was asked for every pending ticket.
    pub fn asked(&mut self) {
        self.pending.clear();
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    pub fn handed(&self) -> usize {
        self.handed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handed.is_empty() && self.pending.is_empty()
    }

    /// Bounded, and every key a plausible id: anything else is not this memory.
    fn valid(&self) -> bool {
        let key_ok = |(connection, native): &ReplanKey| {
            !connection.is_empty()
                && !native.is_empty()
                && connection.len() <= MAX_KEY_BYTES
                && native.len() <= MAX_KEY_BYTES
        };
        self.handed.len() <= MAX_REPLAN_HANDED
            && self.pending.len() <= MAX_REPLAN_HANDED
            && self.handed.iter().all(key_ok)
            && self.pending.keys().all(key_ok)
            && self
                .pending
                .values()
                .flatten()
                .all(|update| valid_identifier(update).is_ok())
    }
}

/// A remembered ticket on disk.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketEntry {
    connection_id: String,
    native_id: String,
    /// Pending only: the latest plan update when it was seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryFile {
    schema_version: u32,
    handed: Vec<TicketEntry>,
    pending: Vec<TicketEntry>,
}

impl MemoryFile {
    fn from_memory(memory: &ReplanMemory) -> Self {
        let entry = |(connection_id, native_id): &ReplanKey, after: Option<&String>| TicketEntry {
            connection_id: connection_id.clone(),
            native_id: native_id.clone(),
            after: after.cloned(),
        };
        Self {
            schema_version: ORCHESTRATION_VERSION,
            handed: memory.handed.iter().map(|key| entry(key, None)).collect(),
            pending: memory
                .pending
                .iter()
                .map(|(key, after)| entry(key, after.as_ref()))
                .collect(),
        }
    }

    fn into_memory(self) -> ReplanMemory {
        let key = |entry: &TicketEntry| (entry.connection_id.clone(), entry.native_id.clone());
        ReplanMemory {
            handed: self.handed.iter().map(key).collect(),
            pending: self
                .pending
                .into_iter()
                .map(|entry| (key(&entry), entry.after))
                .collect(),
        }
    }
}

/// One owner's re-plan memory on this machine. Machine-local and private.
pub struct ReplanMemoryStore {
    storage: Storage,
}

impl ReplanMemoryStore {
    pub fn new(home: &Path, owner_checkout_id: &str) -> Result<Self, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(Self {
            storage: Storage::machine_named(
                home,
                &format!("{REPLAN_MEMORY_DIR}/{owner_checkout_id}.json"),
            ),
        })
    }

    /// Held across a read, the step that uses it and the write, so the owner's
    /// followers take turns.
    pub fn lock(&self) -> Result<crate::FileLock, OrchestrationError> {
        self.storage.lock()
    }

    /// The remembered memory. An absent file is empty and is never created by a read;
    /// a file that does not decode, or holds an implausible memory, is `Corrupt`.
    pub fn read(&self) -> Result<ReplanMemory, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_MEMORY_BYTES)? else {
            return Ok(ReplanMemory::default());
        };
        let file: MemoryFile = decode(&bytes)?;
        if file.handed.len() > MAX_REPLAN_HANDED || file.pending.len() > MAX_REPLAN_HANDED {
            return Err(OrchestrationError::Corrupt);
        }
        let memory = file.into_memory();
        if !memory.valid() {
            return Err(OrchestrationError::Corrupt);
        }
        Ok(memory)
    }

    /// Replace the memory; an empty one removes the file. The caller holds
    /// [`Self::lock`].
    pub fn write(&self, memory: &ReplanMemory) -> Result<(), OrchestrationError> {
        if memory.is_empty() {
            return match std::fs::remove_file(&self.storage.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            };
        }
        if !memory.valid() {
            return Err(OrchestrationError::Invalid("implausible re-plan memory"));
        }
        let bytes = serde_json::to_vec(&MemoryFile::from_memory(memory))
            .map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_MEMORY_BYTES)
    }
}
