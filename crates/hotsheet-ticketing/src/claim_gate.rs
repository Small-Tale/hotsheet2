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
//! - **holder fields** (HS2-EWGPTZ): when the configuration keeps a key in the OS
//!   credential store, the backend never sees a plain worker id. The marker's `worker`
//!   is its `hmac-sha256:` digest (comparable by anyone with the key, readable by no
//!   one) and a display label rides in the marker metadata as `enc:v1:`. A reader
//!   without the key sees [`UNREADABLE_HOLDER`]; it cannot tell its own claim from
//!   another's until the key is imported.
//!
//! An unbound store, or one bound to a `local` configuration, behaves exactly as before.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use hotsheet_model::{ClaimEvent, ClaimEventKind, Ticket, Timestamp, Ulid};

use crate::claim_backends::{BackendKind, Registries, git_store_sources};
use crate::claim_codec::{ClaimKey, KEY_LEN, MetadataCodec, MetadataCodecs, MetadataValue};
use crate::gitref_cas::{Acquired, CasError, GitRefCas, RefMarker, millis};
use crate::orchestration::OfflinePolicy;
use crate::{FsStore, StoreError};

/// The metadata key a claim's display label is stored under.
pub const LABEL_KEY: &str = "label";

/// Shown for a holder whose label this machine cannot read (the key is not here, or it
/// is another key).
pub const UNREADABLE_HOLDER: &str = "(holder unreadable: configuration key not available)";

/// How a gate encodes the holder fields it writes.
#[derive(Clone)]
pub enum HolderKey {
    /// The configuration names no credential: holder fields are written plain, as before.
    Plain,
    /// The configuration's key is available: worker ids are keyed digests.
    Keyed(std::sync::Arc<MetadataCodecs>),
    /// The configuration names a key that is not in the credential store. Nothing is
    /// written (a plain id must never reach the backend) and holders read as unreadable.
    Missing { configuration: String },
}

impl std::fmt::Debug for HolderKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plain => f.write_str("Plain"),
            Self::Keyed(_) => f.write_str("Keyed(..)"),
            Self::Missing { configuration } => write!(f, "Missing({configuration})"),
        }
    }
}

/// The claim backend a store's tickets go through.
#[derive(Debug, Clone)]
pub struct TicketGate {
    pub configuration: String,
    pub cas: GitRefCas,
    pub offline: OfflinePolicy,
    pub holder: HolderKey,
}

impl TicketGate {
    /// The `worker` field a marker for `worker` carries.
    pub fn worker_field(&self, worker: &str) -> Result<String, StoreError> {
        match &self.holder {
            HolderKey::Plain => Ok(worker.to_owned()),
            HolderKey::Keyed(codecs) => codecs
                .encode(MetadataCodec::HmacSha256, worker)
                .map_err(config_error),
            HolderKey::Missing { configuration } => Err(missing_key(configuration)),
        }
    }

    /// The metadata a marker for a claim labelled `label` carries.
    pub fn marker_metadata(&self, label: &str) -> Result<BTreeMap<String, String>, StoreError> {
        let mut metadata = BTreeMap::new();
        if let HolderKey::Keyed(codecs) = &self.holder {
            metadata.insert(
                LABEL_KEY.to_owned(),
                codecs
                    .encode(MetadataCodec::EncV1, label)
                    .map_err(config_error)?,
            );
        }
        Ok(metadata)
    }

    /// Whether a marker's `worker` field names the local `worker`. A plain marker (taken
    /// before the key existed, or by the distributed work loop) compares plain.
    pub fn owns(&self, marker_worker: &str, worker: &str) -> bool {
        match &self.holder {
            HolderKey::Keyed(codecs)
                if marker_worker.starts_with(crate::claim_codec::HMAC_PREFIX) =>
            {
                codecs.hmac_matches(marker_worker, worker)
            }
            _ => marker_worker == worker,
        }
    }

    /// The holder a marker names, for people: the plain worker, the decrypted label, or
    /// [`UNREADABLE_HOLDER`].
    pub fn holder_name(&self, marker: &RefMarker) -> String {
        if !marker.worker.starts_with(crate::claim_codec::HMAC_PREFIX) {
            return marker.worker.clone();
        }
        let HolderKey::Keyed(codecs) = &self.holder else {
            return UNREADABLE_HOLDER.to_owned();
        };
        match marker.metadata.get(LABEL_KEY).map(|v| codecs.decode(v)) {
            Some(Ok(MetadataValue::Decrypted(label))) => label,
            _ => UNREADABLE_HOLDER.to_owned(),
        }
    }
}

