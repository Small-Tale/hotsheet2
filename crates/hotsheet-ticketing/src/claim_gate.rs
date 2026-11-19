//! Exact ticket claims through a bound claim backend (HS2-WJF0D8, `docs/36` §36.14).
//!
//! A Git ticket store whose data source is bound to a `git_ref` configuration
//! (`claim_backends`) takes every claim through the remote's compare-and-swap before the
//! ticket's frontmatter changes, so two machines sharing the store never both hold one
//! ticket. The frontmatter mirrors the marker:
//!
//! - **claim** ([`gate_claim`]): acquire `refs/hotsheet/claims/<ULID>` for the new claim
//!   attempt; a live marker of another attempt refuses the claim and the local ticket is
//!   repaired to name that holder.
//! - **renew** ([`gate_renew`]): extend the attempt's own marker; a missing marker (a
//!   provisional claim, or one taken before the binding) is acquired now.
//! - **release** ([`after_write`]): a write that ended a claim deletes its marker, which
//!   covers explicit release, completion and every other terminal transition.
//! - **offline**: `fail_closed` refuses with [`StoreError::ClaimBackendUnreachable`];
//!   `provisional` writes the claim locally, records it as provisional (never reported as
//!   exclusive), and reconciles it on the next renew or sync.
//! - **lost in a merge** ([`reconcile`]): after a sync pulls, every live claim this host
//!   holds is checked against its marker; a claim another machine holds is repaired to
//!   that holder and recorded as a conflict the losing session reads
//!   ([`take_conflicts`]), and its next renew is refused naming the holder.
//!
//! An unbound store, or one bound to a `local` configuration, behaves exactly as before.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use hotsheet_model::{ClaimEvent, ClaimEventKind, Ticket, Timestamp, Ulid};

use crate::claim_backends::{BackendKind, Registries, git_store_sources};
use crate::gitref_cas::{Acquired, CasError, GitRefCas, RefMarker, millis};
use crate::orchestration::OfflinePolicy;
use crate::{FsStore, StoreError};

/// The claim backend a store's tickets go through.
#[derive(Debug, Clone)]
pub struct TicketGate {
    pub configuration: String,
    pub cas: GitRefCas,
    pub offline: OfflinePolicy,
}

fn config_error(error: impl std::fmt::Display) -> StoreError {
    StoreError::Io(std::io::Error::other(format!(
        "claim backend configuration: {error}"
    )))
}

/// The gate of `store`, from its own project registry and the machine's.
pub fn ticket_gate(store: &FsStore) -> Result<Option<TicketGate>, StoreError> {
    ticket_gate_at(&crate::settings::hotsheet_home(), store)
}

/// [`ticket_gate`] with an explicit machine home.
pub fn ticket_gate_at(home: &Path, store: &FsStore) -> Result<Option<TicketGate>, StoreError> {
    let registries = Registries::load(home, Some(store.root())).map_err(config_error)?;
    let Some(resolved) = registries
        .source(&git_store_sources(store))
        .map_err(config_error)?
    else {
        return Ok(None);
    };
    let configuration = resolved.configuration;
    Ok(match configuration.kind {
        BackendKind::Local => None,
        BackendKind::GitRef => {
            let repository = configuration
                .setting("repository")
                .map_or_else(|| store.root().to_path_buf(), PathBuf::from);
            let remote = configuration.setting("remote").unwrap_or("origin");
            Some(TicketGate {
                cas: GitRefCas::new(repository, remote),
                offline: configuration.offline,
                configuration: configuration.id,
            })
        }
    })
}

/// The id of `worker`'s latest claim attempt on `ticket`.
pub fn attempt_id(ticket: &Ticket, worker: &str) -> Option<Ulid> {
    ticket
        .claim_history
        .iter()
        .rev()
        .find(|event| event.kind == ClaimEventKind::Claim && event.worker == worker)
        .map(|event| event.id)
}

/// What the backend said about a claim or renewal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateOutcome {
    /// The backend holds the claim for this attempt.
    Exclusive,
    /// The backend was unreachable and the configuration allows a provisional claim.
    Provisional,
    /// Another claim holds the ticket.
    HeldBy(Box<RefMarker>),
}

fn unreachable(gate: &TicketGate, ticket: &Ticket, message: &str) -> StoreError {
    StoreError::ClaimBackendUnreachable(format!(
        "{}: claim backend '{}' is unreachable ({message}); nothing was claimed",
        ticket.slug, gate.configuration
    ))
}

fn offline(
    store: &FsStore,
    gate: &TicketGate,
    ticket: &Ticket,
    attempt: &Ulid,
    worker: &str,
    error: CasError,
) -> Result<GateOutcome, StoreError> {
    let message = error.to_string();
    match (gate.offline, error) {
        (OfflinePolicy::Provisional, CasError::Unreachable(_)) => {
            record_provisional(store, ticket.id, attempt, worker)?;
            Ok(GateOutcome::Provisional)
        }
        _ => Err(unreachable(gate, ticket, &message)),
    }
}

