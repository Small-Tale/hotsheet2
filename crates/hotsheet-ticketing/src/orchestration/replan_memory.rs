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
//!
//! HS2-04WW8B adds the *seen* baseline: the open tickets a follower's catch-up listed
//! (or a ticket event showed) on each *seeded* target. A handshake, an overflow or a
//! reconnect lists the target's open tickets; one neither seen, handed nor named by a
//! step was created while no follower saw it, and becomes pending. The first catch-up
//! of a target only seeds its baseline: the default-goal planner already works
//! through every open ticket then.
//!
//! HS2-A0853J: the planner read the target when Propose Plan started it, which can be
//! well before the follower's first catch-up. So Propose Plan lists each target first
//! and seeds the baseline with what the planner is about to read
//! ([`ReplanMemory::planner_read`]). The follower's first catch-up then finds the target
//! seeded, and a ticket created in between becomes pending instead of being taken as
//! seen.

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
/// HS2-04WW8B: the most open tickets the seen baseline holds; past it the smallest key
/// is forgotten first (it may count as new once at a later catch-up).
pub const MAX_REPLAN_SEEN: usize = 8_192;
/// HS2-04WW8B: the most targets whose baseline is seeded.
pub const MAX_REPLAN_TARGETS: usize = 256;
/// The longest connection or native id remembered.
const MAX_KEY_BYTES: usize = 512;
/// Room for every bound at its longest ids (HS2-04WW8B).
const MAX_MEMORY_BYTES: usize = 32 * 1024 * 1024;

/// A target ticket, by connection and canonical native id.
pub type ReplanKey = (String, String);

/// What the automatic re-plan remembers between follower steps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReplanMemory {
    /// Open tickets no step named that were handed to a re-plan.
    handed: BTreeSet<ReplanKey>,
    /// Tickets waiting for an update, with the latest plan update when each was seen.
    pending: BTreeMap<ReplanKey, Option<String>>,
    /// HS2-04WW8B: open tickets a catch-up listed or an event showed.
    seen: BTreeSet<ReplanKey>,
    /// HS2-04WW8B: targets whose baseline was seeded by a first catch-up.
    seeded: BTreeSet<String>,
}

/// HS2-04WW8B: one catch-up listing of a target's open tickets.
#[derive(Debug, Clone, Copy)]
pub struct CatchUp<'a> {
    pub target_id: &'a str,
    /// The target's ticket sources that were listed.
    pub connections: &'a [String],
    /// Every open ticket of those sources was listed (no page bound was reached), so
    /// a remembered ticket that is absent closed meanwhile.
    pub complete: bool,
}

impl ReplanMemory {
    /// A read of a ticket no step names. An open one neither handed nor seen yet
    /// becomes pending (true); a closed or missing one is forgotten, so reopening it
    /// counts again. HS2-2AZQD2: a seen ticket (in the baseline a catch-up listed, or
    /// shown by an earlier event) is not new, so editing it starts no re-plan, like a
    /// handed one.
    pub fn observe(&mut self, key: ReplanKey, open: bool, latest: Option<&str>) -> bool {
        if !open {
            self.handed.remove(&key);
            self.pending.remove(&key);
            self.seen.remove(&key);
            return false;
        }
        let known = self.seen.contains(&key) || self.handed.contains(&key);
        self.saw(key.clone());
        if known {
            return false;
        }
        if self.handed.len() >= MAX_REPLAN_HANDED {
            self.handed.pop_first();
        }
        self.handed.insert(key.clone());
        self.pending.insert(key, latest.map(Into::into));
        true
    }

    /// HS2-04WW8B: an open ticket was seen (an event of a ticket a step names, say), so
    /// a later catch-up does not count it as new.
    pub fn saw(&mut self, key: ReplanKey) {
        if self.seen.contains(&key) {
            return;
        }
        if self.seen.len() >= MAX_REPLAN_SEEN {
            self.seen.pop_first();
        }
        self.seen.insert(key);
    }

