//! HS2-KB2Y51: orchestration lines another machine pushed for a checkout hosted here.
//!
//! The machine that runs an orchestration project answers its remote targets' lines
//! itself (HS2-3D8D6V). So that anyone who views the target sees them too, it pushes
//! those line facts to the target's server, which keeps them here as transient,
//! expiring, display-only observations: never plan records, never git or a ticket
//! store. One entry per owner machine; each push replaces that machine's entry, an
//! empty push withdraws it, and an entry the owner stops refreshing expires.
//!
//! The pure rules are [`ObservationFile::apply`] and [`ObservationFile::live`]; the
//! registry keeps one bounded machine-local file per checkout under
//! [`LINE_OBSERVATION_DIRECTORY`].

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::valid_identifier;
use super::storage::{Storage, decode};
use super::target_lines::TargetLine;
use super::timeline::TargetLocation;
use super::{OrchestrationError, TargetOpError};

/// Under `HOTSHEET_HOME`: one `<checkout>.json` per checkout with observations.
pub const LINE_OBSERVATION_DIRECTORY: &str = "orchestration-line-observations";
/// At most this many owner machines keep observations for one checkout.
pub const MAX_OBSERVATION_OWNERS: usize = 8;
/// At most this many lines in one push.
pub const MAX_OBSERVED_LINES: usize = 256;
/// At most this many bytes in one push's lines, and in one line.
pub const MAX_OBSERVATION_BYTES: usize = 256 * 1024;
const MAX_LINE_BYTES: usize = 4 * 1024;
/// The lifetime an owner may ask for, and the one it gets when it names none.
pub const MIN_OBSERVATION_TTL_MS: u64 = 30_000;
pub const MAX_OBSERVATION_TTL_MS: u64 = 15 * 60_000;
pub const DEFAULT_OBSERVATION_TTL_MS: u64 = 10 * 60_000;
/// At most this many pushes from one owner machine per minute for one checkout.
pub const MAX_PUSHES_PER_MINUTE: usize = 30;
const MINUTE_MS: u64 = 60_000;
const OBSERVATION_VERSION: u32 = 1;
// Every owner's bounded lines plus envelope.
const MAX_FILE_BYTES: usize = MAX_OBSERVATION_OWNERS * (MAX_OBSERVATION_BYTES + 4096) + 4096;

/// What an owner machine pushes for one checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedLines {
    /// The pushing machine's server id.
    pub owner_server_id: String,
    /// Increases with every push from that machine; an older one is ignored.
    pub sequence: u64,
    /// How long the lines stay without a refresh; clamped to the allowed range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    /// The lines for this checkout. Empty withdraws the machine's lines.
    #[serde(default)]
    pub lines: Vec<TargetLine>,
}

/// What a push did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationOutcome {
    /// `stored`, `withdrawn` or `stale` (an older or repeated sequence; nothing changed).
    pub outcome: String,
    /// Lines kept.
    pub accepted: u32,
    /// Lines dropped because their ticket is not in one of this checkout's sources.
    pub dropped: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationEntry {
    pub owner_server_id: String,
    pub sequence: u64,
    pub received_at_ms: u64,
    pub expires_at_ms: u64,
    pub lines: Vec<TargetLine>,
    /// When this owner pushed within the last minute, for the rate bound.
    #[serde(default)]
    pub pushes_ms: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationFile {
    pub schema_version: u32,
    /// In the order the owners first pushed.
    pub entries: Vec<ObservationEntry>,
}

impl Default for ObservationFile {
    fn default() -> Self {
        Self {
            schema_version: OBSERVATION_VERSION,
            entries: Vec::new(),
        }
    }
}

/// One owner machine's live lines, for the merged fold.
pub struct ObservedOwner<'a> {
    pub owner_server_id: &'a str,
    pub lines: &'a [TargetLine],
}

/// A line as another machine may show it: the display facts only. Run, session,
/// permission and decision identifiers stay on the owner's machine, and the work
/// runs on the machine that hosts the target, so the receiver reads it as local.
pub fn display_line(line: &TargetLine) -> TargetLine {
    let mut line = line.clone();
    let item = &mut line.item;
    item.session_id = None;
    item.session_state = None;
    item.reasons.clear();
    item.run = None;
    item.permission = None;
    item.decision = None;
    item.location = TargetLocation::Local;
    item.machine_label = None;
    if let Some(placement) = &mut item.placement {
        placement.session_id = None;
    }
    line
}

impl ObservationFile {
    /// Drop expired entries and rate stamps older than a minute.
    fn prune(&mut self, now_ms: u64) {
        self.entries.retain(|entry| entry.expires_at_ms > now_ms);
        for entry in &mut self.entries {
            entry
                .pushes_ms
                .retain(|at| now_ms.saturating_sub(*at) < MINUTE_MS);
        }
    }

