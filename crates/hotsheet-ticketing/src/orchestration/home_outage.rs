//! HS2-EX594V: a multi-site project's home site out of reach, made visible.
//!
//! A project with several sites runs one coordination pool, on its home site (§26.30,
//! HS2-5P112Z). While the home's machine is offline, its target unbound or disabled in
//! the policy, or its pool unreadable, no dispatcher or integrator starts for the
//! project anywhere: a second home would be a second assignment authority for the same
//! tickets. The maintainer chose no failover (HS2-EX594V). Instead the condition is
//! shown and announced:
//!
//! - [`observe_homes`] reads each multi-site home out of one scheduling pass's inputs.
//!   A home the pass did not read (a project with no work and no session) is not
//!   observed, and what was known of it stays.
//! - [`advance_outages`] folds the observations into this machine's ledger: when the
//!   outage began, why, and whether a person was told.
//! - [`due_notices`] says which notifications are due, debounced: "unavailable" only
//!   once the home has been unavailable without a break for the notify delay
//!   ([`HOME_OUTAGE_NOTIFY_AFTER_MS`]), and "returned" only after an "unavailable" was
//!   sent. Each transition is announced once; a home that flaps within the delay is
//!   never announced.
//!
//! The ledger is machine-local and holds identifiers, codes and times only.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::contract::{valid_identifier, valid_token};
use super::scheduler::ScheduleInput;
use super::storage::{Storage, decode};
use super::*;

/// How long a home must be unavailable without a break before a person is notified.
/// Two minutes rides out a restart or a short network drop; an outage longer than that
/// leaves the project without a dispatcher for long enough to matter.
pub const HOME_OUTAGE_NOTIFY_AFTER_MS: u64 = 2 * 60 * 1000;

/// The directory under the machine home holding one ledger file per owning checkout.
pub const HOME_OUTAGE_DIRECTORY: &str = "orchestration-home-outages";

const MAX_OUTAGE_BYTES: usize = 256 * 1024;
/// The code a pass answers for a target it left alone after a refusal. It says
/// nothing new about the home, so the reason seen before it is kept.
const REFUSAL_BACKOFF: &str = "refusal_backoff";
/// The code of a pass that had no actor to read targets as: nothing was learned.
const ACTOR_REQUIRED: &str = "actor_required";

/// Why a home cannot run its project's coordination sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeOutageReason {
    /// The policy disables the home's target.
    Disabled,
    /// The home's target has no binding on this machine.
    Unbound,
    /// The home's machine could not be read (offline, refused, untrusted…).
    Unreachable,
    /// The home's machine answered but its coordination pool could not be read.
    PoolUnreadable,
}

impl HomeOutageReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Unbound => "unbound",
            Self::Unreachable => "unreachable",
            Self::PoolUnreadable => "pool_unreadable",
        }
    }
}

/// How one pass found one multi-site home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeObservation {
    pub home_target_id: String,
    /// `None`: the home can run its project's coordination sessions.
    pub outage: Option<(HomeOutageReason, Option<String>)>,
}

/// One home's standing in the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeOutage {
    /// The home site: the project's own target.
    pub home_target_id: String,
    /// The project's other sites, where work still runs.
    pub sites: Vec<String>,
    /// When this outage was first seen. Absent once the home was seen available again
    /// and only the "returned" notification is still to be sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<HomeOutageReason>,
    /// The failed read's code, such as `unreachable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// An "unavailable" notification was sent and no "returned" one since.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub announced: bool,
    /// When the home was seen available again after it was announced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returned_at_ms: Option<u64>,
}

impl HomeOutage {
    /// The home is unavailable now.
    pub fn current(&self) -> bool {
        self.since_ms.is_some()
    }
}

/// A notification due for one home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeNoticeKind {
    Unavailable,
    Returned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeNotice {
    pub kind: HomeNoticeKind,
    pub home_target_id: String,
    pub sites: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<HomeOutageReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// When the outage began (unavailable), or when the home came back (returned).
    pub at_ms: u64,
}