fn missing_key(configuration: &str) -> StoreError {
    config_error(format!(
        "no key for configuration '{configuration}' in the OS credential store; run `hotsheet-cli resource backend key {configuration} --generate` on the first machine, then `--export` there and `--import` (key on standard input) here"
    ))
}

/// A configuration key as stored: base64url of 32 bytes.
pub fn decode_key(stored: &str) -> Option<ClaimKey> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(stored.trim())
        .ok()?;
    let bytes: [u8; KEY_LEN] = bytes.try_into().ok()?;
    Some(ClaimKey::from_bytes(bytes))
}

fn holder_key(
    secrets: &dyn crate::secrets::SecretStore,
    configuration: &crate::claim_backends::BackendConfiguration,
) -> HolderKey {
    let Some(account) = &configuration.credential else {
        return HolderKey::Plain;
    };
    let key = secrets
        .get(account)
        .ok()
        .flatten()
        .and_then(|stored| decode_key(&stored))
        .and_then(|key| MetadataCodecs::new(&key).ok());
    key.map_or_else(
        || HolderKey::Missing {
            configuration: configuration.id.clone(),
        },
        |codecs| HolderKey::Keyed(std::sync::Arc::new(codecs)),
    )
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
    #[cfg(test)]
    if let Some(secrets) = tests::injected_secrets() {
        return ticket_gate_with(home, store, secrets.as_ref());
    }
    ticket_gate_with(home, store, &crate::secrets::OsKeychain::at(home))
}

