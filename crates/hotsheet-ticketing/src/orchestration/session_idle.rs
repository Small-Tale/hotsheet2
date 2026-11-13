//! HS2-95SWQ4: when the orchestrating machine first saw each of its sessions idle.
//!
//! A host says whether a session is in the middle of a turn, not since when. The
//! scheduler winds a session down only after it has been idle for the policy's
//! `idle_minutes`, so the machine that asks remembers the first listing that found it
//! idle. That is a lower bound: a session is never taken to have been idle for longer
//! than someone saw. Work that began and ended between two listings is not seen, and
//! the safe point its host checks at the stop is what protects work, not this time.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{valid_identifier, valid_token};
use super::storage::{Storage, decode};
use super::*;

const MAX_IDLE_BYTES: usize = 512 * 1024;

/// Fold one listing into what was remembered. `Some(true)` is a session seen idle: it
/// keeps the time it was first seen so, or gets `now_ms`. `Some(false)` is one seen
/// working: it is forgotten, so its next idle time starts afresh. `None` is one whose
/// host could not say: nothing is concluded and what was remembered stays. A session
/// the listing does not name is finished and is forgotten.
pub fn observe_idle(
    before: &BTreeMap<String, u64>,
    seen: &[(String, Option<bool>)],
    now_ms: u64,
) -> BTreeMap<String, u64> {
    let mut after = BTreeMap::new();
    for (session_id, idle) in seen {
        let known = before.get(session_id).copied();
        let since = match idle {
            Some(true) => Some(known.unwrap_or(now_ms)),
            Some(false) => None,
            None => known,
        };
        if let Some(since) = since {
            // The same session named twice keeps the earlier time.
            let entry = after.entry(session_id.clone()).or_insert(since);
            *entry = (*entry).min(since);
        }
    }
    after
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdleFile {
    schema_version: u32,
    owners: BTreeMap<String, BTreeMap<String, u64>>,
}

/// This machine's idle memory, per owning checkout. Machine-local and private.
#[derive(Clone)]
pub struct SessionIdleStore {
    storage: Storage,
}

impl SessionIdleStore {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-session-idle.json"),
        }
    }

    fn read_file(&self) -> Result<IdleFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_IDLE_BYTES)? else {
            return Ok(IdleFile {
                schema_version: ORCHESTRATION_VERSION,
                owners: BTreeMap::new(),
            });
        };
        let file: IdleFile = decode(&bytes)?;
        for (owner, sessions) in &file.owners {
            valid_identifier(owner).map_err(|_| OrchestrationError::Corrupt)?;
            if sessions.len() > MAX_SESSIONS
                || sessions.keys().any(|session| valid_token(session).is_err())
            {
                return Err(OrchestrationError::Corrupt);
            }
        }
        Ok(file)
    }

    /// An absent file reads as empty and is never created by a read.
    pub fn idle_since(&self, owner_id: &str) -> Result<BTreeMap<String, u64>, OrchestrationError> {
        valid_identifier(owner_id)?;
        Ok(self
            .read_file()?
            .owners
            .remove(owner_id)
            .unwrap_or_default())
    }

    /// Fold one listing of the owner's unfinished sessions into its memory
    /// ([`observe_idle`]) and return the result. A listing that changes nothing writes
    /// nothing.
    pub fn observe(
        &self,
        owner_id: &str,
        seen: &[(String, Option<bool>)],
        now_ms: u64,
    ) -> Result<BTreeMap<String, u64>, OrchestrationError> {
        valid_identifier(owner_id)?;
        if seen.len() > MAX_SESSIONS {
            return Err(OrchestrationError::Capacity);
        }
        for (session_id, _) in seen {
            valid_token(session_id)?;
        }
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let before = file.owners.get(owner_id).cloned().unwrap_or_default();
        let after = observe_idle(&before, seen, now_ms);
        if after == before {
            return Ok(after);
        }
        if after.is_empty() {
            file.owners.remove(owner_id);
        } else {
            file.owners.insert(owner_id.into(), after.clone());
        }
        let bytes = serde_json::to_vec(&file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_IDLE_BYTES)?;
        Ok(after)
    }
}