/// Take the backend claim for `ticket`'s current attempt by `worker` (its frontmatter
/// already names the new lease). Called inside the store transaction, before the write.
pub fn gate_claim(
    store: &FsStore,
    gate: &TicketGate,
    ticket: &Ticket,
    worker: &str,
    now: &Timestamp,
) -> Result<GateOutcome, StoreError> {
    let attempt = attempt_id(ticket, worker)
        .ok_or_else(|| config_error("a claim write without a claim event"))?;
    let lease = ticket.claim_lease_expires_at.as_ref().map_or(0, millis);
    let now_ms = millis(now);
    let mut marker = RefMarker::new(worker, &attempt.to_string(), now_ms, lease);
    marker.host.clone_from(&ticket.claim_host);
    match gate
        .cas
        .acquire(&GitRefCas::ticket_ref(&ticket.id), marker, now_ms)
    {
        Ok(Acquired::Won(_)) => {
            clear_provisional(store, ticket.id);
            Ok(GateOutcome::Exclusive)
        }
        Ok(Acquired::Held(holder)) => {
            // A provisional claim that lost is settled: the caller adopts the holder.
            clear_provisional(store, ticket.id);
            Ok(GateOutcome::HeldBy(Box::new(holder)))
        }
        Err(error) => offline(store, gate, ticket, &attempt, worker, error),
    }
}

/// Extend the backend claim of `worker`'s attempt to `lease`. A marker that is gone is
/// acquired again (a provisional claim reconciling, or a claim from before the binding).
pub fn gate_renew(
    store: &FsStore,
    gate: &TicketGate,
    ticket: &Ticket,
    worker: &str,
    now: &Timestamp,
    lease: &Timestamp,
) -> Result<GateOutcome, StoreError> {
    let Some(attempt) = attempt_id(ticket, worker) else {
        return gate_claim(store, gate, ticket, worker, now);
    };
    let refname = GitRefCas::ticket_ref(&ticket.id);
    match gate
        .cas
        .renew(&refname, &attempt.to_string(), millis(lease))
    {
        Ok(Ok(_)) => {
            clear_provisional(store, ticket.id);
            Ok(GateOutcome::Exclusive)
        }
        Ok(Err(Some(holder))) if holder.is_live(millis(now)) => {
            clear_provisional(store, ticket.id);
            Ok(GateOutcome::HeldBy(Box::new(holder)))
        }
        Ok(Err(_)) => {
            let mut renewed = ticket.clone();
            renewed.claim_lease_expires_at = Some(lease.clone());
            gate_claim(store, gate, &renewed, worker, now)
        }
        Err(error) => offline(store, gate, ticket, &attempt, worker, error),
    }
}

/// Repair `ticket`'s frontmatter to name the backend's holder: the local claim (if any)
/// ends and the holder's claim attempt is recorded with the marker's own id, so the
/// holder's renewals keep matching.
pub fn adopt_holder(ticket: &mut Ticket, holder: &RefMarker, now: &Timestamp) {
    if ticket.claimed_by.as_deref() == Some(holder.worker.as_str())
        && attempt_id(ticket, &holder.worker).map(|id| id.to_string())
            == Some(holder.claim_id.clone())
    {
        ticket.claim_lease_expires_at = Some(holder.expires_at.clone());
        return;
    }
    if let Some(local) = ticket.claimed_by.clone() {
        crate::ops::append_claim_event(ticket, ClaimEventKind::Release, &local, now, None, None);
    }
    ticket.claimed_by = Some(holder.worker.clone());
    ticket.claim_lease_expires_at = Some(holder.expires_at.clone());
    ticket.claim_eta_at = None;
    ticket.worker_label = None;
    ticket.claim_host.clone_from(&holder.host);
    let id = Ulid::from_string(&holder.claim_id).unwrap_or_else(|_| Ulid::new());
    if !ticket.claim_history.iter().any(|event| event.id == id) {
        ticket.claim_history.push(ClaimEvent {
            id,
            kind: ClaimEventKind::Claim,
            worker: holder.worker.clone(),
            at: holder.claimed_at.clone(),
            lease_expires_at: Some(holder.expires_at.clone()),
            eta_at: None,
            worker_label: None,
            host: holder.host.clone(),
        });
    }
    ticket.updated_at = now.clone();
}

/// After a ticket write: when the write ended a claim, delete that attempt's marker.
/// Best effort; a marker left behind lapses at its lease.
pub fn after_write(store: &FsStore, ticket: &Ticket) {
    let mut events = ticket.claim_history.iter().rev();
    let Some(last) = events.next() else {
        return;
    };
    if last.kind != ClaimEventKind::Release
        || ticket.claimed_by.is_some()
        || last.at != ticket.updated_at
    {
        return;
    }
    let Some(attempt) = events
        .find(|event| event.kind == ClaimEventKind::Claim && event.worker == last.worker)
        .map(|event| event.id)
    else {
        return;
    };
    clear_provisional(store, ticket.id);
    if let Ok(Some(gate)) = ticket_gate(store) {
        let _ = gate
            .cas
            .release(&GitRefCas::ticket_ref(&ticket.id), &attempt.to_string());
    }
}