    /// The live entries, in the order their owners first pushed.
    pub fn live(&self, now_ms: u64) -> Vec<&ObservationEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.expires_at_ms > now_ms && !entry.lines.is_empty())
            .collect()
    }

    /// Apply one push for a checkout whose sources are `sources`. Lines naming a
    /// connection not in `sources` are dropped; the rest are kept as display lines.
    pub fn apply(
        &mut self,
        post: ObservedLines,
        sources: &[String],
        now_ms: u64,
    ) -> Result<ObservationOutcome, OrchestrationError> {
        let owner = post
            .owner_server_id
            .parse::<ulid::Ulid>()
            .ok()
            .map(|id| id.to_string());
        if owner.as_deref() != Some(post.owner_server_id.as_str()) {
            return Err(OrchestrationError::Invalid(
                "canonical owner server id required",
            ));
        }
        if post.lines.len() > MAX_OBSERVED_LINES {
            return Err(OrchestrationError::Invalid("too many observed lines"));
        }
        let mut total = 0usize;
        for line in &post.lines {
            let size = serde_json::to_vec(line)
                .map_err(|_| OrchestrationError::Corrupt)?
                .len();
            if size > MAX_LINE_BYTES {
                return Err(OrchestrationError::Invalid("observed line too large"));
            }
            total += size;
            valid_identifier(&line.owner_checkout_id)?;
        }
        if total > MAX_OBSERVATION_BYTES {
            return Err(OrchestrationError::Invalid("observed lines too large"));
        }
        self.prune(now_ms);
        let at = self
            .entries
            .iter()
            .position(|entry| entry.owner_server_id == post.owner_server_id);
        if let Some(at) = at {
            let entry = &self.entries[at];
            if post.sequence <= entry.sequence {
                return Ok(ObservationOutcome {
                    outcome: "stale".into(),
                    accepted: 0,
                    dropped: 0,
                    expires_at_ms: Some(entry.expires_at_ms),
                });
            }
            if entry.pushes_ms.len() >= MAX_PUSHES_PER_MINUTE {
                return Err(OrchestrationError::TargetOperation(
                    TargetOpError::RateLimited,
                ));
            }
        }
        if post.lines.is_empty() {
            if let Some(at) = at {
                self.entries.remove(at);
            }
            return Ok(ObservationOutcome {
                outcome: "withdrawn".into(),
                accepted: 0,
                dropped: 0,
                expires_at_ms: None,
            });
        }
        if at.is_none() && self.entries.len() >= MAX_OBSERVATION_OWNERS {
            return Err(OrchestrationError::Capacity);
        }
        let ttl = post
            .ttl_ms
            .unwrap_or(DEFAULT_OBSERVATION_TTL_MS)
            .clamp(MIN_OBSERVATION_TTL_MS, MAX_OBSERVATION_TTL_MS);
        let offered = post.lines.len();
        let lines: Vec<TargetLine> = post
            .lines
            .iter()
            .filter(|line| sources.contains(&line.item.target.connection_id))
            .map(|line| {
                let mut line = display_line(line);
                line.observed_from = Some(post.owner_server_id.clone());
                line
            })
            .collect();
        let accepted = lines.len();
        let expires_at_ms = now_ms.saturating_add(ttl);
        let mut pushes_ms = at
            .map(|at| std::mem::take(&mut self.entries[at].pushes_ms))
            .unwrap_or_default();
        pushes_ms.push(now_ms);
        let entry = ObservationEntry {
            owner_server_id: post.owner_server_id,
            sequence: post.sequence,
            received_at_ms: now_ms,
            expires_at_ms,
            lines,
            pushes_ms,
        };
        match at {
            Some(at) => self.entries[at] = entry,
            None => self.entries.push(entry),
        }
        Ok(ObservationOutcome {
            outcome: "stored".into(),
            accepted: u32::try_from(accepted).unwrap_or(u32::MAX),
            dropped: u32::try_from(offered - accepted).unwrap_or(u32::MAX),
            expires_at_ms: Some(expires_at_ms),
        })
    }
}

/// The machine-local observations of every checkout hosted here.
#[derive(Clone)]
pub struct LineObservations {
    home: std::path::PathBuf,
}

impl LineObservations {
    pub fn new(home: &Path) -> Self {
        Self { home: home.into() }
    }

    /// The directory a waiting read watches.
    pub fn directory(&self) -> std::path::PathBuf {
        self.home.join(LINE_OBSERVATION_DIRECTORY)
    }

    fn storage(&self, checkout_id: &str) -> Result<Storage, OrchestrationError> {
        valid_identifier(checkout_id)?;
        Ok(Storage::machine_named(
            &self.home,
            &format!("{LINE_OBSERVATION_DIRECTORY}/{checkout_id}.json"),
        ))
    }

    fn read_file(storage: &Storage) -> Result<ObservationFile, OrchestrationError> {
        let Some(bytes) = storage.read(MAX_FILE_BYTES)? else {
            return Ok(ObservationFile::default());
        };
        let file: ObservationFile = decode(&bytes)?;
        if file.schema_version != OBSERVATION_VERSION {
            return Err(OrchestrationError::UnsupportedVersion {
                found: u64::from(file.schema_version),
            });
        }
        Ok(file)
    }

    /// Apply one push under the checkout's lock.
    pub fn put(
        &self,
        checkout_id: &str,
        post: ObservedLines,
        sources: &[String],
        now_ms: u64,
    ) -> Result<ObservationOutcome, OrchestrationError> {
        let storage = self.storage(checkout_id)?;
        let _lock = storage.lock()?;
        let mut file = Self::read_file(&storage)?;
        let outcome = file.apply(post, sources, now_ms)?;
        if outcome.outcome != "stale" {
            let bytes = serde_json::to_vec(&file).map_err(|_| OrchestrationError::Corrupt)?;
            storage.write(&bytes, MAX_FILE_BYTES)?;
        }
        Ok(outcome)
    }

    /// The live entries for a checkout, owner by owner. Never creates anything, and
    /// a file that cannot be read shows no observations rather than failing the read
    /// of this machine's own lines.
    pub fn live(&self, checkout_id: &str, now_ms: u64) -> Vec<ObservationEntry> {
        let Ok(storage) = self.storage(checkout_id) else {
            return Vec::new();
        };
        Self::read_file(&storage)
            .map(|file| file.live(now_ms).into_iter().cloned().collect())
            .unwrap_or_default()
    }
}