/// Each multi-site home of the pass, observed. A home counts only while its project
/// has work or a session, as the pass reads it then; otherwise it is not observed.
/// `bound` says whether this machine holds a binding for a target: a home whose read
/// failed without one is `unbound`, otherwise `unreachable` (a failed read's code does
/// not tell the two apart: both answer `target_unavailable`).
pub fn observe_homes(
    input: &ScheduleInput<'_>,
    bound: &dyn Fn(&str) -> bool,
) -> Vec<HomeObservation> {
    let sites = input.sites;
    let mut observed = Vec::new();
    for home in sites.homes_with_sites() {
        let home = home.as_str();
        let in_project = |target_id: &str| sites.home_of(target_id) == home;
        let busy = input.work.iter().any(|item| {
            in_project(&item.target_id) || item.sites.iter().any(|site| in_project(site))
        }) || input
            .sessions
            .iter()
            .any(|session| in_project(&session.target_id));
        let read = input.targets.iter().find(|state| state.target_id == home);
        let outage = if !input.policy.target(home).enabled {
            if !busy {
                continue;
            }
            Some((HomeOutageReason::Disabled, None))
        } else {
            let Some(read) = read else {
                continue;
            };
            let failed = if bound(home) {
                HomeOutageReason::Unreachable
            } else {
                HomeOutageReason::Unbound
            };
            match (&read.capacity, read.unavailable.as_deref()) {
                (Some(snapshot), _) if snapshot.pool.is_some() => None,
                (Some(_), _) => Some((HomeOutageReason::PoolUnreadable, None)),
                (None, Some(ACTOR_REQUIRED)) => continue,
                (None, code) => Some((failed, code.map(str::to_owned))),
            }
        };
        observed.push(HomeObservation {
            home_target_id: home.to_owned(),
            outage,
        });
    }
    observed
}

/// Fold one pass's observations into the ledger. `sites` is the active grouping: a
/// ledger entry of a target that is no longer a multi-site home is dropped. A home
/// not observed keeps what was known of it.
pub fn advance_outages(
    before: &[HomeOutage],
    observed: &[HomeObservation],
    sites: &ProjectSites,
    now_ms: u64,
) -> Vec<HomeOutage> {
    let homes = sites.homes_with_sites();
    let mut after = Vec::new();
    for home in &homes {
        let others = sites.other_sites(home);
        let known = before
            .iter()
            .find(|entry| &entry.home_target_id == home)
            .cloned();
        let seen = observed
            .iter()
            .find(|observation| &observation.home_target_id == home);
        let next = match (known, seen) {
            (known, None) => known,
            (None, Some(HomeObservation { outage: None, .. })) => None,
            (Some(known), Some(HomeObservation { outage: None, .. })) => {
                known.announced.then(|| HomeOutage {
                    since_ms: None,
                    reason: None,
                    detail: None,
                    returned_at_ms: known.returned_at_ms.or(Some(now_ms)),
                    ..known
                })
            }
            (
                known,
                Some(HomeObservation {
                    outage: Some((reason, detail)),
                    ..
                }),
            ) => {
                let mut entry = known.unwrap_or_else(|| HomeOutage {
                    home_target_id: home.clone(),
                    sites: Vec::new(),
                    since_ms: None,
                    reason: None,
                    detail: None,
                    announced: false,
                    returned_at_ms: None,
                });
                let backoff = detail.as_deref() == Some(REFUSAL_BACKOFF);
                // A home left alone after a refusal is still unavailable for the
                // reason it was refused for.
                if !(backoff && entry.current() && entry.reason.is_some()) {
                    entry.reason = Some(*reason);
                    entry.detail = detail.clone();
                }
                entry.since_ms = entry.since_ms.or(Some(now_ms));
                // Down again before "returned" went out: the outage the person was
                // told about goes on, so neither notice is due.
                entry.returned_at_ms = None;
                Some(entry)
            }
        };
        if let Some(mut entry) = next {
            entry.sites = others;
            after.push(entry);
        }
    }
    after
}