// ---- provisional claims and conflicts (machine-local, in the store's Git directory) ----

const PROVISIONAL_FILE: &str = "hotsheet-provisional-claims.json";
const CONFLICTS_FILE: &str = "hotsheet-claim-conflicts.json";
const MAX_CONFLICTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvisionalClaim {
    pub claim_id: String,
    pub worker: String,
}

/// A claim this host held locally that the backend gives to another claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimConflict {
    pub ticket: Ulid,
    pub slug: String,
    /// The local worker that lost the ticket.
    pub worker: String,
    /// The worker the backend names.
    pub holder: String,
    pub detected_at: Timestamp,
}

fn git_path(store: &FsStore, name: &str) -> Option<PathBuf> {
    let output = crate::git::command_in(store.root())
        .args(["rev-parse", "--git-path", name])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    Some(if path.is_absolute() {
        path
    } else {
        store.root().join(path)
    })
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    let json = serde_json::to_string_pretty(value)?;
    crate::store::write_file_atomically(path, json.as_bytes())?;
    Ok(())
}

fn record_provisional(
    store: &FsStore,
    ticket: Ulid,
    attempt: &Ulid,
    worker: &str,
) -> Result<(), StoreError> {
    let path = git_path(store, PROVISIONAL_FILE)
        .ok_or_else(|| config_error("the store has no Git directory for provisional claims"))?;
    let mut claims: BTreeMap<Ulid, ProvisionalClaim> = read_json(&path);
    claims.insert(
        ticket,
        ProvisionalClaim {
            claim_id: attempt.to_string(),
            worker: worker.to_owned(),
        },
    );
    write_json(&path, &claims)
}

fn clear_provisional(store: &FsStore, ticket: Ulid) {
    let Some(path) = git_path(store, PROVISIONAL_FILE) else {
        return;
    };
    let mut claims: BTreeMap<Ulid, ProvisionalClaim> = read_json(&path);
    if claims.remove(&ticket).is_some() {
        let _ = write_json(&path, &claims);
    }
}

/// Claims this clone took while the backend was unreachable and has not yet confirmed.
/// They are advisory: never report them as exclusive.
pub fn provisional_claims(store: &FsStore) -> BTreeMap<Ulid, ProvisionalClaim> {
    git_path(store, PROVISIONAL_FILE)
        .map(|path| read_json(&path))
        .unwrap_or_default()
}

fn record_conflict(store: &FsStore, conflict: ClaimConflict) {
    let Some(path) = git_path(store, CONFLICTS_FILE) else {
        return;
    };
    let mut conflicts: Vec<ClaimConflict> = read_json(&path);
    conflicts.push(conflict);
    let excess = conflicts.len().saturating_sub(MAX_CONFLICTS);
    conflicts.drain(..excess);
    let _ = write_json(&path, &conflicts);
}

/// The claim conflicts recorded since the last call, oldest first, and forget them.
pub fn take_conflicts(store: &FsStore) -> Vec<ClaimConflict> {
    let Some(path) = git_path(store, CONFLICTS_FILE) else {
        return Vec::new();
    };
    let conflicts: Vec<ClaimConflict> = read_json(&path);
    if !conflicts.is_empty() {
        let _ = std::fs::remove_file(&path);
    }
    conflicts
}

/// Check every live claim this host holds in `store` against its backend marker: confirm
/// provisional claims, and repair and report a claim another machine holds. Returns the
/// conflicts found. Does nothing for an unbound store.
pub fn reconcile(store: &FsStore, now: &Timestamp) -> Result<Vec<ClaimConflict>, StoreError> {
    let Some(gate) = ticket_gate(store)? else {
        return Ok(Vec::new());
    };
    let host = crate::claim_host::local_host_id();
    let mut conflicts = Vec::new();
    for ticket in store.list_tickets()? {
        let Some(worker) = ticket.claimed_by.clone() else {
            continue;
        };
        if crate::ops::claim_available(&ticket, now) || ticket.claim_host != host {
            continue;
        }
        let lease = ticket
            .claim_lease_expires_at
            .clone()
            .unwrap_or_else(|| now.clone());
        let outcome = match gate_renew(store, &gate, &ticket, &worker, now, &lease) {
            Ok(outcome) => outcome,
            Err(StoreError::ClaimBackendUnreachable(_)) => break,
            Err(error) => return Err(error),
        };
        if let GateOutcome::HeldBy(holder) = outcome {
            store.with_ticket_transaction(|| {
                let mut current = store.read_ticket(&ticket.id)?;
                adopt_holder(&mut current, &holder, now);
                store.write_ticket_committing(&current)?;
                Ok::<_, StoreError>(())
            })?;
            clear_provisional(store, ticket.id);
            let conflict = ClaimConflict {
                ticket: ticket.id,
                slug: ticket.slug.clone(),
                worker,
                holder: holder.worker.clone(),
                detected_at: now.clone(),
            };
            record_conflict(store, conflict.clone());
            conflicts.push(conflict);
        }
    }
    Ok(conflicts)
}

#[cfg(test)]
#[path = "claim_gate_tests.rs"]
mod tests;