/// [`ticket_gate_at`] reading configuration keys from `secrets` (tests inject one).
pub fn ticket_gate_with(
    home: &Path,
    store: &FsStore,
    secrets: &dyn crate::secrets::SecretStore,
) -> Result<Option<TicketGate>, StoreError> {
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
        // HS2-3SJ8JX routes ticket claims through every backend kind. Until then a
        // store bound to one must not silently claim without it.
        BackendKind::External | BackendKind::UpstashRedis => {
            return Err(config_error(format!(
                "ticket claims through a {} backend ('{}') are not supported yet",
                configuration.kind.as_str(),
                configuration.id
            )));
        }
        BackendKind::GitRef => {
            let repository = configuration
                .setting("repository")
                .map_or_else(|| store.root().to_path_buf(), PathBuf::from);
            let remote = configuration.setting("remote").unwrap_or("origin");
            Some(TicketGate {
                cas: GitRefCas::new(repository, remote),
                offline: configuration.offline,
                holder: holder_key(secrets, &configuration),
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
    let mut marker = RefMarker::new(
        &gate.worker_field(worker)?,
        &attempt.to_string(),
        now_ms,
        lease,
    );
    marker.metadata = gate.marker_metadata(ticket.worker_label.as_deref().unwrap_or(worker))?;
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
pub fn adopt_holder(gate: &TicketGate, ticket: &mut Ticket, holder: &RefMarker, now: &Timestamp) {
    let own = ticket
        .claimed_by
        .as_deref()
        .is_some_and(|local| gate.owns(&holder.worker, local));
    if own
        && ticket
            .claimed_by
            .as_deref()
            .and_then(|local| attempt_id(ticket, local))
            .map(|id| id.to_string())
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
    let name = gate.holder_name(holder);
    let label = (name != holder.worker).then_some(name);
    ticket.worker_label.clone_from(&label);
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
            worker_label: label,
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

/// The claim conflicts recorded and not yet acknowledged, oldest first, without forgetting them
/// (HS2-1639Q0): a conflict stays pending until the losing session has been told.
pub fn pending_conflicts(store: &FsStore) -> Vec<ClaimConflict> {
    git_path(store, CONFLICTS_FILE)
        .map(|path| read_json(&path))
        .unwrap_or_default()
}

/// HS2-ZR8B4N: how this machine sees a ticket's claim beside its frontmatter, on the ticket
/// and row wire (`claim_standing`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClaimStanding {
    /// The live claim was taken while the backend was unreachable and is not confirmed: it
    /// is advisory, and another machine may hold the ticket.
    Provisional,
    /// A claim this host's `worker` held was given to another machine; `holder` is the
    /// readable holder (its label, or [`UNREADABLE_HOLDER`]), never a keyed digest.
    Lost {
        worker: String,
        holder: String,
        detected_at: String,
    },
}

/// HS2-ZR8B4N: one read of a store's provisional claims and pending conflicts, so a list
/// marks every row from the same two files.
#[derive(Debug)]
pub struct ClaimStandings {
    provisional: BTreeMap<Ulid, ProvisionalClaim>,
    lost: BTreeMap<Ulid, ClaimConflict>,
    /// When the records were read: a provisional claim counts only while its lease is live.
    now: Timestamp,
}

impl ClaimStandings {
    /// Read both machine-local records of `store`. A store without them (no Git directory,
    /// no bound claim backend, nothing recorded) reads as empty.
    pub fn read(store: &FsStore) -> Self {
        Self::read_at(
            store,
            Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        )
    }

    /// [`Self::read`] as of `now`.
    pub fn read_at(store: &FsStore, now: Timestamp) -> Self {
        let read = |name: &'static str| store_git_path(store, name);
        let provisional: BTreeMap<Ulid, ProvisionalClaim> = read(PROVISIONAL_FILE)
            .map(|path| read_json(&path))
            .unwrap_or_default();
        let conflicts: Vec<ClaimConflict> = read(CONFLICTS_FILE)
            .map(|path| read_json(&path))
            .unwrap_or_default();
        // The newest conflict of a ticket wins.
        let lost = conflicts.into_iter().map(|c| (c.ticket, c)).collect();
        Self {
            provisional,
            lost,
            now,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.provisional.is_empty() && self.lost.is_empty()
    }

    /// The standing of one ticket's claim. A pending loss wins: the frontmatter already
    /// names the new holder. A provisional record counts only while the ticket's live
    /// claim is still that attempt's worker.
    pub fn of(
        &self,
        ticket: &str,
        claimed_by: Option<&str>,
        lease_live: bool,
    ) -> Option<ClaimStanding> {
        let id: Ulid = ticket.parse().ok()?;
        if let Some(conflict) = self.lost.get(&id) {
            return Some(ClaimStanding::Lost {
                worker: conflict.worker.clone(),
                holder: conflict.holder.clone(),
                detected_at: conflict.detected_at.as_str().to_owned(),
            });
        }
        let provisional = self.provisional.get(&id)?;
        (lease_live && claimed_by == Some(provisional.worker.as_str()))
            .then_some(ClaimStanding::Provisional)
    }

    /// Mark a list row (HS2-ZR8B4N).
    pub fn apply_row(&self, row: &mut crate::TicketRow) {
        if self.is_empty() {
            return;
        }
        let live = lease_live(row.claim_lease_expires_at.as_deref(), &self.now);
        row.claim_standing = self.of(&row.id, row.claimed_by.as_deref(), live);
    }

    /// Mark a full ticket (HS2-ZR8B4N).
    pub fn apply_ticket(&self, ticket: &mut crate::wire::ApiTicket) {
        if self.is_empty() {
            return;
        }
        let live = lease_live(ticket.claim_lease_expires_at.as_deref(), &self.now);
        ticket.claim_standing = self.of(&ticket.id, ticket.claimed_by.as_deref(), live);
    }
}

fn lease_live(expires: Option<&str>, now: &Timestamp) -> bool {
    expires.is_some_and(|at| {
        Timestamp::new(at).chronological_cmp(now) == Some(std::cmp::Ordering::Greater)
    })
}

/// [`git_path`] memoized per store root and file name: a list reads these records on every
/// request, and asking Git for its directory each time would cost a process per read. A
/// store's Git directory does not move while the server runs; a store that is not a Git
/// repository has no claim backend (HS2-ZR8B4N).
fn store_git_path(store: &FsStore, name: &'static str) -> Option<PathBuf> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Cache = Mutex<HashMap<(PathBuf, &'static str), Option<PathBuf>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = (store.root().to_path_buf(), name);
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().ok().and_then(|map| map.get(&key).cloned()) {
        return found;
    }
    let found = git_path(store, name);
    if let Ok(mut map) = cache.lock() {
        map.insert(key, found.clone());
    }
    found
}

/// Forget the conflicts recorded for `tickets`, after their losing sessions were told.
pub fn acknowledge_conflicts(store: &FsStore, tickets: &[Ulid]) {
    let Some(path) = git_path(store, CONFLICTS_FILE) else {
        return;
    };
    let mut conflicts: Vec<ClaimConflict> = read_json(&path);
    let before = conflicts.len();
    conflicts.retain(|c| !tickets.contains(&c.ticket));
    if conflicts.len() == before {
        return;
    }
    if conflicts.is_empty() {
        let _ = std::fs::remove_file(&path);
    } else {
        let _ = write_json(&path, &conflicts);
    }
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
                adopt_holder(&gate, &mut current, &holder, now);
                store.write_ticket_committing(&current)?;
                Ok::<_, StoreError>(())
            })?;
            clear_provisional(store, ticket.id);
            let conflict = ClaimConflict {
                ticket: ticket.id,
                slug: ticket.slug.clone(),
                worker,
                holder: gate.holder_name(&holder),
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