/// The notifications due now, marking each as sent in `ledger`. Entries with nothing
/// left to tell are removed.
pub fn due_notices(
    ledger: &mut Vec<HomeOutage>,
    now_ms: u64,
    notify_after_ms: u64,
) -> Vec<HomeNotice> {
    let mut notices = Vec::new();
    for entry in ledger.iter_mut() {
        match entry.since_ms {
            Some(since) if !entry.announced && now_ms >= since.saturating_add(notify_after_ms) => {
                entry.announced = true;
                notices.push(HomeNotice {
                    kind: HomeNoticeKind::Unavailable,
                    home_target_id: entry.home_target_id.clone(),
                    sites: entry.sites.clone(),
                    reason: entry.reason,
                    detail: entry.detail.clone(),
                    at_ms: since,
                });
            }
            None if entry.announced => {
                entry.announced = false;
                notices.push(HomeNotice {
                    kind: HomeNoticeKind::Returned,
                    home_target_id: entry.home_target_id.clone(),
                    sites: entry.sites.clone(),
                    reason: None,
                    detail: None,
                    at_ms: entry.returned_at_ms.unwrap_or(now_ms),
                });
                entry.returned_at_ms = None;
            }
            _ => {}
        }
    }
    ledger.retain(|entry| entry.current() || entry.announced);
    notices
}

/// When the next "unavailable" notification falls due, so the loop passes again then.
/// A "returned" notification is due at once and needs no wake.
pub fn next_notice_at(ledger: &[HomeOutage], notify_after_ms: u64) -> Option<u64> {
    ledger
        .iter()
        .filter(|entry| !entry.announced)
        .filter_map(|entry| entry.since_ms)
        .map(|since| since.saturating_add(notify_after_ms))
        .min()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutageFile {
    schema_version: u32,
    outages: Vec<HomeOutage>,
}

/// This machine's ledger of home outages, one file per owning checkout under
/// [`HOME_OUTAGE_DIRECTORY`]. Machine-local and private.
#[derive(Clone)]
pub struct HomeOutageStore {
    home: PathBuf,
}

impl HomeOutageStore {
    pub fn new(home: &Path) -> Self {
        Self { home: home.into() }
    }

    /// The directory a reader watches to learn that a ledger changed.
    pub fn directory(&self) -> PathBuf {
        self.home.join(HOME_OUTAGE_DIRECTORY)
    }

    fn storage(&self, owner_id: &str) -> Result<Storage, OrchestrationError> {
        valid_identifier(owner_id)?;
        Ok(Storage::machine_named(
            &self.home,
            &format!("{HOME_OUTAGE_DIRECTORY}/{owner_id}.json"),
        ))
    }

    /// The owner's ledger. An absent file reads as empty and is never created by a read.
    pub fn outages(&self, owner_id: &str) -> Result<Vec<HomeOutage>, OrchestrationError> {
        Self::read(&self.storage(owner_id)?)
    }

    fn read(storage: &Storage) -> Result<Vec<HomeOutage>, OrchestrationError> {
        let Some(bytes) = storage.read(MAX_OUTAGE_BYTES)? else {
            return Ok(Vec::new());
        };
        let file: OutageFile = decode(&bytes)?;
        if file.outages.len() > MAX_TARGETS
            || file.outages.iter().any(|entry| {
                valid_token(&entry.home_target_id).is_err()
                    || entry.sites.len() > MAX_TARGETS
                    || entry.sites.iter().any(|site| valid_token(site).is_err())
            })
        {
            return Err(OrchestrationError::Corrupt);
        }
        Ok(file.outages)
    }

    /// Fold one pass's observations into the owner's ledger and, with `notify`, take
    /// the notifications due now (marking them sent). Returns the ledger after the
    /// pass and the notices. A pass that changes nothing writes nothing.
    pub fn advance(
        &self,
        owner_id: &str,
        observed: &[HomeObservation],
        sites: &ProjectSites,
        now_ms: u64,
        notify: Option<u64>,
    ) -> Result<(Vec<HomeOutage>, Vec<HomeNotice>), OrchestrationError> {
        let storage = self.storage(owner_id)?;
        let _lock = storage.lock()?;
        let before = Self::read(&storage)?;
        let mut after = advance_outages(&before, observed, sites, now_ms);
        let notices = match notify {
            Some(notify_after_ms) => due_notices(&mut after, now_ms, notify_after_ms),
            None => Vec::new(),
        };
        if after != before {
            let file = OutageFile {
                schema_version: ORCHESTRATION_VERSION,
                outages: after.clone(),
            };
            let bytes = serde_json::to_vec(&file).map_err(|_| OrchestrationError::Corrupt)?;
            storage.write(&bytes, MAX_OUTAGE_BYTES)?;
        }
        Ok((after, notices))
    }
}