    /// Whether the target's baseline was seeded.
    pub fn seeded(&self, target_id: &str) -> bool {
        self.seeded.contains(target_id)
    }

    /// HS2-A0853J: a planner about to read the whole target (Propose Plan) sees `listed`,
    /// the target's open tickets now. The target is seeded with them as seen, and none
    /// of them waits for an update any more: that planner reads every one. Tickets
    /// handed before stay handed. A complete listing forgets remembered tickets of its
    /// sources that closed, as a catch-up does. A later first catch-up of the target
    /// hands what was created since.
    pub fn planner_read(&mut self, listing: CatchUp<'_>, listed: &[ReplanKey]) {
        if !self.seeded.contains(listing.target_id) {
            if self.seeded.len() >= MAX_REPLAN_TARGETS {
                self.seeded.pop_first();
            }
            self.seeded.insert(listing.target_id.to_owned());
        }
        self.forget_closed(listing, listed);
        for key in listed {
            self.pending.remove(key);
            self.saw(key.clone());
        }
    }

    /// A complete listing's sources hold only the listed open tickets: forget the
    /// remembered ones it did not list, so reopening one counts again.
    fn forget_closed(&mut self, listing: CatchUp<'_>, listed: &[ReplanKey]) {
        if listing.complete {
            let gone =
                |key: &ReplanKey| listing.connections.contains(&key.0) && !listed.contains(key);
            self.seen.retain(|key| !gone(key));
            self.handed.retain(|key| !gone(key));
            self.pending.retain(|key, _| !gone(key));
        }
    }

    /// HS2-04WW8B: a catch-up listed `listed`, the target's open tickets. The first one
    /// of a target seeds its baseline and hands nothing. Later, an open ticket neither
    /// seen, handed nor `planned` becomes pending with `latest`, the latest plan update
    /// now. A complete listing also forgets the remembered tickets of those sources it
    /// did not list: they closed meanwhile, so reopening one counts again. Answers how
    /// many tickets became pending.
    pub fn catch_up(
        &mut self,
        listing: CatchUp<'_>,
        listed: &[ReplanKey],
        planned: impl Fn(&ReplanKey) -> bool,
        latest: Option<&str>,
    ) -> usize {
        let mut fresh = 0;
        if self.seeded.contains(listing.target_id) {
            for key in listed {
                if !self.seen.contains(key) && !self.handed.contains(key) && !planned(key) {
                    fresh += usize::from(self.observe(key.clone(), true, latest));
                }
            }
        } else {
            if self.seeded.len() >= MAX_REPLAN_TARGETS {
                self.seeded.pop_first();
            }
            self.seeded.insert(listing.target_id.to_owned());
        }
        self.forget_closed(listing, listed);
        for key in listed {
            self.saw(key.clone());
        }
        fresh
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

    /// HS2-04WW8B: how many open tickets the baseline holds.
    pub fn seen(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handed.is_empty()
            && self.pending.is_empty()
            && self.seen.is_empty()
            && self.seeded.is_empty()
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
            && self.seen.len() <= MAX_REPLAN_SEEN
            && self.seeded.len() <= MAX_REPLAN_TARGETS
            && self.handed.iter().all(key_ok)
            && self.pending.keys().all(key_ok)
            && self.seen.iter().all(key_ok)
            && self
                .seeded
                .iter()
                .all(|target| valid_identifier(target).is_ok())
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
    /// HS2-04WW8B; absent from a file written before the baseline existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    seen: Vec<TicketEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    seeded: Vec<String>,
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
            seen: memory.seen.iter().map(|key| entry(key, None)).collect(),
            seeded: memory.seeded.iter().cloned().collect(),
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
            seen: self.seen.iter().map(key).collect(),
            seeded: self.seeded.into_iter().collect(),
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
        if file.handed.len() > MAX_REPLAN_HANDED
            || file.pending.len() > MAX_REPLAN_HANDED
            || file.seen.len() > MAX_REPLAN_SEEN
            || file.seeded.len() > MAX_REPLAN_TARGETS
        {
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
